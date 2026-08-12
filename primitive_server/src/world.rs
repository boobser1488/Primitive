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
    BlockId, Chunk, ChunkPos, CHUNK_SIZE_Y, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

/// Number of independent lock shards. Powers of two only (we mask).
const SHARD_COUNT: usize = 32;
const SAVE_FORMAT_VERSION: u32 = 1;

struct CachedChunk {
    chunk: Arc<Chunk>,
    last_access: Instant,
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
    max_cached_chunks: usize,
    dirty: AtomicU64,
    generated_total: AtomicU64,
    evicted_total: AtomicU64,
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
        Self {
            shards,
            edits: RwLock::new(HashMap::new()),
            gen: WorldGen::new(seed),
            max_cached_chunks,
            dirty: AtomicU64::new(0),
            generated_total: AtomicU64::new(0),
            evicted_total: AtomicU64::new(0),
        }
    }

    pub fn seed(&self) -> u32 {
        self.gen.seed()
    }

    pub fn spawn_point(&self) -> (f32, f32, f32) {
        (0.5, self.gen.spawn_y(0, 0) + 0.1, 0.5)
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
        let mut guard = shard.write().unwrap_or_else(|e| e.into_inner());
        let entry = guard.chunks.get_mut(&pos)?;
        entry.last_access = Instant::now();
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
        if let Some(existing) = guard.chunks.get_mut(&pos) {
            existing.last_access = Instant::now();
            return Arc::clone(&existing.chunk);
        }
        let arc = Arc::new(chunk);
        guard.chunks.insert(
            pos,
            CachedChunk {
                chunk: Arc::clone(&arc),
                last_access: Instant::now(),
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
        let mut by_age: Vec<(ChunkPos, Instant)> = shard
            .chunks
            .iter()
            .map(|(&pos, entry)| (pos, entry.last_access))
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
            entry.last_access = Instant::now();
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

    #[test]
    fn missing_save_file_is_not_an_error() {
        let world = World::new(1, 64);
        let dir = std::env::temp_dir().join("primitive_definitely_missing_dir");
        assert_eq!(world.load(&dir).unwrap(), 0);
    }
}

/// Lets the falling-block simulation read and write the world without
/// knowing about shards, caching or the edit overlay.
impl crate::falling::BlockWorld for World {
    fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        self.cached_block(gx, gy, gz)
    }

    fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
        self.set_block(gx, gy, gz, block);
    }
}
