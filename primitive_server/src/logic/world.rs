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

use primitive_shared::packed::PackedChunk;
use primitive_shared::types::{
    is_collidable, BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME,
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
    /// Kept packed, a section at a time (`primitive_shared::packed`).
    ///
    /// A flat chunk is 131 KB at the world's height of 256 and the cache
    /// holds up to `max_cached_chunks` of them. Measured flat: 224 MB for
    /// the 1793 chunks one player at render distance 24 is sent, and a
    /// gigabyte for a full default cache -- which in singleplayer is the
    /// same process as the client and its own copy of the same world. See
    /// `tests/chunk_cache_memory.rs`.
    chunk: Arc<PackedChunk>,
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

/// What runs over a freshly generated chunk before the player edits go
/// back on top of it.
///
/// **A boxed closure rather than the mod API's own function-pointer
/// type**, for one reason: this file is compiled with and without the
/// `mods` feature, and without it `primitive_modapi` does not exist at
/// all. The server installs one of these that reaches the loaded mods
/// (see `crate::install_chunk_decorators`); a build with no mod host
/// installs nothing and the `Option` stays `None`.
pub type Decorator = Box<dyn Fn(ChunkPos, u32, &mut [BlockId]) + Send + Sync>;

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
    /// Installed once, after the mods have loaded, and read on every
    /// generation from whatever thread is doing it.
    ///
    /// **An `RwLock` and not a `OnceLock`**: the client embeds this
    /// crate and starts and stops a world per singleplayer session, and
    /// a cell that could only ever be written once would be a cell the
    /// second world could not write. Read-uncontended in every case that
    /// matters -- one read per chunk, against a writer that runs once at
    /// startup.
    decorator: RwLock<Option<Decorator>>,
    /// How many edits there have ever been, and where the last
    /// [`RECENT_EDITS`] of them were: what a cache of something worked out
    /// *from* the blocks asks to find out whether it is stale
    /// ([`World::edited_since`]).
    ///
    /// **Here, at the one door every edit comes through**, and not a
    /// notification each mechanic that edits has to remember to send. The
    /// rooms (`logic::shelters`) were the first such cache, and the edits
    /// that change a room come from a player, a fire burning a wall
    /// through, a door swung, sand falling -- a cache invalidated by the
    /// paths that remember to invalidate it is a hut that stays sealed
    /// after the fire has eaten its wall.
    edit_serial: AtomicU64,
    recent_edits: std::sync::Mutex<std::collections::VecDeque<Edit>>,
}

/// How many edits [`World::edited_since`] can look back over. Past it, a
/// cache older than the oldest is told it is stale, which is always safe:
/// the cost of a wrong "stale" is one recomputation.
pub const RECENT_EDITS: usize = 1024;

/// One entry of the journal: the edit's serial and its cell.
type Edit = (u64, (i32, i32, i32));

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
        Self::with_preset(
            seed,
            primitive_shared::worldgen::Preset::Normal,
            max_cached_chunks,
        )
    }

    /// The same, for a world that was generated by something other than
    /// the ordinary terrain generator. See `worldgen::Preset`.
    pub fn with_preset(
        seed: u32,
        preset: primitive_shared::worldgen::Preset,
        max_cached_chunks: usize,
    ) -> Self {
        Self::with_zone(seed, preset, primitive_shared::worldgen::Zone::default(), max_cached_chunks)
    }

    /// The same, laid somewhere on the planet. See `worldgen::Zone`.
    pub fn with_zone(
        seed: u32,
        preset: primitive_shared::worldgen::Preset,
        zone: primitive_shared::worldgen::Zone,
        max_cached_chunks: usize,
    ) -> Self {
        Self::with_scale(seed, preset, zone, primitive_shared::worldgen::Scale::Landforms, max_cached_chunks)
    }

    /// The same, drawn at a scale: the Earth's for a new world, the
    /// regional one for a world made before it. See `worldgen::Scale`.
    pub fn with_scale(
        seed: u32,
        preset: primitive_shared::worldgen::Preset,
        zone: primitive_shared::worldgen::Zone,
        scale: primitive_shared::worldgen::Scale,
        max_cached_chunks: usize,
    ) -> Self {
        let mut shards = Vec::with_capacity(SHARD_COUNT);
        for _ in 0..SHARD_COUNT {
            shards.push(RwLock::new(Shard::default()));
        }
        let gen = WorldGen::with_scale(seed, preset, zone, scale);
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
            decorator: RwLock::new(None),
            edit_serial: AtomicU64::new(0),
            recent_edits: std::sync::Mutex::new(std::collections::VecDeque::with_capacity(RECENT_EDITS)),
        }
    }

    /// The count of every edit so far: take it with a computed answer, and
    /// hand it back to [`World::edited_since`] to ask whether the answer
    /// still holds.
    pub fn edit_serial(&self) -> u64 {
        self.edit_serial.load(Ordering::Acquire)
    }

    /// Has any cell from `low` to `high` (corners inclusive) been written
    /// since `serial`? `true` too when the answer is no longer known -- the
    /// edits since then have run off the end of [`RECENT_EDITS`].
    ///
    /// Nothing but an atomic load when nothing has been written, which on
    /// a quiet server is almost every time it is asked.
    pub fn edited_since(&self, serial: u64, low: (i32, i32, i32), high: (i32, i32, i32)) -> bool {
        if self.edit_serial() == serial {
            return false;
        }
        let recent = self.recent_edits.lock().unwrap_or_else(|e| e.into_inner());
        if recent.front().is_none_or(|&(oldest, _)| oldest > serial + 1) {
            return true;
        }
        recent.iter().rev().take_while(|&&(at, _)| at > serial).any(|&(_, (x, y, z))| {
            (low.0..=high.0).contains(&x) && (low.1..=high.1).contains(&y) && (low.2..=high.2).contains(&z)
        })
    }

    /// Hands terrain to something else before the edits go back on.
    ///
    /// Called once, after the mods are loaded, and replaces whatever was
    /// there. See `GenerationApi::register_decorator` for the contract a
    /// decorator is held to -- above all that it must be a pure function
    /// of the coordinates, the seed and the blocks it was given, because
    /// **a chunk is evicted and regenerated whenever the cache is
    /// full**, and a decorator that drew on anything else would produce
    /// a world that changes shape when you walk away from it and come
    /// back.
    pub fn set_decorator(&self, decorator: Decorator) {
        let mut slot = self.decorator.write().unwrap_or_else(|e| e.into_inner());
        *slot = Some(decorator);
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

    /// The surface height of a column, straight through to the
    /// generator. See `climate_at` for why these live here.
    pub fn height_at(&self, gx: i32, gz: i32) -> i32 {
        self.gen.height_at(gx, gz)
    }

    /// Which biome a column belongs to.
    pub fn biome_at(&self, gx: i32, gz: i32) -> primitive_shared::worldgen::Biome {
        self.gen.biome_at(gx, gz)
    }

    /// Whether the generator laid a cave lake in this cell, straight through
    /// for `biome_at`'s reason. What makes that water clean to drink is in
    /// `worldgen::cave_water`.
    pub fn cave_water_at(&self, gx: i32, gy: i32, gz: i32) -> bool {
        self.gen.cave_water_at(gx, gy, gz)
    }

    /// How good the ground is for growing things, in three grades.
    ///
    /// Straight through to the generator for the same reason
    /// `climate_at` is: it is a pure function of the seed and the place,
    /// and the world is the thing the server already hands around. See
    /// `worldgen::Fertility` for what decides it, and
    /// `logic::growth::Soil` for what reads it.
    pub fn fertility_at(&self, gx: i32, gz: i32) -> primitive_shared::worldgen::Fertility {
        self.gen.fertility_at(gx, gz)
    }

    /// The climate at a cell: how warm and how wet, both 0..1.
    ///
    /// Straight through to the generator, because that is where the two
    /// fields are and they are pure functions of the seed. Exposed here
    /// rather than by handing `logic::climate` a `WorldGen` because the
    /// world is what the server hands around, and one accessor is
    /// cheaper than threading a second reference through the tick loop.
    pub fn climate_at(&self, gx: i32, gy: i32, gz: i32) -> (f32, f32) {
        self.gen.climate_at(gx, gy, gz)
    }

    /// The latitude of a row of this world, which is how much of a winter
    /// it gets. See `WorldGen::latitude_degrees`, `season::seasonal_swing`.
    pub fn latitude_degrees(&self, gz: i32) -> Option<f32> {
        self.gen.latitude_degrees(gz)
    }

    /// Where on the planet this world is laid. Sent in the handshake
    /// beside the preset, for the preset's reason. See `worldgen::Zone`.
    pub fn zone(&self) -> primitive_shared::worldgen::Zone {
        self.gen.zone()
    }

    /// Which scale the country is drawn at. Sent in the handshake beside
    /// the zone, for the zone's reason. See `worldgen::Scale`.
    pub fn scale(&self) -> primitive_shared::worldgen::Scale {
        self.gen.scale()
    }

    /// Which generator made it. Sent in the handshake, because the
    /// client builds a generator of its own and the seed alone does not
    /// say which one to build.
    pub fn preset(&self) -> primitive_shared::worldgen::Preset {
        self.gen.preset()
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
    pub fn safe_position(&self, wanted: (f64, f64, f64)) -> (f64, f64, f64) {
        let (x, y, z) = wanted;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return primitive_shared::geometry::wide(self.spawn_point());
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
            Some(gy) => (x, f64::from(gy as f32 + SPAWN_CLEARANCE), z),
            None => primitive_shared::geometry::wide(self.spawn_point()),
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
    pub fn cached(&self, pos: ChunkPos) -> Option<Arc<PackedChunk>> {
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
        // **Between the generator and the overlay, and that order is the
        // whole of it.** A decorator that ran after the edits would
        // bulldoze whatever the player has built there since; one that
        // ran before the generator would have nothing to decorate. The
        // lock is let go before the edits are taken, so a decorator that
        // calls back into the host never meets a world lock this thread
        // is holding.
        {
            let decorator = self.decorator.read().unwrap_or_else(|e| e.into_inner());
            if let Some(decorate) = decorator.as_ref() {
                decorate(pos, self.gen.seed(), &mut chunk.blocks);
            }
        }
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
    pub fn insert(&self, chunk: Chunk) -> Arc<PackedChunk> {
        let pos = chunk.pos;
        // Packed before the shard lock is taken, not under it. Packing
        // walks every cell of the chunk, and this is the lock every block
        // read in the shard waits on -- item physics, water, the
        // anticheat. When another task published the same chunk first
        // the packing is thrown away, which is the rare race and a
        // fraction of a millisecond.
        let packed = PackedChunk::pack(&chunk);
        drop(chunk);
        let shard = &self.shards[Self::shard_index(pos)];
        let mut guard = shard.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.chunks.get(&pos) {
            existing.last_access.store(self.stamp(), Ordering::Relaxed);
            return Arc::clone(&existing.chunk);
        }
        // **The overlay again, under the shard's lock.** `generate` read it
        // before this chunk existed anywhere, and an edit that landed in
        // between -- overlay written, then no cached chunk for `set_block`
        // to update -- was lost from the cache: the chunk went in as the
        // generator drew it, and every read said grass where water had been
        // placed. A scenario that filled a river on a field hit it in one full
        // run of two. Read here, a `set_block` is either already in the
        // overlay or takes this lock after us and finds the chunk cached.
        let mut packed = packed;
        {
            let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
            if let Some(chunk_edits) = edits.get(&pos) {
                for (&index, &block) in chunk_edits {
                    let index = index as usize;
                    if index < CHUNK_VOLUME {
                        // `Chunk::index` undone: x fastest, then z, then y.
                        let (x, z, y) = (index % CHUNK_SIZE_X, index / CHUNK_SIZE_X % CHUNK_SIZE_Z, index / (CHUNK_SIZE_X * CHUNK_SIZE_Z));
                        if packed.get(x, y, z) != block {
                            packed.set(x, y, z, block);
                        }
                    }
                }
            }
        }
        let arc = Arc::new(packed);
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
        {
            // Under the journal's lock, so a serial and the entry that
            // carries it are never seen apart.
            let mut recent = self.recent_edits.lock().unwrap_or_else(|e| e.into_inner());
            let serial = self.edit_serial.fetch_add(1, Ordering::AcqRel) + 1;
            if recent.len() == RECENT_EDITS {
                recent.pop_front();
            }
            recent.push_back((serial, (gx, gy, gz)));
        }

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

    /// Whether anything has ever been written to this cell since the
    /// world was generated -- by a player, a fire, falling sand, or a
    /// ruin chest being opened (see `unseal_ruin_chest`, which is what
    /// asks). An edit that wrote the same block back still counts: that
    /// is the whole use of it.
    pub fn is_edited(&self, gx: i32, gy: i32, gz: i32) -> bool {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return false;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let index = Chunk::index(lx, gy as usize, lz) as u32;
        let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
        edits.get(&pos).is_some_and(|chunk| chunk.contains_key(&index))
    }

    /// The generator this world's terrain comes from, for a question
    /// only it can answer -- what a ruin's chest was left holding.
    pub fn generator(&self) -> &WorldGen {
        &self.gen
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
        {
            let mut edits = self.edits.write().unwrap_or_else(|e| e.into_inner());
            for (pos, blocks) in save.edits {
                let entry = edits.entry(pos).or_default();
                for (index, block) in blocks {
                    entry.insert(index, block);
                    count += 1;
                }
            }
        }
        let framed = self.frame_lone_racks();
        if framed > 0 {
            println!("[world] {framed} lone drying rack(s) from an older save are hide frames now");
        }
        let cleared = self.clear_old_lean_tos();
        if cleared > 0 {
            println!("[world] {cleared} cell(s) of two-cell lean-tos from an older build are gone");
        }
        Ok(count)
    }

    /// **A lean-to of two cells, from a build before the hut had fifteen, is
    /// taken down** as the world is read. Answers how many cells.
    ///
    /// Its two cells read now as the hut's mouth and middle (`lean_to::PARTS`),
    /// and the middle draws the whole hut: fifteen cells of thatch over two
    /// that collide, a roof a player walks through. Growing it into fifteen
    /// is the rack's rejected answer again -- a tent put up against a wall
    /// has no room to grow -- and a one-night shelter is worth less than the
    /// question: it would have fallen in the next morning anyway. Only edits
    /// are asked, because nothing generates a lean-to.
    fn clear_old_lean_tos(&self) -> usize {
        use primitive_shared::lean_to;
        let mut old = Vec::new();
        {
            let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
            let at_of = |pos: &ChunkPos, index: u32| {
                let index = index as usize;
                let x = index % CHUNK_SIZE_X;
                let z = (index / CHUNK_SIZE_X) % CHUNK_SIZE_Z;
                let y = index / (CHUNK_SIZE_X * CHUNK_SIZE_Z);
                (pos.x * CHUNK_SIZE_X as i32 + x as i32, y as i32, pos.z * CHUNK_SIZE_Z as i32 + z as i32)
            };
            let edited = |cell: (i32, i32, i32)| {
                if cell.1 < 0 || cell.1 as usize >= CHUNK_SIZE_Y {
                    return None;
                }
                let (pos, lx, lz) = ChunkPos::from_global(cell.0, cell.2);
                edits.get(&pos).and_then(|chunk| chunk.get(&(Chunk::index(lx, cell.1 as usize, lz) as u32))).copied()
            };
            for (pos, cells) in edits.iter() {
                for (&index, &block) in cells {
                    let at = at_of(pos, index);
                    if lean_to::is_lean_to(block) && !lean_to::whole(at, block, edited) {
                        old.push(at);
                    }
                }
            }
        }
        for &(x, y, z) in &old {
            self.set_block(x, y, z, primitive_shared::types::BLOCK_AIR);
        }
        old.len()
    }

    /// **Every lone cell of a drying rack becomes a hide frame**, as the
    /// world is read. Answers how many.
    ///
    /// A save from before the rack of two by two -- or from before the hide
    /// frame came back -- holds racks of one cell, which were the frame in
    /// all but id (`types::BLOCK_HIDE_FRAME`). They become it here, with
    /// their facing and their skin bit (`RACK_LOADED` is the same bit on
    /// both), in the same cell: the container store and the rack file key by
    /// the cell, so whatever was laced in one is still in it and still
    /// drying, and nothing else has to be told.
    ///
    /// **Here, once, and not wherever a rack is next touched.** The lazy
    /// way was the first sketch -- rewrite a lone cell the next time the
    /// server looks at it -- and it leaves an untouched old rack to be
    /// broken as the big rack's item, opened at the wrong cell (a loaded
    /// lone cell carries `RACK_TOP`, and `rack_anchor` reads that as "the
    /// cell under me"), and taking larder goods it was never built for.
    ///
    /// **Whole is asked of the world as it will be**, edits over terrain:
    /// a rack a player built is four edits, but the test world's generator
    /// draws whole racks of its own, and a cell of one of those that a
    /// player's goods rewrote is an edit whose partners are terrain. The
    /// terrain is generated only for a rack cell that needs it -- a handful
    /// of chunks in the rare save that has one -- and kept for the pass.
    fn frame_lone_racks(&self) -> usize {
        use primitive_shared::types::{
            block_kind, rack_whole, BLOCK_DRYING_RACK, BLOCK_HIDE_FRAME, KIND_MASK,
        };
        // A cell: `rack_whole` asks through an `Fn`, and the terrain it fills
        // on the way is a cache, not a result.
        let terrain: std::cell::RefCell<HashMap<ChunkPos, Chunk>> = std::cell::RefCell::new(HashMap::new());
        let mut lone = Vec::new();
        {
            let edits = self.edits.read().unwrap_or_else(|e| e.into_inner());
            let racks: Vec<((i32, i32, i32), BlockId)> = edits
                .iter()
                .flat_map(|(pos, cells)| {
                    cells.iter().filter(|(_, &block)| block_kind(block) == BLOCK_DRYING_RACK).map(move |(&index, &block)| {
                        let index = index as usize;
                        let x = index % CHUNK_SIZE_X;
                        let z = (index / CHUNK_SIZE_X) % CHUNK_SIZE_Z;
                        let y = index / (CHUNK_SIZE_X * CHUNK_SIZE_Z);
                        let at = (
                            pos.x * CHUNK_SIZE_X as i32 + x as i32,
                            y as i32,
                            pos.z * CHUNK_SIZE_Z as i32 + z as i32,
                        );
                        (at, block)
                    })
                })
                .collect();
            for (at, block) in racks {
                let whole = rack_whole(at, block, |cell| {
                    if cell.1 < 0 || cell.1 as usize >= CHUNK_SIZE_Y {
                        return None;
                    }
                    let (pos, lx, lz) = ChunkPos::from_global(cell.0, cell.2);
                    let index = Chunk::index(lx, cell.1 as usize, lz);
                    if let Some(&edited) = edits.get(&pos).and_then(|chunk| chunk.get(&(index as u32))) {
                        return Some(edited);
                    }
                    let mut terrain = terrain.borrow_mut();
                    let chunk = terrain.entry(pos).or_insert_with(|| self.gen.generate_chunk(pos));
                    chunk.blocks.get(index).copied()
                });
                if !whole {
                    lone.push((at, block));
                }
            }
        }
        for &(at, block) in &lone {
            // The kind swapped and every bit above it kept: the facing and
            // the skin. `RACK_FAR` and the goods bits are never set on a
            // lone cell -- `rack_goods` is only written on a whole rack.
            self.set_block(at.0, at.1, at.2, (block & !KIND_MASK) | BLOCK_HIDE_FRAME);
        }
        lone.len()
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

    /// Straight from the generator: a pure function of the seed and the
    /// column, so it costs noise and no lock. The spawner asks it once per
    /// attempt -- see `BlockWorld::biome`.
    fn biome(&self, gx: i32, gz: i32) -> Option<primitive_shared::worldgen::Biome> {
        Some(self.biome_at(gx, gz))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_GLOWSTONE};

    /// **An edit made while its chunk was being generated is not lost.** The
    /// generator read the overlay before the edit, the edit found no cached
    /// chunk to update, and the chunk went in as drawn -- which is what put
    /// grass back where a scenario had placed water. `insert` reads the
    /// overlay again.
    #[test]
    fn an_edit_made_while_its_chunk_was_generating_is_in_the_chunk_that_goes_in() {
        let world = World::new(99, 64);
        let pos = ChunkPos::new(2, 3);
        let (x, y, z) = (2 * 16 + 5, 40, 3 * 16 + 7);
        let stale = world.generate(pos);
        assert!(world.set_block(x, y, z, BLOCK_GLOWSTONE));
        world.insert(stale);
        assert_eq!(world.cached_block(x, y, z), Some(BLOCK_GLOWSTONE), "the edit was lost to the chunk generated under it");
    }

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

    /// **The cache keeps a chunk packed, and it is still the chunk the
    /// generator made, cell for cell.** The server hands chunks from here
    /// to every player and reads them for water, items and the anticheat,
    /// so a packing that lost a cell would be a world the server and its
    /// clients disagree about. And the saving has to be real: a flat
    /// chunk was 131 KB in this cache whatever was in it.
    #[test]
    fn the_cache_keeps_a_chunk_packed_and_answers_for_every_cell_as_generated() {
        let world = World::new(4242, 64);
        let pos = ChunkPos::new(0, 0);
        let generated = world.generate(pos);
        let flat = generated.blocks.clone();
        let cached = world.insert(generated);
        assert_eq!(cached.to_blocks(), flat, "the cache changed the chunk");
        let flat_bytes = flat.len() * std::mem::size_of::<BlockId>();
        assert!(
            cached.heap_bytes() * 4 < flat_bytes,
            "a cached chunk keeps {} bytes against {flat_bytes} flat",
            cached.heap_bytes()
        );

        // ...and an edit reaches the packed copy the way it reached the
        // flat one, high up where the sections are sky.
        assert!(world.set_block(5, 200, 7, BLOCK_GLOWSTONE));
        assert_eq!(world.cached_block(5, 200, 7), Some(BLOCK_GLOWSTONE));
        assert_eq!(world.cached_block(5, 201, 7), Some(BLOCK_AIR));
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
    fn an_old_lone_rack_loads_as_a_hide_frame_and_a_whole_rack_stays_the_larder() {
        use primitive_shared::types::{
            block_facing, block_kind, faced, rack_cells, rack_is_loaded, rack_with_hide, Facing, BLOCK_DRYING_RACK,
            BLOCK_HIDE_FRAME,
        };
        let dir = std::env::temp_dir().join(format!("primitive_test_frames_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let a = World::new(7, 128);
        // A lone rack with a skin in it, as a save from before the rack of
        // two by two has one -- and one without.
        let skinned = rack_with_hide(faced(BLOCK_DRYING_RACK, Facing::East), true);
        a.set_block(40, 200, 40, skinned);
        a.set_block(44, 200, 40, faced(BLOCK_DRYING_RACK, Facing::West));
        // ...and a whole rack, four cells.
        for ((x, y, z), cell) in rack_cells((50, 200, 50), Facing::North) {
            a.set_block(x, y, z, cell);
        }
        a.save(&dir).unwrap();

        let b = World::new(7, 128);
        b.load(&dir).unwrap();
        for at in [(40, 200, 40), (50, 200, 50)] {
            b.insert(b.generate(ChunkPos::from_global(at.0, at.2).0));
        }
        let frame = b.cached_block(40, 200, 40).unwrap();
        assert_eq!(block_kind(frame), BLOCK_HIDE_FRAME, "a lone rack loaded as {frame}");
        assert!(rack_is_loaded(frame), "the skin in the frame was lost on the way");
        assert_eq!(block_facing(frame), Facing::East, "the frame turned round");
        assert_eq!(block_kind(b.cached_block(44, 200, 40).unwrap()), BLOCK_HIDE_FRAME);
        for ((x, y, z), cell) in rack_cells((50, 200, 50), Facing::North) {
            assert_eq!(b.cached_block(x, y, z), Some(cell), "a cell of a whole rack changed");
        }
        // ...and it is written back, so the next start has nothing to do.
        assert!(b.has_unsaved_changes(), "the frames would be framed again every start");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A two-cell lean-to from an older build is taken down on load, and a
    /// whole hut is left standing.** The old tent's two cells are the hut's
    /// mouth and middle to the rules now, and the middle would draw fifteen
    /// cells of thatch over the two that collide.
    #[test]
    fn an_old_two_cell_lean_to_is_taken_down_on_load_and_a_whole_one_stands() {
        use primitive_shared::types::{bed_half_of, Facing, BLOCK_AIR, BLOCK_LEAN_TO};
        let dir = std::env::temp_dir().join(format!("primitive_test_lean_to_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let a = World::new(7, 128);
        a.set_block(40, 200, 40, bed_half_of(BLOCK_LEAN_TO, Facing::South, false));
        a.set_block(40, 200, 39, bed_half_of(BLOCK_LEAN_TO, Facing::South, true));
        let hut = primitive_shared::lean_to::cells((60, 200, 60), Facing::East);
        for ((x, y, z), cell) in hut {
            a.set_block(x, y, z, cell);
        }
        a.save(&dir).unwrap();

        let b = World::new(7, 128);
        b.load(&dir).unwrap();
        for at in [(40, 200, 40), (60, 200, 60)] {
            b.insert(b.generate(ChunkPos::from_global(at.0, at.2).0));
        }
        assert_eq!(b.cached_block(40, 200, 40), Some(BLOCK_AIR), "the old tent's foot stayed");
        assert_eq!(b.cached_block(40, 200, 39), Some(BLOCK_AIR), "the old tent's head stayed");
        for ((x, y, z), cell) in hut {
            assert_eq!(b.cached_block(x, y, z), Some(cell), "a cell of a whole hut changed");
        }
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

        let (sx, sy, sz) = world.safe_position((f64::from(x), f64::from(y), f64::from(z)));
        assert_eq!((sx, sz), (f64::from(x), f64::from(z)), "it moved them off their own column");
        for offset in 0..PLAYER_CELLS {
            let block = world.cached_block(gx, sy as i32 + offset, gz).unwrap();
            assert!(
                !is_collidable(block),
                "came back inside {block:#x} at y = {}",
                sy as i32 + offset
            );
        }
        assert!(sy > f64::from(y), "it put them below the pillar rather than on it");
    }

    #[test]
    fn a_position_that_is_still_clear_is_left_exactly_alone() {
        // The common case by an enormous margin, and it has to be
        // untouched: a player who logs out on a ledge and is nudged up
        // a block every time would climb the world one session at a
        // time.
        let world = World::new(77, 256);
        let spawn = world.spawn_point();
        assert_eq!(world.safe_position(primitive_shared::geometry::wide(spawn)), primitive_shared::geometry::wide(spawn));

        // ...including well up in the air, which is a legal place to be
        // and not this function's business to correct.
        let flying = (spawn.0, spawn.1 + 20.0, spawn.2);
        assert_eq!(world.safe_position(primitive_shared::geometry::wide(flying)), primitive_shared::geometry::wide(flying));
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
        let answer = world.safe_position((f64::from(gx as f32 + 0.5), 30.0, f64::from(gz as f32 + 0.5)));
        assert_eq!(answer, primitive_shared::geometry::wide(world.spawn_point()));
    }

    #[test]
    fn nonsense_coordinates_are_refused_rather_than_believed() {
        let world = World::new(99, 256);
        for bad in [
            (f32::NAN, 40.0, 0.0),
            (0.0, f32::INFINITY, 0.0),
            (0.0, 40.0, f32::NAN),
        ] {
            assert_eq!(world.safe_position(primitive_shared::geometry::wide(bad)), primitive_shared::geometry::wide(world.spawn_point()));
        }
    }

    #[test]
    fn missing_save_file_is_not_an_error() {
        let world = World::new(1, 64);
        let dir = std::env::temp_dir().join("primitive_definitely_missing_dir");
        assert_eq!(world.load(&dir).unwrap(), 0);
    }
}
