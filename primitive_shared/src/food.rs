//! Hunger: what it costs to be alive, and what fills it back up.
//!
//! ## Why the numbers are here rather than on the server
//!
//! The server is the only side that *decides* how full a player is --
//! that is the same rule health lives by, and for the same reason. But
//! the client has to draw the bar, has to know which slot in the pack is
//! worth eating, and has to grey out the rest; and the two would have to
//! agree about what a berry is worth anyway. One table, read by both.
//!
//! ## The shape of the mechanic
//!
//! Three claims, and every constant below is one of them.
//!
//! * **Standing still is nearly free.** A player who is reading a chest
//!   or laying out a floor is not hungry, and a bar that drains while
//!   nothing happens is a bar the player watches instead of the world.
//!   The idle drain is a whole in-game day and a half from full to
//!   empty.
//! * **Work is what costs.** Swinging at a block, sprinting and jumping
//!   are billed on top, and they are most of what a real evening
//!   spends. That is what turns "I should eat" into a consequence of
//!   what you were doing rather than of how long you were logged in.
//! * **Empty is not immediately fatal.** Running out stops the healing
//!   first, and only then starts taking health -- slowly enough that a
//!   player who notices has time to do something about it, and fast
//!   enough that ignoring it ends the way ignoring it should.
//!
//! ## What it is deliberately not
//!
//! Not a second health bar with its own damage sources, not a thirst
//! meter beside it, and not a food *type* system with buffs. Each of
//! those is a bar to keep topped up rather than a reason to go
//! somewhere, and this game already has the reason: the bush is over
//! there, the boar is over there, and the fire is what makes the second
//! one worth the walk.
//!
//! ## Food goes off
//!
//! The one thing added since is that a haunch does not keep. Without
//! it a single good hunt was the end of hunger: eight cuts of boar in
//! a chest is eighty points of bar, and a player with eighty points in
//! a chest never has to go anywhere again. Rot is what makes food a
//! *supply* rather than a bank balance -- the rack and the oven are the
//! larder, and the larder is at home, which is the reason to come back.
//! See the note on [`rot_per_step`] for what keeps and what does not.

use crate::types::{
    block_kind, BlockId, BLOCK_APPLE, BLOCK_BEAR_MEAT, BLOCK_BERRIES, BLOCK_BREAD,
    BLOCK_COOKED_MEAT, BLOCK_DRIED_MEAT, BLOCK_FOWL_MEAT, BLOCK_HARE_MEAT, BLOCK_MUSHROOM,
    BLOCK_RAW_MEAT, BLOCK_RIBS, BLOCK_ROASTED_RIBS, BLOCK_ROASTED_ROOT, BLOCK_ROOT, BLOCK_ROTTEN,
    BLOCK_TOADSTOOL, BLOCK_WOLF_MEAT, VARIANT_MASK, VARIANT_SHIFT,
};

/// A full stomach, in the same units health is in.
///
/// Twenty, matching `MAX_HEALTH`, so the two bars on screen are the same
/// length and a point of one reads as the same size as a point of the
/// other.
pub const MAX_NOURISHMENT: f32 = 20.0;

/// Nourishment spent per second doing nothing at all.
///
/// A whole in-game day is ten real minutes by default, so this empties a
/// full player in about forty minutes of standing still -- which is to
/// say, never, because nobody stands still for forty minutes. That is
/// the intent: idling is not what makes you hungry.
pub const IDLE_DRAIN_PER_SECOND: f32 = 20.0 / (40.0 * 60.0);

/// Extra drain per second while sprinting.
///
/// Roughly six times the idle rate. Sprinting across a continent is the
/// one thing a player does for minutes on end that ought to cost
/// something.
pub const SPRINT_DRAIN_PER_SECOND: f32 = IDLE_DRAIN_PER_SECOND * 6.0;

/// What one swing's worth of mining costs, per second of it.
///
/// Billed by the second rather than by the block, because a block is
/// not a unit of effort: thirteen seconds of iron ore and a quarter of a
/// second of grass are the same *one block*, and charging per block
/// would make the fastest way to eat well a hillside of tall grass.
pub const MINING_DRAIN_PER_SECOND: f32 = IDLE_DRAIN_PER_SECOND * 5.0;

/// What one jump costs.
///
/// A flat charge rather than a rate: a jump is an event. Small -- two
/// hundred jumps to empty a full player -- because jumping is also how
/// you climb a hill, and a game that charges for terrain is a game that
/// tells you to walk around it.
pub const JUMP_DRAIN: f32 = 0.1;

/// Below this fraction of full, wounds stop closing.
///
/// The first thing hunger takes, and the one most players will meet
/// before they ever meet starvation: you are not dying, you are simply
/// not getting better, and the fight you were winning is now a fight you
/// are losing slowly. Seven tenths rather than a half, so it bites while
/// there is still plenty of bar left to see it happen.
pub const REGEN_THRESHOLD: f32 = 0.7;

/// **Starving is a roll every five seconds, not a rate.** Once the bar is
/// empty, every `STARVATION_ROLL_SECONDS` of it rolls `STARVATION_CHANCE`
/// and a success takes `STARVATION_DAMAGE` -- "минус 1 раз в 5 секунд с
/// шансом 1 из 5", in the player's words.
///
/// It was 0.22 health a second, steadily: a full player dead a minute and
/// a half after the bar emptied, one point every four and a half seconds,
/// which read as hunger eating the health bar while the player was still
/// walking to the bush. At one in five it is a point every twenty-five
/// seconds on average -- about eight minutes from full -- and it arrives
/// as a knock now and then rather than a drain, so an empty stomach is a
/// warning a player acts on rather than a clock they lose to.
///
/// Rejected: **a slower even rate** (0.04 a second) -- the same average
/// with nothing to notice, the bar sliding down a hair a tick; **a longer
/// period with no chance** (a point every twenty-five seconds) -- right
/// on average and a metronome, which a player times their walking to.
pub const STARVATION_ROLL_SECONDS: f32 = 5.0;
/// The chance one five-second roll of an empty stomach takes health.
pub const STARVATION_CHANCE: f32 = 0.2;
/// What a roll that lands takes.
pub const STARVATION_DAMAGE: f32 = 1.0;

/// Starvation stops here rather than killing, in *peaceful* terms --
/// which this game does not have, so it does not stop.
///
/// Stated as a constant anyway because the alternative is a number
/// somewhere in the tick loop, and because the decision is worth being
/// able to find: **hunger kills.** A floor under it would mean a player
/// who never eats is a player at one point of health forever, which is
/// not a difficulty setting, it is a mechanic that does not do anything.
pub const STARVATION_FLOOR: f32 = 0.0;

/// The three things a person has to eat some of.
///
/// **A diet, not a buff list.** The player asked for eating a little of
/// everything to matter, and the smallest honest version of that is
/// three groups and one rule: a body mends on a mixed diet and mends
/// slowly on a monotonous one. Three rather than seven, because three
/// is what the world actually offers -- something you killed, something
/// you picked, and something you grew -- and because a player has to be
/// able to hold the whole rule in their head while deciding what to
/// carry.
///
/// What it is deliberately *not* is a set of timed bonuses. Nothing
/// here makes you faster or stronger for five minutes; the only thing
/// variety touches is how fast a wound closes, which is the one number
/// a player already understands and cannot see a bar for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Anything that was an animal.
    Meat,
    /// Anything that was picked or dug: berries, roots, mushrooms,
    /// fruit.
    Plant,
    /// Anything that came off a field: grain, dough, bread.
    Grain,
}

impl Group {
    /// All three, for iterating.
    pub const ALL: [Group; 3] = [Group::Meat, Group::Plant, Group::Grain];
}

/// Which group this food belongs to, if it is food at all.
///
/// The rotten lump and the toadstool have no group: eating either is a
/// mistake, and a mistake should not count toward a balanced diet.
pub fn group(block: BlockId) -> Option<Group> {
    match block_kind(block) {
        BLOCK_RAW_MEAT
        | BLOCK_COOKED_MEAT
        | BLOCK_DRIED_MEAT
        | BLOCK_HARE_MEAT
        | BLOCK_FOWL_MEAT
        | BLOCK_BEAR_MEAT
        | BLOCK_WOLF_MEAT
        | BLOCK_RIBS
        | BLOCK_ROASTED_RIBS
        | crate::types::BLOCK_HUMAN_FLESH
        | crate::types::BLOCK_ROAST_HUMAN_FLESH => Some(Group::Meat),
        BLOCK_BERRIES | BLOCK_MUSHROOM | BLOCK_ROOT | BLOCK_ROASTED_ROOT | BLOCK_APPLE => {
            Some(Group::Plant)
        }
        crate::types::BLOCK_COCONUT => Some(Group::Plant),
        // **Honey is gathered, so it is the forager's group**: what a wood
        // gives a player who did not kill it and did not sow it. Not meat,
        // though bees made it -- the egg is meat because it is an animal's
        // protein, and honey is a flower's sugar carried home.
        crate::types::BLOCK_HONEY => Some(Group::Plant),
        crate::types::BLOCK_BREAD
        | crate::types::BLOCK_GRAIN
        | crate::types::BLOCK_DOUGH
        | crate::types::BLOCK_MILLET
        | crate::types::BLOCK_MILLET_PORRIDGE => {
            Some(Group::Grain)
        }
        // **An egg counts as meat**, which is what it is: an animal's
        // protein, taken without killing anything. It is the only thing
        // in the game that fills that group without a hunt, and that is
        // the whole reason a nest is worth climbing to -- a player who
        // has bread and berries and no kill can still eat a balanced
        // meal if they know where the nests are. See `diet_groups`.
        crate::types::BLOCK_EGG => Some(Group::Meat),
        // **Fish is meat**, for the egg's reason: an animal's protein, and
        // on a coast with nothing on four legs in reach it is the one way
        // to fill that group. Kelp is a plant for the same honest reason,
        // and it is what the far north's shore has instead of berries.
        crate::types::BLOCK_RAW_FISH | crate::types::BLOCK_COOKED_FISH => Some(Group::Meat),
        // A stew is the meat it was made to stretch; the root in it is what
        // stretched it, and one group a mouthful is the rule.
        crate::types::BLOCK_STEW => Some(Group::Meat),
        // ...and salted or dried, a haunch and a fish are what they were.
        crate::types::BLOCK_SALTED_MEAT
        | crate::types::BLOCK_SALTED_FISH
        | crate::types::BLOCK_DRIED_FISH
        | crate::types::BLOCK_DRIED_SALTED_MEAT
        | crate::types::BLOCK_DRIED_SALTED_FISH => Some(Group::Meat),
        crate::types::BLOCK_KELP_FROND | crate::types::BLOCK_DRIED_KELP => Some(Group::Plant),
        _ => None,
    }
}

/// How long a meal keeps counting toward the diet, in seconds.
///
/// Two in-game days at the default day length. Long enough that a
/// player who ate bread this morning still counts as having eaten
/// bread this evening, and short enough that a diet is something they
/// have to keep up rather than something they did once.
pub const DIET_MEMORY_SECS: f32 = 1200.0;

/// The slowest a wound closes, as a share of the full rate, on a diet
/// of one thing.
///
/// A third. Not zero: a player living on venison alone still heals,
/// slowly, which is the difference between a mechanic that rewards
/// variety and one that punishes its absence. See `diet_regen_factor`.
pub const WORST_DIET_REGEN: f32 = 1.0 / 3.0;

/// What a diet of these groups is worth as a multiplier on healing.
///
/// One group is [`WORST_DIET_REGEN`], all three is 1.0, and two is
/// halfway between -- the plainest curve that makes the second group
/// worth as much as the third, so a player who has meat goes looking
/// for *anything* else rather than for a specific missing item.
pub fn diet_regen_factor(groups: usize) -> f32 {
    match groups {
        0 | 1 => WORST_DIET_REGEN,
        2 => (WORST_DIET_REGEN + 1.0) / 2.0,
        _ => 1.0,
    }
}

/// What one of something is worth to eat, or `None` if it is not food.
///
/// The whole table, and it is short on purpose. Cooked meat is worth
/// four raw ones and that single ratio is the argument for the entire
/// fire: everything else about a campfire -- the light, the smelting,
/// the shelter from the rain -- is something a player might do without,
/// and this is the one they will not. The root says the same thing again
/// for the player who killed nothing today.
///
/// Berries, mushrooms and a raw root are deliberately poor. They are
/// what you eat *while* doing something else, and a handful of them will
/// not see you through an evening underground; a bush that filled you
/// would make the animals decoration.
#[inline]
pub fn nutrition(block: BlockId) -> Option<f32> {
    // The table is what the thing is worth *fresh*; how far it has gone
    // off takes a share back at the end. See `freshness`.
    let fresh = match block_kind(block) {
        BLOCK_BERRIES => Some(2.0),
        // **A mouthful, and deliberately not a meal.** Two eggs are a
        // handful of berries; a nest holds a couple. What a nest is
        // worth is that it is *there again next week*, not that it
        // feeds you today -- a clutch that filled the belly would make
        // the hunt optional, which is the mistake the berry bush was
        // thinned twice to fix.
        crate::types::BLOCK_EGG => Some(1.2),
        BLOCK_MUSHROOM => Some(1.5),
        BLOCK_RAW_MEAT => Some(2.5),
        BLOCK_COOKED_MEAT => Some(10.0),
        // **Between raw and cooked, nearer the cooked end.** Drying is
        // the meal you make with no fuel and no attention -- hang it and
        // walk away -- so it must beat eating the cut raw by a wide
        // margin, and it must not beat the fire: the four-to-one ratio
        // cooking pays is the argument for the whole campfire, and a
        // rack that matched it would make the fire a lamp. Twelve
        // minutes of weather against ten seconds of flame is the rack's
        // half of the bargain.
        BLOCK_DRIED_MEAT => Some(7.0),
        // **The same argument the meat makes, at the other end of the
        // day.** A raw root is a poor mouthful you can dig up on the way
        // past; roasted it is most of a meal. Deliberately a little
        // under a haunch: a root is what you eat when you did not manage
        // to kill anything, and it should feel like it.
        BLOCK_ROOT => Some(1.5),
        BLOCK_ROASTED_ROOT => Some(7.0),
        // **The best thing you can eat without a fire, and it grows on
        // one spot.** Worth more than a handful of berries because it
        // is a whole fruit and because finding the tree was the work;
        // worth less than anything cooked, so the fire is still the
        // difference between surviving and eating. An orchard is a
        // place, and a place a player walks back to is the point of it.
        BLOCK_APPLE => Some(3.0),
        // **Less than an apple as food**, because a coconut is mostly
        // water and a shell: what it is worth is `water_in`, and a nut
        // that fed as well as it quenched would make the coast the one
        // place a player never has to find anything else.
        crate::types::BLOCK_COCONUT => Some(2.0),
        // **The best raw mouthful in the world, and under anything cooked.**
        // Over the apple, because a comb is sugar and the finding cost a
        // raid and the stings (`bees::stings`); under the roasted root, so
        // the fire is still the difference between eating and a meal. Three
        // combs to a full hive is most of an evening -- worth the smoke.
        crate::types::BLOCK_HONEY => Some(5.0),
        // **The four meats that are not the same meat.** Deer, boar
        // and sheep give `BLOCK_RAW_MEAT` and always did; these are the
        // ones a hunter would actually tell apart. A hare or a bird is
        // half a meal -- small game is what you eat *while* looking for
        // something bigger. Bear is the richest mouthful in the world
        // and comes off the worst thing to meet. Wolf is stringy and
        // poor, and that is the honest number: you eat it because you
        // killed it, not because you wanted to.
        BLOCK_HARE_MEAT | BLOCK_FOWL_MEAT => Some(1.5),
        BLOCK_BEAR_MEAT => Some(4.0),
        BLOCK_WOLF_MEAT => Some(1.0),
        // **A haunch, raw or roast, and no better.** What makes it a choice
        // is the illness (`sickness_seconds`), not the number: a flesh that
        // also fed poorly would be a thing nobody ever ate, and then there is
        // no decision left in the body lying there.
        crate::types::BLOCK_HUMAN_FLESH => Some(2.5),
        crate::types::BLOCK_ROAST_HUMAN_FLESH => Some(10.0),
        // **The one meal a whole animal makes.** Ribs are bone with the
        // meat left on, roasted whole at a fire: more than a haunch of
        // cooked meat, because it is more meat, and there is exactly one
        // rack per carcass. Raw they are a poor mouthful -- gnawing a
        // raw rib is what it sounds like.
        BLOCK_RIBS => Some(2.0),
        BLOCK_ROASTED_RIBS => Some(14.0),
        // **Worse than a roast and better than everything else**, which
        // is the whole argument for farming. A hunter eats better than a
        // farmer for one evening; the farmer eats every evening, from a
        // field forty paces from the door, and never has to find
        // anything. Eight is enough that a loaf is a meal and few enough
        // that meat is still worth the walk.
        BLOCK_BREAD => Some(8.0),
        // **Porridge is under bread and over a roasted root**: a pot of grain
        // boiled whole feeds less than a loaf, and it is one step at a fire
        // where the loaf is four. And it goes off, where bread keeps
        // (`rot_per_step`) -- the quicker meal is the one eaten the same day.
        // See `types::BLOCK_WILD_MILLET` for the crop the choice is between.
        crate::types::BLOCK_MILLET_PORRIDGE => Some(6.0),
        // **Two bowls at twelve out of a haunch and a root**, which roasted
        // apart are ten and seven: a third more out of the same kill, paid
        // for in bowls, a fire, a jug of water and a day before it turns.
        // Under a rack of ribs, which is a whole animal's one best meal. See
        // `types::BLOCK_STEW` for the decision this is the number of.
        crate::types::BLOCK_STEW => Some(12.0),
        // **A fish is a hare's worth raw and most of a haunch cooked.**
        // Under the haunch, because the sea has no stamina bar in it: a
        // school is caught by swimming into it with a spear, not by a
        // chase that could end in a charge. Over the hare, because getting
        // one means being in the water with your breath running out, which
        // is its own price.
        crate::types::BLOCK_RAW_FISH => Some(2.0),
        crate::types::BLOCK_COOKED_FISH => Some(8.0),
        // **Salt keeps, it does not cook.** A salted haunch eaten as it is
        // is a little better than raw -- the cure has begun -- and nowhere
        // near the fire's: what the salt buys is days, not a meal, and a
        // salting that fed like a roast would put the campfire out of work.
        crate::types::BLOCK_SALTED_MEAT => Some(3.0),
        crate::types::BLOCK_SALTED_FISH => Some(2.5),
        // Dried, a fish is to its cooked self what dried meat is to the
        // roast: seven tenths.
        crate::types::BLOCK_DRIED_FISH => Some(6.0),
        // ...and salted first, the same meal: the salt was for the keeping.
        crate::types::BLOCK_DRIED_SALTED_MEAT => Some(7.0),
        crate::types::BLOCK_DRIED_SALTED_FISH => Some(6.0),
        // **A frond is a mouthful and a dried one is a snack that keeps.**
        // Both poor on purpose: kelp is what a shore gives to somebody who
        // brought nothing, and a kelp forest that fed a player the way a
        // hunt does would make the sea the easy answer to hunger. Dried it
        // is worth more than fresh because the rack took out water, not
        // food -- and it is under the apple, which you did not have to hold
        // your breath for.
        crate::types::BLOCK_KELP_FROND => Some(1.5),
        crate::types::BLOCK_DRIED_KELP => Some(2.5),
        _ => None,
    };
    fresh.map(|worth| worth * freshness(block))
}

// ---- going off ----
//
// A perishable carries how old it is **in the variant field of its
// block id** -- the same three bits a log spends on its axis and a
// carcass on its stage of butchering. An item-shaped block that is not
// loose, does not lie along an axis and has no front leaves that field
// empty, so it is there for the taking, and taking it means nothing
// else in the game has to learn a new type: a stack is still a block
// and a count, a chest still holds stacks, a save still writes ids.
//
// The alternative was a per-stack timestamp. That is a third field on
// `inventory::Stack` beside the count and the wear, a field the wire,
// both container save formats and the profile store would all have to
// carry, and a field that means nothing for thirty-nine slots out of
// forty. Three bits in an id that already exists cost none of that.
//
// What it does cost is stated plainly: **stacks of different ages are
// different block ids and do not merge.** Two half-stacks of meat killed
// on different days sit in two slots. That is the honest reading -- they
// *are* different, and one of them will go off first -- and `block_kind`
// strips the age everywhere it matters, so a recipe, a mouth, a rack, a
// name and a picture all see the same meat whatever day it was cut.

/// How many stages of age a perishable passes through before it is
/// rot: the width of the variant field, and the whole reason for the
/// number. Stage 0 is fresh.
pub const ROT_STAGES: u8 = 8;
/// The last stage something can be and still be food. One more step
/// from here is `BLOCK_ROTTEN`.
pub const LAST_ROT_STAGE: u8 = ROT_STAGES - 1;

/// How many times a day the world ages its food by one step.
///
/// Four, and the number is chosen backwards from the two lifetimes
/// that matter. Raw meat ages **two** stages a step and cooked meat
/// **one** (see [`rot_per_step`]), so out of eight stages:
///
/// * raw meat: 0 → 2 → 4 → 6 → rot, four steps -- **one day**;
/// * cooked meat: 0 → 1 → … → 7 → rot, eight steps -- **two days**.
///
/// A day is ten real minutes by default, so a fresh kill is a
/// two-and-a-half-minute step from its first stage and a quarter of an
/// hour from the bin: long enough to carry it home and long enough that
/// a hunt without a fire at the end of it is a hunt that fed nobody.
pub const ROT_STEPS_PER_DAY: u32 = 4;

/// How much of its fresh worth a thing on its last day still has.
///
/// Seven tenths rather than the half that first suggested itself,
/// because of the mushroom: it is worth 1.5 fresh, and `worth_eating`
/// refuses to spend food on less than a whole point of bar. At half
/// worth an old mushroom is 0.75, the gesture does nothing, and the
/// player is left with a thing that looks like food and cannot be
/// eaten -- which is a bug report, not a mechanic. At seven tenths the
/// poorest food in the game on its last day is still just over a
/// point, so everything that is food stays food until it is rot.
pub const STALE_WORTH: f32 = 0.7;

/// How many stages one step of the world's clock puts on this, or zero
/// if it keeps.
///
/// **The list is the argument.** What goes off is what came off an
/// animal or out of the ground and has been left alone: raw meat
/// fastest, because it is the thing a player most wants to hoard and
/// the thing that most obviously cannot be; cooked meat and the roasted
/// root more slowly, because a fire buys time and should be seen to.
/// Berries, a raw root and a mushroom go at the cooked rate -- they are
/// a poor meal already, and making them a poor meal with a stopwatch
/// on it would only make foraging a chore.
///
/// What does **not** go off is what a process was applied to for the
/// purpose: bread out of the oven, grain in the sack, a brick of peat.
/// Baking *is* the preservation. **Dried and salted meat and fish are
/// the exception, and a slow one**: they age a stage at a time on steps
/// far apart ([`rot_every`]), because a larder in which dried meat kept
/// for ever left the salt nothing to buy. The rack is still the larder --
/// eight days against the fire's two -- and the salt makes it a longer one.
///
/// **Honey does not go off either, and nothing was done to it.** It is the
/// one food that is its own preservation -- too much sugar for anything to
/// live in -- and that is the whole of what a hive is worth over an apple
/// tree: an apple is eaten this week, a pot of honey in the winter.
///
/// The toadstool and the rotten lump themselves never age: one is a
/// trap and the other is the end state, and neither has a next stage.
#[inline]
pub fn rot_per_step(block: BlockId) -> u8 {
    match block_kind(block) {
        // Every raw meat goes off at the meat rate, which is twice
        // everything else: a haunch is a haunch whatever it was cut
        // from, and a species that kept better than another would be a
        // rule nobody could guess.
        BLOCK_RAW_MEAT
        | BLOCK_HARE_MEAT
        | BLOCK_FOWL_MEAT
        | BLOCK_BEAR_MEAT
        | BLOCK_WOLF_MEAT
        | BLOCK_RIBS
        | crate::types::BLOCK_HUMAN_FLESH
        // Fish at the meat rate, and it has always been the thing that
        // goes off first -- a catch is eaten or smoked the day it comes out.
        | crate::types::BLOCK_RAW_FISH => 2,
        BLOCK_ROASTED_RIBS | crate::types::BLOCK_ROAST_HUMAN_FLESH => 1,
        BLOCK_COOKED_MEAT
        | BLOCK_ROASTED_ROOT
        // Wet grain keeps no better than a roast, and a stew no better
        // than the porridge it is the meat twin of.
        | crate::types::BLOCK_MILLET_PORRIDGE
        | crate::types::BLOCK_STEW
        | BLOCK_BERRIES
        | BLOCK_ROOT
        | BLOCK_MUSHROOM
        // An apple keeps as long as a berry does and no longer: it is
        // picked fruit, and the tree it came off is the only larder
        // that keeps it fresh.
        | BLOCK_APPLE
        // An egg out of the nest is on the same clock as picked fruit:
        // it keeps for a day and then it does not.
        | crate::types::BLOCK_EGG
        // Cooked fish keeps as a roast does, and a fresh frond as picked
        // fruit does. Dried kelp is not here: the rack is the preservation,
        // exactly as it is for the meat.
        | crate::types::BLOCK_COOKED_FISH
        | crate::types::BLOCK_KELP_FROND => 1,
        // **The larder, slowest last**: salted, dried, and salted then dried
        // all go off, a stage at a time, on steps far apart -- see
        // [`rot_every`], which is where they differ.
        crate::types::BLOCK_SALTED_MEAT
        | crate::types::BLOCK_SALTED_FISH
        | BLOCK_DRIED_MEAT
        | crate::types::BLOCK_DRIED_FISH
        | crate::types::BLOCK_DRIED_SALTED_MEAT
        | crate::types::BLOCK_DRIED_SALTED_FISH => 1,
        _ => 0,
    }
}

/// On how many of the world's steps this ages at all: 1 for everything that
/// goes off at [`rot_per_step`]'s pace, more for what was preserved.
///
/// **Raw < salted < dried < salted and dried**, in lifetimes of one, four,
/// eight and twenty-four days:
///
/// * raw meat: two stages a step, a day (`ROT_STEPS_PER_DAY`);
/// * salted: a stage every second step, four days -- the salt is a week's
///   walk from the coast to the hills, not a store for the winter;
/// * dried: a stage every fourth step, eight days;
/// * salted and dried: a stage every twelfth step, twenty-four days.
///
/// **Dried meat used to keep for ever**, and the player asked for a larder
/// in which the salt is worth fetching: if the rack's meat never went off,
/// salting it first would buy nothing and the salt would be a chore rather
/// than a decision. Eight days is still nearly two hours of play; nobody
/// eats through a rack's worth before then by accident.
///
/// Rejected: a fractional rate carried in the stack. There is no room in the
/// id for a remainder, and a counter per stack is the per-slot state the
/// note at the top of this section already turned down. A step number is
/// the world's clock, which every pack, chest and drop already share.
#[inline]
pub fn rot_every(block: BlockId) -> u32 {
    match block_kind(block) {
        // **The ladder of the larder**, in steps of a cooked haunch: salt buys
        // days, drying buys a season, and salting before drying buys the
        // winter. The numbers are what the rack's promise is worth --
        // `rot::the_racks_dried_meat_outlasts_the_hunt` walks a month with a
        // pack of it and it is still dried meat at the end.
        crate::types::BLOCK_SALTED_MEAT | crate::types::BLOCK_SALTED_FISH => 4,
        BLOCK_DRIED_MEAT | crate::types::BLOCK_DRIED_FISH => 24,
        crate::types::BLOCK_DRIED_SALTED_MEAT | crate::types::BLOCK_DRIED_SALTED_FISH => 72,
        _ => 1,
    }
}

/// Does this go off at all?
#[inline]
pub fn is_perishable(block: BlockId) -> bool {
    rot_per_step(block) > 0
}

/// How old this is, 0 (fresh) to [`LAST_ROT_STAGE`].
///
/// Zero for anything that does not perish, **whatever is in its
/// variant field** -- a sideways log has bits there and is not stale
/// wood. The guard is the same one `block_axis` keeps for the same
/// reason: a shared field has to be read only by the things it is
/// shared with.
#[inline]
pub fn rot_stage(block: BlockId) -> u8 {
    if !is_perishable(block) {
        return 0;
    }
    ((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8
}

/// The same food at a given age. Clamped to the last stage; anything
/// that does not perish comes back as its plain kind.
#[inline]
pub fn with_rot_stage(block: BlockId, stage: u8) -> BlockId {
    let kind = block_kind(block);
    if !is_perishable(kind) {
        return kind;
    }
    kind | ((stage.min(LAST_ROT_STAGE) as BlockId) << VARIANT_SHIFT)
}

/// The same stack one step of the clock later: a stage or two older,
/// or -- past the last stage -- `BLOCK_ROTTEN`. Anything that keeps
/// comes back unchanged, so a pass over a pack can call this on every
/// slot without asking first.
///
/// As on step zero, which is a step everything ages on: for the slow
/// larder the caller wants [`aged_on`] with the world's step number.
#[inline]
pub fn aged(block: BlockId) -> BlockId {
    aged_on(block, 0)
}

/// The same stack after the world's step number `step`: [`aged`] on the
/// steps [`rot_every`] says it ages on, and unchanged on the rest.
#[inline]
pub fn aged_on(block: BlockId, step: u64) -> BlockId {
    if !step.is_multiple_of(u64::from(rot_every(block))) {
        return block;
    }
    let step = rot_per_step(block);
    if step == 0 {
        return block;
    }
    let next = u16::from(rot_stage(block)) + u16::from(step);
    if next > u16::from(LAST_ROT_STAGE) {
        BLOCK_ROTTEN
    } else {
        with_rot_stage(block, next as u8)
    }
}

/// What share of its fresh worth this still has: 1 when fresh, falling
/// in a straight line to [`STALE_WORTH`] on the last stage. 1 for
/// anything that does not perish.
#[inline]
pub fn freshness(block: BlockId) -> f32 {
    1.0 - (1.0 - STALE_WORTH) * f32::from(rot_stage(block)) / f32::from(LAST_ROT_STAGE)
}

/// What a bad mushroom does to whoever ate it.
///
/// **Not a status effect, and that is the whole design.** There is
/// nothing to cure, nothing to wait out and no icon to watch: there is a
/// thing you should not have eaten, and the cost is paid at the moment
/// you eat it. A poison that ticked for thirty seconds would be a second
/// health bar with a timer, which is exactly what `nutrition`'s own note
/// says this game is not having.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Harm {
    /// Health taken, in the units `MAX_HEALTH` is in.
    pub health: f32,
    /// ...and how much of the stomach it turns out. A bad mushroom does
    /// not merely fail to feed you: it takes back what was in there.
    pub nourishment: f32,
}

/// What eating one of these costs, or `None` if it is only food.
///
/// One row, and it is a mushroom that is not a mushroom. Four health is
/// a fifth of a full player -- enough that it is a mistake worth not
/// making twice, and not enough to kill anybody who was healthy when
/// they made it. Whoever eats one on two hearts has other problems.
#[inline]
pub fn harm(block: BlockId) -> Option<Harm> {
    match crate::types::block_kind(block) {
        BLOCK_TOADSTOOL => Some(Harm {
            health: 4.0,
            nourishment: 4.0,
        }),
        // Food gone off: less poison than the toadstool, more upset.
        // Eaten in the way the toadstool is -- because a player will --
        // and paid for the same way. See `types::BLOCK_ROTTEN`.
        BLOCK_ROTTEN => Some(Harm {
            health: 2.0,
            nourishment: 6.0,
        }),
        _ => None,
    }
}

/// How long eating this leaves a player ill, in seconds.
///
/// **The difference between a cost and a decision.** Harm (`harm`) is
/// paid at the moment of the bite: a toadstool takes four health and
/// empties the stomach, and then it is over. That makes a bad mushroom a
/// *price*, and a price is something a player pays when the sums work
/// out. Illness is the other thing food can do -- it follows you, it
/// bleeds health while you are trying to do something else, and it
/// cannot be paid off by eating more.
///
/// The mechanism is the one stale water already uses
/// (`body::SICKNESS_PER_SECOND` and `Vitals::sick_for`), and it is
/// deliberately the same: a player who has learnt what a bad river feels
/// like knows what bad meat feels like.
///
/// **Raw flesh is the case this exists for.** Cooking used to be an
/// optimisation -- a roast fills more of the bar than the raw haunch --
/// and an optimisation is a thing you skip when you are in a hurry.
/// Three quarters of a minute of illness makes eating it raw a decision
/// with a wrong answer most of the time and a right one when you are
/// starving in the dark with no fire, which is exactly the shape a
/// survival mechanic should have.
///
/// Eggs are raw flesh of a kind and are treated as one, and so is fish.
pub fn sickness_seconds(block: BlockId) -> f32 {
    use crate::types::*;
    match crate::types::block_kind(block) {
        // The toadstool, and it is the longest: a mushroom that only
        // cost health would be a mushroom worth eating when hungry.
        BLOCK_TOADSTOOL => 180.0,
        // **Your own kind, and the longest of all, cooked or not.** Six
        // minutes of illness raw and five roast: a fire does not cook out
        // what makes this flesh dangerous, and a player who reasons "it is
        // meat, I will roast it" has to be told otherwise by the number.
        // See `types::BLOCK_HUMAN_FLESH`.
        BLOCK_HUMAN_FLESH => 360.0,
        BLOCK_ROAST_HUMAN_FLESH => 300.0,
        // Meat that has gone off. Less venom than the toadstool and more
        // of the same misery, which is what spoiled food is.
        BLOCK_ROTTEN => 120.0,
        // Raw flesh, of anything.
        BLOCK_RAW_MEAT | BLOCK_HARE_MEAT | BLOCK_WOLF_MEAT | BLOCK_BEAR_MEAT
        | BLOCK_FOWL_MEAT | BLOCK_RIBS | BLOCK_EGG | BLOCK_RAW_FISH => 45.0,
        _ => 0.0,
    }
}

/// Is this something a player can put in their mouth at all?
///
/// **Includes the toadstool**, which is the entire point of it: a thing
/// the game refuses to let you eat is a thing you cannot get wrong. It
/// sits in the pack looking like food, because it is a mushroom, and the
/// difference is on its cap.
#[inline]
pub fn is_food(block: BlockId) -> bool {
    nutrition(block).is_some() || harm(block).is_some()
}

/// What is left in the hand after eating one of these: the bowl a stew was
/// served in, or nothing.
///
/// **The bowl comes back**, and that is the half of the stew that makes it
/// a household's meal: a player with four bowls can have four stews on the
/// go and no more, and the fifth haunch is roasted. A stew that ate its
/// bowl would make the bowl a cost rather than a thing owned.
#[inline]
pub fn served_in(block: BlockId) -> Option<BlockId> {
    match crate::types::block_kind(block) {
        crate::types::BLOCK_STEW => Some(crate::types::BLOCK_BOWL),
        _ => None,
    }
}

/// What eating one of these does to a stomach that currently holds
/// `have`, in the same units.
///
/// Capped, and the overflow is simply lost -- eating a haunch of meat on
/// a full stomach wastes most of it, which is what stops a player from
/// carrying their nourishment as a stack of food and topping up to
/// exactly full before every swing.
#[inline]
pub fn after_eating(have: f32, block: BlockId) -> f32 {
    after_eating_made(have, block, crate::quality::Quality::PLAIN)
}

/// The same mouthful, from something somebody cooked well or badly.
///
/// **Quality scales what is good and never what is bad.** A toadstool
/// prepared beautifully is a beautifully prepared poison, and a fine loaf
/// that also un-poisoned you would turn the one item in the game that is a
/// trap into a reward for cooking. So `harm` goes through untouched and
/// only [`nutrition`] is scaled.
///
/// The multiplier is narrow (`quality::nutrition_scale`) because food is
/// eaten in quantity: a wide one would make a meal a sum to do rather than
/// a thing to eat, and the real reward for cooking something well is how
/// long it keeps (`quality::keeping_chance`).
pub fn after_eating_made(have: f32, block: BlockId, quality: crate::quality::Quality) -> f32 {
    if let Some(harm) = harm(block) {
        return (have - harm.nourishment).max(0.0);
    }
    match nutrition(block) {
        Some(value) => {
            (have + value * crate::quality::nutrition_scale(quality)).min(MAX_NOURISHMENT)
        }
        None => have,
    }
}

/// How much clean water eating one of this gives, on `body::MAX_HYDRATION`'s
/// scale, or `None` for everything that is only food.
///
/// **The coconut, and the reason it exists.** A coast in the dry belt has
/// the sea and nothing else, and the sea makes a player thirstier
/// (`body::Water::hydration`). A palm on the beach is the well: a nut's
/// water is clean -- no sickness, unlike a pond -- and three hundred and fifty
/// is most of a mouthful from a river (`body::DRINK_HYDRATION`). What it
/// costs is the walk to the palm, the climb for the nuts, and the weight of
/// carrying them inland (`blocks`' row for the coconut).
///
/// Rejected: a coconut that is a jug of water, drunk with the jug's gesture.
/// It is eaten -- it is food as well -- and a second gesture for one item
/// would be a second rule for "is this worth spending" to forget.
pub fn water_in(block: BlockId) -> Option<f32> {
    match block_kind(block) {
        crate::types::BLOCK_COCONUT => Some(COCONUT_WATER),
        _ => None,
    }
}

/// The clean water in one coconut. See `water_in`.
pub const COCONUT_WATER: f32 = 350.0;

/// Whether eating this would do anything at all.
///
/// The check the server makes before spending an item: eating on a full
/// stomach must not consume the food. "My meat vanished and nothing
/// happened" is the same class of bug as a craft that eats its
/// ingredients and produces nothing.
///
/// A whole point of the bar rather than any change at all, because a
/// player 0.05 short of full who spends a cooked haunch on it has been
/// robbed by a rounding error.
#[inline]
pub fn worth_eating(have: f32, block: BlockId) -> bool {
    // A toadstool is always "worth" eating in the sense this asks about:
    // the gesture has to be allowed to happen, or the mistake is one the
    // game quietly refuses to let anybody make. It is the one thing here
    // that is worth eating and not worth having eaten.
    if harm(block).is_some() {
        return true;
    }
    match nutrition(block) {
        Some(_) => after_eating(have, block) - have >= 1.0,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_COBBLESTONE, BLOCK_STONE_PICKAXE};

    #[test]
    fn only_food_is_food() {
        assert!(is_food(BLOCK_BERRIES));
        assert!(is_food(BLOCK_COOKED_MEAT));
        assert!(!is_food(BLOCK_COBBLESTONE));
        assert!(!is_food(BLOCK_STONE_PICKAXE));
        assert_eq!(nutrition(BLOCK_COBBLESTONE), None);
    }

    #[test]
    fn a_root_is_worth_digging_up_and_worth_roasting() {
        // The same shape the meat has: poor as it comes out of the
        // ground, most of a meal after a fire. If this ever stops
        // holding, the root becomes a berry with extra steps.
        let raw = nutrition(crate::types::BLOCK_ROOT).expect("a root is food");
        let roasted = nutrition(BLOCK_ROASTED_ROOT).expect("a roasted root is food");
        assert!(roasted >= raw * 4.0, "roasting {raw} into {roasted} is not worth a fire");
        assert!(
            roasted < nutrition(BLOCK_COOKED_MEAT).expect("meat"),
            "a root should not beat a haunch"
        );
    }

    #[test]
    fn a_toadstool_can_be_eaten_and_should_not_be() {
        // Every half of the trap. It has to *look* edible -- the pack
        // must offer it, the gesture must go through -- and it has to
        // cost something real when it does.
        assert!(is_food(BLOCK_TOADSTOOL), "the game would refuse to let anybody eat it");
        assert!(worth_eating(MAX_NOURISHMENT, BLOCK_TOADSTOOL), "even a full player can");
        assert_eq!(nutrition(BLOCK_TOADSTOOL), None, "it fed somebody");

        let cost = harm(BLOCK_TOADSTOOL).expect("a toadstool harms");
        assert!(cost.health > 0.0 && cost.health < 20.0, "it should hurt, not kill outright");
        // It empties rather than fills, and cannot take a stomach below
        // empty.
        assert!(after_eating(MAX_NOURISHMENT, BLOCK_TOADSTOOL) < MAX_NOURISHMENT);
        assert_eq!(after_eating(0.0, BLOCK_TOADSTOOL), 0.0);

        // ...and the mushroom beside it is still plain food.
        assert!(harm(BLOCK_MUSHROOM).is_none(), "the edible one poisons people");
    }

    #[test]
    fn drying_beats_raw_and_never_beats_the_fire() {
        // The rack's place at the table: well above eating the cut raw
        // -- or nobody would wait twelve minutes -- and below the
        // cooked one, or the campfire becomes a lamp.
        let raw = nutrition(BLOCK_RAW_MEAT).expect("meat is food");
        let dried = nutrition(BLOCK_DRIED_MEAT).expect("dried meat is food");
        let cooked = nutrition(BLOCK_COOKED_MEAT).expect("cooked meat is food");
        assert!(dried >= raw * 2.0, "drying {raw} into {dried} is not worth the wait");
        assert!(dried < cooked, "the rack beat the fire");
    }

    #[test]
    fn the_fire_is_worth_four_animals() {
        // The one ratio the whole feature turns on. If cooking ever
        // stops being worth several times the raw meat, the campfire
        // becomes a lamp and hunger becomes a bar rather than a reason
        // to build anything.
        let raw = nutrition(BLOCK_RAW_MEAT).expect("meat is food");
        let cooked = nutrition(BLOCK_COOKED_MEAT).expect("cooked meat is food");
        assert!(cooked >= raw * 3.0, "cooking {raw} into {cooked} is not worth a fire");
    }

    #[test]
    fn foraging_keeps_you_walking_and_will_not_feed_an_evening() {
        // Berries and mushrooms are on purpose a poor meal: a bush that
        // filled you would make the animals scenery.
        let bush = nutrition(BLOCK_BERRIES).expect("berries are food");
        assert!(
            bush * 4.0 < MAX_NOURISHMENT,
            "four handfuls of berries should not be a full stomach"
        );
    }

    #[test]
    fn eating_stops_at_full_and_the_rest_is_wasted() {
        assert_eq!(after_eating(MAX_NOURISHMENT, BLOCK_COOKED_MEAT), MAX_NOURISHMENT);
        assert_eq!(after_eating(MAX_NOURISHMENT - 1.0, BLOCK_BERRIES), MAX_NOURISHMENT);
        assert_eq!(after_eating(0.0, BLOCK_BERRIES), 2.0);
    }

    #[test]
    fn a_full_player_does_not_spend_food_on_nothing() {
        // The rule that stops "my meat vanished and nothing happened",
        // which is the same bug as a craft that eats its ingredients.
        assert!(!worth_eating(MAX_NOURISHMENT, BLOCK_COOKED_MEAT));
        assert!(!worth_eating(MAX_NOURISHMENT - 0.4, BLOCK_BERRIES));
        assert!(worth_eating(MAX_NOURISHMENT - 4.0, BLOCK_BERRIES));
        assert!(!worth_eating(0.0, BLOCK_COBBLESTONE), "stone is not a meal");
    }

    #[test]
    fn idling_is_nearly_free_and_working_is_not() {
        // The claim the whole set of rates exists to make: what makes
        // you hungry is what you did, not how long you were logged in.
        const { assert!(MINING_DRAIN_PER_SECOND > IDLE_DRAIN_PER_SECOND * 3.0) };
        const { assert!(SPRINT_DRAIN_PER_SECOND > IDLE_DRAIN_PER_SECOND * 3.0) };
        // ...and idling alone takes the better part of an hour, which is
        // longer than most sessions and far longer than any one job.
        const {
            assert!(
                MAX_NOURISHMENT / IDLE_DRAIN_PER_SECOND > 30.0 * 60.0,
                "standing still empties the bar too fast to ignore"
            )
        };
    }

    #[test]
    fn starving_gives_you_time_to_do_something_about_it() {
        // On average: long enough to walk to the next valley for food, short
        // enough that a night without any is an ending.
        let seconds = 20.0 / STARVATION_DAMAGE / STARVATION_CHANCE * STARVATION_ROLL_SECONDS;
        assert!(seconds > 5.0 * 60.0, "starvation kills a full player in {seconds}s on average");
        assert!(seconds < 15.0 * 60.0, "starvation takes {seconds}s on average, which is not a threat");
    }

    // ---- going off ----

    /// Everything that ages, for the loops below. Kept as a list here
    /// rather than read off `rot_per_step`, so that a row quietly
    /// dropped from that table fails a test instead of passing one.
    const PERISHABLES: [BlockId; 6] = [
        BLOCK_RAW_MEAT,
        BLOCK_COOKED_MEAT,
        BLOCK_ROASTED_ROOT,
        BLOCK_BERRIES,
        BLOCK_ROOT,
        BLOCK_MUSHROOM,
    ];

    /// How many steps of the clock from fresh to the bin.
    fn steps_until_rot(block: BlockId) -> u32 {
        let mut stack = block;
        // Far enough for the slowest thing in the larder: salted and then
        // dried ages on one step in seventy-two (`rot_every`).
        for step in 1..=4096 {
            stack = aged_on(stack, u64::from(step));
            if block_kind(stack) == BLOCK_ROTTEN {
                return step;
            }
        }
        panic!("{} never went off", crate::types::block_name(block));
    }

    #[test]
    fn raw_meat_goes_off_in_a_day_and_bread_never_does() {
        // The two lifetimes `ROT_STEPS_PER_DAY` is derived from, and the
        // claim the rack's whole existence rests on.
        assert_eq!(steps_until_rot(BLOCK_RAW_MEAT), ROT_STEPS_PER_DAY, "raw meat is not a day");
        assert_eq!(
            steps_until_rot(BLOCK_COOKED_MEAT),
            ROT_STEPS_PER_DAY * 2,
            "cooked meat is not two days"
        );
        // ...and the fire buys real time: cooked outlasts raw.
        assert!(steps_until_rot(BLOCK_COOKED_MEAT) > steps_until_rot(BLOCK_RAW_MEAT));

        // What a process was applied to keeps for ever. Dried meat left
        // this loop for `the_larder_keeps_longer_the_more_was_done_to_it`.
        for keeps in [
            BLOCK_BREAD,
            crate::types::BLOCK_GRAIN,
            crate::types::BLOCK_DRIED_PEAT,
            BLOCK_TOADSTOOL,
            BLOCK_ROTTEN,
        ] {
            let mut stack = keeps;
            for _ in 0..ROT_STEPS_PER_DAY * 30 {
                stack = aged(stack);
            }
            assert_eq!(stack, keeps, "{} went off", crate::types::block_name(keeps));
            assert!(!is_perishable(keeps));
            assert_eq!(rot_stage(keeps), 0);
        }
    }

    #[test]
    fn the_larder_keeps_longer_the_more_was_done_to_it() {
        use crate::types::{
            BLOCK_DRIED_FISH, BLOCK_DRIED_SALTED_FISH, BLOCK_DRIED_SALTED_MEAT, BLOCK_RAW_FISH, BLOCK_SALTED_FISH,
            BLOCK_SALTED_MEAT,
        };
        // Raw < salted < dried < salted and dried, for a haunch and a fish
        // alike -- the order the salt and the rack are worth fetching in.
        for [raw, salted, dried, both] in [
            [BLOCK_RAW_MEAT, BLOCK_SALTED_MEAT, BLOCK_DRIED_MEAT, BLOCK_DRIED_SALTED_MEAT],
            [BLOCK_RAW_FISH, BLOCK_SALTED_FISH, BLOCK_DRIED_FISH, BLOCK_DRIED_SALTED_FISH],
        ] {
            let lives = [raw, salted, dried, both].map(steps_until_rot);
            assert!(lives.windows(2).all(|w| w[0] < w[1]), "{}: lifetimes out of order {lives:?}", crate::types::block_name(raw));
            // ...and the salt is worth much more than a cooked meal's day.
            assert!(steps_until_rot(salted) > steps_until_rot(BLOCK_COOKED_MEAT), "salting bought less than the fire");
        }
    }

    #[test]
    fn a_slow_food_ages_only_on_its_own_steps() {
        use crate::types::BLOCK_DRIED_SALTED_MEAT;
        // Between its steps a stack is left exactly as it was: the slow
        // larder is slow because most passes skip it, not because a stage
        // is split.
        assert_eq!(aged_on(BLOCK_DRIED_SALTED_MEAT, 5), BLOCK_DRIED_SALTED_MEAT);
        assert_eq!(aged_on(BLOCK_DRIED_SALTED_MEAT, 71), BLOCK_DRIED_SALTED_MEAT);
        assert_eq!(rot_stage(aged_on(BLOCK_DRIED_SALTED_MEAT, 72)), 1);
        // ...and raw meat ages on every one.
        assert_eq!(rot_stage(aged_on(BLOCK_RAW_MEAT, 5)), 2);
    }

    #[test]
    fn honey_feeds_better_than_berries_and_does_not_go_off_as_meat_does() {
        use crate::types::BLOCK_HONEY;
        let honey = nutrition(BLOCK_HONEY).expect("honey is food");
        assert!(honey > nutrition(BLOCK_BERRIES).unwrap(), "honey is no better than berries");
        assert!(honey > nutrition(BLOCK_APPLE).unwrap(), "honey is no better than an apple");
        // ...and not better than the fire: the raw foods stay under a cooked meal.
        assert!(honey < nutrition(BLOCK_COOKED_MEAT).unwrap(), "honey beats a cooked haunch");
        assert!(steps_until_rot(BLOCK_RAW_MEAT) <= ROT_STEPS_PER_DAY, "raw meat keeps");
        let mut pot = BLOCK_HONEY;
        for _ in 0..ROT_STEPS_PER_DAY * 30 {
            pot = aged(pot);
        }
        assert_eq!(pot, BLOCK_HONEY, "honey went off in a month");
        assert!(!is_perishable(BLOCK_HONEY));
        assert_eq!(group(BLOCK_HONEY), Some(Group::Plant));
    }

    #[test]
    fn food_is_worth_less_the_older_it_is() {
        for food in PERISHABLES {
            let fresh = nutrition(food).expect("a perishable is food");
            let mut last = fresh;
            for stage in 0..=LAST_ROT_STAGE {
                let stale = with_rot_stage(food, stage);
                assert_eq!(rot_stage(stale), stage);
                let worth = nutrition(stale).expect("still food");
                assert!(
                    worth <= last,
                    "{} got better with age at stage {stage}",
                    crate::types::block_name(food)
                );
                // ...and it never stops being worth the gesture. A thing
                // the pack offers as food and the mouth refuses is a bug
                // report -- see `STALE_WORTH`.
                assert!(
                    worth_eating(0.0, stale),
                    "{} at stage {stage} is worth {worth}, which the mouth refuses",
                    crate::types::block_name(food)
                );
                last = worth;
            }
            let oldest = nutrition(with_rot_stage(food, LAST_ROT_STAGE)).expect("food");
            assert!(oldest < fresh, "age took nothing off");
            assert!(
                oldest >= fresh * STALE_WORTH - 1e-5,
                "age took more than it says it does"
            );
        }
        // Rot itself is not worth anything; it costs.
        assert_eq!(nutrition(BLOCK_ROTTEN), None);
        assert!(harm(BLOCK_ROTTEN).is_some());
    }

    #[test]
    fn an_aged_stack_is_still_the_same_food_to_a_recipe_and_a_mouth() {
        use crate::inventory::{Inventory, Stack};

        let old_meat = with_rot_stage(BLOCK_RAW_MEAT, 5);
        assert_ne!(old_meat, BLOCK_RAW_MEAT, "the stage has to be in the id");
        assert_eq!(block_kind(old_meat), BLOCK_RAW_MEAT);

        // The mouth.
        assert!(is_food(old_meat));
        assert!(nutrition(old_meat).is_some());
        assert!(harm(old_meat).is_none(), "old meat is not poison, rot is");

        // The recipe table: the cooking row has to see meat in a pack
        // that only has old meat in it. This is what `Inventory::count`
        // matching by kind is for -- before it did, a haunch two hours
        // old could not be cooked, which is exactly the moment a player
        // most wants to cook it.
        let cooking = crate::crafting::RECIPES
            .iter()
            .find(|r| r.output.0 == BLOCK_COOKED_MEAT)
            .expect("there is a cooking recipe");
        let mut pack = Inventory::new();
        pack.add(old_meat, 3);
        assert!(crate::crafting::has_ingredients(&pack, cooking), "old meat is not meat to the fire");
        assert_eq!(pack.count(BLOCK_RAW_MEAT), 3);

        // The hearth itself, which is where cooking actually happens:
        // loaded with old meat, it starts the same batch and puts
        // *fresh* cooked meat in the tray.
        let mut hearth = Inventory::new();
        hearth.put_in_slot(crate::hearth::INPUT_SLOTS.start, Stack::new(old_meat, 2));
        let (_, recipe) = crate::hearth::next_recipe(crate::hearth::Kind::Campfire, &hearth)
            .expect("a campfire full of old meat cooks it");
        assert_eq!(recipe.output.0, BLOCK_COOKED_MEAT);
        assert!(crate::hearth::complete(&mut hearth, recipe));
        assert_eq!(hearth.count(BLOCK_COOKED_MEAT), 1);
        assert_eq!(
            hearth.count_within(crate::hearth::OUTPUT_SLOTS, BLOCK_COOKED_MEAT),
            1,
            "the cooked meat did not reach the tray"
        );
        assert_eq!(
            rot_stage(hearth.block_in(crate::hearth::OUTPUT_SLOTS.start).expect("cooked")),
            0,
            "cooking did not reset the clock"
        );
        assert_eq!(hearth.count(BLOCK_RAW_MEAT), 1, "it took the wrong amount of meat");

        // ...and the rack: old meat still dries into meat that keeps.
        assert_eq!(crate::rack::cures_into(old_meat), Some(BLOCK_DRIED_MEAT));

        // The name and the picture are the kind's, whatever the age.
        assert_eq!(crate::types::block_name(old_meat), crate::types::block_name(BLOCK_RAW_MEAT));
        assert_eq!(crate::types::stack_limit(old_meat), crate::types::stack_limit(BLOCK_RAW_MEAT));
    }

    #[test]
    fn every_perishable_stage_is_a_known_block() {
        // The anti-cheat refuses an id `is_known_block` calls invented,
        // and the save relies on the same answer -- so an aged stack in
        // a pack must be a block the game admits to having written. See
        // `types::may_carry_variant`.
        for food in PERISHABLES {
            for stage in 0..=LAST_ROT_STAGE {
                let stale = with_rot_stage(food, stage);
                assert!(
                    crate::types::is_known_block(stale),
                    "{} at stage {stage} is not a known block",
                    crate::types::block_name(food)
                );
                // Age changes nothing about whether it may be put down.
                // Meat may not be, at any age; a mushroom may be, at any
                // age -- and an old mushroom stuck back in the ground
                // and picked again is a fresh one, which is the one
                // place the clock can be turned back. Left that way on
                // purpose: it is a living thing being replanted, it is
                // worth a point and a half at most, and refusing to
                // plant an old mushroom would be a rule nobody could
                // guess.
                assert_eq!(
                    crate::types::is_placeable(stale),
                    crate::types::is_placeable(food),
                    "age changed whether {} can be placed",
                    crate::types::block_name(food)
                );
            }
        }
        // ...and the bits mean nothing on what keeps: a stage on a loaf
        // is junk off a socket, and is refused like any other junk.
        assert!(!crate::types::is_known_block(BLOCK_BREAD | (3 << VARIANT_SHIFT)));
        // (Dried meat is not here any more: it goes off too, a stage at a
        // time and slowly, since the larder learned salting -- see `rot_every`.)
        assert_eq!(with_rot_stage(BLOCK_BREAD, 4), BLOCK_BREAD, "bread was given an age");
    }

    #[test]
    fn rot_stacks_as_high_as_the_food_it_came_from() {
        // A full stack that goes off is rewritten in its slot at the
        // same count. If rot stacked lower than meat, the tail of the
        // stack would have nowhere to go and would silently vanish.
        for food in PERISHABLES {
            assert!(
                crate::types::stack_limit(BLOCK_ROTTEN) >= crate::types::stack_limit(food),
                "a stack of {} would not fit in a stack of rot",
                crate::types::block_name(food)
            );
        }
    }
}
