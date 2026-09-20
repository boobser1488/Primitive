//! The in-world heads-up display: health, stack counts, death screen.
//!
//! Everything here emits `HotbarVertex` and goes into the same UI buffer
//! as the hotbar and the menus, so it costs no extra pipeline and stacks
//! in the order it is appended.
//!
//! Coordinates are the UI space described in `ui`: y runs -1..1 and x is
//! divided by the aspect ratio in the shader, so widths are in units of
//! screen *height*.


use crate::ui::hotbar::{slot_centre, HotbarVertex, BOTTOM, SLOT};
use crate::logic::inventory::Inventory;
use crate::engine::texture::FontAtlas;
use crate::ui::widgets::{self, Painter, Rect};

// ---- health bar ----
//
// Built around one idea, which solves a problem twenty separate
// hearts do not. Its bar is a single notched gauge lying across the
// top of the hotbar with the exact figure written in a well at the end
// of it, so it answers two different questions at once: the fill
// answers "how bad is it" without being read, and the number answers
// "how much exactly" when that matters. Neither requires counting
// icons.
//
// The notches are the part worth copying. An unbroken strip is hard to
// judge -- half and two thirds look much the same at this size -- and
// discrete segments turn it back into something countable at a glance
// without spending twenty icons' worth of screen.

/// Segments the gauge is divided into.
const SEGMENTS: usize = 10;

/// How thick the line round a gauge is.
///
/// One number, because it used to be two -- `0.0035` on the health bar
/// and `0.002` on every strip under it -- and the strips were therefore
/// a hair wider than the gauge they hang beneath.
const BAR_EDGE_WIDTH: f32 = 0.0035;

/// Width of every gauge here: **the hotbar's, exactly.**
///
/// It used to be `0.46`, a hair under half of this, and every strip in
/// this file was that wide. The stack then had to be centred against
/// the hotbar by hand -- and even centred correctly it read as a small
/// thing floating over a wide one, with half the bar's width carrying
/// nothing at all. The player's own words for the fix: put the strips
/// over *all* the quick slots.
///
/// The hairline is taken off both ends rather than added to them,
/// because [`Painter::border`](widgets::Painter::border) draws outside
/// the rectangle it is given: a track spanning `LEFT..RIGHT` would be
/// bordered from `LEFT - 0.0035` to `RIGHT + 0.0035` and the gauges
/// would hang a hairline off each end of the thing they are drawn
/// over. This way what a player sees ends exactly where the bar ends.
const BAR_WIDTH: f32 =
    crate::ui::hotbar::RIGHT - crate::ui::hotbar::LEFT - 2.0 * BAR_EDGE_WIDTH;

/// Left edge of every gauge.
///
/// **Centring is no longer a calculation, which is the point.** This
/// used to be `-ASSEMBLY_WIDTH / 2.0` -- half the width of the gauge
/// plus its figure, negated -- and before that it was a number picked
/// by eye that put the whole cluster a sixth of the screen out past the
/// left end of the bar. A stack that is exactly as wide as the hotbar
/// starts where the hotbar starts and cannot be off centre unless the
/// hotbar is.
const BAR_LEFT: f32 = crate::ui::hotbar::LEFT + BAR_EDGE_WIDTH;

const BAR_HEIGHT: f32 = 0.034;
/// Gap between segments, as a fraction of one segment's pitch.
const SEGMENT_GAP: f32 = 0.16;
/// Air between the marked part of a meter and the well at its end.
const READOUT_GAP: f32 = 0.020;

/// The column at the right-hand end of the stack that the figures live
/// in, and which no meter marks itself in.
///
/// **A fixed reserve rather than the width of today's number.** The
/// figure is written from the left edge of this column, so measuring
/// the string and right-aligning it instead would move the number every
/// time health crossed from 9 to 10 -- a heads-up display that twitches
/// when you are hurt, which is the one moment it has to be still.
///
/// Wide enough for `20/20` at the readout's own scale with room to
/// breathe either side, and *checked* rather than trusted: see
/// `the_room_kept_for_the_readout_actually_holds_it`. A server with a
/// larger maximum no longer has spare width to run into -- the stack
/// ends at the end of the hotbar -- so its figure is lettered smaller
/// instead, once, against the widest string that maximum can produce.
const READOUT_RESERVE: f32 = 0.134;

/// Air inside a well, left and right of the figure.
const READOUT_PAD: f32 = 0.006;

/// ...and above and below it.
///
/// Small, and it has to be. The degrees are written on a strip only
/// `STAMINA_HEIGHT` tall, so their well stands proud of it and eats
/// into the gap to the next strip; a generous pad here is the
/// temperature reading drawn on the breath meter. See
/// `a_figure_stays_inside_the_gap_around_its_strip`.
const WELL_PAD: f32 = 0.002;

/// Left edge of that column, which is also where every meter's own
/// marks stop.
const READOUT_LEFT: f32 = BAR_LEFT + BAR_WIDTH - READOUT_RESERVE;

/// How much of the width a meter's marks get: everything but the
/// readout column and the air before it.
const METER_SPAN: f32 = BAR_WIDTH - READOUT_RESERVE - READOUT_GAP;

/// The ground a figure is printed on.
///
/// **Opaque, and that is the whole reason it exists.** The figures used
/// to be written over the world with a one-pixel drop shadow, which is
/// what [`widgets::Painter::label_in_two_tones`] does for thumb
/// controls -- and that argument holds for *large* marks at 3:1, not
/// for a five-character number at 4.5:1. Measured on the composite: the
/// pale glyphs over snow come to 1.06:1 and the shadow is doing all of
/// the work, which is the trick working; what the trick cannot do is
/// the middle of the range, where neither stroke is far from the world.
/// Over grass the better of the two is 3.96:1 and over a mid-tone stone
/// it bottoms out at about 3.4:1 -- both short of the floor, and both
/// in the colours a player actually stands on.
///
/// A solid plate measures 9.63:1 whatever is behind it. The same colour
/// as the track, so on the health gauge it is invisible: it is the
/// recess the segments already sit in, merely not letting the world
/// through. See
/// `a_figure_is_printed_on_something_it_can_be_read_on`.
const READOUT_WELL: [f32; 4] = [0.05, 0.05, 0.06, 1.0];

// The gauges are hung over the hotbar, so they have to fit on it, and
// now they fit it exactly. All three are relations between constants,
// which makes the build the right place for them to fail rather than
// the test suite -- the same argument `hotbar` makes about its own two.
const _: () = assert!(BAR_LEFT - BAR_EDGE_WIDTH >= crate::ui::hotbar::LEFT - 1e-6);
// The figures included -- the marks alone fitting says nothing about
// whether the number at the end hangs off it.
const _: () =
    assert!(BAR_LEFT + BAR_WIDTH + BAR_EDGE_WIDTH <= crate::ui::hotbar::RIGHT + 1e-6);
// ...and the readout column cannot be allowed to eat the meter.
const _: () = assert!(METER_SPAN > BAR_WIDTH / 2.0);
/// Where the health gauge sits, and with it everything else here.
///
/// **Derived rather than chosen, and that is the fix.** It used to be
/// `BOTTOM + SLOT + 0.052` -- a number picked by eye against the top of
/// the *slots*, which is not where the hotbar ends: its backdrop reaches
/// a further [`hotbar::PAD`] past them. With two strips hung under the
/// gauge the lower one landed four thousandths above the slots and ten
/// thousandths *inside* the backdrop, so the hunger bar was drawn on the
/// bar it was supposed to be sitting over. The guard test did not catch
/// it because it measured against the same wrong line.
///
/// So the gauge is now placed by what has to fit under it: the strips,
/// their gaps, and a clearance that reads as a gap rather than as two
/// things touching. Add a third strip below and this moves up on its
/// own.
const BAR_Y: f32 = HOTBAR_TOP
    + CLEARANCE
    + STAMINA_HEIGHT
    + (BANDS_BELOW as f32) * (STAMINA_HEIGHT + STAMINA_GAP)
    - STAMINA_HEIGHT;

/// Air between the lowest strip and the top of the hotbar.
const CLEARANCE: f32 = 0.012;

/// The recess the segments sit in.
const BAR_TRACK: [f32; 4] = [0.05, 0.05, 0.06, 0.88];
const BAR_EDGE: [f32; 4] = [0.55, 0.50, 0.42, 0.95];
/// An unfilled segment: present, but clearly spent.
const SEGMENT_EMPTY: [f32; 4] = [0.13, 0.11, 0.11, 0.90];
/// A highlight along the top of a filled segment.
const SEGMENT_GLOSS: [f32; 4] = [1.0, 1.0, 1.0, 0.18];
/// Air left. Blue, because it is the one gauge that is about water, and
/// nothing else on this screen is that colour.
const BREATH_FILL: [f32; 4] = [0.45, 0.72, 0.95, 0.95];
/// The ghost of health just lost, drained away over a moment.
const BAR_RECENT: [f32; 4] = [0.95, 0.83, 0.30, 0.80];

const HEALTH_TEXT_SCALE: f32 = 0.80;
const HEALTH_TEXT: [f32; 4] = [0.94, 0.92, 0.86, 1.0];

/// Writes a figure into the well at the right-hand end of a meter.
///
/// One function for both readings, for the reason `band_above` and
/// `band_below` are one function each: the two figures have to sit in
/// the same column, and two pieces of code spelling out the same left
/// margin are two chances for it to stop being the same. Both used to
/// write out `track.x1 + READOUT_GAP` and a baseline of their own, and
/// they agreed only because the two tracks happened to end at the same
/// x -- which they no longer would if one meter were ever shortened.
///
/// `widest` is the longest string this reading can ever produce, and
/// the size is fitted to *that* rather than to `label`. Fitting the
/// label itself would letter `9/20` bigger than `10/20` and the number
/// would change size as the player was hurt -- the same twitch
/// [`READOUT_RESERVE`] exists to prevent, arriving by the other door.
fn readout(painter: &mut Painter, track: Rect, wanted: f32, widest: &str, label: &str) {
    let scale = widgets::fitted_scale(
        widest,
        wanted,
        READOUT_RESERVE - 2.0 * READOUT_PAD,
        // Half rather than `widgets`' two thirds: this is a number, not
        // a word, and a small number is still readable where a small
        // label has stopped being one. It only bites on a server whose
        // maximum health runs to three digits.
        0.5,
    );
    // Centred on the cap height, not the cell: two of the nine rows are
    // descender space, empty in every string that reaches here, and
    // counting them sits the figure visibly low in its well.
    let cap = widgets::PIXEL * scale * crate::engine::font::CAP_HEIGHT as f32;
    // As tall as the meter it belongs to, and taller only where that is
    // not enough. On the health gauge the writing is a shade shorter
    // than the track: a well sized to the writing alone would lay its
    // hairline four ten-thousandths inside the track's own -- no
    // doubled edge at any pixel density anyone has, but a rectangle
    // that is *almost* the track, which is the near-miss that becomes a
    // visible one the first time either number is touched. Taking the
    // larger makes them the same rectangle by construction. The
    // temperature strip is the other case: too thin for its own
    // degrees, so that well stands proud of it.
    let centre_y = track.centre_y();
    let half = (cap / 2.0 + WELL_PAD).max(track.height().abs() / 2.0);
    let well = Rect::new(
        READOUT_LEFT,
        centre_y - half,
        READOUT_LEFT + READOUT_RESERVE,
        centre_y + half,
    );
    painter.quad(well, READOUT_WELL);
    // The same hairline every track carries, so the well reads as one
    // more piece of the same instrument rather than as a black sticker
    // laid over it. Without this the degrees -- whose well stands proud
    // of the strip it belongs to -- looked like a label from a
    // different program.
    painter.border(well, BAR_EDGE_WIDTH, BAR_EDGE);
    painter.text(
        label,
        well.x0 + READOUT_PAD,
        centre_y + cap / 2.0,
        scale,
        HEALTH_TEXT,
    );
}

// ---- stamina ----
//
// A plain strip under the health gauge rather than a second notched
// bar. Stamina is continuous and nobody needs to read an exact figure
// off it -- what matters is whether there is any left, and the eye gets
// that from a length. Making it look different from health is also the
// point: two identical gauges side by side get confused for each other.

const STAMINA_HEIGHT: f32 = 0.014;
const STAMINA_GAP: f32 = 0.010;

// ---- where the thin meters go ----
//
// **Every strip is now placed by one of these two functions, and that is
// a fix rather than a tidy-up.** Each meter used to spell out its own
// offset -- `BAR_Y - GAP - HEIGHT - GAP`, and the next one down added
// two more terms -- so adding a fourth strip below the health gauge put
// it *inside the hotbar*, which is exactly what happened when the water
// bar arrived. An arithmetic expression written out four times is four
// chances to be off by one strip, and nothing catches it but looking.
//
// Bands are numbered outward from the health gauge: band 0 is the first
// strip on that side, band 1 the next, and so on. The test below asserts
// that nothing lands on the hotbar and that no two strips overlap.

/// The strip `n` places *below* the health gauge, going down.
const fn band_below(n: usize) -> (f32, f32) {
    let top = BAR_Y - STAMINA_GAP - (n as f32) * (STAMINA_HEIGHT + STAMINA_GAP);
    (top - STAMINA_HEIGHT, top)
}

/// ...and `n` places *above* it, going up.
const fn band_above(n: usize) -> (f32, f32) {
    let bottom = BAR_Y + BAR_HEIGHT + STAMINA_GAP + (n as f32) * (STAMINA_HEIGHT + STAMINA_GAP);
    (bottom, bottom + STAMINA_HEIGHT)
}

/// The top of the hotbar, which is the floor nothing may cross.
///
/// The bar's own figure, backdrop and all -- see [`hotbar::TOP`]. Taken
/// from there rather than written down again here, because the version
/// written down again here is the one that was wrong.
const HOTBAR_TOP: f32 = crate::ui::hotbar::TOP;

/// How many strips hang under the health gauge.
///
/// Two: stamina and hunger. A third would push the whole assembly up by
/// itself now (see [`BAR_Y`]) rather than being quietly drawn on the
/// hotbar, which is what happened to the second one.
const BANDS_BELOW: usize = 2;

/// ...and how many sit above it.
///
/// Four: water, breath, the temperature scale and tiredness.
/// Named for the same reason `BANDS_BELOW` is -- the guard test
/// used to write `0..3` in its own body, so a band added above
/// would have been checked by a test that did not know it
/// existed. Tiredness is that band, and this line is where the
/// test was told.
const BANDS_ABOVE: usize = 4;

// ---- what each strip is
//
// **The complaint this answers, in the player's words: "что за полоска
// под температурой".** They were looking at the water bar. Of the six
// meters exactly one said what it was -- temperature, because degrees
// are written beside it -- and health half said so with a figure. Four
// coloured strips carried no letter, no mark, and no explanation
// anywhere in the game.
//
// **Why a pictogram and not a word.** A word beside every strip is four
// labels glued to the corner of the screen for the whole of a session,
// in a HUD whose entire design is that it keeps quiet and gets out of
// the way -- it fades out altogether when nothing is wrong (see
// `Attention`). A mark is a fifth of the width, has no language, and is
// the thing this genre already teaches: a heart is health and a drop is
// water before anybody explains it. The two that are not obvious -- the
// bolt and the thermometer -- are named on the pause screen, which is
// where a player who is confused already goes. See `GAUGE_LEGEND`.
//
// **Why five by five and not something prettier.** Because of the room
// there is, which is a fact rather than a taste: a thin strip is
// `STAMINA_HEIGHT` tall with `STAMINA_GAP` under it, so a mark beside
// one cannot be taller than their sum without touching the mark on the
// strip above. That is twelve pixels at 1080p, about two and a half per
// cell at five cells across -- coarser than the font's own glyphs at
// this size, and the most that fits.

/// How many cells across (and down) a mark is.
const ICON_GRID: usize = 5;

/// A mark beside a meter, as rows of cells read from the top.
///
/// Five bits per row, most significant leftmost -- so the picture in the
/// source is the picture on the screen, which is the only thing that
/// makes these readable as source at all.
///
/// Public because the pause screen draws the same six beside their
/// names. **One table rather than two pictures**: a legend drawn from
/// its own copy of the artwork is a legend that stops being true the
/// first time a mark is redrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon(pub [u8; ICON_GRID]);

/// Health: a heart, which is what a heart has meant in this genre for
/// forty years.
pub const ICON_HEALTH: Icon = Icon([0b01010, 0b11111, 0b11111, 0b01110, 0b00100]);
/// Stamina: a bolt. Not obvious on its own, and named in the legend.
pub const ICON_STAMINA: Icon = Icon([0b00011, 0b00110, 0b01111, 0b00100, 0b01000]);
/// Hunger: a bowl with something in it.
///
/// **Wide at the top and narrow at the bottom, which is the whole
/// reason it is a bowl and not a haunch.** The first cut of this was a
/// joint of meat, and at five cells a joint is a blob with a stem --
/// which is also what a drop is, and what a thermometer is. Three of
/// the six marks were the same picture. A bowl is the drop turned
/// upside down, and two silhouettes that are inverses of each other
/// cannot be confused at any size.
pub const ICON_FOOD: Icon = Icon([0b00000, 0b10001, 0b11111, 0b11111, 0b01110]);
/// Thirst: a drop.
pub const ICON_WATER: Icon = Icon([0b00100, 0b00100, 0b01110, 0b11111, 0b01110]);
/// Air: a bubble with a smaller one leaving it. **Hollow on purpose** --
/// a filled circle at this size is a dot, and a dot is not a bubble.
pub const ICON_AIR: Icon = Icon([0b01100, 0b10010, 0b10010, 0b01100, 0b00001]);
/// Rest: a crescent moon. **Not a bed** -- a bed at five pixels
/// across is a grey smudge, and the one thing every player already
/// reads as sleep is the moon.
pub const ICON_REST: Icon = Icon([0b01110, 0b11100, 0b11000, 0b11100, 0b01110]);
/// Temperature: a thermometer, tube and bulb.
pub const ICON_WARMTH: Icon = Icon([0b00100, 0b01010, 0b01010, 0b11111, 0b01110]);

/// The seven meters, **in the order they are stacked on screen**, top
/// first.
///
/// The order is a fact about the layout -- `band_above(2)` down to
/// `band_below(1)` -- and it is written out here because the pause
/// screen prints this list and a player reading it has to be able to
/// lay it against what they can see. A legend in a different order from
/// the thing it explains is worse than no legend at all.
pub const GAUGE_LEGEND: &[(Icon, crate::ui::lang::Msg)] = &[
    (ICON_WARMTH, crate::ui::lang::Msg::GaugeWarmth),
    (ICON_REST, crate::ui::lang::Msg::GaugeRest),
    (ICON_AIR, crate::ui::lang::Msg::GaugeAir),
    (ICON_WATER, crate::ui::lang::Msg::GaugeWater),
    (ICON_HEALTH, crate::ui::lang::Msg::GaugeHealth),
    (ICON_STAMINA, crate::ui::lang::Msg::GaugeStamina),
    (ICON_FOOD, crate::ui::lang::Msg::GaugeFood),
];

/// How tall -- and wide -- a mark beside a meter is drawn.
///
/// The pitch of the thin strips, less a hair. Any larger and the mark on
/// one strip touches the mark on the next: they are stacked at
/// `STAMINA_HEIGHT + STAMINA_GAP` and each is centred on its own strip.
const ICON_SIZE: f32 = 0.022;

/// Air between a mark and the meter it belongs to.
const ICON_GAP: f32 = 0.009;

// The two things that can go wrong with a column of marks, asserted
// where the numbers are rather than in a test: they can touch each
// other, and the lowest can reach the hotbar.
const _: () = assert!(ICON_SIZE < STAMINA_HEIGHT + STAMINA_GAP);
const _: () = assert!(CLEARANCE + STAMINA_HEIGHT / 2.0 - ICON_SIZE / 2.0 > 0.0);

/// The pale half of a mark, and the dark half under it.
///
/// **A drop shadow rather than one colour**, on exactly the terms
/// `stack_counts` uses for the numbers on the hotbar: a mark sits over
/// the world -- there is no track behind it the way there is behind a
/// strip -- and one stroke has one brightness while the world has all of
/// them. A pale stroke disappears into snow and a dark one into a cave;
/// the pair cannot both.
const ICON_INK: [f32; 4] = [0.90, 0.90, 0.88, 0.95];
const ICON_SHADOW: [f32; 4] = [0.02, 0.02, 0.03, 0.85];

/// Draws one mark, centred on a point.
///
/// Public because the pause screen's legend draws the same marks. A run
/// of set cells in a row becomes one quad rather than one per cell,
/// which is most of the difference between a mark costing five quads and
/// twenty-five.
pub fn draw_icon(painter: &mut Painter, icon: Icon, centre: (f32, f32), size: f32) {
    draw_icon_inked(painter, icon, centre, size, ICON_INK);
}

/// ...in a colour of its own, over the same shadow. For the two marks that
/// are *about* colour -- a drop of blood is red, and a drop of water drawn
/// in the same pale ink as it would say "thirsty".
pub fn draw_icon_inked(painter: &mut Painter, icon: Icon, centre: (f32, f32), size: f32, ink: [f32; 4]) {
    let cell = size / ICON_GRID as f32;
    let left = centre.0 - size / 2.0;
    let top = centre.1 + size / 2.0;
    // The whole shadow first and the whole mark over it, rather than
    // both per row: interleaved, the dark half of one row is drawn on
    // top of the pale half of the row above it.
    // Half a cell, not a whole one. A whole cell at the size a mark is
    // actually drawn -- twelve pixels for five cells -- offsets the
    // shadow by two and a half of them, and what the eye reads is not a
    // shadow but a second, blurred copy of the mark.
    let shadow = cell * 0.5;
    for (ink, offset) in [(ICON_SHADOW, shadow), (ink, 0.0)] {
        for (row, bits) in icon.0.iter().enumerate() {
            let y1 = top - cell * row as f32 - offset;
            let y0 = y1 - cell;
            let mut column = 0;
            while column < ICON_GRID {
                if bits & (1 << (ICON_GRID - 1 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < ICON_GRID && bits & (1 << (ICON_GRID - 1 - column)) != 0 {
                    column += 1;
                }
                painter.quad(
                    Rect::new(
                        left + cell * start as f32 + offset,
                        y0,
                        left + cell * column as f32 + offset,
                        y1,
                    ),
                    ink,
                );
            }
        }
    }
}

/// ...and where a meter's own mark goes: left of the stack, centred on
/// the strip.
///
/// **Drawn by the function that draws the meter**, which is the rule
/// this whole file is built on and matters most here: breath and
/// temperature come and go, and a mark for a strip that is not on screen
/// would be a picture of nothing, pointing at nothing.
fn band_icon(painter: &mut Painter, track: Rect, icon: Icon) {
    draw_icon(
        painter,
        icon,
        (BAR_LEFT - ICON_GAP - ICON_SIZE / 2.0, track.centre_y()),
        ICON_SIZE,
    );
}

/// A drop of blood, while a cut is bleeding. The water's drop, in red --
/// see `draw_icon_inked` for why one shape can say both.
const ICON_BLEEDING: Icon = ICON_WATER;
const BLEEDING_INK: [f32; 4] = [0.90, 0.14, 0.12, 0.98];
/// A bone, while one is broken: two knuckles and a shaft, on the diagonal.
const ICON_BROKEN: Icon = Icon([0b11000, 0b11100, 0b01110, 0b00111, 0b00011]);

/// What is wrong with the body that is still costing something, as marks
/// to the right of the health bar.
///
/// **Two marks, and only while they are true.** The mannequin in the pack
/// is where a player reads their wounds; out in the world the question is
/// only "is something happening to me that I have to stop" -- a cut that
/// bleeds, a bone that slows the walk or the swing. A bruise, a light burn
/// and a dressed cut are none of those, and a HUD that marked every one of
/// them would be a HUD a player learns to ignore before the mark that
/// matters arrives.
///
/// To the *right* of the bar because its left is the column of meter marks,
/// and a mark for a state drawn in the column of marks for meters would be
/// read as the label of a meter that is not there.
pub fn wound_marks(painter: &mut Painter, injuries: &primitive_shared::injury::Injuries) {
    let y = BAR_Y + BAR_HEIGHT / 2.0;
    let mut x = BAR_LEFT + BAR_WIDTH + ICON_GAP + ICON_SIZE / 2.0;
    if injuries.is_bleeding() {
        draw_icon_inked(painter, ICON_BLEEDING, (x, y), ICON_SIZE, BLEEDING_INK);
        x += ICON_SIZE + ICON_GAP;
    }
    if injuries.leg_broken() || injuries.arm_broken() {
        draw_icon(painter, ICON_BROKEN, (x, y), ICON_SIZE);
    }
}

/// The cap height of the degrees, which is what decides how far their
/// well stands proud of the strip it is written on.
const TEMP_CAP: f32 =
    widgets::PIXEL * TEMP_TEXT_SCALE * crate::engine::font::CAP_HEIGHT as f32;

/// How far above -- and below -- its strip that well reaches, the
/// hairline round it included.
const TEMP_WELL_PROUD: f32 =
    (TEMP_CAP + 2.0 * WELL_PAD - STAMINA_HEIGHT) / 2.0 + BAR_EDGE_WIDTH;

// This arithmetic assumes the writing is what decides the well's
// height, which is what `readout` takes the larger of. Make the strips
// deep enough to swallow the degrees and it goes the other way -- the
// well becomes the strip, `TEMP_WELL_PROUD` turns negative, and
// `STACK_TOP` starts reporting a stack shorter than the one drawn.
const _: () = assert!(TEMP_CAP + 2.0 * WELL_PAD > STAMINA_HEIGHT);
// ...and that proud edge has to stay inside the gap it grows into, or
// the degrees are printed on the breath meter. The neighbour's own
// hairline is already counted in `TEMP_WELL_PROUD`.
const _: () = assert!(TEMP_WELL_PROUD < STAMINA_GAP);

/// The top of everything the meters draw.
///
/// The highest strip plus the figure standing proud of it. Anything
/// that has to stay clear of the gauges measures from here rather than
/// from a number somebody once looked at, which is what
/// [`NOTICE_Y`] used to do.
const STACK_TOP: f32 = band_above(BANDS_ABOVE - 1).1 + TEMP_WELL_PROUD;
const STAMINA_TRACK: [f32; 4] = [0.05, 0.05, 0.06, 0.85];
const STAMINA_FILL: [f32; 4] = [0.40, 0.72, 0.92, 1.0];
/// Spent, and locked out until enough comes back. Red, because the
/// sprint key not working needs a visible reason.
const STAMINA_SPENT: [f32; 4] = [0.85, 0.35, 0.25, 1.0];
/// A fed player: a warm ochre, which is the one hue on the bar stack
/// that is neither the blue of breath and stamina nor the red of blood.
/// Three bars in three unrelated colours is what makes them readable
/// with a glance rather than a look.
const FED_FILL: [f32; 4] = [0.80, 0.62, 0.28, 1.0];
/// ...and a hungry one, past the line where wounds stop closing. Dulled
/// rather than reddened: red is what damage means everywhere else on
/// this HUD, and hunger is not damage until it is.
const HUNGRY_FILL: [f32; 4] = [0.62, 0.44, 0.18, 1.0];

/// The water bar, and what it turns when there is not much left.
///
/// Deliberately a *paler* blue than breath and stamina: the three are
/// all cool colours in the same corner of the screen, and thirst is the
/// slow one, so it is the quiet one.
const WATER_FILL: [f32; 4] = [0.38, 0.66, 0.86, 1.0];
/// Rested, and worn out. The pair is one hue turning: a night's
/// sleep is pale and cool, and what is left of it when there has
/// been none goes violet and dim -- the direction reads without
/// the legend.
const RESTED_FILL: [f32; 4] = [0.62, 0.70, 0.90, 1.0];
const WEARY_FILL: [f32; 4] = [0.46, 0.36, 0.58, 1.0];
const PARCHED_FILL: [f32; 4] = [0.74, 0.56, 0.28, 1.0];

/// The temperature gauge, at its four notable states.
///
/// Two ends of one axis rather than four unrelated colours: cold is
/// blue and colder is whiter-blue, hot is orange and hotter is nearer
/// red. A player should be able to read the *direction* without reading
/// the bar.
const COLD_FILL: [f32; 4] = [0.44, 0.66, 0.92, 1.0];
const FREEZING_FILL: [f32; 4] = [0.76, 0.88, 1.0, 1.0];
const HOT_FILL: [f32; 4] = [0.92, 0.60, 0.24, 1.0];
const SCORCHING_FILL: [f32; 4] = [0.96, 0.32, 0.18, 1.0];
/// Drifting out of the comfortable band, either way: the same two hues
/// held back, because this is the warning and the four above it are the
/// thing being warned about. A drift that looked as loud as freezing
/// would teach a player to ignore both.
const COOLING_FILL: [f32; 4] = [0.34, 0.48, 0.66, 1.0];
const WARMING_FILL: [f32; 4] = [0.68, 0.46, 0.22, 1.0];

/// The five tones of the temperature track, coldest first.
///
/// Dark enough to stay a background -- the reading is drawn over them
/// and has to win -- and tinted enough that which half of the scale the
/// marker is sitting in is readable without finding the middle first.
const ZONE_COLD_HARM: [f32; 4] = [0.10, 0.16, 0.28, 0.90];
const ZONE_DRIFT_COLD: [f32; 4] = [0.08, 0.11, 0.17, 0.88];
const ZONE_COMFORT: [f32; 4] = [0.11, 0.13, 0.11, 0.88];
const ZONE_DRIFT_HOT: [f32; 4] = [0.18, 0.12, 0.08, 0.88];
const ZONE_HOT_HARM: [f32; 4] = [0.28, 0.13, 0.07, 0.90];

/// How wide the marker on that track is.
///
/// Wide enough to be a mark rather than a hairline at the size a phone
/// draws this, and narrow enough that where it sits is still a reading.
const MARK_WIDTH: f32 = 0.008;

/// The degrees written at the end of the gauge.
///
/// Smaller than the health figure on purpose. Health is a number a
/// player reads; this is one they glance at, and at the same weight it
/// would compete with health from two strips away.
const TEMP_TEXT_SCALE: f32 = 0.55;

const COUNT_SCALE: f32 = 0.62;
const COUNT_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const COUNT_SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.85];

/// The fill colour at a given fraction of full health.
///
/// Green through amber to red. The colour is the part a player reads
/// without looking directly at the bar, so it has to carry the warning
/// on its own -- a bar that is only ever red tells you nothing until it
/// is nearly empty.
fn fill_colour(fraction: f32) -> [f32; 4] {
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.5 {
        // Green to amber over the top half.
        let t = (1.0 - f) * 2.0;
        [0.30 + 0.65 * t, 0.78 - 0.10 * t, 0.32 - 0.20 * t, 1.0]
    } else {
        // Amber to red over the bottom half.
        let t = 1.0 - f * 2.0;
        [0.95, 0.68 - 0.52 * t, 0.12 - 0.02 * t, 1.0]
    }
}

/// Draws the health bar.
///
/// `recent` is a value that lags `current` downward, so a hit leaves a
/// bright strip that drains away over the next moment. It is what makes
/// damage legible: the bar shrinking by a tenth is easy to miss, the
/// strip draining is not.
pub fn health_bar(painter: &mut Painter, current: f32, max: f32, recent: f32) {
    // These three numbers come off the wire, so none of them is this
    // module's to trust. A NaN in particular is not merely ugly: it
    // makes `clamp` panic, so a malformed `Health` message would take
    // the client down rather than draw a wrong bar.
    let max = if max.is_finite() && max > 0.0 { max } else { 1.0 };
    let current = if current.is_finite() {
        current.clamp(0.0, max)
    } else {
        0.0
    };
    let recent = if recent.is_finite() {
        recent.clamp(current, max)
    } else {
        current
    };

    let track = Rect::new(
        BAR_LEFT,
        BAR_Y,
        BAR_LEFT + BAR_WIDTH,
        BAR_Y + BAR_HEIGHT,
    );
    painter.quad(track, BAR_TRACK);
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_HEALTH);

    // How much health each segment stands for, and how full each one is.
    //
    // Over `METER_SPAN` rather than the whole track: the last stretch of
    // it is the readout column, and notches drawn under a figure are
    // notches nobody can count.
    let per_segment = max / SEGMENTS as f32;
    let pitch = METER_SPAN / SEGMENTS as f32;
    let gap = pitch * SEGMENT_GAP;
    let inset = 0.004;
    let colour = fill_colour(current / max);

    for index in 0..SEGMENTS {
        let x0 = track.x0 + pitch * index as f32 + gap / 2.0;
        let x1 = x0 + pitch - gap;
        let cell = Rect::new(x0, track.y0 + inset, x1, track.y1 - inset);
        painter.quad(cell, SEGMENT_EMPTY);

        let floor = index as f32 * per_segment;
        // A partly-drained segment is drawn partly filled rather than
        // rounded off, or the last point of health vanishes a whole
        // segment early.
        let filled = ((current - floor) / per_segment).clamp(0.0, 1.0);
        let ghost = ((recent - floor) / per_segment).clamp(0.0, 1.0);

        if ghost > filled {
            painter.quad(
                Rect::new(
                    cell.x0 + (cell.x1 - cell.x0) * filled,
                    cell.y0,
                    cell.x0 + (cell.x1 - cell.x0) * ghost,
                    cell.y1,
                ),
                BAR_RECENT,
            );
        }
        if filled > 0.0 {
            let lit = Rect::new(
                cell.x0,
                cell.y0,
                cell.x0 + (cell.x1 - cell.x0) * filled,
                cell.y1,
            );
            painter.quad(lit, colour);
            painter.quad(
                Rect::new(lit.x0, lit.y1 - (lit.y1 - lit.y0) * 0.34, lit.x1, lit.y1),
                SEGMENT_GLOSS,
            );
        }
    }

    // The exact figure, in the well at the end of the track rather than
    // over the segments: there it would have to fight whatever colour
    // they are, and they are every colour between green and red.
    let maximum = max.round() as i32;
    let label = format!("{}/{}", current.ceil() as i32, maximum);
    readout(
        painter,
        track,
        HEALTH_TEXT_SCALE,
        widest_reading(maximum),
        &label,
    );
}

/// The longest string `{current}/{max}` can come to at this maximum.
///
/// Returned as nines of the right *length*, which is all the fitting
/// reads: every glyph in this font is one cell wide, so a string's
/// width is its character count and nothing else. The obvious
/// `format!("{max}/{max}")` would be a heap allocation per frame in a
/// function that runs every frame to ask a question about a length --
/// the same reason `build_into` is handed a `Vec` to refill rather than
/// making one.
fn widest_reading(maximum: i32) -> &'static str {
    const NINES: &str = "99999999999999999999999";
    // `ilog10` is undefined at zero, and a server may well send one:
    // `max` is only guaranteed finite and positive, and anything under
    // a half rounds to nothing here.
    let digits = if maximum <= 0 {
        1
    } else {
        maximum.ilog10() as usize + 1
    };
    &NINES[..(2 * digits + 1).min(NINES.len())]
}

/// Draws the stamina strip, directly under the health gauge.
pub fn stamina_bar(painter: &mut Painter, fraction: f32, exhausted: bool) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (y0, y1) = band_below(0);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    painter.quad(track, STAMINA_TRACK);

    if fraction > 0.0 {
        painter.quad(
            Rect::new(
                track.x0,
                track.y0,
                track.x0 + BAR_WIDTH * fraction,
                track.y1,
            ),
            if exhausted { STAMINA_SPENT } else { STAMINA_FILL },
        );
    }
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_STAMINA);
}

/// Draws the breath meter, above the health gauge.
///
/// **Only while it is running out.** A meter that is always on screen
/// and always full is a meter nobody reads, and this one is full for
/// almost the whole game: the interesting thing about air is the moment
/// it starts to go. It appears when the first second is gone and
/// disappears the moment the player surfaces, which is also exactly
/// when they stop needing it.
pub fn breath_bar(painter: &mut Painter, fraction: f32) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if fraction >= 1.0 {
        return;
    }
    let (y0, y1) = band_above(1);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    painter.quad(track, STAMINA_TRACK);
    if fraction > 0.0 {
        painter.quad(
            Rect::new(track.x0, track.y0, track.x0 + BAR_WIDTH * fraction, track.y1),
            BREATH_FILL,
        );
    }
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_AIR);
}

/// Draws the hunger bar, directly under the stamina strip.
///
/// **Always on screen, unlike the breath meter**, and the difference is
/// the reason each one is where it is. Air is full for almost the whole
/// game and the interesting thing about it is the moment it starts to
/// go, so its meter appears then and vanishes again. Hunger is a slope
/// a player is always somewhere on, and the decision it drives -- is it
/// worth going back for the meat -- is one they make while it is still
/// half full. A bar you have to open a screen to see is a bar nobody
/// plans around.
///
/// Below the stamina strip rather than above the health gauge, so the
/// three read downwards in the order they matter: what is killing you,
/// what you can spend right now, and what you will need this evening.
pub fn nourishment_bar(painter: &mut Painter, fraction: f32) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let (y0, y1) = band_below(1);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    painter.quad(track, STAMINA_TRACK);

    if fraction > 0.0 {
        painter.quad(
            Rect::new(track.x0, track.y0, track.x0 + BAR_WIDTH * fraction, track.y1),
            // Two colours rather than a gradient, and the line between
            // them is exactly where wounds stop closing (see
            // `food::REGEN_THRESHOLD`). That is the one thing about
            // hunger a player has to be able to see coming, so the bar
            // *changes colour* at it rather than merely getting shorter.
            if fraction < primitive_shared::food::REGEN_THRESHOLD {
                HUNGRY_FILL
            } else {
                FED_FILL
            },
        );
    }
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_FOOD);
}

/// Writes the stack size into each occupied hotbar slot.
///
/// Only the count: the icon and the frame come from `hotbar`, which
/// reads the same inventory. An empty slot gets nothing at all -- its
/// frame is already drawn and is the whole of what "empty" looks like.
pub fn stack_counts(painter: &mut Painter, inventory: &Inventory) {
    let count = crate::ui::hotbar::MAX_SLOTS;
    for index in 0..count {
        let Some(stack) = inventory.slots().get(index).copied().flatten() else {
            continue;
        };
        // **A jug prints what is in it, not the one jug that it is.**
        // A jug stacks to one (see `types::stack_limit`), so its own
        // count is a "1" that says nothing, and the number a player
        // wants off the bar is how much grain is left. This is the
        // whole of the jug's appearance on the bar -- the corner mark
        // the inventory screen draws needs room the bar has not got.
        let held = match primitive_shared::inventory::jug_contents(&stack) {
            Some((_, units)) => units,
            None => stack.count,
        };
        if held == 0 {
            continue;
        }
        let centre = slot_centre(index, count);
        let rect = Rect::new(
            centre - SLOT / 2.0,
            BOTTOM,
            centre + SLOT / 2.0,
            BOTTOM + SLOT,
        );

        let label = held.to_string();
        let width = widgets::ink_width(&label, COUNT_SCALE);
        let left = rect.x1 - width - 0.006;
        let top = rect.y0 + widgets::cell_height(COUNT_SCALE) + 0.002;
        // A one-pixel drop shadow, because the count sits on top of a
        // block texture that may be any colour.
        let shadow = widgets::PIXEL * COUNT_SCALE;
        painter.text(&label, left + shadow, top - shadow, COUNT_SCALE, COUNT_SHADOW);
        painter.text(&label, left, top, COUNT_SCALE, COUNT_TEXT);
    }
}

/// Everything the HUD draws, in one call.
///
/// The death screen used to be part of this and is now
/// [`crate::ui::death`]: it is a screen with buttons and a state of its
/// own rather than one more thing drawn over the bars, and the moment it
/// needed a cursor it stopped belonging to the heads-up display.
///
/// What the world is doing to the player, as the server last said.
///
/// One struct rather than three more arguments, because the three
/// numbers arrive in one message and are drawn as one group -- and
/// because `build_into` already takes nine parameters and the tenth
/// would have been the one somebody passed in the wrong order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyGauges {
    /// Skin temperature, in the degrees `body` measures in.
    pub temperature_c: f32,
    /// What that reads as. Derived on the server so the thresholds
    /// cannot drift between the two sides.
    pub comfort: primitive_shared::body::Comfort,
    /// Water left, 0..1.
    pub hydration: f32,
    /// How tired, 0 (fresh) .. 1 (finished).
    ///
    /// Here rather than beside health because it arrives in the same
    /// message as the other two and is drawn with them -- see
    /// `ServerMessage::Body`.
    pub fatigue: f32,
    /// Every wound on the body, as the server last said.
    ///
    /// It was one flag, "a leg is broken", and the note on it said a mark
    /// beside the rest strip would carry it -- a mark nothing ever drew.
    /// The set lives here because it arrives beside the gauges and is read
    /// by everything they are: the HUD's marks beside the health bar
    /// (`wound_marks`), the pace the client predicts, and the mannequin in
    /// the pack.
    pub injuries: primitive_shared::injury::Injuries,
    /// How wet, 0 (dry) .. 1 (soaked).
    ///
    /// **Drawn nowhere on the HUD, and here anyway.** These last four
    /// are read by the pack screen's health page
    /// (`inventory_screen::health_page`), which is a page the player
    /// opens rather than a gauge that is always on screen -- so they
    /// ride the struct that already carries everything
    /// `ServerMessage::Body` says, instead of a second struct that would
    /// arrive in the same message and be threaded through the same
    /// call.
    pub wetness: f32,
    /// How filthy, 0 (clean) .. 1 (caked).
    pub grime: f32,
    /// How fast stamina comes back, as a multiplier -- which is what the
    /// hidden comfort is *worth*. See the health page for why the page
    /// shows this and not comfort itself.
    pub recovery: f32,
    /// How many food groups the recent diet has in it, 0..4.
    pub diet_groups: u8,
    /// The place the body is in, as the server last said
    /// (`ServerMessage::Shelter`): read by the health page to say *why*
    /// the warmth is going where it is.
    pub shelter: primitive_shared::shelter::Reading,
    /// The smoke at the eyes, 0..1 (`ServerMessage::Smoke`), kept here as
    /// well as in the fog so the same page can say what it is doing to
    /// the room.
    pub smoke: f32,
}

impl Default for BodyGauges {
    fn default() -> Self {
        Self {
            temperature_c: primitive_shared::body::NEUTRAL_C,
            comfort: primitive_shared::body::Comfort::Comfortable,
            hydration: 1.0,
            fatigue: 0.0,
            injuries: primitive_shared::injury::Injuries::default(),
            wetness: 0.0,
            grime: 0.0,
            recovery: 1.0,
            diet_groups: 0,
            shelter: primitive_shared::shelter::Reading {
                air_c: primitive_shared::body::NEUTRAL_C,
                ..Default::default()
            },
            smoke: 0.0,
        }
    }
}

/// What to tell a player whose warmth has just crossed into another band,
/// if anything. The heat's half only.
///
/// **Only on a crossing, and only on the crossings that ask something of
/// the player.** The gauge already shows where the body is; a line of text
/// is for the moment something changed. Getting hot asks for shade or
/// water, heatstroke asks for it *now*, and cooling off tells a player the
/// shade worked -- which is worth saying once, because the gauge that says
/// it drifts over half a minute and the player is deciding whether to
/// leave the tree. Scorching easing to Warm says nothing: it is still hot,
/// and "too hot" said a second time on the way *down* reads as the heat
/// getting worse.
///
/// Cold has no lines here. Not an oversight: the night already has its
/// own warnings in the order the cold arrives (the gauge, the shiver in
/// the hunger, the first point of damage), and a cold that talked would
/// talk every single evening.
pub fn heat_notice(
    was: primitive_shared::body::Comfort,
    now: primitive_shared::body::Comfort,
) -> Option<crate::ui::lang::Msg> {
    use crate::ui::lang::Msg;
    use primitive_shared::body::Comfort::{Scorching, Warm};
    match (was, now) {
        (Scorching, Scorching) | (Scorching, Warm) | (Warm, Warm) => None,
        (_, Scorching) => Some(Msg::HeatStroke),
        (_, Warm) => Some(Msg::HeatRising),
        (Warm | Scorching, _) => Some(Msg::HeatEased),
        _ => None,
    }
}

/// How long one heat line holds off the next, in seconds.
///
/// **The body can sit on a line.** Its temperature closes on its target
/// without arriving, and a player at the edge of a tree's shade at noon
/// can walk the skin back and forth across Warm every few seconds -- which
/// without this was "too hot", "cooler now", "too hot" in turn, for as
/// long as they stood there. Twenty seconds is longer than any step in or
/// out of the shade and shorter than it takes the heat to become a
/// different problem.
///
/// Heatstroke is never held back. It is the line that costs health, and
/// the player who is about to be hurt is owed it even if they were told
/// "too hot" a moment ago.
pub const HEAT_NOTICE_QUIET_SECS: f32 = 20.0;

/// Whether a heat line may go up, given what the notice strip is showing.
///
/// Reads the strip rather than keeping a clock of its own, deliberately:
/// the strip is already the one place a line lives, and a second record
/// of "when did we last say something hot" is a record that can disagree
/// with what is on screen -- a heat line pushed off by a refusal from the
/// server is gone, and the next heat line should be allowed.
pub fn heat_notice_allowed(
    line: crate::ui::lang::Msg,
    showing: Option<&(String, std::time::Instant)>,
    language: crate::ui::lang::Language,
    now: std::time::Instant,
) -> bool {
    use crate::ui::lang::Msg;
    if line == Msg::HeatStroke {
        return true;
    }
    let Some((text, at)) = showing else {
        return true;
    };
    let recent = now.saturating_duration_since(*at).as_secs_f32() < HEAT_NOTICE_QUIET_SECS;
    let hot = [Msg::HeatRising, Msg::HeatStroke, Msg::HeatEased]
        .into_iter()
        .any(|msg| language.text(msg) == text);
    !(recent && hot)
}

/// Draws the water bar, **above** the health gauge.
///
/// Always on screen, like hunger and unlike breath, and for the same
/// reason: thirst is a slope a player is always somewhere on, and the
/// decision it drives -- do I fill the jug before I leave the river --
/// is one they make while it is still half full.
///
/// **Above rather than below**, which is not where it belongs by
/// meaning. Hunger and thirst are the two slow meters and reading as a
/// pair would be better -- but there are only two strips' worth of room
/// under the gauge before the hotbar starts, and the first cut of this
/// put the water bar straight through the top row of slots. Given a
/// choice between "in the right group" and "on top of the hotbar", the
/// group loses.
pub fn hydration_bar(painter: &mut Painter, fraction: f32) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let (y0, y1) = band_above(0);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    painter.quad(track, STAMINA_TRACK);
    if fraction > 0.0 {
        painter.quad(
            Rect::new(track.x0, track.y0, track.x0 + BAR_WIDTH * fraction, track.y1),
            // The same two-colour trick the hunger bar uses, at the
            // threshold that actually means something: below `PARCHED`
            // the player is losing water faster than they can ignore.
            if fraction < primitive_shared::body::PARCHED {
                PARCHED_FILL
            } else {
                WATER_FILL
            },
        );
    }
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_WATER);
}

/// The tiredness strip, above the water bar.
///
/// **It shows what is left rather than what is spent**, like every other
/// strip here: health, water and food all drain toward the left, and one
/// meter that filled the other way would be read backwards by everybody
/// on their first night. So a rested player has a full bar.
///
/// The colour changes at `body::TIRED_AT`, which is where tiredness
/// starts costing pace and healing (see `survival::tiredness_factor`):
/// up to there it is the pale blue of a night's sleep, past it the
/// muddy violet of one you did not get. A player should be able to see
/// that the thing has started to matter without reading a number,
/// because the cost itself -- walking slightly slower, healing slightly
/// worse -- is exactly the kind of change nobody notices as it happens.
pub fn rest_bar(painter: &mut Painter, fatigue: f32) {
    let fatigue = if fatigue.is_finite() {
        fatigue.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let left = 1.0 - fatigue;
    let (y0, y1) = band_above(2);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    painter.quad(track, STAMINA_TRACK);
    if left > 0.0 {
        painter.quad(
            Rect::new(track.x0, track.y0, track.x0 + BAR_WIDTH * left, track.y1),
            if fatigue > primitive_shared::body::TIRED_AT {
                WEARY_FILL
            } else {
                RESTED_FILL
            },
        );
    }
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_REST);
}

/// How tired a player has to be before the strip appears at all.
///
/// A quarter of the scale -- about a third of a day awake. Tiredness
/// belongs with breath and the temperature scale rather than with
/// hunger and thirst: those two are slopes a player is always somewhere
/// on, and this one is *nothing at all* until the day has gone on, so a
/// bar sitting there full from dawn would be a bar that says nothing
/// six mornings out of seven.
///
/// Well before it costs anything (`body::TIRED_AT` is three quarters),
/// because the point of showing it is that the player sees it coming.
const REST_SHOWS_AT: f32 = 0.25;

/// Whether the tiredness strip has anything to say.
fn rest_is_worth_showing(fatigue: f32) -> bool {
    !fatigue.is_finite() || fatigue > REST_SHOWS_AT
}

/// The span the temperature gauge draws, end to end.
///
/// The whole range that means anything: below the first the cold is
/// doing damage, and the same at the other end. Fixing the ends to the
/// *harm* thresholds rather than to the comfort band is what lets a
/// position on the track be read as an absolute reading -- "a third of
/// the way to freezing" -- instead of only as a distance from
/// somewhere.
const SCALE_LOW: f32 = primitive_shared::body::FREEZING;
const SCALE_HIGH: f32 = primitive_shared::body::SCALDING;

/// Where a temperature falls along that span, 0..1.
fn temperature_at(celsius: f32) -> f32 {
    ((celsius - SCALE_LOW) / (SCALE_HIGH - SCALE_LOW)).clamp(0.0, 1.0)
}

/// Whether the gauge is on screen at this temperature at all.
///
/// **The comfort band, not the harm thresholds, and that is the fix.**
/// It used to appear when the server called the player Cold or Warm --
/// past `CHILLED` at 22 degrees -- while the *length* it drew was
/// measured from `COMFORT_LOW` at 26. So it arrived already 29% full,
/// jumping onto the screen a third of the way along, and the four
/// degrees of drift before that showed nothing at all.
///
/// Those four degrees are the entire point of the mechanic. `body`'s
/// own words: a player who walks into a snowfield does not become
/// cold, they *start* becoming cold, and they have the length of that
/// drift to do something about it. A gauge that waits until the drift
/// has finished reports the emergency instead of the warning.
fn temperature_is_worth_showing(celsius: f32) -> bool {
    use primitive_shared::body;
    celsius.is_finite() && !(body::COMFORT_LOW..=body::COMFORT_HIGH).contains(&celsius)
}

/// Draws the temperature gauge -- **only when it matters**.
///
/// Hidden while the player is comfortable, on the breath meter's terms
/// rather than the hunger bar's: hunger and thirst are slopes a player
/// is always somewhere on, and temperature is somewhere they are not
/// for most of a game. A gauge that read "fine" all evening would be a
/// gauge nobody looks at on the one night it says otherwise.
///
/// ## What it draws, and why it is not a bar any more
///
/// A bar filling from the middle -- which is what this was -- answers
/// "how far from comfortable", and nothing else. That is one of the two
/// questions a cold player has; the other is "how much further before
/// this starts hurting", and a length with no landmarks on it cannot
/// answer that one at all.
///
/// So the track is the whole scale, freezing to scalding, in five
/// tones: the comfortable band in the middle, a dimmer shade either
/// side of it for the drift, and a darker one at each end where the
/// damage is. The reading is a marker somewhere on that, with the run
/// from the edge of comfort out to it filled in -- so one picture
/// carries both answers, position for how bad it is and length for how
/// far it has come.
///
/// ## And why the number is written at the end of it
///
/// Because `body` says the scale is in degrees for a reason -- "a
/// number a player half-recognises is worth more than an abstract
/// 0..1" -- and nothing in the game ever showed it. A player could see
/// they were cold and had no way to find out whether the coat they just
/// put on was helping. Two degrees of recovery is invisible on any bar
/// this size and obvious as a number.
///
/// Written without a degree sign: the font has no glyph for one (see
/// `texture::GLYPHS`), and a missing glyph draws as a hole.
pub fn temperature_gauge(painter: &mut Painter, body: BodyGauges) {
    use primitive_shared::body::{self, Comfort};

    if !temperature_is_worth_showing(body.temperature_c) {
        return;
    }
    let cold = body.temperature_c < body::COMFORT_LOW;

    // Its own band, clear of the water bar under it and the breath
    // meter between them -- so the three never overlap even when a
    // player is drowning in a cold lake, which is precisely when all of
    // them are on screen at once.
    let (y0, y1) = band_above(3);
    let track = Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);

    // The scale itself, in five tones. Drawn first: it is the ground
    // the reading sits on.
    //
    // Laid over `METER_SPAN` and not the whole track, for the reason
    // the health segments are: the degrees are written in the column at
    // the end, and a scale running under its own reading is a scale
    // whose hot end cannot be read.
    let at = |celsius: f32| track.x0 + METER_SPAN * temperature_at(celsius);
    for (from, to, tone) in [
        (SCALE_LOW, body::CHILLED, ZONE_COLD_HARM),
        (body::CHILLED, body::COMFORT_LOW, ZONE_DRIFT_COLD),
        (body::COMFORT_LOW, body::COMFORT_HIGH, ZONE_COMFORT),
        (body::COMFORT_HIGH, body::OVERHEATED, ZONE_DRIFT_HOT),
        (body::OVERHEATED, SCALE_HIGH, ZONE_HOT_HARM),
    ] {
        painter.quad(Rect::new(at(from), track.y0, at(to), track.y1), tone);
    }

    // The reading: the run from the edge of comfort out to where the
    // body actually is, and a marker at the end of it.
    let fill = match body.comfort {
        Comfort::Freezing => FREEZING_FILL,
        Comfort::Cold => COLD_FILL,
        Comfort::Scorching => SCORCHING_FILL,
        Comfort::Warm => HOT_FILL,
        // Outside the band, but not far enough out for the server to
        // have named it. This is the drift, and it is the state the
        // gauge now exists to show.
        Comfort::Comfortable if cold => COOLING_FILL,
        Comfort::Comfortable => WARMING_FILL,
    };
    let edge = at(if cold {
        body::COMFORT_LOW
    } else {
        body::COMFORT_HIGH
    });
    let here = at(body.temperature_c);
    let (from, to) = if cold { (here, edge) } else { (edge, here) };
    if to > from {
        painter.quad(
            Rect::new(from, track.y0, to, track.y1),
            [fill[0], fill[1], fill[2], 0.55],
        );
    }
    // The marker, kept inside the track at both ends: half of it would
    // otherwise hang off the edge exactly when the reading matters
    // most. The only fully opaque thing on the strip.
    let half = MARK_WIDTH * 0.5;
    let centre = here.clamp(track.x0 + half, track.x0 + METER_SPAN - half);
    painter.quad(
        Rect::new(centre - half, track.y0, centre + half, track.y1),
        fill,
    );
    painter.border(track, BAR_EDGE_WIDTH, BAR_EDGE);
    band_icon(painter, track, ICON_WARMTH);

    // The figure, in the same column the health gauge writes its own
    // in, so the two line up rather than each finding its own margin.
    //
    // The widest it can be is four characters: the harm thresholds are
    // inside two digits at both ends and the cold one is signed.
    let label = format!("{:.0}C", body.temperature_c);
    readout(
        painter,
        track,
        TEMP_TEXT_SCALE,
        "-12C",
        &label,
    );
}

// ## Why the controls are frames and not slabs
//
// A filled button is legible and costs a piece of the world for every
// one of them, and a phone screen is mostly thumb already. Drawn as
// frames they cost the width of two hairlines.
//
// What makes that possible is that every line is a *pair* -- a dark
// stroke and a pale one, side by side. One line has a brightness and
// the world has all of them: measured on the composite, a pale
// hairline over snow is 1.02:1, which is not a faint line but no line,
// and a dark one vanishes into a lit cave just as completely. Two
// lines at opposite ends of the range cannot both disappear into a
// colour that lies between them. See `widgets::hairline_frame` and
// `every_thumb_control_shows_an_edge_against_any_world`.
//
// Brightness carries state, because there is no fill left to carry it:
// a resting control is drawn at the quiet tier and a pressed one at the
// bright tier. The dark companion does not change -- it is what makes
// both of them visible, not what distinguishes them.

/// How thick a thumb control's frame is, as a fraction of its own
/// radius.
///
/// Sized to the control rather than fixed, so a button the player has
/// made small gets a frame in proportion instead of a slab.
const FRAME_THICKNESS: f32 = 0.06;

/// How far outside its own box a control's frame is actually drawn, in
/// whatever unit `radius` is in.
///
/// **Two thicknesses, not one, and that is the number that was
/// missing.** `Painter::hairline_frame` draws the pale line around the
/// box and the dark line around *that*, so what a player sees is wider
/// than what is hit-tested by twice the stroke on every side.
///
/// A player reported the top row of buttons overlapping at INTERFACE
/// SIZE 1.45. Their boxes were 33 px apart -- no overlap at all by the
/// arithmetic every layout test in this repository was doing -- and the
/// frames ate 13 px of that from each side, leaving six pixels of air
/// between two drawn rectangles on a 2712-pixel-wide screen. Six pixels
/// is not a gap; it is two buttons that look like one, with the word on
/// one of them running into the frame of the next.
///
/// So the guard measures this, and the arrangement is spaced against
/// it. See `no_two_controls_are_drawn_on_top_of_one_another_at_any_interface_size`.
pub fn frame_overhang(radius: f32) -> f32 {
    radius * FRAME_THICKNESS * 2.0
}

// ---- the sail's dial ----
//
// **The one control in this game whose position cannot be read off the
// world.** Everything else a player sets is a thing they can look at: the
// block in hand is drawn in the hand, the fire is lit or it is not. The sail
// has an angle, and the angle only means anything *against the wind* -- and
// the wind is invisible. A player braces the yard, sees the yard move, and
// still cannot tell whether they have just caught the wind or spilled it.
//
// So this draws the three things the decision is made out of, in one place
// and in the raft's own frame: the raft (always pointing up, because a person
// steering does not think of themselves as turning), the yard across it, and
// where the wind is going and how hard. Nothing is written in words, which is
// not a saving on letters -- it is that this has to be read in the half
// second between two gusts, and a legend is not read at all.
//
// Rejected: putting it on the sail itself, as a coloured tell on the hide.
// It is on the sail that the player is already looking, which is the argument
// for it -- and it is also the thing the sail is *behind* when the raft is
// running before the wind, which is exactly when the trim is most wrong.

/// How big the dial is, from the middle to an edge of its plate.
const SAIL_DIAL_HALF: f32 = 0.082;
/// Where the middle of it sits: near the top of the screen, clear of the
/// crosshair and clear of everything that grows upward from the hotbar.
const SAIL_DIAL_Y: f32 = 1.0 - SAIL_DIAL_HALF - 0.055;
const SAIL_DIAL_BG: [f32; 4] = [0.05, 0.07, 0.10, 0.62];
const SAIL_DIAL_EDGE: [f32; 4] = [0.55, 0.58, 0.62, 0.80];
/// The raft under the yard: dim, because it is the frame of reference and
/// not a reading.
const SAIL_DIAL_HULL: [f32; 4] = [0.62, 0.55, 0.42, 0.85];
/// The yard, in the hide's own colour, and brighter under a hand.
const SAIL_DIAL_YARD: [f32; 4] = [0.92, 0.86, 0.68, 0.95];
const SAIL_DIAL_YARD_HELD: [f32; 4] = [1.0, 0.97, 0.80, 1.0];
/// The wind: cold, so it cannot be mistaken for a rope.
const SAIL_DIAL_WIND: [f32; 4] = [0.58, 0.80, 1.0, 0.95];

/// A bar through a dial: `angle` radians clockwise from straight up,
/// reaching from `from` to `to` out of the middle, `half_width` thick.
///
/// Two triangles pushed by hand, because [`Painter::quad`] is
/// axis-aligned and everything on this dial is at an angle -- which is the
/// whole point of the dial.
fn spoke(
    painter: &mut Painter,
    centre: (f32, f32),
    angle: f32,
    from: f32,
    to: f32,
    half_width: f32,
    colour: [f32; 4],
) {
    // Straight up is the bow, and a positive angle goes to starboard, which
    // is to the right of the screen: the same hand a player turns the mouse
    // with to brace the yard that way.
    let (sin, cos) = angle.sin_cos();
    let (dir, side) = ((sin, cos), (cos, -sin));
    let at = |along: f32, across: f32| {
        [
            centre.0 + dir.0 * along + side.0 * across,
            centre.1 + dir.1 * along + side.1 * across,
        ]
    };
    let (a, b, c, d) = (
        at(from, -half_width),
        at(to, -half_width),
        at(to, half_width),
        at(from, half_width),
    );
    for position in [a, b, c, a, c, d] {
        painter.vertices.push(HotbarVertex {
            position,
            uv: [0.0, 0.0],
            tex_layer: crate::ui::hotbar::UNTEXTURED,
            tint: colour,
        });
    }
}

/// The sail's dial: the raft bow-up, the yard braced across it, and the
/// wind.
///
/// `relative_wind` is where the wind is blowing *toward*, in radians off the
/// bow; `angle` is the yard's (`raft::Body::sail_angle`); `strength` is the
/// wind's, 0..1. `held` is whether the player's hand is on the sheets this
/// instant, which is the only feedback there is that the drag has taken --
/// the yard also moves, but a yard at the angle it was already at does not.
pub fn sail_gauge(painter: &mut Painter, relative_wind: f32, angle: f32, strength: f32, held: bool) {
    let centre = (0.0, SAIL_DIAL_Y);
    let plate = Rect::centred(centre.0, centre.1, SAIL_DIAL_HALF * 2.0, SAIL_DIAL_HALF * 2.0);
    painter.quad(plate, SAIL_DIAL_BG);
    painter.border(plate, 0.0025, SAIL_DIAL_EDGE);

    let r = SAIL_DIAL_HALF;
    // The raft: a bar fore and aft with a wider stroke at the bow, so which
    // end is the bow is a shape and not a colour. Always pointing up --
    // everything else on the dial is measured against the raft, and a dial
    // that turned under the player would be a compass, which is a different
    // instrument answering a different question.
    spoke(painter, centre, 0.0, -r * 0.62, r * 0.62, r * 0.055, SAIL_DIAL_HULL);
    spoke(painter, centre, 0.0, r * 0.44, r * 0.62, r * 0.14, SAIL_DIAL_HULL);

    // The wind, from the rim toward the middle: an arrow that arrives at the
    // raft, because what matters is which side of the raft it is coming from.
    // Its length is how hard it blows, so a calm is a stub and a squall
    // reaches the rim -- the same number that decides whether the sail is
    // worth setting at all.
    let from = relative_wind + std::f32::consts::PI;
    let reach = r * (0.30 + 0.62 * strength.clamp(0.0, 1.0));
    spoke(painter, centre, from, r * 0.30, reach, r * 0.05, SAIL_DIAL_WIND);
    // ...with a head, two short bars swept back from where it points.
    let head = (centre.0 + from.sin() * r * 0.30, centre.1 + from.cos() * r * 0.30);
    for sweep in [2.4f32, -2.4] {
        spoke(painter, head, from + sweep, 0.0, r * 0.22, r * 0.045, SAIL_DIAL_WIND);
    }

    // The yard, across the raft and braced round by the angle: the same
    // rotation `raft_model` turns it by and the same one the rules measure
    // the wind against. A quarter turn is added because the yard lies
    // *across* the hull when it is square.
    let ink = if held { SAIL_DIAL_YARD_HELD } else { SAIL_DIAL_YARD };
    let across = angle + std::f32::consts::FRAC_PI_2;
    spoke(painter, centre, across, -r * 0.76, r * 0.76, r * 0.07, ink);
}

/// How far right of the middle the compass sits while the sail's dial has
/// the middle: beside it, a gap apart, so a sailor reads both at once.
pub const COMPASS_BESIDE_SAIL: f32 = SAIL_DIAL_HALF * 2.0 + 0.03;
/// The needle's north end: the red a lodestone needle's marked end is
/// painted, so which end is north is a colour *and* a length.
const COMPASS_NORTH: [f32; 4] = [0.90, 0.26, 0.20, 1.0];
const COMPASS_SOUTH: [f32; 4] = [0.82, 0.82, 0.80, 0.9];
/// The mark at the top of the plate: where the player is looking.
const COMPASS_LUBBER: [f32; 4] = [0.95, 0.86, 0.55, 0.9];

/// **The water compass** (`types::BLOCK_WATER_COMPASS`), while one is in
/// the hand: the sail dial's plate, with the top of it the way the player
/// faces and a needle to north. `needle` is `logic::bearing::needle` --
/// radians clockwise from the top of the dial -- and `x` is where the
/// middle of the dial sits across the top of the screen.
///
/// **The top is where the player looks, not north.** A dial with north up
/// and an arrow for the player is the map, which the journal already is;
/// this answers the question the map cannot while it is shut -- "where is
/// the map's top from where I am standing" -- the way a real compass in a
/// hand does. It never points at a bag (see `types::BLOCK_WATER_COMPASS`).
pub fn compass_dial(painter: &mut Painter, needle: f32, x: f32) {
    let centre = (x, SAIL_DIAL_Y);
    let plate = Rect::centred(centre.0, centre.1, SAIL_DIAL_HALF * 2.0, SAIL_DIAL_HALF * 2.0);
    painter.quad(plate, SAIL_DIAL_BG);
    painter.border(plate, 0.0025, SAIL_DIAL_EDGE);
    let r = SAIL_DIAL_HALF;
    spoke(painter, centre, 0.0, r * 0.74, r * 0.92, r * 0.05, COMPASS_LUBBER);
    // The needle: a long red half to north and a short pale half behind.
    spoke(painter, centre, needle, 0.0, r * 0.72, r * 0.075, COMPASS_NORTH);
    spoke(painter, centre, needle + std::f32::consts::PI, 0.0, r * 0.5, r * 0.06, COMPASS_SOUTH);
}

/// Where the sky's hint is written: under the dials' row, so a player
/// with a compass in hand who looks up has both and neither covers the
/// other.
const SKY_HINT_TOP: f32 = SAIL_DIAL_Y - SAIL_DIAL_HALF - 0.035;
const SKY_HINT_SCALE: f32 = 0.85;

/// **What the sky says about north** (`logic::bearing::read_sky`), one
/// line near the top of the view while the player is looking at the thing
/// it is read off. Plain words on a plate, and gone the moment they look
/// away: it is a reading, not an instrument (that is the compass above).
pub fn sky_hint(painter: &mut Painter, text: &str) {
    let ink = widgets::ink_width(text, SKY_HINT_SCALE);
    let cap = widgets::cell_height(SKY_HINT_SCALE);
    painter.quad(
        Rect::new(-ink / 2.0 - 0.014, SKY_HINT_TOP - cap - 0.004, ink / 2.0 + 0.014, SKY_HINT_TOP + 0.012),
        SAIL_DIAL_BG,
    );
    painter.text_centred(text, 0.0, SKY_HINT_TOP, SKY_HINT_SCALE, widgets::TEXT);
}

const FIRST_STEP_SCALE: f32 = 0.8;
/// Air between the notice's plate and this one.
const FIRST_STEP_CLEARANCE: f32 = 0.022;
/// **One thing to do next, for a player who has just woken up.**
///
/// The player said the progression was completely unclear, and the worst
/// of that is the first two minutes: a meadow, empty hands, and no reason
/// to touch anything in particular. So three prompts, one at a time, and
/// then never again -- see `ladder::first_step` for why one and why it
/// does not come back. A line over the belt and not a modal: a box that
/// has to be dismissed is a tutorial, and a tutorial was the thing asked
/// against.
///
/// Drawn in the belt's own ink on the belt's own plate, because that is
/// where the answer to the line is: the stone it tells you to pick up
/// lands in the slot underneath it.
pub fn first_step_line(painter: &mut Painter, text: &str) {
    let ink = widgets::ink_width(text, FIRST_STEP_SCALE);
    let cell = widgets::cell_height(FIRST_STEP_SCALE);
    painter.quad(
        Rect::new(-ink / 2.0 - 0.014, FIRST_STEP_TOP - cell - 0.004, ink / 2.0 + 0.014, FIRST_STEP_TOP + 0.012),
        SAIL_DIAL_BG,
    );
    painter.text_centred(text, 0.0, FIRST_STEP_TOP, FIRST_STEP_SCALE, widgets::TEXT);
}

/// The top of the first two minutes' line, which is what `Painter::text`
/// takes: the glyphs hang *below* it by a whole cell, descenders and all
/// (`font::CAP_HEIGHT` against `GLYPH_HEIGHT`), which is why a whole cell
/// and the plate's own lip are added here -- `отщепы` hangs a good way
/// further than `flakes` does.
///
/// **Above the notice, which is above the gauges** -- derived from both
/// rather than chosen, for the reason `NOTICE_Y` gives at length. It was
/// `hotbar::TOP + 0.055` first, a number picked by eye as "just over the
/// belt", and the snapshot of the HUD showed it printed straight across
/// the stamina strip and the breath gauge: the strip nothing else uses is
/// not over the belt, because three gauges live there. The notice and this
/// can be up at once -- a refusal while a new player is being prompted --
/// so this clears the plate rather than sharing its row.
const FIRST_STEP_TOP: f32 = NOTICE_Y
    + NOTICE_HALF_HEIGHT
    + widgets::cell_height(FIRST_STEP_SCALE)
    + 0.004
    + FIRST_STEP_CLEARANCE;

/// The dark half of every pair. Near-black, and the same behind a
/// frame, a letter and the stick's ring.
const EDGE_DARK: [f32; 4] = [0.03, 0.03, 0.04, 0.92];
/// The pale half at rest: the quiet tier, at about half brightness.
///
/// Half rather than two thirds, and the difference is measured. At
/// `0.66` a pressed control differed from a resting one by 1.47:1
/// against grass -- a change a palette shows and an eye does not. The
/// two tiers have to be far enough apart to be told apart *through* the
/// world behind them, which is a stronger demand than looking different
/// side by side.
const EDGE_IDLE: [f32; 4] = [0.52, 0.52, 0.50, 0.92];
/// ...and under a thumb: the bright tier.
const EDGE_PRESSED: [f32; 4] = [0.97, 0.96, 0.90, 1.0];

/// The controls a thumb uses, drawn over the world.
///
/// Only on a platform that has no keyboard -- see `platform::Window`'s
/// `is_touch_primary`. On a desktop this is never called and costs
/// nothing.
///
/// ## Why they are drawn faintly
///
/// Every pixel of a control on a phone is a pixel of the world the
/// player cannot see past, and they are looking at a world for a
/// living. The buttons are drawn dark and half-transparent so a thumb
/// can find them without the game becoming a picture of its own
/// interface -- and the one that is *held* brightens, which is the only
/// feedback there is when a finger is covering the thing it pressed.
///
/// ## Why the stick ring is where the layout says and not where the
/// thumb is
///
/// The stick centres itself on wherever the thumb landed, so drawing it
/// under the thumb would mean drawing a ring that jumps around the
/// screen. The painted ring is a *reminder of where to put a thumb*,
/// not a picture of the stick's state, and a reminder that stays still
/// is the only kind worth having.
// ---------------------------------------------------------------------------
// The line
// ---------------------------------------------------------------------------
//
// **Two readings on one bar, under the crosshair**: how far the rod is wound
// back, and how near the line is to parting. They never happen at once -- a
// rod being wound back has no line in the water -- so one bar says both, and
// the colour is what tells them apart.
//
// **Why a bar at all, in a mechanic whose whole argument is that the
// information is in the world.** The float says everything about the *water*
// -- how lively it is, when a fish has the bait -- and a player reads it by
// looking at it. The strain on a line is not a thing anybody can see from
// behind their own hands: in life it is felt through the rod, and the two
// ways to put a feeling on a screen are a number or a picture of the rod
// bending. A rod that bent would be right and is a model change in the hand
// for a state that lasts eight seconds; this is the cheap honest version of
// it, and it carries no digits.
const LINE_BAR_HALF: f32 = 0.11;
/// Under the crosshair and clear of it: the eye is on the float, not here.
const LINE_BAR_Y: f32 = -0.085;
const LINE_BAR_THICK: f32 = 0.011;
const LINE_BAR_BACK: [f32; 4] = [0.05, 0.07, 0.10, 0.55];
/// Winding up: the cord's own colour.
const LINE_BAR_WIND: [f32; 4] = [0.86, 0.78, 0.56, 0.95];
/// A line under strain: green while it is safe, red as it goes.
const LINE_BAR_SAFE: [f32; 4] = [0.45, 0.82, 0.48, 0.95];
const LINE_BAR_GONE: [f32; 4] = [0.92, 0.35, 0.28, 1.0];

/// The wind-up, or the strain: `winding` is the rod being drawn back (0..1),
/// `strain` is a fish on the line (0..1). Nothing is drawn when both are
/// `None`, which is nearly always.
pub fn line_gauge(painter: &mut Painter, winding: Option<f32>, strain: Option<f32>) {
    let (fraction, colour) = match (winding, strain) {
        (_, Some(strain)) => {
            let hot = strain.clamp(0.0, 1.0);
            let mix = |a: f32, b: f32| a + (b - a) * hot * hot;
            (
                hot,
                [
                    mix(LINE_BAR_SAFE[0], LINE_BAR_GONE[0]),
                    mix(LINE_BAR_SAFE[1], LINE_BAR_GONE[1]),
                    mix(LINE_BAR_SAFE[2], LINE_BAR_GONE[2]),
                    1.0,
                ],
            )
        }
        (Some(winding), None) => (winding.clamp(0.0, 1.0), LINE_BAR_WIND),
        (None, None) => return,
    };
    painter.quad(
        Rect {
            x0: -LINE_BAR_HALF,
            y0: LINE_BAR_Y - LINE_BAR_THICK,
            x1: LINE_BAR_HALF,
            y1: LINE_BAR_Y + LINE_BAR_THICK,
        },
        LINE_BAR_BACK,
    );
    painter.quad(
        Rect {
            x0: -LINE_BAR_HALF,
            y0: LINE_BAR_Y - LINE_BAR_THICK,
            x1: -LINE_BAR_HALF + LINE_BAR_HALF * 2.0 * fraction,
            y1: LINE_BAR_Y + LINE_BAR_THICK,
        },
        colour,
    );
}

pub fn touch_controls(
    painter: &mut Painter,
    layout: &crate::platform::touch::Layout,
    held: impl Fn(crate::platform::touch::Slot) -> bool,
    language: crate::ui::lang::Language,
) {

    // Pixels to the space the interface is authored in. The same
    // conversion the cursor goes through, so a button drawn here is
    // exactly where `Layout` hit-tests it.
    let size = (layout.size.width, layout.size.height);
    // **Unscaled, and that is not an oversight.** These are the one
    // part of the interface that is already the right physical size:
    // `Layout` sizes a thumb control as a fraction of the screen's
    // shorter side, which is what a thumb actually cares about, and a
    // finger is the same width on every phone. What they need is not to
    // be made bigger but to land exactly where `Layout::button_at`
    // tests them -- which works in pixels, so the drawing has to come
    // back to the same pixels and no others.
    let to_ui = |(x, y): (f32, f32)| {
        crate::ui::widgets::cursor_to_ui((x as f64, y as f64), size, 1.0)
    };
    // A length is not a point, so it converts by scale alone: the y
    // axis spans 2.0 over the window's height.
    let to_len = |pixels: f32| pixels * 2.0 / size.1.max(1) as f32;



    /// The box a control occupies, in interface units.
    ///
    /// Written once and used for the ring and the buttons, because the
    /// one thing they must agree on is exactly this rectangle -- it is
    /// also what `platform::touch::Layout` hit-tests against.
    fn box_of(
        placed: &crate::platform::touch::Placed,
        to_ui: impl Fn((f32, f32)) -> (f32, f32),
        to_len: impl Fn(f32) -> f32,
    ) -> Rect {
        let (cx, cy) = to_ui(placed.centre);
        Rect::centred(
            cx,
            cy,
            to_len(placed.half.0) * 2.0,
            to_len(placed.half.1) * 2.0,
        )
    }

    if layout.stick.shown {
        let ring = box_of(&layout.stick, to_ui, to_len);
        painter.hairline_frame(
            ring,
            to_len(layout.stick.radius()) * 0.05,
            EDGE_DARK,
            EDGE_IDLE,
        );
    }

    for (slot, placed) in layout.buttons.into_iter().enumerate() {
        // A control the player switched off is not drawn and is not
        // pressed -- `Placed::contains` already refuses it -- so the
        // two agree by construction rather than by both remembering.
        if !placed.shown {
            continue;
        }
        let rect = box_of(&placed, to_ui, to_len);
        let down = held(slot);
        // No fill: the frame is the button. See the note above the
        // tones for why one line would not be enough.
        painter.hairline_frame(
            rect,
            to_len(placed.radius()) * FRAME_THICKNESS,
            EDGE_DARK,
            if down { EDGE_PRESSED } else { EDGE_IDLE },
        );
        // **The key's own name, not a picture of what it does.** A
        // button carries a key now, and a key can be any of fifty --
        // there is no glyph for most of them, and the font has no
        // symbols to invent one out of. `SPACE`, `F3`, `G`: what is
        // printed on the key being emulated, which is the one name the
        // player already knows.
        //
        // Sized to the box rather than by a constant, because the name
        // is not a known length: see `widgets::scale_to_fit`. Four
        // fifths of the button, so the letters do not touch the border
        // and it still reads as a button rather than as a word.
        // In the player's language where it is a word -- see
        // `Emits::label_in`.
        let label = placed.emits.label_in(language);
        let scale = crate::ui::widgets::scale_to_fit(rect, label, 0.8);
        painter.label_in_two_tones(
            rect,
            label,
            scale,
            EDGE_DARK,
            if down { EDGE_PRESSED } else { EDGE_IDLE },
        );
    }
}
// ---- when the gauges are worth the screen they cost ----
//
// **A meter that is always full is a meter nobody reads.** That
// sentence is already in this file twice, above `breath_bar` and above
// `temperature_gauge`, and both of those hide themselves on it. The
// four that did not -- health, stamina, hunger, thirst -- are the four
// a player spends most of the game at the top of, and on a 2712x1220
// phone they hold a band 900 px wide across the bottom of the world
// for the privilege of saying that nothing is wrong.
//
// What stopped the same rule being applied to them is that hunger and
// thirst carry an argument for *staying*: they are slopes a player is
// always somewhere on, and the decision they drive -- is it worth going
// back for the meat -- is made while the bar is still half full. That
// argument is untouched here. A bar is hidden only while it is
// **completely** full, which for hunger and thirst is the one state in
// which there is no slope to be somewhere on.

/// How long the gauges stay up after the last thing they had to say.
///
/// Long enough to read the end of what just happened -- a wound
/// closing, a meal landing -- and short enough that a player who has
/// stopped to look at the view is looking at the view.
const GAUGES_LINGER: std::time::Duration = std::time::Duration::from_secs(3);

/// ...and how long they take to go once they start going.
///
/// A fade rather than a cut, because the eye is caught by a change and
/// there is nothing here worth catching it: something vanishing from
/// the bottom of the screen is read as something *happening*, and
/// nothing has. Long enough to be a fade and not a flicker.
const GAUGES_FADE: std::time::Duration = std::time::Duration::from_millis(700);

/// How long the stack has been saying nothing.
///
/// ## Why one clock for the four of them
///
/// Because they are one object. Fading each on its own timer leaves a
/// comb with teeth missing -- the bands are at fixed heights, so a
/// hidden middle strip is a gap rather than a closing-up -- and worse,
/// it answers the wrong question. A player who looks down because they
/// have been hurt is asking *how am I doing*, and the answer to that
/// includes what they have eaten. So anything worth saying brings the
/// whole stack back.
///
/// ## What counts as worth saying
///
/// Not full, in any of them, plus the two that already decide for
/// themselves: air that is going and a temperature outside comfort.
/// Both of those are warnings, and **a warning is never faded** -- it
/// is the reason the fade is driven from the same values the gauges
/// are drawn from, in the same function, rather than from a flag
/// somebody has to remember to set.
#[derive(Debug, Clone, Copy, Default)]
pub struct Attention {
    /// When the gauges last had something to say. `None` until the
    /// first frame: a session opens with them on screen, because a
    /// player arriving in a world should see what the game gives them
    /// before it takes it away again.
    spoke_at: Option<std::time::Instant>,
}

impl Attention {
    /// How visible the stack should be, and remembers why.
    pub fn alpha(&mut self, now: std::time::Instant, worth_saying: bool) -> f32 {
        if worth_saying || self.spoke_at.is_none() {
            self.spoke_at = Some(now);
            return 1.0;
        }
        let since = now.duration_since(self.spoke_at.unwrap_or(now));
        if since <= GAUGES_LINGER {
            return 1.0;
        }
        let going = (since - GAUGES_LINGER).as_secs_f32() / GAUGES_FADE.as_secs_f32();
        (1.0 - going).clamp(0.0, 1.0)
    }
}

/// Whether the gauges have anything to say about these readings.
///
/// Beside [`Attention`] and not inside it, because it is a statement
/// about the *drawing*: every clause here is a gauge that would put
/// something on the screen. Kept where a change to one can be seen
/// against the other.
// One argument per reading, deliberately: this is the same list
// `build_into` is handed, in the same order, and it has to be -- the
// whole point is that the decision to fade is made from exactly the
// numbers the gauges are drawn from, in the same call. Gathering them
// into a struct to please a lint would put a second copy of the list
// between the two.
#[allow(clippy::too_many_arguments)]
fn gauges_are_worth_showing(
    health: f32,
    max_health: f32,
    recent_health: f32,
    stamina: f32,
    exhausted: bool,
    breath: f32,
    nourishment: f32,
    body: BodyGauges,
) -> bool {
    // A gauge with a NaN in it draws *something*, so an unreadable
    // reading counts as worth showing rather than as full: the one
    // thing worse than a bar that will not go away is a bar that
    // vanishes when the numbers stop making sense.
    let full = |value: f32| value.is_finite() && value >= 1.0;
    let unhurt = max_health.is_finite()
        && max_health > 0.0
        && health.is_finite()
        && health >= max_health
        // The strip left behind by a hit drains after the bar has
        // already refilled, and it is the part a player actually sees.
        && (!recent_health.is_finite() || recent_health <= health);

    !unhurt
        || !full(stamina)
        || exhausted
        || !full(breath)
        || !full(nourishment)
        || !full(body.hydration)
        || temperature_is_worth_showing(body.temperature_c)
        // ...and a wound that is doing something: the marks beside the
        // health bar would otherwise fade out with the gauges while a cut
        // went on bleeding, which is the one moment they exist for.
        || body.injuries.is_bleeding()
        || body.injuries.leg_broken()
        || body.injuries.arm_broken()
}

/// Multiplies the alpha of everything drawn since `from`.
///
/// The gauges are faded by dimming what they drew rather than by being
/// handed an alpha each, and that is deliberate: seven functions each
/// threading a fade through every colour they use is seven chances for
/// one border or one figure to stay solid while the rest goes. One pass
/// over the vertices cannot miss a piece.
fn fade_from(vertices: &mut [HotbarVertex], alpha: f32) {
    if alpha >= 1.0 {
        return;
    }
    for vertex in vertices {
        vertex.tint[3] *= alpha;
    }
}

/// The `Vec`-returning form, kept for the tests: they assert on one
/// widget's output in isolation, which is exactly what appending into a
/// shared list is designed not to produce.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn build(
    font: FontAtlas,
    health: f32,
    max_health: f32,
    recent_health: f32,
    stamina: f32,
    exhausted: bool,
    breath: f32,
    nourishment: f32,
    body: BodyGauges,
    inventory: &Inventory,
    // What the server last refused, and how visible it still is.
    notice: Option<(&str, f32)>,
) -> Vec<HotbarVertex> {
    let mut out = Vec::new();
    // A fresh clock, so a test sees the gauges as a player sees them on
    // the first frame of a session: all of them, at full strength.
    let mut attention = Attention::default();
    build_into(
        font,
        health,
        max_health,
        recent_health,
        stamina,
        exhausted,
        breath,
        nourishment,
        body,
        inventory,
        notice,
        &mut attention,
        std::time::Instant::now(),
        &mut out,
    );
    out
}

/// The same HUD, appended to a list the caller keeps between frames --
/// so a rebuild reuses the allocation instead of making a fresh one.
#[allow(clippy::too_many_arguments)]
pub fn build_into(
    font: FontAtlas,
    health: f32,
    max_health: f32,
    recent_health: f32,
    stamina: f32,
    exhausted: bool,
    breath: f32,
    nourishment: f32,
    body: BodyGauges,
    inventory: &Inventory,
    notice: Option<(&str, f32)>,
    attention: &mut Attention,
    now: std::time::Instant,
    out: &mut Vec<HotbarVertex>,
) {
    // The counts belong to the hotbar rather than to the gauges: they
    // say what is in a slot, and a slot the player is about to spend
    // does not become less interesting because nobody is bleeding.
    let mut painter = Painter::onto(font, std::mem::take(out));
    stack_counts(&mut painter, inventory);
    *out = painter.into_vertices();

    let alpha = attention.alpha(
        now,
        gauges_are_worth_showing(
            health,
            max_health,
            recent_health,
            stamina,
            exhausted,
            breath,
            nourishment,
            body,
        ),
    );
    if alpha > 0.0 {
        let from = out.len();
        let mut painter = Painter::onto(font, std::mem::take(out));
        health_bar(&mut painter, health, max_health, recent_health);
        stamina_bar(&mut painter, stamina, exhausted);
        nourishment_bar(&mut painter, nourishment);
        hydration_bar(&mut painter, body.hydration);
        if rest_is_worth_showing(body.fatigue) {
            rest_bar(&mut painter, body.fatigue);
        }
        breath_bar(&mut painter, breath);
        temperature_gauge(&mut painter, body);
        wound_marks(&mut painter, &body.injuries);
        alarms(&mut painter, nourishment, breath, body, now);
        *out = painter.into_vertices();
        fade_from(&mut out[from..], alpha);
    }

    // A refusal is not a gauge. It is the game answering something the
    // player just did, it is on screen for three seconds, and fading it
    // on a rule about *health* would be the game deciding a player who
    // is well does not need to be told why their craft failed.
    if let Some((text, fade)) = notice {
        let mut painter = Painter::onto(font, std::mem::take(out));
        self::notice(&mut painter, text, fade);
        *out = painter.into_vertices();
    }
}

// ---- alarms ----

/// How fast a meter that is killing the player flashes, in flashes a
/// second. Two: fast enough to be seen from the corner of the eye while
/// fighting, slow enough not to read as a rendering fault.
const ALARM_HZ: f32 = 2.0;
const ALARM_FILL: [f32; 4] = [0.95, 0.12, 0.08, 0.45];
const ALARM_EDGE: [f32; 4] = [1.0, 0.25, 0.18, 1.0];

/// The tracks of the meters that are taking health right now.
///
/// **The one on the empty end, and only that one.** A meter at zero is the
/// reason the health bar is going down, and the complaint was that nothing
/// said which: "игрок не понимает от чего умирает". Hunger and thirst hurt
/// at empty, air at empty, and the temperature past either harm line
/// (`Comfort::Freezing`, `Comfort::Scorching`). A meter that is merely low
/// keeps its colour change -- that is the warning -- and does not flash,
/// because a warning that flashes is an alarm that has cried wolf.
fn hurting(nourishment: f32, breath: f32, body: BodyGauges) -> Vec<Rect> {
    use primitive_shared::body::Comfort;
    let band = |(y0, y1): (f32, f32)| Rect::new(BAR_LEFT, y0, BAR_LEFT + BAR_WIDTH, y1);
    let mut tracks = Vec::new();
    if nourishment.is_finite() && nourishment <= 0.0 {
        tracks.push(band(band_below(1)));
    }
    if body.hydration.is_finite() && body.hydration <= 0.0 {
        tracks.push(band(band_above(0)));
    }
    if breath.is_finite() && breath <= 0.0 {
        tracks.push(band(band_above(1)));
    }
    if matches!(Comfort::of(body.temperature_c), Comfort::Freezing | Comfort::Scorching) {
        tracks.push(band(band_above(3)));
    }
    tracks
}

/// Whether any meter is flashing, which is a reason to rebuild the
/// interface every frame: the flash has no event behind it.
pub fn alarming(nourishment: f32, breath: f32, body: BodyGauges) -> bool {
    !hurting(nourishment, breath, body).is_empty()
}

/// Flashes every meter in [`hurting`], over the meter's own track.
///
/// **Inside the track, never around it**: a border grown outward would
/// cross into the band next to it, and the gauges are tested never to
/// touch. The flash is a red wash over the track and the track's own edge
/// redrawn red, both fading in and out on one clock, so two meters that are
/// both killing the player flash together rather than chasing each other.
fn alarms(painter: &mut Painter, nourishment: f32, breath: f32, body: BodyGauges, now: std::time::Instant) {
    let tracks = hurting(nourishment, breath, body);
    if tracks.is_empty() {
        return;
    }
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let since = now.saturating_duration_since(*EPOCH.get_or_init(|| now)).as_secs_f32();
    let pulse = 0.5 - 0.5 * (since * ALARM_HZ * std::f32::consts::TAU).cos();
    let dim = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * pulse];
    for track in tracks {
        painter.quad(track, dim(ALARM_FILL));
        painter.border(track, BAR_EDGE_WIDTH, dim(ALARM_EDGE));
    }
}

// ---- notice ----
//
// The server refuses things: a placement with nothing in hand, a recipe
// that cannot be made, a throw the world has no room for. Those all used
// to go to stderr, which on a released build is a console nobody has
// open -- so the game simply did nothing and never said why.

/// How long a notice stays up, and how much of that is spent fading.
pub const NOTICE_SECONDS: f32 = 3.0;
pub const NOTICE_FADE_SECONDS: f32 = 0.6;

const NOTICE_SCALE: f32 = 0.86;
/// Half the height of the plate a notice is printed on.
const NOTICE_HALF_HEIGHT: f32 = (widgets::cell_height(NOTICE_SCALE) + 0.026) / 2.0;
/// Air between the top of the gauges and the bottom of that plate.
const NOTICE_CLEARANCE: f32 = 0.024;

/// Above the gauges, below the middle of the screen: in view without
/// sitting over the crosshair.
///
/// **Derived from the top of the stack rather than chosen**, and that
/// is a fix. It used to be `BAR_Y + 0.13` -- a number picked by eye
/// when there was one strip above the health gauge and never revisited
/// when there were three. Measured against what is drawn today, the
/// plate's lower edge sat seventeen thousandths *below* the top strip, so
/// a refusal while a player was cold was printed across the temperature
/// scale. The guard test did not catch it because all it knew was that
/// the notice had to be above the hotbar and below the crosshair, and
/// it was both.
const NOTICE_Y: f32 = STACK_TOP + NOTICE_CLEARANCE + NOTICE_HALF_HEIGHT;
const NOTICE_BG: [f32; 4] = [0.10, 0.04, 0.05, 0.88];
const NOTICE_EDGE: [f32; 4] = [0.85, 0.35, 0.30, 0.95];

fn notice(painter: &mut Painter, text: &str, fade: f32) {
    let fade = fade.clamp(0.0, 1.0);
    if fade <= 0.0 || text.is_empty() {
        return;
    }
    let dim = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * fade];

    let width = widgets::ink_width(text, NOTICE_SCALE);
    let rect = Rect::centred(0.0, NOTICE_Y, width + 0.048, NOTICE_HALF_HEIGHT * 2.0);
    painter.quad(rect, dim(NOTICE_BG));
    painter.border(rect, 0.0025, dim(NOTICE_EDGE));
    painter.text(
        text,
        -width / 2.0,
        rect.centre_y() + widgets::cell_height(NOTICE_SCALE) / 2.0 - 0.004,
        NOTICE_SCALE,
        dim(widgets::TEXT),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::BLOCK_STONE;

    #[test]
    fn a_player_is_told_when_the_heat_turns_and_not_every_time_it_wavers() {
        use crate::ui::lang::{Language, Msg};
        use primitive_shared::body::Comfort::*;
        use std::time::{Duration, Instant};

        // The crossings that ask something, and the ones that do not.
        assert_eq!(heat_notice(Comfortable, Warm), Some(Msg::HeatRising));
        assert_eq!(heat_notice(Warm, Scorching), Some(Msg::HeatStroke));
        assert_eq!(heat_notice(Comfortable, Scorching), Some(Msg::HeatStroke));
        assert_eq!(heat_notice(Warm, Comfortable), Some(Msg::HeatEased));
        assert_eq!(heat_notice(Scorching, Comfortable), Some(Msg::HeatEased));
        assert_eq!(
            heat_notice(Scorching, Warm),
            None,
            "easing from heatstroke said 'too hot' again, which reads as worse"
        );
        assert_eq!(heat_notice(Warm, Warm), None);
        for (was, now) in [(Comfortable, Cold), (Cold, Freezing), (Freezing, Cold), (Cold, Comfortable)] {
            assert_eq!(heat_notice(was, now), None, "the cold talked: {was:?} -> {now:?}");
        }

        // A body sitting on the line at the edge of a tree's shade.
        let language = Language::English;
        let said = Instant::now();
        let showing = (language.text(Msg::HeatRising).to_string(), said);
        let moments_later = said + Duration::from_secs(5);
        let a_while_later = said + Duration::from_secs_f32(HEAT_NOTICE_QUIET_SECS + 1.0);
        assert!(
            !heat_notice_allowed(Msg::HeatEased, Some(&showing), language, moments_later),
            "a body wavering on the line made the strip chatter"
        );
        assert!(
            heat_notice_allowed(Msg::HeatStroke, Some(&showing), language, moments_later),
            "heatstroke was held back behind 'too hot'"
        );
        assert!(heat_notice_allowed(Msg::HeatEased, Some(&showing), language, a_while_later));
        // Whatever else is on the strip does not silence the heat.
        let refusal = ("you cannot reach that".to_string(), said);
        assert!(heat_notice_allowed(Msg::HeatRising, Some(&refusal), language, moments_later));
        assert!(heat_notice_allowed(Msg::HeatRising, None, language, moments_later));
    }

    fn painter() -> Painter {
        Painter::new(FontAtlas::for_test())
    }

    /// The yard's own vertices, as offsets from the middle of the dial.
    fn yard_of(relative_wind: f32, angle: f32, strength: f32) -> Vec<(f32, f32)> {
        let mut painter = painter();
        sail_gauge(&mut painter, relative_wind, angle, strength, false);
        painter
            .vertices
            .iter()
            .filter(|v| v.tint == SAIL_DIAL_YARD)
            .map(|v| (v.position[0], v.position[1] - SAIL_DIAL_Y))
            .collect()
    }

    #[test]
    fn the_dial_lays_the_yard_where_the_sail_is_braced_and_never_outside_its_own_plate() {
        // The dial is the only way a player can see what their own hand did
        // to the sail -- the sail itself is a rectangle of hide seen edge-on
        // half the time -- so a yard drawn at the wrong angle here is a lie
        // about the one thing this instrument exists to say.
        //
        // A square yard lies *across* the raft, and the raft always points up
        // the dial: it is wide and it is not tall.
        let square = yard_of(0.0, 0.0, 1.0);
        let widest = square.iter().map(|p| p.0.abs()).fold(0.0f32, f32::max);
        let tallest = square.iter().map(|p| p.1.abs()).fold(0.0f32, f32::max);
        assert!(widest > tallest * 5.0, "a square yard is {widest} wide and {tallest} tall");

        // ...and bracing it round turns it about the middle of the dial by
        // exactly the angle asked for. Asked as a turn of the whole yard
        // rather than as the bearing of one end, because a yard has two ends
        // and a corner of a bar is not on the line through it.
        for step in -6..=6 {
            let angle = step as f32 / 6.0 * primitive_shared::raft::SAIL_MAX_ANGLE;
            let (sin, cos) = angle.sin_cos();
            let braced = yard_of(0.0, angle, 0.6);
            assert_eq!(braced.len(), square.len(), "a braced yard is a different shape");
            for (from, to) in square.iter().zip(&braced) {
                // Clockwise from straight up, which is how everything on this
                // dial is measured: see `spoke`.
                let turned = (from.0 * cos + from.1 * sin, from.1 * cos - from.0 * sin);
                assert!(
                    (turned.0 - to.0).abs() < 1e-4 && (turned.1 - to.1).abs() < 1e-4,
                    "the yard braced to {angle} put a corner at {to:?} where turning it puts {turned:?}"
                );
            }
        }

        // ...and nothing on it hangs off the plate it is drawn on, at any
        // trim and in any wind: a dial whose needle leaves the face is a
        // needle over the world, and half of it is then unreadable.
        for step in 0..32 {
            let angle = (step as f32 / 31.0 * 2.0 - 1.0) * primitive_shared::raft::SAIL_MAX_ANGLE;
            let toward = step as f32 / 32.0 * std::f32::consts::TAU;
            for strength in [0.0f32, 0.5, 1.0] {
                let mut painter = painter();
                sail_gauge(&mut painter, toward, angle, strength, true);
                for vertex in &painter.vertices {
                    let (x, y) = (vertex.position[0], vertex.position[1] - SAIL_DIAL_Y);
                    assert!(
                        x.abs() <= SAIL_DIAL_HALF + 0.004 && y.abs() <= SAIL_DIAL_HALF + 0.004,
                        "the dial drew ({x}, {y}) outside its own {SAIL_DIAL_HALF} plate at trim {angle}, wind {toward}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_dials_wind_arrow_comes_from_where_the_wind_comes_from_and_grows_with_it() {
        // The wind blows *toward* `relative_wind`, so the arrow has to start
        // from the opposite side of the dial: a player reads "it is coming at
        // me from over there", which is what decides which way to brace.
        let arrow = |toward: f32, strength: f32| {
            let mut painter = painter();
            sail_gauge(&mut painter, toward, 0.0, strength, false);
            painter
                .vertices
                .iter()
                .filter(|v| v.tint == SAIL_DIAL_WIND)
                .map(|v| (v.position[0], v.position[1] - SAIL_DIAL_Y))
                .fold((0.0f32, 0.0f32), |far, p| if p.0.hypot(p.1) > far.0.hypot(far.1) { p } else { far })
        };
        // A wind blowing toward the bow comes from astern, so the arrow is
        // drawn below the middle of the dial.
        let astern = arrow(0.0, 1.0);
        assert!(astern.1 < -0.01, "a following wind was drawn ahead of the raft: {astern:?}");
        let ahead = arrow(std::f32::consts::PI, 1.0);
        assert!(ahead.1 > 0.01, "a head wind was drawn astern: {ahead:?}");
        // ...and a squall reaches further out of the dial than a calm.
        let calm = arrow(0.0, 0.0);
        assert!(
            astern.0.hypot(astern.1) > calm.0.hypot(calm.1) + 0.01,
            "a gale's arrow is no longer than a calm's: {astern:?} against {calm:?}"
        );
    }

    /// Every thumb control shows an edge against any world.
    ///
    /// **The property changed with the drawing, and the old one was the
    /// weaker of the two.** The controls used to be filled slabs, and
    /// what was asserted was that the fill was opaque enough to be the
    /// label's own ground. They are frames now -- no fill at all, which
    /// gives the player back the piece of world every button was
    /// standing on -- so there is nothing to measure a label against
    /// except the terrain.
    ///
    /// What holds instead is that every line is a *pair*: a dark stroke
    /// and a pale one, side by side. A single line has one brightness
    /// and the world has all of them -- a pale hairline over snow is
    /// 1.02:1, which is no line at all -- but two strokes at opposite
    /// ends of the range cannot both vanish into a colour that lies
    /// between them.
    ///
    /// So: for every world, and for both states, at least one of the
    /// two tones reaches the floor. Not both, and demanding both is
    /// what would force the fill back.
    #[test]
    fn every_thumb_control_shows_an_edge_against_any_world() {
        use crate::ui::widgets::{contrast, over};

        /// Large marks -- a frame and letters filling a thumb-sized box
        /// -- so the same threshold `widgets` holds its quiet tier to.
        const FLOOR: f32 = 3.0;

        for (world, colour) in [
            ("cave", [0.02, 0.02, 0.025, 1.0]),
            ("grass", [0.25, 0.42, 0.16, 1.0]),
            ("sand", [0.76, 0.70, 0.50, 1.0]),
            ("snow", [0.85, 0.87, 0.90, 1.0]),
            ("sky", [0.45, 0.62, 0.90, 1.0]),
            // The two ends, so the sweep is not only of colours that
            // happen to be in the game today.
            ("black", [0.0, 0.0, 0.0, 1.0]),
            ("white", [1.0, 1.0, 1.0, 1.0]),
        ] {
            let dark = contrast(over(EDGE_DARK, colour), colour);
            for (state, pale) in [("resting", EDGE_IDLE), ("pressed", EDGE_PRESSED)] {
                let light = contrast(over(pale, colour), colour);
                assert!(
                    dark.max(light) >= FLOOR,
                    "{world}, {state}: neither stroke shows -- dark {dark:.2}:1,                      pale {light:.2}:1",
                );
            }
        }
    }

    /// A pressed control looks different from a resting one.
    ///
    /// With the fill gone, brightness is the only thing left to say it
    /// with, so it has to say it loudly enough to be seen against the
    /// world rather than only in a palette.
    #[test]
    fn a_pressed_control_is_told_apart_from_a_resting_one() {
        use crate::ui::widgets::{contrast, over};

        for (world, colour) in [
            ("cave", [0.02, 0.02, 0.025, 1.0]),
            ("grass", [0.25, 0.42, 0.16, 1.0]),
            ("snow", [0.85, 0.87, 0.90, 1.0]),
        ] {
            let resting = over(EDGE_IDLE, colour);
            let pressed = over(EDGE_PRESSED, colour);
            let change = contrast(resting, pressed);
            assert!(
                change >= 1.5,
                "{world}: pressing changes the line by {change:.2}:1, which is not a change",
            );
        }
    }

    /// The y extent of everything drawn, so tests can check the HUD
    /// stays where it belongs.
    fn horizontal_extent(vertices: &[HotbarVertex]) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[0]), hi.max(v.position[0]))
        })
    }

    fn vertical_extent(vertices: &[HotbarVertex]) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[1]), hi.max(v.position[1]))
        })
    }

    /// Total width of everything drawn in the health-fill colour.
    ///
    /// Summed rather than measured end to end, because the gauge is cut
    /// into segments with gaps between them: the lit part is several
    /// quads, and what "half health" means is that half the lit area is
    /// there.
    ///
    /// Picked out by colour rather than by size, because the track and
    /// the border both span the whole gauge whatever the health is.
    fn lit_width(vertices: &[HotbarVertex], fraction: f32) -> f32 {
        let want = fill_colour(fraction);
        vertices
            .chunks(6)
            .filter(|quad| quad[0].tint == want)
            .map(|quad| {
                let (lo, hi) = quad.iter().fold((f32::MAX, f32::MIN), |(l, h), v| {
                    (l.min(v.position[0]), h.max(v.position[0]))
                });
                hi - lo
            })
            .sum()
    }

    /// How many segments have any fill in them at all.
    fn lit_segments(vertices: &[HotbarVertex], fraction: f32) -> usize {
        let want = fill_colour(fraction);
        vertices.chunks(6).filter(|q| q[0].tint == want).count()
    }

    /// The horizontal extent of everything drawn.
    fn total_extent(vertices: &[HotbarVertex]) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(l, h), v| {
            (l.min(v.position[0]), h.max(v.position[0]))
        })
    }

    #[test]
    fn only_the_meter_that_is_taking_health_flashes() {
        // "игрок не понимает от чего умирает": the empty one says so.
        let calm = BodyGauges::default();
        assert!(hurting(0.5, 1.0, calm).is_empty(), "a flash with nothing hurting");
        assert!(hurting(0.05, 1.0, calm).is_empty(), "a low meter is a warning, not an alarm");
        assert_eq!(hurting(0.0, 1.0, calm).len(), 1, "starving did not flash");
        let parched = BodyGauges { hydration: 0.0, ..calm };
        assert_eq!(hurting(0.0, 1.0, parched).len(), 2, "starving and parched are two alarms");
        let frozen = BodyGauges { temperature_c: primitive_shared::body::FREEZING - 1.0, ..calm };
        assert_eq!(hurting(1.0, 1.0, frozen).len(), 1, "freezing did not flash");
        assert_eq!(hurting(1.0, 0.0, calm).len(), 1, "drowning did not flash");
    }

    #[test]
    fn the_bar_fills_in_proportion_to_health() {
        let mut full = painter();
        health_bar(&mut full, 20.0, 20.0, 20.0);
        let mut half = painter();
        health_bar(&mut half, 10.0, 20.0, 10.0);
        let mut empty = painter();
        health_bar(&mut empty, 0.0, 20.0, 0.0);

        let full_width = lit_width(&full.vertices, 1.0);
        let half_width = lit_width(&half.vertices, 0.5);
        assert!(
            (half_width - full_width / 2.0).abs() < 0.01,
            "half health lit {half_width} against a full {full_width}"
        );
        assert_eq!(
            lit_segments(&full.vertices, 1.0),
            SEGMENTS,
            "full health should light every segment"
        );
        assert_eq!(
            lit_segments(&half.vertices, 0.5),
            SEGMENTS / 2,
            "half health should light half the segments"
        );
        assert_eq!(
            lit_segments(&empty.vertices, 0.0),
            0,
            "an empty bar still lit something"
        );
    }

    #[test]
    fn a_part_spent_segment_is_drawn_part_full() {
        // Rounding to whole segments would make the last point of health
        // disappear a segment early, which is the one place the bar has
        // to be exactly right.
        let mut p = painter();
        health_bar(&mut p, 19.0, 20.0, 19.0);
        let fraction = 19.0 / 20.0;
        assert_eq!(
            lit_segments(&p.vertices, fraction),
            SEGMENTS,
            "the part-spent segment vanished instead of shrinking"
        );
        let full_segment = METER_SPAN / SEGMENTS as f32 * (1.0 - SEGMENT_GAP);
        let lit = lit_width(&p.vertices, fraction);
        assert!(
            lit < full_segment * SEGMENTS as f32,
            "nineteen of twenty drew a completely full bar"
        );
    }

    #[test]
    fn the_bar_sits_above_the_hotbar_and_on_screen() {
        let mut p = painter();
        health_bar(&mut p, 20.0, 20.0, 20.0);
        let (lo, hi) = vertical_extent(&p.vertices);
        assert!(lo > BOTTOM + SLOT, "the bar overlaps the hotbar");
        assert!(hi < 1.0 && lo > -1.0, "the bar runs off the screen");
        let (left, right) = total_extent(&p.vertices);
        // Authored as if the window were square, so a square window is
        // the worst case for running off the sides.
        assert!(left > -1.0 && right < 1.0, "the gauge runs from {left} to {right}");
    }

    #[test]
    fn the_colour_warns_before_the_bar_is_nearly_gone() {
        // The point of colouring it at all: a player reads the colour
        // peripherally, long before they look at the length.
        let healthy = fill_colour(1.0);
        let hurt = fill_colour(0.5);
        let dying = fill_colour(0.1);
        assert!(healthy[1] > healthy[0], "full health should read green");
        assert!(dying[0] > dying[1], "low health should read red");
        assert!(
            hurt[0] > healthy[0] && hurt[1] > dying[1],
            "the middle should be amber, between the two"
        );
    }

    /// The gauge arrives when the drift starts, not when it bites.
    ///
    /// **It used to arrive a third of the way along.** Whether it was
    /// drawn was decided by `Comfort`, which turns Cold at 22 degrees,
    /// and how full it was drawn was measured from `COMFORT_LOW` at 26
    /// -- two different pairs of thresholds, four degrees apart. So the
    /// bar popped onto the screen already 29% filled and the four
    /// degrees before that, which are the ones a player can still do
    /// something about, showed nothing at all.
    #[test]
    fn the_temperature_gauge_appears_where_it_starts_filling() {
        use primitive_shared::body;

        assert!(
            !temperature_is_worth_showing(body::NEUTRAL_C),
            "a comfortable player is being shown a gauge that says so",
        );
        assert!(
            !temperature_is_worth_showing(body::COMFORT_LOW)
                && !temperature_is_worth_showing(body::COMFORT_HIGH),
            "the edges of the band are still inside it",
        );
        assert!(
            temperature_is_worth_showing(body::COMFORT_LOW - 0.1)
                && temperature_is_worth_showing(body::COMFORT_HIGH + 0.1),
            "the drift out of the band drew nothing, which is the warning missed",
        );

        // ...and it arrives at its own edge rather than part-filled:
        // the run drawn is measured from the same degree that decides
        // whether to draw at all.
        let on_arrival =
            temperature_at(body::COMFORT_LOW) - temperature_at(body::COMFORT_LOW - 0.1);
        assert!(
            on_arrival < 0.02,
            "the gauge appears {:.0}% along its own track",
            on_arrival * 100.0,
        );
    }

    /// A degree is drawn where it falls on the scale.
    ///
    /// The point of an absolute track: the marker's position answers
    /// "how much further before this hurts", which is the question a
    /// bar measured from the middle could not be asked.
    #[test]
    fn a_degree_is_drawn_where_it_falls_on_the_scale() {
        use primitive_shared::body;

        assert_eq!(temperature_at(body::FREEZING), 0.0);
        assert_eq!(temperature_at(body::SCALDING), 1.0);
        // Past either end reads as the end, not as a marker off the bar.
        assert_eq!(temperature_at(-40.0), 0.0);
        assert_eq!(temperature_at(200.0), 1.0);
        // The comfortable band straddles the middle, which is what makes
        // "which way am I drifting" readable without a legend.
        let neutral = temperature_at(body::NEUTRAL_C);
        assert!(
            (0.4..0.7).contains(&neutral),
            "the comfortable middle sits at {neutral} of the track",
        );
        // Monotone, or the marker would walk backwards as it got colder.
        let mut previous = -1.0;
        let mut c = body::FREEZING;
        while c <= body::SCALDING {
            let here = temperature_at(c);
            assert!(here >= previous, "the scale doubles back at {c}");
            previous = here;
            c += 0.5;
        }
    }

    /// The gauge is hidden when it has nothing to say, and drawn when it
    /// has.
    #[test]
    fn a_comfortable_player_is_shown_no_temperature_gauge() {
        use primitive_shared::body::{self, Comfort};

        let mut comfortable = painter();
        temperature_gauge(&mut comfortable, BodyGauges::default());
        assert!(
            comfortable.vertices.is_empty(),
            "a comfortable player got a gauge telling them so",
        );

        let mut drifting = painter();
        temperature_gauge(
            &mut drifting,
            BodyGauges {
                temperature_c: body::COMFORT_LOW - 1.0,
                // Still `Comfortable` at this temperature -- the server
                // does not name it until `CHILLED`. Drawing it anyway is
                // the whole change.
                comfort: Comfort::Comfortable,
                ..Default::default()
            },
        );
        assert!(
            !drifting.vertices.is_empty(),
            "a player already leaving the band was shown nothing",
        );
    }

    /// The degrees written at the end of the gauge fit where they are
    /// put, and nothing the gauge draws runs under them.
    ///
    /// Same argument as the health figure's own reserve: a number that
    /// runs past the end of the hotbar is a heads-up display hanging
    /// off the side of the screen, and it only happens at the widths
    /// nobody typed into the test. What changed is which end it would
    /// happen at -- the reading used to sit beside the strip with the
    /// screen's own margin to spill into, and now it sits in a column
    /// the strip stops short of.
    #[test]
    fn the_temperature_reading_fits_beside_its_gauge() {
        // The widest it gets: three characters of below-zero body
        // temperature and the unit.
        let widest = widgets::ink_width("-12C", TEMP_TEXT_SCALE);
        let ends_at = READOUT_LEFT + READOUT_PAD + widest;
        assert!(
            ends_at <= crate::ui::hotbar::RIGHT,
            "the reading ends at {ends_at}, past the bar's {}",
            crate::ui::hotbar::RIGHT,
        );
        // ...and the scale itself keeps out of the column. Drawn, not
        // reasoned about: the hot end of the track is exactly where the
        // marker would be at `SCALDING`.
        let mut p = painter();
        temperature_gauge(
            &mut p,
            BodyGauges {
                temperature_c: primitive_shared::body::SCALDING,
                comfort: primitive_shared::body::Comfort::of(primitive_shared::body::SCALDING),
                ..Default::default()
            },
        );
        let zones: f32 = p
            .vertices
            .chunks(6)
            .filter(|quad| quad[0].tint == ZONE_HOT_HARM)
            .map(|quad| quad.iter().fold(f32::MIN, |h, v| h.max(v.position[0])))
            .fold(f32::MIN, f32::max);
        assert!(
            zones <= READOUT_LEFT - READOUT_GAP + 1e-6,
            "the scale reaches {zones}, into a readout column starting at {READOUT_LEFT}",
        );
    }

    #[test]
    fn recent_damage_leaves_a_draining_strip() {
        let mut settled = painter();
        health_bar(&mut settled, 12.0, 20.0, 12.0);
        let mut just_hit = painter();
        health_bar(&mut just_hit, 12.0, 20.0, 19.0);
        assert!(
            just_hit.vertices.len() > settled.vertices.len(),
            "a fresh hit drew no ghost strip"
        );
    }

    /// The room kept for the figure actually holds it.
    ///
    /// `READOUT_RESERVE` is a number, and a number written beside a
    /// font is a number that stops being true when the font, the scale
    /// or the format changes. Everything the meters draw stops at this
    /// column, so being wrong is either a number over a gauge or a
    /// stretch of empty bar.
    #[test]
    fn the_room_kept_for_the_readout_actually_holds_it() {
        let room = READOUT_RESERVE - 2.0 * READOUT_PAD;
        let usual = widgets::ink_width("20/20", HEALTH_TEXT_SCALE);
        assert!(
            usual <= room,
            "the usual reading is {usual} wide against {room} of room",
        );
        // ...and the degrees beside it, which are written smaller and
        // may carry a minus sign.
        let coldest = widgets::ink_width("-12C", TEMP_TEXT_SCALE);
        assert!(
            coldest <= room,
            "the coldest reading is {coldest} wide against {room} of room",
        );
        // A server with a bigger maximum used to overflow into spare
        // width to the right of the bar; there is none now, because the
        // stack ends where the hotbar does. It is lettered smaller
        // instead, and the point of the test is that it still fits.
        // Up to four digits, which is where the floor takes over: a
        // five-digit maximum would have to be lettered at under a third
        // of the health figure's size to fit, and an unreadable number
        // inside the bar is worse than a small one overflowing it. That
        // is `fitted_scale`'s own argument and this is where it bites.
        for maximum in [0, 1, 6, 20, 100, 999, 9_999] {
            let widest = widest_reading(maximum);
            // The stand-in has to be the length of the real thing, or
            // it is fitting the wrong string.
            assert_eq!(
                widest.len(),
                format!("{}/{}", maximum.max(1), maximum.max(1)).len(),
                "the stand-in for a maximum of {maximum} is the wrong length",
            );
            let scale = widgets::fitted_scale(widest, HEALTH_TEXT_SCALE, room, 0.5);
            let drawn = widgets::ink_width(widest, scale);
            assert!(
                drawn <= room + 1e-6,
                "a maximum of {maximum} writes {drawn} wide into {room} of well",
            );
        }
    }

    /// A figure is printed on something it can be read on.
    ///
    /// **The reason the wells exist.** The two readings used to be
    /// written straight over the world with a one-pixel drop shadow --
    /// the trick `label_in_two_tones` uses for thumb controls, which is
    /// held to 3:1 because a frame and a capital letter filling a
    /// thumb-sized box are large marks. A five-character number at a
    /// fifth of that size is small text and owes 4.5:1, and over snow
    /// the pale glyphs measured 1.03:1 against what was behind them.
    ///
    /// An opaque plate makes the answer the same whatever the world is
    /// doing, which is what this asserts: the ratio does not depend on
    /// the terrain at all.
    #[test]
    fn a_figure_is_printed_on_something_it_can_be_read_on() {
        use crate::ui::widgets::{contrast, over};

        /// Small text, so the reading threshold rather than the mark
        /// one. See `small_text_is_readable_against_everything_it_is_drawn_on`.
        const FLOOR: f32 = 4.5;

        for (world, colour) in [
            ("cave", [0.02, 0.02, 0.025, 1.0]),
            ("grass", [0.25, 0.42, 0.16, 1.0]),
            ("sand", [0.76, 0.70, 0.50, 1.0]),
            ("snow", [0.85, 0.87, 0.90, 1.0]),
            ("sky", [0.45, 0.62, 0.90, 1.0]),
            ("black", [0.0, 0.0, 0.0, 1.0]),
            ("white", [1.0, 1.0, 1.0, 1.0]),
        ] {
            let ground = over(READOUT_WELL, colour);
            let ratio = contrast(HEALTH_TEXT, ground);
            assert!(
                ratio >= FLOOR,
                "over {world} the figure reads at {ratio:.2}:1",
            );
        }
        // ...and the same figure over the gauge it belongs to, which is
        // where it would land if the well were ever dropped: the
        // brightest a filled segment gets is what it has to beat.
        let bare = contrast(HEALTH_TEXT, fill_colour(1.0));
        assert!(
            bare < FLOOR,
            "the figure reads at {bare:.2}:1 on the segments, so the well is decoration",
        );

        // The rejected answer, measured rather than remembered: the
        // pale glyph with a dark copy one pixel behind it, over the
        // world, which is how these two figures were drawn until now.
        // It works at the ends of the range and fails in the middle,
        // where neither stroke is far from what it is on -- and the
        // middle is grass, stone and dirt.
        const OLD_SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.85];
        for (world, colour) in [
            ("grass", [0.25, 0.42, 0.16, 1.0]),
            ("stone", [0.24, 0.24, 0.24, 1.0]),
        ] {
            let glyph = contrast(HEALTH_TEXT, colour);
            let shadow = contrast(over(OLD_SHADOW, colour), colour);
            let best = glyph.max(shadow);
            assert!(
                best < FLOOR,
                "over {world} the shadowed figure already read at {best:.2}:1, \
                 so the wells bought nothing",
            );
        }
    }

    /// The gauges are centred on the bar they hang over.
    ///
    /// **They were not, and it was the first thing anyone noticed.**
    /// The gauge sat against the hotbar's left edge with the figure
    /// beside it, so the middle of the assembly landed at `-0.2345`
    /// while the bar's middle was zero: a quarter of the bar's width
    /// out, with the right half of it empty. The whole display read as
    /// slid to one side.
    ///
    /// Now it is centred by being the same width, which is a stronger
    /// property than the one this used to check: a stack the width of
    /// the bar is centred on it *and* covers it, and the old one could
    /// be centred while covering half.
    #[test]
    fn the_gauges_are_centred_on_the_hotbar() {
        use crate::ui::hotbar;
        let bar_middle = (hotbar::LEFT + hotbar::RIGHT) / 2.0;
        let stack_middle = BAR_LEFT + BAR_WIDTH / 2.0;
        assert!(
            (stack_middle - bar_middle).abs() < 0.001,
            "the gauges sit at {stack_middle} over a bar centred on {bar_middle}",
        );
        // ...and reach both ends of it, hairline included.
        let drawn = BAR_WIDTH + 2.0 * BAR_EDGE_WIDTH;
        let bar = hotbar::RIGHT - hotbar::LEFT;
        assert!(
            (drawn - bar).abs() < 0.001,
            "the gauges are {drawn} wide over a bar {bar} wide",
        );
    }

    /// A figure stays inside the gap around its strip.
    ///
    /// The degrees are written bigger than the strip they belong to is
    /// tall -- the cap height alone is half again `STAMINA_HEIGHT` --
    /// so their well stands proud of it on both sides. That the proud
    /// edge fits the gap is asserted at build time, beside the
    /// constants. What cannot be asserted there is that the thing
    /// actually *drawn* stands exactly that proud -- and it is that
    /// number, through `STACK_TOP`, which everything above the gauges
    /// is placed against.
    #[test]
    fn a_figure_stays_inside_the_gap_around_its_strip() {
        let mut p = painter();
        temperature_gauge(
            &mut p,
            BodyGauges {
                temperature_c: primitive_shared::body::FREEZING,
                comfort: primitive_shared::body::Comfort::of(primitive_shared::body::FREEZING),
                ..Default::default()
            },
        );
        let (low, high) = vertical_extent(&p.vertices);
        let (strip_low, strip_high) = band_above(BANDS_ABOVE - 1);
        assert!(
            (high - STACK_TOP).abs() < 1e-6
                && (high - strip_high - TEMP_WELL_PROUD).abs() < 1e-6,
            "the top strip is drawn {low}..{high} against a band \
             {strip_low}..{strip_high} and a well said to stand \
             {TEMP_WELL_PROUD} proud",
        );
        // ...and the health figure's well is not proud at all: its
        // gauge is deep enough to hold it, so the well is exactly the
        // track and the two hairlines coincide. A doubled edge a
        // thousandth apart is what this stops.
        let health_cap =
            widgets::PIXEL * HEALTH_TEXT_SCALE * crate::engine::font::CAP_HEIGHT as f32;
        assert!(
            health_cap + 2.0 * WELL_PAD <= BAR_HEIGHT,
            "the health figure's well is taller than the gauge holding it",
        );
    }

    #[test]
    fn a_ghost_below_current_health_is_ignored() {
        // `recent` lags downward only. Healing must not draw a strip
        // hanging off the end of the bar.
        let mut p = painter();
        health_bar(&mut p, 18.0, 20.0, 3.0);
        let (_, right) = p.vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[0]), hi.max(v.position[0]))
        });
        // The end of the track, not half the gauge's width. Those were
        // the same number while the gauge was centred on the screen;
        // they stopped being the same when the gauge and its figure
        // were centred *together*, and the bound has to mean "nothing
        // hangs off the end" rather than name a coordinate that used to
        // be the end.
        assert!(
            right <= BAR_LEFT + BAR_WIDTH + BAR_EDGE_WIDTH + 1e-6,
            "something ran past the track: {right} against {}",
            BAR_LEFT + BAR_WIDTH,
        );
    }

    /// A moment `ms` after an arbitrary start.
    ///
    /// The clock is handed to `build_into` rather than read inside it
    /// so that a rule about seconds can be tested without spending
    /// them: a fade that can only be checked by waiting is a fade that
    /// gets checked once.
    fn moment(ms: u64) -> std::time::Instant {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *START.get_or_init(std::time::Instant::now) + std::time::Duration::from_millis(ms)
    }

    /// The gauges, drawn at one instant, with everything as it is.
    #[allow(clippy::too_many_arguments)]
    fn stack_at(
        attention: &mut Attention,
        now: std::time::Instant,
        health: f32,
        stamina: f32,
        breath: f32,
        nourishment: f32,
        body: BodyGauges,
    ) -> Vec<HotbarVertex> {
        let mut vertices = Vec::new();
        build_into(
            FontAtlas::for_test(),
            health,
            20.0,
            health,
            stamina,
            false,
            breath,
            nourishment,
            body,
            &Inventory::new(),
            None,
            attention,
            now,
            &mut vertices,
        );
        vertices
    }

    /// Everything as it is when nothing is wrong.
    fn all_well() -> BodyGauges {
        BodyGauges {
            temperature_c: (primitive_shared::body::COMFORT_LOW
                + primitive_shared::body::COMFORT_HIGH)
                / 2.0,
            comfort: primitive_shared::body::Comfort::of(
                (primitive_shared::body::COMFORT_LOW + primitive_shared::body::COMFORT_HIGH) / 2.0,
            ),
            hydration: 1.0,
            fatigue: 0.0,
            injuries: primitive_shared::injury::Injuries::default(),
            // The four the health page reads and the HUD does not.
            // `..Default::default()` would have been shorter and would
            // also have stopped these tests failing the next time a
            // gauge is added -- which is the one thing they are for.
            wetness: 0.0,
            grime: 0.0,
            recovery: 1.0,
            diet_groups: 0,
            shelter: Default::default(),
            smoke: 0.0,
        }
    }

    /// The most opaque thing in a drawing, which is what "is this on
    /// screen" comes down to once everything is faded together.
    fn strongest(vertices: &[HotbarVertex]) -> f32 {
        vertices.iter().fold(0.0f32, |most, v| most.max(v.tint[3]))
    }

    /// The gauges go quiet when there is nothing wrong, and come back
    /// the moment there is.
    ///
    /// **What this buys, measured on the phone this is played on**: the
    /// four strips and their figures hold a band about 900 px wide and
    /// 150 tall across the bottom of the world, and for most of a game
    /// every one of them is saying "full". The rule the two meters that
    /// already hid themselves were written to -- a meter that is always
    /// full is a meter nobody reads -- now covers the other four.
    /// Draws the gauge stack, blown up, for looking at.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib -- --ignored dump_the_gauges
    /// ```
    ///
    /// **At three times the size, and that is the point of it.** The
    /// interface snapshot draws this stack authored, into a picture 720
    /// pixels tall -- so one unit of interface space is 360 pixels there
    /// and 540 on the screen it is actually played on, and everything
    /// small in it looks half as legible as it is. The marks beside the
    /// meters are the smallest thing this file draws; judging them from
    /// a picture that under-draws them by a third is how a mark that
    /// works gets thrown away.
    #[test]
    #[ignore = "diagnostic: writes a picture of the gauge stack"]
    fn dump_the_gauges_to_a_png() {
        const WIDTH: u32 = 1600;
        const HEIGHT: u32 = 900;
        let mut vertices = Vec::new();
        build_into(
            FontAtlas::for_test(),
            13.0,
            20.0,
            16.0,
            0.55,
            false,
            0.4,
            0.62,
            BodyGauges {
                temperature_c: 12.0,
                comfort: primitive_shared::body::Comfort::of(12.0),
                hydration: 0.48,
                fatigue: 0.0,
                injuries: primitive_shared::injury::Injuries::default(),
                wetness: 0.0,
                grime: 0.0,
                recovery: 1.0,
                diet_groups: 0,
                shelter: Default::default(),
                smoke: 0.0,
            },
            &Inventory::new(),
            None,
            &mut Attention::default(),
            std::time::Instant::now(),
            &mut vertices,
        );
        // Grown about the bottom of the glass, exactly as the frame
        // loop grows it -- see the `scale_about` beside `hud::build_into`
        // in `lib.rs`. Three, which is above anything the interface size
        // offers, so that a cell of a mark is several pixels and what is
        // wrong with one is visible.
        widgets::scale_about(
            &mut vertices,
            widgets::anchor::BOTTOM(WIDTH as f32 / HEIGHT as f32),
            3.0,
        );
        let path = std::env::var("PRIMITIVE_UI_DUMP")
            .unwrap_or_else(|_| "target/gauges.png".to_string());
        widgets::dump_to_png(&vertices, WIDTH, HEIGHT, &path);
        println!("wrote {path}");
    }

    #[test]
    fn the_gauges_go_quiet_when_nothing_is_wrong_and_come_back_when_something_is() {
        let mut attention = Attention::default();
        // A session opens with them on screen, whatever the readings:
        // a player arriving in a world should see what the game gives
        // them before it takes it away again.
        let opening = stack_at(&mut attention, moment(0), 20.0, 1.0, 1.0, 1.0, all_well());
        assert_eq!(strongest(&opening), 1.0, "the stack was not there to start with");

        // A few seconds of nothing at all, and it is gone.
        let quiet = stack_at(&mut attention, moment(9_000), 20.0, 1.0, 1.0, 1.0, all_well());
        assert!(
            strongest(&quiet) <= 0.0,
            "the gauges were still on screen with nothing to say: {}",
            strongest(&quiet),
        );

        // ...and one scratch brings the whole stack back at once, in
        // the same frame it happened.
        let hurt = stack_at(&mut attention, moment(9_016), 19.0, 1.0, 1.0, 1.0, all_well());
        assert_eq!(
            strongest(&hurt),
            1.0,
            "the player was hurt and the health bar was still fading out",
        );
    }

    /// A warning is never faded out.
    ///
    /// The brief this was built to says it in as many words: do not
    /// hide what carries a warning. The two that matter are air running
    /// out and a temperature outside the comfort band, and both of them
    /// are *already* hidden while they are fine -- so the failure would
    /// be silent, a meter that appears because a player is drowning and
    /// then fades away while they still are.
    ///
    /// It cannot happen by construction rather than by care, and this
    /// says so: the same readings the gauges are drawn from decide
    /// whether there is anything to say, in the same call.
    #[test]
    fn a_warning_is_never_faded_out_however_long_it_lasts() {
        use primitive_shared::body;
        let freezing = BodyGauges {
            temperature_c: body::FREEZING + 1.0,
            comfort: body::Comfort::of(body::FREEZING + 1.0),
            hydration: 1.0,
            fatigue: 0.0,
            injuries: primitive_shared::injury::Injuries::default(),
            // The four the health page reads and the HUD does not.
            // `..Default::default()` would have been shorter and would
            // also have stopped these tests failing the next time a
            // gauge is added -- which is the one thing they are for.
            wetness: 0.0,
            grime: 0.0,
            recovery: 1.0,
            diet_groups: 0,
            shelter: Default::default(),
            smoke: 0.0,
        };
        for (name, reading) in [
            ("air running out", (0.3f32, 1.0f32, all_well())),
            ("a cold player", (1.0, 1.0, freezing)),
            ("an empty stomach", (1.0, 0.2, all_well())),
        ] {
            let (breath, nourishment, body) = reading;
            let mut attention = Attention::default();
            // Long enough that anything with a timer on it has run out
            // several times over.
            for minute in 0..5u64 {
                let drawn = stack_at(
                    &mut attention,
                    moment(minute * 60_000),
                    20.0,
                    1.0,
                    breath,
                    nourishment,
                    body,
                );
                assert_eq!(
                    strongest(&drawn),
                    1.0,
                    "{name} was faded out after {minute} minutes",
                );
            }
        }
    }

    /// What is in a slot is not a gauge, and does not go with them.
    ///
    /// A player who is well, fed and watered is exactly the player who
    /// is about to spend what they are carrying, and a count that
    /// vanished because nothing was wrong would be the auto-hiding
    /// taking away the one number that is never about the player at
    /// all.
    #[test]
    fn the_counts_on_the_hotbar_do_not_fade_with_the_gauges() {
        let mut stocked = Inventory::new();
        stocked.add(BLOCK_STONE, 42);
        let mut attention = Attention::default();
        let mut vertices = Vec::new();
        for at in [moment(0), moment(20_000)] {
            vertices.clear();
            build_into(
                FontAtlas::for_test(),
                20.0,
                20.0,
                20.0,
                1.0,
                false,
                1.0,
                1.0,
                all_well(),
                &stocked,
                None,
                &mut attention,
                at,
                &mut vertices,
            );
        }
        assert!(
            !vertices.is_empty() && strongest(&vertices) >= 1.0,
            "the stack count went with the gauges",
        );
        // ...and what is left really is only the count: the gauges are
        // gone rather than merely dim.
        let (lo, hi) = vertical_extent(&vertices);
        assert!(
            lo >= BOTTOM - 0.01 && hi <= BOTTOM + SLOT + 0.01,
            "something other than a stack count survived the fade: {lo}..{hi}",
        );
    }

    #[test]
    fn an_empty_slot_shows_no_count_and_a_stocked_one_does() {
        let empty = Inventory::new();
        let mut p = painter();
        stack_counts(&mut p, &empty);
        assert!(
            p.vertices.is_empty(),
            "an empty hotbar should print no numbers at all"
        );

        let mut stocked = Inventory::new();
        stocked.add(BLOCK_STONE, 5);
        let mut p = painter();
        stack_counts(&mut p, &stocked);
        assert!(!p.vertices.is_empty(), "a stocked slot printed no count");
    }

    #[test]
    fn counts_stay_inside_their_slots() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 300);
        let mut p = painter();
        stack_counts(&mut p, &inventory);
        let (lo, hi) = vertical_extent(&p.vertices);
        assert!(
            lo >= BOTTOM - 0.01 && hi <= BOTTOM + SLOT + 0.01,
            "a stack count ({lo}..{hi}) escaped its slot"
        );
    }

    /// **The other half of the same bug.** `no_meter_lands_on_the_hotbar`
    /// checks that nothing here overlaps the bar *vertically*; nothing
    /// checked horizontally, and so every strip in this file hung a
    /// sixth of the screen out past the left end of the thing it is
    /// drawn over for as long as it has existed.
    ///
    /// The slack the readout used to be allowed on the right is gone
    /// with the reason for it: the figures are inside the stack now,
    /// and the stack is the bar. Nothing may start left of the bar and
    /// nothing may end right of it.
    #[test]
    fn the_gauges_are_laid_out_against_the_hotbar() {
        use crate::ui::hotbar;
        // That the *constants* line up is asserted at build time, beside
        // them. What this checks is that what is actually drawn stays
        // there too, which they only imply: a border is drawn outside
        // the rectangle it is given, and the overhang that produced was
        // the whole of the original bug.
        let mut vertices = Vec::new();
        build_into(
            FontAtlas::for_test(),
            13.0,
            20.0,
            16.0,
            1.0,
            false,
            1.0,
            1.0,
            BodyGauges::default(),
            &Inventory::new(),
            None,
            &mut Attention::default(),
            std::time::Instant::now(),
            &mut vertices,
        );
        let (lo, hi) = horizontal_extent(&vertices);
        // **The marks are the one thing outside the bar's span, and by
        // exactly the column they were given.** Everything else here is
        // still held to the hotbar's own edges -- that was the original
        // bug and it has not stopped being one. What changed is that
        // there is now a column of pictograms to the left of the
        // gauges; see `band_icon`. Written as the column rather than as
        // a slacker bound, so a mark that grew or wandered still fails.
        let marks = ICON_GAP + ICON_SIZE;
        assert!(
            lo >= hotbar::LEFT - marks - 1e-6,
            "something is drawn at {lo}, left of the marks beside the bar at {}",
            hotbar::LEFT - marks
        );
        assert!(
            hi <= hotbar::RIGHT + 1e-6,
            "something is drawn at {hi}, past the right end of the bar at {}",
            hotbar::RIGHT
        );
    }

    /// A phone's own aspect and interface size still put the stack on
    /// the screen.
    ///
    /// The HUD is authored as if the window were square and then grown
    /// about the bottom of the screen by whatever INTERFACE SIZE says,
    /// so a stack that is now as wide as the hotbar is a stack that
    /// grows with it: at 1.65 the bar reaches 0.77 either side of the
    /// middle, and a narrow window is where that lands off the edge.
    /// The device this is played on is 2712x1220 at 1.65.
    #[test]
    fn the_stack_stays_on_a_phone_screen_at_the_size_it_is_played_at() {
        let mut vertices = Vec::new();
        build_into(
            FontAtlas::for_test(),
            13.0,
            20.0,
            16.0,
            0.5,
            false,
            0.4,
            0.4,
            BodyGauges {
                temperature_c: 12.0,
                comfort: primitive_shared::body::Comfort::of(12.0),
                hydration: 0.4,
                fatigue: 0.0,
                injuries: primitive_shared::injury::Injuries::default(),
                wetness: 0.0,
                grime: 0.0,
                recovery: 1.0,
                diet_groups: 0,
                shelter: Default::default(),
                smoke: 0.0,
            },
            &Inventory::new(),
            None,
            &mut Attention::default(),
            std::time::Instant::now(),
            &mut vertices,
        );
        let (aspect, scale) = (2712.0_f32 / 1220.0, 1.65_f32);
        widgets::scale_about(&mut vertices, widgets::anchor::BOTTOM(aspect), scale);
        let (lo, hi) = horizontal_extent(&vertices);
        assert!(
            lo >= -aspect && hi <= aspect,
            "at size {scale} the stack runs {lo}..{hi} across a screen {aspect} wide",
        );
        // ...and upward, where a phone has far less to spare: six
        // strips grown by two thirds is a wall of bars, and the one
        // thing it must not do is reach the crosshair.
        let (_, top) = vertical_extent(&vertices);
        assert!(top < 0.0, "at size {scale} the stack reaches {top}");
    }

    #[test]
    fn no_meter_lands_on_the_hotbar_or_on_another_meter() {
        // **The bug this exists to stop coming back.** Every thin strip
        // used to spell out its own offset from the health gauge, and
        // the fourth one added put itself straight through the top row
        // of hotbar slots -- visible instantly in play and invisible to
        // every test there was, because no test knew where the strips
        // were supposed to be.
        //
        // Now they are placed by `band_below`/`band_above` and this
        // checks the two things that can go wrong with a band: it can
        // reach the hotbar, and it can land on its neighbour.
        let mut used: Vec<(f32, f32)> = Vec::new();
        for n in 0..BANDS_BELOW {
            used.push(band_below(n));
        }
        // Water, breath, temperature.
        for n in 0..BANDS_ABOVE {
            used.push(band_above(n));
        }
        for &(y0, y1) in &used {
            assert!(
                y0 > HOTBAR_TOP,
                "a meter at {y0}..{y1} is drawn over the hotbar (top {HOTBAR_TOP})"
            );
            assert!(y1 > y0, "a meter with no height");
        }
        // ...and the health gauge itself is nobody's band.
        let gauge = (BAR_Y, BAR_Y + BAR_HEIGHT);
        used.push(gauge);
        for (i, &a) in used.iter().enumerate() {
            for &b in used.iter().skip(i + 1) {
                assert!(
                    a.1 <= b.0 || b.1 <= a.0,
                    "two meters overlap: {a:?} and {b:?}"
                );
            }
        }
    }

    /// Which bands have a mark beside them, by the middle of the band.
    ///
    /// Found by looking in the column the marks are drawn in rather
    /// than by counting quads: a mark is several quads and the count
    /// depends on the picture, which is not what any of this is about.
    fn marked_bands(vertices: &[HotbarVertex]) -> Vec<f32> {
        let right = BAR_LEFT - ICON_GAP;
        let left = right - ICON_SIZE - ICON_SIZE / ICON_GRID as f32;
        // **Every vertex is snapped to the nearest band**, rather than
        // grouped around whichever one the loop happened to see first.
        //
        // The first version did the latter and was quietly fragile: a
        // mark is `ICON_SIZE` tall and the bands are only a hair
        // further apart than that, so the top row of one mark and the
        // bottom row of the mark above it are less than a band apart --
        // and which of them became "the centre" decided whether the two
        // were counted as one. It gave the right answer for six meters
        // and the wrong one for seven, which is the worst way for a
        // helper to be wrong: it reads as the feature being broken.
        let bands: Vec<f32> = (0..BANDS_BELOW)
            .map(band_below)
            .chain((0..BANDS_ABOVE).map(band_above))
            .chain(std::iter::once((BAR_Y, BAR_Y + BAR_HEIGHT)))
            .map(|(y0, y1)| (y0 + y1) * 0.5)
            .collect();
        let mut centres: Vec<f32> = Vec::new();
        for v in vertices {
            if v.position[0] < left - 1e-4 || v.position[0] > right + 1e-4 {
                continue;
            }
            let y = v.position[1];
            let Some(&band) = bands
                .iter()
                .min_by(|a, b| (*a - y).abs().partial_cmp(&(*b - y).abs()).unwrap())
            else {
                continue;
            };
            if !centres.iter().any(|c: &f32| (c - band).abs() < 1e-6) {
                centres.push(band);
            }
        }
        centres.sort_by(|a, b| a.partial_cmp(b).unwrap());
        centres
    }

    /// Every meter on screen carries its own mark, and a meter that is
    /// not on screen carries none.
    ///
    /// **The second half is the one worth a test.** Breath and
    /// temperature come and go, and a mark left behind for a strip that
    /// is not drawn would be a picture of nothing, floating in the gap
    /// where a bar used to be. That cannot happen while each mark is
    /// drawn by the function that draws its meter -- which is what this
    /// is really asserting, from the outside.
    #[test]
    fn a_meter_is_marked_exactly_when_it_is_on_screen() {
        // Cold, thirsty and *tired*: all seven meters have something to
        // say, which is what the first half of this test is about.
        // Tiredness has to be past `REST_SHOWS_AT` for the same reason
        // the body has to be cold -- a meter with nothing to say is not
        // drawn, and a fixture that forgot that would be asserting the
        // wrong number.
        let cold = BodyGauges {
            temperature_c: 12.0,
            comfort: primitive_shared::body::Comfort::of(12.0),
            hydration: 0.5,
            fatigue: 0.6,
            injuries: primitive_shared::injury::Injuries::default(),
            // The four the health page reads and the HUD does not.
            // `..Default::default()` would have been shorter and would
            // also have stopped these tests failing the next time a
            // gauge is added -- which is the one thing they are for.
            wetness: 0.0,
            grime: 0.0,
            recovery: 1.0,
            diet_groups: 0,
            shelter: Default::default(),
            smoke: 0.0,
        };
        let everything = stack_at(
            &mut Attention::default(),
            std::time::Instant::now(),
            13.0,
            0.5,
            0.4,
            0.5,
            cold,
        );
        let marks = marked_bands(&everything);
        assert_eq!(
            marks.len(),
            BANDS_ABOVE + BANDS_BELOW + 1,
            "not every meter on screen has a mark: {marks:?}",
        );

        // Air full, the body comfortable and the player rested: three
        // meters gone, and their three marks with them.
        let quiet = stack_at(
            &mut Attention::default(),
            std::time::Instant::now(),
            13.0,
            0.5,
            1.0,
            0.5,
            all_well_but_thirsty(),
        );
        let quiet_marks = marked_bands(&quiet);
        assert_eq!(
            quiet_marks.len(),
            BANDS_ABOVE + BANDS_BELOW + 1 - 3,
            "a mark was drawn beside a meter that is not there: {quiet_marks:?}",
        );
    }

    /// Comfortable, breathing, and short of water -- so the stack is up
    /// and two of its six meters are not.
    fn all_well_but_thirsty() -> BodyGauges {
        BodyGauges {
            hydration: 0.5,
            ..all_well()
        }
    }

    /// No two marks touch, and the lowest clears the hotbar.
    ///
    /// Asserted at build time as a relation between constants, and here
    /// against what is actually drawn -- which is the half that can go
    /// wrong on its own, because a mark is centred on its strip and the
    /// strips are placed by two different functions.
    #[test]
    fn the_marks_beside_the_meters_touch_neither_each_other_nor_the_hotbar() {
        let vertices = stack_at(
            &mut Attention::default(),
            std::time::Instant::now(),
            13.0,
            0.5,
            0.4,
            0.5,
            BodyGauges {
                temperature_c: 12.0,
                comfort: primitive_shared::body::Comfort::of(12.0),
                hydration: 0.5,
                fatigue: 0.0,
                injuries: primitive_shared::injury::Injuries::default(),
                wetness: 0.0,
                grime: 0.0,
                recovery: 1.0,
                diet_groups: 0,
                shelter: Default::default(),
                smoke: 0.0,
            },
        );
        let right = BAR_LEFT - ICON_GAP;
        let left = right - ICON_SIZE - ICON_SIZE / ICON_GRID as f32;
        let mut lowest = f32::MAX;
        for v in &vertices {
            if v.position[0] >= left - 1e-4 && v.position[0] <= right + 1e-4 {
                lowest = lowest.min(v.position[1]);
            }
        }
        assert!(
            lowest > HOTBAR_TOP,
            "the lowest mark reaches {lowest}, over a hotbar whose top is {HOTBAR_TOP}",
        );

        // Six marks at six band centres, each one no taller than the
        // pitch it is stacked at -- which is what stops it touching its
        // neighbour.
        let centres = marked_bands(&vertices);
        for pair in centres.windows(2) {
            assert!(
                pair[1] - pair[0] >= ICON_SIZE,
                "two marks at {} and {} are closer than a mark is tall",
                pair[0],
                pair[1],
            );
        }
    }

    /// Every mark is a picture rather than a blank or a full square.
    ///
    /// **A mark with no cells set draws nothing and a mark with all of
    /// them draws a box**, and both look exactly like a bug in the
    /// stack rather than like an icon. Cheap to state and it is the one
    /// thing about the artwork a test can actually judge.
    #[test]
    fn no_mark_is_empty_and_none_is_a_solid_block() {
        for (icon, msg) in GAUGE_LEGEND {
            let set: u32 = icon.0.iter().map(|row| row.count_ones()).sum();
            let cells = (ICON_GRID * ICON_GRID) as u32;
            assert!(
                set > 3 && set < cells - 3,
                "{msg:?} sets {set} of {cells} cells, which is a blank or a block",
            );
            for row in icon.0 {
                assert!(
                    row < (1 << ICON_GRID),
                    "{msg:?} has a row {row:#07b} wider than the grid",
                );
            }
        }
        // ...and no two of them are the same picture, which is the one
        // way a legend of six can be a legend of five.
        for (index, (a, _)) in GAUGE_LEGEND.iter().enumerate() {
            for (b, msg) in GAUGE_LEGEND.iter().skip(index + 1) {
                assert_ne!(a, b, "two meters share a mark ({msg:?})");
            }
        }
    }

    #[test]
    fn the_breath_meter_is_only_there_while_the_air_is_going() {
        // A gauge that is always on screen and always full is a gauge
        // nobody reads -- and this one is full for almost the whole
        // game.
        let inventory = Inventory::new();
        let hud = |breath| {
            build(
                FontAtlas::for_test(),
                20.0,
                20.0,
                20.0,
                1.0,
                false,
                breath,
                // how full: irrelevant to what this test asserts
                1.0,
                // ...and so are the two the world drives.
                BodyGauges::default(),
                &inventory,
                None,
            )
        };
        let dry = hud(1.0).len();
        assert!(hud(0.5).len() > dry, "no meter while the air is running out");
        assert_eq!(hud(1.0).len(), dry);
        // An empty one still draws its track: "no air left" has to look
        // different from "no meter".
        assert!(hud(0.0).len() > dry);
        // Nonsense from the wire reads as full rather than as drowning.
        assert_eq!(hud(f32::NAN).len(), dry);
    }

    #[test]
    fn a_notice_is_drawn_while_it_lasts_and_not_after() {
        let inventory = Inventory::new();
        let hud = |notice| {
            build(
                FontAtlas::for_test(),
                20.0,
                20.0,
                20.0,
                1.0,
                false,
                1.0,
                // how full: irrelevant to what this test asserts
                1.0,
                BodyGauges::default(),
                &inventory,
                notice,
            )
        };
        let quiet = hud(None);
        assert!(
            hud(Some(("you are not carrying that", 1.0))).len() > quiet.len(),
            "a refusal drew nothing"
        );
        assert_eq!(
            hud(Some(("faded away", 0.0))).len(),
            quiet.len(),
            "a spent notice was still drawn"
        );
        assert_eq!(hud(Some(("", 1.0))).len(), quiet.len(), "an empty notice drew a plate");
    }

    #[test]
    fn the_line_gauge_is_drawn_only_while_there_is_a_line_to_read() {
        let mut painter = Painter::new(FontAtlas::for_test());
        line_gauge(&mut painter, None, None);
        assert!(painter.vertices.is_empty(), "the gauge was drawn with no rod in anybody's hand");
        line_gauge(&mut painter, Some(0.5), None);
        let winding = painter.vertices.len();
        assert!(winding > 0, "a rod being wound back showed nothing");
        // A fish on the line reads as strain whatever the wind-up says, and
        // the two are different colours: one bar, two readings.
        let mut fighting = Painter::new(FontAtlas::for_test());
        line_gauge(&mut fighting, Some(0.5), Some(0.5));
        assert_eq!(fighting.vertices.len(), winding, "the two readings are not the same bar");
        let wind_colour = painter.vertices.last().expect("drawn").tint;
        let strain_colour = fighting.vertices.last().expect("drawn").tint;
        assert!(wind_colour != strain_colour, "the wind-up and the strain look the same");
    }

    #[test]
    fn a_notice_sits_clear_of_the_hotbar_and_the_crosshair() {
        // Over the bar it hides the thing the message is usually about;
        // over the middle of the screen it is in the way of aiming.
        let mut p = painter();
        notice(&mut p, "no room for that", 1.0);
        let (low, high) = vertical_extent(&p.vertices);
        assert!(low > BOTTOM + SLOT, "the notice overlaps the hotbar");
        assert!(high < 0.0, "the notice reaches the crosshair");
    }

    /// ...and clear of the gauges, which is the half nobody checked.
    ///
    /// **The notice was drawn across the temperature scale.** Its
    /// height was a chosen offset from the health gauge, fixed when
    /// there was one strip above it; there are three now, and the
    /// plate's lower edge had ended up below the top of them. Both
    /// bounds it did have were satisfied the whole time -- above the
    /// hotbar, below the crosshair -- which is the shape of a guard
    /// test that guards the wrong two things.
    ///
    /// Measured on what is drawn rather than on the offset, because the
    /// offset was the thing that was wrong. That `STACK_TOP` is where
    /// the gauges really end is `a_figure_stays_inside_the_gap_around_its_strip`'s
    /// to say.
    #[test]
    fn a_notice_is_not_printed_across_the_gauges() {
        let mut p = painter();
        notice(&mut p, "you are not carrying that", 1.0);
        let (plate, _) = vertical_extent(&p.vertices);
        assert!(
            plate > STACK_TOP,
            "the notice starts at {plate}, over gauges reaching {STACK_TOP}",
        );
    }

    /// **...and neither is the first two minutes' line**, which is the
    /// same mistake made again one row up.
    ///
    /// It was placed "just over the belt" -- `hotbar::TOP + 0.055` -- and
    /// the HUD snapshot showed it printed straight across the stamina
    /// strip and the breath gauge, because the strip nothing else uses is
    /// not over the belt. Measured on what is drawn, against the notice's
    /// plate as well as the gauges: the two can be up at once, and a
    /// refusal while a new player is being told to pick up a stone must
    /// not land on the same row.
    #[test]
    fn the_first_two_minutes_line_clears_both_the_gauges_and_the_notice() {
        let mut p = painter();
        first_step_line(&mut p, "найдите кремень и отбейте от него отщепы");
        let (low, high) = vertical_extent(&p.vertices);
        assert!(low > STACK_TOP, "the prompt starts at {low}, over gauges reaching {STACK_TOP}");
        assert!(
            low > NOTICE_Y + NOTICE_HALF_HEIGHT,
            "the prompt starts at {low}, on the notice's plate which reaches {}",
            NOTICE_Y + NOTICE_HALF_HEIGHT,
        );
        assert!(high < 0.0, "the prompt reaches the crosshair");
    }

    #[test]
    fn a_server_with_a_different_maximum_still_gets_a_sane_bar() {
        // `max` comes off the wire, so it is not this module's to trust.
        for max in [1.0, 6.0, 20.0, 100.0] {
            let mut p = painter();
            health_bar(&mut p, max, max, max);
            assert!(!p.vertices.is_empty(), "max {max} drew nothing");
            assert_eq!(
                lit_segments(&p.vertices, 1.0),
                SEGMENTS,
                "max {max} did not fill the gauge"
            );
        }
    }

    #[test]
    fn nonsense_health_does_not_panic_or_overflow_the_bar() {
        for (current, max, recent) in [
            (-5.0, 0.0, 0.0),
            (1e9, 20.0, 1e9),
            (20.0, -1.0, 20.0),
            (f32::NAN, 20.0, 20.0),
        ] {
            let mut p = painter();
            health_bar(&mut p, current, max, recent);
            let (lo, hi) = vertical_extent(&p.vertices);
            assert!(lo.is_finite() && hi.is_finite(), "({current}, {max}) broke the bar");
            let (left, right) = total_extent(&p.vertices);
            // The mark beside the gauge is the leftmost thing drawn --
            // see `the_gauges_are_laid_out_against_the_hotbar`.
            assert!(
                left >= BAR_LEFT - ICON_GAP - ICON_SIZE - 0.01 && right < 1.0,
                "({current}, {max}) drew from {left} to {right}"
            );
        }
    }
}


