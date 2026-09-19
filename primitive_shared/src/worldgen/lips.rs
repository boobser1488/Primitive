//! Lips: the top block at the edge of a rise, lowered a quarter at a time,
//! so a gentle slope is a ramp and not a stair.
//!
//! ## What was asked
//!
//! "Сделай в генерации плавные переходы с помощью новой механики ломания
//! блоков (учти голую землю)." The ground is a height field rounded to whole
//! blocks, so every slope -- however gentle -- was a flight of one-block
//! stairs, and a step of a block is more than a body climbs without a jump
//! (`geometry::PLAYER_STEP_HEIGHT`, half of one). Walking up a down was a
//! hop every few metres; a horse or a boar had the same stair to take.
//!
//! The dig already writes a block that fills a quarter, a half or three
//! quarters of its cell from the floor up (`dig::lowered`), and everything
//! that has to agree about a cell's shape -- the collider, the ground probe,
//! the animals' footing, the mesher, the save -- reads that id already. So
//! the generator lays the same ids: at the top of each rise the columns
//! nearest the edge have their top block lowered, the first by three
//! quarters, the next by a half, the next by one, and walking up the slope
//! is four quarter steps where it was one block.
//!
//! ## How many quarters, and where
//!
//! **From the whole-block heights round the column**, not from the smooth
//! field under them. Along each of the four ways out of a column: how far to
//! the first column one lower (`down`), and how far the other way to the
//! first one higher (`up`). The two together are the width of this terrace
//! of the stair along that line, and a column `down` from the edge of a
//! terrace `width` wide stands `down / width` of the way up it -- which is
//! its lip in quarters. A terrace four wide or more is a ramp over its first
//! three columns, 1/4, 1/2, 3/4, and then the whole block; a terrace two wide
//! is a half and a whole, which is a slope of a half a column; a terrace one
//! wide is a slope of one, and stays the stair it is, which is a hillside
//! and not a walk. The most-lowered answer of the four ways wins, so a
//! column at the corner of a rise is a ramp both ways.
//!
//! Rejected: *the fraction of the smooth field* (`terrain_height` before it
//! rounds). It is the right number on open ground and the wrong one exactly
//! where it matters: the lakes, the river cuts and the lake shores are all
//! applied to the *rounded* heights afterwards (`height_on_planet`), so at a
//! bank the fraction describes ground that is not there -- and carrying a
//! second, fractional tile through the tile caches costs memory on every
//! generator thread for a number the neighbours already say. The whole-block
//! heights are what the player walks on; they are what the ramp is fitted
//! to.
//!
//! ## What is lowered and what is not
//!
//! * **Soil, turf, sand and gravel** -- what a hillside is covered in
//!   ([`takes_a_lip`]). Not rock: a slope steep enough to shed its soil is a
//!   face (`surface_for`), and a face is climbed or gone round. Not mud,
//!   clay or peat, which are the water's edge; not permafrost; not snow.
//! * **Not at a cliff.** A column with a drop of more than one block beside
//!   it keeps its edge: a lip there is a ledge over a fall, not a ramp.
//! * **Not at the water.** A column under the waterline, or beside one that
//!   is, keeps its bank whole: a bite is a hole to the water (`logic::water`
//!   washes cut earth away), and a shore of lips would be a shore that
//!   dissolves the first time water moves.
//! * **Not under anything.** This runs last of everything that stands or
//!   lies on the ground, and the cell over the column must be air or a plant
//!   that roots in the lip as it rooted in the block -- a tuft or a flower on
//!   turf (`types::can_grow_on`). A trunk, a bush, a boulder, a wall, a drift
//!   of ash or a stick keeps the whole block it was put on: each of those
//!   needs a full floor, and the first attempt, which ran before the ground
//!   cover, left the ash of a burnt wood missing from every rise in it.
//!
//! **So a lip is the one thing this changes**, and a test says so: a
//! landforms chunk with every lip made whole again is, to the block, the
//! chunk from before lips existed.
//!
//! ## "Учти голую землю"
//!
//! A lowered turf stays turf (`dig::is_turf_lip`): grass on top, the turf's
//! side cropped to the lip down its faces, and every rule that reads grass --
//! the spread, the plants, the grazing -- reads it as grass. A lip laid as
//! earth would have drawn every rise of every meadow as a brown stripe, a
//! hillside that somebody had been digging at. See `dig::is_turf_lip` for
//! what was weighed.
//!
//! ## Old worlds
//!
//! Only `Scale::Landforms` lays lips, and it is a scale no released version
//! has made a world with. A world of an older scale regenerates its new
//! chunks to the block (`an_old_worlds_new_chunks_are_the_old_generators_
//! to_the_block`), and the landforms' own golden prints hold with every lip
//! made whole (`the_landforms_draw_the_ground_they_drew_before_they_were_
//! made_cheaper`).

use super::{Column, ColumnCache, Scale, WorldGen, FEATURE_MARGIN};
use crate::dig::{self, SLICES};
use crate::ground::{self, Form};
use crate::types::{
    block_kind, can_grow_on, is_cross, BlockId, Chunk, BLOCK_AIR, BLOCK_DIRT, BLOCK_DRY_TURF, BLOCK_GRASS, BLOCK_PERMAFROST,
    BLOCK_SANDY_SOIL, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

/// How far along each way a column looks for the edge of its terrace.
///
/// Three, because three lips are all a terrace has room for: the fourth
/// column from an edge is the whole block whatever the slope. Looking
/// further would only find terraces the ramp does not reach.
const REACH: i32 = SLICES as i32 - 1;

// The lookups reach past the chunk into the column cache's margin -- the
// ring one outside the chunk, and `REACH` past that -- and the cache clamps what falls outside it -- which would be a lip decided by the
// wrong ground at every chunk border rather than a panic.
const _: () = assert!(REACH < FEATURE_MARGIN);

const WAYS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[cfg(test)]
thread_local! {
    /// Lips off on this thread, for `what_the_landforms_cost_a_chunk` to
    /// time the generator with and without them in one binary.
    pub(super) static LIPS_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Is this what a lip is cut into? Soil, turf, sand and gravel, whole. See
/// the module note for what is left out and why.
pub(super) fn takes_a_lip(ground: BlockId) -> bool {
    let kind = block_kind(ground);
    // Whole, and nothing in its variant: a generated block that carries a
    // variant (a mossy stone) is not one of these anyway, and one already
    // bitten is not a whole block to lower.
    if ground != kind {
        return false;
    }
    if matches!(kind, BLOCK_GRASS | BLOCK_DIRT | BLOCK_SANDY_SOIL | BLOCK_DRY_TURF) {
        return true;
    }
    if ground::is_soil(kind) {
        return kind != BLOCK_PERMAFROST;
    }
    matches!(ground::rock_of(kind), Some((_, Some(Form::Sand | Form::Gravel))))
}

/// How many quarters of its top block a column keeps, or `None` for the
/// whole block. See the module note for the arithmetic.
fn lip_quarters(columns: &ColumnCache, lx: i32, lz: i32, here: Column) -> Option<u8> {
    let height = here.height;
    // Under the water or at it: a bank, not a slope.
    if height <= here.water {
        return None;
    }
    let mut quarters = SLICES as i32;
    for (dx, dz) in WAYS {
        let beside = columns.at(lx + dx, lz + dz);
        // Beside standing water: the bank keeps its edge.
        if beside.height < beside.water {
            return None;
        }
        let mut down = None;
        for k in 1..=REACH {
            let there = columns.at(lx + dx * k, lz + dz * k).height;
            if there > height {
                break;
            }
            if there < height {
                if there < height - 1 {
                    // A drop of more than a block: a cliff's edge right
                    // here keeps the column whole; further off it is not
                    // this terrace's edge.
                    if k == 1 {
                        return None;
                    }
                } else {
                    down = Some(k);
                }
                break;
            }
        }
        let Some(down) = down else {
            continue;
        };
        // The other way, to the rise this terrace stands under. Not found
        // within reach is a terrace at least as wide as a whole ramp.
        let mut up = REACH + 1;
        for k in 1..=REACH {
            let there = columns.at(lx - dx * k, lz - dz * k).height;
            if there > height {
                up = k;
                break;
            }
            if there < height {
                break;
            }
        }
        let width = (down + up - 1).min(SLICES as i32);
        // `down / width` of the way up the terrace, to the nearest quarter,
        // and never less than one: a lip of nothing is a hole.
        let lip = ((2 * SLICES as i32 * down + width) / (2 * width)).clamp(1, SLICES as i32);
        quarters = quarters.min(lip);
    }
    (quarters < SLICES as i32).then_some(quarters as u8)
}

impl WorldGen {
    /// Lowers the top block at the edge of every gentle rise in the chunk.
    /// See the module note.
    pub(super) fn lay_lips(&self, blocks: &mut [BlockId], columns: &ColumnCache) {
        if self.scale != Scale::Landforms {
            return;
        }
        #[cfg(test)]
        if LIPS_OFF.with(std::cell::Cell::get) {
            return;
        }
        // Every column's own answer, for the chunk and the ring round it,
        // which the foot of a rise is asked about below.
        const SPAN: i32 = CHUNK_SIZE_X as i32 + 2;
        let mut own = [SLICES; (SPAN * SPAN) as usize];
        for z in -1..=CHUNK_SIZE_Z as i32 {
            for x in -1..=CHUNK_SIZE_X as i32 {
                if let Some(quarters) = lip_quarters(columns, x, z, columns.at(x, z)) {
                    own[((z + 1) * SPAN + x + 1) as usize] = quarters;
                }
            }
        }
        let own_at = |x: i32, z: i32| own[((z + 1) * SPAN + x + 1) as usize];
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let mut quarters = own_at(lx, lz);
                if quarters >= SLICES {
                    continue;
                }
                let height = columns.at(lx, lz).height;
                if height < 0 || height + 1 >= CHUNK_SIZE_Y as i32 {
                    continue;
                }
                // **The foot of a rise keeps what the rise needs of it.** A
                // column can be a lip for one way and stand at the foot of
                // a rise the other -- a corner -- and lowered there it
                // turned the rise's quarter step back into a block, or more
                // than one: measured on the downs, one rise in twenty that
                // the ramp had made walkable. The climb from here onto a
                // neighbour one higher is `1 + theirs - ours` in quarters of
                // a block, and it has to stay within a half: a rise of one
                // quarter leaves this column three at least, and anything
                // taller leaves it whole.
                for (dx, dz) in WAYS {
                    if columns.at(lx + dx, lz + dz).height == height + 1 {
                        let theirs = own_at(lx + dx, lz + dz);
                        quarters = quarters.max((theirs + 2).min(SLICES));
                    }
                }
                if quarters >= SLICES {
                    continue;
                }
                let ground_index = Chunk::index(lx as usize, height as usize, lz as usize);
                let above_index = Chunk::index(lx as usize, height as usize + 1, lz as usize);
                let ground = blocks[ground_index];
                // **The column's own ground and nothing laid over it.** A
                // longhouse's earth floor, the bare dirt under a fallen giant
                // -- a pass before this one wrote them, and lowered they were
                // a floor with a step in it and a find that did not match
                // across a chunk seam (`a_ruin_is_whole_across_a_chunk_seam`).
                // What the column was filled with is the ground; anything
                // else in that cell is somebody's.
                let here = columns.at(lx, lz);
                if ground != here.surface.top || !takes_a_lip(ground) {
                    continue;
                }
                let lip = dig::lowered(ground, quarters);
                // Air over it, or a plant that roots in the lip as it did in
                // the whole block -- a tuft on a turf lip. Anything else
                // keeps the floor it was put on. See the module note.
                let above = blocks[above_index];
                if above != BLOCK_AIR && !(is_cross(above) && can_grow_on(above, lip)) {
                    continue;
                }
                blocks[ground_index] = lip;
            }
        }
    }
}
