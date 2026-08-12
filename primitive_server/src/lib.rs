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

pub mod anticheat;
pub mod commands;
mod connection;
pub mod falling;
pub mod plugins;
pub mod players;
pub mod settings;
pub mod world;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;

use primitive_shared::protocol::{
    BlockChange, DisconnectReason, PlayerId, PlayerState, ServerMessage,
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
    /// Scripted plugins. One mutex around the whole host: hooks are
    /// short, and running two scripts concurrently would make plugin
    /// authors reason about data races in a scripting language.
    pub plugins: std::sync::Mutex<plugins::PluginHost>,
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
        plugins: std::sync::Mutex::new(plugins::PluginHost::new()),
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
                        tokio::spawn(connection::handle_connection(ctx, socket, addr));
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
        let tick = ctx.clock.advance();
        ctx.metrics.ticks.fetch_add(1, Ordering::Relaxed);

        // --- plugins ---
        // Once a second rather than every tick: a script that runs 20
        // times a second is a footgun for plugin authors, and nothing
        // a plugin does here needs tick precision.
        if tick % (ctx.settings.tick_rate_hz as u64).max(1) == 0 {
            fire_plugin_hook(&ctx, "on_tick", vec![crate::plugins::Value::Int(tick as i64)], None);
        }

        // --- falling blocks ---
        // Every tick, with the real timestep: falling blocks are
        // entities now, so this integrates their motion rather than
        // teleporting them one cell at a time.
        {
            let (changes, entity_count) = {
                let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                let changes = sim.step(&*ctx.world, tick_duration.as_secs_f32());
                (changes, sim.entity_count())
            };
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
                    for subscriber in ctx.registry.subscribers(pos) {
                        subscriber.send(ServerMessage::BlockUpdates(batch.clone()));
                    }
                }
            }
        }

        let entity_states: Vec<primitive_shared::protocol::EntityState> = {
            let sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
            sim.entities().iter().map(|e| e.state()).collect()
        };

        let handles = ctx.registry.handles();
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
                if !entity_states.is_empty() {
                    visible_entities.clear();
                    let radius_sq = radius * radius;
                    for state in &entity_states {
                        let (dx, dy, dz) =
                            (state.x - origin.0, state.y - origin.1, state.z - origin.2);
                        if dx * dx + dy * dy + dz * dz <= radius_sq {
                            visible_entities.push(*state);
                        }
                    }
                    if !visible_entities.is_empty() {
                        handle.send(ServerMessage::Entities {
                            tick,
                            states: visible_entities.clone(),
                        });
                    }
                }
            }

            if tick % keepalive_every == 0 {
                for handle in &handles {
                    if handle.idle_for() > client_timeout {
                        handle.request_kick(DisconnectReason::Timeout);
                    } else {
                        handle.send(ServerMessage::Ping { nonce: tick });
                    }
                }
            }

            if tick % time_sync_every == 0 {
                let time_of_day = ctx.clock.time_of_day();
                for handle in &handles {
                    handle.send(ServerMessage::TimeSync { tick, time_of_day });
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
        match ctx.world.save(&dir) {
            Ok(n) if ctx.options.logging => {
                println!("[world] saved {n} block edit(s) to {}", dir.display())
            }
            Ok(_) => {}
            Err(e) => eprintln!("[world] save failed: {e}"),
        }
    }
    if ctx.options.logging {
        println!("[server] bye");
    }
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

        Response::Save => match &ctx.world_dir {
            Some(dir) => match ctx.world.save(dir) {
                Ok(n) => vec![format!("saved {n} block edit(s) to {}", dir.display())],
                Err(e) => vec![format!("save failed: {e}")],
            },
            None => vec!["persistence is disabled (world_dir is empty)".to_string()],
        },

        Response::Stop => {
            ctx.request_shutdown();
            vec!["shutting down".to_string()]
        }
    }
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
