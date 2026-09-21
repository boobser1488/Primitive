//! Voxel lighting: a persistent, world-space light map with incremental
//! updates.
//!
//! ## Why this replaced the previous design
//!
//! The old version lit one chunk at a time into a "chunk + one block of
//! border" volume, rebuilt from scratch every time that chunk was
//! re-meshed. That was wrong *and* slow:
//!
//! * **Wrong.** Light travels up to 15 blocks. A one-block border can't
//!   carry it. A glowstone two blocks inside the neighbouring chunk
//!   simply didn't exist as far as this chunk was concerned, so the seam
//!   between them got a hard lighting discontinuity. Skylight under an
//!   overhang that started in the next chunk had the same problem.
//! * **Slow.** Each rebuild copied and flood-filled 18x64x18 = 20,736
//!   cells. Re-meshing one chunk also re-meshes its 8 neighbours (their
//!   edge faces change), so a single block edit cost nine full light
//!   computations -- roughly 190k cells of BFS for one click.
//!
//! Here light lives in world space and persists between meshings. It is
//! computed once when a chunk loads, and after that only *changed*
//! regions are touched: placing a torch lights the cells it actually
//! reaches, and nothing else. Meshing becomes a pure read.
//!
//! ## The two channels
//!
//! Sky and block light stay separate (packed into one byte, a nibble
//! each) because they behave differently at runtime: skylight is
//! modulated by the time of day, block light isn't. The shader does
//! `max(sky * daylight, block)`, so the day/night cycle costs zero
//! re-meshing.
//!
//! ## Sunlight goes straight down for free
//!
//! Downward propagation at full strength costs nothing, so a shaft of
//! daylight reaches the bottom of a hole undimmed instead of fading with
//! depth. Sideways it attenuates normally. This is the standard voxel
//! trick and it's what makes a lit column look like a lit column.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::packed::{PackedChunk, PackedLight, SECTIONS, SECTION_HEIGHT};
use crate::types::{
    is_opaque, light_emission, light_opacity, BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X,
    CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME, MAX_LIGHT,
};

/// Read access to world blocks in global coordinates. `None` means "that
/// chunk isn't loaded", which lighting treats as a wall: light neither
/// enters nor leaves through it, and the region relights itself when the
/// chunk actually arrives.
pub trait BlockSource {
    fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId>;

    /// A whole chunk's blocks at once, if the implementor has them
    /// contiguously.
    ///
    /// Callers that walk many cells of the same chunk should use this:
    /// `block_at` costs a chunk lookup *per cell*, and at 16k cells per
    /// chunk that dominates everything else they do. Defaults to `None`
    /// so simple implementors don't have to care.
    fn chunk_data(&self, _pos: ChunkPos) -> Option<&[BlockId]> {
        None
    }

    /// A whole chunk in the packed form it is kept in, for implementors
    /// that keep it that way -- the client's `ChunkManager` does.
    ///
    /// **A second fast path rather than a replacement for `chunk_data`.**
    /// A packed chunk has no contiguous slice to hand out, so the callers
    /// that want bulk access copy out of it a row at a time
    /// (`PackedChunk::copy_run`) or decode it whole; and the fixtures
    /// that build a world out of plain arrays keep answering
    /// `chunk_data` and need not know this exists. Asked first where
    /// both could answer.
    fn packed_chunk(&self, _pos: ChunkPos) -> Option<&PackedChunk> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Sky,
    Block,
}

/// Sky level one cell below a cell at `level`, entering block `id`.
///
/// **This must match what `propagate` does when it steps downward**, or
/// the two disagree and the world lights differently depending on
/// whether a column was computed fresh or updated incrementally. They
/// used to: the column pass charged only the block's opacity, while
/// propagation also charged the usual 1 per step. Identical everywhere
/// sunlight is at full strength (the free-fall case), and different
/// under anything that dims it -- water, leaves -- which is why it took
/// partial-opacity blocks in the property test to surface it.
///
/// For a block that holds light (`holds_light`) this is the level in its
/// own cell; what goes on down the column under it is nought, and the
/// column passes ask [`sky_onward`] for that.
#[inline]
fn sky_below(level: u8, id: BlockId) -> u8 {
    if is_opaque(id) {
        return 0;
    }
    let opacity = light_opacity(id);
    if level == MAX_LIGHT && opacity == 0 {
        // Full-strength sunlight falls without loss.
        MAX_LIGHT
    } else if opacity >= MAX_LIGHT {
        match holds_light(id) {
            Some(sealed) if sealed != UP => {
                if level == MAX_LIGHT {
                    MAX_LIGHT
                } else {
                    level.saturating_sub(1)
                }
            }
            _ => 0,
        }
    } else {
        level.saturating_sub(1u8.saturating_add(opacity))
    }
}

/// What the sky column carries on below a cell whose own level is `here`:
/// `here` itself, unless the cell holds its light and passes none of it on.
#[inline]
fn sky_onward(here: u8, id: BlockId) -> u8 {
    if here > 0 && holds_light(id).is_some() {
        0
    } else {
        here
    }
}

/// Index into [`NEIGHBOURS`] of straight up. The two directions of an axis
/// sit side by side there, so the opposite of step `k` is `k ^ 1`.
const UP: u8 = 2;

/// **A block that stops light but does not fill its cell holds light**:
/// the air in the rest of its cell is lit from the cells round it, and
/// none of that light goes on through. `Some(side)` is the one side of the
/// cell the block covers whole -- light cannot come in that way -- as an
/// index into [`NEIGHBOURS`] of the step out of the cell through it.
///
/// "у листвы, когда стоит рядом с неполным, чернеет бок": the lip of a
/// slope is turf lowered a quarter at a time (`dig::lowered`), and turf's
/// row stops all light, so the flood left the lip's cell at nought -- under
/// open noon sky. Every face that looks into that cell takes its light
/// from it (`mesh`'s ring of nine), so the side of a column of leaves
/// standing beside the lip, of the next whole block up the slope, of the
/// water, was drawn black up to the lip's cell: a black band at the foot
/// of every rise, on the part of the face that stands over the lip in
/// plain daylight. A slab, a step, a roof and a bite out of a wall did the
/// same to whatever they were set against.
///
/// Three ways to let the light in were weighed:
///
/// * *Take the opacity off partial blocks* -- light passes through them
///   like a skin of snow. That lights the cell, and it lights everything
///   past it too: a roof of slabs is the sky over the room, and a cave
///   wall with a bite out of it is a window. "Caves do not get brighter"
///   is not negotiable.
/// * *Read the light over the partial in the mesher* rather than in its
///   own cell. The mesher's neighbourhood is one cell deep, which is
///   exactly the ring a face reads; the cells round *those* are outside
///   it, so a seam would light the same cell two ways. And it mends one
///   reader of many: the model on the lip, the player standing on it, the
///   fire on a slab all read the same cell.
/// * **Let the cell receive and not pass (chosen).** The cell is lit as
///   the air in it is, a level under its brightest open neighbour, and the
///   flood never steps out of it. Nothing beyond it can tell it is lit, so
///   no light reaches anywhere it did not before. The one side the block
///   covers whole is the one side it is not lit through: a bite into a
///   cave's ceiling under a meadow stands in the top of its cell, and the
///   daylight over that is on the other side of the rock.
///
/// Its own glow still leaves it -- `spreads` -- because an emitter's light
/// is made in the cell rather than let through it.
///
/// Not the shut door, which is a thin board the way a bite is a thin rock
/// but is a wall with a room on each side of it: see `types::light_opacity`
/// for why its cell is dark.
fn holds_light(id: BlockId) -> Option<u8> {
    match holder_table()[id as usize] {
        0 => None,
        side => Some(side - 1),
    }
}

/// The level a cell at `level` holding `id` gives away to its neighbours:
/// all of it, unless it holds light, when only what it makes itself.
#[inline]
fn spreads(level: u8, id: BlockId, channel: Channel, holders: &[u8; 1 << 16]) -> u8 {
    if holders[id as usize] == 0 {
        level
    } else if channel == Channel::Block {
        light_emission(id).min(level)
    } else {
        0
    }
}

/// Light arriving at a cell that holds light, through the step `k` of
/// [`NEIGHBOURS`] from a cell at `level`: nought through the side it
/// covers, full sun straight down through an open top, one level less
/// otherwise -- the opacity of the block is not charged, because the light
/// is in the air beside it and not in it.
#[inline]
fn into_holder(level: u8, hold: u8, k: usize, channel: Channel) -> u8 {
    let sealed = hold - 1;
    if usize::from(sealed) == k ^ 1 {
        0
    } else if channel == Channel::Sky && k == 3 && level == MAX_LIGHT {
        MAX_LIGHT
    } else {
        level.saturating_sub(1)
    }
}

/// [`holds_light`] for every id at once, one byte each: nought for
/// everything else, the sealed side plus one for a holder. A table because
/// the flood asks it of every cell it pops, and the answer depends on the
/// id alone (the mesher's cover table is the same argument).
fn holder_table() -> &'static [u8; 1 << 16] {
    static TABLE: std::sync::OnceLock<Box<[u8; 1 << 16]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Box::new([0u8; 1 << 16]);
        for (id, slot) in table.iter_mut().enumerate() {
            let id = id as BlockId;
            if is_opaque(id) || light_opacity(id) < MAX_LIGHT {
                continue;
            }
            // The side the rock is pushed against is the one opposite the
            // side it was eaten from (`dig::bite_box`).
            let sealed = if let Some((side, _)) = crate::dig::bite(id) {
                use crate::dig::Side;
                match side {
                    Side::PosX => 1,
                    Side::NegX => 0,
                    Side::PosY => 3,
                    Side::NegY => 2,
                    Side::PosZ => 5,
                    Side::NegZ => 4,
                }
            } else if crate::types::is_partial(id) || crate::types::is_step(id) {
                // A slab, a layer and every shape of step stand on the
                // whole floor of their cell and nowhere cover a whole side
                // (`geometry::step_pose_boxes`: the tread is the lower half
                // in all of them).
                3
            } else {
                continue;
            };
            *slot = sealed + 1;
        }
        table
    })
}

const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// Light for every loaded chunk, one byte per cell: sky in the low
/// nibble, block light in the high nibble -- half what two separate
/// channels would cost, and it keeps both values for a cell in the same
/// cache line.
///
/// **Kept a section at a time** (`PackedLight`). At 256 blocks tall a
/// flat array is 64 KB a chunk, and a section of open sky is fifteen in
/// every cell while a section of deep rock is zero in every cell: 77.5%
/// of sections on real terrain are one value and cost one byte. Reads and
/// writes stay a byte at an index; a section is only made dense by a
/// write that changes it. See `primitive_shared::packed`.
#[derive(Default)]
pub struct LightMap {
    chunks: HashMap<ChunkPos, PackedLight>,
}

#[inline]
fn split(gx: i32, gy: i32, gz: i32) -> (ChunkPos, usize) {
    let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
    (pos, Chunk::index(lx, gy as usize, lz))
}

impl LightMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_lit(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Nibble-packed light for one chunk (sky low, block high), whole.
    /// Lets the mesher copy a row at a time instead of doing a hash
    /// lookup per sampled cell.
    #[inline]
    pub fn chunk_light(&self, pos: ChunkPos) -> Option<&PackedLight> {
        self.chunks.get(&pos)
    }

    /// What the map keeps on the heap for light, in bytes -- the cells,
    /// not the hash table. For the F3 panel and the memory measurements.
    pub fn heap_bytes(&self) -> usize {
        self.chunks.values().map(PackedLight::heap_bytes).sum()
    }

    pub fn unload_chunk(&mut self, pos: ChunkPos) {
        self.chunks.remove(&pos);
    }

    /// Sky light at a global coordinate.
    ///
    /// Above the world is open sky. An **unloaded chunk reads as dark**,
    /// not as daylight.
    ///
    /// This used to return `MAX_LIGHT` for unloaded chunks, on the
    /// reasoning that a briefly-too-bright frontier looks better than a
    /// wall of black. That was a rendering decision living in the wrong
    /// place: the propagation code reads through here too, so an
    /// unloaded neighbour behaved like a permanent 15-strength light
    /// source. Sunlight then leaked sideways out of nowhere into sealed
    /// rock -- carve a pocket inside a hill next to an unloaded chunk
    /// and it came out lit. That's the "lighting is sometimes wrong"
    /// symptom, and it only showed up near the edge of the loaded area,
    /// which is why it looked intermittent.
    ///
    /// The frontier still renders bright: the mesher applies that
    /// fallback itself (see `Neighbourhood::fill`), where it belongs.
    #[inline]
    pub fn sky(&self, gx: i32, gy: i32, gz: i32) -> u8 {
        if gy >= CHUNK_SIZE_Y as i32 {
            return MAX_LIGHT;
        }
        if gy < 0 {
            return 0;
        }
        let (pos, idx) = split(gx, gy, gz);
        match self.chunks.get(&pos) {
            Some(data) => data.get(idx) & 0x0F,
            None => 0,
        }
    }

    #[inline]
    pub fn block(&self, gx: i32, gy: i32, gz: i32) -> u8 {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return 0;
        }
        let (pos, idx) = split(gx, gy, gz);
        match self.chunks.get(&pos) {
            Some(data) => (data.get(idx) >> 4) & 0x0F,
            None => 0,
        }
    }

    #[inline]
    fn get(&self, channel: Channel, gx: i32, gy: i32, gz: i32) -> u8 {
        match channel {
            Channel::Sky => self.sky(gx, gy, gz),
            Channel::Block => self.block(gx, gy, gz),
        }
    }

    /// Writes a level, recording which chunk changed. Returns false if
    /// that chunk isn't loaded (light never escapes into unloaded space).
    fn set(
        &mut self,
        channel: Channel,
        gx: i32,
        gy: i32,
        gz: i32,
        level: u8,
        dirty: &mut HashSet<ChunkPos>,
    ) -> bool {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return false;
        }
        let (pos, idx) = split(gx, gy, gz);
        let Some(data) = self.chunks.get_mut(&pos) else {
            return false;
        };
        let level = level.min(MAX_LIGHT);
        let current = data.get(idx);
        let updated = match channel {
            Channel::Sky => (current & 0xF0) | level,
            Channel::Block => (current & 0x0F) | (level << 4),
        };
        // The comparison is also what keeps a uniform section uniform:
        // the flood writes cells to what they already hold all the time,
        // and `PackedLight::set` would make a section dense for that.
        if updated != current {
            data.set(idx, updated);
            dirty.insert(pos);
        }
        true
    }

    /// Computes light for a newly arrived chunk and lets it exchange
    /// light with its already-loaded neighbours. Returns every chunk
    /// whose light changed, i.e. everything the caller must re-mesh.
    ///
    /// Done in two stages, for speed:
    ///
    /// 1. **In isolation, in a local array.** The chunk's own blocks are
    ///    copied once (256 column reads), then sunlight and block light
    ///    are flood-filled inside that array with plain indexing. No
    ///    hash lookups at all.
    /// 2. **Reconcile with the neighbours.** Only the four seams are
    ///    examined, and only cells where the two sides disagree by more
    ///    than one level get queued for the general cross-chunk
    ///    propagation.
    ///
    /// The first version did stage 2's map-based BFS for *every* cell in
    /// the chunk: ~16k queued cells, each doing several hash lookups per
    /// neighbour. That measured at ~130 ms per chunk in a debug build --
    /// the chunk-integration budget was blown by a single chunk, which
    /// is what made the game hitch while streaming terrain.
    pub fn load_chunk<S: BlockSource>(&mut self, src: &S, pos: ChunkPos) -> HashSet<ChunkPos> {
        let data = compute_isolated(&chunk_blocks(src, pos));
        self.insert_precomputed(src, pos, data)
    }

    /// Stage 2 on its own, for callers that ran `compute_isolated` on a
    /// worker thread (see the client's `mesher` module). Stage 1 is pure
    /// and by far the more expensive half, so it parallelises; this half
    /// touches the shared map and stays on the owning thread.
    pub fn insert_precomputed<S: BlockSource>(
        &mut self,
        src: &S,
        pos: ChunkPos,
        data: Vec<u8>,
    ) -> HashSet<ChunkPos> {
        self.insert_packed(src, pos, PackedLight::pack(&data))
    }

    /// The same, for light a worker has already packed -- which is where
    /// the client packs it, so the frame never walks 64 KB to find out
    /// which sections are sky. See the client's `mesher`.
    pub fn insert_packed<S: BlockSource>(
        &mut self,
        src: &S,
        pos: ChunkPos,
        data: PackedLight,
    ) -> HashSet<ChunkPos> {
        let mut dirty = HashSet::new();
        self.chunks.insert(pos, data);
        dirty.insert(pos);

        // --- stage 2: reconcile across the four seams ---
        let mut sky_queue = VecDeque::new();
        let mut block_queue = VecDeque::new();

        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let neighbour = ChunkPos::new(pos.x + dx, pos.z + dz);
            // Both chunks' data are fetched once per seam, not once per
            // cell. Going through `self.sky()`/`self.block()` here meant
            // ~16k hash lookups per chunk, which showed up as a frame
            // rate that sagged in proportion to how fast terrain was
            // streaming in.
            let (Some(mine), Some(theirs)) =
                (self.chunks.get(&pos), self.chunks.get(&neighbour))
            else {
                continue;
            };

            // A column at a time, which walks `y` -- the slowest axis of
            // the layout -- and so reads one byte from each of
            // sixty-four cache lines per column. Sweeping the sixteen
            // pairs plane by plane instead was tried and measured at
            // 4.8% of a scan that costs six microseconds: both light
            // arrays are sixteen kilobytes and sit in cache whichever
            // way round they are read, so there was nothing here for a
            // better order to win. Left as it was.
            //
            // **Then a section at a time, and a pair of uniform sections
            // that cannot disagree is skipped whole.** When the light map
            // became packed, reading a cell stopped being an index into a
            // slice and became a section lookup and a branch, on a scan of
            // 16,384 pairs of reads per seam, four seams a chunk. With the
            // skip, the whole of this reconciliation measures 0.081 ms a
            // chunk over 1793 chunks of the benchmark world, against 0.078
            // when the map was flat (`tests/chunk_memory.rs`, on an idle
            // machine: a reading of 0.214 taken while cargo compiled on
            // the same cores was the machine, not this). Two sections that are each
            // one value are either within a level of each other in both
            // channels, in which case every cell of them passes the test
            // below and nothing would be queued, or they are not, and
            // then they are scanned as before. Most seam sections are sky
            // against sky or rock against rock, so most of the scan is
            // now two comparisons.
            //
            // The queues fill in a different order than they did, a
            // section band at a time rather than a column at a time. The
            // flood only ever raises a level to the brightest path that
            // reaches it, which does not depend on the order it is seeded
            // in -- the property `light_does_not_depend_on_chunk_arrival_
            // order` already holds the whole map to.
            let cells = seam_cells(pos, dx, dz);
            for section in 0..SECTIONS {
                if let (Some(my_value), Some(their_value)) =
                    (mine.uniform_section(section), theirs.uniform_section(section))
                {
                    let apart = |a: u8, b: u8| a > b + 1 || b > a + 1;
                    if !apart(my_value & 0x0F, their_value & 0x0F)
                        && !apart(my_value >> 4, their_value >> 4)
                    {
                        continue;
                    }
                }
                let band = section * SECTION_HEIGHT..(section + 1) * SECTION_HEIGHT;
                for &(my_xz, their_xz) in &cells {
                    let (my_lx, my_lz) = local_of(my_xz);
                    let (their_lx, their_lz) = local_of(their_xz);

                    for gy in band.clone() {
                        let my_cell = mine.get(Chunk::index(my_lx, gy, my_lz));
                        let their_cell = theirs.get(Chunk::index(their_lx, gy, their_lz));

                        let (my_sky, their_sky) = (my_cell & 0x0F, their_cell & 0x0F);
                        if their_sky > my_sky + 1 {
                            sky_queue.push_back((their_xz.0, gy as i32, their_xz.1));
                        } else if my_sky > their_sky + 1 {
                            sky_queue.push_back((my_xz.0, gy as i32, my_xz.1));
                        }

                        let (my_block, their_block) = (my_cell >> 4, their_cell >> 4);
                        if their_block > my_block + 1 {
                            block_queue.push_back((their_xz.0, gy as i32, their_xz.1));
                        } else if my_block > their_block + 1 {
                            block_queue.push_back((my_xz.0, gy as i32, my_xz.1));
                        }
                    }
                }
            }
        }

        self.propagate(src, Channel::Sky, sky_queue, &mut dirty);
        self.propagate(src, Channel::Block, block_queue, &mut dirty);
        dirty
    }

    /// Updates light after one block changed. `src` must already return
    /// the new block. Returns the chunks that need re-meshing.
    ///
    /// This is the whole point of the persistent map: the work is
    /// proportional to the volume the change actually affects, not to
    /// the chunk (let alone nine chunks).
    pub fn set_block<S: BlockSource>(
        &mut self,
        src: &S,
        gx: i32,
        gy: i32,
        gz: i32,
        new_id: BlockId,
    ) -> HashSet<ChunkPos> {
        let mut dirty = HashSet::new();
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return dirty;
        }
        let (pos, _) = split(gx, gy, gz);
        if !self.chunks.contains_key(&pos) {
            return dirty;
        }

        // --- block light ---
        let mut refill = self.remove(src, Channel::Block, (gx, gy, gz), &mut dirty);
        let emission = light_emission(new_id);
        if emission > 0 {
            self.set(Channel::Block, gx, gy, gz, emission, &mut dirty);
            refill.push_back((gx, gy, gz));
        }
        // Seed from the neighbours as well. Breaking a solid block opens
        // a cell that had no light of its own, so `remove` returns
        // nothing and there is no emission either -- without this the
        // newly opened cell stays black even with a glowstone right next
        // to it, and the darkness spreads as the player keeps digging.
        // The sky pass below has always done this; the block pass didn't.
        for (dx, dy, dz) in NEIGHBOURS {
            let n = (gx + dx, gy + dy, gz + dz);
            if self.get(Channel::Block, n.0, n.1, n.2) > 1 {
                refill.push_back(n);
            }
        }
        self.propagate(src, Channel::Block, refill, &mut dirty);

        // --- sky light ---
        let mut refill = self.remove(src, Channel::Sky, (gx, gy, gz), &mut dirty);
        // Recompute direct sunlight down this column: breaking the block
        // may have opened a shaft, placing one may have closed it.
        let mut level = MAX_LIGHT;
        for y in (0..CHUNK_SIZE_Y as i32).rev() {
            let id = src.block_at(gx, y, gz).unwrap_or(BLOCK_AIR);
            let here = sky_below(level, id);
            if here > self.sky(gx, y, gz) {
                self.set(Channel::Sky, gx, y, gz, here, &mut dirty);
                refill.push_back((gx, y, gz));
            }
            level = sky_onward(here, id);
            if level == 0 {
                break;
            }
        }
        // Neighbouring cells may now light the gap we just opened.
        for (dx, dy, dz) in NEIGHBOURS {
            let n = (gx + dx, gy + dy, gz + dz);
            if self.get(Channel::Sky, n.0, n.1, n.2) > 1 {
                refill.push_back(n);
            }
        }
        self.propagate(src, Channel::Sky, refill, &mut dirty);

        dirty
    }

    /// BFS light spread. Each step costs 1 plus whatever the destination
    /// absorbs -- except full-strength sunlight travelling straight down,
    /// which is free.
    fn propagate<S: BlockSource>(
        &mut self,
        src: &S,
        channel: Channel,
        mut queue: VecDeque<(i32, i32, i32)>,
        dirty: &mut HashSet<ChunkPos>,
    ) {
        let holders = holder_table();
        while let Some((gx, gy, gz)) = queue.pop_front() {
            let level = self.get(channel, gx, gy, gz);
            if level <= 1 {
                continue;
            }
            // A cell that holds light gives none of it away (`holds_light`).
            let level = match src.block_at(gx, gy, gz) {
                Some(here) => spreads(level, here, channel, holders),
                None => level,
            };
            if level <= 1 {
                continue;
            }

            for (k, (dx, dy, dz)) in NEIGHBOURS.into_iter().enumerate() {
                let (nx, ny, nz) = (gx + dx, gy + dy, gz + dz);
                if ny < 0 || ny >= CHUNK_SIZE_Y as i32 {
                    continue;
                }
                let (npos, _) = split(nx, ny, nz);
                if !self.chunks.contains_key(&npos) {
                    continue; // never write into unloaded space
                }
                let Some(id) = src.block_at(nx, ny, nz) else {
                    continue;
                };
                if is_opaque(id) {
                    continue;
                }

                let opacity = light_opacity(id);
                let hold = if opacity >= MAX_LIGHT { holders[id as usize] } else { 0 };
                let sunbeam = channel == Channel::Sky && dy == -1 && level == MAX_LIGHT && opacity == 0;
                let new_level = if hold != 0 {
                    into_holder(level, hold, k, channel)
                } else if sunbeam {
                    MAX_LIGHT
                } else {
                    level.saturating_sub(1u8.saturating_add(opacity))
                };
                if new_level == 0 {
                    continue;
                }
                if self.get(channel, nx, ny, nz) < new_level {
                    self.set(channel, nx, ny, nz, new_level, dirty);
                    // Lit, and nothing to pass on: a holder is not queued.
                    if hold == 0 {
                        queue.push_back((nx, ny, nz));
                    }
                }
            }
        }
    }

    /// Clears the light that flowed out of `start`, and collects the
    /// cells at the edge of the cleared region that are still lit -- they
    /// seed the refill pass that fills the hole back in from whatever
    /// other sources remain.
    fn remove<S: BlockSource>(
        &mut self,
        src: &S,
        channel: Channel,
        start: (i32, i32, i32),
        dirty: &mut HashSet<ChunkPos>,
    ) -> VecDeque<(i32, i32, i32)> {
        let mut refill = VecDeque::new();
        let start_level = self.get(channel, start.0, start.1, start.2);
        if start_level == 0 {
            return refill;
        }
        self.set(channel, start.0, start.1, start.2, 0, dirty);

        let holders = holder_table();
        let mut queue = VecDeque::from([(start, start_level)]);
        while let Some(((gx, gy, gz), level)) = queue.pop_front() {
            // **Nothing was lit by a cell that holds light** (`holds_light`),
            // so clearing one clears nothing round it: every lit neighbour
            // is a way the light may come back in, and is kept. Not asked of
            // the cell the edit is at -- the block there now is not the
            // block the light flowed out of.
            let holder = (gx, gy, gz) != start
                && src.block_at(gx, gy, gz).is_some_and(|id| holders[id as usize] != 0);
            if holder {
                if channel == Channel::Block {
                    let glow = src.block_at(gx, gy, gz).map_or(0, light_emission);
                    if glow > 0 {
                        self.set(channel, gx, gy, gz, glow, dirty);
                        refill.push_back((gx, gy, gz));
                    }
                }
                for (dx, dy, dz) in NEIGHBOURS {
                    let (nx, ny, nz) = (gx + dx, gy + dy, gz + dz);
                    if ny < 0 || ny >= CHUNK_SIZE_Y as i32 || !self.chunks.contains_key(&split(nx, ny, nz).0) {
                        continue;
                    }
                    if self.get(channel, nx, ny, nz) > 0 {
                        refill.push_back((nx, ny, nz));
                    }
                }
                continue;
            }
            for (dx, dy, dz) in NEIGHBOURS {
                let (nx, ny, nz) = (gx + dx, gy + dy, gz + dz);
                if ny < 0 || ny >= CHUNK_SIZE_Y as i32 {
                    continue;
                }
                let (npos, _) = split(nx, ny, nz);
                if !self.chunks.contains_key(&npos) {
                    continue;
                }
                let neighbour_level = self.get(channel, nx, ny, nz);
                if neighbour_level == 0 {
                    continue;
                }

                // A full-strength sunbeam below the removed cell was lit
                // *by* it even though its level isn't lower -- without
                // this case a filled-in hole leaves a lit column hanging
                // underneath it.
                let sunbeam = channel == Channel::Sky
                    && dy == -1
                    && level == MAX_LIGHT
                    && neighbour_level == MAX_LIGHT;

                if neighbour_level < level || sunbeam {
                    self.set(channel, nx, ny, nz, 0, dirty);
                    queue.push_back(((nx, ny, nz), neighbour_level));
                } else {
                    refill.push_back((nx, ny, nz));
                }
            }
        }
        refill
    }
}

/// Local (x, z) inside whichever chunk a global (x, z) belongs to.
#[inline]
fn local_of((gx, gz): (i32, i32)) -> (usize, usize) {
    (
        gx.rem_euclid(CHUNK_SIZE_X as i32) as usize,
        gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize,
    )
}

/// Pairs of ((x, z) inside `pos`, (x, z) just across the seam) along the
/// side facing (dx, dz), in global coordinates.
fn seam_cells(pos: ChunkPos, dx: i32, dz: i32) -> Vec<((i32, i32), (i32, i32))> {
    let ox = pos.x * CHUNK_SIZE_X as i32;
    let oz = pos.z * CHUNK_SIZE_Z as i32;
    let last_x = CHUNK_SIZE_X as i32 - 1;
    let last_z = CHUNK_SIZE_Z as i32 - 1;
    let mut cells = Vec::with_capacity(CHUNK_SIZE_X);
    match (dx, dz) {
        (1, 0) => {
            for lz in 0..CHUNK_SIZE_Z as i32 {
                cells.push(((ox + last_x, oz + lz), (ox + last_x + 1, oz + lz)));
            }
        }
        (-1, 0) => {
            for lz in 0..CHUNK_SIZE_Z as i32 {
                cells.push(((ox, oz + lz), (ox - 1, oz + lz)));
            }
        }
        (0, 1) => {
            for lx in 0..CHUNK_SIZE_X as i32 {
                cells.push(((ox + lx, oz + last_z), (ox + lx, oz + last_z + 1)));
            }
        }
        _ => {
            for lx in 0..CHUNK_SIZE_X as i32 {
                cells.push(((ox + lx, oz), (ox + lx, oz - 1)));
            }
        }
    }
    cells
}

/// Copies one chunk's blocks out of the world into a flat array.
///
/// Note this costs a `block_at` call per cell. If you already hold the
/// `Chunk` (because it just arrived, say), clone its `blocks` instead --
/// going through here is 16,384 chunk lookups for data you're holding.
/// Kept for callers that genuinely only have a `BlockSource`.
pub fn chunk_blocks<S: BlockSource>(src: &S, pos: ChunkPos) -> Vec<BlockId> {
    // A decode of sixteen sections when the implementor keeps it packed,
    // which is a few hundred microseconds against the 65,536 chunk
    // lookups of the per-cell path below.
    if let Some(packed) = src.packed_chunk(pos) {
        return packed.to_blocks();
    }
    // One memcpy when the implementor has the chunk contiguously --
    // that is the whole reason `chunk_data` exists.
    if let Some(data) = src.chunk_data(pos) {
        return data.to_vec();
    }
    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;
    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
    for lz in 0..CHUNK_SIZE_Z {
        for lx in 0..CHUNK_SIZE_X {
            let gx = origin_x + lx as i32;
            let gz = origin_z + lz as i32;
            for y in 0..CHUNK_SIZE_Y {
                blocks[Chunk::index(lx, y, lz)] =
                    src.block_at(gx, y as i32, gz).unwrap_or(BLOCK_AIR);
            }
        }
    }
    blocks
}

/// Lights one chunk as if it were alone in the world, into a fresh
/// nibble-packed array. Pure array indexing -- this is the fast path that
/// does the bulk of the work; the seams are reconciled afterwards.
pub fn compute_isolated(blocks: &[BlockId]) -> Vec<u8> {
    let mut data = vec![0u8; CHUNK_VOLUME];
    let mut sky_queue: VecDeque<usize> = VecDeque::new();
    let mut block_queue: VecDeque<usize> = VecDeque::new();

    // Direct sunlight, top down. Levels only: what gets *queued* is
    // decided afterwards, and it is far less than this.
    //
    // **A plane at a time, not a column at a time**, with the level each
    // column has reached carried in a 256-byte side array. Sunlight is a
    // column rule -- each cell's level comes from the one above it -- but
    // walking it as a column steps through `blocks` and `data` in strides
    // of a whole plane, so all 64 reads of a column land on 64 different
    // cache lines and every one of the 256 columns pays that again.
    // Sweeping downward plane by plane touches both arrays strictly
    // sequentially instead, and the carried levels are small enough to
    // stay in L1 throughout.
    //
    // `alive` counts the columns sunlight has not yet been stopped in, so
    // a chunk of solid rock under a shallow skyline stops the sweep
    // instead of walking to the bedrock: the columns that are already
    // dark contribute nothing but zeroes to an array that starts full of
    // them.
    {
        const COLUMNS: usize = CHUNK_SIZE_X * CHUNK_SIZE_Z;
        let mut level = [MAX_LIGHT; COLUMNS];
        let mut alive = COLUMNS;
        for y in (0..CHUNK_SIZE_Y).rev() {
            if alive == 0 {
                break;
            }
            let plane = y * COLUMNS;
            for column in 0..COLUMNS {
                if level[column] == 0 {
                    continue;
                }
                let id = blocks[plane + column];
                let next = sky_below(level[column], id);
                level[column] = sky_onward(next, id);
                // A whole byte rather than the low nibble of one: block
                // light has not been written yet, so the high nibble is
                // still the zero the array was made with. The emission
                // pass below is the one that has to preserve what is
                // already there.
                data[plane + column] = next;
                if level[column] == 0 {
                    alive -= 1;
                }
            }
        }
    }

    // Seed the flood with the cells that can actually give light away.
    //
    // The column pass above already leaves every *vertical* pair
    // consistent -- each cell's level is derived from the one above it
    // through the same rule the flood would apply -- so the only cells
    // with anything to contribute are those whose *horizontal*
    // neighbours are darker. Under open sky that is almost none of them:
    // the previous version queued every lit cell, which on a chunk with
    // a normal skyline is ten thousand pushes and pops that each
    // discover their four neighbours are already at fifteen.
    //
    // Walked plane by plane for the same reason the sunlight sweep above
    // is: `y` is the slowest axis of the layout, so a loop with it
    // innermost reads one byte from each of 64 cache lines per column and
    // then does it again for the next one. Every cell is visited either
    // way; only the order changes.
    for y in 0..CHUNK_SIZE_Y {
        let plane = y * CHUNK_SIZE_X * CHUNK_SIZE_Z;
        for lz in 0..CHUNK_SIZE_Z {
            for lx in 0..CHUNK_SIZE_X {
                let idx = plane + lz * CHUNK_SIZE_X + lx;
                let level = data[idx] & 0x0F;
                if level <= 1 {
                    continue;
                }
                if spreads_sideways(blocks, &data, plane, lx, lz, level) {
                    sky_queue.push_back(idx);
                }
            }
        }
    }

    for (idx, &id) in blocks.iter().enumerate() {
        let emission = light_emission(id);
        if emission > 0 {
            data[idx] = (data[idx] & 0x0F) | (emission << 4);
            block_queue.push_back(idx);
        }
    }

    flood_local(blocks, &mut data, sky_queue, Channel::Sky);
    flood_local(blocks, &mut data, block_queue, Channel::Block);
    data
}

/// Whether a sky-lit cell has a horizontal neighbour it could brighten.
///
/// Only horizontal: see the seeding loop in `compute_isolated`. Cells on
/// the chunk's edge are not seeded on account of what is outside it --
/// the seam pass in `LightMap::insert_precomputed` owns that.
///
/// `plane` is the index the cell's y layer starts at, so a neighbour is
/// an add rather than three coordinates put back through `Chunk::index`.
fn spreads_sideways(blocks: &[BlockId], data: &[u8], plane: usize, lx: usize, lz: usize, level: u8) -> bool {
    // The four horizontal steps, as their indices in `NEIGHBOURS`.
    for (k, (dx, dz)) in [(0usize, (1i32, 0i32)), (1, (-1, 0)), (4, (0, 1)), (5, (0, -1))] {
        let nx = lx as i32 + dx;
        let nz = lz as i32 + dz;
        if nx < 0 || nz < 0 || nx >= CHUNK_SIZE_X as i32 || nz >= CHUNK_SIZE_Z as i32 {
            continue;
        }
        let nidx = plane + nz as usize * CHUNK_SIZE_X + nx as usize;
        // **What the neighbour already has, before what it is made of.**
        //
        // Light arriving from here is at most one step down -- an opacity
        // only takes more off -- so a neighbour already that bright can
        // never be brightened from this cell, whatever block is in it.
        // Answering from the light array alone skips the two table
        // lookups below, and under open sky, where every cell is at
        // fifteen and so is everything beside it, that is the entire
        // test for the great majority of the chunk.
        let current = data[nidx] & 0x0F;
        if current + 1 >= level {
            continue;
        }
        let id = blocks[nidx];
        if is_opaque(id) {
            continue;
        }
        let opacity = light_opacity(id);
        let arriving = if opacity >= MAX_LIGHT {
            match holder_table()[id as usize] {
                0 => 0,
                hold => into_holder(level, hold, k, Channel::Sky),
            }
        } else {
            level.saturating_sub(1u8.saturating_add(opacity))
        };
        if arriving > current {
            // ...and only if this cell gives light away at all.
            return holder_table()[blocks[plane + lz * CHUNK_SIZE_X + lx] as usize] == 0;
        }
    }
    false
}

fn flood_local(
    blocks: &[BlockId],
    data: &mut [u8],
    mut queue: VecDeque<usize>,
    channel: Channel,
) {
    let sx = CHUNK_SIZE_X;
    let sy = CHUNK_SIZE_Y;
    let sz = CHUNK_SIZE_Z;
    let holders = holder_table();

    while let Some(idx) = queue.pop_front() {
        let level = match channel {
            Channel::Sky => data[idx] & 0x0F,
            Channel::Block => (data[idx] >> 4) & 0x0F,
        };
        // A cell that holds light gives none of it away (`holds_light`).
        let level = spreads(level, blocks[idx], channel, holders);
        if level <= 1 {
            continue;
        }

        // Recover local coordinates from the flat index.
        let x = idx % sx;
        let z = (idx / sx) % sz;
        let y = idx / (sx * sz);

        for (k, (dx, dy, dz)) in NEIGHBOURS.into_iter().enumerate() {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            let nz = z as i32 + dz;
            // Outside the chunk: left for the seam reconciliation pass.
            if nx < 0 || ny < 0 || nz < 0
                || nx >= sx as i32 || ny >= sy as i32 || nz >= sz as i32
            {
                continue;
            }
            let nidx = Chunk::index(nx as usize, ny as usize, nz as usize);
            let id = blocks[nidx];
            if is_opaque(id) {
                continue;
            }

            let opacity = light_opacity(id);
            let hold = if opacity >= MAX_LIGHT { holders[id as usize] } else { 0 };
            let sunbeam = channel == Channel::Sky && dy == -1 && level == MAX_LIGHT && opacity == 0;
            let new_level = if hold != 0 {
                into_holder(level, hold, k, channel)
            } else if sunbeam {
                MAX_LIGHT
            } else {
                level.saturating_sub(1u8.saturating_add(opacity))
            };
            if new_level == 0 {
                continue;
            }

            let current = match channel {
                Channel::Sky => data[nidx] & 0x0F,
                Channel::Block => (data[nidx] >> 4) & 0x0F,
            };
            if current < new_level {
                data[nidx] = match channel {
                    Channel::Sky => (data[nidx] & 0xF0) | new_level,
                    Channel::Block => (data[nidx] & 0x0F) | (new_level << 4),
                };
                // Lit, and nothing to pass on: a holder is not queued.
                if hold == 0 {
                    queue.push_back(nidx);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    pub use super::tests::*;

    use crate::types::BlockId;
    pub fn world_block(world: &super::tests::World, gx: i32, gy: i32, gz: i32) -> BlockId {
        use crate::lighting::BlockSource;
        world.block_at(gx, gy, gz).unwrap()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::{Chunk, BLOCK_GLOWSTONE, BLOCK_STONE};

    /// A world of chunks; anything not inserted reads as unloaded.
    #[derive(Default)]
    pub struct World {
        chunks: HashMap<ChunkPos, Chunk>,
    }

    impl World {
        pub fn insert(&mut self, chunk: Chunk) {
            self.chunks.insert(chunk.pos, chunk);
        }
        pub fn set(&mut self, gx: i32, gy: i32, gz: i32, id: BlockId) {
            let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
            if let Some(c) = self.chunks.get_mut(&pos) {
                c.set(lx, gy as usize, lz, id);
            }
        }
    }

    impl BlockSource for World {
        fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                return Some(BLOCK_AIR);
            }
            let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
            self.chunks.get(&pos).map(|c| c.get(lx, gy as usize, lz))
        }
    }

    /// Stone up to y=30 inclusive, air above.
    pub fn slab(pos: ChunkPos) -> Chunk {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..=30 {
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        Chunk { pos, blocks }
    }

    pub fn world_with(positions: &[ChunkPos]) -> (World, LightMap) {
        let mut world = World::default();
        for &p in positions {
            world.insert(slab(p));
        }
        let mut light = LightMap::new();
        for &p in positions {
            light.load_chunk(&world, p);
        }
        (world, light)
    }

    #[test]
    fn open_sky_is_lit_and_rock_is_dark() {
        let (_w, light) = world_with(&[ChunkPos::new(0, 0)]);
        assert_eq!(light.sky(8, 40, 8), MAX_LIGHT, "open air");
        assert_eq!(light.sky(8, 30, 8), 0, "inside the surface block itself");
        assert_eq!(light.sky(8, 31, 8), MAX_LIGHT, "the cell just above ground");
        assert_eq!(light.sky(8, 10, 8), 0, "deep stone");
    }

    #[test]
    fn a_shaft_of_daylight_reaches_the_bottom_undimmed() {
        // Regression: with per-step attenuation downward, the bottom of a
        // 20-block shaft came out nearly black instead of sunlit.
        let mut world = World::default();
        world.insert(slab(ChunkPos::new(0, 0)));
        let mut light = LightMap::new();
        light.load_chunk(&world, ChunkPos::new(0, 0));

        for y in 11..=30 {
            world.set(8, y, 8, BLOCK_AIR);
            light.set_block(&world, 8, y, 8, BLOCK_AIR);
        }
        assert_eq!(
            light.sky(8, 11, 8),
            MAX_LIGHT,
            "sunlight falling straight down must not attenuate"
        );
    }

    #[test]
    fn light_crosses_a_chunk_seam_from_deep_inside_the_neighbour() {
        // This is what the old one-block-border version could not do:
        // the source is 3 blocks inside chunk (1,0), well beyond any
        // border a per-chunk volume would have copied.
        let a = ChunkPos::new(0, 0);
        let b = ChunkPos::new(1, 0);
        let (mut world, mut light) = world_with(&[a, b]);

        // Hollow a tunnel through the seam at y=31 (just above ground).
        for gx in 12..=19 {
            world.set(gx, 31, 8, BLOCK_AIR);
            light.set_block(&world, gx, 31, 8, BLOCK_AIR);
        }
        // Glowstone at x=19, three blocks inside chunk 1.
        world.set(19, 31, 8, BLOCK_GLOWSTONE);
        let dirty = light.set_block(&world, 19, 31, 8, BLOCK_GLOWSTONE);

        assert!(light.block(15, 31, 8) > 0, "light must reach across the seam");
        assert!(
            dirty.contains(&a),
            "the neighbouring chunk must be reported dirty so it re-meshes"
        );
    }

    #[test]
    fn removing_a_light_source_actually_darkens_the_area() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        world.set(8, 31, 8, BLOCK_GLOWSTONE);
        light.set_block(&world, 8, 31, 8, BLOCK_GLOWSTONE);
        let lit = light.block(10, 31, 8);
        assert!(lit > 0, "neighbourhood should be lit");

        world.set(8, 31, 8, BLOCK_AIR);
        light.set_block(&world, 8, 31, 8, BLOCK_AIR);
        assert_eq!(
            light.block(10, 31, 8),
            0,
            "light must be removed, not left behind as a ghost"
        );
    }

    #[test]
    fn two_sources_survive_the_removal_of_one() {
        // Removal has to refill from whatever is left, not blank the
        // whole region.
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        for (x, z) in [(4, 8), (12, 8)] {
            world.set(x, 31, z, BLOCK_GLOWSTONE);
            light.set_block(&world, x, 31, z, BLOCK_GLOWSTONE);
        }
        let before = light.block(4, 31, 8);
        world.set(4, 31, 8, BLOCK_AIR);
        light.set_block(&world, 4, 31, 8, BLOCK_AIR);
        let after = light.block(4, 31, 8);

        assert_eq!(
            light.block(12, 31, 8),
            light_emission(BLOCK_GLOWSTONE),
            "the surviving source must keep its own full emission"
        );
        assert!(
            light.block(11, 31, 8) > 0,
            "the surviving source must still light its surroundings"
        );
        // The removed cell isn't black -- the other glowstone is 8 blocks
        // away and still reaches it -- but it must have dimmed to exactly
        // that falloff instead of keeping its own emission.
        assert!(after < before, "removal left the old brightness behind ({before} -> {after})");
        assert_eq!(after, light_emission(BLOCK_GLOWSTONE).saturating_sub(8));
    }

    #[test]
    fn filling_a_shaft_removes_the_sunlight_underneath_it() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);
        for y in 20..=30 {
            world.set(8, y, 8, BLOCK_AIR);
            light.set_block(&world, 8, y, 8, BLOCK_AIR);
        }
        assert_eq!(light.sky(8, 20, 8), MAX_LIGHT);

        world.set(8, 30, 8, BLOCK_STONE);
        light.set_block(&world, 8, 30, 8, BLOCK_STONE);
        assert_eq!(
            light.sky(8, 20, 8),
            0,
            "capping the shaft must extinguish the column below it"
        );
    }

    #[test]
    fn unloaded_neighbours_do_not_stop_a_chunk_from_lighting() {
        let (_w, light) = world_with(&[ChunkPos::new(5, -3)]);
        assert_eq!(light.sky(5 * 16 + 2, 40, -3 * 16 + 2), MAX_LIGHT);
    }


    #[test]
    fn light_does_not_depend_on_chunk_arrival_order() {
        // Whatever order chunks stream in, the final light must match.
        // If it doesn't, players see different lighting depending on
        // which way they walked into an area.
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(1, 0),
            ChunkPos::new(-1, 0),
            ChunkPos::new(0, 1),
        ];
        let mut world = World::default();
        for &p in &positions {
            world.insert(slab(p));
        }
        // A glowstone near the seam of chunk (0,0).
        world.set(14, 31, 8, BLOCK_GLOWSTONE);

        let mut forward = LightMap::new();
        for &p in &positions {
            forward.load_chunk(&world, p);
        }
        let mut backward = LightMap::new();
        for &p in positions.iter().rev() {
            backward.load_chunk(&world, p);
        }

        for gx in -16..32 {
            for gz in -16..32 {
                for gy in [20, 30, 31, 32, 40] {
                    assert_eq!(
                        forward.block(gx, gy, gz),
                        backward.block(gx, gy, gz),
                        "block light differs at ({gx},{gy},{gz})"
                    );
                    assert_eq!(
                        forward.sky(gx, gy, gz),
                        backward.sky(gx, gy, gz),
                        "sky light differs at ({gx},{gy},{gz})"
                    );
                }
            }
        }
    }

    /// Deterministic pseudo-random sequence, so a failure is reproducible.
    fn lcg(state: &mut u64) -> u64 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *state >> 33
    }

    /// **The property that matters:** light updated incrementally must
    /// equal light computed from scratch. Anything else means the world
    /// looks different depending on what the player happened to do,
    /// which is exactly the "sometimes wrong" symptom.
    #[test]
    fn incremental_edits_match_a_full_recompute() {
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(1, 0),
            ChunkPos::new(0, 1),
            ChunkPos::new(1, 1),
        ];
        let mut world = World::default();
        for &p in &positions {
            world.insert(slab(p));
        }
        let mut light = LightMap::new();
        for &p in &positions {
            light.load_chunk(&world, p);
        }

        let mut rng = 0x5eed_1337u64;
        for step in 0..400 {
            let gx = (lcg(&mut rng) % 32) as i32;
            let gz = (lcg(&mut rng) % 32) as i32;
            let gy = 25 + (lcg(&mut rng) % 12) as i32;
            // Water and leaves matter here: they're the partial-opacity
            // cases, where light passes but is attenuated. Those exercise
            // the cost arithmetic in propagation and removal, which
            // all-or-nothing blocks never touch.
            //
            // Layers matter for the opposite reason: a shallow drift of
            // snow is the same *material* as a block that stops light
            // dead, and stops none at all. A predicate that forgot to
            // ask how deep it was would light the world differently
            // depending on which half of the pair got there first, and
            // this is the test that would catch it.
            //
            // And the blocks that hold light (`holds_light`): lit, passing
            // nothing on, and shut on one side -- the one rule in the flood
            // that is not the same both ways across a face, which is
            // exactly what an incremental removal can get wrong.
            let id = match lcg(&mut rng) % 11 {
                8 => crate::dig::lowered(crate::types::BLOCK_GRASS, 1 + (lcg(&mut rng) % 3) as u8),
                9 => {
                    use crate::dig::Side;
                    let side = [Side::PosX, Side::NegX, Side::PosY, Side::NegY, Side::PosZ, Side::NegZ]
                        [(lcg(&mut rng) % 6) as usize];
                    crate::dig::next_bite(crate::types::BLOCK_DIRT, side).expect("dirt bites")
                }
                10 => [crate::types::BLOCK_TILE_SLAB, crate::types::BLOCK_PLANK_STAIRS][(lcg(&mut rng) % 2) as usize],
                0 => BLOCK_STONE,
                1 => BLOCK_GLOWSTONE,
                2 => crate::types::BLOCK_WATER,
                3 => crate::types::BLOCK_LEAVES,
                4 => crate::types::with_layers(
                    crate::types::BLOCK_SNOW,
                    1 + (lcg(&mut rng) % 8) as u8,
                ),
                5 => crate::types::with_layers(
                    crate::types::BLOCK_GRAVEL,
                    1 + (lcg(&mut rng) % 8) as u8,
                ),
                _ => BLOCK_AIR,
            };
            world.set(gx, gy, gz, id);
            light.set_block(&world, gx, gy, gz, id);

            // Compare against a fresh map every so often (a full rebuild
            // per edit would make this test far too slow).
            if step % 50 == 49 {
                let mut fresh = LightMap::new();
                for &p in &positions {
                    fresh.load_chunk(&world, p);
                }
                for &p in &positions {
                    let ox = p.x * 16;
                    let oz = p.z * 16;
                    for lx in 0..16 {
                        for lz in 0..16 {
                            for gy in 0..CHUNK_SIZE_Y as i32 {
                                let (x, z) = (ox + lx, oz + lz);
                                assert_eq!(
                                    light.sky(x, gy, z),
                                    fresh.sky(x, gy, z),
                                    "sky light drifted at ({x},{gy},{z}) after {} edits",
                                    step + 1
                                );
                                assert_eq!(
                                    light.block(x, gy, z),
                                    fresh.block(x, gy, z),
                                    "block light drifted at ({x},{gy},{z}) after {} edits",
                                    step + 1
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Every partial block that stops light: the lips of a slope, a heap, a
    /// bite out of a side, a slab, a step, a roof.
    fn light_holders() -> Vec<(&'static str, BlockId)> {
        use crate::dig::{self, Side};
        use crate::types::{faced, Facing, BLOCK_DIRT, BLOCK_GRASS, BLOCK_PLANK_STAIRS, BLOCK_THATCH_ROOF, BLOCK_TILE_SLAB};
        vec![
            ("a lip a quarter down", dig::lowered(BLOCK_GRASS, 3)),
            ("a lip half down", dig::lowered(BLOCK_GRASS, 2)),
            ("a lip three quarters down", dig::lowered(BLOCK_GRASS, 1)),
            ("a heap", dig::heaped(BLOCK_DIRT)),
            ("a side bite", dig::next_bite(BLOCK_DIRT, Side::PosX).expect("dirt bites")),
            ("a slab", BLOCK_TILE_SLAB),
            ("a step", faced(BLOCK_PLANK_STAIRS, Facing::North)),
            ("a roof", faced(BLOCK_THATCH_ROOF, Facing::East)),
        ]
    }

    /// Lights `edits` on the stone slab both ways -- from scratch, and one
    /// block at a time into a map already lit -- and insists they agree.
    fn lit_both_ways(edits: &[((i32, i32, i32), BlockId)]) -> LightMap {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut incremental) = world_with(&[pos]);
        for &((x, y, z), id) in edits {
            world.set(x, y, z, id);
            incremental.set_block(&world, x, y, z, id);
        }
        let mut fresh = LightMap::new();
        fresh.load_chunk(&world, pos);
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..CHUNK_SIZE_Y as i32 {
                    assert_eq!(incremental.sky(x, y, z), fresh.sky(x, y, z), "sky at ({x},{y},{z})");
                    assert_eq!(incremental.block(x, y, z), fresh.block(x, y, z), "fire at ({x},{y},{z})");
                }
            }
        }
        fresh
    }

    /// "у листвы, когда стоит рядом с неполным, чернеет бок": the lip of a
    /// slope stops light, as the turf it is does, so the flood left its cell
    /// at nought under open sky -- and every face looking into that cell,
    /// the side of a column of leaves beside it, the next block up the
    /// slope, was drawn black where it stands over the lip in daylight.
    /// The air over a partial block is lit like air; what is under it is
    /// lit only from the side, because none of the light goes on through.
    #[test]
    fn a_cell_with_a_partial_block_in_it_is_lit_by_the_air_over_it() {
        for (what, id) in light_holders() {
            // On the ground, and hung in the air with nothing under it.
            let light = lit_both_ways(&[((4, 31, 4), id), ((10, 36, 10), id)]);
            assert_eq!(light.sky(4, 31, 4), MAX_LIGHT, "{what} on the ground stands in daylight");
            assert_eq!(light.sky(10, 36, 10), MAX_LIGHT, "{what} in the air stands in daylight");
            assert_eq!(
                light.sky(10, 35, 10),
                MAX_LIGHT - 1,
                "under {what} the light comes in from the side, not through it"
            );
        }
    }

    /// The other half of the rule: the cell is lit, and nothing past it is.
    /// A room under a roof of slabs stays dark, and so does a cave behind a
    /// wall with a bite out of it, whichever side the bite was taken from --
    /// and a bite into the ceiling, which stands in the top of its cell
    /// against the daylight over it, is not lit through the rock.
    #[test]
    fn a_full_wall_with_a_partial_block_in_it_still_keeps_the_light_out() {
        use crate::dig::{self, Side};
        use crate::types::{BLOCK_DIRT, BLOCK_TILE_SLAB};
        // A room three wide, deep in the stone, at y 20..=22, round (8, 8).
        let mut room = Vec::new();
        for x in 7..=9 {
            for z in 7..=9 {
                for y in 20..=22 {
                    room.push(((x, y, z), BLOCK_AIR));
                }
            }
        }
        // A shaft from the meadow down to the room's ceiling at y 23, and a
        // second one beside the room, with a wall a block thick at x 10.
        let shaft = |x: i32, z: i32, from: i32| (from..=30).map(move |y| ((x, y, z), BLOCK_AIR));
        let dark = |light: &LightMap, what: &str| {
            for x in 7..=9 {
                for z in 7..=9 {
                    for y in 20..=22 {
                        assert_eq!(light.sky(x, y, z), 0, "{what}: the room at ({x},{y},{z})");
                    }
                }
            }
        };

        let roof = BLOCK_TILE_SLAB;
        let mut edits = room.clone();
        edits.extend(shaft(8, 8, 24));
        edits.push(((8, 23, 8), roof));
        let light = lit_both_ways(&edits);
        assert_eq!(light.sky(8, 23, 8), MAX_LIGHT, "the slab under the shaft stands in daylight");
        dark(&light, "a slab roof");

        let up_into_the_ceiling = dig::next_bite(BLOCK_DIRT, Side::NegY).expect("dirt bites");
        let mut edits = room.clone();
        edits.extend(shaft(8, 8, 24));
        edits.push(((8, 23, 8), up_into_the_ceiling));
        let light = lit_both_ways(&edits);
        assert_eq!(light.sky(8, 23, 8), 0, "a bite up into the ceiling is lit from the room, not the meadow");
        dark(&light, "a bite into the ceiling");

        for (side, lit) in [(Side::PosX, true), (Side::NegX, false)] {
            let bitten = dig::next_bite(BLOCK_DIRT, side).expect("dirt bites");
            let mut edits = room.clone();
            edits.extend((20..=22).map(|y| ((11, y, 8), BLOCK_AIR)));
            edits.extend(shaft(11, 8, 23));
            edits.push(((10, 21, 8), bitten));
            let light = lit_both_ways(&edits);
            assert_eq!(light.sky(10, 21, 8) > 0, lit, "a wall bitten from {side:?} is lit from the side it is open to");
            dark(&light, "a bitten wall");
        }
    }

    #[test]
    fn unloading_frees_the_memory() {
        let pos = ChunkPos::new(0, 0);
        let (_w, mut light) = world_with(&[pos]);
        assert!(light.is_lit(pos));
        light.unload_chunk(pos);
        assert!(!light.is_lit(pos));
        assert_eq!(light.loaded_chunks(), 0);
    }
}

#[cfg(test)]
mod regression_tests {
    use super::tests_support::*;
    use super::*;
    use crate::types::{BLOCK_GLOWSTONE, BLOCK_STONE};

    /// The first of the two "sometimes wrong" bugs: an unloaded chunk
    /// used to read as full daylight, so sunlight leaked sideways out of
    /// nothing into sealed rock. Only visible near the frontier of the
    /// loaded area, which is why it seemed intermittent.
    #[test]
    fn light_does_not_leak_in_from_an_unloaded_neighbour() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        // Carve a sealed pocket deep inside the slab, right against the
        // seam with the (unloaded) chunk to the east.
        world.set(15, 20, 8, BLOCK_AIR);
        light.set_block(&world, 15, 20, 8, BLOCK_AIR);

        assert_eq!(
            light.sky(15, 20, 8),
            0,
            "a sealed pocket inside rock must be dark, even next to unloaded space"
        );
    }

    /// The second: breaking a block opens a cell with no light of its
    /// own, so nothing seeded the refill and the new cell stayed black
    /// however close the light source was.
    #[test]
    fn a_newly_broken_cell_is_lit_by_a_nearby_source() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        // A glowstone in a pocket, and solid stone beside it.
        world.set(8, 20, 8, BLOCK_AIR);
        light.set_block(&world, 8, 20, 8, BLOCK_AIR);
        world.set(8, 20, 8, BLOCK_GLOWSTONE);
        light.set_block(&world, 8, 20, 8, BLOCK_GLOWSTONE);

        // Dig the neighbouring block out.
        assert_eq!(world_block(&world, 9, 20, 8), BLOCK_STONE);
        world.set(9, 20, 8, BLOCK_AIR);
        light.set_block(&world, 9, 20, 8, BLOCK_AIR);

        assert!(
            light.block(9, 20, 8) > 0,
            "the cell next to a glowstone must light up when opened"
        );
    }
}

#[cfg(test)]
mod recheck_tests {
    use super::tests_support::*;
    use super::*;
    use crate::types::{BLOCK_GLOWSTONE, BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER};

    /// Unloading and reloading a chunk must land on the same light it
    /// had before. If it doesn't, walking away and back changes how the
    /// world looks -- another way "sometimes wrong" would show up.
    #[test]
    fn a_reloaded_chunk_lights_up_the_same_way() {
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(1, 0),
            ChunkPos::new(0, 1),
        ];
        let (mut world, mut light) = {
            let mut world = World::default();
            for &p in &positions {
                world.insert(slab(p));
            }
            let mut light = LightMap::new();
            for &p in &positions {
                light.load_chunk(&world, p);
            }
            (world, light)
        };

        // Carve a lit cave that straddles the seam.
        for gx in 12..20 {
            world.set(gx, 31, 8, BLOCK_AIR);
            light.set_block(&world, gx, 31, 8, BLOCK_AIR);
        }
        world.set(18, 31, 8, BLOCK_GLOWSTONE);
        light.set_block(&world, 18, 31, 8, BLOCK_GLOWSTONE);

        let before: Vec<(u8, u8)> = (0..16)
            .map(|lx| (light.sky(lx, 31, 8), light.block(lx, 31, 8)))
            .collect();

        // Walk away and come back.
        light.unload_chunk(ChunkPos::new(0, 0));
        light.load_chunk(&world, ChunkPos::new(0, 0));

        let after: Vec<(u8, u8)> = (0..16)
            .map(|lx| (light.sky(lx, 31, 8), light.block(lx, 31, 8)))
            .collect();
        assert_eq!(before, after, "light changed after a reload");
    }

    #[test]
    fn water_dims_light_with_depth_without_blocking_it() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        // A water column open to the sky.
        for y in 25..=30 {
            world.set(8, y, 8, BLOCK_WATER);
            light.set_block(&world, 8, y, 8, BLOCK_WATER);
        }

        let near_surface = light.sky(8, 30, 8);
        let deeper = light.sky(8, 26, 8);
        assert!(near_surface > 0, "the water surface should be lit");
        assert!(
            deeper < near_surface,
            "light must fade with depth ({near_surface} -> {deeper})"
        );
    }

    #[test]
    fn leaves_cast_shade_rather_than_a_hard_shadow() {
        let pos = ChunkPos::new(0, 0);
        let (mut world, mut light) = world_with(&[pos]);

        // Open a shaft and cap it with leaves, versus one capped with
        // stone.
        for y in 25..=30 {
            for (x, z) in [(4, 4), (12, 12)] {
                world.set(x, y, z, BLOCK_AIR);
                light.set_block(&world, x, y, z, BLOCK_AIR);
            }
        }
        world.set(4, 30, 4, BLOCK_LEAVES);
        light.set_block(&world, 4, 30, 4, BLOCK_LEAVES);
        world.set(12, 30, 12, BLOCK_STONE);
        light.set_block(&world, 12, 30, 12, BLOCK_STONE);

        let under_leaves = light.sky(4, 29, 4);
        let under_stone = light.sky(12, 29, 12);
        assert!(
            under_leaves > 0,
            "leaves should let some daylight through, got {under_leaves}"
        );
        assert_eq!(under_stone, 0, "stone should block it entirely");
        assert!(under_leaves < MAX_LIGHT, "leaves should still dim it");
    }

    #[test]
    fn a_light_source_at_the_very_corner_of_a_chunk_reaches_three_neighbours() {
        // Diagonal spread across a chunk corner is the fiddliest seam
        // case: it involves three other chunks at once.
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(1, 0),
            ChunkPos::new(0, 1),
            ChunkPos::new(1, 1),
        ];
        let (mut world, mut light) = {
            let mut world = World::default();
            for &p in &positions {
                world.insert(slab(p));
            }
            let mut light = LightMap::new();
            for &p in &positions {
                light.load_chunk(&world, p);
            }
            (world, light)
        };

        // Hollow out the air layer around the shared corner (16, 16).
        for gx in 13..20 {
            for gz in 13..20 {
                world.set(gx, 31, gz, BLOCK_AIR);
                light.set_block(&world, gx, 31, gz, BLOCK_AIR);
            }
        }
        world.set(16, 31, 16, BLOCK_GLOWSTONE);
        let dirty = light.set_block(&world, 16, 31, 16, BLOCK_GLOWSTONE);

        for &p in &positions {
            assert!(dirty.contains(&p), "chunk {p:?} should have been relit");
        }
        assert!(light.block(14, 31, 14) > 0, "light must cross the corner");
        assert!(light.block(18, 31, 18) > 0);
    }
}

#[cfg(test)]
mod real_terrain_lighting {
    use super::*;

    /// Checksums the isolated light of real generated chunks.
    ///
    /// Run before and after a change to `compute_isolated`: the numbers
    /// must match. A change that only *mostly* seeds the flood produces
    /// darker chunks that the cross-chunk pass then has to repair on the
    /// main thread, which is a stutter rather than a visible fault.
    #[test]
    #[ignore = "diagnostic: prints a checksum"]
    fn checksum_real_terrain_lighting() {
        use crate::worldgen::WorldGen;
        let gen = WorldGen::new(4242);
        let mut total: u64 = 0xcbf29ce484222325;
        let mut lit = 0u64;
        for cx in -3..=3 {
            for cz in -3..=3 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                let data = compute_isolated(&chunk.blocks);
                for b in &data {
                    total ^= *b as u64;
                    total = total.wrapping_mul(0x100000001b3);
                    lit += (*b & 0x0F) as u64;
                }
            }
        }
        println!("isolated light checksum {total:016x} sum {lit}");
    }

    /// `compute_isolated` written the plain way: a column at a time, and
    /// every lit cell queued rather than only the ones with a darker
    /// neighbour.
    ///
    /// Slow and obviously right, which is the entire job. The real one
    /// sweeps planes instead of columns and decides what to seed from the
    /// light array alone -- two arguments about *order* and *what can be
    /// skipped*, neither of which is visible in the answer. This is what
    /// makes "neither of which is visible" a thing the test suite checks
    /// rather than a thing the comments claim.
    fn plainly(blocks: &[BlockId]) -> Vec<u8> {
        let mut data = vec![0u8; CHUNK_VOLUME];
        let mut sky = VecDeque::new();
        let mut block = VecDeque::new();

        for lz in 0..CHUNK_SIZE_Z {
            for lx in 0..CHUNK_SIZE_X {
                let mut level = MAX_LIGHT;
                for y in (0..CHUNK_SIZE_Y).rev() {
                    let idx = Chunk::index(lx, y, lz);
                    let here = sky_below(level, blocks[idx]);
                    data[idx] = (data[idx] & 0xF0) | here;
                    level = sky_onward(here, blocks[idx]);
                    if level == 0 {
                        break;
                    }
                }
            }
        }
        for (idx, cell) in data.iter().enumerate() {
            if cell & 0x0F > 1 {
                sky.push_back(idx);
            }
        }
        for (idx, &id) in blocks.iter().enumerate() {
            let emission = light_emission(id);
            if emission > 0 {
                data[idx] = (data[idx] & 0x0F) | (emission << 4);
                block.push_back(idx);
            }
        }
        flood_local(blocks, &mut data, sky, Channel::Sky);
        flood_local(blocks, &mut data, block, Channel::Block);
        data
    }

    #[test]
    fn sweeping_planes_and_seeding_sparsely_light_a_chunk_the_same_as_the_plain_way() {
        use crate::worldgen::WorldGen;
        let gen = WorldGen::new(4242);
        // Several chunks rather than one: the two versions can only
        // disagree where a skyline is ragged or a cave reaches the
        // surface, and a single chunk is not guaranteed to have either.
        for (cx, cz) in [(0, 0), (5, 5), (-3, 7), (11, -2), (-8, -8)] {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            assert_eq!(
                compute_isolated(&chunk.blocks),
                plainly(&chunk.blocks),
                "chunk ({cx}, {cz})"
            );
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;

    /// What it costs to light one chunk of real terrain, in isolation.
    ///
    /// A measurement rather than an assertion -- run it with
    /// `cargo test -p primitive_shared --release bench_lighting -- --ignored --nocapture`.
    /// Real generated terrain rather than a slab, because the cost is
    /// dominated by how much of the column is rock with caves in it, and
    /// a flat slab has neither.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn bench_lighting() {
        const ROUNDS: usize = 200;
        const BATCHES: usize = 9;

        let gen = crate::worldgen::WorldGen::new(1337);
        let chunks: Vec<_> = [(5, 5), (0, 0), (-3, 7), (11, -2)]
            .map(|(cx, cz)| gen.generate_chunk(ChunkPos::new(cx, cz)))
            .into_iter()
            .collect();

        // The fastest batch, not the average of all of them: a desktop
        // measuring itself is interrupted constantly, and interruptions
        // only ever make a batch slower. Same argument as the client's
        // `bench_meshing`.
        let mut best = f64::MAX;
        for _ in 0..BATCHES {
            let started = Instant::now();
            for _ in 0..ROUNDS {
                for chunk in &chunks {
                    std::hint::black_box(compute_isolated(std::hint::black_box(&chunk.blocks)));
                }
            }
            best = best.min(started.elapsed().as_secs_f64() * 1000.0 / ROUNDS as f64);
        }
        println!("\ncompute_isolated: {:.4} ms per chunk", best / chunks.len() as f64);
    }
}

