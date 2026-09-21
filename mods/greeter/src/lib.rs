//! A native mod, complete, in one file.
//!
//! ## What it is for
//!
//! Two things, and the second is the important one.
//!
//! It is a **worked example**: everything a mod author needs to do at
//! least once is here, once, with the reason written next to it --
//! holding on to the host table, subscribing, reading a setting out of
//! the manifest, keeping state across a restart, cancelling an action,
//! and answering a command.
//!
//! It is also the **proof the boundary works**. A mod API that is only
//! ever exercised by the server that implements it is an API whose first
//! real user finds the mistakes. This one is built by the workspace,
//! loaded by the integration test in `primitive_server/tests/mods.rs`,
//! and if the C ABI ever stops lining up the build or that test says so
//! rather than somebody's server crashing.
//!
//! ## What it does in play
//!
//! - Greets everybody who joins, by name, with a line from the manifest.
//! - Counts how many people have ever joined, and keeps counting across
//!   a restart -- which is the whole of the save API.
//! - Refuses to let anybody break bedrock, whatever else is loaded.
//!   That is the cancellable-hook half, and it is deliberately the same
//!   thing the `bedrock_guard` *script* does: the two extension points
//!   solving one problem side by side is the clearest statement of what
//!   each is for.
//! - Answers `/greet`, which is the command half.
//!
//! ## Building it
//!
//! ```text
//! cargo build -p greeter --release
//! ```
//!
//! and copy `target/release/greeter.dll` (or `libgreeter.so`) next to
//! `mods/greeter/mod.ron`. The server looks for the platform's own name
//! -- see `primitive_modapi::manifest::platform_library_name`.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use primitive_modapi::{
    ApiVersion, Event, EventData, HookResult, HostApi, LogLevel, Status, Str, API_VERSION,
};

/// The host, kept for the life of the mod.
///
/// **A mod may hold this pointer and a mod may not hold anything it
/// points *into*.** The tables and the handle are alive for as long as
/// the mod is loaded -- the host guarantees that and its own `HostTables`
/// is what enforces it -- but a `Str` handed to a hook is borrowed for
/// that call and not one instruction longer.
///
/// An atomic rather than a `static mut`, because the server may call a
/// hook from the tick loop and a decorator from a generator thread, and
/// a `static mut` read from two threads is undefined behaviour however
/// carefully it was written.
static HOST: AtomicU64 = AtomicU64::new(0);

/// How many people have ever joined. Saved and restored; see
/// [`store_count`].
static JOINS: AtomicU64 = AtomicU64::new(0);

/// Whether the manifest asked us to guard the bedrock. Read once at
/// load, because a setting that could change under a hook is a setting
/// nobody can reason about.
///
/// Three states in one byte -- unset, off, on -- so that "we have not
/// read it yet" is distinguishable from "it is off".
static GUARD_BEDROCK: AtomicU8 = AtomicU8::new(0);
/// Zero, and never written -- it is what the atomic starts at, which is
/// the whole of what "unset" means. Named rather than left as a bare 0
/// so the three states read as three states.
#[allow(dead_code)]
const GUARD_UNSET: u8 = 0;
const GUARD_OFF: u8 = 1;
const GUARD_ON: u8 = 2;

/// The line to greet people with, out of the manifest.
///
/// A `Mutex<String>` rather than a static string, because it comes out
/// of the manifest as borrowed bytes and this mod has to *own* a copy --
/// which is the one rule about `Str` that a mod author has to internalise.
static GREETING: Mutex<String> = Mutex::new(String::new());

fn host() -> Option<&'static HostApi> {
    let raw = HOST.load(Ordering::Acquire);
    if raw == 0 {
        return None;
    }
    // Safety: the only thing that ever writes this is `load`, with the
    // pointer the host handed us, and the host keeps its tables alive
    // for as long as this library is loaded.
    Some(unsafe { &*(raw as *const HostApi) })
}

/// Writes a line to the server log.
fn log(level: LogLevel, message: &str) {
    let Some(api) = host() else { return };
    if api.core.is_null() {
        return;
    }
    unsafe {
        ((*api.core).log)(api.handle, level, Str::borrow(message));
    }
}

/// Reads one setting out of this mod's `mod.ron`.
///
/// The two-call convention every text-returning call in this API uses:
/// ask for the length with a capacity of zero, then ask again with a
/// buffer. Worth wrapping once, because getting it wrong is a truncated
/// string rather than an error.
fn setting(key: &str) -> Option<String> {
    let api = host()?;
    if api.core.is_null() {
        return None;
    }
    let mut needed: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if status == Status::NotFound || needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.core).setting)(
            api.handle,
            Str::borrow(key),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut written,
        )
    };
    if !status.is_ok() {
        return None;
    }
    buffer.truncate(written);
    String::from_utf8(buffer).ok()
}

/// Puts the join count in this mod's save blob.
///
/// Eight bytes, because that is what the state is. The host has no idea
/// what is in here and no business having one -- see
/// `primitive_modapi::SaveApi`.
fn store_count() {
    let Some(api) = host() else { return };
    if api.save.is_null() {
        return;
    }
    let bytes = JOINS.load(Ordering::Relaxed).to_le_bytes();
    unsafe {
        ((*api.save).store)(api.handle, bytes.as_ptr(), bytes.len());
    }
}

/// ...and takes it back out, at load.
fn restore_count() {
    let Some(api) = host() else { return };
    if api.save.is_null() {
        return;
    }
    let mut buffer = [0u8; 8];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.save).load)(api.handle, buffer.as_mut_ptr(), buffer.len(), &mut written)
    };
    // A fresh world has no blob, which is `NotFound` and is not an
    // error: every mod has to handle it, because it is what a new world
    // looks like.
    if status.is_ok() && written == 8 {
        JOINS.store(u64::from_le_bytes(buffer), Ordering::Relaxed);
    }
}

fn tell(player: primitive_modapi::PlayerId, text: &str) {
    let Some(api) = host() else { return };
    if api.network.is_null() {
        return;
    }
    unsafe {
        ((*api.network).tell)(api.handle, player, Str::borrow(text));
    }
}

/// Called once, after every mod is registered and the load order is
/// settled.
///
/// **This is where subscribing happens, and it is separate from the
/// entry point on purpose**: at the entry point the other mods do not
/// exist yet, and a mod that wanted to notice one of them would have
/// nothing to look at.
unsafe extern "C" fn load(host_api: *const HostApi) -> Status {
    if host_api.is_null() {
        return Status::BadArgument;
    }
    // Refused rather than trusted. The host checks this too -- both,
    // because a mod loaded by an older host would otherwise read past
    // the end of these tables before it ever got the chance to complain.
    let version = (*host_api).version;
    if !compatible(version) {
        return Status::Refused;
    }
    HOST.store(host_api as u64, Ordering::Release);

    if let Some(line) = setting("greeting") {
        *GREETING.lock().unwrap_or_else(|e| e.into_inner()) = line;
    }
    GUARD_BEDROCK.store(
        match setting("guard_bedrock").as_deref() {
            Some("false") => GUARD_OFF,
            _ => GUARD_ON,
        },
        Ordering::Relaxed,
    );

    restore_count();

    let api = &*host_api;
    if api.events.is_null() {
        return Status::Unavailable;
    }
    // A mod that subscribes to nothing is never called, which is what
    // makes a dozen loaded mods cost nothing on a tick none of them
    // cares about. So: exactly what this one uses.
    for event in [
        Event::PlayerJoined,
        Event::PlayerLeft,
        Event::BlockBreak,
        Event::Command,
        Event::ServerStopping,
    ] {
        ((*api.events).subscribe)(api.handle, event);
    }
    ((*api.events).register_command)(
        api.handle,
        Str::borrow("greet"),
        Str::borrow("say hello to everybody"),
    );

    log(
        LogLevel::Info,
        &format!(
            "greeter ready against API {version}, {} join(s) remembered",
            JOINS.load(Ordering::Relaxed)
        ),
    );
    Status::Ok
}

/// Whether this mod can run against a host at `version`.
///
/// The mod's side of the same rule the host applies: same major, and the
/// host's minor no older than the one this was built against.
fn compatible(host_version: ApiVersion) -> bool {
    host_version.accepts(API_VERSION)
}

unsafe extern "C" fn event(what: Event, data: *const EventData) -> HookResult {
    let Some(data) = data.as_ref() else {
        return HookResult::Continue;
    };
    match what {
        Event::PlayerJoined => {
            let n = JOINS.fetch_add(1, Ordering::Relaxed) + 1;
            store_count();
            let greeting = GREETING.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let greeting = if greeting.is_empty() {
                "welcome".to_string()
            } else {
                greeting
            };
            tell(data.player, &format!("{greeting} -- you are visitor {n}"));
            HookResult::Continue
        }

        Event::PlayerLeft => HookResult::Continue,

        // **The cancellable half.** Returning `Cancel` stops the break
        // from happening at all, before the world changes -- which is
        // the whole reason the hook runs inside the edit path rather
        // than watching from the side.
        Event::BlockBreak => {
            if GUARD_BEDROCK.load(Ordering::Relaxed) != GUARD_ON {
                return HookResult::Continue;
            }
            // The floor of the world. Two cells, matching what the
            // generator lays down.
            if data.pos.y <= 2 {
                tell(data.player, "the floor of the world stays where it is");
                return HookResult::Cancel;
            }
            HookResult::Continue
        }

        // `/greet`. Returning `Cancel` is how a mod says "I handled
        // this" -- the same convention the scripted plugins use, so a
        // player does not get "no such command" for one this answered.
        Event::Command => {
            let name = data.text.as_str();
            if name != "greet" {
                return HookResult::Continue;
            }
            let Some(api) = host() else {
                return HookResult::Continue;
            };
            if !api.network.is_null() {
                let n = JOINS.load(Ordering::Relaxed);
                ((*api.network).broadcast)(
                    api.handle,
                    Str::borrow(&format!("hello from the greeter mod -- {n} visitor(s) so far")),
                );
            }
            HookResult::Cancel
        }

        // Last chance to write anything down. The host saves after this
        // and not before, which is what makes it a last chance rather
        // than a formality.
        Event::ServerStopping => {
            store_count();
            HookResult::Continue
        }

        _ => HookResult::Continue,
    }
}

// The exported symbol, written by the macro. One function, one name, and
// the host finds it by that name and nothing else.
primitive_modapi::declare_mod! {
    name: "greeter",
    version: "1.0.0",
    load: load,
    event: event,
}

/// A mod cannot be unloaded halfway and cannot leave the host holding a
/// pointer into it, so there is nothing to do here -- but the field
/// exists on the descriptor and a mod with real resources would use it.
/// Said rather than left blank, because "why is `on_unload` `None`" is
/// exactly the question the next person reading this will have.
const _: () = ();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_refuses_a_host_it_cannot_talk_to() {
        // The mod's half of the version handshake. The host checks too;
        // both check, because a mod loaded by an *older* host would
        // otherwise read past the end of a table before it ever got the
        // chance to complain.
        assert!(compatible(API_VERSION));
        assert!(compatible(ApiVersion::new(
            API_VERSION.major,
            API_VERSION.minor + 3
        )));
        assert!(!compatible(ApiVersion::new(API_VERSION.major + 1, 0)));
        // A host older in the *minor* is refused too. Written against a
        // fixed pair rather than against `API_VERSION.minor - 1`, which
        // underflows at minor zero and makes the assertion trivially
        // true -- a test that passes because it stopped meaning
        // anything.
        assert!(!ApiVersion::new(3, 1).accepts(ApiVersion::new(3, 2)));
        assert!(ApiVersion::new(3, 2).accepts(ApiVersion::new(3, 1)));
    }

    #[test]
    fn every_host_call_survives_having_no_host() {
        // Every one of these is reachable before `load` has run -- a
        // host that called an event first, a test that calls one
        // directly -- and none of them may dereference a null table. The
        // release profile aborts on panic, so "it would have panicked"
        // is not a consolation.
        assert!(host().is_none());
        log(LogLevel::Info, "this goes nowhere");
        assert_eq!(setting("greeting"), None);
        store_count();
        restore_count();
        tell(1, "nobody");
    }

    #[test]
    fn a_null_payload_is_not_a_crash() {
        // The host never sends one, and "the host never does that" is
        // not a memory-safety argument.
        let answer = unsafe { event(Event::PlayerJoined, std::ptr::null()) };
        assert_eq!(answer, HookResult::Continue);
    }

    #[test]
    fn the_bedrock_guard_stops_at_the_floor_and_nowhere_else() {
        GUARD_BEDROCK.store(GUARD_ON, Ordering::Relaxed);
        let at = |y: i32| {
            let data = EventData {
                pos: primitive_modapi::BlockPos { x: 0, y, z: 0 },
                ..EventData::default()
            };
            unsafe { event(Event::BlockBreak, &data) }
        };
        assert_eq!(at(0), HookResult::Cancel);
        assert_eq!(at(2), HookResult::Cancel);
        assert_eq!(at(3), HookResult::Continue);
        assert_eq!(at(40), HookResult::Continue);

        // ...and the setting turns it off rather than being decoration.
        GUARD_BEDROCK.store(GUARD_OFF, Ordering::Relaxed);
        assert_eq!(at(0), HookResult::Continue);
        GUARD_BEDROCK.store(GUARD_ON, Ordering::Relaxed);
    }

    #[test]
    fn an_unknown_command_is_left_for_somebody_else() {
        // Cancelling here would mean this mod swallowed every unknown
        // command on the server, which is how one mod breaks every other
        // one's `/home`.
        let name = "something_else";
        let data = EventData {
            text: Str::borrow(name),
            ..EventData::default()
        };
        assert_eq!(unsafe { event(Event::Command, &data) }, HookResult::Continue);
    }
}
