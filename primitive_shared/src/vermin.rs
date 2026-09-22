//! What a rat does to what you have put by.
//!
//! The animal itself is `animals::Species::Rat` and its behaviour is the
//! server's (`logic::vermin`). This is the part both sides have to agree
//! about: **when rats come, what they go for, and what is left of it
//! afterwards.** The client draws none of it and predicts none of it --
//! a chest is emptied by the server and the player is told -- but the
//! rules live here for the reason `rack` and `hearth` give: a rule with
//! two implementations is a rule with two answers.
//!
//! ## The three things they do, and why they are three different things
//!
//! * **They eat out of chests.** Loose food in an unguarded box goes,
//!   a little at a time. This is the one that costs the least and is
//!   noticed the soonest, which is the right way round: it is the
//!   warning.
//! * **They chew the skin off a drying rack.** A rack is out in the
//!   weather by definition -- it wants air (`logic::drying`) -- so it is
//!   the one store a player cannot simply wall in, and it is where the
//!   loss hurts most: twelve minutes of curing, gone, because the frame
//!   stood out in the dark.
//! * **They spoil the larder.** Dried and salted meat does not get
//!   *eaten* so much as **un-preserved**: a gnawed haunch of dried
//!   salted meat is salted meat, and salted meat is meat again. See
//!   [`spoiled`]. This is the expensive one, and it is deliberately the
//!   one that takes longest to notice, because what it destroys is not
//!   an item but a *plan* -- the winter you spent a week smoking for.
//!
//! **Why un-preserving and not simply destroying.** Destroying a stack
//! is the obvious rule and it is worse in every way that matters: it is
//! indistinguishable from a bug ("my chest is empty"), it cannot be
//! partly defended against, and it gives the player nothing to look at.
//! A ladder down the preservation table is legible at a glance -- the
//! dried meat in the box is plain meat now, and plain meat has four days
//! in it -- and it leaves the player something to do about it, which is
//! eat it first.
//!
//! ## Why there is no trap and no cat
//!
//! Both were designed and both were rejected, and the reason is the same
//! one: they are things you build *once*. A cat patrolling a storeroom is
//! a solved problem with an item attached, and after the first one no
//! player thinks about vermin again -- which makes the whole mechanic a
//! tutorial for a crafting recipe. The answers this has are answers you
//! have to keep giving: **a light that burns out, and a wall with no gap
//! under it.** A lit storeroom is a storeroom somebody keeps lit.
//!
//! (A trap is also the wrong shape for another reason: it would have to
//! be *placed where the rats are*, and the player does not see them
//! arrive. A defence you aim at something you have never seen is a
//! guess.)

use crate::inventory::Stack;
use crate::types::{block_kind, BlockId};

/// The light level at which a place stops being dark enough.
///
/// Seven, which is the same threshold the game already uses for "this is
/// a dark place" -- a torch (`blocks`' emission) lights a small room well
/// past it, and the last red of a hearth's embers does not. **One number
/// and not a curve**, because what the player has to be able to work out
/// is "will a torch here stop them", and the answer has to be yes or no.
pub const LIGHT_KEEPS_THEM_OUT: u8 = 7;

/// The most rats one player's home holds at a time.
///
/// Four. A pair is a noise in the wall and a nuisance in the chest; a
/// dozen is a plague, and a plague has no answer that a torch and a door
/// can give -- which would make the mechanic a demand for a solution the
/// game does not have. Four is enough that the storeroom is a problem
/// and few enough that it is *your* problem rather than the world's.
pub const MOST_RATS: usize = 4;

/// Whether rats may come out here and now.
///
/// The three conditions, and all three have to hold:
///
/// * **After dark.** Not "at any dark moment" -- an eclipse is not an
///   infestation -- but actually night. The night is when a player is
///   asleep or somewhere else, which is the only reason any of this is
///   interesting: a thing that happened in front of you would just be an
///   animal to kill.
/// * **In the dark.** `light` is the block's own light, and
///   [`LIGHT_KEEPS_THEM_OUT`] is the whole of the defence a lamp gives.
/// * **Somewhere lived in**, out of `haunt`: `heat` at or over
///   [`crate::haunt::LIVED_IN`]. A camp somebody sleeps in every night
///   qualifies; a wood does not, however dark it is.
///
/// The third is the one that makes this a mechanic rather than a hazard.
/// Without it rats would be "night-time animals", which the wolf already
/// is; with it they are a consequence of having somewhere to keep things.
pub fn may_appear(night: bool, light: u8, heat: f32) -> bool {
    night && light < LIGHT_KEEPS_THEM_OUT && heat >= crate::haunt::LIVED_IN
}

/// What a rat wants with this, if anything.
///
/// Ordered, and the order is what a rat walks past to get to: it will
/// leave a chest of berries alone if there is a rack with a skin on it in
/// the same room, because the skin is the thing it can smell. That is
/// also the order of how much it costs the player, which is not a
/// coincidence -- **the worst loss must be the one the player can see
/// coming**, or the mechanic is a dice roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wants {
    /// Nothing here. Stone, planks, a pickaxe.
    Nothing,
    /// Ordinary food: berries, a root, a cooked haunch.
    Food,
    /// The winter store: dried, salted, or both.
    Larder,
    /// A skin. Raw hide and the leather off it both -- a rat chews hide
    /// because it is hide, and a cured one is no less chewable.
    Hide,
}

/// What a rat makes of one block.
pub fn wants(block: BlockId) -> Wants {
    use crate::types::{
        BLOCK_DRIED_FISH, BLOCK_DRIED_MEAT, BLOCK_DRIED_SALTED_FISH, BLOCK_DRIED_SALTED_MEAT,
        BLOCK_HIDE, BLOCK_LEATHER, BLOCK_PELT, BLOCK_SALTED_FISH, BLOCK_SALTED_MEAT,
    };
    match block_kind(block) {
        BLOCK_HIDE | BLOCK_LEATHER | BLOCK_PELT => Wants::Hide,
        BLOCK_DRIED_MEAT
        | BLOCK_DRIED_FISH
        | BLOCK_SALTED_MEAT
        | BLOCK_SALTED_FISH
        | BLOCK_DRIED_SALTED_MEAT
        | BLOCK_DRIED_SALTED_FISH => Wants::Larder,
        // Everything else that is food at all, and nothing that is not.
        // Read off `food` rather than listed here, so a new fruit is
        // something a rat eats on the day it exists -- the alternative is
        // a second food table that goes stale silently.
        other if crate::food::nutrition(other).is_some() => Wants::Food,
        _ => Wants::Nothing,
    }
}

/// One rung down the preservation ladder, or `None` for something that
/// was never preserved.
///
/// The ladder is `food::rot_every`'s, walked backwards: dried and salted
/// (72 steps a stage) → salted (4) → the meat itself (1). **Exactly the
/// ladder the player climbed**, so what they see is their own work coming
/// undone one step at a time, and a haunch that has been got at twice is
/// plain meat rather than gone.
///
/// The rot stage is not carried over, and that is on purpose: a haunch
/// that has been dried for a month has been *kept*, not aged, so what
/// comes out is fresh meat with a few days in it. Carrying the stage over
/// would make a spoiled store instantly rotten, which is the
/// "indistinguishable from destroying it" outcome the module note
/// rejected.
pub fn spoiled(block: BlockId) -> Option<BlockId> {
    use crate::types::{
        BLOCK_COOKED_MEAT, BLOCK_DRIED_FISH, BLOCK_DRIED_MEAT, BLOCK_DRIED_SALTED_FISH,
        BLOCK_DRIED_SALTED_MEAT, BLOCK_RAW_FISH, BLOCK_SALTED_FISH, BLOCK_SALTED_MEAT,
    };
    Some(match block_kind(block) {
        BLOCK_DRIED_SALTED_MEAT => BLOCK_SALTED_MEAT,
        BLOCK_DRIED_SALTED_FISH => BLOCK_SALTED_FISH,
        // Dried meat is cooked meat and not raw: it went on the rack
        // already dressed, and a rat chewing it does not put blood back
        // into it.
        BLOCK_DRIED_MEAT | BLOCK_SALTED_MEAT => BLOCK_COOKED_MEAT,
        BLOCK_DRIED_FISH | BLOCK_SALTED_FISH => BLOCK_RAW_FISH,
        _ => return None,
    })
}

/// What one night's gnawing leaves of a stack.
///
/// `None` means there is nothing left in that slot. The three cases are
/// the three in the module note:
///
/// * **Larder**: one rung down [`spoiled`], the whole stack, and one of
///   them eaten -- unless it is the last one, which is spoiled and left. The whole stack because they are in the same box and a
///   rat does not sort; the one eaten because something has to be
///   *missing*, or a player who does not read block names never notices.
/// * **Hide**: one skin gone. Not spoiled into anything -- there is
///   nothing below hide -- and not the whole rack, because a rack that
///   was emptied in one night would make the drying rack unusable rather
///   than risky.
/// * **Food**: one eaten.
///
/// **Quality is carried through untouched.** A rat does not un-make a
/// thing somebody made well; what is in the box is the same piece of work
/// it was, on a shorter clock. (The first cut re-rolled it, which read as
/// the rat having *cooked* something.)
pub fn gnaw(stack: Stack) -> Option<Stack> {
    match wants(stack.block) {
        Wants::Nothing => Some(stack),
        Wants::Larder => {
            let spoiled = spoiled(stack.block)?;
            // The rung is the loss; the piece eaten is only the sign of
            // it. So the last piece is spoiled and *not* eaten: a stack of
            // one used to take both at once and vanish, which is a whole
            // haunch of dried salted meat gone in one night -- the
            // "indistinguishable from a bug" the module note is written
            // against, just on the smallest stack there is. It comes
            // apart the same three nights as any other haunch.
            let left = stack.count.saturating_sub(1).max(1);
            Some(Stack::worn(spoiled, left, stack.damage))
        }
        Wants::Hide | Wants::Food => {
            let left = stack.count.saturating_sub(1);
            (left > 0).then(|| Stack::worn(stack.block, left, stack.damage))
        }
    }
}

/// Which slot of a container a rat goes for, and `None` when there is
/// nothing in it worth the trip.
///
/// **By appetite first and by slot second.** The slot order is the
/// tie-break rather than the rule, so two servers running the same world
/// pick the same slot -- the same reason `haunt::lived_in` sorts.
pub fn target_slot(slots: &[Option<Stack>]) -> Option<usize> {
    slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            let stack = (*slot)?;
            let wants = wants(stack.block);
            (wants != Wants::Nothing).then_some((wants, index))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, index)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::haunt::LIVED_IN;
    use crate::types::{
        BLOCK_BERRIES, BLOCK_COBBLESTONE, BLOCK_COOKED_MEAT, BLOCK_DRIED_MEAT,
        BLOCK_DRIED_SALTED_MEAT, BLOCK_HIDE, BLOCK_SALTED_MEAT,
    };

    #[test]
    fn rats_come_at_night_to_a_dark_place_somebody_lives_in() {
        assert!(may_appear(true, 0, LIVED_IN), "nothing came to a dark house at night");
    }

    #[test]
    fn nothing_comes_to_a_wilderness_in_daylight() {
        // The three failures, one at a time, so the test says which rule
        // broke rather than that some rule did.
        assert!(!may_appear(false, 0, LIVED_IN * 4.0), "rats in the daytime");
        assert!(
            !may_appear(true, LIGHT_KEEPS_THEM_OUT, LIVED_IN * 4.0),
            "a lamp did not keep them out"
        );
        assert!(!may_appear(true, 0, 0.0), "rats in an empty wood");
        // ...and the one that matters most: a dark wood at midnight, far
        // from anybody, is still a wood.
        assert!(!may_appear(true, 0, LIVED_IN - 0.01));
    }

    #[test]
    fn a_rat_spoils_what_is_on_a_rack() {
        // A rack with two skins on it, which is what a player leaves out
        // overnight.
        let rack = Stack::new(BLOCK_HIDE, 2);
        let after = gnaw(rack).expect("a rack stripped in one night");
        assert_eq!(after.block, BLOCK_HIDE);
        assert_eq!(after.count, 1, "the rat took more than one skin");
        // ...and the last one goes, so a rack left out long enough is a
        // rack with nothing on it.
        assert_eq!(gnaw(after), None);
    }

    #[test]
    fn the_winter_store_comes_apart_one_rung_at_a_time() {
        let mut stack = Stack::new(BLOCK_DRIED_SALTED_MEAT, 4);
        stack = gnaw(stack).expect("the whole store went in one night");
        assert_eq!(stack.block, BLOCK_SALTED_MEAT, "the ladder skipped a rung");
        assert_eq!(stack.count, 3, "nothing was eaten");
        stack = gnaw(stack).expect("the store went");
        assert_eq!(stack.block, BLOCK_COOKED_MEAT, "salted meat did not come back to meat");
        // ...and once it is plain food it is only eaten, not spoiled again.
        let last = gnaw(stack).expect("the meat went");
        assert_eq!(last.block, BLOCK_COOKED_MEAT);
        assert_eq!(last.count, 1);
    }

    #[test]
    fn a_single_haunch_of_the_winter_store_comes_down_a_rung_instead_of_vanishing() {
        let one = Stack::new(BLOCK_DRIED_SALTED_MEAT, 1);
        let after = gnaw(one).expect("one piece of the store vanished in one night");
        assert_eq!(after.block, BLOCK_SALTED_MEAT, "the ladder skipped a rung");
        assert_eq!(after.count, 1);
        let after = gnaw(after).expect("the salted piece vanished");
        assert_eq!(after.block, BLOCK_COOKED_MEAT, "salted meat did not come back to meat");
        // Only once it is plain food is the last piece eaten.
        assert_eq!(gnaw(after), None);
    }

    #[test]
    fn a_big_winter_store_loses_one_piece_and_one_rung_a_night() {
        let after = gnaw(Stack::new(BLOCK_DRIED_SALTED_MEAT, 40)).expect("the store went");
        assert_eq!(after.block, BLOCK_SALTED_MEAT);
        assert_eq!(after.count, 39, "a big store lost more or less than one piece");
    }

    #[test]
    fn spoiling_a_store_leaves_it_worth_less_and_never_worthless() {
        use crate::food::{rot_every, LAST_ROT_STAGE};
        for block in [BLOCK_DRIED_SALTED_MEAT, BLOCK_DRIED_MEAT, BLOCK_SALTED_MEAT] {
            let after = spoiled(block).expect("a store with no rung below it");
            assert!(
                rot_every(after) < rot_every(block),
                "{} spoiled into something that keeps as well",
                crate::types::block_name(block)
            );
            assert!(
                crate::food::nutrition(after).is_some(),
                "{} spoiled into something that is not food",
                crate::types::block_name(block)
            );
            // Fresh, not stale: see `spoiled`.
            assert!(crate::food::rot_stage(after) < LAST_ROT_STAGE);
        }
    }

    #[test]
    fn a_rat_leaves_the_stone_and_takes_the_skin() {
        assert_eq!(wants(BLOCK_COBBLESTONE), Wants::Nothing);
        assert_eq!(wants(BLOCK_HIDE), Wants::Hide);
        assert_eq!(wants(BLOCK_DRIED_MEAT), Wants::Larder);
        assert_eq!(wants(BLOCK_BERRIES), Wants::Food);
        // A pick in a chest is a pick in the morning.
        let pick = Stack::new(BLOCK_COBBLESTONE, 12);
        assert_eq!(gnaw(pick), Some(pick));
    }

    #[test]
    fn it_goes_for_the_skin_before_the_store_and_the_store_before_the_berries() {
        let chest = [
            Some(Stack::new(BLOCK_BERRIES, 10)),
            Some(Stack::new(BLOCK_COBBLESTONE, 10)),
            Some(Stack::new(BLOCK_DRIED_MEAT, 10)),
            Some(Stack::new(BLOCK_HIDE, 1)),
        ];
        assert_eq!(target_slot(&chest), Some(3));
        assert_eq!(target_slot(&chest[..3]), Some(2));
        assert_eq!(target_slot(&chest[..2]), Some(0));
        assert_eq!(target_slot(&chest[1..2]), None, "a rat robbed a box of rubble");
        assert_eq!(target_slot(&[]), None);
    }

    #[test]
    fn what_a_rat_leaves_is_still_the_thing_somebody_made() {
        use crate::quality::Quality;
        let fine = Stack::new(BLOCK_DRIED_MEAT, 3).with_quality(Quality::from_fraction(1.0));
        let after = gnaw(fine).expect("the store went");
        assert_eq!(after.quality(), fine.quality(), "the rat re-cooked the meat");
    }
}
