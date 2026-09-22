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
    // Inside the grown block, with the words: `give_up_rect` is authored
    // at the size the words are, and the finger is taken back through the
    // same growth (`GiveUpButton::handle`'s caller).
    if widgets::touch_layout() {
        p.button(give_up_rect(ui_scale), language.text(Msg::DownedGiveUpButton), false, true);
    }
    *out = p.into_vertices();
    // The words grow with the interface size; the red is already at every
    // edge of every window, and scaling it would push it off them.
    widgets::scale_about(&mut out[text_from..], widgets::anchor::CENTRE(aspect), ui_scale);
}

/// **Where a phone's give-up button is**, in the authored space the words
/// are drawn in -- before `scale_about(CENTRE, ui_scale)` grows them.
///
/// Under the three lines, centred, above the crosshair. At least a finger
/// tall *after* the growth, which is why it takes the scale: at an
/// interface size of one, a button authored for one and a half would be
/// too small to hit on purpose.
pub fn give_up_rect(ui_scale: f32) -> Rect {
    let lines_end = FIRST_LINE_TOP - 3.0 * (widgets::cell_height(LINE_SCALE) + LINE_GAP);
    let height = (widgets::FINGER_SIDE / ui_scale.max(0.1)).max(0.10);
    let top = lines_end - LINE_GAP;
    Rect::new(-0.26, top - height, 0.26, top)
}

/// A finger on the downed overlay's give-up button.
///
/// ## Why a phone has one
///
/// Giving up was the respawn key and nothing else, and a phone has no
/// keys: a downed player on a phone could only wait out the whole clock,
/// face down, however hopeless it was. The keyboard's way out is a key;
/// the phone's is this button.
///
/// ## Why on the lift, and only for a finger that started on it
///
/// Every other button in the game acts on the tap. This one ends a life,
/// and it sits in the look area, where a thumb that lands to turn the
/// camera and drags away has pressed nothing -- so it gives up only when
/// the finger that *went down* on it also *comes up* on it. A drag that
/// began there is claimed and swallowed rather than turning the view,
/// because a button that turned the camera when pressed would be a
/// button that seemed not to be there.
#[derive(Debug, Default)]
pub struct GiveUpButton {
    finger: Option<crate::platform::TouchId>,
}

/// What [`GiveUpButton::handle`] made of a touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiveUpTap {
    /// Not ours: the thumb controls and the look area have it.
    Ignored,
    /// Ours, and nothing decided yet.
    Held,
    /// Lifted on the button: send the give-up.
    GiveUp,
}

impl GiveUpButton {
    /// `at` is the finger taken back into the words' authored space
    /// (`widgets::unscale_about` about the centre by `ui_scale`), the exact
    /// inverse of the `scale_about` the drawing does.
    pub fn handle(
        &mut self,
        id: crate::platform::TouchId,
        phase: crate::platform::TouchPhase,
        at: (f32, f32),
        downed: bool,
        ui_scale: f32,
    ) -> GiveUpTap {
        use crate::platform::TouchPhase;
        if !downed {
            self.finger = None;
            return GiveUpTap::Ignored;
        }
        let on = give_up_rect(ui_scale).contains(at.0, at.1);
        match phase {
            TouchPhase::Started if on && self.finger.is_none() => {
                self.finger = Some(id);
                GiveUpTap::Held
            }
            _ if self.finger != Some(id) => GiveUpTap::Ignored,
            TouchPhase::Started | TouchPhase::Moved => GiveUpTap::Held,
            TouchPhase::Ended => {
                self.finger = None;
                if on {
                    GiveUpTap::GiveUp
                } else {
                    GiveUpTap::Held
                }
            }
            TouchPhase::Cancelled => {
                self.finger = None;
                GiveUpTap::Held
            }
        }
    }
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
    fn on_a_phone_the_give_up_button_is_pressed_where_it_is_drawn() {
        use crate::platform::TouchPhase;
        let aspect = 20.0 / 9.0;
        for ui_scale in [1.0, 1.5] {
            let drawn_rect = widgets::as_a_phone(|| {
                let mut out = Vec::new();
                let down = Down::new(Cause::Wound).unwrap();
                build_into(FontAtlas::for_test(), Some(down), Language::Russian, aspect, ui_scale, std::time::Instant::now(), &mut out);
                // Where the drawing put the button's corners, found by
                // growing its authored rect the way the drawing does.
                let r = give_up_rect(ui_scale);
                let (x0, y0) = (r.x0 * ui_scale, r.y0 * ui_scale);
                let (x1, y1) = (r.x1 * ui_scale, r.y1 * ui_scale);
                let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
                assert!(
                    out.iter().any(|v| near(v.position[0], x0) && near(v.position[1], y0))
                        && out.iter().any(|v| near(v.position[0], x1) && near(v.position[1], y1)),
                    "no button drawn where give_up_rect says at scale {ui_scale}"
                );
                Rect::new(x0, y0, x1, y1)
            });
            assert!(
                drawn_rect.height() >= widgets::FINGER_SIDE - 1e-4,
                "the give-up button is {} tall on glass, under a finger",
                drawn_rect.height()
            );
            // A finger in the middle of the drawn button, taken back the
            // way the frame loop takes it back.
            let finger = (drawn_rect.centre_x(), drawn_rect.centre_y());
            let authored = widgets::unscale_about(finger, widgets::anchor::CENTRE(aspect), ui_scale);
            let mut button = GiveUpButton::default();
            assert_eq!(button.handle(7, TouchPhase::Started, authored, true, ui_scale), GiveUpTap::Held);
            assert_eq!(button.handle(7, TouchPhase::Ended, authored, true, ui_scale), GiveUpTap::GiveUp);

            // Just outside it, nothing: the look area keeps the finger.
            let outside = widgets::unscale_about(
                (drawn_rect.x1 + 0.02, drawn_rect.centre_y()),
                widgets::anchor::CENTRE(aspect),
                ui_scale,
            );
            assert_eq!(button.handle(8, TouchPhase::Started, outside, true, ui_scale), GiveUpTap::Ignored);
        }
    }

    #[test]
    fn a_thumb_that_lands_on_give_up_and_drags_away_has_not_given_up() {
        use crate::platform::TouchPhase;
        let on = {
            let r = give_up_rect(1.5);
            (r.centre_x(), r.centre_y())
        };
        let mut button = GiveUpButton::default();
        assert_eq!(button.handle(1, TouchPhase::Started, on, true, 1.5), GiveUpTap::Held);
        assert_eq!(button.handle(1, TouchPhase::Moved, (1.2, -0.5), true, 1.5), GiveUpTap::Held);
        assert_eq!(button.handle(1, TouchPhase::Ended, (1.2, -0.5), true, 1.5), GiveUpTap::Held);
        // ...and a player on their feet has no button at all.
        assert_eq!(button.handle(2, TouchPhase::Started, on, false, 1.5), GiveUpTap::Ignored);
    }

    #[test]
    fn a_desktop_draws_no_give_up_button_because_it_has_the_key() {
        let down = Down::new(Cause::Wound).unwrap();
        let count = |touch| {
            widgets::with_touch(touch, || {
                let mut out = Vec::new();
                build_into(FontAtlas::for_test(), Some(down), Language::English, 16.0 / 9.0, 1.0, std::time::Instant::now(), &mut out);
                out.len()
            })
        };
        assert!(count(true) > count(false), "the phone's button was not drawn");
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
