//! Wild bees: a hive on the side of a trunk, the honey in it, and what it
//! costs to take.
//!
//! ## The decision
//!
//! Honey is the best thing in a wood to eat without a fire, and it keeps
//! (`food::rot_per_step` has no row for it). A hive is found, not made, and
//! taking from it stirs the bees: every raid stings the raider
//! ([`stings`]), a little health at once and a little venom after
//! ([`STING_HEALTH`], [`VENOM_SECONDS`]). **Smoke calms them** -- a lit torch
//! in the hand that takes the comb, a fire under the tree, a smoky room --
//! and cuts the stings to a third. So a player who finds a hive decides
//! whether to come back with fire or to put a hand in now and pay for it,
//! and in winter the bees are clustered and do not fly at all
//! ([`BEES_FLY_C`]) -- the safe raid, on a hive that will not fill again
//! until spring.
//!
//! What the stings are *not* is a death. Six at worst, half a point each and
//! a few seconds of venom behind each: a quarter of a healthy player, and
//! never the last of anybody's health (the server's `sting_from_bees`).
//! "пчёл сделай ядовитыми но чтоб сносили не особо много" -- a price, not a
//! trap.
//!
//! ## What the hive remembers, and where
//!
//! **How much honey is in it, in the variant field** ([`honey_in`]), from
//! nought to [`HIVE_FULL`]. The generator grows full hives; a raid takes all
//! of it and leaves the empty comb standing (`types::block_residue`); the
//! growth clock fills it a comb at a time in warm air
//! (`types::ripens_into`). A hive raided early gives what it had, so coming
//! back later is worth more honey and coming back now is worth some.
//!
//! **The anger is the emptiness**, and no second number. A hive with less
//! in it was robbed more recently, so it stings harder ([`stings`]) until it
//! has filled again. Rejected: *a separate "stirred" bit with its own
//! clock*. The field has room for it, and it would be a second deadline on
//! the growth mechanic for a cell that already has one, meaning the same
//! thing -- "somebody was at this hive lately" -- read off a different
//! counter that could disagree with the comb.
//!
//! **The bees themselves are the client's** (`engine::critters`): a drawing
//! round a hive, like the butterflies over a flower, costing no entity and
//! no packet. The stings are the server's, because a sting changes what is
//! true about a player.
//!
//! ## Rejected
//!
//! * **Bees as server animals that chase.** A swarm is dozens of bodies, and
//!   a player cannot fight or outrun a cloud -- there is no decision in a
//!   chase you cannot win, only a health bar going down while you walk.
//!   What the player can decide about is the moment they reach into the
//!   hive, so that is where the whole price is paid.
//! * **A placeable hive, a skep.** Beekeeping is a farm, and a farm is a
//!   second answer to the same hunger the field already answers. A wild hive
//!   is a place in a wood to remember, the nest's argument (`types::BLOCK_NEST`).
//! * **Wax with no use.** Taking the empty comb apart gives beeswax, and it is
//!   in the game because it does something a player already wants: it wads a
//!   torch as tallow does (the "wax torch" rows in `crafting`). What it costs
//!   is the hive -- it is gone, and so is the honey it would have made.

use crate::types::{block_kind, BlockId, BLOCK_WILD_HIVE, VARIANT_MASK, VARIANT_SHIFT};

/// The most honey a hive holds, in combs: what the generator grows and what a
/// raid on a full hive gives.
///
/// **Three**, so a full raid is three pieces of the best raw food in a wood --
/// a meal and a half, worth a walk and a sting -- and a hive stripped every
/// visit is emptier than one left to fill.
pub const HIVE_FULL: u8 = 3;

/// The air, in degrees, below which the bees stay in the hive: they neither
/// sting nor forage, so a hive raided in the cold stings nobody and does not
/// fill again until it is warm.
///
/// **Ten**, the line an apple sets fruit at (`growth::FRUIT_SET_C` on the
/// server), and the one honeybees really stop flying under. The same number
/// is what the client's bees come out at, so a hive with nothing flying round
/// it is a hive that will not sting -- which a player can see.
pub const BEES_FLY_C: f32 = 10.0;

/// Health a sting takes at once, out of the server's `MAX_HEALTH` of twenty.
///
/// **Half a nettle.** A nettle is one point for one handful; a sting is
/// smaller and there are several, and it is the several that is felt.
pub const STING_HEALTH: f32 = 0.5;

/// Seconds of venom a sting leaves, on the illness clock
/// (`body::SICKNESS_PER_SECOND`, a twentieth of a point a second).
///
/// **Six**: three tenths of a point each, slower than the sting itself, and
/// it stops the healing while it lasts -- so a raid is felt for a minute
/// afterwards, not only in the moment. Not the stomach's two-minute wait
/// (`body::DIGESTION_SECONDS`): venom is in the blood already.
pub const VENOM_SECONDS: f32 = 6.0;

/// How many stings a raid on a full hive costs with no smoke.
///
/// **Three**, and a robbed hive adds one for every comb it is missing, so the
/// worst raid -- taking apart a hive just emptied -- is six.
pub const STINGS_CALM: u8 = 3;

/// How many cells round a hive a fire has to be to smoke it. Three: a fire
/// at the foot of the tree the hive is on, or a torch stood beside it.
pub const SMOKE_REACH: i32 = 3;

/// How much honey is in this hive, or nought for anything that is not one.
#[inline]
pub fn honey_in(id: BlockId) -> u8 {
    if block_kind(id) != BLOCK_WILD_HIVE {
        return 0;
    }
    (((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8).min(HIVE_FULL)
}

/// A hive holding `honey` combs, clamped to what one holds: a fourth comb is
/// a variant `types::is_known_block` has no reason to believe.
#[inline]
pub fn hive_holding(honey: u8) -> BlockId {
    BLOCK_WILD_HIVE | (BlockId::from(honey.min(HIVE_FULL)) << VARIANT_SHIFT)
}

/// Is this a wild hive, of any fullness?
#[inline]
pub fn is_hive(id: BlockId) -> bool {
    block_kind(id) == BLOCK_WILD_HIVE
}

/// Does this block, burning near a hive, smoke the bees?
///
/// Anything alight: a hearth with fuel in it, a lit hand torch set down or
/// held, a standing torch, a pit kiln or a log pile burning. Not the ash and
/// not a torch gone out -- smoke is what calms them, and nothing that has
/// stopped burning makes any.
#[inline]
pub fn smokes_bees(block: BlockId) -> bool {
    crate::types::is_burning(block)
        || crate::types::is_lit_torch(block)
        || block_kind(block) == crate::types::BLOCK_STANDING_TORCH_LIT
        || crate::pit::smokes(block)
}

/// How many stings a raid on `hive` costs, in air of `air_c`, with smoke or
/// without.
///
/// **Nought in the cold** ([`BEES_FLY_C`]). Otherwise [`STINGS_CALM`] and one
/// more for every comb the hive is missing, and smoke takes it to a third --
/// never below one, because a hand in a hive is a hand in a hive: smoke
/// calms bees, it does not remove them.
pub fn stings(hive: BlockId, smoked: bool, air_c: f32) -> u8 {
    if !is_hive(hive) || air_c < BEES_FLY_C {
        return 0;
    }
    let angry = STINGS_CALM + (HIVE_FULL - honey_in(hive));
    if smoked {
        (angry / 3).max(1)
    } else {
        angry
    }
}

/// What a raid of `count` stings costs a player at full health, in health:
/// the stings and the venom after them together. For the tests and the guide;
/// the server applies the two halves separately (the venom through the
/// illness clock).
pub fn raid_cost(count: u8) -> f32 {
    f32::from(count) * (STING_HEALTH + VENOM_SECONDS * crate::body::SICKNESS_PER_SECOND)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMER: f32 = 22.0;

    #[test]
    fn a_player_with_smoke_is_stung_less_than_one_without() {
        for honey in 0..=HIVE_FULL {
            let hive = hive_holding(honey);
            let bare = stings(hive, false, SUMMER);
            let smoked = stings(hive, true, SUMMER);
            assert!(smoked < bare, "smoke did not calm a hive of {honey}: {smoked} against {bare}");
            assert!(smoked >= 1, "smoke took every bee out of a hive of {honey}");
        }
    }

    #[test]
    fn a_robbed_hive_stings_harder_until_it_has_filled_again() {
        let mut last = u8::MAX;
        for honey in 0..=HIVE_FULL {
            let now = stings(hive_holding(honey), false, SUMMER);
            assert!(now <= last, "a hive of {honey} stings more than an emptier one");
            last = now;
        }
        assert!(stings(hive_holding(0), false, SUMMER) > stings(hive_holding(HIVE_FULL), false, SUMMER));
    }

    #[test]
    fn in_the_cold_the_bees_do_not_fly_and_nobody_is_stung() {
        for honey in 0..=HIVE_FULL {
            assert_eq!(stings(hive_holding(honey), false, BEES_FLY_C - 0.5), 0);
        }
        assert_eq!(stings(crate::types::BLOCK_NEST_EGGS, false, SUMMER), 0, "a nest stung");
    }

    #[test]
    fn the_worst_raid_costs_a_healthy_player_under_a_third_of_their_health() {
        // Twenty is the server's `MAX_HEALTH`; the shared crate cannot name it.
        const HEALTHY: f32 = 20.0;
        let worst = (0..=HIVE_FULL).map(|h| stings(hive_holding(h), false, 40.0)).max().unwrap();
        let cost = raid_cost(worst);
        assert!(cost < HEALTHY / 3.0, "the worst raid takes {cost} of {HEALTHY}");
        // ...and it is felt: a raid that cost nothing would not be a decision.
        assert!(cost >= 2.0, "the worst raid takes only {cost}");
    }

    #[test]
    fn honey_in_a_hive_is_what_it_was_given_and_never_more_than_it_holds() {
        for honey in 0..=HIVE_FULL + 3 {
            let hive = hive_holding(honey);
            assert_eq!(honey_in(hive), honey.min(HIVE_FULL));
            assert!(crate::types::is_known_block(hive));
        }
        assert!(!crate::types::is_known_block(BLOCK_WILD_HIVE | (5 << VARIANT_SHIFT)));
        assert_eq!(honey_in(crate::types::BLOCK_LOG | (2 << VARIANT_SHIFT)), 0, "a log was read as a hive");
    }

    #[test]
    fn only_something_burning_smokes_the_bees() {
        use crate::types::{BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_TORCH, BLOCK_TORCH_LIT, BLOCK_TORCH_SPENT};
        assert!(smokes_bees(BLOCK_CAMPFIRE_LIT));
        assert!(smokes_bees(BLOCK_TORCH_LIT));
        assert!(!smokes_bees(BLOCK_CAMPFIRE));
        assert!(!smokes_bees(BLOCK_TORCH));
        assert!(!smokes_bees(BLOCK_TORCH_SPENT));
    }
}
