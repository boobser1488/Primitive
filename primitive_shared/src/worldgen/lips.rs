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
//! * **Under what stands on the ground, as what stands there allows** --
//!   see the next part.
//!
//! **So a lip is the one thing this changes about the ground**, and a test
//! says so: a landforms chunk with every lip made whole again, and every
//! feature it had to meet put back on the whole block it was laid on, is to
//! the block the chunk from before lips existed.
//!
//! ## Why there is still a lip on very nearly every slope
//!
//! A player looked at this ground and called it layers: "слои ландшафта
//! странные и слишком гладкие". The lips were half of what they were looking
//! at. A shelf twenty columns wide -- which is what the landforms' country
//! was, measured (`landforms::GRAIN_HEIGHT`) -- gets a rim of 1/4, 1/2, 3/4
//! round its edge and stays flat in the middle, and a hillside of those from
//! a distance is a contour map with the lines drawn in.
//!
//! *Rejected: fewer lips* -- a lip on some slopes and not others, by a hash
//! or a field. It reads as damage rather than as ground, and it costs
//! exactly what the lips were made to buy: a column left whole at the edge
//! of a terrace is a block-high step, and a block is more than a body climbs
//! (`geometry::PLAYER_STEP_HEIGHT`). The ramp is not the fault. The fault
//! was that a shelf twenty wide has no ramp in it to speak of -- three
//! columns of quarters and seventeen of plain -- and the answer is to stop
//! drawing shelves twenty wide. The grain does that: the landforms' bands
//! are now three to seven columns, which is the width a lip *is* a ramp, and
//! the rim and the slope became the same thing.
//!
//! What the grain does change here is that a lip is no longer laid on every
//! rise. A grain crest on hard rock is a drop of more than a block, and a
//! column beside one of those keeps its edge by the rule above: the
//! landforms now have small faces in open country, which is where the
//! outcrops and the scree at their feet come from.
//!
//! ## What stands on a lip
//!
//! This runs after everything that stands or lies on the ground, and it used
//! to lower only a column with air over it or a plant that roots in the lip.
//! Everything else -- a trunk, a bush, a boulder, a pebble, a stick -- kept
//! the whole block it was put on, and a hillside showed it: "деревья растут
//! только на полных блоках, камни также только так появляются". Every tree
//! and every stone on a slope stood on a step of its own with the ramp going
//! past it, and the smooth parts were bare. What the slope says now comes
//! first, and each kind of thing meets the lip the way it can:
//!
//! * **What is drawn at the real top stays on the lip** -- a tuft, a flower,
//!   a berry bush, a pebble, a flint, a stick, ash, fallen leaves: anything
//!   that grows or lies (`types::is_cross`, `types::is_flat`) and that
//!   `types::can_grow_on` lets have the lip for ground. The mesher, the aim
//!   and the cracks put it on the lip's real top (`types::stand_drop`), and
//!   none of them is collided with.
//! * **A standing trunk goes down into the lip's cell: a root flare.** The
//!   cell the lip would have been is the trunk's own log, upright, standing
//!   on the whole block under it; the lips round it form as the slope says,
//!   so the bark shows a quarter to three quarters further down on the low
//!   side, which is how a trunk meets a hillside. A trunk of pieces
//!   (`branches`) gets the log of its bark (`types::piece_log`), which is a
//!   foot wider than the piece over it: what it costs is two sixteenths of
//!   bark round the foot, and it reads as the flare. Felling is unchanged in
//!   kind -- the flare is the stump, standing on the ground, and cutting it
//!   drops what stands on it as cutting the foot of any trunk does
//!   (`logic::felling`). Weighed and rejected:
//!   - *the whole block kept under the trunk*, which is what there was: a
//!     turf plinth with its grass sides showing under every tree on a slope,
//!     the pattern the player saw;
//!   - *the trunk's lowest piece cut short to meet the lip*: a log or a piece
//!     shorter than its cell is a new shape for the collider, the mesher, the
//!     light, the felling, the support rules and the save -- all of it for the
//!     bottom quarter of a tree that nobody walks under;
//!   - *the piece itself carried down* instead of the log: twelve sixteenths
//!     of trunk in a cell that was ground leaves a slot two sixteenths wide
//!     and a block deep round every tree.
//! * **A boulder is bedded**: the lip's cell is the boulder's own stone, and
//!   the stone over it stays -- a rock half sunk in a hillside, standing a
//!   block and a bit out of the low side. A stone with more stone on it is
//!   not a boulder but a course of a wall, and it keeps its whole block.
//!   Rejected: *the stone moved down a
//!   cell*, which leaves its top flush with the terrace above -- a paving
//!   stone, not a boulder -- and *a stone drawn lowered*, which would be a
//!   cube that is not in its cell for the collider, the light and every face
//!   that is culled against it.
//! * **A bush is rooted in the slope the same way**: its bottom leaf fills
//!   the lip's cell. Leaves are pushed through, not walked on
//!   (`types::is_collidable`), so what this costs is a hollow under the bush
//!   a quarter to three quarters deep, and a body inside a bush on a slope
//!   is a body in a bush. Rejected: *the lip under the leaf*, which is a
//!   bush floating over its own gap, and *the whole block kept*, the plinth.
//! * **Anything else keeps the whole block it was put on**: a wall's
//!   footing, a mound, a sapling's stem (a twig, too thin to flare), a bough
//!   or a log lying on the ground, a canopy low enough to touch it. Each of
//!   those either needs a full floor or is something somebody built, and a
//!   plinth under a rarity is not the pattern of a whole hillside.
//!
//! A column whose ground something else already replaced -- a ruin's floor,
//! the dirt under a fallen giant -- is left alone whatever stands on it (see
//! `lay_lips`).
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
//! made whole and every feature kept on its step (`the_landforms_draw_the_
//! ground_they_drew_before_they_were_made_cheaper`).

use super::{Column, ColumnCache, Scale, WorldGen, FEATURE_MARGIN};
use crate::dig::{self, SLICES};
use crate::ground::{self, Form};
use crate::types::{
    block_axis, block_kind, can_grow_on, is_bough, is_branch, is_cross, is_flat, piece_log, Axis, BlockId, Chunk, BLOCK_AIR,
    BLOCK_BUSH_LEAVES, BLOCK_DIRT, BLOCK_DRY_TURF, BLOCK_GRASS, BLOCK_PERMAFROST, BLOCK_SANDY_SOIL, CHUNK_SIZE_X,
    CHUNK_SIZE_Y, CHUNK_SIZE_Z,
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
    /// Everything that stands on a column keeping the whole block under it,
    /// as before the lips met the features (the module note, "What stands on
    /// a lip"): the before of a before-and-after in one binary, for the
    /// timing and for the prints that hold the rest of the ground still.
    pub(super) static FEATURES_KEEP_THEIR_STEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What the top cell of a column becomes where the slope wants `lip` in it,
/// with `above` standing on it and `over_that` on that -- or `None` for the
/// whole block it was. See the module note, "What stands on a lip", for each
/// answer and what was weighed against it.
pub(super) fn under(lip: BlockId, above: BlockId, over_that: BlockId) -> Option<BlockId> {
    if above == BLOCK_AIR {
        return Some(lip);
    }
    #[cfg(test)]
    if FEATURES_KEEP_THEIR_STEP.with(std::cell::Cell::get) {
        return (is_cross(above) && can_grow_on(above, lip)).then_some(lip);
    }
    if (is_cross(above) || is_flat(above)) && can_grow_on(above, lip) {
        return Some(lip);
    }
    // A standing trunk of logs: its own log, upright, a cell further down.
    if crate::wood::is_log(above) && block_axis(above) == Axis::Y {
        return Some(above);
    }
    // A trunk of pieces: a bough with wood standing on it, which a limb
    // lying on the forest floor has not.
    if is_bough(above) && is_branch(over_that) {
        return piece_log(above);
    }
    // A boulder, bedded: a whole stone or its cobble, moss and all, with
    // nothing standing on it -- a boulder is one stone of a cluster, and a
    // course under more of itself is a wall's footing
    // (`a_ruin_is_whole_across_a_chunk_seam`).
    if matches!(ground::rock_of(above), Some((_, None | Some(Form::Cobble)))) && over_that == BLOCK_AIR {
        return Some(above);
    }
    (block_kind(above) == BLOCK_BUSH_LEAVES).then_some(above)
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
    pub(super) fn lay_lips(&self, blocks: &mut [BlockId], columns: &ColumnCache, origin_x: i32, origin_z: i32) {
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
                // What stands on it decides what the cell becomes: the lip,
                // or the foot of the thing itself. See the module note.
                let over_that = if height + 2 < CHUNK_SIZE_Y as i32 {
                    blocks[Chunk::index(lx as usize, height as usize + 2, lz as usize)]
                } else {
                    BLOCK_AIR
                };
                let lip = dig::lowered(ground, quarters);
                let Some(cell) = under(lip, blocks[above_index], over_that) else {
                    continue;
                };
                // **A ruin's ground is the ruin's.** A stone fallen off a
                // wall lies on the ground exactly as a boulder does, and
                // bedded into the lip it ate the turf the site laid -- the
                // generator disagreeing with its own plan, which
                // `a_ruin_is_whole_across_a_chunk_seam` reads cell by cell.
                // A lip is still cut there: the ruin's own test reads its
                // ground through `dig::whole`.
                if cell != lip && self.ruin_claims(origin_x + lx, origin_z + lz) {
                    continue;
                }
                blocks[ground_index] = cell;
            }
        }
    }
}
