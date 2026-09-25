//! The hotbar: one slot per placeable block, showing that block's actual
//! texture.
//!
//! Geometry only -- this module builds screen-space quads and the
//! renderer draws them with `hotbar.wgsl`, which samples the same block
//! texture array the terrain uses. That's deliberate: an icon atlas
//! maintained separately from the block textures is a thing that drifts
//! out of date the first time someone re-skins a block.
//!
//! Coordinates are in NDC, authored as if the viewport were square; the
//! vertex shader divides x by the aspect ratio. Y is up (NDC), so the bar
//! sits at negative y.

use bytemuck::{Pod, Zeroable};

use primitive_shared::types::BlockId;

use crate::engine::texture::{TextureManager, FACE_SOUTH, FACE_TOP};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct HotbarVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    /// `UNTEXTURED` draws flat `tint` instead of sampling -- used for
    /// the slot frames and the selection box.
    pub tex_layer: u32,
    pub tint: [f32; 4],
}

/// Must match `UNTEXTURED` in hotbar.wgsl.
pub const UNTEXTURED: u32 = u32::MAX;

/// The bar holds every placeable block. Keys 1-9 reach the first nine
/// and 0 reaches the tenth, which is where the numeric row runs out --
/// past that the wheel is the only way to select, so the list is capped
/// here rather than growing a second row.
pub const MAX_SLOTS: usize = 10;
/// Sixteen quads per slot at the very most: the recess, the icon and four
/// frame edges, and then the pack's marks (`inventory_screen::slot_marks`)
/// -- two for a quality square, four for a chipped edge, two for a wear
/// bar. It was eight while the bar drew no marks at all.
///
/// **And two hundred and fifty-six over, for the backdrop**, which is no
/// longer one quad: it is the skin's panel, sliced into a frame and a
/// field that tiles along the bar (about two dozen quads at the size the
/// bar is drawn -- see `widgets::FIELD_TILE`). This is only the capacity
/// the buffer starts at and the renderer grows it when it has to, but a
/// hint that is wrong every frame is a reallocation every frame.
pub const MAX_HOTBAR_VERTICES: usize = MAX_SLOTS * 16 * 6 + 256;

/// Slot geometry. Public because the HUD draws stack counts and the
/// health row relative to the bar, and two modules laying the same bar
/// out from two sets of numbers is how they end up half a slot apart.
pub const SLOT: f32 = 0.080;
const GAP: f32 = 0.012;
pub const BOTTOM: f32 = -0.94;
/// How far the backdrop reaches past the slots on every side.
///
/// **Public because it is the bar's real edge.** The slots stop at
/// `BOTTOM + SLOT`; the thing a player sees stops a bit further out, and
/// anything laid out against the smaller number is laid out against a
/// line that is not on the screen. That is exactly how the hunger strip
/// ended up drawn on top of the bar -- see `hud::BAR_Y`.
pub const PAD: f32 = 0.014;
/// The top of the hotbar as it is actually drawn, backdrop included.
pub const TOP: f32 = BOTTOM + SLOT + PAD;

/// The left edge of the hotbar as it is actually drawn, backdrop
/// included.
///
/// **Public for the same reason [`PAD`] is, and it was missing for the
/// same reason it was.** The HUD hangs its gauges over this bar, and it
/// used to place them at a chosen number -- `-0.60` -- against a bar
/// whose real edge is here. The two disagreed by thirteen hundredths of
/// the screen, which is a sixth of a slot short of two slots: the health
/// gauge stuck out past the left end of the bar and the whole cluster
/// read as belonging to something else.
///
/// Derived from the same three constants the bar is drawn from, so it
/// cannot drift from it again.
pub const LEFT: f32 = -(SLOT * MAX_SLOTS as f32 + GAP * (MAX_SLOTS as f32 - 1.0)) / 2.0 - PAD;
/// ...and the right edge, which is the same distance the other way.
pub const RIGHT: f32 = -LEFT;

/// The bar has to be on screen, and the pack has to be at least as long
/// as the bar. Both are relations between constants, so the build is
/// where they should fail rather than the test suite.
const _: () = assert!(BOTTOM > -1.0 && BOTTOM + SLOT < 1.0);
const _: () = assert!(crate::logic::inventory::SLOTS >= MAX_SLOTS);
const BACKDROP: [f32; 4] = [0.05, 0.06, 0.09, 0.72];
/// The recess each slot sits in.
///
/// The bar used to be frames drawn straight over the world, so an empty
/// slot was a rectangle of whatever happened to be behind it -- against
/// a bright sky the frames vanished, and against a dark cave the icons
/// did. A cell of its own under each one costs a quad and makes the bar
/// legible over anything.
const CELL_TOP: [f32; 4] = [0.03, 0.035, 0.05, 0.82];
const CELL_BOTTOM: [f32; 4] = [0.09, 0.10, 0.13, 0.82];
/// ...and a little brighter under the slot in hand, so the selection
/// reads even where the frame is against something pale.
const CELL_SELECTED_TOP: [f32; 4] = [0.14, 0.13, 0.07, 0.88];
const CELL_SELECTED_BOTTOM: [f32; 4] = [0.24, 0.22, 0.11, 0.88];
const FRAME: [f32; 4] = [0.75, 0.78, 0.83, 0.9];
const FRAME_SELECTED: [f32; 4] = [1.0, 0.95, 0.55, 1.0];

/// The ring the skin draws round the square in hand.
///
/// The pack's own `HIGHLIGHT_SOURCE_RING`, at full alpha because the
/// picture carries its own: one mark for one thing, and "this is the one
/// I am holding" is the same thing on the belt as it is in the pack.
const SELECTED_RING: [f32; 4] = [1.0, 0.85, 0.35, 1.0];
const ICON_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const ICON_TINT_DIM: [f32; 4] = [0.72, 0.72, 0.72, 1.0];

fn push_quad(
    out: &mut Vec<HotbarVertex>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    tex_layer: u32,
    tint: [f32; 4],
) {
    // v = 0 is the top of the image, so the top edge of the quad (y1 in
    // NDC, where y is up) takes v = 0. Getting this backwards flips every
    // icon upside down -- the same trap as the block face UVs.
    let corners = [
        ([x0, y0], [0.0, 1.0]),
        ([x1, y0], [1.0, 1.0]),
        ([x1, y1], [1.0, 0.0]),
        ([x0, y0], [0.0, 1.0]),
        ([x1, y1], [1.0, 0.0]),
        ([x0, y1], [0.0, 0.0]),
    ];
    for (position, uv) in corners {
        out.push(HotbarVertex {
            position,
            uv,
            tex_layer,
            tint,
        });
    }
}

/// Horizontal centre of slot `index` of `count`.
/// The same quad, shaded from one colour at the top to another at the
/// bottom. The vertex carries its own tint and the hardware
/// interpolates it, so this costs exactly what a flat quad costs.
fn push_gradient(
    out: &mut Vec<HotbarVertex>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    top: [f32; 4],
    bottom: [f32; 4],
) {
    for (position, tint) in [
        ([x0, y0], bottom),
        ([x1, y0], bottom),
        ([x1, y1], top),
        ([x0, y0], bottom),
        ([x1, y1], top),
        ([x0, y1], top),
    ] {
        out.push(HotbarVertex {
            position,
            uv: [0.0, 0.0],
            tex_layer: UNTEXTURED,
            tint,
        });
    }
}

pub fn slot_centre(index: usize, count: usize) -> f32 {
    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    let left = -total / 2.0;
    left + pitch * index as f32 + SLOT / 2.0
}

/// Which slot a point in interface space is on, if any.
///
/// **The exact inverse of `slot_centre`**, which is the whole reason it
/// lives beside it rather than wherever it is called from. There is a
/// test that walks every slot's own middle back through this, because a
/// hit-test that has drifted from the drawing is a bar that looks right
/// and selects the wrong thing -- the kind of fault a player reports as
/// "it picks the one next to what I tapped".
///
/// Tested against the drawn cell and no larger. The bar is drawn low
/// and wide and there is nothing else down there to steal a tap from,
/// but a slot that reached past its neighbour would make the gap
/// between two of them belong to both, and then which one you get
/// depends on the order they happen to be checked in.
pub fn slot_at(x: f32, y: f32, count: usize) -> Option<usize> {
    if count == 0 || !(BOTTOM..=BOTTOM + SLOT).contains(&y) {
        return None;
    }
    (0..count).find(|index| {
        let centre = slot_centre(*index, count);
        (x - centre).abs() <= SLOT / 2.0
    })
}

/// Which texture to show for a block in the pack.
///
/// A block that has asked for a picture of its own gets it -- see
/// `ITEM_SLOT`. Otherwise a face, and the side face rather than the top:
/// for grass, the side shows the green strip over dirt, which is what
/// makes it recognisable at icon size. Blocks with no side texture fall
/// back to the top.
fn icon_layer(textures: &TextureManager, block: BlockId) -> u32 {
    if let Some(layer) = textures.layer_for_item(block) {
        return layer;
    }
    let side = textures.layer_for_face(block, FACE_SOUTH);
    if side == 0 {
        textures.layer_for_face(block, FACE_TOP)
    } else {
        side
    }
}

/// Builds the whole bar from what the player is actually carrying,
/// appending to a list the caller keeps between frames -- so a rebuild
/// reuses the allocation instead of making a fresh one.
///
/// The bar always shows its ten slots -- an empty one is a frame with
/// nothing in it, so the number keys keep pointing at the same places as
/// the inventory fills and empties. Only the *contents* come and go.
pub fn build_into(
    textures: &TextureManager,
    inventory: &crate::logic::inventory::Inventory,
    selected: usize,
    out: &mut Vec<HotbarVertex>,
) {
    use crate::ui::widgets::{Painter, Piece, Rect};

    let count = MAX_SLOTS;

    let pitch = SLOT + GAP;
    let total = pitch * count as f32 - GAP;
    // One painter for the whole bar, so the belt is drawn out of the
    // same pictures the pack is. **The bar and the pack are the same
    // ten squares**, and while the bar drew its own gradient recess and
    // the pack drew a bevelled well they were visibly two different
    // interfaces stacked on one screen -- which is what the player was
    // looking at when he asked for this.
    let mut p = Painter::onto(textures.font, std::mem::take(out));
    let backdrop = Rect::new(-total / 2.0 - PAD, BOTTOM - PAD, total / 2.0 + PAD, TOP);
    if !p.nine(backdrop, Piece::Panel, BACKDROP, Some(crate::ui::widgets::FIELD_TILE)) {
        push_quad(
            &mut p.vertices,
            backdrop.x0,
            backdrop.y0,
            backdrop.x1,
            backdrop.y1,
            UNTEXTURED,
            BACKDROP,
        );
    }

    for index in 0..count {
        let centre = slot_centre(index, count);
        let x0 = centre - SLOT / 2.0;
        let x1 = centre + SLOT / 2.0;
        let y0 = BOTTOM;
        let y1 = BOTTOM + SLOT;
        let is_selected = index == selected;

        let cell = Rect::new(x0, y0, x1, y1);
        // The recess, under everything else in the slot: the pack's own
        // cell picture, or the gradient it was before there was one.
        //
        // **The pack's own colour too, which it was not.** The belt kept
        // a near-black blue of its own (`CELL_TOP`/`CELL_BOTTOM`, the
        // gradient's two ends) and multiplied the skin's picture by it,
        // so the lip and the floor came out within a few bytes of each
        // other: on a phone the belt was ten flat dark squares with a
        // thin white outline round each, beside a pack made of cells
        // with depth in them. Photographed and reported as "the skin has
        // not been applied on the touch layer at all". `widgets::WELL`
        // is what the pack's forty squares are tinted with, and the
        // belt's ten are the same ten squares.
        //
        // **Through `Painter::cell_picture`, not through `stretched`**,
        // and that is not tidiness either: that call is what says "this
        // rectangle is a square of a grid", and the test that keeps text
        // off the lip of a slot reads exactly those. Drawn as a bare
        // picture, the belt's ten slots were invisible to it -- which is
        // how the counts on the belt came to be sitting on their own
        // edges while the same test watched the pack.
        let skinned = p.cell_picture(cell, crate::ui::widgets::WELL);
        if !skinned {
            let (top, bottom) = if is_selected {
                (CELL_SELECTED_TOP, CELL_SELECTED_BOTTOM)
            } else {
                (CELL_TOP, CELL_BOTTOM)
            };
            push_gradient(&mut p.vertices, x0, y0, x1, y1, top, bottom);
        }

        // Which one is in hand: the pack's own mark for a chosen square,
        // a ring of amber thread drawn *inside* the cell.
        //
        // **Not four quads around it any more.** The outline was drawn
        // round every slot, selected or not, in a pale blue-grey a
        // shade off white -- ten bright rectangles over the world, which
        // is the one thing on the HUD that had no equivalent anywhere
        // else in the interface. A ring inside the cell is what the pack
        // marks a square with, so the belt now says "this one" the same
        // way the pack does. The quads stay as the fallback where there
        // is no skin, because there the cell has no lip of its own and
        // an unframed one would not read as a square at all.
        if skinned {
            if is_selected {
                let _ = p.cell_mark(cell, Piece::SlotSelected, SELECTED_RING);
            }
        } else {
            let frame_colour = if is_selected { FRAME_SELECTED } else { FRAME };
            let t = if is_selected { 0.008 } else { 0.004 };
            push_quad(&mut p.vertices, x0 - t, y0 - t, x1 + t, y0, UNTEXTURED, frame_colour);
            push_quad(&mut p.vertices, x0 - t, y1, x1 + t, y1 + t, UNTEXTURED, frame_colour);
            push_quad(&mut p.vertices, x0 - t, y0, x0, y1, UNTEXTURED, frame_colour);
            push_quad(&mut p.vertices, x1, y0, x1 + t, y1, UNTEXTURED, frame_colour);
        }

        // An empty slot draws its frame and nothing else. Drawing a
        // greyed-out block instead would suggest the player has one.
        let Some(block) = inventory.block_in(index) else {
            continue;
        };
        let layer = icon_layer(textures, block);
        let tint = if is_selected { ICON_TINT } else { ICON_TINT_DIM };
        push_quad(&mut p.vertices, x0, y0, x1, y1, layer, icon_tint(block, tint));

        // The pack's marks, by the pack's function: a chipped corner for
        // the tool that needs the stone, a square for how well a thing was
        // made, a bar for what is left of it. The bar is where a player
        // actually looks for "which axe is going" -- see
        // `inventory_screen::slot_marks` for why it used to show none.
        let stack = inventory.slots().get(index).copied().flatten();
        crate::ui::inventory_screen::slot_marks(
            &mut p,
            Rect::new(x0, y0, x1, y1),
            block,
            stack.map(|s| s.condition()),
            stack.and_then(|s| s.quality().band()),
        );
    }
    *out = p.into_vertices();
}

/// The colour one item's icon is drawn in.
///
/// The slot's own shade -- bright for the selected slot, dimmer for the
/// rest -- multiplied by whatever the *block* says about itself. Nothing
/// says anything, except the garments: twelve of them share four
/// greyscale pictures and are told apart by this. See
/// `types::garment_tint` for why.
pub fn icon_tint(block: primitive_shared::types::BlockId, slot: [f32; 4]) -> [f32; 4] {
    // ...and the spears, which share one picture for the same reason
    // and are told apart by the colour of the head
    // (`types::spear_tint`). Asked first only because it is the shorter
    // list; the two can never both answer, since a spear is not a
    // garment.
    //
    // **A poisoned one is green**, and that is the only way a player
    // can see that the paste is on: the fly agaric lives in a bit of
    // the block's variant field (`types::POISONED`), so without this
    // the pack would show a spear that is somehow worth two toadstools
    // and look exactly like the one that is not.
    if primitive_shared::types::is_poisoned(block) {
        return [slot[0] * 0.55, slot[1] * 0.95, slot[2] * 0.45, slot[3]];
    }
    // **A steeled iron tool is faintly blue**, the colour a quenched edge
    // takes in the fire -- and the only way to tell it from a wrought one of
    // the same shape at a glance. The hardening lives in a variant bit
    // (`tools::HARDENED`), exactly as the poison does. Faint, because the
    // whole picture takes the tint, haft and all.
    if primitive_shared::tools::is_hardened(block) {
        return [slot[0] * 0.80, slot[1] * 0.87, slot[2] * 1.0, slot[3]];
    }
    // **A wet thing is darker and a little blue** (`wet`), the way a soaked
    // stick is darker than a dry one: the hotbar has no room for a mark, and
    // the question a player asks it at a cold fire is "is my kindling wet".
    // Darker rather than bluer, mostly -- a blue stick reads as a different
    // stick, a dark one as the same stick soaked.
    if primitive_shared::wet::is_wet(block) {
        return [slot[0] * 0.62, slot[1] * 0.68, slot[2] * 0.82, slot[3]];
    }
    // **Raw clay is darker the wetter it is** (`clay::shade`): wet off the
    // hands, leather-hard, then the pale picture itself when it is dry
    // enough to fire -- the one way to see across a pack which pots are
    // ready for the kiln without hovering each.
    if primitive_shared::clay::is_raw_pottery(block) {
        let shade = primitive_shared::clay::shade(block);
        return [slot[0] * shade[0], slot[1] * shade[1], slot[2] * shade[2], slot[3]];
    }
    // **A green log is a shade greener and darker than a seasoned one**
    // (`wood::is_green`): sap under the bark. Faint, as the steel is,
    // because the whole picture takes it.
    if primitive_shared::wood::is_green(block) {
        return [slot[0] * 0.82, slot[1] * 0.92, slot[2] * 0.78, slot[3]];
    }
    if let Some(head) = primitive_shared::types::spear_tint(block) {
        return [
            slot[0] * head[0],
            slot[1] * head[1],
            slot[2] * head[2],
            slot[3],
        ];
    }
    match primitive_shared::types::garment_tint(block) {
        Some(material) => [
            slot[0] * material[0],
            slot[1] * material[1],
            slot[2] * material[2],
            slot[3],
        ],
        None => slot,
    }
}

// ---- what a finger on the bar means ----
//
// **Four gestures on one strip, and none of them is a button.** A phone
// has no number row, no wheel, and no room for a button per key: `1`
// through `0`, the wheel, `E` and `Q` are seven controls that all point
// at the same ten squares, and the squares are already on screen.
//
// So the bar reads its own finger. The four answers are told apart by
// what the finger *does*, not by where it starts -- every one of them
// starts on a slot:
//
// * lifts where it landed  -> choose that slot   (`1`..`0`)
// * slides along the bar   -> step the choice    (the wheel)
// * slides up off the bar  -> throw one out      (`Q`)
// * stays put              -> eat what is held   (`E`)
//
// Sliding wins over resting, and the direction decides which slide it
// was, so a finger that wanders while resting does not eat by accident.

/// How far a finger must travel before it is a slide rather than a tap,
/// as a fraction of a slot's width.
///
/// Measured against the *slot* rather than the screen because that is
/// what the finger is aiming at: the gesture is "I have left the square
/// I started on", and a fraction of the screen means something
/// different on every phone. Two thirds, so a thumb rolling on one
/// square is still on it.
const SLIDE: f32 = SLOT * 0.66;

/// How long a finger must rest on its slot before it is eating.
///
/// Longer than the hold that starts mining (see
/// `platform::touch::is_mining`), and deliberately: mining is what a
/// player does constantly and eating is what they do a few times an
/// hour, so the cheap gesture goes to the common one. Long enough,
/// too, that a slow tap is a tap.
const HOLD_TO_EAT: std::time::Duration = std::time::Duration::from_millis(400);

/// What the bar decided a finger meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touched {
    /// Nothing yet, or nothing at all.
    Nothing,
    /// Choose this slot: the number keys.
    Pick(usize),
    /// Step the choice by one, in the direction of the slide: the
    /// wheel. **One step per slide, never a run of them** -- the spec
    /// this was built to calls it "no inertia", and a bar that keeps
    /// stepping while the finger moves is a bar nobody can land on.
    Step(i32),
    /// Throw one out of this slot: `Q`.
    Throw(usize),
    /// Eat what is in the slot being held: `E`.
    Eat(usize),
}

/// The finger currently on the bar, and what it has done so far.
#[derive(Debug, Clone, Copy, Default)]
pub struct Gestures {
    finger: Option<OnTheBar>,
}

#[derive(Debug, Clone, Copy)]
struct OnTheBar {
    id: crate::platform::TouchId,
    slot: usize,
    origin: (f32, f32),
    down_at: std::time::Instant,
    /// Set once the finger has committed to a slide or to eating, so
    /// that one gesture cannot also be another on the way up.
    spent: bool,
}

impl Gestures {
    /// A touch that landed on, or is moving over, the bar.
    ///
    /// `slot` is where the finger *started*, worked out by the caller
    /// with [`slot_at`] -- the bar cannot do that itself because the
    /// conversion from pixels depends on the interface scale, which
    /// lives with the window. Everything after that is here.
    pub fn handle(
        &mut self,
        phase: crate::platform::TouchPhase,
        id: crate::platform::TouchId,
        slot: Option<usize>,
        at: (f32, f32),
        now: std::time::Instant,
    ) -> Touched {
        use crate::platform::TouchPhase;
        match phase {
            TouchPhase::Started => {
                // One finger at a time. A second on the bar while the
                // first is working is a palm, not a second choice.
                if self.finger.is_none() {
                    if let Some(slot) = slot {
                        self.finger = Some(OnTheBar {
                            id,
                            slot,
                            origin: at,
                            down_at: now,
                            spent: false,
                        });
                    }
                }
                Touched::Nothing
            }
            TouchPhase::Moved => {
                let Some(finger) = self.finger.as_mut().filter(|f| f.id == id && !f.spent) else {
                    return Touched::Nothing;
                };
                let (dx, dy) = (at.0 - finger.origin.0, at.1 - finger.origin.1);
                // Up first: a throw is a deliberate flick and a slide
                // along is what a wandering thumb does, so the rarer
                // gesture gets the stricter test by being asked about
                // first rather than by being given a wider threshold.
                //
                // The interface's y runs *up*, so leaving the bar
                // upward is an increasing y.
                if dy > SLIDE && dy.abs() > dx.abs() {
                    finger.spent = true;
                    return Touched::Throw(finger.slot);
                }
                if dx.abs() > SLIDE {
                    finger.spent = true;
                    return Touched::Step(if dx > 0.0 { 1 } else { -1 });
                }
                Touched::Nothing
            }
            TouchPhase::Ended => {
                let Some(finger) = self.finger.filter(|f| f.id == id) else {
                    return Touched::Nothing;
                };
                self.finger = None;
                if finger.spent {
                    return Touched::Nothing;
                }
                Touched::Pick(finger.slot)
            }
            TouchPhase::Cancelled => {
                // The system taking a finger away is not a choice. See
                // the same argument in `platform::touch`.
                if self.finger.is_some_and(|f| f.id == id) {
                    self.finger = None;
                }
                Touched::Nothing
            }
        }
    }

    /// Whether the finger has now rested long enough to be eating.
    ///
    /// Polled, for the reason a resting finger always has to be: it
    /// produces no events while it sits still, so the moment it stops
    /// being a tap arrives when the platform has nothing to say.
    pub fn resting(&mut self, now: std::time::Instant) -> Touched {
        let Some(finger) = self.finger.as_mut().filter(|f| !f.spent) else {
            return Touched::Nothing;
        };
        if now.duration_since(finger.down_at) < HOLD_TO_EAT {
            return Touched::Nothing;
        }
        // Spent, so the lift that follows does not also choose the slot
        // -- eating and choosing at once is the player watching their
        // selection jump as they eat.
        finger.spent = true;
        Touched::Eat(finger.slot)
    }

    /// Whether this finger is the one the bar is following.
    ///
    /// Asked by the caller *before* handing an event over, because a
    /// lift is what ends the bar's claim: asking afterwards would let
    /// the lift fall through to whatever is drawn underneath, and press
    /// a thumb control the player never aimed at.
    pub fn owns(&self, id: crate::platform::TouchId) -> bool {
        self.finger.is_some_and(|f| f.id == id)
    }

    /// Every finger forgotten, for when the bar stops being touchable.
    pub fn release_all(&mut self) {
        self.finger = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::{TouchPhase, TouchId};

    fn moment(ms: u64) -> std::time::Instant {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *START.get_or_init(std::time::Instant::now) + std::time::Duration::from_millis(ms)
    }

    /// The middle of a slot, in the space the bar is authored in.
    fn on_slot(index: usize) -> (f32, f32) {
        (slot_centre(index, MAX_SLOTS), BOTTOM + SLOT / 2.0)
    }

    const FINGER: TouchId = 1;

    /// A tap on a slot chooses it.
    #[test]
    fn a_tap_on_the_bar_chooses_that_slot() {
        let mut bar = Gestures::default();
        let at = on_slot(3);
        assert_eq!(
            bar.handle(TouchPhase::Started, FINGER, Some(3), at, moment(0)),
            Touched::Nothing,
        );
        assert_eq!(
            bar.handle(TouchPhase::Ended, FINGER, Some(3), at, moment(80)),
            Touched::Pick(3),
        );
    }

    /// A slide along the bar steps the choice once, and only once.
    ///
    /// "One swipe, one step" is the rule this is built to: a bar that
    /// keeps stepping while the finger travels is a bar nobody can land
    /// on. The second and third moves of the same finger say nothing.
    #[test]
    fn a_slide_along_the_bar_steps_once_per_slide() {
        for (direction, expected) in [(1.0f32, 1), (-1.0, -1)] {
            let mut bar = Gestures::default();
            let at = on_slot(4);
            bar.handle(TouchPhase::Started, FINGER, Some(4), at, moment(0));
            let far = (at.0 + direction * SLIDE * 1.5, at.1);
            assert_eq!(
                bar.handle(TouchPhase::Moved, FINGER, Some(4), far, moment(40)),
                Touched::Step(expected),
            );
            let further = (at.0 + direction * SLIDE * 4.0, at.1);
            assert_eq!(
                bar.handle(TouchPhase::Moved, FINGER, Some(4), further, moment(80)),
                Touched::Nothing,
                "the same slide stepped twice",
            );
            // ...and the lift is not also a choice.
            assert_eq!(
                bar.handle(TouchPhase::Ended, FINGER, Some(4), further, moment(120)),
                Touched::Nothing,
                "a slide also chose a slot when it ended",
            );
        }
    }

    /// A flick up off the bar throws one out of that slot.
    #[test]
    fn a_flick_up_off_the_bar_throws_from_that_slot() {
        let mut bar = Gestures::default();
        let at = on_slot(2);
        bar.handle(TouchPhase::Started, FINGER, Some(2), at, moment(0));
        // The interface's y runs up the screen.
        let up = (at.0, at.1 + SLIDE * 1.5);
        assert_eq!(
            bar.handle(TouchPhase::Moved, FINGER, Some(2), up, moment(40)),
            Touched::Throw(2),
        );
        assert_eq!(
            bar.handle(TouchPhase::Ended, FINGER, Some(2), up, moment(90)),
            Touched::Nothing,
            "a throw also chose the slot it came from",
        );
    }

    /// A finger that rests on a slot eats what is in it.
    ///
    /// And having eaten, the lift does not also choose the slot: a
    /// player watching their selection jump as they eat is watching a
    /// second gesture they did not make.
    #[test]
    fn a_finger_resting_on_a_slot_eats_and_does_not_also_choose_it() {
        let mut bar = Gestures::default();
        let at = on_slot(1);
        bar.handle(TouchPhase::Started, FINGER, Some(1), at, moment(0));
        assert_eq!(bar.resting(moment(200)), Touched::Nothing, "ate before the hold was up");
        assert_eq!(bar.resting(moment(500)), Touched::Eat(1));
        assert_eq!(bar.resting(moment(700)), Touched::Nothing, "ate twice on one hold");
        assert_eq!(
            bar.handle(TouchPhase::Ended, FINGER, Some(1), at, moment(900)),
            Touched::Nothing,
        );
    }

    /// A slide beats a rest, however long the finger has been down.
    ///
    /// The failure this stops: a thumb that travels slowly along the bar
    /// crosses the eating threshold on the way and eats instead of
    /// stepping. Direction is what tells the gestures apart, and it is
    /// tested before the clock is.
    #[test]
    fn a_slow_slide_steps_rather_than_eating() {
        let mut bar = Gestures::default();
        let at = on_slot(5);
        bar.handle(TouchPhase::Started, FINGER, Some(5), at, moment(0));
        let far = (at.0 + SLIDE * 1.5, at.1);
        assert_eq!(
            bar.handle(TouchPhase::Moved, FINGER, Some(5), far, moment(100)),
            Touched::Step(1),
        );
        assert_eq!(
            bar.resting(moment(9_000)),
            Touched::Nothing,
            "a finger that had already stepped went on to eat",
        );
    }

    /// A touch that starts off the bar is not the bar's business.
    #[test]
    fn a_finger_that_lands_beside_the_bar_is_ignored() {
        let mut bar = Gestures::default();
        let beside = (0.0, BOTTOM + SLOT * 4.0);
        bar.handle(TouchPhase::Started, FINGER, None, beside, moment(0));
        assert_eq!(bar.resting(moment(900)), Touched::Nothing);
        assert_eq!(
            bar.handle(TouchPhase::Ended, FINGER, None, beside, moment(950)),
            Touched::Nothing,
        );
    }

    /// The system taking the finger away decides nothing.
    #[test]
    fn a_cancelled_touch_on_the_bar_chooses_nothing() {
        let mut bar = Gestures::default();
        let at = on_slot(7);
        bar.handle(TouchPhase::Started, FINGER, Some(7), at, moment(0));
        assert_eq!(
            bar.handle(TouchPhase::Cancelled, FINGER, Some(7), at, moment(50)),
            Touched::Nothing,
        );
        assert_eq!(bar.resting(moment(900)), Touched::Nothing);
    }

    /// Every slot is selected by tapping the middle of where it is
    /// drawn.
    ///
    /// The one property that matters: `slot_at` has to be the exact
    /// inverse of `slot_centre`. They are two expressions of one
    /// layout, and when they disagree the bar selects the slot beside
    /// the one under the finger -- which reads as the game ignoring
    /// half the taps rather than as an arithmetic slip.
    #[test]
    fn a_slot_is_selected_where_it_is_drawn() {
        for count in 1..=MAX_SLOTS {
            for index in 0..count {
                let x = slot_centre(index, count);
                let y = BOTTOM + SLOT / 2.0;
                assert_eq!(
                    slot_at(x, y, count),
                    Some(index),
                    "slot {index} of {count} was not found at its own middle"
                );
            }
        }
    }

    /// A tap that is not on the bar selects nothing.
    ///
    /// Not a formality: the bar sits over the world, and a tap above it
    /// is a tap meant for the world. A `slot_at` that answered for the
    /// whole screen would swallow every dig.
    #[test]
    fn a_tap_away_from_the_bar_selects_nothing() {
        assert_eq!(slot_at(0.0, 0.5, MAX_SLOTS), None, "the middle of the screen");
        assert_eq!(slot_at(0.0, -0.999, MAX_SLOTS), None, "below the bar");
        assert_eq!(
            slot_at(LEFT - 0.1, BOTTOM + SLOT / 2.0, MAX_SLOTS),
            None,
            "left of the bar"
        );
        // ...and the gap between two slots belongs to neither.
        let gap = (slot_centre(0, MAX_SLOTS) + slot_centre(1, MAX_SLOTS)) / 2.0;
        assert_eq!(slot_at(gap, BOTTOM + SLOT / 2.0, MAX_SLOTS), None);
    }

    use super::*;

    #[test]
    fn the_bar_is_centred_horizontally() {
        let count = MAX_SLOTS;
        let first = slot_centre(0, count);
        let last = slot_centre(count - 1, count);
        assert!(
            (first + last).abs() < 1e-5,
            "slots should be symmetric about x=0, got {first} and {last}"
        );
    }

    #[test]
    fn slots_do_not_overlap() {
        let count = MAX_SLOTS;
        for i in 1..count {
            let gap = slot_centre(i, count) - slot_centre(i - 1, count) - SLOT;
            assert!(gap > 0.0, "slots {} and {i} overlap", i - 1);
        }
    }

    #[test]
    fn the_bar_fits_on_screen() {
        // NDC runs -1..1; with the aspect divide, x shrinks on a wide
        // window, so checking the square case is the worst case.
        let count = MAX_SLOTS;
        let half = slot_centre(count - 1, count) + SLOT / 2.0;
        assert!(half < 1.0, "hotbar is wider than the viewport: {half}");
    }

    #[test]
    fn icon_quads_are_not_upside_down() {
        // The top edge of the quad (larger y in NDC) must carry v = 0,
        // the top of the image.
        let mut quad = Vec::new();
        push_quad(&mut quad, -0.1, -0.1, 0.1, 0.1, 3, ICON_TINT);
        for vertex in &quad {
            let expected_v = if vertex.position[1] > 0.0 { 0.0 } else { 1.0 };
            assert_eq!(
                vertex.uv[1], expected_v,
                "vertex at y={} should have v={expected_v}",
                vertex.position[1]
            );
        }
    }

    #[test]
    fn a_full_bar_fits_in_the_vertex_budget() {
        // 1 backdrop + per slot: the recess, 4 frame quads, 1 icon, and
        // every mark at once -- quality 2, edge 4, wear 2.
        let quads = 1 + MAX_SLOTS * (6 + 8);
        assert!(
            quads * 6 <= MAX_HOTBAR_VERTICES,
            "{MAX_SLOTS} full slots need {} vertices but the budget is {MAX_HOTBAR_VERTICES}",
            quads * 6
        );
    }

    #[test]
    fn the_hotbar_matches_the_inventory_it_draws() {
        // The bar draws the inventory's *hotbar* slots -- the first ten
        // of them. If the two disagreed, the number keys would point
        // somewhere the player cannot see.
        assert_eq!(MAX_SLOTS, crate::logic::inventory::HOTBAR_SLOTS);
    }

    #[test]
    fn a_full_bar_still_fits_on_a_square_window() {
        let count = MAX_SLOTS;
        let half = slot_centre(count - 1, count) + SLOT / 2.0;
        assert!(half < 1.0, "a full hotbar is wider than the viewport: {half}");
    }
}
