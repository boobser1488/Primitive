//! Authoritative world state. Per the plan: "Никогда не доверяйте
//! клиенту" -- this module, and only this module, decides what the world
//! actually contains.
//!
//! Three design choices here are what make it survivable at high player
//! counts:
//!
//! 1. **Sharded locking.** One global `Mutex<World>` serialises every
//!    chunk read from every player: with 200 players walking around,
//!    that lock *is* the server. Chunks are spread over N independent
//!    shards keyed by position hash, so unrelated chunk accesses proceed
//!    in parallel and contention is roughly 1/N.
//!
//! 2. **Edits live in an overlay, not in the chunk.** Player edits are
//!    stored as a sparse `chunk -> (index -> block)` map, separate from
//!    the generated terrain. Because generation is deterministic, an
//!    evicted chunk can be regenerated and the overlay reapplied to get
//!    a byte-identical result. That's what lets the cache be a *cache*:
//!    RAM is bounded by `max_cached_chunks`, not by how much of the world
//!    players have walked over. It also makes saving cheap -- we persist
//!    a few thousand changed blocks, not a few gigabytes of noise output
//!    that we can recompute for free.
//!
//! 3. **No locks held across `.await`.** Every method here is
//!    synchronous and short. Chunk generation (the genuinely expensive
//!    part) happens outside every lock, on a blocking worker thread; see
//!    `connection::chunk_pump`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use primitive_shared::types::{
    is_collidable, BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_Y, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

/// Number of independent lock shards. Powers of two only (we mask).
const SHARD_COUNT: usize = 32;

/// How many whole cells of clear air a standing player needs.
///
/// The collider is 1.8 tall, so it spans two cells wherever its feet
/// are: rounding up rather than dividing is the point, because a player
/// standing at the bottom of a cell still has their head in the one
/// above it.
const PLAYER_CELLS: i32 = 2;

/// How far above the floor a spawning player is put.
///
/// A hair, not nothing. Landing exactly on a surface leaves the collider
/// and the floor touching, and the first frame's overlap test then has
/// to decide what "exactly touching" means with numbers that have been
/// rounded twice -- the same reason the client's collider keeps a
/// contact skin.
const SPAWN_CLEARANCE: f32 = 0.1;
const SAVE_FORMAT_VERSION: u32 = 1;

struct CachedChunk {
    chunk: Arc<Chunk>,
    /// When this chunk was last read, as milliseconds since the world
    /// was created.
    ///
    /// **Atomic so that reading a block is a read lock.** Touching an
    /// `Instant` field means taking the shard's *write* lock, which is
    /// to say that every block lookup on the server serialised against
    /// every other lookup in the same shard -- for the sake of an LRU
    /// timestamp nobody reads until the shard is full. That was a
    /// slow-burning cost with only players walking about; it stops being
    /// one the moment a mechanic reads the world (see `logic::water`,
    /// which asks about six cells per queued cell, hundreds of cells a
    /// second, from the tick loop).
    ///
    /// Milliseconds rather than an `Instant` because there is no atomic
    /// `Instant`, and millisecond resolution is far finer than an
    /// eviction policy that measures ages in minutes.
    last_access: AtomicU64,
}

#[derive(Default)]
struct Shard {
    chunks: HashMap<ChunkPos, CachedChunk>,
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    seed: u32,
    /// (chunk, [(flat block index, block id)])
    edits: Vec<(ChunkPos, Vec<(u32, BlockId)>)>,
}

pub struct World {
    shards: Vec<RwLock<Shard>>,
    /// Sparse player edits, applied on top of generated terrain.
    edits: RwLock<HashMap<ChunkPos, HashMap<u32, BlockId>>>,
    gen: WorldGen,
    /// Where players are put down. Not the origin: with real oceans, a
    /// good share of seeds have deep water there. See
    /// `WorldGen::spawn_column`.
    spawn_column: (i32, i32),
    max_cached_chunks: usize,
    dirty: AtomicU64,
    generated_total: AtomicU64,
    evicted_total: AtomicU64,
    /// What `CachedChunk::last_access` counts from.
    epoch: Instant,
    /// `stamp()` as of the last `refresh_clock` call. `cached()` is the
    /// funnel for every block read the tick loop does -- item physics,
    /// falling sand, anticheat ground checks -- tens of thousands of
    /// calls a tick, and a clock read per call is measurable where a
    /// relaxed atomic load is not. The tick loop refreshes this once per
    /// tick, which is fifty times finer than an eviction policy that
    /// measures ages in minutes needs.
    coarse_now: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
pub struct WorldStats {
    pub cached_chunks: usize,
    pub edited_chunks: usize,
    pub edited_blocks: usize,
    pub generated_total: u64,
    pub evicted_total: u64,
    pub unsaved_edits: u64,
}

impl World {
    pub fn new(seed: u32, max_cached_chunks: usize) -> Self {
        let mut shards = Vec::with_capacity(SHARD_COUNT);
        for _ in 0..SHARD_COUNT {
            shards.push(RwLock::new(Shard::default()));
        }
        let gen = WorldGen::new(seed);
        // Found once, at startup: the search walks outwards over a few
        // thousand columns, and every join and every respawn asks for
        // the same answer.
        let spawn_column = gen.spawn_column();
        Self {
            shards,
            edits: RwLock::new(HashMap::new()),
            spawn_column,
            gen,
            max_cached_chunks,
            dirty: AtomicU64::new(0),
            generated_total: AtomicU64::new(0),
            evicted_total: AtomicU64::new(0),
            epoch: Instant::now(),
            coarse_now: AtomicU64::new(0),
        }
    }

    /// Refresh the coarse clock `cached()` stamps accesses with. Called
    /// once per tick; cheap enough to call anywhere else that wants
    /// fresher eviction stamps.
    pub fn refresh_clock(&self) {
        self.coarse_now.store(self.stamp(), Ordering::Relaxed);
    }

    /// Now, in the units `CachedChunk::last_access` is kept in.
    #[inline]
    fn stamp(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    pub fn seed(&self) -> u32 {
        self.gen.seed()
    }

    /// Where to put a player down, in the world as it actually is.
    ///
    /// **This used to ask the generator, and the generator does not know
    /// what is there.** `spawn_y` returns one block above the *terrain
    /// height* -- the shape of the ground before anything was put on it
    /// or built on it -- so a player arrived inside whatever was
    /// occupying that cell. Three ways that happens, and all three are
    /// ordinary rather than exotic:
    ///
    /// * **A tree grew at the spawn column.** Worldgen plants trees
    ///   after it decides the height, so the trunk starts exactly at the
    ///   height the spawn point was computed from. Every player of that
    ///   seed spawns inside a tree, for ever.
    /// * **Somebody built there.** The spawn point is the one place in a
    ///   world that everybody passes through, so it is the one place
    ///   most likely to have a shelter on it -- and the edit overlay is
    ///   invisible to the generator by design.
    /// * **Somebody dug there**, and the answer was a cell of air with a
    ///   hole under it rather than a floor.
    ///
    /// Being inside a block is not a cosmetic problem. The collider
    /// answers "may I move there" and every direction out of a block you
    /// are already in is blocked by that same block, so a player who
    /// spawns inside one is welded in place -- and dying does not help,
    /// because they respawn in exactly the same cell.
    ///
    /// So the column is *read*, and the first place the player fits with
    /// a floor under them is the answer.
    pub fn spawn_point(&self) -> (f32, f32, f32) {
        let (gx, gz) = self.spawn_column;
        (
            gx as f32 + 0.5,
            self.standing_height(gx, gz),
            gz as f32 + 0.5,
        )
    }

    /// A position the player will not be standing inside something at.
    ///
    /// **The place a player logged out of is not somewhere they can
    /// necessarily log back in.** The world moves underneath a saved
    /// position: somebody builds where they were standing, sand falls on
    /// it, water rises over it -- or the generator itself changes, and a
    /// tree that was not there in the version they left grows exactly
    /// where they were.
    ///
    /// The profile already promised to guard against this and only
    /// checked that the coordinates were finite and inside the world's
    /// height, which catches a corrupt file and nothing else. It could
    /// not do better on its own: whether a position is *inside* anything
    /// is a question about the world, and the profile store has never
    /// seen one.
    ///
    /// So the world answers it. The column is kept -- coming back a
    /// thousand blocks from where you left is far worse than coming back
    /// two metres higher -- and only the height moves. A column with no
    /// room in it at all gives up and returns the spawn point, which is
    /// the one position this server guarantees.
    pub fn safe_position(&self, wanted: (f32, f32, f32)) -> (f32, f32, f32) {
        let (x, y, z) = wanted;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return self.spawn_point();
        }
        let (gx, gz) = (x.floor() as i32, z.floor() as i32);
        let column = self.read_column(gx, gz);
        let feet = y.floor() as i32;

        // Already fine: the overwhelmingly common case, and it has to
        // stay free of any nudging. A player who logs out on a ledge and
        // is put back a block higher every time would climb the world.
        if (0..CHUNK_SIZE_Y as i32).contains(&feet) && Self::room_at(&column, feet) {
            return wanted;
        }

        match Self::standing_in(&column, feet.max(0)) {
            Some(gy) => (x, gy as f32 + SPAWN_CLEARANCE, z),
            None => self.spawn_point(),
        }
    }

    /// Every block of one column, read once.
    fn read_column(&self, gx: i32, gz: i32) -> Vec<BlockId> {
        (0..CHUNK_SIZE_Y as i32)
            .map(|gy| self.block_or_generate(gx, gy, gz))
            .collect()
    }

    /// Is there room for a standing player with their feet here?
    fn room_at(column: &[BlockId], feet: i32) -> bool {
        (0..PLAYER_CELLS).all(|d| {
            let gy = feet + d;
            match column.get(gy as usize) {
                Some(&block) => !is_collidable(block),
                // Above the sky is room; below the floor is not.
                None => gy >= 0,
            }
        })
    }

    /// The lowest height at or above `from` where a player fits *and*
    /// has something to stand on, or -- failing that -- merely fits.
    ///
    /// Two passes, because those are different questions and only the
    /// second always has an answer. A player over a hole should be
    /// dropped into it rather than refused a position.
    fn standing_in(column: &[BlockId], from: i32) -> Option<i32> {
        let top = CHUNK_SIZE_Y as i32 - PLAYER_CELLS;
        let solid = |gy: i32| {
            column
                .get(gy.clamp(0, CHUNK_SIZE_Y as i32 - 1) as usize)
                .is_some_and(|&block| is_collidable(block))
        };
        for gy in from..top {
            if Self::room_at(column, gy) && (gy == 0 || solid(gy - 1)) {
                return Some(gy);
            }
        }
        (from..top).find(|&gy| Self::room_at(column, gy))
    }

    /// The lowest height at this column a player fits at, at or above
    /// where the ground is.
    ///
    /// Searched **upward from the generator's answer** rather than
    /// downward from the sky. Both find somewhere legal; only one of
    /// them finds somewhere sensible. From above, the first floor is the
    /// roof of whatever is there -- so a shelter built over the spawn
    /// point puts everybody on its roof, and a tree puts them on the
    /// canopy. From below, they end up on the ground where the ground is
    /// clear and on top of the obstruction only when there is one.
    ///
    /// Two passes, because "fits" and "fits, standing on something" are
    /// different questions and only the first of them always has an
    /// answer. A player over a hole should be dropped into it rather
    /// than refused a spawn.
    fn standing_height(&self, gx: i32, gz: i32) -> f32 {
        // The column, read once. A respawn is not a hot path, but it
        // does happen while the tick loop is holding things, and the
        // difference between one chunk lookup and thirty is free.
        let column = self.read_column(gx, gz);
        let ground = (self.gen.spawn_y(gx, gz) as i32).max(0);
        match Self::standing_in(&column, ground) {
            Some(gy) => gy as f32 + SPAWN_CLEARANCE,
            // Solid to the sky. Nothing here is a good answer; the
            // generator's is at least the one every other part of the
            // server agrees about.
            None => self.gen.spawn_y(gx, gz) + SPAWN_CLEARANCE,
        }
    }

    /// A block from the cache, generating the chunk if it is not there.
    ///
    /// The spawn column is exactly the place that may *not* be cached --
    /// the first player to join has not been sent anything yet -- and a
    /// spawn point that silently reads air for an unloaded chunk is the
    /// bug this is here to fix, wearing a different hat.
    fn block_or_generate(&self, gx: i32, gy: i32, gz: i32) -> BlockId {
        if let Some(block) = self.cached_block(gx, gy, gz) {
            return block;
        }
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return BLOCK_AIR;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let chunk = self.insert(self.generate(pos));
        chunk.get(lx, gy as usize, lz)
    }

    #[inline]
    fn shard_index(pos: ChunkPos) -> usize {
        // Cheap spatial hash. Mixing both axes matters: `x % N` alone
        // would put a whole north-south corridor of chunks -- exactly what
        // one player walking in a straight line touches -- into one shard.
        let h = (pos.x as u32).wrapping_mul(0x9E37_79B1)
            ^ (pos.z as u32).wrapping_mul(0x85EB_CA6B);
        (h as usize) % SHARD_COUNT
    }

    /// Cached lookup only -- never generates. Used on the hot path and by
    /// the anti-cheat's ground check (which must not be able to trigger
    /// terrain generation, or a malicious client could make the server
    /// generate chunks at will).
    pub fn cached(&self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        let shard = &self.shards[Self::shard_index(pos)];
        let guard = shard.read().unwrap_or_else(|e| e.into_inner());
        let entry = guard.chunks.get(&pos)?;
        entry
            .last_access
            .store(self.coarse_now.load(Ordering::Relaxed), Ordering::Relaxed);
        Some(Arc::clone(&entry.chunk))
    }

    /// Generates a chunk *without touching any lock* except a brief read
    /// of the edit overlay. Safe (and intended) to call from a blocking
    /// worker thread.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = self.gen.generate_chunk(pos);
        let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
        if let Some(chunk_edits) = edits.get(&pos) {
            for (&index, &block) in chunk_edits {
                if (index as usize) < CHUNK_VOLUME {
                    chunk.blocks[index as usize] = block;
                }
            }
        }
        self.generated_total.fetch_add(1, Ordering::Relaxed);
        chunk
    }

    /// Publishes a freshly generated chunk. If another task generated the
    /// same chunk concurrently, that one wins -- generation is
    /// deterministic and the overlay is applied to both, so they're
    /// identical anyway, and keeping the existing `Arc` avoids
    /// invalidating handles other tasks are already holding.
    pub fn insert(&self, chunk: Chunk) -> Arc<Chunk> {
        let pos = chunk.pos;
        let shard = &self.shards[Self::shard_index(pos)];
        let mut guard = shard.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.chunks.get(&pos) {
            existing.last_access.store(self.stamp(), Ordering::Relaxed);
            return Arc::clone(&existing.chunk);
        }
        let arc = Arc::new(chunk);
        guard.chunks.insert(
            pos,
            CachedChunk {
                chunk: Arc::clone(&arc),
                last_access: AtomicU64::new(self.stamp()),
            },
        );
        self.evict_if_needed(&mut guard);
        arc
    }

    /// LRU eviction, per shard. Evicting is safe precisely because of the
    /// overlay: the chunk can be rebuilt on demand, edits and all.
    fn evict_if_needed(&self, shard: &mut Shard) {
        let per_shard_cap = (self.max_cached_chunks / SHARD_COUNT).max(8);
        if shard.chunks.len() <= per_shard_cap {
            return;
        }
        let overflow = shard.chunks.len() - per_shard_cap;
        let mut by_age: Vec<(ChunkPos, u64)> = shard
            .chunks
            .iter()
            .map(|(&pos, entry)| (pos, entry.last_access.load(Ordering::Relaxed)))
            .collect();
        by_age.sort_by_key(|&(_, t)| t);
        for (pos, _) in by_age.into_iter().take(overflow) {
            shard.chunks.remove(&pos);
            self.evicted_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Applies an authoritative block change. Returns false if the
    /// coordinate is out of the world's vertical range.
    ///
    /// The edit is recorded in the overlay *first*, so it survives the
    /// chunk being evicted a moment later.
    pub fn set_block(&self, gx: i32, gy: i32, gz: i32, block: BlockId) -> bool {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return false;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let index = Chunk::index(lx, gy as usize, lz) as u32;

        {
            let mut edits = self.edits.write().unwrap_or_else(|e| e.into_inner());
            edits.entry(pos).or_default().insert(index, block);
        }
        self.dirty.fetch_add(1, Ordering::Relaxed);

        // Update the cached copy if we have one. `Arc::make_mut` clones
        // only when another task is mid-send with the old version, which
        // is exactly the behaviour we want: no reader ever sees a chunk
        // mutate underneath it.
        let shard = &self.shards[Self::shard_index(pos)];
        let mut guard = shard.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.chunks.get_mut(&pos) {
            Arc::make_mut(&mut entry.chunk).set(lx, gy as usize, lz, block);
            *entry.last_access.get_mut() = self.stamp();
        }
        true
    }

    /// Block lookup that only consults the cache; `None` means "not
    /// loaded, and I'm not going to generate it to find out".
    pub fn cached_block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return None;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let chunk = self.cached(pos)?;
        Some(chunk.get(lx, gy as usize, lz))
    }

    pub fn stats(&self) -> WorldStats {
        let cached_chunks = self
            .shards
            .iter()
            .map(|s| s.read().unwrap_or_else(|e| e.into_inner()).chunks.len())
            .sum();
        let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
        WorldStats {
            cached_chunks,
            edited_chunks: edits.len(),
            edited_blocks: edits.values().map(|m| m.len()).sum(),
            generated_total: self.generated_total.load(Ordering::Relaxed),
            evicted_total: self.evicted_total.load(Ordering::Relaxed),
            unsaved_edits: self.dirty.load(Ordering::Relaxed),
        }
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.dirty.load(Ordering::Relaxed) > 0
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("edits.bin")
    }

    /// Writes the edit overlay to disk. Atomic: written to a temp file and
    /// renamed, so a crash mid-save can't leave a truncated world behind.
    pub fn save(&self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;

        let payload = {
            let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
            SaveFile {
                version: SAVE_FORMAT_VERSION,
                seed: self.gen.seed(),
                edits: edits
                    .iter()
                    .map(|(&pos, blocks)| {
                        let mut v: Vec<(u32, BlockId)> =
                            blocks.iter().map(|(&i, &b)| (i, b)).collect();
                        v.sort_unstable(); // stable file bytes for the same world
                        (pos, v)
                    })
                    .collect(),
            }
        };
        let block_count: usize = payload.edits.iter().map(|(_, v)| v.len()).sum();

        let bytes = bincode::serialize(&payload)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;

        self.dirty.store(0, Ordering::Relaxed);
        Ok(block_count)
    }

    /// Loads a previously saved overlay. A missing file is not an error
    /// (that's just a brand new world).
    pub fn load(&self, dir: &Path) -> std::io::Result<usize> {
        let path = Self::save_path(dir);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let save: SaveFile = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if save.version != SAVE_FORMAT_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "world save is format v{}, this server speaks v{}",
                    save.version, SAVE_FORMAT_VERSION
                ),
            ));
        }
        if save.seed != self.gen.seed() {
            eprintln!(
                "warning: world save was generated with seed {} but settings say {} -- \
                 terrain under existing edits will not match",
                save.seed,
                self.gen.seed()
            );
        }

        let mut count = 0;
        let mut edits = self.edits.write().unwrap_or_else(|e| e.into_inner());
        for (pos, blocks) in save.edits {
            let entry = edits.entry(pos).or_default();
            for (index, block) in blocks {
                entry.insert(index, block);
                count += 1;
            }
        }
        Ok(count)
    }
}

/// Lets the falling-block simulation read and write the world without
/// knowing about shards, caching or the edit overlay.
impl crate::logic::falling::BlockWorld for World {
    fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        self.cached_block(gx, gy, gz)
    }

    fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
        self.set_block(gx, gy, gz, block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_GLOWSTONE};

    #[test]
    fn edits_survive_eviction_and_regeneration() {
        // The whole point of the overlay: drop the chunk from RAM, ask for
        // it again, and the player's edit is still there.
        let world = World::new(99, 64);
        let pos = ChunkPos::new(2, 3);
        let generated = world.generate(pos);
        world.insert(generated);

        world.set_block(2 * 16 + 5, 30, 3 * 16 + 7, BLOCK_GLOWSTONE);
        assert_eq!(
            world.cached_block(2 * 16 + 5, 30, 3 * 16 + 7),
            Some(BLOCK_GLOWSTONE)
        );

        // Simulate eviction by regenerating from scratch.
        let regenerated = world.generate(pos);
        assert_eq!(regenerated.get(5, 30, 7), BLOCK_GLOWSTONE);
    }

    #[test]
    fn out_of_range_edits_are_rejected() {
        let world = World::new(1, 64);
        assert!(!world.set_block(0, -1, 0, BLOCK_AIR));
        assert!(!world.set_block(0, CHUNK_SIZE_Y as i32, 0, BLOCK_AIR));
        assert!(world.set_block(0, 10, 0, BLOCK_AIR));
    }

    #[test]
    fn cache_stays_bounded() {
        let world = World::new(5, 64);
        for x in 0..400 {
            let pos = ChunkPos::new(x, 0);
            let chunk = world.generate(pos);
            world.insert(chunk);
        }
        let stats = world.stats();
        // Per-shard cap is max(8, 64/32) = 8, so at most 32*8 = 256.
        assert!(
            stats.cached_chunks <= 256,
            "cache grew to {} chunks",
            stats.cached_chunks
        );
        assert!(stats.evicted_total > 0, "nothing was ever evicted");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("primitive_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let a = World::new(7, 128);
        a.set_block(10, 25, -30, BLOCK_GLOWSTONE);
        a.set_block(11, 25, -30, BLOCK_AIR);
        let saved = a.save(&dir).unwrap();
        assert_eq!(saved, 2);
        assert!(!a.has_unsaved_changes());

        let b = World::new(7, 128);
        let loaded = b.load(&dir).unwrap();
        assert_eq!(loaded, 2);
        let chunk = b.generate(ChunkPos::from_global(10, -30).0);
        b.insert(chunk);
        assert_eq!(b.cached_block(10, 25, -30), Some(BLOCK_GLOWSTONE));
        assert_eq!(b.cached_block(11, 25, -30), Some(BLOCK_AIR));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The cells the player's collider occupies at a spawn point.
    fn spawn_cells(world: &World) -> (i32, i32, i32, i32) {
        let (x, y, z) = world.spawn_point();
        (x.floor() as i32, y.floor() as i32, z.floor() as i32, y as i32)
    }

    #[test]
    fn a_player_never_spawns_inside_a_block() {
        // The bug: the spawn height came from the *generator*, which
        // knows the shape of the ground and nothing about what is
        // standing on it or has been built on it. Every one of these
        // used to put the player inside something.
        use primitive_shared::types::{is_collidable, BLOCK_STONE};

        for seed in [1u32, 7, 99, 1337, 40_000] {
            let world = World::new(seed, 256);
            let (gx, _, gz, _) = spawn_cells(&world);

            // A tower where the player is about to appear -- which is
            // what a shelter built at spawn looks like to this code, and
            // what a tree that grew there looks like too.
            let ground = world.spawn_point().1 as i32;
            for gy in ground..(ground + 6) {
                assert!(world.set_block(gx, gy, gz, BLOCK_STONE));
            }

            let (x, y, z) = world.spawn_point();
            let (cx, cz) = (x.floor() as i32, z.floor() as i32);
            for offset in 0..PLAYER_CELLS {
                let block = world.cached_block(cx, y as i32 + offset, cz).unwrap();
                assert!(
                    !is_collidable(block),
                    "seed {seed}: spawned inside {block:#x} at y = {}",
                    y as i32 + offset
                );
            }
            assert!(
                y as i32 >= ground + 6,
                "seed {seed}: spawned at {y}, below the top of the tower at {}",
                ground + 6
            );
        }
    }

    #[test]
    fn a_spawn_point_has_a_floor_under_it() {
        // Not merely "somewhere they fit": a spawn point in mid-air is a
        // fall, and a fall at spawn is fall damage before the player has
        // pressed anything.
        use primitive_shared::types::is_collidable;

        for seed in [3u32, 21, 500, 8_192] {
            let world = World::new(seed, 256);
            let (x, y, z) = world.spawn_point();
            let under = world
                .cached_block(x.floor() as i32, y as i32 - 1, z.floor() as i32)
                .unwrap();
            assert!(
                is_collidable(under),
                "seed {seed}: nothing to stand on under the spawn point"
            );
        }
    }

    #[test]
    fn a_hole_at_the_spawn_column_is_still_a_spawn_point() {
        // The other half: somebody digs out the spawn and there is no
        // floor to be found above the ground. Refusing to answer is not
        // an option -- a respawn has to go somewhere -- so the player is
        // put where they fit and allowed to fall.
        use primitive_shared::types::{is_collidable, BLOCK_AIR};

        let world = World::new(4242, 256);
        let (x, y, z) = world.spawn_point();
        let (gx, gz) = (x.floor() as i32, z.floor() as i32);
        for gy in 0..(y as i32 + 8) {
            world.set_block(gx, gy, gz, BLOCK_AIR);
        }

        let (_, y, _) = world.spawn_point();
        assert!(y >= 0.0, "the spawn point left the world");
        for offset in 0..PLAYER_CELLS {
            let block = world.cached_block(gx, y as i32 + offset, gz).unwrap();
            assert!(!is_collidable(block), "spawned inside {block:#x}");
        }
    }

    #[test]
    fn the_spawn_point_does_not_need_the_chunk_to_be_cached() {
        // The first player to join is the case: nothing has been sent to
        // anybody, so nothing is in the cache, and a lookup that reads
        // air for an uncached chunk would answer "the ground is at zero"
        // -- which is the same bug wearing a different hat.
        let world = World::new(2024, 256);
        assert_eq!(world.stats().cached_chunks, 0, "nothing should be cached yet");
        let (_, y, _) = world.spawn_point();
        assert!(
            y > 1.0,
            "spawned at {y}, which is what reading air for an unloaded chunk gives"
        );
    }

    #[test]
    fn logging_back_in_where_the_world_has_moved_does_not_bury_you() {
        // **The bug the screenshot showed.** A saved position is a place
        // in a world that has since changed -- somebody built there, or
        // the generator itself grew a tree where the player was standing
        // -- and the profile store cannot tell, because it has never
        // seen a world. Coming back inside a block welds the player in
        // place: every direction out of it is blocked by it.
        use primitive_shared::types::{is_collidable, BLOCK_STONE};

        let world = World::new(555, 256);
        let (x, y, z) = world.spawn_point();
        let (gx, gz) = (x.floor() as i32, z.floor() as i32);

        // Somebody built a solid pillar over where the player logged
        // out, which is what a new tree looks like to this code.
        for gy in (y as i32)..(y as i32 + 5) {
            assert!(world.set_block(gx, gy, gz, BLOCK_STONE));
        }

        let (sx, sy, sz) = world.safe_position((x, y, z));
        assert_eq!((sx, sz), (x, z), "it moved them off their own column");
        for offset in 0..PLAYER_CELLS {
            let block = world.cached_block(gx, sy as i32 + offset, gz).unwrap();
            assert!(
                !is_collidable(block),
                "came back inside {block:#x} at y = {}",
                sy as i32 + offset
            );
        }
        assert!(sy > y, "it put them below the pillar rather than on it");
    }

    #[test]
    fn a_position_that_is_still_clear_is_left_exactly_alone() {
        // The common case by an enormous margin, and it has to be
        // untouched: a player who logs out on a ledge and is nudged up
        // a block every time would climb the world one session at a
        // time.
        let world = World::new(77, 256);
        let spawn = world.spawn_point();
        assert_eq!(world.safe_position(spawn), spawn);

        // ...including well up in the air, which is a legal place to be
        // and not this function's business to correct.
        let flying = (spawn.0, spawn.1 + 20.0, spawn.2);
        assert_eq!(world.safe_position(flying), flying);
    }

    #[test]
    fn a_hopeless_column_falls_back_to_the_spawn_point() {
        use primitive_shared::types::BLOCK_STONE;

        let world = World::new(88, 256);
        let (x, _, z) = world.spawn_point();
        // Filled to the sky a long way from spawn: nothing in this
        // column is a position, so the answer has to come from
        // somewhere else.
        let (gx, gz) = (x.floor() as i32 + 300, z.floor() as i32 + 300);
        for gy in 0..CHUNK_SIZE_Y as i32 {
            world.set_block(gx, gy, gz, BLOCK_STONE);
        }
        let answer = world.safe_position((gx as f32 + 0.5, 30.0, gz as f32 + 0.5));
        assert_eq!(answer, world.spawn_point());
    }

    #[test]
    fn nonsense_coordinates_are_refused_rather_than_believed() {
        let world = World::new(99, 256);
        for bad in [
            (f32::NAN, 40.0, 0.0),
            (0.0, f32::INFINITY, 0.0),
            (0.0, 40.0, f32::NAN),
        ] {
            assert_eq!(world.safe_position(bad), world.spawn_point());
        }
    }

    #[test]
    fn missing_save_file_is_not_an_error() {
        let world = World::new(1, 64);
        let dir = std::env::temp_dir().join("primitive_definitely_missing_dir");
        assert_eq!(world.load(&dir).unwrap(), 0);
    }
}
