//! `/fly` — flight, granted by a mod rather than built into the game.
//!
//! ## Why this is a mod and what the engine had to grow first
//!
//! Flight cannot be a pure mod on this server, and finding that out is
//! most of what this mod is worth reading for.
//!
//! The client runs gravity locally — it has to, or every step would cost
//! a round trip — so nothing a server-side mod does can stop a player
//! falling. The obvious workaround is to teleport them back up on every
//! tick, and it fails three ways at once: `PlayerHandle::teleport` zeroes
//! the client's velocity, so the player can never build up horizontal
//! speed either and ends up pinned; airborne acceleration is capped at
//! 1.6 blocks a second, so even without that they could barely steer;
//! and every correction puts "moved back" on the player's screen, which
//! at twenty a second is a permanent alarm.
//!
//! So the engine grew the capability and the mod owns the policy. That
//! is the split this file is an argument for:
//!
//! | | who decides |
//! |---|---|
//! | *can a client stop applying gravity* | the engine — `ServerMessage::Flight` |
//! | *is that cheating* | the engine — the anti-cheat is told, and keeps checking everything else |
//! | *who may fly, how fast, and for how long* | **this mod** |
//!
//! One call was added to the mod API for it, `PlayersApi::set_flying`,
//! and everything below is built out of calls that were already there.
//!
//! ## What it does in play
//!
//! | | |
//! |---|---|
//! | `/fly` | turn your own flight on or off |
//! | `/fly on`, `/fly off` | say which, rather than toggling |
//! | `/fly 20` | fly, at twenty blocks a second |
//! | `/fly <player>` | **operators only** — toggle somebody else's |
//! | `/fly <player> <speed>` | operators only |
//! | `/flyspeed <n>` | change speed without touching the switch |
//!
//! While flying: **jump goes up, sprint goes down**, and letting go of
//! both holds height. Blocks are still solid — flight is not noclip, and
//! the two are deliberately separate powers.
//!
//! ## Building it
//!
//! ```text
//! cargo build -p flight --release
//! ```
//!
//! and copy `target/release/flight.dll` (or `libflight.so`) next to
//! `mods/flight/mod.ron`. The server looks for the platform's own name —
//! see `primitive_modapi::manifest::platform_library_name`.
//!
//! **Dedicated servers only.** The client embeds `primitive_server` with
//! `default-features = false`, which turns the `mods` feature off, so a
//! singleplayer world has no mod host to load this into. That is a
//! deliberate decision of the game's rather than a limitation of this
//! mod — see the note in `primitive_server/Cargo.toml`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

use primitive_modapi::{
    ApiVersion, Event, EventData, HookResult, HostApi, LogLevel, PlayerId, Status, Str,
    API_VERSION,
};

/// The host, kept for the life of the mod. See the note on the same
/// static in the greeter: a mod may hold this pointer and may not hold
/// anything it points *into*.
static HOST: AtomicU64 = AtomicU64::new(0);

/// The speed out of the manifest, as `f32` bits. Read once at load,
/// because a setting that could change under a hook is a setting nobody
/// can reason about.
static SPEED: AtomicU32 = AtomicU32::new(0);

static OPERATORS_ONLY: AtomicBool = AtomicBool::new(true);
static REMEMBER: AtomicBool = AtomicBool::new(true);
static DROP_ON_DEATH: AtomicBool = AtomicBool::new(true);

/// Who is flying, by **name**, and how fast.
///
/// By name rather than by [`PlayerId`], and that is not a detail: an id
/// is a *connection* number, handed out fresh on every join and reused
/// once it is free. A remembered list of ids would grant flight to
/// whoever happened to reconnect into the same slot, which is a
/// permissions bug that would take an afternoon to reproduce.
///
/// Holds people who are not connected. That is what makes `remember`
/// work across a restart, and it is why the entry is keyed on something
/// that outlives a session.
static FLYING: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

/// The default speed, if the manifest says nothing sensible. The host
/// clamps whatever it is given, so this only has to be plausible.
const FALLBACK_SPEED: f32 = 12.0;

/// The widest speed a player may ask for. The host clamps to 1..80 of
/// its own accord; this is the mod refusing before it asks, so a typo
/// gets an answer rather than a silent clamp.
const MIN_SPEED: f32 = 1.0;
const MAX_SPEED: f32 = 80.0;

fn host() -> Option<&'static HostApi> {
    let raw = HOST.load(Ordering::Acquire);
    if raw == 0 {
        return None;
    }
    // Safety: only `load` ever writes this, with the pointer the host
    // handed us, and the host keeps its tables alive for as long as this
    // library is loaded.
    Some(unsafe { &*(raw as *const HostApi) })
}

fn log(level: LogLevel, message: &str) {
    let Some(api) = host() else { return };
    if api.core.is_null() {
        return;
    }
    unsafe {
        ((*api.core).log)(api.handle, level, Str::borrow(message));
    }
}

fn tell(player: PlayerId, text: &str) {
    let Some(api) = host() else { return };
    if api.network.is_null() {
        return;
    }
    unsafe {
        ((*api.network).tell)(api.handle, player, Str::borrow(text));
    }
}

/// Reads one setting out of this mod's `mod.ron`.
///
/// The two-call convention every text-returning call in this API uses:
/// ask for the length with a capacity of zero, then ask again with a
/// buffer.
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

/// A player's name, or `None` if they are not here.
fn player_name(player: PlayerId) -> Option<String> {
    let api = host()?;
    if api.players.is_null() {
        return None;
    }
    let mut needed: usize = 0;
    let status = unsafe {
        ((*api.players).name)(api.handle, player, std::ptr::null_mut(), 0, &mut needed)
    };
    if !status.is_ok() || needed == 0 {
        return None;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.players).name)(
            api.handle,
            player,
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

/// Everybody connected.
fn everyone() -> Vec<PlayerId> {
    let Some(api) = host() else { return Vec::new() };
    if api.players.is_null() {
        return Vec::new();
    }
    let mut needed: usize = 0;
    unsafe {
        ((*api.players).all)(api.handle, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Vec::new();
    }
    let mut ids = vec![0 as PlayerId; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.players).all)(api.handle, ids.as_mut_ptr(), ids.len(), &mut written)
    };
    if !status.is_ok() {
        return Vec::new();
    }
    ids.truncate(written);
    ids
}

/// The player with this name, if they are connected.
///
/// Case-insensitive, because a player typing another player's name is
/// typing it from memory. A linear scan over everybody online, which is
/// the right shape for a call that happens when somebody types a command
/// and never otherwise.
fn find_player(name: &str) -> Option<PlayerId> {
    everyone()
        .into_iter()
        .find(|id| player_name(*id).is_some_and(|had| had.eq_ignore_ascii_case(name)))
}

fn is_operator(player: PlayerId) -> bool {
    let Some(api) = host() else { return false };
    if api.players.is_null() {
        return false;
    }
    unsafe { ((*api.players).is_operator)(api.handle, player) }
}

/// The call this whole mod exists to make.
fn grant(player: PlayerId, enabled: bool, speed: f32) -> bool {
    let Some(api) = host() else { return false };
    if api.players.is_null() {
        return false;
    }
    let status = unsafe { ((*api.players).set_flying)(api.handle, player, enabled, speed) };
    status.is_ok()
}

fn is_flying(player: PlayerId) -> bool {
    let Some(api) = host() else { return false };
    if api.players.is_null() {
        return false;
    }
    unsafe { ((*api.players).is_flying)(api.handle, player) }
}

fn default_speed() -> f32 {
    let bits = SPEED.load(Ordering::Relaxed);
    let speed = f32::from_bits(bits);
    if speed.is_finite() && speed >= MIN_SPEED {
        speed
    } else {
        FALLBACK_SPEED
    }
}

// ------------------------------------------------------------- the roster

/// Runs `f` with the roster, creating it if this is the first look.
///
/// A `Mutex<Option<HashMap>>` rather than a `Mutex<HashMap>` because
/// `HashMap::new` is not `const` and a `static` needs a constant
/// initialiser. The alternative -- a lazily-initialised cell -- is a
/// dependency for one map.
fn with_roster<T>(f: impl FnOnce(&mut HashMap<String, f32>) -> T) -> T {
    let mut guard = FLYING.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// Notes that this player is flying, or is not.
///
/// Only writes the save blob when the roster actually changed, because
/// the natural place to call this is from a hook that runs a lot.
fn remember(name: &str, speed: Option<f32>) {
    if !REMEMBER.load(Ordering::Relaxed) {
        return;
    }
    let changed = with_roster(|roster| match speed {
        Some(speed) => roster.insert(name.to_string(), speed) != Some(speed),
        None => roster.remove(name).is_some(),
    });
    if changed {
        store_roster();
    }
}

/// The roster, as bytes for the host to keep beside the world.
///
/// One line per player, `name<tab>speed`. Plain text rather than a
/// packed encoding: the blob is opaque to the host and owned entirely by
/// this mod, so the only reader that matters is a person looking at it
/// while working out why somebody can fly. A name cannot contain a tab
/// or a newline — `protocol::sanitize_username` strips control
/// characters — so there is nothing to escape.
fn encode_roster(roster: &HashMap<String, f32>) -> Vec<u8> {
    let mut names: Vec<&String> = roster.keys().collect();
    // Sorted, so two servers with the same roster write the same bytes
    // and a diff of the save file means something.
    names.sort();
    let mut out = String::new();
    for name in names {
        out.push_str(name);
        out.push('\t');
        out.push_str(&format!("{:.2}", roster[name]));
        out.push('\n');
    }
    out.into_bytes()
}

/// ...and back. A line that does not parse is dropped rather than
/// failing the load: a corrupted save should cost somebody their flight,
/// not the whole mod.
fn decode_roster(bytes: &[u8]) -> HashMap<String, f32> {
    let mut roster = HashMap::new();
    let Ok(text) = std::str::from_utf8(bytes) else {
        return roster;
    };
    for line in text.lines() {
        let Some((name, speed)) = line.split_once('\t') else {
            continue;
        };
        let Ok(speed) = speed.parse::<f32>() else {
            continue;
        };
        if name.is_empty() || !speed.is_finite() {
            continue;
        }
        roster.insert(name.to_string(), speed.clamp(MIN_SPEED, MAX_SPEED));
    }
    roster
}

fn store_roster() {
    let Some(api) = host() else { return };
    if api.save.is_null() {
        return;
    }
    let bytes = with_roster(|roster| encode_roster(roster));
    unsafe {
        ((*api.save).store)(api.handle, bytes.as_ptr(), bytes.len());
    }
}

fn restore_roster() {
    let Some(api) = host() else { return };
    if api.save.is_null() {
        return;
    }
    let mut needed: usize = 0;
    let status =
        unsafe { ((*api.save).load)(api.handle, std::ptr::null_mut(), 0, &mut needed) };
    // A fresh world has no blob, which is `NotFound` and is not an
    // error: every mod has to handle it, because it is what a new world
    // looks like.
    if !status.is_ok() || needed == 0 {
        return;
    }
    let mut buffer = vec![0u8; needed];
    let mut written: usize = 0;
    let status = unsafe {
        ((*api.save).load)(api.handle, buffer.as_mut_ptr(), buffer.len(), &mut written)
    };
    if !status.is_ok() {
        return;
    }
    buffer.truncate(written);
    let restored = decode_roster(&buffer);
    with_roster(|roster| *roster = restored);
}

// ------------------------------------------------------------- the command

/// What `/fly`'s arguments asked for.
///
/// Parsed into a value before anything is done about it, which is what
/// makes the parsing testable without a host — and the parsing is where
/// every interesting mistake lives.
#[derive(Debug, Clone, PartialEq)]
enum Ask {
    /// `/fly` — the other way round from whatever it is now.
    ToggleSelf,
    /// `/fly on`, `/fly off`.
    SetSelf(bool),
    /// `/fly 20` — on, at this speed.
    SpeedSelf(f32),
    /// `/fly <player>` — toggle theirs.
    ToggleOther(String),
    /// `/fly <player> <speed>` — on, at this speed.
    SpeedOther(String, f32),
    /// Something that is not any of those.
    Confused(String),
}

/// Reads `/fly`'s arguments.
///
/// **A number is a speed and a word is a name**, which is the whole
/// grammar. It works because a speed is always a number and a username
/// never is: `protocol::sanitize_username` allows digits, so `"42"` is a
/// legal name — and this deliberately reads it as a speed anyway,
/// because somebody typing `/fly 42` means forty-two blocks a second
/// every time and a player calling themselves `42` is a problem for one
/// person rather than for everyone.
fn parse(args: &str) -> Ask {
    let mut words = args.split_whitespace();
    let Some(first) = words.next() else {
        return Ask::ToggleSelf;
    };
    let second = words.next();
    if words.next().is_some() {
        return Ask::Confused("too many words".to_string());
    }

    match (first.to_ascii_lowercase().as_str(), second) {
        ("on", None) => return Ask::SetSelf(true),
        ("off", None) => return Ask::SetSelf(false),
        ("on" | "off", Some(_)) => {
            return Ask::Confused("on and off take nothing after them".to_string())
        }
        _ => {}
    }

    if let Ok(speed) = first.parse::<f32>() {
        return match second {
            None => speed_or_complaint(speed).map_or_else(Ask::Confused, Ask::SpeedSelf),
            Some(_) => Ask::Confused("a speed takes nothing after it".to_string()),
        };
    }

    match second {
        None => Ask::ToggleOther(first.to_string()),
        Some(word) => match word.parse::<f32>() {
            Ok(speed) => speed_or_complaint(speed)
                .map_or_else(Ask::Confused, |s| Ask::SpeedOther(first.to_string(), s)),
            Err(_) => Ask::Confused(format!("{word} is not a speed")),
        },
    }
}

/// A speed, or why it is not one.
fn speed_or_complaint(speed: f32) -> Result<f32, String> {
    if !speed.is_finite() {
        return Err("that is not a number".to_string());
    }
    if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
        return Err(format!("speed has to be between {MIN_SPEED:.0} and {MAX_SPEED:.0}"));
    }
    Ok(speed)
}

/// Turns flight on or off for one player and says so, to them and to
/// whoever asked.
fn apply(who: PlayerId, asked_by: PlayerId, enabled: bool, speed: f32) {
    if !grant(who, enabled, speed) {
        tell(asked_by, "the server would not do that");
        return;
    }

    let name = player_name(who).unwrap_or_else(|| "somebody".to_string());
    if enabled {
        remember(&name, Some(speed));
        tell(
            who,
            &format!(
                "flight on, {speed:.0} blocks a second -- jump to rise, sprint to descend"
            ),
        );
    } else {
        remember(&name, None);
        tell(who, "flight off");
    }

    // The operator who asked hears about it too, unless they are the
    // player -- in which case they have already been told once and do
    // not need it twice.
    if asked_by != who {
        tell(
            asked_by,
            &format!("{name}: flight {}", if enabled { "on" } else { "off" }),
        );
    }
}

/// Carries out one `/fly`.
fn run_fly(player: PlayerId, args: &str) {
    let self_allowed = !OPERATORS_ONLY.load(Ordering::Relaxed) || is_operator(player);

    match parse(args) {
        Ask::Confused(why) => {
            tell(player, &format!("{why}. try: /fly, /fly off, /fly 20, /fly <player>"));
        }

        Ask::ToggleSelf | Ask::SetSelf(_) | Ask::SpeedSelf(_) if !self_allowed => {
            tell(player, "flight is for operators on this server");
        }

        Ask::ToggleSelf => {
            let on = !is_flying(player);
            apply(player, player, on, default_speed());
        }
        Ask::SetSelf(on) => apply(player, player, on, default_speed()),
        Ask::SpeedSelf(speed) => apply(player, player, true, speed),

        // Turning it on for somebody else is an operator action
        // whatever `operators_only` says: it is a change to another
        // person's game.
        Ask::ToggleOther(_) | Ask::SpeedOther(..) if !is_operator(player) => {
            tell(player, "only an operator can do that to somebody else");
        }

        Ask::ToggleOther(name) => match find_player(&name) {
            Some(other) => apply(other, player, !is_flying(other), default_speed()),
            None => tell(player, &format!("nobody here is called {name}")),
        },
        Ask::SpeedOther(name, speed) => match find_player(&name) {
            Some(other) => apply(other, player, true, speed),
            None => tell(player, &format!("nobody here is called {name}")),
        },
    }
}

/// `/flyspeed <n>` — change speed without touching the switch.
///
/// Its own command rather than another shape of `/fly`, because
/// "how fast" and "on or off" are different questions and a player who
/// is already flying should not have to think about whether naming a
/// speed will also toggle them.
fn run_flyspeed(player: PlayerId, args: &str) {
    let Some(word) = args.split_whitespace().next() else {
        tell(player, "how fast? try: /flyspeed 20");
        return;
    };
    let speed = match word.parse::<f32>().map_err(|_| format!("{word} is not a speed")) {
        Ok(speed) => match speed_or_complaint(speed) {
            Ok(speed) => speed,
            Err(why) => {
                tell(player, &why);
                return;
            }
        },
        Err(why) => {
            tell(player, &why);
            return;
        }
    };
    if !is_flying(player) {
        tell(player, "you are not flying. /fly first");
        return;
    }
    apply(player, player, true, speed);
}

// ---------------------------------------------------------------- the mod

/// Called once, after every mod is registered and the load order is
/// settled. See the note on the greeter's version of this.
unsafe extern "C" fn load(host_api: *const HostApi) -> Status {
    if host_api.is_null() {
        return Status::BadArgument;
    }
    let version = (*host_api).version;
    if !compatible(version) {
        return Status::Refused;
    }
    HOST.store(host_api as u64, Ordering::Release);

    SPEED.store(
        setting("speed")
            .and_then(|text| text.parse::<f32>().ok())
            .filter(|speed| speed.is_finite() && (MIN_SPEED..=MAX_SPEED).contains(speed))
            .unwrap_or(FALLBACK_SPEED)
            .to_bits(),
        Ordering::Relaxed,
    );
    // Absent or unreadable means the safe answer, not the friendly one:
    // a manifest typo must not quietly hand flight to a public server.
    OPERATORS_ONLY.store(
        setting("operators_only").as_deref() != Some("false"),
        Ordering::Relaxed,
    );
    REMEMBER.store(
        setting("remember").as_deref() != Some("false"),
        Ordering::Relaxed,
    );
    DROP_ON_DEATH.store(
        setting("drop_on_death").as_deref() != Some("false"),
        Ordering::Relaxed,
    );

    restore_roster();

    let api = &*host_api;
    if api.events.is_null() {
        return Status::Unavailable;
    }
    if api.players.is_null() {
        // A dedicated server always has this table. A host that does not
        // is one this mod has nothing to say to, and saying so at load
        // is better than a null dereference on the first `/fly`.
        log(LogLevel::Error, "no players table -- flight cannot work here");
        return Status::Unavailable;
    }
    for event in [
        Event::PlayerJoined,
        Event::PlayerLeft,
        Event::PlayerDied,
        Event::Command,
        Event::ServerStopping,
    ] {
        ((*api.events).subscribe)(api.handle, event);
    }
    ((*api.events).register_command)(
        api.handle,
        Str::borrow("fly"),
        Str::borrow("turn flight on or off -- /fly, /fly off, /fly 20, /fly <player>"),
    );
    ((*api.events).register_command)(
        api.handle,
        Str::borrow("flyspeed"),
        Str::borrow("how fast to fly -- /flyspeed 20"),
    );

    let remembered = with_roster(|roster| roster.len());
    log(
        LogLevel::Info,
        &format!(
            "flight ready against API {version}: {:.0} b/s, {}, {remembered} remembered",
            default_speed(),
            if OPERATORS_ONLY.load(Ordering::Relaxed) {
                "operators only"
            } else {
                "open to everybody"
            }
        ),
    );
    Status::Ok
}

/// Whether this mod can run against a host at `version`.
fn compatible(host_version: ApiVersion) -> bool {
    host_version.accepts(API_VERSION)
}

unsafe extern "C" fn event(what: Event, data: *const EventData) -> HookResult {
    let Some(data) = data.as_ref() else {
        return HookResult::Continue;
    };
    match what {
        // Flight does not survive a disconnect on the server's side --
        // a fresh connection is a fresh body -- so somebody who had it
        // when they left is given it back here. That is the whole of
        // what `remember` means, and it is the mod's decision rather
        // than the engine's.
        Event::PlayerJoined => {
            if !REMEMBER.load(Ordering::Relaxed) {
                return HookResult::Continue;
            }
            let Some(name) = player_name(data.player) else {
                return HookResult::Continue;
            };
            let speed = with_roster(|roster| roster.get(&name).copied());
            if let Some(speed) = speed {
                if grant(data.player, true, speed) {
                    tell(
                        data.player,
                        &format!("flight still on, {speed:.0} blocks a second"),
                    );
                }
            }
            HookResult::Continue
        }

        // Nothing to do: the body goes away with the connection, and the
        // roster is keyed on the name so it survives on its own.
        Event::PlayerLeft => HookResult::Continue,

        Event::PlayerDied => {
            if !DROP_ON_DEATH.load(Ordering::Relaxed) || !is_flying(data.player) {
                return HookResult::Continue;
            }
            grant(data.player, false, 0.0);
            if let Some(name) = player_name(data.player) {
                remember(&name, None);
            }
            tell(data.player, "you were flying. you are not now");
            HookResult::Continue
        }

        // Returning `Cancel` is how a mod says "I handled this" -- so a
        // player does not get "no such command" for one this answered.
        // Anything else is left alone, or this mod would swallow every
        // other mod's commands.
        Event::Command => match data.text.as_str() {
            "fly" => {
                run_fly(data.player, data.args.as_str());
                HookResult::Cancel
            }
            "flyspeed" => {
                run_flyspeed(data.player, data.args.as_str());
                HookResult::Cancel
            }
            _ => HookResult::Continue,
        },

        // Last chance to write the roster down. The host saves after
        // this and not before.
        Event::ServerStopping => {
            store_roster();
            HookResult::Continue
        }

        _ => HookResult::Continue,
    }
}

primitive_modapi::declare_mod! {
    name: "flight",
    version: "1.0.0",
    load: load,
    event: event,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_refuses_a_host_it_cannot_talk_to() {
        assert!(compatible(API_VERSION));
        assert!(compatible(ApiVersion::new(
            API_VERSION.major,
            API_VERSION.minor + 3
        )));
        assert!(!compatible(ApiVersion::new(API_VERSION.major + 1, 0)));
        // A host older in the minor is refused too: this mod calls
        // `set_flying`, which 1.0 does not have, and reading a function
        // pointer past the end of that host's table is exactly the
        // crash the version handshake exists to prevent.
        assert!(!ApiVersion::new(1, 0).accepts(API_VERSION));
    }

    #[test]
    fn every_host_call_survives_having_no_host() {
        // All of these are reachable before `load` has run -- a host
        // that dispatched an event first, a test that calls one
        // directly -- and none may dereference a null table. The
        // release profile aborts on panic, so "it would have panicked"
        // is not a consolation.
        assert!(host().is_none());
        log(LogLevel::Info, "this goes nowhere");
        assert_eq!(setting("speed"), None);
        assert_eq!(player_name(1), None);
        assert!(everyone().is_empty());
        assert_eq!(find_player("nobody"), None);
        assert!(!is_operator(1));
        assert!(!grant(1, true, 12.0));
        assert!(!is_flying(1));
        tell(1, "nobody");
        store_roster();
        restore_roster();
        run_fly(1, "");
        run_flyspeed(1, "20");
    }

    #[test]
    fn a_null_payload_is_not_a_crash() {
        // The host never sends one, and "the host never does that" is
        // not a memory-safety argument.
        let answer = unsafe { event(Event::Command, std::ptr::null()) };
        assert_eq!(answer, HookResult::Continue);
    }

    #[test]
    fn an_unknown_command_is_left_for_somebody_else() {
        // Cancelling here would mean this mod swallowed every unknown
        // command on the server, which is how one mod breaks every
        // other one's `/home`.
        let data = EventData {
            text: Str::borrow("home"),
            ..EventData::default()
        };
        assert_eq!(unsafe { event(Event::Command, &data) }, HookResult::Continue);
    }

    // ---------------------------------------------------------- parsing

    #[test]
    fn no_arguments_is_a_toggle() {
        assert_eq!(parse(""), Ask::ToggleSelf);
        assert_eq!(parse("   "), Ask::ToggleSelf);
    }

    #[test]
    fn on_and_off_say_which() {
        assert_eq!(parse("on"), Ask::SetSelf(true));
        assert_eq!(parse("off"), Ask::SetSelf(false));
        // Case is what a person typed, not what they meant.
        assert_eq!(parse("OFF"), Ask::SetSelf(false));
        assert_eq!(parse("On"), Ask::SetSelf(true));
    }

    #[test]
    fn a_number_is_a_speed() {
        assert_eq!(parse("20"), Ask::SpeedSelf(20.0));
        assert_eq!(parse("7.5"), Ask::SpeedSelf(7.5));
    }

    #[test]
    fn a_word_is_a_player() {
        assert_eq!(parse("shamkhan"), Ask::ToggleOther("shamkhan".to_string()));
        // ...and the name keeps its case, because it is going to be
        // shown back to somebody.
        assert_eq!(parse("Shamkhan"), Ask::ToggleOther("Shamkhan".to_string()));
        assert_eq!(
            parse("shamkhan 30"),
            Ask::SpeedOther("shamkhan".to_string(), 30.0)
        );
    }

    #[test]
    fn a_speed_outside_the_range_is_refused_rather_than_clamped() {
        // Clamping silently is how somebody types 1000 and spends ten
        // minutes wondering why it feels the same as 80.
        assert!(matches!(parse("0"), Ask::Confused(_)));
        assert!(matches!(parse("1000"), Ask::Confused(_)));
        assert!(matches!(parse("-5"), Ask::Confused(_)));
        assert!(matches!(parse("nan"), Ask::Confused(_)));
        assert!(matches!(parse("inf"), Ask::Confused(_)));
        assert!(matches!(parse("someone 0"), Ask::Confused(_)));
    }

    #[test]
    fn nonsense_is_answered_rather_than_guessed_at() {
        assert!(matches!(parse("on off"), Ask::Confused(_)));
        assert!(matches!(parse("someone fast"), Ask::Confused(_)));
        assert!(matches!(parse("a b c"), Ask::Confused(_)));
        assert!(matches!(parse("20 30"), Ask::Confused(_)));
    }

    // ---------------------------------------------------------- the save

    #[test]
    fn a_roster_survives_being_written_down() {
        let mut roster = HashMap::new();
        roster.insert("shamkhan".to_string(), 12.0);
        roster.insert("someone else".to_string(), 30.5);
        let back = decode_roster(&encode_roster(&roster));
        assert_eq!(back.len(), 2);
        assert_eq!(back["shamkhan"], 12.0);
        assert_eq!(back["someone else"], 30.5);
    }

    #[test]
    fn the_saved_form_is_stable() {
        // Sorted, so the same roster is the same bytes. Without it every
        // save writes a different file and a diff of the world folder
        // says the mod changed something when it did not.
        let mut roster = HashMap::new();
        for name in ["c", "a", "b"] {
            roster.insert(name.to_string(), 12.0);
        }
        assert_eq!(
            String::from_utf8(encode_roster(&roster)).unwrap(),
            "a\t12.00\nb\t12.00\nc\t12.00\n"
        );
    }

    #[test]
    fn a_corrupt_roster_costs_a_line_rather_than_the_mod() {
        let bytes = b"good\t12.0\nno-tab-here\nbad\tnotanumber\n\t9\nalso\tgood\t9\nfine\t20\n";
        let roster = decode_roster(bytes);
        // The first and the last survive; the rest are dropped. The
        // "also" line has a second tab, so its speed does not parse.
        assert_eq!(roster.len(), 2);
        assert!(roster.contains_key("good"));
        assert!(roster.contains_key("fine"));
    }

    #[test]
    fn a_roster_from_nothing_is_empty_rather_than_an_error() {
        assert!(decode_roster(b"").is_empty());
        assert!(decode_roster(&[0xff, 0xfe]).is_empty());
    }

    #[test]
    fn a_saved_speed_is_brought_back_into_range() {
        // The file is on disk and a person can edit it, so what comes
        // out of it is input.
        let roster = decode_roster(b"someone\t9999\nother\t0.0001\n");
        assert_eq!(roster["someone"], MAX_SPEED);
        assert_eq!(roster["other"], MIN_SPEED);
    }
}
