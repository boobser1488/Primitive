//! What a block, an item or a recipe is called, in the player's language.
//!
//! ## Why this exists
//!
//! "убери из названий предметов _, переведи предметы и вообще все в игре
//! на разные языки". The underscores went first (`lang::readable`), and
//! the tooltip then read "flint knife" to a player who had chosen
//! Russian for everything else on the screen -- the one English word in
//! a Russian sentence, and the word that mattered most in it.
//!
//! ## The name stays the identifier
//!
//! `copper_ingot` is what `blocks.toml`, the save file, the wire, `/give`
//! and every mod spell, and a recipe's `name` is what the server and the
//! mod API look a recipe up by. None of that changes. **A translation is
//! only ever drawn**, never sent and never stored: the give menu still
//! asks for `copper_ingot`, and a player who types `/give медный_слиток`
//! is told there is no such block, exactly as before. Search is the one
//! place a translation is read back, and it matches every language and
//! the identifier at once (see [`found`]).
//!
//! ## The shape
//!
//! Two tables, `BLOCKS` and `RECIPES`, one row per identifier with the
//! four languages side by side -- `lang::STRINGS`'s shape, for
//! `lang`'s reason: a name changed in one language is on the same line
//! as the three it now disagrees with. A row that is short a language
//! does not compile, and a row that is *missing* fails
//! `every_block_has_a_name_in_every_language`, which walks
//! `ALL_BLOCK_IDS` rather than this table -- so a block somebody adds in
//! `primitive_shared` turns this test red on the day it is added, not
//! the day a Polish player hovers it.
//!
//! Keyed by the identifier string rather than by `BlockId`. The string
//! is the thing that never changes (it is the save format), where a
//! numeric id is an implementation detail of the same row; and it lets
//! the recipe table use the same row type, since a recipe has no id but
//! its name.
//!
//! A missing row draws the readable identifier instead of panicking. The
//! test is where a hole is caught; a player on a build with one should
//! see "wild hive", not a crash.
//!
//! ## What was rejected
//!
//! * **A field on `blocks::BlockDef`.** One place per block is the
//!   argument `blocks` makes for everything else, and it is right about
//!   *rules*. A name in Polish is not a rule: it would put four
//!   languages into the crate the server compiles and never draws, and
//!   make adding a fifth language an edit to every one of three hundred
//!   rows in a file that is about hardness and light.
//! * **`Msg` variants.** The block names would be three hundred variants
//!   mixed in with "PAUSED", each spelled once as a variant and again as
//!   a mapping from `BlockId`, and that mapping is the second table that
//!   drifts from `ALL_BLOCK_IDS` which this module exists to avoid.
//! * **A file under `assets/`** (toml or po). It is what a translator
//!   outside the repository would want, and nobody is outside the
//!   repository. It would be read at start-up -- through
//!   `AAssetManager` on a phone -- so a missing language on a row is a
//!   runtime blank instead of a compile error, and the glyph test would
//!   have to load the file to check it. If translation ever leaves the
//!   people who write the code, this is the version to build, and these
//!   tables convert to it mechanically.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use primitive_shared::types::{block_name, BlockId};

use crate::ui::lang::Language;

/// One thing's name in the four languages. See `lang::Line` for what
/// the columns are.
pub struct Name {
    /// The identifier: `types::block_name` for a block, `Recipe::name`
    /// for a recipe.
    pub id: &'static str,
    pub en: &'static str,
    pub simple: &'static str,
    pub ru: &'static str,
    pub pl: &'static str,
}

impl Name {
    pub fn in_language(&self, language: Language) -> &'static str {
        match language {
            Language::English => self.en,
            Language::SimpleEnglish => self.simple,
            Language::Russian => self.ru,
            Language::Polish => self.pl,
        }
    }

    fn all(&self) -> [&'static str; 4] {
        [self.en, self.simple, self.ru, self.pl]
    }
}

/// A table by identifier, built once. A linear search would be three
/// hundred string comparisons for every cell of a give menu that draws
/// sixty of them a frame.
fn index(table: &'static [Name], cell: &'static OnceLock<HashMap<&'static str, &'static Name>>, id: &str) -> Option<&'static Name> {
    cell.get_or_init(|| table.iter().map(|row| (row.id, row)).collect()).get(id).copied()
}

fn block_row(id: &str) -> Option<&'static Name> {
    static CELL: OnceLock<HashMap<&'static str, &'static Name>> = OnceLock::new();
    index(BLOCKS, &CELL, id)
}

fn recipe_row(id: &str) -> Option<&'static Name> {
    static CELL: OnceLock<HashMap<&'static str, &'static Name>> = OnceLock::new();
    index(RECIPES, &CELL, id)
}

fn or_readable(row: Option<&'static Name>, id: &str, language: Language) -> Cow<'static, str> {
    match row {
        Some(row) => Cow::Borrowed(row.in_language(language)),
        None => Cow::Owned(crate::ui::lang::readable(id)),
    }
}

/// What `block` is called in `language`. Variants share their kind's
/// name, as `types::block_name` has them.
pub fn block(block: BlockId, language: Language) -> Cow<'static, str> {
    identified(block_name(block), language)
}

/// The same, from the identifier -- for the screens that hold a name
/// rather than an id (the give menu's rows and its "given" line).
pub fn identified(id: &str, language: Language) -> Cow<'static, str> {
    or_readable(block_row(id), id, language)
}

/// What the recipe named `id` (`Recipe::name`) is called in `language`.
pub fn recipe(id: &str, language: Language) -> Cow<'static, str> {
    or_readable(recipe_row(id), id, language)
}

/// Whether a search for `query` (already trimmed and lower-cased) finds
/// the block `id`.
///
/// **In every language, not only the chosen one.** A player who reads
/// Russian and remembers "flint" from a video, or who switched language
/// last week, is still looking for the same thing, and there is no
/// search a player means that a hit in another language gets wrong.
/// The identifier and its readable form stay searchable, because that
/// is what `/give` takes and what the manual quotes.
///
/// **From the start of a word, not anywhere inside one.** Four languages
/// of names are four times as many letters for a query to land in by
/// accident, and the first one found was the recipe book's own promise:
/// "tin" found bronze -- the one recipe whose tin it must not give away --
/// through the crucible, which Simple English calls a "melting pot".
/// Nobody searching means the middle of a word, and "нож" still finds
/// "Кремнёвый нож" because the knife is a word of its own there.
pub fn block_found(id: &str, query: &str) -> bool {
    found(id, block_row(id), query)
}

/// [`block_found`] for a recipe.
pub fn recipe_found(id: &str, query: &str) -> bool {
    found(id, recipe_row(id), query)
}

fn found(id: &str, row: Option<&'static Name>, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let id = id.to_lowercase();
    starts_a_word(&id, query)
        || starts_a_word(&crate::ui::lang::readable(&id), query)
        || row.is_some_and(|row| row.all().iter().any(|name| starts_a_word(&name.to_lowercase(), query)))
}

/// Whether `query` occurs in `text` beginning where a word does.
fn starts_a_word(text: &str, query: &str) -> bool {
    text.match_indices(query)
        .any(|(at, _)| !text[..at].chars().next_back().is_some_and(char::is_alphanumeric))
}

/// What the tooltip adds in brackets after a name -- which water is in
/// a jug, how blunt a blade is -- in `language`.
///
/// `types::water_label` and `tools::label` are the rules' answer, in
/// English; this translates that answer rather than asking the rules
/// again, so what is *true* about the stack is still decided in one
/// place and the client only chooses the words. A label this table does
/// not know is printed as the rules gave it, and
/// `every_condition_the_rules_can_print_is_translated` is what stops
/// that happening.
pub fn condition(block: BlockId, language: Language) -> Option<Cow<'static, str>> {
    let label = rules_label(block)?;
    Some(match CONDITIONS.iter().find(|row| row.id == label) {
        Some(row) => Cow::Borrowed(row.in_language(language)),
        None => Cow::Borrowed(label),
    })
}

/// Every rule that has a word for the state of a stack, asked in turn:
/// which water, how blunt, how dry the clay (`clay::label`), how green the
/// wood (`wood::seasoning_label`). At most one of them answers for any
/// block, since no block is two of those things.
fn rules_label(block: BlockId) -> Option<&'static str> {
    primitive_shared::types::water_label(block)
        .or_else(|| primitive_shared::tools::label(block))
        .or_else(|| primitive_shared::clay::label(block))
        .or_else(|| primitive_shared::wood::seasoning_label(block))
}

/// The tooltip's first line for a stack: name, condition, count, weight.
/// One function because the pack and the chest print the same line, and
/// used to print it from two copies of one `format!`.
pub fn stack_line(block: BlockId, count: u32, language: Language) -> String {
    format!(
        "{}{} x{count}   {:.0} kg",
        self::block(block, language),
        condition(block, language).map(|w| format!(" ({w})")).unwrap_or_default(),
        primitive_shared::types::block_weight(block) * count as f32,
    )
}

/// What an animal is called in `language`: "trout", "форель", "pstrąg".
///
/// **A third table, keyed by `Species::name`**, for the reason the other two
/// are keyed by their identifiers: `Species::name` is what the wire, the sound
/// bank (`wild.deer.call`) and `/stats` spell, and it does not change. Until
/// this nothing in the interface named a live animal at all -- the one place a
/// player met one by name was an English death message -- and the first thing
/// that needed to was the fishing line, which lands one of five fish that all
/// used to arrive as "raw fish".
///
/// A missing row is the readable identifier, as for a block; the test
/// `every_species_has_a_name_in_every_language` walks `Species::ALL`, so a new
/// animal is caught the day it is added.
pub fn animal(species: primitive_shared::animals::Species, language: Language) -> Cow<'static, str> {
    or_readable(ANIMALS.iter().find(|row| row.id == species.name()), species.name(), language)
}

/// What the young of `species` is called -- "fawn", "ягнёнок" -- or the adult's
/// name for a species whose young have no word of their own.
///
/// Nothing on screen names a *live* animal yet -- a catch and a death are the
/// two places an animal is named, and neither is ever young -- so only the
/// tests ask. It is here, with its rows, so that the first thing that does
/// name the animal in front of you calls a fawn a fawn in all four languages
/// rather than a deer; `every_species_has_a_name_in_every_language` keeps the
/// rows honest until then.
#[allow(dead_code)]
pub fn young(species: primitive_shared::animals::Species, language: Language) -> Cow<'static, str> {
    match primitive_shared::youth::young_name(species) {
        Some(id) => or_readable(ANIMALS.iter().find(|row| row.id == id), id, language),
        None => animal(species, language),
    }
}

/// What killed them, in their language, when an animal did.
///
/// **The server's cause is English and stays English** -- it is the one side
/// that knows why somebody died, and it does not know what language they read
/// (`ServerMessage::Died`). An animal's cause is written by the species
/// (`Species::death_cause`), so the client can find the species that wrote it
/// and say it again from `DEATHS`, which names the animal in the player's own
/// word for it. Anything else -- a fall, a drowning, another player -- is
/// printed as the server sent it, which is what it always was.
pub fn death_cause(cause: &str, language: Language) -> Cow<'_, str> {
    let Some(species) = primitive_shared::animals::Species::of_death_cause(cause) else {
        return Cow::Borrowed(cause);
    };
    match DEATHS.iter().find(|row| row.id == species.name()) {
        Some(row) => Cow::Borrowed(row.in_language(language)),
        None => Cow::Borrowed(cause),
    }
}

/// Every animal, and the young of those that have a word for them. Keyed by
/// `Species::name` and `youth::young_name`.
///
/// Russian as a Russian player would say it standing in a field: "олень" and
/// not "благородный олень"; the fowl is a grouse, as the manual calls it.
#[rustfmt::skip]
const ANIMALS: &[Name] = &[
    Name { id: "hare", en: "hare", simple: "hare", ru: "заяц", pl: "zając" },
    Name { id: "deer", en: "deer", simple: "deer", ru: "олень", pl: "jeleń" },
    Name { id: "boar", en: "boar", simple: "wild pig", ru: "кабан", pl: "dzik" },
    Name { id: "wolf", en: "wolf", simple: "wolf", ru: "волк", pl: "wilk" },
    Name { id: "sheep", en: "sheep", simple: "sheep", ru: "овца", pl: "owca" },
    Name { id: "bear", en: "bear", simple: "bear", ru: "медведь", pl: "niedźwiedź" },
    Name { id: "fowl", en: "grouse", simple: "wild bird", ru: "тетерев", pl: "cietrzew" },
    Name { id: "zebra", en: "zebra", simple: "zebra", ru: "зебра", pl: "zebra" },
    Name { id: "antelope", en: "antelope", simple: "antelope", ru: "антилопа", pl: "antylopa" },
    Name { id: "lion", en: "lion", simple: "lion", ru: "лев", pl: "lew" },
    Name { id: "fish", en: "fish", simple: "fish", ru: "рыба", pl: "ryba" },
    Name { id: "cod", en: "cod", simple: "sea fish", ru: "треска", pl: "dorsz" },
    Name { id: "gull", en: "gull", simple: "sea bird", ru: "чайка", pl: "mewa" },
    Name { id: "trout", en: "trout", simple: "river fish", ru: "форель", pl: "pstrąg" },
    Name { id: "pike", en: "pike", simple: "big lake fish", ru: "щука", pl: "szczupak" },
    Name { id: "herring", en: "herring", simple: "small sea fish", ru: "сельдь", pl: "śledź" },
    Name { id: "rat", en: "rat", simple: "rat", ru: "крыса", pl: "szczur" },
    // The young. See `youth::young_name`.
    Name { id: "fawn", en: "fawn", simple: "young deer", ru: "оленёнок", pl: "jelonek" },
    Name { id: "lamb", en: "lamb", simple: "young sheep", ru: "ягнёнок", pl: "jagnię" },
    Name { id: "piglet", en: "piglet", simple: "young wild pig", ru: "поросёнок", pl: "warchlak" },
    Name { id: "foal", en: "foal", simple: "young zebra", ru: "жеребёнок", pl: "źrebię" },
    Name { id: "antelope_calf", en: "antelope calf", simple: "young antelope", ru: "антилопёнок", pl: "młode antylopy" },
];

/// How an animal killed somebody, in each language, keyed by `Species::name`:
/// one row for each species whose `damage` is more than nothing. English is
/// the server's own sentence, word for word, so a player reading English sees
/// exactly what they always saw.
#[rustfmt::skip]
const DEATHS: &[Name] = &[
    Name { id: "boar", en: "was gored by a boar", simple: "a wild pig killed you", ru: "поднят на клыки кабаном", pl: "rozpruty przez dzika" },
    Name { id: "wolf", en: "was pulled down by a wolf", simple: "a wolf killed you", ru: "загрызен волком", pl: "rozszarpany przez wilka" },
    Name { id: "bear", en: "was mauled by a bear", simple: "a bear killed you", ru: "задран медведем", pl: "poturbowany przez niedźwiedzia" },
    Name { id: "lion", en: "was brought down by a lion", simple: "a lion killed you", ru: "растерзан львом", pl: "powalony przez lwa" },
];

/// Conditions, keyed by the English the rules print.
const CONDITIONS: &[Name] = &[
    Name { id: "clean water", en: "clean water", simple: "clean water", ru: "чистая вода", pl: "czysta woda" },
    Name { id: "pond water", en: "pond water", simple: "still water", ru: "стоячая вода", pl: "stojąca woda" },
    Name { id: "sea water", en: "sea water", simple: "salt water", ru: "морская вода", pl: "morska woda" },
    Name { id: "dulled", en: "dulled", simple: "a little blunt", ru: "чуть затуплен", pl: "lekko stępiony" },
    Name { id: "dull", en: "dull", simple: "blunt", ru: "затуплен", pl: "stępiony" },
    Name { id: "blunt", en: "blunt", simple: "very blunt", ru: "тупой", pl: "tępy" },
    Name { id: "steeled", en: "steeled", simple: "hardened", ru: "закалён", pl: "hartowany" },
    Name { id: "steeled, dulled", en: "steeled, dulled", simple: "hardened, a little blunt", ru: "закалён, чуть затуплен", pl: "hartowany, lekko stępiony" },
    Name { id: "steeled, dull", en: "steeled, dull", simple: "hardened, blunt", ru: "закалён, затуплен", pl: "hartowany, stępiony" },
    Name { id: "steeled, blunt", en: "steeled, blunt", simple: "hardened, very blunt", ru: "закалён, тупой", pl: "hartowany, tępy" },
    Name { id: "wet", en: "wet", simple: "still wet", ru: "сырая", pl: "mokra" },
    Name { id: "leather-hard", en: "leather-hard", simple: "half dry", ru: "подсохшая", pl: "podeschnięta" },
    Name { id: "bone-dry", en: "bone-dry", simple: "dry, ready to fire", ru: "сухая", pl: "sucha" },
    Name { id: "green", en: "green", simple: "fresh cut, wet", ru: "сырое", pl: "surowe" },
    Name { id: "seasoning", en: "seasoning", simple: "drying out", ru: "подсыхает", pl: "schnie" },
];

/// Every block and item, in `ALL_BLOCK_IDS`'s order so a new row has an
/// obvious place.
///
/// Russian follows `GUIDE.md`, which is where a Russian player learned
/// the words first: a worked stick is "обработанное древко" because the
/// manual's crafting steps call it that, `log` is oak because the
/// manual's table of woods says so. Simple English says what a thing
/// *is* where the proper word is a trade's ("iron furnace", not
/// "bloomery") -- the same line `lang` draws for the rest of the
/// interface, and the same words it already uses for the stations.
pub const BLOCKS: &[Name] = &[
    Name { id: "grass", en: "Grass", simple: "Grass", ru: "Дёрн", pl: "Darń" },
    Name { id: "dirt", en: "Dirt", simple: "Dirt", ru: "Земля", pl: "Ziemia" },
    Name { id: "stone", en: "Stone", simple: "Stone", ru: "Камень", pl: "Kamień" },
    Name { id: "sand", en: "Sand", simple: "Sand", ru: "Песок", pl: "Piasek" },
    Name { id: "snow", en: "Snow", simple: "Snow", ru: "Снег", pl: "Śnieg" },
    Name { id: "water", en: "Water", simple: "Water", ru: "Вода", pl: "Woda" },
    Name { id: "log", en: "Oak log", simple: "Oak log", ru: "Дубовое бревно", pl: "Dębowy pień" },
    Name { id: "leaves", en: "Oak leaves", simple: "Oak leaves", ru: "Дубовая листва", pl: "Liście dębu" },
    Name { id: "glowstone", en: "Glowstone", simple: "Light stone", ru: "Светящийся камень", pl: "Świecący kamień" },
    Name { id: "planks", en: "Oak planks", simple: "Oak boards", ru: "Дубовые доски", pl: "Dębowe deski" },
    Name { id: "cobblestone", en: "Cobblestone", simple: "Broken stone", ru: "Булыжник", pl: "Bruk" },
    Name { id: "tall_grass", en: "Tall grass", simple: "Tall grass", ru: "Высокая трава", pl: "Wysoka trawa" },
    Name { id: "cactus", en: "Cactus", simple: "Cactus", ru: "Кактус", pl: "Kaktus" },
    Name { id: "stick", en: "Stick", simple: "Stick", ru: "Палка", pl: "Patyk" },
    Name { id: "fiber", en: "Fibre", simple: "Plant fibre", ru: "Волокно", pl: "Włókno" },
    Name { id: "pebble", en: "Pebble", simple: "Small stone", ru: "Камушек", pl: "Kamyk" },
    Name { id: "flint", en: "Flint", simple: "Flint", ru: "Кремень", pl: "Krzemień" },
    Name { id: "chest", en: "Chest", simple: "Storage box", ru: "Сундук", pl: "Skrzynia" },
    Name { id: "ash", en: "Ash", simple: "Ash", ru: "Зола", pl: "Popiół" },
    Name { id: "clay", en: "Clay", simple: "Clay", ru: "Глина", pl: "Glina" },
    Name { id: "gravel", en: "Gravel", simple: "Gravel", ru: "Гравий", pl: "Żwir" },
    Name { id: "birch_log", en: "Birch log", simple: "Birch log", ru: "Берёзовое бревно", pl: "Brzozowy pień" },
    Name { id: "birch_leaves", en: "Birch leaves", simple: "Birch leaves", ru: "Берёзовая листва", pl: "Liście brzozy" },
    Name { id: "birch_planks", en: "Birch planks", simple: "Birch boards", ru: "Берёзовые доски", pl: "Brzozowe deski" },
    Name { id: "backpack", en: "Backpack", simple: "Backpack", ru: "Рюкзак", pl: "Plecak" },
    Name { id: "coal_ore", en: "Coal ore", simple: "Coal in rock", ru: "Угольная руда", pl: "Ruda węgla" },
    Name { id: "copper_ore", en: "Copper ore", simple: "Copper rock", ru: "Медная руда", pl: "Ruda miedzi" },
    Name { id: "tin_ore", en: "Tin ore", simple: "Tin rock", ru: "Оловянная руда", pl: "Ruda cyny" },
    Name { id: "iron_ore", en: "Iron ore", simple: "Iron rock", ru: "Железная руда", pl: "Ruda żelaza" },
    Name { id: "coal", en: "Coal", simple: "Coal", ru: "Уголь", pl: "Węgiel" },
    Name { id: "copper_ingot", en: "Copper ingot", simple: "Copper bar", ru: "Медный слиток", pl: "Sztabka miedzi" },
    Name { id: "tin_ingot", en: "Tin ingot", simple: "Tin bar", ru: "Оловянный слиток", pl: "Sztabka cyny" },
    Name { id: "bronze_ingot", en: "Bronze ingot", simple: "Bronze bar", ru: "Бронзовый слиток", pl: "Sztabka brązu" },
    Name { id: "iron_ingot", en: "Iron ingot", simple: "Iron bar", ru: "Железный слиток", pl: "Sztabka żelaza" },
    Name { id: "flint_flake", en: "Flint flake", simple: "Sharp flint chip", ru: "Кремнёвый отщеп", pl: "Odłupek krzemienny" },
    Name { id: "worked_stick", en: "Worked stick", simple: "Shaped stick", ru: "Обработанное древко", pl: "Obrobiony kij" },
    Name { id: "flint_knife_head", en: "Flint knife head", simple: "Flint blade", ru: "Кремнёвая головка ножа", pl: "Krzemienne ostrze" },
    Name { id: "stone_axe_head", en: "Stone axe head", simple: "Stone axe head", ru: "Каменная головка топора", pl: "Kamienna głowica topora" },
    Name { id: "stone_pick_head", en: "Stone pick head", simple: "Stone pick head", ru: "Каменная головка кирки", pl: "Kamienna głowica kilofa" },
    Name { id: "flint_knife", en: "Flint knife", simple: "Flint knife", ru: "Кремнёвый нож", pl: "Krzemienny nóż" },
    Name { id: "stone_axe", en: "Stone axe", simple: "Stone axe", ru: "Каменный топор", pl: "Kamienny topór" },
    Name { id: "stone_pickaxe", en: "Stone pickaxe", simple: "Stone pick", ru: "Каменная кирка", pl: "Kamienny kilof" },
    Name { id: "berry_bush", en: "Berry bush", simple: "Berry bush", ru: "Ягодный куст", pl: "Krzew jagodowy" },
    Name { id: "bare_bush", en: "Picked berry bush", simple: "Bush with no berries", ru: "Обобранный куст", pl: "Obrany krzew" },
    Name { id: "mushroom", en: "Mushroom", simple: "Mushroom", ru: "Гриб", pl: "Grzyb" },
    Name { id: "reeds", en: "Reeds", simple: "Reeds", ru: "Камыш", pl: "Sitowie" },
    Name { id: "flower", en: "Flower", simple: "Flower", ru: "Цветок", pl: "Kwiat" },
    Name { id: "berries", en: "Berries", simple: "Berries", ru: "Ягоды", pl: "Jagody" },
    Name { id: "raw_meat", en: "Raw meat", simple: "Raw meat", ru: "Сырое мясо", pl: "Surowe mięso" },
    Name { id: "cooked_meat", en: "Cooked meat", simple: "Cooked meat", ru: "Жареное мясо", pl: "Pieczone mięso" },
    Name { id: "hide", en: "Hide", simple: "Animal skin", ru: "Шкура", pl: "Surowa skóra" },
    Name { id: "campfire", en: "Campfire", simple: "Campfire", ru: "Костёр", pl: "Ognisko" },
    Name { id: "campfire_lit", en: "Lit campfire", simple: "Burning campfire", ru: "Горящий костёр", pl: "Płonące ognisko" },
    Name { id: "copper_knife", en: "Copper knife", simple: "Copper knife", ru: "Медный нож", pl: "Miedziany nóż" },
    Name { id: "copper_axe", en: "Copper axe", simple: "Copper axe", ru: "Медный топор", pl: "Miedziany topór" },
    Name { id: "copper_pickaxe", en: "Copper pickaxe", simple: "Copper pick", ru: "Медная кирка", pl: "Miedziany kilof" },
    Name { id: "bronze_knife", en: "Bronze knife", simple: "Bronze knife", ru: "Бронзовый нож", pl: "Brązowy nóż" },
    Name { id: "bronze_axe", en: "Bronze axe", simple: "Bronze axe", ru: "Бронзовый топор", pl: "Brązowy topór" },
    Name { id: "bronze_pickaxe", en: "Bronze pickaxe", simple: "Bronze pick", ru: "Бронзовая кирка", pl: "Brązowy kilof" },
    Name { id: "iron_knife", en: "Iron knife", simple: "Iron knife", ru: "Железный нож", pl: "Żelazny nóż" },
    Name { id: "iron_axe", en: "Iron axe", simple: "Iron axe", ru: "Железный топор", pl: "Żelazny topór" },
    Name { id: "iron_pickaxe", en: "Iron pickaxe", simple: "Iron pick", ru: "Железная кирка", pl: "Żelazny kilof" },
    Name { id: "kiln", en: "Kiln", simple: "Clay oven", ru: "Горн", pl: "Piec" },
    Name { id: "kiln_lit", en: "Lit kiln", simple: "Burning clay oven", ru: "Горящий горн", pl: "Rozpalony piec" },
    Name { id: "brick", en: "Brick", simple: "Brick", ru: "Кирпич", pl: "Cegła" },
    Name { id: "bricks", en: "Bricks", simple: "Brick wall", ru: "Кирпичная кладка", pl: "Mur z cegieł" },
    Name { id: "hoe", en: "Hoe", simple: "Hoe", ru: "Мотыга", pl: "Motyka" },
    Name { id: "seeds", en: "Seeds", simple: "Seeds", ru: "Семена", pl: "Nasiona" },
    Name { id: "farmland", en: "Farmland", simple: "Dug soil", ru: "Пашня", pl: "Pole uprawne" },
    Name { id: "wheat", en: "Young wheat", simple: "Growing wheat", ru: "Всходы пшеницы", pl: "Młoda pszenica" },
    Name { id: "wheat_ripe", en: "Ripe wheat", simple: "Ripe wheat", ru: "Спелая пшеница", pl: "Dojrzała pszenica" },
    Name { id: "grain", en: "Grain", simple: "Grain", ru: "Зерно", pl: "Ziarno" },
    Name { id: "dough", en: "Dough", simple: "Dough", ru: "Тесто", pl: "Ciasto" },
    Name { id: "bread", en: "Bread", simple: "Bread", ru: "Хлеб", pl: "Chleb" },
    Name { id: "native_copper", en: "Native copper", simple: "Copper nugget", ru: "Самородная медь", pl: "Miedź rodzima" },
    Name { id: "vessel_raw", en: "Unfired crucible", simple: "Wet clay pot", ru: "Сырой горшок", pl: "Surowy tygiel" },
    Name { id: "vessel", en: "Crucible", simple: "Melting pot", ru: "Горшок", pl: "Tygiel" },
    Name { id: "mould_raw", en: "Unfired mould", simple: "Wet clay mould", ru: "Сырая форма", pl: "Surowa forma" },
    Name { id: "mould", en: "Ingot mould", simple: "Bar mould", ru: "Форма для слитка", pl: "Forma na sztabkę" },
    Name { id: "bloomery", en: "Bloomery", simple: "Iron furnace", ru: "Домница", pl: "Dymarka" },
    Name { id: "bloomery_lit", en: "Lit bloomery", simple: "Burning iron furnace", ru: "Горящая домница", pl: "Rozpalona dymarka" },
    Name { id: "iron_bloom", en: "Iron bloom", simple: "Lump of raw iron", ru: "Крица", pl: "Łupa żelaza" },
    Name { id: "ice", en: "Ice", simple: "Ice", ru: "Лёд", pl: "Lód" },
    Name { id: "leather", en: "Leather", simple: "Leather", ru: "Кожа", pl: "Skóra garbowana" },
    Name { id: "drying_rack", en: "Drying rack", simple: "Meat and fish rack", ru: "Сушилка", pl: "Suszarnia" },
    Name { id: "hide_frame", en: "Hide frame", simple: "Skin frame", ru: "Рама для шкуры", pl: "Rama na skórę" },
    Name { id: "jug_raw", en: "Unfired jug", simple: "Wet clay jug", ru: "Сырой кувшин", pl: "Surowy dzban" },
    Name { id: "jug", en: "Jug", simple: "Jug", ru: "Кувшин", pl: "Dzban" },
    Name { id: "bowl_raw", en: "Unfired bowl", simple: "Wet clay bowl", ru: "Сырая миска", pl: "Surowa miska" },
    Name { id: "bowl", en: "Bowl", simple: "Bowl", ru: "Миска", pl: "Miska" },
    Name { id: "stew", en: "Bowl of stew", simple: "Stew", ru: "Миска похлёбки", pl: "Miska gulaszu" },
    Name { id: "jug_water", en: "Jug of water", simple: "Jug of water", ru: "Кувшин с водой", pl: "Dzban wody" },
    Name { id: "leather_cap", en: "Leather cap", simple: "Leather hat", ru: "Кожаная шапка", pl: "Skórzana czapka" },
    Name { id: "leather_tunic", en: "Leather tunic", simple: "Leather shirt", ru: "Кожаная рубаха", pl: "Skórzana tunika" },
    Name { id: "leather_leggings", en: "Leather leggings", simple: "Leather trousers", ru: "Кожаные штаны", pl: "Skórzane spodnie" },
    Name { id: "leather_boots", en: "Leather boots", simple: "Leather boots", ru: "Кожаные сапоги", pl: "Skórzane buty" },
    Name { id: "bronze_helm", en: "Bronze helm", simple: "Bronze helmet", ru: "Бронзовый шлем", pl: "Hełm z brązu" },
    Name { id: "bronze_cuirass", en: "Bronze cuirass", simple: "Bronze chest armour", ru: "Бронзовая кираса", pl: "Kirys z brązu" },
    Name { id: "bronze_greaves", en: "Bronze greaves", simple: "Bronze leg armour", ru: "Бронзовые поножи", pl: "Nagolenniki z brązu" },
    Name { id: "bronze_boots", en: "Bronze boots", simple: "Bronze boots", ru: "Бронзовые сапоги", pl: "Buty z brązu" },
    Name { id: "iron_helm", en: "Iron helm", simple: "Iron helmet", ru: "Железный шлем", pl: "Żelazny hełm" },
    Name { id: "iron_cuirass", en: "Iron cuirass", simple: "Iron chest armour", ru: "Железная кираса", pl: "Żelazny kirys" },
    Name { id: "iron_greaves", en: "Iron greaves", simple: "Iron leg armour", ru: "Железные поножи", pl: "Żelazne nagolenniki" },
    Name { id: "iron_boots", en: "Iron boots", simple: "Iron boots", ru: "Железные сапоги", pl: "Żelazne buty" },
    Name { id: "stripped_log", en: "Stripped log", simple: "Log without bark", ru: "Окорённое бревно", pl: "Okorowany pień" },
    Name { id: "roots", en: "Roots", simple: "Root plant", ru: "Корни", pl: "Korzenie" },
    Name { id: "root", en: "Root", simple: "Root", ru: "Корень", pl: "Korzeń" },
    Name { id: "roasted_root", en: "Roasted root", simple: "Cooked root", ru: "Печёный корень", pl: "Pieczony korzeń" },
    Name { id: "toadstool", en: "Toadstool", simple: "Poison mushroom", ru: "Мухомор", pl: "Muchomor" },
    Name { id: "dried_meat", en: "Dried meat", simple: "Dried meat", ru: "Вяленое мясо", pl: "Suszone mięso" },
    Name { id: "planter", en: "Planter", simple: "Pot of earth", ru: "Горшок с землёй", pl: "Donica" },
    Name { id: "prop", en: "Pit prop", simple: "Roof post", ru: "Подпорка", pl: "Stempel" },
    Name { id: "stake", en: "Stake", simple: "Sharpened pole", ru: "Кол", pl: "Kołek" },
    Name { id: "window_lattice", en: "Window lattice", simple: "Window grid", ru: "Оконная решётка", pl: "Krata okienna" },
    Name { id: "salt", en: "Salt", simple: "Salt", ru: "Соль", pl: "Sól" },
    Name { id: "salted_meat", en: "Salted meat", simple: "Meat in salt", ru: "Солёное мясо", pl: "Solone mięso" },
    Name { id: "salted_fish", en: "Salted fish", simple: "Fish in salt", ru: "Солёная рыба", pl: "Solona ryba" },
    Name { id: "dried_fish", en: "Dried fish", simple: "Dried fish", ru: "Вяленая рыба", pl: "Suszona ryba" },
    Name { id: "dried_salted_meat", en: "Salt-dried meat", simple: "Meat dried in salt", ru: "Вяленое солёное мясо", pl: "Suszone solone mięso" },
    Name { id: "dried_salted_fish", en: "Salt-dried fish", simple: "Fish dried in salt", ru: "Вяленая солёная рыба", pl: "Suszona solona ryba" },
    Name { id: "wool", en: "Wool", simple: "Wool", ru: "Шерсть", pl: "Wełna" },
    Name { id: "wool_cap", en: "Wool cap", simple: "Wool hat", ru: "Шерстяная шапка", pl: "Wełniana czapka" },
    Name { id: "wool_tunic", en: "Wool tunic", simple: "Wool shirt", ru: "Шерстяная рубаха", pl: "Wełniana tunika" },
    Name { id: "wool_leggings", en: "Wool leggings", simple: "Wool trousers", ru: "Шерстяные штаны", pl: "Wełniane spodnie" },
    Name { id: "wool_boots", en: "Wool boots", simple: "Wool boots", ru: "Шерстяные сапоги", pl: "Wełniane buty" },
    Name { id: "copper_axe_head", en: "Copper axe head", simple: "Copper axe head", ru: "Медная головка топора", pl: "Miedziana głowica topora" },
    Name { id: "copper_pick_head", en: "Copper pick head", simple: "Copper pick head", ru: "Медная головка кирки", pl: "Miedziana głowica kilofa" },
    Name { id: "copper_shovel_head", en: "Copper shovel blade", simple: "Copper shovel blade", ru: "Медное полотно лопаты", pl: "Miedziane ostrze łopaty" },
    Name { id: "copper_hoe_head", en: "Copper hoe blade", simple: "Copper hoe blade", ru: "Медное лезвие мотыги", pl: "Miedziane ostrze motyki" },
    Name { id: "copper_shovel", en: "Copper shovel", simple: "Copper shovel", ru: "Медная лопата", pl: "Miedziana łopata" },
    Name { id: "copper_hoe", en: "Copper hoe", simple: "Copper hoe", ru: "Медная мотыга", pl: "Miedziana motyka" },
    Name { id: "torch", en: "Torch", simple: "Torch", ru: "Факел", pl: "Pochodnia" },
    Name { id: "torch_lit", en: "Lit torch", simple: "Burning torch", ru: "Горящий факел", pl: "Płonąca pochodnia" },
    Name { id: "torch_spent", en: "Burnt-out torch", simple: "Used-up torch", ru: "Прогоревший факел", pl: "Wypalona pochodnia" },
    Name { id: "sandstone", en: "Sandstone", simple: "Sandstone", ru: "Песчаник", pl: "Piaskowiec" },
    Name { id: "limestone", en: "Limestone", simple: "Limestone", ru: "Известняк", pl: "Wapień" },
    Name { id: "granite", en: "Granite", simple: "Granite", ru: "Гранит", pl: "Granit" },
    Name { id: "stalagmite", en: "Stalagmite", simple: "Stone spike", ru: "Сталагмит", pl: "Stalagmit" },
    Name { id: "stalactite", en: "Stalactite", simple: "Hanging stone spike", ru: "Сталактит", pl: "Stalaktyt" },
    Name { id: "peat", en: "Peat", simple: "Peat", ru: "Торф", pl: "Torf" },
    Name { id: "drying_peat", en: "Half-dried peat", simple: "Peat, drying", ru: "Подсохший торф", pl: "Podsuszony torf" },
    Name { id: "dried_peat", en: "Dried peat", simple: "Dry peat", ru: "Сухой торф", pl: "Suchy torf" },
    Name { id: "sinew", en: "Sinew", simple: "Animal string", ru: "Жила", pl: "Ścięgno" },
    Name { id: "bone", en: "Bone", simple: "Bone", ru: "Кость", pl: "Kość" },
    Name { id: "carcass_hare", en: "Hare carcass", simple: "Dead hare", ru: "Туша зайца", pl: "Tusza zająca" },
    Name { id: "carcass_deer", en: "Deer carcass", simple: "Dead deer", ru: "Туша оленя", pl: "Tusza jelenia" },
    Name { id: "carcass_boar", en: "Boar carcass", simple: "Dead boar", ru: "Туша кабана", pl: "Tusza dzika" },
    Name { id: "carcass_wolf", en: "Wolf carcass", simple: "Dead wolf", ru: "Туша волка", pl: "Tusza wilka" },
    Name { id: "carcass_sheep", en: "Sheep carcass", simple: "Dead sheep", ru: "Туша овцы", pl: "Tusza owcy" },
    Name { id: "cord", en: "Cord", simple: "Rope", ru: "Верёвка", pl: "Sznur" },
    Name { id: "sail", en: "Sail", simple: "Sail", ru: "Парус", pl: "Żagiel" },
    Name { id: "oar", en: "Oar", simple: "Oar", ru: "Весло", pl: "Wiosło" },
    Name { id: "raft", en: "Raft", simple: "Raft", ru: "Плот", pl: "Tratwa" },
    Name { id: "wedged_axe", en: "Wedged axe", simple: "Strong stone axe", ru: "Расклиненный топор", pl: "Topór klinowany" },
    Name { id: "wedged_pickaxe", en: "Wedged pickaxe", simple: "Strong stone pick", ru: "Расклиненная кирка", pl: "Kilof klinowany" },
    Name { id: "flint_spear", en: "Flint spear", simple: "Flint spear", ru: "Кремнёвое копьё", pl: "Włócznia krzemienna" },
    Name { id: "rusty_stone", en: "Rusty stone", simple: "Rusty stone", ru: "Ржавый камень", pl: "Rdzawy kamień" },
    Name { id: "iron_dust", en: "Iron dust", simple: "Iron powder", ru: "Железная пыль", pl: "Pył żelazny" },
    Name { id: "whetstone", en: "Whetstone", simple: "Sharpening stone", ru: "Точильный камень", pl: "Osełka" },
    Name { id: "slag", en: "Slag", simple: "Furnace waste", ru: "Шлак", pl: "Żużel" },
    Name { id: "steel_ingot", en: "Steel ingot", simple: "Steel bar", ru: "Стальной слиток", pl: "Sztabka stali" },
    Name { id: "stream_tin", en: "Stream tin", simple: "Tin from a stream", ru: "Речное олово", pl: "Cyna z potoku" },
    Name { id: "wild_hive", en: "Wild hive", simple: "Wild bee nest", ru: "Дикий улей", pl: "Dziki ul" },
    Name { id: "honey", en: "Honey", simple: "Honey", ru: "Мёд", pl: "Miód" },
    Name { id: "beeswax", en: "Beeswax", simple: "Bee wax", ru: "Воск", pl: "Wosk pszczeli" },
    Name { id: "fish_trap", en: "Fish trap", simple: "Fish trap", ru: "Верша", pl: "Więcierz" },
    Name { id: "fishing_rod", en: "Fishing rod", simple: "Fishing rod", ru: "Удочка", pl: "Wędka" },
    Name { id: "worm", en: "Worm", simple: "Worm", ru: "Червь", pl: "Robak" },
    Name { id: "grub", en: "Grub", simple: "Bug", ru: "Личинка", pl: "Larwa" },
    Name { id: "fishing_fly", en: "Fishing fly", simple: "Fly lure", ru: "Мушка", pl: "Mucha wędkarska" },
    Name { id: "copper_hook", en: "Copper hook", simple: "Fish hook", ru: "Медный крючок", pl: "Miedziany haczyk" },
    Name { id: "rotten", en: "Rotten food", simple: "Rotten food", ru: "Гниль", pl: "Zgnilizna" },
    Name { id: "apple_leaves", en: "Apple leaves", simple: "Apple tree leaves", ru: "Листва яблони", pl: "Liście jabłoni" },
    Name { id: "apple_leaves_fruit", en: "Apple leaves with fruit", simple: "Apple tree with apples", ru: "Яблоня с яблоками", pl: "Jabłoń z owocami" },
    Name { id: "apple", en: "Apple", simple: "Apple", ru: "Яблоко", pl: "Jabłko" },
    Name { id: "peg", en: "Peg", simple: "Wooden peg", ru: "Штифт", pl: "Kołek" },
    Name { id: "nails", en: "Nails", simple: "Nails", ru: "Гвозди", pl: "Gwoździe" },
    Name { id: "frame", en: "Frame", simple: "Furniture frame", ru: "Рама", pl: "Rama" },
    Name { id: "pegged_planks", en: "Pegged oak planks", simple: "Pinned oak boards", ru: "Дубовые доски на штифтах", pl: "Dębowe deski na kołkach" },
    Name { id: "pegged_birch_planks", en: "Pegged birch planks", simple: "Pinned birch boards", ru: "Берёзовые доски на штифтах", pl: "Brzozowe deski na kołkach" },
    Name { id: "hare_meat", en: "Hare meat", simple: "Hare meat", ru: "Зайчатина", pl: "Zajęczyna" },
    Name { id: "fowl_meat", en: "Fowl meat", simple: "Bird meat", ru: "Птичье мясо", pl: "Drób" },
    Name { id: "bear_meat", en: "Bear meat", simple: "Bear meat", ru: "Медвежатина", pl: "Niedźwiedzina" },
    Name { id: "wolf_meat", en: "Wolf meat", simple: "Wolf meat", ru: "Волчатина", pl: "Wilczyna" },
    Name { id: "pelt", en: "Pelt", simple: "Small fur", ru: "Шкурка", pl: "Futerko" },
    Name { id: "bear_hide", en: "Bear hide", simple: "Bear skin", ru: "Медвежья шкура", pl: "Niedźwiedzia skóra" },
    Name { id: "feather", en: "Feather", simple: "Feather", ru: "Перо", pl: "Pióro" },
    Name { id: "fat", en: "Fat", simple: "Animal fat", ru: "Жир", pl: "Łój" },
    Name { id: "ribs", en: "Ribs", simple: "Raw ribs", ru: "Рёбра", pl: "Żeberka" },
    Name { id: "roasted_ribs", en: "Roasted ribs", simple: "Cooked ribs", ru: "Жареные рёбра", pl: "Pieczone żeberka" },
    Name { id: "carcass_bear", en: "Bear carcass", simple: "Dead bear", ru: "Туша медведя", pl: "Tusza niedźwiedzia" },
    Name { id: "carcass_fowl", en: "Fowl carcass", simple: "Dead bird", ru: "Тушка птицы", pl: "Tuszka ptaka" },
    Name { id: "carcass_zebra", en: "Zebra carcass", simple: "Dead zebra", ru: "Туша зебры", pl: "Tusza zebry" },
    Name { id: "carcass_antelope", en: "Antelope carcass", simple: "Dead antelope", ru: "Туша антилопы", pl: "Tusza antylopy" },
    Name { id: "carcass_lion", en: "Lion carcass", simple: "Dead lion", ru: "Туша льва", pl: "Tusza lwa" },
    Name { id: "basalt", en: "Basalt", simple: "Basalt", ru: "Базальт", pl: "Bazalt" },
    Name { id: "bracket_fungus", en: "Bracket fungus", simple: "Tree fungus", ru: "Трутовик", pl: "Huba" },
    Name { id: "fur_hood", en: "Fur hood", simple: "Fur hood", ru: "Меховой капюшон", pl: "Futrzany kaptur" },
    Name { id: "fur_cloak", en: "Fur cloak", simple: "Fur cloak", ru: "Меховая накидка", pl: "Futrzana peleryna" },
    Name { id: "wild_wheat", en: "Wild wheat", simple: "Wild wheat", ru: "Дикая пшеница", pl: "Dzika pszenica" },
    Name { id: "flour", en: "Flour", simple: "Flour", ru: "Мука", pl: "Mąka" },
    Name { id: "nest_eggs", en: "Nest with eggs", simple: "Nest with eggs", ru: "Гнездо с яйцами", pl: "Gniazdo z jajkami" },
    Name { id: "nest", en: "Nest", simple: "Empty nest", ru: "Гнездо", pl: "Gniazdo" },
    Name { id: "egg", en: "Egg", simple: "Egg", ru: "Яйцо", pl: "Jajko" },
    Name { id: "bone_spear", en: "Bone spear", simple: "Bone spear", ru: "Костяное копьё", pl: "Włócznia kościana" },
    Name { id: "copper_spear", en: "Copper spear", simple: "Copper spear", ru: "Медное копьё", pl: "Włócznia miedziana" },
    Name { id: "bronze_spear", en: "Bronze spear", simple: "Bronze spear", ru: "Бронзовое копьё", pl: "Włócznia z brązu" },
    Name { id: "iron_spear", en: "Iron spear", simple: "Iron spear", ru: "Железное копьё", pl: "Włócznia żelazna" },
    Name { id: "straw_bed", en: "Straw bed", simple: "Grass bed", ru: "Соломенное лежбище", pl: "Posłanie ze słomy" },
    Name { id: "bed", en: "Bed", simple: "Bed", ru: "Кровать", pl: "Łóżko" },
    Name { id: "stool", en: "Stool", simple: "Stool", ru: "Табурет", pl: "Stołek" },
    Name { id: "table", en: "Table", simple: "Table", ru: "Стол", pl: "Stół" },
    Name { id: "chair", en: "Chair", simple: "Chair", ru: "Стул", pl: "Krzesło" },
    Name { id: "workbench", en: "Workbench", simple: "Work table", ru: "Верстак", pl: "Warsztat" },
    Name { id: "mason_block", en: "Mason's block", simple: "Stone work block", ru: "Колода каменотёса", pl: "Blok kamieniarski" },
    Name { id: "potters_wheel", en: "Potter's wheel", simple: "Clay wheel", ru: "Гончарный круг", pl: "Koło garncarskie" },
    Name { id: "leather_bench", en: "Leather bench", simple: "Leather table", ru: "Скорняжный стол", pl: "Stół rymarski" },
    Name { id: "anvil", en: "Anvil", simple: "Anvil", ru: "Наковальня", pl: "Kowadło" },
    Name { id: "stone_hammer", en: "Stone hammer", simple: "Stone hammer", ru: "Каменный молот", pl: "Kamienny młot" },
    Name { id: "bronze_hammer", en: "Bronze hammer", simple: "Bronze hammer", ru: "Бронзовый молот", pl: "Brązowy młot" },
    Name { id: "iron_hammer", en: "Iron hammer", simple: "Iron hammer", ru: "Железный молот", pl: "Żelazny młot" },
    Name { id: "flint_chisel", en: "Flint chisel", simple: "Flint chisel", ru: "Кремнёвая стамеска", pl: "Krzemienne dłuto" },
    Name { id: "bronze_chisel", en: "Bronze chisel", simple: "Bronze chisel", ru: "Бронзовая стамеска", pl: "Brązowe dłuto" },
    Name { id: "plank_stairs", en: "Plank stairs", simple: "Wooden steps", ru: "Деревянные ступени", pl: "Drewniane schody" },
    Name { id: "cobblestone_stairs", en: "Cobblestone stairs", simple: "Stone steps", ru: "Каменные ступени", pl: "Kamienne schody" },
    Name { id: "tile_roof", en: "Tiled roof", simple: "Clay roof", ru: "Черепичная крыша", pl: "Dach z dachówki" },
    Name { id: "tile_slab", en: "Roof tiles", simple: "Baked clay roof", ru: "Черепица", pl: "Dachówka" },
    Name { id: "thatch_roof", en: "Thatched roof", simple: "Grass roof", ru: "Соломенная крыша", pl: "Dach kryty strzechą" },
    Name { id: "thatch_slab", en: "Thatch", simple: "Grass roof", ru: "Соломенная кровля", pl: "Strzecha" },
    Name { id: "branch_roof", en: "Branch roof", simple: "Stick roof", ru: "Крыша из веток", pl: "Dach z gałęzi" },
    Name { id: "branch_slab", en: "Branch roofing", simple: "Sticks and leaves roof", ru: "Кровля из веток", pl: "Dach z gałęzi" },
    Name { id: "door", en: "Door", simple: "Door", ru: "Дверь", pl: "Drzwi" },
    Name { id: "door_top", en: "Top of a door", simple: "Top of a door", ru: "Верх двери", pl: "Górna część drzwi" },
    Name { id: "bush_leaves", en: "Bush leaves", simple: "Bush", ru: "Листва кустарника", pl: "Liście krzewu" },
    Name { id: "bones", en: "Bones", simple: "Bones", ru: "Кости", pl: "Kości" },
    Name { id: "bones_2", en: "Bones", simple: "Bones", ru: "Кости", pl: "Kości" },
    Name { id: "bones_3", en: "Bones", simple: "Bones", ru: "Кости", pl: "Kości" },
    Name { id: "bones_4", en: "Bones", simple: "Bones", ru: "Кости", pl: "Kości" },
    Name { id: "barrel", en: "Barrel", simple: "Barrel", ru: "Бочка", pl: "Beczka" },
    Name { id: "barrel_standing", en: "Barrel of pond water", simple: "Barrel of still water", ru: "Бочка стоячей воды", pl: "Beczka stojącej wody" },
    Name { id: "barrel_salt", en: "Barrel of sea water", simple: "Barrel of salt water", ru: "Бочка морской воды", pl: "Beczka morskiej wody" },
    Name { id: "barrel_grain", en: "Barrel of grain", simple: "Barrel of grain", ru: "Бочка зерна", pl: "Beczka ziarna" },
    Name { id: "barrel_seeds", en: "Barrel of seed", simple: "Barrel of seeds", ru: "Бочка семян", pl: "Beczka nasion" },
    Name { id: "barrel_millet", en: "Barrel of millet", simple: "Barrel of millet", ru: "Бочка проса", pl: "Beczka prosa" },
    Name { id: "acacia_leaves", en: "Acacia leaves", simple: "Acacia leaves", ru: "Листва акации", pl: "Liście akacji" },
    Name { id: "termite_mound", en: "Termite mound", simple: "Termite hill", ru: "Термитник", pl: "Kopiec termitów" },
    Name { id: "maple_leaves", en: "Maple leaves", simple: "Maple leaves", ru: "Кленовая листва", pl: "Liście klonu" },
    Name { id: "sandy_soil", en: "Sandy soil", simple: "Sandy dirt", ru: "Супесь", pl: "Piaszczysta gleba" },
    Name { id: "dry_grass", en: "Dry grass", simple: "Dry grass", ru: "Сухая трава", pl: "Sucha trawa" },
    Name { id: "dry_turf", en: "Dry turf", simple: "Dry grass ground", ru: "Сухой дёрн", pl: "Sucha darń" },
    Name { id: "wild_cotton", en: "Wild cotton", simple: "Wild cotton", ru: "Дикий хлопок", pl: "Dzika bawełna" },
    Name { id: "cotton_seeds", en: "Cotton seeds", simple: "Cotton seeds", ru: "Семена хлопка", pl: "Nasiona bawełny" },
    Name { id: "cotton_plant", en: "Young cotton", simple: "Growing cotton", ru: "Всходы хлопка", pl: "Młoda bawełna" },
    Name { id: "cotton_ripe", en: "Ripe cotton", simple: "Ripe cotton", ru: "Спелый хлопок", pl: "Dojrzała bawełna" },
    Name { id: "cotton", en: "Cotton", simple: "Cotton", ru: "Хлопок", pl: "Bawełna" },
    Name { id: "wild_millet", en: "Wild millet", simple: "Wild millet", ru: "Дикое просо", pl: "Dzikie proso" },
    Name { id: "millet", en: "Millet", simple: "Millet", ru: "Просо", pl: "Proso" },
    Name { id: "millet_plant", en: "Young millet", simple: "Growing millet", ru: "Всходы проса", pl: "Młode proso" },
    Name { id: "millet_ripe", en: "Ripe millet", simple: "Ripe millet", ru: "Спелое просо", pl: "Dojrzałe proso" },
    Name { id: "millet_porridge", en: "Millet porridge", simple: "Millet porridge", ru: "Пшённая каша", pl: "Kasza jaglana" },
    Name { id: "cloth", en: "Cloth", simple: "Cloth", ru: "Ткань", pl: "Tkanina" },
    Name { id: "cloth_cap", en: "Cloth cap", simple: "Cloth hat", ru: "Тканевая шапка", pl: "Płócienna czapka" },
    Name { id: "cloth_tunic", en: "Cloth tunic", simple: "Cloth shirt", ru: "Тканевая рубаха", pl: "Płócienna tunika" },
    Name { id: "cloth_trousers", en: "Cloth trousers", simple: "Cloth trousers", ru: "Тканевые штаны", pl: "Płócienne spodnie" },
    Name { id: "cloth_wraps", en: "Cloth wraps", simple: "Cloth foot wraps", ru: "Тканевые обмотки", pl: "Płócienne onuce" },
    Name { id: "withered_crop", en: "Withered crop", simple: "Dead crop", ru: "Засохшие всходы", pl: "Uschnięta uprawa" },
    Name { id: "twig", en: "Twig", simple: "Twig", ru: "Ветка", pl: "Gałązka" },
    Name { id: "bough", en: "Bough", simple: "Thick branch", ru: "Сук", pl: "Konar" },
    Name { id: "kelp", en: "Kelp", simple: "Seaweed", ru: "Ламинария", pl: "Listownica" },
    Name { id: "kelp_top", en: "Kelp top", simple: "Seaweed top", ru: "Верхушка ламинарии", pl: "Wierzchołek listownicy" },
    Name { id: "seagrass", en: "Seagrass", simple: "Sea grass", ru: "Морская трава", pl: "Trawa morska" },
    Name { id: "sea_fan", en: "Sea fan", simple: "Sea fan", ru: "Горгонария", pl: "Gorgonia" },
    Name { id: "staghorn_coral", en: "Staghorn coral", simple: "Branching coral", ru: "Ветвистый коралл", pl: "Koral rogowy" },
    Name { id: "brain_coral", en: "Brain coral", simple: "Round coral", ru: "Мозговой коралл", pl: "Koral mózgowy" },
    Name { id: "fire_coral", en: "Fire coral", simple: "Fire coral", ru: "Огненный коралл", pl: "Koral ognisty" },
    Name { id: "shell", en: "Shell", simple: "Sea shell", ru: "Ракушка", pl: "Muszla" },
    Name { id: "kelp_frond", en: "Kelp frond", simple: "Seaweed leaf", ru: "Лист ламинарии", pl: "Liść listownicy" },
    Name { id: "dried_kelp", en: "Dried kelp", simple: "Dried seaweed", ru: "Сушёная ламинария", pl: "Suszona listownica" },
    Name { id: "raw_fish", en: "Raw fish", simple: "Raw fish", ru: "Сырая рыба", pl: "Surowa ryba" },
    Name { id: "cooked_fish", en: "Cooked fish", simple: "Cooked fish", ru: "Жареная рыба", pl: "Pieczona ryba" },
    Name { id: "palm_trunk", en: "Palm trunk", simple: "Palm trunk", ru: "Ствол пальмы", pl: "Pień palmy" },
    Name { id: "palm_fronds", en: "Palm fronds", simple: "Palm leaves", ru: "Листья пальмы", pl: "Liście palmy" },
    Name { id: "palm_coconuts", en: "Palm coconuts", simple: "Coconuts on a palm", ru: "Гроздь кокосов", pl: "Kiść kokosów" },
    Name { id: "coconut", en: "Coconut", simple: "Coconut", ru: "Кокос", pl: "Kokos" },
    Name { id: "mud", en: "Mud", simple: "Mud", ru: "Ил", pl: "Muł" },
    Name { id: "dung", en: "Dung", simple: "Poop", ru: "Навоз", pl: "Łajno" },
    Name { id: "lily_pad", en: "Lily pad", simple: "Water lily leaf", ru: "Кувшинка", pl: "Lilia wodna" },
    Name { id: "hanging_moss", en: "Hanging moss", simple: "Hanging moss", ru: "Свисающий мох", pl: "Zwisający mech" },
    Name { id: "drowned_twig", en: "Drowned twig", simple: "Twig under water", ru: "Затопленная ветка", pl: "Zatopiona gałązka" },
    Name { id: "drowned_bough", en: "Drowned bough", simple: "Branch under water", ru: "Затопленный сук", pl: "Zatopiony konar" },
    Name { id: "bandage", en: "Bandage", simple: "Bandage", ru: "Бинт", pl: "Bandaż" },
    Name { id: "splint", en: "Splint", simple: "Splint for a bone", ru: "Шина", pl: "Szyna" },
    Name { id: "poultice", en: "Poultice", simple: "Leaf dressing", ru: "Припарка", pl: "Okład" },
    Name { id: "rucksack", en: "Rucksack", simple: "Bag for your back", ru: "Рюкзак", pl: "Plecak" },
    Name { id: "brick_raw", en: "Unfired brick", simple: "Wet clay brick", ru: "Сырой кирпич", pl: "Surowa cegła" },
    Name { id: "pit_kiln", en: "Pit kiln", simple: "Firing pit", ru: "Земляная печь", pl: "Piec dołowy" },
    Name { id: "pit_kiln_fibre", en: "Pit kiln with fibre", simple: "Firing pit with grass", ru: "Земляная печь с волокном", pl: "Piec dołowy z włóknem" },
    Name { id: "pit_kiln_logs", en: "Pit kiln with logs", simple: "Firing pit with logs", ru: "Земляная печь с дровами", pl: "Piec dołowy z drewnem" },
    Name { id: "pit_kiln_lit", en: "Lit pit kiln", simple: "Burning firing pit", ru: "Горящая земляная печь", pl: "Płonący piec dołowy" },
    Name { id: "log_pile", en: "Log pile", simple: "Pile of logs", ru: "Поленница", pl: "Stos drewna" },
    Name { id: "log_pile_lit", en: "Burning log pile", simple: "Burning pile of logs", ru: "Горящая поленница", pl: "Płonący stos drewna" },
    Name { id: "charcoal_pile", en: "Charcoal heap", simple: "Pile of charcoal", ru: "Куча угля", pl: "Stos węgla drzewnego" },
    Name { id: "firepit", en: "Firepit", simple: "Fire ring", ru: "Кострище", pl: "Palenisko" },
    Name { id: "firepit_lit", en: "Lit firepit", simple: "Burning fire ring", ru: "Горящее кострище", pl: "Płonące palenisko" },
    Name { id: "burning_log", en: "Burning log", simple: "Burning log", ru: "Горящее бревно", pl: "Płonący pień" },
    Name { id: "burning_planks", en: "Burning planks", simple: "Burning boards", ru: "Горящие доски", pl: "Płonące deski" },
    Name { id: "charred_log", en: "Charred log", simple: "Burnt log", ru: "Обугленное бревно", pl: "Zwęglony pień" },
    Name { id: "charred_planks", en: "Charred planks", simple: "Burnt boards", ru: "Обугленные доски", pl: "Zwęglone deski" },
    Name { id: "standing_torch", en: "Standing torch", simple: "Torch on a pole", ru: "Факел на шесте", pl: "Pochodnia na tyczce" },
    Name { id: "standing_torch_lit", en: "Lit standing torch", simple: "Burning torch on a pole", ru: "Горящий факел на шесте", pl: "Płonąca pochodnia na tyczce" },
    Name { id: "standing_torch_out", en: "Burnt-out standing torch", simple: "Used-up torch on a pole", ru: "Погасший факел на шесте", pl: "Zgasła pochodnia na tyczce" },
    Name { id: "snow_cover", en: "Snow cover", simple: "Snow on the ground", ru: "Снежный покров", pl: "Warstwa śniegu" },
    Name { id: "sandstone_bricks", en: "Sandstone bricks", simple: "Sandstone wall", ru: "Кладка из песчаника", pl: "Mur z piaskowca" },
    Name { id: "human_flesh", en: "Human flesh", simple: "Human meat", ru: "Человечина", pl: "Ludzkie mięso" },
    Name { id: "roast_human_flesh", en: "Roast human flesh", simple: "Cooked human meat", ru: "Жареная человечина", pl: "Pieczone ludzkie mięso" },
    Name { id: "leaf_handful", en: "Handful of leaves", simple: "Handful of leaves", ru: "Горсть листьев", pl: "Garść liści" },
    Name { id: "leaf_litter", en: "Fallen leaves", simple: "Dead leaves on the ground", ru: "Опавшие листья", pl: "Opadłe liście" },
    Name { id: "resin", en: "Resin", simple: "Tree sap", ru: "Смола", pl: "Żywica" },
    Name { id: "fireweed", en: "Fireweed", simple: "Fireweed", ru: "Иван-чай", pl: "Wierzbówka" },
    Name { id: "cattail", en: "Cattail", simple: "Cattail", ru: "Рогоз", pl: "Pałka wodna" },
    Name { id: "nettle", en: "Nettle", simple: "Nettle", ru: "Крапива", pl: "Pokrzywa" },
    Name { id: "bracken", en: "Bracken", simple: "Tall fern", ru: "Орляк", pl: "Orlica" },
    Name { id: "arundo", en: "Giant reed", simple: "Tall reed", ru: "Арундо", pl: "Arundo" },
    Name { id: "cane", en: "Cane", simple: "Reed cane", ru: "Трость", pl: "Trzcina" },
    Name { id: "bilberry", en: "Bilberry", simple: "Bilberry bush", ru: "Черника", pl: "Borówka" },
    Name { id: "bilberry_bare", en: "Picked bilberry", simple: "Bilberry with no berries", ru: "Обобранная черника", pl: "Obrana borówka" },
    Name { id: "strawberry", en: "Wild strawberry", simple: "Wild strawberry", ru: "Земляника", pl: "Poziomka" },
    Name { id: "strawberry_bare", en: "Picked wild strawberry", simple: "Strawberry with no berries", ru: "Обобранная земляника", pl: "Obrana poziomka" },
    Name { id: "plantain", en: "Plantain", simple: "Plantain", ru: "Подорожник", pl: "Babka" },
    Name { id: "fern", en: "Fern", simple: "Fern", ru: "Папоротник", pl: "Paproć" },
    Name { id: "sundew", en: "Sundew", simple: "Sundew", ru: "Росянка", pl: "Rosiczka" },
    Name { id: "fir_log", en: "Fir log", simple: "Fir log", ru: "Еловое бревно", pl: "Jodłowy pień" },
    Name { id: "fir_needles", en: "Fir needles", simple: "Fir needles", ru: "Еловая хвоя", pl: "Igliwie jodły" },
    Name { id: "fir_planks", en: "Fir planks", simple: "Fir boards", ru: "Еловые доски", pl: "Jodłowe deski" },
    Name { id: "pegged_fir_planks", en: "Pegged fir planks", simple: "Pinned fir boards", ru: "Еловые доски на штифтах", pl: "Jodłowe deski na kołkach" },
    Name { id: "sandstone_cobble", en: "Sandstone cobblestone", simple: "Sandstone broken stone", ru: "Песчаниковый булыжник", pl: "Piaskowcowy bruk" },
    Name { id: "sandstone_gravel", en: "Sandstone gravel", simple: "Sandstone gravel", ru: "Песчаниковый гравий", pl: "Piaskowcowy żwir" },
    Name { id: "sandstone_pebble", en: "Sandstone pebble", simple: "Sandstone small stone", ru: "Песчаниковый камушек", pl: "Piaskowcowy kamyk" },
    Name { id: "limestone_cobble", en: "Limestone cobblestone", simple: "Limestone broken stone", ru: "Известняковый булыжник", pl: "Wapienny bruk" },
    Name { id: "limestone_gravel", en: "Limestone gravel", simple: "Limestone gravel", ru: "Известняковый гравий", pl: "Wapienny żwir" },
    Name { id: "limestone_sand", en: "Limestone sand", simple: "Limestone sand", ru: "Известняковый песок", pl: "Wapienny piasek" },
    Name { id: "limestone_pebble", en: "Limestone pebble", simple: "Limestone small stone", ru: "Известняковый камушек", pl: "Wapienny kamyk" },
    Name { id: "granite_cobble", en: "Granite cobblestone", simple: "Granite broken stone", ru: "Гранитный булыжник", pl: "Granitowy bruk" },
    Name { id: "granite_gravel", en: "Granite gravel", simple: "Granite gravel", ru: "Гранитный гравий", pl: "Granitowy żwir" },
    Name { id: "granite_sand", en: "Granite sand", simple: "Granite sand", ru: "Гранитный песок", pl: "Granitowy piasek" },
    Name { id: "granite_pebble", en: "Granite pebble", simple: "Granite small stone", ru: "Гранитный камушек", pl: "Granitowy kamyk" },
    Name { id: "basalt_cobble", en: "Basalt cobblestone", simple: "Basalt broken stone", ru: "Базальтовый булыжник", pl: "Bazaltowy bruk" },
    Name { id: "basalt_gravel", en: "Basalt gravel", simple: "Basalt gravel", ru: "Базальтовый гравий", pl: "Bazaltowy żwir" },
    Name { id: "basalt_sand", en: "Basalt sand", simple: "Basalt sand", ru: "Базальтовый песок", pl: "Bazaltowy piasek" },
    Name { id: "basalt_pebble", en: "Basalt pebble", simple: "Basalt small stone", ru: "Базальтовый камушек", pl: "Bazaltowy kamyk" },
    Name { id: "shale", en: "Shale", simple: "Soft grey rock", ru: "Сланец", pl: "Łupek" },
    Name { id: "shale_cobble", en: "Shale cobblestone", simple: "Shale broken stone", ru: "Сланцевый булыжник", pl: "Łupkowy bruk" },
    Name { id: "shale_gravel", en: "Shale gravel", simple: "Shale gravel", ru: "Сланцевый гравий", pl: "Łupkowy żwir" },
    Name { id: "shale_sand", en: "Shale sand", simple: "Shale sand", ru: "Сланцевый песок", pl: "Łupkowy piasek" },
    Name { id: "shale_pebble", en: "Shale pebble", simple: "Shale small stone", ru: "Сланцевый камушек", pl: "Łupkowy kamyk" },
    Name { id: "chalk", en: "Chalk", simple: "White soft rock", ru: "Мел", pl: "Kreda" },
    Name { id: "chalk_cobble", en: "Chalk cobblestone", simple: "Chalk broken stone", ru: "Меловой булыжник", pl: "Kredowy bruk" },
    Name { id: "chalk_gravel", en: "Chalk gravel", simple: "Chalk gravel", ru: "Меловой гравий", pl: "Kredowy żwir" },
    Name { id: "chalk_sand", en: "Chalk sand", simple: "Chalk sand", ru: "Меловой песок", pl: "Kredowy piasek" },
    Name { id: "chalk_pebble", en: "Chalk pebble", simple: "Chalk small stone", ru: "Меловой камушек", pl: "Kredowy kamyk" },
    Name { id: "dolomite", en: "Dolomite", simple: "Yellow-grey rock", ru: "Доломит", pl: "Dolomit" },
    Name { id: "dolomite_cobble", en: "Dolomite cobblestone", simple: "Dolomite broken stone", ru: "Доломитовый булыжник", pl: "Dolomitowy bruk" },
    Name { id: "dolomite_gravel", en: "Dolomite gravel", simple: "Dolomite gravel", ru: "Доломитовый гравий", pl: "Dolomitowy żwir" },
    Name { id: "dolomite_sand", en: "Dolomite sand", simple: "Dolomite sand", ru: "Доломитовый песок", pl: "Dolomitowy piasek" },
    Name { id: "dolomite_pebble", en: "Dolomite pebble", simple: "Dolomite small stone", ru: "Доломитовый камушек", pl: "Dolomitowy kamyk" },
    Name { id: "marble", en: "Marble", simple: "Marble", ru: "Мрамор", pl: "Marmur" },
    Name { id: "marble_cobble", en: "Marble cobblestone", simple: "Marble broken stone", ru: "Мраморный булыжник", pl: "Marmurowy bruk" },
    Name { id: "marble_gravel", en: "Marble gravel", simple: "Marble gravel", ru: "Мраморный гравий", pl: "Marmurowy żwir" },
    Name { id: "marble_sand", en: "Marble sand", simple: "Marble sand", ru: "Мраморный песок", pl: "Marmurowy piasek" },
    Name { id: "marble_pebble", en: "Marble pebble", simple: "Marble small stone", ru: "Мраморный камушек", pl: "Marmurowy kamyk" },
    Name { id: "quartzite", en: "Quartzite", simple: "Glassy hard rock", ru: "Кварцит", pl: "Kwarcyt" },
    Name { id: "quartzite_cobble", en: "Quartzite cobblestone", simple: "Quartzite broken stone", ru: "Кварцитовый булыжник", pl: "Kwarcytowy bruk" },
    Name { id: "quartzite_gravel", en: "Quartzite gravel", simple: "Quartzite gravel", ru: "Кварцитовый гравий", pl: "Kwarcytowy żwir" },
    Name { id: "quartzite_sand", en: "Quartzite sand", simple: "Quartzite sand", ru: "Кварцитовый песок", pl: "Kwarcytowy piasek" },
    Name { id: "quartzite_pebble", en: "Quartzite pebble", simple: "Quartzite small stone", ru: "Кварцитовый камушек", pl: "Kwarcytowy kamyk" },
    Name { id: "gneiss", en: "Gneiss", simple: "Striped rock", ru: "Гнейс", pl: "Gnejs" },
    Name { id: "gneiss_cobble", en: "Gneiss cobblestone", simple: "Gneiss broken stone", ru: "Гнейсовый булыжник", pl: "Gnejsowy bruk" },
    Name { id: "gneiss_gravel", en: "Gneiss gravel", simple: "Gneiss gravel", ru: "Гнейсовый гравий", pl: "Gnejsowy żwir" },
    Name { id: "gneiss_sand", en: "Gneiss sand", simple: "Gneiss sand", ru: "Гнейсовый песок", pl: "Gnejsowy piasek" },
    Name { id: "gneiss_pebble", en: "Gneiss pebble", simple: "Gneiss small stone", ru: "Гнейсовый камушек", pl: "Gnejsowy kamyk" },
    Name { id: "diorite", en: "Diorite", simple: "Speckled rock", ru: "Диорит", pl: "Dioryt" },
    Name { id: "diorite_cobble", en: "Diorite cobblestone", simple: "Diorite broken stone", ru: "Диоритовый булыжник", pl: "Diorytowy bruk" },
    Name { id: "diorite_gravel", en: "Diorite gravel", simple: "Diorite gravel", ru: "Диоритовый гравий", pl: "Diorytowy żwir" },
    Name { id: "diorite_sand", en: "Diorite sand", simple: "Diorite sand", ru: "Диоритовый песок", pl: "Diorytowy piasek" },
    Name { id: "diorite_pebble", en: "Diorite pebble", simple: "Diorite small stone", ru: "Диоритовый камушек", pl: "Diorytowy kamyk" },
    Name { id: "gabbro", en: "Gabbro", simple: "Dark deep rock", ru: "Габбро", pl: "Gabro" },
    Name { id: "gabbro_cobble", en: "Gabbro cobblestone", simple: "Gabbro broken stone", ru: "Габбровый булыжник", pl: "Gabrowy bruk" },
    Name { id: "gabbro_gravel", en: "Gabbro gravel", simple: "Gabbro gravel", ru: "Габбровый гравий", pl: "Gabrowy żwir" },
    Name { id: "gabbro_sand", en: "Gabbro sand", simple: "Gabbro sand", ru: "Габбровый песок", pl: "Gabrowy piasek" },
    Name { id: "gabbro_pebble", en: "Gabbro pebble", simple: "Gabbro small stone", ru: "Габбровый камушек", pl: "Gabrowy kamyk" },
    Name { id: "andesite", en: "Andesite", simple: "Grey lava rock", ru: "Андезит", pl: "Andezyt" },
    Name { id: "andesite_cobble", en: "Andesite cobblestone", simple: "Andesite broken stone", ru: "Андезитовый булыжник", pl: "Andezytowy bruk" },
    Name { id: "andesite_gravel", en: "Andesite gravel", simple: "Andesite gravel", ru: "Андезитовый гравий", pl: "Andezytowy żwir" },
    Name { id: "andesite_sand", en: "Andesite sand", simple: "Andesite sand", ru: "Андезитовый песок", pl: "Andezytowy piasek" },
    Name { id: "andesite_pebble", en: "Andesite pebble", simple: "Andesite small stone", ru: "Андезитовый камушек", pl: "Andezytowy kamyk" },
    Name { id: "tuff", en: "Tuff", simple: "Ash rock", ru: "Туф", pl: "Tuf" },
    Name { id: "tuff_cobble", en: "Tuff cobblestone", simple: "Tuff broken stone", ru: "Туфовый булыжник", pl: "Tufowy bruk" },
    Name { id: "tuff_gravel", en: "Tuff gravel", simple: "Tuff gravel", ru: "Туфовый гравий", pl: "Tufowy żwir" },
    Name { id: "tuff_sand", en: "Tuff sand", simple: "Tuff sand", ru: "Туфовый песок", pl: "Tufowy piasek" },
    Name { id: "tuff_pebble", en: "Tuff pebble", simple: "Tuff small stone", ru: "Туфовый камушек", pl: "Tufowy kamyk" },
    Name { id: "loam", en: "Loam", simple: "Brown earth", ru: "Суглинок", pl: "Glina" },
    Name { id: "chernozem", en: "Chernozem", simple: "Black earth", ru: "Чернозём", pl: "Czarnoziem" },
    Name { id: "podzol", en: "Podzol", simple: "Ashy earth", ru: "Подзол", pl: "Bielica" },
    Name { id: "laterite", en: "Laterite", simple: "Red hard earth", ru: "Латерит", pl: "Lateryt" },
    Name { id: "solonchak", en: "Solonchak", simple: "Salty earth", ru: "Солончак", pl: "Solończak" },
    Name { id: "loess", en: "Loess", simple: "Pale dust earth", ru: "Лёсс", pl: "Less" },
    Name { id: "gley", en: "Gley", simple: "Wet grey earth", ru: "Глей", pl: "Glej" },
    Name { id: "rendzina", en: "Rendzina", simple: "Chalky earth", ru: "Рендзина", pl: "Rędzina" },
    Name { id: "andosol", en: "Andosol", simple: "Ash earth", ru: "Андосоль", pl: "Andosol" },
    Name { id: "permafrost", en: "Permafrost", simple: "Frozen earth", ru: "Мерзлота", pl: "Wieczna zmarzlina" },
    Name { id: "feather_grass", en: "Feather grass", simple: "Silver grass", ru: "Ковыль", pl: "Ostnica" },
    Name { id: "sedge", en: "Sedge", simple: "Sharp grass", ru: "Осока", pl: "Turzyca" },
    Name { id: "cotton_grass", en: "Cotton grass", simple: "Fluffy grass", ru: "Пушица", pl: "Wełnianka" },
    Name { id: "fescue", en: "Fescue", simple: "Blue mountain grass", ru: "Овсяница", pl: "Kostrzewa" },
    Name { id: "marram", en: "Marram grass", simple: "Dune grass", ru: "Песколюб", pl: "Piaskownica" },
    Name { id: "elephant_grass", en: "Elephant grass", simple: "Tall cane grass", ru: "Слоновая трава", pl: "Trawa słoniowa" },
    Name { id: "bluegrass", en: "Bluegrass", simple: "Soft grass", ru: "Мятлик", pl: "Wiechlina" },
    Name { id: "timothy", en: "Timothy", simple: "Meadow grass", ru: "Тимофеевка", pl: "Tymotka" },
    Name { id: "tussock_grass", en: "Tussock grass", simple: "Clump grass", ru: "Щучка", pl: "Śmiałek" },
    Name { id: "spinifex", en: "Spinifex", simple: "Spiky desert grass", ru: "Спинифекс", pl: "Spinifeks" },
    Name { id: "pine_log", en: "Pine log", simple: "Pine log", ru: "Сосновое бревно", pl: "Sosnowy pień" },
    Name { id: "pine_needles", en: "Pine needles", simple: "Pine needles", ru: "Сосновая хвоя", pl: "Igliwie sosny" },
    Name { id: "pine_planks", en: "Pine planks", simple: "Pine boards", ru: "Сосновые доски", pl: "Sosnowe deski" },
    Name { id: "pegged_pine_planks", en: "Pegged pine planks", simple: "Pinned pine boards", ru: "Сосновые доски на штифтах", pl: "Sosnowe deski na kołkach" },
    Name { id: "willow_log", en: "Willow log", simple: "Willow log", ru: "Ивовое бревно", pl: "Wierzbowy pień" },
    Name { id: "willow_leaves", en: "Willow leaves", simple: "Willow leaves", ru: "Ивовая листва", pl: "Liście wierzby" },
    Name { id: "willow_planks", en: "Willow planks", simple: "Willow boards", ru: "Ивовые доски", pl: "Wierzbowe deski" },
    Name { id: "pegged_willow_planks", en: "Pegged willow planks", simple: "Pinned willow boards", ru: "Ивовые доски на штифтах", pl: "Wierzbowe deski na kołkach" },
    Name { id: "fir_twig", en: "Fir twig", simple: "Fir twig", ru: "Еловая ветка", pl: "Gałązka jodły" },
    Name { id: "fir_bough", en: "Fir bough", simple: "Thick fir branch", ru: "Еловый сук", pl: "Konar jodły" },
    Name { id: "saxaul_twig", en: "Saxaul twig", simple: "Saxaul twig", ru: "Саксауловая ветка", pl: "Gałązka saksaułu" },
    Name { id: "saxaul_bough", en: "Saxaul bough", simple: "Thick saxaul branch", ru: "Саксауловый сук", pl: "Konar saksaułu" },
    Name { id: "pine_twig", en: "Pine twig", simple: "Pine twig", ru: "Сосновая ветка", pl: "Gałązka sosny" },
    Name { id: "pine_bough", en: "Pine bough", simple: "Thick pine branch", ru: "Сосновый сук", pl: "Konar sosny" },
    Name { id: "willow_twig", en: "Willow twig", simple: "Willow twig", ru: "Ивовая ветка", pl: "Gałązka wierzby" },
    Name { id: "willow_bough", en: "Willow bough", simple: "Thick willow branch", ru: "Ивовый сук", pl: "Konar wierzby" },
    Name { id: "moss", en: "Moss", simple: "Moss", ru: "Мох", pl: "Mech" },
    Name { id: "saxaul_log", en: "Saxaul log", simple: "Desert tree log", ru: "Бревно саксаула", pl: "Pień saksaułu" },
    Name { id: "saxaul_leaves", en: "Saxaul leaves", simple: "Desert tree leaves", ru: "Листва саксаула", pl: "Liście saksaułu" },
    Name { id: "saxaul_planks", en: "Saxaul planks", simple: "Desert tree boards", ru: "Доски саксаула", pl: "Deski z saksaułu" },
    Name { id: "pegged_saxaul_planks", en: "Pegged saxaul planks", simple: "Pinned desert tree boards", ru: "Доски саксаула на штифтах", pl: "Deski z saksaułu na kołkach" },
    Name { id: "corpse", en: "Body", simple: "Dead body", ru: "Тело", pl: "Ciało" },
    Name { id: "remains", en: "Remains", simple: "Old dead body", ru: "Останки", pl: "Szczątki" },
    Name { id: "set_down", en: "Set down", simple: "Thing on the ground", ru: "Положенный предмет", pl: "Położony przedmiot" },
];

/// Every recipe, by `Recipe::name`, in `crafting::RECIPES`'s order.
///
/// A recipe's name is the thing made, as `Recipe::name` asks, and the
/// translations keep it a noun where the language allows ("Обжиг
/// горшка", not "обжечь горшок"): the book's column is a list of things,
/// and a row that reads as an order is a row that looks like a button.
/// "br. cuirass" is shortened in the source for the column's width, and
/// the translations are shortened the same way for the same reason.
pub const RECIPES: &[Name] = &[
    Name { id: "planks", en: "Planks", simple: "Boards", ru: "Доски", pl: "Deski" },
    Name { id: "beam", en: "Beam", simple: "Log from boards", ru: "Брус", pl: "Belka" },
    Name { id: "split cobble", en: "Split cobble", simple: "Broken cobblestone", ru: "Колотый булыжник", pl: "Rozbity bruk" },
    Name { id: "mulch", en: "Mulch", simple: "Leaf soil", ru: "Перегной", pl: "Ściółka" },
    Name { id: "turf", en: "Turf", simple: "Grass ground", ru: "Дёрн", pl: "Darń" },
    Name { id: "sand", en: "Sand", simple: "Sand", ru: "Песок", pl: "Piasek" },
    Name { id: "sticks", en: "Sticks", simple: "Sticks", ru: "Палки", pl: "Patyki" },
    Name { id: "thatch", en: "Thatch", simple: "Grass soil", ru: "Травяной настил", pl: "Strzecha" },
    Name { id: "grass tuft", en: "Grass tuft", simple: "Tuft of grass", ru: "Пучок травы", pl: "Kępka trawy" },
    Name { id: "knapped stone", en: "Knapped stone", simple: "Stone from pebbles", ru: "Сбитый булыжник", pl: "Łupany kamień" },
    Name { id: "bound sticks", en: "Bound sticks", simple: "Sticks from a board", ru: "Связанные палки", pl: "Związane patyki" },
    Name { id: "flint knapping", en: "Flint knapping", simple: "Stone from flint", ru: "Оббитый кремень", pl: "Łupanie krzemienia" },
    Name { id: "chest", en: "Chest", simple: "Storage box", ru: "Сундук", pl: "Skrzynia" },
    Name { id: "sifted gravel", en: "Sifted gravel", simple: "Flint from gravel", ru: "Просеянный гравий", pl: "Przesiany żwir" },
    Name { id: "birch planks", en: "Birch planks", simple: "Birch boards", ru: "Берёзовые доски", pl: "Brzozowe deski" },
    Name { id: "birch beam", en: "Birch beam", simple: "Birch log from boards", ru: "Берёзовый брус", pl: "Brzozowa belka" },
    Name { id: "bundle leaves", en: "Bundle leaves", simple: "Leaf bundle", ru: "Связка листвы", pl: "Wiązka liści" },
    Name { id: "flint flakes", en: "Flint flakes", simple: "Sharp flint chips", ru: "Отщепы", pl: "Odłupki" },
    Name { id: "worked stick", en: "Worked stick", simple: "Shaped stick", ru: "Обработанное древко", pl: "Obrobiony kij" },
    Name { id: "knife head", en: "Knife head", simple: "Flint blade", ru: "Головка ножа", pl: "Ostrze noża" },
    Name { id: "axe head", en: "Axe head", simple: "Stone axe head", ru: "Головка топора", pl: "Głowica topora" },
    Name { id: "pick head", en: "Pick head", simple: "Stone pick head", ru: "Головка кирки", pl: "Głowica kilofa" },
    Name { id: "flint knife", en: "Flint knife", simple: "Flint knife", ru: "Кремнёвый нож", pl: "Krzemienny nóż" },
    Name { id: "torch", en: "Torch", simple: "Torch", ru: "Факел", pl: "Pochodnia" },
    Name { id: "haft torch", en: "Haft torch", simple: "Torch on a shaped stick", ru: "Факел на древке", pl: "Pochodnia na drzewcu" },
    Name { id: "rewad torch", en: "Rewad torch", simple: "New grass on a torch", ru: "Новая обмотка факела", pl: "Nowa owijka pochodni" },
    Name { id: "fungus torch", en: "Fungus torch", simple: "Tree fungus torch", ru: "Факел с трутовиком", pl: "Pochodnia z hubą" },
    Name { id: "fungus rewad", en: "Fungus rewad", simple: "New fungus on a torch", ru: "Обмотка трутовиком", pl: "Owijka z huby" },
    Name { id: "stone axe", en: "Stone axe", simple: "Stone axe", ru: "Каменный топор", pl: "Kamienny topór" },
    Name { id: "sinewed axe", en: "Sinewed axe", simple: "Axe tied with animal string", ru: "Топор на жиле", pl: "Topór na ścięgnie" },
    Name { id: "sinewed pick", en: "Sinewed pick", simple: "Pick tied with animal string", ru: "Кирка на жиле", pl: "Kilof na ścięgnie" },
    Name { id: "sinewed knife", en: "Sinewed knife", simple: "Knife tied with animal string", ru: "Нож на жиле", pl: "Nóż na ścięgnie" },
    Name { id: "cord", en: "Cord", simple: "Rope", ru: "Верёвка", pl: "Sznur" },
    Name { id: "pegs", en: "Pegs", simple: "Wooden pegs", ru: "Штифты", pl: "Kołki" },
    Name { id: "wedged axe", en: "Wedged axe", simple: "Strong stone axe", ru: "Расклиненный топор", pl: "Topór klinowany" },
    Name { id: "wedged pick", en: "Wedged pick", simple: "Strong stone pick", ru: "Расклиненная кирка", pl: "Kilof klinowany" },
    Name { id: "flint spear", en: "Flint spear", simple: "Flint spear", ru: "Кремнёвое копьё", pl: "Włócznia krzemienna" },
    Name { id: "poison spear", en: "Poison spear", simple: "Poison on a flint spear", ru: "Отравленное копьё", pl: "Zatruta włócznia" },
    Name { id: "poison bone", en: "Poison bone spear", simple: "Poison on a bone spear", ru: "Отравленное костяное", pl: "Zatruta kościana" },
    Name { id: "poison copper", en: "Poison copper spear", simple: "Poison on a copper spear", ru: "Отравленное медное", pl: "Zatruta miedziana" },
    Name { id: "poison bronze", en: "Poison bronze spear", simple: "Poison on a bronze spear", ru: "Отравленное бронзовое", pl: "Zatruta z brązu" },
    Name { id: "poison iron", en: "Poison iron spear", simple: "Poison on an iron spear", ru: "Отравленное железное", pl: "Zatruta żelazna" },
    Name { id: "bone spear", en: "Bone spear", simple: "Bone spear", ru: "Костяное копьё", pl: "Włócznia kościana" },
    Name { id: "copper spear", en: "Copper spear", simple: "Copper spear", ru: "Медное копьё", pl: "Włócznia miedziana" },
    Name { id: "bronze spear", en: "Bronze spear", simple: "Bronze spear", ru: "Бронзовое копьё", pl: "Włócznia z brązu" },
    Name { id: "iron spear", en: "Iron spear", simple: "Iron spear", ru: "Железное копьё", pl: "Włócznia żelazna" },
    Name { id: "stone pick", en: "Stone pick", simple: "Stone pick", ru: "Каменная кирка", pl: "Kamienny kilof" },
    Name { id: "copper ingot", en: "Copper ingot", simple: "Copper bar", ru: "Медный слиток", pl: "Sztabka miedzi" },
    Name { id: "tin ingot", en: "Tin ingot", simple: "Tin bar", ru: "Оловянный слиток", pl: "Sztabka cyny" },
    Name { id: "bronze ingot", en: "Bronze ingot", simple: "Bronze bar", ru: "Бронзовый слиток", pl: "Sztabka brązu" },
    Name { id: "iron bloom", en: "Iron bloom", simple: "Lump of raw iron", ru: "Крица", pl: "Łupa żelaza" },
    Name { id: "iron dust", en: "Iron dust", simple: "Iron powder", ru: "Железная пыль", pl: "Pył żelazny" },
    Name { id: "bog iron bloom", en: "Bog iron bloom", simple: "Iron lump from rusty stones", ru: "Болотная крица", pl: "Łupa z rudy darniowej" },
    Name { id: "wrought iron", en: "Wrought iron", simple: "Iron bar from a lump", ru: "Кричное железо", pl: "Żelazo kute" },
    Name { id: "campfire", en: "Campfire", simple: "Campfire", ru: "Костёр", pl: "Ognisko" },
    Name { id: "reed fibre", en: "Reed fibre", simple: "Fibre from reeds", ru: "Волокно из камыша", pl: "Włókno z sitowia" },
    Name { id: "reed thatch", en: "Reed thatch", simple: "Reed soil", ru: "Камышовый настил", pl: "Strzecha z sitowia" },
    Name { id: "cooked meat", en: "Cooked meat", simple: "Cooked meat", ru: "Жареное мясо", pl: "Pieczone mięso" },
    Name { id: "cook hare", en: "Cooked hare", simple: "Cooked hare meat", ru: "Жареная зайчатина", pl: "Pieczona zajęczyna" },
    Name { id: "cook fowl", en: "Cooked fowl", simple: "Cooked bird meat", ru: "Жареная птица", pl: "Pieczony drób" },
    Name { id: "cook bear", en: "Cooked bear", simple: "Cooked bear meat", ru: "Жареная медвежатина", pl: "Pieczona niedźwiedzina" },
    Name { id: "cook wolf", en: "Cooked wolf", simple: "Cooked wolf meat", ru: "Жареная волчатина", pl: "Pieczona wilczyna" },
    Name { id: "roast flesh", en: "Roast flesh", simple: "Cooked human meat", ru: "Жареная человечина", pl: "Pieczone ludzkie mięso" },
    Name { id: "cook fish", en: "Cooked fish", simple: "Cooked fish", ru: "Жареная рыба", pl: "Pieczona ryba" },
    Name { id: "planter", en: "Planter", simple: "Pot of earth", ru: "Горшок с землёй", pl: "Donica" },
    Name { id: "pit prop", en: "Pit prop", simple: "Roof post", ru: "Подпорка", pl: "Stempel" },
    Name { id: "stakes", en: "Stakes", simple: "Sharpened poles", ru: "Колья", pl: "Kołki" },
    Name { id: "window lattice", en: "Window lattice", simple: "Window grid", ru: "Оконная решётка", pl: "Krata okienna" },
    // ---- the hammer, the chisel and the two mini-games ----
    Name { id: "stone hammer", en: "Stone hammer", simple: "Stone hammer", ru: "Каменный молот", pl: "Kamienny młot" },
    Name { id: "bronze hammer", en: "Bronze hammer", simple: "Bronze hammer", ru: "Бронзовый молот", pl: "Brązowy młot" },
    Name { id: "iron hammer", en: "Iron hammer", simple: "Iron hammer", ru: "Железный молот", pl: "Żelazny młot" },
    Name { id: "flint chisel", en: "Flint chisel", simple: "Flint chisel", ru: "Кремнёвая стамеска", pl: "Krzemienne dłuto" },
    Name { id: "bronze chisel", en: "Bronze chisel", simple: "Bronze chisel", ru: "Бронзовая стамеска", pl: "Brązowe dłuto" },
    Name { id: "anvil", en: "Anvil", simple: "Anvil", ru: "Наковальня", pl: "Kowadło" },
    Name { id: "pared door", en: "Chiselled door", simple: "Door with a chisel", ru: "Дверь стамеской", pl: "Drzwi dłutem" },
    Name { id: "pared chair", en: "Chiselled chair", simple: "Chair with a chisel", ru: "Стул стамеской", pl: "Krzesło dłutem" },
    Name { id: "pared table", en: "Chiselled table", simple: "Table with a chisel", ru: "Стол стамеской", pl: "Stół dłutem" },
    Name { id: "pared bed", en: "Chiselled bed", simple: "Bed with a chisel", ru: "Кровать стамеской", pl: "Łóżko dłutem" },
    Name { id: "dressed ashlar", en: "Dressed ashlar", simple: "Better stone blocks", ru: "Тёсаный камень", pl: "Ciosany kamień" },
    Name { id: "struck hones", en: "Struck hones", simple: "More sharpening stones", ru: "Колотые точильные камни", pl: "Odbite osełki" },
    Name { id: "fishing fly", en: "Fishing fly", simple: "Fly lure", ru: "Мушка", pl: "Mucha wędkarska" },
    Name { id: "boil salt", en: "Boil salt", simple: "Salt from sea water", ru: "Выпарить соль", pl: "Wywarzyć sól" },
    Name { id: "salt meat", en: "Salt meat", simple: "Meat in salt", ru: "Засолить мясо", pl: "Zasolić mięso" },
    Name { id: "salt fish", en: "Salt fish", simple: "Fish in salt", ru: "Засолить рыбу", pl: "Zasolić rybę" },
    Name { id: "roast ribs", en: "Roast ribs", simple: "Cooked ribs", ru: "Жареные рёбра", pl: "Pieczone żeberka" },
    Name { id: "standing torch", en: "Standing torch", simple: "Torch on a pole", ru: "Факел на шесте", pl: "Pochodnia na tyczce" },
    Name { id: "fat torch", en: "Fat torch", simple: "Torch with fat", ru: "Факел с жиром", pl: "Pochodnia z łojem" },
    Name { id: "fat rewad", en: "Fat rewad", simple: "New fat on a torch", ru: "Обмотка с жиром", pl: "Owijka z łojem" },
    Name { id: "copper knife", en: "Copper knife", simple: "Copper knife", ru: "Медный нож", pl: "Miedziany nóż" },
    Name { id: "hoe casting", en: "Hoe casting", simple: "Poured hoe blade", ru: "Отливка мотыги", pl: "Odlew motyki" },
    Name { id: "shovel casting", en: "Shovel casting", simple: "Poured shovel blade", ru: "Отливка лопаты", pl: "Odlew łopaty" },
    Name { id: "axe casting", en: "Axe casting", simple: "Poured axe head", ru: "Отливка топора", pl: "Odlew topora" },
    Name { id: "pick casting", en: "Pick casting", simple: "Poured pick head", ru: "Отливка кирки", pl: "Odlew kilofa" },
    Name { id: "copper axe", en: "Copper axe", simple: "Copper axe", ru: "Медный топор", pl: "Miedziany topór" },
    Name { id: "copper pick", en: "Copper pick", simple: "Copper pick", ru: "Медная кирка", pl: "Miedziany kilof" },
    Name { id: "copper shovel", en: "Copper shovel", simple: "Copper shovel", ru: "Медная лопата", pl: "Miedziana łopata" },
    Name { id: "copper hoe", en: "Copper hoe", simple: "Copper hoe", ru: "Медная мотыга", pl: "Miedziana motyka" },
    Name { id: "bronze knife", en: "Bronze knife", simple: "Bronze knife", ru: "Бронзовый нож", pl: "Brązowy nóż" },
    Name { id: "bronze axe", en: "Bronze axe", simple: "Bronze axe", ru: "Бронзовый топор", pl: "Brązowy topór" },
    Name { id: "bronze pick", en: "Bronze pick", simple: "Bronze pick", ru: "Бронзовая кирка", pl: "Brązowy kilof" },
    Name { id: "iron knife", en: "Iron knife", simple: "Iron knife", ru: "Железный нож", pl: "Żelazny nóż" },
    Name { id: "iron axe", en: "Iron axe", simple: "Iron axe", ru: "Железный топор", pl: "Żelazny topór" },
    Name { id: "iron pick", en: "Iron pick", simple: "Iron pick", ru: "Железная кирка", pl: "Żelazny kilof" },
    Name { id: "charcoal", en: "Charcoal", simple: "Charcoal", ru: "Древесный уголь", pl: "Węgiel drzewny" },
    Name { id: "kiln", en: "Kiln", simple: "Clay oven", ru: "Горн", pl: "Piec" },
    Name { id: "bricks", en: "Bricks", simple: "Bricks", ru: "Кирпичи", pl: "Cegły" },
    Name { id: "brickwork", en: "Brickwork", simple: "Brick wall", ru: "Кирпичная кладка", pl: "Mur z cegieł" },
    Name { id: "sand brickwork", en: "Sandstone brickwork", simple: "Sandstone wall", ru: "Кладка из песчаника", pl: "Mur z piaskowca" },
    Name { id: "hoe", en: "Hoe", simple: "Hoe", ru: "Мотыга", pl: "Motyka" },
    Name { id: "bone hoe", en: "Bone hoe", simple: "Bone hoe", ru: "Костяная мотыга", pl: "Motyka kościana" },
    Name { id: "grind grain", en: "Ground grain", simple: "Flour", ru: "Помол зерна", pl: "Mielenie ziarna" },
    Name { id: "dough", en: "Dough", simple: "Dough", ru: "Тесто", pl: "Ciasto" },
    Name { id: "bread", en: "Bread", simple: "Bread", ru: "Хлеб", pl: "Chleb" },
    Name { id: "clay vessel", en: "Clay crucible", simple: "Wet clay pot", ru: "Сырой горшок", pl: "Gliniany tygiel" },
    Name { id: "fire vessel", en: "Fired crucible", simple: "Baked melting pot", ru: "Обжиг горшка", pl: "Wypalanie tygla" },
    Name { id: "ingot mould", en: "Ingot mould", simple: "Bar mould", ru: "Форма для слитка", pl: "Forma na sztabkę" },
    Name { id: "fire mould", en: "Fired mould", simple: "Baked bar mould", ru: "Обжиг формы", pl: "Wypalanie formy" },
    Name { id: "bloomery", en: "Bloomery", simple: "Iron furnace", ru: "Домница", pl: "Dymarka" },
    Name { id: "melt nuggets", en: "Melt nuggets", simple: "Melted copper nuggets", ru: "Переплавка самородков", pl: "Przetop samorodków" },
    Name { id: "drying rack", en: "Drying rack", simple: "Meat and fish rack", ru: "Сушилка", pl: "Suszarnia" },
    Name { id: "hide cap", en: "Hide cap", simple: "Leather hat", ru: "Кожаная шапка", pl: "Skórzana czapka" },
    Name { id: "hide tunic", en: "Hide tunic", simple: "Leather shirt", ru: "Кожаная рубаха", pl: "Skórzana tunika" },
    Name { id: "hide leggings", en: "Hide leggings", simple: "Leather trousers", ru: "Кожаные штаны", pl: "Skórzane spodnie" },
    Name { id: "hide boots", en: "Hide boots", simple: "Leather boots", ru: "Кожаные сапоги", pl: "Skórzane buty" },
    Name { id: "hide rucksack", en: "Hide rucksack", simple: "Bag for your back", ru: "Кожаный рюкзак", pl: "Skórzany plecak" },
    Name { id: "straw bed", en: "Straw bed", simple: "Grass bed", ru: "Соломенное лежбище", pl: "Posłanie ze słomy" },
    Name { id: "bed", en: "Bed", simple: "Bed", ru: "Кровать", pl: "Łóżko" },
    Name { id: "stool", en: "Stool", simple: "Stool", ru: "Табурет", pl: "Stołek" },
    Name { id: "chair", en: "Chair", simple: "Chair", ru: "Стул", pl: "Krzesło" },
    Name { id: "table", en: "Table", simple: "Table", ru: "Стол", pl: "Stół" },
    Name { id: "barrel", en: "Barrel", simple: "Barrel", ru: "Бочка", pl: "Beczka" },
    Name { id: "door", en: "Door", simple: "Door", ru: "Дверь", pl: "Drzwi" },
    Name { id: "fur hood", en: "Fur hood", simple: "Fur hood", ru: "Меховой капюшон", pl: "Futrzany kaptur" },
    Name { id: "bear hood", en: "Bear hood", simple: "Bear skin hood", ru: "Капюшон из медведя", pl: "Kaptur z niedźwiedzia" },
    Name { id: "fur cloak", en: "Fur cloak", simple: "Fur cloak", ru: "Меховая накидка", pl: "Futrzana peleryna" },
    Name { id: "bear cloak", en: "Bear cloak", simple: "Bear skin cloak", ru: "Накидка из медведя", pl: "Peleryna z niedźwiedzia" },
    Name { id: "wool cap", en: "Wool cap", simple: "Wool hat", ru: "Шерстяная шапка", pl: "Wełniana czapka" },
    Name { id: "wool tunic", en: "Wool tunic", simple: "Wool shirt", ru: "Шерстяная рубаха", pl: "Wełniana tunika" },
    Name { id: "wool leggings", en: "Wool leggings", simple: "Wool trousers", ru: "Шерстяные штаны", pl: "Wełniane spodnie" },
    Name { id: "wool boots", en: "Wool boots", simple: "Wool boots", ru: "Шерстяные сапоги", pl: "Wełniane buty" },
    Name { id: "cloth", en: "Cloth", simple: "Cloth", ru: "Ткань", pl: "Tkanina" },
    Name { id: "cloth cap", en: "Cloth cap", simple: "Cloth hat", ru: "Тканевая шапка", pl: "Płócienna czapka" },
    Name { id: "cloth tunic", en: "Cloth tunic", simple: "Cloth shirt", ru: "Тканевая рубаха", pl: "Płócienna tunika" },
    Name { id: "cloth trousers", en: "Cloth trousers", simple: "Cloth trousers", ru: "Тканевые штаны", pl: "Płócienne spodnie" },
    Name { id: "cloth wraps", en: "Cloth wraps", simple: "Cloth foot wraps", ru: "Тканевые обмотки", pl: "Płócienne onuce" },
    Name { id: "bronze helm", en: "Bronze helm", simple: "Bronze helmet", ru: "Бронзовый шлем", pl: "Hełm z brązu" },
    Name { id: "br. cuirass", en: "Br. cuirass", simple: "Br. chest armour", ru: "Бронз. кираса", pl: "Kirys z brązu" },
    Name { id: "br. greaves", en: "Br. greaves", simple: "Br. leg armour", ru: "Бронз. поножи", pl: "Brąz. nagolenniki" },
    Name { id: "bronze boots", en: "Bronze boots", simple: "Bronze boots", ru: "Бронзовые сапоги", pl: "Buty z brązu" },
    Name { id: "iron helm", en: "Iron helm", simple: "Iron helmet", ru: "Железный шлем", pl: "Żelazny hełm" },
    Name { id: "iron cuirass", en: "Iron cuirass", simple: "Iron chest armour", ru: "Железная кираса", pl: "Żelazny kirys" },
    Name { id: "iron greaves", en: "Iron greaves", simple: "Iron leg armour", ru: "Железные поножи", pl: "Żel. nagolenniki" },
    Name { id: "iron boots", en: "Iron boots", simple: "Iron boots", ru: "Железные сапоги", pl: "Żelazne buty" },
    Name { id: "clay jug", en: "Clay jug", simple: "Wet clay jug", ru: "Сырой кувшин", pl: "Gliniany dzban" },
    Name { id: "fire jug", en: "Fired jug", simple: "Baked jug", ru: "Обжиг кувшина", pl: "Wypalanie dzbana" },
    Name { id: "bare planks", en: "Bare planks", simple: "Boards from a bare log", ru: "Доски из окорённого", pl: "Deski z okorowanego" },
    Name { id: "strip a log", en: "Strip a log", simple: "Bark off a log", ru: "Окорить бревно", pl: "Okorowanie pnia" },
    Name { id: "strip birch", en: "Strip birch", simple: "Bark off a birch log", ru: "Окорить берёзу", pl: "Okorowanie brzozy" },
    Name { id: "roast a root", en: "Roast a root", simple: "Cooked root", ru: "Печёный корень", pl: "Pieczony korzeń" },
    Name { id: "bandage", en: "Bandage", simple: "Bandage", ru: "Бинт", pl: "Bandaż" },
    Name { id: "cloth bandages", en: "Cloth bandages", simple: "Bandages from cloth", ru: "Бинты из ткани", pl: "Bandaże z tkaniny" },
    Name { id: "splint", en: "Splint", simple: "Splint for a bone", ru: "Шина", pl: "Szyna" },
    Name { id: "poultice", en: "Poultice", simple: "Leaf dressing", ru: "Припарка", pl: "Okład" },
    Name { id: "sail", en: "Sail", simple: "Sail", ru: "Парус", pl: "Żagiel" },
    Name { id: "cloth sail", en: "Cloth sail", simple: "Sail from cloth", ru: "Парус из ткани", pl: "Żagiel z tkaniny" },
    Name { id: "oar", en: "Oar", simple: "Oar", ru: "Весло", pl: "Wiosło" },
    Name { id: "raft", en: "Raft", simple: "Raft", ru: "Плот", pl: "Tratwa" },
    Name { id: "raw bricks", en: "Raw bricks", simple: "Wet clay bricks", ru: "Сырые кирпичи", pl: "Surowe cegły" },
    Name { id: "fire bricks", en: "Fired bricks", simple: "Baked bricks", ru: "Обжиг кирпичей", pl: "Wypalanie cegieł" },
    Name { id: "leaf poultice", en: "Leaf poultice", simple: "Leaf dressing", ru: "Припарка из листьев", pl: "Okład z liści" },
    Name { id: "birch charcoal", en: "Birch charcoal", simple: "Charcoal from birch", ru: "Берёзовый уголь", pl: "Węgiel z brzozy" },
    Name { id: "whetstone", en: "Whetstone", simple: "Sharpening stone", ru: "Точильный камень", pl: "Osełka" },
    Name { id: "steel", en: "Steel", simple: "Steel", ru: "Сталь", pl: "Stal" },
    Name { id: "steel knife", en: "Steel knife", simple: "Hardened knife", ru: "Стальной нож", pl: "Stalowy nóż" },
    Name { id: "steel axe", en: "Steel axe", simple: "Hardened axe", ru: "Стальной топор", pl: "Stalowy topór" },
    Name { id: "steel pick", en: "Steel pick", simple: "Hardened pick", ru: "Стальная кирка", pl: "Stalowy kilof" },
    Name { id: "hone", en: "Hone", simple: "Sharpen", ru: "Заточка", pl: "Ostrzenie" },
    Name { id: "fir planks", en: "Fir planks", simple: "Fir boards", ru: "Еловые доски", pl: "Jodłowe deski" },
    Name { id: "fir beam", en: "Fir beam", simple: "Fir log from boards", ru: "Еловый брус", pl: "Jodłowa belka" },
    Name { id: "pine planks", en: "Pine planks", simple: "Pine boards", ru: "Сосновые доски", pl: "Sosnowe deski" },
    Name { id: "pine beam", en: "Pine beam", simple: "Pine log from boards", ru: "Сосновый брус", pl: "Sosnowa belka" },
    Name { id: "pine charcoal", en: "Pine charcoal", simple: "Charcoal from pine", ru: "Сосновый уголь", pl: "Węgiel sosnowy" },
    Name { id: "willow planks", en: "Willow planks", simple: "Willow boards", ru: "Ивовые доски", pl: "Wierzbowe deski" },
    Name { id: "willow beam", en: "Willow beam", simple: "Willow log from boards", ru: "Ивовый брус", pl: "Wierzbowa belka" },
    Name { id: "willow coal", en: "Willow coal", simple: "Charcoal from willow", ru: "Ивовый уголь", pl: "Węgiel wierzbowy" },
    Name { id: "moss dressing", en: "Moss dressing", simple: "Bandage of moss", ru: "Моховая перевязка", pl: "Opatrunek z mchu" },
    Name { id: "workbench", en: "Workbench", simple: "Work table", ru: "Верстак", pl: "Warsztat" },
    Name { id: "mason block", en: "Mason's block", simple: "Stone work block", ru: "Колода каменотёса", pl: "Blok kamieniarski" },
    Name { id: "potter's wheel", en: "Potter's wheel", simple: "Clay wheel", ru: "Гончарный круг", pl: "Koło garncarskie" },
    Name { id: "leather bench", en: "Leather bench", simple: "Leather table", ru: "Скорняжный стол", pl: "Stół rymarski" },
    Name { id: "bench frame", en: "Bench frame", simple: "Frame without flint", ru: "Рама на верстаке", pl: "Rama przy warsztacie" },
    Name { id: "quern flour", en: "Quern flour", simple: "Flour ground on stone", ru: "Мука с зернотёрки", pl: "Mąka z żaren" },
    Name { id: "ashlar", en: "Ashlar", simple: "Cut stone bricks", ru: "Тёсаный камень", pl: "Ciosy" },
    Name { id: "split hones", en: "Split hones", simple: "Three whetstones", ru: "Колотые оселки", pl: "Łupane osełki" },
    Name { id: "thrown vessel", en: "Thrown crucible", simple: "Wheel-made melting pot", ru: "Тигель на круге", pl: "Tygiel z koła" },
    Name { id: "thrown jug", en: "Thrown jug", simple: "Wheel-made jug", ru: "Кувшин на круге", pl: "Dzban z koła" },
    Name { id: "pinched bowl", en: "Pinched bowl", simple: "Clay bowl by hand", ru: "Лепная миска", pl: "Lepiona miska" },
    Name { id: "thrown bowl", en: "Thrown bowl", simple: "Wheel-made bowl", ru: "Миска на круге", pl: "Miska z koła" },
    Name { id: "fire bowl", en: "Fired bowl", simple: "Baked bowl", ru: "Обжиг миски", pl: "Wypalanie miski" },
    Name { id: "cut cap", en: "Cut cap", simple: "Leather cap, less leather", ru: "Кроеная шапка", pl: "Krojona czapka" },
    Name { id: "cut tunic", en: "Cut tunic", simple: "Leather shirt, less leather", ru: "Кроеная рубаха", pl: "Krojona tunika" },
    Name { id: "cut leggings", en: "Cut leggings", simple: "Leather trousers, less leather", ru: "Кроеные штаны", pl: "Krojone nogawice" },
    Name { id: "cut boots", en: "Cut boots", simple: "Leather boots, less leather", ru: "Кроеные сапоги", pl: "Krojone buty" },
    Name { id: "sewn rucksack", en: "Sewn rucksack", simple: "Bag for your back, less leather", ru: "Шитый рюкзак", pl: "Szyty plecak" },
    Name { id: "roof tiles", en: "Roof tiles", simple: "Baked clay roof", ru: "Обжиг черепицы", pl: "Wypalanie dachówki" },
    Name { id: "plank stairs", en: "Plank stairs", simple: "Wooden steps", ru: "Деревянные ступени", pl: "Drewniane schody" },
    Name { id: "stone stairs", en: "Stone stairs", simple: "Stone steps", ru: "Каменные ступени", pl: "Kamienne schody" },
    Name { id: "tiled roof", en: "Tiled roof", simple: "Clay roof", ru: "Черепичная крыша", pl: "Dach z dachówki" },
    Name { id: "thatch roof", en: "Thatch roof", simple: "Grass roof", ru: "Соломенная кровля", pl: "Strzecha" },
    Name { id: "thatched roof", en: "Thatched roof", simple: "Grass roof", ru: "Соломенная крыша", pl: "Dach kryty strzechą" },
    Name { id: "branch roofing", en: "Branch roofing", simple: "Sticks and leaves roof", ru: "Кровля из веток", pl: "Dach z gałęzi" },
    Name { id: "branch roof", en: "Branch roof", simple: "Stick roof", ru: "Крыша из веток", pl: "Dach z gałęzi" },
    Name { id: "saxaul planks", en: "Saxaul planks", simple: "Desert tree boards", ru: "Доски саксаула", pl: "Deski z saksaułu" },
    Name { id: "saxaul beam", en: "Saxaul beam", simple: "Desert tree log", ru: "Брус саксаула", pl: "Belka z saksaułu" },
    Name { id: "fir charcoal", en: "Fir charcoal", simple: "Charcoal from fir", ru: "Еловый уголь", pl: "Węgiel z jodły" },
    Name { id: "saxaul coal", en: "Saxaul charcoal", simple: "Charcoal from desert tree", ru: "Уголь саксаула", pl: "Węgiel z saksaułu" },
    Name { id: "nails", en: "Nails", simple: "Nails", ru: "Гвозди", pl: "Gwoździe" },
    Name { id: "pegged frame", en: "Pegged frame", simple: "Frame with pegs", ru: "Рама на штифтах", pl: "Rama na kołkach" },
    Name { id: "nailed frame", en: "Nailed frame", simple: "Frame with nails", ru: "Рама на гвоздях", pl: "Rama na gwoździach" },
    Name { id: "nailed chest", en: "Nailed chest", simple: "Storage box with nails", ru: "Сундук на гвоздях", pl: "Skrzynia na gwoździach" },
    Name { id: "fish trap", en: "Fish trap", simple: "Fish trap", ru: "Верша", pl: "Więcierz" },
    Name { id: "copper hooks", en: "Copper hooks", simple: "Fish hooks", ru: "Медные крючки", pl: "Miedziane haczyki" },
    Name { id: "fishing rod", en: "Fishing rod", simple: "Fishing rod", ru: "Удочка", pl: "Wędka" },
    Name { id: "porridge", en: "Porridge", simple: "Millet porridge", ru: "Пшённая каша", pl: "Kasza jaglana" },
    Name { id: "stew in bowls", en: "Stew in bowls", simple: "Meat stew", ru: "Похлёбка по мискам", pl: "Gulasz do misek" },
    Name { id: "hide frame", en: "Hide frame", simple: "Skin frame", ru: "Рама для шкуры", pl: "Rama na skórę" },
    Name { id: "cane frame", en: "Cane frame", simple: "Frame from reed cane", ru: "Рама из трости", pl: "Rama z trzciny" },
    Name { id: "wax torch", en: "Wax torch", simple: "Torch with wax", ru: "Факел с воском", pl: "Pochodnia z woskiem" },
    Name { id: "wax rewad", en: "Wax rewad", simple: "New wax on a torch", ru: "Обмотка с воском", pl: "Owijka z woskiem" },
    Name { id: "cane rod", en: "Cane rod", simple: "Fishing rod from reed cane", ru: "Удочка из трости", pl: "Wędka z trzciny" },
];

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::ALL_BLOCK_IDS;

    fn assert_printable(what: &str, text: &str) {
        assert!(!text.is_empty(), "{what} is empty");
        assert!(!text.contains('_'), "{what} {text:?} still reads like an identifier");
        assert_eq!(text.trim(), text, "{what} {text:?} has spaces at an end, which `fit` measures");
        for c in text.chars() {
            assert!(
                crate::engine::texture::GLYPHS.contains(c),
                "{what} {text:?} needs a glyph for {c:?}, which the font has not got -- add the letter to `font::ORDER` or say it another way"
            );
        }
    }

    /// Walks `Species::ALL`, so a new animal is a red test the day it is
    /// added rather than an identifier on somebody's screen -- and the young,
    /// through `youth::young_name`, so a species that starts breeding needs a
    /// word for its young in all four languages too.
    #[test]
    fn every_species_has_a_name_in_every_language() {
        use primitive_shared::animals::Species;
        for &species in Species::ALL {
            let ids = std::iter::once(species.name()).chain(primitive_shared::youth::young_name(species));
            for id in ids {
                let row = ANIMALS.iter().find(|row| row.id == id).unwrap_or_else(|| panic!("no name in `ANIMALS` for {id}"));
                for language in Language::ALL {
                    assert_printable(&format!("the {id} in {language:?}"), row.in_language(*language));
                }
            }
            // ...and the young's name is not the adult's, in any language:
            // a row copied and not changed would pass the line above.
            if primitive_shared::youth::young_name(species).is_some() {
                for language in Language::ALL {
                    assert_ne!(young(species, *language), animal(species, *language), "{} in {language:?}", species.name());
                }
            }
        }
        for (i, row) in ANIMALS.iter().enumerate() {
            let known = Species::ALL
                .iter()
                .any(|&s| s.name() == row.id || primitive_shared::youth::young_name(s) == Some(row.id));
            assert!(known, "`ANIMALS` names {:?}, which is no animal", row.id);
            assert!(!ANIMALS[..i].iter().any(|earlier| earlier.id == row.id), "{:?} is in `ANIMALS` twice", row.id);
        }
    }

    /// Every animal that can kill somebody is named in their language when it
    /// does, and the English is exactly what the server sent.
    #[test]
    fn an_animal_death_is_told_in_the_players_language() {
        use primitive_shared::animals::Species;
        for &species in Species::ALL.iter().filter(|s| s.damage() > 0.0) {
            let cause = species.death_cause();
            assert_eq!(Species::of_death_cause(cause), Some(species), "{cause:?} named nobody");
            assert_eq!(death_cause(cause, Language::English), cause, "the English changed");
            for language in Language::ALL {
                let told = death_cause(cause, *language);
                assert_printable(&format!("the {} death in {language:?}", species.name()), &told);
                if *language != Language::English {
                    assert_ne!(told, cause, "the {} death was left in English for {language:?}", species.name());
                }
            }
        }
        // ...and anything else goes through untouched.
        assert_eq!(death_cause("fell from a great height", Language::Russian), "fell from a great height");
    }

    /// Walks the shared table rather than this one, so a block added in
    /// `primitive_shared` without a name here is caught the day it lands.
    #[test]
    fn every_block_has_a_name_in_every_language() {
        let mut missing = Vec::new();
        for &(_, id) in ALL_BLOCK_IDS {
            match block_row(id) {
                Some(row) => {
                    for language in Language::ALL {
                        assert_printable(&format!("{id} in {language:?}"), row.in_language(*language));
                    }
                }
                None => missing.push(id),
            }
        }
        assert!(missing.is_empty(), "no name in `ui::names::BLOCKS` for {missing:?}");
    }

    #[test]
    fn every_recipe_has_a_name_in_every_language() {
        let mut missing = Vec::new();
        for recipe in primitive_shared::crafting::RECIPES {
            match recipe_row(recipe.name) {
                Some(row) => {
                    for language in Language::ALL {
                        assert_printable(&format!("recipe {:?} in {language:?}", recipe.name), row.in_language(*language));
                    }
                }
                None => missing.push(recipe.name),
            }
        }
        missing.dedup();
        assert!(missing.is_empty(), "no name in `ui::names::RECIPES` for {missing:?}");
    }

    /// A row for a block that no longer exists is a translation somebody
    /// will keep fixing for nothing, and a row listed twice is dead after
    /// the first -- the two say different things by the time anybody
    /// notices, as `lang` has it.
    #[test]
    fn no_name_is_for_a_thing_that_does_not_exist_or_is_listed_twice() {
        for (i, row) in BLOCKS.iter().enumerate() {
            // "set_down" is a real block with no entry in `ALL_BLOCK_IDS`
            // on purpose: that table is also the anti-cheat's list of what a
            // `SetBlock` may carry, and a thing set down by hand is only ever
            // made by the server from the item in the hand. Its name is still
            // what the player reads when looking at one.
            let known = row.id == "set_down" || ALL_BLOCK_IDS.iter().any(|&(_, id)| id == row.id);
            assert!(known, "`BLOCKS` names {:?}, which is not a block", row.id);
            assert!(!BLOCKS[..i].iter().any(|earlier| earlier.id == row.id), "{:?} is in `BLOCKS` twice", row.id);
        }
        for (i, row) in RECIPES.iter().enumerate() {
            assert!(
                primitive_shared::crafting::RECIPES.iter().any(|recipe| recipe.name == row.id),
                "`RECIPES` names {:?}, which is not a recipe",
                row.id
            );
            assert!(!RECIPES[..i].iter().any(|earlier| earlier.id == row.id), "{:?} is in `RECIPES` twice", row.id);
        }
    }

    #[test]
    fn every_condition_the_rules_can_print_is_translated() {
        use primitive_shared::body::Water;
        use primitive_shared::tools;
        use primitive_shared::types::{barrel_of, jug_of};
        let mut stacks = Vec::new();
        for water in [Water::Fresh, Water::Standing, Water::Salt] {
            stacks.push(jug_of(water));
            stacks.push(barrel_of(water, 1));
        }
        for &(block, _) in ALL_BLOCK_IDS {
            // Clay at every stage of drying, and wood at every stage of
            // seasoning.
            if primitive_shared::clay::is_raw_pottery(block) {
                use primitive_shared::clay::{with_dryness, Dryness};
                stacks.extend([Dryness::Wet, Dryness::LeatherHard, Dryness::BoneDry].map(|d| with_dryness(block, d)));
            }
            if primitive_shared::wood::is_log(block) {
                let mut log = primitive_shared::wood::green(block);
                for _ in 0..primitive_shared::wood::GREENEST {
                    stacks.push(log);
                    log = primitive_shared::wood::season_a_stage(log);
                }
            }
            if !tools::takes_an_edge(block) {
                continue;
            }
            for step in 0..=tools::BLUNTEST {
                stacks.push(tools::with_edge(block, step));
                if tools::can_be_steeled(block) {
                    stacks.push(tools::with_edge(tools::steeled(block), step));
                }
            }
        }
        let mut seen = 0;
        for block in stacks {
            let Some(label) = rules_label(block) else {
                continue;
            };
            seen += 1;
            let row = CONDITIONS.iter().find(|row| row.id == label);
            assert!(row.is_some(), "the rules print {label:?}, and `CONDITIONS` has no row for it");
            for language in Language::ALL {
                assert_printable(&format!("condition {label:?} in {language:?}"), row.unwrap().in_language(*language));
            }
        }
        assert!(seen >= CONDITIONS.len(), "the walk found {seen} labels -- it is no longer reaching the rules");
    }

    #[test]
    fn a_russian_player_is_told_the_russian_name_and_the_save_still_says_the_identifier() {
        use primitive_shared::types::BLOCK_FLINT;
        assert_eq!(block(BLOCK_FLINT, Language::Russian), "Кремень");
        assert_eq!(block(BLOCK_FLINT, Language::English), "Flint");
        assert_eq!(block_name(BLOCK_FLINT), "flint", "the identifier is the save format and must not move");
    }

    #[test]
    fn a_search_finds_a_block_by_any_language_and_by_its_identifier() {
        for query in ["flint_knife", "flint knife", "кремнёвый нож", "krzemienny", "нож"] {
            assert!(block_found("flint_knife", query), "{query:?} does not find the flint knife");
        }
        assert!(!block_found("flint_knife", "топор"));
        assert!(!block_found("vessel", "tin"), "a word's middle is not a match: melTINg pot");
        assert!(recipe_found("hone", "заточка"));
    }

    #[test]
    fn a_block_with_no_row_is_drawn_readable_rather_than_as_nothing() {
        assert_eq!(identified("not_a_block_yet", Language::Polish), "not a block yet");
    }
}
