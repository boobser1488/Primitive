//! The screen you get for dying.
//!
//! ## Why it is a screen and not a caption
//!
//! What this replaces was a red wash, a line of text and "press R to
//! respawn". It said everything that needed saying and it was still the
//! worst moment in the game to look at, for two reasons that are
//! separate:
//!
//! * **A death is the one event the player did not choose.** Everything
//!   else on screen happens because they pressed something. This
//!   arrives, so it has to *land* -- a beat of its own, rather than a
//!   label appearing over a world that is still going on behind it as
//!   though nothing happened.
//! * **"Press R" is a legend for a control that does not exist.** Every
//!   other screen in this game is clickable and every one of them has a
//!   key; only this one made the player read an instruction and
//!   remember a letter. It now has buttons, and R still works.
//!
//! ## What it does with the moment
//!
//! Nothing that takes time away from the player. The fade is under a
//! second and the buttons are live from the first frame -- a screen that
//! makes you wait for an animation before you are allowed to carry on is
//! a screen you learn to resent. What the fade buys is that the world
//! *recedes*: the wash comes in, the panel drops in over it, and the
//! cause of death arrives last, which is the order the player wants to
//! read them in.
//!
//! ## Where the state lives
//!
//! Here, including the cause of death. It used to be an
//! `Option<String>` in `main`, read by six different pieces of the frame
//! loop to decide whether the player was dead -- so "is the death screen
//! up" and "is the player dead" were the same variable by coincidence
//! rather than by construction. Now there is one thing to ask.

use crate::engine::texture::FontAtlas;
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// What the player asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Back into the world at the spawn point.
    Respawn,
    /// Out to the main menu, leaving the world behind.
    LeaveWorld,
}

/// How long the wash takes to come in.
const FADE_SECONDS: f32 = 0.55;
/// ...and the panel, which follows it rather than arriving with it.
const PANEL_DELAY: f32 = 0.12;
const PANEL_SECONDS: f32 = 0.35;
/// When the cause of death appears, which is last.
const CAUSE_DELAY: f32 = 0.45;

/// How dark the world behind gets, and how red.
const WASH: [f32; 4] = [0.20, 0.015, 0.02, 0.78];
/// A second, harder wash right behind the panel, so the text is never
/// read against whatever the world happened to be.
const PANEL_FILL: [f32; 4] = [0.09, 0.05, 0.06, 0.96];
const PANEL_EDGE: [f32; 4] = [0.52, 0.16, 0.16, 1.0];
/// The bar of colour across the top of the panel: the one thing on the
/// screen that is purely decorative, and the thing that makes it read as
/// a plate rather than as a box.
const ACCENT: [f32; 4] = [0.78, 0.18, 0.16, 1.0];

const TITLE: [f32; 4] = [0.94, 0.28, 0.24, 1.0];
const TITLE_SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.75];
const CAUSE: [f32; 4] = [0.86, 0.80, 0.78, 1.0];

const BUTTON: [f32; 4] = [0.17, 0.11, 0.12, 1.0];
const BUTTON_HOVER: [f32; 4] = [0.34, 0.16, 0.16, 1.0];
const BUTTON_EDGE: [f32; 4] = [0.46, 0.24, 0.24, 1.0];

const PANEL_WIDTH: f32 = 1.24;
const PANEL_HEIGHT: f32 = 0.86;
const BUTTON_WIDTH: f32 = 0.86;
const BUTTON_HEIGHT: f32 = 0.115;

/// The death screen, and the fact of being dead.
#[derive(Default)]
pub struct DeathScreen {
    /// What killed the player. `None` means they are alive, and is the
    /// whole of what "alive" means to the rest of the client.
    cause: Option<String>,
    /// Seconds since it opened, which drives the fade and nothing else.
    age: f32,
    cursor: Option<(f32, f32)>,
    /// Which button the keyboard has, if the keyboard has been used.
    /// `None` leaves the highlight to the mouse.
    focus: Option<usize>,
}

impl DeathScreen {
    pub fn new() -> Self {
        Self::default()
    }

    /// The player died. `cause` is the server's words for it -- the
    /// client cannot tell a fall from a fight.
    pub fn open(&mut self, cause: String) {
        self.cause = Some(cause);
        self.age = 0.0;
        self.cursor = None;
        self.focus = None;
    }

    /// They are back in the world.
    pub fn close(&mut self) {
        self.cause = None;
        self.age = 0.0;
    }

    pub fn is_open(&self) -> bool {
        self.cause.is_some()
    }

    /// What killed them, for anything that wants to say so.
    ///
    /// Nothing outside the tests asks yet -- the screen draws it itself,
    /// and the server has already said it in chat -- but "what killed
    /// this player" is the one piece of state here that is about the
    /// world rather than about the drawing, so it is worth being able to
    /// ask for without reaching into the field.
    #[allow(dead_code)]
    pub fn cause(&self) -> Option<&str> {
        self.cause.as_deref()
    }

    pub fn tick(&mut self, dt: f32) {
        if self.is_open() {
            // Clamped, so a long stall on the frame the player died does
            // not skip the whole animation.
            self.age += dt.clamp(0.0, 0.1);
        }
    }

    pub fn set_cursor(&mut self, at: Option<(f32, f32)>) {
        self.cursor = at;
        if at.is_some() {
            // The mouse takes the highlight back from the keyboard, or
            // two things would claim it at once.
            self.focus = None;
        }
    }

    /// Moves the keyboard highlight. Wraps, so Up on the first entry
    /// lands on the last.
    pub fn move_focus(&mut self, delta: i32) {
        let count = CHOICES.len() as i32;
        self.focus = Some(match self.focus {
            Some(current) => (((current as i32 + delta) % count + count) % count) as usize,
            None if delta < 0 => (count - 1) as usize,
            None => 0,
        });
        self.cursor = None;
    }

    /// What the highlighted button would do, for Enter.
    pub fn focused(&self) -> Option<Choice> {
        CHOICES.get(self.focus?).map(|&(_, choice)| choice)
    }

    /// A click at wherever the cursor last was.
    pub fn click(&self) -> Option<Choice> {
        let cursor = self.cursor?;
        self.hit(cursor)
    }

    fn hit(&self, (x, y): (f32, f32)) -> Option<Choice> {
        CHOICES
            .iter()
            .enumerate()
            .find(|(index, _)| button_rect(*index).contains(x, y))
            .map(|(_, &(_, choice))| choice)
    }

    /// Eased 0..1 for a stage of the opening.
    fn phase(&self, delay: f32, length: f32) -> f32 {
        let t = ((self.age - delay) / length).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Whether the opening animation is still moving.
    ///
    /// While it is, the screen redraws differently every frame with no
    /// event behind it, so the caller has to keep rebuilding on time
    /// alone; once every phase has settled the screen is static and a
    /// key comparison is enough. The margin covers the last phase
    /// reaching exactly 1.0 -- ending the forced rebuilds a frame early
    /// would freeze the cause of death at almost-opaque.
    pub fn is_animating(&self) -> bool {
        const SETTLED: f32 = CAUSE_DELAY + PANEL_SECONDS + 0.25;
        self.is_open() && self.age < SETTLED
    }

    /// A fingerprint of what `build` would draw, cheap enough to take
    /// every frame. The animation clock is deliberately absent -- it
    /// moves every frame, and `is_animating` is what covers it.
    pub fn ui_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.cause.hash(&mut h);
        self.focus.hash(&mut h);
        // The cursor matters only through which button it lights up.
        // Hashing the raw position would rebuild the whole interface for
        // every pixel the mouse moves over a screen it changes nothing on.
        self.cursor
            .map(|(x, y)| {
                CHOICES
                    .iter()
                    .enumerate()
                    .find(|(index, _)| button_rect(*index).contains(x, y))
                    .map(|(index, _)| index)
            })
            .hash(&mut h);
        h.finish()
    }

    /// The `Vec`-returning form, kept for the tests: they assert on
    /// one widget's output in isolation, which is exactly what appending
    /// into a shared list is designed not to produce.
    #[cfg(test)]
    pub fn build(&self, font: FontAtlas, language: Language) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        self.build_into(font, language, &mut out);
        out
    }

    /// The same screen, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(&self, font: FontAtlas, language: Language, out: &mut Vec<HotbarVertex>) {
        if !self.is_open() {
            return;
        }
        let mut p = Painter::onto(font, std::mem::take(out));
        self.paint(&mut p, language);
        *out = p.into_vertices();
    }

    fn paint(&self, p: &mut Painter, language: Language) {
        let fade = self.phase(0.0, FADE_SECONDS);
        let alpha = |c: [f32; 4], t: f32| [c[0], c[1], c[2], c[3] * t];

        p.scrim(alpha(WASH, fade));

        let settle = self.phase(PANEL_DELAY, PANEL_SECONDS);
        if settle <= 0.0 {
            return;
        }
        // The panel drops the last fraction of its own height into
        // place. Small on purpose: enough to read as arriving, not
        // enough to be an animation anyone waits through.
        let drop = (1.0 - settle) * 0.10;
        let panel = Rect::centred(0.0, -drop, PANEL_WIDTH, PANEL_HEIGHT);
        p.quad(panel, alpha(PANEL_FILL, settle));
        p.border(panel, 0.005, alpha(PANEL_EDGE, settle));
        // The bar across the top, inset so it reads as part of the plate.
        p.quad(
            Rect::new(panel.x0 + 0.02, panel.y1 - 0.022, panel.x1 - 0.02, panel.y1 - 0.008),
            alpha(ACCENT, settle),
        );

        // The title, with a hard shadow under it: it is drawn over a
        // panel over a world, and at this size a drop shadow is the
        // difference between a heading and a smear.
        let title_top = panel.y1 - 0.10;
        let shadow_offset = widgets::PIXEL * 2.0;
        let title = language.text(Msg::YouDied);
        p.text_centred(
            title,
            shadow_offset,
            title_top - shadow_offset,
            2.4,
            alpha(TITLE_SHADOW, settle),
        );
        p.text_centred(title, 0.0, title_top, 2.4, alpha(TITLE, settle));

        // What happened, last and quietly.
        let told = self.phase(CAUSE_DELAY, PANEL_SECONDS);
        if told > 0.0 {
            let cause = self.cause.as_deref().unwrap_or("");
            let text = widgets::fit(cause, 1.0, PANEL_WIDTH - 0.14);
            p.text_centred(&text, 0.0, title_top - 0.13, 1.0, alpha(CAUSE, told));
        }

        for (index, (label, _)) in CHOICES.iter().enumerate() {
            let rect = button_rect(index);
            // Nudged with the panel, so the whole plate arrives as one
            // thing rather than a box with buttons sliding inside it.
            let rect = Rect::new(rect.x0, rect.y0 - drop, rect.x1, rect.y1 - drop);
            let hovered = self.cursor.is_some_and(|(x, y)| {
                button_rect(index).contains(x, y)
            }) || self.focus == Some(index);
            let fill = if hovered { BUTTON_HOVER } else { BUTTON };
            p.quad(rect, alpha(fill, settle));
            p.border(
                rect,
                0.0035,
                alpha(if hovered { ACCENT } else { BUTTON_EDGE }, settle),
            );
            p.label_in(rect, language.text(*label), 1.1, alpha(widgets::TEXT, settle));
        }

        // The keys, under the buttons. Both are still bound, and a
        // player whose hand never left the keyboard should not have to
        // reach for the mouse to get back into the world.
        p.text_centred(
            language.text(Msg::DeathHelp),
            0.0,
            panel.y0 + 0.062 - drop,
            0.72,
            alpha(widgets::TEXT_DIM, settle),
        );
    }
}

/// The buttons, in the order they are drawn.
const CHOICES: &[(Msg, Choice)] = &[
    (Msg::Respawn, Choice::Respawn),
    (Msg::QuitToMenu, Choice::LeaveWorld),
];

/// Where a button sits. One definition, used to draw it and to hit-test
/// it -- so what lights up is by construction what a click activates.
fn button_rect(index: usize) -> Rect {
    let first = -0.10;
    let pitch = BUTTON_HEIGHT + 0.035;
    Rect::centred(
        0.0,
        first - index as f32 * pitch,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> DeathScreen {
        let mut screen = DeathScreen::new();
        screen.open("fell from a great height".to_string());
        screen
    }

    /// Runs the opening animation to the end.
    fn settled() -> DeathScreen {
        let mut screen = opened();
        screen.tick(0.1);
        for _ in 0..40 {
            screen.tick(0.05);
        }
        screen
    }

    #[test]
    fn animating_covers_the_opening_and_then_stops() {
        // While it reports animating, `main` rebuilds the interface on
        // time alone; a screen that never stopped would be the old
        // rebuild-every-frame behaviour back under a new name.
        let mut screen = opened();
        assert!(screen.is_animating(), "the opening went unreported");
        screen = settled();
        assert!(!screen.is_animating(), "a settled screen kept animating");
        screen.close();
        assert!(!screen.is_animating());
    }

    #[test]
    fn the_key_moves_with_the_highlight_and_not_with_the_clock() {
        let mut screen = settled();
        let plain = screen.ui_key();
        screen.tick(1.0);
        assert_eq!(plain, screen.ui_key(), "time alone moved the key");

        screen.move_focus(1);
        let focused = screen.ui_key();
        assert_ne!(plain, focused, "the keyboard highlight was invisible");

        // Onto a button: the hover highlight is what changes, not the
        // raw position -- a cursor wandering the empty wash must not
        // rebuild anything.
        let rect = button_rect(0);
        screen.set_cursor(Some((rect.centre_x(), rect.centre_y())));
        assert_ne!(focused, screen.ui_key(), "the hover highlight was invisible");
    }

    #[test]
    fn being_alive_draws_nothing_at_all() {
        let screen = DeathScreen::new();
        assert!(!screen.is_open());
        assert!(screen.build(FontAtlas::for_test(), Language::English).is_empty());
        assert_eq!(screen.cause(), None);
    }

    #[test]
    fn dying_says_what_happened_and_respawning_forgets_it() {
        let mut screen = settled();
        assert!(screen.is_open());
        assert_eq!(screen.cause(), Some("fell from a great height"));
        assert!(!screen.build(FontAtlas::for_test(), Language::English).is_empty());

        screen.close();
        assert!(!screen.is_open());
        assert!(screen.build(FontAtlas::for_test(), Language::English).is_empty());
    }

    #[test]
    fn the_wash_covers_a_window_of_any_shape() {
        // Authored as if the window were square, and the shader divides
        // x by the aspect -- so a quad that merely reached the edges of
        // a square window leaves bare strips down the sides of a wide
        // one.
        let vertices = settled().build(FontAtlas::for_test(), Language::English);
        let (left, right) = vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[0]), hi.max(v.position[0]))
        });
        assert!(left <= -4.0 && right >= 4.0, "the wash left bare edges");
    }

    #[test]
    fn the_screen_arrives_in_order_rather_than_all_at_once() {
        // Wash, then panel, then what killed you: the order they want
        // to be read in. All of it is over inside a second.
        let mut screen = opened();
        let count = |s: &DeathScreen| s.build(FontAtlas::for_test(), Language::English).len();
        let first_frame = count(&screen);
        assert!(first_frame > 0, "nothing at all on the frame they died");

        screen.tick(0.05);
        let washing = count(&screen);
        screen.tick(0.25);
        let with_panel = count(&screen);
        assert!(with_panel > washing, "the panel never arrived");

        for _ in 0..20 {
            screen.tick(0.05);
        }
        assert!(count(&screen) > with_panel, "the cause was never told");
        assert!(screen.age < 1.5, "the opening takes {}s", screen.age);
    }

    #[test]
    fn a_click_on_a_button_is_the_button_that_was_lit() {
        let mut screen = settled();
        // Nothing under the cursor, no choice.
        screen.set_cursor(Some((0.0, 0.9)));
        assert_eq!(screen.click(), None);

        for (index, (_, expected)) in CHOICES.iter().enumerate() {
            let rect = button_rect(index);
            screen.set_cursor(Some((rect.centre_x(), rect.centre_y())));
            assert_eq!(screen.click(), Some(*expected), "button {index}");
        }
    }

    #[test]
    fn a_click_with_no_cursor_chooses_nothing() {
        // The cursor is only known once it has moved over the window,
        // and a click before that must not activate whatever happens to
        // be at the origin.
        let screen = settled();
        assert_eq!(screen.click(), None);
    }

    #[test]
    fn the_buttons_do_not_overlap_each_other_or_leave_the_panel() {
        let panel = Rect::centred(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT);
        for index in 0..CHOICES.len() {
            let rect = button_rect(index);
            assert!(rect.x0 > panel.x0 && rect.x1 < panel.x1, "button {index} is too wide");
            assert!(rect.y0 > panel.y0 && rect.y1 < panel.y1, "button {index} escaped");
        }
        let (first, second) = (button_rect(0), button_rect(1));
        assert!(second.y1 < first.y0, "the buttons overlap");
    }

    #[test]
    fn the_keyboard_can_reach_both_buttons_and_wraps() {
        let mut screen = settled();
        assert_eq!(screen.focused(), None, "nothing is focused until asked");
        screen.move_focus(1);
        assert_eq!(screen.focused(), Some(Choice::Respawn));
        screen.move_focus(1);
        assert_eq!(screen.focused(), Some(Choice::LeaveWorld));
        screen.move_focus(1);
        assert_eq!(screen.focused(), Some(Choice::Respawn), "focus did not wrap");
        // Up from nothing lands on the last, which is what Up means.
        let mut screen = settled();
        screen.move_focus(-1);
        assert_eq!(screen.focused(), Some(Choice::LeaveWorld));
    }

    #[test]
    fn the_mouse_and_the_keyboard_do_not_both_own_the_highlight() {
        // Two lit buttons is two answers to "what would Enter do".
        let mut screen = settled();
        screen.move_focus(1);
        screen.set_cursor(Some((0.0, 0.0)));
        assert_eq!(screen.focused(), None, "the keyboard kept the highlight");
    }

    #[test]
    fn a_long_cause_is_truncated_rather_than_running_off_the_panel() {
        let mut screen = DeathScreen::new();
        screen.open(format!("was struck down by {}", "a".repeat(200)));
        for _ in 0..40 {
            screen.tick(0.05);
        }
        let vertices = screen.build(FontAtlas::for_test(), Language::English);
        let (left, right) = vertices
            .iter()
            // The wash spans the whole window; everything else is the
            // plate, and that is what has to fit.
            .filter(|v| v.position[0].abs() < 4.0)
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                (lo.min(v.position[0]), hi.max(v.position[0]))
            });
        assert!(
            left >= -PANEL_WIDTH / 2.0 - 0.02 && right <= PANEL_WIDTH / 2.0 + 0.02,
            "the plate runs from {left} to {right}"
        );
    }

    #[test]
    fn a_stall_does_not_skip_the_whole_animation() {
        // The frame a player dies on is exactly the frame that is
        // likeliest to be a long one -- a chunk landing, a mesh going
        // up. Without the clamp the screen would be fully open before
        // it was ever drawn.
        let mut screen = opened();
        screen.tick(9.0);
        assert!(screen.age <= 0.1, "one frame skipped {}s", screen.age);
    }
}
