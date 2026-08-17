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
//! Block and biome names. Those come from `primitive_shared`, are used
//! as identifiers in `blocks.toml` and in save files, and translating
//! them would mean either translating a file format or keeping a second
//! name per block. Worth doing; not the same job as this.

use serde::{Deserialize, Serialize};

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
    DeleteThisWorld,
    SeedHelp,
    WorldFormHelp,
    NeverPlayed,
    JustNow,
    MinutesAgo,
    HoursAgo,
    DaysAgo,
    // ---- confirmation ----
    CannotBeUndone,
    ConfirmHelp,
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
    CannotConnect,
    Retry,
    AddressRequired,
    // ---- settings ----
    LanguageRow,
    RenderDistance,
    FieldOfView,
    Fog,
    MouseSensitivity,
    Sound,
    On,
    Off,
    Apply,
    Vsync,
    AmbientOcclusion,
    Anisotropy,
    TransparentLeaves,
    DetailDistance,
    Cloudiness,
    LocalViewDistance,
    MenuBackground,
    MenuBackgroundBlock,
    Chunks,
    Degrees,
    Toggle,
    Controls,
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
    DropItem,
    ToggleFog,
    ToggleStats,
    Fullscreen,
    // ---- credits ----
    RoleTextures,
    RoleCode,
    RoleEngine,
    // ---- in play ----
    Inventory,
    Crafting,
    Paused,
    Resume,
    Respawn,
    YouDied,
    LeaveWorld,
    QuitToMenu,
    DeathHelp,
    // ---- inventory screen ----
    Belt,
    TidyPile,
    NoRoom,
    Need,
    No,
    Of,
    KgCarried,
    Speed,
    // ---- chest screen ----
    Chest,
    /// The same screen over a dead player's pack. A separate word
    /// because the two blocks mean opposite things -- one is where you
    /// chose to put something, the other is where you lost it -- and the
    /// heading is the only thing on the screen that says which.
    Backpack,
    Stored,
    Carried,
    StoreAll,
    TakeAll,
    SlotsWord,
    ItemsWord,
    ChestHint1,
    ChestHint2,
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
    Line { msg: Msg::Seed,         en: "SEED",         simple: "WORLD NUMBER", ru: "ЗЕРНО", pl: "ZIARNO" },
    Line { msg: Msg::Name,         en: "NAME",         simple: "NAME",       ru: "ИМЯ", pl: "NAZWA" },
    Line { msg: Msg::Create,       en: "CREATE",       simple: "MAKE IT",    ru: "СОЗДАТЬ", pl: "UTWÓRZ" },
    Line { msg: Msg::Cancel,       en: "CANCEL",       simple: "NEVER MIND", ru: "ОТМЕНА", pl: "ANULUJ" },
    Line { msg: Msg::DeleteThisWorld, en: "DELETE THIS WORLD?", simple: "THROW THIS WORLD AWAY?", ru: "УДАЛИТЬ ЭТОТ МИР?", pl: "USUNĄĆ TEN ŚWIAT?" },
    Line { msg: Msg::SeedHelp,     en: "the seed decides the terrain -- leave it for the default", simple: "this number shapes the land -- leave it if unsure", ru: "зерно определяет рельеф -- можно оставить как есть", pl: "ziarno decyduje o terenie -- zostaw dla domyślnego" },
    Line { msg: Msg::WorldFormHelp, en: "tab switches field   enter creates   esc cancels", simple: "tab moves   enter makes it   esc goes back", ru: "tab переключает поле   ввод создаёт   esc отменяет", pl: "tab zmienia pole   enter tworzy   esc anuluje" },
    Line { msg: Msg::NeverPlayed,  en: "never played",  simple: "never played", ru: "не играли", pl: "nigdy nie grano" },
    Line { msg: Msg::JustNow,      en: "just now",      simple: "just now",  ru: "только что", pl: "przed chwilą" },
    Line { msg: Msg::MinutesAgo,   en: "min ago",       simple: "min ago",   ru: "мин назад", pl: "min temu" },
    Line { msg: Msg::HoursAgo,     en: "h ago",         simple: "h ago",     ru: "ч назад", pl: "godz. temu" },
    Line { msg: Msg::DaysAgo,      en: "d ago",         simple: "d ago",     ru: "д назад", pl: "dni temu" },

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
    Line { msg: Msg::Sound,        en: "SOUND",        simple: "SOUND",      ru: "ЗВУК", pl: "DŹWIĘK" },
    Line { msg: Msg::On,           en: "ON",           simple: "YES",        ru: "ВКЛ", pl: "WŁ" },
    Line { msg: Msg::Off,          en: "OFF",          simple: "NO",         ru: "ВЫКЛ", pl: "WYŁ" },
    Line { msg: Msg::Apply,        en: "APPLY",        simple: "USE THESE",  ru: "ПРИМЕНИТЬ", pl: "ZASTOSUJ" },
    Line { msg: Msg::Vsync,        en: "VSYNC",        simple: "SMOOTH FRAMES", ru: "ВЕРТ. СИНХРОНИЗАЦИЯ", pl: "SYNCHRONIZACJA PIONOWA" },
    Line { msg: Msg::AmbientOcclusion, en: "AMBIENT OCCLUSION", simple: "SOFT CORNER SHADOWS", ru: "ЗАТЕНЕНИЕ УГЛОВ", pl: "OKLUZJA OTOCZENIA" },
    Line { msg: Msg::Anisotropy,   en: "ANISOTROPIC FILTERING", simple: "SHARPER GROUND TEXTURES", ru: "АНИЗОТРОПНАЯ ФИЛЬТРАЦИЯ", pl: "FILTROWANIE ANIZOTROPOWE" },
    Line { msg: Msg::TransparentLeaves, en: "TRANSPARENT LEAVES", simple: "SEE-THROUGH LEAVES", ru: "ПРОЗРАЧНАЯ ЛИСТВА", pl: "PRZEZROCZYSTE LIŚCIE" },
    Line { msg: Msg::DetailDistance, en: "GRASS & STONE DISTANCE", simple: "HOW FAR DETAILS SHOW", ru: "ДАЛЬНОСТЬ ТРАВЫ И КАМНЕЙ", pl: "ZASIĘG TRAWY I KAMIENI" },
    Line { msg: Msg::Cloudiness,   en: "CLOUD COVER",  simple: "HOW CLOUDY IT IS", ru: "ОБЛАЧНОСТЬ", pl: "ZACHMURZENIE" },
    Line { msg: Msg::LocalViewDistance, en: "LOCAL WORLD DISTANCE", simple: "HOW FAR YOUR WORLD LOADS", ru: "ДАЛЬНОСТЬ В ОДИНОЧНОЙ ИГРЕ", pl: "ZASIĘG W GRZE LOKALNEJ" },
    Line { msg: Msg::MenuBackground, en: "MENU BACKGROUND", simple: "PICTURE BEHIND MENUS", ru: "ФОН МЕНЮ", pl: "TŁO MENU" },
    Line { msg: Msg::MenuBackgroundBlock, en: "BACKGROUND BLOCK", simple: "WHICH BLOCK TO SHOW", ru: "БЛОК ФОНА", pl: "BLOK TŁA" },
    Line { msg: Msg::Chunks,       en: "chunks",       simple: "chunks",     ru: "чанков", pl: "chunków" },
    Line { msg: Msg::Degrees,      en: "deg",          simple: "deg",        ru: "град", pl: "stopni" },
    Line { msg: Msg::Toggle,       en: "TOGGLE",       simple: "ON/OFF",     ru: "ВКЛ/ВЫКЛ", pl: "PRZEŁĄCZ" },
    Line { msg: Msg::Controls,     en: "CONTROLS",     simple: "KEYS",       ru: "УПРАВЛЕНИЕ", pl: "STEROWANIE" },
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
    Line { msg: Msg::DropItem,     en: "DROP ITEM",    simple: "THROW OUT",  ru: "ВЫБРОСИТЬ", pl: "WYRZUĆ" },
    Line { msg: Msg::ToggleFog,    en: "TOGGLE FOG",   simple: "HAZE ON/OFF", ru: "ТУМАН ВКЛ/ВЫКЛ", pl: "MGŁA WŁ/WYŁ" },
    Line { msg: Msg::ToggleStats,  en: "TOGGLE STATS", simple: "NUMBERS ON/OFF", ru: "СТАТИСТИКА", pl: "STATYSTYKI" },
    Line { msg: Msg::Fullscreen,   en: "FULLSCREEN",   simple: "WHOLE SCREEN", ru: "ПОЛНЫЙ ЭКРАН", pl: "PEŁNY EKRAN" },

    Line { msg: Msg::RoleTextures, en: "TEXTURES",     simple: "PICTURES",   ru: "ТЕКСТУРЫ", pl: "TEKSTURY" },
    Line { msg: Msg::RoleCode,     en: "CODE",         simple: "CODE",       ru: "КОД", pl: "KOD" },
    Line { msg: Msg::RoleEngine,   en: "ENGINE",       simple: "BUILT WITH", ru: "ДВИЖОК", pl: "SILNIK" },

    Line { msg: Msg::Inventory,    en: "INVENTORY",    simple: "WHAT YOU CARRY", ru: "РЮКЗАК", pl: "EKWIPUNEK" },
    Line { msg: Msg::Crafting,     en: "CRAFTING",     simple: "MAKING THINGS", ru: "ВЕРСТАК", pl: "WYTWARZANIE" },
    Line { msg: Msg::Paused,       en: "PAUSED",       simple: "STOPPED",    ru: "ПАУЗА", pl: "PAUZA" },
    Line { msg: Msg::Resume,       en: "RESUME",       simple: "CARRY ON",   ru: "ПРОДОЛЖИТЬ", pl: "WRÓĆ DO GRY" },
    Line { msg: Msg::Respawn,      en: "RESPAWN",      simple: "START AGAIN", ru: "ВОЗРОДИТЬСЯ", pl: "ODRODŹ SIĘ" },
    Line { msg: Msg::YouDied,      en: "YOU DIED",     simple: "YOU DIED",   ru: "ВЫ ПОГИБЛИ", pl: "ZGINĄŁEŚ" },
    Line { msg: Msg::LeaveWorld,   en: "LEAVE WORLD",  simple: "LEAVE THE WORLD", ru: "ПОКИНУТЬ МИР", pl: "OPUŚĆ ŚWIAT" },
    Line { msg: Msg::QuitToMenu,   en: "QUIT TO MENU", simple: "BACK TO MENU", ru: "ВЫЙТИ В МЕНЮ", pl: "WYJDŹ DO MENU" },
    Line { msg: Msg::DeathHelp,    en: "R RESPAWN    ESC MENU", simple: "R START AGAIN    ESC MENU", ru: "R ВОЗРОДИТЬСЯ    ESC МЕНЮ", pl: "R ODRODŹ SIĘ    ESC MENU" },

    Line { msg: Msg::Belt,         en: "BELT",         simple: "BELT",       ru: "ПОЯС", pl: "PAS" },
    Line { msg: Msg::TidyPile,     en: "TIDY PILE",    simple: "TIDY UP",    ru: "ПРИБРАТЬ", pl: "SPRZĄTNIJ" },
    Line { msg: Msg::NoRoom,       en: "no room",      simple: "no room",    ru: "нет места", pl: "brak miejsca" },
    Line { msg: Msg::Need,         en: "need",         simple: "need",       ru: "нужно", pl: "potrzeba" },
    Line { msg: Msg::No,           en: "no",           simple: "no",         ru: "нет", pl: "nie" },
    Line { msg: Msg::Of,           en: "OF",           simple: "OF",         ru: "ИЗ", pl: "Z" },
    Line { msg: Msg::KgCarried,    en: "kg carried",   simple: "kg carried", ru: "кг при себе", pl: "kg przy sobie" },
    Line { msg: Msg::Speed,        en: "speed",        simple: "speed",      ru: "скорость", pl: "szybkość" },

    Line { msg: Msg::Chest,        en: "CHEST",        simple: "STORAGE BOX", ru: "СУНДУК", pl: "SKRZYNIA" },
    // Not "РЮКЗАК" in Russian and not "PLECAK" in Polish: both are
    // already the word this game uses for the pack you are carrying (see
    // Msg::Inventory and Msg::Pack), and a heading that says the same
    // thing as the grid under it says nothing.
    Line { msg: Msg::Backpack,     en: "BACKPACK",     simple: "WHAT THEY LEFT", ru: "ПОЖИТКИ", pl: "SAKWA" },
    Line { msg: Msg::Stored,       en: "STORED",       simple: "IN THE BOX", ru: "В СУНДУКЕ", pl: "W SKRZYNI" },
    Line { msg: Msg::Carried,      en: "CARRIED",      simple: "ON YOU",     ru: "ПРИ СЕБЕ", pl: "PRZY SOBIE" },
    Line { msg: Msg::StoreAll,     en: "STORE ALL ^",  simple: "PUT IN ^",   ru: "СЛОЖИТЬ ^", pl: "SCHOWAJ ^" },
    Line { msg: Msg::TakeAll,      en: "v TAKE ALL",   simple: "v TAKE OUT", ru: "v ЗАБРАТЬ", pl: "v ZABIERZ" },
    Line { msg: Msg::SlotsWord,    en: "slots",        simple: "spaces",     ru: "ячеек", pl: "pól" },
    Line { msg: Msg::ItemsWord,    en: "items",        simple: "things",     ru: "предметов", pl: "przedmiotów" },
    Line { msg: Msg::ChestHint1,   en: "left click takes and places  |  right click halves", simple: "left click takes and puts  |  right click halves", ru: "ЛКМ берёт и кладёт  |  ПКМ кладёт половину", pl: "LPM bierze i kładzie  |  PPM kładzie połowę" },
    Line { msg: Msg::ChestHint2,   en: "shift click sends it across  |  esc closes", simple: "shift click sends it over  |  esc closes", ru: "shift+клик шлёт на другую сторону  |  esc выход", pl: "shift+klik śle na drugą stronę  |  esc zamyka" },
];

#[cfg(test)]
mod tests {
    use super::*;

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
