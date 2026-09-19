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
use primitive_shared::inventory::CHEST_SLOTS;
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{Language, Msg};
use crate::ui::inventory_screen::{
    draw_slot_stack, held_stack, Button, SlotEdge, CELL, GAP, HOTBAR_SPLIT, PANEL_PAD,
    TRAY_PAD,
};
use crate::ui::widgets::{self, Painter, Rect};

/// How many squares a side has.
///
/// **The two sides stopped being the same shape.** A chest used to hold
/// exactly what a player can carry, which made one grid drawn twice; the
/// pack was then halved (`inventory::STORAGE_ROWS`) and the chest stayed
/// at forty, deliberately -- see `inventory::CHEST_SLOTS`, which gives
/// both reasons. So the grid is still drawn twice, but it is asked how
/// tall it is each time instead of being told once.
fn side_slots(side: Side) -> usize {
    match side {
        Side::Chest => CHEST_SLOTS,
        Side::Pack => SLOTS,
    }
}

/// Rows in one side's grid.
fn rows(side: Side) -> usize {
    side_slots(side) / HOTBAR_SLOTS
}
/// Space between the chest's grid and the player's.
///
/// **Derived from what lives in it, which is now a bar.** It was 0.135,
/// a number measured by eye for two small buttons and a caption, and the
/// caption and the buttons kept colliding as one or the other changed --
/// at 0.070 "CARRIED" was struck through by TAKE ALL. What is between
/// the grids now is the action bar (see [`action_bar_rect`]) with the
/// caption under it, so this is that bar's height plus the air around
/// it, and there is nothing left to guess.
///
/// The `- (HOTBAR_SPLIT + LABEL_HEIGHT)` is not a fudge: `grid_height`
/// counts both of those, and on the chest's side of the screen neither
/// is drawn -- a chest has no belt and its caption is above it, not in
/// the gap -- so that much of the gap comes free and has to be taken off
/// or it is paid for twice.
fn grid_split() -> f32 {
    let room = 0.014 + bar_height() + 0.014 + TRAY_PAD + LABEL_HEIGHT;
    room - (HOTBAR_SPLIT + LABEL_HEIGHT)
}

/// The gap a hearth's or a rack's half leaves above the pack.
///
/// **Not [`grid_split`], which it used to be.** Those two screens draw
/// no action bar, so sizing their gap for one left a hand's breadth of
/// empty stone between the fire and the word CARRIED -- and "there is
/// nothing there" is what a player reads as "I have not found the thing
/// yet". It is air between two groups and nothing else, so it is the
/// size of air.
const HALF_GAP: f32 = 0.060;
/// Room for each grid's label, above it. A hair more than the text is
/// tall, so a label never touches either the row under it or whatever
/// is over it.
const LABEL_HEIGHT: f32 = 0.058;
/// The side of the way out, before the touch floor is applied.
const CLOSE_SIDE: f32 = 0.066;

/// Room for the title, above the top label.
///
/// The title is drawn at 1.05 scale -- about 0.049 tall -- and a band
/// of 0.058 left four thousandths of clearance, so CHEST and STORED
/// read as one smudged line. Room for the text plus half its height
/// again.
///
/// **It used to have to hold the way out as well**, floored to a
/// finger, which on a phone made the band nearly twice as tall as the
/// words in it. The `X` is gone -- see the note on [`Intent::Close`] --
/// so the band is back to being what it is called: room for a line of
/// text. Every hundredth that buys comes back as a bigger slot, which
/// is the arithmetic spelled out on [`FOOTER_HEIGHT`].
fn header_height() -> f32 {
    widgets::tappable_when(CLOSE_SIDE, false) + 0.016
}

/// Room under the lower grid for the readout and one line of hint.
///
/// **It was 0.215 and held three lines.** The pack screen had already
/// worked out what that costs -- "they cost a third of the panel's
/// height to say what a player reads once", see
/// `inventory_screen::FOOTER_HEIGHT` -- and threw its own hints out; the
/// chest kept two, one of which said that a left click takes and places
/// and a right click halves, which is true of every slot in every screen
/// in the game and of most games this one resembles. What is left is the
/// line that says the thing only *this* screen does: a shift click sends
/// a stack across the seam.
///
/// The height that buys is not decoration. A chest is capped by the
/// window's height on a phone held sideways -- `Layout::fit` on a
/// 1220-tall screen was returning 1.19 against a setting of 1.65 -- so
/// every hundredth taken out of the footer comes back as a bigger slot.
const FOOTER_HEIGHT: f32 = 0.155;

/// How much fuel counts as a full flame on the gauge.
///
/// Two minutes. A fire can hold twenty (see the fire map's cap), and a
/// bar that showed that honestly would sit at a twentieth for the whole
/// of an ordinary evening. What the gauge is for is "will this last the
/// batch", and two minutes is several batches.
const FULL_FLAME_SECONDS: f32 = 120.0;
/// The empty half of a gauge, the fire in a full one, and the arrow.
///
/// The gauges are wells like the slots -- the same hole cut in the same
/// stone -- with a colour poured into them. A bar with its own frame in
/// its own palette is how an interface ends up looking like five
/// interfaces.
///
/// **The well's own colour, which the sentence above always said and the
/// constant did not.** It was a flat mid-grey, written when the stone was
/// pale and a grey hole in it read as a hole. On the dark stone the same
/// grey is the lightest surface on the panel, so an *empty* flame bar
/// looked lit and a full one looked like two colours of the same thing --
/// the one question a gauge answers, answered backwards.
const GAUGE_BACK: [f32; 4] = widgets::WELL;
const FLAME: [f32; 4] = [0.90, 0.45, 0.10, 1.0];
/// What a dry afternoon looks like on a gauge. Paler and yellower than
/// the flame beside it, because the two mean different things -- sun and
/// smoke -- and a rack beside a fire shows the flame colour instead.
const SUN: [f32; 4] = [0.93, 0.80, 0.35, 1.0];
const PROGRESS: [f32; 4] = [0.93, 0.93, 0.93, 1.0];

const HINT_SCALE: f32 = 0.68;
/// The one line of instruction the screen still prints.
///
/// Kept short on purpose: the panel is exactly as wide as ten slots,
/// and a line of prose that runs past that runs out of the screen.
///
/// **One line, and it used to be two.** The other one said that a left
/// click takes and places and a right click halves -- which is true of
/// the pack screen, of the hearth, of the rack, and of every game of
/// this kind the player has met before. Permanent text that says what
/// the reader already knows is the cheapest thing on a screen to
/// remove and the dearest to keep: it was 0.040 of panel height, and on
/// a phone the panel's height is the thing the slots are competing for.
///
/// What is left is the gesture only *this* screen has: sending a stack
/// straight across the seam between two inventories. It names a modifier
/// key on a desktop and a gesture on glass; see `lang::by_input`.
fn hint() -> Msg {
    crate::ui::lang::by_input(Msg::ChestHint2, Msg::ChestHint2Touch)
}

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
    /// Every stack of what is in this slot, across: the ctrl-click.
    ///
    /// **"Сделай сочетания клавиш для работы в хранилищах."** Unloading a
    /// day's cobble into a chest was a shift-click per stack, and a pack
    /// is mostly a handful of kinds in many stacks. See
    /// `ClientMessage::ChestMoveKind`.
    MoveKind(Side, usize),
    /// Fold the container's part-stacks together.
    ///
    /// A chest is the one place a player ends up with nine part-stacks
    /// of the same thing, and it was the one place with no way to tidy
    /// them: the button existed for the pack and stopped at the seam.
    Sort,
    /// Everything that fits, in one gesture. True to fill the chest.
    ///
    /// The one thing every storage screen has and this one did not:
    /// emptying a full pack into a chest was forty separate
    /// shift-clicks, and the player counted them.
    BulkMove { to_chest: bool },
    /// Put the screen away.
    ///
    /// **There is no button for this any more.** The header band used
    /// to carry an `X`, and it was the smallest control on the screen
    /// on the device most likely to need it: 48 device pixels against a
    /// 91-pixel finger. It was also the fourth way of saying the same
    /// thing -- Escape, the inventory key, and a tap anywhere off the
    /// panel all close a container, and on a phone Escape is the
    /// system's own Back gesture.
    ///
    /// What its going costs is one tap in one case, and it is worth
    /// writing down: a tap beside the panel with a stack in hand puts
    /// the stack back and deliberately does *not* close, because a near
    /// miss on a slot must not both leave the screen and appear to
    /// swallow what was being carried. So leaving with something in
    /// hand is down-then-out, or Back, which closes whatever is held.
    /// See `tapping_beside_a_container_closes_it_but_only_with_empty_hands`.
    Close,
}

/// Which of the three container screens is being drawn.
///
/// The *shape* rather than the name: two hearths of different kinds draw
/// the same screen, and what this decides is where the slots are. See
/// `primitive_shared::protocol::ContainerKind` for the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Forty slots above forty slots.
    Chest,
    /// Ingredients, fuel and results, with a fire under them.
    Hearth,
    /// A frame and a tray, with the weather over them.
    Rack,
    /// One slot of loose goods in a jug, and what it holds of its measure.
    Vessel,
    /// A dead player's body with a rucksack's compartment: the chest's
    /// screen, with two tabs over its grid -- the body's forty, and the
    /// rucksack's twenty (`inventory::CORPSE_COMPARTMENT`). `rucksack` is
    /// which of the two is showing.
    ///
    /// **Tabs, not a third grid.** Two more rows under the body's four is
    /// the obvious drawing, and on a phone held sideways it is the expensive
    /// one: the chest is capped by the window's height there (see
    /// `FOOTER_HEIGHT`), so two rows more is every slot on the screen a
    /// sixth smaller -- the pack's too, for the whole visit, to show twenty
    /// squares a player looks at once. A tab strip is a finger tall and
    /// costs about half of that; and it is the shape the pack screen already
    /// gives a worn rucksack (`inventory_screen::Tab::Backpack`), so a
    /// player meets their rucksack on their body the way they met it on
    /// their back. The compartment is laid out as it is there, too: its
    /// twenty in the two rows under the tabs, in reading order.
    ///
    /// Not a `ContainerKind`, because the server does not say it and does
    /// not need to: a body is a chest to the protocol, and a body with a
    /// compartment is one whose contents are that long. See
    /// [`ChestScreen::layout`].
    Body { rucksack: bool },
    /// A horse's saddlebags: the chest's grid, of which only the first
    /// `horse::BAGS_SLOTS` squares are there.
    ///
    /// **The chest's screen with squares missing, not a screen of its own.**
    /// Everything a player does at bags is what they do at a chest -- drag,
    /// shift-click, take all -- and a second screen would be a second set of
    /// gestures to learn for twelve squares. What is not drawn is not hit
    /// (`chest_square`, one function for both), so a square the server
    /// would refuse (`Roles::Bags`) is a square nobody can click.
    Bags,
}

impl Layout {
    /// Whether this is one of the two layouts that are a grid over the
    /// pack -- a chest or a body -- with the action bar between them.
    pub fn grid(self) -> bool {
        matches!(self, Layout::Chest | Layout::Body { .. } | Layout::Bags)
    }

    pub fn of(kind: primitive_shared::protocol::ContainerKind) -> Layout {
        use primitive_shared::protocol::ContainerKind;
        match kind {
            ContainerKind::Chest => Layout::Chest,
            ContainerKind::Hearth(_) => Layout::Hearth,
            ContainerKind::Rack => Layout::Rack,
            ContainerKind::Vessel => Layout::Vessel,
            ContainerKind::Saddlebags => Layout::Bags,
        }
    }
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
    /// What is open: a chest, or one of the three hearths. Decides
    /// which layout is drawn and which slots a click can land on.
    kind: primitive_shared::protocol::ContainerKind,
    /// The fire, for a hearth. `None` for a chest.
    fire: Option<primitive_shared::protocol::HearthState>,
    /// The weather, for a drying rack, on the same terms.
    weather: Option<primitive_shared::protocol::RackState>,
    /// The block this screen belongs to, for its title icon. `None`
    /// until the chunk it is in has arrived, which is a fraction of a
    /// second in which the screen has a word and no picture.
    block: Option<primitive_shared::types::BlockId>,
    /// What to call the thing being looked into.
    ///
    /// Decided from the block in the world rather than from anything the
    /// server said, because the server never says: every message about a
    /// container names a side and a slot, deliberately, so the only
    /// thing the client is told is *where* it is. The block at that cell
    /// is a lookup the client already has.
    heading: Msg,
    /// The pack slot of the jug being looked into, when the jug is in the
    /// hand rather than on a table.
    ///
    /// **A second way to be open, and no server state behind it.** A jug
    /// in the pack keeps its contents in its own slot (see
    /// `inventory::jug_contents`), and the pack is already sent whole on
    /// every change -- so the screen reads the jug straight out of the
    /// snapshot the rest of the interface draws from. The rejected version
    /// had the server open a "held container" and send its contents as a
    /// `ChestState` of their own: a second copy of one fact, two messages
    /// that could disagree about how much grain is in the jug, and a
    /// position to invent for a thing that is not anywhere.
    vessel_slot: Option<usize>,
    /// The container the player shut themselves, until they ask for one
    /// again.
    ///
    /// **An update already on its way is not an answer to anything.** The
    /// server sends `ChestState` to whoever stands at a container every
    /// time it changes -- a warming hearth on every tick its heat moves, a
    /// rack every two seconds -- and it is the same message that opens this
    /// screen. Between Escape and the server reading the `CloseChest` that
    /// follows it there is a round trip, and a burning fire fills it: the
    /// update already in the socket landed, the screen the player had just
    /// shut came back under their hand, took the cursor off the world again
    /// and played the lid a second time.
    ///
    /// Only `OpenChest` makes a server start sending, so a player who has
    /// not asked since shutting this one is owed nothing about it. Cleared
    /// by [`ChestScreen::asked_to_open`]. The rejected fix was a timer --
    /// ignore updates for half a second -- which is a guess at somebody
    /// else's ping.
    dismissed: Option<(i32, i32, i32)>,
    /// Whether a body's rucksack tab is the one showing. Meaningless for
    /// anything without a compartment, and put back to the body's own
    /// forty whenever a different container opens -- a player opening a
    /// body is looking for the body's things first.
    rucksack_page: bool,
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
            kind: primitive_shared::protocol::ContainerKind::Chest,
            fire: None,
            weather: None,
            block: None,
            held: None,
            held_block: None,
            cursor: None,
            heading: Msg::Chest,
            vessel_slot: None,
            dismissed: None,
            rucksack_page: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.at.is_some() || self.vessel_slot.is_some()
    }

    /// Opens the jug in pack slot `slot`, the one in the hand.
    ///
    /// Nothing is asked of the server: the jug and what is in it are
    /// already in `pack`. A slot that does not hold a jug opens nothing,
    /// so a stale click cannot open an axe.
    pub fn show_held_vessel(&mut self, slot: usize, pack: &Inventory) {
        let Some(stack) = pack.slots().get(slot).copied().flatten() else {
            return;
        };
        if !primitive_shared::types::opens_as_vessel(stack.block) {
            return;
        }
        self.close();
        self.vessel_slot = Some(slot);
        self.kind = primitive_shared::protocol::ContainerKind::Vessel;
        self.block = Some(stack.block);
        self.heading = Msg::Jug;
        self.cursor = Some((0.0, 0.0));
        self.read_held_vessel(pack);
    }

    /// The pack slot of the jug open in the hand, if that is what is open.
    pub fn held_vessel(&self) -> Option<usize> {
        self.vessel_slot
    }

    /// Refills the screen's one slot from the jug in the pack, or shuts
    /// the screen if the jug has gone -- thrown, dropped, eaten by a
    /// death. Called with every pack snapshot.
    ///
    /// Shut rather than followed: the jug's new slot is not in the
    /// snapshot as a fact, only as a guess among however many jugs the
    /// player carries, and a screen that guessed would be pouring into
    /// somebody else's jug.
    fn read_held_vessel(&mut self, pack: &Inventory) {
        let Some(slot) = self.vessel_slot else {
            return;
        };
        let jug = pack
            .slots()
            .get(slot)
            .copied()
            .flatten()
            .filter(|stack| primitive_shared::types::opens_as_vessel(stack.block));
        let Some(jug) = jug else {
            self.close();
            return;
        };
        let mut contents = Inventory::new();
        if let Some((goods, count)) = primitive_shared::inventory::jug_contents(&jug) {
            contents.put_in_slot(
                primitive_shared::inventory::VESSEL_SLOT,
                primitive_shared::inventory::Stack::new(goods, count),
            );
        }
        self.contents = contents;
        self.sync();
    }

    /// Which chest is open.
    ///
    /// The messages that act on a chest name a side and a slot rather than
    /// a position, precisely so that the client cannot reach into one it
    /// did not open. What does ask is the lid's sound: only a chest has one
    /// (a hearth or a kiln opens this same screen).
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
        kind: primitive_shared::protocol::ContainerKind,
        fire: Option<primitive_shared::protocol::HearthState>,
        weather: Option<primitive_shared::protocol::RackState>,
    ) {
        // Whatever the server has opened replaces a jug looked into in
        // the hand: one screen, and the server's answer is the newer ask.
        self.vessel_slot = None;
        self.kind = kind;
        self.fire = fire;
        self.weather = weather;
        if self.at != Some(at) {
            // A different chest: nothing carried over from the last one.
            self.release();
            self.cursor = Some((0.0, 0.0));
            self.heading = Msg::Chest;
            self.rucksack_page = false;
        }
        if let Some(block) = block {
            self.heading = heading_for(block);
            self.block = Some(block);
        }
        self.at = Some(at);
        self.contents = contents;
        self.sync();
    }

    pub fn close(&mut self) {
        self.at = None;
        self.vessel_slot = None;
        self.contents = Inventory::new();
        self.kind = primitive_shared::protocol::ContainerKind::Chest;
        self.fire = None;
        self.weather = None;
        self.block = None;
        self.heading = Msg::Chest;
        self.rucksack_page = false;
        self.release();
    }

    /// The player shut the screen, as opposed to the server shutting it.
    /// Remembers which container it was, so that an update for it already
    /// in the socket cannot open it again -- see `dismissed`.
    pub fn close_by_player(&mut self) {
        self.dismissed = self.at;
        self.close();
    }

    /// The player has asked the server to open a container, so whatever it
    /// answers is wanted -- the one they shut a moment ago included.
    pub fn asked_to_open(&mut self) {
        self.dismissed = None;
    }

    /// Whether a `ChestState` for the container at `at` may be shown.
    pub fn wants_state_for(&self, at: (i32, i32, i32)) -> bool {
        self.dismissed != Some(at)
    }

    /// Which of the three screens is up.
    ///
    /// Asked by everything that has to know: the hit test, the click
    /// handler, and the drawing below. A method rather than the boolean
    /// it was, because "not a hearth" stopped meaning "a chest" the
    /// moment a rack could be open -- and a boolean that had grown a
    /// third value would have handed a click on a rack's frame to
    /// whatever a chest has in that corner.
    pub fn layout(&self) -> Layout {
        match Layout::of(self.kind) {
            // Read off the snapshot's length, the one place the fact is:
            // the server sends a body's compartment as squares past forty
            // and says nothing else about it.
            Layout::Chest if self.contents.has_compartment() => Layout::Body {
                rucksack: self.rucksack_page,
            },
            layout => layout,
        }
    }

    /// Which heading this screen is wearing. For the tests, which assert
    /// on the word rather than on the pixels it turns into.
    #[allow(dead_code)]
    pub fn heading(&self) -> Msg {
        self.heading
    }

    /// How much of the screen this container is taking right now.
    ///
    /// The frame loop grows the screen by what fits this, and
    /// `place_cursor` divides a click by the same number -- so it has to
    /// be the shape actually being drawn, which is the shape this
    /// container's kind asks for. See `extent_for`.
    pub fn extent(&self) -> (f32, f32) {
        extent_for(self.layout())
    }

    /// How much bigger than its authored size this screen is drawn.
    ///
    /// One definition, used to draw it and to hit-test it -- see
    /// `inventory_screen::grow_by`, which this is the other half of: the
    /// two screens are the same grid of the same cells, drawn at the
    /// same size, and two answers here would be two screens.
    pub fn grow_by(&self, layout: crate::ui::widgets::Layout) -> f32 {
        layout.fit(self.extent())
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
        // A jug in the hand *is* a pack slot, so every pack snapshot is
        // also a snapshot of what is in it -- read before the pick-up is
        // checked, because the pick-up may be the jug's own slot.
        self.read_held_vessel(pack);
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

    /// Handles a click. `quick` is the shift modifier, `every` the ctrl.
    pub fn click(&mut self, pack: &Inventory, button: Button, quick: bool, every: bool) -> Option<Intent> {
        // **The bulk buttons are hit-tested only where they are drawn.**
        // They are a chest's, and the test used to be unconditional: at
        // a hearth or a rack -- neither of which draws them -- the two
        // rectangles were still live, sitting in the empty band above
        // the pack, and a click on nothing at all emptied your pack into
        // the fire. An invisible button is worse than a missing one.
        if let (Some(cursor), true) = (self.cursor, self.layout().grid()) {
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
        let layout = self.layout();
        // Before the slots and before the held stack: a way out that
        // only works with empty hands is not a way out.
        //
        // Two of them, for the same reason the pack has two: the `X` is
        // what a player can see and a tap off the panel is what a thumb
        // can hit. A stack in hand is put back first and the screen
        // stays open -- a near miss on a slot must not both close the
        // chest and appear to swallow what was being carried.
        let panel = panel_rect(layout);
        if !panel.contains(cursor.0, cursor.1) {
            if self.held().is_some() {
                self.release();
                return None;
            }
            return Some(Intent::Close);
        }
        // A body's two tabs. Only the page changes, so nothing goes to the
        // server, and **a stack in hand stays in hand**: turning the page
        // with something held is how a thing is carried between the body's
        // forty and its rucksack, so dropping the pick-up here would make
        // the one move the tabs exist for impossible.
        if let Layout::Body { .. } = layout {
            if let Some(rucksack) = page_tab_at(cursor) {
                self.rucksack_page = rucksack;
                return None;
            }
        }
        if layout.grid()
            && sort_button_rect().contains(cursor.0, cursor.1)
        {
            self.release();
            return Some(Intent::Sort);
        }
        let Some(target) = slot_at(cursor, layout) else {
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
                if every {
                    return Some(Intent::MoveKind(target.0, target.1));
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
        self.cursor.and_then(|cursor| slot_at(cursor, self.layout()))
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
        // The page as well as the squares: turning it changes no slot.
        self.rucksack_page.hash(&mut h);
        self.held
            .map(|(side, slot)| (side == Side::Chest, slot))
            .hash(&mut h);
        self.cursor
            .map(|(x, y)| (x.to_bits(), y.to_bits()))
            .hash(&mut h);
        for slot in self.contents.slots() {
            slot.map(|stack| (stack.block, stack.count)).hash(&mut h);
        }
        // **What the container is doing, as well as what is in it.**
        // Left out, both gauges were frozen pictures: a hearth's fuel
        // burns down and a rack's skin cures without a single slot
        // changing, so a key made of the contents alone says "nothing
        // has happened" for the whole twelve minutes a rack is working.
        // The server only sends these when they change, so hashing them
        // costs nothing and rebuilds exactly when one arrives.
        // The heat is in it for the same reason: a fire climbing towards
        // copper changes nothing in any slot for half a minute, and a key
        // without the temperature would draw the gauge where it started.
        self.fire
            .map(|fire| {
                (
                    fire.fuel_left.to_bits(),
                    fire.progress.to_bits(),
                    fire.degrees.to_bits(),
                    fire.needs.to_bits(),
                    fire.wet,
                )
            })
            .hash(&mut h);
        self.weather
            .map(|w| (w.progress.to_bits(), w.rate.to_bits(), w.wet, w.near_fire))
            .hash(&mut h);
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

    /// The hearth half of the screen: eight slots with roles, a flame,
    /// the heat, and an arrow.
    ///
    /// Drawn rather than laid out as a grid because the shape is the
    /// explanation -- ingredients along the top, fire under them,
    /// results off to the right with an arrow filling up towards them.
    /// A player who has seen one furnace in one game knows what this is
    /// without a word of text, and the words are there anyway.
    fn draw_hearth(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        hovered: Option<(Side, usize)>,
        language: Language,
    ) {
        use primitive_shared::hearth::{
            Glow, ASH_SLOT, FUEL_SLOT, INPUT_SLOTS, OUTPUT_SLOTS, USED_SLOTS,
        };

        let fire = self.fire.unwrap_or(primitive_shared::protocol::HearthState {
            fuel_left: 0.0,
            progress: 0.0,
            degrees: 0.0,
            needs: 0.0,
            wet: false,
        });

        // The three labels, each over its own group and clear of the
        // slots. They used to be drawn a hair above the cell, which at
        // this size means *on* it -- the first version had "GOES IN"
        // written across the first ingredient.
        let inputs = hearth_slot_rect(INPUT_SLOTS.start).expect("input slots");
        let outputs = hearth_slot_rect(OUTPUT_SLOTS.start).expect("output slots");
        let fuel = hearth_slot_rect(FUEL_SLOT).expect("a fuel slot");
        let ash = hearth_slot_rect(ASH_SLOT).expect("an ash slot");
        // In the quiet ink, like CARRIED under them: they name groups of
        // slots, and in full ink they were the same step as the readings.
        // The results' two from the right -- see `caption_over`.
        let room = (outputs.x1 - inputs.x0) / 2.0 - 0.01;
        caption_over(p, inputs, language.text(Msg::HearthInputs), false, room);
        caption_over(p, outputs, language.text(Msg::HearthOutput), true, room);
        caption_over(p, ash, language.text(Msg::HearthAsh), true, room);

        for slot in 0..USED_SLOTS {
            let Some(rect) = hearth_slot_rect(slot) else {
                continue;
            };
            let edge = if self.held == Some((Side::Chest, slot)) {
                SlotEdge::Source
            } else if hovered == Some((Side::Chest, slot)) {
                SlotEdge::Hovered
            } else {
                SlotEdge::Plain
            };
            draw_slot_stack(p, rect, layers, self.contents.slots()[slot], edge);
        }

        // The flame: a bar over the fuel slot that shrinks as the fire
        // burns down, and goes out with it.
        let flame = flame_rect();
        p.well(flame, GAUGE_BACK);
        let burning = (fire.fuel_left / FULL_FLAME_SECONDS).clamp(0.0, 1.0);
        if burning > 0.0 {
            let inside = Rect::new(
                flame.x0 + widgets::BEVEL,
                flame.y0 + widgets::BEVEL,
                flame.x1 - widgets::BEVEL,
                flame.y1 - widgets::BEVEL,
            );
            p.quad(
                Rect::new(
                    inside.x0,
                    inside.y0,
                    inside.x0 + inside.width() * burning,
                    inside.y1,
                ),
                FLAME,
            );
        }
        // A flame beside the word, from the same sheet the fire in the
        // world is drawn with.
        let label_top = widgets::caption_top_over(fuel.y1, CAPTION_SCALE);
        let flame_icon = crate::ui::inventory_screen::icon_beside(
            p,
            (fuel.x0, label_top),
            widgets::cell_height(CAPTION_SCALE) * 1.05,
            layers.flame(0),
        );
        // The same size as the captions over the other groups, and fitted
        // to the half of the row the ash's word leaves it.
        let fuel_word = language.text(Msg::HearthFuel);
        let room = (ash.x1 - fuel.x0) / 2.0 - 0.01 - (flame_icon.x1 + 0.006 - fuel.x0);
        p.text(
            fuel_word,
            flame_icon.x1 + 0.006,
            label_top,
            widgets::fitted_scale(fuel_word, CAPTION_SCALE, room, 0.7),
            widgets::INK_DIM,
        );

        // ---- the heat ----
        //
        // A bar that fills in the colour the fire is, with a mark where
        // the batch needs it, and the colour named above it. **The mark is
        // the point**: a fire that is burning and doing nothing now has a
        // third reason, and the one thing a player can read off a bar
        // stopping short of a line is "this wants a hotter fuel". See
        // `heat_rect` for why it sits between the fuel and the ash.
        let heat = heat_rect();
        p.well(heat, GAUGE_BACK);
        let glow = Glow::of(fire.degrees);
        let inside = Rect::new(
            heat.x0 + widgets::BEVEL,
            heat.y0 + widgets::BEVEL,
            heat.x1 - widgets::BEVEL,
            heat.y1 - widgets::BEVEL,
        );
        let filled = (fire.degrees / GAUGE_TOP_C).clamp(0.0, 1.0);
        if filled > 0.0 {
            p.quad(
                Rect::new(inside.x0, inside.y0, inside.x0 + inside.width() * filled, inside.y1),
                glow_colour(glow),
            );
        }
        if fire.needs > 0.0 {
            let x = inside.x0 + inside.width() * (fire.needs / GAUGE_TOP_C).clamp(0.0, 1.0);
            p.quad(
                Rect::new(x - NEEDS_MARK / 2.0, heat.y0, x + NEEDS_MARK / 2.0, heat.y1),
                widgets::INK,
            );
        }
        // The caption quiet and the reading under it not: `yellow` is the
        // thing a player came to this corner to find out.
        p.text(language.text(Msg::HearthHeat), heat.x0, heat_label_top(), 0.72, widgets::INK_DIM);
        p.text(language.text(glow_msg(glow)), heat.x0, heat_word_top(), 0.72, widgets::INK);

        // ...and the arrow, filling towards the results.
        //
        // Drawn as a track with a head on it rather than as a plain bar:
        // a rectangle that grows says "a number is going up", and what
        // this actually means is "that is turning into this". The head
        // is three stacked quads, which is what a triangle is when
        // everything you can draw is a rectangle.
        let arrow = progress_rect();
        let shaft = Rect::new(arrow.x0, arrow.y0, arrow.x1 - ARROW_HEIGHT * 0.9, arrow.y1);
        p.well(shaft, GAUGE_BACK);
        if fire.progress > 0.0 {
            let inside = Rect::new(
                shaft.x0 + widgets::BEVEL,
                shaft.y0 + widgets::BEVEL,
                shaft.x1 - widgets::BEVEL,
                shaft.y1 - widgets::BEVEL,
            );
            p.quad(
                Rect::new(
                    inside.x0,
                    inside.y0,
                    inside.x0 + inside.width() * fire.progress.clamp(0.0, 1.0),
                    inside.y1,
                ),
                PROGRESS,
            );
        }
        let head_colour = if fire.progress > 0.0 { PROGRESS } else { widgets::WELL_DARK };
        // Four steps rather than three, each a little narrower than the
        // last: at this size a two-step head reads as a smudge on the
        // end of the bar rather than as a point.
        const STEPS: usize = 4;
        let head = ARROW_HEIGHT * 0.9;
        for step in 0..STEPS {
            let fraction = step as f32 / STEPS as f32;
            let inset = ARROW_HEIGHT * 0.5 * fraction;
            let x0 = shaft.x1 + head * fraction;
            p.quad(
                Rect::new(
                    x0,
                    arrow.y0 + inset,
                    x0 + head / STEPS as f32 + 0.002,
                    arrow.y1 - inset,
                ),
                head_colour,
            );
        }

        // One line under the fire saying what it is doing. A gauge that
        // is not moving has several possible reasons -- no fire, nothing
        // to make, not hot enough, rain -- and a player cannot tell them
        // apart by looking. See `hearth_status` for the order.
        let (say, colour) = hearth_status(fire, &self.contents, language);
        // **Centred on the panel, not the arrow -- see `hearth_status_y`.**
        // `arrow.centre_x()` is what this used to be, and at
        // `HearthUnlit`'s length in Russian the line reached past the
        // arrow's own gap and onto the ingredient slot beside it.
        // **Centred on the panel, not the arrow -- see `hearth_status_y`.**
        // `arrow.centre_x()` is what this used to be, and even in
        // English "not burning -- strike it with flint" was already
        // wide enough to be drawn straight over an ingredient slot.
        p.text_centred(&say, 0.0, hearth_status_y(), 0.68, colour);
    }

    /// The rack half of the screen: a frame, a tray, and the sky.
    ///
    /// **The gauge is the whole reason this screen exists.** A hearth
    /// that is doing nothing is a hearth a player can see is out; a rack
    /// that is doing nothing looks exactly like a rack that is working,
    /// because the thing stopping it is the weather. So the bar under
    /// the frame measures how fast the sky is letting it go, and the
    /// line under the arrow says why in words.
    fn draw_rack(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        hovered: Option<(Side, usize)>,
        language: Language,
    ) {
        use primitive_shared::rack::{CURE_SECONDS, HIDE_SLOT, LEATHER_SLOT, USED_SLOTS};

        let state = self
            .weather
            .unwrap_or(primitive_shared::protocol::RackState {
                progress: 0.0,
                rate: 0.0,
                wet: false,
                near_fire: false,
            });
        let frame = rack_slot_rect(HIDE_SLOT).expect("a rack has a frame");
        let tray = rack_slot_rect(LEATHER_SLOT).expect("a rack has a tray");

        // The frame's word from its left edge and the leather's from its
        // right, for the reason `caption_over` gives: DONE written rightward
        // from the tray's left edge was, in the plain English, TAKE IT HERE
        // running out through the side of the panel.
        let room = (tray.x1 - frame.x0) / 2.0 - 0.01;
        caption_over(p, frame, language.text(Msg::RackSkins), false, room);
        caption_over(p, tray, language.text(Msg::RackLeather), true, room);

        for slot in 0..USED_SLOTS {
            let Some(rect) = rack_slot_rect(slot) else {
                continue;
            };
            let edge = if self.held == Some((Side::Chest, slot)) {
                SlotEdge::Source
            } else if hovered == Some((Side::Chest, slot)) {
                SlotEdge::Hovered
            } else {
                SlotEdge::Plain
            };
            draw_slot_stack(p, rect, layers, self.contents.slots()[slot], edge);
        }

        // The weather gauge, under the frame: as wide as the slot it
        // belongs to, the way the hearth's flame is.
        let gauge = rack_gauge_rect();
        p.well(gauge, GAUGE_BACK);
        let rate = state.rate.clamp(0.0, 1.0);
        if rate > 0.0 {
            let inside = Rect::new(
                gauge.x0 + widgets::BEVEL,
                gauge.y0 + widgets::BEVEL,
                gauge.x1 - widgets::BEVEL,
                gauge.y1 - widgets::BEVEL,
            );
            p.quad(
                Rect::new(
                    inside.x0,
                    inside.y0,
                    inside.x0 + inside.width() * rate,
                    inside.y1,
                ),
                // Fire-coloured when a fire is what is doing it, and the
                // colour of a dry afternoon otherwise. The two are the
                // two ways a skin cures and the gauge says which.
                if state.near_fire { FLAME } else { SUN },
            );
        }
        p.text(
            language.text(Msg::RackWeather),
            gauge.x0,
            gauge.y0 - 0.010,
            0.62,
            widgets::INK_DIM,
        );

        // ...and the arrow, filling towards the tray. The same track and
        // head the hearth draws, because it means the same thing: that
        // is turning into this.
        let arrow = rack_progress_rect();
        let shaft = Rect::new(arrow.x0, arrow.y0, arrow.x1 - ARROW_HEIGHT * 0.9, arrow.y1);
        p.well(shaft, GAUGE_BACK);
        let progress = state.progress.clamp(0.0, 1.0);
        if progress > 0.0 {
            let inside = Rect::new(
                shaft.x0 + widgets::BEVEL,
                shaft.y0 + widgets::BEVEL,
                shaft.x1 - widgets::BEVEL,
                shaft.y1 - widgets::BEVEL,
            );
            p.quad(
                Rect::new(
                    inside.x0,
                    inside.y0,
                    inside.x0 + inside.width() * progress,
                    inside.y1,
                ),
                PROGRESS,
            );
        }
        let head_colour = if progress > 0.0 { PROGRESS } else { widgets::WELL_DARK };
        const STEPS: usize = 4;
        let head = ARROW_HEIGHT * 0.9;
        for step in 0..STEPS {
            let fraction = step as f32 / STEPS as f32;
            let inset = ARROW_HEIGHT * 0.5 * fraction;
            let x0 = shaft.x1 + head * fraction;
            p.quad(
                Rect::new(
                    x0,
                    arrow.y0 + inset,
                    x0 + head / STEPS as f32 + 0.002,
                    arrow.y1 - inset,
                ),
                head_colour,
            );
        }

        // **How far, and how long.** A bar on its own says "wait"; a
        // rack is twelve minutes of waiting and the one thing a player
        // wants off this screen is whether that is worth standing here
        // for. The minutes are the honest arithmetic -- what is left, at
        // the speed the weather is currently allowing -- so they stretch
        // when it clouds over, which is the truth.
        let curing = primitive_shared::rack::curing(&self.contents).is_some();
        let mut reading = format!("{:.0}%", progress * 100.0);
        if curing && rate > 0.0 {
            // The frame's own time for what is on it: a minute for grass,
            // twelve for a skin (`rack::cure_seconds`).
            let seconds = self
                .contents
                .block_in(HIDE_SLOT)
                .map_or(CURE_SECONDS, primitive_shared::rack::cure_seconds);
            let left = (1.0 - progress) * seconds / rate;
            reading = format!(
                "{reading}   ~{} {}",
                (left / 60.0).ceil().max(1.0) as u32,
                language.text(Msg::MinutesLeft),
            );
        }
        p.text_centred(
            &reading,
            arrow.centre_x(),
            arrow.y1 + widgets::cell_height(0.68) + 0.012,
            0.68,
            widgets::INK,
        );

        // One line under it saying what is going on, because every way a
        // rack can be stopped looks the same from the outside.
        let has_skin = self.contents.block_in(HIDE_SLOT).is_some();
        let say = if !has_skin {
            Msg::RackEmpty
        } else if !curing {
            Msg::RackFull
        } else if state.wet {
            Msg::RackWet
        } else if rate <= 0.0 {
            Msg::RackFrozen
        } else if state.near_fire {
            Msg::RackSmoking
        } else {
            Msg::RackDrying
        };
        p.text_centred(
            language.text(say),
            0.0,
            rack_status_y(),
            0.68,
            if curing && rate > 0.0 {
                widgets::TEXT_GOOD
            } else {
                widgets::TEXT_BAD
            },
        );
    }

    /// The jug's half of the screen: its one slot, how much of its measure
    /// is in it, and what may go in.
    ///
    /// **The measure is written because it is the refusal nobody can
    /// see.** A stack dropped on a full jug, or meat dropped on an empty
    /// one, simply does not go in -- the slot looks the same before and
    /// after -- so the reading says how far from full it is and the line
    /// under it says which of the two refusals is coming.
    fn draw_vessel(
        &self,
        p: &mut Painter,
        layers: &FaceLayers,
        hovered: Option<(Side, usize)>,
        language: Language,
    ) {
        use primitive_shared::inventory::{JUG_UNITS, VESSEL_SLOT};
        let rect = vessel_slot_rect(VESSEL_SLOT).expect("a jug has its slot");
        let edge = if self.held == Some((Side::Chest, VESSEL_SLOT)) {
            SlotEdge::Source
        } else if hovered == Some((Side::Chest, VESSEL_SLOT)) {
            SlotEdge::Hovered
        } else {
            SlotEdge::Plain
        };
        draw_slot_stack(p, rect, layers, self.contents.slots()[VESSEL_SLOT], edge);

        let held = self.contents.count_in(VESSEL_SLOT);
        p.text_centred(
            &format!("{held} / {JUG_UNITS}"),
            rect.centre_x(),
            vessel_reading_y(),
            0.72,
            widgets::INK,
        );
        let full = held >= JUG_UNITS;
        p.text_centred(
            language.text(if full { Msg::VesselFull } else { Msg::VesselEmpty }),
            0.0,
            vessel_status_y(),
            0.68,
            if full { widgets::TEXT_BAD } else { widgets::INK_DIM },
        );
    }

    /// The same screen, appended to a list the caller keeps between    /// The same screen, appended to a list the caller keeps between
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

        let layout = self.layout();
        let panel = panel_rect(layout);
        p.deep_panel(panel);
        let band = p.panel_header(
            panel,
            language.text(self.heading),
            header_height(),
            // Nothing to be indented past any more: the band holds the
            // name of the thing and its picture, and the way out is the
            // whole of the rest of the screen.
            0.0,
        );
        // The thing's own picture at the right end of the title bar: a
        // player who has three chests and a kiln open in a session
        // recognises which is which before they have read the word.
        if let Some(block) = self.block {
            let size = header_height() * 0.62;
            crate::ui::inventory_screen::icon_beside(
                &mut p,
                (panel.x1 - PANEL_PAD - size, band + header_height() * 0.80),
                size,
                crate::ui::inventory_screen::icon_layer(layers, block),
            );
        }

        // The working half's own recess, before anything stands in it.
        // See `work_tray`.
        if let Some(tray) = work_tray(layout) {
            p.well(tray, widgets::TRAY);
        }

        let hovered = self.cursor.and_then(|cursor| slot_at(cursor, layout));

        match layout {
            Layout::Hearth => self.draw_hearth(&mut p, layers, hovered, language),
            Layout::Rack => self.draw_rack(&mut p, layers, hovered, language),
            Layout::Vessel => self.draw_vessel(&mut p, layers, hovered, language),
            Layout::Chest | Layout::Bags => {}
            Layout::Body { rucksack } => {
                // The body's own word on its tab rather than a new one:
                // the heading already says BODY or BONES, and the tab
                // under it saying the same is what ties the forty to it.
                for (page, label) in [(false, self.heading), (true, Msg::TabBackpack)] {
                    let rect = page_tab_rect(page);
                    let text = language.text(label);
                    if page == rucksack {
                        p.well(rect, widgets::TRAY);
                        let scale = widgets::fitted_scale(text, 0.80, (rect.width() - 0.03).max(0.0), 0.55);
                        p.text_centred(
                            text,
                            rect.centre_x(),
                            rect.centre_y() + widgets::cell_height(scale) / 2.0,
                            scale,
                            widgets::ACCENT,
                        );
                    } else {
                        let over = self.cursor.is_some_and(|(x, y)| rect.contains(x, y));
                        p.button(rect, text, over, true);
                    }
                }
            }
        }
        // Two words rather than "chest" and "pack" again: the heading
        // already says which screen this is, and what the player is
        // deciding between is what is *stored* and what they are
        // *carrying*.
        let sides: &[(Side, &Inventory, &str)] = if !layout.grid() {
            // The hearth's and the rack's halves are drawn by the two
            // functions above; only the pack is a grid.
            &[(Side::Pack, pack, language.text(Msg::Carried))]
        } else {
            &[
                (Side::Chest, &self.contents, language.text(Msg::Stored)),
                (Side::Pack, pack, language.text(Msg::Carried)),
            ]
        };
        // The trays, before what stands in them. See the note over
        // `chest_tray` for what eighty identical holes on one flat slab
        // did to the one question this screen asks -- which side.
        for &(side, _, _) in sides {
            p.well(
                match side {
                    Side::Chest => chest_tray(),
                    Side::Pack => pack_tray(),
                },
                widgets::TRAY,
            );
        }
        for &(side, inventory, label) in sides {
            let first = slot_rect(side, 0);
            // Above the *top* row of the grid it names. Slot
            // `HOTBAR_SLOTS` is the first of the storage rows, which is
            // that row's left end; the bottom row is the belt.
            let top_left = slot_rect(side, HOTBAR_SLOTS);
            p.text(
                label,
                top_left.x0,
                caption_top(side),
                CAPTION_SCALE,
                // The quiet tier: this names a region, and the amber
                // title above already says which screen it is. Three
                // steps -- title, contents, the words naming a half --
                // is what makes the screen scannable without reading
                // it. Drawn in full ink, all three were one step.
                widgets::INK_DIM,
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
                // The belt is a raised strip of the same stone with the
                // ten in-hand slots cut into it -- the wells are drawn
                // over it by the loop below. A coloured band behind them
                // was the one place this screen still looked like a web
                // page.
                // No word under it. The pack screen threw `BELT` out
                // first -- "a caption under it says nothing the box has
                // not already said" -- and this screen kept it, which
                // left one screen captioning two of its three regions
                // above the rows and the third below them. That
                // inconsistency is a thing a player feels without being
                // able to name, and the strip already says what it is.
                p.slab(strip, widgets::BUTTON);
            }

            // By place on the screen, and each place asked which square
            // it shows -- the same question `slot_at` asks, so the two
            // cannot disagree about a body's rucksack page.
            for place in 0..side_slots(side) {
                let square = match side {
                    Side::Chest => chest_square(layout, place),
                    Side::Pack => Some(place),
                };
                let Some((slot, &stack)) = square.and_then(|s| Some((s, inventory.slots().get(s)?))) else {
                    continue;
                };
                let edge = if self.held == Some((side, slot)) {
                    SlotEdge::Source
                } else if hovered == Some((side, slot)) {
                    SlotEdge::Hovered
                } else {
                    SlotEdge::Plain
                };
                draw_slot_stack(&mut p, slot_rect(side, place), layers, stack, edge);
            }
        }

        // **What is under the pointer, in words.** This screen had no
        // tooltip at all: a chest is forty squares of picture, and a
        // player looking for their third stack of cobblestone was
        // counting pixels. It is the same one the pack's own screen has
        // -- name, count, weight -- drawn last so it is over everything.
        if let (Some(cursor), Some((side, slot))) = (self.cursor, hovered) {
            let inventory = match side {
                Side::Chest => &self.contents,
                Side::Pack => pack,
            };
            if let Some(block) = inventory.block_in(slot) {
                let count = inventory.count_in(slot);
                let text = crate::ui::names::stack_line(block, count, language);
                crate::ui::inventory_screen::hover_note(&mut p, cursor, &text, panel);
            }
        }

        // The action bar, between the grids. A hearth has none: "store
        // everything" into a furnace is a gesture with no meaning,
        // "take everything" would empty the fuel and the apparatus
        // along with the results, and tidying slots that have roles
        // would shovel the fuel in with the ore -- see the server's
        // `chest_sort`.
        if layout.grid() {
            p.well(action_bar_rect(), widgets::TRAY);
            // The two directions at the ends and the tidy between them.
            // The arrow in each bulk label points the way the things go
            // and the word says what goes: "store" is not a direction
            // and neither is an arrow on its own.
            let buttons = [
                (bulk_button_rect(false), language.text(Msg::TakeAll)),
                (sort_button_rect(), language.text(Msg::TidyPile)),
                (bulk_button_rect(true), language.text(Msg::StoreAll)),
            ];
            for (rect, label) in buttons {
                let hovered = self.cursor.is_some_and(|(x, y)| rect.contains(x, y));
                p.slab(
                    rect,
                    if hovered { widgets::BUTTON_HOVER } else { widgets::BUTTON },
                );
                // **Fitted, not fixed.** At a flat 0.72 these ran out of
                // their own slabs in the longer languages -- `SPRZATNIJ`
                // is nine letters where `TIDY` is four -- and a button
                // whose word overflows it reads as broken rather than as
                // full. Not through `Painter::button`, which sizes its
                // label off the button's *height* against a design
                // height of 0.100: these are half that tall on a
                // desktop, so it would letter them at its floor of 0.55
                // and shrink four readable words for no reason.
                let scale = widgets::fitted_scale(label, 0.72, rect.width() - 0.024, 0.7);
                p.label_in(rect, label, scale, widgets::INK);
            }
        }

        // What the chest is holding, in the units the player thinks in.
        // Weight, because it is the number the whole load mechanic turns
        // on -- and a chest is where you go to stop carrying it.
        let stored = self.contents.total_items();
        // Every square the container has: a body with a rucksack is sixty,
        // and "12/40" over a body holding fifty stacks is a lie about the
        // one thing the line is for.
        let squares = if layout.grid() { self.contents.slots().len().max(CHEST_SLOTS) } else { CHEST_SLOTS };
        let used = (0..squares)
            .filter(|&slot| self.contents.count_in(slot) > 0)
            .count();
        // **"2/40 slots" is a true sentence about a rack and a useless
        // one.** Forty is the size of the inventory type these
        // containers share, not the size of the container: a rack has
        // two slots and a hearth has seven, and a fraction whose
        // denominator is neither says nothing a player can act on. What
        // is left -- how much is in there, and what it weighs -- is true
        // of all three.
        let summary = if layout.grid() {
            format!(
                "{used}/{squares} {}   {stored} {}   {:.0} kg",
                language.text(Msg::SlotsWord),
                language.text(Msg::ItemsWord),
                self.contents.total_weight(),
            )
        } else {
            format!(
                "{stored} {}   {:.0} kg",
                language.text(Msg::ItemsWord),
                self.contents.total_weight(),
            )
        };
        // **No rule under the grid.** There was one, drawn the whole
        // width of the panel to separate a row of text from the slots
        // over it -- and the pack's tray now ends in a bevel exactly
        // there, so the rule was a second line saying what the first one
        // already said. Worse than redundant: the tray reaches lower
        // than the rule did, so the two crossed.
        p.text(&summary, grid_left(), panel.y0 + 0.108, 0.86, widgets::INK);
        p.text(
            language.text(hint()),
            grid_left(),
            panel.y0 + 0.056,
            HINT_SCALE,
            widgets::INK_DIM,
        );

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
    // A hearth first: it is a container now, and a kiln whose screen
    // says CHEST is a screen that has not been told what it is looking
    // at.
    match primitive_shared::hearth::Kind::of(block) {
        Some(primitive_shared::hearth::Kind::Campfire) => return Msg::Campfire,
        Some(primitive_shared::hearth::Kind::Kiln) => return Msg::Kiln,
        Some(primitive_shared::hearth::Kind::Bloomery) => return Msg::Bloomery,
        None => {}
    }
    if primitive_shared::rack::is_rack(block) {
        return Msg::DryingRack;
    }
    if primitive_shared::types::opens_as_vessel(block) {
        return Msg::Jug;
    }
    // A horse's bags are named by the client for the screen (see the
    // `ChestState` arm in `drain_network`): there is no cell to ask.
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_SADDLEBAGS {
        return Msg::Saddlebags;
    }
    // Where you died, and what is left of it once the ground has had the
    // soft half -- and the pack an older world may still be holding, which
    // nothing lays any more (`types::BLOCK_BACKPACK`).
    match primitive_shared::types::block_kind(block) {
        primitive_shared::types::BLOCK_CORPSE => Msg::Corpse,
        primitive_shared::types::BLOCK_REMAINS => Msg::Remains,
        primitive_shared::types::BLOCK_BACKPACK => Msg::Backpack,
        _ => Msg::Chest,
    }
}

/// What a gesture on a jug open in the hand asks the server for.
///
/// **None of the container messages mean anything for this jug** -- the
/// server has no container open for it (see `ChestScreen::vessel_slot`) --
/// so each intent becomes one of the pack's own jug messages against the
/// jug's slot, `jug`:
///
/// * pack to jug is `PourIntoJug`, all that fits, half or not -- loose
///   goods are tipped in, not counted in;
/// * jug to pack is `TakeFromJug`, into the square the stack was dropped
///   on, all or half;
/// * a shift-click out of the jug is `EmptyJug`, into wherever there is
///   room, which is what that message always was;
/// * a move within the pack is the pack screen's own move or split.
///
/// **The jug's own slot is never either end of any of them.** Pouring the
/// jug into itself is the one vessel-inside-a-vessel that the pouring rule
/// is not the first thing to see (the server refuses it on the slot
/// numbers before `types::pours` is asked), and dragging the open jug to
/// another square would leave the screen looking into a slot that no longer
/// holds it. Both are no message at all -- and so are the two chest verbs
/// this screen does not draw.
pub fn held_vessel_message(
    intent: Intent,
    jug: usize,
) -> Option<primitive_shared::protocol::ClientMessage> {
    use primitive_shared::protocol::ClientMessage;
    let byte = |slot: usize| u8::try_from(slot).ok();
    match intent {
        Intent::Move {
            from: (Side::Pack, from),
            to: (Side::Chest, _),
            ..
        }
        | Intent::QuickMove(Side::Pack, from) => {
            if from == jug {
                return None;
            }
            Some(ClientMessage::PourIntoJug {
                from: byte(from)?,
                jug: byte(jug)?,
            })
        }
        Intent::Move {
            from: (Side::Chest, _),
            to: (Side::Pack, to),
            half,
        } => {
            if to == jug {
                return None;
            }
            Some(ClientMessage::TakeFromJug {
                jug: byte(jug)?,
                to: byte(to)?,
                half,
            })
        }
        Intent::QuickMove(Side::Chest, _) => Some(ClientMessage::EmptyJug { slot: byte(jug)? }),
        Intent::Move {
            from: (Side::Pack, from),
            to: (Side::Pack, to),
            half,
        } => {
            if from == jug || to == jug {
                return None;
            }
            let (from, to) = (byte(from)?, byte(to)?);
            Some(if half {
                ClientMessage::SplitSlot { from, to }
            } else {
                ClientMessage::MoveSlots { from, to }
            })
        }
        Intent::Move {
            from: (Side::Chest, _),
            to: (Side::Chest, _),
            ..
        }
        | Intent::Sort
        | Intent::BulkMove { .. }
        | Intent::MoveKind(..)
        | Intent::Close => None,
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

/// Height of one side's grid, its label included.
fn grid_height(side: Side) -> f32 {
    let rows = rows(side) as f32;
    rows * CELL + (rows - 1.0) * GAP + HOTBAR_SPLIT + LABEL_HEIGHT
}



fn grid_left() -> f32 {
    -grid_width() / 2.0
}

/// Top of the upper grid's cells.
fn content_top() -> f32 {
    (grid_height(Side::Chest) + grid_height(Side::Pack) + grid_split()) / 2.0 - LABEL_HEIGHT
}

// ---- two trays, not eighty holes ----
//
// **The screen was one field of slots.** Eighty identical wells on one
// flat slab, in two blocks of forty separated by a gap -- and a gap is
// the weakest thing an interface has to say "these are two different
// places". It matters here more than anywhere else in the game, because
// getting it wrong is not a misread, it is a stack of iron put in the
// wrong inventory: the whole gesture on this screen is *which side*.
//
// Each grid sits in a shallow tray now, with its caption inside the
// tray's own top so the word belongs visibly to the rows under it, and
// the action bar in the groove between them is the line where one place
// ends and the other begins. No slot moved: the trays are drawn in the
// gaps that were already there.

/// The tray under the chest's grid, its caption included.
fn chest_tray() -> Rect {
    tray_around(Side::Chest, content_top() + LABEL_HEIGHT)
}

/// The tray under the player's, from its caption down past the belt.
///
/// Its floor is the belt strip's rather than the bottom row's: the strip
/// is a raised slab standing *in* this tray, and a tray that stopped at
/// the slots would have it hanging over the edge.
fn pack_tray() -> Rect {
    let top = slot_rect(Side::Pack, HOTBAR_SLOTS).y1 + LABEL_HEIGHT;
    tray_around(Side::Pack, top)
}

/// How much stone is left between a working half's tray and the pack's.
///
/// The fourteen thousandths a chest leaves between its tray and its action
/// bar. It was twenty at first, and on a phone that set the tray's floor
/// under the hearth's status line: the tail of the `g` in `working` sat on
/// the bevel. The line hangs a little below the half it belongs to (see
/// `hearth_status_y`), so the floor has to give it the room.
const WORK_TRAY_GROOVE: f32 = 0.014;

/// The tray a hearth's, a rack's or a jug's half stands in.
///
/// **The chest had two trays and these screens had one.** STORED and
/// CARRIED each sit in a recess with their caption inside it, which is
/// what says "two places"; a hearth drew its fire on bare stone above a
/// pack in a tray, so the half the screen was opened *for* was the only
/// half that did not look like a place. The same recess now, with the same
/// sides as the pack's under it, so the two halves line up down both edges.
///
/// Down to a groove above the pack's tray and no further. The chest spends
/// that gap on its action bar; here it is air, and a tray run on into it
/// would be one recess holding two places, which is the thing trays exist
/// to stop.
fn work_tray(layout: Layout) -> Option<Rect> {
    let top = match layout {
        Layout::Hearth => hearth_top(),
        Layout::Rack => rack_top(),
        Layout::Vessel => vessel_top(),
        Layout::Chest | Layout::Body { .. } | Layout::Bags => return None,
    };
    Some(Rect::new(
        grid_left() - TRAY_PAD,
        pack_tray().y1 + WORK_TRAY_GROOVE,
        grid_left() + grid_width() + TRAY_PAD,
        top + TRAY_PAD,
    ))
}

/// A caption over a group of slots on a working half, at the size every
/// stone screen writes one (`widgets::CAPTION_SCALE`).
///
/// `flush_right` hangs the word off the group's right edge instead of its
/// left. **The results sit at the right of the panel**, and a word written
/// rightward from their left edge ran on past them: COMES OUT ended a few
/// thousandths inside the panel's bevel, and the plain English -- TAKE
/// THINGS HERE -- ended outside the panel. Hung from the right it runs back
/// over the band above the arrow, which is empty at that height.
///
/// `room` is how far it may run before it is fitted down: half the row it
/// shares with the caption at the other end, so the two cannot meet in the
/// middle whichever language is the longer.
fn caption_over(p: &mut Painter, group: Rect, text: &str, flush_right: bool, room: f32) {
    let scale = widgets::fitted_scale(text, CAPTION_SCALE, room, 0.7);
    let top = widgets::caption_top_over(group.y1, scale);
    let x = if flush_right { group.x1 - widgets::ink_width(text, scale) } else { group.x0 };
    p.text(text, x, top, scale, widgets::INK_DIM);
}

/// How large a region caption is written: the size every stone screen
/// writes one at. It was 0.86 here and 0.62 on the pack, for the same
/// word doing the same job -- see `widgets::CAPTION_SCALE`.
const CAPTION_SCALE: f32 = widgets::CAPTION_SCALE;

/// Where the word naming a grid is written: above its top row, always.
///
/// **A function because the screen used to say it two ways.** STORED and
/// CARRIED were drawn over their grids and BELT under its strip, on one
/// screen, so a player scanning down it met a caption, some rows, a
/// caption, some rows, some rows, a caption -- and could not tell by
/// looking whether a word belonged to what was over it or under it. BELT
/// is gone (the strip says what it is), and what is left goes through
/// here so there is one answer to "where does a caption go".
///
/// `top` is the top of the text and the glyphs hang *down* from it, so a
/// caption handed the row's own edge is a caption written across the
/// row. Its own height plus a hair of air is what puts it above one --
/// which is what the first cut of this screen got wrong on every
/// heading it had.
fn caption_top(side: Side) -> f32 {
    widgets::caption_top_over(slot_rect(side, HOTBAR_SLOTS).y1, CAPTION_SCALE)
}

/// The tray around one grid, down to whatever it has at the bottom.
fn tray_around(side: Side, top: f32) -> Rect {
    let first = slot_rect(side, 0);
    let floor = match side {
        // The belt strip's own margin, and then the tray's.
        Side::Pack => first.y0 - 0.012,
        Side::Chest => first.y0,
    };
    Rect::new(
        grid_left() - TRAY_PAD,
        floor - TRAY_PAD,
        grid_left() + grid_width() + TRAY_PAD,
        top,
    )
}

fn panel_rect(layout: Layout) -> Rect {
    // **The panel is as tall as what is in it.** A hearth's half is a
    // third the height of a chest's grid, and keeping the chest's panel
    // for it left a hand's breadth of empty dark between the fire and
    // the pack -- which is what "there is nothing there" looks like when
    // a player is trying to work out where to click. A rack's half is
    // shorter still: one row of two.
    let top = match layout {
        Layout::Hearth => hearth_top() + LABEL_HEIGHT,
        Layout::Rack => rack_top() + LABEL_HEIGHT,
        Layout::Vessel => vessel_top() + LABEL_HEIGHT,
        Layout::Chest | Layout::Bags => content_top() + LABEL_HEIGHT,
        // The tabs are stacked on top of the chest's own panel rather than
        // cut out of it, so not one slot moves between a chest and a body
        // -- and the body's forty are clicked exactly where a chest's are.
        Layout::Body { .. } => content_top() + LABEL_HEIGHT + page_band(),
    };
    Rect::new(
        grid_left() - PANEL_PAD,
        -content_top() - LABEL_HEIGHT - FOOTER_HEIGHT,
        grid_left() + grid_width() + PANEL_PAD,
        top + header_height(),
    )
}

/// How much of the screen this container takes, about the middle.
///
/// Per kind, because the panel is: a hearth's half is a third the height
/// of a chest's, and asking a hearth to leave room for a chest's grid
/// would keep it a third smaller than it could be. See
/// `widgets::Layout::fit`, and `place_cursor`, which divides a click by
/// the answer to this.
fn extent_for(layout: Layout) -> (f32, f32) {
    let panel = panel_rect(layout);
    (panel.width() / 2.0, panel.y1.abs().max(panel.y0.abs()))
}

// ---- the action bar ----
//
// **The three verbs, in a groove, on the route.**
//
// TAKE ALL and STORE ALL were two small slabs floating in a band of
// empty stone, the same colour and half the size of the tidy button up
// in the title bar -- and they are the reason this screen exists. A
// player opens a chest to *move a lot of things at once*; forty
// shift-clicks is what these two replaced, and they were drawn as
// afterthoughts.
//
// Three decisions, and the ones they beat:
//
// * **Still between the grids, not in the footer.** The gesture is
//   "send what I am carrying up there" or "bring that down here", and a
//   control that sits on the route reads as the route. The same buttons
//   under the readout would need words to say which way they pointed;
//   the arrows in the labels do it for free where they are.
// * **In a groove rather than on bare stone.** The band had nothing in
//   it but two buttons and a gap, so it read as a hole between two
//   grids. A recess with the buttons standing in it is one object, and
//   it doubles as the line that separates STORED from CARRIED -- which
//   the screen did not have either.
// * **TIDY PILE moved down into it.** It is a verb that acts on the
//   chest, and it was in the title bar next to the way out: the two
//   controls a thumb is most likely to confuse, sitting at either end
//   of a band too short for either of them. The header carries the
//   name of the thing and the way out of it, and nothing else.

/// Width of the two bulk buttons, and of the tidy button between them.
const BULK_WIDTH: f32 = 0.30;
const SORT_WIDTH: f32 = 0.27;
/// How tall a button in the bar is where a mouse points at it. A finger
/// gets `widgets::tappable` of this -- see [`action_height`].
const BULK_HEIGHT: f32 = 0.050;

/// How tall a button in the action bar is drawn.
fn action_height() -> f32 {
    widgets::tappable(BULK_HEIGHT)
}

/// How tall the groove the buttons stand in is.
fn bar_height() -> f32 {
    action_height() + 0.024
}

/// The groove between the two grids that the three verbs stand in.
pub fn action_bar_rect() -> Rect {
    // Hung off the chest grid's tray rather than off the middle of the
    // gap: the tray's floor is a line the eye already has, and a bar
    // measured from the gap's centre moved every time the gap changed.
    let top = chest_tray().y0 - 0.014;
    Rect::new(grid_left(), top - bar_height(), grid_left() + grid_width(), top)
}

/// Where one of the three buttons sits inside the bar.
fn bar_button(x0: f32, width: f32) -> Rect {
    let bar = action_bar_rect();
    let y0 = bar.y0 + 0.012;
    Rect::new(x0, y0, x0 + width, y0 + action_height())
}

/// Where a bulk button sits: at its own end of the bar, pointing the way
/// it moves things.
pub fn bulk_button_rect(to_chest: bool) -> Rect {
    let x0 = if to_chest {
        grid_left() + grid_width() - BULK_WIDTH
    } else {
        grid_left()
    };
    bar_button(x0, BULK_WIDTH)
}

/// Where a slot sits on screen.
///
/// Within each grid the layout is the inventory screen's: the ten slots
/// that are the hotbar are the *bottom* row, matching where the real bar
/// is, and storage runs above them in reading order. The chest's grid
/// uses the same shape because it is the same inventory type -- its
/// bottom row is just ten more slots.
pub fn slot_rect(side: Side, slot: usize) -> Rect {
    chest_slot_rect(side, slot)
}

// ---- the hearth layout ----
//
// **A hearth is not a grid, and drawing it as one is what made the first
// attempt unreadable.** Its seven slots have roles -- four for what goes
// in, one for fuel, two for what comes out -- and a row of identical
// squares says none of that. What tells a player what a furnace does is
// the *shape*: things go in at the top, fire underneath them, results
// off to the side with an arrow filling up towards them. That shape is
// the oldest one in this kind of game because it explains itself.
//
// Everything below is measured from the middle of the panel outward, so
// the whole assembly is centred rather than hung off the left edge -- an
// arrangement that sits in one corner of a panel sized for something
// else was most of what was wrong with the first version.

/// The input square: two by two, which reads as a *batch* where a row of
/// four reads as a queue.
const INPUT_COLUMNS: usize = 2;

/// How far apart the three groups sit: ingredients, fire, results.
///
/// Wide, and deliberately: the three groups are meant to read as three
/// *places* rather than as one run of slots, and the assembly is sized
/// to fill the panel it is in. A furnace huddled in the middle of a
/// screen built for a chest looks like a mistake even when every slot is
/// where it should be.
const GROUP_GAP: f32 = 0.105;

/// The arrow between the ingredients and the results.
const ARROW_WIDTH: f32 = 0.34;
const ARROW_HEIGHT: f32 = 0.030;

/// The flame gauge under the fuel slot.
const FLAME_HEIGHT: f32 = 0.040;

/// How tall the hearth's half of the screen is: two rows of slots, the
/// fuel row under them, room for the labels over each group, and the
/// line underneath saying what the fire is doing.
///
/// **`STATUS_HEIGHT` is in this sum now, and it used to not be.** The
/// status line was drawn in the margin below the arrow instead of in a
/// row of its own -- which is exactly the mistake the rack's layout
/// note beside [`rack_status_y`] describes and was fixed for: at
/// `HearthUnlit`'s length in Russian, "не горит -- подожгите кремнём"
/// reaches wider than the gap between the ingredients and the arrow and
/// was drawn straight over the bottom-left ingredient slot. English
/// never showed it, because "not burning -- strike it with flint" is
/// short enough to fit the gap it was never actually given.
fn hearth_height() -> f32 {
    // Two rows of ingredients, the fuel slot under them with its label
    // and its gauge, one label's worth of air over the top, and the
    // status line's own row at the bottom -- see the note above.
    CELL * 3.0 + GAP * 2.0 + LABEL_HEIGHT * 2.0 + FLAME_HEIGHT + 0.012 + STATUS_HEIGHT
}

/// The middle of the hearth's half of the screen.
fn hearth_top() -> f32 {
    // Stacked *on top of* the pack, a hand's breadth clear of its label.
    // Measured from the pack rather than from the top of the panel,
    // because the pack is the half that does not move: whatever the
    // hearth's half turns out to be tall, the belt row stays exactly
    // where a player's eye already expects it.
    let pack_top = content_top() - grid_height(Side::Chest) - grid_split();
    pack_top + LABEL_HEIGHT + HALF_GAP + hearth_height()
}

/// Where the input square starts: left of centre by half the assembly.
fn hearth_left() -> f32 {
    let inputs = CELL * INPUT_COLUMNS as f32 + GAP;
    let outputs = CELL;
    let total = inputs + GROUP_GAP + ARROW_WIDTH + GROUP_GAP + outputs;
    -total / 2.0
}

/// Where a hearth's slot sits, or `None` for a slot it does not have.
pub fn hearth_slot_rect(slot: usize) -> Option<Rect> {
    use primitive_shared::hearth::{ASH_SLOT, FUEL_SLOT, INPUT_SLOTS, OUTPUT_SLOTS};
    let step = CELL + GAP;
    let left = hearth_left();
    let top = hearth_top() - LABEL_HEIGHT;

    if INPUT_SLOTS.contains(&slot) {
        let (column, row) = (slot % INPUT_COLUMNS, slot / INPUT_COLUMNS);
        let x0 = left + column as f32 * step;
        let y1 = top - row as f32 * step;
        return Some(Rect::new(x0, y1 - CELL, x0 + CELL, y1));
    }
    if slot == FUEL_SLOT {
        // Under the middle of the input square, a label's height below
        // it: the fire is beneath what it is heating, and the gap is
        // where the word FUEL goes.
        let x0 = left + step * 0.5;
        let y1 = top - step * 2.0 - LABEL_HEIGHT;
        return Some(Rect::new(x0, y1 - CELL, x0 + CELL, y1));
    }
    if OUTPUT_SLOTS.contains(&slot) {
        let index = slot - OUTPUT_SLOTS.start;
        let x0 = left + CELL * INPUT_COLUMNS as f32 + GAP + GROUP_GAP + ARROW_WIDTH + GROUP_GAP;
        let y1 = top - index as f32 * step;
        return Some(Rect::new(x0, y1 - CELL, x0 + CELL, y1));
    }
    if slot == ASH_SLOT {
        // Under the results and level with the fuel. What the fire gives
        // off comes out on the same side as what it made, and the fuel and
        // its ash face each other across the heat gauge between them --
        // which puts the new slot in a row the screen already had, so the
        // panel did not grow and the pack under it did not move.
        let x0 = left + CELL * INPUT_COLUMNS as f32 + GAP + GROUP_GAP + ARROW_WIDTH + GROUP_GAP;
        let y1 = top - step * 2.0 - LABEL_HEIGHT;
        return Some(Rect::new(x0, y1 - CELL, x0 + CELL, y1));
    }
    None
}

/// The arrow between the ingredients and the results: a track that fills
/// up towards the output.
///
/// Level with the *middle* of both groups, so it reads as pointing from
/// one to the other rather than as a bar that happens to be there.
fn progress_rect() -> Rect {
    let inputs = hearth_slot_rect(0).expect("a hearth has input slots");
    let bottom = hearth_slot_rect(2).expect("a hearth has four input slots");
    let y = (inputs.y1 + bottom.y0) / 2.0;
    let x0 = inputs.x1 + CELL + GAP + GROUP_GAP;
    Rect::new(x0, y - ARROW_HEIGHT / 2.0, x0 + ARROW_WIDTH, y + ARROW_HEIGHT / 2.0)
}

/// The flame gauge: a bar over the fuel slot, as wide as the input
/// square, because what it measures feeds the whole of it.
fn flame_rect() -> Rect {
    // Directly under the fuel slot and exactly as wide as it, so it
    // reads as *that slot's* gauge. A bar wider than the slot -- which
    // is where this started -- reads as a second progress bar for the
    // ingredients above it.
    let fuel = hearth_slot_rect(primitive_shared::hearth::FUEL_SLOT).expect("a fuel slot");
    Rect::new(
        fuel.x0,
        fuel.y0 - 0.008 - FLAME_HEIGHT,
        fuel.x1,
        fuel.y0 - 0.008,
    )
}

/// Where the line saying what the fire is doing is written: under the
/// flame gauge, across the middle of the panel.
///
/// **Centred on the panel and not on the arrow**, for the same reason
/// [`rack_status_y`] is: the arrow is a third of the panel's width, and
/// `HearthUnlit`'s Russian reaches wider than the gap between the
/// ingredients and it. Its own row, below everything else the hearth
/// draws -- see the note on [`hearth_height`] -- rather than squeezed
/// into the margin under the arrow, which is where it used to be and
/// why it used to reach the slot beside it.
fn hearth_status_y() -> f32 {
    flame_rect().y0 - 0.010 - widgets::cell_height(0.68) - 0.014
}

/// The hottest the heat gauge shows, in degrees.
///
/// TerraFirmaCraft's brilliant white, and the hottest thing in this world:
/// a bloomery on charcoal. A scale that ran further would leave the top of
/// the bar empty for ever; one that stopped at copper would pin every
/// charcoal fire against the end, where "hotter" cannot be seen.
const GAUGE_TOP_C: f32 = 1600.0;

/// How wide the mark on the heat gauge is where the batch needs the fire.
const NEEDS_MARK: f32 = 0.005;

/// The heat gauge: level with the flame gauge, under the middle of the
/// screen, between the fuel slot and the ash slot.
///
/// **Not beside the fuel label, where it was first measured to go.** The
/// label row over the fuel slot is where FUEL is written, and in the plain
/// English it reads WOOD OR COAL, with a flame in front of it -- wide
/// enough to reach a sixth of the way into the middle column. The row
/// *under* that, level with the fuel slot itself, is empty from the slot
/// to the ash: that is where the heat's own words go, and the bar goes
/// under them at the flame gauge's height, so the two gauges of the fire
/// read as a pair.
fn heat_rect() -> Rect {
    let flame = flame_rect();
    let arrow = progress_rect();
    Rect::new(arrow.x0, flame.y0, arrow.x1, flame.y1)
}

/// Where HEAT is written: the top of the fuel slot's row.
fn heat_label_top() -> f32 {
    let fuel = hearth_slot_rect(primitive_shared::hearth::FUEL_SLOT).expect("a fuel slot");
    fuel.y1 - 0.004
}

/// Where the colour of the fire is written: under HEAT, inside the same row.
fn heat_word_top() -> f32 {
    heat_label_top() - widgets::cell_height(0.72) - 0.010
}

/// The word for a heat colour.
fn glow_msg(glow: primitive_shared::hearth::Glow) -> Msg {
    use primitive_shared::hearth::Glow;
    match glow {
        Glow::Cold => Msg::GlowCold,
        Glow::Warm => Msg::GlowWarm,
        Glow::Hot => Msg::GlowHot,
        Glow::VeryHot => Msg::GlowVeryHot,
        Glow::FaintRed => Msg::GlowFaintRed,
        Glow::DarkRed => Msg::GlowDarkRed,
        Glow::BrightRed => Msg::GlowBrightRed,
        Glow::Orange => Msg::GlowOrange,
        Glow::Yellow => Msg::GlowYellow,
        Glow::White => Msg::GlowWhite,
    }
}

/// The colour a heat colour fills the gauge with.
///
/// The four bands below a glow are stone greys that warm a little, because
/// a fire that is not yet glowing does not look like anything; the rest
/// are the colours their names say, so the word and the bar agree.
fn glow_colour(glow: primitive_shared::hearth::Glow) -> [f32; 4] {
    use primitive_shared::hearth::Glow;
    match glow {
        Glow::Cold | Glow::Warm => [0.50, 0.50, 0.50, 1.0],
        Glow::Hot | Glow::VeryHot => [0.62, 0.57, 0.52, 1.0],
        Glow::FaintRed => [0.45, 0.12, 0.10, 1.0],
        Glow::DarkRed => [0.60, 0.13, 0.09, 1.0],
        Glow::BrightRed => [0.82, 0.18, 0.10, 1.0],
        Glow::Orange => [0.93, 0.48, 0.12, 1.0],
        Glow::Yellow => [0.96, 0.84, 0.28, 1.0],
        Glow::White => [0.99, 0.97, 0.88, 1.0],
    }
}

/// What the line under a hearth's fire says, and in what colour.
///
/// **In the order a player should act on them.** No fire at all comes
/// first, because nothing else matters. Charring comes before everything
/// that is left, because it is the one reason that is *losing* something
/// while the player reads it. Then the rain, because the rain is why the
/// fire is too cool, and saying "not hot enough" to a player standing in
/// a storm sends them for charcoal when what they need is a roof. A fire
/// that has gone out and is only coasting on its heat says so before it
/// says it is too cool, for the same reason: the answer is to light it.
///
/// One function, read by the drawing and by the test that holds every
/// answer it can give off the slots -- a reason added here that the test
/// did not walk would be a reason nobody measured.
fn hearth_status(
    fire: primitive_shared::protocol::HearthState,
    contents: &Inventory,
    language: Language,
) -> (String, [f32; 4]) {
    use primitive_shared::hearth::{is_charring, Glow};
    let lit = fire.fuel_left > 0.0;
    let say = |msg: Msg| language.text(msg).to_string();
    if !lit && fire.degrees <= 0.0 {
        return (say(Msg::HearthUnlit), widgets::TEXT_BAD);
    }
    if is_charring(contents, fire.degrees) {
        return (say(Msg::HearthCharring), widgets::TEXT_BAD);
    }
    if lit && fire.wet {
        return (say(Msg::HearthRained), widgets::TEXT_BAD);
    }
    let too_cool = fire.needs > 0.0 && fire.degrees < fire.needs;
    if !lit && (fire.needs <= 0.0 || too_cool) {
        return (say(Msg::HearthCooling), widgets::INK);
    }
    if too_cool {
        let needs = language.text(glow_msg(Glow::of(fire.needs)));
        return (
            format!("{} {needs}", language.text(Msg::HearthTooCool)),
            widgets::TEXT_BAD,
        );
    }
    if fire.progress > 0.0 {
        return (say(Msg::HearthWorking), widgets::TEXT_GOOD);
    }
    (say(Msg::HearthIdle), widgets::TEXT_GOOD)
}

// ---- the rack layout ----
//
// The hearth's shape with the middle taken out: a frame on the left, a
// tray on the right, an arrow between them, and where a hearth has its
// fire the rack has the sky. Same measurements, so the two screens read
// as two of a kind rather than as two designs -- and the pack below both
// of them does not move an inch when a player closes one and opens the
// other.

/// How tall the rack's half of the screen is: one row of slots, the
/// weather gauge under the frame with its word, and the two lines the
/// arrow is written between.
fn rack_height() -> f32 {
    // The last term is the line of words under it all. It is a *line*
    // rather than a label -- "rain has stopped it, put it under cover"
    // in four languages -- and it wants the full width of the panel,
    // which it can only have if nothing else is on that row.
    CELL + LABEL_HEIGHT * 2.0 + FLAME_HEIGHT + 0.012 + STATUS_HEIGHT
}

/// Room for the line under the rack -- and, now, under the hearth --
/// that says what it is doing.
const STATUS_HEIGHT: f32 = 0.052;

/// Where that line is written: under the gauge and its word, across the
/// middle of the panel.
///
/// **Centred on the panel and not on the arrow.** It is the longest text
/// on the screen and the arrow is a third of the width, so hanging it
/// off the arrow's middle put half of it over the frame slot and the
/// other half out past the tray.
fn rack_status_y() -> f32 {
    rack_gauge_rect().y0 - 0.010 - widgets::cell_height(0.62) - 0.014
}

/// The top of the rack's half. Measured up from the pack, exactly the
/// way `hearth_top` is and for the same reason: whatever the top half
/// turns out to be tall, the belt row stays where the eye expects it.
fn rack_top() -> f32 {
    let pack_top = content_top() - grid_height(Side::Chest) - grid_split();
    pack_top + LABEL_HEIGHT + HALF_GAP + rack_height()
}

/// Where the frame slot starts: left of centre by half the assembly.
fn rack_left() -> f32 {
    let total = CELL + GROUP_GAP + ARROW_WIDTH + GROUP_GAP + CELL;
    -total / 2.0
}

/// Where a rack's slot sits, or `None` for a slot it does not have.
pub fn rack_slot_rect(slot: usize) -> Option<Rect> {
    use primitive_shared::rack::{HIDE_SLOT, LEATHER_SLOT};
    let top = rack_top() - LABEL_HEIGHT;
    let x0 = match slot {
        s if s == HIDE_SLOT => rack_left(),
        s if s == LEATHER_SLOT => rack_left() + CELL + GROUP_GAP + ARROW_WIDTH + GROUP_GAP,
        _ => return None,
    };
    Some(Rect::new(x0, top - CELL, x0 + CELL, top))
}

/// The arrow between the frame and the tray, level with the middle of
/// both.
fn rack_progress_rect() -> Rect {
    let frame = rack_slot_rect(primitive_shared::rack::HIDE_SLOT).expect("a frame");
    let y = (frame.y0 + frame.y1) / 2.0;
    let x0 = frame.x1 + GROUP_GAP;
    Rect::new(x0, y - ARROW_HEIGHT / 2.0, x0 + ARROW_WIDTH, y + ARROW_HEIGHT / 2.0)
}

/// The weather gauge: a bar under the frame and exactly as wide as it,
/// so it reads as *that slot's* gauge -- the same rule the flame under a
/// fuel slot follows.
fn rack_gauge_rect() -> Rect {
    let frame = rack_slot_rect(primitive_shared::rack::HIDE_SLOT).expect("a frame");
    Rect::new(
        frame.x0,
        frame.y0 - 0.008 - FLAME_HEIGHT,
        frame.x1,
        frame.y0 - 0.008,
    )
}

// ---- the vessel layout ----
//
// The rack's shape with everything but one slot taken out: a jug has no
// process and no weather, only a measure. So the slot sits in the middle
// with how full it is written under it, and one line under that says the
// thing a player cannot see from the slot -- what may go in, or that
// nothing more will. Measured up from the pack the way the hearth and the
// rack are, so the belt does not move when a jug is opened after a chest.

/// How tall a jug's half of the screen is: a label's air over the slot,
/// the slot, the reading under it, and the status line's own row.
fn vessel_height() -> f32 {
    CELL + LABEL_HEIGHT * 2.0 + STATUS_HEIGHT
}

/// The top of a jug's half. See `rack_top`, which this follows exactly.
fn vessel_top() -> f32 {
    let pack_top = content_top() - grid_height(Side::Chest) - grid_split();
    pack_top + LABEL_HEIGHT + HALF_GAP + vessel_height()
}

/// Where a jug's slot sits, or `None` for a slot it does not have -- which
/// is every slot but `inventory::VESSEL_SLOT`.
pub fn vessel_slot_rect(slot: usize) -> Option<Rect> {
    if slot != primitive_shared::inventory::VESSEL_SLOT {
        return None;
    }
    let top = vessel_top() - LABEL_HEIGHT;
    Some(Rect::new(-CELL / 2.0, top - CELL, CELL / 2.0, top))
}

/// The top of the "12 / 16" under the jug's slot.
fn vessel_reading_y() -> f32 {
    vessel_slot_rect(primitive_shared::inventory::VESSEL_SLOT)
        .expect("a jug has its slot")
        .y0
        - 0.010
}

/// The top of the line under the reading. Centred on the panel and in a
/// row of its own, for the reason `rack_status_y` gives: it is the longest
/// text on the screen.
fn vessel_status_y() -> f32 {
    vessel_reading_y() - LABEL_HEIGHT
}

/// Where the tidy button sits: the middle of the action bar, between the
/// two directions.
///
/// **It was in the title bar**, 0.215 by 0.048, wedged between the
/// container's own icon and the heading -- 35 device pixels tall on the
/// phone this was measured on, against a 91-pixel finger, and its
/// nearest neighbour under a thumb was the way out. A verb that acts on
/// the chest belongs with the other two verbs that act on the chest; the
/// header is left holding the name of the thing and the way out of it.
pub fn sort_button_rect() -> Rect {
    bar_button(-SORT_WIDTH / 2.0, SORT_WIDTH)
}

/// Where a chest's slot sits on screen.
fn chest_slot_rect(side: Side, slot: usize) -> Rect {
    let top = match side {
        Side::Chest => content_top(),
        Side::Pack => content_top() - grid_height(Side::Chest) - grid_split(),
    };
    // **The split under the bottom row belongs to the pack alone.** It
    // marks the ten slots that are in hand during play, and drawing it
    // under a chest's bottom row says the same thing about a row that is
    // nothing of the kind: a chest with a detached strip along the
    // bottom reads as a chest with a hotbar in it.
    let split = if side == Side::Pack { HOTBAR_SPLIT } else { 0.0 };
    let (column, row_from_top, extra) = if slot < HOTBAR_SLOTS {
        (slot, rows(side) - 1, split)
    } else {
        let storage = slot - HOTBAR_SLOTS;
        (storage % HOTBAR_SLOTS, storage / HOTBAR_SLOTS, 0.0)
    };
    let x0 = grid_left() + column as f32 * (CELL + GAP);
    let y1 = top - row_from_top as f32 * (CELL + GAP) - extra;
    Rect::new(x0, y1 - CELL, x0 + CELL, y1)
}

/// Which slot of which grid a point is over, if any.
///
/// `layout` is what the open container is, because the three screens put
/// their slots in different places and a hit test that guessed would
/// hand a click on the fuel slot to whatever a chest has there.
pub fn slot_at(cursor: (f32, f32), layout: Layout) -> Option<(Side, usize)> {
    let hit = match layout {
        Layout::Hearth => (0..primitive_shared::hearth::USED_SLOTS).find(|&slot| {
            hearth_slot_rect(slot).is_some_and(|rect| rect.contains(cursor.0, cursor.1))
        }),
        Layout::Rack => (0..primitive_shared::rack::USED_SLOTS).find(|&slot| {
            rack_slot_rect(slot).is_some_and(|rect| rect.contains(cursor.0, cursor.1))
        }),
        // The rect `draw_vessel` draws, and only that one -- the inverse
        // the whole screen is tested against.
        Layout::Vessel => vessel_slot_rect(primitive_shared::inventory::VESSEL_SLOT)
            .filter(|rect| rect.contains(cursor.0, cursor.1))
            .map(|_| primitive_shared::inventory::VESSEL_SLOT),
        Layout::Chest | Layout::Body { .. } | Layout::Bags => {
            // A place under the pointer that shows no square -- the two
            // bare rows under a rucksack's twenty -- is nothing at all,
            // not the pack slot that is never there.
            let place = (0..side_slots(Side::Chest))
                .find(|&place| chest_slot_rect(Side::Chest, place).contains(cursor.0, cursor.1));
            if let Some(place) = place {
                return chest_square(layout, place).map(|square| (Side::Chest, square));
            }
            None
        }
    };
    if let Some(slot) = hit {
        return Some((Side::Chest, slot));
    }
    (0..side_slots(Side::Pack))
        .find(|&slot| chest_slot_rect(Side::Pack, slot).contains(cursor.0, cursor.1))
        .map(|slot| (Side::Pack, slot))
}

/// Which of the container's squares the chest grid's `place` shows, or
/// `None` for a place that shows nothing on this page.
///
/// The identity for a chest and for a body's own forty. On a body's
/// rucksack page the compartment's twenty are the grid's top two rows in
/// reading order -- the places a pack's storage starts at, which is where
/// the pack screen puts a worn rucksack (`inventory_screen::slot_in_place`)
/// -- and the two rows under them show nothing. **One function for the
/// drawing and the hit test**, which is the whole of how the two stay each
/// other's inverse.
fn chest_square(layout: Layout, place: usize) -> Option<usize> {
    use primitive_shared::inventory::{BACKPACK_SLOTS, CORPSE_COMPARTMENT};
    match layout {
        Layout::Body { rucksack: true } => {
            let offset = place.checked_sub(HOTBAR_SLOTS)?;
            (offset < BACKPACK_SLOTS).then_some(CORPSE_COMPARTMENT.start + offset)
        }
        Layout::Bags => (place < primitive_shared::horse::BAGS_SLOTS).then_some(place),
        _ => (place < CHEST_SLOTS).then_some(place),
    }
}

// ---- a body's two tabs ----
//
// Stacked above the chest's tray, inside a panel grown by exactly their
// band (`panel_rect`), so the grids under them are where a chest's are.
// The pack screen's strip at a smaller count: the same finger-floored
// height, the same hairline gap, the same margin above and below.

/// Air above and below the strip.
const PAGE_MARGIN: f32 = 0.014;
/// The gap between the two tabs: narrow, because they are two faces of
/// one thing -- see the pack screen's `TAB_GAP`.
const PAGE_GAP: f32 = 0.008;

/// How tall a tab is: a finger on glass, which on a phone is most of what
/// the strip costs.
fn page_tab_height() -> f32 {
    widgets::tappable(0.058)
}

/// The height the strip adds to the panel, air included.
fn page_band() -> f32 {
    page_tab_height() + PAGE_MARGIN * 2.0
}

/// Where a body's tab is drawn: the body's own forty on the left, the
/// rucksack on the right, together as wide as the tray under them.
///
/// **The exact inverse of [`page_tab_at`].**
pub fn page_tab_rect(rucksack: bool) -> Rect {
    let tray = chest_tray();
    let width = (tray.width() - PAGE_GAP) / 2.0;
    let x0 = tray.x0 + if rucksack { width + PAGE_GAP } else { 0.0 };
    let y0 = tray.y1 + PAGE_MARGIN;
    Rect::new(x0, y0, x0 + width, y0 + page_tab_height())
}

/// Which tab a point is over: `Some(true)` for the rucksack.
pub fn page_tab_at(cursor: (f32, f32)) -> Option<bool> {
    [false, true]
        .into_iter()
        .find(|&rucksack| page_tab_rect(rucksack).contains(cursor.0, cursor.1))
}

/// Whether a point is anywhere on the screen at all, so a click beside
/// the grids is not also a swing at the world behind it.
#[allow(dead_code)]
pub fn contains(cursor: (f32, f32)) -> bool {
    // The largest of the three, so a click beside a chest screen is
    // never treated as a swing at the world behind it. A few pixels of
    // dead margin round a rack costs nothing; a live one costs a block.
    // A body with a rucksack is the tallest now, by its tab strip.
    panel_rect(Layout::Body { rucksack: false }).contains(cursor.0, cursor.1)
}

#[cfg(test)]
mod bulk_tests {
    use super::*;
    use primitive_shared::protocol::ContainerKind;
    use primitive_shared::types::{BLOCK_CHEST, BLOCK_STONE};

    fn opened_at(cursor: (f32, f32)) -> ChestScreen {
        let mut screen = ChestScreen::new();
        screen.show((0, 0, 0), Inventory::new(), Some(BLOCK_CHEST), ContainerKind::Chest, None, None);
        screen.set_cursor(Some(cursor));
        screen
    }

    /// A screen with roles, opened at the same cell.
    fn opened_with_roles(kind: ContainerKind, cursor: (f32, f32)) -> ChestScreen {
        let mut screen = ChestScreen::new();
        screen.show((0, 0, 0), Inventory::new(), None, kind, None, None);
        screen.set_cursor(Some(cursor));
        screen
    }

    #[test]
    fn a_screen_without_the_bulk_buttons_has_no_invisible_ones() {
        // **The bug this is here for.** The two rectangles were
        // hit-tested whatever was open, and only *drawn* for a chest --
        // so at a hearth or a rack there were two live buttons in the
        // empty band over the pack with nothing on them, and a click on
        // apparently nothing shovelled the pack into the fire.
        let pack = {
            let mut inventory = Inventory::new();
            inventory.add(BLOCK_STONE, 12);
            inventory
        };
        for kind in [
            ContainerKind::Hearth(primitive_shared::hearth::Kind::Kiln),
            ContainerKind::Rack,
        ] {
            for to_chest in [true, false] {
                let rect = bulk_button_rect(to_chest);
                let mut screen =
                    opened_with_roles(kind, (rect.centre_x(), rect.centre_y()));
                assert_eq!(
                    screen.click(&pack, Button::Left, false, false),
                    None,
                    "{kind:?} answered a click on a button it does not draw"
                );
            }
        }
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
                screen.click(&pack, Button::Left, false, false),
                Some(Intent::BulkMove { to_chest })
            );
        }
    }

    #[test]
    fn the_buttons_sit_between_the_grids_and_on_no_slot() {
        // They are hit-tested before the slots, so a button over a slot
        // would make that slot unreachable.
        //
        // The tidy button is in this list now: it used to live in the
        // title bar, where nothing could collide with it, and moving it
        // down between the grids is exactly the change that makes this
        // assertion worth having.
        for rect in [bulk_button_rect(true), bulk_button_rect(false), sort_button_rect()] {
            for side in [Side::Chest, Side::Pack] {
                for slot in 0..SLOTS {
                    let s = slot_rect(side, slot);
                    let overlaps =
                        s.x0 < rect.x1 && s.x1 > rect.x0 && s.y0 < rect.y1 && s.y1 > rect.y0;
                    assert!(!overlaps, "a bar button covers {side:?} slot {slot}");
                }
            }
            let panel = panel_rect(Layout::Chest);
            assert!(
                rect.x0 >= panel.x0 && rect.x1 <= panel.x1,
                "a bar button hangs off the panel"
            );
            assert!(rect.y0 >= panel.y0 && rect.y1 <= panel.y1);
            let bar = action_bar_rect();
            assert!(
                rect.x0 >= bar.x0 && rect.x1 <= bar.x1 && rect.y0 >= bar.y0 && rect.y1 <= bar.y1,
                "a button stands outside the groove it is drawn in",
            );
        }
        // ...and the three of them do not stand on each other.
        let bar_buttons = [bulk_button_rect(false), sort_button_rect(), bulk_button_rect(true)];
        for (n, a) in bar_buttons.iter().enumerate() {
            for b in &bar_buttons[n + 1..] {
                assert!(
                    a.x1 <= b.x0 || b.x1 <= a.x0,
                    "two buttons in the action bar share a column",
                );
            }
        }
    }

    /// Every control on a container screen can be hit with a thumb.
    ///
    /// **The measurement this is here for**, taken on the phone the
    /// complaint came from -- 2712 by 1220 held sideways, interface size
    /// 1.65, where one unit of the interface is 610 device pixels and a
    /// finger is 91 of them:
    ///
    /// ```text
    /// X            48 px      TIDY PILE    35 px
    /// TAKE ALL     36 px      STORE ALL    36 px
    /// ```
    ///
    /// Every one of them under half a finger, on the screen a player
    /// opens *in order to move forty things*, and the way out among
    /// them.
    ///
    /// Asserted twice, and the second one is the one that matters. The
    /// phone above is a phone; the *floor* is `Layout::fit`, which never
    /// returns less than 1.0, so a control authored at
    /// `widgets::FINGER_SIDE` is a finger at every interface size a
    /// player can choose. Checking only the reported device would have
    /// let the whole thing come apart again the moment somebody dragged
    /// the setting down.
    #[test]
    fn every_control_on_a_container_is_at_least_a_finger_under_a_thumb() {
        widgets::as_a_phone(|| {
            let phone = widgets::Layout::for_screen(2712.0 / 1220.0, 1.65);
            // Floating subtraction on the way out of a `Rect`, so a
            // control sized at exactly a finger is not failed by its own
            // last bit.
            const SLACK: f32 = 1e-4;
            for kind in [
                ContainerKind::Chest,
                ContainerKind::Hearth(primitive_shared::hearth::Kind::Kiln),
                ContainerKind::Rack,
            ] {
                let layout = Layout::of(kind);
                let panel = panel_rect(layout);
                let drawn = phone.fit(extent_for(layout));
                // The way out is not in this list any more, and that
                // is the point of it not being here: leaving is a tap
                // anywhere off the panel, which is the largest target
                // on the screen by a wide margin.
                let _ = panel;
                let mut controls: Vec<(&str, Rect)> = Vec::new();
                if layout == Layout::Chest {
                    controls.push(("take all", bulk_button_rect(false)));
                    controls.push(("store all", bulk_button_rect(true)));
                    controls.push(("tidy pile", sort_button_rect()));
                }
                for (name, rect) in controls {
                    for (way, side) in [("across", rect.width()), ("down", rect.height())] {
                        assert!(
                            side >= widgets::FINGER_SIDE - SLACK,
                            "{kind:?}: {name} is {side:.3} {way} as authored, \
                             under a finger of {:.3} -- and a screen is never \
                             drawn smaller than it is authored",
                            widgets::FINGER_SIDE,
                        );
                        assert!(
                            side * drawn >= widgets::FINGER_SIDE - SLACK,
                            "{kind:?}: {name} is {:.0} device pixels {way} on the \
                             phone this was measured on, against a finger of 91",
                            side * drawn * 610.0,
                        );
                    }
                }
            }
        });
    }

    /// A caption belongs to the rows under it, on both halves.
    ///
    /// The screen used to write STORED and CARRIED above their grids and
    /// BELT below its strip, which left a player scanning it unable to
    /// tell by looking whether a word was a heading or a footnote. BELT
    /// is gone; this holds the other two to one rule.
    #[test]
    fn every_caption_on_a_chest_is_above_the_rows_it_names() {
        for side in [Side::Chest, Side::Pack] {
            let top_row = slot_rect(side, HOTBAR_SLOTS);
            let caption = caption_top(side);
            // `caption_top` is the top of the line and the glyphs hang
            // down from it, so clearing the row means the *bottom* of
            // the line clears it.
            let bottom = caption - widgets::cell_height(CAPTION_SCALE);
            assert!(
                bottom >= top_row.y1,
                "{side:?}: the caption reaches down to {bottom:.3}, into a row \
                 whose top is {:.3}",
                top_row.y1,
            );
            // ...and it is inside the tray it names, not floating over
            // the stone above it.
            let tray = match side {
                Side::Chest => chest_tray(),
                Side::Pack => pack_tray(),
            };
            assert!(
                caption <= tray.y1,
                "{side:?}: the caption is written above its own tray",
            );
        }
    }

    /// Everything the line under a hearth's fire can say in one language,
    /// found by asking `hearth_status` across every kind of fire: lit or
    /// out, at the start of every heat colour, wanting every heat colour,
    /// wet or dry, working or not, with supper in the tray or without.
    fn every_hearth_status(language: Language) -> Vec<String> {
        use primitive_shared::hearth::{Glow, OUTPUT_SLOTS};
        use primitive_shared::protocol::HearthState;
        let heats: Vec<f32> = std::iter::once(0.0)
            .chain(Glow::ALL.iter().skip(1).map(|glow| glow.starts_at() + 0.5))
            .collect();
        let mut supper = Inventory::new();
        supper.put_in_slot(
            OUTPUT_SLOTS.start,
            primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_COOKED_MEAT, 2),
        );
        let trays = [Inventory::new(), supper];
        let mut said = Vec::new();
        for fuel_left in [0.0, 60.0] {
            for &degrees in &heats {
                for &needs in &heats {
                    for wet in [false, true] {
                        for progress in [0.0, 0.5] {
                            for tray in &trays {
                                let fire = HearthState { fuel_left, progress, degrees, needs, wet };
                                let (text, _) = hearth_status(fire, tray, language);
                                if !said.contains(&text) {
                                    said.push(text);
                                }
                            }
                        }
                    }
                }
            }
        }
        said
    }

    #[test]
    fn the_heat_readout_stays_in_its_column_and_covers_no_slot_in_any_language() {
        // The heat's words sit in the one stretch of the fuel row nothing
        // else uses -- see `heat_rect` for the label that ruled out the row
        // above. Every colour word, in every language, has to fit it.
        use primitive_shared::hearth::{Glow, USED_SLOTS};
        const SCALE: f32 = 0.72;
        let overlaps = |a: Rect, b: Rect| a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0;
        let heat = heat_rect();
        let slots: Vec<(usize, Rect)> = (0..USED_SLOTS)
            .filter_map(|slot| hearth_slot_rect(slot).map(|rect| (slot, rect)))
            .collect();
        for &(slot, rect) in &slots {
            assert!(!overlaps(heat, rect), "the heat gauge covers hearth slot {slot}");
        }
        assert!(!overlaps(heat, flame_rect()), "the two gauges overlap");
        for language in Language::ALL {
            let mut lines = vec![(language.text(Msg::HearthHeat), heat_label_top())];
            for glow in Glow::ALL {
                lines.push((language.text(glow_msg(glow)), heat_word_top()));
            }
            for (text, top) in lines {
                let line = Rect::new(
                    heat.x0,
                    top - widgets::cell_height(SCALE),
                    heat.x0 + widgets::ink_width(text, SCALE),
                    top,
                );
                assert!(
                    line.x1 <= heat.x1,
                    "{language:?}: \"{text}\" runs out of the heat's column"
                );
                assert!(!overlaps(line, heat), "{language:?}: \"{text}\" is written on the gauge");
                assert!(
                    !overlaps(line, progress_rect()),
                    "{language:?}: \"{text}\" is written on the arrow"
                );
                for &(slot, rect) in &slots {
                    assert!(
                        !overlaps(line, rect),
                        "{language:?}: \"{text}\" covers hearth slot {slot}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_hearth_slot_is_clicked_where_it_is_drawn() {
        // The ash slot is new, and a slot that is drawn and cannot be
        // clicked is ash nobody can take out.
        use primitive_shared::hearth::USED_SLOTS;
        for slot in 0..USED_SLOTS {
            let rect = hearth_slot_rect(slot)
                .unwrap_or_else(|| panic!("hearth slot {slot} is used and never drawn"));
            assert_eq!(
                slot_at((rect.centre_x(), rect.centre_y()), Layout::Hearth),
                Some((Side::Chest, slot)),
                "hearth slot {slot} is drawn somewhere a click does not find it"
            );
        }
    }

    #[test]
    fn the_line_under_the_fire_names_the_first_thing_to_do() {
        use primitive_shared::hearth::{COPPER_MELTS_C, OUTPUT_SLOTS};
        use primitive_shared::protocol::HearthState;
        let language = Language::English;
        let fire = |fuel_left: f32, degrees: f32, needs: f32, wet: bool, progress: f32| HearthState {
            fuel_left,
            progress,
            degrees,
            needs,
            wet,
        };
        let empty = Inventory::new();
        let mut supper = Inventory::new();
        supper.put_in_slot(
            OUTPUT_SLOTS.start,
            primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_COOKED_MEAT, 2),
        );
        let says = |state: HearthState, tray: &Inventory| hearth_status(state, tray, language).0;
        let text = |msg: Msg| language.text(msg).to_string();

        assert_eq!(says(fire(0.0, 0.0, 0.0, false, 0.0), &empty), text(Msg::HearthUnlit));
        // Losing supper beats the rain that is also falling.
        assert_eq!(says(fire(60.0, 1000.0, 0.0, true, 0.0), &supper), text(Msg::HearthCharring));
        // The rain is named before the heat it is taking away.
        assert_eq!(
            says(fire(60.0, 500.0, COPPER_MELTS_C, true, 0.0), &empty),
            text(Msg::HearthRained)
        );
        // Too cool, and for what: copper wants orange.
        assert_eq!(
            says(fire(60.0, 500.0, COPPER_MELTS_C, false, 0.0), &empty),
            format!("{} {}", text(Msg::HearthTooCool), text(Msg::GlowOrange))
        );
        // Out and coasting: light it, rather than feed it something hotter.
        assert_eq!(
            says(fire(0.0, 500.0, COPPER_MELTS_C, false, 0.0), &empty),
            text(Msg::HearthCooling)
        );
        // Out, and still hot enough for what it is doing: that is work.
        assert_eq!(says(fire(0.0, 500.0, 200.0, false, 0.3), &empty), text(Msg::HearthWorking));
        assert_eq!(says(fire(60.0, 650.0, 0.0, false, 0.0), &empty), text(Msg::HearthIdle));
    }

    /// The line under a hearth's fire, saying what it is doing, never
    /// lands on top of a slot -- in any language.
    ///
    /// **The bug this is here for.** The line used to be centred on the
    /// arrow between the ingredients and the results rather than on the
    /// panel, in a margin with no row of its own -- see the note on
    /// [`hearth_height`]. `HearthUnlit` in English, "not burning --
    /// strike it with flint", fit that margin by luck; the Russian,
    /// "не горит -- подожгите кремнём", is wider than the gap it was
    /// squeezed into and was drawn straight over the bottom-left
    /// ingredient slot. Walking every language and every reason the
    /// line has to say is what `caption_top`'s own test does for the
    /// two grids; this holds the hearth to the same rule.
    #[test]
    fn the_hearth_status_line_never_covers_a_slot_however_long_the_words_are() {
        use primitive_shared::hearth::USED_SLOTS;
        const SCALE: f32 = 0.68;
        let slots: Vec<Rect> = (0..USED_SLOTS).filter_map(hearth_slot_rect).collect();
        assert!(!slots.is_empty(), "a hearth with no slots at all");
        let top = hearth_status_y();
        for language in Language::ALL {
            // **Every line `hearth_status` can say**, found by asking it
            // across every kind of fire rather than by listing messages
            // here: the list used to be three words long, and the first
            // reason added after it -- "not hot enough, needs:" with a
            // colour glued on -- is the longest thing the line says.
            for text in every_hearth_status(*language) {
                let text = text.as_str();
                let half = widgets::ink_width(text, SCALE) / 2.0;
                let line = Rect::new(-half, top - widgets::cell_height(SCALE), half, top);
                for (slot, rect) in slots.iter().enumerate() {
                    let overlaps = line.x0 < rect.x1
                        && line.x1 > rect.x0
                        && line.y0 < rect.y1
                        && line.y1 > rect.y0;
                    assert!(
                        !overlaps,
                        "{language:?}: \"{text}\" covers hearth slot {slot}",
                    );
                }
            }
        }
    }

    /// The two grids sit in two trays, and the trays do not touch.
    ///
    /// What the trays are for is the one question this screen asks --
    /// which side -- so a tray that reached into the other grid, or two
    /// that met in the middle, would be worse than none.
    #[test]
    fn each_half_of_a_chest_has_its_own_tray() {
        let panel = panel_rect(Layout::Chest);
        for (side, tray) in [(Side::Chest, chest_tray()), (Side::Pack, pack_tray())] {
            for slot in 0..SLOTS {
                let cell = slot_rect(side, slot);
                assert!(
                    cell.x0 >= tray.x0
                        && cell.x1 <= tray.x1
                        && cell.y0 >= tray.y0
                        && cell.y1 <= tray.y1,
                    "{side:?} slot {slot} hangs out of its own tray",
                );
            }
            for slot in 0..SLOTS {
                let other = match side {
                    Side::Chest => Side::Pack,
                    Side::Pack => Side::Chest,
                };
                let cell = slot_rect(other, slot);
                assert!(
                    cell.y0 > tray.y1 || cell.y1 < tray.y0,
                    "the {side:?} tray reaches into {other:?} slot {slot}",
                );
            }
            assert!(
                tray.x0 >= panel.x0 && tray.x1 <= panel.x1 && tray.y0 >= panel.y0,
                "the {side:?} tray hangs off the panel",
            );
        }
        // The action bar is the line between them, so it lies in the
        // gap and touches neither.
        let bar = action_bar_rect();
        assert!(bar.y1 < chest_tray().y0, "the action bar overlaps the chest's tray");
        assert!(bar.y0 > pack_tray().y1, "the action bar overlaps the pack's tray");
    }

    #[test]
    fn a_working_half_stands_in_a_tray_of_its_own_clear_of_the_pack() {
        for (name, layout) in [("hearth", Layout::Hearth), ("rack", Layout::Rack), ("jug", Layout::Vessel)] {
            let tray = work_tray(layout).expect("a working half has a tray");
            let panel = panel_rect(layout);
            assert!(
                tray.x0 >= panel.x0 && tray.x1 <= panel.x1 && tray.y0 >= panel.y0 && tray.y1 <= panel.y1,
                "the {name}'s tray hangs off its panel",
            );
            // The same sides as the pack's tray, so the two halves line up.
            assert!(
                (tray.x0 - pack_tray().x0).abs() < 1e-5 && (tray.x1 - pack_tray().x1).abs() < 1e-5,
                "the {name}'s tray and the pack's under it have different sides",
            );
            assert!(tray.y0 > pack_tray().y1, "the {name}'s tray runs into the pack's");
            let cells: Vec<Rect> = (0..SLOTS)
                .filter_map(|slot| match layout {
                    Layout::Hearth => hearth_slot_rect(slot),
                    Layout::Rack => rack_slot_rect(slot),
                    _ => vessel_slot_rect(slot),
                })
                .collect();
            assert!(!cells.is_empty(), "the {name} has no slots to hold");
            for cell in cells {
                assert!(
                    cell.x0 >= tray.x0 && cell.x1 <= tray.x1 && cell.y0 >= tray.y0 && cell.y1 <= tray.y1,
                    "a slot of the {name} hangs out of its own tray at {cell:?}",
                );
            }
        }
        assert!(work_tray(Layout::Chest).is_none(), "a chest already has its two trays");
    }

    #[test]
    fn no_word_on_a_hearth_or_a_rack_runs_off_its_panel_in_any_language() {
        // The captions over the results hang from the right now; before
        // they did, the plain English one ran out through the panel's side.
        use primitive_shared::protocol::{HearthState, RackState};
        for (index, language) in Language::ALL.iter().copied().enumerate() {
            for (kind, fire, weather) in [
                (
                    ContainerKind::Hearth(primitive_shared::hearth::Kind::Kiln),
                    Some(HearthState { fuel_left: 60.0, progress: 0.5, degrees: 900.0, needs: 1100.0, wet: false }),
                    None,
                ),
                (ContainerKind::Rack, None, Some(RackState { progress: 0.4, rate: 0.0, wet: true, near_fire: false })),
            ] {
                let mut screen = ChestScreen::new();
                screen.show((0, 0, 0), Inventory::new(), None, kind, fire, weather);
                let panel = panel_rect(Layout::of(kind));
                let vertices = screen.build(
                    crate::engine::texture::FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &Inventory::new(),
                    language,
                );
                let slack = widgets::SHADOW_OFFSET + 0.001;
                for vertex in &vertices {
                    let [x, y] = vertex.position;
                    if x.abs() > 4.0 {
                        continue; // the scrim
                    }
                    assert!(
                        x >= panel.x0 - 0.001 && x <= panel.x1 + slack && y >= panel.y0 - slack && y <= panel.y1 + 0.001,
                        "language {index}: something is drawn at ({x:.3}, {y:.3}), off a panel of \
                         {:.3}..{:.3} by {:.3}..{:.3}",
                        panel.x0,
                        panel.x1,
                        panel.y0,
                        panel.y1,
                    );
                }
            }
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
        screen.click(&pack, Button::Left, false, false);
        assert!(screen.held().is_some(), "nothing was picked up to begin with");

        let button = bulk_button_rect(true);
        screen.set_cursor(Some((button.centre_x(), button.centre_y())));
        assert_eq!(
            screen.click(&pack, Button::Left, false, false),
            Some(Intent::BulkMove { to_chest: true })
        );
        assert_eq!(screen.held(), None, "the held stack was dragged along");
    }
}

#[cfg(test)]
mod tests {
    use primitive_shared::protocol::ContainerKind;

    /// Every container screen can be got out of by tapping it.
    ///
    /// **The bug this exists for trapped the player.** A chest, a
    /// hearth and a rack are closed with Escape or the inventory key,
    /// and a phone has neither: the key is a button on the glass, and
    /// an open screen owns the glass -- so the button that opened the
    /// screen is the one thing that cannot close it. Tapping a chest on
    /// a phone meant killing the app to get out of it.
    ///
    /// All three layouts, because the hearth and the rack draw neither
    /// the tidy button nor the bulk buttons, and a way out that only
    /// exists on the screen that already had the most furniture is the
    /// way this would come back.
    #[test]
    fn every_container_screen_can_be_closed_by_tapping_it() {
        // **The way out, now that there is no button for it.** The `X`
        // in the header band was removed because it was a third way of
        // saying what Escape, the inventory key and the whole of the
        // rest of the glass already said -- and on a phone it was the
        // smallest of the four. What is left has to work on every
        // container, or a player is inside a kiln until they kill the
        // app.
        for kind in [
            ContainerKind::Chest,
            ContainerKind::Hearth(primitive_shared::hearth::Kind::Kiln),
            ContainerKind::Rack,
        ] {
            let mut screen = ChestScreen::new();
            let block = primitive_shared::types::BLOCK_CHEST;
            screen.show((0, 0, 0), Inventory::new(), Some(block), kind, None, None);

            let panel = panel_rect(Layout::of(kind));
            // Just outside the panel's own corner: the nearest a tap
            // can be to the screen and still be off it.
            let beside = (panel.x0 - 0.02, panel.centre_y());
            screen.set_cursor(Some(beside));
            assert_eq!(
                screen.click(&Inventory::new(), Button::Left, false, false),
                Some(Intent::Close),
                "{kind:?} could not be closed by tapping beside it",
            );
        }
    }

    /// Tapping beside the panel closes it -- unless something is held.
    ///
    /// The second way out, and the one a thumb can actually hit: the
    /// `X` in the header is about 24 device-independent pixels because
    /// the header band is not tall enough for the 44 a finger is
    /// usually given, so the large target is the rest of the screen.
    ///
    /// The exception is what stops it being annoying. A player holding
    /// a stack who taps beside the panel has almost always missed a
    /// slot, and closing on that would put the screen away and appear
    /// to swallow what they were carrying.
    #[test]
    fn tapping_beside_a_container_closes_it_but_only_with_empty_hands() {
        let mut contents = Inventory::new();
        contents.add(primitive_shared::types::BLOCK_COBBLESTONE, 64);
        let mut screen = ChestScreen::new();
        screen.show(
            (0, 0, 0),
            contents,
            Some(primitive_shared::types::BLOCK_CHEST),
            ContainerKind::Chest,
            None,
            None,
        );

        // Well outside any panel this screen draws.
        let outside = (5.0, 5.0);

        // With a stack in hand: put it back, stay open.
        let slot = chest_slot_rect(Side::Chest, 0);
        screen.set_cursor(Some((slot.centre_x(), slot.centre_y())));
        let _ = screen.click(&Inventory::new(), Button::Left, false, false);
        assert!(screen.held().is_some(), "nothing was picked up to test with");
        screen.set_cursor(Some(outside));
        assert_eq!(screen.click(&Inventory::new(), Button::Left, false, false), None);
        assert_eq!(screen.held(), None, "the stack was not put back");

        // Empty-handed: out.
        assert_eq!(
            screen.click(&Inventory::new(), Button::Left, false, false),
            Some(Intent::Close),
        );
    }


    /// **Nothing is drawn outside the panel** -- on either screen this
    /// file draws.
    ///
    /// The bug this catches is the one that kept coming back here: a
    /// label or a readout placed at a fixed offset from something that
    /// later moved, whose text then hangs over the panel's edge. The
    /// collisions *inside* the panel (a label written across the row it
    /// names) are a different failure and are caught by the eye; this
    /// one is caught by arithmetic.
    #[test]
    fn neither_screen_draws_outside_its_panel() {
        for hearth in [false, true] {
            let mut screen = ChestScreen::new();
            let mut pack = Inventory::new();
            pack.add(primitive_shared::types::BLOCK_COBBLESTONE, 128);
            pack.add(primitive_shared::types::BLOCK_LOG, 9);
            let (kind, fire, block) = if hearth {
                (
                    ContainerKind::Hearth(primitive_shared::hearth::Kind::Kiln),
                    // Hot, wet and waiting on a hotter fire: every piece of
                    // the heat readout drawn at once, so none of it can hang
                    // over the panel's edge unnoticed.
                    Some(primitive_shared::protocol::HearthState {
                        fuel_left: 60.0,
                        progress: 0.5,
                        degrees: 1580.0,
                        needs: primitive_shared::hearth::COPPER_MELTS_C,
                        wet: true,
                    }),
                    primitive_shared::types::BLOCK_KILN_LIT,
                )
            } else {
                (ContainerKind::Chest, None, primitive_shared::types::BLOCK_CHEST)
            };
            screen.show((0, 0, 0), pack.clone(), Some(block), kind, fire, None);

            let panel = panel_rect(Layout::of(kind));
            let slack = widgets::SHADOW_OFFSET + 0.001;
            let vertices = screen.build(
                crate::engine::texture::FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &pack,
                crate::ui::lang::Language::English,
            );
            for vertex in vertices.iter().skip(6) {
                let (x, y) = (vertex.position[0], vertex.position[1]);
                assert!(
                    x >= panel.x0 - 0.001
                        && x <= panel.x1 + slack
                        && y >= panel.y0 - slack
                        && y <= panel.y1 + 0.001,
                    "hearth={hearth}: something is drawn at ({x:.3}, {y:.3}), outside                      the panel ({:.3}..{:.3}, {:.3}..{:.3})",
                    panel.x0,
                    panel.x1,
                    panel.y0,
                    panel.y1
                );
            }
        }
    }

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
        screen.show(AT, chest, Some(BLOCK_CHEST), ContainerKind::Chest, None, None);
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
                    slot_at(centre_of(side, slot), Layout::Chest),
                    Some((side, slot)),
                    "{side:?} slot {slot}"
                );
            }
        }
        // ...and nothing at all in the gap between the two grids.
        let between = (0.0, slot_rect(Side::Pack, SLOTS - 1).y1 + grid_split() / 2.0);
        assert_eq!(slot_at(between, Layout::Chest), None);
    }

    /// A body opened with a rucksack's compartment: sixty squares, a
    /// stone in the last of them.
    fn a_body_with_a_rucksack() -> ChestScreen {
        use primitive_shared::inventory::{Stack, CORPSE_COMPARTMENT};
        use primitive_shared::types::{BLOCK_CORPSE, BLOCK_STONE};
        let mut body = Inventory::body(true);
        body.put_in_slot(CORPSE_COMPARTMENT.end - 1, Stack::new(BLOCK_STONE, 3));
        let mut screen = ChestScreen::new();
        screen.show(AT, body, Some(BLOCK_CORPSE), ContainerKind::Chest, None, None);
        screen
    }

    /// **The rucksack's compartment is clicked where it is drawn**: every
    /// one of its twenty squares is found under the place `build` draws it,
    /// the two bare rows under it are nobody's, a tap on a tab turns the
    /// page, and a chest -- forty long -- never grows the tabs at all.
    #[test]
    fn a_bodys_rucksack_compartment_is_clicked_where_it_is_drawn() {
        use primitive_shared::inventory::{BACKPACK_SLOTS, CORPSE_COMPARTMENT};
        let page = Layout::Body { rucksack: true };
        let mut reached = Vec::new();
        for place in 0..CHEST_SLOTS {
            let cell = chest_slot_rect(Side::Chest, place);
            let hit = slot_at((cell.centre_x(), cell.centre_y()), page);
            assert_eq!(hit, chest_square(page, place).map(|s| (Side::Chest, s)), "place {place}");
            reached.extend(hit.map(|(_, square)| square));
        }
        reached.sort_unstable();
        assert_eq!(reached, CORPSE_COMPARTMENT.collect::<Vec<_>>(), "a rucksack square has no place, or two");
        assert_eq!(reached.len(), BACKPACK_SLOTS);
        // The body's own page is a chest's forty, square for place.
        for place in 0..CHEST_SLOTS {
            let cell = chest_slot_rect(Side::Chest, place);
            assert_eq!(
                slot_at((cell.centre_x(), cell.centre_y()), Layout::Body { rucksack: false }),
                Some((Side::Chest, place))
            );
        }

        let mut screen = a_body_with_a_rucksack();
        assert_eq!(screen.layout(), Layout::Body { rucksack: false }, "a body opened on its rucksack");
        let tab = page_tab_rect(true);
        screen.set_cursor(Some((tab.centre_x(), tab.centre_y())));
        assert_eq!(screen.click(&Inventory::new(), Button::Left, false, false), None);
        assert_eq!(screen.layout(), Layout::Body { rucksack: true }, "the tab did not turn the page");
        // The stone in the compartment's last square, picked up where it
        // is drawn: the last place of the second row.
        let cell = chest_slot_rect(Side::Chest, HOTBAR_SLOTS + BACKPACK_SLOTS - 1);
        screen.set_cursor(Some((cell.centre_x(), cell.centre_y())));
        let _ = screen.click(&Inventory::new(), Button::Left, false, false);
        assert_eq!(screen.held(), Some((Side::Chest, CORPSE_COMPARTMENT.end - 1)));
        // Turning back keeps it in hand -- that is how it gets to the body.
        let tab = page_tab_rect(false);
        screen.set_cursor(Some((tab.centre_x(), tab.centre_y())));
        let _ = screen.click(&Inventory::new(), Button::Left, false, false);
        assert_eq!(screen.held(), Some((Side::Chest, CORPSE_COMPARTMENT.end - 1)), "the page dropped the stack");

        let mut chest = ChestScreen::new();
        chest.show(AT, Inventory::chest(), Some(primitive_shared::types::BLOCK_CHEST), ContainerKind::Chest, None, None);
        assert_eq!(chest.layout(), Layout::Chest, "a chest grew a rucksack");
    }

    /// **On a phone the tabs are a finger, inside the panel, clear of the
    /// title and of the tray under them** -- and the body's panel still
    /// fits a square window.
    #[test]
    fn a_bodys_tabs_are_a_finger_on_a_phone_and_stay_on_the_panel() {
        for touch in [false, true] {
            let check = || {
                let panel = panel_rect(Layout::Body { rucksack: false });
                assert!(panel.y1 < 1.0 && panel.y0 > -1.0, "the body's panel runs off a square window");
                for rucksack in [false, true] {
                    let tab = page_tab_rect(rucksack);
                    assert!(tab.x0 >= panel.x0 && tab.x1 <= panel.x1, "a tab left the panel");
                    assert!(tab.y1 <= panel.y1 - header_height(), "a tab is under the title");
                    assert!(tab.y0 >= chest_tray().y1, "a tab is on the tray");
                    if touch {
                        assert!(tab.height() >= widgets::FINGER_SIDE - 1e-4, "a tab under a finger");
                    }
                }
                assert!(page_tab_rect(false).x1 < page_tab_rect(true).x0, "the tabs overlap");
            };
            if touch {
                widgets::as_a_phone(check);
            } else {
                check();
            }
        }
    }

    /// A rack with skins on the frame, at whatever the sky is doing.
    fn a_rack(weather: primitive_shared::protocol::RackState) -> ChestScreen {
        use primitive_shared::rack::{HIDE_SLOT, LEATHER_SLOT};
        let mut screen = ChestScreen::new();
        let mut contents = Inventory::new();
        contents.put_in_slot(
            HIDE_SLOT,
            primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_HIDE, 4),
        );
        contents.put_in_slot(
            LEATHER_SLOT,
            primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_LEATHER, 2),
        );
        screen.show(
            AT,
            contents,
            Some(primitive_shared::types::BLOCK_DRYING_RACK),
            ContainerKind::Rack,
            None,
            Some(weather),
        );
        screen
    }

    /// Every state the rack's own line of words can be in.
    fn every_sky() -> Vec<primitive_shared::protocol::RackState> {
        use primitive_shared::protocol::RackState;
        vec![
            RackState { progress: 0.0, rate: 1.0, wet: false, near_fire: false },
            RackState { progress: 0.42, rate: 0.6, wet: false, near_fire: false },
            RackState { progress: 0.42, rate: 0.8, wet: false, near_fire: true },
            RackState { progress: 0.42, rate: 0.0, wet: true, near_fire: false },
            RackState { progress: 0.99, rate: 0.0, wet: false, near_fire: false },
        ]
    }

    #[test]
    fn the_rack_screen_stays_inside_its_panel_in_every_language() {
        // **This is the test the rack screen needed most.** Its readout
        // is not a word but a *sentence* -- "rain has stopped it, put it
        // under cover" -- written in four languages, and Polish is a
        // third longer than English in almost every string this game
        // has. A line that overruns the panel is drawn across the world
        // beside it, which reads as a bug rather than as a layout
        // choice.
        let panel = panel_rect(Layout::Rack);
        let slack = 0.02;
        for language in Language::ALL.iter().copied() {
            for weather in every_sky() {
                let screen = a_rack(weather);
                let vertices = screen.build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &stocked(),
                    language,
                );
                for v in &vertices {
                    let [x, y] = v.position;
                    if x.abs() > 4.0 {
                        continue; // the scrim
                    }
                    assert!(
                        x >= panel.x0 - slack && x <= panel.x1 + slack,
                        "{language:?} draws at x={x}, outside a panel of {}..{}",
                        panel.x0,
                        panel.x1
                    );
                    assert!(
                        y >= panel.y0 - slack && y <= panel.y1 + slack,
                        "{language:?} draws at y={y}, outside a panel of {}..{}",
                        panel.y0,
                        panel.y1
                    );
                }
            }
        }
    }

    #[test]
    fn every_rack_slot_finds_itself_and_the_pack_below_it() {
        // The hit test is told which screen is up; told the wrong one it
        // hands a click on the frame to whatever a chest has in that
        // corner, which is a stack of something going somewhere the
        // player did not ask for.
        for slot in 0..primitive_shared::rack::USED_SLOTS {
            let rect = rack_slot_rect(slot).expect("a rack slot");
            assert_eq!(
                slot_at((rect.centre_x(), rect.centre_y()), Layout::Rack),
                Some((Side::Chest, slot)),
                "rack slot {slot} misses itself"
            );
        }
        for slot in 0..SLOTS {
            let centre = centre_of(Side::Pack, slot);
            assert_eq!(slot_at(centre, Layout::Rack), Some((Side::Pack, slot)));
        }
        // ...and the rack's own slots are clear of the pack's grid, or
        // one of them would be unreachable.
        for slot in 0..primitive_shared::rack::USED_SLOTS {
            let rect = rack_slot_rect(slot).expect("a rack slot");
            assert!(
                rect.y0 > slot_rect(Side::Pack, SLOTS - 1).y1,
                "the rack's slot {slot} overlaps the pack"
            );
        }
    }

    #[test]
    fn a_rack_reports_what_the_weather_is_doing_to_it() {
        // The fingerprint the frame loop rebuilds on. Without the sky in
        // it, the bar is a photograph: a rack's contents do not change
        // for twelve minutes, so a key made of the slots alone says
        // nothing has happened for the whole of the wait.
        use primitive_shared::protocol::RackState;
        let still = a_rack(RackState { progress: 0.10, rate: 1.0, wet: false, near_fire: false });
        let later = a_rack(RackState { progress: 0.20, rate: 1.0, wet: false, near_fire: false });
        assert_ne!(still.ui_key(), later.ui_key(), "the drying bar never redraws");
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
        let panel = panel_rect(Layout::Chest);
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
        let panel = panel_rect(Layout::Chest);
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
        assert_eq!(screen.click(&pack, Button::Left, false, false), None, "the pick-up moved something");
        assert_eq!(screen.held(), Some((Side::Chest, 0)));

        screen.set_cursor(Some(centre_of(Side::Pack, 3)));
        assert_eq!(
            screen.click(&pack, Button::Left, false, false),
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
        screen.click(&pack, Button::Left, false, false);
        screen.set_cursor(Some(centre_of(Side::Chest, 5)));
        assert_eq!(
            screen.click(&pack, Button::Right, false, false),
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
            screen.click(&pack, Button::Left, true, false),
            Some(Intent::QuickMove(Side::Pack, 0))
        );
        assert_eq!(screen.held(), None, "a quick move picked something up as well");
    }

    #[test]
    fn picking_up_an_empty_slot_is_not_a_gesture() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 30)));
        assert_eq!(screen.click(&pack, Button::Left, false, false), None);
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn clicking_outside_the_grids_cancels_rather_than_losing_it() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false, false);
        assert!(screen.held().is_some());
        screen.set_cursor(Some((0.0, 0.95)));
        assert_eq!(screen.click(&pack, Button::Left, false, false), None);
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
        screen.click(&pack, Button::Left, false, false);
        assert!(screen.held().is_some());

        let mut emptied = Inventory::new();
        emptied.add(BLOCK_STONE, 1);
        screen.show(AT, emptied, None, ContainerKind::Chest, None, None);
        assert_eq!(screen.held(), None, "the gesture survived the chest changing");
    }

    #[test]
    fn opening_a_different_chest_starts_over() {
        let mut screen = opened();
        let pack = stocked();
        screen.set_cursor(Some(centre_of(Side::Chest, 0)));
        screen.click(&pack, Button::Left, false, false);
        screen.show((99, 40, 99), Inventory::new(), Some(BLOCK_CHEST), ContainerKind::Chest, None, None);
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

    #[test]
    fn an_update_already_on_its_way_does_not_reopen_a_container_the_player_shut() {
        // The hearth's next tick was in the socket when Escape was
        // pressed, and it opened the screen again under the player's hand.
        let mut screen = opened();
        screen.close_by_player();
        assert!(!screen.is_open());
        assert!(
            !screen.wants_state_for(AT),
            "an update sent before the server heard the close would open it again"
        );
        assert!(
            screen.wants_state_for((99, 40, 99)),
            "shutting one container refused every other"
        );
        // Asking again is a new question, and its answer is wanted.
        screen.asked_to_open();
        assert!(screen.wants_state_for(AT), "the player asked and was refused");
    }

    #[test]
    fn a_container_the_server_shut_is_not_held_against_the_next_answer() {
        // A death or a broken chest is the server letting go, not the
        // player turning something down.
        let mut screen = opened();
        screen.close();
        assert!(screen.wants_state_for(AT));
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
        screen.show(AT, chest, Some(BLOCK_CHEST), ContainerKind::Chest, None, None);

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
        screen.show(AT, Inventory::new(), Some(BLOCK_BACKPACK), ContainerKind::Chest, None, None);
        assert_eq!(screen.heading(), Msg::Backpack);

        // An update with no block -- the chunk has not arrived, or this
        // is the answer to a gesture rather than to an open -- must not
        // quietly rename it.
        screen.show(AT, Inventory::new(), None, ContainerKind::Chest, None, None);
        assert_eq!(screen.heading(), Msg::Backpack, "an update renamed the screen");

        // ...and moving to a real chest goes back to the other word.
        screen.show((1, 2, 3), Inventory::new(), Some(BLOCK_CHEST), ContainerKind::Chest, None, None);
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
                screen.show(AT, Inventory::new(), Some(block), ContainerKind::Chest, None, None);
                let vertices = screen.build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &stocked(),
                    language,
                );
                let panel = panel_rect(Layout::Chest);
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
        screen.show(AT, Inventory::new(), Some(BLOCK_CHEST), ContainerKind::Chest, None, None);
        screen.set_cursor(None);
        assert_eq!(screen.click(&stocked(), Button::Left, false, false), None);
    }
}

#[cfg(test)]
mod vessel_tests {
    use super::*;
    use primitive_shared::inventory::{filled_jug, Stack, JUG_UNITS, VESSEL_SLOT};
    use primitive_shared::protocol::{ClientMessage, ContainerKind};
    use primitive_shared::types::{jug_of, BLOCK_GRAIN, BLOCK_JUG, BLOCK_STONE};

    /// Where the jug sits in the pack in every test here: not slot zero,
    /// so a mapping that confused "the jug's slot" with "the jug's one
    /// slot" would show.
    const JUG: usize = 2;

    fn a_pack_with_grain_in_the_jug(grain: u32) -> Inventory {
        let mut pack = Inventory::new();
        pack.put_in_slot(JUG, filled_jug(BLOCK_GRAIN, grain));
        pack.put_in_slot(4, Stack::new(BLOCK_GRAIN, 30));
        pack
    }

    #[test]
    fn every_vessel_slot_is_clicked_where_it_is_drawn() {
        let rect = vessel_slot_rect(VESSEL_SLOT).expect("a jug has its slot");
        assert_eq!(
            slot_at((rect.centre_x(), rect.centre_y()), Layout::Vessel),
            Some((Side::Chest, VESSEL_SLOT)),
            "the jug's slot misses itself"
        );
        assert_eq!(vessel_slot_rect(VESSEL_SLOT + 1), None, "a jug grew a second slot");
        for slot in 0..SLOTS {
            let cell = slot_rect(Side::Pack, slot);
            assert_eq!(
                slot_at((cell.centre_x(), cell.centre_y()), Layout::Vessel),
                Some((Side::Pack, slot)),
                "pack slot {slot} under a jug"
            );
        }
        assert!(
            rect.y0 > slot_rect(Side::Pack, SLOTS - 1).y1,
            "the jug's slot overlaps the pack"
        );
        let panel = panel_rect(Layout::Vessel);
        assert!(rect.x0 >= panel.x0 && rect.x1 <= panel.x1 && rect.y1 <= panel.y1);
    }

    #[test]
    fn the_jug_screen_stays_inside_its_panel_in_every_language() {
        // Both lines under the slot are sentences, and the empty one names
        // four goods -- the longest words this screen has, in whichever
        // language makes them longest.
        let panel = panel_rect(Layout::Vessel);
        let slack = 0.02;
        for language in Language::ALL.iter().copied() {
            for grain in [0, 7, JUG_UNITS] {
                let pack = a_pack_with_grain_in_the_jug(grain);
                let mut screen = ChestScreen::new();
                screen.show_held_vessel(JUG, &pack);
                let vertices = screen.build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &pack,
                    language,
                );
                assert!(!vertices.is_empty(), "an open jug drew nothing");
                for v in &vertices {
                    let [x, y] = v.position;
                    if x.abs() > 4.0 {
                        continue; // the scrim
                    }
                    assert!(
                        x >= panel.x0 - slack && x <= panel.x1 + slack,
                        "{language:?}, {grain} grain: drawn at x={x}, outside {}..{}",
                        panel.x0,
                        panel.x1
                    );
                    assert!(
                        y >= panel.y0 - slack && y <= panel.y1 + slack,
                        "{language:?}, {grain} grain: drawn at y={y}, outside {}..{}",
                        panel.y0,
                        panel.y1
                    );
                }
            }
        }
    }

    #[test]
    fn the_jug_screens_words_never_cover_its_slot_or_the_pack() {
        use crate::ui::widgets::cell_height;
        let slot = vessel_slot_rect(VESSEL_SLOT).expect("a jug has its slot");
        let pack_top = slot_rect(Side::Pack, HOTBAR_SLOTS).y1;
        assert!(vessel_reading_y() <= slot.y0, "the reading is written across the slot");
        let status_bottom = vessel_status_y() - cell_height(0.68);
        assert!(
            vessel_status_y() <= vessel_reading_y() - cell_height(0.72),
            "the status line is written across the reading"
        );
        assert!(status_bottom > pack_top, "the status line is written across the pack");
    }

    #[test]
    fn the_jug_in_the_hand_cannot_be_poured_into_itself() {
        let into_the_jug = |from| Intent::Move {
            from: (Side::Pack, from),
            to: (Side::Chest, VESSEL_SLOT),
            half: false,
        };
        assert!(held_vessel_message(into_the_jug(JUG), JUG).is_none(), "the jug was poured into itself");
        assert!(held_vessel_message(Intent::QuickMove(Side::Pack, JUG), JUG).is_none());
        // ...nor dragged away from under the screen looking into it.
        for (from, to) in [(JUG, 5), (5, JUG)] {
            let intent = Intent::Move { from: (Side::Pack, from), to: (Side::Pack, to), half: false };
            assert!(held_vessel_message(intent, JUG).is_none(), "the open jug moved {from} -> {to}");
        }
        let taken_into_it = Intent::Move {
            from: (Side::Chest, VESSEL_SLOT),
            to: (Side::Pack, JUG),
            half: true,
        };
        assert!(held_vessel_message(taken_into_it, JUG).is_none());

        // Every other square is an ordinary pour, take or move.
        assert!(matches!(
            held_vessel_message(into_the_jug(4), JUG),
            Some(ClientMessage::PourIntoJug { from: 4, jug: 2 })
        ));
        assert!(matches!(
            held_vessel_message(
                Intent::Move { from: (Side::Chest, VESSEL_SLOT), to: (Side::Pack, 7), half: true },
                JUG
            ),
            Some(ClientMessage::TakeFromJug { jug: 2, to: 7, half: true })
        ));
        assert!(matches!(
            held_vessel_message(Intent::QuickMove(Side::Chest, VESSEL_SLOT), JUG),
            Some(ClientMessage::EmptyJug { slot: 2 })
        ));
        assert!(matches!(
            held_vessel_message(
                Intent::Move { from: (Side::Pack, 4), to: (Side::Pack, 8), half: true },
                JUG
            ),
            Some(ClientMessage::SplitSlot { from: 4, to: 8 })
        ));
        assert!(held_vessel_message(Intent::BulkMove { to_chest: true }, JUG).is_none());
    }

    #[test]
    fn a_jug_open_in_the_hand_shows_what_is_in_it_and_shuts_when_it_leaves() {
        let pack = a_pack_with_grain_in_the_jug(7);
        let mut screen = ChestScreen::new();
        screen.show_held_vessel(JUG, &pack);
        assert!(screen.is_open());
        assert_eq!(screen.held_vessel(), Some(JUG));
        assert_eq!(screen.layout(), Layout::Vessel);
        assert_eq!(screen.heading(), Msg::Jug);
        assert_eq!(screen.contents.count_in(VESSEL_SLOT), 7);

        // A pour lands: the next pack snapshot is the next picture of it.
        let poured = a_pack_with_grain_in_the_jug(12);
        screen.sync_with(&poured);
        assert_eq!(screen.contents.count_in(VESSEL_SLOT), 12, "the screen kept an old jug");

        // The jug is thrown out: the screen shuts rather than guessing.
        let mut gone = poured.clone();
        gone.take_slot(JUG);
        screen.sync_with(&gone);
        assert!(!screen.is_open(), "a screen stayed open on a jug that is not there");
    }

    #[test]
    fn only_a_jug_that_can_hold_goods_opens_in_the_hand() {
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE, 3));
        pack.put_in_slot(1, Stack::new(jug_of(primitive_shared::body::Water::Fresh), 1));
        pack.put_in_slot(2, Stack::new(BLOCK_JUG, 1));
        for slot in [0, 1, 5] {
            let mut screen = ChestScreen::new();
            screen.show_held_vessel(slot, &pack);
            assert!(!screen.is_open(), "slot {slot} opened as a jug");
        }
        let mut screen = ChestScreen::new();
        screen.show_held_vessel(2, &pack);
        assert!(screen.is_open(), "an empty jug would not open");
    }

    #[test]
    fn a_jug_on_a_table_is_the_same_screen_and_can_be_tapped_shut() {
        let mut contents = Inventory::new();
        contents.put_in_slot(VESSEL_SLOT, Stack::new(BLOCK_GRAIN, 3));
        let mut screen = ChestScreen::new();
        screen.show((1, 2, 3), contents, Some(BLOCK_JUG), ContainerKind::Vessel, None, None);
        assert_eq!(screen.layout(), Layout::Vessel);
        assert_eq!(screen.heading(), Msg::Jug);
        assert_eq!(screen.held_vessel(), None, "a jug on a table was taken for the jug in hand");

        let panel = panel_rect(Layout::Vessel);
        screen.set_cursor(Some((panel.x0 - 0.02, panel.centre_y())));
        assert_eq!(screen.click(&Inventory::new(), Button::Left, false, false), Some(Intent::Close));
    }
}
