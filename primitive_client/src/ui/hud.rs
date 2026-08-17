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
// hearts do not. Its bar is a single notched gauge sitting to the left
// of the hotbar with the exact figure written beside it, so it answers
// two different questions at once: the fill answers "how bad is it"
// without being read, and the number answers "how much exactly" when
// that matters. Neither requires counting icons.
//
// The notches are the part worth copying. An unbroken strip is hard to
// judge -- half and two thirds look much the same at this size -- and
// discrete segments turn it back into something countable at a glance
// without spending twenty icons' worth of screen.

/// Segments the gauge is divided into.
const SEGMENTS: usize = 10;
/// Width of the whole gauge.
const BAR_WIDTH: f32 = 0.46;
const BAR_HEIGHT: f32 = 0.034;
/// Gap between segments, as a fraction of one segment's pitch.
const SEGMENT_GAP: f32 = 0.16;
/// Left edge of the gauge. Offset from centre so the numeric readout
/// has somewhere to sit beside the bar rather than on top of it: a
/// number over a bar is unreadable at exactly the moment it matters,
/// which is when the bar is nearly empty and mostly dark.
const BAR_LEFT: f32 = -0.60;
/// Clear of the hotbar's frames and its own backdrop.
const BAR_Y: f32 = BOTTOM + SLOT + 0.052;

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
const HEALTH_TEXT_SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.85];

// ---- stamina ----
//
// A plain strip under the health gauge rather than a second notched
// bar. Stamina is continuous and nobody needs to read an exact figure
// off it -- what matters is whether there is any left, and the eye gets
// that from a length. Making it look different from health is also the
// point: two identical gauges side by side get confused for each other.

const STAMINA_HEIGHT: f32 = 0.014;
const STAMINA_GAP: f32 = 0.010;
const STAMINA_TRACK: [f32; 4] = [0.05, 0.05, 0.06, 0.85];
const STAMINA_FILL: [f32; 4] = [0.40, 0.72, 0.92, 1.0];
/// Spent, and locked out until enough comes back. Red, because the
/// sprint key not working needs a visible reason.
const STAMINA_SPENT: [f32; 4] = [0.85, 0.35, 0.25, 1.0];

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
    painter.border(track, 0.0035, BAR_EDGE);

    // How much health each segment stands for, and how full each one is.
    let per_segment = max / SEGMENTS as f32;
    let pitch = BAR_WIDTH / SEGMENTS as f32;
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

    // The exact figure, beside the gauge rather than inside it: over the
    // segments it would have to fight whatever colour they are.
    let label = format!("{}/{}", current.ceil() as i32, max.round() as i32);
    let baseline = track.y1 - (BAR_HEIGHT - widgets::cell_height(HEALTH_TEXT_SCALE)) / 2.0;
    let left = track.x1 + 0.020;
    let shadow = widgets::PIXEL * HEALTH_TEXT_SCALE;
    painter.text(
        &label,
        left + shadow,
        baseline - shadow,
        HEALTH_TEXT_SCALE,
        HEALTH_TEXT_SHADOW,
    );
    painter.text(&label, left, baseline, HEALTH_TEXT_SCALE, HEALTH_TEXT);
}

/// Draws the stamina strip, directly under the health gauge.
pub fn stamina_bar(painter: &mut Painter, fraction: f32, exhausted: bool) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let top = BAR_Y - STAMINA_GAP;
    let track = Rect::new(BAR_LEFT, top - STAMINA_HEIGHT, BAR_LEFT + BAR_WIDTH, top);
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
    painter.border(track, 0.002, BAR_EDGE);
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
    let bottom = BAR_Y + BAR_HEIGHT + STAMINA_GAP;
    let track = Rect::new(BAR_LEFT, bottom, BAR_LEFT + BAR_WIDTH, bottom + STAMINA_HEIGHT);
    painter.quad(track, STAMINA_TRACK);
    if fraction > 0.0 {
        painter.quad(
            Rect::new(track.x0, track.y0, track.x0 + BAR_WIDTH * fraction, track.y1),
            BREATH_FILL,
        );
    }
    painter.border(track, 0.002, BAR_EDGE);
}

/// Writes the stack size into each occupied hotbar slot.
///
/// Only the count: the icon and the frame come from `hotbar`, which
/// reads the same inventory. An empty slot gets nothing at all -- its
/// frame is already drawn and is the whole of what "empty" looks like.
pub fn stack_counts(painter: &mut Painter, inventory: &Inventory) {
    let count = crate::ui::hotbar::MAX_SLOTS;
    for index in 0..count {
        let held = inventory.count_in(index);
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
    inventory: &Inventory,
    // What the server last refused, and how visible it still is.
    notice: Option<(&str, f32)>,
) -> Vec<HotbarVertex> {
    let mut out = Vec::new();
    build_into(
        font,
        health,
        max_health,
        recent_health,
        stamina,
        exhausted,
        breath,
        inventory,
        notice,
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
    inventory: &Inventory,
    notice: Option<(&str, f32)>,
    out: &mut Vec<HotbarVertex>,
) {
    let mut painter = Painter::onto(font, std::mem::take(out));
    stack_counts(&mut painter, inventory);
    health_bar(&mut painter, health, max_health, recent_health);
    stamina_bar(&mut painter, stamina, exhausted);
    breath_bar(&mut painter, breath);
    if let Some((text, fade)) = notice {
        self::notice(&mut painter, text, fade);
    }
    *out = painter.into_vertices();
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
/// Above the gauges, below the middle of the screen: in view without
/// sitting over the crosshair.
const NOTICE_Y: f32 = BAR_Y + 0.13;
const NOTICE_BG: [f32; 4] = [0.10, 0.04, 0.05, 0.88];
const NOTICE_EDGE: [f32; 4] = [0.85, 0.35, 0.30, 0.95];

fn notice(painter: &mut Painter, text: &str, fade: f32) {
    let fade = fade.clamp(0.0, 1.0);
    if fade <= 0.0 || text.is_empty() {
        return;
    }
    let dim = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * fade];

    let width = widgets::ink_width(text, NOTICE_SCALE);
    let rect = Rect::centred(
        0.0,
        NOTICE_Y,
        width + 0.048,
        widgets::cell_height(NOTICE_SCALE) + 0.026,
    );
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

    fn painter() -> Painter {
        Painter::new(FontAtlas::for_test())
    }

    /// The y extent of everything drawn, so tests can check the HUD
    /// stays where it belongs.
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
        let full_segment = BAR_WIDTH / SEGMENTS as f32 * (1.0 - SEGMENT_GAP);
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

    #[test]
    fn a_ghost_below_current_health_is_ignored() {
        // `recent` lags downward only. Healing must not draw a strip
        // hanging off the end of the bar.
        let mut p = painter();
        health_bar(&mut p, 18.0, 20.0, 3.0);
        let (_, right) = p.vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[0]), hi.max(v.position[0]))
        });
        assert!(right <= BAR_WIDTH / 2.0 + 0.01, "something ran past the track");
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
    fn a_notice_sits_clear_of_the_hotbar_and_the_crosshair() {
        // Over the bar it hides the thing the message is usually about;
        // over the middle of the screen it is in the way of aiming.
        let mut p = painter();
        notice(&mut p, "no room for that", 1.0);
        let (low, high) = vertical_extent(&p.vertices);
        assert!(low > BOTTOM + SLOT, "the notice overlaps the hotbar");
        assert!(high < 0.0, "the notice reaches the crosshair");
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
            assert!(
                left >= BAR_LEFT - 0.01 && right < 1.0,
                "({current}, {max}) drew from {left} to {right}"
            );
        }
    }
}
