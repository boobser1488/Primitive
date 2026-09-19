//! What works in a pot by itself: young cheese ripening, and must turning to
//! mead.
//!
//! ## The decision
//!
//! Milk sours within the day (`food::rot_per_step`) and honey never goes off
//! at all, so until this a bowl of milk was drunk at the pen and a comb was
//! eaten whenever. Both now have a second life that asks **where you keep
//! it**, and the two answers are opposite on purpose:
//!
//! * **Cheese wants the cold.** Two bowls of milk and a handful of salt make
//!   a young cheese (`BLOCK_CURD`), and a young cheese in the cool -- a
//!   cellar, a northern summer, anywhere at all in the winter -- ripens in two
//!   days into a cheese that keeps as dried meat does. In the warm it goes
//!   off in one. So a household in the south drinks its milk, and a
//!   household that dug a cellar has a larder of it. What it costs is the
//!   salt the meat also wants, two bowls, and the walk to the cellar before
//!   the day is out.
//! * **Mead wants the warm.** Two combs stirred into a jug of fresh water
//!   (`BLOCK_JUG_MUST`) work into mead in a day by a fire or in summer air,
//!   and barely at all in a cold store. Mead is water, a little food and a
//!   glow against the cold (`food::warmth_in`) -- so the honey that was the
//!   best thing in the wood to eat now is also the thing to take north, and
//!   a player decides which it is before the winter and not in it.
//!
//! One clock and one counter, for both: a stage in the variant field, moved
//! on by the rot clock's step the way raw clay dries (`clay`, and the
//! server's `rot::Rot::cured`), with the air where the thing sits deciding
//! how often. The yeast and the cheese's own moulds are living things, and
//! the rule a player has already learned from the cellar -- the cold slows
//! what lives -- is the rule here too.
//!
//! ## Where the state lives
//!
//! **In the variant field**, for `food`'s reason ("going off"): a stage
//! carried in the id costs no field on `Stack`, and the wire, the chests
//! and the profile store already carry ids. Neither kind here is perishable
//! in `food`'s sense, so the field is not read as an age by anything else.
//!
//! ## Rejected
//!
//! * **Rennet.** Real cheese is curdled with a calf's stomach or a sour
//!   plant, and a milk that needs a third ingredient no animal in this world
//!   gives is a cheese nobody makes. Salt is already fetched from the coast
//!   for the meat, and a cheese that competes with the haunch for the same
//!   handful is the decision.
//! * **A press, a barrel, a shelf.** Each is a block that exists to be stood
//!   next to. The place a cheese ripens is the cellar the game already has,
//!   and the place a must works is anywhere warm: the station is the
//!   weather.
//! * **Mead that makes you drunk.** A swaying camera is a chore and a joke;
//!   what alcohol is to somebody crossing snow is a warmth that is there at
//!   once, and that is all this does.

use crate::types::{
    block_kind, BlockId, BLOCK_CHEESE, BLOCK_CURD, BLOCK_JUG_MEAD, BLOCK_JUG_MUST, BLOCK_ROTTEN, VARIANT_MASK,
    VARIANT_SHIFT,
};

/// How many stages a working thing passes through before it is done: the
/// step that would take it past the last is the one that finishes it.
///
/// **Four, so a day of warm air is one lifetime.** The rot clock steps four
/// times a day (`food::ROT_STEPS_PER_DAY`), and "a day in the warm and it
/// is mead" / "a day in the warm and the cheese has gone" are the two sums a
/// player should be able to do without a table.
pub const STAGES: u8 = 4;

/// At or below this, in the degrees `body` measures in, nothing works: the
/// frost stops a yeast and a mould alike.
///
/// Zero, the line the server's `rot::KEEPS_BELOW_C` draws for meat, because
/// "the frost keeps things as they are" is one rule, not two.
pub const STILL_BELOW_C: f32 = 0.0;

/// At or below this, and above [`STILL_BELOW_C`], the air is a cellar's.
///
/// **Twelve, the server's `rot::COOL_BELOW_C`**, and there is a test on the
/// server that says the two agree: a cellar that kept meat and did not ripen
/// cheese would be two cellars with one hole in the ground.
pub const CELLAR_BELOW_C: f32 = 12.0;

/// The air a thing is working in, by the two lines above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Air {
    Frozen,
    Cellar,
    Warm,
}

impl Air {
    pub fn of(temperature_c: f32) -> Air {
        if !temperature_c.is_finite() || temperature_c <= STILL_BELOW_C {
            Air::Frozen
        } else if temperature_c <= CELLAR_BELOW_C {
            Air::Cellar
        } else {
            Air::Warm
        }
    }
}

/// Is this something that is still working -- a young cheese or a must?
#[inline]
pub fn is_working(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_CURD | BLOCK_JUG_MUST)
}

/// How far along it is, 0 to `STAGES - 1`. Zero for anything that is not
/// working, whatever its variant says -- the field is shared.
#[inline]
pub fn stage(block: BlockId) -> u8 {
    if !is_working(block) {
        return 0;
    }
    (((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8).min(STAGES - 1)
}

/// The same working thing at a given stage.
#[inline]
pub fn with_stage(block: BlockId, stage: u8) -> BlockId {
    block_kind(block) | ((BlockId::from(stage.min(STAGES - 1)) << VARIANT_SHIFT) & VARIANT_MASK)
}

/// On how many of the rot clock's steps this moves a stage, in this air, or
/// `None` if it does not move at all.
///
/// * **A young cheese**: every second step in a cellar's air, to ripen --
///   two days; every step in the warm, to spoil -- one day. Frozen, it waits.
/// * **A must**: every step in the warm -- mead in a day; every fourth in a
///   cellar's air -- four days, the yeast asleep but not dead. Frozen, it
///   waits.
pub fn steps_per_stage(block: BlockId, air: Air) -> Option<u32> {
    match (block_kind(block), air) {
        (_, Air::Frozen) => None,
        (BLOCK_CURD, Air::Cellar) => Some(2),
        (BLOCK_CURD, Air::Warm) => Some(1),
        (BLOCK_JUG_MUST, Air::Warm) => Some(1),
        (BLOCK_JUG_MUST, Air::Cellar) => Some(4),
        _ => None,
    }
}

/// What it turns into when it is done, in this air.
///
/// **The same counter ends in a cheese or in rot**, and which is decided by
/// the air on the step that finishes it. A young cheese carried from the
/// warm into a cellar half-way has not been ruined by the walk -- a curd
/// that was going off and is now cool is a curd that ripens -- and one
/// carried out of the cellar on its last day goes off. That is the honest
/// reading of one counter, and a second counter for "how sour" would be a
/// number nobody can see.
pub fn finished(block: BlockId, air: Air) -> BlockId {
    match (block_kind(block), air) {
        (BLOCK_CURD, Air::Warm) => BLOCK_ROTTEN,
        (BLOCK_CURD, _) => BLOCK_CHEESE,
        (BLOCK_JUG_MUST, _) => BLOCK_JUG_MEAD,
        _ => block,
    }
}

/// One step of the rot clock, number `step`, over a working thing in air at
/// `temperature_c`. Anything that is not working comes back as it was, so a
/// pass over a whole pack can ask this of every slot.
pub fn worked(block: BlockId, step: u64, temperature_c: f32) -> BlockId {
    if !is_working(block) {
        return block;
    }
    let air = Air::of(temperature_c);
    let Some(every) = steps_per_stage(block, air) else {
        return block;
    };
    if !step.is_multiple_of(u64::from(every)) {
        return block;
    }
    let next = stage(block) + 1;
    if next >= STAGES {
        finished(block, air)
    } else {
        with_stage(block, next)
    }
}

/// The word the tooltip puts after its name: how far along it is.
pub fn label(block: BlockId) -> Option<&'static str> {
    if !is_working(block) {
        return None;
    }
    Some(match stage(block) {
        0 => "fresh",
        1 | 2 => "working",
        _ => "nearly done",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps the clock over one thing in one air until it stops changing
    /// kind, and answers what it became and after how many steps.
    fn left_in(block: BlockId, temperature_c: f32, most: u64) -> (BlockId, u64) {
        let mut now = block;
        for step in 1..=most {
            now = worked(now, step, temperature_c);
            if !is_working(now) {
                return (now, step);
            }
        }
        (now, most)
    }

    #[test]
    fn a_young_cheese_ripens_in_a_cellar_in_two_days() {
        let (became, steps) = left_in(BLOCK_CURD, 8.0, 100);
        assert_eq!(became, BLOCK_CHEESE);
        assert_eq!(steps, 8, "two days is eight steps of the rot clock");
    }

    #[test]
    fn a_young_cheese_left_in_the_warm_goes_off_in_a_day() {
        let (became, steps) = left_in(BLOCK_CURD, 22.0, 100);
        assert_eq!(became, BLOCK_ROTTEN);
        assert_eq!(steps, 4);
    }

    #[test]
    fn must_turns_to_mead_in_a_warm_day_and_takes_four_in_a_cellar() {
        assert_eq!(left_in(BLOCK_JUG_MUST, 20.0, 100), (BLOCK_JUG_MEAD, 4));
        assert_eq!(left_in(BLOCK_JUG_MUST, 6.0, 100), (BLOCK_JUG_MEAD, 16));
    }

    #[test]
    fn the_frost_stops_a_cheese_and_a_must_alike() {
        for block in [BLOCK_CURD, BLOCK_JUG_MUST] {
            assert_eq!(left_in(block, -5.0, 200), (block, 200), "{block} worked in the frost");
        }
    }

    #[test]
    fn a_curd_carried_into_the_cellar_on_its_last_warm_step_still_ripens() {
        let late = with_stage(BLOCK_CURD, STAGES - 1);
        assert_eq!(worked(late, 2, 8.0), BLOCK_CHEESE);
        assert_eq!(worked(late, 2, 25.0), BLOCK_ROTTEN);
    }

    #[test]
    fn nothing_else_is_touched_by_the_clock_whatever_its_variant() {
        use crate::types::{BLOCK_HONEY, BLOCK_LOG};
        for block in [BLOCK_HONEY, BLOCK_CHEESE, BLOCK_JUG_MEAD, BLOCK_LOG | (3 << VARIANT_SHIFT)] {
            assert_eq!(worked(block, 4, 20.0), block);
            assert_eq!(stage(block), 0);
        }
    }

    #[test]
    fn the_stage_survives_the_trip_through_the_variant_field() {
        for s in 0..STAGES {
            assert_eq!(stage(with_stage(BLOCK_JUG_MUST, s)), s);
            assert_eq!(block_kind(with_stage(BLOCK_JUG_MUST, s)), BLOCK_JUG_MUST);
        }
    }
}
