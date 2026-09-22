//! Getting the game onto a phone.
//!
//! Everything in this file exists because of one difference between a
//! desktop and an Android package: **on a phone the game's own files are
//! not files.** The textures, the block table, the sounds -- all of it
//! is inside the APK, which is a zip the system mounts read-only and
//! hands out through an asset manager. `std::fs::read` cannot see any of
//! it, and fifty-four places in this client call `std::fs::read`.
//!
//! ## Why unpack rather than abstract
//!
//! The obvious fix is a virtual filesystem: a trait with `read` on it,
//! implemented once over `std::fs` and once over the asset manager, and
//! every call site changed to go through it. That is the right shape
//! and the wrong trade. It touches fifty-four places to buy a property
//! -- "assets may live anywhere" -- that exactly one platform needs, and
//! every one of those fifty-four is a chance to change behaviour on the
//! platform that already worked.
//!
//! So the assets are copied out of the APK into the app's own directory
//! once, on first run, and every existing call site keeps working
//! unchanged. It costs about three and a half megabytes of the phone's
//! storage and a second of the first launch, and it costs the desktop
//! nothing at all, because none of this is compiled there.
//!
//! **That was before `embedded.rs`, and today the list is empty.** Every
//! texture, recording and model is compiled into the library and read
//! from there when no file on disk replaces it, so unpacking them only
//! put a second copy in the APK and a third on the phone. The machinery
//! stays for a file that one day cannot be embedded; see
//! `package-android.sh` for the manifest and `bootstrap` for how an
//! upgraded install sheds the copies an older package unpacked.
//!
//! ## Why a manifest
//!
//! The asset manager can open a path but cannot reliably *list* one --
//! directory enumeration inside an APK works on some Android versions
//! and returns nothing on others, depending on how the packager stored
//! the entries. Rather than depend on that, the packaging script writes
//! `assets/MANIFEST`: one relative path per line, generated from the
//! same directory walk that put the files in. Reading a list is a thing
//! that works everywhere.
//!
//! ## Why the working directory moves
//!
//! The client's settings file and its saved worlds are named relatively
//! -- `client_settings.toml`, `saves/` -- and resolve against the
//! process's working directory. On a desktop that is wherever the
//! player launched the game, which is what they expect. On Android the
//! working directory is `/`, which the app cannot write to. Moving it
//! to the app's own data directory once, before anything reads a
//! setting, makes every one of those relative paths land somewhere
//! writable without any of them being changed.

use std::io::Write;
use std::path::{Path, PathBuf};

// winit's re-export, not a direct dependency: see the note in
// `Cargo.toml` about there being only one `AndroidApp` in a build.
use winit::platform::android::activity::AndroidApp;

/// Sends everything the game prints to `adb logcat`.
///
/// ## Why this is not optional
///
/// A Rust program's stdout on Android goes nowhere. Not to logcat, not
/// to a file, not to a console -- the system never gave the process
/// one, and `println!` writes into a descriptor that is closed. So the
/// game's whole account of itself -- which GPU it found, how many
/// textures loaded, and above all *what went wrong* -- is invisible,
/// and an app that fails to start fails silently.
///
/// That is the difference between a bug a player can report and a black
/// screen nobody can do anything about, which is why this runs before
/// anything else in `android_main`.
///
/// ## How
///
/// A pipe, with stdout and stderr both pointed at the writing end, and
/// a thread reading the other end and handing each line to the system
/// log. Line-buffered on purpose: logcat is a line-oriented log, and a
/// half-line written now and finished later would arrive as two
/// entries.
pub fn log_to_logcat() {
    // The log level `println!` output is filed under. 4 is INFO in
    // Android's `android_LogPriority`. Everything goes to one level:
    // the game already distinguishes ordinary output from trouble by
    // what it says, and guessing severity from the text would get it
    // wrong in both directions.
    const INFO: i32 = 4;
    const TAG: &str = "Primitive\0";

    extern "C" {
        fn __android_log_write(prio: i32, tag: *const libc::c_char, text: *const libc::c_char)
            -> libc::c_int;
    }

    let mut fds = [0; 2];
    // SAFETY: `pipe` fills two ints, which is exactly what it is given.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    let (read_end, write_end) = (fds[0], fds[1]);
    // SAFETY: both descriptors are ours, and `dup2` on a valid pair is
    // defined. Failure is ignored: it would mean the process has no
    // stdout to replace, which is the situation this is fixing anyway.
    unsafe {
        libc::dup2(write_end, libc::STDOUT_FILENO);
        libc::dup2(write_end, libc::STDERR_FILENO);
    }

    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        use std::os::fd::FromRawFd;

        // SAFETY: `read_end` came from `pipe` above and is not used
        // anywhere else, so this is the only owner.
        let reader = BufReader::new(unsafe { std::fs::File::from_raw_fd(read_end) });
        for line in reader.lines() {
            let Ok(line) = line else { break };
            // A line with a NUL in it cannot be a C string. Dropped
            // rather than truncated: nothing this game prints has one,
            // so a line that does is a bug worth not hiding behind a
            // half-message.
            let Ok(text) = std::ffi::CString::new(line) else {
                continue;
            };
            // SAFETY: both pointers are NUL-terminated C strings that
            // outlive the call.
            unsafe {
                __android_log_write(INFO, TAG.as_ptr().cast(), text.as_ptr());
            }
        }
    });
}

/// The file listing everything else. See the module note.
const MANIFEST: &str = "MANIFEST";

/// Written beside the unpacked assets, holding the game's version.
///
/// What makes an upgrade replace them. Without it the first run after
/// an update would keep the old textures -- the directory exists, so
/// nothing would think to look -- and the player would get a new game
/// wearing the old one's clothes.
const STAMP: &str = ".unpacked-version";

/// Prepares the app's own directory and returns where the assets ended
/// up.
///
/// Called once, before anything reads a setting or a texture.
pub fn bootstrap(app: &AndroidApp) -> anyhow::Result<PathBuf> {
    let data_dir = app
        .internal_data_path()
        .ok_or_else(|| anyhow::anyhow!("the activity has no internal data directory"))?;
    std::fs::create_dir_all(&data_dir)?;

    // Everything relative -- the settings file, the saves folder --
    // now lands here. See the module note on why this is a working
    // directory rather than fifty-four edited paths.
    std::env::set_current_dir(&data_dir)?;

    // Before anything can read one. `PRIMITIVE_AUTOSTART` is consulted
    // partway through `run`, which is far too late to be set by
    // whatever calls this.
    read_env_file(&data_dir);

    let assets_dir = data_dir.join("assets");
    let version = env!("CARGO_PKG_VERSION");
    if unpacked_version(&assets_dir).as_deref() == Some(version) {
        return Ok(assets_dir);
    }

    // **The previous version's files go first.** Up to 1.5.0 every asset
    // was unpacked here, and a file here wins over the copy compiled into
    // the library. Packages now unpack nothing (everything is embedded --
    // see `package-android.sh`), so without this an upgraded phone would
    // keep last version's pictures and recordings on top of the new ones
    // for as long as the app stayed installed, and keep paying 4 MB of
    // storage for them. Nothing else lives in this directory: the saves
    // and settings are one level up, and nobody but this function writes
    // here.
    if assets_dir.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(&assets_dir) {
            eprintln!("could not clear the old unpacked assets ({e}); they may shadow the built-in ones");
        }
    }

    println!("unpacking assets for {version}...");
    let (unpacked, wanted) = unpack_assets(app, &assets_dir)?;
    println!("unpacked {unpacked} of {wanted} file(s) into {}", assets_dir.display());

    // **The stamp is a record that it worked, not that it was tried.**
    //
    // It used to be written unconditionally, and that turned one bad
    // build into a permanently broken install: a packaging bug meant
    // every single asset failed to open, the game fell back to its
    // built-in textures, and the stamp said 1.5.0 anyway -- so the next
    // launch, with the bug fixed, skipped unpacking entirely and used
    // the built-ins again. The only way out was to clear the app's
    // data, which is not something a player will think of.
    if unpacked == wanted {
        std::fs::write(assets_dir.join(STAMP), version)?;
    } else {
        eprintln!(
            "{} file(s) did not arrive; not marking the unpack done, so the next start retries",
            wanted - unpacked,
        );
    }
    Ok(assets_dir)
}

/// Where an Android process's environment variables come from.
///
/// ## Why a file
///
/// `PRIMITIVE_AUTOSTART` and `PRIMITIVE_BENCH` are how the game is
/// measured without a person driving it: one opens a named world and
/// turns the F3 dump on, the other quits after so many seconds, and
/// between them "is this change faster" is a question a script can
/// answer. On a desktop they are typed in front of the command.
///
/// There is no command on Android. The activity is started by the
/// system with an environment nobody chose, `am start` has no way to
/// add to it, and the one channel that does exist -- intent extras --
/// is reachable only through JNI, which this package has no Java to
/// hold. So the variables arrive the way everything else the app is
/// told arrives: as a file in its own directory, written with
/// `adb shell run-as com.primitive.game`.
///
/// ## Why only `PRIMITIVE_`
///
/// Everything here is set into the process's own environment, and the
/// environment is read by more than this game: `LD_PRELOAD` and
/// `LD_LIBRARY_PATH` are the loader's, and a file that could set them
/// would turn "anything that can write in the app's directory" into
/// "anything that can run code in the app's process". The prefix keeps
/// the file to variables this game invented.
///
/// Missing is the normal case and says nothing. A shipped game has no
/// such file, and one that appears is somebody measuring.
const ENV_FILE: &str = "primitive.env";

/// The second place the file is looked for, because on some phones the
/// first one cannot be written at all.
///
/// `run-as` is supposed to make the app's own directory reachable from
/// a workstation, and on the device this was developed against it does
/// not: MIUI runs SELinux enforcing with a policy that lets the
/// `runas_app` context *read* the app's data and refuses every write.
/// `adb shell run-as com.primitive.game sh -c 'echo hi > files/x'`
/// answers `Permission denied`, and so does the same line aimed at
/// `cache/`. There is no flag that turns this on; the package being
/// `--debuggable` is what makes `run-as` work at all and it is already
/// set.
///
/// That left the one hook this game has for being measured without a
/// person holding it unreachable on the only phone there is to measure
/// on -- and a hook nobody can reach is the same as no hook. So the
/// file is also looked for in `/data/local/tmp`, which belongs to the
/// shell user and which `adb push` can write.
///
/// **This does not widen who can set these variables.** Reaching
/// `/data/local/tmp` means holding an authorised adb session, which is
/// already enough to install a different build of the game entirely.
/// An app on the device cannot write there; the directory is the shell
/// user's. And the `PRIMITIVE_` rule below still applies to both
/// files, so what either one can say is bounded by the same list.
///
/// The app's own directory is still tried first, so a phone where
/// `run-as` does work behaves exactly as it did.
const SHELL_ENV_FILE: &str = "/data/local/tmp/primitive.env";

/// Reads `primitive.env` into the process environment, if it is there.
///
/// One `KEY=VALUE` per line; `#` comments and blank lines ignored.
fn read_env_file(data_dir: &Path) {
    let own = data_dir.join(ENV_FILE);
    // First the app's own copy, then the shell's. Only one is read: two
    // files disagreeing about `PRIMITIVE_AUTOSTART` is a coin toss, and
    // a measurement that depends on a coin toss is not a measurement.
    let found = std::fs::read_to_string(&own)
        .map(|text| (own.display().to_string(), text))
        .or_else(|_| {
            std::fs::read_to_string(SHELL_ENV_FILE)
                .map(|text| (SHELL_ENV_FILE.to_string(), text))
        });
    let Ok((from, text)) = found else {
        return;
    };
    println!("{ENV_FILE}: reading {from}");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            eprintln!("{ENV_FILE}: not a KEY=VALUE line, ignored: {line}");
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !key.starts_with("PRIMITIVE_") {
            eprintln!("{ENV_FILE}: refusing to set {key}; only PRIMITIVE_* may be set here");
            continue;
        }
        println!("{ENV_FILE}: {key}={value}");
        // Safe here in a way it would not be later: this runs on the
        // only thread there is except the logcat pump, which never
        // reads the environment. `set_var` races with any concurrent
        // `getenv` in the process, and after `start` there are worker
        // threads and a tokio runtime for it to race with.
        std::env::set_var(key, value);
    }
}

/// Which version of the game last unpacked into this directory, if any.
fn unpacked_version(assets_dir: &Path) -> Option<String> {
    std::fs::read_to_string(assets_dir.join(STAMP))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Copies every file the manifest names out of the APK.
///
/// Returns how many arrived and how many were asked for. A file the
/// manifest names but the package does not hold is reported and skipped
/// rather than fatal: a texture missing from a build is a pink square
/// in the game, which the player can at least tell you about, where
/// refusing to start is a phone that does nothing. The caller compares
/// the two numbers to decide whether to remember the unpack as done --
/// see `bootstrap`.
fn unpack_assets(app: &AndroidApp, into: &Path) -> anyhow::Result<(usize, usize)> {
    use std::ffi::CString;

    let manager = app.asset_manager();
    let manifest_path = CString::new(MANIFEST)?;
    let mut manifest = manager
        .open(&manifest_path)
        .ok_or_else(|| anyhow::anyhow!("the package has no {MANIFEST}; was it built by package.sh --android?"))?;
    let manifest = {
        let mut text = String::new();
        use std::io::Read;
        manifest.read_to_string(&mut text)?;
        text
    };

    std::fs::create_dir_all(into)?;
    let mut written = 0usize;
    let mut wanted = 0usize;
    for line in manifest.lines() {
        let relative = line.trim();
        // Blank lines and comments, so the manifest can be read by a
        // person trying to work out what is in a build.
        if relative.is_empty() || relative.starts_with('#') {
            continue;
        }
        wanted += 1;
        // A path that climbs out of the assets directory is either a
        // broken packaging script or a hostile package, and the
        // difference does not matter: refuse both. Without this a
        // manifest line of `../../../databases/x` would have the game
        // write wherever it liked inside its own sandbox.
        if relative.contains("..") || Path::new(relative).is_absolute() {
            eprintln!("refusing manifest entry that leaves the assets directory: {relative}");
            continue;
        }

        let Ok(asset_path) = CString::new(relative) else {
            eprintln!("manifest entry is not a usable path: {relative}");
            continue;
        };
        let Some(mut asset) = manager.open(&asset_path) else {
            eprintln!("the package does not hold {relative}");
            continue;
        };

        let target = into.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = {
            use std::io::Read;
            let mut buffer = Vec::new();
            asset.read_to_end(&mut buffer)?;
            buffer
        };
        let mut file = std::fs::File::create(&target)?;
        file.write_all(&bytes)?;
        written += 1;
    }
    Ok((written, wanted))
}

#[cfg(test)]
mod tests {
    // Nothing here runs off a phone, and a test that cannot run is worse
    // than no test: it looks like coverage. The one thing worth checking
    // without an activity is the manifest guard, which is pure string
    // work -- see `refuses_paths_that_climb_out`.

    /// The check `unpack_assets` makes on every manifest line, alone.
    ///
    /// Duplicated deliberately rather than extracted: pulling it into a
    /// function that both the loop and the test call would mean the
    /// loop's guard could be edited without this failing, which is the
    /// opposite of what the test is for. It is three lines.
    fn would_refuse(relative: &str) -> bool {
        relative.contains("..") || std::path::Path::new(relative).is_absolute()
    }

    #[test]
    fn refuses_paths_that_climb_out_of_the_assets_directory() {
        assert!(would_refuse("../../../databases/settings"));
        assert!(would_refuse("textures/../../escape.png"));
        assert!(would_refuse("/etc/passwd"));

        assert!(!would_refuse("textures/terrain/stone.png"));
        assert!(!would_refuse("blocks.toml"));
    }
}

/// Tells the compositor what frame rate this window intends to produce.
///
/// **The third attempt at one problem, and the first two are worth
/// writing down because each looked like the answer.** The symptom was
/// a phone whose panel does 60, 90 and 120, sitting at 120 on the
/// launcher, and dropping to 60 the moment the game presented under
/// vsync. Everything else followed from that one fact and each step
/// looked like its own bug: a 60 Hz panel means FIFO can only hand over
/// 60 frames a second; at 60 the GPU idles, so its governor drops the
/// clock; and the *same* terrain pass then measured 9.7 ms instead of
/// 5.9, which looks exactly like a phone that cannot hold 120.
///
/// Attempt one was `WindowManager.LayoutParams.preferredDisplayModeId`
/// in `PrimitiveActivity`, set once at `onSetUpWindow`. It never went
/// out: the code returned early because the panel was *already* at 120
/// when the activity was set up. Attempt two removed that early return
/// and logged the request. The log proves it is now sent --
/// `[display] asked for mode 2 at 120 Hz (was mode 1 at 60 Hz)` -- and
/// `dumpsys display` proves it is refused: `renderFrameRate 60`. On
/// this device the window's preferred mode is not what decides.
///
/// So this asks the other way, about the *surface* rather than the
/// window. `ANativeWindow_setFrameRate` is the API meant for exactly
/// this question, and `FIXED_SOURCE` says the number is what the
/// content actually produces rather than a wish. The change strategy is
/// `ALWAYS`: a 60-to-120 switch on this panel is not seamless, and
/// `ONLY_IF_SEAMLESS` -- which is what the plain
/// `ANativeWindow_setFrameRate` implies -- is very likely why the
/// window-level request was ignored in the first place.
///
/// ## Why `dlsym` rather than a dependency
///
/// `primitive_client/Cargo.toml` says, with reasons, that neither
/// `ndk` nor `android-activity` is named there: winit already depends
/// on both, and naming them again is how a graph ends up with two
/// versions and an `AndroidApp` that is not the one winit's event loop
/// accepts. Adding `ndk` for one function would put that decision at
/// risk to save nothing -- the function lives in `libandroid.so`, which
/// is loaded into this process already.
///
/// It also removes the API-level problem. The symbol arrived in API 30
/// (31 for the change-strategy form) and this package installs from 24.
/// Linking it would mean building against a newer stub and hoping;
/// looking it up means asking the device that is actually running the
/// game, and doing nothing on the ones that answer no.
///
/// Asked for 120 rather than "the maximum": the number is a statement
/// about what the game intends to produce, and the compositor picks a
/// mode that fits it -- 60 on a 60 Hz panel, since 120 is a multiple of
/// it. A 144 Hz panel will therefore be asked for 120 and settle at
/// 120, which is a limitation and not a bug: this is the rate the game
/// aims at.
#[cfg(target_os = "android")]
pub fn ask_for_a_fast_frame_rate() {
    use std::ffi::{c_void, CString};

    const WANTED_FPS: f32 = 120.0;
    // `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_FIXED_SOURCE`.
    const FIXED_SOURCE: i8 = 1;
    // `ANATIVEWINDOW_CHANGE_FRAME_RATE_ALWAYS`.
    const ALWAYS: i8 = 1;

    let Some(app) = super::android_app() else {
        return;
    };
    let Some(window) = app.native_window() else {
        // No surface yet. The caller retries on the next `Resumed`,
        // which is the only moment a surface is known to exist.
        return;
    };
    let handle = window.ptr().as_ptr().cast::<c_void>();

    // **Asked of `libandroid.so` by name, and the null handle is why.**
    //
    // The first version passed `RTLD_DEFAULT` -- a null handle, which
    // on a 64-bit Android means "search everything already loaded".
    // On a device that answered `this device has no
    // ANativeWindow_setFrameRate`, on a phone whose Android is new
    // enough to have had it for four releases. Bionic's null handle
    // searches the *global* group: libraries opened with `RTLD_GLOBAL`.
    // `libandroid.so` is here as this library's own `DT_NEEDED`
    // dependency, which puts it in a local group and out of that
    // search. Naming it finds it, and `dlopen` on something already
    // mapped is a reference count rather than a load.
    //
    // The handle is deliberately never closed: it belongs to the
    // process for as long as there is a window to ask about, and
    // closing it would be a decrement for a library the whole graphics
    // stack is using.
    let Ok(soname) = CString::new("libandroid.so") else {
        return;
    };
    // SAFETY: a valid C string and a flag the linker defines; a
    // failure is a null return.
    let library = unsafe { libc::dlopen(soname.as_ptr(), libc::RTLD_NOW) };
    if library.is_null() {
        println!("[display] libandroid.so could not be opened for the frame-rate call");
        return;
    }
    let lookup = |name: &str| -> *mut c_void {
        let Ok(symbol) = CString::new(name) else {
            return std::ptr::null_mut();
        };
        // SAFETY: a live handle from `dlopen` and a valid C string; a
        // missing symbol is a null return, not a fault.
        unsafe { libc::dlsym(library, symbol.as_ptr()) }
    };

    let with_strategy = lookup("ANativeWindow_setFrameRateWithChangeStrategy");
    if !with_strategy.is_null() {
        // SAFETY: the signature is the NDK's, the handle came from a
        // live `NativeWindow`, and the two small integers are the
        // constants above.
        let call: extern "C" fn(*mut c_void, f32, i8, i8) -> i32 =
            unsafe { std::mem::transmute(with_strategy) };
        let status = call(handle, WANTED_FPS, FIXED_SOURCE, ALWAYS);
        println!("[display] surface frame rate {WANTED_FPS} asked with strategy: status {status}");
        return;
    }

    let plain = lookup("ANativeWindow_setFrameRate");
    if !plain.is_null() {
        // SAFETY: as above, one argument shorter.
        let call: extern "C" fn(*mut c_void, f32, i8) -> i32 =
            unsafe { std::mem::transmute(plain) };
        let status = call(handle, WANTED_FPS, FIXED_SOURCE);
        println!("[display] surface frame rate {WANTED_FPS} asked: status {status}");
        return;
    }

    // Neither exists: a device older than API 30. Nothing to do, and
    // said out loud so a report from such a phone is readable.
    println!("[display] this device has no ANativeWindow_setFrameRate");
}
