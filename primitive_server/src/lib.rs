//! The Primitive server, as a library.
//!
//! Shape of the process:
//! - an **accept loop** (this file), which does nothing but hand new
//!   sockets to `connection::handle_connection`;
//! - **three tasks per connected client** (reader / writer / chunk pump,
//!   see `connection`);
//! - one **tick loop**, which is where everything that scales with player
//!   count lives: sampling player positions, building per-player
//!   interest-filtered snapshots, keepalives and timeouts;
//! - background **autosave** and **stats** tasks.
//!
//! The tick loop is deliberately the only place that touches all players
//! at once, and it does exactly one pass over them per tick. Movement is
//! not relayed message-by-message any more: a client sending 30 updates a
//! second no longer causes 30 broadcasts a second to everyone else, it
//! just changes what the next snapshot says.
//!
//! ## Why a library and not just a binary
//!
//! Singleplayer is this same server, started in-process by the client on
//! the loopback interface. That is a deliberate choice over the usual
//! alternative -- a second, simpler, offline code path inside the client
//! -- because a second code path is a second set of physics, a second
//! world generator and a second falling-sand simulation, and they drift.
//! Here there is exactly one implementation of the world, and
//! singleplayer differs from multiplayer only in who owns the process.
//!
//! [`start`] is the entry point either way: [`RunOptions::standalone`]
//! for the `primitive_server` binary, [`RunOptions::embedded`] for the
//! client. See [`Server`] for the handle it hands back.

// The two layers the server is built out of, each a directory with its
// own `mod.rs` saying what belongs in it:
//
//   net    -- sockets, framing, and who is on the other end
//   logic  -- the rules the server is authoritative about
//
// `settings` belongs to neither: it is read once at startup and both
// layers consult it. This file is the third thing -- the process
// itself: the accept loop, the tick loop, and the wiring between them.
pub mod logic;
pub mod net;
pub mod settings;

// The old flat paths, kept because they are this crate's public surface:
// the client and the tests say `primitive_server::items`, and moving a
// file inside the crate is not a reason to break them.
pub use logic::{
    anticheat, commands, containers, falling, items, plugins, profiles, simulation, survival, water,
    world,
};
pub use net::players;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;

use primitive_shared::protocol::{
    BlockChange, DisconnectReason, PlayerId, PlayerState, ServerMessage, Side,
};
use primitive_shared::types::ChunkPos;

use players::Registry;
use settings::ServerSettings;
use world::World;

/// World clock driving the day/night cycle. Every client derives sun
/// direction, sky colour and skylight strength from this, so everyone
/// sees the same sky at the same moment -- lighting is server-synced
/// state, not a local animation.
pub struct WorldClock {
    /// (when the current epoch started, what the time of day was then).
    /// `/time` rebases both rather than nudging a running counter, so the
    /// clock keeps advancing smoothly from wherever it was set to.
    origin: std::sync::Mutex<(Instant, f32)>,
    day_length_seconds: f32,
    tick: AtomicU64,
}

impl WorldClock {
    fn new(start_time_of_day: f32, day_length_seconds: f32) -> Self {
        Self {
            origin: std::sync::Mutex::new((Instant::now(), start_time_of_day.rem_euclid(1.0))),
            day_length_seconds,
            tick: AtomicU64::new(0),
        }
    }

    /// 0.0 = midnight, 0.5 = noon.
    pub fn time_of_day(&self) -> f32 {
        let (started, base) = *self.origin.lock().unwrap_or_else(|e| e.into_inner());
        let elapsed = started.elapsed().as_secs_f32();
        (base + elapsed / self.day_length_seconds).rem_euclid(1.0)
    }

    /// Jumps the world clock (the `/time` command).
    pub fn set_time_of_day(&self, time_of_day: f32) {
        let mut origin = self.origin.lock().unwrap_or_else(|e| e.into_inner());
        *origin = (Instant::now(), time_of_day.rem_euclid(1.0));
    }

    pub fn day_length_seconds(&self) -> f32 {
        self.day_length_seconds
    }

    pub fn tick(&self) -> u64 {
        self.tick.load(Ordering::Relaxed)
    }

    fn advance(&self) -> u64 {
        self.tick.fetch_add(1, Ordering::Relaxed) + 1
    }
}

#[derive(Default)]
pub struct Metrics {
    pub messages_in: AtomicU64,
    pub chunks_sent: AtomicU64,
    pub block_edits: AtomicU64,
    pub snapshots_sent: AtomicU64,
    pub kicks: AtomicU64,
    pub anticheat_flags: AtomicU64,
    pub falling_entities: AtomicU64,
    pub ticks: AtomicU64,
    pub tick_overruns: AtomicU64,
}

pub struct Context {
    pub settings: ServerSettings,
    /// How this instance was started. Carried here so the code that
    /// logs (connection churn, chat, anti-cheat flags) can stay quiet
    /// for an embedded server, whose stdout belongs to the game.
    pub options: RunOptions,
    pub world: Arc<World>,
    /// Falling-block simulation. A plain mutex: it's touched once per
    /// tick and once per block edit, so contention is a non-issue and a
    /// lock-free structure would be complexity for nothing.
    pub falling: std::sync::Mutex<falling::FallingBlocks>,
    /// Every other cell-watching mechanic, stepped on the same tick and
    /// broadcast through the same path.
    ///
    /// Water is registered here, which is what the registry was built
    /// for: sand keeps its own field because the entity replication path
    /// asks it directly for what is in the air, and everything after it
    /// is a `CellMechanic` and a `register` call rather than another
    /// loop wired into the tick by hand. Fire that spreads and grass
    /// that creeps would land the same way. See `logic::simulation`.
    pub mechanics: std::sync::Mutex<simulation::Mechanics>,
    /// Dropped stacks lying in the world. Same reasoning as `falling`.
    pub items: std::sync::Mutex<items::Items>,
    /// What is inside the chests. Touched only when someone has one
    /// open, so a plain mutex is more than enough.
    pub chests: std::sync::Mutex<containers::Chests>,
    /// Scripted plugins. One mutex around the whole host: hooks are
    /// short, and running two scripts concurrently would make plugin
    /// authors reason about data races in a scripting language.
    pub plugins: std::sync::Mutex<plugins::PluginHost>,
    /// Who has played here, and what they had when they left. Keyed by
    /// UUID; see `profiles`.
    pub profiles: std::sync::Mutex<profiles::Profiles>,
    pub registry: Arc<Registry>,
    pub clock: Arc<WorldClock>,
    pub metrics: Arc<Metrics>,
    /// Where the world is saved; `None` disables persistence.
    pub world_dir: Option<PathBuf>,
    /// Set by `/stop`, by Ctrl-C, and by the client leaving a
    /// singleplayer world.
    ///
    /// A `watch` channel rather than a `Notify`, because `Notify` only
    /// wakes tasks that are *already* waiting: a stop that arrives
    /// between spawning the accept loop and its first poll would be
    /// dropped on the floor, and the server would never come down. A
    /// watch channel holds the value, so the race has no window --
    /// which matters most for the shortest-lived servers of all, the
    /// ones a test starts and immediately stops.
    shutdown: tokio::sync::watch::Sender<bool>,
    pub started: Instant,
}

impl Context {
    /// Asks everything watching to wind up. Idempotent.
    ///
    /// `send_replace`, not `send`: `send` reports an error *and leaves
    /// the value unchanged* when no receiver happens to be alive at that
    /// instant. Receivers here are created on demand inside
    /// `shutdown_requested`, so whether one exists depends on exactly
    /// where the accept loop is in its `select!` -- and a stop that
    /// landed in the gap was silently discarded, leaving a server that
    /// could never be shut down. `send_replace` always stores the value.
    pub fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    pub fn is_shutting_down(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// Resolves as soon as shutdown has been requested -- including when
    /// it was requested before this was ever called.
    pub async fn shutdown_requested(&self) {
        let mut receiver = self.shutdown.subscribe();
        // `subscribe` marks the current value as seen, so a shutdown
        // that already happened has to be caught here rather than by
        // `changed()`, which would wait for a *second* one.
        if *receiver.borrow_and_update() {
            return;
        }
        let _ = receiver.changed().await;
    }
}

/// How a particular server instance should behave around the edges: the
/// things that differ between "an operator ran the binary" and "the game
/// started a world for one player".
#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    /// Load and run scripted plugins. Off for singleplayer -- see the
    /// `plugins` module for why that isn't just a policy choice but a
    /// compile-time one as well.
    pub plugins: bool,
    /// Read operator commands from stdin. A server embedded in the game
    /// client must not, or it would steal the terminal from the client.
    pub console: bool,
    /// Print the startup banner and the periodic stats line.
    pub logging: bool,
}

impl RunOptions {
    /// The `primitive_server` binary: everything on.
    pub fn standalone() -> Self {
        Self {
            plugins: true,
            console: true,
            logging: true,
        }
    }

    /// A world running inside the game client: no plugins, no console,
    /// and quiet, because its stdout is the player's.
    pub fn embedded() -> Self {
        Self {
            plugins: false,
            console: false,
            logging: false,
        }
    }
}

/// A running server.
///
/// Holds the address it actually bound to, which matters because an
/// embedded server asks for port 0 and lets the OS choose -- the client
/// cannot connect to "0.0.0.0:0", and hard-coding a port would mean two
/// copies of the game couldn't run side by side.
pub struct Server {
    address: SocketAddr,
    ctx: Arc<Context>,
    accept: tokio::task::JoinHandle<()>,
    world_dir: Option<PathBuf>,
}

impl Server {
    /// Where clients should connect.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// How many plugins are loaded and enabled. Always zero for an
    /// embedded server, and always zero in a build without the `plugins`
    /// feature.
    pub fn plugin_count(&self) -> usize {
        self.ctx
            .plugins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_count()
    }

    /// Asks the server to stop, without waiting for it.
    pub fn request_shutdown(&self) {
        self.ctx.request_shutdown();
    }

    /// Runs a command as the console does, and returns what it said.
    ///
    /// The same path chat commands take, at console permission -- so a
    /// test can do the operator things a client is not allowed to ask
    /// for, and an embedded server has a way to be driven that does not
    /// involve typing into a window it does not own.
    pub fn console_command(&self, line: &str) -> Vec<String> {
        run_command(&self.ctx, line, commands::Permission::Operator, None)
    }

    /// Puts a block into the world directly, as the world generator
    /// would have.
    ///
    /// For tests and tools: everything a *player* does goes through
    /// `SetBlock` and is checked against what they are carrying, which
    /// is exactly the machinery a test of something else does not want
    /// to have to satisfy.
    pub fn place_block(&self, x: i32, y: i32, z: i32, block: primitive_shared::types::BlockId) {
        let (chunk_pos, _, _) = ChunkPos::from_global(x, z);
        // Loaded first, because `set_block` only updates a chunk that is
        // already cached -- and everything that *reads* a block outside
        // the chunk pump reads the cache and nothing else, deliberately
        // (see `World::cached`). Placing into an uncached chunk would
        // record the edit and leave the world still answering with what
        // the generator put there.
        if self.ctx.world.cached(chunk_pos).is_none() {
            let chunk = self.ctx.world.generate(chunk_pos);
            self.ctx.world.insert(chunk);
        }
        if !self.ctx.world.set_block(x, y, z, block) {
            return;
        }
        let change = BlockChange {
            global_x: x,
            global_y: y,
            global_z: z,
            block_id: block,
        };
        for subscriber in self.ctx.registry.subscribers(chunk_pos) {
            subscriber.send(ServerMessage::BlockUpdate(change));
        }
    }

    /// Runs until something stops it (`/stop`, or `request_shutdown`),
    /// then disconnects players and saves.
    pub async fn wait(self) {
        let _ = self.accept.await;
        shutdown(&self.ctx, self.world_dir.clone()).await;
    }

    /// Stops the server and waits for the world to be saved.
    ///
    /// Worth waiting for: this is what runs when a singleplayer session
    /// ends, and returning before the save completes would lose whatever
    /// the player built since the last autosave.
    pub async fn stop(self) {
        self.request_shutdown();
        self.wait().await;
    }
}

/// Starts a server and returns once it is accepting connections.
///
/// Binding happens here rather than inside the spawned task so that a
/// port already in use is an error the caller can show, and so the
/// chosen port is known by the time this returns.
pub async fn start(settings: ServerSettings, options: RunOptions) -> anyhow::Result<Server> {
    let listener = TcpListener::bind(&settings.bind_addr)
        .await
        .map_err(|e| anyhow::anyhow!("could not bind {}: {e}", settings.bind_addr))?;
    let address = listener.local_addr()?;

    let ctx = build_context(settings, options)?;

    if options.logging {
        let spawn = ctx.world.spawn_point();
        println!(
            "[server] \"{}\" listening on {} | seed {} | tick {:.0} Hz | view {} chunks | \
             max {} players | anti-cheat {} | spawn ({:.1}, {:.1}, {:.1})",
            ctx.settings.server_name,
            address,
            ctx.world.seed(),
            ctx.settings.tick_rate_hz,
            ctx.settings.view_distance_chunks,
            ctx.settings.max_players,
            if ctx.settings.anticheat.enabled { "on" } else { "OFF" },
            spawn.0,
            spawn.1,
            spawn.2,
        );
    }

    // --- plugins ---
    if options.plugins {
        {
            let mut host = ctx.plugins.lock().unwrap_or_else(|e| e.into_inner());
            for line in host.load_dir(&PathBuf::from(&ctx.settings.plugin_dir)) {
                println!("[plugins] {line}");
            }
            println!("[plugins] {} active", host.active_count());
        }
        fire_plugin_hook(&ctx, "on_load", Vec::new(), None);
    }

    tokio::spawn(tick_loop(Arc::clone(&ctx)));
    if options.console {
        tokio::spawn(console_loop(Arc::clone(&ctx)));
    }
    let world_dir = ctx.world_dir.clone();
    if let Some(dir) = world_dir.clone() {
        tokio::spawn(autosave_loop(Arc::clone(&ctx), dir));
    }
    if options.logging && ctx.settings.stats_interval_secs > 0.0 {
        tokio::spawn(stats_loop(Arc::clone(&ctx)));
    }

    let accept = tokio::spawn(accept_loop(Arc::clone(&ctx), listener));

    Ok(Server {
        address,
        ctx,
        accept,
        world_dir,
    })
}

/// Starts a server and runs it to completion, also stopping on Ctrl-C.
///
/// Ctrl-C handling lives here rather than in `start` because an embedded
/// server has no business intercepting the *game's* interrupt.
pub async fn run(settings: ServerSettings, options: RunOptions) -> anyhow::Result<()> {
    let server = start(settings, options).await?;
    let ctx = Arc::clone(&server.ctx);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\n[server] shutdown requested");
            ctx.request_shutdown();
        }
    });
    server.wait().await;
    Ok(())
}

fn build_context(settings: ServerSettings, options: RunOptions) -> anyhow::Result<Arc<Context>> {
    let world = Arc::new(World::new(settings.world_seed, settings.max_cached_chunks));
    let world_dir = if settings.world_dir.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(&settings.world_dir))
    };
    if let Some(dir) = &world_dir {
        match world.load(dir) {
            Ok(0) if options.logging => println!("[world] no saved edits in {}", dir.display()),
            Ok(0) => {}
            Ok(n) if options.logging => {
                println!("[world] restored {n} block edit(s) from {}", dir.display())
            }
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load {}: {e}", dir.display()),
        }
    }

    // Chests, from their own file beside the world's. A world saved
    // before chests existed has no such file, which reads as "no
    // chests" -- see `containers`.
    let mut chests = containers::Chests::new();
    if let Some(dir) = &world_dir {
        match chests.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] restored {n} chest(s)"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load chests: {e}"),
        }
    }

    let mut profiles = profiles::Profiles::new();
    if let Some(dir) = &world_dir {
        match profiles.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[players] restored {n} player profile(s)"),
            Ok(_) => {}
            Err(e) => eprintln!("[players] could not load profiles: {e}"),
        }
    }

    let ctx = Arc::new(Context {
        registry: Arc::new(Registry::new(
            settings.max_players,
            settings.max_connections_per_ip,
        )),
        clock: Arc::new(WorldClock::new(
            settings.start_time_of_day,
            settings.day_length_seconds,
        )),
        metrics: Arc::new(Metrics::default()),
        world: Arc::clone(&world),
        falling: std::sync::Mutex::new(falling::FallingBlocks::new()),
        mechanics: std::sync::Mutex::new({
            let mut mechanics = simulation::Mechanics::new();
            // Water, which is the second mechanic written to this shape
            // and the first one that did not come with it. Registered
            // here rather than constructed with the world, because a
            // mechanic is a thing the server runs and not a thing the
            // world contains.
            mechanics.register(Box::new(water::Water::new()));
            mechanics
        }),
        items: std::sync::Mutex::new(items::Items::new()),
        chests: std::sync::Mutex::new(chests),
        plugins: std::sync::Mutex::new(plugins::PluginHost::new()),
        profiles: std::sync::Mutex::new(profiles),
        settings,
        options,
        world_dir,
        shutdown: tokio::sync::watch::channel(false).0,
        started: Instant::now(),
    });

    Ok(ctx)
}

/// Hands new sockets to `connection::handle_connection` and nothing else.
/// Returns when the server is asked to stop.
async fn accept_loop(ctx: Arc<Context>, listener: TcpListener) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((socket, addr)) => {
                        let ctx = Arc::clone(&ctx);
                        tokio::spawn(net::connection::handle_connection(ctx, socket, addr));
                    }
                    Err(e) => {
                        // A per-connection accept error (fd exhaustion,
                        // for instance) must not take the server down.
                        eprintln!("[net] accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
            _ = ctx.shutdown_requested() => break,
        }
    }
}

/// One pass per tick over every connected player. Everything that is
/// O(players) lives here, once, instead of being triggered O(messages)
/// times by clients.
async fn tick_loop(ctx: Arc<Context>) {
    let tick_duration = ctx.settings.tick_duration();
    let mut ticker = tokio::time::interval(tick_duration);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let keepalive_every =
        ((ctx.settings.keepalive_interval_secs * ctx.settings.tick_rate_hz) as u64).max(1);
    let time_sync_every = ((2.0 * ctx.settings.tick_rate_hz) as u64).max(1);
    let client_timeout = Duration::from_secs_f32(ctx.settings.client_timeout_secs);
    let radius = ctx.settings.interest_radius_blocks;

    // Reused every tick for every player. These were two fresh `Vec`s
    // per player per tick -- at 20 Hz and a full server, tens of
    // thousands of allocations a second for data that is serialised
    // and dropped immediately.
    let mut visible: Vec<PlayerState> = Vec::new();
    let mut visible_entities: Vec<primitive_shared::protocol::EntityState> = Vec::new();

    loop {
        // Every background loop exits on shutdown rather than running
        // until the process does. That is invisible for the standalone
        // binary, whose shutdown *is* the process ending -- but the
        // client starts and stops a server for every singleplayer
        // session, and a tick loop per world left running would go on
        // ticking, saving and simulating for the rest of the game.
        tokio::select! {
            _ = ticker.tick() => {}
            _ = ctx.shutdown_requested() => break,
        }
        let started = Instant::now();
        ctx.world.refresh_clock();
        let tick = ctx.clock.advance();
        ctx.metrics.ticks.fetch_add(1, Ordering::Relaxed);

        // --- plugins ---
        // Once a second rather than every tick: a script that runs 20
        // times a second is a footgun for plugin authors, and nothing
        // a plugin does here needs tick precision.
        if tick.is_multiple_of((ctx.settings.tick_rate_hz as u64).max(1)) {
            fire_plugin_hook(&ctx, "on_tick", vec![crate::logic::plugins::Value::Int(tick as i64)], None);
        }

        // --- falling blocks ---
        // Every tick, with the real timestep: falling blocks are
        // entities now, so this integrates their motion rather than
        // teleporting them one cell at a time.
        {
            let (mut changes, entity_count) = {
                let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                let changes = sim.step(&*ctx.world, tick_duration.as_secs_f32());
                (changes, sim.entity_count())
            };
            // ...and anything else registered, on the same tick and
            // into the same batch. Water is the one that ships; the
            // emptiness check keeps this to a lock and a length for a
            // server that has none.
            {
                let mut mechanics = ctx.mechanics.lock().unwrap_or_else(|e| e.into_inner());
                if !mechanics.is_empty() {
                    changes.extend(mechanics.step(&*ctx.world, tick_duration.as_secs_f32()));
                }
            }
            // A column of sand sliding away takes whatever was growing
            // on top of it. Done here rather than inside the simulation
            // because the simulation is about sand: it has no opinion on
            // plants, and giving it one would mean teaching it about
            // drops and inventories too.
            if !changes.is_empty() {
                let mut fallen = Vec::new();
                for change in &changes {
                    fallen.extend(collapse_unsupported(
                        &ctx,
                        change.global_x,
                        change.global_y,
                        change.global_z,
                    ));
                }
                changes.extend(fallen);
            }
            ctx.metrics
                .falling_entities
                .store(entity_count as u64, Ordering::Relaxed);

            if !changes.is_empty() {
                // Grouped by chunk so each player gets one batched
                // message per affected chunk, through the same
                // subscriber index manual edits use.
                let mut by_chunk: std::collections::HashMap<ChunkPos, Vec<BlockChange>> =
                    std::collections::HashMap::new();
                for change in changes {
                    let (pos, _, _) = ChunkPos::from_global(change.global_x, change.global_z);
                    by_chunk.entry(pos).or_default().push(change);
                }
                for (pos, batch) in by_chunk {
                    // Serialised once per chunk, not once per recipient:
                    // every subscriber gets the same bytes, and the old
                    // shape deep-cloned the whole change list for each
                    // of them just so each writer task could bincode an
                    // identical copy.
                    let Some(frame) = players::frame(&ServerMessage::BlockUpdates(batch)) else {
                        continue;
                    };
                    for subscriber in ctx.registry.subscribers(pos) {
                        subscriber.send_raw(Arc::clone(&frame));
                    }
                }
            }
        }

        // Everyone currently connected, sampled once for the whole
        // tick.
        //
        // This used to be asked for separately by each pass that wanted
        // it -- the item pickup, the snapshots, the regeneration -- and
        // each ask copies the map into a fresh `Vec` of reference
        // counts. Three copies a tick is nothing at two players and
        // exactly the shape of thing that stops being nothing at two
        // hundred.
        let handles = ctx.registry.handles();

        // --- dropped items ---
        //
        // Stepped every tick, then offered to each player in range. The
        // pickup pass holds the item list and one player's lock at a
        // time, never both across an await -- there are none here.
        {
            let now = Instant::now();
            // Which cells hold items at all, sampled once after the
            // step. The pickup pass below filters players against this
            // *before* touching the items mutex or their own state
            // lock: on a big server nearly everyone is standing nowhere
            // near a drop, and the old shape took both locks for each
            // of them anyway just to find that out.
            let occupied = {
                let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
                items.step(&ctx.world, tick_duration.as_secs_f32(), now);
                items.occupied_cells()
            };

            if !occupied.is_empty() {
                for handle in &handles {
                    let (feet, dead) = {
                        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        (state.position, state.vitals.is_dead())
                    };
                    if dead {
                        continue; // a corpse does not pick things up
                    }
                    if !items::Items::any_within_reach(feet, &occupied) {
                        continue; // nothing on the ground anywhere near
                    }
                    let mut took_something = false;
                    {
                        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        items.collect_near(handle.id, feet, now, |block, count| {
                            // Whatever does not fit stays on the ground.
                            let left = state.inventory.add(block, count);
                            let taken = count - left;
                            if taken > 0 {
                                state.inventory_dirty = true;
                                took_something = true;
                            }
                            taken
                        });
                    }
                    if took_something {
                        send_inventory(handle);
                    }
                }
            }
        }

        // Falling blocks and dropped items, bucketed the same way the
        // players are: built once per tick, queried once per player, so
        // the cost is what is actually near people rather than
        // players x entities. See `EntityGrid`.
        let entity_grid = {
            let sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
            let items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
            let states: Vec<primitive_shared::protocol::EntityState> = sim
                .entities()
                .iter()
                .map(|e| e.state())
                .chain(items.states())
                .collect();
            players::EntityGrid::build(states, radius)
        };

        if !handles.is_empty() {
            // Sample every player once, then reuse that sample for all
            // recipients. Sampling per recipient would be O(n²) lock
            // acquisitions instead of O(n).
            let states: Vec<(PlayerId, PlayerState)> = handles
                .iter()
                .map(|h| (h.id, h.player_state()))
                .collect();
            // Built once per tick and queried once per player, instead
            // of every player being compared against every other. See
            // `InterestGrid` for the arithmetic that made this worth
            // doing.
            let grid = players::InterestGrid::build(states, radius);

            for handle in &handles {
                let origin = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.position
                };
                grid.nearby(origin, radius, handle.id, &mut visible);
                if !visible.is_empty() {
                    handle.send(ServerMessage::Snapshot {
                        tick,
                        states: visible.clone(),
                    });
                    ctx.metrics.snapshots_sent.fetch_add(1, Ordering::Relaxed);
                }

                // Entities get the same interest filtering as players,
                // and the same "no explicit despawn" contract: a client
                // drops whatever it stops hearing about.
                if !entity_grid.is_empty() {
                    entity_grid.nearby(origin, radius, &mut visible_entities);
                    if !visible_entities.is_empty() {
                        handle.send(ServerMessage::Entities {
                            tick,
                            states: visible_entities.clone(),
                        });
                    }
                }
            }

            // Breathing, then regeneration. Both every tick, because
            // both are continuous -- but `Vitals` only reports a change
            // once it adds up to something worth a packet, so this is
            // almost always free.
            //
            // Drowning is judged against the *server's* world at the
            // server's copy of the player's position, for the same
            // reason the swim check in the anti-cheat is: a client that
            // decided for itself whether its own head was under water
            // would be a client that never drowns.
            for handle in &handles {
                let (position, dead) = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    (state.position, state.vitals.is_dead())
                };
                if !dead {
                    let eye = (
                        position.0,
                        position.1 + primitive_shared::geometry::EYE_HEIGHT,
                        position.2,
                    );
                    // How deep the cell at eye level actually is, not
                    // merely whether it holds water: a cell can hold an
                    // eighth of a puddle now that water flows, and the
                    // client draws -- and the collider reads -- exactly
                    // this line. See `fluid::covers_with_above`.
                    //
                    // **And the cell over it**, which is what makes the
                    // difference at depth. A full cell of water stops
                    // `SURFACE_DROP` short of its ceiling, so without
                    // the block above in hand the top twelve per cent of
                    // every submerged cell read as air: a player on the
                    // sea floor whose eyes landed in that band had their
                    // breath handed back every tick, and drowning deep
                    // under water was a matter of where you happened to
                    // be standing.
                    let eye_cell = (
                        eye.0.floor() as i32,
                        eye.1.floor() as i32,
                        eye.2.floor() as i32,
                    );
                    let head_under = ctx
                        .world
                        .cached_block(eye_cell.0, eye_cell.1, eye_cell.2)
                        .is_some_and(|block| {
                            let above = ctx
                                .world
                                .cached_block(eye_cell.0, eye_cell.1 + 1, eye_cell.2)
                                .unwrap_or(primitive_shared::types::BLOCK_AIR);
                            primitive_shared::fluid::covers_with_above(
                                block,
                                above,
                                eye.1 - eye.1.floor(),
                            )
                        });
                    let drowning = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.vitals.breathe(head_under, tick_duration.as_secs_f32())
                    };
                    // `report_vitals` already announces a death, closes
                    // the chest screen and fires the plugin hook.
                    if !matches!(drowning, survival::Outcome::Unchanged) {
                        report_vitals(&ctx, handle, drowning);
                    }
                    // The meter, when it has changed. A player who
                    // drowns with no warning on screen has been ambushed
                    // by a rule -- the fog says "under water", not "for
                    // how much longer".
                    //
                    // **On change, including the change back to full.**
                    // Sending only while the head was under meant the
                    // last thing a client ever heard was "nearly out of
                    // air", and it kept the bar on screen for the rest
                    // of the session -- through surfacing, through
                    // drowning, through respawning.
                    let breath = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let now = state.vitals.breath_fraction();
                        // A hundredth is under a pixel of the bar, and
                        // this runs twenty times a second per player.
                        if (now - state.breath_reported).abs() > 0.01 {
                            state.breath_reported = now;
                            Some(now)
                        } else {
                            None
                        }
                    };
                    if let Some(fraction) = breath {
                        handle.send(ServerMessage::Breath { fraction });
                    }
                }

                let outcome = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.regenerate(tick_duration.as_secs_f32())
                };
                let needs_report = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.needs_report()
                };
                if matches!(outcome, survival::Outcome::Changed) && needs_report {
                    report_vitals(&ctx, handle, outcome);
                }
            }

            // Both of these say the same thing to everyone, so both are
            // serialised once and fanned out as shared bytes rather
            // than bincoded again in every writer task.
            if tick.is_multiple_of(keepalive_every) {
                let ping = players::frame(&ServerMessage::Ping { nonce: tick });
                for handle in &handles {
                    if handle.idle_for() > client_timeout {
                        handle.request_kick(DisconnectReason::Timeout);
                    } else if let Some(ping) = &ping {
                        handle.send_raw(Arc::clone(ping));
                    }
                }
            }

            if tick.is_multiple_of(time_sync_every) {
                let time_of_day = ctx.clock.time_of_day();
                if let Some(sync) = players::frame(&ServerMessage::TimeSync { tick, time_of_day }) {
                    for handle in &handles {
                        handle.send_raw(Arc::clone(&sync));
                    }
                }
            }
        }

        if started.elapsed() > tick_duration {
            ctx.metrics.tick_overruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

async fn autosave_loop(ctx: Arc<Context>, dir: PathBuf) {
    let mut ticker = tokio::time::interval(Duration::from_secs_f32(
        ctx.settings.autosave_interval_secs.max(5.0),
    ));
    ticker.tick().await; // the first tick fires immediately; skip it
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = ctx.shutdown_requested() => break,
        }
        // Players first: their packs and places of exit are cheap to
        // write and are what a crash would be most annoying to lose.
        //
        // On a blocking worker, like the world save below it. Writing a
        // file from an async task parks a runtime thread that is also
        // driving player sockets and the tick loop -- which in
        // singleplayer is the same process as the game, so it shows up
        // as the world stuttering every autosave.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_profiles(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[players] saved {n} profile(s)");
            }
        }

        // Chests before the world's own guard below: they change
        // without any block changing, so a world whose overlay is clean
        // would otherwise never write them.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_chests(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[world] saved {n} chest(s)");
            }
        }

        if !ctx.world.has_unsaved_changes() {
            continue;
        }
        let world = Arc::clone(&ctx.world);
        let dir = dir.clone();
        // Serialising the overlay is blocking I/O; keep it off the async
        // workers that are driving player sockets.
        match tokio::task::spawn_blocking(move || world.save(&dir)).await {
            Ok(Ok(n)) => println!("[world] autosaved {n} block edit(s)"),
            Ok(Err(e)) => eprintln!("[world] autosave failed: {e}"),
            Err(e) => eprintln!("[world] autosave task failed: {e}"),
        }
    }
}

/// Writes the chests out, quietly doing nothing if none changed.
///
/// Synchronous, like the profiles and unlike the world: it is a handful
/// of inventories, and the lock is one nobody else is holding for long.
///
/// Answers how many were written -- `None` for "nothing to do", which is
/// what an idle server does every autosave, and also for a write that
/// failed, which has already said so on stderr. The count is for the
/// caller to report; this no longer prints one itself, because the two
/// callers want it in different shapes (a line in the autosave log, and
/// a sentence back to whoever typed `/save`) and printing both meant
/// saying it twice.
fn save_chests(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
    if !chests.is_dirty() {
        return None;
    }
    match chests.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] chest save failed: {e}");
            None
        }
    }
}

/// Writes the player profiles, quietly doing nothing if none changed.
///
/// Synchronous, unlike the world save: the file is a few kilobytes even
/// with a hundred players in it, and the lock it takes is one every
/// join and part already takes.
///
/// Answers how many were written, on the same terms as `save_chests`.
fn save_profiles(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    // Everyone still connected is written as they stand, so a crash or
    // a `/stop` does not roll them back to wherever they last logged
    // out -- which for a long session is a lot of walking.
    for handle in ctx.registry.handles() {
        store_profile(ctx, &handle);
    }
    let mut profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
    match profiles.save(dir) {
        Ok(written) => written,
        Err(e) => {
            eprintln!("[players] save failed: {e}");
            None
        }
    }
}

/// Copies one connected player's state into their profile.
///
/// Called when they leave, and for everyone on every autosave.
pub(crate) fn store_profile(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let Some(uuid) = handle.uuid else {
        return;
    };
    let (inventory, position, yaw, pitch, health, slot) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (
            state.inventory.clone(),
            state.position,
            state.yaw,
            state.pitch,
            state.vitals.health(),
            state.selected_slot as u8,
        )
    };
    let mut profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
    profiles.store(uuid, inventory, position, yaw, pitch, health, slot);
}

async fn stats_loop(ctx: Arc<Context>) {
    let interval = Duration::from_secs_f32(ctx.settings.stats_interval_secs.max(1.0));
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await;

    let mut previous = Snapshotted::capture(&ctx);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = ctx.shutdown_requested() => break,
        }
        let current = Snapshotted::capture(&ctx);
        let seconds = interval.as_secs_f32();
        let world = ctx.world.stats();

        println!(
            "[stats] players={} (peak {}) | tps={:.1} (overruns {}) | in={:.0}/s chunks={:.0}/s \
             edits={:.0}/s snapshots={:.0}/s | cached_chunks={} edited_blocks={} | \
             anticheat_flags={} kicks={}",
            ctx.registry.len(),
            ctx.registry.peak_players(),
            (current.ticks - previous.ticks) as f32 / seconds,
            current.tick_overruns - previous.tick_overruns,
            (current.messages_in - previous.messages_in) as f32 / seconds,
            (current.chunks_sent - previous.chunks_sent) as f32 / seconds,
            (current.block_edits - previous.block_edits) as f32 / seconds,
            (current.snapshots_sent - previous.snapshots_sent) as f32 / seconds,
            world.cached_chunks,
            world.edited_blocks,
            current.anticheat_flags,
            current.kicks,
        );
        previous = current;
    }
}

struct Snapshotted {
    ticks: u64,
    tick_overruns: u64,
    messages_in: u64,
    chunks_sent: u64,
    block_edits: u64,
    snapshots_sent: u64,
    anticheat_flags: u64,
    kicks: u64,
}

impl Snapshotted {
    fn capture(ctx: &Context) -> Self {
        let m = &ctx.metrics;
        Self {
            ticks: m.ticks.load(Ordering::Relaxed),
            tick_overruns: m.tick_overruns.load(Ordering::Relaxed),
            messages_in: m.messages_in.load(Ordering::Relaxed),
            chunks_sent: m.chunks_sent.load(Ordering::Relaxed),
            block_edits: m.block_edits.load(Ordering::Relaxed),
            snapshots_sent: m.snapshots_sent.load(Ordering::Relaxed),
            anticheat_flags: m.anticheat_flags.load(Ordering::Relaxed),
            kicks: m.kicks.load(Ordering::Relaxed),
        }
    }
}

/// Tell everyone why they're being disconnected, then persist the world.
/// Order matters: players first (so the message goes out while sockets are
/// still alive), save second (so it happens even if a client hangs).
async fn shutdown(ctx: &Arc<Context>, world_dir: Option<PathBuf>) {
    let online = ctx.registry.len();
    if online > 0 {
        if ctx.options.logging {
            println!("[server] disconnecting {online} player(s)");
        }
        ctx.registry
            .broadcast(ServerMessage::Kick(DisconnectReason::ServerShutdown));
        for handle in ctx.registry.handles() {
            handle.request_kick(DisconnectReason::ServerShutdown);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    if let Some(dir) = world_dir {
        for line in save_everything(ctx, &dir) {
            if ctx.options.logging {
                println!("[world] {line}");
            }
        }
    }
    if ctx.options.logging {
        println!("[server] bye");
    }
}

/// Writes down everything a running server is holding in memory, and
/// says what it did.
///
/// **The three files are one operation, and the whole point of this
/// function is that there is nowhere left to write only some of them.**
/// A world is not `edits.bin`: it is the block overlay, the chests, and
/// the profiles that hold what every player is carrying and where they
/// are standing. Saving the overlay alone is worse than not saving at
/// all, because it produces a world whose *buildings* are current and
/// whose *contents* are as old as the last autosave -- so the chest a
/// player filled five minutes ago opens empty next to the wall they
/// built around it, and the pack they filled is gone with it.
///
/// That is exactly what `/save` used to do, while the help line promised
/// to "flush the world to disk". `shutdown` had all three calls in the
/// right order and `/save` had one of them, which is the shape a bug
/// like this always has: two callers that must agree, written twice.
/// Now there is one caller's worth of code and two callers.
///
/// Order matters and is the same order `shutdown` used. Profiles first,
/// because they are the cheapest to write and the most annoying to lose;
/// the world next; chests last, because a chest can change without any
/// block changing and the world's own dirty flag would not know it.
/// Each is independently guarded, so a failure to write one does not
/// take the other two down with it -- half a save is bad, but a save
/// abandoned at the first error is worse.
fn save_everything(ctx: &Arc<Context>, dir: &std::path::Path) -> Vec<String> {
    let mut said = Vec::new();
    let profiles = save_profiles(ctx, dir);
    match ctx.world.save(dir) {
        Ok(blocks) => {
            // One line naming all three, because the whole bug was an
            // operator being told "the world is saved" when two thirds
            // of it were not.
            let chests = save_chests(ctx, dir).unwrap_or(0);
            said.push(format!(
                "saved {blocks} block edit(s), {chests} chest(s) and {} profile(s) to {}",
                profiles.unwrap_or(0),
                dir.display()
            ));
        }
        Err(e) => {
            said.push(format!("world save failed: {e}"));
            save_chests(ctx, dir);
        }
    }
    said
}

/// Runs a parsed command's effect and returns the lines to show whoever
/// asked. Shared by the console and by chat commands, so `/list` can't
/// mean two different things depending on where it was typed.
pub fn run_command(
    ctx: &Arc<Context>,
    line: &str,
    permission: commands::Permission,
    caller: Option<PlayerId>,
) -> Vec<String> {
    use commands::{authorize, parse, Response};

    let command = match parse(line) {
        Ok(command) => command,
        Err(commands::ParseError::Unknown(name)) => {
            // Unknown commands are offered to the plugins before being
            // reported as a mistake -- that's how a plugin adds `/home`
            // or `/kit` without touching the server's own command table.
            let args: Vec<String> = line
                .trim_start_matches('/')
                .split_whitespace()
                .skip(1)
                .map(|s| s.to_string())
                .collect();
            let handled = fire_plugin_hook(
                ctx,
                "on_command",
                vec![
                    plugins::Value::Int(caller.unwrap_or(0) as i64),
                    plugins::Value::Text(name.clone()),
                    plugins::Value::List(args.into_iter().map(plugins::Value::Text).collect()),
                ],
                None,
            );
            // A plugin signals "I handled this" by returning false, the
            // same convention the cancellable hooks use.
            if !handled {
                return Vec::new();
            }
            return vec![commands::ParseError::Unknown(name).to_string()];
        }
        Err(e) => return vec![e.to_string()],
    };

    match authorize(command, permission, caller) {
        Response::Denied(reason) => vec![reason],

        Response::Reply(lines) => match lines.first().map(|s| s.as_str()) {
            Some("__LIST__") => {
                let handles = ctx.registry.handles();
                if handles.is_empty() {
                    return vec!["nobody is online".to_string()];
                }
                let mut out = vec![format!(
                    "{} player(s) online (peak {}):",
                    handles.len(),
                    ctx.registry.peak_players()
                )];
                for handle in handles {
                    let state = handle.player_state();
                    out.push(format!(
                        "  #{} {} at ({:.1}, {:.1}, {:.1})",
                        handle.id, handle.username, state.x, state.y, state.z
                    ));
                    // The identity their things are filed under, which
                    // is the number an operator needs when a name is
                    // ambiguous or has been changed.
                    if let Some(uuid) = handle.uuid {
                        out.push(format!("       {uuid}"));
                    }
                }
                out
            }

            // Everyone the server has ever seen, online or not: names,
            // identities, and where each of them left off.
            Some("__PROFILES__") => {
                let profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
                let all = profiles.all();
                if all.is_empty() {
                    return vec!["nobody has ever played here".to_string()];
                }
                let online: std::collections::HashSet<crate::logic::profiles::Uuid> = ctx
                    .registry
                    .handles()
                    .iter()
                    .filter_map(|h| h.uuid)
                    .collect();
                let mut out = vec![format!("{} known player(s):", all.len())];
                for profile in all {
                    out.push(format!(
                        "  {} {} -- {} join(s), left at ({:.0}, {:.0}, {:.0}), {} item(s){}",
                        profile.uuid,
                        profile.username,
                        profile.joins,
                        profile.position.0,
                        profile.position.1,
                        profile.position.2,
                        profile.inventory.total_items(),
                        if online.contains(&profile.uuid) { ", online" } else { "" },
                    ));
                }
                out
            }

            Some("__STATS__") => {
                let m = &ctx.metrics;
                let uptime = ctx.started.elapsed().as_secs();
                let world = ctx.world.stats();
                vec![
                    format!(
                        "uptime {}h{:02}m{:02}s | players {} (peak {})",
                        uptime / 3600,
                        (uptime % 3600) / 60,
                        uptime % 60,
                        ctx.registry.len(),
                        ctx.registry.peak_players()
                    ),
                    format!(
                        "ticks {} (overruns {}) | messages in {} | chunks sent {} | edits {}",
                        m.ticks.load(Ordering::Relaxed),
                        m.tick_overruns.load(Ordering::Relaxed),
                        m.messages_in.load(Ordering::Relaxed),
                        m.chunks_sent.load(Ordering::Relaxed),
                        m.block_edits.load(Ordering::Relaxed),
                    ),
                    format!(
                        "cached chunks {} | edited blocks {} | anti-cheat flags {} | kicks {}",
                        world.cached_chunks,
                        world.edited_blocks,
                        m.anticheat_flags.load(Ordering::Relaxed),
                        m.kicks.load(Ordering::Relaxed),
                    ),
                    {
                        let sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                        format!(
                            "falling blocks: {} landed, {} in the air, {} cells queued",
                            sim.dropped(),
                            sim.entity_count(),
                            sim.pending()
                        )
                    },
                    {
                        // A queue that is growing rather than draining is
                        // the one failure mode a cell mechanic has, and
                        // it is invisible unless something says so.
                        let mechanics = ctx.mechanics.lock().unwrap_or_else(|e| e.into_inner());
                        let queues: Vec<String> = mechanics
                            .pending()
                            .into_iter()
                            .map(|(name, pending)| format!("{name}: {pending} cells queued"))
                            .collect();
                        queues.join(" | ")
                    },
                ]
            }

            Some("__TIME__") => {
                let t = ctx.clock.time_of_day();
                let minutes = (t * 24.0 * 60.0) as u32;
                vec![format!(
                    "time of day {:.3} ({:02}:{:02}), full day = {:.0}s",
                    t,
                    minutes / 60,
                    minutes % 60,
                    ctx.clock.day_length_seconds()
                )]
            }

            Some("__WHERE__") => match caller.and_then(|id| ctx.registry.get(id)) {
                Some(handle) => {
                    let s = handle.player_state();
                    vec![format!("you are at ({:.2}, {:.2}, {:.2})", s.x, s.y, s.z)]
                }
                None => vec!["you are not online".to_string()],
            },

            _ => lines,
        },

        Response::Broadcast(text) => {
            if ctx.options.logging {
                println!("[server] {text}");
            }
            ctx.registry.broadcast(ServerMessage::Chat {
                from: None,
                username: "server".to_string(),
                text: text.clone(),
            });
            vec![format!("broadcast: {text}")]
        }

        Response::SetTime(t) => {
            ctx.clock.set_time_of_day(t);
            // Push it immediately rather than waiting for the periodic
            // sync, so `/time night` looks instant.
            let tick = ctx.clock.tick();
            ctx.registry.broadcast(ServerMessage::TimeSync {
                tick,
                time_of_day: t,
            });
            vec![format!("time of day set to {t:.3}")]
        }

        Response::TeleportSelf { x, y, z } => match caller.and_then(|id| ctx.registry.get(id)) {
            Some(handle) => {
                teleport(&handle, x, y, z, "teleported");
                vec![format!("teleported to ({x:.1}, {y:.1}, {z:.1})")]
            }
            None => vec!["you are not online".to_string()],
        },

        Response::TeleportSelfToSpawn => match caller.and_then(|id| ctx.registry.get(id)) {
            Some(handle) => {
                let (x, y, z) = ctx.world.spawn_point();
                teleport(&handle, x, y, z, "returned to spawn");
                vec!["teleported to spawn".to_string()]
            }
            None => vec!["you are not online".to_string()],
        },

        Response::Give { block, count } => {
            let Some(handle) = caller.and_then(|id| ctx.registry.get(id)) else {
                return vec!["you are not online".to_string()];
            };
            // Named the way the game names it, so `/give cobblestone`
            // works and `/give 11` is not a thing anyone has to know.
            let Some(&(id, name)) = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|&&(_, name)| name == block)
            else {
                return vec![format!("no block called '{block}'")];
            };
            // Bounded: this is the one command that makes something out
            // of nothing, and a typo with an extra zero should not be a
            // pack that takes a minute to sort out.
            let count = count.clamp(1, primitive_shared::inventory::MAX_STACK);
            let left = {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                let left = state.inventory.add(id, count);
                state.inventory_dirty = true;
                left
            };
            send_inventory(&handle);
            if left > 0 {
                vec![format!("gave {} {name}, {left} would not fit", count - left)]
            } else {
                vec![format!("gave {count} {name}")]
            }
        }

        Response::Kick { username, reason } => {
            let target = ctx
                .registry
                .handles()
                .into_iter()
                .find(|h| h.username.eq_ignore_ascii_case(&username));
            match target {
                Some(handle) => {
                    handle.request_kick(DisconnectReason::Other(reason.clone()));
                    ctx.metrics.kicks.fetch_add(1, Ordering::Relaxed);
                    vec![format!("kicked {} ({reason})", handle.username)]
                }
                None => vec![format!("no player called '{username}' is online")],
            }
        }

        Response::SetOperator { username, operator } => {
            use crate::logic::profiles::OperatorChange;

            // By profile and not by connection: promoting someone who is
            // not here is the ordinary case, and requiring them to be
            // online would mean an operator has to wait for the person
            // they are trying to give the keys to.
            let change = {
                let mut profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
                profiles.set_operator(&username, operator)
            };
            let name = match &change {
                OperatorChange::Changed { username } | OperatorChange::Unchanged { username } => {
                    username.clone()
                }
                OperatorChange::NoSuchPlayer => username.clone(),
            };
            match change {
                // Said rather than done silently: an operator who typed
                // `/op alice` and got nothing back would reasonably
                // assume it worked, and the difference matters when the
                // name was a typo for someone else's.
                OperatorChange::NoSuchPlayer => {
                    vec![format!("no player called '{name}' has ever played here")]
                }
                OperatorChange::Unchanged { .. } => vec![if operator {
                    format!("{name} is already an operator")
                } else {
                    format!("{name} is not an operator")
                }],
                OperatorChange::Changed { .. } => {
                    // Flushed now rather than at the next autosave. Every
                    // other profile field describes where a player was
                    // standing, and losing a few minutes of that to a
                    // crash is a nuisance; losing the fact that somebody
                    // is an operator is the kind of thing nobody notices
                    // until they need it.
                    if let Some(dir) = ctx.world_dir.clone() {
                        save_profiles(ctx, &dir);
                    }
                    // The other party hears about it too. Being quietly
                    // promoted and finding out by guessing is no way to
                    // learn you have the run of the server; being quietly
                    // demoted and finding out by being refused is worse.
                    if let Some(handle) = ctx
                        .registry
                        .handles()
                        .into_iter()
                        .find(|h| h.username.eq_ignore_ascii_case(&name))
                    {
                        handle.send(ServerMessage::Chat {
                            from: None,
                            username: "server".to_string(),
                            text: if operator {
                                "you are now an operator".to_string()
                            } else {
                                "you are no longer an operator".to_string()
                            },
                        });
                    }
                    vec![if operator {
                        format!("{name} is now an operator")
                    } else {
                        format!("{name} is no longer an operator")
                    }]
                }
            }
        }

        // Everything, not just the blocks. An operator types this
        // before doing something they might have to recover from, and
        // the one thing it must not do is leave the chests and the packs
        // behind -- see `save_everything`.
        Response::Save => match &ctx.world_dir {
            Some(dir) => save_everything(ctx, dir),
            None => vec!["persistence is disabled (world_dir is empty)".to_string()],
        },

        Response::Stop => {
            ctx.request_shutdown();
            vec!["shutting down".to_string()]
        }
    }
}

/// Tells a player what their health is now, and handles the case where
/// the answer is "none".
///
/// Health is only ever pushed from here, so the "did it change enough to
/// be worth a message" decision lives in exactly one place.
pub(crate) fn report_vitals(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    outcome: survival::Outcome,
) {
    match outcome {
        survival::Outcome::Unchanged => {}
        survival::Outcome::Changed => send_health(handle),
        survival::Outcome::Died { cause } => {
            send_health(handle);
            // A corpse is not standing at a chest. Closed here rather
            // than left for the next gesture to refuse, so the screen
            // does not sit open behind the death screen.
            {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.open_chest = None;
            }
            handle.send(ServerMessage::ChestClosed);
            // Before `Died`, and that ordering is the whole of what the
            // client sees: the pack is already in the world by the time
            // the death screen goes up, so the empty inventory that
            // arrives with it is the truth rather than a state the
            // client has to be corrected out of a moment later.
            drop_backpack(ctx, handle);
            handle.send(ServerMessage::Died {
                cause: cause.clone(),
            });
            if ctx.options.logging {
                println!("[survival] {} {cause}", handle.username);
            }
            // Everyone hears about it. Deaths are the most interesting
            // thing that happens on a small server.
            ctx.registry.broadcast(ServerMessage::Chat {
                from: None,
                username: "server".to_string(),
                text: format!("{} {cause}", handle.username),
            });
            fire_plugin_hook(
                ctx,
                "on_death",
                vec![
                    plugins::Value::Int(handle.id as i64),
                    plugins::Value::Text(cause),
                ],
                None,
            );
        }
    }
}

/// Pushes the inventory to its owner, if it has changed.
///
/// A snapshot rather than a delta: forty slots is under half a kilobyte,
/// and a snapshot cannot drift out of step with the server the way a
/// stream of deltas can after one dropped message.
pub(crate) fn send_inventory(handle: &Arc<players::PlayerHandle>) {
    let inventory = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.inventory_dirty {
            return;
        }
        state.inventory_dirty = false;
        state.inventory.clone()
    };
    handle.send(ServerMessage::InventoryState { inventory });
}

/// Pulls down anything that was standing on a cell which has just
/// stopped being able to hold it, and says what changed.
///
/// Worldgen refuses to *plant* grass on rock or a cactus on soil (see
/// `types::can_grow_on`), but nothing enforced that afterwards: mine the
/// dirt out from under a tuft and it stayed exactly where it was,
/// hanging in the air with daylight under it. The same went for a sand
/// column collapsing beneath a cactus, which is the commonest way it
/// happened, since sand is the one thing that falls.
///
/// Cascading matters as much as the first step. A cactus is a stack, and
/// removing the sand under it has to take the whole plant, not the
/// bottom segment -- so each cell that is emptied becomes the new ground
/// for the one above it, which is then asked the same question.
///
/// Returns the changes rather than broadcasting them, because the two
/// callers have different ideas about batching: a player's edit sends
/// one message per cell, the falling-block simulation groups a tick's
/// worth by chunk.
pub(crate) fn collapse_unsupported(
    ctx: &Arc<Context>,
    x: i32,
    y: i32,
    z: i32,
) -> Vec<BlockChange> {
    use primitive_shared::types::{BLOCK_AIR, CHUNK_SIZE_Y};

    let mut broken = Vec::new();
    let Some(ground) = ctx.world.cached_block(x, y, z) else {
        return broken; // the chunk is not loaded; nothing to decide
    };

    // The run of plants standing on this cell, read before anything is
    // changed. Stops at the first thing that is not one, which is
    // usually the very first cell.
    let mut column = Vec::new();
    for level in (y + 1)..CHUNK_SIZE_Y as i32 {
        match ctx.world.cached_block(x, level, z) {
            Some(block) if primitive_shared::types::needs_support(block) => {
                column.push(block)
            }
            _ => break,
        }
    }

    for (offset, block) in column
        .iter()
        .copied()
        .take(unsupported_run(ground, &column))
        .enumerate()
    {
        let level = y + 1 + offset as i32;
        if !ctx.world.set_block(x, level, z, BLOCK_AIR) {
            break;
        }
        spawn_block_drop(ctx, block, (x, level, z));
        broken.push(BlockChange {
            global_x: x,
            global_y: level,
            global_z: z,
            block_id: BLOCK_AIR,
        });
    }
    broken
}

/// How many of `column` -- the cells stacked directly above a support --
/// come down when that support becomes `ground`.
///
/// Pure, so the cascade can be checked without a world. The cascade is
/// the part worth checking: each cell that empties becomes the ground
/// for the one above it, which is what takes a whole cactus rather than
/// its bottom segment.
pub(crate) fn unsupported_run(
    ground: primitive_shared::types::BlockId,
    column: &[primitive_shared::types::BlockId],
) -> usize {
    use primitive_shared::types::{can_grow_on, BLOCK_AIR};

    let mut under = ground;
    let mut count = 0;
    for &block in column {
        if can_grow_on(block, under) {
            break;
        }
        count += 1;
        under = BLOCK_AIR;
    }
    count
}

/// Puts what a broken block yields on the ground where it was.
/// Tells every registered mechanic that a cell changed.
///
/// Beside the falling-sand notification rather than inside it: sand is
/// not a mechanic that happens to be registered, it is a field the
/// entity replication path reads directly, and folding the two together
/// would mean the tick loop could no longer ask it what is in the air.
/// One extra uncontended lock on an edit, and nothing at all when
/// nothing is registered.
pub(crate) fn notify_mechanics(ctx: &Arc<Context>, x: i32, y: i32, z: i32) {
    let mut mechanics = ctx.mechanics.lock().unwrap_or_else(|e| e.into_inner());
    if !mechanics.is_empty() {
        mechanics.on_block_changed(x, y, z);
    }
}

pub(crate) fn spawn_block_drop(ctx: &Arc<Context>, broken: u16, at: (i32, i32, i32)) {
    let Some(drop) = primitive_shared::types::block_drop(broken) else {
        return; // water and air leave nothing behind
    };
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
    items.spawn(
        drop,
        1,
        // Centre of the cell that was just emptied, so the drop pops
        // out of the hole rather than out of its floor.
        (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5),
        (0.0, 0.0, 0.0),
        None,
        Instant::now(),
    );
}

/// Throws part or all of a slot into the world in front of the player.
pub(crate) fn drop_from_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
    whole_stack: bool,
) {
    let thrown = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(block) = state.inventory.block_in(slot) else {
            return;
        };
        let want = if whole_stack {
            state.inventory.count_in(slot)
        } else {
            1
        };
        let count = state.inventory.take_from(slot, want);
        if count == 0 {
            return;
        }
        state.inventory_dirty = true;
        let (x, y, z) = state.position;
        (block, count, (x, y, z), state.yaw, state.pitch)
    };
    let (block, count, position, yaw, pitch) = thrown;

    // Where the player is actually looking, pitch included. Yaw alone
    // threw everything flat out in front regardless of whether you were
    // aiming at your own feet or at the sky, which is the one thing a
    // throw is expected to obey.
    //
    // Same basis as the client camera's `forward`, so the two agree.
    let look = (
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    );

    let thrown = {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn(
            block,
            count,
            // From eye height and a little way along the look direction,
            // so it leaves from where the player is aiming rather than
            // from inside them -- and, more to the point, so a throw at
            // a wall one step away does not spawn on the far side of it.
            (
                position.0 + look.0 * 0.4,
                position.1 + primitive_shared::geometry::EYE_HEIGHT + look.1 * 0.4,
                position.2 + look.2 * 0.4,
            ),
            look,
            Some(handle.id),
            Instant::now(),
        )
    };

    if !thrown {
        // The world is at its item cap. The stack is already out of the
        // pack, so it has to go back in: a throw that quietly deletes
        // what it threw is the same bug as a refused placement that
        // still spends the block.
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inventory.add(block, count);
        drop(state);
        handle.send(ServerMessage::Error(
            "too much is already lying around to drop that".to_string(),
        ));
    }
    send_inventory(handle);
}

fn send_health(handle: &Arc<players::PlayerHandle>) {
    let current = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.mark_reported();
        state.vitals.health()
    };
    handle.send(ServerMessage::Health {
        current,
        max: survival::MAX_HEALTH,
    });
}

// ---- chests ----
//
// A chest is the first thing in this world that two players can be
// inside at once, and everything below is shaped by that. The client is
// never told "your move succeeded"; it is told what the chest contains
// now, and so is everyone else standing at it. A snapshot cannot drift,
// and two people taking from the same slot at the same moment end up
// with one of them getting it and both of them seeing that.
//
// What the server checks, every single gesture: that the player has a
// chest open, that the cell still holds one, and that they are still
// near enough to reach it. A client that skips the open, walks away
// mid-gesture, or names a slot that does not exist is asking questions
// this already has answers to -- and the answer is silence.

/// How far a player may be from a chest and still use it.
///
/// The block-editing reach with the same slack a swing gets: the player
/// moves between the click and the message arriving, and a chest that
/// slams shut because you stepped back half a block is a chest nobody
/// keeps anything in.
fn chest_reach(ctx: &Arc<Context>) -> f32 {
    ctx.settings.anticheat.max_reach + 1.5
}

/// Whether a player standing at `from` may use the chest at `at`.
///
/// Both halves matter and they fail differently: out of range means the
/// player walked off, and no chest there means it was broken (possibly
/// by someone else) while the screen was open.
fn chest_in_use(ctx: &Arc<Context>, from: (f32, f32, f32), at: containers::ChestPos) -> bool {
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    let (dx, dy, dz) = (centre.0 - from.0, centre.1 - from.1, centre.2 - from.2);
    let distance_squared = dx * dx + dy * dy + dz * dz;
    if !distance_squared.is_finite() || distance_squared > chest_reach(ctx).powi(2) {
        return false;
    }
    ctx.world
        .cached_block(at.0, at.1, at.2)
        .is_some_and(primitive_shared::types::is_container)
}

/// A player asking to open the chest they are looking at.
pub(crate) fn open_chest(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: containers::ChestPos,
) {
    let position = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        state.position
    };
    if !chest_in_use(ctx, position, at) {
        return;
    }
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.open_chest = Some(at);
    }
    send_chest_state(ctx, handle, at);
}

/// Sends one player what is in a chest.
fn send_chest_state(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: containers::ChestPos,
) {
    let inventory = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at)
    };
    handle.send(ServerMessage::ChestState {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        inventory,
    });
}

/// Sends it to *everyone* standing at that chest.
///
/// The reason two players can share one: whoever changed it is told the
/// same way everyone else is, so there is one code path and no chance of
/// the mover seeing something the others do not.
fn broadcast_chest_state(ctx: &Arc<Context>, at: containers::ChestPos) {
    let inventory = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at)
    };
    for handle in ctx.registry.handles() {
        let watching = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.open_chest == Some(at)
        };
        if watching {
            handle.send(ServerMessage::ChestState {
                global_x: at.0,
                global_y: at.1,
                global_z: at.2,
                inventory: inventory.clone(),
            });
        }
    }
}

/// Shuts the screen of everyone who has this chest open.
///
/// Called when the block goes. Leaving them looking at a chest that no
/// longer exists is how a player puts something into nothing.
fn close_chest_for_everyone(ctx: &Arc<Context>, at: containers::ChestPos) {
    for handle in ctx.registry.handles() {
        let watching = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.open_chest == Some(at) {
                state.open_chest = None;
                true
            } else {
                false
            }
        };
        if watching {
            handle.send(ServerMessage::ChestClosed);
        }
    }
}

/// One move between a player's pack and the chest they have open.
///
/// `half` is the right-click. `from` and `to` may name either side, so
/// this is also how things are rearranged inside a chest or inside a
/// pack while the screen is up.
pub(crate) fn chest_move(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    from: (Side, u8),
    to: (Side, u8),
    half: bool,
) {
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let (from_side, from_slot) = (from.0, from.1 as usize);
    let (to_side, to_slot) = (to.0, to.1 as usize);
    if from_side == to_side && from_slot == to_slot {
        return;
    }

    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| {
            use primitive_shared::inventory::{move_between, split_between};
            let pack = &mut state.inventory;
            match (from_side, to_side) {
                // Within one inventory: the pack's own rules, which
                // already know how to merge, swap and halve.
                (Side::Pack, Side::Pack) => {
                    if half {
                        pack.split_into(from_slot, to_slot)
                    } else {
                        pack.move_or_merge(from_slot, to_slot)
                    }
                }
                (Side::Chest, Side::Chest) => {
                    if half {
                        chest.split_into(from_slot, to_slot)
                    } else {
                        chest.move_or_merge(from_slot, to_slot)
                    }
                }
                (Side::Pack, Side::Chest) => {
                    if half {
                        split_between(pack, from_slot, chest, to_slot)
                    } else {
                        move_between(pack, from_slot, chest, to_slot)
                    }
                }
                (Side::Chest, Side::Pack) => {
                    if half {
                        split_between(chest, from_slot, pack, to_slot)
                    } else {
                        move_between(chest, from_slot, pack, to_slot)
                    }
                }
            }
        })
    };

    if changed {
        finish_chest_gesture(ctx, handle, at);
    }
}

/// The shift-click: a whole slot to the other side.
pub(crate) fn chest_quick_move(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    side: Side,
    slot: u8,
) {
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let slot = slot as usize;
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| {
            use primitive_shared::inventory::quick_move_between;
            match side {
                Side::Pack => quick_move_between(&mut state.inventory, slot, chest),
                Side::Chest => quick_move_between(chest, slot, &mut state.inventory),
            }
        })
    };
    if changed {
        finish_chest_gesture(ctx, handle, at);
    }
}

/// Everything that fits, in one gesture.
///
/// The loop is here rather than on the client for the reason every
/// other chest gesture is: the client would have to send forty
/// messages, the rate limit exists to refuse exactly that, and a
/// transfer that is half applied is worse than one that is refused. It
/// is also the only version that can be *atomic* -- both sides see one
/// answer, with no window in which the pack and the chest disagree.
///
/// What does not fit stays where it is. A player who asks to store
/// everything and has nine slots' worth of room gets nine slots' worth
/// stored, which is what "store what fits" means and what the button
/// says.
pub(crate) fn chest_bulk_move(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    to_chest: bool,
) {
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| {
            use primitive_shared::inventory::{quick_move_between, SLOTS};
            let mut moved = false;
            for slot in 0..SLOTS {
                moved |= if to_chest {
                    quick_move_between(&mut state.inventory, slot, chest)
                } else {
                    quick_move_between(chest, slot, &mut state.inventory)
                };
            }
            moved
        })
    };
    if changed {
        finish_chest_gesture(ctx, handle, at);
    }
}

/// The chest this player may act on right now, if any.
fn usable_chest(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
) -> Option<containers::ChestPos> {
    let (at, position, dead) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.open_chest?, state.position, state.vitals.is_dead())
    };
    if dead || !chest_in_use(ctx, position, at) {
        // Walked off, died, or someone broke it: shut the screen rather
        // than silently ignoring everything they do at it.
        close_chest_for_everyone(ctx, at);
        return None;
    }
    Some(at)
}

/// What every successful gesture ends with: both sides told what they
/// hold now.
fn finish_chest_gesture(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: containers::ChestPos,
) {
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inventory_dirty = true;
    }
    send_inventory(handle);
    broadcast_chest_state(ctx, at);
}

/// Empties a chest into the world, because the block holding it is gone.
///
/// One dropped stack per slot rather than one per block: forty items
/// popping out of a broken chest is fine, five thousand is a server
/// falling over. The stacks land in the cell the chest was in, which is
/// now air, so they are reachable.
pub(crate) fn spill_chest(ctx: &Arc<Context>, at: containers::ChestPos) {
    close_chest_for_everyone(ctx, at);
    let Some(contents) = ({
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.take(at)
    }) else {
        return; // an empty chest has nothing stored and nothing to spill
    };

    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    spill_inventory(ctx, &contents, centre);
}

/// Tips a whole inventory out into the world at a point.
///
/// One dropped stack per slot rather than one per block: forty items
/// popping out of a broken chest is fine, five thousand is a server
/// falling over.
///
/// If the world is at its item cap a stack is lost, which is the same
/// answer every other drop gets -- and the only alternative is refusing
/// to break the block, which leaves the player with a chest they cannot
/// get rid of.
fn spill_inventory(
    ctx: &Arc<Context>,
    contents: &primitive_shared::inventory::Inventory,
    at: (f32, f32, f32),
) {
    let now = Instant::now();
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
    for stack in contents.slots().iter().flatten() {
        items.spawn(stack.block, stack.count, at, (0.0, 0.0, 0.0), None, now);
    }
}

/// Which cell a dead player's pack goes into, given a way to look at the
/// world.
///
/// Pure, and takes the world as a closure, because the interesting cases
/// are all *where somebody died* rather than what the server did next:
/// inside a wall, in water, at the bottom of a hole, on the floor of the
/// world. Each of those is a line here and would be a fixture apiece
/// against a real world.
///
/// `look` answering `None` means "not loaded", and that is a refusal
/// rather than "empty": a block written into a chunk nobody has is a
/// block the world will regenerate over.
///
/// Two passes, and their order is the decision. A bag standing on the
/// ground is a bag you can see from where you respawned; a bag hanging in
/// the air over the ravine that killed you is a second death. So a cell
/// with a floor under it wins over a nearer one without, and only when
/// nothing in reach has a floor does the first free cell take it.
pub(crate) fn backpack_cell(
    at: (f32, f32, f32),
    look: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
) -> Option<(i32, i32, i32)> {
    use primitive_shared::types::{has_full_top, layer_placement, BLOCK_BACKPACK, CHUNK_SIZE_Y};

    if !at.0.is_finite() || !at.1.is_finite() || !at.2.is_finite() {
        return None;
    }
    let (x, z) = (at.0.floor() as i32, at.2.floor() as i32);
    let feet = at.1.floor() as i32;

    // The cell they stood in first, then upwards, then the two below.
    // Up before down because the commonest way to die inside a block is
    // to be crushed or drowned in one, and the commonest way to die over
    // a hole is to fall into it -- in both cases the bag wants to be
    // where the player can walk back to, not where they cannot.
    let free = |y: i32| -> bool {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            return false;
        }
        look(x, y, z).is_some_and(|here| layer_placement(here, BLOCK_BACKPACK).is_some())
    };
    for want_floor in [true, false] {
        for offset in [0, 1, 2, 3, -1, -2] {
            let y = feet + offset;
            if !free(y) {
                continue;
            }
            if want_floor && !look(x, y - 1, z).is_some_and(has_full_top) {
                continue;
            }
            return Some((x, y, z));
        }
    }
    None
}

/// Chooses a cell, claims it, and puts `contents` in it -- as one
/// operation, with the chest map held for all three.
///
/// Returns where the bag went and whatever would not fit in it.
///
/// ## Why the lock spans the whole of it
///
/// **This was three steps with nothing joining them, and the gap
/// between them destroyed things.** The old shape was: read the world to
/// pick a free cell, write `BLOCK_BACKPACK` into it, take the chest lock,
/// and assign. `World::set_block` answers `true` for any cell in range --
/// it is a write, not a compare-and-swap -- so nothing anywhere in that
/// sequence noticed that somebody else had claimed the cell in between.
///
/// Two players dying in the same cell on the same tick is not exotic: it
/// is a cave-in, a lava flow, a fall down the same shaft, a fight. Both
/// read the cell as free, both wrote the block, and then the second
/// `*inventory = contents` overwrote the first player's pack with the
/// second's. Everything the first player owned stopped existing, with no
/// error anywhere and nothing in the log.
///
/// Holding `chests` across the choice is what closes it. The lock is not
/// protecting the chest map here so much as being *used* as the one lock
/// that serialises the whole gesture -- the world has no lock of its own
/// that spans a read and a write, and inventing a second one would mean
/// two locks taken in an order every future caller has to get right. The
/// hold is a few microseconds of map lookups; the alternative was a
/// compare-and-swap on the world, which is a larger change to the one
/// structure every other system on the server reads through.
///
/// ## Why it merges rather than assigns
///
/// Belt and braces, and cheap. With the lock held the second player will
/// find the cell taken and go elsewhere, so the merge should never have
/// anything to do -- but "should never" is exactly what the old code
/// assumed, and `*inventory = contents` is a statement that *deletes*
/// when the assumption is wrong. `add` cannot delete anything: what does
/// not fit comes back as leftovers and is dropped on the ground by the
/// caller, which is the same fallback a bag that could not be placed at
/// all already used.
fn stash_backpack(
    chests: &mut containers::Chests,
    contents: &primitive_shared::inventory::Inventory,
    position: (f32, f32, f32),
    look: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
    place: impl FnOnce(i32, i32, i32) -> bool,
) -> Option<(containers::ChestPos, primitive_shared::inventory::Inventory)> {
    use primitive_shared::inventory::Inventory;

    let at = backpack_cell(position, &look)?;
    if !place(at.0, at.1, at.2) {
        return None;
    }
    let mut leftovers = Inventory::new();
    chests.edit(at, |inventory| {
        for stack in contents.slots().iter().flatten() {
            let left = inventory.add(stack.block, stack.count);
            if left > 0 {
                leftovers.add(stack.block, left);
            }
        }
    });
    Some((at, leftovers))
}

/// Puts what a dead player was carrying into a block where they fell.
///
/// ## Why a block and not a heap of drops
///
/// Dropping forty stacks on the ground is the obvious answer and it is
/// the wrong one twice over. Items despawn, so a player who dies far
/// from spawn and has to walk back loses everything by arriving late;
/// and forty entities in one cell is the worst case the item system has,
/// produced by the event most likely to happen to several players at
/// once. A block holds its contents forever, costs one cell, and is
/// already the thing this server knows how to store, save, open, share
/// between two players and spill when broken -- see `containers`. The
/// backpack is a chest with a different picture on it and no recipe.
///
/// ## Why the pack is emptied even if the block cannot be placed
///
/// The one outcome nothing here may produce is a player who respawns
/// carrying their things *and* a bag in the world holding them, which is
/// how a death doubles somebody's stock. So the inventory is taken
/// first, and every path after that is about where it ends up; the worst
/// case is the heap of drops this exists to avoid, not a duplication.
pub(crate) fn drop_backpack(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::BLOCK_BACKPACK;

    // An empty pack leaves nothing behind. Checked before anything is
    // moved: a bag with nothing in it is a block the player has to walk
    // back to, break, and find empty, which is worse than no bag at all.
    let (contents, position) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.inventory.is_empty() {
            return;
        }
        let taken = std::mem::replace(&mut state.inventory, Inventory::new());
        state.inventory_dirty = true;
        (taken, state.position)
    };
    send_inventory(handle);

    // Loaded before anything is decided, for the reason `place_block`
    // gives: reads outside the chunk pump see the cache and nothing
    // else, so an uncached column answers `None` to every question and
    // an edit written into it is an edit the generator will paint over.
    // A player can perfectly well die in a chunk nobody is standing in.
    let (chunk_pos, _, _) = ChunkPos::from_global(position.0.floor() as i32, position.2.floor() as i32);
    if ctx.world.cached(chunk_pos).is_none() {
        let chunk = ctx.world.generate(chunk_pos);
        ctx.world.insert(chunk);
    }

    // Choosing the cell, claiming it and filling it, all under the one
    // lock. See `stash_backpack` for why that is the fix and not an
    // incidental tidy-up.
    let stashed = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        stash_backpack(
            &mut chests,
            &contents,
            position,
            |x, y, z| ctx.world.cached_block(x, y, z),
            |x, y, z| ctx.world.set_block(x, y, z, BLOCK_BACKPACK),
        )
    };
    let Some((at, leftovers)) = stashed else {
        // Nowhere to put it: buried to the horizon, or at the very top
        // of the world. The heap of drops is the fallback rather than
        // the design, and it is still better than silence.
        spill_inventory(ctx, &contents, position);
        return;
    };
    // Whatever would not fit, which needs a bag already holding
    // something *and* a second bag landing in it -- see `stash_backpack`.
    // Drops rather than deletion, on the rule this whole function is
    // built on: the one outcome nothing here may produce is things
    // ceasing to exist.
    if !leftovers.is_empty() {
        spill_inventory(ctx, &leftovers, position);
    }

    let change = BlockChange {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        block_id: BLOCK_BACKPACK,
    };
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(ServerMessage::BlockUpdate(change));
    }
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(at.0, at.1, at.2);
    }
    notify_mechanics(ctx, at.0, at.1, at.2);

    if ctx.options.logging {
        println!(
            "[survival] {}'s pack is at ({}, {}, {})",
            handle.username, at.0, at.1, at.2
        );
    }
}

/// One player swinging at another.
///
/// Everything the client said is the *target*, and even that is only a
/// name: the distance is measured between the server's own copies of the
/// two positions, the damage figure is the server's, and the rate is
/// limited here rather than by the client's restraint. A client that
/// strips out its own cooldown, aims at someone across the map, or
/// swings while dead is asking a question this already has the answer
/// to.
///
/// Silence is the response to every refusal. A hit that did not land
/// looks exactly like a miss from the attacker's side, and telling them
/// which of the several reasons applied is telling a cheat client what
/// to fix.
pub(crate) fn melee_attack(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    target: PlayerId,
) {
    use primitive_shared::combat;

    if target == handle.id {
        return; // nobody punches themselves
    }
    let Some(victim) = ctx.registry.get(target) else {
        return; // they left between the swing and it arriving
    };

    // The attacker's half: alive, and not swinging faster than a person.
    let from = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let now = std::time::Instant::now();
        let ready = state.last_swing.is_none_or(|last| {
            now.saturating_duration_since(last).as_secs_f32()
                >= combat::MELEE_COOLDOWN_SECS - combat::COOLDOWN_SLACK_SECS
        });
        if !ready {
            return;
        }
        state.last_swing = Some(now);
        state.position
    };

    // The victim's half. Note the lock is taken *after* the attacker's
    // is released: two players punching each other at the same instant
    // on two connection tasks would otherwise be a deadlock, and the
    // only thing needed from the first lock is a position.
    let outcome = {
        let mut state = victim.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() || !combat::within_reach(from, state.position) {
            return;
        }
        state.vitals.hurt(
            combat::MELEE_DAMAGE,
            &format!("was struck down by {}", handle.username),
        )
    };
    report_vitals(ctx, &victim, outcome);
}

/// Puts a dead player back in the world at full health.
pub(crate) fn respawn_player(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let spawn = ctx.world.spawn_point();
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.respawn();
        state.position = spawn;
        // Whatever they were rummaging in is a long way away now.
        state.open_chest = None;
        // The anti-cheat has to be told, or the jump from the death site
        // to the spawn point looks exactly like the teleport hack it
        // exists to catch.
        state.anticheat.reset_to(spawn);
    }
    handle.send(ServerMessage::Respawned {
        x: spawn.0,
        y: spawn.1,
        z: spawn.2,
    });
    send_health(handle);
}

/// Server-authoritative reposition. Reuses `PositionCorrection`, which
/// the client already obeys unconditionally, so no new message type and
/// no new client code path is needed for teleports.
fn teleport(handle: &Arc<players::PlayerHandle>, x: f32, y: f32, z: f32, why: &str) {
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.position = (x, y, z);
        // Tell the anti-cheat too, or the jump it just authorised looks
        // exactly like the teleport hack it exists to catch.
        state.anticheat.reset_to((x, y, z));
        // And the fall tracker, or being moved downwards -- by `/tp`, by
        // a plugin, or by a rubber-band correction -- arrives as fall
        // damage for a fall that never happened.
        state.vitals.clear_fall();
    }
    handle.send(ServerMessage::PositionCorrection {
        x,
        y,
        z,
        reason: why.to_string(),
    });
}

/// Reads operator commands from the server's own stdin.
async fn console_loop(ctx: Arc<Context>) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    println!("[server] console ready -- type 'help' for commands");
    loop {
        let line = tokio::select! {
            line = lines.next_line() => line,
            _ = ctx.shutdown_requested() => break,
        };
        match line {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                for reply in run_command(&ctx, &line, commands::Permission::Operator, None) {
                    println!("[console] {reply}");
                }
            }
            // stdin closed (running detached or piped from /dev/null):
            // that's normal, just stop reading. The server keeps serving.
            Ok(None) => break,
            Err(e) => {
                eprintln!("[console] read error: {e}");
                break;
            }
        }
    }
}

/// Builds the read-only world snapshot plugins see during a hook.
///
/// Assembled up front rather than letting scripts query live state:
/// a hook runs while the caller may already hold world or registry
/// locks, so a script reaching back into them could deadlock the tick
/// loop. `blocks` carries only the cells relevant to the event.
fn plugin_view(ctx: &Arc<Context>, blocks: Vec<(i32, i32, i32)>) -> plugins::HostView {
    let mut view = plugins::HostView {
        time_of_day: ctx.clock.time_of_day(),
        seed: ctx.world.seed(),
        tick: ctx.clock.tick(),
        players: Vec::new(),
        blocks: std::collections::HashMap::new(),
    };
    for handle in ctx.registry.handles() {
        let state = handle.player_state();
        view.players.push((
            handle.id,
            handle.username.clone(),
            (state.x, state.y, state.z),
        ));
    }
    for (x, y, z) in blocks {
        if let Some(id) = ctx.world.cached_block(x, y, z) {
            view.blocks.insert((x, y, z), id);
        }
    }
    view
}

/// Runs a hook and applies whatever the plugins asked for.
///
/// Returns false if any plugin vetoed the action (only meaningful for
/// the cancellable hooks).
fn fire_plugin_hook(
    ctx: &Arc<Context>,
    hook: &str,
    args: Vec<plugins::Value>,
    blocks: Option<Vec<(i32, i32, i32)>>,
) -> bool {
    let view = plugin_view(ctx, blocks.unwrap_or_default());
    let (allowed, effects) = {
        let mut host = ctx.plugins.lock().unwrap_or_else(|e| e.into_inner());
        if host.active_count() == 0 {
            return true;
        }
        host.fire(hook, args, &view)
    };

    for effect in effects {
        match effect {
            plugins::Effect::Broadcast(text) => {
                if ctx.options.logging {
                    println!("[plugins] {text}");
                }
                ctx.registry.broadcast(ServerMessage::Chat {
                    from: None,
                    username: "server".to_string(),
                    text,
                });
            }
            plugins::Effect::Tell { player, text } => {
                if let Some(handle) = ctx.registry.get(player) {
                    handle.send(ServerMessage::Chat {
                        from: None,
                        username: "server".to_string(),
                        text,
                    });
                }
            }
            plugins::Effect::Log { plugin, text } => {
                println!("[plugin:{plugin}] {text}");
            }
            plugins::Effect::SetBlock { x, y, z, block } => {
                if ctx.world.set_block(x, y, z, block) {
                    let (chunk_pos, _, _) = ChunkPos::from_global(x, z);
                    let change = BlockChange {
                        global_x: x,
                        global_y: y,
                        global_z: z,
                        block_id: block,
                    };
                    for subscriber in ctx.registry.subscribers(chunk_pos) {
                        subscriber.send(ServerMessage::BlockUpdate(change));
                    }
                    let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                    sim.on_block_changed(x, y, z);
                    notify_mechanics(ctx, x, y, z);
                }
            }
            plugins::Effect::Kick { player, reason } => {
                if let Some(handle) = ctx.registry.get(player) {
                    handle.request_kick(DisconnectReason::Other(reason));
                    ctx.metrics.kicks.fetch_add(1, Ordering::Relaxed);
                }
            }
            plugins::Effect::Teleport { player, x, y, z } => {
                if let Some(handle) = ctx.registry.get(player) {
                    teleport(&handle, x, y, z, "moved by a plugin");
                }
            }

            plugins::Effect::SpawnFallingBlock { x, y, z, block } => {
                // Put the block in the world and poke the simulation at
                // it. Going through the same path a player edit takes
                // means it falls, lands and replicates by exactly the
                // rules everything else follows.
                if ctx.world.set_block(x, y, z, block) {
                    let (chunk_pos, _, _) = ChunkPos::from_global(x, z);
                    let change = BlockChange {
                        global_x: x,
                        global_y: y,
                        global_z: z,
                        block_id: block,
                    };
                    for subscriber in ctx.registry.subscribers(chunk_pos) {
                        subscriber.send(ServerMessage::BlockUpdate(change));
                    }
                }
                let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                sim.on_block_changed(x, y, z);
                notify_mechanics(ctx, x, y, z);
            }

            plugins::Effect::SetTime(t) => {
                ctx.clock.set_time_of_day(t);
                let tick = ctx.clock.tick();
                ctx.registry.broadcast(ServerMessage::TimeSync {
                    tick,
                    time_of_day: t,
                });
            }
        }
    }

    allowed
}

/// What comes down when the ground under a plant goes.
///
/// Worldgen refuses to plant grass on rock, but until this existed
/// nothing enforced the rule afterwards: mine the dirt out from under a
/// tuft and it hung in the air with daylight beneath it.
#[cfg(test)]
mod support_tests {
    use super::unsupported_run;
    use primitive_shared::types::{
        BLOCK_AIR, BLOCK_CACTUS, BLOCK_DIRT, BLOCK_GRASS, BLOCK_SAND, BLOCK_STICK, BLOCK_STONE,
        BLOCK_TALL_GRASS,
    };

    #[test]
    fn a_tuft_falls_when_its_soil_does() {
        assert_eq!(unsupported_run(BLOCK_AIR, &[BLOCK_TALL_GRASS]), 1);
        assert_eq!(unsupported_run(BLOCK_STONE, &[BLOCK_TALL_GRASS]), 1);
    }

    #[test]
    fn a_tuft_on_soil_stays_where_it_is() {
        assert_eq!(unsupported_run(BLOCK_GRASS, &[BLOCK_TALL_GRASS]), 0);
        assert_eq!(unsupported_run(BLOCK_DIRT, &[BLOCK_TALL_GRASS]), 0);
    }

    #[test]
    fn a_whole_cactus_comes_down_rather_than_its_bottom_segment() {
        // The cascade: each emptied cell becomes the ground for the one
        // above it. Without that, digging the sand out from under a
        // cactus leaves three quarters of it floating.
        let cactus = [BLOCK_CACTUS; 4];
        assert_eq!(unsupported_run(BLOCK_AIR, &cactus), 4);
        // ...and none of it if the sand is still there.
        assert_eq!(unsupported_run(BLOCK_SAND, &cactus), 0);
    }

    #[test]
    fn the_collapse_stops_at_the_first_thing_that_can_hold_itself_up() {
        // A stick lies on anything solid, so a stick sitting on top of a
        // cactus keeps standing until the cactus under it goes -- and
        // then it goes too, because air holds nothing.
        assert_eq!(unsupported_run(BLOCK_SAND, &[BLOCK_CACTUS, BLOCK_STICK]), 0);
        assert_eq!(unsupported_run(BLOCK_AIR, &[BLOCK_CACTUS, BLOCK_STICK]), 2);
    }

    #[test]
    fn nothing_falls_out_of_an_empty_column() {
        assert_eq!(unsupported_run(BLOCK_AIR, &[]), 0);
    }
}

/// Where a dead player's things end up.
///
/// The block goes into a cell chosen from the corpse's position, and
/// every interesting case is a *place to die* rather than anything the
/// server does afterwards -- which is why the chooser takes the world as
/// a closure and this needs no fixture.
#[cfg(test)]
mod backpack_tests {
    use super::backpack_cell;
    use primitive_shared::types::{
        BlockId, BLOCK_AIR, BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_WATER, CHUNK_SIZE_Y,
    };

    /// A world that is solid stone below `ground` and air above it.
    fn ground_at(ground: i32) -> impl Fn(i32, i32, i32) -> Option<BlockId> {
        move |_x, y, _z| {
            if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                return None;
            }
            Some(if y < ground { BLOCK_STONE } else { BLOCK_AIR })
        }
    }

    #[test]
    fn dying_on_your_feet_leaves_the_bag_where_you_stood() {
        let at = backpack_cell((4.3, 10.0, 7.8), ground_at(10));
        assert_eq!(at, Some((4, 10, 7)));
    }

    #[test]
    fn a_negative_coordinate_floors_rather_than_truncates() {
        // -7.8 is in cell -8, not -7. Truncation would put the bag a
        // block away from the body, on the wrong side of a chunk
        // boundary as often as not.
        let at = backpack_cell((-0.2, 10.0, -7.8), ground_at(10));
        assert_eq!(at, Some((-1, 10, -8)));
    }

    #[test]
    fn dying_inside_a_block_puts_the_bag_above_it() {
        // Crushed or suffocated: the cell the body is in is solid, so
        // the bag goes up rather than nowhere.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 12 => BLOCK_STONE,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(backpack_cell((0.5, 10.0, 0.5), world), Some((0, 12, 0)));
    }

    #[test]
    fn drowning_leaves_it_in_the_water_rather_than_on_the_shore() {
        // Water is something you build into, so it is somewhere a bag
        // can go -- and a bag that dodged sideways out of the lake would
        // be a bag the player cannot find.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 5 => BLOCK_STONE,
                y if y < 20 => BLOCK_WATER,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(backpack_cell((0.5, 9.0, 0.5), world), Some((0, 9, 0)));
    }

    #[test]
    fn a_bag_prefers_a_floor_under_it_to_the_cell_you_died_in() {
        // Falling: the server's last position for a body in mid-air is
        // in mid-air. A bag left there hangs over whatever killed them.
        let at = backpack_cell((0.5, 12.0, 0.5), ground_at(11));
        assert_eq!(at, Some((0, 11, 0)), "the bag was left hanging");
    }

    #[test]
    fn something_growing_in_the_cell_is_no_obstacle() {
        // A tuft of grass is walked through, built through, and is not a
        // reason to put a bag somewhere else.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 10 => BLOCK_STONE,
                10 => BLOCK_TALL_GRASS,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(backpack_cell((0.5, 10.0, 0.5), world), Some((0, 10, 0)));
    }

    #[test]
    fn a_cell_nobody_has_loaded_is_a_refusal_rather_than_an_empty_one() {
        // `None` means "not loaded". Treating it as air writes a block
        // into a chunk that will be regenerated over the top of it,
        // which loses everything in the bag.
        assert_eq!(backpack_cell((0.5, 10.0, 0.5), |_, _, _| None), None);
    }

    #[test]
    fn there_is_no_cell_at_all_inside_solid_rock() {
        // The caller's fallback: the things are spilled as drops rather
        // than left in a block that could not be placed.
        assert_eq!(backpack_cell((0.5, 30.0, 0.5), |_, _, _| Some(BLOCK_STONE)), None);
    }

    #[test]
    fn the_roof_and_the_floor_of_the_world_are_both_respected() {
        // Off the top: the cells above do not exist, so the search has
        // to come back down rather than run past the array.
        let ceiling = CHUNK_SIZE_Y as i32 - 1;
        let world = move |_x: i32, y: i32, _z: i32| {
            if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                return None;
            }
            Some(if y == 0 { BLOCK_STONE } else { BLOCK_AIR })
        };
        assert_eq!(backpack_cell((0.5, ceiling as f32, 0.5), world), Some((0, ceiling, 0)));
        // ...and a body on the floor of the world has a floor under it.
        assert_eq!(backpack_cell((0.5, 1.0, 0.5), world), Some((0, 1, 0)));
    }

    #[test]
    fn a_position_that_is_not_a_number_is_refused() {
        // Positions come off a socket, and `f32::NAN as i32` is zero --
        // which would quietly bury somebody's pack at the origin.
        assert_eq!(backpack_cell((f32::NAN, 10.0, 0.0), ground_at(10)), None);
        assert_eq!(backpack_cell((0.0, f32::INFINITY, 0.0), ground_at(10)), None);
    }

    // ---- and what happens when two of them arrive at once ----

    use super::stash_backpack;
    use crate::logic::containers::Chests;
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::{BLOCK_BACKPACK, BLOCK_DIRT, BLOCK_STICK};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A world of stone with a floor at `ground`, that remembers what is
    /// written into it -- which is the whole point: the second caller
    /// has to be able to see the first caller's block.
    struct FakeWorld {
        cells: RefCell<HashMap<(i32, i32, i32), BlockId>>,
        ground: i32,
    }

    impl FakeWorld {
        fn new(ground: i32) -> Self {
            Self {
                cells: RefCell::new(HashMap::new()),
                ground,
            }
        }

        fn look(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
            if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                return None;
            }
            if let Some(&block) = self.cells.borrow().get(&(x, y, z)) {
                return Some(block);
            }
            Some(if y < self.ground { BLOCK_STONE } else { BLOCK_AIR })
        }

        fn place(&self, x: i32, y: i32, z: i32, block: BlockId) -> bool {
            self.cells.borrow_mut().insert((x, y, z), block);
            true
        }

        /// One player's death, start to finish, exactly as
        /// `drop_backpack` runs it.
        fn die_with(&self, chests: &mut Chests, at: (f32, f32, f32), carried: &Inventory) {
            let stashed = stash_backpack(
                chests,
                carried,
                at,
                |x, y, z| self.look(x, y, z),
                |x, y, z| self.place(x, y, z, BLOCK_BACKPACK),
            );
            assert!(stashed.is_some(), "there was nowhere to put a bag at all");
            let (_, leftovers) = stashed.unwrap();
            assert!(
                leftovers.is_empty(),
                "a bag with room in it turned somebody's things away"
            );
        }
    }

    fn carrying(block: BlockId, count: u32) -> Inventory {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.add(block, count), 0);
        inventory
    }

    #[test]
    fn the_second_of_two_deaths_in_one_cell_goes_somewhere_else() {
        // The property the lock buys, stated as behaviour: the second
        // caller looks at a world that already has the first caller's
        // bag in it, so it picks a different cell. It can only do that
        // because it cannot run until the first one has finished -- the
        // signature of `stash_backpack` takes `&mut Chests`, so there is
        // no way to reach it without holding the one lock that
        // serialises the whole gesture.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();

        world.die_with(&mut chests, (0.5, 10.0, 0.5), &carrying(BLOCK_DIRT, 40));
        world.die_with(&mut chests, (0.5, 10.0, 0.5), &carrying(BLOCK_STICK, 7));

        assert_eq!(chests.len(), 2, "the two bags shared one cell");
        assert_eq!(chests.contents((0, 10, 0)).count(BLOCK_DIRT), 40);
    }

    #[test]
    fn two_bags_forced_into_one_cell_keep_both_sets_of_things() {
        // **The race, staged.** Two players dying in the same cell on
        // the same tick -- a cave-in, a shaft, a fight -- both read the
        // cell as free before either had written to it, and the second
        // one's `*inventory = contents` replaced the first one's
        // outright. Everything the first player owned stopped existing,
        // silently and with nothing in the log.
        //
        // The second caller here is handed the world *as it was before
        // the first one wrote*, which is precisely that interleaving.
        // The lock now makes it unreachable in production; what this
        // covers is the last line of defence, which is that the fill is
        // `add` and `add` has no way to delete anything.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();
        let before_anyone_died = |_x: i32, y: i32, _z: i32| {
            if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                return None;
            }
            Some(if y < 10 { BLOCK_STONE } else { BLOCK_AIR })
        };

        world.die_with(&mut chests, (0.5, 10.0, 0.5), &carrying(BLOCK_DIRT, 40));
        let (at, leftovers) = stash_backpack(
            &mut chests,
            &carrying(BLOCK_STICK, 7),
            (0.5, 10.0, 0.5),
            before_anyone_died,
            |x, y, z| world.place(x, y, z, BLOCK_BACKPACK),
        )
        .expect("nowhere to put the second bag");

        assert_eq!(at, (0, 10, 0), "the fixture did not stage the collision");
        assert!(leftovers.is_empty());
        assert_eq!(
            chests.contents(at).count(BLOCK_DIRT),
            40,
            "the first player's things were overwritten"
        );
        assert_eq!(
            chests.contents(at).count(BLOCK_STICK),
            7,
            "the second player's things went missing"
        );
    }

    #[test]
    fn a_bag_landing_on_a_full_one_merges_rather_than_replaces() {
        // The other half, and the reason the fill is `add` and not an
        // assignment. An entry against a cell can outlive the block it
        // belonged to; the old code called any such entry orphaned and
        // wrote straight over it. `add` has no way to delete anything --
        // whatever does not fit comes back for the caller to drop.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();
        chests.edit((0, 10, 0), |inventory| {
            inventory.add(BLOCK_DIRT, 12);
        });

        let stashed = stash_backpack(
            &mut chests,
            &carrying(BLOCK_STICK, 5),
            (0.5, 10.0, 0.5),
            |x, y, z| world.look(x, y, z),
            |x, y, z| world.place(x, y, z, BLOCK_BACKPACK),
        );
        let (at, leftovers) = stashed.expect("nowhere to put the bag");
        assert_eq!(at, (0, 10, 0));
        assert!(leftovers.is_empty());
        assert_eq!(chests.contents(at).count(BLOCK_DIRT), 12, "the old contents were wiped");
        assert_eq!(chests.contents(at).count(BLOCK_STICK), 5, "the new contents were lost");
    }

    #[test]
    fn a_cell_that_cannot_be_written_is_no_cell_at_all() {
        // `place` refusing is the world saying "not there". Filling the
        // chest map anyway would file somebody's things against a cell
        // holding no bag, where nothing will ever find them again.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();
        let stashed = stash_backpack(
            &mut chests,
            &carrying(BLOCK_DIRT, 3),
            (0.5, 10.0, 0.5),
            |x, y, z| world.look(x, y, z),
            |_, _, _| false,
        );
        assert!(stashed.is_none());
        assert!(chests.is_empty(), "things were filed against a cell with no bag in it");
    }
}

/// What `/save` actually writes.
///
/// Driven through `run_command` rather than by calling `save_everything`
/// directly, because the bug this covers was not in any save routine: it
/// was in the *command*, which called one of the three and was
/// documented as flushing the world. A test of the routine would have
/// passed throughout.
#[cfg(test)]
mod save_command_tests {
    use super::*;
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::{BLOCK_STONE, BLOCK_WATER};

    /// A directory that removes itself, so a failed run does not leave
    /// half a world in the system temp folder. Deliberately not under
    /// `saves/`: those are somebody's real worlds.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "primitive_save_cmd_{}_{tag}_{:?}",
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

    /// A server with somewhere to save to, and something in every one of
    /// the three places a save has to reach.
    fn loaded_context(dir: &std::path::Path) -> Arc<Context> {
        let settings = ServerSettings {
            bind_addr: "127.0.0.1:0".to_string(),
            world_dir: dir.display().to_string(),
            plugin_dir: String::new(),
            ..Default::default()
        };
        let ctx = build_context(settings, RunOptions::embedded()).expect("context");

        // A block edit, a chest with something in it, and a player who
        // has been here. One of each, because the bug was that only the
        // first of the three came out.
        ctx.world.set_block(0, 40, 0, BLOCK_STONE);
        {
            let mut chests = ctx.chests.lock().unwrap();
            chests.edit((0, 40, 1), |inventory| {
                inventory.add(BLOCK_WATER, 3);
            });
        }
        {
            let mut profiles = ctx.profiles.lock().unwrap();
            let joined = profiles.join("saver", (1.0, 2.0, 3.0), 20.0);
            profiles.store(joined.uuid, Inventory::new(), (9.0, 8.0, 7.0), 0.0, 0.0, 20.0, 0);
        }
        ctx
    }

    #[test]
    fn save_writes_the_chests_and_the_players_as_well_as_the_blocks() {
        // **The data-loss bug.** An operator types `/save` before doing
        // something risky, the server then dies, and the world comes
        // back with the walls they built and none of the things they
        // put behind them: chests and death packs open empty, and every
        // player is rolled back to the last autosave.
        let dir = TempDir::new("all_three");
        let ctx = loaded_context(&dir.0);

        let said = run_command(&ctx, "/save", commands::Permission::Operator, None);

        assert!(dir.0.join("edits.bin").exists(), "the blocks were not written");
        assert!(dir.0.join("chests.bin").exists(), "the chests were not written");
        assert!(
            dir.0.join("players.bin").exists(),
            "the player profiles were not written"
        );
        // ...and it said so, in all three. An operator who is told "the
        // world is saved" when two thirds of it are not is the whole of
        // how this bug went unnoticed.
        let reply = said.join(" ");
        for expected in ["1 block edit", "1 chest", "1 profile"] {
            assert!(reply.contains(expected), "'{expected}' missing from: {reply}");
        }
    }

    #[test]
    fn a_world_with_no_directory_says_so_rather_than_pretending() {
        let settings = ServerSettings {
            bind_addr: "127.0.0.1:0".to_string(),
            world_dir: String::new(),
            plugin_dir: String::new(),
            ..Default::default()
        };
        let ctx = build_context(settings, RunOptions::embedded()).expect("context");
        let said = run_command(&ctx, "/save", commands::Permission::Operator, None);
        assert!(
            said.iter().any(|line| line.contains("persistence is disabled")),
            "{said:?}"
        );
    }
}
