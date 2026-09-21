//! What the interface says, in four languages.
//!
//! ## The shape
//!
//! A `Msg` names a thing the game has to say. `STRINGS` is one row per
//! `Msg` with the four translations side by side, so adding a line of
//! interface text is **one enum variant and one row**, and a row that is
//! short a language does not compile.
//!
//! Side by side rather than four separate files. Four files drift: a
//! string is changed in English and the others quietly keep saying the
//! old thing, and nothing anywhere shows that they disagree. In a row
//! the four are on one screen and a stale one is visible while you are
//! editing the one next to it.
//!
//! ## The four
//!
//! * **English** -- the language the game was written in.
//! * **Simple English** -- the same game with the jargon taken out.
//!   "Render distance" is a term of art; "how far you can see" is what
//!   it means. It is for players who read English as a second language
//!   and for anybody who would rather be told plainly, and it is a
//!   language rather than a setting because that is what it behaves
//!   like.
//! * **Russian** and **Polish** -- both need letters ASCII does not
//!   have, which is why the font grew a Cyrillic and a Polish block. See
//!   `engine::font`.
//!
//! ## What is not here
//!
//! Block, item and recipe names. Those are identifiers in `blocks.toml`,
//! in save files and on the wire, and their translations are a table of
//! their own keyed by the identifier: `ui::names`, which says why it is
//! not rows here. Biome names and the causes of a death are tables there
//! too, for the same reason: `names::biome`, `names::death_cause`.

use serde::{Deserialize, Serialize};

use primitive_shared::notice::Notice;

/// A language the interface can be read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    /// English with the jargon taken out. See the module docs.
    SimpleEnglish,
    Russian,
    Polish,
}

/// Whichever of two messages suits the hardware.
///
/// **Half the hints in this game name a key.** "tab switches field",
/// "esc cancels", "R respawn" -- all true on a desktop and all
/// instructions for hardware a phone has not got, printed under a
/// screen the player is meant to be tapping. Every one of them has a
/// touch twin, and this picks between them.
///
/// A compile-time choice rather than a runtime one: a build either has
/// a keyboard behind it or does not, and a desktop with a touchscreen
/// attached is still a desktop -- the same argument
/// `platform::Window::is_touch_primary` makes.
pub fn by_input(keyboard: Msg, touch: Msg) -> Msg {
    if touch_primary() {
        touch
    } else {
        keyboard
    }
}

/// Whether a finger, rather than a mouse and a keyboard, is what points
/// at this build.
///
/// The one switch behind both halves of adapting to a phone: which of
/// two hints [`by_input`] prints, and how big `widgets::Layout` insists
/// a thing worth tapping must be. One answer rather than two, because a
/// build that says "tap YES" and then draws YES too small to tap is
/// worse than either mistake alone.
///
/// Compile-time, for the reason `by_input` gives -- a desktop with a
/// touchscreen attached is still a desktop -- **with one door left
/// open**: the phone's layout has to be reviewable from a desktop, and
/// the alternative to an environment variable is a build that can only
/// be looked at by flashing it to a device. `PRIMITIVE_TOUCH_UI=1` lays
/// a desktop build out the way the phone does.
///
/// Read once. An interface that asks the environment sixty times a
/// second is an interface paying a lock for a constant.
pub fn touch_primary() -> bool {
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        cfg!(target_os = "android") || std::env::var_os("PRIMITIVE_TOUCH_UI").is_some()
    })
}

impl Language {
    /// Every language, in the order the settings screen offers them.
    pub const ALL: &'static [Language] = &[
        Language::English,
        Language::SimpleEnglish,
        Language::Russian,
        Language::Polish,
    ];

    /// What to call this language **in itself**.
    ///
    /// Never translated: a player looking for their own language is
    /// looking for the word they would use for it, and a list that says
    /// "Russian" to somebody who does not read English is a list they
    /// cannot use.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "ENGLISH",
            Language::SimpleEnglish => "SIMPLE ENGLISH",
            Language::Russian => "РУССКИЙ",
            Language::Polish => "POLSKI",
        }
    }

    /// The next one round, for a settings row that steps through them.
    pub fn step(self, delta: i32) -> Language {
        let count = Self::ALL.len() as i32;
        let at = Self::ALL.iter().position(|l| *l == self).unwrap_or(0) as i32;
        Self::ALL[(((at + delta) % count + count) % count) as usize]
    }

    /// What this language calls `msg`.
    pub fn text(self, msg: Msg) -> &'static str {
        let row = STRINGS
            .iter()
            .find(|row| row.msg == msg)
            .unwrap_or(&MISSING);
        match self {
            Language::English => row.en,
            Language::SimpleEnglish => row.simple,
            Language::Russian => row.ru,
            Language::Polish => row.pl,
        }
    }
}

/// Something the interface says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Msg {
    // ---- main menu ----
    Singleplayer,
    Multiplayer,
    Settings,
    Credits,
    Quit,
    Subtitle,
    // ---- worlds ----
    Worlds,
    NoWorldsYet,
    Play,
    New,
    Delete,
    Back,
    WorldsHelp,
    NewWorld,
    Seed,
    Name,
    Create,
    Cancel,
    /// The new-world form's third row, and the two worlds it chooses
    /// between. See `worldgen::Preset`.
    WorldType,
    PresetNormal,
    PresetTest,
    PresetNormalHelp,
    PresetTestHelp,
    /// The new-world form's fourth row: where on the planet the world is
    /// laid. See `worldgen::Zone`.
    Climate,
    ZoneTropics,
    ZoneDryBelt,
    ZoneTemperate,
    ZoneNorth,
    ZoneTropicsHelp,
    ZoneDryBeltHelp,
    ZoneTemperateHelp,
    ZoneNorthHelp,
    // ---- the hearth screen ----
    Campfire,
    Kiln,
    Bloomery,
    HearthInputs,
    HearthFuel,
    HearthOutput,
    HearthUnlit,
    HearthIdle,
    HearthWorking,
    // ---- the fire's heat ----
    //
    // The heat is named in the colours a smith reads -- TerraFirmaCraft's
    // -- rather than printed in degrees: "orange" says what the fire is
    // doing in a word a player already has, and the font has no degree
    // sign. See `hearth::Glow`. The adjectives agree with "жар"/"żar",
    // which is what they describe on the screen and in the status line.
    HearthAsh,
    HearthHeat,
    HearthTooCool,
    HearthRained,
    HearthCharring,
    HearthCooling,
    GlowCold,
    GlowWarm,
    GlowHot,
    GlowVeryHot,
    GlowFaintRed,
    GlowDarkRed,
    GlowBrightRed,
    GlowOrange,
    GlowYellow,
    GlowWhite,
    // ---- the drying rack ----
    //
    // A rack says more than a hearth does, and it has to: a fire that is
    // doing nothing is a fire a player can see is out, and a rack that
    // is doing nothing looks exactly like a rack that is working. Every
    // reason it can be stopped for is a line here.
    DryingRack,
    RackSkins,
    RackLeather,
    RackWeather,
    RackDrying,
    RackSmoking,
    RackWet,
    RackFrozen,
    RackEmpty,
    RackFull,
    MinutesLeft,
    DeleteThisWorld,
    SeedHelp,
    /// What an empty seed box says it will do: roll one.
    SeedRandom,
    /// The button beside the seed box that rolls one into it now, so the
    /// player sees the number before the world is made from it.
    RollSeed,
    WorldFormHelp,
    /// The same hint for a screen with no keyboard on it.
    WorldFormHelpTouch,
    NeverPlayed,
    JustNow,
    MinutesAgo,
    HoursAgo,
    DaysAgo,
    // ---- confirmation ----
    CannotBeUndone,
    ConfirmHelp,
    /// The same, for a device that is tapped rather than typed at.
    ConfirmHelpTouch,
    // ---- servers ----
    Servers,
    Connect,
    Address,
    Add,
    Connecting,
    NoServersYet,
    Edit,
    ServersHelp,
    EditServer,
    AddServer,
    AddressHelp,
    Save,
    ServerFormHelp,
    /// The same, for a device that is tapped rather than typed at.
    ServerFormHelpTouch,
    CannotConnect,
    Retry,
    AddressRequired,
    // ---- settings ----
    LanguageRow,
    RenderDistance,
    FieldOfView,
    Fog,
    MouseSensitivity,
    /// The same row, on a device that turns the view with a thumb.
    LookSensitivity,
    /// How big the interface is drawn. See `ClientSettings::ui_scale`.
    UiScale,
    /// The master volume row. There is no effects row under it any
    /// more: the effects were cut from the game (see `audio`), and
    /// music is the only thing left to be loud.
    Volume,
    Music,
    On,
    Off,
    Apply,
    Vsync,
    /// What share of the screen the game is drawn at. See
    /// `ClientSettings::resolution_scale`.
    ///
    /// Named RESOLUTION rather than "resolution scale": a player who
    /// has ever changed this in another game changed a row called
    /// resolution, and the row shows a percentage, which says on its
    /// own that it is a share of something.
    Resolution,
    /// The step of a row where the game decides for itself. Only the
    /// resolution row has one so far.
    Auto,
    AmbientOcclusion,
    Anisotropy,
    /// How finely the sky is drawn. See `ClientSettings::sky_scale`.
    ///
    /// Named for what a player is choosing rather than for what
    /// the renderer does: the setting is a divisor on the sky's
    /// own resolution, and "SKY RESOLUTION 1/3" is a sentence
    /// about the implementation. The three levels read the way
    /// every other game's graphics menu reads.
    SkyQuality,
    QualityHigh,
    QualityMedium,
    QualityLow,
    TransparentLeaves,
    /// The far end of a distance that has one: "no limit". Shown by the
    /// see-through leaves row, where a number would be a thousand chunks.
    Everywhere,
    Shadows,
    /// The shadows row's two steps past OFF. See `shadow::Mode`.
    ShadowsHard,
    ShadowsSoft,
    /// How far the sun's shadows reach, and the unit it is shown in.
    ShadowDistance,
    Blocks,
    /// Which plants cast the sun's shadow, and its two steps past OFF. See
    /// `shadow::PlantShadows`.
    PlantShadows,
    PlantShadowsTrees,
    PlantShadowsAll,
    /// The lighting quality row. Its three values are the sky row's
    /// LOW / MEDIUM / HIGH, for the reason given at `SkyQuality`.
    LightingQuality,
    DetailDistance,
    ReliefDistance,
    LodDistance,
    LodQuality,
    LodFine,
    LodNormal,
    LodCoarse,
    Cloudiness,
    MenuBackground,
    /// Which kind of place the menu stands in front of. It replaced a
    /// row that chose a *block*, back when the backdrop was one texture
    /// tiled across the screen.
    MenuBackgroundScene,
    SceneRandom,
    SceneShore,
    SceneForest,
    ScenePlains,
    SceneCave,
    Chunks,
    Degrees,
    Toggle,
    Controls,
    /// The screen where the on-screen controls are dragged into place.
    ArrangeControls,
    /// Put the arrangement back the way the game shipped it.
    ResetControls,
    /// The one line of help on the arrangement screen.
    DragToArrange,
    /// The screen where the thumb controls are arranged.
    /// What to do on that screen, in one line.
    /// The four numbers on a held control.
    /// Switch the held control on or off.
    /// Put everything back where it started.
    /// Pick up the next control in order.
    Done,
    SettingsHelp,
    // ---- controls ----
    PressAKey,
    ResetToDefaults,
    ControlsHelp,
    KeyBound,
    KeyCannotBind,
    WalkForward,
    WalkBack,
    StrafeLeft,
    StrafeRight,
    Jump,
    Sprint,
    Rein,
    DropItem,
    ToggleFog,
    ToggleStats,
    ToggleHud,
    Fullscreen,
    // ---- credits ----
    RoleTextures,
    RoleCode,
    RoleEngine,
    // ---- in play ----
    Inventory,
    Crafting,
    /// Heading over the four equipment squares on the pack screen.
    ///
    /// **Four letters or fewer in every language**, because the column
    /// it names is one slot wide. `every_translated_message_fits_its_box`
    /// is what holds that.
    Worn,
    Paused,
    Resume,
    Respawn,
    YouDied,
    LeaveWorld,
    QuitToMenu,
    DeathHelp,
    /// The same, for a device that is tapped rather than typed at.
    DeathHelpTouch,
    // ---- inventory screen ----
    TidyPile,
    NoRoom,
    /// A recipe that wants a fire and has not got one. Its own line
    /// rather than a shade of "need", because what the player has to do
    /// about it is completely different: not find more tin, but go and
    /// stand by a fire.
    NeedFire,
    /// Shown on a recipe that wants a *kiln* rather than any fire. Its
    /// own line because "needs a fire" to a player standing at a blazing
    /// campfire is a menu that lies -- see `Feasibility::NeedsForge`.
    NeedKiln,
    /// ...and a recipe that wants the iron shaft.
    NeedBloomery,
    /// ...and the four that want a workshop. One line per workshop rather
    /// than "needs" and the block's name, because the word agrees with the
    /// thing in three of the four languages: нужен верстак, нужна рама.
    NeedBench,
    NeedMason,
    NeedWheel,
    NeedLeatherBench,
    /// The hunger bar's own label, for the F3 panel.
    Nourishment,
    /// What the weather is doing, on the same panel.
    WeatherLabel,
    WeatherClear,
    WeatherRain,
    WeatherStorm,
    /// The key hints for the two gestures 1.5 added.
    Eat,
    UseBlock,
    Need,
    No,
    Of,
    KgCarried,
    /// The unit alone, after a weight in a tooltip or a container's summary.
    Kg,
    /// The words on the thumb controls that name a place or an act rather
    /// than a key. See `settings::Emits::label_in`.
    ThumbMine,
    ThumbPlace,
    ThumbMenu,
    ThumbChat,
    ThumbJump,
    ThumbPack,
    ThumbMap,
    Speed,
    // ---- chest screen ----
    Chest,
    /// The same screen over a dead player's pack. A separate word
    /// because the two blocks mean opposite things -- one is where you
    /// chose to put something, the other is where you lost it -- and the
    /// heading is the only thing on the screen that says which.
    Backpack,
    /// The same screen over your own body, where you died.
    ///
    /// Its own word rather than the pack's, and for the pack's own reason:
    /// the heading is the only thing on the screen that says what is being
    /// opened, and "what you lost" and "what is left of you" are not the
    /// same sentence. See `types::BLOCK_CORPSE`.
    Corpse,
    /// ...and over what is left of it once the ground has had the soft
    /// half. See `types::BLOCK_REMAINS`.
    Remains,
    /// The same screen over a jug, set down or in hand. One slot of loose
    /// goods -- see `types::opens_as_vessel`.
    Jug,
    Saddlebags,
    /// What an empty jug's screen says: what may go in. The one thing a
    /// player cannot see from the slot, because the refusal is silent --
    /// a stack of meat dropped on the slot simply does not go in.
    VesselEmpty,
    /// ...and a jug at its measure, which is the other silent refusal.
    VesselFull,
    /// What a body square is for: head, chest, legs, feet.
    ///
    /// **A word, and it used to be a letter.** The four squares were
    /// labelled `H C L F`, and a player who has not read this file
    /// cannot tell whether `C` is a chest piece or a cape -- on a phone
    /// least of all, where there is no pointer to rest on a square and
    /// no tooltip to rest it for. What answers that on the glass is the
    /// ghost of the garment drawn in the empty square
    /// (`inventory_screen::GHOST`); these are the same answer in words,
    /// for the note under a mouse pointer, and they are free to be as
    /// long as the language needs because they are no longer being
    /// squeezed into a slot.
    SlotHead,
    SlotChest,
    SlotLegs,
    SlotFeet,
    /// The fifth square: what is slung over the shoulders.
    SlotBack,
    // ---- the pack screen's three tabs ----
    //
    // Short on purpose. Three of them share one strip across the top of
    // the panel, and the strip has to hold the longest of the three in
    // the longest of the four languages at a size a thumb can read --
    // see `inventory_screen::tab_rect`, which shrinks the text to fit
    // rather than letting it run out of its tab.
    TabHealth,
    TabPack,
    TabBackpack,
    // ---- the health page ----
    //
    // A label per vital the server sends, plus the two lines that
    // explain the page when there is nothing to report. The values
    // beside them are numbers and percentages, which need no
    // translation; these are the words.
    VitalHealth,
    VitalHunger,
    VitalThirst,
    VitalStamina,
    VitalTiredness,
    VitalWarmth,
    VitalWetness,
    VitalDirt,
    VitalDiet,
    /// Comfort's *effect*, never comfort itself -- see the note on the
    /// health page for why a hidden number stays hidden.
    VitalRecovery,
    /// The heading over the wounds half of the health page.
    VitalInjuries,
    /// What the backpack tab says when nothing is worn on the back.
    NoBackpack,
    Stored,
    Carried,
    StoreAll,
    TakeAll,
    SlotsWord,
    ItemsWord,
    /// The one line of instruction a container screen still prints.
    ///
    /// There were two. The other said that a left click takes and
    /// places and a right click halves, which is true of every slot in
    /// this game and of most games this one resembles -- permanent text
    /// telling the reader what they already know, paid for in the panel
    /// height a phone has least of. See `chest_screen::hint`.
    ChestHint2,
    /// The same, for a device that is tapped rather than typed at.
    ChestHint2Touch,
    WorldsHelpTouch,
    ServersHelpTouch,
    NoServersYetTouch,
    NoWorldsYetTouch,
    // ---- what is extending the server ----
    //
    // One screen for both extension points, because a player asking
    // "what is running here" is not asking which of two loaders it came
    // through. Which one it *was* is a column on the row, not a
    // separate list -- see `Menu::build_extensions`.
    Extensions,
    /// The question is out and the answer has not come back.
    ExtensionsAsking,
    /// The server answered, and it is running nothing.
    ExtensionsNone,
    /// ...and cannot run anything, which is a different sentence.
    ExtensionsNoLoader,
    /// ...and on a phone it is a third sentence again: it is not that
    /// this world happens to have no loader, it is that the package
    /// carries none and no world opened from it ever will. Saying the
    /// desktop line there would read as "join a server and you will get
    /// mods", which is the wrong thing to go looking for.
    ExtensionsNoLoaderPhone,
    ExtensionScript,
    ExtensionNative,
    ExtensionBy,
    ExtensionSettings,
    ExtensionNoSettings,
    ExtensionOff,
    ExtensionOn,
    /// The contract version the server's native loader speaks, and the
    /// one a mod was built against.
    ExtensionApi,
    ExtensionsReadOnly,
    ExtensionsHelp,
    /// The same, for a device that is tapped rather than typed at.
    ExtensionsHelpTouch,
    /// The badge on a shut chat log: how many things people said while
    /// nobody was reading. See `chat::Chat::unread`.
    ChatUnread,
    // ---- what an empty field is for ----
    //
    // A placeholder rather than a label alone, because a well with
    // nothing in it says nothing about what belongs there -- and the
    // one on the seed row replaces a real value that used to be typed
    // *into* the field, which read as a number somebody had chosen.
    WorldNamePlaceholder,
    ServerNamePlaceholder,
    AddressPlaceholder,
    UsernamePlaceholder,
    // ---- what the strips over the hotbar are ----
    //
    // Printed once, on the pause screen, beside the same marks the
    // gauges themselves carry -- see `hud::GAUGE_LEGEND`. Nouns rather
    // than sentences: they are read against a picture of the thing they
    // name, and the picture is doing most of the work.
    GaugesTitle,
    GaugeWarmth,
    GaugeAir,
    GaugeWater,
    GaugeRest,
    GaugeHealth,
    GaugeStamina,
    GaugeFood,
    // ---- the heat ----
    //
    // Said once, when the warmth gauge crosses a line, and not again
    // while it stays there -- see `hud::heat_notice`. Worded for heat
    // from anywhere rather than for the sun, because a player sitting
    // too close to a fire in summer crosses the same line, and "find
    // shade" said to somebody indoors reads as a bug.
    HeatRising,
    HeatStroke,
    HeatEased,
    // ---- sleep ----
    //
    // Said on the dark a sleeper's screen goes (`ui::sleep`), and once when
    // the morning has lifted it. They replaced "asleep -- press any key to
    // get up" and "awake", which were English on every screen and the first
    // of which was not even true: only walking and jumping ever got anybody
    // up.
    SleepGetUp,
    SleepWaiting,
    SleepMorning,
    // ---- on the ground ----
    //
    // Said over the red of a body that has given out and is not yet dead
    // (`ui::downed`): what put it there, what gets it up, and how to let go.
    // One line per cause and per rescue rather than one sentence with a
    // blank in it, because "the ____ has taken your legs" does not decline
    // in Russian or Polish, and a sentence that only reads in English is a
    // sentence the other three languages would carry broken.
    DownedTitle,
    DownedWound,
    DownedBleeding,
    DownedFall,
    DownedBurn,
    DownedCold,
    DownedHeat,
    DownedHunger,
    DownedThirst,
    DownedSickness,
    DownedSmoke,
    SavedByDressing,
    SavedBySplint,
    SavedByWarmth,
    SavedByCooling,
    SavedByFood,
    SavedByWater,
    SavedByAir,
    DownedGiveUp,
    DownedGiveUpTouch,
    // ---- fishing ----
    //
    // Said in place of a cast or a reach into a trap that would take nothing
    // (`logic::fishing::Notice`). The client judges these from the same
    // survey the server does, so they can be said in the player's language
    // rather than arriving from the server in English.
    FishingTooSmall,
    FishingTooShallow,
    FishTrapEmpty,
    FishTrapDry,
    /// A throw that came down on dry land, or against a wall.
    FishingNoWater,
    /// A strike at open water: the cast is over.
    FishingStruckAtNothing,
    /// The line parted, or the hook pulled out.
    FishingLineGone,
    /// A fish landed, before its name (`ui::names::animal`): "caught: trout".
    /// A label and a colon rather than a sentence with the name inside it,
    /// because the name changes its ending in three of the four languages and
    /// a colon asks nothing of it.
    FishingCaught,
    // Said in place of setting a thing down where the server would refuse
    // it (`set_down_cell`): the side of a block, water, a drift.
    SetDownWhere,
    /// The rack of two by two refused a skin: it is the larder now, and the
    /// hide frame is where a skin goes (`ServerMessage::RackRefused`).
    LarderRefusesSkins,
    /// ...and the hide frame refused food: that hangs on the big rack.
    FrameRefusesFood,
    // ---- the journal: the map and the recipe book ----
    //
    // Block and recipe names inside it stay untranslated, for the reason
    // the module docs give; everything around them is here.
    MapTab,
    RecipesTab,
    MapYou,
    MapSpawn,
    MapBag,
    MapUnexplored,
    MapCentre,
    MapHelp,
    MapHelpTouch,
    RecipesAll,
    RecipesHands,
    /// The recipe book's chip for all four workshops at once: four chips
    /// more would push the search field off a square window.
    RecipesWorkshops,
    RecipesSearch,
    RecipesEmpty,
    RecipesNoMatch,
    RecipesHelp,
    RecipesHelpTouch,
    /// The journal's way out on a phone, said in its header. See
    /// `journal::Header`.
    JournalCloseTouch,
    // ---- the give menu ----
    //
    // The names of the things it lists are *not* here, for the reason
    // the module docs give: a block's name is an identifier in
    // `blocks.toml`, in a save file and in the very `/give` this screen
    // sends. Everything that is the screen's own voice is.
    GiveTab,
    GiveBlocks,
    GiveTools,
    GiveClothes,
    GiveFood,
    GiveStuff,
    GiveHelp,
    GiveHelpTouch,
    /// The status bar before anything has been asked for.
    GiveHint,
    GiveAsking,
    GiveGiven,
    GivePackFull,
    /// **The sentence this whole screen's feedback exists for.** Without
    /// it the menu is a list that does nothing when tapped, which reads
    /// as a broken game rather than as a permission the player has not
    /// got. See `give_screen::Status::Denied`.
    GiveDenied,
    /// Prefixes whatever else the server said, which stays in the
    /// server's own English: a wrong translation of an unexpected
    /// sentence is worse than an untranslated true one.
    GiveRefused,
    RecipeMadeFrom,
    RecipeWhere,
    RecipeByHand,
    RecipeAtFire,
    RecipeAtKiln,
    RecipeAtBloomery,
    RecipeAtBench,
    RecipeAtMason,
    RecipeAtWheel,
    RecipeAtLeatherBench,
    RecipeKept,
    RecipeLead,
    RecipeNotFound,
    RecipeMayFail,
    /// The heading over everything standing between the pack and this
    /// row. See `crafting::Shortfall`.
    RecipeMissing,
    /// Prefixes the tool a row is worked with when the pack holds none of
    /// it at any rung -- "a tool: chisel". A prefix rather than a
    /// sentence, because what follows is a block name and block names are
    /// not translated (see the journal's note).
    RecipeNeedTool,
    /// A hone with every tool of that kind already sharp: everything the
    /// row asks for is in the pack and there is nothing for it to do.
    RecipeNothingToWork,
    /// Prefixes the colour the fire has to reach, which is said in the
    /// words a smith reads rather than in degrees -- see `Msg::GlowCold`
    /// and the note above it about the font having no degree sign.
    RecipeHeat,
    /// The heading over what the thing a row makes is itself good for:
    /// the book read backwards (`crafting::uses`).
    RecipeUsedFor,
    /// ...and when nothing this player knows of takes it.
    RecipeUsedForNothing,
    // ---- the path page, which is a tab of the pack ----
    //
    // The seven ages, where their materials come from, and the four words
    // the page says in its own voice. Block names inside it stay
    // untranslated, as they do everywhere else. `LadderTab` is the tab's
    // word and it kept its name -- the page is still the ladder, it has
    // just moved out of the journal (see `ui::ladder_screen`).
    LadderTab,
    /// Against the highest rung this player has taken.
    LadderYouAreHere,
    /// The heading over the rung above that one.
    LadderNext,
    /// The heading over everything that rung takes. What is already held
    /// is green and what is not is pale, so one heading covers both -- a
    /// heading that said "still to find" over a list three quarters of
    /// which is in the pack would be arguing with its own colours.
    LadderWants,
    /// Said once, at the foot, for a player who has climbed all seven.
    LadderDone,
    AgeBareHands,
    AgeFlint,
    AgeFire,
    AgeClay,
    AgeCopper,
    AgeBronze,
    AgeIron,
    FoundUnderfoot,
    FoundRiverbank,
    FoundHills,
    FoundDeepRock,
    FoundWoods,
    // ---- the first two minutes, and the page that carries them ----
    //
    // Three sentences, each a *fact about the world* rather than an order.
    // They were orders -- "pick a stone up off the ground" -- and the
    // player's reply was "что за тупые подсказки". A game that tells a
    // grown adult to pick up a stone is a game talking down to them; a
    // game that mentions there are stones on the ground has told them the
    // same thing and left the decision where it belongs. See
    // `hud::first_minute_line` for the one that is still drawn over the
    // belt and the one condition it is drawn under.
    StepStone,
    StepFibre,
    StepFlake,
    // ---- the path page, in the pack ----
    //
    // The tab is `LadderTab`, which is the word it already had. What is
    // new is the three headings the page adds to the ladder it grew out
    // of: what to do now, the handful of controls that are not guessable,
    // and what this player has turned up. See `ui::ladder_screen`.
    LearnNow,
    LearnControls,
    /// The two mouse buttons, which no key binding names and which are
    /// the whole of how the world is touched.
    LearnDig,
    LearnPlace,
    /// ...and the same two under a thumb, where there are no buttons.
    LearnDigTouch,
    LearnPlaceTouch,
    /// How much of the world this player has turned up: kinds held, out
    /// of every kind there is.
    LearnFound,
    // ---- the body in the pack ----
    //
    // The mannequin's heading, its four parts that are not also equipment
    // squares (the head and the body borrow `SlotHead` and `SlotChest` --
    // see `mannequin::part_name`), the four kinds of wound, three words for
    // how bad, what each wound needs, and the five lines said when the body
    // changes. Every wound kind is masculine in Russian and neuter in
    // Polish (`SKALECZENIE` rather than `RANA CIĘTA` for exactly that), so
    // one severity word agrees with all four.
    Wounds,
    PartLeftArm,
    PartRightArm,
    PartLeftLeg,
    PartRightLeg,
    WoundCut,
    WoundBruise,
    WoundFracture,
    WoundBurn,
    SeverityLight,
    SeveritySerious,
    SeveritySevere,
    WoundBleeding,
    WoundNeedsSplint,
    WoundNeedsDressing,
    WoundHealsAlone,
    WoundBandaged,
    WoundSplinted,
    WoundPoultice,
    NoWounds,
    NoticeBleeding,
    NoticeBleedingStopped,
    NoticeBoneBroken,
    NoticeBoneKnit,
    NoticeBadBurn,
    // ---- the anvil and the potter's wheel ----
    //
    // The names of the two screens and the four things they say. The *jobs*
    // are not here: they are identifiers (`minigame::Job::name`) and are
    // translated by `ui::names` with every other block and recipe, for the
    // reason the module docs give at the top.
    AnvilTitle,
    WheelTitle,
    StationPickJob,
    StationStrike,
    StationStrikeTouch,
    RunFine,
    RunFair,
    RunRuined,

    // ---- how well a thing was made ----
    //
    // The four words under an item in the pack (`quality::Band`). Here
    // rather than in `ui::names` because they are not identifiers: a
    // block has a name and this is a *judgement*, and the four are
    // written to be read in the fraction of a second a tooltip gets.
    QualityPoor,
    QualityPlain,
    QualityGood,
    QualityFine,
    /// The word for the thing in your storeroom. Nothing in the
    /// interface names a live animal -- `animals::Species::name` is an
    /// identifier and says so -- but the rat is the first one a player
    /// is meant to *talk* about, and the manual and the debug line both
    /// want it in the language they are reading.
    AnimalRat,
    /// The place, on the health page under the wounds: the air the skin is
    /// drifting towards, and what about the room is making it so. See
    /// `inventory_screen::shelter_lines`.
    ShelterAir,
    ShelterSmoky,
    ShelterDraughty,
    ShelterHoleTakesHeat,
    ShelterWallsWarm,
    ShelterWallsThin,
    /// A cairn on the map's legend. See `types::BLOCK_CAIRN`.
    MapCairn,
    /// What the chat box says while it is asking for a cairn's name.
    CairnNamePrompt,
    /// The sky read for a bearing (`logic::bearing`): which body it was
    /// read off, then where north is from where the player looks. Two
    /// halves rather than twelve sentences, because each half is a whole
    /// phrase in every language here and the pair reads as "by the sun:
    /// north is to your left".
    SkyBySun,
    SkyByMoon,
    SkyByStars,
    NorthAhead,
    NorthRight,
    NorthBehind,
    NorthLeft,
    // ---- the barter stall ----
    //
    // Its heading, the three places on its screen, its two verbs, the two
    // footer lines -- one for the owner and one for everybody else, because
    // the two are doing different things at the same counter -- and a line
    // for each of the server's refusals (`stall::Refusal`).
    Stall,
    StallPrices,
    StallYourPrices,
    StallStock,
    StallTakings,
    StallTrade,
    StallClear,
    StallLeft,
    StallHintOwner,
    StallHintBuyer,
    StallNotYours,
    StallNoOffer,
    StallOfferChanged,
    StallSoldOut,
    StallCannotPay,
    StallNoRoom,
    StallTillFull,
    StallBadOffer,
    // ---- the sawhorse and the honing stone ----
    //
    // The two new stations' headings. Their jobs are names, like the anvil's.
    SawhorseTitle,
    HoningTitle,
    // ---- the larder, the trapline and the pack ----
    //
    // Willow bark on the mannequin, and what is said in place of a reach
    // into a snare or a salt pan that would take nothing -- judged on the
    // client from the block, as the fish trap's are (`logic::fishing`), so
    // they are said in the player's language.
    WoundBarked,
    /// What the server tells a player, as a code (`notice`): one row per
    /// notice, checked against `Notice::ALL`.
    Notice(primitive_shared::notice::Notice),
    // ---- the ages as journeys ----
    /// Where the bronze rung's tin is: `ladder::Found::FarCountry`.
    FoundFarCountry,
}

/// One line of interface text, in every language at once.
pub struct Line {
    pub msg: Msg,
    pub en: &'static str,
    /// English with the jargon taken out. Often the same string, and
    /// that is fine -- most of the interface is already plain.
    pub simple: &'static str,
    pub ru: &'static str,
    pub pl: &'static str,
}

/// What a `Msg` with no row resolves to.
///
/// Visible on purpose. A missing string that falls back to English looks
/// like a translation nobody got round to; one that says `???` looks
/// like the bug it is.
const MISSING: Line = Line {
    msg: Msg::Play,
    en: "???",
    simple: "???",
    ru: "???",
    pl: "???",
};

/// Every line of interface text.
#[rustfmt::skip]
pub const STRINGS: &[Line] = &[
    // How well a thing was made. The English is a craftsman's word in
    // each case rather than a grade: "poor work" and "fine work" are what
    // somebody says holding the thing, and "plain" is the honest word for
    // what most work is.
    Line { msg: Msg::QualityPoor,  en: "poor work",  simple: "badly made",  ru: "плохая работа", pl: "licha robota" },
    Line { msg: Msg::QualityPlain, en: "plain work", simple: "ordinary",    ru: "обычная работа", pl: "zwykła robota" },
    Line { msg: Msg::QualityGood,  en: "good work",  simple: "well made",   ru: "хорошая работа", pl: "dobra robota" },
    Line { msg: Msg::QualityFine,  en: "fine work",  simple: "beautifully made", ru: "отличная работа", pl: "wyborna robota" },
    Line { msg: Msg::AnimalRat,    en: "rat",        simple: "rat",         ru: "крыса", pl: "szczur" },

    Line { msg: Msg::Singleplayer, en: "SINGLEPLAYER", simple: "PLAY ALONE", ru: "ОДИНОЧНАЯ ИГРА", pl: "GRA JEDNOOSOBOWA" },
    Line { msg: Msg::Multiplayer,  en: "MULTIPLAYER",  simple: "PLAY TOGETHER", ru: "ПО СЕТИ", pl: "GRA WIELOOSOBOWA" },
    Line { msg: Msg::Settings,     en: "SETTINGS",     simple: "SETTINGS",   ru: "НАСТРОЙКИ", pl: "USTAWIENIA" },
    Line { msg: Msg::Credits,      en: "CREDITS",      simple: "WHO MADE IT", ru: "АВТОРЫ", pl: "AUTORZY" },
    Line { msg: Msg::Quit,         en: "QUIT",         simple: "LEAVE",      ru: "ВЫХОД", pl: "WYJŚCIE" },
    Line { msg: Msg::Subtitle,     en: "a voxel world", simple: "a world of blocks", ru: "воксельный мир", pl: "świat wokseli" },

    Line { msg: Msg::Worlds,       en: "WORLDS",       simple: "YOUR WORLDS", ru: "МИРЫ", pl: "ŚWIATY" },
    Line { msg: Msg::NoWorldsYet,  en: "no worlds yet -- press NEW", simple: "no worlds yet -- press NEW", ru: "миров пока нет -- нажмите СОЗДАТЬ", pl: "brak światów -- naciśnij NOWY" },
    Line { msg: Msg::Play,         en: "PLAY",         simple: "PLAY",       ru: "ИГРАТЬ", pl: "GRAJ" },
    Line { msg: Msg::New,          en: "NEW",          simple: "MAKE ONE",   ru: "СОЗДАТЬ", pl: "NOWY" },
    Line { msg: Msg::Delete,       en: "DELETE",       simple: "THROW AWAY", ru: "УДАЛИТЬ", pl: "USUŃ" },
    Line { msg: Msg::Back,         en: "BACK",         simple: "GO BACK",    ru: "НАЗАД", pl: "WSTECZ" },
    Line { msg: Msg::WorldsHelp,   en: "up/down select   enter play   N new   del remove", simple: "up/down choose   enter play   N make one   del throw away", ru: "вверх/вниз выбрать   ввод играть   N создать   del удалить", pl: "góra/dół wybór   enter graj   N nowy   del usuń" },
    Line { msg: Msg::NewWorld,     en: "NEW WORLD",    simple: "A NEW WORLD", ru: "НОВЫЙ МИР", pl: "NOWY ŚWIAT" },
    Line { msg: Msg::Seed,         en: "SEED",         simple: "WORLD NUMBER", ru: "СИД", pl: "ZIARNO" },
    Line { msg: Msg::Name,         en: "NAME",         simple: "NAME",       ru: "ИМЯ", pl: "NAZWA" },
    Line { msg: Msg::Create,       en: "CREATE",       simple: "MAKE IT",    ru: "СОЗДАТЬ", pl: "UTWÓRZ" },
    Line { msg: Msg::Cancel,       en: "CANCEL",       simple: "NEVER MIND", ru: "ОТМЕНА", pl: "ANULUJ" },
    Line { msg: Msg::DeleteThisWorld, en: "DELETE THIS WORLD?", simple: "THROW THIS WORLD AWAY?", ru: "УДАЛИТЬ ЭТОТ МИР?", pl: "USUNĄĆ TEN ŚWIAT?" },
    Line { msg: Msg::WorldType,    en: "WORLD TYPE",   simple: "WORLD TYPE", ru: "ТИП МИРА", pl: "TYP ŚWIATA" },
    Line { msg: Msg::PresetNormal, en: "NORMAL",       simple: "THE USUAL ONE", ru: "ОБЫЧНЫЙ", pl: "ZWYKŁY" },
    Line { msg: Msg::PresetTest,   en: "TEST",         simple: "A WORLD TO TRY THINGS IN", ru: "ТЕСТОВЫЙ", pl: "TESTOWY" },
    Line { msg: Msg::PresetNormalHelp, en: "oceans, mountains, rivers and caves -- the game", simple: "seas, hills, rivers and caves -- the real game", ru: "океаны, горы, реки и пещеры -- обычная игра", pl: "oceany, góry, rzeki i jaskinie -- zwykła gra" },
    Line { msg: Msg::PresetTestHelp, en: "a flat field with one of everything already built on it", simple: "flat ground with one of everything already made", ru: "ровное поле, где всего уже построено по одному", pl: "płaskie pole, na którym wszystko już stoi" },
    // **A place, not a property of the world.** The row used to say
    // "climate", which was true about living there and silent about what
    // the choice now is: one seed is one planet, and this picks the corner
    // of it you wake in (`worldgen::PLANET_ORIGIN_DEGREES`). A player who
    // read "climate" had every reason to think the two worlds were two
    // worlds.
    Line { msg: Msg::Climate,      en: "WHERE YOU WAKE", simple: "WHERE ON EARTH YOU START", ru: "ГДЕ ПРОСНЁТЕСЬ", pl: "GDZIE SIĘ OBUDZISZ" },
    Line { msg: Msg::ZoneTropics,  en: "TROPICS",      simple: "HOT AND WET", ru: "ТРОПИКИ", pl: "TROPIKI" },
    Line { msg: Msg::ZoneDryBelt,  en: "DRY BELT",     simple: "HOT AND DRY", ru: "СУХОЙ ПОЯС", pl: "PAS SUCHY" },
    Line { msg: Msg::ZoneTemperate, en: "TEMPERATE",   simple: "MILD", ru: "УМЕРЕННЫЙ", pl: "UMIARKOWANY" },
    Line { msg: Msg::ZoneNorth,    en: "NORTH",        simple: "COLD", ru: "СЕВЕР", pl: "PÓŁNOC" },
    Line { msg: Msg::ZoneTropicsHelp, en: "8 degrees north: savanna, warm swamps, palms, hardly a winter", simple: "near the middle of the Earth: grass, swamps, palm trees, no real winter", ru: "8 градусов: саванна, тёплые болота, пальмы, зимы почти нет", pl: "8 stopni: sawanna, ciepłe bagna, palmy, prawie bez zimy" },
    Line { msg: Msg::ZoneDryBeltHelp, en: "25 degrees north: saxaul desert inland, palms on the coast, little water", simple: "sand and grey trees far from the sea, palms by the sea, little to drink", ru: "25 градусов: пустыня с саксаулом, пальмы у моря, воды мало", pl: "25 stopni: pustynia z saksaułem, palmy na brzegu, mało wody" },
    Line { msg: Msg::ZoneTemperateHelp, en: "45 degrees north: oak and birch, meadows and marshes, a real winter", simple: "woods, fields and marshes, and a cold winter to get ready for", ru: "45 градусов: дуб и берёза, луга и болота, настоящая зима", pl: "45 stopni: dęby i brzozy, łąki i bagna, prawdziwa zima" },
    Line { msg: Msg::ZoneNorthHelp, en: "60 degrees north: firs, bogs, frozen lakes, dress warm", simple: "pine woods, bogs and ice: you will need warm clothes", ru: "60 градусов: ели, топи, замёрзшие озёра, одевайтесь теплее", pl: "60 stopni: jodły, torfowiska, zamarznięte jeziora, ubierz się ciepło" },
    Line { msg: Msg::Campfire,     en: "CAMPFIRE",     simple: "CAMPFIRE",  ru: "КОСТЁР", pl: "OGNISKO" },
    Line { msg: Msg::Kiln,         en: "KILN",         simple: "CLAY OVEN", ru: "ГОРН", pl: "PIEC" },
    Line { msg: Msg::Bloomery,     en: "BLOOMERY",     simple: "IRON FURNACE", ru: "ДОМНИЦА", pl: "DYMARKA" },
    Line { msg: Msg::HearthInputs, en: "GOES IN",     simple: "PUT THINGS HERE", ru: "ЧТО КЛАСТЬ", pl: "SKŁADNIKI" },
    Line { msg: Msg::HearthFuel,   en: "FUEL",        simple: "WOOD OR COAL", ru: "ТОПЛИВО", pl: "PALIWO" },
    Line { msg: Msg::HearthOutput, en: "COMES OUT",   simple: "TAKE THINGS HERE", ru: "ЧТО ВЫШЛО", pl: "WYNIK" },
    Line { msg: Msg::HearthUnlit,  en: "not burning -- strike it with flint", simple: "no fire -- hit it with flint", ru: "не горит -- подожгите кремнём", pl: "nie pali się -- skrzesz ogień krzemieniem" },
    Line { msg: Msg::HearthIdle,   en: "burning, nothing to do", simple: "burning, nothing to make", ru: "горит, работы нет", pl: "pali się, nic do roboty" },
    Line { msg: Msg::HearthWorking, en: "working",    simple: "making something", ru: "работает", pl: "pracuje" },
    Line { msg: Msg::HearthAsh,    en: "ASH",         simple: "ASH",          ru: "ЗОЛА", pl: "POPIÓŁ" },
    Line { msg: Msg::HearthHeat,   en: "HEAT",        simple: "HOW HOT",      ru: "ЖАР", pl: "ŻAR" },
    // Ends in a colon because a heat colour follows it: "needs: orange".
    // Written so the colour stands on its own after it in every language,
    // rather than having to agree with a verb it is glued to.
    Line { msg: Msg::HearthTooCool, en: "not hot enough, needs:", simple: "too cold, needs:", ru: "мало жара, нужен:", pl: "za słaby żar, trzeba:" },
    Line { msg: Msg::HearthRained, en: "rain is cooling it -- roof it over", simple: "rain is putting it out", ru: "дождь студит огонь -- нужна крыша", pl: "deszcz studzi ogień -- trzeba dachu" },
    Line { msg: Msg::HearthCharring, en: "burning the food -- take it out", simple: "the food is burning -- take it", ru: "еда горит -- заберите", pl: "jedzenie się pali -- zabierz" },
    Line { msg: Msg::HearthCooling, en: "gone out, still hot", simple: "fire is out, still hot", ru: "погас, ещё горячий", pl: "zgasł, jeszcze gorący" },
    Line { msg: Msg::GlowCold,     en: "cold",        simple: "cold",         ru: "остыл", pl: "wygasły" },
    Line { msg: Msg::GlowWarm,     en: "warm",        simple: "warm",         ru: "тёплый", pl: "ciepły" },
    Line { msg: Msg::GlowHot,      en: "hot",         simple: "hot",          ru: "горячий", pl: "gorący" },
    Line { msg: Msg::GlowVeryHot,  en: "very hot",    simple: "very hot",     ru: "очень горячий", pl: "bardzo gorący" },
    Line { msg: Msg::GlowFaintRed, en: "faint red",   simple: "dull red",     ru: "слабо-красный", pl: "bladoczerwony" },
    Line { msg: Msg::GlowDarkRed,  en: "dark red",    simple: "dark red",     ru: "тёмно-красный", pl: "ciemnoczerwony" },
    Line { msg: Msg::GlowBrightRed, en: "bright red", simple: "red",          ru: "ярко-красный", pl: "jasnoczerwony" },
    Line { msg: Msg::GlowOrange,   en: "orange",      simple: "orange",       ru: "оранжевый", pl: "pomarańczowy" },
    Line { msg: Msg::GlowYellow,   en: "yellow",      simple: "yellow",       ru: "жёлтый", pl: "żółty" },
    Line { msg: Msg::GlowWhite,    en: "white",       simple: "white",        ru: "белый", pl: "biały" },

    Line { msg: Msg::DryingRack,   en: "DRYING RACK",  simple: "SKIN FRAME", ru: "СУШИЛКА", pl: "SUSZARNIA" },
    // "To dry" rather than "skins": the frame takes meat as well now,
    // and the label has to be true of everything it accepts.
    Line { msg: Msg::RackSkins,    en: "TO DRY",       simple: "PUT RAW THINGS HERE", ru: "СУШИТЬ", pl: "DO SUSZENIA" },
    // "TAKE LEATHER HERE" spelt out in full is four characters wider
    // than the panel: the tray is the rightmost thing on the screen and
    // its label starts at the tray's own left edge. See the test that
    // measures every language against the panel.
    Line { msg: Msg::RackLeather,  en: "DONE",         simple: "TAKE IT HERE", ru: "ГОТОВО", pl: "GOTOWE" },
    // What the gauge under the frame measures: not fuel, but the sky.
    Line { msg: Msg::RackWeather,  en: "WEATHER",      simple: "WEATHER",    ru: "ПОГОДА", pl: "POGODA" },
    Line { msg: Msg::RackDrying,   en: "drying",       simple: "drying",     ru: "сушится", pl: "schnie" },
    Line { msg: Msg::RackSmoking,  en: "drying by the fire", simple: "drying by the fire", ru: "сушится у огня", pl: "schnie przy ogniu" },
    // Both of these say what stopped it *and* leave the player
    // something to do about it: move it under a roof, or light a fire
    // beside it.
    Line { msg: Msg::RackWet,      en: "rain has stopped it -- put it under cover", simple: "rain stopped it -- move it inside", ru: "дождь остановил сушку -- уберите под навес", pl: "deszcz wstrzymał suszenie -- schowaj pod dach" },
    Line { msg: Msg::RackFrozen,   en: "too cold -- a fire beside it would help", simple: "too cold -- light a fire next to it", ru: "слишком холодно -- поможет костёр рядом", pl: "za zimno -- pomoże ognisko obok" },
    Line { msg: Msg::RackEmpty,    en: "nothing on the frame", simple: "nothing on it", ru: "на раме пусто", pl: "rama pusta" },
    Line { msg: Msg::RackFull,     en: "the tray is full", simple: "no room for more leather", ru: "лоток полон", pl: "taca pełna" },
    Line { msg: Msg::MinutesLeft,  en: "min left",     simple: "min to go",  ru: "мин осталось", pl: "min zostało" },
    Line { msg: Msg::SeedHelp,     en: "the seed decides the terrain -- leave it empty for a new one", simple: "this number shapes the land -- leave it empty for a surprise", ru: "сид задаёт рельеф -- оставьте пустым, и будет новый", pl: "ziarno decyduje o terenie -- puste da nowy świat" },
    Line { msg: Msg::SeedRandom,   en: "random",       simple: "any",          ru: "случайное",    pl: "losowe" },
    Line { msg: Msg::RollSeed,     en: "ROLL",         simple: "PICK",         ru: "НАУГАД",       pl: "LOSUJ" },
    Line { msg: Msg::WorldFormHelp, en: "tab switches field   enter creates   esc cancels", simple: "tab moves   enter makes it   esc goes back", ru: "tab переключает поле   ввод создаёт   esc отменяет", pl: "tab zmienia pole   enter tworzy   esc anuluje" },
    Line { msg: Msg::NeverPlayed,  en: "never played",  simple: "never played", ru: "не играли", pl: "nigdy nie grano" },
    Line { msg: Msg::JustNow,      en: "just now",      simple: "just now",  ru: "только что", pl: "przed chwilą" },
    Line { msg: Msg::MinutesAgo,   en: "min ago",       simple: "min ago",   ru: "мин назад", pl: "min temu" },
    Line { msg: Msg::HoursAgo,     en: "h ago",         simple: "h ago",     ru: "ч назад", pl: "godz. temu" },
    Line { msg: Msg::DaysAgo,      en: "d ago",         simple: "d ago",     ru: "дн. назад", pl: "dni temu" },

    Line { msg: Msg::CannotBeUndone, en: "this cannot be undone", simple: "there is no way back", ru: "это нельзя отменить", pl: "tego nie można cofnąć" },
    Line { msg: Msg::ConfirmHelp,  en: "Y confirms   N or esc cancels", simple: "Y means yes   N or esc means no", ru: "Y подтвердить   N или esc отменить", pl: "Y potwierdza   N lub esc anuluje" },

    Line { msg: Msg::Servers,      en: "SERVERS",      simple: "OTHER PEOPLE'S GAMES", ru: "СЕРВЕРЫ", pl: "SERWERY" },
    Line { msg: Msg::Connect,      en: "CONNECT",      simple: "JOIN",       ru: "ПОДКЛЮЧИТЬСЯ", pl: "POŁĄCZ" },
    Line { msg: Msg::Address,      en: "ADDRESS",      simple: "WHERE IT IS", ru: "АДРЕС", pl: "ADRES" },
    Line { msg: Msg::Add,          en: "ADD",          simple: "ADD ONE",    ru: "ДОБАВИТЬ", pl: "DODAJ" },
    Line { msg: Msg::Connecting,   en: "CONNECTING",   simple: "JOINING",    ru: "ПОДКЛЮЧЕНИЕ", pl: "ŁĄCZENIE" },
    Line { msg: Msg::NoServersYet, en: "no servers yet -- press ADD", simple: "no servers yet -- press ADD ONE", ru: "серверов пока нет -- нажмите ДОБАВИТЬ", pl: "brak serwerów -- naciśnij DODAJ" },
    Line { msg: Msg::Edit,         en: "EDIT",         simple: "CHANGE",     ru: "ИЗМЕНИТЬ", pl: "EDYTUJ" },
    Line { msg: Msg::ServersHelp,  en: "up/down select   enter play   A add   E edit   del remove", simple: "up/down choose   enter play   A add one   E change   del throw away", ru: "вверх/вниз выбрать   ввод играть   A добавить   E изменить   del удалить", pl: "góra/dół wybór   enter graj   A dodaj   E edytuj   del usuń" },
    Line { msg: Msg::EditServer,   en: "EDIT SERVER",  simple: "CHANGE A SERVER", ru: "ИЗМЕНИТЬ СЕРВЕР", pl: "EDYTUJ SERWER" },
    Line { msg: Msg::AddServer,    en: "ADD SERVER",   simple: "ADD A SERVER", ru: "НОВЫЙ СЕРВЕР", pl: "DODAJ SERWER" },
    Line { msg: Msg::AddressHelp,  en: "host:port  --  the port defaults to 7878", simple: "name:number  --  the number is 7878 if left out", ru: "хост:порт  --  порт по умолчанию 7878", pl: "host:port  --  domyślny port to 7878" },
    Line { msg: Msg::Save,         en: "SAVE",         simple: "KEEP IT",    ru: "СОХРАНИТЬ", pl: "ZAPISZ" },
    Line { msg: Msg::ServerFormHelp, en: "tab switches field   enter saves   esc cancels", simple: "tab moves   enter keeps it   esc goes back", ru: "tab переключает поле   ввод сохраняет   esc отменяет", pl: "tab zmienia pole   enter zapisuje   esc anuluje" },
    Line { msg: Msg::CannotConnect, en: "CANNOT CONNECT", simple: "CANNOT JOIN", ru: "НЕТ СОЕДИНЕНИЯ", pl: "BRAK POŁĄCZENIA" },
    Line { msg: Msg::Retry,        en: "RETRY",        simple: "TRY AGAIN",  ru: "ПОВТОРИТЬ", pl: "PONÓW" },
    Line { msg: Msg::AddressRequired, en: "an address is required", simple: "it needs an address", ru: "нужно указать адрес", pl: "adres jest wymagany" },

    Line { msg: Msg::LanguageRow,  en: "LANGUAGE",     simple: "LANGUAGE",   ru: "ЯЗЫК", pl: "JĘZYK" },
    Line { msg: Msg::RenderDistance, en: "RENDER DISTANCE", simple: "HOW FAR YOU CAN SEE", ru: "ДАЛЬНОСТЬ ПРОРИСОВКИ", pl: "ZASIĘG WIDZENIA" },
    Line { msg: Msg::FieldOfView,  en: "FIELD OF VIEW", simple: "HOW WIDE THE VIEW IS", ru: "ПОЛЕ ЗРЕНИЯ", pl: "POLE WIDZENIA" },
    Line { msg: Msg::Fog,          en: "FOG",          simple: "HAZE IN THE DISTANCE", ru: "ТУМАН", pl: "MGŁA" },
    Line { msg: Msg::MouseSensitivity, en: "MOUSE SENSITIVITY", simple: "HOW FAST THE VIEW TURNS", ru: "ЧУВСТВИТЕЛЬНОСТЬ МЫШИ", pl: "CZUŁOŚĆ MYSZY" },
    // The same row on a device with no mouse in it. A separate line
    // rather than a word swapped in, because three languages each want
    // their own phrase for it and none of them is "mouse" with a word
    // crossed out.
    Line { msg: Msg::LookSensitivity, en: "LOOK SENSITIVITY", simple: "HOW FAST THE VIEW TURNS", ru: "ЧУВСТВИТЕЛЬНОСТЬ ОБЗОРА", pl: "CZUŁOŚĆ ROZGLĄDANIA" },
    Line { msg: Msg::UiScale, en: "INTERFACE SIZE", simple: "HOW BIG THE BUTTONS ARE", ru: "РАЗМЕР ИНТЕРФЕЙСА", pl: "ROZMIAR INTERFEJSU" },
    Line { msg: Msg::Volume,       en: "MASTER VOLUME", simple: "HOW LOUD EVERYTHING IS", ru: "ОБЩАЯ ГРОМКОСТЬ", pl: "GŁOŚNOŚĆ OGÓLNA" },
    Line { msg: Msg::Music,        en: "MUSIC",        simple: "MUSIC",      ru: "МУЗЫКА", pl: "MUZYKA" },
    Line { msg: Msg::On,           en: "ON",           simple: "YES",        ru: "ВКЛ", pl: "WŁ" },
    Line { msg: Msg::Off,          en: "OFF",          simple: "NO",         ru: "ВЫКЛ", pl: "WYŁ" },
    Line { msg: Msg::Apply,        en: "APPLY",        simple: "USE THESE",  ru: "ПРИМЕНИТЬ", pl: "ZASTOSUJ" },
    Line { msg: Msg::Vsync,        en: "VSYNC",        simple: "SMOOTH FRAMES", ru: "ВЕРТ. СИНХРОНИЗАЦИЯ", pl: "SYNCHRONIZACJA PIONOWA" },
    Line { msg: Msg::Resolution,   en: "RESOLUTION",   simple: "HOW MANY PIXELS", ru: "РАЗРЕШЕНИЕ", pl: "ROZDZIELCZOŚĆ" },
    Line { msg: Msg::Auto,         en: "AUTO",         simple: "GAME DECIDES", ru: "АВТО", pl: "AUTO" },
    Line { msg: Msg::AmbientOcclusion, en: "AMBIENT OCCLUSION", simple: "SOFT CORNER SHADOWS", ru: "ЗАТЕНЕНИЕ УГЛОВ", pl: "OKLUZJA OTOCZENIA" },
    Line { msg: Msg::Anisotropy,   en: "ANISOTROPIC FILTERING", simple: "SHARPER GROUND TEXTURES", ru: "АНИЗОТРОПНАЯ ФИЛЬТРАЦИЯ", pl: "FILTROWANIE ANIZOTROPOWE" },
    Line { msg: Msg::SkyQuality,   en: "SKY QUALITY",  simple: "HOW SHARP THE SKY IS", ru: "КАЧЕСТВО НЕБА", pl: "JAKOŚĆ NIEBA" },
    Line { msg: Msg::QualityHigh,  en: "HIGH",         simple: "BEST",      ru: "ВЫСОКОЕ", pl: "WYSOKA" },
    Line { msg: Msg::QualityMedium, en: "MEDIUM",      simple: "MIDDLE",    ru: "СРЕДНЕЕ", pl: "ŚREDNIA" },
    Line { msg: Msg::QualityLow,   en: "LOW",          simple: "FASTEST",   ru: "НИЗКОЕ", pl: "NISKA" },
    Line { msg: Msg::TransparentLeaves, en: "TRANSPARENT LEAVES", simple: "SEE-THROUGH LEAVES", ru: "ПРОЗРАЧНАЯ ЛИСТВА", pl: "PRZEZROCZYSTE LIŚCIE" },
    Line { msg: Msg::Everywhere,   en: "EVERYWHERE",   simple: "EVERYWHERE", ru: "ВЕЗДЕ", pl: "WSZĘDZIE" },
    Line { msg: Msg::Shadows,     en: "REALISTIC SHADOWS",  simple: "SHADOWS FROM SUN AND FIRE", ru: "РЕАЛИСТИЧНЫЕ ТЕНИ", pl: "REALISTYCZNE CIENIE" },
    Line { msg: Msg::ShadowsHard, en: "SHARP",  simple: "SHARP EDGES", ru: "РЕЗКИЕ", pl: "OSTRE" },
    Line { msg: Msg::ShadowsSoft, en: "SOFT",   simple: "SOFT EDGES",  ru: "МЯГКИЕ", pl: "MIĘKKIE" },
    Line { msg: Msg::ShadowDistance, en: "SHADOW DISTANCE", simple: "HOW FAR SHADOWS REACH", ru: "ДАЛЬНОСТЬ ТЕНЕЙ", pl: "ZASIĘG CIENI" },
    Line { msg: Msg::Blocks,      en: "blocks", simple: "blocks",      ru: "блоков", pl: "bloków" },
    Line { msg: Msg::PlantShadows, en: "PLANT SHADOWS", simple: "SHADOWS FROM PLANTS", ru: "ТЕНИ РАСТЕНИЙ", pl: "CIENIE ROŚLIN" },
    Line { msg: Msg::PlantShadowsTrees, en: "TREES", simple: "ONLY TREES", ru: "ДЕРЕВЬЯ", pl: "DRZEWA" },
    Line { msg: Msg::PlantShadowsAll, en: "ALL", simple: "ALL PLANTS", ru: "ВСЕ", pl: "WSZYSTKIE" },
    Line { msg: Msg::LightingQuality, en: "LIGHTING QUALITY", simple: "WARM LIGHT AND SUNSETS", ru: "КАЧЕСТВО ОСВЕЩЕНИЯ", pl: "JAKOŚĆ OŚWIETLENIA" },
    Line { msg: Msg::DetailDistance, en: "GRASS & STONE DISTANCE", simple: "HOW FAR DETAILS SHOW", ru: "ДАЛЬНОСТЬ ТРАВЫ И КАМНЕЙ", pl: "ZASIĘG TRAWY I KAMIENI" },
    Line { msg: Msg::ReliefDistance, en: "3D STONES DISTANCE", simple: "HOW FAR STONES LOOK 3D", ru: "ДАЛЬНОСТЬ ОБЪЁМНЫХ КАМНЕЙ", pl: "ZASIĘG KAMIENI 3D" },
    Line { msg: Msg::LodDistance, en: "SIMPLE TERRAIN FROM", simple: "FAR LAND IS CHUNKY", ru: "УПРОЩАТЬ РЕЛЬЕФ С", pl: "UPROSZCZONY TEREN OD" },
    Line { msg: Msg::LodQuality,  en: "SIMPLIFICATION",     simple: "HOW CHUNKY",         ru: "УПРОЩЕНИЕ",       pl: "UPROSZCZENIE" },
    Line { msg: Msg::LodFine,     en: "GENTLE",             simple: "BARELY",             ru: "МЯГКОЕ",          pl: "ŁAGODNE" },
    Line { msg: Msg::LodNormal,   en: "NORMAL",             simple: "NORMAL",             ru: "ОБЫЧНОЕ",         pl: "ZWYKŁE" },
    Line { msg: Msg::LodCoarse,   en: "HEAVY",              simple: "A LOT",              ru: "СИЛЬНОЕ",         pl: "MOCNE" },
    Line { msg: Msg::Cloudiness,   en: "CLOUD COVER",  simple: "HOW CLOUDY IT IS", ru: "ОБЛАЧНОСТЬ", pl: "ZACHMURZENIE" },
    Line { msg: Msg::MenuBackground, en: "MENU BACKGROUND", simple: "PICTURE BEHIND MENUS", ru: "ФОН МЕНЮ", pl: "TŁO MENU" },
    Line { msg: Msg::MenuBackgroundScene, en: "BACKGROUND SCENE", simple: "WHAT IS BEHIND THEM", ru: "МЕСТО ФОНА", pl: "SCENA TŁA" },
    Line { msg: Msg::SceneRandom,  en: "RANDOM",       simple: "ANY",        ru: "СЛУЧАЙНОЕ", pl: "LOSOWA" },
    Line { msg: Msg::SceneShore,   en: "SHORE",        simple: "THE SEA",    ru: "БЕРЕГ", pl: "BRZEG" },
    Line { msg: Msg::SceneForest,  en: "FOREST",       simple: "TREES",      ru: "ЛЕС", pl: "LAS" },
    Line { msg: Msg::ScenePlains,  en: "PLAINS",       simple: "OPEN FIELD", ru: "РАВНИНА", pl: "RÓWNINA" },
    Line { msg: Msg::SceneCave,    en: "CAVE",         simple: "UNDERGROUND", ru: "ПЕЩЕРА", pl: "JASKINIA" },
    Line { msg: Msg::Chunks,       en: "chunks",       simple: "chunks",     ru: "чанков", pl: "chunków" },
    // The sign, not the word. "deg" was a stub of a word, Polish's
    // "stopni" was wrong after 1, 2, 3 and 4, and Russian's "град" is
    // also the Russian for hail. `°` is none of those things in any
    // language and is a glyph the font carries (`font::ORDER`).
    Line { msg: Msg::Degrees,      en: "°",            simple: "°",          ru: "°", pl: "°" },
    Line { msg: Msg::Toggle,       en: "TOGGLE",       simple: "ON/OFF",     ru: "ВКЛ/ВЫКЛ", pl: "PRZEŁĄCZ" },
    Line { msg: Msg::Controls,     en: "CONTROLS",     simple: "KEYS",       ru: "УПРАВЛЕНИЕ", pl: "STEROWANIE" },
    Line { msg: Msg::ArrangeControls, en: "BUTTONS",  simple: "BUTTONS",    ru: "КНОПКИ",     pl: "PRZYCISKI" },
    Line { msg: Msg::ResetControls, en: "RESET",      simple: "RESET",      ru: "СБРОС",      pl: "RESET" },
    Line { msg: Msg::DragToArrange, en: "drag a control where your thumb wants it", simple: "drag a button where you want it", ru: "перетащите кнопку туда, где её ждёт большой палец", pl: "przeciągnij przycisk tam, gdzie chce go kciuk" },
    Line { msg: Msg::Done,         en: "DONE",         simple: "DONE",       ru: "ГОТОВО", pl: "GOTOWE" },
    Line { msg: Msg::SettingsHelp, en: "changes apply at once and are saved when you leave", simple: "changes happen right away and are kept when you leave", ru: "изменения применяются сразу и сохраняются при выходе", pl: "zmiany działają od razu i zapisują się przy wyjściu" },

    Line { msg: Msg::PressAKey,    en: "PRESS A KEY",  simple: "PRESS A KEY", ru: "НАЖМИТЕ...", pl: "NACIŚNIJ..." },
    Line { msg: Msg::ResetToDefaults, en: "RESET TO DEFAULTS", simple: "PUT KEYS BACK", ru: "СБРОСИТЬ КЛАВИШИ", pl: "PRZYWRÓĆ DOMYŚLNE" },
    Line { msg: Msg::ControlsHelp, en: "taking a key from another action leaves that one unbound", simple: "giving a key away leaves its old action with none", ru: "клавиша, занятая другим действием, снимается с него", pl: "klawisz zabrany innej akcji zostawia ją bez klawisza" },
    Line { msg: Msg::KeyBound,     en: "key bound",    simple: "key set",    ru: "клавиша назначена", pl: "klawisz przypisany" },
    Line { msg: Msg::KeyCannotBind, en: "that key cannot be bound", simple: "that key cannot be used", ru: "эту клавишу нельзя назначить", pl: "tego klawisza nie da się przypisać" },
    Line { msg: Msg::WalkForward,  en: "WALK FORWARD", simple: "GO FORWARD", ru: "ИДТИ ВПЕРЁД", pl: "IDŹ NAPRZÓD" },
    Line { msg: Msg::WalkBack,     en: "WALK BACK",    simple: "GO BACK",    ru: "ИДТИ НАЗАД", pl: "IDŹ W TYŁ" },
    Line { msg: Msg::StrafeLeft,   en: "STRAFE LEFT",  simple: "STEP LEFT",  ru: "ШАГ ВЛЕВО", pl: "KROK W LEWO" },
    Line { msg: Msg::StrafeRight,  en: "STRAFE RIGHT", simple: "STEP RIGHT", ru: "ШАГ ВПРАВО", pl: "KROK W PRAWO" },
    Line { msg: Msg::Jump,         en: "JUMP",         simple: "JUMP",       ru: "ПРЫЖОК", pl: "SKOK" },
    Line { msg: Msg::Sprint,       en: "SPRINT",       simple: "RUN",        ru: "БЕГ", pl: "SPRINT" },
    Line { msg: Msg::Rein,         en: "WALK / GET OFF", simple: "HORSE SLOW / GET DOWN", ru: "ШАГОМ / СПЕШИТЬСЯ", pl: "STĘPA / ZSIĄDŹ" },
    Line { msg: Msg::DropItem,     en: "DROP ITEM",    simple: "THROW OUT",  ru: "ВЫБРОСИТЬ", pl: "WYRZUĆ" },
    Line { msg: Msg::ToggleFog,    en: "TOGGLE FOG",   simple: "HAZE ON/OFF", ru: "ТУМАН ВКЛ/ВЫКЛ", pl: "MGŁA WŁ/WYŁ" },
    Line { msg: Msg::ToggleStats,  en: "TOGGLE STATS", simple: "NUMBERS ON/OFF", ru: "СТАТИСТИКА", pl: "STATYSTYKI" },
    Line { msg: Msg::ToggleHud,    en: "HIDE INTERFACE", simple: "HIDE THE SCREEN ITEMS", ru: "СКРЫТЬ ИНТЕРФЕЙС", pl: "UKRYJ INTERFEJS" },
    Line { msg: Msg::Fullscreen,   en: "FULLSCREEN",   simple: "WHOLE SCREEN", ru: "ПОЛНЫЙ ЭКРАН", pl: "PEŁNY EKRAN" },

    Line { msg: Msg::RoleTextures, en: "TEXTURES",     simple: "PICTURES",   ru: "ТЕКСТУРЫ", pl: "TEKSTURY" },
    Line { msg: Msg::RoleCode,     en: "CODE",         simple: "CODE",       ru: "КОД", pl: "KOD" },
    Line { msg: Msg::RoleEngine,   en: "ENGINE",       simple: "BUILT WITH", ru: "ДВИЖОК", pl: "SILNIK" },

    // **Not "РЮКЗАК"**, which it was until the pack screen grew a third tab
    // called exactly that (`Msg::TabBackpack`): the heading over the page
    // of your own things and the tab for the bag on your back read the
    // same word, and the tab looked like the page it was already on.
    Line { msg: Msg::Inventory,    en: "INVENTORY",    simple: "WHAT YOU CARRY", ru: "ИНВЕНТАРЬ", pl: "EKWIPUNEK" },
    Line { msg: Msg::Crafting,     en: "CRAFTING",     simple: "MAKING THINGS", ru: "СОЗДАНИЕ", pl: "WYTWARZANIE" },
    // Short in every language: the column is one slot wide. **Not `ТЕЛО`**,
    // which it was: `ТЕЛО` is what the game calls a dead player's body
    // (`Msg::Corpse`) and what the body tab is called (`Msg::TabHealth`),
    // so one word stood over three unrelated things. `НА СЕБЕ` is what the
    // English and the Polish already said.
    Line { msg: Msg::Worn,         en: "WORN",         simple: "ON YOU",     ru: "НА СЕБЕ", pl: "NA SOBIE" },
    Line { msg: Msg::Paused,       en: "PAUSED",       simple: "STOPPED",    ru: "ПАУЗА", pl: "PAUZA" },
    Line { msg: Msg::Resume,       en: "RESUME",       simple: "CARRY ON",   ru: "ПРОДОЛЖИТЬ", pl: "WRÓĆ DO GRY" },
    Line { msg: Msg::Respawn,      en: "RESPAWN",      simple: "START AGAIN", ru: "ВОЗРОДИТЬСЯ", pl: "ODRODŹ SIĘ" },
    Line { msg: Msg::YouDied,      en: "YOU DIED",     simple: "YOU DIED",   ru: "ВЫ ПОГИБЛИ", pl: "ZGINĄŁEŚ" },
    Line { msg: Msg::LeaveWorld,   en: "LEAVE WORLD",  simple: "LEAVE THE WORLD", ru: "ПОКИНУТЬ МИР", pl: "OPUŚĆ ŚWIAT" },
    Line { msg: Msg::QuitToMenu,   en: "QUIT TO MENU", simple: "BACK TO MENU", ru: "ВЫЙТИ В МЕНЮ", pl: "WYJDŹ DO MENU" },
    Line { msg: Msg::DeathHelp,    en: "R RESPAWN    ESC MENU", simple: "R START AGAIN    ESC MENU", ru: "R ВОЗРОДИТЬСЯ    ESC МЕНЮ", pl: "R ODRODŹ SIĘ    ESC MENU" },

    // No `BELT`. The pack screen dropped the word first -- "the strip is
    // the whole label", see `inventory_screen::build_into` -- and the
    // chest screen kept it, which left the one screen with three region
    // captions writing two of them *above* their rows and the third
    // *below* its own. Whichever way round is right, both is wrong.
    Line { msg: Msg::TidyPile,     en: "TIDY PILE",    simple: "TIDY UP",    ru: "ПРИБРАТЬ", pl: "SPRZĄTNIJ" },
    Line { msg: Msg::NoRoom,       en: "no room",      simple: "no room",    ru: "нет места", pl: "brak miejsca" },
    Line { msg: Msg::NeedFire,     en: "needs a fire", simple: "needs a fire", ru: "нужен огонь", pl: "potrzebny ogień" },
    Line { msg: Msg::NeedKiln,     en: "needs a kiln", simple: "needs a kiln", ru: "нужен горн", pl: "potrzebny piec" },
    Line { msg: Msg::NeedBloomery, en: "needs a bloomery", simple: "needs a bloomery", ru: "нужна домница", pl: "potrzebna dymarka" },
    Line { msg: Msg::NeedBench, en: "needs a workbench", simple: "needs a work table", ru: "нужен верстак", pl: "potrzebny warsztat" },
    Line { msg: Msg::NeedMason, en: "needs a mason's block", simple: "needs a stone work block", ru: "нужна колода каменотёса", pl: "potrzebny kamieniarski blok" },
    Line { msg: Msg::NeedWheel, en: "needs a potter's wheel", simple: "needs a clay wheel", ru: "нужен гончарный круг", pl: "potrzebne koło garncarskie" },
    Line { msg: Msg::NeedLeatherBench, en: "needs a leather bench", simple: "needs a leather table", ru: "нужен кожевенный стол", pl: "potrzebny stół rymarski" },
    Line { msg: Msg::Nourishment,  en: "FED",          simple: "FOOD",       ru: "СЫТОСТЬ", pl: "SYTOŚĆ" },
    Line { msg: Msg::WeatherLabel, en: "SKY",          simple: "SKY",        ru: "НЕБО", pl: "NIEBO" },
    Line { msg: Msg::WeatherClear, en: "clear",        simple: "clear",      ru: "ясно", pl: "czysto" },
    Line { msg: Msg::WeatherRain,  en: "rain",         simple: "rain",       ru: "дождь", pl: "deszcz" },
    Line { msg: Msg::WeatherStorm, en: "storm",        simple: "storm",      ru: "гроза", pl: "burza" },
    Line { msg: Msg::Eat,          en: "EAT",          simple: "EAT",        ru: "СЪЕСТЬ", pl: "ZJEDZ" },
    Line { msg: Msg::UseBlock,     en: "USE",          simple: "USE",        ru: "ИСПОЛЬЗОВАТЬ", pl: "UŻYJ" },
    Line { msg: Msg::Need,         en: "need",         simple: "need",       ru: "нужно", pl: "potrzeba" },
    Line { msg: Msg::No,           en: "no",           simple: "no",         ru: "нет", pl: "nie" },
    Line { msg: Msg::Of,           en: "OF",           simple: "OF",         ru: "ИЗ", pl: "Z" },
    Line { msg: Msg::KgCarried,    en: "kg carried",   simple: "kg carried", ru: "кг при себе", pl: "kg przy sobie" },
    // The unit on its own. It was written into the format strings as a
    // literal "kg", so a Russian tooltip said "1 kg" one line above a
    // footer that said "229 кг при себе" -- two spellings of one unit on
    // one screen.
    Line { msg: Msg::Kg,           en: "kg",           simple: "kg",         ru: "кг", pl: "kg" },
    // The thumb controls. Short, because a button is a square a thumb
    // wide and `scale_to_fit` shrinks a long word until it is small: the
    // Russian pack button is `ВЕЩИ`, not the screen's own `ИНВЕНТАРЬ`,
    // for that reason and no other.
    Line { msg: Msg::ThumbMine,    en: "MINE",         simple: "DIG",        ru: "КОПАТЬ", pl: "KOP" },
    Line { msg: Msg::ThumbPlace,   en: "PLACE",        simple: "PUT",        ru: "СТАВИТЬ", pl: "POŁÓŻ" },
    Line { msg: Msg::ThumbMenu,    en: "MENU",         simple: "MENU",       ru: "МЕНЮ", pl: "MENU" },
    Line { msg: Msg::ThumbChat,    en: "CHAT",         simple: "TALK",       ru: "ЧАТ", pl: "CZAT" },
    Line { msg: Msg::ThumbJump,    en: "JUMP",         simple: "JUMP",       ru: "ПРЫЖОК", pl: "SKOK" },
    Line { msg: Msg::ThumbPack,    en: "PACK",         simple: "BAG",        ru: "ВЕЩИ", pl: "TORBA" },
    Line { msg: Msg::ThumbMap,     en: "MAP",          simple: "MAP",        ru: "КАРТА", pl: "MAPA" },
    Line { msg: Msg::Speed,        en: "speed",        simple: "speed",      ru: "скорость", pl: "szybkość" },

    Line { msg: Msg::Chest,        en: "CHEST",        simple: "STORAGE BOX", ru: "СУНДУК", pl: "SKRZYNIA" },
    // Not "РЮКЗАК" in Russian and not "PLECAK" in Polish: both are
    // already the word this game uses for the pack you are carrying (see
    // Msg::Inventory and Msg::Pack), and a heading that says the same
    // thing as the grid under it says nothing. **And not "ПОЖИТКИ"**,
    // which it was until somebody noticed that is `Msg::TabPack` -- the
    // tab for your own things -- so a player opening a dead stranger's
    // pack was told they were looking at their own.
    Line { msg: Msg::Backpack,     en: "BACKPACK",     simple: "WHAT THEY LEFT", ru: "КОТОМКА", pl: "SAKWA" },
    // The body and the bones. Not "ТРУП" in Russian: the word a player
    // needs here is the one for *their own* body, and the heading is read
    // by the person it belonged to.
    Line { msg: Msg::Corpse,       en: "YOUR BODY",    simple: "WHAT IS LEFT OF YOU", ru: "ТЕЛО", pl: "CIAŁO" },
    Line { msg: Msg::Remains,      en: "REMAINS",      simple: "BONES",      ru: "КОСТИ", pl: "KOŚCI" },
    Line { msg: Msg::Jug,          en: "JUG",          simple: "CLAY JUG",   ru: "КУВШИН", pl: "DZBAN" },
    Line { msg: Msg::Saddlebags,   en: "SADDLEBAGS",   simple: "HORSE BAGS", ru: "ПЕРЕМЁТНЫЕ СУМЫ", pl: "JUKI" },
    Line { msg: Msg::VesselEmpty,  en: "grain, seeds, sand, flakes -- loose and dry", simple: "only loose dry things go in", ru: "зерно, семена, песок, отщепы -- сыпучее", pl: "ziarno, nasiona, piasek, odłupki -- sypkie" },
    Line { msg: Msg::VesselFull,   en: "full to the neck", simple: "it is full", ru: "полон до горла", pl: "pełny po szyjkę" },
    // Head, chest, legs, feet -- the whole word now, because it is read
    // in a note beside the pointer rather than printed inside a square
    // one slot wide. `ТОРС` rather than `ТЕЛО` for the chest: `ТЕЛО` is
    // the body tab (`Msg::TabHealth`) and a dead player's own body
    // (`Msg::Corpse`), and a page and one of its four squares called the
    // same thing is a screen that answers "which of these is it" with
    // "both".
    Line { msg: Msg::SlotHead,     en: "HEAD",         simple: "HEAD",       ru: "ГОЛОВА", pl: "GŁOWA" },
    Line { msg: Msg::SlotChest,    en: "CHEST",        simple: "BODY",       ru: "ТОРС", pl: "TUŁÓW" },
    Line { msg: Msg::SlotLegs,     en: "LEGS",         simple: "LEGS",       ru: "НОГИ", pl: "NOGI" },
    Line { msg: Msg::SlotFeet,     en: "FEET",         simple: "FEET",       ru: "СТУПНИ", pl: "STOPY" },
    Line { msg: Msg::SlotBack,     en: "BACK",         simple: "YOUR BACK",  ru: "СПИНА", pl: "PLECY" },
    // The three tabs across the top of the pack screen.
    Line { msg: Msg::TabHealth,    en: "BODY",         simple: "HOW YOU ARE", ru: "ТЕЛО", pl: "CIAŁO" },
    Line { msg: Msg::TabPack,      en: "PACK",         simple: "WHAT YOU CARRY", ru: "ПОЖИТКИ", pl: "TORBA" },
    Line { msg: Msg::TabBackpack,  en: "RUCKSACK",     simple: "BAG ON YOUR BACK", ru: "РЮКЗАК", pl: "PLECAK" },
    // ...and the health page under the first of them. Sentence case, not
    // capitals: this is a page of readings to be read down, and a column
    // of shouted words is harder to read than a column of words.
    Line { msg: Msg::VitalHealth,  en: "health",       simple: "how hurt",   ru: "здоровье", pl: "zdrowie" },
    Line { msg: Msg::VitalHunger,  en: "food",         simple: "how full",   ru: "еда", pl: "jedzenie" },
    // **The same gauge is called the same thing here as it is beside
    // the strip it belongs to** (`Msg::Gauge*`, printed on the pause
    // screen's key). It was not, and the stamina row is why this rule is
    // written down: the key called it STAMINA / СИЛЫ / WYTRZYMAŁOŚĆ and
    // this page called it "breath / дыхание / oddech" -- which is the
    // word the key uses for the *air* gauge. A player reading "дыхание
    // 50%" here and looking for it on the HUD would find the bubble.
    // Water and food were a smaller version of the same drift, in
    // Russian and Polish only.
    //
    // Tiredness is deliberately still the odd one out: the key names the
    // strip REST because the strip fills as a player rests, and this row
    // is the same number counted the other way up. Two names for two
    // readings, which the percentage beside each of them settles.
    Line { msg: Msg::VitalThirst,  en: "water",        simple: "how thirsty", ru: "вода", pl: "woda" },
    Line { msg: Msg::VitalStamina, en: "stamina",      simple: "running",    ru: "силы", pl: "wytrzymałość" },
    Line { msg: Msg::VitalTiredness, en: "tiredness",  simple: "how tired",  ru: "усталость", pl: "zmęczenie" },
    Line { msg: Msg::VitalWarmth,  en: "warmth",       simple: "how warm",   ru: "тепло", pl: "ciepło" },
    Line { msg: Msg::VitalWetness, en: "wetness",      simple: "how wet",    ru: "сырость", pl: "przemoczenie" },
    Line { msg: Msg::VitalDirt,    en: "dirt",         simple: "how dirty",  ru: "грязь", pl: "brud" },
    Line { msg: Msg::VitalDiet,    en: "diet",         simple: "kinds eaten", ru: "разнообразие еды", pl: "różnorodność jedzenia" },
    Line { msg: Msg::VitalRecovery, en: "recovery",    simple: "getting breath back", ru: "восстановление", pl: "regeneracja" },
    Line { msg: Msg::VitalInjuries, en: "INJURIES",    simple: "WHAT HURTS", ru: "ПОВРЕЖДЕНИЯ", pl: "OBRAŻENIA" },
    Line { msg: Msg::NoBackpack,   en: "no rucksack on your back", simple: "you have no bag on your back", ru: "на спине нет рюкзака", pl: "nie masz plecaka na plecach" },
    // **Not "В СУНДУКЕ"**: the same caption heads a body, a horse's bags and
    // a stall's counter, and a dead player's own things captioned "in the
    // chest" is a label that is wrong three screens out of four.
    Line { msg: Msg::Stored,       en: "STORED",       simple: "INSIDE",     ru: "ВНУТРИ", pl: "W ŚRODKU" },
    Line { msg: Msg::Carried,      en: "CARRIED",      simple: "ON YOU",     ru: "ПРИ СЕБЕ", pl: "PRZY SOBIE" },
    Line { msg: Msg::StoreAll,     en: "STORE ALL ^",  simple: "PUT IN ^",   ru: "СЛОЖИТЬ ^", pl: "SCHOWAJ ^" },
    Line { msg: Msg::TakeAll,      en: "v TAKE ALL",   simple: "v TAKE OUT", ru: "v ЗАБРАТЬ", pl: "v ZABIERZ" },
    Line { msg: Msg::SlotsWord,    en: "slots",        simple: "spaces",     ru: "ячеек", pl: "pól" },
    Line { msg: Msg::ItemsWord,    en: "items",        simple: "things",     ru: "предметов", pl: "przedmiotów" },
    Line { msg: Msg::WorldFormHelpTouch, en: "tap a field to fill it   CREATE when you are done", simple: "tap a box   then CREATE", ru: "коснитесь поля   потом СОЗДАТЬ", pl: "dotknij pola   potem UTWÓRZ" },
    Line { msg: Msg::ConfirmHelpTouch, en: "tap YES or NO", simple: "tap YES or NO", ru: "коснитесь ДА или НЕТ", pl: "dotknij TAK lub NIE" },
    Line { msg: Msg::ServerFormHelpTouch, en: "tap a field to fill it   SAVE when you are done", simple: "tap a box   then SAVE", ru: "коснитесь поля   потом СОХРАНИТЬ", pl: "dotknij pola   potem ZAPISZ" },
    Line { msg: Msg::DeathHelpTouch, en: "TAP RESPAWN", simple: "TAP START AGAIN", ru: "КОСНИТЕСЬ, ЧТОБЫ ВОЗРОДИТЬСЯ", pl: "DOTKNIJ, BY SIĘ ODRODZIĆ" },
    Line { msg: Msg::ChestHint2Touch, en: "hold to send it across  |  tap outside to close", simple: "hold to send it over  |  tap outside to close", ru: "удержание шлёт на другую сторону  |  касание вне закрывает", pl: "przytrzymaj by przesłać  |  dotknij poza by zamknąć" },
    Line { msg: Msg::ChestHint2,   en: "shift click sends it across  |  esc closes", simple: "shift click sends it over  |  esc closes", ru: "shift+клик шлёт на другую сторону  |  esc выход", pl: "shift+klik śle na drugą stronę  |  esc zamyka" },
    // The rest of the keyboard-only lines, in the version a phone can
    // act on. See `by_input`: a hint naming a key the device has not got
    // is worse than no hint, because it reads as a feature that is
    // missing rather than as one that works differently.
    Line { msg: Msg::WorldsHelpTouch, en: "tap a world to choose it   tap again to play", simple: "tap a world   tap it again to play", ru: "коснитесь мира   ещё раз чтобы играть", pl: "dotknij świata   znowu by grać" },
    Line { msg: Msg::ServersHelpTouch, en: "tap a server to choose it   tap again to join", simple: "tap a server   tap it again to join", ru: "коснитесь сервера   ещё раз чтобы войти", pl: "dotknij serwera   znowu by dołączyć" },
    Line { msg: Msg::NoServersYetTouch, en: "no servers yet -- tap ADD", simple: "no servers yet -- tap ADD ONE", ru: "серверов пока нет -- коснитесь ДОБАВИТЬ", pl: "brak serwerów -- dotknij DODAJ" },
    Line { msg: Msg::NoWorldsYetTouch, en: "no worlds yet -- tap NEW", simple: "no worlds yet -- tap MAKE ONE", ru: "миров пока нет -- коснитесь СОЗДАТЬ", pl: "brak światów -- dotknij NOWY" },

    Line { msg: Msg::Extensions,   en: "EXTENSIONS",   simple: "ADDED THINGS", ru: "РАСШИРЕНИЯ", pl: "ROZSZERZENIA" },
    Line { msg: Msg::ExtensionsAsking, en: "asking the server...", simple: "asking the server...", ru: "спрашиваем сервер...", pl: "pytamy serwer..." },
    Line { msg: Msg::ExtensionsNone, en: "this server is running none", simple: "nothing has been added here", ru: "здесь ничего не запущено", pl: "nic tu nie działa" },
    Line { msg: Msg::ExtensionsNoLoader, en: "this world runs inside the game, which loads none", simple: "this world runs inside the game, which adds none", ru: "мир идёт внутри самой игры, а она их не грузит", pl: "świat działa w samej grze, która ich nie ładuje" },
    Line { msg: Msg::ExtensionsNoLoaderPhone, en: "the phone build loads no mods -- a mod is a library, and an app package has no folder to drop one into", simple: "no mods on a phone -- a mod is a file you put in a folder, and a phone has no folder for it", ru: "сборка для телефона моды не грузит -- мод это библиотека, а в пакете приложения её некуда положить", pl: "wersja na telefon nie ładuje modów -- mod to biblioteka, a w pakiecie aplikacji nie ma gdzie jej położyć" },
    Line { msg: Msg::ExtensionScript, en: "PLUGIN", simple: "SCRIPT", ru: "ПЛАГИН", pl: "WTYCZKA" },
    Line { msg: Msg::ExtensionNative, en: "MOD",   simple: "MOD",    ru: "МОД", pl: "MOD" },
    Line { msg: Msg::ExtensionBy,  en: "BY",           simple: "MADE BY",     ru: "АВТОРЫ", pl: "AUTORZY" },
    Line { msg: Msg::ExtensionSettings, en: "SETTINGS", simple: "SETTINGS",  ru: "НАСТРОЙКИ", pl: "USTAWIENIA" },
    Line { msg: Msg::ExtensionNoSettings, en: "declares no settings", simple: "has nothing to set", ru: "настроек не объявлено", pl: "brak ustawień" },
    Line { msg: Msg::ExtensionOff, en: "OFF",          simple: "NOT RUNNING", ru: "ВЫКЛ", pl: "WYŁ" },
    Line { msg: Msg::ExtensionOn,  en: "ON",           simple: "RUNNING",    ru: "ВКЛ", pl: "WŁ" },
    Line { msg: Msg::ExtensionApi, en: "API",          simple: "VERSION IT NEEDS", ru: "API", pl: "API" },
    Line { msg: Msg::ExtensionsReadOnly, en: "shown only -- settings live in the server's own files", simple: "you can only look -- the server keeps these", ru: "только просмотр -- настройки лежат в файлах сервера", pl: "tylko podgląd -- ustawienia są w plikach serwera" },
    Line { msg: Msg::ExtensionsHelp, en: "up/down select   esc back", simple: "up/down choose   esc go back", ru: "вверх/вниз выбрать   esc назад", pl: "góra/dół wybór   esc wstecz" },
    Line { msg: Msg::ExtensionsHelpTouch, en: "tap one to read what it is", simple: "tap one to read what it is", ru: "коснитесь, чтобы прочитать", pl: "dotknij, by przeczytać" },

    Line { msg: Msg::ChatUnread,   en: "NEW",          simple: "NOT READ",     ru: "НОВЫХ", pl: "NOWE" },

    Line { msg: Msg::WorldNamePlaceholder, en: "a name for this world", simple: "what to call it", ru: "название мира", pl: "nazwa świata" },
    Line { msg: Msg::ServerNamePlaceholder, en: "what to call this server", simple: "what to call it", ru: "как назвать сервер", pl: "jak nazwać serwer" },
    Line { msg: Msg::AddressPlaceholder, en: "host or host:port", simple: "where the server is", ru: "адрес или адрес:порт", pl: "adres lub adres:port" },
    Line { msg: Msg::UsernamePlaceholder, en: "the name others will see", simple: "the name others will see", ru: "имя, которое видят другие", pl: "imię, które widzą inni" },

    Line { msg: Msg::GaugesTitle,  en: "THE BARS",     simple: "WHAT THE BARS MEAN", ru: "ПОЛОСЫ", pl: "PASKI" },
    Line { msg: Msg::GaugeWarmth,  en: "WARMTH",       simple: "HOW WARM YOU ARE", ru: "ТЕПЛО", pl: "CIEPŁO" },
    Line { msg: Msg::GaugeAir,     en: "AIR",          simple: "AIR UNDER WATER", ru: "ВОЗДУХ", pl: "POWIETRZE" },
    Line { msg: Msg::GaugeWater,   en: "WATER",        simple: "HOW THIRSTY", ru: "ВОДА", pl: "WODA" },
    Line { msg: Msg::GaugeRest,    en: "REST",         simple: "HOW TIRED",  ru: "ОТДЫХ", pl: "ODPOCZYNEK" },
    Line { msg: Msg::GaugeHealth,  en: "HEALTH",       simple: "HOW HURT",   ru: "ЗДОРОВЬЕ", pl: "ZDROWIE" },
    Line { msg: Msg::GaugeStamina, en: "STAMINA",      simple: "RUNNING",    ru: "СИЛЫ", pl: "WYTRZYMAŁOŚĆ" },
    Line { msg: Msg::GaugeFood,    en: "FOOD",         simple: "HOW HUNGRY", ru: "ЕДА", pl: "JEDZENIE" },

    Line { msg: Msg::HeatRising,   en: "too hot -- find shade, water or lighter clothes", simple: "you are too hot -- get into the shade or into water", ru: "жарко -- ищите тень, воду или одежду полегче", pl: "za gorąco -- szukaj cienia, wody albo lżejszego ubrania" },
    Line { msg: Msg::HeatStroke,   en: "heatstroke -- get cool now", simple: "the heat is hurting you -- shade or water, now", ru: "перегрев -- скорее в тень или в воду", pl: "udar cieplny -- szybko do cienia albo do wody" },
    Line { msg: Msg::HeatEased,    en: "cooler now",   simple: "you have cooled down", ru: "жара отпустила", pl: "już chłodniej" },

    Line { msg: Msg::SleepGetUp,   en: "walk or jump to get up", simple: "walk or jump to get out of bed", ru: "идите или прыгните, чтобы встать", pl: "idź albo skocz, aby wstać" },
    Line { msg: Msg::SleepWaiting, en: "the night passes once everyone is asleep", simple: "morning comes when everybody is asleep", ru: "ночь пройдёт, когда уснут все", pl: "noc minie, gdy wszyscy zasną" },
    Line { msg: Msg::SleepMorning, en: "morning -- walk or jump to get up", simple: "it is morning -- walk or jump to get out of bed", ru: "утро -- идите или прыгните, чтобы встать", pl: "rano -- idź albo skocz, aby wstać" },

    Line { msg: Msg::DownedTitle,     en: "YOU ARE DOWN", simple: "YOU CANNOT STAND UP", ru: "ВЫ ПРИ СМЕРТИ", pl: "OSTATKIEM SIŁ" },
    Line { msg: Msg::DownedWound,     en: "a wound has put you on the ground", simple: "you are badly hurt", ru: "рана свалила вас с ног", pl: "rana powaliła cię na ziemię" },
    Line { msg: Msg::DownedBleeding,  en: "you have lost too much blood to stand", simple: "you lost too much blood", ru: "вы потеряли слишком много крови", pl: "za mało krwi, by ustać na nogach" },
    Line { msg: Msg::DownedFall,      en: "the fall has broken your legs", simple: "the fall broke your legs", ru: "падение переломало вам ноги", pl: "upadek połamał ci nogi" },
    Line { msg: Msg::DownedBurn,      en: "the fire has burned you to the ground", simple: "the fire burned you badly", ru: "огонь сжёг вас до земли", pl: "ogień powalił cię na ziemię" },
    Line { msg: Msg::DownedCold,      en: "the cold has taken your legs", simple: "you are too cold to stand", ru: "холод отнял у вас ноги", pl: "zimno odebrało ci nogi" },
    Line { msg: Msg::DownedHeat,      en: "the heat has struck you down", simple: "you are too hot to stand", ru: "жара свалила вас с ног", pl: "upał powalił cię na ziemię" },
    Line { msg: Msg::DownedHunger,    en: "hunger has taken your strength", simple: "you are too hungry to stand", ru: "голод отнял у вас силы", pl: "głód odebrał ci siły" },
    Line { msg: Msg::DownedThirst,    en: "thirst has taken your strength", simple: "you are too thirsty to stand", ru: "жажда отняла у вас силы", pl: "pragnienie odebrało ci siły" },
    Line { msg: Msg::DownedSickness,  en: "the sickness has emptied you", simple: "you are too sick to stand", ru: "болезнь опустошила вас", pl: "choroba cię wyczerpała" },
    Line { msg: Msg::DownedSmoke,     en: "the smoke has taken your breath", simple: "the smoke took your breath", ru: "дым отнял у вас дыхание", pl: "dym odebrał ci oddech" },
    Line { msg: Msg::SavedByDressing, en: "a bandage or a poultice will get you up", simple: "put on a bandage to get up", ru: "встать поможет повязка или припарка", pl: "wstaniesz z bandażem albo okładem" },
    Line { msg: Msg::SavedBySplint,   en: "a splint will get you up -- or crawl home to one", simple: "put on a splint to get up", ru: "встать поможет шина -- или доползите до неё", pl: "wstaniesz z szyną -- albo doczołgaj się do niej" },
    Line { msg: Msg::SavedByWarmth,   en: "crawl to a fire -- warmth will get you up", simple: "get warm by a fire to get up", ru: "ползите к огню -- тепло поднимет вас", pl: "czołgaj się do ognia -- ciepło cię podniesie" },
    Line { msg: Msg::SavedByCooling,  en: "crawl into shade or water and cool down", simple: "get into shade or water to get up", ru: "ползите в тень или в воду, чтобы остыть", pl: "czołgaj się w cień albo do wody, aby ochłonąć" },
    Line { msg: Msg::SavedByFood,     en: "eat anything to get up", simple: "eat something to get up", ru: "съешьте что-нибудь, чтобы встать", pl: "zjedz cokolwiek, aby wstać" },
    Line { msg: Msg::SavedByWater,    en: "drink to get up", simple: "drink some water to get up", ru: "попейте, чтобы встать", pl: "napij się, aby wstać" },
    Line { msg: Msg::SavedByAir,      en: "crawl out into clean air", simple: "get out of the smoke to get up", ru: "выползите на чистый воздух", pl: "wyczołgaj się na czyste powietrze" },
    Line { msg: Msg::DownedGiveUp,    en: "R GIVE UP", simple: "R GIVE UP", ru: "R СДАТЬСЯ", pl: "R PODDAJ SIĘ" },
    Line { msg: Msg::DownedGiveUpTouch, en: "somebody can still help you up", simple: "a friend can still help you up", ru: "вас ещё могут поднять", pl: "ktoś jeszcze może cię podnieść" },

    Line { msg: Msg::FishingTooSmall,   en: "no fish live in water this small", simple: "this water is too small for fish", ru: "в такой маленькой воде рыба не живёт", pl: "w tak małej wodzie nie ma ryb" },
    Line { msg: Msg::FishingTooShallow, en: "too shallow: the float would lie on the bottom", simple: "too shallow to fish here -- find deeper water", ru: "слишком мелко: поплавок ляжет на дно", pl: "za płytko: spławik leżałby na dnie" },
    Line { msg: Msg::SetDownWhere,      en: "things are set down on top of solid ground, in an empty place", simple: "put it on top of firm ground, where nothing else is", ru: "класть можно только сверху на твёрдую опору, в пустое место", pl: "kłaść można tylko na twarde podłoże, w puste miejsce" },
    Line { msg: Msg::LarderRefusesSkins, en: "this rack is for meat and fish: a hide is stretched on a hide frame", simple: "only meat and fish go here -- put the skin on a hide frame", ru: "эта сушилка для мяса и рыбы: шкуру натягивают на раму", pl: "ta suszarnia jest na mięso i ryby: skórę naciąga się na ramę" },
    Line { msg: Msg::FrameRefusesFood,   en: "a hide frame takes skins: meat and fish hang on the drying rack", simple: "only skins go on this frame -- hang food on the big drying rack", ru: "рама только для шкур: мясо и рыбу вешают на сушилку", pl: "rama jest tylko na skóry: mięso i ryby wiesza się na suszarni" },
    Line { msg: Msg::FishTrapEmpty,     en: "nothing has gone into the trap yet", simple: "no fish in the trap yet -- come back later", ru: "в вершу ещё ничего не зашло", pl: "do więcierza jeszcze nic nie wpłynęło" },
    Line { msg: Msg::FishTrapDry,       en: "the trap is not in the water: set it with water on two sides", simple: "put the trap in water, with water on two sides", ru: "верша не в воде: поставьте её так, чтобы вода была с двух сторон", pl: "więcierz nie stoi w wodzie: postaw go tak, by woda była z dwóch stron" },
    Line { msg: Msg::FishingNoWater,    en: "the line came down short of the water", simple: "the line did not reach the water -- throw harder", ru: "леска не долетела до воды", pl: "żyłka nie doleciała do wody" },
    Line { msg: Msg::FishingStruckAtNothing, en: "you struck at nothing: the float came back", simple: "nothing was biting -- the float came back", ru: "подсечка впустую: поплавок вернулся", pl: "zacięcie w próżnię: spławik wrócił" },
    Line { msg: Msg::FishingCaught,     en: "caught", simple: "you caught", ru: "улов", pl: "złowiono" },
    Line { msg: Msg::FishingLineGone,   en: "the line went slack: it is gone", simple: "the line broke -- the fish got away", ru: "леска ослабла: рыба ушла", pl: "żyłka poluzowała: ryba uciekła" },

    Line { msg: Msg::MapTab,       en: "MAP",          simple: "MAP",        ru: "КАРТА", pl: "MAPA" },
    Line { msg: Msg::RecipesTab,   en: "RECIPES",      simple: "HOW TO MAKE", ru: "РЕЦЕПТЫ", pl: "PRZEPISY" },
    Line { msg: Msg::MapYou,       en: "you",          simple: "you",        ru: "вы", pl: "ty" },
    Line { msg: Msg::MapSpawn,     en: "spawn",        simple: "where you start", ru: "начало", pl: "start" },
    Line { msg: Msg::MapBag,       en: "your bag",     simple: "your things", ru: "ваш рюкзак", pl: "twój plecak" },
    Line { msg: Msg::MapUnexplored, en: "nothing seen yet -- the map fills in as you walk", simple: "you have not been anywhere yet -- walk and the map fills in", ru: "вы ещё нигде не были -- карта рисуется по пути", pl: "jeszcze nigdzie nie byłeś -- mapa rysuje się po drodze" },
    Line { msg: Msg::MapCentre,    en: "ME",           simple: "ME",         ru: "Я", pl: "JA" },
    Line { msg: Msg::MapHelp,      en: "drag to look around   wheel zooms   tab recipes   esc close", simple: "drag to look around   wheel zooms   tab recipes   esc close", ru: "тащите карту   колесо масштаб   tab рецепты   esc закрыть", pl: "przeciągnij mapę   kółko skala   tab przepisy   esc zamknij" },
    Line { msg: Msg::MapHelpTouch, en: "drag to look around   pinch or + and - zoom", simple: "drag to look around   pinch or + and - zoom", ru: "тащите карту   щипок или + и - масштаб", pl: "przeciągnij mapę   szczypanie lub + i - skala" },
    Line { msg: Msg::RecipesAll,   en: "ALL",          simple: "ALL",        ru: "ВСЕ", pl: "WSZYSTKO" },
    Line { msg: Msg::RecipesHands, en: "HANDS",        simple: "BY HAND",    ru: "РУКИ", pl: "RĘCE" },
    Line { msg: Msg::RecipesWorkshops, en: "WORKSHOP",  simple: "TABLES",     ru: "МАСТЕРСКАЯ", pl: "WARSZTAT" },
    Line { msg: Msg::RecipesSearch, en: "type to search", simple: "type to find one", ru: "печатайте для поиска", pl: "pisz, aby szukać" },
    Line { msg: Msg::RecipesEmpty, en: "nothing known yet -- pick things up and the book fills", simple: "you know nothing yet -- pick things up and it fills in", ru: "пока пусто -- берите вещи в руки, и книга пополнится", pl: "na razie pusto -- podnoś rzeczy, a księga się zapełni" },
    Line { msg: Msg::RecipesNoMatch, en: "nothing matches", simple: "nothing found", ru: "ничего не нашлось", pl: "nic nie pasuje" },
    Line { msg: Msg::RecipesHelp,  en: "type to search   tab map   esc close", simple: "type to find   tab map   esc close", ru: "печатайте для поиска   tab карта   esc закрыть", pl: "pisz, aby szukać   tab mapa   esc zamknij" },
    Line { msg: Msg::RecipesHelpTouch, en: "tap a recipe to read it", simple: "tap one to read it", ru: "коснитесь рецепта, чтобы прочитать", pl: "dotknij przepisu, by przeczytać" },
    Line { msg: Msg::JournalCloseTouch, en: "tap beside the page to close", simple: "tap outside to close", ru: "коснитесь сбоку, чтобы закрыть", pl: "dotknij obok, by zamknąć" },

    Line { msg: Msg::GiveTab,      en: "GIVE",         simple: "GET THINGS", ru: "ВЫДАЧА", pl: "WYDAWANIE" },
    Line { msg: Msg::GiveBlocks,   en: "BLOCKS",       simple: "BLOCKS",     ru: "БЛОКИ", pl: "BLOKI" },
    Line { msg: Msg::GiveTools,    en: "TOOLS",        simple: "TOOLS",      ru: "ОРУДИЯ", pl: "NARZĘDZIA" },
    Line { msg: Msg::GiveClothes,  en: "CLOTHES",      simple: "CLOTHES",    ru: "ОДЕЖДА", pl: "UBRANIA" },
    Line { msg: Msg::GiveFood,     en: "FOOD",         simple: "FOOD",       ru: "ЕДА", pl: "JEDZENIE" },
    Line { msg: Msg::GiveStuff,    en: "OTHER",        simple: "OTHER",      ru: "ПРОЧЕЕ", pl: "INNE" },
    Line { msg: Msg::GiveHelp,     en: "type to search   tab map   esc close", simple: "type to find   tab map   esc close", ru: "печатайте для поиска   tab карта   esc закрыть", pl: "pisz, aby szukać   tab mapa   esc zamknij" },
    Line { msg: Msg::GiveHelpTouch, en: "tap a thing to ask the server for it", simple: "tap a thing and the server hands it over", ru: "коснитесь вещи, чтобы попросить её у сервера", pl: "dotknij rzeczy, by poprosić o nią serwer" },
    Line { msg: Msg::GiveHint,     en: "tap a thing and the server puts it in your pack", simple: "tap a thing and it goes into your bag", ru: "коснитесь вещи -- сервер положит её в рюкзак", pl: "dotknij rzeczy -- serwer włoży ją do plecaka" },
    Line { msg: Msg::GiveAsking,   en: "asking the server...", simple: "asking the server...", ru: "спрашиваем сервер...", pl: "pytamy serwer..." },
    Line { msg: Msg::GiveGiven,    en: "given",        simple: "you got",    ru: "выдано", pl: "wydano" },
    Line { msg: Msg::GivePackFull, en: "your pack is full -- some of it did not fit", simple: "your bag is full -- some of it did not go in", ru: "рюкзак полон -- часть не поместилась", pl: "plecak pełny -- część się nie zmieściła" },
    Line { msg: Msg::GiveDenied,   en: "only an operator may take things out of nothing -- ask for /op", simple: "only the person running the server may do this -- ask them for /op", ru: "брать вещи из ничего может только оператор -- попросите /op", pl: "tylko operator może brać rzeczy z niczego -- poproś o /op" },
    Line { msg: Msg::GiveRefused,  en: "the server said:", simple: "the server said:", ru: "сервер ответил:", pl: "serwer odpowiedział:" },
    Line { msg: Msg::RecipeMadeFrom, en: "MADE FROM",  simple: "YOU NEED",   ru: "ИЗ ЧЕГО", pl: "Z CZEGO" },
    Line { msg: Msg::RecipeWhere,  en: "WHERE",        simple: "WHERE",      ru: "ГДЕ", pl: "GDZIE" },
    Line { msg: Msg::RecipeByHand, en: "by hand, anywhere", simple: "with your hands, anywhere", ru: "руками, где угодно", pl: "rękami, gdziekolwiek" },
    Line { msg: Msg::RecipeAtFire, en: "in any lit fire", simple: "in a fire that is burning", ru: "в любом горящем огне", pl: "w każdym płonącym ogniu" },
    Line { msg: Msg::RecipeAtKiln, en: "in a lit kiln", simple: "in a burning kiln", ru: "в горящем горне", pl: "w rozpalonym piecu" },
    Line { msg: Msg::RecipeAtBloomery, en: "in a lit bloomery", simple: "in a burning iron furnace", ru: "в горящей домнице", pl: "w rozpalonej dymarce" },
    Line { msg: Msg::RecipeAtBench, en: "beside a workbench", simple: "next to a work table", ru: "у верстака", pl: "przy warsztacie" },
    Line { msg: Msg::RecipeAtMason, en: "beside a mason's block", simple: "next to a stone work block", ru: "у колоды каменотёса", pl: "przy bloku kamieniarskim" },
    Line { msg: Msg::RecipeAtWheel, en: "beside a potter's wheel", simple: "next to a clay wheel", ru: "у гончарного круга", pl: "przy kole garncarskim" },
    Line { msg: Msg::RecipeAtLeatherBench, en: "beside a leather bench", simple: "next to a leather table", ru: "у кожевенного стола", pl: "przy stole rymarskim" },
    Line { msg: Msg::RecipeKept,   en: "KEPT AFTERWARDS", simple: "YOU GET BACK", ru: "ОСТАЁТСЯ", pl: "ZOSTAJE" },
    Line { msg: Msg::RecipeLead,   en: "one thing in it you have never held", simple: "you have not found one of these yet", ru: "здесь есть то, чего вы ещё не держали в руках", pl: "jest tu coś, czego jeszcze nie miałeś w rękach" },
    Line { msg: Msg::RecipeNotFound, en: "not found yet", simple: "not found yet", ru: "ещё не найдено", pl: "jeszcze nie znalezione" },
    Line { msg: Msg::RecipeMayFail, en: "can go wrong", simple: "does not always work", ru: "может не выйти", pl: "może się nie udać" },
    Line { msg: Msg::RecipeMissing, en: "STILL MISSING", simple: "YOU STILL NEED", ru: "ЧЕГО НЕ ХВАТАЕТ", pl: "CZEGO BRAKUJE" },
    Line { msg: Msg::RecipeNeedTool, en: "a tool:",      simple: "a tool:",    ru: "инструмент:", pl: "narzędzie:" },
    Line { msg: Msg::RecipeNothingToWork, en: "nothing in the pack for it to work on", simple: "nothing in your bag to use it on", ru: "в рюкзаке нечего этим обрабатывать", pl: "w plecaku nie ma czego tym obrobić" },
    Line { msg: Msg::RecipeHeat,   en: "heat:",          simple: "fire colour:", ru: "жар:", pl: "żar:" },
    Line { msg: Msg::RecipeUsedFor, en: "GOES INTO",     simple: "USED TO MAKE", ru: "ИДЁТ НА", pl: "IDZIE NA" },
    Line { msg: Msg::RecipeUsedForNothing, en: "nothing you know of yet", simple: "nothing you know about yet", ru: "пока ни на что из известного вам", pl: "na razie na nic, co znasz" },
    // The path page, in the pack. See `ui::ladder_screen`.
    //
    // It has no help line of its own any more. It had two -- `tab
    // recipes   esc close` and `tap an age to read it` -- and both were
    // about the journal's footer, which this page no longer stands in;
    // the pack closes the pack's way, and a page with one thing to press
    // does not need telling that it can be pressed.
    Line { msg: Msg::LadderTab,    en: "LADDER",         simple: "THE WAY UP", ru: "ПУТЬ", pl: "DROGA" },
    Line { msg: Msg::LadderYouAreHere, en: "you are here", simple: "you are here", ru: "вы здесь", pl: "tu jesteś" },
    Line { msg: Msg::LadderNext,   en: "NEXT",           simple: "NEXT",       ru: "СЛЕДУЮЩЕЕ", pl: "NASTĘPNE" },
    Line { msg: Msg::LadderWants,  en: "WHAT IT TAKES", simple: "WHAT YOU NEED", ru: "ЧТО НУЖНО", pl: "CZEGO TRZEBA" },
    Line { msg: Msg::LadderDone,   en: "the whole ladder is behind you", simple: "you have done all of it", ru: "весь путь пройден", pl: "cała droga za tobą" },
    Line { msg: Msg::AgeBareHands, en: "BARE HANDS",     simple: "JUST HANDS", ru: "ГОЛЫЕ РУКИ", pl: "GOŁE RĘCE" },
    Line { msg: Msg::AgeFlint,     en: "FLINT",          simple: "FLINT",      ru: "КРЕМЕНЬ", pl: "KRZEMIEŃ" },
    Line { msg: Msg::AgeFire,      en: "FIRE",           simple: "FIRE",       ru: "ОГОНЬ", pl: "OGIEŃ" },
    Line { msg: Msg::AgeClay,      en: "CLAY",           simple: "CLAY",       ru: "ГЛИНА", pl: "GLINA" },
    Line { msg: Msg::AgeCopper,    en: "COPPER",         simple: "COPPER",     ru: "МЕДЬ", pl: "MIEDŹ" },
    Line { msg: Msg::AgeBronze,    en: "BRONZE",         simple: "BRONZE",     ru: "БРОНЗА", pl: "BRĄZ" },
    Line { msg: Msg::AgeIron,      en: "IRON",           simple: "IRON",       ru: "ЖЕЛЕЗО", pl: "ŻELAZO" },
    Line { msg: Msg::FoundUnderfoot, en: "underfoot, in any meadow", simple: "on the ground, anywhere", ru: "под ногами, в любом лугу", pl: "pod nogami, na każdej łące" },
    Line { msg: Msg::FoundRiverbank, en: "on gravel and river banks", simple: "on stones by the water", ru: "на отмелях и берегах рек", pl: "na żwirze i brzegach rzek" },
    Line { msg: Msg::FoundHills,   en: "in the hills, where the rock is bare", simple: "in the hills, where you can see rock", ru: "в холмах, где камень голый", pl: "w górach, gdzie skała jest naga" },
    Line { msg: Msg::FoundDeepRock, en: "deep in the rock, down a shaft", simple: "deep underground", ru: "глубоко в породе, в шахте", pl: "głęboko w skale, w szybie" },
    Line { msg: Msg::FoundWoods,   en: "in standing timber", simple: "in the woods", ru: "в лесу, на стоячем дереве", pl: "w lesie, na stojącym drzewie" },
    // The first two minutes. See `ladder::first_step`.
    Line { msg: Msg::StepStone,    en: "there are stones lying on the ground", simple: "there are stones on the ground", ru: "камни лежат прямо на земле", pl: "kamienie leżą wprost na ziemi" },
    Line { msg: Msg::StepFibre,    en: "tall grass comes apart into fibre", simple: "tall grass gives you fibre", ru: "высокая трава расходится на волокно", pl: "wysoka trawa rozchodzi się na włókno" },
    Line { msg: Msg::StepFlake,    en: "flint splits into flakes when it is struck", simple: "hit flint and it breaks into flakes", ru: "кремень от удара колется на отщепы", pl: "krzemień od uderzenia łupie się na odłupki" },
    // The path page. See `ui::ladder_screen`.
    Line { msg: Msg::LearnNow,     en: "WHAT NOW",      simple: "WHAT TO DO", ru: "ЧТО СЕЙЧАС", pl: "CO TERAZ" },
    Line { msg: Msg::LearnControls, en: "CONTROLS",     simple: "THE KEYS",   ru: "УПРАВЛЕНИЕ", pl: "STEROWANIE" },
    Line { msg: Msg::LearnDig,     en: "left button -- dig, and hit", simple: "left button breaks things", ru: "левая кнопка -- копать и бить", pl: "lewy przycisk -- kopać i bić" },
    Line { msg: Msg::LearnPlace,   en: "right button -- place, and use", simple: "right button puts things down", ru: "правая кнопка -- ставить и брать", pl: "prawy przycisk -- stawiać i używać" },
    Line { msg: Msg::LearnDigTouch, en: "left thumb -- dig, and hit", simple: "left thumb breaks things", ru: "левый палец -- копать и бить", pl: "lewy kciuk -- kopać i bić" },
    Line { msg: Msg::LearnPlaceTouch, en: "right thumb -- place, and use", simple: "right thumb puts things down", ru: "правый палец -- ставить и брать", pl: "prawy kciuk -- stawiać i używać" },
    Line { msg: Msg::LearnFound,   en: "FOUND SO FAR",  simple: "WHAT YOU HAVE SEEN", ru: "НАЙДЕНО", pl: "ZNALEZIONE" },
    // The body in the pack. See the note on `Msg::Wounds`.
    Line { msg: Msg::Wounds,       en: "WOUNDS",       simple: "HURTS",      ru: "РАНЫ", pl: "RANY" },
    Line { msg: Msg::PartLeftArm,  en: "LEFT ARM",     simple: "LEFT ARM",   ru: "ЛЕВАЯ РУКА", pl: "LEWA RĘKA" },
    Line { msg: Msg::PartRightArm, en: "RIGHT ARM",    simple: "RIGHT ARM",  ru: "ПРАВАЯ РУКА", pl: "PRAWA RĘKA" },
    Line { msg: Msg::PartLeftLeg,  en: "LEFT LEG",     simple: "LEFT LEG",   ru: "ЛЕВАЯ НОГА", pl: "LEWA NOGA" },
    Line { msg: Msg::PartRightLeg, en: "RIGHT LEG",    simple: "RIGHT LEG",  ru: "ПРАВАЯ НОГА", pl: "PRAWA NOGA" },
    Line { msg: Msg::WoundCut,     en: "CUT",          simple: "CUT",        ru: "ПОРЕЗ", pl: "SKALECZENIE" },
    Line { msg: Msg::WoundBruise,  en: "BRUISE",       simple: "BRUISE",     ru: "УШИБ", pl: "STŁUCZENIE" },
    Line { msg: Msg::WoundFracture, en: "BROKEN BONE", simple: "BROKEN BONE", ru: "ПЕРЕЛОМ", pl: "ZŁAMANIE" },
    Line { msg: Msg::WoundBurn,    en: "BURN",         simple: "BURN",       ru: "ОЖОГ", pl: "OPARZENIE" },
    Line { msg: Msg::SeverityLight, en: "LIGHT",       simple: "SMALL",      ru: "ЛЁГКИЙ", pl: "LEKKIE" },
    Line { msg: Msg::SeveritySerious, en: "SERIOUS",   simple: "BAD",        ru: "СЕРЬЁЗНЫЙ", pl: "POWAŻNE" },
    Line { msg: Msg::SeveritySevere, en: "SEVERE",     simple: "VERY BAD",   ru: "ТЯЖЁЛЫЙ", pl: "CIĘŻKIE" },
    Line { msg: Msg::WoundBleeding, en: "bleeding, needs a bandage", simple: "bleeding, put a bandage on", ru: "кровоточит, нужен бинт", pl: "krwawi, potrzebny bandaż" },
    Line { msg: Msg::WoundNeedsSplint, en: "will not knit without a splint", simple: "needs a splint to get better", ru: "без шины не срастётся", pl: "bez szyny się nie zrośnie" },
    Line { msg: Msg::WoundNeedsDressing, en: "will not heal undressed", simple: "needs a poultice or a bandage", ru: "без повязки не заживёт", pl: "bez opatrunku się nie zagoi" },
    Line { msg: Msg::WoundHealsAlone, en: "heals by itself", simple: "gets better on its own", ru: "пройдёт само", pl: "zagoi się samo" },
    Line { msg: Msg::WoundBandaged, en: "bandaged, healing", simple: "bandaged, getting better", ru: "перевязан, заживает", pl: "opatrzone, goi się" },
    Line { msg: Msg::WoundSplinted, en: "splinted, knitting", simple: "in a splint, getting better", ru: "в шине, срастается", pl: "w szynie, zrasta się" },
    Line { msg: Msg::WoundPoultice, en: "under a poultice, healing", simple: "poultice on, getting better", ru: "под припаркой, заживает", pl: "pod okładem, goi się" },
    Line { msg: Msg::NoWounds,     en: "no wounds",    simple: "not hurt",   ru: "без ран", pl: "bez ran" },
    Line { msg: Msg::NoticeBleeding, en: "you are bleeding - bandage it from your pack", simple: "you are bleeding - put a bandage on from your pack", ru: "вы истекаете кровью - перевяжите рану из рюкзака", pl: "krwawisz - opatrz ranę z plecaka" },
    Line { msg: Msg::NoticeBleedingStopped, en: "the bleeding has stopped", simple: "you stopped bleeding", ru: "кровь остановилась", pl: "krwawienie ustało" },
    Line { msg: Msg::NoticeBoneBroken, en: "a bone is broken - splint it", simple: "a bone is broken - put a splint on it", ru: "сломана кость - наложите шину", pl: "złamana kość - załóż szynę" },
    Line { msg: Msg::NoticeBoneKnit, en: "the bone has knitted", simple: "the bone is whole again", ru: "кость срослась", pl: "kość się zrosła" },
    Line { msg: Msg::NoticeBadBurn, en: "a bad burn - it will not heal undressed", simple: "a bad burn - put something on it", ru: "сильный ожог - без повязки не заживёт", pl: "ciężkie oparzenie - bez opatrunku się nie zagoi" },

    Line { msg: Msg::AnvilTitle,   en: "ANVIL",        simple: "ANVIL",      ru: "НАКОВАЛЬНЯ", pl: "KOWADŁO" },
    Line { msg: Msg::WheelTitle,   en: "POTTER'S WHEEL", simple: "CLAY WHEEL", ru: "ГОНЧАРНЫЙ КРУГ", pl: "KOŁO GARNCARSKIE" },
    Line { msg: Msg::StationPickJob, en: "pick what to make", simple: "choose what to make", ru: "выберите, что делать", pl: "wybierz, co zrobić" },
    Line { msg: Msg::StationStrike, en: "click or space on the mark", simple: "click or space on the mark", ru: "бейте мышью или пробелом по метке", pl: "kliknij lub spacja na znaku" },
    Line { msg: Msg::StationStrikeTouch, en: "tap on the mark", simple: "tap on the mark", ru: "бейте по метке", pl: "dotknij znaku" },
    Line { msg: Msg::RunFine,      en: "struck true - more out of the same", simple: "well struck - you got more", ru: "точно - вышло больше", pl: "celnie - wyszło więcej" },
    Line { msg: Msg::RunFair,      en: "serviceable",  simple: "good enough", ru: "сойдёт", pl: "ujdzie" },
    Line { msg: Msg::RunRuined,    en: "spoiled - the piece is lost", simple: "ruined - you lost it", ru: "испорчено - заготовка пропала", pl: "zepsute - sztuka stracona" },

    Line { msg: Msg::ShelterAir,   en: "air here",     simple: "air around you", ru: "воздух здесь", pl: "powietrze tutaj" },
    Line { msg: Msg::ShelterSmoky, en: "smoky: the fire needs a way out, a hole in the roof", simple: "smoky: cut a hole in the roof over the fire", ru: "дымно: дыму нужен выход, дыра в крыше", pl: "dym: ogień potrzebuje ujścia, otworu w dachu" },
    Line { msg: Msg::ShelterDraughty, en: "draughty: the wind blows in through an opening", simple: "the wind comes in: shut the door or move it", ru: "сквозняк: ветер дует в проём", pl: "przeciąg: wiatr wieje przez otwór" },
    Line { msg: Msg::ShelterHoleTakesHeat, en: "the smoke hole lets heat out too", simple: "heat goes out of the roof hole too", ru: "дыра в крыше выпускает и тепло", pl: "otwór w dachu wypuszcza też ciepło" },
    Line { msg: Msg::ShelterWallsWarm, en: "these walls keep the night out", simple: "warm walls", ru: "стены хорошо держат тепло", pl: "te ściany dobrze trzymają ciepło" },
    Line { msg: Msg::ShelterWallsThin, en: "these walls let the night in", simple: "cold walls", ru: "стены плохо держат тепло", pl: "te ściany słabo trzymają ciepło" },
    Line { msg: Msg::MapCairn,     en: "cairn",        simple: "stone pile", ru: "тур", pl: "kopiec" },
    Line { msg: Msg::CairnNamePrompt, en: "name this cairn for your map - empty leaves it unnamed", simple: "give this stone pile a name for your map", ru: "назовите тур для карты - пусто значит без имени", pl: "nazwij ten kopiec na mapie - puste zostawia bez nazwy" },
    Line { msg: Msg::SkyBySun,     en: "by the sun",   simple: "by the sun", ru: "по солнцу", pl: "według słońca" },
    Line { msg: Msg::SkyByMoon,    en: "by the moon",  simple: "by the moon", ru: "по луне", pl: "według księżyca" },
    Line { msg: Msg::SkyByStars,   en: "by the stars", simple: "by the stars", ru: "по звёздам", pl: "według gwiazd" },
    Line { msg: Msg::NorthAhead,   en: "north is ahead", simple: "north is in front of you", ru: "север впереди", pl: "północ przed tobą" },
    Line { msg: Msg::NorthRight,   en: "north is to your right", simple: "north is on your right", ru: "север справа", pl: "północ po prawej" },
    Line { msg: Msg::NorthBehind,  en: "north is behind you", simple: "north is behind you", ru: "север за спиной", pl: "północ za tobą" },
    Line { msg: Msg::NorthLeft,    en: "north is to your left", simple: "north is on your left", ru: "север слева", pl: "północ po lewej" },

    Line { msg: Msg::Stall,        en: "STALL",        simple: "TRADING TABLE", ru: "ПРИЛАВОК", pl: "STRAGAN" },
    Line { msg: Msg::StallPrices,  en: "PRICES",       simple: "SWAPS",      ru: "ЦЕНЫ", pl: "CENY" },
    Line { msg: Msg::StallYourPrices, en: "YOUR PRICES", simple: "YOUR SWAPS", ru: "ВАШИ ЦЕНЫ", pl: "TWOJE CENY" },
    Line { msg: Msg::StallStock,   en: "ON THE COUNTER", simple: "TO SWAP AWAY", ru: "НА ПРИЛАВКЕ", pl: "NA LADZIE" },
    Line { msg: Msg::StallTakings, en: "TAKINGS",      simple: "PAID TO YOU", ru: "ВЫРУЧКА", pl: "UTARG" },
    Line { msg: Msg::StallTrade,   en: "TRADE",        simple: "SWAP",       ru: "ОБМЕН", pl: "WYMIEŃ" },
    Line { msg: Msg::StallClear,   en: "REMOVE",       simple: "REMOVE",     ru: "УБРАТЬ", pl: "USUŃ" },
    Line { msg: Msg::StallLeft,    en: "left",         simple: "left",       ru: "в наличии", pl: "zostało" },
    Line { msg: Msg::StallHintOwner, en: "hold a thing, tap a square to price it", simple: "pick up a thing, tap a square to ask for it", ru: "возьмите вещь и нажмите на клетку цены", pl: "weź rzecz i stuknij pole ceny" },
    Line { msg: Msg::StallHintBuyer, en: "TRADE gives the price for the goods", simple: "SWAP gives what is asked for what is shown", ru: "ОБМЕН отдаёт цену за товар", pl: "WYMIEŃ oddaje cenę za towar" },
    Line { msg: Msg::StallNotYours, en: "this stall is not yours", simple: "this is somebody else's table", ru: "это чужой прилавок", pl: "to nie twój stragan" },
    Line { msg: Msg::StallNoOffer, en: "nothing is offered there", simple: "nothing to swap there", ru: "там ничего не предлагают", pl: "nic tam nie ma w ofercie" },
    Line { msg: Msg::StallOfferChanged, en: "the price has just changed - look again", simple: "the swap just changed - look again", ru: "цена только что изменилась - посмотрите снова", pl: "cena właśnie się zmieniła - spójrz jeszcze raz" },
    Line { msg: Msg::StallSoldOut, en: "sold out", simple: "none left", ru: "всё разобрали", pl: "wyprzedane" },
    Line { msg: Msg::StallCannotPay, en: "you do not carry the price", simple: "you do not have what is asked", ru: "у вас нет того, что просят", pl: "nie masz tego, o co proszą" },
    Line { msg: Msg::StallNoRoom,  en: "no room in your pack", simple: "your bag is full", ru: "нет места в рюкзаке", pl: "brak miejsca w plecaku" },
    Line { msg: Msg::StallTillFull, en: "the takings are full - the owner must empty them", simple: "the table is full of payments - the owner must take them", ru: "выручке нет места - хозяину пора её забрать", pl: "utarg pełny - właściciel musi go zabrać" },
    Line { msg: Msg::StallBadOffer, en: "that is not a price", simple: "that swap cannot be", ru: "такой цены быть не может", pl: "to nie jest cena" },
    Line { msg: Msg::SawhorseTitle, en: "SAWHORSE", simple: "SAWING TRESTLE", ru: "КОЗЛЫ", pl: "KOZIOŁ STOLARSKI" },
    Line { msg: Msg::HoningTitle, en: "HONING STONE", simple: "SHARPENING STONE", ru: "ТОЧИЛО", pl: "KAMIEŃ SZLIFIERSKI" },
    Line { msg: Msg::WoundBarked, en: "bound with willow bark, going down", simple: "willow bark on it, getting better fast", ru: "под ивовой корой, спадает", pl: "pod korą wierzby, schodzi" },
    // What the server tells a player (`ServerMessage::Notice`): its refusals and
    // its news, which were English sentences on the wire. See `notice`.
    //
    // **Russian here says "вы", like the rest of the interface.** These
    // were written as "ты" -- "у тебя ничего нет в руке", "сначала
    // приручи" -- while every other line the game prints says "вы"
    // ("коснитесь поля", "перевяжите рану"). Both land in the same banner
    // in the same second, so a player was addressed two ways by one
    // program. Better still is no pronoun at all, which is what most of
    // these now do: "в руке ничего нет" is shorter than either.
    Line { msg: Msg::Notice(Notice::NothingInHand), en: "you have nothing in your hand", simple: "your hand is empty", ru: "в руке ничего нет", pl: "nie masz nic w ręce" },
    Line { msg: Msg::Notice(Notice::AnimalGone), en: "it is gone", simple: "it is not there any more", ru: "его уже нет", pl: "już go nie ma" },
    Line { msg: Msg::Notice(Notice::TooFarAway), en: "too far away", simple: "too far away", ru: "слишком далеко", pl: "za daleko" },
    Line { msg: Msg::Notice(Notice::CannotBeKept), en: "that animal cannot be kept", simple: "you cannot keep that animal", ru: "это животное не приручить", pl: "tego zwierzęcia nie da się oswoić" },
    Line { msg: Msg::Notice(Notice::NothingToShear), en: "there is nothing on it to shear", simple: "it has no wool to cut", ru: "с него нечего стричь", pl: "nie ma z niego czego strzyc" },
    Line { msg: Msg::Notice(Notice::TameItFirst), en: "it will not stand for the knife: tame it first", simple: "it will not let you cut it - tame it first", ru: "не даётся под нож: сначала приручите", pl: "nie da się ostrzyc: najpierw ją oswój" },
    Line { msg: Msg::Notice(Notice::FleeceNotGrown), en: "the fleece has not grown back yet", simple: "the wool has not grown back yet", ru: "шерсть ещё не отросла", pl: "runo jeszcze nie odrosło" },
    Line { msg: Msg::Notice(Notice::OnlyEweGivesMilk), en: "only a tame ewe with a lamb gives milk", simple: "only a tame mother sheep with a lamb gives milk", ru: "молоко даёт только ручная овца с ягнёнком", pl: "mleko daje tylko oswojona owca z jagnięciem" },
    Line { msg: Msg::Notice(Notice::NoMilkYet), en: "she has no milk to give yet", simple: "she has no milk yet", ru: "у неё пока нет молока", pl: "nie ma jeszcze mleka" },
    Line { msg: Msg::Notice(Notice::ShiesAway), en: "it shies away from you: come at it slowly", simple: "it is afraid - walk up to it slowly", ru: "шарахается: подходите медленно", pl: "płoszy się: podchodź powoli" },
    Line { msg: Msg::Notice(Notice::NotHungry), en: "it is not hungry yet", simple: "it is not hungry yet", ru: "оно ещё не голодно", pl: "nie jest jeszcze głodne" },
    Line { msg: Msg::Notice(Notice::DoesNotEatThat), en: "it does not eat that", simple: "it will not eat that", ru: "оно такое не ест", pl: "tego nie je" },
    Line { msg: Msg::Notice(Notice::GoesOnAHorse), en: "that goes on a horse", simple: "that is for a horse", ru: "это надевают на лошадь", pl: "to zakłada się koniowi" },
    Line { msg: Msg::Notice(Notice::BreakItFirst), en: "it will not stand for that: break it first", simple: "it will not let you - ride it in first", ru: "не даётся: сначала объездите", pl: "nie pozwoli: najpierw go ujeźdź" },
    Line { msg: Msg::Notice(Notice::TooYoungToCarry), en: "it is too young to carry anything", simple: "it is too young to carry things", ru: "молодняк не навьючивают", pl: "jest za młody, żeby coś nieść" },
    Line { msg: Msg::Notice(Notice::AlreadySaddled), en: "it already wears a saddle", simple: "it has a saddle on already", ru: "седло на нём уже есть", pl: "już ma siodło" },
    Line { msg: Msg::Notice(Notice::AlreadyBagged), en: "it already carries saddlebags", simple: "it has bags on already", ru: "перемётные сумы на нём уже есть", pl: "już ma juki" },
    Line { msg: Msg::Notice(Notice::AlreadyRiding), en: "you are already riding", simple: "you are already riding", ru: "вы уже в седле", pl: "już jedziesz wierzchem" },
    Line { msg: Msg::Notice(Notice::CannotRideThat), en: "you cannot ride that", simple: "you cannot ride that", ru: "на этом не поездить", pl: "na tym nie pojedziesz" },
    Line { msg: Msg::Notice(Notice::SomebodyOnIt), en: "somebody is already on it", simple: "somebody is riding it", ru: "на нём уже кто-то сидит", pl: "ktoś już na nim siedzi" },
    Line { msg: Msg::Notice(Notice::TooYoungToRide), en: "it is too young to carry anybody", simple: "it is too young to ride", ru: "на молодняк не садятся", pl: "jest za młody, żeby kogoś nieść" },
    Line { msg: Msg::Notice(Notice::GentleItFirst), en: "it will not let you near its back: gentle it with food first", simple: "it will not let you on - feed it first", ru: "не подпускает к спине: сначала прикормите", pl: "nie da ci wsiąść: najpierw go dokarm" },
    Line { msg: Msg::Notice(Notice::LetItSettle), en: "it is still wild-eyed from the last time: let it settle", simple: "it is still scared from last time - wait a little", ru: "ещё не успокоилась после прошлого раза: дайте ей время", pl: "jeszcze się nie uspokoił: daj mu chwilę" },
    Line { msg: Msg::Notice(Notice::HorseThrowsYou), en: "the horse throws you: let it settle, and try again", simple: "the horse threw you off - wait, then try again", ru: "лошадь сбросила вас: дайте ей успокоиться и попробуйте снова", pl: "koń cię zrzucił: daj mu się uspokoić i spróbuj znowu" },
    Line { msg: Msg::Notice(Notice::HorseIsYours), en: "the horse stands for you: it is yours now, and home is here", simple: "the horse lets you ride - it is yours now, and this is its home", ru: "лошадь стоит смирно: теперь она ваша, и её дом здесь", pl: "koń stoi spokojnie: jest teraz twój, a jego dom jest tutaj" },
    Line { msg: Msg::Notice(Notice::HorseTakesFood), en: "the horse takes it from your hand: it may let you on its back now", simple: "the horse eats from your hand - you can try to ride it now", ru: "лошадь берёт с руки: теперь можно попробовать сесть верхом", pl: "koń je z ręki: możesz spróbować go dosiąść" },
    Line { msg: Msg::Notice(Notice::MonkeyTakesIt), en: "a monkey snatches it out of your hand and is up the tree", simple: "a monkey took the food from your hand", ru: "обезьяна выхватывает еду из руки и уносит на дерево", pl: "małpa wyrywa ci jedzenie z ręki i ucieka na drzewo" },
    Line { msg: Msg::Notice(Notice::NoBagsInReach), en: "there are no saddlebags within reach", simple: "there are no horse bags near you", ru: "рядом нет перемётных сум", pl: "w pobliżu nie ma juków" },
    Line { msg: Msg::Notice(Notice::HorseGone), en: "your horse is gone", simple: "your horse is gone", ru: "вашей лошади больше нет", pl: "twojego konia już nie ma" },
    Line { msg: Msg::Notice(Notice::NothingToUnbuckle), en: "it wears nothing to take off", simple: "there is nothing on it to take off", ru: "снимать с неё нечего", pl: "nie ma z niego czego zdjąć" },
    Line { msg: Msg::Notice(Notice::PackCannotTakeBags), en: "your pack cannot take the saddlebags and their load", simple: "your bag has no room for the horse bags and what is in them", ru: "в рюкзак не влезут сумы с поклажей", pl: "juki z ładunkiem nie zmieszczą się w plecaku" },
    Line { msg: Msg::Notice(Notice::WoodWet), en: "the wood is wet: dry it by a fire or in the sun first", simple: "the wood is wet - dry it by a fire or in the sun", ru: "дрова сырые: просушите их у огня или на солнце", pl: "drewno jest mokre: najpierw wysusz je przy ogniu lub na słońcu" },
    Line { msg: Msg::Notice(Notice::FuelWet), en: "the fuel is wet: dry it by a fire or in the sun first", simple: "the fuel is wet - dry it by a fire or in the sun", ru: "топливо сырое: просушите его у огня или на солнце", pl: "opał jest mokry: najpierw wysusz go przy ogniu lub na słońcu" },
    Line { msg: Msg::Notice(Notice::TorchWet), en: "the torch is wet: dry it by a fire or in the sun first", simple: "the torch is wet - dry it by a fire or in the sun", ru: "факел сырой: просушите его у огня или на солнце", pl: "pochodnia jest mokra: najpierw wysusz ją przy ogniu lub na słońcu" },
    Line { msg: Msg::Notice(Notice::PackFull), en: "your pack is full", simple: "your bag is full", ru: "рюкзак полон", pl: "plecak jest pełny" },
    Line { msg: Msg::Notice(Notice::JugNotEmpty), en: "the jug has something in it", simple: "the jug is not empty", ru: "в кувшине что-то есть", pl: "w dzbanie coś jest" },
    Line { msg: Msg::Notice(Notice::BarrelFull), en: "the barrel is full", simple: "the barrel is full", ru: "бочка полна", pl: "beczka jest pełna" },
    Line { msg: Msg::Notice(Notice::BarrelEmpty), en: "the barrel is empty", simple: "the barrel is empty", ru: "бочка пуста", pl: "beczka jest pusta" },
    Line { msg: Msg::Notice(Notice::EmptyRucksackFirst), en: "empty the rucksack first", simple: "take everything out of the rucksack first", ru: "сначала освободите рюкзак", pl: "najpierw opróżnij plecak" },
    Line { msg: Msg::Notice(Notice::NowhereForBowl), en: "nowhere to put the bowl", simple: "there is no room for the bowl", ru: "миску некуда деть", pl: "nie ma gdzie odłożyć miski" },
    Line { msg: Msg::Notice(Notice::ToolBroke), en: "your tool broke", simple: "your tool broke", ru: "инструмент сломался", pl: "narzędzie się złamało" },
    Line { msg: Msg::Notice(Notice::NeedsAKnife), en: "it needs a knife", simple: "you need a knife for that", ru: "тут нужен нож", pl: "potrzebny jest nóż" },
    Line { msg: Msg::Notice(Notice::NeedHammerAtAnvil), en: "you need a hammer to work at an anvil", simple: "you need a hammer to use the anvil", ru: "чтобы работать на наковальне, нужен молот", pl: "do pracy na kowadle potrzebny jest młot" },
    Line { msg: Msg::Notice(Notice::NeedSawAtSawhorse), en: "you need a saw to work at a sawhorse", simple: "you need a saw to use the trestle", ru: "чтобы работать на козлах, нужна пила", pl: "do pracy na koźle potrzebna jest piła" },
    Line { msg: Msg::Notice(Notice::HoldBladeToSharpen), en: "hold the blade you want to sharpen", simple: "hold the blade you want to sharpen", ru: "возьмите в руку клинок, который хотите наточить", pl: "weź do ręki ostrze, które chcesz naostrzyć" },
    Line { msg: Msg::Notice(Notice::NotAtStation), en: "you are not at that station", simple: "you are not at that work place", ru: "вы не у этого верстака", pl: "nie jesteś przy tym stanowisku" },
    Line { msg: Msg::Notice(Notice::NotARun), en: "that is not a run", simple: "that is not something you can make here", ru: "этого здесь не сделать", pl: "tego się tu nie zrobi" },
    Line { msg: Msg::Notice(Notice::NothingOnAnvil), en: "there is nothing on the anvil", simple: "the anvil is empty", ru: "на наковальне ничего нет", pl: "na kowadle nic nie ma" },
    Line { msg: Msg::Notice(Notice::PieceWentCold), en: "the piece went cold", simple: "the metal went cold", ru: "заготовка остыла", pl: "odkuwka ostygła" },
    Line { msg: Msg::Notice(Notice::FurrowHasAsh), en: "this furrow already has ash in it", simple: "this row already has ash in it", ru: "в этой борозде уже есть зола", pl: "w tej bruździe już jest popiół" },
    Line { msg: Msg::Notice(Notice::AshDugIn), en: "ash dug into the furrow: the next crop here grows a third faster", simple: "ash is in the soil - the next crop here grows a third faster", ru: "зола в борозде: следующий урожай здесь вырастет на треть быстрее", pl: "popiół w bruździe: następny plon wyrośnie tu o jedną trzecią szybciej" },
    Line { msg: Msg::Notice(Notice::AlreadyAsleep), en: "you are already asleep", simple: "you are already asleep", ru: "вы уже спите", pl: "już śpisz" },
    Line { msg: Msg::Notice(Notice::HurtCannotSleep), en: "you cannot sleep while something is hurting you", simple: "you cannot sleep while you are being hurt", ru: "нельзя уснуть, пока вас ранят", pl: "nie zaśniesz, kiedy coś cię rani" },
    Line { msg: Msg::Notice(Notice::TooHungryToSleep), en: "you are too hungry to sleep", simple: "you are too hungry to sleep", ru: "слишком голодны, чтобы уснуть", pl: "jesteś zbyt głodny, żeby zasnąć" },
    Line { msg: Msg::Notice(Notice::TooThirstyToSleep), en: "you are too thirsty to sleep", simple: "you are too thirsty to sleep", ru: "слишком хочется пить, чтобы уснуть", pl: "jesteś zbyt spragniony, żeby zasnąć" },
    Line { msg: Msg::Notice(Notice::BedTaken), en: "somebody is already asleep there", simple: "somebody is already sleeping there", ru: "там уже кто-то спит", pl: "ktoś już tam śpi" },
    Line { msg: Msg::Notice(Notice::SeatTaken), en: "somebody is already sitting there", simple: "somebody is already sitting there", ru: "там уже кто-то сидит", pl: "ktoś już tam siedzi" },
    Line { msg: Msg::Notice(Notice::NoRoomToSit), en: "there is no room to sit there", simple: "there is no room to sit there", ru: "там негде сесть", pl: "nie ma tam miejsca, żeby usiąść" },
    Line { msg: Msg::Notice(Notice::StepAboardForOars), en: "step aboard to take the oars", simple: "get on the raft to row", ru: "чтобы грести, встаньте на плот", pl: "wejdź na tratwę, żeby wiosłować" },
    Line { msg: Msg::Notice(Notice::RaftAlreadyThere), en: "there is already a raft there", simple: "there is a raft there already", ru: "там уже есть плот", pl: "tam już jest tratwa" },
    Line { msg: Msg::Notice(Notice::TrapEmpty), en: "nothing has gone into the trap yet", simple: "the trap is still empty", ru: "в ловушку пока ничего не попало", pl: "w pułapkę jeszcze nic nie wpadło" },
    Line { msg: Msg::Notice(Notice::LineParted), en: "the line parted: the rod is done", simple: "the line broke - the rod is finished", ru: "леска лопнула: удочке конец", pl: "żyłka pękła: wędka się skończyła" },
    Line { msg: Msg::Notice(Notice::WillNotOpen), en: "it will not open", simple: "it does not open", ru: "не открывается", pl: "nie da się otworzyć" },
    Line { msg: Msg::Notice(Notice::NoRoomToTakeOff), en: "no room to take that off", simple: "no room in your bag to take that off", ru: "некуда это снять", pl: "nie ma gdzie tego zdjąć" },
    Line { msg: Msg::Notice(Notice::TooMuchLyingAround), en: "too much is already lying around to drop that", simple: "too many things are on the ground here already", ru: "здесь и так слишком много всего валяется", pl: "za dużo już tu leży, żeby to upuścić" },
    Line { msg: Msg::Notice(Notice::SetDownOnSolidGround), en: "that is set down on top of solid ground, in an empty place", simple: "put that on solid ground, where nothing else is", ru: "это ставят на твёрдую землю, в пустое место", pl: "to stawia się na twardym gruncie, w pustym miejscu" },
    Line { msg: Msg::Notice(Notice::SnareEmpty), en: "nothing has come to the snare yet", simple: "no hare in the snare yet -- come back later", ru: "в силок ещё никто не попался", pl: "nic jeszcze nie wpadło w sidła" },
    Line { msg: Msg::Notice(Notice::PanDrying), en: "the pan is still drying: it wants sun and no rain", simple: "the salt is not ready -- it needs sun, and rain undoes it", ru: "соль ещё выпаривается: нужно солнце и ни капли дождя", pl: "panew jeszcze paruje: trzeba słońca i żadnego deszczu" },
    Line { msg: Msg::Notice(Notice::PanWantsSea), en: "a salt pan is filled from a jug of the sea", simple: "pour a jug of sea water into it", ru: "солеварню наполняют кувшином морской воды", pl: "panew napełnia się dzbanem morskiej wody" },
    Line { msg: Msg::Notice(Notice::PanFreshWater), en: "that is fresh water: it dries to nothing", simple: "that water has no salt in it", ru: "это пресная вода: от неё ничего не останется", pl: "to słodka woda: nic z niej nie zostanie" },
    Line { msg: Msg::Notice(Notice::SeaWaterIsSalt), en: "the sea is salt, and you are thirstier for it", simple: "sea water makes you more thirsty, not less", ru: "море солёное -- от него пить хочется сильнее", pl: "morze jest słone -- po nim pragnienie jest większe" },
    Line { msg: Msg::Notice(Notice::StaleWater), en: "the water is stale, and it sits badly", simple: "this water is old -- it will make you ill", ru: "вода затхлая, и она ещё аукнется", pl: "woda jest zastała i odbije się czkawką" },
    Line { msg: Msg::Notice(Notice::RaftNeedsOpenWater), en: "a raft needs open water: three blocks long, two wide, and deep enough to float", simple: "a raft needs open water - three blocks long, two wide, and deep enough", ru: "плоту нужна чистая вода: три блока в длину, два в ширину и достаточно глубоко", pl: "tratwa potrzebuje otwartej wody: trzy bloki długości, dwa szerokości i dość głęboko" },
    // Woken at home by a lit fire. See `comfort::RESTED_SECONDS`.
    // Where tin is, on the path page. See `ladder::Found::FarCountry`.
    Line { msg: Msg::FoundFarCountry, en: "far from home, in tin country: an expedition", simple: "far away - you must travel to find it", ru: "далеко от дома, в оловянном краю: туда нужен поход", pl: "daleko od domu, w krainie cyny: to wyprawa" },
    Line { msg: Msg::Notice(Notice::SleptAtHome), en: "you slept well at home: the day ahead will cost less food", simple: "you slept well at home - you will get hungry slower today", ru: "вы хорошо выспались дома: день пройдёт сытнее", pl: "dobrze się wyspałeś w domu: dzień minie syciej" },
    // The rest of the English the server was sending. See `notice`.
    Line { msg: Msg::Notice(Notice::EditPutBack), en: "the world put that back", simple: "that did not work", ru: "мир вернул всё как было", pl: "świat cofnął tę zmianę" },
    Line { msg: Msg::Notice(Notice::PluginRefused), en: "a plugin refused that change", simple: "a mod stopped that", ru: "мод запретил это изменение", pl: "mod zablokował tę zmianę" },
    Line { msg: Msg::Notice(Notice::NotInsideYourself), en: "you cannot place a block inside yourself", simple: "you are standing there", ru: "нельзя ставить блок в самого себя", pl: "nie można postawić bloku w sobie" },
    Line { msg: Msg::Notice(Notice::NotInsideSomebody), en: "somebody is standing there", simple: "somebody is standing there", ru: "там кто-то стоит", pl: "ktoś tam stoi" },
    Line { msg: Msg::Notice(Notice::NeedBetterTool), en: "you need a better tool for that", simple: "your tool is too weak for this", ru: "для этого нужен инструмент получше", pl: "do tego potrzeba lepszego narzędzia" },
    Line { msg: Msg::Notice(Notice::NeedsSolidGround), en: "that needs solid ground under it", simple: "put it on solid ground", ru: "под этим нужна твёрдая земля", pl: "to potrzebuje twardego gruntu pod spodem" },
    Line { msg: Msg::Notice(Notice::LeanToNeedsRoom), en: "a lean-to needs three by three cells of clear, level ground in front of you and room over them", simple: "a lean-to needs a flat, empty space three by three in front of you", ru: "шалашу нужно ровное свободное место три на три клетки перед вами и простор над ним", pl: "szałas potrzebuje równego, wolnego miejsca trzy na trzy przed tobą i przestrzeni nad nim" },
    Line { msg: Msg::Notice(Notice::BedNeedsRoom), en: "a bed needs a second free cell on solid ground behind it", simple: "a bed needs one more free space on solid ground behind it", ru: "кровати нужна ещё одна свободная клетка на твёрдой земле позади", pl: "łóżko potrzebuje drugiej wolnej komórki na twardym gruncie za nim" },
    Line { msg: Msg::Notice(Notice::RackNeedsRoom), en: "a drying rack needs two cells of clear ground and two of air over them", simple: "a drying rack needs two empty spaces on the ground and air above them", ru: "сушилке нужны две свободные клетки земли и две клетки воздуха над ними", pl: "suszarka potrzebuje dwóch wolnych komórek gruntu i dwóch komórek powietrza nad nimi" },
    Line { msg: Msg::Notice(Notice::DoorNeedsRoom), en: "a door needs a free cell over it for its top half", simple: "a door needs an empty space above it", ru: "двери нужна свободная клетка сверху для верхней половины", pl: "drzwi potrzebują wolnej komórki nad sobą na górną połowę" },
    Line { msg: Msg::Notice(Notice::TorchNeedsRoom), en: "a standing torch needs a free cell over it for its flame", simple: "a standing torch needs an empty space above it", ru: "стоячему факелу нужна свободная клетка сверху для пламени", pl: "stojąca pochodnia potrzebuje wolnej komórki nad sobą na płomień" },
    Line { msg: Msg::Notice(Notice::DoesNotGoThere), en: "that does not go there", simple: "that does not fit there", ru: "это сюда не ставится", pl: "to tu nie pasuje" },
    Line { msg: Msg::Notice(Notice::NotCarryingThat), en: "you are not carrying that", simple: "you do not have that", ru: "у вас этого нет", pl: "nie masz tego przy sobie" },
    Line { msg: Msg::Notice(Notice::PastTheEdge), en: "that is past the edge of the world", simple: "you cannot build there", ru: "там край мира", pl: "tam jest krawędź świata" },
    Line { msg: Msg::Notice(Notice::CannotMakeThat), en: "cannot make that", simple: "you cannot make that", ru: "это не сделать", pl: "nie da się tego zrobić" },
    Line { msg: Msg::Notice(Notice::NotCarryingEnough), en: "you are not carrying enough of that", simple: "you do not have enough of that", ru: "у вас этого не хватает", pl: "nie masz tego dość" },
    Line { msg: Msg::Notice(Notice::WallWantsMortar), en: "that wall is laid in mortar, and you have none", simple: "that wall needs mortar, and you have none", ru: "эта стена кладётся на раствор, а его у вас нет", pl: "ten mur kładzie się na zaprawie, a nie masz jej" },
    Line { msg: Msg::Notice(Notice::WallFinished), en: "that wall is finished", simple: "that wall is as high as it goes", ru: "эта стена уже готова", pl: "ten mur jest już skończony" },
    Line { msg: Msg::Notice(Notice::LiftStillWet), en: "the lift under it is still wet", simple: "the layer below is still wet -- wait", ru: "слой под ним ещё сырой", pl: "warstwa pod spodem jest jeszcze mokra" },
    Line { msg: Msg::Notice(Notice::HeadArmourFell), en: "your head armour fell apart", simple: "your helmet broke", ru: "броня на голове развалилась", pl: "zbroja na głowie się rozpadła" },
    Line { msg: Msg::Notice(Notice::ChestArmourFell), en: "your chest armour fell apart", simple: "your body armour broke", ru: "броня на груди развалилась", pl: "zbroja na piersi się rozpadła" },
    Line { msg: Msg::Notice(Notice::LegsArmourFell), en: "your legs armour fell apart", simple: "your leg armour broke", ru: "броня на ногах развалилась", pl: "zbroja na nogach się rozpadła" },
    Line { msg: Msg::Notice(Notice::FeetArmourFell), en: "your feet armour fell apart", simple: "your boots broke", ru: "броня на ступнях развалилась", pl: "zbroja na stopach się rozpadła" },
    Line { msg: Msg::Notice(Notice::BackArmourFell), en: "your back armour fell apart", simple: "the thing on your back broke", ru: "броня на спине развалилась", pl: "zbroja na plecach się rozpadła" },
    Line { msg: Msg::Notice(Notice::FlintShattered), en: "the flint shattered", simple: "the flint broke into useless bits", ru: "кремень раскололся", pl: "krzemień się rozprysnął" },
    Line { msg: Msg::Notice(Notice::FlintsShattered), en: "{0} flints shattered", simple: "{0} flints broke into useless bits", ru: "раскололось кремней: {0}", pl: "rozprysło się krzemieni: {0}" },
    Line { msg: Msg::Notice(Notice::TrunkScored), en: "this trunk has been scored: it has no more resin for now", simple: "this tree has no more resin for now", ru: "этот ствол уже подсечен: смолы пока больше нет", pl: "ten pień jest już nacięty: na razie nie ma więcej żywicy" },
    Line { msg: Msg::Notice(Notice::BarrelHoldsWater), en: "there is water in the barrel", simple: "the barrel has water in it", ru: "в бочке вода", pl: "w beczce jest woda" },
    Line { msg: Msg::Notice(Notice::BarrelHoldsGrain), en: "there is grain in the barrel", simple: "the barrel has grain in it", ru: "в бочке зерно", pl: "w beczce jest ziarno" },
    Line { msg: Msg::Notice(Notice::BarrelHoldsOtherGrain), en: "the barrel holds another grain", simple: "the barrel has a different grain in it", ru: "в бочке другое зерно", pl: "w beczce jest inne ziarno" },
    Line { msg: Msg::Notice(Notice::BarrelWantsFullJug), en: "a barrel is filled a full jug at a time", simple: "fill the jug all the way first", ru: "бочку наполняют только полным кувшином", pl: "beczkę napełnia się tylko pełnym dzbanem" },
    Line { msg: Msg::Notice(Notice::OnlyGrainInBarrel), en: "only grain is kept in a barrel", simple: "a barrel is only for grain", ru: "в бочке хранят только зерно", pl: "w beczce trzyma się tylko ziarno" },
    Line { msg: Msg::Notice(Notice::WrongCap), en: "the cap was the wrong one, and you know it now", simple: "that mushroom was bad -- you will be ill", ru: "гриб оказался не тот, и теперь вы это знаете", pl: "to był nie ten grzyb i teraz już to wiesz" },
    Line { msg: Msg::Notice(Notice::MeatTurned), en: "it had turned, and it sits badly", simple: "that food was rotten -- you will be ill", ru: "оно протухло, и это ещё аукнется", pl: "to było zepsute i odbije się czkawką" },
    Line { msg: Msg::Notice(Notice::RawFlesh), en: "raw flesh, and your stomach says so", simple: "raw meat -- you will be ill", ru: "сырое мясо, и желудок это чувствует", pl: "surowe mięso i żołądek to czuje" },
    Line { msg: Msg::Notice(Notice::NothingItWouldHelp), en: "nothing there that this would help", simple: "this does not help any wound there", ru: "там нет ничего, чему бы это помогло", pl: "nie ma tam nic, czemu by to pomogło" },
    Line { msg: Msg::Notice(Notice::EdgeAlreadySharp), en: "that edge is already sharp", simple: "that blade is already sharp", ru: "это лезвие и так острое", pl: "to ostrze jest już ostre" },
    Line { msg: Msg::Notice(Notice::ShortOfMaterials), en: "you are short of what that takes", simple: "you do not have everything for that", ru: "не хватает того, что для этого нужно", pl: "brakuje tego, czego to wymaga" },
    Line { msg: Msg::Notice(Notice::NoRoomForIt), en: "no room for what that would make", simple: "no space in your pack for what it makes", ru: "некуда положить то, что получится", pl: "nie ma miejsca na to, co powstanie" },
    Line { msg: Msg::Notice(Notice::StruckEarly), en: "you struck before the marker moved", simple: "you hit too soon", ru: "удар раньше, чем пошла метка", pl: "uderzenie, zanim ruszył znacznik" },
    Line { msg: Msg::Notice(Notice::OutOfTurn), en: "those blows are out of turn", simple: "those hits were in the wrong order", ru: "удары не по порядку", pl: "uderzenia nie po kolei" },
    Line { msg: Msg::Notice(Notice::Rushed), en: "no hand strikes that fast", simple: "that was too fast to be real", ru: "так быстро рука не бьёт", pl: "żadna ręka nie bije tak szybko" },
    Line { msg: Msg::Notice(Notice::RunNotYet), en: "that run has not happened yet", simple: "that has not happened yet", ru: "этой работы ещё не было", pl: "tej pracy jeszcze nie było" },
    Line { msg: Msg::Notice(Notice::CastShort), en: "the line came down short of the water", simple: "the line did not reach the water", ru: "леска не долетела до воды", pl: "żyłka nie doleciała do wody" },
    Line { msg: Msg::Notice(Notice::TooShallowToFish), en: "too shallow: the float would lie on the bottom", simple: "the water is too shallow to fish", ru: "слишком мелко: поплавок ляжет на дно", pl: "za płytko: spławik legnie na dnie" },
    Line { msg: Msg::Notice(Notice::NoFishHere), en: "no fish live in water this small", simple: "no fish live in water this small", ru: "в такой маленькой воде рыба не живёт", pl: "w tak małej wodzie nie żyją ryby" },
    Line { msg: Msg::Notice(Notice::FirepitWants), en: "a firepit is {0} sticks and {1} log lying on the ground ({2} sticks and {3} logs here)", simple: "a firepit needs {0} sticks and {1} log on the ground (here: {2} sticks, {3} logs)", ru: "костровище -- это палки ({0}) и бревно ({1}) на земле (здесь палок: {2}, брёвен: {3})", pl: "palenisko to patyki ({0}) i kłoda ({1}) na ziemi (tu patyków: {2}, kłód: {3})" },
    Line { msg: Msg::Notice(Notice::NoRaftThere), en: "there is no raft there", simple: "there is no raft there", ru: "там нет плота", pl: "tam nie ma tratwy" },
    Line { msg: Msg::Notice(Notice::SomebodyAtOars), en: "somebody is already at the oars", simple: "somebody else is rowing", ru: "на вёслах уже кто-то есть", pl: "ktoś już siedzi przy wiosłach" },
    Line { msg: Msg::Notice(Notice::PitNeedsFloor), en: "the pit needs solid earth or stone under it", simple: "the pit needs earth or stone under it", ru: "яме нужна твёрдая земля или камень снизу", pl: "dół potrzebuje twardej ziemi lub kamienia pod spodem" },
    Line { msg: Msg::Notice(Notice::PitOpenAtSide), en: "the pit is open at the side: dig it one block deep, with earth or stone on all four sides", simple: "the pit has a hole in its side: dig it one block deep with earth or stone all round", ru: "яма открыта сбоку: выройте её на блок вглубь, с землёй или камнем со всех четырёх сторон", pl: "dół jest otwarty z boku: wykop go na jeden blok, z ziemią lub kamieniem ze wszystkich czterech stron" },
    Line { msg: Msg::Notice(Notice::PitSmothered), en: "something is lying on the pit and smothers the fire", simple: "something on top of the pit puts the fire out", ru: "на яме что-то лежит и душит огонь", pl: "coś leży na dole i dusi ogień" },
    Line { msg: Msg::Notice(Notice::TakePotteryOut), en: "take the fired pottery out of the pit first", simple: "take the finished pots out first", ru: "сначала достаньте обожжённую посуду", pl: "najpierw wyjmij wypaloną ceramikę" },
    Line { msg: Msg::Notice(Notice::PitHoldsFour), en: "a pit kiln holds four pieces of pottery", simple: "a pit kiln fits four pots", ru: "в яму для обжига входят четыре изделия", pl: "w dole do wypalania mieszczą się cztery naczynia" },
    Line { msg: Msg::Notice(Notice::PitFibreFirst), en: "{0} fibre go over the pottery first, then {1} logs, then it is lit", simple: "put {0} fibre on the pots first, then {1} logs, then light it", ru: "сначала на посуду волокно ({0}), потом брёвна ({1}), потом поджечь", pl: "najpierw włókno ({0}) na ceramikę, potem kłody ({1}), potem podpalić" },
    Line { msg: Msg::Notice(Notice::PitFibrePacked), en: "the fibre is packed in: {0} logs go on top now", simple: "the fibre is in: now put {0} logs on top", ru: "волокно уложено: теперь сверху брёвна ({0})", pl: "włókno ułożone: teraz kłody na wierzch ({0})" },
    Line { msg: Msg::Notice(Notice::PitFibreBeforeLogs), en: "the pit needs {0} fibre before the logs go on ({1} now)", simple: "the pit needs {0} fibre before the logs ({1} now)", ru: "до брёвен в яме нужно волокна: {0} (сейчас {1})", pl: "przed kłodami dół potrzebuje włókna: {0} (teraz {1})" },
    Line { msg: Msg::Notice(Notice::PitLitWithFibreAndLogs), en: "a pit kiln is lit with {0} fibre and {1} logs in it ({2} fibre, 0 logs now)", simple: "a pit kiln needs {0} fibre and {1} logs before lighting ({2} fibre, 0 logs now)", ru: "яму поджигают с волокном ({0}) и брёвнами ({1}) (сейчас волокна: {2}, брёвен: 0)", pl: "dół podpala się z włóknem ({0}) i kłodami ({1}) (teraz włókna: {2}, kłód: 0)" },
    Line { msg: Msg::Notice(Notice::PitFull), en: "the pit kiln is full: strike it with flint to light it", simple: "the pit kiln is ready: hit it with flint to light it", ru: "яма для обжига заполнена: чиркните кремнём, чтобы поджечь", pl: "dół do wypalania jest pełny: uderz krzemieniem, by podpalić" },
    Line { msg: Msg::Notice(Notice::PitLitWithLogs), en: "a pit kiln is lit with {0} logs on it ({1} now)", simple: "a pit kiln needs {0} logs on it before lighting ({1} now)", ru: "яму поджигают с брёвнами сверху: {0} (сейчас {1})", pl: "dół podpala się z kłodami na wierzchu: {0} (teraz {1})" },
    Line { msg: Msg::Notice(Notice::PitLogs), en: "{0} of {1} logs on the pit kiln", simple: "{0} of {1} logs on the pit kiln", ru: "брёвен на яме: {0} из {1}", pl: "kłód na dole: {0} z {1}" },
    Line { msg: Msg::Notice(Notice::PitBurning), en: "the pit kiln is burning: {0} minutes until the pottery is fired", simple: "the pit kiln is burning: {0} minutes to go", ru: "яма для обжига горит: до готовности посуды {0} мин", pl: "dół do wypalania się pali: do wypalenia {0} min" },
    Line { msg: Msg::Notice(Notice::RainOnPit), en: "it is raining on the pit: a pit kiln in the open goes out in the rain", simple: "it is raining: a pit kiln without a roof goes out", ru: "на яму идёт дождь: под открытым небом обжиг гаснет", pl: "pada na dół: dół pod gołym niebem gaśnie w deszczu" },
    Line { msg: Msg::Notice(Notice::NothingToFire), en: "there is nothing in the pit to fire", simple: "the pit is empty", ru: "в яме нечего обжигать", pl: "w dole nie ma nic do wypalenia" },
    Line { msg: Msg::Notice(Notice::PitAlight), en: "the pit kiln is alight: in an hour the pottery is fired", simple: "the pit kiln is lit: the pots are done in an hour", ru: "яма для обжига горит: через час посуда будет готова", pl: "dół do wypalania płonie: za godzinę ceramika będzie gotowa" },
    Line { msg: Msg::Notice(Notice::PileNeedsFloor), en: "a log pile needs an empty cell with a floor under it", simple: "a log pile needs an empty space with ground under it", ru: "поленнице нужна пустая клетка с полом снизу", pl: "stos kłód potrzebuje pustej komórki z podłożem pod spodem" },
    Line { msg: Msg::Notice(Notice::PileOpen), en: "the pile is burning open to the air: cover every face with earth or stone, or it burns to ash", simple: "the pile is burning in the open: cover every side with earth or stone, or it turns to ash", ru: "поленница горит на воздухе: закройте каждую грань землёй или камнем, иначе всё сгорит в золу", pl: "stos pali się na powietrzu: zakryj każdą ścianę ziemią lub kamieniem, inaczej spłonie na popiół" },
    Line { msg: Msg::Notice(Notice::CharcoalBurning), en: "the charcoal pit is burning: {0} minutes to go", simple: "the charcoal pit is burning: {0} minutes to go", ru: "углежогная яма горит: ещё {0} мин", pl: "mielerz się pali: jeszcze {0} min" },
    Line { msg: Msg::Notice(Notice::PileHolds), en: "a log pile holds {0} logs", simple: "a log pile fits {0} logs", ru: "в поленницу входит брёвен: {0}", pl: "stos mieści kłód: {0}" },
    Line { msg: Msg::Notice(Notice::PileAlight), en: "the log pile is alight: cover it within {0} seconds, every face, or it burns to ash", simple: "the log pile is lit: cover every side within {0} seconds, or it turns to ash", ru: "поленница загорелась: закройте все грани за {0} с, иначе всё сгорит в золу", pl: "stos się zapalił: zakryj każdą ścianę w ciągu {0} s, inaczej spłonie na popiół" },
    Line { msg: Msg::Notice(Notice::RichSoilWatered), en: "this is rich soil, and water is close by", simple: "good soil, and water is near", ru: "земля здесь жирная, и вода рядом", pl: "ziemia jest tu żyzna, a woda blisko" },
    Line { msg: Msg::Notice(Notice::RichSoilDry), en: "this is rich soil, but there is no water close by", simple: "good soil, but no water near", ru: "земля здесь жирная, но воды рядом нет", pl: "ziemia jest tu żyzna, ale w pobliżu nie ma wody" },
    Line { msg: Msg::Notice(Notice::ThinSoilWatered), en: "this soil is thin, but water is close by", simple: "poor soil, but water is near", ru: "земля здесь тощая, но вода рядом", pl: "ziemia jest tu jałowa, ale woda blisko" },
    Line { msg: Msg::Notice(Notice::ThinSoilDry), en: "this soil is thin, and there is no water close by", simple: "poor soil, and no water near", ru: "земля здесь тощая, и воды рядом нет", pl: "ziemia jest tu jałowa i w pobliżu nie ma wody" },
    Line { msg: Msg::Notice(Notice::SoilWatered), en: "water is close by", simple: "water is near", ru: "вода рядом", pl: "woda jest blisko" },
    Line { msg: Msg::Notice(Notice::SoilDry), en: "there is no water close by", simple: "no water near", ru: "воды рядом нет", pl: "w pobliżu nie ma wody" },
];

/// A block's or a recipe's name as a person reads it: `copper_ingot` as
/// "copper ingot".
///
/// **Only the underscores, and only on the screen.** The name is still the
/// identifier -- `blocks.toml`, the save file and `/give` all spell it with
/// underscores, and the give menu still sends it that way. This was the
/// first half of "убери из названий предметов _"; the translation is the
/// second, in `ui::names`, and this is what `names` falls back to for a
/// thing its table has no row for yet.
pub fn readable(name: &str) -> String {
    name.replace('_', " ")
}

/// A word that follows a number, in the form that number puts it in.
///
/// **English has two forms and hides one of them.** "chunks" is right
/// after every number but one, so a row that writes `{n} {word}` is
/// correct in English by luck and wrong in Russian and Polish for a
/// quarter of the numbers a player can dial in. The render distance row
/// said "2 чанков" and "3 чанков", the shadow row said "1 блоков", and a
/// chest with three things in it said "3 предметов" -- the genitive
/// plural stuck on everything, which is what a machine does and what a
/// person notices immediately.
///
/// Three forms a language, because that is how many the two Slavic
/// languages here need, and a fourth column for a language that needs
/// four can be added the day one arrives. English and simple English
/// take their one word from `STRINGS` as they always did: a row here is
/// not needed unless a language declines.
pub struct Counted {
    pub msg: Msg,
    /// 1, then 2--4, then the rest: `1 чанк`, `2 чанка`, `5 чанков`.
    pub ru: [&'static str; 3],
    /// The same three slots, filled by Polish's own rule: `1 chunk`,
    /// `2 chunki`, `5 chunków`.
    pub pl: [&'static str; 3],
}

/// Every word this game ever prints a number in front of.
#[rustfmt::skip]
pub const COUNTED: &[Counted] = &[
    Counted { msg: Msg::Chunks,    ru: ["чанк", "чанка", "чанков"],             pl: ["chunk", "chunki", "chunków"] },
    Counted { msg: Msg::Blocks,    ru: ["блок", "блока", "блоков"],             pl: ["blok", "bloki", "bloków"] },
    Counted { msg: Msg::ItemsWord, ru: ["предмет", "предмета", "предметов"],    pl: ["przedmiot", "przedmioty", "przedmiotów"] },
];

/// Which of the three forms `n` calls for.
///
/// Russian and Polish agree about 2--4 and about the teens being an
/// exception to it, and disagree about 21: Russian says "21 чанк" and
/// Polish says "21 chunków". That one difference is the whole reason
/// this is not one rule.
fn form_of(language: Language, n: u64) -> usize {
    let last_two = n % 100;
    let last = n % 10;
    let few = (2..=4).contains(&last) && !(12..=14).contains(&last_two);
    match language {
        Language::Russian if last == 1 && last_two != 11 => 0,
        Language::Polish if n == 1 => 0,
        _ if few => 1,
        _ => 2,
    }
}

/// `msg` in the form `n` of it takes, for a row that prints `{n} {word}`.
///
/// Falls back to the plain row for a language with no forms and for a
/// `Msg` with no row here, so a call site can use this for every counted
/// word and a word that turns out not to decline costs nothing.
pub fn counted(language: Language, n: u64, msg: Msg) -> &'static str {
    let Some(row) = COUNTED.iter().find(|row| row.msg == msg) else {
        return language.text(msg);
    };
    let forms = match language {
        Language::Russian => &row.ru,
        Language::Polish => &row.pl,
        Language::English | Language::SimpleEnglish => return language.text(msg),
    };
    forms[form_of(language, n)]
}

/// A notice with its numbers put in: the row's `{0}`, `{1}` replaced in
/// order by what the server counted (`notice::Said`).
///
/// **Positional rather than one `{}` after another**, because the four
/// languages do not put the numbers in the same order -- "3 of 6 logs" is
/// "брёвен на яме: 3 из 6" -- and a row has to be free to say them where its
/// grammar wants them. The rows name the noun around a number rather than
/// declining it ("брёвен: 4"), which is what lets one row serve every count.
pub fn said(language: Language, said: &primitive_shared::notice::Said) -> String {
    let mut text = language.text(Msg::Notice(said.what)).to_string();
    for (index, number) in said.numbers.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), &number.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A number the English row says is a number every row says**, in
    /// the same slots. A translation that dropped `{1}` would tell a Polish
    /// player the pit wants logs without saying how many; one that added a
    /// `{2}` the server never sends would print the braces.
    #[test]
    fn every_language_says_the_same_numbers_in_a_notice() {
        let slots = |text: &str| (0..8).filter(|i| text.contains(&format!("{{{i}}}"))).collect::<Vec<_>>();
        for &notice in Notice::ALL {
            let english = slots(Language::English.text(Msg::Notice(notice)));
            for &language in Language::ALL {
                let text = language.text(Msg::Notice(notice));
                assert_eq!(slots(text), english, "{language:?} says other numbers than English in {notice:?}: {text}");
            }
            // ...and filled, nothing of the braces is left.
            let filled = said(Language::Russian, &primitive_shared::notice::Said::new(notice, &[7; 8]));
            assert!(!filled.contains('{'), "{notice:?} left a placeholder: {filled}");
        }
    }

    /// Every character the interface can print has to have a glyph, or
    /// the word comes out as a row of boxes. This is the check that the
    /// font and the translations were extended together.
    #[test]
    fn every_translated_character_has_a_glyph() {
        for line in STRINGS {
            for (language, text) in [
                ("en", line.en),
                ("simple", line.simple),
                ("ru", line.ru),
                ("pl", line.pl),
            ] {
                for c in text.chars() {
                    assert!(
                        crate::engine::texture::GLYPHS.contains(c),
                        "{language} {:?} needs a glyph for {c:?}, which the font has not got",
                        text
                    );
                }
            }
        }
        for language in Language::ALL {
            for c in language.name().chars() {
                assert!(
                    crate::engine::texture::GLYPHS.contains(c),
                    "the name of {language:?} needs a glyph for {c:?}"
                );
            }
        }
    }

    #[test]
    fn every_message_has_a_row_in_every_language() {
        for line in STRINGS {
            for language in Language::ALL {
                let text = language.text(line.msg);
                assert!(!text.is_empty(), "{:?} is empty in {language:?}", line.msg);
                assert_ne!(text, "???", "{:?} has no row", line.msg);
            }
        }
    }

    /// **Every notice the server can send has its words**, in every
    /// language: a notice with no row is `???` in the banner, and the server
    /// sends codes now, not English (`notice`).
    #[test]
    fn every_notice_the_server_sends_has_words_in_every_language() {
        for &notice in Notice::ALL {
            for language in Language::ALL {
                assert_ne!(language.text(Msg::Notice(notice)), "???", "{notice:?} has no words in {language:?}");
            }
        }
    }

    #[test]
    fn no_message_is_listed_twice() {
        // A duplicate row makes the second one dead, and the two say
        // different things by the time anybody notices.
        for (i, line) in STRINGS.iter().enumerate() {
            assert!(
                !STRINGS[..i].iter().any(|earlier| earlier.msg == line.msg),
                "{:?} appears twice",
                line.msg
            );
        }
    }

    /// The numbers a settings row can actually be dialled to, and the
    /// three that used to come out wrong in both Slavic languages.
    #[test]
    fn counting_in_russian_and_polish_uses_the_form_the_number_calls_for() {
        use Language::{English, Polish, Russian};
        for (n, ru, pl) in [
            (1_u64, "чанк", "chunk"),
            (2, "чанка", "chunki"),
            (4, "чанка", "chunki"),
            (5, "чанков", "chunków"),
            // The teens are the exception both languages make.
            (11, "чанков", "chunków"),
            (12, "чанков", "chunków"),
            (14, "чанков", "chunków"),
            (15, "чанков", "chunków"),
            // ...and 21 is where the two rules part company.
            (21, "чанк", "chunków"),
            (22, "чанка", "chunki"),
            (25, "чанков", "chunków"),
            (100, "чанков", "chunków"),
        ] {
            assert_eq!(counted(Russian, n, Msg::Chunks), ru, "{n} in Russian");
            assert_eq!(counted(Polish, n, Msg::Chunks), pl, "{n} in Polish");
        }
        // English takes the row it always took, and a word with no forms
        // of its own falls back to the row as well.
        assert_eq!(counted(English, 1, Msg::Chunks), English.text(Msg::Chunks));
        assert_eq!(counted(Russian, 3, Msg::Paused), Russian.text(Msg::Paused));
    }

    /// Every form a counted word has is a word the font can draw. The
    /// glyph test above walks `STRINGS`, which these are not in.
    #[test]
    fn every_form_of_a_counted_word_has_a_glyph() {
        for row in COUNTED {
            for form in row.ru.iter().chain(row.pl.iter()) {
                for c in form.chars() {
                    assert!(
                        crate::engine::texture::GLYPHS.contains(c),
                        "{form:?} needs a glyph for {c:?}, which the font has not got"
                    );
                }
            }
        }
    }

    #[test]
    fn stepping_through_the_languages_wraps_both_ways() {
        let first = Language::ALL[0];
        let last = Language::ALL[Language::ALL.len() - 1];
        assert_eq!(first.step(-1), last);
        assert_eq!(last.step(1), first);
        // ...and every language is reachable from every other.
        let mut at = first;
        for _ in 0..Language::ALL.len() {
            at = at.step(1);
        }
        assert_eq!(at, first, "stepping all the way round should come home");
    }

    #[test]
    fn a_language_names_itself_in_itself() {
        // A list that says "Russian" to somebody who does not read
        // English is a list they cannot use.
        assert_eq!(Language::Russian.name(), "РУССКИЙ");
        assert_eq!(Language::Polish.name(), "POLSKI");
    }
}
