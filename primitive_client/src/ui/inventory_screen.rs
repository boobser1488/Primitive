//! The inventory screen, opened with `I`.
//!
//! ## Interaction
//!
//! Click a slot to pick its stack up, click another to put it down.
//! Click-then-click rather than drag, because a drag has to survive the
//! cursor leaving the window, being released over nothing, and the
//! screen being closed mid-gesture, and each of those is a way to lose a
//! stack.
//!
//! Four gestures, all of them click-then-click:
//!
//! * **Left, left** -- move a stack. Onto the same block it merges;
//!   onto anything else it swaps.
//! * **Left, right** -- place half of what is held, and keep holding
//!   the rest, so a stack can be dealt out across several slots.
//! * **Shift + left** -- send a stack between the bar and the pile
//!   behind it, without picking anything up.
//! * **Left/right on a recipe** -- make one, or make as many as the
//!   ingredients allow.
//!
//! Nothing is ever destroyed: a click on empty space outside the grid
//! puts the held stack back where it came from.
//!
//! **A dressing is put on the body the same way.** Pick a bandage, a
//! splint or a poultice up out of the pack and put it down on the part of
//! the figure that needs it; the parts it would help are lit while it is
//! in hand (see `ui::mannequin`). Click-then-click here too, and not a
//! press-drag-release, for the reason at the top of this note -- and
//! because on a phone the two are the same two taps, so there is one
//! gesture to learn on both.
//!
//! **There is no close button.** Escape, the inventory key, or a tap
//! anywhere off the panel -- which on a phone is the way out, with the
//! system's Back gesture behind it. See `Intent::Close`.
//!
//! Nothing is edited here either. Every gesture turns into an `Intent`
//! for the server, which owns the inventory; the screen only draws the
//! snapshot it is sent. `sync` is what keeps a pick-up honest when that
//! snapshot changes underneath it -- see the note there.
//!
//! ## Layout
//!
//! The hotbar is the bottom row of the same grid, not a separate widget.
//! It is the same ten slots the bar draws, so moving something into the
//! bottom row of the inventory is exactly moving it onto the hotbar, and
//! the player never has to learn that they are two different things.

use crate::ui::hotbar::HotbarVertex;
use crate::logic::inventory::{Inventory, HOTBAR_SLOTS, SLOTS};
use primitive_shared::inventory::{BACKPACK_SLOTS, STORAGE_ROWS};
use crate::engine::texture::{FaceLayers, FontAtlas, FACE_SOUTH, FACE_TOP};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};
use primitive_shared::injury::{Injuries, Part, Treatment};

// ---- layout ----
//
// The screen is two columns -- the slot grid and the recipe list -- and
// the pair is centred as a whole.
//
// The first version centred the *grid* and hung the recipes off to the
// right. That put the panel's right edge at x = 1.47 in a space that
// only runs to ±aspect, so on anything narrower than about 3:2 the
// crafting column was simply off the screen, and on a 16:9 monitor the
// whole screen sat visibly off to one side.
//
// The two columns are also no longer the same height. Recipe rows carry
// icons now, which makes them taller than a row of slots, so the panel
// is sized to whichever column is taller (`content_height`) and the
// grid is centred inside that band. Sizing to the grid was what kept
// the recipe rows short enough to be a wall of text.
//
// Everything below is derived from the cell size so there is one number
// to change, and a test checks the result fits a *square* window, which
// is the worst case the aspect divide can produce.

/// Side of one slot, in UI units.
///
/// `pub(crate)` because the chest screen is the same grid of the same
/// cells: two screens that drew slots at two sizes would read as two
/// games. See `draw_slot`, which both of them go through.
pub(crate) const CELL: f32 = 0.096;
pub(crate) const GAP: f32 = 0.011;
/// Space between the slot grid and the recipe column.
///
/// Tightened when the body column arrived. The screen is three columns
/// now rather than two, and on a square window -- the worst case the
/// aspect divide can produce -- the three of them plus their padding is
/// what has to fit inside two units of width. What gave was the air
/// between them rather than the size of a slot: a smaller cell would
/// have made the pack read differently from the hotbar it is drawn to
/// match, and that match is the point of `CELL` being shared.
const COLUMN_GAP: f32 = 0.030;
/// Space between the storage rows and the hotbar row, so the bar reads
/// as the thing it is rather than as a fourth row.
pub(crate) const HOTBAR_SPLIT: f32 = 0.030;

/// The margin between the panel's edge and what is in it.
///
/// Tightened with `COLUMN_GAP`, and for the same reason -- see the note
/// there.
pub(crate) const PANEL_PAD: f32 = 0.030;
/// Room above the first row of slots for the word "PACK".
const GRID_LABEL: f32 = 0.046;
/// Room at the top of the panel for the title, and at the bottom for the
/// readout and the three lines of controls. Named, because "why is there
/// a gap" is exactly the question a bare number in a `Rect::new` never
/// answers.
///
/// **A function, because the band has to hold the tidy button**, which is
/// `widgets::tappable` -- a finger's height where a finger presses it.
///
/// It used to be sized by the way out: an `X` at the band's left end, 72
/// device pixels against a floor of 91 on the phone it was measured on,
/// and floored to a finger for that reason. The `X` is gone (see
/// `Intent::Close`), so the band is what the button beside the title needs
/// plus the air it always had. On a desktop that comes back to 0.100
/// exactly, which is what it was: nothing a mouse points at has moved.
fn header_height() -> f32 {
    widgets::tappable(SORT_HEIGHT) + 0.048
}
/// Room under the grid for the one line of readout, and nothing else.
///
/// It used to be three times this: four lines of control hints -- every
/// gesture the screen understands, spelled out permanently -- and then
/// two lines of numbers under them. They cost a third of the panel's
/// height to say what a player reads once, and the panel is a window
/// over the world.
///
/// A test measures the line that is left against this, because the
/// first guess at it was too small and put the readout through the
/// bottom edge of the panel.
const FOOTER_HEIGHT: f32 = 0.150;
/// How far the readout's baseline sits above the panel's floor.
const FOOTER_MARGIN: f32 = 0.028;

/// The plate a stack count sits on, so a white number over a pale block
/// texture is still readable.
/// How far the count's shadow is offset, and what colour it is.
const COUNT_SHADOW: f32 = 0.0034;
const COUNT_DARK: [f32; 4] = [0.13, 0.13, 0.13, 1.0];
const COUNT_SCALE: f32 = 0.72;
const COUNT_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// A stack at the slot limit is worth pointing out: it is the reason
/// the next pickup will claim another slot.
const COUNT_FULL: [f32; 4] = [1.0, 0.80, 0.35, 1.0];

/// A recipe the player cannot run: the cell is drawn as any other and
/// then veiled, rather than being given a colour scheme of its own.
///
/// The veil is over the *whole* cell, icon included, so an unavailable
/// recipe recedes as one thing. Dimming the icon alone leaves a bright
/// frame around a grey picture, which reads as a slot with a problem
/// rather than as a recipe that is not yet possible.
const RECIPE_VEIL: [f32; 4] = [0.04, 0.045, 0.06, 0.62];

/// What an empty body square's ghost garment is drawn in.
///
/// Grey rather than the material's own colour, and translucent: a cap
/// drawn in leather brown at full strength is a leather cap *lying in
/// the slot*, which is exactly the thing this square is trying to say it
/// has not got. Measured against the well it sits in it comes to 3.4:1
/// -- the quiet tier's floor, which is where a thing that is a hint
/// rather than a fact belongs.
const GHOST: [f32; 4] = [0.62, 0.58, 0.54, 0.45];

/// Which picture stands for an empty body square.
///
/// The leather set, because it is the one every player meets first --
/// and because the picture is the *garment*, not the material: twelve
/// garments share these four images and the material is a tint over them
/// (`types::garment_tint`), so any of the three sets would draw the same
/// cap. Leather is simply the one whose name the player will have read.
fn ghost_garment(part: usize) -> Option<primitive_shared::types::BlockId> {
    use primitive_shared::equipment::Slot;
    use primitive_shared::types::{
        BLOCK_LEATHER_BOOTS, BLOCK_LEATHER_CAP, BLOCK_LEATHER_LEGGINGS, BLOCK_LEATHER_TUNIC,
    };
    Some(match Slot::from_index(part)? {
        Slot::Head => BLOCK_LEATHER_CAP,
        Slot::Chest => BLOCK_LEATHER_TUNIC,
        Slot::Legs => BLOCK_LEATHER_LEGGINGS,
        Slot::Feet => BLOCK_LEATHER_BOOTS,
        // The rucksack's own picture, which is the bag's -- see the
        // `rucksack` entry in `blocks.toml` for why they share it. The
        // ghost is the item picture drawn faint, exactly as the four
        // garments' are, so an empty back square says "a rucksack goes
        // here" in every language at once.
        Slot::Back => primitive_shared::types::BLOCK_RUCKSACK,
    })
}

/// What to call a body part, in the language being read.
///
/// One place, so the note under the pointer and anything that comes
/// after it cannot disagree about which square is which.
fn part_name(part: usize, language: Language) -> Option<&'static str> {
    use primitive_shared::equipment::Slot;
    Some(language.text(match Slot::from_index(part)? {
        Slot::Head => Msg::SlotHead,
        Slot::Chest => Msg::SlotChest,
        Slot::Legs => Msg::SlotLegs,
        Slot::Feet => Msg::SlotFeet,
        Slot::Back => Msg::SlotBack,
    }))
}

/// Whether dropping the stack in `from` onto the slot `jug` would pour
/// rather than move.
///
/// The client's copy of the rule the server enforces in
/// `pour_into_jug`. It is here only to decide *which gesture this
/// click is*: a wrong answer sends a message the server refuses, which
/// is a click that does nothing rather than a rule that has been got
/// round.
fn pours_into(inventory: &Inventory, from: usize, jug: usize) -> bool {
    let (Some(source), Some(vessel)) = (
        inventory.slots().get(from).copied().flatten(),
        inventory.slots().get(jug).copied().flatten(),
    ) else {
        return false;
    };
    if !primitive_shared::types::pours(source.block) {
        return false;
    }
    if primitive_shared::types::block_kind(vessel.block) != primitive_shared::types::BLOCK_JUG {
        return false;
    }
    match primitive_shared::inventory::jug_contents(&vessel) {
        // Empty: anything that pours may go in.
        None => true,
        // Already holding these goods, and not yet full. A jug holding
        // something *else* is left alone, so dropping sand on a jug of
        // grain is the ordinary swap it looks like rather than a
        // silently refused pour.
        Some((held, count)) => {
            held == source.block && count < primitive_shared::inventory::JUG_UNITS
        }
    }
}

/// What a click on the screen wants the server to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Merge onto the same block, swap with anything else.
    Move { from: usize, to: usize },
    /// Half of `from` into `to`, which must be empty or the same block.
    Split { from: usize, to: usize },
    /// Between the hotbar and the storage rows, whichever way round.
    QuickMove(usize),
    /// `times` is how many the player asked for, not how many are
    /// possible; the server makes as many of them as it can.
    Craft { index: usize, times: u8 },
    Sort,
    /// Put on whatever is in this slot of the pack.
    ///
    /// A slot rather than a block, and no destination: the body part a
    /// garment goes on is a fact about the garment
    /// (`equipment::slot_of`), so there is nothing here for the player
    /// to get wrong or for the client to lie about.
    Equip(usize),
    /// Take off what is on this body part. The index is into
    /// `equipment::ALL_SLOTS`.
    Unequip(usize),
    /// Pour the loose goods held in `from` into the jug in `jug`.
    PourIntoJug { from: usize, jug: usize },
    /// Tip the jug in this slot back out into the pack.
    EmptyJug(usize),
    /// Put what is in `slot` on a part of the body: a bandage dropped on
    /// a bleeding arm. Whether it suits the wound is the server's to say
    /// (`injury::Injuries::treat`); the screen asks only for something
    /// that is a dressing at all.
    Treat { slot: usize, part: Part },
    /// Put the screen away.
    ///
    /// **There is no button for this any more**, on the chest screen's
    /// reasoning and at the player's request. The `X` in the header was the
    /// fourth way of saying one thing -- Escape, the inventory key and a
    /// tap anywhere off the panel all close the pack, and on a phone Escape
    /// is the system's Back gesture -- and the one that took room from the
    /// title band. What a phone has now is the tap off the panel, with Back
    /// behind it, and `a_phone_always_has_somewhere_off_the_pack_to_tap`
    /// holds the first of those true at every interface size.
    ///
    /// The cost is the chest's cost: a tap beside the panel with a stack in
    /// hand puts the stack back and stays open, so leaving while holding
    /// something is two taps, or Back.
    Close,
}

/// Which mouse button a click came from. Its own type rather than
/// winit's, so the screen and its tests do not depend on the windowing
/// library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// Which of the three pages the pack screen is showing.
///
/// ## Why three pages and not three screens
///
/// They are one screen because they are one gesture away from each
/// other and because two of them share a grid. The player's own squares
/// are drawn in the same place on the pack page and the rucksack page --
/// see [`slot_in_place`] -- so switching tabs moves one row of ten and
/// nothing else, which is the whole reason a tab is the right control
/// here rather than a second key and a second panel.
///
/// The health page is the odd one and earns its place by what it
/// replaces: the readings it shows were spread across a gauge on the
/// HUD, a bar that only appears when it is bad, a colour on a number and
/// four things that were never shown at all. A player who wants to know
/// why they are slow had nowhere to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    /// Every vital the server sends, read down a page.
    Health,
    /// What the pack has always shown.
    #[default]
    Pack,
    /// The worn rucksack's squares, in the row the pack's own storage
    /// occupies on the page before.
    Backpack,
}

/// The three, left to right, in the order they are drawn and indexed.
///
/// **The pack is in the middle**, which is where the eye and the thumb
/// both start, because it is the page the screen is opened for nine
/// times out of ten. The body is to its left because a player reads left
/// to right and "how am I" comes before "what have I"; the rucksack is
/// to its right because it is the pack's overflow and it is not always
/// there.
pub const ALL_TABS: [Tab; 3] = [Tab::Health, Tab::Pack, Tab::Backpack];

impl Tab {
    fn label(self) -> Msg {
        match self {
            Tab::Health => Msg::TabHealth,
            Tab::Pack => Msg::TabPack,
            Tab::Backpack => Msg::TabBackpack,
        }
    }
}

/// **The rucksack's squares fit in the pack's storage rows.**
///
/// [`slot_in_place`] draws them in those rows' places, from the first, so
/// there must be room: the rucksack is half the pack (`BACKPACK_SLOTS`) and
/// the rows above the belt are the rest of it. Checked at compile time,
/// because it is a relation between two constants. Places past the
/// rucksack's last square are no square at all on its page.
const _: () = assert!(BACKPACK_SLOTS <= SLOTS - HOTBAR_SLOTS);

/// Which slot of the inventory is drawn in grid position `place`.
///
/// The belt is the belt on every page: position 0..`HOTBAR_SLOTS` is
/// always the player's own first ten squares, so the row that is on
/// screen during play never moves and never means something else. What
/// changes is the row above it -- the player's own storage on
/// [`Tab::Pack`], the rucksack's ten on [`Tab::Backpack`].
///
/// **This is the only difference between those two pages**, and it is
/// deliberate: the body column, the crafting column and the trays are
/// all still drawn, so switching tabs is not a screen change. Rejected:
/// a page with only the rucksack on it, which would have meant putting
/// the rucksack *on* was impossible from the page the rucksack is on.
///
/// The cost is that the pack's own storage and the rucksack are never on
/// screen together, so moving a stack between them is two gestures
/// through the belt -- or one shift-click, which sends a rucksack stack
/// to the belt (`Inventory::quick_move`).
///
/// `None` for a place on the rucksack page past the rucksack's last square:
/// the pack's storage is three rows and the rucksack two, and a place that
/// answered with a slot the pack does not have was a square drawn empty that
/// no click could fill.
pub fn slot_in_place(tab: Tab, place: usize) -> Option<usize> {
    if tab == Tab::Backpack && place >= HOTBAR_SLOTS {
        let square = place - HOTBAR_SLOTS;
        (square < BACKPACK_SLOTS).then_some(SLOTS + square)
    } else {
        Some(place)
    }
}

/// What a right click on a recipe asks for.
///
/// Not `u8::MAX`: the server runs the loop, and a bound that is merely
/// large is a bound a slow tick can feel. Sixty-four is more than a full
/// pack of any ingredient can make in one go.
pub const CRAFT_MANY: u8 = 64;

#[derive(Default)]
pub struct InventoryScreen {
    /// See `set_heat`.
    heat: primitive_shared::crafting::Heat,
    pub open: bool,
    /// The slot a click picked up from. The stack stays *in* that slot
    /// until it lands somewhere -- nothing is held in limbo, so closing
    /// the screen at any moment cannot lose it.
    held: Option<usize>,
    /// What was in that slot when it was picked up, so a snapshot that
    /// changes it underneath the player can cancel the gesture instead of
    /// completing it against something else. See `sync`.
    held_block: Option<primitive_shared::types::BlockId>,
    /// Last known cursor position in UI space.
    cursor: Option<(f32, f32)>,
    /// Which of the three pages is showing.
    ///
    /// Kept on the screen rather than in the settings, and deliberately
    /// *not* reset by `close`: a player who was looking at the rucksack
    /// and pressed `I` twice means to be looking at the rucksack. It is
    /// reset by `open_at` only when the page it is on cannot be drawn --
    /// see there.
    tab: Tab,
    /// First recipe row on screen.
    ///
    /// The list scrolls rather than the panel growing. Recipes are
    /// added over time -- the fourteenth is what pushed the screen off
    /// the bottom of a square window -- and a menu that gets taller
    /// with every addition eventually cannot be drawn at all, on a
    /// monitor nobody has yet.
    recipe_scroll: usize,
}

impl InventoryScreen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn close(&mut self) {
        self.open = false;
        self.release();
    }

    fn release(&mut self) {
        self.held = None;
        self.held_block = None;
    }

    /// Scrolls the recipe list. Positive is down the list.
    ///
    /// Clamped at both ends rather than wrapping: a list that jumps back
    /// to the top when you overscroll costs the player the place they
    /// had found.
    /// Bounded by the whole table rather than by what is on offer: the
    /// offered list shrinks as the pack empties, and a scroll position
    /// clamped against it would silently walk back to the top every time
    /// a recipe stopped being possible. What is drawn is clamped instead,
    /// at the moment of drawing -- see `visible_scroll`.
    pub fn scroll_recipes(&mut self, rows: i32) {
        let last = primitive_shared::crafting::RECIPES
            .len()
            .saturating_sub(visible_recipes());
        self.recipe_scroll = (self.recipe_scroll as i32 + rows).clamp(0, last as i32) as usize;
    }

    /// How far down the list it is scrolled.
    ///
    /// Read by the tests; the drawing code has it from `self`. Kept
    /// because "where is the list" is the state this screen has, and a
    /// screen whose state cannot be asked about is a screen that cannot
    /// be tested.
    #[allow(dead_code)]
    pub fn recipe_scroll(&self) -> usize {
        self.recipe_scroll
    }

    /// Which page is showing.
    ///
    /// `allow(dead_code)` for `recipe_scroll`'s reason: the drawing code
    /// has it from `self`, and a screen whose state cannot be asked
    /// about is a screen that cannot be tested.
    #[allow(dead_code)]
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Turns to a page. Any pick-up in progress is dropped: the stack it
    /// came from may not be drawn on the new page at all, and a gesture
    /// whose source is off screen is a gesture that finishes somewhere
    /// the player cannot see.
    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.release();
        }
    }

    pub fn set_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.cursor = cursor;
    }

    /// Whether the player is standing beside a lit fire.
    ///
    /// State on the screen rather than an argument threaded through
    /// every drawing function, for the same reason the cursor is: it is
    /// a fact about the moment that half a dozen things need and none of
    /// them owns. The frame loop sets it from the world; what the screen
    /// does with it is grey out the recipes that cannot be run here.
    ///
    /// It is only ever *advice*. The server asks the same question
    /// against its own copy of where the player is standing, and a
    /// client that lied here would have its craft refused -- see
    /// `beside_a_fire` on the server.
    pub fn set_heat(&mut self, heat: primitive_shared::crafting::Heat) {
        self.heat = heat;
    }

    /// Exposed for the tests, which assert on the pick-up/put-down
    /// state machine rather than on the pixels it produces.
    #[allow(dead_code)]
    pub fn held(&self) -> Option<usize> {
        self.held
    }

    /// Reconciles a pick-up with a fresh snapshot from the server.
    ///
    /// A held slot is only an *index*, and between picking up and putting
    /// down the server can change what is in it -- an item walked over, a
    /// craft finishing, another gesture landing. Completing the move then
    /// sends whatever happens to be there now, which is how a player
    /// throws away something they never touched. If the block under the
    /// pick-up is not the one they took hold of, the gesture is dropped.
    pub fn sync(&mut self, inventory: &Inventory) {
        self.follow_the_rucksack(inventory);
        if let Some(slot) = self.held {
            if inventory.block_in(slot) != self.held_block || self.held_block.is_none() {
                self.release();
            }
        }
    }

    /// Handles a click, returning what the server should be asked to do.
    ///
    /// `quick` is the shift-click modifier.
    ///
    /// The screen never edits the inventory itself. It cannot: the
    /// server owns it, and a click that moved a stack locally would be
    /// undone by the next snapshot -- visibly, as a stack jumping back.
    pub fn click(&mut self, inventory: &Inventory, button: Button, quick: bool) -> Option<Intent> {
        let cursor = self.cursor?;

        // Before everything: the way out. A tap anywhere off the panel is
        // what a thumb can hit, and it is the gesture this kind of screen
        // has on every phone, so it is the one a player tries first. See
        // `Intent::Close` for why it is the only one on the glass.
        if !panel_rect().contains(cursor.0, cursor.1) {
            // **A stack in hand is put back first, and the screen stays
            // open.** Off-panel already meant "cancel", and a player who
            // has picked something up and taps beside the panel is
            // almost always aiming at a slot and missing -- closing on
            // that would put the screen away *and* leave them wondering
            // where the stack went. So the first tap cancels and the
            // second one closes.
            if self.held().is_some() {
                self.release();
                return None;
            }
            return Some(Intent::Close);
        }

        // **The tabs, before anything on a page.** They are drawn over
        // the top of the trays, so a click that reached the pages first
        // would find the tray's own "outside the grid, cancel" arm and
        // the strip would never answer.
        if let Some(index) = tab_at(cursor) {
            let wanted = ALL_TABS[index];
            // A rucksack page with no rucksack is refused rather than
            // shown empty: the tab is drawn greyed out (see
            // `tab_strip`), and a disabled control that still works is
            // worse than one that is not there.
            if wanted == Tab::Backpack && !inventory.backpack_open() {
                return None;
            }
            self.set_tab(wanted);
            return None;
        }

        if sort_button_rect().contains(cursor.0, cursor.1) {
            self.release();
            return Some(Intent::Sort);
        }

        // **Nothing below this point is on the health page.** It is a
        // page of readings with no slot, no recipe and no drop target,
        // so a click anywhere on it that is not a tab is a click on
        // nothing -- and letting it fall through would hit-test the
        // squares of the page underneath, which are not drawn.
        if self.tab == Tab::Health {
            self.release();
            return None;
        }

        // Recipes next. They live outside the slot grid, so testing
        // slots first sends every click on the crafting column down the
        // "outside the grid, cancel" path and the menu never responds.
        if let Some(recipe) = recipe_at(cursor, self.recipe_scroll, inventory) {
            self.release();
            return Some(Intent::Craft {
                index: recipe,
                times: match button {
                    Button::Left => 1,
                    Button::Right => CRAFT_MANY,
                },
            });
        }

        // **The figure, before the squares beside it.** A dressing put
        // down on a part asks the server to dress that part. A garment put
        // down on the figure goes on, as it would on its square -- a player
        // who drops a tunic on the body meant the body. Anything else put
        // down here is put back: a stone dropped on an arm is a stack that
        // missed its slot, and sending it would ask the server a question
        // with no answer. Empty hands on the figure are the tooltip's
        // business, not a gesture.
        if let Some(part) = body_part_at(cursor) {
            let from = self.held;
            self.release();
            let block = from.and_then(|slot| inventory.block_in(slot));
            return match (from, block) {
                (Some(slot), Some(b)) if Treatment::of(b).is_some() => {
                    Some(Intent::Treat { slot, part })
                }
                (Some(slot), Some(b)) if primitive_shared::equipment::is_wearable(b) => {
                    Some(Intent::Equip(slot))
                }
                _ => None,
            };
        }

        // The body, before the pack. Two gestures land here and they
        // are the mirror of each other: dropping a held garment on a
        // square puts it on, and clicking a filled square with empty
        // hands takes it off.
        if let Some(part) = equipment_at(cursor) {
            return match self.held.take() {
                Some(from) => {
                    self.release();
                    // Whether it *fits* is the server's to decide -- see
                    // `equip_from_slot`, which refuses anything that is
                    // not a garment for that part. The client sends the
                    // request rather than pre-judging it, on the same
                    // rule every other gesture on this screen follows.
                    Some(Intent::Equip(from))
                }
                None => Some(Intent::Unequip(part)),
            };
        }

        let Some(slot) = self.slot_at(cursor) else {
            // Outside everything: cancel the move. Nothing was taken out
            // of anywhere, so there is nothing to put back.
            self.release();
            return None;
        };

        match (button, self.held) {
            // Half of what is held, and keep holding the rest: dealing a
            // stack out across several slots is one gesture repeated,
            // not a pick-up per slot.
            (Button::Right, Some(from)) => {
                if from == slot {
                    None
                } else {
                    Some(Intent::Split { from, to: slot })
                }
            }
            // A right click with empty hands used not to be a gesture at
            // all -- it could have been made to pick up half, but
            // nothing is held in limbo here and there is nowhere for
            // that half to be. Tipping a jug out is what finally moved
            // into it.
            //
            // **Why not the left click, which is what a jug being
            // "clicked with an empty hand" would mean.** Left click on
            // a full slot picks it up, and that is how *everything* in
            // this screen is moved. Spending it on emptying would mean
            // a jug with grain in it could never be dragged to another
            // square, put in a chest or thrown out -- the player would
            // have to tip it out, move it, and pour it back. So the
            // gesture went to the button that had nothing to do.
            (Button::Right, None) => inventory.slots()[slot]
                .as_ref()
                .and_then(primitive_shared::inventory::jug_contents)
                .map(|_| Intent::EmptyJug(slot)),
            (Button::Left, Some(from)) => {
                self.release();
                if from == slot {
                    None // put it back down
                } else if pours_into(inventory, from, slot) {
                    // **Before the move, and it takes the move's
                    // place.** Dropping a handful of grain on a jug can
                    // only sensibly mean one thing, and the swap it
                    // replaces is still available by dropping the grain
                    // on any other square first. The server asks the
                    // same question again -- see `pour_into_jug` -- so
                    // getting this wrong is a gesture that does nothing
                    // rather than one that pours meat into a jug.
                    Some(Intent::PourIntoJug { from, jug: slot })
                } else {
                    Some(Intent::Move { from, to: slot })
                }
            }
            (Button::Left, None) => {
                // Picking up nothing is not a gesture worth starting.
                if inventory.count_in(slot) == 0 {
                    return None;
                }
                if quick {
                    // **Shift-clicking a garment puts it on.** The
                    // gesture already means "send this where it
                    // obviously goes", and for a cuirass that is not the
                    // other half of the pack. Nothing else changes: a
                    // shift-click on anything that is not a garment is
                    // the move between the hotbar and the storage rows
                    // it always was.
                    if inventory
                        .block_in(slot)
                        .is_some_and(primitive_shared::equipment::is_wearable)
                    {
                        return Some(Intent::Equip(slot));
                    }
                    return Some(Intent::QuickMove(slot));
                }
                self.held = Some(slot);
                self.held_block = inventory.block_in(slot);
                None
            }
        }
    }

    /// The slot the cursor is over, for the throw-out key.
    pub fn hovered_slot(&self) -> Option<usize> {
        self.cursor.and_then(|cursor| self.slot_at(cursor))
    }

    /// Which slot of the inventory a point is over, on the page that is
    /// showing.
    ///
    /// The exact inverse of what draws them: the grid places are the
    /// same on both slot pages and [`slot_in_place`] is the one thing
    /// that differs, so it is applied here and nowhere else.
    fn slot_at(&self, cursor: (f32, f32)) -> Option<usize> {
        if self.tab == Tab::Health {
            return None;
        }
        slot_place_at(cursor).and_then(|place| slot_in_place(self.tab, place))
    }

    /// Seeds the cursor when the screen opens.
    ///
    /// Without it the first click after opening does nothing: the
    /// pointer has not *moved* yet, so no `CursorMoved` has arrived and
    /// the screen has no idea where it is. That reads as the inventory
    /// ignoring clicks.
    pub fn open_at(&mut self, cursor: Option<(f32, f32)>) {
        self.open = true;
        self.release();
        self.cursor = cursor;
    }

    /// Puts the screen back on the pack page when the page it is on has
    /// stopped existing.
    ///
    /// The only page that can stop existing is the rucksack's, and it
    /// stops the moment the rucksack comes off -- which can happen while
    /// the screen is open, because taking it off is a gesture on this
    /// very screen. Left alone, the player would be looking at ten
    /// squares that are not there.
    ///
    /// Called from `sync`, beside the other thing a fresh snapshot can
    /// invalidate.
    fn follow_the_rucksack(&mut self, inventory: &Inventory) {
        if self.tab == Tab::Backpack && !inventory.backpack_open() {
            self.tab = Tab::Pack;
            self.release();
        }
    }

    /// A fingerprint of what `build` would draw, cheap enough to take
    /// every frame -- see the UI block in `main`, which only rebuilds
    /// the interface when a key like this one changes.
    ///
    /// The raw cursor position is in it because the screen genuinely
    /// draws at the cursor: the tooltip and the stack in hand both ride
    /// the pointer, so a moved mouse *is* a changed screen. The
    /// inventory itself is not -- the caller already fingerprints the
    /// inventory, and hashing it twice would only hide that.
    pub fn ui_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.open.hash(&mut h);
        self.held.hash(&mut h);
        self.tab.hash(&mut h);
        self.recipe_scroll.hash(&mut h);
        self.cursor
            .map(|(x, y)| (x.to_bits(), y.to_bits()))
            .hash(&mut h);
        h.finish()
    }

    /// Builds the screen. Returns nothing when closed.
    ///
    /// The `Vec`-returning form, kept for the tests: they assert on one
    /// widget's output in isolation, which is exactly what appending
    /// into a shared list is designed not to produce.
    #[cfg(test)]
    pub fn build(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        inventory: &Inventory,
        stamina_fraction: f32,
        language: Language,
    ) -> Vec<HotbarVertex> {
        self.build_wounded(font, layers, inventory, &Injuries::default(), language, stamina_fraction)
    }

    /// ...with a body that has something wrong with it, for the tests and
    /// the snapshots that are about the figure.
    #[cfg(test)]
    pub fn build_wounded(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        inventory: &Inventory,
        injuries: &Injuries,
        language: Language,
        stamina_fraction: f32,
    ) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        self.build_into(
            font,
            layers,
            inventory,
            &primitive_shared::inventory::Equipment::new(),
            injuries,
            &Vitals { stamina: stamina_fraction, ..Vitals::default() },
            language,
            &mut out,
        );
        out
    }

    /// The same screen, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    #[allow(clippy::too_many_arguments)] // a screen has a lot on it
    pub fn build_into(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        inventory: &Inventory,
        equipment: &primitive_shared::inventory::Equipment,
        injuries: &Injuries,
        vitals: &Vitals,
        language: Language,
        out: &mut Vec<HotbarVertex>,
    ) {
        if !self.open {
            return;
        }
        let mut p = Painter::onto(font, std::mem::take(out));
        p.scrim(widgets::SCRIM);

        let panel = panel_rect();
        p.deep_panel(panel);
        // Not indented: nothing stands in front of the title in the band
        // any more (see `Intent::Close`), so it starts where the body
        // column under it does.
        p.panel_header(
            panel,
            language.text(Msg::Inventory),
            header_height() - 0.012,
            0.0,
        );
        tab_strip(&mut p, self.tab, self.cursor, inventory.backpack_open(), language);

        // ---- the health page, which shares nothing below this ----
        //
        // An early return rather than a branch round each of the dozen
        // things below it. The two slot pages are the same screen with
        // one row swapped (`slot_in_place`); the health page is a
        // different screen that happens to be behind the same key, and
        // pretending otherwise would mean a condition on every line of
        // the drawing code.
        if self.tab == Tab::Health {
            health_page(&mut p, vitals, injuries, language);
            *out = p.into_vertices();
            return;
        }

        // The trays, before anything that stands in them. See the note
        // over `pack_tray` for what this screen looked like without them
        // and why the answer could not be "more space between the
        // columns".
        for tray in [pack_tray(), recipe_tray()] {
            p.well(tray, widgets::TRAY);
        }

        crafting_panel(
            &mut p,
            layers,
            inventory,
            self.cursor.and_then(|c| recipe_at(c, self.recipe_scroll, inventory)),
            self.recipe_scroll,
            language,
            self.heat,
        );
        sort_button(&mut p, self.cursor, language);

        // A strip behind the hotbar row, so the ten slots that are on
        // screen during play are visibly the same ten.
        //
        // **The strip is the whole label now.** There used to be three
        // words on this screen -- the title said PACK, a second PACK sat
        // over the first row of the pile, and BELT sat under the strip
        // -- and the first two were the same word twice. The third was
        // labelling something the player looks at during every minute of
        // play: the bar at the bottom of the screen, drawn here in the
        // same place, at the same size, with a box round it. A caption
        // under it says nothing the box has not already said.
        p.slab(belt_strip(), widgets::BUTTON);
        // **After the strip, not before it.** The gap this line is drawn
        // in is exactly the margin the belt strip keeps on its left, so a
        // line drawn first is a line the strip paints over for the
        // bottom quarter of its length -- and a rule that stops three
        // quarters of the way down reads as a mistake rather than as a
        // boundary. Over the strip, the belt visibly begins at the line,
        // which is the truth: everything right of it is the pack.
        p.quad(body_divider(), widgets::PANEL_DARK);

        // ---- what the player has on ----
        //
        // Drawn before the pack, so a garment being dragged out of a
        // square passes over the pack rather than under it.
        let over_body = self.cursor.and_then(equipment_at);
        // A heading, level with the one over the recipes and for the
        // same reason: three columns with a word over two of them reads
        // as a mistake, and the fourth square is not obviously a body
        // part without one.
        {
            let top = equipment_rect(0);
            let text = language.text(Msg::Worn);
            // Room to run on over the pack's first two columns, which carry
            // no caption of their own: the squares under it are one slot
            // wide, and no language names them that briefly.
            let room = slot_rect(HOTBAR_SLOTS + 1).x1 - top.x0;
            let scale = widgets::fitted_scale(text, widgets::CAPTION_SCALE, room, 0.7);
            p.text(text, top.x0, widgets::caption_top_over(top.y1, scale), scale, widgets::INK_DIM);
        }
        for part in 0..primitive_shared::equipment::SLOTS {
            let cell = equipment_rect(part);
            let worn = equipment.slots().get(part).copied().flatten();
            draw_slot_worn(
                &mut p,
                cell,
                layers,
                worn.map(|stack| stack.block),
                // Never a count: a garment is a thing you have on, and
                // "1" printed in the corner of every square would be
                // four ones saying nothing.
                0,
                if over_body == Some(part) {
                    SlotEdge::Hovered
                } else {
                    SlotEdge::Plain
                },
                worn.map(|stack| stack.condition()),
            );
            // An empty square says which part it is, so the four are not
            // four identical holes.
            //
            // **A ghost of the garment, not a letter.** They were
            // labelled `H C L F` -- one character each, translated, and
            // still a riddle: `C` is a chest piece or a cape or a cap
            // depending on who is reading, and the screen that could
            // have settled it is the one asking the question. A pointer
            // could have been rested on the square for a note; a phone
            // has no pointer to rest, and a phone is where this screen
            // is hardest to read.
            //
            // So the empty square holds the picture of the thing that
            // goes in it, drawn faint -- the shape of a cap over the
            // head square and a pair of boots over the feet one. It is
            // the same trick `icon_beside` is for: a picture works in
            // four languages at once and costs no layer of the texture
            // array, because these four images are already in it (see
            // `types::garment_tint`, which is why twelve garments share
            // them).
            if worn.is_none() {
                if let Some(ghost) = ghost_garment(part) {
                    icon_tinted(&mut p, cell, icon_layer(layers, ghost), GHOST);
                }
            }
        }

        // ---- the figure ----
        //
        // Beside the squares and in the same tray, because they are the
        // same body: the squares say what it is wearing and the figure
        // says what is wrong with it. Its heading sits level with the ones
        // over the squares and the recipes, for the reason those two are
        // level with each other.
        {
            let figure = mannequin_rect();
            let text = language.text(Msg::Wounds);
            // No further than the squares' own caption beside it.
            let room = equipment_rect(0).x0 - figure.x0 - 0.012;
            let scale = widgets::fitted_scale(text, widgets::CAPTION_SCALE, room, 0.7);
            p.text(text, figure.x0, widgets::caption_top_over(figure.y1, scale), scale, widgets::INK_DIM);
            crate::ui::mannequin::draw(
                &mut p,
                figure,
                injuries,
                self.cursor.and_then(body_part_at),
                self.held.and_then(|slot| inventory.block_in(slot)),
            );
        }

        let hovered = self.cursor.and_then(|cursor| self.slot_at(cursor));
        // Pointing at a recipe lights up the slots it would spend. The
        // two columns are otherwise unconnected: the row says "4
        // cobblestone" and the player still has to find the cobblestone.
        let wanted = self
            .cursor
            .and_then(|c| recipe_at(c, self.recipe_scroll, inventory))
            .and_then(primitive_shared::crafting::recipe);

        // **Over grid places, not over slot numbers.** Which slot stands
        // in a place is the page's business (`slot_in_place`), and the
        // belt is the same ten squares on every page -- so this loop
        // draws the same grid either way and the rucksack page differs
        // by one row of contents.
        for place in 0..SLOTS {
            let Some(slot) = slot_in_place(self.tab, place) else {
                continue;
            };
            let block = inventory.block_in(slot);
            let ingredient_here = matches!((wanted, block), (Some(r), Some(b))
                if r.inputs.iter().any(|&(input, _)| input == b));
            let edge = if self.held == Some(slot) {
                SlotEdge::Source
            } else if hovered == Some(slot) {
                SlotEdge::Hovered
            } else if ingredient_here {
                SlotEdge::Ingredient
            } else {
                SlotEdge::Plain
            };
            draw_slot_stack(
                &mut p,
                slot_rect(place),
                layers,
                inventory.slots().get(slot).copied().flatten(),
                edge,
            );
        }

        // A word over the storage row on the rucksack page, and none on
        // the pack page.
        //
        // **Only where it changes something.** The pack page names
        // nothing above the belt, on the argument the belt strip makes a
        // dozen lines up: a caption over squares that are obviously the
        // pack says nothing the screen has not said. The rucksack page
        // is the one case where the same row of squares is a different
        // place, and there the word is the only thing saying so.
        if self.tab == Tab::Backpack {
            let row = slot_rect(HOTBAR_SLOTS);
            let text = language.text(Msg::TabBackpack);
            // Over the row itself, and clear of `Msg::Worn`: the pack
            // hangs from the floor of the band now (see `slot_rect`), so
            // its first row is well below the caption line the worn
            // squares use and the two cannot collide.
            let room = slot_rect(SLOTS - 1).x1 - row.x0;
            let scale = widgets::fitted_scale(text, widgets::CAPTION_SCALE, room, 0.7);
            p.text(text, row.x0, widgets::caption_top_over(row.y1, scale), scale, widgets::ACCENT);
        }

        // **One line, and only what the pack itself decides.**
        //
        // There were four numbers on two lines: weight, load, speed and
        // stamina. Stamina is already a bar on the HUD, drawn a moment
        // ago and still on screen behind this panel, and load is weight
        // said again as a percentage. What is left is the pair that
        // cannot be read anywhere else -- what the pack weighs, and what
        // carrying it is costing you.
        let weight = inventory.total_weight();
        let load = primitive_shared::load::load_fraction(weight);
        let summary = format!(
            "{weight:.0} {}   {} {:.0}%",
            language.text(Msg::KgCarried),
            language.text(Msg::Speed),
            primitive_shared::load::speed_scale(weight) * 100.0,
        );
        let colour = if load >= 1.0 {
            widgets::TEXT_BAD
        } else if load > 0.6 {
            widgets::ACCENT
        } else {
            widgets::INK_DIM
        };
        // Under the belt, in the space the grid used to be centred in.
        // It is a readout *about the pack*, so it belongs in the pack's
        // column rather than in a footer under both of them.
        //
        // No rule above it any more: a line drawn the whole width of the
        // panel to separate one row of text from the grid over it was
        // furniture around furniture.
        // **Measured from the panel's own floor, not from the belt.**
        // Placed by eye under the belt it was a fixed offset that had to
        // be right for a footer whose height had since changed twice --
        // and the tails of "y" and "%" were being cut off by the bottom
        // bevel. A line's own height plus a margin above the floor
        // cannot be cut by it.
        let baseline = panel.y0 + widgets::cell_height(0.9) + FOOTER_MARGIN;
        p.text(&summary, grid_left(), baseline, 0.9, colour);

        // Last, so they sit over everything they describe. The stack in
        // hand rides the cursor: the gesture is click-then-click rather
        // than a drag, and without something following the pointer there
        // is nothing on screen that says a stack is in hand at all --
        // only a coloured border on a slot the player has since looked
        // away from.
        if let Some(cursor) = self.cursor {
            match self.held {
                Some(slot) => held_stack(&mut p, layers, inventory, slot, cursor),
                // Not while carrying something: the two would sit on top
                // of each other, and the player already knows what they
                // picked up.
                None => tooltip(
                    &mut p,
                    self.tab,
                    inventory,
                    injuries,
                    cursor,
                    self.recipe_scroll,
                    language,
                    self.heat,
                ),
            }
        }

        *out = p.into_vertices();
    }
}

/// Every reading the server sends about the body, in one argument.
///
/// **One struct rather than six more parameters on `build_into`**, which
/// already takes eight. They all arrive in two messages
/// (`ServerMessage::Body` and `Health`/`Nourishment`) and they are all
/// drawn on one page, so they travel together -- and the seventh
/// argument is the one somebody passes in the wrong order.
///
/// `stamina` is the client's own prediction rather than a number off the
/// wire, which is deliberate and is the same value the HUD bar draws:
/// two readings of one thing that disagreed would be worse than not
/// showing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vitals {
    /// Health left, 0..1.
    pub health: f32,
    /// How full, 0..1.
    pub nourishment: f32,
    /// Breath left, 0..1, as the client predicts it.
    pub stamina: f32,
    /// Everything `ServerMessage::Body` carries.
    pub body: crate::ui::hud::BodyGauges,
}

impl Default for Vitals {
    fn default() -> Self {
        Self {
            health: 1.0,
            nourishment: 1.0,
            stamina: 1.0,
            body: crate::ui::hud::BodyGauges::default(),
        }
    }
}

/// How tall one line of the health page is *at most*.
///
/// A ceiling rather than the step itself: ten rows have to end above the
/// panel's floor, and the row that fell through it was the tenth --
/// found in a picture, which is what the dump test is for. The step
/// actually used is whichever is smaller, this or what fits.
const VITAL_ROW: f32 = 0.060;
/// How wide the little bar beside a reading is, and how tall.
const VITAL_BAR: f32 = 0.26;
const VITAL_BAR_HEIGHT: f32 = 0.020;
/// How far the bars sit from the labels: the longest label in the
/// longest language plus air. Measured against Polish
/// ("roznorodnosc jedzenia") rather than English, which is the shortest
/// of the four and would have put the bars through the words.
const VITAL_LABEL_WIDTH: f32 = 0.50;

/// Draws the health page: every vital down the left, every wound down
/// the right.
///
/// ## Why a bar *and* a number on every row
///
/// The bar is what a glance reads and the number is what a decision
/// needs. The HUD has bars and no numbers, which is right for a thing
/// that is on screen while you are being chased; this page is opened on
/// purpose, and "is my water low" has already been answered by the HUD
/// by the time a player comes here. What they came for is how low.
///
/// ## Comfort
///
/// **It is not on this page and it is not going to be.** Comfort is a
/// hidden score the server adds up out of a dozen things -- warmth, wet,
/// filth, smoke, a fire nearby, a roof, a bed -- and printing it would
/// turn a set of habits into a number to optimise, which is exactly the
/// failure the design rule in `CLAUDE.md` names. What is printed instead
/// is what comfort *does*: the multiplier on how fast breath comes back
/// (`comfort::recovery`, sent as `ServerMessage::Body::recovery`). A
/// player who sees `x0.70` and reads the three rows above it -- soaked,
/// filthy, freezing -- has been told everything the hidden number knows,
/// in terms of things they can do something about.
///
/// Warmth is a temperature and a colour rather than a band name, for a
/// smaller version of the same reason: the five bands have no translated
/// names anywhere in the game, and five new strings to say what a
/// coloured number already says is furniture.
fn health_page(p: &mut Painter, vitals: &Vitals, injuries: &Injuries, language: Language) {
    let panel = panel_rect();
    let left = grid_left() - body_column_width();
    let top = content_top() - GRID_LABEL;

    // Falling is *good* for four of these and bad for three, so each row
    // says which way is which rather than sharing one ramp -- a green
    // "wetness 90%" is a page that has stopped meaning anything.
    let good_high = |fraction: f32| {
        if fraction < 0.25 {
            widgets::TEXT_BAD
        } else if fraction < 0.5 {
            widgets::ACCENT
        } else {
            widgets::TEXT_GOOD
        }
    };
    let good_low = |fraction: f32| good_high(1.0 - fraction);

    let body = &vitals.body;
    let warmth_colour = match body.comfort {
        primitive_shared::body::Comfort::Freezing
        | primitive_shared::body::Comfort::Scorching => widgets::TEXT_BAD,
        primitive_shared::body::Comfort::Cold | primitive_shared::body::Comfort::Warm => {
            widgets::ACCENT
        }
        primitive_shared::body::Comfort::Comfortable => widgets::TEXT_GOOD,
    };
    let percent = |fraction: f32| format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0);
    let groups = f32::from(body.diet_groups.min(4)) / 4.0;

    // (label, what it reads, its colour, how full its bar is)
    //
    // A table rather than ten copies of the same three draw calls, on
    // the argument `player_model::PARTS` makes at length: a row that is
    // data can be reordered or retuned by anybody, and a row that is
    // code is arithmetic nobody can edit.
    let rows: [(Msg, String, [f32; 4], Option<f32>); 10] = [
        (Msg::VitalHealth, percent(vitals.health), good_high(vitals.health), Some(vitals.health)),
        (Msg::VitalHunger, percent(vitals.nourishment), good_high(vitals.nourishment), Some(vitals.nourishment)),
        (Msg::VitalThirst, percent(body.hydration), good_high(body.hydration), Some(body.hydration)),
        (Msg::VitalStamina, percent(vitals.stamina), good_high(vitals.stamina), Some(vitals.stamina)),
        (Msg::VitalTiredness, percent(body.fatigue), good_low(body.fatigue), Some(body.fatigue)),
        (Msg::VitalWarmth, format!("{:.0}C", body.temperature_c), warmth_colour, None),
        (Msg::VitalWetness, percent(body.wetness), good_low(body.wetness), Some(body.wetness)),
        (Msg::VitalDirt, percent(body.grime), good_low(body.grime), Some(body.grime)),
        // Out of four, which is how many groups there are -- the rule
        // that reads this (`food::diet_regen_factor`) reads the count
        // and nothing else. A bare "3" would be a number with no scale.
        (Msg::VitalDiet, format!("{}/4", body.diet_groups.min(4)), good_high(groups), Some(groups)),
        (
            Msg::VitalRecovery,
            format!("x{:.2}", body.recovery),
            if body.recovery < 0.85 {
                widgets::TEXT_BAD
            } else if body.recovery < 1.0 {
                widgets::ACCENT
            } else {
                widgets::TEXT_GOOD
            },
            // No bar: it is a multiplier around one rather than a
            // fraction of anything, and a bar under it would have to
            // invent a maximum to be a fraction of.
            None,
        ),
    ];

    let scale = 0.82;
    let line = widgets::cell_height(scale);
    // **The step is measured, not written down.** Ten rows of a fixed
    // 0.072 put the last one through the bottom of the panel, and the
    // only thing that would have caught that is somebody looking at the
    // picture. The rows share whatever room there is between the first
    // baseline and the panel's floor, and `VITAL_ROW` is a ceiling so
    // they do not sprawl when there is room to spare.
    let floor = panel.y0 + FOOTER_MARGIN + line;
    let step = ((top - floor) / (rows.len() - 1) as f32).min(VITAL_ROW);
    for (index, (label, reading, colour, fraction)) in rows.iter().enumerate() {
        let baseline = top - index as f32 * step;
        p.text(language.text(*label), left, baseline, scale, widgets::INK_DIM);
        let bar_left = left + VITAL_LABEL_WIDTH;
        if let Some(fraction) = *fraction {
            let floor = baseline - line + (line - VITAL_BAR_HEIGHT) / 2.0;
            let track = Rect::new(bar_left, floor, bar_left + VITAL_BAR, floor + VITAL_BAR_HEIGHT);
            p.well(track, widgets::PANEL_DARK);
            let filled = fraction.clamp(0.0, 1.0);
            if filled > 0.0 {
                p.quad(
                    Rect::new(track.x0, track.y0, track.x0 + track.width() * filled, track.y1),
                    *colour,
                );
            }
        }
        p.text(reading, bar_left + VITAL_BAR + 0.020, baseline, scale, *colour);
    }

    // ---- what is wrong, in the column the recipes use ----
    //
    // Beside the readings rather than under them, because the readings
    // are a fixed ten rows and the wounds are anything from none to a
    // dozen: stacked, a bad day would push the page off the bottom of
    // the panel, and there is a whole column standing empty on this
    // page.
    // **Where the readings end, not where the recipe column starts.**
    // It was `recipe_left()`, which is right for the two slot pages and
    // wrong here: this page draws no recipes, so a wound line hung off
    // that edge had a quarter of the panel to live in and ran off the
    // right-hand side of the screen -- text lying on the world, which is
    // the exact failure `nothing_is_drawn_outside_the_panel` exists for.
    // Here the whole right half is empty and these are the longest
    // sentences on the screen, so they get all of it.
    // The gap is a reading's width plus air: the numbers are
    // right-aligned by nothing, so "100%" is the widest of them and
    // the wounds have to start clear of it.
    let wounds_left = left + VITAL_LABEL_WIDTH + VITAL_BAR + 0.22;
    let room = (panel.x1 - PANEL_PAD - wounds_left).max(0.0);
    p.text(
        language.text(Msg::VitalInjuries),
        wounds_left,
        top,
        widgets::CAPTION_SCALE,
        widgets::INK_DIM,
    );
    let note = 0.74;
    let mut y = top - 0.056;
    let mut said_anything = false;
    for part in primitive_shared::injury::Part::ALL {
        if injuries.part(part).is_whole() {
            continue;
        }
        for (text, colour) in crate::ui::mannequin::lines(part, injuries, language) {
            // Stops at the panel's floor rather than running through it.
            // A body can carry more wounds than there is room to list,
            // and what is listed first is the worst -- `mannequin::lines`
            // sorts by danger, so the ones that fall off the end are the
            // ones that matter least.
            if y - widgets::cell_height(note) < panel.y0 + FOOTER_HEIGHT {
                break;
            }
            // Fitted to the room, on `widgets::button`'s reasoning: a
            // wound is described in a whole sentence, and the sentence
            // is half again longer in Polish than in English.
            let fitted = widgets::fitted_scale(&text, note, room, 0.45);
            p.text(&text, wounds_left, y, fitted, colour);
            y -= widgets::cell_height(note) + 0.010;
            said_anything = true;
        }
        y -= 0.008;
    }
    if !said_anything {
        p.text(language.text(Msg::NoWounds), wounds_left, y, note, widgets::TEXT_GOOD);
        y -= widgets::cell_height(note) + 0.010;
    }

    // ---- the place, under the wounds ----
    //
    // Under them and not above, because a wound is the more urgent line
    // and the list sorts by danger; and fitted to the same room, stopping
    // at the same floor, for the same reasons.
    y -= 0.024;
    for (text, colour) in shelter_lines(body, language) {
        if y - widgets::cell_height(note) < panel.y0 + FOOTER_HEIGHT {
            break;
        }
        let fitted = widgets::fitted_scale(&text, note, room, 0.45);
        p.text(&text, wounds_left, y, fitted, colour);
        y -= widgets::cell_height(note) + 0.010;
    }
}

/// What the health page says about the place the player is in: the air
/// their skin is drifting towards, and each thing about the room that is
/// costing or earning them warmth.
///
/// **The causes, in words, and never a score.** The comfort note above
/// says why a hidden number stays hidden; the same goes for a room. What a
/// player can act on is "the wind comes in at the door" and "the smoke
/// hole lets heat out" -- each names the thing to change -- and the air's
/// temperature is there so the change can be seen to work. The walls are
/// named only at the ends -- warm or thin -- because a middling wall is
/// not a decision anybody has to make again.
pub(crate) fn shelter_lines(body: &crate::ui::hud::BodyGauges, language: Language) -> Vec<(String, [f32; 4])> {
    let shelter = &body.shelter;
    let mut lines = Vec::new();
    if shelter.air_c.is_finite() {
        let colour = match primitive_shared::body::Comfort::of(shelter.air_c) {
            primitive_shared::body::Comfort::Comfortable => widgets::TEXT_GOOD,
            primitive_shared::body::Comfort::Freezing | primitive_shared::body::Comfort::Scorching => widgets::TEXT_BAD,
            _ => widgets::ACCENT,
        };
        lines.push((format!("{} {:.0}C", language.text(Msg::ShelterAir), shelter.air_c), colour));
    }
    // A fifth: the fog is already grey at it, and the breath goes at half
    // (`wildfire::SMOKE_CHOKES`), which is too late to be the first thing
    // the page says.
    if body.smoke >= 0.2 {
        let colour = if body.smoke >= primitive_shared::wildfire::SMOKE_CHOKES {
            widgets::TEXT_BAD
        } else {
            widgets::ACCENT
        };
        lines.push((language.text(Msg::ShelterSmoky).to_string(), colour));
    }
    if shelter.indoors && shelter.draught >= 0.25 {
        lines.push((language.text(Msg::ShelterDraughty).to_string(), widgets::ACCENT));
    }
    if shelter.roof_open {
        lines.push((language.text(Msg::ShelterHoleTakesHeat).to_string(), widgets::INK_DIM));
    }
    if shelter.indoors {
        if shelter.keeps_out >= 0.85 {
            lines.push((language.text(Msg::ShelterWallsWarm).to_string(), widgets::TEXT_GOOD));
        } else if shelter.keeps_out <= 0.6 {
            lines.push((language.text(Msg::ShelterWallsThin).to_string(), widgets::ACCENT));
        }
    }
    lines
}

/// Draws the three tabs.
///
/// The one showing is a *well* and the other two are buttons: the page
/// you are on is the hole the panel's content comes out of, and the two
/// you are not on are things to press. That is one shape carrying the
/// state rather than a colour, which survives being looked at on a phone
/// in daylight.
fn tab_strip(
    p: &mut Painter,
    showing: Tab,
    cursor: Option<(f32, f32)>,
    backpack_on: bool,
    language: Language,
) {
    for (index, tab) in ALL_TABS.iter().copied().enumerate() {
        let rect = tab_rect(index);
        let text = language.text(tab.label());
        // The rucksack page exists only while a rucksack is worn. Drawn
        // greyed rather than hidden, because a strip that changes from
        // three tabs to two moves the other two -- and a control that
        // moves when the player takes their pack off is a control they
        // have to find again.
        let enabled = tab != Tab::Backpack || backpack_on;
        if tab == showing {
            p.well(rect, widgets::TRAY);
            let usable = (rect.width() - 0.03).max(0.0);
            let scale = widgets::fitted_scale(text, 0.80, usable, 0.55);
            p.text_centred(
                text,
                rect.centre_x(),
                rect.centre_y() + widgets::cell_height(scale) / 2.0,
                scale,
                widgets::ACCENT,
            );
        } else {
            let hovered = enabled && cursor.is_some_and(|(x, y)| rect.contains(x, y));
            p.button(rect, text, hovered, enabled);
        }
    }
}

/// Why a slot's border is lit, if it is.
///
/// A slot can only be one of these at a time, which is the point of the
/// enum: two reasons at once would need an order of precedence, and the
/// player would have to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotEdge {
    Plain,
    /// Under the pointer.
    Hovered,
    /// Where the stack in hand came from.
    Source,
    /// Holds something the hovered recipe would spend.
    Ingredient,
}

/// Draws one slot: the recess, its border, its icon and its count.
///
/// Shared with the chest screen, which is a grid of exactly these -- and
/// sharing the *drawing* rather than the numbers is what keeps the two
/// looking like one game after either of them is next tweaked.
/// The wash that marks a slot the pointer is over, the one a stack was
/// picked up from, and one holding an ingredient of the recipe under the
/// cursor. Drawn inside the well, over the stone but under the icon.
const HIGHLIGHT_HOVER: [f32; 4] = [1.0, 1.0, 1.0, 0.35];
const HIGHLIGHT_SOURCE: [f32; 4] = [1.0, 0.85, 0.35, 0.45];
const HIGHLIGHT_INGREDIENT: [f32; 4] = [0.55, 0.85, 1.0, 0.30];

/// One bevel in from a cell: where a wash goes, so it does not paint
/// over the well's own edge.
fn inset(cell: Rect) -> Rect {
    Rect::new(
        cell.x0 + widgets::BEVEL,
        cell.y0 + widgets::BEVEL,
        cell.x1 - widgets::BEVEL,
        cell.y1 - widgets::BEVEL,
    )
}

/// The wear bar's empty half, and the three states of its full half.
const WEAR_BACK: [f32; 4] = [0.06, 0.06, 0.08, 0.9];
const WEAR_GOOD: [f32; 4] = [0.42, 0.78, 0.38, 1.0];
const WEAR_HALF: [f32; 4] = [0.86, 0.72, 0.24, 1.0];
const WEAR_LOW: [f32; 4] = [0.86, 0.32, 0.24, 1.0];

/// The corner a judged piece carries in the grid: rust for *poor*, gold
/// for *fine*, nothing for the two bands between.
///
/// **Only the ends are marked.** The words are four (`quality::Band`) and
/// the tooltip has all of them, but a mark on every made thing is the
/// wear bar under every fresh axe again -- a pack full of corners saying
/// "plain". What a player scanning a chest wants is the one to eat first
/// and the one to keep, and those are the two ends. Rejected: a tint
/// over the whole icon (it fights `icon_tint`, which already colours
/// garments and wood kinds) and a letter (it needs the font at a size
/// the count plate already owns, and would be four languages of it).
///
/// Drawn in the top-right corner, which is the one no other mark uses:
/// the jug's contents are top-left, the wear bar along the bottom, the
/// count bottom-right. **Drawing only**: the cell is the same rectangle
/// whatever is in it, so hit-testing cannot move.
const QUALITY_POOR: [f32; 4] = [0.72, 0.30, 0.18, 1.0];
const QUALITY_FINE: [f32; 4] = [0.98, 0.80, 0.26, 1.0];

/// The corner's colour for this band, or `None` for no corner.
pub(crate) fn quality_corner(band: Option<primitive_shared::quality::Band>) -> Option<[f32; 4]> {
    use primitive_shared::quality::Band;
    match band? {
        Band::Poor => Some(QUALITY_POOR),
        Band::Fine => Some(QUALITY_FINE),
        Band::Plain | Band::Good => None,
    }
}

/// The corner a dulled tool carries in the grid: slate for *dull*, the
/// wear bar's red for *blunt*, nothing for sharp or merely dulled.
///
/// **The last two steps of four, and not the first.** A tool is *dulled* a
/// few dozen swings after every hone (`tools::edge_swings`), so a mark at
/// the first step would sit on nearly every tool in the pack and say
/// nothing -- the wear bar's lesson again (see `QUALITY_FINE`). What a player
/// scanning the hotbar wants is "this one needs the stone": a pick at two
/// thirds of its speed, and a knife that is tearing hides.
///
/// **Top-left, the jug's corner**, and they never meet: a jug takes no edge
/// and a tool holds no contents. Read off the id alone, because that is where
/// the edge is kept (`tools`, "why the edge is in the id"), so every caller
/// that draws a slot draws it without a new argument.
const EDGE_DULL: [f32; 4] = [0.55, 0.62, 0.70, 1.0];

pub(crate) fn edge_corner(block: primitive_shared::types::BlockId) -> Option<[f32; 4]> {
    match primitive_shared::tools::blunt_step(block) {
        2 => Some(EDGE_DULL),
        3 => Some(WEAR_LOW),
        _ => None,
    }
}

pub(crate) fn draw_slot(
    p: &mut Painter,
    cell: Rect,
    layers: &FaceLayers,
    block: Option<primitive_shared::types::BlockId>,
    count: u32,
    edge: SlotEdge,
) {
    draw_slot_full(p, cell, layers, block, count, edge, None, None, None)
}

/// The same, for a slot that might hold a worn tool.
///
/// `wear` is how much of it is left, 0..1 -- `None` for a caller that
/// has no stack to ask (a recipe row draws an ingredient, not a thing
/// anybody owns).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_slot_worn(
    p: &mut Painter,
    cell: Rect,
    layers: &FaceLayers,
    block: Option<primitive_shared::types::BlockId>,
    count: u32,
    edge: SlotEdge,
    wear: Option<f32>,
) {
    draw_slot_full(p, cell, layers, block, count, edge, wear, None, None)
}

/// The same again, from the stack itself.
///
/// **The only form that can draw what is inside a jug**, and therefore
/// the one every caller that actually has a `Stack` should use. The
/// contents live in the stack's `damage` (see
/// `inventory::jug_contents`), so a caller that has taken the block and
/// the count apart has already thrown them away -- which is exactly how
/// a jug of grain used to be drawn as an empty jug.
pub(crate) fn draw_slot_stack(
    p: &mut Painter,
    cell: Rect,
    layers: &FaceLayers,
    stack: Option<primitive_shared::inventory::Stack>,
    edge: SlotEdge,
) {
    draw_slot_full(
        p,
        cell,
        layers,
        stack.map(|s| s.block),
        stack.map_or(0, |s| s.count),
        edge,
        stack.map(|s| s.condition()),
        stack
            .as_ref()
            .and_then(primitive_shared::inventory::jug_contents),
        stack.and_then(|s| s.quality().band()),
    );
}

/// `contents` is what a jug is carrying, drawn instead of the slot's own
/// count -- see the corner mark below.
#[allow(clippy::too_many_arguments)] // a slot has a lot on it
fn draw_slot_full(
    p: &mut Painter,
    cell: Rect,
    layers: &FaceLayers,
    block: Option<primitive_shared::types::BlockId>,
    count: u32,
    edge: SlotEdge,
    wear: Option<f32>,
    contents: Option<(primitive_shared::types::BlockId, u32)>,
    quality: Option<primitive_shared::quality::Band>,
) {
    // **A slot is a well cut into the stone**, and the bevel is what
    // says so: dark along the top and left where the lip shades it,
    // light along the bottom and right. See `Painter::well`.
    p.well(cell, widgets::WELL);

    // What the pointer is over, and what has been picked up, are drawn
    // as a wash *inside* the well rather than as an outline around it: a
    // second frame round a bevelled hole reads as a third edge, and the
    // eye stops being able to tell which line is the slot.
    match edge {
        SlotEdge::Source => p.quad(inset(cell), HIGHLIGHT_SOURCE),
        SlotEdge::Hovered => p.quad(inset(cell), HIGHLIGHT_HOVER),
        SlotEdge::Ingredient => p.quad(inset(cell), HIGHLIGHT_INGREDIENT),
        SlotEdge::Plain => {}
    }

    let Some(block) = block else {
        return;
    };
    icon_tinted(
        p,
        cell,
        icon_layer(layers, block),
        crate::ui::hotbar::icon_tint(block, [1.0, 1.0, 1.0, 1.0]),
    );

    // The quality corner, over the icon so a dark picture cannot hide it,
    // on a dark backing so a gold one shows on straw. See `QUALITY_FINE`.
    if let Some(colour) = quality_corner(quality) {
        let size = CELL * 0.16;
        let (x1, y1) = (cell.x1 - widgets::BEVEL, cell.y1 - widgets::BEVEL);
        p.quad(Rect::new(x1 - size - CELL * 0.03, y1 - size - CELL * 0.03, x1, y1), WEAR_BACK);
        p.quad(Rect::new(x1 - size, y1 - size, x1, y1), colour);
    }

    // The edge mark, in the top-left corner, for a tool gone dull or blunt.
    if let Some(colour) = edge_corner(block) {
        let size = CELL * 0.16;
        let (x0, y1) = (cell.x0 + widgets::BEVEL, cell.y1 - widgets::BEVEL);
        p.quad(Rect::new(x0, y1 - size - CELL * 0.03, x0 + size + CELL * 0.03, y1), WEAR_BACK);
        p.quad(Rect::new(x0, y1 - size, x0 + size, y1), colour);
    }

    // The wear bar, under the icon of anything that wears out.
    //
    // Only for a tool that has actually been used: a bar under every
    // fresh axe is a row of full bars saying nothing, and the thing a
    // player wants to see at a glance is *which* of their tools is about
    // to go. See `inventory::Stack::condition`.
    if let Some(condition) = wear {
        if condition < 1.0 {
            let bar = Rect::new(
                cell.x0 + CELL * 0.12,
                cell.y0 + CELL * 0.10,
                cell.x1 - CELL * 0.12,
                cell.y0 + CELL * 0.16,
            );
            p.quad(bar, WEAR_BACK);
            // Green through amber to red: the colour is the warning, and
            // it has to be readable without reading the length.
            let colour = if condition > 0.5 {
                WEAR_GOOD
            } else if condition > 0.2 {
                WEAR_HALF
            } else {
                WEAR_LOW
            };
            p.quad(
                Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * condition, bar.y1),
                colour,
            );
        }
    }

    // The count sits on its own dark plate in the corner. Over a sand or
    // snow icon a plain white number is invisible, and a drop shadow
    // alone is not enough at this size.
    // **A count of one on a tool is noise.** A slot that can only ever
    // hold one thing does not need a number saying so, and thirteen of
    // them across a pack full of tools is thirteen numbers that never
    // change. See `types::stack_limit`.
    //
    // **A full jug is the one slot that has a number worth printing
    // anyway**, and it is not the slot's count -- a jug stacks to one,
    // so its own count is the "1" this rule exists to suppress. What is
    // printed is how many units are *in* it, over a thumbnail of what
    // they are, so a row of four jugs reads as sand, grain, ash, seed
    // rather than as four identical pots. The mark costs no texture
    // layer: it is the contents' own icon, already in the array
    // (`icon_layer`), drawn small.
    let (count, full) = match contents {
        Some((inside, units)) => {
            let size = CELL * 0.34;
            let pad = CELL * 0.07;
            textured(
                p,
                Rect::new(
                    cell.x0 + pad,
                    cell.y1 - pad - size,
                    cell.x0 + pad + size,
                    cell.y1 - pad,
                ),
                icon_layer(layers, inside),
                crate::ui::hotbar::icon_tint(inside, [1.0, 1.0, 1.0, 1.0]),
            );
            (units, units >= primitive_shared::inventory::JUG_UNITS)
        }
        None => {
            if primitive_shared::types::stack_limit(block) == 1 {
                return;
            }
            (count, count >= primitive_shared::types::stack_limit(block))
        }
    };
    // **The number lives inside the well.** It used to be placed from
    // the cell's own edge with a fixed margin, which is fine for one
    // digit and wrong for three: "128" started outside its slot on the
    // left and sat on the bevel at the bottom. Measured, then clamped
    // into the cell's inner area -- and a count too wide even for that
    // is drawn smaller rather than allowed out.
    let label = count.to_string();
    let inner = inset(cell);
    let mut scale = COUNT_SCALE;
    if widgets::ink_width(&label, scale) > inner.width() - 0.006 {
        scale *= 0.8;
    }
    let width = widgets::ink_width(&label, scale);
    let x0 = (inner.x1 - width - 0.007).max(inner.x0 + 0.002);
    // Lifted by the shadow's own offset as well: the shadow is drawn
    // down and to the right, so a number sitting exactly on the well's
    // floor puts its shadow *through* the bevel -- which is what the
    // clipped-looking digits along the bottom of every slot were.
    let floor = inner.y0 + 0.003 + COUNT_SHADOW;
    let plate = Rect::new(
        x0,
        floor,
        (x0 + width + 0.006).min(inner.x1),
        floor + widgets::cell_height(scale),
    );
    // **A shadow rather than a plate.** The count used to sit on a dark
    // rectangle, which is a second thing in the slot; what every
    // interface of this kind does instead is draw the number twice, once
    // dark and offset a pixel, once bright on top. It is legible over a
    // pale sand icon and a black coal one alike, and it adds nothing to
    // the slot but the number.
    let colour = if full { COUNT_FULL } else { COUNT_TEXT };
    let (x, y) = (plate.x0 + 0.003, plate.y1);
    p.text(&label, x + COUNT_SHADOW, y - COUNT_SHADOW, scale, COUNT_DARK);
    p.text(&label, x, y, scale, colour);
}

/// Draws the picked-up stack under the cursor.
pub(crate) fn held_stack(
    p: &mut Painter,
    layers: &FaceLayers,
    inventory: &Inventory,
    slot: usize,
    cursor: (f32, f32),
) {
    let Some(block) = inventory.block_in(slot) else {
        return;
    };
    let size = CELL * 0.66;
    let rect = Rect::new(
        cursor.0 - size / 2.0,
        cursor.1 - size / 2.0,
        cursor.0 + size / 2.0,
        cursor.1 + size / 2.0,
    );
    // Slightly transparent, so it reads as carried rather than as one
    // more thing lying on the screen.
    textured(
        p,
        rect,
        icon_layer(layers, block),
        crate::ui::hotbar::icon_tint(block, [1.0, 1.0, 1.0, 0.85]),
    );
    let label = inventory.count_in(slot).to_string();
    let scale = 0.66;
    p.text(
        &label,
        rect.x1 - widgets::ink_width(&label, scale),
        rect.y0 + widgets::cell_height(scale),
        scale,
        COUNT_TEXT,
    );
}

// ---- tooltip ----

/// The plate a note is written on: the deepest recess the stone has.
///
/// **It was a blue-black** -- `[0.04, 0.05, 0.07]`, with the blue-grey
/// `RULE` round it -- and it was the one cold thing on a warm screen: a
/// chest is stone and wood and amber, and the note naming what was in it
/// was a scrap of the menu pasted over the top. The same darkness, a
/// shade under in fact, so every ink measured against it still clears;
/// the stone's cast and the stone's lit edge, so it reads as part of the
/// slab it is lying on.
const TOOLTIP_BG: [f32; 4] = [0.050, 0.043, 0.036, 0.97];
const TOOLTIP_EDGE: [f32; 4] = widgets::Theme::STONE.light;
/// **A tooltip is dark, so it is written in the dark skin's ink.**
///
/// The panel under it is pale stone and everything printed on that is
/// near-black -- and a tooltip that inherited the panel's ink was black
/// text on a black box: a line that was *there*, and unreadable, which
/// is the worst of both. See `widgets::Theme`.
const TOOLTIP_INK: [f32; 4] = widgets::Theme::DARK.ink;
const TOOLTIP_DIM: [f32; 4] = widgets::Theme::DARK.ink_dim;
const TOOLTIP_GOOD: [f32; 4] = [0.52, 0.88, 0.55, 1.0];
const TOOLTIP_BAD: [f32; 4] = [1.00, 0.48, 0.42, 1.0];

const TOOLTIP_SCALE: f32 = 0.72;

/// Names what the cursor is over.
///
/// Block textures at icon size are not self-explanatory -- cobblestone
/// and stone are the same grey square to anyone who has not learned them
/// -- and the weight is the number the whole load mechanic turns on, so
/// it belongs where the player is deciding what to carry.
/// One line of text in a box, beside the pointer and inside `bounds`.
///
/// The tooltip's box, without the part that decides what to say -- so
/// the chest and the hearth can have one without a second copy of the
/// arithmetic that keeps it on screen.
pub(crate) fn hover_note(p: &mut Painter, cursor: (f32, f32), text: &str, bounds: Rect) {
    note_box(p, cursor, &[(text.to_string(), TOOLTIP_INK)], bounds);
}

/// The box itself: lines, sized, placed and drawn.
fn note_box(p: &mut Painter, cursor: (f32, f32), lines: &[(String, [f32; 4])], bounds: Rect) {
    let scale = TOOLTIP_SCALE;
    let width = lines
        .iter()
        .map(|(text, _)| widgets::ink_width(text, scale))
        .fold(0.0, f32::max)
        + 0.020;
    let line_height = widgets::cell_height(scale) + 0.006;
    // Padding at both ends, and enough of it: the last line's
    // descenders were sitting on the bottom edge of the box.
    let height = line_height * lines.len() as f32 + 0.020;
    // Up and to the right of the pointer, then pulled back inside the
    // panel: a tooltip that runs off the screen is worse than none.
    let x0 = (cursor.0 + 0.014).min(bounds.x1 - width);
    let y0 = (cursor.1 + 0.012).min(bounds.y1 - height);
    let rect = Rect::new(x0, y0, x0 + width, y0 + height);
    p.quad(rect, TOOLTIP_BG);
    p.border(rect, 0.002, TOOLTIP_EDGE);
    for (n, (text, colour)) in lines.iter().enumerate() {
        p.text(
            text,
            rect.x0 + 0.010,
            rect.y1 - 0.010 - n as f32 * line_height,
            scale,
            *colour,
        );
    }
}

#[allow(clippy::too_many_arguments)] // it is a note about everything on the screen
fn tooltip(
    p: &mut Painter,
    tab: Tab,
    inventory: &Inventory,
    injuries: &Injuries,
    cursor: (f32, f32),
    scroll: usize,
    language: Language,
    heat: primitive_shared::crafting::Heat,
) {
    // A part of the figure: its name, and every wound on it with what the
    // wound needs. On a phone this is what a tap on an arm with empty hands
    // shows, which is how a player with no pointer reads the figure.
    if let Some(part) = body_part_at(cursor) {
        note_box(p, cursor, &crate::ui::mannequin::lines(part, injuries, language), panel_rect());
        return;
    }
    // A body square, before the pack: the four are outside the grid, so
    // asking `slot_at` first would send every one of them down the
    // "nothing here" path and the note would never appear.
    //
    // **The square says what it is even when it is empty.** The ghost
    // garment in it is the answer a phone gets, and it is a picture; a
    // pointer can have the word as well, and the word is the thing
    // somebody asks for when the picture is a shape they do not yet
    // recognise.
    if let Some(part) = equipment_at(cursor) {
        let Some(name) = part_name(part, language) else {
            return;
        };
        note_box(p, cursor, &[(name.to_string(), TOOLTIP_INK)], panel_rect());
        return;
    }
    let lines = match slot_place_at(cursor).and_then(|place| slot_in_place(tab, place)) {
        Some(slot) => {
            let Some(block) = inventory.block_in(slot) else {
                return;
            };
            let count = inventory.count_in(slot);
            // Name in the player's language, then which water for a full
            // jug -- the thing that decides whether to drink it, see
            // `types::water_label` -- and for a tool whether it is steeled
            // and how blunt, which the picture cannot show: `tools::label`.
            let mut lines =
                vec![(crate::ui::names::stack_line(block, count, language), TOOLTIP_INK)];
            // ...and how well it was made, on a line of its own, for
            // anything anybody judged (`quality::Band`). **Nothing at all
            // for an unmarked piece**, which is every stone off the ground
            // and everything in a save older than the mechanic: a line
            // saying "plain work" under a lump of coal would put a
            // judgement on a thing nobody made.
            if let Some(band) = inventory
                .slots()
                .get(slot)
                .copied()
                .flatten()
                .and_then(|stack| stack.quality().band())
            {
                lines.push((quality_line(band, language), quality_ink(band)));
            }
            lines
        }
        // A recipe cell shows what it makes; the tooltip is where what
        // it *costs* lives now. The cell used to carry the whole
        // sentence -- name, count, and the ingredients in pictures --
        // and paid for it in width on every recipe at once, including
        // the seventeen nobody is pointing at.
        None => match recipe_at(cursor, scroll, inventory)
            .and_then(primitive_shared::crafting::recipe)
        {
            Some(r) => recipe_lines(inventory, r, language, heat),
            None => return,
        },
    };

    note_box(p, cursor, &lines, panel_rect());

}

/// The word under an item saying how well it was made.
fn quality_line(band: primitive_shared::quality::Band, language: Language) -> String {
    use primitive_shared::quality::Band;
    use crate::ui::lang::Msg;
    let msg = match band {
        Band::Poor => Msg::QualityPoor,
        Band::Plain => Msg::QualityPlain,
        Band::Good => Msg::QualityGood,
        Band::Fine => Msg::QualityFine,
    };
    language.text(msg).to_string()
}

/// ...and what colour it is written in.
///
/// **Only the two ends are coloured.** The middle two are the tooltip's
/// own dim ink, because a four-colour scale would make every item in the
/// pack shout something, and what the player is looking for is the one
/// piece that is unusual either way.
fn quality_ink(band: primitive_shared::quality::Band) -> [f32; 4] {
    use primitive_shared::quality::Band;
    match band {
        Band::Poor => TOOLTIP_BAD,
        Band::Fine => TOOLTIP_GOOD,
        Band::Plain | Band::Good => TOOLTIP_DIM,
    }
}

/// What a recipe says when you point at it: what it makes, what it
/// costs, and whether you can.
fn recipe_lines(
    inventory: &Inventory,
    r: &primitive_shared::crafting::Recipe,
    language: Language,
    heat: primitive_shared::crafting::Heat,
) -> Vec<(String, [f32; 4])> {
    use primitive_shared::crafting::{
        feasibility, missing_ingredient, possible_crafts, Feasibility,
    };
    use crate::ui::names;

    let mut lines = vec![(names::recipe(r.name, language).into_owned(), TOOLTIP_INK)];
    // The cost, spelled out rather than drawn: a line of text is read
    // once, and the icons it replaces were being drawn for every recipe
    // on screen whether or not anyone was looking at them.
    let cost = r
        .inputs
        .iter()
        .map(|&(block, amount)| {
            let have = inventory.count(block);
            format!("{amount}x {} ({have})", names::block(block, language))
        })
        .collect::<Vec<_>>()
        .join("   ");
    lines.push((cost, TOOLTIP_DIM));
    // The odds of the attempt eating its ingredients and making nothing
    // -- knapping, and nothing else (see `Recipe::failure`). In the
    // colour a refusal is drawn in, because that is what it is a
    // warning of, and as a bare percentage because the one word that
    // would explain it is a word in two alphabets and this tooltip is
    // read in a fraction of a second: a red "50%" beside "knife head"
    // is understood, and the manual says the rest.
    if r.failure > 0.0 {
        lines.push((format!("{:.0}%", r.failure * 100.0), TOOLTIP_BAD));
    }

    lines.push(match feasibility(inventory, r, heat) {
        Feasibility::Ready => (
            format!("x{}", possible_crafts(inventory, r)),
            TOOLTIP_GOOD,
        ),
        Feasibility::NoRoom => (language.text(Msg::NoRoom).to_string(), TOOLTIP_BAD),
        // A player holding everything the recipe wants and standing in
        // a field is being told to go and light a fire, which is a
        // completely different instruction from "find more tin" -- see
        // `Feasibility::NeedsFire`.
        Feasibility::NeedsFire => (language.text(Msg::NeedFire).to_string(), TOOLTIP_BAD),
        Feasibility::NeedsForge => (language.text(Msg::NeedKiln).to_string(), TOOLTIP_BAD),
        Feasibility::NeedsBloomery => {
            (language.text(Msg::NeedBloomery).to_string(), TOOLTIP_BAD)
        }
        Feasibility::NeedsWorkshop(station) => {
            use primitive_shared::crafting::Station;
            let line = match station {
                Station::Mason => Msg::NeedMason,
                Station::Wheel => Msg::NeedWheel,
                Station::Leather => Msg::NeedLeatherBench,
                _ => Msg::NeedBench,
            };
            (language.text(line).to_string(), TOOLTIP_BAD)
        }
        Feasibility::MissingIngredients => match missing_ingredient(inventory, r) {
            Some((block, short)) => (
                format!("{} {short}x {}", language.text(Msg::Need), names::block(block, language)),
                TOOLTIP_DIM,
            ),
            None => (language.text(Msg::No).to_string(), TOOLTIP_DIM),
        },
    });
    lines
}

// ---- sort button ----

const SORT_WIDTH: f32 = 0.215;
/// How tall it is where a mouse points at it. See `sort_button_rect` for
/// what a finger does to that.
const SORT_HEIGHT: f32 = 0.052;

/// Where the tidy button sits: the right end of the header band, clear of
/// both titles and of everything below them.
///
/// A finger's height where a finger is what presses it. It was 0.052 at
/// every scale, which on a 1220-tall phone is 52 device pixels of a
/// 91-pixel floor -- and unlike the way out, this one has no second,
/// larger gesture standing behind it.
pub fn sort_button_rect() -> Rect {
    // Measured down from the panel's own top edge rather than from
    // `content_top() + header_height()`, which is where it used to be:
    // the tab strip was added between the two and the button would
    // otherwise have stayed where the content now starts, sitting on the
    // tabs. `panel_rect().y1` is the header band's ceiling by
    // construction, so this cannot drift again.
    let top = panel_rect().y1 - 0.014;
    let right = recipe_left() + recipe_grid_width();
    Rect::new(right - SORT_WIDTH, top - widgets::tappable(SORT_HEIGHT), right, top)
}

fn sort_button(p: &mut Painter, cursor: Option<(f32, f32)>, language: Language) {
    let rect = sort_button_rect();
    let hovered = cursor.is_some_and(|(x, y)| rect.contains(x, y));
    p.slab(
        rect,
        if hovered { widgets::BUTTON_HOVER } else { widgets::BUTTON },
    );
    let scale = 0.72;
    let label = language.text(Msg::TidyPile);
    p.text(
        label,
        rect.centre_x() - widgets::ink_width(label, scale) / 2.0,
        rect.centre_y() + widgets::cell_height(scale) / 2.0 - 0.004,
        scale,
        widgets::INK,
    );
}

/// Which texture to show for a block in the pack.
///
/// A block that has asked for a picture of its own gets it -- see
/// `texture::ITEM_SLOT`, and `ash` for the case it exists for: a tile of
/// ash says what a floor of it looks like and nothing about the handful
/// you are carrying. Otherwise a face, and the side rather than the top,
/// because for grass the side shows the green strip over dirt and that
/// is what makes it recognisable at icon size.
pub(crate) fn icon_layer(layers: &FaceLayers, block: primitive_shared::types::BlockId) -> u32 {
    if let Some(layer) = layers.layer_for_item(block) {
        return layer;
    }
    let side = layers.layer_for_face(block, FACE_SOUTH);
    if side == 0 {
        layers.layer_for_face(block, FACE_TOP)
    } else {
        side
    }
}

/// A textured block icon filling most of a cell.
/// One block's picture, inside a slot, in a colour.
///
/// The colour is white for nearly everything and is the *material* for a
/// garment, whose picture is greyscale -- see `types::garment_tint` for
/// why twelve garments share four pictures. Callers get it from
/// `hotbar::icon_tint`, which is the one place that decision is made.
pub(crate) fn icon_tinted(p: &mut Painter, cell: Rect, layer: u32, tint: [f32; 4]) {
    let inset = CELL * 0.16;
    textured(
        p,
        Rect::new(
            cell.x0 + inset,
            cell.y0 + inset,
            cell.x1 - inset,
            cell.y1 - inset,
        ),
        layer,
        tint,
    );
}

/// One textured quad. The block icons everywhere on this screen -- slots,
/// recipe rows, the stack on the cursor -- are all this.
pub(crate) fn textured(p: &mut Painter, rect: Rect, layer: u32, tint: [f32; 4]) {
    // v = 0 is the top of the image, so the top edge takes v = 0.
    for (position, uv) in [
        ([rect.x0, rect.y0], [0.0, 1.0]),
        ([rect.x1, rect.y0], [1.0, 1.0]),
        ([rect.x1, rect.y1], [1.0, 0.0]),
        ([rect.x0, rect.y0], [0.0, 1.0]),
        ([rect.x1, rect.y1], [1.0, 0.0]),
        ([rect.x0, rect.y1], [0.0, 0.0]),
    ] {
        p.vertices.push(HotbarVertex {
            position,
            uv,
            tex_layer: layer,
            tint,
        });
    }
}

/// A small picture beside a word.
///
/// **Icons rather than more words.** A screen full of labels is a screen
/// a player reads; a screen where the fuel slot has a flame over it and
/// the title has the thing's own block on it is a screen they recognise.
/// The pictures cost nothing -- every block texture and every effect
/// tile is already in the array the interface samples -- and they work in
/// four languages at once, which is the other half of why they are worth
/// the space.
pub(crate) fn icon_beside(p: &mut Painter, at: (f32, f32), size: f32, layer: u32) -> Rect {
    let rect = Rect::new(at.0, at.1 - size, at.0 + size, at.1);
    textured(p, rect, layer, [1.0, 1.0, 1.0, 1.0]);
    rect
}

// ---- crafting ----
//
// A block of cells beside the pack rather than a list of rows. The
// recipes are shapeless (see `primitive_shared::crafting`), so there is
// nothing to arrange on a bench: a cell per recipe, and one click to
// make it.
//
// The cell shows the *result* -- the picture of the thing and how many
// of it -- because that is what the player is shopping for, and because
// it makes a recipe look like what it would become once it is in the
// pack. Everything else about a recipe is a question about one of them,
// and is answered for one of them: point at it and the tooltip gives
// the name, the cost and whether it is possible, while the pack lights
// up the slots it would spend.

/// How many recipes stand side by side.
///
/// **Crafting is a grid of the same cells the pack is.** It used to be
/// a column of wide rows, each one a sentence in pictures -- name,
/// count, `stone + stone -> wall` -- and beside a grid of forty square
/// slots it read as a second screen borrowed from somewhere else. The
/// eye had to learn two ways of looking at the same window.
///
/// A recipe is a thing you can have, drawn the way everything else you
/// can have is drawn: one cell, the picture of what it makes, and how
/// many of them. What it *costs* is a question about one recipe at a
/// time, so it is answered one recipe at a time -- by the tooltip, and
/// by the pack lighting up the slots it would spend. Both of those were
/// already there; the row was spending width to say permanently what
/// they say on demand.
///
/// **Four.** It was seven, and five before that. Seven was chosen when
/// the grid showed every recipe in the game and "all of it at once, with
/// no scrolling" was worth two columns of width. That stopped being the
/// question when the grid began showing only what the pack can make
/// (`offered`): a list of what is possible right now is short, and when it
/// is not, it scrolls with a bar that says so.
///
/// What the three columns bought is the body. This screen fits a square
/// window with four thousandths of width to spare
/// (`the_whole_screen_fits_a_square_window`), so a figure a third of a
/// unit wide could come from nowhere else -- and a figure whose arm is
/// narrower than two thirds of a slot is a drop target a thumb misses.
/// Four by four is sixteen things makeable at once before the bar appears.
const RECIPE_COLUMNS: usize = 4;


/// Width of the recipe block: the same cells and the same gaps as the
/// pack, so the two grids line up rather than merely sit next to each
/// other.
fn recipe_grid_width() -> f32 {
    RECIPE_COLUMNS as f32 * CELL + (RECIPE_COLUMNS as f32 - 1.0) * GAP
}

/// Where a recipe cell sits.
///
/// Filled left to right and then down, from the top of the content band.
///
/// **`place` is a position on screen, not a recipe id.** The two used to
/// be the same number, and stopped being when the grid started showing
/// only what the player can actually make: what sits in the third cell
/// depends on what is in the pack. The recipe's identity on the wire is
/// still its index in `RECIPES`, and `offered` is the one place the two
/// are translated -- see there for why the ordering underneath never
/// reshuffles.
pub fn recipe_rect(place: usize, scroll: usize) -> Rect {
    let place = place.saturating_sub(scroll);
    let column = place % RECIPE_COLUMNS;
    let row = place / RECIPE_COLUMNS;
    let x0 = recipe_left() + column as f32 * (CELL + GAP);
    // Level with the first row of the pack: two grids of the same cell
    // whose rows do not line up look like a mistake, and this is the
    // one line that keeps them honest.
    let y1 = grid_top() - row as f32 * (CELL + GAP);
    Rect::new(x0, y1 - CELL, x0 + CELL, y1)
}

/// Rows of recipes on screen at once.
///
/// **A number, and it used to be "however many rows the pack has".**
/// That read as elegant -- one height for both columns, nothing to keep
/// in step -- and it was a coupling with nothing behind it: the pack was
/// then halved (`inventory::STORAGE_ROWS`), and halving how much a
/// player can carry silently halved how many recipes they can see. Nine
/// offered rows into eight cells, found by a test rather than by
/// anybody's judgement.
///
/// Five, which is what the body column needs
/// (`equipment_column_height`) -- so the two tall columns are exactly
/// the same height and the band they share is the height of both.
const RECIPE_ROWS: usize = 5;

/// How many recipes are on screen at once.
pub fn visible_recipes() -> usize {
    RECIPE_ROWS * RECIPE_COLUMNS
}

/// The recipes worth showing, in table order, as indices into `RECIPES`.
///
/// **Only what the player can actually make.** The grid used to show
/// every recipe in the game with the impossible ones veiled over, which
/// is a list that grows with the game and is mostly grey: a player
/// hunting for what to do next reads twenty-five cells to find the three
/// that mean anything. Now the twenty-two that need something they have
/// not got are simply not there, and the crafting block is a list of
/// what is possible right now -- which is the question being asked when
/// someone opens it.
///
/// Judged on ingredients, not on room in the pack. A recipe you have the
/// materials for but nowhere to put the result is a recipe you can make
/// in a moment, by dropping one thing; one whose ingredients you have
/// never seen is a different kind of absent. So `NoRoom` stays on screen
/// and says so, and only `MissingIngredients` is hidden.
///
/// Table order is kept. What is on offer changes as the pack does --
/// there is no way round that, it is the point -- but two recipes never
/// swap places with each other, so a thing that was left of another is
/// still left of it the next time both are available.
pub fn offered(inventory: &Inventory) -> Vec<usize> {
    use primitive_shared::crafting::{has_ingredients, RECIPES};
    // **The ingredients, not the verdict.** `feasibility` reports the
    // station before it counts anything, so filtering on it made this
    // list depend on whether the player happened to be standing at a
    // fire: empty-handed in a field offered every metal recipe in the
    // game, and lighting a fire made them all disappear. What belongs on
    // the list is what the player has the makings of; whether they are
    // somewhere they can do it is what the row itself says.
    RECIPES
        .iter()
        .enumerate()
        // **Hands only.** What a fire makes is made *in* the fire now:
        // you open a hearth, load it and let it work (see
        // `primitive_shared::hearth`). Leaving those rows here would
        // offer a player a smelt this screen can no longer run, and the
        // server would refuse it with nothing on screen to explain why.
        //
        // A workshop row is not a fire's and stays: it is made from the pack
        // like a hand row, and the row itself says "needs a workbench" when
        // there is none within reach -- the same reason the list does not
        // hide a row for the lack of a fire, above.
        .filter(|(_, recipe)| !recipe.station.is_hearth() && has_ingredients(inventory, recipe))
        .map(|(index, _)| index)
        .collect()
}

/// Where the window onto the offered list actually starts.
///
/// The stored scroll position is bounded by the whole recipe table, not
/// by the shorter list of what can be made right now -- otherwise using
/// up an ingredient would drag the view back up the page under the
/// player's hand. Clamping here instead means the position survives the
/// list getting shorter and comes back when it grows again.
fn visible_scroll(scroll: usize, offered: usize) -> usize {
    scroll.min(offered.saturating_sub(visible_recipes()))
}

/// Whether the offered list is longer than the window on it.
fn recipes_overflow(offered: usize) -> bool {
    offered > visible_recipes()
}

/// Left edge of the slot grid.
fn grid_left() -> f32 {
    let (grid_width, _) = grid_size();
    // The whole screen is three columns now -- the body, the pack, the
    // recipes -- and the middle of it is what gets centred. The body
    // column shifts the other two right by exactly its own width, which
    // is what keeps the panel symmetrical about the screen rather than
    // symmetrical about the pack with a strip hanging off the side.
    -(grid_width + COLUMN_GAP + recipe_grid_width()) / 2.0 + body_column_width() / 2.0
}

/// How much the body takes up: the figure, the four squares beside it,
/// and the gaps after each.
///
/// Named, because it appears in two places -- the grid's own offset and
/// the panel's left edge -- and two copies of a four-term sum are two
/// places to forget the term that was added last, which is how a figure
/// ends up hanging off the panel.
fn body_column_width() -> f32 {
    MANNEQUIN_WIDTH + MANNEQUIN_GAP + CELL + EQUIPMENT_GAP
}

/// Left edge of the recipe block.
fn recipe_left() -> f32 {
    let (grid_width, _) = grid_size();
    grid_left() + grid_width + COLUMN_GAP
}

/// Top of both grids: the pack's first row and the first row of
/// recipes start here.
fn grid_top() -> f32 {
    content_top() - GRID_LABEL
}

// ---- two trays and a score line ----
//
// **The screen is three things and looked like one.** The body squares,
// the pack and the recipes are three different kinds of place -- one you
// drop a garment on, one you rummage in, one you read -- and they were
// drawn as thirty-nine identical wells on one flat slab, told apart by
// the width of the gap between them. That gap is 0.011 inside a grid and
// 0.012 between the pack and the body column: a thousandth of a screen,
// under a pixel at the size this is drawn. So the body column read as a
// fifth column of the pack, and the only thing saying otherwise was a
// four-letter caption in the quiet ink.
//
// The obvious fix -- widen the gaps -- is not available. The panel is
// 1.995 wide and the window it has to fit is 2.0 across at its worst
// (see `the_whole_screen_fits_a_square_window`), so there are four
// thousandths of width in the whole screen and no more.
//
// What costs no width is depth: a group stands in a shallow tray cut
// into the stone, and the tray runs up far enough to carry the group's
// own caption, so a word belongs visibly to the thing under it.
//
// **Two trays, not three, and that is the width talking.** A tray needs
// `TRAY_PAD` on each side of what it holds, so two of them side by side
// need 0.024 between the columns; there is 0.012 between the body and
// the pack and there is nowhere to get the other twelve thousandths
// from. Tried and rejected: pads of 0.006, which is under one bevel, so
// the tray's own edge would be drawn through the squares it was framing.
//
// So the seam that has 0.030 to spend gets the tray edge -- the pack
// against the recipes, which is also the bigger of the two distinctions:
// things you have against things you could make. The seam that has 0.012
// gets a line scored into the tray floor between the body squares and
// the pack ([`body_divider`]), which needs no room at all because it is
// drawn *in* the gap that is already there.

/// How far a tray stands out past the cells in it.
///
/// The same as the gap between two slots plus a bevel, so the border
/// round a group is visibly wider than the gap inside it. That
/// difference is the whole message.
pub(crate) const TRAY_PAD: f32 = 0.012;

/// The tray under the body squares and the pack, captions included.
///
/// Its floor is the belt strip's, not the bottom row's: the strip is a
/// raised slab sitting *in* this tray, and a tray that stopped at the
/// slots would have the belt hanging over its edge.
fn pack_tray() -> Rect {
    Rect::new(
        // From the figure, which is the body column's left edge: the
        // figure, the squares and the pack are all things *on* the player.
        mannequin_rect().x0 - TRAY_PAD,
        // **The lower of the belt and the last worn square**, which used
        // to be simply the belt. With four squares beside a four-row
        // pack the two ended level and the distinction did not exist;
        // there are five squares now and two rows, so the body column
        // reaches a third of a screen below the belt and a tray cut to
        // the belt would have the feet and the back hanging over its
        // edge.
        belt_strip().y0.min(equipment_rect(primitive_shared::equipment::SLOTS - 1).y0) - TRAY_PAD,
        slot_rect(SLOTS - 1).x1 + TRAY_PAD,
        content_top(),
    )
}

/// The line scored between the body squares and the pack.
///
/// Exactly the gap that is already between them, floor to ceiling of the
/// tray they share. It costs no width -- which is the whole reason it is
/// a line and not a second tray -- and what it has to say is small:
/// dropping a tunic on a square to the left of this line puts it on, and
/// dropping it to the right of it puts it away.
fn body_divider() -> Rect {
    let tray = pack_tray();
    Rect::new(
        equipment_rect(0).x1,
        tray.y0 + widgets::BEVEL,
        slot_rect(HOTBAR_SLOTS).x0,
        tray.y1 - widgets::BEVEL,
    )
}

/// The tray under the recipe block.
///
/// Wider on the right than on the left, because the scroll track is
/// drawn out there (see `crafting_panel`) and a groove hanging half off
/// the edge of the tray it belongs to is the sort of detail that reads
/// as a mistake without anybody being able to say what.
fn recipe_tray() -> Rect {
    Rect::new(
        recipe_left() - TRAY_PAD,
        // **Down to the pack tray's floor**, not to the last row of
        // recipes. The pack has a gap before its belt and the recipes have
        // none, so a tray cut to its own cells stopped three hundredths
        // short of the one beside it -- two trays side by side with two
        // floors, which is the one thing a row of trays must not look
        // like. The lower of the two, in case the recipe block is ever the
        // taller.
        (grid_top() - recipes_height() - TRAY_PAD).min(pack_tray().y0),
        recipe_left() + recipe_grid_width() + 0.022,
        content_top(),
    )
}

/// The raised strip the ten in-hand slots are cut into.
fn belt_strip() -> Rect {
    let first = slot_rect(0);
    let last = slot_rect(HOTBAR_SLOTS - 1);
    Rect::new(
        first.x0 - 0.012,
        first.y0 - 0.012,
        last.x1 + 0.012,
        last.y1 + 0.012,
    )
}

/// The whole screen's extent, panel padding included.
fn panel_rect() -> Rect {
    Rect::new(
        // Past the body column rather than past the pack: the figure and
        // the four squares are inside the panel, and a panel that stopped
        // at the pack would have them hanging over its edge.
        grid_left() - body_column_width() - PANEL_PAD,
        -content_top() - FOOTER_HEIGHT,
        recipe_left() + recipe_grid_width() + PANEL_PAD,
        // The tab strip is a band of its own between the title and the
        // content, so the panel is that much taller. Growing the *header*
        // instead was tried and rejected: the tidy button is positioned
        // inside the header band (`sort_button_rect`) and is centred in
        // it, so a header twice as tall is a button floating in the
        // middle of nothing.
        content_top() + header_height() + tab_band(),
    )
}

// ---- the three tabs ----
//
// A strip of three across the top of the panel, under the title and over
// everything else. Full width, three equal parts, because a tab whose
// width depends on its own label moves when the language changes -- and
// a control that is in a different place in Polish is a control the
// player has to find twice.

/// Height of one tab.
///
/// `tappable`, like the tidy button: this is the one control on the pack
/// screen a phone player uses on every visit, and it is at the top of
/// the panel where a thumb is least accurate.
fn tab_height() -> f32 {
    widgets::tappable(0.058)
}

/// How much height the strip takes out of the panel, air included.
fn tab_band() -> f32 {
    tab_height() + TAB_MARGIN * 2.0
}

/// Air above and below the strip.
///
/// Above it separates the tabs from the title; below it separates them
/// from the trays. The same number for both, because a strip closer to
/// one than the other reads as belonging to it.
const TAB_MARGIN: f32 = 0.014;

/// The gap between two tabs.
///
/// Narrow on purpose: three tabs with a slot's gap between them read as
/// three buttons that happen to be in a row, and what this has to say is
/// that they are three faces of one thing.
const TAB_GAP: f32 = 0.008;

/// Where one tab is drawn.
///
/// **The exact inverse of [`tab_at`]**, which is not a style note here:
/// the interface is laid out in its own space and scaled, so a hit test
/// written independently of the drawing is a strip that looks right and
/// answers in the wrong place. There is a test that says so.
pub fn tab_rect(index: usize) -> Rect {
    let panel = panel_rect();
    let inner = panel.width() - PANEL_PAD * 2.0;
    let count = ALL_TABS.len() as f32;
    let width = (inner - TAB_GAP * (count - 1.0)) / count;
    let left = panel.x0 + PANEL_PAD + index as f32 * (width + TAB_GAP);
    let top = panel.y1 - header_height() - TAB_MARGIN;
    Rect::new(left, top - tab_height(), left + width, top)
}

/// Which tab a point is over, as an index into [`ALL_TABS`].
pub fn tab_at(cursor: (f32, f32)) -> Option<usize> {
    (0..ALL_TABS.len()).find(|&index| tab_rect(index).contains(cursor.0, cursor.1))
}

/// How much of the screen the pack takes, about the middle it is
/// centred on.
///
/// What the frame loop asks before growing it, and what a click is
/// divided by on the way back in -- see `widgets::Layout::fit` and
/// `place_cursor`. Measured off `panel_rect` rather than written down,
/// because the panel is itself derived from the cell size and a second
/// copy of the answer would go stale the first time a cell changed.
/// How much bigger than its authored size the pack is drawn.
///
/// **One definition, used to draw it and to hit-test it** -- the frame
/// loop multiplies the geometry by this and `place_cursor` divides a
/// click by it, so a second copy of the arithmetic anywhere is a pack
/// whose slots answer to the wrong square.
///
/// **Not floored at a finger**, which it was for a day: a slot at the
/// size a phone starts at comes out three pixels under one, and forcing
/// the pack up to meet that made every setting below 1.5 draw the same
/// pack. See `Layout::finger`, which is a floor under a widget and not
/// under a screen.
pub fn grow_by(layout: crate::ui::widgets::Layout) -> f32 {
    layout.fit(extent())
}

pub fn extent() -> (f32, f32) {
    let panel = panel_rect();
    (
        panel.width() / 2.0,
        panel.y1.abs().max(panel.y0.abs()),
    )
}

/// Which recipe a point is over, as an index into `RECIPES`.
///
/// Takes the pack because the grid holds only what can be made from it:
/// the cell under the cursor is a *position*, and `offered` is what says
/// which recipe is standing in it.
pub fn recipe_at(
    cursor: (f32, f32),
    scroll: usize,
    inventory: &Inventory,
) -> Option<usize> {
    let offered = offered(inventory);
    let scroll = visible_scroll(scroll, offered.len());
    let last = (scroll + visible_recipes()).min(offered.len());
    (scroll..last)
        .find(|&place| recipe_rect(place, scroll).contains(cursor.0, cursor.1))
        .map(|place| offered[place])
}

/// Draws the recipe column.
fn crafting_panel(
    p: &mut Painter,
    layers: &FaceLayers,
    inventory: &Inventory,
    hovered: Option<usize>,
    scroll: usize,
    language: Language,
    heat: primitive_shared::crafting::Heat,
) {
    use primitive_shared::crafting::{feasibility, RECIPES};

    // What can be made from what is in the pack, and nothing else. See
    // `offered`.
    let offered = offered(inventory);
    let scroll = visible_scroll(scroll, offered.len());

    // Level with the word over the pack, because it is the same kind of
    // word over the same kind of grid.
    let first = recipe_rect(scroll, scroll);
    // The count stays the small print it was: it is a number beside a
    // heading, not a heading.
    let label_scale = 0.62;
    let counted = recipes_overflow(offered.len()).then(|| {
        format!("{}-{} {} {}",
            scroll + 1,
            (scroll + visible_recipes()).min(offered.len()),
            language.text(Msg::Of),
            offered.len())
    });
    // The caption at the size every stone screen writes one, fitted to the
    // column less whatever the count beside it takes -- `MAKING THINGS` and
    // `1-16 of 23` share one line.
    let heading = language.text(Msg::Crafting);
    let room = recipe_grid_width()
        - counted.as_ref().map_or(0.0, |c| widgets::ink_width(c, label_scale) + 0.02);
    let heading_scale = widgets::fitted_scale(heading, widgets::CAPTION_SCALE, room, 0.7);
    // Its own height above the row: `top` is the top of the text and the
    // glyphs hang down from it, so anything less writes the word across the
    // slots it names.
    p.text(heading, first.x0, widgets::caption_top_over(first.y1, heading_scale), heading_scale, widgets::INK_DIM);
    // How far down a longer table this is, when there is one.
    if let Some(counted) = counted {
        let width = widgets::ink_width(&counted, label_scale);
        p.text(
            &counted,
            recipe_left() + recipe_grid_width() - width,
            first.y1 + 0.030,
            label_scale,
            widgets::INK_DIM,
        );
    }

    let last = (scroll + visible_recipes()).min(offered.len());

    // A bar down the side, and only when there is something to scroll.
    // Without it the list is a list of eight recipes that mysteriously
    // changes when the wheel is touched; with it, it is obviously a
    // window onto a longer one.
    if recipes_overflow(offered.len()) && last > scroll {
        let top = recipe_rect(scroll, scroll).y1;
        let bottom = recipe_rect(last - 1, scroll).y0;
        let track = Rect::new(
            recipe_left() + recipe_grid_width() + 0.008,
            bottom,
            recipe_left() + recipe_grid_width() + 0.016,
            top,
        );
        // A groove with a stone thumb in it, like everything else on
        // this panel.
        p.well(track, widgets::WELL);
        let span = top - bottom;
        let fraction = visible_recipes() as f32 / offered.len() as f32;
        let travel = span * (1.0 - fraction);
        let offset = travel * scroll as f32
            / offered.len().saturating_sub(visible_recipes()).max(1) as f32;
        let thumb = Rect::new(track.x0, top - offset - span * fraction, track.x1, top - offset);
        p.slab(thumb, widgets::BUTTON);
    }

    for (place, &index) in offered.iter().enumerate().take(last).skip(scroll) {
        let r = &RECIPES[index];
        let rect = recipe_rect(place, scroll);
        let ready = feasibility(inventory, r, heat).is_ready();

        // The same cell the pack is made of, holding the picture of what
        // the recipe makes and how many it makes at once -- which is
        // exactly what a slot holding that stack would look like. That
        // is the point: what you would get, drawn where you would get
        // it.
        draw_slot(
            p,
            rect,
            layers,
            Some(r.output.0),
            r.output.1,
            if hovered == Some(index) { SlotEdge::Hovered } else { SlotEdge::Plain },
        );
        // ...and a recipe you cannot run yet recedes rather than
        // shouting. The veil goes over the icon too: see `RECIPE_VEIL`.
        if !ready {
            p.quad(rect, RECIPE_VEIL);
        }

    }
}

/// Total size of the slot grid.
fn grid_size() -> (f32, f32) {
    let width = HOTBAR_SLOTS as f32 * CELL + (HOTBAR_SLOTS as f32 - 1.0) * GAP;
    let rows = STORAGE_ROWS + 1;
    let height = rows as f32 * CELL + (rows as f32 - 1.0) * GAP + HOTBAR_SPLIT;
    (width, height)
}

/// Height of the recipe block.
fn recipes_height() -> f32 {
    let shown = visible_recipes().min(primitive_shared::crafting::RECIPES.len().max(1));
    let rows = shown.div_ceil(RECIPE_COLUMNS).max(1) as f32;
    rows * CELL + (rows - 1.0) * GAP
}

/// How tall the column of worn squares is.
///
/// **It is the tallest thing on the screen now, and it did not used to
/// be.** There are five squares since the rucksack arrived
/// (`equipment::Slot::Back`) and the pack behind them is two rows, where
/// it was four -- so the body, not the pack, is what the band has to be
/// able to hold. Before either change the three columns happened to
/// agree and nothing had to say so.
fn equipment_column_height() -> f32 {
    let squares = primitive_shared::equipment::SLOTS as f32;
    squares * CELL + (squares - 1.0) * GAP
}

/// Top of the band the three columns live in.
///
/// Whichever column is tallest sets it, so none of them can run out of
/// the panel: the recipe list grows with `RECIPE_ROWS`, the grid with
/// the slot count, and the body column with the number of things a
/// person can wear.
fn content_top() -> f32 {
    let (_, grid_height) = grid_size();
    grid_height
        .max(recipes_height())
        .max(equipment_column_height())
        / 2.0
}

/// Where a slot sits on screen.
///
/// Slots 0..HOTBAR_SLOTS are the hotbar and are drawn as the *bottom*
/// row, matching where the real hotbar is. Storage runs above it, in
/// reading order. The grid is centred in the content band rather than
/// hung from its top, so it sits opposite the middle of the recipe list
/// instead of leaving a hole under itself.
/// Where the four equipment squares sit.
///
/// **Down the left of the pack rather than beside the crafting column**,
/// and the reason is which gesture they take part in. What a player does
/// with these is drag a garment out of the pack and drop it on one -- so
/// they belong next to the pack, in the direction the hand is already
/// travelling. The crafting column on the right is a *list you read*,
/// and putting a drop target in it would mean two different kinds of
/// thing in one column.
///
/// Head at the top and feet at the bottom, which is the one ordering
/// nobody has to learn.
pub fn equipment_rect(slot: usize) -> Rect {
    // Aligned with the first slot row and stepping down from it, so the
    // four squares line up with the pack rather than floating beside it.
    let top = content_top() - GRID_LABEL - slot as f32 * (CELL + GAP);
    let left = grid_left() - CELL - EQUIPMENT_GAP;
    Rect::new(left, top - CELL, left + CELL, top)
}

/// How far the equipment column sits from the pack.
///
/// Wider than the gap between two slots, so the four squares read as a
/// separate thing rather than as a fifth column of the pack -- which
/// matters, because dropping a tunic in the pack and dropping it on the
/// body are different gestures with different outcomes.
const EQUIPMENT_GAP: f32 = 0.012;

/// How wide the figure is.
///
/// **A third of a unit, which is what an arm needs to be dropped on.** At
/// this width an arm is two thirds of a slot across and a leg about the
/// same; narrower, and a bandage aimed with a thumb lands on the tray
/// beside the arm. The height is the pack's, so the head sits level with
/// the first row and the feet with the belt, and the four squares beside
/// it line up with the parts they dress.
const MANNEQUIN_WIDTH: f32 = 0.300;
/// Air between the figure and the squares beside it: wider than a gap in
/// the grid, so the arm and the head square do not read as one thing.
const MANNEQUIN_GAP: f32 = 0.016;

/// Where the figure is drawn: left of the equipment squares, as tall as
/// the pack.
pub fn mannequin_rect() -> Rect {
    let left = equipment_rect(0).x0 - MANNEQUIN_GAP - MANNEQUIN_WIDTH;
    // **Down to the last worn square, not down to the belt.** The two
    // were the same line while there were four squares beside a four-row
    // pack; now the pack is two rows and there are five squares, and a
    // figure cut off at the belt would be a head and a chest standing
    // beside a column of squares twice its height. The squares are the
    // right thing to match: they are the same body.
    Rect::new(
        left,
        equipment_rect(primitive_shared::equipment::SLOTS - 1).y0,
        left + MANNEQUIN_WIDTH,
        grid_top(),
    )
}

/// Which part of the figure a point is over. The same rectangles the
/// figure is drawn from -- see `mannequin::part_at`.
pub fn body_part_at(cursor: (f32, f32)) -> Option<Part> {
    crate::ui::mannequin::part_at(mannequin_rect(), cursor)
}

/// Which body part the cursor is over, if any.
pub fn equipment_at(cursor: (f32, f32)) -> Option<usize> {
    (0..primitive_shared::equipment::SLOTS)
        .find(|&slot| equipment_rect(slot).contains(cursor.0, cursor.1))
}

pub fn slot_rect(slot: usize) -> Rect {
    let left = grid_left();
    // **Hung from the floor of the band, not from its ceiling.**
    //
    // It was top-aligned, and the note here said the empty space would
    // then be all in one place, under the belt, "which is where the
    // readout goes". That was true while the pack was four rows deep and
    // the space under it was a finger's width. The pack is two rows now
    // (`inventory::STORAGE_ROWS`) in a band five squares tall -- the
    // body column sets that, see `equipment_column_height` -- and
    // top-aligned it left a third of the tray empty *below* the belt,
    // against the tray's own floor, with nothing under it. A gap against
    // a floor reads as something missing.
    //
    // Bottom-aligned, the belt's underside is level with the last worn
    // square and with the last row of recipes: all three columns stand
    // on one line, the tray is full at the bottom, and what air there is
    // sits between the pile and the captions at the top, bounded on both
    // sides by columns that run the whole height. That is the same
    // argument the old note made, applied to a screen whose proportions
    // have changed.
    let top = equipment_rect(primitive_shared::equipment::SLOTS - 1).y0 + grid_size().1;

    let (column, row_from_top, extra) = if slot < HOTBAR_SLOTS {
        (slot, STORAGE_ROWS, HOTBAR_SPLIT)
    } else {
        let storage = slot - HOTBAR_SLOTS;
        (storage % HOTBAR_SLOTS, storage / HOTBAR_SLOTS, 0.0)
    };

    let x0 = left + column as f32 * (CELL + GAP);
    let y1 = top - row_from_top as f32 * (CELL + GAP) - extra;
    Rect::new(x0, y1 - CELL, x0 + CELL, y1)
}

/// Which *place in the grid* a point is over, if any.
///
/// A place, not a slot: which slot of the inventory is drawn in a place
/// depends on the page (see [`slot_in_place`]), and a free function has
/// no page. `InventoryScreen::slot_at` is the one that answers with a
/// slot.
pub fn slot_place_at(cursor: (f32, f32)) -> Option<usize> {
    (0..SLOTS).find(|&slot| slot_rect(slot).contains(cursor.0, cursor.1))
}

#[cfg(test)]
mod tests {
    use primitive_shared::inventory::MAX_STACK;
    use super::*;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_STONE};

    /// How many vertices one slot holding `stack` draws.
    fn drawn(stack: primitive_shared::inventory::Stack) -> Vec<crate::ui::hotbar::HotbarVertex> {
        let mut p = Painter::new(FontAtlas::for_test());
        let layers = FaceLayers::empty_for_test();
        draw_slot_stack(&mut p, Rect::new(0.0, 0.0, CELL, CELL), &layers, Some(stack), SlotEdge::Plain);
        p.vertices
    }

    #[test]
    fn a_poor_and_a_fine_piece_carry_a_corner_and_a_plain_one_does_not() {
        use primitive_shared::quality::Quality;
        use primitive_shared::types::BLOCK_COOKED_MEAT;
        let meat = |q: f32| primitive_shared::inventory::Stack::new(BLOCK_COOKED_MEAT, 3).with_quality(Quality::from_fraction(q));
        let unmarked = drawn(primitive_shared::inventory::Stack::new(BLOCK_COOKED_MEAT, 3)).len();
        assert_eq!(drawn(meat(0.5)).len(), unmarked, "a plain piece was marked");
        assert_eq!(drawn(meat(0.7)).len(), unmarked, "a good piece was marked");
        assert!(drawn(meat(0.1)).len() > unmarked, "a poor piece carries no corner");
        assert!(drawn(meat(0.95)).len() > unmarked, "a fine piece carries no corner");
        assert_ne!(quality_corner(meat(0.1).quality().band()), quality_corner(meat(0.95).quality().band()), "poor and fine look the same");
    }

    #[test]
    fn a_dull_and_a_blunt_tool_are_marked_and_a_sharp_or_dulled_one_is_not() {
        use primitive_shared::tools::with_edge;
        use primitive_shared::types::{BLOCK_BRONZE_AXE, BLOCK_COPPER_SAW, BLOCK_JUG};
        assert_eq!(edge_corner(BLOCK_BRONZE_AXE), None, "a sharp axe carries a mark");
        assert_eq!(edge_corner(with_edge(BLOCK_BRONZE_AXE, 1)), None, "every tool fresh off the stone is marked");
        let dull = edge_corner(with_edge(BLOCK_COPPER_SAW, 2));
        let blunt = edge_corner(with_edge(BLOCK_COPPER_SAW, 3));
        assert!(dull.is_some() && blunt.is_some() && dull != blunt, "dull and blunt do not read apart: {dull:?} {blunt:?}");
        assert_eq!(edge_corner(BLOCK_JUG), None, "a jug's corner was taken by an edge");
    }

    #[test]
    fn the_quality_corner_stays_inside_its_slot() {
        // Drawing only: a corner that spilled over the edge would be a
        // mark on the neighbour, and the neighbour is where the click goes.
        use primitive_shared::quality::Quality;
        let fine = primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_COOKED_MEAT, 1)
            .with_quality(Quality::from_fraction(1.0));
        for v in drawn(fine) {
            assert!((0.0..=CELL).contains(&v.position[0]) && (0.0..=CELL).contains(&v.position[1]), "drawn outside the slot at {:?}", v.position);
        }
    }

    /// Tapping beside the pack closes it -- unless something is held.
    ///
    /// **The way out on a phone**, now that the `X` is gone (see
    /// `Intent::Close`). The pack closes on Escape and on the inventory
    /// key, and a phone has the first only as the system's Back gesture
    /// and the second not at all while a screen owns the glass (see
    /// `world_owns_the_glass`) -- so this tap is what a thumb has.
    ///
    /// The exception is the chest's: a player holding a stack who taps
    /// beside the panel has almost always missed a slot, and closing on
    /// that would put the screen away and appear to swallow what they were
    /// carrying. So the first tap puts it back and the second one leaves.
    #[test]
    fn tapping_beside_the_pack_closes_it_but_only_with_empty_hands() {
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 64);
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.sync(&pack);

        // Well outside any panel this screen draws.
        let outside = (5.0, 5.0);

        let _ = click_slot(&mut screen, &pack, 0);
        assert!(screen.held().is_some(), "nothing was picked up to test with");
        screen.set_cursor(Some(outside));
        assert_eq!(screen.click(&pack, Button::Left, false), None);
        assert_eq!(screen.held(), None, "the stack was not put back");

        assert_eq!(screen.click(&pack, Button::Left, false), Some(Intent::Close));
    }

    /// The part of the figure a click lands on, for the tests below.
    fn click_part(screen: &mut InventoryScreen, pack: &Inventory, part: Part) -> Option<Intent> {
        let r = crate::ui::mannequin::part_rect(mannequin_rect(), part);
        screen.set_cursor(Some((r.centre_x(), r.centre_y())));
        screen.click(pack, Button::Left, false)
    }

    #[test]
    fn every_part_of_the_body_is_clicked_where_it_is_drawn_and_on_nothing_else() {
        // The figure is hit-tested before the squares and the pack, so a
        // part that overlapped either would take clicks meant for it.
        let panel = panel_rect();
        let tray = pack_tray();
        for part in Part::ALL {
            let r = crate::ui::mannequin::part_rect(mannequin_rect(), part);
            let centre = (r.centre_x(), r.centre_y());
            assert_eq!(body_part_at(centre), Some(part), "{part:?} misses itself");
            assert_eq!(slot_place_at(centre), None, "{part:?} is drawn on a slot");
            assert_eq!(equipment_at(centre), None, "{part:?} is drawn on a square");
            assert!(
                r.x0 >= tray.x0 && r.x1 <= tray.x1 && r.y0 >= tray.y0 && r.y1 <= tray.y1,
                "{part:?} hangs out of the tray"
            );
            assert!(r.x0 >= panel.x0 && r.y0 >= panel.y0, "{part:?} hangs off the panel");
            // Not so thin a thumb aimed at it lands beside it.
            assert!(
                r.width().min(r.height()) >= CELL * 0.6,
                "{part:?} is {:.3} across, under two thirds of a slot",
                r.width().min(r.height())
            );
        }
        for slot in 0..SLOTS {
            let s = slot_rect(slot);
            assert_eq!(body_part_at((s.centre_x(), s.centre_y())), None, "slot {slot} hits the figure");
        }
    }

    #[test]
    fn a_bandage_dropped_on_the_body_asks_the_server_to_dress_that_part() {
        use primitive_shared::types::BLOCK_BANDAGE;
        let mut pack = Inventory::new();
        pack.put_in_slot(0, primitive_shared::inventory::Stack::new(BLOCK_BANDAGE, 3));
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.sync(&pack);

        assert_eq!(click_slot(&mut screen, &pack, 0), None);
        assert_eq!(
            click_part(&mut screen, &pack, Part::LeftArm),
            Some(Intent::Treat { slot: 0, part: Part::LeftArm })
        );
        assert_eq!(screen.held(), None, "the bandage stayed on the pointer after the drop");
        // Empty hands on the figure ask for nothing: that is the tooltip.
        assert_eq!(click_part(&mut screen, &pack, Part::LeftArm), None);
    }

    #[test]
    fn a_stone_dropped_on_the_body_is_put_back_and_a_tunic_is_put_on() {
        use primitive_shared::inventory::Stack;
        use primitive_shared::types::BLOCK_LEATHER_TUNIC;
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE, 5));
        pack.put_in_slot(1, Stack::new(BLOCK_LEATHER_TUNIC, 1));
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.sync(&pack);

        let _ = click_slot(&mut screen, &pack, 0);
        assert_eq!(click_part(&mut screen, &pack, Part::Torso), None, "a stone was sent to the body");
        assert_eq!(screen.held(), None, "the stone stayed on the pointer");

        let _ = click_slot(&mut screen, &pack, 1);
        assert_eq!(click_part(&mut screen, &pack, Part::Torso), Some(Intent::Equip(1)));
    }

    #[test]
    fn a_wounded_body_is_drawn_differently_from_a_whole_one() {
        use primitive_shared::injury::Kind;
        let screen = {
            let mut s = InventoryScreen::new();
            s.open = true;
            s
        };
        let pack = stocked();
        let tints = |injuries: &Injuries| {
            screen
                .build_wounded(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &pack,
                    injuries,
                    Language::English,
                    1.0,
                )
                .iter()
                .map(|v| v.tint.map(f32::to_bits))
                .collect::<std::collections::BTreeSet<_>>()
        };
        let mut hurt = Injuries::default();
        hurt.inflict(Part::RightLeg, Kind::Fracture, 1.0);
        assert_ne!(tints(&Injuries::default()), tints(&hurt), "a broken leg drew as a whole one");
    }

    /// Puts the cursor in the middle of a pack square and clicks it
    /// with the named button. `click_slot` below is the same thing for
    /// the left one, which is what every other test wants.
    fn click_slot_with(
        screen: &mut InventoryScreen,
        pack: &Inventory,
        slot: usize,
        button: Button,
    ) -> Option<Intent> {
        let cell = slot_rect(slot);
        screen.set_cursor(Some((cell.centre_x(), cell.centre_y())));
        screen.click(pack, button, false)
    }

    #[test]
    fn dropping_a_handful_of_grain_on_a_jug_pours_it_in() {
        use primitive_shared::inventory::{filled_jug, Stack};
        use primitive_shared::types::{BLOCK_GRAIN, BLOCK_JUG, BLOCK_SAND};

        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_GRAIN, 20));
        pack.put_in_slot(1, Stack::new(BLOCK_JUG, 1));
        pack.put_in_slot(2, filled_jug(BLOCK_SAND, 4));
        pack.put_in_slot(3, Stack::new(BLOCK_STONE, 9));

        // Pick the grain up, drop it on the empty jug.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        assert_eq!(click_slot_with(&mut screen, &pack, 0, Button::Left), None);
        assert_eq!(
            click_slot_with(&mut screen, &pack, 1, Button::Left),
            Some(Intent::PourIntoJug { from: 0, jug: 1 })
        );

        // A jug already holding something *else* is the ordinary swap
        // it looks like -- pouring would have replaced the sand, which
        // is a quiet way of deleting it.
        let mut swapper = InventoryScreen::new();
        swapper.open = true;
        assert_eq!(click_slot_with(&mut swapper, &pack, 0, Button::Left), None);
        assert_eq!(
            click_slot_with(&mut swapper, &pack, 2, Button::Left),
            Some(Intent::Move { from: 0, to: 2 })
        );

        // ...and dropping something that does not pour on an empty jug
        // is still a swap, so the gesture cannot be used on meat.
        let mut mover = InventoryScreen::new();
        mover.open = true;
        assert_eq!(click_slot_with(&mut mover, &pack, 3, Button::Left), None);
        assert_eq!(
            click_slot_with(&mut mover, &pack, 1, Button::Left),
            Some(Intent::Move { from: 3, to: 1 })
        );
    }

    #[test]
    fn a_full_jug_is_tipped_out_by_a_right_click_and_still_picked_up_by_a_left_one() {
        use primitive_shared::inventory::{filled_jug, Stack};
        use primitive_shared::types::BLOCK_JUG;

        let mut pack = Inventory::new();
        pack.put_in_slot(0, filled_jug(primitive_shared::types::BLOCK_ASH, 7));
        pack.put_in_slot(1, Stack::new(BLOCK_JUG, 1));

        let mut screen = InventoryScreen::new();
        screen.open = true;
        assert_eq!(
            click_slot_with(&mut screen, &pack, 0, Button::Right),
            Some(Intent::EmptyJug(0))
        );
        // An empty one has nothing to tip out, so the right click is
        // the nothing it has always been.
        assert_eq!(click_slot_with(&mut screen, &pack, 1, Button::Right), None);

        // **The half that made the right button the right button.** A
        // left click still picks the full jug up, so a jug with
        // something in it can be moved, put in a chest and thrown out
        // like anything else.
        let mut mover = InventoryScreen::new();
        mover.open = true;
        assert_eq!(click_slot_with(&mut mover, &pack, 0, Button::Left), None);
        assert_eq!(mover.held(), Some(0), "a full jug could not be picked up");
    }

    /// A lit kiln within reach, which counts as a fire as well -- so it
    /// is "every station open", which is what these tests want when
    /// they are not about stations at all.
    const KILN: primitive_shared::crafting::Heat = primitive_shared::crafting::Heat {
        fire: false,
        kiln: true,
        bloomery: false,
        // ...and every workshop, for the same reason.
        workshops: u8::MAX,
    };

    fn stocked() -> Inventory {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 5);
        inventory.add(BLOCK_DIRT, 3);
        inventory
    }

    /// A pack that can actually make something.
    ///
    /// The grid shows only what the ingredients allow, so a fixture of
    /// stone and dirt -- neither of which is an ingredient of anything
    /// -- now draws an empty crafting block, and every test about
    /// pointing at or clicking a recipe needs a pack with a recipe in
    /// reach. Flint is the shortest such chain: a nodule knaps into
    /// flakes with nothing else at all.
    fn with_something_to_make() -> Inventory {
        let mut inventory = stocked();
        inventory.add(primitive_shared::types::BLOCK_FLINT, 8);
        inventory.add(primitive_shared::types::BLOCK_STICK, 4);
        inventory.add(primitive_shared::types::BLOCK_FIBER, 8);
        inventory
    }

    /// Where the first offered recipe is drawn, and which one it is.
    fn first_offered(inventory: &Inventory) -> (usize, Rect) {
        let offered = offered(inventory);
        assert!(!offered.is_empty(), "the fixture can make nothing at all");
        (offered[0], recipe_rect(0, 0))
    }

    fn centre_of(slot: usize) -> (f32, f32) {
        let r = slot_rect(slot);
        (r.centre_x(), r.centre_y())
    }

    #[test]
    fn every_slot_has_its_own_place_on_screen() {
        // Two slots sharing a rectangle would make one of them
        // unreachable by the mouse.
        for a in 0..SLOTS {
            for b in (a + 1)..SLOTS {
                assert_ne!(slot_rect(a), slot_rect(b), "slots {a} and {b} overlap");
                assert_ne!(
                    slot_place_at(centre_of(a)),
                    slot_place_at(centre_of(b)),
                    "slots {a} and {b} hit-test the same"
                );
            }
        }
    }

    #[test]
    fn hit_testing_agrees_with_where_things_are_drawn() {
        for slot in 0..SLOTS {
            assert_eq!(slot_place_at(centre_of(slot)), Some(slot), "slot {slot} misses itself");
        }
        assert_eq!(slot_place_at((5.0, 5.0)), None, "empty space hit a slot");
    }

    #[test]
    fn the_hotbar_is_the_bottom_row() {
        // Moving a stack to the bottom row has to be the same gesture as
        // putting it on the hotbar, or the player has to learn that the
        // two are different things.
        let lowest_storage = (HOTBAR_SLOTS..SLOTS)
            .map(|s| slot_rect(s).y0)
            .fold(f32::MAX, f32::min);
        for slot in 0..HOTBAR_SLOTS {
            assert!(
                slot_rect(slot).y1 <= lowest_storage + 1e-4,
                "hotbar slot {slot} is not below the storage rows"
            );
        }
    }

    /// How many recipes there are, for the layout tests.
    const RECIPES_LEN: usize = primitive_shared::crafting::RECIPES.len();

    #[test]
    fn the_recipe_list_scrolls_instead_of_growing_the_screen() {
        // The fourteenth recipe is what pushed this screen off the
        // bottom of a square window. The list has to be able to grow
        // without the panel growing with it, or the menu eventually
        // cannot be drawn at all.
        let mut screen = InventoryScreen::new();
        assert_eq!(screen.recipe_scroll(), 0);
        screen.scroll_recipes(-5);
        assert_eq!(screen.recipe_scroll(), 0, "scrolled above the first row");

        let last = RECIPES_LEN.saturating_sub(visible_recipes());
        screen.scroll_recipes(1000);
        assert_eq!(screen.recipe_scroll(), last, "scrolled past the last row");

        // ...and the row that is first on screen is drawn where the
        // first row goes, whatever its index.
        assert_eq!(
            recipe_rect(last, last).y1,
            recipe_rect(0, 0).y1,
            "the top of the list moved when it scrolled"
        );
    }

    #[test]
    fn the_whole_screen_fits_a_square_window() {
        // Authored as if the window were square, which is the worst case
        // the aspect divide can produce: x runs -aspect..+aspect, and
        // aspect is 1.0 there.
        //
        // Regression: the recipe column used to hang off the right of a
        // centred grid, putting the panel's right edge at x = 1.47. On
        // anything narrower than about 3:2 the crafting menu was off the
        // screen entirely, and on 16:9 the whole thing sat visibly to
        // one side.
        let panel = panel_rect();
        assert!(
            panel.x0 > -1.0 && panel.x1 < 1.0,
            "the panel runs from {} to {}, off a square window",
            panel.x0,
            panel.x1
        );
        assert!(panel.y0 > -1.0 && panel.y1 < 1.0);

        for slot in 0..SLOTS {
            let r = slot_rect(slot);
            assert!(r.x0 >= panel.x0 && r.x1 <= panel.x1, "slot {slot} escapes the panel");
            assert!(r.y0 >= panel.y0 && r.y1 <= panel.y1, "slot {slot} escapes the panel");
        }
        // Every row, at every scroll position it can be seen at: the
        // list is longer than the window on it, and a row is only ever
        // drawn inside that window.
        for scroll in 0..=primitive_shared::crafting::RECIPES.len() {
            for index in scroll..(scroll + visible_recipes()).min(RECIPES_LEN) {
                let r = recipe_rect(index, scroll);
                assert!(r.x0 >= panel.x0 && r.x1 <= panel.x1, "recipe {index} escapes the panel");
                assert!(r.y0 >= panel.y0 && r.y1 <= panel.y1, "recipe {index} escapes the panel");
            }
        }
    }

    #[test]
    fn the_footer_text_fits_inside_the_panel() {
        // The line this replaced was wider than the panel it sat in, so
        // it ran off the edge on every window shape.
        let panel = panel_rect();
        let room = panel.x1 - panel.x0 - 0.06;
        // The readout is the widest thing the footer can be asked to
        // draw, so it is checked at its longest -- in the wordiest
        // language, which is what actually decides the width.
        let summary = "9999 килограммов при себе   скорость 100%";
        assert!(widgets::measure(summary, 0.9) <= room, "the readout runs out of the panel");

        // ...and it has to fit *downwards* too. Every time the footer
        // loses a line the height comes down with it, and the first
        // number tried was too small and put the readout through the
        // bottom edge of the panel -- which no width check could catch.
        let under_belt = slot_rect(0).y0 - 0.052;
        let lowest = under_belt - widgets::cell_height(0.9);
        assert!(
            lowest > panel.y0,
            "the readout reaches {lowest}, below the panel floor at {}",
            panel.y0
        );
    }

    /// An empty body square says what it is without a word.
    ///
    /// **The riddle this replaces**: the four were labelled `H C L F`,
    /// one translated letter apiece, and `C` is a chest piece or a cape
    /// or a cap depending on who is reading. A pointer could have been
    /// rested on the square for a note and a phone has no pointer to
    /// rest -- which is where this screen is hardest to read.
    ///
    /// The property is not "there is a picture": it is that the picture
    /// in the head square is a thing that goes on a head. A ghost drawn
    /// from the wrong garment would be a *confident* lie, which is worse
    /// than the letter was.
    #[test]
    fn an_empty_body_square_shows_the_thing_that_goes_in_it() {
        use primitive_shared::equipment::{slot_of, Slot, SLOTS as BODY_SLOTS};
        for part in 0..BODY_SLOTS {
            let ghost = ghost_garment(part).expect("every body square has a ghost");
            assert_eq!(
                slot_of(ghost),
                Slot::from_index(part),
                "the square for part {part} shows a garment that goes somewhere else",
            );
        }
        assert_eq!(ghost_garment(BODY_SLOTS), None, "a fifth body part grew a ghost");
    }

    /// ...and the word for it, where there is a pointer to ask with.
    ///
    /// Four names, four different names, in each of the four languages.
    /// Two squares answering the same word is the `H C L F` problem with
    /// more letters in it.
    #[test]
    fn every_body_square_has_its_own_name_in_every_language() {
        use primitive_shared::equipment::SLOTS as BODY_SLOTS;
        for &language in Language::ALL {
            let names: Vec<&str> = (0..BODY_SLOTS)
                .map(|part| part_name(part, language).expect("a name for every part"))
                .collect();
            for (n, name) in names.iter().enumerate() {
                assert!(!name.is_empty(), "{language:?}: part {n} has no name");
                assert!(
                    !names[n + 1..].contains(name),
                    "{language:?}: two body squares are both called {name}",
                );
            }
            // ...and none of them is the heading over the whole column,
            // which is the collision that turned up first: `ТЕЛО` was
            // both the word for the column and the word for the chest
            // square in it.
            let heading = language.text(Msg::Worn);
            assert!(
                !names.contains(&heading),
                "{language:?}: a body square is called {heading}, which is the \
                 heading over all four of them",
            );
        }
    }

    /// Each of the three columns is marked off from its neighbour.
    ///
    /// The gaps could not do it -- there is no width left to widen them
    /// with, see `the_whole_screen_fits_a_square_window` -- so the seam
    /// with 0.030 to spend gets a tray edge and the seam with 0.012 gets
    /// a line scored between them. A tray that reached into the column
    /// beside it, or a line drawn across a slot, would say the opposite
    /// of what both are for.
    #[test]
    fn the_columns_are_told_apart_by_a_tray_edge_or_a_score_line() {
        let panel = panel_rect();
        let trays = [("pack", pack_tray()), ("crafting", recipe_tray())];
        for (name, tray) in trays {
            assert!(
                tray.x0 >= panel.x0 && tray.x1 <= panel.x1 && tray.y0 >= panel.y0,
                "the {name} tray hangs off the panel",
            );
        }
        assert!(
            pack_tray().x1 <= recipe_tray().x0,
            "the pack's tray and the crafting tray share a column of the screen",
        );
        // ...and each holds everything it is the tray for.
        for slot in 0..SLOTS {
            let cell = slot_rect(slot);
            let tray = pack_tray();
            assert!(
                cell.x0 >= tray.x0 && cell.x1 <= tray.x1 && cell.y0 >= tray.y0,
                "slot {slot} hangs out of the pack's tray",
            );
        }
        for part in 0..primitive_shared::equipment::SLOTS {
            let cell = equipment_rect(part);
            let tray = pack_tray();
            assert!(
                cell.x0 >= tray.x0 && cell.x1 <= tray.x1 && cell.y0 >= tray.y0,
                "body square {part} hangs out of the tray it shares with the pack",
            );
        }
        // ...the figure too, and clear of the squares beside it.
        let figure = mannequin_rect();
        assert!(
            figure.x0 >= pack_tray().x0 && figure.y0 >= pack_tray().y0 && figure.y1 <= pack_tray().y1,
            "the figure hangs out of the tray it shares with the pack",
        );
        assert!(figure.x1 < equipment_rect(0).x0, "the figure runs into the body squares");
        // ...and the score line lies in the gap between the two of them,
        // touching neither. It is drawn *over* the tray, so a line one
        // thousandth too wide would be a line drawn across a slot.
        let line = body_divider();
        assert!(line.width() > 0.0, "the body divider has no width to be seen in");
        for part in 0..primitive_shared::equipment::SLOTS {
            assert!(equipment_rect(part).x1 <= line.x0, "the divider covers a body square");
        }
        for slot in 0..SLOTS {
            assert!(slot_rect(slot).x0 >= line.x1, "the divider covers a pack slot");
        }
        for place in 0..visible_recipes().min(RECIPES_LEN) {
            let cell = recipe_rect(place, 0);
            let tray = recipe_tray();
            assert!(
                cell.x0 >= tray.x0 && cell.x1 <= tray.x1 && cell.y0 >= tray.y0,
                "recipe cell {place} hangs out of the crafting tray",
            );
        }
    }

    /// The pack's own button can be hit with a thumb.
    ///
    /// The same measurement the chest screen carries, on the same phone:
    /// TIDY PILE was 52 device pixels of a 91-pixel finger. There were two
    /// buttons here while the `X` stood beside it; the way out is a tap off
    /// the panel now, measured by `a_phone_always_has_somewhere_off_the_pack_to_tap`.
    #[test]
    fn the_pack_screens_buttons_are_at_least_a_finger_under_a_thumb() {
        widgets::as_a_phone(|| {
            let phone = widgets::Layout::for_screen(2712.0 / 1220.0, 1.65);
            let drawn = grow_by(phone);
            const SLACK: f32 = 1e-4;
            let rect = sort_button_rect();
            for (way, side) in [("across", rect.width()), ("down", rect.height())] {
                assert!(
                    side >= widgets::FINGER_SIDE - SLACK,
                    "tidy pile is {side:.3} {way} as authored, under a finger of {:.3}",
                    widgets::FINGER_SIDE,
                );
                assert!(
                    side * drawn >= widgets::FINGER_SIDE - SLACK,
                    "tidy pile is {:.0} device pixels {way} on the phone this was \
                     measured on, against a finger of 91",
                    side * drawn * 610.0,
                );
            }
            // ...and the band it is drawn in still holds it.
            let panel = panel_rect();
            assert!(
                rect.y1 <= panel.y1 && rect.y0 >= content_top() + tab_band(),
                "the tidy button has grown out of its band",
            );

            // **And the three tabs**, which are the control a phone
            // player uses on every visit and sit at the very top of the
            // panel, where a thumb is least accurate. Across is not
            // checked: a third of the panel is far wider than a finger
            // in every language.
            for index in 0..ALL_TABS.len() {
                let tab = tab_rect(index);
                assert!(
                    tab.height() >= widgets::FINGER_SIDE - SLACK
                        && tab.height() * drawn >= widgets::FINGER_SIDE - SLACK,
                    "tab {index} is {:.0} device pixels down, against a finger of 91",
                    tab.height() * drawn * 610.0,
                );
                assert!(
                    tab.y1 <= panel.y1 && tab.y0 >= content_top(),
                    "tab {index} has grown out of its band",
                );
            }
        });
    }

    #[test]
    fn the_screen_is_centred() {
        let panel = panel_rect();
        assert!(
            (panel.x0 + panel.x1).abs() < 1e-4,
            "the panel runs {} to {}, which is not centred",
            panel.x0,
            panel.x1
        );
    }

    #[test]
    fn slots_and_recipes_never_share_a_pixel() {
        // They are hit-tested in sequence, so an overlap would make one
        // of them unreachable.
        for slot in 0..SLOTS {
            let a = slot_rect(slot);
            for index in 0..visible_recipes().min(RECIPES_LEN) {
                let b = recipe_rect(index, 0);
                let overlaps =
                    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0;
                assert!(!overlaps, "slot {slot} overlaps recipe {index}");
            }
        }
    }

    /// A left click at the centre of a slot, which is most of what the
    /// tests below do.
    fn click_slot(screen: &mut InventoryScreen, inventory: &Inventory, slot: usize) -> Option<Intent> {
        screen.set_cursor(Some(centre_of(slot)));
        screen.click(inventory, Button::Left, false)
    }

    #[test]
    fn clicking_a_slot_then_another_asks_for_a_move() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        let (a, b) = (0usize, 15usize);
        assert!(inventory.block_in(a).is_some() && inventory.block_in(b).is_none());

        assert_eq!(
            click_slot(&mut screen, &inventory, a),
            None,
            "picking up is not yet a request"
        );
        assert_eq!(screen.held(), Some(a));

        assert_eq!(
            click_slot(&mut screen, &inventory, b),
            Some(Intent::Move { from: a, to: b })
        );
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn a_right_click_while_holding_asks_for_half_and_keeps_holding() {
        // Dealing a stack out across several slots is one gesture
        // repeated, not a pick-up per slot.
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        click_slot(&mut screen, &inventory, 0);

        screen.set_cursor(Some(centre_of(15)));
        assert_eq!(
            screen.click(&inventory, Button::Right, false),
            Some(Intent::Split { from: 0, to: 15 })
        );
        assert_eq!(screen.held(), Some(0), "the split let go of the stack");
    }

    #[test]
    fn a_right_click_with_empty_hands_asks_for_nothing() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        screen.set_cursor(Some(centre_of(0)));
        assert_eq!(screen.click(&inventory, Button::Right, false), None);
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn shift_clicking_sends_a_stack_the_other_way() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        screen.set_cursor(Some(centre_of(0)));
        assert_eq!(
            screen.click(&inventory, Button::Left, true),
            Some(Intent::QuickMove(0))
        );
        assert_eq!(screen.held(), None, "a quick move started a pick-up as well");
    }

    #[test]
    fn a_snapshot_that_changes_the_held_slot_cancels_the_gesture() {
        // The bug: the pick-up is only an index, so if the server puts
        // something else in that slot -- an item walked over, a craft
        // finishing -- finishing the move would send whatever is there
        // now. That is how a player throws away something untouched.
        let mut screen = InventoryScreen::new();
        let mut inventory = stocked();
        screen.open = true;
        click_slot(&mut screen, &inventory, 0);
        assert_eq!(screen.held(), Some(0));

        inventory.take_from(0, MAX_STACK);
        inventory.add(primitive_shared::types::BLOCK_LOG, 1);
        screen.sync(&inventory);
        assert_eq!(screen.held(), None, "the pick-up survived its stack changing");
    }

    #[test]
    fn a_snapshot_that_leaves_the_held_slot_alone_keeps_the_gesture() {
        let mut screen = InventoryScreen::new();
        let mut inventory = stocked();
        screen.open = true;
        click_slot(&mut screen, &inventory, 0);

        // More of the same block: still the stack they took hold of.
        inventory.add(BLOCK_STONE, 1);
        screen.sync(&inventory);
        assert_eq!(screen.held(), Some(0));
    }

    #[test]
    fn the_screen_never_edits_the_inventory_itself() {
        // The server owns it. A stack moved locally would be undone by
        // the next snapshot, visibly, as it jumped back.
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        let before: Vec<_> = (0..SLOTS).map(|s| inventory.block_in(s)).collect();

        click_slot(&mut screen, &inventory, 0);
        click_slot(&mut screen, &inventory, 15);

        let after: Vec<_> = (0..SLOTS).map(|s| inventory.block_in(s)).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn picking_up_an_empty_slot_does_nothing() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        assert_eq!(click_slot(&mut screen, &inventory, SLOTS - 1), None);
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn clicking_the_same_slot_twice_cancels() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        click_slot(&mut screen, &inventory, 0);
        assert_eq!(
            click_slot(&mut screen, &inventory, 0),
            None,
            "asked for a move onto itself"
        );
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn clicking_outside_the_grid_cancels_rather_than_asking_for_anything() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;

        click_slot(&mut screen, &inventory, 0);
        screen.set_cursor(Some((5.0, 5.0)));
        assert_eq!(screen.click(&inventory, Button::Left, false), None);
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn opening_seeds_the_cursor_so_the_first_click_lands() {
        // The bug this fixes: with no starting position, no CursorMoved
        // has arrived yet and the first click after opening does
        // nothing, which reads as the inventory ignoring the mouse.
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open_at(Some(centre_of(0)));
        assert!(screen.open);
        screen.click(&inventory, Button::Left, false);
        assert_eq!(screen.held(), Some(0), "the first click after opening was dead");
    }

    #[test]
    fn a_click_with_no_cursor_at_all_is_ignored() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open_at(None);
        assert_eq!(screen.click(&inventory, Button::Left, false), None);
    }

    #[test]
    fn clicking_a_recipe_asks_for_it() {
        let mut screen = InventoryScreen::new();
        let inventory = with_something_to_make();
        screen.open = true;
        // The cell that is *drawn* first, and whatever recipe is
        // standing in it -- the two are no longer the same number, and
        // the click has to ask for the recipe rather than the position.
        let (index, row) = first_offered(&inventory);
        screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        assert_eq!(
            screen.click(&inventory, Button::Left, false),
            Some(Intent::Craft { index, times: 1 })
        );
        assert_eq!(
            screen.click(&inventory, Button::Right, false),
            Some(Intent::Craft {
                index,
                times: CRAFT_MANY
            }),
            "a right click on a recipe should make as many as it can"
        );
    }

    #[test]
    fn the_tidy_button_is_reachable_and_only_tidies() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        let button = sort_button_rect();
        screen.set_cursor(Some((button.centre_x(), button.centre_y())));
        assert_eq!(screen.click(&inventory, Button::Left, false), Some(Intent::Sort));

        // It is hit-tested before the grid and the recipes, so it must
        // not be sitting on either of them.
        for slot in 0..SLOTS {
            assert_eq!(slot_place_at((button.centre_x(), button.centre_y())), None);
            let r = slot_rect(slot);
            let overlaps =
                r.x0 < button.x1 && r.x1 > button.x0 && r.y0 < button.y1 && r.y1 > button.y0;
            assert!(!overlaps, "the tidy button covers slot {slot}");
        }
        for index in 0..visible_recipes().min(RECIPES_LEN) {
            let r = recipe_rect(index, 0);
            let overlaps =
                r.x0 < button.x1 && r.x1 > button.x0 && r.y0 < button.y1 && r.y1 > button.y0;
            assert!(!overlaps, "the tidy button covers recipe {index}");
        }
        let panel = panel_rect();
        assert!(button.x1 <= panel.x1 && button.y1 <= panel.y1, "the button escapes the panel");
    }

    #[test]
    fn closing_mid_move_forgets_the_pick_up() {
        let mut screen = InventoryScreen::new();
        let inventory = stocked();
        screen.open = true;
        click_slot(&mut screen, &inventory, 0);
        screen.close();
        assert_eq!(screen.held(), None);
    }

    #[test]
    fn a_closed_screen_draws_nothing() {
        let screen = InventoryScreen::new();
        let inventory = stocked();
        assert!(screen
            .build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &inventory,
                1.0,
                Language::English,
            )
            .is_empty());
    }

    /// Draws the screen to a PNG, for looking at.
    ///
    /// Not a check -- it asserts nothing. It exists because the only
    /// other way to see this screen is to build the client, start a
    /// world and press I, and a layout is the kind of thing that has to
    /// be *looked* at: text over text, a column off its grid and a
    /// number in the wrong corner all pass every assertion in this file.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib -- --ignored dump
    /// ```
    ///
    /// Writes `inventory_screen.png` into `target/` (or `$PRIMITIVE_UI_DUMP`).
    /// Block icons have no textures here, so they come out as flat
    /// squares; everything else -- panels, borders, glyphs -- is exactly
    /// what the game draws.
    ///
    /// `PRIMITIVE_UI_TAB=health|pack|rucksack` picks the page. The
    /// rucksack page comes with a rucksack on, because there is no other
    /// state it exists in.
    #[test]
    #[ignore = "diagnostic: writes a picture of the screen"]
    fn dump_the_screen_to_a_png() {
        const WIDTH: u32 = 1600;
        const HEIGHT: u32 = 900;

        let mut screen = InventoryScreen::new();
        screen.open = true;
        let page = std::env::var("PRIMITIVE_UI_TAB").unwrap_or_default();
        screen.set_tab(match page.as_str() {
            "health" => Tab::Health,
            "rucksack" => Tab::Backpack,
            _ => Tab::Pack,
        });
        let mut inventory = stocked();
        inventory.add(primitive_shared::types::BLOCK_LOG, 12);
        inventory.add(primitive_shared::types::BLOCK_COBBLESTONE, MAX_STACK);
        inventory.add(primitive_shared::types::BLOCK_LEAVES, 30);
        inventory.add(primitive_shared::types::BLOCK_SAND, 7);
        // Two states, because the overlays are the parts most likely to
        // land somewhere silly: the tooltip under the pointer, and the
        // stack riding it once something has been picked up.
        let carrying = std::env::var("PRIMITIVE_UI_HELD").is_ok();
        let over_recipe = std::env::var("PRIMITIVE_UI_RECIPE").is_ok();
        screen.set_cursor(Some(centre_of(3)));
        if over_recipe {
            let row = recipe_rect(4, 0); // turf: two different ingredients
            screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        }
        if carrying {
            screen.click(&inventory, Button::Left, false);
            let over = slot_rect(HOTBAR_SLOTS + 4);
            screen.set_cursor(Some((over.centre_x(), over.centre_y())));
        }
        let mut worn = primitive_shared::inventory::Equipment::new();
        if screen.tab() == Tab::Backpack {
            inventory.open_backpack();
            worn.wear(primitive_shared::inventory::Stack::new(
                primitive_shared::types::BLOCK_RUCKSACK,
                1,
            ));
            inventory.put_in_slot(
                primitive_shared::inventory::BACKPACK_RANGE.start,
                primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_FLINT, 9),
            );
        }
        // Something wrong with the body, so the health page's right-hand
        // column has something in it -- a page of "no wounds" cannot show
        // whether a list of them fits.
        let mut hurt = Injuries::default();
        hurt.inflict(Part::LeftArm, primitive_shared::injury::Kind::Cut, 0.7);
        hurt.inflict(Part::RightLeg, primitive_shared::injury::Kind::Fracture, 0.5);
        let vitals = Vitals {
            health: 0.62,
            nourishment: 0.41,
            stamina: 0.72,
            body: crate::ui::hud::BodyGauges {
                hydration: 0.28,
                fatigue: 0.66,
                wetness: 0.5,
                grime: 0.7,
                recovery: 0.74,
                diet_groups: 2,
                ..crate::ui::hud::BodyGauges::default()
            },
        };
        let mut vertices = Vec::new();
        screen.build_into(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &inventory,
            &worn,
            &hurt,
            &vitals,
            Language::English,
            &mut vertices,
        );

        let path = std::env::var("PRIMITIVE_UI_DUMP")
            .unwrap_or_else(|_| "target/inventory_screen.png".to_string());
        widgets::dump_to_png(&vertices, WIDTH, HEIGHT, &path);
        println!("wrote {path}");
    }

    // ---- the three tabs ----

    /// A pack with a rucksack on its back and its ten extra squares
    /// open, which is the only state the third tab exists in.
    fn with_a_rucksack() -> (Inventory, primitive_shared::inventory::Equipment) {
        let mut pack = stocked();
        pack.open_backpack();
        let mut worn = primitive_shared::inventory::Equipment::new();
        assert!(
            worn.wear(primitive_shared::inventory::Stack::new(
                primitive_shared::types::BLOCK_RUCKSACK,
                1
            ))
            .is_none(),
            "a rucksack would not go on"
        );
        (pack, worn)
    }

    #[test]
    fn every_tab_is_clicked_where_it_is_drawn_and_on_nothing_else() {
        // The interface is laid out in its own space and then scaled, so
        // a hit test written independently of the drawing is a strip
        // that looks right and answers in the wrong place. `tab_rect` is
        // what draws and `tab_at` is what answers, and this is the line
        // that says they are the same rectangles.
        let (pack, _) = with_a_rucksack();
        for (index, tab) in ALL_TABS.iter().copied().enumerate() {
            let rect = tab_rect(index);
            let centre = (rect.centre_x(), rect.centre_y());
            assert_eq!(tab_at(centre), Some(index), "{tab:?} misses itself");
            // ...and the four corners just inside it, which is where a
            // thumb lands when it is aiming at the tab beside this one.
            for (x, y) in [
                (rect.x0 + 0.002, rect.y0 + 0.002),
                (rect.x1 - 0.002, rect.y1 - 0.002),
            ] {
                assert_eq!(tab_at((x, y)), Some(index), "{tab:?} misses its own corner");
            }

            let mut screen = InventoryScreen::new();
            screen.open = true;
            screen.set_cursor(Some(centre));
            assert_eq!(screen.click(&pack, Button::Left, false), None, "a tab asked the server for something");
            assert_eq!(screen.tab(), tab, "clicking {tab:?} turned to another page");
        }
        // Between two tabs, and well away from the strip, is nothing.
        let gap = (tab_rect(0).x1 + TAB_GAP / 2.0, tab_rect(0).centre_y());
        assert_eq!(tab_at(gap), None, "the gap between two tabs is a tab");
        assert_eq!(tab_at((0.0, -0.9)), None, "the bottom of the screen is a tab");
        // ...and no tab is drawn over a slot, a recipe or the tidy
        // button, which are the three things under it.
        for index in 0..ALL_TABS.len() {
            let rect = tab_rect(index);
            let centre = (rect.centre_x(), rect.centre_y());
            assert_eq!(slot_place_at(centre), None, "a tab sits on a slot");
            assert_eq!(recipe_at(centre, 0, &pack), None, "a tab sits on a recipe");
            assert!(!sort_button_rect().contains(centre.0, centre.1), "a tab sits on the tidy button");
        }
    }

    #[test]
    fn the_rucksack_page_cannot_be_opened_without_a_rucksack() {
        // The squares do not exist when nothing is worn on the back (see
        // `Inventory::open_backpack`), so a page that showed them would
        // be a page of ten holes that swallow clicks.
        let bare = stocked();
        let rucksack = tab_rect(2);
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.set_cursor(Some((rucksack.centre_x(), rucksack.centre_y())));
        assert_eq!(screen.click(&bare, Button::Left, false), None);
        assert_eq!(screen.tab(), Tab::Pack, "the rucksack page opened with no rucksack");

        let (worn, _) = with_a_rucksack();
        assert_eq!(screen.click(&worn, Button::Left, false), None);
        assert_eq!(screen.tab(), Tab::Backpack, "a worn rucksack still would not open its page");

        // ...and taking it off while the page is open puts the screen
        // back on the pack rather than leaving it on ten squares that
        // have stopped existing.
        screen.sync(&bare);
        assert_eq!(screen.tab(), Tab::Pack, "the page outlived the rucksack");
    }

    #[test]
    fn the_rucksacks_squares_are_clicked_where_the_storage_row_is_drawn() {
        // The two slot pages are one grid with one row swapped
        // (`slot_in_place`). The belt has to mean the belt on both --
        // it is the row a player uses during play -- and the row above
        // it has to mean the rucksack on the page that says so.
        let (pack, _) = with_a_rucksack();
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.set_tab(Tab::Backpack);
        for place in 0..HOTBAR_SLOTS {
            assert_eq!(slot_in_place(Tab::Backpack, place), Some(place), "the belt moved");
        }
        for place in HOTBAR_SLOTS..HOTBAR_SLOTS + primitive_shared::inventory::BACKPACK_SLOTS {
            let slot = slot_in_place(Tab::Backpack, place).expect("a place on the rucksack page with no square");
            assert!(
                primitive_shared::inventory::BACKPACK_RANGE.contains(&slot),
                "grid place {place} is not a rucksack square on the rucksack page"
            );
            // ...and it is reached by clicking exactly where it is drawn.
            let rect = slot_rect(place);
            screen.set_cursor(Some((rect.centre_x(), rect.centre_y())));
            assert_eq!(screen.hovered_slot(), Some(slot), "place {place} hit-tests to the wrong slot");
        }
        // Every rucksack square is reachable and no two share a place.
        let reached: std::collections::HashSet<usize> =
            (HOTBAR_SLOTS..SLOTS).filter_map(|p| slot_in_place(Tab::Backpack, p)).collect();
        assert_eq!(reached.len(), primitive_shared::inventory::BACKPACK_SLOTS);
        // ...and on the pack page the same places are the player's own.
        for place in 0..SLOTS {
            assert_eq!(slot_in_place(Tab::Pack, place), Some(place));
        }
        let _ = pack;
    }

    #[test]
    fn the_health_page_shows_every_vital_the_server_sends() {
        // **Every one, and the test is what makes that true.** The page
        // is a table (`health_page`) and a table is easy to add a row to
        // and easy to forget one from; so each reading is nudged on its
        // own and the page has to come out different. A reading the
        // table does not name draws the same pixels twice and fails
        // here.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.set_tab(Tab::Health);
        let pack = stocked();
        let worn = primitive_shared::inventory::Equipment::new();
        // Compared as bytes, because a vertex is a plain-old-data
        // struct for the GPU and has no `PartialEq` -- and adding one
        // for a test would be a trait on a hot type for the sake of an
        // assertion.
        let draw = |vitals: &Vitals| {
            let mut out = Vec::new();
            screen.build_into(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &pack,
                &worn,
                &Injuries::default(),
                vitals,
                Language::English,
                &mut out,
            );
            bytemuck::cast_slice::<_, u8>(&out).to_vec()
        };
        let calm = Vitals::default();
        let base = draw(&calm);
        assert!(!base.is_empty(), "the health page drew nothing at all");

        let changed: [(&str, Vitals); 9] = [
            ("health", Vitals { health: 0.31, ..calm }),
            ("food", Vitals { nourishment: 0.31, ..calm }),
            ("breath", Vitals { stamina: 0.31, ..calm }),
            ("water", Vitals { body: crate::ui::hud::BodyGauges { hydration: 0.31, ..calm.body }, ..calm }),
            ("tiredness", Vitals { body: crate::ui::hud::BodyGauges { fatigue: 0.31, ..calm.body }, ..calm }),
            (
                "warmth",
                Vitals {
                    body: crate::ui::hud::BodyGauges {
                        temperature_c: primitive_shared::body::NEUTRAL_C - 9.0,
                        ..calm.body
                    },
                    ..calm
                },
            ),
            ("wetness", Vitals { body: crate::ui::hud::BodyGauges { wetness: 0.31, ..calm.body }, ..calm }),
            ("dirt", Vitals { body: crate::ui::hud::BodyGauges { grime: 0.31, ..calm.body }, ..calm }),
            ("diet", Vitals { body: crate::ui::hud::BodyGauges { diet_groups: 3, ..calm.body }, ..calm }),
        ];
        for (name, vitals) in changed {
            assert_ne!(draw(&vitals), base, "'{name}' is not on the health page");
        }
        // Comfort is the tenth and it is shown as what it *does* -- the
        // multiplier on how fast breath comes back -- never as the
        // hidden score itself. See the note on `health_page`.
        let uncomfortable = Vitals { body: crate::ui::hud::BodyGauges { recovery: 0.64, ..calm.body }, ..calm };
        assert_ne!(draw(&uncomfortable), base, "comfort's effect is not on the health page");

        // ...and so are the wounds, which arrive in their own message.
        let mut hurt = Injuries::default();
        hurt.inflict(Part::LeftArm, primitive_shared::injury::Kind::Cut, 0.8);
        let mut with_wounds = Vec::new();
        screen.build_into(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &pack,
            &worn,
            &hurt,
            &calm,
            Language::English,
            &mut with_wounds,
        );
        assert_ne!(
            bytemuck::cast_slice::<_, u8>(&with_wounds).to_vec(),
            base,
            "a cut arm is not on the health page"
        );
    }

    #[test]
    fn the_health_page_has_no_slots_and_no_recipes_under_it() {
        // It is a different screen behind the same key, and the pages
        // under it are not drawn -- so a click that fell through to them
        // would move a stack the player cannot see.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.set_tab(Tab::Health);
        let pack = with_something_to_make();
        for probe in [centre_of(0), centre_of(HOTBAR_SLOTS)] {
            screen.set_cursor(Some(probe));
            assert_eq!(screen.hovered_slot(), None, "the health page hovers a slot");
            assert_eq!(screen.click(&pack, Button::Left, false), None, "a click on the health page asked for something");
            assert_eq!(screen.held(), None, "a click on the health page picked something up");
        }
    }

    #[test]
    fn every_tab_and_every_vital_has_a_word_in_every_language() {
        // Four languages, and a missing line is an English word in the
        // middle of a Russian page -- see `lang::Language::text`, which
        // falls back rather than failing.
        for msg in [
            Msg::TabHealth,
            Msg::TabPack,
            Msg::TabBackpack,
            Msg::SlotBack,
            Msg::VitalHealth,
            Msg::VitalHunger,
            Msg::VitalThirst,
            Msg::VitalStamina,
            Msg::VitalTiredness,
            Msg::VitalWarmth,
            Msg::VitalWetness,
            Msg::VitalDirt,
            Msg::VitalDiet,
            Msg::VitalRecovery,
            Msg::VitalInjuries,
            Msg::NoBackpack,
        ] {
            let mut seen: Vec<&str> = Vec::new();
            for language in Language::ALL {
                let text = language.text(msg);
                assert!(!text.trim().is_empty(), "{msg:?} is blank in {language:?}");
                seen.push(text);
            }
            // Russian and Polish are different alphabets and different
            // languages; a line that is the same in both is a line
            // somebody forgot to translate.
            assert_ne!(
                language_of(&seen, Language::Russian),
                language_of(&seen, Language::Polish),
                "{msg:?} is the same in Russian and Polish"
            );
        }
    }

    fn language_of<'a>(seen: &[&'a str], language: Language) -> &'a str {
        let index = Language::ALL.iter().position(|&l| l == language).expect("a language");
        seen[index]
    }

    #[test]
    fn nothing_the_screen_draws_escapes_its_panel() {
        // The broad version of the fits-a-square-window test, and the
        // one that catches content rather than layout: a recipe name
        // wider than its row, a status string that outgrows its corner,
        // an icon row that runs past the column. All of those look like
        // text lying on the world outside the panel.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        screen.set_cursor(Some(centre_of(0)));
        let mut inventory = stocked();
        // Enough of everything that every recipe row is in its longest
        // state -- counts, "x N", and the widest names.
        for block in [BLOCK_STONE, BLOCK_DIRT, primitive_shared::types::BLOCK_LOG] {
            inventory.add(block, MAX_STACK);
        }

        for held in [false, true] {
            if held {
                screen.click(&inventory, Button::Left, false);
            }
            let vertices = screen.build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &inventory,
                1.0,
                Language::English,
            );
            let panel = panel_rect();
            // Borders are drawn *outside* the rectangle they frame, and
            // the panel's shadow is deliberately further out still --
            // that offset is the whole of what makes it read as a shadow
            // rather than as a second border. Neither is content, and
            // this test is about content not escaping.
            let slack = 0.02;
            for v in &vertices {
                let [x, y] = v.position;
                // The scrim, which is meant to cover the whole screen.
                if x.abs() > 4.0 {
                    continue;
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
    }

    #[test]
    fn the_stack_in_hand_follows_the_cursor() {
        // Click-then-click needs something on the pointer, or nothing on
        // screen says a stack is in hand except a border on a slot the
        // player has looked away from.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        let inventory = stocked();
        let build = |screen: &InventoryScreen| {
            screen.build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &inventory,
                1.0,
                Language::English,
            )
        };
        // Measured with the pointer over nothing in particular, so the
        // difference is the stack in hand rather than a tooltip. The
        // footer is the one part of the panel that answers neither
        // `slot_at` nor `recipe_at` -- pointing at a recipe now raises a
        // tooltip of its own, which is what this used to measure by
        // mistake.
        let panel = panel_rect();
        let off_the_grid = (panel.x0 + 0.02, panel.y0 + 0.02);
        assert!(slot_place_at(off_the_grid).is_none() && recipe_at(off_the_grid, 0, &inventory).is_none());
        screen.set_cursor(Some(off_the_grid));
        let empty_handed = build(&screen);

        screen.set_cursor(Some(centre_of(0)));
        screen.click(&inventory, Button::Left, false);
        assert_eq!(screen.held(), Some(0));
        screen.set_cursor(Some(off_the_grid));
        let carrying = build(&screen);
        assert!(
            carrying.len() > empty_handed.len(),
            "the held stack drew nothing on the cursor"
        );
    }

    #[test]
    fn crafting_is_the_same_grid_of_the_same_cells_as_the_pack() {
        // The whole of the redesign, as measurements: a recipe is a
        // cell, the cells are the pack's cells, and the two grids line
        // up with each other. A crafting column drawn to its own
        // proportions is what made the screen read as two screens.
        let pack = slot_rect(SLOTS - 1);
        let first = recipe_rect(0, 0);
        assert!(
            ((first.x1 - first.x0) - (pack.x1 - pack.x0)).abs() < 1e-5,
            "recipe cells are a different size from the pack's"
        );
        assert!(((first.y1 - first.y0) - (pack.y1 - pack.y0)).abs() < 1e-5);
        // **Level at the bottom, where it used to be level at the top.**
        // The pack hangs from the floor of the band now (`slot_rect`),
        // so the row that lines up with the recipe column is the belt
        // against the *last* recipe row rather than the pile against the
        // first. The property is the same one and it is still the point:
        // two grids of one cell size whose rows are half a cell out of
        // step is what made this read as two screens.
        let last_row = recipe_rect((RECIPE_ROWS - 1) * RECIPE_COLUMNS, 0);
        assert!((last_row.y0 - slot_rect(0).y0).abs() < 1e-5, "the grids do not line up");
        // ...and the pile above the belt is off that rhythm by exactly
        // `HOTBAR_SPLIT` and nothing else -- the gap that makes the belt
        // read as the belt.
        // The storage row *nearest* the belt, whichever slots it holds:
        // with three rows above the belt, the first storage slot is in the
        // row furthest from it.
        let nearest = (HOTBAR_SLOTS..SLOTS).map(|slot| slot_rect(slot).y0).fold(f32::MAX, f32::min);
        let split = nearest - slot_rect(0).y1;
        assert!((split - (GAP + HOTBAR_SPLIT)).abs() < 1e-5, "the belt's gap is not the belt's gap");

        // Filled across and then down, with the same gap the pack uses.
        let second = recipe_rect(1, 0);
        assert!((second.x0 - first.x1 - GAP).abs() < 1e-5, "wrong gap between recipes");
        assert!((second.y1 - first.y1).abs() < 1e-5, "the second recipe is not beside the first");
        let next_row = recipe_rect(RECIPE_COLUMNS, 0);
        assert!((next_row.x0 - first.x0).abs() < 1e-5, "the row did not wrap to the left");
        assert!(next_row.y1 < first.y0, "the second row overlaps the first");

        // The whole *table* no longer has to fit -- the grid shows only
        // what can be made, and that list is short. What still has to
        // hold is that a plausible pack fits without scrolling, because
        // a player who can make six things should see six things.
        let inventory = with_something_to_make();
        assert!(
            offered(&inventory).len() <= visible_recipes(),
            "{} offered recipes do not fit in {} cells",
            offered(&inventory).len(),
            visible_recipes()
        );
    }

    #[test]
    fn only_what_can_be_made_is_offered() {
        use primitive_shared::crafting::{feasibility, Feasibility, RECIPES};
        use primitive_shared::types::BLOCK_FLINT;

        // An empty pack can make nothing, and shows nothing. The grid
        // used to show all of it, veiled -- a wall of grey squares that
        // says "not yet" twenty-nine times.
        let empty = Inventory::new();
        assert!(offered(&empty).is_empty(), "an empty pack was offered recipes");

        // A nodule of flint knaps into flakes with nothing else at all,
        // so exactly the recipes it can reach appear.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT, 4);
        let offered = offered(&pack);
        assert!(!offered.is_empty(), "flint offered nothing to do with it");
        for &index in &offered {
            assert!(
                !matches!(
                    feasibility(&pack, &RECIPES[index], KILN),
                    Feasibility::MissingIngredients
                ),
                "{} was offered without its ingredients",
                RECIPES[index].name
            );
        }
        // ...and nothing that needs something absent slipped in.
        for (index, recipe) in RECIPES.iter().enumerate() {
            if matches!(feasibility(&pack, recipe, KILN), Feasibility::MissingIngredients) {
                assert!(!offered.contains(&index), "{} should be hidden", recipe.name);
            }
        }
        // Table order survives filtering: two recipes never swap places.
        assert!(offered.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_click_asks_for_the_recipe_in_the_cell_not_the_cell() {
        // The bug this guards: position and recipe id used to be the
        // same number. With the grid filtered they are not, and a click
        // that sent the position would craft whatever happened to sit at
        // that index in the table -- the wrong thing, silently.
        let mut screen = InventoryScreen::new();
        let inventory = with_something_to_make();
        screen.open = true;
        let offered = offered(&inventory);
        assert!(offered.len() > 1, "need at least two offers to tell them apart");
        // Second cell on screen.
        let cell = recipe_rect(1, 0);
        screen.set_cursor(Some((cell.centre_x(), cell.centre_y())));
        assert_eq!(
            screen.click(&inventory, Button::Left, false),
            Some(Intent::Craft { index: offered[1], times: 1 }),
            "the click asked for the cell's position rather than its recipe"
        );
    }

    #[test]
    fn pointing_at_a_recipe_says_what_it_costs() {
        // The cost used to be drawn into every row, all the time. Now it
        // is a tooltip, so it has to actually appear -- otherwise the
        // recipe is a picture of a result with no way to find out what
        // it takes.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        let inventory = with_something_to_make();
        let build = |screen: &InventoryScreen| {
            screen.build(
                FontAtlas::for_test(),
                &FaceLayers::empty_for_test(),
                &inventory,
                1.0,
                Language::Russian,
            )
        };
        let panel = panel_rect();
        screen.set_cursor(Some((panel.x0 + 0.02, panel.y0 + 0.02)));
        let idle = build(&screen).len();

        let (_, cell) = first_offered(&inventory);
        screen.set_cursor(Some((cell.centre_x(), cell.centre_y())));
        assert!(build(&screen).len() > idle, "pointing at a recipe said nothing");
    }

    #[test]
    fn hovering_a_recipe_points_at_the_slots_it_would_spend() {
        // Without it the row says "4 cobblestone" and the player is left
        // to find the cobblestone themselves, in forty slots of icons.
        let mut screen = InventoryScreen::new();
        screen.open = true;
        let mut inventory = Inventory::new();
        inventory.add(primitive_shared::types::BLOCK_LOG, 4);

        let lit = |screen: &InventoryScreen| {
            screen
                .build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &inventory,
                    1.0,
                    Language::English,
                )
                .iter()
                // The wash inside the well, which is what marks an
                // ingredient now that a slot is a bevelled hole rather
                // than a framed square.
                .filter(|v| v.tint == HIGHLIGHT_INGREDIENT)
                .count()
        };

        screen.set_cursor(Some(centre_of(SLOTS - 1)));
        assert_eq!(lit(&screen), 0, "a slot was lit with no recipe hovered");

        // Recipe 0 turns a log into planks, and the log is in slot 0.
        let row = recipe_rect(0, 0);
        screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        assert!(lit(&screen) > 0, "the ingredient slot was not pointed out");
    }

    /// **Nothing is drawn outside the panel.**
    ///
    /// The class of bug this catches is the one that keeps coming back:
    /// a line of text placed by eye at a fixed offset from something
    /// that later moved, whose descenders then hang over the panel's
    /// bottom bevel or off its side. A vertex outside the frame is that
    /// bug, whatever drew it.
    ///
    /// The scrim is excluded by construction: it is drawn far outside
    /// the visible range on purpose, and it is the first quad.
    #[test]
    fn nothing_is_drawn_outside_the_panel() {
        let mut screen = InventoryScreen::new();
        screen.open = true;
        let mut inventory = Inventory::new();
        inventory.add(primitive_shared::types::BLOCK_COBBLESTONE, 128);
        inventory.add(primitive_shared::types::BLOCK_LOG, 9);
        screen.sync(&inventory);

        let panel = panel_rect();
        let vertices = screen.build(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &inventory,
            0.7,
            Language::English,
        );
        escapes_nothing(&vertices, panel);

        // ...and the other two pages, in every language, with a body in
        // a bad enough state that the health page is carrying its
        // longest lines. **This is the test the health page was written
        // against**: its tenth row fell through the panel's floor and
        // its wound lines ran off the right of the screen, and neither
        // was visible from the page that was being checked.
        let mut hurt = Injuries::default();
        for part in [Part::LeftArm, Part::RightLeg, Part::Torso, Part::Head] {
            hurt.inflict(part, primitive_shared::injury::Kind::Cut, 0.9);
            hurt.inflict(part, primitive_shared::injury::Kind::Fracture, 0.9);
        }
        let low = Vitals {
            health: 0.03,
            nourishment: 0.03,
            stamina: 0.03,
            body: crate::ui::hud::BodyGauges {
                hydration: 0.03,
                fatigue: 0.97,
                wetness: 1.0,
                grime: 1.0,
                recovery: 0.51,
                diet_groups: 4,
                temperature_c: primitive_shared::body::NEUTRAL_C - 12.0,
                ..crate::ui::hud::BodyGauges::default()
            },
        };
        let (packed, worn) = with_a_rucksack();
        for &language in Language::ALL {
            for tab in ALL_TABS {
                screen.set_tab(tab);
                let mut out = Vec::new();
                screen.build_into(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    &packed,
                    &worn,
                    &hurt,
                    &low,
                    language,
                    &mut out,
                );
                escapes_nothing(&out, panel);
            }
        }
    }

    /// Every vertex is inside the panel, bar the scrim behind it and the
    /// shadow under it.
    ///
    /// Its own function because three pages in four languages is twelve
    /// calls, and a copy of the assertion per call is eleven places for
    /// the slack to be written down differently.
    fn escapes_nothing(vertices: &[HotbarVertex], panel: Rect) {
        // The panel's shadow is the one thing drawn outside it on
        // purpose -- down and to the right by exactly this much.
        let slack = widgets::SHADOW_OFFSET + 0.001;
        for vertex in vertices.iter().skip(6) {
            let (x, y) = (vertex.position[0], vertex.position[1]);
            assert!(
                x >= panel.x0 - 0.001
                    && x <= panel.x1 + slack
                    && y >= panel.y0 - slack
                    && y <= panel.y1 + 0.001,
                "something is drawn at ({x:.3}, {y:.3}), outside the panel                  ({:.3}..{:.3}, {:.3}..{:.3})",
                panel.x0,
                panel.x1,
                panel.y0,
                panel.y1
            );
        }
    }

    #[test]
    fn an_open_screen_draws_every_slot() {
        let mut screen = InventoryScreen::new();
        screen.open = true;
        let inventory = stocked();
        let vertices = screen.build(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &inventory,
            1.0,
            Language::English,
        );
        assert!(
            vertices.len() > SLOTS * 6,
            "only {} vertices for {SLOTS} slots",
            vertices.len()
        );
    }

    #[test]
    fn a_jug_with_something_in_it_is_drawn_differently_from_an_empty_one() {
        use primitive_shared::inventory::{filled_jug, Stack};
        use primitive_shared::types::{BLOCK_GRAIN, BLOCK_JUG};
        // **The count on a slot that stacks to one is suppressed**, and
        // a jug now stacks to one -- so the number that says how much
        // grain is in it goes down the one path that was written to
        // print nothing here. Two packs, drawn: if they come out the
        // same length, a full jug looks exactly like an empty one and
        // the player has no way at all to tell which is which.
        let draw = |pack: &Inventory| {
            let mut screen = InventoryScreen::new();
            screen.open = true;
            screen
                .build(
                    FontAtlas::for_test(),
                    &FaceLayers::empty_for_test(),
                    pack,
                    1.0,
                    Language::English,
                )
                .len()
        };

        let mut empty = Inventory::new();
        empty.put_in_slot(0, Stack::new(BLOCK_JUG, 1));
        let mut full = Inventory::new();
        full.put_in_slot(0, filled_jug(BLOCK_GRAIN, 12));

        assert!(
            draw(&full) > draw(&empty),
            "a jug of grain drew the same as an empty jug"
        );
    }
}


#[cfg(test)]
mod touch_layout_tests {
    use super::*;
    use crate::ui::widgets::Layout;

    /// The phone this was cut for, held sideways.
    const PHONE: f32 = 2712.0 / 1220.0;

    #[test]
    fn the_interface_size_setting_makes_the_pack_bigger_across_its_range() {
        // **The complaint this answers**, in the player's words: the
        // interface setting "affects only the HUD". It did -- every
        // centred screen was capped by one number that worked out to
        // 1.05 whatever the window was, so the hotbar moved and nothing
        // else did.
        for aspect in [16.0f32 / 9.0, PHONE] {
            let mut previous = 0.0;
            for requested in [1.0f32, 1.5, 2.0] {
                let scale = grow_by(Layout::for_screen(aspect, requested));
                assert!(
                    scale > previous + 0.1,
                    "at {aspect:.2}, asking for {requested} drew the pack at {scale},                      barely past {previous}",
                );
                previous = scale;
            }
        }
    }

    #[test]
    fn a_slot_at_the_size_a_phone_starts_at_is_near_enough_a_finger() {
        // Near enough, and *not* forced: the size a phone starts at
        // draws a slot within a few percent of a finger, and buying
        // those few percent by flooring the pack cost every setting
        // below 1.5 -- they all drew the same pack. See `grow_by`.
        let layout = Layout::for_screen(PHONE, 1.5);
        if !layout.is_touch() {
            return;
        }
        let drawn = CELL * grow_by(layout);
        assert!(
            drawn >= layout.finger() * 0.9,
            "a slot comes out {drawn} against a finger of {}",
            layout.finger(),
        );
    }

    #[test]
    fn no_two_neighbouring_settings_draw_the_same_pack() {
        // The failure a touch floor introduced and this is here to stop
        // coming back: the setting moved between 0.5 and 1.5 and the
        // pack sat still, because the floor was above all of them.
        let layout = |r| Layout::for_screen(PHONE, r);
        for pair in [(1.0f32, 1.25f32), (1.25, 1.5), (1.5, 1.75), (1.75, 2.0)] {
            let (small, large) = (grow_by(layout(pair.0)), grow_by(layout(pair.1)));
            assert!(
                large > small * 1.05,
                "{} and {} both draw the pack at about {small}",
                pair.0,
                pair.1,
            );
        }
    }

    /// **With no `X`, a phone's way out is the glass beside the pack**, so
    /// there has to be some at every interface size a player can choose.
    ///
    /// Measured in the window's own units after the pack has grown, against
    /// a finger: somewhere left, right, above or below the panel is at least
    /// a finger deep, and a tap there really does close it. At the largest
    /// sizes the pack runs to the sides of a phone held sideways and the
    /// room is under it, which is why all four sides are asked.
    #[test]
    fn a_phone_always_has_somewhere_off_the_pack_to_tap() {
        widgets::as_a_phone(|| {
            for requested in [1.0f32, 1.25, 1.5, 1.65, 2.0, 3.0, 4.0] {
                let layout = Layout::for_screen(PHONE, requested);
                let scale = grow_by(layout);
                let panel = panel_rect();
                let rooms = [
                    (PHONE - panel.x1 * scale, (0.5 * (PHONE + panel.x1 * scale), 0.0)),
                    (1.0 - panel.y1 * scale, (0.0, 0.5 * (1.0 + panel.y1 * scale))),
                    (1.0 + panel.y0 * scale, (0.0, -0.5 * (1.0 - panel.y0 * scale))),
                ];
                let (room, middle) = rooms
                    .into_iter()
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .expect("three sides");
                assert!(
                    room >= widgets::FINGER_SIDE,
                    "at interface size {requested} the widest glass beside the pack is {room:.3}, \
                     under a finger of {:.3}",
                    widgets::FINGER_SIDE
                );
                // ...and a tap in the middle of it is read back as off the panel.
                let mut screen = InventoryScreen::new();
                screen.open = true;
                screen.set_cursor(Some(layout.hit(middle, scale)));
                assert_eq!(
                    screen.click(&Inventory::new(), Button::Left, false),
                    Some(Intent::Close),
                    "at interface size {requested} a tap beside the pack did not close it"
                );
            }
        });
    }

    /// **A bigger interface still drops a bandage where the arm is drawn.**
    ///
    /// The invariant `widgets::a_bigger_interface_is_still_clicked_where_it_is_drawn`
    /// states, walked through the figure: every part authored here is grown
    /// by the frame loop's own `scale_about`, read back through the frame
    /// loop's own `Layout::hit`, and must still be the part a bandage lands
    /// on. A figure whose parts answered half a slot away would look right
    /// and dress the wrong arm.
    #[test]
    fn a_bigger_interface_still_drops_a_bandage_where_the_arm_is_drawn() {
        use primitive_shared::types::BLOCK_BANDAGE;
        let mut pack = Inventory::new();
        pack.put_in_slot(0, primitive_shared::inventory::Stack::new(BLOCK_BANDAGE, 9));
        for aspect in [16.0f32 / 9.0, PHONE] {
            for requested in [1.0f32, 1.5, 2.0, 4.0] {
                let layout = Layout::for_screen(aspect, requested);
                let scale = grow_by(layout);
                for part in Part::ALL {
                    let r = crate::ui::mannequin::part_rect(mannequin_rect(), part);
                    // Where the frame loop puts a point authored at the
                    // middle of the part...
                    let mut drawn = [HotbarVertex {
                        position: [r.centre_x(), r.centre_y()],
                        uv: [0.0, 0.0],
                        tex_layer: 0,
                        tint: [1.0; 4],
                    }];
                    widgets::scale_about(&mut drawn, widgets::anchor::CENTRE(aspect), scale);
                    let [x, y] = drawn[0].position;
                    // ...and what a click there is read back as.
                    let mut screen = InventoryScreen::new();
                    screen.open = true;
                    screen.sync(&pack);
                    let slot = slot_rect(0);
                    let at = (slot.centre_x() * scale, slot.centre_y() * scale);
                    screen.set_cursor(Some(layout.hit(at, scale)));
                    let _ = screen.click(&pack, Button::Left, false);
                    screen.set_cursor(Some(layout.hit((x, y), scale)));
                    assert_eq!(
                        screen.click(&pack, Button::Left, false),
                        Some(Intent::Treat { slot: 0, part }),
                        "at {aspect:.2} and size {requested} a bandage aimed at {part:?} missed it"
                    );
                }
            }
        }
    }

    #[test]
    fn the_health_page_names_what_is_wrong_with_the_room_and_only_that() {
        use primitive_shared::shelter::Reading;
        let field = crate::ui::hud::BodyGauges {
            shelter: Reading { air_c: 12.0, ..Reading::default() },
            ..Default::default()
        };
        let said = shelter_lines(&field, Language::English);
        assert_eq!(said.len(), 1, "a field said more than its air: {said:?}");
        assert!(said[0].0.contains("12C"));

        let hut = crate::ui::hud::BodyGauges {
            shelter: Reading { air_c: 4.0, indoors: true, draught: 0.5, keeps_out: 0.55, roof_open: true },
            smoke: 0.6,
            ..Default::default()
        };
        for language in Language::ALL {
            let said: Vec<String> = shelter_lines(&hut, *language).into_iter().map(|(text, _)| text).collect();
            for msg in [
                Msg::ShelterSmoky,
                Msg::ShelterDraughty,
                Msg::ShelterHoleTakesHeat,
                Msg::ShelterWallsThin,
            ] {
                assert!(said.iter().any(|line| line == language.text(msg)), "{language:?} never said {msg:?}: {said:?}");
            }
        }
    }

    #[test]
    fn the_pack_never_grows_off_the_window() {
        for aspect in [16.0f32 / 9.0, PHONE, 1.0] {
            for requested in [1.0f32, 1.5, 2.0, 4.0] {
                let layout = Layout::for_screen(aspect, requested);
                let scale = grow_by(layout);
                let (half_w, half_h) = extent();
                // A pack already too big for the window at its authored
                // size is clipped either way; what must not happen is
                // growing one past the glass.
                if scale <= 1.0 {
                    continue;
                }
                assert!(
                    half_w * scale <= aspect + 1e-3,
                    "at {aspect:.2}x{requested} the pack is {} wide on a screen {}",
                    half_w * scale * 2.0,
                    aspect * 2.0,
                );
                assert!(
                    half_h * scale <= 1.0 + 1e-3,
                    "at {aspect:.2}x{requested} the pack hangs off the top and bottom",
                );
            }
        }
    }
}

