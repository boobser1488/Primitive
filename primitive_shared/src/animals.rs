//! The animals: what they are, and what is true about them on both
//! sides of the socket.
//!
//! ## What is here
//!
//! One row per species -- size, speed, health, what it drops, and how it
//! answers being hit. The server runs the behaviour and the client draws
//! the result, and both of them need the same numbers: the client sizes
//! the box it draws from `height`, and the server collides the same box
//! against the world. A client drawing a boar a half-block taller than
//! the one the server is simulating is a boar you cannot hit.
//!
//! ## What is not here
//!
//! The behaviour itself. Wandering, fleeing, charging and dying live on
//! the server (`primitive_server::logic::animals`), because they are
//! decisions about the world and the client is never allowed to make
//! one. What crosses the wire is a position, a facing and a species --
//! the same contract a dropped item has.
//!
//! ## Four, and why these four
//!
//! A world needs at least one animal that runs, one that fights, and one
//! that is barely worth chasing, or "hunting" is a single verb with a
//! single answer. The hare is the one you meet first and the one you
//! will mostly fail to catch; the deer is the meal; the boar is the
//! reason to think about whether you want the meal.
//!
//! The wolf is the fourth sentence, and it is a different one: it is the
//! only animal here that is hunting *you*. Everything else in this world
//! answers a player's decision -- to chase, to swing, to walk away --
//! and a world in which nothing ever opens the exchange is a world with
//! no reason to be anywhere in particular after dark. What stops it from
//! being a punishment is that a wolf alone is a coward: it takes a pack
//! to come at you, so the danger is something a player can *count*
//! before it arrives. See `Species::provoke_range` and the server's
//! `think_hunter`.
//!
//! ## The savanna's three
//!
//! The seven above are one country's animals, and a savanna that grew
//! acacias and cotton and was grazed by deer was a meadow with the colour
//! turned down. So the hot grassland has animals of its own, and each is
//! one of the old sentences said differently rather than a new verb:
//!
//! * the **zebra** is the deer's place in the food chain -- the hide and
//!   the meat -- on open ground with nowhere to hide, so where a deer
//!   makes for a wood a zebra simply outlasts what is behind it
//!   (`stamina_seconds`);
//! * the **antelope** is the hare's trick at a bigger size: the widest
//!   eyes on the grass, a sprint rather than a marathon, and a swerve as
//!   it runs (`zigzags`);
//! * the **lion** is the wolf's hunt without the pack: it comes at you
//!   alone (`needs_company`), it is shy of fire after dark, and it is
//!   slower than a sprint, so meeting one is the choice meeting a boar is
//!   -- back away, or have a spear in your hand.
//!
//! **Where each lives is a biome and not a guess from the ground** -- see
//! `lives_in` for why the savanna could not be read off the blocks the
//! way a wood is.

use serde::{Deserialize, Serialize};

use crate::types::{
    BlockId, BLOCK_BEAR_HIDE, BLOCK_BEAR_MEAT, BLOCK_BONE, BLOCK_CARCASS_ANTELOPE,
    BLOCK_CARCASS_BEAR, BLOCK_CARCASS_BOAR, BLOCK_CARCASS_DEER, BLOCK_CARCASS_FOWL,
    BLOCK_CARCASS_HARE, BLOCK_CARCASS_LION, BLOCK_CARCASS_SHEEP, BLOCK_CARCASS_WOLF,
    BLOCK_CARCASS_ZEBRA, BLOCK_FAT, BLOCK_FEATHER, BLOCK_FOWL_MEAT,
    BLOCK_HARE_MEAT, BLOCK_HIDE, BLOCK_PELT, BLOCK_RAW_MEAT, BLOCK_RIBS, BLOCK_SINEW,
    BLOCK_WOLF_MEAT, BLOCK_WOOL,
};

/// How much of an animal there is, over what the table says.
///
/// **Five, and the player asked for it in those words**: "сделай
/// животным в 5 раз больше здоровья". One constant rather than thirteen
/// rewritten rows, because what the rows argue is the *order* -- a bear
/// is three boars, a gull is a fowl, a fish is two punches -- and that
/// argument is the part worth keeping. See `Species::base_health`.
///
/// **What it does to the hunt, stated so nobody has to rediscover it.**
/// Everything that measures health measures it as a fraction
/// (`logic::animals::BREAKS_OFF_BELOW`, `WOLF_SHADOWS_BELOW`), so
/// fleeing, breaking off and a wolf's shadowing are unchanged: an animal
/// still turns and runs at a third. What did change is the number of
/// blows, and it changed by exactly five:
///
/// * a flint spear takes a deer in **seven** thrusts where it took two,
///   a boar in **ten** where it took two, a bear in **thirty-four**
///   where it took seven;
/// * a flint knife takes a hare in **thirteen** strokes;
/// * a bare fist still cannot open a bear at all (`hurt_by`), and now
///   takes a hare thirteen punches instead of three.
///
/// A flint spear survives seventy thrusts (`blocks`, its `durability`),
/// so one spear is two boars or half a bear where it used to be a
/// hunting season. **This is the edge of the design rule in CLAUDE.md**
/// -- a mechanic should create a decision -- because at thirty-four
/// thrusts the answer to a bear is always "leave", which is one correct
/// answer rather than a choice. The knob that buys the decision back is
/// not this one: it is `primitive_server::hunting_damage` and
/// `hide_armour`, which decide what a blow is worth. Raising the health
/// alone made a bear thirty-four spear thrusts, so the blow was raised
/// with it -- once, at the point it lands, see [`WEAPON_BITE`]. See
/// `what_five_times_the_health_costs_in_blows`, which states the
/// arithmetic as a test so the day somebody moves either number the
/// build says what it did to the hunt.
pub const TOUGHNESS: f32 = 5.0;

/// What a landed blow is worth against an animal, as a multiple of what
/// the tool does to a block.
///
/// **The other half of "сделай животным в 5 раз больше здоровья".** Five
/// times the health with the same blows is five times the hits, and the
/// bill came to **thirty-four spear thrusts into a bear** out of the
/// seventy a flint spear survives: half a spear for one animal. At that
/// price the answer to a bear is always "walk away", and an animal with
/// one right answer is not the choice this game is built out of
/// (CLAUDE.md).
///
/// Three ways to buy the fight back were weighed:
///
/// * *Lower [`TOUGHNESS`].* That is the thing the player asked for, and
///   asking for tougher animals is not asking for the old fight back.
/// * *Raise the tools.* `primitive_server::hunting_damage` reads the tool
///   from the block table, so the same numbers are what a pick does to
///   stone; a spear that bites an animal harder would also dig faster.
/// * **One multiplier at the point the blow lands (chosen).** It sits
///   after the hide is subtracted, so every sentence the hide makes
///   survives it -- a fist still cannot get through a bear, and the boar's
///   shield is still what a spear is for -- and every ratio between
///   species is untouched. The hunt now costs exactly **twice** what it
///   did before the health went up, rather than five times: a bear is
///   fourteen thrusts, a boar four, a deer three.
///
/// Two and a half, and not two, because a bear at seventeen thrusts is
/// still a quarter of a spear.
pub const WEAPON_BITE: f32 = 2.5;

/// What kind of animal something is.
///
/// An enum on the wire rather than a free id, for the reason
/// `EntityKind` is one: a client cannot be asked to draw a species it
/// has never heard of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Species {
    /// Small, fast, and worth almost nothing. Bolts at the sight of you.
    Hare,
    /// The meal. Flees, but not fast enough to be impossible.
    Deer,
    /// Fights back. The only thing in the world that will hurt a player
    /// who has not fallen off something.
    Boar,
    /// Hunts. Bold in a pack and shy alone, and out mostly after dark.
    Wolf,
    /// The one animal worth more alive than dead, and the reason the
    /// far north is reachable before metal is.
    ///
    /// Slow, unafraid and useless in a fight, which is a deliberate
    /// break from the other four: every one of them is a chase or a
    /// threat, and this is neither. What it carries is wool -- see
    /// `drops` and `equipment::garment` -- and what wool is for is not
    /// being cold.
    Sheep,
    /// **The thing you run from.** A bear is the first animal in this
    /// world that a player cannot win against by standing still and
    /// swinging: it carries three times a boar's health behind a hide
    /// no fist gets through (`hide_armour`), and it hits harder than
    /// anything else alive.
    ///
    /// Rare, and rare on purpose -- one in a wood rather than one in a
    /// clearing. What it is *for* is the same thing the cliff is for:
    /// a reason to look where you are going, and a reward at the end of
    /// it worth the walk home (the biggest hide and the best meat).
    Bear,
    /// A bird -- a grouse, near enough. Walks, pecks, and goes up when
    /// it is frightened.
    ///
    /// **The one animal here that leaves the ground**, and the only one
    /// with somewhere it is trying to be. Everything else in this world
    /// is where it happens to have wandered to; a bird keeps a nest
    /// (`BLOCK_NEST`, put in the canopies by `worldgen::place_nests`),
    /// forages within sight of it, flies back to it when it is done and
    /// sits there -- see the server's `logic::animals::homing`, which is
    /// where that state machine lives.
    ///
    /// This note used to say "nothing in this world flies" and the flight
    /// is still not a second physics: it is a target altitude the body is
    /// pulled toward while the bird has a reason to be up
    /// (`FLIGHT_HEIGHT`), on the same collider, the same turn rate and
    /// the same states as everything that walks.
    ///
    /// What it gives is feathers and a light meal, what the nest gives is
    /// eggs, and what it costs is catching something small and quick with
    /// a spear.
    Fowl,
    /// **The savanna's deer: the hide and the meat.**
    ///
    /// Appended, as the two after it are, rather than written in beside
    /// the deer -- because the order of this enum is on the wire and in
    /// every save. Bincode sends a species as its index, a mod names one by
    /// its place in `ALL`, and a skeleton keeps it in its variant bits
    /// (`types::bones_of`): a zebra written between the deer and the boar
    /// would turn every boar in every old world into a zebra.
    ///
    /// A herd on open ground. It has nothing to hide behind, so it does not
    /// make for cover the way a deer does; it outlasts. Faster than a
    /// sprinting player, with more wind than the lion that hunts it, and
    /// harmless to anybody who leaves it be.
    Zebra,
    /// Small, quick and jumpy: the first thing on the savanna to see you
    /// and the first to go. Its wind is short -- it is built for a sprint,
    /// not a chase -- and it swerves as it runs, the hare's trick at a
    /// larger size (`zigzags`). What it gives is a pelt and a modest meal;
    /// what it costs is getting near one at all.
    Antelope,
    /// **The savanna's hunter.** Alone or in a pair, out mostly after dark,
    /// and after the zebra and the antelope rather than after you -- until
    /// you come too close or strike it.
    ///
    /// The wolf's opposite in the one way that matters: a wolf alone is a
    /// coward and a lion alone is not (`needs_company`). What stops that
    /// from being a punishment is the rule every hostile animal but the
    /// bear keeps -- it is slower than a sprinting player -- so a lion
    /// watching you from fifteen blocks is a decision and not a death.
    Lion,
    /// **A small fish, in a school.** The first animal that lives in the
    /// water rather than beside it, and the reason the sea is somewhere to
    /// go rather than something to swim across.
    ///
    /// Appended for the zebra's reason. What it is, as a sentence among the
    /// others: the hare's joke under water -- faster than you, worth a
    /// mouthful -- with the difference that you are holding your breath
    /// while you try. A school scatters when one of it sees you (the herd's
    /// alarm, `logic::animals::survey`), turns back together, and is caught
    /// by a spear or by a player who waits in the weed until it swims past.
    ///
    /// **It never leaves the water**, and that is a rule of the body rather
    /// than of the mind: the server's `swim` refuses any step that would put
    /// the fish's box outside liquid. A fish that ends up on land anyway --
    /// the pond drained under it -- gasps and dies there.
    Fish,
    /// **A cod: the big fish of the open sea.** Alone, down where the shelf
    /// falls away, slower than the school and three times the meal. Deep
    /// water is what makes it a decision: eight blocks of sea over it is
    /// eight blocks of breath to spend on the way down and the way back.
    Cod,
    /// **The sea's bird.** Over the coast and the water, circling high; down
    /// on the sand and the rocks to walk about; up and away the moment
    /// somebody comes near. Appended for the zebra's reason.
    ///
    /// The fowl said again on the shore, with the one difference that
    /// decides how it is hunted: a gull in the air is out of any reach this
    /// world has -- nine blocks up over the surf (the server's
    /// `SOAR_HEIGHT`) -- so it is taken where it lands, by somebody who kept
    /// a dune or a rock between them, or after dark, when a flock roosts on
    /// the beach and watches with half its eyes. A flock going up all at
    /// once with nobody near it is something coming along the shore.
    ///
    /// What it gives is two feathers and a mouthful, where it falls: see
    /// `carcass` for why a gull leaves no body.
    Gull,
    /// **The cold water's fish**, and the reason a river is somewhere to
    /// walk to rather than something to cross. Appended for the zebra's
    /// reason, as the two after it are.
    ///
    /// A trout keeps to running and cold water -- the river itself, and the
    /// lakes of the birch wood, the taiga, the tundra and the hills -- so
    /// what comes out of the water tells a player which country they are
    /// standing in. It holds station in a current, which is why it is the
    /// quickest thing that swims here, and it is two mouthfuls rather than
    /// one.
    ///
    /// **Why a named fish at all**, when `fishing` argued the opposite for a
    /// whole release ("Species with names... every one is an id that differs
    /// from the raw fish only in a number"): that argument was about the
    /// *rod*, where a species is a label on a catch, and it still holds --
    /// nothing here has a raw-trout item, and the salt barrel and the
    /// drying rack know one fish. What it got wrong is the water. A school
    /// that is the same school in a mountain stream and a mangrove makes
    /// every body of water the same place, and a player who has learned
    /// that the deep sea holds a cod has learned the only thing there was
    /// to know about water. Three more shapes, each living somewhere
    /// particular, is a map of the water drawn in the animals -- for three
    /// pictures and no new items.
    Trout,
    /// **The still water's hunter, and the biggest fish in fresh water.**
    ///
    /// Warm standing water: a meadow lake, a forest pond, the swamp. Alone,
    /// always -- a pike does not share a reed bed, and `group_size` says so
    /// -- and slower off the mark than anything else that swims, because a
    /// fish that lies in the weed and waits has no use for wind
    /// (`stamina_seconds`). What it has instead is bulk: three fish of
    /// meat, the best single catch outside the deep sea, in water a player
    /// can stand up in.
    ///
    /// The decision it creates is the counterpart of the cod's. A cod is
    /// eight blocks of breath down a shelf in the open sea; a pike is a
    /// lake you can wade into, guarded by nothing but the fact that there
    /// is one of it and it saw you first (`awareness`).
    Pike,
    /// **The shoal of the shallow sea**: many, small, and near the top of
    /// the water.
    ///
    /// The coast's own fish, where the cod is the coast's *deep* fish. A
    /// shoal is four to six of them a spear's length under the surface, so
    /// the sea at the foot of the dunes feeds somebody who has no rod, no
    /// trap and no patience -- which is what makes a beach a place to camp
    /// on the first evening rather than scenery to walk along.
    ///
    /// It is also what the gulls are over. Nothing in the code connects the
    /// two -- a gull's dive is its own (`DIVE_CHANCE` in the server's
    /// `seabird`) and it catches nothing -- but a flock working the water
    /// where the shoals are is the reason a player looks there, and that is
    /// the whole of what a bird circling ever meant.
    Herring,
    /// **The first animal here that comes to the player rather than the
    /// other way round.**
    ///
    /// Everything else in this world is somewhere, and hunting it means
    /// going there. A rat is *in your house*, at night, because you live
    /// there -- see `haunt`, which is the map of where that is -- and the
    /// decision it creates is about the house rather than about the rat:
    /// a lamp in the storeroom, a wall with no gap under it, or a chest
    /// you keep having to refill.
    ///
    /// It is deliberately feeble. Two hits with anything, no hide at all
    /// (`hide_armour`), and it will not fight back -- a rat that could
    /// hurt a player would be a boar in a corridor, and what this is for
    /// is not a fight. What it costs you is the larder: it eats out of
    /// chests, it chews the skin off a drying rack, and it takes the
    /// winter out of dried meat (`vermin::gnaw`). Losing a week of
    /// smoking to something you could have killed with a stick is a
    /// worse feeling than losing a fight, and it is the feeling this is
    /// for.
    ///
    /// What keeps it out is light and walls, and nothing else: there are
    /// no cats in this world and there is not going to be a rat trap
    /// recipe, because both of those are a thing you *build once* and
    /// then stop thinking about. A torch burns out (`inventory::
    /// TorchBurn`), so a lit storeroom is a place somebody keeps lit.
    ///
    /// Appended, for `Species::Zebra`'s reason -- the index is on the
    /// wire -- and it is the seventeenth, which is why
    /// `types::BLOCK_BONES_3` exists.
    Rat,
    /// **The wild horse of the plains, and the first animal here that is
    /// worth more under you than beside you.**
    ///
    /// A herd on open grass with a stallion that keeps it (the server's
    /// `stallion`), faster than a sprinting player and with a long wind, so a
    /// wild one is never run down: it is crept up on, fed, fed again, and
    /// then sat on until it stops throwing you (`husbandry::Breaking`). What
    /// that buys is the only thing in the world that carries a trip's ore
    /// home and covers a day's walk in an hour (`crate::horse`) -- and what it
    /// costs is a mouth to feed, a roof in the rain, and an animal a pack of
    /// wolves will pull down where it stands tied.
    ///
    /// Appended, for `Species::Zebra`'s reason: the index is on the wire and
    /// in every skeleton (`types::bones_of`) -- the eighteenth, which the
    /// third bones block still has room for.
    Horse,
    /// **The troop in the palms, and the first animal in this world that
    /// takes something off a player rather than giving something up.**
    ///
    /// A rat eats out of a chest at night, in a house, while nobody is
    /// looking (`Species::Rat`). A monkey does it in daylight, in front of
    /// you, and then sits six blocks up a trunk with it. That is the same
    /// larder problem asked where the player can answer it, and the answer
    /// is a decision rather than a chore: stay with the camp and lose the
    /// afternoon, build the stores where a monkey cannot reach, or feed the
    /// troop the fruit it came for and get on.
    ///
    /// **It climbs rather than flies** (`climbs`). The bird's altitude is a
    /// place in the air; a monkey's is a place on a tree, so it goes up what
    /// is there and comes down when there is a reason -- fruit on the
    /// ground, or the rest of the troop gone somewhere. A stone thrown at
    /// one scatters the lot of them, chattering, and **the chatter is the
    /// mechanic** as much as the theft is: a troop that goes off in the
    /// canopy behind you is something in the wood that is not you, which is
    /// the only warning a player gets in a country with no wolves in it.
    ///
    /// Appended, for `Species::Zebra`'s reason -- the index is on the wire
    /// and in the bones -- the nineteenth, which the fourth bones block has
    /// room for (`types::SKELETON_ROOM`).
    Monkey,
    /// **The beach at night.** Sideways, quick over the sand, and under a
    /// stone before you have got to it.
    ///
    /// It is the smallest decision in the game and it is a real one: a crab
    /// is a meal you can have on the first night with nothing but your
    /// hands and a fire, and the cost of reaching for one is that it takes a
    /// claw to your fingers (`damage`) -- the only animal here that hurts a
    /// player without being a fight. A boar is a choice to fight or leave; a
    /// crab is a choice to be quick or to be bitten, at a hundredth of the
    /// stake, which is the shape a coast's first evening should have.
    ///
    /// **Out in numbers after dark** (`spawn_weight_in`), so a beach walked
    /// at noon and the same beach walked at midnight are two places.
    Crab,
}

/// How much commoner a wolf (by day) and a bear are in a wood than
/// elsewhere. See `Species::spawn_weight_in`.
pub const WOODS_WOLF_BY_DAY: u32 = 4;
pub const WOODS_BEAR: u32 = 2;

impl Species {
    /// Every species, in the order the world spawns them in -- which is
    /// also roughly the order a player meets them.
    pub const ALL: &'static [Species] = &[
        Species::Hare,
        Species::Deer,
        Species::Boar,
        Species::Wolf,
        Species::Sheep,
        Species::Bear,
        Species::Fowl,
        // Appended, never inserted: see `Species::Zebra`.
        Species::Zebra,
        Species::Antelope,
        Species::Lion,
        // ...and the two that swim, appended for the same reason.
        Species::Fish,
        Species::Cod,
        // ...and the shore's bird, after them.
        Species::Gull,
        // ...and the three that fill in the rest of the water: the river's,
        // the lake's and the shallow sea's. Appended, never inserted, for
        // `Species::Zebra`'s reason -- the index is on the wire.
        //
        // **This list is now full at sixteen**, and the ceiling is the
        // skeleton: a species' place here is written into three variant bits
        // of one of two bone blocks (`types::bones_of`), which is eight
        // apiece. The seventeenth animal needs a third bones block and a row
        // in `species_in_bones` *before* it is appended here --
        // `every_species_comes_back_out_of_its_own_skeleton` is a `const`
        // assert, so the build stops rather than a new animal's bones
        // reading back as a hare's.
        Species::Trout,
        Species::Pike,
        Species::Herring,
        // **Seventeen now, and the ceiling moved rather than bent.** The
        // note above said the seventeenth animal needs a third bones
        // block and a row in `species_in_bones` *first*; it got both
        // (`types::BLOCK_BONES_3`). The rule it states is unaltered:
        // eight species to a bones block, and a new block before a new
        // animal.
        //
        // **What changed since is where the room is kept.** The blocks are
        // one table now (`types::SKELETON_BLOCKS`), and a fourth
        // (`types::BLOCK_BONES_4`) was put there before anybody needed it,
        // so the ceiling is thirty-two (`types::SKELETON_ROOM`) and the
        // const assert in `every_species_comes_back_out_of_its_own_skeleton`
        // reads it rather than a number written beside it. The next animal
        // is one line here; the thirty-third is one entry in that table.
        // A young animal is *not* a species (`crate::youth`) and costs no
        // room at all -- it dies into its parent's carcass and bones.
        Species::Rat,
        // ...and the plains' horse, the eighteenth: one line, as promised.
        Species::Horse,
        // ...and the shore's two, the nineteenth and the twentieth. One line
        // apiece, still, and twelve of the fourth bones block's room left.
        Species::Monkey,
        Species::Crab,
    ];

    /// What to call it. Not translated, for the same reason block names
    /// are not: this is an identifier as well as a label -- it goes in
    /// the death message and in `/stats`.
    pub fn name(self) -> &'static str {
        match self {
            Species::Hare => "hare",
            Species::Deer => "deer",
            Species::Boar => "boar",
            Species::Wolf => "wolf",
            Species::Sheep => "sheep",
            Species::Bear => "bear",
            Species::Fowl => "fowl",
            Species::Zebra => "zebra",
            Species::Antelope => "antelope",
            Species::Lion => "lion",
            Species::Fish => "fish",
            Species::Cod => "cod",
            Species::Gull => "gull",
            Species::Trout => "trout",
            Species::Pike => "pike",
            Species::Herring => "herring",
            Species::Rat => "rat",
            Species::Horse => "horse",
            Species::Monkey => "monkey",
            Species::Crab => "crab",
        }
    }

    /// The server's words for a player this animal killed, as the death
    /// screen and the chat print them: "was gored by a boar".
    ///
    /// **Here rather than in the server's tick loop**, where the match used to
    /// live, because the client reads it too: the server's cause is English,
    /// and the client finds the species that wrote it (`of_death_cause`) and
    /// says it again in the player's language (`ui::names::death_cause`). Two
    /// copies of these strings -- one to send and one to recognise -- would
    /// drift the first time somebody reworded a death, and a death that no
    /// longer matched would quietly go back to English.
    ///
    /// **Every species by name, and no `_` arm**, for the reason the old match
    /// gave: a new hunter is a compile error here rather than an animal that
    /// kills people under another's name.
    pub fn death_cause(self) -> &'static str {
        match self {
            Species::Boar => "was gored by a boar",
            Species::Wolf => "was pulled down by a wolf",
            Species::Bear => "was mauled by a bear",
            Species::Lion => "was brought down by a lion",
            // Nothing that runs lands a blow (`Species::damage` is zero), so
            // these never name anybody's death; they are spelled out for the
            // compile error above.
            Species::Hare
            | Species::Deer
            | Species::Sheep
            | Species::Fowl
            | Species::Zebra
            | Species::Antelope
            | Species::Fish
            | Species::Cod
            | Species::Trout
            | Species::Pike
            | Species::Herring
            | Species::Gull
            | Species::Rat
            // A horse never comes at anybody; what it does to a person on
            // its back is the server's (`BREAKING_THROW`), and that death is
            // a fall rather than an animal's.
            | Species::Horse
            // A monkey takes and runs. It has no blow at all (`damage`), and
            // the one thing it could be said to cost a player is a meal.
            | Species::Monkey => "was killed by an animal",
            // **The smallest death in the game, and it needs a sentence of
            // its own** -- not because anybody will read it often, but
            // because `of_death_cause` finds a species by its words, and a
            // crab sharing the catch-all would make every animal death in
            // the world a crab's.
            Species::Crab => "was nipped by a crab",
        }
    }

    /// ...and the species that wrote a cause, if an animal did and the cause
    /// names one. The catch-all "killed by an animal" names nobody.
    pub fn of_death_cause(cause: &str) -> Option<Species> {
        Species::ALL
            .iter()
            .copied()
            .filter(|species| species.damage() > 0.0)
            .find(|species| species.death_cause() == cause)
    }

    /// How tall it stands, in blocks. The collider and the drawn box are
    /// both this, which is what keeps what you see and what you can hit
    /// the same object.
    pub fn height(self) -> f32 {
        match self {
            Species::Hare => 0.5,
            Species::Deer => 1.5,
            Species::Boar => 1.0,
            // Shoulder-high on a crouching person, which is what a
            // wolf is: taller than a boar in the leg and lighter
            // everywhere else.
            Species::Wolf => 0.85,
            // Between a boar and a deer, and squarer than either. A
            // sheep is a barrel on short legs.
            Species::Sheep => 0.95,
            // Shoulder-high on a standing person: the tallest
            // thing that walks here, and the silhouette is most
            // of the warning a player gets.
            Species::Bear => 1.3,
            // Ankle-high. A bird you have to aim *down* at is a
            // bird that is hard to hit, which is the whole of
            // what makes catching one worth anything.
            // Half a metre with its head up, which is what a grouse
            // standing is -- and it is the height the model is built
            // to (`animal_model::FOWL`).
            Species::Fowl => 0.5,
            // A striped horse: a little under the deer at the shoulder and
            // still the tallest thing on the grass.
            Species::Zebra => 1.35,
            // Small enough that the long grass nearly hides it, big enough
            // to be a meal.
            Species::Antelope => 0.95,
            // Low and long. A lion's back is under a deer's, and the heavy
            // head carried level with its shoulders is most of the warning.
            Species::Lion => 1.1,
            // A fish's "height" is its depth, fin to belly: the collider
            // a swimming body is kept inside the water with, and the box
            // the model is built to (`animal_model::FISH`).
            Species::Fish => 0.3,
            Species::Cod => 0.65,
            // A trout between the two, a pike the deepest-bodied thing in
            // fresh water, a herring the smallest thing that swims.
            Species::Trout => 0.36,
            Species::Pike => 0.45,
            Species::Herring => 0.26,
            // A hand high. Small enough that what the eye catches is a
            // shape moving along the foot of a wall rather than an animal,
            // which is what a rat looks like and the whole reason it is
            // frightening in a storeroom and not in a field.
            Species::Rat => 0.22,
            // A little under the fowl: a gull stands on short legs with its
            // weight low, and its length is in the wings folded behind it.
            Species::Gull => 0.45,
            // **Withers at a player's shoulder, head carried above it.** A
            // horse is the one animal a person looks *up* at, and a rider sits
            // with their eye over two blocks up -- which is half of what a
            // horse is for on a plain. Taller than the zebra, its wild cousin
            // on the savanna, by a hand and a half.
            Species::Horse => 1.6,
            // Knee-high on all fours, which is what a monkey on the ground
            // is -- it is upright only when it is sitting in a tree with
            // both hands full of somebody's dinner.
            Species::Monkey => 0.72,
            // Ankle-high, and most of that is shell -- the last third is the
            // eyes on their stalks, which is the part of a crab a person
            // actually sees over the sand. See `width`: a crab is the one
            // animal here that is wider than it is long.
            Species::Crab => 0.34,
        }
    }

    /// How wide, in blocks. Under one for all three, so an animal fits
    /// through the gaps the terrain leaves and never has to be pushed
    /// out of a wall it was standing in.
    pub fn width(self) -> f32 {
        match self {
            Species::Hare => 0.45,
            Species::Deer => 0.85,
            Species::Boar => 0.9,
            Species::Wolf => 0.6,
            // The broadest of the five for its length, which is what
            // a fleece does to an outline.
            Species::Sheep => 0.8,
            // Just under a block, like everything else here: an animal
            // as wide as the gap it walks through is an animal that
            // gets stuck in a doorway it can see through.
            Species::Bear => 0.95,
            Species::Fowl => 0.3,
            Species::Zebra => 0.75,
            Species::Antelope => 0.5,
            Species::Lion => 0.7,
            Species::Fish => 0.25,
            Species::Cod => 0.5,
            Species::Trout => 0.3,
            Species::Pike => 0.36,
            Species::Herring => 0.2,
            Species::Rat => 0.16,
            // Standing, wings folded: the collider is the bird on the sand.
            // Spread, it is three times that, and nothing up there collides
            // with a wingtip -- see the server's `walk`.
            Species::Gull => 0.35,
            // Under a block, like everything else that walks: a horse that
            // could not be led through a one-block gate could not be stabled.
            Species::Horse => 0.8,
            Species::Monkey => 0.35,
            // **Wider than it is long, and that is the animal.** Everything
            // else in this world is a body with a head at one end; a crab is
            // a shell with the legs down the sides, and it goes the way it
            // is wide (`sidles`).
            Species::Crab => 0.58,
        }
    }

    /// How long, nose to tail, in blocks.
    ///
    /// **A boar is not a cube.** Width is what it takes up crossways and
    /// this is what it takes up along its own length, and for every one
    /// of these animals the two differ by a factor of two -- which is the
    /// whole reason a hit test needs the animal's own frame rather than
    /// a sphere.
    pub fn length(self) -> f32 {
        match self {
            Species::Hare => 0.7,
            Species::Deer => 1.7,
            Species::Boar => 1.5,
            Species::Wolf => 1.3,
            Species::Sheep => 1.2,
            // Measured off the drawn model, like the hit box below. The
            // rebuilt bear carries its muzzle out of its face and its head
            // in front of a neck, so its nose is 1.15 blocks ahead of its
            // middle where the old flat face was 1.0. What it gave up is
            // tail: a box five sixteenths long, which a bear has not got.
            Species::Bear => 2.3,
            Species::Fowl => 0.45,
            Species::Zebra => 1.9,
            Species::Antelope => 1.2,
            Species::Lion => 1.9,
            Species::Fish => 0.6,
            Species::Cod => 1.4,
            Species::Trout => 0.85,
            // Long and narrow: the shape is the animal, and a pike drawn as
            // a fat cod would be a cod.
            Species::Pike => 1.5,
            Species::Herring => 0.5,
            // Body and tail. Longer than it is wide by a good deal, which
            // is what `every_animal_is_longer_than_it_is_wide` is about and
            // what makes the thing read as a rat from behind.
            Species::Rat => 0.42,
            // Bill to crossed wingtips.
            Species::Gull => 0.8,
            // Nose to tail, with the head carried forward: the longest thing
            // that walks here, which is also why a horse turns wide.
            Species::Horse => 2.3,
            // Body and head. The tail is longer than both and is not counted
            // here for the rat's reason -- `length` is the body the animal
            // is walked as, and the hit box is what the tail is in.
            Species::Monkey => 0.7,
            Species::Crab => 0.35,
        }
    }

    /// The box a blow has to land in, as half-extents in the animal's
    /// own frame: across, up, and along.
    ///
    /// **One box, read by three places**, and that is the point of it
    /// being here rather than in either half of the game. The client
    /// aims with it, so what you can hit is what you can see; the server
    /// validates a blow with it, so a swing that visibly connects is not
    /// refused; and a test in the client checks the drawn model fits
    /// inside it, so the two cannot drift apart.
    ///
    /// It used to be a *sphere* on the server, of the animal's own
    /// height -- so the far half of a deer was outside the box the
    /// client had aimed at, and hitting one in the flank was a swing
    /// that hit on screen and missed on the wire.
    pub fn half_extents(self) -> (f32, f32, f32) {
        // Measured off the drawn model rather than derived from the
        // collider: the collider is a square footprint an animal is
        // *walked* around with, and the box a blow lands in has to be
        // the shape on screen. The client has a test that compares these
        // numbers with the model it builds, so a boar that grows a
        // longer snout fails that test rather than growing a blind spot.
        //
        // (across, up, along), in blocks, from the animal's own middle.
        match self {
            Species::Hare => (0.13, 0.38, 0.44),
            Species::Deer => (0.31, 0.97, 1.00),
            Species::Boar => (0.41, 0.53, 1.06),
            Species::Wolf => (0.22, 0.50, 1.09),
            // Half an inch taller than it was, and the fleece is why.
            // The sheep's legs used to show less of themselves than
            // they were wide -- a bump rather than a limb -- and what
            // decides how much shows is where the wool ends, not how
            // long the leg is. Lifting the fleece by an inch and a half
            // made the model's furthest reach the body instead of the
            // hoof, and this box is measured as the furthest reach.
            //
            // The cost is a block's sixteenth and a half of box below
            // the hooves, because the reach is taken as a half-extent
            // about the animal's centre. A sheep is slightly easier to hit
            // from underneath than it was; nothing else changes, and
            // `the_shared_hit_box_is_the_model_that_is_drawn` is what
            // makes the two numbers stay one number.
            Species::Sheep => (0.38, 0.53, 0.81),
            // Half of `width`, `height` and `length` -- the box
            // is the model, and the model is the animal. A bear
            // is the biggest box in the world and the easiest
            // thing to hit; what makes it dangerous is that
            // hitting it is not the problem.
            Species::Bear => (0.48, 0.65, 1.15),
            Species::Fowl => (0.15, 0.25, 0.23),
            // Measured off `animal_model::{ZEBRA, ANTELOPE, LION}` like the
            // rest. The zebra's reach up is its ears on a raised head; the
            // lion's across is the mane, which is wider than the body it
            // grows on -- and a mane is what a spear aimed at a lion's
            // head lands in.
            Species::Zebra => (0.28, 0.95, 1.14),
            Species::Antelope => (0.17, 0.74, 0.75),
            // ...and the lion's reach along it is its muzzle now, not its
            // tail: the tail used to stand straight out behind like a broom
            // handle and now hangs off the rump, so the longest thing about
            // a lion is the end that bites.
            Species::Lion => (0.31, 0.55, 0.97),
            // Measured off `animal_model::{FISH, COD}`: across is the
            // pectoral fins, up is the dorsal fin, along is the mouth.
            Species::Fish => (0.15, 0.175, 0.375),
            Species::Cod => (0.28, 0.375, 0.81),
            // Measured off the models, as every row here is: see
            // `animal_model::the_shared_hit_box_is_the_model_that_is_drawn`.
            Species::Trout => (0.1725, 0.21, 0.5063),
            Species::Pike => (0.2, 0.2313, 0.8625),
            Species::Herring => (0.12, 0.1575, 0.3113),
            // Low and long: the tail is most of the "along", which is
            // why the hit box is longer than the body and has to be --
            // a tail you can see and cannot hit is the bug
            // `the_shared_hit_box_is_the_model_that_is_drawn` exists for.
            Species::Rat => (0.094, 0.10, 0.31),
            // Measured off `animal_model::GULL` standing -- the folded wing,
            // not the spread one: a blow lands on a bird on the ground.
            Species::Gull => (0.175, 0.225, 0.41),
            // Measured off `animals/horse.bbmodel` like the rest: up is the
            // ears, a block and nine tenths off the ground; along is the muzzle
            // held out in front, which is further from the middle than the
            // tail is because the middle is where the saddle is, behind the
            // withers (`model_notes`, the horse).
            Species::Horse => (0.3, 1.09, 1.44),
            // Measured off `animals/monkey.bbmodel`: across is the elbows,
            // up is the crown of the head, and **along is the tail**, which
            // is half again the body and reaches further behind the middle
            // than the muzzle does in front of it. A tail you can see and
            // cannot hit is what this test exists for.
            Species::Monkey => (0.1688, 0.3563, 0.55),
            // Measured off `animals/crab.bbmodel`: across is the claws held
            // out, which is the whole silhouette of a crab and therefore the
            // whole of what a hand comes down on.
            Species::Crab => (0.2875, 0.1688, 0.1688),
        }
    }

    /// Blocks per second at a walk.
    ///
    /// **Every one of these went up by about a fifth** when the animals
    /// were made rare (see `MAX_ANIMALS_PER_PLAYER`), and the two changes
    /// are one change. A meadow with three animals in it instead of six
    /// is a meadow where an animal is a *thing that happened*, and a
    /// thing that happened has to look like it is going somewhere: at the
    /// old pace a lone deer ambling across an empty field read as scenery
    /// that had come loose. Still under a walking player throughout --
    /// nothing here paces you while it is merely wandering.
    pub fn walk_speed(self) -> f32 {
        match self {
            Species::Hare => 2.0,
            Species::Deer => 2.3,
            Species::Boar => 1.7,
            Species::Wolf => 2.6,
            // The slowest walk of the five. A sheep is not going
            // anywhere and has never been in a hurry.
            Species::Sheep => 1.3,
            // Ambling. A bear walking is a bear that has not
            // decided about you yet, and it is slower than you.
            Species::Bear => 1.6,
            // Quick little steps, always busy.
            Species::Fowl => 1.7,
            Species::Zebra => 2.2,
            Species::Antelope => 2.4,
            // Unhurried. A lion walking toward you has not decided either,
            // and it is still slower than you are.
            Species::Lion => 2.3,
            // A cruise. A school drifting along a reef is going nowhere,
            // and a swimmer overtakes it -- which is what lets a player
            // get close enough to be seen at all.
            Species::Fish => 1.6,
            Species::Cod => 1.2,
            Species::Trout => 1.8,
            // A pike at rest is a stick lying in the weed.
            Species::Pike => 0.9,
            Species::Herring => 1.7,
            // It does not stroll. A rat crossing open floor is already
            // running; what it does the rest of the time is stop dead,
            // which is the server's business and not a speed.
            Species::Rat => 1.6,
            // A slow waddle along the tideline. What a gull does on foot is
            // look about, not go anywhere.
            Species::Gull => 1.2,
            // A long stride at no effort: still under a walking player, so a
            // grazing herd is something a person can walk up to -- which is
            // where taming one starts.
            Species::Horse => 2.4,
            // Busy, like the fowl's: a troop on the ground is foraging, and
            // foraging is short runs between things worth turning over.
            Species::Monkey => 1.9,
            // A crab on the sand is not going anywhere in particular, and it
            // is the slowest walk in the world.
            Species::Crab => 0.9,
        }
    }

    /// Blocks per second when it has a reason to hurry.
    ///
    /// **Nothing worth eating can be run down in a straight line**, and
    /// that is the whole of hunting here. The deer used to be the
    /// exception -- under a sprint, so the answer to a deer was to hold
    /// the sprint key until it tired -- and a chase with one input in it
    /// is not a hunt. It is over a sprint now, which means a deer is
    /// caught the way the hare already had to be: by cutting it off,
    /// using the ground, or reaching it with something thrown far enough
    /// (see `combat::SPEAR_REACH`, which is the other half of this
    /// change). The hare is still the fastest thing in the meadow and
    /// still bolts in bursts and stops, which is what makes it catchable
    /// at all (the server's `logic::animals` is what makes it stop).
    ///
    /// The boar is under a sprint, and that is not an oversight: a
    /// hostile animal a player cannot walk away from is not a danger, it
    /// is a punishment. What makes a boar dangerous is that you meet it
    /// at seven blocks, not that it is faster than you. The bear is the
    /// one exception, and it is the exception on purpose.
    ///
    /// **The whole column came down when the sprint did.** It had been
    /// tuned against `NOMINAL_SPRINT_SPEED = 8.8`, a number that stopped
    /// being true when the base walk went to 4.3 -- so an animal written
    /// as "just under a sprint" was in fact comfortably over one, and
    /// the world had quietly become a place where nothing could be
    /// walked away from. Every figure here is now a ratio to 6.45, which
    /// is what a sprinting player really does, and the ratios are the
    /// ones the paragraphs above describe.
    ///
    /// The comparisons the tests hold, and therefore the mechanics: the
    /// hare and the deer are over a sprint, everything hostile except
    /// the bear is under it, the sheep is under a *walk*, and the bear
    /// is over it. The wolf lost its old advantage over the deer with
    /// the rest of the column -- see `stamina_seconds`, where that is
    /// argued rather than merely recorded.
    pub fn run_speed(self) -> f32 {
        match self {
            Species::Hare => 8.4,
            Species::Deer => 7.2,
            // Just under a sprint, like everything else that means
            // you harm: a wolf you cannot outrun is a wolf that has
            // already killed you, and the decision to run is the one
            // the animal exists to force. What makes it dangerous is
            // that there are three of them and the stamina bar is
            // finite.
            Species::Boar => 6.1,
            Species::Wolf => 6.3,
            // **Slower than a walking player**, and that is the whole
            // design of the animal. Every other creature here is a
            // chase or a threat; this one is a resource that happens to
            // be able to move. A player who wants wool gets wool, and
            // the interesting decision is not the catching -- it is
            // what they give up to carry a coat's worth home.
            // Still under a *walking* player, which is the number
            // `a_sheep_cannot_outrun_a_walking_player` holds still: a
            // player who has to sprint at a sheep is a player who can be
            // outrun by one while exhausted.
            Species::Sheep => 3.6,
            // **Faster than a sprinting player**, which is the one number
            // that makes a bear what it is: you cannot simply turn round
            // and leave. What you can do is break its line of sight, get
            // water or a ledge between you, or not be there in the first
            // place.
            //
            // It was 6.0, and 6.0 is *slower than a sprint* (8.8 -- see
            // `NOMINAL_SPRINT_SPEED`). The note above it claimed the
            // opposite and cited "a sprint is 5.5", which is the
            // player's **walk**: so for as long as it stood, the animal
            // this file describes as the thing you run from was a thing
            // you outran by holding shift, and every other line about the
            // bear -- the thirty-second grudge, the twenty-second wind,
            // the hide no fist gets through -- was paying for a promise
            // the speed did not keep. Nine and a fifth is that promise:
            // ahead of a sprint by enough to close, behind the hare, so
            // the fastest thing in the world is still the one that is
            // worth nothing to catch.
            Species::Bear => 7.0,
            // Bolts, and in a straight line for a few metres
            // only. A bird outruns a walk and not a sprint.
            Species::Fowl => 5.2,
            // Over a sprint and under the deer. A zebra on open ground is
            // not escaping into a wood, it is simply leaving, and for that
            // it need not be faster than a deer -- only faster than what is
            // behind it for longer (see `stamina_seconds`).
            Species::Zebra => 7.0,
            // The fastest thing on the savanna and still under the hare,
            // which stays the fastest thing in the world: the joke the hare
            // exists for is not the antelope's to take.
            Species::Antelope => 8.0,
            // **Under a sprint, like every hostile animal but the bear.** A
            // lion nobody could walk away from would be a bear in a warmer
            // coat. It is over a walk, so turning your back on one at a
            // stroll is a mistake; holding the sprint key is an answer, and
            // it costs the stamina bar.
            Species::Lion => 6.2,
            // **Over a walk, and far over a swim.** A player in the water
            // moves at a fraction of their walk, so a fish that has seen
            // you is gone -- and it is gone in a burst (`stamina_seconds`)
            // and then circles back to its school, which is when a spear
            // gets it. Not over a sprint: nobody sprints under water, and
            // a number chosen against a sprint would be a number chosen
            // against nothing.
            Species::Fish => 5.0,
            // **All of them under a sprint**, which is the rule every
            // swimmer keeps: a fish faster than the player is a fish nobody
            // catches with a spear, and the spear is the first way there is.
            // The trout is the quickest -- it lives in a current -- and the
            // pike is the slowest away, because its whole trade is the
            // first two blocks and it has none after them.
            Species::Trout => 5.4,
            Species::Pike => 4.4,
            Species::Herring => 5.0,
            // **Slower than a walking player**, and that is the mechanic.
            // A rat cannot outrun you across a room, so a rat you have seen
            // is a rat you can kill -- the problem was never catching one,
            // it is that they come when you are asleep and there is no
            // light. Anything faster would turn a nuisance into a chase
            // around the furniture, which is a chore.
            Species::Rat => 3.6,
            // Heavier and slower off the mark, and still more than a
            // swimmer: a cod is reached by coming down on it, not after it.
            Species::Cod => 4.2,
            // **Over a sprint, and it does not matter**: a flushed gull is
            // nine blocks up before its speed means anything. The number
            // that is a hunt is `awareness`, not this.
            Species::Gull => 7.4,
            // **Well over a sprint, and the whole of what a horse is.** A wild
            // herd that has seen you is gone, and no player on foot follows it:
            // a horse is caught by patience (`husbandry::Breaking`), never by a
            // chase. Over the zebra and under the antelope's sprint, because a
            // horse's gift is not the burst but the distance -- see
            // `stamina_seconds`, and `horse::GALLOP`, which is the same animal
            // with somebody on it and carrying them.
            Species::Horse => 7.8,
            // **Under a sprinting player** (`NOMINAL_SPRINT_SPEED`), and
            // that is deliberate: a monkey that outran you on the flat would
            // be a thief with no answer. It is caught on open sand and it is
            // never caught in the palms, because what saves it is the trunk
            // and not its legs (`climbs`).
            Species::Monkey => 5.3,
            // **Quick for a hand and hopeless for a chase.** A crab crosses
            // the yard of sand between you and the nearest stone faster than
            // you can reach down, and it has about two seconds of it in
            // (`stamina_seconds`).
            Species::Crab => 3.6,
        }
    }

    /// How many seconds it can hold `run_speed` before it is blown.
    ///
    /// **A chase should end because something ran out of wind, not
    /// because two constants were compared once at the start of it.**
    /// Every hunt in this world used to be decided the instant it began:
    /// two speeds were compared and the faster one won, given enough
    /// map. With a finite wind the same speeds produce a *race*: whoever
    /// is still running when the other is blown takes it, and the deer
    /// -- which is now faster than everything that hunts it, the player
    /// included -- is caught by being cut off or by being reached, never
    /// by being followed. The server slows a blown animal rather than stopping it
    /// (`logic::animals::BLOWN_SPEED`) and gives the wind back while it
    /// stands.
    ///
    /// The numbers are what each animal is:
    ///
    /// * The **hare** has almost none. Its whole design is a burst and a
    ///   stop, and it is faster than a sprint -- a hare with a deer's
    ///   wind could never be caught at all.
    /// * The **deer** has the longest of the prey, because the deer is
    ///   the meal and running one down is meant to be a commitment: a
    ///   quarter of a minute at full sprint, which costs a player their
    ///   own stamina bar and then some.
    /// * The **wolf** has *less than the deer*, and that is the one
    ///   comparison here that decides a mechanic. A wolf that outlasts
    ///   its dinner catches every deer it ever sees and the meadows
    ///   empty; a wolf that blows first has to have closed the gap
    ///   before the deer noticed it, which is what the stalk in
    ///   `think_hunter` is for. It is also *slower* than the deer now,
    ///   and slower than a sprinting player: a pack you can neither
    ///   outrun nor outnumber is the punishment this file argues against
    ///   everywhere else, and the bear is the one animal allowed to be
    ///   that.
    /// * The **bear** has more than anything else alive, because "you
    ///   cannot outrun a bear" is the whole animal (see `run_speed`) and
    ///   a short wind would quietly repeal it. Twenty seconds is a
    ///   hundred and eighty blocks -- further than anybody escapes in a
    ///   straight line, and further than it used to be, because the
    ///   bear's run was raised to actually beat a sprint.
    pub fn stamina_seconds(self) -> f32 {
        match self {
            Species::Hare => 3.5,
            Species::Deer => 15.0,
            Species::Boar => 7.0,
            Species::Wolf => 11.0,
            // It is already slower than a walking player; what this adds
            // is that a flock which has been driven across a field is a
            // flock that has stopped bothering to run.
            Species::Sheep => 5.0,
            Species::Bear => 20.0,
            // A bird's bolt is three seconds of flight and then it is
            // back on the ground, which is exactly what flushing a
            // grouse twice in a minute looks like.
            Species::Fowl => 3.0,
            // **More than the lion that hunts it**, which is the wolf and
            // the deer said again: a lion that outlasted a zebra would
            // empty the grass, so a lion has to be close before the zebra
            // looks up. Under the bear, which keeps its promise.
            Species::Zebra => 17.0,
            // A sprint, not a marathon. The antelope is faster than anything
            // behind it for five seconds and then it is not, and those five
            // seconds are all its swerve has to work in.
            Species::Antelope => 5.0,
            // Short, and shorter than the zebra's: a lion is an ambush with
            // a burst at the end of it. Longer than the antelope's, which is
            // what makes an antelope catchable at all -- by the lion that
            // got near enough before it was seen.
            Species::Lion => 8.0,
            // A dart, then a glide. Just over the hare's, so the burst
            // animals keep the least wind (`nothing_here_can_run_flat_out`),
            // and short: a school that fled for ten seconds would leave the
            // reef, and the reef is where the player is.
            Species::Fish => 4.0,
            Species::Cod => 6.0,
            Species::Trout => 5.0,
            // The ambusher's wind: two seconds, and then it is a fish being
            // chased rather than a fish getting away.
            Species::Pike => 2.0,
            Species::Herring => 4.0,
            // Two bolts and it is spent. A rat's whole plan is the gap in
            // the wall, and it is never more than a few blocks from one.
            Species::Rat => 2.5,
            // Longer than the fowl's three: a gull's fright is a climb and a
            // wide circle, not a flutter into the next bush. Its soaring
            // spends none of it (`logic::animals::Mind::Soar`).
            Species::Gull => 12.0,
            // **The longest wind of anything that runs away**: a plains animal
            // escapes by distance, and a wolf pack (eleven seconds) that has not
            // closed before the herd looked up has lost it. Under the bear's,
            // which keeps its promise.
            Species::Horse => 18.0,
            Species::Monkey => 7.0,
            // The shortest wind of anything alive. A crab's escape is the
            // rock, not the distance.
            Species::Crab => 2.0,
        }
    }

    /// How much damage it takes to kill.
    ///
    /// In the player's own units, so a boar is worth two thirds of a
    /// person -- which, given that a flint knife is not a spear, is
    /// several swings and time enough for it to answer.
    ///
    /// Every row here is multiplied by [`TOUGHNESS`], which is where the
    /// player's "сделай животным в 5 раз больше здоровья" lives. The rows
    /// themselves are the *ratios* -- a bear is three boars, a fowl is
    /// one good hit -- and those are the arguments the comments make, so
    /// the multiplier is one knob beside them rather than five times
    /// thirteen numbers that would have to be argued again.
    pub fn health(self) -> f32 {
        TOUGHNESS * self.base_health()
    }

    /// The species' own number, before [`TOUGHNESS`].
    ///
    /// Private and separate so the table can go on saying what a species
    /// is *worth against the others*, which is the part of it that was
    /// designed: every comment below is a comparison, and every one of
    /// them survives any multiplier at all.
    fn base_health(self) -> f32 {
        match self {
            Species::Hare => 4.0,
            Species::Deer => 10.0,
            Species::Boar => 14.0,
            Species::Wolf => 12.0,
            // More than a hare and less than a deer. Enough that a
            // flint knife takes a few strokes, so shearing a flock is
            // a job rather than a walk.
            Species::Sheep => 8.0,
            // Three boars. With `hide_armour` on top, this is
            // the animal that cannot be worn down by swinging:
            // see the note there for the arithmetic.
            Species::Bear => 40.0,
            // A bird. One good hit.
            Species::Fowl => 3.0,
            // A deer and a half: a big animal to bring down, which is why a
            // lion goes for the antelope when it can.
            Species::Zebra => 14.0,
            Species::Antelope => 6.0,
            // Between the wolf and the bear: three spear thrusts through its
            // hide (`hide_armour`). Enough that a fight with one is a fight,
            // and not so much that it is the bear's.
            Species::Lion => 22.0,
            // Two punches, one thrust. The fish is not hard to kill; it is
            // hard to reach.
            Species::Fish => 2.0,
            Species::Cod => 6.0,
            Species::Trout => 3.0,
            // Twice a cod's meat is not twice a cod's hide: a pike is soft
            // and it is one good thrust, which is what keeps it a *catch*
            // rather than a fight under water with no air.
            Species::Pike => 7.0,
            Species::Herring => 2.0,
            // Half a hare: two hits with anything, one with a spear. It
            // is not meant to survive being found.
            Species::Rat => 2.0,
            // The fowl's: one good hit, which is why the hunt is the getting
            // near and not the fight.
            Species::Gull => 3.0,
            // A zebra and a bit: a big animal, and what a wolf pack pulls down
            // is a horse left standing, not a horse that can run.
            Species::Horse => 16.0,
            Species::Monkey => 6.0,
            Species::Crab => 3.0,
        }
    }

    /// What it does to a player who is standing too close, per hit.
    ///
    /// Zero for everything that runs. A boar's four is enough that being
    /// gored twice while distracted is a quarter of your health, and not
    /// enough to make walking through a forest a fight.
    pub fn damage(self) -> f32 {
        match self {
            Species::Hare
            | Species::Deer
            | Species::Sheep
            | Species::Fowl
            | Species::Zebra
            | Species::Antelope
            | Species::Fish
            | Species::Cod
            | Species::Trout
            | Species::Pike
            | Species::Herring
            | Species::Gull
            // **A rat never fights**, and that is a design decision
            // rather than an oversight. Something that bit back would be
            // a fight in a corridor at night with a torch in one hand,
            // and the answer to it would be armour -- which is the one
            // answer this mechanic must not have. What a rat costs is
            // stores, and stores are defended with light and walls.
            | Species::Rat
            // A horse runs. The one way it hurts anybody is by throwing
            // them, and that is the breaking's (`husbandry::Breaking`), not
            // a blow it chooses to land.
            | Species::Horse
            // **A monkey does not fight, it takes.** Something that bit back
            // would turn the troop into a swarm to be killed, and killing
            // the troop is the answer this mechanic must not have -- see
            // `Species::Monkey`. What it costs you is the stores.
            | Species::Monkey => 0.0,
            // **Half a point, which is the smallest blow in the game.** A
            // crab cannot beat anybody: what it can do is make reaching into
            // the dark for one a thing you decide to do. See `Species::Crab`
            // -- and `death_cause`, which has to name it because a blow this
            // small can still be the last one somebody takes.
            Species::Crab => 0.5,
            Species::Boar => 4.0,
            // **Between a boar and a bear.** Four bites is a dead player,
            // which makes a lion something you do not stand and trade blows
            // with -- and not the bear's three, so a spear and a steady
            // hand can still win it.
            Species::Lion => 6.0,
            // **Two and a half boars a blow.** Three landed hits
            // kill a healthy player outright, which is the
            // number that makes fighting one a mistake rather
            // than a fight. Nothing in leather survives standing
            // still in front of it.
            Species::Bear => 9.0,
            // Less per bite than a boar and far more often: a boar
            // hits you twice and leaves, and a pack stays.
            Species::Wolf => 3.0,
        }
    }

    /// Does it come at you rather than away?
    ///
    /// **Read off `provoke_range` and not off `damage`**, and the crab is why
    /// the two had to be told apart. This used to be `damage() > 0.0`, which
    /// was the same list for as long as the only things that could hurt
    /// anybody were the four that hunt -- and then the shore got an animal
    /// that lands a blow and never walks toward anybody in its life
    /// (`Species::Crab`, whose blow is the server's `pinch`, a defence
    /// against the hand that closes on it). Under the old definition a crab
    /// was hostile, which is four wrong answers in four different files: the
    /// client warned about one, a bird was flushed by one, and the server
    /// gave it a predator's recovery.
    ///
    /// So: `damage` is what a blow is worth when it lands, and this is
    /// whether the animal will come and land one. A hostile animal is one
    /// with a distance at which it takes offence, which is exactly what
    /// `provoke_range` is.
    #[inline]
    pub fn is_hostile(self) -> bool {
        self.provoke_range() > 0.0
    }

    /// Does this species hunt that one?
    ///
    /// **The row that makes the animals a world rather than four kinds
    /// of furniture.** Every animal here used to know about exactly one
    /// thing: the player. A wolf walked past a deer, and a deer grazed
    /// beside a wolf, because neither had any idea the other existed --
    /// which is the single most visible way for a simulated animal to
    /// look stupid, and no amount of cleverness about fleeing *players*
    /// fixes it.
    ///
    /// One direction only, and asymmetric on purpose: a wolf hunts, a
    /// deer is hunted, and nothing here hunts a wolf. A boar is left out
    /// of both columns -- it is big, bad-tempered and not worth the
    /// trouble, which is the truth about boars and also stops the two
    /// hostile species from spending the whole game killing each other
    /// where no player can see it.
    pub fn hunts(self, other: Species) -> bool {
        match self {
            // **And the sheep**, which was left out while a sheep was only
            // ever wild: slow, unafraid and no fighter, it is the easiest
            // dinner on the map, and a wolf that walked past a flock was the
            // one reason a pen was never needed. With it in, a flock grazing
            // loose at night is a bet, and a wall two blocks high is the
            // answer to it (the server's `raid_the_pens`).
            //
            // **And the horse**, which is the price of keeping one. A wild
            // herd outlasts a pack (`stamina_seconds`), so what a wolf takes is
            // the horse left tied at home through the night -- and the answer
            // to that is the stable wall, the same one the sheep needed.
            Species::Wolf => matches!(other, Species::Hare | Species::Deer | Species::Sheep | Species::Horse),
            // The savanna's grazers and nothing from the woods: the two
            // never share ground (`lives_in`), and a rule about a deer that
            // strayed over a border is a rule nobody ever sees working. Not
            // the hare either, which shares the grass: a lion does not run
            // down a mouthful, and a lion that did would be one more thing
            // emptying the place a player needs something in.
            // ...and a horse that has grazed out onto the straw
            // (`lives_in`): the savanna's edge is a border because both
            // sides of it are dangerous in their own way.
            Species::Lion => matches!(other, Species::Zebra | Species::Antelope | Species::Horse),
            _ => false,
        }
    }

    /// Does it hunt anything at all?
    #[inline]
    pub fn is_predator(self) -> bool {
        Species::ALL.iter().any(|&other| self.hunts(other))
    }

    /// How much of a blow this animal's hide simply takes, before any of
    /// it reaches the animal.
    ///
    /// **The answer to "why can I beat a boar to death with my fists".**
    /// Subtracted from every hit rather than multiplied into it, and
    /// that is the whole design: a big animal is not *tougher* in
    /// proportion, it is wearing something a fist cannot get through at
    /// all. The arithmetic, against `primitive_server::hunting_damage`:
    ///
    /// | | fist 1.5 | flint knife 4.2 | flint spear 8.6 |
    /// |---|---|---|---|
    /// | hare (0.0) | 1.5 | 4.2 | 8.6 |
    /// | deer (0.4) | 1.1 | 3.8 | 8.2 |
    /// | boar (1.4) | **0.1** | 2.8 | 7.2 |
    /// | bear (2.6) | **0.0** | 1.6 | 6.0 |
    ///
    /// The table is what gets *through* the hide, before [`WEAPON_BITE`]
    /// multiplies it and before `TOUGHNESS` is counted on the other side;
    /// `what_five_times_the_health_costs_in_blows` states the fight in
    /// whole blows. So a boar takes hundreds of punches and a handful of
    /// knife strokes, and a bear cannot be punched to death at all -- while
    /// the hare a new player meets on their first morning is exactly as
    /// killable by hand as it ever was. What the mechanic asks for is a
    /// *weapon* before a big animal, which is what the flint spear is
    /// for and why it was worth knapping.
    ///
    /// A floor of a tenth rather than zero for anything that can be
    /// hurt at all: a blow that lands for literally nothing reads as a
    /// broken game, and a hundred and forty punches is already the
    /// answer "not like this". The bear is the exception and is meant
    /// to be: `bare_hands_are_hopeless` names it.
    pub fn hide_armour(self) -> f32 {
        match self {
            // Nothing worth speaking of: skin and feathers -- and the
            // antelope's thin summer coat, which is the hare's at a larger
            // size.
            // ...and scales, which turn nothing a knife is sharp enough for.
            Species::Hare
            | Species::Fowl
            | Species::Antelope
            | Species::Fish
            | Species::Cod
            | Species::Trout
            | Species::Pike
            | Species::Herring
            | Species::Gull
            | Species::Rat
            | Species::Monkey
            // **A shell, and it stops nothing.** It is armour against a gull
            // and a wave; a boot goes through it. Armour on an animal worth
            // one mouthful would be a chore with a right answer written on
            // it, which is what `hide_armour`'s own table is careful not to
            // be.
            | Species::Crab => 0.0,
            // A deer's coat turns a graze and no more.
            Species::Deer | Species::Sheep | Species::Zebra | Species::Horse => 0.4,
            // A mane and a thick hide over the shoulders, under a boar's
            // shield: a lion is killed with a spear, and a knife is a long
            // argument with something that bites back.
            Species::Lion => 1.0,
            // A boar's shield -- the slab of gristle over its shoulders
            // that a spear has to get through, and the reason a wild
            // pig is hunted with one rather than with a club.
            Species::Boar => 1.4,
            // Thinner than a boar's, and a wolf is not what you punch
            // anyway: it is what punches you.
            Species::Wolf => 0.8,
            // Fur, fat and hide. Nothing a person carries in their hand
            // gets through it but an edge on a shaft.
            Species::Bear => 2.6,
        }
    }

    /// What a blow of `damage` actually takes off this animal.
    ///
    /// One function so the server, the mods and the tests cannot
    /// disagree about it, and so the floor above is stated once.
    pub fn hurt_by(self, damage: f32) -> f32 {
        if damage <= 0.0 {
            return 0.0;
        }
        let through = damage - self.hide_armour();
        // A bear takes nothing at all from a bare hand -- see
        // `hide_armour` -- and everything else keeps a tenth, so a
        // player who is landing hits is always making some progress.
        let floor = if self == Species::Bear { 0.0 } else { 0.1 };
        // ...and the whole blow is worth [`WEAPON_BITE`] of itself, which
        // is what keeps five times the health from being five times the
        // hunt. After the hide, never before it: multiplied in first, a
        // fist would have got through a bear.
        through.max(floor) * WEAPON_BITE
    }

    /// What a blow has to be worth for [`hurt_by`] to take `wanted` off
    /// this animal: the same arithmetic read backwards.
    ///
    /// **Because three tests were doing it by hand and got it wrong the
    /// day it changed.** A test that wants a wolf standing at a third of
    /// its health is about the *third*; the hide it has to pay for on the
    /// way in, and now [`WEAPON_BITE`], are in front of that sentence and
    /// not part of it. Written out at each site, they were three copies of
    /// a formula that has been edited twice -- once when `TOUGHNESS`
    /// arrived and once when the bite did -- and both times the copies
    /// went on compiling while meaning something else.
    ///
    /// [`hurt_by`]: Species::hurt_by
    pub fn blow_taking(self, wanted: f32) -> f32 {
        wanted / WEAPON_BITE + self.hide_armour()
    }

    /// Does it leave the ground?
    ///
    /// **The birds, and it is what makes a bird a bird.** Everything else
    /// here walks or swims; a flushed grouse goes up and away, which is why
    /// catching one is worth anything. The flight itself is the server's
    /// (`logic::animals`, `FLIGHT_HEIGHT`): a target altitude while the
    /// animal has a reason to be up, not a second physics.
    ///
    /// It said "one species" until the gull. What the second one changed
    /// is that a bird is now a *kind* of body rather than a name, so the
    /// client's model tests and the server's flushing ask this rather than
    /// `== Fowl` -- see `soars` for what separates the two.
    #[inline]
    pub fn flies(self) -> bool {
        matches!(self, Species::Fowl | Species::Gull)
    }

    /// Does it keep a stretch of sky over the shore, rather than a nest?
    ///
    /// **The gull's.** `flies` is the body; this is the life. A soaring bird
    /// circles high over its coast (the server's `Mind::Soar`) instead of
    /// hopping between perches round a tree, comes down on sand and rock
    /// rather than into a canopy, sits on the water where it lands on it
    /// instead of making for the bank, and spawns from the coast against its
    /// own allowance (`MAX_SEABIRDS_PER_PLAYER`).
    ///
    /// A second predicate rather than `== Gull` for the reason `swims` is
    /// one: every rule the gull does not share with the fowl is one line
    /// hung off this, and the day a cormorant arrives it is one name here.
    #[inline]
    pub fn soars(self) -> bool {
        matches!(self, Species::Gull)
    }

    /// Where it will spawn: `None` means anywhere the ground is grass.
    ///
    /// **Only the bear has an answer**, and the player asked for it:
    /// a bear belongs in the woods, not in a meadow behind the house.
    /// Stated as *what the ground says* rather than as a biome name,
    /// because the spawner reads the world and not the generator -- see
    /// `logic::animals::wooded`, which counts the standing timber round
    /// the spot. That also makes it true of a wood a player planted,
    /// which a biome test would not be.
    #[inline]
    pub fn needs_trees(self) -> bool {
        // ...and the monkey, which is the same question asked for a
        // different reason: a bear wants a wood to be *in*, and a monkey
        // wants something to go *up* (`climbs`). Both are answered by the
        // timber round the spot rather than by the biome, which is also what
        // makes a grove a player planted on a warm shore fill with monkeys
        // -- and that is the right answer, where a planted acacia filling
        // with lions was not (`lives_in`). A grove is a grove.
        matches!(self, Species::Bear | Species::Monkey)
    }

    /// Does this species live in that biome? The spawner's question, asked
    /// of the spot a group would stand on.
    ///
    /// **A biome, not what the ground says** -- the opposite of
    /// `needs_trees`, so it has to be argued. A wood is timber, and timber
    /// is on the blocks: a spawner can count it. A savanna is not on the
    /// blocks. Its turf is the same grass a meadow's is, its acacias stand
    /// two hundred and forty columns apart, and nothing a spawner could read
    /// in a few dozen cells tells a savanna from a plain -- so it is asked
    /// of the generator (the server's `BlockWorld::biome`). The one thing a
    /// block test would have got right and this gets wrong is a grove of
    /// acacias a player planted in a meadow, and that not filling with lions
    /// is also right.
    ///
    /// **The whole table, not "everything except"**, so a new species is a
    /// compile error here rather than a deer in the desert. The woods'
    /// animals keep out of the savanna -- a zebra grazing beside a sheep is
    /// the meadow with the colour turned down, which is exactly what the
    /// savanna was before it had animals of its own -- and the hare and the
    /// bird live wherever there is grass, which is what makes the border a
    /// place rather than a wall.
    pub fn lives_in(self, biome: crate::worldgen::Biome) -> bool {
        use crate::worldgen::Biome;
        match self {
            Species::Zebra | Species::Antelope | Species::Lion => biome == Biome::Savanna,
            // **The plains, and the edge of the savanna.** The open grass
            // of the temperate country is the horse's, and the grass does not
            // stop at a line: a herd grazing out onto the straw is what the
            // border between the two looks like. It is rarer out there than
            // the zebra (`spawn_weight_in`), so the savanna stays the
            // zebra's country. Not the woods: a horse in a forest is a horse
            // somebody rode there.
            Species::Horse => matches!(biome, Biome::Plains | Biome::Savanna),
            // **The palm coast, which is this world's tropics.** There is no
            // jungle biome and one is not being added for a troop of
            // monkeys: a biome is a climate, a soil, a tree and a place on
            // the map, and inventing a continent to hang one animal off is
            // the tail wagging the world. What a monkey wants is fruit over
            // its head and something to climb, and the hot beach already has
            // both -- the palms and their coconuts
            // (`types::BLOCK_PALM_COCONUTS`).
            //
            // The beach *and* the trees: `needs_trees` is true of it too, so
            // a cold shingle strand with nothing growing on it gets none.
            // The two together are "a grove on a warm shore", said in the two
            // ways the spawner can ask -- the generator for the country and
            // the blocks for the timber.
            Species::Monkey => biome == Biome::Beach,
            // The sand and the shallows off it. A crab is found where the
            // water meets the land, which is these two biomes and no other.
            Species::Crab => matches!(biome, Biome::Beach | Biome::Ocean),
            Species::Deer | Species::Boar | Species::Wolf | Species::Sheep | Species::Bear => {
                biome != Biome::Savanna
            }
            Species::Hare | Species::Fowl => true,
            // **The water decides, and the biome only says which water.** A
            // school lives in any river, lake or sea deep enough to swim in,
            // whatever country is round it -- the spawner asks for depth
            // (`logic::animals::populate_water`). The cod is the open sea's.
            Species::Fish => true,
            Species::Cod => biome == Biome::Ocean,
            // **The rest of the water, divided by the two things a biome
            // already knows: whether it is salt, and whether it is cold.**
            //
            // A trout is cold, running, fresh water: the river itself, and
            // the lakes and tarns of the cold countries. A pike is the
            // opposite corner -- warm standing fresh water, the meadow lake
            // and the forest pond and the swamp -- and is deliberately *not*
            // in the river, so that walking upstream into the hills changes
            // what is on the end of the line. A herring is the shallow sea
            // and the coast off it, where the cod is the deep sea: the two
            // together make the depth of the water the thing that decides,
            // which is what `fishing::rod_catch` reads as well.
            //
            // The temperature is read off the biome rather than off the
            // climate, for `lives_in`'s own reason: this is asked once per
            // spawn attempt, of a spot, and a taiga is cold country whatever
            // the day is doing. The one thing it gets wrong is a hot summer
            // in the north, and a trout that is still in the mountain lake
            // in August is the right answer anyway.
            Species::Trout => matches!(
                biome,
                Biome::River
                    | Biome::Taiga
                    | Biome::Tundra
                    | Biome::Bog
                    | Biome::Mountains
                    | Biome::SnowyPeaks
                    | Biome::BirchForest
            ),
            Species::Pike => matches!(
                biome,
                Biome::Swamp
                    | Biome::Plains
                    | Biome::Steppe
                    | Biome::Hills
                    | Biome::Forest
                    | Biome::DeadForest
                    | Biome::Savanna
            ),
            Species::Herring => matches!(biome, Biome::Ocean | Biome::Beach),
            // **The coast, and the sea off it.** Not a river: a gull on a
            // river bank a hundred blocks from salt water is a bird that is
            // lost. Keyed on the biome and not on the sand, because the
            // shore is changing under it (palms, a climate with an equator
            // in it) and a beach is still a beach whatever grows on it.
            Species::Gull => matches!(biome, Biome::Ocean | Biome::Beach),
            // **Nowhere, and that is the point.** Every other row here is
            // a country; a rat's country is a kitchen. It is not put into
            // the world by the biome spawner at all (`spawn_weight` is
            // zero for it in both halves of the day) but by the server's
            // vermin pass, out of the map of where a player actually
            // lives (`haunt`). Saying "false" here rather than leaving it
            // out is what keeps `the_savanna_keeps_its_own_animals` and
            // every other biome test true without a special case in them.
            Species::Rat => {
                let _ = biome;
                false
            }
        }
    }

    /// Does it live in the water rather than on the land?
    ///
    /// **The one line the swimming hangs off**, on the server and in the
    /// tests: a swimmer thinks with `think_fish` and moves with `swim`
    /// instead of `think` and `walk`, spawns from `populate_water` against
    /// its own cap (`MAX_FISH_PER_PLAYER`), and is left out of every rule
    /// about pasture, cover, carcasses and thirst -- each of which is a
    /// sentence about an animal with legs.
    #[inline]
    pub fn swims(self) -> bool {
        matches!(
            self,
            Species::Fish | Species::Cod | Species::Trout | Species::Pike | Species::Herring
        )
    }

    /// Does it go up a tree rather than round it?
    ///
    /// **The monkey's, and it is not a second flight.** A bird is given a
    /// target altitude and pulled toward it through open air (the server's
    /// `FLIGHT_HEIGHT`); a climber has no altitude of its own at all -- it
    /// is held against whatever timber it is touching and walks up it, and
    /// the moment there is no trunk under its hands it falls like everything
    /// else. So the wood decides how high a monkey gets, which is what makes
    /// a grove of palms a place a troop lives in rather than a backdrop they
    /// hover over.
    ///
    /// Rejected: *making it `flies()` with a low ceiling.* Three lines and
    /// wrong in every one of them -- it would have wingbeat on the model, a
    /// glide path, a landing flare, and a monkey able to cross open ground
    /// at head height with nothing under it.
    #[inline]
    pub fn climbs(self) -> bool {
        matches!(self, Species::Monkey)
    }

    /// Does it take food that is not its own -- out of a hand, off the
    /// ground of a camp, out of what is stored there?
    ///
    /// **Two species do, and they do it in opposite places**: the rat in a
    /// house at night out of a chest (`vermin::gnaw`, which is its own pass
    /// and stays its own), and the monkey in daylight out of what a player
    /// is carrying. A predicate rather than `== Monkey` because the thing
    /// the server hangs off it -- a mind that goes for the player's stores
    /// instead of away from the player -- is the interesting half, and the
    /// day something else learns it, it is one name here.
    #[inline]
    pub fn pilfers(self) -> bool {
        matches!(self, Species::Monkey)
    }

    /// Will it be found standing on bare sand, shingle or rock rather than
    /// on grass?
    ///
    /// **The spawner's second ground, and until the shore there was only
    /// one.** The land spawner puts an animal down where a tuft of grass
    /// would grow (the server's `is_pasture`), and for eighteen species that
    /// was the same question as "will it be standing here": every one of
    /// them either grazed or hunted something that did. A crab and a troop of
    /// monkeys do neither -- what they live on is the sand under the palms --
    /// and under the pasture test alone neither of them would ever have
    /// appeared in a world at all.
    ///
    /// A predicate rather than "the beach's animals", because the question is
    /// about the *floor* and not about the country: the day something is
    /// wanted on a scree or a dune, it is one name here.
    #[inline]
    pub fn walks_on_sand(self) -> bool {
        matches!(self, Species::Monkey | Species::Crab)
    }

    /// Does it go sideways -- its body across its heading?
    ///
    /// The crab's, and **nothing draws anything differently for it**: the
    /// model is simply built wide (`animals/crab.bbmodel` is 0.58 across and
    /// 0.31 along), so an animal walked forwards by the ordinary code goes
    /// the way it is wide, which is what sidling is. A quarter turn between
    /// where it points and where it is drawn was the first idea and it is
    /// the wrong one: it makes the body and the heading disagree, and then
    /// every rule about a flank, a hit box and a blind spot has to be told
    /// about the lie.
    ///
    /// What the predicate is for is the three places that would otherwise
    /// have to know the crab by name: the hit box, which lies the same way
    /// (`every_animal_has_a_long_way_and_a_short_way_and_the_crab_is_the_one_turned_sideways`);
    /// the drawn model, which is checked against it
    /// (`animal_model::an_animal_has_a_long_way_and_a_short_way_and_the_crab_lies_across_its_own`);
    /// and the server's `nimbleness`, which lets a sidler change heading as
    /// fast as it likes instead of as fast as a body its width could swing
    /// round -- because a crab does not have to turn to go.
    #[inline]
    pub fn sidles(self) -> bool {
        matches!(self, Species::Crab)
    }

    /// Does it swerve as it runs?
    ///
    /// **The hare's trick, and it was written as the hare's**: `species ==
    /// Hare` in the server's `bolt`. A second `== Antelope` beside it would
    /// have been two copies of one rule waiting to disagree. Only the two
    /// small sprinters: a swerve costs ground, and an animal that outlasts
    /// what is behind it -- a deer, a zebra -- runs straight because it can.
    #[inline]
    pub fn zigzags(self) -> bool {
        matches!(self, Species::Hare | Species::Antelope)
    }

    /// Does it need another of its kind nearby before it comes at a person?
    ///
    /// **The wolf's nerve, and only the wolf's.** A boar and a bear defend
    /// the ground they stand on and do not count; a lion is a hunter that
    /// takes on something its own size alone. The wolf is the one that
    /// counts first, and the count is what a player can read before it
    /// arrives (see the server's `think_hunter`).
    #[inline]
    pub fn needs_company(self) -> bool {
        matches!(self, Species::Wolf)
    }

    /// Does the second of a pair work round to your far side before it
    /// comes in?
    ///
    /// The wolf's and the lion's, because both hunt *together* when there
    /// are two of them -- a pair of lions taking a zebra from two sides is
    /// the picture anybody has of one. A pair of boars does not: two boars
    /// are two charges from wherever they happen to be standing.
    #[inline]
    pub fn flanks(self) -> bool {
        matches!(self, Species::Wolf | Species::Lion)
    }

    /// Does it keep out of the light of a fire after dark?
    ///
    /// The two night hunters, for one reason: the campfire is the answer to
    /// the night, and a savanna whose night animal walked through a hearth
    /// would be the one country where a fire was furniture. Not the boar or
    /// the bear, which are not night animals and whose rule is distance.
    #[inline]
    pub fn shies_from_fire(self) -> bool {
        matches!(self, Species::Wolf | Species::Lion)
    }

    /// When it is losing, does it fall back and keep you in sight rather
    /// than leave?
    ///
    /// **The wolf's alone.** A wolf that shadows you at the edge of the
    /// torchlight is a wolf still out there. Everything else that fights
    /// breaks off and goes, the lion included: a beaten lion that followed a
    /// player home would make every fight with the one hostile animal that
    /// does not wait for a pack a fight to somebody's death.
    #[inline]
    pub fn falls_back_when_hurt(self) -> bool {
        matches!(self, Species::Wolf)
    }

    /// Does it eat what it did not kill?
    ///
    /// Separate from `is_predator` although the answer is the same
    /// today, because the two are different facts and the day a bear or
    /// a crow arrives they will differ: everything that hunts also
    /// scavenges, and not everything that scavenges hunts. The server
    /// asks this before spending a scan on finding a carcass -- see
    /// `logic::animals::carcass_near`.
    #[inline]
    pub fn scavenges(self) -> bool {
        // The lion as well: a kill left lying on the savanna is the lion's
        // dinner the way one in a meadow is the pack's.
        matches!(self, Species::Wolf | Species::Lion)
    }

    /// How long a kill keeps it fed, in seconds.
    ///
    /// Four minutes, and it is a population control as much as a
    /// stomach. A pack that hunted continuously would clear every deer
    /// within the spawn radius and then stand in an empty meadow, and
    /// what the player would see is a world that had run out of animals
    /// for reasons invisible to them. A fed wolf walks past a deer,
    /// which is also what a fed wolf does.
    pub fn fed_seconds(self) -> f32 {
        match self {
            Species::Wolf => 240.0,
            // Longer than a wolf's: a lion eats a zebra, and then it lies
            // under a tree -- which is what keeps a pair from clearing the
            // grass of the only animals a savanna has to eat.
            Species::Lion => 300.0,
            _ => 0.0,
        }
    }

    /// How close a player has to come before it takes offence, in
    /// blocks.
    ///
    /// **Not the same as `awareness`, and the gap between them is the
    /// whole of what a boar is.** It *notices* you at seven metres and
    /// stops to look; it comes at you at three. A hostile animal that
    /// charges the moment it can see you is not a boar, it is a homing
    /// missile with tusks -- and it makes a forest unwalkable, because
    /// there is no distance at which you can decide to leave one alone.
    ///
    /// Zero for anything that runs: they have no offence to take.
    pub fn provoke_range(self) -> f32 {
        match self {
            Species::Hare | Species::Deer | Species::Zebra | Species::Antelope
            | Species::Rat | Species::Horse
            // A monkey coming at you is coming at what you are holding, and
            // that is a mind (`pilfers`) rather than an offence taken.
            | Species::Monkey
            // ...and a crab never comes at anybody at all. Its blow lands on
            // the hand that closes on it (the server's `pinch`), which is a
            // defence and not a charge.
            | Species::Crab => 0.0,
            Species::Boar => 3.0,
            // Between the bear's four and a half and the wolf's six, and
            // unlike the wolf's it means what it says: a lion does not wait
            // for company (`needs_company`). Five blocks is still a distance
            // backing away works from, against something slower than a
            // sprint.
            Species::Lion => 5.0,
            // Twice a boar's, and it *still* does not mean a wolf
            // comes at you: the server asks whether it has company
            // first. See `think_hunter`.
            Species::Wolf => 6.0,
            // Nothing. A sheep does not decide to come at anybody.
            Species::Sheep => 0.0,
            Species::Fish | Species::Cod | Species::Trout | Species::Pike | Species::Herring | Species::Gull => 0.0,
            // Between the boar's three and the wolf's six: a
            // bear does not charge across a clearing at you, and
            // it does not let you walk past it either.
            Species::Bear => 4.5,
            Species::Fowl => 0.0,
        }
    }

    /// How long it stays angry after being hit, in seconds.
    ///
    /// What makes hitting one a *decision*. A boar you have wounded
    /// comes after you whether or not you back off, for long enough that
    /// running is a real choice with a real cost -- and then it stops,
    /// because an animal that never forgets is an animal you have to
    /// kill.
    pub fn grudge_seconds(self) -> f32 {
        match self {
            Species::Hare
            | Species::Deer
            | Species::Sheep
            | Species::Fowl
            | Species::Zebra
            | Species::Antelope
            | Species::Fish
            | Species::Cod
            | Species::Trout
            | Species::Pike
            | Species::Herring
            | Species::Gull
            // Nothing to hold. A rat that has been swung at runs, and
            // when it comes back it is because the store is still there,
            // not because it remembers you.
            | Species::Rat
            | Species::Horse
            | Species::Crab => 0.0,
            // **A troop remembers, and it is the one thing here that holds a
            // grudge without ever fighting.** Two minutes is long enough
            // that a stone thrown at a monkey buys a quiet afternoon, and
            // short enough that it does not buy the week: what keeps the
            // stores is where they are built, not what was thrown.
            Species::Monkey => 120.0,
            Species::Boar => 12.0,
            // Under the wolf's twenty. A lion that has been hurt comes for
            // you, and then it has a zebra to think about.
            Species::Lion => 18.0,
            // The longest in the world. A bear that has decided
            // about you has decided, and outrunning it is not
            // one of the things you can do (`run_speed`).
            Species::Bear => 30.0,
            // Longer than a boar's. A boar drives you off its patch
            // and stops; a wolf was hunting you before you touched it.
            Species::Wolf => 20.0,
        }
    }

    /// What killing one leaves on the ground: the block, and how many.
    ///
    /// Meat scales with the animal, which is the only sensible way for
    /// it to scale, and it is also what stops the hare from being the
    /// efficient hunt: it is the fastest thing in the world and it is
    /// one mouthful.
    ///
    /// Hide only from the two big ones. A hare skin is not a strap, and
    /// the strap is what the metal tools are bound with -- so the metal
    /// age runs on the animals you have to work for.
    pub fn drops(self) -> &'static [(BlockId, u32)] {
        match self {
            Species::Hare => &[(BLOCK_HARE_MEAT, 1), (BLOCK_PELT, 1)],
            // **One scrap of meat, and no new item.** A rat skin was the
            // obvious drop and it was rejected: an item that one animal
            // gives and nothing uses is a square in the pack that means
            // nothing, a picture in an atlas with a ceiling
            // (`texture::MAX_LAYERS`) and a row in four tables. The meat
            // is the game's existing small-game meat, and it says the
            // thing worth saying -- **the vermin eating your stores are
            // themselves food**, which is a joke the player gets to make
            // in a bad winter rather than a mechanic anybody has to be
            // told about.
            Species::Rat => &[(BLOCK_HARE_MEAT, 1)],
            Species::Deer => &[(BLOCK_RAW_MEAT, 3), (BLOCK_HIDE, 2)],
            Species::Boar => &[(BLOCK_RAW_MEAT, 4), (BLOCK_HIDE, 1)],
            // Little meat and a good pelt, which is the whole trade a
            // wolf offers: you do not kill one for dinner, you kill one
            // because it came at you, and what you get for it is the
            // strap a metal tool is bound with.
            Species::Wolf => &[(BLOCK_WOLF_MEAT, 1), (BLOCK_PELT, 2)],
            // The only animal that drops something no other one does,
            // and the only reason to look for one. Three fleeces is
            // most of a garment, so a small flock is a coat -- which
            // is the pace this ought to go at, because being cold is
            // the problem it solves and being cold is an early
            // problem.
            //
            // No hide: a sheepskin is wool with a skin attached, and
            // giving it both would make the flock the answer to the
            // leather chain as well as the wool one.
            Species::Sheep => &[(BLOCK_RAW_MEAT, 2), (BLOCK_WOOL, 3)],
            Species::Bear => &[(BLOCK_BEAR_MEAT, 3), (BLOCK_BEAR_HIDE, 1)],
            Species::Fowl => &[(BLOCK_FOWL_MEAT, 1), (BLOCK_FEATHER, 3)],
            // **The savanna's leather.** Deer and boar do not live there
            // (`lives_in`), and a metal age that needed a hide the hot
            // country could not give would be a metal age the savanna sat
            // out. A deer's two hides and a boar's meat.
            Species::Zebra => &[(BLOCK_RAW_MEAT, 4), (BLOCK_HIDE, 2)],
            // Pelts rather than a hide, for the hare's reason at a larger
            // size: it cures to the same leather, and it takes a few.
            Species::Antelope => &[(BLOCK_RAW_MEAT, 2), (BLOCK_PELT, 2)],
            // The wolf's trade, bigger. Nobody hunts a lion for dinner; what
            // the fight is worth is the pelts.
            Species::Lion => &[(BLOCK_RAW_MEAT, 3), (BLOCK_PELT, 3)],
            // **The fish itself**, one of it -- and three off a cod. No skin,
            // no bone, no sinew: see `carcass` for why a fish is not taken
            // apart with a knife.
            Species::Fish => &[(crate::types::BLOCK_RAW_FISH, 1)],
            Species::Cod => &[(crate::types::BLOCK_RAW_FISH, 3)],
            // **One item, in different amounts, and that is deliberate.**
            // A raw trout, a raw pike and a raw herring would each want a
            // cooked one, a salted one, a dried one and a salt-dried one --
            // sixteen blocks, sixteen pictures and four more rows in
            // `rack` and `crafting` -- to say a thing the count already
            // says. What the species decide is *how much fish* a water
            // holds; what the barrel and the rack know is fish. See
            // `fishing`'s note on species with names.
            Species::Trout => &[(crate::types::BLOCK_RAW_FISH, 2)],
            Species::Pike => &[(crate::types::BLOCK_RAW_FISH, 3)],
            Species::Herring => &[(crate::types::BLOCK_RAW_FISH, 1)],
            // **Feathers, and not a dead end.** They already burn as
            // kindling (`hearth`), which is the one thing a stone-age player
            // wants a light, dry handful of; fletching was the other use
            // weighed and left alone, because there is nothing here yet to
            // fletch and the thrown weapon is the spear's, not a new bow's.
            Species::Gull => &[(BLOCK_FEATHER, 2), (BLOCK_FOWL_MEAT, 1)],
            // **The most meat of anything that runs, and a wretched trade.** A
            // horse killed is a week of dinners and two hides; the same horse
            // kept is every trip to the hills for as long as it is fed. Both
            // are answers, which is why the number is this high.
            Species::Horse => &[(BLOCK_RAW_MEAT, 5), (BLOCK_HIDE, 2)],
            // **One haunch and nothing else, and the meagreness is the
            // point.** A monkey is a nuisance you *can* kill, and the world
            // should not pay well for killing the nuisance: a troop hunted
            // out for a haunch apiece is an afternoon spent on one dinner,
            // where the same afternoon spent putting the stores out of reach
            // keeps the dinners already had. No hide -- nothing here tans
            // one -- and no sinew, for the hare's reason.
            Species::Monkey => &[(BLOCK_RAW_MEAT, 1)],
            // The claws and the body of one crab: a mouthful, and one that
            // has to see a fire (`food::sickness_seconds`).
            Species::Crab => &[(crate::types::BLOCK_CRAB_MEAT, 1)],
        }
    }

    /// The block an animal of this kind dies into, if it dies into one.
    /// See `butchering`.
    ///
    /// **`None` for a fish, and that is two decisions.** A fish dies in the
    /// water, and a carcass never lies in water -- the server already drops
    /// the heap for anything killed over a lake (`lay_carcass`) -- so a fish
    /// carcass would be a block that is laid only when a fish dies stranded,
    /// which is to say almost never. And a fish is one cut: gutting it is not
    /// a sequence of skin, sinew, bone and meat, and a block whose whole
    /// butchery is a single click is a click for nothing. So a fish gives
    /// itself (`drops`) where it dies, and nothing is laid.
    pub fn carcass(self) -> Option<BlockId> {
        Some(match self {
            Species::Hare => BLOCK_CARCASS_HARE,
            Species::Deer => BLOCK_CARCASS_DEER,
            Species::Boar => BLOCK_CARCASS_BOAR,
            Species::Wolf => BLOCK_CARCASS_WOLF,
            Species::Sheep => BLOCK_CARCASS_SHEEP,
            Species::Bear => BLOCK_CARCASS_BEAR,
            Species::Fowl => BLOCK_CARCASS_FOWL,
            Species::Zebra => BLOCK_CARCASS_ZEBRA,
            Species::Antelope => BLOCK_CARCASS_ANTELOPE,
            Species::Lion => BLOCK_CARCASS_LION,
            Species::Horse => crate::types::BLOCK_CARCASS_HORSE,
            // **Neither of the shore's two leaves a body**, for the gull's
            // reason: a carcass is a thing you kneel at with a knife and
            // there is nothing on a monkey or a crab worth the kneeling.
            Species::Monkey | Species::Crab
            | Species::Fish | Species::Cod | Species::Trout | Species::Pike | Species::Herring
            // Nothing to butcher. A rat drops what it drops where it
            // falls, like a gull, because a carcass is a thing you kneel
            // at with a knife and there is nothing on one worth the
            // kneeling.
            | Species::Rat => return None,
            // **No body: it falls as what it gives**, like a fish. Three
            // ways were weighed. A carcass of its own is a block id, a
            // block definition, a steppable-list row and a texture row in
            // three crates, for a body an eighth of a cell that yields three
            // things. The fowl's carcass, shared, would lie on the beach as a
            // grouse and pluck to a grouse's table. Dropping where it falls
            // costs nothing, and the decision the knife makes about a bird is
            // already the fowl's to teach; a gull struck down over the surf
            // has nothing to lie on anyway.
            Species::Gull => return None,
        })
    }

    /// The species whose carcass this block is, if it is one.
    pub fn of_carcass(block: BlockId) -> Option<Species> {
        Species::ALL
            .iter()
            .copied()
            .find(|species| species.carcass() == Some(crate::types::block_kind(block)))
    }

    /// What a knife takes off the carcass, cut by cut, in the order a
    /// butcher works.
    ///
    /// **A carcass instead of a heap, and cuts instead of a drop**,
    /// because the heap was the least interesting moment in a hunt: the
    /// chase was a decision and the kill was a fight, and then meat and
    /// hide simply appeared. Now the animal lies where it fell and is
    /// taken apart with a knife -- the skin whole first, then the sinew
    /// along the back, then the bones, then the meat -- and each cut is
    /// a click that yields one thing. The order is the order the work
    /// has to be done in, and it is what makes the *tool* a decision:
    /// see `primitive_server`, where an axe or a pick ruins the skin
    /// and a bare hand gets nothing but the one cut of meat that
    /// breaking the block gives.
    ///
    /// The sheep gives its fleece before its skin, because that is what
    /// a sheep is for. The yields are `drops` plus the sinew and bone
    /// the old heap never had -- a hunt is worth more now, not less --
    /// and a test holds the two lists together.
    pub fn butchering(self) -> &'static [(BlockId, u32)] {
        match self {
            Species::Hare => &[(BLOCK_PELT, 1), (BLOCK_SINEW, 1), (BLOCK_BONE, 1), (BLOCK_HARE_MEAT, 1)],
            // One lump of fat and no more: a deer is lean, and that is
            // the difference between it and a boar in every way that
            // matters here -- the deer is the hide and the meat, the
            // boar is the tallow.
            Species::Deer => &[
                (BLOCK_HIDE, 2),
                (BLOCK_FAT, 1),
                (BLOCK_SINEW, 3),
                (BLOCK_BONE, 2),
                (BLOCK_RAW_MEAT, 3),
            ],
            // **The fat animal, and until now it gave none.** A boar
            // is the commonest heavy animal in the world and the one
            // people have actually rendered lard out of for as long as
            // there have been people; fat came off the bear alone, so
            // the only light that burns properly was gated behind the
            // one animal that can kill you. Two against the bear's
            // three: the bear is still the better carcass, it is just
            // no longer the only one.
            Species::Boar => &[
                (BLOCK_HIDE, 1),
                (BLOCK_FAT, 2),
                (BLOCK_SINEW, 2),
                (BLOCK_BONE, 3),
                (BLOCK_RAW_MEAT, 4),
            ],
            Species::Wolf => &[(BLOCK_PELT, 2), (BLOCK_SINEW, 2), (BLOCK_BONE, 2), (BLOCK_WOLF_MEAT, 1)],
            Species::Sheep => &[
                (BLOCK_WOOL, 3),
                (BLOCK_HIDE, 1),
                (BLOCK_SINEW, 2),
                (BLOCK_BONE, 2),
                (BLOCK_RAW_MEAT, 2),
            ],
            // The biggest yield in the world, and the reason to want
            // one dead in spite of everything: a hide that makes the
            // warmest coat there is, more fat than anything else
            // carries, and the ribs, which are one meal rather than
            // several.
            Species::Bear => &[
                (BLOCK_BEAR_HIDE, 1),
                (BLOCK_FAT, 3),
                (BLOCK_SINEW, 3),
                (BLOCK_RIBS, 1),
                (BLOCK_BONE, 3),
                (BLOCK_BEAR_MEAT, 4),
            ],
            // Plucked rather than skinned, which is why the first cut
            // is feathers where every other animal's is a skin.
            Species::Fowl => &[(BLOCK_FEATHER, 4), (BLOCK_FOWL_MEAT, 1)],
            // A deer's cuts with a boar's meat, and one lump of fat: a zebra
            // is a lean runner, like the deer whose place it takes.
            Species::Zebra => &[
                (BLOCK_HIDE, 2),
                (BLOCK_FAT, 1),
                (BLOCK_SINEW, 3),
                (BLOCK_BONE, 2),
                (BLOCK_RAW_MEAT, 4),
            ],
            Species::Antelope => &[(BLOCK_PELT, 2), (BLOCK_SINEW, 2), (BLOCK_BONE, 1), (BLOCK_RAW_MEAT, 2)],
            // More than a wolf in every column, and a lump of fat a lean wolf
            // has not got: the most carcass of the hunters, bar the bear.
            Species::Lion => &[
                (BLOCK_PELT, 3),
                (BLOCK_FAT, 1),
                (BLOCK_SINEW, 3),
                (BLOCK_BONE, 2),
                (BLOCK_RAW_MEAT, 3),
            ],
            // A zebra's cuts at a horse's size: the most meat on the plain
            // and three bones, which is what a big frame leaves.
            Species::Horse => &[
                (BLOCK_HIDE, 2),
                (BLOCK_FAT, 1),
                (BLOCK_SINEW, 3),
                (BLOCK_BONE, 3),
                (BLOCK_RAW_MEAT, 5),
            ],
            // No carcass, so no cuts: see `carcass`.
            Species::Fish | Species::Cod | Species::Trout | Species::Pike | Species::Herring | Species::Gull
            | Species::Rat | Species::Monkey | Species::Crab => &[],
        }
    }

    /// What is left of a carcass nobody came back for.
    ///
    /// **The bones and the skin, and none of the meat.** A hunter who
    /// walks away from a kill loses the whole reason for the kill; what
    /// the ground keeps is the frame, which is the part that does not
    /// spoil, and a lump of carrion that is worth exactly what a
    /// toadstool is worth (see `food::harm`). The hide survives because
    /// a skin dries where it lies -- badly, and it is still a skin.
    ///
    /// Derived from `butchering` rather than written out again, so a
    /// species whose yields change cannot end up spoiling into
    /// something it never had. What is dropped is the same list with
    /// the meat turned to rot and the sinew gone: sinew is the one
    /// thing here that has to be taken out fresh.
    pub fn spoils_into(self, stage: usize) -> Vec<(BlockId, u32)> {
        let mut left = Vec::new();
        let mut spoiled = 0;
        // **From the cut the knife stopped at**, not from the top of the
        // list: what was already taken is in somebody's pack. Reading the
        // whole list gave a deer skinned and left lying a second hide the
        // next day. See `butchering_stage`.
        for &(block, count) in self.butchering().iter().skip(stage) {
            match block {
                BLOCK_RAW_MEAT => spoiled += count,
                BLOCK_SINEW => {}
                _ => left.push((block, count)),
            }
        }
        if spoiled > 0 {
            // Half, rounded up: a carcass rots into fewer lumps than it
            // would have fed people, because most of it is gone.
            left.push((crate::types::BLOCK_ROTTEN, spoiled.div_ceil(2)));
        }
        left
    }

    /// How far off it notices a player.
    ///
    /// The deer's is the longest, which is why it is the animal you have
    /// to *stalk* rather than walk up to; the boar's is the shortest,
    /// which is why you meet one by surprise.
    pub fn awareness(self) -> f32 {
        match self {
            Species::Hare => 9.0,
            // **It sees you coming and it does not care much.** Six
            // blocks: shorter than a hare's, because a rat's escape is
            // three feet of skirting board rather than a field, and it
            // will go on eating out of a chest with somebody in the
            // doorway. What actually moves it is light, which is the
            // server's `flees_light` and not this number.
            Species::Rat => 6.0,
            Species::Deer => 12.0,
            Species::Boar => 7.0,
            // The longest of the four, because a wolf finds you rather
            // than meeting you.
            Species::Wolf => 18.0,
            // Barely notices. It looks up, decides you are not a wolf,
            // and goes back to what it was doing.
            Species::Sheep => 6.0,
            // It hears you long before it looks up, and it is in
            // no hurry about it.
            Species::Bear => 14.0,
            // Jumpy: a bird is watching everything.
            Species::Fowl => 11.0,
            // A herd on open ground watches the whole horizon, and there is
            // nothing on it to stalk behind.
            Species::Zebra => 13.0,
            // **The widest eyes on the grass.** An antelope sees you before
            // the zebra does and before the lion does, which is what makes
            // it the one that bolts first -- and the animal a savanna hunt
            // is a stalk for.
            Species::Antelope => 16.0,
            // It finds its dinner before its dinner finds it, and it is
            // still short of the antelope: a lion that saw further than
            // everything it hunts would never need to creep.
            Species::Lion => 15.0,
            // **Short, because the water is.** The underwater fog closes at
            // about eighteen blocks, and a fish that bolted from a swimmer
            // it could see further off than the swimmer could see it would
            // be a school nobody ever saw -- a reef that is always empty
            // when you arrive.
            Species::Fish => 6.0,
            Species::Cod => 5.0,
            Species::Trout => 7.0,
            // **It sees you first**, which is the whole of what makes a
            // lake with a pike in it a stalk: there is one of it, it is
            // worth three fish, and it is gone before a careless swimmer is
            // in range.
            Species::Pike => 9.0,
            Species::Herring => 6.0,
            // **Nearer than a grouse lets you come**, because a gull lives
            // beside people and the harbour taught it how close is close.
            // Nine is the number the hunt is made of: a spear reaches six
            // (`combat::SPEAR_REACH`), so the three between are the stalk --
            // a dune, a rock, the dark. The server halves it for a flock
            // roosting after dusk.
            Species::Gull => 9.0,
            // The zebra's open-ground eyes and a little more: a plains herd
            // sees you across the grass, which is why taming starts with a
            // creep and not a walk (`husbandry::horse_lets_you_near`).
            Species::Horse => 14.0,
            // **The sharpest eyes in the world, and they are what the troop
            // is for.** A monkey sees you before anything on the ground
            // does, and then it says so (the server's `chatter`) -- which is
            // how a player who has learned the sound knows that something is
            // coming along the beach before they can see it.
            Species::Monkey => 20.0,
            // Stalked eyes: they see all round (`view_cone`) and not far. A
            // crab is watching the yard of sand it can reach a stone from.
            Species::Crab => 6.0,
        }
    }

    // ---- the three senses, and what each is for ----
    //
    // `awareness` used to be the whole of an animal's perception: a radius,
    // asked of the player's position and nothing else. So a deer knew a
    // person standing still behind it at eleven blocks exactly as well as
    // one sprinting at it across the meadow, and the only stalk there was
    // was a line-of-sight ray. What a hunter can actually *do* -- go
    // slowly, keep low in the grass, come at dusk, come from downwind --
    // changed nothing, so there was nothing to decide.
    //
    // Now there are three, and each is answered by a different choice:
    //
    // * **sight** (`awareness`, now read as how far it sees a walking
    //   person in daylight) needs a clear line and a direction it is
    //   looking in (`view_cone`), and is shortened by cover, darkness and
    //   standing still -- answered by *where* and *when* you go;
    // * **hearing** (`hearing`) goes through leaves and round corners, and
    //   scales with how loud the person is being -- answered by *how* you
    //   go: a sprint is heard at twice a walk, a creep at a third of one,
    //   and breaking stone rings out further than either;
    // * **scent** (`nose`) is carried by the wind -- answered by *which
    //   side* you come from.
    //
    // The arithmetic that combines them is the server's
    // (`logic::animals::perceive`); the numbers are here, per species,
    // beside the one they grew out of.

    /// How far off it hears an ordinary walking person, in blocks.
    ///
    /// Distance only: sound goes through a hedge and round a trunk, which
    /// is the whole difference between this and sight. The server scales
    /// it by what the person is doing (still, creeping, walking, running,
    /// working) -- see `logic::animals::Gait`.
    pub fn hearing(self) -> f32 {
        match self {
            // All ears. A hare hears you before anything else in the
            // meadow does, and a sprint is heard at twenty-four blocks --
            // which is why the hare you catch is the one you crept on.
            Species::Hare => 12.0,
            Species::Rat => 10.0,
            // A little short of its sight: the deer is the stalk, and the
            // stalk has to be something a patient walker can win.
            Species::Deer => 11.0,
            // Head down in the leaf litter, grunting over its own noise --
            // under its sight, and the shortest ears but the sheep's. You
            // meet a boar by surprise because it did not hear you walking;
            // it hears a sprint at sixteen.
            Species::Boar => 8.0,
            // A wolf hears a walker at fourteen and a runner at twenty-
            // eight, which is further than it sees: a pack finds a person
            // in the wood before it lays eyes on them.
            Species::Wolf => 14.0,
            // Chewing. A flock hears you when you are nearly among it.
            Species::Sheep => 5.0,
            // "It hears you long before it looks up" -- the bear's own
            // line in `awareness`, made true.
            Species::Bear => 12.0,
            Species::Fowl => 8.0,
            Species::Zebra => 10.0,
            // Twitchy ears on a twitchy animal.
            Species::Antelope => 12.0,
            Species::Lion => 11.0,
            // Unused: a swimmer thinks with `think_fish`, which has its own
            // short, water-bound awareness.
            // The lateral line: a fish feels the water a swimmer pushes
            // before it sees them, and a pike feels it furthest.
            Species::Fish | Species::Cod | Species::Herring => 4.0,
            Species::Trout => 5.0,
            Species::Pike => 7.0,
            // Over the noise of the surf.
            Species::Gull => 7.0,
            Species::Horse => 11.0,
            Species::Monkey => 14.0,
            // It feels a footfall through the sand rather than hearing it,
            // which comes to the same number and is why a crab is under a
            // stone before you are in reach of it.
            Species::Crab => 7.0,
        }
    }

    /// How far off it smells a person with the wind carrying the scent
    /// straight to it, in blocks. Zero for anything that does not hunt or
    /// hide by its nose.
    ///
    /// **Downwind is the far end, not the usual one.** The server takes
    /// under half of this in still air and a tenth of it with a wind
    /// blowing from the animal to the person, which is the approach a
    /// hunter learns: a deer at eighteen blocks downwind of you knows, and
    /// the same deer upwind lets you walk to within a couple of blocks of
    /// its nose -- if it does not see or hear you first.
    pub fn nose(self) -> f32 {
        match self {
            Species::Hare => 6.0,
            // **Nose-led, but this is the nose it turns on *people*.**
            // It was twenty -- "the best nose in the game" -- and scent
            // is the one sense with no front and no back, so a rat knew a
            // person standing still at nine blocks whichever way it faced
            // (`detection_distance_per_species_and_gait`): the only
            // animal in the game with no blind side, and a rat that was
            // gone before it was ever seen. Vermin is meant to be the
            // pitiful thing you catch in the storeroom; a seen rat has to
            // be a rat you can reach.
            //
            // Seven keeps it nose-led where that means something -- the
            // *wind* decides: downwind of you it knows at eight, upwind
            // it lets you to the skirting board -- and in still air
            // leaves it a hare's shape, a few blocks in front by the eyes
            // (`awareness`, six, with its wide prey's cone) and a little
            // less behind by the nose (measured: four in front, three
            // behind, against the hare's five and two). Timid is the
            // flight and the walls (`logic::animals::skulk`), not the
            // reach.
            // Rejected: keeping twenty and shortening only the back (a
            // nose that knows which way the head points is not a nose),
            // and cutting sight instead (sight was never the nine).
            // Finding the storeroom is not this number: rats come where
            // `haunt` says people live.
            Species::Rat => 7.0,
            Species::Deer => 18.0,
            // A pig's nose: in still air, seven blocks, which is why a
            // boar watching you from its thicket was never fooled by your
            // standing still.
            Species::Boar => 16.0,
            // The best nose here. It is how a pack finds you at night.
            Species::Wolf => 24.0,
            Species::Sheep => 6.0,
            Species::Bear => 22.0,
            Species::Zebra => 12.0,
            Species::Horse => 12.0,
            Species::Antelope => 10.0,
            Species::Lion => 12.0,
            // A monkey lives by fruit and finds it the way a bear finds
            // honey, at a fraction of the distance.
            Species::Monkey => 10.0,
            // Birds and fish go by their eyes.
            // ...and a crab by the water on its gills.
            Species::Crab
            | Species::Fowl
            | Species::Gull
            | Species::Fish
            | Species::Cod
            | Species::Trout
            | Species::Pike
            | Species::Herring => 0.0,
        }
    }

    /// How wide it sees, as the cosine of the angle off its nose at which
    /// its sight ends: `-1` is all the way round, `0` is the half in front.
    ///
    /// **Prey has eyes on the sides of its head and hunters have them in
    /// front**, and that is the one fact about eyes a player can use: a
    /// grazing deer is blind only in the narrow wedge straight behind it,
    /// and a watching wolf is blind to everything behind its shoulders.
    /// Outside the cone a person is still heard and smelled -- and
    /// anything within a couple of blocks is simply noticed.
    pub fn view_cone(self) -> f32 {
        match self {
            Species::Hare | Species::Fowl | Species::Gull | Species::Rat => -0.9,
            Species::Antelope => -0.85,
            Species::Deer | Species::Sheep | Species::Zebra | Species::Horse => -0.8,
            // A pig's eyes are halfway round.
            Species::Boar => -0.3,
            Species::Wolf | Species::Bear | Species::Lion => 0.0,
            Species::Fish | Species::Cod | Species::Trout | Species::Pike | Species::Herring
            // **All round, and it is the only land animal that can say
            // that.** A crab's eyes are on stalks over the shell: there is
            // no behind it, which is why creeping up on one does not work
            // and reaching quickly does.
            | Species::Crab => -1.0,
            // A monkey's eyes face forward -- it is judging the gap to the
            // next branch -- but its head is never still, and what a player
            // can use is the same as for every other prey animal: it has a
            // blind wedge dead behind it and nowhere else. The thing that
            // makes a troop hard to creep up on is not the cone, it is the
            // twenty blocks of `awareness` and the fact that there are four
            // of them looking different ways.
            Species::Monkey => -0.8,
        }
    }

    /// How it lives with its own kind. See [`Grouping`].
    pub fn grouping(self) -> Grouping {
        match self {
            Species::Deer | Species::Sheep | Species::Zebra | Species::Antelope | Species::Horse => Grouping::Herd,
            Species::Wolf | Species::Lion => Grouping::Pack,
            // **A troop is a herd that argues.** Nothing in `Grouping` says
            // anything a monkey does not do -- it keeps together, it has an
            // alarm the whole group answers, and it moves as one -- so it is
            // the herd's row, and the difference is in the mind rather than
            // in the shape of the group (the server's `chatter` and `raid`).
            Species::Monkey => Grouping::Herd,
            // Loose, and it is the loosest of the lot: crabs on a beach are
            // a lot of crabs in one place rather than a group of them, which
            // is exactly what `Loose` means.
            Species::Hare | Species::Fowl | Species::Boar | Species::Gull | Species::Rat
            | Species::Crab => Grouping::Loose,
            Species::Bear => Grouping::Alone,
            Species::Fish | Species::Cod | Species::Trout | Species::Herring => Grouping::School,
            // **The one swimmer that is not a school.** A cod is alone too
            // and is left in `School` because it is a school of one and
            // nothing reads that field for a lone fish; a pike is alone on
            // purpose, and saying so is what stops two of them being put in
            // one pond by `group_size`.
            Species::Pike => Grouping::Alone,
        }
    }

    /// When it runs, does it run for the trees?
    ///
    /// **The deer does and the zebra does not**, and the zebra's own doc
    /// said so for a whole release while the server sent every prey animal
    /// to the nearest wood. An animal of the open grass is safest in the
    /// middle of its herd on open ground, where it can see what is coming;
    /// sending it into a thicket put it exactly where a lion would want it.
    /// The sheep is the same animal, slower.
    pub fn hides_in_cover(self) -> bool {
        // ...and the crab, whose cover is a *stone* rather than a thicket --
        // which is the same rule reading the same table, because `is_cover`
        // now names the boulders as well as the trees. A crab has no
        // distance in it at all (`run_speed`, `stamina_seconds`): the rock
        // is the whole of its escape, and without this it would be an animal
        // that runs three blocks along an open beach and stops.
        // ...and the monkey, for whom "cover" means the thing it goes *up*:
        // `is_cover` is the standing timber, `bolt` steers a frightened
        // animal toward the nearest of it, and the server's `climbing` does
        // the rest once a hand is on a trunk. Without this line a monkey
        // that had been swung at ran along the beach like a small slow deer,
        // which is every part of the animal except the one that matters.
        matches!(
            self,
            Species::Hare | Species::Deer | Species::Fowl | Species::Crab | Species::Monkey
        )
    }

    /// Does it keep a patch of ground it will not let you walk through?
    ///
    /// **The bear's.** A boar drives you off the few blocks it is standing
    /// in; a bear has a den, and the thing to learn about a bear is where
    /// its ground is -- walk round it and it watches you go, walk across
    /// it and it comes, however far off it was when you started. See the
    /// server's `TERRITORY_RADIUS`.
    pub fn keeps_territory(self) -> bool {
        matches!(self, Species::Bear | Species::Lion)
    }

    // **The lion keeps ground and the boar does not**, and the difference is
    // what each animal is for. A lion has a range it hunts and comes back to,
    // so the savanna has places in it -- a waterhole you can cross at noon and
    // not at dusk -- and a player who walked into one can walk back out of it
    // the way they came, which is the whole of what a territory buys.
    //
    // A boar was asked for and turned down. Its rule is already distance:
    // it watches at seven metres and charges at three, wherever it happens to
    // be standing, and the reason that design exists is written up at
    // `Mind::Watch` -- a boar that charged everything it could see made the
    // wood it stood in impassable. A territory *adds* a reason to charge
    // somebody who has not come close, which is exactly the animal that was
    // taken out. What a boar defends is the ground under its feet, and it
    // does that already.

    /// How often one turns up here, relative to the others that live here.
    ///
    /// [`spawn_weight`](Self::spawn_weight), with **the predators of the woods
    /// commoner in the woods.** A wolf and a bear were weighted
    /// the same everywhere they lived, so a forest -- where both belong, and
    /// where a player goes for timber -- had as few as an open meadow, and a
    /// player walking through one met none at all: "медведи и волки в лесах
    /// очень редкие или их нету вовсе. опасности нету". The meadow keeps the
    /// old odds on purpose: open ground is where a player can see a wolf
    /// coming, and the wood is where they cannot, so the wood is where the
    /// danger should be.
    ///
    /// **A wolf four times by day and not at all more by night; a bear twice,
    /// always.** A wolf is already three times commoner after dark
    /// (`spawn_weight`), and multiplying that too made a wooded night three
    /// animals in five a predator -- the siege that weight was written to
    /// avoid. As it is, a wood is about a quarter predators by day and a
    /// third by night: the reason to carry a spear, not a place nobody can
    /// work in.
    pub fn spawn_weight_in(self, night: bool, biome: crate::worldgen::Biome) -> u32 {
        use crate::worldgen::Biome;
        let wooded = matches!(biome, Biome::Forest | Biome::BirchForest | Biome::Taiga | Biome::DeadForest);
        let base = self.spawn_weight(night);
        match (wooded, self, night) {
            (true, Species::Wolf, false) => base * WOODS_WOLF_BY_DAY,
            (true, Species::Bear, _) => base * WOODS_BEAR,
            // **The savanna's edge, not its middle**: one in the zebra's four,
            // so a herd of horses out on the straw is something a player
            // crossing the border meets now and then, and the savanna is
            // still the zebra's (`lives_in`).
            (_, Species::Horse, _) if biome == Biome::Savanna => 1,
            _ => base,
        }
    }

    /// How often one turns up, relative to the others.
    ///
    /// The spawner used to pick uniformly from `ALL`, which was fine
    /// while all three were harmless and is not fine with a predator in
    /// the list: an even split makes a quarter of everything alive a
    /// wolf, and a meadow with as many wolves in it as hares is not a
    /// meadow, it is a kennel.
    ///
    /// Weighted by what the animal *is*: prey is common, the thing that
    /// fights back is less so, and the thing that hunts is rarest of all
    /// -- except at night, which is when it hunts. That inversion is the
    /// whole of why being out after dark is somewhere different from
    /// where you were at noon.
    pub fn spawn_weight(self, night: bool) -> u32 {
        match (self, night) {
            // **Zero, day and night, and not an oversight.** The biome
            // spawner puts animals in the country around a player; a rat
            // is put in a player's *house*, by the server's vermin pass,
            // out of `haunt`. A weight here would mean rats in a meadow.
            (Species::Rat, _) => 0,
            (Species::Hare, false) => 4,
            (Species::Hare, true) => 3,
            (Species::Deer, false) => 4,
            (Species::Deer, true) => 4,
            (Species::Boar, _) => 2,
            // **One, and one is the point.** A bear is the rarest thing
            // that walks -- rarer by day than a wolf is, and no
            // commoner at night, because it is not a night animal, it
            // is a *place* animal: what makes meeting one a story is
            // that it is the only one for a long way. See
            // `group_size`, which is (1, 1) for the same reason.
            (Species::Bear, _) => 1,
            // Common as hares and half as interesting, which is what a
            // bird is: something the meadow always has, worth one meal
            // and four feathers to whoever can catch it.
            (Species::Fowl, false) => 4,
            // Roosting. A meadow at night has fewer birds in it than a
            // meadow at noon, and the ones out are the ones a wolf
            // finds first.
            (Species::Fowl, true) => 1,
            (Species::Wolf, false) => 1,
            // Three times as likely after dark, and still not the
            // commonest thing out there. A night that is mostly wolves
            // is not a night, it is a siege -- and a player who cannot
            // go out at all has been given a curfew rather than a
            // reason to carry a torch.
            (Species::Wolf, true) => 3,
            // Common by day, rarer after dark -- a flock beds down,
            // and a field of sheep at midnight is a field nobody has
            // ever walked past.
            (Species::Sheep, false) => 4,
            (Species::Sheep, true) => 1,
            // The savanna's commonest animal: a herd of zebra is what open
            // grass looks like from a hill.
            (Species::Zebra, false) => 4,
            (Species::Zebra, true) => 3,
            (Species::Antelope, false) => 3,
            (Species::Antelope, true) => 2,
            // **The wolf's hours.** Rare by day, when a lion is asleep under
            // an acacia, twice as likely after dark, when it hunts -- and
            // never the commonest thing out, for the reason the wolf's night
            // is not a siege.
            //
            // These weights are only ever compared with the animals that
            // live in the same place (see the server's `pick_species`), so a
            // lion's one is a sixteenth of a savanna afternoon and nothing
            // at all of a meadow's.
            (Species::Lion, false) => 1,
            (Species::Lion, true) => 2,
            // Only ever weighed against each other: the land spawner never
            // draws a swimmer and the water spawner never draws anything
            // else (`logic::animals::pick_species`). Schools are the sea; a
            // cod is the thing you are lucky to see.
            (Species::Fish, _) => 5,
            (Species::Cod, _) => 1,
            // The three that came after, against the same table: in the
            // water each of them lives in, the plain school is still the
            // commonest thing and the named fish is the one worth the walk.
            // A pike is the cod's number for the cod's reason -- one lake,
            // one pike.
            (Species::Trout, _) => 3,
            (Species::Pike, _) => 1,
            (Species::Herring, _) => 4,
            // Only ever weighed against the coast's own table, which is the
            // gull alone (the server's `populate_shore`): what these decide
            // is how often an attempt on the shore goes ahead at all. Fewer
            // after dark, when a flock is down on the sand and not arriving.
            (Species::Gull, false) => 4,
            (Species::Gull, true) => 2,
            // A zebra's numbers on the plains, where it is the grazer a player
            // sees from a hill; fewer after dark, when a herd stands and dozes
            // rather than arriving. See `spawn_weight_in` for the savanna.
            (Species::Horse, false) => 3,
            (Species::Horse, true) => 2,
            // **A troop is a thing you meet, not a thing the grove is made
            // of.** Three by day against the hare's and the fowl's, and one
            // by night, when the troop is up in the crowns asleep and the
            // beach is the crab's.
            (Species::Monkey, false) => 3,
            (Species::Monkey, true) => 1,
            // ...and the crab is the other way about, which is the whole
            // sentence: a beach at noon has a crab on it and a beach at
            // midnight is crawling.
            (Species::Crab, false) => 2,
            (Species::Crab, true) => 8,
        }
    }
}

/// A sprinting player, in blocks per second.
///
/// Written down here rather than reached for, because the number a
/// player actually moves at lives in the client's physics and in a
/// settings file, and this crate cannot see either. What it is *for* is
/// the comparisons the whole hunt turns on -- see `run_speed`.
///
/// **It said 8.8 for three versions after the truth became 6.45**, and
/// a copy that drifts is worse than no copy at all: every animal in the
/// table below was tuned against a sprint half again faster than the
/// one a player actually has, so "just under a sprint" meant *over* it
/// and the whole column was wrong in the direction that makes the world
/// harmless. The walk came down to 4.3 (`physics::DEFAULT_MOVE_SPEED`)
/// and the multiplier to 1.5 (`SPRINT_MULTIPLIER`); this is their
/// product, and the tests below hold the comparisons rather than the
/// numbers so the next drift fails the build instead of the hunt.
pub const NOMINAL_SPRINT_SPEED: f32 = 6.45;

/// How much a wandering animal is worth to a world, expressed as a cap.
///
/// Per player rather than per world: the spawner works near people,
/// because an animal nobody can see is a position update nobody needs.
///
/// **Three, and it was six.** Six inside a ninety-six block bubble is
/// not a population, it is a backdrop: whichever way a player walked
/// there was something grazing, so meeting a deer meant nothing and
/// meeting a bear meant only that this particular patch of scenery bit.
/// Halving it is the whole of "make the animals rare", and it is the
/// cheap half -- the rest is in the server's `SPAWN_INTERVAL` and
/// `SPAWN_MIN`, which decide how *often* and how *far off* the three
/// arrive.
///
/// What it costs is that a group is now most of a player's allowance,
/// which is why `group_size` came down with it: a flock of six sheep
/// under this cap would mean a player who found sheep found nothing else
/// until they left.
pub const MAX_ANIMALS_PER_PLAYER: usize = 3;

/// How many of a kind arrive together, at the least and at the most.
///
/// **A herd has to be spawned as a herd.** Everything about how these
/// animals behave in a group is already written -- deer take their lead
/// from the deer beside them, a spooked one starts the whole field
/// running, and a wolf alone will not attack a player while two will --
/// and none of it could ever happen, because they arrived one at a
/// time from random directions. What a player met was a lone deer, then
/// another lone deer somewhere else: the mechanics were in the code and
/// not in the world.
///
/// So a spawn is a *group*. The sizes are what each animal is: hares
/// scatter about singly or in pairs, deer and boar move in small
/// family groups, and wolves come as a pack -- which is the one that
/// changes a night, because two wolves is where a pack stops watching
/// and comes in.
///
/// **Every one of them came down when `MAX_ANIMALS_PER_PLAYER` did**,
/// and it had to: a single spawn must not be able to use up a player's
/// whole allowance, or the first flock they walk into is the last animal
/// they see. `a_group_is_a_group_and_never_bigger_than_the_pack_limit`
/// is the test that says so, and it is the reason this list cannot be
/// tuned on its own.
pub const fn group_size(species: Species) -> (u32, u32) {
    match species {
        Species::Hare => (1, 2),
        // One or two. A pair is a nuisance and a plague is a mechanic
        // nobody can answer with a torch -- and the per-player cap keeps
        // the total down anyway (`vermin::MOST_RATS`).
        Species::Rat => (1, 2),
        // A family herd: a hind or two and their young. Four at the most,
        // because a herd is spawned whole now (the server's `populate`) and a
        // meadow with five deer in it is the only thing near a player for as
        // long as they stay.
        Species::Deer => (2, 4),
        Species::Boar => (1, 2),
        // Still two at the least, because one wolf does not come in --
        // see the server's `think_hunter`. A pack of one is not a pack,
        // and shrinking this to a single wolf would quietly delete the
        // only animal in the world that opens the exchange.
        //
        // **Three to six, and a pack is a pack.** Two wolves were the
        // smallest thing the pack rules could fire on and they were what a
        // player met: a pair, never a pack. What a pack of five *does* is
        // different -- two stalk round you while the rest wait for your
        // back (`logic::animals::think_hunter`) -- and what a player does
        // about it is the fire, the wall at their back, or not being out.
        // It is spawned whole against the land's hard cap and not against
        // the per-player three (`MAX_GROUP`), and it is rare: a wolf is one
        // spawn in ten by day outside a wood.
        Species::Wolf => (3, 6),
        // Three, and no longer up to six. A flock is still the largest
        // group here and still the thing that clothes you -- three
        // fleeces apiece is nine, and the dearest garment is five -- but
        // six sheep against a cap of three meant a player who found sheep
        // found nothing else for as long as they stayed.
        Species::Sheep => (3, 3),
        // Alone, always. A pair of bears is two players' worth
        // of trouble in one clearing, and one is already the
        // most dangerous thing here.
        Species::Bear => (1, 1),
        // A covey. Two or three rather than up to five: birds now have a
        // nest to sit round (`Species::Fowl`), so a covey is what is
        // *at* one tree rather than what fills a meadow.
        Species::Fowl => (2, 3),
        // Herds on open grass, a little bigger than the deer's: the whole of
        // a zebra's safety is the herd round it (`Species::hides_in_cover`).
        Species::Zebra => (3, 5),
        Species::Antelope => (3, 5),
        // Alone or a pair. A lion does not need a second to come at you
        // (`Species::needs_company`), so a pair is not a precondition, it
        // is a flank (`Species::flanks`) -- and three would be a pride,
        // which against a cap of three is every animal a player has.
        Species::Lion => (1, 2),
        // **A school is a school**, and against its own cap
        // (`MAX_FISH_PER_PLAYER`) rather than the land's three: one fish is
        // a fish, and what makes the sea read as alive is a knot of them
        // turning together. A cod is alone.
        Species::Fish => (3, 5),
        Species::Cod => (1, 1),
        Species::Trout => (2, 4),
        // One. See `Species::Pike`: a second pike in the same pond is the
        // same water twice.
        Species::Pike => (1, 1),
        // The biggest group of anything here, against the swimmers' own cap
        // of eight: a shoal that is three fish is a school, and what a
        // herring *is* is too many to count.
        Species::Herring => (4, 6),
        // A few together, against the shore's own allowance: a lone gull is
        // a bird that has lost its flock, and a flock is what goes up at
        // once when something comes along the beach.
        Species::Gull => (2, 3),
        // **A stallion and his mares**: three to five, the zebra's herd, with
        // the last of them the stallion that keeps the rest together (the
        // server's `stallion`). Two would be a pair, not a herd, and nothing
        // for a stallion to keep.
        Species::Horse => (3, 5),
        // **Three to five, and never one.** A lone monkey has nobody to call
        // to, and the alarm is half of what the animal is for -- see
        // `Species::Monkey`. It is the horse's herd for the horse's reason
        // and for one more: a troop robbing a camp has to be more than a
        // player can keep an eye on at once, or guarding is just watching.
        Species::Monkey => (3, 5),
        // One to four. A crab is not a group; what makes a beach crawl at
        // night is the spawn weight (`spawn_weight`), not the party size,
        // and those are deliberately separate numbers -- a party of eight
        // would arrive in a ring like a herd rather than being found one at
        // a time under different stones.
        Species::Crab => (1, 4),
    }
}

/// How many soaring birds one player keeps near them.
///
/// **A count of its own, for the fish's reason.** The land's three is "a
/// meeting is an event"; one gull over a coast is an empty coast, and a
/// flock is the smallest thing that reads as a shore. Sharing the land's
/// three would have made the flock cost a player the deer behind the dunes,
/// and a beach backed by a meadow should have both. Four is a flock and a
/// straggler, and there is none of it anywhere but the coast.
pub const MAX_SEABIRDS_PER_PLAYER: usize = 4;

/// How many swimming animals one player keeps near them.
///
/// **Its own number rather than a share of `MAX_ANIMALS_PER_PLAYER`**, and
/// the two caps are two decisions. The land's three is "a meeting is an
/// event"; under the sea a single fish is not an event, it is an empty sea
/// with one fish in it, and a school is the smallest thing that reads as
/// life. Sharing the land's cap would have made every school cost a player
/// every deer they were going to meet. Eight is two schools or a school and
/// a cod, and they only exist where there is water deep enough to swim in,
/// which on most walks is none.
pub const MAX_FISH_PER_PLAYER: usize = 8;

/// The hard cap on swimming animals in a world, for `MAX_ANIMALS`'s reason.
pub const MAX_FISH: usize = 60;

/// The most animals that arrive together, of any kind.
///
/// **A group is spawned whole, and the per-player cap decides only whether a
/// new one may start.** The cap used to be checked before every member, so a
/// group could never be bigger than three and a wolf pack was always a
/// pair -- the pack rules had nothing to be about. Now a group arrives as
/// what it is and nothing else arrives until the player's allowance is free
/// again, which keeps "a meeting is an event" (one herd is one meeting) at
/// the price of a player near a big herd seeing only that herd. Six is a
/// pack's upper end, and the number the entity list pays for one meeting.
pub const MAX_GROUP: usize = 6;

/// How a species lives with its own kind: what the server's herd, pack and
/// flee rules read.
///
/// Five shapes rather than a `bool`, because the rules differ in kind and
/// not in degree: a herd follows one animal and keeps a body's length
/// apart; a pack hunts together and keeps within a stalk of each other; a
/// loose group is animals of a kind that happen to share a meadow; the bear
/// has nobody; a school is the sea's own business (`think_fish`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grouping {
    Herd,
    Pack,
    Loose,
    Alone,
    School,
}

/// How many of this species' class one player may have near them.
#[inline]
pub fn per_player_cap(species: Species) -> usize {
    if species.swims() {
        MAX_FISH_PER_PLAYER
    } else if species.soars() {
        MAX_SEABIRDS_PER_PLAYER
    } else {
        MAX_ANIMALS_PER_PLAYER
    }
}

/// The hard cap on animals in a world, whatever the player count.
///
/// Sixty, halved with the per-player cap. It is a ceiling for a crowded
/// server rather than a number a single player ever meets -- one person
/// is held to three by `MAX_ANIMALS_PER_PLAYER` long before this -- and
/// it is what keeps the entity list, and the position updates that go
/// out with it, bounded when twenty people are online in one valley.
pub const MAX_ANIMALS: usize = 60;

/// The ceiling on **kept** animals in a world -- tame ones, and the young
/// born to them -- counted apart from the wild.
///
/// ## The rule
///
/// The wild's caps (`MAX_ANIMALS`, `MAX_ANIMALS_PER_PLAYER`) count the wild
/// and nothing else, and the kept have this ceiling of their own. A tame
/// animal is a thing a player made, like a wall; it is not the world's
/// share of deer.
///
/// **They were one count, and a herd emptied the world.** A shepherd with
/// three ewes in a pen had used the whole of their three-animal allowance,
/// and nothing wild ever arrived near them again -- no deer to hunt, no wolf
/// at the fold, no horse herd to go and catch -- and on a server sixty kept
/// animals would have done the same to everybody. What keeping a flock
/// should cost is the grass and the grain, not the hunting.
///
/// Still a ceiling, because the kept are entities like the wild ones: every
/// one is in the snapshot every player is sent, and a flock that bred
/// without end would be a snapshot that grew without end. Sixty is the
/// wild's own hard cap, so the list can at most double; past it the ewes
/// are simply barren until somebody eats a sheep.
pub const MAX_KEPT: usize = 60;

/// Beyond this many blocks from every player, an animal is forgotten.
///
/// Comfortably past the interest radius, so nothing is despawned while
/// somebody can still see it -- an animal that pops out of existence at
/// the edge of vision is worse than one that is never there.
pub const DESPAWN_DISTANCE: f32 = 96.0;

// ---- what an animal can hide in, and what it can hide behind ----
//
// Two predicates about blocks, here rather than in `types` because they
// are an animal's opinion and not a fact about the block: a tuft of
// grass is cover to a hare and nothing to a deer, and a pane of leaves
// that light passes through is a wall to a pair of eyes. The server
// steers by them (`logic::animals::cover_heading`, `sees`); a client
// that ever wanted to draw "the deer has lost you" would need the same
// two answers, which is why they are shared and pure.

/// Something a fleeing animal makes for: a trunk, a canopy, tall grass.
///
/// **The list, not `is_foliage`.** Foliage is what a grazing animal puts
/// its head down for -- a berry bush, a flower, reeds -- and none of it
/// hides a deer. What hides one is a wood, and a wood is trunks and
/// leaves. Tall grass is on the list for the one animal short enough to
/// vanish in it; `blocks_sight` is where that height is asked about.
#[inline]
pub fn is_cover(block: BlockId) -> bool {
    use crate::types::{
        block_kind, BLOCK_ACACIA_LEAVES, BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_LEAVES,
        BLOCK_LOG, BLOCK_TALL_GRASS,
    };
    // Every wood's trunk and crown (`wood`), and the oak's other crowns.
    crate::wood::is_log(block)
        || crate::wood::is_wood_leaves(block)
        || matches!(
            block_kind(block),
            BLOCK_LEAVES
                | BLOCK_BIRCH_LEAVES
                | BLOCK_ACACIA_LEAVES
                | crate::types::BLOCK_MAPLE_LEAVES
                | BLOCK_LOG
                | BLOCK_BIRCH_LOG
                | BLOCK_TALL_GRASS
                // **...and the boulders, which are cobble and granite.** The
                // boulder's own note in `worldgen` has said since the day it
                // was written that what a boulder is *for* is that "a
                // hillside with a few of them has cover in a way a smooth one
                // does not" -- and until the crab arrived nothing read it
                // that way, because every animal that hid hid in a wood. A
                // crab on a beach has no wood and no distance
                // (`hides_in_cover`): the stone *is* its escape.
                //
                // **`BLOCK_STONE` is deliberately not here**, and the
                // difference is not pedantry: cobble and granite are what the
                // generator lays loose on a slope and on the sea floor, and
                // plain stone is the face of the ground itself. A deer that
                // made for the nearest rock face would be a deer that runs
                // into corners, which
                // `a_wood_is_cover_and_a_wall_of_leaves_hides_a_deer_but_grass_hides_only_a_hare`
                // has said since long before this.
                | crate::types::BLOCK_COBBLESTONE
                | crate::types::BLOCK_GRANITE
        )
}

/// Does this block stop a line of sight -- for an eye at `eye_height`
/// blocks off the ground?
///
/// **Not `is_opaque`.** Opaque is the lighting's word and it means "lets
/// no light through", which leaves are deliberately not (a canopy that
/// blacked out the ground under it would be a cave). What an eye asks
/// is whether the cell is *full*: a leaf block is a wall of leaves
/// whatever the light meter says, and a log is a log. So this is
/// `is_opaque` *or* a collidable block that fills its cell -- and never
/// water, which is neither.
///
/// Tall grass counts only for an eye under a block off the ground: a
/// hare's whole head is inside the tuft, and a deer looks over it. This
/// is what makes grass a hiding place for one animal and scenery for
/// the other, which is the difference between the two hunts.
#[inline]
pub fn blocks_sight(block: BlockId, eye_height: f32) -> bool {
    use crate::types::{block_kind, has_full_top, is_leafy, is_opaque, BLOCK_TALL_GRASS};
    // **Foliage hides things, and it has to say so itself now.** Leaves
    // are not opaque -- they are a cutout, drawn with holes -- so this
    // rule leant on `has_full_top`, which was true of them only because
    // they were solid. The moment a player was allowed to push through
    // a thicket (`types::is_collidable`) that stopped being true, and a
    // deer that made for the trees could be watched through them: the
    // whole flight mechanic went, silently, from a change about walking.
    if is_opaque(block) || has_full_top(block) || is_leafy(block) {
        return true;
    }
    eye_height < 1.0 && block_kind(block) == BLOCK_TALL_GRASS
}

// ---- butchering ----
//
// The decision is here, in the shared crate, and the *doing* is on the
// server (`primitive_server::lib::use_block`): which cut a click makes,
// what it yields and what the cell holds afterwards is a pure function
// of the block and the thing in hand, and a pure function is what a
// test can hold still. The server version of this used to be the only
// version, and "use_block cannot be exercised without a socket" was
// the reason the fire-lighting rules went a release without a test.

/// The stage a carcass is at: how many cuts have already been made.
///
/// Read out of the variant field. Zero -- every id the kill path writes
/// -- means nothing has been taken yet.
#[inline]
pub fn butchering_stage(block: BlockId) -> usize {
    ((block & crate::types::VARIANT_MASK) >> crate::types::VARIANT_SHIFT) as usize
}

/// The carcass of this species with `stage` cuts already made.
///
/// Three bits hold eight stages and the longest sequence (the sheep's)
/// is five, which a test below keeps true: a sixth cut on a sheep would
/// silently wrap the field and put a whole sheep back on the ground.
///
/// Air for a species that leaves no carcass (see `Species::carcass`):
/// every caller writes this into a cell, and air is the honest thing to
/// write for a fish. `lay_carcass` asks `carcass` first and never gets here
/// with one.
#[inline]
pub fn carcass_at_stage(species: Species, stage: usize) -> BlockId {
    species
        .carcass()
        .map_or(crate::types::BLOCK_AIR, |body| body | ((stage as BlockId) << crate::types::VARIANT_SHIFT))
}

/// How many cuts of flesh a dead player's body gives before only the bones are
/// left. See `cut_body`.
pub const BODY_CUTS: usize = 3;

/// What one click on a dead player's body does with `held` in hand, or `None`
/// if the block is not a body.
///
/// **A knife and only a knife.** An axe on a carcass tears the hide and still
/// takes the meat; on a person it is not butchery, and the game has no reason
/// to make that easier than it has to be. Each cut takes two pieces of flesh
/// (`types::BLOCK_HUMAN_FLESH`) and counts in the body's variant field; the
/// last leaves the bones (`types::BLOCK_REMAINS`) -- and the bones are the same
/// container the body was, so whatever the dead player carried is still there
/// to take. A body that has rotted to bones is not cut at all: there is nothing
/// left on it.
pub fn cut_body(block: BlockId, held: Option<BlockId>) -> Option<Butchered> {
    use crate::types::{block_kind, is_knife, BLOCK_CORPSE, BLOCK_HUMAN_FLESH, BLOCK_REMAINS, VARIANT_MASK, VARIANT_SHIFT};
    if block_kind(block) != BLOCK_CORPSE {
        return None;
    }
    if !held.is_some_and(is_knife) {
        return Some(Butchered::NeedsATool);
    }
    let stage = butchering_stage(block);
    let next = if stage + 1 >= BODY_CUTS {
        BLOCK_REMAINS
    } else {
        (block & !VARIANT_MASK) | (((stage + 1) as BlockId) << VARIANT_SHIFT)
    };
    Some(Butchered::Cut { took: Some((BLOCK_HUMAN_FLESH, 2)), next })
}

/// What one click on a carcass does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Butchered {
    /// A cut was made. `took` is what came off it -- `None` when the
    /// tool ruined that cut -- and `next` is what the cell holds now:
    /// the same carcass one stage on, or air after the last cut.
    Cut {
        took: Option<(BlockId, u32)>,
        next: BlockId,
    },
    /// A bare hand on a carcass. Nothing comes off, and the player is
    /// told why rather than left clicking at a heap that ignores them.
    NeedsATool,
}

/// Decides one click on `block` with `held` in hand. `None` if the
/// block is not a carcass at all, so the caller can fall through to
/// whatever else a right click means.
///
/// **The tool is the decision.** A knife makes every cut in
/// `Species::butchering` order. Any other tool -- an axe, a pick, a
/// shovel, a hoe -- takes the carcass apart too, but *ruins the skin*:
/// the hide or fleece cut yields nothing, and the sinew, bone and meat
/// behind it come off as they would for a knife. A bare hand does
/// nothing. Three answers rather than "knife or nothing" because the
/// point of the carcass was to give the hunt a decision after the kill:
/// a player who has run a deer down with an axe in hand gets dinner
/// tonight and no hide, and the question of whether to walk home for
/// the knife is a real one. A rule that let the axe do everything would
/// make the knife a stat; a rule that let it do nothing would make the
/// carcass a locked chest.
///
/// A stage past the end of the list -- which nothing writes, but a save
/// from a build with a longer list could -- reads as "done": the cell
/// is cleared and nothing comes off, rather than indexing off the end
/// or leaving a block in the world that no cut can remove.
pub fn butcher(block: BlockId, held: Option<BlockId>) -> Option<Butchered> {
    use crate::blocks::{definition, Tier};
    use crate::types::{is_implement, is_knife, BLOCK_AIR};

    let species = Species::of_carcass(block)?;
    let stage = butchering_stage(block);
    let cuts = species.butchering();

    let Some(tool) = held else {
        return Some(Butchered::NeedsATool);
    };
    let knife = is_knife(tool);
    // `tool` is `Hand` for a lump of dirt as well as for an empty hand,
    // and it is right to be: a rock held in the fist is a fist. The hoe
    // is the exception the second test is for -- it has no tier at all
    // (see `is_implement`), and it is still an edge on a stick, which
    // is exactly the thing that tears a skin.
    let bladed = definition(tool).tool.unwrap_or(Tier::Hand) != Tier::Hand || is_implement(tool);
    if !knife && !bladed {
        return Some(Butchered::NeedsATool);
    }

    let next = if stage + 1 >= cuts.len() {
        BLOCK_AIR
    } else {
        carcass_at_stage(species, stage + 1)
    };
    // **A blunt knife is a knife that tears.** At the last step of its edge
    // (`tools::BLUNTEST`) it saws at a hide instead of parting it from the
    // flesh, and the skin comes off in rags -- the axe's answer. At *dull*,
    // the step before, the skin still comes whole and the meat comes off
    // ragged: a cut of flesh a piece short. So a hunter who has worked their
    // knife blunt far from the stone has the same choice the axe gives:
    // dinner now, or the hide later. Rejected: *slower cuts*, which is what
    // the edge does to digging -- a carcass is one click a cut, and a click
    // that took longer would be a wait and not a decision.
    let step = crate::tools::blunt_step(tool);
    let knife = knife && step < crate::tools::BLUNTEST;
    let took = cuts.get(stage).copied().map(|(what, count)| {
        let skin = matches!(what, BLOCK_HIDE | BLOCK_WOOL | BLOCK_PELT | BLOCK_BEAR_HIDE | BLOCK_FEATHER);
        if step >= 2 && !skin && count > 1 {
            (what, count - 1)
        } else {
            (what, count)
        }
    });
    let took = took.filter(|&(what, _)| {
        // The skin is the first cut -- the fleece too, on a sheep, and
        // the skin under it -- and it is what a blade that is not a
        // knife tears rather than takes. Decided by *what the cut is*
        // rather than by its index, so a species whose list started
        // with something else would not lose that instead.
        // Every skin, not only the deer's: a pelt tears the same way a
        // hide does, and a bear's more easily than either. Listed by
        // what the thing *is* rather than by which animal gave it, so
        // a sixth skin is one name here and nothing else.
        knife
            || !matches!(
                what,
                BLOCK_HIDE | BLOCK_WOOL | BLOCK_PELT | BLOCK_BEAR_HIDE | BLOCK_FEATHER
            )
    });
    Some(Butchered::Cut { took, next })
}

#[cfg(test)]
mod tests {

    #[test]
    fn breaking_a_body_gives_nothing_and_the_knife_gives_everything() {
        // **The rule that makes butchering the only way to eat what you
        // killed.** A carcass used to come apart under the fist for one
        // cut of meat, and that single line undid the whole mechanism:
        // the blade, the order of the cuts and the choice between
        // hide-first and meat-now were an option for players who felt
        // like doing it properly, and punching the deer was faster.
        //
        // Every body in the world now drops nothing when it is broken,
        // and every body still gives its full table under a knife.
        for species in carcassed() {
            let body = species.carcass().expect("carcassed");
            assert!(
                crate::types::block_drop(body).is_none(),
                "breaking a {} still pays",
                species.name()
            );
            assert!(
                !species.butchering().is_empty(),
                "{} gives nothing to a knife either, which leaves no way to use it",
                species.name()
            );
            // ...and it is slow enough that nobody destroys one by
            // accident with a stray swing.
            let seconds = crate::types::break_seconds(body).expect("a body can be cleared away");
            assert!(
                seconds >= 1.0,
                "a {} comes apart in {seconds}s, which is one careless click",
                species.name()
            );
        }
    }

    use super::*;

    /// Every species that dies into a carcass -- which is every species
    /// but the two that swim (`Species::carcass`). The butchery tests are
    /// sentences about a body on the ground, and a fish is not one.
    fn carcassed() -> impl Iterator<Item = Species> {
        Species::ALL.iter().copied().filter(|s| s.carcass().is_some())
    }

    /// Runs a carcass all the way down with one tool, the way the
    /// server does it: click, read what came off, click the block that
    /// is left. Returns everything that came off, in order.
    fn butcher_all(species: Species, held: Option<BlockId>) -> Vec<(BlockId, u32)> {
        let mut block = carcass_at_stage(species, 0);
        let mut taken = Vec::new();
        // One more click than the longest list, so an endless carcass
        // fails loudly here instead of spinning.
        for _ in 0..=8 {
            match butcher(block, held) {
                Some(Butchered::Cut { took, next }) => {
                    taken.extend(took);
                    if crate::types::is_air(next) {
                        return taken;
                    }
                    block = next;
                }
                Some(Butchered::NeedsATool) => return taken,
                None => panic!("{} stopped being a carcass mid-way", species.name()),
            }
        }
        panic!("{} never came apart", species.name());
    }

    #[test]
    fn a_blunt_knife_tears_the_skin_and_a_dull_one_takes_less_meat() {
        use crate::tools::{with_edge, BLUNTEST};
        use crate::types::BLOCK_COPPER_KNIFE;
        let count = |taken: &[(BlockId, u32)], what: BlockId| taken.iter().filter(|t| t.0 == what).map(|t| t.1).sum::<u32>();
        let sharp = butcher_all(Species::Deer, Some(BLOCK_COPPER_KNIFE));
        let dull = butcher_all(Species::Deer, Some(with_edge(BLOCK_COPPER_KNIFE, 2)));
        let blunt = butcher_all(Species::Deer, Some(with_edge(BLOCK_COPPER_KNIFE, BLUNTEST)));
        assert!(count(&sharp, BLOCK_HIDE) > 0, "a sharp knife lost the hide");
        assert_eq!(count(&dull, BLOCK_HIDE), count(&sharp, BLOCK_HIDE), "a dull knife tore the hide");
        assert_eq!(count(&blunt, BLOCK_HIDE), 0, "a blunt knife skinned a deer as well as a sharp one");
        let meat = |taken: &[(BlockId, u32)]| taken.iter().filter(|t| t.0 != BLOCK_HIDE).map(|t| t.1).sum::<u32>();
        assert!(meat(&dull) < meat(&sharp), "a dull knife took as much off the deer as a sharp one");
        assert!(meat(&dull) > 0, "a dull knife took nothing at all");
    }

    #[test]
    fn a_knife_takes_a_carcass_apart_in_the_order_a_butcher_works() {
        use crate::types::BLOCK_FLINT_KNIFE;
        for species in carcassed() {
            let taken = butcher_all(species, Some(BLOCK_FLINT_KNIFE));
            assert_eq!(taken, species.butchering(), "{}", species.name());
            // ...and each cut moves the stage on by exactly one, so the
            // block in the world is always the carcass it was.
            let first = butcher(carcass_at_stage(species, 0), Some(BLOCK_FLINT_KNIFE));
            assert_eq!(
                first,
                Some(Butchered::Cut {
                    took: Some(species.butchering()[0]),
                    next: carcass_at_stage(species, 1),
                }),
                "{}",
                species.name()
            );
            assert_eq!(Species::of_carcass(carcass_at_stage(species, 1)), Some(species));
        }
        // Every knife is the same knife here: the tier buys speed on
        // the hunt, not more meat off the table.
        for knife in [
            crate::types::BLOCK_COPPER_KNIFE,
            crate::types::BLOCK_BRONZE_KNIFE,
            crate::types::BLOCK_IRON_KNIFE,
        ] {
            assert_eq!(butcher_all(Species::Deer, Some(knife)), Species::Deer.butchering());
        }
    }

    #[test]
    fn an_axe_ruins_the_skin_and_gets_the_rest() {
        use crate::types::{BLOCK_STONE_AXE, BLOCK_STONE_PICKAXE, BLOCK_HOE};
        for tool in [BLOCK_STONE_AXE, BLOCK_STONE_PICKAXE, BLOCK_HOE] {
            for species in carcassed() {
                let taken = butcher_all(species, Some(tool));
                // Every skin and the feathers: the list is the one in
                // `butcher`, and the two have to be the same list or
                // the test is checking a rule of its own.
                let ruined = |what: BlockId| {
                    matches!(
                        what,
                        BLOCK_HIDE | BLOCK_WOOL | BLOCK_PELT | BLOCK_BEAR_HIDE | BLOCK_FEATHER
                    )
                };
                let wanted: Vec<(BlockId, u32)> = species
                    .butchering()
                    .iter()
                    .copied()
                    .filter(|&(what, _)| !ruined(what))
                    .collect();
                assert_eq!(taken, wanted, "{} with {}", species.name(), tool);
                assert!(
                    taken.iter().any(|&(what, _)| crate::food::is_food(what)),
                    "{}: the axe got no dinner",
                    species.name()
                );
                assert!(
                    !taken.iter().any(|&(what, _)| ruined(what)),
                    "{}: the axe took the skin",
                    species.name()
                );
            }
        }
        // The sheep loses both the fleece and the skin under it: an axe
        // through a sheep is an axe through wool.
        let sheep = butcher_all(Species::Sheep, Some(BLOCK_STONE_AXE));
        assert!(!sheep.iter().any(|&(what, _)| what == BLOCK_WOOL));
    }

    #[test]
    fn bare_hands_take_nothing_off_a_carcass() {
        use crate::types::{BLOCK_DIRT, BLOCK_FLINT};
        for species in carcassed() {
            let body = carcass_at_stage(species, 0);
            assert_eq!(butcher(body, None), Some(Butchered::NeedsATool));
            // A rock in the fist is a fist, and so is a nodule of flint
            // that has not been knapped into anything.
            assert_eq!(butcher(body, Some(BLOCK_DIRT)), Some(Butchered::NeedsATool));
            assert_eq!(butcher(body, Some(BLOCK_FLINT)), Some(Butchered::NeedsATool));
            // ...and at every stage, not only the first.
            for stage in 1..species.butchering().len() {
                assert_eq!(
                    butcher(carcass_at_stage(species, stage), None),
                    Some(Butchered::NeedsATool)
                );
            }
        }
        // Something that is not a carcass is not this function's
        // business at all.
        assert_eq!(butcher(BLOCK_DIRT, Some(crate::types::BLOCK_FLINT_KNIFE)), None);
    }

    #[test]
    fn the_last_cut_takes_the_carcass_away() {
        use crate::types::{is_air, BLOCK_FLINT_KNIFE};
        for species in carcassed() {
            let last = species.butchering().len() - 1;
            let block = carcass_at_stage(species, last);
            match butcher(block, Some(BLOCK_FLINT_KNIFE)) {
                Some(Butchered::Cut { took, next }) => {
                    assert_eq!(took, Some(species.butchering()[last]));
                    assert!(is_air(next), "{} left something behind", species.name());
                }
                other => panic!("{}: {other:?}", species.name()),
            }
            // A stage nothing writes -- past the end of the list -- is
            // cleared rather than left in the world forever.
            let stale = carcass_at_stage(species, 7);
            assert_eq!(
                butcher(stale, Some(BLOCK_FLINT_KNIFE)),
                Some(Butchered::Cut {
                    took: None,
                    next: crate::types::BLOCK_AIR
                })
            );
        }
    }

    #[test]
    fn every_stage_of_every_carcass_fits_in_the_variant_field() {
        // Three bits, eight values, and the sheep has the longest list.
        // A list of nine would wrap the field on the last cut and put a
        // whole carcass back on the ground.
        for species in carcassed() {
            let cuts = species.butchering().len();
            assert!(cuts >= 2, "{} comes apart in one piece", species.name());
            let room = (crate::types::VARIANT_MASK >> crate::types::VARIANT_SHIFT) as usize + 1;
            assert!(
                cuts <= room,
                "{} needs {cuts} stages and the field holds {room}",
                species.name()
            );
            for stage in 0..cuts {
                let block = carcass_at_stage(species, stage);
                assert_eq!(butchering_stage(block), stage);
                assert_eq!(Species::of_carcass(block), Some(species));
                assert!(crate::types::is_known_block(block), "{} stage {stage}", species.name());
            }
        }
    }

    #[test]
    fn a_sheep_is_the_only_thing_carrying_wool() {
        // The point of the animal. If anything else dropped wool, the
        // flock would be a convenience rather than a reason to go
        // looking -- and the whole clothing chain would have a second
        // entrance nobody designed.
        for &species in Species::ALL {
            let carries_wool = species
                .drops()
                .iter()
                .any(|(block, _)| *block == BLOCK_WOOL);
            assert_eq!(
                carries_wool,
                species == Species::Sheep,
                "{} and wool disagree",
                species.name(),
            );
        }
    }

    #[test]
    fn a_sheep_cannot_outrun_a_walking_player() {
        // **The animal is a resource, not a hunt.** Everything else out
        // there is either a chase or a threat; catching this one is
        // meant to be a decision about carrying capacity rather than
        // about stamina. A sheep that had to be run down would put the
        // warm clothes behind the same skill gate the meat is behind,
        // and being cold is a much earlier problem than being hungry.
        //
        // Against the *walk*, not the sprint: a player who has to
        // sprint at a sheep is a player who can be outrun by one while
        // exhausted.
        const NOMINAL_WALK: f32 = NOMINAL_SPRINT_SPEED / 1.6;
        assert!(
            Species::Sheep.run_speed() < NOMINAL_WALK,
            "a sheep runs at {:.1} and a player walks at {NOMINAL_WALK:.1}",
            Species::Sheep.run_speed(),
        );
    }

    #[test]
    fn a_sheep_never_fights_back() {
        // It has no damage, no provocation range and no memory of being
        // hit. All three, because any one of them left on would make a
        // flock something a player has to be careful around -- and a
        // dangerous sheep is a joke the second time and a bug the
        // first.
        assert_eq!(Species::Sheep.damage(), 0.0);
        assert_eq!(Species::Sheep.provoke_range(), 0.0);
        assert_eq!(Species::Sheep.grudge_seconds(), 0.0);
        assert!(!Species::Sheep.is_hostile());
    }

    #[test]
    fn a_flock_is_worth_walking_across_a_field_for() {
        // One sheep is not a coat and should not be. What the group
        // size and the drop have to add up to is: finding a flock is
        // finding enough wool to dress with, so the walk was worth it.
        let (fewest, _) = group_size(Species::Sheep);
        let per_animal: u32 = Species::Sheep
            .drops()
            .iter()
            .filter(|(block, _)| *block == BLOCK_WOOL)
            .map(|(_, count)| count)
            .sum();
        // A tunic is the dearest of the four garments at five fleeces;
        // the smallest flock has to cover at least that.
        assert!(
            fewest * per_animal >= 5,
            "the smallest flock yields {} wool, which does not clothe anybody",
            fewest * per_animal,
        );
    }

    #[test]
    fn every_species_is_a_complete_row() {
        for &species in Species::ALL {
            assert!(!species.name().is_empty());
            assert!(species.height() > 0.0, "{}", species.name());
            assert!(species.width() > 0.0 && species.width() < 1.0, "{}", species.name());
            assert!(species.health() > 0.0, "{}", species.name());
            assert!(
                species.run_speed() > species.walk_speed(),
                "{} does not run faster than it walks",
                species.name()
            );
            assert!(species.awareness() > 0.0, "{} never notices anything", species.name());
            assert!(!species.drops().is_empty(), "{} drops nothing", species.name());
            for &(block, count) in species.drops() {
                assert!(count > 0, "{} drops zero of something", species.name());
                assert!(
                    crate::types::is_known_block(block),
                    "{} drops an unknown block",
                    species.name()
                );
            }
        }
    }

    #[test]
    fn the_four_that_fight_back_are_dangerous_in_four_different_ways() {
        // Everything that ran would make hunting one verb; everything
        // that charged would make a walk in the woods a fight. Four of
        // ten, and each of them is a different sentence -- if two ever
        // collapse into one, one of them is a picture rather than an
        // animal. It was three of seven until the savanna had a hunter of
        // its own: the list grew by one and the property did not bend.
        let hostile: Vec<&str> = Species::ALL
            .iter()
            .filter(|s| s.is_hostile())
            .map(|s| s.name())
            .collect();
        assert_eq!(hostile, ["boar", "wolf", "bear", "lion"]);
        // ...and the lion is the fourth sentence: the wolf's hunt without
        // the wolf's arithmetic. It comes alone where a wolf waits for a
        // second, it bites harder than a boar and softer than a bear, and
        // -- the bear's number, not granted -- a sprint still gets you
        // away from it.
        assert!(Species::Wolf.needs_company() && !Species::Lion.needs_company());
        assert!(Species::Lion.damage() > Species::Boar.damage());
        assert!(Species::Lion.damage() < Species::Bear.damage());
        assert!(Species::Lion.run_speed() < NOMINAL_SPRINT_SPEED);
        assert!(Species::Lion.is_predator(), "a lion that hunts nothing is a boar with a mane");
        // The boar hits harder than the wolf and the wolf reaches
        // further: one is a thing you blunder into and the other is a
        // thing that arrives.
        assert!(Species::Boar.damage() > Species::Wolf.damage());
        assert!(Species::Wolf.provoke_range() > Species::Boar.provoke_range());
        assert!(Species::Wolf.awareness() > Species::Boar.awareness());
        // ...and the bear is the third sentence: it hits hardest, it
        // holds a grudge longest, and -- the number that matters -- it
        // is the only thing here that a running player cannot leave.
        assert!(Species::Bear.damage() > Species::Boar.damage());
        assert!(Species::Bear.grudge_seconds() > Species::Wolf.grudge_seconds());
        assert!(
            Species::Bear.run_speed() > Species::Boar.run_speed(),
            "a bear slower than a boar is a boar with a bigger picture"
        );
    }

    /// **What the hide is worth, stated as the fight it makes.** The
    /// mechanic the player asked for: a huge boar should not go down to
    /// bare hands. See `Species::hide_armour`.
    #[test]
    fn bare_hands_are_hopeless_against_a_big_animal_and_fine_against_a_small_one() {
        // The three numbers `primitive_server::hunting_damage` produces
        // for a fist, a flint knife and a flint spear. Written out
        // rather than imported: this crate cannot see the server, and
        // the point of the test is the *ratio*, which would survive a
        // change to either side.
        let (fist, knife, spear) = (1.5, 4.2, 8.6);
        let blows = |species: Species, damage: f32| species.health() / species.hurt_by(damage);

        // **A ratio, and it has to be one since `TOUGHNESS`.** This said
        // "a hare should still be three punches", and three punches is
        // not what the hide decides -- five times the health is thirteen
        // punches for a hare and five times as many for everything else,
        // and the sentence the hide is actually making survives that
        // untouched: hands are for the small animals and useless on the
        // big ones. Stated as the gap between the two, the absolute
        // counts are pinned separately in
        // `what_five_times_the_health_costs_in_blows`, which is where a
        // hunt that has become unplayably long shows up.
        assert!(Species::Hare.hurt_by(fist) >= fist);
        let hare = blows(Species::Hare, fist);
        let boar = blows(Species::Boar, fist);
        assert!(
            boar > 30.0 * hare,
            "a boar is {boar:.0} punches against a hare's {hare:.0}, which is the same animal twice"
        );

        // A boar is not for fists. A hundreds-of-punches figure is the
        // game saying "not like this" without ever refusing the swing.
        assert!(boar > 100.0, "a boar goes down in {boar:.0} punches");
        // ...and a knife or a spear is the answer, in that order, each a
        // real step and not a rounding.
        assert!(blows(Species::Boar, spear) * 2.0 < blows(Species::Boar, knife));
        assert!(blows(Species::Boar, knife) * 2.0 < boar);

        // A bear cannot be punched to death at all, and can be speared --
        // and it is a longer fight than a boar with the same spear, which
        // is the hide doing its job on the biggest animal in the world.
        assert_eq!(Species::Bear.hurt_by(fist), 0.0, "a bear went down to fists");
        assert!(Species::Bear.hurt_by(spear) > 5.0);
        assert!(blows(Species::Bear, spear) > blows(Species::Boar, spear));
    }

    /// **What five times the health costs, in blows.** The player asked
    /// for it -- "сделай животным в 5 раз больше здоровья" -- and this is
    /// the bill, written down where a change to either side of it fails
    /// the build rather than the evening.
    ///
    /// Every number here is a *count of landed hits*, which is the thing
    /// a player actually experiences; `TOUGHNESS` is asserted beside them
    /// so that moving it without looking at them is impossible.
    ///
    /// **It was a warning, and the warning was taken.** Written first with
    /// the health alone raised, this fixture said thirty-four spear thrusts
    /// into a bear -- out of the seventy a flint spear survives (`blocks`,
    /// its `durability`), half a spear for one animal, and therefore an
    /// animal you always walk away from. [`WEAPON_BITE`] is the answer, and
    /// the counts below are what it bought: the hunt costs twice what it
    /// did before the health went up, not five times.
    #[test]
    fn what_five_times_the_health_costs_in_blows() {
        let (fist, knife, spear) = (1.5, 4.2, 8.6);
        let blows =
            |species: Species, damage: f32| (species.health() / species.hurt_by(damage)).ceil() as u32;

        assert_eq!(TOUGHNESS, 5.0, "the bill below was drawn up against five");
        assert_eq!(WEAPON_BITE, 2.5, "...and against a blow worth two and a half");

        // The first morning: a hare with bare hands, and with the first
        // knife that gets knapped.
        assert_eq!(blows(Species::Hare, fist), 6);
        assert_eq!(blows(Species::Hare, knife), 2);
        assert_eq!(blows(Species::Fowl, knife), 2);

        // The spear, on the three animals it exists for.
        assert_eq!(blows(Species::Deer, spear), 3);
        assert_eq!(blows(Species::Boar, spear), 4);
        assert_eq!(blows(Species::Bear, spear), 14);

        // Shearing a flock is still a job rather than a walk, and it is
        // a longer job than it was.
        assert_eq!(blows(Species::Sheep, knife), 5);

        // **And the hide still says what it said.** A boar is beaten to
        // death with fists in two hundred and eighty punches, which is
        // the answer "not like this" -- see `hide_armour`.
        assert_eq!(blows(Species::Boar, fist), 280);
    }

    #[test]
    fn nothing_worth_eating_can_be_run_down_in_a_straight_line() {
        // **Both of them are over a sprint now, and that is the
        // mechanic.** The deer used to be under one, which made the
        // answer to a deer "hold shift until it tires" -- a chase with a
        // single input in it. Neither can be run down in a straight
        // line; a deer is taken by cutting it off, by the ground, or by
        // reach (`combat::SPEAR_REACH`).
        let sprint = NOMINAL_SPRINT_SPEED;
        assert!(
            Species::Hare.run_speed() > sprint,
            "a hare at {} is caught by walking after it",
            Species::Hare.run_speed()
        );
        assert!(
            Species::Deer.run_speed() > sprint,
            "a deer at {} is caught by holding the sprint key",
            Species::Deer.run_speed()
        );
        // ...and the hare is still the faster of the two, or the burst
        // animal is the easy one and the design is upside down.
        assert!(
            Species::Hare.run_speed() > Species::Deer.run_speed(),
            "a hare is meant to be the fastest thing in the meadow"
        );
        // ...and the savanna's two grazers keep the same rule: both over a
        // sprint, and both under the hare, which stays the fastest thing
        // alive anywhere.
        for grazer in [Species::Zebra, Species::Antelope] {
            assert!(
                grazer.run_speed() > sprint,
                "a {} at {} is caught by holding the sprint key",
                grazer.name(),
                grazer.run_speed()
            );
            assert!(
                Species::Hare.run_speed() > grazer.run_speed(),
                "a {} outruns the hare, which makes the hare a slower {}",
                grazer.name(),
                grazer.name()
            );
        }
    }

    #[test]
    fn a_bear_is_the_one_dangerous_thing_a_sprint_does_not_get_you_away_from() {
        // **The sentence this file has always written about the bear,
        // finally true of the number.** `run_speed` was 6.0 against a
        // sprint of 8.8, so the animal described as "the thing you run
        // from" was for a long time a thing you ran from successfully, by
        // holding shift. Everything else about it -- the grudge, the
        // wind, the hide -- was paying for a promise nothing kept.
        //
        // What the mechanic asks for instead is terrain: break the line
        // of sight, put water or a ledge between you, or do not be there.
        let sprint = NOMINAL_SPRINT_SPEED;
        assert!(
            Species::Bear.run_speed() > sprint,
            "a bear at {:.1} against a sprint of {sprint:.1} is a boar with a bigger picture",
            Species::Bear.run_speed()
        );
        // ...and it is the *only* one that is dangerous and faster than a
        // sprint. A boar or a wolf you cannot walk away from is not a
        // danger, it is a punishment -- see `run_speed`.
        for &species in Species::ALL {
            if species == Species::Bear || !species.is_hostile() {
                continue;
            }
            assert!(
                species.run_speed() < sprint,
                "a {} at {:.1} cannot be left behind either",
                species.name(),
                species.run_speed()
            );
        }
        // The hare is over a sprint too and is not a threat, and it is
        // still the fastest thing alive: the animal worth nothing is the
        // one nobody can catch, which is the joke the hare exists for.
        assert!(
            Species::Hare.run_speed() > Species::Bear.run_speed(),
            "a bear runs down a hare, which makes the hare a slower deer"
        );
    }

    #[test]
    fn everything_that_runs_is_worth_running_from_except_the_sheep() {
        // **The raise, stated as the property it was for.** The speeds
        // went up when the animals were made rare (see
        // `MAX_ANIMALS_PER_PLAYER`), because three encounters in an
        // afternoon each have to be worth the walk they interrupted --
        // and an animal a player overtakes at a stroll is not one.
        //
        // Two exceptions, and both are designed. The sheep is a
        // resource that happens to move. The rat is the other end of the
        // same idea: it is not an encounter at all, it is a thing in your
        // storeroom, and it has to be killable by somebody who has just
        // woken up and is holding a torch. A rat that had to be *chased*
        // would turn a nuisance into a game of tag round the furniture,
        // which is the chore this mechanic is most at risk of becoming.
        //
        // Three now, and the crab is the third: it cannot outwalk anybody
        // and it is not meant to. What a crab escapes into is a *stone*
        // (the server's `scuttle`), which is two blocks away and not two
        // hundred -- so the property this test states is really "nothing
        // escapes by distance unless it can", and the crab escapes by
        // arriving somewhere a hand cannot follow.
        const NOMINAL_WALK: f32 = NOMINAL_SPRINT_SPEED / 1.6;
        for &species in Species::ALL {
            assert_eq!(
                species.run_speed() > NOMINAL_WALK,
                !matches!(species, Species::Sheep | Species::Rat | Species::Crab),
                "{} runs at {:.1} against a walk of {NOMINAL_WALK:.1}",
                species.name(),
                species.run_speed(),
            );
            // ...and nothing *wandering* keeps pace with a walking
            // player, sheep included. A world where the scenery follows
            // you about at your own speed is a world that is chasing you.
            assert!(
                species.walk_speed() < NOMINAL_WALK,
                "a wandering {} keeps up with a walking person",
                species.name()
            );
        }
    }

    #[test]
    fn a_meeting_is_an_event_rather_than_a_backdrop() {
        // **What "make the animals rare" means, as a number.** Six per
        // player inside a ninety-six block bubble meant something was
        // always grazing somewhere, so meeting a deer meant nothing and
        // meeting a bear meant only that this patch of scenery bit.
        const {
            assert!(
                MAX_ANIMALS_PER_PLAYER <= 3,
                "that many animals a player is a field, not an encounter"
            )
        };
        // ...and the group sizes have to come down with it, or the first
        // flock a player walks into is the last animal they see for as
        // long as they stand there. The general rule is next door in
        // `a_group_is_a_group_and_never_bigger_than_the_pack_limit`;
        // what is here is the one that pays for it -- the flock is still
        // a coat.
        let (fewest, most) = group_size(Species::Sheep);
        assert!(most as usize <= MAX_ANIMALS_PER_PLAYER);
        let wool: u32 = Species::Sheep
            .drops()
            .iter()
            .filter(|(block, _)| *block == BLOCK_WOOL)
            .map(|(_, count)| count)
            .sum();
        assert!(
            fewest * wool >= 5,
            "the smallest flock is now {} wool, which does not clothe anybody",
            fewest * wool
        );
    }

    #[test]
    fn nothing_here_can_run_flat_out_for_ever_and_the_wolf_blows_before_the_deer() {
        // The row that turns a chase into a race rather than a
        // comparison of two speeds. Two properties, and the second is
        // the mechanic: a wolf is faster than a deer, so if it also had
        // the longer wind it would catch every deer it ever saw and the
        // meadows would empty on their own.
        for &species in Species::ALL {
            assert!(
                species.stamina_seconds() > 0.0,
                "{} can sprint for ever",
                species.name()
            );
            assert!(
                species.stamina_seconds() < 60.0,
                "{} has a minute of sprint in it, which is not a wind, it is a vehicle",
                species.name()
            );
        }
        assert!(
            Species::Wolf.stamina_seconds() < Species::Deer.stamina_seconds(),
            "a wolf that outlasts its dinner catches every deer there is"
        );
        // The same race on the savanna, where there is no wood to run for:
        // the zebra's whole escape is its wind.
        assert!(
            Species::Lion.stamina_seconds() < Species::Zebra.stamina_seconds(),
            "a lion that outlasts its dinner catches every zebra there is"
        );
        // ...and the two burst animals have the least of it. A hare is
        // faster than a sprinting player and a bird gets off the ground:
        // neither is ever run down, both are caught by being cut off,
        // and the thing that gives a player the chance is that the burst
        // ends. Anything with a hare's speed and a deer's wind would
        // simply leave.
        // ...and the pike is the third of them, under water: its whole
        // trade is the first two blocks out of the weed (`Species::Pike`).
        // ...and the rat is the fourth, for a different reason: it has
        // nowhere to run *to* but the gap in the wall, and it is never
        // more than a few blocks from one. Wind is what an animal needs
        // to cross a field, and a rat never crosses one.
        // ...and the crab is the fifth, and the shortest-winded thing
        // alive: two seconds, which is the width of a beach between one
        // stone and the next.
        for burst in [Species::Hare, Species::Fowl, Species::Pike, Species::Rat, Species::Crab] {
            assert!(
                Species::ALL
                    .iter()
                    .filter(|s| !matches!(
                        s,
                        Species::Hare | Species::Fowl | Species::Pike | Species::Rat | Species::Crab
                    ))
                    .all(|s| s.stamina_seconds() > burst.stamina_seconds()),
                "a {} has as much wind as something that has to be run down",
                burst.name()
            );
        }
        // The bear keeps its promise: nothing else can hold a run for as
        // long, which is what "you cannot outrun a bear" costs.
        assert!(
            Species::ALL
                .iter()
                .all(|s| *s == Species::Bear || s.stamina_seconds() < Species::Bear.stamina_seconds()),
            "something outlasts a bear, and a bear you can outlast is a boar"
        );
    }

    #[test]
    fn the_hard_work_is_where_the_skin_is() {
        // Leather is what the metal tools are bound with, so which
        // animals carry a skin decides what the metal age costs. A hare
        // that gave a full hide would make the bronze age a rabbit
        // farm -- so it gives a *pelt*, which cures to the same leather
        // and takes as many hares as a hare is small.
        for &species in Species::ALL {
            let skin = species
                .drops()
                .iter()
                .any(|&(b, _)| matches!(b, BLOCK_HIDE | BLOCK_PELT | BLOCK_BEAR_HIDE));
            // Two exceptions, and each says why. A bird is plucked, not
            // skinned, and feathers are not leather. A sheep gives its
            // fleece instead: a sheepskin is wool with a skin attached,
            // and giving it both would make the flock the answer to the
            // leather chain as well as the wool one.
            // ...and a fish, which has scales and gives itself (`drops`).
            // ...and every bird, the gull as much as the grouse: plucked.
            // ...and the rat, which is too small: a pelt is a hare's
            // skin and a hare is a big animal beside this. A rat pelt
            // would also be a fourth way to leather that costs nothing
            // to get, which is the one thing `drops` is most careful
            // about (see the rat's own row).
            // ...and the shore's two. A crab has a shell and no skin at
            // all. A monkey has a skin and nothing in this world tans one,
            // and that is a *decision* rather than an omission: a troop that
            // paid out leather would make the palm grove the cheapest
            // tannery on the map -- three to five animals that come to you,
            // in the open, worth two hits apiece -- and the leather chain is
            // the one price in this game that must stay a walk into the
            // country. See the rat's row for the same argument at the other
            // end of the scale.
            let expected = !(species.flies()
                || species.swims()
                || matches!(species, Species::Sheep | Species::Rat | Species::Monkey | Species::Crab));
            assert_eq!(
                skin,
                expected,
                "{} disagrees about whether it is worth skinning",
                species.name()
            );
        }
        // ...and the *full* hide -- the one a strap is cut from -- comes
        // off few animals, which is the number that actually prices the
        // metal age. The wolf gives pelts now and the bear gives its own
        // hide; both cure to the same leather, and both take a fight to
        // get.
        //
        // **Three, and it was two.** The third is the zebra, and it is
        // there because the deer and the boar do not live on the savanna
        // (`lives_in`): without it the hot country would have no full hide
        // at all, and its metal age would begin with a walk to a wood.
        // Still one meal animal's hide per country, which is the price the
        // two-animal rule was holding.
        //
        // **Four, and the fourth is the horse** -- which does not break the
        // rule so much as price it. A horse killed for its two hides is a
        // horse nobody rides home, and taming one costs a week of feeding;
        // nobody hunts the plains' best mount for leather while there are
        // deer, and anybody who does has made the decision the horse is for.
        let full_hide: Vec<&str> = Species::ALL
            .iter()
            .filter(|s| s.drops().iter().any(|&(b, _)| b == BLOCK_HIDE))
            .map(|s| s.name())
            .collect();
        assert_eq!(full_hide, ["deer", "boar", "zebra", "horse"]);
    }

    #[test]
    fn something_hunts_and_something_is_hunted() {
        // The asymmetry, written down: without it the animals only ever
        // react to the player, which is a zoo rather than a world.
        let predators: Vec<&str> = Species::ALL
            .iter()
            .filter(|s| s.is_predator())
            .map(|s| s.name())
            .collect();
        // One hunter per country: the wolf in the woods and meadows, the
        // lion on the savanna.
        assert_eq!(predators, ["wolf", "lion"]);
        let hunted: Vec<&str> = Species::ALL
            .iter()
            .filter(|&&s| Species::ALL.iter().any(|p| p.hunts(s)))
            .map(|s| s.name())
            .collect();
        // ...and the horse, by both: the pack on the plain and the lion at
        // the savanna's edge (`lives_in`).
        assert_eq!(hunted, ["hare", "deer", "sheep", "zebra", "antelope", "horse"]);
        // Nothing hunts itself, and the ones that fight back are left out
        // of each other's business.
        for &species in Species::ALL {
            assert!(!species.hunts(species), "{} hunts its own kind", species.name());
        }
        assert!(!Species::Wolf.hunts(Species::Boar));
        assert!(!Species::Boar.hunts(Species::Wolf));
        for fighter in [Species::Boar, Species::Wolf, Species::Bear] {
            assert!(!Species::Lion.hunts(fighter), "a lion hunts the {}", fighter.name());
            assert!(!fighter.hunts(Species::Lion), "the {} hunts lions", fighter.name());
        }
        // **A hunter and its dinner live in the same country**, or the
        // hunt is a rule that never happens anywhere a player could see
        // it.
        for &hunter in Species::ALL {
            for &prey in Species::ALL {
                if hunter.hunts(prey) {
                    assert!(
                        crate::worldgen::Biome::ALL
                            .iter()
                            .any(|&biome| hunter.lives_in(biome) && prey.lives_in(biome)),
                        "the {} hunts the {} and they never meet",
                        hunter.name(),
                        prey.name()
                    );
                }
            }
        }
        // A predator is the only thing with a stomach clock.
        for &species in Species::ALL {
            assert_eq!(
                species.fed_seconds() > 0.0,
                species.is_predator(),
                "{} disagrees about whether it eats",
                species.name()
            );
        }
    }

    #[test]
    fn prey_is_common_and_predators_are_not() {
        // An even split over four species makes a quarter of everything
        // alive a wolf, which is what the spawner did before it was
        // weighted.
        //
        // **Asked per country now**, because the spawner only ever draws
        // among the animals that live where it is standing (`lives_in`, and
        // the server's `pick_species`). Summed over all ten the savanna's
        // odds would be diluted by deer it never spawns, and a lion-heavy
        // savanna would pass because the meadow's sheep were counted.
        // "Prey" is everything that does not fight back -- what was a hand
        // list of four, and would have silently left the zebra out.
        use crate::worldgen::Biome;
        for biome in [Biome::Plains, Biome::Forest, Biome::Savanna] {
            let here: Vec<Species> = Species::ALL.iter().copied().filter(|s| !s.swims() && s.lives_in(biome)).collect();
            for night in [false, true] {
                let total: u32 = here.iter().map(|s| s.spawn_weight(night)).sum();
                assert!(total > 0, "nothing spawns in the {} at all", biome.name());
                let prey: u32 = here
                    .iter()
                    .filter(|s| !s.is_hostile())
                    .map(|s| s.spawn_weight(night))
                    .sum();
                assert!(
                    prey * 2 > total,
                    "most of what is alive in the {} should be something that runs (night: {night})",
                    biome.name()
                );
            }
        }
        // ...and the night belongs to the thing that hunts, in both
        // countries.
        assert!(Species::Wolf.spawn_weight(true) > Species::Wolf.spawn_weight(false));
        assert!(Species::Lion.spawn_weight(true) > Species::Lion.spawn_weight(false));
        assert!(Species::Hare.spawn_weight(true) < Species::Hare.spawn_weight(false));
    }

    #[test]
    fn a_beach_at_midnight_is_a_different_place_from_the_same_beach_at_noon() {
        // **The crab's whole mechanic, as one number against another.** A
        // player who walks the tideline in daylight meets the odd crab; the
        // same walk after dark is what the first hungry night on a coast is
        // for. If these two ever come level, the beach has stopped having an
        // hour worth choosing.
        let (day, night) = (Species::Crab.spawn_weight(false), Species::Crab.spawn_weight(true));
        assert!(night >= day * 4, "a beach at night draws {night} crabs against {day} by day");
        // ...and it is the only land animal here that is commoner after
        // dark. The hunters are out more at night too, but a wolf is not
        // *four times* the wolf it was, and a player who has learned that the
        // sand crawls at midnight has learned something about the crab and
        // not about the dark.
        for &species in Species::ALL {
            // The rat is not drawn by the biome spawner at all -- both its
            // weights are nought (`spawn_weight`) -- and "nought is not four
            // times nought" is arithmetic rather than a fact about rats.
            if species == Species::Crab || species.swims() || species == Species::Rat {
                continue;
            }
            assert!(
                species.spawn_weight(true) < species.spawn_weight(false) * 4,
                "{} is as much a night animal as the crab",
                species.name()
            );
        }
    }

    #[test]
    fn a_monkey_is_caught_on_the_sand_and_never_in_the_palms() {
        // **The two halves of the troop, and they have to disagree.** On the
        // ground a monkey is slower than a sprinting player, so a thief that
        // took your dinner and ran along the beach is a thief you catch. Up
        // a trunk it is out of reach of everything this world has -- there is
        // no bow -- so the answer to a troop is never "kill the troop", it is
        // where you put the stores (`Species::Monkey`).
        assert!(
            Species::Monkey.run_speed() < NOMINAL_SPRINT_SPEED,
            "a monkey outruns a sprinting player at {:.1}",
            Species::Monkey.run_speed()
        );
        assert!(Species::Monkey.climbs(), "a monkey that cannot climb is a small slow deer");
        // Nothing else climbs, and nothing that climbs flies: a climber with
        // wings would be a bird with a second altitude rule (`Species::climbs`).
        for &species in Species::ALL {
            assert!(!(species.climbs() && species.flies()), "{} both climbs and flies", species.name());
        }
        // A troop, never a lone one: the alarm is half of what the animal is
        // for, and one monkey has nobody to call to.
        assert!(group_size(Species::Monkey).0 >= 3, "a troop of two is a pair");
    }

    #[test]
    fn nothing_that_lands_a_blow_without_walking_toward_you_counts_as_hostile() {
        // **The line the crab drew between `damage` and `is_hostile`.** Four
        // animals come at a person; the crab lands a blow and comes at
        // nobody. Before the crab the two were one list, and the day they
        // parted is the day this test was written -- see `Species::is_hostile`
        // for the four wrong answers the old definition gave.
        assert!(Species::Crab.damage() > 0.0, "a crab that does not pinch is a slow stone");
        assert!(!Species::Crab.is_hostile(), "a crab is coming for you");
        let hostile: Vec<&str> = Species::ALL.iter().filter(|s| s.is_hostile()).map(|s| s.name()).collect();
        assert_eq!(hostile, ["boar", "wolf", "bear", "lion"]);
        // ...and every one of *them* names its own death, because a player
        // killed by an animal has to be told which. The crab names one too,
        // and it is the only harmless-looking thing in the world that has to.
        for &species in Species::ALL.iter().filter(|s| s.damage() > 0.0) {
            assert_eq!(
                Species::of_death_cause(species.death_cause()),
                Some(species),
                "{} shares its death with somebody else",
                species.name()
            );
        }
    }

    #[test]
    fn the_savanna_keeps_its_own_animals_and_the_woods_keep_theirs() {
        // What a player walking from a meadow into dry grass should see
        // change: the deer, the boar, the wolf, the sheep and the bear stay
        // behind, and the zebra, the antelope and the lion begin. The hare
        // and the bird are on both sides, which is what makes the border a
        // place rather than a wall.
        use crate::worldgen::Biome;
        let living_in = |biome: Biome| -> Vec<&str> {
            // The land's animals: a school lives in any water in any
            // country (`Species::Fish`), and is not what this border is about.
            Species::ALL.iter().filter(|s| !s.swims() && s.lives_in(biome)).map(|s| s.name()).collect()
        };
        // The horse is on both sides of this one border and no other: the
        // plains' own, grazing out onto the straw (`Species::Horse`'s
        // `lives_in` row, and `spawn_weight_in` for how rarely).
        assert_eq!(living_in(Biome::Savanna), ["hare", "fowl", "zebra", "antelope", "lion", "horse"]);
        assert_eq!(living_in(Biome::Plains), ["hare", "deer", "boar", "wolf", "sheep", "bear", "fowl", "horse"]);
        // ...and the shore has two animals that are nowhere else at all. The
        // beach is not a *closed* country the way the savanna is -- a deer
        // walks down to the sand and a wolf follows it -- but nothing walks
        // inland and finds a crab.
        for inland in Biome::ALL.iter().copied().filter(|&b| !matches!(b, Biome::Beach | Biome::Ocean)) {
            for shore in [Species::Monkey, Species::Crab] {
                assert!(!shore.lives_in(inland), "a {} in the {inland:?}", shore.name());
            }
        }
        assert!(Species::Monkey.lives_in(Biome::Beach) && Species::Crab.lives_in(Biome::Beach));
        // What puts a monkey in a grove rather than on a bare dune is not
        // the biome at all: it is the timber round the spot, which the
        // spawner reads off the blocks (see its `lives_in` row).
        assert!(Species::Monkey.needs_trees(), "a monkey spawns on bare sand");
        // Every species lives somewhere -- except the one whose country
        // is a kitchen. A rat is not put into the world by biome at all
        // (see its `lives_in` row and `vermin::may_appear`), and giving
        // it a biome so that this loop passes would be putting rats in
        // meadows to satisfy a test.
        for &species in Species::ALL.iter().filter(|&&s| s != Species::Rat) {
            assert!(
                Biome::ALL.iter().any(|&biome| species.lives_in(biome)),
                "the {} lives nowhere",
                species.name()
            );
        }
        assert!(
            Biome::ALL.iter().all(|&biome| !Species::Rat.lives_in(biome)),
            "a rat has moved into the countryside"
        );
        // ...and both countries have something that runs and something that
        // hunts it, or one of them is scenery.
        for biome in [Biome::Plains, Biome::Savanna] {
            let here: Vec<Species> = Species::ALL.iter().copied().filter(|s| !s.swims() && s.lives_in(biome)).collect();
            assert!(here.iter().any(|s| s.is_predator()), "nothing hunts in the {}", biome.name());
            assert!(
                here.iter().any(|s| here.iter().any(|p| p.hunts(*s))),
                "nothing in the {} is hunted",
                biome.name()
            );
        }
    }

    #[test]
    fn a_lion_loses_a_straight_race_to_both_its_meals_and_wins_by_getting_close() {
        // **The savanna's hunt, in numbers.** Both grazers outrun a lion
        // flat out, so a lion that is seen early eats nothing...
        for prey in [Species::Zebra, Species::Antelope] {
            assert!(
                prey.run_speed() > Species::Lion.run_speed(),
                "a lion runs a {} down in a straight line",
                prey.name()
            );
        }
        // ...the antelope sees it first, and sees you first...
        assert!(Species::Antelope.awareness() > Species::Lion.awareness());
        assert!(
            Species::Antelope.awareness() > Species::Zebra.awareness(),
            "the antelope is meant to be the first thing on the grass to bolt"
        );
        // ...the zebra gets away on its wind and the antelope on its swerve,
        // and an antelope with a zebra's wind would never need to swerve.
        assert!(Species::Zebra.stamina_seconds() > Species::Lion.stamina_seconds());
        assert!(Species::Antelope.stamina_seconds() < Species::Lion.stamina_seconds());
        let names = |rule: fn(Species) -> bool| -> Vec<&'static str> {
            Species::ALL.iter().copied().filter(|&s| rule(s)).map(Species::name).collect()
        };
        assert_eq!(names(Species::zigzags), ["hare", "antelope"]);
        // **The lion's manners: the wolf's where they are the night's, its
        // own where they are the pack's.** Short lists on purpose; each
        // name on them is a branch of the server's `think_hunter`.
        assert_eq!(names(Species::shies_from_fire), ["wolf", "lion"]);
        assert_eq!(names(Species::flanks), ["wolf", "lion"]);
        assert_eq!(names(Species::needs_company), ["wolf"]);
        assert_eq!(names(Species::falls_back_when_hurt), ["wolf"]);
    }

    #[test]
    fn every_species_comes_back_out_of_its_own_skeleton() {
        use crate::types::{
            block_kind, bones_of, is_bones, is_known_block, species_in_bones, BLOCK_BONES,
            BLOCK_BONES_2, VARIANT_MASK, VARIANT_SHIFT,
        };
        // Three bits name eight animals, and the ninth used to write a bit
        // the field does not have: an antelope's skeleton would have read
        // back as a hare's, silently, with nothing anywhere refusing it.
        // See `types::BLOCK_BONES_2`.
        const {
            assert!(
                Species::ALL.len() <= crate::types::SKELETON_ROOM,
                "the skeleton blocks are full; the next species needs another entry in types::SKELETON_BLOCKS"
            )
        };
        let mut ids = std::collections::HashSet::new();
        for &species in Species::ALL {
            let bones = bones_of(species);
            assert!(is_bones(bones) && is_known_block(bones), "{}'s skeleton is not a skeleton", species.name());
            assert_eq!(species_in_bones(bones), Some(species), "{}'s skeleton is somebody else's", species.name());
            assert!(ids.insert(bones), "the {} shares a skeleton id", species.name());
        }
        // ...and the first eight are exactly where they always were, so every
        // skeleton already lying in an old world is still the animal it was.
        for (index, &species) in Species::ALL.iter().enumerate().take(8) {
            let bones = bones_of(species);
            assert_eq!(block_kind(bones), BLOCK_BONES, "{}", species.name());
            assert_eq!(((bones & VARIANT_MASK) >> VARIANT_SHIFT) as usize, index, "{}", species.name());
        }
        assert_eq!(block_kind(bones_of(Species::Antelope)), BLOCK_BONES_2);
        // ...and every block in the table is a skeleton the rest of the game
        // knows: a name, a definition, and `is_bones` for the mesher. The
        // fourth has no species in it yet, which is the one way it could be
        // missing from a list and nothing else notice.
        for &block in crate::types::SKELETON_BLOCKS.iter() {
            assert!(is_bones(block) && is_known_block(block), "skeleton block {block} is not a known skeleton");
        }
    }

    #[test]
    fn every_animal_has_a_long_way_and_a_short_way_and_the_crab_is_the_one_turned_sideways() {
        // Not a fussy detail: it is why the hit box is oriented at all. A
        // creature as wide as it is long could be tested with a sphere, and
        // none of these is.
        //
        // **This test was called `every_animal_is_longer_than_it_is_wide`,
        // and the crab is why it is not.** A crab's long axis runs *across*
        // its heading -- that is the whole animal, and `Species::sidles` is
        // what the rest of the game reads it as. The property actually being
        // defended is that a body has a long way and a short way and the box
        // knows which is which; it survives the crab intact, with the sign
        // turned round.
        for &species in Species::ALL {
            assert_ne!(
                species.length(),
                species.width(),
                "{} is as wide as it is long and could be a sphere",
                species.name()
            );
            let (x, y, z) = species.half_extents();
            assert!(x > 0.0 && y > 0.0 && z > 0.0);
            let sideways = species.sidles();
            assert_eq!(
                z > x,
                !sideways,
                "{}: the hit box lies the wrong way for what this animal is",
                species.name()
            );
            assert_eq!(
                species.length() > species.width(),
                !sideways,
                "{}: the body lies the wrong way for what this animal is",
                species.name()
            );
        }
    }

    #[test]
    fn a_group_is_a_group_and_never_bigger_than_a_pack() {
        for &species in Species::ALL {
            let (low, high) = group_size(species);
            assert!(low >= 1, "{} arrives as nothing", species.name());
            assert!(high >= low, "{}: a backwards range", species.name());
            // The land's groups are spawned whole and bounded by the largest
            // pack (`MAX_GROUP`); the sea's and the shore's are still checked
            // member by member against their own class's allowance, so there
            // the allowance is the bound.
            let bound = if species.swims() || species.soars() { per_player_cap(species) } else { MAX_GROUP };
            assert!(
                high as usize <= bound,
                "{} arrives {high} at a time against a bound of {bound}",
                species.name()
            );
        }
        // **A pack is three or more.** Two wolves were a pair that took turns;
        // the stalk-and-wait-for-your-back rules need somebody to wait.
        assert!(group_size(Species::Wolf).0 >= 3, "a pack of two is a pair");
        assert!(group_size(Species::Wolf).1 as usize <= MAX_GROUP);
        assert_eq!(group_size(Species::Bear), (1, 1), "bears keep company with nobody");
        assert_eq!(Species::Bear.grouping(), Grouping::Alone);
        for herd in [Species::Deer, Species::Sheep, Species::Zebra, Species::Antelope] {
            assert_eq!(herd.grouping(), Grouping::Herd, "{} is not a herd", herd.name());
            assert!(group_size(herd).0 >= 2, "a herd of one {}", herd.name());
        }
    }

    #[test]
    fn prey_sees_nearly_all_round_and_hunters_see_what_is_in_front_of_them() {
        // The one fact about eyes a player can use: come at a grazing deer
        // from dead behind, and at a wolf from anywhere behind its shoulders.
        for &species in Species::ALL {
            if species.swims() {
                continue;
            }
            if species.is_hostile() {
                assert!(species.view_cone() >= -0.5, "a {} sees behind itself", species.name());
            } else {
                assert!(species.view_cone() <= -0.75, "a {} has a blind half", species.name());
            }
            // Hearing is not a longer sight: nothing hears a *walker* twice as
            // far as it sees one, or creeping would be pointless.
            assert!(
                species.hearing() <= species.awareness() * 2.0,
                "a {} hears a walker at {} and sees one at {}",
                species.name(),
                species.hearing(),
                species.awareness()
            );
        }
        // ...and the nose is the hunters' and the deer's, not the birds'.
        assert!(Species::Wolf.nose() > Species::Deer.nose());
        assert_eq!(Species::Fowl.nose(), 0.0);
    }

    #[test]
    fn a_wood_is_cover_and_a_wall_of_leaves_hides_a_deer_but_grass_hides_only_a_hare() {
        use crate::types::{
            BLOCK_AIR, BLOCK_BIRCH_LEAVES, BLOCK_CAMPFIRE, BLOCK_DIRT, BLOCK_LEAVES, BLOCK_LOG,
            BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_WATER,
        };
        // What a fleeing animal makes for: trunks, canopies, tall grass.
        for block in [BLOCK_LEAVES, BLOCK_BIRCH_LEAVES, BLOCK_LOG, BLOCK_TALL_GRASS] {
            assert!(is_cover(block), "{block} is not cover");
        }
        // ...and a boulder, which is the cover a shore has instead of a
        // wood -- the whole of a crab's escape (`Species::hides_in_cover`).
        for block in [crate::types::BLOCK_COBBLESTONE, crate::types::BLOCK_GRANITE] {
            assert!(is_cover(block), "a boulder of {block} is not cover");
        }
        // ...and not the ground it is running over, or the wall it is
        // running from, or a pond -- a deer that made for the nearest
        // rock face would be a deer that runs into corners.
        for block in [BLOCK_AIR, BLOCK_DIRT, BLOCK_STONE, BLOCK_WATER, BLOCK_CAMPFIRE] {
            assert!(!is_cover(block), "{block} counts as cover");
        }

        let deer_eye = Species::Deer.height() * 0.9;
        let hare_eye = Species::Hare.height() * 0.9;
        // Leaves let light through and stop eyes: the whole reason this
        // is not `is_opaque`.
        assert!(blocks_sight(BLOCK_LEAVES, deer_eye));
        assert!(blocks_sight(BLOCK_LOG, deer_eye));
        assert!(blocks_sight(BLOCK_STONE, deer_eye));
        for block in [BLOCK_AIR, BLOCK_WATER, BLOCK_CAMPFIRE] {
            assert!(!blocks_sight(block, deer_eye), "{block} hides a deer");
        }
        // A tuft is over a hare's head and under a deer's chin.
        assert!(blocks_sight(BLOCK_TALL_GRASS, hare_eye), "a hare in grass is in plain view");
        assert!(!blocks_sight(BLOCK_TALL_GRASS, deer_eye), "grass hides a deer");
    }

    #[test]
    fn the_caps_are_a_crowd_rather_than_a_herd_or_a_plague() {
        const { assert!(MAX_ANIMALS_PER_PLAYER >= 2) };
        const { assert!(MAX_ANIMALS >= MAX_ANIMALS_PER_PLAYER) };
        const { assert!(MAX_ANIMALS <= 1000, "an unbounded entity list with a network cost") };
        // ...and the sea's, on the same terms: room for a school, and a
        // ceiling on what twenty players on one coast cost the wire.
        const { assert!(MAX_FISH_PER_PLAYER >= 5, "no room for a school") };
        const { assert!(MAX_FISH >= MAX_FISH_PER_PLAYER) };
        const { assert!(MAX_FISH + MAX_ANIMALS <= 1000, "an unbounded entity list with a network cost") };
        // The shore's allowance is a flock, and it is spent from the land's
        // hard ceiling (`MAX_ANIMALS`), so it adds nothing to that sum.
        const { assert!(MAX_SEABIRDS_PER_PLAYER >= 2, "no room for a flock") };
        const { assert!(MAX_SEABIRDS_PER_PLAYER <= MAX_ANIMALS) };
    }

    #[test]
    fn a_fish_lives_in_the_water_and_no_rule_about_legs_applies_to_it() {
        use crate::worldgen::Biome;
        let swimmers: Vec<&str> = Species::ALL.iter().filter(|s| s.swims()).map(|s| s.name()).collect();
        assert_eq!(swimmers, ["fish", "cod", "trout", "pike", "herring"]);
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            // It gives itself where it dies, and there is no body to cut.
            assert_eq!(species.carcass(), None, "a {} left a carcass", species.name());
            assert!(species.butchering().is_empty());
            assert_eq!(carcass_at_stage(species, 0), crate::types::BLOCK_AIR);
            assert!(
                species.drops().iter().all(|&(b, _)| b == crate::types::BLOCK_RAW_FISH),
                "a {} drops something that is not fish",
                species.name()
            );
            // Harmless, and nothing hunts it or is hunted by it.
            assert!(!species.is_hostile() && !species.is_predator());
            assert!(Species::ALL.iter().all(|s| !s.hunts(species)));
            assert!(!species.flies() && !species.needs_trees());
        }
        // A school is several; a cod is one, and only in the sea.
        assert!(group_size(Species::Fish).0 >= 3, "a school of fewer than three is two fish");
        assert_eq!(group_size(Species::Cod), (1, 1));
        assert!(Species::Cod.lives_in(Biome::Ocean));
        assert!(!Species::Cod.lives_in(Biome::River), "a cod in a river");
        assert!(Species::Fish.lives_in(Biome::River) && Species::Fish.lives_in(Biome::Ocean));
        // ...and a cod is the bigger meal, which is what the deep water is for.
        let fish = |s: Species| s.drops().iter().map(|&(_, n)| n).sum::<u32>();
        assert!(fish(Species::Cod) > fish(Species::Fish));
    }

    #[test]
    fn each_kind_of_water_has_its_own_fish_and_only_the_school_is_in_all_of_them() {
        // **The map of the water, as the spawner reads it.** The whole point
        // of three more swimmers is that where a player fishes decides what
        // they get: cold running water is a trout, a warm still lake is a
        // pike, the shallow sea is a herring and the deep sea is a cod. If
        // any two of those tables ever overlap, the place stops deciding and
        // the fish are decoration again -- so this test is the rule.
        use crate::worldgen::Biome;
        let lives = |species: Species| -> Vec<&'static str> {
            Biome::ALL.iter().filter(|&&b| species.lives_in(b)).map(|b| b.name()).collect()
        };
        assert_eq!(
            lives(Species::Trout),
            ["birch forest", "bog", "taiga", "tundra", "mountains", "snowy peaks", "river"],
            "the trout has left the cold running water"
        );
        assert_eq!(
            lives(Species::Pike),
            ["savanna", "plains", "forest", "dead forest", "swamp", "steppe", "hills"],
            "the pike has left the warm still water"
        );
        assert_eq!(lives(Species::Herring), ["ocean", "beach"], "the herring has left the coast");
        // No country holds both of the fresh-water two: that is the cold
        // half and the warm half of the land, and walking from one to the
        // other is the mechanic.
        assert!(
            !Biome::ALL.iter().any(|&b| Species::Trout.lives_in(b) && Species::Pike.lives_in(b)),
            "a trout and a pike in the same water"
        );
        // ...and neither of them is ever in the sea, where the cod and the
        // herring are, and the herring is never inland.
        for &salt in &[Biome::Ocean, Biome::Beach] {
            assert!(!Species::Trout.lives_in(salt) && !Species::Pike.lives_in(salt));
        }
        assert!(!Species::Herring.lives_in(Biome::River));
        // The plain school is still the fish that is everywhere: something
        // has to be in the pond a player dug.
        assert!(Biome::ALL.iter().all(|&b| Species::Fish.lives_in(b)));
        // And the size of the catch is the size of the fish, which is what
        // makes one water worth walking to over another. See `drops`.
        let fish = |s: Species| s.drops().iter().map(|&(_, n)| n).sum::<u32>();
        assert_eq!(fish(Species::Herring), fish(Species::Fish));
        assert!(fish(Species::Fish) < fish(Species::Trout));
        assert!(fish(Species::Trout) < fish(Species::Pike));
        assert_eq!(fish(Species::Pike), fish(Species::Cod));
    }

    #[test]
    fn a_gull_lives_on_the_coast_and_nowhere_inland() {
        use crate::worldgen::Biome;
        for &biome in Biome::ALL {
            assert_eq!(
                Species::Gull.lives_in(biome),
                matches!(biome, Biome::Ocean | Biome::Beach),
                "a gull and the {} disagree",
                biome.name()
            );
        }
        // The one that soars is a bird, and the only one.
        let soaring: Vec<&str> = Species::ALL.iter().filter(|s| s.soars()).map(|s| s.name()).collect();
        assert_eq!(soaring, ["gull"]);
        assert!(Species::ALL.iter().all(|s| !s.soars() || s.flies()), "something soars without flying");
    }

    #[test]
    fn a_gull_is_hunted_by_getting_near_it_and_gives_feathers_where_it_falls() {
        // **The hunt is the stalk**: it notices you further off than a spear
        // reaches, and not so far that nothing on a beach can hide you.
        let reach = crate::combat::SPEAR_REACH;
        assert!(Species::Gull.awareness() > reach, "a gull lets a spear walk up to it");
        assert!(Species::Gull.awareness() < Species::Fowl.awareness(), "a gull warier than a grouse");
        assert!(!Species::Gull.is_hostile());
        assert!(Species::Gull.health() <= Species::Fowl.health(), "a gull is more than one good hit");
        // No body; feathers and a mouthful instead, and feathers are
        // something (kindling) rather than a dead end.
        assert_eq!(Species::Gull.carcass(), None);
        assert!(Species::Gull.drops().iter().any(|&(b, _)| b == BLOCK_FEATHER));
        assert!(
            crate::hearth::fuel_seconds(BLOCK_FEATHER).is_some(),
            "a feather burns for nothing, which makes the gull's drop a dead end"
        );
        // ...and its flock is counted against the shore, not the meadow.
        assert_eq!(per_player_cap(Species::Gull), MAX_SEABIRDS_PER_PLAYER);
        assert!(group_size(Species::Gull).1 as usize <= MAX_SEABIRDS_PER_PLAYER);
    }
}

#[cfg(test)]
mod body_cutting_tests {
    use super::*;
    use crate::types::{BLOCK_CORPSE, BLOCK_FLINT_KNIFE, BLOCK_HUMAN_FLESH, BLOCK_REMAINS, BLOCK_STONE_AXE};

    #[test]
    fn a_knife_takes_flesh_off_a_body_in_three_cuts_and_leaves_the_bones() {
        // "Добавь разделку трупов людей и человеческое мясо".
        let mut body = BLOCK_CORPSE;
        let mut flesh = 0;
        for cut in 0..BODY_CUTS {
            let Some(Butchered::Cut { took, next }) = cut_body(body, Some(BLOCK_FLINT_KNIFE)) else {
                panic!("cut {cut} was refused");
            };
            let (what, count) = took.expect("a cut with nothing on it");
            assert_eq!(what, BLOCK_HUMAN_FLESH);
            flesh += count;
            body = next;
        }
        assert_eq!(body, BLOCK_REMAINS, "the last cut did not leave the bones");
        assert_eq!(flesh, 6);
        assert_eq!(cut_body(BLOCK_CORPSE, Some(BLOCK_STONE_AXE)), Some(Butchered::NeedsATool), "an axe butchered a person");
        assert_eq!(cut_body(BLOCK_CORPSE, None), Some(Butchered::NeedsATool));
        assert_eq!(cut_body(BLOCK_REMAINS, Some(BLOCK_FLINT_KNIFE)), None, "bones were cut for flesh");
        assert!(crate::types::is_known_block(BLOCK_CORPSE | (2 << crate::types::VARIANT_SHIFT)), "a cut body is an invented id");
    }

    #[test]
    fn human_flesh_feeds_like_meat_and_sickens_longer_than_anything_even_roasted() {
        use crate::food::{nutrition, sickness_seconds};
        use crate::types::{BLOCK_COOKED_MEAT, BLOCK_RAW_MEAT, BLOCK_ROAST_HUMAN_FLESH, BLOCK_TOADSTOOL};
        assert_eq!(nutrition(BLOCK_HUMAN_FLESH), nutrition(BLOCK_RAW_MEAT));
        assert_eq!(nutrition(BLOCK_ROAST_HUMAN_FLESH), nutrition(BLOCK_COOKED_MEAT));
        assert!(sickness_seconds(BLOCK_ROAST_HUMAN_FLESH) > sickness_seconds(BLOCK_TOADSTOOL), "a fire made human flesh safe");
        assert!(sickness_seconds(BLOCK_HUMAN_FLESH) > sickness_seconds(BLOCK_ROAST_HUMAN_FLESH));
    }
}
