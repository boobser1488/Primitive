//! The recordings: which real sound plays for which [`Sfx`], and how a
//! file becomes a clip.
//!
//! ## Why there are recordings at all now
//!
//! The sounds were recipes first -- noise through resonances, the second
//! answer to "the game sounds like a 1991 role-playing game". They were
//! good at being *not a jingle* and not good at being grass under a boot,
//! a crunch of teeth through a root, or a gull. The player asked for real
//! sounds for every action, from free sources, with no strings attached;
//! then, when a fire, the rain, three winds and every animal's voice were
//! still recipes, ordered *all* of them downloaded and everything generated
//! deleted. So this table is now the whole of what the game can play.
//!
//! **Only CC0.** Every file here is a public-domain dedication (Kenney's
//! packs, CC0-marked OpenGameArt uploads, and Freesound sounds whose own
//! page says "Creative Commons 0"); `assets/sounds/SOURCES.md` names the
//! page and the original for each one, and a test fails if a file is
//! shipped without its line there. CC-BY was rejected even where it
//! sounded better: attribution is an obligation that has to survive every
//! fork, APK and screenshot of the credits, and the brief was "nothing
//! owed".
//!
//! ## Silence, never a recipe
//!
//! The order a sound is decided in, at startup:
//!
//! 1. the recording from this table, decoded on its own thread;
//! 2. a resource pack's `assets/sounds/<name>.wav` ([`super::bank::Bank::load_overrides`]),
//!    because the rule since the first texture pack is that what the
//!    player put in the folder wins.
//!
//! A sound with neither is silent. [`SILENT`] lists the sounds that have
//! no recording, each with the reason, so "no row" is a decision somebody
//! wrote down rather than a row somebody forgot --
//! `every_sound_is_either_recorded_or_says_why_not` holds the two lists to
//! exactly `bank::all()`. There used to be a list of sounds that stayed
//! recipes beside it; it is gone with the recipes.
//!
//! ## Why Vorbis, and why `lewton`
//!
//! Three ways to carry four hundred short clips (about six minutes of
//! audio) were weighed:
//!
//! * **16-bit WAV**, which `hound` already reads: about 9 MB, embedded in
//!   an eight-megabyte executable and packed again into every APK. Rejected
//!   on size alone.
//! * **`symphonia`**: decodes everything, and is a dozen crates to link
//!   for one format.
//! * **Ogg Vorbis through `lewton`** -- chosen. Pure Rust (nothing for the
//!   NDK to compile), one small crate plus `ogg`, and the whole set is
//!   3.95 MB at quality 0.3 mono (0.1 to 0.2 at 24 kHz for the beds, winds
//!   and animals, 16 kHz for the horse), which on foley, rain and cries is not something anybody
//!   can hear.
//!
//! Decoding happens once, on its own thread while the game starts (it
//! is most of a second even on a desktop -- see `Bank::load_recordings`),
//! into the `i16` clips the mixer plays.
//!
//! ## Why a table in code and not a folder scan
//!
//! The same reason `embedded::TEXTURES` is a list: `include_bytes!` needs
//! a literal path, and an APK can open an asset by name but cannot be
//! relied on to list a directory. A forgotten file is a failing test
//! naming it, not a sound that is quietly silent on one platform and a
//! recording on another.

use std::borrow::Cow;
use std::path::Path;

use primitive_shared::animals::Species;
use primitive_shared::horse::Gait;

use super::bank::{Cry, Footing, Impact, Material, Sfx};
use super::clip::Clip;

/// One sound's recordings.
pub struct Recording {
    pub sfx: Sfx,
    /// A level trim applied once at decode.
    ///
    /// **Why the files are not simply normalised to the right loudness.**
    /// Every file is peak-normalised to -1 dBFS so it keeps the full 16
    /// bits through the codec (the rain and fire beds are levelled by
    /// energy instead -- see `bank::BED_SECONDS`); the level it is *played*
    /// at belongs here, beside the sound it is for, and was set against the
    /// recipe it replaced (their peaks were -10.5 dB for a step, -7.5 for a
    /// blow, -6 for setting a block down, -3 for a break, about -9 for a
    /// gust and -6 for an animal's call) so the call sites' own gains, tuned
    /// for years against the recipes, still mean what they meant. Where a
    /// recording is far denser than its recipe
    /// -- a compressed punch, a swish -- the trim is lower than the peak
    /// alone would say, because the ear hears energy, not peaks.
    pub gain: f32,
    /// Paths under `assets/sounds`, ASCII only (an APK asset is opened
    /// through a C string -- see `package-android.sh`).
    pub files: &'static [&'static str],
}

/// Where the recordings live under the assets folder. The same folder a
/// resource pack's `.wav` files go in; the recordings are one level down,
/// by category, so the two can never collide on a name.
pub const DIR: &str = "sounds";

const BREAK_CERAMIC: &[&str] = &["break/ceramic_1.ogg", "break/ceramic_2.ogg", "break/ceramic_3.ogg", "break/ceramic_4.ogg", "break/ceramic_5.ogg"];
const BREAK_DIRT: &[&str] = &["break/dirt_1.ogg", "break/dirt_2.ogg", "break/dirt_3.ogg", "break/dirt_4.ogg"];
const BREAK_GLASS: &[&str] = &["break/glass_1.ogg", "break/glass_2.ogg", "break/glass_3.ogg", "break/glass_4.ogg", "break/glass_5.ogg"];
const BREAK_GRASS: &[&str] = &["break/grass_1.ogg", "break/grass_2.ogg", "break/grass_3.ogg", "break/grass_4.ogg", "break/grass_5.ogg"];
const BREAK_GRAVEL: &[&str] = &["break/gravel_1.ogg", "break/gravel_2.ogg", "break/gravel_3.ogg"];
const BREAK_METAL: &[&str] = &["break/metal_1.ogg", "break/metal_2.ogg", "break/metal_3.ogg", "break/metal_4.ogg", "break/metal_5.ogg"];
const BREAK_STONE: &[&str] = &["break/stone_1.ogg", "break/stone_2.ogg", "break/stone_3.ogg", "break/stone_4.ogg", "break/stone_5.ogg"];
const BREAK_WOOD: &[&str] = &["break/wood_1.ogg", "break/wood_2.ogg", "break/wood_3.ogg", "break/wood_4.ogg", "break/wood_5.ogg"];
const CHEST_CLOSE: &[&str] = &["chest/close_1.ogg", "chest/close_2.ogg", "chest/close_3.ogg", "chest/close_4.ogg"];
const CHEST_OPEN: &[&str] = &["chest/open_1.ogg", "chest/open_2.ogg", "chest/open_3.ogg", "chest/open_4.ogg", "chest/open_5.ogg", "chest/open_6.ogg"];
const DIG_CERAMIC: &[&str] = &["dig/ceramic_1.ogg", "dig/ceramic_2.ogg", "dig/ceramic_3.ogg", "dig/ceramic_4.ogg", "dig/ceramic_5.ogg"];
const DIG_CLOTH: &[&str] = &["dig/cloth_1.ogg", "dig/cloth_2.ogg", "dig/cloth_3.ogg", "dig/cloth_4.ogg"];
const DIG_DIRT: &[&str] = &["dig/dirt_1.ogg", "dig/dirt_2.ogg", "dig/dirt_3.ogg", "dig/dirt_4.ogg"];
const DIG_GLASS: &[&str] = &["dig/glass_1.ogg", "dig/glass_2.ogg", "dig/glass_3.ogg", "dig/glass_4.ogg", "dig/glass_5.ogg"];
const DIG_GRASS: &[&str] = &["dig/grass_1.ogg", "dig/grass_2.ogg", "dig/grass_3.ogg", "dig/grass_4.ogg", "dig/grass_5.ogg"];
const DIG_GRAVEL: &[&str] = &["dig/gravel_1.ogg", "dig/gravel_2.ogg", "dig/gravel_3.ogg", "dig/gravel_4.ogg"];
const DIG_LIQUID: &[&str] = &["dig/liquid_1.ogg", "dig/liquid_2.ogg", "dig/liquid_3.ogg"];
const DIG_METAL: &[&str] = &["dig/metal_1.ogg", "dig/metal_2.ogg", "dig/metal_3.ogg", "dig/metal_4.ogg", "dig/metal_5.ogg"];
const DIG_STONE: &[&str] = &["dig/stone_1.ogg", "dig/stone_2.ogg", "dig/stone_3.ogg", "dig/stone_4.ogg", "dig/stone_5.ogg"];
const DIG_WOOD: &[&str] = &["dig/wood_1.ogg", "dig/wood_2.ogg", "dig/wood_3.ogg", "dig/wood_4.ogg", "dig/wood_5.ogg"];
const FIRE_IGNITE: &[&str] = &["fire/ignite_1.ogg"];
const HAND_HIT: &[&str] = &["hand/hit_1.ogg", "hand/hit_2.ogg", "hand/hit_3.ogg", "hand/hit_4.ogg", "hand/hit_5.ogg"];
const HAND_SWING: &[&str] = &["hand/swing_1.ogg", "hand/swing_2.ogg", "hand/swing_3.ogg", "hand/swing_4.ogg", "hand/swing_5.ogg"];
const ITEM_CRAFT: &[&str] = &["item/craft_1.ogg", "item/craft_2.ogg", "item/craft_3.ogg", "item/craft_4.ogg"];
const ITEM_DROP: &[&str] = &["item/drop_1.ogg", "item/drop_2.ogg", "item/drop_3.ogg", "item/drop_4.ogg"];
const ITEM_EQUIP: &[&str] = &["item/equip_1.ogg", "item/equip_2.ogg", "item/equip_3.ogg", "item/equip_4.ogg"];
const ITEM_PICKUP: &[&str] = &["item/pickup_1.ogg", "item/pickup_2.ogg", "item/pickup_3.ogg"];
const PLACE_CERAMIC: &[&str] = &["place/ceramic_1.ogg", "place/ceramic_2.ogg", "place/ceramic_3.ogg", "place/ceramic_4.ogg", "place/ceramic_5.ogg"];
const PLACE_GLASS: &[&str] = &["place/glass_1.ogg", "place/glass_2.ogg", "place/glass_3.ogg", "place/glass_4.ogg", "place/glass_5.ogg"];
const PLACE_METAL: &[&str] = &["place/metal_1.ogg", "place/metal_2.ogg", "place/metal_3.ogg", "place/metal_4.ogg", "place/metal_5.ogg"];
const PLACE_STONE: &[&str] = &["place/stone_1.ogg", "place/stone_2.ogg", "place/stone_3.ogg", "place/stone_4.ogg", "place/stone_5.ogg", "place/stone_6.ogg"];
const PLACE_WOOD: &[&str] = &["place/wood_1.ogg", "place/wood_2.ogg", "place/wood_3.ogg", "place/wood_4.ogg"];
const PLAYER_BUBBLE: &[&str] = &["player/bubble_1.ogg", "player/bubble_2.ogg", "player/bubble_3.ogg", "player/bubble_4.ogg"];
const PLAYER_DEATH: &[&str] = &["player/death_1.ogg", "player/death_2.ogg"];
const PLAYER_DRINK: &[&str] = &["player/drink_1.ogg", "player/drink_2.ogg", "player/drink_3.ogg"];
const PLAYER_EAT: &[&str] = &["player/eat_1.ogg", "player/eat_2.ogg", "player/eat_3.ogg", "player/eat_4.ogg", "player/eat_5.ogg", "player/eat_6.ogg", "player/eat_7.ogg"];
const PLAYER_HURT: &[&str] = &["player/hurt_1.ogg", "player/hurt_2.ogg", "player/hurt_3.ogg", "player/hurt_4.ogg", "player/hurt_5.ogg"];
const PLAYER_SPLASH: &[&str] = &["player/splash_1.ogg", "player/splash_2.ogg", "player/splash_3.ogg", "player/splash_4.ogg"];
const PLAYER_STAKED: &[&str] = &["player/stake_1.ogg", "player/stake_2.ogg", "player/stake_3.ogg", "player/stake_4.ogg", "player/stake_5.ogg"];
const PLAYER_SWIM: &[&str] = &["player/swim_1.ogg", "player/swim_2.ogg", "player/swim_3.ogg", "player/swim_4.ogg", "player/swim_5.ogg", "player/swim_6.ogg"];
const PLAYER_WADE: &[&str] = &["player/wade_1.ogg", "player/wade_2.ogg", "player/wade_3.ogg", "player/wade_4.ogg", "player/wade_5.ogg", "player/wade_6.ogg"];
const STEP_CLOTH: &[&str] = &["step/cloth_1.ogg", "step/cloth_2.ogg", "step/cloth_3.ogg"];
const STEP_DIRT: &[&str] = &["step/dirt_1.ogg", "step/dirt_2.ogg", "step/dirt_3.ogg", "step/dirt_4.ogg", "step/dirt_5.ogg", "step/dirt_6.ogg"];
const STEP_GRASS: &[&str] = &["step/grass_1.ogg", "step/grass_2.ogg", "step/grass_3.ogg", "step/grass_4.ogg", "step/grass_5.ogg"];
const STEP_GRAVEL: &[&str] = &["step/gravel_1.ogg", "step/gravel_2.ogg", "step/gravel_3.ogg", "step/gravel_4.ogg", "step/gravel_5.ogg", "step/gravel_6.ogg"];
const STEP_SAND: &[&str] = &["step/sand_1.ogg", "step/sand_2.ogg", "step/sand_3.ogg", "step/sand_4.ogg", "step/sand_5.ogg", "step/sand_6.ogg"];
const STEP_SNOW: &[&str] = &["step/snow_1.ogg", "step/snow_2.ogg", "step/snow_3.ogg", "step/snow_4.ogg", "step/snow_5.ogg"];
const STEP_STONE: &[&str] = &["step/stone_1.ogg", "step/stone_2.ogg", "step/stone_3.ogg", "step/stone_4.ogg", "step/stone_5.ogg"];
const STEP_WOOD: &[&str] = &["step/wood_1.ogg", "step/wood_2.ogg", "step/wood_3.ogg", "step/wood_4.ogg", "step/wood_5.ogg"];
const UI_BACK: &[&str] = &["ui/back_1.ogg", "ui/back_2.ogg"];
const UI_CLICK: &[&str] = &["ui/click_1.ogg", "ui/click_2.ogg", "ui/click_3.ogg", "ui/click_4.ogg", "ui/click_5.ogg"];
const UI_MESSAGE: &[&str] = &["ui/message_1.ogg", "ui/message_2.ogg", "ui/message_3.ogg"];
const WILD_FROG: &[&str] = &["wild/frog_1.ogg", "wild/frog_2.ogg", "wild/frog_3.ogg", "wild/frog_4.ogg"];
const WILD_GULL: &[&str] = &["wild/gull_1.ogg", "wild/gull_2.ogg", "wild/gull_3.ogg", "wild/gull_4.ogg"];
const WORLD_THUNDER: &[&str] = &["world/thunder_1.ogg"];
const FIRE_CRACKLE: &[&str] = &["fire/crackle_1.ogg", "fire/crackle_2.ogg", "fire/crackle_3.ogg"];
const WILD_BEES: &[&str] = &["wild/bees_1.ogg", "wild/bees_2.ogg", "wild/bees_3.ogg", "wild/bees_4.ogg"];
const WILD_WINGS: &[&str] = &["wild/wings_1.ogg", "wild/wings_2.ogg", "wild/wings_3.ogg", "wild/wings_4.ogg", "wild/wings_5.ogg"];
const WORK_BENCH: &[&str] = &["work/bench_1.ogg", "work/bench_2.ogg", "work/bench_3.ogg", "work/bench_4.ogg", "work/bench_5.ogg"];
const WORK_MASON: &[&str] = &["work/mason_1.ogg", "work/mason_2.ogg", "work/mason_3.ogg", "work/mason_4.ogg", "work/mason_5.ogg"];
const WORK_WHEEL: &[&str] = &["work/wheel_1.ogg", "work/wheel_2.ogg", "work/wheel_3.ogg", "work/wheel_4.ogg", "work/wheel_5.ogg"];
const WORK_LEATHER: &[&str] = &["work/leather_1.ogg", "work/leather_2.ogg", "work/leather_3.ogg", "work/leather_4.ogg", "work/leather_5.ogg"];
const WORK_ANVIL: &[&str] = &["work/anvil_1.ogg", "work/anvil_2.ogg", "work/anvil_3.ogg", "work/anvil_4.ogg"];
const WORK_OPEN: &[&str] = &["work/open_1.ogg", "work/open_2.ogg"];
const CRUMBLE_STONE: &[&str] = &["crumble/stone_1.ogg", "crumble/stone_2.ogg", "crumble/stone_3.ogg", "crumble/stone_4.ogg", "crumble/stone_5.ogg"];
const CRUMBLE_DIRT: &[&str] = &["crumble/dirt_1.ogg", "crumble/dirt_2.ogg", "crumble/dirt_3.ogg"];
const CRUMBLE_GRAVEL: &[&str] = &["crumble/gravel_1.ogg", "crumble/gravel_2.ogg", "crumble/gravel_3.ogg", "crumble/gravel_4.ogg"];
const CRUMBLE_SAND: &[&str] = &["crumble/sand_1.ogg", "crumble/sand_2.ogg", "crumble/sand_3.ogg", "crumble/sand_4.ogg"];
const CRUMBLE_WOOD: &[&str] = &["crumble/wood_1.ogg", "crumble/wood_2.ogg", "crumble/wood_3.ogg", "crumble/wood_4.ogg"];
const WORLD_DRIP: &[&str] = &["world/drip_1.ogg", "world/drip_2.ogg", "world/drip_3.ogg", "world/drip_4.ogg"];
const WORLD_RAIN: &[&str] = &["world/rain_1.ogg", "world/rain_2.ogg", "world/rain_3.ogg"];
const WORLD_RAIN_ROOF: &[&str] = &["world/rain_roof_1.ogg", "world/rain_roof_2.ogg", "world/rain_roof_3.ogg"];
const WORLD_WIND: &[&str] = &["world/wind_1.ogg", "world/wind_2.ogg", "world/wind_3.ogg", "world/wind_4.ogg"];
const WORLD_WIND_HOWL: &[&str] = &["world/wind_howl_1.ogg", "world/wind_howl_2.ogg", "world/wind_howl_3.ogg", "world/wind_howl_4.ogg"];
const WORLD_WIND_LEAVES: &[&str] = &["world/wind_leaves_1.ogg", "world/wind_leaves_2.ogg", "world/wind_leaves_3.ogg", "world/wind_leaves_4.ogg"];
const WILD_ANTELOPE_ALARM: &[&str] = &["wild/antelope_alarm_1.ogg", "wild/antelope_alarm_2.ogg", "wild/antelope_alarm_3.ogg"];
const WILD_ANTELOPE_DEATH: &[&str] = &["wild/antelope_death_1.ogg", "wild/antelope_death_2.ogg"];
const WILD_ANTELOPE_HURT: &[&str] = &["wild/antelope_hurt_1.ogg", "wild/antelope_hurt_2.ogg", "wild/antelope_hurt_3.ogg"];
const WILD_ANTELOPE_IDLE: &[&str] = &["wild/antelope_idle_1.ogg", "wild/antelope_idle_2.ogg", "wild/antelope_idle_3.ogg"];
const WILD_BEAR_ALARM: &[&str] = &["wild/bear_alarm_1.ogg", "wild/bear_alarm_2.ogg", "wild/bear_alarm_3.ogg"];
const WILD_BEAR_DEATH: &[&str] = &["wild/bear_death_1.ogg", "wild/bear_death_2.ogg"];
const WILD_BEAR_HURT: &[&str] = &["wild/bear_hurt_1.ogg", "wild/bear_hurt_2.ogg", "wild/bear_hurt_3.ogg"];
const WILD_BEAR_IDLE: &[&str] = &["wild/bear_idle_1.ogg", "wild/bear_idle_2.ogg", "wild/bear_idle_3.ogg"];
const WILD_BEAR_THREAT: &[&str] = &["wild/bear_threat_1.ogg", "wild/bear_threat_2.ogg", "wild/bear_threat_3.ogg"];
const WILD_BOAR_ALARM: &[&str] = &["wild/boar_alarm_1.ogg", "wild/boar_alarm_2.ogg", "wild/boar_alarm_3.ogg"];
const WILD_BOAR_DEATH: &[&str] = &["wild/boar_death_1.ogg", "wild/boar_death_2.ogg", "wild/boar_death_3.ogg"];
const WILD_BOAR_HURT: &[&str] = &["wild/boar_hurt_1.ogg", "wild/boar_hurt_2.ogg", "wild/boar_hurt_3.ogg"];
const WILD_BOAR_IDLE: &[&str] = &["wild/boar_idle_1.ogg", "wild/boar_idle_2.ogg", "wild/boar_idle_3.ogg"];
const WILD_BOAR_THREAT: &[&str] = &["wild/boar_threat_1.ogg", "wild/boar_threat_2.ogg", "wild/boar_threat_3.ogg"];
const WILD_COD_ALARM: &[&str] = &["wild/cod_alarm_1.ogg", "wild/cod_alarm_2.ogg"];
const WILD_COD_DEATH: &[&str] = &["wild/cod_death_1.ogg", "wild/cod_death_2.ogg"];
const WILD_COD_HURT: &[&str] = &["wild/cod_hurt_1.ogg", "wild/cod_hurt_2.ogg"];
const WILD_DEER_ALARM: &[&str] = &["wild/deer_alarm_1.ogg", "wild/deer_alarm_2.ogg", "wild/deer_alarm_3.ogg"];
const WILD_DEER_DEATH: &[&str] = &["wild/deer_death_1.ogg", "wild/deer_death_2.ogg", "wild/deer_death_3.ogg"];
const WILD_DEER_HURT: &[&str] = &["wild/deer_hurt_1.ogg", "wild/deer_hurt_2.ogg", "wild/deer_hurt_3.ogg"];
const WILD_DEER_IDLE: &[&str] = &["wild/deer_idle_1.ogg", "wild/deer_idle_2.ogg", "wild/deer_idle_3.ogg"];
const WILD_FISH_ALARM: &[&str] = &["wild/fish_alarm_1.ogg", "wild/fish_alarm_2.ogg", "wild/fish_alarm_3.ogg"];
const WILD_FISH_DEATH: &[&str] = &["wild/fish_death_1.ogg", "wild/fish_death_2.ogg"];
const WILD_FISH_HURT: &[&str] = &["wild/fish_hurt_1.ogg", "wild/fish_hurt_2.ogg"];
const WILD_FOWL_ALARM: &[&str] = &["wild/fowl_alarm_1.ogg", "wild/fowl_alarm_2.ogg", "wild/fowl_alarm_3.ogg"];
const WILD_FOWL_DEATH: &[&str] = &["wild/fowl_death_1.ogg", "wild/fowl_death_2.ogg"];
const WILD_FOWL_HURT: &[&str] = &["wild/fowl_hurt_1.ogg", "wild/fowl_hurt_2.ogg", "wild/fowl_hurt_3.ogg"];
const WILD_FOWL_IDLE: &[&str] = &["wild/fowl_idle_1.ogg", "wild/fowl_idle_2.ogg", "wild/fowl_idle_3.ogg"];
const WILD_GULL_DEATH: &[&str] = &["wild/gull_death_1.ogg", "wild/gull_death_2.ogg"];
const WILD_GULL_HURT: &[&str] = &["wild/gull_hurt_1.ogg", "wild/gull_hurt_2.ogg", "wild/gull_hurt_3.ogg"];
const WILD_HARE_ALARM: &[&str] = &["wild/hare_alarm_1.ogg", "wild/hare_alarm_2.ogg"];
const WILD_HARE_DEATH: &[&str] = &["wild/hare_death_1.ogg", "wild/hare_death_2.ogg", "wild/hare_death_3.ogg"];
const WILD_HARE_HURT: &[&str] = &["wild/hare_hurt_1.ogg", "wild/hare_hurt_2.ogg", "wild/hare_hurt_3.ogg"];
const WILD_LION_ALARM: &[&str] = &["wild/lion_alarm_1.ogg", "wild/lion_alarm_2.ogg", "wild/lion_alarm_3.ogg"];
const WILD_LION_DEATH: &[&str] = &["wild/lion_death_1.ogg", "wild/lion_death_2.ogg"];
const WILD_LION_HURT: &[&str] = &["wild/lion_hurt_1.ogg", "wild/lion_hurt_2.ogg", "wild/lion_hurt_3.ogg"];
const WILD_LION_IDLE: &[&str] = &["wild/lion_idle_1.ogg", "wild/lion_idle_2.ogg", "wild/lion_idle_3.ogg"];
const WILD_LION_THREAT: &[&str] = &["wild/lion_threat_1.ogg", "wild/lion_threat_2.ogg", "wild/lion_threat_3.ogg"];
const WILD_SHEEP_ALARM: &[&str] = &["wild/sheep_alarm_1.ogg", "wild/sheep_alarm_2.ogg", "wild/sheep_alarm_3.ogg"];
const WILD_SHEEP_DEATH: &[&str] = &["wild/sheep_death_1.ogg", "wild/sheep_death_2.ogg"];
const WILD_SHEEP_HURT: &[&str] = &["wild/sheep_hurt_1.ogg", "wild/sheep_hurt_2.ogg", "wild/sheep_hurt_3.ogg"];
const WILD_SHEEP_IDLE: &[&str] = &["wild/sheep_idle_1.ogg", "wild/sheep_idle_2.ogg", "wild/sheep_idle_3.ogg"];
const WILD_WOLF_ALARM: &[&str] = &["wild/wolf_alarm_1.ogg", "wild/wolf_alarm_2.ogg", "wild/wolf_alarm_3.ogg"];
const WILD_WOLF_DEATH: &[&str] = &["wild/wolf_death_1.ogg", "wild/wolf_death_2.ogg", "wild/wolf_death_3.ogg"];
const WILD_WOLF_HURT: &[&str] = &["wild/wolf_hurt_1.ogg", "wild/wolf_hurt_2.ogg", "wild/wolf_hurt_3.ogg"];
const WILD_WOLF_IDLE: &[&str] = &["wild/wolf_idle_1.ogg", "wild/wolf_idle_2.ogg", "wild/wolf_idle_3.ogg"];
const WILD_WOLF_THREAT: &[&str] = &["wild/wolf_threat_1.ogg", "wild/wolf_threat_2.ogg", "wild/wolf_threat_3.ogg"];
const WILD_ZEBRA_ALARM: &[&str] = &["wild/zebra_alarm_1.ogg", "wild/zebra_alarm_2.ogg", "wild/zebra_alarm_3.ogg"];
const WILD_ZEBRA_DEATH: &[&str] = &["wild/zebra_death_1.ogg", "wild/zebra_death_2.ogg"];
const WILD_ZEBRA_HURT: &[&str] = &["wild/zebra_hurt_1.ogg", "wild/zebra_hurt_2.ogg", "wild/zebra_hurt_3.ogg"];
const WILD_ZEBRA_IDLE: &[&str] = &["wild/zebra_idle_1.ogg", "wild/zebra_idle_2.ogg"];
const WILD_HORSE_ALARM: &[&str] = &["wild/horse_alarm_1.ogg", "wild/horse_alarm_2.ogg", "wild/horse_alarm_3.ogg"];
const WILD_HORSE_DEATH: &[&str] = &["wild/horse_death_1.ogg", "wild/horse_death_2.ogg"];
const WILD_HORSE_HURT: &[&str] = &["wild/horse_hurt_1.ogg", "wild/horse_hurt_2.ogg", "wild/horse_hurt_3.ogg"];
const WILD_HORSE_IDLE: &[&str] = &["wild/horse_idle_1.ogg", "wild/horse_idle_2.ogg", "wild/horse_idle_3.ogg", "wild/horse_idle_4.ogg"];
const WORLD_CRICKETS: &[&str] = &["world/crickets_1.ogg", "world/crickets_2.ogg", "world/crickets_3.ogg", "world/crickets_4.ogg"];
const HOOF_WALK: &[&str] = &["step/hoof_walk_1.ogg", "step/hoof_walk_2.ogg", "step/hoof_walk_3.ogg", "step/hoof_walk_4.ogg"];
const HOOF_TROT: &[&str] = &["step/hoof_trot_1.ogg", "step/hoof_trot_2.ogg", "step/hoof_trot_3.ogg", "step/hoof_trot_4.ogg"];
const HOOF_GALLOP: &[&str] = &["step/hoof_gallop_1.ogg", "step/hoof_gallop_2.ogg", "step/hoof_gallop_3.ogg", "step/hoof_gallop_4.ogg"];
const HOOF_WALK_HARD: &[&str] = &["step/hoof_walk_hard_1.ogg", "step/hoof_walk_hard_2.ogg", "step/hoof_walk_hard_3.ogg", "step/hoof_walk_hard_4.ogg"];
const HOOF_TROT_HARD: &[&str] = &["step/hoof_trot_hard_1.ogg", "step/hoof_trot_hard_2.ogg", "step/hoof_trot_hard_3.ogg", "step/hoof_trot_hard_4.ogg"];
const RUSTLE: &[&str] = &[
    "dig/grass_1.ogg", "dig/grass_2.ogg", "dig/grass_3.ogg", "dig/grass_4.ogg", "dig/grass_5.ogg",
    "break/grass_1.ogg", "break/grass_2.ogg", "break/grass_3.ogg", "break/grass_4.ogg", "break/grass_5.ogg",
];

/// Shorthand for a material row, so the table below reads as a table.
const fn mat(impact: Impact, material: Material, gain: f32, files: &'static [&'static str]) -> Recording {
    Recording { sfx: Sfx::Material(impact, material), gain, files }
}

const fn one(sfx: Sfx, gain: f32, files: &'static [&'static str]) -> Recording {
    Recording { sfx, gain, files }
}

use Impact::{Break, Dig, Place, Step};
use Material as M;

/// Every recorded sound.
///
/// **Some rows share files, on purpose.** Sand is one grainy crunch
/// whether it is walked on, dug or dropped, and a swimmer's stroke and a
/// wader's step are the same water. Recording three versions of a thing
/// that makes one noise would be variety for its own sake; the call
/// sites already play them at different gains and pitches.
///
/// **What is not here is in [`SILENT`], with why.**
pub const RECORDINGS: &[Recording] = &[
    // ---- walking on it ----
    // Kenney's concrete for rock: a short, dry heel with no ring.
    mat(Step, M::Stone, 0.34, STEP_STONE),
    mat(Step, M::Dirt, 0.34, STEP_DIRT),
    mat(Step, M::Grass, 0.34, STEP_GRASS),
    // Lower than its neighbours: a boot on a board is a low, dense thump
    // whose energy sits far above its peak (-14 dB RMS against the
    // others' -20), and at the same trim the floor of a house was the
    // loudest thing in the game.
    mat(Step, M::Wood, 0.22, STEP_WOOD),
    mat(Step, M::Sand, 0.38, STEP_SAND),
    mat(Step, M::Gravel, 0.38, STEP_GRAVEL),
    mat(Step, M::Snow, 0.32, STEP_SNOW),
    // Nobody walks on an ingot; the light tap is the honest sound of a
    // foot on a metal thing, and the bell-like heavy one was not.
    mat(Step, M::Metal, 0.22, DIG_METAL),
    mat(Step, M::Glass, 0.25, DIG_GLASS),
    mat(Step, M::Ceramic, 0.25, DIG_CERAMIC),
    mat(Step, M::Cloth, 0.3, STEP_CLOTH),
    mat(Step, M::Liquid, 0.4, PLAYER_WADE),
    // ---- one blow of a swing ----
    // A pick on rock does ring, briefly. The clips are cut at 0.4 s so
    // four blows a second do not pile their tails into a drone.
    mat(Dig, M::Stone, 0.42, DIG_STONE),
    mat(Dig, M::Dirt, 0.45, DIG_DIRT),
    mat(Dig, M::Grass, 0.5, DIG_GRASS),
    mat(Dig, M::Wood, 0.47, DIG_WOOD),
    mat(Dig, M::Sand, 0.47, STEP_SAND),
    mat(Dig, M::Gravel, 0.45, DIG_GRAVEL),
    mat(Dig, M::Snow, 0.45, STEP_SNOW),
    mat(Dig, M::Metal, 0.42, DIG_METAL),
    mat(Dig, M::Glass, 0.42, DIG_GLASS),
    mat(Dig, M::Ceramic, 0.47, DIG_CERAMIC),
    mat(Dig, M::Cloth, 0.5, DIG_CLOTH),
    mat(Dig, M::Liquid, 0.4, DIG_LIQUID),
    // ---- the blow that finished it ----
    mat(Break, M::Stone, 0.7, BREAK_STONE),
    mat(Break, M::Dirt, 0.75, BREAK_DIRT),
    mat(Break, M::Grass, 0.75, BREAK_GRASS),
    mat(Break, M::Wood, 0.75, BREAK_WOOD),
    mat(Break, M::Sand, 0.8, STEP_SAND),
    mat(Break, M::Gravel, 0.8, BREAK_GRAVEL),
    mat(Break, M::Snow, 0.7, STEP_SNOW),
    mat(Break, M::Metal, 0.7, BREAK_METAL),
    mat(Break, M::Glass, 0.75, BREAK_GLASS),
    mat(Break, M::Ceramic, 0.75, BREAK_CERAMIC),
    mat(Break, M::Cloth, 0.6, ITEM_DROP),
    mat(Break, M::Liquid, 0.6, DIG_LIQUID),
    // ---- setting one down ----
    // Rock placed is a real stone footfall (Fantozzi's) rather than
    // Kenney's concrete, so building a wall and walking along it are two
    // sounds.
    mat(Place, M::Stone, 0.56, PLACE_STONE),
    mat(Place, M::Dirt, 0.5, DIG_DIRT),
    mat(Place, M::Grass, 0.56, STEP_GRASS),
    mat(Place, M::Wood, 0.5, PLACE_WOOD),
    mat(Place, M::Sand, 0.56, STEP_SAND),
    mat(Place, M::Gravel, 0.5, DIG_GRAVEL),
    mat(Place, M::Snow, 0.5, STEP_SNOW),
    mat(Place, M::Metal, 0.5, PLACE_METAL),
    mat(Place, M::Glass, 0.5, PLACE_GLASS),
    mat(Place, M::Ceramic, 0.56, PLACE_CERAMIC),
    mat(Place, M::Cloth, 0.5, ITEM_DROP),
    mat(Place, M::Liquid, 0.5, DIG_LIQUID),
    // ---- the player's own body ----
    // A jump is the clothes moving, not a grunt: a voice would be a
    // character, and the player is not one.
    one(Sfx::Jump, 0.3, DIG_CLOTH),
    // A landing is feet meeting ground hard, and the ground gets its own
    // word on top (see `Soundscape::body`). Kenney's "soft impact" was
    // the first choice and failed `no_recording_is_a_note_either` by a
    // mile: it is a sub-bass boom at 90 Hz -- a kick drum, even with the
    // bottom filtered off.
    one(Sfx::Land, 0.6, STEP_DIRT),
    // **A person in water, not a game's water.** The splash and the stroke
    // were cut from a pack of "water splash & slime" effects -- short,
    // bright, all attack -- and a stroke every two thirds of a second of
    // that was what the player called the splashing he wanted gone. Now
    // they are field recordings: a person jumping into a lake for the
    // splash, a person swimming for the stroke, a person wading a
    // shallow for the stride (`Wade`). Six strokes and six strides, so
    // `next_variant` has room to not sound like a loop. The stroke's
    // trim is under the step's on purpose: the stroke's recording is
    // hot (-17 dB RMS against a step's -21), and a swimmer should hear
    // the water around them, not a slap per arm.
    one(Sfx::Splash, 0.7, PLAYER_SPLASH),
    one(Sfx::Swim, 0.38, PLAYER_SWIM),
    one(Sfx::Wade, 0.5, PLAYER_WADE),
    one(Sfx::Bubble, 0.3, PLAYER_BUBBLE),
    // **Blows, not cries**, for the same reason as the jump. Kenney's
    // punches are compressed hard (-15 dB RMS at a -1 dB peak), so the
    // trim is a quarter of what the peak alone would give.
    one(Sfx::Hurt, 0.25, PLAYER_HURT),
    // Played over `Hurt`, which is the blow; this is what the blow was:
    // a bundle of sharpened poles knocked and shoved, a twig snapping or
    // cloth tearing, a body stopping -- logs thrown on dirt and bodies
    // falling into debris and leaves, laid over each other. It used to be
    // knives into melons and a stake "into a vampire", which was a horror
    // film's stab and which the player called bad -- and at 0.6 over
    // files at -17 dB it was 2.5 dB *louder* than the `Hurt` it sits on.
    // The files are levelled by energy to -19 dB RMS, and 0.4 puts them
    // near -28 against `Hurt`'s -24: under it, never over it.
    one(Sfx::Staked, 0.4, PLAYER_STAKED),
    one(Sfx::Death, 0.18, PLAYER_DEATH),
    one(Sfx::Eat, 0.42, PLAYER_EAT),
    one(Sfx::Drink, 0.5, PLAYER_DRINK),
    // ---- what the player does to the world ----
    one(Sfx::Swing, 0.25, HAND_SWING),
    one(Sfx::Hit, 0.35, HAND_HIT),
    one(Sfx::Pickup, 0.47, ITEM_PICKUP),
    one(Sfx::Drop, 0.4, ITEM_DROP),
    one(Sfx::Equip, 0.45, ITEM_EQUIP),
    one(Sfx::Craft, 0.45, ITEM_CRAFT),
    // A lid and a box, not a hinge. The creaks in the same pack were the
    // obvious pick and scored 0.87-0.96 on the note measure: a creak *is*
    // a pitch, sliding, and the player's ear files it with the jingles.
    one(Sfx::ChestOpen, 0.47, CHEST_OPEN),
    one(Sfx::ChestClose, 0.45, CHEST_CLOSE),
    one(Sfx::Ignite, 0.3, FIRE_IGNITE),
    // ---- the workshops ----
    // **Half a stroke, not a montage.** Each of these is one gesture --
    // a saw crossing a board once, one blow of a mallet on a chisel, one
    // turn of the wheel head -- because a workshop sound plays once per
    // thing made, and a two-second loop of a busy shop played on every
    // plank would be a tape recorder behind the player. The bench and the
    // wheel carry two gestures apiece (a saw and a plane, the head
    // turning and wet clay under a hand) so that making six planks is not
    // the same eight hundred milliseconds six times.
    one(Sfx::WorkBench, 0.5, WORK_BENCH),
    one(Sfx::WorkMason, 0.5, WORK_MASON),
    one(Sfx::WorkWheel, 0.45, WORK_WHEEL),
    one(Sfx::WorkLeather, 0.5, WORK_LEATHER),
    // Louder than the rest, and it is the one struck with a hammer:
    // trimmed by energy rather than peak, a blow on an anvil that sat
    // level with a saw stroke sounded like a tap on a tin.
    one(Sfx::WorkAnvil, 0.55, WORK_ANVIL),
    one(Sfx::StationOpen, 0.4, WORK_OPEN),
    // ---- something giving way ----
    // Quieter than a break, because a crumble is usually heard *under*
    // one (`soundscape::on_block_broken`): it is the debris after the
    // blow, not a second blow. The exceptions -- a roof coming down, a
    // column of sand letting go -- are placed by the call site at its own
    // gain, which is where a collapse gets to be louder than a pick.
    one(Sfx::Crumble(M::Stone), 0.6, CRUMBLE_STONE),
    one(Sfx::Crumble(M::Dirt), 0.6, CRUMBLE_DIRT),
    one(Sfx::Crumble(M::Gravel), 0.6, CRUMBLE_GRAVEL),
    one(Sfx::Crumble(M::Sand), 0.55, CRUMBLE_SAND),
    // A dry crack and the rumble after it, which is what a board does and
    // what none of the other four do.
    one(Sfx::Crumble(M::Wood), 0.65, CRUMBLE_WOOD),
    // ---- the float ----
    // **Reused, and the reason is the budget as much as the sound.** A cork
    // landing is a small splash, which is what the two short splashes the
    // hand-in-a-pool was cut from are (splash_09 and splash_10 of the same
    // pack); a bite is one soft bubble, which is what the drowning player's
    // single bubbles are. New recordings of both would be the same two
    // noises again and eat into `the_recordings_stay_a_modest_download`.
    // What makes them the float's is where and how they play: at the float,
    // quieter than anything the player does with their own hands, and the
    // plop pitched up for a thing the size of a thumb (`lib.rs`, the
    // `ServerMessage::Line` arm).
    one(Sfx::FloatPlop, 0.35, &["dig/liquid_1.ogg", "dig/liquid_2.ogg"]),
    one(Sfx::FloatBite, 0.3, PLAYER_BUBBLE),
    // ---- the interface ----
    // A light knock on wood and a book: things a stone-age hand could
    // make, and not a single one of them a beep.
    one(Sfx::Click, 0.33, UI_CLICK),
    one(Sfx::Back, 0.3, UI_BACK),
    one(Sfx::Message, 0.3, UI_MESSAGE),
    // ---- the world ----
    // **The fire is a bed, not a string of snaps.** The CC0 crackle this
    // table had before was separate snaps with silence between them, and
    // the player sent it back; this is a small campfire's steady hiss with
    // the odd pop in it, cut into pieces exactly `bank::FIRE_SECONDS` long
    // and levelled by energy so the soundscape can lay them end over end
    // (`a_fire_is_a_dense_bed_not_a_string_of_snaps`).
    one(Sfx::FireCrackle, 1.0, FIRE_CRACKLE),
    one(Sfx::Thunder, 0.9, WORLD_THUNDER),
    // A steady heavy shower rather than rain on something: a recording of
    // drops on one surface close to the microphone is ticks, which is what
    // the rain was once taken away from.
    one(Sfx::Rain, 1.0, WORLD_RAIN),
    // Rain on a shed roof, with its hiss taken below 5 kHz when it was
    // cut: the same shower heard from under the thing it is hitting, and
    // quieter, because the old sheltered bed was.
    one(Sfx::RainSheltered, 0.55, WORLD_RAIN_ROOF),
    // Single drops. A drop *is* a pitch -- its size decides it -- and is
    // held to that the way a bubble is.
    one(Sfx::RainTick, 0.25, WORLD_DRIP),
    // **The gust is cut from a recording that does not whistle.** The two
    // short CC0 gusts found first scored 0.62 and 0.64 on the note measure
    // (a whistle is a note), and a recipe was kept for years because of
    // it. Four pieces of a longer take of wind on open ground, each with a
    // swell in and out.
    one(Sfx::Wind, 0.34, WORLD_WIND),
    // Leaves in a moderate wind, high-passed so the air under them does
    // not make this a second gust.
    one(Sfx::WindBreeze, 0.3, WORLD_WIND_LEAVES),
    one(Sfx::WindHowl, 0.38, WORLD_WIND_HOWL),
    // **Pushing through a bush is the dry leaves again**, all ten of them:
    // the same handful of rustles that are a leaf block being dug and
    // broken, which is what brushing past one is. Ten rather than five
    // because this plays at every stride through a thicket, and
    // `next_variant` never plays one twice running. Quieter than the dig:
    // it is a side effect of walking, not the thing being done.
    one(Sfx::Rustle, 0.38, RUSTLE),
    // ---- what lives in it ----
    // The gulls were recorded on a beach and carry its surf; the rumble
    // was cut away below 700 Hz when they were converted, or every call
    // would bring a wave with it.
    one(Sfx::GullCall, 0.55, WILD_GULL),
    one(Sfx::FrogCroak, 0.3, WILD_FROG),
    // A covey going up: pigeons and other birds taking off, the clatter
    // of the first wingbeats with the rumble under them cut away.
    one(Sfx::WingBeats, 0.45, WILD_WINGS),
    // A short burst of bees, in pieces that crossfade, because the hum is
    // laid over itself from wherever a bee is flying.
    one(Sfx::Swarm, 0.4, WILD_BEES),
    // **Every animal's voice.** Real animals where a CC0 recording of that
    // animal exists; where none does, the nearest animal that makes the
    // same kind of noise, and SOURCES.md says which -- a pig for a boar, a
    // dog's bark, yelp and whine for a wolf's, a goat's bleat for a
    // wounded antelope, a donkey for a zebra past its one bray, a rabbit
    // for a hare, a chicken's alarm for wild fowl. A fish's cries are what
    // a person on the bank hears of one: the tail on the water and the
    // thrash on the stones. Bigger animals a little louder, as the recipes
    // had them.
    one(Sfx::Animal(Species::Antelope, Cry::Alarm), 0.55, WILD_ANTELOPE_ALARM),
    one(Sfx::Animal(Species::Antelope, Cry::Death), 0.55, WILD_ANTELOPE_DEATH),
    one(Sfx::Animal(Species::Antelope, Cry::Hurt), 0.55, WILD_ANTELOPE_HURT),
    one(Sfx::Animal(Species::Antelope, Cry::Idle), 0.55, WILD_ANTELOPE_IDLE),
    one(Sfx::Animal(Species::Bear, Cry::Alarm), 0.65, WILD_BEAR_ALARM),
    one(Sfx::Animal(Species::Bear, Cry::Death), 0.65, WILD_BEAR_DEATH),
    one(Sfx::Animal(Species::Bear, Cry::Hurt), 0.65, WILD_BEAR_HURT),
    one(Sfx::Animal(Species::Bear, Cry::Idle), 0.65, WILD_BEAR_IDLE),
    one(Sfx::Animal(Species::Bear, Cry::Threat), 0.65, WILD_BEAR_THREAT),
    one(Sfx::Animal(Species::Boar, Cry::Alarm), 0.55, WILD_BOAR_ALARM),
    one(Sfx::Animal(Species::Boar, Cry::Death), 0.55, WILD_BOAR_DEATH),
    one(Sfx::Animal(Species::Boar, Cry::Hurt), 0.55, WILD_BOAR_HURT),
    one(Sfx::Animal(Species::Boar, Cry::Idle), 0.55, WILD_BOAR_IDLE),
    one(Sfx::Animal(Species::Boar, Cry::Threat), 0.55, WILD_BOAR_THREAT),
    one(Sfx::Animal(Species::Cod, Cry::Alarm), 0.5, WILD_COD_ALARM),
    one(Sfx::Animal(Species::Cod, Cry::Death), 0.5, WILD_COD_DEATH),
    one(Sfx::Animal(Species::Cod, Cry::Hurt), 0.5, WILD_COD_HURT),
    one(Sfx::Animal(Species::Deer, Cry::Alarm), 0.55, WILD_DEER_ALARM),
    one(Sfx::Animal(Species::Deer, Cry::Death), 0.55, WILD_DEER_DEATH),
    one(Sfx::Animal(Species::Deer, Cry::Hurt), 0.55, WILD_DEER_HURT),
    one(Sfx::Animal(Species::Deer, Cry::Idle), 0.55, WILD_DEER_IDLE),
    one(Sfx::Animal(Species::Fish, Cry::Alarm), 0.45, WILD_FISH_ALARM),
    one(Sfx::Animal(Species::Fish, Cry::Death), 0.45, WILD_FISH_DEATH),
    one(Sfx::Animal(Species::Fish, Cry::Hurt), 0.45, WILD_FISH_HURT),
    one(Sfx::Animal(Species::Fowl, Cry::Alarm), 0.45, WILD_FOWL_ALARM),
    one(Sfx::Animal(Species::Fowl, Cry::Death), 0.45, WILD_FOWL_DEATH),
    one(Sfx::Animal(Species::Fowl, Cry::Hurt), 0.45, WILD_FOWL_HURT),
    one(Sfx::Animal(Species::Fowl, Cry::Idle), 0.45, WILD_FOWL_IDLE),
    one(Sfx::Animal(Species::Gull, Cry::Death), 0.5, WILD_GULL_DEATH),
    one(Sfx::Animal(Species::Gull, Cry::Hurt), 0.5, WILD_GULL_HURT),
    one(Sfx::Animal(Species::Hare, Cry::Alarm), 0.45, WILD_HARE_ALARM),
    one(Sfx::Animal(Species::Hare, Cry::Death), 0.45, WILD_HARE_DEATH),
    one(Sfx::Animal(Species::Hare, Cry::Hurt), 0.45, WILD_HARE_HURT),
    one(Sfx::Animal(Species::Lion, Cry::Alarm), 0.65, WILD_LION_ALARM),
    one(Sfx::Animal(Species::Lion, Cry::Death), 0.65, WILD_LION_DEATH),
    one(Sfx::Animal(Species::Lion, Cry::Hurt), 0.65, WILD_LION_HURT),
    one(Sfx::Animal(Species::Lion, Cry::Idle), 0.65, WILD_LION_IDLE),
    one(Sfx::Animal(Species::Lion, Cry::Threat), 0.65, WILD_LION_THREAT),
    one(Sfx::Animal(Species::Sheep, Cry::Alarm), 0.55, WILD_SHEEP_ALARM),
    one(Sfx::Animal(Species::Sheep, Cry::Death), 0.55, WILD_SHEEP_DEATH),
    one(Sfx::Animal(Species::Sheep, Cry::Hurt), 0.55, WILD_SHEEP_HURT),
    one(Sfx::Animal(Species::Sheep, Cry::Idle), 0.55, WILD_SHEEP_IDLE),
    one(Sfx::Animal(Species::Wolf, Cry::Alarm), 0.55, WILD_WOLF_ALARM),
    one(Sfx::Animal(Species::Wolf, Cry::Death), 0.55, WILD_WOLF_DEATH),
    one(Sfx::Animal(Species::Wolf, Cry::Hurt), 0.55, WILD_WOLF_HURT),
    one(Sfx::Animal(Species::Wolf, Cry::Idle), 0.55, WILD_WOLF_IDLE),
    one(Sfx::Animal(Species::Wolf, Cry::Threat), 0.55, WILD_WOLF_THREAT),
    one(Sfx::Animal(Species::Zebra, Cry::Alarm), 0.55, WILD_ZEBRA_ALARM),
    one(Sfx::Animal(Species::Zebra, Cry::Death), 0.55, WILD_ZEBRA_DEATH),
    one(Sfx::Animal(Species::Zebra, Cry::Hurt), 0.55, WILD_ZEBRA_HURT),
    one(Sfx::Animal(Species::Zebra, Cry::Idle), 0.55, WILD_ZEBRA_IDLE),
    // **The horse's own**, where it borrowed the zebra's bray and a
    // donkey's. Snorts and blows for a horse at rest, three whinnies for
    // one that bolts, the first breath of a whinny cut short for a blow, and
    // a long breath out for the end. A whinny is loud next to a snort, as
    // it is in a field: they are all levelled to one peak, and the snorts
    // are what plays most.
    one(Sfx::Animal(Species::Horse, Cry::Alarm), 0.6, WILD_HORSE_ALARM),
    one(Sfx::Animal(Species::Horse, Cry::Death), 0.6, WILD_HORSE_DEATH),
    one(Sfx::Animal(Species::Horse, Cry::Hurt), 0.6, WILD_HORSE_HURT),
    one(Sfx::Animal(Species::Horse, Cry::Idle), 0.45, WILD_HORSE_IDLE),
    // ---- the night, and the horse's feet ----
    // Crickets in a field, high-passed at 2.5 kHz so no road or wind comes
    // with them, cut as a bed like the rain (`bank::BED_SECONDS`) and
    // levelled by energy. Four pieces from two fields: the soundscape lays
    // one from wherever in the grass the chorus is, so a repeat is also a
    // move, and never the same piece twice running.
    one(Sfx::Crickets, 0.5, WORLD_CRICKETS),
    // **A piece of each pace, not a clop at a time.** A walk is four beats
    // in an uneven rhythm and a gallop three in a rush and a gap; timing
    // single hoof strikes from a speed would be a metronome of whichever
    // the code guessed. Pieces a stride or two long, cut from a real horse
    // at that pace and laid end over end by `soundscape::Hoofbeats`, carry
    // the rhythm in them.
    one(Sfx::Hoofs(Gait::Walk, Footing::Soft), 0.4, HOOF_WALK),
    one(Sfx::Hoofs(Gait::Trot, Footing::Soft), 0.45, HOOF_TROT),
    one(Sfx::Hoofs(Gait::Gallop, Footing::Soft), 0.55, HOOF_GALLOP),
    one(Sfx::Hoofs(Gait::Walk, Footing::Hard), 0.4, HOOF_WALK_HARD),
    one(Sfx::Hoofs(Gait::Trot, Footing::Hard), 0.45, HOOF_TROT_HARD),
    // **A gallop on stone is the gallop.** No CC0 gallop on a road was
    // found, and at eleven blocks a second the drum of four hooves is what
    // the ear takes; the clop of each is lost in it.
    one(Sfx::Hoofs(Gait::Gallop, Footing::Hard), 0.6, HOOF_GALLOP),
];

/// The sounds with no recording, and why. Each is silent: nothing is
/// generated in a recording's place any more.
///
/// **Only one, and it is one nothing plays.** Everything the game asks for
/// is in [`RECORDINGS`] -- `every_sound_the_game_asks_for_is_recorded_and_audible`
/// in `bank` plays each one.
pub const SILENT: &[(Sfx, &str)] = &[(
    Sfx::Hover,
    "played nowhere: the interface has no hover sound, and a recording for a \
     sound nothing asks for is download size for nothing",
)];

/// The bytes of one recording: the file under `assets_dir/sounds` if
/// there is one, the copy compiled into the game if not.
///
/// Disk first for the reason it is disk first for textures -- a player
/// can replace `step/grass_3.ogg` without a rebuild -- and the embedded
/// copy second so that an executable moved out of its folder still
/// sounds like the game.
pub fn bytes(assets_dir: &Path, file: &str) -> Option<Cow<'static, [u8]>> {
    let path = assets_dir.join(DIR).join(file);
    if let Ok(read) = std::fs::read(&path) {
        return Some(Cow::Owned(read));
    }
    crate::embedded::sound(file).map(Cow::Borrowed)
}

/// Decodes one Ogg Vorbis file into what the mixer plays: mono, 16-bit,
/// at the device's rate, scaled by `gain`.
///
/// Channels are summed rather than the left one taken, for the reason
/// `bank::read_wav` gives: two channels in opposite phase would cancel
/// to nothing, and the files are mono anyway -- this is for the player's
/// replacement that is not.
pub fn decode(bytes: &[u8], gain: f32, device_rate: u32) -> Result<Clip, String> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let channels = reader.ident_hdr.audio_channels.max(1) as usize;
    let rate = reader.ident_hdr.audio_sample_rate.max(1);
    let mut samples: Vec<i16> = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
        for frame in packet.chunks(channels) {
            let sum: i32 = frame.iter().map(|s| *s as i32).sum();
            let mixed = (sum / channels as i32) as f32 * gain;
            samples.push(mixed.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
        }
    }
    if samples.is_empty() {
        return Err("no audio in the stream".to_string());
    }
    Ok(Clip { samples, sample_rate: rate }.resampled(device_rate))
}

/// Which variant plays next, given how many there are, which one played
/// last, and a random number.
///
/// **Never the same file twice running when there is a choice.** Three
/// recipes picked uniformly repeat one time in three, and at walking pace
/// that is a repeat every second and a half -- the exact failure that
/// killed the first footsteps (see `bank`, "Three of everything"). Picking
/// among the *other* variants costs nothing and removes it: with five
/// grass steps, the ear gets a different one every time.
///
/// `last` is `usize::MAX` before anything has played.
pub fn next_variant(count: usize, last: usize, roll: u64) -> usize {
    if count <= 1 {
        return 0;
    }
    if last >= count {
        return (roll % count as u64) as usize;
    }
    let pick = (roll % (count as u64 - 1)) as usize;
    if pick >= last {
        pick + 1
    } else {
        pick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::bank;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn assets() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("assets")
    }

    fn every_file() -> HashSet<&'static str> {
        RECORDINGS.iter().flat_map(|r| r.files.iter().copied()).collect()
    }

    #[test]
    fn every_sound_is_either_recorded_or_says_why_not() {
        let mut seen = HashSet::new();
        for r in RECORDINGS {
            assert!(!r.files.is_empty(), "{} has a row and no files", r.sfx.file_name());
            assert!(seen.insert(r.sfx.index()), "{} is recorded twice", r.sfx.file_name());
        }
        for (sfx, why) in SILENT {
            assert!(!why.is_empty());
            assert!(seen.insert(sfx.index()), "{} is both recorded and listed as silent", sfx.file_name());
        }
        for sfx in bank::all() {
            assert!(
                seen.contains(&sfx.index()),
                "{} has neither a recording nor a written reason to be silent",
                sfx.file_name()
            );
        }
    }

    #[test]
    fn every_recording_exists_decodes_and_is_short_enough_to_place() {
        for file in every_file() {
            let path = assets().join(DIR).join(file);
            let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let clip = decode(&on_disk, 1.0, 44_100).unwrap_or_else(|e| panic!("{file}: {e}"));
            // Positioning is worked out once, when a sound starts (see
            // `mixer`); a clip much longer than this would be heard from
            // where something *was*. Thunder, the winds and a wolf's howl
            // are the long ones, and only the howl is placed -- from far
            // enough away that a wolf does not cross the stereo field in
            // five seconds.
            assert!(clip.seconds() > 0.05, "{file} is a click of nothing");
            assert!(clip.seconds() < 5.0, "{file} lasts {:.1} s", clip.seconds());
            assert!(
                clip.samples.iter().any(|s| s.unsigned_abs() > 3000),
                "{file} decodes to near-silence"
            );
        }
    }

    #[test]
    fn no_recording_path_would_be_refused_by_an_apk() {
        for file in every_file() {
            assert!(
                file.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/_.".contains(&b)),
                "{file} is not a plain ASCII path, and AAssetManager_open takes a C string"
            );
            assert!(file.ends_with(".ogg") && !file.contains(".."), "{file}");
        }
    }

    #[test]
    fn every_recording_is_compiled_in_and_nothing_else_is() {
        let used = every_file();
        for file in &used {
            assert!(crate::embedded::sound(file).is_some(), "{file} is not in embedded::SOUNDS");
        }
        for (name, bytes) in crate::embedded::SOUNDS {
            assert!(used.contains(name), "{name} is embedded and no sound plays it");
            assert_eq!(&bytes[..4], b"OggS", "{name} is not an Ogg file");
        }
    }

    #[test]
    fn no_file_in_the_sounds_folder_is_forgotten() {
        // A file copied in and never added to the table is download size
        // for a sound nobody hears -- and on Android it is packed and
        // unpacked on every install too.
        let used = every_file();
        let root = assets().join(DIR);
        for dir in std::fs::read_dir(&root).expect("assets/sounds") {
            let dir = dir.unwrap().path();
            if !dir.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(&dir).unwrap() {
                let file = file.unwrap().path();
                let relative = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                assert!(used.contains(relative.as_str()), "{relative} is shipped and never played");
            }
        }
    }

    #[test]
    fn every_recording_says_where_it_came_from() {
        // The licence is the whole reason these files may be here. A file
        // without its line in SOURCES.md is a file nobody can vouch for --
        // and a line in a section whose licence is not CC0 is a file that
        // may not be here at all. Each `## ` section names one source and
        // its licence; every shipped file's row has to sit under a section
        // that says `Licence: CC0 1.0` and gives a page to check it on.
        let sources = std::fs::read_to_string(assets().join(DIR).join("SOURCES.md")).expect("SOURCES.md");
        let sections: Vec<&str> = sources.split("\n## ").skip(1).collect();
        for file in every_file() {
            let row = format!("| `{file}` |");
            let section = sections
                .iter()
                .find(|section| section.contains(&row))
                .unwrap_or_else(|| panic!("{file} has no row in SOURCES.md"));
            let title = section.lines().next().unwrap_or("");
            assert!(section.contains("- Licence: CC0 1.0"), "{file} is listed under \"{title}\", which does not say CC0");
            assert!(section.contains("- Page: https://"), "{file} is listed under \"{title}\", which names no page to check");
        }
    }

    #[test]
    fn a_swimming_stroke_has_finished_before_the_next_one_starts() {
        // Strokes overlapping is the machine gun the old splash effects
        // were: see `soundscape::SWIM_GAP`.
        for file in PLAYER_SWIM {
            let on_disk = std::fs::read(assets().join(DIR).join(file)).unwrap();
            let clip = decode(&on_disk, 1.0, 44_100).unwrap();
            assert!(
                clip.seconds() <= super::super::soundscape::SWIM_GAP.0,
                "{file} lasts {:.2} s and the next stroke can come {:.2} s after it starts",
                clip.seconds(),
                super::super::soundscape::SWIM_GAP.0
            );
        }
    }

    #[test]
    fn the_crickets_the_hooves_and_the_horse_each_have_room_not_to_repeat() {
        // The crickets and the hooves are laid end over end for minutes at
        // a time; a horse's snort is its calm call and its answer to being
        // walked up to. Four is the least that does not come round again
        // audibly with `next_variant` never playing one twice running.
        let four = [
            Sfx::Crickets,
            Sfx::Hoofs(Gait::Walk, Footing::Soft),
            Sfx::Hoofs(Gait::Trot, Footing::Soft),
            Sfx::Hoofs(Gait::Gallop, Footing::Soft),
            Sfx::Hoofs(Gait::Walk, Footing::Hard),
            Sfx::Hoofs(Gait::Trot, Footing::Hard),
            Sfx::Hoofs(Gait::Gallop, Footing::Hard),
            Sfx::Animal(Species::Horse, Cry::Idle),
        ];
        for sfx in four {
            let row = RECORDINGS.iter().find(|r| r.sfx == sfx).unwrap_or_else(|| panic!("{sfx:?} has no row"));
            assert!(row.files.len() >= 4, "{} has {} recordings", sfx.file_name(), row.files.len());
        }
        // ...and a whinny, a wound and a death are rarer, and have more than one.
        for cry in [Cry::Alarm, Cry::Hurt, Cry::Death] {
            let row = RECORDINGS.iter().find(|r| r.sfx == Sfx::Animal(Species::Horse, cry)).unwrap();
            assert!(row.files.len() >= 2 && row.files.iter().all(|f| f.starts_with("wild/horse_")));
        }
        assert_eq!(WORLD_CRICKETS.len(), 4);
    }

    #[test]
    fn wading_swimming_and_the_stakes_each_have_room_not_to_repeat() {
        // One or two clips a second, for as long as somebody is in the
        // water: fewer than six and the loop is audible.
        assert!(PLAYER_SWIM.len() >= 6 && PLAYER_WADE.len() >= 6);
        assert!(PLAYER_STAKED.len() >= 4);
        for (sfx, files) in [(Sfx::Swim, PLAYER_SWIM), (Sfx::Wade, PLAYER_WADE), (Sfx::Staked, PLAYER_STAKED)] {
            let row = RECORDINGS.iter().find(|r| r.sfx == sfx).unwrap_or_else(|| panic!("{sfx:?} has no row"));
            assert_eq!(row.files, files, "{sfx:?} plays somebody else's recordings");
        }
    }

    #[test]
    fn the_recordings_stay_a_modest_download() {
        // They are compiled into the executable and packed into the APK;
        // 3.95 MB today -- half of it the fire, rain, winds and animals that
        // were recipes until the player asked for recordings of everything,
        // 0.4 MB of it the workshops and the crumbles, and 0.27 MB the horse
        // and the crickets, which came in with seven rarely heard fourth
        // variants dropped to make room (SOURCES.md says which).
        // A budget rather than a number, so the next recording has room and
        // the fifty after it do not.
        let total: usize = crate::embedded::SOUNDS.iter().map(|(_, b)| b.len()).sum();
        assert!(total < 4 * 1024 * 1024, "the recordings have grown to {total} bytes");
    }

    #[test]
    fn the_next_variant_is_never_the_one_just_played() {
        for count in 2..8usize {
            for last in 0..count {
                for roll in 0..200u64 {
                    let next = next_variant(count, last, roll);
                    assert!(next < count);
                    assert_ne!(next, last, "{count} variants played {last} twice");
                }
            }
        }
    }

    #[test]
    fn every_other_variant_still_gets_its_turn() {
        // Excluding the last one must not exclude anything else, or five
        // recordings become four.
        for count in 2..8usize {
            for last in 0..count {
                let reached: HashSet<usize> = (0..64).map(|roll| next_variant(count, last, roll)).collect();
                assert_eq!(reached.len(), count - 1, "{count} variants after {last}");
            }
        }
        assert_eq!(next_variant(1, 0, 7), 0);
        assert!(next_variant(4, usize::MAX, 9) < 4);
    }

    #[test]
    fn a_decoded_recording_is_what_plays_and_nothing_plays_before_it() {
        let bank = bank::Bank::new(22_050);
        assert!(bank.pick(Sfx::Click, &mut crate::audio::clip::Rng::new(1)).is_none(), "something played before anything was decoded");
        assert_eq!(bank.recorded(), 0);
        bank.load_recordings(&assets());
        assert_eq!(bank.recorded(), RECORDINGS.len());
        assert_eq!(bank.variants(Sfx::Click), UI_CLICK.len());
        let click = bank.pick(Sfx::Click, &mut crate::audio::clip::Rng::new(1)).unwrap();
        assert_eq!(click.sample_rate, 22_050, "a recording was not resampled to the device");
        for (sfx, _) in SILENT {
            assert_eq!(bank.variants(*sfx), 0, "{} is written down as silent and has something to play", sfx.file_name());
        }
    }

    #[test]
    fn a_missing_folder_falls_back_to_the_compiled_in_copies() {
        let bank = bank::Bank::new(22_050);
        bank.load_recordings(Path::new("this folder does not exist"));
        assert_eq!(bank.recorded(), RECORDINGS.len());
    }

    #[test]
    fn a_resource_pack_file_still_beats_a_recording() {
        let dir = std::env::temp_dir().join(format!("primitive-pack-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(DIR)).unwrap();
        let spec = hound::WavSpec { channels: 1, sample_rate: 22_050, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(dir.join(DIR).join("ui.click.wav"), spec).unwrap();
        for i in 0..2_000 {
            writer.write_sample(((i % 50) as i16 - 25) * 400).unwrap();
        }
        writer.finalize().unwrap();

        let mut bank = bank::Bank::new(22_050);
        bank.load_overrides(&dir.join(DIR));
        bank.load_recordings(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(bank.variants(Sfx::Click), 1, "the pack's one click lost to the recordings");
        assert_eq!(bank.recorded(), RECORDINGS.len() - 1);
    }
}
