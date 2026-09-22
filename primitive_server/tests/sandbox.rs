//! The sandbox mod, across the real C ABI.
//!
//! `tests/mods.rs` proves the boundary works and `tests/flight.rs`
//! proves it carries a feature. This one proves the thing neither of
//! them could: that **every function in every table is actually
//! reachable from the other side, and every event actually arrives**.
//!
//! That distinction is not academic. Before 2.0 the API declared
//! twenty-one events and the server fired twelve of them; it accepted a
//! chunk decorator, wrote it down, and never called it. All of that
//! compiled, and `tests/mods.rs` passed throughout, because everything
//! it checks is on the host's side of the boundary. What was missing was
//! a mod that calls the whole surface and says what happened -- which is
//! `mods/sandbox`, and this is what runs it.
//!
//! ## What a passing run means
//!
//! The sweep is straight-line code over every table: a hundred-odd
//! function pointers, read out of structs this test's crate did not
//! write, called with arguments this test's crate did not choose. If a
//! table had grown in the middle rather than at the end, or a field had
//! moved, the sweep would be calling the wrong function with the wrong
//! arguments -- and the failure would be a crash or a hang inside this
//! test rather than a wrong number somewhere in somebody's world a month
//! later.
//!
//! ## Why it may skip
//!
//! The library has to be *built* before it can be loaded, and
//! `cargo test` does not build the workspace's `cdylib` targets as a
//! dependency of this test. So it looks for the artefact and reports a
//! skip if it is missing rather than failing: a red test that means "you
//! did not run a build command" trains people to ignore red tests.
//!
//! ```text
//! cargo build -p sandbox && cargo test -p primitive_server --test sandbox
//! ```

use std::path::{Path, PathBuf};

use primitive_modapi::{Event, EventData, HookResult, Str};
use primitive_server::mods::{self, HostContext};
use primitive_server::settings::ServerSettings;
use primitive_server::{Context, RunOptions};

/// A directory that removes itself, so a failed run does not leave half
/// a mod folder in the system temp directory.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "primitive_sandbox_{}_{tag}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Where cargo put the compiled mod, if it has been built. Walks up from
/// this test's own executable -- see the note in `tests/mods.rs`.
fn built_library() -> Option<PathBuf> {
    let name = primitive_modapi::manifest::platform_library_name("sandbox");
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    dir.pop();
    [dir.join(&name), dir.join("deps").join(&name)]
        .into_iter()
        .find(|candidate| candidate.exists())
}

fn install(into: &Path, manifest: &str) -> bool {
    let Some(library) = built_library() else {
        return false;
    };
    let folder = into.join("sandbox");
    std::fs::create_dir_all(&folder).expect("mod folder");
    std::fs::write(folder.join("mod.ron"), manifest).expect("manifest");
    let target = folder.join(primitive_modapi::manifest::platform_library_name("sandbox"));
    std::fs::copy(&library, &target).expect("copy the library");
    true
}

const MANIFEST: &str = r#"(
    name: "sandbox",
    version: "1.0.0",
    api: (major: 2, minor: 0),
    description: "the sweep",
    settings: { "allow_writes": Bool(false), "scratch_offset": I64(3) },
)"#;

macro_rules! needs_the_library {
    ($dir:expr) => {
        if !install(&$dir.0, MANIFEST) {
            eprintln!("skipping: no sandbox library built (cargo build -p sandbox)");
            return;
        }
    };
}

/// A server with nowhere to save to, so nothing a test does lands on
/// disk beside somebody's real world.
fn context() -> std::sync::Arc<Context> {
    let settings = ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        ..ServerSettings::default()
    };
    // Embedded: no console logging, so a sweep's hundred report lines do
    // not scroll past the test runner's own output.
    primitive_server::test_context(settings, RunOptions::embedded())
}

fn load(ctx: &std::sync::Arc<Context>, dir: &Path) -> String {
    let host = HostContext(std::sync::Arc::clone(ctx));
    let lines = {
        let mut mods = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        mods.load_dir(dir, host)
    };
    let mut said = lines.join("\n");
    said.push('\n');
    said.push_str(&mods::start_all(ctx).join("\n"));
    said
}

fn active(ctx: &std::sync::Arc<Context>) -> usize {
    ctx.mods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_count()
}

/// What the mod wrote down about its last sweep: how many report lines
/// it produced, and how many events it had been told about by then. See
/// `sandbox::remember`.
fn last_sweep(ctx: &std::sync::Arc<Context>) -> Option<(u32, u64)> {
    let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
    let blobs = host.take_dirty_blobs();
    let (_, blob) = blobs.into_iter().find(|(name, _)| name == "sandbox")?;
    if blob.len() < 12 {
        return None;
    }
    let lines = u32::from_le_bytes(blob[0..4].try_into().ok()?);
    let events = u64::from_le_bytes(blob[4..12].try_into().ok()?);
    Some((lines, events))
}

fn command(ctx: &std::sync::Arc<Context>, name: &str, args: &str) -> HookResult {
    let data = EventData {
        // Player 1 is not connected in this fixture, so every host call
        // the sweep makes about a player answers "no such player". That
        // is deliberate and is half of what this test is for: the whole
        // surface has to survive being asked about somebody who is not
        // there, and answer rather than dereference.
        player: 1,
        text: Str::borrow(name),
        args: Str::borrow(args),
        ..EventData::default()
    };
    mods::dispatch(ctx, Event::Command, &data)
}

#[test]
fn it_subscribes_to_every_event_the_contract_declares() {
    // **The one mod in the workspace for which "everything" is the right
    // answer.** The question it exists to settle is which events never
    // arrive, and it cannot settle that about an event it did not
    // subscribe to -- so a variant missing from its list is the same
    // blind spot that let nine events go unfired for two versions.
    let dir = TempDir::new("subscribe");
    needs_the_library!(dir);

    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 1, "the sandbox mod did not load: {said}");

    let wants = |event| {
        ctx.mods
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wants(event)
    };
    for event in [
        Event::ServerStarted,
        Event::ServerStopping,
        Event::Tick,
        Event::PlayerJoined,
        Event::PlayerLeft,
        Event::PlayerChat,
        Event::PlayerDied,
        Event::PlayerHurt,
        Event::PlayerHealed,
        Event::PlayerAte,
        Event::PlayerEnteredWater,
        Event::PlayerLeftWater,
        Event::HeldSlotChanged,
        Event::BlockPlace,
        Event::BlockBreak,
        Event::BlockChanged,
        Event::TreeFelled,
        Event::ChunkGenerated,
        Event::EntitySpawned,
        Event::EntityRemoved,
        Event::ItemCrafted,
        Event::ItemPickedUp,
        Event::ItemDropped,
        Event::ToolBroke,
        Event::HideCured,
        Event::PlayerDrank,
        Event::EquipmentChanged,
        Event::ContainerOpened,
        Event::ContainerClosed,
        Event::SmeltingFinished,
        Event::GrowthStep,
        Event::FireDied,
        Event::WeatherChanged,
        Event::TimeChanged,
        Event::Command,
    ] {
        assert!(wants(event), "the sandbox never subscribed to {event:?}");
    }
    mods::unload_all(&ctx);
}

#[test]
fn the_read_sweep_calls_every_table_and_comes_back() {
    // **A hundred-odd function pointers, read out of structs this crate
    // did not write and called with arguments it did not choose.** If a
    // table had grown in the middle instead of at the end, or an
    // out-parameter struct had changed size, this is where it shows --
    // as a crash inside a test rather than as a wrong number in
    // somebody's world a month from now.
    let dir = TempDir::new("read");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    assert_eq!(
        command(&ctx, "sandbox", ""),
        HookResult::Cancel,
        "/sandbox went unanswered"
    );
    let (lines, _) = last_sweep(&ctx).expect("the sweep wrote nothing down");
    // Nine tables report at least one line each, and several report
    // two. A sweep that produced two lines is a sweep that bailed at the
    // first null table, which on this host would mean the host handed
    // out a bundle with holes in it.
    assert!(
        lines >= 10,
        "the sweep produced {lines} line(s), which is not a sweep"
    );
    mods::unload_all(&ctx);
}

#[test]
fn the_write_sweep_is_refused_unless_the_manifest_allows_it() {
    // A diagnostic must not be a disaster. `/sandbox write` puts blocks
    // down, lights fires and spills water within a few metres of
    // whoever ran it, and the default has to be that it does not.
    let dir = TempDir::new("write_off");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    assert_eq!(command(&ctx, "sandbox", "write"), HookResult::Cancel);
    let (lines, _) = last_sweep(&ctx).expect("nothing was written down");
    assert_eq!(
        lines, 1,
        "the write sweep ran with allow_writes off: {lines} lines"
    );
    mods::unload_all(&ctx);
}

#[test]
fn the_write_sweep_runs_when_it_is_allowed_and_finds_no_player() {
    // Turned on, and then asked about somebody who is not connected.
    // The whole write half has to reach its first call, be told there is
    // no such player, and stop -- rather than build a chest at the
    // origin because a missing position read as zero.
    let dir = TempDir::new("write_on");
    let allowed = MANIFEST.replace("Bool(false)", "Bool(true)");
    if !install(&dir.0, &allowed) {
        eprintln!("skipping: no sandbox library built");
        return;
    }

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    assert_eq!(command(&ctx, "sandbox", "write"), HookResult::Cancel);
    let (lines, _) = last_sweep(&ctx).expect("nothing was written down");
    assert_eq!(
        lines, 1,
        "the write sweep built something for a player who is not here"
    );
    mods::unload_all(&ctx);
}

/// Every event it is handed is counted, and the count survives the
/// crossing.
///
/// The number in the blob is what `/sandbox events` reports, so this is
/// also the test that the report is not a fiction.
#[test]
fn what_it_is_told_about_is_what_it_counts() {
    let dir = TempDir::new("counting");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    // Three events of two kinds, none of them a command.
    let block = EventData {
        pos: primitive_modapi::BlockPos { x: 1, y: 2, z: 3 },
        block: 4,
        ..EventData::default()
    };
    mods::dispatch(&ctx, Event::BlockChanged, &block);
    mods::dispatch(&ctx, Event::BlockChanged, &block);
    mods::dispatch(&ctx, Event::GrowthStep, &block);

    assert_eq!(command(&ctx, "sandbox", "events"), HookResult::Cancel);
    let (lines, events) = last_sweep(&ctx).expect("nothing was written down");
    // Two lines: what never fired, and what did.
    assert_eq!(lines, 2);
    // Three, plus the `Command` this very call is. `ServerStarted` is
    // not among them: this fixture never starts a server, which is
    // exactly the kind of hole the mod is for.
    assert_eq!(
        events, 4,
        "the sandbox counted {events} events where four arrived"
    );
    mods::unload_all(&ctx);
}

/// It answers `/sandbox` and leaves everybody else's commands alone.
///
/// The second half matters more than the first: a mod that cancelled
/// every command it did not recognise would swallow every *other* mod's,
/// and the failure would look like the other mod being broken.
#[test]
fn it_answers_its_own_command_and_leaves_the_rest_alone() {
    let dir = TempDir::new("dispatch");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    assert_eq!(command(&ctx, "sandbox", ""), HookResult::Cancel);
    assert_eq!(command(&ctx, "sandbox", "events"), HookResult::Cancel);
    assert_eq!(command(&ctx, "sandbox", "nonsense"), HookResult::Cancel);
    assert_eq!(
        command(&ctx, "fly", ""),
        HookResult::Continue,
        "the sandbox swallowed somebody else's command"
    );
    assert_eq!(command(&ctx, "greet", ""), HookResult::Continue);
    mods::unload_all(&ctx);
}

/// Its command reaches the host's registry with a help line on it.
#[test]
fn its_command_reaches_the_host() {
    let dir = TempDir::new("commands");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let registered = ctx
        .mods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .commands();
    let found = registered.iter().find(|(name, _)| name == "sandbox");
    let Some((_, help)) = found else {
        panic!("/sandbox was never registered: {registered:?}");
    };
    assert!(!help.is_empty(), "registered with no help text");
    mods::unload_all(&ctx);
}
