//! The chest screen: what is in the box, and what is in your pack.
//!
//! ## Two grids, one gesture
//!
//! Everything here is the inventory screen's interaction with one thing
//! added: a slot is now a *side* as well as a number. Click to pick up,
//! click to put down, right click to place half, shift click to send a
//! stack across -- and because the side rides along with the index,
//! every one of those works within a grid and between the two without
//! any of them being a special case.
//!
//! The crafting column is deliberately absent. A chest is a place to
//! put things down, the screen is already two grids wide, and a third
//! column would push the whole thing off a 16:9 window -- which is the
//! mistake the inventory screen's own layout notes record.
//!
//! ## Nothing here is authoritative
//!
//! The screen draws two snapshots and emits intents; the server owns
//! both inventories and answers every gesture with what they contain
//! afterwards. That is what lets two players share a chest: neither
//! client predicts anything, so neither can be wrong about it for
//! longer than a round trip.
//!
//! `sync` is the one subtlety, and it is the same one the inventory
//! screen has: a pick-up is an *index*, and between picking up and
//! putting down the other player at the chest can change what is in it.
//! If the block under the pick-up is not the one that was taken hold
//! of, the gesture is dropped rather than completed against whatever is
//! there now.

use primitive_shared::protocol::Side;

use crate::engine::texture::{FaceLayers, FontAtlas};
use crate::logic::inventory::{Inventory, HOTBAR_SLOTS, SLOTS};
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{Language, Msg};
use crate::ui::inventory_screen::{
    draw_slot, held_stack, Button, SlotEdge, CELL, GAP, HOTBAR_SPLIT, HOTBAR_STRIP, PANEL_PAD,
    RULE,
};
use crate::ui::widgets::{self, Painter, Rect};

/// Rows in each grid. Both inventories have the same shape -- a chest
/// holds exactly what a player can carry -- so there is one grid to
/// draw, twice, and no second set of numbers to keep in step.
const ROWS: usize = SLOTS / HOTBAR_SLOTS;
/// Space between the chest's grid and the player's, so the two read as
/// two places rather than as one grid of eight rows.
const GRID_SPLIT: f32 = 0.070;
/// Room for each grid's label, above it.
const LABEL_HEIGHT: f32 = 0.052;
const HEADER_HEIGHT: f32 = 0.058;
/// Room under the lower grid for the readout and the two lines of
/// hints. Three lines and the space around them; anything less and the
/// belt row is drawn over.
const FOOTER_HEIGHT: f32 = 0.215;

const HINT_SCALE: f32 = 0.68;
/// Kept short on purpose: the panel is exactly as wide as ten slots,
/// and a line of prose that runs past that runs out of the screen. A
/// test measures them against the panel.
const HINTS: [Msg; 2] = [Msg::ChestHint1, Msg::ChestHint2];

/// What a click wants the server to do. The client never moves anything
/// itself -- see the note at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Onto the same block it merges, onto anything else it swaps.
    /// `half` is the right-click.
    Move {
        from: (Side, usize),
        to: (Side, usize),
        half: bool,
    },
    /// Straight across to the other side, wherever it fits.
    QuickMove(Side, usize),
    /// Everything that fits, in one gesture. True to fill the chest.
    ///
    /// The one thing every storage screen has and this one did not:
    /// emptying a full pack into a chest was forty separate
    /// shift-clicks, and the player counted them.
    BulkMove { to_chest: bool },
}

pub struct ChestScreen {
    /// Where the open chest is, and the whole of "is this screen open".
    at: Option<(i32, i32, i32)>,
    /// The last snapshot of its contents. Empty until the server
    /// answers, which is a fraction of a second in which the screen
    /// honestly says the chest is empty rather than guessing.
    contents: Inventory,
    held: Option<(Side, usize)>,
    held_block: Option<primitive_shared::types::BlockId>,
    cursor: Option<(f32, f32)>,
    /// What to call the thing being looked into.
    ///
    /// Decided from the block in the world rather than from anything the
    /// server said, because the server never says: every message about a
    /// container names a side and a slot, deliberately, so the only
    /// thing the client is told is *where* it is. The block at that cell
    /// is a lookup the client already has.
    heading: Msg,
}

impl Default for ChestScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl ChestScreen {
    pub fn new() -> Self {
        Self {
            at: None,
            contents: Inventory::new(),
            held: None,
            held_block: None,
            cursor: None,
            heading: Msg::Chest,
        }
    }

    pub fn is_open(&self) -> bool {
        self.at.is_some()
    }

    /// Which chest is open.
    ///
    /// Nothing outside the tests asks: the messages that act on a chest
    /// name a side and a slot rather than a position, precisely so that
    /// the client cannot reach into one it did not open. It is here
    /// because "which chest" is the state this screen *is*, and being
    /// unable to ask would be strange.
    #[allow(dead_code)]
    pub fn at(&self) -> Option<(i32, i32, i32)> {
        self.at
    }

    /// The server has answered with a chest's contents.
    ///
    /// Also what *opens* the screen: the client asks, and the screen
    /// appears when the answer arrives. Opening on the click and filling
    /// in later would show an empty chest for a round trip, and an empty
    /// chest is a thing a player will act on.
    ///
    /// `block` is what the client has in that cell, if it has the chunk
    /// at all. `None` leaves the heading alone rather than guessing: an
    /// update to a container already open cannot have changed what kind
    /// of container it is, and a chunk that has not arrived is not a
    /// reason to relabel the screen the player is looking at.
    pub fn show(
        &mut self,
        at: (i32, i32, i32),
        contents: Inventory,
        block: Option<primitive_shared::types::BlockId>,
    ) {
        if self.at != Some(at) {
            // A different chest: nothing carried over from the last one.
            self.release();
            self.cursor = Some((0.0, 0.0));
            self.heading = Msg::Chest;
        }
        if let Some(block) = block {
            self.heading = heading_for(block);
        }
        self.at = Some(at);
        self.contents = contents;
        self.sync();
    }

    pub fn close(&mut self) {
        self.at = None;
        self.contents = Inventory::new();
        self.heading = Msg::Chest;
        self.release();
    }

    /// Which heading this screen is wearing. For the tests, which assert
    /// on the word rather than on the pixels it turns into.
    #[allow(dead_code)]
    pub fn heading(&self) -> Msg {
        self.heading
    }

    fn release(&mut self) {
        self.held = None;
        self.held_block = None;
    }

    pub fn set_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.cursor = cursor;
    }

    /// Exposed for the tests, which assert on the pick-up state machine
    /// rather than on the pixels it produces.
    #[allow(dead_code)]
    pub fn held(&self) -> Option<(Side, usize)> {
        self.held
    }

    /// Drops a pick-up whose slot no longer holds what was picked up.
    ///
    /// Called with each fresh snapshot of either inventory. The chest is
    /// the one place in this game where *another player* can change a
    /// slot between the two halves of a gesture, so this matters more
    /// here than it does in the pack.
    pub fn sync_with(&mut self, pack: &Inventory) {
        if let Some((Side::Pack, slot)) = self.held {
            if pack.block_in(slot) != self.held_block || self.held_block.is_none() {
                self.release();
            }
        }
        self.sync();
    }

    fn sync(&mut self) {
        if let Some((Side::Chest, slot)) = self.held {
            if self.contents.block_in(slot) != self.held_block || self.held_block.is_none() {
                self.release();
            }
        }
    }

    /// Handles a click. `quick` is the shift modifier.
    pub fn click(&mut self, pack: &Inventory, button: Button, quick: bool) -> Option<Intent> {
        if let Some(cursor) = self.cursor {
            for to_chest in [true, false] {
                if bulk_button_rect(to_chest).contains(cursor.0, cursor.1) {
                    // A bulk move with a stack in hand would be two
                    // gestures at once, and the held stack is the one
                    // the player is thinking about -- so it is put back
                    // rather than swept along with everything else.
                    self.release();
                    return Some(Intent::BulkMove { to_chest });
                }
            }
        }
        let cursor = self.cursor?;
        let Some(target) = slot_at(cursor) else {
            // Outside the grids: cancel. Nothing was taken out of
            // anywhere, so there is nothing to put back.
            self.release();
            return None;
        };
        let count_in = |(side, slot): (Side, usize)| match side {
            Side::Pack => pack.count_in(slot),
            Side::Chest => self.contents.count_in(slot),
        };
        let block_in = |(side, slot): (Side, usize)| match side {
            Side::Pack => pack.block_in(slot),
            Side::Chest => self.contents.block_in(slot),
        };

        match (button, self.held) {
            // Half of what is held, keeping the rest: dealing a stack
            // out across several slots is one gesture repeated.
            (Button::Right, Some(from)) => (from != target).then_some(Intent::Move {
                from,
                to: target,
                half: true,
            }),
            // Nothing is held in limbo here, so a right click with empty
            // hands has nowhere to put half of anything.
            (Button::Right, None) => None,
            (Button::Left, Some(from)) => {
                self.release();
                (from != target).then_some(Intent::Move {
                    from,
                    to: target,
                    half: false,
                })
            }
            (Button::Left, None) => {
                if count_in(target) == 0 {
                    return None; // picking up nothing is not a gesture
                }
                if quick {
                    return Some(Intent::QuickMove(target.0, target.1));
                }
                self.held = Some(target);
                self.held_block = block_in(target);
                None
            }
        }
    }

    /// The slot under the pointer, for the throw-out key.
    pub fn hovered(&self) -> Option<(Side, usize)> {
        self.cursor.and_then(slot_at)
    }

    /// A fingerprint of what `build` would draw, cheap enough to take
    /// every frame -- see the UI block in `main`, which only rebuilds
    /// the interface when a key like this one changes.
    ///
    /// The chest's contents are in it because they live *here* rather
    /// than in the caller's inventory -- another player can change them
    /// mid-look, and that change arrives as a snapshot with no local
    /// event attached. The raw cursor is in it because the stack in hand
    /// rides the pointer.
    pub fn ui_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.at.hash(&mut h);
        self.heading.hash(&mut h);
        self.held
            .map(|(side, slot)| (side == Side::Chest, slot))
            .hash(&mut h);
        self.cursor
            .map(|(x, y)| (x.to_bits(), y.to_bits()))
            .hash(&mut h);
        for slot in self.contents.slots() {
            slot.map(|stack| (stack.block, stack.count)).hash(&mut h);
        }
        h.finish()
    }

    /// The `Vec`-returning form, kept for the tests: they assert on
    /// one widget's output in isolation, which is exactly what appending
    /// into a shared list is designed not to produce.
    #[cfg(test)]
    pub fn build(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        pack: &Inventory,
        language: Language,
    ) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        self.build_into(font, layers, pack, language, &mut out);
        out
    }

    /// The same screen, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        pack: &Inventory,
        language: Language,
        out: &mut Vec<HotbarVertex>,
    ) {
        if !self.is_open() {
            return;
        }
        let mut p = Painter::onto(font, std::mem::take(out));
        p.scrim(widgets::SCRIM);

        let panel = panel_rect();
        p.deep_panel(panel);
        p.panel_header(panel, language.text(self.heading), HEADER_HEIGHT);

        let hovered = self.cursor.and_then(slot_at);
        // Two words rather than "chest" and "pack" again: the heading
        // already says which screen this is, and what the player is
        // deciding between is what is *stored* and what they are
        // *carrying*.
        for (side, inventory, label) in [
            (Side::Chest, &self.contents, language.text(Msg::Stored)),
            (Side::Pack, pack, language.text(Msg::Carried)),
        ] {
            let first = slot_rect(side, 0);
            // Above the *top* row of the grid it names. Slot
            // `HOTBAR_SLOTS` is the first of the storage rows, which is
            // that row's left end; the bottom row is the belt.
            let top_left = slot_rect(side, HOTBAR_SLOTS);
            p.text(
                label,
                top_left.x0,
                top_left.y1 + LABEL_HEIGHT - 0.016,
                0.86,
                widgets::ACCENT,
            );

            // The strip behind the player's bottom row, marking the ten
            // slots that are on screen during play. The chest has no
            // such row -- nothing in it is in hand.
            if side == Side::Pack {
                let last = slot_rect(side, HOTBAR_SLOTS - 1);
                let strip = Rect::new(
                    first.x0 - 0.012,
                    first.y0 - 0.012,
                    last.x1 + 0.012,
                    last.y1 + 0.012,
                );
                p.vgradient(strip, HOTBAR_STRIP, [0.07, 0.08, 0.11, 1.0]);
                p.border(strip, 0.0025, RULE);
                p.text(language.text(Msg::Belt), strip.x0, strip.y0 - 0.006, 0.62, widgets::TEXT_DIM);
            }

            for slot in 0..SLOTS {
                let edge = if self.held == Some((side, slot)) {
                    SlotEdge::Source
                } else if hovered == Some((side, slot)) {
                    SlotEdge::Hovered
                } else {
                    SlotEdge::Plain
                };
                draw_slot(
                    &mut p,
                    slot_rect(side, slot),
                    layers,
                    inventory.block_in(slot),
                    inventory.count_in(slot),
                    edge,
                );
            }
        }

        // The two bulk buttons, in the gap between the grids.
        for to_chest in [true, false] {
            let rect = bulk_button_rect(to_chest);
            let hovered = self
                .cursor
                .is_some_and(|(x, y)| rect.contains(x, y));
            p.quad(rect, if hovered { widgets::BUTTON_HOVER } else { widgets::BUTTON });
            p.border(
                rect,
                0.0025,
                if hovered { widgets::ACCENT } else { widgets::BUTTON_EDGE },
            );
            // The arrow points the way the things go, and the word says
            // what goes: "store" is not a direction and neither is an
            // arrow on its own.
            let label = language.text(if to_chest { Msg::StoreAll } else { Msg::TakeAll });
            p.label_in(rect, label, 0.72, widgets::TEXT);
        }

        // What the chest is holding, in the units the player thinks in.
        // Weight, because it is the number the whole load mechanic turns
        // on -- and a chest is where you go to stop carrying it.
        let stored = self.contents.total_items();
        let used = (0..SLOTS)
            .filter(|&slot| self.contents.count_in(slot) > 0)
            .count();
        let summary = format!(
            "{used}/{SLOTS} {}   {stored} {}   {:.0} kg",
            language.text(Msg::SlotsWord),
            language.text(Msg::ItemsWord),
            self.contents.total_weight(),
        );
        let footer_rule = panel.y0 + FOOTER_HEIGHT - 0.010;
        p.quad(
            Rect::new(panel.x0 + 0.03, footer_rule, panel.x1 - 0.03, footer_rule + 0.0025),
            RULE,
        );
        p.text(&summary, grid_left(), panel.y0 + 0.160, 0.86, widgets::TEXT);
        for (line, hint) in HINTS.iter().enumerate() {
            p.text(
                language.text(*hint),
                grid_left(),
                panel.y0 + 0.108 - line as f32 * 0.040,
                HINT_SCALE,
                widgets::TEXT_DIM,
            );
        }

        // Last, over everything it describes: the stack in hand rides
        // the cursor, or there is nothing on screen saying one is held.
        if let (Some(cursor), Some((side, slot))) = (self.cursor, self.held) {
            let inventory = match side {
                Side::Pack => pack,
                Side::Chest => &self.contents,
            };
            held_stack(&mut p, layers, inventory, slot, cursor);
        }

        *out = p.into_vertices();
    }
}

/// What to call the container in `cell`.
///
/// One place, so the heading cannot say "chest" while something else on
/// the screen says otherwise. Anything that is not a backpack is a
/// chest: the fallback matters, because this is asked about a block that
/// may have arrived from a build the client does not fully know.
fn heading_for(block: primitive_shared::types::BlockId) -> Msg {
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_BACKPACK {
        Msg::Backpack
    } else {
        Msg::Chest
    }
}

// ---- layout ----
//
// Two grids, one above the other, each the same shape as the inventory
// screen's. Everything is derived from the cell size, so there is one
// number to change and a test that checks the result fits a *square*
// window -- the worst case the aspect divide can produce.

fn grid_width() -> f32 {
    HOTBAR_SLOTS as f32 * CELL + (HOTBAR_SLOTS as f32 - 1.0) * GAP
}

/// Height of one grid, its label included.
fn grid_height() -> f32 {
    ROWS as f32 * CELL + (ROWS as f32 - 1.0) * GAP + HOTBAR_SPLIT + LABEL_HEIGHT
}

fn grid_left() -> f32 {
    -grid_width() / 2.0
}

/// Top of the upper grid's cells.
fn content_top() -> f32 {
    (grid_height() * 2.0 + GRID_SPLIT) / 2.0 - LABEL_HEIGHT
}

fn panel_rect() -> Rect {
    Rect::new(
        grid_left() - PANEL_PAD,
        -content_top() - LABEL_HEIGHT - FOOTER_HEIGHT,
        grid_left() + grid_width() + PANEL_PAD,
        content_top() + LABEL_HEIGHT + HEADER_HEIGHT,
    )
}

/// Width and height of the two bulk buttons.
const BULK_WIDTH: f32 = 0.30;
const BULK_HEIGHT: f32 = 0.050;

/// Where a bulk button sits: in the gap between the two grids, pointing
/// the way it moves things.
///
/// Between them on purpose. The gesture is "send what I am carrying up
/// there" or "bring that down here", and a button that sits on the
/// route reads as the route; the same two buttons in the footer would
/// need words to say which direction they meant.
pub fn bulk_button_rect(to_chest: bool) -> Rect {
    let middle = content_top() - grid_height() - GRID_SPLIT / 2.0;
    let centre_y = middle + LABEL_HEIGHT * 0.5;
    let x0 = if to_chest {
        grid_left() + grid_width() - BULK_WIDTH
    } else {
        grid_left()
    };
    Rect::new(x0, centre_y - BULK_HEIGHT / 2.0, x0 + BULK_WIDTH, centre_y + BULK_HEIGHT / 2.0)
}

/// Where a slot sits on screen.
///
/// Within each grid the layout is the inventory screen's: the ten slots
/// that are the hotbar are the *bottom* row, matching where the real bar
/// is, and storage runs above them in reading order. The chest's grid
/// uses the same shape because it is the same inventory type -- its
/// bottom row is just ten more slots.
pub fn slot_rect(side: Side, slot: usize) -> Rect {
    let top = match side {
        Side::Chest => content_top(),
        Side::Pack => content_top() - grid_height() - GRID_SPLIT,
    };
    let (column, row_from_top, extra) = if slot < HOTBAR_SLOTS {
        (slot, ROWS - 1, HOTBAR_SPLIT)
    } else {
        let storage = slot - HOTBAR_SLOTS;
        (storage % HOTBAR_SLOTS, storage / HOTBAR_SLOTS, 0.0)
    };
    let x0 = grid_left() + column as f32 * (CELL + GAP);
    let y1 = top - row_from_top as f32 * (CELL + GAP) - extra;
    Rect::new(x0, y1 - CELL, x0 + CELL, y1)
}

/// Which slot of which grid a point is over, if any.
pub fn slot_at(cursor: (f32, f32)) -> Option<(Side, usize)> {
    [Side::Chest, Side::Pack].into_iter().find_map(|side| {
        (0..SLOTS)
            .find(|&slot| slot_rect(side, slot).contains(cursor.0, cursor.1))
            .map(|slot| (side, slot))
    })
}

/// Whether a point is anywhere on the screen at all, so a click beside
/// the grids is not also a swing at the world behind it.
#[allow(dead_code)]
pub fn contains(cursor: (f32, f32)) -> bool {
    panel_rect().contains(cursor.0, cursor.1)
}

#[cfg(test)]
mod bulk_tests {
    use super::*;
    use primitive_shared::types::{BLOCK_CHEST, BLOCK_STONE};

    fn opened_at(cursor: (f32, f32)) -> ChestScreen {
        let mut screen = ChestScreen::new();
        screen.show((0, 0, 0), Inventory::new(), Some(BLOCK_CHEST));
        screen.set_cursor(Some(cursor));
        screen
    }

    #[test]
    fn the_two_buttons_move_things_in_opposite_directions() {
        let pack = {
            let mut inventory = Inventory::new();
            inventory.add(BLOCK_STONE, 12);
            inventory
        };
        for to_chest in [true, false] {
            let rect = bulk_button_rect(to_chest);
            let mut screen = opened_at((rect.centre_x(), rect.centre_y()));
            assert_eq!(
                screen.click(&pack, Button::Left, false),
                Some(Intent::BulkMove { to_chest })
            );
        }
    }

    #[test]
    fn the_buttons_sit_between_the_grids_and_on_no_slot() {
        // They are hit-tested before the slots, so a button over a slot
        // would make that slot unreachable.
        for to_chest in [true, false] {
            let rect = bulk_button_rect(to_chest);
            for side in [Side::Chest, Side::Pack] {
                for slot in 0..SLOTS {
                    let s = slot_rect(side, slot);
                    let overlaps =
                        s.x0 < rect.x1 && s.x1 > rect.x0 && s.y0 < rect.y1 && s.y1 > rect.y0;
                    assert!(!overlaps, "a bulk button covers {side:?} slot {slot}");
                }
            }
            let panel = panel_rect();
            assert!(
                rect.x0 >= panel.x0 && rect.x1 <= panel.x1,
                "a bulk button hangs off the panel"
            );
            assert!(rect.y0 >= panel.y0 && rect.y1 <= panel.y1);
        }
    }

    #[test]
    fn a_bulk_click_puts_down_whatever_was_in_hand() {
        // Two gestures at once is one gesture too many: the held stack
        // is the one the player is thinking about, and sweeping it into
        // the chest with everything else is not what they asked for.
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 4);
        let first = slot_rect(Side::Pack, 0);
        let mut screen = opened_at((first.centre_x(), first.centre_y()));
        screen.click(&pack, Button::Left, false);
        assert!(screen.held().is_some(), "nothing was picked up to begin with");

        let button = bulk_button_rect(true);
        screen.set_cursor(Some((button.centre_x(), button.centre_y())));
        assert_eq!(
            screen.click(&pack, Button::Left, false),
            Some(Intent::BulkMove { to_chest: true })
        );
        assert_eq!(screen.held(), None, "the held stack was dragged along");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_BACKPACK, BLOCK_CHEST, BLOCK_DIRT, BLOCK_STONE};

    const AT: (i32, i32, i32) = (4, 30, -7);

    fn stocked() -> Inventory {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 12);
        inventory
    }

    fn opened() -> ChestScreen {
        let mut screen = ChestScreen::new();
        let mut chest = Inventory::new();
        chest.add(BLOCK_DIRT, 30);
        screen.show(AT, chest, Some(BLOCK_CHEST));
        screen
    }

    fn centre_of(side: Side, slot: usize) -> (f32, f32) {
        let r = slot_rect(side, slot);
        (r.centre_x(), r.centre_y())
    }

    #[test]
    fn a_shut_screen_draws_nothing_and_holds_nothing() {
        let screen = ChestScreen::new();
        assert!(!screen.is_open());
        assert_eq!(screen.at(), None);
        assert!(screen
            .build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &stocked(),
                Language::English,
            )
            .is_empty());
    }

    #[test]
    fn opening_shows_what_the_server_said_is_in_it() {
        let screen = opened();
        assert!(screen.is_open());
        assert_eq!(screen.at(), Some(AT));
        assert!(!screen
            .build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &stocked(),
                Language::English,
            )
            .is_empty());
    }

    #[test]
    fn the_two_grids_never_share_a_cell() {
        // A slot two grids claim is a slot the player cannot aim at:
        // whichever one the hit test happens to find first wins, and it
        // is not the one they clicked.
        for slot in 0..SLOTS {
            for other in 0..SLOTS {
                assert_ne!(
                    slot_rect(Side::Chest, slot),
                    slot_rect(Side::Pack, other),
                    "chest slot {slot} sits on pack slot {other}"
                );
            }
        }
        for a in 0..SLOTS {
            for b in (a + 1)..SLOTS {
                assert_ne!(slot_rect(Side::Chest, a), slot_rect(Side::Chest, b));
            }
        }
    }

    #[test]
    fn the_hit_test_finds_the_slot_that_was_drawn() {
        for side in [Side::Chest, Side::Pack] {
            for slot in 0..SLOTS {
                assert_eq!(
                    slot_at(centre_of(side, slot)),
                    Some((side, slot)),
                    "{side:?} slot {slot}"
                );
            }
        }
        // ...and nothing at all in the gap between the two grids.
        let between = (0.0, slot_rect(Side::Pack, SLOTS - 1).y1 + GRID_SPLIT / 2.0);
        assert_eq!(slot_at(between), None);
    }

    #[test]
    fn nothing_the_screen_draws_escapes_its_panel() {
        // The panel is exactly as wide as ten slots, so it is the text
        // that overruns it -- a readout or a line of hints one word too
        // long ends up lying on the world beside the screen, which is
        // the one mistake here that looks like a bug rather than a
        // layout choice.
        let screen = opened();
        let mut pack = stocked();
        pack.add(BLOCK_DIRT, 128);
        let vertices = screen.build(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &pack,
            Language::English,
        );
        let panel = panel_rect();
        // Borders are drawn outside what they frame, and the panel's
        // shadow is further out still; neither is content.
        let slack = 0.02;
        for v in &vertices {
            let [x, y] = v.position;
            if x.abs() > 4.0 {
                continue; // the scrim, which covers the whole screen
            }
            assert!(
                x >= panel.x0 - slack && x <= panel.x1 + slack,
                "something is drawn at x={x}, outside a panel of {}..{}",
                panel.x0,
                panel.x1
            );
            assert!(
                y >= panel.y0 - slack && y <= panel.y1 + slack,
                "something is drawn at y={y}, outside a panel of {}..{}",
                panel.y0,
                panel.y1
            );
        }
    }

    #[test]
    fn the_whole_screen_fits_a_square_window() {
        // Authored as if the window were square, and the shader divides
        // x by the aspect -- so a square window is the worst case.
        let panel = panel_rect();
        assert!(panel.x0 > -1.0 && panel.x1 < 1.0, "runs off the sides");
        assert!(panel.y0 > -1.0 && panel.y1 < 1.0, "runs off the top or bottom");
        for side in [Side::Chest, Side::Pack] {
            for slot in 0..SLOTS {
                let cell = slot_rect(side, slot);
                assert!(cell.x0 >= panel.x0 && cell.x1 <= panel.x1, "a slot left the panel");
                assert!(cell.y0 >= panel.y0 && cell.y1 <= panel.y1, "a slot left the panel");
            }
        }
    }

    #[test]
    fn a_click_picks_up_and_a_second_one_moves_it() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        assert_eq!(screen.click(&pack, Button::Left, false), None, "the pick-up moved something");
        assert_eq!(screen.held(), Some((Side::Chest, 0)));

        screen.set_cursor(Some(centre_of(Side::Pack, 3)));
        assert_eq!(
            screen.click(&pack, Button::Left, false),
            Some(Intent::Move {
                from: (Side::Chest, 0),
                to: (Side::Pack, 3),
                half: false,
            })
        );
        assert_eq!(screen.held(), None, "the stack stayed in hand");
    }

    #[test]
    fn a_right_click_places_half_and_keeps_the_rest() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false);
        screen.set_cursor(Some(centre_of(Side::Chest, 5)));
        assert_eq!(
            screen.click(&pack, Button::Right, false),
            Some(Intent::Move {
                from: (Side::Chest, 0),
                to: (Side::Chest, 5),
                half: true,
            })
        );
        assert_eq!(screen.held(), Some((Side::Chest, 0)), "the rest was dropped");
    }

    #[test]
    fn shift_click_sends_a_stack_straight_across() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Pack, 0)));
        assert_eq!(
            screen.click(&pack, Button::Left, true),
            Some(Intent::QuickMove(Side::Pack, 0))
        );
        assert_eq!(screen.held(), None, "a quick move picked something up as well");
    }

    #[test]
    fn picking_up_an_empty_slot_is_not_a_gesture() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 30)));
        assert_eq!(screen.click(&pack, Button::Left, false), None);
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn clicking_outside_the_grids_cancels_rather_than_losing_it() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false);
        assert!(screen.held().is_some());
        screen.set_cursor(Some((0.0, 0.95)));
        assert_eq!(screen.click(&pack, Button::Left, false), None);
        assert_eq!(screen.held(), None, "the pick-up survived a click on nothing");
    }

    #[test]
    fn a_pick_up_is_dropped_when_the_slot_changes_underneath_it() {
        // The chest is the one place another player can change a slot
        // between the two halves of a gesture. Completing it then sends
        // whatever happens to be there now, which is how someone throws
        // away something they never touched.
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false);
        assert!(screen.held().is_some());

        let mut emptied = Inventory::new();
        emptied.add(BLOCK_STONE, 1);
        screen.show(AT, emptied, None);
        assert_eq!(screen.held(), None, "the gesture survived the chest changing");
    }

    #[test]
    fn opening_a_different_chest_starts_over() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false);
        screen.show((99, 40, 99), Inventory::new(), Some(BLOCK_CHEST));
        assert_eq!(screen.at(), Some((99, 40, 99)));
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn closing_forgets_the_chest_and_what_was_in_it() {
        let mut screen = opened();
        screen.close();
        assert!(!screen.is_open());
        assert_eq!(screen.at(), None);
        assert!(screen
            .build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &stocked(),
                Language::English,
            )
            .is_empty());
    }

    /// Draws the screen into a PNG, for looking at.
    ///
    /// Layout is the one thing here no assertion settles: labels that
    /// collide, a readout that runs into the grid beside it and a panel
    /// with a hole in the middle of it all pass every test in this file
    /// and are obvious in a picture.
    ///
    /// ```text
    /// cargo test -p primitive_client --bins -- --ignored --nocapture dump_the_chest
    /// ```
    #[test]
    #[ignore = "diagnostic: writes a picture of the screen"]
    fn dump_the_chest_to_a_png() {
        use primitive_shared::types::{BLOCK_COBBLESTONE, BLOCK_LOG, BLOCK_SAND};

        let mut chest = Inventory::new();
        chest.add(BLOCK_COBBLESTONE, 200);
        chest.add(BLOCK_LOG, 12);
        chest.add(BLOCK_SAND, 64);
        let mut screen = ChestScreen::new();
        screen.show(AT, chest, Some(BLOCK_CHEST));

        let mut pack = stocked();
        pack.add(BLOCK_DIRT, 30);
        screen.set_cursor(Some(centre_of(Side::Chest, 1)));

        let vertices = screen.build(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &pack,
            Language::English,
        );
        let path = std::env::var("PRIMITIVE_UI_DUMP")
            .unwrap_or_else(|_| "target/chest_screen.png".to_string());
        widgets::dump_to_png(&vertices, 1600, 900, &path);
        println!("wrote {path}");
    }

    #[test]
    fn a_dead_players_pack_is_not_called_a_chest() {
        // The two blocks share every gesture and every pixel of layout,
        // so the heading is the only thing that tells a player whether
        // they are looking at somewhere they put things or somewhere
        // they lost them.
        let mut screen = ChestScreen::new();
        screen.show(AT, Inventory::new(), Some(BLOCK_BACKPACK));
        assert_eq!(screen.heading(), Msg::Backpack);

        // An update with no block -- the chunk has not arrived, or this
        // is the answer to a gesture rather than to an open -- must not
        // quietly rename it.
        screen.show(AT, Inventory::new(), None);
        assert_eq!(screen.heading(), Msg::Backpack, "an update renamed the screen");

        // ...and moving to a real chest goes back to the other word.
        screen.show((1, 2, 3), Inventory::new(), Some(BLOCK_CHEST));
        assert_eq!(screen.heading(), Msg::Chest);
        screen.close();
        assert_eq!(screen.heading(), Msg::Chest, "a shut screen kept the last word");
    }

    #[test]
    fn every_heading_fits_the_panel_in_every_language() {
        // The heading is drawn left-aligned into a band exactly as wide
        // as the panel, and the panel is exactly ten slots wide. A word
        // one letter too long in one language runs off the side of the
        // screen, and only that language ever sees it.
        for block in [BLOCK_CHEST, BLOCK_BACKPACK] {
            for &language in Language::ALL {
                let mut screen = ChestScreen::new();
                screen.show(AT, Inventory::new(), Some(block));
                let vertices = screen.build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &stocked(),
                    language,
                );
                let panel = panel_rect();
                for v in &vertices {
                    let x = v.position[0];
                    if x.abs() > 4.0 {
                        continue; // the scrim
                    }
                    assert!(
                        x <= panel.x1 + 0.02,
                        "{language:?} draws at x={x}, past a panel edge of {}",
                        panel.x1
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_before_the_mouse_has_moved_does_nothing() {
        // The cursor is only known once it has moved over the window.
        let mut screen = ChestScreen::new();
        screen.show(AT, Inventory::new(), Some(BLOCK_CHEST));
        screen.set_cursor(None);
        assert_eq!(screen.click(&stocked(), Button::Left, false), None);
    }
}
