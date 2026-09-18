//! What a person is wearing, and what it does for them.
//!
//! ## One table, four slots
//!
//! Clothing and armour are not two systems here. They are one set of
//! four slots -- head, chest, legs, feet -- and one row per garment
//! saying what filling a slot with it is worth. A leather tunic and a
//! bronze cuirass differ in the numbers on their rows and in nothing
//! else, which is the whole reason armour is not "clothing plus a damage
//! bonus": a cuirass keeps the rain off exactly as a tunic does, and a
//! tunic turns a boar's tusk exactly as a cuirass does, only worse.
//!
//! That matters for more than tidiness. Every one of the four numbers on
//! a row is read by a *different* system -- protection by combat,
//! insulation by the body's heat balance, weight by the load rules that
//! already decide what a fall costs, bulk by movement -- and a design
//! with two parallel tables would mean each of those four systems asking
//! two questions and adding the answers up. Here they ask
//! [`Worn::total`] once.
//!
//! ## What is deliberately *not* here
//!
//! - **Where the garments are stored.** That is
//!   [`crate::inventory::Equipment`], because it is a container and the
//!   container rules -- swapping, weight, wear -- belong with the other
//!   container.
//! - **How hot the player is.** That is [`crate::body`], which reads
//!   insulation off here and knows nothing about leather.
//! - **How much of a blow lands.** Combat asks [`Worn::absorbed`] and
//!   does the subtraction itself.
//!
//! ## Why the numbers are shaped the way they are
//!
//! Armour is not a flat percentage off the top, because a flat
//! percentage makes the last piece of a set worth exactly as much as the
//! first and turns "do I wear the heavy one" into arithmetic with one
//! answer. Instead:
//!
//! - **Protection is per slot and is subtracted, not scaled.** A helmet
//!   stops a fixed amount of a blow that lands on a head. A blow is
//!   distributed over the slots by `HIT_SHARE`, so a full set stops more
//!   than any one piece and a bare-legged player in a cuirass is exactly
//!   as vulnerable as the fraction of hits that go low.
//! - **Insulation adds up, and the coverage is the point.** Three
//!   quarters of the heat a person loses goes through the trunk and
//!   head, so a cap and a tunic are most of staying warm and boots are a
//!   detail -- until the ground is snow.
//! - **Weight goes through the load system that already exists**, so
//!   plate makes a fall worse for the same reason a full pack does.
//! - **Bulk is the cost that has nothing to do with weight.** Plate does
//!   not merely weigh a lot; it is stiff. Bulk slows you down whether or
//!   not you are carrying anything else, which is what makes a full iron
//!   set a decision rather than an upgrade.

use crate::types::BlockId;

/// Where on a body a garment goes.
///
/// Four for clothing, and the reason there are not six is that every
/// slot has to be worth filling separately. A "belt" or a "cloak" slot
/// that only ever holds one item is a slot that is either always full
/// or always empty, and neither is a choice.
///
/// **[`Slot::Back`] is the exception that proves the rule.** It holds
/// one thing -- a rucksack -- and it is still a choice, because what it
/// buys is ten squares of pack and what it costs is its own weight, the
/// squares it needs to be carried in before it goes on, and everything
/// in it when you die. It is in this enum rather than in a system of its
/// own for the reason the doc comment at the top of this file gives
/// about armour and clothing: it is a thing you put on a body part, it
/// is stored in [`crate::inventory::Equipment`] beside the others, it
/// goes on and comes off with the same two gestures, it rides the same
/// `Outfit` to every other player, and a parallel one-slot container
/// would have duplicated all four of those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Slot {
    Head = 0,
    Chest = 1,
    Legs = 2,
    Feet = 3,
    /// What is slung over the shoulders. See [`GARMENT_SLOTS`].
    Back = 4,
}

/// How many there are. Anything indexing a worn set uses this.
pub const SLOTS: usize = 5;

/// How many of them clothe the body.
///
/// The first four. `Back` is worn and is not clothing: it stops no
/// blow, keeps no heat in and sheds no rain, so a "full set" is four
/// pieces and not five. Written down because `pieces == SLOTS` was the
/// test for a fully dressed player right up until the rucksack arrived,
/// and a reader who has not seen this line would read the new number as
/// a bug.
pub const GARMENT_SLOTS: usize = 4;

/// Every slot, in the order they are drawn and indexed -- head down, and
/// then the back.
pub const ALL_SLOTS: [Slot; SLOTS] =
    [Slot::Head, Slot::Chest, Slot::Legs, Slot::Feet, Slot::Back];

impl Slot {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// The slot at an index, if there is one.
    ///
    /// Fallible because the index comes off a socket: a client naming
    /// slot 9 is a client to ignore, not to panic on.
    pub fn from_index(index: usize) -> Option<Slot> {
        ALL_SLOTS.get(index).copied()
    }

    /// What it is called. Not translated -- like block names, this is an
    /// identifier the protocol and the save format use.
    pub fn name(self) -> &'static str {
        match self {
            Slot::Head => "head",
            Slot::Chest => "chest",
            Slot::Legs => "legs",
            Slot::Feet => "feet",
            Slot::Back => "back",
        }
    }

    /// What share of an incoming blow this slot takes.
    ///
    /// A body, roughly: the trunk is most of what there is to hit, the
    /// legs are next, and a head is a small target that is worth
    /// covering because of what is behind it rather than because of how
    /// often it is struck.
    ///
    /// The four sum to one, which is checked below. That is what makes
    /// [`Worn::absorbed`] an expected value rather than a number pulled
    /// out of the air: a full set of one material stops exactly that
    /// material's protection, and a partial set stops the share it
    /// actually covers.
    pub fn hit_share(self) -> f32 {
        match self {
            Slot::Head => 0.15,
            Slot::Chest => 0.45,
            Slot::Legs => 0.28,
            Slot::Feet => 0.12,
            // A rucksack is not armour and must not become armour by
            // accident. Zero rather than absent so the four shares still
            // sum to one -- see the test below, which is what makes
            // `Worn::absorbed` an expected value.
            Slot::Back => 0.0,
        }
    }

    /// What share of the body's heat loss goes through this slot.
    ///
    /// Also sums to one, and for the same reason: a fully clothed player
    /// gets the whole of their garments' insulation and a half-dressed
    /// one gets the half they are wearing. The distribution is not the
    /// same as `hit_share` -- a head loses far more heat than it takes
    /// blows, which is why a hat is the first thing anybody sensible
    /// puts on and the last thing a game usually models.
    pub fn heat_share(self) -> f32 {
        match self {
            Slot::Head => 0.25,
            Slot::Chest => 0.40,
            Slot::Legs => 0.22,
            Slot::Feet => 0.13,
            // Zero for `hit_share`'s reason. A pack on your back does
            // keep a little wind off, and saying so here would make a
            // rucksack a warm coat you can also put things in -- one
            // item that is the answer to two questions, which is the
            // shape of feature this game refuses.
            Slot::Back => 0.0,
        }
    }
}

/// What one garment is worth.
///
/// Every field is *per garment*, before its slot's share is applied. So
/// a row reads as "what this would be worth if a person were made
/// entirely of this slot", which is the only reading under which two
/// rows can be compared at a glance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Garment {
    pub slot: Slot,
    /// Points of damage this stops, before the slot's share.
    ///
    /// Subtracted rather than scaled -- see the module note. Against the
    /// four-point punch a player throws, a full leather set turns a
    /// third of it and a full iron set nearly all of it, which is what
    /// armour is for and why nobody in history wore it for fun.
    pub protection: f32,
    /// How much warmth it holds in, in the same units
    /// [`crate::body`] measures the world's cold in: degrees of ambient
    /// chill this garment offsets, before the slot's share.
    ///
    /// Metal is *worse* than leather here and that is not a balance
    /// decision, it is what metal is: it conducts, it has no loft, and a
    /// steel cuirass over a bare chest in the cold is a refrigerator.
    /// The consequence in play is the interesting part -- the best
    /// armour is the worst clothing, so the far north is somewhere you
    /// go dressed rather than armed.
    pub insulation: f32,
    /// How much of the rain it keeps off, 0..1.
    ///
    /// Wet clothing stops insulating (see [`crate::body`]), so this is
    /// what decides whether a shower is an inconvenience or an
    /// emergency. Leather sheds; metal sheds completely and then holds
    /// the cold against you, which the insulation figure already says.
    pub shed_rain: f32,
    /// How much it gets in the way, 0..1, independent of its weight.
    ///
    /// Summed over the worn set and turned into a speed multiplier by
    /// [`Worn::mobility`]. This is the cost that a pack does not have:
    /// you can put a pack down.
    pub bulk: f32,
    /// Degrees of hot air it keeps off the skin, before the slot's share:
    /// shade, and a way for sweat to go. Zero for everything but loose
    /// cloth. See [`crate::body::felt_ambient_shaded`] for what it does and
    /// why it is not insulation run backwards.
    pub shade: f32,
}

/// What a block is when worn, if it is anything.
///
/// `None` for every block that is not a garment, which is nearly all of
/// them -- so this doubles as "may this go in an equipment slot", and
/// the server asks exactly that before accepting a client's request to
/// put something on.
pub fn garment(block: BlockId) -> Option<Garment> {
    use crate::types::*;
    let g = |slot, protection, insulation, shed_rain, bulk| {
        Some(Garment {
            slot,
            protection,
            insulation,
            shed_rain,
            bulk,
            shade: 0.0,
        })
    };
    // A garment that stops nothing and gets in nobody's way, and keeps
    // the sun off. A second constructor rather than a sixth argument on
    // every row, because the zeros would be most of every row.
    let loose = |slot, insulation, shed_rain, shade| {
        Some(Garment {
            slot,
            protection: 0.0,
            insulation,
            shed_rain,
            bulk: 0.0,
            shade,
        })
    };
    match crate::types::block_kind(block) {
        // ---- leather ----
        //
        // The first thing anybody makes, and it is *clothing*: it stops
        // a little, it keeps a lot of heat in, it sheds a shower, and it
        // weighs nothing worth mentioning. A player in full leather is
        // not armoured, they are dressed -- which is what the far north
        // asks of them and what a boar does not care about.
        BLOCK_LEATHER_CAP => g(Slot::Head, 1.0, 7.0, 0.55, 0.00),
        BLOCK_LEATHER_TUNIC => g(Slot::Chest, 1.6, 9.0, 0.60, 0.01),
        BLOCK_LEATHER_LEGGINGS => g(Slot::Legs, 1.2, 8.0, 0.55, 0.01),
        BLOCK_LEATHER_BOOTS => g(Slot::Feet, 1.0, 6.0, 0.70, 0.00),

        // ---- wool ----
        //
        // **The warmest clothing there is, and the worst in the rain.**
        //
        // Leather is the compromise: warm enough, sheds a shower, turns
        // a little. Wool is the extreme at both ends. A full set holds
        // in about half again what leather does -- which is the
        // difference between the tundra being survivable and not -- and
        // stops nothing at all, because a jumper is not armour and
        // pretending otherwise would make leather pointless.
        //
        // `shed_rain` is the number that matters and it is deliberately
        // terrible. Wet clothing stops insulating (see `crate::body`),
        // so a player crossing the north in wool is warm right up until
        // the weather turns and then is in more trouble than if they
        // had worn leather. That is the decision the material exists to
        // create: the best coat in the game is the one you cannot wear
        // in the rain, and what a player does about that -- a leather
        // cap over the top, a fire, a roof, waiting -- is a plan rather
        // than a stat.
        //
        // No bulk worth counting. It is cloth.
        BLOCK_WOOL_CAP => g(Slot::Head, 0.0, 11.0, 0.10, 0.00),
        BLOCK_WOOL_TUNIC => g(Slot::Chest, 0.0, 14.0, 0.10, 0.01),
        BLOCK_WOOL_LEGGINGS => g(Slot::Legs, 0.0, 12.0, 0.10, 0.01),
        BLOCK_WOOL_BOOTS => g(Slot::Feet, 0.0, 9.0, 0.05, 0.00),

        // ---- cloth ----
        //
        // **The coolest thing to wear, and the only garment that is better
        // than nothing in the heat.** Every other row here holds warmth in,
        // and holding warmth in is exactly wrong in a savanna at noon (see
        // `body::HEAT_TRAP_SCALE`). The answer to hot country used to be to
        // take everything off, which is not a decision, it is undressing.
        // Cloth keeps the sun off (`shade`) and holds in next to nothing, so
        // in hot air a full set is a few degrees *cooler* than bare skin
        // while wool is a couple hotter.
        //
        // What it costs is everything else. No protection -- it is a shirt.
        // A third of leather's warmth, so a player in cloth on a northern
        // night is barely dressed. It sheds little rain. The choice a player
        // carries south is cloth for the day and something warmer in the
        // pack for the night, which is how deserts have always been crossed.
        //
        // **Only cloth casts shade.** A leather cap keeps sun off a head as
        // well, and was considered; but a hide does not breathe, and what it
        // keeps off it keeps in under it. Giving every hat shade would make
        // every set a desert set and the choice would be gone.
        //
        // The shade is heaviest on the head, because a hat is the oldest
        // answer to the sun there is, and lightest on the feet.
        BLOCK_CLOTH_CAP => loose(Slot::Head, 2.0, 0.15, 7.0),
        BLOCK_CLOTH_TUNIC => loose(Slot::Chest, 3.0, 0.20, 5.0),
        BLOCK_CLOTH_TROUSERS => loose(Slot::Legs, 3.0, 0.15, 4.0),
        BLOCK_CLOTH_WRAPS => loose(Slot::Feet, 2.0, 0.10, 2.0),

        // ---- fur ----
        //
        // **The skin with the hair still on, and the first coat anybody
        // owns.** Warmer than leather and nearly as warm as wool,
        // because that is what fur is; it sheds rain better than wool
        // and worse than a tanned hide, because the hair carries water
        // off and the skin under it was never worked. It stops about
        // what leather stops -- a hide is a hide.
        //
        // What it costs is the interesting half, and both costs are
        // real. The **bulk** is four times the leather tunic's: a raw
        // pelt over the shoulders is stiff, and a player in fur walks
        // slower than one in a shirt. The **weight** (in `blocks`) is
        // the other, and it is the one the north will not forgive --
        // untanned skin is heavy, and heavy is what sinks a swimmer now
        // (see `load::buoyancy`). Wearing a bear across a river is a
        // decision with a wrong answer in it.
        //
        // So the ladder has a rung it was missing. Fur is the coat you
        // have on the third night, leather is the coat you make when
        // there is a tannery, and wool is the coat you cross the tundra
        // in -- and each is better than the next at exactly one thing.
        BLOCK_FUR_HOOD => g(Slot::Head, 1.0, 10.0, 0.45, 0.03),
        BLOCK_FUR_CLOAK => g(Slot::Chest, 1.5, 13.0, 0.50, 0.04),

        // ---- bronze ----
        //
        // Real protection and the first of the two costs that come with
        // it: it is heavy, it is stiff, and it is a poorer coat than the
        // leather it replaced.
        BLOCK_BRONZE_HELM => g(Slot::Head, 3.4, 2.0, 0.95, 0.04),
        BLOCK_BRONZE_CUIRASS => g(Slot::Chest, 5.0, 2.5, 1.00, 0.09),
        BLOCK_BRONZE_GREAVES => g(Slot::Legs, 4.0, 2.0, 0.95, 0.07),
        BLOCK_BRONZE_BOOTS => g(Slot::Feet, 3.0, 1.5, 0.95, 0.04),

        // ---- iron ----
        //
        // The end of the ladder, and the most expensive thing in the
        // game to walk around in. A full set turns nearly everything and
        // costs about a fifth of your speed, most of your warmth, and
        // enough weight to make a survivable fall lethal.
        BLOCK_IRON_HELM => g(Slot::Head, 4.6, 1.5, 0.95, 0.05),
        BLOCK_IRON_CUIRASS => g(Slot::Chest, 6.8, 2.0, 1.00, 0.11),
        BLOCK_IRON_GREAVES => g(Slot::Legs, 5.4, 1.5, 0.95, 0.08),
        BLOCK_IRON_BOOTS => g(Slot::Feet, 4.2, 1.0, 0.95, 0.05),

        _ => None,
    }
}

/// Which slot this block goes in, if any.
///
/// The question the server asks when a client says "wear this": a
/// garment goes in its own slot and nowhere else, so there is no
/// argument about which slot a request meant.
#[inline]
pub fn slot_of(block: BlockId) -> Option<Slot> {
    // **The rucksack is not in the garment table and must not be.**
    // Every row there says what filling a slot is worth in protection,
    // insulation, rain and bulk, and a row of four zeros would make a
    // rucksack count as a piece of a "full set" (`Worn::pieces`) and
    // would put a thing you carry into the table that `body` reads to
    // decide how cold you are. What a rucksack is worth is squares, and
    // squares are `inventory`'s business -- so the only thing this
    // module has to know about it is which slot it goes in.
    if crate::types::block_kind(block) == crate::types::BLOCK_RUCKSACK {
        return Some(Slot::Back);
    }
    garment(block).map(|g| g.slot)
}

/// Whether a block may be worn at all.
#[inline]
pub fn is_wearable(block: BlockId) -> bool {
    slot_of(block).is_some()
}

/// What a worn set adds up to.
///
/// Computed once from four slots and then asked four different
/// questions, so no caller has to know how the shares work.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Worn {
    /// Points of a blow the set stops, already weighted by hit share.
    pub protection: f32,
    /// Degrees of cold the set offsets when dry, weighted by heat share.
    pub insulation: f32,
    /// How much rain the set keeps off, weighted by heat share -- the
    /// same weighting, because what a soaking costs is heat.
    pub shed_rain: f32,
    /// Summed bulk, 0..1-ish.
    pub bulk: f32,
    /// Degrees of hot air the set keeps off, weighted by heat share --
    /// the same weighting, because what shade saves is heat.
    pub shade: f32,
    /// How many slots are filled. Not used by the maths; it is what the
    /// HUD and the `/stats` line report.
    pub pieces: u8,
}

impl Worn {
    /// Adds up a set of four optional garments, in slot order.
    pub fn total(pieces: [Option<Garment>; SLOTS]) -> Worn {
        let mut worn = Worn::default();
        for (index, piece) in pieces.iter().enumerate() {
            let Some(g) = piece else { continue };
            let Some(slot) = Slot::from_index(index) else {
                continue;
            };
            worn.protection += g.protection * slot.hit_share();
            worn.insulation += g.insulation * slot.heat_share();
            worn.shed_rain += g.shed_rain * slot.heat_share();
            worn.shade += g.shade * slot.heat_share();
            worn.bulk += g.bulk;
            worn.pieces += 1;
        }
        worn
    }

    /// How much of a blow of `damage` this set turns.
    ///
    /// Never all of it. A floor of `MIN_DAMAGE_THROUGH` gets through
    /// whatever is worn, because armour that made a player immune would
    /// end every fight in the game the moment somebody finished a set --
    /// and because the fraction of a blow that lands between the plates
    /// is the reason a knife was ever worth carrying.
    pub fn absorbed(&self, damage: f32) -> f32 {
        if damage <= 0.0 || !damage.is_finite() {
            return 0.0;
        }
        let through = (damage - self.protection).max(damage * MIN_DAMAGE_THROUGH);
        (damage - through).max(0.0)
    }

    /// What is left of a blow after the set has taken its share.
    #[inline]
    pub fn through(&self, damage: f32) -> f32 {
        (damage - self.absorbed(damage)).max(0.0)
    }

    /// Speed multiplier from bulk alone.
    ///
    /// Weight is *not* in here: weight already slows a player down
    /// through [`crate::load`], which the movement code has consulted
    /// since packs got heavy, and counting it twice would make a full
    /// iron set roughly unplayable rather than merely expensive.
    pub fn mobility(&self) -> f32 {
        (1.0 - self.bulk.clamp(0.0, MAX_BULK)).clamp(MIN_MOBILITY, 1.0)
    }
}

/// The share of any blow that gets past any armour.
///
/// A tenth. Enough that a fight is always a fight; small enough that a
/// full set is worth the walk it cost.
pub const MIN_DAMAGE_THROUGH: f32 = 0.10;

/// Most bulk a set can have, however it is assembled.
pub const MAX_BULK: f32 = 0.45;

/// ...and the floor that leaves on movement.
pub const MIN_MOBILITY: f32 = 0.55;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    /// Dresses the four garment slots. The back is always empty here --
    /// a rucksack is worn and is not a garment, so it can change none of
    /// the numbers these tests are about.
    fn set(blocks: [Option<BlockId>; GARMENT_SLOTS]) -> Worn {
        let mut pieces = [None; SLOTS];
        for (i, block) in blocks.iter().enumerate() {
            pieces[i] = block.and_then(garment);
        }
        Worn::total(pieces)
    }

    #[test]
    fn fur_is_the_warm_coat_you_can_have_on_the_third_night_and_it_costs_you_to_wear_it() {
        // The whole argument for a fourth material, in one test. Fur has
        // to sit between leather and wool on warmth -- otherwise it is
        // either pointless or it makes the tannery pointless -- and it
        // has to be *worse* than both at moving about, because what it
        // costs is the only reason a player ever takes it off.
        let fur = garment(BLOCK_FUR_CLOAK).expect("fur is worn");
        let leather = garment(BLOCK_LEATHER_TUNIC).expect("leather is worn");
        let wool = garment(BLOCK_WOOL_TUNIC).expect("wool is worn");

        assert!(
            leather.insulation < fur.insulation && fur.insulation < wool.insulation,
            "fur must be warmer than leather and cooler than wool"
        );
        // Rain: the hair carries water off a fur better than a fleece
        // holds it out, and worse than a tanned skin does. That
        // ordering is what makes fur the coat for a wet cold and wool
        // the coat for a dry one.
        assert!(
            wool.shed_rain < fur.shed_rain && fur.shed_rain < leather.shed_rain,
            "fur must shed better than wool and worse than leather"
        );
        // Stiffness: a raw pelt is the bulkiest thing anybody wears
        // before metal.
        assert!(fur.bulk > leather.bulk && fur.bulk > wool.bulk);
        // ...and weight, which is the cost the water will collect. See
        // `load::buoyancy` -- a fur cloak is most of what sinks a
        // swimmer who forgot to take it off.
        let heavy = |b| crate::blocks::definition(b).weight;
        assert!(
            heavy(BLOCK_FUR_CLOAK) > heavy(BLOCK_LEATHER_TUNIC) * 2.0,
            "an untanned skin has to weigh what an untanned skin weighs"
        );
    }

    /// The three full sets, for the comparisons the materials exist to
    /// make.
    fn full_leather() -> Worn {
        set([
            Some(BLOCK_LEATHER_CAP),
            Some(BLOCK_LEATHER_TUNIC),
            Some(BLOCK_LEATHER_LEGGINGS),
            Some(BLOCK_LEATHER_BOOTS),
        ])
    }

    fn full_wool() -> Worn {
        set([
            Some(BLOCK_WOOL_CAP),
            Some(BLOCK_WOOL_TUNIC),
            Some(BLOCK_WOOL_LEGGINGS),
            Some(BLOCK_WOOL_BOOTS),
        ])
    }

    fn full_iron() -> Worn {
        set([
            Some(BLOCK_IRON_HELM),
            Some(BLOCK_IRON_CUIRASS),
            Some(BLOCK_IRON_GREAVES),
            Some(BLOCK_IRON_BOOTS),
        ])
    }

    fn full_cloth() -> Worn {
        set([
            Some(BLOCK_CLOTH_CAP),
            Some(BLOCK_CLOTH_TUNIC),
            Some(BLOCK_CLOTH_TROUSERS),
            Some(BLOCK_CLOTH_WRAPS),
        ])
    }

    #[test]
    fn cloth_is_cooler_than_bare_skin_in_the_heat_and_wool_is_hotter() {
        // **The reason cloth exists, as three temperatures.** Noon in hot
        // country: a body in a fleece heads for somewhere hotter than the
        // air, a bare one for the air itself, and one in a cotton shirt
        // and a hat for a few degrees under it. If cloth ever comes out
        // level with bare skin, the answer to the savanna is to undress
        // again and there was never a reason to grow any.
        use crate::body::{felt_ambient_shaded, COMFORT_HIGH};
        let noon = 42.0;
        let felt = |w: Worn| felt_ambient_shaded(noon, w.insulation, w.shade, 0.0);
        let bare = felt(Worn::default());
        let cloth = felt(full_cloth());
        let wool = felt(full_wool());
        assert!(
            cloth < bare - 3.0,
            "a full cloth set heads for {cloth:.1} against bare skin's {bare:.1}"
        );
        assert!(wool > bare, "wool heads for {wool:.1} in the heat, no hotter than bare skin");

        // ...and a thin coat in the cold rather than a refrigerator: better
        // than nothing on a cold night, and much worse than leather.
        let night = 5.0;
        let cold = |w: Worn| felt_ambient_shaded(night, w.insulation, w.shade, 0.0);
        assert!(cold(full_cloth()) >= cold(Worn::default()), "a shirt made a cold night colder");
        assert!(cold(full_cloth()) < cold(full_leather()), "cloth is as warm as leather");

        // Shade takes the edge off hot air; it never pulls a body down
        // through the comfortable band, where nothing is meant to happen.
        let just_hot = COMFORT_HIGH + 0.5;
        assert!(felt_ambient_shaded(just_hot, 0.0, full_cloth().shade, 0.0) >= COMFORT_HIGH);

        // ...and nothing but cloth casts any.
        for &(id, name) in ALL_BLOCK_IDS {
            let Some(g) = garment(id) else { continue };
            let cloth = matches!(
                id,
                BLOCK_CLOTH_CAP | BLOCK_CLOTH_TUNIC | BLOCK_CLOTH_TROUSERS | BLOCK_CLOTH_WRAPS
            );
            assert_eq!(g.shade > 0.0, cloth, "{name}: shade is cloth's and only cloth's");
        }
    }

    #[test]
    fn cloth_is_no_armour_fills_every_slot_and_weighs_less_than_wool() {
        // The other side of the trade. A shirt that turned a blow would
        // make leather pointless; a set with a hole in it could not dress
        // anybody; and a cool set that weighed more than a fleece would be
        // a set nobody carried south for the afternoon.
        let worn = full_cloth();
        assert_eq!(worn.protection, 0.0);
        assert_eq!(worn.bulk, 0.0);
        assert_eq!(worn.pieces, GARMENT_SLOTS as u8);
        assert!(
            worn.insulation < full_leather().insulation * 0.5,
            "cloth insulates {:.1}, which is most of leather's {:.1}",
            worn.insulation,
            full_leather().insulation
        );
        assert!(worn.shed_rain < full_leather().shed_rain, "cloth sheds rain like a tanned hide");
        let weight = |b| crate::blocks::definition(b).weight;
        for (cloth, wool, slot) in [
            (BLOCK_CLOTH_CAP, BLOCK_WOOL_CAP, Slot::Head),
            (BLOCK_CLOTH_TUNIC, BLOCK_WOOL_TUNIC, Slot::Chest),
            (BLOCK_CLOTH_TROUSERS, BLOCK_WOOL_LEGGINGS, Slot::Legs),
            (BLOCK_CLOTH_WRAPS, BLOCK_WOOL_BOOTS, Slot::Feet),
        ] {
            assert_eq!(slot_of(cloth), Some(slot));
            assert!(
                weight(cloth) < weight(wool),
                "{} weighs no less than {}",
                block_name(cloth),
                block_name(wool)
            );
        }
    }

    #[test]
    fn wool_is_the_warmest_thing_there_is() {
        // The reason the material exists. If a full wool set is not
        // clearly warmer than leather then there is no argument for
        // shearing a sheep, and the far north stays behind the metal
        // chain -- which is exactly the gate wool was added to open.
        let wool = full_wool().insulation;
        let leather = full_leather().insulation;
        let iron = full_iron().insulation;
        assert!(
            wool > leather * 1.3,
            "wool insulates {wool:.1} against leather's {leather:.1}, which is not a reason to bother",
        );
        assert!(iron < leather, "metal should be the worst coat, not the best");
    }

    #[test]
    fn wool_is_no_armour_at_all() {
        // The other half of the trade, and it has to be *zero* rather
        // than merely low: a jumper that turned a blow would make
        // leather pointless, because leather's whole case is that it is
        // the compromise.
        assert_eq!(full_wool().protection, 0.0);
        assert!(full_leather().protection > 0.0);
    }

    #[test]
    fn wool_is_ruined_by_rain_and_leather_is_not() {
        // What makes the choice a decision rather than an upgrade. Wet
        // clothing stops insulating (see `crate::body`), so the warmest
        // set in the game is the one that cannot be worn in the
        // weather -- and a player crossing the north in it needs a plan
        // for when the sky changes.
        let wool = full_wool().shed_rain;
        let leather = full_leather().shed_rain;
        assert!(
            wool < leather * 0.5,
            "wool sheds {wool:.2} against leather's {leather:.2}; it is supposed to soak",
        );
    }

    #[test]
    fn every_slot_can_be_filled_with_wool() {
        // Four garments and four *garment* slots -- `GARMENT_SLOTS`, not
        // `SLOTS`, since the back was added: a rucksack is worn and is
        // not clothing, so a fully dressed player is four pieces and the
        // fifth slot has nothing to do with dressing. A set with a hole
        // in it would be a material that cannot actually dress anybody,
        // and the heat shares would silently under-count.
        assert_eq!(full_wool().pieces, GARMENT_SLOTS as u8);
        for (block, slot) in [
            (BLOCK_WOOL_CAP, Slot::Head),
            (BLOCK_WOOL_TUNIC, Slot::Chest),
            (BLOCK_WOOL_LEGGINGS, Slot::Legs),
            (BLOCK_WOOL_BOOTS, Slot::Feet),
        ] {
            assert_eq!(slot_of(block), Some(slot));
        }
    }

    #[test]
    fn the_shares_are_a_whole_body() {
        // Both distributions have to sum to one, or `absorbed` and the
        // heat balance stop meaning what their documentation says: a
        // full set of one material would stop more (or less) than that
        // material's own protection figure, and nobody comparing two
        // rows would be comparing anything.
        let hits: f32 = ALL_SLOTS.iter().map(|s| s.hit_share()).sum();
        let heat: f32 = ALL_SLOTS.iter().map(|s| s.heat_share()).sum();
        assert!((hits - 1.0).abs() < 1e-5, "hit shares sum to {hits}");
        assert!((heat - 1.0).abs() < 1e-5, "heat shares sum to {heat}");
    }

    #[test]
    fn every_garment_lands_in_its_own_slot() {
        // The rule the server leans on: a request to wear something
        // names a block and nothing else, and the slot is a fact about
        // the block. Two garments claiming one slot would be fine; a
        // garment claiming none would be an item that can be equipped
        // into nowhere.
        for &(id, name) in ALL_BLOCK_IDS {
            let Some(g) = garment(id) else { continue };
            assert_eq!(
                slot_of(id),
                Some(g.slot),
                "{name} disagrees with itself about where it goes"
            );
        }
        // ...and every slot has at least one thing that fills it, or the
        // slot is decoration.
        for slot in ALL_SLOTS {
            assert!(
                ALL_BLOCK_IDS
                    .iter()
                    .any(|&(id, _)| slot_of(id) == Some(slot)),
                "nothing fits the {} slot",
                slot.name()
            );
        }
    }

    #[test]
    fn a_full_set_stops_its_own_protection_figure() {
        // The property that makes the numbers on a row readable: wear
        // four pieces of one material and the set stops exactly what
        // those rows say, weighted by nothing the reader has to work
        // out.
        let iron = set([
            Some(BLOCK_IRON_HELM),
            Some(BLOCK_IRON_CUIRASS),
            Some(BLOCK_IRON_GREAVES),
            Some(BLOCK_IRON_BOOTS),
        ]);
        let expected: f32 = ALL_SLOTS
            .iter()
            .zip([
                BLOCK_IRON_HELM,
                BLOCK_IRON_CUIRASS,
                BLOCK_IRON_GREAVES,
                BLOCK_IRON_BOOTS,
            ])
            .map(|(slot, block)| garment(block).unwrap().protection * slot.hit_share())
            .sum();
        assert!((iron.protection - expected).abs() < 1e-4);
        assert_eq!(iron.pieces, 4);
    }

    #[test]
    fn armour_never_makes_anyone_invulnerable() {
        let iron = set([
            Some(BLOCK_IRON_HELM),
            Some(BLOCK_IRON_CUIRASS),
            Some(BLOCK_IRON_GREAVES),
            Some(BLOCK_IRON_BOOTS),
        ]);
        // A punch, and a boar's charge. Both get through, and both get
        // through by less than they would have.
        for blow in [1.0f32, 4.0, 9.0, 40.0] {
            let through = iron.through(blow);
            assert!(through > 0.0, "a blow of {blow} was stopped entirely");
            assert!(through < blow, "a blow of {blow} was not reduced at all");
            assert!(
                through >= blow * MIN_DAMAGE_THROUGH - 1e-5,
                "less than the floor got through"
            );
        }
    }

    #[test]
    fn more_of_a_set_is_always_better_than_less_of_it() {
        // Not a balance claim -- a monotonicity one. Adding a piece must
        // never make a player easier to hurt, or the equipment screen is
        // a puzzle rather than a decision.
        let none = set([None, None, None, None]);
        let helm = set([Some(BLOCK_BRONZE_HELM), None, None, None]);
        let two = set([Some(BLOCK_BRONZE_HELM), Some(BLOCK_BRONZE_CUIRASS), None, None]);
        assert!(none.protection < helm.protection);
        assert!(helm.protection < two.protection);
        assert!(none.through(6.0) > helm.through(6.0));
        assert!(helm.through(6.0) > two.through(6.0));
    }

    #[test]
    fn the_best_armour_is_the_worst_coat() {
        // The trade the whole design turns on, stated as a test so a
        // later balance pass cannot quietly abolish it: iron protects
        // most and keeps the least heat in. A world where the strongest
        // armour was also the warmest clothing would have no reason for
        // leather to exist past the first hour.
        let leather = set([
            Some(BLOCK_LEATHER_CAP),
            Some(BLOCK_LEATHER_TUNIC),
            Some(BLOCK_LEATHER_LEGGINGS),
            Some(BLOCK_LEATHER_BOOTS),
        ]);
        let iron = set([
            Some(BLOCK_IRON_HELM),
            Some(BLOCK_IRON_CUIRASS),
            Some(BLOCK_IRON_GREAVES),
            Some(BLOCK_IRON_BOOTS),
        ]);
        assert!(iron.protection > leather.protection);
        assert!(leather.insulation > iron.insulation);
        assert!(leather.mobility() > iron.mobility());
        // ...and metal keeps the rain off better, which is the one thing
        // it is unambiguously good at.
        assert!(iron.shed_rain > leather.shed_rain);
    }

    #[test]
    fn nothing_can_be_slowed_to_a_standstill() {
        // Bulk is clamped, so no combination of garments -- including
        // ones a future patch adds -- can produce a player who cannot
        // move. A speed of zero is not a cost, it is a soft lock.
        let absurd = Worn {
            bulk: 10.0,
            ..Worn::default()
        };
        assert!(absurd.mobility() >= MIN_MOBILITY);
        assert!(absurd.mobility() <= 1.0);
    }

    #[test]
    fn a_worn_set_of_nothing_costs_nothing() {
        let bare = Worn::default();
        assert_eq!(bare.mobility(), 1.0);
        assert_eq!(bare.through(7.0), 7.0);
        assert_eq!(bare.pieces, 0);
    }
}
