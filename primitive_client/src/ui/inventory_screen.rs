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
use crate::logic::inventory::{Inventory, HOTBAR_SLOTS, MAX_STACK, SLOTS};
use primitive_shared::inventory::STORAGE_ROWS;
use crate::engine::texture::{FaceLayers, FontAtlas, FACE_SOUTH, FACE_TOP};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

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
const COLUMN_GAP: f32 = 0.055;
/// Space between the storage rows and the hotbar row, so the bar reads
/// as the thing it is rather than as a fourth row.
pub(crate) const HOTBAR_SPLIT: f32 = 0.030;

pub(crate) const PANEL_PAD: f32 = 0.055;
/// Room above the first row of slots for the word "PACK".
const GRID_LABEL: f32 = 0.046;
/// Room at the top of the panel for the title, and at the bottom for the
/// readout and the three lines of controls. Named, because "why is there
/// a gap" is exactly the question a bare number in a `Rect::new` never
/// answers.
const HEADER_HEIGHT: f32 = 0.100;
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
const FOOTER_HEIGHT: f32 = 0.145;

/// An empty slot: a recess, darker than the panel behind it.
///
/// Two colours because it is drawn as a gradient -- shaded under the top
/// lip, lifting toward the near edge, which is what a hole in a surface
/// looks like. See `draw_slot`.
pub(crate) const SLOT_TOP: [f32; 4] = [0.025, 0.028, 0.038, 1.0];
pub(crate) const SLOT_BOTTOM: [f32; 4] = [0.052, 0.057, 0.073, 1.0];
/// An occupied one, lighter, so a full bar reads as full at a glance
/// rather than needing the icons to be counted.
pub(crate) const SLOT_LIT_TOP: [f32; 4] = [0.135, 0.145, 0.185, 1.0];
pub(crate) const SLOT_LIT_BOTTOM: [f32; 4] = [0.195, 0.205, 0.255, 1.0];
pub(crate) const CELL_EDGE: [f32; 4] = [0.28, 0.31, 0.38, 1.0];
pub(crate) const CELL_HOVER: [f32; 4] = [0.62, 0.68, 0.80, 1.0];
pub(crate) const CELL_SOURCE: [f32; 4] = [0.95, 0.80, 0.30, 1.0];
/// A slot holding something the hovered recipe wants.
const CELL_INGREDIENT: [f32; 4] = [0.45, 0.85, 0.50, 1.0];
/// The strip behind the hotbar row, marking it as the part that is on
/// screen during play.
pub(crate) const HOTBAR_STRIP: [f32; 4] = [0.11, 0.13, 0.17, 1.0];
/// A rule under the title and above the readout.
pub(crate) const RULE: [f32; 4] = [0.30, 0.34, 0.42, 0.9];
/// The plate a stack count sits on, so a white number over a pale block
/// texture is still readable.
const COUNT_PLATE: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
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
}

/// Which mouse button a click came from. Its own type rather than
/// winit's, so the screen and its tests do not depend on the windowing
/// library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// What a right click on a recipe asks for.
///
/// Not `u8::MAX`: the server runs the loop, and a bound that is merely
/// large is a bound a slow tick can feel. Sixty-four is more than a full
/// pack of any ingredient can make in one go.
pub const CRAFT_MANY: u8 = 64;

#[derive(Default)]
pub struct InventoryScreen {
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

    pub fn set_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.cursor = cursor;
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

        if sort_button_rect().contains(cursor.0, cursor.1) {
            self.release();
            return Some(Intent::Sort);
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

        let Some(slot) = slot_at(cursor) else {
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
            // A right click with empty hands is not a gesture. It could
            // be made to pick up half, but nothing is held in limbo here
            // -- there is nowhere for that half to be.
            (Button::Right, None) => None,
            (Button::Left, Some(from)) => {
                self.release();
                if from == slot {
                    None // put it back down
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
        self.cursor.and_then(slot_at)
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
        let mut out = Vec::new();
        self.build_into(font, layers, inventory, stamina_fraction, language, &mut out);
        out
    }

    /// The same screen, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(
        &self,
        font: FontAtlas,
        layers: &FaceLayers,
        inventory: &Inventory,
        stamina_fraction: f32,
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
        p.panel_header(panel, language.text(Msg::Inventory), HEADER_HEIGHT - 0.012);

        crafting_panel(
            &mut p,
            layers,
            inventory,
            self.cursor.and_then(|c| recipe_at(c, self.recipe_scroll, inventory)),
            self.recipe_scroll,
            language,
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
        let first = slot_rect(0);
        let last = slot_rect(HOTBAR_SLOTS - 1);
        let strip = Rect::new(
            first.x0 - 0.012,
            first.y0 - 0.012,
            last.x1 + 0.012,
            last.y1 + 0.012,
        );
        p.vgradient(strip, HOTBAR_STRIP, [0.07, 0.08, 0.11, 1.0]);
        p.border(strip, 0.0025, RULE);

        let hovered = self.cursor.and_then(slot_at);
        // Pointing at a recipe lights up the slots it would spend. The
        // two columns are otherwise unconnected: the row says "4
        // cobblestone" and the player still has to find the cobblestone.
        let wanted = self
            .cursor
            .and_then(|c| recipe_at(c, self.recipe_scroll, inventory))
            .and_then(primitive_shared::crafting::recipe);

        for slot in 0..SLOTS {
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
            draw_slot(
                &mut p,
                slot_rect(slot),
                layers,
                block,
                inventory.count_in(slot),
                edge,
            );
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
        let _ = stamina_fraction;
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
            widgets::TEXT_DIM
        };
        // Under the belt, in the space the grid used to be centred in.
        // It is a readout *about the pack*, so it belongs in the pack's
        // column rather than in a footer under both of them.
        //
        // No rule above it any more: a line drawn the whole width of the
        // panel to separate one row of text from the grid over it was
        // furniture around furniture.
        let under_belt = slot_rect(0).y0 - 0.052;
        p.text(&summary, grid_left(), under_belt, 0.9, colour);

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
                None => {
                    tooltip(&mut p, inventory, cursor, self.recipe_scroll, language)
                }
            }
        }

        *out = p.into_vertices();
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
pub(crate) fn draw_slot(
    p: &mut Painter,
    cell: Rect,
    layers: &FaceLayers,
    block: Option<primitive_shared::types::BlockId>,
    count: u32,
    edge: SlotEdge,
) {
    // A slot is a hole in the panel, and a hole has light in the bottom
    // of it: darkest along the top where the lip shades it, lifting
    // toward the near edge. Two colours instead of one, for the same six
    // vertices -- see `Painter::vgradient`.
    let (top, bottom) = if block.is_some() {
        (SLOT_LIT_TOP, SLOT_LIT_BOTTOM)
    } else {
        (SLOT_TOP, SLOT_BOTTOM)
    };
    p.vgradient(cell, top, bottom);

    // The lit halo goes *behind* the border, so what the eye follows is
    // one bright outline rather than a rectangle with a fringe.
    if matches!(edge, SlotEdge::Hovered | SlotEdge::Source) {
        let glow = if matches!(edge, SlotEdge::Source) {
            [CELL_SOURCE[0], CELL_SOURCE[1], CELL_SOURCE[2], 0.20]
        } else {
            [CELL_HOVER[0], CELL_HOVER[1], CELL_HOVER[2], 0.16]
        };
        p.border(cell, 0.012, glow);
    }

    let (colour, thickness) = match edge {
        SlotEdge::Source => (CELL_SOURCE, 0.005),
        SlotEdge::Hovered => (CELL_HOVER, 0.004),
        SlotEdge::Ingredient => (CELL_INGREDIENT, 0.004),
        SlotEdge::Plain => (CELL_EDGE, 0.002),
    };
    p.border(cell, thickness, colour);

    let Some(block) = block else {
        return;
    };
    icon(p, cell, icon_layer(layers, block));

    // The count sits on its own dark plate in the corner. Over a sand or
    // snow icon a plain white number is invisible, and a drop shadow
    // alone is not enough at this size.
    let label = count.to_string();
    let width = widgets::ink_width(&label, COUNT_SCALE);
    let plate = Rect::new(
        cell.x1 - width - 0.011,
        cell.y0 + 0.003,
        cell.x1 - 0.003,
        cell.y0 + widgets::cell_height(COUNT_SCALE) + 0.006,
    );
    p.quad(plate, COUNT_PLATE);
    p.text(
        &label,
        plate.x0 + 0.004,
        plate.y1 - 0.002,
        COUNT_SCALE,
        if count >= MAX_STACK { COUNT_FULL } else { COUNT_TEXT },
    );
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
        [1.0, 1.0, 1.0, 0.85],
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

const TOOLTIP_BG: [f32; 4] = [0.04, 0.05, 0.07, 0.96];
const TOOLTIP_SCALE: f32 = 0.72;

/// Names what the cursor is over.
///
/// Block textures at icon size are not self-explanatory -- cobblestone
/// and stone are the same grey square to anyone who has not learned them
/// -- and the weight is the number the whole load mechanic turns on, so
/// it belongs where the player is deciding what to carry.
fn tooltip(
    p: &mut Painter,
    inventory: &Inventory,
    cursor: (f32, f32),
    scroll: usize,
    language: Language,
) {
    let lines = match slot_at(cursor) {
        Some(slot) => {
            let Some(block) = inventory.block_in(slot) else {
                return;
            };
            let count = inventory.count_in(slot);
            vec![(
                format!(
                    "{} x{count}   {:.0} kg",
                    primitive_shared::types::block_name(block),
                    primitive_shared::types::block_weight(block) * count as f32,
                ),
                widgets::TEXT,
            )]
        }
        // A recipe cell shows what it makes; the tooltip is where what
        // it *costs* lives now. The cell used to carry the whole
        // sentence -- name, count, and the ingredients in pictures --
        // and paid for it in width on every recipe at once, including
        // the seventeen nobody is pointing at.
        None => match recipe_at(cursor, scroll, inventory).and_then(primitive_shared::crafting::recipe) {
            Some(r) => recipe_lines(inventory, r, language),
            None => return,
        },
    };

    let scale = TOOLTIP_SCALE;
    let width = lines
        .iter()
        .map(|(text, _)| widgets::ink_width(text, scale))
        .fold(0.0, f32::max)
        + 0.020;
    let line_height = widgets::cell_height(scale) + 0.006;
    let height = line_height * lines.len() as f32 + 0.012;
    // Up and to the right of the pointer, then pulled back inside the
    // panel: a tooltip that runs off the screen is worse than none.
    let panel = panel_rect();
    let x0 = (cursor.0 + 0.014).min(panel.x1 - width);
    let y0 = (cursor.1 + 0.012).min(panel.y1 - height);
    let rect = Rect::new(x0, y0, x0 + width, y0 + height);
    p.quad(rect, TOOLTIP_BG);
    p.border(rect, 0.002, RULE);
    for (n, (text, colour)) in lines.iter().enumerate() {
        p.text(
            text,
            rect.x0 + 0.010,
            rect.y1 - 0.008 - n as f32 * line_height,
            scale,
            *colour,
        );
    }
}

/// What a recipe says when you point at it: what it makes, what it
/// costs, and whether you can.
fn recipe_lines(
    inventory: &Inventory,
    r: &primitive_shared::crafting::Recipe,
    language: Language,
) -> Vec<(String, [f32; 4])> {
    use primitive_shared::crafting::{
        feasibility, missing_ingredient, possible_crafts, Feasibility,
    };
    use primitive_shared::types::block_name;

    let mut lines = vec![(r.name.to_string(), widgets::TEXT)];
    // The cost, spelled out rather than drawn: a line of text is read
    // once, and the icons it replaces were being drawn for every recipe
    // on screen whether or not anyone was looking at them.
    let cost = r
        .inputs
        .iter()
        .map(|&(block, amount)| {
            let have = inventory.count(block);
            format!("{amount}x {} ({have})", block_name(block))
        })
        .collect::<Vec<_>>()
        .join("   ");
    lines.push((cost, widgets::TEXT_DIM));

    lines.push(match feasibility(inventory, r) {
        Feasibility::Ready => (
            format!("x{}", possible_crafts(inventory, r)),
            widgets::TEXT_GOOD,
        ),
        Feasibility::NoRoom => (language.text(Msg::NoRoom).to_string(), widgets::TEXT_BAD),
        Feasibility::MissingIngredients => match missing_ingredient(inventory, r) {
            Some((block, short)) => (
                format!("{} {short}x {}", language.text(Msg::Need), block_name(block)),
                widgets::TEXT_DIM,
            ),
            None => (language.text(Msg::No).to_string(), widgets::TEXT_DIM),
        },
    });
    lines
}

// ---- sort button ----

const SORT_WIDTH: f32 = 0.215;
const SORT_HEIGHT: f32 = 0.052;

/// Where the tidy button sits: the right end of the header band, clear of
/// both titles and of everything below them.
pub fn sort_button_rect() -> Rect {
    let top = content_top() + HEADER_HEIGHT - 0.014;
    let right = recipe_left() + recipe_grid_width();
    Rect::new(right - SORT_WIDTH, top - SORT_HEIGHT, right, top)
}

fn sort_button(p: &mut Painter, cursor: Option<(f32, f32)>, language: Language) {
    let rect = sort_button_rect();
    let hovered = cursor.is_some_and(|(x, y)| rect.contains(x, y));
    p.quad(rect, if hovered { widgets::BUTTON_HOVER } else { widgets::BUTTON });
    p.border(rect, 0.0025, if hovered { RECIPE_EDGE_HOVER } else { widgets::BUTTON_EDGE });
    let scale = 0.72;
    let label = language.text(Msg::TidyPile);
    p.text(
        label,
        rect.centre_x() - widgets::ink_width(label, scale) / 2.0,
        rect.centre_y() + widgets::cell_height(scale) / 2.0 - 0.004,
        scale,
        widgets::TEXT,
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
pub(crate) fn icon(p: &mut Painter, cell: Rect, layer: u32) {
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
        [1.0, 1.0, 1.0, 1.0],
    );
}

/// One textured quad. The block icons everywhere on this screen -- slots,
/// recipe rows, the stack on the cursor -- are all this.
fn textured(p: &mut Painter, rect: Rect, layer: u32, tint: [f32; 4]) {
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
/// Seven, because the recipe table is twenty-five long and seven by four
/// shows all of it with no scrolling at all. It was five while the table
/// was eighteen; the ages of metal added eight recipes at a stroke, and
/// the choice was two more columns or a list the player has to scroll.
/// Scrolling loses the property this grid was built for -- everything
/// you can make, visible at once, in a stable place -- and two columns
/// of cells is cheaper than that.
const RECIPE_COLUMNS: usize = 7;

const RECIPE_EDGE: [f32; 4] = [0.30, 0.34, 0.42, 1.0];
const RECIPE_EDGE_HOVER: [f32; 4] = [0.95, 0.80, 0.30, 1.0];

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

/// How many recipes are on screen at once.
///
/// Whatever fits beside the slot grid, so crafting is never what decides
/// how tall the screen is. That is the whole point: the pack has a fixed
/// size and the recipe table does not.
pub fn visible_recipes() -> usize {
    let (_, grid_height) = grid_size();
    let rows = (((grid_height + GAP) / (CELL + GAP)).floor() as usize).max(1);
    rows * RECIPE_COLUMNS
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
    use primitive_shared::crafting::{feasibility, Feasibility, RECIPES};
    RECIPES
        .iter()
        .enumerate()
        .filter(|(_, recipe)| {
            !matches!(feasibility(inventory, recipe), Feasibility::MissingIngredients)
        })
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
    -(grid_width + COLUMN_GAP + recipe_grid_width()) / 2.0
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

/// The whole screen's extent, panel padding included.
fn panel_rect() -> Rect {
    Rect::new(
        grid_left() - PANEL_PAD,
        -content_top() - FOOTER_HEIGHT,
        recipe_left() + recipe_grid_width() + PANEL_PAD,
        content_top() + HEADER_HEIGHT,
    )
}

/// Which recipe a point is over, as an index into `RECIPES`.
///
/// Takes the pack because the grid holds only what can be made from it:
/// the cell under the cursor is a *position*, and `offered` is what says
/// which recipe is standing in it.
pub fn recipe_at(cursor: (f32, f32), scroll: usize, inventory: &Inventory) -> Option<usize> {
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
) {
    use primitive_shared::crafting::{feasibility, RECIPES};

    // What can be made from what is in the pack, and nothing else. See
    // `offered`.
    let offered = offered(inventory);
    let scroll = visible_scroll(scroll, offered.len());

    // Level with the word over the pack, because it is the same kind of
    // word over the same kind of grid.
    let first = recipe_rect(scroll, scroll);
    let label_scale = 0.62;
    p.text(
        language.text(Msg::Crafting),
        first.x0,
        first.y1 + 0.030,
        label_scale,
        widgets::TEXT_DIM,
    );
    // How far down a longer table this is, when there is one.
    if recipes_overflow(offered.len()) {
        let counted = format!("{}-{} {} {}",
            scroll + 1,
            (scroll + visible_recipes()).min(offered.len()),
            language.text(Msg::Of),
            offered.len());
        let width = widgets::ink_width(&counted, label_scale);
        p.text(
            &counted,
            recipe_left() + recipe_grid_width() - width,
            first.y1 + 0.030,
            label_scale,
            widgets::TEXT_DIM,
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
        p.quad(track, SLOT_TOP);
        let span = top - bottom;
        let fraction = visible_recipes() as f32 / offered.len() as f32;
        let travel = span * (1.0 - fraction);
        let offset = travel * scroll as f32
            / offered.len().saturating_sub(visible_recipes()).max(1) as f32;
        let thumb = Rect::new(track.x0, top - offset - span * fraction, track.x1, top - offset);
        p.quad(thumb, RECIPE_EDGE);
    }

    for (place, &index) in offered.iter().enumerate().take(last).skip(scroll) {
        let r = &RECIPES[index];
        let rect = recipe_rect(place, scroll);
        let ready = feasibility(inventory, r).is_ready();

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

/// Top of the band both columns live in.
///
/// Whichever column is taller sets it, so neither can run out of the
/// panel: the recipe list grows with the recipe table, and the grid
/// grows with the slot count.
fn content_top() -> f32 {
    let (_, grid_height) = grid_size();
    grid_height.max(recipes_height()) / 2.0
}

/// Where a slot sits on screen.
///
/// Slots 0..HOTBAR_SLOTS are the hotbar and are drawn as the *bottom*
/// row, matching where the real hotbar is. Storage runs above it, in
/// reading order. The grid is centred in the content band rather than
/// hung from its top, so it sits opposite the middle of the recipe list
/// instead of leaving a hole under itself.
pub fn slot_rect(slot: usize) -> Rect {
    let left = grid_left();
    // Aligned with the top of the recipe column rather than centred in
    // the band it shares with it. The recipe list is the taller of the
    // two, so centring the grid inside it left a hand's width of empty
    // panel above the slots *and* below them, and a screen with a hole
    // in the middle of it reads as unfinished. Top-aligned, the empty
    // space is all in one place -- under the belt -- which is where the
    // readout goes.
    //
    // Less the height of the "PACK" label, which sits above the first
    // row: without the gap it is drawn into the title bar.
    let top = content_top() - GRID_LABEL;

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

/// Which slot a point is over, if any.
pub fn slot_at(cursor: (f32, f32)) -> Option<usize> {
    (0..SLOTS).find(|&slot| slot_rect(slot).contains(cursor.0, cursor.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_STONE};

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
                    slot_at(centre_of(a)),
                    slot_at(centre_of(b)),
                    "slots {a} and {b} hit-test the same"
                );
            }
        }
    }

    #[test]
    fn hit_testing_agrees_with_where_things_are_drawn() {
        for slot in 0..SLOTS {
            assert_eq!(slot_at(centre_of(slot)), Some(slot), "slot {slot} misses itself");
        }
        assert_eq!(slot_at((5.0, 5.0)), None, "empty space hit a slot");
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
            assert_eq!(slot_at((button.centre_x(), button.centre_y())), None);
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
    /// cargo test -p primitive_client --bin primitive_client -- --ignored dump
    /// ```
    ///
    /// Writes `inventory_screen.png` into `target/` (or `$PRIMITIVE_UI_DUMP`).
    /// Block icons have no textures here, so they come out as flat
    /// squares; everything else -- panels, borders, glyphs -- is exactly
    /// what the game draws.
    #[test]
    #[ignore = "diagnostic: writes a picture of the screen"]
    fn dump_the_screen_to_a_png() {
        const WIDTH: u32 = 1600;
        const HEIGHT: u32 = 900;

        let mut screen = InventoryScreen::new();
        screen.open = true;
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
        let vertices = screen.build(
            FontAtlas::for_test(),
            &FaceLayers::empty_for_test(),
            &inventory,
            0.72,
            Language::English,
        );

        let path = std::env::var("PRIMITIVE_UI_DUMP")
            .unwrap_or_else(|_| "target/inventory_screen.png".to_string());
        widgets::dump_to_png(&vertices, WIDTH, HEIGHT, &path);
        println!("wrote {path}");
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
        assert!(slot_at(off_the_grid).is_none() && recipe_at(off_the_grid, 0, &inventory).is_none());
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
        // Level with the top row of the pile, not floating between rows.
        assert!((first.y1 - slot_rect(HOTBAR_SLOTS).y1).abs() < 1e-5, "the grids do not line up");

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
                    feasibility(&pack, &RECIPES[index]),
                    Feasibility::MissingIngredients
                ),
                "{} was offered without its ingredients",
                RECIPES[index].name
            );
        }
        // ...and nothing that needs something absent slipped in.
        for (index, recipe) in RECIPES.iter().enumerate() {
            if matches!(feasibility(&pack, recipe), Feasibility::MissingIngredients) {
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
                .filter(|v| v.tint == CELL_INGREDIENT)
                .count()
        };

        screen.set_cursor(Some(centre_of(SLOTS - 1)));
        assert_eq!(lit(&screen), 0, "a slot was lit with no recipe hovered");

        // Recipe 0 turns a log into planks, and the log is in slot 0.
        let row = recipe_rect(0, 0);
        screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        assert!(lit(&screen) > 0, "the ingredient slot was not pointed out");
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
}
