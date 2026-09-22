//! A snare: a noose on the ground for a hare, and the round of walking to
//! it.
//!
//! ## The decision
//!
//! A hare is half a meal and the fastest thing on four legs a player meets
//! in a meadow (`animals::Species::Hare`), so chasing one with a spear is
//! the worst trade in the game. A snare takes it without the chase: a cord
//! and two sticks, set and left, and the rot clock -- the one a fish trap
//! fills on (`fishing`) -- lets a hare into it or does not. What makes it a
//! decision rather than a chore is **where**, and **how often you come
//! back**:
//!
//! * **Hares keep to cover and away from people.** A snare catches in
//!   proportion to the grass, scrub and brush round it ([`catch_chance`]),
//!   and never within [`KEEPS_OFF`] blocks of anybody -- a hare smells a camp
//!   as a deer smells a hunter. So the snares are out past the edge of camp,
//!   a walk away, and the walk is the price.
//! * **Snares set close together share the same hares.** Each one within
//!   [`SHARED_RUN`] of another divides the catch, so ten snares round one
//!   bush catch what two do, and a trapline is a *line*: spread along the
//!   edge of a wood, and walked.
//! * **A catch left out is somebody else's supper.** A hare hangs in the
//!   noose [`ROBBED_AFTER`] steps of the clock -- a day -- and then a fox has
//!   it, and the snare is left pulled out of true (`BLOCK_SNARE_SPRUNG`) to be
//!   set again by hand. So a line of twenty snares a day's walk long is a
//!   line that feeds the foxes; the right number is the number a player walks
//!   every day.
//!
//! ## Taking the hare out
//!
//! **The hare is left lying where it was caught, and the snare comes back
//! into the pack** (the server's `empty_snare`). A carcass is a body in the
//! world and is butchered where it lies (`animals::butcher`), so a snared
//! hare is skinned with a knife like a speared one -- the knife still
//! decides what a body is worth -- and the snare, being in the pack, is set
//! again wherever the player now thinks the hares are.
//!
//! ## Rejected
//!
//! * **A snare that catches the hares the animal code is walking about.**
//!   The honest version, and a poor one: the hares a player can see are the
//!   few near them, which is exactly where a snare must not catch. The
//!   population a snare fishes from is the one nobody is looking at, which is
//!   why the chance is read off the ground, as a trap's is off the water.
//! * **Snares for deer.** A deer in a noose is a deer that tears the stake
//!   out. Big game is the pit's (`pitfall`) and the spear's.
//! * **Breaking a full snare to take the hare.** The left hand destroys, as
//!   it does on a carcass; the right one takes. A snare broken with a hare in
//!   it loses the hare, and that is the same rule the carcass keeps.

use crate::types::{block_kind, BlockId, BLOCK_SNARE, BLOCK_SNARE_CAUGHT, BLOCK_SNARE_SPRUNG, VARIANT_MASK, VARIANT_SHIFT};

/// The chance of a hare in one step of the rot clock, in perfect cover with
/// nobody about and no other snare near.
///
/// **A quarter**, so a well-placed snare takes a hare about once a day
/// (four steps) -- half a meal a day for a cord and two sticks, which is
/// what makes a line of four worth a morning's walk and makes the hunt still
/// worth doing for the bigger meal.
pub const CATCH_CHANCE: f32 = 0.25;

/// How close a person may be, in blocks, before a hare will not come to the
/// snare at all.
///
/// **Twelve**, a little over the hare's own nose (`Species::Hare`'s
/// awareness is nine): a snare in camp catches nothing, and one at the edge
/// of the clearing catches while the player sleeps a stone's throw away only
/// if the clearing is wide.
pub const KEEPS_OFF: f32 = 12.0;

/// How far apart two snares have to be, in blocks, not to share one run of
/// hares.
pub const SHARED_RUN: i32 = 4;

/// How many steps a hare hangs in the noose before a fox takes it: a day.
pub const ROBBED_AFTER: u8 = 4;

/// How far out from the snare the cover is counted, in blocks each way: a
/// five-by-five square.
pub const COVER_REACH: i32 = 2;

/// The chance of a hare in one step.
///
/// `cover` is the share of the cells round the snare that are cover a hare
/// runs in (0..1, [`is_cover`]); `people_near` whether anybody is within
/// [`KEEPS_OFF`]; `neighbours` how many other snares are within
/// [`SHARED_RUN`].
pub fn catch_chance(cover: f32, people_near: bool, neighbours: u32) -> f32 {
    if people_near || !cover.is_finite() {
        return 0.0;
    }
    CATCH_CHANCE * cover.clamp(0.0, 1.0) / (1 + neighbours) as f32
}

/// Is this a cell a hare runs through: grass, low plants, a bush, a wood's
/// floor?
///
/// **Read off what stands on the ground, not off the ground itself.** Bare
/// turf is a lawn and a hare crossing a lawn is a hare in the open; it is the
/// tufts, the bracken and the brush that it keeps to, and those are the
/// sprites and bushes a player can see.
pub fn is_cover(block: BlockId) -> bool {
    use crate::blocks::{definition, Shape};
    use crate::types::*;
    let kind = block_kind(block);
    if matches!(kind, BLOCK_BERRY_BUSH | BLOCK_BARE_BUSH | BLOCK_BUSH_LEAVES | BLOCK_LEAF_LITTER) {
        return true;
    }
    // Every sprite that stands in a meadow: tall and dry grass, the four
    // tall plants and the four low ones, a flower, wild grain. Worked as a
    // plant, so a stake and a torch -- crossed sprites too -- are not brush.
    let row = definition(block);
    row.shape == Shape::Cross && row.work == crate::blocks::Work::Plant && row.matter != crate::blocks::Matter::Liquid
}

/// Is this a snare in any of its three states?
#[inline]
pub fn is_snare(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_SNARE | BLOCK_SNARE_CAUGHT | BLOCK_SNARE_SPRUNG)
}

/// How many steps the hare in a full snare has hung there. Zero for anything
/// else.
#[inline]
pub fn hung_for(block: BlockId) -> u8 {
    if block_kind(block) != BLOCK_SNARE_CAUGHT {
        return 0;
    }
    ((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8
}

/// A full snare whose hare has hung `steps`.
#[inline]
pub fn caught(steps: u8) -> BlockId {
    BLOCK_SNARE_CAUGHT | ((BlockId::from(steps.min(ROBBED_AFTER - 1)) << VARIANT_SHIFT) & VARIANT_MASK)
}

/// One step of the clock over a snare: `roll` is a number in 0..1 drawn for
/// this snare this step, and `chance` what [`catch_chance`] made of where it
/// is.
///
/// A set snare takes a hare if the roll is under the chance; a full one
/// hangs a step longer, and past [`ROBBED_AFTER`] is robbed; a robbed one
/// waits for a hand.
pub fn step(block: BlockId, chance: f32, roll: f32) -> BlockId {
    match block_kind(block) {
        BLOCK_SNARE if roll < chance => caught(0),
        BLOCK_SNARE_CAUGHT => {
            let hung = hung_for(block) + 1;
            if hung >= ROBBED_AFTER {
                BLOCK_SNARE_SPRUNG
            } else {
                caught(hung)
            }
        }
        _ => block,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snare_in_camp_catches_nothing_however_good_the_cover() {
        assert_eq!(catch_chance(1.0, true, 0), 0.0);
    }

    #[test]
    fn a_snare_on_a_bare_lawn_catches_nothing() {
        assert_eq!(catch_chance(0.0, false, 0), 0.0);
    }

    #[test]
    fn a_well_placed_snare_takes_about_a_hare_a_day() {
        let per_day = catch_chance(1.0, false, 0) * crate::food::ROT_STEPS_PER_DAY as f32;
        assert!((0.8..=1.2).contains(&per_day), "{per_day} hares a day");
    }

    #[test]
    fn two_snares_on_one_run_share_its_hares() {
        let alone = catch_chance(1.0, false, 0);
        let pair = catch_chance(1.0, false, 1);
        assert!((2.0 * pair - alone).abs() < 1e-6, "two snares caught more than one run holds");
    }

    #[test]
    fn a_hare_left_a_day_in_the_noose_is_the_foxes() {
        let mut snare = step(BLOCK_SNARE, 1.0, 0.0);
        assert_eq!(block_kind(snare), BLOCK_SNARE_CAUGHT);
        for _ in 1..ROBBED_AFTER {
            snare = step(snare, 1.0, 0.0);
            assert_eq!(block_kind(snare), BLOCK_SNARE_CAUGHT, "robbed early");
        }
        assert_eq!(step(snare, 1.0, 0.0), BLOCK_SNARE_SPRUNG);
    }

    #[test]
    fn a_robbed_snare_waits_for_a_hand_and_does_not_set_itself() {
        assert_eq!(step(BLOCK_SNARE_SPRUNG, 1.0, 0.0), BLOCK_SNARE_SPRUNG);
    }

    #[test]
    fn a_meadow_is_cover_and_a_lawn_and_a_river_are_not() {
        use crate::types::{BLOCK_GRASS, BLOCK_TALL_GRASS, BLOCK_WATER};
        assert!(is_cover(BLOCK_TALL_GRASS));
        assert!(!is_cover(BLOCK_GRASS));
        assert!(!is_cover(BLOCK_WATER));
    }
}
