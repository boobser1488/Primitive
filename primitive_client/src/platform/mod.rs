//! The line between the game and whatever is holding the window.
//!
//! ## Why this exists
//!
//! The game used to speak winit directly: `winit::keyboard::KeyCode` was
//! the type a keybinding was stored in, `winit::dpi::PhysicalSize` was
//! how the renderer knew how big it was, and the frame loop matched on
//! `winit::event::WindowEvent` in place. None of that is wrong on a
//! desktop, and all of it is a wall the moment the game has to run
//! somewhere winit is not the answer -- an Android activity, a headless
//! harness that replays a recorded session, a test that wants to press
//! a key without a compositor.
//!
//! The types here are the game's own. They name what a player did --
//! a key went down, the window changed size, a finger touched the
//! screen -- and say nothing about who noticed. A backend's whole job is
//! to turn its own events into these; see `winit_backend`, which is the
//! only file in the client that still mentions winit at all.
//!
//! ## Why an enum rather than a trait for keys
//!
//! A key is a value, not a behaviour. It is compared, stored in a
//! settings file, and put in a set -- all of which a trait object is bad
//! at and an enum is free at. The cost is that a backend with a key this
//! enum has never heard of has to drop it, which is the right trade:
//! an unbindable key is better than a binding that silently does
//! nothing, and that was already this game's rule (see `ui::keybinds`).

pub mod touch;
pub mod winit_backend;

/// Getting the game's own files out of the APK, and its working
/// directory somewhere writable. Nothing here exists off a phone.
#[cfg(target_os = "android")]
pub mod android;

/// Which backend this build talks to.
///
/// The seam. `main` names `platform::backend` and nothing else, so
/// bringing up a second backend is this alias plus a file -- not a
/// search through the frame loop for everything that assumed a
/// compositor.
///
/// Today every target routes here, Android included: winit speaks
/// `android-activity` on its own, and a second backend that re-did that
/// work would be a second backend to keep in step for no gain. What
/// Android actually needs is different *behaviour* -- touch instead of
/// a mouse, a surface that goes away when the activity does -- and that
/// lives in `platform::touch` and in the `Suspended`/`Resumed` events,
/// both of which are above this line rather than behind it.
pub use winit_backend as backend;

/// The activity, from the moment `android_main` is handed it.
///
/// A global, and there is no way around it that is not worse. The
/// activity arrives as an argument to `android_main` and is needed
/// again several layers down, when winit builds its event loop -- and
/// the layers in between are the game's own startup, which has no
/// business carrying an Android handle through it just to hand it back.
/// The alternatives were a parameter threaded through every one of
/// those layers, or a field on a struct that exists on one platform.
///
/// Written exactly once, before anything reads it, and never again.
#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<winit::platform::android::activity::AndroidApp> =
    std::sync::OnceLock::new();

/// Remembers the activity for the rest of the process's life.
#[cfg(target_os = "android")]
pub fn set_android_app(app: winit::platform::android::activity::AndroidApp) {
    let _ = ANDROID_APP.set(app);
}

/// The activity, if `android_main` has run.
#[cfg(target_os = "android")]
pub fn android_app() -> Option<winit::platform::android::activity::AndroidApp> {
    ANDROID_APP.get().cloned()
}

/// A physical key position, independent of what is printed on it.
///
/// *Physical*, deliberately: `Forward` is the key where W sits on a
/// QWERTY board, so a player on AZERTY walks forward with the key
/// labelled Z rather than hunting for a letter. The names are QWERTY
/// names because they have to be some board's names, and that is the
/// one the labels come from.
///
/// The list is exactly what the game can see -- every key it reads and
/// every key `ui::keybinds` will bind. A backend reporting anything else
/// reports nothing.
///
/// Serialisable because a player can now put a key *on the glass*: an
/// on-screen button carries the key it sends, and that has to survive
/// being written to the settings file. It goes out under its own
/// variant name -- `KeyG`, `Space` -- rather than the printed label
/// `ui::keybinds` uses, because a variant name is one unambiguous
/// string per key and a label is a thing that gets translated.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Key {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,

    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,

    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    /// Not a key any board has in the usual sense. It is what the
    /// on-screen controls report for a press that has no keyboard
    /// equivalent, and it exists so `Keybinds::key` can return
    /// *something* unbindable rather than an `Option` every caller has
    /// to unwrap. See `is_bindable`.
    F24,

    Escape,
    Enter,
    NumpadEnter,
    Space,
    Tab,
    Backspace,
    Delete,
    CapsLock,

    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,

    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,

    /// The two ends of a line.
    ///
    /// **Here only because a text field wants them**, and appended at
    /// the end of the enum rather than filed with the arrows because
    /// this list is serialised by name into the key bindings -- a
    /// variant moved is a saved binding that still reads, and a variant
    /// renamed is one that does not. Neither is offered as a binding:
    /// see `ui::keybinds::KEYS`, which is a separate table for exactly
    /// this reason.
    Home,
    End,
}

/// Which mouse button, or which finger's stand-in for one.
///
/// `Other` is carried rather than dropped because a player with a
/// five-button mouse has bound *something* to buttons four and five on
/// every other game they own, and the day this one lets them bind it
/// too, the number has to have survived the trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

/// A size in physical pixels -- real pixels in the framebuffer, not the
/// logical units a desktop scales by.
///
/// Physical because that is what the swapchain is measured in, and a
/// renderer told a logical size on a 200% display draws a quarter of
/// the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Both dimensions clamped to at least one pixel.
    ///
    /// A window dragged to nothing, or an Android activity mid-rotation,
    /// reports a zero, and a swapchain configured to zero is a validation
    /// error on every backend. One pixel is a frame nobody sees rather
    /// than a crash somebody does.
    /// The shorter of the two sides.
    ///
    /// What a thumb is measured against: a phone held sideways is much
    /// wider than it is tall, and a control sized against the long side
    /// is a control the size of a fist. See `touch::Layout::for_size`.
    pub fn shorter(self) -> u32 {
        self.width.min(self.height)
    }

    pub fn non_zero(self) -> Self {
        Self {
            width: self.width.max(1),
            height: self.height.max(1),
        }
    }
}

/// The characters one keystroke produced, if any.
///
/// Inline rather than a `String`, so an `Event` stays `Copy` and a
/// keystroke costs no allocation. Four is not a guess about typing
/// speed -- it is the ceiling on what a single key can emit: one
/// character for an ordinary key, one for a dead-key sequence that has
/// resolved, and headroom for the handful of layouts whose keys emit a
/// short cluster. A platform that somehow produced more would have the
/// tail dropped, which is why `push` says so rather than panicking:
/// losing the fifth character of a keystroke is a cosmetic bug, and
/// killing the game over it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text {
    chars: [char; Text::MAX],
    len: u8,
}

impl Default for Text {
    fn default() -> Self {
        // `char` has no `Default`, so the empty slots are spelled out.
        // Never read: `chars()` stops at `len`.
        Self {
            chars: ['\0'; Text::MAX],
            len: 0,
        }
    }
}

impl Text {
    pub const MAX: usize = 4;

    /// The characters, in the order they were typed.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.chars[..self.len as usize].iter().copied()
    }

    /// The first character, which is what anything wanting "the letter
    /// that was typed" means.
    pub fn first(&self) -> Option<char> {
        self.chars().next()
    }

    /// Appends a character, or drops it if there is no room.
    pub fn push(&mut self, c: char) {
        if (self.len as usize) < Self::MAX {
            self.chars[self.len as usize] = c;
            self.len += 1;
        }
    }
}

impl FromIterator<char> for Text {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
        let mut text = Text::default();
        for c in iter {
            text.push(c);
        }
        text
    }
}

/// Which finger, over the life of one touch.
///
/// The same id runs from the press through every move to the release,
/// which is the only way to tell "one finger dragged across the screen"
/// from "several fingers appeared in a row" -- and that difference is
/// the whole of the on-screen stick.
pub type TouchId = u64;

/// What happened to a finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    /// The system took the touch away -- a notification shade pulled
    /// down, a call arriving. Handled exactly like `Ended` by anything
    /// that does not care *why* the finger left, and distinguished for
    /// anything that does.
    Cancelled,
}

/// One thing that happened, in the game's own words.
///
/// Deliberately flat rather than winit's window/device split. The game
/// never cared which device a key came from, and the one place the
/// distinction mattered -- raw mouse motion, which must not be the
/// cursor's position -- is its own variant here (`MouseMotion` against
/// `CursorMoved`), which says the difference in the name instead of in
/// a type two levels up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The framebuffer changed size. Carries physical pixels.
    Resized(Size),
    /// The player asked to close the window.
    CloseRequested,
    /// Time to draw.
    RedrawRequested,
    /// Every event that was waiting has been handled.
    ///
    /// Where a frame is asked for, and the reason the loop does not ask
    /// for one per event: a burst of twenty mouse moves is one frame's
    /// worth of news, not twenty frames.
    AboutToWait,

    /// The keyboard did something.
    ///
    /// Position and text in one event rather than two, because the
    /// game reads them together and the answer depends on both. A menu
    /// asks "was this Escape?" (position) and then "was this a letter
    /// for the field I am editing?" (text), and it has to ask in that
    /// order about *the same keystroke* -- split into two events, a
    /// shortcut and the character it also produced arrive separately
    /// and each handler has to guess whether the other already ran.
    ///
    /// `key` is `None` for a key the game has no name for. `text` is
    /// empty for one that produces no character. Either can happen
    /// without the other: F1 is a key with no text, and a compose
    /// sequence is text with no key.
    Keyboard {
        key: Option<Key>,
        text: Text,
        pressed: bool,
        /// Whether the platform's auto-repeat produced this, rather
        /// than the player pressing the key again. Anything that must
        /// happen once per press -- sending a chat line, opening a
        /// screen -- has to ask, or holding the key does it forty
        /// times a second.
        repeat: bool,
    },

    /// The cursor moved to a point in the window, in physical pixels
    /// from the top-left. Menus use this; the world does not.
    CursorMoved { x: f32, y: f32 },
    /// The cursor left the window, so nothing on a menu is hovered.
    CursorLeft,
    /// Raw pointer movement, unstuck from any position.
    ///
    /// What the camera turns on. It has to be raw: the cursor is locked
    /// while the world has it, so its *position* stops changing and only
    /// the deltas are left.
    MouseMotion { dx: f32, dy: f32 },
    MouseButton { button: MouseButton, pressed: bool },
    /// Wheel movement in lines. Positive is away from the player.
    ///
    /// Normalised to lines by the backend, because a trackpad reports
    /// pixels and a wheel reports notches, and the hotbar wants neither
    /// -- it wants "one step".
    MouseWheel { lines: f32 },

    /// A finger did something. Coordinates are physical pixels, like the
    /// cursor's.
    Touch {
        id: TouchId,
        phase: TouchPhase,
        x: f32,
        y: f32,
    },

    /// The window gained or lost focus.
    ///
    /// Worth an event because key-up does not arrive while a window is
    /// unfocused: without this, a player who alt-tabs mid-stride comes
    /// back still running into whatever they were running into.
    Focused(bool),

    /// The app is going away and cannot draw again -- Android's activity
    /// being destroyed, or a desktop session ending under us.
    ///
    /// Distinct from `CloseRequested`, which is a request the game may
    /// answer however it likes. This one is notice.
    Suspended,
    /// The app can draw again. On Android the surface is *new*, so
    /// anything holding the old one has to let go; see the backend.
    Resumed,
}

/// What to open, before there is anything to open it with.
///
/// A plain description rather than a builder, because every field is
/// something the settings file already said and none of them is
/// optional in the sense a builder is for. A backend takes what it can
/// use and ignores the rest: an Android activity is fullscreen at the
/// display's size whatever this asks for, and has no title bar to put
/// `title` in.
pub struct WindowConfig {
    pub title: String,
    /// Logical pixels -- what the player set in the settings, before
    /// the display's scale factor is applied. The backend converts.
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    /// RGBA8 pixels and their dimensions, or nothing.
    ///
    /// Carried as bytes rather than as a decoded platform icon so that
    /// loading it stays the game's business and displaying it stays the
    /// backend's. A backend with nowhere to put an icon drops it.
    pub icon: Option<(Vec<u8>, u32, u32)>,
}

/// The one thing a frame handler can tell the loop: stop.
///
/// A type rather than a bool return, because "did this event ask to
/// quit" is a question with one answer in about a thousand events and
/// threading it back through every arm of a match that size is how you
/// get an arm that forgets. Handed in, set where it matters, read by
/// the loop.
#[derive(Default)]
pub struct Control {
    exit: bool,
}

impl Control {
    /// Asks the loop to stop after this event.
    ///
    /// Not immediate: the handler runs to the end of the event it is
    /// in. Everything that has to happen on the way out -- saving a
    /// world, telling the server -- therefore happens where it is
    /// written rather than being skipped by an early exit.
    pub fn exit(&mut self) {
        self.exit = true;
    }

    pub fn exit_requested(&self) -> bool {
        self.exit
    }
}

/// What the game needs from whatever is holding its pixels.
///
/// Small on purpose. Every method here is something the frame loop
/// actually calls, and nothing here leaks the backend's own types --
/// which is what makes a second backend a file rather than a rewrite.
pub trait Window {
    /// The framebuffer size, in physical pixels.
    fn size(&self) -> Size;

    /// How many physical pixels there are to one density-independent
    /// pixel -- Android's `density`, and the same number every other
    /// platform calls the scale factor.
    ///
    /// ## Why the game needs it at all
    ///
    /// **Because a fraction of a screen is not a size.** Everything a
    /// thumb aims at in this game is laid out as a fraction of the
    /// screen's shorter side (see `touch::Layout` and
    /// `widgets::FINGER`), which travels between phones of different
    /// pixel counts and says nothing whatever about millimetres: the
    /// same 0.075 is 30 dp on the phone this was cut for and would be
    /// something else again on a cheaper screen or a tablet. Every
    /// platform's own guidance puts the smallest honest touch target at
    /// 44 dp, and a game that cannot say what a dp is cannot check
    /// itself against that number.
    ///
    /// This is the number that makes the check possible. It is not yet
    /// what the layout is *built* from -- moving the layout onto dp
    /// re-spaces every menu in the game and is the player's call, not a
    /// silent change -- so for now the measurement is printed at
    /// startup (`[touch]`) with anything under 44 dp named, which is
    /// the difference between a known number and a guess.
    ///
    /// One everywhere it cannot be asked, which is the value that makes
    /// "pixels" and "dp" the same word.
    fn scale_factor(&self) -> f32 {
        1.0
    }

    /// Sets the window's title.
    ///
    /// A no-op where there is no title bar to put it in -- an Android
    /// activity, a fullscreen kiosk. The game uses it for the F3
    /// readout, which is a debugging convenience and not something a
    /// platform without a title bar is missing out on.
    fn set_title(&self, _title: &str) {}

    /// Ask for a frame. The backend answers with `Event::RedrawRequested`
    /// when it is ready, which may be immediately or may be never if the
    /// window is not visible.
    fn request_redraw(&self);

    /// Borderless fullscreen, on or off.
    ///
    /// A no-op where the concept does not apply -- an Android activity
    /// is always fullscreen, and asking it to stop is not a failure,
    /// it is a question with no meaning.
    fn set_fullscreen(&self, on: bool);

    /// Lock the pointer to the window and hide it, or let it go.
    ///
    /// Returns whether the pointer is now actually grabbed, which is not
    /// the same as what was asked for: X11 and Wayland disagree about
    /// which grab modes exist, a phone has no pointer to grab, and the
    /// caller has to know which world it is in -- a game that thinks it
    /// has the mouse when it does not turns every cursor movement into
    /// a camera spin the player cannot stop.
    fn set_cursor_grabbed(&self, grabbed: bool) -> bool;

    /// Whether this platform has an on-screen keyboard rather than a
    /// real one, and therefore needs touch controls drawn over the
    /// world.
    ///
    /// A property of the platform rather than a setting, because it is
    /// not a preference: a phone has no W key to walk forward with.
    fn is_touch_primary(&self) -> bool {
        false
    }

    /// Ask the platform to show or hide its on-screen keyboard.
    ///
    /// Only ever called where `is_touch_primary` is true; a no-op
    /// everywhere else, because a desktop's keyboard is already shown.
    fn set_ime_visible(&self, _visible: bool) {}

    /// Whether the platform's own editor holds the text of the focused
    /// field, rather than the game building it up out of keystrokes.
    ///
    /// ## Why a field can have two owners at all
    ///
    /// A desktop keyboard sends one keystroke and the game appends one
    /// character; there is one copy of the text and it is the game's.
    /// An Android input method does not work that way. It is an editor
    /// in its own right -- it holds the whole field, it has a cursor
    /// and a composing region, it corrects a word three characters back
    /// when it decides what the word was, and it commits *text*, not
    /// keys. That is the entire reason the game moved to GameActivity:
    /// there is no keycode for `щ`, and a platform that reports
    /// keystrokes cannot report Russian.
    ///
    /// So on a phone there are two copies of the field and they have to
    /// be kept in step. `ime_text` and `set_ime_text` are the two
    /// directions; this predicate is how the frame loop knows which
    /// world it is in.
    ///
    /// ## Why not an event carrying the text
    ///
    /// The obvious shape is an `Event::TextField(String)` beside
    /// `Event::Keyboard`. It was rejected for one concrete reason:
    /// `Event` is `Copy`, which is what lets the touch layer rewrite an
    /// event into another one (see `platform::touch`) and lets the
    /// frame loop match on it without ceremony. A `String` in there
    /// ends that, in every arm, on every platform, to carry something
    /// exactly one platform produces.
    ///
    /// The second reason is that an event is a thing that *happened*,
    /// and this is a thing that *is*. The input method's state can
    /// change three times between two frames and the game only ever
    /// wants the last one; polling it once a frame while a field has
    /// focus reads the answer rather than the history.
    fn ime_owns_text(&self) -> bool {
        false
    }

    /// What the platform's editor holds, when it is the owner.
    ///
    /// `None` where there is no such editor, which is everywhere but a
    /// phone with the keyboard up.
    fn ime_text(&self) -> Option<String> {
        None
    }

    /// Puts the game's own text back into the platform's editor.
    ///
    /// Needed in both directions because the game is still the
    /// authority on what a field may contain: the world-name field
    /// refuses a character the font cannot draw, the seed field takes
    /// digits only, and both stop at a length. When the input method
    /// offers something one of those rules rejects, the game's copy and
    /// the editor's copy have diverged -- and the *next* thing typed
    /// would be read against the wrong text. This is how they are made
    /// to agree again, with the game's version winning.
    ///
    /// Also how the editor is started off holding what the field
    /// already had, so that backspacing over a name the player is
    /// editing deletes the name rather than nothing.
    fn set_ime_text(&self, _text: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_dragged_to_nothing_still_has_a_pixel() {
        // A zero-sized swapchain is a validation error on every backend,
        // and a window can report zero: minimised on Windows, mid-rotation
        // on Android.
        assert_eq!(Size::new(0, 0).non_zero(), Size::new(1, 1));
        assert_eq!(Size::new(0, 720).non_zero(), Size::new(1, 720));
        assert_eq!(Size::new(1280, 720).non_zero(), Size::new(1280, 720));
    }
}
