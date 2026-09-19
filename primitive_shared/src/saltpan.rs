//! A salt pan: the sea poured into a bed of clay and left to the sun.
//!
//! ## The decision
//!
//! Salt has been boiled out of a jug of the sea at a fire (`crafting`, "boil
//! salt"): a jug, some fuel, and the time a player stands at the fire. A pan
//! is the other way, and the one every warm coast in history used: **no
//! fuel and no standing about, and the weather decides when.** A pan of the
//! sea dries to a crust in two days of open sky, one in the heat of the dry
//! belt ([`gain`]), and gives more salt than a jug boiled dry ([`YIELD`]),
//! because the sun has all day. But **rain on a pan puts it back to the
//! beginning** -- the water is fresh again, and a crust half-scraped is
//! dissolved -- and **a roof over it stops it**, because what dries it is
//! the sun. So a player choosing between the two is choosing between wood
//! and weather: a fire when salt is needed tomorrow, a row of pans in the
//! summer for the winter's salting, and a look at the sky before walking to
//! the shore to scrape them.
//!
//! ## Where the state lives
//!
//! **In three ids and one variant.** Empty (`BLOCK_SALT_PAN`), drying
//! (`BLOCK_SALT_PAN_BRINE`, how far in the variant) and a crust
//! (`BLOCK_SALT_PAN_SALT`): three pictures a player reads across a beach
//! (see `types::BLOCK_SALT_PAN_BRINE`), and one number only the drying one
//! needs.
//!
//! ## Rejected
//!
//! * **A pan that fills itself from the tide.** There is no tide, and a pan
//!   dug below the waterline would be a pond. Carrying the sea to it in a
//!   jug is the work that makes a row of pans a place a player built.
//! * **River water drying to salt.** Fresh water dries to nothing, for the
//!   boiling row's reason (`crafting::refuses`): a pan filled at the river
//!   is refused and said so.
//! * **Brine that dries a little in the shade.** A pan under a lean-to that
//!   still made salt, slowly, would make the roof the right answer to the
//!   rain, and the decision is exactly that there is no roof for it.

use crate::types::{block_kind, BlockId, BLOCK_SALT_PAN, BLOCK_SALT_PAN_BRINE, BLOCK_SALT_PAN_SALT, VARIANT_MASK, VARIANT_SHIFT};

/// How far a pan of brine has to dry, in stages, before it is a crust.
///
/// **Eight**, the whole variant field: two days of temperate sun at one a
/// step, a day of the dry belt's at two.
pub const STAGES: u8 = 8;

/// The salt one crust gives: **two**, where a jug boiled at a fire gives one.
/// The sun had all day with it, and what the pan costs is days and the
/// risk of rain -- a pan that gave what the fire gives would never be worth
/// the walk to the shore twice.
pub const YIELD: u32 = 2;

/// Below this, in degrees, the air does not dry anything worth counting: a
/// pan in the cold sits wet.
pub const DRIES_ABOVE_C: f32 = 5.0;

/// At or above this, the sun dries two stages a step: the dry belt's shore.
pub const HOT_C: f32 = 20.0;

/// Is this a salt pan in any state?
#[inline]
pub fn is_pan(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_SALT_PAN | BLOCK_SALT_PAN_BRINE | BLOCK_SALT_PAN_SALT)
}

/// How far a pan of brine has dried, 0 to `STAGES - 1`; zero for anything
/// else.
#[inline]
pub fn dried(block: BlockId) -> u8 {
    if block_kind(block) != BLOCK_SALT_PAN_BRINE {
        return 0;
    }
    ((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8
}

/// A pan of brine dried this far.
#[inline]
pub fn brine(stage: u8) -> BlockId {
    BLOCK_SALT_PAN_BRINE | ((BlockId::from(stage.min(STAGES - 1)) << VARIANT_SHIFT) & VARIANT_MASK)
}

/// How many stages a step of the clock dries, with nothing falling on the
/// pan and sky over it.
pub fn gain(temperature_c: f32) -> u8 {
    if !temperature_c.is_finite() || temperature_c <= DRIES_ABOVE_C {
        0
    } else if temperature_c >= HOT_C {
        2
    } else {
        1
    }
}

/// One step of the rot clock over a pan.
///
/// `rained_on` is rain reaching the pan this step; `roofed` a roof between
/// it and the sky. Rain takes brine and crust back to fresh brine; a roof
/// stops the sun; otherwise it dries by [`gain`] and a dry one is a crust.
pub fn step(block: BlockId, rained_on: bool, roofed: bool, temperature_c: f32) -> BlockId {
    match block_kind(block) {
        BLOCK_SALT_PAN_BRINE | BLOCK_SALT_PAN_SALT if rained_on => brine(0),
        BLOCK_SALT_PAN_BRINE if !roofed => {
            let next = dried(block) + gain(temperature_c);
            if next >= STAGES {
                BLOCK_SALT_PAN_SALT
            } else {
                brine(next)
            }
        }
        _ => block,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days_to_crust(temperature_c: f32) -> Option<u32> {
        let mut pan = brine(0);
        for step_count in 1..=64 {
            pan = step(pan, false, false, temperature_c);
            if pan == BLOCK_SALT_PAN_SALT {
                return Some(step_count / crate::food::ROT_STEPS_PER_DAY);
            }
        }
        None
    }

    #[test]
    fn a_pan_dries_in_two_temperate_days_and_one_hot_one() {
        assert_eq!(days_to_crust(14.0), Some(2));
        assert_eq!(days_to_crust(28.0), Some(1));
    }

    #[test]
    fn a_pan_in_the_cold_never_dries() {
        assert_eq!(days_to_crust(2.0), None);
    }

    #[test]
    fn rain_puts_a_pan_back_to_the_start_and_dissolves_a_crust() {
        let half = brine(5);
        assert_eq!(step(half, true, false, 25.0), brine(0));
        assert_eq!(step(BLOCK_SALT_PAN_SALT, true, false, 25.0), brine(0));
    }

    #[test]
    fn a_roof_over_a_pan_stops_the_sun() {
        assert_eq!(step(brine(3), false, true, 30.0), brine(3));
    }

    #[test]
    fn an_empty_pan_waits_for_a_jug() {
        assert_eq!(step(BLOCK_SALT_PAN, false, false, 30.0), BLOCK_SALT_PAN);
        assert_eq!(step(BLOCK_SALT_PAN, true, false, 30.0), BLOCK_SALT_PAN, "rain filled a pan with salt water");
    }

    #[test]
    fn every_count_the_clock_writes_is_an_id_the_anti_cheat_believes_and_one_past_it_is_not() {
        use crate::types::{is_known_block, BLOCK_CURD, BLOCK_JUG_MUST, BLOCK_SNARE_CAUGHT};
        for stage in 0..STAGES {
            assert!(is_known_block(brine(stage)), "a pan at {stage}");
        }
        for hung in 0..crate::snare::ROBBED_AFTER {
            assert!(is_known_block(crate::snare::caught(hung)), "a hare hung {hung}");
        }
        assert!(!is_known_block(BLOCK_SNARE_CAUGHT | (BlockId::from(crate::snare::ROBBED_AFTER) << VARIANT_SHIFT)));
        for kind in [BLOCK_CURD, BLOCK_JUG_MUST] {
            for stage in 0..crate::ferment::STAGES {
                assert!(is_known_block(crate::ferment::with_stage(kind, stage)), "{kind} at {stage}");
            }
            assert!(!is_known_block(kind | (BlockId::from(crate::ferment::STAGES) << VARIANT_SHIFT)));
        }
    }

    #[test]
    fn a_pan_gives_more_salt_than_the_jug_it_holds_boils_to() {
        let boiled = crate::crafting::RECIPES.iter().find(|r| r.name == "boil salt").expect("the boiling row");
        assert!(YIELD > boiled.output.1, "the sun paid no better than the fire");
    }
}
