//! The catalogue: every sound the game can make, and which block sounds
//! like what.
//!
//! ## Every sound is a recording
//!
//! Twelve materials, four things that can happen to each of them
//! (digging, breaking, placing, walking on) plus a fifth that only five of
//! them can do ([`crumble_of`] -- giving way), six workshop sounds, about
//! thirty sounds that belong to nothing in particular, and fifty animal
//! cries. **Every one of them is a CC0 recording** (see
//! [`super::recorded`]), decoded at startup into the clips the mixer
//! plays.
//!
//! This file used to be two thousand lines of recipes -- noise through
//! modal resonances, a pulse train through a vocal tract for each animal,
//! crossfaded beds of drops and embers -- and the recordings were laid over
//! them one sound at a time. The player's order was "all sounds downloaded,
//! with no strings attached, and delete everything generated", so the
//! recipes and the synthesiser under them are gone. A sound with no
//! recording is silent, and `recorded::SILENT` says which and why; nothing
//! is quietly made up in its place.
//!
//! The materials are *acoustic* categories, not the game's own. Coal ore
//! is rock and coal is a handful of loose lumps; a flint knife is stone
//! and a bronze one is metal; a fired pot is ceramic and the wet clay it
//! was made from is soil. [`Material::of`] is where that judgement
//! lives, and it is deliberately the only place: adding a block to the
//! game means adding at most one line here, and forgetting to means the
//! block sounds like whatever its `Matter` says, which is always
//! plausible and never silent.
//!
//! ## Nothing here is allowed to be a note
//!
//! The first version of the sound sounded like a role-playing game from
//! 1991: a third of it was pure tones, some a musical interval apart. The
//! rule that came out of it outlived the recipes -- **a sound is an object
//! being disturbed, not a note being sounded** -- and
//! `nothing_in_the_bank_is_a_note` holds every recording to it, except
//! where a pitch is physics (a bubble, struck metal, an animal's voice).
//!
//! ## `hound` earns its place twice
//!
//! * **Out.** `--export-sounds <dir>` writes every recording, as the game
//!   decodes it, as `.wav` -- a file of the right length and shape for
//!   anybody who wants to replace one.
//! * **In.** Anything in `assets/sounds` named for a sound replaces its
//!   recordings. Exactly the story `embedded` tells about textures: what is
//!   built in is the fallback, and a file on disk wins.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use primitive_shared::animals::Species;
use primitive_shared::blocks::{self, Matter, Work};
use primitive_shared::types::{block_kind, block_name, BlockId};

use super::clip::{Clip, Rng};

/// The most numbered files a resource pack may give one sound
/// (`dig.stone.0.wav` ... `dig.stone.63.wav`). A cap only so a folder of
/// junk cannot make startup walk forever; the recordings have at most ten.
const MAX_PACK_VARIANTS: usize = 64;

/// How long one piece of the rain lasts, and how much of it is the
/// crossfade into the next.
///
/// **The soundscape lays the recorded pieces end over end, `BED_SECONDS -
/// BED_FADE` apart**, and reads both numbers from here. The recordings are
/// cut to exactly this length with an equal-power fade of this length at
/// each end, so the two numbers and the files are one decision --
/// `every_bed_is_as_long_as_the_soundscape_lays_it` holds them together.
pub const BED_SECONDS: f32 = 3.2;
pub const BED_FADE: f32 = 0.8;

/// The same two numbers for the fire, which is shorter because it is
/// placed at a block and the placement is worked out once per piece.
pub const FIRE_SECONDS: f32 = 2.4;
pub const FIRE_FADE: f32 = 0.6;

/// Where a resource pack puts its `.wav` files, under the assets folder.
pub const SOUNDS_DIR: &str = "sounds";

/// What a thing sounds like when it is struck.
///
/// Twelve, and each one is a genuinely different physical event rather
/// than a different loudness of the same one -- which is the test a
/// candidate has to pass to get in here. Ceramic is the newest and the
/// clearest illustration: it started as glass with a lower band, sounded
/// like a thin wine glass, and a game with a whole pottery tree in it
/// deserved better than that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// Rock, ore, worked stone. The default for anything solid.
    Stone,
    /// Soil, clay, ash: a dull thud with no ring in it.
    Dirt,
    /// Turf, leaves, stems -- anything growing. A swish rather than a
    /// knock.
    Grass,
    /// Timber, standing or worked. The one material with a real
    /// resonance to it.
    Wood,
    Sand,
    /// Loose stones: gravel, pebbles, flint, lumps of coal. Several
    /// small impacts rather than one.
    Gravel,
    Snow,
    /// Ingots, metal tools, plate armour.
    Metal,
    /// Ice, and anything else that would shatter.
    Glass,
    /// Fired pottery and brick.
    Ceramic,
    /// Hide, leather, fibre, cloth, food. Almost nothing: a soft
    /// rustle.
    Cloth,
    /// Water.
    Liquid,
}

impl Material {
    pub const ALL: [Material; 12] = [
        Material::Stone,
        Material::Dirt,
        Material::Grass,
        Material::Wood,
        Material::Sand,
        Material::Gravel,
        Material::Snow,
        Material::Metal,
        Material::Glass,
        Material::Ceramic,
        Material::Cloth,
        Material::Liquid,
    ];

    /// What it is called in a filename.
    pub fn key(self) -> &'static str {
        match self {
            Material::Stone => "stone",
            Material::Dirt => "dirt",
            Material::Grass => "grass",
            Material::Wood => "wood",
            Material::Sand => "sand",
            Material::Gravel => "gravel",
            Material::Snow => "snow",
            Material::Metal => "metal",
            Material::Glass => "glass",
            Material::Ceramic => "ceramic",
            Material::Cloth => "cloth",
            Material::Liquid => "liquid",
        }
    }

    fn index(self) -> usize {
        Material::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    /// What a block sounds like.
    ///
    /// Read on the *kind* rather than the id, so a log lying on its side
    /// and a campfire facing north are the same material as the ones
    /// facing the other way -- orientation is a rendering fact, and a
    /// table keyed on the oriented id would need four rows for every
    /// block that can turn.
    ///
    /// The order of the tests is the whole of the logic: the specific
    /// names first, then the tool and foliage questions the shared crate
    /// can answer, then `Matter` as the backstop. An ore is tested
    /// before a metal because `iron_ore` starts with `iron` and is a
    /// rock.
    pub fn of(id: BlockId) -> Material {
        let kind = block_kind(id);
        let def = blocks::definition(kind);
        let name = block_name(kind);

        if def.matter == Matter::Liquid {
            return Material::Liquid;
        }

        // ---- named exceptions, in the order they have to be asked ----
        // Rock that is named after a metal.
        if name.ends_with("_ore") || name == "native_copper" {
            return Material::Stone;
        }
        // Anything smelted, forged, or worn as plate.
        if name.starts_with("copper")
            || name.starts_with("tin")
            || name.starts_with("bronze")
            || name.starts_with("iron")
        {
            return Material::Metal;
        }
        // Fired earth. `_raw` is the unfired form and is still clay.
        if name.ends_with("_raw") || name == "clay" {
            return Material::Dirt;
        }
        if matches!(name, "vessel" | "jug" | "jug_water" | "mould" | "brick" | "bricks" | "bowl")
            || name.starts_with("kiln")
        {
            return Material::Ceramic;
        }
        if name == "ice" {
            return Material::Glass;
        }
        if name == "snow" {
            return Material::Snow;
        }
        if name == "sand" {
            return Material::Sand;
        }
        // Loose stone: several small impacts rather than one big one.
        // ...and a skeleton, which the comment below has always said and
        // this list never did: bones fell through to rock.
        if matches!(name, "gravel" | "pebble" | "flint" | "flint_flake" | "coal" | "cobblestone" | "remains")
            || name.starts_with("bones")
        {
            return Material::Gravel;
        }
        // **An animal is flesh, whatever the table calls its matter.** A
        // carcass is `Matter::Solid` so it can be stood on and cut, and a
        // solid block with no name here fell through to `Stone` -- whose
        // recorded blow is a pick on rock. Every cut into a deer rang like
        // metal ("животные ломаются со звуком метала"). Everything eaten,
        // and everything taken off an animal, is the soft sound.
        if name.starts_with("carcass")
            || primitive_shared::food::group(kind).is_some()
            || matches!(name, "pelt" | "bear_hide" | "wool" | "feather" | "sinew" | "fat" | "honey" | "beeswax")
        {
            return Material::Cloth;
        }
        // Soft things -- worn, carried, or eaten.
        if matches!(
            name,
            // A body is the soft one; its bones are under `Gravel` below,
            // which is the closest this bank has to something dry that
            // rattles.
            "hide" | "leather" | "fiber" | "leaf_handful" | "backpack" | "corpse" | "raw_meat" | "cooked_meat"
                | "bread" | "dough" | "grain" | "seeds" | "berries" | "root" | "roasted_root"
        ) || name.starts_with("leather_")
        {
            return Material::Cloth;
        }
        // Timber, worked or standing, and everything built out of it.
        if name.contains("log")
            || name.contains("planks")
            || name.contains("stick")
            || matches!(name, "chest" | "drying_rack" | "hide_frame" | "hoe")
            || name.starts_with("campfire")
            || name.starts_with("bloomery")
        {
            return Material::Wood;
        }
        // Flint tools: a stone edge on a wooden haft, and the edge is
        // what meets whatever is being hit.
        if name.starts_with("flint") {
            return Material::Stone;
        }

        // ---- what the shared crate already knows ----
        // Dry turf is turf, and sounds like it underfoot -- it is not
        // foliage (it is untinted) and not worked as a plant, so it has to
        // be named here or it would crunch like bare dirt.
        if primitive_shared::types::is_foliage(kind)
            || def.work == Work::Plant
            || name == "grass"
            || name == "dry_turf"
        {
            return Material::Grass;
        }
        if def.work == Work::Wood {
            return Material::Wood;
        }

        match def.matter {
            Matter::Solid => Material::Stone,
            Matter::Loose => Material::Dirt,
            Matter::Liquid => Material::Liquid,
        }
    }
}

/// What is happening to the material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    /// One blow of a swing that has not finished yet. The quietest of
    /// the four, because it repeats several times a second.
    Dig,
    /// The blow that finished it, plus the block coming apart.
    Break,
    /// Setting one down.
    Place,
    /// A foot on it.
    Step,
}

impl Impact {
    const ALL: [Impact; 4] = [Impact::Dig, Impact::Break, Impact::Place, Impact::Step];

    fn key(self) -> &'static str {
        match self {
            Impact::Dig => "dig",
            Impact::Break => "break",
            Impact::Place => "place",
            Impact::Step => "step",
        }
    }

    fn index(self) -> usize {
        Impact::ALL.iter().position(|i| *i == self).unwrap_or(0)
    }
}

/// One sound the game can ask for.
///
/// A flat enum rather than strings at the call site: a typo in
/// `audio.play(Sfx::Clik)` should not compile, and the mixer should not
/// be hashing a string on a path that runs on every footfall. Names only
/// exist for the two things that genuinely need them -- files on disk,
/// and the mod API, which cannot share a Rust enum across a `dlopen`
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    /// Something happening to a material. Four by twelve of them.
    Material(Impact, Material),

    // ---- the player's own body ----
    Jump,
    Land,
    /// Hitting water from above.
    Splash,
    /// A stroke, while swimming.
    Swim,
    /// A step through water that is not deep enough to swim in.
    ///
    /// **Its own sound, not a quiet stroke.** Wading played `Swim` at a
    /// lower gain, and a swimmer's arm hitting the surface turned down is
    /// still an arm hitting the surface: every stride along a shore was a
    /// slap. A leg pushed through the water and lifted out is a drag and a
    /// drip, and nothing but a different recording sounds like one.
    Wade,
    /// Air running out.
    Bubble,
    Hurt,
    /// A body driven onto sharpened stakes: poles knocked and shoved, a
    /// twig or a sleeve giving, and the body stopping -- not a knife.
    ///
    /// On top of `Hurt`, not instead of it, and placed where it happened:
    /// the victim hears their own leg taken, and everybody near hears it
    /// happen to a player or an animal that is not them. See
    /// `ServerMessage::Staked`.
    Staked,
    Death,
    Eat,
    Drink,

    // ---- what the player does to the world ----
    /// A swing that connects with nothing.
    Swing,
    /// One that lands on something alive.
    Hit,
    Pickup,
    Drop,
    Equip,
    Craft,
    ChestOpen,
    ChestClose,
    /// Flint on stone.
    Ignite,

    // ---- the interface ----
    Click,
    Hover,
    Back,
    /// A line arriving in chat.
    Message,

    // ---- the world itself ----
    FireCrackle,
    RainTick,
    Thunder,
    Wind,

    // ---- what lives in it ----
    //
    // **Decided on the client, from what the client already knows**: a
    // gull's position and speed came in a snapshot, and a frog is the
    // client's own (`engine::critters`). A call is a drawing of something,
    // like blood is -- nothing about the world is decided by a sound.
    /// A gull: a string of two or three harsh, falling "kyow"s.
    GullCall,
    /// A bird going up: the clatter of its first wingbeats. The sound of
    /// a covey flushed by something the player has not seen yet -- see
    /// `Soundscape::wildlife`.
    WingBeats,
    /// A frog: a quick ratchet of clicks through a throat that rings.
    FrogCroak,
    /// Bees round a hive: the hum of many wings, none of them a note. See
    /// `Soundscape::wildlife`.
    Swarm,

    // ---- the weather as a bed, not as events ----
    /// Rain under open sky: a continuous band of noise, laid end over end
    /// by the soundscape: see `BED_SECONDS`. It was once a shower of drops,
    /// and a shower of drops is heard as ticks.
    Rain,
    /// The same rain heard from under a roof or an overhang: the hiss
    /// gone, the patter on the roof left.
    RainSheltered,
    /// A light air in grass and leaves: a rustle, not a whoosh.
    WindBreeze,
    /// Wind on high ground and in a storm: a hollow moan with no note in
    /// it. `Sfx::Wind` is the third, the gust on open ground.
    WindHowl,

    // ---- something pushing through a bush ----
    /// A body moving through leaves or tall stems. The player's and an
    /// animal's are the same sound, played at their own size.
    Rustle,

    // ---- the workshops ----
    //
    // **One sound a workshop, and it is the work and not the making.**
    // `Sfx::Craft` is a generic rustle of things coming together, and it
    // is what the pack's own crafting column still plays; what these are
    // is the *place* -- a player who crafts a plank at a bench hears the
    // saw, and the same recipe run somewhere else does not, which is the
    // whole reason to carry a bench into the woods. The alternative
    // considered was one sound per recipe, and it is a hundred and fifty
    // downloads for a distinction nobody could name.
    /// The joiner's bench: a saw through a board, and a plane along one.
    WorkBench,
    /// The mason's block: a chisel and a mallet on stone.
    WorkMason,
    /// The potter's wheel: the head turning, and wet clay under a hand.
    WorkWheel,
    /// The currier's bench: leather cut with a round knife and punched.
    WorkLeather,
    /// The anvil: the hammer on hot iron, which rings -- see
    /// `nothing_in_the_bank_is_a_note`, where it is an exception for the
    /// same reason struck metal is.
    WorkAnvil,
    /// A workshop's screen opening.
    ///
    /// **Not the chest's lid.** A chest is a thing with a lid and the
    /// sound of opening one is the lid; a bench has no lid, and playing
    /// the chest's meant that stepping up to the anvil sounded like
    /// finding a box. This is tools taken up off the bench instead.
    StationOpen,

    /// Something giving way: scree off a rock face, soil off a bank, a
    /// board cracking through. See [`crumble_of`] for which materials
    /// have one and why the rest do not.
    Crumble(Material),

    // ---- the float ----
    //
    // **Its own two sounds, and not the water's.** The bite used to play
    // `Material(Dig, Liquid)`, which is a hand going into a pool -- the
    // sound of the player, at the player's own gain, for something happening
    // twelve blocks away to a cork. A float has two things to say: that it
    // is down, and that something has it.
    /// The float landing: a small plop, placed where it came down.
    FloatPlop,
    /// A fish taking the bait: the float drawn under, and the one soft
    /// bubble that comes up where it was.
    FloatBite,

    /// What an animal says, and when. See [`Cry`] and [`VOICES`].
    Animal(Species, Cry),
}

/// What an animal is doing when it makes a sound.
///
/// **Five, and each is a thing the client can actually see happen.** The
/// server does not send an animal's state of mind, and a sound is not a
/// reason to start sending it: `soundscape::Voices` reads a calm animal
/// from a slow one, an alarm from the moment it breaks into a run, a
/// threat from a hunter breaking into a run *at you*, a wound from the
/// flash going up, and a death from an animal that was just wounded and
/// is no longer there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cry {
    /// Grazing, pecking, calling to the rest of the herd.
    Idle,
    /// Something was seen and it is leaving.
    Alarm,
    /// Something was seen and it is coming.
    Threat,
    Hurt,
    Death,
}

impl Cry {
    pub fn key(self) -> &'static str {
        match self {
            Cry::Idle => "idle",
            Cry::Alarm => "alarm",
            Cry::Threat => "threat",
            Cry::Hurt => "hurt",
            Cry::Death => "death",
        }
    }
}

/// Every animal sound there is, species by species.
///
/// **A table of the pairs that exist rather than every species times every
/// cry**, because the gaps are facts and not omissions:
///
/// * a hare is silent until it is caught -- what it does when alarmed is
///   drum the ground with a hind foot, which is its `Alarm` here, and it
///   has no idle call at all;
/// * nothing that eats grass threatens, so only the four that fight
///   (boar, wolf, bear, lion) have a `Threat`;
/// * a fish says nothing a person above the water hears -- what it has is
///   the flick of its tail as a school scatters, and the thrash on a
///   spear;
/// * a gull's calm call and its alarm are the recordings it already had
///   (`Sfx::GullCall`), so only its wound and death are here.
///
/// Appended to the bank after everything else, so every other sound keeps
/// its slot. `every_species_has_a_voice` holds this to `Species::ALL`.
pub const VOICES: &[(Species, Cry)] = &[
    (Species::Hare, Cry::Alarm),
    (Species::Hare, Cry::Hurt),
    (Species::Hare, Cry::Death),
    (Species::Deer, Cry::Idle),
    (Species::Deer, Cry::Alarm),
    (Species::Deer, Cry::Hurt),
    (Species::Deer, Cry::Death),
    (Species::Boar, Cry::Idle),
    (Species::Boar, Cry::Alarm),
    (Species::Boar, Cry::Threat),
    (Species::Boar, Cry::Hurt),
    (Species::Boar, Cry::Death),
    (Species::Wolf, Cry::Idle),
    (Species::Wolf, Cry::Alarm),
    (Species::Wolf, Cry::Threat),
    (Species::Wolf, Cry::Hurt),
    (Species::Wolf, Cry::Death),
    (Species::Sheep, Cry::Idle),
    (Species::Sheep, Cry::Alarm),
    (Species::Sheep, Cry::Hurt),
    (Species::Sheep, Cry::Death),
    (Species::Bear, Cry::Idle),
    (Species::Bear, Cry::Alarm),
    (Species::Bear, Cry::Threat),
    (Species::Bear, Cry::Hurt),
    (Species::Bear, Cry::Death),
    (Species::Fowl, Cry::Idle),
    (Species::Fowl, Cry::Alarm),
    (Species::Fowl, Cry::Hurt),
    (Species::Fowl, Cry::Death),
    (Species::Zebra, Cry::Idle),
    (Species::Zebra, Cry::Alarm),
    (Species::Zebra, Cry::Hurt),
    (Species::Zebra, Cry::Death),
    (Species::Antelope, Cry::Idle),
    (Species::Antelope, Cry::Alarm),
    (Species::Antelope, Cry::Hurt),
    (Species::Antelope, Cry::Death),
    (Species::Lion, Cry::Idle),
    (Species::Lion, Cry::Alarm),
    (Species::Lion, Cry::Threat),
    (Species::Lion, Cry::Hurt),
    (Species::Lion, Cry::Death),
    (Species::Fish, Cry::Alarm),
    (Species::Fish, Cry::Hurt),
    (Species::Fish, Cry::Death),
    (Species::Cod, Cry::Alarm),
    (Species::Cod, Cry::Hurt),
    (Species::Cod, Cry::Death),
    (Species::Gull, Cry::Hurt),
    (Species::Gull, Cry::Death),
];

/// Which sound an animal makes for a cry, if it makes one.
///
/// **The one door the soundscape asks through**, so the gull's borrowed
/// recording and the pairs [`VOICES`] leaves out are decided here and not
/// at a call site. A pair that is not in the table answers `None` rather
/// than a slot, because `Sfx::index` of a pair it does not know is somebody
/// else's sound.
pub fn voice_of(species: Species, cry: Cry) -> Option<Sfx> {
    if species == Species::Gull && matches!(cry, Cry::Idle | Cry::Alarm) {
        return Some(Sfx::GullCall);
    }
    // **The trout, the pike and the herring are heard as the school is**,
    // and that is not a saving deferred until somebody records three more
    // splashes: a fish has no voice. What these clips are is *water* -- a
    // surface broken, a body thrashing in the shallows -- and one recording
    // of that is as true of a pike as of a minnow. The cod keeps its own
    // because it is heavier water for a much heavier fish, which is the one
    // difference an ear could name.
    if matches!(species, Species::Trout | Species::Pike | Species::Herring) {
        return voice_of(Species::Fish, cry);
    }
    // **A rat is heard as a hare**, and for the same kind of reason the
    // three fish are heard as the school: these recordings are not a
    // *hare*, they are a small mammal squealing, and a rat squealing is
    // the same throat at the same size. Borrowed rather than left as a
    // gap waiting for a file, because a rat with no voice is the one
    // animal in this game whose whole job is to be heard before it is
    // seen -- you are asleep, the room is dark, and the noise is the
    // mechanic.
    //
    // A rat gets the alarm squeal for its *idle* as well, which the hare
    // has no sound for and does not need: a hare at rest is silent and a
    // rat in your storeroom is not. It is the same clip either way; what
    // differs is when the soundscape asks for it (`idle_chance`).
    if species == Species::Rat {
        return voice_of(Species::Hare, if cry == Cry::Idle { Cry::Alarm } else { cry });
    }
    VOICES
        .contains(&(species, cry))
        .then_some(Sfx::Animal(species, cry))
}

/// The sounds that are not a material times an impact, in the order they
/// are indexed. Adding one means adding it here, to `file_name`, and a
/// row to `recorded::RECORDINGS`.
const LOOSE: [Sfx; 24] = [
    Sfx::Jump,
    Sfx::Land,
    Sfx::Splash,
    Sfx::Swim,
    Sfx::Wade,
    Sfx::Bubble,
    Sfx::Hurt,
    Sfx::Staked,
    Sfx::Death,
    Sfx::Eat,
    Sfx::Drink,
    Sfx::Swing,
    Sfx::Hit,
    Sfx::Pickup,
    Sfx::Drop,
    Sfx::Equip,
    Sfx::Craft,
    Sfx::ChestOpen,
    Sfx::ChestClose,
    Sfx::Ignite,
    Sfx::Click,
    Sfx::Hover,
    Sfx::Back,
    Sfx::Message,
    // FireCrackle, RainTick, Thunder and Wind are appended by `ALL`
    // below rather than listed here -- see the note there.
];

/// The four ambient sounds, kept apart from [`LOOSE`] only because a
/// 26-long array literal is harder to read than 22 and 4.
const AMBIENT: [Sfx; 4] = [Sfx::FireCrackle, Sfx::RainTick, Sfx::Thunder, Sfx::Wind];

/// The animals' calls, appended after [`AMBIENT`] rather than into it so
/// that every sound already in a player's `assets/sounds` keeps its index.
const WILDLIFE: [Sfx; 4] = [Sfx::GullCall, Sfx::WingBeats, Sfx::FrogCroak, Sfx::Swarm];

/// The rain beds, the two winds that joined the gust, and a body in a
/// bush -- appended after [`WILDLIFE`] for the same reason it was appended
/// after [`AMBIENT`].
const WEATHER: [Sfx; 5] = [Sfx::Rain, Sfx::RainSheltered, Sfx::WindBreeze, Sfx::WindHowl, Sfx::Rustle];

/// The four workshops of `crafting::Station`, the anvil (which is not one
/// of them -- it is a mini-game, see [`station_blow`]), and the sound of a
/// station's screen opening. Appended after [`WEATHER`] for the reason
/// that list was appended after [`WILDLIFE`].
const WORKSHOP: [Sfx; 6] = [
    Sfx::WorkBench,
    Sfx::WorkMason,
    Sfx::WorkWheel,
    Sfx::WorkLeather,
    Sfx::WorkAnvil,
    Sfx::StationOpen,
];

/// The materials that have a sound for *giving way*, in the order they
/// are indexed.
///
/// **Five out of twelve, and the seven gaps are the answer rather than a
/// backlog.** A crumble is loose debris leaving the place it was resting
/// in: scree off a face, soil off a bank, gravel raining down, sand
/// pouring, a board cracking through. Metal bends, glass and pottery
/// shatter (which is their break, and already recorded), cloth and turf
/// have nothing to shed, snow has its own soft collapse nobody has
/// recorded, and water does not crumble. A row for each of those would be
/// seven downloads whose only job is to make a table square.
pub const CRUMBLES: [Material; 5] =
    [Material::Stone, Material::Dirt, Material::Gravel, Material::Sand, Material::Wood];

/// What a recipe sounds like being made, if it is made at a workshop.
///
/// **The station decides, not the recipe.** A plank and a haft are two
/// rows of `crafting::RECIPES` and one saw; asking the station is one
/// line here instead of a table that has to grow with every recipe
/// anybody adds. `None` is the hand, the fire and the kiln -- a fire has
/// its own sound already and a pair of hands is `Sfx::Craft`.
pub fn workshop_of(station: primitive_shared::crafting::Station) -> Option<Sfx> {
    use primitive_shared::crafting::Station;
    match station {
        Station::Bench => Some(Sfx::WorkBench),
        Station::Mason => Some(Sfx::WorkMason),
        Station::Wheel => Some(Sfx::WorkWheel),
        Station::Leather => Some(Sfx::WorkLeather),
        Station::Hands | Station::Heat | Station::Forge | Station::Bloomery => None,
    }
}

/// One blow of the mini-game at a station: the hammer, or the potter's
/// hands closing on the wall of a pot.
///
/// Every blow has a sound and not only the ones that land. The marker
/// and the sweet spot are what tell a player how they did; a hammer that
/// rang only on a good blow would answer the question the bar is asking,
/// and a miss would be the smith's arm passing through the iron.
pub fn station_blow(game: primitive_shared::minigame::Game) -> Sfx {
    use primitive_shared::minigame::Game;
    match game {
        Game::Anvil => Sfx::WorkAnvil,
        Game::Wheel => Sfx::WorkWheel,
    }
}

/// The sound of `material` giving way, if it has one.
///
/// **The one door**, like [`voice_of`]: a caller asks about a material and
/// gets `None` for the ones that do not crumble, rather than reaching for
/// `Sfx::Crumble` of something that has no slot.
pub fn crumble_of(material: Material) -> Option<Sfx> {
    CRUMBLES.contains(&material).then_some(Sfx::Crumble(material))
}

/// The sound of a quarter coming off a block being dug in slices
/// (`dig::took_a_slice`): the crumble of its material, and **soil's for
/// turf** -- a quarter cut out of a sod is roots and earth falling off a
/// spade, and grass has no crumble of its own because a lawn does not give
/// way. `None` for anything else with no crumble, which then digs silently
/// rather than borrowing something wrong.
pub fn slice_of(material: Material) -> Option<Sfx> {
    crumble_of(material).or_else(|| (material == Material::Grass).then_some(Sfx::Crumble(Material::Dirt)))
}

/// How many material-and-impact combinations there are.
const MATERIAL_SOUNDS: usize = Impact::ALL.len() * Material::ALL.len();

/// Where the crumbles begin. The one place the lists before them are
/// summed, so [`Sfx::index`] and [`slot_count`] cannot drift apart.
fn crumble_base() -> usize {
    MATERIAL_SOUNDS + LOOSE.len() + AMBIENT.len() + WILDLIFE.len() + WEATHER.len() + WORKSHOP.len()
}

/// Where the animals' voices begin.
fn voice_base() -> usize {
    crumble_base() + CRUMBLES.len()
}

/// The float's two sounds, appended after the voices for the reason every
/// list here was appended after the one before it: a slot's index is kept.
const FLOAT: [Sfx; 2] = [Sfx::FloatPlop, Sfx::FloatBite];

/// Where the float's sounds begin.
fn float_base() -> usize {
    voice_base() + VOICES.len()
}

/// How many slots the bank has.
fn slot_count() -> usize {
    float_base() + FLOAT.len()
}

/// Every sound, exactly once. The bank is a `Vec` indexed by
/// [`Sfx::index`], so this and that function are two readings of one
/// numbering and the test at the bottom of the file checks they agree.
pub fn all() -> Vec<Sfx> {
    let mut out = Vec::with_capacity(slot_count());
    for impact in Impact::ALL {
        for material in Material::ALL {
            out.push(Sfx::Material(impact, material));
        }
    }
    out.extend_from_slice(&LOOSE);
    out.extend_from_slice(&AMBIENT);
    out.extend_from_slice(&WILDLIFE);
    out.extend_from_slice(&WEATHER);
    out.extend_from_slice(&WORKSHOP);
    out.extend(CRUMBLES.iter().map(|&material| Sfx::Crumble(material)));
    out.extend(VOICES.iter().map(|&(species, cry)| Sfx::Animal(species, cry)));
    out.extend_from_slice(&FLOAT);
    out
}

impl Sfx {
    /// Where this sound's clips live in the bank.
    #[inline]
    pub fn index(self) -> usize {
        match self {
            Sfx::Material(impact, material) => {
                impact.index() * Material::ALL.len() + material.index()
            }
            Sfx::Crumble(material) => {
                // Asked only for what `crumble_of` handed out; the
                // fallback is the first crumble rather than slot zero, on
                // the animals' reasoning below.
                let crumble = CRUMBLES.iter().position(|m| *m == material).unwrap_or(0);
                crumble_base() + crumble
            }
            Sfx::Animal(species, cry) => {
                // Only ever asked for a pair `voice_of` handed out; the
                // fallback is the first voice rather than slot zero, so a
                // mistake is at least an animal.
                let voice = VOICES.iter().position(|v| *v == (species, cry)).unwrap_or(0);
                voice_base() + voice
            }
            Sfx::FloatPlop | Sfx::FloatBite => {
                float_base() + FLOAT.iter().position(|s| *s == self).unwrap_or(0)
            }
            other => {
                let loose = LOOSE
                    .iter()
                    .chain(AMBIENT.iter())
                    .chain(WILDLIFE.iter())
                    .chain(WEATHER.iter())
                    .chain(WORKSHOP.iter())
                    .position(|s| *s == other)
                    .unwrap_or(0);
                MATERIAL_SOUNDS + loose
            }
        }
    }

    /// What the `.wav` beside the game is called, without the extension
    /// or the variant number.
    ///
    /// Dotted rather than nested in folders, so a resource pack is one
    /// flat directory and `ls` sorts it into families. The textures went
    /// the other way (`terrain/`, `plants/`) because there are two
    /// hundred of them and they are looked at one at a time; these are
    /// looked at as a set.
    pub fn file_name(self) -> String {
        match self {
            Sfx::Material(impact, material) => {
                format!("{}.{}", impact.key(), material.key())
            }
            Sfx::Jump => "player.jump".into(),
            Sfx::Land => "player.land".into(),
            Sfx::Splash => "player.splash".into(),
            Sfx::Swim => "player.swim".into(),
            Sfx::Wade => "player.wade".into(),
            Sfx::Staked => "player.staked".into(),
            Sfx::Bubble => "player.bubble".into(),
            Sfx::Hurt => "player.hurt".into(),
            Sfx::Death => "player.death".into(),
            Sfx::Eat => "player.eat".into(),
            Sfx::Drink => "player.drink".into(),
            Sfx::Swing => "hand.swing".into(),
            Sfx::Hit => "hand.hit".into(),
            Sfx::Pickup => "item.pickup".into(),
            Sfx::Drop => "item.drop".into(),
            Sfx::Equip => "item.equip".into(),
            Sfx::Craft => "item.craft".into(),
            Sfx::ChestOpen => "chest.open".into(),
            Sfx::ChestClose => "chest.close".into(),
            Sfx::Ignite => "fire.ignite".into(),
            Sfx::Click => "ui.click".into(),
            Sfx::Hover => "ui.hover".into(),
            Sfx::Back => "ui.back".into(),
            Sfx::Message => "ui.message".into(),
            Sfx::FireCrackle => "world.fire".into(),
            // **A drip now, and named for it.** This was `world.rain` when
            // the rain was a shower of these; a resource pack's file of
            // that name was a drop, and would now be taken for the whole
            // downpour.
            Sfx::RainTick => "world.drip".into(),
            Sfx::Thunder => "world.thunder".into(),
            Sfx::Wind => "world.wind".into(),
            Sfx::GullCall => "wild.gull".into(),
            Sfx::WingBeats => "wild.wings".into(),
            Sfx::FrogCroak => "wild.frog".into(),
            Sfx::Swarm => "wild.bees".into(),
            Sfx::Rain => "world.rain".into(),
            Sfx::RainSheltered => "world.rain_roof".into(),
            Sfx::WindBreeze => "world.wind_leaves".into(),
            Sfx::WindHowl => "world.wind_howl".into(),
            Sfx::Rustle => "player.rustle".into(),
            Sfx::WorkBench => "work.bench".into(),
            Sfx::WorkMason => "work.mason".into(),
            Sfx::WorkWheel => "work.wheel".into(),
            Sfx::WorkLeather => "work.leather".into(),
            Sfx::WorkAnvil => "work.anvil".into(),
            Sfx::StationOpen => "work.open".into(),
            Sfx::Crumble(material) => format!("crumble.{}", material.key()),
            Sfx::FloatPlop => "fishing.plop".into(),
            Sfx::FloatBite => "fishing.bite".into(),
            Sfx::Animal(species, cry) => format!("wild.{}.{}", species.name(), cry.key()),
        }
    }
}

/// Everything the mixer can play, decoded once and shared.
pub struct Bank {
    /// A resource pack's files, indexed by [`Sfx::index`]. Empty for every
    /// sound the player has not replaced: there is nothing generated behind
    /// the recordings any more.
    clips: Vec<Vec<Arc<Clip>>>,
    pub sample_rate: u32,
    /// How many sounds a resource pack replaced. Only used for the line
    /// printed at startup, which is the one place a player finds out their
    /// pack was seen.
    pub overridden: usize,
    /// The recordings, indexed like `clips`, once they have been decoded
    /// -- see [`Bank::load_recordings`] for why that is later than the rest.
    /// An empty entry is a sound with no recording.
    recordings: OnceLock<Vec<Vec<Arc<Clip>>>>,
    /// Slots a resource pack replaced. A recording never plays over one of
    /// these: the player's file wins over everything built in.
    pinned: Vec<bool>,
    /// Which variant of each sound played last, indexed like `clips`.
    ///
    /// Atomics because the bank is shared behind an `Arc` and picked from
    /// the game thread without a lock; a lost update between two threads
    /// would at worst allow one repeat, which is what there was before.
    last: Vec<AtomicUsize>,
}

impl Bank {
    /// An empty bank for a device running at `sample_rate`: a slot for
    /// every sound, and nothing in any of them until the recordings are
    /// decoded or a resource pack is read.
    pub fn new(sample_rate: u32) -> Bank {
        let rate = sample_rate.max(8_000);
        let slots = slot_count();
        let last = (0..slots).map(|_| AtomicUsize::new(usize::MAX)).collect();
        Bank {
            clips: vec![Vec::new(); slots],
            sample_rate: rate,
            overridden: 0,
            recordings: OnceLock::new(),
            pinned: vec![false; slots],
            last,
        }
    }

    /// Decodes the recordings in [`super::recorded`], at the device's rate.
    ///
    /// **Through `&self`, so it can run on its own thread while the game
    /// is already starting.** Decoding all of them is 0.74 s on a desktop
    /// in release (236 Vorbis files, 4.5 million samples through `lewton`)
    /// and several times that on a phone -- and a phone whose startup does
    /// not service the activity loop is killed by the watchdog at five
    /// seconds (see CLAUDE.md, "Startup must service the loop"). So the
    /// bank starts empty, is handed to the mixer at once, and the
    /// recordings arrive when they are ready: the menu's first click may
    /// be silent, and nothing in a world is. Decoding lazily on first
    /// play was the other option and was rejected: the first footstep on
    /// sand would be a 20 ms hitch in the frame, on every launch.
    ///
    /// Each file is read from `assets_dir/sounds` if it is there and from
    /// the copy compiled into the game if not. A file that will not decode
    /// costs that one variant and a line on stderr; a sound all of whose
    /// files fail is silent, and the test that decodes every file is what
    /// keeps that from shipping. A second call does nothing.
    pub fn load_recordings(&self, assets_dir: &Path) {
        if self.recordings.get().is_some() {
            return;
        }
        let mut decoded = vec![Vec::new(); self.clips.len()];
        for recording in super::recorded::RECORDINGS {
            let slot = &mut decoded[recording.sfx.index()];
            for file in recording.files {
                let Some(bytes) = super::recorded::bytes(assets_dir, file) else {
                    eprintln!("sound {file}: neither on disk nor built in");
                    continue;
                };
                match super::recorded::decode(&bytes, recording.gain, self.sample_rate) {
                    Ok(clip) => slot.push(Arc::new(clip)),
                    Err(e) => eprintln!("sound {file}: {e}"),
                }
            }
        }
        let _ = self.recordings.set(decoded);
    }

    /// How many sounds have a recording to play -- zero until
    /// [`Bank::load_recordings`] has finished.
    pub fn recorded(&self) -> usize {
        (0..self.clips.len())
            .filter(|&slot| self.recording_at(slot).is_some())
            .count()
    }

    /// The recordings for a slot, if it has any and a resource pack has
    /// not replaced it.
    fn recording_at(&self, slot: usize) -> Option<&Vec<Arc<Clip>>> {
        if self.pinned.get(slot).copied().unwrap_or(false) {
            return None;
        }
        self.recordings.get()?.get(slot).filter(|clips| !clips.is_empty())
    }

    /// What a slot plays now: a resource pack's files if it has any, else
    /// the recordings, else nothing.
    fn playing(&self, slot: usize) -> Option<&Vec<Arc<Clip>>> {
        self.recording_at(slot).or_else(|| self.clips.get(slot).filter(|clips| !clips.is_empty()))
    }

    /// How many variants a sound has now -- recordings, or a resource
    /// pack's files. Only the tests ask.
    #[cfg(test)]
    pub fn variants(&self, sfx: Sfx) -> usize {
        self.playing(sfx.index()).map_or(0, Vec::len)
    }

    /// Replaces a sound's recordings with `.wav` files from `dir`.
    ///
    /// Two spellings, matching the textures: `dig.stone.wav` replaces
    /// every variant with one sound, and `dig.stone.0.wav`,
    /// `dig.stone.1.wav`, ... replace them with as many as there are,
    /// read until the first number missing. The numbered form is what
    /// `--export-sounds` writes, so a pack made by editing an export keeps
    /// its variety -- all ten rustles, not the three a recipe used to make.
    ///
    /// Missing folder is not an error -- almost nobody has one. A file
    /// that is there and unreadable *is* worth a line on stderr: the
    /// player put it there on purpose.
    pub fn load_overrides(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        for sfx in all() {
            let base = sfx.file_name();
            let slot = sfx.index();

            let single = dir.join(format!("{base}.wav"));
            if single.is_file() {
                match read_wav(&single, self.sample_rate) {
                    Ok(clip) => {
                        self.clips[slot] = vec![Arc::new(clip)];
                        self.pinned[slot] = true;
                        self.overridden += 1;
                        continue;
                    }
                    Err(e) => eprintln!("sound {}: {e}", single.display()),
                }
            }

            let mut replaced = false;
            for variant in 0..MAX_PACK_VARIANTS {
                let path = dir.join(format!("{base}.{variant}.wav"));
                if !path.is_file() {
                    break;
                }
                match read_wav(&path, self.sample_rate) {
                    Ok(clip) => {
                        if !replaced {
                            // The first file found takes the whole slot,
                            // so a pack that supplies one variant does not
                            // leave the recordings alternating with it --
                            // which sounds like a bug rather than variety.
                            self.clips[slot].clear();
                            replaced = true;
                        }
                        self.clips[slot].push(Arc::new(clip));
                    }
                    Err(e) => eprintln!("sound {}: {e}", path.display()),
                }
            }
            if replaced {
                self.pinned[slot] = true;
                self.overridden += 1;
            }
        }
    }

    /// One of this sound's variants, at random -- and never the one that
    /// played last, when there is another. See
    /// [`super::recorded::next_variant`] for why.
    #[inline]
    pub fn pick(&self, sfx: Sfx, rng: &mut Rng) -> Option<Arc<Clip>> {
        let slot = sfx.index();
        let variants = self.playing(slot)?;
        if variants.is_empty() {
            return None;
        }
        let last = &self.last[slot];
        let chosen = super::recorded::next_variant(variants.len(), last.load(Ordering::Relaxed), rng.next_u64());
        last.store(chosen, Ordering::Relaxed);
        Some(variants[chosen].clone())
    }

    /// Writes every sound the bank plays out as `.wav`, one file per
    /// variant; a silent sound writes nothing.
    ///
    /// Answers the paths written, so `--export-sounds` can print them.
    pub fn export(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let mut written = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        for sfx in all() {
            let base = sfx.file_name();
            for (variant, clip) in self.playing(sfx.index()).into_iter().flatten().enumerate() {
                let path = dir.join(format!("{base}.{variant}.wav"));
                let mut writer = hound::WavWriter::create(&path, spec)
                    .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
                for sample in &clip.samples {
                    writer
                        .write_sample(*sample)
                        .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
                }
                writer
                    .finalize()
                    .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
                written.push(path);
            }
        }
        Ok(written)
    }

    /// Total memory a resource pack's clips occupy, for the startup line.
    pub fn bytes(&self) -> usize {
        self.clips
            .iter()
            .flatten()
            .map(|clip| clip.len() * std::mem::size_of::<i16>())
            .sum()
    }

    pub fn clip_count(&self) -> usize {
        self.clips.iter().map(Vec::len).sum()
    }

    /// How much audio a resource pack gave, in seconds. Printed at startup
    /// beside the byte count, because the two together say what was read --
    /// a file that has quietly become ten times too long shows up here.
    pub fn seconds(&self) -> f32 {
        self.clips.iter().flatten().map(|clip| clip.seconds()).sum()
    }
}

/// Reads one `.wav` and converts it to what the mixer plays: mono,
/// 16-bit, at the device's rate.
///
/// Every format `hound` can open is accepted, because the whole point of
/// the folder is that somebody drops a file in it, and "unsupported bit
/// depth" is a worse answer than a conversion. Stereo is summed rather
/// than taking the left channel -- a file whose two channels are
/// opposite phases would otherwise cancel to nothing, and someone would
/// spend an evening on it.
fn read_wav(path: &Path, device_rate: u32) -> Result<Clip, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            // The full-scale value for this depth, so 8-, 16-, 24- and
            // 32-bit files all arrive at the same loudness.
            let scale = 1.0 / (1i64 << (spec.bits_per_sample.max(2) - 1)) as f32;
            reader
                .samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|s| s as f32 * scale)
                .collect()
        }
    };

    let mono: Vec<i16> = raw
        .chunks(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().sum();
            ((sum / channels as f32).clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect();

    let clip = Clip { samples: mono, sample_rate: spec.sample_rate.max(1) };
    Ok(clip.resampled(device_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::*;
    use std::collections::HashMap;

    /// The bank is a `Vec` indexed by `Sfx::index`, and `all()` is what
    /// fills it. If those two ever disagree, some sound is silent and
    /// some other sound is playing in its place -- which is exactly the
    /// kind of bug that gets diagnosed as "the audio is broken".
    #[test]
    fn every_sound_has_its_own_slot() {
        let sounds = all();
        let mut seen = vec![false; sounds.len()];
        for sfx in &sounds {
            let index = sfx.index();
            assert!(index < sounds.len(), "{sfx:?} indexes past the bank");
            assert!(!seen[index], "{sfx:?} shares slot {index}");
            seen[index] = true;
        }
        assert!(seen.iter().all(|s| *s), "a slot in the bank is never filled");
    }

    /// Four workshops, four sounds, and nothing else claims one.
    ///
    /// The point of failure this guards is a fifth workshop added to
    /// `crafting::Station` and quietly mapped onto `None`: it would craft
    /// perfectly and sound like a pair of hands, which is the one bug in
    /// this file nobody reports because nothing is wrong with it.
    #[test]
    fn every_workshop_is_heard_as_itself_and_the_hands_are_not() {
        use primitive_shared::crafting::Station;
        let mut heard = Vec::new();
        for station in Station::WORKSHOPS {
            let sfx = workshop_of(station)
                .unwrap_or_else(|| panic!("{station:?} is a workshop with no sound"));
            assert!(!heard.contains(&sfx.index()), "two workshops share {}", sfx.file_name());
            heard.push(sfx.index());
        }
        for station in [Station::Hands, Station::Heat, Station::Forge, Station::Bloomery] {
            assert!(workshop_of(station).is_none(), "{station:?} is not a workshop and has a workshop's sound");
        }
    }

    /// A workshop's screen is not a chest's lid, and the test says so
    /// because the two were the same sound and it was wrong in a way that
    /// reads as "the anvil is a box".
    #[test]
    fn a_workshop_opening_is_not_the_chest_being_opened() {
        assert_ne!(Sfx::StationOpen.index(), Sfx::ChestOpen.index());
        assert_ne!(Sfx::StationOpen.file_name(), Sfx::ChestOpen.file_name());
    }

    /// Only what can shed debris has a crumble, and asking about anything
    /// else answers `None` rather than somebody else's slot.
    #[test]
    fn only_what_can_shed_debris_crumbles() {
        for material in CRUMBLES {
            let sfx = crumble_of(material).unwrap_or_else(|| panic!("{} lost its crumble", material.key()));
            assert_eq!(sfx, Sfx::Crumble(material));
        }
        for material in Material::ALL {
            if CRUMBLES.contains(&material) {
                continue;
            }
            assert!(
                crumble_of(material).is_none(),
                "{} crumbles, and nothing was recorded for it",
                material.key()
            );
        }
    }

    /// **Every rock, soil, rubble and ore that comes away in quarters has
    /// something to sound like when a quarter goes.** `on_slice_taken` asks
    /// the crumble of the block's material and plays nothing if there is
    /// none; a new soil whose name fell through to cloth or metal would dig
    /// in silence, and this is where that is caught. And they are the four
    /// the dig was asked for by name: stone, soil, gravel and sand.
    #[test]
    fn every_block_that_digs_in_slices_has_a_sound_for_a_quarter_coming_off() {
        let mut heard = Vec::new();
        for kind in 0..(1 as BlockId) << 10 {
            if !primitive_shared::dig::digs_in_slices(kind) || !primitive_shared::types::is_known_block(kind) {
                continue;
            }
            let material = Material::of(kind);
            assert!(
                slice_of(material).is_some(),
                "{} digs in quarters and sounds like {}, which has nothing to shed",
                primitive_shared::types::block_name(kind),
                material.key()
            );
            heard.push(material);
        }
        for material in [Material::Stone, Material::Dirt, Material::Gravel, Material::Sand] {
            assert!(heard.contains(&material), "nothing that digs in quarters sounds like {}", material.key());
            assert_eq!(slice_of(material), Some(Sfx::Crumble(material)), "{} sheds somebody else's debris", material.key());
        }
    }

    /// A burning board ends as a charred one, and both are timber.
    ///
    /// `sound_for` in `lib.rs` plays the wooden crumble for that change,
    /// and it asks this classifier what the block was. A rename in
    /// `blocks.toml` that dropped "planks" out of the name would fall
    /// through to `Matter::Solid` and a burning house would rain gravel.
    #[test]
    fn a_burning_board_is_still_timber() {
        use primitive_shared::types::{BLOCK_BURNING_LOG, BLOCK_BURNING_PLANKS, BLOCK_CHARRED_LOG, BLOCK_CHARRED_PLANKS};
        for block in [BLOCK_BURNING_PLANKS, BLOCK_BURNING_LOG, BLOCK_CHARRED_PLANKS, BLOCK_CHARRED_LOG] {
            assert_eq!(Material::of(block), Material::Wood, "{} is not heard as timber", block_name(block_kind(block)));
        }
    }

    #[test]
    fn names_are_unique_and_file_safe() {
        let mut seen = HashMap::new();
        for sfx in all() {
            let name = sfx.file_name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{name} is not a safe filename"
            );
            assert!(
                seen.insert(name.clone(), sfx).is_none(),
                "two sounds are both called {name}"
            );
        }
    }

    /// The classifier is the one part of this file that a change
    /// elsewhere in the game can quietly break -- a block renamed in
    /// `blocks.toml` falls straight through to its `Matter`. These are
    /// the judgements that would be wrong in a way a player notices.
    #[test]
    fn cutting_an_animal_sounds_like_flesh_and_its_bones_like_bones_never_like_rock() {
        use primitive_shared::types::{BLOCK_BONES, BLOCK_CARCASS_BEAR, BLOCK_CARCASS_DEER, BLOCK_CARCASS_HARE, BLOCK_RAW_MEAT};
        for carcass in [BLOCK_CARCASS_DEER, BLOCK_CARCASS_BEAR, BLOCK_CARCASS_HARE, BLOCK_RAW_MEAT] {
            assert_eq!(Material::of(carcass), Material::Cloth, "{} rings like a pick on rock", block_name(carcass));
        }
        assert_eq!(Material::of(BLOCK_BONES), Material::Gravel);
    }

    #[test]
    fn materials_are_classified_by_what_they_are() {
        assert_eq!(Material::of(BLOCK_STONE), Material::Stone);
        assert_eq!(Material::of(BLOCK_COBBLESTONE), Material::Gravel);
        assert_eq!(Material::of(BLOCK_WATER), Material::Liquid);
        assert_eq!(Material::of(BLOCK_GRASS), Material::Grass);
        assert_eq!(Material::of(BLOCK_LEAVES), Material::Grass);
        assert_eq!(Material::of(BLOCK_SAND), Material::Sand);
        assert_eq!(Material::of(BLOCK_SNOW), Material::Snow);
        assert_eq!(Material::of(BLOCK_ICE), Material::Glass);
        assert_eq!(Material::of(BLOCK_PLANKS), Material::Wood);
        assert_eq!(Material::of(BLOCK_LOG), Material::Wood);
        assert_eq!(Material::of(BLOCK_CHEST), Material::Wood);
        // Ore is rock, however it is named. Metal is what comes out of
        // it.
        assert_eq!(Material::of(BLOCK_IRON_ORE), Material::Stone);
        assert_eq!(Material::of(BLOCK_COPPER_ORE), Material::Stone);
        assert_eq!(Material::of(BLOCK_NATIVE_COPPER), Material::Stone);
        assert_eq!(Material::of(BLOCK_IRON_INGOT), Material::Metal);
        assert_eq!(Material::of(BLOCK_BRONZE_PICKAXE), Material::Metal);
        // A flint tool is a stone edge; a leather boot is not.
        assert_eq!(Material::of(BLOCK_STONE_PICKAXE), Material::Stone);
        assert_eq!(Material::of(BLOCK_LEATHER_BOOTS), Material::Cloth);
        assert_eq!(Material::of(BLOCK_HIDE), Material::Cloth);
        // Wet clay is soil; the pot it becomes is not.
        assert_eq!(Material::of(BLOCK_CLAY), Material::Dirt);
        assert_eq!(Material::of(BLOCK_VESSEL_RAW), Material::Dirt);
        assert_eq!(Material::of(BLOCK_VESSEL), Material::Ceramic);
        assert_eq!(Material::of(BLOCK_BRICKS), Material::Ceramic);
    }

    /// Every block in the game gets *some* material, and no lookup
    /// panics on an id that is not a block at all.
    #[test]
    fn nothing_is_unclassifiable() {
        for id in 0..=255u16 {
            let _ = Material::of(id as BlockId);
        }
    }

    fn assets() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets")
    }

    /// One bank with every recording decoded, shared by the tests that
    /// measure them. Decoding four hundred files is the slow part of this
    /// module's tests, and doing it once rather than once a test is the
    /// difference between a second and ten.
    fn recorded_bank() -> &'static Bank {
        static BANK: OnceLock<Bank> = OnceLock::new();
        BANK.get_or_init(|| {
            let bank = Bank::new(22_050);
            bank.load_recordings(&assets());
            bank
        })
    }

    /// Every variant a sound plays, in the order the table lists them.
    fn clips(sfx: Sfx) -> &'static [Arc<Clip>] {
        recorded_bank().playing(sfx.index()).map_or(&[], |v| v.as_slice())
    }

    /// The whole `hound` story, both directions, in one test: decode the
    /// recordings, write them out, read them back into a second bank as a
    /// resource pack, and check the second one plays the files.
    ///
    /// Worth a test that touches the disk because it is the one part of
    /// the audio system a player is invited to take apart, and because
    /// the two halves are written against a filename convention that
    /// nothing else would notice going wrong. **Every variant comes back**,
    /// not the first three: a pack made from an export of ten rustles used
    /// to be read as three, because the loader stopped where the recipes'
    /// variant count did.
    #[test]
    fn a_bank_survives_a_trip_through_the_disk() {
        let dir = std::env::temp_dir().join(format!("primitive-sound-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let bank = recorded_bank();
        let written = bank.export(&dir).expect("exported");
        let variants: usize = all().iter().map(|sfx| bank.variants(*sfx)).sum();
        assert_eq!(written.len(), variants);

        let mut reloaded = Bank::new(22_050);
        reloaded.load_overrides(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let sounding = all().iter().filter(|sfx| bank.variants(**sfx) > 0).count();
        assert_eq!(reloaded.overridden, sounding, "every sound should have been replaced by its own export");

        for sfx in [Sfx::Material(Impact::Break, Material::Stone), Sfx::Rustle] {
            assert_eq!(reloaded.variants(sfx), bank.variants(sfx), "{} lost variants on the way back", sfx.file_name());
            let before = &clips(sfx)[0];
            let after = &reloaded.clips[sfx.index()][0];
            // The export is 16-bit mono at the bank's own rate, so nothing
            // in the path may resample it.
            assert_eq!(before.sample_rate, after.sample_rate);
            assert_eq!(before.samples.len(), after.samples.len());
        }
    }

    /// How much of a clip is a *note*, 0..1.
    ///
    /// The peak of the normalised autocorrelation over musical lags. A
    /// pure sine repeats itself exactly one period later and scores
    /// close to 1; filtered noise never repeats and scores low; a damped
    /// resonance sits in between, which is where an impact belongs.
    ///
    /// Decimated by four before measuring, which costs nothing -- the
    /// lags that matter are tens of samples long -- and turns a test
    /// that took a second into one that takes a tenth.
    fn tonality(clip: &Clip) -> f32 {
        let rate = clip.sample_rate as f32 / 4.0;
        let all: Vec<f32> = clip.samples.iter().step_by(4).map(|s| *s as f32 / i16::MAX as f32).collect();
        if all.len() < 64 {
            return 0.0;
        }
        // Centred on the loudest sample, so a long silent tail does not
        // dilute the answer.
        let window = all.len().min((rate * 0.25) as usize);
        let peak_at = all
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let start = peak_at.saturating_sub(window / 4).min(all.len() - window);
        let x = &all[start..start + window];

        let mean = x.iter().sum::<f32>() / x.len() as f32;
        let x: Vec<f32> = x.iter().map(|v| v - mean).collect();
        if x.iter().map(|v| v * v).sum::<f32>() < 1e-9 {
            return 0.0;
        }

        // 60 Hz to 2 kHz: everything a person would hear as a pitch.
        let low = ((rate / 2000.0) as usize).max(4);
        let high = ((rate / 60.0) as usize).min(x.len() - 1);
        // Normalised by the energy of the *whole* window rather than of
        // the overlap. Dividing by the overlap is the unbiased estimator
        // and it is the wrong one here: at long lags the overlap is a
        // handful of samples and the ratio spikes on noise, so a 90 ms
        // footstep scored higher than a sine. The biased form falls away
        // with lag, which is exactly the statement being made -- a note
        // has to last long enough to be heard as one.
        let energy: f32 = x.iter().map(|v| v * v).sum();
        let mut best = 0.0f32;
        for lag in low..high {
            let mut dot = 0.0;
            for i in 0..(x.len() - lag) {
                dot += x[i] * x[i + lag];
            }
            best = best.max(dot / energy);
        }
        best
    }

    /// **The regression test for the complaint that started the sound
    /// work**, now held against the recordings.
    ///
    /// The first bank sounded like a role-playing game from 1991, and the
    /// cause was measurable: a third of the recipes were pure tones. The
    /// recipes are gone, and real sounds are not automatically
    /// not-jingles -- an interface pack's "click" is as often a sine blip as
    /// a knock, and a creak *is* a pitch, sliding. So every recording is
    /// measured the way the recipes were.
    ///
    /// **Left out, because they are physics and not indulgence**: a bubble
    /// and a drop of water ring at the pitch their size gives them; struck
    /// metal, glass and pottery ring; and an animal's voice is a vocal
    /// tract driven by a pulse, which is to say a pitch -- a howl that
    /// passed this measure would not be a howl. A recorded gust that
    /// whistled did fail it (0.62 and 0.64) and was not shipped; the wind
    /// is cut from a recording that does not whistle instead.
    ///
    /// Two thresholds, because there are two kinds of sound: a thing
    /// struck (a material, a landing, a blow) has a damped resonance that
    /// is the whole of what separates wood from stone, and is held to the
    /// looser line; everything else has nothing that could legitimately
    /// ring and is held to the strict one.
    #[test]
    fn nothing_in_the_bank_is_a_note() {
        for sfx in all() {
            let limit = match sfx {
                // The bite is the same single bubbles, and held to the same
                // physics.
                Sfx::Bubble | Sfx::FloatBite | Sfx::RainTick | Sfx::GullCall | Sfx::FrogCroak | Sfx::Animal(..) => continue,
                Sfx::Material(_, Material::Metal | Material::Glass | Material::Ceramic) => continue,
                // An anvil is a tuned lump of steel and a hammer on it
                // rings for a second and a half; that ring is the sound,
                // and a recording that passed the measure would be a
                // hammer on a sandbag. Exempt for exactly the reason
                // struck metal above it is.
                Sfx::WorkAnvil => continue,
                // The other four workshops are things struck, scraped and
                // cut, which is the looser line's own description.
                Sfx::Material(..) | Sfx::Crumble(..) | Sfx::WorkBench | Sfx::WorkMason | Sfx::WorkWheel
                | Sfx::WorkLeather | Sfx::StationOpen | Sfx::Land | Sfx::Hit | Sfx::Hurt | Sfx::Death
                // Wood knocked and a body stopped is a blow, and scored
                // as the other blows are.
                | Sfx::Staked
                // A float striking the water is a thing struck, the way the
                // hand in the pool it was cut from always was.
                | Sfx::FloatPlop => 0.80,
                _ => 0.62,
            };
            for (variant, clip) in clips(sfx).iter().enumerate() {
                let score = tonality(clip);
                assert!(
                    score < limit,
                    "{} variant {variant} scored {score:.3} against {limit}: it is a note, not a sound",
                    sfx.file_name()
                );
            }
        }
    }

    /// The measure has to be able to tell the two apart, or the test
    /// above passes because it measures nothing.
    #[test]
    fn the_tonality_measure_knows_a_tone_from_noise() {
        let rate = 22_050u32;
        let clip = |samples: Vec<i16>| Clip { samples, sample_rate: rate };
        let pure = (0..rate as usize / 5)
            .map(|i| ((std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 26_000.0) as i16)
            .collect();
        assert!(tonality(&clip(pure)) > 0.9, "a sine did not read as a tone");

        let mut rng = Rng::new(11);
        let hiss = (0..rate as usize / 5).map(|_| (rng.bipolar() * 26_000.0) as i16).collect();
        assert!(tonality(&clip(hiss)) < 0.3, "noise read as a tone");
    }

    /// Loudness of `samples` at `rate` in windows of `window` seconds,
    /// leaving out `skip` seconds at either end (a bed's crossfades).
    fn windows(samples: &[f32], rate: u32, window: f32, skip: f32) -> Vec<f32> {
        let rate = rate as f32;
        let skip = (skip * rate) as usize;
        let size = ((window * rate) as usize).max(1);
        let end = samples.len().saturating_sub(skip);
        samples[skip.min(end)..end]
            .chunks_exact(size)
            .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / size as f32).sqrt())
            .collect()
    }

    fn unit(clip: &Clip) -> Vec<f32> {
        clip.samples.iter().map(|s| *s as f32 / i16::MAX as f32).collect()
    }

    fn median(values: &[f32]) -> f32 {
        let mut sorted = values.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
    }

    /// How much a clip is a string of separate events: the loudest five
    /// milliseconds of it against the typical five milliseconds.
    ///
    /// A steady noise sits near 1.3 on this -- five milliseconds is a couple
    /// of hundred samples, and that many samples of noise vary by about that
    /// much. A tick every twenty-five milliseconds is loud in the window it
    /// lands in and nearly silent in the four after it, and scores many
    /// times higher.
    fn clickiness(samples: &[f32], rate: u32, skip: f32) -> f32 {
        let levels = windows(samples, rate, 0.005, skip);
        let typical = median(&levels).max(1e-6);
        levels.iter().fold(0.0f32, |m, v| m.max(*v)) / typical
    }

    /// **The regression test for "a click at every drop".** Rain was once
    /// forty thirty-millisecond ticks a second, and that is what the player
    /// heard: ticks. Rain at the ear is a band of noise, and a recording of
    /// a shower close to one hard surface is ticks again -- which is why the
    /// open-sky rain is a steady heavy shower and the roof is a shed roof
    /// taken below 5 kHz. This holds both to it.
    #[test]
    fn rain_is_a_band_of_noise_not_a_string_of_clicks() {
        for sfx in [Sfx::Rain, Sfx::RainSheltered] {
            assert!(!clips(sfx).is_empty(), "{} has no recording", sfx.file_name());
            for (variant, clip) in clips(sfx).iter().enumerate() {
                let score = clickiness(&unit(clip), clip.sample_rate, BED_FADE);
                assert!(
                    score < 2.2,
                    "{} variant {variant} has a five-millisecond window {score:.2} times the typical one: it is drops, not rain",
                    sfx.file_name()
                );
            }
        }
    }

    /// ...and the measure hears the rain that was: forty ticks a second.
    #[test]
    fn the_click_measure_hears_the_rain_that_was_clicks() {
        let rate = 22_050u32;
        let mut rng = Rng::new(77);
        let mut shower = vec![0.0f32; (BED_SECONDS * rate as f32) as usize];
        let mut at = 0.0;
        while at < BED_SECONDS {
            let start = (at * rate as f32) as usize;
            let loud = rng.range(0.2, 0.3);
            for i in 0..(0.03 * rate as f32) as usize {
                if let Some(s) = shower.get_mut(start + i) {
                    *s += rng.bipolar() * loud * (-(i as f32) / 40.0).exp();
                }
            }
            at += 1.0 / 40.0 * rng.range(0.5, 1.5);
        }
        let score = clickiness(&shower, rate, BED_FADE);
        assert!(score > 4.0, "forty ticks a second scored only {score:.2}");
    }

    /// **A bed is cut to exactly the length the soundscape lays it at.**
    /// The pieces are laid `SECONDS - FADE` apart; a recording a quarter of
    /// a second short is a quarter-second hole in the rain every three.
    #[test]
    fn every_bed_is_as_long_as_the_soundscape_lays_it() {
        for (sfx, seconds) in [(Sfx::Rain, BED_SECONDS), (Sfx::RainSheltered, BED_SECONDS), (Sfx::FireCrackle, FIRE_SECONDS)] {
            for clip in clips(sfx) {
                assert!(
                    (clip.seconds() - seconds).abs() < 0.02,
                    "{} lasts {:.2} s and is laid as {seconds} s",
                    sfx.file_name(),
                    clip.seconds()
                );
            }
        }
    }

    /// The soundscape lays a bed's pieces `SECONDS - FADE` apart. Laid that
    /// way, the level must not dip or swell at the joins -- a dip every two
    /// and a half seconds is rain that breathes like a machine. The
    /// recordings are levelled by energy rather than by their loudest
    /// sample for this: a fire piece with one big pop in it, peak-normalised,
    /// is a quiet piece.
    #[test]
    fn a_bed_laid_end_over_end_has_no_seam() {
        for (sfx, seconds, fade) in [
            (Sfx::Rain, BED_SECONDS, BED_FADE),
            (Sfx::RainSheltered, BED_SECONDS, BED_FADE),
            (Sfx::FireCrackle, FIRE_SECONDS, FIRE_FADE),
        ] {
            let pieces = clips(sfx);
            let rate = pieces[0].sample_rate;
            let step = seconds - fade;
            let count = 6;
            let mut laid = vec![0.0f32; ((step * count as f32 + fade + 0.1) * rate as f32) as usize];
            for piece in 0..count {
                let start = (piece as f32 * step * rate as f32) as usize;
                for (i, s) in unit(&pieces[piece % pieces.len()]).iter().enumerate() {
                    if let Some(slot) = laid.get_mut(start + i) {
                        *slot += s;
                    }
                }
            }
            let levels = windows(&laid, rate, 0.2, seconds);
            let typical = median(&levels);
            let quietest = levels.iter().fold(f32::MAX, |m, v| m.min(*v));
            assert!(
                quietest > typical * 0.7,
                "{} dips to {:.2} of its level somewhere along the bed",
                sfx.file_name(),
                quietest / typical
            );
        }
    }

    /// A fire is a bed with the odd pop in it, not a string of snaps: most
    /// of it sits at its typical level, and only a few of its moments stand
    /// well out of it. **The regression test for the fire the player sent
    /// back**, which was a CC0 recording of separate snaps with silence
    /// between them.
    #[test]
    fn a_fire_is_a_dense_bed_not_a_string_of_snaps() {
        for (variant, clip) in clips(Sfx::FireCrackle).iter().enumerate() {
            let levels = windows(&unit(clip), clip.sample_rate, 0.01, FIRE_FADE);
            let typical = median(&levels);
            let standing_out = levels.iter().filter(|v| **v > typical * 2.5).count();
            let share = standing_out as f32 / levels.len() as f32;
            assert!(
                share < 0.04,
                "fire variant {variant}: {:.0}% of it stands out of the bed -- that is snaps, not a fire",
                share * 100.0
            );
        }
    }

    /// Where a clip's energy sits, as a frequency: the energy-weighted mean
    /// of twenty log-spaced band-pass outputs from 150 Hz to 7 kHz, taken in
    /// octaves. A plain two-pole band-pass per band, which is all a
    /// centroid needs.
    fn centroid(clip: &Clip) -> f32 {
        let x = unit(clip);
        let rate = clip.sample_rate as f32;
        let bands = 20;
        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        for band in 0..bands {
            let centre = 150.0 * (7000.0f32 / 150.0).powf(band as f32 / (bands - 1) as f32);
            if centre > rate * 0.45 {
                continue;
            }
            // RBJ band-pass, constant peak gain, Q of 2.
            let w = std::f32::consts::TAU * centre / rate;
            let alpha = w.sin() / 4.0;
            let a0 = 1.0 + alpha;
            let (b0, b2, a1, a2) = (alpha / a0, -alpha / a0, -2.0 * w.cos() / a0, (1.0 - alpha) / a0);
            let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            let mut energy = 0.0f32;
            for &v in &x {
                let y = b0 * v + b2 * x2 - a1 * y1 - a2 * y2;
                x2 = x1;
                x1 = v;
                y2 = y1;
                y1 = y;
                energy += y * y;
            }
            weighted += centre.log2() * energy;
            total += energy;
        }
        (weighted / total.max(1e-12)).exp2()
    }

    /// **Three winds, not one wind at three volumes.** A breeze is heard as
    /// the leaves it moves, so its energy sits high; a gust on open ground
    /// and a howl are the air itself, and sit low.
    #[test]
    fn the_breeze_is_leaves_and_the_other_winds_are_air() {
        let mean = |sfx: Sfx| {
            let pieces = clips(sfx);
            assert!(!pieces.is_empty(), "{} has no recording", sfx.file_name());
            pieces.iter().map(|c| centroid(c)).sum::<f32>() / pieces.len() as f32
        };
        let (breeze, gust, howl) = (mean(Sfx::WindBreeze), mean(Sfx::Wind), mean(Sfx::WindHowl));
        assert!(breeze > gust * 1.5, "the breeze centres at {breeze:.0} Hz and the gust at {gust:.0}");
        assert!(breeze > howl * 1.5, "the breeze centres at {breeze:.0} Hz and the howl at {howl:.0}");
    }

    /// **Every animal in the world has something to say**, and says it in its
    /// own voice: a wound and a flight for all of them, a death for all of
    /// them, and no two species sharing a slot.
    #[test]
    fn every_species_has_a_voice() {
        // Who a sound belongs to. Every species is its own, except the three
        // swimmers that are heard as the school: see `voice_of` for why one
        // recording of disturbed water is the truth about all of them, and
        // not a gap waiting for three more files.
        let holder = |species: Species| match species {
            Species::Trout | Species::Pike | Species::Herring => Species::Fish,
            // ...and the rat, which is heard as a hare: see `voice_of`.
            Species::Rat => Species::Hare,
            other => other,
        };
        let mut owner: HashMap<usize, Species> = HashMap::new();
        for &species in Species::ALL {
            for cry in [Cry::Alarm, Cry::Hurt, Cry::Death] {
                assert!(voice_of(species, cry).is_some(), "a {} has no {} sound", species.name(), cry.key());
            }
            for cry in [Cry::Idle, Cry::Alarm, Cry::Threat, Cry::Hurt, Cry::Death] {
                let Some(sfx) = voice_of(species, cry) else {
                    continue;
                };
                assert!(all().contains(&sfx), "{} is not in the bank", sfx.file_name());
                assert!(!clips(sfx).is_empty(), "{} has no recording", sfx.file_name());
                if let Some(other) = owner.insert(sfx.index(), holder(species)) {
                    assert_eq!(
                        other,
                        holder(species),
                        "a {} and a {} share {}",
                        other.name(),
                        species.name(),
                        sfx.file_name()
                    );
                }
            }
        }
        // Everything that hunts or fights back can be heard coming.
        for species in [Species::Boar, Species::Wolf, Species::Bear, Species::Lion] {
            assert!(voice_of(species, Cry::Threat).is_some(), "a {} charges in silence", species.name());
        }
    }

    /// A voice is the one thing here that is heard as high or low, so it is
    /// worth knowing the recordings chosen are: a bear's growl sits well
    /// below a hare's scream.
    #[test]
    fn a_big_animal_sounds_bigger_than_a_small_one() {
        let at = |species, cry| {
            let pieces = clips(Sfx::Animal(species, cry));
            pieces.iter().map(|c| centroid(c)).sum::<f32>() / pieces.len().max(1) as f32
        };
        let bear = at(Species::Bear, Cry::Threat);
        let hare = at(Species::Hare, Cry::Hurt);
        let sheep = at(Species::Sheep, Cry::Idle);
        // An octave was the line for the recipes, whose hare was a pure
        // squeal. A real rabbit's squeak carries its breath and the ground
        // under it, and the recordings measure 905 Hz against the bear's
        // 466: most of an octave, which is what the ear needs to hear a
        // small animal and a big one.
        assert!(hare > bear * 1.6, "a hare's scream centres at {hare:.0} Hz and a bear's growl at {bear:.0}");
        assert!(sheep > bear, "a sheep at {sheep:.0} Hz is lower than a bear at {bear:.0}");
    }

    /// **Every sound the game can ask for is a recording**, short enough to
    /// place and loud enough to hear -- and the ones written down as silent
    /// really are, rather than being quietly synthesised by something.
    #[test]
    fn every_sound_the_game_asks_for_is_recorded_and_audible() {
        let bank = recorded_bank();
        let silent: Vec<Sfx> = super::super::recorded::SILENT.iter().map(|(sfx, _)| *sfx).collect();
        for sfx in all() {
            let clip = bank.pick(sfx, &mut Rng::new(1));
            if silent.contains(&sfx) {
                assert!(clip.is_none(), "{} is written down as silent and plays something", sfx.file_name());
                continue;
            }
            let clip = clip.unwrap_or_else(|| panic!("{} has nothing to play", sfx.file_name()));
            assert!(clip.seconds() < 5.0, "{} lasts {} seconds", sfx.file_name(), clip.seconds());
            assert!(clip.samples.iter().any(|s| s.unsigned_abs() > 512), "{} is inaudible", sfx.file_name());
        }
    }
}
