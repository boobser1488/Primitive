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

use crate::engine::font::{text_width, CAP_HEIGHT, GLYPH_HEIGHT, GLYPH_SPACING, GLYPH_WIDTH};
use crate::ui::hotbar::{HotbarVertex, UNTEXTURED};

/// Height of one font pixel at scale 1.0.
pub const PIXEL: f32 = 0.0052;

/// Vertical distance between consecutive lines of text at a given scale.
///
/// The full cell plus a little: the cell already includes the descender
/// rows, so consecutive lines cannot collide, and the extra is leading.
pub fn line_height(scale: f32) -> f32 {
    PIXEL * scale * (GLYPH_HEIGHT as f32 + 2.0)
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
pub fn cell_height(scale: f32) -> f32 {
    PIXEL * scale * GLYPH_HEIGHT as f32
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

pub const SCRIM: [f32; 4] = [0.03, 0.04, 0.06, 0.86];
pub const PANEL: [f32; 4] = [0.08, 0.09, 0.12, 0.94];
pub const PANEL_EDGE: [f32; 4] = [0.30, 0.34, 0.42, 1.0];
pub const BUTTON: [f32; 4] = [0.16, 0.18, 0.23, 1.0];
pub const BUTTON_HOVER: [f32; 4] = [0.26, 0.30, 0.38, 1.0];
pub const BUTTON_EDGE: [f32; 4] = [0.38, 0.43, 0.52, 1.0];
pub const ACCENT: [f32; 4] = [1.00, 0.78, 0.28, 1.0];
pub const ROW: [f32; 4] = [0.12, 0.14, 0.18, 1.0];
pub const ROW_SELECTED: [f32; 4] = [0.20, 0.26, 0.22, 1.0];
pub const FIELD: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
pub const TEXT: [f32; 4] = [0.88, 0.91, 0.95, 1.0];
pub const TEXT_DIM: [f32; 4] = [0.55, 0.59, 0.66, 1.0];
pub const TEXT_BAD: [f32; 4] = [1.00, 0.48, 0.42, 1.0];
pub const TEXT_GOOD: [f32; 4] = [0.52, 0.88, 0.55, 1.0];

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
        .map(|b| (font.layer(b as char), b as char))
        .collect();

    let aspect = width as f32 / height as f32;
    let to_pixels = |p: [f32; 2]| {
        (
            ((p[0] / aspect + 1.0) * 0.5 * width as f32).round() as i64,
            ((1.0 - p[1]) * 0.5 * height as f32).round() as i64,
        )
    };

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
                let fill = if flat { colour } else { [0.40, 0.44, 0.52, colour[3]] };
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
}

/// Turns a physical cursor position into UI coordinates.
///
/// This has to be the exact inverse of the vertex shader's aspect divide.
/// When it isn't, the mouse works at one window size and is subtly offset
/// at every other -- which is a miserable thing to debug, so it lives
/// here next to the coordinate documentation and is covered by a test.
pub fn cursor_to_ui(cursor: (f64, f64), size: (u32, u32)) -> (f32, f32) {
    let width = size.0.max(1) as f32;
    let height = size.1.max(1) as f32;
    let aspect = width / height;
    let ndc_x = (cursor.0 as f32 / width) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor.1 as f32 / height) * 2.0;
    (ndc_x * aspect, ndc_y)
}

/// Collects UI geometry.
pub struct Painter {
    pub vertices: Vec<HotbarVertex>,
    font: crate::engine::texture::FontAtlas,
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
        Self { vertices, font }
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

    /// A hollow rectangle: four thin quads rather than a filled one
    /// behind the content, so a border can sit over any background.
    pub fn border(&mut self, rect: Rect, thickness: f32, colour: [f32; 4]) {
        let t = thickness;
        self.quad(Rect::new(rect.x0 - t, rect.y1, rect.x1 + t, rect.y1 + t), colour);
        self.quad(Rect::new(rect.x0 - t, rect.y0 - t, rect.x1 + t, rect.y0), colour);
        self.quad(Rect::new(rect.x0 - t, rect.y0, rect.x0, rect.y1), colour);
        self.quad(Rect::new(rect.x1, rect.y0, rect.x1 + t, rect.y1), colour);
    }

    /// Covers the whole screen, whatever its shape.
    ///
    /// Authored deliberately far outside the visible range: the aspect
    /// divide shrinks x, so a quad that merely reached ±1 would leave
    /// bare strips down the sides of a wide window.
    pub fn scrim(&mut self, colour: [f32; 4]) {
        self.quad(Rect::new(-8.0, -8.0, 8.0, 8.0), colour);
    }

    /// Tiles a block texture across the whole screen.
    ///
    /// The menu's wallpaper. Off by default, because a wall of texture
    /// behind small text is a legibility cost the player should opt into
    /// rather than be handed -- and because at night the sky already
    /// shows through, which some people prefer.
    ///
    /// `aspect` is needed here and nowhere else in this module: the tile
    /// grid is the one thing that has to cover the *window* rather than
    /// occupy a fixed part of it, so it is the one thing that cares how
    /// wide the window is.
    pub fn block_background(&mut self, layer: u32, aspect: f32, tint: [f32; 4]) {
        /// Tile size in UI units. Twelve tiles down a screen: small
        /// enough to read as a wall of blocks, large enough that a
        /// 16x16 texture isn't being asked to be a texture at 4 pixels.
        const TILE: f32 = 2.0 / 12.0;

        let half_width = aspect.max(0.1) + TILE;
        let columns = (half_width * 2.0 / TILE).ceil() as i32;
        let rows = (2.0 / TILE).ceil() as i32 + 1;

        for row in 0..rows {
            for column in 0..columns {
                let x0 = -half_width + column as f32 * TILE;
                let y0 = -1.0 + row as f32 * TILE;
                let rect = Rect::new(x0, y0, x0 + TILE, y0 + TILE);
                for (position, uv) in [
                    ([rect.x0, rect.y0], [0.0, 1.0]),
                    ([rect.x1, rect.y0], [1.0, 1.0]),
                    ([rect.x1, rect.y1], [1.0, 0.0]),
                    ([rect.x0, rect.y0], [0.0, 1.0]),
                    ([rect.x1, rect.y1], [1.0, 0.0]),
                    ([rect.x0, rect.y1], [0.0, 0.0]),
                ] {
                    self.vertices.push(HotbarVertex {
                        position,
                        uv,
                        tex_layer: layer,
                        tint,
                    });
                }
            }
        }
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
    pub fn row_labels(
        &mut self,
        rect: Rect,
        pad: f32,
        name: &str,
        name_colour: [f32; 4],
        detail: &str,
        detail_colour: [f32; 4],
    ) {
        const NAME_SCALE: f32 = 1.0;
        const DETAIL_SCALE: f32 = 0.8;
        const GAP: f32 = 0.005;

        let name_cell = cell_height(NAME_SCALE);
        let detail_cell = cell_height(DETAIL_SCALE);
        let block = name_cell + GAP + detail_cell;

        // Top of the block, centred vertically in the row.
        let top = rect.centre_y() + block / 2.0;
        let available = (rect.x1 - rect.x0 - pad * 2.0).max(0.0);

        self.text(
            &fit(name, NAME_SCALE, available),
            rect.x0 + pad,
            top,
            NAME_SCALE,
            name_colour,
        );
        self.text(
            &fit(detail, DETAIL_SCALE, available),
            rect.x0 + pad,
            top - name_cell - GAP,
            DETAIL_SCALE,
            detail_colour,
        );
    }

    pub fn panel(&mut self, rect: Rect) {
        self.quad(rect, PANEL);
        self.border(rect, 0.004, PANEL_EDGE);
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
        const SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.38];
        const OFFSET: f32 = 0.014;
        const TOP: [f32; 4] = [0.13, 0.145, 0.185, 0.97];
        const BOTTOM: [f32; 4] = [0.055, 0.062, 0.085, 0.97];
        const INNER_EDGE: [f32; 4] = [0.42, 0.47, 0.58, 0.55];

        self.quad(
            Rect::new(
                rect.x0 + OFFSET,
                rect.y0 - OFFSET,
                rect.x1 + OFFSET,
                rect.y1 - OFFSET,
            ),
            SHADOW,
        );
        self.vgradient(rect, TOP, BOTTOM);
        self.border(rect, 0.005, [0.02, 0.02, 0.03, 1.0]);
        // Inset by its own thickness, so the two edges sit side by side
        // rather than one on top of the other.
        let inner = Rect::new(
            rect.x0 + 0.004,
            rect.y0 + 0.004,
            rect.x1 - 0.004,
            rect.y1 - 0.004,
        );
        self.border(inner, 0.0015, INNER_EDGE);
    }

    /// A title bar across the top of a panel: a band, a rule under it,
    /// and the heading in the accent colour.
    ///
    /// Returns where the content below it may start.
    pub fn panel_header(&mut self, panel: Rect, title: &str, height: f32) -> f32 {
        const BAND_TOP: [f32; 4] = [0.20, 0.23, 0.30, 1.0];
        const BAND_BOTTOM: [f32; 4] = [0.11, 0.125, 0.16, 1.0];

        let band = Rect::new(
            panel.x0 + 0.006,
            panel.y1 - height,
            panel.x1 - 0.006,
            panel.y1 - 0.006,
        );
        self.vgradient(band, BAND_TOP, BAND_BOTTOM);
        // The rule is the accent colour rather than the panel edge: it
        // is the one line on the screen that says which way is up.
        self.quad(
            Rect::new(band.x0, band.y0, band.x1, band.y0 + 0.0022),
            [ACCENT[0], ACCENT[1], ACCENT[2], 0.75],
        );
        let cap = PIXEL * 1.05 * CAP_HEIGHT as f32;
        self.text(
            title,
            band.x0 + 0.018,
            band.centre_y() + cap / 2.0,
            1.05,
            ACCENT,
        );
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
        let px = PIXEL * scale;
        let advance = (GLYPH_WIDTH + GLYPH_SPACING) as f32 * px;
        let (w, h) = (GLYPH_WIDTH as f32 * px, GLYPH_HEIGHT as f32 * px);
        let mut x = left;

        for c in text.chars() {
            // A space has nothing lit, so drawing it is a quad that can
            // only cost bandwidth. Text is mostly spaces in a column of
            // aligned labels.
            if c != ' ' {
                let layer = self.font.layer(c);
                let (u, v) = (self.font.u_max, self.font.v_max);
                let rect = Rect::new(x, top - h, x + w, top);
                for (position, uv) in [
                    ([rect.x0, rect.y0], [0.0, v]),
                    ([rect.x1, rect.y0], [u, v]),
                    ([rect.x1, rect.y1], [u, 0.0]),
                    ([rect.x0, rect.y0], [0.0, v]),
                    ([rect.x1, rect.y1], [u, 0.0]),
                    ([rect.x0, rect.y1], [0.0, 0.0]),
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
        let fill = if !enabled {
            [0.11, 0.12, 0.15, 1.0]
        } else if hovered {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        self.quad(rect, fill);
        self.border(rect, 0.003, if hovered && enabled { ACCENT } else { BUTTON_EDGE });
        let colour = if enabled { TEXT } else { TEXT_DIM };
        self.label_in(rect, text, 1.0, colour);
    }

    /// One row of the settings screen: a label on the left, the current
    /// value on the right, and the widgets that change it in between.
    ///
    /// Drawn as a row rather than a dialog per setting because the whole
    /// point of a settings screen is seeing what everything is set to at
    /// once -- that is the thing a config file is bad at.
    pub fn setting_row(&mut self, rect: Rect, label: &str, value: &str, enabled: bool) {
        self.quad(rect, ROW);
        let (label_colour, value_colour) = if enabled {
            (TEXT, ACCENT)
        } else {
            (TEXT_DIM, TEXT_DIM)
        };
        // Text scaled to the row rather than fixed, so a screen with
        // enough settings on it to make the rows short does not have
        // its labels standing taller than the bars they sit in.
        // `ROW_DESIGN_HEIGHT` is the height the size below was chosen
        // for; anything shorter scales down in proportion.
        const ROW_DESIGN_HEIGHT: f32 = 0.105;
        let scale = (rect.height() / ROW_DESIGN_HEIGHT).min(1.0);
        self.label_left(rect, label, 0.025, scale, label_colour);
        let width = measure(value, scale);
        // Right-aligned, clear of the buttons the caller draws after.
        self.label_left(
            Rect::new(rect.x1 - width - 0.30, rect.y0, rect.x1, rect.y1),
            value,
            0.0,
            scale,
            value_colour,
        );
    }

    /// A single-line text field. `caret` shows the insertion point; it is
    /// drawn only when the field has focus, which is the only cue the
    /// player gets about where typing will go.
    pub fn field(&mut self, rect: Rect, text: &str, focused: bool, caret: bool) {
        self.quad(rect, FIELD);
        self.border(rect, 0.003, if focused { ACCENT } else { BUTTON_EDGE });

        let pad = 0.018;
        let scale = 1.0;
        // Show the tail of an over-long value rather than the head: what
        // matters while typing is the end you are typing at.
        let usable = (rect.x1 - rect.x0 - pad * 2.0).max(0.0);
        let mut shown = text;
        while measure(shown, scale) > usable && !shown.is_empty() {
            shown = &shown[shown.char_indices().nth(1).map(|(i, _)| i).unwrap_or(shown.len())..];
        }
        self.label_left(rect, shown, pad, scale, TEXT);

        if focused && caret {
            let x = rect.x0 + pad + measure(shown, scale) + PIXEL * 0.5;
            let half = PIXEL * scale * CAP_HEIGHT as f32 / 2.0;
            self.quad(
                Rect::new(x, rect.centre_y() - half, x + PIXEL * scale, rect.centre_y() + half),
                ACCENT,
            );
        }
    }
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
    use super::*;

    #[test]
    fn the_cursor_maps_to_where_the_geometry_is_drawn() {
        // The centre of the window is the centre of UI space, whatever
        // the aspect ratio.
        for size in [(1280u32, 720u32), (800, 800), (3440, 1440)] {
            let (x, y) = cursor_to_ui((size.0 as f64 / 2.0, size.1 as f64 / 2.0), size);
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
        let (x, _) = cursor_to_ui((pixel_x, 450.0), size);
        assert!((x - 0.5).abs() < 1e-4, "expected 0.5, got {x}");
    }

    #[test]
    fn the_top_left_of_the_window_is_the_top_left_of_ui_space() {
        let (x, y) = cursor_to_ui((0.0, 0.0), (1280, 720));
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
    fn every_character_samples_its_own_layer() {
        let atlas = crate::engine::texture::FontAtlas::for_test();
        assert_ne!(atlas.layer('A'), atlas.layer('B'));
        // Anything the font has no glyph for lands on the placeholder,
        // which is visible -- the same promise `font::glyph` makes.
        assert_eq!(atlas.layer('\u{2603}'), 0);
        assert_ne!(atlas.layer(' '), 0, "space has a layer, it is just blank");
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
        p.row_labels(rect, 0.025, "Jumpy gqpy World", TEXT, "seed 9   3 d ago", TEXT_DIM);

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
        p.row_labels(rect, 0.025, "yyy", TEXT, "ddd", TEXT_DIM);
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
        p.row_labels(rect, 0.025, &"W".repeat(80), TEXT, &"a".repeat(80), TEXT_DIM);
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
    fn the_wallpaper_reaches_both_edges_of_any_window() {
        // It has to cover the window, not a fixed part of it -- the one
        // thing in this module that cares how wide the window is.
        for aspect in [0.6f32, 1.0, 1.78, 3.5] {
            let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
            p.block_background(0, aspect, [1.0, 1.0, 1.0, 1.0]);
            let min = p.vertices.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
            let max = p.vertices.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
            let top = p.vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            let bottom = p.vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            assert!(min <= -aspect, "aspect {aspect}: left edge bare at {min}");
            assert!(max >= aspect, "aspect {aspect}: right edge bare at {max}");
            assert!(bottom <= -1.0 && top >= 1.0, "aspect {aspect}: vertical gap");
        }
    }

    #[test]
    fn the_wallpaper_is_textured_rather_than_flat_colour() {
        // Flat-tinted quads use `UNTEXTURED`; these have to sample the
        // block texture or the setting does nothing visible.
        let mut p = Painter::new(crate::engine::texture::FontAtlas::for_test());
        p.block_background(7, 1.78, [1.0, 1.0, 1.0, 1.0]);
        assert!(p.vertices.iter().all(|v| v.tex_layer == 7));
        // And every tile must span the whole texture, or the blocks
        // come out sliced.
        let us: Vec<f32> = p.vertices.iter().take(6).map(|v| v.uv[0]).collect();
        assert!(us.contains(&0.0) && us.contains(&1.0));
    }

    #[test]
    fn a_disabled_setting_row_is_visibly_greyed() {
        let rect = Rect::centred(0.0, 0.0, 1.5, 0.1);
        let mut on = Painter::new(crate::engine::texture::FontAtlas::for_test());
        on.setting_row(rect, "FOG", "ON", true);
        let mut off = Painter::new(crate::engine::texture::FontAtlas::for_test());
        off.setting_row(rect, "FOG", "ON", false);
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
}
