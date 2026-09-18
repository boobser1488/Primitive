//! The give menu: every block and item in the game, and a tap that asks
//! the server for one.
//!
//! ## Why a menu over `/give` and not a creative mode
//!
//! "добавь креатив или хотя бы меню". A creative mode is a second set of
//! rules: a player who does not fall, does not starve and puts blocks
//! down out of nothing is a player the server has to stop validating,
//! and every rule in `primitive_shared` would grow a branch asking which
//! kind of player this is. The server already has the one command that
//! makes something out of nothing -- `/give`, operator-only, written
//! exactly so that testing a chest full of stone does not mean mining a
//! chest full of stone. A menu over it adds no rule at all: what leaves
//! this screen is the same chat line an operator could type, and the
//! server decides, exactly as it did before this file existed.
//!
//! So the client stays what it is (`CLAUDE.md`: the client is never the
//! authority). Nothing here puts a block in a pack. Every row is a
//! sentence the server may refuse.
//!
//! ## Why the list is `ALL_BLOCK_IDS` and not a curated one
//!
//! Because that table *is* what `/give` accepts -- the server looks a
//! name up in it (`Response::Give`) -- so a hand-picked list would be a
//! second table that drifts from the first, and the drift would show up
//! as a row that looks like a thing you can have and answers "no block
//! called ...". A few rows in it are states rather than things a player
//! would ask for (a lit campfire, ripe cotton); they are given anyway,
//! because the alternative is a list of exceptions nobody maintains.
//!
//! Rows are drawn with `ui::names`, the same names the pack's tooltip and
//! the recipe book print, in the player's language. **What is sent is
//! still the identifier** from `types::block_name`: it is the name in
//! `blocks.toml`, in save files and in the very command this screen
//! sends, and a translated `/give` would be a command the server has
//! never heard of. Everything that is this screen's own words -- its tab,
//! its sections, its refusal -- is in `lang`, in all four languages.
//!
//! ## How a refusal gets back here
//!
//! The server answers a chat command with a chat line from its own
//! name, and there is no protocol message for "your give worked". So
//! the screen remembers that it asked, and the frame loop offers it the
//! next server line before the chat log gets it (`take_reply`): a line
//! that answers a give is read into the status bar at the bottom of the
//! page and **swallowed**, so a player who is not an operator sees why
//! the menu does nothing instead of watching refusals pile into the chat
//! log -- which is the thing the "Отказы больше не пишутся в чат" change
//! deliberately took out.
//!
//! Two alternatives were rejected. A new `ServerMessage` saying what the
//! give did would be the clean version and is the one to build if this
//! screen ever needs more than a sentence -- it was not built because it
//! is a protocol change for a debug tool, and because the client would
//! still have to work against servers that do not send it. Watching the
//! pack for a block that appeared was the other, and it cannot tell a
//! refusal from a full pack from a server that is simply slow.
//!
//! Reading English out of the server's reply is the price, and it is
//! paid with a test rather than with hope:
//! `what_the_real_server_answers_a_give_is_what_this_screen_reads` runs
//! the server's own parser and authorizer and asserts that this file
//! still understands both answers. The day someone rewords that line,
//! the client's test goes red in the same commit.

use primitive_shared::types::{self, BlockId, ALL_BLOCK_IDS};

use crate::engine::texture::FaceLayers;
use crate::ui::inventory_screen::{icon_layer, textured};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// Which part of the game's contents the list is showing.
///
/// **A partition, not a set of tags.** Every block is in exactly one
/// section (`section_of`), which is what makes the chips a way of
/// *dividing* two hundred and fifty rows rather than five overlapping
/// searches over them -- and what lets
/// `every_block_in_the_game_is_in_exactly_one_section` be a test at all.
/// The cost is that a wool coat is under CLOTHES and not also under
/// STUFF; the search is what finds a thing whose section you guessed
/// wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Section {
    #[default]
    All,
    /// Anything that can be put down in the world.
    Blocks,
    /// Anything with a tier, a spear, or a thing used on the ground.
    Tools,
    /// Anything that can be worn.
    Clothes,
    Food,
    /// Everything else: the carried things that are none of the above --
    /// ingots, fibre, bones, a jug.
    Stuff,
}

pub const SECTIONS: [Section; 6] = [
    Section::All,
    Section::Blocks,
    Section::Tools,
    Section::Clothes,
    Section::Food,
    Section::Stuff,
];

impl Section {
    fn label(self) -> Msg {
        match self {
            // The same word the recipe book's own ALL chip uses. One row
            // in the table rather than two that have to be kept saying
            // the same thing in four languages.
            Section::All => Msg::RecipesAll,
            Section::Blocks => Msg::GiveBlocks,
            Section::Tools => Msg::GiveTools,
            Section::Clothes => Msg::GiveClothes,
            Section::Food => Msg::GiveFood,
            Section::Stuff => Msg::GiveStuff,
        }
    }

    fn admits(self, block: BlockId) -> bool {
        self == Section::All || self == section_of(block)
    }
}

/// The one section a block belongs to.
///
/// **Asked in this order on purpose.** A hoe is an item, a tool and
/// something you carry, and every one of those is true; what a player
/// looking for it would call it is a tool. So the narrow questions are
/// asked first and the broad ones catch what is left, which is also why
/// the last arm is a plain `else` -- a section that could be empty
/// because a predicate moved is a section with nothing in it and no
/// error anywhere.
pub fn section_of(block: BlockId) -> Section {
    if primitive_shared::blocks::definition(block).tool.is_some()
        || types::is_weapon(block)
        || types::is_implement(block)
    {
        Section::Tools
    } else if primitive_shared::equipment::garment(block).is_some() {
        Section::Clothes
    } else if primitive_shared::food::is_food(block) {
        Section::Food
    } else if !types::is_item(block) {
        Section::Blocks
    } else {
        Section::Stuff
    }
}

/// The rows the player sees, in the order they see them.
///
/// Table order, always: `ALL_BLOCK_IDS` is written in the order of the
/// ages, so stone comes before copper and copper before iron, and two
/// rows never swap places between one search and the next.
pub fn entries(section: Section, query: &str) -> Vec<(BlockId, &'static str)> {
    let query = query.trim().to_lowercase();
    ALL_BLOCK_IDS
        .iter()
        .copied()
        .filter(|&(block, name)| {
            // The printed name, in any language, as well as the identifier:
            // the rows read "Медный слиток" to a Russian player now, and a
            // search for what is on the screen has to find it -- while
            // `copper_ingot`, which is what `/give` takes, still does.
            section.admits(block) && crate::ui::names::block_found(name, &query)
        })
        .collect()
}

/// How many of a thing one tap asks for.
///
/// **Three chips rather than a second mouse button.** A right click
/// could have meant "a stack" on a desktop and it would have meant
/// nothing at all on glass, where there is one kind of tap; a number
/// chosen before the tap works the same in both hands. One is the
/// default because this is the screen that can fill a pack with a
/// hundred axes by accident.
pub const AMOUNTS: [u32; 3] = [1, 10, 100];

// ---- layout ----
//
// Everything is placed inside the `body` the journal hands over, and
// every rectangle comes from one function that both the drawing and the
// hit-testing call. See the note in `CLAUDE.md`: anything that
// hit-tests must be the exact inverse of what draws it.

const GAP: f32 = 0.014;

fn chip_height() -> f32 {
    widgets::tappable(0.075)
}

/// How tall the bar at the bottom that says what the server answered is.
fn status_height() -> f32 {
    0.062
}

/// How wide a section chip is.
///
/// Shared out of the body rather than fixed, because six fixed chips
/// that fit a 16:9 window run off a square one -- and the journal is as
/// wide as the window is.
fn section_width(body: Rect) -> f32 {
    (body.width() - GAP * (SECTIONS.len() as f32 - 1.0)) / SECTIONS.len() as f32
}

/// Where a section chip is: the top row of the page.
pub fn section_rect(section: Section, body: Rect) -> Rect {
    let index = SECTIONS.iter().position(|s| *s == section).unwrap_or(0) as f32;
    let width = section_width(body);
    let x0 = body.x0 + index * (width + GAP);
    Rect::new(x0, body.y1 - chip_height(), x0 + width, body.y1)
}

const AMOUNT_WIDTH: f32 = 0.17;

/// How wide an amount chip is.
///
/// **The whole row where there is no field to share it with.** The
/// search field is drawn only where there are keys (see
/// [`GiveScreen::paint`]), so on glass the rest of this row was a strip
/// of nothing between three small chips and the edge of the page --
/// dead glass on the one device that has none to spare. Wide chips fill
/// it and are easier to hit, which is the same answer twice.
fn amount_width(body: Rect) -> f32 {
    if widgets::touch_layout() {
        (body.width() - GAP * (AMOUNTS.len() as f32 - 1.0)) / AMOUNTS.len() as f32
    } else {
        AMOUNT_WIDTH
    }
}

/// Where an amount chip is: the second row, at the left.
pub fn amount_rect(amount: u32, body: Rect) -> Rect {
    let index = AMOUNTS.iter().position(|a| *a == amount).unwrap_or(0) as f32;
    let width = amount_width(body);
    let x0 = body.x0 + index * (width + GAP);
    let y1 = body.y1 - chip_height() - GAP;
    Rect::new(x0, y1 - chip_height(), x0 + width, y1)
}

/// Where the search field is: the rest of the second row.
pub fn search_rect(body: Rect) -> Rect {
    let last = amount_rect(AMOUNTS[AMOUNTS.len() - 1], body);
    let x0 = (last.x1 + GAP * 2.0).min(body.x1);
    Rect::new(x0, last.y0, body.x1, last.y1)
}

/// The grid the things are in.
pub fn list_rect(body: Rect) -> Rect {
    let top = amount_rect(AMOUNTS[0], body).y0 - GAP;
    Rect::new(body.x0, body.y0 + status_height() + GAP, body.x1, top)
}

/// The bar along the bottom that says what the server answered.
pub fn status_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0, body.x1, body.y0 + status_height())
}

/// The narrowest a cell may be.
///
/// A block name is up to twenty characters (`pegged_birch_planks`) and
/// the picture takes the height of the row; below this the name is
/// clipped on every row rather than on the long ones, which is a grid of
/// pictures with initials beside them.
const MIN_CELL: f32 = 0.66;

/// How many things stand side by side.
///
/// **A grid rather than a column, and that is the whole reason this
/// screen is usable.** Two hundred and fifty rows one under another is
/// eighteen screenfuls; six across is three. The column count comes off
/// the width, so a phone held sideways gets its six and a square window
/// gets its two -- and the hit-test is the same function either way.
pub fn columns(body: Rect) -> usize {
    ((list_rect(body).width() / MIN_CELL).floor() as usize).max(1)
}

pub fn cell_height() -> f32 {
    widgets::tappable(0.082)
}

/// How many rows of the grid fit.
pub fn visible_rows(body: Rect) -> usize {
    ((list_rect(body).height() / cell_height()).floor() as usize).max(1)
}

/// How many things are on the page at once.
pub fn visible_cells(body: Rect) -> usize {
    visible_rows(body) * columns(body)
}

/// Where the cell in `place` on the page is, reading left to right and
/// then down -- the order everything on a screen in this game is read
/// in, and the order `entries` hands the rows over in.
pub fn cell_rect(place: usize, body: Rect) -> Rect {
    let list = list_rect(body);
    let columns = columns(body);
    let width = list.width() / columns as f32;
    let (column, row) = (place % columns, place / columns);
    let x0 = list.x0 + column as f32 * width;
    let y1 = list.y1 - row as f32 * cell_height();
    Rect::new(x0, y1 - cell_height() + 0.004, x0 + width - 0.006, y1)
}

/// Which cell on the page a point is over.
///
/// Walks the drawn rectangles rather than dividing the point by the
/// pitch, so this cannot drift from `cell_rect` when the insets change:
/// the gap between two cells belongs to neither of them, in the
/// hit-test exactly as on the screen.
pub fn cell_at(at: (f32, f32), body: Rect) -> Option<usize> {
    (0..visible_cells(body)).find(|&place| cell_rect(place, body).contains(at.0, at.1))
}

/// Which section chip a point is over.
pub fn section_at(at: (f32, f32), body: Rect) -> Option<Section> {
    SECTIONS.into_iter().find(|s| section_rect(*s, body).contains(at.0, at.1))
}

/// Which amount chip a point is over.
pub fn amount_at(at: (f32, f32), body: Rect) -> Option<u32> {
    AMOUNTS.into_iter().find(|a| amount_rect(*a, body).contains(at.0, at.1))
}

/// What the server last said about a give, as this screen understands it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub enum Status {
    #[default]
    Idle,
    /// A command has gone and no answer has come back yet.
    Asking,
    Given {
        name: &'static str,
        count: u32,
    },
    /// It was given and some of it did not fit.
    PackFull,
    /// The server says this player is not an operator. **The state this
    /// whole screen's feedback exists for**: without it the menu is a
    /// list of things that do nothing when tapped, which reads as a
    /// broken game rather than as a permission the player has not got.
    Denied,
    /// The server refused for a reason this screen has no words of its
    /// own for. Its line is shown as it came, in English, because a
    /// wrong translation of an unexpected sentence is worse than an
    /// untranslated true one.
    Refused(String),
}

/// The longest search worth typing. Past this the field would run under
/// the edge of the window, and the longest block name is shorter.
const QUERY_LIMIT: usize = 24;

/// The menu's own state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GiveScreen {
    section: Section,
    query: String,
    /// In whole rows of the grid.
    scroll: usize,
    amount: Option<u32>,
    status: Status,
    /// The chat line the frame loop has not sent yet.
    pending: Option<String>,
}

impl GiveScreen {
    /// How many one tap asks for.
    pub fn amount(&self) -> u32 {
        self.amount.unwrap_or(AMOUNTS[0])
    }

    #[cfg(test)]
    pub fn status(&self) -> &Status {
        &self.status
    }

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

    /// Scrolls by whole rows of the grid, stopping at both ends.
    pub fn scroll_by(&mut self, rows: i32, body: Rect) {
        let total = entries(self.section, &self.query).len();
        let columns = columns(body);
        let last_row = total.div_ceil(columns).saturating_sub(visible_rows(body));
        self.scroll = (self.scroll as i64 + rows as i64).clamp(0, last_row as i64) as usize;
    }

    /// A press at `at`. Answers whether it changed anything.
    ///
    /// A tap on a thing does not give it: it writes the command down and
    /// says the screen is asking. The frame loop takes the line with
    /// [`take_command`](Self::take_command) and puts it on the wire, so
    /// the one place in this client that talks to a server is still the
    /// one place that talks to a server.
    pub fn click(&mut self, at: (f32, f32), body: Rect) -> bool {
        if let Some(section) = section_at(at, body) {
            let changed = section != self.section;
            self.section = section;
            self.scroll = 0;
            return changed;
        }
        if let Some(amount) = amount_at(at, body) {
            let changed = amount != self.amount();
            self.amount = Some(amount);
            return changed;
        }
        if let Some(place) = cell_at(at, body) {
            let rows = entries(self.section, &self.query);
            let first = self.scroll * columns(body);
            if let Some(&(_, name)) = rows.get(first + place) {
                self.pending = Some(format!("/give {name} {}", self.amount()));
                self.status = Status::Asking;
                return true;
            }
        }
        false
    }

    /// The chat line this screen wants sent, if it has one.
    pub fn take_command(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// Offers a line the server said in its own name to this screen.
    ///
    /// Answers whether it was this screen's -- in which case the caller
    /// must **not** also put it in the chat log. Only ever true while a
    /// give is outstanding, so `/time` typed into the chat box still
    /// answers into the chat box.
    ///
    /// There is always exactly one line: `run_command` answers a
    /// `/give` with one whichever way it goes (given, refused, no such
    /// block, not an operator), which is why this screen needs no timer
    /// to stop saying "asking". A plugin's `on_chat` veto is the one
    /// thing that can swallow the command instead, and it would swallow
    /// a typed one just the same.
    pub fn take_reply(&mut self, text: &str) -> bool {
        if self.status != Status::Asking {
            return false;
        }
        self.status = read_reply(text);
        true
    }

    /// Everything `paint` reads that belongs to this screen.
    pub fn key(&self) -> impl std::hash::Hash + '_ {
        (self.section, &self.query, self.scroll, self.amount, &self.status)
    }

    /// Draws the menu into `body`.
    pub fn paint(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        body: Rect,
        cursor: Option<(f32, f32)>,
        language: Language,
    ) {
        let hovered = |rect: Rect| cursor.is_some_and(|(x, y)| rect.contains(x, y));

        for section in SECTIONS {
            let rect = section_rect(section, body);
            let chosen = section == self.section;
            p.button(rect, language.text(section.label()), hovered(rect) || chosen, true);
            if chosen {
                p.border(rect, 0.004, ACCENT);
            }
        }
        for amount in AMOUNTS {
            let rect = amount_rect(amount, body);
            let chosen = amount == self.amount();
            p.button(rect, &format!("x{amount}"), hovered(rect) || chosen, true);
            if chosen {
                p.border(rect, 0.004, ACCENT);
            }
        }
        // The field only where there are keys to type into it with --
        // the recipe book's rule, for the recipe book's reason: a
        // phone's keyboard covers half the glass and this page is the
        // other half. The sections are what divide the list for a
        // finger, which is why there are six of them and not two.
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

        let list = list_rect(body);
        p.well(list, WELL);
        let rows = entries(self.section, &self.query);
        if rows.is_empty() {
            p.text(
                language.text(Msg::RecipesNoMatch),
                list.x0 + 0.04,
                list.y1 - 0.06,
                0.9,
                widgets::TEXT_DIM,
            );
        }
        let first = self.scroll * columns(body);
        for (place, &(block, name)) in rows.iter().skip(first).take(visible_cells(body)).enumerate() {
            let rect = cell_rect(place, body);
            if hovered(rect) {
                p.quad(rect, HOVER);
            }
            let side = rect.height() - 0.014;
            let icon = Rect::new(rect.x0 + 0.010, rect.y0 + 0.007, rect.x0 + 0.010 + side, rect.y1 - 0.007);
            textured(
                p,
                icon,
                icon_layer(layers, block),
                crate::ui::hotbar::icon_tint(block, [1.0, 1.0, 1.0, 1.0]),
            );
            let room = rect.x1 - icon.x1 - 0.03;
            let label = Rect::new(icon.x1 + 0.018, rect.y0, rect.x1, rect.y1);
            p.label_left(label, &widgets::fit(&crate::ui::names::identified(name, language), 0.85, room), 0.0, 0.85, widgets::TEXT);
        }

        // ...and what the server said, which is the half of this screen
        // that makes a refusal legible.
        let status = status_rect(body);
        p.well(status, WELL);
        let (text, ink) = self.status_line(language);
        p.label_left(status, &widgets::fit(&text, 0.85, status.width() - 0.06), 0.03, 0.85, ink);
    }

    /// What the bar at the bottom says, and in what ink.
    fn status_line(&self, language: Language) -> (String, [f32; 4]) {
        match &self.status {
            Status::Idle => (language.text(Msg::GiveHint).to_string(), widgets::TEXT_DIM),
            Status::Asking => (language.text(Msg::GiveAsking).to_string(), widgets::TEXT_DIM),
            Status::Given { name, count } => (
                format!("{} {count}x {}", language.text(Msg::GiveGiven), crate::ui::names::identified(name, language)),
                widgets::TEXT_GOOD,
            ),
            Status::PackFull => (language.text(Msg::GivePackFull).to_string(), widgets::TEXT_BAD),
            Status::Denied => (language.text(Msg::GiveDenied).to_string(), widgets::TEXT_BAD),
            Status::Refused(line) => (
                format!("{} {line}", language.text(Msg::GiveRefused)),
                widgets::TEXT_BAD,
            ),
        }
    }
}

/// What one line from the server means to this screen.
///
/// Kept apart from the state so the mapping can be tested against the
/// sentences the *real* server builds -- see the module note and
/// `what_the_real_server_answers_a_give_is_what_this_screen_reads`.
fn read_reply(text: &str) -> Status {
    // Asked before the success, not after: "gave" never appears in a
    // refusal, but reading the refusal first is what keeps this arm
    // from depending on that.
    if text.contains("operator-only") {
        return Status::Denied;
    }
    let Some(rest) = text.strip_prefix("gave ") else {
        return Status::Refused(text.to_string());
    };
    if text.contains("would not fit") {
        return Status::PackFull;
    }
    // "gave 10 cobblestone" -- a count and the game's own name for the
    // block. Read back rather than remembered from the tap, because the
    // server clamps the count (`MAX_STACK`) and the number that matters
    // is the one it actually handed over.
    let mut words = rest.splitn(2, ' ');
    let count = words.next().and_then(|n| n.parse::<u32>().ok());
    let name = words
        .next()
        .and_then(|name| ALL_BLOCK_IDS.iter().find(|&&(_, known)| known == name))
        .map(|&(_, known)| known);
    match (count, name) {
        (Some(count), Some(name)) => Status::Given { name, count },
        _ => Status::Refused(text.to_string()),
    }
}

const ACCENT: [f32; 4] = [0.95, 0.72, 0.30, 1.0];
const WELL: [f32; 4] = [0.08, 0.075, 0.07, 0.96];
const HOVER: [f32; 4] = [0.16, 0.15, 0.14, 1.0];

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_COBBLESTONE, BLOCK_STONE_AXE};

    fn body(aspect: f32) -> Rect {
        crate::ui::journal::body_rect(aspect)
    }

    /// The shapes a window can be, including the phone this is cut for.
    const ASPECTS: [f32; 4] = [1.0, 4.0 / 3.0, 16.0 / 9.0, 2712.0 / 1220.0];

    #[test]
    fn every_block_in_the_game_is_in_exactly_one_section() {
        // The property that makes the chips a division of the list
        // rather than five searches over it. A block in two sections
        // would be counted twice by ALL and appear under both; a block
        // in none would be a thing `/give` accepts that this menu can
        // never offer.
        for &(block, name) in ALL_BLOCK_IDS {
            let mine: Vec<Section> = SECTIONS
                .into_iter()
                .filter(|s| *s != Section::All && s.admits(block))
                .collect();
            assert_eq!(mine.len(), 1, "{name} is in {mine:?}");
        }
        let counted: usize = SECTIONS
            .into_iter()
            .filter(|s| *s != Section::All)
            .map(|s| entries(s, "").len())
            .sum();
        assert_eq!(counted, entries(Section::All, "").len());
        assert_eq!(counted, ALL_BLOCK_IDS.len());
    }

    #[test]
    fn no_section_is_empty() {
        // A chip that lists nothing is a chip that looks broken. If a
        // predicate in `primitive_shared` moves under this file, this
        // is what says so.
        for section in SECTIONS {
            assert!(!entries(section, "").is_empty(), "{section:?} lists nothing");
        }
    }

    #[test]
    fn the_search_finds_a_thing_by_the_name_the_game_prints() {
        let found = |query: &str| {
            entries(Section::All, query)
                .iter()
                .any(|&(block, _)| block == BLOCK_COBBLESTONE)
        };
        assert!(found("cobble"), "cobblestone could not be found by name");
        assert!(found("  COBBLE "), "the search should not care about case or spaces");
        assert!(!found("obsidian"), "a search for a thing not in the game found one");
        assert!(entries(Section::All, "zzzz").is_empty());
    }

    #[test]
    fn the_search_and_the_section_narrow_together() {
        // Either alone leaves too much; a player who has typed "axe"
        // and is on TOOLS must not be shown a block called "axe head"
        // from another section.
        let axes = entries(Section::Tools, "axe");
        assert!(!axes.is_empty());
        assert!(axes.iter().all(|&(block, _)| section_of(block) == Section::Tools));
        assert!(axes.iter().any(|&(block, _)| block == BLOCK_STONE_AXE));
    }

    #[test]
    fn every_chip_and_every_cell_is_pressed_where_it_is_drawn() {
        // The rule from CLAUDE.md, for this screen: get it wrong and the
        // menu looks right and gives you the thing beside the one you
        // tapped.
        for aspect in ASPECTS {
            let body = body(aspect);
            for section in SECTIONS {
                let rect = section_rect(section, body);
                assert_eq!(section_at((rect.centre_x(), rect.centre_y()), body), Some(section), "aspect {aspect}");
                assert!(rect.x1 <= body.x1 + 1e-4, "{section:?} ran off a window of aspect {aspect}");
                assert!(rect.x0 >= body.x0 - 1e-4, "{section:?} started off a window of aspect {aspect}");
            }
            for amount in AMOUNTS {
                let rect = amount_rect(amount, body);
                assert_eq!(amount_at((rect.centre_x(), rect.centre_y()), body), Some(amount), "aspect {aspect}");
                assert!(rect.y0 >= list_rect(body).y1 - 1e-4, "x{amount} reaches into the grid at aspect {aspect}");
            }
            for place in 0..visible_cells(body) {
                let rect = cell_rect(place, body);
                assert_eq!(cell_at((rect.centre_x(), rect.centre_y()), body), Some(place), "aspect {aspect}");
                assert!(rect.y0 >= list_rect(body).y0 - 1e-4, "cell {place} fell out of the grid at aspect {aspect}");
                assert!(rect.x1 <= list_rect(body).x1 + 1e-4, "cell {place} ran off the grid at aspect {aspect}");
                assert!(rect.y1 <= list_rect(body).y1 + 1e-4, "cell {place} is over the chips at aspect {aspect}");
            }
            assert!(status_rect(body).y1 <= list_rect(body).y0 + 1e-4, "the status bar is under the grid");
        }
    }

    #[test]
    fn two_cells_never_overlap() {
        // The other half of the rule above: a hit-test that walks the
        // drawn rectangles finds the *first* match, so two cells sharing
        // a pixel would be one cell nobody can press.
        for aspect in ASPECTS {
            let body = body(aspect);
            for a in 0..visible_cells(body) {
                for b in (a + 1)..visible_cells(body) {
                    let (x, y) = (cell_rect(a, body), cell_rect(b, body));
                    assert!(
                        x.x1 <= y.x0 || y.x1 <= x.x0 || x.y1 <= y.y0 || y.y1 <= x.y0,
                        "cells {a} and {b} overlap at aspect {aspect}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_phone_gets_chips_and_cells_a_finger_tall() {
        widgets::as_a_phone(|| {
            let body = body(2712.0 / 1220.0);
            assert!(cell_height() >= widgets::FINGER_SIDE - 1e-5);
            assert!(section_rect(Section::All, body).height() >= widgets::FINGER_SIDE - 1e-5);
            assert!(amount_rect(AMOUNTS[0], body).height() >= widgets::FINGER_SIDE - 1e-5);
            // ...and the grid still holds something after the chips have
            // taken their finger's worth twice over.
            assert!(visible_cells(body) >= 12, "only {} things fit on a phone", visible_cells(body));
            // ...and the amount row is the width of the page rather than
            // three small chips and a strip of nothing, because the
            // field that would have shared it is not drawn here.
            let last = amount_rect(AMOUNTS[AMOUNTS.len() - 1], body);
            assert!(last.x1 >= body.x1 - GAP, "the amount chips leave dead glass on a phone");
            assert!(last.x1 <= body.x1 + 1e-4, "the amount chips run off a phone");
        });
    }

    #[test]
    fn the_phone_shows_several_things_across_and_a_square_window_still_works() {
        widgets::as_a_phone(|| {
            assert!(columns(body(2712.0 / 1220.0)) >= 4);
        });
        assert!(columns(body(1.0)) >= 1);
    }

    #[test]
    fn scrolling_stops_at_both_ends_of_the_list() {
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        screen.scroll_by(-5, body);
        assert_eq!(screen.scroll, 0, "the list scrolled off the top");
        screen.scroll_by(10_000, body);
        let last = ALL_BLOCK_IDS.len().div_ceil(columns(body)) - visible_rows(body);
        assert_eq!(screen.scroll, last);
        // ...and the last row is still a row with things on it.
        let rows = entries(Section::All, "");
        assert!(rows.get(screen.scroll * columns(body)).is_some(), "the last page is empty");
    }

    #[test]
    fn a_short_list_does_not_scroll_at_all() {
        // Held by `div_ceil` rather than by luck: a section that fits on
        // one page used to be scrollable into blank space.
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen { section: Section::Clothes, ..GiveScreen::default() };
        if entries(Section::Clothes, "").len() <= visible_cells(body) {
            screen.scroll_by(10_000, body);
            assert_eq!(screen.scroll, 0);
        }
    }

    #[test]
    fn tapping_a_thing_writes_the_command_an_operator_would_have_typed() {
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        let cell = cell_rect(0, body);
        assert!(screen.click((cell.centre_x(), cell.centre_y()), body));
        let first = entries(Section::All, "")[0].1;
        assert_eq!(screen.take_command().as_deref(), Some(format!("/give {first} 1").as_str()));
        assert_eq!(screen.take_command(), None, "the same command went twice");
        assert_eq!(*screen.status(), Status::Asking);
    }

    #[test]
    fn the_amount_chip_is_what_the_command_asks_for() {
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        let hundred = amount_rect(100, body);
        screen.click((hundred.centre_x(), hundred.centre_y()), body);
        let cell = cell_rect(0, body);
        screen.click((cell.centre_x(), cell.centre_y()), body);
        assert!(screen.take_command().is_some_and(|line| line.ends_with(" 100")));
    }

    #[test]
    fn a_tap_on_the_page_with_nothing_under_it_asks_for_nothing() {
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        // Between the chips and the grid, where the page is bare.
        let gap = (body.x0 + 0.01, list_rect(body).y1 + GAP / 2.0);
        assert!(!screen.click(gap, body));
        assert_eq!(screen.take_command(), None);
        assert_eq!(*screen.status(), Status::Idle);
    }

    #[test]
    fn a_search_that_hides_a_thing_hides_it_from_the_tap_as_well() {
        // The failure this prevents: the grid draws the filtered list
        // and the tap indexes the whole one, so tapping "stone" hands
        // you whatever was at that place before you typed.
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        for c in "cobble".chars() {
            screen.type_char(c);
        }
        let cell = cell_rect(0, body);
        screen.click((cell.centre_x(), cell.centre_y()), body);
        let wanted = entries(Section::All, "cobble")[0].1;
        assert_eq!(screen.take_command().as_deref(), Some(format!("/give {wanted} 1").as_str()));
    }

    #[test]
    fn a_scrolled_page_gives_the_thing_that_is_drawn_in_the_cell() {
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        screen.scroll_by(2, body);
        let cell = cell_rect(0, body);
        screen.click((cell.centre_x(), cell.centre_y()), body);
        let wanted = entries(Section::All, "")[2 * columns(body)].1;
        assert_eq!(screen.take_command().as_deref(), Some(format!("/give {wanted} 1").as_str()));
    }

    #[test]
    fn the_search_is_a_search_and_not_a_way_to_type_anything() {
        let mut screen = GiveScreen::default();
        for _ in 0..100 {
            screen.type_char('a');
        }
        assert_eq!(screen.query().chars().count(), QUERY_LIMIT);
        screen.type_char('\n');
        assert_eq!(screen.query().chars().count(), QUERY_LIMIT, "a control character got in");
        screen.backspace();
        assert_eq!(screen.query().chars().count(), QUERY_LIMIT - 1);
    }

    #[test]
    fn a_refusal_lands_in_the_menu_and_is_taken_out_of_the_chat() {
        // Both halves matter. The player has to be told why the menu
        // does nothing; and the line must not also go to the chat log,
        // which is what "Отказы больше не пишутся в чат" took out.
        let body = body(16.0 / 9.0);
        let mut screen = GiveScreen::default();
        let cell = cell_rect(0, body);
        screen.click((cell.centre_x(), cell.centre_y()), body);
        assert!(screen.take_reply("'give' is operator-only"));
        assert_eq!(*screen.status(), Status::Denied);
        for language in Language::ALL {
            let (line, ink) = screen.status_line(*language);
            assert!(!line.is_empty(), "the refusal says nothing in {language:?}");
            assert_eq!(ink, widgets::TEXT_BAD, "a refusal must not read as good news");
        }
    }

    #[test]
    fn a_server_line_that_answers_nothing_this_screen_asked_stays_in_the_chat() {
        // The other side of swallowing: `/time` typed into the chat box
        // answers into the chat box, and a join notice is nobody's give.
        let mut screen = GiveScreen::default();
        assert!(!screen.take_reply("it is morning"));
        assert_eq!(*screen.status(), Status::Idle);

        let body = body(16.0 / 9.0);
        let cell = cell_rect(0, body);
        screen.click((cell.centre_x(), cell.centre_y()), body);
        assert!(screen.take_reply("gave 1 grass"));
        // One answer per ask: the next line belongs to whoever else is
        // talking.
        assert!(!screen.take_reply("alice joined"));
    }

    #[test]
    fn what_the_server_says_it_did_is_what_the_menu_reads_back() {
        assert_eq!(read_reply("gave 10 cobblestone"), Status::Given { name: "cobblestone", count: 10 });
        // The count is the server's, not the one that was asked for:
        // `/give x 1000` is clamped to a stack and the menu must not
        // claim a thousand.
        assert_eq!(read_reply("gave 128 stone"), Status::Given { name: "stone", count: 128 });
        assert_eq!(read_reply("gave 90 stone, 38 would not fit"), Status::PackFull);
        assert_eq!(read_reply("'give' is operator-only"), Status::Denied);
        assert_eq!(
            read_reply("no block called 'unobtainium'"),
            Status::Refused("no block called 'unobtainium'".to_string())
        );
    }

    #[test]
    fn what_the_real_server_answers_a_give_is_what_this_screen_reads() {
        // **The test that makes reading English off the wire honest.**
        // This screen has no protocol message of its own; it recognises
        // the server's own sentence. So it is checked against the server
        // that builds it -- the real parser, the real permission check
        // -- rather than against a copy of the string kept here. Reword
        // the refusal in `commands.rs` and this goes red in the same
        // commit.
        use primitive_server::logic::commands::{authorize, parse, Permission, Response};
        let command = parse("/give cobblestone 10").expect("the menu's own line parses");
        let refused = authorize(command.clone(), Permission::Player, Some(1));
        let Response::Denied(line) = refused else {
            panic!("a plain player was allowed to give themselves things: {refused:?}");
        };
        assert_eq!(read_reply(&line), Status::Denied, "the server's refusal was not read as one: {line:?}");
        // ...and the same line the operator's path takes, which is the
        // one this menu hopes for.
        assert_eq!(
            authorize(command, Permission::Operator, Some(1)),
            Response::Give { block: "cobblestone".to_string(), count: 10 },
            "the command this menu sends is not the one the server acts on"
        );
    }

    #[test]
    fn every_thing_the_menu_offers_is_a_name_the_server_would_accept() {
        // The menu's list and `/give`'s lookup are the same table, and
        // this is what says so through the real parser: a row whose name
        // needed quoting or had a space in it would be a row that looks
        // givable and answers "no block called".
        use primitive_server::logic::commands::{parse, Command};
        for &(_, name) in ALL_BLOCK_IDS {
            let line = format!("/give {name} 1");
            match parse(&line) {
                Ok(Command::Give { block, count }) => {
                    assert_eq!(block, name, "{line} parsed as a different block");
                    assert_eq!(count, 1);
                }
                other => panic!("{line} is not a give the server understands: {other:?}"),
            }
        }
    }

    #[test]
    fn the_status_bar_says_something_in_every_language_for_every_state() {
        let screen = GiveScreen::default();
        for status in [
            Status::Idle,
            Status::Asking,
            Status::Given { name: "stone", count: 10 },
            Status::PackFull,
            Status::Denied,
            Status::Refused("no block called 'x'".to_string()),
        ] {
            let mut screen = screen.clone();
            screen.status = status.clone();
            for language in Language::ALL {
                let (line, _) = screen.status_line(*language);
                assert!(!line.trim().is_empty(), "{status:?} says nothing in {language:?}");
            }
        }
    }
}
