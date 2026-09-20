//! **The ladder page**: seven ages, which of them these hands have
//! climbed, what the next one wants and where those things are.
//!
//! ## Why a page and not a quest
//!
//! The player said the progression was completely unclear, and the honest
//! reading of that is not "there is no tutorial" -- it is that the game
//! knew the shape of its own ladder and never showed it. `progression`
//! walks the whole thing from bare hands to steel, in a test; the recipe
//! book lists four hundred rows in table order; the crafting grid answers
//! only "what can I make this second". None of the three answers "where am
//! I and what is above me".
//!
//! So this page, and nothing else: no marker in the world, no arrow, no
//! reward for a rung, no line that appears when one is taken. What it adds
//! is a *fact the player has earned* -- see `ladder`'s note on how a rung
//! counts as taken -- laid out in the order the world is climbed in. The
//! rung above the one you are on says what it wants and what kind of place
//! that comes from; where *exactly* is still the land's to answer, and the
//! map is the next tab along.
//!
//! ## Why it opens on the next rung
//!
//! A page that opened at the bottom would show a player of forty hours the
//! stone age. The question this page exists for is "what now", so the row
//! read out when it opens is the one *above where they stand*
//! (`ladder::working_towards`) -- not the lowest gap, which for a player
//! handed an ingot on a server would be the campfire they never built. A
//! player who wants the whole ladder still has all seven rows in front of
//! them, in order, at a glance, gaps and all.
//!
//! ## The layout is the book's
//!
//! Rows down the left, one thing read out on the right, the same row
//! height and the same well: two pages of one journal that behaved
//! differently under a finger would be two things to learn. Everything
//! here is hit-tested by the same functions that draw it, which is the
//! rule the whole interface is held to.

use primitive_shared::discovery::Discovered;
use primitive_shared::ladder::{self, Age, Found, Rung, LADDER};
use primitive_shared::types::{block_kind, BlockId};

use crate::engine::texture::FaceLayers;
use crate::ui::inventory_screen::{icon_layer, textured};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// What this language calls an age.
pub fn age_name(age: Age) -> Msg {
    match age {
        Age::BareHands => Msg::AgeBareHands,
        Age::Flint => Msg::AgeFlint,
        Age::Fire => Msg::AgeFire,
        Age::Clay => Msg::AgeClay,
        Age::Copper => Msg::AgeCopper,
        Age::Bronze => Msg::AgeBronze,
        Age::Iron => Msg::AgeIron,
    }
}

/// What this language says for one of the first two minutes' prompts.
pub fn first_step_msg(step: ladder::FirstStep) -> Msg {
    use ladder::FirstStep;
    match step {
        FirstStep::Stone => Msg::StepStone,
        FirstStep::Fibre => Msg::StepFibre,
        FirstStep::Flake => Msg::StepFlake,
    }
}

/// ...and the kind of place its materials come from.
pub fn found_text(found: Found) -> Msg {
    match found {
        Found::Underfoot => Msg::FoundUnderfoot,
        Found::Riverbank => Msg::FoundRiverbank,
        Found::Hills => Msg::FoundHills,
        Found::DeepRock => Msg::FoundDeepRock,
        Found::Woods => Msg::FoundWoods,
    }
}

// ---- layout ----

const GAP: f32 = 0.014;

/// How tall one rung's row is. A finger, as every list in the journal is.
pub fn row_height() -> f32 {
    widgets::tappable(0.09)
}

/// The column the seven rungs are in.
pub fn list_rect(body: Rect) -> Rect {
    let width = (body.width() * 0.46).clamp(0.8, 1.6).min(body.width());
    Rect::new(body.x0, body.y0, body.x0 + width, body.y1)
}

/// The pane that reads one rung out.
pub fn detail_rect(body: Rect) -> Rect {
    let list = list_rect(body);
    Rect::new(list.x1 + GAP * 2.0, body.y0, body.x1, list.y1)
}

/// Where the rung at `index` in `ladder::LADDER` is drawn.
pub fn row_rect(index: usize, body: Rect) -> Rect {
    let list = list_rect(body);
    let y1 = list.y1 - index as f32 * row_height();
    Rect::new(list.x0, y1 - row_height() + 0.004, list.x1, y1)
}

/// Which rung a point is over.
pub fn row_at(at: (f32, f32), body: Rect) -> Option<usize> {
    (0..LADDER.len()).find(|&index| row_rect(index, body).contains(at.0, at.1))
}

/// What the page is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LadderScreen {
    /// Into `ladder::LADDER`. `None` reads out the rung the player is
    /// working towards -- see the module note.
    selected: Option<usize>,
}

impl LadderScreen {
    /// Which rung is read out on the right.
    fn chosen(&self, held: &Discovered) -> usize {
        self.selected.unwrap_or_else(|| {
            ladder::working_towards(held)
                .and_then(|next| LADDER.iter().position(|rung| rung.age == next.age))
                // Standing on the top rung: read that one out, rather than
                // an empty pane.
                .unwrap_or(LADDER.len() - 1)
        })
    }

    /// A press at `at`. Answers whether it changed anything.
    pub fn click(&mut self, at: (f32, f32), body: Rect) -> bool {
        let Some(index) = row_at(at, body) else {
            return false;
        };
        let changed = self.selected != Some(index);
        self.selected = Some(index);
        changed
    }

    /// Everything `paint` reads that belongs to the page.
    pub fn key(&self) -> Option<usize> {
        self.selected
    }

    /// Draws the page into `body`.
    pub fn paint(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        held: &Discovered,
        body: Rect,
        cursor: Option<(f32, f32)>,
        language: Language,
    ) {
        let hovered = |rect: Rect| cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let list = list_rect(body);
        p.well(list, WELL);
        let chosen = self.chosen(held);
        let standing = ladder::standing_on(held).map(|rung| rung.age);

        for (index, rung) in LADDER.iter().enumerate() {
            let rect = row_rect(index, body);
            let taken = rung.taken(held);
            if index == chosen {
                p.quad(rect, CHOSEN);
                p.border(rect, 0.003, ACCENT);
            } else if hovered(rect) {
                p.quad(rect, HOVER);
            }
            // The rung's own picture: the thing that *is* this age, greyed
            // while it has never been held. A picture rather than a number,
            // because "copper" means the ingot and the player has seen one.
            let side = rect.height() - 0.024;
            let icon = Rect::new(rect.x0 + 0.014, rect.y0 + 0.012, rect.x0 + 0.014 + side, rect.y1 - 0.012);
            textured(p, icon, icon_layer(layers, rung.marks[0]), tint(rung.marks[0], taken));

            // **The rung is climbed or it is not**, and that is said by ink
            // rather than by a tick: a column of ticks and empty boxes is a
            // checklist, and a checklist is the chore this page is not.
            let name = language.text(age_name(rung.age));
            let ink = if taken { widgets::TEXT } else { widgets::TEXT_DIM };
            let here = standing == Some(rung.age);
            let tail = if here { language.text(Msg::LadderYouAreHere) } else { "" };
            let tail_width = if tail.is_empty() { 0.0 } else { widgets::measure(tail, 0.75) + 0.03 };
            let text_left = icon.x1 + 0.024;
            let name_rect = Rect::new(text_left, rect.y0, rect.x1 - tail_width, rect.y1);
            p.label_left(name_rect, &widgets::fit(name, 1.0, name_rect.width() - 0.02), 0.0, 1.0, ink);
            if here {
                // Centred in the row's own rectangle, the way the name is:
                // a baseline worked out by hand sat ten pixels low, and
                // "you are here" reading as though it belonged to the rung
                // below is the one thing this label must never do.
                let tail_rect = Rect::new(rect.x1 - tail_width, rect.y0, rect.x1 - 0.012, rect.y1);
                p.label_in(tail_rect, tail, 0.75, ACCENT);
            }
        }

        let pane = detail_rect(body);
        if pane.width() < 0.4 {
            return;
        }
        p.well(pane, WELL);
        read_out(p, layers, held, &LADDER[chosen], pane, language);
    }
}

const ACCENT: [f32; 4] = [0.95, 0.72, 0.30, 1.0];
const WELL: [f32; 4] = [0.08, 0.075, 0.07, 0.96];
const CHOSEN: [f32; 4] = [0.22, 0.18, 0.11, 1.0];
const HOVER: [f32; 4] = [0.16, 0.15, 0.14, 1.0];
const HEADING: [f32; 4] = [0.95, 0.72, 0.30, 1.0];
const GOOD: [f32; 4] = [0.55, 0.85, 0.45, 1.0];

/// Greyed until it has been held: the same promise-versus-thing the recipe
/// book draws a lead's icon with.
fn tint(block: BlockId, held: bool) -> [f32; 4] {
    let colour = crate::ui::hotbar::icon_tint(block, [1.0, 1.0, 1.0, 1.0]);
    if held {
        colour
    } else {
        [colour[0] * 0.55, colour[1] * 0.55, colour[2] * 0.55, colour[3]]
    }
}

/// The right-hand pane: one rung, read out.
fn read_out(
    p: &mut Painter,
    layers: &FaceLayers,
    held: &Discovered,
    rung: &Rung,
    pane: Rect,
    language: Language,
) {
    let left = pane.x0 + 0.04;
    let room = pane.width() - 0.08;
    let mut top = pane.y1 - 0.06;
    let line = 0.072;

    let title = language.text(age_name(rung.age));
    p.text(&widgets::fit(title, 1.3, room), left, top, 1.3, widgets::TEXT);
    top -= 0.075;
    if rung.taken(held) {
        p.text(language.text(Msg::LadderYouAreHere), left, top, 0.85, GOOD);
    } else {
        p.text(language.text(Msg::LadderNext), left, top, 0.85, HEADING);
    }
    top -= 0.075;

    // What it wants: everything, with the ones never held drawn as they are
    // in the book -- because "copper ore, charcoal, a pot and a mould" to
    // somebody holding three of the four has told them nothing.
    let still: Vec<BlockId> = rung.still_to_find(held).collect();
    p.text(language.text(Msg::LadderWants), left, top, 0.85, HEADING);
    top -= 0.055;
    for &want in rung.wants {
        if top - line < pane.y0 {
            break;
        }
        let have = still.iter().all(|&s| block_kind(s) != block_kind(want));
        let icon = Rect::new(left, top - line + 0.012, left + line - 0.012, top);
        textured(p, icon, icon_layer(layers, want), tint(want, have));
        let ink = if have { GOOD } else { widgets::TEXT };
        p.text(
            &widgets::fit(&crate::ui::names::block(want, language), 0.9, room - line - 0.04),
            icon.x1 + 0.025,
            top - 0.018,
            0.9,
            ink,
        );
        top -= line;
    }
    top -= 0.02;

    // Where those things come from: a kind of place, never a coordinate.
    // See `ladder::Found`.
    if top - 0.05 > pane.y0 {
        let width = ((room / widgets::measure("m", 0.9)).max(8.0)) as usize;
        for (n, text) in widgets::wrap(language.text(found_text(rung.found)), width).iter().enumerate() {
            p.text(text, left, top - n as f32 * 0.05, 0.9, widgets::TEXT_DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_COPPER_INGOT, BLOCK_IRON_INGOT, BLOCK_PEBBLE};

    fn body(aspect: f32) -> Rect {
        crate::ui::journal::body_rect(aspect)
    }

    fn holding(kinds: &[BlockId]) -> Discovered {
        Discovered::from_kinds(kinds.iter().copied())
    }

    #[test]
    fn every_rung_is_pressed_where_it_is_drawn_and_none_of_them_fall_off_the_page() {
        for aspect in [1.0, 4.0 / 3.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            let body = body(aspect);
            for index in 0..LADDER.len() {
                let rect = row_rect(index, body);
                assert_eq!(row_at((rect.centre_x(), rect.centre_y()), body), Some(index), "aspect {aspect}");
                assert!(rect.y0 >= body.y0 - 1e-4, "rung {index} fell off the page at aspect {aspect}");
                assert!(rect.y1 <= body.y1 + 1e-4, "rung {index} is over the top at aspect {aspect}");
            }
        }
    }

    #[test]
    fn a_phone_gets_rungs_a_finger_tall() {
        widgets::as_a_phone(|| assert!(row_height() >= widgets::FINGER_SIDE - 1e-5));
    }

    /// The page's whole job: open it and it is already reading out the
    /// thing to do next.
    #[test]
    fn the_page_opens_on_the_rung_the_player_is_working_towards() {
        let page = LadderScreen::default();
        let copper = holding(&[BLOCK_PEBBLE, BLOCK_COPPER_INGOT]);
        assert_eq!(LADDER[page.chosen(&copper)].age, Age::Bronze);
        // Nothing held at all: the bottom of the ladder, which is where
        // that player is.
        assert_eq!(LADDER[page.chosen(&Discovered::new())].age, Age::BareHands);
        // ...and a player who has held one of every mark is left on the
        // top rung rather than on a page reading out nothing.
        let everything = holding(&LADDER.iter().flat_map(|r| r.marks.iter().copied()).collect::<Vec<_>>());
        assert_eq!(LADDER[page.chosen(&everything)].age, Age::Iron);
    }

    #[test]
    fn pressing_a_rung_reads_that_rung_out_and_it_stays_read_out() {
        let body = body(16.0 / 9.0);
        let mut page = LadderScreen::default();
        let held = holding(&[BLOCK_IRON_INGOT]);
        let rect = row_rect(0, body);
        assert!(page.click((rect.centre_x(), rect.centre_y()), body));
        assert_eq!(LADDER[page.chosen(&held)].age, Age::BareHands);
        assert!(!page.click((rect.centre_x(), rect.centre_y()), body), "the same rung pressed twice was news");
    }

    /// Drawn at every shape a window is, at both ends of the ladder, with
    /// nothing panicking and something on the screen.
    #[test]
    fn the_page_draws_for_a_beginner_and_for_a_smith_at_every_window_shape() {
        for aspect in [1.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            for held in [Discovered::new(), holding(&[BLOCK_IRON_INGOT])] {
                let mut p = Painter::onto_themed(
                    crate::engine::texture::FontAtlas::for_test(),
                    Vec::new(),
                    widgets::Theme::DARK,
                );
                LadderScreen::default().paint(
                    &mut p,
                    &FaceLayers::empty_for_test(),
                    &held,
                    body(aspect),
                    None,
                    Language::Russian,
                );
                assert!(!p.into_vertices().is_empty(), "the ladder page drew nothing at aspect {aspect}");
            }
        }
    }
}
