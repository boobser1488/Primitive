//! What happens when something goes wrong, and where the player finds
//! out about it.
//!
//! A game is not run from a terminal. Printing to stderr and exiting is
//! the same as vanishing without explanation: the window disappears, the
//! console it would have printed to was never open, and the player has
//! nothing to report but "it closed".
//!
//! So every failure that reaches this module does two things: it writes
//! `crash.log` next to the executable, and it says so on the way out.
//! The log is the part that matters -- it is the only artefact that
//! survives the process, and it is what a bug report can actually
//! contain.
//!
//! Failures that *don't* reach here are the ones handled in the game:
//! a refused connection and a kick both go back to the menu with the
//! reason on screen, because neither is a reason to close a game.

use std::io::Write;
use std::path::PathBuf;

/// Where the log goes: next to the executable, falling back to the
/// working directory.
///
/// Next to the executable because that is the folder the player
/// unzipped and the one they can find again. The working directory of a
/// double-clicked game is wherever the shell felt like.
pub fn log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(FILENAME)))
        .unwrap_or_else(|| PathBuf::from(FILENAME))
}

const FILENAME: &str = "crash.log";

/// Installs the panic handler.
///
/// Keeps the default hook and adds to it rather than replacing it: the
/// default prints a backtrace when `RUST_BACKTRACE` is set, which is
/// exactly what a developer wants and what the file cannot replace.
pub fn install_panic_handler() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = describe_panic(info);
        write_log("panic", &message);
        eprintln!("\n{}", banner(&message));
        previous(info);
    }));
}

/// Records a fatal error that isn't a panic -- a missing GPU, a window
/// that wouldn't open, a runtime that wouldn't start.
pub fn report_fatal(context: &str, error: &dyn std::fmt::Display) {
    let message = format!("{context}: {error}");
    write_log("error", &message);
    eprintln!("\n{}", banner(&message));
}

fn banner(message: &str) -> String {
    format!(
        "Primitive has stopped.\n\n  {message}\n\nWritten to {}\n",
        log_path().display()
    )
}

/// Appends an entry. Appends rather than truncates so a crash that only
/// happens every tenth launch still has its predecessors for company,
/// and capped so a crash loop cannot fill a disk.
fn write_log(kind: &str, message: &str) {
    const MAX_BYTES: u64 = 256 * 1024;

    let path = log_path();
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let _ = std::fs::remove_file(&path);
    }

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return; // read-only install directory; the stderr copy is all we have
    };

    let _ = writeln!(
        file,
        "---- {kind} | Primitive {} | {} {} ----\n{message}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
}

fn describe_panic(info: &std::panic::PanicHookInfo<'_>) -> String {
    // `payload` is a `&str` for `panic!("...")` and a `String` for
    // `panic!("{}", x)`; neither is reachable without asking for both.
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_string());

    match info.location() {
        Some(location) => format!("{payload}\n  at {}:{}", location.file(), location.line()),
        None => payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_sits_next_to_the_executable() {
        // Not the working directory: a double-clicked game inherits
        // whatever the shell felt like, and the player cannot find it.
        let path = log_path();
        assert_eq!(path.file_name().unwrap(), FILENAME);
        if let Ok(exe) = std::env::current_exe() {
            assert_eq!(path.parent(), exe.parent());
        }
    }

    #[test]
    fn the_banner_says_where_to_look() {
        // The message on the way out has to name the file, or nobody
        // will know there is one.
        let text = banner("something went wrong");
        assert!(text.contains("something went wrong"));
        assert!(text.contains(FILENAME));
    }
}
