//! Lightning: what a bolt is drawn to, what it does where it lands, and
//! how long the noise takes to arrive. The clock is the server's
//! (`logic::lightning`); the client draws the flash and times the crack
//! off the same numbers, which is why they are here and not there.
//!
//! ## Why a storm needed something to *do*
//!
//! The sky already had three states and the middle one was a darker
//! shower. It rained harder, it was greyer, and the soundscape rolled
//! thunder over it every half minute -- thunder with nothing behind it,
//! an ambience track for a sky where nothing was happening. A player
//! caught in a storm did what they did in rain: kept walking.
//!
//! A bolt is the thing that makes the sky a *decision*. Not because it
//! is likely to hit you -- it is not, and a storm that killed people who
//! went outside would be a storm nobody goes outside in, which is the
//! same as not having one -- but because of what it is drawn to:
//! **height, exposure and metal**. Crossing an open ridge in a storm
//! with an iron axe on your back is a choice with a price on it; the
//! same crossing under the trees, or an hour later, is free. That is one
//! mechanic producing a decision out of nothing but where the player is
//! standing, which is what this game asks of a mechanic.
//!
//! ## Why it is an event and not a field
//!
//! The obvious implementation is a charge map: every column carries how
//! exposed it is, the storm walks it, the tallest thing under the front
//! takes the bolt. It is the physically honest one and it is a map of
//! the world kept up to date for something that happens once a minute.
//!
//! What is here instead: when a bolt is due, a handful of columns near a
//! player ([`CANDIDATES`]) are *looked at* -- their top block found, the
//! score below applied -- and the best one is struck. The cost of a
//! storm is therefore a few hundred block reads a minute and nothing at
//! all between bolts, and the result is the same one the map would give
//! wherever a player can see it. Away from players nothing is struck,
//! for the reason nothing burns there either (`wildfire::NEAR_PLAYER`):
//! a forest that burned down in an empty world would be the world
//! punishing somebody for being logged out.
//!
//! ## What a bolt leaves
//!
//! Fire, where there was anything to burn ([`wildfire::fuel`]), and the
//! wildfire takes it from there -- wind, dryness, the rain putting it
//! out. Scorched ground where there was not ([`scorched`]): the turf
//! burnt off to bare earth with ash over it, which is a mark on the
//! world that says a bolt came down here and is two blocks the game
//! already has. A glassy fulgurite was the other candidate and it is a
//! new block, a new picture and a new layer of the atlas for something a
//! player finds once; the burnt patch tells the same story for nothing.

use crate::types::{
    is_air, BlockId, BLOCK_ASH, BLOCK_DIRT, BLOCK_GRASS, BLOCK_PEAT, BLOCK_SNOW,
};

/// Seconds between bolts while a storm is overhead.
///
/// **A range and not a rate**, so that a player counts the gap rather
/// than learning a number: near enough one a minute, which over a
/// five-minute squall is a handful of them. Rolled fresh after each one.
///
/// It used to be tempting to make this much shorter -- lightning every
/// ten seconds is *exciting* for the thirty seconds before it becomes
/// weather-as-fireworks, and then a forest near a player burns down
/// every storm and the storm is a disaster rather than a risk.
pub const STRIKE_GAP_SECONDS: std::ops::Range<f32> = 35.0..95.0;

/// How far from a player a bolt may land, in blocks.
///
/// Inside the wildfire's own reach (`wildfire::NEAR_PLAYER`, 64), so
/// anything a bolt sets alight is in the country the fire simulation is
/// awake in. A bolt that started a fire nobody's server was stepping
/// would be a fire that waits, frozen, until somebody walks past it.
pub const REACH: i32 = 48;

/// ...and how near. A bolt is a thing that happens *over there*; one
/// that lands on the player's own head with no warning is a death out of
/// a clear sky as far as they can tell.
///
/// The player's own column is still a candidate when they are holding
/// metal in the open -- see [`METAL`]. That one is not "no warning": it
/// is the sky answering a decision they made, and the game has told them
/// (GUIDE, and the `struck` notice) that it will.
pub const NEAREST: i32 = 6;

/// How many columns are looked at before a bolt is placed. **The bound.**
///
/// Sixteen is enough that the tallest thing in a stand of trees usually
/// wins and not so many that a storm is a survey of the county. See the
/// module note on the charge map that is not here.
pub const CANDIDATES: usize = 16;

/// How high above a candidate column the search starts looking for its
/// top block, in blocks over the player's feet.
///
/// Above the tallest tree the generator grows, so a bolt finds the crown
/// of a pine rather than the ground beside it.
pub const SEARCH_ABOVE: i32 = 32;
/// ...and how far below, so a bolt can still find the floor of a valley
/// the player is standing above.
pub const SEARCH_BELOW: i32 = 24;

/// What one block of height is worth in the draw.
///
/// The whole of the rule is "the tallest thing under the cloud", so
/// height is the term everything else is measured against: a metal tool
/// is worth [`METAL`] blocks of it, standing in a tree is worth
/// [`FUEL`].
pub const PER_BLOCK: f32 = 1.0;

/// What being something that burns is worth.
///
/// Three blocks: a tree beats the hilltop beside it, an oak on a hill
/// beats the hill. A bolt prefers what will burn because *a strike that
/// leaves a scorch mark is a strike a player never hears about*, and the
/// mechanic exists to be noticed.
pub const FUEL: f32 = 3.0;

/// What an iron tool over a player's shoulder in the open is worth.
///
/// **Six blocks of height**, which is to say: standing in the open with
/// metal makes a player about as attractive as a tree, and standing in
/// the open *without* it does not. That is the decision -- the axe is
/// the thing you carry because you are going to cut wood, and the storm
/// is the reason you might leave it under a rock for ten minutes.
///
/// It is not a certainty. Six blocks loses to the pine on the ridge
/// above, which is exactly right: the safe thing to do is to be near
/// something taller.
pub const METAL: f32 = 6.0;

/// How far from the strike a player is hurt at all, in blocks.
pub const HURT_REACH: f32 = 4.0;

/// The damage a direct hit does, in the units `body` counts health in.
///
/// **Not an instant death, and the argument matters.** A bolt that
/// killed outright would be a mechanic that deletes a session with no
/// recourse -- the single thing this game's death rules are written to
/// avoid. What it does instead is take nearly everything: a player at
/// full health lives, badly, and knows precisely what happened and that
/// they were lucky. A player already hurt does not, and they were warned
/// twice over.
pub const DIRECT_DAMAGE: f32 = 16.0;

/// How fast the noise travels, in blocks a second.
///
/// Sound, in metres, and a block is a metre here. This is the whole
/// reason the flash and the crack are two things: a bolt a hundred
/// blocks off flashes now and is heard in three seconds, and a player
/// who counts learns how far away the storm is without a single number
/// on the screen.
pub const THUNDER_SPEED: f32 = 340.0;

/// How long the flash lasts, in seconds.
///
/// Two frames' worth at sixty, and it is drawn as a brightening of the
/// world rather than as a shape in the sky (see `engine::sky::Sky`). A
/// bolt drawn as geometry needs a mesh, a pass and a shape that reads
/// from every angle; the flash is what a player actually notices, and
/// what they notice is that the whole world went white for an instant --
/// including the inside of the cave they were standing in the mouth of.
pub const FLASH_SECONDS: f32 = 0.22;

/// How far off a bolt can still be heard, in blocks. Past this the crack
/// is not played at all rather than played at nothing.
pub const HEARD_WITHIN: f32 = 900.0;

/// How long the crack takes to arrive from this far away, in seconds.
#[inline]
pub fn thunder_delay(distance: f32) -> f32 {
    (distance.max(0.0) / THUNDER_SPEED).min(HEARD_WITHIN / THUNDER_SPEED)
}

/// How loud it is from this far away, 0..1.
///
/// Not an inverse square: thunder rolls off far more slowly than that
/// (it is a line source, and the ground and the cloud bounce it), and a
/// square law puts a bolt at two hundred blocks below the rain. A
/// straight fall to nothing at [`HEARD_WITHIN`], with the near end kept
/// short of one so that a strike beside you is a crack rather than a
/// clipped speaker.
#[inline]
pub fn thunder_gain(distance: f32) -> f32 {
    let t = 1.0 - (distance.max(0.0) / HEARD_WITHIN).clamp(0.0, 1.0);
    0.15 + 0.8 * t * t
}

/// How much a player this far from the strike is hurt.
///
/// Falls to nothing at [`HURT_REACH`]: the bolt itself is what hurts,
/// not the storm. A player under a roof is not asked -- the caller tests
/// the sky over them first, because "I was indoors and lightning hit me"
/// is the report that makes a mechanic feel arbitrary.
#[inline]
pub fn hurt(distance: f32) -> f32 {
    if distance >= HURT_REACH {
        return 0.0;
    }
    let t = 1.0 - distance.max(0.0) / HURT_REACH;
    DIRECT_DAMAGE * t * t
}

/// How attractive a candidate column is.
///
/// Everything in one line, in blocks of height, so the trade is readable
/// as a sentence: *the tallest thing wins; being able to burn is worth
/// three blocks; carrying metal in the open is worth six*.
///
/// `top` and `floor` are the height of this column's top block and of
/// the ground the search started from, so what is scored is how far this
/// column stands *above its neighbours* rather than its altitude: a
/// valley floor two hundred blocks up is not a target, and the one tree
/// on it is.
#[inline]
pub fn attraction(top: i32, floor: i32, burns: bool, metal_in_the_open: bool) -> f32 {
    (top - floor) as f32 * PER_BLOCK
        + if burns { FUEL } else { 0.0 }
        + if metal_in_the_open { METAL } else { 0.0 }
}

/// What the ground a bolt hit turns into, or `None` for ground a bolt
/// leaves alone.
///
/// Turf, peat and the soils burn off to bare earth. Sand, stone and
/// gravel are left: a scorch mark on rock is a texture nobody would
/// read, and the atlas is not spent on one. Snow is left too -- a bolt
/// that melted a hole in a snowfield would need the hole to fill in
/// again, and nothing else in this game melts snow.
pub fn scorched(block: BlockId) -> Option<BlockId> {
    match crate::types::block_kind(block) {
        BLOCK_GRASS | BLOCK_PEAT => Some(BLOCK_DIRT),
        _ => None,
    }
}

/// Can a bolt leave its ash in this cell? Only open air: ash is a flat
/// cover and putting it where something already stands would delete it.
#[inline]
pub fn may_leave_ash(cell: BlockId) -> bool {
    is_air(cell)
}

/// The ash itself, for the caller that has room for it. The same ash a
/// burnt tuft leaves (`wildfire::leaves_ash`), deliberately: a player who
/// has seen one has read the other.
pub const ASH: BlockId = BLOCK_ASH;

/// Is this what a bolt calls snow -- ground a strike is not allowed to
/// scorch? Kept beside `scorched` so the two cannot drift.
#[inline]
pub fn is_left_alone(block: BlockId) -> bool {
    crate::types::block_kind(block) == BLOCK_SNOW
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_AIR, BLOCK_LEAVES, BLOCK_SAND, BLOCK_STONE};

    #[test]
    fn the_tallest_thing_takes_the_bolt() {
        let hill = attraction(20, 10, false, false);
        let flat = attraction(10, 10, false, false);
        assert!(hill > flat, "a hilltop was no likelier than the plain below it");
    }

    #[test]
    fn a_tree_beats_the_bare_hill_beside_it_and_a_tree_on_the_hill_beats_both() {
        let bare_hill = attraction(13, 10, false, false);
        let tree = attraction(13, 10, true, false);
        let tree_on_the_hill = attraction(18, 10, true, false);
        assert!(tree > bare_hill);
        assert!(tree_on_the_hill > tree);
    }

    #[test]
    fn metal_in_the_open_makes_a_player_about_as_attractive_as_a_tree() {
        // The decision the whole mechanic exists for. Not a certainty:
        // see the tree above, which is taller.
        let player = attraction(10, 10, false, true);
        let tree = attraction(14, 10, true, false);
        assert!(player > attraction(10, 10, false, false), "the axe changed nothing");
        assert!(
            tree > player,
            "standing next to something taller stopped being the safe thing to do"
        );
    }

    #[test]
    fn the_crack_arrives_after_the_flash_and_later_the_further_off_it_is() {
        assert_eq!(thunder_delay(0.0), 0.0);
        assert!(thunder_delay(340.0) > thunder_delay(34.0));
        // Three seconds to the kilometre, near enough -- which is the
        // rule a player counts by.
        assert!((thunder_delay(1000.0_f32.min(HEARD_WITHIN)) - HEARD_WITHIN / THUNDER_SPEED).abs() < 0.01);
        assert!(thunder_gain(10.0) > thunder_gain(500.0));
        assert!(thunder_gain(HEARD_WITHIN * 2.0) > 0.0, "a far bolt was silent rather than distant");
    }

    #[test]
    fn a_bolt_hurts_what_it_lands_on_and_nothing_across_the_field() {
        assert!(hurt(0.0) >= DIRECT_DAMAGE * 0.9);
        assert!(hurt(HURT_REACH - 0.5) > 0.0);
        assert_eq!(hurt(HURT_REACH), 0.0);
        assert_eq!(hurt(40.0), 0.0);
        // A player at full health lives through a direct hit. That is
        // the rule, not an accident of the number: see `DIRECT_DAMAGE`.
        // Full health is twenty (`logic::survival::MAX_HEALTH`, which
        // lives on the server and cannot be named from here -- the same
        // arrangement `combat` works under).
        assert!(hurt(0.0) < 20.0, "a bolt killed a healthy player outright");
    }

    #[test]
    fn a_bolt_burns_the_turf_off_and_leaves_the_rock_alone() {
        assert_eq!(scorched(BLOCK_GRASS), Some(BLOCK_DIRT));
        assert_eq!(scorched(BLOCK_STONE), None);
        assert_eq!(scorched(BLOCK_SAND), None);
        assert!(is_left_alone(BLOCK_SNOW));
        assert!(may_leave_ash(BLOCK_AIR));
        assert!(!may_leave_ash(BLOCK_LEAVES), "ash was laid over a canopy");
    }
}
