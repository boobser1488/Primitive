//! The desktop program.
//!
//! Three lines, and that is the whole of it. The game is a library --
//! see `lib.rs` -- because Android does not run programs: it loads a
//! shared object into a process the system already started and calls
//! `android_main` inside it. Rather than keep two arrangements of the
//! same code, there is one library and two thin ways in, and this is
//! the one a desktop uses.

fn main() -> std::process::ExitCode {
    primitive::desktop_main()
}
