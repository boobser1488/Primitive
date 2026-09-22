//! The dark a sleeper's screen goes, and the lines said on it.
//!
//! **Under the gauges, over the world.** Everything else in the overlay --
//! the hotbar, the gauges, a notice, and on a phone the thumb controls -- is
//! appended after this and so is drawn on top of the black. On a desktop
//! that is the rest bar draining, which is what a sleeper is there to
//! watch. On a phone it is the way out of bed: a phone has no key to press,
//! and a black that covered the stick and JUMP would be a screen with
//! nothing on it that gets the player up.
//!
//! The state -- how dark, and why -- is `logic::posture::Sleep`; this only
//! draws it.

use crate::engine::texture::FontAtlas;
use crate::logic::posture::Sleep;
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter};

/// How big the lines on the black are: the notice's size, because they are
/// read at the same distance for the same reason.
const SCALE: f32 = 0.86;
/// The top of the first line: above the crosshair, well below the top edge.
const FIRST_LINE_TOP: f32 = 0.24;
/// Air between two lines.
const LINE_GAP: f32 = 0.03;

/// The dark and its lines, appended to the overlay. Nothing at all while
/// the screen is clear.
pub fn build_into(
    font: FontAtlas,
    sleep: &Sleep,
    language: Language,
    aspect: f32,
    ui_scale: f32,
    out: &mut Vec<HotbarVertex>,
) {
    let dark = sleep.darkness();
    if dark <= 0.0 {
        return;
    }
    let mut p = Painter::onto(font, std::mem::take(out));
    p.scrim([0.0, 0.0, 0.0, dark]);
    let text_from = p.vertices.len();
    for (index, (msg, colour)) in lines(sleep).into_iter().enumerate() {
        let text = language.text(msg);
        let width = widgets::ink_width(text, SCALE);
        let top = FIRST_LINE_TOP - index as f32 * (widgets::cell_height(SCALE) + LINE_GAP);
        p.text(text, -width / 2.0, top, SCALE, colour);
    }
    *out = p.into_vertices();
    // The lines grow with the interface size; the black does not need to,
    // it is already past every edge of every window.
    widgets::scale_about(&mut out[text_from..], widgets::anchor::CENTRE(aspect), ui_scale);
}

/// What the black says, and in what colour: nothing while it is still
/// falling or lifting, how to get up once it is whole, and -- once the night
/// should have passed and has not -- why.
///
/// "Walk or jump" and not "press any key", which the screen said for as long
/// as sleep existed while no key but those did anything. Walking is what
/// both a W key and a thumb on the stick are.
fn lines(sleep: &Sleep) -> Vec<(Msg, [f32; 4])> {
    if !sleep.is_asleep() || sleep.darkness() < 1.0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    if sleep.waiting() {
        lines.push((Msg::SleepWaiting, widgets::TEXT));
    }
    lines.push((Msg::SleepGetUp, widgets::TEXT_DIM));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::body::{FALLING_ASLEEP_SECONDS, NIGHT_PASSES_AFTER_SECONDS};

    fn drawn(sleep: &Sleep) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        build_into(FontAtlas::for_test(), sleep, Language::English, 16.0 / 9.0, 1.5, &mut out);
        out
    }

    #[test]
    fn an_awake_screen_is_not_dimmed_and_a_black_one_says_how_to_get_up() {
        assert!(drawn(&Sleep::default()).is_empty(), "an awake player's world was dimmed");

        let mut sleep = Sleep::default();
        sleep.set_asleep(true);
        sleep.tick(FALLING_ASLEEP_SECONDS * 0.5);
        let falling = drawn(&sleep);
        assert_eq!(falling.len(), 6, "a screen still going dark is only the dark");
        let alpha = falling[0].tint[3];
        assert!(alpha > 0.0 && alpha < 1.0, "half way down the dark is at {alpha}");

        sleep.tick(FALLING_ASLEEP_SECONDS);
        let black = drawn(&sleep);
        assert_eq!(black[0].tint[3], 1.0);
        assert!(black.len() > 6, "a black screen said nothing about how to get up");
        // Wider than the widest window, whatever the interface size.
        for v in &black[..6] {
            assert!(v.position[0].abs() >= 4.0 && v.position[1].abs() >= 1.0, "the dark stops at {:?}", v.position);
        }

        // ...and a second line once the night is plainly waiting on somebody.
        sleep.tick(NIGHT_PASSES_AFTER_SECONDS * 2.0);
        assert!(sleep.waiting());
        assert!(drawn(&sleep).len() > black.len(), "a sleeper left waiting was never told why");
    }
}
