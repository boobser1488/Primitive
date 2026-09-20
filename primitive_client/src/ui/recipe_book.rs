//! The recipe book: what this player knows how to make, and what they are
//! one find away from.
//!
//! ## Its own screen, not a tab of the pack
//!
//! The pack's crafting grid answers "what can I make *now*" -- it lists
//! only what the player has the ingredients for (see
//! `inventory_screen::offered`), and that is right for a screen a player
//! opens with their hands full. The book answers the other question,
//! "what is this for, and what do I need", which is asked with empty
//! hands, by somebody deciding where to walk tomorrow. Two questions, two
//! screens: a tab inside the pack would have had to be both at once, in a
//! panel already three columns wide. It lives beside the map instead,
//! because the two are read together -- the book says there is a second
//! metal, the map says where the hills are.
//!
//! ## What a row shows, and what it does not
//!
//! A known recipe shows everything: what it makes, what it takes and how
//! much of each the pack holds, where it has to be done, **how hot the
//! fire has to be** (in the colour a smith reads -- see `hearth::Glow`),
//! what is still in the way that a list of ingredients cannot say (a tool,
//! a hone with nothing blunt left, a full pack), what comes back, and
//! **what the thing made is itself good for** -- the book read backwards
//! (`crafting::uses`), which is the question a player holding their first
//! flint actually has.
//! A lead (see `primitive_shared::discovery`) shows the thing it makes and
//! the ingredients that have been held, and draws the one that has not as
//! a question mark. **The search never matches that ingredient**, or a
//! player could type every block name in turn and read the answer off
//! which rows appear.
//!
//! ## Search on a keyboard, filters for a finger
//!
//! Typing is the fast way through a long list, and a desktop has a
//! keyboard. A phone's keyboard covers half the glass and this screen is
//! the other half; so the stations are a row of buttons, which work on
//! both, and the search field is drawn only where there are keys to type
//! into it with.

use primitive_shared::crafting::{has_ingredients, Heat, Recipe, Shortfall, Station, RECIPES};
use primitive_shared::discovery::{Discovered, Knowledge};
use primitive_shared::inventory::Inventory;
use primitive_shared::types::{block_kind, block_name, BlockId, BLOCK_BLOOMERY, BLOCK_CAMPFIRE, BLOCK_KILN};

use crate::engine::texture::FaceLayers;
use crate::ui::inventory_screen::{icon_layer, textured};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// Which stations the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Filter {
    #[default]
    All,
    Hands,
    Fire,
    Kiln,
    Bloomery,
    /// All four workshops under one chip. See `Msg::RecipesWorkshops`.
    Workshops,
}

pub const FILTERS: [Filter; 6] =
    [Filter::All, Filter::Hands, Filter::Workshops, Filter::Fire, Filter::Kiln, Filter::Bloomery];

impl Filter {
    fn label(self) -> Msg {
        match self {
            Filter::All => Msg::RecipesAll,
            Filter::Hands => Msg::RecipesHands,
            Filter::Fire => Msg::Campfire,
            Filter::Kiln => Msg::Kiln,
            Filter::Bloomery => Msg::Bloomery,
            Filter::Workshops => Msg::RecipesWorkshops,
        }
    }

    fn admits(self, station: Station) -> bool {
        match self {
            Filter::All => true,
            Filter::Hands => station == Station::Hands,
            Filter::Fire => station == Station::Heat,
            Filter::Kiln => station == Station::Forge,
            Filter::Bloomery => station == Station::Bloomery,
            Filter::Workshops => Station::WORKSHOPS.contains(&station),
        }
    }
}

/// One row of the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Into `RECIPES`.
    pub index: usize,
    pub knowledge: Knowledge,
}

/// Whether `query` finds `recipe`, without looking at `hidden`.
fn matches(recipe: &Recipe, query: &str, hidden: Option<BlockId>) -> bool {
    if query.is_empty() {
        return true;
    }
    // Against the name as it is printed, in any of the languages, as well
    // as the identifier: "copper ingot" and "медный слиток" both find
    // what the screen calls one. See `names::block_found`.
    crate::ui::names::recipe_found(recipe.name, query)
        || crate::ui::names::block_found(block_name(recipe.output.0), query)
        || recipe
            .inputs
            .iter()
            .filter(|&&(block, _)| Some(block_kind(block)) != hidden)
            .any(|&(block, _)| crate::ui::names::block_found(block_name(block), query))
}

/// The rows a player sees, in the order they see them.
///
/// **Known first, then leads**, each in table order. Table order is kept
/// within each, for the reason the pack's grid keeps it: two rows never
/// swap places, so a recipe that was above another is still above it the
/// next time both are listed.
pub fn entries(discovered: &Discovered, filter: Filter, query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    let mut known = Vec::new();
    let mut leads = Vec::new();
    for (index, recipe) in RECIPES.iter().enumerate() {
        if !filter.admits(recipe.station) {
            continue;
        }
        let knowledge = discovered.of(recipe);
        let hidden = match knowledge {
            Knowledge::Hidden => continue,
            Knowledge::Lead { missing } => Some(missing),
            Knowledge::Known => None,
        };
        if !matches(recipe, &query, hidden) {
            continue;
        }
        let entry = Entry { index, knowledge };
        if hidden.is_some() {
            leads.push(entry);
        } else {
            known.push(entry);
        }
    }
    known.extend(leads);
    known
}

// ---- layout ----
//
// Everything is placed inside the `body` the journal hands over, and
// every rectangle comes from one function used by both the drawing and the
// hit-testing.

const GAP: f32 = 0.014;
const FILTER_WIDTH: f32 = 0.30;

fn chip_height() -> f32 {
    widgets::tappable(0.075)
}

/// How tall one row of the list is.
pub fn row_height() -> f32 {
    widgets::tappable(0.08)
}

/// Where a filter button is.
pub fn filter_rect(filter: Filter, body: Rect) -> Rect {
    let index = FILTERS.iter().position(|f| *f == filter).unwrap_or(0) as f32;
    let x0 = body.x0 + index * (FILTER_WIDTH + GAP);
    Rect::new(x0, body.y1 - chip_height(), x0 + FILTER_WIDTH, body.y1)
}

/// Where the search field is: the rest of the filter row.
pub fn search_rect(body: Rect) -> Rect {
    let last = FILTERS[FILTERS.len() - 1];
    let x0 = filter_rect(last, body).x1 + GAP * 2.0;
    Rect::new(x0.min(body.x1), body.y1 - chip_height(), body.x1, body.y1)
}

/// The column the rows are in.
pub fn list_rect(body: Rect) -> Rect {
    let width = (body.width() * 0.46).clamp(0.8, 1.6).min(body.width());
    Rect::new(body.x0, body.y0, body.x0 + width, body.y1 - chip_height() - GAP)
}

/// The pane that reads out the chosen recipe.
pub fn detail_rect(body: Rect) -> Rect {
    let list = list_rect(body);
    Rect::new(list.x1 + GAP * 2.0, body.y0, body.x1, list.y1)
}

/// How many rows fit.
pub fn visible_rows(body: Rect) -> usize {
    ((list_rect(body).height() / row_height()).floor() as usize).max(1)
}

/// Where the row in `place` on screen is.
pub fn row_rect(place: usize, body: Rect) -> Rect {
    let list = list_rect(body);
    let y1 = list.y1 - place as f32 * row_height();
    Rect::new(list.x0, y1 - row_height() + 0.004, list.x1, y1)
}

/// Which row on screen a point is over.
pub fn row_at(at: (f32, f32), body: Rect) -> Option<usize> {
    (0..visible_rows(body)).find(|&place| row_rect(place, body).contains(at.0, at.1))
}

/// Which filter a point is over.
pub fn filter_at(at: (f32, f32), body: Rect) -> Option<Filter> {
    FILTERS.into_iter().find(|f| filter_rect(*f, body).contains(at.0, at.1))
}

/// What the book is showing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecipeBook {
    filter: Filter,
    query: String,
    scroll: usize,
    /// Into `RECIPES`. `None` reads out the first row.
    selected: Option<usize>,
}

/// The longest search worth typing. Past this the field would run under
/// the edge of the window, and no recipe name is this long.
const QUERY_LIMIT: usize = 24;

impl RecipeBook {
    #[cfg(test)]
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn type_char(&mut self, c: char) {
        if c.is_control() || self.query.chars().count() >= QUERY_LIMIT {
            return;
        }
        self.query.push(c);
        self.scroll = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.scroll = 0;
    }

    /// Scrolls by whole rows, stopping at both ends.
    pub fn scroll_by(&mut self, rows: i32, discovered: &Discovered, body: Rect) {
        let total = entries(discovered, self.filter, &self.query).len();
        let last = total.saturating_sub(visible_rows(body));
        self.scroll = (self.scroll as i64 + rows as i64).clamp(0, last as i64) as usize;
    }

    /// A press at `at`. Answers whether it changed anything.
    pub fn click(&mut self, at: (f32, f32), discovered: &Discovered, body: Rect) -> bool {
        if let Some(filter) = filter_at(at, body) {
            let changed = filter != self.filter;
            self.filter = filter;
            self.scroll = 0;
            return changed;
        }
        if let Some(place) = row_at(at, body) {
            let rows = entries(discovered, self.filter, &self.query);
            if let Some(entry) = rows.get(self.scroll + place) {
                let changed = self.selected != Some(entry.index);
                self.selected = Some(entry.index);
                return changed;
            }
        }
        false
    }

    /// Everything `paint` reads that belongs to the book.
    pub fn key(&self) -> (Filter, &str, usize, Option<usize>) {
        (self.filter, &self.query, self.scroll, self.selected)
    }

    /// Draws the book into `body`.
    #[allow(clippy::too_many_arguments)] // a book, its pictures, a pack, the knowledge, a place, a pointer, a language
    pub fn paint(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        inventory: &Inventory,
        discovered: &Discovered,
        body: Rect,
        cursor: Option<(f32, f32)>,
        language: Language,
    ) {
        let hovered = |rect: Rect| cursor.is_some_and(|(x, y)| rect.contains(x, y));

        for filter in FILTERS {
            let rect = filter_rect(filter, body);
            p.button(rect, language.text(filter.label()), hovered(rect) || self.filter == filter, true);
            if self.filter == filter {
                p.border(rect, 0.004, ACCENT);
            }
        }
        if !widgets::touch_layout() {
            let field = search_rect(body);
            if field.width() > 0.2 {
                p.well(field, WELL);
                let (text, colour) = if self.query.is_empty() {
                    (language.text(Msg::RecipesSearch).to_string(), widgets::TEXT_DIM)
                } else {
                    (format!("{}_", self.query), widgets::TEXT)
                };
                p.label_left(field, &widgets::fit(&text, 0.9, field.width() - 0.04), 0.02, 0.9, colour);
            }
        }

        let rows = entries(discovered, self.filter, &self.query);
        let list = list_rect(body);
        p.well(list, WELL);
        if rows.is_empty() {
            let message = if discovered.is_empty() { Msg::RecipesEmpty } else { Msg::RecipesNoMatch };
            let lines = widgets::wrap(language.text(message), ((list.width() - 0.08) / widgets::measure("m", 0.85)).max(8.0) as usize);
            for (n, line) in lines.iter().enumerate() {
                p.text(line, list.x0 + 0.04, list.y1 - 0.06 - n as f32 * 0.06, 0.85, widgets::TEXT_DIM);
            }
        }
        let chosen = self
            .selected
            .and_then(|index| rows.iter().find(|entry| entry.index == index))
            .or_else(|| rows.get(self.scroll))
            .copied();
        for (place, entry) in rows.iter().skip(self.scroll).take(visible_rows(body)).enumerate() {
            let rect = row_rect(place, body);
            let recipe = &RECIPES[entry.index];
            let lead = matches!(entry.knowledge, Knowledge::Lead { .. });
            if Some(entry.index) == chosen.map(|c| c.index) {
                p.quad(rect, CHOSEN);
                p.border(rect, 0.003, ACCENT);
            } else if hovered(rect) {
                p.quad(rect, HOVER);
            }
            let side = rect.height() - 0.02;
            let icon = Rect::new(rect.x0 + 0.012, rect.y0 + 0.01, rect.x0 + 0.012 + side, rect.y1 - 0.01);
            textured(p, icon, icon_layer(layers, recipe.output.0), tint(recipe.output.0, lead));
            let makeable = !lead && has_ingredients(inventory, recipe);
            let count = if lead { "?".to_string() } else { format!("x{}", recipe.output.1) };
            let count_width = widgets::measure(&count, 0.85) + 0.02;
            let name_room = rect.width() - side - 0.05 - count_width;
            let name_colour = if lead { widgets::TEXT_DIM } else { widgets::TEXT };
            let name_rect = Rect::new(icon.x1 + 0.02, rect.y0, rect.x1, rect.y1);
            p.label_left(name_rect, &widgets::fit(&crate::ui::names::recipe(recipe.name, language), 0.9, name_room), 0.0, 0.9, name_colour);
            let count_rect = Rect::new(rect.x1 - count_width, rect.y0, rect.x1 - 0.01, rect.y1);
            p.label_in(count_rect, &count, 0.85, if makeable { GOOD } else { widgets::TEXT_DIM });
        }

        let pane = detail_rect(body);
        if pane.width() < 0.4 {
            return;
        }
        p.well(pane, WELL);
        if let Some(entry) = chosen {
            read_out(p, layers, inventory, discovered, &RECIPES[entry.index], entry.knowledge, pane, language);
        }
    }
}

/// One line of "what is missing", in this player's language.
///
/// **Here rather than in either screen**, because both the book and the
/// pack's crafting list say it and they must say it the same way: two
/// wordings of the same refusal is two different games to learn, and the
/// one in the pack is read in a hurry against the one in the book that is
/// read carefully. See `crafting::Shortfall`.
pub fn shortfall_text(short: Shortfall, language: Language) -> String {
    use crate::ui::names;
    match short {
        Shortfall::Ingredient { block, have, need } => {
            format!("{} {}/{}", names::block(block, language), have, need)
        }
        // The tool is named and the count is not, because any rung of its
        // ladder will do: "1/1 bronze chisel" to a player who could knap a
        // flint one would send them to the wrong age entirely.
        Shortfall::Tool(block) => {
            format!("{} {}", language.text(Msg::RecipeNeedTool), names::block(block, language))
        }
        Shortfall::NothingToWork(_) => language.text(Msg::RecipeNothingToWork).to_string(),
        Shortfall::Station(station) => language.text(station_msg(station)).to_string(),
        Shortfall::NoRoom => language.text(Msg::NoRoom).to_string(),
    }
}

/// Where a row is done, as a sentence.
fn station_msg(station: Station) -> Msg {
    match station {
        Station::Hands => Msg::RecipeByHand,
        Station::Heat => Msg::RecipeAtFire,
        Station::Forge => Msg::RecipeAtKiln,
        Station::Bloomery => Msg::RecipeAtBloomery,
        Station::Bench => Msg::RecipeAtBench,
        Station::Mason => Msg::RecipeAtMason,
        Station::Wheel => Msg::RecipeAtWheel,
        Station::Leather => Msg::RecipeAtLeatherBench,
    }
}

const ACCENT: [f32; 4] = [0.95, 0.72, 0.30, 1.0];
const WELL: [f32; 4] = [0.08, 0.075, 0.07, 0.96];
const CHOSEN: [f32; 4] = [0.22, 0.18, 0.11, 1.0];
const HOVER: [f32; 4] = [0.16, 0.15, 0.14, 1.0];
const GOOD: [f32; 4] = [0.55, 0.85, 0.45, 1.0];
const BAD: [f32; 4] = [0.95, 0.45, 0.38, 1.0];
const HEADING: [f32; 4] = [0.95, 0.72, 0.30, 1.0];

/// The tint an icon is drawn in: the garment's material where it has one
/// (see `hotbar::icon_tint`), and greyed for a lead, whose picture is a
/// promise rather than a thing in hand.
fn tint(block: BlockId, lead: bool) -> [f32; 4] {
    let colour = crate::ui::hotbar::icon_tint(block, [1.0, 1.0, 1.0, 1.0]);
    if lead {
        [colour[0] * 0.55, colour[1] * 0.55, colour[2] * 0.55, colour[3]]
    } else {
        colour
    }
}

/// The right-hand pane: one recipe, read out in full.
#[allow(clippy::too_many_arguments)] // a page, its pictures, a pack, the knowledge, a row, a place, a language
fn read_out(
    p: &mut Painter,
    layers: &FaceLayers,
    inventory: &Inventory,
    discovered: &Discovered,
    recipe: &Recipe,
    knowledge: Knowledge,
    pane: Rect,
    language: Language,
) {
    let missing = match knowledge {
        Knowledge::Lead { missing } => Some(missing),
        _ => None,
    };
    let left = pane.x0 + 0.04;
    let room = pane.width() - 0.08;
    let mut top = pane.y1 - 0.03;

    // What it makes.
    let big = 0.12;
    textured(p, Rect::new(left, top - big, left + big, top), icon_layer(layers, recipe.output.0), tint(recipe.output.0, false));
    let title = format!("{} x{}", crate::ui::names::recipe(recipe.name, language), recipe.output.1);
    p.text(&widgets::fit(&title, 1.2, room - big - 0.03), left + big + 0.03, top - 0.03, 1.2, widgets::TEXT);
    if missing.is_some() {
        p.text(
            &widgets::fit(language.text(Msg::RecipeLead), 0.8, room - big - 0.03),
            left + big + 0.03,
            top - 0.085,
            0.8,
            widgets::TEXT_DIM,
        );
    }
    top -= big + 0.04;

    let line = 0.072;
    let heading = |p: &mut Painter, text: Msg, top: &mut f32| {
        p.text(language.text(text), left, *top, 0.85, HEADING);
        *top -= 0.055;
    };

    // Everything standing between this pack and this row, worked out once:
    // the list below is coloured by it and the section further down names
    // what a list of ingredients cannot say.
    //
    // Asked of a player standing at every fire at once, because the book is
    // read with empty hands in a field: a book that opened with "you are
    // not at a kiln" for every metal row would be saying what the WHERE
    // line two inches below already says.
    let short: Vec<Shortfall> = if missing.is_some() {
        Vec::new()
    } else {
        primitive_shared::crafting::shortfall(inventory, recipe, EVERYWHERE)
            .into_iter()
            .filter(|s| !matches!(s, Shortfall::Station(_)))
            .collect()
    };

    heading(p, Msg::RecipeMadeFrom, &mut top);
    for &(block, need) in recipe.inputs {
        let icon = Rect::new(left, top - line + 0.012, left + line - 0.012, top);
        if Some(block_kind(block)) == missing {
            p.well(icon, [0.03, 0.03, 0.03, 1.0]);
            p.label_in(icon, "?", 1.0, BAD);
            p.text(language.text(Msg::RecipeNotFound), icon.x1 + 0.025, top - 0.018, 0.9, widgets::TEXT_DIM);
        } else {
            textured(p, icon, icon_layer(layers, block), tint(block, false));
            // **A tool counts at any rung of its ladder.** Counted by the
            // id in the row, a player who had replaced their flint chisel
            // with a bronze one read "flint chisel 0/1" in red and was
            // being told to go back to the stone age. `shortfall` asks the
            // question the craft path asks (`tool_slot`), so the two agree.
            let is_the_tool = primitive_shared::crafting::used_tool(recipe) == Some(block);
            let have = if is_the_tool {
                if short.contains(&Shortfall::Tool(block)) { 0 } else { need }
            } else {
                inventory.count(block)
            };
            let tally = format!("{have}/{need}");
            let tally_width = widgets::measure(&tally, 0.9);
            p.text(
                &widgets::fit(&crate::ui::names::block(block, language), 0.9, room - line - tally_width - 0.06),
                icon.x1 + 0.025,
                top - 0.018,
                0.9,
                widgets::TEXT,
            );
            p.text(&tally, pane.x1 - 0.04 - tally_width, top - 0.018, 0.9, if have >= need { GOOD } else { BAD });
        }
        top -= line;
    }
    top -= 0.02;

    heading(p, Msg::RecipeWhere, &mut top);
    let station_block = match recipe.station {
        Station::Hands => None,
        Station::Heat => Some(BLOCK_CAMPFIRE),
        Station::Forge => Some(BLOCK_KILN),
        Station::Bloomery => Some(BLOCK_BLOOMERY),
        Station::Bench | Station::Mason | Station::Wheel | Station::Leather => recipe.station.workshop_block(),
    };
    let mut text_left = left;
    if let Some(block) = station_block {
        let icon = Rect::new(left, top - line + 0.012, left + line - 0.012, top);
        textured(p, icon, icon_layer(layers, block), tint(block, false));
        text_left = icon.x1 + 0.025;
    }
    p.text(&widgets::fit(language.text(station_msg(recipe.station)), 0.9, pane.x1 - 0.04 - text_left), text_left, top - 0.018, 0.9, widgets::TEXT);
    top -= line + 0.02;

    // **How hot, said in the colour a smith reads.** "In a lit kiln" is
    // where to stand and not how far to push the fire, and a player whose
    // kiln is at a dark red and whose ore wants a white heat was being told
    // nothing at all -- they could see the row, see the kiln, load it and
    // watch nothing happen. The word is `hearth::Glow`'s, for the reason
    // the note above `Msg::GlowCold` gives: the font has no degree sign,
    // and a colour is what a fire actually shows you.
    if let Some(degrees) = primitive_shared::hearth::needs_degrees(recipe) {
        if recipe.station.is_hearth() && top - 0.05 > pane.y0 {
            let glow = primitive_shared::hearth::Glow::of(degrees);
            let text = format!(
                "{} {}",
                language.text(Msg::RecipeHeat),
                language.text(crate::ui::chest_screen::glow_msg(glow)),
            );
            p.text(&widgets::fit(&text, 0.9, room), left, top, 0.9, widgets::TEXT_DIM);
            top -= 0.06;
        }
    }

    // **What a list of ingredients cannot say.** The counts are already
    // above, in red, where they belong; what has nowhere else to go is the
    // tool (which any rung of its ladder satisfies, so it has no count),
    // the hone with nothing blunt left to hone, and the pack with no room.
    // A row refused for want of a tool has no missing *ingredient* at all,
    // and used to reach the player as the single word "no".
    let rest: Vec<&Shortfall> = short.iter().filter(|s| !matches!(s, Shortfall::Ingredient { .. })).collect();
    if !rest.is_empty() && top - line > pane.y0 {
        heading(p, Msg::RecipeMissing, &mut top);
        for item in rest {
            if top - 0.05 < pane.y0 {
                break;
            }
            p.text(&widgets::fit(&shortfall_text(*item, language), 0.9, room), left, top, 0.9, BAD);
            top -= 0.05;
        }
        top -= 0.02;
    }

    if !recipe.returns.is_empty() && top - line > pane.y0 {
        heading(p, Msg::RecipeKept, &mut top);
        let mut x = left;
        for &(block, _) in recipe.returns {
            let icon = Rect::new(x, top - line + 0.012, x + line - 0.012, top);
            textured(p, icon, icon_layer(layers, block), tint(block, false));
            let name = crate::ui::names::block(block, language);
            p.text(&name, icon.x1 + 0.02, top - 0.018, 0.9, widgets::TEXT);
            x = icon.x1 + 0.06 + widgets::measure(&name, 0.9);
        }
        top -= line + 0.02;
    }

    if recipe.failure > 0.0 && top - 0.05 > pane.y0 {
        let text = format!("{} ({}%)", language.text(Msg::RecipeMayFail), (recipe.failure * 100.0).round() as u32);
        p.text(&widgets::fit(&text, 0.85, room), left, top, 0.85, BAD);
        top -= 0.06;
    }

    // **The book read backwards.** A player holding their first flint is
    // not looking up a recipe called flint -- there is none -- and until
    // this line the only way to learn that flint is a knife, a spear and a
    // striker was to read four hundred rows looking for one. See
    // `crafting::uses`.
    //
    // Only rows this player would be shown anyway (`Knowledge::Hidden` is
    // skipped), for the reason the whole book exists: knowing what a thing
    // is for is knowledge, and printing the iron age on the first evening
    // turns the ladder into a shopping list.
    if top - 0.05 <= pane.y0 {
        return;
    }
    heading(p, Msg::RecipeUsedFor, &mut top);
    let mut named = 0;
    for (_, into) in primitive_shared::crafting::uses(recipe.output.0) {
        if top - 0.05 < pane.y0 {
            break;
        }
        if matches!(discovered.of(into), Knowledge::Hidden) {
            continue;
        }
        let icon = Rect::new(left, top - 0.05, left + 0.044, top - 0.006);
        textured(p, icon, icon_layer(layers, into.output.0), tint(into.output.0, false));
        p.text(
            &widgets::fit(&crate::ui::names::recipe(into.name, language), 0.85, room - 0.07),
            icon.x1 + 0.02,
            top,
            0.85,
            widgets::TEXT,
        );
        top -= 0.055;
        named += 1;
    }
    if named == 0 {
        p.text(&widgets::fit(language.text(Msg::RecipeUsedForNothing), 0.85, room), left, top, 0.85, widgets::TEXT_DIM);
    }
}

/// A player standing at every hearth and every workshop at once.
///
/// What the book's "still missing" list is read against: the page is read
/// in a field, and telling somebody planning tomorrow that they are not
/// standing at a kiln is telling them what the WHERE line above already
/// says. See `read_out`.
const EVERYWHERE: Heat = Heat { fire: true, kiln: true, bloomery: true, workshops: 0b1111 };

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_BRONZE_INGOT, BLOCK_COPPER_INGOT, BLOCK_LOG, BLOCK_TIN_INGOT};

    fn body(aspect: f32) -> Rect {
        crate::ui::journal::body_rect(aspect)
    }

    fn index_making(output: BlockId) -> usize {
        RECIPES.iter().position(|r| r.output.0 == output).expect("a recipe")
    }

    fn holding(kinds: &[BlockId]) -> Discovered {
        Discovered::from_kinds(kinds.iter().copied())
    }

    /// Everything bronze asks for except the tin.
    fn one_find_from_bronze() -> Discovered {
        let bronze = &RECIPES[index_making(BLOCK_BRONZE_INGOT)];
        holding(
            &bronze
                .inputs
                .iter()
                .map(|&(block, _)| block_kind(block))
                .filter(|&kind| kind != BLOCK_TIN_INGOT)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_player_who_has_held_nothing_has_an_empty_book() {
        assert!(entries(&Discovered::new(), Filter::All, "").is_empty());
    }

    #[test]
    fn a_log_in_the_pack_puts_planks_in_the_book() {
        let rows = entries(&holding(&[BLOCK_LOG]), Filter::All, "");
        let planks = RECIPES.iter().position(|r| r.name == "planks").expect("planks");
        assert!(rows.contains(&Entry { index: planks, knowledge: Knowledge::Known }), "{rows:?}");
    }

    #[test]
    fn what_is_known_is_listed_before_what_is_only_a_lead() {
        let rows = entries(&one_find_from_bronze(), Filter::All, "");
        let first_lead = rows.iter().position(|e| matches!(e.knowledge, Knowledge::Lead { .. }));
        if let Some(first_lead) = first_lead {
            assert!(
                rows[first_lead..].iter().all(|e| matches!(e.knowledge, Knowledge::Lead { .. })),
                "a known recipe was listed among the leads"
            );
        }
    }

    #[test]
    fn the_search_finds_a_recipe_by_what_it_makes_and_never_by_what_is_missing() {
        let hands = one_find_from_bronze();
        assert!(hands.has_held(BLOCK_COPPER_INGOT));
        let bronze = index_making(BLOCK_BRONZE_INGOT);
        let found = |query: &str| entries(&hands, Filter::All, query).iter().any(|e| e.index == bronze);
        assert!(found("bronze"), "bronze could not be found by name");
        assert!(!found("tin"), "the search gave away what bronze is missing");
    }

    #[test]
    fn a_filter_lists_only_its_own_station() {
        let everything = Discovered::from_kinds(RECIPES.iter().flat_map(|r| r.inputs.iter().map(|&(b, _)| b)));
        for (filter, station) in [
            (Filter::Hands, Station::Hands),
            (Filter::Fire, Station::Heat),
            (Filter::Kiln, Station::Forge),
            (Filter::Bloomery, Station::Bloomery),
        ] {
            let rows = entries(&everything, filter, "");
            assert!(!rows.is_empty(), "{filter:?} lists nothing");
            assert!(rows.iter().all(|e| RECIPES[e.index].station == station), "{filter:?} let another station in");
        }
        let workshops = entries(&everything, Filter::Workshops, "");
        for station in Station::WORKSHOPS {
            assert!(workshops.iter().any(|e| RECIPES[e.index].station == station), "the workshop chip hides {station:?}");
        }
        assert!(
            workshops.iter().all(|e| Station::WORKSHOPS.contains(&RECIPES[e.index].station)),
            "the workshop chip let a hand or a fire row in"
        );
        assert_eq!(entries(&everything, Filter::All, "").len(), RECIPES.len());
    }

    #[test]
    fn every_row_and_every_filter_is_pressed_where_it_is_drawn() {
        for aspect in [1.0, 4.0 / 3.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            let body = body(aspect);
            for filter in FILTERS {
                let rect = filter_rect(filter, body);
                assert_eq!(filter_at((rect.centre_x(), rect.centre_y()), body), Some(filter), "aspect {aspect}");
                assert!(rect.x1 <= body.x1 + 1e-4, "{filter:?} ran off a window of aspect {aspect}");
            }
            for place in 0..visible_rows(body) {
                let rect = row_rect(place, body);
                assert_eq!(row_at((rect.centre_x(), rect.centre_y()), body), Some(place), "aspect {aspect}");
                assert!(rect.y0 >= body.y0 - 1e-4, "row {place} fell out of the book at aspect {aspect}");
                assert!(rect.y1 <= filter_rect(Filter::All, body).y0, "row {place} is under the filters");
            }
        }
    }

    #[test]
    fn a_phone_gets_rows_and_filters_a_finger_tall() {
        widgets::as_a_phone(|| {
            assert!(row_height() >= widgets::FINGER_SIDE - 1e-5);
            assert!(filter_rect(Filter::All, body(2712.0 / 1220.0)).height() >= widgets::FINGER_SIDE - 1e-5);
        });
    }

    #[test]
    fn clicking_a_row_reads_that_recipe_out() {
        let hands = holding(&[BLOCK_LOG]);
        let mut book = RecipeBook::default();
        let body = body(16.0 / 9.0);
        let rect = row_rect(0, body);
        book.click((rect.centre_x(), rect.centre_y()), &hands, body);
        assert_eq!(book.selected, entries(&hands, Filter::All, "").first().map(|e| e.index));
    }

    /// **A refusal is never a blank line**, in any language, for any row.
    /// The point of naming what is missing is lost the moment one of the
    /// answers comes out empty, and the one that used to was the worst of
    /// them: a row short of a *tool* has no missing ingredient at all and
    /// the pack's list answered it with the word "no".
    #[test]
    fn every_way_a_recipe_can_be_refused_says_something_in_every_language() {
        let empty = Inventory::new();
        for &language in Language::ALL {
            for recipe in RECIPES {
                let short = primitive_shared::crafting::shortfall(&empty, recipe, primitive_shared::crafting::Heat::NONE);
                assert!(!short.is_empty(), "{} refuses silently", recipe.name);
                for item in short {
                    let said = shortfall_text(item, language);
                    assert!(!said.trim().is_empty(), "{} said nothing about {item:?} in {language:?}", recipe.name);
                }
            }
        }
    }

    /// The detail pane, drawn for the one player it was written for: hands
    /// that have held everything and a pack that holds nothing, so every
    /// section is on the page at once -- what it takes, where, how hot,
    /// what is missing, and what the thing is then good for.
    #[test]
    fn a_recipe_a_player_cannot_make_is_still_read_out_in_full() {
        let everything = Discovered::from_kinds(RECIPES.iter().flat_map(|r| r.inputs.iter().map(|&(b, _)| b)));
        let smelt = index_making(BLOCK_COPPER_INGOT);
        let book = RecipeBook { selected: Some(smelt), ..Default::default() };
        for aspect in [1.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            let mut p = Painter::onto_themed(
                crate::engine::texture::FontAtlas::for_test(),
                Vec::new(),
                widgets::Theme::DARK,
            );
            book.paint(
                &mut p,
                &FaceLayers::empty_for_test(),
                &Inventory::new(),
                &everything,
                body(aspect),
                None,
                Language::Russian,
            );
            assert!(!p.into_vertices().is_empty(), "the book drew nothing at aspect {aspect}");
        }
    }

    #[test]
    fn scrolling_stops_at_both_ends_of_the_list() {
        let everything = Discovered::from_kinds(RECIPES.iter().flat_map(|r| r.inputs.iter().map(|&(b, _)| b)));
        let body = body(16.0 / 9.0);
        let mut book = RecipeBook::default();
        book.scroll_by(-5, &everything, body);
        assert_eq!(book.scroll, 0);
        book.scroll_by(10_000, &everything, body);
        assert_eq!(book.scroll, RECIPES.len() - visible_rows(body));
    }
}
