//! The chat window: what people said, and the line you are typing.
//!
//! ## Two states, one widget
//!
//! Closed, it is a log: the last few lines, fading out after a while so
//! the corner of the screen does not permanently belong to a
//! conversation that finished ten minutes ago. Open (Enter), it is the
//! same log -- all of it, no fading, because you are reading it now --
//! plus a line you are typing into.
//!
//! ## Why Enter is not rebindable
//!
//! Same reason Escape is not: it is the way *out* of the thing it opens.
//! A player who bound chat to a letter would find that letter typing
//! itself into the box, and a player who bound it to something they
//! later forgot would have no way to answer anyone.
//!
//! ## What it does not do
//!
//! It does not decide anything. The text goes to the server as a
//! `Chat` message and comes back as one -- including commands, which the
//! server parses, and including the player's own line, which is *not*
//! echoed locally. Echoing locally is the shortcut that makes a client
//! show messages the server rejected (muted, rate-limited, filtered by a
//! plugin) as though they had been said.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::ui::hotbar::HotbarVertex;
use crate::engine::texture::FontAtlas;
use crate::ui::widgets::{self, Painter, Rect};

/// How much history is kept. Beyond this the oldest lines go: this is a
/// game's chat, not a transcript.
const HISTORY: usize = 80;
/// How many lines are on screen with the box closed.
const VISIBLE_CLOSED: usize = 6;
/// ...and open, where the point is to read what was said.
const VISIBLE_OPEN: usize = 12;
/// How long a line stays on screen once the box is closed.
const FADE_AFTER: Duration = Duration::from_secs(12);
const FADE_LENGTH: f32 = 1.5;

/// Longest line the box accepts, matching what the server will keep.
/// Typing past the limit silently does nothing, which is better than
/// sending something that arrives truncated.
const MAX_INPUT: usize = primitive_shared::protocol::MAX_CHAT_LEN;

const SCALE: f32 = 0.78;
const LINE_HEIGHT: f32 = 0.040;
/// Where the bottom line of the log sits, clear of the health gauge and
/// the hotbar under it.
const LOG_BOTTOM: f32 = -0.60;
const INPUT_Y: f32 = -0.68;

const PLATE: [f32; 4] = [0.02, 0.03, 0.05, 0.62];
const PLATE_OPEN: [f32; 4] = [0.02, 0.03, 0.05, 0.86];
const INPUT_PLATE: [f32; 4] = [0.04, 0.05, 0.08, 0.92];
const INPUT_EDGE: [f32; 4] = [0.45, 0.50, 0.60, 1.0];
/// Anything the server says in its own name.
const SYSTEM: [f32; 4] = [0.72, 0.80, 0.95, 1.0];

/// One line of the log.
struct Line {
    text: String,
    at: Instant,
    system: bool,
}

#[derive(Default)]
pub struct Chat {
    lines: VecDeque<Line>,
    /// `Some` while the box is open, holding what has been typed.
    input: Option<String>,
    /// When the box was opened, which is what the caret blinks against.
    opened_at: Option<Instant>,
}

impl Chat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_typing(&self) -> bool {
        self.input.is_some()
    }

    /// Opens the box, empty.
    pub fn open(&mut self, now: Instant) {
        self.input = Some(String::new());
        self.opened_at = Some(now);
    }

    /// Closes it, throwing away whatever was half-typed.
    pub fn close(&mut self) {
        self.input = None;
        self.opened_at = None;
    }

    pub fn type_char(&mut self, c: char) {
        let Some(input) = self.input.as_mut() else {
            return;
        };
        // Drawable only, and bounded: a character the font has no glyph
        // for would show as a box, and a control character in a chat
        // line is either a mistake or an attempt at one. The font's own
        // list is the filter, so a player whose interface speaks Russian
        // can also *write* it.
        if !crate::engine::texture::has_glyph(c) || input.chars().count() >= MAX_INPUT {
            return;
        }
        input.push(c);
    }

    pub fn backspace(&mut self) {
        if let Some(input) = self.input.as_mut() {
            input.pop();
        }
    }

    /// Takes what was typed and closes the box.
    ///
    /// Returns `None` for an empty line, so pressing Enter twice is a
    /// way to close the box rather than a way to say nothing.
    pub fn submit(&mut self) -> Option<String> {
        let typed = self.input.take()?;
        let trimmed = typed.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    /// Adds a line that came from the server.
    ///
    /// `username` is whoever said it; `None` for the server's own
    /// voice, which is drawn differently -- a player cannot be mistaken
    /// for the server, which matters the moment anyone tries.
    pub fn push(&mut self, username: Option<&str>, text: &str, now: Instant) {
        let text = match username {
            Some(name) => format!("<{name}> {text}"),
            None => text.to_string(),
        };
        self.lines.push_back(Line {
            text,
            at: now,
            system: username.is_none(),
        });
        while self.lines.len() > HISTORY {
            self.lines.pop_front();
        }
    }

    /// A line the client says to itself: connection notices and the
    /// like. Kept in the same log because the player does not care which
    /// side of the wire a message came from.
    pub fn note(&mut self, text: &str, now: Instant) {
        self.push(None, text, now);
    }

    /// Whether the widget would draw anything at all.
    ///
    /// Asked once a frame so that a game nobody is chatting in does not
    /// pay for laying out a log: with the box shut and every line faded,
    /// this is the whole cost of chat.
    pub fn has_anything_to_draw(&self, now: Instant) -> bool {
        self.is_typing()
            || self
                .lines
                .back()
                .is_some_and(|line| self.opacity(line, now) > 0.0)
    }

    /// How opaque a line should be, 0 when it has faded out entirely.
    ///
    /// Always 1 while the box is open: fading out what someone is
    /// reading is the one thing a chat log must not do.
    fn opacity(&self, line: &Line, now: Instant) -> f32 {
        if self.is_typing() {
            return 1.0;
        }
        let age = now.saturating_duration_since(line.at);
        if age < FADE_AFTER {
            return 1.0;
        }
        let over = (age - FADE_AFTER).as_secs_f32();
        (1.0 - over / FADE_LENGTH).clamp(0.0, 1.0)
    }

    /// A fingerprint of what `build` would draw, cheap enough to take
    /// every frame.
    ///
    /// The interface is only rebuilt when something on it changed -- see
    /// the UI block in `main` -- and this is how chat reports change
    /// without being built: two equal keys mean two identical widgets.
    /// The caret is in the key (it flips twice a second, and each flip
    /// is a real change); the fade alphas are deliberately *not*,
    /// because they move every frame -- `is_fading` is what tells the
    /// caller to keep rebuilding while one is in motion.
    pub fn ui_key(&self, now: Instant) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for line in &self.lines {
            // Only what would actually be drawn: a fully faded line is
            // not on screen, and hashing it would make the key change
            // when nothing visible did.
            if self.opacity(line, now) > 0.0 {
                line.text.hash(&mut h);
                line.system.hash(&mut h);
            }
        }
        self.input.hash(&mut h);
        if self.input.is_some() {
            self.caret_on(now).hash(&mut h);
        }
        h.finish()
    }

    /// Whether any line is mid-fade, which is the one part of this
    /// widget that changes with no event behind it.
    ///
    /// The window is padded by a moment past the end of the fade, so the
    /// caller is still told to rebuild on the frame a line reaches zero
    /// -- otherwise the last rebuild would leave a barely-visible ghost
    /// of it on screen for good.
    pub fn is_fading(&self, now: Instant) -> bool {
        if self.is_typing() {
            return false;
        }
        self.lines.iter().any(|line| {
            let age = now.saturating_duration_since(line.at);
            age >= FADE_AFTER
                && age.as_secs_f32() <= FADE_AFTER.as_secs_f32() + FADE_LENGTH + 0.25
        })
    }

    /// Whether the block caret is on this instant.
    fn caret_on(&self, now: Instant) -> bool {
        let since_open = self
            .opened_at
            .map(|at| now.saturating_duration_since(at))
            .unwrap_or_default();
        since_open.subsec_millis() < 500
    }

    /// Builds the widget. `aspect` is needed because chat hangs off the
    /// left edge of the *window*, which is at `-aspect` in UI space.
    ///
    /// The `Vec`-returning form, kept for the tests: they assert on one
    /// widget's output in isolation, which is exactly what appending
    /// into a shared list is designed not to produce.
    #[cfg(test)]
    pub fn build(&self, font: FontAtlas, aspect: f32, now: Instant) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        self.build_into(font, aspect, now, &mut out);
        out
    }

    /// The same widget, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(
        &self,
        font: FontAtlas,
        aspect: f32,
        now: Instant,
        out: &mut Vec<HotbarVertex>,
    ) {
        let mut p = Painter::onto(font, std::mem::take(out));
        let left = -aspect.max(0.1) + 0.03;
        let width = (aspect.max(0.1) * 2.0 - 0.06).min(1.6);

        let shown = if self.is_typing() { VISIBLE_OPEN } else { VISIBLE_CLOSED };
        let plate = if self.is_typing() { PLATE_OPEN } else { PLATE };

        for (index, line) in self.lines.iter().rev().take(shown).enumerate() {
            let alpha = self.opacity(line, now);
            if alpha <= 0.0 {
                continue;
            }
            let y = LOG_BOTTOM + index as f32 * LINE_HEIGHT;
            let text = widgets::fit(&line.text, SCALE, width - 0.02);
            let ink = widgets::ink_width(&text, SCALE);
            p.quad(
                Rect::new(left - 0.008, y - 0.006, left + ink + 0.010, y + LINE_HEIGHT - 0.008),
                dim(plate, alpha),
            );
            let colour = if line.system { SYSTEM } else { widgets::TEXT };
            p.text(&text, left, y + widgets::cell_height(SCALE) - 0.004, SCALE, dim(colour, alpha));
        }

        if let Some(input) = self.input.as_ref() {
            let box_rect = Rect::new(left - 0.010, INPUT_Y - 0.010, left + width, INPUT_Y + 0.036);
            p.quad(box_rect, INPUT_PLATE);
            p.border(box_rect, 0.002, INPUT_EDGE);

            // The tail of a long line, not the head: what you are typing
            // is at the end, and watching the caret disappear off the
            // edge of the box is the worst version of this.
            let shown_text = tail_that_fits(input, SCALE, width - 0.04);
            let typed_width = widgets::ink_width(&shown_text, SCALE);
            p.text(
                &shown_text,
                left,
                INPUT_Y + widgets::cell_height(SCALE) - 0.002,
                SCALE,
                widgets::TEXT,
            );
            // A block caret, blinking against the moment the box was
            // opened. The first version blinked against `now.elapsed()`
            // -- time since the instant that was passed *in*, which is
            // microseconds, so the caret was simply always on.
            if self.caret_on(now) {
                p.quad(
                    Rect::new(
                        left + typed_width + 0.004,
                        INPUT_Y - 0.002,
                        left + typed_width + 0.012,
                        INPUT_Y + widgets::cell_height(SCALE),
                    ),
                    widgets::TEXT,
                );
            }
        }

        *out = p.into_vertices();
    }
}

fn dim(colour: [f32; 4], alpha: f32) -> [f32; 4] {
    [colour[0], colour[1], colour[2], colour[3] * alpha]
}

/// The longest suffix of `text` that fits `max_width`.
fn tail_that_fits(text: &str, scale: f32, max_width: f32) -> String {
    if widgets::measure(text, scale) <= max_width {
        return text.to_string();
    }
    let per_char = widgets::measure("x", scale).max(1e-4);
    let keep = (max_width / per_char).floor().max(0.0) as usize;
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(keep)..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn typing_collects_printable_characters_only() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        for c in "hi \u{7}there\n".chars() {
            chat.type_char(c);
        }
        assert_eq!(chat.submit().as_deref(), Some("hi there"));
        assert!(!chat.is_typing(), "submitting left the box open");
    }

    #[test]
    fn chat_speaks_every_alphabet_the_font_does() {
        // A player whose interface is in Russian can write it too; a
        // character with no glyph (here, CJK) is still refused rather
        // than sent as a box.
        let mut chat = Chat::new();
        chat.open(Instant::now());
        for c in "привет 你 świecie".chars() {
            chat.type_char(c);
        }
        assert_eq!(chat.submit().as_deref(), Some("привет  świecie"));
    }

    #[test]
    fn nothing_is_typed_while_the_box_is_shut() {
        // Otherwise every letter of ordinary play accumulates in a line
        // that appears the moment the player opens chat.
        let mut chat = Chat::new();
        chat.type_char('w');
        chat.type_char('a');
        chat.open(Instant::now());
        assert_eq!(chat.submit(), None, "keys pressed while walking became a message");
    }

    #[test]
    fn an_empty_line_says_nothing() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        assert_eq!(chat.submit(), None);
        chat.open(Instant::now());
        chat.type_char(' ');
        chat.type_char(' ');
        assert_eq!(chat.submit(), None, "whitespace was sent as a message");
    }

    #[test]
    fn backspace_deletes_and_escape_throws_the_line_away() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        for c in "helloo".chars() {
            chat.type_char(c);
        }
        chat.backspace();
        assert!(chat.is_typing());
        chat.close();
        assert!(!chat.is_typing());
        chat.open(Instant::now());
        assert_eq!(chat.submit(), None, "a cancelled line came back");
    }

    #[test]
    fn a_line_is_capped_at_what_the_server_will_keep() {
        // A message that arrives truncated is worse than one that could
        // not be typed: the player watches it send and sees it cut.
        let mut chat = Chat::new();
        chat.open(Instant::now());
        for _ in 0..(MAX_INPUT + 50) {
            chat.type_char('x');
        }
        let sent = chat.submit().expect("something");
        assert_eq!(sent.chars().count(), MAX_INPUT);
    }

    #[test]
    fn the_log_keeps_the_most_recent_lines_and_drops_the_rest() {
        let mut chat = Chat::new();
        for i in 0..(HISTORY + 20) {
            chat.push(Some("someone"), &format!("line {i}"), now());
        }
        assert_eq!(chat.lines.len(), HISTORY);
        assert!(chat.lines.back().unwrap().text.contains(&format!("line {}", HISTORY + 19)));
    }

    #[test]
    fn the_server_and_a_player_do_not_look_alike() {
        // A player called "server" must not be able to pass for one.
        let mut chat = Chat::new();
        chat.push(None, "the world is saving", now());
        chat.push(Some("server"), "give me your things", now());
        let system = &chat.lines[0];
        let player = &chat.lines[1];
        assert!(system.system && !player.system);
        assert!(player.text.starts_with("<server>"), "{}", player.text);
    }

    #[test]
    fn old_lines_fade_out_when_the_box_is_closed_and_not_while_it_is_open() {
        let mut chat = Chat::new();
        let long_ago = Instant::now() - FADE_AFTER - Duration::from_secs(5);
        chat.push(Some("a"), "old news", long_ago);
        let stale = Line {
            text: chat.lines[0].text.clone(),
            at: long_ago,
            system: false,
        };

        assert_eq!(chat.opacity(&stale, Instant::now()), 0.0, "a stale line stayed up");
        chat.open(Instant::now());
        assert_eq!(
            chat.opacity(&stale, Instant::now()),
            1.0,
            "the log faded out while it was being read"
        );
    }

    /// Draws the chat window to a PNG, for looking at.
    ///
    /// ```text
    /// cargo test -p primitive_client --bin primitive_client -- --ignored dump_the_chat
    /// ```
    #[test]
    #[ignore = "diagnostic: writes a picture of the chat window"]
    fn dump_the_chat_to_a_png() {
        const WIDTH: u32 = 1600;
        const HEIGHT: u32 = 900;

        let mut chat = Chat::new();
        let now = Instant::now();
        chat.note("connected to grim.example.net", now);
        chat.push(Some("Shamkhan"), "found a cave under the ridge", now);
        chat.push(Some("Ivan"), "bring planks, mine went in the river", now);
        chat.note("Ivan fell from a great height", now);
        chat.push(Some("Shamkhan"), "/players", now);
        chat.note("2 known player(s):", now);
        if std::env::var("PRIMITIVE_CHAT_OPEN").is_ok() {
            chat.open(Instant::now());
            for c in "on my way with the planks".chars() {
                chat.type_char(c);
            }
        }

        let vertices = chat.build(FontAtlas::for_test(), WIDTH as f32 / HEIGHT as f32, now);
        let path = std::env::var("PRIMITIVE_UI_DUMP")
            .unwrap_or_else(|_| "target/chat.png".to_string());
        widgets::dump_to_png(&vertices, WIDTH, HEIGHT, &path);
        println!("wrote {path}");
    }

    #[test]
    fn the_key_moves_exactly_when_the_chat_does() {
        // Rebuilds are driven by this key -- see the UI block in `main`
        // -- so it has to hold still for an untouched log and move for
        // anything a player would see.
        let mut chat = Chat::new();
        let at = Instant::now();
        assert_eq!(chat.ui_key(at), chat.ui_key(at), "an idle key drifted");

        let quiet = chat.ui_key(at);
        chat.push(Some("a"), "hello", at);
        let spoken = chat.ui_key(at);
        assert_ne!(quiet, spoken, "a new line was invisible to the key");

        chat.open(at);
        let opened = chat.ui_key(at);
        assert_ne!(spoken, opened, "opening the box was invisible");
        chat.type_char('x');
        assert_ne!(opened, chat.ui_key(at), "typing was invisible");
    }

    #[test]
    fn fading_is_reported_while_a_line_fades_and_not_after() {
        let mut chat = Chat::new();
        let now = Instant::now();
        chat.push(Some("a"), "hello", now);
        assert!(!chat.is_fading(now), "a fresh line claimed to be fading");

        let mid_fade = now + FADE_AFTER + Duration::from_millis(500);
        assert!(chat.is_fading(mid_fade), "a fading line went unreported");

        let long_gone = now + FADE_AFTER + Duration::from_secs(30);
        assert!(!chat.is_fading(long_gone), "a dead log kept forcing rebuilds");
    }

    #[test]
    fn a_quiet_game_pays_nothing_for_chat() {
        // Asked once a frame. A game nobody is chatting in must not be
        // laying out a log it is not going to draw.
        let mut chat = Chat::new();
        assert!(!chat.has_anything_to_draw(Instant::now()));

        chat.push(Some("a"), "hello", Instant::now());
        assert!(chat.has_anything_to_draw(Instant::now()));

        // ...and once the last line has faded, back to nothing.
        let mut stale = Chat::new();
        let long_ago = Instant::now() - FADE_AFTER - Duration::from_secs(5);
        stale.push(Some("a"), "old news", long_ago);
        assert!(!stale.has_anything_to_draw(Instant::now()));
        assert!(stale.build(FontAtlas::for_test(), 1.78, Instant::now()).is_empty());

        // Except with the box open, where the log is what is being read.
        stale.open(Instant::now());
        assert!(stale.has_anything_to_draw(Instant::now()));
    }

    #[test]
    fn the_caret_blinks() {
        // It blinked against `now.elapsed()` -- time since the instant
        // passed *in*, which is microseconds -- so it was simply always
        // on, and an empty box looked like one that was not listening.
        let mut chat = Chat::new();
        let opened = Instant::now();
        chat.open(opened);
        let count = |at: Instant| {
            chat.build(FontAtlas::for_test(), 1.78, at).len()
        };
        let on = count(opened + Duration::from_millis(100));
        let off = count(opened + Duration::from_millis(700));
        assert!(on > off, "the caret never goes out ({on} vs {off})");
        assert_eq!(
            count(opened + Duration::from_millis(1_100)),
            on,
            "the caret never comes back"
        );
    }

    #[test]
    fn a_closed_chat_with_nothing_in_it_draws_nothing() {
        let chat = Chat::new();
        assert!(chat.build(FontAtlas::for_test(), 1.78, now()).is_empty());
    }

    #[test]
    fn an_open_chat_draws_its_box_even_when_empty() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        assert!(!chat.build(FontAtlas::for_test(), 1.78, now()).is_empty());
    }

    #[test]
    fn the_widget_stays_off_the_hotbar_and_inside_the_window() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        for i in 0..VISIBLE_OPEN + 4 {
            chat.push(Some("talker"), &format!("message number {i} with some length"), now());
        }
        for c in "typing something quite long into the box".chars() {
            chat.type_char(c);
        }
        for aspect in [1.0f32, 1.78, 2.4] {
            let vertices = chat.build(FontAtlas::for_test(), aspect, now());
            let (mut left, mut right) = (f32::MAX, f32::MIN);
            let mut lowest = f32::MAX;
            for v in &vertices {
                left = left.min(v.position[0]);
                right = right.max(v.position[0]);
                lowest = lowest.min(v.position[1]);
            }
            assert!(left >= -aspect - 1e-3, "chat ran off the left edge at aspect {aspect}");
            assert!(right <= aspect + 1e-3, "chat ran off the right edge at aspect {aspect}");
            assert!(
                lowest > crate::ui::hotbar::BOTTOM + crate::ui::hotbar::SLOT,
                "chat overlaps the hotbar at aspect {aspect}"
            );
        }
    }
}
