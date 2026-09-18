//! The journal: the map, the recipe book and the give menu, one screen
//! with three tabs.
//!
//! ## Why one screen with tabs
//!
//! Because the first two are read together. The book says bronze wants a
//! second metal; the map says where the hills are. A player who has to
//! close one screen and remember a second key to open the other reads one
//! of them. It is also one button on a phone rather than two, and the
//! thumb controls have no room to give (see `settings::TouchLayout`).
//!
//! ## ...and why the give menu is the third one
//!
//! It is not read with the other two -- it is an operator's tool, not a
//! player's knowledge -- and it is here anyway, for one reason: **a phone
//! has no room for another way in.** A screen of its own would need a
//! ninth thumb control, and the wheel is the shape it is precisely
//! because a fourth member breaks it (see the `Spot` note in
//! `settings::TouchLayout::default`, and the F3 button it pushed under
//! the mining thumb). A tab costs one word of the header row that is
//! already drawn, works with the finger that already opens the journal,
//! and inherits the way out, the drag-to-scroll and the close-by-tapping
//! -beside that every page here has. A key of its own (`Action::Give`)
//! opens it straight, so a desktop never pays the extra tap.
//!
//! The honest cost: a player who is not an operator can open a page that
//! will refuse them. That is why the refusal is a sentence on the page
//! rather than silence -- see `give_screen::Status::Denied`.
//!
//! ## What this owns, and why here
//!
//! Everything the two screens draw from that is not already the frame
//! loop's: the explored map, what the player has held, and the landmarks.
//! One owner means `lib.rs` hands one thing to the network drain, one to
//! chunk integration and one to the drawing, rather than five -- and it
//! means a session ending clears all of it in one place, so a map from one
//! world can never be drawn over another.
//!
//! ## A finger is not a mouse here
//!
//! Every other screen in the game is a set of buttons, so a finger on it
//! is turned into a tap or a scroll by `touch::Pointer` and nothing else
//! is needed. The map is a surface that is dragged in two directions, and
//! `Pointer` reports vertical scrolls only. So while the journal is open
//! the touch arm in `lib.rs` hands it the raw finger instead (see
//! [`Journal::touch`]): a finger that travels drags, a finger that does
//! not is a press, on the same slop rule `Pointer` uses.
//!
//! **And a second finger on the map is a pinch.** It used to be thrown
//! away -- `touch` took the first finger and ignored every other one --
//! which is why a pinch could be given the meaning it has on every other
//! map on the phone without taking it from anything. It is the map's
//! alone: on the recipe page a second finger is still ignored, because a
//! list has nothing to scale. See `map_screen::Pinch`.

use std::path::PathBuf;

use primitive_shared::discovery::Discovered;
use primitive_shared::inventory::Inventory;

use crate::engine::texture::{FaceLayers, FontAtlas};
use crate::logic::map::{ExploredMap, Landmarks};
use crate::platform::{TouchId, TouchPhase};
use crate::ui::hotbar::HotbarVertex;
use crate::ui::give_screen::{self, GiveScreen};
use crate::ui::lang::{by_input, Language, Msg};
use crate::ui::map_screen::{self, MapView, Pinch, PlayerMark};
use crate::ui::recipe_book::{self, RecipeBook};
use crate::ui::widgets::{self, Painter, Rect};

/// Which page of the journal is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Map,
    Recipes,
    /// The give menu. See [`crate::ui::give_screen`], and below for why
    /// it is a page of this screen rather than one of its own.
    Give,
}

/// A button along the top of the journal.
///
/// **Tabs, and no longer a way out.** There was an `X` at the left of this
/// row, and it was the last one in the game: the pack and every container
/// had lost theirs and close by Escape, by their own key, or by a tap off
/// the panel. A journal that alone kept a cross taught a phone two ways out
/// of two screens opened from the same wheel. It closes the pack's way now;
/// see [`panel_rect`] for the glass that makes that reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Header {
    Tab(Tab),
}

const HEADERS: [Header; 3] = [
    Header::Tab(Tab::Map),
    Header::Tab(Tab::Recipes),
    Header::Tab(Tab::Give),
];

/// What a gesture on the journal did, for the frame loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Changed,
    /// The journal shut itself: the cursor goes back to the world.
    Closed,
}

/// Air between the journal and the edge of the window where a mouse
/// points. See [`panel_rect`] for where a finger does.
const MARGIN: f32 = 0.04;
/// The strip along the bottom for the help line and the legend.
const FOOTER: f32 = 0.075;
/// How far a finger may wander and still be a press, in interface units.
/// The same share of a finger as `touch::TAP_SLOP` is of the short side.
const TAP_SLOP: f32 = 0.04;

/// The whole journal.
///
/// **A finger narrower on each side where a finger closes it.** With no
/// `X`, the way out is the glass off the page, as it is for the pack
/// (`inventory_screen::Intent::Close`) -- and four hundredths of a screen
/// is a strip a mouse can hit and a thumb cannot. The sides rather than the
/// top or bottom, because width is what a phone held sideways has spare:
/// the page's height is the map's, and the tabs and the footer live in it.
pub fn panel_rect(aspect: f32) -> Rect {
    let edge = (aspect - widgets::tappable(MARGIN)).max(0.5);
    Rect::new(-edge, -0.97, edge, 0.97)
}

fn header_height() -> f32 {
    widgets::tappable(0.10)
}

/// Where a header button is.
///
/// Laid left to right, each as wide as its own word needs -- so a third
/// tab costs the width of one word and nothing else moves. `GIVE` is
/// last because it is the one page of the three that is not part of
/// playing: the map and the book are read while walking, and this is
/// reached on purpose.
pub fn header_rect(header: Header, aspect: f32) -> Rect {
    let panel = panel_rect(aspect);
    let h = header_height();
    let top = panel.y1 - 0.015;
    let map = Rect::new(panel.x0 + 0.015, top - h, panel.x0 + 0.015 + 0.36, top);
    let recipes = Rect::new(map.x1 + 0.015, top - h, map.x1 + 0.015 + 0.46, top);
    match header {
        Header::Tab(Tab::Map) => map,
        Header::Tab(Tab::Recipes) => recipes,
        Header::Tab(Tab::Give) => Rect::new(recipes.x1 + 0.015, top - h, recipes.x1 + 0.015 + 0.42, top),
    }
}

/// Which header button a point is over.
pub fn header_at(at: (f32, f32), aspect: f32) -> Option<Header> {
    HEADERS.into_iter().find(|h| header_rect(*h, aspect).contains(at.0, at.1))
}

/// Where a tab draws its contents.
pub fn body_rect(aspect: f32) -> Rect {
    let panel = panel_rect(aspect);
    let header_bottom = header_rect(Header::Tab(Tab::Map), aspect).y0;
    Rect::new(panel.x0 + 0.02, panel.y0 + FOOTER, panel.x1 - 0.02, header_bottom - 0.02)
}

/// The second finger on the map, while there is one.
///
/// Kept beside the drag rather than inside it because it is not a drag:
/// it never scrolls the book, never becomes a press, and never closes
/// the journal. All it carries is where it is, which is half of what
/// [`Pinch`] is made of.
#[derive(Debug, Clone, Copy)]
struct Second {
    finger: TouchId,
    at: (f32, f32),
}

/// A pointer held down on the journal.
#[derive(Debug, Clone, Copy)]
struct Drag {
    /// `None` for the mouse.
    finger: Option<TouchId>,
    from: (f32, f32),
    last: (f32, f32),
    travelled: bool,
    /// Part of a row of scroll not yet handed out.
    carry: f32,
}

/// The map, the book, and what they are drawn from.
#[derive(Default)]
pub struct Journal {
    pub explored: ExploredMap,
    pub discovered: Discovered,
    pub landmarks: Landmarks,
    open: Option<Tab>,
    map: MapView,
    book: RecipeBook,
    give: GiveScreen,
    /// Whether the server said this player is an operator, which is the
    /// whole of whether the give page exists.
    ///
    /// **False until the server says otherwise**, and it is asked every
    /// time the journal opens (`protocol::ClientMessage::AmIAnOperator`).
    /// A page that appeared for everyone and refused everyone read as a
    /// broken game; a page that appears a round trip late reads as a page
    /// that was always there. Of the two, only one is ever wrong in the
    /// direction of offering something that cannot work.
    operator: bool,
    cursor: Option<(f32, f32)>,
    drag: Option<Drag>,
    second: Option<Second>,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// A world was entered. `cache` is where its map is kept, if it has a
    /// place to be kept.
    pub fn begin_session(&mut self, cache: Option<PathBuf>) {
        self.end_session();
        self.explored = cache.map(ExploredMap::open).unwrap_or_default();
        if self.explored.surveyed() > 0 {
            println!("[map] {} chunks already seen here", self.explored.surveyed());
        }
    }

    /// A world was left: the map is written and everything about that
    /// world is forgotten, so none of it can be drawn over the next one.
    pub fn end_session(&mut self) {
        self.save_map();
        *self = Journal {
            map: MapView::default(),
            ..Journal::default()
        };
    }

    /// Writes the map if it has changed. Said in the log either way it
    /// goes, because a map that silently failed to save is a map the
    /// player finds blank next evening with no idea why.
    pub fn save_map(&mut self) {
        match self.explored.save() {
            Ok(true) => println!("[map] saved {} chunks", self.explored.surveyed()),
            Ok(false) => {}
            Err(e) => eprintln!("[map] could not save the map: {e}"),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens a tab, or shuts the journal if that tab is already showing --
    /// so the key that opens the map is also the key that puts it away.
    /// Answers whether it is open afterwards.
    /// The server answered `AmIAnOperator`. See [`Journal::operator`].
    ///
    /// A player demoted while the page is open is put back on the map
    /// rather than left looking at a menu that will now refuse them: the
    /// server is the authority on this, and the screen follows it in both
    /// directions.
    pub fn set_operator(&mut self, yes: bool) {
        self.operator = yes;
        if !yes && self.open == Some(Tab::Give) {
            self.open = Some(Tab::Map);
            self.forget_the_fingers();
        }
    }

    /// Is this a page this player has?
    fn reachable(&self, tab: Tab) -> bool {
        tab != Tab::Give || self.operator
    }

    pub fn toggle(&mut self, tab: Tab) -> bool {
        // The give key does nothing at all for a player who is not an
        // operator -- not "opens and refuses", which is what the page used
        // to do and what a player reads as a broken menu.
        if !self.reachable(tab) {
            return self.open.is_some();
        }
        if self.open == Some(tab) {
            self.close();
            false
        } else {
            self.open = Some(tab);
            self.forget_the_fingers();
            true
        }
    }

    pub fn close(&mut self) {
        self.open = None;
        self.forget_the_fingers();
        self.cursor = None;
    }

    /// Both fingers forgotten.
    ///
    /// **One place, because half a pinch is worse than none.** A page
    /// that cleared the drag and left the second finger behind would
    /// pinch against the point the player's other thumb was last seen
    /// at, and the map would leap the moment they touched it again.
    fn forget_the_fingers(&mut self) {
        self.drag = None;
        self.second = None;
    }

    /// Tab: the next page round.
    ///
    /// Round rather than between two, now that there are three: a key
    /// that turned the page and sometimes did not reach the third one
    /// would be a page a keyboard could not get to.
    pub fn switch_tab(&mut self) {
        self.open = match self.open {
            Some(Tab::Map) => Some(Tab::Recipes),
            // ...and past the give page for anybody who has not got one,
            // or Tab would land on a blank page and appear to be stuck.
            Some(Tab::Recipes) if self.operator => Some(Tab::Give),
            Some(Tab::Recipes) => Some(Tab::Map),
            Some(Tab::Give) => Some(Tab::Map),
            None => None,
        };
        self.forget_the_fingers();
    }

    /// Whether letters typed now are a search rather than shortcuts.
    pub fn takes_text(&self) -> bool {
        matches!(self.open, Some(Tab::Recipes) | Some(Tab::Give))
    }

    pub fn type_char(&mut self, c: char) {
        if !crate::engine::texture::has_glyph(c) {
            return;
        }
        match self.open {
            Some(Tab::Recipes) => self.book.type_char(c),
            Some(Tab::Give) => self.give.type_char(c),
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.open {
            Some(Tab::Recipes) => self.book.backspace(),
            Some(Tab::Give) => self.give.backspace(),
            _ => {}
        }
    }

    /// The chat line the give menu wants sent, if it has one.
    ///
    /// **The menu never sends anything itself.** It writes down the
    /// command an operator would have typed and the frame loop puts it
    /// on the wire, so the client has one place that talks to a server
    /// rather than two. See `give_screen`'s module note.
    pub fn take_command(&mut self) -> Option<String> {
        self.give.take_command()
    }

    /// Offers a line the server said in its own name to the give menu.
    ///
    /// Answers whether it was the menu's, in which case the caller must
    /// not also put it in the chat log -- see
    /// [`GiveScreen::take_reply`], and the "Отказы больше не пишутся в
    /// чат" note in the changelog for why a refusal must not go there.
    pub fn take_server_note(&mut self, text: &str) -> bool {
        self.give.take_reply(text)
    }

    /// The mouse moved. A held button drags the map.
    ///
    /// Takes the window's shape for the same signature as every other
    /// pointer call on the journal, though a drag needs only the distance.
    pub fn set_cursor(&mut self, at: Option<(f32, f32)>, _aspect: f32, player: PlayerMark) {
        self.cursor = at;
        let (Some(at), Some(drag)) = (at, self.drag.as_mut()) else {
            return;
        };
        if drag.finger.is_some() {
            return;
        }
        let (dx, dy) = (at.0 - drag.last.0, at.1 - drag.last.1);
        drag.last = at;
        if self.open == Some(Tab::Map) {
            self.map.drag_by(dx, dy, player);
        }
    }

    /// The left button went down wherever the cursor is.
    pub fn press(&mut self, aspect: f32, player: PlayerMark) -> Outcome {
        let Some(at) = self.cursor else {
            return Outcome::Nothing;
        };
        // **A mouse click beside the page does not shut the journal.** A
        // finger has no other way out (there is no cross, and no Esc key on
        // glass), so a tap there still closes it -- see `touch`. A mouse has
        // Esc and the key that opened the page, and a click that closed the
        // journal also handed the mouse back to the camera: "когда начинаешь
        // жать лкм или пкм мышка захватывается", with the map under the
        // cursor refusing to move. Whatever put that press outside the page
        // on the player's machine, a press that cannot close anything cannot
        // take the mouse away.
        if !panel_rect(aspect).contains(at.0, at.1) {
            return Outcome::Nothing;
        }
        let outcome = self.click_at(at, aspect, player);
        if outcome == Outcome::Nothing
            && self.open == Some(Tab::Map)
            && body_rect(aspect).contains(at.0, at.1)
        {
            self.drag = Some(Drag { finger: None, from: at, last: at, travelled: false, carry: 0.0 });
        }
        outcome
    }

    /// The left button came up.
    pub fn release(&mut self) {
        if self.drag.is_some_and(|d| d.finger.is_none()) {
            self.drag = None;
        }
    }

    /// The wheel: zoom on the map, scroll in the book.
    pub fn wheel(&mut self, lines: f32, aspect: f32, player: PlayerMark) {
        let body = body_rect(aspect);
        match self.open {
            Some(Tab::Map) => {
                let about = self.cursor.filter(|(x, y)| body.contains(*x, *y));
                self.map.wheel(lines, about, body, player);
            }
            Some(Tab::Recipes) => {
                // Negative is towards the end of the list -- see the
                // handler in `lib.rs`.
                let rows = if lines < 0.0 { 1 } else { -1 };
                self.book.scroll_by(rows, &self.discovered, body);
            }
            Some(Tab::Give) => {
                let rows = if lines < 0.0 { 1 } else { -1 };
                self.give.scroll_by(rows, body);
            }
            None => {}
        }
    }

    /// One touch event, in interface units.
    pub fn touch(&mut self, id: TouchId, phase: TouchPhase, at: (f32, f32), aspect: f32, player: PlayerMark) -> Outcome {
        match phase {
            TouchPhase::Started => {
                if self.drag.is_none() {
                    self.drag = Some(Drag { finger: Some(id), from: at, last: at, travelled: false, carry: 0.0 });
                    self.cursor = Some(at);
                    return Outcome::Changed;
                }
                // **A second finger on the map pinches; anywhere else it
                // is still ignored.** Only over the map, because that is
                // the one thing on this screen with a scale; and only
                // against a finger, never against a held mouse button,
                // which has no second one to be.
                let held_by_a_finger = self
                    .drag
                    .is_some_and(|d| d.finger.is_some() && d.finger != Some(id));
                if self.open == Some(Tab::Map) && self.second.is_none() && held_by_a_finger {
                    self.second = Some(Second { finger: id, at });
                    // **The first finger has stopped being a press.** A
                    // pinch that began on the `+` button would otherwise
                    // end as a tap on it and zoom a step further than
                    // the player asked, which is the same surprise
                    // `travelled` exists to prevent for a drag.
                    if let Some(drag) = self.drag.as_mut() {
                        drag.travelled = true;
                    }
                    // ...and nothing stays lit under either of them.
                    self.cursor = None;
                }
                Outcome::Changed
            }
            TouchPhase::Moved if self.second.is_some() => self.pinched(id, at, aspect, player),
            TouchPhase::Moved => {
                let Some(drag) = self.drag.as_mut().filter(|d| d.finger == Some(id)) else {
                    return Outcome::Nothing;
                };
                if !drag.travelled && (at.0 - drag.from.0).hypot(at.1 - drag.from.1) > TAP_SLOP {
                    drag.travelled = true;
                    // A finger that has started dragging is not pointing
                    // at a button any more, so nothing stays lit under it.
                    self.cursor = None;
                }
                if !drag.travelled {
                    return Outcome::Nothing;
                }
                let (dx, dy) = (at.0 - drag.last.0, at.1 - drag.last.1);
                drag.last = at;
                match self.open {
                    Some(Tab::Map) => self.map.drag_by(dx, dy, player),
                    Some(Tab::Recipes) => {
                        // The list follows the finger: dragging up shows
                        // what is below.
                        drag.carry += dy;
                        let row = recipe_book::row_height();
                        let rows = (drag.carry / row).trunc();
                        if rows != 0.0 {
                            drag.carry -= rows * row;
                            let body = body_rect(aspect);
                            self.book.scroll_by(rows as i32, &self.discovered, body);
                        }
                    }
                    // The same gesture, against the grid's own row
                    // height: a page dragged by a finger has to move by
                    // what is under the finger, not by what the page
                    // beside it uses.
                    Some(Tab::Give) => {
                        drag.carry += dy;
                        let row = give_screen::cell_height();
                        let rows = (drag.carry / row).trunc();
                        if rows != 0.0 {
                            drag.carry -= rows * row;
                            self.give.scroll_by(rows as i32, body_rect(aspect));
                        }
                    }
                    None => {}
                }
                Outcome::Changed
            }
            TouchPhase::Ended | TouchPhase::Cancelled if self.lifted_a_pinching_finger(id) => {
                Outcome::Nothing
            }
            TouchPhase::Ended => {
                let Some(drag) = self.drag.filter(|d| d.finger == Some(id)) else {
                    return Outcome::Nothing;
                };
                self.drag = None;
                self.cursor = None;
                if drag.travelled {
                    Outcome::Changed
                } else {
                    self.click_at(at, aspect, player)
                }
            }
            TouchPhase::Cancelled => {
                if self.drag.is_some_and(|d| d.finger == Some(id)) {
                    self.drag = None;
                    self.cursor = None;
                }
                Outcome::Nothing
            }
        }
    }

    /// Both fingers are down and one of them moved.
    ///
    /// The whole gesture is the *change* between one pair of points and
    /// the next, so only the finger that moved is written down and the
    /// pair is read twice around it. A third finger is not part of
    /// anything and is answered with nothing at all -- a palm on the
    /// glass is not a zoom.
    fn pinched(&mut self, id: TouchId, at: (f32, f32), aspect: f32, player: PlayerMark) -> Outcome {
        let (Some(mut drag), Some(mut second)) = (self.drag, self.second) else {
            return Outcome::Nothing;
        };
        let before = Pinch::between(drag.last, second.at);
        if drag.finger == Some(id) {
            drag.last = at;
        } else if second.finger == id {
            second.at = at;
        } else {
            return Outcome::Nothing;
        }
        self.drag = Some(drag);
        self.second = Some(second);
        let after = Pinch::between(drag.last, second.at);
        self.map.pinch(before, after, body_rect(aspect), player);
        Outcome::Changed
    }

    /// One of the two fingers of a pinch has gone.
    ///
    /// Answers whether this lift was that, because the arm above it in
    /// [`Journal::touch`] must not then go on to treat it as a press.
    ///
    /// **The finger that stays keeps the map.** If the one that lifted
    /// was the drag, the other is promoted to it -- from where it
    /// actually is, and already counted as travelled, so the map neither
    /// jumps by the distance between the two thumbs nor reads the second
    /// thumb's lift as a tap on whatever is under it. Letting the drag
    /// die with its finger instead meant a player who lifts one thumb
    /// after a pinch is left holding a map that has stopped answering.
    fn lifted_a_pinching_finger(&mut self, id: TouchId) -> bool {
        let Some(second) = self.second else {
            return false;
        };
        if second.finger == id {
            self.second = None;
            return true;
        }
        if self.drag.is_some_and(|d| d.finger == Some(id)) {
            self.drag = Some(Drag {
                finger: Some(second.finger),
                from: second.at,
                last: second.at,
                travelled: true,
                carry: 0.0,
            });
            self.second = None;
            return true;
        }
        false
    }

    /// A press at a point: a header button, a map button, a filter, a row.
    fn click_at(&mut self, at: (f32, f32), aspect: f32, player: PlayerMark) -> Outcome {
        // Off the page is the way out, exactly as off the pack is. Only a
        // press that stayed a press gets here -- `touch` gives a finger that
        // travelled to the map -- so a drag ending past the edge is a drag.
        if !panel_rect(aspect).contains(at.0, at.1) {
            self.close();
            return Outcome::Closed;
        }
        if let Some(Header::Tab(tab)) = header_at(at, aspect) {
            // A tab that is not drawn is not pressed either: the
            // hit-testing here has to be the exact inverse of the drawing
            // below, which is the rule the whole interface is held to.
            if !self.reachable(tab) {
                return Outcome::Nothing;
            }
            self.open = Some(tab);
            return Outcome::Changed;
        }
        let body = body_rect(aspect);
        let changed = match (self.open, map_screen::control_at(at, body)) {
            (Some(Tab::Map), Some(control)) => {
                self.map.press(control, body, player);
                true
            }
            (Some(Tab::Recipes), _) => self.book.click(at, &self.discovered, body),
            (Some(Tab::Give), _) => self.give.click(at, body),
            _ => false,
        };
        if changed {
            Outcome::Changed
        } else {
            Outcome::Nothing
        }
    }

    /// A fingerprint of what `build_into` would draw.
    ///
    /// The player's position counts only while the map is showing, and
    /// only to a quarter of a block and five degrees: the arrow moving by
    /// less than that is not a picture worth rebuilding the interface for.
    pub fn ui_key(&self, player: PlayerMark, aspect: f32) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.open.hash(&mut h);
        // ...and whether the give tab is in the row, or the header would
        // not be redrawn the moment the server answers.
        self.operator.hash(&mut h);
        let Some(tab) = self.open else {
            return h.finish();
        };
        let body = body_rect(aspect);
        self.cursor
            .map(|at| (header_at(at, aspect), map_screen::control_at(at, body), recipe_book::row_at(at, body), recipe_book::filter_at(at, body)))
            .hash(&mut h);
        // The give page's own three things under the pointer, which
        // nothing above answers: a cell, a section chip, an amount chip.
        // Left out, the menu would not relight under a moving mouse.
        if tab == Tab::Give {
            self.cursor
                .map(|at| (give_screen::cell_at(at, body), give_screen::section_at(at, body), give_screen::amount_at(at, body)))
                .hash(&mut h);
        }
        match tab {
            Tab::Map => {
                self.map.key().hash(&mut h);
                self.explored.revision().hash(&mut h);
                self.landmarks.bags.hash(&mut h);
                self.landmarks.spawn.hash(&mut h);
                ((player.x * 4.0) as i32, (player.z * 4.0) as i32, (player.yaw.to_degrees() / 5.0) as i32).hash(&mut h);
            }
            Tab::Recipes => {
                self.book.key().hash(&mut h);
                self.discovered.kinds().hash(&mut h);
            }
            Tab::Give => self.give.key().hash(&mut h),
        }
        h.finish()
    }

    /// Draws the journal, if it is open.
    #[allow(clippy::too_many_arguments)] // the font, the pictures, the pack, the player, the window, the language, the list
    pub fn build_into(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        inventory: &Inventory,
        player: PlayerMark,
        aspect: f32,
        language: Language,
        out: &mut Vec<HotbarVertex>,
    ) {
        let Some(tab) = self.open else {
            return;
        };
        let mut p = Painter::onto_themed(font, std::mem::take(out), widgets::Theme::DARK);
        p.scrim(SCRIM);
        let panel = panel_rect(aspect);
        p.panel(panel);

        for header in HEADERS {
            let Header::Tab(this) = header;
            if !self.reachable(this) {
                continue;
            }
            let rect = header_rect(header, aspect);
            let hovered = self.cursor.is_some_and(|(x, y)| rect.contains(x, y));
            let label = language.text(match this {
                Tab::Map => Msg::MapTab,
                Tab::Recipes => Msg::RecipesTab,
                Tab::Give => Msg::GiveTab,
            });
            p.button(rect, label, hovered || this == tab, true);
            if this == tab {
                p.quad(Rect::new(rect.x0 + 0.01, rect.y0 - 0.012, rect.x1 - 0.01, rect.y0 - 0.004), ACCENT);
            }
        }
        // **Where the way out is, said once, on a phone.** A desktop's help
        // line already says `esc`; a phone had nothing in place of the
        // cross, and a way out nobody is told about is a player reaching
        // for the Back gesture and hoping. In the empty right-hand end of
        // the header, level with the tabs, in the quiet ink.
        if widgets::touch_layout() {
            let hint = language.text(Msg::JournalCloseTouch);
            let last = header_rect(Header::Tab(Tab::Give), aspect);
            let scale = 0.75;
            let cap = widgets::PIXEL * scale * crate::engine::font::CAP_HEIGHT as f32;
            let x = (panel.x1 - 0.03 - widgets::measure(hint, scale)).max(last.x1 + 0.03);
            p.text(hint, x, last.centre_y() + cap / 2.0, scale, widgets::TEXT_DIM);
        }

        let body = body_rect(aspect);
        let footer_middle = panel.y0 + FOOTER / 2.0;
        let help_scale = 0.75;
        let cap = widgets::PIXEL * help_scale * crate::engine::font::CAP_HEIGHT as f32;
        match tab {
            Tab::Map => {
                map_screen::paint(&mut p, &self.map, &self.explored, player, &self.landmarks, body, self.cursor, language);
                map_screen::legend(&mut p, body.x0, footer_middle, language);
                let help = language.text(by_input(Msg::MapHelp, Msg::MapHelpTouch));
                let width = widgets::measure(help, help_scale);
                p.text(help, body.x1 - width, footer_middle + cap / 2.0, help_scale, widgets::TEXT_DIM);
            }
            Tab::Recipes => {
                self.book.paint(&mut p, layers, inventory, &self.discovered, body, self.cursor, language);
                let help = language.text(by_input(Msg::RecipesHelp, Msg::RecipesHelpTouch));
                p.text(help, body.x0, footer_middle + cap / 2.0, help_scale, widgets::TEXT_DIM);
            }
            Tab::Give => {
                self.give.paint(&mut p, layers, body, self.cursor, language);
                let help = language.text(by_input(Msg::GiveHelp, Msg::GiveHelpTouch));
                p.text(help, body.x0, footer_middle + cap / 2.0, help_scale, widgets::TEXT_DIM);
            }
        }
        *out = p.into_vertices();
    }
}

const SCRIM: [f32; 4] = [0.02, 0.02, 0.03, 0.55];
const ACCENT: [f32; 4] = [0.95, 0.72, 0.30, 1.0];

// ---- no compass ----
//
// **There was a compass on the HUD**, a plate at the top of the view with
// an arrow and the metres to the nearest bag, and it is gone on purpose:
// "убери компас". An arrow that always knows the way turns the walk back
// to a bag into following a pointer, which is a chore with one correct
// answer and not a decision. The way back is the map's now -- the bag is a
// red square on it (`map_screen`), and reading a map means stopping,
// looking at the land and choosing a way, which is the walk this game is
// about. The server still sends the bags (`ServerMessage::Landmarks`),
// because the map is what draws them.
//
// **There is a compass again, and it is not that one.** A water compass
// (`types::BLOCK_WATER_COMPASS`), made from an iron nail and
// held in the hand, draws a needle to *north* -- never to a bag. It says
// which way the top of this map is from where the player stands, which is
// how a map is read; the way is still chosen off the land. Before iron the
// sky says the same thing to a player who looks up (`logic::bearing`).
//
// Nothing to migrate: the compass was never an item, a recipe or a saved
// field on the client -- only a picture of the landmarks the map also
// reads -- so old worlds and profiles load exactly as they did.

#[cfg(test)]
mod tests {
    use super::*;

    fn with_bags(bags: &[(i32, i32, i32)]) -> Landmarks {
        Landmarks { spawn: Some((0, 64, 0)), bags: bags.to_vec() }
    }

    #[test]
    fn every_header_button_is_pressed_where_it_is_drawn() {
        for aspect in [1.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            for header in HEADERS {
                let rect = header_rect(header, aspect);
                assert_eq!(header_at((rect.centre_x(), rect.centre_y()), aspect), Some(header));
                assert!(rect.x1 <= panel_rect(aspect).x1, "{header:?} is off the journal at aspect {aspect}");
                assert!(rect.y0 >= body_rect(aspect).y1, "{header:?} reaches into the page at aspect {aspect}");
            }
        }
    }

    #[test]
    fn the_header_is_a_finger_tall_on_a_phone() {
        widgets::as_a_phone(|| {
            for header in HEADERS {
                assert!(header_rect(header, 2.2).height() >= widgets::FINGER_SIDE - 1e-5, "{header:?}");
            }
        });
    }

    #[test]
    fn tab_reaches_every_page_and_comes_home() {
        // Three pages, one key: a page the keyboard could not turn to
        // would be a page only a mouse could reach.
        let mut journal = Journal::new();
        // An operator, because the give page is only theirs to open --
        // see `a_player_who_is_not_an_operator_has_no_give_page`.
        journal.set_operator(true);
        journal.toggle(Tab::Map);
        let mut seen = vec![journal.open];
        for _ in 0..HEADERS.len() {
            journal.switch_tab();
            seen.push(journal.open);
        }
        for header in HEADERS {
            let Header::Tab(tab) = header;
            assert!(seen.contains(&Some(tab)), "{tab:?} cannot be reached with tab");
        }
        assert_eq!(journal.open, Some(Tab::Map), "turning the page all the way round did not come home");
    }

    #[test]
    fn tapping_a_thing_on_the_give_page_leaves_a_command_for_the_frame_loop() {
        // End to end through the screen the player actually touches: the
        // header button, then a cell, then the line that goes on the
        // wire. The client itself hands nothing over -- see
        // `give_screen`'s module note.
        let aspect = 16.0 / 9.0;
        let player = PlayerMark::default();
        let mut journal = Journal::new();
        // An operator, because the give page is only theirs to open --
        // see `a_player_who_is_not_an_operator_has_no_give_page`.
        journal.set_operator(true);
        journal.toggle(Tab::Give);
        let body = body_rect(aspect);
        let cell = give_screen::cell_rect(0, body);
        journal.touch(1, TouchPhase::Started, (cell.centre_x(), cell.centre_y()), aspect, player);
        journal.touch(1, TouchPhase::Ended, (cell.centre_x(), cell.centre_y()), aspect, player);
        let command = journal.take_command().expect("a tap on a thing asks for it");
        assert!(command.starts_with("/give "), "{command:?} is not the command an operator would type");
        assert_eq!(journal.take_command(), None, "the same command would have gone twice");
    }

    #[test]
    fn a_refusal_is_taken_out_of_the_chat_and_a_stranger_s_line_is_not() {
        let aspect = 16.0 / 9.0;
        let mut journal = Journal::new();
        // An operator, because the give page is only theirs to open --
        // see `a_player_who_is_not_an_operator_has_no_give_page`.
        journal.set_operator(true);
        assert!(!journal.take_server_note("alice joined"), "the journal ate somebody else's line");
        journal.toggle(Tab::Give);
        let body = body_rect(aspect);
        let cell = give_screen::cell_rect(0, body);
        journal.set_cursor(Some((cell.centre_x(), cell.centre_y())), aspect, PlayerMark::default());
        journal.press(aspect, PlayerMark::default());
        assert!(journal.take_command().is_some());
        assert!(journal.take_server_note("'give' is operator-only"), "the refusal would have gone to the chat log");
    }

    #[test]
    fn the_give_page_takes_typing_and_the_map_does_not() {
        let mut journal = Journal::new();
        // An operator, because the give page is only theirs to open --
        // see `a_player_who_is_not_an_operator_has_no_give_page`.
        journal.set_operator(true);
        journal.toggle(Tab::Map);
        assert!(!journal.takes_text(), "letters on the map would throw away what is in the hand");
        journal.toggle(Tab::Give);
        assert!(journal.takes_text());
        for c in "stone".chars() {
            journal.type_char(c);
        }
        assert_eq!(journal.give.query(), "stone");
        journal.backspace();
        assert_eq!(journal.give.query(), "ston");
    }

    #[test]
    fn the_key_that_opens_a_tab_also_shuts_it() {
        let mut journal = Journal::new();
        assert!(journal.toggle(Tab::Map));
        assert!(journal.toggle(Tab::Recipes), "the other tab shut the journal instead of switching");
        assert!(!journal.toggle(Tab::Recipes));
        assert!(!journal.is_open());
    }

    #[test]
    fn a_shut_journal_draws_nothing() {
        let journal = Journal::new();
        let mut out = Vec::new();
        journal.build_into(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &Inventory::new(),
            PlayerMark::default(),
            16.0 / 9.0,
            Language::English,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_tap_beside_the_page_shuts_the_journal_and_a_drag_that_ends_there_does_not() {
        let aspect = 16.0 / 9.0;
        let panel = panel_rect(aspect);
        let beside = ((panel.x1 + aspect) / 2.0, 0.0);
        let inside = (panel.x1 - 0.3, 0.0);
        let player = PlayerMark::default();

        let mut journal = Journal::new();
        journal.toggle(Tab::Map);
        journal.touch(1, TouchPhase::Started, inside, aspect, player);
        journal.touch(1, TouchPhase::Moved, beside, aspect, player);
        assert_ne!(journal.touch(1, TouchPhase::Ended, beside, aspect, player), Outcome::Closed);
        assert!(journal.is_open(), "a drag across the map that ended off the page shut the journal");

        journal.touch(2, TouchPhase::Started, beside, aspect, player);
        assert_eq!(journal.touch(2, TouchPhase::Ended, beside, aspect, player), Outcome::Closed);
        assert!(!journal.is_open());
    }

    /// Two fingers on the map scale it, and neither of them is a press.
    ///
    /// The gesture every phone map has, in the one place on this screen
    /// that had no use for a second finger: `touch` took the first and
    /// dropped the rest on the floor.
    #[test]
    fn two_fingers_spreading_on_the_map_zoom_it_and_press_nothing() {
        let aspect = 2712.0 / 1220.0;
        let player = PlayerMark::default();
        let body = body_rect(aspect);
        let middle = (body.centre_x(), body.centre_y());
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);
        let was = journal.map.scale();

        journal.touch(1, TouchPhase::Started, (middle.0 - 0.1, middle.1), aspect, player);
        journal.touch(2, TouchPhase::Started, (middle.0 + 0.1, middle.1), aspect, player);
        journal.touch(1, TouchPhase::Moved, (middle.0 - 0.3, middle.1), aspect, player);
        journal.touch(2, TouchPhase::Moved, (middle.0 + 0.3, middle.1), aspect, player);
        assert!(
            journal.map.scale() > was * 2.0,
            "spreading from 0.2 apart to 0.6 left the scale at {}",
            journal.map.scale(),
        );

        assert_eq!(journal.touch(2, TouchPhase::Ended, (middle.0 + 0.3, middle.1), aspect, player), Outcome::Nothing);
        assert_eq!(journal.touch(1, TouchPhase::Ended, (middle.0 - 0.3, middle.1), aspect, player), Outcome::Changed);
        assert!(journal.is_open(), "a pinch on the map shut the journal");
    }

    /// A pinch that begins on a button does not also press it.
    ///
    /// The `+` button is a finger wide and sits in the corner of the map:
    /// a thumb going down for a pinch lands on it as often as not. If the
    /// lift still counted as a tap, every pinch that started there would
    /// zoom one extra step the player never asked for -- the same
    /// surprise a drag that also pressed what it started on would be.
    #[test]
    fn a_pinch_that_starts_on_the_zoom_button_does_not_also_press_it() {
        let aspect = 2712.0 / 1220.0;
        let player = PlayerMark::default();
        let body = body_rect(aspect);
        let plus = map_screen::control_rect(map_screen::Control::ZoomIn, body);
        let on_the_button = (plus.centre_x(), plus.centre_y());
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);

        journal.touch(1, TouchPhase::Started, on_the_button, aspect, player);
        journal.touch(2, TouchPhase::Started, (body.centre_x(), body.centre_y()), aspect, player);
        journal.touch(2, TouchPhase::Moved, (body.centre_x() + 0.2, body.centre_y()), aspect, player);
        let before_the_lift = journal.map.scale();
        journal.touch(1, TouchPhase::Ended, on_the_button, aspect, player);
        journal.touch(2, TouchPhase::Ended, (body.centre_x() + 0.2, body.centre_y()), aspect, player);
        assert!(
            (journal.map.scale() - before_the_lift).abs() < 1e-6,
            "the lift pressed the + button: the scale went from {before_the_lift} to {}",
            journal.map.scale(),
        );
    }

    /// One thumb up after a pinch, and the map is still under the other.
    ///
    /// **The lift that ends a pinch is not the end of the gesture.** A
    /// hand that has finished scaling keeps dragging with whichever
    /// finger is still down; letting the drag die with the finger that
    /// happened to be first left the player holding a map that had
    /// stopped answering until they lifted and touched it again.
    #[test]
    fn lifting_one_finger_of_a_pinch_leaves_the_other_dragging_the_map() {
        let aspect = 2712.0 / 1220.0;
        let player = PlayerMark::default();
        let body = body_rect(aspect);
        let middle = (body.centre_x(), body.centre_y());
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);

        journal.touch(1, TouchPhase::Started, (middle.0 - 0.1, middle.1), aspect, player);
        journal.touch(2, TouchPhase::Started, (middle.0 + 0.1, middle.1), aspect, player);
        // The first finger goes; the second is still on the glass.
        journal.touch(1, TouchPhase::Ended, (middle.0 - 0.1, middle.1), aspect, player);
        let before = journal.map.centre(player);
        journal.touch(2, TouchPhase::Moved, (middle.0 + 0.4, middle.1), aspect, player);
        let after = journal.map.centre(player);
        assert!(
            (after.0 - before.0).abs() > 1.0,
            "the remaining finger dragged the map from {before:?} to {after:?}",
        );
    }

    /// The recipe page has nothing to scale, so a second finger there is
    /// still ignored -- and in particular does not scroll the list twice.
    #[test]
    fn a_second_finger_on_the_recipe_page_is_still_nothing_at_all() {
        let aspect = 2712.0 / 1220.0;
        let player = PlayerMark::default();
        let body = body_rect(aspect);
        let middle = (body.centre_x(), body.centre_y());
        let mut journal = Journal::new();
        journal.toggle(Tab::Recipes);
        journal.touch(1, TouchPhase::Started, middle, aspect, player);
        journal.touch(2, TouchPhase::Started, (middle.0 + 0.2, middle.1), aspect, player);
        assert!(journal.second.is_none(), "the book took a second finger");
        assert_eq!(
            journal.touch(2, TouchPhase::Moved, (middle.0 + 0.6, middle.1), aspect, player),
            Outcome::Nothing,
            "a second finger moved the book",
        );
    }

    #[test]
    fn a_mouse_click_beside_the_page_leaves_the_journal_open_and_the_mouse_free() {
        let aspect = 16.0 / 9.0;
        let panel = panel_rect(aspect);
        let player = PlayerMark::default();
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);
        journal.set_cursor(Some(((panel.x1 + aspect) / 2.0, 0.5)), aspect, player);
        assert_eq!(journal.press(aspect, player), Outcome::Nothing);
        assert!(journal.is_open(), "a click beside the map shut the journal and took the mouse");
    }

    #[test]
    fn a_phone_has_a_finger_of_glass_beside_the_journal_to_shut_it_with() {
        // With the cross gone this is the only way out a thumb has, so it
        // has to be a thumb wide on every shape a phone is held at.
        widgets::as_a_phone(|| {
            for aspect in [16.0f32 / 9.0, 2712.0 / 1220.0, 20.0 / 9.0] {
                let panel = panel_rect(aspect);
                let room = aspect - panel.x1;
                assert!(
                    room >= widgets::FINGER_SIDE - 1e-5,
                    "at {aspect:.2} the glass beside the journal is {room:.3}, under a finger of {:.3}",
                    widgets::FINGER_SIDE,
                );
                let beside = (aspect - room / 2.0, -0.4);
                let player = PlayerMark::default();
                let mut journal = Journal::new();
                journal.toggle(Tab::Map);
                journal.touch(4, TouchPhase::Started, beside, aspect, player);
                assert_eq!(
                    journal.touch(4, TouchPhase::Ended, beside, aspect, player),
                    Outcome::Closed,
                    "at {aspect:.2} a tap in the middle of that glass did not shut it",
                );
            }
        });
    }

    #[test]
    fn a_finger_dragged_across_the_map_moves_the_map() {
        let aspect = 16.0 / 9.0;
        let body = body_rect(aspect);
        let player = PlayerMark::default();
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);
        let start = (body.centre_x() - 0.5, body.centre_y());
        journal.touch(7, TouchPhase::Started, start, aspect, player);
        journal.touch(7, TouchPhase::Moved, (start.0 + 0.4, start.1), aspect, player);
        journal.touch(7, TouchPhase::Ended, (start.0 + 0.4, start.1), aspect, player);
        assert!(!journal.map.is_following(), "the map stayed on the player through a drag");
        let (cx, _) = journal.map.centre(player);
        assert!(cx < 0.0, "dragging right moved the land left: centre {cx}");
    }

    #[test]
    fn letters_are_a_search_only_in_the_book() {
        let mut journal = Journal::new();
        journal.toggle(Tab::Map);
        journal.type_char('m');
        journal.toggle(Tab::Recipes);
        assert_eq!(journal.book.query(), "", "a letter typed on the map searched the book");
        journal.type_char('a');
        journal.type_char('x');
        journal.backspace();
        assert_eq!(journal.book.query(), "a");
    }

    #[test]
    fn leaving_a_world_forgets_its_map_and_its_book() {
        let mut journal = Journal::new();
        journal.discovered.note(primitive_shared::types::BLOCK_LOG);
        journal.landmarks = with_bags(&[(1, 2, 3)]);
        journal.explored.insert(
            primitive_shared::types::ChunkPos::new(0, 0),
            crate::logic::map::Tile::uniform(crate::logic::map::Ground::Grass, 64),
        );
        journal.toggle(Tab::Map);
        journal.end_session();
        assert!(journal.discovered.is_empty() && journal.landmarks.bags.is_empty());
        assert_eq!(journal.explored.surveyed(), 0);
        assert!(!journal.is_open());
    }

    #[test]
    fn a_shut_journal_with_bags_out_there_puts_nothing_on_the_view() {
        // **The compass is gone.** It was the one thing this module drew
        // with the journal shut, and only while a bag was out there -- so
        // that is the case asked about: bags remembered, the journal shut,
        // and not one vertex on the screen. The bags are still the map's.
        let mut journal = Journal::new();
        journal.landmarks = with_bags(&[(50, 60, 50), (-400, 64, 12)]);
        let mut out = Vec::new();
        journal.build_into(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &Inventory::new(),
            PlayerMark { x: 0.0, z: 0.0, yaw: 0.3 },
            16.0 / 9.0,
            Language::Russian,
            &mut out,
        );
        assert!(out.is_empty(), "a shut journal drew {} vertices over the world", out.len());
        assert_eq!(journal.landmarks.bags.len(), 2, "the map lost the bags it marks");
    }

    /// **"Сделай возможность открывать это меню только если у игрока есть
    /// операторские права".** The give page is `/give` with a screen in
    /// front of it, and `/give` is the operator's. It used to open for
    /// anybody and then say it was refusing them, which is a menu a player
    /// reads as broken rather than as forbidden.
    #[test]
    fn a_player_who_is_not_an_operator_has_no_give_page() {
        const ASPECT: f32 = 16.0 / 9.0;
        let mut journal = Journal::new();
        // The key.
        journal.toggle(Tab::Give);
        assert_ne!(journal.open, Some(Tab::Give), "the give key opened a page this player may not use");
        // The tab, pressed exactly where it would have been drawn.
        journal.toggle(Tab::Map);
        let tab = header_rect(Header::Tab(Tab::Give), ASPECT);
        journal.click_at((tab.centre_x(), tab.centre_y()), ASPECT, PlayerMark::default());
        assert_eq!(journal.open, Some(Tab::Map), "the give tab is not drawn, and it was pressed anyway");
        // ...and the key that turns the page does not stop on it.
        journal.open = Some(Tab::Recipes);
        journal.switch_tab();
        assert_eq!(journal.open, Some(Tab::Map), "Tab landed on a page that is not there");
    }

    #[test]
    fn the_give_page_is_there_for_an_operator_and_goes_when_the_server_takes_it_back() {
        const ASPECT: f32 = 16.0 / 9.0;
        let mut journal = Journal::new();
        journal.set_operator(true);
        journal.toggle(Tab::Give);
        assert_eq!(journal.open, Some(Tab::Give), "an operator was refused their own menu");
        journal.open = Some(Tab::Recipes);
        journal.switch_tab();
        assert_eq!(journal.open, Some(Tab::Give), "Tab skipped the page an operator has");
        journal.open = Some(Tab::Give);
        // Demoted with the page open: the server is the authority in both
        // directions, and a player left looking at a menu that will now
        // refuse them is the state this whole change is about.
        journal.set_operator(false);
        assert_eq!(journal.open, Some(Tab::Map), "a demoted player was left on the give page");
        let tab = header_rect(Header::Tab(Tab::Give), ASPECT);
        journal.click_at((tab.centre_x(), tab.centre_y()), ASPECT, PlayerMark::default());
        assert_eq!(journal.open, Some(Tab::Map));
    }
}
