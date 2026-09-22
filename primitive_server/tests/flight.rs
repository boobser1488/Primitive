//! The flight mod, across the real C ABI.
//!
//! `tests/mods.rs` proves the boundary works with the worked example.
//! This one proves it carries a *feature*: the flight mod is the first
//! thing that uses a capability the API grew for it
//! (`PlayersApi::set_flying`, added in 1.1), and an extension point
//! whose only user is a demonstration is an extension point nobody has
//! tried to build anything with.
//!
//! What is checked here is what can only be checked by having actually
//! crossed the boundary: that a mod declaring this host's API is
//! accepted, that
//! it found the two new fields at the end of a table it did not write,
//! that its commands reached the host's registry, and that it answers
//! its own commands and leaves everybody else's alone.
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
//! cargo build -p flight && cargo test -p primitive_server --test flight
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
            "primitive_flight_{}_{tag}_{:?}",
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
    let name = primitive_modapi::manifest::platform_library_name("flight");
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
    let folder = into.join("flight");
    std::fs::create_dir_all(&folder).expect("mod folder");
    std::fs::write(folder.join("mod.ron"), manifest).expect("manifest");
    let target = folder.join(primitive_modapi::manifest::platform_library_name("flight"));
    std::fs::copy(&library, &target).expect("copy the library");
    true
}

const MANIFEST: &str = r#"(
    name: "flight",
    version: "1.0.0",
    api: (major: 2, minor: 0),
    description: "flight",
    settings: {
        "speed": F64(20.0),
        "operators_only": Bool(true),
        "remember": Bool(true),
        "drop_on_death": Bool(true),
    },
)"#;

fn context() -> std::sync::Arc<Context> {
    let settings = ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        world_preset: primitive_shared::worldgen::Preset::Test,
        ..Default::default()
    };
    primitive_server::test_context(settings, RunOptions::embedded())
}

fn load(ctx: &std::sync::Arc<Context>, dir: &Path) -> String {
    let mut lines = {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.load_dir(dir, HostContext(std::sync::Arc::clone(ctx)))
    };
    lines.extend(mods::start_all(ctx));
    lines.join("\n")
}

fn active(ctx: &std::sync::Arc<Context>) -> usize {
    ctx.mods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_count()
}

fn commands(ctx: &std::sync::Arc<Context>) -> Vec<(String, String)> {
    ctx.mods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .commands()
}

macro_rules! needs_the_library {
    ($dir:expr) => {
        if !install(&$dir.0, MANIFEST) {
            eprintln!(
                "skipping: no flight library built.\\n\
                 run `cargo build -p flight` first"
            );
            return;
        }
    };
}

/// The version handshake, from the other side of the one that
/// `tests/mods.rs` checks: that a mod declaring exactly what this host
/// implements is loaded, and that the version the host reports is the
/// one it read back out of the *library* rather than the one the
/// manifest claimed.
#[test]
fn a_mod_that_needs_the_newest_api_loads() {
    let dir = TempDir::new("load");
    needs_the_library!(dir);

    let ctx = context();
    let said = load(&ctx, &dir.0);

    assert_eq!(
        active(&ctx),
        1,
        "the flight mod did not load. what the host said:\n{said}"
    );
    assert!(said.contains("loaded mod 'flight'"), "{said}");
    // **The version the host read back out of the library**, not the one
    // the manifest claimed -- the manifest is checked before the library
    // is opened and the descriptor after, and this is the second of the
    // two.
    assert!(said.contains("API 2.0"), "{said}");

    // What the mod itself said about its settings goes to the server
    // log through `CoreApi::log` rather than into this string -- the
    // loader reports on the mod, the mod reports on itself. Its
    // behaviour under those settings is checked by the command tests
    // below.
    mods::unload_all(&ctx);
}

#[test]
fn it_is_refused_by_a_host_that_is_too_old_for_it() {
    // The manifest is read *before* the library is opened, which is the
    // whole reason the API version is declared in both places: opening a
    // library runs its initialisers, and a mod that needs fields this
    // host does not have is precisely the one you do not want to have
    // run anything first.
    let dir = TempDir::new("too_new");
    let manifest = MANIFEST.replace("minor: 0", "minor: 99");
    if !install(&dir.0, &manifest) {
        eprintln!("skipping: no flight library built");
        return;
    }

    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 0, "a mod from the future was loaded: {said}");
    mods::unload_all(&ctx);
}

#[test]
fn it_subscribes_to_what_it_uses_and_nothing_else() {
    let dir = TempDir::new("subscribe");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let wants = |event| {
        ctx.mods
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wants(event)
    };
    for event in [
        Event::Command,
        Event::PlayerJoined,
        Event::PlayerDied,
        Event::ServerStopping,
    ] {
        assert!(wants(event), "never subscribed to {event:?}");
    }
    // A mod that subscribes to nothing is never called, which is what
    // makes a dozen loaded mods cost nothing on a tick none of them
    // cares about -- so subscribing to one it does not use is a real
    // cost rather than an untidiness.
    assert!(
        !wants(Event::Tick),
        "flight subscribed to the tick, which it has no use for"
    );
    assert!(!wants(Event::ChunkGenerated));
    mods::unload_all(&ctx);
}

#[test]
fn its_commands_reach_the_host() {
    let dir = TempDir::new("commands");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let registered = commands(&ctx);
    for wanted in ["fly", "flyspeed"] {
        let found = registered.iter().find(|(name, _)| name == wanted);
        let Some((_, help)) = found else {
            panic!("{wanted} was never registered: {registered:?}");
        };
        assert!(!help.is_empty(), "{wanted} was registered with no help text");
    }
    mods::unload_all(&ctx);
}

/// `/fly` is answered and `/home` is not.
///
/// The second half matters more than the first: a mod that cancelled
/// every command it did not recognise would swallow every *other* mod's,
/// and the failure would look like the other mod being broken.
#[test]
fn it_answers_its_own_commands_and_leaves_the_rest_alone() {
    let dir = TempDir::new("dispatch");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let say = |name: &str, args: &str| {
        let data = EventData {
            // Player 1 is not connected in this fixture, so every host
            // call the mod makes about them answers "no such player".
            // That is the point: the whole command path has to survive
            // it without dereferencing anything.
            player: 1,
            text: Str::borrow(name),
            args: Str::borrow(args),
            ..EventData::default()
        };
        mods::dispatch(&ctx, Event::Command, &data)
    };

    assert_eq!(say("fly", ""), HookResult::Cancel, "/fly went unanswered");
    assert_eq!(say("fly", "off"), HookResult::Cancel);
    assert_eq!(say("fly", "20"), HookResult::Cancel);
    assert_eq!(say("fly", "somebody"), HookResult::Cancel);
    assert_eq!(say("fly", "one two three"), HookResult::Cancel);
    assert_eq!(say("flyspeed", "20"), HookResult::Cancel);
    assert_eq!(say("flyspeed", ""), HookResult::Cancel);

    assert_eq!(
        say("home", ""),
        HookResult::Continue,
        "flight swallowed somebody else's command"
    );
    assert_eq!(say("greet", ""), HookResult::Continue);

    mods::unload_all(&ctx);
}

/// Nothing in the mod's roster is written until somebody actually
/// flies, so a server that loads it and is never used leaves no blob.
#[test]
fn loading_it_alone_writes_nothing_down() {
    let dir = TempDir::new("save");
    needs_the_library!(dir);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let saved = {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.take_dirty_blobs()
    };
    assert!(
        saved.is_empty(),
        "a mod nobody has used wrote a save blob: {saved:?}"
    );
    mods::unload_all(&ctx);
}
