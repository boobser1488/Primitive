//! Этап 2: the client keeps only the chunks near the player, requests the
//! missing ones and drops the distant ones.
//!
//! Additions this pass:
//! - **Request retry.** The server now has bounded queues and *will* drop
//!   a chunk request under load. The old client asked for each chunk
//!   exactly once when it entered range, so a dropped request meant a
//!   permanent hole in the world you could fall through. Outstanding
//!   requests are now tracked with a timestamp and re-sent if the chunk
//!   hasn't arrived.
//! - **Neighbour access**, so the mesher can see across chunk boundaries
//!   for face culling and lighting.
//!
//! Still purely local bookkeeping -- it doesn't talk to the network
//! itself; `main.rs` sends whatever `update()` hands back.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use primitive_shared::lighting::BlockSource;
use primitive_shared::types::{is_collidable, BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_Y};

pub struct ChunkManager {
    loaded: HashMap<ChunkPos, Chunk>,
    /// Chunks that have arrived from the server but haven't been
    /// integrated yet (integration is budgeted -- see `integrate_chunks`
    /// in main.rs).
    ///
    /// They must count as satisfied here, otherwise the retry logic
    /// keeps asking for them while they sit in the queue, the server
    /// dutifully sends them again, and the backlog feeds itself. That
    /// bug turned a 169-chunk area into 800+ queued arrivals.
    arrived: HashSet<ChunkPos>,
    /// Chunks we've asked for and haven't received yet, with the time of
    /// the last request.
    pending: HashMap<ChunkPos, Instant>,
    render_distance: i32,
    retry_after: Duration,
    last_scan: Option<Instant>,
    last_player_chunk: Option<ChunkPos>,
    pub requests_sent: u64,
    pub retries_sent: u64,
}

/// The eight neighbours, in a fixed order.
pub const NEIGHBOUR_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

impl ChunkManager {
    pub fn new(render_distance: i32) -> Self {
        Self {
            loaded: HashMap::new(),
            arrived: HashSet::new(),
            pending: HashMap::new(),
            render_distance: render_distance.max(1),
            retry_after: Duration::from_secs(3),
            last_scan: None,
            last_player_chunk: None,
            requests_sent: 0,
            retries_sent: 0,
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn render_distance(&self) -> i32 {
        self.render_distance
    }

    /// Changes the radius without discarding what is already loaded.
    ///
    /// Rebuilding the manager instead would drop every chunk and every
    /// mesh and stream the world back in from nothing, which is a
    /// second of empty sky for a setting the player expects to take
    /// effect quietly. Growing it lets the next `update` ask for the new
    /// ring; shrinking it lets the same call unload the outer one.
    pub fn set_render_distance(&mut self, render_distance: i32) {
        let wanted = render_distance.max(1);
        if wanted == self.render_distance {
            return;
        }
        self.render_distance = wanted;
        // Force the next `update` to rescan. It normally skips unless
        // the player crossed a chunk boundary, and standing still while
        // turning the setting up is exactly the case that matters.
        self.last_player_chunk = None;
    }

    /// How much of the immediate 3x3 neighbourhood around `centre` is
    /// loaded, as (loaded, needed).
    ///
    /// This is what the loading screen reports and what gates physics.
    /// Only the 3x3 matters, not the whole render distance: the player
    /// can only fall into or walk into a chunk they're touching, so
    /// waiting for the entire ring would keep them staring at a loading
    /// bar long after the world under their feet was solid.
    pub fn spawn_area_progress(&self, centre: ChunkPos) -> (usize, usize) {
        let mut loaded = 0;
        let mut needed = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                needed += 1;
                if self.loaded.contains_key(&ChunkPos::new(centre.x + dx, centre.z + dz)) {
                    loaded += 1;
                }
            }
        }
        (loaded, needed)
    }

    /// Is this chunk's neighbourhood settled enough to mesh?
    ///
    /// A chunk's mesh depends on its eight neighbours (face culling and
    /// lighting across the seam), so meshing it before they arrive means
    /// meshing it again for each one that shows up. During a fresh
    /// stream of 169 chunks that turned ~169 mesh jobs into well over a
    /// thousand -- the reason the frame rate sagged while terrain
    /// loaded.
    ///
    /// A neighbour counts as settled if it's loaded *or* if it lies
    /// outside the render distance and is therefore never coming. That
    /// second case matters: without it the outermost ring would wait
    /// forever and never render.
    pub fn neighbourhood_settled(&self, pos: ChunkPos, centre: ChunkPos) -> bool {
        for (dx, dz) in NEIGHBOUR_OFFSETS {
            let neighbour = ChunkPos::new(pos.x + dx, pos.z + dz);
            if self.loaded.contains_key(&neighbour) {
                continue;
            }
            let distance = (neighbour.x - centre.x)
                .abs()
                .max((neighbour.z - centre.z).abs());
            if distance > self.render_distance {
                continue; // outside the streamed area; it will never arrive
            }
            return false;
        }
        true
    }

    /// True once the ground under and around the player exists.
    #[allow(dead_code)] // convenience wrapper used by tests
    pub fn is_area_ready(&self, centre: ChunkPos) -> bool {
        let (loaded, needed) = self.spawn_area_progress(centre);
        loaded == needed
    }

    pub fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.loaded.contains_key(&pos)
    }

    pub fn chunk_for_world_pos(world_x: f32, world_z: f32) -> ChunkPos {
        ChunkPos::from_world(world_x, world_z)
    }

    /// Called every frame. Returns (chunks to request, chunks to unload).
    ///
    /// The expensive part -- building the wanted set -- only runs when the
    /// player crosses a chunk boundary or when the retry timer is due,
    /// not 60 times a second.
    pub fn update(
        &mut self,
        player_chunk: ChunkPos,
        now: Instant,
    ) -> (Vec<ChunkPos>, Vec<ChunkPos>) {
        let moved = self.last_player_chunk != Some(player_chunk);
        let due = self
            .last_scan
            .map(|t| now.duration_since(t) >= Duration::from_millis(500))
            .unwrap_or(true);
        if !moved && !due {
            return (Vec::new(), Vec::new());
        }
        self.last_player_chunk = Some(player_chunk);
        self.last_scan = Some(now);

        let mut wanted = HashSet::new();
        for dx in -self.render_distance..=self.render_distance {
            for dz in -self.render_distance..=self.render_distance {
                wanted.insert(ChunkPos::new(player_chunk.x + dx, player_chunk.z + dz));
            }
        }

        let mut to_request = Vec::new();
        for pos in &wanted {
            if self.loaded.contains_key(pos) || self.arrived.contains(pos) {
                continue;
            }
            match self.pending.get(pos) {
                Some(&requested_at) if now.duration_since(requested_at) < self.retry_after => {}
                Some(_) => {
                    self.retries_sent += 1;
                    self.pending.insert(*pos, now);
                    to_request.push(*pos);
                }
                None => {
                    self.requests_sent += 1;
                    self.pending.insert(*pos, now);
                    to_request.push(*pos);
                }
            }
        }

        let to_unload: Vec<ChunkPos> = self
            .loaded
            .keys()
            .filter(|pos| !wanted.contains(pos))
            .copied()
            .collect();

        // Stop chasing chunks that are no longer wanted.
        self.pending.retain(|pos, _| wanted.contains(pos));

        (to_request, to_unload)
    }

    /// Call the moment a chunk arrives, before it's integrated, so it
    /// stops being re-requested.
    pub fn note_arrival(&mut self, pos: ChunkPos) {
        self.pending.remove(&pos);
        self.arrived.insert(pos);
    }

    pub fn insert(&mut self, chunk: Chunk) {
        self.arrived.remove(&chunk.pos);
        self.pending.remove(&chunk.pos);
        self.loaded.insert(chunk.pos, chunk);
    }

    pub fn unload(&mut self, pos: ChunkPos) {
        self.loaded.remove(&pos);
        self.pending.remove(&pos);
        self.arrived.remove(&pos);
    }

    pub fn get(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.loaded.get(&pos)
    }

    /// Neighbour chunk lookup for the mesher's padded volume.
    #[allow(dead_code)] // kept: the natural neighbour accessor, used by tests
    pub fn neighbour(&self, pos: ChunkPos, dx: i32, dz: i32) -> Option<&Chunk> {
        self.loaded.get(&ChunkPos::new(pos.x + dx, pos.z + dz))
    }

    #[allow(dead_code)] // used by tests; kept as the natural iteration API
    pub fn iter(&self) -> impl Iterator<Item = &Chunk> {
        self.loaded.values()
    }

    /// Block at a global (block-space) coordinate. `None` means "we don't
    /// have that chunk loaded, so we don't actually know" -- physics
    /// treats that as non-solid, which means you *can* fall through the
    /// world at the edge of loaded chunks. Acceptable for a prototype;
    /// the request-retry above at least means the hole gets filled.
    pub fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.loaded.get(&pos).map(|c| c.get(lx, gy as usize, lz))
    }

    /// Solid for collision purposes. Note this asks `is_collidable`, not
    /// "is not air": water is a block you can be inside, and treating it
    /// as solid is what used to let players walk across lakes.
    pub fn is_solid(&self, gx: i32, gy: i32, gz: i32) -> bool {
        matches!(self.block_at(gx, gy, gz), Some(id) if is_collidable(id))
    }

    /// What a build/break ray can hit. Water is see-through to targeting,
    /// so you can mine the lake bed and place blocks into water rather
    /// than having the ray stop at the surface.
    pub fn is_targetable(&self, gx: i32, gy: i32, gz: i32) -> bool {
        matches!(self.block_at(gx, gy, gz), Some(id) if is_collidable(id))
    }

    /// Applies a confirmed block edit from the server. Returns the chunk's
    /// position if something actually changed, so the caller knows which
    /// mesh to rebuild.
    pub fn apply_block_update(
        &mut self,
        gx: i32,
        gy: i32,
        gz: i32,
        block_id: BlockId,
    ) -> Option<ChunkPos> {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return None;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let chunk = self.loaded.get_mut(&pos)?;
        if chunk.get(lx, gy as usize, lz) == block_id {
            return None; // nothing changed; don't force a re-mesh
        }
        chunk.set(lx, gy as usize, lz, block_id);
        Some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_STONE, CHUNK_VOLUME};

    fn chunk(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn the_render_distance_can_change_without_losing_the_world() {
        // Rebuilding the manager would drop every chunk and mesh and
        // stream the world back from nothing -- a second of empty sky
        // for a setting the player expects to apply quietly.
        let mut chunks = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        let now = Instant::now();
        let (requested, _) = chunks.update(centre, now);
        for pos in &requested {
            chunks.insert(Chunk {
                pos: *pos,
                blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
            });
        }
        let before = chunks.loaded_count();
        assert_eq!(before, 25, "5x5 at distance 2");

        chunks.set_render_distance(3);
        assert_eq!(chunks.render_distance(), 3);
        assert_eq!(chunks.loaded_count(), before, "it threw the world away");

        // And the next scan asks for the new ring even though the
        // player has not moved.
        let (more, unload) = chunks.update(centre, now);
        assert_eq!(more.len(), 49 - 25, "the new ring was not requested");
        assert!(unload.is_empty());
    }

    #[test]
    fn shrinking_the_render_distance_unloads_the_outer_ring() {
        let mut chunks = ChunkManager::new(3);
        let centre = ChunkPos::new(0, 0);
        let now = Instant::now();
        let (requested, _) = chunks.update(centre, now);
        for pos in &requested {
            chunks.insert(Chunk {
                pos: *pos,
                blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
            });
        }

        chunks.set_render_distance(1);
        let (request, unload) = chunks.update(centre, now);
        assert!(request.is_empty());
        assert_eq!(unload.len(), 49 - 9, "the outer rings should go");
    }

    #[test]
    fn setting_the_same_render_distance_changes_nothing() {
        let mut chunks = ChunkManager::new(4);
        let now = Instant::now();
        chunks.update(ChunkPos::new(0, 0), now);
        chunks.set_render_distance(4);
        // The scan state is untouched, so a no-op setting does not cost
        // a full rescan every time the player nudges another slider.
        let (request, unload) = chunks.update(ChunkPos::new(0, 0), now);
        assert!(request.is_empty() && unload.is_empty());
    }

    #[test]
    fn a_render_distance_of_zero_is_refused() {
        let mut chunks = ChunkManager::new(4);
        chunks.set_render_distance(0);
        assert!(chunks.render_distance() >= 1, "a world with no chunks is not a world");
    }

    #[test]
    fn a_dropped_request_is_retried() {
        let mut cm = ChunkManager::new(1);
        // Retry window deliberately longer than the 500 ms rescan
        // interval, so the two timers can be told apart below.
        cm.retry_after = Duration::from_millis(1000);
        let now = Instant::now();
        let (first, _) = cm.update(ChunkPos::new(0, 0), now);
        assert_eq!(first.len(), 9, "3x3 ring around the player");

        // Nothing arrived; before the retry window nothing is re-sent.
        let (again, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_millis(600));
        assert!(again.is_empty(), "retried far too eagerly");

        // After the window, everything still missing is asked for again.
        let (retried, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_millis(1200));
        assert_eq!(retried.len(), 9);
        assert_eq!(cm.retries_sent, 9);
    }

    #[test]
    fn an_arrived_chunk_is_not_requested_again() {
        let mut cm = ChunkManager::new(1);
        cm.retry_after = Duration::from_millis(10);
        let now = Instant::now();
        cm.update(ChunkPos::new(0, 0), now);
        cm.insert(chunk(ChunkPos::new(0, 0)));
        let (retried, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_secs(2));
        assert!(!retried.contains(&ChunkPos::new(0, 0)));
        assert_eq!(cm.pending_count(), 8);
    }

    #[test]
    fn walking_away_unloads_and_stops_chasing() {
        let mut cm = ChunkManager::new(1);
        let now = Instant::now();
        cm.update(ChunkPos::new(0, 0), now);
        cm.insert(chunk(ChunkPos::new(0, 0)));

        let (_, unload) = cm.update(ChunkPos::new(50, 50), now + Duration::from_secs(1));
        assert_eq!(unload, vec![ChunkPos::new(0, 0)]);
        // The old pending set must not linger, or we'd keep re-requesting
        // chunks on the other side of the map forever.
        assert!(cm.pending_count() <= 9);
    }

    #[test]
    fn neighbour_lookup_finds_the_right_chunk() {
        let mut cm = ChunkManager::new(2);
        cm.insert(chunk(ChunkPos::new(0, 0)));
        cm.insert(chunk(ChunkPos::new(1, 0)));
        assert!(cm.neighbour(ChunkPos::new(0, 0), 1, 0).is_some());
        assert!(cm.neighbour(ChunkPos::new(0, 0), 0, 1).is_none());
    }

    #[test]
    fn a_no_op_block_update_does_not_request_a_remesh() {
        let mut cm = ChunkManager::new(1);
        let mut c = chunk(ChunkPos::new(0, 0));
        c.set(1, 2, 3, BLOCK_STONE);
        cm.insert(c);
        assert_eq!(cm.apply_block_update(1, 2, 3, BLOCK_STONE), None);
        assert_eq!(
            cm.apply_block_update(1, 2, 3, BLOCK_AIR),
            Some(ChunkPos::new(0, 0))
        );
    }
}

/// Lets the light engine and the mesher read the world in global
/// coordinates. `None` means the chunk isn't loaded, which lighting
/// treats as a wall rather than as air -- light doesn't leak out into
/// the unknown and then have to be taken back when the chunk arrives.
impl BlockSource for ChunkManager {
    #[inline]
    fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.loaded.get(&pos).map(|c| c.get(lx, gy as usize, lz))
    }

    #[inline]
    fn chunk_data(&self, pos: ChunkPos) -> Option<&[BlockId]> {
        self.loaded.get(&pos).map(|c| c.blocks.as_slice())
    }
}

#[cfg(test)]
mod loading_tests {
    use super::*;
    use primitive_shared::types::CHUNK_VOLUME;

    fn chunk_at(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn the_area_is_not_ready_until_all_nine_chunks_are_there() {
        let mut cm = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        assert!(!cm.is_area_ready(centre), "nothing loaded yet");
        assert_eq!(cm.spawn_area_progress(centre), (0, 9));

        for dx in -1..=1 {
            for dz in -1..=1 {
                cm.insert(chunk_at(ChunkPos::new(dx, dz)));
            }
        }
        assert_eq!(cm.spawn_area_progress(centre), (9, 9));
        assert!(cm.is_area_ready(centre));
    }

    #[test]
    fn a_hole_in_the_neighbourhood_keeps_the_player_waiting() {
        // The centre chunk alone isn't enough: stepping one block sideways
        // would drop the player into an unloaded chunk, which reads as air.
        let mut cm = ChunkManager::new(2);
        cm.insert(chunk_at(ChunkPos::new(0, 0)));
        assert!(!cm.is_area_ready(ChunkPos::new(0, 0)));
    }
}

#[cfg(test)]
mod arrival_tests {
    use super::*;
    use primitive_shared::types::CHUNK_VOLUME;

    #[test]
    fn an_arrived_but_unintegrated_chunk_is_not_requested_again() {
        // Regression: deferring integration made the retry logic think
        // these chunks were still missing, so it asked for them again
        // and again while they waited in the queue.
        let mut cm = ChunkManager::new(1);
        cm.retry_after = Duration::from_millis(1);
        let now = Instant::now();
        let (first, _) = cm.update(ChunkPos::new(0, 0), now);
        assert_eq!(first.len(), 9);

        for pos in &first {
            cm.note_arrival(*pos);
        }

        let (again, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_secs(5));
        assert!(
            again.is_empty(),
            "re-requested {} chunks that had already arrived",
            again.len()
        );
    }

    #[test]
    fn integration_clears_the_arrived_marker() {
        let mut cm = ChunkManager::new(1);
        let pos = ChunkPos::new(0, 0);
        cm.note_arrival(pos);
        assert!(cm.arrived.contains(&pos));
        cm.insert(Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        });
        assert!(cm.arrived.is_empty());
        assert!(cm.is_loaded(pos));
    }
}

#[cfg(test)]
mod settling_tests {
    use super::*;
    use primitive_shared::types::CHUNK_VOLUME;

    fn air_chunk(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn a_chunk_waits_for_its_neighbours_before_meshing() {
        let mut cm = ChunkManager::new(4);
        let centre = ChunkPos::new(0, 0);
        cm.insert(air_chunk(centre));
        assert!(
            !cm.neighbourhood_settled(centre, centre),
            "should wait while neighbours are still streaming in"
        );

        for (dx, dz) in NEIGHBOUR_OFFSETS {
            cm.insert(air_chunk(ChunkPos::new(dx, dz)));
        }
        assert!(cm.neighbourhood_settled(centre, centre));
    }

    #[test]
    fn the_outermost_ring_does_not_wait_forever() {
        // Its outward neighbours are outside the render distance and
        // will never arrive; without this the edge of the world would
        // never be drawn.
        let mut cm = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        for dx in -2..=2 {
            for dz in -2..=2 {
                cm.insert(air_chunk(ChunkPos::new(dx, dz)));
            }
        }
        let edge = ChunkPos::new(2, 2);
        assert!(
            cm.neighbourhood_settled(edge, centre),
            "the outer ring must mesh once everything inside it has arrived"
        );
    }

    #[test]
    fn a_gap_inside_the_radius_still_blocks() {
        let mut cm = ChunkManager::new(3);
        let centre = ChunkPos::new(0, 0);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if (dx, dz) == (1, 1) {
                    continue; // the one still in flight
                }
                cm.insert(air_chunk(ChunkPos::new(dx, dz)));
            }
        }
        assert!(!cm.neighbourhood_settled(centre, centre));
    }
}
