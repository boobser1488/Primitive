//! Boards left out in the rain: wet while it falls, dry again after, and
//! over years grey, dark, and at last rotten. The rules both sides agree
//! on; the clock is the server's (`logic::weathering` there).
//!
//! ## What the player asked for
//!
//! > «Сделай намокание досок и их гниение с годами.»
//!
//! And the decision it is for: **a roof.** A plank house with no roof is a
//! house whose top course rots out from under the rain, and a house with
//! one keeps every board under it sound for as long as the world runs. The
//! roof itself is out in the weather and rots in its turn -- it is the part
//! that is meant to be replaced -- so what a player is choosing is where
//! the wood goes: into walls that last, under a roof that does not.
//!
//! ## What is wet
//!
//! **A board with open sky over it, while anything is falling and for a
//! while after** ([`DRIES_IN_DAYS`]). "Open sky" is `types::blocks_the_sky`,
//! the rule the shelter and the fog already share, asked walking down the
//! column: the first thing in it that stops the sky is what gets rained on,
//! and only that. So the top course of a wall is wet and the course under
//! it is not, a board under a crown of leaves is wet (rain comes through
//! leaves) and a board under a single plank of roof is dry.
//!
//! Wetness is not stored. The sky is one weather for the world
//! (`weather`), so how long ago it stopped raining is one number, the
//! server's; and whether a cell has sky over it is the world. A stored
//! "wet" per block would be a third copy of two facts that are already
//! true, and it would have to be written back to dry every board in the
//! world when the sun came out.
//!
//! ## How long it takes, and why by chance
//!
//! A stage is [`WET_DAYS_PER_STAGE`] days of being wet, **on average**,
//! and a board takes a stage when its roll comes up ([`stage_chance`]).
//! At the world's rain (`weather::CHANCE_OF_WEATHER`: about two spells in
//! five are wet) a forty-day year has some sixteen wet days in it, so a
//! board open to the sky greys in its first year and is rotten through in
//! about three -- the years the player asked for, at the world's own
//! calendar (`season::YEAR_DAYS`).
//!
//! A roll, where the soot on a ceiling is a clock that counts (see
//! `wildfire::SOOT_STAGE_SECONDS`), for two reasons:
//!
//! * **There is nowhere to keep a count.** The soot counts in a map of the
//!   few ceilings over the few hearths; the boards out in the rain are
//!   every roof, wall top and floor in the world. A per-board count of wet
//!   seconds is a file the size of everything anybody ever built, kept
//!   for changes that come a year apart.
//! * **A roof that rots evenly rots all at once.** Every board of a roof
//!   laid on one afternoon would turn on the same day, and a roof
//!   collapsing in one piece is a trap, not a warning. By chance, a roof
//!   greys in patches, the first board is rotten a season before the
//!   last, and the patchwork is the notice to re-roof.
//!
//! ## Where the stage is kept
//!
//! **In the variant field, beside the soot**, and they share it. Three
//! options were weighed:
//!
//! * **A spare bit of the field.** The soot takes values 0..=3, which is
//!   two bits of three, and the third bit is free -- but one bit is
//!   "rotten or not", with nothing to see coming. A roof that is sound
//!   until the day it falls in is the trap again.
//! * **An id per stage.** Four woods, planks and pegged, four stages:
//!   thirty-two ids, from a table with a handful left.
//! * **The field's other four values (chosen).** 0..=3 stay the soot,
//!   exactly as every saved ceiling already has them, and 4..=7 are the
//!   four stages of weathering on a board with no soot on it. What that
//!   gives up is a board that is both sooted *and* weathered, and that is
//!   a trade stated in the rule rather than hidden in it: **smoke cures
//!   wood**. A board that has been sooted does not rot (`weathers`) -- the
//!   old reason rafters over an open hearth outlasted the roof they held,
//!   and here only the board straight over a fire is ever sooted
//!   (`wildfire::ceiling_over`), so it is a board, not a house, that the
//!   smoke keeps. And soot does not settle on a board
//!   already grey with weather (`wildfire::with_soot`), because the grey
//!   is darker than the first stage of it and the black would say the
//!   board is sound.
//!
//! A server-side table was the fourth option and loses to the first
//! bullet above: the stage has to be *drawn*, so it has to be on the wire,
//! and the block is what is on the wire.
//!
//! ## What rotten means
//!
//! The last stage, and only the last -- the three before it are the
//! warning. Each consequence is an existing rule asked about one more
//! block, not a new rule:
//!
//! * **It breaks in half the time** ([`ROTTEN_BREAK_FACTOR`], in
//!   `types::break_seconds_with`), because it is soft.
//! * **It gives nothing back** (`types::block_drop`): it crumbles. A roof
//!   pulled down while it is only grey gives its boards back; one left to
//!   rot gives dust.
//! * **It catches slowly** (`wildfire::Fuel::Punk`): wet punk smoulders
//!   before it burns, a log's wait rather than a board's.
//! * **It holds like earth** (`falling::material_looseness` on the
//!   server): a peg in rotten wood holds nothing, and a span of it wants a
//!   post every other cell. This is the one that matters, and it is why
//!   the stages before it exist.

use crate::types::{block_kind, is_pegged, pegged_form, BlockId, VARIANT_MASK, VARIANT_SHIFT};
use crate::wildfire::SOOT_STAGES;

/// How many stages a board weathers through: grey, dark, rotting, and
/// rotten.
pub const STAGES: u8 = 4;

/// The last stage: the board is rotten. See the module note for what that
/// changes.
pub const ROTTEN: u8 = STAGES;

/// Days of being wet a stage takes, on average. See "How long it takes".
///
/// Twelve: at the world's rain that is most of a year a stage, and three
/// years or so from new boards to rotten ones.
pub const WET_DAYS_PER_STAGE: f32 = 12.0;

/// How long a board stays wet after the rain stops, in days.
///
/// A quarter of a day: an afternoon of sun after a morning's rain. Long
/// enough that the clock does not stop the instant the sky clears -- wood
/// does not -- and short enough that a dry season is dry.
pub const DRIES_IN_DAYS: f32 = 0.25;

/// How much of the table's breaking time a rotten board takes.
pub const ROTTEN_BREAK_FACTOR: f32 = 0.5;

/// Can this block weather? Boards of every wood, pegged or not -- asked the
/// way `wildfire::fuel` asks, so a new wood's planks weather the day they
/// exist.
///
/// **Not a log**, whose variant is its axis, and not cobble or brick,
/// which carry soot and do not rot.
#[inline]
pub fn may_weather(block: BlockId) -> bool {
    pegged_form(block).is_some() || is_pegged(block)
}

/// How weathered a board is: nought for sound (sooted or not), up to
/// [`ROTTEN`].
#[inline]
pub fn weathering(block: BlockId) -> u8 {
    if !may_weather(block) {
        return 0;
    }
    let code = ((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8;
    code.saturating_sub(SOOT_STAGES)
}

/// The same board at a stage of weathering; nought is the sound board, with
/// no soot. Anything that cannot weather comes back as it was.
#[inline]
pub fn weathered(block: BlockId, stage: u8) -> BlockId {
    if !may_weather(block) {
        return block;
    }
    let kind = block_kind(block);
    match stage.min(STAGES) {
        0 => kind,
        stage => kind | (BlockId::from(SOOT_STAGES + stage) << VARIANT_SHIFT),
    }
}

/// Is this a rotten board?
#[inline]
pub fn is_rotten(block: BlockId) -> bool {
    weathering(block) >= ROTTEN
}

/// Does the rain still have anything to do to this block?
///
/// A board, not yet rotten, **and not sooted**: smoke cures wood. See
/// "Where the stage is kept".
#[inline]
pub fn weathers(block: BlockId) -> bool {
    may_weather(block) && weathering(block) < ROTTEN && crate::wildfire::soot(block) == 0
}

/// Is a board with open sky over it wet, given whether anything is falling
/// and how many days ago it last was?
#[inline]
pub fn is_wet(falling: bool, days_since_rain: f32) -> bool {
    falling || days_since_rain <= DRIES_IN_DAYS
}

/// The chance a wet board takes its next stage over `wet_days` of being
/// wet.
///
/// **The chance of at least one event of a steady rate**, not the rate
/// times the time: `wet_days / WET_DAYS_PER_STAGE` is the same number for
/// short steps and a chance above one for long ones, and this is never
/// past one. A look takes one stage at most, so a board looked at very
/// seldom ages a little slower than the rate; the server looks at a board
/// every tenth of a day or so at the default day, where the two differ by
/// less than a part in a hundred -- which is what lets it look at a
/// handful of columns a second and not at every board in reach.
#[inline]
pub fn stage_chance(wet_days: f32) -> f32 {
    if wet_days.is_nan() || wet_days <= 0.0 {
        return 0.0;
    }
    1.0 - (-wet_days / WET_DAYS_PER_STAGE).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        block_drop, break_seconds, BLOCK_BIRCH_PLANKS, BLOCK_COBBLESTONE, BLOCK_LOG, BLOCK_PEGGED_FIR_PLANKS,
        BLOCK_PLANKS,
    };
    use crate::wildfire::{fuel, soot, with_soot, Fuel};

    #[test]
    fn every_board_weathers_through_every_stage_and_nothing_else_does() {
        for board in [BLOCK_PLANKS, BLOCK_BIRCH_PLANKS, BLOCK_PEGGED_FIR_PLANKS] {
            for stage in 0..=STAGES {
                let aged = weathered(board, stage);
                assert_eq!(weathering(aged), stage, "{board} at stage {stage} read back wrong");
                assert_eq!(soot(aged), 0, "{board} weathered to {stage} reads as sooted");
                assert!(crate::types::is_known_block(aged), "{board} at stage {stage} is an id the anti-cheat refuses");
            }
            assert!(is_rotten(weathered(board, ROTTEN)));
            assert!(!weathers(weathered(board, ROTTEN)), "a rotten board kept rotting");
        }
        assert_eq!(weathered(BLOCK_LOG, 2), BLOCK_LOG, "a log's axis was overwritten by weather");
        assert_eq!(weathered(BLOCK_COBBLESTONE, 2), BLOCK_COBBLESTONE, "stone rotted");
    }

    #[test]
    fn soot_and_weather_share_a_board_by_whichever_came_first() {
        // Smoke cures: a sooted board does not rot.
        let sooted = with_soot(BLOCK_PLANKS, 2);
        assert!(!weathers(sooted), "a smoked ceiling board rots in the rain");
        assert_eq!(weathering(sooted), 0);
        // ...and a grey board takes no soot, which would read as sound.
        let grey = weathered(BLOCK_PLANKS, 1);
        assert_eq!(with_soot(grey, 3), grey, "soot painted over a weathered board");
        // Every soot stage a saved ceiling already has still means soot.
        for stage in 0..=SOOT_STAGES {
            assert_eq!(soot(with_soot(BLOCK_PLANKS, stage)), stage);
        }
    }

    #[test]
    fn a_rotten_board_breaks_quicker_gives_nothing_and_catches_slowly() {
        for board in [BLOCK_PLANKS, BLOCK_PEGGED_FIR_PLANKS] {
            let rotten = weathered(board, ROTTEN);
            let (sound, soft) = (break_seconds(board).unwrap(), break_seconds(rotten).unwrap());
            assert!((soft - sound * ROTTEN_BREAK_FACTOR).abs() < 1e-4, "rotten {board} took {soft}s against {sound}s");
            assert_eq!(block_drop(rotten), None, "rotten {board} gave a sound board back");
            assert_eq!(fuel(rotten), Some(Fuel::Punk));
            assert!(Fuel::Punk.catch_heat() > Fuel::Boards.catch_heat(), "rot catches quicker than a sound board");
            // Grey is only the warning: it still gives its board back.
            assert_eq!(block_drop(weathered(board, ROTTEN - 1)), block_drop(board));
            assert_eq!(fuel(weathered(board, ROTTEN - 1)), Some(Fuel::Boards));
        }
    }

    #[test]
    fn a_board_is_wet_in_the_rain_and_an_afternoon_after_and_then_dry() {
        assert!(is_wet(true, 10.0));
        assert!(is_wet(false, DRIES_IN_DAYS * 0.5));
        assert!(!is_wet(false, DRIES_IN_DAYS * 1.5), "a board stayed wet through a dry spell");
    }

    #[test]
    fn the_chance_of_a_stage_is_the_same_split_into_two_looks_or_taken_in_one() {
        assert_eq!(stage_chance(0.0), 0.0);
        assert!(stage_chance(1000.0) <= 1.0);
        // Two looks of a day each are exactly as likely to pass a stage as
        // one look of two days.
        let two_short = 1.0 - (1.0 - stage_chance(1.0)).powi(2);
        assert!((two_short - stage_chance(2.0)).abs() < 1e-6);
    }
}
