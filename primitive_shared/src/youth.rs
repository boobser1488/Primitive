//! Young animals: what a fawn, a lamb or a piglet is, as rules.
//!
//! ## A young animal is its parent's species, smaller
//!
//! **Not a species of its own**, and that is the decision everything else in
//! here follows from. A lamb as `Species::Lamb` would be a second row in every
//! table `Species` has -- speeds, senses, spawn weights, carcasses, the
//! skeleton ladder (`types::SKELETON_BLOCKS`), the wire's index -- each of them
//! a copy of the sheep's with one number changed, and the day somebody tunes
//! the sheep's hearing the lamb stays deaf. And it could not *grow*: an entity
//! that changes species half way through its life is a despawn and a spawn,
//! which the client draws as one animal vanishing and another appearing where
//! it stood.
//!
//! So a young animal is an adult with a **growth** beside it, nought at birth
//! and one when grown, and every rule that differs reads it through a function
//! here: how big it is drawn and collides ([`size`]), how fast it can go
//! ([`speed`]), how much it takes to kill ([`strength`]), what it leaves
//! ([`drops`]). One byte of it crosses the wire ([`to_wire`]).
//!
//! Rejected as well: **a young flag and a fixed small size**, which is a
//! lamb that is a lamb until the moment it is a sheep, a pop a player would
//! see happen.
//!
//! ## Why young at all
//!
//! The design principle in `CLAUDE.md`: a mechanic should create a decision.
//! A doe with a fawn is a doe that runs slower -- she will not leave it
//! (`MOTHER_LEASH`) -- so she is the easy kill, and the fawn is worth almost
//! nothing ([`drops`]) and will be a deer in a few days if it is left alone.
//! A sow with piglets does not break off a fight. That is the decision: the
//! easy meat now or the herd next week, and the angry mother either way.

use crate::animals::Species;
use crate::season::Season;
use crate::types::BlockId;

/// Grown: the growth of every adult, and of everything the spawner makes.
pub const GROWN: f32 = 1.0;

/// How many in-game days a newborn takes to become an adult.
///
/// **Days, not minutes**, because a young animal is a thing a player notices
/// over a trip rather than inside a fight: a fawn seen on the way out to the
/// hills is still a fawn on the way back, and is a deer the week after. Four
/// is two trips' worth at the default day length. Shorter and the young would
/// be a flicker nobody sees; much longer and the spawner's own turnover
/// (animals are forgotten when nobody is near, `animals::forget_the_distant`)
/// would take most of them before they grew, which is a young that never
/// grows as far as any player can tell.
pub const GROWN_DAYS: f32 = 4.0;

/// How big a newborn is beside its parent, as a fraction of every dimension.
///
/// **A little over half**, which is what a fawn or a lamb actually is at its
/// shoulder, and the lower bound on what reads: at a third, a piglet at twenty
/// blocks is a few pixels and a player cannot tell it from a hare.
pub const BIRTH_SIZE: f32 = 0.55;

/// How fast a newborn can go, as a fraction of the adult's walk and run.
///
/// Slower than a person walking for most species -- which is the point of a
/// mother that waits: a fawn at a full adult's speed would need no mother.
pub const BIRTH_SPEED: f32 = 0.6;

/// What a newborn's health is, as a fraction of the adult's; see [`strength`].
pub const BIRTH_STRENGTH: f32 = 0.3;

/// How far the young may be from the mother, in blocks, before she stops for
/// it -- stands and turns to face it -- and it comes to her.
///
/// Inside a herd's own spacing (`animals::HERD_RADIUS` is a good deal wider),
/// because a herd is animals that drift apart and a mother and young are not.
pub const MOTHER_LEASH: f32 = 4.0;

/// How far behind she lets it fall while they are running, in blocks, before
/// she slows to let it close. Tighter than the grazing leash, because a
/// running mother that notices at four blocks has left a fawn at ten by the
/// time it has closed.
pub const FLEE_LEASH: f32 = 2.5;

/// How much of the young's own running pace the mother keeps to while they
/// run together: a little under it, so the young gains on her rather than
/// holding a gap.
pub const MOTHER_PACE: f32 = 0.9;

/// How far a partner has to be, in blocks, for two adults to count as "near
/// each other" for a birth outside spring. See [`births_per_day`].
pub const PARTNER_RANGE: f32 = 8.0;

/// Days after a birth before the same mother can give birth again.
///
/// Longer than [`GROWN_DAYS`], so one mother has one young at a time -- a
/// ewe followed by a line of lambs of every size would be a nursery, not a
/// flock -- and so a herd that is left alone grows by a few a week and not
/// by doubling.
pub const BIRTH_REST_DAYS: f32 = 6.0;

/// Does this species raise young in the wild?
///
/// **The grazers, and the boar.** The herd species are the ones whose young a
/// player ought to see among the adults, and the boar is the one whose
/// mother is the reason not to walk up to a piglet. Not the hunters: a wolf
/// pup or a lion cub is a den, and a den is a feature of its own. Not the
/// hare, the rat or the fowl, whose young are the size of the adult's own
/// rounding error. Not the fish, which have no mothers to speak of.
pub fn breeds(species: Species) -> bool {
    matches!(
        species,
        Species::Deer | Species::Boar | Species::Sheep | Species::Zebra | Species::Antelope
    )
}

/// How many births one calm, fed adult has a day in `season`, if it has a
/// partner near it or not.
///
/// **Spring is when the young come**, and it does not need a partner in
/// sight, because by spring the pairing happened months ago. Outside it a
/// birth needs two adults near each other and safe, and comes a good deal
/// less often -- so a herd a player has stopped frightening fills out even
/// in autumn, and a herd they keep hunting does not. Winter: none.
pub fn births_per_day(season: Season, partner_near: bool) -> f32 {
    match (season, partner_near) {
        (Season::Spring, _) => 0.8,
        (Season::Winter, _) => 0.0,
        (_, true) => 0.25,
        (_, false) => 0.0,
    }
}

/// How far from grown, in the one shape every rule below uses: `growth` as
/// a number the rules can trust, nought to one.
#[inline]
fn grown(growth: f32) -> f32 {
    if growth.is_finite() {
        growth.clamp(0.0, GROWN)
    } else {
        GROWN
    }
}

#[inline]
fn between(at_birth: f32, growth: f32) -> f32 {
    at_birth + (1.0 - at_birth) * grown(growth)
}

/// Is this growth still young?
#[inline]
pub fn is_young(growth: f32) -> bool {
    grown(growth) < GROWN
}

/// How big it is, as a fraction of the adult in every dimension: what it is
/// drawn at and what it collides as, so what you see is what you can hit.
#[inline]
pub fn size(growth: f32) -> f32 {
    between(BIRTH_SIZE, growth)
}

/// How fast it can go, as a fraction of the adult's walk and run.
#[inline]
pub fn speed(growth: f32) -> f32 {
    between(BIRTH_SPEED, growth)
}

/// Its health, and the weight behind a blow it lands, as a fraction of the
/// adult's. A piglet's tusks are a piglet's.
#[inline]
pub fn strength(growth: f32) -> f32 {
    between(BIRTH_STRENGTH, growth)
}

/// Does a death at this growth leave the species' carcass, or only a heap?
///
/// **A young animal leaves no carcass**, and the reason is the carcass block:
/// it is drawn from the *adult's* model at the adult's size and butchers into
/// the adult's yield (`animals::butchering`), and a fawn killed at a week old
/// lying there as a full-grown deer with two hides in it is the one outcome
/// that would make the young a way to farm deer. A heap of what a small body
/// actually holds ([`drops`]) is the honest answer. Nearly grown is grown.
#[inline]
pub fn leaves_carcass(growth: f32) -> bool {
    grown(growth) >= 0.8
}

/// What a young one leaves, where it leaves a heap: the adult's
/// `Species::drops`, each count cut to its [`strength`] and rounded down --
/// and never nothing at all, so a kill is always worth one mouthful.
pub fn drops(species: Species, growth: f32) -> Vec<(BlockId, u32)> {
    let share = strength(growth);
    let mut out: Vec<(BlockId, u32)> = species
        .drops()
        .iter()
        .map(|&(block, count)| (block, (count as f32 * share).floor() as u32))
        .filter(|&(_, count)| count > 0)
        .collect();
    if out.is_empty() {
        if let Some(&(block, _)) = species.drops().first() {
            out.push((block, 1));
        }
    }
    out
}

/// Growth on the wire: one byte, 255 for grown.
///
/// **A byte, because a size is all a watcher needs from it**, and 255 steps
/// between a newborn and an adult is finer than any screen can show. Grown is
/// the top of the range rather than nought so that a snapshot built by
/// something that never heard of young -- a test, a mod's fake entity -- that
/// writes the byte at its maximum reads as an adult; see `from_wire`.
#[inline]
pub fn to_wire(growth: f32) -> u8 {
    (grown(growth) * 255.0).round() as u8
}

/// ...and back.
#[inline]
pub fn from_wire(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// What the young of a species is called, as an identifier: the key the
/// client's names table looks it up by (`ui::names::animal`). `None` for a
/// species whose young are not born in this world.
///
/// Its own word where English has one, because "a young deer" is how nobody
/// talks; the antelope's is a calf, as a zebra's is a foal.
pub fn young_name(species: Species) -> Option<&'static str> {
    match species {
        Species::Deer => Some("fawn"),
        Species::Sheep => Some("lamb"),
        Species::Boar => Some("piglet"),
        Species::Zebra => Some("foal"),
        Species::Antelope => Some("antelope_calf"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newborn_is_smaller_slower_and_weaker_and_a_grown_one_is_the_adult() {
        assert!(size(0.0) < 0.6 && speed(0.0) < 0.7 && strength(0.0) < 0.5);
        assert_eq!((size(GROWN), speed(GROWN), strength(GROWN)), (1.0, 1.0, 1.0));
        // ...and on the way, never back down.
        let mut last = 0.0;
        for step in 0..=20 {
            let now = size(step as f32 / 20.0);
            assert!(now >= last, "it shrank at {step}/20");
            last = now;
        }
    }

    #[test]
    fn nonsense_growth_is_an_adult_rather_than_an_invisible_animal() {
        assert_eq!(size(f32::NAN), 1.0);
        assert_eq!(size(-3.0), BIRTH_SIZE);
        assert_eq!(size(7.0), 1.0);
    }

    #[test]
    fn growth_survives_the_wire_to_within_a_step() {
        for step in 0..=10 {
            let growth = step as f32 / 10.0;
            assert!((from_wire(to_wire(growth)) - growth).abs() < 1.0 / 255.0);
        }
        assert_eq!(to_wire(GROWN), 255);
        assert!(!is_young(from_wire(255)), "a grown animal arrived young");
    }

    #[test]
    fn a_young_kill_is_worth_less_than_the_adult_and_never_nothing() {
        for &species in Species::ALL.iter().filter(|&&s| breeds(s)) {
            let adult: u32 = species.drops().iter().map(|d| d.1).sum();
            let young: u32 = drops(species, 0.0).iter().map(|d| d.1).sum();
            assert!(young >= 1, "a newborn {} left nothing", species.name());
            assert!(young < adult, "a newborn {} was worth the adult", species.name());
            assert!(!leaves_carcass(0.0));
        }
        assert!(leaves_carcass(GROWN));
    }

    #[test]
    fn every_species_that_breeds_has_a_name_for_its_young() {
        for &species in Species::ALL {
            assert_eq!(breeds(species), young_name(species).is_some(), "{}", species.name());
        }
    }

    #[test]
    fn spring_brings_young_and_winter_none() {
        assert!(births_per_day(Season::Spring, false) > births_per_day(Season::Summer, true));
        assert_eq!(births_per_day(Season::Winter, true), 0.0);
        assert_eq!(births_per_day(Season::Autumn, false), 0.0, "a lone adult bred outside spring");
    }
}
