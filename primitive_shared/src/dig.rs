//! Taking a block apart in slices: what is left of a rock or a soil that
//! has been worked at but not yet worked through.
//!
//! ## What this is for
//!
//! A block used to vanish whole. The pick filled a bar, the bar filled,
//! and a cubic metre of granite blinked out of the world in one frame --
//! which reads as a thing being *deleted* rather than a thing being
//! *quarried*. The player asked for the other reading: "сделай вместо
//! добычи блока породы и земли сразу их постепенное удаление по
//! горизонтали и вертикали".
//!
//! So a rock now comes away a slice at a time, out of the face the digger
//! is standing at. A wall recedes sideways toward them; a floor sinks;
//! a ceiling climbs. The block is still one cell and one id -- what
//! changes is how much of the cell it fills and which side of the cell
//! the rest of it is pushed against.
//!
//! **The dig is not faster or slower than it was.** [`SLICES`] slices, each
//! of `break_seconds_with / SLICES`, add up to exactly the time the same
//! block took before, and the drop comes off the *last* slice
//! (`swing_seconds`). Progression -- which is measured in how long a copper
//! pick saves you over a flint one -- is untouched by design: this is a
//! change to what a dig looks like, not to what it costs.
//!
//! ## Where the state lives
//!
//! In the id, because everything that has to agree about the shape of
//! that cell -- the mesher, the collider, the fluid simulation, the
//! server's anti-cheat, the save on disk -- already reads the id and
//! nothing else. A side table of "cells part-dug" would be a second
//! source of truth that a chunk eviction, a save or a rejoin could lose,
//! and a lost entry is a block that silently becomes whole again under a
//! player's pick.
//!
//! Six bits, and **the clash with what those bits already mean is the
//! whole of the argument**:
//!
//! * The **variant field** ([`types::VARIANT_MASK`], three bits) is not
//!   free on these blocks. A stone or a cobble wears its moss in the
//!   third bit (`ground::MOSSY`) and a cobble its soot in the low two
//!   (`wildfire::soot`). Read as a bite, the boulders the generator lays
//!   mossy would come out of worldgen already half quarried.
//! * Loose material -- sand, gravel, ash, snow -- carries *legacy* bits
//!   there: worlds saved while a drift came in eighths have a depth
//!   written into the same field (see `types::may_carry_variant`). A save
//!   from 1.3 would load as a quarry.
//!
//! Both are answered by a **flag bit** that no id in any save has ever had
//! set on a rock or a soil: [`DUG`], the sixteenth, which means "the
//! fields below me are a bite and not what they usually are". Nothing
//! reads a bite without it, so every id ever written stays exactly what it
//! was. What it costs is that the first slice *takes the moss and the
//! soot with it* -- and that is the right answer rather than a concession:
//! moss and soot are on the surface of the block, and the surface is the
//! part the digger has just taken off.
//!
//! Under the flag: the variant field holds **which face the bite is
//! eating from** (six of its eight values, [`Side`]), and the two bits the
//! kinds never used -- `WOOD_LOW_SHIFT`, a wood only on furniture and
//! furniture is not quarried -- hold **how many slices have gone**, minus
//! one. Three intermediate shapes and then the cell is air.
//!
//! Weighed and rejected:
//!
//! * **A block kind per dug state.** Fifteen rocks, four rubble forms,
//!   fourteen soils and four ores, times six sides times three depths, is
//!   thousands of kinds in a ten-bit field that holds 1024.
//! * **The variant alone, no flag.** Eight codes is six sides and *one*
//!   intermediate shape -- a block that is whole, then half, then gone --
//!   and it reads as a stone breaking in two rather than as a face being
//!   worked. It also walks straight into both clashes above.
//! * **A per-chunk map of part-dug cells.** See above: a second truth,
//!   and the one that gets lost.
//!
//! ## What is dug in slices and what is not
//!
//! Rock, the cobble, gravel and sand it breaks into, the soils, and ore.
//! [`digs_in_slices`] is the list, and the exclusions in it each have a
//! reason:
//!
//! * **Nothing drawn as anything but its cube.** A pebble is a flat quad
//!   two centimetres thick (`mesh`'s model list); a quarter of one is not
//!   a shape, it is a rounding error.
//! * **Plants, furniture and timber**, which the player's words put
//!   outside this from the start. A stool does not recede.
//!
//! **What falls is in, and its bite does not survive the fall.** Sand,
//! gravel and eight of the ten soils come down when the cell under them
//! opens, and leaving them out would have left "земля" out of the one
//! feature the player asked for it by name -- soils are the material a
//! spade is *for*. What a falling block carries is [`whole`] of itself
//! (`logic::falling`): a shovelful of half-cut earth that loses its footing
//! collapses into a shovelful of earth, rather than landing three cells
//! down still bitten out of a face that is now somewhere else.
//!
//! ## What the rest of the game sees
//!
//! One box, and every consumer already asks for it:
//! `geometry::block_box` and `for_each_block_box` return the bite's box,
//! so the player stands on a half-dug floor and walks into a half-dug
//! wall; the server's ground probe and the animals' footing read
//! `types::collision_height`, which is the bite's height when the bite
//! came out of the top; the mesher draws the same box.
//!
//! The two that are *decisions* rather than consequences, and both are
//! tested:
//!
//! * **A bitten block still holds sand up.** `can_be_displaced_by_falling`
//!   is unchanged, so a drift over a cell with any rock left in it stays
//!   where it is. What brings sand down is a cell becoming air, and a cell
//!   with three quarters of a granite block in it is not air.
//! * **Water washes the rest of a bitten block away.** A bite is a hole in
//!   the block, and a hole is what a lake needs. Water that reaches one
//!   takes the remainder with it and gives nothing back -- so cutting into
//!   a cistern from the wet side costs you the stone, and going round to
//!   the dry side is the decision. See `logic::water`'s wash.

use crate::types::{
    block_kind, BlockId, VARIANT_MASK, VARIANT_SHIFT, WOOD_HIGH_BIT, WOOD_LOW_SHIFT,
    BLOCK_CLAY, BLOCK_COAL_ORE, BLOCK_GRASS, BLOCK_COPPER_ORE, BLOCK_DIRT, BLOCK_DRY_TURF, BLOCK_IRON_ORE, BLOCK_MUD,
    BLOCK_PEAT, BLOCK_SANDY_SOIL, BLOCK_TIN_ORE,
};

/// How many swings a block comes away in.
///
/// Four, and the number is a shape rather than a difficulty: quarters of a
/// cell are what the eye reads as a face being worked, and each of them
/// lands on a sixteenth-of-a-block grid, which is the grid every model in
/// this game is written to. Three would leave a third of a cell, which is
/// 5.333 sixteenths and lands on nothing. Eight would be a swing a fifth
/// of a second long on soft ground -- a block that dissolves rather than a
/// block that is quarried, and eight remesh dispatches for one dig.
pub const SLICES: u8 = 4;

/// "The fields below me are a bite." The sixteenth bit.
///
/// The same bit `types::WOOD_HIGH_BIT` is on a stool -- and it can be,
/// because a stool is not quarried: which of the two an id means is
/// decided by its kind, exactly as a drying rack and a chest already share
/// the bits below it. See the module doc for why a flag is needed at all.
pub const DUG: BlockId = WOOD_HIGH_BIT;

/// How many slices have gone, minus one: the two bits under the variant
/// field that no kind has ever used.
const GONE_SHIFT: u32 = WOOD_LOW_SHIFT;
/// The field itself.
const GONE_MASK: BlockId = 0b11 << GONE_SHIFT;

/// Which face of the cell the bite is eating in from -- the face the
/// digger is aimed at, named by the way you would step out of the block to
/// reach it.
///
/// The remaining rock is pushed against the *opposite* face, which is what
/// makes a wall recede from the digger and a floor sink under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl Side {
    /// The face a ray came out of, given the step from the block struck to
    /// the empty cell in front of it -- which is exactly what a raycast
    /// hands back (`physics::raycast_block_for` returns both cells, and
    /// the face is the difference).
    ///
    /// `None` for anything that is not one axial step: a caller with a
    /// diagonal has not hit a face.
    #[inline]
    pub fn from_normal(step: (i32, i32, i32)) -> Option<Side> {
        Some(match step {
            (1, 0, 0) => Side::PosX,
            (-1, 0, 0) => Side::NegX,
            (0, 1, 0) => Side::PosY,
            (0, -1, 0) => Side::NegY,
            (0, 0, 1) => Side::PosZ,
            (0, 0, -1) => Side::NegZ,
            _ => return None,
        })
    }

    /// Its place in the variant field.
    #[inline]
    pub fn code(self) -> BlockId {
        self as BlockId
    }

    /// Back from the field. `None` for the two values of eight that name
    /// no face -- a client is free to put anything in the bits, and an id
    /// that names no face is one `is_known_block` has to be able to refuse.
    #[inline]
    pub fn from_code(code: BlockId) -> Option<Side> {
        Some(match code {
            0 => Side::PosX,
            1 => Side::NegX,
            2 => Side::PosY,
            3 => Side::NegY,
            4 => Side::PosZ,
            5 => Side::NegZ,
            _ => return None,
        })
    }
}

/// Does this block come away a slice at a time?
///
/// Rock, its cobble, its gravel, the soils and the ores. See the module
/// doc for the three exclusions and why each is out.
#[inline]
pub fn digs_in_slices(id: BlockId) -> bool {
    let kind = block_kind(id);
    if crate::ground::is_soil(kind) {
        return true;
    }
    if matches!(
        kind,
        // Clay, for the bank a potter cuts into. (The turf is not here: its
        // first swing peels the grass off, see `next_bite`.)
        BLOCK_CLAY
            | BLOCK_DIRT
            | BLOCK_SANDY_SOIL
            | BLOCK_MUD
            | BLOCK_PEAT
            | BLOCK_DRY_TURF
            | BLOCK_COAL_ORE
            | BLOCK_COPPER_ORE
            | BLOCK_TIN_ORE
            | BLOCK_IRON_ORE
    ) {
        return true;
    }
    // A rock, or the cobble, gravel and sand it breaks into. Not the
    // pebble, which is drawn as a flat quad lying on the ground and has no
    // cube to take a quarter out of.
    match crate::ground::rock_of(kind) {
        Some((_, None)) => true,
        Some((_, Some(form))) => !matches!(form, crate::ground::Form::Pebble),
        None => false,
    }
}

/// Is there a bite out of this block?
///
/// The flag first and by itself, because this is asked from
/// `types::is_opaque` and from the mesher's cover table -- a question put
/// to every id there is, and answered for all but a handful of them by one
/// bit test.
#[inline]
pub fn is_dug(id: BlockId) -> bool {
    id & DUG != 0 && bite(id).is_some()
}

/// The bite: which face it came in from, and how many of [`SLICES`] have
/// gone (1, 2 or 3 -- the fourth is the cell becoming air).
///
/// `None` for a whole block, and for an id that has the flag set on
/// something that is not quarried or names no face. That second answer is
/// what `is_known_block` refuses: the bits are on the wire and a modified
/// client is free to invent them.
#[inline]
pub fn bite(id: BlockId) -> Option<(Side, u8)> {
    if id & DUG == 0 || !digs_in_slices(id) {
        return None;
    }
    let side = Side::from_code((id & VARIANT_MASK) >> VARIANT_SHIFT)?;
    Some((side, (((id & GONE_MASK) >> GONE_SHIFT) as u8) + 1))
}

/// The block this bite is out of: the same cell with nothing taken off it
/// yet.
///
/// Its moss and its soot are not in it, and cannot be -- the bite is
/// written over that field. See the module doc.
#[inline]
pub fn whole(id: BlockId) -> BlockId {
    if is_dug(id) {
        id & !(DUG | GONE_MASK | VARIANT_MASK)
    } else {
        id
    }
}

/// What this cell holds after one more swing at `side`, or `None` when
/// that swing is the one that takes the last of it -- which is the caller's
/// signal to break the block the way it has always been broken, drop and
/// all.
///
/// **The bite keeps the face it started at.** A digger who walks round and
/// works the other side of a half-quarried block goes on taking the same
/// slices off the same face. The alternative is a cell with two bites in
/// it, which is two boxes for the collider, two for the mesher, and a
/// shape neither the id nor the eye can describe; and the player's own
/// picture -- a face receding -- is the one-face one.
#[inline]
pub fn next_bite(block: BlockId, side: Side) -> Option<BlockId> {
    // **A spade into turf takes the grass off first.** The turf is the top of
    // almost all the ground there is, and it came away whole -- so the one
    // block everybody digs first showed nothing of the slices ("где моя
    // система удаления блоков"), and then a quarter out of it would have
    // left grass growing on the cut. What a digger does to a meadow is lift
    // the sod: the first swing leaves the earth under it, whole, and the
    // earth then comes away a quarter at a time like any other soil. Nothing
    // drops for the sod -- the blade of grass is not a thing to carry.
    if block_kind(block) == BLOCK_GRASS && !is_dug(block) {
        return Some(BLOCK_DIRT);
    }
    if !digs_in_slices(block) {
        return None;
    }
    let (side, gone) = match bite(block) {
        // Already being worked: the face is the one it has, not the one
        // this swing came from.
        Some((had, gone)) => (had, gone + 1),
        None => (side, 1),
    };
    if gone >= SLICES {
        return None;
    }
    Some(
        block_kind(block)
            | DUG
            | (BlockId::from(gone - 1) << GONE_SHIFT)
            | (side.code() << VARIANT_SHIFT),
    )
}

/// How much of the cell is left, as a fraction: 1.0 for a whole block.
#[inline]
pub fn left(id: BlockId) -> f32 {
    match bite(id) {
        Some((_, gone)) => f32::from(SLICES - gone) / f32::from(SLICES),
        None => 1.0,
    }
}

/// The box the rock still stands in, in cell units (0..1 on each axis), or
/// `None` for a block with no bite out of it.
///
/// One box and never two, which is the whole reason the bite keeps one
/// face: the collider, the mesher and the aim outline each take this and
/// nothing else, so none of them can disagree about where the rock is.
#[inline]
pub fn bite_box(id: BlockId) -> Option<([f32; 3], [f32; 3])> {
    let (side, _) = bite(id)?;
    let left = left(id);
    let (mut min, mut max) = ([0.0f32; 3], [1.0f32; 3]);
    // The rock is pushed against the face *opposite* the one being worked:
    // dug from +X it stands in the low x of the cell, so the wall recedes
    // away from the digger rather than toward them.
    match side {
        Side::PosX => max[0] = left,
        Side::NegX => min[0] = 1.0 - left,
        Side::PosY => max[1] = left,
        Side::NegY => min[1] = 1.0 - left,
        Side::PosZ => max[2] = left,
        Side::NegZ => min[2] = 1.0 - left,
    }
    Some((min, max))
}

/// How long one swing at this block takes with this tool: the whole
/// block's time divided between its slices.
///
/// **This, and not a cheaper block, is what keeps progression untouched.**
/// `break_seconds_with` still answers what a whole block costs -- it is
/// what the stamina bill and the tool comparison are measured in -- and
/// four swings of a quarter of it are the same dig they always were.
#[inline]
pub fn swing_seconds(block: BlockId, tool: Option<BlockId>) -> Option<f32> {
    let whole = crate::types::break_seconds_with(block, tool)?;
    if digs_in_slices(block) {
        Some(whole / f32::from(SLICES))
    } else {
        Some(whole)
    }
}

/// The same, for a tool somebody made well or badly.
///
/// **A divisor on the time and not a second table.** Everything about how
/// long a block takes -- the hardness, the tier, the edge, the slowdown --
/// is argued in `types::break_seconds_with`, and those arguments are about
/// ratios; one divisor at the end leaves every one of them exactly as it
/// was, the way `work_slowdown` does.
///
/// The range is small on purpose (`quality::speed_scale`): see that
/// function for why durability, and not speed, is where a fine tool is
/// meant to be felt.
pub fn swing_seconds_made(
    block: BlockId,
    tool: Option<BlockId>,
    quality: crate::quality::Quality,
) -> Option<f32> {
    swing_seconds(block, tool).map(|seconds| seconds / crate::quality::speed_scale(quality))
}

impl Side {
    /// The face this names as an axis (0 = x, 1 = y, 2 = z) and the sign of
    /// its outward normal -- the shape the mesher's own face table is in
    /// (`mesh::FACE_OUTWARD`), so the one comparison that picks out the cut
    /// face is between two things written the same way.
    #[inline]
    pub fn outward(self) -> (usize, i32) {
        match self {
            Side::PosX => (0, 1),
            Side::NegX => (0, -1),
            Side::PosY => (1, 1),
            Side::NegY => (1, -1),
            Side::PosZ => (2, 1),
            Side::NegZ => (2, -1),
        }
    }
}

/// **The face the pick has opened**, as an axis and an outward sign, or
/// `None` for a block with no bite out of it.
///
/// One face and never the others. The four faces round the edge of the bite
/// were inside the rock as well, but they stand in the wall of their cell
/// against the rock beside it and are culled whenever anything is there; the
/// face the digger is looking at is the only one that is *always* new stone,
/// and the only one the chip picture goes on (`mesh`, `CHIPPED_BIT`).
#[inline]
pub fn cut_face(id: BlockId) -> Option<(usize, i32)> {
    bite(id).map(|(side, _)| side.outward())
}

/// Did a slice just come off -- is `now` the same block as `was` with one
/// more quarter gone?
///
/// **The one question the sound of a quarter coming away is asked on.** A
/// bite arrives as an ordinary block change, and without this the change
/// from granite to three quarters of granite was heard as granite being *set
/// down*, which is the knock of a stone put on a wall -- the opposite of what
/// happened. The last quarter is not one of these: the cell goes to air and
/// is heard as the break it always was.
#[inline]
pub fn took_a_slice(was: BlockId, now: BlockId) -> bool {
    match bite(now) {
        Some(_) => block_kind(was) == block_kind(now) && left(now) < left(was),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_swing_at_turf_lifts_the_grass_and_the_earth_under_it_comes_away_in_quarters() {
        use crate::types::{BLOCK_DIRT, BLOCK_GRASS};
        let peeled = next_bite(BLOCK_GRASS, Side::PosY).expect("turf came away whole");
        assert_eq!(peeled, BLOCK_DIRT, "the first swing left something other than the earth under the grass");
        let mut block = peeled;
        let mut swings = 0;
        while let Some(next) = next_bite(block, Side::PosY) {
            block = next;
            swings += 1;
        }
        assert_eq!(swings, SLICES as usize - 1, "the earth under the turf did not come away a quarter at a time");
    }
    use crate::types::{
        is_known_block, BLOCK_COBBLESTONE, BLOCK_GRANITE, BLOCK_LOG, BLOCK_SAND, BLOCK_STONE,
        BLOCK_TABLE,
    };

    #[test]
    fn a_block_dug_from_the_side_loses_its_slices_toward_the_digger() {
        // Worked from the +X face: what is left stands in the low x of the
        // cell and gets thinner, so the wall recedes away from the digger.
        let mut block = BLOCK_STONE;
        let mut widths = Vec::new();
        for _ in 0..SLICES {
            match next_bite(block, Side::PosX) {
                Some(next) => {
                    block = next;
                    let (min, max) = bite_box(block).expect("a bitten block has a box");
                    assert_eq!(min, [0.0, 0.0, 0.0], "the rock stays against the far face");
                    assert_eq!(max[1], 1.0, "a side bite takes nothing off the height");
                    assert_eq!(max[2], 1.0);
                    widths.push(max[0]);
                }
                None => widths.push(0.0),
            }
        }
        assert_eq!(widths, vec![0.75, 0.5, 0.25, 0.0], "four equal slices");
    }

    #[test]
    fn every_quarter_but_the_last_is_a_slice_and_nothing_else_is() {
        let mut block = BLOCK_GRANITE;
        let mut slices = 0;
        while let Some(next) = next_bite(block, Side::NegZ) {
            assert!(took_a_slice(block, next), "quarter {} came off unheard", slices + 1);
            block = next;
            slices += 1;
        }
        assert_eq!(slices, usize::from(SLICES) - 1, "the last quarter is the break, not a slice");
        // Setting a block down, a block staying what it was, and a different
        // rock arriving in a bitten cell are none of them a slice.
        assert!(!took_a_slice(crate::types::BLOCK_AIR, BLOCK_STONE));
        assert!(!took_a_slice(block, block));
        assert!(!took_a_slice(BLOCK_STONE, next_bite(BLOCK_GRANITE, Side::PosX).unwrap()));
    }

    #[test]
    fn the_cut_face_is_the_one_the_digger_is_looking_at() {
        for (side, face) in [
            (Side::PosX, (0, 1)),
            (Side::NegX, (0, -1)),
            (Side::PosY, (1, 1)),
            (Side::NegY, (1, -1)),
            (Side::PosZ, (2, 1)),
            (Side::NegZ, (2, -1)),
        ] {
            let bitten = next_bite(BLOCK_STONE, side).unwrap();
            assert_eq!(cut_face(bitten), Some(face), "{side:?}");
            // ...and the box ends on exactly that face: its far side is
            // where the pick has got to, not the wall of the cell.
            let (min, max) = bite_box(bitten).unwrap();
            let (axis, sign) = face;
            let edge = if sign > 0 { max[axis] } else { min[axis] };
            assert!(edge > 0.0 && edge < 1.0, "{side:?} cut face stands at {edge}");
        }
        assert_eq!(cut_face(BLOCK_STONE), None, "a whole block has no cut face");
    }

    #[test]
    fn a_floor_is_dug_downward_and_a_ceiling_upward() {
        let floor = next_bite(BLOCK_STONE, Side::PosY).unwrap();
        let (min, max) = bite_box(floor).unwrap();
        assert_eq!((min[1], max[1]), (0.0, 0.75), "a floor sinks under the digger");
        let ceiling = next_bite(BLOCK_STONE, Side::NegY).unwrap();
        let (min, max) = bite_box(ceiling).unwrap();
        assert_eq!((min[1], max[1]), (0.25, 1.0), "a ceiling climbs away from them");
    }

    #[test]
    fn a_bite_keeps_the_face_it_was_started_at() {
        let first = next_bite(BLOCK_GRANITE, Side::PosZ).unwrap();
        let second = next_bite(first, Side::NegX).unwrap();
        assert_eq!(bite(second).unwrap().0, Side::PosZ, "the face does not move");
        assert_eq!(bite(second).unwrap().1, 2);
    }

    #[test]
    fn the_whole_block_comes_back_out_of_every_bite() {
        for kind in [BLOCK_STONE, BLOCK_GRANITE, BLOCK_COBBLESTONE, BLOCK_DIRT] {
            let mut block = kind;
            while let Some(next) = next_bite(block, Side::NegZ) {
                block = next;
                assert_eq!(whole(block), kind, "a bite never changes what the block is");
                assert_eq!(block_kind(block), kind);
            }
        }
    }

    #[test]
    fn plants_furniture_timber_and_a_pebble_on_the_ground_are_not_dug_in_slices() {
        for whole_block in [BLOCK_LOG, BLOCK_TABLE, crate::types::BLOCK_PEBBLE] {
            assert!(!digs_in_slices(whole_block), "block {whole_block} should come away whole");
            assert!(next_bite(whole_block, Side::PosX).is_none());
        }
    }

    #[test]
    fn the_total_time_to_take_a_block_apart_is_what_it_always_was() {
        for block in [BLOCK_STONE, BLOCK_GRANITE, BLOCK_DIRT, BLOCK_COBBLESTONE] {
            for tool in [None, Some(crate::types::BLOCK_COPPER_PICKAXE)] {
                let Some(whole_time) = crate::types::break_seconds_with(block, tool) else {
                    continue;
                };
                let swing = swing_seconds(block, tool).unwrap();
                let total: f32 = (0..SLICES).map(|_| swing).sum();
                assert!(
                    (total - whole_time).abs() < 1e-4,
                    "block {block}: {SLICES} swings of {swing} make {total}, not {whole_time}"
                );
                // ...and a block part-way through is quoted the same
                // swing, so a dig that changes tools mid-way is still the
                // same arithmetic.
                let bitten = next_bite(block, Side::PosX).unwrap();
                assert_eq!(swing_seconds(bitten, tool), Some(swing));
            }
        }
    }

    #[test]
    fn the_block_is_given_up_on_the_last_slice_and_on_no_other() {
        // "Drop what the block drops when the last piece goes, not
        // before." `None` is the caller's signal to run the break path --
        // the drop, the tool's wear, the collapse -- and it has to come
        // exactly once, on the swing that empties the cell.
        for kind in [BLOCK_STONE, BLOCK_GRANITE, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_SAND] {
            let mut block = kind;
            let mut breaks = 0;
            for swing in 1..=SLICES {
                match next_bite(block, Side::PosX) {
                    Some(next) => {
                        assert!(swing < SLICES, "swing {swing} of {SLICES} still left rock");
                        block = next;
                    }
                    None => breaks += 1,
                }
            }
            assert_eq!(breaks, 1, "block {kind} asked to be broken {breaks} times in {SLICES} swings");
        }
    }

    #[test]
    fn a_half_dug_floor_is_as_tall_as_what_is_left_of_it() {
        // What the player stands on, what the server's ground probe finds
        // and what an animal's footing reads are one number
        // (`types::collision_height`), and a floor dug downward has to
        // move it -- otherwise every body in the world hovers over the
        // hole. A wall dug sideways must *not* move it: its top is where
        // it always was.
        use crate::types::{collision_height, has_full_top, is_collidable};
        let floor = next_bite(BLOCK_STONE, Side::PosY).unwrap();
        assert_eq!(collision_height(floor), 0.75);
        let wall = next_bite(BLOCK_STONE, Side::PosX).unwrap();
        assert_eq!(collision_height(wall), 1.0, "a side bite is not a lower floor");
        for block in [floor, wall] {
            assert!(is_collidable(block), "a block with rock left in it stops a body");
            assert!(!has_full_top(block), "nothing is set down on a block being quarried");
        }
    }

    #[test]
    fn a_bite_is_a_block_the_anti_cheat_knows_and_an_invented_one_is_not() {
        let bitten = next_bite(BLOCK_STONE, Side::NegY).unwrap();
        assert!(is_known_block(bitten), "the server writes this id on every swing");
        // The two values of the variant field that name no face, and the
        // flag on something nobody quarries.
        assert!(!is_known_block(BLOCK_STONE | DUG | (6 << VARIANT_SHIFT)));
        assert!(!is_known_block(BLOCK_STONE | DUG | (7 << VARIANT_SHIFT)));
        assert!(!is_known_block(BLOCK_LOG | DUG));
    }

    #[test]
    fn no_save_ever_written_reads_back_as_a_bite() {
        // Every id in the game without the flag is whole, and that is the
        // whole of the migration: a mossy boulder, a sooty cobble and a
        // 1.3 drift of sand with a depth still in its variant field.
        for id in [
            crate::ground::with_moss(BLOCK_STONE),
            crate::ground::with_moss(BLOCK_COBBLESTONE),
            BLOCK_COBBLESTONE | (2 << VARIANT_SHIFT),
            BLOCK_SAND | (3 << VARIANT_SHIFT),
            BLOCK_DIRT,
        ] {
            assert!(!is_dug(id), "id {id} is not a bite");
            assert_eq!(left(id), 1.0);
            assert!(bite_box(id).is_none());
        }
    }
}
