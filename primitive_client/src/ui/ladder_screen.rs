//! **The path page**: a tab of the pack. Seven ages across the top,
//! which of them these hands have climbed, what the next one wants,
//! where those things are, the handful of controls nobody can guess,
//! and how much of the world this player has turned up.
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
//! map is in the journal.
//!
//! ## Why it is in the pack and not in the journal any more
//!
//! It was the journal's second tab, beside the map and the recipe book,
//! and the argument for that was good: the ladder says what to go for, the
//! map says where, and the book says what it costs. The player's request
//! -- "сделай раздел в инвентаре с обучением и прогрессией" -- is the
//! other half of the same argument, and it wins, for two reasons.
//!
//! The first is what the page is *about*. The journal is three things a
//! player looks **up**: a place, a recipe, an operator's tool. This page
//! is about the player -- what they have done, what is left, what their
//! hands can do -- which is what the whole pack screen is about, and it
//! now sits beside the body page and the pack itself in the order a
//! person reads them: how am I, what have I, where am I going.
//!
//! The second is that the pack is the screen a player is *in*. The
//! journal is opened on purpose, by somebody who already knows they are
//! stuck; the pack is opened every couple of minutes by everybody. A page
//! that teaches the game is no use behind a door only the unstuck open.
//!
//! **And it is in one place, not two.** Leaving the tab in the journal as
//! well was the tempting answer and it is the wrong one: two doors to one
//! page means a player who finds one never goes looking for the other,
//! and every change to the page has to be checked against two panels of
//! different shapes. The journal is the map and the book now.
//!
//! ## Why the rungs are a strip and not a column
//!
//! The journal's panel is tall and the pack's is wide and short -- seven
//! rows a finger tall do not fit in it, and squeezing them until they did
//! is exactly the mistake the controls screen was just rescued from. A
//! ladder read left to right along the top is also the truer picture:
//! this is a sequence, and a row of seven with one of them lit says
//! "fifth of seven" at a glance in a way a scrolling column never did.
//!
//! Everything here is hit-tested by the same functions that draw it,
//! which is the rule the whole interface is held to.

use primitive_shared::discovery::Discovered;
use primitive_shared::ladder::{self, Age, Found, Rung, LADDER};
use primitive_shared::types::{block_kind, BlockId};

use crate::engine::texture::FaceLayers;
use crate::ui::inventory_screen::{icon_layer, textured};
use crate::ui::keybinds::{Action, Keybinds};
use crate::ui::lang::{by_input, Language, Msg};
use crate::ui::widgets::{self, size, Painter, Rect};

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

/// What this language says for one of the first minutes' facts.
pub fn first_step_msg(step: ladder::FirstStep) -> Msg {
    use ladder::FirstStep;
    match step {
        FirstStep::Stone => Msg::StepStone,
        FirstStep::Fibre => Msg::StepFibre,
        FirstStep::Flake => Msg::StepFlake,
    }
}

/// ...and the kind of place a rung's materials come from.
pub fn found_text(found: Found) -> Msg {
    match found {
        Found::Underfoot => Msg::FoundUnderfoot,
        Found::Riverbank => Msg::FoundRiverbank,
        Found::Hills => Msg::FoundHills,
        Found::DeepRock => Msg::FoundDeepRock,
        Found::Woods => Msg::FoundWoods,
        Found::FarCountry => Msg::FoundFarCountry,
    }
}

// ---- what the page is drawn from ----

/// Everything the path page reads that is not already on the pack screen.
///
/// **One struct rather than two more arguments on `build_into`**, on the
/// argument `Vitals` makes: that function already takes nine, and the
/// tenth is the one somebody passes in the wrong order. These two travel
/// together because they are the same fact from two directions -- what
/// this player has done, and what their hands can do about it.
#[derive(Clone, Copy)]
pub struct Learning<'a> {
    /// Every kind this player has ever held. The whole of what the ladder
    /// is worked out from -- see `ladder`'s note on how a rung counts.
    pub discovered: &'a Discovered,
    /// What the keys are bound to, so the controls block names the key
    /// that is actually bound rather than the one that shipped.
    pub keys: &'a Keybinds,
}

impl Learning<'static> {
    /// A player who has held nothing, with the keys as they shipped.
    ///
    /// For the dozen tests and snapshots that build the pack screen and
    /// are not about this page: they all need a `Learning` and none of
    /// them cares what is in it, and threading two `let` bindings through
    /// each of them would be two lines of noise apiece. `'static`
    /// because the two values live in the test binary rather than on a
    /// caller's stack -- a function returning references to its own
    /// locals cannot exist.
    #[cfg(test)]
    pub fn nothing_yet() -> Self {
        static NOTHING: std::sync::OnceLock<Discovered> = std::sync::OnceLock::new();
        static KEYS: std::sync::OnceLock<Keybinds> = std::sync::OnceLock::new();
        Learning {
            discovered: NOTHING.get_or_init(Discovered::new),
            keys: KEYS.get_or_init(Keybinds::default),
        }
    }
}

// ---- layout ----

/// Air between the page's own blocks.
const GAP: f32 = 0.016;

/// The band the page is drawn in: the pack's content band, padded.
pub fn body() -> Rect {
    crate::ui::inventory_screen::content_band()
}

/// How tall the strip of seven rungs is.
///
/// Three things stacked and the air around them: the picture, the age's
/// name, and -- on one of the seven -- `вы здесь`. Measured from those
/// three rather than picked, because it was picked once (0.175) and the
/// third line hung out of the bottom of the strip and landed on the
/// heading of the pane below it. Floored at a finger, because tapping a
/// rung is the one thing this page is pressed for.
pub fn strip_height() -> f32 {
    let stacked = RUNG_PAD * 2.0
        + rung_icon_side()
        + 0.008
        + widgets::cell_height(size::CAPTION)
        + 0.004
        + widgets::cell_height(size::NOTE);
    widgets::tappable(stacked)
}

/// Air inside a rung cell, above the picture and below the last line.
const RUNG_PAD: f32 = 0.012;

/// How big a rung's picture is. A slot, so a rung reads as a *thing* --
/// the same square the ingot is drawn in everywhere else on this screen.
fn rung_icon_side() -> f32 {
    crate::ui::inventory_screen::CELL * 0.9
}

/// Where the rung at `index` in `ladder::LADDER` is drawn.
///
/// Seven equal parts of the band's width. Equal rather than sized to
/// their own words, because this is a scale: a `BRONZE` twice the width
/// of `IRON` would say the bronze age is twice as long, which is not a
/// thing this game has an opinion about.
pub fn rung_rect(index: usize) -> Rect {
    let body = body();
    let count = LADDER.len() as f32;
    let width = (body.width() - GAP * (count - 1.0)) / count;
    let left = body.x0 + index as f32 * (width + GAP);
    Rect::new(left, body.y1 - strip_height(), left + width, body.y1)
}

/// Which rung a point is over. The exact inverse of [`rung_rect`].
pub fn rung_at(at: (f32, f32)) -> Option<usize> {
    (0..LADDER.len()).find(|&index| rung_rect(index).contains(at.0, at.1))
}

/// The strip of seven, as one rectangle.
fn strip_rect() -> Rect {
    let body = body();
    Rect::new(body.x0, body.y1 - strip_height(), body.x1, body.y1)
}

/// The half of the lower band that reads the chosen rung out.
fn wants_pane() -> Rect {
    let body = body();
    let split = body.x0 + body.width() * 0.52;
    Rect::new(body.x0 + TRAY_PAD, body.y0 + TRAY_PAD, split - GAP, body.y1 - strip_height() - GAP - TRAY_PAD)
}

/// ...and the half that says what to do and what the hands can do.
fn now_pane() -> Rect {
    let body = body();
    let split = body.x0 + body.width() * 0.52;
    Rect::new(split + GAP, body.y0 + TRAY_PAD, body.x1 - TRAY_PAD, body.y1 - strip_height() - GAP - TRAY_PAD)
}

/// What the page is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LadderScreen {
    /// Into `ladder::LADDER`. `None` reads out the rung the player is
    /// working towards -- see the module note.
    selected: Option<usize>,
}

impl LadderScreen {
    /// Which rung is read out below the strip.
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
    pub fn click(&mut self, at: (f32, f32)) -> bool {
        let Some(index) = rung_at(at) else {
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

    /// Draws the page into the pack's content band.
    pub fn paint(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        learning: Learning,
        cursor: Option<(f32, f32)>,
        language: Language,
    ) {
        let held = learning.discovered;
        let hovered = |rect: Rect| cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let chosen = self.chosen(held);
        let standing = ladder::standing_on(held).map(|rung| rung.age);

        // The three groups stand in trays, as every group on this screen
        // does: the strip is one thing, and the two panes under it are
        // two more. Without them the page was a row of pictures floating
        // over bare stone with text beside it, which is the flat slab the
        // pack was rescued from (see `pack_tray`).
        for group in [strip_rect(), wants_pane(), now_pane()] {
            p.well(
                Rect::new(group.x0 - TRAY_PAD, group.y0 - TRAY_PAD, group.x1 + TRAY_PAD, group.y1 + TRAY_PAD),
                widgets::TRAY,
            );
        }
        for (index, rung) in LADDER.iter().enumerate() {
            let rect = rung_rect(index);
            let taken = rung.taken(held);
            if index == chosen {
                p.quad(rect, CHOSEN);
                p.border(rect, 0.003, widgets::ACCENT);
            } else if hovered(rect) {
                p.quad(rect, HOVER);
            }
            // The rung's own picture: the thing that *is* this age, greyed
            // while it has never been held. A picture rather than a number,
            // because "copper" means the ingot and the player has seen one.
            let side = rung_icon_side().min(rect.width() - RUNG_PAD * 2.0);
            let icon = Rect::new(
                rect.centre_x() - side / 2.0,
                rect.y1 - RUNG_PAD - side,
                rect.centre_x() + side / 2.0,
                rect.y1 - RUNG_PAD,
            );
            textured(p, icon, icon_layer(layers, rung.marks[0]), tint(rung.marks[0], taken));

            // **The rung is climbed or it is not**, and that is said by ink
            // rather than by a tick: a column of ticks and empty boxes is a
            // checklist, and a checklist is the chore this page is not.
            let name = language.text(age_name(rung.age));
            let ink = if taken { widgets::INK } else { widgets::INK_DIM };
            let name_top = icon.y0 - 0.008;
            // **Fitted down rather than elided.** `ГОЛЫЕ РУКИ` is the
            // one age name too wide for a seventh of the band, and an
            // age called `ГОЛЫЕ Р..` is a label that has stopped being a
            // name. A ceiling of `size::CAPTION` with a floor under it is
            // what every column on these screens does with a word that
            // will not fit -- see `widgets::fitted_scale`.
            let room = rect.width() - RUNG_PAD * 2.0;
            p.text_centred(
                name,
                rect.centre_x(),
                name_top,
                widgets::fitted_scale(name, size::CAPTION, room, 0.62),
                ink,
            );
            if standing == Some(rung.age) {
                // "you are here", under the name rather than beside it:
                // a strip cell is a column, and a tail hung off the right
                // of a name in a column is a tail in the next cell.
                let tail = language.text(Msg::LadderYouAreHere);
                p.text_centred(
                    &widgets::fit(tail, size::NOTE, rect.width() - RUNG_PAD * 2.0),
                    rect.centre_x(),
                    name_top - widgets::cell_height(size::CAPTION) - 0.004,
                    size::NOTE,
                    widgets::ACCENT,
                );
            }
        }

        let _ = wants_block(p, layers, held, &LADDER[chosen], language);
        now_block(p, learning, &LADDER[chosen], language);
    }
}

use crate::ui::inventory_screen::TRAY_PAD;

const CHOSEN: [f32; 4] = [0.22, 0.18, 0.11, 1.0];
const HOVER: [f32; 4] = [0.16, 0.15, 0.14, 1.0];
const GOOD: [f32; 4] = [0.35, 0.62, 0.30, 1.0];

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

/// What the chosen rung takes, and where those things come from. `false`
/// if the how-to line under it was cut off by the pane's foot.
fn wants_block(
    p: &mut Painter,
    layers: &FaceLayers,
    held: &Discovered,
    rung: &Rung,
    language: Language,
) -> bool {
    let pane = wants_pane();
    let mut top = pane.y1;
    let heading = |p: &mut Painter, top: &mut f32, msg: Msg| {
        p.text(language.text(msg), pane.x0, *top, size::CAPTION, widgets::INK_DIM);
        *top -= widgets::cell_height(size::CAPTION) + 0.012;
    };
    // One heading, whether the rung is taken or not. It used to say
    // `вы здесь` over a taken rung, which is not a heading -- it is the
    // strip's word for the rung the player is standing on, printed a
    // second time in the wrong place and in the wrong voice.
    heading(p, &mut top, Msg::LadderWants);

    // What it wants: everything, with the ones never held drawn as they are
    // in the book -- because "copper ore, charcoal, a pot and a mould" to
    // somebody holding three of the four has told them nothing.
    let still: Vec<BlockId> = rung.still_to_find(held).collect();
    let row = widgets::cell_height(size::BODY) + 0.024;
    for &want in rung.wants {
        if top - row < pane.y0 {
            break;
        }
        let have = still.iter().all(|&s| block_kind(s) != block_kind(want));
        let icon = Rect::new(pane.x0, top - row + 0.010, pane.x0 + row - 0.012, top);
        textured(p, icon, icon_layer(layers, want), tint(want, have));
        let ink = if have { GOOD } else { widgets::INK };
        p.text(
            &widgets::fit(
                &crate::ui::names::block(want, language),
                size::BODY,
                pane.width() - row - 0.03,
            ),
            icon.x1 + 0.020,
            top - 0.012,
            size::BODY,
            ink,
        );
        top -= row;
    }

    // How the things are put together, for the one rung whose making is
    // not a recipe in the book. See `how_to_lay`.
    if let Some(msg) = how_to_lay(rung.age) {
        top -= 0.010;
        return wrapped(p, pane.x0, &mut top, pane, language.text(msg), widgets::INK_DIM);
    }
    true
}

/// The line under a rung's list that says how its things become the rung.
///
/// **Only the fire has one**, because only the fire is not made in the
/// book: the firepit's sticks and log are dropped on the ground and struck
/// with flint (`strike_firepit` on the server), and a player shown three
/// icons with no recipe behind them was left to guess. Every other rung's
/// things go into a recipe the book already explains.
pub fn how_to_lay(age: Age) -> Option<Msg> {
    (age == Age::Fire).then_some(Msg::LadderLayFirepit)
}

/// What to do now, the two buttons nothing names, and what has turned up.
fn now_block(p: &mut Painter, learning: Learning, rung: &Rung, language: Language) {
    let pane = now_pane();
    let held = learning.discovered;
    let mut top = pane.y1;
    let heading = |p: &mut Painter, top: &mut f32, msg: Msg| {
        p.text(language.text(msg), pane.x0, *top, size::CAPTION, widgets::INK_DIM);
        *top -= widgets::cell_height(size::CAPTION) + 0.012;
    };

    // ---- what now ----
    //
    // **The first minutes and the next age are the same question**, asked
    // of a player who has held nothing and of one who has held everything,
    // and this is the one line that answers it. For an empty-handed player
    // it is the fact about the world that the belt used to nag about (see
    // `ladder::first_step`); after that it is the rung above the one they
    // are standing on, named, with the kind of place its materials are in.
    heading(p, &mut top, Msg::LearnNow);
    // **The first minutes' facts stop the moment the flint age starts.**
    // Read straight off `ladder::first_step` this said "tall grass comes
    // apart into fibre" to a player holding a copper ingot, because that
    // player had happened never to hold raw fibre -- a true sentence,
    // uselessly placed, and exactly the "тупые подсказки" the whole pass
    // is about. Below the flint rung the three facts are the answer; at
    // or above it, the answer is the next age and the kind of place its
    // materials are in.
    let starting = matches!(ladder::standing_on(held).map(|r| r.age), None | Some(Age::BareHands));
    let now: String = match ladder::first_step(held).filter(|_| starting) {
        Some(step) => language.text(first_step_msg(step)).to_string(),
        _ if ladder::working_towards(held).is_some() => format!(
            "{} -- {}",
            language.text(age_name(rung.age)),
            language.text(found_text(rung.found)),
        ),
        // Standing on the top rung: there is no next thing, and saying so
        // is better than repeating the last one.
        _ => language.text(Msg::LadderDone).to_string(),
    };
    let _ = wrapped(p, pane.x0, &mut top, pane, &now, widgets::INK);
    top -= 0.014;

    // ---- the controls that are not guessable ----
    //
    // **Four, and not the eighteen the bindings screen lists.** A page
    // that reprinted every key would be the bindings screen with worse
    // typography; what belongs here is the handful a player cannot work
    // out by pressing things. The two mouse buttons are the whole of how
    // the world is touched and nothing else in the game names them; eat
    // and drop are the two that are bound to letters nobody guesses.
    // Walking, looking and the hotbar teach themselves in five seconds.
    heading(p, &mut top, Msg::LearnControls);
    let touch = widgets::touch_layout();
    let row = widgets::cell_height(size::NOTE) + 0.010;
    // **The count is pinned to the floor, and the control rows stop
    // above it.** Written in flow order it was the last thing on the
    // page and it fell off the bottom by four thousandths -- a line that
    // is there on a desktop and gone on a phone, which is the worst kind
    // of missing. What is at the foot of a pane belongs at the foot of
    // the pane.
    let floor = pane.y0 + widgets::cell_height(size::BODY) + 0.012;
    let say_row = |p: &mut Painter, top: &mut f32, text: &str| {
        if *top - row < floor {
            return;
        }
        p.text(
            &widgets::fit(text, size::NOTE, pane.width()),
            pane.x0,
            *top,
            size::NOTE,
            widgets::INK_DIM,
        );
        *top -= row;
    };
    say_row(p, &mut top, language.text(by_input(Msg::LearnDig, Msg::LearnDigTouch)));
    say_row(p, &mut top, language.text(by_input(Msg::LearnPlace, Msg::LearnPlaceTouch)));
    // A key and what it does, and only where there is a key: a phone has
    // no keyboard, and "E -- eat" on a phone is a line about a device the
    // player is not holding.
    if !touch {
        for action in [Action::Eat, Action::Drop] {
            let Some(key) = learning.keys.key(action).map(|_| learning.keys.label(action)) else {
                continue;
            };
            // Lower-cased, because `Action::label` is written for the
            // bindings screen, where every row is a shouted `EAT`. Two
            // lines of small caps under two lines of ordinary words read
            // as two different lists.
            let text = format!("{key} -- {}", action.label(language).to_lowercase());
            say_row(p, &mut top, &text);
        }
    }

    // ---- how much of the world has turned up ----
    //
    // A count and not a list: the recipe book is the list, and what this
    // adds is the one number that says a world this big is still mostly
    // unopened. Out of every kind there is, for the reason the diet row
    // on the health page is out of four -- a bare number has no scale.
    // On one line with its own heading, because it is one number: a
    // caption on a row of its own over a single `6 / 497` is two rows
    // spent saying one thing, and this pane is the shortest on the page.
    let word = language.text(Msg::LearnFound);
    let count = format!("{} / {}", held.len(), primitive_shared::types::ALL_BLOCK_IDS.len());
    let baseline = pane.y0 + widgets::cell_height(size::BODY);
    p.text(word, pane.x0, baseline, size::CAPTION, widgets::INK_DIM);
    // On the caption's baseline, not on one of its own -- see
    // `widgets::cap_height`, which is what two sizes on one row have to
    // agree about.
    p.text(
        &count,
        pane.x0 + widgets::measure(word, size::CAPTION) + widgets::measure("m", size::CAPTION),
        baseline - (widgets::cap_height(size::CAPTION) - widgets::cap_height(size::BODY)),
        size::BODY,
        widgets::ACCENT,
    );
}

/// One sentence, wrapped to a pane and stopping at its floor.
///
/// Wrapped rather than fitted down: a sentence at half the size of the
/// line above it is not a smaller line, it is a different voice -- which
/// is the whole of what the player was complaining about.
/// Whether every line fitted: `false` is a sentence cut off at the pane's
/// foot, which the tests look for.
fn wrapped(p: &mut Painter, left: f32, top: &mut f32, pane: Rect, text: &str, ink: [f32; 4]) -> bool {
    let columns = ((pane.x1 - left) / widgets::measure("m", size::BODY)).max(8.0) as usize;
    let step = widgets::cell_height(size::BODY) + 0.008;
    for line in widgets::wrap(text, columns) {
        if *top - widgets::cell_height(size::BODY) < pane.y0 {
            return false;
        }
        p.text(&line, left, *top, size::BODY, ink);
        *top -= step;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_COPPER_INGOT, BLOCK_IRON_INGOT, BLOCK_PEBBLE};

    fn holding(kinds: &[BlockId]) -> Discovered {
        Discovered::from_kinds(kinds.iter().copied())
    }

    #[test]
    fn how_to_lay_a_firepit_is_read_to_the_end_in_every_language_on_both_layouts() {
        let fire = LADDER.iter().find(|rung| rung.age == Age::Fire).unwrap();
        assert!(how_to_lay(fire.age).is_some(), "the fire rung says nothing about laying a firepit");
        let layers = FaceLayers::empty_for_test();
        for touch in [false, true] {
            widgets::with_touch(touch, || {
                for &language in Language::ALL {
                    let mut p = Painter::onto(crate::engine::texture::FontAtlas::for_test(), Vec::new());
                    assert!(
                        wants_block(&mut p, &layers, &Discovered::new(), fire, language),
                        "the firepit's how-to is cut off in {language:?} (touch: {touch})"
                    );
                }
            });
        }
    }

    #[test]
    fn every_rung_is_pressed_where_it_is_drawn_and_none_of_them_fall_off_the_page() {
        for scale in [1.0, 1.5] {
            widgets::with_touch(scale > 1.0, || {
                let band = body();
                for index in 0..LADDER.len() {
                    let rect = rung_rect(index);
                    assert_eq!(rung_at((rect.centre_x(), rect.centre_y())), Some(index));
                    assert!(rect.x0 >= band.x0 - 1e-4, "rung {index} fell off the left");
                    assert!(rect.x1 <= band.x1 + 1e-4, "rung {index} fell off the right");
                    assert!(rect.y0 >= band.y0 - 1e-4, "rung {index} fell through the floor");
                }
            });
        }
    }

    /// The strip and the two panes under it have to share a band that is
    /// a third of the height the journal's page had. If the strip eats it
    /// all there is nowhere for the rung to be read out, and the page is a
    /// row of pictures.
    #[test]
    fn the_two_panes_under_the_rungs_are_still_worth_writing_in() {
        widgets::as_a_phone(|| {
            for pane in [wants_pane(), now_pane()] {
                assert!(
                    pane.height() > widgets::cell_height(size::BODY) * 3.0,
                    "a pane with no room to say anything: {pane:?}"
                );
                assert!(pane.width() > 0.5, "a pane too narrow to write in: {pane:?}");
            }
        });
    }

    #[test]
    fn a_phone_gets_rungs_a_finger_wide_and_a_finger_tall() {
        widgets::as_a_phone(|| {
            let rect = rung_rect(0);
            assert!(rect.height() >= widgets::FINGER_SIDE - 1e-5, "a rung shorter than a finger");
            assert!(rect.width() >= widgets::FINGER - 1e-5, "a rung narrower than a finger");
        });
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
        let mut page = LadderScreen::default();
        let held = holding(&[BLOCK_IRON_INGOT]);
        let rect = rung_rect(0);
        assert!(page.click((rect.centre_x(), rect.centre_y())));
        assert_eq!(LADDER[page.chosen(&held)].age, Age::BareHands);
        assert!(!page.click((rect.centre_x(), rect.centre_y())), "the same rung pressed twice was news");
    }

    /// Drawn for a beginner and for a smith, on a desktop and on a phone,
    /// with nothing panicking and something on the screen.
    #[test]
    fn the_page_draws_for_a_beginner_and_for_a_smith_on_a_desktop_and_a_phone() {
        for phone in [false, true] {
            widgets::with_touch(phone, || {
                for held in [Discovered::new(), holding(&[BLOCK_IRON_INGOT])] {
                    let keys = Keybinds::default();
                    let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
                    LadderScreen::default().paint(
                        &mut p,
                        &FaceLayers::empty_for_test(),
                        Learning { discovered: &held, keys: &keys },
                        None,
                        Language::Russian,
                    );
                    assert!(!p.into_vertices().is_empty(), "the path page drew nothing");
                }
            });
        }
    }
}
