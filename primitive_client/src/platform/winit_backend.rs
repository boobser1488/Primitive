//! winit, translated.
//!
//! This is the only file in the client that names a winit type, and that
//! is the whole point of it. Everything above it -- the frame loop, the
//! keybindings, the renderer's idea of how big it is -- speaks
//! `platform::Event`, `platform::Key` and `platform::Size`, so a second
//! backend is a second file rather than a second game.
//!
//! ## What translation costs
//!
//! Nothing measurable. The conversions below are matches over small
//! enums that the compiler turns into jump tables, run once per event;
//! a busy frame has a few dozen events in it. The one that is not free
//! is `wheel_lines`, which divides -- once per wheel notch, which is as
//! often as a player can turn a wheel.
//!
//! ## What it cannot translate
//!
//! Keys the game has no name for. `key_from_winit` returns `None` for
//! them and the loop drops them, which is deliberate: see the note on
//! `platform::Key` about unbindable keys being better than bindings
//! that do nothing.

use std::sync::Arc;

use winit::window::{CursorGrabMode, Window as WinitWindow};

use super::{Key, MouseButton, Size, TouchPhase};

/// A winit window, wearing the game's interface.
pub struct Window {
    window: Arc<WinitWindow>,
}

impl Window {
    pub fn new(window: Arc<WinitWindow>) -> Self {
        Self { window }
    }

    /// The winit window underneath.
    ///
    /// Exists for exactly one caller: building the wgpu surface, which
    /// needs a real window handle and cannot be given a trait object.
    /// Everything else goes through `platform::Window`.
    pub fn raw(&self) -> Arc<WinitWindow> {
        self.window.clone()
    }

    /// Which window this is, for telling our own events from anyone
    /// else's. winit's own identity type, and deliberately not on the
    /// `platform::Window` trait: "which of several windows" is a
    /// question only a backend that can have several needs to answer.
    pub fn id(&self) -> winit::window::WindowId {
        self.window.id()
    }
}

impl super::Window for Window {
    fn size(&self) -> Size {
        let size = self.window.inner_size();
        Size::new(size.width, size.height)
    }

    /// winit's own scale factor, which on Android is the display's
    /// `density` divided by 160 -- 3.0 on the 480-dpi phone this game
    /// is tested on. On a desktop it is whatever the window manager
    /// says about the monitor it is on.
    fn scale_factor(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }

    fn set_fullscreen(&self, on: bool) {
        self.window
            .set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
    }

    fn set_cursor_grabbed(&self, grabbed: bool) -> bool {
        // There is no pointer on a phone, so there is none to take and
        // none to lose. The game asks this to find out whether the
        // world has the input or a menu does, and on glass the answer
        // is simply whatever was asked for: nothing can steal the
        // cursor, because there is no cursor.
        if cfg!(target_os = "android") {
            return grabbed;
        }
        if !grabbed {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            return false;
        }
        // Locked first, Confined second, and the order matters. Locked is
        // what a first-person camera wants -- the pointer stops existing
        // and only deltas are left -- but Windows does not implement it,
        // where Confined merely keeps the pointer inside the window and
        // is close enough. Asking for the good one and settling for the
        // workable one is why this returns a bool rather than a Result.
        let ok = self
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined))
            .is_ok();
        if ok {
            self.window.set_cursor_visible(false);
        }
        ok
    }

    fn is_touch_primary(&self) -> bool {
        // Compiled out rather than detected. A desktop with a
        // touchscreen is still a desktop -- it has a W key -- and
        // drawing a thumbstick over the world because someone poked
        // their monitor would be a bug, not a feature.
        cfg!(target_os = "android")
    }

    fn set_ime_visible(&self, visible: bool) {
        // **On Android this line does nothing at all, and says so
        // nowhere.** winit 0.29's Android backend is
        // `pub fn set_ime_allowed(&self, _allowed: bool) {}` -- an
        // empty body, no warning, no error. So every screen with a
        // text field in it asked for a keyboard, got no keyboard and no
        // complaint, and the player could not type a world name, a
        // seed, a server address or their own username. The whole of
        // singleplayer was unreachable on a phone because the
        // create-world screen refused an empty name.
        //
        // The activity has the real thing on it. See `show_the_keyboard`.
        #[cfg(target_os = "android")]
        show_the_keyboard(visible);
        // Still asked for on a desktop, where it is what turns IME
        // events on for a compose key or an East Asian input method.
        self.window.set_ime_allowed(visible);
    }

    fn ime_owns_text(&self) -> bool {
        // Compiled out rather than detected, for the same reason as
        // `is_touch_primary`: this is a property of the platform's
        // input model, not of the hardware plugged into it. A phone
        // with a Bluetooth keyboard still routes text through an input
        // method; a desktop with a touchscreen still does not.
        //
        // ...unless a desktop has been asked to stand in for one. See
        // `stand_in_editor`.
        cfg!(target_os = "android") || stand_in_editor().is_some()
    }

    fn ime_text(&self) -> Option<String> {
        if let Some(editor) = stand_in_editor() {
            return Some(editor.lock().expect("the stand-in editor").clone());
        }
        #[cfg(target_os = "android")]
        {
            // A read of GameTextInput's own copy of the field, which
            // the Java `InputConnection` writes to as the player types.
            // Not a JNI call and not a queued event: the state lives in
            // native memory beside the glue, so asking is a pointer
            // dereference and a string copy, and asking once a frame
            // while a field has focus costs nothing worth measuring.
            //
            // Deliberately a *poll*. winit 0.29's Android backend
            // matches on `MotionEvent` and `KeyEvent` and drops
            // `InputEvent::TextEvent` on the floor, so the event the
            // glue does raise never reaches this crate -- and even if
            // it did, three edits between two frames are three events
            // and one answer. See `Window::ime_owns_text`.
            super::android_app().map(|app| app.text_input_state().text)
        }
        #[cfg(not(target_os = "android"))]
        {
            None
        }
    }

    fn set_ime_text(&self, text: &str) {
        if let Some(editor) = stand_in_editor() {
            text.clone_into(&mut editor.lock().expect("the stand-in editor"));
            return;
        }
        #[cfg(not(target_os = "android"))]
        let _ = text;
        #[cfg(target_os = "android")]
        {
            use winit::platform::android::activity::input::{TextInputState, TextSpan};
            let Some(app) = super::android_app() else {
                return;
            };
            // The cursor goes to the end of the text, always. The game
            // has no caret of its own -- `Menu::type_char` appends and
            // `backspace` pops -- so any other answer would be an
            // invention, and an input method told the cursor is in the
            // middle of a word it did not put there starts correcting
            // the word.
            //
            // **Counted in UTF-16 units, not bytes.** The number goes
            // straight across JNI: `GameActivity_setTextInputState`
            // hands it to `gametextinput.State`'s constructor
            // (`stateToJava` in `gametextinput.cpp` passes
            // `selection.start` through untouched), and that field is
            // an index into a Java `String` -- so it is measured in the
            // units Java measures strings in.
            //
            // `str::len` is bytes, and every Cyrillic letter is two of
            // them: eight characters of Russian asked for the cursor at
            // fifteen, past the end of an eight-character string. On
            // the device that showed up as the committed text arriving
            // with stale text still stuck to it -- what exactly the
            // Java side does with an out-of-range selection was not
            // established, only that a valid index is what it is owed.
            //
            // Nothing about it shows up in ASCII, where the two counts
            // are the same number -- which is exactly the shape of bug
            // that survives being tested in English.
            //
            // (`android-activity` reads the span back clamped against
            // the *byte* length, which is its own inconsistency. It
            // does not matter here: nothing in this game reads the
            // cursor back, only the text.)
            let end = text.encode_utf16().count();
            app.set_text_input_state(TextInputState {
                text: text.to_owned(),
                selection: TextSpan { start: end, end },
                // No composing region. A composing region is the input
                // method's own workings -- the underlined word it has
                // not decided about yet -- and re-asserting one the
                // game invented would make it re-decide a word the
                // player already finished.
                compose_region: None,
            });
        }
    }
}

/// Takes the whole panel and stops it going dark.
///
/// ## Why a game has to say it must not dim
///
/// Android turns the display off after a couple of minutes without a
/// touch, and it counts *touches*, not frames. A player standing still
/// watching the sun come up, or walking with one thumb on a stick that
/// is already where they want it, is idle by that measure -- so the
/// screen dims and then goes out in the middle of play, which suspends
/// the activity, drops the surface, and on a server ends the session.
/// Every game on the platform sets this flag; a game that does not is
/// one that switches itself off while you are looking at it.
///
/// ## Why the flag rather than a wake lock
///
/// `KEEP_SCREEN_ON` is a property of *this window*, so it applies
/// exactly while the game is the thing on screen and stops applying the
/// moment it is not -- no permission, and nothing to release. A wake
/// lock is a process-wide claim on the device that has to be taken and
/// given back by hand, and one that is leaked keeps a phone awake in a
/// pocket until the battery is flat.
///
/// Deliberately *not* `SHOW_WHEN_LOCKED` or `TURN_SCREEN_ON`, which sit
/// next to it in the same enum: those put the game over the lock screen
/// and wake the device on their own. A game has no business doing
/// either, and the flag being one bit away from them is not a reason to
/// set them.
/// ## ...and why it also takes the whole panel
///
/// ### What the black stripe was
///
/// Android reserves a band along one edge for the navigation bar --
/// the gesture pill -- and gives the activity a window that stops short
/// of it. The game filled its window exactly, so on a phone held
/// sideways there was a black strip across the bottom of the picture
/// with the pill floating in the middle of it. The manifest's theme,
/// `NoTitleBar.Fullscreen`, does not help: it hides the *status* bar
/// and says nothing about the navigation bar.
///
/// Three flags together, and each is doing a separate job:
/// `FULLSCREEN` takes the status bar, `LAYOUT_IN_SCREEN` asks for the
/// window to be positioned against the whole display rather than the
/// space left between the decorations, and `LAYOUT_NO_LIMITS` is what
/// actually lets it extend past them. The first two on their own leave
/// the band exactly where it was.
///
/// ### Why the pill is still drawn
///
/// Because it is the system's and the system puts it back. What
/// changes is that the game is now drawn *underneath* it instead of
/// stopping at its edge, so the picture reaches the bottom of the glass
/// and the pill floats over the world -- which is what every other
/// game on the platform looks like.
///
/// Nothing of the interface is lost behind it. The thumb controls sit
/// in from the corner by `TouchLayout`'s own margin, six hundredths of
/// the short side, which on this phone is 73 pixels against a pill
/// about 40 tall.
///
/// ### Why here rather than in the manifest
///
/// A theme could say some of it, and a theme of our own would mean a
/// `res/values` and an `@style` reference for a property the window can
/// simply be told at runtime. This is set beside `KEEP_SCREEN_ON`
/// because it is the same kind of statement about the same window, and
/// it is set on `Resumed` for the same reason that one is: the window
/// the flags apply to does not exist before then, and it is a
/// different window after the activity has been through `Suspended`.
#[cfg(target_os = "android")]
fn take_the_whole_screen_and_keep_it_lit() {
    use winit::platform::android::activity::WindowManagerFlags;
    let Some(app) = super::android_app() else {
        return;
    };
    app.set_window_flags(
        WindowManagerFlags::KEEP_SCREEN_ON
            | WindowManagerFlags::FULLSCREEN
            | WindowManagerFlags::LAYOUT_IN_SCREEN
            | WindowManagerFlags::LAYOUT_NO_LIMITS,
        WindowManagerFlags::empty(),
    );
}



/// Raises or dismisses the on-screen keyboard.
///
/// One call, and it is the documented one:
/// `GameActivity_showSoftInput`, reached through `AndroidApp`.
///
/// **This is the line that used to need a hundred lines of JNI beside
/// it.** Under `NativeActivity` the framework threw the request away --
/// `Ignoring showSoftInput() as view=NativeContentView is not served` --
/// because a NativeActivity's content view is never focused and
/// therefore never the view the input method is serving. Getting round
/// that meant calling `setFocusableInTouchMode` and `requestFocus`
/// through JNI, which raised a keyboard that then could not type
/// anything but ASCII, because a view with no `InputConnection` is a
/// `TYPE_NULL` editor and a `TYPE_NULL` editor is sent keycodes.
/// `GameActivity`'s view is focusable and does return an
/// `InputConnection`, so the request lands and the text comes back.
///
/// ## Why explicit rather than implicit
///
/// `show_implicit: false`. An *implicit* keyboard is one the system
/// decides the user probably wants -- it is dismissed by anything that
/// looks like a change of mind, and several launchers refuse to show
/// one at all over a fullscreen landscape window, which is exactly what
/// this game is. Explicit is the player having tapped a text field,
/// which is the only thing that gets here.
///
/// `hide_implicit_only: false` for the same reason in reverse: hiding
/// only implicit keyboards would leave the explicit one we just raised
/// on screen, over the world, with nothing left to type into.
///
/// A missing activity is not an error worth reporting. It means
/// `android_main` has not run, which on a phone cannot happen -- and
/// the alternative, a panic, would kill a game over a keyboard.
#[cfg(target_os = "android")]
fn show_the_keyboard(visible: bool) {
    let Some(app) = super::android_app() else {
        return;
    };
    if visible {
        app.show_soft_input(false);
    } else {
        app.hide_soft_input(false);
    }
}

/// An input method's editor, for a machine that has none.
///
/// ## Why a desktop pretends to be a phone
///
/// **Because the Android text path had no check that could be run
/// twice.** Everything about it -- the keyboard following the focus,
/// the two copies of one field being reconciled, a rejected character
/// being taken back out of the editor -- exists only where there *is*
/// an input method, and the only machine with one is a phone that
/// refuses synthetic input (see `CLAUDE.md`). So the whole of it was
/// verified by a person typing, once, and the second field on the form
/// was never typed into at all. That is how the seed box came to accept
/// nothing.
///
/// This is one `String` behind a mutex, and with it in place a desktop
/// answers `ime_owns_text` with true, hands the frame loop its contents
/// once a frame and takes the game's corrections back -- which is the
/// entire contract GameTextInput fulfils on a device. What it does
/// *not* stand in for is the keyboard itself: `show_soft_input`, the
/// doubled key events, the composing region. Those need a device and
/// are said so plainly rather than pretended at.
///
/// Off unless asked for. `PRIMITIVE_IME_TYPE` implies it, because a
/// scripted run of the text path with no editor to commit into is a run
/// that proves nothing; `PRIMITIVE_IME_EDITOR=1` turns it on by itself,
/// for driving the forms by hand on a desktop.
fn stand_in_editor() -> Option<&'static std::sync::Mutex<String>> {
    // Never on a device: there is a real editor there, and a second one
    // in front of it would be a game talking to itself.
    if cfg!(target_os = "android") {
        return None;
    }
    static EDITOR: std::sync::OnceLock<Option<std::sync::Mutex<String>>> =
        std::sync::OnceLock::new();
    EDITOR
        .get_or_init(|| {
            let asked = std::env::var_os("PRIMITIVE_IME_TYPE").is_some()
                || std::env::var("PRIMITIVE_IME_EDITOR").is_ok_and(|v| v != "0");
            asked.then(|| std::sync::Mutex::new(String::new()))
        })
        .as_ref()
}

/// The event loop and the window it owns, before either has started.
///
/// Split from `run` so the game can build its renderer against the
/// window *before* the loop takes over the thread -- which it must,
/// because winit's loop never returns control except through a
/// callback, and a renderer built inside that callback would have to be
/// an `Option` that every frame unwraps.
pub struct App {
    event_loop: winit::event_loop::EventLoop<()>,
    /// Which window's events are ours.
    ///
    /// Kept here rather than asked of the window at `run` time, because
    /// the game hands the window to its renderer and then into the
    /// frame closure -- so by the time the loop starts there is nothing
    /// left to ask.
    ours: winit::window::WindowId,
    /// Events that arrived before the frame loop started.
    ///
    /// Filled by `pump_idle` and replayed into the handler the moment
    /// `run` begins, so that answering Android during startup does not
    /// mean throwing away what it said. A `Resized` dropped here is a
    /// swapchain built at the wrong size; a `Suspended` dropped here is
    /// a renderer drawing into a window that no longer exists.
    #[cfg(target_os = "android")]
    pending: Vec<super::Event>,
}

impl App {
    /// Opens the window.
    ///
    /// Returns the loop and the window separately: the window is what
    /// the game keeps, the loop is what it hands back to `run` once it
    /// has everything else built.
    pub fn new(config: &super::WindowConfig) -> anyhow::Result<(Self, Window)> {
        let mut event_loop = Self::build_event_loop()?;
        // **On Android there is nothing to draw into yet.**
        //
        // A desktop window exists the moment it is built. An Android
        // activity does not: the system gives it an `ANativeWindow`
        // when it becomes the thing on screen, and takes it away again
        // when it stops being. Until the first `Resumed`, the window
        // winit hands back has no native handle behind it -- so
        // `wgpu::Instance::create_surface` against it fails, the game
        // reports "graphics could not start", and the activity dies
        // before it has drawn a frame.
        //
        // This waits for it. The loop is pumped rather than run,
        // because `run` never gives the thread back and everything the
        // game builds next -- the renderer, the sound, the world --
        // has to happen before the frame loop starts.
        Self::wait_for_a_surface(&mut event_loop);
        // Poll, not Wait. A game draws whether or not anything happened
        // -- the world moves on its own -- and `Wait` would sit still
        // until the player touched something.
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

        let icon = config.icon.as_ref().and_then(|(pixels, w, h)| {
            winit::window::Icon::from_rgba(pixels.clone(), *w, *h)
                .map_err(|e| eprintln!("window icon rejected: {e}"))
                .ok()
        });

        let window = winit::window::WindowBuilder::new()
            .with_title(&config.title)
            .with_window_icon(icon)
            .with_inner_size(winit::dpi::LogicalSize::new(
                config.width as f64,
                config.height as f64,
            ))
            // Straight into fullscreen if that is how the game was left,
            // rather than a windowed flash first: the size above is what
            // it returns to when fullscreen is turned off.
            .with_fullscreen(
                config
                    .fullscreen
                    .then_some(winit::window::Fullscreen::Borderless(None)),
            )
            .build(&event_loop)?;

        let window = Window::new(Arc::new(window));
        // Now rather than in `android_main`, because it is a property of
        // a window and there was none until this line.
        #[cfg(target_os = "android")]
        take_the_whole_screen_and_keep_it_lit();
        Ok((
            Self {
                event_loop,
                ours: window.id(),
                #[cfg(target_os = "android")]
                pending: Vec::new(),
            },
            window,
        ))
    }

    /// An event loop, however this platform makes one.
    ///
    /// A desktop's is built from nothing. Android's needs the
    /// `AndroidApp` the system handed to `android_main` -- it is the
    /// activity, and winit has no way to find it on its own -- which is
    /// why `android_main` stashes it before calling into the game. See
    /// `super::android_app`.
    #[cfg(not(target_os = "android"))]
    fn build_event_loop() -> anyhow::Result<winit::event_loop::EventLoop<()>> {
        Ok(winit::event_loop::EventLoop::new()?)
    }

    /// Blocks until Android has given the activity a window.
    ///
    /// Nothing at all on a desktop, where a window is a window from the
    /// moment it is made.
    #[cfg(not(target_os = "android"))]
    fn wait_for_a_surface(_event_loop: &mut winit::event_loop::EventLoop<()>) {}

    #[cfg(target_os = "android")]
    fn wait_for_a_surface(event_loop: &mut winit::event_loop::EventLoop<()>) {
        use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};

        println!("waiting for the activity to be given a window...");
        // How often to repeat that, so a wait is distinguishable from a
        // hang.
        //
        // **A locked phone never gives the activity a window**, and this
        // loop is then correct and indefinite: it blocks, the game
        // prints one line, and logcat shows a process that started and
        // stopped talking -- which reads exactly like a freeze. Twenty
        // minutes went into that mistake once. Saying so again every
        // few seconds costs nothing and turns "it hung" into "it is
        // waiting for the screen", which is a different bug report.
        const STILL_WAITING: std::time::Duration = std::time::Duration::from_secs(5);
        let mut said = std::time::Instant::now();
        loop {
            if said.elapsed() >= STILL_WAITING {
                println!("still waiting for a window -- is the screen off, or the phone locked?");
                said = std::time::Instant::now();
            }
            let mut resumed = false;
            // Bounded rather than `None`, and only so the line above can
            // be printed: a blocking wait would never come back to say
            // it was still waiting. Five seconds asleep is not a spin --
            // the phone showing its launcher pays one wake-up per five
            // seconds for it, which is nothing.
            let status = event_loop.pump_events(Some(STILL_WAITING), |event, _| {
                if matches!(event, winit::event::Event::Resumed) {
                    resumed = true;
                }
            });
            if resumed {
                return;
            }
            // The activity was dismissed before it ever came up -- the
            // player backed out of a cold start. There is no window
            // coming; returning lets the caller fail on the surface
            // instead of waiting here for a process that is ending.
            if let PumpStatus::Exit(_) = status {
                return;
            }
        }
    }

    #[cfg(target_os = "android")]
    fn build_event_loop() -> anyhow::Result<winit::event_loop::EventLoop<()>> {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let app = super::android_app()
            .ok_or_else(|| anyhow::anyhow!("no activity: android_main did not run first"))?;
        Ok(winit::event_loop::EventLoopBuilder::new()
            .with_android_app(app)
            .build()?)
    }

    /// Answers Android while the game is still starting up.
    ///
    /// ## Why a game has to do this at all
    ///
    /// **Android asks questions that have a five-second deadline, and
    /// startup does not hear them.** Between this loop being built and
    /// `run` being called, the main thread loads textures, builds
    /// pipelines and generates the sound bank -- and on a first run it
    /// unpacks the whole package before that. None of that touches the
    /// event loop, so an `onPause` arriving in the middle of it goes
    /// unanswered: android-activity blocks the Java main thread on a
    /// condvar until this thread acknowledges the lifecycle command,
    /// and the system kills an app that has not answered in five
    /// seconds.
    ///
    /// That is not a theory. The phone's dropbox held seven ANRs for
    /// this package in one afternoon, every one of them the same shape:
    /// Java `main` inside `NativeActivity.onPause`, and `android_main`
    /// asleep in `read()` with a millisecond of CPU to its name --
    /// still in startup, never having pumped. The app was killed each
    /// time, which on a phone means the world was never saved.
    ///
    /// The activity class has changed since (`GameActivity` now) and
    /// nothing about that changed: the same glue blocks the same Java
    /// thread on the same condvar, and the watchdog counts the same
    /// five seconds.
    ///
    /// So the startup path calls this between its phases. It costs a
    /// syscall that finds nothing when nothing is waiting.
    ///
    /// ## Why the events are kept rather than dropped
    ///
    /// Because they are real. A `Resized` arriving here is the window
    /// telling us the size the swapchain is about to be built at, and
    /// dropping it means building the wrong one. A `Suspended` is the
    /// activity's window being taken away, and dropping it means the
    /// renderer drawing into nothing. They are held and replayed into
    /// the handler in order the moment `run` starts, which is the first
    /// time there is anything able to act on them.
    #[cfg(target_os = "android")]
    pub fn pump_idle(&mut self) {
        use winit::platform::pump_events::EventLoopExtPumpEvents;

        let ours = self.ours;
        // Borrowed before the call so the two fields are borrowed
        // separately: the closure needs the queue, `pump_events` needs
        // the loop, and they are different fields.
        let pending = &mut self.pending;
        let _ = self
            .event_loop
            .pump_events(Some(std::time::Duration::ZERO), |event, _| {
                if let Some(event) = translate(&event, ours) {
                    pending.push(event);
                }
            });
    }

    /// Nothing at all off a phone.
    ///
    /// A desktop's window manager has no deadline and asks the game
    /// nothing it must answer within one, so there is nothing here to
    /// answer. Present so the startup path reads the same on every
    /// platform rather than being written twice around a `cfg`.
    #[cfg(not(target_os = "android"))]
    pub fn pump_idle(&mut self) {}

    /// Runs the loop until the handler asks to stop.
    ///
    /// Takes the thread and does not give it back. `handler` sees the
    /// game's own events, never winit's -- which is the whole point of
    /// this file.
    pub fn run(
        self,
        handler: impl FnMut(super::Event, &mut super::Control),
    ) -> anyhow::Result<()> {
        self.run_inner(handler)
    }

    /// The desktop loop: hand the thread to winit and never take it back.
    #[cfg(not(target_os = "android"))]
    fn run_inner(
        self,
        mut handler: impl FnMut(super::Event, &mut super::Control),
    ) -> anyhow::Result<()> {
        let ours = self.ours;
        self.event_loop.run(move |event, elwt| {
            let Some(event) = translate(&event, ours) else {
                return;
            };
            let mut control = super::Control::default();
            handler(event, &mut control);
            if control.exit_requested() {
                elwt.exit();
            }
        })?;
        Ok(())
    }

    /// The Android loop, pumped rather than run.
    ///
    /// **It has to be pumped, because `new` already pumped it.** Waiting
    /// for the activity to be handed a window is done by pumping events
    /// until `Resumed` arrives -- there is no other way to find out --
    /// and winit will not accept `run` on a loop that has already
    /// started: it answers `EventLoop is already running` and the game
    /// dies before its first frame.
    ///
    /// The two are otherwise the same loop. `Some(ZERO)` rather than
    /// `None` *while the game is on screen*, because a game draws
    /// whether or not anything happened: blocking until the next touch
    /// would stop the world between them.
    ///
    /// ## ...and `None` while it is not
    ///
    /// **A zero timeout in every state is a phone that burns a whole
    /// core in the player's pocket.** Backgrounded, the activity's
    /// window is gone, `graphics.surface_ready()` is false and every
    /// frame returns immediately without drawing -- so the loop spins
    /// as fast as the CPU will let it, doing nothing, for as long as
    /// the process lives. Measured on the phone with the screen off and
    /// the game in the background: 105% of one core, sustained, still
    /// going after thirty seconds. Android's freezer does not save it;
    /// the game holds an audio device, which keeps it out.
    ///
    /// So the loop blocks when there is nothing to draw into. Two
    /// things have to change together for that to work, and one alone
    /// does nothing: the timeout here, *and* the control flow. winit
    /// takes the smaller of the two, and `ControlFlow::Poll` is a zero
    /// of its own -- asking for `None` while Poll is set still spins.
    #[cfg(target_os = "android")]
    fn run_inner(
        mut self,
        mut handler: impl FnMut(super::Event, &mut super::Control),
    ) -> anyhow::Result<()> {
        use winit::event_loop::ControlFlow;
        use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};

        let ours = self.ours;
        // Starts true because `App::new` already waited for the first
        // `Resumed`; the game would not be here otherwise.
        let mut on_screen = true;

        // **And that sentence is exactly why the request has to be made
        // here as well as on `Resumed`.** The surface the game runs on
        // for its whole life is the one `App::new` waited for, so the
        // `Resumed` arm below never fires again unless the activity is
        // suspended and brought back. Hanging the frame-rate request on
        // that arm alone meant it was never made at all -- verified by
        // its log line being absent from a device run, which is the
        // second time this fix has been attached to a moment that does
        // not happen. See `android::ask_for_a_fast_frame_rate`.
        #[cfg(target_os = "android")]
        super::android::ask_for_a_fast_frame_rate();

        // Whatever arrived while the game was still loading, in the
        // order it arrived. See `pump_idle`: this is the first moment
        // there is a handler able to act on any of it.
        for event in std::mem::take(&mut self.pending) {
            match event {
                super::Event::Suspended => on_screen = false,
                super::Event::Resumed => on_screen = true,
                _ => {}
            }
            let mut control = super::Control::default();
            handler(event, &mut control);
            if control.exit_requested() {
                return Ok(());
            }
        }
        loop {
            let mut exit = false;
            let timeout = on_screen.then_some(std::time::Duration::ZERO);
            let status = self.event_loop.pump_events(timeout, |event, _| {
                let Some(event) = translate(&event, ours) else {
                    return;
                };
                // Read here rather than left to the game, because the
                // loop's own timing depends on it and the game is not
                // obliged to tell us what it did with an event.
                match event {
                    super::Event::Suspended => on_screen = false,
                    super::Event::Resumed => {
                        on_screen = true;
                        // The surface is new, and a frame-rate request
                        // belongs to a surface rather than to the app --
                        // so it has to be made again for each one. See
                        // `android::ask_for_a_fast_frame_rate`.
                        #[cfg(target_os = "android")]
                        super::android::ask_for_a_fast_frame_rate();
                    }
                    _ => {}
                }
                let mut control = super::Control::default();
                handler(event, &mut control);
                exit |= control.exit_requested();
            });
            // After the pump, not before: the state this sets is for
            // the *next* one, and setting it first would use the answer
            // from the frame before last.
            self.event_loop.set_control_flow(if on_screen {
                ControlFlow::Poll
            } else {
                ControlFlow::Wait
            });
            if exit {
                return Ok(());
            }
            if let PumpStatus::Exit(_) = status {
                return Ok(());
            }
        }
    }
}

/// One winit event in the game's own words, or nothing.
///
/// `None` for everything the game does not act on -- events for another
/// window, device events that are not raw pointer motion, the dozen
/// lifecycle events winit reports that this game has no use for. The
/// loop drops them, which is why the match above it has no catch-all
/// arm doing the same thing in a second place.
pub fn translate(
    event: &winit::event::Event<()>,
    ours: winit::window::WindowId,
) -> Option<super::Event> {
    use super::Event as E;
    use winit::event::{DeviceEvent, ElementState, Event, WindowEvent};

    Some(match event {
        Event::AboutToWait => E::AboutToWait,
        Event::Suspended => E::Suspended,
        Event::Resumed => E::Resumed,

        // Raw pointer movement, which is not the cursor's position and
        // must not be confused with it: the cursor is locked while the
        // world has it, so its position stops changing and these deltas
        // are the only thing left to turn the camera by.
        Event::DeviceEvent {
            event: DeviceEvent::MouseMotion { delta },
            ..
        } => E::MouseMotion {
            dx: delta.0 as f32,
            dy: delta.1 as f32,
        },

        Event::WindowEvent { window_id, event } if *window_id == ours => match event {
            WindowEvent::CloseRequested => E::CloseRequested,
            WindowEvent::RedrawRequested => E::RedrawRequested,
            WindowEvent::Resized(size) => E::Resized(Size::new(size.width, size.height)),
            WindowEvent::Focused(focused) => E::Focused(*focused),
            WindowEvent::CursorLeft { .. } => E::CursorLeft,
            WindowEvent::CursorMoved { position, .. } => E::CursorMoved {
                x: position.x as f32,
                y: position.y as f32,
            },
            WindowEvent::MouseWheel { delta, .. } => E::MouseWheel {
                lines: wheel_lines(*delta),
            },
            WindowEvent::MouseInput { state, button, .. } => E::MouseButton {
                button: button_from_winit(*button),
                pressed: *state == ElementState::Pressed,
            },
            WindowEvent::Touch(touch) => E::Touch {
                id: touch.id,
                phase: touch_phase_from_winit(touch.phase),
                x: touch.location.x as f32,
                y: touch.location.y as f32,
            },
            WindowEvent::KeyboardInput { event, .. } => E::Keyboard {
                key: match event.physical_key {
                    winit::keyboard::PhysicalKey::Code(code) => key_from_winit(code),
                    // A key winit itself could not name. Nothing to
                    // bind and nothing to look up, but it may still
                    // have produced text, so the event goes on rather
                    // than being dropped.
                    //
                    // **Android's Back arrives here, and used to stop
                    // here.** It has no physical code in winit's table
                    // at all -- `Keycode::Back` is mapped only to the
                    // *logical* `NamedKey::BrowserBack` -- so the arm
                    // above never saw it and the game never heard the
                    // gesture every Android player reaches for first.
                    // What that cost is concrete: a phone has no
                    // Escape and no inventory key, so a player who
                    // tapped a chest was inside it until they killed
                    // the app. The way out was a button drawn on the
                    // glass and nothing else.
                    //
                    // Read as Escape because that is what it *means*
                    // here: one step back out of whatever is open. The
                    // game already has one answer to that question, in
                    // one place, and this hands the gesture to it
                    // rather than inventing a second.
                    //
                    // Android only. On a desktop the same logical key
                    // is a mouse's side button, and a thumb resting on
                    // a mouse must not pause the game.
                    winit::keyboard::PhysicalKey::Unidentified(_) => {
                        #[cfg(target_os = "android")]
                        {
                            use winit::keyboard::{Key as LogicalKey, NamedKey};
                            if matches!(
                                event.logical_key,
                                LogicalKey::Named(NamedKey::BrowserBack)
                            ) {
                                Some(Key::Escape)
                            } else {
                                None
                            }
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            None
                        }
                    }
                },
                text: typed_text(event),
                pressed: event.state == ElementState::Pressed,
                repeat: event.repeat,
            },
            _ => return None,
        },

        _ => return None,
    })
}

/// The characters a keystroke produced.
///
/// `event.text` on every platform that fills it in, which is every
/// desktop one.
///
/// ## Why Android needs a second source
///
/// winit's Android backend builds its `KeyEvent` with `text: None`,
/// always -- it works the character out (it has to, to name the logical
/// key) and then does not put it in the field the rest of winit uses.
/// So a phone with the keyboard up reported *which key* was pressed and
/// never *what was typed*, and every text field in the game stayed
/// empty however much the player tapped.
///
/// `logical_key` is where that backend does put it, so that is where
/// this looks -- and only there, and only for `Character`. A named key
/// (Enter, Backspace, an arrow) is a key and not text; the arms above
/// already read it as one.
///
/// ## Why not on a desktop as well
///
/// Because there `text` being empty is *information*. Ctrl+C has a
/// logical key of `Character("c")` and no text, deliberately: the C is
/// part of a shortcut, not a letter for the field. Falling back there
/// would put a `c` in the chat box every time somebody copied
/// something.
///
/// ## What this is *not* for any more
///
/// Filling text fields. Under GameActivity a field's contents come
/// from the input method's own editor -- see
/// `Window::ime_owns_text` -- and the frame loop ignores this text
/// while a field has focus. What is left here is the keystroke a
/// *menu shortcut* is looked up by (`menu_key` takes `text.first()`),
/// which is a hardware keyboard's business and works the same on both
/// platforms.
fn typed_text(event: &winit::event::KeyEvent) -> super::Text {
    if let Some(text) = event.text.as_ref() {
        return text.chars().collect();
    }
    #[cfg(target_os = "android")]
    // Presses only, matching what `text` carries elsewhere: a release
    // that also reported the letter would type it twice into any field
    // that reads text without checking which way the key went.
    if event.state == winit::event::ElementState::Pressed {
        if let winit::keyboard::Key::Character(typed) = &event.logical_key {
            return typed.chars().collect();
        }
    }
    super::Text::default()
}

/// The game's name for a winit key, if the game has one.
///
/// `None` for everything else, which the loop drops.
pub fn key_from_winit(code: winit::keyboard::KeyCode) -> Option<Key> {
    use winit::keyboard::KeyCode as W;
    Some(match code {
        W::KeyA => Key::KeyA,
        W::KeyB => Key::KeyB,
        W::KeyC => Key::KeyC,
        W::KeyD => Key::KeyD,
        W::KeyE => Key::KeyE,
        W::KeyF => Key::KeyF,
        W::KeyG => Key::KeyG,
        W::KeyH => Key::KeyH,
        W::KeyI => Key::KeyI,
        W::KeyJ => Key::KeyJ,
        W::KeyK => Key::KeyK,
        W::KeyL => Key::KeyL,
        W::KeyM => Key::KeyM,
        W::KeyN => Key::KeyN,
        W::KeyO => Key::KeyO,
        W::KeyP => Key::KeyP,
        W::KeyQ => Key::KeyQ,
        W::KeyR => Key::KeyR,
        W::KeyS => Key::KeyS,
        W::KeyT => Key::KeyT,
        W::KeyU => Key::KeyU,
        W::KeyV => Key::KeyV,
        W::KeyW => Key::KeyW,
        W::KeyX => Key::KeyX,
        W::KeyY => Key::KeyY,
        W::KeyZ => Key::KeyZ,

        W::Digit0 => Key::Digit0,
        W::Digit1 => Key::Digit1,
        W::Digit2 => Key::Digit2,
        W::Digit3 => Key::Digit3,
        W::Digit4 => Key::Digit4,
        W::Digit5 => Key::Digit5,
        W::Digit6 => Key::Digit6,
        W::Digit7 => Key::Digit7,
        W::Digit8 => Key::Digit8,
        W::Digit9 => Key::Digit9,

        W::F1 => Key::F1,
        W::F2 => Key::F2,
        W::F3 => Key::F3,
        W::F4 => Key::F4,
        W::F5 => Key::F5,
        W::F6 => Key::F6,
        W::F7 => Key::F7,
        W::F8 => Key::F8,
        W::F9 => Key::F9,
        W::F10 => Key::F10,
        W::F11 => Key::F11,
        W::F12 => Key::F12,
        W::F24 => Key::F24,

        W::Escape => Key::Escape,
        W::Enter => Key::Enter,
        W::NumpadEnter => Key::NumpadEnter,
        W::Space => Key::Space,
        W::Tab => Key::Tab,
        W::Backspace => Key::Backspace,
        W::Delete => Key::Delete,
        W::CapsLock => Key::CapsLock,

        W::ShiftLeft => Key::ShiftLeft,
        W::ShiftRight => Key::ShiftRight,
        W::ControlLeft => Key::ControlLeft,
        W::ControlRight => Key::ControlRight,
        W::AltLeft => Key::AltLeft,
        W::AltRight => Key::AltRight,

        W::ArrowUp => Key::ArrowUp,
        W::ArrowDown => Key::ArrowDown,
        W::ArrowLeft => Key::ArrowLeft,
        W::ArrowRight => Key::ArrowRight,

        W::Home => Key::Home,
        W::End => Key::End,

        _ => return None,
    })
}

pub fn button_from_winit(button: winit::event::MouseButton) -> MouseButton {
    use winit::event::MouseButton as W;
    match button {
        W::Left => MouseButton::Left,
        W::Right => MouseButton::Right,
        W::Middle => MouseButton::Middle,
        W::Back => MouseButton::Other(3),
        W::Forward => MouseButton::Other(4),
        W::Other(n) => MouseButton::Other(n),
    }
}

/// A wheel movement in lines, whatever the device measured it in.
///
/// A wheel reports notches and a trackpad reports pixels, and the
/// hotbar wants neither -- it wants "one step". The divisor is the
/// height of a nominal line of text, which is what every toolkit uses
/// for the same conversion and what a trackpad's own scrolling is
/// tuned against.
pub fn wheel_lines(delta: winit::event::MouseScrollDelta) -> f32 {
    const PIXELS_PER_LINE: f64 = 16.0;
    match delta {
        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
        winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / PIXELS_PER_LINE) as f32,
    }
}

pub fn touch_phase_from_winit(phase: winit::event::TouchPhase) -> TouchPhase {
    use winit::event::TouchPhase as W;
    match phase {
        W::Started => TouchPhase::Started,
        W::Moved => TouchPhase::Moved,
        W::Ended => TouchPhase::Ended,
        W::Cancelled => TouchPhase::Cancelled,
    }
}
/// Whether winit can ever report this key.
///
/// The reverse of `key_from_winit`, answered by walking the codes
/// rather than by a second match, so there is no reverse table to drift
/// out of step with the forward one.
///
/// Test-only, and that is not an oversight. The game never asks: the
/// translation runs one way at runtime, and a key that cannot arrive
/// simply never arrives. The question is worth asking *once*, at build
/// time, so that `ui::keybinds` cannot offer a binding this backend
/// could never fire -- which is what the test in that module does.
#[cfg(test)]
pub fn can_produce(key: Key) -> bool {
    use winit::keyboard::KeyCode as W;
    const CANDIDATES: &[W] = &[
        W::KeyA, W::KeyB, W::KeyC, W::KeyD, W::KeyE, W::KeyF, W::KeyG,
        W::KeyH, W::KeyI, W::KeyJ, W::KeyK, W::KeyL, W::KeyM, W::KeyN,
        W::KeyO, W::KeyP, W::KeyQ, W::KeyR, W::KeyS, W::KeyT, W::KeyU,
        W::KeyV, W::KeyW, W::KeyX, W::KeyY, W::KeyZ,
        W::Digit0, W::Digit1, W::Digit2, W::Digit3, W::Digit4,
        W::Digit5, W::Digit6, W::Digit7, W::Digit8, W::Digit9,
        W::F1, W::F2, W::F3, W::F4, W::F5, W::F6, W::F7, W::F8,
        W::F9, W::F10, W::F11, W::F12, W::F24,
        W::Escape, W::Enter, W::NumpadEnter, W::Space, W::Tab,
        W::Backspace, W::Delete, W::CapsLock,
        W::ShiftLeft, W::ShiftRight, W::ControlLeft, W::ControlRight,
        W::AltLeft, W::AltRight,
        W::ArrowUp, W::ArrowDown, W::ArrowLeft, W::ArrowRight,
        W::Home, W::End,
    ];
    CANDIDATES.iter().any(|w| key_from_winit(*w) == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackpad_and_a_wheel_agree_on_which_way_is_up() {
        // A wheel counts notches and a trackpad counts pixels. The
        // hotbar only ever reads the sign, so the one thing that must
        // survive the conversion is which way is up.
        use winit::event::MouseScrollDelta;
        assert!(wheel_lines(MouseScrollDelta::LineDelta(0.0, 1.0)) > 0.0);
        assert!(wheel_lines(MouseScrollDelta::PixelDelta((0.0, 32.0).into())) > 0.0);
        assert!(wheel_lines(MouseScrollDelta::LineDelta(0.0, -1.0)) < 0.0);
        assert!(wheel_lines(MouseScrollDelta::PixelDelta((0.0, -32.0).into())) < 0.0);
    }

    #[test]
    fn a_key_the_game_has_no_name_for_is_dropped_rather_than_guessed() {
        // Better a key that does nothing than a key that does something
        // the player never asked for. See `platform::Key`.
        assert_eq!(key_from_winit(winit::keyboard::KeyCode::Pause), None);
        assert!(!can_produce(Key::AltRight) || key_from_winit(W_ALT_RIGHT) == Some(Key::AltRight));
    }

    const W_ALT_RIGHT: winit::keyboard::KeyCode = winit::keyboard::KeyCode::AltRight;
}
