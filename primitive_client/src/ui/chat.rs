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
//!
//! ## What was missing, and where each answer is
//!
//! The player's report was that it "looks very plain, the basic parts
//! are not there". They were exactly these, and they are worth listing
//! because each one is a rule the rest of the file follows:
//!
//! * **Reading what has scrolled off.** The log kept eighty lines and
//!   drew twelve, so sixty-eight of them existed and could not be
//!   looked at. It scrolls now -- with the wheel, and with a thumb,
//!   because a drag arrives as a wheel already (see the
//!   `Gesture::Scrolled` arm in `lib.rs`), so the phone needed nothing
//!   of its own.
//! * **Telling one kind of line from another.** There were two: the
//!   server's and everybody else's. There are four, because "somebody
//!   said this", "*I* said this", "the server is telling you something"
//!   and "that was refused" are four different things to the person
//!   reading -- and the last especially, since a command that failed
//!   used to look exactly like a command that worked. See [`Kind`].
//! * **When a line was said.** Stamped with the *world's* clock rather
//!   than the wall's -- see [`Chat::set_world_time`] for why that is a
//!   decision and not a shortcut.
//! * **That anything was said at all.** A line fades after twelve
//!   seconds whether or not anybody read it. See [`Chat::unread`].
//! * **Saying the same thing again.** The arrow keys walk back through
//!   what was sent, which is mostly a way of fixing a mistyped command
//!   without retyping the whole of it.
//! * **Finding out what the commands are.** Typing `/` lists them. The
//!   list is the *server's own* table -- see [`Chat::command_hint`].
//!
//! And one thing that is deliberately not here: coloured names,
//! avatars, reactions, a second panel for whispers. The game is one
//! bitmap font and a handful of greys, and a chat that looked like a
//! messenger would be the loudest thing on the screen.

use std::cell::Cell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::ui::hotbar::HotbarVertex;
use crate::engine::texture::FontAtlas;
use crate::ui::field::{Motion, TextField};
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// How much history is kept. Beyond this the oldest lines go: this is a
/// game's chat, not a transcript.
///
/// Raised from eighty when the log learned to scroll, and only then.
/// Eighty lines with twelve of them reachable was not a history, it was
/// twelve lines and sixty-eight that cost memory and could not be read.
const HISTORY: usize = 200;
/// How many lines are on screen with the box closed.
const VISIBLE_CLOSED: usize = 6;
/// ...and open, where the point is to read what was said.
const VISIBLE_OPEN: usize = 12;
/// How long a line stays on screen once the box is closed.
const FADE_AFTER: Duration = Duration::from_secs(12);
const FADE_LENGTH: f32 = 1.5;

/// How many sent lines the box remembers, for the arrow keys.
///
/// Small on purpose: this is for "say that again" and "I mistyped the
/// coordinates", not for a shell history. A player who wanted the
/// fortieth line back would find retyping it quicker than pressing Up
/// forty times.
const RECALL: usize = 20;

/// How many command lines the hint offers at once.
///
/// Four, because the hint stands between the log and the box: every row
/// it takes is a row of conversation covered up. Nothing in the table
/// matches more than four rows on a prefix of one letter, and the bare
/// `/` -- which matches everything -- is the case where the player has
/// not yet said what they are looking for.
const HINT_ROWS: usize = 4;

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

/// How far the whole widget is pushed up the glass once it has been
/// grown, so that the lowest thing it draws clears the keyboard.
///
/// **The chat box cannot live where the chat log lives, on a phone.**
/// Both are pinned to the bottom-left corner, which is right for a log
/// -- it is the corner nothing else wants -- and wrong for the one
/// widget that is only ever on screen *because a keyboard is up*. The
/// player's report was exact: "ввод всё ещё не работает... клавиатуре
/// некуда встать, если поле ввода находится низко". The line being
/// typed, its caret, and both of its buttons were drawn under the
/// keyboard typing into them.
///
/// **Why this is not simply authored higher.** Because the widget is
/// grown about the corner it is pinned to, and at any scale below one
/// that growth pulls everything back *toward* the corner: a box
/// authored above [`widgets::KEYBOARD_TOP`] lands below it again, by
/// more the smaller the interface is set. The clearance is a fact about
/// the finished picture, so it has to be applied to the finished
/// picture.
///
/// **Both sides call this.** The frame loop lifts the geometry by it
/// and the hit-test subtracts it before undoing the growth, which is
/// the rule this interface is held to: what draws and what is pressed
/// must be inverses, and the way to keep two of those in step is to
/// have one number.
///
/// Zero everywhere but a phone with the box open, so the desktop layout
/// is untouched and cannot be broken by this.
pub fn keyboard_lift(touch: bool, typing: bool, scale: f32) -> f32 {
    if !touch || !typing {
        return 0.0;
    }
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let corner = -1.0;
    let grown_bottom = corner + (authored_bottom(touch) - corner) * scale;
    ((widgets::KEYBOARD_TOP + CLEARANCE) - grown_bottom).max(0.0)
}

/// The lowest point the widget draws, before it is grown.
///
/// **Not the box: the buttons.** They are floored to a finger and
/// centred on the row, so they reach further down than the plate behind
/// the text by half the difference. Clearing the keyboard by the box
/// alone left them under it, which is the same bug one layer in.
fn authored_bottom(touch: bool) -> f32 {
    let row_bottom = INPUT_Y - INPUT_DROP;
    if !touch {
        return row_bottom;
    }
    let side = widgets::tappable_when(INPUT_HEIGHT, true);
    (row_bottom + INPUT_HEIGHT / 2.0) - side / 2.0
}

/// How far above the keyboard the lowest thing drawn is put.
///
/// Small on purpose: every hundredth spent here is a hundredth of chat
/// log covered by the box on a screen that has little enough of it.
const CLEARANCE: f32 = 0.02;

/// The height of the plate the typed line sits on.
const INPUT_HEIGHT: f32 = 0.046;

/// How far that plate hangs below the text's own baseline row.
const INPUT_DROP: f32 = 0.010;

/// How much of the screen the log and the box occupy, measured from the
/// bottom-left corner they are pinned to.
///
/// What the frame loop asks before growing them -- see
/// `widgets::Layout::fit_from_corner`. Wider than `width` caps at,
/// because the plate behind a line is drawn a little past the text, and
/// tall enough for the open log's twelve lines plus the box under them.
pub const EXTENT: (f32, f32) = (1.62, 0.90);

const PLATE: [f32; 4] = [0.02, 0.03, 0.05, 0.62];
const PLATE_OPEN: [f32; 4] = [0.02, 0.03, 0.05, 0.86];
const INPUT_PLATE: [f32; 4] = [0.04, 0.05, 0.08, 0.92];
const INPUT_EDGE: [f32; 4] = [0.45, 0.50, 0.60, 1.0];
/// Anything the server says in its own name.
const SYSTEM: [f32; 4] = [0.72, 0.80, 0.95, 1.0];
/// The player's own line, come back from the server.
///
/// Warm against the cool blue the server speaks in, and never used for
/// anything else: the one line in the log a player scans for is their
/// own, to check that it was actually said.
const MINE: [f32; 4] = [0.96, 0.86, 0.58, 1.0];
/// The clock in front of an open line, and the markers that say there
/// is more log above or below.
///
/// Quieter than any of the kinds of line: it is punctuation, and a
/// timestamp that competes with the sentence beside it is a timestamp
/// that has to be read past twelve times a screen.
const QUIET: [f32; 4] = [0.48, 0.53, 0.60, 1.0];

/// What kind of line this is.
///
/// **Three rather than the "system or not" this used to carry**: they are
/// three different things to the person reading. There was a fourth, a
/// refusal in red, and it went with the refusals themselves -- every
/// "that does not go there" the server answered a gesture with was written
/// into the log, and a log of the player's own mistakes over what the
/// others said was noise that buried the conversation. A refusal is the HUD's banner now (`ServerMessage::Error` in
/// `lib.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Somebody else said something.
    Said,
    /// The player's own line, as the server echoed it back.
    Mine,
    /// The server in its own voice: joins, deaths, command replies.
    System,
}

impl Kind {
    fn colour(self) -> [f32; 4] {
        match self {
            Kind::Said => widgets::TEXT,
            Kind::Mine => MINE,
            Kind::System => SYSTEM,
        }
    }

    /// Whether a line of this kind, missed, is worth a badge.
    ///
    /// **Only what a person said.** The alternative was tried and is
    /// worse: counting the server's own lines means a singleplayer
    /// world, where the only thing that ever speaks is the server
    /// saying "you joined", wears a permanent unread badge from the
    /// first second of the first session. A badge that is always on is
    /// a badge nobody looks at, and it would have been on in the one
    /// mode where there is provably nothing to read.
    fn worth_noticing(self) -> bool {
        matches!(self, Kind::Said)
    }
}

/// A block behind the selected part of the line being typed.
///
/// Dim, and behind the letters rather than over them: the box is a dark
/// plate over the world already, and anything brighter here would be the
/// loudest thing on the screen while somebody is typing into it.
const SELECTION: [f32; 4] = [0.30, 0.40, 0.55, 0.85];

/// One edit of the line being typed.
///
/// The same shape the menu's own forms use. It is declared here rather
/// than shared with `menu` because the two are reached by different
/// paths -- a menu key goes through `menu::Key` and a chat key does not
/// -- and a type in common would be a dependency between two screens
/// that have nothing else to say to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Backspace { word: bool },
    Delete { word: bool },
    Move { motion: Motion, extend: bool },
    SelectAll,
}

/// One line of the log.
struct Line {
    text: String,
    at: Instant,
    kind: Kind,
    /// Minutes into the world's own day, 0..1439. See
    /// [`Chat::set_world_time`].
    stamp: u16,
}

/// One row as it will be drawn: a line may be several of these, because
/// an open log wraps rather than truncates.
struct Row {
    text: String,
    /// Whether this row carries the clock. Only the first row of a
    /// wrapped line does; the rest are indented under it, so the
    /// timestamp column stays a column.
    stamped: bool,
    stamp: u16,
    colour: [f32; 4],
    alpha: f32,
}

#[derive(Default)]
pub struct Chat {
    lines: VecDeque<Line>,
    /// `Some` while the box is open, holding what has been typed.
    ///
    /// A [`TextField`] rather than a `String`, so the one line a player
    /// types most often in this game is edited the same way the form
    /// fields are -- see `ui::field`. A command mistyped at the fourth
    /// character used to mean deleting back to it.
    input: Option<TextField>,
    /// When the box was opened, which is what the caret blinks against.
    opened_at: Option<Instant>,
    /// How many rows up from the bottom the log is looked at.
    ///
    /// Rows rather than lines, because a wrapped line is several rows
    /// and a scroll that moved in lines would jump by four when it met
    /// a long one.
    scroll: usize,
    /// The furthest `scroll` may go, worked out by the last `build`.
    ///
    /// **A `Cell`, and this is the one place in the file that needed
    /// justifying.** How many rows there are depends on how wide the
    /// window is and on whether the box is open -- facts that only
    /// exist while drawing, and drawing takes `&self` because the frame
    /// loop holds the chat immutably there. The alternative was for
    /// `scroll_by` to guess the bound from the line count, which is
    /// wrong by however much wrapping added and leaves the log able to
    /// scroll into blank space above its own first line.
    max_scroll: Cell<usize>,
    /// Lines somebody said that the player has not had the box open
    /// for. Cleared by opening it.
    unread: usize,
    /// Minutes into the world's day, as of the last frame. What a new
    /// line is stamped with.
    world_minutes: u16,
    /// What has been sent, newest last, for the arrow keys.
    sent: VecDeque<String>,
    /// How far back through `sent` the player has walked, or `None`
    /// while they are typing something of their own.
    recall: Option<usize>,
    /// The cairn whose name the box is asking for, while it is -- see
    /// [`Chat::open_naming`]. `None` for an ordinary line to say.
    naming: Option<(i32, i32, i32)>,
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
        self.input = Some(TextField::new());
        self.opened_at = Some(now);
        self.recall = None;
        // Opening is reading: the log is shown from the bottom, which
        // is where the newest line is, and nothing is unread any more.
        self.scroll = 0;
        self.unread = 0;
    }

    /// Opens the box to ask for a cairn's name, holding the name it has.
    ///
    /// **The chat box, and not a naming screen of its own.** The box is the
    /// one line of text in a running world that already works on a phone:
    /// the input method's mirror (`ime::Typing`), the lift above the
    /// keyboard, the send and leave buttons a touch screen needs because
    /// an input method's "Done" key is often nothing a game can read. A
    /// second field would be a second copy of all of it, proven only by a
    /// human tapping a phone (see `CLAUDE.md`: a phone cannot be driven).
    /// What changes is where the line goes: `submit` answers it and the
    /// frame files it with the map rather than sending it to anybody.
    pub fn open_naming(&mut self, at: (i32, i32, i32), current: &str, now: Instant) {
        self.open(now);
        self.naming = Some(at);
        if let Some(input) = self.input.as_mut() {
            input.set_text(current);
        }
    }

    /// The cairn being named, while the box is asking for a name.
    pub fn naming(&self) -> Option<(i32, i32, i32)> {
        self.naming.filter(|_| self.is_typing())
    }

    /// Closes it, throwing away whatever was half-typed.
    pub fn close(&mut self) {
        self.naming = None;
        self.input = None;
        self.opened_at = None;
        self.recall = None;
        self.scroll = 0;
    }

    /// What the world's clock says, so a line can be stamped with it.
    ///
    /// **The world's clock and not the wall's**, and that is a decision
    /// with a reason on both sides. `SystemTime` is UTC and the standard
    /// library has no way to turn it into local time, so a wall-clock
    /// stamp would be right in Britain in winter and wrong everywhere
    /// else -- a number that looks authoritative and is off by hours.
    /// The world's own time needs nothing, is the same for everybody on
    /// the server, and answers the question a player in this game
    /// actually asks: not "was that at half past four" but "was that
    /// before dark".
    ///
    /// Set once a frame from the sky. Handed in rather than read,
    /// because the chat has no business knowing about `logic::sky`.
    pub fn set_world_time(&mut self, time_of_day: f32) {
        let day = if time_of_day.is_finite() {
            time_of_day.rem_euclid(1.0)
        } else {
            0.0
        };
        self.world_minutes = ((day * 24.0 * 60.0) as u16).min(24 * 60 - 1);
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
        // A selection is about to be replaced, so a full line still
        // takes the character -- select-all-then-type on a line at the
        // cap is exactly the line somebody wants to throw away.
        if !crate::engine::texture::has_glyph(c)
            || (input.chars() >= MAX_INPUT && input.selection().is_none())
        {
            return;
        }
        input.insert(c);
        // Typing is leaving the recalled line behind: the player has
        // made it theirs, and the next Up should walk from the end
        // again rather than from wherever they had got to.
        self.recall = None;
    }

    /// One edit of the line being typed.
    ///
    /// The same vocabulary the menu forms use, for the same reason: the
    /// chat box and a form field are one kind of thing to the person
    /// typing, and two sets of rules for them is one set that gets
    /// forgotten.
    pub fn edit(&mut self, edit: Edit) {
        let Some(input) = self.input.as_mut() else {
            return;
        };
        match edit {
            Edit::Backspace { word: false } => input.backspace(),
            Edit::Backspace { word: true } => input.backspace_word(),
            Edit::Delete { word: false } => input.delete(),
            Edit::Delete { word: true } => input.delete_word(),
            Edit::Move { motion, extend } => input.move_caret(motion, extend),
            Edit::SelectAll => input.select_all(),
        }
        // Every one of these leaves the line the player's own: the next
        // Up should walk from the end of the sent lines again rather
        // than from wherever the last recall had got to.
        if !matches!(edit, Edit::Move { .. }) {
            self.recall = None;
        }
    }

    /// Walks back through what has been sent, and forward again.
    ///
    /// **Mostly a way of fixing a command.** Chat lines are said once;
    /// commands are mistyped, and `/tp 812 74 -1290` is thirteen
    /// characters of coordinate that nobody wants to enter twice
    /// because the first attempt was refused for the fourteenth.
    ///
    /// Walking off the front stops at the oldest line rather than
    /// wrapping round to the newest, which is what every shell does and
    /// what a player pressing Up repeatedly expects. Walking off the
    /// back empties the box, which is how you get back to a blank line
    /// without pressing Escape and losing the box with it.
    pub fn recall(&mut self, delta: i32) {
        if self.input.is_none() || self.sent.is_empty() {
            return;
        }
        let last = self.sent.len() - 1;
        let next = match (self.recall, delta) {
            (None, d) if d < 0 => Some(last),
            (None, _) => None,
            (Some(at), d) if d < 0 => Some(at.saturating_sub(1)),
            (Some(at), _) if at >= last => None,
            (Some(at), _) => Some(at + 1),
        };
        self.recall = next;
        let line = next.and_then(|at| self.sent.get(at)).cloned();
        // The caret lands at the end of the recalled line, which is
        // where somebody about to correct a coordinate wants it.
        self.input = Some(TextField::with(line.unwrap_or_default()));
    }

    /// What is half-typed in the box, for a platform that keeps its own
    /// copy of it.
    ///
    /// Empty when the box is shut, rather than an `Option`: the caller
    /// is asking what an input method should be holding, and for a
    /// closed box that is nothing. See `platform::Window::ime_owns_text`.
    pub fn typed_text(&self) -> &str {
        self.input.as_ref().map_or("", TextField::text)
    }

    /// Replaces the half-typed line with what an outside editor says it
    /// holds.
    ///
    /// Through `type_char`, for the reason spelled out on
    /// `Menu::set_focused_text`: the glyph filter and the length cap
    /// live there, and a setter that went round them would let an input
    /// method put a line in the box that the box cannot draw.
    pub fn set_typed_text(&mut self, text: &str) {
        if self.input.is_none() {
            return;
        }
        self.input = Some(TextField::new());
        for c in text.chars() {
            self.type_char(c);
        }
    }

    /// Takes what was typed and closes the box.
    ///
    /// Returns `None` for an empty line, so pressing Enter twice is a
    /// way to close the box rather than a way to say nothing.
    pub fn submit(&mut self) -> Option<String> {
        let typed = self.input.take()?;
        self.opened_at = None;
        self.recall = None;
        self.scroll = 0;
        let trimmed = typed.text().trim();
        // A name is not a line said: it is not recalled with Up, and an
        // empty one is an answer (the cairn stays unnamed), so the caller
        // is told it with the empty string rather than with nothing.
        if self.naming.take().is_some() {
            return Some(trimmed.to_string());
        }
        if trimmed.is_empty() {
            return None;
        }
        // Remembered here rather than where it is sent, because this is
        // the one place that knows a line was actually said. The
        // same line twice running is not two entries: repeating
        // yourself is common and pressing Up past three copies of one
        // command is not.
        if self.sent.back().map(String::as_str) != Some(trimmed) {
            self.sent.push_back(trimmed.to_string());
            while self.sent.len() > RECALL {
                self.sent.pop_front();
            }
        }
        Some(trimmed.to_string())
    }

    /// One player's line, with whether it is this player's own.
    ///
    /// `mine` comes from the id on the wire rather than from the name,
    /// and that is the whole of it: two players may be called the same
    /// thing on a server that allows it, and a log that highlighted by
    /// name would show one of them their neighbour's messages as their
    /// own.
    pub fn said(&mut self, username: &str, mine: bool, text: &str, now: Instant) {
        let kind = if mine { Kind::Mine } else { Kind::Said };
        self.add(kind, format!("<{username}> {text}"), now);
    }

    /// A line the client says to itself: connection notices and the
    /// like. Kept in the same log because the player does not care which
    /// side of the wire a message came from.
    pub fn note(&mut self, text: &str, now: Instant) {
        self.add(Kind::System, text.to_string(), now);
    }

    fn add(&mut self, kind: Kind, text: String, now: Instant) {
        if kind.worth_noticing() && !self.is_typing() {
            self.unread += 1;
        }
        // A line arriving while the log is scrolled up must not slide
        // the view: the player is reading something, and a log that
        // moves under them every time somebody speaks is a log they
        // cannot finish a sentence in. Kept in *rows from the bottom*,
        // so this is one more row between here and the bottom.
        if self.scroll > 0 {
            self.scroll += 1;
        }
        self.lines.push_back(Line {
            text,
            at: now,
            kind,
            stamp: self.world_minutes,
        });
        while self.lines.len() > HISTORY {
            self.lines.pop_front();
            self.scroll = self.scroll.saturating_sub(1);
        }
    }

    /// How many things people said while the box was shut.
    ///
    /// Cleared by opening it, which is the only honest definition of
    /// "read" a game's chat has: there is no per-line acknowledgement
    /// to hang it on and nothing to gain from inventing one.
    pub fn unread(&self) -> usize {
        self.unread
    }

    /// Moves the log. Positive walks back into the past.
    ///
    /// Bounded by what the last frame actually drew, so the log stops
    /// at its own oldest row instead of scrolling into blank space --
    /// the failure every list in this game is held to not having. See
    /// `max_scroll`.
    pub fn scroll_by(&mut self, rows: i32) {
        if !self.is_typing() {
            // A closed log is a glance, not a document. Scrolling one
            // would move something the player cannot see the edges of,
            // and the wheel means "change what I am holding" out there.
            return;
        }
        let ceiling = self.max_scroll.get() as i32;
        self.scroll = (self.scroll as i32 + rows).clamp(0, ceiling.max(0)) as usize;
    }

    /// The command lines worth showing under a half-typed `/`.
    ///
    /// **The server's own table, not a copy of it.** `HELP_TEXT` is what
    /// `/help` prints, it lives beside the parser that reads these
    /// commands, and there is a test in that module which fails if a
    /// command is added without a line in it. A list written out here
    /// instead would be a second table with nothing keeping it honest,
    /// and the first command anybody added would be missing from
    /// exactly one of the two.
    ///
    /// The client already links the server -- singleplayer runs a real
    /// one in-process -- so this costs nothing. Against a *remote*
    /// server the two tables are the same table only because the
    /// protocol version pins the pair; a mod that claimed its own
    /// command would not appear here, which is why this is a hint and
    /// the server is still the one that answers.
    pub fn command_hint(&self) -> Vec<&'static str> {
        use primitive_server::logic::commands::HELP_TEXT;
        let Some(typed) = self
            .input
            .as_ref()
            .and_then(|t| t.text().strip_prefix('/'))
        else {
            return Vec::new();
        };
        let finished = typed.contains(' ');
        let word = typed.split(' ').next().unwrap_or("").to_ascii_lowercase();
        HELP_TEXT
            .iter()
            .copied()
            .filter(|line| {
                let name = line
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                // Once there is a space the player has chosen a command
                // and is typing its arguments -- so the hint narrows to
                // that one row, which is where its usage is written.
                if finished {
                    name == word
                } else {
                    name.starts_with(&word)
                }
            })
            .take(HINT_ROWS)
            .collect()
    }

    /// Whether the widget would draw anything at all.
    ///
    /// Asked once a frame so that a game nobody is chatting in does not
    /// pay for laying out a log: with the box shut and every line faded,
    /// this is the whole cost of chat.
    pub fn has_anything_to_draw(&self, now: Instant) -> bool {
        self.is_typing()
            || self.unread() > 0
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
                line.kind.hash(&mut h);
                line.stamp.hash(&mut h);
            }
        }
        self.typed_text().hash(&mut h);
        // The caret is a bar on screen and moving it moves nothing
        // else, so a key that only walked it would leave the bar drawn
        // where it used to be.
        if let Some(input) = self.input.as_ref() {
            input.caret().hash(&mut h);
            input.selection().hash(&mut h);
        }
        // Scrolling moves nothing else -- no line arrives, no key is
        // pressed -- so without this the wheel would move a log that
        // went on being drawn where it was, and went on being *pressed*
        // where it was drawn. The server list had exactly this bug.
        self.scroll.hash(&mut h);
        self.unread.hash(&mut h);
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
        self.build_into(font, aspect, false, Language::English, now, &mut out);
        out
    }

    /// What a finger on the chat box landed on, if anything.
    ///
    /// **The exact inverse of what [`input_row`] draws**, and it shares
    /// the rectangles with it rather than repeating them, which is the
    /// rule this interface is held to: a hit-test that has drifted from
    /// the drawing is a box that looks right and does nothing where it
    /// is pressed.
    ///
    /// `at` is in the space the chat is *authored* in -- the caller
    /// takes the finger back through the growth the widget was drawn
    /// with, the same way the hotbar's own hit-test does.
    pub fn tapped(&self, aspect: f32, touch: bool, at: (f32, f32)) -> Option<Tap> {
        if !self.is_typing() {
            return None;
        }
        let (typed, buttons) = input_row(aspect, touch);
        if let Some((send, leave)) = buttons {
            if send.contains(at.0, at.1) {
                return Some(Tap::Send);
            }
            if leave.contains(at.0, at.1) {
                return Some(Tap::Leave);
            }
        }
        if typed.contains(at.0, at.1) {
            // **The exact inverse of what `build_into` draws**, and it
            // shares the arithmetic rather than repeating it: the same
            // margin, the same window on a long line, and the same walk
            // through it. See `widgets::caret_in_run`.
            let input = self.input.as_ref()?;
            let left = typed.x0 + 0.010;
            let usable = typed.width() - 0.030;
            let text = input.text();
            let (from, to) = widgets::window(text, input.caret(), SCALE, usable);
            return Some(Tap::Caret(widgets::caret_in_run(
                text, from, to, SCALE, left, at.0,
            )));
        }
        None
    }

    /// Puts the caret where a tap landed. See [`Tap::Caret`].
    pub fn place_caret(&mut self, at: usize) {
        if let Some(input) = self.input.as_mut() {
            input.place_caret(at);
            // The line is the player's own again: the next Up walks
            // from the end of what was sent rather than from wherever
            // the last recall had got to.
            self.recall = None;
        }
    }

    /// The log as rows, newest first, ready to be drawn upward from the
    /// bottom.
    ///
    /// **Truncated when shut and wrapped when open**, which is one
    /// decision rather than an inconsistency. A shut log is glanced at
    /// over the top of the world: one line per thing said keeps six
    /// different things on screen, and a single long sentence eating
    /// four of those six rows is a worse glance. An open log is being
    /// read, and the end of a sentence that has been cut off is not
    /// reachable by any means -- there is nothing to scroll sideways.
    fn rows(&self, width: f32, open: bool, now: Instant) -> Vec<Row> {
        let mut out = Vec::new();
        let columns = ((width - if open { stamp_width() } else { 0.0 })
            / widgets::measure("x", SCALE).max(1e-4)) as usize;
        for line in self.lines.iter().rev() {
            let alpha = self.opacity(line, now);
            if alpha <= 0.0 {
                continue;
            }
            let colour = line.kind.colour();
            if !open {
                out.push(Row {
                    text: widgets::fit(&line.text, SCALE, width),
                    stamped: false,
                    stamp: line.stamp,
                    colour,
                    alpha,
                });
                continue;
            }
            let wrapped = widgets::wrap(&line.text, columns.max(8));
            // Backwards, because the caller draws upward: the last
            // wrapped row of a line is the one nearest the bottom of
            // the screen, and only the first carries the clock.
            for (index, text) in wrapped.iter().enumerate().rev() {
                out.push(Row {
                    text: text.clone(),
                    stamped: index == 0,
                    stamp: line.stamp,
                    colour,
                    alpha,
                });
            }
        }
        out
    }

    /// The same widget, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(
        &self,
        font: FontAtlas,
        aspect: f32,
        touch: bool,
        language: Language,
        now: Instant,
        out: &mut Vec<HotbarVertex>,
    ) {
        let mut p = Painter::onto(font, std::mem::take(out));
        let left = -aspect.max(0.1) + 0.03;
        let width = (aspect.max(0.1) * 2.0 - 0.06).min(1.6);
        let open = self.is_typing();
        let plate = if open { PLATE_OPEN } else { PLATE };

        // The hint first, because what is left after it is the log. It
        // stands between the log and the box -- beside the slash that
        // summoned it -- rather than in the middle of the screen, which
        // is where a completion popup would put itself and where nobody
        // typing at the bottom-left is looking.
        // While a cairn is being named, the hint is the question -- the
        // one thing a box that opened by itself has to say.
        let hint = if self.naming().is_some() {
            vec![language.text(Msg::CairnNamePrompt)]
        } else if open {
            self.command_hint()
        } else {
            Vec::new()
        };
        let mut y = LOG_BOTTOM;
        for line in hint.iter().rev() {
            let text = widgets::fit(line, SCALE, width - 0.02);
            let ink = widgets::ink_width(&text, SCALE);
            p.quad(
                Rect::new(left - 0.008, y - 0.006, left + ink + 0.010, y + LINE_HEIGHT - 0.008),
                plate,
            );
            p.text(&text, left, y + widgets::cell_height(SCALE) - 0.004, SCALE, QUIET);
            y += LINE_HEIGHT;
        }

        let rows = self.rows(width - 0.02, open, now);
        let room = if open { VISIBLE_OPEN } else { VISIBLE_CLOSED }.saturating_sub(hint.len());
        // Two of the rows go to the markers when there is something the
        // other side of them, and they are counted before the view is
        // cut so that what is drawn always adds up to `room`.
        let mut body = room;
        let first = self.scroll.min(rows.len().saturating_sub(1));
        let more_below = first > 0;
        if more_below {
            body = body.saturating_sub(1);
        }
        let more_above = rows.len() > first + body;
        if more_above {
            body = body.saturating_sub(1);
        }
        // What the wheel is allowed to do next frame. Written here
        // because here is the only place the numbers exist -- see the
        // field.
        self.max_scroll.set(rows.len().saturating_sub(room.max(1)));

        let indent = if open { stamp_width() } else { 0.0 };
        let row_at = |p: &mut Painter, y: f32, text: &str, colour: [f32; 4], alpha: f32| {
            let ink = widgets::ink_width(text, SCALE);
            p.quad(
                Rect::new(
                    left - 0.008,
                    y - 0.006,
                    left + indent + ink + 0.010,
                    y + LINE_HEIGHT - 0.008,
                ),
                dim(plate, alpha),
            );
            p.text(
                text,
                left + indent,
                y + widgets::cell_height(SCALE) - 0.004,
                SCALE,
                dim(colour, alpha),
            );
        };

        if more_below {
            // How much log is between here and the newest line. A
            // player who has scrolled up needs to know there is
            // something under them, or a quiet conversation looks like
            // a stopped one.
            let text = format!("v {first}");
            row_at(&mut p, y, &text, QUIET, 1.0);
            y += LINE_HEIGHT;
        }

        for row in rows.iter().skip(first).take(body) {
            if row.stamped && open {
                p.text(
                    &clock(row.stamp),
                    left,
                    y + widgets::cell_height(SCALE) - 0.004,
                    SCALE,
                    dim(QUIET, row.alpha),
                );
            }
            row_at(&mut p, y, &row.text, row.colour, row.alpha);
            y += LINE_HEIGHT;
        }

        if more_above {
            let text = format!("^ {}", rows.len() - first - body);
            row_at(&mut p, y, &text, QUIET, 1.0);
        }

        if let Some(input) = self.input.as_ref() {
            let (box_rect, buttons) = input_row(aspect, touch);
            p.quad(box_rect, INPUT_PLATE);
            p.border(box_rect, 0.002, INPUT_EDGE);

            // The two buttons, if there is nothing else to send with.
            if let Some((send, leave)) = buttons {
                for (rect, label, colour) in
                    [(send, ">", SEND_EDGE), (leave, "X", INPUT_EDGE)]
                {
                    p.quad(rect, INPUT_PLATE);
                    p.border(rect, 0.003, colour);
                    // Sized to the box rather than by a constant: one
                    // glyph in a square that is a finger across, and the
                    // square is a finger across on every phone and not
                    // the same number of units on any two of them.
                    let scale = widgets::scale_to_fit(rect, label, 0.55);
                    p.label_in(rect, label, scale, colour);
                }
            }

            // The tail of a long line, not the head: what you are typing
            // is at the end, and watching the caret disappear off the
            // edge of the box is the worst version of this.
            //
            // Measured against the box the text is actually in, which on
            // a phone is shorter than the row by two buttons: text sized
            // against the row would be typed straight underneath the
            // button that sends it.
            let left = box_rect.x0 + 0.010;
            // **The window follows the caret**, which is what
            // `widgets::window` is for. Showing the tail of the line
            // unconditionally -- which is what this did while the caret
            // was always at the end -- hides both the caret and the
            // character about to be deleted the moment somebody walks
            // back into a long line.
            let text = input.text();
            let usable = box_rect.width() - 0.030;
            let (from, to) = widgets::window(text, input.caret(), SCALE, usable);
            let shown_text = &text[from..to];
            // The selection goes behind the writing rather than
            // inverting it: this font is one bitmap, and a knocked-out
            // glyph is a hole rather than a letter.
            if let Some((low, high)) = input.selection() {
                let low = low.clamp(from, to);
                let high = high.clamp(from, to);
                if high > low {
                    let x0 = left + widgets::measure(&text[from..low], SCALE);
                    let x1 = left + widgets::measure(&text[from..high], SCALE);
                    p.quad(
                        Rect::new(x0, INPUT_Y - 0.004, x1, INPUT_Y + widgets::cell_height(SCALE)),
                        SELECTION,
                    );
                }
            }
            p.text(
                shown_text,
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
                let at = input.caret().clamp(from, to);
                let x = left + widgets::measure(&text[from..at], SCALE);
                p.quad(
                    Rect::new(
                        x + 0.002,
                        INPUT_Y - 0.002,
                        x + 0.010,
                        INPUT_Y + widgets::cell_height(SCALE),
                    ),
                    widgets::TEXT,
                );
            }
        } else if self.unread() > 0 {
            // The badge stands where the box would be, which is empty
            // whenever the box is shut. **It is the one part of this
            // widget that outlives the fade**: a line is gone from the
            // glass twelve seconds after it arrives whether or not
            // anybody was looking, and without this the whole of a
            // conversation could happen behind a player's back with
            // nothing left to say it had.
            let text = format!("{}: {}", language.text(Msg::ChatUnread), self.unread());
            let ink = widgets::ink_width(&text, SCALE);
            p.quad(
                Rect::new(
                    left - 0.008,
                    INPUT_Y - 0.006,
                    left + ink + 0.010,
                    INPUT_Y + LINE_HEIGHT - 0.008,
                ),
                INPUT_PLATE,
            );
            p.text(
                &text,
                left,
                INPUT_Y + widgets::cell_height(SCALE) - 0.004,
                SCALE,
                MINE,
            );
        }

        *out = p.into_vertices();
    }
}

/// The world's clock, as a chat stamp reads it.
fn clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// How much of a row the clock takes, including the space after it.
///
/// One function called by both the wrapping and the drawing, for the
/// reason `input_row` is one: a wrap width that disagreed with the
/// indent would put the last character of every long line off the end
/// of the plate behind it.
fn stamp_width() -> f32 {
    widgets::measure("00:00 ", SCALE)
}

/// What a finger on the chat box asked for.
///
/// ## Why this exists at all
///
/// **There is no Enter key on a phone.** Sending a line hung on
/// `KeyCode::Enter` and leaving without sending hung on `Escape`, and
/// a player with a chat button on the glass could open the box, type
/// into it, and then do neither -- the same shape of hole the username
/// field had. An on-screen keyboard may send Enter and may not: its
/// action key is often "Done" and often sends nothing a game can read,
/// so a control scheme that depends on it is a control scheme that
/// works on some phones.
///
/// So the box carries its own two answers, on the box, where the line
/// being typed is: a `>` to send and an `X` to leave. Both are drawn
/// only where there is no keyboard -- see [`input_row`] -- because two
/// buttons nobody presses are two pieces of the chat log covered up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tap {
    /// Send what has been typed and close the box.
    Send,
    /// Close the box and throw the line away.
    Leave,
    /// Put the caret at this byte offset in the line being typed.
    ///
    /// **The thing a person tries first on any text they can see.** The
    /// box answered a tap on its two buttons and nothing else, so
    /// pointing at a typo in the middle of a line did nothing at all --
    /// which reads as a broken box rather than as a missing feature.
    Caret(usize),
}

/// A brighter edge for the one button that does something.
///
/// The pair is deliberately not the same colour: they sit side by side,
/// they are one glyph each, and a player about to send a message should
/// not have to read a `>` against an `X` to find out which is which.
const SEND_EDGE: [f32; 4] = [0.55, 0.85, 0.60, 1.0];

/// Where the line being typed goes, and where the two buttons go.
///
/// **One function for the drawing and the pressing.** The rule in
/// CLAUDE.md -- anything that hit-tests must be the exact inverse of
/// what draws it -- is kept here by there being nothing to invert: both
/// sides ask this for the same rectangles.
///
/// The buttons are floored at [`widgets::FINGER_SIDE`] through
/// [`widgets::tappable_when`], which is the game's one answer to "how
/// big is a finger". The input box itself is 0.046 tall, which is a
/// third of that: buttons the height of the box would have been about
/// four millimetres on the phone this was reported from. They are taken
/// out of the row's own width rather than added to it, so the widget
/// still occupies exactly [`EXTENT`] and `fit_from_corner` still knows
/// how big it is allowed to be drawn.
pub fn input_row(aspect: f32, touch: bool) -> (Rect, Option<(Rect, Rect)>) {
    let left = -aspect.max(0.1) + 0.03;
    let width = (aspect.max(0.1) * 2.0 - 0.06).min(1.6);
    let row = Rect::new(
        left - 0.010,
        INPUT_Y - INPUT_DROP,
        left + width,
        INPUT_Y - INPUT_DROP + INPUT_HEIGHT,
    );
    if !touch {
        return (row, None);
    }
    let side = widgets::tappable_when(row.height(), true);
    let gap = 0.010;
    let middle = row.centre_y();
    // Send on the outside, where a thumb reaches first and where a
    // mis-aim goes off the end of the row instead of onto the other
    // button. Leaving is the one that loses what was typed, so it is
    // the one that is harder to hit by accident.
    let send = Rect::centred(row.x1 - side / 2.0, middle, side, side);
    let leave = Rect::centred(send.x0 - gap - side / 2.0, middle, side, side);
    let typed = Rect::new(row.x0, row.y0, leave.x0 - gap, row.y1);
    (typed, Some((send, leave)))
}

fn dim(colour: [f32; 4], alpha: f32) -> [f32; 4] {
    [colour[0], colour[1], colour[2], colour[3] * alpha]
}


#[cfg(test)]
mod tests {
    /// The screen the phone this was reported from has, held sideways.
    const PHONE_ASPECT: f32 = 2712.0 / 1220.0;

    /// Nothing a phone is typing into is under the keyboard typing
    /// into it -- at any interface size.
    ///
    /// **The player's report, in their words: "ввод всё ещё не
    /// работает... клавиатуре некуда встать, если поле ввода находится
    /// низко".** The chat box is pinned to the bottom-left corner
    /// because the log it belongs to is, and on a desktop that is the
    /// corner nothing wants. On a phone it is the half of the glass a
    /// keyboard stands on.
    ///
    /// Run through the whole pipeline the frame loop runs -- author,
    /// grow about the corner, lift -- rather than against the authored
    /// rectangles, because **the growth is what defeated the first fix**.
    /// A box simply authored higher is pulled back toward the corner by
    /// any scale under one, and the interface size is a setting: the
    /// first version of this passed at 1.0 and put the box back under
    /// the keyboard at 0.6.
    ///
    /// Checked against every vertex, not the box's own rectangle: the
    /// two buttons are floored to a finger and hang below the row.
    #[test]
    fn a_phone_never_types_into_a_box_the_keyboard_is_standing_on() {
        for step in 0..=14 {
            let scale = 0.5 + step as f32 * 0.1;
            let mut chat = Chat::new();
            chat.note("кто-то что-то сказал", Instant::now());
            chat.open(Instant::now());
            for c in "привет".chars() {
                chat.type_char(c);
            }

            let mut vertices = Vec::new();
            chat.build_into(
                FontAtlas::for_test(),
                PHONE_ASPECT,
                true,
                Language::English,
                Instant::now(),
                &mut vertices,
            );
            assert!(!vertices.is_empty(), "the box drew nothing to check");
            widgets::scale_about(
                &mut vertices,
                widgets::anchor::BOTTOM_LEFT(PHONE_ASPECT),
                scale,
            );
            widgets::lift(&mut vertices, keyboard_lift(true, true, scale));

            let lowest = vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::INFINITY, f32::min);
            assert!(
                lowest >= widgets::KEYBOARD_TOP - 1e-5,
                "at an interface scale of {scale} the box reaches down to {lowest},                  and the keyboard starts at {}",
                widgets::KEYBOARD_TOP,
            );
        }
    }

    /// A finger still lands on the button it is pointing at after the
    /// box has been lifted.
    ///
    /// **The rule in CLAUDE.md, at the one place this change could have
    /// broken it.** The lift goes on after the growth, so the inverse
    /// has to come off before it -- and getting that order wrong misses
    /// by the lift times the scale, which is most of a button. Both
    /// sides call `keyboard_lift`, and this is what says so.
    #[test]
    fn the_send_button_is_pressed_where_the_lifted_box_draws_it() {
        for step in 0..=14 {
            let scale = 0.5 + step as f32 * 0.1;
            let mut chat = Chat::new();
            chat.open(Instant::now());
            chat.type_char('o');

            let (_, buttons) = input_row(PHONE_ASPECT, true);
            let (send, _) = buttons.expect("a touch device gets buttons");
            // Where that button ends up on the glass.
            let corner = widgets::anchor::BOTTOM_LEFT(PHONE_ASPECT);
            let lift = keyboard_lift(true, true, scale);
            let on_glass = (
                corner.0 + (send.centre_x() - corner.0) * scale,
                corner.1 + (send.centre_y() - corner.1) * scale + lift,
            );
            // ...and back again, the way the frame loop does it.
            let authored = widgets::unscale_about(
                (on_glass.0, on_glass.1 - lift),
                corner,
                scale,
            );
            assert_eq!(
                chat.tapped(PHONE_ASPECT, true, authored),
                Some(Tap::Send),
                "at an interface scale of {scale} the send button was not where it is drawn",
            );
        }
    }

    /// ...and a desktop, which has no keyboard, keeps its chat in the
    /// corner it has always been in.
    ///
    /// The lift is for the one case that needs it. A box that floated
    /// into the middle of the screen on a machine with a real keyboard
    /// would be a fix for a phone charged to everybody else.
    #[test]
    fn a_desktop_chat_box_does_not_move_out_of_its_corner() {
        assert_eq!(keyboard_lift(false, true, 1.0), 0.0);
        assert_eq!(keyboard_lift(false, true, 0.6), 0.0);
        // ...and neither does a phone with the box shut.
        assert_eq!(keyboard_lift(true, false, 0.6), 0.0);
        let (row, _) = input_row(PHONE_ASPECT, false);
        assert!(
            (row.y0 - (INPUT_Y - INPUT_DROP)).abs() < 1e-6,
            "the desktop box moved to {}",
            row.y0,
        );
    }

    /// A player with no keyboard can both send a line and walk away
    /// from one.
    ///
    /// **The hole this closes**: the chat button on the glass opened a
    /// box that could be typed into and neither sent nor abandoned.
    /// Sending was `Enter` and leaving was `Escape`, and a phone has
    /// neither -- an input method's action key is usually "Done" and
    /// frequently sends nothing a game can read at all. The player's
    /// own words were "there is nothing to send messages with".
    #[test]
    fn a_player_with_no_keyboard_can_both_send_a_line_and_leave_without_sending() {
        let mut chat = Chat::new();
        chat.open(Instant::now());
        chat.type_char('o');

        let (_, buttons) = input_row(PHONE_ASPECT, true);
        let (send, leave) = buttons.expect("a touch device gets buttons on the box");
        assert_eq!(
            chat.tapped(PHONE_ASPECT, true, (send.centre_x(), send.centre_y())),
            Some(Tap::Send),
        );
        assert_eq!(
            chat.tapped(PHONE_ASPECT, true, (leave.centre_x(), leave.centre_y())),
            Some(Tap::Leave),
        );
        // The rest of the row is the line itself, and a tap there is a
        // caret rather than nothing -- see `Tap::Caret`. What is still
        // nothing is a tap that misses the row altogether: the log
        // above it is read, not pressed.
        assert!(matches!(
            chat.tapped(PHONE_ASPECT, true, (-2.0, INPUT_Y)),
            Some(Tap::Caret(_)),
        ));
        assert_eq!(chat.tapped(PHONE_ASPECT, true, (-2.0, LOG_BOTTOM)), None);
    }

    /// The buttons are only there while there is a line to do something
    /// with.
    ///
    /// A control that answers a finger while it is not drawn is the
    /// same fault as a button hit-tested through a shut wheel: the
    /// player presses a piece of the world and something happens.
    /// A tap on the line being typed puts the caret in it.
    ///
    /// The same property the form fields are held to, on the one text
    /// box that is used while the world has the screen: the hit-test
    /// walks the same string at the same size through the same window
    /// as the drawing, so a tap between two letters lands between those
    /// two letters.
    #[test]
    fn a_tap_on_the_typed_line_puts_the_caret_where_it_was_aimed() {
        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, PHONE_ASPECT] {
            for touch in [false, true] {
                let mut chat = Chat::new();
                chat.open(now());
                for c in "привет".chars() {
                    chat.type_char(c);
                }
                let (typed, _) = input_row(aspect, touch);
                let left = typed.x0 + 0.010;
                let usable = typed.width() - 0.030;
                let text = chat.typed_text().to_string();
                let (from, to) = widgets::window(&text, text.len(), SCALE, usable);
                assert_eq!((from, to), (0, text.len()), "the sample line did not fit");

                for (index, c) in text.char_indices() {
                    let end = index + c.len_utf8();
                    let x0 = left + widgets::measure(&text[..index], SCALE);
                    let x1 = left + widgets::measure(&text[..end], SCALE);
                    let y = typed.centre_y();
                    assert_eq!(
                        chat.tapped(aspect, touch, (x0 + (x1 - x0) * 0.2, y)),
                        Some(Tap::Caret(index)),
                        "at aspect {aspect} a tap left of {c:?} missed",
                    );
                    assert_eq!(
                        chat.tapped(aspect, touch, (x0 + (x1 - x0) * 0.8, y)),
                        Some(Tap::Caret(end)),
                        "at aspect {aspect} a tap right of {c:?} missed",
                    );
                }
            }
        }
    }

    /// ...and the caret that lands there is where the next character
    /// goes.
    #[test]
    fn a_tapped_caret_is_where_the_next_character_is_typed() {
        let mut chat = Chat::new();
        chat.open(now());
        for c in "helo".chars() {
            chat.type_char(c);
        }
        let (typed, _) = input_row(1.78, false);
        let left = typed.x0 + 0.010;
        // Between the second and third letters.
        let x = left + widgets::measure("hel", SCALE) - widgets::measure("l", SCALE) * 0.2;
        let Some(Tap::Caret(at)) = chat.tapped(1.78, false, (x, typed.centre_y())) else {
            panic!("a tap on the line did not answer with a caret");
        };
        chat.place_caret(at);
        chat.type_char('l');
        assert_eq!(chat.typed_text(), "hello");
    }

    /// The buttons still win where they overlap the line.
    ///
    /// They are drawn on top of the row, so a finger on one has to
    /// reach the button rather than the text under it -- what is drawn
    /// on top is what is pressed.
    #[test]
    fn the_send_button_still_beats_the_line_it_is_drawn_over() {
        let mut chat = Chat::new();
        chat.open(now());
        chat.type_char('o');
        let (_, buttons) = input_row(PHONE_ASPECT, true);
        let (send, leave) = buttons.expect("a touch device gets buttons");
        assert_eq!(
            chat.tapped(PHONE_ASPECT, true, (send.centre_x(), send.centre_y())),
            Some(Tap::Send),
        );
        assert_eq!(
            chat.tapped(PHONE_ASPECT, true, (leave.centre_x(), leave.centre_y())),
            Some(Tap::Leave),
        );
    }

    #[test]
    fn a_shut_chat_box_has_no_buttons_to_press() {
        let chat = Chat::new();
        assert!(!chat.is_typing());
        let (_, buttons) = input_row(PHONE_ASPECT, true);
        let (send, _) = buttons.expect("the rectangles exist even when nothing is typed");
        assert_eq!(
            chat.tapped(PHONE_ASPECT, true, (send.centre_x(), send.centre_y())),
            None,
            "a closed box answered a finger",
        );
    }

    /// ...and they are not there at all where there is a keyboard.
    ///
    /// Two buttons nobody presses are two pieces of the chat log
    /// covered up, and a player with Enter under their finger presses
    /// Enter.
    #[test]
    fn a_desktop_chat_box_keeps_its_whole_width_for_the_line() {
        let (typed, buttons) = input_row(PHONE_ASPECT, false);
        assert!(buttons.is_none(), "a keyboard was given buttons it does not need");
        let (touch_typed, _) = input_row(PHONE_ASPECT, true);
        assert!(
            typed.width() > touch_typed.width(),
            "the buttons cost nothing, so they are not where they are drawn",
        );
    }

    /// The line being typed never runs underneath the button that sends
    /// it.
    ///
    /// The buttons are taken out of the row's width rather than added
    /// to it -- so the widget still occupies exactly `EXTENT` -- and
    /// the text has to be measured against what is left, not against
    /// the row. Measured against the row, the end of a long line is
    /// drawn under the `>` and the caret with it, which is exactly the
    /// thing `tail_that_fits` exists to prevent at the other end.
    #[test]
    fn the_line_being_typed_never_runs_under_the_button_that_sends_it() {
        for aspect in [1.33f32, 16.0 / 9.0, PHONE_ASPECT, 3.0] {
            let (typed, buttons) = input_row(aspect, true);
            let (send, leave) = buttons.expect("a touch device gets buttons");
            assert!(
                typed.x1 <= leave.x0,
                "at aspect {aspect} the text box reaches {} and the buttons start at {}",
                typed.x1,
                leave.x0,
            );
            assert!(leave.x1 <= send.x0, "the two buttons overlap at aspect {aspect}");
        }
    }

    /// Both buttons are at least a finger across.
    ///
    /// **The one measure of a finger this game has**, and it is a
    /// fraction of the screen rather than a count of pixels -- see
    /// `widgets::FINGER`. The input box is 0.046 tall, a third of that:
    /// buttons the height of the box would have been about four
    /// millimetres on the phone this was reported from, which is a
    /// button that is drawn and cannot be pressed.
    #[test]
    fn the_buttons_on_the_chat_box_are_at_least_a_finger_across() {
        for aspect in [1.33f32, 16.0 / 9.0, PHONE_ASPECT, 3.0] {
            let (_, buttons) = input_row(aspect, true);
            let (send, leave) = buttons.expect("a touch device gets buttons");
            for (name, rect) in [("send", send), ("leave", leave)] {
                assert!(
                    rect.width() >= widgets::FINGER_SIDE - 1e-6
                        && rect.height() >= widgets::FINGER_SIDE - 1e-6,
                    "the {name} button is {}x{} at aspect {aspect}, under a finger",
                    rect.width(),
                    rect.height(),
                );
            }
        }
    }

    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn naming_a_cairn_is_not_a_line_said_and_an_empty_name_is_still_an_answer() {
        let mut chat = Chat::new();
        chat.open_naming((4, 70, -9), "old", now());
        assert_eq!(chat.naming(), Some((4, 70, -9)));
        assert_eq!(chat.typed_text(), "old", "the box did not hold the name the cairn has");
        chat.set_typed_text("  the ford ");
        assert_eq!(chat.submit(), Some("the ford".to_string()));
        assert_eq!(chat.naming(), None);
        // Up does not bring a cairn's name back as a line to say.
        chat.open(now());
        chat.recall(1);
        assert_eq!(chat.typed_text(), "");
        chat.close();
        // Emptied and sent: the answer is "no name", not "nothing happened".
        chat.open_naming((0, 0, 0), "x", now());
        chat.set_typed_text("");
        assert_eq!(chat.submit(), Some(String::new()));
        // ...and leaving the box is leaving the name as it was.
        chat.open_naming((0, 0, 0), "x", now());
        chat.close();
        assert_eq!(chat.naming(), None);
    }

    #[test]
    fn a_closed_box_refuses_a_line_an_input_method_offers() {
        // An Android input method is asked for its text once a frame
        // and answers whatever it last held, including after the box
        // has been closed. Taking that would reopen a chat line the
        // player had already sent or abandoned -- and `is_typing` is
        // what the rest of the game reads to decide who owns the
        // keyboard, so the box would be filled and invisible.
        let mut chat = Chat::new();
        chat.set_typed_text("hello");
        assert!(!chat.is_typing());
        assert_eq!(chat.typed_text(), "");
    }

    #[test]
    fn a_line_handed_in_by_an_input_method_obeys_the_rules_typing_does() {
        // Same argument as `Menu::set_focused_text`: the glyph filter
        // and the length cap live in `type_char`, and a phone is where
        // an emoji keyboard is one tap away from the chat box.
        let mut chat = Chat::new();
        chat.open(now());
        chat.set_typed_text("привет 🙂");
        assert_eq!(chat.typed_text(), "привет ");
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
        chat.edit(Edit::Backspace { word: false });
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
            chat.said("someone", false, &format!("line {i}"), now());
        }
        assert_eq!(chat.lines.len(), HISTORY);
        assert!(chat.lines.back().unwrap().text.contains(&format!("line {}", HISTORY + 19)));
    }

    #[test]
    fn the_server_and_a_player_do_not_look_alike() {
        // A player called "server" must not be able to pass for one.
        let mut chat = Chat::new();
        chat.note("the world is saving", now());
        chat.said("server", false, "give me your things", now());
        let system = &chat.lines[0];
        let player = &chat.lines[1];
        assert_eq!(system.kind, Kind::System);
        assert_eq!(player.kind, Kind::Said);
        assert!(player.text.starts_with("<server>"), "{}", player.text);
    }

    #[test]
    fn old_lines_fade_out_when_the_box_is_closed_and_not_while_it_is_open() {
        let mut chat = Chat::new();
        let long_ago = Instant::now() - FADE_AFTER - Duration::from_secs(5);
        chat.said("a", false, "old news", long_ago);
        let stale = Line {
            text: chat.lines[0].text.clone(),
            at: long_ago,
            kind: Kind::Said,
            stamp: 0,
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
    /// cargo test -p primitive_client --lib -- --ignored dump_the_chat
    /// ```
    #[test]
    #[ignore = "diagnostic: writes a picture of the chat window"]
    fn dump_the_chat_to_a_png() {
        const WIDTH: u32 = 1600;
        const HEIGHT: u32 = 900;

        let mut chat = Chat::new();
        let now = Instant::now();
        // Every kind of line, with the world's clock moving under them.
        // This is the half of the widget that cannot be checked any other
        // way: whether a reply, somebody talking and the player's own line
        // are actually told apart at a glance, or whether the colours have
        // quietly become one.
        chat.set_world_time(0.32);
        chat.note("connected to grim.example.net", now);
        chat.said("Shamkhan", false, "found a cave under the ridge", now);
        chat.set_world_time(0.55);
        chat.said("Ivan", false, "bring planks, mine went in the river", now);
        chat.note("Ivan fell from a great height", now);
        chat.set_world_time(0.71);
        chat.said("Shamkhan", true, "/give stone 4", now);
        chat.note("gave you 4 stone", now);
        chat.said("Shamkhan", true, "/players", now);
        chat.note("2 known player(s):", now);
        if std::env::var("PRIMITIVE_CHAT_OPEN").is_ok() {
            chat.open(Instant::now());
            // `PRIMITIVE_CHAT_LINE=/t` is how the command hint is
            // looked at: it only appears under a half-typed slash.
            for c in std::env::var("PRIMITIVE_CHAT_LINE")
                .unwrap_or_else(|_| "on my way with the planks".to_string())
                .chars()
            {
                chat.type_char(c);
            }
        }

        // **As a phone draws it**, which is the version with the send
        // and leave buttons on the box: they are the half of this
        // widget that cannot be checked any other way without a phone
        // in your hand, and this dump is the way it is looked at.
        let mut vertices = Vec::new();
        chat.build_into(
            FontAtlas::for_test(),
            WIDTH as f32 / HEIGHT as f32,
            true,
            Language::English,
            now,
            &mut vertices,
        );
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
        chat.said("a", false, "hello", at);
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
        chat.said("a", false, "hello", now);
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

        chat.said("a", false, "hello", Instant::now());
        assert!(chat.has_anything_to_draw(Instant::now()));

        // ...and once the last line has faded, back to nothing.
        //
        // **A note the server made, not something a person said.** A
        // faded line somebody spoke leaves the unread badge behind and
        // is therefore still worth drawing -- see the test below --
        // which is a deliberate exception to "a quiet game pays
        // nothing" rather than a hole in it: a game with a badge up is
        // not a quiet one.
        let mut stale = Chat::new();
        let long_ago = Instant::now() - FADE_AFTER - Duration::from_secs(5);
        stale.note("the world was saved", long_ago);
        assert!(!stale.has_anything_to_draw(Instant::now()));
        assert!(stale.build(FontAtlas::for_test(), 1.78, Instant::now()).is_empty());

        // Except with the box open, where the log is what is being read.
        stale.open(Instant::now());
        assert!(stale.has_anything_to_draw(Instant::now()));
    }

    /// Everything that has been said can be read, not just the last
    /// twelve lines of it.
    ///
    /// **The hole this closes.** The log kept its history and drew a
    /// window onto the end of it, and there was no way to move the
    /// window: the wheel changed what the player was holding, the
    /// arrows did nothing, and a conversation that went past twelve
    /// lines was gone. Two hundred lines with twelve reachable is not a
    /// history, it is twelve lines and a leak.
    #[test]
    fn a_conversation_longer_than_the_window_can_still_be_read() {
        let mut chat = Chat::new();
        let now = now();
        for i in 0..40 {
            chat.said("someone", false, &format!("line {i}"), now);
        }
        chat.open(now);
        let mut out = Vec::new();
        chat.build_into(FontAtlas::for_test(), 1.78, false, Language::English, now, &mut out);

        let bottom = fingerprint(&chat, now);
        chat.scroll_by(5);
        assert_eq!(chat.scroll, 5, "the wheel did not move the log");
        assert_ne!(bottom, fingerprint(&chat, now), "the log was scrolled and did not move");

        // ...and it stops at its own oldest row rather than scrolling
        // into blank space above it, which is the failure every list in
        // this game is held to not having.
        for _ in 0..500 {
            chat.scroll_by(1);
        }
        let far = chat.scroll;
        assert!(far > 0 && far < 500, "the log overscrolled to {far}");
        for _ in 0..500 {
            chat.scroll_by(-1);
        }
        assert_eq!(chat.scroll, 0, "the log would not come back to the newest line");
    }

    /// A shut log is not scrolled, because out there the wheel is what
    /// changes the block in the player's hand.
    #[test]
    fn a_shut_log_leaves_the_wheel_to_the_hotbar() {
        let mut chat = Chat::new();
        for i in 0..40 {
            chat.said("someone", false, &format!("line {i}"), now());
        }
        chat.scroll_by(3);
        assert_eq!(chat.scroll, 0, "a shut log took the wheel off the hotbar");
    }

    /// A line arriving while the log is scrolled up does not slide it.
    ///
    /// The player is reading something. A log that jumps a row every
    /// time anybody speaks is a log you cannot finish a sentence in --
    /// and in a fight, which is when the log fills fastest, it would be
    /// unreadable exactly when it mattered.
    #[test]
    fn a_new_line_does_not_move_the_log_out_from_under_a_reader() {
        let mut chat = Chat::new();
        let now = now();
        for i in 0..40 {
            chat.said("someone", false, &format!("line {i}"), now);
        }
        chat.open(now);
        let mut out = Vec::new();
        chat.build_into(FontAtlas::for_test(), 1.78, false, Language::English, now, &mut out);
        chat.scroll_by(4);

        // The offset is kept in rows *from the bottom*, so holding the
        // view still means moving it: one more row arrived underneath.
        // Nothing in this log wraps, so a row is a line and the index
        // of the line at the bottom of the window is the arithmetic
        // below.
        let looking_at = chat.lines.len() - 1 - chat.scroll;
        chat.said("someone", false, "and another thing", now);
        assert_eq!(
            chat.lines.len() - 1 - chat.scroll,
            looking_at,
            "the line the reader was looking at moved when somebody spoke",
        );
    }

    /// The three kinds of line are three different colours.
    ///
    /// There were four: a refusal had a red of its own. Refusals left the log
    /// for the HUD's banner, because a log of one's own mistakes buried the
    /// conversation, and what a player reads back here is people and
    /// the server's replies -- which still must not look like each other.
    #[test]
    fn the_kinds_of_line_are_told_apart_by_their_ink() {
        assert_ne!(Kind::System.colour(), Kind::Said.colour());
        assert_ne!(Kind::Mine.colour(), Kind::Said.colour());
        assert_ne!(Kind::Mine.colour(), Kind::System.colour());

        let now = now();
        let mut reply = Chat::new();
        reply.note("gave you 4 stone", now);
        let mut spoken = Chat::new();
        spoken.said("Shamkhan", false, "gave you 4 stone", now);
        assert_ne!(
            fingerprint_colours(&reply, now),
            fingerprint_colours(&spoken, now),
            "the server's reply is drawn in the same ink as a person",
        );
    }

    /// A player's own line is told apart from everybody else's by the
    /// id on the wire, not by the name printed in it.
    #[test]
    fn two_players_with_one_name_do_not_share_a_colour() {
        let mut chat = Chat::new();
        let now = now();
        chat.said("Ivan", true, "mine", now);
        chat.said("Ivan", false, "not mine", now);
        assert_eq!(chat.lines[0].kind, Kind::Mine);
        assert_eq!(chat.lines[1].kind, Kind::Said);
    }

    /// Things said while nobody was reading are counted, and the count
    /// outlives the fade.
    ///
    /// A line is off the glass twelve seconds after it arrives whether
    /// or not anybody saw it. Without a badge the whole of a
    /// conversation can happen behind a player's back with nothing left
    /// on screen to say that it did.
    #[test]
    fn what_was_said_while_nobody_was_looking_is_counted() {
        let mut chat = Chat::new();
        let long_ago = Instant::now() - FADE_AFTER - Duration::from_secs(5);
        chat.said("someone", false, "are you there", long_ago);
        chat.said("someone", false, "hello?", long_ago);
        assert_eq!(chat.unread(), 2);
        // Faded out entirely, and still worth drawing -- the badge is
        // the only thing left saying it happened.
        assert!(chat.has_anything_to_draw(Instant::now()));
        assert!(!chat.build(FontAtlas::for_test(), 1.78, Instant::now()).is_empty());

        chat.open(Instant::now());
        assert_eq!(chat.unread(), 0, "opening the box did not count as reading it");
    }

    /// The server's own notices are not counted as unread.
    ///
    /// Counting them would put a permanent badge on every singleplayer
    /// world from the first second: the only thing that ever speaks
    /// there is the server saying somebody joined, and a badge that is
    /// always on is a badge nobody looks at.
    #[test]
    fn a_singleplayer_world_never_wears_an_unread_badge() {
        let mut chat = Chat::new();
        let now = now();
        chat.note("player joined", now);
        chat.note("the world was saved", now);
        assert_eq!(chat.unread(), 0);
    }

    /// A line is stamped with the world's clock at the moment it
    /// arrives, not at the moment it is drawn.
    ///
    /// Drawn-time would stamp the whole log with whatever time it is
    /// now, which is a timestamp that is right once and wrong for every
    /// line above it.
    #[test]
    fn a_line_carries_the_time_it_was_said_and_not_the_time_it_is_read() {
        let mut chat = Chat::new();
        chat.set_world_time(0.25); // six in the morning
        chat.said("someone", false, "morning", now());
        chat.set_world_time(0.75); // six in the evening
        chat.said("someone", false, "evening", now());
        assert_eq!(clock(chat.lines[0].stamp), "06:00");
        assert_eq!(clock(chat.lines[1].stamp), "18:00");
        // ...and nothing a clock can be handed produces a time that is
        // not a time.
        for t in [-3.5f32, 0.0, 0.999_999, 1.0, 7.25, f32::NAN, f32::INFINITY] {
            chat.set_world_time(t);
            let text = clock(chat.world_minutes);
            assert_eq!(text.len(), 5, "time_of_day {t} produced {text:?}");
            assert!(chat.world_minutes < 24 * 60, "time_of_day {t} left the day");
        }
    }

    /// Typing a slash offers the commands, and the list is the server's
    /// own.
    ///
    /// A copy of the command names here would be a second table with
    /// nothing keeping it honest -- and the first command anybody added
    /// would be missing from exactly one of the two. See
    /// `Chat::command_hint`.
    #[test]
    fn a_slash_offers_the_commands_the_server_actually_has() {
        let mut chat = Chat::new();
        chat.open(now());
        assert!(chat.command_hint().is_empty(), "an empty box offered commands");

        chat.type_char('/');
        assert!(!chat.command_hint().is_empty(), "a slash offered nothing");
        assert!(chat.command_hint().len() <= HINT_ROWS);

        for c in "ti".chars() {
            chat.type_char(c);
        }
        let narrowed = chat.command_hint();
        assert!(
            narrowed.iter().all(|line| line.starts_with("/time")),
            "typing /ti offered {narrowed:?}",
        );

        // Once there is an argument the hint is the usage line for the
        // one command being typed, not a list to choose from.
        chat.type_char('m');
        chat.type_char('e');
        chat.type_char(' ');
        assert!(
            chat.command_hint().iter().all(|line| line.starts_with("/time")),
            "an argument widened the hint again",
        );

        // ...and ordinary talk is not a command.
        let mut talking = Chat::new();
        talking.open(now());
        for c in "hello there".chars() {
            talking.type_char(c);
        }
        assert!(talking.command_hint().is_empty());
    }

    /// The hint never costs more of the log than it is worth, and never
    /// pushes anything off the bottom of the widget.
    #[test]
    fn the_command_hint_takes_its_rows_out_of_the_log_and_not_the_screen() {
        let mut plain = Chat::new();
        let mut hinting = Chat::new();
        let now = now();
        for i in 0..40 {
            plain.said("someone", false, &format!("line {i}"), now);
            hinting.said("someone", false, &format!("line {i}"), now);
        }
        plain.open(now);
        hinting.open(now);
        hinting.type_char('/');

        let bottom = |chat: &Chat| {
            let mut out = Vec::new();
            chat.build_into(FontAtlas::for_test(), 1.78, false, Language::English, now, &mut out);
            let top = out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            let low = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            (low, top)
        };
        let (plain_low, plain_top) = bottom(&plain);
        let (hint_low, hint_top) = bottom(&hinting);
        assert!(
            hint_low >= plain_low - 1e-4 && hint_top <= plain_top + 1e-4,
            "the hint grew the widget: {hint_low}..{hint_top} against {plain_low}..{plain_top}",
        );
    }

    /// The arrow keys bring back what was sent, and stop at both ends.
    #[test]
    fn the_arrows_bring_back_a_command_that_was_mistyped() {
        let mut chat = Chat::new();
        for line in ["/tp 812 74 -1290", "hello", "/time noon"] {
            chat.open(now());
            for c in line.chars() {
                chat.type_char(c);
            }
            assert_eq!(chat.submit().as_deref(), Some(line));
        }

        chat.open(now());
        chat.recall(-1);
        assert_eq!(chat.typed_text(), "/time noon");
        chat.recall(-1);
        assert_eq!(chat.typed_text(), "hello");
        chat.recall(-1);
        assert_eq!(chat.typed_text(), "/tp 812 74 -1290");
        // The front of the list is a wall, not a loop: a player holding
        // Up expects to arrive at the oldest line and stay there.
        chat.recall(-1);
        assert_eq!(chat.typed_text(), "/tp 812 74 -1290");

        chat.recall(1);
        chat.recall(1);
        assert_eq!(chat.typed_text(), "/time noon");
        // ...and walking off the end empties the box, which is how you
        // get a blank line back without pressing Escape and losing the
        // box with it.
        chat.recall(1);
        assert_eq!(chat.typed_text(), "");
    }

    /// Saying the same thing twice running is one entry in the recall,
    /// not two.
    ///
    /// Repeating yourself is common; pressing Up past three copies of
    /// one command is not something anybody wants to do.
    #[test]
    fn the_same_line_twice_is_remembered_once() {
        let mut chat = Chat::new();
        for _ in 0..3 {
            chat.open(now());
            for c in "/list".chars() {
                chat.type_char(c);
            }
            chat.submit();
        }
        assert_eq!(chat.sent.len(), 1);
    }

    /// Typing after recalling a line leaves the recall behind.
    ///
    /// Otherwise the next Up walks on from wherever the player had got
    /// to and silently throws away what they had just written.
    #[test]
    fn editing_a_recalled_line_makes_it_the_players_own() {
        let mut chat = Chat::new();
        chat.open(now());
        for c in "/list".chars() {
            chat.type_char(c);
        }
        chat.submit();
        chat.open(now());
        chat.recall(-1);
        chat.type_char('s');
        assert_eq!(chat.typed_text(), "/lists");
        chat.recall(-1);
        assert_eq!(chat.typed_text(), "/list", "the walk did not start again from the end");
    }

    /// An open log wraps a long line instead of cutting it off.
    ///
    /// A cut sentence in a shut log can be read by opening it. A cut
    /// sentence in an *open* one cannot be read at all -- there is
    /// nothing to scroll sideways.
    #[test]
    fn an_open_log_wraps_a_long_line_and_a_shut_one_does_not() {
        let mut chat = Chat::new();
        let now = now();
        chat.said("someone", false, &"a very long sentence ".repeat(12), now);
        let shut = chat.rows(1.5, false, now);
        assert_eq!(shut.len(), 1, "a shut log wrapped a line it should have cut");

        chat.open(now);
        let open = chat.rows(1.5, true, now);
        assert!(open.len() > 1, "an open log cut a line it should have wrapped");
        // Only the first row of a wrapped line carries the clock, so
        // the timestamps stay a column.
        assert_eq!(open.iter().filter(|row| row.stamped).count(), 1);
        // ...and they are in drawing order, which is bottom-up: the
        // stamped row is the topmost, so it comes last.
        assert!(open.last().is_some_and(|row| row.stamped));
    }

    /// A fingerprint of where the chat draws, for the tests above.
    fn fingerprint(chat: &Chat, now: Instant) -> Vec<u64> {
        let mut out = Vec::new();
        chat.build_into(FontAtlas::for_test(), 1.78, false, Language::English, now, &mut out);
        out.iter()
            .flat_map(|v| {
                [
                    (v.position[0] * 10_000.0) as i64 as u64,
                    (v.position[1] * 10_000.0) as i64 as u64,
                    (v.uv[0] * 10_000.0) as i64 as u64,
                ]
            })
            .collect()
    }

    /// ...and of what colour it draws in.
    fn fingerprint_colours(chat: &Chat, now: Instant) -> Vec<u64> {
        let mut out = Vec::new();
        chat.build_into(FontAtlas::for_test(), 1.78, false, Language::English, now, &mut out);
        out.iter()
            .flat_map(|v| v.tint.map(|c| (c * 10_000.0) as i64 as u64))
            .collect()
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
            chat.said("talker", false, &format!("message number {i} with some length"), now());
        }
        for c in "typing something quite long into the box".chars() {
            chat.type_char(c);
        }
        // **And with the buttons on it.** They are floored at a
        // finger, which is three times the height of the box they sit
        // beside, so they stand proud of it in both directions -- and
        // the hotbar is right underneath.
        for (aspect, touch) in [1.0f32, 1.78, 2.4, PHONE_ASPECT]
            .into_iter()
            .flat_map(|aspect| [(aspect, false), (aspect, true)])
        {
            let mut vertices = Vec::new();
            chat.build_into(
                FontAtlas::for_test(),
                aspect,
                touch,
                Language::English,
                now(),
                &mut vertices,
            );
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
                "chat overlaps the hotbar at aspect {aspect} (touch: {touch})"
            );
        }
    }
}
