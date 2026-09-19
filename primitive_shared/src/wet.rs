//! Things that get wet: what a soaking does to the pack, and what dries it.
//!
//! ## The decision this is for
//!
//! The body has been wet for a long time (`body::felt_ambient`: a soaked
//! coat stops insulating), and nothing a player *carried* ever was. A swim
//! across a river cost a shiver and nothing else, so the river was a shiver
//! wide whatever was on the player's back. Now the pack has an answer: the
//! tinder, the torch and the firewood a player swam with are wet on the far
//! bank, and a wet fire does not catch. So a crossing is a plan -- find the
//! ford, wait out the rain, or swim and dry the kindling at the next fire --
//! and a roof, a lean-to or a leather coat is worth what it keeps dry.
//!
//! ## What wet does, and to what
//!
//! Only things wetting changes are wet at all ([`gets_wet`]); a stone in a
//! river is a stone. Three kinds of consequence, one per family:
//!
//! - **Fuel and tinder burn as green wood does** (`hearth::GREEN_BURN`,
//!   `hearth::GREEN_HEAT`) -- a wet stick in a burning fire is supper's heat
//!   and never a kiln's -- and **will not light**: a firepit laid with wet
//!   sticks, a hearth with wet fuel in its slot and a wet torch all refuse the
//!   flame. One rule a player can hold, "dry it first", tied to the rule they
//!   already know for green logs rather than a third number.
//! - **Flour and bread go off faster**: wet flour moulds where dry flour keeps
//!   for ever, and a wet loaf goes in half its time (`food::rot_per_step`,
//!   `food::rot_every`).
//! - **Leather, pelt, cloth and wool must dry before they are worked**
//!   (`crafting::refuses`): nobody cuts a sodden hide or sews wet cloth.
//!
//! ## Where it is kept
//!
//! **In the id, in the sixteenth bit** -- the third bit of a furniture's
//! wood (`types::WOOD_HIGH_BIT`), which none of these kinds has a use for:
//! a log spends only the low two on its greenness, and bread and flour
//! spend the variant field on their age. The argument is `food`'s ("going
//! off") and `wood`'s ("seasoning") again: a per-stack field is one the wire,
//! both container saves and the profile store would all have to learn and
//! version, for a fact that is one bit. What it costs is theirs too, and
//! stated plainly: **a wet stack and a dry stack of sticks are two stacks.**
//! They *are* two things, and one of them will light a fire.
//!
//! Rejected: *a wet stage count*, drying a step at a time like clay. Nobody
//! can see a third of a wet stick; "wet or dry" is the whole question a
//! player asks of their kindling, and one bit answers it.
//!
//! Rejected: *everything gets wet*. A wet axe, a wet pot and a wet stone
//! would be a mark on every slot after every swim that changed nothing --
//! a mark that means nothing teaches a player to ignore the one that does.
//!
//! A kind outside [`gets_wet`] never carries the bit, and the leaf handful
//! above all: it spends the same bit on which tree it fell from
//! (`types::carries_wood`), and leaves are wet anyway.
//!
//! Pure rules, no I/O. The server decides when a pack soaks and dries
//! (`logic::climate`, `pack_weather` below); the client draws the mark.

use crate::types::{block_kind, BlockId, VARIANT_MASK, WOOD_HIGH_BIT};

/// The bit that says a stack is wet. See the module note for why here.
pub const WET: BlockId = WOOD_HIGH_BIT;

/// Can this kind of thing get wet at all?
///
/// Fuel and tinder, the torch, flour and bread, and the four soft goods a
/// tanner or a weaver works. Fat is not here -- tallow sheds water, which is
/// what it is smeared on a torch for -- and neither is the leaf handful (see
/// the module note).
pub fn gets_wet(id: BlockId) -> bool {
    use crate::types::*;
    let kind = block_kind(id);
    crate::wood::is_log(kind)
        || matches!(
            kind,
            BLOCK_STICK
                | BLOCK_CANE
                | BLOCK_FIBER
                | BLOCK_DRY_GRASS
                | BLOCK_FEATHER
                | BLOCK_BRACKET_FUNGUS
                | BLOCK_COAL
                | BLOCK_DRIED_PEAT
                | BLOCK_TORCH
                | BLOCK_FLOUR
                | BLOCK_BREAD
                | BLOCK_LEATHER
                | BLOCK_PELT
                | BLOCK_CLOTH
                | BLOCK_WOOL
        )
}

/// Is this stack wet?
#[inline]
pub fn is_wet(id: BlockId) -> bool {
    id & WET != 0 && gets_wet(id)
}

/// The same thing, soaked. Anything that does not get wet comes back as it
/// was, so a pass over a whole pack can call this on every slot.
#[inline]
pub fn wetted(id: BlockId) -> BlockId {
    if gets_wet(id) {
        id | WET
    } else {
        id
    }
}

/// The same thing, dry again.
///
/// **Flour comes back fresh.** Wet flour ages in the variant field
/// (`food::rot_per_step`), and dry flour keeps for ever and has no age --
/// so a stage left in the field would be an id nothing defines. Dried before
/// it rotted, it is flour; the mould that had not happened yet did not.
#[inline]
pub fn dried(id: BlockId) -> BlockId {
    if !is_wet(id) {
        return id;
    }
    let dry = id & !WET;
    if block_kind(dry) == crate::types::BLOCK_FLOUR {
        dry & !VARIANT_MASK
    } else {
        dry
    }
}

/// Does fire refuse this -- wet fuel, wet tinder, a wet torch?
#[inline]
pub fn will_not_light(id: BlockId) -> bool {
    is_wet(id)
}

/// Must this be dry before a hand works it? Leather, pelt, cloth and wool,
/// wet. See `crafting::refuses`.
pub fn must_dry_first(id: BlockId) -> bool {
    use crate::types::{BLOCK_CLOTH, BLOCK_LEATHER, BLOCK_PELT, BLOCK_WOOL};
    is_wet(id) && matches!(block_kind(id), BLOCK_LEATHER | BLOCK_PELT | BLOCK_CLOTH | BLOCK_WOOL)
}

/// The word the tooltip puts after a wet thing's name.
pub fn label(id: BlockId) -> Option<&'static str> {
    is_wet(id).then_some("wet")
}

// ---- when a pack soaks and when it dries ----

/// How wet the body has to be, 0..1, before what it carries is wet too.
///
/// **Most of the way, not at the first drop.** Rain works through a coat
/// before it reaches a pack under it, so a shower that is over in a minute
/// costs a damp shirt and not the tinder -- and the wetting rate is the
/// coat's (`equipment::Worn::shed_rain`), so leather keeps the pack dry for
/// longer than wool and metal keeps it dry for good. What a player learns is
/// the thing that is true: get under something before you are soaked.
pub const PACK_SOAKS_AT: f32 = 0.8;

/// Seconds of drying weather it takes a wet pack to dry: by a fire at the
/// full rate, in the sun at half of it ([`SUN_DRIES`]).
///
/// **A minute by a fire.** Long enough that a player who swam has to stop
/// and sit by the flames rather than walk past them, short enough that the
/// stop is a rest and not a chore: it is about as long as the body itself
/// takes to dry at a hearth (`climate::drying_rate`).
pub const PACK_DRIES_SECONDS: f32 = 60.0;

/// How much of a fire's drying the sun gives a pack: half. Two minutes in
/// the open on a clear warm day, which is what a player walking in the sun
/// gets for nothing, and a reason to wait for the fire when it is cloudy.
pub const SUN_DRIES: f32 = 0.5;

/// What the weather is doing to the pack this sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackWeather {
    /// Everything that gets wet, wet now.
    Soak,
    /// Everything wet, dry now.
    Dry,
    /// Nothing changes yet.
    Keep,
}

/// What one sample of the weather does to the pack, and the drying carried
/// forward to the next one.
///
/// - `swimming`: the body is in the water, not only the feet -- a pack on
///   the back of a swimmer is in the river. Wading keeps it dry.
/// - `rained_on`: rain is reaching the player (no roof, no lean-to).
/// - `body_wetness`: the body's own wetness after this sample, 0..1.
/// - `by_fire` / `in_sun`: what is drying the player now.
/// - `drying`: seconds of drying so far, carried between samples.
///
/// **Soaking wins and resets the drying**: a pack in the river is not half
/// dried by the fire it was at a minute ago.
pub fn pack_weather(
    swimming: bool,
    rained_on: bool,
    body_wetness: f32,
    by_fire: bool,
    in_sun: bool,
    drying: f32,
    dt: f32,
) -> (PackWeather, f32) {
    let wetness = if body_wetness.is_finite() { body_wetness } else { 0.0 };
    if swimming || (rained_on && wetness >= PACK_SOAKS_AT) {
        return (PackWeather::Soak, 0.0);
    }
    if rained_on {
        return (PackWeather::Keep, 0.0);
    }
    let rate = if by_fire {
        1.0
    } else if in_sun {
        SUN_DRIES
    } else {
        0.0
    };
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let drying = if drying.is_finite() { drying.max(0.0) } else { 0.0 } + rate * dt;
    if drying >= PACK_DRIES_SECONDS {
        (PackWeather::Dry, 0.0)
    } else {
        (PackWeather::Keep, drying)
    }
}

/// Every stack in `inventory` wetted or dried, in its own slot at its own
/// count and with its own wear word. Answers whether anything changed.
///
/// **In place, slot by slot**, for `logic::rot`'s reason: tinder that jumps
/// to another square of the pack when it gets wet is tinder a player loses
/// track of. Two stacks that are now the same thing -- a wet stick beside
/// a stick that was wet already -- stay two stacks until the player merges
/// them; tidying a pack is the player's business.
pub fn weather_inventory(inventory: &mut crate::inventory::Inventory, weather: PackWeather) -> bool {
    let change: fn(BlockId) -> BlockId = match weather {
        PackWeather::Soak => wetted,
        PackWeather::Dry => dried,
        PackWeather::Keep => return false,
    };
    let mut changed = false;
    for slot in 0..inventory.slots().len() {
        let Some(stack) = inventory.slots()[slot] else {
            continue;
        };
        let next = change(stack.block);
        if next == stack.block {
            continue;
        }
        inventory.take_slot(slot);
        if let Some(left) =
            inventory.put_in_slot(slot, crate::inventory::Stack::worn(next, stack.count, stack.damage))
        {
            inventory.add_worn(left.block, left.count, left.damage);
        }
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn a_wet_stick_is_still_a_stick_and_dries_back_into_the_same_one() {
        let wet = wetted(BLOCK_STICK);
        assert!(is_wet(wet));
        assert_eq!(block_kind(wet), BLOCK_STICK, "a wet stick stopped being a stick");
        assert!(is_known_block(wet), "a wet stick is an invented id");
        assert_eq!(dried(wet), BLOCK_STICK);
    }

    #[test]
    fn a_stone_and_a_leaf_handful_never_get_wet() {
        assert_eq!(wetted(BLOCK_STONE), BLOCK_STONE);
        // The handful's sixteenth bit is which tree it fell from.
        let birch = in_wood(BLOCK_LEAF_HANDFUL, 4);
        assert_eq!(wetted(birch), birch, "a leaf handful's wood was taken for water");
        assert!(!is_wet(birch));
    }

    #[test]
    fn every_kind_that_gets_wet_is_a_known_block_wet_and_dry() {
        for id in 1..1024 {
            if !crate::blocks::is_defined(id) || !gets_wet(id) {
                continue;
            }
            assert!(is_known_block(wetted(id)), "wet {} is invented", block_name(id));
            assert_eq!(dried(wetted(id)), id);
        }
    }

    #[test]
    fn a_green_log_that_gets_wet_keeps_its_greenness_and_its_wood() {
        let log = crate::wood::green(BLOCK_FIR_LOG);
        let wet = wetted(log);
        assert!(is_known_block(wet));
        assert!(crate::wood::is_green(wet) && is_wet(wet));
        assert_eq!(dried(wet), log);
        assert_eq!(crate::wood::seasoned(dried(wet)), BLOCK_FIR_LOG);
    }

    #[test]
    fn wet_fuel_burns_like_green_wood_and_dry_fuel_does_not() {
        use crate::hearth::{fuel_degrees, fuel_seconds, GREEN_HEAT};
        let dry = fuel_degrees(BLOCK_STICK).unwrap();
        let wet = fuel_degrees(wetted(BLOCK_STICK)).unwrap();
        assert!((wet - dry * GREEN_HEAT).abs() < 1e-3, "a wet stick burned as hot as a dry one");
        assert!(fuel_seconds(wetted(BLOCK_LOG)).unwrap() < fuel_seconds(BLOCK_LOG).unwrap());
        // ...and never twice: a green log soaked is not greener than green.
        let green = crate::wood::green(BLOCK_LOG);
        assert_eq!(fuel_degrees(wetted(green)), fuel_degrees(green));
    }

    #[test]
    fn a_swim_soaks_the_pack_at_once_and_wading_does_not() {
        assert_eq!(pack_weather(true, false, 0.1, false, false, 0.0, 0.5).0, PackWeather::Soak);
        assert_eq!(pack_weather(false, false, 0.4, false, false, 0.0, 0.5).0, PackWeather::Keep);
    }

    #[test]
    fn a_shower_soaks_the_coat_before_it_soaks_the_pack() {
        assert_eq!(pack_weather(false, true, PACK_SOAKS_AT - 0.1, false, false, 0.0, 0.5).0, PackWeather::Keep);
        assert_eq!(pack_weather(false, true, PACK_SOAKS_AT, false, false, 0.0, 0.5).0, PackWeather::Soak);
    }

    #[test]
    fn a_pack_dries_in_a_minute_by_a_fire_and_in_two_in_the_sun() {
        let dry_in = |fire, sun| {
            let mut drying = 0.0;
            for tick in 1..=600 {
                let (weather, next) = pack_weather(false, false, 0.0, fire, sun, drying, 0.5);
                if weather == PackWeather::Dry {
                    return tick as f32 * 0.5;
                }
                drying = next;
            }
            f32::INFINITY
        };
        assert_eq!(dry_in(true, false), PACK_DRIES_SECONDS);
        assert_eq!(dry_in(false, true), PACK_DRIES_SECONDS / SUN_DRIES);
        assert!(dry_in(false, false).is_infinite(), "a pack dried in the shade of a cloudy night");
    }

    #[test]
    fn rain_stops_the_drying_and_starts_it_again_from_nothing() {
        let (_, drying) = pack_weather(false, false, 0.0, true, false, 0.0, 30.0);
        assert!(drying > 0.0);
        let (_, drying) = pack_weather(false, true, 0.2, true, false, drying, 0.5);
        assert_eq!(drying, 0.0, "a shower left the half-dried pack half dried");
    }

    #[test]
    fn soaking_a_pack_wets_the_tinder_and_leaves_the_axe_alone() {
        let mut pack = crate::inventory::Inventory::new();
        pack.add(BLOCK_STICK, 5);
        pack.add(BLOCK_STONE_AXE, 1);
        assert!(weather_inventory(&mut pack, PackWeather::Soak));
        assert!(pack.slots().iter().flatten().any(|s| s.block == wetted(BLOCK_STICK) && s.count == 5));
        assert!(pack.slots().iter().flatten().any(|s| s.block == BLOCK_STONE_AXE));
        assert!(weather_inventory(&mut pack, PackWeather::Dry));
        assert_eq!(pack.count(BLOCK_STICK), 5);
        assert!(pack.slots().iter().flatten().all(|s| !is_wet(s.block)));
        assert!(!weather_inventory(&mut pack, PackWeather::Dry), "drying a dry pack changed it");
    }

    #[test]
    fn wet_leather_is_refused_by_the_bench_until_it_dries() {
        assert!(must_dry_first(wetted(BLOCK_LEATHER)));
        assert!(!must_dry_first(BLOCK_LEATHER));
        assert!(!must_dry_first(wetted(BLOCK_STICK)), "a wet stick cannot be a haft");
    }

    #[test]
    fn a_wet_torch_will_not_take_a_flame_and_a_dry_one_does() {
        let mut pack = crate::inventory::Inventory::new();
        pack.add(wetted(BLOCK_TORCH), 1);
        assert!(!pack.light_torch(0, BLOCK_TORCH_LIT), "a soaked wad caught");
        assert!(weather_inventory(&mut pack, PackWeather::Dry));
        assert!(pack.light_torch(0, BLOCK_TORCH_LIT));
    }

    #[test]
    fn nothing_put_down_in_the_world_carries_the_water_it_was_carried_in() {
        let torch = placed(wetted(BLOCK_TORCH), 0.0, (0, 1, 0));
        assert!(!is_wet(torch) && block_kind(torch) == BLOCK_TORCH);
        let log = placed(wetted(crate::wood::green(BLOCK_LOG)), 0.0, (0, 1, 0));
        assert!(!is_wet(log) && is_known_block(log));
    }
}
