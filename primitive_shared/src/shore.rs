//! The shore between the sand and the deep water: mussel beds, and the
//! starfish that eat them.
//!
//! ## What is here and why it is not an animal
//!
//! Two things live on the rocks a wave covers twice a day, and neither of
//! them is a `Species`. The argument is written out at
//! [`crate::types::BLOCK_STARFISH`]; the short of it is that an animal in
//! this game is a body that goes somewhere, and these do not go anywhere.
//! What they do instead is belong to a *stretch of rock*, and a stretch of
//! rock is a block.
//!
//! ## The decision the bed creates
//!
//! A mussel bed is the floor of the shallows with a crust of mussels on it
//! (a whole cell of rock, not a sprite standing in the water --
//! [`crate::types::BLOCK_MUSSEL_BED`] says why), and it is food a player can
//! carry away by the armful on the first evening, with no rod, no trap, no
//! spear and no fire but the one they will need to cook it.
//!
//! That is a lot to give away, so it is given away *once*: the bed holds
//! [`BED_FULL`] mussels, a hand takes one, and it grows one back on the same
//! clock a berry bush uses -- twenty five times slower ([`REGROW_STEPS`]).
//!
//! So a shore is a larder with a rate rather than a pile, and the decision is
//! the fishing spot's ([`crate::fishing::SPOT_HOLDS`]): strip the rock at the
//! bottom of the camp and eat well tonight and nothing tomorrow, or walk the
//! headland and take one bed's worth from each. **The difference from the
//! fishing spot is that this one is visible.** A fished-out lake looks like a
//! lake; a stripped bed is bare rock, and a player can read a whole coast off
//! the colour of it -- including, and this is the part that makes it a place
//! rather than a number, the fact that somebody else has been along it.
//!
//! ## The starfish
//!
//! A starfish is the reason a bed with nothing wrong with it does not come
//! back. It eats mussels, so while one is lying within [`STARFISH_REACH`] of
//! a bed the bed does not regrow at all ([`starfish_stall`]) -- and a player
//! who has worked out why can knock it off the rock, which takes one swing
//! of a bare hand and gives nothing back (see
//! [`crate::types::BLOCK_STARFISH`]). A two-second job with a whole headland
//! behind it: that is the shape a piece of knowledge should have.
//!
//! Rejected: **making a starfish eat a mussel** -- a tick that takes the
//! bed's count down. It is what the animal does, and it is a larder that
//! empties while nobody is looking: a player who comes back to a bed they
//! left half full and finds it bare has been robbed by something they never
//! saw. Stalling the regrowth costs the same mussels over a week and every
//! one of them is a mussel the player watched not appear.

use crate::types::{
    block_kind, BlockId, BLOCK_MUSSEL_BED, BLOCK_MUSSEL_ROCK, BLOCK_STARFISH, VARIANT_SHIFT,
};

/// How many mussels a full bed carries.
///
/// **Four, which is the whole of the variant field a hive uses**
/// (`bees::HIVE_FULL`) and the same number for the same reason: it is as
/// many steps as a picture can show. A bed with seven on it and a bed with
/// six are one picture, and a count a player cannot see is a count that
/// might as well be a die.
pub const BED_FULL: u8 = 4;

/// How many growth steps one mussel takes.
///
/// **Twenty-five.** The growth pass runs a bush on `REGROW_SECONDS` -- half
/// an hour -- and four mussels at a bush's pace would be a bed back inside
/// two hours, which is a vending machine on a rock. At twenty-five it is
/// about twelve hours of play to fill a stripped bed and three to take the
/// last one back, so a camp on a small cove is a camp that runs its shore
/// down, and the answer is to walk.
///
/// A multiplier rather than a `REGROW_SECONDS` of its own, because the one
/// thing that must stay true is that a mussel is *slower than a berry*, and a
/// multiple says that where two unrelated constants only happen to.
pub const REGROW_STEPS: u32 = 25;

/// How far a starfish reaches, in blocks: the radius the server searches
/// round a bed before it lets it grow.
///
/// Three, and it is a radius rather than "the cell beside it" because a
/// starfish that had to be touching the bed would be a starfish a player
/// never connected to anything. Three blocks is inside one look at the sea
/// floor.
pub const STARFISH_REACH: i32 = 3;

/// Is this a mussel bed -- crusted or stripped bare?
///
/// **Both ids**, because every rule about a bed is a rule about the rock: it
/// regrows, it is stalled by a starfish, it is gathered from. Only the
/// picture and the count differ (`crate::types::BLOCK_MUSSEL_BED`).
#[inline]
pub fn is_bed(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_MUSSEL_BED | BLOCK_MUSSEL_ROCK)
}

/// How many mussels are on this rock. Nought for a stripped one, and nought
/// for anything that is not a bed at all.
#[inline]
pub fn mussels_in(id: BlockId) -> u8 {
    if block_kind(id) != BLOCK_MUSSEL_BED {
        return 0;
    }
    (((id & crate::types::VARIANT_MASK) >> VARIANT_SHIFT) as u8).clamp(1, BED_FULL)
}

/// The rock carrying `count` mussels: the bare one at nought.
#[inline]
pub fn bed_holding(count: u8) -> BlockId {
    if count == 0 {
        return BLOCK_MUSSEL_ROCK;
    }
    BLOCK_MUSSEL_BED | (BlockId::from(count.min(BED_FULL)) << VARIANT_SHIFT)
}

/// Is this id a bed anybody could have written -- a count no bigger than a
/// bed holds?
///
/// Asked by `types::is_known_block` for the hive's reason: the server writes
/// a new count into the world every time a hand comes off the rock, and a
/// fifth mussel is a claim.
#[inline]
pub fn is_valid_bed(id: BlockId) -> bool {
    let count = (id & crate::types::VARIANT_MASK) >> VARIANT_SHIFT;
    // **Never nought.** A bed with no mussels on it is the bare rock's own id
    // (`BLOCK_MUSSEL_ROCK`), and a second id for the same bare rock is a
    // second kind of bare rock that does not look like the first -- the
    // barrel's argument (`types::barrel_goods`), which is the same argument.
    (1..=BlockId::from(BED_FULL)).contains(&count)
}

/// What the bed becomes when a hand takes from it, and how many mussels come
/// away -- or `None` if there is nothing on it.
///
/// **A tool takes two and a hand takes one.** A mussel is held on by a beard
/// of threads and anything with an edge cuts a clump off where fingers pull
/// them one at a time; that is the whole of what the knife buys, and it is
/// deliberately not *faster*. A bed that gave up its four to a knife in one
/// gesture would make the knife the only correct way to touch a shore, and a
/// mechanic with one right answer is not a mechanic (CLAUDE.md).
pub fn gather(bed: BlockId, held: Option<BlockId>) -> Option<(BlockId, u32)> {
    let on = mussels_in(bed);
    if !is_bed(bed) || on == 0 {
        return None;
    }
    let wanted = if helps_gathering(held) { 2 } else { 1 };
    let taken = wanted.min(on);
    Some((bed_holding(on - taken), u32::from(taken)))
}

/// Does what is in the hand help prise mussels off? A knife, and nothing
/// else: `types::is_knife` is already this game's word for "an edge"
/// (a hoe is `Work::Plant` and is not one), and a second list here would be
/// the day somebody adds a fifth knife and a shore does not notice.
#[inline]
pub fn helps_gathering(held: Option<BlockId>) -> bool {
    held.is_some_and(crate::types::is_knife)
}

/// Does a starfish within reach stop this bed growing?
///
/// Takes what is in each cell round the bed, as the server's growth pass can
/// ask it: shared so that a test can state the rule without a world.
pub fn starfish_stall(mut at: impl FnMut(i32, i32, i32) -> Option<BlockId>) -> bool {
    let reach = STARFISH_REACH;
    // **Flat, and a block either way** -- not a ball of the same radius. Both
    // of these lie *on the floor of the shallows*, so the only way a starfish
    // gets three blocks above or below a bed is a step in the sea bed, and a
    // starfish on the shelf over a bed is on another rock. It is also seven
    // times less to look at, and this runs on the growth pass for every bed
    // in every loaded chunk.
    for dy in -1..=1 {
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                // A disc rather than a square: a corner of a seven-block box
                // is four blocks away, which is further than a player would
                // call "on this rock".
                if dx * dx + dz * dz > reach * reach {
                    continue;
                }
                if at(dx, dy, dz).map(block_kind) == Some(BLOCK_STARFISH) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_WATER, BLOCK_STARFISH};

    #[test]
    fn a_bed_carries_its_mussels_in_its_own_id() {
        for count in 0..=BED_FULL {
            assert_eq!(mussels_in(bed_holding(count)), count);
            assert!(is_bed(bed_holding(count)), "{count} mussels is not a bed");
        }
        for count in 1..=BED_FULL {
            assert!(is_valid_bed(bed_holding(count)));
        }
        assert_eq!(bed_holding(0), BLOCK_MUSSEL_ROCK, "a stripped bed kept the crusted rock's id");
        assert!(!is_valid_bed(BLOCK_MUSSEL_BED), "a bed of nought is the bare rock's id");
        assert!(!is_valid_bed(BLOCK_MUSSEL_BED | (BlockId::from(BED_FULL + 1) << VARIANT_SHIFT)));
    }

    #[test]
    fn a_bed_can_be_stripped_and_then_gives_nothing() {
        let mut bed = bed_holding(BED_FULL);
        let mut got = 0;
        while let Some((left, taken)) = gather(bed, None) {
            bed = left;
            got += taken;
        }
        assert_eq!(got, u32::from(BED_FULL), "a bed gave more than it held");
        assert_eq!(mussels_in(bed), 0, "a stripped bed is bare rock");
        assert!(gather(bed, None).is_none(), "bare rock gave a mussel");
    }

    #[test]
    fn a_knife_takes_two_at_a_time_and_never_more_than_is_there() {
        let knife = crate::types::BLOCK_FLINT_KNIFE;
        let (left, taken) = gather(bed_holding(BED_FULL), Some(knife)).unwrap();
        assert_eq!(taken, 2);
        assert_eq!(mussels_in(left), BED_FULL - 2);
        let (left, taken) = gather(bed_holding(1), Some(knife)).unwrap();
        assert_eq!(taken, 1, "a knife took a mussel that was not on the rock");
        assert_eq!(mussels_in(left), 0);
    }

    #[test]
    fn a_starfish_on_the_next_rock_stops_the_bed_and_one_across_the_bay_does_not() {
        let one_at = |wx: i32, wy: i32, wz: i32| {
            move |dx: i32, dy: i32, dz: i32| {
                Some(if (dx, dy, dz) == (wx, wy, wz) { BLOCK_STARFISH } else { BLOCK_WATER })
            }
        };
        assert!(starfish_stall(one_at(2, 0, 1)), "a starfish two blocks off did not stall the bed");
        assert!(starfish_stall(one_at(2, 1, 1)), "a starfish a step up the bed did not count");
        assert!(!starfish_stall(one_at(STARFISH_REACH + 1, 0, 0)), "a starfish out of reach stalled the bed");
        // The corner of the box is outside the disc, and anything two blocks
        // over the bed is on a different rock: see `starfish_stall`.
        assert!(!starfish_stall(one_at(STARFISH_REACH, 0, STARFISH_REACH)));
        assert!(!starfish_stall(one_at(0, 2, 0)));
        assert!(!starfish_stall(|_, _, _| Some(BLOCK_WATER)));
    }

    #[test]
    fn a_mussel_takes_longer_to_come_back_than_a_berry() {
        // A `const` assert, so the day somebody "simplifies" the multiplier
        // back to one the build stops rather than a shore quietly turning
        // into a hedgerow. See `REGROW_STEPS` for what the multiple buys.
        const { assert!(REGROW_STEPS > 1, "a mussel grew at a berry's pace") }
    }
}
