//! The red a downed player's screen goes, and what is said on it.
//!
//! **Three things, in the order a player needs them**: that something has
//! changed (the eye drops to the ground and the edges go red), how long is
//! left (the number, and a pulse that quickens as it runs down), and what to
//! do about it (what put you here, and what gets you up). The last is the
//! one that matters and the one this screen exists for -- a clock with no
//! instruction under it is a countdown to a death screen, and the point of
//! being down is that it asks a question. See `primitive_shared::downed`.
//!
//! **Under the gauges, over the world**, for `ui::sleep`'s reason: the
//! health bar, the belt with the bandage in it and on a phone the thumb
//! controls are what a downed player is reaching for, so they are drawn on
//! top of the red and not behind it.
//!
//! **The centre stays clear.** The red is a band round the edges and the
//! dim over the middle is light: a downed player still has to see where
//! they are crawling to, and a screen that went dark would make the crawl
//! a guess.

use crate::engine::texture::FontAtlas;
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{by_input, Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};
use primitive_shared::downed::{Cause, Down, Rescue};

/// How many rings the red is drawn in, and how deep they reach in from each
/// edge, in interface units. Rings rather than one soft quad because the
/// interface pipeline has one colour a quad; eight steps is past where the
/// eye sees the bands.
const RINGS: usize = 8;
const RING_DEPTH: f32 = 0.045;
/// The red at the edge, at its strongest.
const EDGE_RED: [f32; 4] = [0.55, 0.02, 0.02, 0.85];
/// The dim over everything: faint with the whole clock left, heavier as it
/// runs out -- the world going away is the clock the eye reads first.
const DIM_FRESH: f32 = 0.12;
const DIM_SPENT: f32 = 0.45;
/// A heartbeat, from a resting one to a racing one as the clock runs out.
const PULSE_HZ_FRESH: f32 = 1.0;
const PULSE_HZ_SPENT: f32 = 2.4;

const TITLE_SCALE: f32 = 1.25;
const CLOCK_SCALE: f32 = 2.0;
const LINE_SCALE: f32 = 0.86;
const CLOCK_TOP: f32 = 0.62;
const TITLE_TOP: f32 = 0.46;
const FIRST_LINE_TOP: f32 = 0.35;
const LINE_GAP: f32 = 0.03;

/// The red and its lines, appended to the overlay. Nothing at all while the
/// body is on its feet.
pub fn build_into(
    font: FontAtlas,
    down: Option<Down>,
    language: Language,
    aspect: f32,
    ui_scale: f32,
    now: std::time::Instant,
    out: &mut Vec<HotbarVertex>,
) {
    let Some(down) = down else {
        return;
    };
    let spent = 1.0 - down.fraction_left();
    let mut p = Painter::onto(font, std::mem::take(out));
    p.scrim([0.08, 0.0, 0.0, DIM_FRESH + (DIM_SPENT - DIM_FRESH) * spent]);

    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let since = now.saturating_duration_since(*EPOCH.get_or_init(|| now)).as_secs_f32();
    let hz = PULSE_HZ_FRESH + (PULSE_HZ_SPENT - PULSE_HZ_FRESH) * spent;
    // A beat is a quick swell and a slow fall, not a sine: `powf` keeps the
    // red near its floor for most of the cycle and lets it flare once.
    let beat = (0.5 + 0.5 * (since * hz * std::f32::consts::TAU).cos()).powf(3.0);
    let strength = 0.55 + 0.45 * beat;
    for (x0, y0, x1, y1, alpha) in rings(aspect) {
        p.quad(Rect::new(x0, y0, x1, y1), [EDGE_RED[0], EDGE_RED[1], EDGE_RED[2], EDGE_RED[3] * alpha * strength]);
    }

    let text_from = p.vertices.len();
    let seconds = down.left.max(0.0).ceil() as i32;
    let clock = seconds.to_string();
    let centred = |p: &mut Painter, text: &str, top: f32, scale: f32, colour: [f32; 4]| {
        let width = widgets::ink_width(text, scale);
        p.text(text, -width / 2.0, top, scale, colour);
    };
    centred(&mut p, &clock, CLOCK_TOP, CLOCK_SCALE, [1.0, 0.35, 0.30, 1.0]);
    centred(&mut p, language.text(Msg::DownedTitle), TITLE_TOP, TITLE_SCALE, widgets::TEXT);
    for (index, (msg, colour)) in lines(down).into_iter().enumerate() {
        let top = FIRST_LINE_TOP - index as f32 * (widgets::cell_height(LINE_SCALE) + LINE_GAP);
        centred(&mut p, language.text(msg), top, LINE_SCALE, colour);
    }
    *out = p.into_vertices();
    // The words grow with the interface size; the red is already at every
    // edge of every window, and scaling it would push it off them.
    widgets::scale_about(&mut out[text_from..], widgets::anchor::CENTRE(aspect), ui_scale);
}

/// The rings of red, outermost first: `(x0, y0, x1, y1, alpha)`, four
/// quads a ring. The outermost reaches past the window so no sliver of
/// world shows at an edge the aspect rounded.
fn rings(aspect: f32) -> Vec<(f32, f32, f32, f32, f32)> {
    let mut quads = Vec::with_capacity(RINGS * 4);
    for ring in 0..RINGS {
        let inner = (ring + 1) as f32 * RING_DEPTH;
        let outer = if ring == 0 { -1.0 } else { ring as f32 * RING_DEPTH };
        let fade = 1.0 - ring as f32 / RINGS as f32;
        let alpha = fade * fade;
        let (ax, ay) = (aspect - outer, 1.0 - outer);
        let (bx, by) = (aspect - inner, 1.0 - inner);
        // Top and bottom run the whole width of the ring; the sides fill in
        // between them, so no corner is drawn twice and darker than the rest.
        quads.push((-ax, by, ax, ay, alpha));
        quads.push((-ax, -ay, ax, -by, alpha));
        quads.push((-ax, -by, -bx, by, alpha));
        quads.push((bx, -by, ax, by, alpha));
    }
    quads
}

/// What the red says under the title: what put you here, what gets you up,
/// and how to let go.
fn lines(down: Down) -> Vec<(Msg, [f32; 4])> {
    let cause = match down.cause {
        Cause::Wound => Msg::DownedWound,
        Cause::Bleeding => Msg::DownedBleeding,
        Cause::Fall => Msg::DownedFall,
        Cause::Burn => Msg::DownedBurn,
        Cause::Cold => Msg::DownedCold,
        Cause::Heat => Msg::DownedHeat,
        Cause::Hunger => Msg::DownedHunger,
        Cause::Thirst => Msg::DownedThirst,
        Cause::Sickness => Msg::DownedSickness,
        Cause::Smoke => Msg::DownedSmoke,
        // Never down for these (`Cause::profile`), and said as a wound if a
        // server ever sends one: the rescue line under it is still right.
        Cause::Drowned | Cause::Crushed => Msg::DownedWound,
    };
    let saves = match down.rescue() {
        Rescue::Dressing => Msg::SavedByDressing,
        Rescue::Splint => Msg::SavedBySplint,
        Rescue::Warmth => Msg::SavedByWarmth,
        Rescue::Cooling => Msg::SavedByCooling,
        Rescue::Food => Msg::SavedByFood,
        Rescue::Water => Msg::SavedByWater,
        Rescue::Air => Msg::SavedByAir,
    };
    vec![
        (cause, widgets::TEXT_DIM),
        (saves, widgets::TEXT),
        // **No give-up on a phone.** The key is the respawn key, which a
        // phone has not got until the death screen gives it a button; what a
        // phone is told instead is the one thing still true there.
        (by_input(Msg::DownedGiveUp, Msg::DownedGiveUpTouch), widgets::TEXT_DIM),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(down: Option<Down>) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        build_into(FontAtlas::for_test(), down, Language::English, 16.0 / 9.0, 1.5, std::time::Instant::now(), &mut out);
        out
    }

    #[test]
    fn a_player_on_their_feet_sees_no_red_and_one_on_the_ground_is_told_what_gets_them_up() {
        assert!(drawn(None).is_empty(), "a standing player's screen went red");
        let down = Down::new(Cause::Hunger).unwrap();
        let red = drawn(Some(down));
        let quads = 1 + RINGS * 4;
        assert!(red.len() > quads * 6, "the red said nothing: {} vertices", red.len());
        let said: Vec<Msg> = lines(down).into_iter().map(|(msg, _)| msg).collect();
        assert!(said.contains(&Msg::DownedHunger) && said.contains(&Msg::SavedByFood), "{said:?}");
    }

    #[test]
    fn every_cause_that_downs_a_body_names_itself_and_its_rescue() {
        for cause in Cause::ALL {
            let Some(down) = Down::new(cause) else { continue };
            let said = lines(down);
            assert_eq!(said.len(), 3, "{cause:?}");
            assert_ne!(said[0].0, said[1].0);
        }
    }

    #[test]
    fn the_red_stays_at_the_edges_and_leaves_the_crosshair_clear() {
        let aspect = 16.0 / 9.0;
        let reach = RINGS as f32 * RING_DEPTH;
        for (x0, y0, x1, y1, _) in rings(aspect) {
            // Every quad is within `reach` of one edge or another.
            let near_edge = y0 >= 1.0 - reach - 1e-4
                || y1 <= -1.0 + reach + 1e-4
                || x1 <= -aspect + reach + 1e-4
                || x0 >= aspect - reach - 1e-4;
            assert!(near_edge, "a band of red reached ({x0}, {y0})..({x1}, {y1})");
        }
        assert!(reach < 0.5, "the red reaches half way to the crosshair");
    }

    #[test]
    fn the_world_dims_further_as_the_clock_runs_down() {
        let mut down = Down::new(Cause::Wound).unwrap();
        let fresh = drawn(Some(down))[0].tint[3];
        down.tick(down.of * 0.9);
        let spent = drawn(Some(down))[0].tint[3];
        assert!(spent > fresh, "the dim was {fresh} fresh and {spent} nearly spent");
    }
}
