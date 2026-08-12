//! Plugins and modding.
//!
//! A plugin is a folder under `plugins/` containing a `plugin.toml`
//! (metadata) and a `main.rhai` (script). Scripts are [Rhai], a small
//! embedded language: no build step, no native code, and no way for a
//! plugin to reach outside the API the server hands it.
//!
//! ```text
//! plugins/
//!   welcome/
//!     plugin.toml
//!     main.rhai
//! ```
//!
//! ## Why scripts and not native plugins
//!
//! A native plugin (a `.dll`/`.so` loaded at runtime) has to be compiled
//! against the exact compiler and dependency versions the server was
//! built with, and any mistake in one takes the whole server down with
//! it. For a server people will actually drop files into, a script is
//! the right trade: slower per call, but a broken plugin is a log line
//! rather than a crash, and a plugin that runs away is stopped by the
//! operation limit rather than hanging the tick loop.
//!
//! ## Hooks
//!
//! A script defines any subset of these functions:
//!
//! | function | called when | returning `false` |
//! |---|---|---|
//! | `on_load()` | server start | -- |
//! | `on_join(player)` | a player connects | -- |
//! | `on_leave(player)` | a player disconnects | -- |
//! | `on_chat(player, text)` | a player chats | cancels the message |
//! | `on_block_place(player, x, y, z, block)` | before a placement | cancels it |
//! | `on_block_break(player, x, y, z)` | before a break | cancels it |
//! | `on_command(player, command, args)` | unknown `/command` | -- |
//! | `on_tick(tick)` | every server tick | -- |
//!
//! The cancellable hooks are why this runs *inside* the block-edit path
//! rather than watching from the side: a protection plugin has to be
//! able to say no before the world changes, not undo it afterwards.
//!
//! ## What a script can do
//!
//! `broadcast`, `tell`, `log`, `set_block`, `get_block`, `block_name`,
//! `player_position`, `player_names`, `time_of_day`, `set_time`, `kick`,
//! plus per-plugin persistent `store`/`fetch`. No file, network or
//! process access is registered, so a plugin cannot touch anything the
//! server didn't hand it.
//!
//! ## Compiling without plugins
//!
//! The whole subsystem sits behind the `plugins` cargo feature, which is
//! on by default for the standalone server. Turning it off drops the
//! scripting engine entirely and leaves a `PluginHost` that loads
//! nothing and vetoes nothing.
//!
//! That exists for singleplayer. The client embeds this crate to run a
//! local server in-process, and a singleplayer world has no operator to
//! install plugins for, no one to protect the player's builds from, and
//! no reason to carry a scripting language in the game binary. The rest
//! of the server is written against `plugins::Value` rather than against
//! Rhai's own types, so nothing outside this module changes shape when
//! the feature is off.
//!
//! [Rhai]: https://rhai.rs

use std::collections::HashMap;
#[cfg(feature = "plugins")]
use std::path::PathBuf;
use std::path::Path;
#[cfg(feature = "plugins")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "plugins")]
use rhai::{Dynamic, Engine, Map, Scope, AST};
#[cfg(feature = "plugins")]
use serde::Deserialize;

use primitive_shared::protocol::PlayerId;
// `Effect` and `HostView` are part of the API in both builds, so the
// types they mention have to be imported in both. Everything else here
// is only reachable from the scripting engine.
use primitive_shared::types::BlockId;
#[cfg(feature = "plugins")]
use primitive_shared::types::{
    block_name, is_collidable, is_known_block, ALL_BLOCK_IDS, CHUNK_SIZE_Y,
};

/// One argument to a hook.
///
/// The server passes these instead of Rhai's `Dynamic` so that the call
/// sites -- which are scattered through the connection and tick paths --
/// don't have to exist in two versions depending on a cargo feature, and
/// don't drag a scripting engine into their signatures.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
    List(Vec<Value>),
}

impl Value {
    pub fn int(n: impl Into<i64>) -> Self {
        Value::Int(n.into())
    }
    pub fn text(s: impl Into<String>) -> Self {
        Value::Text(s.into())
    }
}

/// Converts a hook argument for the scripting engine.
///
/// Deliberately a free function rather than a `From<Value> for Dynamic`
/// impl. Rhai has an *inherent* `Dynamic::from<T: Variant>`, which wins
/// name resolution over any `From` impl -- so `Dynamic::from(value)`
/// would silently box the `Value` itself as an opaque foreign type, and
/// scripts would see arguments they cannot read or compare. The failure
/// is invisible at compile time and shows up as every hook quietly
/// misbehaving.
#[cfg(feature = "plugins")]
fn to_dynamic(value: Value) -> Dynamic {
    match value {
        Value::Int(n) => Dynamic::from(n),
        Value::Text(s) => Dynamic::from(s),
        Value::List(items) => {
            Dynamic::from(items.into_iter().map(to_dynamic).collect::<rhai::Array>())
        }
    }
}

/// Cap on Rhai operations per hook call. A plugin with an accidental
/// infinite loop gets aborted instead of freezing the server.
#[cfg(feature = "plugins")]
const MAX_OPERATIONS: u64 = 200_000;
/// Plugins whose hooks keep failing get disabled rather than spamming
/// the log forever.
#[cfg(feature = "plugins")]
const MAX_ERRORS: u32 = 10;

#[cfg(feature = "plugins")]
#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    /// Script filename relative to the plugin folder.
    #[serde(default = "default_script")]
    script: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[cfg(feature = "plugins")]
fn default_script() -> String {
    "main.rhai".to_string()
}
#[cfg(feature = "plugins")]
fn default_true() -> bool {
    true
}

/// Everything a script may ask the server to do, as data. The host
/// applies these after the script returns, so a plugin can't re-enter
/// the server mid-hook and deadlock it.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Broadcast(String),
    Tell { player: PlayerId, text: String },
    SetBlock { x: i32, y: i32, z: i32, block: BlockId },
    Kick { player: PlayerId, reason: String },
    SetTime(f32),
    Log { plugin: String, text: String },
    /// Move a player. The server sends it as a position correction, the
    /// same message the anti-cheat uses, so the client obeys it without
    /// needing to know a plugin was involved.
    Teleport { player: PlayerId, x: f32, y: f32, z: f32 },
    /// Turn a block into a falling entity, wherever it is.
    SpawnFallingBlock { x: i32, y: i32, z: i32, block: BlockId },
}

/// Most blocks a single `fill` may write.
///
/// A plugin asking for a million-block region would hold the tick loop
/// for as long as it took, and the tick loop is where every player's
/// snapshot comes from. The cap turns "the server froze" into "the
/// plugin got a smaller box than it asked for and was told so".
pub const MAX_FILL_BLOCKS: usize = 4096;

/// A read-only snapshot of the world, passed to the script's queries so
/// they never block on live server locks.
#[derive(Default, Clone)]
pub struct HostView {
    pub time_of_day: f32,
    /// The world's seed and the current tick. Constant and near-constant
    /// respectively, but a plugin has no other way to reach either, and
    /// both are what you need to write anything deterministic.
    pub seed: u32,
    pub tick: u64,
    pub players: Vec<(PlayerId, String, (f32, f32, f32))>,
    /// Blocks the host pre-fetched for this call (the cells around an
    /// edit). A script asking for anything else gets `None`.
    pub blocks: HashMap<(i32, i32, i32), BlockId>,
}

/// Shared between the host and the script bindings for one hook call.
#[cfg(feature = "plugins")]
#[derive(Default)]
struct CallState {
    effects: Vec<Effect>,
    view: HostView,
    plugin: String,
}

/// The no-scripting build. Same surface, no behaviour: `load_dir`
/// explains itself once and `fire` is a pair of empty answers, so the
/// server's hook call sites are identical in both builds.
#[cfg(not(feature = "plugins"))]
#[derive(Default)]
pub struct PluginHost;

#[cfg(not(feature = "plugins"))]
impl PluginHost {
    pub fn new() -> Self {
        Self
    }

    pub fn active_count(&self) -> usize {
        0
    }

    pub fn load_dir(&mut self, _dir: &Path) -> Vec<String> {
        vec!["plugin support is not compiled into this build".to_string()]
    }

    pub fn fire(&mut self, _hook: &str, _args: Vec<Value>, _view: &HostView) -> (bool, Vec<Effect>) {
        (true, Vec::new())
    }
}

#[cfg(feature = "plugins")]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub path: PathBuf,
    ast: AST,
    /// Per-plugin key/value storage, persisted with the world.
    store: HashMap<String, String>,
    errors: u32,
    pub disabled: bool,
}

#[cfg(feature = "plugins")]
pub struct PluginHost {
    engine: Engine,
    plugins: Vec<Plugin>,
    state: Arc<Mutex<CallState>>,
}

#[cfg(feature = "plugins")]
impl PluginHost {
    /// Builds the engine and registers the plugin API. No plugins are
    /// loaded yet -- call `load_dir`.
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(CallState::default()));
        let mut engine = Engine::new();
        engine.set_max_operations(MAX_OPERATIONS);
        // Depth limits keep a malformed script from blowing the stack
        // while parsing.
        engine.set_max_expr_depths(64, 64);

        register_api(&mut engine, Arc::clone(&state));

        Self {
            engine,
            plugins: Vec::new(),
            state,
        }
    }

    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    pub fn active_count(&self) -> usize {
        self.plugins.iter().filter(|p| !p.disabled).count()
    }

    /// Loads every plugin folder under `dir`. A plugin that fails to
    /// parse is reported and skipped; one bad plugin must not stop the
    /// server or the other plugins from starting.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut report = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
                report.push(format!("no plugin directory at {}", dir.display()));
                return report;
            }
        };

        let mut folders: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        folders.sort(); // deterministic load order

        for folder in folders {
            match self.load_one(&folder) {
                Ok(name) => report.push(format!("loaded plugin '{name}'")),
                Err(e) => report.push(format!("plugin at {} failed: {e}", folder.display())),
            }
        }
        report
    }

    fn load_one(&mut self, folder: &Path) -> Result<String, String> {
        let manifest_path = folder.join("plugin.toml");
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("can't read {}: {e}", manifest_path.display()))?;
        let manifest: PluginManifest =
            toml::from_str(&text).map_err(|e| format!("invalid plugin.toml: {e}"))?;

        if !manifest.enabled {
            return Err(format!("'{}' is disabled in its manifest", manifest.name));
        }

        let script_path = folder.join(&manifest.script);
        let source = std::fs::read_to_string(&script_path)
            .map_err(|e| format!("can't read {}: {e}", script_path.display()))?;
        let ast = self
            .engine
            .compile(&source)
            .map_err(|e| format!("script error: {e}"))?;

        self.plugins.push(Plugin {
            name: manifest.name.clone(),
            version: manifest.version,
            description: manifest.description,
            author: manifest.author,
            path: folder.to_path_buf(),
            ast,
            store: HashMap::new(),
            errors: 0,
            disabled: false,
        });
        Ok(manifest.name)
    }

    /// Calls a hook on every plugin that defines it.
    ///
    /// Returns `(allowed, effects)`. `allowed` is false if *any* plugin
    /// returned `false` -- a veto from one plugin is enough, which is
    /// what makes protection plugins composable.
    pub fn fire(
        &mut self,
        hook: &str,
        args: Vec<Value>,
        view: &HostView,
    ) -> (bool, Vec<Effect>) {
        let mut allowed = true;
        let mut effects = Vec::new();
        let args: Vec<Dynamic> = args.into_iter().map(to_dynamic).collect();

        for index in 0..self.plugins.len() {
            if self.plugins[index].disabled {
                continue;
            }

            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.effects.clear();
                state.view = view.clone();
                state.plugin = self.plugins[index].name.clone();
            }

            // The plugin's store is exposed as a scope variable, then
            // read back out, so scripts can keep state between calls
            // without the host handing out a live reference.
            let mut scope = Scope::new();
            let mut store_map = Map::new();
            for (key, value) in &self.plugins[index].store {
                store_map.insert(key.into(), Dynamic::from(value.clone()));
            }
            scope.push("store", store_map);

            let ast = self.plugins[index].ast.clone();
            let result: Result<Dynamic, _> =
                self.engine
                    .call_fn(&mut scope, &ast, hook, args_tuple(args.clone()));

            match result {
                Ok(value) => {
                    if value.is::<bool>() && !value.as_bool().unwrap_or(true) {
                        allowed = false;
                    }
                }
                Err(e) => {
                    // A missing hook is the normal case, not an error:
                    // most plugins implement one or two.
                    let message = e.to_string();
                    if !message.contains("Function not found") {
                        self.plugins[index].errors += 1;
                        effects.push(Effect::Log {
                            plugin: self.plugins[index].name.clone(),
                            text: format!("{hook} failed: {message}"),
                        });
                        if self.plugins[index].errors >= MAX_ERRORS {
                            self.plugins[index].disabled = true;
                            effects.push(Effect::Log {
                                plugin: self.plugins[index].name.clone(),
                                text: "disabled after too many errors".to_string(),
                            });
                        }
                    }
                }
            }

            // Persist whatever the script left in `store`.
            if let Some(map) = scope.get_value::<Map>("store") {
                let plugin = &mut self.plugins[index];
                plugin.store.clear();
                for (key, value) in map {
                    plugin.store.insert(key.to_string(), value.to_string());
                }
            }

            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            effects.append(&mut state.effects);
        }

        (allowed, effects)
    }
}

#[cfg(feature = "plugins")]
impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Rhai's `call_fn` wants a tuple; this adapts a runtime-length vector
/// to the arities the hooks actually use.
#[cfg(feature = "plugins")]
fn args_tuple(args: Vec<Dynamic>) -> impl rhai::FuncArgs {
    struct Args(Vec<Dynamic>);
    impl rhai::FuncArgs for Args {
        fn parse<C: Extend<Dynamic>>(self, container: &mut C) {
            container.extend(self.0);
        }
    }
    Args(args)
}

#[cfg(feature = "plugins")]
fn register_api(engine: &mut Engine, state: Arc<Mutex<CallState>>) {
    // --- actions ---
    let s = Arc::clone(&state);
    engine.register_fn("broadcast", move |text: &str| {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .effects
            .push(Effect::Broadcast(text.to_string()));
    });

    let s = Arc::clone(&state);
    engine.register_fn("tell", move |player: i64, text: &str| {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .effects
            .push(Effect::Tell {
                player: player as PlayerId,
                text: text.to_string(),
            });
    });

    let s = Arc::clone(&state);
    engine.register_fn("log", move |text: &str| {
        let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
        let plugin = guard.plugin.clone();
        guard.effects.push(Effect::Log {
            plugin,
            text: text.to_string(),
        });
    });

    let s = Arc::clone(&state);
    engine.register_fn("set_block", move |x: i64, y: i64, z: i64, block: i64| {
        // Validate here rather than trusting the script: an unknown id
        // would otherwise reach the world and render as a placeholder
        // for every client.
        let block = block as BlockId;
        if !is_known_block(block) {
            return;
        }
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .effects
            .push(Effect::SetBlock {
                x: x as i32,
                y: y as i32,
                z: z as i32,
                block,
            });
    });

    let s = Arc::clone(&state);
    engine.register_fn("kick", move |player: i64, reason: &str| {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .effects
            .push(Effect::Kick {
                player: player as PlayerId,
                reason: reason.to_string(),
            });
    });

    let s = Arc::clone(&state);
    engine.register_fn("set_time", move |t: f64| {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .effects
            .push(Effect::SetTime(t as f32));
    });

    // --- queries ---
    let s = Arc::clone(&state);
    engine.register_fn("time_of_day", move || -> f64 {
        s.lock().unwrap_or_else(|e| e.into_inner()).view.time_of_day as f64
    });

    let s = Arc::clone(&state);
    engine.register_fn("player_names", move || -> rhai::Array {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .players
            .iter()
            .map(|(_, name, _)| Dynamic::from(name.clone()))
            .collect()
    });

    let s = Arc::clone(&state);
    engine.register_fn("player_count", move || -> i64 {
        s.lock().unwrap_or_else(|e| e.into_inner()).view.players.len() as i64
    });

    let s = Arc::clone(&state);
    engine.register_fn("player_position", move |player: i64| -> rhai::Array {
        let guard = s.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .view
            .players
            .iter()
            .find(|(id, _, _)| *id == player as PlayerId)
            .map(|(_, _, (x, y, z))| {
                vec![
                    Dynamic::from(*x as f64),
                    Dynamic::from(*y as f64),
                    Dynamic::from(*z as f64),
                ]
            })
            .unwrap_or_default()
    });

    let s = Arc::clone(&state);
    engine.register_fn("get_block", move |x: i64, y: i64, z: i64| -> i64 {
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .blocks
            .get(&(x as i32, y as i32, z as i32))
            .copied()
            .map(|id| id as i64)
            .unwrap_or(-1) // -1 = "not available to this hook"
    });

    engine.register_fn("block_name", |id: i64| -> String {
        block_name(id as BlockId).to_string()
    });

    // --- the extended surface ---

    let s = Arc::clone(&state);
    engine.register_fn(
        "teleport",
        move |player: i64, x: f64, y: f64, z: f64| {
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .effects
                .push(Effect::Teleport {
                    player: player as PlayerId,
                    x: x as f32,
                    y: y as f32,
                    z: z as f32,
                });
        },
    );

    // Fills a box, inclusive of both corners.
    //
    // A loop of `set_block` in the script would be the obvious
    // alternative and is not usable: the engine's operation limit stops
    // a script long before it has placed a wall. Doing it in one call
    // means the *host* bounds the work, which is where the bound
    // belongs.
    let s = Arc::clone(&state);
    engine.register_fn(
        "fill",
        move |x0: i64, y0: i64, z0: i64, x1: i64, y1: i64, z1: i64, block: i64| -> i64 {
            let block = block as BlockId;
            if !is_known_block(block) {
                return 0;
            }
            let (lo_x, hi_x) = (x0.min(x1), x0.max(x1));
            let (lo_y, hi_y) = (y0.min(y1).max(0), y0.max(y1).min(CHUNK_SIZE_Y as i64 - 1));
            let (lo_z, hi_z) = (z0.min(z1), z0.max(z1));
            if lo_y > hi_y {
                return 0;
            }

            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let mut written = 0usize;
            'outer: for y in lo_y..=hi_y {
                for z in lo_z..=hi_z {
                    for x in lo_x..=hi_x {
                        if written >= MAX_FILL_BLOCKS {
                            break 'outer;
                        }
                        guard.effects.push(Effect::SetBlock {
                            x: x as i32,
                            y: y as i32,
                            z: z as i32,
                            block,
                        });
                        written += 1;
                    }
                }
            }
            // Returns what it actually did, so a script can notice it
            // was truncated instead of assuming the wall is there.
            written as i64
        },
    );

    let s = Arc::clone(&state);
    engine.register_fn(
        "spawn_falling_block",
        move |x: i64, y: i64, z: i64, block: i64| {
            let block = block as BlockId;
            if !is_known_block(block) {
                return;
            }
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .effects
                .push(Effect::SpawnFallingBlock {
                    x: x as i32,
                    y: y as i32,
                    z: z as i32,
                    block,
                });
        },
    );

    let s = Arc::clone(&state);
    engine.register_fn("world_seed", move || -> i64 {
        s.lock().unwrap_or_else(|e| e.into_inner()).view.seed as i64
    });

    let s = Arc::clone(&state);
    engine.register_fn("tick", move || -> i64 {
        s.lock().unwrap_or_else(|e| e.into_inner()).view.tick as i64
    });

    let s = Arc::clone(&state);
    engine.register_fn("player_id", move |name: &str| -> i64 {
        // -1 rather than 0: 0 is not a valid player id, but a script
        // that forgets to check would silently address player 0.
        s.lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .players
            .iter()
            .find(|(_, username, _)| username.eq_ignore_ascii_case(name))
            .map(|(id, _, _)| *id as i64)
            .unwrap_or(-1)
    });

    // The inverse of `block_name`, so scripts can say what they mean
    // instead of carrying a table of magic numbers that changes
    // whenever a block is added.
    engine.register_fn("block_id", |name: &str| -> i64 {
        ALL_BLOCK_IDS
            .iter()
            .find(|(_, known)| known.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id as i64)
            .unwrap_or(-1)
    });

    engine.register_fn("is_solid", |id: i64| -> bool {
        is_collidable(id as BlockId)
    });

    engine.register_fn("is_air", |id: i64| -> bool {
        (id as BlockId) == primitive_shared::types::BLOCK_AIR
    });
}

#[cfg(all(test, feature = "plugins"))]
mod extended_api_tests {
    use super::*;
    use crate::plugins::tests::host_with;

    fn view() -> HostView {
        HostView {
            time_of_day: 0.5,
            seed: 4242,
            tick: 99,
            players: vec![(7, "Shamkhan".to_string(), (1.0, 2.0, 3.0))],
            blocks: HashMap::new(),
        }
    }

    #[test]
    fn a_plugin_can_move_a_player() {
        // The canonical plugin -- /home, /spawn, /warp -- was impossible
        // before this: a script could talk to a player but not move one.
        let (mut host, _dir) = host_with("fn on_tick(t) { teleport(7, 10.0, 40.0, -5.0); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        match effects.first() {
            Some(Effect::Teleport { player, x, y, z }) => {
                assert_eq!(*player, 7);
                assert_eq!((*x, *y, *z), (10.0, 40.0, -5.0));
            }
            other => panic!("expected a teleport, got {other:?}"),
        }
    }

    #[test]
    fn fill_writes_a_whole_box_in_one_call() {
        // A loop of set_block in the script hits the engine's operation
        // limit long before it has built a wall.
        let (mut host, _dir) =
            host_with("fn on_tick(t) { fill(0, 10, 0, 2, 10, 2, block_id(\"stone\")); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        assert_eq!(effects.len(), 9, "3x1x3 should be nine blocks");
        assert!(effects.iter().all(|e| matches!(e, Effect::SetBlock { .. })));
    }

    #[test]
    fn fill_is_bounded_so_a_script_cannot_stall_the_tick_loop() {
        // The tick loop is where every player's snapshot comes from.
        let (mut host, _dir) =
            host_with("fn on_tick(t) { fill(0, 0, 0, 999, 63, 999, 3); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        assert_eq!(effects.len(), MAX_FILL_BLOCKS);
    }

    #[test]
    fn fill_reports_how_much_it_actually_did() {
        // So a script can notice it was truncated rather than assume the
        // wall is there.
        let (mut host, _dir) = host_with(
            "fn on_tick(t) { let n = fill(0, 0, 0, 999, 63, 999, 3); log(\"\" + n); }",
        );
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some(MAX_FILL_BLOCKS.to_string().as_str()));
    }

    #[test]
    fn fill_stays_inside_the_world_vertically() {
        let (mut host, _dir) =
            host_with("fn on_tick(t) { fill(0, -50, 0, 0, 500, 0, 3); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        for effect in &effects {
            if let Effect::SetBlock { y, .. } = effect {
                assert!((0..64).contains(y), "wrote outside the world at y={y}");
            }
        }
        assert_eq!(effects.len(), 64);
    }

    #[test]
    fn fill_refuses_a_block_that_does_not_exist() {
        let (mut host, _dir) = host_with("fn on_tick(t) { fill(0, 0, 0, 4, 0, 4, 9999); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        assert!(effects.is_empty(), "an unknown block reached the world");
    }

    #[test]
    fn a_plugin_can_drop_a_block() {
        let (mut host, _dir) =
            host_with("fn on_tick(t) { spawn_falling_block(1, 50, 2, block_id(\"sand\")); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        assert!(matches!(
            effects.first(),
            Some(Effect::SpawnFallingBlock { .. })
        ));
    }

    #[test]
    fn names_and_ids_round_trip() {
        // So scripts can say what they mean instead of carrying a table
        // of magic numbers that changes whenever a block is added.
        let (mut host, _dir) = host_with(
            "fn on_tick(t) { log(block_name(block_id(\"cobblestone\"))); }",
        );
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("cobblestone"));
    }

    #[test]
    fn an_unknown_block_name_is_minus_one_rather_than_a_valid_block() {
        let (mut host, _dir) =
            host_with("fn on_tick(t) { log(\"\" + block_id(\"cheese\")); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("-1"));
    }

    #[test]
    fn a_plugin_can_look_a_player_up_by_name() {
        let (mut host, _dir) =
            host_with("fn on_tick(t) { log(\"\" + player_id(\"shamkhan\")); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("7"), "name lookup should be case-insensitive");
    }

    #[test]
    fn an_absent_player_is_minus_one_not_zero() {
        // 0 is not a valid id, but a script that forgets to check would
        // silently address player 0 rather than nobody.
        let (mut host, _dir) =
            host_with("fn on_tick(t) { log(\"\" + player_id(\"nobody\")); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("-1"));
    }

    #[test]
    fn the_world_seed_and_tick_are_reachable() {
        // A plugin has no other way to reach either, and both are what
        // you need to write anything deterministic.
        let (mut host, _dir) =
            host_with("fn on_tick(t) { log(\"\" + world_seed() + \"/\" + tick()); }");
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("4242/99"));
    }

    #[test]
    fn block_properties_are_visible_to_scripts() {
        let (mut host, _dir) = host_with(
            "fn on_tick(t) { log(\"\" + is_solid(block_id(\"stone\")) + \" \" + is_air(0)); }",
        );
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view());
        let logged = effects.iter().find_map(|e| match e {
            Effect::Log { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(logged.as_deref(), Some("true true"));
    }
}

#[cfg(all(test, feature = "plugins"))]
mod tests {
    use super::*;

    pub(super) fn host_with(script: &str) -> (PluginHost, tempdir::TempDir) {
        let dir = tempdir::TempDir::new();
        let folder = dir.path().join("test_plugin");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("plugin.toml"),
            "name = \"test\"\nversion = \"1.0\"\n",
        )
        .unwrap();
        std::fs::write(folder.join("main.rhai"), script).unwrap();

        let mut host = PluginHost::new();
        let report = host.load_dir(dir.path());
        assert!(
            report.iter().any(|line| line.contains("loaded")),
            "plugin failed to load: {report:?}"
        );
        (host, dir)
    }

    /// Minimal temp directory helper -- no dev-dependency needed for
    /// something this small.
    mod tempdir {
        use std::path::{Path, PathBuf};
        pub struct TempDir(PathBuf);
        impl TempDir {
            pub fn new() -> Self {
                let base = std::env::temp_dir().join(format!(
                    "primitive-plugin-test-{}-{:?}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir_all(&base).unwrap();
                Self(base)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_plugin_can_broadcast_on_join() {
        let (mut host, _dir) = host_with(
            r#"fn on_join(player) { broadcast("welcome, player " + player); }"#,
        );
        let (allowed, effects) =
            host.fire("on_join", vec![Value::Int(7)], &HostView::default());
        assert!(allowed);
        assert_eq!(
            effects,
            vec![Effect::Broadcast("welcome, player 7".to_string())]
        );
    }

    #[test]
    fn returning_false_cancels_the_action() {
        // This is the whole point of the cancellable hooks: a protection
        // plugin must be able to refuse an edit before it happens.
        let (mut host, _dir) = host_with(
            r#"fn on_block_break(player, x, y, z) { if y < 5 { return false; } return true; }"#,
        );

        let deep = vec![
            Value::Int(1),
            Value::Int(0),
            Value::Int(3),
            Value::Int(0),
        ];
        let (allowed, _) = host.fire("on_block_break", deep, &HostView::default());
        assert!(!allowed, "the plugin should have vetoed this break");

        let shallow = vec![
            Value::Int(1),
            Value::Int(0),
            Value::Int(40),
            Value::Int(0),
        ];
        let (allowed, _) = host.fire("on_block_break", shallow, &HostView::default());
        assert!(allowed);
    }

    #[test]
    fn a_missing_hook_is_not_an_error() {
        let (mut host, _dir) = host_with(r#"fn on_join(player) { }"#);
        let (allowed, effects) = host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
        assert!(allowed);
        assert!(effects.is_empty(), "got unexpected effects: {effects:?}");
    }

    #[test]
    fn a_runaway_script_is_stopped_rather_than_hanging_the_server() {
        let (mut host, _dir) = host_with(r#"fn on_tick(t) { let i = 0; while true { i += 1; } }"#);
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Log { text, .. } if text.contains("failed"))),
            "the operation limit should have aborted the loop: {effects:?}"
        );
    }

    #[test]
    fn a_persistently_broken_plugin_gets_disabled() {
        let (mut host, _dir) = host_with(r#"fn on_tick(t) { throw "boom"; }"#);
        for _ in 0..MAX_ERRORS {
            host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
        }
        assert_eq!(host.active_count(), 0, "should have been disabled by now");

        // And it stops being called at all.
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
        assert!(effects.is_empty());
    }

    #[test]
    fn scripts_cannot_place_unknown_blocks() {
        let (mut host, _dir) = host_with(r#"fn on_tick(t) { set_block(1, 2, 3, 9999); }"#);
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
        assert!(
            effects.is_empty(),
            "an invalid block id must be rejected at the boundary: {effects:?}"
        );
    }

    #[test]
    fn a_plugin_keeps_state_between_calls() {
        let (mut host, _dir) = host_with(
            r#"
            fn on_tick(t) {
                let count = 0;
                if store.contains("count") { count = parse_int(store.count); }
                count += 1;
                store.count = "" + count;
                if count == 3 { broadcast("third tick"); }
            }
            "#,
        );
        for _ in 0..3 {
            let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &HostView::default());
            if !effects.is_empty() {
                assert_eq!(effects, vec![Effect::Broadcast("third tick".to_string())]);
                return;
            }
        }
        panic!("the store did not persist across calls");
    }

    #[test]
    fn a_plugin_can_read_the_world_snapshot() {
        let (mut host, _dir) = host_with(
            r#"fn on_tick(t) { broadcast("block is " + block_name(get_block(1, 2, 3))); }"#,
        );
        let mut view = HostView::default();
        view.blocks
            .insert((1, 2, 3), primitive_shared::types::BLOCK_STONE);
        let (_, effects) = host.fire("on_tick", vec![Value::Int(1)], &view);
        assert_eq!(effects, vec![Effect::Broadcast("block is stone".to_string())]);
    }

    #[test]
    fn a_broken_plugin_does_not_stop_the_others_loading() {
        let dir = tempdir::TempDir::new();
        for (name, script) in [("aaa_broken", "fn on_join( {"), ("zzz_good", "fn on_join(p) { }")] {
            let folder = dir.path().join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("plugin.toml"),
                format!("name = \"{name}\"\n"),
            )
            .unwrap();
            std::fs::write(folder.join("main.rhai"), script).unwrap();
        }

        let mut host = PluginHost::new();
        let report = host.load_dir(dir.path());
        assert!(report.iter().any(|l| l.contains("failed")), "{report:?}");
        assert_eq!(host.active_count(), 1, "the good plugin should still load");
    }

    #[test]
    fn a_disabled_plugin_is_skipped() {
        let dir = tempdir::TempDir::new();
        let folder = dir.path().join("off");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("plugin.toml"),
            "name = \"off\"\nenabled = false\n",
        )
        .unwrap();
        std::fs::write(folder.join("main.rhai"), "fn on_join(p) { }").unwrap();

        let mut host = PluginHost::new();
        host.load_dir(dir.path());
        assert_eq!(host.active_count(), 0);
    }

    #[test]
    fn plugins_load_in_a_deterministic_order() {
        // Load order decides which plugin sees an event first, so it
        // must not depend on the filesystem's whim.
        let dir = tempdir::TempDir::new();
        for name in ["ccc", "aaa", "bbb"] {
            let folder = dir.path().join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("plugin.toml"), format!("name = \"{name}\"\n")).unwrap();
            std::fs::write(folder.join("main.rhai"), "fn on_join(p) { }").unwrap();
        }
        let mut host = PluginHost::new();
        host.load_dir(dir.path());
        let names: Vec<&str> = host.plugins().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["aaa", "bbb", "ccc"]);
    }
}
