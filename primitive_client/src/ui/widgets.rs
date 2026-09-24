//! Screen-space widgets: panels, buttons, text fields and text.
//!
//! Everything here emits `HotbarVertex`, so the menus, the pause screen
//! and the debug panel all go through the one UI pipeline the hotbar
//! already uses. No second shader, no second buffer, no second set of
//! blend state to keep in sync.
//!
//! ## Coordinates
//!
//! The UI vertex shader divides x by the viewport aspect, so geometry is
//! authored as if the window were square and one unit tall from centre
//! to edge:
//!
//! ```text
//!   y:  +1 top ... -1 bottom          (always)
//!   x:  -aspect ... +aspect           (so ±1 is a screen height wide)
//! ```
//!
//! The consequence worth remembering is that widths are in units of
//! *screen height*, not width: a panel 1.5 wide stays the same physical
//! size when the window is made wider, which is what you want -- a menu
//! that stretches to fill an ultrawide monitor is unreadable.
//!
//! `cursor_to_ui` is the exact inverse of that mapping, and is what makes
//! the mouse land where the player sees the button.
//!
//! ## Why quads for text
//!
//! One quad per lit pixel of the 6x9 bitmap font. Wasteful in the
//! abstract and completely irrelevant here: a full screen of menu text is
//! a few thousand triangles, drawn once per frame, on a screen that
//! mostly isn't drawing a world. It buys us text with no font file to
//! ship, no glyph atlas to pack and no dependency to add.
//!
//! # The seven rules this interface is held to
//!
//! Written down after a pass that rendered every screen at four window
//! shapes in two languages and looked at the pictures. The player's own
//! verdict on what was there was two words long, and every one of these
//! rules names a specific thing in those pictures that earned it. They
//! are here rather than in a document because a rule nobody trips over
//! while editing is a rule that lasts one release.
//!
//! **1. Use the room.** A screen is drawn as large as the glass allows
//! *before* anybody is asked to change a setting. This was the single
//! worst thing in the audit: at 1280x720 the pack was a 710x310 postage
//! stamp in the middle of an empty window, the hearth 400x510, and the
//! anvil's job list 445x380 -- a quarter of the screen each, at the
//! smallest legible size the font has, with the other three quarters
//! carrying nothing. [`Layout::fit`] used to answer `requested`, which
//! is 1.0 until a player finds INTERFACE SIZE; it now answers the room.
//! The setting multiplies from there, so it is still live in both
//! directions.
//!
//! **2. A panel is sized to what is in it, never to the worst case.**
//! The anvil offers two jobs and drew a list eight rows tall, so the
//! common case was a panel with a hole in it. A screen whose furniture
//! is a function of its contents cannot have a hole.
//!
//! **3. One colour scale, and a meter owns its hue everywhere it is
//! drawn.** The HUD had seven strips in four blues and three ambers --
//! stamina, breath, water and rest were the same blue at a glance, and
//! food, thirst-warning and warmth the same brown -- which is what
//! turned the stack into a barcode. Worse, the health page of the pack
//! drew the same seven gauges in a *fourth* language, green/yellow/pink
//! by level, so nothing on one screen could be matched to anything on
//! the other. See `hud::METER_INK`: one table, read by both.
//!
//! **4. One mark for one thing, and the mark shows the thing itself.**
//! A heart is health on the HUD, in the legend and on the health page.
//! A picture drawn twice is a picture that stops agreeing with itself.
//!
//! **5. One spacing scale.** Air between things comes from this file --
//! [`BEVEL`], [`SHADOW_OFFSET`], the pads a screen derives from its own
//! cell -- not from a number picked by eye at the call site. Two
//! screens with different margins read as two programs.
//!
//! **6. One way out.** Escape, the key that opened it, or a tap beside
//! the panel. No screen gets a close button of its own that the others
//! do not have, because a control that exists on one screen and not the
//! next is a control a player has to look for every time.
//!
//! **7. Anything that hit-tests is the exact inverse of what draws
//! it.** Not a style note: the interface is authored in its own space
//! and then multiplied, so a hit test written independently of the
//! drawing is a panel that looks right and answers in the wrong place.
//! Every rect here has one function that both halves call, and a test
//! that says so.
//!
//! What is deliberately *not* a rule: prettiness that costs a reading.
//! Nothing here is decorative. A bevel is there so a raised thing reads
//! as pressable; a well is there so a figure over the world can be read
//! at all (`hud::READOUT_WELL` measures 9.63:1 against 3.4:1 for the
//! same glyphs over stone).

use crate::engine::font::{text_width, CAP_HEIGHT, GLYPH_HEIGHT, GLYPH_SPACING, GLYPH_WIDTH};
use crate::ui::hotbar::{HotbarVertex, UNTEXTURED};

/// Height of one font pixel at scale 1.0.
pub const PIXEL: f32 = 0.0052;

/// How much of a settings row its buttons take up, measured from the
/// right edge.
///
/// Named here rather than repeated as a bare `0.26` in the screen and a
/// bare `0.30` in the row painter, which is what it was -- two numbers
/// that had to agree, four hundred lines apart, and the reading was
/// right-aligned against the wrong one. Everything that has to keep
/// clear of the buttons measures from this.
pub const CONTROL_COLUMN: f32 = 0.28;

/// Air either side of a button's label, so a fitted word does not sit
/// against the bevel.
const BUTTON_TEXT_PAD: f32 = 0.018;

/// How far a button's label may shrink before it is left to overflow.
///
/// Two thirds. Below that a label stops being legible, and an unreadable
/// button is not an improvement on a slightly overfull one -- what it is
/// instead is a sign that the button wants to be wider, which is a
/// layout decision rather than something a text routine should make on
/// its own.
const BUTTON_TEXT_FLOOR: f32 = 0.66;

/// The button height the full-size label was chosen for.
///
/// A menu button -- `ГОТОВО`, `ПРОДОЛЖИТЬ` -- is this tall, and anything
/// shorter letters itself in proportion. That is what keeps a switch on
/// a settings row from being written at the size of a button that is
/// three times its height.
const BUTTON_DESIGN_HEIGHT: f32 = 0.100;

/// Vertical distance between consecutive lines of text at a given scale.
///
/// The full cell plus a little: the cell already includes the descender
/// rows, so consecutive lines cannot collide, and the extra is leading.
pub fn line_height(scale: f32) -> f32 {
    PIXEL * scale * (GLYPH_HEIGHT as f32 + 2.0)
}

/// The size a button letters `text` at on `rect`, at an interface `content`
/// scale: fitted to the width and capped by the height (see
/// `Painter::button`). Its own function so a screen that puts something else
/// on a button -- a picture beside the words -- can ask how wide the words
/// will be, rather than keeping a copy of this arithmetic that drifts.
pub fn button_label_scale(rect: Rect, text: &str, content: f32) -> f32 {
    let usable = (rect.width() - BUTTON_TEXT_PAD * 2.0).max(0.0);
    // **Never under the smallest size the interface writes in** while the
    // button is tall enough to hold it. The floor was 0.55, under
    // `size::NOTE` -- so a desktop's short buttons (the pack's tabs, TIDY,
    // TAKE ALL) were lettered smaller than the tooltip beside them, a sixth
    // tier nobody chose; the pack's tab words came out smaller than the
    // caption over the grid under them. The height check keeps a switch on
    // a settings row from being written taller than itself.
    let holds = rect.height() * 0.8 / (PIXEL * crate::engine::font::GLYPH_HEIGHT as f32);
    let floor = size::NOTE.min(holds).max(0.55);
    let tall_enough = (rect.height() / BUTTON_DESIGN_HEIGHT).max(floor).min(content.max(floor));
    fitted_scale(text, tall_enough, usable, BUTTON_TEXT_FLOOR)
}

/// How far the pen advances over a string -- what to use for laying out
/// and for fitting text into a box.
pub fn measure(text: &str, scale: f32) -> f32 {
    text_width(text) as f32 * PIXEL * scale
}

/// The width text actually occupies: the advance less the blank column
/// the last glyph carries on its right.
///
/// This is what centring uses. Centring on the raw advance puts every
/// line half a pixel left of where it belongs, because that trailing gap
/// gets counted as part of the word.
///
/// It is deliberately still a whole number of cells rather than the
/// exact extent of the lit pixels. Measuring the ink would centre
/// "PLAY" and "PLAY!" differently by a pixel or two, and in a column of
/// buttons that reads as the labels being slightly crooked -- a fixed
/// grid staying on its grid looks better than each label being
/// individually perfect.
pub fn ink_width(text: &str, scale: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    (measure(text, scale) - PIXEL * scale * 1_f32).max(0.0)
}

/// Height of one glyph cell, descender rows included.
///
/// This, not the cap height, is what a line of text needs vertically:
/// centring uses the cap height because that is what the eye reads as
/// the text, but a box has to hold the whole cell or the tails of
/// `g j p q y` hang out of it.
///
/// `const` so that a layout can be *derived* from it rather than
/// measured by eye: the HUD's notice plate is placed above a stack of
/// meters whose height is a sum of constants, and the plate's own
/// height is one of the terms. See `hud::NOTICE_Y`.
pub const fn cell_height(scale: f32) -> f32 {
    PIXEL * scale * GLYPH_HEIGHT as f32
}

/// The largest scale at or below `wanted` at which `text` fits
/// `max_width`.
///
/// **What every box with writing in it should be asking**, and until now
/// nothing did: `button` drew its label at a fixed size whatever the
/// button was, so a long word simply ran out of both ends of it. That is
/// invisible in English -- `DONE`, `BACK`, `PLAY` -- and immediate in
/// Russian, where the same words are `ГОТОВО`, `НАЗАД` and `ВКЛ/ВЫКЛ`.
/// The switch label was half again wider than the switch.
///
/// Shrinking rather than truncating, because these are *labels* and a
/// truncated one (`ВКЛ/ВЫ..`) says less than a small one. `floor` is
/// where it gives up and lets the text overflow: below about two thirds
/// a label stops being legible, and a button nobody can read is not an
/// improvement on a button that is slightly too full.
pub fn fitted_scale(text: &str, wanted: f32, max_width: f32, floor: f32) -> f32 {
    if max_width <= 0.0 || text.is_empty() {
        return wanted;
    }
    let needed = measure(text, wanted);
    if needed <= max_width {
        return wanted;
    }
    (wanted * max_width / needed).max(wanted * floor)
}

/// Shortens `text` until it fits `max_width`, ending in `..`.
///
/// Visibly truncated rather than merely clipped: a name that just stops
/// looks like the name, and the player has no way to tell that the
/// server they are looking at is not the one they meant.
pub fn fit(text: &str, scale: f32, max_width: f32) -> String {
    if measure(text, scale) <= max_width {
        return text.to_string();
    }
    const ELLIPSIS: &str = "..";
    let budget = max_width - measure(ELLIPSIS, scale);
    if budget <= 0.0 {
        return String::new();
    }
    let per_char = PIXEL * scale * (GLYPH_WIDTH + GLYPH_SPACING) as f32;
    let keep = (budget / per_char).floor().max(0.0) as usize;
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(ELLIPSIS);
    out
}

// --- palette ---
//
// One place, so the menus, the pause screen and the debug panel can't
// drift into looking like three different games.

// ---- the palette ----
//
// **Stone, not glass.** This was a dark translucent sheet with hairline
// edges and a gradient -- the look every application has had since about
// 2015 -- and against a world made of sixteen-pixel blocks it read as an
// overlay from a different program. What replaced it is the oldest
// interface in this kind of game and the reason it has lasted: a slab of
// light grey stone with a *bevel*, wells cut into it for the slots, and
// dark text printed on it.
//
// The bevel is the whole of the effect and it is four quads: a light
// edge along the top and left, a dark one along the bottom and right.
// That is what a raised surface looks like, it costs nothing, and it
// works at any size because it is not a gradient or a blur -- it is two
// colours and a corner.

// ---- two skins, and why ----
//
// The **stone** is what the world's own screens are made of: the pack,
// a chest, a hearth. They sit over the game, they are full of blocks,
// and a light slab with wells cut into it is the interface that kind of
// screen has always had.
//
// The **dark** skin is the menu's, and the menu is a different thing: it
// is what you look at *instead of* the world -- before there is one, or
// with it paused behind a scrim -- and a big pale slab there is a wall
// of grey in a dark room. It is also what this game's menu looked like
// before the stone arrived, and it was not the thing that was wrong.
//
// One `Theme` carried by the painter rather than two sets of widgets:
// every screen calls the same `panel`, `button` and `field`, and what
// differs is eleven colours.

/// What a painter is drawing with.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub panel: [f32; 4],
    /// The two bevel edges: lit (top and left) and shaded.
    pub light: [f32; 4],
    pub dark: [f32; 4],
    /// A slot, and the two edges of the well it sits in.
    pub well: [f32; 4],
    pub well_dark: [f32; 4],
    pub well_light: [f32; 4],
    pub button: [f32; 4],
    pub button_hover: [f32; 4],
    /// The floor of a shallow tray: a group of slots stands in one.
    ///
    /// Between the panel and a well, and nearer the panel. It exists
    /// because the screens that are made of slots had no way of saying
    /// that one run of slots is a different *place* from the run beside
    /// it -- the pack, the body and the recipes were thirty-nine
    /// identical holes told apart by a thousandth of a screen of extra
    /// gap. Widening the gaps was not available: the pack screen is
    /// 1.995 wide and has to fit a window 2.0 across. Depth was.
    pub tray: [f32; 4],
    pub disabled: [f32; 4],
    pub field: [f32; 4],
    pub row: [f32; 4],
    pub row_selected: [f32; 4],
    /// What is written on it, and the quieter version.
    pub ink: [f32; 4],
    pub ink_dim: [f32; 4],
    /// The one colour that is not grey: a heading, a chosen row, a full
    /// stack.
    pub accent: [f32; 4],
}

impl Theme {
    /// The world's own screens: a slab of stone with wells cut in it.
    ///
    /// **Lit like the rest of the game, which it was not.** The first
    /// cut of this was near-white, glared against a dark world, and was
    /// pulled back to two thirds of the way up from black -- and that
    /// version still made the panel the brightest thing on the screen
    /// by a wide margin, which is exactly what the change was supposed
    /// to stop. A chest opened at night lit the room.
    ///
    /// What actually went wrong is measurable, and all of it points the
    /// same way. On the old grey, `accent` -- the one colour on this
    /// theme that is not grey, and the colour a heading is written in
    /// -- came to **2.24:1** against the panel and **1.56:1** against a
    /// slot. The colour whose whole job is to mark a heading was the
    /// least readable thing on the screen, so nothing read as a
    /// heading, and the screens had no hierarchy at all: a title, a
    /// section caption and a hint line were three shades of the same
    /// near-black. `well` against `panel` was 1.43:1, so a slot barely
    /// read as a recess either.
    ///
    /// The fix is not to make it darker *and keep going*: it is to
    /// light it the way everything else in this game is lit -- pale ink
    /// and the same amber -- on stone rather than on the menu's
    /// blue-black. The two skins stay two skins, which is the point of
    /// having them; what they now share is the ink, so a player is
    /// reading one interface in two lights instead of two interfaces.
    ///
    /// Every surface here is a luminance chosen backwards from the
    /// contrast it has to reach, then given a warm cast at that
    /// luminance -- warm, because grey with a red bias reads as stone
    /// and grey with a blue bias reads as the menu. See
    /// `small_text_is_readable_against_everything_it_is_drawn_on`,
    /// which now holds `accent` to a floor as well; it did not, and
    /// that omission is the whole reason a 2.24:1 heading colour
    /// survived this long.
    pub const STONE: Theme = Theme {
        panel: [0.144, 0.125, 0.107, 1.0],
        light: [0.304, 0.264, 0.225, 1.0],
        dark: [0.051, 0.044, 0.037, 1.0],
        well: [0.059, 0.051, 0.043, 1.0],
        well_dark: [0.029, 0.025, 0.022, 1.0],
        well_light: [0.327, 0.284, 0.241, 1.0],
        // A button cannot go lighter than this and still carry `ink` at
        // 4.5:1, so what makes it read as raised is its bevel rather
        // than its fill -- the same trade `DARK` makes, for the same
        // reason.
        button: [0.180, 0.157, 0.133, 1.0],
        button_hover: [0.276, 0.240, 0.204, 1.0],
        // Dark enough to read as a recess against the panel and light
        // enough that the forty wells cut into it still read as the
        // deeper thing. Its bevel does most of that work, which is why
        // the colour can sit this close to the panel's own.
        tray: [0.101, 0.088, 0.075, 1.0],
        disabled: [0.118, 0.103, 0.087, 1.0],
        field: [0.056, 0.049, 0.042, 1.0],
        row: [0.152, 0.132, 0.112, 1.0],
        row_selected: [0.175, 0.215, 0.150, 1.0],
        ink: [0.95, 0.94, 0.91, 1.0],
        ink_dim: [0.72, 0.71, 0.68, 1.0],
        // The menu's amber, unchanged. One accent for the whole game:
        // a full stack means the same thing on a chest as it does in
        // the inventory, and it should not change colour on the way.
        accent: [1.00, 0.78, 0.28, 1.0],
    };

    /// The menu's: dark panels over a darkened world, pale text.
    ///
    /// The same shapes -- slabs, wells, bevels -- in the other
    /// direction, so the two skins are one interface in two lights
    /// rather than two interfaces.
    pub const DARK: Theme = Theme {
        panel: [0.105, 0.115, 0.145, 0.96],
        light: [0.27, 0.29, 0.35, 1.0],
        dark: [0.04, 0.045, 0.06, 1.0],
        well: [0.075, 0.082, 0.105, 1.0],
        well_dark: [0.03, 0.035, 0.045, 1.0],
        well_light: [0.22, 0.24, 0.30, 1.0],
        // A shade darker than it was, because the ink above is
        // already within a hair of white and there was nowhere left to
        // go on that side: 0.180 put the pair at 4.34:1, just under
        // what body text asks for. A button is a surface, and a surface
        // can give way; the writing on it cannot.
        button: [0.148, 0.162, 0.202, 1.0],
        button_hover: [0.255, 0.285, 0.355, 1.0],
        // The menus have no trays -- they are made of rows, not of
        // slots -- but a skin with a hole in it is a skin that cannot be
        // switched to, so it is here and it is measured like the rest.
        tray: [0.088, 0.096, 0.122, 1.0],
        disabled: [0.115, 0.125, 0.155, 1.0],
        field: [0.055, 0.062, 0.080, 1.0],
        // Two per cent darker than it was, and the test is why:
        // the amber written on a menu row came to 4.44:1, just
        // under what small text asks for. Nobody would see the
        // difference in the surface; the point is the writing on
        // it. Found by holding `accent` to a floor for the first
        // time -- the stone theme was what prompted that, and it
        // turned up a second theme sitting just under the line.
        row: [0.121, 0.135, 0.169, 1.0],
        row_selected: [0.200, 0.255, 0.220, 1.0],
        // Lifted a little for the same measurement: against a button
        // -- the lightest surface in this theme -- the old ink came to
        // 4.33:1, just under what body text asks for.
        ink: [0.93, 0.95, 0.98, 1.0],
        ink_dim: [0.62, 0.66, 0.73, 1.0],
        accent: [1.00, 0.78, 0.28, 1.0],
    };
}

// ---------------------------------------------------------------- skin
//
// **The interface is drawn out of pictures now, and the pictures are
// multipliers rather than colours.**
//
// The player's words were that the screens looked like flat rectangles
// out of code, and they were: a panel was a fill and four bevel quads,
// a slot was the same five quads upside down. What they are now is
// `assets/textures/ui/*.png` -- tanned hide with a stitched channel and
// four rivets, cells with a lip and a floor, boards with a grain and a
// pressed face -- sampled off the same texture array the world uses,
// through the same pipeline the hotbar already used for block icons.
//
// ## Why the pictures have no colour of their own
//
// The shader does `sampled.rgb * tint.rgb`. The tint is the theme's
// surface colour and the picture is a multiplier about 1.0, so a texel
// of 1.0 leaves the surface exactly the colour `Theme` says it is.
// That is what lets **one** skin serve the stone screens and the menu's
// dark one -- the promise `Theme` already makes, "one interface in two
// lights". A picture with its own browns in it would have broken that
// promise the moment the menu opened, and it would also have made every
// contrast measurement in this file a lie: those are taken against the
// theme's numbers, and they stay true only while the skin's mean texel
// is neutral. See `the_skin_shades_a_surface_without_moving_its_mean`.
//
// Rejected: two skins, one drawn in stone and one in the menu's blue.
// Thirty pictures instead of fifteen, and two of everything that has to
// keep agreeing -- which in this codebase is the failure that always
// happens.
//
// ## Why nothing moved
//
// Every one of these draws **inside the rectangle it is given**, the
// same rectangle the old fill and bevel covered. No hit test changed,
// because no rectangle changed; rule 7 at the top of this file is kept
// by construction rather than by a second piece of arithmetic that has
// to match. See `the_skin_covers_its_rectangle_and_nothing_outside_it`.

/// One picture of the interface's own skin.
///
/// The order is the lookup -- a piece's layer is the skin's base plus
/// its number -- and it is the order of the `ui/` run at the end of
/// `texture::EXTRA_TEXTURES`. A piece inserted in the middle of either
/// list draws a button where a slot belongs, with nothing to say so, so
/// there is a test holding the two lists together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Panel,
    Tray,
    Well,
    Slot,
    SlotHover,
    SlotSelected,
    SlotBlocked,
    Button,
    ButtonHover,
    ButtonDown,
    TabOn,
    TabOff,
    Track,
    Grip,
    Rule,
}

impl Piece {
    /// Every piece, in the order the array holds them.
    pub const ALL: [Piece; 15] = [
        Piece::Panel,
        Piece::Tray,
        Piece::Well,
        Piece::Slot,
        Piece::SlotHover,
        Piece::SlotSelected,
        Piece::SlotBlocked,
        Piece::Button,
        Piece::ButtonHover,
        Piece::ButtonDown,
        Piece::TabOn,
        Piece::TabOff,
        Piece::Track,
        Piece::Grip,
        Piece::Rule,
    ];

    /// The file it is drawn in, under `assets/textures/`.
    pub fn file(self) -> &'static str {
        crate::engine::texture::EXTRA_TEXTURES[crate::engine::texture::EXTRA_UI_SKIN + self as usize]
    }

    /// How many of the picture's 32 texels the frame takes on each side.
    ///
    /// Eight for the panel, because that is what a row of stitches needs
    /// to be stitches rather than a dotted line; six for everything else,
    /// which is a lip and a bead and no more.
    fn border_texels(self) -> f32 {
        match self {
            Piece::Panel => 8.0,
            // A cell and its overlays are drawn whole rather than sliced
            // -- see `Painter::cell` -- so their border is never asked
            // for. Answering the lip's own depth keeps the number honest
            // for anything that does slice one.
            Piece::Slot | Piece::SlotHover | Piece::SlotSelected | Piece::SlotBlocked => 4.0,
            _ => 6.0,
        }
    }

    /// How thick the frame is drawn on the screen.
    ///
    /// Separate from [`Piece::border_texels`], which is how much of the
    /// *picture* the frame is, because the two answer different
    /// questions: a panel's frame is eight texels because stitching
    /// needs eight, and it is [`PANEL_BORDER`] wide because that is what
    /// the screens around it already clear. Everything else is drawn at
    /// one texel to one font pixel, which is what a lip should be.
    fn screen_border(self) -> f32 {
        match self {
            Piece::Panel => PANEL_BORDER,
            other => other.border_texels() * SKIN_TEXEL,
        }
    }
}

/// How big one texel of the skin is on the screen.
///
/// One font pixel: the stitching, the lip and the letters are then all
/// drawn at the same size, and a screen made bigger by INTERFACE SIZE
/// grows all three together (the whole vertex list is multiplied about
/// the middle -- see `scale_about`).
pub const SKIN_TEXEL: f32 = PIXEL;

/// How much of a panel's edge its frame takes, on the screen.
///
/// **Public because it is what a screen has to clear**: a title written
/// closer than this to a panel's edge is a title printed on the
/// stitching. The old bevel was [`BEVEL`], four times thinner, which is
/// why the two pads that were picked against *that* are derived from
/// this now instead of being numbers somebody typed.
///
/// Thirty thousandths, which is `inventory_screen::PANEL_PAD` exactly:
/// the pack already inset its contents by that much, so the frame ends
/// on the line the pack's first slot begins at. The picture spends
/// eight of its thirty-two texels on the frame -- what a row of
/// stitches needs to be stitches -- and those eight are drawn a shade
/// under one screen pixel each at 720p, which is the same squeeze a
/// letter takes at any interface size that is not exactly 1.0.
pub const PANEL_BORDER: f32 = 0.030;

/// What a skin quad's tint is multiplied by.
///
/// Two and a half. A texel byte cannot go above 1.0, so a plain
/// multiplier can only ever *darken* a surface -- and a bevel is half
/// highlight. Lifting the tint on the way in and drawing mid-grey at 1.0
/// buys the other half: a texel of 2.5 is two and a half times the
/// theme's colour, a texel of 0.4 is a third of it, and 1.0 is the
/// colour itself.
///
/// Two and a half rather than two, because the bevel this skin replaces
/// was already brighter than two: `Theme::STONE.light` is 2.11 times
/// `Theme::STONE.panel`, and a skin that could not reach the edge it was
/// replacing would have made every panel flatter than the one before it.
/// `tools/draw_ui_skin.py` encodes against this number, so the two have
/// to move together.
pub const SKIN_GAIN: f32 = 2.5;

/// The side of one tile of a panel's field, on the screen.
///
/// **The middle of a panel is tiled, not stretched.** A sixteen-texel
/// field pulled across a screen two units wide is not a texture, it is
/// four soft blobs; this is a pixel game and that is the one thing it
/// must not look like. Tiling costs quads -- about a hundred and forty
/// for the biggest panel in the game, against five for the old flat
/// fill -- and buys hide that is the same grain wherever it is drawn.
/// Measured rather than guessed: see
/// `a_whole_screen_of_skin_stays_inside_one_upload`.
///
/// Twice the field's own sixteen texels, so the grain is chunky enough
/// to read as a material at arm's length and half as many quads as it
/// would be at one to one.
pub const FIELD_TILE: f32 = 32.0 * SKIN_TEXEL;

/// Where the interface's pictures are in the texture array.
///
/// `None` until the atlas has been built, which is not a fallback
/// nobody meets: every test in this crate lays screens out without a
/// graphics card, and the geometry they check is the same either way --
/// the skin only ever changes what a rectangle is *filled with*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skin {
    base: Option<u32>,
}

/// The base layer, or [`NO_SKIN`] for "the atlas has not been built".
static SKIN_BASE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(NO_SKIN);

/// Not a layer: `UNTEXTURED` is what a flat quad carries, so no real
/// picture can ever be there.
const NO_SKIN: u32 = UNTEXTURED;

/// Says where the interface's pictures ended up.
///
/// Called once, by the texture loader, because that is the only place
/// that knows: a layer number is whatever the atlas happened to hand
/// out. **A global rather than a parameter**, and the argument is worth
/// writing down because the obvious answer is the other one.
///
/// Threading it would mean a `Skin` on all fifty-six `Painter`
/// constructors and on every `build` beneath them -- including the
/// menu's, which has no texture table in scope at all and would have
/// had to grow one. Fifty-six places is fifty-six chances to build a
/// painter with no skin, and a screen drawn with no skin does not fail:
/// it comes out in the old flat rectangles, next to screens that did
/// not, which is exactly the "two programs" that rule 5 of this file
/// exists to stop. One number, published once, cannot be forgotten on
/// one screen.
///
/// The price is a global, and it is paid down the way `PRETEND_TOUCH`
/// pays it: in a test binary this number is never read at all, and a
/// test that wants the skin on turns it on for its own thread. See
/// `skin`, which is two functions for that reason.
pub fn publish_skin(base: u32) {
    SKIN_BASE.store(base, std::sync::atomic::Ordering::Relaxed);
}

/// What the painters are drawing with.
#[cfg(not(test))]
pub fn skin() -> Skin {
    match SKIN_BASE.load(std::sync::atomic::Ordering::Relaxed) {
        NO_SKIN => Skin::NONE,
        base => Skin::at(base),
    }
}

/// **In a test binary the skin is whatever the test asked for, and
/// nothing else** -- the published number is not read here at all.
///
/// It is not tidiness. This crate has tests that build a real atlas on
/// a real device (`atlas_split_repro`, `model_light_repro`), and the
/// loader publishes the skin as part of doing that. Read the global
/// here and one of those tests would turn the skin on for every other
/// test sharing the process, so whether a menu's golden fingerprint
/// matched would depend on the order the tests happened to run in --
/// which is exactly the failure `while_recording_text` refuses a global
/// for, arriving by the back door.
#[cfg(test)]
pub fn skin() -> Skin {
    match TEST_SKIN.with(|cell| cell.get()) {
        NO_SKIN => Skin::NONE,
        base => Skin::at(base),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SKIN: std::cell::Cell<u32> = const { std::cell::Cell::new(NO_SKIN) };
}

/// Draws the rest of *this thread* with the skin's pictures at `base`.
///
/// For the snapshot tools, whose whole body is one screen after another
/// and which would otherwise be one closure four hundred lines long.
/// It needs no restoring: `cargo test` gives every test its own thread,
/// so the setting dies with the test that made it.
#[cfg(test)]
pub fn use_skin(base: u32) {
    TEST_SKIN.with(|cell| cell.set(base));
}

/// Runs `body` with the skin's pictures at `base`.
///
/// A thread-local and not a store into the global, for the reason
/// `while_recording_text` gives: `cargo test` runs several tests at
/// once and a global would have one test drawing another test's screen.
#[cfg(test)]
pub fn with_skin<T>(base: u32, body: impl FnOnce() -> T) -> T {
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_SKIN.with(|cell| cell.set(self.0));
        }
    }
    let restore = Restore(TEST_SKIN.with(|cell| cell.replace(base)));
    let answer = body();
    drop(restore);
    answer
}

impl Skin {
    /// Nothing drawn out of pictures: the flat fills and bevels this
    /// interface was made of before there was a skin.
    pub const NONE: Skin = Skin { base: None };

    /// The pictures starting at `base`.
    pub fn at(base: u32) -> Skin {
        Skin { base: Some(base) }
    }

    /// The array layer a piece is on.
    pub fn layer(self, piece: Piece) -> Option<u32> {
        self.base.map(|base| base + piece as u32)
    }

    /// Which piece a layer is, if it is one of ours.
    ///
    /// For the harnesses that turn a vertex list back into pixels
    /// without a graphics card -- `ui::snapshot` and `dump_to_png` --
    /// which would otherwise draw every panel as the grey plate they
    /// stand a block icon in.
    #[cfg(test)]
    pub fn piece_of(self, layer: u32) -> Option<Piece> {
        let base = self.base?;
        let index = layer.checked_sub(base)? as usize;
        Piece::ALL.get(index).copied()
    }
}

/// How many texels square one picture of the skin is.
///
/// The pack's own resolution (`blocks.toml`), because these live in the
/// same array as the blocks and every layer in an array is one size.
const SKIN_RESOLUTION: f32 = 32.0;

/// A surface colour as a skin quad carries it. See [`SKIN_GAIN`].
fn skin_tint(colour: [f32; 4]) -> [f32; 4] {
    [
        colour[0] * SKIN_GAIN,
        colour[1] * SKIN_GAIN,
        colour[2] * SKIN_GAIN,
        colour[3],
    ]
}

/// Cuts a run into tiles, or leaves it whole.
///
/// The last tile is short and its picture is cut short with it, so the
/// run ends exactly where it was asked to -- a tiled edge that rounded
/// up to a whole tile would hang a texel or two past the rectangle,
/// which is the one thing nothing in this file is allowed to do.
fn spans(from: f32, to: f32, u_from: f32, u_to: f32, tile: Option<f32>) -> Vec<(f32, f32, f32, f32)> {
    let span = to - from;
    match tile {
        Some(tile) if tile > 0.0 && span > tile => {
            let count = (span / tile).ceil() as usize;
            (0..count)
                .map(|index| {
                    let start = from + index as f32 * tile;
                    let end = (start + tile).min(to);
                    (start, end, u_from, u_from + (end - start) / tile * (u_to - u_from))
                })
                .collect()
        }
        _ => vec![(from, to, u_from, u_to)],
    }
}

/// The stone the panels are cut from.
pub const PANEL: [f32; 4] = Theme::STONE.panel;
/// How thick a bevel is. Three pixels at a 720-tall window: the size a
/// bevel has been in this kind of interface since the first one.
pub const BEVEL: f32 = 0.008;

/// How far a panel's shadow is offset, down and to the right.
///
/// Public because it is the one thing a screen legitimately draws
/// *outside* its own panel, and the test that checks nothing else does
/// has to know about it.
pub const SHADOW_OFFSET: f32 = 0.010;
pub const PANEL_DARK: [f32; 4] = Theme::STONE.dark;

/// A slot: a well *cut into* the stone, so its bevel runs the other way.
pub const WELL: [f32; 4] = Theme::STONE.well;
pub const WELL_DARK: [f32; 4] = Theme::STONE.well_dark;

/// A button is a smaller slab of the same stone.
pub const BUTTON: [f32; 4] = Theme::STONE.button;

/// The shallow tray a group of slots stands in. See `Theme::tray`.
pub const TRAY: [f32; 4] = Theme::STONE.tray;
pub const BUTTON_HOVER: [f32; 4] = Theme::STONE.button_hover;

/// What is written on stone, and what is written over the world.
///
/// Two inks, and the difference matters: a panel is light, so text on it
/// is dark; the chat and the debug readout are drawn over the sky, so
/// they stay pale. Reversing either is the fastest way to make an
/// interface unreadable.
pub const INK: [f32; 4] = Theme::STONE.ink;
pub const INK_DIM: [f32; 4] = Theme::STONE.ink_dim;
/// A heading on stone -- now the same amber the menus use.
///
/// It was "the dark gold of a title in a book, not a glow", and a book
/// is lit from the front. These panels are not: they hang in a dark
/// world, and dark gold on grey stone measured 2.24:1, which is not a
/// heading colour, it is a heading nobody can see. See `Theme::STONE`.
pub const ACCENT: [f32; 4] = Theme::STONE.accent;

pub const TEXT: [f32; 4] = [0.94, 0.94, 0.94, 1.0];
pub const TEXT_DIM: [f32; 4] = [0.72, 0.72, 0.72, 1.0];
/// Something is wrong, and something is working.
///
/// **Lifted for the surfaces they are written on today, which are not
/// the ones they were picked for.** `[0.62, 0.13, 0.13]` and
/// `[0.16, 0.42, 0.11]` were a dark red and a dark green for printing
/// on pale stone. Both skins went dark (`Theme::STONE`, `Theme::DARK`)
/// and these stayed where they were. Measured on what they sit on now,
/// the red came to 1.6:1 on a hearth's panel and 1.7:1 on a menu's,
/// the green to 2.2:1 and 2.4:1 -- so the rack's "rain has stopped it",
/// the kiln's "working", a stopped mod's name and "this cannot be
/// undone" were lines that were on the screen and could not be read.
/// Chat had already found this out and kept a red of its own for it
/// (`chat::REFUSED`); every other screen was still using this one.
///
/// A coral rather than a red, and that is the price rather than a taste:
/// small text at 4.5:1 on the lightest surface it is printed on (a menu
/// row) needs a luminance of about 0.78, and a colour that bright cannot
/// also be a deep red. The hue is kept; the saturation is what gave. See
/// `a_warning_and_an_all_clear_read_on_every_surface_they_are_printed_on`.
pub const TEXT_BAD: [f32; 4] = [1.00, 0.74, 0.64, 1.0];
pub const TEXT_GOOD: [f32; 4] = [0.58, 0.88, 0.52, 1.0];

pub const SCRIM: [f32; 4] = [0.03, 0.03, 0.04, 0.62];

/// **Every size any of the world's screens writes at, and there are
/// five.**
///
/// ## Why a scale at all
///
/// The player's words were "то слишком крупный, то наоборот" -- some of
/// it too big, some of it too small. He was right, and counting made
/// that embarrassing: the pack, the chests, the stations and the journal
/// between them wrote at 0.62, 0.66, 0.68, 0.72, 0.74, 0.75, 0.8, 0.82,
/// 0.85, 0.86, 0.9, 1.0, 1.3 -- thirteen sizes for five jobs, every one
/// of them a number somebody typed while looking at one screen. Two
/// captions over two grids on the same panel were 0.80 and 0.62, and a
/// wound line was whatever `fitted_scale` left of 0.74 after the
/// sentence, so a page of wounds was a page in four sizes.
///
/// A type scale is not decoration. Its whole job is that the *same kind
/// of thing* is the same size wherever it is drawn, so a player can tell
/// what a line is by looking at it rather than by reading it.
///
/// ## Why these five and not more
///
/// Each one names a job, and two jobs that cannot be told apart in a
/// sentence do not get two sizes. The gaps between them are wide enough
/// to read as different -- a tenth is not a size, it is a wobble, and
/// the old list was mostly wobbles.
///
/// ## What is deliberately not here
///
/// The HUD (`hud::NOTICE_SCALE` and its neighbours) and the menu write
/// at their own sizes and always have. They are different surfaces: the
/// HUD is read over a moving world at arm's length and the menu is the
/// only screen with no world behind it. What this scale governs is the
/// stone screens -- the pack, the containers, the stations, the journal
/// -- which are one visual thing and were drawn as several.
///
/// Fitting is still allowed and still right: `fitted_scale` takes one of
/// these as its ceiling and comes down only when a word genuinely will
/// not fit a column. What is not allowed is *starting* anywhere else.
pub mod size {
    /// The one word naming a screen or a pane: `INVENTORY`, `BRONZE
    /// AGE`. There is one of these on a screen, and it is the only text
    /// bigger than the writing under it.
    pub const TITLE: f32 = 1.30;

    /// **The default.** Anything a player reads as language: an item's
    /// name, a row of a list, a sentence about a wound, the line saying
    /// what the pack weighs. If you are unsure which size something is,
    /// it is this one.
    pub const BODY: f32 = 0.90;

    /// The small word naming a group of slots -- `WORN`, `WOUNDS`,
    /// `CRAFTING`, `STORED`. Smaller than body text on purpose: it is a
    /// label on a thing rather than something to read, and it is set in
    /// the quiet ink.
    ///
    /// 0.80 is the largest that still hangs inside the band the pack
    /// leaves over its trays.
    pub const CAPTION: f32 = 0.80;

    /// A second line: a status, a unit, a reason, a tooltip, the
    /// `1-20 of 23` beside a caption. Always subordinate to something
    /// else on the same screen, and never the only thing said.
    pub const NOTE: f32 = 0.72;

    /// The number stamped in the corner of a slot.
    ///
    /// **Sized so three digits fit and are never shrunk.** It was 0.72
    /// with "if it does not fit, multiply by 0.8", which meant `128`
    /// was drawn visibly smaller than `12` in the square beside it --
    /// the single most visible case of the complaint this scale is the
    /// answer to. The largest stack in the game is 128 and a jug holds
    /// under a thousand units, so three digits is the whole of what has
    /// to fit; `a_three_digit_count_fits_a_slot_without_being_shrunk`
    /// is what holds this number down.
    pub const COUNT: f32 = 0.70;
}

/// How far the baseline sits below the `top` [`Painter::text`] is given.
///
/// What two labels of *different sizes on one row* have to agree on. A
/// glyph hangs from `top`, so sharing a `top` lines up the cap tops and
/// leaves the baselines a couple of pixels apart -- which is exactly
/// what a caption and the small count beside it looked like. Sharing a
/// baseline is what the eye reads as "one row".
pub const fn cap_height(scale: f32) -> f32 {
    PIXEL * scale * CAP_HEIGHT as f32
}

/// Where a caption over a row starts: its own height and a hair above the
/// row's top edge, because `Painter::text` hangs the glyphs down from `top`.
///
/// One answer for every stone screen. Each had its own -- four hundredths
/// and a bit on the pack, five on a chest -- so the same word sat at three
/// distances from the same kind of row.
pub fn caption_top_over(row_top: f32, scale: f32) -> f32 {
    row_top + cell_height(scale) + 0.006
}

/// Rasterises UI geometry into a PNG, for looking at a screen without
/// starting the game.
///
/// Test-only, and deliberately crude: it fills each quad's bounding box,
/// which is exact because every quad this UI emits is an axis-aligned
/// rectangle. Glyph quads are drawn as their actual 6x9 bitmaps -- text
/// as grey boxes would hide exactly the mistakes worth looking for
/// (labels overlapping, a number in the wrong corner) -- and block icons
/// come out as flat squares, since their textures need a GPU to load.
#[cfg(test)]
pub fn dump_to_png(vertices: &[HotbarVertex], width: u32, height: u32, path: &str) {
    use std::collections::HashMap;

    // Layer back to character, so glyph quads can be drawn as the
    // glyphs they stand for rather than as grey boxes.
    let font = crate::engine::texture::FontAtlas::for_test();
    let glyphs: HashMap<u32, char> = (0x21u8..=0x7e)
        .map(|b| (font.place(b as char).0, b as char))
        .collect();

    let aspect = width as f32 / height as f32;
    let to_pixels = |p: [f32; 2]| {
        (
            ((p[0] / aspect + 1.0) * 0.5 * width as f32).round() as i64,
            ((1.0 - p[1]) * 0.5 * height as f32).round() as i64,
        )
    };

    let skin = skin();
    let mut pixels = vec![[26u8, 30, 38]; (width * height) as usize];
    let mut blend = |x: i64, y: i64, colour: [f32; 4]| {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            return;
        }
        let index = (y as u32 * width + x as u32) as usize;
        let a = colour[3].clamp(0.0, 1.0);
        for channel in 0..3 {
            let over = colour[channel] * 255.0;
            let under = pixels[index][channel] as f32;
            pixels[index][channel] = (over * a + under * (1.0 - a)) as u8;
        }
    };

    for quad in vertices.chunks(6) {
        if quad.len() < 6 {
            break;
        }
        let (mut x0, mut y0) = (i64::MAX, i64::MAX);
        let (mut x1, mut y1) = (i64::MIN, i64::MIN);
        for v in quad {
            let (x, y) = to_pixels(v.position);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let colour = quad[0].tint;
        // A piece of the skin comes out as the surface colour it stands
        // for, with the gain taken back off. This dump is for reading
        // layouts in a terminal -- `ui::snapshot` is the harness that
        // draws the pictures themselves -- and a panel drawn here as a
        // block icon's grey plate would hide the very thing a layout
        // dump is taken to check.
        if skin.piece_of(quad[0].tex_layer).is_some() {
            let flat = [
                colour[0] / SKIN_GAIN,
                colour[1] / SKIN_GAIN,
                colour[2] / SKIN_GAIN,
                colour[3],
            ];
            for y in y0..y1 {
                for x in x0..x1 {
                    blend(x, y, flat);
                }
            }
            continue;
        }
        match glyphs.get(&quad[0].tex_layer) {
            Some(&c) => {
                let rows = crate::engine::font::glyph(c);
                let cell_w = (x1 - x0).max(1) as f32 / crate::engine::font::GLYPH_WIDTH as f32;
                let cell_h = (y1 - y0).max(1) as f32 / crate::engine::font::GLYPH_HEIGHT as f32;
                for (row, bits) in rows.iter().enumerate() {
                    for column in 0..crate::engine::font::GLYPH_WIDTH {
                        if bits & (0b100000 >> column) == 0 {
                            continue;
                        }
                        let px0 = x0 + (column as f32 * cell_w) as i64;
                        let py0 = y0 + (row as f32 * cell_h) as i64;
                        for y in py0..=(py0 + cell_h.ceil() as i64 - 1).max(py0) {
                            for x in px0..=(px0 + cell_w.ceil() as i64 - 1).max(px0) {
                                blend(x, y, colour);
                            }
                        }
                    }
                }
            }
            None => {
                let flat = quad[0].tex_layer == UNTEXTURED;
                // **The stand-in is tinted, because the shader tints.**
                // A block icon needs a GPU to load, so it comes out here
                // as a flat square standing for the picture -- and the
                // square used to be one fixed blue-grey whatever tint the
                // quad carried. That made every garment the same colour
                // as every stone (see `types::garment_tint`, which is how
                // twelve garments share four pictures), and it made the
                // faint ghost in an empty body square look like a solid
                // item lying in it: the one thing a picture of that
                // screen is taken to check.
                let fill = if flat {
                    colour
                } else {
                    [
                        0.40 * colour[0],
                        0.44 * colour[1],
                        0.52 * colour[2],
                        colour[3],
                    ]
                };
                for y in y0..y1 {
                    for x in x0..x1 {
                        blend(x, y, fill);
                    }
                }
            }
        }
    }

    let mut out = image::RgbImage::new(width, height);
    for (index, p) in pixels.iter().enumerate() {
        out.put_pixel(index as u32 % width, index as u32 / width, image::Rgb(*p));
    }
    out.save(path).expect("could not write the dump");
}

/// An axis-aligned rectangle in UI coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// A rectangle of the given size centred on `(cx, cy)`.
    pub fn centred(cx: f32, cy: f32, width: f32, height: f32) -> Self {
        Self::new(
            cx - width / 2.0,
            cy - height / 2.0,
            cx + width / 2.0,
            cy + height / 2.0,
        )
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    pub fn centre_x(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    pub fn centre_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }
}

/// Turns a physical cursor position into UI coordinates.
///
/// This has to be the exact inverse of the vertex shader's aspect divide.
/// When it isn't, the mouse works at one window size and is subtly offset
/// at every other -- which is a miserable thing to debug, so it lives
/// here next to the coordinate documentation and is covered by a test.
/// **The exact inverse of what the shader does**, and it has to be: the
/// vertex shader multiplies every interface position by the scale, so a
/// button that was authored at 0.4 is drawn at 0.8, and a click has to
/// be divided by the same number to land back on the button rather than
/// half a screen away from it. Two numbers that must agree, named in
/// both places -- see `hotbar.wgsl`.
pub fn cursor_to_ui(cursor: (f64, f64), size: (u32, u32), ui_scale: f32) -> (f32, f32) {
    let width = size.0.max(1) as f32;
    let height = size.1.max(1) as f32;
    let aspect = width / height;
    let scale = if ui_scale.is_finite() && ui_scale > 0.0 {
        ui_scale
    } else {
        1.0
    };
    let ndc_x = (cursor.0 as f32 / width) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor.1 as f32 / height) * 2.0;
    (ndc_x * aspect / scale, ndc_y / scale)
}

/// Back the other way: from the space the interface is authored in to
/// the pixel the pointer would be at.
///
/// **The exact inverse of [`cursor_to_ui`]**, and there is a test that
/// round-trips the pair. It exists for the one screen that has to work
/// in both: the thumb-control arrangement editor draws controls whose
/// size and position are decided in *pixels* -- a finger is a physical
/// thing -- while everything it draws them with, and the cursor it
/// drags them by, is in interface space. A conversion that is only
/// nearly right there is a button that creeps away from the finger.
pub fn ui_to_cursor(at: (f32, f32), size: (u32, u32), ui_scale: f32) -> (f32, f32) {
    let width = size.0.max(1) as f32;
    let height = size.1.max(1) as f32;
    let aspect = width / height;
    let scale = if ui_scale.is_finite() && ui_scale > 0.0 {
        ui_scale
    } else {
        1.0
    };
    let ndc_x = at.0 * scale / aspect;
    let ndc_y = at.1 * scale;
    (((ndc_x + 1.0) / 2.0) * width, ((1.0 - ndc_y) / 2.0) * height)
}

/// How much of the screen's shorter side one finger covers.
///
/// The same argument -- and very nearly the same number -- as the thumb
/// controls make in `platform::touch::Layout`: phones differ by a factor
/// of three in pixel density and by more than that in pixel count, so
/// the only measure of "a finger" that survives the trip from one device
/// to the next is a fraction of the side the hand is wrapped around.
/// Three quarters of a thumb button, which on the screen this was cut
/// for is about nine millimetres.
pub const FINGER: f32 = 0.075;

/// One finger, in the units a centred screen is *authored* in.
///
/// [`Layout::finger`] measures a finger against the window and is
/// therefore a number a layout function cannot have: `sort_button_rect`
/// is called with no window in hand, by drawing code and by hit-testing
/// code alike, and threading a `Layout` into every rectangle on every
/// screen to ask one question is how the two halves of an inverse pair
/// drift apart.
///
/// The arithmetic that makes an authored number safe is short. `finger()`
/// is `FINGER * min(2, 2*aspect)`, and the window is two units tall, so
/// on anything at least as wide as it is tall the floor is exactly
/// `FINGER * 2`. A screen is drawn at [`Layout::fit`], which is
/// `.max(1.0)` -- **it never shrinks a screen**, only grows one -- so a
/// widget authored this tall is drawn at least this tall whatever the
/// interface size is set to. That is the property the alternative did not
/// have: sizing the buttons for the phone-at-1.65 the bug was reported on
/// would have put them back under a finger the moment somebody dragged
/// the setting down.
pub const FINGER_SIDE: f32 = FINGER * 2.0;

/// How big to draw something meant to be pressed.
///
/// `desktop` is the size it has always been where a mouse points at it;
/// where a finger does, it is floored at [`FINGER_SIDE`]. One helper
/// rather than a pair of constants per screen, because the number that
/// matters is the same number everywhere -- and a screen that grew its
/// own touch size by hand is a screen that will be a hundredth out from
/// the next one.
pub fn tappable(desktop: f32) -> f32 {
    tappable_when(desktop, touch_layout())
}

/// Whether the layout should be drawn for a finger.
///
/// [`lang::touch_primary`](crate::ui::lang::touch_primary) with one door
/// left open for the tests, and the door is the whole reason this
/// function exists. That one reads the environment into a `OnceLock`, so
/// a process answers it once and for all -- which means the phone's
/// layout could only be measured by running the suite a second time
/// under a second environment, and a measurement that expensive is one
/// nobody takes. The screens that grew touch sizes would have shipped
/// with nothing checking them.
///
/// A thread-local rather than a global, because `cargo test` runs tests
/// on several threads at once and a global would have one test's pretend
/// phone answering another test's question.
pub fn touch_layout() -> bool {
    #[cfg(test)]
    if let Some(pretend) = PRETEND_TOUCH.with(std::cell::Cell::get) {
        return pretend;
    }
    crate::ui::lang::touch_primary()
}

#[cfg(test)]
thread_local! {
    static PRETEND_TOUCH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Runs `body` with every layout function answering as a phone's would.
///
/// Restored afterwards even if `body` panics, so a failing assertion
/// inside one of these cannot leave the rest of the thread's tests
/// measuring a phone they never asked for.
#[cfg(test)]
pub fn as_a_phone<T>(body: impl FnOnce() -> T) -> T {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PRETEND_TOUCH.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(PRETEND_TOUCH.with(std::cell::Cell::get));
    PRETEND_TOUCH.with(|cell| cell.set(Some(true)));
    body()
}

/// Runs `body` with the layout functions answering for the pointer named.
///
/// The general form of [`as_a_phone`]: a property that has to hold on a
/// desktop *and* on a phone is one loop over `[false, true]` rather than
/// the same assertions written twice.
#[cfg(test)]
pub fn with_touch<T>(touch: bool, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PRETEND_TOUCH.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(PRETEND_TOUCH.with(std::cell::Cell::get));
    PRETEND_TOUCH.with(|cell| cell.set(Some(touch)));
    body()
}

/// The same, told what kind of pointer to answer for.
///
/// **The tests cannot use [`tappable`].** `lang::touch_primary` reads the
/// environment into a `OnceLock`, so a process answers it once; a
/// property that could only be checked by running the suite twice under
/// two environments is a property nobody would ever check. This is the
/// form the assertions call.
pub fn tappable_when(desktop: f32, touch: bool) -> f32 {
    if touch {
        desktop.max(FINGER_SIDE)
    } else {
        desktop
    }
}

/// Air between a screen that has grown and the edge of the window.
///
/// Physical rather than scaled: it is a margin, and a margin that grows
/// with the interface is a margin eating the room the interface wanted.
const SCREEN_MARGIN: f32 = 0.06;

/// How far past its authored size a fixed-shape screen grows on its own,
/// before the player has asked for anything.
///
/// **1.3, and it used to be 1.6 -- chosen per screen.** At 1.6 the pack
/// filled 1136x496 of a 1280x720 window, and every screen took that much
/// *of its own room*: the pack grew by 1.6, the chest ran out of height
/// at 1.34 and stopped, the anvil's list grew by 1.6 from a smaller
/// start. So the same square was 55 pixels in the pack and 44 in the
/// chest beside it, and on a phone the pack was drawn at 2.1 against the
/// chest's 1.34 -- the player's words were "the inventory looks strange,
/// it is huge relative to the other screens", and it was: its body text
/// was a size and a half of the settings screen's. Now every screen
/// grows by *one* number ([`screen_growth`]), and this is how much of
/// it there is before the setting is touched: at 1.3 the pack comes to
/// about 920 pixels of a 1280 window, the chest's slots are the pack's
/// slots, and the writing is within a third of the menus' rather than
/// half again. The cap still matters most for the *small* screens: the
/// death notice and the anvil's job list have room to triple, and a
/// two-line notice drawn the height of a monitor is a billboard rather
/// than an interface. Everything here is capped again by the glass in
/// [`Layout::fit_wanted`], so this number can only ever make a screen
/// smaller than the window, never larger.
///
/// Rejected: growing to a fixed *fraction* of the window. That reads
/// well for the pack and badly for everything narrow -- a confirm
/// dialogue stretched to 80% of a 22:9 phone is a sentence with a metre
/// of air either side of it -- because the screens are not the same
/// shape and a fraction has no idea which one it is looking at.
const NATURAL_GROWTH: f32 = 1.3;

/// How much bigger than authored every centred screen is drawn: the pack,
/// a chest, a hearth, a station, the death notice.
///
/// **One number for all of them**, which is the whole of what it is for.
/// Each screen used to ask [`Layout::fit`] about its *own* shape, and the
/// answers differed by up to half: the pack is wide and low, so it had
/// room to spare where the chest, tall and narrow, had run out -- and the
/// pack's slots, its captions and its body text came out bigger than the
/// same slots and the same words one screen over. A player moves between
/// the pack and a chest a hundred times an hour; the squares have to be
/// the same squares.
///
/// The number is what fits the union of the pack and a chest -- the
/// widest and the tallest screen that is opened often -- so neither is
/// ever grown past the glass by it. A screen taller still (a body with
/// its rucksack tabs) takes [`Layout::no_more_than`] its own room, which
/// only ever makes it smaller than the rest, never larger.
///
/// Rejected: sizing everything to the *smallest* of the screens' own
/// answers across every kind. That is the corpse's rucksack page, opened
/// a few times a life, setting the size of the pack every other minute.
pub fn screen_growth(layout: Layout) -> f32 {
    layout.fit(screen_extent())
}

/// What [`screen_growth`] is measured against: the pack's width and a
/// chest's height, whichever of the two is the larger each way.
pub fn screen_extent() -> (f32, f32) {
    let pack = crate::ui::inventory_screen::extent();
    let chest = crate::ui::chest_screen::chest_extent();
    (pack.0.max(chest.0), pack.1.max(chest.1))
}

/// What this window is, and therefore how to lay a screen out on it.
///
/// ## The bug this replaced
///
/// There used to be one `fit_scale` for the whole interface, capping
/// every centred panel at what the *largest* of them could grow to. Its
/// height term was `1.0 / 0.95` -- a number with no screen in it -- so
/// on every device and every window shape the answer was 1.05. A player
/// dragging INTERFACE SIZE from 1.0 to 4.0 watched the reading climb and
/// the interface grow by five percent and stop. The setting was not
/// merely capped, it was inert.
///
/// Two things were wrong with it, and they need different answers:
///
/// * **One cap for every screen.** A confirm dialogue is half a screen
///   tall and could double; it was held to 1.05 because the settings
///   panel could not. So the cap is now per screen: [`fit`](Self::fit)
///   takes the half-extents of the thing being drawn.
/// * **A panel that can only be multiplied.** y is always -1..1, so a
///   panel already reaching 0.95 has five hundredths of a screen to grow
///   into, whatever anybody asks for. That one is not a cap to be
///   loosened -- it is arithmetic. The settings screen answers it by
///   scaling its *content* instead: [`at`](Self::at) makes the rows and
///   the buttons as big as was asked for, the panel stays the size the
///   screen allows, and fewer rows are visible. That is a layout, and it
///   cannot overflow however far the setting is pushed.
///
/// ## The desktop
///
/// At any window with the scale left at 1.0, `at(v) == v`,
/// `fit(_) == 1.0` and `finger() == 0.0` -- so a screen rewritten in
/// terms of a layout draws exactly the geometry it drew before. There is
/// a test that says so, and it is the one that matters: the desktop was
/// not what was broken.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// Half the window's width in UI units: x runs -aspect..aspect.
    aspect: f32,
    /// What the player asked for.
    requested: f32,
    /// Whether a finger, rather than a pointer, is what aims at this.
    touch: bool,
}

impl Layout {
    /// The layout for a window of this shape at this interface size.
    pub fn for_screen(aspect: f32, requested: f32) -> Self {
        Self {
            aspect: if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 },
            requested: if requested.is_finite() && requested > 0.0 {
                requested.clamp(0.5, 4.0)
            } else {
                1.0
            },
            touch: crate::ui::lang::touch_primary(),
        }
    }

    /// A desktop window with the scale left alone: the layout every
    /// helper here is the identity on.
    #[cfg(test)]
    pub fn desktop() -> Self {
        Self::for_screen(16.0 / 9.0, 1.0)
    }

    /// A length the player asked to be bigger: a row, a button, a slot,
    /// a size of writing.
    ///
    /// Anything *not* passed through this is a decision that it should
    /// stay the size it is on a desktop -- a title, a help line, the air
    /// between a panel and the window edge. On a screen with no vertical
    /// room a title scaled half again costs two rows of the thing the
    /// player actually came for.
    pub fn at(&self, desktop: f32) -> f32 {
        desktop * self.requested
    }

    /// How much bigger than the desktop the writing on this screen is.
    ///
    /// What a [`Painter`] is handed so a button drawn twice the height
    /// letters itself twice the size. See `Painter::content`.
    pub fn content(&self) -> f32 {
        self.requested
    }

    /// How wide to draw a centred panel that is `desktop` wide on a
    /// desktop.
    ///
    /// It grows with the interface size and stops at the window, which
    /// is the fix for the settings panel being **a fixed box in the
    /// middle of a window twice as wide as it**: on a 22:9 phone its
    /// rows were squeezed into 2.3 units of a 4.4-unit screen, with the
    /// label, the reading and the buttons fighting over the last
    /// quarter of them.
    ///
    /// It narrows as well as widens -- on a square window the settings
    /// panel was 1.15 wide against a screen 1.0 wide, and the last
    /// column of buttons was simply off the glass. A narrower row is a
    /// row with less space in it, which the labels handle by fitting
    /// themselves; a row past the edge is a row with buttons that
    /// cannot be pressed.
    pub fn panel_half_width(&self, desktop: f32) -> f32 {
        let room = (self.aspect - SCREEN_MARGIN).max(0.2);
        self.at(desktop).min(room)
    }

    /// A layout for a screen whose vertical room is already spoken for.
    ///
    /// **The direction a phone has nothing to spare in.** y is always
    /// -1..1 whatever the window, so a screen that already piles up 1.3
    /// units of furniture at a desktop's size can grow that pile by half
    /// again only by putting a third of it off the glass. `room` is the
    /// band the pile has to fit in and `stack` is what it comes to at a
    /// desktop's size; the answer is a layout whose `at` grows content
    /// by what is left and no further.
    ///
    /// **Width is deliberately not capped by it.** Width is the one
    /// thing a 22:9 screen has going spare, so a screen asks *this* for
    /// its heights and the original for `panel_half_width` -- a create-
    /// world form on a phone wants long fields much more than it wants
    /// tall ones, and the two answers are different numbers.
    pub fn within(&self, room: f32, stack: f32) -> Layout {
        Layout {
            requested: self
                .requested
                .min(room.max(0.001) / stack.max(0.001))
                .max(1.0),
            ..*self
        }
    }

    /// The largest a screen with these half-extents may be drawn at.
    ///
    /// For the screens that are a fixed shape and simply want to be
    /// bigger -- the pack, a chest, the death notice. `extent` is what
    /// the screen occupies about the point it grows from, so the answer
    /// is what fits, and never less than 1.0: a screen too big for the
    /// window at its authored size is clipped either way, and shrinking
    /// it would make it unreadable as well as cut off.
    ///
    /// **The authored size is a floor, not a target** -- rule 1 at the
    /// top of this file, and the reason it is rule 1. This used to
    /// answer `requested`, which is 1.0 until a player goes looking for
    /// INTERFACE SIZE, so the default was *never grow*: at 1280x720 the
    /// pack came out 710x310 pixels in the middle of an empty window and
    /// the anvil's job list 445x380. The room was there the whole time
    /// and nothing asked for it.
    ///
    /// So the room is taken first -- [`NATURAL_GROWTH`] of it, or as
    /// much as there is -- and what the player asked for multiplies
    /// *that*. The setting therefore still moves the interface at every
    /// step (`the_interface_size_setting_actually_changes_the_interface`
    /// is unchanged) and still cannot push a screen off the glass,
    /// because both halves go through [`fit_wanted`](Self::fit_wanted).
    pub fn fit(&self, extent: (f32, f32)) -> f32 {
        let natural = self.fit_wanted(NATURAL_GROWTH, extent);
        self.fit_wanted(self.requested * natural, extent)
    }

    /// `growth`, or less if a screen of this extent would leave the glass
    /// at it -- and never under 1.0.
    ///
    /// What a screen grown by [`screen_growth`] asks about its *own*
    /// shape: the shared number is what fits the pack and a chest, and a
    /// screen taller than both has to stop where its own room runs out.
    pub fn no_more_than(&self, growth: f32, extent: (f32, f32)) -> f32 {
        self.fit_wanted(growth, extent)
    }

    /// The largest a screen with these half-extents can ever be drawn at
    /// on this window, whatever anybody asks for: the glass, less the
    /// margin every screen keeps.
    ///
    /// **What the interface-size setting runs into, and the reason it
    /// has to be askable.** Now that a screen takes the room before the
    /// setting is consulted, a big screen on a small window is already
    /// against this when the player first opens it, and no further
    /// setting can move it. The tests that check the setting still does
    /// something therefore have to be able to tell "this step did
    /// nothing" from "this step did nothing *because there is no room
    /// left*", and those are different bugs.
    ///
    /// `cfg(test)` for the reason [`desktop`](Self::desktop) is: nothing
    /// the game draws needs to ask this, because [`fit`](Self::fit)
    /// already applies it. Left in the build it is one dead-code warning
    /// in a repository whose standard is that there are none.
    #[cfg(test)]
    pub fn ceiling(&self, extent: (f32, f32)) -> f32 {
        self.fit_wanted(f32::INFINITY, extent)
    }

    /// As big as asked, capped by the glass.
    fn fit_wanted(&self, wanted: f32, extent: (f32, f32)) -> f32 {
        let room_x = (self.aspect - SCREEN_MARGIN).max(0.001);
        let room_y = (1.0 - SCREEN_MARGIN).max(0.001);
        wanted
            .min(room_x / extent.0.max(0.001))
            .min(room_y / extent.1.max(0.001))
            .max(1.0)
    }

    /// The largest a screen pinned to a *corner* may be drawn at.
    ///
    /// The same question as [`fit`](Self::fit) with twice the room in
    /// each direction: something growing from the middle runs out of
    /// screen at half the window, and something growing from a corner
    /// has the whole of it.
    ///
    /// **Without the natural growth [`fit`](Self::fit) now applies**, and
    /// the difference is what a corner screen *is*. The only one is the
    /// chat log, which is drawn over a world the player is still looking
    /// at: it is a thing that appears beside the game rather than in
    /// place of it, and a log that helped itself to a third of the glass
    /// the moment somebody said hello would be covering the thing the
    /// message is about. A screen that replaces the world should take
    /// the room; one that sits beside it should take what it was asked
    /// for and no more.
    pub fn fit_from_corner(&self, extent: (f32, f32)) -> f32 {
        self.fit_wanted(self.requested, (extent.0 / 2.0, extent.1 / 2.0))
    }

    /// Where a click landed, in the space a screen of this extent was
    /// authored in.
    ///
    /// **The exact inverse of [`scale_about`]**, and it has to be: the
    /// frame loop multiplies a screen's geometry by `scale` and a click
    /// divided by anything else lands where the button is not. It takes
    /// the factor rather than working it out again from an extent,
    /// because there are two ways to arrive at one -- see
    /// [`fit_touchable`](Self::fit_touchable) -- and re-deriving it here
    /// is how the two answers drift apart. Covered by a test.
    pub fn hit(&self, at: (f32, f32), scale: f32) -> (f32, f32) {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        (at.0 / scale, at.1 / scale)
    }

    /// The smallest a thing worth tapping may be, or zero where the
    /// pointer is a mouse.
    ///
    /// Zero rather than some small number, so that on a desktop this
    /// term drops out of the `max` arithmetic it appears in and cannot
    /// move a pixel of a layout it was never meant to touch.
    ///
    /// **A floor under a widget, never under a whole screen.** The
    /// difference is what makes it safe. A settings row that is a finger
    /// tall costs nothing but a row of the list, so the floor buys
    /// usable buttons for free -- they were forty-nine pixels. A *pack*
    /// floored the same way would have to grow bodily, and the arithmetic
    /// on the screen this was cut for made it grow past everything the
    /// player could ask for below 1.5: the setting moved and the pack
    /// sat still, which is the disease this whole pass was for. Three
    /// pixels of slot are not worth half the range of a setting.
    pub fn finger(&self) -> f32 {
        if !self.touch {
            return 0.0;
        }
        // The shorter side, because that is the one the hand is wrapped
        // around -- see `FINGER`. In UI units the window is 2 tall and
        // 2*aspect wide.
        FINGER * 2.0f32.min(2.0 * self.aspect)
    }

    /// How far from the middle a screen may reach sideways.
    ///
    /// The right-hand edge, less the margin every screen keeps. What
    /// anything laid out about the centre has to check itself against
    /// -- a pair of buttons at the top of the interface-size range is
    /// wider than a 4:3 window.
    pub fn edge(&self) -> f32 {
        (self.aspect - SCREEN_MARGIN).max(0.1)
    }

    /// How far up the glass the on-screen keyboard reaches, or `None`
    /// where there isn't one.
    ///
    /// See [`KEYBOARD_TOP`] for the number and why it is a guess.
    pub fn keyboard_top(&self) -> Option<f32> {
        self.touch.then_some(KEYBOARD_TOP)
    }

    /// Whether this screen is aimed at with a finger.
    pub fn is_touch(&self) -> bool {
        self.touch
    }
}

/// How far up the glass an on-screen keyboard reaches.
///
/// **A guess, and it has to be one.** Android does not tell the
/// interface how tall its keyboard is: `android-activity` 0.5 reports
/// only that the insets *changed*, not what they became, and the
/// manifest's `adjustResize` -- which would have shrunk the surface and
/// made the question moot -- is ignored outright for a fullscreen
/// window, which this game's is.
///
/// So nothing tries to dodge the keyboard by however much. Everything
/// worth typing into is laid out above this line, which is correct
/// whether the keyboard covers the window or shrinks it. The number is
/// what "down here" means: a stock keyboard on a phone held sideways
/// comes to a little under half the glass.
///
/// **Anything that draws a field must check itself against this**, and
/// there are tests that say so. The chat box did not, for a while: it
/// was pinned to the bottom-left corner like the log it belongs to, and
/// on a phone that put the line being typed, the caret, and both of its
/// buttons underneath the keyboard that was typing into it.
pub const KEYBOARD_TOP: f32 = -0.10;

/// Grows a run of finished interface geometry around a fixed point.
///
/// The free form of [`Painter::scale_about`], for the frame loop, which
/// appends every piece of the interface into one list and has no
/// painter in hand between them. See that method for why the origin
/// matters more than the factor.
pub fn scale_about(vertices: &mut [HotbarVertex], origin: (f32, f32), scale: f32) {
    if !scale.is_finite() || (scale - 1.0).abs() < 1e-4 {
        return;
    }
    for vertex in vertices {
        vertex.position[0] = origin.0 + (vertex.position[0] - origin.0) * scale;
        vertex.position[1] = origin.1 + (vertex.position[1] - origin.1) * scale;
    }
}

/// How far apart two colours are, as the ratio a person reading has to
/// live with.
///
/// The WCAG contrast ratio: 1 is two identical colours, 21 is black on
/// white. **Computed on the numbers as written**, which is correct here
/// and would be wrong almost anywhere else: the interface is drawn into
/// an sRGB surface, so what the shader is handed is already linear
/// light, and relative luminance of a grey in linear light is the grey
/// itself. Colours are weighted the way the standard weights them --
/// green carries most of the brightness a human eye reports, blue
/// almost none.
///
/// This exists because "does this read" had been a matter of opinion,
/// and opinions were being formed on a bright desk. See
/// `small_text_is_readable_against_everything_it_is_drawn_on`.
///
/// Only the tests measure; nothing in the game asks at run time, so it
/// is not compiled into the game. It stays here rather than in the test
/// module because it belongs beside the palette it judges -- somebody
/// adding a colour should meet this function on the way past.
#[cfg(test)]
pub fn contrast(a: [f32; 4], b: [f32; 4]) -> f32 {
    let luminance = |c: [f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// `top` painted over `bottom`, the way the blender does it.
///
/// [`contrast`] answers what a person sees, and what a person sees is
/// never the colour a constant holds: every control over the world is
/// translucent, so the colour that reaches the eye is a mixture with
/// whatever is behind it. Measuring the constants alone says a thumb
/// button's letters are near-white on near-black -- a fine reading of
/// two numbers that are never both on screen.
///
/// The result is opaque: it is a colour that has already been shown,
/// not one still waiting to be mixed.
///
/// See `a_thumb_button_can_be_read_against_the_world_behind_it`.
#[cfg(test)]
pub fn over(top: [f32; 4], bottom: [f32; 4]) -> [f32; 4] {
    let a = top[3];
    [
        top[0] * a + bottom[0] * (1.0 - a),
        top[1] * a + bottom[1] * (1.0 - a),
        top[2] * a + bottom[2] * (1.0 - a),
        1.0,
    ]
}

/// The largest text scale at which `text` fits inside `rect`.
///
/// Written because a label on a thumb button is not a label of a known
/// length any more: a button carries a key, and the key's name is `G`
/// or `SPACE` or `L SHIFT`. Sized by a constant, the short ones look
/// right and the long ones run out over the world; sized by dividing a
/// constant by the letter count, the long ones fit and the short ones
/// shrink for no reason.
///
/// So: as big as it can be, bounded by both sides of the box. `fill` is
/// how much of the box the writing is allowed to take -- never all of
/// it, or the letters touch the border and the button stops reading as
/// a button.
pub fn scale_to_fit(rect: Rect, text: &str, fill: f32) -> f32 {
    let letters = text_width(text).max(1) as f32;
    let by_width = rect.width().abs() * fill / (PIXEL * letters);
    let by_height = rect.height().abs() * fill / (PIXEL * GLYPH_HEIGHT as f32);
    by_width.min(by_height).max(0.0)
}

/// Slides a run of finished interface geometry straight up the glass.
///
/// **The one thing `scale_about` cannot express.** Growing a widget
/// about the corner it is pinned to keeps it in that corner, which is
/// the whole point -- and it is exactly wrong for a widget that has to
/// clear something the corner does not know about. The chat box is
/// pinned bottom-left and has to stand above the on-screen keyboard;
/// under a scale of less than one, a box authored above
/// [`KEYBOARD_TOP`] is pulled back down under it, because everything is
/// pulled back toward the corner.
///
/// So the lift is applied to the grown geometry, after the growth, and
/// whatever inverts the growth has to subtract it first. See
/// `chat::keyboard_lift`, which both sides call so there is only one
/// number.
pub fn lift(vertices: &mut [HotbarVertex], dy: f32) {
    if !dy.is_finite() || dy == 0.0 {
        return;
    }
    for v in vertices {
        v.position[1] += dy;
    }
}

/// Where a point on the glass was before [`scale_about`] moved the
/// interface out from under it.
///
/// **The exact inverse of `scale_about`**, and the thing a tap on the
/// HUD has to go through. The hotbar is authored at a fixed size and
/// then grown about the bottom of the screen; a finger lands in the
/// *grown* picture, and asking `hotbar::slot_at` about that point
/// straight away asks it about the layout as it would have been at
/// scale one. At an interface size of two, that is a bar half as wide
/// as the one being tapped, and every slot but the middle answers
/// wrongly.
///
/// There is a test that composes the two and gets the identity back.
pub fn unscale_about(point: (f32, f32), origin: (f32, f32), scale: f32) -> (f32, f32) {
    if !scale.is_finite() || scale <= 0.0 {
        return point;
    }
    (
        origin.0 + (point.0 - origin.0) / scale,
        origin.1 + (point.1 - origin.1) / scale,
    )
}

/// Where a piece of the interface is pinned, and therefore the point it
/// grows around.
///
/// Named rather than passed as a bare pair, because every one of these
/// is a decision about what stays put when the interface gets bigger,
/// and a wrong pair is a piece of the interface that walks off the
/// screen. `aspect` is the half-width of the interface space -- the
/// screen's right edge.
pub mod anchor {
    /// The bottom of the screen: the hotbar and everything hanging off
    /// it. Grows upward.
    pub const BOTTOM: fn(f32) -> (f32, f32) = |_aspect| (0.0, -1.0);
    /// The middle: every screen a player opens. Grows outward, which is
    /// what a dialogue should do.
    pub const CENTRE: fn(f32) -> (f32, f32) = |_aspect| (0.0, 0.0);
    /// The bottom-left corner: the chat log, which fills upward from
    /// where the box is typed.
    pub const BOTTOM_LEFT: fn(f32) -> (f32, f32) = |aspect| (-aspect, -1.0);
    /// The top of the screen: the sail's dial, which hangs from it and
    /// grows downward. Anything pinned to an edge has to grow *away* from
    /// that edge, or making the interface larger pushes it off the screen.
    pub const TOP: fn(f32) -> (f32, f32) = |_aspect| (0.0, 1.0);
}

/// Whether a pressable thing is being pointed at or pressed.
///
/// Three states and not two, because a phone has no pointer: a thumb
/// button is never hovered and always either idle or held, and a
/// desktop button is never held long enough to matter but is hovered
/// constantly. One enum covers both, so a screen does not have to know
/// which kind of glass it is on to draw a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Idle,
    Hovered,
    Held,
}

/// How big a settings row's furniture is.
///
/// **Passed rather than worked out from the row's own height**, which is
/// what it was. A row used to letter itself in proportion to how tall it
/// was, on the reasoning that a screen with enough settings on it to
/// squeeze the rows should not have labels standing taller than the bars
/// they sit in. That reasoning is sound downward and wrong upward: on a
/// phone a row is as tall as the *finger* that has to hit it, which is
/// nearly twice the height the writing wants, and deriving one from the
/// other made a touch-sized row shout.
///
/// So the two are separate now. The row is as tall as the finger; the
/// writing on it is as big as the player asked for.
#[derive(Debug, Clone, Copy)]
pub struct RowStyle {
    /// How big the label is written.
    pub text: f32,
    /// How big the reading is, against the label.
    ///
    /// One caller uses anything but 1.0: a switch, whose reading is a
    /// word (`ON`/`OFF`, and in Russian `ВКЛ`/`ВЫКЛ`) rather than a
    /// number with a unit after it. A word set at the label's size reads
    /// as a second label competing with the first, and the Russian one
    /// is wide enough to crowd the button beside it; a number never is.
    pub value: f32,
    /// How much of the row, measured from its right edge, the buttons
    /// own -- and therefore what everything else on the row has to keep
    /// clear of.
    ///
    /// It was a constant in this module and a second constant in the
    /// settings screen, four hundred lines apart, and the reading was
    /// right-aligned against the wrong one of them. Now there is one
    /// number and the screen that draws the buttons is the one that says
    /// how wide they are -- which it has to be, because on a phone they
    /// are as wide as a finger and on a desktop they are not.
    pub controls: f32,
}

/// Collects UI geometry.
pub struct Painter {
    pub vertices: Vec<HotbarVertex>,
    font: crate::engine::texture::FontAtlas,
    /// Which skin the shared widgets draw in. See `Theme`.
    theme: Theme,
    /// Which pictures the panels, cells and boards are drawn out of.
    ///
    /// Taken once, when the painter is made, rather than looked up per
    /// quad: it is an atomic read, and a screen draws a few thousand
    /// quads.
    skin: Skin,
    /// How big the writing on a full-size widget is, against the
    /// desktop's one.
    ///
    /// **Only ever a ceiling.** `button` and `field` letter themselves
    /// in proportion to their own height so a short widget does not
    /// shout, and that proportion was capped at 1.0 -- the size the
    /// design was drawn at. On a phone every button is half again
    /// taller and every one of them was still lettered for a desktop,
    /// which is a bigger button with the same small word on it. This
    /// raises the ceiling and nothing else: at 1.0 the arithmetic is
    /// what it always was. See `Layout::content`.
    content: f32,
}

/// One line of text a painter drew, with the box it occupies.
///
/// **Test-only, and it exists because the overlaps were found by a
/// person looking at PNGs.** A caption written across the slots it
/// names, or a count and a heading sharing a row, is invisible to every
/// test this interface had: the vertex list says a glyph is at a
/// coordinate, not which line it belongs to, and a test cannot tell a
/// caption's `C` from a count's `1`. Recording the *call* gives a test
/// the one thing it needs, which is a box round each line.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub rect: Rect,
    pub text: String,
    pub scale: f32,
}

#[cfg(test)]
thread_local! {
    static WRITTEN: std::cell::RefCell<Option<Vec<Written>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `body` and answers every line of text drawn inside it.
///
/// **A recorder rather than a field on the painter**, because a screen
/// is not built through one painter that the caller keeps: `build_into`
/// takes a vertex list, makes a painter, and hands the list back. A
/// field would have needed every one of those signatures to grow a way
/// of getting it out again. A thread-local costs nothing in the game --
/// it is not compiled into it -- and works through any entry point.
///
/// A thread-local and not a global, for the reason `PRETEND_TOUCH` is
/// one: `cargo test` runs several tests at once and a global would have
/// one test recording another test's screen.
#[cfg(test)]
pub fn while_recording_text<T>(body: impl FnOnce() -> T) -> (T, Vec<Written>) {
    struct Restore(Option<Vec<Written>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            WRITTEN.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let restore = Restore(WRITTEN.with(|cell| cell.borrow_mut().replace(Vec::new())));
    let answer = body();
    let lines = WRITTEN.with(|cell| cell.borrow().clone().unwrap_or_default());
    drop(restore);
    (answer, lines)
}

impl Default for Painter {
    fn default() -> Self {
        Self::new(crate::engine::texture::FontAtlas::for_test())
    }
}

impl Painter {
    pub fn new(font: crate::engine::texture::FontAtlas) -> Self {
        Self {
            vertices: Vec::new(),
            font,
            theme: Theme::STONE,
            skin: skin(),
            content: 1.0,
        }
    }

    /// A painter that appends to a list the caller already owns.
    ///
    /// The interface is rebuilt into one persistent vertex list rather
    /// than a fresh `Vec` per widget per frame -- see the UI block in
    /// `main` -- and this is how a builder gets hold of it: take the
    /// list with `mem::take`, paint onto it, hand it back with
    /// `into_vertices`. The capacity survives the round trip, which is
    /// the point.
    pub fn onto(
        font: crate::engine::texture::FontAtlas,
        vertices: Vec<HotbarVertex>,
    ) -> Self {
        Self {
            vertices,
            font,
            theme: Theme::STONE,
            skin: skin(),
            content: 1.0,
        }
    }

    /// The same, in the menu's skin. See `Theme`.
    pub fn onto_themed(
        font: crate::engine::texture::FontAtlas,
        vertices: Vec<HotbarVertex>,
        theme: Theme,
    ) -> Self {
        Self {
            vertices,
            font,
            theme,
            skin: skin(),
            content: 1.0,
        }
    }

    /// The same, lettering its widgets for a screen this much bigger
    /// than the desktop. See the `content` field.
    pub fn with_content(mut self, content: f32) -> Self {
        if content.is_finite() && content >= 1.0 {
            self.content = content;
        }
        self
    }

    /// What this painter is drawing with, for a screen that needs a
    /// colour the widgets do not offer.
    #[allow(dead_code)]
    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn into_vertices(self) -> Vec<HotbarVertex> {
        self.vertices
    }

    pub fn quad(&mut self, rect: Rect, colour: [f32; 4]) {
        for position in [
            [rect.x0, rect.y0],
            [rect.x1, rect.y0],
            [rect.x1, rect.y1],
            [rect.x0, rect.y0],
            [rect.x1, rect.y1],
            [rect.x0, rect.y1],
        ] {
            self.vertices.push(HotbarVertex {
                position,
                uv: [0.0, 0.0],
                tex_layer: UNTEXTURED,
                tint: colour,
            });
        }
    }

    /// A rectangle whose colour changes from top to bottom.
    ///
    /// The UI vertex carries its own tint and the hardware interpolates
    /// it across the triangle, so a gradient costs exactly what a flat
    /// quad costs -- the same six vertices, with two of the four corners
    /// given a different colour. That is worth knowing, because it is
    /// the difference between a panel that looks like a rectangle of
    /// paint and one that looks like a surface with light falling on it,
    /// and it is free.
    /// Two colours down a quad.
    ///
    /// Nothing draws one any more -- the panels are stone with a bevel
    /// rather than glass with a gradient -- but it is four lines and the
    /// next thing that wants a sky or a bar will want it.
    #[allow(dead_code)]
    pub fn vgradient(&mut self, rect: Rect, top: [f32; 4], bottom: [f32; 4]) {
        for (position, colour) in [
            ([rect.x0, rect.y0], bottom),
            ([rect.x1, rect.y0], bottom),
            ([rect.x1, rect.y1], top),
            ([rect.x0, rect.y0], bottom),
            ([rect.x1, rect.y1], top),
            ([rect.x0, rect.y1], top),
        ] {
            self.vertices.push(HotbarVertex {
                position,
                uv: [0.0, 0.0],
                tex_layer: UNTEXTURED,
                tint: colour,
            });
        }
    }

    /// One rectangle of a picture onto one rectangle of the screen.
    ///
    /// `v` runs down the picture and `y` runs up the screen, so the top
    /// edge of the quad takes the *smaller* v. Getting that backwards
    /// draws the whole interface upside down -- the same trap the block
    /// face UVs carry, and the reason `hotbar::push_quad` says so too.
    fn picture(&mut self, rect: Rect, layer: u32, (u0, v0, u1, v1): (f32, f32, f32, f32), tint: [f32; 4]) {
        for (position, uv) in [
            ([rect.x0, rect.y0], [u0, v1]),
            ([rect.x1, rect.y0], [u1, v1]),
            ([rect.x1, rect.y1], [u1, v0]),
            ([rect.x0, rect.y0], [u0, v1]),
            ([rect.x1, rect.y1], [u1, v0]),
            ([rect.x0, rect.y1], [u0, v0]),
        ] {
            self.vertices.push(HotbarVertex {
                position,
                uv,
                tex_layer: layer,
                tint,
            });
        }
    }

    /// A whole picture stretched over a rectangle.
    ///
    /// What a cell is drawn with: a slot is always about square and
    /// always about one size, so slicing it would be five times the
    /// quads for a difference nobody can see at eighty thousandths of a
    /// screen.
    ///
    /// Answers whether it drew anything, so a caller can fall back to
    /// the flat fill this interface had before it had pictures.
    #[must_use]
    pub fn stretched(&mut self, rect: Rect, piece: Piece, colour: [f32; 4]) -> bool {
        let Some(layer) = self.skin.layer(piece) else {
            return false;
        };
        self.picture(rect, layer, (0.0, 0.0, 1.0, 1.0), skin_tint(colour));
        true
    }

    /// A picture in nine pieces: four corners at a fixed size, four
    /// edges run along their sides, and a field in the middle.
    ///
    /// **The frame never eats more than a quarter of the rectangle.** A
    /// settings row is a third the height of the border it would like,
    /// and a widget whose frame meets in the middle is not a framed
    /// thing, it is a lip drawn twice with a scar down it. A quarter
    /// also leaves half the widget as field, which is what makes a
    /// short row still read as a row rather than as a bead.
    ///
    /// `tile` is the side of one repeat of the middle and of the edges.
    /// `None` stretches them, which is right for a board or a cell-sized
    /// hollow and wrong for a panel -- see [`FIELD_TILE`].
    #[must_use]
    pub fn nine(&mut self, rect: Rect, piece: Piece, colour: [f32; 4], tile: Option<f32>) -> bool {
        let Some(layer) = self.skin.layer(piece) else {
            return false;
        };
        let inset = piece.border_texels() / SKIN_RESOLUTION;
        let border = piece
            .screen_border()
            .min(rect.width() * 0.25)
            .min(rect.height() * 0.25)
            .max(0.0);
        let tint = skin_tint(colour);
        let xs = [rect.x0, rect.x0 + border, rect.x1 - border, rect.x1];
        let ys = [rect.y0, rect.y0 + border, rect.y1 - border, rect.y1];
        let us = [0.0, inset, 1.0 - inset, 1.0];
        // v is measured down the picture while y is measured up the
        // screen, so the bottom of the rectangle takes the bottom of the
        // picture, which is v = 1.
        let vs = [1.0, 1.0 - inset, inset, 0.0];
        for column in 0..3 {
            let across = spans(
                xs[column],
                xs[column + 1],
                us[column],
                us[column + 1],
                if column == 1 { tile } else { None },
            );
            for row in 0..3 {
                let down = spans(
                    ys[row],
                    ys[row + 1],
                    vs[row],
                    vs[row + 1],
                    if row == 1 { tile } else { None },
                );
                for &(x0, x1, u0, u1) in &across {
                    for &(y0, y1, v1, v0) in &down {
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }
                        self.picture(Rect::new(x0, y0, x1, y1), layer, (u0, v0, u1, v1), tint);
                    }
                }
            }
        }
        true
    }

    /// A hairline divider across a panel: a groove with a lit line under
    /// it, which is what an edge with a thickness looks like.
    ///
    /// **Only four rows of the picture are drawn.** Every layer in the
    /// array is thirty-two texels square, and a divider is four of them
    /// tall; sampling the whole layer would squeeze thirty-two rows into
    /// four and leave half a texel of line. The other rows are empty, so
    /// they are simply not asked for.
    ///
    /// Nothing in the picture varies along x, so stretching it sideways
    /// across a panel cannot smear anything -- which is why this is one
    /// quad rather than a nine-slice.
    pub fn rule(&mut self, x0: f32, x1: f32, y: f32, colour: [f32; 4]) {
        let half = 2.0 * SKIN_TEXEL;
        let rect = Rect::new(x0, y - half, x1, y + half);
        match self.skin.layer(Piece::Rule) {
            Some(layer) => {
                let (top, bottom) = (14.0 / SKIN_RESOLUTION, 18.0 / SKIN_RESOLUTION);
                self.picture(rect, layer, (0.0, top, 1.0, bottom), skin_tint(colour));
            }
            None => self.quad(
                Rect::new(x0, y - SKIN_TEXEL * 0.5, x1, y + SKIN_TEXEL * 0.5),
                colour,
            ),
        }
    }

    /// A hollow rectangle: four thin quads rather than a filled one
    /// behind the content, so a border can sit over any background.
    pub fn border(&mut self, rect: Rect, thickness: f32, colour: [f32; 4]) {
        let t = thickness;
        self.quad(Rect::new(rect.x0 - t, rect.y1, rect.x1 + t, rect.y1 + t), colour);
        self.quad(Rect::new(rect.x0 - t, rect.y0 - t, rect.x1 + t, rect.y0), colour);
        self.quad(Rect::new(rect.x0 - t, rect.y0, rect.x0, rect.y1), colour);
        self.quad(Rect::new(rect.x1, rect.y0, rect.x1 + t, rect.y1), colour);
    }

    /// A frame of two hairlines, one dark and one pale.
    ///
    /// ## Why two lines and not one
    ///
    /// Because a single line has one brightness and the world behind it
    /// has all of them. Measured with [`contrast`] on the composite: a
    /// pale hairline over snow comes to **1.02:1** -- it is not a faint
    /// line there, it is no line at all -- and a dark one disappears
    /// just as completely against a lit cave floor.
    ///
    /// A dark line and a pale line side by side cannot both vanish into
    /// the same colour, because they are at opposite ends of the range
    /// the background has to sit somewhere inside. Whatever is behind
    /// them, one of the two is showing; the pair reads as an edge even
    /// when only half of it is doing the work.
    ///
    /// The dark line goes *outside* the pale one. Terrain is more often
    /// bright than black -- sky, sand, snow, grass in daylight -- so the
    /// line that meets the world first should be the one that survives
    /// brightness, and the pale line then has a dark ground of its own
    /// to sit against.
    ///
    /// This is what the thumb controls are drawn out of. It replaces a
    /// filled slab, which was legible and cost a piece of the world for
    /// every button; see `hud::touch_controls`.
    pub fn hairline_frame(
        &mut self,
        rect: Rect,
        thickness: f32,
        dark: [f32; 4],
        light: [f32; 4],
    ) {
        // Outward from the rectangle, so the box a player is told to aim
        // at is the box that was hit-tested -- the frame is drawn around
        // it rather than eating into it.
        self.border(Rect::new(
            rect.x0 - thickness,
            rect.y0 - thickness,
            rect.x1 + thickness,
            rect.y1 + thickness,
        ), thickness, dark);
        self.border(rect, thickness, light);
    }

    /// Covers the whole screen, whatever its shape.
    ///
    /// Authored deliberately far outside the visible range: the aspect
    /// divide shrinks x, so a quad that merely reached ±1 would leave
    /// bare strips down the sides of a wide window.
    pub fn scrim(&mut self, colour: [f32; 4]) {
        self.quad(Rect::new(-8.0, -8.0, 8.0, 8.0), colour);
    }

    /// A list row: a name, and a smaller detail line under it.
    ///
    /// Stacked by the font's own metrics and centred as a block. The
    /// obvious-looking alternative -- centre the name in the top half of
    /// the row and the detail in the bottom half -- is what this did
    /// before, and it only worked while the font had no descenders. The
    /// cell is now two rows taller than the cap height, so the name no
    /// longer fitted in its half and hung out of the top of the row.
    ///
    /// Both lines are shortened to fit the row's width, so a long name
    /// ends in `..` inside the panel rather than running out of it.
    ///
    /// `content` is how big the writing on this screen is against the
    /// desktop's -- see `Layout::content`. Passed rather than derived
    /// from the row's height, for the reason `RowStyle` gives: on a
    /// phone a row is as tall as a finger, which is not how tall the
    /// writing wants to be.
    #[allow(clippy::too_many_arguments)] // a row, two strings, two colours and a size
    pub fn row_labels(
        &mut self,
        rect: Rect,
        pad: f32,
        name: &str,
        name_colour: [f32; 4],
        detail: &str,
        detail_colour: [f32; 4],
        content: f32,
    ) {
        let name_scale = content;
        let detail_scale = content * 0.8;
        let gap = 0.005 * content;

        let name_cell = cell_height(name_scale);
        let detail_cell = cell_height(detail_scale);
        let block = name_cell + gap + detail_cell;

        // Top of the block, centred vertically in the row.
        let top = rect.centre_y() + block / 2.0;
        let available = (rect.x1 - rect.x0 - pad * 2.0).max(0.0);

        self.text(
            &fit(name, name_scale, available),
            rect.x0 + pad,
            top,
            name_scale,
            name_colour,
        );
        self.text(
            &fit(detail, detail_scale, available),
            rect.x0 + pad,
            top - name_cell - gap,
            detail_scale,
            detail_colour,
        );
    }

    pub fn panel(&mut self, rect: Rect) {
        self.slab(rect, self.theme.panel);
    }

    /// A raised slab of stone: the fill, then a bevel.
    ///
    /// **Four quads and two colours.** Light along the top and the left,
    /// dark along the bottom and the right -- which is what a surface
    /// standing proud of its background looks like, at any size, without
    /// a gradient or a shadow or a blur. The corners are mitred the lazy
    /// way (the light edges own them), because at three pixels nobody
    /// has ever noticed and the alternative is eight quads.
    ///
    /// **It is a picture now**, and the four quads below are what is
    /// drawn where there is none: the hide, its stitched channel and its
    /// four rivets say "a made thing" in a way two colours cannot, and
    /// they say it at every size because the frame is sliced rather than
    /// stretched. See the skin block at the top of this file.
    pub fn slab(&mut self, rect: Rect, face: [f32; 4]) {
        if self.nine(rect, Piece::Panel, face, Some(FIELD_TILE)) {
            return;
        }
        self.bevelled(rect, face, true);
    }

    /// The fill and the bevel a surface is drawn as where there is no
    /// skin: light along the top and the left of a raised thing, the
    /// other way round for a hollow.
    ///
    /// Kept whole rather than deleted, and it is not sentiment. Every
    /// test in this crate lays these screens out without a graphics
    /// card, so this is what all of them draw -- and it is the picture
    /// the skin has to agree with, because the geometry is the same
    /// either way.
    fn bevelled(&mut self, rect: Rect, face: [f32; 4], raised: bool) {
        let (light, dark) = if raised {
            (self.theme.light, self.theme.dark)
        } else {
            (self.theme.well_light, self.theme.well_dark)
        };
        let (top, bottom) = if raised { (light, dark) } else { (dark, light) };
        let t = BEVEL;
        self.quad(rect, face);
        self.quad(Rect::new(rect.x0, rect.y1 - t, rect.x1, rect.y1), top);
        self.quad(Rect::new(rect.x0, rect.y0, rect.x0 + t, rect.y1), top);
        self.quad(Rect::new(rect.x0 + t, rect.y0, rect.x1, rect.y0 + t), bottom);
        self.quad(Rect::new(rect.x1 - t, rect.y0 + t, rect.x1, rect.y1 - t), bottom);
    }

    /// A well cut *into* the stone: the same bevel, upside down.
    ///
    /// Dark along the top and the left, light along the bottom and the
    /// right. It is the only difference between a thing standing up and
    /// a hole going in, and it is what makes a grid of these read as
    /// somewhere to put things.
    ///
    /// **A picture too**, and which one depends on how deep the hollow
    /// is meant to be: the tray a group of slots stands in is the same
    /// shape with a gentler lip, and it is one row in this match rather
    /// than a second widget, because the difference between a tray and a
    /// well has to stay one difference. Anything else falls back to the
    /// bevel below.
    pub fn well(&mut self, rect: Rect, face: [f32; 4]) {
        let piece = if face == self.theme.tray { Piece::Tray } else { Piece::Well };
        if self.nine(rect, piece, face, None) {
            return;
        }
        self.bevelled(rect, face, false);
    }

    /// One cell of a grid: the picture of a slot, whole.
    ///
    /// **This is the thing the player asked for by name** -- "make the
    /// whole inventory a texture, with slots and the rest" -- and it is
    /// one quad, not five: `ui/slot.png` carries the seam, the lip, the
    /// floor and its vignette, so a wall of forty of them costs *less*
    /// than the forty bevelled wells it replaces.
    ///
    /// Drawn stretched rather than sliced because a cell is always about
    /// square and always about one size. `well` is what a hollow that
    /// can be any shape goes through.
    pub fn cell(&mut self, rect: Rect, face: [f32; 4]) {
        if !self.stretched(rect, Piece::Slot, face) {
            self.bevelled(rect, face, false);
        }
    }

    /// The groove a meter runs in.
    ///
    /// The HUD's seven strips each drew their own flat rectangle, so the
    /// gauges over the belt were the one part of the interface with no
    /// depth in it at all -- a row of coloured bars on a row of black
    /// ones, over a belt that now has a frame round it. This is the
    /// skin's scrollbar groove at whatever height the strip is; `nine`
    /// squeezes the lip into half the strip when the strip is thinner
    /// than the lip, which is what a groove a few pixels tall should do.
    ///
    /// The rectangle is the caller's, unchanged, and the fill, the
    /// hairline and the mark are still drawn over it by the caller --
    /// this replaces the floor and nothing else.
    pub fn track(&mut self, rect: Rect, colour: [f32; 4]) {
        if !self.nine(rect, Piece::Track, colour, None) {
            self.quad(rect, colour);
        }
    }

    /// What is laid over a cell to say something about it: the pointer
    /// is on it, it is the one picked up from, nothing may go in it.
    ///
    /// **An overlay with its own alpha, not a second cell.** A second
    /// opaque picture would have to agree with the first about where
    /// the lip is, and two pictures that have to agree are two pictures
    /// that stop agreeing the first time one of them is redrawn.
    ///
    /// Answers whether it drew, so a caller keeps its flat wash where
    /// there is no skin.
    #[must_use]
    pub fn cell_mark(&mut self, rect: Rect, piece: Piece, colour: [f32; 4]) -> bool {
        self.stretched(rect, piece, colour)
    }

    /// A panel with something under it and light on it.
    ///
    /// Three things, and each one is a quad or two:
    ///
    /// * **A shadow**, offset down and to the right, so the panel reads
    ///   as being *over* the world rather than cut into it. This is most
    ///   of the effect: a flat rectangle on a busy background is a hole,
    ///   and a rectangle with a shadow is an object.
    /// * **A gradient**, lighter at the top. Every real surface is, and
    ///   at this size the eye reads a flat fill as unfinished long
    ///   before it can say why.
    /// * **Two borders**, a dark one outside and a bright one inside,
    ///   which is what an edge with a thickness looks like.
    ///
    /// Used by every screen that is a slab of interface over the game --
    /// the inventory, the chest, the death screen -- so they cannot
    /// drift into looking like three different games.
    pub fn deep_panel(&mut self, rect: Rect) {
        // A shadow under it, and then the slab. The shadow is what makes
        // it an object lying on the world rather than a hole cut in it;
        // the bevel does the rest.
        const SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.35];
        const OFFSET: f32 = SHADOW_OFFSET;
        self.quad(
            Rect::new(
                rect.x0 + OFFSET,
                rect.y0 - OFFSET,
                rect.x1 + OFFSET,
                rect.y1 - OFFSET,
            ),
            SHADOW,
        );
        self.slab(rect, PANEL);
    }

    /// `indent` pushes the title right to make room for something.
    ///
    /// That something was the way out. The screens that could be opened
    /// carried an `X` in the left of their header, and a title written from
    /// the band's own edge was a title written underneath it: the pack read
    /// `XIVENTORY` the first time the two were drawn together. The chest
    /// and then the pack lost their `X` (see `inventory_screen::Intent::Close`),
    /// so every caller passes nothing today; the parameter stays for the
    /// next thing that has to stand in front of a title, which is cheaper
    /// than rediscovering `XIVENTORY`.
    ///
    /// An indent rather than a shorter band, because the band is the
    /// panel's full width and the title is the only thing that has to
    /// move.
    pub fn panel_header(
        &mut self,
        panel: Rect,
        title: &str,
        height: f32,
        indent: f32,
    ) -> f32 {
        // **No band.** A title bar with its own fill and its own edge is
        // a window chrome, and this is a slab of stone with a word
        // printed on it -- which is what every interface of this kind
        // does, and the reason they never look dated.
        // **Inside the frame, not inside the old bevel.** The band used
        // to start one bevel in -- eight thousandths -- and the title
        // eight and a half more, which cleared a three-pixel edge and
        // nothing else. The panel has a stitched frame now, and a title
        // measured against the bevel would be a title printed across it.
        // Derived from [`PANEL_BORDER`] rather than typed, so it moves
        // if the frame ever does.
        // `max` so that a caller asking for a band shallower than the
        // frame gets a band of nothing rather than one turned inside
        // out: `Rect` keeps whatever corners it is given, and an
        // upside-down rectangle draws its text at the wrong end of the
        // panel instead of failing.
        let band = Rect::new(
            panel.x0 + PANEL_BORDER,
            panel.y1 - height,
            panel.x1 - PANEL_BORDER,
            (panel.y1 - PANEL_BORDER).max(panel.y1 - height),
        );
        let cap = PIXEL * 1.05 * CAP_HEIGHT as f32;
        self.text(
            title,
            band.x0 + 0.008 + indent,
            band.centre_y() + cap / 2.0,
            1.05,
            // **The accent, not the ink**, which is what the accent is
            // for: "the one colour that is not grey -- a heading, a
            // chosen row, a full stack". Written in ink, the title of a
            // screen was the same colour as the hint line at the bottom
            // of it, and these screens had no hierarchy at all -- a
            // player looking for what they had opened had to read the
            // words to find out. It was written in ink because on the
            // old stone the accent was a 2.24:1 brown that would have
            // been *less* readable than the ink; now that it is the
            // menu's amber, a heading can look like one.
            self.theme.accent,
        );
        // A scored line under the title, sitting wholly inside the band.
        //
        // **Above the line the caller is given, never on it.** What this
        // answers is where the content starts, and a rule centred on
        // that line would take its lower half out of the content's first
        // row -- which on the hearth is the word over the fuel slot.
        self.rule(band.x0, band.x1, band.y0 + 2.0 * SKIN_TEXEL, self.theme.dark);
        band.y0
    }

    /// Draws `text` with its left edge at `left` and its *top* at `top`.
    ///
    /// One textured quad per character, sampling that character's layer
    /// of the font atlas. This used to emit a quad per *lit pixel* --
    /// about twelve per character -- which made the debug panel forty
    /// thousand vertices a frame and cost more frame time than the world
    /// behind it. See `texture::FontAtlas`.
    pub fn text(&mut self, text: &str, left: f32, top: f32, scale: f32, colour: [f32; 4]) {
        // The box this line occupies, kept so a test can ask what
        // overlaps what. See `while_recording_text`.
        #[cfg(test)]
        if !text.trim().is_empty() {
            WRITTEN.with(|cell| {
                if let Some(lines) = cell.borrow_mut().as_mut() {
                    lines.push(Written {
                        rect: Rect::new(left, top - cell_height(scale), left + ink_width(text, scale), top),
                        text: text.to_string(),
                        scale,
                    });
                }
            });
        }
        let px = PIXEL * scale;
        let advance = (GLYPH_WIDTH + GLYPH_SPACING) as f32 * px;
        let (w, h) = (GLYPH_WIDTH as f32 * px, GLYPH_HEIGHT as f32 * px);
        let mut x = left;

        for c in text.chars() {
            // A space has nothing lit, so drawing it is a quad that can
            // only cost bandwidth. Text is mostly spaces in a column of
            // aligned labels.
            if c != ' ' {
                // Where the glyph sits: a layer, and a corner within it.
                // Glyphs share a layer now -- see `FontAtlas` for why --
                // so the quad's texture coordinates start at that corner
                // rather than at the origin.
                let (layer, u0, v0) = self.font.place(c);
                let (u1, v1) = (u0 + self.font.u_max, v0 + self.font.v_max);
                let rect = Rect::new(x, top - h, x + w, top);
                for (position, uv) in [
                    ([rect.x0, rect.y0], [u0, v1]),
                    ([rect.x1, rect.y0], [u1, v1]),
                    ([rect.x1, rect.y1], [u1, v0]),
                    ([rect.x0, rect.y0], [u0, v1]),
                    ([rect.x1, rect.y1], [u1, v0]),
                    ([rect.x0, rect.y1], [u0, v0]),
                ] {
                    self.vertices.push(HotbarVertex {
                        position,
                        uv,
                        tex_layer: layer,
                        tint: colour,
                    });
                }
            }
            x += advance;
        }
    }

    pub fn text_centred(&mut self, text: &str, centre_x: f32, top: f32, scale: f32, colour: [f32; 4]) {
        self.text(text, centre_x - ink_width(text, scale) / 2.0, top, scale, colour);
    }

    /// Text vertically centred inside `rect`, at its horizontal centre.
    pub fn label_in(&mut self, rect: Rect, text: &str, scale: f32, colour: [f32; 4]) {
        // Centred on the cap height rather than the whole cell: two of
        // the nine rows are descender space that is empty for most
        // characters, and counting them sits every label visibly low.
        let cap = PIXEL * scale * CAP_HEIGHT as f32;
        let top = rect.centre_y() + cap / 2.0;
        self.text_centred(text, rect.centre_x(), top, scale, colour);
    }

    /// A label in two tones, for writing over the world.
    ///
    /// The same argument as [`hairline_frame`](Self::hairline_frame),
    /// applied to letters: a pale glyph over snow measures 1.03:1
    /// against the snow, so what makes it readable is not its own
    /// colour but the dark copy one pixel behind it tracing its shape.
    /// Whatever the world is doing, one of the two strokes is showing.
    ///
    /// One pixel of offset and no more. It is a shadow that says "there
    /// is an edge here", not a drop shadow that says "this is floating"
    /// -- at two the letters look embossed and at three they look like
    /// two labels.
    pub fn label_in_two_tones(
        &mut self,
        rect: Rect,
        text: &str,
        scale: f32,
        dark: [f32; 4],
        light: [f32; 4],
    ) {
        let cap = PIXEL * scale * CAP_HEIGHT as f32;
        let top = rect.centre_y() + cap / 2.0;
        let shadow = PIXEL * scale;
        self.text_centred(text, rect.centre_x() + shadow, top - shadow, scale, dark);
        self.text_centred(text, rect.centre_x(), top, scale, light);
    }

    /// Text vertically centred inside `rect`, aligned to its left edge
    /// plus `pad`.
    pub fn label_left(&mut self, rect: Rect, text: &str, pad: f32, scale: f32, colour: [f32; 4]) {
        let cap = PIXEL * scale * CAP_HEIGHT as f32;
        let top = rect.centre_y() + cap / 2.0;
        self.text(text, rect.x0 + pad, top, scale, colour);
    }

    /// A clickable button. `hovered` comes from hit-testing the same
    /// rectangle against the cursor, so what lights up is by construction
    /// what a click would activate.
    pub fn button(&mut self, rect: Rect, text: &str, hovered: bool, enabled: bool) {
        // A smaller slab of the same stone, with the same bevel: raised
        // when it can be pressed, and washed out when it cannot. The
        // hover state lightens the face rather than adding an outline,
        // because an outline on a bevelled thing is a third edge.
        self.pressable(rect, text, if hovered { Press::Hovered } else { Press::Idle }, enabled);
    }

    /// The same, for a caller that knows the button is being held down.
    ///
    /// **A pressed state at all is new**, and the picture is the whole
    /// of it: `ui/button_down.png` is the same board with the light on
    /// the other two sides. A player pressing a thumb button on a phone
    /// had nothing at all to tell them the tap had landed -- the finger
    /// is over the label -- and a bevel that turns over is visible at
    /// the edge of the fingertip, which a change of fill is not.
    pub fn pressable(&mut self, rect: Rect, text: &str, press: Press, enabled: bool) {
        let face = if !enabled {
            self.theme.disabled
        } else if press == Press::Hovered {
            self.theme.button_hover
        } else {
            self.theme.button
        };
        let piece = match (enabled, press) {
            (false, _) => Piece::Button,
            (true, Press::Held) => Piece::ButtonDown,
            (true, Press::Hovered) => Piece::ButtonHover,
            (true, Press::Idle) => Piece::Button,
        };
        if !self.nine(rect, piece, face, None) {
            self.bevelled(rect, face, press != Press::Held);
        }
        let colour = if enabled { self.theme.ink } else { self.theme.ink_dim };
        // Fitted rather than fixed. A label wider than its button used to
        // run out of both ends of it -- see `fitted_scale`, and see the
        // settings screen in Russian, where `ВКЛ/ВЫКЛ` was half again
        // wider than the switch it was written on.
        //
        // The size is also capped by the button's *height*, not only its
        // width. A settings row is a third the height of a menu button
        // and its switch was still lettered like one, which is what made
        // the settings screen read as shouting.
        let scale = button_label_scale(rect, text, self.content);
        self.label_in(rect, text, scale, colour);
    }

    /// One tab of a strip: the page showing stands forward and is
    /// lettered in the accent, the others sit back.
    ///
    /// **One function for every strip, and the lettering is the button's.**
    /// The pack screen and a body's two pages each drew their own showing
    /// tab, with `fitted_scale(text, 0.80, ...)` and a top worked out from
    /// the whole cell -- while the tabs beside it were lettered by
    /// `button_label_scale` and centred on the cap height. So the word on
    /// the page you were on was a different size from its neighbours
    /// (smaller on a phone, where the buttons grow with the finger floor;
    /// larger on a desktop) and sat a pixel or two low: the one tab that is
    /// supposed to look *chosen* looked like it came from another screen.
    /// Only the face and the colour may differ between the two states.
    pub fn tab(&mut self, rect: Rect, text: &str, showing: bool, hovered: bool, enabled: bool) {
        if showing {
            // **The page you are on is a tab standing forward**, which
            // is what `ui/tab_on.png` draws: lit all round and open at
            // the bottom, so it reads as joined to the panel under it
            // rather than as a hole in the strip. It was a *well* --
            // the chosen tab was drawn as the one cut into the stone --
            // which is the opposite of what a tab strip means and was
            // only ever legible because of the accent on it.
            //
            // **The panel's own colour, not the tray's.** A chosen tab
            // is a piece of the panel pulled forward, so it wears what
            // the panel wears. The flat fallback keeps the tray it has
            // always been: it has no picture to say "forward" with, so
            // its colour is the only thing it can say it with, and
            // every test in this crate is drawn against that colour.
            if !self.nine(rect, Piece::TabOn, self.theme.panel, None) {
                self.bevelled(rect, self.theme.tray, false);
            }
            let scale = button_label_scale(rect, text, self.content);
            self.label_in(rect, text, scale, self.theme.accent);
        } else {
            // **The same shape whether or not the pointer is on it.**
            // The hover lightens the face, exactly as a button's does;
            // a tab that changed *shape* under the pointer would be a
            // strip whose pieces stop being the same kind of thing the
            // moment the mouse moves across it.
            let face = if !enabled {
                self.theme.disabled
            } else if hovered {
                self.theme.button_hover
            } else {
                self.theme.button
            };
            if !self.nine(rect, Piece::TabOff, face, None) {
                self.bevelled(rect, face, true);
            }
            let colour = if enabled { self.theme.ink } else { self.theme.ink_dim };
            let scale = button_label_scale(rect, text, self.content);
            self.label_in(rect, text, scale, colour);
        }
    }

    /// One row of the settings screen: a label on the left, the current
    /// value on the right, and the widgets that change it in between.
    ///
    /// Drawn as a row rather than a dialog per setting because the whole
    /// point of a settings screen is seeing what everything is set to at
    /// once -- that is the thing a config file is bad at.
    pub fn setting_row(&mut self, rect: Rect, label: &str, value: &str, enabled: bool, style: RowStyle) {
        self.well(rect, self.theme.row);
        let (label_colour, value_colour) = if enabled {
            (self.theme.ink, self.theme.accent)
        } else {
            (self.theme.ink_dim, self.theme.ink_dim)
        };
        // The label first, and fitted: a long setting name in a language
        // that spells things out -- `ДАЛЬНОСТЬ В ОДИНОЧНОЙ ИГРЕ` -- would
        // otherwise run under the reading beside it.
        let label_room = (rect.width() - style.controls - 0.05).max(0.0);
        let label_size = fitted_scale(label, style.text, label_room, 0.7);
        self.label_left(rect, label, 0.025, label_size, label_colour);

        // ...then the reading, right-aligned into the gap between the
        // label and the buttons the caller draws after. Fitted to that
        // gap rather than trusted to be short: `COBBLESTONE` is as long
        // as some of the labels.
        //
        // **A hair clear of the buttons**, which it was not: the column
        // the buttons start at was also where the reading ended, so
        // `6 chunks` and the minus button shared an edge and read as one
        // control with a number stuck to it.
        // Scaled with the writing rather than fixed: at a desktop's
        // size a hundredth of a screen reads as a gap, and beside a
        // button drawn for a finger it reads as a number stuck to it.
        let reading_gap = 0.014 * style.text;
        let reading_room = (style.controls - 0.05 * style.text).max(0.0);
        let value_size = fitted_scale(value, style.text * style.value, reading_room, 0.7);
        let width = measure(value, value_size);
        self.label_left(
            Rect::new(
                rect.x1 - width - style.controls - reading_gap,
                rect.y0,
                rect.x1,
                rect.y1,
            ),
            value,
            0.0,
            value_size,
            value_colour,
        );
    }

    /// A scrollbar down a list's gutter: a well for the track and a thumb
    /// on it as long a share of the track as the window is of the list.
    ///
    /// **One of these, where there were four copies** -- the world list,
    /// the server list, the settings and the extensions each drew their
    /// own. What the four agreed on was the colour, and the colour was
    /// the problem: the thumb was `accent`, the amber every *reading* on
    /// the settings screen is written in, so the brightest solid shape on
    /// that screen was the one thing on it nobody reads. It is the quiet
    /// ink now: as easy to find as a help line, which is what a scrollbar
    /// is.
    ///
    /// Nothing at all when the whole list fits. A thumb as long as its
    /// track that can never move is not information; it is furniture
    /// saying "there is more" to anybody glancing at it.
    pub fn scrollbar(&mut self, track: Rect, first: usize, visible: usize, count: usize) {
        if count == 0 || count <= visible {
            return;
        }
        if !self.nine(track, Piece::Track, self.theme.well, None) {
            self.bevelled(track, self.theme.well, false);
        }
        let span = visible as f32 / count as f32;
        let offset = first.min(count - visible) as f32 / count as f32;
        // From the top down, because a list runs down the screen and y
        // runs up it.
        let top = track.y1 - track.height() * offset;
        let thumb = self.theme.ink_dim;
        let grip = Rect::new(track.x0, top - track.height() * span, track.x1, top);
        // A strap with notches across it rather than a bar of ink: a
        // thumb that looks like something to take hold of is the whole
        // difference between a scrollbar a player drags and one they
        // click either side of.
        if !self.nine(grip, Piece::Grip, thumb, None) {
            self.quad(grip, thumb);
        }
    }

    /// A single-line text field. `caret` shows the insertion point; it is
    /// drawn only when the field has focus, which is the only cue the
    /// player gets about where typing will go.
    pub fn field(&mut self, rect: Rect, text: &str, focused: bool, caret: bool) {
        // A value that is shown rather than edited -- a settings row, a
        // world type. `caret` is the blink, and where it goes is the
        // end, because there is nowhere else for it to be.
        let shown = crate::ui::field::TextField::with(text);
        self.text_field(rect, &shown, "", focused, focused && caret);
    }

    /// A field with a caret in it that the player can move, a selection
    /// behind it, and a placeholder when it is empty.
    ///
    /// **The window on a long value follows the caret**, which is the
    /// one thing the old version could not do. It always showed the
    /// tail, because the caret was always at the end; walk the caret
    /// back into a thirty-character address and it disappears off the
    /// left of the box, along with the letter about to be deleted.
    pub fn text_field(
        &mut self,
        rect: Rect,
        field: &crate::ui::field::TextField,
        placeholder: &str,
        focused: bool,
        caret: bool,
    ) {
        // A well with writing in it. Focus is a bright rim inside the
        // well rather than a coloured frame around it -- see `button`
        // for why nothing here grows an outline.
        self.well(rect, self.theme.field);
        if focused {
            let accent = self.theme.accent;
            self.quad(
                Rect::new(rect.x0 + BEVEL, rect.y1 - BEVEL * 2.0, rect.x1 - BEVEL, rect.y1 - BEVEL),
                accent,
            );
            // **And a rim on the other three sides.** One bright line
            // along the top is easy to miss on a screen of rows that
            // all have a bevel there anyway, and "which field am I
            // typing into" is the question a form has to answer without
            // being asked. Quiet enough not to read as a button.
            self.border(rect, BEVEL * 0.5, with_alpha(accent, 0.35));
        }

        let text = field.text();
        // `usable` is the drawing's own -- the placeholder is fitted to
        // it below and the window has already been taken against it.
        let Metrics { scale, pad, usable, from, to } =
            field_metrics(rect, self.content, text, field.caret());
        if text.is_empty() {
            // The placeholder is what the field is *for*, in the field,
            // where an empty well says nothing at all. Drawn in the
            // quiet ink and never mistaken for a value: it disappears
            // the moment there is one.
            if !placeholder.is_empty() {
                let ink_dim = self.theme.ink_dim;
                // **Pushed past the caret.** Both want the same first
                // column, and the caret is as thick as a letter's
                // stroke in this font -- so drawn on top of each other
                // the hint's first character reads as a different
                // letter. Looked at in `menu_new_world.png`, where
                // "a name for this world" came out as "b name".
                let clear = if focused { PIXEL * scale * 2.0 } else { 0.0 };
                let room = (usable - clear).max(0.0);
                self.label_left(
                    rect,
                    &fit(placeholder, scale, room),
                    pad + clear,
                    scale,
                    ink_dim,
                );
            }
            if focused && caret {
                self.caret_at(rect, rect.x0 + pad, scale);
            }
            return;
        }

        let shown = &text[from..to];
        // Selection behind the writing, so the letters stay the colour
        // they were rather than being inverted -- this font is one
        // bitmap and a knocked-out glyph is a hole, not a letter.
        if let Some((low, high)) = field.selection() {
            let low = low.clamp(from, to);
            let high = high.clamp(from, to);
            if high > low {
                let x0 = rect.x0 + pad + measure(&text[from..low], scale);
                let x1 = rect.x0 + pad + measure(&text[from..high], scale);
                let half = PIXEL * scale * CAP_HEIGHT as f32 / 2.0 + PIXEL * scale;
                let accent = self.theme.accent;
                self.quad(
                    Rect::new(x0, rect.centre_y() - half, x1, rect.centre_y() + half),
                    with_alpha(accent, 0.30),
                );
            }
        }
        let ink = self.theme.ink;
        self.label_left(rect, shown, pad, scale, ink);

        if focused && caret {
            let at = field.caret().clamp(from, to);
            self.caret_at(rect, rect.x0 + pad + measure(&text[from..at], scale), scale);
        }
    }

    /// The blinking bar, wherever the caret is.
    fn caret_at(&mut self, rect: Rect, x: f32, scale: f32) {
        let x = x + PIXEL * 0.5;
        let half = PIXEL * scale * CAP_HEIGHT as f32 / 2.0;
        self.quad(
            Rect::new(x, rect.centre_y() - half, x + PIXEL * scale, rect.centre_y() + half),
            ACCENT,
        );
    }
}

/// The same colour, at a different opacity.
///
/// A free function rather than a method, because the two callers are a
/// selection block and a focus rim and neither is worth a `Painter` to
/// work out.
fn with_alpha(colour: [f32; 4], alpha: f32) -> [f32; 4] {
    [colour[0], colour[1], colour[2], alpha]
}

/// Everything both drawing a field and clicking in one need to know.
///
/// **One function for the two of them, and it is not a tidy-up.** The
/// rule this interface is held to is that a hit-test must be the exact
/// inverse of the drawing, and here the inverse has to be right to the
/// character: a click lands on the boundary between two letters, and the
/// only way to say which two is to walk the same string at the same size
/// through the same window. Three numbers worked out twice would agree
/// until the day one of them was changed.
pub struct Metrics {
    /// How big the writing is drawn.
    pub scale: f32,
    /// Air inside the well, before the first character.
    pub pad: f32,
    /// How much width the writing has.
    pub usable: f32,
    /// The slice of the value that is on screen. See [`window`].
    pub from: usize,
    pub to: usize,
}

/// The numbers a field is drawn from, given its box and its content.
pub fn field_metrics(rect: Rect, content: f32, text: &str, caret: usize) -> Metrics {
    let pad = 0.018;
    // **Scaled to the box, not fixed.** A field on the settings screen
    // is as tall as its row, and the rows shrink to fit however many
    // settings there are -- so a fixed-size value sat taller than the
    // well it was in, with the descenders of `player` hanging through
    // the bottom bevel. The same number the settings rows use, so the
    // writing in a field matches the writing beside it.
    const FIELD_DESIGN_HEIGHT: f32 = 0.070;
    let scale = (rect.height() / FIELD_DESIGN_HEIGHT).clamp(0.5, content);
    let usable = (rect.x1 - rect.x0 - pad * 2.0).max(0.0);
    let (from, to) = window(text, caret.min(text.len()), scale, usable);
    Metrics { scale, pad, usable, from, to }
}

/// Where in the text a click at `x` landed.
///
/// **The exact inverse of [`Painter::text_field`]**, which is the rule
/// in CLAUDE.md at the one place it has to hold to a single character
/// rather than to a rectangle. Both sides ask [`field_metrics`] for the
/// size and the window, and this walks the same slice the drawing walks.
///
/// The answer is the nearest *boundary between characters*, not the
/// character the pointer is over -- so a click on the left half of a
/// letter puts the caret before it and one on the right half puts it
/// after, which is what every editor does and what a player aiming at a
/// gap between two letters is actually aiming at.
///
/// A click past either end of the visible text lands at that end, which
/// falls out of taking the nearest boundary and is also what should
/// happen: a field is clicked in the middle far more often than it is
/// clicked precisely.
pub fn caret_at_x(rect: Rect, content: f32, text: &str, caret: usize, x: f32) -> usize {
    let m = field_metrics(rect, content, text, caret);
    caret_in_run(text, m.from, m.to, m.scale, rect.x0 + m.pad, x)
}

/// The same question for a run of text that is not in a field's well.
///
/// **The chat box is the other caller**, and it is not a `text_field`:
/// it is drawn at its own fixed size on a plate of its own, with its
/// own margin. What the two share is the walk -- the window, the
/// measuring and the nearest boundary -- and sharing it is the only
/// thing that keeps two widgets' carets from being placed by two
/// slightly different rules. See `chat::Chat::tapped`.
pub fn caret_in_run(text: &str, from: usize, to: usize, scale: f32, left: f32, x: f32) -> usize {
    let mut best = from;
    let mut best_gap = (left - x).abs();
    for (index, c) in text[from..to].char_indices() {
        let at = from + index + c.len_utf8();
        let gap = (left + measure(&text[from..at], scale) - x).abs();
        if gap < best_gap {
            best_gap = gap;
            best = at;
        }
    }
    best
}

/// Which slice of a value is on screen, given where the caret is.
///
/// **Always contains the caret**, which is the whole point: a field that
/// showed the tail of the text unconditionally hides the caret the
/// moment it is walked back into a long line, and with it the character
/// about to be deleted.
///
/// Grown leftward from the caret first and rightward afterwards, so a
/// caret at the end of a long value shows the end of it -- which is
/// what the old unconditional tail did and what typing wants.
pub fn window(text: &str, caret: usize, scale: f32, usable: f32) -> (usize, usize) {
    let caret = caret.min(text.len());
    if measure(text, scale) <= usable {
        return (0, text.len());
    }
    let mut from = caret;
    for (index, _) in text[..caret].char_indices().rev() {
        if measure(&text[index..caret], scale) > usable {
            break;
        }
        from = index;
    }
    let mut to = caret;
    for (index, c) in text[caret..].char_indices() {
        let end = caret + index + c.len_utf8();
        if measure(&text[from..end], scale) > usable {
            break;
        }
        to = end;
    }
    (from, to)
}

/// Breaks text on whitespace to at most `width` characters per line.
///
/// Used for connection errors, which can be long, and a truncated one is
/// useless for working out what is wrong.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    /// Every piece of small writing is readable on every surface it is
    /// drawn on.
    ///
    /// **4.5:1 is the number, and it is not arbitrary**: it is what the
    /// accessibility guidelines ask of body text, and it is roughly
    /// where a person with ordinary vision stops having to lean in.
    ///
    /// This test was written after measuring, not before. The world's
    /// own screens -- the pack, a chest -- were failing it everywhere:
    /// the quiet ink on a slot came to **1.31:1**, which is not quiet
    /// text but text that is very nearly not there. The four letters
    /// naming the empty armour slots were drawn in exactly that, on
    /// exactly that, and the whole reason they went unnoticed for so
    /// long is that whoever looked at them already knew what they said.
    ///
    /// The menus passed and were left alone.
    #[test]
    fn small_text_is_readable_against_everything_it_is_drawn_on() {
        /// What body text has to reach.
        const BODY: f32 = 4.5;
        /// What the quiet tier has to reach.
        ///
        /// Three, not four and a half, and the difference is a real
        /// decision rather than a concession. On a light panel the only
        /// way to make writing quieter is to make it *lighter*, and
        /// that is the same move as making it harder to read -- hold
        /// both tiers to the body threshold and they converge, and the
        /// hierarchy the screens are built out of disappears. So the
        /// quiet tier keeps the large-text threshold, and anything
        /// small that was being drawn in it moves to `ink`. The armour
        /// letters did.
        const QUIET: f32 = 3.0;

        // `tray` joined the list with the trays themselves: every
        // caption on the pack and the chest screens is now written on
        // one, so a surface nobody had measured acquired all the small
        // writing on the two screens made of slots.
        const INK_SURFACES: [&str; 5] = ["panel", "well", "button", "row", "tray"];
        const ACCENT_SURFACES: [&str; 4] = ["panel", "well", "row", "tray"];

        for (name, theme) in [("stone", Theme::STONE), ("dark", Theme::DARK)] {
            for (ink_name, ink, floor, surfaces) in [
                ("ink", theme.ink, BODY, &INK_SURFACES[..]),
                ("ink_dim", theme.ink_dim, QUIET, &INK_SURFACES[..]),
                // **The tier this test used to leave out**, and it is
                // the one that went wrong: `accent` is the colour a
                // heading is written in, and on the old stone it came
                // to 2.24:1 against the panel -- the least readable
                // thing on a screen, wearing the job of the most. Held
                // to the body threshold rather than the large-text one
                // because a full stack's count is drawn in it too, and
                // that is four small digits.
                //
                // Not measured against a button: a button carries
                // `ink`, and a surface light enough to look raised
                // cannot also hold amber at 4.5:1. Add an amber label
                // to a button and this list is what has to change.
                ("accent", theme.accent, BODY, &ACCENT_SURFACES[..]),
            ] {
                for on_name in surfaces {
                    let on = match *on_name {
                        "panel" => theme.panel,
                        "well" => theme.well,
                        "button" => theme.button,
                        "tray" => theme.tray,
                        _ => theme.row,
                    };
                    let ratio = contrast(ink, on);
                    assert!(
                        ratio >= floor,
                        "{name}: {ink_name} on {on_name} is {ratio:.2}:1, below {floor}:1"
                    );
                }
            }
        }
    }

    /// The two status inks read wherever a status is printed.
    ///
    /// **The test that would have caught them.** `TEXT_BAD` and
    /// `TEXT_GOOD` were chosen for pale stone and outlived it by two
    /// redesigns of both skins, because the readability test above held
    /// `ink`, `ink_dim` and `accent` to a floor and never these -- so a
    /// red at 1.6:1 carried every warning in the game. Held to the body
    /// threshold, not the quiet one: "rain has stopped it" is the most
    /// important line on its screen, not a footnote.
    #[test]
    fn a_warning_and_an_all_clear_read_on_every_surface_they_are_printed_on() {
        for (name, theme) in [("stone", Theme::STONE), ("dark", Theme::DARK)] {
            for (what, ink) in [("TEXT_BAD", TEXT_BAD), ("TEXT_GOOD", TEXT_GOOD)] {
                for (on_name, on) in [
                    ("panel", theme.panel),
                    ("tray", theme.tray),
                    ("well", theme.well),
                    ("row", theme.row),
                    ("field", theme.field),
                ] {
                    let ratio = contrast(ink, on);
                    assert!(
                        ratio >= 4.5,
                        "{name}: {what} on {on_name} is {ratio:.2}:1, below 4.5:1"
                    );
                }
            }
        }
    }

    /// A scrollbar is there only while there is more list than window,
    /// and its thumb never leaves the track.
    ///
    /// The second half is the one a list gets wrong at its last page: a
    /// thumb measured from the top by `first / count` and as long as
    /// `visible / count` has to land exactly on the bottom of the track
    /// when the window is at the end, or it hangs off the panel.
    #[test]
    fn a_scrollbar_appears_only_for_a_list_longer_than_its_window_and_stays_on_its_track() {
        let track = Rect::new(0.9, -0.5, 0.92, 0.5);
        let mut fits = Painter::onto_themed(
            crate::engine::texture::FontAtlas::for_test(),
            Vec::new(),
            Theme::DARK,
        );
        fits.scrollbar(track, 0, 11, 11);
        assert!(fits.vertices.is_empty(), "a list that fits grew a scrollbar");

        for first in [0usize, 5, 11] {
            let mut p = Painter::onto_themed(
                crate::engine::texture::FontAtlas::for_test(),
                Vec::new(),
                Theme::DARK,
            );
            p.scrollbar(track, first, 11, 22);
            let low = p.vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let high = p.vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            assert!(
                low >= track.y0 - 1e-5 && high <= track.y1 + 1e-5,
                "at {first} the scrollbar reaches {low}..{high} on a track {track:?}"
            );
        }
    }

    /// A label fits inside its button whatever the key is called.
    ///
    /// The failure this replaces: a scale picked as a constant, which
    /// suited `G` and let `L SHIFT` run out over the world, and then a
    /// scale divided by the letter count, which fitted `L SHIFT` and
    /// shrank `G` to a speck. Both were guesses about a length that is
    /// not known until a player picks a key.
    #[test]
    fn a_label_is_as_large_as_it_can_be_and_still_fit() {
        let button = Rect::centred(0.0, 0.0, 0.32, 0.32);
        for text in ["G", "F3", "MINE", "PLACE", "L SHIFT", "BACKSPACE"] {
            let scale = scale_to_fit(button, text, 0.8);
            let width = PIXEL * scale * text_width(text) as f32;
            let height = PIXEL * scale * GLYPH_HEIGHT as f32;
            assert!(
                width <= button.width() && height <= button.height(),
                "{text:?} at {scale} measured {width} x {height} in a {} box",
                button.width()
            );
            assert!(scale > 0.0, "{text:?} was given no size at all");
        }

        // ...and a longer name is never given a *larger* size than a
        // shorter one in the same box.
        let short = scale_to_fit(button, "G", 0.8);
        let long = scale_to_fit(button, "BACKSPACE", 0.8);
        assert!(long <= short);
    }

    /// Scaling the interface and un-scaling a tap are one operation and
    /// its inverse.
    ///
    /// The property the whole HUD hit-test rests on. `scale_about`
    /// moves the picture out from under the finger; `unscale_about` is
    /// how the finger finds out what it landed on. Two expressions of
    /// one transform -- when they drift, the interface looks right and
    /// answers in the wrong place, which is the failure this file's
    /// header warns about.
    #[test]
    fn a_tap_is_unscaled_to_exactly_where_the_drawing_came_from() {
        for scale in [0.5, 1.0, 1.5, 2.0, 4.0] {
            for origin in [
                anchor::BOTTOM(1.78),
                anchor::CENTRE(1.78),
                anchor::BOTTOM_LEFT(1.78),
            ] {
                for authored in [(0.0, -0.94), (-1.2, 0.3), (0.7, -0.1)] {
                    // Where `scale_about` would put this vertex...
                    let drawn = (
                        origin.0 + (authored.0 - origin.0) * scale,
                        origin.1 + (authored.1 - origin.1) * scale,
                    );
                    // ...and back again.
                    let (x, y) = unscale_about(drawn, origin, scale);
                    assert!(
                        (x - authored.0).abs() < 1e-4 && (y - authored.1).abs() < 1e-4,
                        "at scale {scale} about {origin:?}, {authored:?} came back as {:?}",
                        (x, y)
                    );
                }
            }
        }
    }

    use super::*;

    /// **The tab you are on is lettered like the tabs you are not on.**
    /// Same glyph size, same baseline: only the face and the ink change.
    /// See `Painter::tab` for the strip where the chosen word was a size
    /// of its own.
    #[test]
    fn a_showing_tab_is_lettered_exactly_like_its_neighbours() {
        let glyphs = |showing: bool| {
            let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
            p.tab(Rect::new(-0.3, 0.0, 0.3, 0.1), "ПОЖИТКИ", showing, false, true);
            p.into_vertices()
                .into_iter()
                .filter(|v| v.tex_layer != crate::ui::hotbar::UNTEXTURED)
                .map(|v| v.position)
                .collect::<Vec<_>>()
        };
        let (chosen, other) = (glyphs(true), glyphs(false));
        assert!(!chosen.is_empty(), "the showing tab wrote nothing");
        assert_eq!(chosen, other, "the showing tab's word is drawn at another size or place");
    }

    #[test]
    fn a_desktop_layout_leaves_every_number_exactly_as_it_was() {
        // **The hard constraint on the whole of this.** Every screen
        // that was rewritten to ask a `Layout` for its sizes has to
        // draw the identical geometry on the machine it was designed
        // on -- and the only way to know that without a golden image
        // is for each helper to be provably the identity there.
        let desktop = Layout::desktop();
        for v in [0.0f32, 0.014, 0.105, 1.15, 3.0] {
            assert_eq!(desktop.at(v), v, "at({v}) moved a desktop layout");
        }
        assert_eq!(desktop.content(), 1.0);
        assert_eq!(desktop.panel_half_width(1.15), 1.15);
        assert_eq!(desktop.finger(), 0.0, "a mouse is not a finger");
        // ...at every window shape, not only 16:9: a scale of one means
        // the interface it always had.
        for aspect in [1.0f32, 4.0 / 3.0, 16.0 / 9.0, 2712.0 / 1220.0] {
            let layout = Layout::for_screen(aspect, 1.0);
            assert_eq!(layout.at(0.105), 0.105);
        }
        // **`fit` is deliberately no longer on this list.** It asserted
        // `fit(extent) == 1.0` for three extents and at four aspects,
        // which is exactly the behaviour rule 1 at the top of this file
        // was written to end: a screen that never grows unless a player
        // finds a setting is a screen that is a postage stamp for
        // everyone who does not. What survives of the guarantee is the
        // half that was worth having -- a screen is never drawn
        // *smaller* than it was authored -- and it is checked here
        // rather than deleted.
        for extent in [(1.15f32, 0.95f32), (0.5, 0.4), (4.0, 4.0)] {
            assert!(desktop.fit(extent) >= 1.0, "{extent:?} was shrunk");
        }
        // The chat log is the one screen that still answers the old way,
        // and for a stated reason -- see `fit_from_corner`.
        assert_eq!(desktop.fit_from_corner((1.0, 0.8)), 1.0);
    }

    #[test]
    fn a_screen_with_room_around_it_is_drawn_bigger_without_being_asked() {
        // Rule 1. The three extents are measured off the real screens:
        // the pack, the hearth and the anvil's job list, which between
        // them are the widest, the tallest and the smallest thing the
        // game centres on the glass. Every one of them was drawn at
        // 1.0 -- a quarter of a 1280x720 window -- until this changed.
        for (w, h) in [(1280.0f32, 720.0), (1920.0, 1080.0), (2712.0, 1220.0)] {
            let layout = Layout::for_screen(w / h, 1.0);
            for (extent, name) in
                [((0.99f32, 0.43f32), "the pack"), ((0.56, 0.70), "the hearth"), ((0.35, 0.26), "a job list")]
            {
                let scale = layout.fit(extent);
                assert!(
                    scale > 1.2,
                    "at {w}x{h} {name} still draws at {scale} with the setting untouched",
                );
            }
        }
        // ...and it is still the glass that stops it, not the cap: a
        // window with no room gives none away.
        let cramped = Layout::for_screen(1.0, 1.0);
        assert_eq!(cramped.fit((1.2, 1.1)), 1.0, "a screen already past the glass was grown");
    }

    #[test]
    fn the_interface_size_setting_actually_changes_the_interface() {
        // **The bug this replaced.** One `fit_scale` for the whole
        // interface capped everything at `1.0 / 0.95` -- a number with
        // no screen in it -- so every device answered 1.05 to every
        // request. Moving the setting did nothing anybody could see.
        //
        // What has to hold now: each step of the setting is a step of
        // the interface, on both shapes of screen.
        for aspect in [16.0f32 / 9.0, 2712.0 / 1220.0] {
            let mut previous = 0.0;
            for requested in [1.0f32, 1.25, 1.5, 2.0, 3.0, 4.0] {
                let layout = Layout::for_screen(aspect, requested);
                let row = layout.at(0.105);
                assert!(
                    row > previous * 1.05,
                    "at {aspect:.2}, {requested} draws a row {row}, barely past {previous}",
                );
                previous = row;
            }
        }
    }

    #[test]
    fn nothing_a_screen_draws_ends_up_off_the_window() {
        // The property the old global cap was protecting, kept: a
        // screen grown to `fit` its extent stays inside the window,
        // whatever is asked for and whatever shape the window is.
        for (w, h) in [(1920.0f32, 1080.0f32), (2712.0, 1220.0), (1620.0, 1080.0), (720.0, 1280.0)] {
            let aspect = w / h;
            for requested in [1.0f32, 1.5, 2.0, 4.0] {
                let layout = Layout::for_screen(aspect, requested);
                for extent in [(1.15f32, 0.95f32), (0.62, 0.50), (1.30, 0.72)] {
                    let scale = layout.fit(extent);
                    // The ceiling moved with rule 1: a screen takes the
                    // room first and the setting multiplies that, so the
                    // most it can come to is `requested * NATURAL_GROWTH`
                    // -- and the glass, checked below, is what actually
                    // binds on every window in this list.
                    assert!(
                        scale <= requested * NATURAL_GROWTH + 1e-3,
                        "asked for {requested}, got {scale}",
                    );
                    assert!(scale >= 1.0, "shrank the interface to {scale}");
                    // A screen already too big for the window at its
                    // authored size -- the settings panel on a phone
                    // held upright -- is clipped whatever happens, and
                    // shrinking it would make it unreadable as well as
                    // cut off. What must never happen is *growing* one
                    // past the glass.
                    if scale <= 1.0 {
                        continue;
                    }
                    assert!(
                        extent.0 * scale <= aspect + 1e-3,
                        "at {w}x{h} scale {scale} puts {} off the sides of {}",
                        extent.0 * scale * 2.0,
                        aspect * 2.0,
                    );
                    assert!(
                        extent.1 * scale <= 1.0 + 1e-3,
                        "at {w}x{h} scale {scale} puts {extent:?} off the top and bottom",
                    );
                }
            }
        }
    }

    #[test]
    fn a_panel_widens_into_the_room_a_phone_has_and_no_further() {
        // Width is the one thing a 22:9 screen has spare, and the
        // settings panel was a fixed box in the middle of it.
        let phone = Layout::for_screen(2712.0 / 1220.0, 1.5);
        let half = phone.panel_half_width(1.15);
        assert!(half > 1.15, "the panel did not use the width it was given");
        assert!(
            half <= phone.aspect - SCREEN_MARGIN + 1e-4,
            "the panel {half} reaches past the edge at {}",
            phone.aspect,
        );
        // ...and on a narrow window it stops at the glass rather than
        // hanging off both sides.
        let tall = Layout::for_screen(0.6, 4.0);
        assert!(tall.panel_half_width(1.15) <= 1.15 + 1e-4);
    }

    #[test]
    fn a_click_lands_on_the_screen_that_was_grown_under_it() {
        // The invariant a per-screen scale can break silently: the
        // frame loop grows a screen about the centre by `fit`, so a
        // click has to be divided by that same number or the button is
        // drawn under the finger and answers half a screen away.
        let layout = Layout::for_screen(2712.0 / 1220.0, 2.0);
        let scale = layout.fit((0.62, 0.50));
        assert!(scale > 1.0, "a screen with room to grow did not grow");
        for point in [(0.0f32, 0.0f32), (0.4, -0.3), (-0.55, 0.45)] {
            // Where the frame loop would draw a widget authored here.
            let drawn = (point.0 * scale, point.1 * scale);
            let (back_x, back_y) = layout.hit(drawn, scale);
            assert!(
                (back_x - point.0).abs() < 1e-4 && (back_y - point.1).abs() < 1e-4,
                "{point:?} was drawn at {drawn:?} and read back as ({back_x}, {back_y})",
            );
        }
    }

    #[test]
    fn a_finger_is_a_floor_only_where_there_is_a_finger() {
        let phone = Layout::for_screen(2712.0 / 1220.0, 1.5);
        if phone.is_touch() {
            assert!(phone.finger() > 0.14, "a finger came out at {}", phone.finger());
        } else {
            assert_eq!(phone.finger(), 0.0, "a desktop has no finger");
        }
    }

    #[test]
    fn a_bigger_interface_is_still_clicked_where_it_is_drawn() {
        // **The one invariant a UI scale can break silently.** The
        // vertex shader multiplies every interface position by the
        // scale; this divides by it. If the two ever disagree the
        // interface still looks right and simply stops responding
        // where it is -- a button drawn under the finger that answers
        // half a screen away, which reads as "touch is broken" rather
        // than as an arithmetic error.
        //
        // Checked by round-tripping: take a point in interface space,
        // work out which pixel the shader would put it at, and ask
        // `cursor_to_ui` to name the point again.
        let size = (2712u32, 1220u32);
        let (w, h) = (size.0 as f32, size.1 as f32);
        let aspect = w / h;

        for scale in [1.0f32, 1.5, 2.0, 4.0] {
            for point in [(0.0f32, 0.0f32), (0.4, -0.7), (-0.9, 0.2)] {
                // What the shader does: scale, then squash x by the
                // aspect, giving normalised device coordinates.
                let ndc_x = point.0 * scale / aspect;
                let ndc_y = point.1 * scale;
                // ...and what the window system does with those.
                let pixel_x = ((ndc_x + 1.0) / 2.0 * w) as f64;
                let pixel_y = ((1.0 - ndc_y) / 2.0 * h) as f64;

                let (back_x, back_y) = cursor_to_ui((pixel_x, pixel_y), size, scale);
                assert!(
                    (back_x - point.0).abs() < 1e-3 && (back_y - point.1).abs() < 1e-3,
                    "at scale {scale}: {point:?} was drawn at ({pixel_x:.0}, {pixel_y:.0})                      and read back as ({back_x}, {back_y})",
                );
            }
        }
    }

    #[test]
    fn a_scale_of_one_is_the_mapping_the_desktop_always_had() {
        // The desktop path has to be untouched by the existence of the
        // scale: the centre of the window is the centre of the
        // interface, the left edge is minus the aspect, and the top is
        // one. Spelled out rather than compared against itself, which
        // would pass however wrong the function became.
        let size = (1920u32, 1080u32);
        let aspect = 1920.0 / 1080.0;
        let (x, y) = cursor_to_ui((960.0, 540.0), size, 1.0);
        assert!(x.abs() < 1e-3 && y.abs() < 1e-3, "middle came out at ({x}, {y})");
        let (x, y) = cursor_to_ui((0.0, 0.0), size, 1.0);
        assert!((x + aspect).abs() < 1e-3 && (y - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_cursor_maps_to_where_the_geometry_is_drawn() {
        // The centre of the window is the centre of UI space, whatever
        // the aspect ratio.
        for size in [(1280u32, 720u32), (800, 800), (3440, 1440)] {
            let (x, y) = cursor_to_ui((size.0 as f64 / 2.0, size.1 as f64 / 2.0), size, 1.0);
            assert!(x.abs() < 1e-5 && y.abs() < 1e-5, "{size:?} gave ({x}, {y})");
        }
    }

    #[test]
    fn the_cursor_mapping_inverts_the_shaders_aspect_divide() {
        // A button drawn at x = 0.5 must be clickable at the pixel the
        // shader puts it at: ndc 0.5/aspect, i.e. that fraction across
        // the window.
        let size = (1600u32, 900u32);
        let aspect = 1600.0 / 900.0;
        let ndc_x = 0.5 / aspect;
        let pixel_x = ((ndc_x + 1.0) / 2.0) * 1600.0;
        let (x, _) = cursor_to_ui((pixel_x, 450.0), size, 1.0);
        assert!((x - 0.5).abs() < 1e-4, "expected 0.5, got {x}");
    }

    #[test]
    fn the_top_left_of_the_window_is_the_top_left_of_ui_space() {
        let (x, y) = cursor_to_ui((0.0, 0.0), (1280, 720), 1.0);
        assert!(y > 0.99, "y should be at the top, got {y}");
        assert!(x < -1.7, "x should be at the left edge, got {x}");
    }

    #[test]
    fn a_rect_contains_its_own_centre_and_not_a_point_outside() {
        let r = Rect::centred(0.0, 0.0, 1.0, 0.2);
        assert!(r.contains(0.0, 0.0));
        assert!(r.contains(0.49, 0.09));
        assert!(!r.contains(0.51, 0.0));
        assert!(!r.contains(0.0, 0.11));
    }

    #[test]
    fn buttons_stacked_by_a_layout_do_not_overlap() {
        let first = Rect::centred(0.0, 0.3, 1.0, 0.09);
        let second = Rect::centred(0.0, 0.3 - 0.12, 1.0, 0.09);
        assert!(second.y1 < first.y0, "adjacent buttons overlap");
    }

    /// **The stone screens write at the five named sizes and at no
    /// others**, and this reads the source to say so.
    ///
    /// ## Why a test that parses Rust
    ///
    /// Because the property is about what somebody types. Every other
    /// way of checking it needs the scale to stop being an `f32`, and
    /// making it a newtype means touching a hundred call sites in six
    /// files to catch a mistake nobody makes twice a year. What this
    /// costs is forty lines of bracket matching; what it buys is that
    /// `p.text(..., 0.62, ...)` -- the exact line that gave one screen
    /// two captions at two sizes -- cannot be committed again.
    ///
    /// ## What it allows
    ///
    /// A named size, a variable, or a call. `fitted_scale(word,
    /// size::CAPTION, room, 0.7)` is *right*: the ceiling is one of the
    /// five and the floor is how far a long word may come down before it
    /// is left to overflow. What it refuses is a scale argument that
    /// *starts* with a digit, which is the shape every one of the
    /// thirteen sizes had.
    ///
    /// ## What it does not cover
    ///
    /// The HUD and the menu. They are different surfaces and they have
    /// always had sizes of their own -- see the note on `widgets::size`.
    #[test]
    fn the_stone_screens_write_at_the_named_sizes_and_at_no_others() {
        // The screens the player was talking about: the pack, the
        // things that share its widgets, and the journal's pages.
        let sources: [(&str, &str); 9] = [
            ("inventory_screen.rs", include_str!("inventory_screen.rs")),
            ("chest_screen.rs", include_str!("chest_screen.rs")),
            ("station_screen.rs", include_str!("station_screen.rs")),
            ("mannequin.rs", include_str!("mannequin.rs")),
            ("ladder_screen.rs", include_str!("ladder_screen.rs")),
            ("recipe_book.rs", include_str!("recipe_book.rs")),
            ("give_screen.rs", include_str!("give_screen.rs")),
            ("map_screen.rs", include_str!("map_screen.rs")),
            ("journal.rs", include_str!("journal.rs")),
        ];
        // Which argument of each call is the size.
        let calls: [(&str, usize); 5] = [
            (".text(", 3),
            (".text_centred(", 3),
            (".label_in(", 2),
            (".label_left(", 3),
            (".label_in_two_tones(", 2),
        ];
        let mut offenders = Vec::new();
        for (name, source) in sources {
            for (call, index) in calls {
                let mut from = 0;
                while let Some(at) = source[from..].find(call) {
                    let open = from + at + call.len();
                    from = open;
                    let Some(args) = balanced(&source[open..]) else {
                        continue;
                    };
                    let Some(scale) = split_top_level(args).into_iter().nth(index) else {
                        continue;
                    };
                    let scale = scale.trim();
                    if scale.starts_with(|c: char| c.is_ascii_digit()) {
                        let line = source[..open].matches('\n').count() + 1;
                        offenders.push(format!("{name}:{line} writes at {scale}"));
                    }
                }
            }
        }
        // ...and the back door: a file that writes `SCALE` at the call
        // site and defines `const SCALE: f32 = 0.62;` ten lines up has
        // named nothing. Every `*_SCALE` on these screens has to be one
        // of the five or derived from one.
        for (name, source) in sources {
            for (at, text) in source.lines().enumerate() {
                let text = text.trim();
                // The map's zoom is a scale too and it is not a text
                // one: `MIN_SCALE` is blocks to the pixel. Named here
                // rather than guessed from the identifier, because a
                // guess is how a real offender gets waved through.
                let zoom = ["MIN_SCALE", "MAX_SCALE", "DEFAULT_SCALE"]
                    .iter()
                    .any(|name| text.contains(name));
                let is_size = text.starts_with("const ")
                    && text.contains("SCALE")
                    && text.contains(": f32 =")
                    && !zoom;
                if is_size && !text.contains("size::") {
                    offenders.push(format!("{name}:{} defines {text}", at + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a size nobody named -- use one of `widgets::size`:\n{}",
            offenders.join("\n"),
        );
        // ...and the scan itself has to be doing something, or a
        // refactor that renamed `text` would make this pass forever.
        assert!(
            sources.iter().any(|(_, s)| s.contains("size::CAPTION")),
            "the scan found no named sizes at all: has `Painter::text` been renamed?",
        );
    }

    /// The text of a call's arguments: everything up to the `)` that
    /// closes the `(` just consumed.
    #[cfg(test)]
    fn balanced(after_open: &str) -> Option<&str> {
        let mut depth = 1usize;
        for (at, c) in after_open.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&after_open[..at]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Splits on the commas that are not inside a nested call.
    #[cfg(test)]
    fn split_top_level(args: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0usize, 0usize);
        for (at, c) in args.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(&args[start..at]);
                    start = at + 1;
                }
                _ => {}
            }
        }
        parts.push(&args[start..]);
        parts
    }


    #[test]
    fn text_starts_where_it_is_asked_to() {
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.text("AB", -0.4, 0.0, 1.0, TEXT);
        let min = p.vertices.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
        assert!((min + 0.4).abs() < 1e-6, "text drifted to {min}");
    }

    #[test]
    fn centred_text_is_actually_centred() {
        // Each character is one quad spanning its whole cell, and the
        // last cell carries the blank spacing column on its right. The
        // drawn geometry therefore runs from -ink/2 to -ink/2 + advance,
        // and it is the *ink* that sits centred. Asserting plain
        // symmetry would be asserting that the gap after the last letter
        // is part of the word.
        for text in ["HELLO", "MM", "NUN", "i", "Server 1"] {
            let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
            p.text_centred(text, 0.0, 0.0, 1.0, TEXT);
            let min = p.vertices.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
            let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);

            let ink = ink_width(text, 1.0);
            assert!((min + ink / 2.0).abs() < 1e-4, "{text:?} starts at {min}");
            assert!(
                (max - (ink / 2.0 + PIXEL)).abs() < 1e-4,
                "{text:?} ends at {max}"
            );
        }
    }

    #[test]
    fn a_character_is_one_quad_and_a_space_is_none() {
        // The whole reason the font became an atlas. It used to emit a
        // quad per lit pixel -- about twelve a character -- which made
        // the debug panel forty thousand vertices a frame.
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.text("AB", 0.0, 0.0, 1.0, TEXT);
        assert_eq!(p.vertices.len(), 12, "two characters is two quads");

        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.text("A B", 0.0, 0.0, 1.0, TEXT);
        assert_eq!(p.vertices.len(), 12, "a space should draw nothing");
    }

    #[test]
    fn a_space_still_advances_the_pen() {
        // Drawing nothing must not mean occupying nothing, or every
        // aligned column of text collapses.
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.text("A B", 0.0, 0.0, 1.0, TEXT);
        let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        assert!((max - measure("A B", 1.0)).abs() < 1e-4, "got {max}");
    }

    #[test]
    fn every_character_samples_its_own_corner_of_the_font() {
        let atlas = crate::engine::texture::FontAtlas::for_test();
        assert_ne!(atlas.place('A'), atlas.place('B'));
        // Anything the font has no glyph for lands on the placeholder
        // layer, which is visible -- the same promise `font::glyph`
        // makes.
        assert_eq!(atlas.place('\u{2603}'), (0, 0.0, 0.0));
        assert_ne!(atlas.place(' ').0, 0, "space has a place, it is just blank");
    }

    #[test]
    fn centring_never_drifts_by_more_than_the_grid_it_sits_on() {
        // Narrow end glyphs ('1', 'i') do not fill their cells, so the
        // ink is a little off centre even though the cells are not.
        // What must hold is that the error stays inside one cell --
        // anything more would be a layout bug, not a fixed-grid font.
        let cell = PIXEL * f32::from(GLYPH_WIDTH as u8);
        for text in ["Server 1", "i", "gyp", "127.0.0.1:7878"] {
            let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
            p.text_centred(text, 0.0, 0.0, 1.0, TEXT);
            let min = p.vertices.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
            let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
            assert!(
                (min + max).abs() < cell,
                "{text:?} is {} off centre, more than one cell",
                (min + max).abs()
            );
        }
    }

    #[test]
    fn the_ink_is_narrower_than_the_advance_by_exactly_one_column() {
        let advance = measure("AB", 1.0);
        assert!((advance - ink_width("AB", 1.0) - PIXEL).abs() < 1e-6);
        assert_eq!(ink_width("", 1.0), 0.0);
    }

    #[test]
    fn scaling_text_scales_it() {
        assert!(measure("HELLO", 2.0) > measure("HELLO", 1.0) * 1.9);
        assert!(line_height(2.0) > line_height(1.0));
    }

    #[test]
    fn a_field_shows_the_end_of_an_over_long_value() {
        // While typing an address, the character just entered has to be
        // visible; clipping the tail would hide it.
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        let rect = Rect::centred(0.0, 0.0, 0.4, 0.08);
        let long = "abcdefghijklmnopqrstuvwxyz0123456789";
        p.field(rect, long, true, true);
        let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        // The caret sits just past the last glyph, and both must be
        // inside the field.
        assert!(max <= rect.x1 + 0.01, "text overflowed the field to {max}");
    }

    #[test]
    fn an_unfocused_field_draws_no_caret() {
        let mut focused = Painter::new(crate::engine::texture::FontAtlas::for_test());
        focused.field(Rect::centred(0.0, 0.0, 0.4, 0.08), "ab", true, true);
        let mut blurred = Painter::new(crate::engine::texture::FontAtlas::for_test());
        blurred.field(Rect::centred(0.0, 0.0, 0.4, 0.08), "ab", false, true);
        assert!(focused.vertices.len() > blurred.vertices.len());
    }

    #[test]
    fn a_hovered_button_looks_different_from_an_idle_one() {
        let rect = Rect::centred(0.0, 0.0, 0.6, 0.09);
        let mut idle = Painter::new(crate::engine::texture::FontAtlas::for_test());
        idle.button(rect, "PLAY", false, true);
        let mut hovered = Painter::new(crate::engine::texture::FontAtlas::for_test());
        hovered.button(rect, "PLAY", true, true);
        assert_eq!(idle.vertices.len(), hovered.vertices.len());
        assert_ne!(
            idle.vertices[0].tint, hovered.vertices[0].tint,
            "hover must be visible"
        );
    }

    #[test]
    fn the_scrim_covers_a_very_wide_window() {
        // The aspect divide shrinks x; a scrim that stopped at ±1 would
        // leave the world showing down both sides of an ultrawide.
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.scrim(SCRIM);
        let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        assert!(max >= 4.0, "scrim only reaches {max}");
    }

    #[test]
    fn a_two_line_row_stays_inside_its_row() {
        // Regression: the name was centred in the top half of the row
        // and the detail in the bottom half, which only worked while
        // the font had no descenders. The cell is two rows taller than
        // the cap height, and the name started hanging out of the top.
        let rect = Rect::centred(0.0, 0.0, 1.8, 0.11);
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        // Deliberately full of descenders and tall capitals.
        p.row_labels(rect, 0.025, "Jumpy gqpy World", TEXT, "seed 9   3 d ago", TEXT_DIM, 1.0);

        let top = p.vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        let bottom = p.vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        assert!(top <= rect.y1, "text hangs {} above the row", top - rect.y1);
        assert!(bottom >= rect.y0, "text hangs {} below the row", rect.y0 - bottom);
    }

    #[test]
    fn the_two_lines_of_a_row_do_not_collide() {
        let rect = Rect::centred(0.0, 0.0, 1.8, 0.11);
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        // 'y' descends on the first line, 'd' ascends on the second.
        p.row_labels(rect, 0.025, "yyy", TEXT, "ddd", TEXT_DIM, 1.0);
        let mid = rect.centre_y();
        let upper_bottom = p
            .vertices
            .iter()
            .filter(|v| v.tint == TEXT)
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        let lower_top = p
            .vertices
            .iter()
            .filter(|v| v.tint == TEXT_DIM)
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        assert!(upper_bottom >= lower_top, "the descenders overlap the line below");
        let _ = mid;
    }

    #[test]
    fn a_long_name_is_truncated_rather_than_running_out_of_the_panel() {
        let rect = Rect::centred(0.0, 0.0, 0.6, 0.11);
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.row_labels(rect, 0.025, &"W".repeat(80), TEXT, &"a".repeat(80), TEXT_DIM, 1.0);
        let right = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        assert!(right <= rect.x1, "text runs {} past the row", right - rect.x1);
    }

    #[test]
    fn truncation_is_visible_rather_than_a_silent_cut() {
        // A name that merely stops looks like the name, and the player
        // cannot tell that this is not the server they meant.
        let wide = measure("ABCDEFGHIJ", 1.0);
        let shortened = fit("ABCDEFGHIJKLMNOP", 1.0, wide);
        assert!(shortened.ends_with(".."), "got {shortened:?}");
        assert!(measure(&shortened, 1.0) <= wide);
        // Something that already fits is left exactly alone.
        assert_eq!(fit("ABC", 1.0, wide), "ABC");
    }

    #[test]
    fn fitting_into_no_space_at_all_does_not_panic() {
        assert_eq!(fit("anything", 1.0, 0.0), "");
        assert_eq!(fit("", 1.0, 1.0), "");
    }

    #[test]
    fn a_disabled_setting_row_is_visibly_greyed() {
        let rect = Rect::centred(0.0, 0.0, 1.5, 0.1);
        let mut on = Painter::new(crate::engine::texture::FontAtlas::for_test());
        on.setting_row(rect, "FOG", "ON", true, RowStyle { text: 1.0, value: 1.0, controls: CONTROL_COLUMN });
        let mut off = Painter::new(crate::engine::texture::FontAtlas::for_test());
        off.setting_row(rect, "FOG", "ON", false, RowStyle { text: 1.0, value: 1.0, controls: CONTROL_COLUMN });
        assert_eq!(on.vertices.len(), off.vertices.len());
        assert_ne!(
            on.vertices.last().unwrap().tint,
            off.vertices.last().unwrap().tint,
            "a setting that does nothing must not look active"
        );
    }

    #[test]
    fn long_errors_wrap_instead_of_running_off_screen() {
        let long = format!("{} {} {}", "a".repeat(20), "b".repeat(20), "c".repeat(20));
        let wrapped = wrap(&long, 25);
        assert!(wrapped.len() >= 3);
        assert!(wrapped.iter().all(|line| line.chars().count() <= 25));
    }

    // ---- the skin ----

    /// Where the skin's pictures are put in these tests.
    ///
    /// Well past the font's stand-in, which is one layer a glyph from
    /// one (`FontAtlas::for_test`): a skin laid on top of those would
    /// have `dump_to_png` drawing panels as letters.
    const A_SKIN: u32 = 900;

    /// The box a painter's whole output occupies.
    fn drawn_bounds(vertices: &[HotbarVertex]) -> (f32, f32, f32, f32) {
        vertices.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(x0, y0, x1, y1), v| {
                (
                    x0.min(v.position[0]),
                    y0.min(v.position[1]),
                    x1.max(v.position[0]),
                    y1.max(v.position[1]),
                )
            },
        )
    }

    #[test]
    fn the_skin_covers_its_rectangle_and_nothing_outside_it() {
        // Rule 7 of this file, for the pictures: a widget that drew a
        // texel past its own rectangle would be a widget whose hit test
        // is a rectangle smaller than the thing a player can see, and
        // the panel beside it would be overlapped by a frame nobody
        // asked for. Every shape, at a size that makes the frame small
        // against the field and at one that makes it larger than half
        // the widget -- which is where the corners have to be squeezed.
        for rect in [
            Rect::new(-0.62, -0.38, 0.71, 0.44),
            Rect::new(-0.04, -0.011, 0.04, 0.011),
        ] {
            for piece in Piece::ALL {
                let mut p = with_skin(A_SKIN, Painter::default);
                assert!(p.nine(rect, piece, PANEL, Some(FIELD_TILE)));
                let vertices = p.into_vertices();
                assert!(!vertices.is_empty(), "{piece:?} drew nothing");
                let (x0, y0, x1, y1) = drawn_bounds(&vertices);
                for (drawn, asked, side) in [
                    (x0, rect.x0, "left"),
                    (y0, rect.y0, "bottom"),
                    (x1, rect.x1, "right"),
                    (y1, rect.y1, "top"),
                ] {
                    assert!(
                        (drawn - asked).abs() < 1e-5,
                        "{piece:?} drew its {side} edge at {drawn}, not at {asked}",
                    );
                }
            }
        }
    }

    #[test]
    fn a_button_is_drawn_on_the_same_rectangle_with_the_skin_and_without_it() {
        // The one property the whole change rests on. Every hit test in
        // this interface is a `Rect::contains` against the rectangle the
        // widget was drawn on, so as long as the pictures fill exactly
        // what the fills and bevels filled, not one of them had to
        // move -- and this is the test that says so rather than the
        // comment claiming it.
        let rect = Rect::new(-0.31, -0.062, 0.29, 0.058);
        let draw = || {
            let mut p = Painter::default();
            p.button(rect, "GO", true, true);
            drawn_bounds(&p.into_vertices())
        };
        let plain = draw();
        let skinned = with_skin(A_SKIN, draw);
        for (a, b) in [
            (plain.0, skinned.0),
            (plain.1, skinned.1),
            (plain.2, skinned.2),
            (plain.3, skinned.3),
        ] {
            assert!((a - b).abs() < 1e-5, "the skin moved a button's edge: {a} against {b}");
        }
        assert!((plain.0 - rect.x0).abs() < 1e-5, "a button is not drawn on its own rectangle");
        assert!((plain.2 - rect.x1).abs() < 1e-5, "a button is not drawn on its own rectangle");
    }

    #[test]
    fn a_cell_is_one_quad_and_it_is_the_cell() {
        // The pack draws forty of these a frame and the belt ten more,
        // so "one quad" is a cost as well as a look: it was five before
        // (a fill and four bevel edges) and the picture carries all
        // five. If this ever goes back up, the pack's vertex count goes
        // up with it.
        let cell = Rect::new(-0.05, -0.05, 0.05, 0.05);
        let mut p = with_skin(A_SKIN, Painter::default);
        p.cell(cell, WELL);
        let vertices = p.into_vertices();
        assert_eq!(vertices.len(), 6, "a cell is one quad");
        let (x0, y0, x1, y1) = drawn_bounds(&vertices);
        assert!((x0 - cell.x0).abs() < 1e-5 && (y0 - cell.y0).abs() < 1e-5);
        assert!((x1 - cell.x1).abs() < 1e-5 && (y1 - cell.y1).abs() < 1e-5);
    }

    #[test]
    fn a_tiled_field_ends_exactly_where_it_was_asked_to() {
        // A tile that rounded up to a whole one would hang a texel or
        // two past the panel -- see `spans`. Run over a span that is
        // not a whole number of tiles, which is every real panel.
        let runs = spans(-0.5, 0.42, 0.25, 0.75, Some(0.1));
        assert!(runs.len() > 1, "a span nine tiles long came back whole");
        assert!((runs[0].0 + 0.5).abs() < 1e-6, "the first tile does not start at the edge");
        let last = *runs.last().expect("a tile");
        assert!((last.1 - 0.42).abs() < 1e-6, "the last tile runs past the edge");
        let share = (last.1 - last.0) / 0.1;
        assert!(
            (last.3 - (0.25 + share * 0.5)).abs() < 1e-6,
            "the last tile is short and its picture is not",
        );
        for pair in runs.windows(2) {
            assert!((pair[0].1 - pair[1].0).abs() < 1e-6, "a seam between two tiles");
        }
    }

    #[test]
    fn every_piece_of_the_skin_is_the_picture_the_atlas_loads_for_it() {
        // The lookup is arithmetic -- base plus piece number -- so the
        // two lists are one list written twice. A picture moved in
        // either draws a button where a slot belongs, at run time, on a
        // graphics card, with nothing anywhere to say so.
        use crate::engine::texture::{EXTRA_TEXTURES, EXTRA_STRETCHED_LEATHER, EXTRA_UI_SKIN, UI_SKIN_PIECES};
        assert_eq!(UI_SKIN_PIECES, Piece::ALL.len(), "the run is not as long as the skin");
        for (index, piece) in Piece::ALL.into_iter().enumerate() {
            assert_eq!(piece.file(), EXTRA_TEXTURES[EXTRA_UI_SKIN + index], "{piece:?}");
            assert!(
                crate::embedded::texture(piece.file()).is_some(),
                "{piece:?} names {} and nothing is compiled in under it",
                piece.file(),
            );
            assert_eq!(Skin::at(100).layer(piece), Some(100 + index as u32));
            assert_eq!(Skin::NONE.layer(piece), None);
        }
        // ...and the run really is at the end of the list, which is what
        // `EXTRA_STRETCHED_LEATHER` is now derived from.
        assert_eq!(EXTRA_TEXTURES[EXTRA_STRETCHED_LEATHER], "hide/stretched_leather.png");
        assert_eq!(EXTRA_UI_SKIN + UI_SKIN_PIECES, EXTRA_TEXTURES.len());
    }

    #[test]
    fn the_skin_shades_a_surface_without_moving_the_colour_under_it() {
        // Every contrast in this file is measured against `Theme`'s own
        // numbers -- `small_text_is_readable_against_everything_it_is_drawn_on`
        // and its neighbours. Those measurements are truthful only while
        // the middle of a surface's picture leaves the theme's colour
        // where it is: a field drawn a third dark would make a panel a
        // third darker than every number in this file says it is, and
        // every one of those tests would be passing about a screen that
        // no longer exists.
        //
        // The states are deliberately not in this list. A hovered board
        // is *meant* to be lighter than its theme colour and a pressed
        // one darker -- that difference is the whole of what they say.
        let linear = |byte: u8| {
            let c = byte as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        for piece in [Piece::Panel, Piece::Tray, Piece::Well, Piece::Track, Piece::Slot, Piece::Button] {
            let bytes = crate::embedded::texture(piece.file()).expect("a skin picture");
            let picture = image::load_from_memory(bytes).expect("a png").to_rgba8();
            let middle = picture.get_pixel(picture.width() / 2, picture.height() / 2).0;
            let multiplier = linear(middle[0]) * SKIN_GAIN;
            assert!(
                (multiplier - 1.0).abs() < 0.15,
                "the middle of {} multiplies its surface by {multiplier:.2}",
                piece.file(),
            );
            assert_eq!(middle[3], 255, "{} is a surface and has to be opaque", piece.file());
        }
    }

    #[test]
    fn a_whole_screen_of_skin_stays_inside_one_upload() {
        // Tiling a panel's field costs quads, and a number written down
        // in a doc comment is a number that stops being true. The pack
        // is the biggest panel the game draws; this is what it comes to
        // with a full grid of cells on it, and it is here so that a
        // change which makes it ten times that is a red test rather
        // than a stutter somebody notices six months later.
        let mut p = with_skin(A_SKIN, Painter::default);
        p.deep_panel(Rect::new(-1.0, -0.72, 1.0, 0.72));
        for row in 0..4 {
            for column in 0..10 {
                let (x, y) = (-0.9 + column as f32 * 0.09, -0.6 + row as f32 * 0.09);
                p.cell(Rect::new(x, y, x + 0.08, y + 0.08), WELL);
            }
        }
        let quads = p.into_vertices().len() / 6;
        assert!(
            quads < 400,
            "a panel and forty cells came to {quads} quads; the field is tiling too finely",
        );
    }

}
