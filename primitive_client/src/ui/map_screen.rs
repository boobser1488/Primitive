//! The map: the land this player has seen, from above.
//!
//! ## North is up, and it does not turn
//!
//! The map could rotate with the player, the way a car's navigation
//! does. It does not, because what a map in this game is *for* is the
//! walk back -- to a bag, to a base, to the spawn -- and a walk back is
//! planned against landmarks that stay where they are: the lake is north
//! of the hill whichever way you were facing when you looked. The arrow
//! turns instead. There is no compass on the HUD any more to answer "which
//! way do I turn now" (see the note in `journal`): that is read off this
//! map, against the land -- with north found off the sky, or off a water
//! compass in the hand once there is iron (`logic::bearing`).
//!
//! ## What a cell is
//!
//! One block, while blocks are big enough to see. Zoomed out past that,
//! a cell is two, four, eight blocks -- **a power of two, lined up with
//! the world's own grid**, and drawn from the column at its corner. Any
//! other sampling shimmers: a cell that took whichever column happened to
//! fall under its middle would take a different column every time the map
//! moved by less than a cell, and panning would make the coast boil.
//!
//! Runs of one colour along a row are one quad. Land is mostly runs, and
//! that is what keeps a zoomed-out map to a few thousand quads rather
//! than tens of thousands; `drawing_a_screen_of_land_stays_a_few_thousand_quads`
//! says so.
//!
//! ## Buttons *and* a pinch, and why the old argument stopped holding
//!
//! This used to say "buttons and not a pinch", and the reason given was
//! that two fingers were already spoken for: `touch::Pointer` reads the
//! second one as the shift modifier, so a pinch would have been a third
//! meaning for one gesture. That argument was about a screen the journal
//! is not on. The map is dragged in two directions and `Pointer` hands
//! out vertical scrolls only, so the journal never went through it -- it
//! reads the raw finger itself (see `journal::Journal::touch`), and a
//! second finger there was *dropped on the floor*. Nothing is being taken
//! away from anything by giving it the meaning every map on the phone
//! already has.
//!
//! `+`, `-` and the wheel stay exactly as they were: a pinch is not
//! available to a mouse, and a player who wants one step of zoom should
//! not have to make a gesture to get it.
//!
//! ## Why the pinch does not rotate the map
//!
//! Two fingers can say "turn", and this one refuses to hear it, because
//! north is up and stays up (see the top of this file). What it reads is
//! the two numbers a map needs: how far apart the fingers are, and where
//! the middle of them is. See [`Pinch`].

use crate::logic::map::{ExploredMap, Ground, Landmarks};
use crate::ui::hotbar::{HotbarVertex, UNTEXTURED};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// Where the player is and which way they face, in world units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerMark {
    pub x: f32,
    pub z: f32,
    /// The camera's yaw: forward is `(cos yaw, sin yaw)` on the ground.
    pub yaw: f32,
}

/// Interface units per block, closest and furthest.
///
/// Out to a block about a pixel across on a 720-line window, which shows
/// a walk of a kilometre and a half end to end; in to a block the size of
/// a finger tip, which is close enough to count the trees on a ridge.
pub const MIN_SCALE: f32 = 0.0025;
pub const MAX_SCALE: f32 = 0.08;
/// What a fresh map opens at: about a hundred and forty blocks top to
/// bottom, which is the view distance and then some.
const DEFAULT_SCALE: f32 = 0.012;
/// How small a cell may be drawn before cells start standing for more
/// than one block. About five pixels on a 720-line window.
const MIN_CELL: f32 = 0.014;
/// How much one press of `+` or one line of the wheel zooms.
const ZOOM_STEP: f32 = 1.5;
/// A ceiling on cells sampled for one picture, whatever the window. A
/// phone held sideways is twice as wide as a monitor in interface units,
/// and it is the one with the weakest processor.
const MAX_CELLS: f32 = 40_000.0;

const UNSEEN: [f32; 4] = [0.07, 0.065, 0.06, 1.0];
const FRAME: [f32; 4] = [0.30, 0.27, 0.22, 1.0];
const PLAYER: [f32; 4] = [1.0, 0.86, 0.30, 1.0];
const BAG: [f32; 4] = [0.90, 0.22, 0.16, 1.0];
const SPAWN: [f32; 4] = [0.96, 0.96, 0.96, 1.0];
const OUTLINE: [f32; 4] = [0.0, 0.0, 0.0, 0.9];
const CAIRN: [f32; 4] = [0.80, 0.78, 0.72, 1.0];
/// What a cairn's name is written on, so it reads over snow and forest
/// alike.
const LABEL_PLATE: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
/// How big a cairn's name is written.
const LABEL_SCALE: f32 = 0.7;

/// A button laid over the corner of the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    ZoomIn,
    ZoomOut,
    /// Back to following the player.
    Centre,
}

pub const CONTROLS: [Control; 3] = [Control::ZoomIn, Control::ZoomOut, Control::Centre];

/// Where one of the map's buttons is, inside the area the map is drawn
/// in. One definition, for the drawing and the hit-test both.
pub fn control_rect(control: Control, body: Rect) -> Rect {
    let side = widgets::tappable(0.10);
    let gap = 0.016;
    let index = CONTROLS.iter().position(|c| *c == control).unwrap_or(0) as f32;
    let x1 = body.x1 - gap;
    let y1 = body.y1 - gap - index * (side + gap);
    Rect::new(x1 - side, y1 - side, x1, y1)
}

/// Which button a point is over.
pub fn control_at(at: (f32, f32), body: Rect) -> Option<Control> {
    CONTROLS
        .into_iter()
        .find(|control| control_rect(*control, body).contains(at.0, at.1))
}

/// Which way the player's arrow points on the map, as a unit vector in
/// interface space: x to the right, y up the screen.
///
/// Screen x is world x and screen up is world *north*, which is -z -- so
/// the ground's forward `(cos yaw, sin yaw)` is drawn as `(cos, -sin)`.
pub fn heading(yaw: f32) -> (f32, f32) {
    (yaw.cos(), -yaw.sin())
}

/// A column's colour, lit from the north.
///
/// A column higher than the one north of it faces the light and is drawn
/// brighter; lower, darker. That one comparison is what turns a field of
/// flat colours into hills and valleys, and it is why the height is kept
/// at all. Water is left flat -- a lake is level whatever its bed does.
///
/// Stepped in twenty-fifths rather than continuous, so two neighbouring
/// columns on a gentle slope come out the same colour and share a quad.
pub fn shade(ground: Ground, height: u8, north: u8) -> [f32; 4] {
    let [r, g, b] = ground.colour();
    if ground == Ground::Water {
        return [r, g, b, 1.0];
    }
    let rise = (height as f32 - north as f32) * 0.07;
    let factor = 1.0 + (rise.clamp(-0.28, 0.28) * 25.0).round() / 25.0;
    [(r * factor).min(1.0), (g * factor).min(1.0), (b * factor).min(1.0), 1.0]
}

/// Two fingers on the map, as the only two numbers a pinch is made of.
///
/// Not the two points: a pair of points also carries an angle, and this
/// map does not turn (see the module docs). Reducing them here means the
/// view cannot accidentally be handed one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pinch {
    /// Half way between the fingers, in interface units.
    pub mid: (f32, f32),
    /// How far apart they are.
    pub span: f32,
}

impl Pinch {
    pub fn between(one: (f32, f32), two: (f32, f32)) -> Self {
        Self {
            mid: ((one.0 + two.0) / 2.0, (one.1 + two.1) / 2.0),
            span: (one.0 - two.0).hypot(one.1 - two.1),
        }
    }
}

/// How close two fingers may be and still be two fingers.
///
/// A hundredth of the interface's own height. Below that the span is
/// mostly the noise of two fingertips rolling, and the ratio of two
/// noises is a zoom that jumps about by a factor of three between
/// frames -- which is what a pinch reads as when it starts from a
/// double-tap that did not quite land as one.
const MIN_SPAN: f32 = 0.01;

/// What the map is looking at, and how closely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    /// The world point in the middle of the map, or `None` to follow the
    /// player -- which is where it opens, and where `ME` puts it back.
    centre: Option<(f32, f32)>,
    scale: f32,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            centre: None,
            scale: DEFAULT_SCALE,
        }
    }
}

impl MapView {
    pub fn centre(&self, player: PlayerMark) -> (f32, f32) {
        self.centre.unwrap_or((player.x, player.z))
    }

    #[cfg(test)]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn is_following(&self) -> bool {
        self.centre.is_none()
    }

    /// Where a world point is drawn.
    pub fn to_screen(self, body: Rect, player: PlayerMark, world: (f32, f32)) -> (f32, f32) {
        let (cx, cz) = self.centre(player);
        (
            body.centre_x() + (world.0 - cx) * self.scale,
            body.centre_y() - (world.1 - cz) * self.scale,
        )
    }

    /// Where on the ground a point on the map is.
    pub fn to_world(self, body: Rect, player: PlayerMark, at: (f32, f32)) -> (f32, f32) {
        let (cx, cz) = self.centre(player);
        (
            cx + (at.0 - body.centre_x()) / self.scale,
            cz - (at.1 - body.centre_y()) / self.scale,
        )
    }

    /// Moves the land with a pointer that moved by `(dx, dy)`.
    ///
    /// The land follows the finger, the way paper does. It stops following
    /// the player the moment it is dragged: a map that snapped back to the
    /// player every frame could never be used to look at anywhere else.
    pub fn drag_by(&mut self, dx: f32, dy: f32, player: PlayerMark) {
        let (cx, cz) = self.centre(player);
        self.centre = Some((cx - dx / self.scale, cz + dy / self.scale));
    }

    /// Zooms by `factor`, keeping the ground under `about` where it is.
    ///
    /// About the pointer rather than the middle, because the thing a player
    /// zooms towards is the thing they are pointing at; zooming about the
    /// middle slides that thing off towards the edge and makes them chase
    /// it. With no pointer -- a button, a key -- the middle is what there is.
    pub fn zoom(&mut self, factor: f32, about: Option<(f32, f32)>, body: Rect, player: PlayerMark) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let about = about.unwrap_or((body.centre_x(), body.centre_y()));
        let anchored = self.to_world(body, player, about);
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        if (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        // Only move the centre if it had been moved already, or the zoom
        // was about somewhere other than the middle: zooming a map that is
        // following the player about the player keeps it following.
        if self.centre.is_some() || about != (body.centre_x(), body.centre_y()) {
            self.centre = Some((
                anchored.0 - (about.0 - body.centre_x()) / scale,
                anchored.1 + (about.1 - body.centre_y()) / scale,
            ));
        }
    }

    /// What pressing a button does.
    pub fn press(&mut self, control: Control, body: Rect, player: PlayerMark) {
        match control {
            Control::ZoomIn => self.zoom(ZOOM_STEP, None, body, player),
            Control::ZoomOut => self.zoom(1.0 / ZOOM_STEP, None, body, player),
            Control::Centre => self.centre = None,
        }
    }

    /// The wheel, about the pointer.
    pub fn wheel(&mut self, lines: f32, about: Option<(f32, f32)>, body: Rect, player: PlayerMark) {
        // **Away from the player is closer in, and it used to be the
        // other way round.** The sign was taken from a comment that said
        // "wheel away from the player is negative", and that is simply
        // not what arrives: `platform::Event::MouseWheel` says positive
        // is away, the backend passes winit's line delta through
        // untouched, and the hotbar's own handler reads exactly the same
        // convention two arms further up (`forward = lines < 0.0`). So
        // one notch away from the player zoomed *out*, against every
        // other map anybody has used, and the player asked for it back.
        //
        // Clamped because a trackpad can report a dozen lines in one
        // event, and a dozen steps of 1.5 is the whole zoom range in one
        // flick.
        self.zoom(ZOOM_STEP.powf(lines.clamp(-4.0, 4.0)), about, body, player);
    }

    /// Two fingers moved: the land follows both of them.
    ///
    /// ## What a pinch is allowed to say
    ///
    /// Two fingers on a plane say four things -- move, scale, turn, and
    /// shear -- and this map hears the first two. Turning is refused
    /// because north is up (see the top of this file) and shear is not a
    /// gesture. So a pinch is read as the two numbers in [`Pinch`]: the
    /// distance between the fingers, which is the scale, and the point
    /// half way between them, which is where that scale happens.
    ///
    /// ## Why the middle is the anchor
    ///
    /// Because with rotation refused, *both* fingers can only be kept on
    /// the ground they started on while the line between them keeps its
    /// direction -- and a real hand turns it by a few degrees on every
    /// pinch. Something has to give, and what gives is the ends: the
    /// ground under the middle of the fingers is exactly where it was,
    /// and the two fingers slide by whatever the turn was worth. The
    /// alternative -- anchoring one finger and letting the other take the
    /// whole error -- makes the map lurch under whichever finger the
    /// player happened to move second.
    ///
    /// A pinch stops the map following the player, for the reason
    /// [`drag_by`](Self::drag_by) gives: it is a drag as well as a zoom,
    /// and a map that snapped back to the player could not be used to
    /// look anywhere else.
    pub fn pinch(&mut self, before: Pinch, after: Pinch, body: Rect, player: PlayerMark) {
        // Two fingers on the same spot have no distance to scale by, and
        // dividing by it would put a NaN in the centre -- which is a map
        // that never draws again, for the rest of the session.
        if !(before.span > MIN_SPAN && after.span > MIN_SPAN) {
            return;
        }
        // The ground under the old middle, read through the same
        // inverse the drawing uses, before the scale changes under it.
        let anchored = self.to_world(body, player, before.mid);
        let scale = (self.scale * after.span / before.span).clamp(MIN_SCALE, MAX_SCALE);
        if !scale.is_finite() {
            return;
        }
        self.scale = scale;
        self.centre = Some((
            anchored.0 - (after.mid.0 - body.centre_x()) / scale,
            anchored.1 + (after.mid.1 - body.centre_y()) / scale,
        ));
    }

    /// A fingerprint of what the view would draw.
    pub fn key(&self) -> (Option<(u32, u32)>, u32) {
        (
            self.centre.map(|(x, z)| (x.to_bits(), z.to_bits())),
            self.scale.to_bits(),
        )
    }
}

/// How many blocks one cell stands for at this scale: the smallest power
/// of two that is at least `MIN_CELL` across. See the module docs.
pub fn step_for(scale: f32) -> i32 {
    let mut step = 1;
    while (step as f32) * scale < MIN_CELL && step < 1 << 12 {
        step *= 2;
    }
    step
}

/// The land inside `body`, as quads: one per run of one colour along a
/// row of cells.
///
/// A pure function of the map and the view, so the tests can ask what is
/// drawn without reading vertices back out of a painter.
pub fn land_quads(view: &MapView, map: &ExploredMap, player: PlayerMark, body: Rect) -> Vec<(Rect, [f32; 4])> {
    let mut quads = Vec::new();
    if map.surveyed() == 0 {
        return quads;
    }
    let scale = view.scale;
    let (cx, cz) = view.centre(player);
    let half_w = body.width() / 2.0 / scale;
    let half_h = body.height() / 2.0 / scale;
    let mut step = step_for(scale);
    while (half_w * 2.0 / step as f32) * (half_h * 2.0 / step as f32) > MAX_CELLS {
        step *= 2;
    }
    let cell = step as f32 * scale;

    let first_x = ((cx - half_w).floor() as i32).div_euclid(step) * step;
    let last_x = (cx + half_w).ceil() as i32;
    let first_z = ((cz - half_h).floor() as i32).div_euclid(step) * step;
    let last_z = (cz + half_h).ceil() as i32;

    let colour_at = |gx: i32, gz: i32| -> Option<[f32; 4]> {
        let (ground, height) = map.at(gx, gz)?;
        let north = map.at(gx, gz - step).map_or(height, |(_, h)| h);
        Some(shade(ground, height, north))
    };

    let mut gz = first_z;
    while gz <= last_z {
        // The row's top edge is its northern edge, which is its smaller z.
        let top = body.centre_y() - (gz as f32 - cz) * scale;
        let (y0, y1) = ((top - cell).max(body.y0), top.min(body.y1));
        if y1 > y0 {
            let mut run: Option<(f32, [f32; 4])> = None;
            let mut gx = first_x;
            while gx <= last_x + step {
                let x = body.centre_x() + (gx as f32 - cx) * scale;
                let colour = if gx <= last_x { colour_at(gx, gz) } else { None };
                if run.map(|(_, c)| Some(c)) != Some(colour) {
                    if let Some((start, c)) = run.take() {
                        let (x0, x1) = (start.max(body.x0), x.min(body.x1));
                        if x1 > x0 {
                            quads.push((Rect::new(x0, y0, x1, y1), c));
                        }
                    }
                    run = colour.map(|c| (x, c));
                }
                gx += step;
            }
        }
        gz += step;
    }
    quads
}

/// One triangle, counter-clockwise whatever order it was given in -- the
/// interface pipeline is set up for the winding its quads have, and a
/// triangle wound the other way is a triangle that might not be there.
fn triangle(p: &mut Painter, a: (f32, f32), b: (f32, f32), c: (f32, f32), tint: [f32; 4]) {
    let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
    let (b, c) = if area < 0.0 { (c, b) } else { (b, c) };
    for point in [a, b, c] {
        p.vertices.push(HotbarVertex {
            position: [point.0, point.1],
            uv: [0.0, 0.0],
            tex_layer: UNTEXTURED,
            tint,
        });
    }
}

/// An arrow at `at` pointing along `dir`, with a dark copy behind it so it
/// reads over snow and over forest alike.
///
/// **Notched, not a plain triangle.** It was a triangle first, and a
/// picture of it at the size it is drawn settled that: a triangle whose
/// three corners are nearly equally far apart has no front, and the eye
/// reads whichever corner is on top as the point -- so a player facing
/// south-east saw an arrow pointing north. A notch in the back leaves one
/// corner that can only be the tip.
pub fn arrow(p: &mut Painter, at: (f32, f32), dir: (f32, f32), size: f32, tint: [f32; 4]) {
    let side = (-dir.1, dir.0);
    let point = |along: f32, across: f32, grow: f32| {
        (
            at.0 + (dir.0 * along + side.0 * across) * size * grow,
            at.1 + (dir.1 * along + side.1 * across) * size * grow,
        )
    };
    for (grow, colour) in [(1.35, OUTLINE), (1.0, tint)] {
        let tip = point(1.0, 0.0, grow);
        let notch = point(-0.3, 0.0, grow);
        triangle(p, tip, point(-0.75, 0.62, grow), notch, colour);
        triangle(p, tip, notch, point(-0.75, -0.62, grow), colour);
    }
}

/// A marked square: a bag, the spawn.
fn marker(p: &mut Painter, at: (f32, f32), size: f32, tint: [f32; 4]) {
    p.quad(Rect::centred(at.0, at.1, size * 1.5, size * 1.5), OUTLINE);
    p.quad(Rect::centred(at.0, at.1, size, size), tint);
}

/// A cairn: three stones stacked, a wide one, a smaller, a smallest --
/// the shape the thing in the world is, so the legend is hardly needed.
fn cairn(p: &mut Painter, at: (f32, f32), size: f32) {
    let stones = [(1.0, -0.33), (0.7, 0.0), (0.4, 0.3)];
    for (width, up) in stones {
        p.quad(Rect::centred(at.0, at.1 + up * size, size * width + 0.006, size * 0.36 + 0.006), OUTLINE);
    }
    for (width, up) in stones {
        p.quad(Rect::centred(at.0, at.1 + up * size, size * width, size * 0.36), CAIRN);
    }
}

/// Draws the map into `body`.
#[allow(clippy::too_many_arguments)] // a view, the land, three marks, a place, a pointer, a language
pub fn paint(
    p: &mut Painter,
    view: &MapView,
    map: &ExploredMap,
    player: PlayerMark,
    landmarks: &Landmarks,
    body: Rect,
    cursor: Option<(f32, f32)>,
    language: Language,
) {
    p.quad(body, UNSEEN);
    for (rect, colour) in land_quads(view, map, player, body) {
        p.quad(rect, colour);
    }

    // Marks are kept inside the frame: a bag off the edge of the map is
    // drawn at the edge, smaller, on the line towards it -- which is more
    // use than not drawing it at all.
    let inset = 0.03;
    let place = |world: (f32, f32)| -> ((f32, f32), bool) {
        let (x, y) = view.to_screen(body, player, world);
        let inside = x > body.x0 + inset && x < body.x1 - inset && y > body.y0 + inset && y < body.y1 - inset;
        (
            (x.clamp(body.x0 + inset, body.x1 - inset), y.clamp(body.y0 + inset, body.y1 - inset)),
            inside,
        )
    };
    if let Some(spawn) = landmarks.spawn {
        let (at, inside) = place((spawn.0 as f32 + 0.5, spawn.2 as f32 + 0.5));
        marker(p, at, if inside { 0.022 } else { 0.014 }, SPAWN);
    }
    for bag in &landmarks.bags {
        let (at, inside) = place((bag.0 as f32 + 0.5, bag.2 as f32 + 0.5));
        marker(p, at, if inside { 0.028 } else { 0.018 }, BAG);
    }
    // **The cairns, only where they are on the map.** A bag off the edge
    // is drawn at the edge because it is the one place a player must get
    // back to; cairns are many, and a frame lined with every one of them
    // is a frame nobody can read. Named ones carry their name beside them,
    // on a plate, clipped at the frame.
    for (cell, name) in map.marks() {
        let (at, inside) = place((cell.0 as f32 + 0.5, cell.2 as f32 + 0.5));
        if !inside {
            continue;
        }
        cairn(p, at, 0.03);
        if name.is_empty() {
            continue;
        }
        let left = at.0 + 0.025;
        let text = widgets::fit(name, LABEL_SCALE, (body.x1 - inset - left).max(0.0));
        if text.is_empty() {
            continue;
        }
        let cap = widgets::PIXEL * LABEL_SCALE * crate::engine::font::CAP_HEIGHT as f32;
        let ink = widgets::ink_width(&text, LABEL_SCALE);
        p.quad(Rect::new(left - 0.006, at.1 - cap / 2.0 - 0.008, left + ink + 0.006, at.1 + cap / 2.0 + 0.008), LABEL_PLATE);
        p.text(&text, left, at.1 + cap / 2.0, LABEL_SCALE, widgets::TEXT);
    }
    let (at, _) = place((player.x, player.z));
    arrow(p, at, heading(player.yaw), 0.034, PLAYER);

    if map.surveyed() == 0 {
        let text = widgets::fit(language.text(Msg::MapUnexplored), 0.9, body.width() - 0.3);
        p.text_centred(&text, body.centre_x(), body.centre_y() + 0.1, 0.9, widgets::TEXT_DIM);
    }

    for control in CONTROLS {
        let rect = control_rect(control, body);
        let hovered = cursor.is_some_and(|(x, y)| rect.contains(x, y));
        let label = match control {
            Control::ZoomIn => "+",
            Control::ZoomOut => "-",
            Control::Centre => language.text(Msg::MapCentre),
        };
        // The follow button is lit while the map is following, so it says
        // what state the map is in as well as offering to change it.
        let lit = hovered || (control == Control::Centre && view.is_following());
        p.button(rect, label, lit, true);
    }

    p.border(body, 0.005, FRAME);
}

/// The legend along the bottom of the journal: four marks and their
/// names, left to right from `left`.
pub fn legend(p: &mut Painter, left: f32, middle: f32, language: Language) {
    let scale = 0.8;
    let mut x = left;
    let mut entry = |p: &mut Painter, draw: &dyn Fn(&mut Painter, (f32, f32)), text: &str| {
        draw(p, (x + 0.015, middle));
        let cap = widgets::PIXEL * scale * crate::engine::font::CAP_HEIGHT as f32;
        p.text(text, x + 0.045, middle + cap / 2.0, scale, widgets::TEXT_DIM);
        x += 0.07 + widgets::measure(text, scale);
    };
    entry(p, &|p, at| arrow(p, at, (0.0, 1.0), 0.02, PLAYER), language.text(Msg::MapYou));
    entry(p, &|p, at| marker(p, at, 0.018, BAG), language.text(Msg::MapBag));
    entry(p, &|p, at| marker(p, at, 0.016, SPAWN), language.text(Msg::MapSpawn));
    entry(p, &|p, at| cairn(p, at, 0.024), language.text(Msg::MapCairn));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::map::Tile;
    use primitive_shared::types::ChunkPos;

    fn body() -> Rect {
        Rect::new(-1.6, -0.8, 1.6, 0.7)
    }

    fn standing_at(x: f32, z: f32) -> PlayerMark {
        PlayerMark { x, z, yaw: 0.0 }
    }

    #[test]
    fn every_map_button_is_pressed_where_it_is_drawn() {
        for aspect_body in [body(), Rect::new(-0.9, -0.8, 0.9, 0.7), Rect::new(-2.3, -0.8, 2.3, 0.7)] {
            for control in CONTROLS {
                let rect = control_rect(control, aspect_body);
                assert_eq!(control_at((rect.centre_x(), rect.centre_y()), aspect_body), Some(control));
                assert!(rect.x0 > aspect_body.x0 && rect.x1 < aspect_body.x1, "{control:?} left the map");
                assert!(rect.y0 > aspect_body.y0 && rect.y1 < aspect_body.y1, "{control:?} left the map");
            }
            for (i, a) in CONTROLS.iter().enumerate() {
                for b in &CONTROLS[i + 1..] {
                    let (ra, rb) = (control_rect(*a, aspect_body), control_rect(*b, aspect_body));
                    assert!(ra.y1 <= rb.y0 || rb.y1 <= ra.y0, "{a:?} and {b:?} overlap");
                }
            }
        }
    }

    #[test]
    fn the_map_buttons_are_a_finger_wide_on_a_phone() {
        widgets::as_a_phone(|| {
            for control in CONTROLS {
                let rect = control_rect(control, body());
                assert!(rect.width() >= widgets::FINGER_SIDE - 1e-5, "{control:?} is {} wide", rect.width());
            }
        });
    }

    #[test]
    fn unexplored_land_is_drawn_as_nothing_at_all() {
        let map = ExploredMap::new();
        assert!(land_quads(&MapView::default(), &map, standing_at(0.0, 0.0), body()).is_empty());
    }

    #[test]
    fn north_is_up_the_screen_and_east_is_to_the_right() {
        let mut map = ExploredMap::new();
        // One chunk north of a player standing in the middle of chunk
        // (0, 0), and one to the east.
        map.insert(ChunkPos::new(0, -1), Tile::uniform(Ground::Water, 60));
        map.insert(ChunkPos::new(1, 0), Tile::uniform(Ground::Snow, 90));
        let player = standing_at(8.0, 8.0);
        let quads = land_quads(&MapView::default(), &map, player, body());
        let water = Ground::Water.colour();
        let middle = (body().centre_x(), body().centre_y());
        let (mut north, mut east) = (false, false);
        for (rect, colour) in &quads {
            if colour[..3] == water {
                assert!(rect.y0 >= middle.1 - 1e-4, "the chunk to the north was drawn below the player");
                north = true;
            } else {
                assert!(rect.x0 >= middle.0 - 1e-4, "the chunk to the east was drawn left of the player");
                east = true;
            }
        }
        assert!(north && east, "a chunk that has been seen was not drawn");
    }

    #[test]
    fn dragging_moves_the_land_with_the_pointer() {
        let mut view = MapView::default();
        let player = standing_at(100.0, -40.0);
        let lake = (130.0, -60.0);
        let before = view.to_screen(body(), player, lake);
        view.drag_by(0.25, -0.1, player);
        let after = view.to_screen(body(), player, lake);
        assert!((after.0 - before.0 - 0.25).abs() < 1e-3, "moved {} across for a drag of 0.25", after.0 - before.0);
        assert!((after.1 - before.1 + 0.1).abs() < 1e-3, "moved {} up for a drag of -0.1", after.1 - before.1);
        assert!(!view.is_following(), "a dragged map snapped back to the player");
    }

    #[test]
    fn zooming_keeps_the_ground_under_the_pointer_where_it_is() {
        let mut view = MapView::default();
        let player = standing_at(12.0, 7.0);
        let pointer = (0.9, -0.3);
        let under = view.to_world(body(), player, pointer);
        view.wheel(-2.0, Some(pointer), body(), player);
        let after = view.to_screen(body(), player, under);
        assert!((after.0 - pointer.0).abs() < 1e-3 && (after.1 - pointer.1).abs() < 1e-3, "{after:?}");
    }

    /// One notch of the wheel away from the player brings the land
    /// closer, the way it does on every other map there is.
    ///
    /// **It went the other way for as long as the map existed**, and the
    /// reason was a comment rather than an event: `wheel` said "away
    /// from the player is negative", while `platform::Event::MouseWheel`
    /// says positive is away and the hotbar two arms up in `lib.rs`
    /// reads it that way. Nothing had ever asserted the direction --
    /// `zooming_keeps_the_ground_under_the_pointer_where_it_is` passes
    /// whichever way round it is -- so the sign lived on a comment.
    #[test]
    fn pushing_the_wheel_away_from_the_player_brings_the_map_closer() {
        let player = standing_at(0.0, 0.0);
        let mut away = MapView::default();
        away.wheel(1.0, None, body(), player);
        assert!(
            away.scale() > DEFAULT_SCALE,
            "a notch away from the player zoomed out, to {}",
            away.scale(),
        );
        let mut back = MapView::default();
        back.wheel(-1.0, None, body(), player);
        assert!(
            back.scale() < DEFAULT_SCALE,
            "a notch back towards the player zoomed in, to {}",
            back.scale(),
        );
    }

    /// Two fingers spreading is closer in, and closing is further out.
    #[test]
    fn spreading_two_fingers_brings_the_land_closer_and_closing_them_takes_it_away() {
        let player = standing_at(40.0, 40.0);
        let centre = (body().centre_x(), body().centre_y());
        let apart = |span: f32| {
            Pinch::between((centre.0 - span / 2.0, centre.1), (centre.0 + span / 2.0, centre.1))
        };

        let mut view = MapView::default();
        view.pinch(apart(0.4), apart(0.8), body(), player);
        assert!(
            (view.scale() / DEFAULT_SCALE - 2.0).abs() < 1e-3,
            "fingers twice as far apart gave {} rather than twice {DEFAULT_SCALE}",
            view.scale(),
        );

        let mut closing = MapView::default();
        closing.pinch(apart(0.8), apart(0.4), body(), player);
        assert!(
            (closing.scale() / DEFAULT_SCALE - 0.5).abs() < 1e-3,
            "fingers half as far apart gave {}",
            closing.scale(),
        );
    }

    /// The ground half way between the fingers does not move while they
    /// do, which is what makes a pinch feel like paper rather than like
    /// a slider.
    #[test]
    fn the_ground_between_two_pinching_fingers_stays_between_them() {
        let player = standing_at(-120.0, 64.0);
        let before = Pinch::between((-0.5, -0.2), (0.3, 0.4));
        let after = Pinch::between((-0.7, -0.3), (0.9, 0.5));
        let mut view = MapView::default();
        // Already dragged somewhere, because a map that is following the
        // player has a centre it can silently fall back to.
        view.drag_by(0.3, -0.2, player);
        let under = view.to_world(body(), player, before.mid);
        view.pinch(before, after, body(), player);
        let drawn = view.to_screen(body(), player, under);
        assert!(
            (drawn.0 - after.mid.0).abs() < 1e-3 && (drawn.1 - after.mid.1).abs() < 1e-3,
            "the ground under the middle of the pinch moved from {:?} to {drawn:?}",
            after.mid,
        );
    }

    /// Two fingers landing on the same point do not divide by nothing.
    ///
    /// A double tap that arrives as two touches a pixel apart used to be
    /// a span of about zero, and the scale is multiplied by a ratio of
    /// spans: the map's centre came out NaN and nothing was ever drawn
    /// again for the rest of the session.
    #[test]
    fn two_fingers_in_one_place_are_not_an_infinite_zoom() {
        let player = standing_at(0.0, 0.0);
        let mut view = MapView::default();
        let pinched = Pinch::between((0.1, 0.1), (0.1, 0.1));
        view.pinch(pinched, Pinch::between((0.0, 0.0), (0.6, 0.0)), body(), player);
        assert!((view.scale() - DEFAULT_SCALE).abs() < 1e-9, "{}", view.scale());
        view.pinch(Pinch::between((0.0, 0.0), (0.6, 0.0)), pinched, body(), player);
        assert!(view.scale().is_finite() && view.centre(player).0.is_finite());
    }

    /// A pinch that asks for more than the map has stops where the
    /// buttons stop, rather than running off to a scale nothing draws at.
    #[test]
    fn a_pinch_cannot_push_the_scale_past_either_end() {
        let player = standing_at(0.0, 0.0);
        let mut view = MapView::default();
        for _ in 0..20 {
            view.pinch(
                Pinch::between((-0.1, 0.0), (0.1, 0.0)),
                Pinch::between((-0.4, 0.0), (0.4, 0.0)),
                body(),
                player,
            );
        }
        assert!((view.scale() - MAX_SCALE).abs() < 1e-6, "{}", view.scale());
        for _ in 0..40 {
            view.pinch(
                Pinch::between((-0.4, 0.0), (0.4, 0.0)),
                Pinch::between((-0.1, 0.0), (0.1, 0.0)),
                body(),
                player,
            );
        }
        assert!((view.scale() - MIN_SCALE).abs() < 1e-6, "{}", view.scale());
    }

    #[test]
    fn zooming_stops_at_both_ends() {
        let mut view = MapView::default();
        for _ in 0..40 {
            view.press(Control::ZoomIn, body(), PlayerMark::default());
        }
        assert!((view.scale() - MAX_SCALE).abs() < 1e-6);
        for _ in 0..80 {
            view.press(Control::ZoomOut, body(), PlayerMark::default());
        }
        assert!((view.scale() - MIN_SCALE).abs() < 1e-6);
    }

    #[test]
    fn the_arrow_points_the_way_the_player_walks() {
        // The camera walks along (cos yaw, sin yaw) on the ground; on the
        // map that is x to the right and z *down* the screen.
        for yaw in [0.0f32, 0.7, 1.9, 3.1, -2.2] {
            let (sx, sy) = heading(yaw);
            let (wx, wz) = (yaw.cos(), yaw.sin());
            let player = standing_at(0.0, 0.0);
            let view = MapView::default();
            let ahead = view.to_screen(body(), player, (wx * 10.0, wz * 10.0));
            let here = view.to_screen(body(), player, (0.0, 0.0));
            let drawn = (ahead.0 - here.0, ahead.1 - here.1);
            let length = drawn.0.hypot(drawn.1);
            assert!((drawn.0 / length - sx).abs() < 1e-3 && (drawn.1 / length - sy).abs() < 1e-3, "yaw {yaw}");
        }
    }

    #[test]
    fn following_the_player_survives_a_zoom_from_the_buttons() {
        let mut view = MapView::default();
        view.press(Control::ZoomIn, body(), standing_at(5.0, 5.0));
        assert!(view.is_following());
        view.drag_by(0.1, 0.0, standing_at(5.0, 5.0));
        view.press(Control::Centre, body(), standing_at(5.0, 5.0));
        assert!(view.is_following(), "ME did not put the map back on the player");
    }

    #[test]
    fn drawing_a_screen_of_land_stays_a_few_thousand_quads() {
        // A bad case: every column seen, and the ground changing every
        // eight blocks along both axes so a run cannot outlast half a
        // chunk. **Not a column checkerboard**: cells are sampled at a
        // power-of-two grid, so a pattern that alternates every column
        // is sampled at one parity and draws as one colour -- which is
        // what the first version of this test did, and it measured
        // seventy-six quads.
        let mut map = ExploredMap::new();
        for cx in -60..60 {
            for cz in -40..40 {
                let mut tile = Tile::uniform(Ground::Grass, 64);
                for lx in 0..16 {
                    for lz in 0..16 {
                        if (lx / 8 + lz / 8 + i32::rem_euclid(cx + cz, 2) as usize).is_multiple_of(2) {
                            tile.set(lx, lz, Ground::Rock, 70 + (lx % 3) as u8);
                        }
                    }
                }
                map.insert(ChunkPos::new(cx, cz), tile);
            }
        }
        let wide = Rect::new(-2.3, -0.8, 2.3, 0.7);
        for zoom_outs in [0, 3, 6, 20] {
            let mut view = MapView::default();
            for _ in 0..zoom_outs {
                view.press(Control::ZoomOut, body(), PlayerMark::default());
            }
            let started = std::time::Instant::now();
            let quads = land_quads(&view, &map, PlayerMark::default(), wide);
            println!(
                "[map] {} quads in {:?}, {} blocks a cell",
                quads.len(),
                started.elapsed(),
                step_for(view.scale())
            );
            assert!(quads.len() as f32 <= MAX_CELLS, "{} quads", quads.len());
        }
    }

    #[test]
    fn a_slope_facing_north_is_brighter_than_one_facing_south() {
        let up = shade(Ground::Grass, 70, 66);
        let down = shade(Ground::Grass, 66, 70);
        assert!(up[1] > down[1]);
        assert_eq!(shade(Ground::Water, 70, 60), shade(Ground::Water, 60, 70), "a lake was shaded as a hill");
    }
}
