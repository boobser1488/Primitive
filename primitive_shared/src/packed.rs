//! Chunks, and their light, kept sixteen blocks of height at a time.
//!
//! ## Why
//!
//! **The world grew four times taller and a quarter more interesting.**
//! At `CHUNK_SIZE_Y = 256` a flat `Chunk` is 131 KB of block ids and its
//! light another 64 KB -- 195 KB a chunk, against 48 KB at sixty-four --
//! and a client at render distance 24 holds 1793 of them, with the
//! singleplayer server in the same process holding its own copy. Almost
//! none of that is information. Measured over those 1793 chunks of the
//! benchmark world (`saves/night`, seed 4242), cut into sections sixteen
//! blocks tall:
//!
//! ```text
//! 28 688 sections
//!   65.1%  one block all through -- nearly all of them sky
//!    0.2%  one block that is not air -- the deep rock has ore and caves in it
//!   34.7%  mixed, of which:
//!            1825 hold  2 kinds of block or fewer
//!            2630 hold  3..4
//!            5338 hold  5..16
//!             150 hold 17..64
//!               0 hold more than 64
//!
//! blocks, per chunk:  flat 131 072 B   uniform-or-u16 45 428   u8 palette 22 777
//!                     nibble palette (<= 16 kinds, else u8)  11 591
//! light, per chunk:   flat  65 536 B   uniform-or-dense      14 714
//!                     a nibble array per channel              7 815
//! ```
//!
//! So a section is stored as one of four things, chosen when it is packed
//! and changed only when a write needs it: one value, a palette of up to
//! sixteen with two cells to a byte, a palette of up to 256 with a byte a
//! cell, or -- for a section nobody has built yet -- plain ids.
//!
//! ## The three ways it could have gone
//!
//! * **Run-length in memory, as on the wire** (`types::rle_blocks`). The
//!   smallest of all, and every read a scan: physics asks about single
//!   cells from the middle of columns several times a frame, the server's
//!   water asks six cells per queued cell, and an edit in the middle of a
//!   run splits it. Rejected: it moves the cost from memory to every
//!   reader, and the readers are the hot paths.
//! * **Sections of flat ids with only the uniform ones collapsed.** Simple,
//!   and it keeps a slice for the mesher -- but 45 KB of the 131 are still
//!   there, because the sections that are not uniform are all of the
//!   interesting ones and each is 8 KB of mostly stone.
//! * **Sections with a palette (this).** A read is a section index, one
//!   branch, and at most two array loads; nothing is scanned. What it costs
//!   is that there is no contiguous slice of a chunk any more, so the two
//!   consumers that wanted one -- the mesher's `Neighbourhood::fill` and
//!   the isolated light pass -- copy out of it instead. `copy_run` exists
//!   for the first; the second decodes on a worker thread, where it was
//!   already doing work a hundred times larger.
//!
//! The light keeps a byte a cell in its mixed sections rather than the
//! 7.8 KB a nibble array per channel would get it to. Light is written on
//! the hottest path in lighting -- the flood fill reads and writes one
//! cell's byte per step -- and splitting it would put a shift and a mask
//! on both, for 7 KB a chunk.
//!
//! ## Why sixteen blocks a section
//!
//! It is the height at which a section is 4096 cells, which makes the
//! section a shift of the flat index `Chunk::index` already produces and
//! the cell a mask of it -- no division anywhere. Eight would find a few
//! more uniform sections and pay a second header per sixteen blocks for
//! them; thirty-two would find almost none, because every thirty-two-block
//! slab of a hillside has both ground and sky in it.

use crate::types::{BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME};

/// Blocks of height per section.
pub const SECTION_HEIGHT: usize = 16;
/// Cells in one horizontal plane of a chunk.
const PLANE: usize = CHUNK_SIZE_X * CHUNK_SIZE_Z;
/// Cells in one section.
pub const SECTION_CELLS: usize = PLANE * SECTION_HEIGHT;
/// Sections in one chunk.
pub const SECTIONS: usize = CHUNK_SIZE_Y / SECTION_HEIGHT;
/// `Chunk::index >> SECTION_SHIFT` is the section, `& CELL_MASK` the cell
/// inside it: the index is laid out y-first, so a section is exactly a
/// contiguous run of it.
const SECTION_SHIFT: u32 = SECTION_CELLS.trailing_zeros();
const CELL_MASK: usize = SECTION_CELLS - 1;

// The shift and the mask above are only the section and the cell if all
// three of these hold. A world height that is not a multiple of sixteen,
// or a chunk that is not a power of two across, would compile to reads
// from the wrong cell -- so it does not compile.
const _: () = assert!(CHUNK_SIZE_Y.is_multiple_of(SECTION_HEIGHT));
const _: () = assert!(SECTION_CELLS.is_power_of_two());
const _: () = assert!(SECTIONS * SECTION_CELLS == CHUNK_VOLUME);

/// The most kinds of block a nibble can name.
const NIBBLE_KINDS: usize = 16;
/// ...and a byte.
const BYTE_KINDS: usize = 256;

/// A boxed array made on the heap, never on the stack on its way there.
///
/// `Box::new([0; N])` builds the array in the caller's frame and then
/// moves it; for four kilobytes that is harmless, but it is also exactly
/// the idiom that overflows a worker's stack the day `N` is a whole chunk.
fn boxed<T: Copy, const N: usize>(value: T) -> Box<[T; N]> {
    match vec![value; N].into_boxed_slice().try_into() {
        Ok(array) => array,
        Err(_) => unreachable!("a vec of N elements is an array of N"),
    }
}

/// Where a palette index for `id` would go: its existing slot, a new one
/// if there is room, or `None` when the palette is full without it.
#[inline]
fn slot_for(palette: &mut Vec<BlockId>, id: BlockId, capacity: usize) -> Option<usize> {
    if let Some(slot) = palette.iter().position(|&kind| kind == id) {
        return Some(slot);
    }
    if palette.len() < capacity {
        palette.push(id);
        return Some(palette.len() - 1);
    }
    None
}

/// The palette slot of one cell of a nibble section.
///
/// Both indices are masked into range rather than trusted, which costs
/// nothing -- `cell` is always inside the section -- and lets the compiler
/// see that neither the byte nor the sixteen-entry palette it goes on to
/// index can be out of bounds, so neither is checked. This runs once per
/// cell of every row the mesher copies.
#[inline]
fn read_nibble(cells: &[u8; SECTION_CELLS / 2], cell: usize) -> usize {
    ((cells[(cell >> 1) & (SECTION_CELLS / 2 - 1)] >> ((cell & 1) * 4)) & 0x0F) as usize
}

#[inline]
fn write_nibble(cells: &mut [u8; SECTION_CELLS / 2], cell: usize, value: usize) {
    let shift = (cell & 1) * 4;
    let byte = &mut cells[(cell >> 1) & (SECTION_CELLS / 2 - 1)];
    *byte = (*byte & !(0x0F << shift)) | ((value as u8) << shift);
}

/// One section's blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Blocks {
    /// Every cell the same. Nothing on the heap.
    Uniform(BlockId),
    /// Up to sixteen kinds, two cells to a byte: 2 KB.
    ///
    /// **The palette is an inline array of exactly sixteen**, `kinds` of
    /// them in use, where the byte form's is a `Vec`. It was a `Vec` here
    /// too, and the mesher's `fill` -- which decodes these a row at a
    /// time, on the main thread, and nearly every mixed section is in
    /// this form -- then paid a bounds check per cell, or a table copied
    /// out of the `Vec` per row, which for the one-cell-wide rows of the
    /// neighbouring chunks was worse. A nibble cannot name more than
    /// fifteen, so indexing sixteen needs no check at all. It costs eight
    /// bytes more per section header, on the stack side of the `Box`.
    ///
    /// Measured by filling the 64 nearest chunks of the benchmark world,
    /// one thread, against the same chunks held flat in the same run
    /// (`engine::arena::world_cost` in the client): 7.18 ms with the `Vec`
    /// against 4.58 flat, **5.05** with this -- ten per cent over a flat
    /// chunk, where it had been sixty.
    Nibbles {
        kinds: u8,
        palette: [BlockId; NIBBLE_KINDS],
        cells: Box<[u8; SECTION_CELLS / 2]>,
    },
    /// Up to 256 kinds, a byte a cell: 4 KB.
    Bytes {
        palette: Vec<BlockId>,
        cells: Box<[u8; SECTION_CELLS]>,
    },
    /// Anything at all: 8 KB. Never produced by generated terrain, which
    /// tops out at 64 kinds a section -- this is for what players build.
    Wide(Box<[BlockId; SECTION_CELLS]>),
}

impl Blocks {
    /// The smallest form that holds these cells.
    fn pack(cells: &[BlockId]) -> Self {
        debug_assert_eq!(cells.len(), SECTION_CELLS);
        let first = cells[0];
        if cells.iter().all(|&id| id == first) {
            return Blocks::Uniform(first);
        }

        // One pass that names every cell by its palette slot. Runs of the
        // same block are the rule -- a layer of stone is thousands of
        // cells -- so the last answer is kept and the palette is only
        // searched when the block changes.
        let mut palette = vec![first];
        let mut slots = [0u8; SECTION_CELLS];
        let (mut last_id, mut last_slot) = (first, 0u8);
        for (slot, &id) in slots.iter_mut().zip(cells) {
            if id != last_id {
                last_slot = match slot_for(&mut palette, id, BYTE_KINDS) {
                    Some(found) => found as u8,
                    None => {
                        let mut wide = boxed::<BlockId, SECTION_CELLS>(BLOCK_AIR);
                        wide.copy_from_slice(cells);
                        return Blocks::Wide(wide);
                    }
                };
                last_id = id;
            }
            *slot = last_slot;
        }

        if palette.len() <= NIBBLE_KINDS {
            let mut packed = boxed::<u8, { SECTION_CELLS / 2 }>(0);
            for (cell, &slot) in slots.iter().enumerate() {
                packed[cell >> 1] |= slot << ((cell & 1) * 4);
            }
            let mut table = [BLOCK_AIR; NIBBLE_KINDS];
            table[..palette.len()].copy_from_slice(&palette);
            Blocks::Nibbles {
                kinds: palette.len() as u8,
                palette: table,
                cells: packed,
            }
        } else {
            palette.shrink_to_fit();
            let mut packed = boxed::<u8, SECTION_CELLS>(0);
            packed.copy_from_slice(&slots);
            Blocks::Bytes { palette, cells: packed }
        }
    }

    #[inline]
    fn get(&self, cell: usize) -> BlockId {
        match self {
            Blocks::Uniform(id) => *id,
            Blocks::Nibbles { palette, cells, .. } => palette[read_nibble(cells, cell)],
            Blocks::Bytes { palette, cells } => palette[cells[cell] as usize],
            Blocks::Wide(cells) => cells[cell],
        }
    }

    fn set(&mut self, cell: usize, id: BlockId) {
        match self {
            Blocks::Uniform(current) => {
                if *current == id {
                    return;
                }
                // Slot 0 is what every cell already was, so the fresh
                // array is right as it stands except for this one cell.
                let mut cells = boxed::<u8, { SECTION_CELLS / 2 }>(0);
                write_nibble(&mut cells, cell, 1);
                let mut palette = [BLOCK_AIR; NIBBLE_KINDS];
                palette[0] = *current;
                palette[1] = id;
                *self = Blocks::Nibbles {
                    kinds: 2,
                    palette,
                    cells,
                };
            }
            Blocks::Nibbles { kinds, palette, cells } => {
                let used = *kinds as usize;
                let slot = match palette[..used].iter().position(|&kind| kind == id) {
                    Some(slot) => Some(slot),
                    None if used < NIBBLE_KINDS => {
                        palette[used] = id;
                        *kinds += 1;
                        Some(used)
                    }
                    None => None,
                };
                if let Some(slot) = slot {
                    write_nibble(cells, cell, slot);
                    return;
                }
                self.repack_with(cell, id);
            }
            Blocks::Bytes { palette, cells } => {
                if let Some(slot) = slot_for(palette, id, BYTE_KINDS) {
                    cells[cell] = slot as u8;
                    return;
                }
                self.repack_with(cell, id);
            }
            Blocks::Wide(cells) => cells[cell] = id,
        }
    }

    /// A write the palette has no room for.
    ///
    /// **Repacked from scratch rather than widened in place**, because a
    /// full palette is not the same thing as a section with that many
    /// kinds in it. Slots are never given back as blocks are overwritten
    /// -- finding out that a kind is gone would be a scan of the section
    /// on every write -- so a section a player has dug through and built
    /// in for an hour can have a palette full of blocks that are no
    /// longer there. Repacking counts what is actually present and picks
    /// the form for that, which is usually the same size as before. It
    /// costs one pass over 4096 cells, and only on the write that
    /// overflows.
    fn repack_with(&mut self, cell: usize, id: BlockId) {
        let mut flat = [BLOCK_AIR; SECTION_CELLS];
        self.copy_run(0, &mut flat);
        flat[cell] = id;
        *self = Blocks::pack(&flat);
    }

    /// `out.len()` cells from `start`, decoded.
    #[inline]
    fn copy_run(&self, start: usize, out: &mut [BlockId]) {
        match self {
            Blocks::Uniform(id) => out.fill(*id),
            Blocks::Nibbles { palette, cells, .. } => {
                // Nothing checked in here: see `read_nibble` and the note
                // on the palette.
                for (offset, slot) in out.iter_mut().enumerate() {
                    *slot = palette[read_nibble(cells, start + offset)];
                }
            }
            Blocks::Bytes { palette, cells } => {
                let len = out.len();
                for (slot, &index) in out.iter_mut().zip(&cells[start..start + len]) {
                    *slot = palette[index as usize];
                }
            }
            Blocks::Wide(cells) => {
                let len = out.len();
                out.copy_from_slice(&cells[start..start + len]);
            }
        }
    }

    /// Whether any cell could hold `id`. A palette that does not name it
    /// is a definite no; one that does is only a maybe (see `repack_with`
    /// on slots that outlive their blocks).
    fn may_hold(&self, id: BlockId) -> bool {
        match self {
            Blocks::Uniform(only) => *only == id,
            Blocks::Nibbles { kinds, palette, .. } => palette[..*kinds as usize].contains(&id),
            Blocks::Bytes { palette, .. } => palette.contains(&id),
            Blocks::Wide(_) => true,
        }
    }

    fn heap_bytes(&self) -> usize {
        let id = std::mem::size_of::<BlockId>();
        match self {
            Blocks::Uniform(_) => 0,
            Blocks::Nibbles { .. } => SECTION_CELLS / 2,
            Blocks::Bytes { palette, .. } => palette.capacity() * id + SECTION_CELLS,
            Blocks::Wide(_) => SECTION_CELLS * id,
        }
    }
}

/// A chunk's blocks, a section at a time.
///
/// **The form a chunk is *kept* in**, by the client's `ChunkManager` and
/// the server's cache. `Chunk` is still what the generator produces and
/// what the protocol carries, because both of those want one flat array
/// they can write into or run-length encode; a chunk is packed when it
/// arrives somewhere to stay and unpacked when it has to leave.
///
/// Addressed by the same flat index as `Chunk` (`Chunk::index`), so code
/// that already has an index needs nothing new.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedChunk {
    pub pos: ChunkPos,
    sections: [Blocks; SECTIONS],
}

impl PackedChunk {
    /// Packs a well-formed chunk.
    ///
    /// **Panics on a chunk of the wrong length**, and that is a promise
    /// kept elsewhere rather than a risk taken here: everything that
    /// arrives off a socket is checked by `Chunk::is_well_formed` before
    /// anything is done with it (the client's `network::well_formed`),
    /// and everything else was made by the generator. Padding a short
    /// chunk with air would draw a hole in the world -- see
    /// `is_well_formed` for why that is worse than refusing.
    pub fn pack(chunk: &Chunk) -> Self {
        assert!(
            chunk.is_well_formed(),
            "chunk ({}, {}) has {} blocks, not {CHUNK_VOLUME}; check is_well_formed before packing",
            chunk.pos.x,
            chunk.pos.z,
            chunk.blocks.len()
        );
        Self {
            pos: chunk.pos,
            sections: std::array::from_fn(|section| {
                let start = section * SECTION_CELLS;
                Blocks::pack(&chunk.blocks[start..start + SECTION_CELLS])
            }),
        }
    }

    /// Back to the flat form, for the protocol and for the isolated light
    /// pass.
    pub fn unpack(&self) -> Chunk {
        Chunk {
            pos: self.pos,
            blocks: self.to_blocks(),
        }
    }

    /// The flat block array alone.
    pub fn to_blocks(&self) -> Vec<BlockId> {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for (section, cells) in self.sections.iter().zip(blocks.as_chunks_mut::<SECTION_CELLS>().0.iter_mut()) {
            section.copy_run(0, cells);
        }
        blocks
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.get_index(Chunk::index(x, y, z))
    }

    /// Every cell whose block `wanted` says yes to, as local `(x, y, z)`.
    ///
    /// **For finding the rare thing in a chunk as it arrives** -- a dead
    /// player's body, which the client draws apart from the chunk mesh
    /// (`ChunkManager::bodies`). A section whose palette names nothing
    /// wanted is dismissed without reading a cell, so on real terrain the
    /// question costs a few dozen palette lookups rather than 65 536 reads.
    pub fn cells_where(&self, wanted: impl Fn(BlockId) -> bool, mut found: impl FnMut(usize, usize, usize)) {
        for (index, section) in self.sections.iter().enumerate() {
            let maybe = match section {
                Blocks::Uniform(only) => wanted(*only),
                Blocks::Nibbles { kinds, palette, .. } => palette[..*kinds as usize].iter().any(|&id| wanted(id)),
                Blocks::Bytes { palette, .. } => palette.iter().any(|&id| wanted(id)),
                Blocks::Wide(_) => true,
            };
            if !maybe {
                continue;
            }
            for cell in 0..SECTION_CELLS {
                if wanted(section.get(cell)) {
                    let flat = index * SECTION_CELLS + cell;
                    let (x, rest) = (flat % CHUNK_SIZE_X, flat / CHUNK_SIZE_X);
                    found(x, rest / CHUNK_SIZE_Z, rest % CHUNK_SIZE_Z);
                }
            }
        }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        self.set_index(Chunk::index(x, y, z), id);
    }

    /// The block at a flat `Chunk::index`.
    #[inline]
    pub fn get_index(&self, index: usize) -> BlockId {
        self.sections[index >> SECTION_SHIFT].get(index & CELL_MASK)
    }

    #[inline]
    pub fn set_index(&mut self, index: usize, id: BlockId) {
        self.sections[index >> SECTION_SHIFT].set(index & CELL_MASK, id);
    }

    /// `out.len()` blocks from flat index `start`, decoded into `out`.
    ///
    /// **The run must stay inside one section**, which a row of one plane
    /// always does -- and a row is what the mesher copies. Asserted in
    /// debug builds, because a run that crossed would read the next
    /// section's cells through this one's palette.
    #[inline]
    pub fn copy_run(&self, start: usize, out: &mut [BlockId]) {
        debug_assert!(out.is_empty() || (start >> SECTION_SHIFT) == ((start + out.len() - 1) >> SECTION_SHIFT));
        self.sections[start >> SECTION_SHIFT].copy_run(start & CELL_MASK, out);
    }

    /// One past the highest plane holding anything but air; 0 for a chunk
    /// of sky.
    ///
    /// Walks sections from the top, and a section of sky is one
    /// comparison rather than 4096 -- which on real terrain is two thirds
    /// of the chunk dismissed in a dozen branches.
    pub fn skyline(&self) -> i32 {
        for (index, section) in self.sections.iter().enumerate().rev() {
            if !section.may_hold(BLOCK_AIR) {
                // No air anywhere in it, so its top plane is not air.
                return ((index + 1) * SECTION_HEIGHT) as i32;
            }
            if matches!(section, Blocks::Uniform(_)) {
                continue; // uniform and may hold air: all air
            }
            for plane in (0..SECTION_HEIGHT).rev() {
                let start = plane * PLANE;
                if (start..start + PLANE).any(|cell| section.get(cell) != BLOCK_AIR) {
                    return (index * SECTION_HEIGHT + plane + 1) as i32;
                }
            }
        }
        0
    }

    /// Highest non-air block in a column, or -1. The same answer as
    /// `Chunk::height_at`.
    pub fn height_at(&self, x: usize, z: usize) -> i32 {
        for y in (0..CHUNK_SIZE_Y).rev() {
            if self.sections[y / SECTION_HEIGHT].may_hold(BLOCK_AIR)
                && matches!(self.sections[y / SECTION_HEIGHT], Blocks::Uniform(_))
            {
                continue;
            }
            if self.get(x, y, z) != BLOCK_AIR {
                return y as i32;
            }
        }
        -1
    }

    /// What this chunk keeps on the heap, in bytes. The array of section
    /// headers is inline and not counted.
    pub fn heap_bytes(&self) -> usize {
        self.sections.iter().map(Blocks::heap_bytes).sum()
    }
}

impl From<Chunk> for PackedChunk {
    fn from(chunk: Chunk) -> Self {
        Self::pack(&chunk)
    }
}

impl From<&Chunk> for PackedChunk {
    fn from(chunk: &Chunk) -> Self {
        Self::pack(chunk)
    }
}

/// One section's light: sky in the low nibble, block light in the high,
/// as everywhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Light {
    Uniform(u8),
    Dense(Box<[u8; SECTION_CELLS]>),
}

/// A chunk's light, a section at a time.
///
/// Sky above the ground is fifteen and nothing else, and rock under it is
/// dark all through: 77.5% of sections on real terrain are one value. See
/// the module note for why the rest stay a byte a cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedLight {
    sections: [Light; SECTIONS],
}

impl PackedLight {
    /// Packs a flat nibble-packed light array of exactly `CHUNK_VOLUME`
    /// bytes -- what `lighting::compute_isolated` returns.
    pub fn pack(flat: &[u8]) -> Self {
        assert_eq!(flat.len(), CHUNK_VOLUME, "a light array is one byte a cell");
        Self {
            sections: std::array::from_fn(|section| {
                let cells = &flat[section * SECTION_CELLS..(section + 1) * SECTION_CELLS];
                let first = cells[0];
                if cells.iter().all(|&v| v == first) {
                    Light::Uniform(first)
                } else {
                    let mut dense = boxed::<u8, SECTION_CELLS>(0);
                    dense.copy_from_slice(cells);
                    Light::Dense(dense)
                }
            }),
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> u8 {
        match &self.sections[index >> SECTION_SHIFT] {
            Light::Uniform(value) => *value,
            Light::Dense(cells) => cells[index & CELL_MASK],
        }
    }

    /// The value every cell of `section` holds, if they all hold one.
    ///
    /// For scans that compare two chunks' light -- the seam pass in
    /// `LightMap::insert_packed` -- so a pair of sections of open sky, or
    /// of dark rock, is dismissed without reading 4,096 cells of either.
    #[inline]
    pub fn uniform_section(&self, section: usize) -> Option<u8> {
        match &self.sections[section] {
            Light::Uniform(value) => Some(*value),
            Light::Dense(_) => None,
        }
    }

    /// Writes one cell, making its section dense only if the value
    /// actually differs -- the flood fill sets cells to what they already
    /// hold constantly, and each of those must not cost four kilobytes.
    #[inline]
    pub fn set(&mut self, index: usize, value: u8) {
        let section = &mut self.sections[index >> SECTION_SHIFT];
        match section {
            Light::Uniform(current) => {
                if *current == value {
                    return;
                }
                let mut dense = boxed::<u8, SECTION_CELLS>(*current);
                dense[index & CELL_MASK] = value;
                *section = Light::Dense(dense);
            }
            Light::Dense(cells) => cells[index & CELL_MASK] = value,
        }
    }

    /// `out.len()` bytes from flat index `start`, inside one section --
    /// the same contract as `PackedChunk::copy_run`.
    #[inline]
    pub fn copy_run(&self, start: usize, out: &mut [u8]) {
        debug_assert!(out.is_empty() || (start >> SECTION_SHIFT) == ((start + out.len() - 1) >> SECTION_SHIFT));
        let cell = start & CELL_MASK;
        match &self.sections[start >> SECTION_SHIFT] {
            Light::Uniform(value) => out.fill(*value),
            Light::Dense(cells) => out.copy_from_slice(&cells[cell..cell + out.len()]),
        }
    }

    /// The flat array back.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut flat = vec![0u8; CHUNK_VOLUME];
        for (section, cells) in self.sections.iter().zip(flat.as_chunks_mut::<SECTION_CELLS>().0.iter_mut()) {
            match section {
                Light::Uniform(value) => cells.fill(*value),
                Light::Dense(dense) => cells.copy_from_slice(&dense[..]),
            }
        }
        flat
    }

    pub fn heap_bytes(&self) -> usize {
        self.sections
            .iter()
            .map(|section| match section {
                Light::Uniform(_) => 0,
                Light::Dense(_) => SECTION_CELLS,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_DIRT, BLOCK_GRASS, BLOCK_STONE};

    /// A tiny deterministic generator, so a test that goes red goes red
    /// the same way twice.
    struct Noise(u64);
    impl Noise {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn sky(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn a_section_of_one_block_keeps_no_cells_at_all() {
        // The whole of the saving above the ground: a chunk of sky is
        // sixteen headers and not one byte on the heap, where the flat
        // array was 131 KB of zeroes.
        let packed = PackedChunk::pack(&sky(ChunkPos::new(0, 0)));
        assert_eq!(packed.heap_bytes(), 0);

        let mut half = sky(ChunkPos::new(0, 0));
        for cell in half.blocks.iter_mut().take(CHUNK_VOLUME / 2) {
            *cell = BLOCK_STONE;
        }
        assert_eq!(PackedChunk::pack(&half).heap_bytes(), 0, "stone below, sky above: still no cells");
    }

    #[test]
    fn every_cell_reads_back_what_was_packed_in_every_form() {
        // Four sections' worth of each form, and a check that the form
        // really is the one meant -- a roundtrip that quietly packed
        // everything wide would pass and prove nothing.
        let mut chunk = sky(ChunkPos::new(-3, 7));
        let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
        for (index, cell) in chunk.blocks.iter_mut().enumerate() {
            let kinds = match index / SECTION_CELLS {
                0..=3 => 1,    // uniform
                4..=7 => 11,   // nibbles
                8..=11 => 200, // bytes
                _ => 4000,     // wide
            };
            *cell = (noise.next() % kinds) as BlockId + 1;
        }
        let packed = PackedChunk::pack(&chunk);
        let forms: Vec<&str> = packed
            .sections
            .iter()
            .map(|s| match s {
                Blocks::Uniform(_) => "uniform",
                Blocks::Nibbles { .. } => "nibbles",
                Blocks::Bytes { .. } => "bytes",
                Blocks::Wide(_) => "wide",
            })
            .collect();
        assert_eq!(&forms[0..4], ["uniform"; 4]);
        assert_eq!(&forms[4..8], ["nibbles"; 4]);
        assert_eq!(&forms[8..12], ["bytes"; 4]);
        assert_eq!(&forms[12..16], ["wide"; 4]);

        for (index, &id) in chunk.blocks.iter().enumerate() {
            assert_eq!(packed.get_index(index), id, "cell {index}");
        }
        assert_eq!(packed.to_blocks(), chunk.blocks);
        assert_eq!(packed.unpack().pos, chunk.pos);
    }

    #[test]
    fn writes_of_every_kind_land_and_disturb_nothing_else() {
        // Random writes against a flat mirror, through every transition
        // there is: uniform to nibbles, nibbles to bytes, bytes to wide,
        // and the repacks in between.
        let mut mirror = sky(ChunkPos::new(0, 0));
        for (index, cell) in mirror.blocks.iter_mut().enumerate() {
            if index < SECTION_CELLS * 4 {
                *cell = BLOCK_STONE;
            }
        }
        let mut packed = PackedChunk::pack(&mirror);
        let mut noise = Noise(12345);
        for step in 0..60_000u64 {
            let index = (noise.next() % CHUNK_VOLUME as u64) as usize;
            // The alphabet widens as the test goes on, so every section
            // walks all the way up through the forms.
            let alphabet = 2 + step / 150;
            let id = (noise.next() % alphabet) as BlockId;
            mirror.blocks[index] = id;
            packed.set_index(index, id);
        }
        assert_eq!(packed.to_blocks(), mirror.blocks);
    }

    #[test]
    fn a_seventeenth_kind_in_a_section_widens_it_without_losing_the_other_sixteen() {
        let mut chunk = sky(ChunkPos::new(0, 0));
        for (cell, id) in chunk.blocks.iter_mut().take(SECTION_CELLS).enumerate() {
            *id = (cell % 16) as BlockId + 1;
        }
        let mut packed = PackedChunk::pack(&chunk);
        assert!(matches!(packed.sections[0], Blocks::Nibbles { .. }));
        packed.set_index(100, 999);
        assert!(matches!(packed.sections[0], Blocks::Bytes { .. }));
        chunk.blocks[100] = 999;
        assert_eq!(packed.to_blocks(), chunk.blocks);
    }

    #[test]
    fn a_palette_full_of_blocks_that_are_gone_is_compacted_rather_than_widened() {
        // A section dug through and rebuilt: sixteen kinds came and went,
        // two remain. Its palette is full, and the next new kind must not
        // turn a 2 KB section into a 4 KB one for blocks that do not exist.
        let mut packed = PackedChunk::pack(&sky(ChunkPos::new(0, 0)));
        for kind in 1..=15 {
            packed.set_index(7, kind);
        }
        packed.set_index(7, BLOCK_AIR);
        let before = packed.heap_bytes();
        packed.set_index(8, BLOCK_GRASS);
        assert!(
            matches!(packed.sections[0], Blocks::Nibbles { .. }),
            "a section of two kinds went to {:?} bytes",
            packed.heap_bytes()
        );
        assert!(packed.heap_bytes() <= before);
        assert_eq!(packed.get_index(8), BLOCK_GRASS);
        assert_eq!(packed.get_index(7), BLOCK_AIR);
    }

    #[test]
    fn writing_what_a_uniform_section_already_holds_keeps_it_uniform() {
        let mut packed = PackedChunk::pack(&sky(ChunkPos::new(0, 0)));
        packed.set(3, 200, 9, BLOCK_AIR);
        assert_eq!(packed.heap_bytes(), 0);
    }

    #[test]
    fn a_run_copied_out_is_the_same_as_reading_it_a_cell_at_a_time() {
        let mut chunk = sky(ChunkPos::new(0, 0));
        let mut noise = Noise(777);
        for (index, cell) in chunk.blocks.iter_mut().enumerate() {
            let kinds = [1, 3, 40, 3000][(index / SECTION_CELLS) % 4];
            *cell = (noise.next() % kinds) as BlockId;
        }
        let packed = PackedChunk::pack(&chunk);
        let mut row = [0 as BlockId; CHUNK_SIZE_X];
        for y in 0..CHUNK_SIZE_Y {
            for z in 0..CHUNK_SIZE_Z {
                for (x0, width) in [(0, 16), (0, 1), (15, 1), (5, 7)] {
                    let start = Chunk::index(x0, y, z);
                    packed.copy_run(start, &mut row[..width]);
                    assert_eq!(&row[..width], &chunk.blocks[start..start + width], "y {y} z {z} x {x0}+{width}");
                }
            }
        }
    }

    #[test]
    fn the_skyline_and_column_heights_agree_with_the_flat_chunk() {
        let generator = crate::worldgen::WorldGen::new(4242);
        let mut cases: Vec<Chunk> = [(0, 0), (5, -3), (-20, 11)]
            .into_iter()
            .map(|(x, z)| generator.generate_chunk(ChunkPos::new(x, z)))
            .collect();
        cases.push(sky(ChunkPos::new(1, 1)));
        let mut roof = sky(ChunkPos::new(2, 2));
        roof.blocks[Chunk::index(4, CHUNK_SIZE_Y - 1, 4)] = BLOCK_DIRT;
        cases.push(roof);
        let mut floor = sky(ChunkPos::new(3, 3));
        floor.blocks[Chunk::index(0, 0, 0)] = BLOCK_DIRT;
        cases.push(floor);

        for chunk in &cases {
            let packed = PackedChunk::pack(chunk);
            let flat = (0..CHUNK_SIZE_Y)
                .rev()
                .find(|&y| chunk.blocks[y * PLANE..(y + 1) * PLANE].iter().any(|&b| b != BLOCK_AIR))
                .map_or(0, |y| y as i32 + 1);
            assert_eq!(packed.skyline(), flat, "chunk {:?}", chunk.pos);
            for (x, z) in [(0, 0), (4, 4), (15, 15), (7, 3)] {
                assert_eq!(packed.height_at(x, z), chunk.height_at(x, z), "chunk {:?} column {x},{z}", chunk.pos);
            }
        }
    }

    #[test]
    fn a_generated_chunk_keeps_less_than_a_fifth_of_its_flat_block_array() {
        // The number the feature exists for, as a floor a regression has
        // to break: real terrain measured at 12.4 KB a chunk against 131,
        // headers and all (`tests/chunk_memory.rs`), and a fifth leaves
        // room for the generator to change without this going red for no
        // reason.
        let generator = crate::worldgen::WorldGen::new(4242);
        let (sx, sz) = generator.spawn_column();
        let chunk = generator.generate_chunk(ChunkPos::from_global(sx, sz).0);
        let packed = PackedChunk::pack(&chunk);
        let flat = CHUNK_VOLUME * std::mem::size_of::<BlockId>();
        assert!(
            packed.heap_bytes() * 5 < flat,
            "{} bytes packed against {flat} flat",
            packed.heap_bytes()
        );
        assert_eq!(packed.to_blocks(), chunk.blocks);
    }

    #[test]
    #[should_panic(expected = "check is_well_formed before packing")]
    fn a_chunk_of_the_wrong_length_is_refused_rather_than_padded() {
        let _ = PackedChunk::pack(&Chunk {
            pos: ChunkPos::new(0, 0),
            blocks: vec![BLOCK_STONE; CHUNK_VOLUME - 1],
        });
    }

    #[test]
    fn a_sunlit_or_a_dark_section_of_light_is_one_byte() {
        let mut flat = vec![0x0Fu8; CHUNK_VOLUME];
        for cell in flat.iter_mut().take(SECTION_CELLS * 5) {
            *cell = 0;
        }
        let light = PackedLight::pack(&flat);
        assert_eq!(light.heap_bytes(), 0);
        assert_eq!(light.to_vec(), flat);
    }

    #[test]
    fn light_is_made_dense_only_by_a_write_that_changes_something() {
        let mut light = PackedLight::pack(&vec![0x0F; CHUNK_VOLUME]);
        light.set(Chunk::index(1, 200, 1), 0x0F);
        assert_eq!(light.heap_bytes(), 0, "writing fifteen into the sky cost a section");
        light.set(Chunk::index(1, 200, 1), 0x3F);
        assert_eq!(light.heap_bytes(), SECTION_CELLS);
        assert_eq!(light.get(Chunk::index(1, 200, 1)), 0x3F);
        assert_eq!(light.get(Chunk::index(2, 200, 1)), 0x0F, "the rest of the section kept its value");
    }

    #[test]
    fn a_light_section_says_it_is_uniform_only_while_every_cell_agrees() {
        let mut light = PackedLight::pack(&vec![0x0F; CHUNK_VOLUME]);
        assert_eq!(light.uniform_section(12), Some(0x0F));
        light.set(Chunk::index(0, 12 * SECTION_HEIGHT, 0), 0x0E);
        assert_eq!(light.uniform_section(12), None, "a section with two values called itself uniform");
        assert_eq!(light.uniform_section(11), Some(0x0F), "the write reached the section below");
    }

    #[test]
    fn light_reads_back_cell_for_cell_and_run_for_run() {
        let mut flat = vec![0u8; CHUNK_VOLUME];
        let mut noise = Noise(4242);
        for (index, cell) in flat.iter_mut().enumerate() {
            if (index / SECTION_CELLS) % 2 == 1 {
                *cell = noise.next() as u8;
            }
        }
        let light = PackedLight::pack(&flat);
        for (index, &value) in flat.iter().enumerate() {
            assert_eq!(light.get(index), value);
        }
        let mut row = [0u8; CHUNK_SIZE_X];
        for y in (0..CHUNK_SIZE_Y).step_by(7) {
            let start = Chunk::index(0, y, 9);
            light.copy_run(start, &mut row);
            assert_eq!(&row[..], &flat[start..start + CHUNK_SIZE_X]);
        }
        assert_eq!(light.to_vec(), flat);
    }
}

#[cfg(test)]
mod finding_tests {
    use super::*;

    /// **A body is found wherever it lies in the chunk**, and nothing else
    /// is: in a section of a dozen kinds, in a section of sky, and at the
    /// last cell of the top section -- the three shapes a section is kept in
    /// that a corpse can land in.
    #[test]
    fn every_cell_of_a_wanted_block_is_found_and_no_other() {
        use crate::types::{BLOCK_CORPSE, BLOCK_DIRT, BLOCK_STONE};
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for (i, cell) in blocks.iter_mut().enumerate().take(PLANE * 3) {
            *cell = if i % 3 == 0 { BLOCK_STONE } else { BLOCK_DIRT };
        }
        let wanted = [(3usize, 1usize, 7usize), (15, 255, 15), (0, 100, 9)];
        for &(x, y, z) in &wanted {
            blocks[Chunk::index(x, y, z)] = BLOCK_CORPSE;
        }
        let packed = PackedChunk::pack(&Chunk { pos: ChunkPos::new(0, 0), blocks });
        let mut seen = Vec::new();
        packed.cells_where(|id| id == BLOCK_CORPSE, |x, y, z| seen.push((x, y, z)));
        seen.sort();
        let mut expected = wanted.to_vec();
        expected.sort();
        assert_eq!(seen, expected);
    }
}
