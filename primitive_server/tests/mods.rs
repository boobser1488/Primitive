//! The native mod API, end to end.
//!
//! **What makes this test worth its weight is that it uses the real
//! boundary.** It builds a folder that looks exactly like an installed
//! mod -- a `mod.ron` and the compiled `greeter` library beside it --
//! points a real server at it, and then checks the things that can only
//! be checked by having actually crossed a C ABI: that the entry symbol
//! was found, that the version handshake agreed, that a subscription
//! took, that a hook was called, and that a veto stopped the world from
//! changing.
//!
//! A mod API exercised only by the server that implements it is an API
//! whose first real user finds the mistakes.
//!
//! ## Why it may skip
//!
//! The library has to be *built* before it can be loaded, and
//! `cargo test` does not build the workspace's `cdylib` targets as a
//! dependency of this test -- there is no way to say "and also that
//! artefact" in a manifest. So the test looks for it and reports a skip
//! if it is not there rather than failing: a red test that means "you
//! did not run a build command" trains people to ignore red tests.
//!
//! ```text
//! cargo build -p greeter && cargo test -p primitive_server --test mods
//! ```

use std::path::{Path, PathBuf};

use primitive_server::mods::{self, HostContext};
use primitive_server::settings::ServerSettings;
use primitive_server::{Context, RunOptions};

/// A directory that removes itself, so a failed run does not leave half
/// a mod folder in the system temp directory.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "primitive_mods_{}_{tag}_{:?}",
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

/// Where cargo put the compiled mod, if it has been built.
///
/// Walks up from this test's own executable, which is the only reliable
/// way to find the target directory: `CARGO_TARGET_DIR` may be set to
/// anywhere, and `target/` beside the manifest is a guess that is wrong
/// on any machine with a shared target directory.
fn built_library() -> Option<PathBuf> {
    let name = primitive_modapi::manifest::platform_library_name("greeter");
    let mut dir = std::env::current_exe().ok()?;
    // .../target/debug/deps/mods-<hash>.exe -> .../target/debug
    dir.pop();
    dir.pop();
    [dir.join(&name), dir.join("deps").join(&name)]
        .into_iter()
        .find(|candidate| candidate.exists())
}

/// Lays out a mod folder the way an installed one looks.
fn install(into: &Path, manifest: &str) -> bool {
    let Some(library) = built_library() else {
        return false;
    };
    let folder = into.join("greeter");
    std::fs::create_dir_all(&folder).expect("mod folder");
    std::fs::write(folder.join("mod.ron"), manifest).expect("manifest");
    let target = folder.join(primitive_modapi::manifest::platform_library_name("greeter"));
    std::fs::copy(&library, &target).expect("copy the library");
    true
}

const MANIFEST: &str = r#"(
    name: "greeter",
    version: "1.0.0",
    api: (major: 2, minor: 0),
    description: "the worked example",
    settings: { "greeting": Text("hello there"), "guard_bedrock": Bool(true) },
)"#;

/// A server with nowhere to save to, so nothing a test does lands on
/// disk beside somebody's real world.
fn context() -> std::sync::Arc<primitive_server::Context> {
    let settings = ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        world_preset: primitive_shared::worldgen::Preset::Test,
        ..Default::default()
    };
    // The one way in: `start` binds a socket and spawns the loops, which
    // is more than this needs, so the context is built through the same
    // embedded path a singleplayer world uses.
    primitive_server::test_context(settings, RunOptions::embedded())
}

/// Loads a mod folder into a real server context, exactly as `start`
/// does.
///
/// **Into the context's own host rather than a standalone one**, and
/// that is not incidental: a mod's `on_load` calls back through
/// `Context::mods`, so a test that loaded into a host on the side would
/// be testing a mod talking to a different server than the one it was
/// loaded by -- which is precisely the shape that hid the deadlock this
/// module was rewritten to fix.
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

fn wants(ctx: &std::sync::Arc<Context>, event: primitive_modapi::Event) -> bool {
    ctx.mods
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .wants(event)
}

macro_rules! needs_the_library {
    ($dir:expr, $manifest:expr) => {
        if !install(&$dir.0, $manifest) {
            eprintln!(
                "skipping: no greeter library built.\\n\
                 run `cargo build -p greeter` first"
            );
            return;
        }
    };
}

#[test]
fn a_real_mod_loads_across_a_real_c_abi() {
    let dir = TempDir::new("load");
    needs_the_library!(dir, MANIFEST);

    let ctx = context();
    let said = load(&ctx, &dir.0);

    assert_eq!(
        active(&ctx),
        1,
        "the mod did not load. what the host said:\n{said}"
    );
    assert!(said.contains("loaded mod 'greeter'"), "{said}");
    // ...and it subscribed during `on_load`, which is the half of the
    // handshake that proves the *host's* tables were reachable from
    // inside the library rather than merely that the symbol resolved.
    // It is also the assertion that caught the deadlock: a `dispatch`
    // holding the host's lock across the call made this fail.
    assert!(
        wants(&ctx, primitive_modapi::Event::BlockBreak),
        "the mod loaded but never subscribed: {said}"
    );
    assert!(
        !wants(&ctx, primitive_modapi::Event::Tick),
        "the mod was subscribed to something it never asked for"
    );
    mods::unload_all(&ctx);
}

#[test]
fn a_mod_may_call_back_into_the_host_from_inside_a_hook() {
    // **The deadlock, as a test.** Every one of these calls locks
    // `Context::mods` from inside a handler that was itself reached
    // through it. If dispatch ever goes back to holding that lock across
    // the call, this hangs rather than failing -- which is why the
    // greeter subscribes to `PlayerJoined` and calls `tell` and `store`
    // from it, and why this test dispatches one.
    let dir = TempDir::new("reentrant");
    needs_the_library!(dir, MANIFEST);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let data = primitive_modapi::EventData {
        player: 1,
        ..primitive_modapi::EventData::default()
    };
    // `tell` finds no such player and says so; `store` writes the blob.
    // Neither may deadlock, and the second is checked below.
    let answer = mods::dispatch(&ctx, primitive_modapi::Event::PlayerJoined, &data);
    assert_eq!(answer, primitive_modapi::HookResult::Continue);

    // The mod counted the join and wrote it down from inside the hook,
    // which means `SaveApi::store` reached the host while the host was
    // dispatching.
    let saved = {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.take_dirty_blobs()
    };
    assert_eq!(saved.len(), 1, "the mod's hook never reached the save API");
    assert_eq!(saved[0].0, "greeter");
    assert_eq!(
        u64::from_le_bytes(saved[0].1.clone().try_into().expect("eight bytes")),
        1,
        "the mod counted the wrong number of joins"
    );
    mods::unload_all(&ctx);
}

#[test]
fn a_veto_from_a_mod_stops_the_action() {
    // The cancellable half, and the reason hooks run *inside* the edit
    // path rather than watching from the side.
    let dir = TempDir::new("veto");
    needs_the_library!(dir, MANIFEST);

    let ctx = context();
    load(&ctx, &dir.0);
    if active(&ctx) == 0 {
        return;
    }

    let at = |y: i32| {
        let data = primitive_modapi::EventData {
            pos: primitive_modapi::BlockPos { x: 0, y, z: 0 },
            ..primitive_modapi::EventData::default()
        };
        mods::dispatch(&ctx, primitive_modapi::Event::BlockBreak, &data)
    };
    assert_eq!(
        at(1),
        primitive_modapi::HookResult::Cancel,
        "breaking the floor of the world was allowed"
    );
    assert_eq!(at(40), primitive_modapi::HookResult::Continue);
    mods::unload_all(&ctx);
}

#[test]
fn a_mod_built_against_another_major_version_is_refused_unopened() {
    // **Unopened is the point.** `dlopen` runs a library's initialisers,
    // and a mod built against a different major version of the contract
    // is exactly the one you do not want to have run anything before you
    // find out. The manifest declares the version so the refusal can
    // happen first.
    let dir = TempDir::new("version");
    let future = r#"(
        name: "greeter",
        version: "1.0.0",
        api: (major: 99, minor: 0),
    )"#;
    needs_the_library!(dir, future);

    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 0, "{said}");
    assert!(said.contains("mod API 99.0"), "{said}");
}

#[test]
fn a_mod_built_against_the_previous_major_is_refused_with_both_versions_named() {
    // **The compatibility promise, stated as a test.** 2.0 changed what
    // `PlayerVitals::temperature_c` means -- see the note beside
    // `primitive_modapi::API_VERSION` -- so a mod built against 1.1 is
    // not merely old, it is *wrong about the player's body*, and this
    // host must refuse it rather than load it and let it be quietly
    // wrong.
    //
    // Two things are asserted and the second matters as much as the
    // first. It is refused, and the refusal **names both versions**: the
    // whole cost of a major bump is that somebody has to be told what to
    // change, and a line saying only "incompatible" is a line that sends
    // them to read the loader's source.
    let dir = TempDir::new("previous_major");
    let old = r#"(
        name: "greeter",
        version: "1.0.0",
        api: (major: 1, minor: 1),
    )"#;
    needs_the_library!(dir, old);

    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 0, "{said}");
    assert!(said.contains("mod API 1.1"), "{said}");
    // The host's *own* version rather than a copy of it. This line read
    // "this server is 2.0" and went red the day the API's minor went
    // up, which is a test failing for the one reason it was never about:
    // what it is checking is that the refusal names the version, not
    // which version that is.
    let host = primitive_modapi::API_VERSION;
    assert!(
        said.contains(&format!("this server is {}.{}", host.major, host.minor)),
        "{said}"
    );
}

#[test]
fn a_mod_whose_manifest_and_library_disagree_about_its_name_is_refused() {
    // The folder and the binary being from different builds. Carrying on
    // would file this mod's settings and its saved state under a name
    // its own code does not use.
    let dir = TempDir::new("naming");
    let wrong = r#"(
        name: "impostor",
        version: "1.0.0",
        api: (major: 2, minor: 0),
        library: Named("wrongly_named"),
    )"#;
    // Installed by hand: the helper names the file after the mod, and
    // here the point is that they differ.
    let Some(library) = built_library() else {
        eprintln!("skipping: no greeter library built");
        return;
    };
    let folder = dir.0.join("impostor");
    std::fs::create_dir_all(&folder).expect("mod folder");
    std::fs::write(folder.join("mod.ron"), wrong).expect("manifest");
    // Named with the platform's own extension: Windows refuses to load
    // a library without one, and the point of this test is the *name
    // check*, not the loader's file-extension rules.
    let named = format!(
        "wrongly_named{}",
        primitive_modapi::manifest::platform_library_name("x")
            .trim_start_matches("libx")
            .trim_start_matches('x')
    );
    std::fs::write(folder.join("mod.ron"), wrong.replace("wrongly_named", &named))
        .expect("manifest");
    std::fs::copy(&library, folder.join(&named)).expect("copy");

    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 0, "{said}");
    assert!(said.contains("calls itself 'greeter'"), "{said}");
}

#[test]
fn a_folder_with_no_manifest_is_reported_rather_than_ignored() {
    let dir = TempDir::new("empty");
    std::fs::create_dir_all(dir.0.join("not_a_mod")).expect("folder");
    let ctx = context();
    let said = load(&ctx, &dir.0);
    assert_eq!(active(&ctx), 0);
    assert!(said.contains("mod.ron"), "{said}");
}

#[test]
fn a_server_with_no_mod_directory_starts_anyway() {
    // The commonest case by a long way, and it must not be an error:
    // most people never install a mod.
    let ctx = context();
    let said = load(&ctx, Path::new("no/such/place"));
    assert_eq!(active(&ctx), 0);
    assert!(said.contains("no mod directory"), "{said}");
}
