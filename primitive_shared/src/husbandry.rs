//! **Keeping animals**: what a sheep or a boar thinks of the person feeding
//! it, and what a kept animal gives back.
//!
//! The rules only -- numbers and the days they run over. Where an animal
//! walks, who it follows and how a pen holds it are the server's
//! (`logic::animals`), because they need the world; what is decided here is
//! the part both ends and the save file have to agree on.
//!
//! **What this is for is a decision, not a larder.** A wild ewe killed is
//! three wool, a hide and two meat *today*. The same ewe kept is two wool
//! every few days, a bowl of milk while she has a lamb, lambs in season and
//! dung for the furrows -- and it costs grain a player could have eaten, a
//! walled pen, a door somebody has to shut, and a visit every day or two or
//! she is off. Neither answer is always right, which is the test
//! `CLAUDE.md` sets for a mechanic.
//!
//! Rejected: **taming as one gesture** (feed once and it is yours). Then a
//! sheep is an item that walks, and the only question is how many; taming
//! by *trust*, a few feeds apart over most of a day, makes the first pen a
//! small project and the tenth sheep a real cost. Rejected too: **a kept
//! animal that never needs anything.** Everything it gives is then free,
//! and "kill it" stops being an answer anybody would pick.

use serde::{Deserialize, Serialize};

use crate::animals::Species;
use crate::types::{block_kind, BlockId};

/// Can this species be kept at all?
///
/// **The sheep and the boar** -- the two animals here whose tame cousins
/// every farm in the world has. Not the deer, which has never been kept
/// in a pen that held it; not the wolf, whose taming is a dog and a
/// feature of its own; not the fowl, whose whole worth is eggs and eggs
/// are another mechanic, not an extra line here. Not the goat, which does
/// not exist: the ewe gives the milk (`MILK_EVERY_DAYS`).
///
/// **And the horse**, which is kept for what it does rather than for what it
/// gives, and is the one animal here that has to be *broken* as well as
/// tamed: see [`needs_breaking`].
pub fn tameable(species: Species) -> bool {
    matches!(species, Species::Sheep | Species::Boar | Species::Horse)
}

/// Feeds from the first to trust: three for everything but the horse.
///
/// **Five for a horse**, a quarter of a day apart like the rest
/// (`SATED_DAYS`), so gentling one is the best part of two days of coming
/// back to a herd that bolts if you walk at it -- and then it still has to be
/// sat on. A horse that came as cheaply as a ewe would be a vehicle found in
/// a field; one that costs this is a thing a player decided to have.
pub fn feeds_to_tame(species: Species) -> f32 {
    match species {
        Species::Horse => HORSE_FEEDS_TO_TAME,
        _ => FEEDS_TO_TAME,
    }
}

/// Feeds that gentle a wild horse. See [`feeds_to_tame`].
pub const HORSE_FEEDS_TO_TAME: f32 = 5.0;

/// Does trust stop short of tame, until somebody has sat on it?
///
/// **The horse's, and only the horse's.** A ewe fed three times follows the
/// grain home; a horse fed five times will take food from your hand and still
/// throw you the first time you get on it. What finishes it is the mounting
/// ([`thrown`]), so the last step of taming a horse is a few falls, and the
/// feeding is what makes it stand still long enough for them.
pub fn needs_breaking(species: Species) -> bool {
    matches!(species, Species::Horse)
}

/// The chance a gentled horse throws its rider, by how many times it has
/// been got on already.
///
/// **Nearly always on the first try, never on the fourth.** Nine in ten, then
/// six, then three, then none: a few falls, which is what breaking a horse
/// is, and a promise that it ends -- a horse that threw you at a fixed one in
/// two for ever would be a slot machine with a mane. See [`thrown`].
pub const THROW_CHANCES: [f32; 4] = [0.9, 0.6, 0.3, 0.0];

/// What a throw costs the rider, in health: a hard landing, not a wound.
pub const THROW_DAMAGE: f32 = 2.0;

/// Seconds after a throw before it will let anybody near its back again: long
/// enough that the next try is a decision, short enough that it is today.
pub const SETTLE_SECONDS: f32 = 8.0;

/// Whether the `attempt`th time on a gentled horse ends on the ground, for a
/// roll in `0..1`. Nought is the first attempt.
pub fn thrown(attempt: u8, roll: f32) -> bool {
    let chance = THROW_CHANCES[(attempt as usize).min(THROW_CHANCES.len() - 1)];
    roll < chance
}

/// Condition lost a day by a kept horse standing out in the rain with no
/// roof over it.
///
/// **What "shelter it" means, as a number.** Half a day's feeding's worth
/// every day of rain: a horse left out through a wet week is skin and bone
/// (`THRIVING`), and a horse out of condition has less wind to gallop with
/// (`Keeping::most_wind`). A roof -- a lean-to, a stable -- is the answer, and
/// it is a building a player makes because they have a horse.
pub const EXPOSED_CONDITION_PER_DAY: f32 = 0.5;

/// How much a mouthful is worth to the animal that takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ration {
    /// Keeps it: trust, a full belly, its condition. Cut grass for a sheep,
    /// berries and toadstools for a boar -- what it would have found for
    /// itself.
    Fodder,
    /// Keeps it *and makes it thrive*: the fleece comes back twice as fast
    /// and it will breed. Grain for a sheep, roots and apples for a boar --
    /// **exactly the food a player eats**, which is the whole point: the
    /// grain for the ewe is grain that is not bread.
    Rich,
}

/// What `food` is to `species`, if it will eat it at all.
pub fn ration(species: Species, food: BlockId) -> Option<Ration> {
    use crate::types::{
        BLOCK_APPLE, BLOCK_BERRIES, BLOCK_FIBER, BLOCK_GRAIN, BLOCK_HAY, BLOCK_MILLET, BLOCK_MUSHROOM, BLOCK_ROOT,
    };
    match (species, block_kind(food)) {
        // Grass and grain, and the apple every horse will come across a
        // field for -- the lure, as much as a meal. Hay is grass that kept.
        (Species::Horse, BLOCK_FIBER | BLOCK_HAY) => Some(Ration::Fodder),
        (Species::Horse, BLOCK_GRAIN | BLOCK_MILLET | BLOCK_APPLE) => Some(Ration::Rich),
        (Species::Sheep, BLOCK_FIBER | BLOCK_HAY) => Some(Ration::Fodder),
        (Species::Sheep, BLOCK_GRAIN | BLOCK_MILLET) => Some(Ration::Rich),
        (Species::Boar, BLOCK_BERRIES | BLOCK_MUSHROOM) => Some(Ration::Fodder),
        (Species::Boar, BLOCK_ROOT | BLOCK_APPLE) => Some(Ration::Rich),
        _ => None,
    }
}

/// Would anything that can be kept eat this? What a lure is: the animals
/// take notice of a player holding one (see the server's `lure`).
pub fn is_feed(held: BlockId) -> bool {
    [Species::Sheep, Species::Boar, Species::Horse].into_iter().any(|s| ration(s, held).is_some())
}

/// **Is this a thing a right click on an animal means something with?**
/// Feed, a knife to shear with, an empty bowl to milk into.
///
/// Asked by the client before it turns a right click into
/// `ClientMessage::TendAnimal`, so a player carrying a stack of planks past
/// a sheep still builds rather than being told the sheep is not hungry.
pub fn is_tending_tool(held: BlockId) -> bool {
    is_feed(held)
        || crate::types::is_knife(held)
        || block_kind(held) == crate::types::BLOCK_BOWL
        // ...and a saddle or saddlebags, put on a horse with the same click.
        || crate::types::is_tack(held)
}

/// Feeds that make a wild animal tame: three, a quarter of a day apart at
/// the least (`SATED_DAYS`), so most of a day of coming back to it.
pub const FEEDS_TO_TAME: f32 = 3.0;

/// **Days after a feed before it will take another.** What stops taming
/// being three clicks in a second, and what makes a flock something you
/// visit rather than something you stand beside with a sack.
pub const SATED_DAYS: f32 = 0.25;

/// Days unfed before it goes hungry: from then on it trusts you less every
/// day (`TRUST_LOST_PER_DAY`), its fleece stops growing and it loses
/// condition.
///
/// **A day and a half, so "feed them every day" is the rule and a missed
/// day is not a disaster.** Shorter made a flock a chore on a server with
/// long days; longer made the cost a thing a player only ever read about.
pub const HUNGRY_AFTER_DAYS: f32 = 1.5;

/// How fast hunger comes **standing on grass**: half as fast. A pen on a
/// meadow feeds its flock half their keep, which is what makes a big
/// pen worth the wall -- and a small pen on bare earth worth the grain.
pub const GRAZING_HUNGER: f32 = 0.5;

/// Trust lost a day once hungry. A tame animal fed nothing goes wild again
/// once it is under `LEAVES_BELOW`: about three days after it went hungry,
/// four and a half after the last feed.
pub const TRUST_LOST_PER_DAY: f32 = 0.22;

/// **Under this a tame animal is not tame any more**: it forgets its home
/// and walks off -- over the wall, if the wall is a single block high.
/// A third, one feed's worth, so the way back is a feed rather than a
/// taming from the start.
pub const LEAVES_BELOW: f32 = 1.0 / FEEDS_TO_TAME;

/// Condition lost a day while hungry. Nought is an animal that is skin and
/// bone: no lambs, no milk (`THRIVING`).
pub const CONDITION_LOST_PER_DAY: f32 = 0.25;

/// Condition a feed puts back.
pub const CONDITION_PER_FEED: f32 = 0.25;

/// At or above this it breeds and gives milk.
pub const THRIVING: f32 = 0.5;

/// Days a rich ration keeps it well fed: the fleece grows at
/// `FLEECE_DAYS_WELL_FED` and it may breed.
pub const WELL_FED_DAYS: f32 = 1.0;

/// Days for a sheared fleece to grow back, well fed.
pub const FLEECE_DAYS_WELL_FED: f32 = 3.0;

/// ...and on fodder or grass alone. Twice as long: see `Ration::Rich`.
pub const FLEECE_DAYS: f32 = 6.0;

/// Wool off one full fleece. Two, where a carcass gives three
/// (`Species::butchering`): the shears leave the skin, and a sheep sheared
/// every three days has given the dead one's wool by the end of the week.
pub const FLEECE_WOOL: u32 = 2;

/// **Once a day, and only while she has a lamb at foot.** And the milk is
/// the lamb's: a lamb whose mother was milked in the last day grows at
/// `MILKED_LAMB_GROWTH` (the server's `raise_young`). A bowl today, or a
/// grown sheep a day sooner.
pub const MILK_EVERY_DAYS: f32 = 1.0;

/// How fast a lamb grows while its mother is being milked, against its
/// ordinary rate.
pub const MILKED_LAMB_GROWTH: f32 = 0.5;

/// Days between pats of dung from a kept animal that is eating.
pub const DUNG_EVERY_DAYS: f32 = 0.5;

/// **Does the turf feed a flock at this time of year?** Everywhere but in
/// winter.
///
/// The year used to reach a pen only as cold: a flock on a meadow grazed
/// half its keep in midwinter as in May (`GRAZING_HUNGER`), so a pen once
/// built was as cheap in the tenth day of snow as in the first of summer,
/// and nothing a player did in summer was *for* the flock's winter. Now the
/// grass stops with the year: from the first day of winter a pen is bare
/// ground whatever is under the snow, and what a flock eats is what was cut
/// and dried for it (`types::BLOCK_HAY`) or fed by hand that day.
///
/// **The calendar, not the frost**, which is the other reading and the one
/// `growth` takes for crops. A crop dies in one night's frost and that is
/// its whole story; grazing is a flock's week, and a pen whose grass came
/// and went with every warm afternoon of February would be a rule no
/// player could plan a stack of hay against. A warm coast's winter is a
/// winter too: the grass there is growing and the sheep are still eating
/// the stack, which is generous to nobody and simple to know.
pub fn grazes(season: crate::season::Season) -> bool {
    season != crate::season::Season::Winter
}

/// How far from a haystack a kept animal eats from it, in blocks, across;
/// [`MANGER_RISE`] up and down.
///
/// **Five: a pen.** Far enough that a stack in the corner of a walled
/// pen of ten by ten reaches every sheep in it, near enough that a stack by
/// the house does not feed a flock loose in the meadow beyond -- where the
/// stack stands is where the flock has to be.
pub const MANGER_REACH: i32 = 5;
/// See [`MANGER_REACH`].
pub const MANGER_RISE: i32 = 2;

/// **Days of want before a kept animal goes to the stack**: one, half a
/// day before it would go hungry (`HUNGRY_AFTER_DAYS`).
///
/// Before hungry, so an animal with a stack beside it never loses trust or
/// condition for want of a bite it could have taken; not at once, so a
/// stack is eaten a bite a day an animal and not a bite an hour -- the rate
/// the arithmetic under [`HAYSTACK_HOLDS`](crate::types::HAYSTACK_HOLDS)
/// promises a player putting one up.
pub const STACK_AFTER_DAYS: f32 = 1.0;

/// What a kept animal will eat out of a haystack, if anything: what it
/// eats hay as. The boar does not -- a pig is wintered on roots and apples
/// out of the cellar, fed by hand, which is the other half of the larder.
pub fn eats_hay(species: Species) -> bool {
    ration(species, crate::types::BLOCK_HAY).is_some()
}

/// Everything a person has done to one animal, and what it has come to.
///
/// **On the animal only once somebody has fed it** -- `None` for every wild
/// animal alive -- and that is also the rule for what is saved with the
/// world: an animal with one of these was touched, and is kept across a
/// restart and never simply forgotten when nobody is near (the server's
/// `forget_the_distant` parks it instead).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keeping {
    /// How far it trusts people, nought to one.
    pub trust: f32,
    /// Tame: follows food from further, never flees a person, keeps `home`.
    ///
    /// **A flag beside `trust` and not a threshold on it**, because going
    /// tame and going wild again are different numbers on purpose: it takes
    /// three feeds to make it tame and two missed days' worth of trust to
    /// lose it (`LEAVES_BELOW`). One threshold would be an animal that
    /// flickers between the two on every hungry evening.
    pub tame: bool,
    /// Where it lives: **wherever it was last fed while tame.** So leading a
    /// flock into a pen and feeding them there is what makes the pen home,
    /// and there is no separate gesture to learn.
    pub home: Option<(f32, f32, f32)>,
    /// Days of want: nought just after a feed, up by one a day -- by half
    /// on grass (`GRAZING_HUNGER`).
    pub hunger: f32,
    /// Days of rich feeding left (`WELL_FED_DAYS`).
    pub well_fed: f32,
    /// A sheep's coat, nought just sheared, one ready to shear. A wild
    /// sheep comes with a full one.
    pub fleece: f32,
    /// Nought to one: see `THRIVING`.
    pub condition: f32,
    /// Days since she was last milked.
    pub since_milked: f32,
    /// Days to the next pat of dung.
    pub dung_due: f32,
}

impl Default for Keeping {
    fn default() -> Self {
        Self::wild()
    }
}

/// Why a feed was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// It does not eat that.
    NotItsFood,
    /// It ate less than `SATED_DAYS` ago.
    Sated,
}

impl Keeping {
    /// A wild animal the first time somebody holds food out to it.
    pub fn wild() -> Self {
        Self {
            trust: 0.0,
            tame: false,
            home: None,
            // Hungry already: a wild animal takes the first feed at once.
            hunger: SATED_DAYS,
            well_fed: 0.0,
            fleece: 1.0,
            condition: 1.0,
            since_milked: MILK_EVERY_DAYS,
            dung_due: DUNG_EVERY_DAYS,
        }
    }

    /// **Born to a kept mother, it is kept**: tame from the first day, at her
    /// home, and with nothing on its back yet.
    pub fn born_to(mother: &Keeping) -> Self {
        Self {
            trust: 1.0,
            tame: true,
            home: mother.home,
            hunger: 0.0,
            well_fed: 0.0,
            fleece: 0.0,
            condition: mother.condition,
            since_milked: MILK_EVERY_DAYS,
            dung_due: DUNG_EVERY_DAYS,
        }
    }

    /// One feed of `food` held out to `species`, standing `at`. `Ok(true)` if
    /// that feed is the one that tamed it.
    pub fn feed(&mut self, species: Species, food: BlockId, at: (f32, f32, f32)) -> Result<bool, Refused> {
        let ration = ration(species, food).ok_or(Refused::NotItsFood)?;
        if self.hunger < SATED_DAYS {
            return Err(Refused::Sated);
        }
        let was_tame = self.tame;
        self.trust = (self.trust + 1.0 / feeds_to_tame(species)).min(1.0);
        // A hair under one third times three is 0.999..., and a sheep that
        // wanted a fourth feed for a rounding error would be a bug report.
        if self.trust >= 1.0 - 1e-4 {
            self.trust = 1.0;
            // A horse is only gentled by food: see `needs_breaking`.
            if !needs_breaking(species) {
                self.tame = true;
            }
        }
        if self.tame {
            self.home = Some(at);
        }
        self.hunger = 0.0;
        self.condition = (self.condition + CONDITION_PER_FEED).min(1.0);
        if ration == Ration::Rich {
            self.well_fed = WELL_FED_DAYS;
        }
        Ok(self.tame && !was_tame)
    }

    /// Trusts you all the way and is not yet tame: a horse that has been fed
    /// enough and has not been sat on (`needs_breaking`).
    pub fn gentled(&self) -> bool {
        !self.tame && self.trust >= 1.0 - 1e-4
    }

    /// A gentled horse stood for its rider: it is tame, and home is where it
    /// was broken.
    pub fn break_in(&mut self, at: (f32, f32, f32)) {
        self.trust = 1.0;
        self.tame = true;
        self.home = Some(at);
    }

    /// `days` out in the rain with nothing over it. See
    /// [`EXPOSED_CONDITION_PER_DAY`].
    pub fn exposed(&mut self, days: f32) {
        if days.is_finite() && days > 0.0 {
            self.condition = (self.condition - EXPOSED_CONDITION_PER_DAY * days).max(0.0);
        }
    }

    /// How long a kept horse can gallop, in seconds: all of
    /// `horse::GALLOP_SECONDS` in condition and under half of it as skin and
    /// bone. **Condition, not hunger**, because hunger is already the other
    /// rule (a hungry horse will not gallop at all, `will_gallop`), and what a
    /// wet week or a lean month costs should be a horse that tires, not one
    /// that refuses.
    pub fn most_wind(&self) -> f32 {
        crate::horse::GALLOP_SECONDS * (0.4 + 0.6 * self.condition.clamp(0.0, 1.0))
    }

    /// Whether it will be asked for a gallop: fed within the last day and a
    /// half. A hungry horse walks and trots, and that is what makes feeding it
    /// a thing the rider does before the ride rather than after.
    pub fn will_gallop(&self) -> bool {
        !self.is_hungry()
    }

    /// Hungry: past `HUNGRY_AFTER_DAYS` since it was last fed.
    pub fn is_hungry(&self) -> bool {
        self.hunger >= HUNGRY_AFTER_DAYS
    }

    /// Fed, rich, and in condition: what breeding asks of both parents.
    pub fn thriving(&self) -> bool {
        self.tame && !self.is_hungry() && self.well_fed > 0.0 && self.condition >= THRIVING
    }

    /// Is the fleece ready for the knife?
    pub fn fleece_ready(&self) -> bool {
        // Twelve quarter days of a third each is not quite one in `f32`.
        self.fleece >= 1.0 - 1e-4
    }

    /// Has it gone back to being nobody's? Wild, and with no trust left to
    /// build on: the server drops the keeping altogether, and it can be
    /// forgotten like any wild animal.
    pub fn forgotten(&self) -> bool {
        !self.tame && self.trust <= 0.0
    }

    /// `days` go by, on grass or not. Returns the pats of dung it left.
    ///
    /// **Called with a tick's sliver of a day** in the world and with the
    /// whole absence at once when a parked flock is met again; the second is
    /// cut into quarter days so it comes out as the first would have
    /// (a sheep that went hungry halfway through must stop growing wool
    /// halfway through, not at the end).
    pub fn pass_days(&mut self, days: f32, grazing: bool) -> u32 {
        if !days.is_finite() || days <= 0.0 {
            return 0;
        }
        let mut left = days;
        let mut dung = 0;
        while left > 0.0 {
            let step = left.min(0.25);
            left -= step;
            dung += self.pass(step, grazing);
        }
        dung
    }

    fn pass(&mut self, days: f32, grazing: bool) -> u32 {
        let hungry = self.is_hungry();
        if !hungry {
            let fleece_days = if self.well_fed > 0.0 { FLEECE_DAYS_WELL_FED } else { FLEECE_DAYS };
            self.fleece = (self.fleece + days / fleece_days).min(1.0);
        }
        self.hunger += days * if grazing { GRAZING_HUNGER } else { 1.0 };
        self.well_fed = (self.well_fed - days).max(0.0);
        self.since_milked += days;
        if hungry {
            self.trust = (self.trust - TRUST_LOST_PER_DAY * days).max(0.0);
            self.condition = (self.condition - CONDITION_LOST_PER_DAY * days).max(0.0);
            if self.tame && self.trust < LEAVES_BELOW {
                // **Off.** Not dead and not gone: wild again, homeless, and
                // over the wall if the wall lets it. See `LEAVES_BELOW`.
                self.tame = false;
                self.home = None;
            }
            return 0;
        }
        // Only an animal that is eating leaves anything behind.
        self.dung_due -= days;
        let mut dung = 0;
        while self.dung_due <= 0.0 {
            dung += 1;
            self.dung_due += DUNG_EVERY_DAYS;
        }
        dung
    }

    /// **`days` go by for an animal that may have a stack beside it**: the
    /// days as `pass_days` has them, in quarter days, with the ground's
    /// grazing asked of the season each quarter falls in, and a bite from
    /// the stack whenever it is due one (`STACK_AFTER_DAYS`).
    ///
    /// `hay` is the bites the stacks within reach hold, and goes down by
    /// what was eaten; `from_day` is the world's day the time starts on,
    /// or `None` where nobody has a calendar (the season is then summer's,
    /// as it always was). Returns the dung left and the bites taken.
    ///
    /// **One function for a watched pen and a parked one**, and that is the
    /// point of it: a flock nobody was near for a week has to come out of
    /// that week exactly as a flock somebody watched would have, or a player
    /// learns to stand in the pen all winter, which is the chore a stack is
    /// there to take away.
    pub fn winter_through(
        &mut self,
        species: Species,
        days: f32,
        from_day: Option<f32>,
        pasture: bool,
        hay: &mut u32,
    ) -> (u32, u32) {
        if !days.is_finite() || days <= 0.0 {
            return (0, 0);
        }
        let takes_hay = self.tame && eats_hay(species);
        let (mut dung, mut eaten, mut gone) = (0, 0, 0.0);
        while gone < days {
            let step = (days - gone).min(0.25);
            if takes_hay && *hay > 0 && self.hunger >= STACK_AFTER_DAYS {
                self.eat_from_the_stack();
                *hay -= 1;
                eaten += 1;
            }
            let grazing = pasture
                && from_day.is_none_or(|day| grazes(crate::season::Season::at(day + gone)));
            dung += self.pass(step, grazing);
            gone += step;
        }
        (dung, eaten)
    }

    /// A bite from a haystack: fed, as a handful of grass held out is, and
    /// **no trust for it** -- nobody held it out. Home stays where it was:
    /// a stack is not a person, and a sheep that moved house to whichever
    /// stack it last ate from would be a flock that wandered off along a
    /// row of them.
    fn eat_from_the_stack(&mut self) {
        self.hunger = 0.0;
        self.condition = (self.condition + CONDITION_PER_FEED).min(1.0);
    }

    /// Shears it: the wool, if the coat was ready, and a bare back.
    pub fn shear(&mut self) -> Option<u32> {
        if !self.tame || !self.fleece_ready() {
            return None;
        }
        self.fleece = 0.0;
        Some(FLEECE_WOOL)
    }

    /// Milks her if she can be: tame, in condition, a lamb at foot (the
    /// caller's to know) and a day since the last time.
    pub fn milk(&mut self, has_lamb: bool) -> bool {
        if !self.tame || !has_lamb || self.condition < THRIVING || self.since_milked < MILK_EVERY_DAYS {
            return false;
        }
        self.since_milked = 0.0;
        true
    }

    /// Her lamb is short of milk: she was milked within the last day.
    pub fn lamb_goes_short(&self) -> bool {
        self.since_milked < MILK_EVERY_DAYS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_BOWL, BLOCK_FIBER, BLOCK_FLINT_KNIFE, BLOCK_GRAIN, BLOCK_PLANKS, BLOCK_ROOT};

    const PEN: (f32, f32, f32) = (4.0, 20.0, 4.0);

    fn tamed() -> Keeping {
        let mut k = Keeping::wild();
        for _ in 0..3 {
            let _ = k.feed(Species::Sheep, BLOCK_GRAIN, PEN);
            k.pass_days(SATED_DAYS, false);
        }
        k
    }

    #[test]
    fn a_sheep_eats_grain_and_grass_and_a_boar_eats_roots_and_neither_eats_planks() {
        assert_eq!(ration(Species::Sheep, BLOCK_GRAIN), Some(Ration::Rich));
        assert_eq!(ration(Species::Sheep, BLOCK_FIBER), Some(Ration::Fodder));
        assert_eq!(ration(Species::Boar, BLOCK_ROOT), Some(Ration::Rich));
        assert_eq!(ration(Species::Sheep, BLOCK_ROOT), None);
        assert_eq!(ration(Species::Sheep, BLOCK_PLANKS), None);
        assert!(!tameable(Species::Deer) && !tameable(Species::Wolf));
    }

    #[test]
    fn a_right_click_with_planks_is_not_a_tending_gesture() {
        assert!(is_tending_tool(BLOCK_GRAIN) && is_tending_tool(BLOCK_FLINT_KNIFE) && is_tending_tool(BLOCK_BOWL));
        assert!(!is_tending_tool(BLOCK_PLANKS));
    }

    #[test]
    fn three_feeds_a_quarter_day_apart_tame_a_wild_sheep_and_three_at_once_do_not() {
        let mut hasty = Keeping::wild();
        assert_eq!(hasty.feed(Species::Sheep, BLOCK_GRAIN, PEN), Ok(false));
        assert_eq!(hasty.feed(Species::Sheep, BLOCK_GRAIN, PEN), Err(Refused::Sated));
        assert!(!hasty.tame);
        let k = tamed();
        assert!(k.tame, "three spaced feeds did not tame it");
        assert_eq!(k.home, Some(PEN), "home is where it was fed");
    }

    #[test]
    fn a_tame_sheep_nobody_feeds_goes_wild_in_a_few_days_and_one_fed_daily_never_does() {
        let mut left = tamed();
        left.pass_days(6.0, false);
        assert!(!left.tame && left.home.is_none(), "a flock nobody fed for six days is still tame");
        let mut kept = tamed();
        for _ in 0..20 {
            kept.pass_days(1.0, false);
            kept.feed(Species::Sheep, BLOCK_FIBER, PEN).expect("a day later it was not hungry enough to eat");
        }
        assert!(kept.tame && kept.condition >= THRIVING);
    }

    #[test]
    fn grass_under_the_pen_halves_the_hunger() {
        let (mut bare, mut meadow) = (tamed(), tamed());
        bare.pass_days(2.0, false);
        meadow.pass_days(2.0, true);
        assert!(bare.is_hungry() && !meadow.is_hungry());
    }

    #[test]
    fn a_fleece_comes_back_twice_as_fast_on_grain_as_on_grass() {
        let mut grain = tamed();
        let mut grass = tamed();
        assert_eq!(grain.shear(), Some(FLEECE_WOOL));
        assert_eq!(grain.shear(), None, "a bare sheep gave wool");
        grass.shear();
        grain.feed(Species::Sheep, BLOCK_GRAIN, PEN).unwrap();
        grass.feed(Species::Sheep, BLOCK_FIBER, PEN).unwrap();
        for _ in 0..3 {
            grain.pass_days(1.0, false);
            grain.feed(Species::Sheep, BLOCK_GRAIN, PEN).unwrap();
            grass.pass_days(1.0, false);
            grass.feed(Species::Sheep, BLOCK_FIBER, PEN).unwrap();
        }
        assert!(grain.fleece_ready(), "three grain-fed days did not grow a fleece: {}", grain.fleece);
        assert!(!grass.fleece_ready(), "grass grew it as fast as grain");
    }

    #[test]
    fn a_hungry_sheep_grows_no_wool_and_leaves_no_dung() {
        let mut k = tamed();
        k.shear();
        k.pass_days(HUNGRY_AFTER_DAYS, false);
        let fleece = k.fleece;
        assert_eq!(k.pass_days(2.0, false), 0, "a starving sheep left dung");
        assert_eq!(k.fleece, fleece, "a starving sheep grew wool");
    }

    #[test]
    fn a_fed_animal_leaves_two_pats_a_day() {
        let mut k = tamed();
        assert_eq!(k.pass_days(1.0, false), 2);
    }

    #[test]
    fn a_ewe_gives_milk_once_a_day_and_only_with_a_lamb() {
        let mut k = tamed();
        assert!(!k.milk(false), "milk with no lamb");
        assert!(k.milk(true));
        assert!(k.lamb_goes_short());
        assert!(!k.milk(true), "milked twice in a day");
        k.pass_days(MILK_EVERY_DAYS, false);
        assert!(k.milk(true));
    }

    #[test]
    fn only_a_fed_tame_animal_in_condition_thrives() {
        let mut k = tamed();
        assert!(k.thriving(), "fed on grain an hour ago and not thriving");
        k.pass_days(WELL_FED_DAYS + 0.1, false);
        assert!(!k.thriving(), "still thriving a day after the grain ran out");
    }

    #[test]
    fn five_feeds_gentle_a_horse_and_only_a_ride_tames_it() {
        let mut k = Keeping::wild();
        for n in 0..5 {
            assert!(!k.gentled(), "gentled after {n} feeds");
            k.feed(Species::Horse, BLOCK_GRAIN, PEN).expect("a hungry horse refused grain");
            k.pass_days(SATED_DAYS, false);
        }
        assert!(k.gentled() && !k.tame, "five feeds did not gentle it, or tamed it outright");
        k.break_in(PEN);
        assert!(k.tame && k.home == Some(PEN) && !k.gentled());
        assert!(tamed().tame, "a ewe needs breaking now");
    }

    #[test]
    fn a_gentled_horse_throws_its_rider_a_few_times_and_then_never() {
        // Every roll a player could get: the first try is nearly always a
        // fall and the fourth never is.
        assert!(thrown(0, 0.5) && thrown(0, 0.85));
        assert!(!thrown(3, 0.0) && !thrown(200, 0.0), "the fourth try threw");
        let falls = |roll: f32| (0u8..).take_while(|&n| thrown(n, roll)).count();
        assert!((0..100).all(|r| falls(r as f32 / 100.0) <= 3));
    }

    #[test]
    fn a_horse_left_out_in_the_rain_gallops_less_and_a_hungry_one_not_at_all() {
        let mut k = Keeping::wild();
        k.break_in(PEN);
        let fresh = k.most_wind();
        k.exposed(1.0);
        assert!(k.most_wind() < fresh * 0.85, "a wet day cost nothing: {} of {}", k.most_wind(), fresh);
        k.hunger = 0.0;
        assert!(k.will_gallop());
        k.pass_days(HUNGRY_AFTER_DAYS, false);
        assert!(!k.will_gallop(), "a hungry horse still galloped");
    }

    /// The world's first day of winter, found off the calendar rather than
    /// written down, so a change to where a world opens moves it here too.
    fn first_winter_day() -> f32 {
        (0..400)
            .map(|q| q as f32 * 0.25)
            .find(|&day| crate::season::Season::at(day) == crate::season::Season::Winter)
            .expect("a year with no winter in it")
    }

    #[test]
    fn a_pen_on_turf_feeds_half_a_flock_in_summer_and_none_of_it_in_winter() {
        let summer = crate::season::MIDSUMMER_WORLD_TIME;
        let winter = first_winter_day();
        let (mut grazed, mut snowed) = (tamed(), tamed());
        let mut none = 0;
        grazed.winter_through(Species::Sheep, 2.0, Some(summer), true, &mut none);
        snowed.winter_through(Species::Sheep, 2.0, Some(winter), true, &mut none);
        assert!(!grazed.is_hungry(), "a meadow pen in summer did not feed its sheep");
        assert!(snowed.is_hungry(), "the turf fed a sheep in winter");
    }

    #[test]
    fn a_flock_with_a_haystack_is_kept_through_a_winter_alone_and_one_without_goes_wild() {
        // **The trip the stack is for.** A whole winter -- ten days -- with
        // nobody there, on turf that feeds nothing in winter: the ewe with
        // a stack beside her comes out of it tame and in condition, the one
        // without is wild and skin and bone.
        let winter = first_winter_day();
        let mut kept = tamed();
        let mut left = tamed();
        let mut stack = u32::from(crate::types::HAYSTACK_HOLDS) + 4;
        let mut none = 0;
        let (_, eaten) = kept.winter_through(Species::Sheep, crate::season::SEASON_DAYS, Some(winter), true, &mut stack);
        left.winter_through(Species::Sheep, crate::season::SEASON_DAYS, Some(winter), true, &mut none);
        assert!(kept.tame && kept.condition >= THRIVING, "a ewe with hay in reach went wild or thin: {kept:?}");
        assert!(!left.tame, "a ewe left a winter with nothing to eat is still tame");
        // A bite a day, give or take the quarter day it waits for.
        assert!((8..=11).contains(&eaten), "a winter took {eaten} bites of hay, not about one a day");
        assert_eq!(stack, u32::from(crate::types::HAYSTACK_HOLDS) + 4 - eaten);
    }

    #[test]
    fn a_stack_is_not_touched_by_an_animal_that_is_not_yet_getting_hungry() {
        let mut k = tamed();
        let mut stack = 8;
        k.winter_through(Species::Sheep, 0.75, Some(first_winter_day()), false, &mut stack);
        assert_eq!(stack, 8, "a sheep fed this morning ate from the stack");
        let mut wild = Keeping::wild();
        wild.hunger = 3.0;
        wild.winter_through(Species::Sheep, 1.0, None, false, &mut stack);
        assert_eq!(stack, 8, "a wild sheep ate a player's hay");
    }

    #[test]
    fn hay_is_fodder_for_a_sheep_and_a_horse_and_nothing_to_a_boar() {
        use crate::types::BLOCK_HAY;
        assert_eq!(ration(Species::Sheep, BLOCK_HAY), Some(Ration::Fodder));
        assert_eq!(ration(Species::Horse, BLOCK_HAY), Some(Ration::Fodder));
        assert_eq!(ration(Species::Boar, BLOCK_HAY), None);
        assert!(!eats_hay(Species::Boar) && eats_hay(Species::Sheep));
        let mut boar = Keeping::wild();
        boar.tame = true;
        boar.hunger = 2.0;
        let mut stack = 8;
        boar.winter_through(Species::Boar, 1.0, None, false, &mut stack);
        assert_eq!(stack, 8, "a boar ate hay");
    }

    #[test]
    fn a_lamb_of_a_kept_ewe_is_born_tame_at_her_home() {
        let lamb = Keeping::born_to(&tamed());
        assert!(lamb.tame && lamb.home == Some(PEN) && !lamb.fleece_ready());
    }
}
