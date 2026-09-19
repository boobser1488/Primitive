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
    animals, anticheat, carrion, chunkgen, climate, commands, containers, drying, falling, felling,
    fire, growth, peat, walls,
    items, plugins, profiles, rafts, horses, rng, simulation, smelting, stalls, survival, water, weather, world,
};
#[cfg(feature = "mods")]
pub use logic::mods;
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
    /// When the clock was last set, and what it read then -- in *days*,
    /// whole days included, never wrapped. The wrap happens in
    /// `time_of_day`, at the last moment, because the seasons
    /// (`primitive_shared::season`) are a function of the day count and
    /// a clock that forgot its days at midnight left every world on day
    /// zero for ever: the seasonal offset climbed through each day and
    /// fell back at midnight, so mornings were colder than evenings by a
    /// few degrees and winter never came. `f64` because a day is 900
    /// seconds and a world can run for years of them.
    origin: std::sync::Mutex<(Instant, f64)>,
    day_length_seconds: f32,
    tick: AtomicU64,
}

impl WorldClock {
    /// `start_world_days` is the world's age in days with the hour in
    /// the fraction -- what `save_time_of_day` wrote -- and a plain
    /// hour-of-day from an older save reads as day zero, which is what it
    /// always was.
    fn new(start_world_days: f32, day_length_seconds: f32) -> Self {
        Self {
            origin: std::sync::Mutex::new((Instant::now(), f64::from(start_world_days))),
            day_length_seconds,
            tick: AtomicU64::new(0),
        }
    }

    /// 0.0 = midnight, 0.5 = noon.
    pub fn time_of_day(&self) -> f32 {
        self.world_days_f64().rem_euclid(1.0) as f32
    }

    /// The world's age in days, with the hour of the day in the fraction:
    /// what the seasons read (`primitive_shared::season::Season::at`).
    pub fn world_days(&self) -> f32 {
        self.world_days_f64() as f32
    }

    fn world_days_f64(&self) -> f64 {
        let (started, base) = *self.origin.lock().unwrap_or_else(|e| e.into_inner());
        base + f64::from(started.elapsed().as_secs_f32()) / f64::from(self.day_length_seconds)
    }

    /// Jumps the world clock (the `/time` command).
    /// Sets the hour and keeps the day: `/time night` is a change of
    /// hour, not a journey through the calendar.
    pub fn set_time_of_day(&self, time_of_day: f32) {
        let mut origin = self.origin.lock().unwrap_or_else(|e| e.into_inner());
        let days = {
            let (started, base) = *origin;
            base + f64::from(started.elapsed().as_secs_f32()) / f64::from(self.day_length_seconds)
        };
        *origin = (Instant::now(), days.floor() + f64::from(time_of_day.rem_euclid(1.0)));
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
    /// Microseconds actually spent *inside* a tick, summed.
    ///
    /// **Because "the tick rate is low" is not an answerable complaint
    /// without it.** A loop that is late is either doing too much or
    /// waiting badly, and those have opposite fixes; `tick_overruns`
    /// cannot tell them apart because it only fires when a tick exceeds
    /// the *whole* interval, which is a state a server reaches long
    /// after it has started running slow. Divided by `ticks` in the
    /// stats line, this says which half the time went to.
    pub tick_busy_micros: AtomicU64,
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
    /// Water spilled out of a broken barrel or a knocked-over jug, timed
    /// until it soaks away. See `logic::water::spill` and `tip_out_water`.
    pub spills: std::sync::Mutex<water::Spills>,
    /// Dropped stacks lying in the world. Same reasoning as `falling`.
    pub items: std::sync::Mutex<items::Items>,
    /// What is burning, and for how much longer.
    ///
    /// Its own field rather than a `CellMechanic` in the registry above,
    /// for the reason sand has one: three other paths ask it questions
    /// directly. Crafting asks whether there is a fire within reach,
    /// the use gesture lights and feeds one, and the save asks for its
    /// contents -- and a mechanic inside the registry is, by design,
    /// something nobody can reach except through `step`.
    pub fires: std::sync::Mutex<fire::Fires>,
    /// What is in every pit kiln and log pile, and how long each has left
    /// to burn. A field for the fires' reason: the use gesture builds and
    /// lights them, the break path spills them, the tick burns them and
    /// the save writes them. See `logic::pits`.
    pub pits: std::sync::Mutex<crate::logic::pits::Pits>,
    /// Wood that has caught from a fire, the smoke in closed rooms, the soot
    /// on ceilings and the standing torches' wads. A field for the fires'
    /// reason: the tick hands it the fire map's hearths and the players, the
    /// breath asks it about smoke, the resin gesture relights a torch and
    /// the save writes it. See `logic::wildfire`.
    pub wildfire: std::sync::Mutex<crate::logic::wildfire::Wildfire>,
    /// Which room each player stands in, cached, and the warmth each room
    /// holds from its fire. Taken on the warmth's sample only, never under
    /// another lock. See `logic::shelters`.
    pub shelters: std::sync::Mutex<crate::logic::shelters::Shelters>,
    /// Bushes waiting to fill again. Same reasoning: the tick loop has
    /// to hand it the player positions its sample walks out from.
    pub growth: std::sync::Mutex<growth::Growth>,
    /// How old every carcass in the world is. A field rather than a
    /// mechanic for the fires' reason: the rot clock drives it, the
    /// save asks for it, and what it produces is items on the ground,
    /// which needs the item store the registry cannot reach.
    pub carrion: std::sync::Mutex<carrion::Carrion>,
    /// Where the players live, and the clocks the rats come out on. A
    /// field and not a local of the tick, which it was until the map had
    /// to be saved: the tick warms it, the save writes it, and a local is
    /// something only the tick can reach. See `logic::vermin`.
    pub vermin: std::sync::Mutex<logic::vermin::Vermin>,
    /// Where the fish traps are, and whose line is in the water. A field for
    /// the carrion's reason: the rot clock fills the traps, the use gesture
    /// empties them and casts, the tick lands the lines and the save writes
    /// the list. See `logic::fishing`.
    pub fishing: std::sync::Mutex<crate::logic::fishing::Fishing>,
    /// Everything alive that is not a player.
    pub animals: std::sync::Mutex<animals::Animals>,
    /// Rafts on the water. A field for the fires' reason: the tick moves
    /// them, three gestures and a blow act on them, the entity snapshot and
    /// the save both ask for them. See `logic::rafts`.
    pub rafts: std::sync::Mutex<rafts::Rafts>,
    /// The weather, for the whole world. See `logic::weather`.
    pub sky: std::sync::Mutex<weather::Sky>,
    /// What is inside the chests **and the hearths**. One store: a
    /// hearth is a container, and everything written for chests --
    /// opening, moving, spilling on break, saving -- applies to it
    /// unchanged. What differs is which slots take what, and that is
    /// `primitive_shared::hearth`.
    pub chests: std::sync::Mutex<containers::Chests>,
    /// Who owns each barter stall and what it asks. The goods on one are in
    /// `chests`, at the stall's cell; see `logic::stalls` for why the two
    /// halves are kept apart. **Locked after `chests`**, never before: a
    /// trade holds the buyer, the store and this at once, in that order.
    pub stalls: std::sync::Mutex<stalls::Stalls>,
    /// Hides on racks, and how far along each of them is.
    ///
    /// Its own field rather than a `CellMechanic`, for the reason the
    /// fires have one: three paths ask it questions directly -- the use
    /// gesture puts a skin on and takes leather off, the break path
    /// spills what was on a rack that is gone, and the save asks for its
    /// contents. A mechanic inside the registry is, by design, something
    /// nobody can reach except through `step`.
    pub drying: std::sync::Mutex<drying::Drying>,
    /// Sods of peat set down on the ground to dry, and how far along each
    /// is (`logic::peat`). Beside the racks and saved with them: the same
    /// kind of wait, in the same weather, on the same slow clock.
    pub peat: std::sync::Mutex<peat::Peat>,
    /// Daub and cob drying on the walls they were laid on, and how far along
    /// each is (`logic::walls`): the peat's weather, and its file beside it.
    pub walls: std::sync::Mutex<walls::Walls>,
    /// How far along each burning hearth is. Not saved: see
    /// `logic::smelting`.
    pub smelting: std::sync::Mutex<smelting::Smelting>,
    /// Native mods. One mutex around the host for the reason the
    /// plugins have one: a hook is short, and two mods running
    /// concurrently would make every mod author reason about data races
    /// in somebody else's code.
    ///
    /// **Never held while a mod is called.** See `logic::mods` -- a
    /// handler that calls back into the host while the caller holds this
    /// would deadlock, and calling back into the host is the whole point
    /// of the API.
    #[cfg(feature = "mods")]
    pub mods: std::sync::Mutex<mods::ModHost>,
    /// Scripted plugins. One mutex around the whole host: hooks are
    /// short, and running two scripts concurrently would make plugin
    /// authors reason about data races in a scripting language.
    pub plugins: std::sync::Mutex<plugins::PluginHost>,
    /// Who has played here, and what they had when they left. Keyed by
    /// UUID; see `profiles`.
    pub profiles: std::sync::Mutex<profiles::Profiles>,
    /// Where terrain comes from: a priority queue and a small pool of
    /// threads, shared by every player. See `logic::chunkgen` for what
    /// it replaced and why.
    pub chunks: Arc<chunkgen::ChunkService>,
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
    /// Load native mods.
    ///
    /// **A switch of its own, and it used to be the plugins'.** Until
    /// singleplayer grew mods, one `if options.plugins` gated both, and
    /// that was fine for exactly as long as the answer was the same for
    /// both. It is not any more: a local world runs no scripting engine
    /// -- it does not even link one -- and does load mods, because the
    /// person who put a folder in `mods/` on their own machine meant
    /// it. Two questions, two answers, two fields.
    pub mods: bool,
    /// Everyone who joins is an operator.
    ///
    /// On for a world running inside the game client, and the argument
    /// is the one already written down beside
    /// `profiles::Profiles::is_operator` for the console: the person
    /// holding the keyboard the server runs under can already do
    /// anything a `/deop` would take away. A singleplayer world is that
    /// person's own machine, bound to loopback, with nobody else able
    /// to reach it -- and without this, the mods that this same change
    /// just made loadable would all refuse the only player there is,
    /// because operator-only is the sane default for a mod that can
    /// hand out flight or run commands.
    pub local_operator: bool,
    /// Read operator commands from stdin. A server embedded in the game
    /// client must not, or it would steal the terminal from the client.
    pub console: bool,
    /// Print the startup banner and the periodic stats line.
    pub logging: bool,
}

impl RunOptions {
    /// The `primitive_server` binary: everything on.
    ///
    /// Nobody is an operator here until somebody says so: a shared
    /// server is exactly the case the permission system exists for.
    pub fn standalone() -> Self {
        Self {
            plugins: true,
            mods: true,
            local_operator: false,
            console: true,
            logging: true,
        }
    }

    /// A world running inside the game client: no plugins, no console,
    /// and quiet, because its stdout is the player's -- but **with
    /// mods**, and with the only player holding operator rights.
    pub fn embedded() -> Self {
        Self {
            plugins: false,
            mods: true,
            local_operator: true,
            console: false,
            logging: false,
        }
    }
}

#[cfg(test)]
mod run_options_tests {
    use super::*;

    /// Somebody who joins arrives with their name.
    ///
    /// The contract has promised `text` on this event since 1.0 and the
    /// host quietly did not fill it, so every mod that greeted a player
    /// by name greeted an empty string -- which looks exactly like a
    /// working mod until you read what it printed. Asserted here rather
    /// than trusted, because nothing else fails when it is wrong.
    #[cfg(feature = "mods")]
    #[test]
    fn a_player_who_joins_or_leaves_brings_their_name_with_them() {
        use primitive_modapi::Event;
        let named = [
            plugins::Value::Int(7),
            plugins::Value::Text("Иван".to_string()),
        ];
        for event in [
            Event::PlayerJoined,
            Event::PlayerLeft,
            Event::PlayerChat,
            Event::PlayerDied,
        ] {
            let data = native_payload(event, &named, &[], "");
            assert_eq!(data.player, 7, "{event:?}");
            assert_eq!(
                unsafe { data.text.as_str() },
                "Иван",
                "{event:?} lost the name it was handed"
            );
        }
    }

    #[test]
    fn a_local_world_loads_mods_and_still_runs_no_scripts() {
        // The two used to be one flag, and this is the test that says
        // why they are not. A plugin is a script the *operator* of a
        // shared server installs for players who did not ask for it; a
        // mod on a local world is a folder the player put there on
        // their own machine. Singleplayer wants the second and not the
        // first -- and cannot have the first anyway, because the client
        // does not link a scripting engine at all.
        let local = RunOptions::embedded();
        assert!(local.mods, "a local world must load mods");
        assert!(!local.plugins, "a local world must not run scripts");

        let shared = RunOptions::standalone();
        assert!(shared.mods && shared.plugins, "a server runs both");
    }

    #[test]
    fn the_only_player_in_a_local_world_is_an_operator_and_nobody_on_a_server_is() {
        // Without the first half, every mod that defaults to
        // operator-only -- which is every mod that can hand out flight
        // or run a command, and it should be -- refuses the only player
        // there is, in the world that was just made able to load it.
        //
        // Without the second half, the same line would hand operator
        // rights to everyone who ever joined a public server, which is
        // the exact opposite failure and a much worse one.
        assert!(RunOptions::embedded().local_operator);
        assert!(!RunOptions::standalone().local_operator);
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
        // Every mechanic that watches cells hears about it, exactly as
        // they would if a player had made the edit.
        //
        // They did not, until 1.5, and the omission was invisible while
        // the only mechanics were sand and water -- both of which are
        // also driven by the *neighbours* of a change, so a block
        // dropped in by a tool got noticed on the next edit anyway. A
        // fire does not work that way: a lit campfire placed here and
        // never announced is a fire the server has no fuel timer for and
        // no crafting range around, which reads as a fire that does not
        // work.
        {
            let mut sim = self.ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
            sim.on_block_changed(x, y, z);
        }
        notify_mechanics(&self.ctx, x, y, z);
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

    // ---- doors for the tests ----
    //
    // Three of them, and each exists because the thing it reaches is
    // *state* rather than a message: the ambient temperature is
    // resampled twice a second and never published, a worn set is only
    // ever pushed on change, and a rack takes twelve minutes of world
    // time to finish. A test that could not reach these would have to
    // either sleep for a quarter of an hour or assert on nothing.
    //
    // They are deliberately read-only or idempotent, and none of them is
    // reachable from a client: a door a player could open would be a
    // cheat rather than a test hook.

    /// Brings down whatever tree is standing on a cell.
    ///
    /// The same path a player's break takes, and it exists for the same
    /// reason the other doors do: `place_block` writes a block without
    /// going through the break handler, so a test that stages a felling
    /// has no way to reach it otherwise.
    pub fn fell(&self, x: i32, y: i32, z: i32) {
        fell_tree(&self.ctx, (x, y, z), None);
    }

    /// What is at a cell, if anybody has that chunk.
    ///
    /// The cache and only the cache, on the same rule every other read
    /// outside the chunk pump follows -- see `World::cached`.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
        self.ctx.world.cached_block(x, y, z)
    }

    /// Puts blocks straight into the one connected player's pack.
    ///
    /// The same thing `/give` does, and it exists because `/give` cannot
    /// be driven from here: the command needs a *caller* to give to, and
    /// the console is not standing anywhere. A test with one player has
    /// no ambiguity about who is meant.
    ///
    /// Goes through `Inventory::add` and `send_inventory` exactly as
    /// every other path does, so what a test sets up is a pack the
    /// server really has and the client has really been told about.
    /// Answers how many would not fit.
    pub fn give(&self, block: primitive_shared::types::BlockId, count: u32) -> u32 {
        let Some(handle) = self.ctx.registry.handles().into_iter().next() else {
            return count;
        };
        let left = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let left = state.inventory.add(block, count);
            state.inventory_dirty = true;
            left
        };
        send_inventory(&handle);
        refresh_carried_weight(&handle);
        left
    }

    // ---- the same doors, for a server with two people on it ----
    //
    // **By name**, because "the one connected player" stops being a
    // question with an answer the moment a second client joins -- which is
    // what a trade between two people needs (`primitive_client`'s scenario
    // harness, `Scenario::join`). The unnamed doors above are kept: every
    // one-player test already says what it means with them.

    fn named(&self, name: &str) -> Option<Arc<players::PlayerHandle>> {
        self.ctx.registry.handles().into_iter().find(|handle| handle.username == name)
    }

    /// `give`, to the player called `name`. Answers how many did not fit --
    /// all of them, if nobody is called that.
    pub fn give_to(&self, name: &str, block: primitive_shared::types::BlockId, count: u32) -> u32 {
        let Some(handle) = self.named(name) else {
            return count;
        };
        let left = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let left = state.inventory.add(block, count);
            state.inventory_dirty = true;
            left
        };
        send_inventory(&handle);
        refresh_carried_weight(&handle);
        left
    }

    /// `teleport_player`, for the player called `name`.
    pub fn teleport_named(&self, name: &str, x: f32, y: f32, z: f32) {
        if let Some(handle) = self.named(name) {
            teleport(&handle, x, y, z, "scenario");
        }
    }

    /// What is in the container at a cell, as the server has it -- a
    /// stall's counter and till included. Read-only.
    pub fn container_at(&self, x: i32, y: i32, z: i32) -> primitive_shared::inventory::Inventory {
        self.ctx.chests.lock().unwrap_or_else(|e| e.into_inner()).contents((x, y, z))
    }

    /// Moves the one connected player, the way `/tp` does.
    ///
    /// A door for the scenario runner (`primitive_client`'s `scenario`),
    /// which plays the real client against this server with the movement
    /// validator *on* and needs to stand a player beside the thing under
    /// test. `/tp` cannot be driven from the console -- it has nobody to
    /// move -- and a client that simply walked there would be a walk the
    /// test did not ask about. Through `teleport`, so the anti-cheat and
    /// the fall tracker are told exactly as they are for a real `/tp`.
    pub fn teleport_player(&self, x: f32, y: f32, z: f32) {
        if let Some(handle) = self.ctx.registry.handles().into_iter().next() {
            teleport(&handle, x, y, z, "scenario");
        }
    }

    /// An animal of `species` at `at`, for a scenario: the spawner's own
    /// `Animals::spawn`, so it is capped and announced like any other.
    pub fn spawn_animal(&self, species: primitive_shared::animals::Species, at: (f32, f32, f32)) -> Option<primitive_shared::protocol::EntityId> {
        self.ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).spawn(species, at)
    }

    /// Puts a keeping, and a horse's gear, on an animal outright: what a
    /// scenario about riding does instead of spending two days gentling one.
    pub fn keep_animal(
        &self,
        id: primitive_shared::protocol::EntityId,
        keep: primitive_shared::husbandry::Keeping,
        gear: Option<primitive_shared::horse::Gear>,
    ) {
        self.ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).put_keeping(id, keep, gear);
    }

    /// Turns an animal to face `yaw`: a scenario stands a horse along its
    /// strip before it rides it.
    pub fn face_animal(&self, id: primitive_shared::protocol::EntityId, yaw: f32) {
        self.ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).face_for_test(id, yaw);
    }

    /// What a horse wears and carries, as the server has it.
    pub fn horse_gear(&self, id: primitive_shared::protocol::EntityId) -> Option<primitive_shared::horse::Gear> {
        self.ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).gear(id)
    }

    /// Where an animal's feet are, as the server has them.
    pub fn animal_position(&self, id: primitive_shared::protocol::EntityId) -> Option<(f32, f32, f32)> {
        self.ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).position(id)
    }

    /// Whether the one connected player is riding, as the server has it.
    pub fn player_riding(&self) -> Option<primitive_shared::protocol::EntityId> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.riding
    }

    /// What the world is doing to the one connected player.
    ///
    /// `None` for a server nobody is on. The *first* player, because
    /// every test that asks has exactly one -- a server with several
    /// would be a test asking an ambiguous question.
    pub fn player_ambient(&self) -> Option<f32> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(state.ambient.temperature_c)
    }

    /// What the one connected player has on.
    pub fn player_equipment(&self) -> Option<primitive_shared::inventory::Equipment> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(state.equipment.clone())
    }

    /// Wounds the one connected player, on a part and of a kind the test
    /// chooses.
    ///
    /// **Not a blow**, and on purpose: a blow's part is a roll, and a test
    /// of "a bandage on a cut arm" that had to hope the wolf bit the arm is
    /// a test that fails one run in five. The wound goes on exactly where
    /// it is asked for and the client is told through `send_injuries`, the
    /// path every real wound takes.
    pub fn injure(
        &self,
        part: primitive_shared::injury::Part,
        kind: primitive_shared::injury::Kind,
        severity: f32,
    ) {
        let Some(handle) = self.ctx.registry.handles().into_iter().next() else {
            return;
        };
        {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let mut wounds = *state.vitals.injuries();
            wounds.inflict(part, kind, severity);
            state.vitals.set_injuries(wounds);
        }
        send_injuries(&handle);
    }

    /// Sets how full the one connected player is, and tells them.
    ///
    /// A door for the reason the others are: a fresh player is full, eating
    /// on a full stomach is refused as an item destroyed, and the idle drain
    /// takes two minutes to empty a single point -- so a test of what a
    /// mouthful *does* (`tests/seen_by_others.rs`, the raw fish) could not
    /// otherwise take the mouthful at all.
    pub fn set_player_nourishment(&self, value: f32) {
        let Some(handle) = self.ctx.registry.handles().into_iter().next() else {
            return;
        };
        handle
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .vitals
            .set_nourishment(value);
        send_nourishment(&handle);
    }

    /// Brings the one connected player's last bad mouthful on now, rather
    /// than after `body::DIGESTION_SECONDS` -- a door for the raw fish test,
    /// which is about what the illness does and cannot wait two minutes to
    /// see it.
    pub fn digest_player_meal(&self) {
        if let Some(handle) = self.ctx.registry.handles().into_iter().next() {
            handle.state.lock().unwrap_or_else(|e| e.into_inner()).vitals.digest_now();
        }
    }

    /// What is wrong with the one connected player, as the server has it.
    pub fn player_injuries(&self) -> Option<primitive_shared::injury::Injuries> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(*state.vitals.injuries())
    }

    /// The one connected player's pack, as the server has it.
    pub fn player_inventory(&self) -> Option<primitive_shared::inventory::Inventory> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(state.inventory.clone())
    }

    /// Runs the pit kilns and log piles on by `seconds` at once.
    ///
    /// **An hour is not a test**, and this is how a test gets through one:
    /// the same step the tick runs, with the elapsed time handed in, so
    /// what a test sees at the end -- the fired pots, the broadcast, the
    /// news -- is exactly what a player sees at the end of the real hour.
    pub fn advance_pits(&self, seconds: f64) {
        step_pits(&self.ctx, seconds);
    }

    /// Pushes a rack to the end of its cure.
    ///
    /// The same door the test world uses to arrive with a nearly
    /// finished hide on one -- see `showcase::rack_stock` -- rather than
    /// a second way in written for the tests.
    pub fn finish_drying(&self, at: drying::RackPos) {
        {
            let mut chests = self.ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
            let mut racks = self.ctx.drying.lock().unwrap_or_else(|e| e.into_inner());
            racks.set_progress(at, 1.0);
            // Banked here rather than left for the next step, so a test
            // that calls this and looks immediately afterwards sees the
            // leather. The step would do exactly this within two seconds
            // -- see `Drying::step` -- and a test that has to sleep for
            // it is a test that is sometimes flaky instead.
            if chests.edit(at, primitive_shared::rack::complete) {
                racks.set_progress(at, 0.0);
            }
        }
        broadcast_chest_state(&self.ctx, at);
    }

    /// Pushes a sod of peat lying at `at` to `progress` of its drying.
    ///
    /// The rack's door (`finish_drying`) for the peat's reason: a sod takes
    /// a quarter of an hour of sun, which is not a test. Only the progress
    /// is set; the *stage* still turns on the ordinary step, in whatever
    /// sky there is -- so a scenario that sets a sod a breath short of dry
    /// and sees it turn has seen the sun do it, and one that set it in the
    /// rain would see it go back. A piece of raw pottery set down is on the
    /// same list (`peat`, "raw pottery dries here too") and is pushed the
    /// same way.
    pub fn set_peat_progress(&self, at: (i32, i32, i32), progress: f32) {
        self.ctx.peat.lock().unwrap_or_else(|e| e.into_inner()).set_progress(at, progress);
    }

    /// How wet the player called `name` is, 0..1, as the server has it --
    /// the number the health page is sent on a gate, read without the gate.
    pub fn wetness_of(&self, name: &str) -> Option<f32> {
        self.named(name).map(|handle| handle.state.lock().unwrap_or_else(|e| e.into_inner()).vitals.wetness())
    }

    /// Whether the player called `name` is asleep in a bed, as the server has
    /// it -- which is a tick ahead of any client, and that tick is what a
    /// scenario timing a night on the client's word raced (see
    /// `a_lean_to_keeps_its_sleeper_out_of_the_rain_and_falls_in_at_dawn`).
    pub fn asleep(&self, name: &str) -> bool {
        self.named(name).is_some_and(|handle| handle.state.lock().unwrap_or_else(|e| e.into_inner()).sleeping_in.is_some())
    }

    /// Where the server has the feet of the player called `name` -- which,
    /// for a sleeper, is where `lying_place` put them and no client has
    /// moved them from: what a scenario asks to know a body was laid in a
    /// lean-to and not on its roof.
    pub fn position_of(&self, name: &str) -> Option<(f64, f64, f64)> {
        self.named(name).map(|handle| handle.state.lock().unwrap_or_else(|e| e.into_inner()).position)
    }

    /// The server's own tick count: what a scenario waits on when it needs
    /// the server to have *done* something a number of times, rather than
    /// the client to have waited a while -- the two part company on a
    /// machine running a whole test suite at once.
    pub fn ticks(&self) -> u64 {
        self.ctx.clock.tick()
    }

    /// Sets how long a player's wet pack has been drying (`wet::pack_weather`),
    /// `peat_progress`'s way: a scenario sets it a breath short and watches
    /// the ordinary sample finish the job by a fire, or a shower undo it,
    /// rather than sitting through the minute.
    pub fn set_pack_drying(&self, name: &str, seconds: f32) {
        if let Some(handle) = self.named(name) {
            handle.state.lock().unwrap_or_else(|e| e.into_inner()).pack_drying = seconds;
        }
    }

    /// Fills a hearth's room with smoke at once, `peat_progress`'s way: a
    /// scenario sets the room a breath from full and watches the ordinary
    /// step keep it there or clear it, rather than waiting the minute and a
    /// half the smoke takes to rise (`wildfire::SMOKE_RISE_PER_SECOND`).
    pub fn set_smoke(&self, hearth: (i32, i32, i32), thickness: f32) {
        self.ctx.wildfire.lock().unwrap_or_else(|e| e.into_inner()).set_smoke(hearth, thickness);
    }

    /// One step of the rot clock over every trap, snare and salt pan, now,
    /// in the weather the sky has -- the step the tick loop's rot pass takes
    /// four times a day (`fill_traps`). For the scenarios, which cannot wait
    /// a quarter of a day for a hare.
    pub fn step_traps(&self) {
        let weather = self.ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
        fill_traps(&self.ctx, self.ctx.clock.time_of_day(), weather);
    }

    /// `steps` of the rot clock's slow changes -- a cheese ripening, a must
    /// working, a pot drying (`rot::Rot::cure_inventory`) -- over the one
    /// player's pack, in air at `temperature_c` and otherwise the air they
    /// stand in. For the scenarios: a cellar is a temperature, and two days
    /// in one is not a thing a test waits for.
    pub fn work_pack(&self, steps: u64, temperature_c: f32) {
        let Some(handle) = self.ctx.registry.handles().into_iter().next() else {
            return;
        };
        {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let air = logic::climate::Ambient { temperature_c, ..state.ambient };
            for step in 1..=steps {
                logic::rot::Rot::cure_inventory(&mut state.inventory, step, &air);
            }
            state.inventory_dirty = true;
        }
        send_inventory(&handle);
    }

    /// The one player's body temperature, in `body`'s degrees.
    pub fn player_body_c(&self) -> Option<f32> {
        let handle = self.ctx.registry.handles().into_iter().next()?;
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(state.vitals.temperature())
    }

    /// Sets the one player's body temperature, as a cold night would have.
    pub fn chill_player(&self, body_c: f32) {
        if let Some(handle) = self.ctx.registry.handles().into_iter().next() {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let wet = state.vitals.wetness();
            state.vitals.set_warmth(body_c, wet);
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

    // --- native mods ---
    //
    // After the plugins, so a mod that runs a command in `on_load` runs
    // it against a server whose scripts are already in place -- but on
    // a switch of its own, because singleplayer wants these and not
    // those. See `logic::mods` for the order within the load itself.
    #[cfg(feature = "mods")]
    if options.mods {
        // Whatever each mod saved last time, restored before any of them
        // is called -- a mod's `on_load` is exactly where it will ask
        // for its blob.
        if let Some(dir) = &ctx.world_dir {
            match load_mod_blobs(dir) {
                Ok(blobs) if !blobs.is_empty() => {
                    let count = blobs.len();
                    ctx.mods
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .restore_blobs(blobs);
                    if options.logging {
                        println!("[mods] restored saved state for {count} mod(s)");
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("[mods] could not read saved mod state: {e}"),
            }
        }
        let dir = PathBuf::from(&ctx.settings.mod_dir);
        // **Whether to say any of this out loud.**
        //
        // A singleplayer client is quiet on purpose: its stdout is the
        // player's, and "no mod directory at mods" every time somebody
        // opens a world is noise about a feature they are not using.
        // But a player who *did* put a folder there wants to know what
        // became of it -- a mod that refused to load is otherwise a
        // mod that silently does nothing. So: no folder and no logging,
        // say nothing; a folder, say everything, exactly as a dedicated
        // server does.
        let announce = options.logging || dir.is_dir();
        let host = mods::HostContext(Arc::clone(&ctx));
        // Opening the libraries under the lock is safe -- nothing a mod
        // runs at that point can reach back, because the entry point is
        // handed a table and told to describe itself. `start_all` is
        // where the mods' own code runs, and it takes no lock across a
        // call.
        let lines = {
            let mut mods = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
            mods.load_dir(&dir, host)
        };
        for line in lines {
            if announce {
                println!("[mods] {line}");
            }
        }
        for line in mods::start_all(&ctx) {
            if announce {
                println!("[mods] {line}");
            }
        }
        let active = ctx
            .mods
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_count();
        if announce {
            println!("[mods] {active} active");
        }
        // ...and now the terrain hook, because now there is something to
        // hook. Installed after `start_all`, which is where a mod calls
        // `register_decorator`, and before the spawn area is queued
        // below -- so the first chunk this world ever makes is already
        // decorated.
        install_chunk_decorators(&ctx);
        // The server is up. Told after the load rather than during it,
        // so a mod's `ServerStarted` handler sees every other mod.
        let _ = notify_mods!(
            &ctx,
            primitive_modapi::Event::ServerStarted,
            primitive_modapi::EventData::default()
        );
    }

    // The spawn area, made now rather than when somebody walks into it.
    //
    // Queued rather than generated here: `start` returns as soon as the
    // socket is accepting, and a server that spent a second on terrain
    // before opening its port would look like a server that failed to
    // start. The pool has nothing else to do at this moment, so by the
    // time the first client has finished its handshake the ground under
    // the spawn point is already there.
    //
    // **A disc, because that is what a client asks for.** The streamer
    // keeps a circle round the player (see
    // `primitive_client::chunk_manager::inside`), so the corners of a
    // square here are terrain generated at startup that nobody will
    // request -- a quarter of the work, spent before the first player
    // has finished their handshake.
    if ctx.settings.pregenerate_radius_chunks > 0 {
        let spawn = ctx.world.spawn_point();
        let centre = ChunkPos::from_world(spawn.0, spawn.2);
        let r = ctx.settings.pregenerate_radius_chunks;
        ctx.chunks.request_many((-r..=r).flat_map(|dz| {
            (-r..=r).filter_map(move |dx| {
                let distance = (dx * dx + dz * dz) as i64;
                // The priority and the test are the same number: nearest
                // first, and nothing past the radius at all.
                (distance <= (r * r) as i64)
                    .then(|| (ChunkPos::new(centre.x + dx, centre.z + dz), distance))
            })
        }));
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

/// A context with nothing running on it, for tests.
///
/// The ordinary way in is [`start`], which binds a socket and spawns
/// four background loops -- more than a test of one subsystem wants, and
/// a port a test of one subsystem should not be taking. This is the same
/// construction without any of that.
///
/// Panics rather than returning a `Result`, which is what a test wants
/// from a fixture: a context that could not be built is not a case to
/// handle, it is a broken test.
pub fn test_context(settings: ServerSettings, options: RunOptions) -> Arc<Context> {
    build_context(settings, options).expect("a context with no world directory cannot fail")
}

fn build_context(settings: ServerSettings, options: RunOptions) -> anyhow::Result<Arc<Context>> {
    let world = Arc::new(World::with_scale(
        settings.world_seed,
        settings.world_preset,
        settings.world_zone,
        settings.world_scale,
        settings.max_cached_chunks,
    ));
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

    // The test world's chests, on the first run of a world that has
    // them.
    //
    // **Only when the store is empty**, which is the whole of the
    // "first run" test and is deliberately not a flag in a file. A
    // player who empties the plaza chests has emptied them; a world that
    // restocked itself every restart would be a world where nothing a
    // player did to a chest lasted the evening. The cost of the rule is
    // that a player who breaks *every* chest in the world gets a fresh
    // set on the next start, which is a strange thing to do and a
    // harmless thing to be handed.
    if settings.world_preset == primitive_shared::worldgen::Preset::Test && chests.is_empty() {
        for (at, inventory) in primitive_shared::showcase::chest_stock() {
            chests.edit(at, |chest| *chest = inventory);
        }
        if options.logging {
            println!("[world] stocked {} chest(s) for the test world", chests.len());
        }
    }

    // ...and the fires, from a file of their own beside both. Same
    // story: a world saved before fires existed has none, which reads
    // as "nothing is burning".
    let mut fires = fire::Fires::new();
    if let Some(dir) = &world_dir {
        match fires.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} fire(s) still burning"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load fires: {e}"),
        }
    }

    // ...and whatever is alight that is not a hearth, and the standing
    // torches -- see `logic::wildfire`. A world from before has none.
    let mut wildfire = crate::logic::wildfire::Wildfire::new();
    if let Some(dir) = &world_dir {
        match wildfire.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} burning block(s) and standing torch(es)"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load what is burning: {e}"),
        }
    }

    // ...and how far gone every carcass is, from a file beside them.
    // A world saved before carcasses spoiled has none, which reads as
    // "every kill out there is fresh" -- see `logic::carrion`.
    let mut carrion = carrion::Carrion::new();
    if let Some(dir) = &world_dir {
        match carrion.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} carcass(es) going off"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load carrion: {e}"),
        }
    }

    // ...and the kept animals, parked until somebody comes near them. A
    // world saved before animals were kept has none, and every animal in it
    // is as wild as it always was -- see `Animals::load_herd`.
    let mut herd = animals::Animals::new();
    if let Some(dir) = &world_dir {
        match herd.load_herd(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} kept animal(s)"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load the kept animals: {e}"),
        }
    }

    // ...and where the players live, which is where the rats come out. A
    // world saved before the map was has none: nobody lives anywhere yet,
    // and the house is found again in the few minutes it always took.
    let mut vermin = logic::vermin::Vermin::new();
    if let Some(dir) = &world_dir {
        match vermin.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} lived-in place(s) remembered"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load where the players live: {e}"),
        }
    }

    // ...and where the fish traps are, from a file of their own. A world
    // saved before traps has none; the fish already in a trap are in the
    // block and do not need the file. See `logic::fishing`.
    let mut fishing = crate::logic::fishing::Fishing::new();
    if let Some(dir) = &world_dir {
        match fishing.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} fish trap(s) in the water"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load traps.bin, so no trap fills until it is touched: {e}"),
        }
    }

    // ...and the rafts, where they were left. A world saved before rafts
    // has none. See `Rafts::load` for why a file that cannot be read is
    // said out loud rather than read as an empty lake.
    // ...and who owns each stall, from its own file. See `logic::stalls`
    // for why an unreadable one is said out loud.
    let mut stall_owners = stalls::Stalls::new();
    if let Some(dir) = &world_dir {
        match stall_owners.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] restored {n} stall(s)"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load stalls: {e}"),
        }
    }

    let mut rafts = rafts::Rafts::new();
    if let Some(dir) = &world_dir {
        match rafts.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} raft(s) on the water"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load rafts: {e}"),
        }
    }

    // ...and what is in the pit kilns and log piles, and how long each has
    // left to burn, from a file of its own. A world saved before pits
    // existed has none, which reads as "nothing is in any pit".
    let mut pits = crate::logic::pits::Pits::new();
    if let Some(dir) = &world_dir {
        match pits.load(dir) {
            Ok(0) => {}
            Ok(n) if options.logging => println!("[world] {n} pit kiln(s) and log pile(s) with something in them"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load pits.bin, so every pit starts empty: {e}"),
        }
    }
    // The test world's kilns, on the chests' "first run" rule: pottery in
    // every kiln the plot was built with, so each stage can be taken on
    // from where it stands.
    if settings.world_preset == primitive_shared::worldgen::Preset::Test && pits.is_empty() {
        for (at, pottery) in primitive_shared::showcase::pit_kiln_stock() {
            pits.stock_kiln(at, &pottery);
        }
    }

    // ...and the hearths the test world was *built* alight.
    //
    // **A lit block is not a burning fire.** Fire is state in the map
    // above, and the only thing that ever puts an entry in it is a
    // player striking flint -- so a hearth the generator drew as lit
    // draws as lit, throws light as lit, and refuses to cook anything,
    // because nothing ever told `Fires` it exists. Registering them here
    // is what makes the plaza's fire a fire.
    //
    // Read out of the *world* rather than out of the generator, so a
    // hearth a player has since broken or let go out is not relit behind
    // their back -- `generate` is generation with the player's own edits
    // laid over it. And only when nothing is burning at all, which is
    // the same "first run" test the chests use above.
    if settings.world_preset == primitive_shared::worldgen::Preset::Test && fires.is_empty() {
        for pos in primitive_shared::showcase::built_chunks() {
            let chunk = world.generate(pos);
            for y in 0..primitive_shared::types::CHUNK_SIZE_Y {
                for z in 0..primitive_shared::types::CHUNK_SIZE_Z {
                    for x in 0..primitive_shared::types::CHUNK_SIZE_X {
                        let block = chunk.get(x, y, z);
                        let at = (
                            pos.x * primitive_shared::types::CHUNK_SIZE_X as i32 + x as i32,
                            y as i32,
                            pos.z * primitive_shared::types::CHUNK_SIZE_Z as i32 + z as i32,
                        );
                        // A burning kiln or pile the world was born with is
                        // the same story on the pits' own clock: queued, so
                        // the first tick times it (`Pits::step`).
                        if primitive_shared::pit::smokes(block) {
                            pits.on_block_changed(at.0, at.1, at.2);
                        }
                        if !primitive_shared::types::is_burning(block) {
                            continue;
                        }
                        fires.light(at);
                    }
                }
            }
        }
        if options.logging && !fires.is_empty() {
            println!("[world] lit {} hearth(s) for the test world", fires.len());
        }
    }

    // ...and the racks, from a file of their own beside both. Same
    // story again: a world saved before racks existed has none, which
    // reads as "nothing is drying".
    let mut racks = drying::Drying::new();
    if let Some(dir) = &world_dir {
        match racks.load(dir) {
            // **The skins out of a world saved before racks had an
            // inside.** They used to live in the rack file; they live in
            // the container store now, and a world that opened with
            // every frame mysteriously bare would be this change eating
            // a player's hides. See `Drying::load`.
            Ok(skins) => {
                for (at, raw) in &skins {
                    chests.edit(*at, |contents| {
                        contents.put_in_slot(
                            primitive_shared::rack::HIDE_SLOT,
                            primitive_shared::inventory::Stack::new(*raw, 1),
                        );
                    });
                }
                if options.logging && !skins.is_empty() {
                    println!("[world] moved {} hide(s) onto the new racks", skins.len());
                }
                if options.logging && !racks.is_empty() {
                    println!("[world] {} hide(s) still drying", racks.len());
                }
            }
            Err(e) => eprintln!("[world] could not load racks: {e}"),
        }
    }
    // ...and the peat lying out to dry, from its own file on the same terms.
    let mut sods = peat::Peat::new();
    if let Some(dir) = &world_dir {
        match sods.load(dir) {
            Ok(n) if options.logging && n > 0 => println!("[world] {n} sod(s) of peat drying on the ground"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load the peat: {e}"),
        }
    }
    // ...and the walls drying where they were laid.
    let mut wet_walls = walls::Walls::new();
    if let Some(dir) = &world_dir {
        match wet_walls.load(dir) {
            Ok(n) if options.logging && n > 0 => println!("[world] {n} wall(s) drying"),
            Ok(_) => {}
            Err(e) => eprintln!("[world] could not load the drying walls: {e}"),
        }
    }

    // ...and the hides the test world was built with already on its
    // racks.
    //
    // **A rack the generator drew is an empty rack**, because a rack is
    // a block and what is on it is server state -- the same split the
    // lit hearths above have, and for the same reason. Without this the
    // *end* of the drying process is twelve minutes away from a fresh
    // world, which is twelve minutes of not being able to check that it
    // works. See `showcase::rack_stock` for what arrives and how far
    // along.
    //
    // Only when nothing is drying at all, which is the same "first run"
    // test the chests and the fires use: a player who took the hides off
    // has taken them off.
    if settings.world_preset == primitive_shared::worldgen::Preset::Test && racks.is_empty() {
        for (at, raw, progress) in primitive_shared::showcase::rack_stock() {
            chests.edit(at, |contents| {
                contents.put_in_slot(
                    primitive_shared::rack::HIDE_SLOT,
                    primitive_shared::inventory::Stack::new(raw, 1),
                );
            });
            racks.set_progress(at, progress);
        }
        if options.logging && !racks.is_empty() {
            println!("[world] {} hide(s) already on the racks", racks.len());
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
        if profiles.is_unreadable() {
            eprintln!(
                "[players] the players file is not one this build can read; it is left as it is, \
                 and nobody's progress this run will be saved over it"
            );
        }
    }

    let ctx = Arc::new(Context {
        chunks: chunkgen::ChunkService::start(
            Arc::clone(&world),
            settings.generator_threads,
        ),
        registry: Arc::new(Registry::new(
            settings.max_players,
            settings.max_connections_per_ip,
        )),
        clock: Arc::new(WorldClock::new(
            // **The hour the world was left at, if it has one.** The
            // clock used to start at the setting every time, so a
            // player who logged out at dusk came back to mid-morning
            // and a world had no history of its own days at all. The
            // setting is what a *new* world starts at; a saved one
            // starts where it stopped.
            world_dir
                .as_ref()
                .and_then(|dir| load_time_of_day(dir))
                .unwrap_or(settings.start_time_of_day),
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
            mechanics.register(Box::new(water::Water::soaking()));
            mechanics
        }),
        spills: std::sync::Mutex::new(water::Spills::new()),
        items: std::sync::Mutex::new(items::Items::new()),
        fires: std::sync::Mutex::new(fires),
        pits: std::sync::Mutex::new(pits),
        wildfire: std::sync::Mutex::new(wildfire),
        shelters: std::sync::Mutex::new(crate::logic::shelters::Shelters::new()),
        growth: std::sync::Mutex::new(growth::Growth::new()),
        carrion: std::sync::Mutex::new(carrion),
        vermin: std::sync::Mutex::new(vermin),
        fishing: std::sync::Mutex::new(fishing),
        animals: std::sync::Mutex::new(herd),
        rafts: std::sync::Mutex::new(rafts),
        sky: std::sync::Mutex::new(weather::Sky::new()),
        chests: std::sync::Mutex::new(chests),
        stalls: std::sync::Mutex::new(stall_owners),
        drying: std::sync::Mutex::new(racks),
        peat: std::sync::Mutex::new(sods),
        walls: std::sync::Mutex::new(wet_walls),
        smelting: std::sync::Mutex::new(smelting::Smelting::new()),
        #[cfg(feature = "mods")]
        mods: std::sync::Mutex::new(mods::ModHost::new()),
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
/// Asks Windows for a timer it can actually keep.
///
/// **Without this the server runs a fifth slow, and nothing says so.**
/// Windows' default timer granularity is 15.6 ms, and a sleep is
/// rounded *up* to the next multiple of it. A 20 Hz server asks for a
/// 50 ms tick, gets 62.5 ms, and settles at exactly 16.0 ticks per
/// second -- measured, repeatedly, on a machine with no load and with
/// the tick loop reporting zero overruns. 25 Hz gives 21, 64 Hz gives
/// 32. The pattern is the granularity, not the work.
///
/// Two things go wrong with that, and the second is the one nobody
/// would trace back to a timer:
///
///   * The world is stepped by `tick_duration` -- the time the tick was
///     *meant* to take -- so it advances 50 ms of simulation per 62.5 ms
///     of daylight. Growth, fire, drying and every animal run at four
///     fifths speed against the clock on the wall.
///   * `Welcome` tells the client 20 Hz, and the client interpolates
///     entities over that interval. Snapshots arrive every 62.5 ms
///     instead, so every animal reaches its target and then waits a
///     twelfth of a second for the next one. It reads as a stutter in
///     the animation, not as a server problem.
///
/// `timeBeginPeriod(1)` is the standard answer and the one media players
/// and games have used for thirty years. Since Windows 10 2004 it is
/// per-process, so it no longer imposes anything on the rest of the
/// machine. Declared here rather than pulled in with a crate: it is one
/// function from a library that ships with the OS.
///
/// Never undone. It lasts as long as the process, which is exactly how
/// long the tick loop needs it.
#[cfg(windows)]
fn ask_for_a_finer_timer() {
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
    }
    // SAFETY: one integer in, a status out, no memory involved. A
    // refusal (anything but 0) is not worth acting on -- the server
    // still runs, just at the coarse rate it ran at before.
    let granted = unsafe { timeBeginPeriod(1) } == 0;
    if !granted {
        eprintln!("[server] the system refused a 1 ms timer; ticks will be coarse");
    }
    stop_windows_from_ignoring_the_timer();
}

/// Asking for a fine timer is not enough on Windows 11; you must also
/// say you meant it.
///
/// Windows 11 throttles processes it considers background -- and a
/// server with no window is always background -- by *ignoring* their
/// `timeBeginPeriod` request. The call still returns success. That is
/// what made the first attempt at this look like a wrong diagnosis:
/// the timer was granted, reported granted, and had no effect, and
/// ticks stayed pinned at 16 Hz (62.5 ms, exactly four of the coarse
/// 15.625 ms quanta) while `busy` said the tick body cost 0.0 ms. A
/// loop that does no work and still misses its deadline is not slow,
/// it is being put to sleep for too long.
///
/// `ControlMask` names the throttle we have an opinion about;
/// `StateMask` of zero is that opinion: do not ignore our request.
/// Everything we do not name is left to the system.
#[cfg(windows)]
fn stop_windows_from_ignoring_the_timer() {
    #[repr(C)]
    struct PowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    const CURRENT_VERSION: u32 = 1;
    const IGNORE_TIMER_RESOLUTION: u32 = 0x4;
    /// `ProcessPowerThrottling` in `PROCESS_INFORMATION_CLASS`.
    const PROCESS_POWER_THROTTLING: u32 = 4;

    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(
            process: isize,
            class: u32,
            information: *mut core::ffi::c_void,
            size: u32,
        ) -> i32;
    }

    let mut state = PowerThrottlingState {
        version: CURRENT_VERSION,
        control_mask: IGNORE_TIMER_RESOLUTION,
        state_mask: 0,
    };

    // SAFETY: the struct matches `PROCESS_POWER_THROTTLING_STATE` field
    // for field, and we pass its own size. Windows 10 before 1709 has
    // no such class and fails the call, which costs us nothing beyond
    // the coarse ticks we already had.
    let honoured = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            (&mut state as *mut PowerThrottlingState).cast(),
            core::mem::size_of::<PowerThrottlingState>() as u32,
        ) != 0
    };
    if !honoured {
        eprintln!("[server] the system kept its timer throttle; ticks will be coarse");
    }
}

#[cfg(not(windows))]
fn ask_for_a_finer_timer() {}

async fn tick_loop(ctx: Arc<Context>) {
    // Before the ticker is built, because it is the ticker this is for.
    ask_for_a_finer_timer();

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
    /// Above this many blocks a second, a player is sprinting.
    ///
    /// Between a walk (5.5) and a sprint (8.8), so that neither a
    /// laggy update nor a player being carried downhill by their own
    /// momentum is billed as running. See `survival::Effort`.
    const SPRINT_THRESHOLD: f32 = 7.0;
    // Where each player was when hunger last billed them, so the
    // distance moved can be measured without a field on the player.
    //
    // A local rather than state on `PlayerRuntime`, because it is the
    // tick loop's own bookkeeping: nothing else in the server has any
    // business knowing where somebody was one tick ago, and a field
    // would invite something to.
    let mut billed_position: std::collections::HashMap<PlayerId, (f32, f32, f32)> =
        std::collections::HashMap::new();
    // What the sky puts back into standing water. A local for the same
    // reason: it is a clock and a random number generator, it belongs to
    // the tick and nothing else in the server has any business reaching
    // for it. See `logic::water::Rainfall`.
    let mut rainfall = water::Rainfall::new();
    // ...and what the year does to standing water: the bay that closes in
    // the autumn and opens in the spring. A local for the rain's reason --
    // it is a clock and a random number generator, and it belongs to the
    // tick. See `logic::water::Frost`.
    let mut frost = water::Frost::new();
    frost.reach_view(ctx.settings.view_distance_chunks);
    let mut snowfall = crate::logic::snowfall::Snowfall::new();
    snowfall.reach_view(ctx.settings.view_distance_chunks);
    // The rain on the boards; a local for the snow's reason. See
    // `logic::weathering`.
    let mut weathering = crate::logic::weathering::Weathering::new();
    // The clock the world's food goes off by. A local for the same
    // reason again; see `logic::rot`.
    let mut rot = logic::rot::Rot::new();
    // ...and whether they were standing on something, which is the only
    // way the server can see a jump: a player who was on the ground, is
    // not now, and has gone *up* has jumped. Leaving a ledge is the same
    // transition without the rise, and it costs nothing, which is right
    // -- falling is not effort.
    let mut billed_grounded: std::collections::HashMap<PlayerId, bool> =
        std::collections::HashMap::new();
    // Whether the last `Entities` message sent to each player had
    // anything in it, so the *first* empty one is sent and the rest are
    // not. See the note at the send site.
    let mut sent_entities: std::collections::HashMap<PlayerId, bool> =
        std::collections::HashMap::new();
    // ...and whether each head was under water last tick, which is the
    // only way to turn a *state* the tick loop already computes into the
    // two *events* a mod can act on. A local for the same reason the
    // three above are: nothing else in the server has any business
    // knowing whether somebody was submerged one tick ago, and a field
    // on `PlayerRuntime` would invite something to.
    let mut was_submerged: std::collections::HashMap<PlayerId, bool> =
        std::collections::HashMap::new();

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

            broadcast_changes(&ctx, changes);
            // After the blocks have moved and before anything else reads
            // health: whoever a block fell through this tick is hurt now,
            // through the armour path. See `logic::collapse`.
            logic::collapse::crush_whoever_is_under(&ctx);
        }

        // --- the sky ---
        //
        // Before anything that reads it: the fires want to know whether
        // it is raining on them this tick rather than last one. Costs a
        // subtraction on every tick and a message a few times an hour.
        let raining = {
            let (changed, now) = {
                let mut sky = ctx.sky.lock().unwrap_or_else(|e| e.into_inner());
                let changed = sky.step(tick_duration.as_secs_f32());
                // Read out of the same lock the step took, because the
                // rain below wants it and a second lock a tick for a
                // value that has just been computed is a lock a tick for
                // nothing.
                (changed, sky.weather())
            };
            if let Some(weather) = changed {
                ctx.fires
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_weather(weather);
                // ...and the pits, which the rain puts out on the same
                // news.
                ctx.pits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_weather(weather);
                weather_changed(&ctx, weather);
                if let Some(frame) = players::frame(&ServerMessage::WeatherSync { weather }) {
                    for handle in ctx.registry.handles() {
                        handle.send_raw(Arc::clone(&frame));
                    }
                }
                if ctx.options.logging {
                    println!("[weather] {}", weather.name());
                }
            }
            now.is_wet()
        };

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

        // --- the rain, into whatever standing water it can find ---
        //
        // **The one thing in the game that makes water**, and the only
        // reason a world that has been played in for a month is not
        // drier than the one that was generated. Everything about how it
        // is bounded is in `logic::water::Rainfall`; what belongs here is
        // only that it needs to know where people are, which this file
        // knows and that one does not.
        //
        // The clock is asked first and the positions collected second,
        // so a dry sky costs a comparison and a wet one costs a lock per
        // player once every two seconds.
        // --- spilled water, soaking away ---
        //
        // Every tick and not on the rain's clock, because a spill's minute
        // and a half is counted in the ticks it stood; nothing but a length
        // check when nothing has been spilled, which is nearly always.
        {
            let dried = {
                let mut spills = ctx.spills.lock().unwrap_or_else(|e| e.into_inner());
                if spills.is_empty() {
                    Vec::new()
                } else {
                    spills.dry(&*ctx.world, tick_duration.as_secs_f32())
                }
            };
            let changes: Vec<BlockChange> = dried
                .into_iter()
                .map(|((x, y, z), block_id)| {
                    notify_mechanics(&ctx, x, y, z);
                    BlockChange { global_x: x, global_y: y, global_z: z, block_id }
                })
                .collect();
            broadcast_changes(&ctx, changes);
        }

        if rainfall.due(tick_duration.as_secs_f32(), raining) && !handles.is_empty() {
            let around: Vec<(i32, i32, i32)> = handles
                .iter()
                .map(|handle| {
                    let (x, y, z) = handle
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .position;
                    (
                        x.floor() as i32,
                        (y.floor() as i32)
                            .clamp(0, primitive_shared::types::CHUNK_SIZE_Y as i32 - 1),
                        z.floor() as i32,
                    )
                })
                .collect();
            for cell in rainfall.fall(&*ctx.world, &around) {
                // Nothing is broadcast: a cell going from three eighths
                // to four draws identically, so there is nothing for a
                // client to redraw until the water moves -- and when it
                // does, the flow simulation sends it like any other
                // change. What *is* necessary is telling that
                // simulation, or the water the sky just added sits there
                // until something else nearby is disturbed.
                notify_mechanics(&ctx, cell.0, cell.1, cell.2);
            }
        }

        // --- the year, into the water it can see ---
        //
        // Ice that forms in the autumn and opens in the spring. The
        // generator lays only the ice that never thaws
        // (`worldgen::freezes`); this is the rest of it, and the two used to
        // be one number -- which is why a northern lake was frozen solid in
        // a country whose thermometer read twenty degrees. Everything about
        // how it is bounded is in `logic::water::Frost`; what belongs here
        // is that it needs to know where people are and what day it is.
        //
        // The clock is asked first and the positions collected second, on
        // the rain's terms: a tick that is not due costs one comparison.
        if frost.due(tick_duration.as_secs_f32()) && !handles.is_empty() {
            let around: Vec<(i32, i32, i32)> = handles
                .iter()
                .map(|handle| {
                    let (x, y, z) = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
                    (
                        x.floor() as i32,
                        (y.floor() as i32).clamp(0, primitive_shared::types::CHUNK_SIZE_Y as i32 - 1),
                        z.floor() as i32,
                    )
                })
                .collect();
            let world = Arc::clone(&ctx.world);
            let changes = frost.pass(
                &*ctx.world,
                &around,
                ctx.clock.world_days(),
                |x, y, z| world.climate_at(x, y, z).0,
                |z| world.latitude_degrees(z),
            );
            // **Broadcast, unlike the rain.** Water going from three eighths
            // to four draws identically and ice does not: a lid arriving is
            // a cell every client has to redraw. The flow simulation is told
            // as well, because water that has just been uncovered by a thaw
            // has to be looked at again -- it may have somewhere to go.
            for change in &changes {
                notify_mechanics(&ctx, change.global_x, change.global_y, change.global_z);
            }
            broadcast_changes(&ctx, changes);
        }

        // --- the snow, onto the ground it falls on ---
        //
        // A snowfall lays drifts round the players and a thaw takes them
        // back; everything about how is in `logic::snowfall`. Beside the
        // frost because it asks the frost's questions -- where people are,
        // what the climate and the season say -- and the rain's one besides.
        if snowfall.due(tick_duration.as_secs_f32()) && !handles.is_empty() {
            let around: Vec<(i32, i32, i32)> = handles
                .iter()
                .map(|handle| {
                    let (x, y, z) = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
                    (
                        x.floor() as i32,
                        (y.floor() as i32).clamp(0, primitive_shared::types::CHUNK_SIZE_Y as i32 - 1),
                        z.floor() as i32,
                    )
                })
                .collect();
            let world = Arc::clone(&ctx.world);
            let changes = snowfall.pass(
                &*ctx.world,
                &around,
                raining,
                ctx.clock.world_days(),
                |x, y, z| world.climate_at(x, y, z).0,
                |z| world.latitude_degrees(z),
            );
            for change in &changes {
                notify_mechanics(&ctx, change.global_x, change.global_y, change.global_z);
            }
            broadcast_changes(&ctx, changes);
        }

        // --- the rain, onto the boards it falls on ---
        //
        // Boards with open sky over them grey and, over years, rot; see
        // `logic::weathering`. The sky is noted every tick, so the boards
        // start drying the moment it clears; the positions are only
        // collected on a pass. **Mechanics are told**, and that matters here
        // more than for the snow: a board gone rotten holds like earth
        // (`falling::material_looseness`), and the collapse has to look at
        // the roof it is in.
        if weathering.due(tick_duration.as_secs_f32(), raining) && !handles.is_empty() {
            let around: Vec<(i32, i32, i32)> = handles
                .iter()
                .map(|handle| {
                    let (x, y, z) = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
                    (
                        x.floor() as i32,
                        (y.floor() as i32).clamp(0, primitive_shared::types::CHUNK_SIZE_Y as i32 - 1),
                        z.floor() as i32,
                    )
                })
                .collect();
            let changes = weathering.pass(&*ctx.world, &around, ctx.clock.day_length_seconds);
            for change in &changes {
                notify_mechanics(&ctx, change.global_x, change.global_y, change.global_z);
            }
            broadcast_changes(&ctx, changes);
        }

        // Forget the three per-player notes above about anyone who has
        // left. **A player id is never reused** -- `Registry::allocate_id`
        // is a counter -- so an entry for somebody who logged out can
        // never be read again, and nothing here ever removed one: the
        // maps grew by three entries per session for the life of the
        // process. Invisible on a server that is restarted daily and a
        // slow leak on one that is not.
        //
        // Guarded by the length rather than run every tick, so the
        // common case -- nobody left this tick -- is a comparison. The
        // sweep costs one pass over the maps, on the tick after a
        // departure and no other.
        if billed_position.len() > handles.len() {
            let online: std::collections::HashSet<PlayerId> =
                handles.iter().map(|handle| handle.id).collect();
            billed_position.retain(|id, _| online.contains(id));
            billed_grounded.retain(|id, _| online.contains(id));
            sent_entities.retain(|id, _| online.contains(id));
            was_submerged.retain(|id, _| online.contains(id));
        }

        // --- rafts, and whoever is standing on them ---
        //
        // Before the items and long before the snapshots: a rider's world
        // position is worked out here from their raft (see
        // `PlayerRuntime::aboard` and `rafts::tick`), so every snapshot sent
        // below has the rider and the raft at the same tick.
        let rafted = rafts::tick(&ctx, &handles, tick_duration.as_secs_f32());
        for (id, (dx, dy, dz)) in &rafted.carried {
            // **Riding is not walking.** The ground a rider covers on a
            // moving deck is the raft's, and billing it as a stride would
            // make a passenger at five blocks a second a sprinter, starving
            // for standing still. Moved by the carriage, the billed position
            // leaves only what they walked on the deck.
            if let Some(billed) = billed_position.get_mut(id) {
                billed.0 += dx;
                billed.1 += dy;
                billed.2 += dz;
            }
        }

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
                    // What was actually taken, collected here and
                    // announced *after* both locks are let go. A mod
                    // called from inside `collect_near` would be a mod
                    // called while the item store and somebody's pack
                    // are both held, which is the one thing this
                    // subsystem forbids -- and is why
                    // `Event::ItemPickedUp` is not cancellable.
                    let mut taken_stacks: Vec<(primitive_shared::types::BlockId, u32)> =
                        Vec::new();
                    {
                        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        items.collect_near(handle.id, feet, now, |block, count, damage| {
                            // Whatever does not fit stays on the ground.
                            // `add_worn`, not `add`: picking a thing up
                            // must not change any fact about it, and
                            // wear is a fact about it.
                            let left = state.inventory.add_worn(block, count, damage);
                            let taken = count - left;
                            if taken > 0 {
                                state.inventory_dirty = true;
                                took_something = true;
                                taken_stacks.push((block, taken));
                            }
                            taken
                        });
                    }
                    if took_something {
                        send_inventory(handle);
                        for (block, count) in taken_stacks {
                            item_picked_up(&ctx, handle.id, block, count);
                        }
                    }
                }
            }
        }

        // Where everybody is, sampled once and reused by the three
        // things that need it: the animals, which decide what to run
        // from; the growth sample, which walks out from players; and
        // nothing else, because everything else already has the
        // handles.
        // ...and who is carrying a lit torch, gathered under the same
        // lock: a wolf keeps off a flame in a hand as it keeps off a
        // hearth on the ground (`Animals::carrying_fire`), and the only
        // place the held item is known is the player's state.
        let mut fire_bearers: Vec<PlayerId> = Vec::new();
        // ...and what each of them is doing that an ear or an eye would
        // notice -- facing, working, jumping, sitting, bleeding -- for the
        // same reason and under the same lock: see `animals::PlayerSign`.
        let mut player_signs: Vec<logic::animals::PlayerSign> = Vec::new();
        // ...and the state each would cook in, for whatever comes off a
        // fire they are standing at (`smelting::cook_at`). Read here,
        // under the player's own lock, because the hearths are stepped
        // under the fires and the chests, and the player lock is never
        // taken inside those.
        let mut cooks: Vec<logic::smelting::Cook> = Vec::new();
        let where_everyone_is: Vec<(PlayerId, (f32, f32, f32))> = handles
            .iter()
            .map(|h| {
                let state = h.state.lock().unwrap_or_else(|e| e.into_inner());
                if state
                    .inventory
                    .block_in(state.selected_slot)
                    .is_some_and(primitive_shared::types::is_lit_torch)
                {
                    fire_bearers.push(h.id);
                }
                let now = std::time::Instant::now();
                let lately = |at: Option<std::time::Instant>| {
                    at.is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(1))
                };
                player_signs.push(logic::animals::PlayerSign {
                    who: h.id,
                    facing: state.yaw,
                    working: state.digging_until.is_some_and(|until| until > now)
                        || lately(state.last_edit)
                        || lately(state.last_swing),
                    airborne: !state.on_ground && !state.flying,
                    low: state.sitting_on.is_some() || state.sleeping_in.is_some() || state.rowing.is_some(),
                    wounded: state.vitals.health() < logic::survival::MAX_HEALTH * 0.5,
                    held: state.inventory.block_in(state.selected_slot),
                    reek: state.equipment.reek(),
                });
                // At the hearth, which is the station, with the hands a
                // cook uses: see `quality::Maker::craft` for why no tool
                // is `1.0` here.
                cooks.push(logic::smelting::Cook {
                    at: primitive_shared::geometry::narrow(state.position),
                    maker: maker_of(&state, true, 1.0, None),
                });
                (h.id, primitive_shared::geometry::narrow(state.position))
            })
            .collect();

        // --- fires, and what grows ---
        //
        // Both hand back block changes, and both go out through the same
        // batching path the falling sand uses -- one message per chunk
        // per tick, however many cells changed.
        {
            let dt = tick_duration.as_secs_f32();
            // **Kept apart until they have been announced.** Both end up
            // in the same batch on the wire -- one message per chunk,
            // however many cells changed -- but a fire going out and a
            // bush filling again are two different events, and merging
            // the lists first would leave nothing to tell them apart by.
            // Every change `Fires::step` returns is a fire that has just
            // burnt through, and every change `Growth::step` returns is
            // something that grew; see both.
            let fire_changes = {
                let mut fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                fires.step(&*ctx.world, dt, simulation::DEFAULT_TICK_BUDGET)
            };
            // **The night passes when everybody is in bed, and only
            // then.** One sleeper on a server of five does not decide
            // what time it is for the other four -- that is the rule
            // every game of this shape settles on, and it settles on it
            // because the alternative is a player being marched into
            // morning by somebody they have never met.
            //
            // Singleplayer is the same code with one player in it,
            // which is the whole reason it is written this way: the
            // common case is not a special case.
            //
            // The hours skipped are *charged for*, not skipped over --
            // see `sleep_through_to_dawn` -- and they are skipped behind
            // every sleeper's closed eyes, never in front of them: see
            // `night_may_pass`.
            if night_may_pass(&handles) {
                sleep_through_to_dawn(&ctx, &handles);
            }

            // A crop's air is the climate's, and the climate reads the fire
            // map (a fire beside a field warms it) -- so this step holds
            // two locks. The weather is read first and let go at once; the
            // fire map is taken *before* the growth queue, which is the
            // order the `/stats` line takes them in as well. A second order
            // anywhere is a deadlock waiting for a busy tick.
            let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
            let growth_changes = {
                let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                let mut growth = ctx.growth.lock().unwrap_or_else(|e| e.into_inner());
                growth.watch(where_everyone_is.iter().map(|&(_, at)| at).collect());
                let soil = WorldSoil {
                    world: &ctx.world,
                    fires: &fires,
                    world_days: ctx.clock.world_days(),
                    weather,
                };
                growth.step(&*ctx.world, &soil, dt, simulation::DEFAULT_TICK_BUDGET)
            };
            // The carcasses, on the same budget: this only reconciles
            // the queue of changed cells -- a kill joining the list and
            // a butchered one leaving it. The *ageing* is on the rot
            // clock, four times a day, in `rot::Rot::pass`.
            {
                let mut carrion = ctx.carrion.lock().unwrap_or_else(|e| e.into_inner());
                carrion.reconcile(simulation::DEFAULT_TICK_BUDGET, |at| {
                    ctx.world.cached_block(at.0, at.1, at.2)
                });
            }
            // ...and the traps, on the same budget and the same terms: a
            // trap set joins the list here, and the filling is on the rot
            // clock (`rot::Rot::pass`).
            ctx.fishing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reconcile(simulation::DEFAULT_TICK_BUDGET, |at| ctx.world.cached_block(at.0, at.1, at.2));
            for change in &fire_changes {
                fire_died(
                    &ctx,
                    (change.global_x, change.global_y, change.global_z),
                );
            }
            for change in &growth_changes {
                growth_step(
                    &ctx,
                    (change.global_x, change.global_y, change.global_z),
                    change.block_id,
                );
            }
            let mut changes = fire_changes;
            changes.extend(growth_changes);
            broadcast_changes(&ctx, changes);

            // ...and the fires in the ground, on their own hour. See
            // `step_pits`.
            step_pits(&ctx, f64::from(dt));

            // ...and the lines in the water. See `step_fishing`.
            step_fishing(&ctx, dt);

            // ...and what the hearths set alight, the smoke they fill a
            // room with and the soot they lay on its ceiling. See
            // `step_wildfire`.
            step_wildfire(&ctx, &where_everyone_is, dt);

            // ...and what the lit ones are cooking. After the fires
            // rather than with them: a hearth that has just gone out
            // this tick should not also finish a batch on it.
            //
            // The two locks are taken here and only here, in this
            // order, which is what keeps the lock order in this file a
            // fact rather than a hope.
            let cooked = {
                let mut fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
                let mut smelting = ctx.smelting.lock().unwrap_or_else(|e| e.into_inner());
                let world = Arc::clone(&ctx.world);
                smelting.step(
                    &mut fires,
                    &mut chests,
                    move |at| world.cached_block(at.0, at.1, at.2),
                    &cooks,
                    dt,
                )
            };
            // Three lists rather than one: a hearth that drew a log is
            // sent to its watchers but is not a finished smelt, and a
            // hearth whose fire merely got hotter is sent so its gauge
            // moves -- see `smelting::Stepped`.
            for at in cooked.changed.iter().chain(&cooked.warming) {
                broadcast_chest_state(&ctx, *at);
            }
            for at in cooked.finished {
                smelting_finished(&ctx, at);
            }

            // ...and the hides on the racks, on the same tick and out of
            // the same two locks. Stepped on its own coarse interval
            // inside `Drying::step` rather than here, because twelve
            // minutes of curing does not need a twentieth-of-a-second
            // clock -- see `logic::drying`.
            //
            // The weather and the hour are read once and handed in, so
            // the racks and the players are looking at the same sky.
            let stepped = {
                let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
                let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
                let mut racks = ctx.drying.lock().unwrap_or_else(|e| e.into_inner());
                racks.step(
                    &ctx.world,
                    &fires,
                    &mut chests,
                    weather,
                    ctx.clock.world_days(),
                    dt,
                )
            };
            // **The bar has to move on its own.** A rack's contents do
            // not change for twelve minutes, so a screen told only about
            // changes to them would show one photograph for the whole of
            // the wait -- and the thing a player opened it to find out is
            // exactly what is happening in between. Once every step (two
            // seconds), and only to whoever has that rack open: see the
            // guard at the top of `broadcast_chest_state`.
            for at in stepped.advanced {
                broadcast_chest_state(&ctx, at);
                // The frame empties itself when the last skin is banked,
                // and the block has to stop showing one.
                refresh_rack_block(&ctx, at);
            }
            // ...and the peat set down on the ground, in the same sky. A sod
            // that turned -- half dried, back to wet in the rain, into a
            // brick -- is a different thing lying there, and everybody near
            // is told what (`tell_set_down`); the bar between moves nothing.
            let turned = {
                let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
                let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
                let mut sods = ctx.peat.lock().unwrap_or_else(|e| e.into_inner());
                sods.step(&ctx.world, &fires, &mut chests, weather, ctx.clock.world_days(), dt)
            };
            for at in turned {
                tell_set_down(&ctx, at);
            }
            // ...and the walls drying in it, which change what the cell *is*:
            // a lift dry enough for the next, or daub washed off its rods.
            // Written as any edit is, so a washed lift that leaves air lets
            // the sand over it come down.
            let walls_changed = {
                let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
                let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                let mut wet = ctx.walls.lock().unwrap_or_else(|e| e.into_inner());
                wet.step(&ctx.world, &fires, weather, ctx.clock.world_days(), dt)
            };
            for (at, now) in walls_changed {
                if ctx.world.set_block(at.0, at.1, at.2, now) {
                    broadcast_block(&ctx, at, now);
                    ctx.falling.lock().unwrap_or_else(|e| e.into_inner()).on_block_changed(at.0, at.1, at.2);
                    notify_mechanics(&ctx, at.0, at.1, at.2);
                }
            }
            for at in stepped.finished {
                fire_plugin_hook(
                    &ctx,
                    "on_hide_cured",
                    vec![
                        plugins::Value::Int(at.0 as i64),
                        plugins::Value::Int(at.1 as i64),
                        plugins::Value::Int(at.2 as i64),
                    ],
                    None,
                );
            }
        }

        // --- the food, going off ---
        //
        // Four times a game day, every pack, chest and drop. The packs
        // are sent from inside; a chest that aged has to reach whoever
        // is standing at it. See `logic::rot`.
        for at in rot.pass(&ctx, tick_duration.as_secs_f32()) {
            broadcast_chest_state(&ctx, at);
        }

        // --- the vermin ---
        //
        // After the food's own clock and before the animals move, so a rat
        // that raided a chest this tick is standing where it was standing
        // when it did. Three clocks, all slow (`logic::vermin`): the map of
        // where people live is warmed every tick and aged every ten
        // seconds, somewhere to put a rat is looked for every thirty, and
        // the containers near one are gone through every twenty.
        let raided = {
            let mut vermin = ctx.vermin.lock().unwrap_or_else(|e| e.into_inner());
            step_vermin(&ctx, &mut vermin, &where_everyone_is, tick_duration.as_secs_f32())
        };
        for at in raided {
            broadcast_chest_state(&ctx, at);
        }

        // --- animals ---
        //
        // Stepped after the sky and the fires and before the snapshots,
        // so what is replicated this tick is where they actually are
        // rather than where they were. The blows come back as data for
        // the same reason a mechanic's changes do: what a hit *does* to
        // a player -- the death screen, the body left behind, the plugin
        // hook -- is the tick loop's business and not the boar's.
        {
            // The rafts' wind (`raft::wind`), read with the sky's lock let go
            // before the animals' is taken: a hunter who has watched the smoke
            // lean knows which side to come at a deer from.
            let (wind, raining) = {
                let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
                (
                    primitive_shared::raft::wind(ctx.clock.world_days(), weather).vector(),
                    weather != primitive_shared::weather::Weather::Clear,
                )
            };
            let (blows, births, deaths, fallen, staked, dung, heavy) = {
                let mut animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
                animals.carrying_fire(std::mem::take(&mut fire_bearers));
                animals.player_signs(std::mem::take(&mut player_signs));
                animals.feel_wind(wind);
                animals.rain(raining);
                // The calendar the young grow by and the spring they are
                // born in. See `Animals::calendar`.
                animals.calendar(ctx.clock.world_days());
                let blows = animals.step(
                    &*ctx.world,
                    &where_everyone_is,
                    tick_duration.as_secs_f32(),
                    ctx.clock.time_of_day(),
                );
                // Drained with the same lock that produced them and
                // announced without it -- see `Animals::take_births`.
                (
                    blows,
                    animals.take_births(),
                    animals.take_deaths(),
                    animals.take_fallen(),
                    animals.take_staked(),
                    animals.take_dung(),
                    animals.heavy_feet(),
                )
            };
            // **A pit's cover gives way under a deer, and under a person**
            // (`pitfall`): every heavy animal and every player, where they
            // stand after the step. Written with the animals' lock let go,
            // for the carcasses' reason.
            collapse_pit_covers(&ctx, heavy.iter().chain(where_everyone_is.iter().map(|(_, at)| at)).copied());
            for at in staked {
                broadcast_staked(&ctx, at);
            }
            // What the kept animals left, written after the lock is gone for
            // the carcasses' reason: `set_block` and the broadcast.
            for at in dung {
                lay_animal_dung(&ctx, at);
            }
            for (entity, at) in births {
                entity_appeared(&ctx, entity, at, true);
            }
            for entity in deaths {
                entity_appeared(&ctx, entity, (0.0, 0.0, 0.0), false);
            }
            // What the world killed rather than anybody's blow -- a fall, a
            // fire, held breath, the last of a spear's poison -- leaves a
            // body the way a spear's kill does. After the lock is gone:
            // `lay_carcass` takes the items and broadcasts.
            //
            // **Every** death comes this way now, a spear's included: the body
            // falls for `animals::FALL_SECONDS` first and the carcass goes
            // where it came to rest. See `Animals::fell`.
            for death in fallen {
                lay_carcass(&ctx, death);
            }
            for blow in blows {
                let Some(victim) = ctx.registry.get(blow.victim) else {
                    continue;
                };
                // What the animal does, rather than one verb for
                // everything alive: a boar gores and a wolf does not
                // have the tusks to.
                //
                // **Every species by name, and no `_` arm.** The catch-all
                // said "gored by a boar" and was wrong from the day the bear
                // arrived: a new hostile animal is exactly when nobody
                // remembers this line, so a new species is a compile error
                // here instead.
                use primitive_shared::animals::Species;
                // The words are the species' own (`Species::death_cause`),
                // because the client reads them back to say them again in
                // the player's language.
                let how = blow.species.death_cause();
                // ...and what the blow leaves on a body, on the same rule:
                // every species by name, so a new hunter is a compile error
                // here rather than an animal whose bite never bleeds. A
                // wolf's teeth cut and never break; a boar's tusk comes in
                // low and bruises what it cuts; a bear or a lion rears and
                // comes down with weight, which is what breaks an arm. See
                // `injury::Blow`.
                use primitive_shared::injury::Blow;
                let wound = match blow.species {
                    Species::Wolf => Blow::Bite,
                    Species::Boar => Blow::Tusk,
                    Species::Bear | Species::Lion => Blow::Claw,
                    Species::Hare
                    | Species::Deer
                    | Species::Sheep
                    | Species::Fowl
                    | Species::Zebra
                    | Species::Antelope
                    | Species::Fish
                    | Species::Cod
                    | Species::Trout
                    | Species::Pike
                    | Species::Herring
                    | Species::Gull
                    | Species::Rat
                    | Species::Horse => Blow::Blunt,
                };
                let outcome = strike_player(&ctx, &victim, blow.damage, how, wound);
                if !matches!(outcome, survival::Outcome::Unchanged) {
                    report_vitals(&ctx, &victim, outcome);
                }
            }
        }

        // --- riders, on the horses that have just moved ---
        //
        // After the animals and before the snapshots, for the reason the
        // rafts go first (`horses::tick`): the rider and the horse in one
        // snapshot are one instant. What a dead horse carried goes on the
        // ground here too.
        horses::spill(&ctx);
        for (id, (dx, dy, dz)) in horses::tick(&ctx, &handles) {
            // **Riding is not walking**, the raft's rule again: the ground a
            // horse covers is not a stride the rider is billed hunger for.
            if let Some(billed) = billed_position.get_mut(&id) {
                billed.0 += dx;
                billed.1 += dy;
                billed.2 += dz;
            }
        }

        // Falling blocks, dropped items and animals, bucketed the same
        // way the players are: built once per tick, queried once per
        // player, so the cost is what is actually near people rather
        // than players x entities. See `EntityGrid`.
        let entity_grid = {
            let sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
            let items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
            let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
            let rafts = ctx.rafts.lock().unwrap_or_else(|e| e.into_inner());
            let states: Vec<primitive_shared::protocol::EntityState> = sim
                .entities()
                .iter()
                .map(|e| e.state())
                .chain(items.states())
                .chain(animals.states())
                .chain(rafts.states())
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

                // Entities get the same interest filtering as players.
                //
                // **And an explicit empty set when the last one leaves**,
                // which the players' snapshots do not need and this does.
                // The old contract was "a client drops whatever it stops
                // hearing about", with a 150 ms timeout on the client
                // doing the dropping -- so removal and *silence* were the
                // same signal, and the timeout had to be short enough to
                // make a picked-up item vanish promptly. Three dropped
                // messages in a row then blinked a dropped block out of
                // existence, and chunk data shares this queue, so a
                // player streaming terrain saw exactly that.
                //
                // One empty message when the set empties separates the
                // two: removal is a snapshot that omits the thing, and
                // the client's timeout goes back to being a backstop for
                // a dead connection. It costs one message per player per
                // time the last nearby entity leaves, which is nothing.
                entity_grid.nearby(origin, radius, &mut visible_entities);
                let had_entities = sent_entities.insert(handle.id, !visible_entities.is_empty());
                if !visible_entities.is_empty() || had_entities == Some(true) {
                    handle.send(ServerMessage::Entities {
                        tick,
                        states: visible_entities.clone(),
                    });
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
                        position.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT),
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
                    // The rule itself lives in `head_under_water` now,
                    // because `PlayersApi::is_submerged` asks the same
                    // question and two copies of the twelve per cent at
                    // the top of a full cell is exactly the sort of
                    // duplicate that took a player drowning on the sea
                    // floor to find the first time.
                    let head_under = head_under_water(&ctx, primitive_shared::geometry::narrow(position));
                    // ...and whether the water is past the waist, which is
                    // what everybody else draws them by. See
                    // `Posture::Swimming`.
                    let swimming = waist_in_water(&ctx, position);
                    handle.state.lock().unwrap_or_else(|e| e.into_inner()).swimming = swimming;
                    // The crossing, either way, for the mods. Recorded
                    // before anything else happens with it so that a
                    // player who drowns this tick still gets the
                    // "went under" they earned.
                    let before = was_submerged.insert(handle.id, head_under);
                    // A player nobody has looked at yet crossed nothing.
                    // Their first reading is where they are standing,
                    // not a change -- and somebody who joins already in
                    // a lake did not *enter* it. A mod that cares asks
                    // `PlayersApi::is_submerged` on `PlayerJoined`.
                    if before.is_some_and(|was| was != head_under) {
                        player_water_line(&ctx, handle.id, eye_cell, head_under);
                    }
                    // **Standing in a fire burns, and so does standing on
                    // one.** The feet's cell and the eye's, because a
                    // campfire is a quarter of a cell tall; and the cell
                    // under the feet, because a burning pit fills its
                    // cell and a player on it is in the air above the
                    // fire. See `survival::touches_fire`.
                    let in_fire = survival::touches_fire(primitive_shared::geometry::narrow(position), &[(eye.1 - position.1) as f32], |x, y, z| {
                        ctx.world.cached_block(x, y, z)
                    });
                    // **The torch burns down**, and it burns in seconds
                    // rather than in ticks: the counter is hundredths of
                    // a second (see `types::TORCH_LIFE`), so the same wad
                    // lasts the same forty-five seconds on a server
                    // ticking at twenty hertz and on one ticking at
                    // five. A torch measured in ticks would be a
                    // different item on every host.
                    //
                    // Only the torch actually in the hand. One in the
                    // pack is a torch nobody is holding to the dark, and
                    // burning it there would mean a player's spare
                    // torches went out while they walked.
                    {
                        let (slot, went_out) = {
                            let mut state =
                                handle.state.lock().unwrap_or_else(|e| e.into_inner());
                            let slot = state.selected_slot;
                            // Hundredths, so a twenty-hertz tick is
                            // exactly five of them and nothing has to be
                            // carried over -- see `types::TORCH_LIFE`.
                            let steps = (tick_duration.as_secs_f32() * 100.0) as u32;
                            let burn = state.inventory.burn_torch(slot, steps);
                            (slot, burn == primitive_shared::inventory::TorchBurn::WentOut)
                        };
                        if went_out {
                            held_slot_changed(
                                &ctx,
                                handle.id,
                                slot,
                                Some(primitive_shared::types::BLOCK_TORCH_SPENT),
                            );
                        }
                    }
                    // Read before the player's lock is taken: the wildfire's
                    // lock is never held under it.
                    let smoke = ctx.wildfire.lock().unwrap_or_else(|e| e.into_inner()).smoke_at(eye_cell);
                    let drowning = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let burned = state.vitals.burn(in_fire, tick_duration.as_secs_f32());
                        if !matches!(burned, survival::Outcome::Unchanged) {
                            drop(state);
                            report_vitals(&ctx, handle, burned);
                            state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        }
                        // **The smoke of a fire in a closed room** is breathed
                        // where water is not. See `Vitals::breathe_smoke`.
                        if head_under {
                            state.vitals.breathe(true, tick_duration.as_secs_f32())
                        } else {
                            state.vitals.breathe_smoke(smoke, tick_duration.as_secs_f32())
                        }
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
                    // ...and the smoke, on the same terms: a twentieth is
                    // under a shade of the fog, and the last reading is
                    // always nought once the room clears.
                    let smoke_news = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let moved = (smoke - state.smoke_reported).abs() > 0.05;
                        let cleared = smoke == 0.0 && state.smoke_reported != 0.0;
                        if moved || cleared {
                            state.smoke_reported = smoke;
                            Some(smoke)
                        } else {
                            None
                        }
                    };
                    if let Some(thickness) = smoke_news {
                        handle.send(ServerMessage::Smoke { thickness });
                    }
                    // The way back: a body emptied, broken or burnt comes
                    // off the map. See `forget_recovered_bags`.
                    forget_recovered_bags(&ctx, handle);
                }

                // --- warmth, and the water it costs ---
                //
                // Before hunger, because being cold is what makes a
                // player hungry (`body::shiver_hunger_multiplier`) and
                // being hot is what makes them thirsty
                // (`body::thirst_multiplier`) -- so the temperature has
                // to be this tick's before either bill is worked out.
                //
                // The *world* half of it is resampled on a slow interval
                // and held in between; the body keeps drifting toward
                // whatever the last sample said, every tick. See
                // `logic::climate` for why that is not a compromise.
                let (ambient, worn, wetness) = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.since_ambient += tick_duration.as_secs_f32();
                    let due = state.since_ambient >= climate::SAMPLE_INTERVAL_SECS;
                    let worn = state.equipment.worn();
                    if due {
                        let position = state.position;
                        // The lock on the player is dropped before the
                        // fire map is taken, and taken again after.
                        // Every other path in this file that touches
                        // both takes them in this order, and that is
                        // what keeps the lock order in this server a
                        // fact rather than a hope.
                        state.since_ambient = 0.0;
                        drop(state);
                        let weather =
                            ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
                        // **The room, with the shelters' lock let go before
                        // the fires' is taken** and taken again after: the
                        // lock order stays the fact the note above says it
                        // is. See `logic::shelters` for why the room is
                        // cached and the warmth is the room's.
                        let days = ctx.clock.world_days();
                        let wind = primitive_shared::raft::wind(days, weather);
                        let feet_cell = (
                            position.0.floor() as i32,
                            position.1.floor() as i32,
                            position.2.floor() as i32,
                        );
                        let (room, afterglow) = {
                            let mut shelters = ctx.shelters.lock().unwrap_or_else(|e| e.into_inner());
                            let room = shelters.room_at(&ctx.world, feet_cell);
                            let afterglow = room.as_deref().map_or(0.0, |r| shelters.warmth(r, f64::from(days)));
                            (room, afterglow)
                        };
                        let (sampled, reading, hearth) = {
                            let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                            climate::Ambient::of_player(
                                &ctx.world,
                                &fires,
                                primitive_shared::geometry::narrow(position),
                                days,
                                weather,
                                room.as_deref().map(|room| climate::Indoors { room, wind, afterglow }),
                            )
                        };
                        if let Some(room) = &room {
                            ctx.shelters
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .warmed(room, wind, hearth, f64::from(days));
                        }
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.ambient = sampled;
                        if state.shelter_reported.is_none_or(|told| told.differs(&reading)) {
                            state.shelter_reported = Some(reading);
                            handle.send(ServerMessage::Shelter { reading });
                        }
                        let wetness = sampled.step_wetness_dressed(
                            state.vitals.wetness(),
                            climate::SAMPLE_INTERVAL_SECS,
                            worn.shed_rain,
                        );
                        // **The pack gets wet with the body** (`wet`): at
                        // once in a swim, once the coat is through in the
                        // rain, and dry again after a while by a fire or in
                        // the sun. Decided on the sample, where the rain and
                        // the water are already known, and sent only when a
                        // slot changed -- which is almost never.
                        let (weather, drying) = primitive_shared::wet::pack_weather(
                            sampled.swimming,
                            sampled.rained_on,
                            wetness,
                            sampled.near_fire,
                            sampled.sun_c > 0.0,
                            state.pack_drying,
                            climate::SAMPLE_INTERVAL_SECS,
                        );
                        state.pack_drying = drying;
                        if primitive_shared::wet::weather_inventory(&mut state.inventory, weather) {
                            state.inventory_dirty = true;
                            let slot = state.selected_slot;
                            let held = state.inventory.block_in(slot);
                            drop(state);
                            held_slot_changed(&ctx, handle.id, slot, held);
                            send_inventory(handle);
                            state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        }
                        (sampled, worn, wetness)
                    } else {
                        (state.ambient, worn, state.vitals.wetness())
                    }
                };
                let exposure = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.warm(
                        ambient.exposure(),
                        worn.insulation,
                        worn.shade,
                        wetness,
                        tick_duration.as_secs_f32(),
                    )
                };
                if !matches!(exposure, survival::Outcome::Unchanged) {
                    report_vitals(&ctx, handle, exposure);
                }

                // Hunger, before regeneration and not after: an empty
                // stomach is what *stops* a wound closing, and the two
                // running the other way round would heal a starving
                // player for one tick out of every one.
                //
                // What the player was doing is read off two things the
                // server already knows -- how far they moved since the
                // last tick, and whether they finished a block edit
                // recently. Neither is something a client asserts, which
                // is the whole point: a "I am not sprinting" flag is a
                // flag a cheat client never sets.
                let starving = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let moved = billed_position
                        .get(&handle.id)
                        .map(|&(x, _, z)| {
                            let (dx, dz) = (state.position.0 - f64::from(x), state.position.2 - f64::from(z));
                            (dx * dx + dz * dz).sqrt()
                        })
                        .unwrap_or(0.0);
                    let rose = billed_position
                        .get(&handle.id)
                        .is_some_and(|&(_, y, _)| state.position.1 > f64::from(y + 0.05));
                    let was_grounded = billed_grounded
                        .insert(handle.id, state.on_ground)
                        .unwrap_or(true);
                    if was_grounded && !state.on_ground && rose {
                        state.vitals.jumped();
                    }
                    billed_position.insert(handle.id, primitive_shared::geometry::narrow(state.position));
                    let effort = survival::Effort {
                        // Faster than a walk can only be a sprint. The
                        // threshold sits between the two rather than at
                        // the sprint speed itself, so a player shoved
                        // downhill by their own momentum is not billed
                        // for running.
                        sprinting: moved / f64::from(tick_duration.as_secs_f32()) > f64::from(SPRINT_THRESHOLD),
                        mining: state.last_edit.is_some_and(|at| {
                            at.elapsed() < std::time::Duration::from_secs(1)
                        })
                            // **Rowing is work**, billed as the work a pick
                            // is: the oars' pace costs food, and the sail's
                            // does not -- which is half of why a sail is
                            // worth four leathers. Read off the server's own
                            // oars, not anything the rower claims about them.
                            || rafted.pulling.contains(&handle.id),
                        // **Swimming is work, and the server decides
                        // whether it is happening.** Read off the
                        // server's own copy of the world at the
                        // server's own copy of the position -- the same
                        // question the breath clock asks a few lines
                        // down -- because a client that said "I am not
                        // swimming" would otherwise cross an ocean for
                        // nothing. See `survival::Effort::swimming`.
                        swimming: head_under_water(&ctx, primitive_shared::geometry::narrow(state.position)),
                    };
                    state.vitals.digest(effort, tick_duration.as_secs_f32())
                };
                if !matches!(starving, survival::Outcome::Unchanged) {
                    report_vitals(&ctx, handle, starving);
                }
                let food_changed = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.needs_food_report()
                };
                if food_changed {
                    send_nourishment(handle);
                }

                // --- thirst ---
                //
                // The same `Effort` hunger was just billed against, in
                // the vocabulary `body` uses. Two names for one fact
                // rather than two measurements: the tick loop worked out
                // what this player was doing once, above, and neither
                // meter gets to disagree with the other about it.
                let parched = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let moved = billed_position
                        .get(&handle.id)
                        .map(|&(x, _, z)| {
                            let (dx, dz) = (state.position.0 - f64::from(x), state.position.2 - f64::from(z));
                            (dx * dx + dz * dz).sqrt()
                        })
                        .unwrap_or(0.0)
                        / f64::from(tick_duration.as_secs_f32());
                    let exertion = primitive_shared::body::Exertion {
                        moving: moved > 0.6,
                        sprinting: moved > f64::from(SPRINT_THRESHOLD),
                        working: state.last_edit.is_some_and(|at| {
                            at.elapsed() < std::time::Duration::from_secs(1)
                        }),
                    };
                    state
                        .vitals
                        .drink_down(exertion, tick_duration.as_secs_f32())
                };
                if !matches!(parched, survival::Outcome::Unchanged) {
                    report_vitals(&ctx, handle, parched);
                }

                // **Tiredness, and the three ways it moves.** Asleep
                // it drains fast and the player is pinned; sitting on
                // a stool it trickles off while they stay there; and
                // otherwise it fills, slowly, all day. One place, so
                // the three can never disagree about which is
                // happening. See `body::WAKING_SECONDS`.
                let (tired, get_up) = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let dt = tick_duration.as_secs_f32();
                    // A sleeper whose bed has gone -- broken, burnt,
                    // dug out from under them -- is a player lying in a
                    // hole. They wake, which is also what stops
                    // `sleeping_in` outliving the block it names.
                    let bed_gone = state.sleeping_in.is_some_and(|at| {
                        ctx.world
                            .cached_block(at.0, at.1, at.2)
                            .and_then(primitive_shared::body::Rest::of)
                            .is_none()
                    });
                    // ...and so does one that something is hitting.
                    let hurt = state.vitals.last_damage_elapsed() < 1.0;
                    let woken = state.sleeping_in.is_some() && (bed_gone || hurt);
                    // A player who walked off their stool, or whose stool
                    // was broken under them, is not sitting on it any
                    // more. The distance is a stride's worth rather than
                    // the block and a quarter it used to be: the server
                    // puts a sitter exactly on the seat, so any real step
                    // is getting up, and the old allowance kept a player
                    // resting while they worked at the bench beside it.
                    // What is under them, read once: whether it is still a
                    // seat decides whether they are still sitting, and
                    // which seat decides how fast they rest.
                    let seat_block = state
                        .sitting_on
                        .and_then(|seat| ctx.world.cached_block(seat.0, seat.1, seat.2));
                    let risen = state.sitting_on.is_some_and(|seat| {
                        let (dx, dy, dz) = (
                            state.position.0 - f64::from(seat.0 as f32 + 0.5),
                            state.position.1 - f64::from(seat.1 as f32),
                            state.position.2 - f64::from(seat.2 as f32 + 0.5),
                        );
                        // Any seat, not the stool by name: a chair checked
                        // against `BLOCK_STOOL` here stood its sitter up on
                        // the first tick, as if it had been broken.
                        let gone = !seat_block.is_some_and(primitive_shared::types::is_seat);
                        gone || dx * dx + dz * dz > 0.6 * 0.6 || dy.abs() > 1.0
                    });
                    let sleeping = state.sleeping_in.filter(|_| !woken);
                    let sitting = state.sitting_on.is_some() && !risen;
                    // Both are stood up below, once the lock is let go:
                    // `stand_up` has to look at the world to find a place.
                    let get_up = woken || risen;
                    let outcome = if let Some(rest) = sleeping.and_then(|at| {
                        ctx.world
                            .cached_block(at.0, at.1, at.2)
                            .and_then(primitive_shared::body::Rest::of)
                    }) {
                        let floor = 1.0 - rest.recovery();
                        state.vitals.rest(
                            dt,
                            primitive_shared::body::SLEEP_RECOVERY_PER_SECOND,
                            floor,
                        )
                    } else if sitting {
                        // The seat decides the rate: a chair rests half
                        // again as fast as a stool (`body::sitting_recovery`).
                        state.vitals.rest(
                            dt,
                            primitive_shared::body::sitting_recovery(seat_block.unwrap_or(0)),
                            0.0,
                        )
                    } else {
                        state.vitals.tire(dt)
                    };
                    (outcome, get_up)
                };
                if get_up {
                    stand_up(&ctx, handle, None);
                }
                // **Not reported here.** Tiredness is a gauge, and it
                // rides with the other two on `send_body` below -- see
                // `Vitals::needs_body_report`, which explains what
                // sending it as a health event cost. What this arm is
                // for is the *deaths*: `rest` and `tire` cannot kill
                // anybody, so an outcome that is not `Unchanged` is
                // simply a number that moved.
                let _ = tired;

                // **Wounds bleed and mend here**, six times as fast in a
                // bed -- which is what makes sleep the second half of the
                // answer to a cut or a break. Beside the tiredness because
                // both ask the same question of the same tick: is this
                // player lying down?
                //
                // **The outcome goes to `report_vitals`.** Bleeding can
                // kill, and a `Died` dropped here is a player at zero
                // walking about -- the shape of the bug that once made a
                // starving sleeper immortal. A `Changed` is only worth a
                // packet when the bar has visibly moved, on the rule
                // `needs_report` keeps for everything else: a cut bleeds a
                // crumb every tick.
                //
                // It used to say "your leg has mended" in the chat. The
                // client says it now, in the player's language, off the
                // `Injuries` it is sent.
                {
                    let (outcome, bar_moved, drip) = {
                        let mut state =
                            handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        let asleep = state.sleeping_in.is_some();
                        let outcome = state.vitals.mend(tick_duration.as_secs_f32(), asleep);
                        // ...and what an open cut sheds, a drop at a time,
                        // from the hip rather than the chest: a drip runs
                        // down before it falls. See `Vitals::drips`.
                        let drops = state.vitals.drips(tick_duration.as_secs_f32());
                        let (x, y, z) = state.position;
                        let drip = (drops > 0).then_some(((x, y + 0.9, z), drops));
                        (outcome, state.vitals.needs_report(), drip)
                    };
                    if let Some((at, drops)) = drip {
                        broadcast_blood(&ctx, at, drops);
                    }
                    match outcome {
                        survival::Outcome::Died { .. } => report_vitals(&ctx, handle, outcome),
                        survival::Outcome::Changed if bar_moved => {
                            report_vitals(&ctx, handle, outcome)
                        }
                        _ => {}
                    }
                    send_injuries(handle);
                }

                // Whatever the block above decided, the client is told
                // if it changed -- including the two involuntary
                // wakings, which are the cases where a silent server
                // would leave a player's controls dead.
                send_asleep(handle);

                // ...and what bad water is still taking. Beside the
                // thirst because it *is* the thirst's other half: what
                // a player drank decides both how full they are and
                // how ill. See `Vitals::sicken`.
                //
                // **Told when the bar has visibly moved**, on the rule the
                // bleeding above keeps. It was a `Health` message every tick
                // of an illness -- twenty a second for the three quarters of
                // a minute a raw fish lasts -- and the client drew each one
                // as a blow: a burst of blood at the chest, a kick of the
                // camera and the hurt sound. That was the fountain a player
                // saw after eating a fish. The blood belongs to blows and
                // cuts now (`ServerMessage::Blood`), and a crumb of health
                // nobody can see is not worth a packet until it is a
                // twentieth of a point.
                let (ill, bar_moved) = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let ill = state.vitals.sicken(tick_duration.as_secs_f32());
                    (ill, state.vitals.needs_report())
                };
                match ill {
                    survival::Outcome::Died { .. } => report_vitals(&ctx, handle, ill),
                    survival::Outcome::Changed if bar_moved => report_vitals(&ctx, handle, ill),
                    _ => {}
                }

                // --- comfort ---
                //
                // **After everything it reads and before the healing it
                // scales**: the temperature, the tiredness, the wetness and
                // the wounds are this tick's by here, and `regenerate` below
                // is the one place the value is spent on the server. See
                // `primitive_shared::comfort`.
                //
                // The place is looked at every `SURVEY_SECONDS`, off the
                // world with the player's lock let go (the flood fill reads
                // up to a few thousand cells); the body settles every tick.
                {
                    let dt = tick_duration.as_secs_f32();
                    let survey_at = {
                        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.since_survey += dt;
                        let due = state.since_survey >= primitive_shared::comfort::SURVEY_SECONDS
                            && !state.vitals.is_dead();
                        if due {
                            state.since_survey = 0.0;
                        }
                        due.then_some(state.position)
                    };
                    if let Some(position) = survey_at {
                        let feet = (
                            position.0.floor() as i32,
                            position.1.floor() as i32,
                            position.2.floor() as i32,
                        );
                        let place = primitive_shared::comfort::survey(
                            |x, y, z| ctx.world.cached_block(x, y, z),
                            feet,
                        );
                        let due = {
                            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                            state.surroundings = place;
                            // Not in a bed, on a seat or in the air: a body
                            // lying down or mid-jump waits until it is
                            // standing somewhere.
                            state.vitals.goes_now(place.enclosure)
                                && state.on_ground
                                && state.sleeping_in.is_none()
                                && state.sitting_on.is_none()
                        };
                        if due {
                            leave_dung(&ctx, handle, feet);
                        }
                    }
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let place = state.surroundings;
                    let asleep = state.sleeping_in.is_some();
                    // The smoke as last reported rather than read again: it
                    // is within a twentieth of the real thickness, and taking
                    // the wildfire's lock a second time a tick for a number
                    // that settles over twenty seconds buys nothing.
                    let smoke = state.smoke_reported;
                    let (in_water, rained_on) =
                        (state.ambient.in_water, state.ambient.getting_wet && !state.ambient.in_water);
                    state.vitals.settle_comfort(place, asleep, smoke, in_water, rained_on, dt);
                }

                // ...and the two gauges, when either has moved far
                // enough to be worth a packet. See
                // `Vitals::needs_body_report`.
                //
                // ...and the stamina multiplier comfort is worth, past a
                // twentieth: finer than that is a bar filling a hair faster,
                // which nobody sees.
                let body_changed = {
                    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.vitals.needs_body_report(state.body_reported)
                        || (state.vitals.recovery() - state.recovery_reported).abs() > 0.05
                };
                if body_changed {
                    send_body(handle);
                }

                // The worn set, if a blow or a save changed it since the
                // last tick. Almost always free: `send_equipment`
                // returns immediately unless the dirty flag is set.
                send_equipment(handle);

                let (outcome, regained) = {
                    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                    let before = state.vitals.health();
                    let outcome = state.vitals.regenerate(tick_duration.as_secs_f32());
                    (outcome, state.vitals.health() - before)
                };
                // The mods hear about every heal, this one included --
                // see `heal_player`, which is the other half. Outside
                // the guard block, because nothing may call into a mod
                // with a lock held.
                if regained > 0.0 {
                    player_healed(&ctx, handle.id, regained);
                }
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
                let world_days = ctx.clock.world_days();
                if let Some(sync) =
                    players::frame(&ServerMessage::TimeSync { tick, time_of_day, world_days })
                {
                    for handle in &handles {
                        handle.send_raw(Arc::clone(&sync));
                    }
                }
            }
        }

        let busy = started.elapsed();
        ctx.metrics
            .tick_busy_micros
            .fetch_add(busy.as_micros() as u64, Ordering::Relaxed);
        if busy > tick_duration {
            ctx.metrics.tick_overruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What a whole tick costs, measured by running the real loop.
///
/// **The real `tick_loop`, not a list of the steps it calls.** Every step has
/// a benchmark of its own (`herd_of_a_hundred_and_twenty_animals_*`, the
/// water's reference spill), and none of them can say what the loop costs
/// with all of them in it plus the snapshots, the vitals and the locks
/// between -- which is the number a player feels as a server that keeps up or
/// does not. So this builds a context, puts one player and a full field of
/// animals on real terrain, lets the loop run for a few seconds at its own
/// rate, and reads `tick_busy_micros` back: the same counter `/stats` prints.
#[cfg(test)]
mod tick_cost {
    use super::*;
    use primitive_shared::animals::{Species, MAX_ANIMALS};

    /// ```text
    /// cargo test -p primitive_server --release --lib what_a_tick_costs -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_a_tick_costs() {
        for (label, animals) in [("nobody but the player", 0usize), ("a full field", MAX_ANIMALS)] {
            let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
            let (sx, _, sz) = ctx.world.spawn_point();
            let centre = ChunkPos::from_global(sx.floor() as i32, sz.floor() as i32).0;
            for dz in -3..=3 {
                for dx in -3..=3 {
                    let pos = ChunkPos::new(centre.x + dx, centre.z + dz);
                    ctx.world.insert(ctx.world.generate(pos));
                }
            }
            let ground = |x: i32, z: i32| -> f32 {
                (1..primitive_shared::types::CHUNK_SIZE_Y as i32)
                    .rev()
                    .find(|&y| ctx.world.cached_block(x, y, z).is_some_and(primitive_shared::types::is_collidable))
                    .map_or(80.0, |y| y as f32 + 1.0)
            };
            let feet = (sx, ground(sx.floor() as i32, sz.floor() as i32), sz);
            let (tx, rx) = tokio::sync::mpsc::channel(4096);
            let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
            // Kept alive, as `drinking_tests` does: a closed queue counts
            // every send as dropped, which is not the tick being measured.
            std::mem::forget((rx, chunk_rx));
            let handle = Arc::new(players::PlayerHandle::new(
                1,
                "measured".to_string(),
                "127.0.0.1:1".parse().unwrap(),
                tx,
                chunk_tx,
                10_000,
                primitive_shared::geometry::wide(feet),
                crate::logic::anticheat::AntiCheat::new(crate::settings::AntiCheatSettings::default(), 8, primitive_shared::geometry::wide(feet)),
            ));
            assert!(ctx.registry.insert_unique(handle));
            {
                let mut herd = ctx.animals.lock().unwrap();
                for i in 0..animals {
                    let species = Species::ALL[i % Species::ALL.len()];
                    let (x, z) = (sx.floor() as i32 + (i % 8) as i32 * 4 - 16, sz.floor() as i32 + (i / 8) as i32 * 4 - 16);
                    herd.spawn(species, (x as f32 + 0.5, ground(x, z) + 0.1, z as f32 + 0.5));
                }
            }
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
            let (ticks, busy) = runtime.block_on(async {
                let looping = tokio::spawn(tick_loop(Arc::clone(&ctx)));
                tokio::time::sleep(Duration::from_secs(4)).await;
                ctx.request_shutdown();
                let _ = looping.await;
                (
                    ctx.metrics.ticks.load(Ordering::Relaxed),
                    ctx.metrics.tick_busy_micros.load(Ordering::Relaxed),
                )
            });
            println!(
                "[tick] {label}: {:.3} ms busy a tick over {ticks} ticks, {} animals alive at the end",
                busy as f64 / ticks.max(1) as f64 / 1000.0,
                ctx.animals.lock().unwrap().len()
            );
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
            let written = tokio::task::spawn_blocking(move || {
                let chests = save_chests(&ctx, &dir);
                save_stalls(&ctx, &dir);
                chests
            })
            .await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[world] saved {n} chest(s)");
            }
        }

        // ...and the fires, for exactly the reason the chests are here:
        // a fire burns down without a single block changing, so a world
        // whose overlay is clean would never write one.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_fires(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[world] saved {n} fire(s)");
            }
        }

        // ...and what is alight, for the fires' reason: a burning wall
        // burns down without a block changing until it is char.
        {
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let _ = tokio::task::spawn_blocking(move || save_wildfire(&ctx, &dir)).await;
        }

        // ...and the pits, for the fires' reason and one more: a kiln's
        // hour counts down without a block changing, and it is the hour a
        // player already waited. See `logic::pits` on why it stays dirty.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_pits(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[world] saved {n} pit kiln(s) and log pile(s)");
            }
        }

        // ...and where the fish traps are. The fish are in the blocks; this
        // is the list the clock fills them from. See `logic::fishing`.
        {
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let _ = tokio::task::spawn_blocking(move || save_traps(&ctx, &dir)).await;
        }

        // ...and the racks, third for the same reason. A hide cures
        // without a block changing anywhere.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_racks(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[world] saved {n} rack(s)");
            }
        }

        // ...and the carcasses and the calendar, which only a clean stop
        // used to write. See `autosave_calendar`.
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || autosave_calendar(&ctx, &dir)).await;
            if let (true, Ok((Some(n), _))) = (logging, written) {
                println!("[world] saved {n} carcass(es)");
            }
        }

        // ...and whatever the mods are keeping, fourth and last, for the
        // fourth time for the same reason: a mod's state changes without
        // any block changing.
        #[cfg(feature = "mods")]
        {
            let logging = ctx.options.logging;
            let ctx = Arc::clone(&ctx);
            let dir = dir.clone();
            let written = tokio::task::spawn_blocking(move || save_mod_blobs(&ctx, &dir)).await;
            if let (true, Ok(Some(n))) = (logging, written) {
                println!("[mods] saved state for {n} mod(s)");
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

/// Writes who owns the stalls, on the chests' terms: nothing if nothing
/// changed. Beside the chests in every save, because a stall is half in
/// each file and a save that wrote one half is a stall whose prices and
/// goods are from two different moments.
fn save_stalls(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut stalls = ctx.stalls.lock().unwrap_or_else(|e| e.into_inner());
    if !stalls.is_dirty() {
        return None;
    }
    match stalls.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] stall save failed: {e}");
            None
        }
    }
}

/// Writes the rafts, on the fires' terms: nothing if none moved.
fn save_rafts(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut rafts = ctx.rafts.lock().unwrap_or_else(|e| e.into_inner());
    if !rafts.is_dirty() {
        return None;
    }
    match rafts.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] raft save failed: {e}");
            None
        }
    }
}

/// Writes how old the carcasses are, on the fires' terms exactly.
/// Writes the kept animals. **Every save, with no dirty flag**: a flock
/// moves every tick, so "nothing changed" is never true of it, and the file
/// is a few dozen records.
fn save_herd(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
    match animals.save_herd(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] kept animal save failed: {e}");
            None
        }
    }
}

fn save_carrion(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut carrion = ctx.carrion.lock().unwrap_or_else(|e| e.into_inner());
    if !carrion.is_dirty() {
        return None;
    }
    match carrion.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] carrion save failed: {e}");
            None
        }
    }
}

/// Writes the fires, on exactly the same terms as the chests: nothing at
/// all if none of them changed since the last save.
/// Writes what is alight and the standing torches, on the fires' terms.
fn save_wildfire(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut wildfire = ctx.wildfire.lock().unwrap_or_else(|e| e.into_inner());
    if !wildfire.is_dirty() {
        return None;
    }
    match wildfire.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] save of what is burning failed: {e}");
            None
        }
    }
}

fn save_fires(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
    if !fires.is_dirty() {
        return None;
    }
    match fires.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] fire save failed: {e}");
            None
        }
    }
}

/// Writes the list of fish traps, on the carrion's terms: only when it
/// changed.
fn save_traps(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut fishing = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner());
    if !fishing.is_dirty() {
        return None;
    }
    match fishing.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] trap save failed: {e}");
            None
        }
    }
}

/// Writes the pit kilns and log piles, on the fires' terms.
fn save_pits(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let mut pits = ctx.pits.lock().unwrap_or_else(|e| e.into_inner());
    if !pits.is_dirty() {
        return None;
    }
    match pits.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] pit save failed: {e}");
            None
        }
    }
}

/// Writes the map of where the players live. Every save, not only when
/// something changed: it changes whenever anybody stands anywhere.
fn save_haunts(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let vermin = ctx.vermin.lock().unwrap_or_else(|e| e.into_inner());
    match vermin.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] save of where the players live failed: {e}");
            None
        }
    }
}

/// What each mod saved, in one file beside the world's.
///
/// **Opaque bytes, keyed by mod name.** The host has no idea what is in
/// them and no business having one: a mod knows what its own state is,
/// and a key/value store here would be the server inventing a schema for
/// data it cannot read. What the server owns is the filing.
///
/// Its own file on the same terms the chests and the racks have theirs:
/// a world saved before mods existed has none, which reads as "no mod
/// has saved anything", and nothing has to be migrated.
#[cfg(feature = "mods")]
const MOD_SAVE_VERSION: u32 = 1;

#[cfg(feature = "mods")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ModSaveFile {
    version: u32,
    blobs: Vec<(String, Vec<u8>)>,
}

/// Reads `mods.bin` back.
///
/// A missing file is not an error, and neither is one this build cannot
/// read: a mod's blob is that mod's business, and refusing to start a
/// world because one of them wrote something odd would be the server
/// taking a hostage. The mod is told it has no saved state, which is a
/// case every mod already has to handle -- it is what a fresh world
/// looks like.
#[cfg(feature = "mods")]
fn load_mod_blobs(dir: &std::path::Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let path = dir.join("mods.bin");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let version: u32 = match bincode::deserialize(&bytes) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    if version != MOD_SAVE_VERSION {
        return Ok(Vec::new());
    }
    match bincode::deserialize::<ModSaveFile>(&bytes) {
        Ok(save) => Ok(save.blobs),
        Err(_) => Ok(Vec::new()),
    }
}

/// Writes them, quietly doing nothing if none changed.
#[cfg(feature = "mods")]
fn save_mod_blobs(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    let blobs = {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.take_dirty_blobs()
    };
    if blobs.is_empty() {
        return None;
    }
    let count = blobs.len();
    let payload = ModSaveFile {
        version: MOD_SAVE_VERSION,
        blobs,
    };
    let Ok(bytes) = bincode::serialize(&payload) else {
        eprintln!("[mods] could not serialise saved mod state");
        return None;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return None;
    }
    let final_path = dir.join("mods.bin");
    let tmp_path = final_path.with_extension("bin.tmp");
    // Atomic, like every other save here: a temp file and a rename, so
    // a crash mid-write cannot leave a truncated one.
    if std::fs::write(&tmp_path, &bytes).is_err() || std::fs::rename(&tmp_path, &final_path).is_err()
    {
        eprintln!("[mods] could not write saved mod state");
        return None;
    }
    Some(count)
}

/// Writes the racks, on exactly the same terms as the fires: nothing at
/// all if none of them changed since the last save.
fn save_racks(ctx: &Arc<Context>, dir: &std::path::Path) -> Option<usize> {
    // **The peat on the ground goes with the racks**, every place the racks
    // are written: it is the same kind of wait and it is lost the same way
    // by a restart. Its own file (`peat.bin`), and its own dirty flag.
    {
        let mut sods = ctx.peat.lock().unwrap_or_else(|e| e.into_inner());
        if sods.is_dirty() {
            if let Err(e) = sods.save(dir) {
                eprintln!("[world] peat save failed: {e}");
            }
        }
    }
    // ...and the walls drying, on the same terms (`walls.bin`).
    {
        let mut wet = ctx.walls.lock().unwrap_or_else(|e| e.into_inner());
        if wet.is_dirty() {
            if let Err(e) = wet.save(dir) {
                eprintln!("[world] walls save failed: {e}");
            }
        }
    }
    let mut racks = ctx.drying.lock().unwrap_or_else(|e| e.into_inner());
    if !racks.is_dirty() {
        return None;
    }
    match racks.save(dir) {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[world] rack save failed: {e}");
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
/// Where the world's own hour is kept.
///
/// **One number, written as text.** Everything else beside a world is a
/// binary file because everything else is thousands of records; this is
/// a single fraction of a day, and a fraction of a day is a thing an
/// operator wants to be able to read and change with an editor -- "the
/// server is stuck at night" is a support question, and `0.5` typed
/// into a file is the answer to it.
fn clock_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("clock.txt")
}

/// The hour a saved world was left at, if it kept one.
///
/// `None` for a world saved before this existed, which is every world
/// there was until now: that reads as "no opinion", and the caller
/// falls back to the setting a new world starts at. An unreadable or
/// nonsensical file reads the same way rather than refusing to open the
/// world -- a corrupted clock is not worth a save nobody can get into.
/// The clock file holds the world's age in days, hour in the fraction
/// -- unwrapped, so the seasons survive a restart. A file written before
/// the calendar existed holds a bare hour, which reads as day zero.
fn load_time_of_day(dir: &std::path::Path) -> Option<f32> {
    let text = std::fs::read_to_string(clock_path(dir)).ok()?;
    let parsed: f32 = text.trim().parse().ok()?;
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
}

/// Writes the hour down, and returns what was written so the save line
/// can say it.
fn save_time_of_day(ctx: &Arc<Context>, dir: &std::path::Path) -> f32 {
    let now = ctx.clock.world_days();
    if let Err(e) = std::fs::write(clock_path(dir), format!("{now}\n")) {
        eprintln!("[world] could not save the clock: {e}");
    }
    now
}

/// The two things beside a world that the autosave used to leave out:
/// how old the carcasses are, and what day it is.
///
/// **Written on every autosave, not only by `/save` and a clean stop.**
/// The loop wrote the profiles, the chests, the fires, the racks and the
/// overlay, and left these two to `save_everything` alone. A server that
/// crashed or was killed therefore came back with every edit of the last
/// few minutes and the calendar of its last clean shutdown -- the season
/// and the hour wound back by however long it had been up, under a world
/// that had moved on -- and with every carcass left since then reading
/// as fresh. The clock is one line of text and a carcass a few bytes, so
/// leaving them out never saved anything.
///
/// Answers the carcasses written, on `save_carrion`'s terms, and the day
/// the clock was written at.
fn autosave_calendar(ctx: &Arc<Context>, dir: &std::path::Path) -> (Option<usize>, f32) {
    // A world nobody has edited yet may have no directory: the overlay's
    // own save is what makes it, and that is skipped while it is clean.
    // Best effort, because the write below says so itself if it fails.
    let _ = std::fs::create_dir_all(dir);
    // The map of where the players live rides along for the same reason:
    // a crash that lost it cost the rats their way back to the house.
    save_haunts(ctx, dir);
    (save_carrion(ctx, dir), save_time_of_day(ctx, dir))
}

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
    let (inventory, position, yaw, pitch, health, nourishment, slot, body) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (
            state.inventory.clone(),
            state.position,
            state.yaw,
            state.pitch,
            state.vitals.health(),
            state.vitals.nourishment(),
            state.selected_slot as u8,
            profiles::StoredBody {
                equipment: state.equipment.clone(),
                hydration: state.vitals.hydration(),
                body_c: state.vitals.temperature(),
                wetness: state.vitals.wetness(),
                fatigue: state.vitals.fatigue(),
                injuries: *state.vitals.injuries(),
                // ...and the illness, which used to be cured by quitting
                // to the menu. See `profiles::Profile::sick_for`. With
                // what is still being digested folded in, so logging out
                // before it arrives does not escape it.
                sick_for: state.vitals.illness_owed(),
            },
        )
    };
    // What they have found and where their bags are, read under a second
    // short lock rather than as two more columns of the tuple above: that
    // tuple is the body, and this is the memory -- see
    // `Profiles::remember`.
    let (discovered, bags) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.discovered.clone(), state.bags.clone())
    };
    let mut profiles = ctx.profiles.lock().unwrap_or_else(|e| e.into_inner());
    profiles.remember(uuid, &discovered, &bags);
    profiles.store(
        uuid,
        inventory,
        position,
        yaw,
        pitch,
        health,
        nourishment,
        slot,
        body,
    );
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
            "[stats] players={} (peak {}) | tps={:.1} (busy {:.1}ms/tick, overruns {}) |              in={:.0}/s chunks={:.0}/s \
             edits={:.0}/s snapshots={:.0}/s | cached_chunks={} edited_blocks={} | \
             anticheat_flags={} kicks={}",
            ctx.registry.len(),
            ctx.registry.peak_players(),
            (current.ticks - previous.ticks) as f32 / seconds,
            // What a tick actually spent working, against what it was
            // given. A number near the interval is a server doing too
            // much; a number near zero with a low tick rate is a server
            // that is waiting badly, and the two want opposite fixes.
            {
                let ticks = (current.ticks - previous.ticks).max(1);
                (current.tick_busy_micros - previous.tick_busy_micros) as f32
                    / ticks as f32
                    / 1000.0
            },
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
    tick_busy_micros: u64,
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
            tick_busy_micros: m.tick_busy_micros.load(Ordering::Relaxed),
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
    // The mods, told before anything is written: a mod's `on_unload` is
    // its last chance to hand the host a blob, and a save that ran first
    // would write the one from before.
    #[cfg(feature = "mods")]
    {
        let _ = notify_mods!(
            ctx,
            primitive_modapi::Event::ServerStopping,
            primitive_modapi::EventData::default()
        );
        mods::unload_all(ctx);
    }

    // The generator pool, joined rather than left to the allocator.
    //
    // It holds an `Arc<World>`, and the client starts and stops one of
    // these per singleplayer session: threads still running after the
    // player went back to the menu would keep the last world's terrain
    // alive underneath the next one's. Joined *after* the save, because
    // a chunk a worker is halfway through is a chunk the save has
    // already decided it does not contain.
    ctx.chunks.stop();
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
    // **Before anything is written: put down whatever is in the air.**
    // A falling block is an entity in memory, the cell it left is
    // already an edit in the overlay, and the save format knows only
    // about cells -- so a world saved while a dune was coming down came
    // back with that sand missing from both ends. Finishing the fall
    // first costs a few dozen passes over a handful of entities and
    // puts every block exactly where it was going. See
    // `falling::FallingBlocks::ground_all` for the alternative that was
    // rejected (teaching the format about things in mid-air).
    let grounded = {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.ground_all(&*ctx.world)
    };
    if !grounded.is_empty() {
        // Anyone still connected -- `/save` is not only a shutdown --
        // has to see the landing, or their copy of the world keeps a
        // hole where the sand used to be.
        broadcast_changes(ctx, grounded);
    }
    let profiles = save_profiles(ctx, dir);
    match ctx.world.save(dir) {
        Ok(blocks) => {
            // One line naming all three, because the whole bug was an
            // operator being told "the world is saved" when two thirds
            // of it were not.
            let chests = save_chests(ctx, dir).unwrap_or(0);
            save_stalls(ctx, dir);
            let fires = save_fires(ctx, dir).unwrap_or(0);
            save_wildfire(ctx, dir);
            let racks = save_racks(ctx, dir).unwrap_or(0);
            let carrion = save_carrion(ctx, dir).unwrap_or(0);
            let herd = save_herd(ctx, dir).unwrap_or(0);
            save_traps(ctx, dir);
            save_haunts(ctx, dir);
            let rafts = save_rafts(ctx, dir).unwrap_or(0);
            #[cfg(feature = "mods")]
            save_mod_blobs(ctx, dir);
            let clock = save_time_of_day(ctx, dir);
            // One line naming every part of it, because the whole bug
            // this format is answering was an operator being told "the
            // world is saved" when two thirds of it were not.
            said.push(format!(
                "saved {blocks} block edit(s), {chests} chest(s), {fires} fire(s),                  {racks} rack(s), {carrion} carcass(es), {herd} kept animal(s), {rafts} raft(s), {} profile(s) and the clock at {clock:.3} to {}",
                profiles.unwrap_or(0),
                dir.display()
            ));
        }
        Err(e) => {
            said.push(format!("world save failed: {e}"));
            save_chests(ctx, dir);
            save_stalls(ctx, dir);
            save_fires(ctx, dir);
            save_wildfire(ctx, dir);
            save_racks(ctx, dir);
            save_carrion(ctx, dir);
            save_herd(ctx, dir);
            save_traps(ctx, dir);
            save_haunts(ctx, dir);
            save_rafts(ctx, dir);
            save_time_of_day(ctx, dir);
        }
    }
    said
}

/// What is extending this server, for a client that asked.
///
/// **The same host state `/mods` prints, in columns rather than in
/// sentences** -- see `protocol::ExtensionList` for why a screen must
/// not be handed a console's lines to parse. The two renderings are
/// built one after the other from the same two locks, which is the only
/// arrangement in which they cannot disagree.
///
/// Whether this build *has* either loader is part of the answer, not
/// something the client is left to infer from an empty list. The
/// client's own embedded server is built with `default-features =
/// false` and therefore has neither, so "no mods" is the ordinary
/// singleplayer case and a screen that could only say "none installed"
/// would be lying about it every time.
pub fn extension_list(ctx: &Arc<Context>) -> primitive_shared::protocol::ExtensionList {
    // The `mut` is used only in the build that has a native loader.
    // One attribute rather than two copies of this function behind
    // `cfg`, which is how the two builds drift.
    #[allow(unused_mut)]
    let mut items = {
        let host = ctx.plugins.lock().unwrap_or_else(|e| e.into_inner());
        host.catalogue()
    };
    #[cfg(feature = "mods")]
    let native_api = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        items.extend(host.catalogue());
        Some((
            primitive_modapi::API_VERSION.major,
            primitive_modapi::API_VERSION.minor,
        ))
    };
    #[cfg(not(feature = "mods"))]
    let native_api = None;

    primitive_shared::protocol::ExtensionList {
        native_api,
        scripts_supported: cfg!(feature = "plugins"),
        items,
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
                        let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
                        let (spawned, killed, forgotten) = animals.stats();
                        format!(
                            "animals: {} alive, {spawned} spawned, {killed} killed,                              {forgotten} forgotten",
                            animals.len()
                        )
                    },
                    {
                        let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
                        let growth = ctx.growth.lock().unwrap_or_else(|e| e.into_inner());
                        format!(
                            "fires: {} burning | bushes: {} ripening | weather: {}",
                            fires.len(),
                            growth.ripening(),
                            ctx.sky
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .weather()
                                .name()
                        )
                    },
                    {
                        let g = ctx.chunks.stats();
                        format!(
                            "terrain: {} thread(s), {} queued, {} in flight, {} made,                              {} shared, {} dropped",
                            g.workers, g.queued, g.in_flight, g.generated, g.deduplicated, g.dropped
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

            // What is extending this server, both kinds together. See
            // `commands::Command::Extensions` for why it is one command.
            Some("__EXTENSIONS__") => {
                let mut out = Vec::new();
                {
                    let host = ctx.plugins.lock().unwrap_or_else(|e| e.into_inner());
                    // Rendered by the host rather than walked here: in a
                    // build without the scripting engine there is no
                    // `Plugin` type to walk, and this call site is
                    // compiled into both. See `PluginHost::describe`.
                    let all = host.describe();
                    out.push(format!(
                        "{} plugin(s), {} active:",
                        all.len(),
                        host.active_count()
                    ));
                    for line in all {
                        out.push(format!("  {line}"));
                    }
                }
                #[cfg(feature = "mods")]
                {
                    let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
                    let all = host.describe();
                    out.push(format!(
                        "{} mod(s), {} active (API {}):",
                        all.len(),
                        host.active_count(),
                        primitive_modapi::API_VERSION
                    ));
                    for line in all {
                        out.push(format!("  {line}"));
                    }
                    let commands = host.commands();
                    if !commands.is_empty() {
                        out.push("commands claimed by mods:".to_string());
                        for (name, help) in commands {
                            out.push(format!("  /{name} -- {help}"));
                        }
                    }
                }
                #[cfg(not(feature = "mods"))]
                out.push("this build has no native mod loader".to_string());
                out
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

            Some("__WEATHER__") => {
                let sky = ctx.sky.lock().unwrap_or_else(|e| e.into_inner());
                let held = if sky.is_held() { " (held by an operator)" } else { "" };
                vec![format!(
                    "weather: {}{held}, next roll in {:.0}s",
                    sky.weather().name(),
                    sky.remaining().max(0.0)
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
            // **Somebody moved the sun**, which is what
            // `Event::TimeChanged` means -- as opposed to time passing,
            // which is every tick and is `Event::Tick`. Fired here as
            // well as in the mod API's own `set_time_of_day`, because a
            // mod watching for the hour being changed should not care
            // which of the two did it.
            time_changed(ctx, t);
            // Push it immediately rather than waiting for the periodic
            // sync, so `/time night` looks instant.
            let tick = ctx.clock.tick();
            ctx.registry.broadcast(ServerMessage::TimeSync {
                tick,
                time_of_day: t,
                world_days: ctx.clock.world_days(),
            });
            vec![format!("time of day set to {t:.3}")]
        }

        Response::SetWeather(wanted) => {
            let (weather, changed) = {
                let mut sky = ctx.sky.lock().unwrap_or_else(|e| e.into_inner());
                match wanted {
                    Some(weather) => (weather, sky.set(weather)),
                    None => {
                        sky.release();
                        (sky.weather(), false)
                    }
                }
            };
            if changed {
                // Pushed immediately rather than waiting for the sky to
                // change on its own, so `/weather storm` looks instant.
                ctx.fires
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_weather(weather);
                ctx.pits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_weather(weather);
                // ...and the mods, with the fire map's lock let go.
                weather_changed(ctx, weather);
                ctx.registry.broadcast(ServerMessage::WeatherSync { weather });
            }
            match wanted {
                Some(_) => vec![format!("weather set to {}", weather.name())],
                None => vec![format!(
                    "weather is {} again and left to the world",
                    weather.name()
                )],
            }
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

/// A blow landing on somebody, with what they are wearing taken off it.
///
/// **Every hit from another body goes through here**, and that is the
/// whole of what makes armour armour: there is no second path where a
/// boar's tusk or a bronze knife reaches `Vitals::hurt` directly, so
/// there is nowhere for a damage source to be added that quietly ignores
/// a cuirass.
///
/// What does *not* come through here is deliberate and is the other half
/// of the design. Drowning, starving, thirst and exposure are not blows:
/// a helmet does not help you breathe, and a coat that stopped
/// hypothermia by "absorbing" it would be doing the same job twice --
/// insulation is already in the temperature maths (`body::felt_ambient`)
/// and counting it again here would make a full leather set immune to
/// the cold rather than slow to feel it. Falls are the third exception,
/// and armour makes those *worse* rather than better, through the weight
/// it adds to the load the fall is judged against.
///
/// The set is charged one point of wear per blow it actually stopped
/// something of. A blow that gets through untouched -- because nothing
/// is worn -- costs nothing, which is what keeps a naked player's
/// equipment screen from filling up with wear on garments they do not
/// have.
///
/// **And every blow says what it was.** `blow` is what the hit leaves on
/// the body -- a cut, a bruise, a break (see `injury::Blow`) -- and it is a
/// parameter rather than something worked out here for the reason the
/// armour is applied here: this is the one door every blow comes through,
/// so a damage source added later cannot forget to leave a wound, and it
/// cannot leave one the armour did not see first. What the wound is sized
/// by is what got *through*, so a tunic turns a bite into a scratch.
pub(crate) fn strike_player(
    ctx: &Arc<Context>,
    victim: &Arc<players::PlayerHandle>,
    damage: f32,
    cause: &str,
    blow: primitive_shared::injury::Blow,
) -> survival::Outcome {
    // **Asked before the blow lands, with the number that will actually
    // land.** A mod deciding whether somebody may be hurt needs to see
    // what it would cost after the armour, not the raw swing -- a rule
    // like "never take more than half their health" is unwriteable
    // against a number the cuirass has not been subtracted from yet.
    //
    // Cheap enough to do before the dead check: `player_hurt` returns
    // immediately when nothing subscribed.
    let landing = {
        let state = victim.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return survival::Outcome::Unchanged;
        }
        state.equipment.worn().through(damage)
    };
    if !player_hurt(ctx, victim.id, landing, cause) {
        return survival::Outcome::Unchanged;
    }

    let (outcome, broke, bled) = {
        let mut state = victim.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return survival::Outcome::Unchanged;
        }
        let worn = state.equipment.worn();
        let through = worn.through(damage);
        let stopped = damage - through > 1e-4;
        let broke = if stopped {
            let worn_out = state.equipment.take_a_blow();
            if !worn_out.is_empty() {
                state.equipment_dirty = true;
            }
            worn_out
                .into_iter()
                .filter(|(_, wear)| {
                    matches!(wear, primitive_shared::inventory::Wear::Broke)
                })
                .map(|(slot, _)| slot)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // The wound before the damage, so a blow that kills still leaves
        // one -- the order of cause and effect should not depend on who
        // survived it.
        state.vitals.take_blow(blow, through);
        // **Blood for a blow that got through, and only here.** This is the
        // one door every blow comes in by (see above), which makes it the one
        // place a spray can be sent from without a damage-over-time path ever
        // being able to send one. A blow the armour stopped entirely shows
        // nothing: it did not reach skin.
        let bled = (through > 0.0).then(|| (chest_of(primitive_shared::geometry::narrow(state.position)), blow.drops()));
        (state.vitals.hurt(through, cause), broke, bled)
    };
    // With the victim's lock let go: telling everybody near takes each of
    // their locks in turn, the victim's among them.
    if let Some((at, drops)) = bled {
        broadcast_blood(ctx, primitive_shared::geometry::wide(at), drops);
    }
    for slot in broke {
        victim.send(ServerMessage::Error(format!(
            "your {} armour fell apart",
            slot.name()
        )));
    }
    // The worn set changed, so the client has to be told: it draws the
    // equipment screen from this and works out its own speed from it.
    send_equipment(victim);
    // ...and the weight, because a piece that broke is a piece nobody is
    // carrying any more -- and weight is what a fall costs.
    refresh_carried_weight(victim);
    outcome
}

/// Puts health back and says so.
///
/// **One path, so `Event::PlayerHealed` fires for both kinds of heal.**
/// Health comes back two ways -- a mod handing it over and the body's
/// own regeneration -- and an event that fired for one of them would be
/// an event a mod could not build a rule on. See the note on the event
/// for why it is not cancellable.
///
/// Behind the feature because the *other* kind of heal is the only one
/// in a build with no mod host: nothing in the game hands a player
/// health outright, and a helper compiled into the client's embedded
/// server for nobody to call is a helper the dead-code pass is right
/// about.
#[cfg(feature = "mods")]
pub(crate) fn heal_player(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    amount: f32,
) -> survival::Outcome {
    if !amount.is_finite() || amount <= 0.0 {
        return survival::Outcome::Unchanged;
    }
    let gained = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        // **A corpse is not healed.** `set_health` is the loading path and
        // clears `dead` as it writes -- rightly, for a profile coming in
        // off disk -- so a heal that went through it on a dead player
        // raised them where they fell: alive at the death site with the
        // death screen still up, the pack already on the ground, and the
        // respawn button refused because the server no longer thought
        // they were dead. A mod healing everyone nearby every tick did
        // that to every death in range. Coming back is `respawn_player`.
        if state.vitals.is_dead() {
            return survival::Outcome::Unchanged;
        }
        let before = state.vitals.health();
        state
            .vitals
            .set_health((before + amount).min(survival::MAX_HEALTH));
        state.vitals.health() - before
    };
    if gained <= 0.0 {
        return survival::Outcome::Unchanged;
    }
    player_healed(ctx, handle.id, gained);
    survival::Outcome::Changed
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
            let was_at_chest = {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                let was_at_chest = state.open_chest.take();
                // ...and a corpse is not sitting in a chair. The seat used
                // to be let go of only at the respawn, so for as long as the
                // death screen stood the body was drawn seated on every
                // other screen and the chair refused the next player who
                // tried it ("somebody is already sitting there").
                state.sitting_on = None;
                was_at_chest
            };
            handle.send(ServerMessage::ChestClosed);
            // A mod told the chest was opened is told it was shut, or the
            // corpse stands at that chest in its books for ever.
            // ...and the lid comes down, or a chest stands open over a body.
            if let Some(at) = was_at_chest {
                container_closed(ctx, handle.id, at);
                tell_chest_lid(ctx, at);
            }
            // Before `Died`, and that ordering is the whole of what the
            // client sees: the pack is already in the world by the time
            // the death screen goes up, so the empty inventory that
            // arrives with it is the truth rather than a state the
            // client has to be corrected out of a moment later.
            leave_corpse(ctx, handle);
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
    // **What was held is noted here, because this is the one door.** A
    // pick-up, a craft, a chest emptied into the pack, a mod's gift: every
    // change to a pack leaves the server through this function, so a
    // kind noted here is a kind noted whatever put it there -- and a
    // hook in each of those paths would be a recipe book with a hole
    // wherever somebody added a path and forgot. See `discovery`.
    let (inventory, learned) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.inventory_dirty {
            return;
        }
        state.inventory_dirty = false;
        let inventory = state.inventory.clone();
        let learned = state
            .discovered
            .note_inventory(&inventory)
            .then(|| state.discovered.kinds().to_vec());
        (inventory, learned)
    };
    handle.send(ServerMessage::InventoryState { inventory });
    if let Some(kinds) = learned {
        handle.send(ServerMessage::Discovered { kinds });
    }
}

/// Tells a player everything they have held. On join; after that
/// `send_inventory` sends it whenever it grows.
pub(crate) fn send_discovered(handle: &Arc<players::PlayerHandle>) {
    let kinds = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.discovered.kinds().to_vec()
    };
    handle.send(ServerMessage::Discovered { kinds });
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
        // **Only what is still there.** The column was read before anything
        // changed, and the partner of a half taken below -- a door's top, a
        // standing torch's flame -- has already gone with it
        // (`break_bed_partner`): writing air over that air paid the thing out
        // a second time, so a door whose floor was dug out dropped two doors.
        if ctx.world.cached_block(x, level, z) != Some(block) {
            continue;
        }
        if !ctx.world.set_block(x, level, z, BLOCK_AIR) {
            break;
        }
        spawn_block_drop(ctx, block, (x, level, z));
        // The half of a bed whose floor went takes the other half, which
        // still has one: a bed hanging half off a ledge is not a bed.
        // Broadcast by `break_bed_partner` itself, because the other half
        // may be in the next chunk and `broken` goes to this one's players.
        break_bed_partner(ctx, (x, level, z), block);
        broken.push(BlockChange {
            global_x: x,
            global_y: level,
            global_z: z,
            block_id: BLOCK_AIR,
        });
    }

    // **And the shelves growing out of its sides.**
    //
    // The run above walks straight up, which is where every other
    // supported block in this world lives. A bracket fungus is held by
    // the cell *beside* it (see `types::support_at`), so felling a
    // trunk left its brackets hanging in the air -- the exact fault
    // this function was written to stop, in the one direction it could
    // not see.
    //
    // One ring and no cascade: a bracket holds nothing up, so nothing
    // comes down with it.
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (nx, nz) = (x + dx, z + dz);
        let Some(block) = ctx.world.cached_block(nx, y, nz) else {
            continue;
        };
        if !primitive_shared::types::needs_support(block) {
            continue;
        }
        let (sx, sy, sz) = primitive_shared::types::support_at(block);
        if (nx + sx, y + sy, nz + sz) != (x, y, z) {
            continue; // held by something else, or held from below
        }
        if primitive_shared::types::can_grow_on(block, ground) {
            continue; // whatever is there now still holds it
        }
        if !ctx.world.set_block(nx, y, nz, BLOCK_AIR) {
            continue;
        }
        spawn_block_drop(ctx, block, (nx, y, nz));
        broken.push(BlockChange {
            global_x: nx,
            global_y: y,
            global_z: nz,
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
    {
        let mut mechanics = ctx.mechanics.lock().unwrap_or_else(|e| e.into_inner());
        if !mechanics.is_empty() {
            mechanics.on_block_changed(x, y, z);
        }
    }
    // The two that keep their own fields rather than living in the
    // registry (see `Context::fires`). They are notified through this
    // one function anyway, so that every edit path -- the player's, the
    // falling sand's, the water's -- reaches all of them or none.
    ctx.fires
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
    ctx.growth
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
    ctx.carrion
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
    ctx.fishing
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
    ctx.pits
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
    ctx.wildfire
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .on_block_changed(x, y, z);
}

/// What this player has burning within reach to work at.
///
/// What `crafting::Station` asks, answered against the server's own copy
/// of where the player is and its own map of what is alight. A client
/// that decided this for itself would be a client that smelts bronze in
/// a meadow.
///
/// The *kind* of each fire comes from the world rather than from the
/// fire map, which stores positions and fuel and has never needed to
/// know what it is burning in. One block lookup per nearby fire, on a
/// path taken once per craft.
pub(crate) fn heat_within_reach(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
) -> primitive_shared::crafting::Heat {
    use primitive_shared::types::{block_kind, BLOCK_BLOOMERY_LIT, BLOCK_KILN_LIT};
    let feet = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.position
    };
    let near: Vec<(i32, i32, i32)> = ctx
        .fires
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .within(primitive_shared::geometry::narrow(feet), primitive_shared::types::FIRE_WORKING_RANGE)
        .collect();
    let mut heat = primitive_shared::crafting::Heat::NONE;
    for (x, y, z) in near {
        match ctx.world.cached_block(x, y, z) {
            Some(block) if block_kind(block) == BLOCK_KILN_LIT => heat.kiln = true,
            Some(block) if block_kind(block) == BLOCK_BLOOMERY_LIT => heat.bloomery = true,
            // A cell the server has burning but cannot read is counted
            // as an ordinary fire rather than as nothing: the fire map
            // is the authority on *whether* something is alight, and
            // downgrading a kiln to a campfire refuses a craft where
            // guessing the other way would allow one.
            _ => heat.fire = true,
        }
    }
    // **The workshops are looked for in the world, not in a map.** A fire
    // map exists because fires burn down and have to be ticked; a bench
    // just stands there, and an index of benches would be one more thing
    // to keep in step with every block a player places or breaks, for a
    // question asked once per craft. A cube of cells the range across is
    // a few hundred cached lookups, and the same cube `fire_within_reach`
    // scans on the client -- centre of the cell, from the feet -- so a row
    // the menu offers is a row this allows.
    let range = primitive_shared::types::FIRE_WORKING_RANGE;
    let reach = range.ceil() as i32;
    let (fx, fy, fz) = (feet.0.floor() as i32, feet.1.floor() as i32, feet.2.floor() as i32);
    for dy in -reach..=reach {
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y, z) = (fx + dx, fy + dy, fz + dz);
                let Some(station) = ctx.world.cached_block(x, y, z).and_then(primitive_shared::crafting::Station::of_workshop)
                else {
                    continue;
                };
                // Widened through a generic rather than a cast, so the sum is the
                // same whether a position is kept in `f32` or in `f64`.
                fn wide<T: Into<f64>>(v: T) -> f64 {
                    v.into()
                }
                let (ox, oy, oz) = (f64::from(x) + 0.5 - wide(feet.0), f64::from(y) + 0.5 - wide(feet.1), f64::from(z) + 0.5 - wide(feet.2));
                if ox * ox + oy * oy + oz * oz <= f64::from(range * range) {
                    heat = heat.with_workshop(station);
                }
            }
        }
    }
    heat
}

/// Runs a recipe against a player's pack, up to `times`, and answers how
/// many came out.
///
/// **One path for both callers.** A player's `Craft` message and a mod's
/// `CraftingApi::craft` used to be able to diverge -- there was only
/// one, inline in the connection loop, and the mod API had no way in at
/// all -- and the day they diverged would be the day a mod could smelt
/// bronze in a meadow while a player could not. What is shared is the
/// whole of it: the fire is checked against the *server's* idea of where
/// the player is standing, `ItemCrafted` is asked first and may refuse,
/// and the new pack goes out.
///
/// `times` is bounded by the caller. The connection loop caps what a
/// client may ask for; a mod is native code in this process and is not
/// second-guessed, on the same reasoning `CoreApi::run_command` runs at
/// operator permission.
/// The state the player is in, as `quality` wants it.
///
/// **One place, because there are two benches.** The crafting screen and
/// the anvil both judge a piece out of the same person, and two readings
/// of "how tired are they" that drifted apart would be two mechanics
/// wearing one name. `accuracy` is `None` for every row that has no
/// marker to hit, which is nearly all of them.
fn maker_of(
    state: &net::players::PlayerRuntime,
    at_station: bool,
    tool: f32,
    accuracy: Option<f32>,
) -> primitive_shared::quality::Maker {
    primitive_shared::quality::Maker {
        fatigue: state.vitals.fatigue(),
        comfort: state.vitals.comfort_level(),
        health: (state.vitals.health() / logic::survival::MAX_HEALTH).clamp(0.0, 1.0),
        // The bar the other way up: `nourishment_fraction` is 1 when full
        // and `Maker::hunger` is 1 when empty.
        hunger: 1.0 - state.vitals.nourishment_fraction(),
        at_station,
        tool,
        accuracy,
    }
}

pub(crate) fn craft_for(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    index: usize,
    times: u32,
) -> Crafts {
    use primitive_shared::crafting::{Attempt, Crafted};

    let Some(recipe) = primitive_shared::crafting::recipe(index) else {
        return Crafts::NONE;
    };
    // Whether there is a fire within reach, asked once before the loop
    // and with the player's own lock not yet taken -- `heat_within_reach`
    // takes the fires and then the player, and the reverse order here
    // would be the deadlock that only happens on a busy server.
    let heat = heat_within_reach(ctx, handle);
    // ...and the mods, asked once for the same reason: a hook fired per
    // repetition would be a hook fired sixty-four times for one click.
    if !item_crafted(ctx, handle.id, index, recipe.output.0, recipe.output.1) {
        return Crafts::NONE;
    }
    // **The knapping die, rolled here and nowhere else.** The recipe
    // table says how often a blow shatters the nodule (`Recipe::failure`)
    // and the pure `craft` is told the outcome rather than rolling it --
    // the shared crate has no random source, on purpose, because the
    // client has a copy of the table and a client that could roll would
    // be a client that could re-roll. One generator per click rather
    // than one per repetition: `from_clock` mixes a counter into the
    // nanoseconds, but sixty-four seeds taken in the same microsecond are
    // sixty-four seeds a step apart, and one stream is the honest shape.
    let mut dice = logic::rng::Rng::from_clock();
    let crafts = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        // **One judgement for the click, not one per repetition.** The
        // first cut rolled inside the loop, on the argument that eight
        // blades are eight separate things -- and they are, but two of
        // them at different qualities do not stack (`quality`), so a
        // click for sixty-four blades made sixty-four one-item stacks,
        // overflowed a forty-square pack and dropped the rest on the
        // floor. `knapping_on_the_server_shatters_some_of_the_flint`
        // counted eighty-seven blades where a hundred and thirty-five
        // blows had landed, which is how that was found.
        //
        // The honest reading is also the cheap one: a batch is one
        // sitting, by one person, in one state, with one hand. The roll
        // still varies from click to click, which is where the spread
        // was ever going to be felt.
        //
        // The maker is read once for the same reason, before anything is
        // spent: the tool this row is worked with wears as the batch goes
        // (`craft_made`), and re-reading would make the last blade of a
        // long run worse for work the same run did.
        let maker = maker_of(
            &state,
            !recipe.station.is_hearth(),
            primitive_shared::crafting::tool_goodness(&state.inventory, recipe),
            None,
        );
        let quality = maker.judge(dice.range(0.0, 1.0));
        let mut crafts = Crafts::NONE;
        for _ in 0..times {
            let attempt = if dice.chance(recipe.failure) {
                Attempt::Fails
            } else {
                Attempt::Succeeds
            };
            match primitive_shared::crafting::craft_made(
                &mut state.inventory,
                recipe,
                heat,
                attempt,
                quality,
            ) {
                Crafted::Made => crafts.made += 1,
                // A shattered nodule spent its flakes and the loop goes
                // on: the player asked for eight blades and the honest
                // answer to that is "here are the five that did not
                // break", not a stop at the first one that did.
                Crafted::Failed => crafts.failed += 1,
                Crafted::Refused => break,
            }
        }
        state.inventory_dirty |= crafts.ran() > 0;
        crafts
    };
    if crafts.ran() > 0 {
        send_inventory(handle);
        refresh_carried_weight(handle);
    }
    if crafts.failed > 0 {
        // Told in words, because the pack shows only that the flakes are
        // gone, and flakes that vanish without a blade look like a bug
        // rather than a bad blow. The same channel every other refusal
        // uses, so it lands in the same place on the screen.
        handle.send(ServerMessage::Error(if crafts.failed == 1 {
            "the flint shattered".to_string()
        } else {
            format!("{} flints shattered", crafts.failed)
        }));
    }
    crafts
}

/// What a run of `craft_for` did: how many came out, and how many
/// attempts spent their flint on nothing.
///
/// Two numbers rather than the one this used to return, because the
/// callers ask different questions of them. The connection loop says
/// "cannot make that" only when *nothing ran* -- a click that shattered
/// a nodule ran, and telling the player it could not be made on top of
/// telling them it broke is two messages for one blow. A mod's
/// `CraftingApi::craft` reports what was *made*, and a shattered nodule
/// is not a thing made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Crafts {
    pub made: u32,
    pub failed: u32,
}

impl Crafts {
    pub const NONE: Crafts = Crafts { made: 0, failed: 0 };

    /// Attempts that spent their inputs, whichever way they went.
    pub fn ran(self) -> u32 {
        self.made + self.failed
    }
}

/// Whether the head of a player standing at `feet` is under water.
///
/// **Extracted so there is one answer to it.** The tick loop asks this
/// to run the breath clock and `PlayersApi::is_submerged` asks it for a
/// mod, and two copies of the rule would be two copies of the twelve per
/// cent at the top of a full cell that is water and does not look like
/// it -- which is the part that took a player drowning on the sea floor
/// to find. See `fluid::covers_with_above`.
pub(crate) fn head_under_water(ctx: &Arc<Context>, feet: (f32, f32, f32)) -> bool {
    let eye = (
        feet.0,
        feet.1 + primitive_shared::geometry::EYE_HEIGHT,
        feet.2,
    );
    let cell = (
        eye.0.floor() as i32,
        eye.1.floor() as i32,
        eye.2.floor() as i32,
    );
    ctx.world
        .cached_block(cell.0, cell.1, cell.2)
        .is_some_and(|block| {
            let above = ctx
                .world
                .cached_block(cell.0, cell.1 + 1, cell.2)
                .unwrap_or(primitive_shared::types::BLOCK_AIR);
            primitive_shared::fluid::covers_with_above(block, above, eye.1 - eye.1.floor())
        })
}

/// Is the water past the waist of a body standing at `feet`?
///
/// **The line the client's own physics swims at** (`physics::SWIM_DEPTH`,
/// half a body): past it there is more water than legs and a player swims
/// rather than walks, and a figure drawn by a different rule would be a
/// swimmer everybody else sees wading, or a wader they see swimming. Asked
/// at one point, the middle of the body, with the cell over it for the
/// twelve per cent at the top of a full cell (`head_under_water`).
pub(crate) fn waist_in_water(ctx: &Arc<Context>, feet: (f64, f64, f64)) -> bool {
    let waist = feet.1 + f64::from(primitive_shared::geometry::PLAYER_HEIGHT * 0.5);
    let cell = (feet.0.floor() as i32, waist.floor() as i32, feet.2.floor() as i32);
    ctx.world.cached_block(cell.0, cell.1, cell.2).is_some_and(|block| {
        let above = ctx
            .world
            .cached_block(cell.0, cell.1 + 1, cell.2)
            .unwrap_or(primitive_shared::types::BLOCK_AIR);
        primitive_shared::fluid::covers_with_above(block, above, (waist - waist.floor()) as f32)
    })
}

/// Moves the selected hotbar slot, and says so.
///
/// The client's own key press and `PlayersApi::set_selected_slot` come
/// through here together, so a mod watching `HeldSlotChanged` hears its
/// own change as well as the player's -- an event that fired for one and
/// not the other would be an event a mod could not trust.
pub(crate) fn select_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
) {
    let slot = slot.min(primitive_shared::inventory::HOTBAR_SLOTS - 1);
    let (changed, block) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let changed = state.selected_slot != slot;
        state.selected_slot = slot;
        (changed, state.inventory.block_in(slot))
    };
    if changed {
        held_slot_changed(ctx, handle.id, slot, block);
    }
}

/// Empties a full jug into the player.
///
/// The empty jug comes back, which is the whole reason a jug is worth
/// making: a mouthful from a river is a mouthful, and a jug is a
/// mouthful you can carry and then keep. Nothing is spent unless the
/// drink actually did something -- a jug emptied at full hydration is an
/// item destroyed, which is the same class of bug as a craft that
/// consumes its ingredients and produces nothing.
fn drink_from_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
    becomes_empty: primitive_shared::types::BlockId,
) {
    let drank = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        // Room for the empty one, **before the water is credited**. A
        // pack with no room keeps the full jug rather than losing it --
        // the player is thirsty, not robbed.
        //
        // This used to run *after* `drink`, and the order was the whole
        // bug: `drink` is not a question, it fills the meter. So a
        // player whose pack was full drank the jug, kept the jug, and
        // could click it again -- an unlimited water supply, available
        // exactly when a player is most likely to be carrying a full
        // pack. A slot holding one jug is always fine, because emptying
        // it frees the slot the empty one goes back into.
        let single = state.inventory.count_in(slot) == 1;
        if !single && !state.inventory.has_room_for(becomes_empty, 1) {
            return;
        }
        // **What the jug was filled from**, which it has carried in its
        // own id since it was dipped (see `types::vessel_water`). A jug
        // of pond water is still pond water a week later and in
        // somebody else's chest.
        let kind = primitive_shared::types::vessel_water(
            state.inventory.block_in(slot).unwrap_or(becomes_empty),
        );
        if !state
            .vitals
            .drink_water(kind, primitive_shared::body::JUG_HYDRATION)
        {
            return;
        }
        state.inventory.take_from(slot, 1);
        let left = state.inventory.add(becomes_empty, 1);
        debug_assert_eq!(left, 0, "an emptied jug had nowhere to go");
        state.inventory_dirty = true;
        state.gesture.made(primitive_shared::protocol::Action::Drink);
        true
    };
    if drank {
        send_inventory(handle);
        send_body(handle);
        // Two litres less to carry.
        refresh_carried_weight(handle);
        fire_plugin_hook(
            ctx,
            "on_drink",
            vec![plugins::Value::Int(handle.id as i64)],
            None,
        );
    }
}

/// A right click on a block that was neither a placement nor a chest.
///
/// Fire, water, the hoe, and the knife: strike a spark into a laid
/// hearth, drink from or fill a jug at a river, till turf, and take a
/// cut off a carcass. What happens is decided entirely here, from the
/// block and from what the server believes is in the player's hand --
/// the message carries neither, because a client that could name the
/// *effect* could light a fire with an empty hand, or skin a deer with
/// one.
pub(crate) fn use_block(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
) {
    use primitive_shared::types::{is_burning, is_hearth, lights_into};

    let (feet, held, _slot, dead) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (
            state.position,
            state.inventory.block_in(state.selected_slot),
            state.selected_slot,
            state.vitals.is_dead(),
        )
    };
    if dead {
        return;
    }
    // The same reach a punch has. Using a block across the map is the
    // same cheat as hitting somebody across it, and it gets the same
    // answer.
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    // `None`, deliberately: reaching a lever is an arm, not a weapon.
    // A spear in the hand must not let a player open a chest from six
    // blocks away.
    if !primitive_shared::combat::within_reach(
        (feet.0, feet.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT), feet.2),
        primitive_shared::geometry::wide(centre),
        None,
    ) {
        // A door the client has already swung on its own screen is put back
        // the way it really hangs (`swing_door`): refused in silence, the
        // player's door would stay open for them and shut for everyone else.
        unswing_door(ctx, handle, at);
        return;
    }

    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return; // a cell nobody has loaded
    };

    // **An unlit torch at any fire takes the flame from it**, first and
    // whatever the fire is: a hearth, a kiln, a pit kiln or a log pile
    // burning, a burning wall, a standing torch alight
    // (`types::lights_a_torch`). It used to be two branches further down,
    // one for hearths and one for standing torches, and a torch held to a
    // burning house or a firing pit kiln did nothing -- or, at a kiln, fed
    // the kiln. A torch is lit *from* a fire rather than struck: the wad is
    // dry grass, which takes from a flame far more readily than from a
    // spark. Nothing is spent but the wad's forty-five seconds.
    if held.is_some_and(|held| primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_TORCH)
        && primitive_shared::types::lights_a_torch(block)
    {
        // A wet wad does not take the flame (`wet`), and says so rather
        // than falling through to feeding the fire with the torch.
        if held.is_some_and(primitive_shared::wet::will_not_light) {
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::TorchWet });
            return;
        }
        let (slot, lit) = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let slot = state.selected_slot;
            (slot, state.inventory.light_torch(slot, primitive_shared::types::BLOCK_TORCH_LIT))
        };
        if lit {
            held_slot_changed(ctx, handle.id, slot, Some(primitive_shared::types::BLOCK_TORCH_LIT));
        }
        return;
    }

    // ---- the gestures that need nothing in hand ----
    //
    // **These come before the empty-hand guard, and that ordering is the
    // whole of a bug this had.** Everything below the guard needs a
    // *tool*: a hoe to till with, a striker to light a fire with, a jug
    // to fill. Taking leather off a rack and drinking from a river need
    // neither, and a player doing either would naturally have empty
    // hands -- so a guard placed before them meant the answer to "take
    // the leather off" was silence unless you happened to be holding
    // something.

    // **A door swings**, whatever is in the hand: a player carrying boards
    // home opens the door with them in their arms, and a door that wanted an
    // empty hand would be a door that wanted the pack rearranged first. See
    // `swing_door`.
    if primitive_shared::types::is_door(block) {
        swing_door(ctx, at, block);
        return;
    }

    // **Lying down**, which needs nothing in hand and belongs up here
    // with the other empty-handed gestures for exactly that reason.
    if let Some(rest) = primitive_shared::body::Rest::of(block) {
        lie_down(ctx, handle, at, rest);
        return;
    }

    // ...and sitting, which is the same gesture at a smaller piece of
    // furniture and is deliberately *not* the same mechanic. Sitting
    // does not pass time and does not lock the player: it is a slow
    // trickle off the tiredness meter while they stay put, which is
    // what somebody at a fire between jobs is doing. See
    // `body::sitting_recovery`. A stool or a chair (`types::is_seat`).
    if primitive_shared::types::is_seat(block) {
        sit_down(ctx, handle, at, block);
        return;
    }

    // **A fish trap with fish in it**, emptied into the pack with whatever
    // is in the hand -- a player coming back to a trap is carrying the day's
    // things, and a trap that wanted an empty hand would be a trap that
    // wanted the pack rearranged first. An empty one says so. See
    // `empty_trap`.
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_FISH_TRAP {
        empty_trap(ctx, handle, at, block);
        return;
    }
    // **A snare with a hare in it, or one a fox has robbed**, whatever is in
    // the hand, on the trap's argument: see `tend_snare`.
    if primitive_shared::snare::is_snare(block) {
        tend_snare(ctx, handle, at, block);
        return;
    }
    // **A salt pan**, filled from a jug of the sea or scraped: `tend_pan`.
    if primitive_shared::saltpan::is_pan(block) {
        tend_pan(ctx, handle, at, block);
        return;
    }

    // **A thing set down is taken back**, with whatever is in the hand: the
    // hand that laid a knife on a stone is usually holding the next thing.
    if primitive_shared::types::is_set_down(block) {
        pick_up_set_down(ctx, handle, at);
        return;
    }

    // **Apples, picked off the tree by hand.** Up here with the other
    // gestures that need nothing in hand, because an empty hand is exactly
    // what somebody reaching into a tree has.
    // ...and **moss, scraped off a stone or a trunk by an empty hand**
    // (`ground::scraped`): the same take into the pack, what is left the bare
    // block. An empty hand only, so a mossy log still strips under an axe.
    if primitive_shared::types::picks_by_hand(block) || (held.is_none() && primitive_shared::ground::is_mossy(block)) {
        pick_by_hand(ctx, handle, at, block);
        return;
    }

    // **A palm, shaken by its trunk**, with the empty hand that shakes a
    // tree. The coconuts hang a trunk's height up, and climbing to pick
    // them is one way; this is the other, and what it costs is where the
    // nut lands -- on the ground under the crown, where it rolls, and on a
    // beach that is sometimes the sea.
    if held.is_none()
        && primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_PALM_TRUNK
    {
        shake_palm(ctx, at);
        return;
    }

    // **A barrel**: pour the full jug in hand into it, dip the empty one,
    // or drink from it with nothing in hand. Not water to the world -- the
    // ray stops at its staves -- so it is asked about by name rather than
    // falling into the river below, and it comes before the tool guard
    // because a bare hand is one of the three.
    if primitive_shared::types::is_barrel(block) {
        use_barrel(ctx, handle, at, block, held);
        return;
    }

    // **A raft, launched.** Before the water's own gestures, because a raft
    // in the hand at a lake is not a thirsty player: it is what they carried
    // there. See `rafts::launch`.
    if primitive_shared::types::is_liquid(block)
        && held.is_some_and(|held| {
            primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_RAFT
        })
    {
        rafts::launch(ctx, handle, at);
        return;
    }

    // **A rod at the water is not a use at all any more**, and this branch
    // stays to say so: the throw is held and let go
    // (`ClientMessage::CastLine`), so a plain use with a rod in hand must
    // fall through to nothing rather than on to the water's own gestures.
    // Without this, a player holding a rod at a lake would drink from it,
    // which is what happened the first time the cast moved off `UseBlock`.
    if primitive_shared::types::is_liquid(block)
        && held.is_some_and(|held| {
            primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_FISHING_ROD
        })
    {
        return;
    }

    // Water: fill a jug if there is one in hand, and drink from it if
    // there is not.
    if primitive_shared::types::is_liquid(block) {
        // **What kind of water this is** -- see `water_kind`. It decides
        // what the mouthful is worth and what it costs afterwards, and
        // a jug carries the answer away with it.
        //
        // **A cave lake is clean to drink, and only to drink.** Water that
        // came down through the rock is what a well taps, and that is the
        // reason to carry a jug down to one (`worldgen::cave_water`). It is
        // decided here rather than in `water_kind`, because the rod and the
        // trap ask that too, and a pool no light reaches that bit at a
        // river's rate would be the best fishing in the world.
        let kind = match water_kind(ctx, at, block) {
            primitive_shared::body::Water::Standing if ctx.world.cave_water_at(at.0, at.1, at.2) => {
                primitive_shared::body::Water::Fresh
            }
            kind => kind,
        };
        if held.and_then(primitive_shared::types::filled_vessel).is_some() {
            fill_vessel(ctx, handle, primitive_shared::types::jug_of(kind));
            return;
        }
        // A mouthful, straight from the river. Less than a jug (see
        // `body::DRINK_HYDRATION`), which is what keeps a jug worth
        // making for somebody standing at a lake.
        let drank = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let drank = state
                .vitals
                .drink_water(kind, primitive_shared::body::DRINK_HYDRATION);
            if drank {
                state.gesture.made(primitive_shared::protocol::Action::Drink);
            }
            drank
        };
        if drank {
            send_body(handle);
            warn_about_water(handle, kind);
        }
        return;
    }

    // **A carcass.** Before the empty-hand guard on purpose, and for
    // the opposite reason to the two gestures above: a bare hand on a
    // carcass does nothing, and the player has to be *told* that,
    // because a heap that ignores a click reads as a heap that cannot
    // be used. The decision -- knife, wrong tool, no tool -- is in the
    // shared crate; see `animals::butcher`.
    if let Some(outcome) = primitive_shared::animals::butcher(block, held) {
        butcher_carcass(ctx, handle, at, outcome);
        return;
    }
    // ...and a dead player's body, with a knife. The same doing as a
    // carcass -- the cut at the butcher's feet, the knife worn -- and the
    // body's things stay where they are: its last cut leaves the bones,
    // which are the same container. See `animals::cut_body`.
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CORPSE
        && held.is_some_and(primitive_shared::types::is_knife)
    {
        if let Some(outcome) = primitive_shared::animals::cut_body(block, held) {
            butcher_carcass(ctx, handle, at, outcome);
        }
        return;
    }

    // **Fires in the ground: a pit kiln or a log pile.** Before the tool
    // guard, because two of their gestures are empty-handed -- taking a pot
    // back out, asking how long is left -- and everything they do is
    // decided in `logic::pits`, which answers what left the hand and what
    // the player is told.
    if primitive_shared::pit::is_pit_kiln(block) || primitive_shared::pit::is_log_pile(block) {
        let outcome = {
            let mut pits = ctx.pits.lock().unwrap_or_else(|e| e.into_inner());
            if primitive_shared::pit::is_pit_kiln(block) {
                pits.use_kiln(&*ctx.world, at, held)
            } else {
                pits.use_pile(&*ctx.world, at, held)
            }
        };
        apply_pit_outcome(ctx, handle, outcome);
        return;
    }
    // ...and pottery held at the floor of an empty pit, which starts one:
    // «Кладутся в низ предметы для обжога». The cell used is the floor the
    // player is looking at, and the pit is the air over it.
    if held.is_some_and(primitive_shared::pit::is_raw_pottery)
        && primitive_shared::pit::takes_pottery_above(|x, y, z| ctx.world.cached_block(x, y, z), at)
    {
        let outcome = ctx
            .pits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .use_kiln(&*ctx.world, (at.0, at.1 + 1, at.2), held);
        apply_pit_outcome(ctx, handle, outcome);
        return;
    }

    // Everything past here is a gesture with a tool in it.
    let Some(held) = held else { return };

    // **Tapping a trunk for resin, back.** It went with the glue, on the
    // argument that a gesture whose yield no recipe wants teaches a dead
    // end; the standing torch wants it now. See `tap_trunk`.
    if primitive_shared::types::is_knife(held) {
        if let Some(outcome) = tap_trunk(ctx, handle, at, block) {
            if let Err(said) = outcome {
                handle.send(ServerMessage::Error(said.to_string()));
            }
            return;
        }
    }

    // **A lump of resin at a standing torch that has burnt out** lights it
    // again where it stands: the wad is what burns, and the pole is fine.
    // Either cell of the torch, because a player aims at the pole as often
    // as at the knot a head higher.
    if primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_RESIN
        && primitive_shared::wildfire::is_standing_torch(block)
    {
        let top = if primitive_shared::wildfire::is_standing_torch_top(block) { at } else { (at.0, at.1 + 1, at.2) };
        let lit = ctx
            .wildfire
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .rewad_torch(&*ctx.world, top);
        if lit {
            spend_held(ctx, handle);
            ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
            notify_mechanics(ctx, top.0, top.1, top.2);
            broadcast_block(ctx, top, primitive_shared::types::BLOCK_STANDING_TORCH_LIT);
        }
        return;
    }

    // **Ash dug into a field.** See `wildfire::ASH_DRESSING_FACTOR` for why
    // this is what ash is for.
    if primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_ASH {
        if let Some(field) = primitive_shared::wildfire::dressed(block) {
            if ctx.world.set_block(at.0, at.1, at.2, field) {
                spend_held(ctx, handle);
                ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
                notify_mechanics(ctx, at.0, at.1, at.2);
                broadcast_block(ctx, at, field);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::AshDugIn });
            }
            return;
        }
        if primitive_shared::wildfire::is_dressed(block) {
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::FurrowHasAsh });
            return;
        }
    }

    // **The peg.** Driven into boards, and only into boards: what it
    // fastens is a joint (see `types::pegged_form`). A pegged board
    // holds itself up whatever it spans and ends the span its
    // neighbours are measured across, which is the whole answer to a
    // roof that will otherwise come down -- see
    // `falling::Looseness::Built`.
    //
    // Before the hoe, because both are implements and the hoe's own
    // test is about the ground: a peg held over a meadow tilled it for
    // exactly as long as `is_implement` was the only question asked.
    if primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_PEG {
        let Some(fastened) = primitive_shared::types::pegged_form(block) else {
            return;
        };
        if !ctx.world.set_block(at.0, at.1, at.2, fastened) {
            return;
        }
        let (slot, left) = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let slot = state.selected_slot;
            state.inventory.take_from(slot, 1);
            (slot, state.inventory.block_in(slot))
        };
        held_slot_changed(ctx, handle.id, slot, left);
        send_inventory(handle);
        refresh_carried_weight(handle);
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        notify_mechanics(ctx, at.0, at.1, at.2);
        broadcast_block(ctx, at, fastened);
        return;
    }

    // What a hoe makes a field of is the ground's question (`ground::tills`):
    // turf, bare earth, the savanna's dry ground and turf, and the fertile
    // soils -- the steppe's black earth, not the taiga's podzol.
    if primitive_shared::types::is_hoe(held) && primitive_shared::ground::tills(block)
    {
        // Room to grow. Tilling under a floor gives a field that can
        // never be planted, which reads as the hoe not working.
        let above = ctx.world.cached_block(at.0, at.1 + 1, at.2);
        if !above.is_some_and(primitive_shared::types::is_air) {
            return;
        }
        if !ctx
            .world
            .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_FARMLAND)
        {
            return;
        }
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        notify_mechanics(ctx, at.0, at.1, at.2);
        broadcast_block(ctx, at, primitive_shared::types::BLOCK_FARMLAND);
        // **The soil says what it is when it is turned, and only
        // then.** Fertility decides how fast a crop grows here (see
        // `worldgen::Fertility`), and a mechanic a player cannot
        // perceive is a mechanic that does not exist -- but a permanent
        // readout would be another number on the screen. The moment a
        // spadeful of earth is turned over is exactly when a farmer
        // learns this, so it is said once, in the place they are
        // looking, and never again.
        //
        // Only the two ends of the soil are named; ordinary ground says
        // nothing about itself, because most of the world is ordinary.
        //
        // ...and whether the earth is wet, which is the other half of what
        // a farmer learns with a spade in their hand. A field with no water
        // within `growth::WATER_REACH` grows two and a half times slower
        // (`growth::DRY_FIELD_FACTOR`), and a player who cannot see that
        // happening would put every field in the wrong place.
        //
        // **Said when the news changes, not every spadeful.** The soil was
        // the only thing said, and only at its two ends, because a line
        // that appears every time is a line nobody reads. Water is worth
        // saying either way, which put a line under every cell: a
        // nine-by-nine field was eighty-one lines of chat. So the same
        // news to the same player within `FIELD_NOTE_REPEAT_SECS` is not
        // repeated. The next cell of the same field says nothing; a
        // different field, or the same one once the channel reaches it,
        // does.
        let note = field_note(
            ctx.world.fertility_at(at.0, at.2),
            logic::growth::watered(&*ctx.world, at.0, at.1, at.2),
        );
        let news = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let news = match state.field_note {
                Some((said, when)) => {
                    said != note || when.elapsed().as_secs_f32() > FIELD_NOTE_REPEAT_SECS
                }
                None => true,
            };
            if news {
                state.field_note = Some((note, Instant::now()));
            }
            news
        };
        if news {
            handle.send(ServerMessage::Chat {
                from: None,
                username: "server".to_string(),
                text: note.to_string(),
            });
        }
        return;
    }

    // **A firepit, struck where it lies.** Flint at the ground with three
    // sticks and a log lying on it -- see `strike_firepit`. Before the
    // hearth guard, because the ground is not a hearth yet.
    if !is_hearth(block) && primitive_shared::types::block_kind(held) == fire::STRIKER {
        strike_firepit(ctx, handle, at);
        return;
    }

    if !is_hearth(block) {
        return;
    }

    // **Feeding by hand is gone.** A hearth has a fuel slot now (see
    // `primitive_shared::hearth`), and a gesture that put a log into a
    // fire without opening it would be a second way to do one thing --
    // with its own rules about what counts as fuel, its own cap, and its
    // own message when the cap is reached. What is left of the right
    // click is lighting, which is the one thing that is *not* about the
    // inside of the fire.
    if is_burning(block) {
        return;
    }

    // Lighting. A nodule of flint struck against the ring of stones --
    // see `fire::STRIKER` for why it is the nodule rather than a flake
    // or a finished knife.
    if primitive_shared::types::block_kind(held) != fire::STRIKER {
        return;
    }
    // **Wet fuel in the slot does not catch** (`wet`). The strike is refused
    // before the flint is spent, and the player is told why: a hearth that
    // took the spark and burned nothing would be a nodule gone for a fire
    // that never was. An empty slot is still the laid fire it always was.
    let fuel = ctx
        .chests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contents(at)
        .block_in(primitive_shared::hearth::FUEL_SLOT);
    if fuel.is_some_and(primitive_shared::wet::will_not_light) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::FuelWet });
        return;
    }
    if !ctx
        .fires
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .light(at)
    {
        return; // somebody else struck it in the same tick
    }
    let Some(alight) = lights_into(block) else {
        return; // already burning, which the branch above handled
    };
    if !ctx.world.set_block(at.0, at.1, at.2, alight) {
        // Out of bounds, which cannot happen for a cell that just read
        // as a campfire -- and if it ever does, the fire has to be
        // forgotten again rather than left burning in a map with nothing
        // under it.
        ctx.fires
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extinguish(at);
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    // **The nodule is spent**, where it used to be kept. See
    // `fire::STRIKER` for the decision and the two it was chosen over.
    spend_striker(ctx, handle);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, alight);
}

/// Takes one of whatever is in the selected slot out of the hand, and tells
/// the client: the pack, the hand, the weight.
pub(crate) fn spend_held(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let (slot, left) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        state.inventory.take_from(slot, 1);
        state.inventory_dirty = true;
        (slot, state.inventory.block_in(slot))
    };
    held_slot_changed(ctx, handle.id, slot, left);
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// How long a scored trunk takes to bleed again, in seconds.
///
/// Ten minutes: a player who wants a torch for every night taps a stand of
/// trees and not one trunk over and over, and a camp in the steppe with two
/// trees in sight has to carry its resin in. See `tap_trunk`.
pub(crate) const RESIN_REGROW_SECONDS: f32 = 600.0;

/// A knife on a standing trunk: a lump of resin into the pack, if this cell
/// of trunk has not been scored in the last `RESIN_REGROW_SECONDS`.
///
/// `None` when the block is not a standing trunk, so the knife's other uses
/// go on; `Some(Err)` is a refusal with the reason.
///
/// **The trunk is not changed**, and that is the difference from the tap
/// that went with the glue, which took the bark off the block. A stripped
/// cell in a standing tree is a cell the felling has to recognise as the
/// same tree, and a pale ring round every trunk a player has tapped is a
/// forest that looks damaged for good. So the wound is remembered on the
/// server instead, and forgotten at a restart -- which costs a restart's
/// worth of extra resin, and nobody a thing.
pub(crate) fn tap_trunk(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) -> Option<Result<(), &'static str>> {
    use primitive_shared::types::{block_axis, block_kind, Axis, BLOCK_RESIN, BLOCK_STRIPPED_LOG};
    let standing = primitive_shared::wildfire::fuel(block) == Some(primitive_shared::wildfire::Fuel::Log)
        && block_axis(block) == Axis::Y
        && block_kind(block) != BLOCK_STRIPPED_LOG;
    if !standing {
        return None;
    }
    let bleeding = ctx
        .wildfire
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .score_trunk(at, RESIN_REGROW_SECONDS);
    if !bleeding {
        return Some(Err("this trunk has been scored: it has no more resin for now"));
    }
    // **A birch and a willow give their bark, and the rest their resin.**
    // Neither is a resinous tree -- the pitch a torch is wadded with bleeds
    // out of a conifer, and a birch's white bark is what tar is cooked from
    // (`crafting`, "distil tar"), a willow's what a bruise is bound with
    // (`injury::Treatment::WillowBark`). The same cut, the same ten minutes
    // before the trunk gives again: the difference is which tree the
    // player walked to, which is the whole of what makes a birch wood and a
    // river's willows two places.
    //
    // Rejected: bark *and* resin off every trunk. A knife that took two
    // things off one tree would make the tree a second answer to the
    // question the other trees already answer.
    let taken = match block_kind(block) {
        primitive_shared::types::BLOCK_BIRCH_LOG => primitive_shared::types::BLOCK_BIRCH_BARK,
        primitive_shared::types::BLOCK_WILLOW_LOG => primitive_shared::types::BLOCK_WILLOW_BARK,
        _ => BLOCK_RESIN,
    };
    let (spare, slot, held) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let spare = state.inventory.add(taken, 1);
        state.inventory_dirty = true;
        let slot = state.selected_slot;
        let _ = state.inventory.wear_tool(slot);
        (spare, slot, state.inventory.block_in(slot))
    };
    if spare > 0 {
        let feet = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            taken,
            spare,
            (feet.0, (feet.1 + 0.5), feet.2),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
    held_slot_changed(ctx, handle.id, slot, held);
    send_inventory(handle);
    refresh_carried_weight(handle);
    Some(Ok(()))
}

/// Takes the nodule a fire was just struck with out of the hand.
///
/// Called only once the fire has caught -- after the block is written --
/// so a strike refused for any reason (a fire already lit, rain, a pit
/// with nothing in it) costs nothing, which is the one rule that keeps
/// spending a striker from being a tax on clicking. See `fire::STRIKER`.
pub(crate) fn spend_striker(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let spent = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        if state.inventory.block_in(slot).map(primitive_shared::types::block_kind) != Some(fire::STRIKER) {
            None
        } else {
            state.inventory.take_from(slot, 1);
            state.inventory_dirty = true;
            Some((slot, state.inventory.block_in(slot)))
        }
    };
    let Some((slot, left)) = spent else {
        return;
    };
    held_slot_changed(ctx, handle.id, slot, left);
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// Swaps an empty vessel in the selected slot for a full one.
///
/// Split out of `use_block` because it is the only branch there that
/// changes the *pack* rather than the world, and because the two rules
/// it has to get right -- spend the empty one only if the full one fits,
/// and only ever one at a time -- are the same two rules every other
/// gesture in this file that turns one item into another has to get
/// right.
/// What kind of water is in this cell: a stream, a pond, or the sea.
///
/// **Three questions, in the order that answers them cheapest.** The
/// biome says which body of water this is -- the sea is salt, a river
/// is running -- and it is the generator's own answer, so a channel a
/// player dug from a river is not a river. What catches that is the
/// second test: water that is *flowing* (a cell holding less than a
/// full block, see `fluid::is_flowing`) is water that is moving, and
/// moving water is fresh wherever it is. Everything else -- a lake, a
/// puddle, a flooded shaft -- is standing.
///
/// The rule a player learns from it is the true one: drink from
/// something that is going somewhere.
fn water_kind(
    ctx: &Arc<Context>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) -> primitive_shared::body::Water {
    use primitive_shared::body::Water;
    use primitive_shared::worldgen::Biome;
    match ctx.world.biome_at(at.0, at.2) {
        Biome::Ocean | Biome::Beach => Water::Salt,
        Biome::River => Water::Fresh,
        _ if primitive_shared::fluid::is_flowing(block) => Water::Fresh,
        _ => Water::Standing,
    }
}

fn fill_vessel(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    full: primitive_shared::types::BlockId,
) {
    let filled = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        // **A jug with grain in it is not dipped.** The swap below takes
        // the jug out of its slot and puts a jug of water in, and the grain
        // that rode in the old jug's `damage` went with it -- the barrel's
        // `exchange_in_hand` already refused this, and the river did not.
        let holding_goods = state
            .inventory
            .slots()
            .get(slot)
            .copied()
            .flatten()
            .and_then(|stack| primitive_shared::inventory::jug_contents(&stack))
            .is_some();
        if holding_goods {
            drop(state);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::JugNotEmpty });
            return;
        }
        let single = state.inventory.count_in(slot) == 1;
        if !single && !state.inventory.has_room_for(full, 1) {
            drop(state);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
            return;
        }
        if state.inventory.take_from(slot, 1) == 0 {
            return;
        }
        let left = state.inventory.add(full, 1);
        debug_assert_eq!(left, 0, "a filled jug had nowhere to go");
        state.inventory_dirty = true;
        true
    };
    if filled {
        send_inventory(handle);
        // Two litres more to carry, which is the cost of the decision.
        refresh_carried_weight(handle);
        let _ = ctx;
    }
}

/// Takes what grows on a cell -- the apples on an apple tree -- into the
/// pack, and leaves the plant standing to grow them again.
///
/// **Exactly what breaking the cell gives, and exactly what breaking it
/// leaves** (`types::block_drop`, `block_drop_count`, `block_residue`), so
/// a right click and a punch can never disagree about how many apples a
/// leaf holds or what it turns into. What differs is where the apple goes:
/// into the hand that picked it rather than onto the ground below, which
/// is the difference between picking an apple and knocking one down.
///
/// **A full pack leaves the apple on the tree**, and says so. Dropping it at
/// the player's feet instead was not taken: an apple that falls out of a
/// right click is an apple to go down on your knees for, which is the chore
/// picking was asked for to remove -- and the tree would be picked bare
/// into a pack that took nothing.
///
/// The cell is written first and put back if the pack refuses, for the
/// reason `use_barrel` gives: a cell is one value, and nobody is told about
/// either write until both have held.
fn pick_by_hand(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    use primitive_shared::types::{block_drop, block_drop_count, block_residue};

    // Moss is a pick of its own: what comes off is a wad of moss and what is
    // left is the block without it, where breaking it would give the log.
    let (fruit, count, left) = match primitive_shared::ground::scraped(block) {
        Some(bare) => (primitive_shared::types::BLOCK_MOSS, 1, bare),
        None => {
            let Some(fruit) = block_drop(block) else {
                return;
            };
            (fruit, u32::from(block_drop_count(block)), block_residue(block))
        }
    };
    if !ctx.world.set_block(at.0, at.1, at.2, left) {
        return;
    }
    let picked = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.inventory.has_room_for(fruit, count) {
            let spare = state.inventory.add(fruit, count);
            debug_assert_eq!(spare, 0, "an apple the pack had room for did not go in");
            state.inventory_dirty = true;
            true
        } else {
            false
        }
    };
    if !picked {
        let restored = ctx.world.set_block(at.0, at.1, at.2, block);
        debug_assert!(restored, "apples written off a moment ago could not be put back");
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    // The growth mechanic hears about the picked leaf here, and that is
    // what starts it filling again.
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, left);
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// Shakes the palm whose trunk is at `at`: one cluster of coconuts comes
/// down out of its crown, as an item where it hung, and the frond it hung
/// in is left to fruit again.
///
/// **One cluster a shake, not the whole crown.** A crown that emptied at a
/// touch would make picking pointless and the palm a vending machine; one at
/// a time is the same count a player gets by climbing, paid for in nuts
/// chased down a beach instead of in a climb.
///
/// The crown is found the way the palm grew it: up the trunk one piece at a
/// time, straight up or one column across (`worldgen::place_palm`), and the
/// nuts hang beside the top piece. What comes down and what is left are the
/// break's own answers (`block_drop`, `block_residue`), so a shake, a pick
/// and a felling all agree about a cluster.
fn shake_palm(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    use primitive_shared::types::{
        block_drop, block_drop_count, block_kind, block_residue, BLOCK_PALM_COCONUTS, BLOCK_PALM_TRUNK,
    };
    const STEPS: [(i32, i32); 9] = [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
    let is_trunk = |(x, y, z): (i32, i32, i32)| {
        ctx.world.cached_block(x, y, z).is_some_and(|b| block_kind(b) == BLOCK_PALM_TRUNK)
    };
    let mut top = at;
    for _ in 0..felling::MAX_TRUNK {
        match STEPS.iter().map(|&(dx, dz)| (top.0 + dx, top.1 + 1, top.2 + dz)).find(|&c| is_trunk(c)) {
            Some(next) => top = next,
            None => break,
        }
    }
    let mut cluster = None;
    'search: for dy in (-1..=1).rev() {
        for &(dx, dz) in &STEPS {
            let cell = (top.0 + dx, top.1 + dy, top.2 + dz);
            if let Some(block) = ctx.world.cached_block(cell.0, cell.1, cell.2) {
                if block_kind(block) == BLOCK_PALM_COCONUTS {
                    cluster = Some((cell, block));
                    break 'search;
                }
            }
        }
    }
    let Some((cell, block)) = cluster else {
        return;
    };
    let Some(nut) = block_drop(block) else {
        return;
    };
    let left = block_residue(block);
    if !ctx.world.set_block(cell.0, cell.1, cell.2, left) {
        return;
    }
    ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
        nut,
        u32::from(block_drop_count(block)),
        (f64::from(cell.0 as f32 + 0.5), f64::from(cell.1 as f32 + 0.5), f64::from(cell.2 as f32 + 0.5)),
        (0.0, 0.0, 0.0),
        None,
        Instant::now(),
    );
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, cell.0, cell.1, cell.2);
    broadcast_block(ctx, cell, left);
}

/// Pours the jug in hand into a barrel, or dips it full from one.
///
/// Decided by the jug, the way a river decides: a full one pours, an
/// empty one dips, and anything else is not a gesture at a barrel. What
/// the barrel holds afterwards is `types::barrel_after_pouring` and
/// `barrel_after_dipping`, tested there; this is the doing.
///
/// **The barrel is written first and put back if the hand refuses.**
/// The other order -- empty the jug, then find the cell will not take
/// the water -- leaves a jug poured into nothing, and undoing a pack is
/// the harder half: the empty jug `add` produced need not be in the slot
/// the full one left. A cell is one value, so undoing it is writing the
/// old value back, and nobody is told about either write until both have
/// held.
fn use_barrel(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    barrel: primitive_shared::types::BlockId,
    held: Option<primitive_shared::types::BlockId>,
) {
    use primitive_shared::types::{
        barrel_after_dipping, barrel_after_pouring, block_kind, BLOCK_JUG, BLOCK_JUG_WATER,
    };

    // **An empty hand drinks.** It used to be no gesture at all -- "the
    // water is reached with a jug, the way it is carried" -- and that made
    // the barrel the one body of water in the game a thirsty player could
    // stand at and not drink from, while the pond behind it let them.
    let Some(held) = held else {
        // ...unless there is grain in it, which is said rather than being
        // "the barrel is empty" to somebody looking at a full one.
        if primitive_shared::types::barrel_goods(barrel).is_some() {
            refuse_at_barrel(handle, primitive_shared::types::BarrelRefusal::HoldsGrain);
            return;
        }
        drink_from_barrel(ctx, handle, at, barrel);
        return;
    };
    // **A jug with grain in it, or any jug at a barrel of grain**, is the
    // harvest's half of the barrel. Asked of the pack and not of `held`,
    // because `held` is an id and a jug's contents are not in its id: an
    // empty jug and a jug of sixteen seeds are the same `BLOCK_JUG` here.
    // An empty jug at a barrel of *water* is not this and goes on to be
    // dipped below.
    if block_kind(held) == BLOCK_JUG {
        let inside = selected_jug_contents(handle);
        if inside.is_some() || primitive_shared::types::barrel_goods(barrel).is_some() {
            pour_or_scoop_grain(ctx, handle, at, barrel, inside);
            return;
        }
    }
    if block_kind(held) == BLOCK_JUG_WATER && primitive_shared::types::barrel_goods(barrel).is_some() {
        refuse_at_barrel(handle, primitive_shared::types::BarrelRefusal::HoldsGrain);
        return;
    }
    let (next, got) = match block_kind(held) {
        BLOCK_JUG_WATER => match barrel_after_pouring(barrel, held) {
            Some(next) => (next, BLOCK_JUG),
            None => {
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::BarrelFull });
                return;
            }
        },
        BLOCK_JUG => match barrel_after_dipping(barrel) {
            Some((next, full)) => (next, full),
            None => {
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::BarrelEmpty });
                return;
            }
        },
        _ => return,
    };
    if !ctx.world.set_block(at.0, at.1, at.2, next) {
        return;
    }
    if !exchange_in_hand(handle, held, got) {
        let restored = ctx.world.set_block(at.0, at.1, at.2, barrel);
        debug_assert!(restored, "a barrel written a moment ago could not be written back");
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, next);
    send_inventory(handle);
    // Two litres more or less to carry.
    refresh_carried_weight(handle);
}

/// Tells a player why the barrel did nothing.
fn refuse_at_barrel(handle: &Arc<players::PlayerHandle>, why: primitive_shared::types::BarrelRefusal) {
    handle.send(ServerMessage::Error(why.words().to_string()));
}

/// What the jug in the selected slot holds, if it is a jug holding
/// anything.
fn selected_jug_contents(
    handle: &Arc<players::PlayerHandle>,
) -> Option<(primitive_shared::types::BlockId, u32)> {
    let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
    let stack = state.inventory.slots().get(state.selected_slot).copied().flatten()?;
    primitive_shared::inventory::jug_contents(&stack)
}

/// Empties the full jug of grain in hand into a barrel, or scoops a jug's
/// measure of grain out of one into the empty jug in hand.
///
/// What the barrel holds afterwards, and every refusal, is
/// `types::barrel_after_pouring_goods` and `barrel_after_scooping`, tested
/// there; this is the doing, in `use_barrel`'s order and for its reason --
/// **the cell first and put back if the pack refuses**, because a cell is
/// one value to write back and a pack is not.
///
/// The pack is checked again under its lock rather than trusted from
/// `inside`: between the look and the write the slot can have changed --
/// another message from the same player, a death -- and a jug that is no
/// longer there must not have been poured.
fn pour_or_scoop_grain(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    barrel: primitive_shared::types::BlockId,
    inside: Option<(primitive_shared::types::BlockId, u32)>,
) {
    use primitive_shared::inventory::{filled_jug, jug_contents, Stack, JUG_UNITS};
    use primitive_shared::types::{barrel_after_pouring_goods, barrel_after_scooping, BLOCK_JUG};

    let (next, jug_after) = match inside {
        Some((goods, units)) => match barrel_after_pouring_goods(barrel, goods, units) {
            Ok(next) => (next, Stack::new(BLOCK_JUG, 1)),
            Err(why) => return refuse_at_barrel(handle, why),
        },
        None => match barrel_after_scooping(barrel) {
            Ok((next, goods)) => (next, filled_jug(goods, JUG_UNITS)),
            Err(why) => return refuse_at_barrel(handle, why),
        },
    };
    if !ctx.world.set_block(at.0, at.1, at.2, next) {
        return;
    }
    let swapped = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        let still = state
            .inventory
            .slots()
            .get(slot)
            .copied()
            .flatten()
            .filter(|stack| stack.block == BLOCK_JUG && stack.count == 1)
            .map(|stack| jug_contents(&stack));
        if state.vitals.is_dead() || still != Some(inside) {
            false
        } else {
            // Emptied and refilled rather than merged into, for the reason
            // `pour_into_jug` gives: a jug stacks to one, so there is no room
            // to merge a new `damage` into.
            state.inventory.take_slot(slot);
            let rejected = state.inventory.put_in_slot(slot, jug_after);
            debug_assert!(rejected.is_none(), "a jug did not fit back into its own slot");
            state.inventory_dirty = true;
            true
        }
    };
    if !swapped {
        let restored = ctx.world.set_block(at.0, at.1, at.2, barrel);
        debug_assert!(restored, "a barrel written a moment ago could not be written back");
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, next);
    send_inventory(handle);
    // Sixteen grains more or less to carry.
    refresh_carried_weight(handle);
}

/// Drinks from a barrel with a bare hand.
///
/// One jug off the level and a jug's worth into the player -- see
/// `types::BARREL_DRINK_JUGS` for why exactly that, and not a river's
/// mouthful for nothing. The swallow is `Vitals::drink_water`, the call a
/// river and a jug make, so a barrel of pond water costs what a pond
/// costs and a barrel of the sea makes a player thirstier exactly as the
/// sea does. There is no second set of rules to drift.
///
/// **The barrel is written first and put back if the body refuses**,
/// the order `use_barrel` keeps for the reason written there. Here the
/// refusal is a player who is not thirsty: `drink_water` answers no
/// within a sip of full, and a barrel lowered for a drink nobody took is
/// a jug of water poured on the ground. The other order cannot be undone
/// at all -- water credited to a body does not come back out of it.
fn drink_from_barrel(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    barrel: primitive_shared::types::BlockId,
) {
    let Some((next, kind)) = primitive_shared::types::barrel_after_drinking(barrel) else {
        // The words dipping an empty jug gets, because it is the same
        // discovery.
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::BarrelEmpty });
        return;
    };
    if !ctx.world.set_block(at.0, at.1, at.2, next) {
        return;
    }
    let drank = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let drank = !state.vitals.is_dead()
            && state
                .vitals
                .drink_water(kind, primitive_shared::body::JUG_HYDRATION);
        if drank {
            state.gesture.made(primitive_shared::protocol::Action::Drink);
        }
        drank
    };
    if !drank {
        let restored = ctx.world.set_block(at.0, at.1, at.2, barrel);
        debug_assert!(restored, "a barrel written a moment ago could not be written back");
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    // The level everybody at the barrel sees drop -- the one piece of
    // feedback a bare-handed drink has that a river does not.
    broadcast_block(ctx, at, next);
    send_body(handle);
    warn_about_water(handle, kind);
    // A jug's worth, told to the mods the way a jug is.
    fire_plugin_hook(
        ctx,
        "on_drink",
        vec![plugins::Value::Int(handle.id as i64)],
        None,
    );
}

/// Tells a player they have just swallowed something that will cost them.
///
/// **Said out loud, once, at the moment of the mistake.** What it costs
/// comes slowly (`body::SICKNESS_PER_SECOND`), and a slow cost nobody was
/// warned about is a bug report rather than a lesson. One copy of the
/// words, because a river and a barrel of the same pond are the same
/// mistake and saying it two ways would make them sound like two.
fn warn_about_water(handle: &Arc<players::PlayerHandle>, kind: primitive_shared::body::Water) {
    let words = match kind {
        primitive_shared::body::Water::Fresh => return,
        primitive_shared::body::Water::Salt => "the sea is salt, and you are thirstier for it",
        primitive_shared::body::Water::Standing => "the water is stale, and it sits badly",
    };
    handle.send(ServerMessage::Error(words.to_string()));
}

/// Swaps one `gave` in the selected slot for one `got`, if the pack has
/// room for it; `false`, with nothing changed, if not.
///
/// **A jug with something poured into it is not an empty jug here.**
/// Grain rides in the slot's `damage` (`inventory::jug_contents`), and a
/// jug of grain dipped in a barrel would come out a jug of water with the
/// grain simply gone -- contents deleted by a gesture that was only ever
/// about water.
fn exchange_in_hand(
    handle: &Arc<players::PlayerHandle>,
    gave: primitive_shared::types::BlockId,
    got: primitive_shared::types::BlockId,
) -> bool {
    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
    if state.vitals.is_dead() {
        return false;
    }
    let slot = state.selected_slot;
    let Some(stack) = state.inventory.slots().get(slot).copied().flatten() else {
        return false;
    };
    if stack.block != gave {
        return false;
    }
    if primitive_shared::inventory::jug_contents(&stack).is_some() {
        drop(state);
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::JugNotEmpty });
        return false;
    }
    // A slot holding one is always fine, because taking it frees the
    // slot the other goes back into -- the drink's rule.
    let single = state.inventory.count_in(slot) == 1;
    if !single && !state.inventory.has_room_for(got, 1) {
        drop(state);
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
        return false;
    }
    if state.inventory.take_from(slot, 1) == 0 {
        return false;
    }
    let left = state.inventory.add(got, 1);
    debug_assert_eq!(left, 0, "a jug taken from its slot had nowhere to go back to");
    state.inventory_dirty = true;
    true
}

/// Sends a tick's worth of block changes to everybody who can see them.
///
/// Grouped by chunk, so each player gets one batched message per
/// affected chunk rather than one per cell, through the same subscriber
/// index manual edits use. Serialised once per chunk, not once per
/// recipient: every subscriber gets the same bytes, and the shape this
/// replaced deep-cloned the whole change list for each of them just so
/// every writer task could bincode an identical copy.
///
/// A function rather than the loop it was, because four things produce
/// changes now -- the falling sand, the water, the fires going out and
/// the bushes filling -- and four copies of this would be four places to
/// forget the batching in.
pub(crate) fn broadcast_changes(ctx: &Arc<Context>, changes: Vec<BlockChange>) {
    if changes.is_empty() {
        return;
    }
    // **Mods hear about every cell, players about every chunk.** The
    // batching below is a network decision -- one message per chunk
    // rather than one per cell -- and a mod watching for a particular
    // block would have to undo it to be useful. Fired first and with no
    // lock held, which is the rule this whole subsystem rests on.
    for change in &changes {
        block_changed(
            ctx,
            (change.global_x, change.global_y, change.global_z),
            change.block_id,
        );
    }
    let mut by_chunk: std::collections::HashMap<ChunkPos, Vec<BlockChange>> =
        std::collections::HashMap::new();
    for change in changes {
        let (pos, _, _) = ChunkPos::from_global(change.global_x, change.global_z);
        by_chunk.entry(pos).or_default().push(change);
    }
    for (pos, batch) in by_chunk {
        let Some(frame) = players::frame(&ServerMessage::BlockUpdates(batch)) else {
            continue;
        };
        for subscriber in ctx.registry.subscribers(pos) {
            subscriber.send_raw(Arc::clone(&frame));
        }
    }
}

/// Blood at `at`, told to everybody near enough to see it -- the player it
/// came off and the player who drew it included. See `ServerMessage::Blood`
/// for why nobody predicts it, and why nothing but a blow or an open cut
/// sends it.
///
/// Out to the snapshot's own radius, so blood is seen by exactly the people
/// who can see the body it came off.
pub(crate) fn broadcast_blood(ctx: &Arc<Context>, at: (f64, f64, f64), drops: u8) {
    if drops == 0 {
        return;
    }
    let radius = ctx.settings.interest_radius_blocks;
    for handle in ctx.registry.handles() {
        let near = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let (dx, dy, dz) = (
                state.position.0 - at.0,
                state.position.1 - at.1,
                state.position.2 - at.2,
            );
            dx * dx + dy * dy + dz * dz <= f64::from(radius * radius)
        };
        if near {
            handle.send(ServerMessage::Blood { at, drops });
        }
    }
}

/// A body driven onto sharpened stakes at `at`, told to everybody near
/// enough to hear it -- the victim included. See `ServerMessage::Staked`.
///
/// Out to the same radius blood is, for the same reason: it is heard by the
/// people who can see the body it happened to.
pub(crate) fn broadcast_staked(ctx: &Arc<Context>, at: (f64, f64, f64)) {
    let radius = f64::from(ctx.settings.interest_radius_blocks);
    for handle in ctx.registry.handles() {
        let near = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let (dx, dy, dz) = (state.position.0 - at.0, state.position.1 - at.1, state.position.2 - at.2);
            dx * dx + dy * dy + dz * dz <= radius * radius
        };
        if near {
            handle.send(ServerMessage::Staked { at });
        }
    }
}

/// Where the blood of a blow comes from on a body standing at `feet`: the
/// chest, a third of a metre under the eye.
///
/// **The point the client sprayed its own blood from, and for its reason.**
/// A burst at the eye is a burst inside the near plane of the player it came
/// off -- half of it clipped and the rest a smear across the frame -- and a
/// third of a metre down it arcs through the bottom of their view instead.
fn chest_of(feet: (f32, f32, f32)) -> (f32, f32, f32) {
    (feet.0, feet.1 + primitive_shared::geometry::EYE_HEIGHT - 0.35, feet.2)
}

/// Sends one block change to everybody who has the chunk loaded, and
/// tells the mods.
///
/// **The choke point for `Event::BlockChanged`**, with
/// `broadcast_changes` above. Every path that alters a cell -- a
/// player's edit, the falling sand, the water, a fire going out, a mod's
/// own `set_block` -- ends up in one of these two, which is what makes
/// "anything at all changed a cell" an event that can be honestly
/// promised rather than one that is fired from wherever somebody
/// remembered to.
pub(crate) fn broadcast_block(ctx: &Arc<Context>, at: (i32, i32, i32), block_id: primitive_shared::types::BlockId) {
    block_changed(ctx, at, block_id);
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    let change = BlockChange {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        block_id,
    };
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(ServerMessage::BlockUpdate(change));
    }
}

/// Eats what is in a slot.
///
/// The slot rather than a block id, because the server's copy of the
/// pack is the real one. Nothing is spent unless it actually did
/// something: a haunch of meat eaten at full is an item destroyed, which
/// is the same class of bug as a craft that consumes its ingredients and
/// produces nothing.
/// Answers whether anything was actually eaten, which is what
/// `FoodApi::eat` reports back to a mod -- and what tells a mod that
/// cancelled its own `PlayerAte` that it was refused.
pub(crate) fn eat_from_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
) -> bool {
    // **A full jug is drunk rather than eaten.** The same gesture and
    // the same message, because it is the same thing from the player's
    // side -- a thing in your pack that you put in your mouth -- and a
    // second `Drink` message would be a second rate limit, a second
    // reach check and a second place to forget the "is it worth
    // spending" rule below.
    {
        let held = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.inventory.block_in(slot)
        };
        if let Some(empty) = held.and_then(primitive_shared::types::emptied_vessel) {
            drink_from_slot(ctx, handle, slot, empty);
            return true;
        }
    }

    // What is about to go in the mouth, read and the lock let go, so the
    // mods can be asked with nothing held.
    let Some(about_to_eat) = ({
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inventory.block_in(slot)
    }) else {
        return false;
    };
    if !player_ate(ctx, handle.id, about_to_eat, slot) {
        return false;
    }

    let (ate, outcome) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(stack) = state.inventory.slots().get(slot).copied().flatten() else {
            return false;
        };
        let block = stack.block;
        // **The bowl comes back, so it has to have somewhere to go**, and
        // that is asked before anything is eaten -- the jug's rule
        // (`a_jug_with_nowhere_to_put_the_empty_one_is_not_drunk_for_free`)
        // the other way round: eating first and then finding the pack full
        // would be a bowl that stopped existing. The last stew in a stack
        // leaves its own slot for its bowl. See `food::served_in`.
        let bowl = primitive_shared::food::served_in(block);
        if let Some(bowl) = bowl {
            if stack.count > 1 && !state.inventory.has_room_for(bowl, 1) {
                drop(state);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NowhereForBowl });
                return false;
            }
        }
        // How well it was cooked, off the stack rather than off the id: a
        // fine loaf and a poor one are the same block (`quality`).
        let outcome = state.vitals.eat_made(block, stack.quality());
        if matches!(outcome, survival::Outcome::Unchanged) {
            // Either it is not food or there is no room for it. Both are
            // "nothing happened", and the client greys the gesture out
            // for the first, so this is not worth an error message.
            return false;
        }
        state.inventory.take_from(slot, 1);
        if let Some(bowl) = bowl {
            let left = state.inventory.add(bowl, 1);
            debug_assert_eq!(left, 0, "the room for the bowl was asked for and is gone");
        }
        state.inventory_dirty = true;
        state.gesture.made(primitive_shared::protocol::Action::Eat);
        (true, outcome)
    };
    if ate {
        // **Said out loud, once, at the moment of the mistake.** The
        // same rule the stale river follows: what illness costs comes
        // slowly (`body::SICKNESS_PER_SECOND`), and a slow cost nobody
        // was warned about is a bug report rather than a lesson.
        let illness = primitive_shared::food::sickness_seconds(about_to_eat);
        if illness > 0.0 {
            handle.send(ServerMessage::Error(
                match primitive_shared::types::block_kind(about_to_eat) {
                    primitive_shared::types::BLOCK_TOADSTOOL => {
                        "the cap was the wrong one, and you know it now"
                    }
                    primitive_shared::types::BLOCK_ROTTEN => {
                        "it had turned, and it sits badly"
                    }
                    _ => "raw flesh, and your stomach says so",
                }
                .to_string(),
            ));
        }
        send_inventory(handle);
        send_nourishment(handle);
        // Weight changed, and weight decides what a fall costs.
        refresh_carried_weight(handle);
        // ...and whatever it did to them, which for one mushroom in this
        // world is take health -- possibly the last of it. Reported
        // through the one path that knows how to announce a death.
        report_vitals(ctx, handle, outcome);
    }
    ate
}

// ---- what a player is wearing ----
//
// Two gestures and nothing else. There is no *moving* a garment between
// slots, because the slot a garment goes in is a fact about the garment
// (see `primitive_shared::equipment::slot_of`) -- so the whole surface
// is "put this on" and "take that off", and neither of them carries a
// destination for a client to get wrong or to lie about.

/// Puts on whatever is in a slot of the player's pack.
///
/// Whatever comes off goes back into the pack, and if it will not fit
/// the swap is refused outright rather than half done. That is the one
/// rule this function actually enforces: **a gesture that cannot be
/// completed must change nothing.** Taking the tunic off first and then
/// discovering there is nowhere to put it is how a player's second-best
/// coat stops existing.
pub(crate) fn equip_from_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
) {
    let changed: Option<primitive_shared::equipment::Slot> = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let Some(stack) = state.inventory.slots().get(slot).copied().flatten() else {
            return;
        };
        let Some(body_slot) = primitive_shared::equipment::slot_of(stack.block) else {
            return; // not a garment; the client should not have offered it
        };
        // Room for what is coming off, checked *before* anything moves.
        // The slot being vacated counts, which is what makes swapping a
        // helmet for a helmet work in a completely full pack.
        let displaced = state.equipment.in_slot(body_slot);
        if let Some(coming_off) = displaced {
            let single = state.inventory.count_in(slot) == 1;
            let room = single || state.inventory.has_room_for(coming_off.block, 1);
            if !room {
                drop(state);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NoRoomToTakeOff });
                return;
            }
        }
        // Out of the pack first, so the slot is free for the swap.
        let taken = state.inventory.take_slot(slot);
        let Some(taken) = taken else { return };
        // A garment stacks to one, so `take_slot` cannot have handed
        // back more -- but if a future block table says otherwise, the
        // remainder goes back rather than being eaten.
        if taken.count > 1 {
            state.inventory.add(taken.block, taken.count - 1);
        }
        let coming_off = state.equipment.wear(primitive_shared::inventory::Stack::worn(
            taken.block,
            1,
            taken.damage,
        ));
        // **The squares appear before the displaced piece looks for a
        // home.** Putting a rucksack on is the one equip that changes
        // how big the pack is, and doing it after the line below would
        // mean swapping one rucksack for another in a full pack fails
        // for want of a square that was about to exist.
        if body_slot == primitive_shared::equipment::Slot::Back {
            state.inventory.open_backpack();
        }
        if let Some(old) = coming_off {
            let left = state
                .inventory
                .put_in_slot(slot, old)
                .and_then(|rejected| {
                    // The freed slot was taken by something in between,
                    // which cannot happen under one lock -- belt and
                    // braces, and `add` cannot delete anything.
                    let over = state.inventory.add(rejected.block, rejected.count);
                    (over > 0).then_some(rejected.block)
                });
            debug_assert!(left.is_none(), "a displaced garment had nowhere to go");
        }
        state.inventory_dirty = true;
        state.equipment_dirty = true;
        Some(body_slot)
    };
    if let Some(slot) = changed {
        finish_equipment_change(ctx, handle, slot);
    }
}

/// Takes a piece off and puts it in the pack.
pub(crate) fn unequip_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
) {
    let Some(body_slot) = primitive_shared::equipment::Slot::from_index(slot) else {
        return; // an index off the end of the body
    };
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let Some(worn) = state.equipment.in_slot(body_slot) else {
            return;
        };
        // **A rucksack with anything in it stays on.** The ten squares
        // it lends vanish the moment it comes off, and there is no good
        // answer to "where do the stacks in them go" -- see
        // `Inventory::close_backpack`, which argues the three bad ones.
        // Refused here, before anything moves, so the player gets a line
        // of text and a pack exactly as they left it.
        //
        // Asked against a *trial* copy rather than against the real one,
        // because `close_backpack` shortens the pack when it succeeds
        // and the piece still has to find a square afterwards.
        if body_slot == primitive_shared::equipment::Slot::Back {
            let mut trial = state.inventory.clone();
            if !trial.close_backpack() {
                drop(state);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::EmptyRucksackFirst });
                return;
            }
            // ...and the square it lands in has to be one of the body's
            // own. `has_room_for` below counts the rucksack's ten as
            // room, which for every other garment is true and here is
            // the one case where it is not: they are about to stop
            // existing.
            if (0..primitive_shared::inventory::SLOTS)
                .all(|square| state.inventory.block_in(square).is_some())
            {
                drop(state);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
                return;
            }
        }
        // Same rule as above: nowhere to put it means nothing happens,
        // rather than a garment that stops existing.
        if !state.inventory.has_room_for(worn.block, 1) {
            drop(state);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
            return;
        }
        let Some(taken) = state.equipment.take(body_slot) else {
            return;
        };
        // Through `put_in_slot` into the first free square rather than
        // `add`, because `add` merges by block and a part-worn garment
        // must not merge with a fresh one -- the wear is on the stack.
        let mut placed = false;
        for square in 0..primitive_shared::inventory::SLOTS {
            if state.inventory.block_in(square).is_none() {
                state.inventory.put_in_slot(square, taken);
                placed = true;
                break;
            }
        }
        if !placed {
            // `has_room_for` said there was room, so this is
            // unreachable -- and if it ever is reached, the garment goes
            // back on rather than nowhere.
            state.equipment.wear(taken);
            return;
        }
        // **Last, and only once the rucksack itself has a square.** The
        // squares go away here rather than at the check above, because
        // between the two the piece has to be put somewhere and the
        // `!placed` arm puts it back on -- shortening the pack before
        // that would leave a player wearing a rucksack whose squares
        // were gone.
        if body_slot == primitive_shared::equipment::Slot::Back {
            let closed = state.inventory.close_backpack();
            debug_assert!(closed, "a rucksack that was empty a moment ago is not");
        }
        state.inventory_dirty = true;
        state.equipment_dirty = true;
        true
    };
    if changed {
        finish_equipment_change(ctx, handle, body_slot);
    }
}

// ---- what a jug carries when it is not carrying water ----
//
// Two gestures, and they are each other's inverse: pour a stack of dry
// goods into an empty jug, and tip the jug back out into the pack.
// What is in the jug lives in the slot's `damage` field -- see
// `inventory::jug_contents`, which is also where the encoding and the
// two designs it beat are written down.
//
// **Both of them are re-decided here from the server's own copy of the
// pack**, exactly as `equip_from_slot` is. The client offers the
// gesture, which means the client has already asked all three
// questions; asking them again costs two table lookups and is the
// difference between a rule and a suggestion. A client that skipped
// them would otherwise pour meat into a jug and stop its rot clock (see
// `types::pours` for why that is the one thing this must not allow).

/// Pours everything in `from` that will fit into the jug in `jug`.
///
/// Nothing is destroyed on any path out of this: what does not fit
/// stays in the slot it came from, so a half-filled jug and a
/// part-spent stack is the honest result of pouring twenty units into
/// a sixteen-unit jug.
pub(crate) fn pour_into_jug(handle: &Arc<players::PlayerHandle>, from: usize, jug: usize) {
    let poured = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        // Pouring a slot into itself is not a gesture; it is also the
        // one index pair that would read the source after the jug had
        // already been rewritten.
        if from == jug {
            return;
        }
        let slots = state.inventory.slots();
        let (Some(source), Some(vessel)) = (
            slots.get(from).copied().flatten(),
            slots.get(jug).copied().flatten(),
        ) else {
            return;
        };
        if !primitive_shared::types::pours(source.block) {
            return;
        }
        if primitive_shared::types::block_kind(vessel.block)
            != primitive_shared::types::BLOCK_JUG
        {
            return;
        }
        // Empty, or already holding the same goods -- topping a jug up
        // is the commonest second pour and refusing it would mean
        // tipping it out first. A jug holding *something else* is
        // refused rather than swapped: one number describes one kind of
        // contents, and quietly replacing them is quietly deleting them.
        // The rule itself is `inventory::jug_room`, which a jug set down
        // on a shelf is asked by too -- one answer to "may this go in",
        // wherever the jug is.
        let already = primitive_shared::inventory::jug_contents(&vessel);
        let room = primitive_shared::inventory::jug_room(already, source.block);
        let moved = room.min(source.count);
        if moved == 0 {
            return;
        }
        let now = already.map_or(0, |(_, count)| count) + moved;
        let filled = primitive_shared::inventory::filled_jug(source.block, now);
        // Out of the source first and then straight in, under one lock:
        // there is no moment here where the goods are in neither slot
        // that anything else could observe.
        state.inventory.take_from(from, moved);
        // **Emptied and refilled rather than merged into.**
        // `put_in_slot` on a slot that already holds a jug takes the
        // "same block" arm, and that arm keeps the target's own
        // `damage` and moves nothing -- with a stack limit of one there
        // is no room, so the new contents would be silently dropped.
        // Taking the slot first makes it the empty arm, which writes
        // the whole stack including its `damage`.
        state.inventory.take_slot(jug);
        let rejected = state.inventory.put_in_slot(jug, filled);
        debug_assert!(rejected.is_none(), "a filled jug did not fit its own slot");
        state.inventory_dirty = true;
        true
    };
    if poured {
        send_inventory(handle);
        // The jug is heavier by exactly what went into it. See
        // `Stack::weight`, which is what makes that true.
        refresh_carried_weight(handle);
    }
}

/// Takes what is in the jug in `jug` out into the pack slot `to` -- all of
/// it, or half.
///
/// **The gesture opening a jug needed.** `empty_jug` tips everything into
/// wherever there is room, which is a shift-click; a player looking into a
/// jug of seeds and dragging a handful onto the square beside the hoe
/// wants them *there*, and some of them. What will not fit in `to` stays in
/// the jug.
///
/// `to` must be empty or already hold the same goods. Anything else would
/// be a swap with the inside of a jug, which is two pours at once and the
/// second one unchecked -- an axe swapped into a jug is exactly the vessel
/// inside a vessel `inventory::jug_room` exists to refuse.
///
/// One lock, the slot filled and the jug rewritten together, so there is
/// no moment in which the goods are in both places or in neither.
pub(crate) fn take_from_jug(
    handle: &Arc<players::PlayerHandle>,
    jug: usize,
    to: usize,
    half: bool,
) {
    use primitive_shared::inventory::{filled_jug, jug_contents, Stack, SLOTS};
    let taken = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() || jug == to || to >= SLOTS {
            return;
        }
        let Some(vessel) = state.inventory.slots().get(jug).copied().flatten() else {
            return;
        };
        let Some((goods, count)) = jug_contents(&vessel) else {
            return; // not a jug, or an empty one
        };
        match state.inventory.slots()[to] {
            None => {}
            Some(there) if there.block == goods => {}
            Some(_) => return,
        }
        // Rounded up, the rule every other half gesture follows: half of
        // one seed is the seed.
        let wanted = if half { count.div_ceil(2) } else { count };
        let left = state.inventory.put_in_slot(to, Stack::new(goods, wanted));
        let moved = wanted - left.map_or(0, |left| left.count);
        if moved == 0 {
            return;
        }
        // Emptied and refilled rather than merged into, for the reason
        // `pour_into_jug` gives: a stack limit of one leaves no room to
        // merge a new `damage` into.
        state.inventory.take_slot(jug);
        let rejected = state.inventory.put_in_slot(jug, filled_jug(goods, count - moved));
        debug_assert!(rejected.is_none(), "a jug did not fit back into its own slot");
        state.inventory_dirty = true;
        true
    };
    if taken {
        // No weight to refresh: the grain moved from a jug in the pack to
        // a square of the same pack, and weighs what it weighed.
        send_inventory(handle);
    }
}

/// Tips a jug out into the pack.
///
/// What will not fit stays in the jug. The alternative -- refusing the
/// whole gesture unless every unit fits -- reads as a jug that cannot
/// be emptied at all when a player most needs the thing in it, and the
/// alternative to *that* is goods on the floor of a screen with no
/// floor.
pub(crate) fn empty_jug(handle: &Arc<players::PlayerHandle>, slot: usize) {
    let tipped = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let Some(vessel) = state.inventory.slots().get(slot).copied().flatten() else {
            return;
        };
        let Some((block, count)) = primitive_shared::inventory::jug_contents(&vessel) else {
            return; // not a jug, or an empty one
        };
        // The jug is emptied *first*, and then the goods are offered to
        // the pack. Doing it the other way round means `add` can find
        // room, and then the jug has to be emptied by a second step
        // that has no way of failing but is a second step all the same;
        // this way there is exactly one moment where the goods exist
        // twice and it is inside the same lock as the moment they do
        // not. Whatever `add` hands back goes straight back in below,
        // so no path leaves the pack holding less than it started with.
        state.inventory.take_slot(slot);
        state.inventory.put_in_slot(
            slot,
            primitive_shared::inventory::Stack::new(vessel.block, 1),
        );
        let left = state.inventory.add(block, count);
        if left > 0 {
            // Whatever had nowhere to go goes back in the jug rather
            // than nowhere at all.
            state.inventory.take_slot(slot);
            state
                .inventory
                .put_in_slot(slot, primitive_shared::inventory::filled_jug(block, left));
        }
        state.inventory_dirty = true;
        left < count
    };
    if tipped {
        send_inventory(handle);
        refresh_carried_weight(handle);
    }
}

/// Everything a change of clothes has to update, in one place.
///
/// Three things follow from what a player is wearing: what they are
/// carrying, what they are told they have on, and how heavy they are.
///
/// Two things deliberately need nothing here. **Warmth** is recomputed
/// by the tick loop from the worn set every time, so there is no cached
/// insulation to invalidate. **Speed** is the client's: it reads the
/// same `equipment::Worn::mobility` off the same `EquipmentState` this
/// sends, and the anti-cheat needs no telling because armour can only
/// ever make a player *slower* -- its speed check is an upper bound, and
/// a bound nobody is trying to reach is a bound that stays correct.
///
/// `block` is what is on that part of the body now, or zero for bare --
/// which is what `Event::EquipmentChanged` carries, and is the one thing
/// a mod cannot work out for itself from the fact that something moved.
fn finish_equipment_change(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: primitive_shared::equipment::Slot,
) {
    send_inventory(handle);
    send_equipment(handle);
    refresh_carried_weight(handle);
    let now = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.equipment.in_slot(slot).map(|stack| stack.block)
    };
    equipment_changed(ctx, handle.id, now.unwrap_or(0), slot);
}

/// Pushes the worn set to its owner, if it has changed.
pub(crate) fn send_equipment(handle: &Arc<players::PlayerHandle>) {
    let equipment = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.equipment_dirty {
            return;
        }
        state.equipment_dirty = false;
        state.equipment.clone()
    };
    handle.send(ServerMessage::EquipmentState { equipment });
}

/// Leaves a pat of dung beside a player who is due, if there is anywhere to
/// put one.
///
/// **Beside the feet, never under them.** A block written into the cell a
/// player is standing in is a body inside a block, which the collider pushes
/// out of and the anticheat then corrects -- a jolt for something that should
/// pass unnoticed. A neighbouring cell of air over ground that holds a roof
/// (`blocks_the_sky`, so not water, a fire or a doorway) is enough; with none,
/// nothing is left and the debt waits for the next survey.
fn leave_dung(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, feet: (i32, i32, i32)) {
    use primitive_shared::types::{blocks_the_sky, is_air, BLOCK_DUNG};
    let spot = [(1, 0), (0, 1), (-1, 0), (0, -1)].into_iter().map(|(dx, dz)| (feet.0 + dx, feet.1, feet.2 + dz)).find(|&(x, y, z)| {
        ctx.world.cached_block(x, y, z).is_some_and(is_air)
            && ctx.world.cached_block(x, y - 1, z).is_some_and(blocks_the_sky)
    });
    let Some(at) = spot else {
        return;
    };
    if !ctx.world.set_block(at.0, at.1, at.2, BLOCK_DUNG) {
        return;
    }
    broadcast_block(ctx, at, BLOCK_DUNG);
    handle.state.lock().unwrap_or_else(|e| e.into_inner()).vitals.went();
}

/// Leaves a pat of dung beside a kept animal standing at `feet`.
///
/// **`leave_dung`'s rule, and for its reason**: beside the feet, on ground
/// that holds a roof, never in the cell the body is in -- there it would be
/// an animal standing inside a block. With nowhere to put it, nothing is
/// left: a flock packed wall to wall in a pen of bare stone is a pen with
/// nowhere to foul, and that is not worth a special case.
///
/// **This is the dung the field was waiting for** (`manure_the_furrow_under`):
/// a pen put on a tired strip for a few days leaves it dunged where the flock
/// stood, and the pats are cleared into the furrows. Rejected: dung as an
/// item carried from the pen, for `BLOCK_DUNG`'s reason -- carried muck is a
/// way to foul somebody's house.
fn lay_animal_dung(ctx: &Arc<Context>, feet: (f64, f64, f64)) {
    use primitive_shared::types::{blocks_the_sky, is_air, BLOCK_DUNG};
    let (x, y, z) = (feet.0.floor() as i32, feet.1.floor() as i32, feet.2.floor() as i32);
    let spot = [(1, 0), (0, 1), (-1, 0), (0, -1)].into_iter().map(|(dx, dz)| (x + dx, y, z + dz)).find(|&(x, y, z)| {
        ctx.world.cached_block(x, y, z).is_some_and(is_air)
            && ctx.world.cached_block(x, y - 1, z).is_some_and(blocks_the_sky)
    });
    let Some(at) = spot else {
        return;
    };
    if ctx.world.set_block(at.0, at.1, at.2, BLOCK_DUNG) {
        broadcast_block(ctx, at, BLOCK_DUNG);
    }
}

/// A right click on an animal with something to tend it with
/// (`ClientMessage::TendAnimal`): the animal decides (`Animals::tend`), and
/// the pack pays for what it decided -- one feed, the knife's wear and the
/// wool, a bowl for a bowl of milk.
///
/// **The pack is checked for room before the animal is asked**, for the wool
/// and the milk: a fleece taken off a sheep into a full pack would be wool
/// lost, and the sheep would be bare for three days for nothing.
pub(crate) fn tend_animal(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    target: primitive_shared::protocol::EntityId,
) {
    use primitive_shared::types::{block_kind, is_knife, BLOCK_BOWL, BLOCK_BOWL_MILK, BLOCK_WOOL};
    // **A knife on a horse takes its tack off** (`Animals::unbuckle`). Its
    // own way before the wool's room is checked below: the pack is asked a
    // different question -- room for the bags and their load, not for a
    // fleece -- and a pack full of wool is no reason a saddle cannot come off.
    let knife = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inventory.block_in(state.selected_slot).is_some_and(is_knife)
    };
    if knife && ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).horse_at(target).is_some() {
        horses::unbuckle(ctx, handle, target);
        return;
    }
    let (eye, held, slot) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let eye = (
            state.position.0,
            state.position.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT),
            state.position.2,
        );
        let slot = state.selected_slot;
        let held = state.inventory.block_in(slot);
        let room = match held {
            Some(h) if is_knife(h) => state.inventory.has_room_for(BLOCK_WOOL, primitive_shared::husbandry::FLEECE_WOOL),
            // The milk goes where the bowl was if it was the last one, and
            // needs a slot of its own otherwise.
            Some(h) if block_kind(h) == BLOCK_BOWL => {
                state.inventory.count_in(slot) == 1 || state.inventory.has_room_for(BLOCK_BOWL_MILK, 1)
            }
            _ => true,
        };
        if !room {
            drop(state);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PackFull });
            return;
        }
        (primitive_shared::geometry::narrow(eye), held, slot)
    };
    let tended = {
        let mut animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.tend(target, eye, held)
    };
    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
    match tended {
        animals::Tended::Refused(why) => {
            drop(state);
            handle.send(ServerMessage::Notice { what: why });
            return;
        }
        animals::Tended::Fed { gentled, .. } => {
            state.inventory.take_from(slot, 1);
            if gentled {
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::HorseTakesFood });
            }
        }
        // The saddle or the bags in the hand went onto the horse.
        animals::Tended::Saddled | animals::Tended::Bagged => {
            state.inventory.take_from(slot, 1);
        }
        animals::Tended::Shorn(wool) => {
            state.inventory.wear_tool(slot);
            state.inventory.add(BLOCK_WOOL, wool);
        }
        animals::Tended::Milked => {
            if state.inventory.count_in(slot) == 1 {
                state.inventory.retype_slot(slot, BLOCK_BOWL_MILK);
            } else {
                state.inventory.take_from(slot, 1);
                state.inventory.add(BLOCK_BOWL_MILK, 1);
            }
        }
    }
    state.inventory_dirty = true;
    drop(state);
    send_inventory(handle);
}

/// Digs a pat of dung cleared from `at` into the furrow under it, if there is
/// one: the furrow is dressed, exactly as ash dresses it
/// (`wildfire::dressed`), and a tired one is rested by the same stroke.
///
/// **Muck is what a field was fed with before anything else was**, and the
/// dung already existed with nothing to do (`types::BLOCK_DUNG`). It stays
/// no item -- carrying it about would be a way to foul somebody's house --
/// so the only dung a field gets is dung dropped on it, and a body that is
/// due waits for the open air (`comfort::goes_now`). Where a player goes is
/// now a choice with a use: the fallow strip beside the wheat, rather than
/// the doorstep, where it is filth to everybody's comfort.
///
/// Rejected: dung fertilising the ground it lies on by itself, on a clock.
/// It would be a second fallow timer beside the one the furrow already has,
/// and a pat nobody clears is filth that nobody chose to put to use.
pub(crate) fn manure_the_furrow_under(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    let below = (at.0, at.1 - 1, at.2);
    let Some(ground) = ctx.world.cached_block(below.0, below.1, below.2) else {
        return;
    };
    let Some(field) = primitive_shared::wildfire::dressed(ground) else {
        return;
    };
    if !ctx.world.set_block(below.0, below.1, below.2, field) {
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, below.0, below.1, below.2);
    broadcast_block(ctx, below, field);
}

/// Tells a player how warm they are and how much water they have left.
fn send_body(handle: &Arc<players::PlayerHandle>) {
    let (temperature_c, comfort, hydration, fatigue, recovery, wetness, grime, diet_groups) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let temperature_c = state.vitals.temperature();
        let hydration = state.vitals.hydration_fraction();
        state.body_reported = (temperature_c, hydration, state.vitals.fatigue());
        state.recovery_reported = state.vitals.recovery();
        (
            temperature_c,
            state.vitals.comfort(),
            hydration,
            state.vitals.fatigue(),
            state.vitals.recovery(),
            // **Read on this message's gate, not on one of their own.**
            // Wetness, grime and the diet count are read by the pack
            // screen's health page and by nothing that has to be exact:
            // a number a tenth of a second stale on a page the player
            // opened deliberately is invisible, and three more gates
            // would be three more reasons to send a packet.
            state.vitals.wetness(),
            state.vitals.grime(),
            state.vitals.diet_groups().min(u8::MAX as usize) as u8,
        )
    };
    handle.send(ServerMessage::Body {
        temperature_c,
        comfort,
        hydration,
        fatigue,
        recovery,
        wetness,
        grime,
        diet_groups,
    });
}

/// Tells a player what is wrong with them, if it has changed enough to
/// draw since they were last told. See `Injuries::worth_reporting`.
///
/// Called every tick and almost always free: the comparison is two
/// hundred-byte values side by side, and it is what stands between a
/// bleeding player and a packet a tick.
pub(crate) fn send_injuries(handle: &Arc<players::PlayerHandle>) {
    let injuries = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.vitals.needs_injury_report(&state.injuries_reported) {
            return;
        }
        let now = *state.vitals.injuries();
        state.injuries_reported = now;
        now
    };
    handle.send(ServerMessage::Injuries { injuries });
}

/// Puts what is in a slot of the pack on a part of the body.
///
/// **Read, judged and spent under one lock**, so the bandage that is
/// checked is the bandage that is taken: a pack snapshot changing between
/// "is this a bandage" and "take one" would be a player dressing a wound
/// with a stone. A refusal keeps the item and says why, in words -- a
/// silent nothing on the mannequin reads as the drop having missed, and
/// the player tries again with the same bandage on the same clean leg.
pub(crate) fn treat_from_slot(handle: &Arc<players::PlayerHandle>, slot: usize, part: usize) {
    use primitive_shared::injury::{Part, Refusal, Treatment};
    let Some(part) = Part::from_index(part) else {
        return;
    };
    let verdict = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(block) = state.inventory.block_in(slot) else {
            return;
        };
        match state.vitals.treat(part, block) {
            Ok(kind) => {
                state.inventory.take_from(slot, 1);
                state.inventory_dirty = true;
                Ok(kind)
            }
            Err(refusal) => Err((refusal, Treatment::of(block))),
        }
    };
    match verdict {
        Ok(_) => {
            send_inventory(handle);
            send_injuries(handle);
            // A bandage weighs next to nothing, and the load is still the
            // load a fall is judged against.
            refresh_carried_weight(handle);
        }
        Err((Refusal::NotATreatment, _)) | Err((_, None)) => {}
        Err((Refusal::NothingItHelps, Some(treatment))) => {
            handle.send(ServerMessage::Error(format!(
                "nothing on your {} that a {} would help",
                part.name(),
                treatment.name()
            )));
        }
    }
}

/// A swing at something that is not a player.
///
/// The reach and the cooldown are the same ones a punch at a player
/// uses, because it is the same swing -- see `melee_attack`. What is
/// different is what a tool is worth: an animal is the one thing in the
/// world where the *edge* matters rather than the tier, so a knife hits
/// harder than a fistful of dirt.
pub(crate) fn attack_animal(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    target: primitive_shared::protocol::EntityId,
) {
    use primitive_shared::combat;

    let (from, damage, poison, reach) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let now = Instant::now();
        // **The weapon's own rhythm, not one rhythm for all of them.**
        // A knife swings half again as often as a fist and a spear
        // rather less often, so the refusal here has to ask what is in
        // the hand -- read before the check rather than after it, which
        // is the whole of the change. See `combat::swing_seconds`.
        let held = state.inventory.block_in(state.selected_slot);
        let ready = state.last_swing.is_none_or(|last| {
            now.saturating_duration_since(last).as_secs_f32()
                >= combat::swing_seconds(held) - combat::COOLDOWN_SLACK_SECS
        });
        if !ready {
            return;
        }
        state.last_swing = Some(now);
        // Seen by everybody near, hit or miss: the arm moved either way.
        state.gesture.made(primitive_shared::protocol::Action::Strike);
        (
            (
                state.position.0,
                state.position.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT),
                state.position.2,
            ),
            // ...less what a broken arm cannot put behind it. See
            // `injury::BROKEN_ARM_STRENGTH`.
            hunting_damage(held) * state.vitals.strength_factor(),
            // **Fly agaric on the point, if there is any.** The paste
            // lives in the weapon's own id (`types::is_poisoned`), so
            // this is a question about what is in the hand and nothing
            // else -- read here, inside the lock that already has the
            // pack open.
            if held.is_some_and(primitive_shared::types::is_poisoned) {
                primitive_shared::combat::POISON_SECONDS
            } else {
                0.0
            },
            combat::reach_with(held),
        )
    };

    // **A raft takes the same swing** -- the same cooldown and the same
    // reach, spent above -- and is not an animal: no edge matters, no paste
    // is spent and no blade is worn on timber. See `rafts::strike`.
    if primitive_shared::protocol::entity_source(target)
        == Some(primitive_shared::protocol::EntitySource::Raft)
    {
        rafts::strike(ctx, handle, target, primitive_shared::geometry::narrow(from), reach);
        return;
    }

    let struck = {
        let mut animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.strike_poisoned(target, primitive_shared::geometry::narrow(from), reach, damage, poison)
    };
    // **And one thrust takes it off.** The spear keeps its wear and
    // loses its paste, which is the whole of what "one use" means --
    // done here rather than in `Animals` because the pack is the
    // server's and the animal knows nothing about it. Only a blow that
    // landed spends it: smearing a toadstool onto the air would be a
    // way to lose two of them to a misjudged reach.
    if poison > 0.0 && !matches!(struck, animals::Struck::Missed) {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        if let Some(block) = state.inventory.block_in(slot) {
            let clean = primitive_shared::types::unpoisoned(block);
            if clean != block {
                state.inventory.retype_slot(slot, clean);
                state.inventory_dirty = true;
            }
        }
    }
    // A blow costs the blade, the same as a swing at a block costs the
    // pick -- and only when it lands, so swinging at the air is free.
    if !matches!(struck, animals::Struck::Missed) {
        let wear = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let slot = state.selected_slot;
            let wear = state.inventory.wear_tool(slot);
            state.inventory_dirty |= !matches!(wear, primitive_shared::inventory::Wear::None);
            wear
        };
        match wear {
            primitive_shared::inventory::Wear::None => {}
            primitive_shared::inventory::Wear::Worn => send_inventory(handle),
            primitive_shared::inventory::Wear::Broke => {
                send_inventory(handle);
                handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::ToolBroke });
            }
        }
    }
    // A kill leaves nothing *here*. The body goes down for
    // `animals::FALL_SECONDS` and the tick loop lays the carcass where it came
    // to rest (`Animals::take_fallen`): laying it now, where the blow landed,
    // was a carcass under a deer that was still falling -- and a second one
    // when the fall ended.
}

/// Where a dead animal's carcass goes, if anywhere.
///
/// The cell at its feet, or the first free cell under it that has
/// something solid to lie on -- at most three down, which is the height
/// an animal can be off the ground in the middle of a hop or a knock
/// back. `None` if there is no such cell: the animal died in water, in
/// mid-air over a pit, or standing in a doorway somebody has already
/// filled.
///
/// "Free" is air or the two decorative shapes you walk through -- a tuft
/// of grass, a flower, a pebble. A carcass has to be able to land on a
/// meadow, and a meadow is mostly tufts; a rule that wanted bare air
/// under every deer would put most of them back into the old heap. A
/// torch or a campfire is not free: it is a thing somebody placed, and
/// a boar does not get to put it out by dying on it.
///
/// A pure function of a lookup so it can be tested against a closure
/// rather than a `World`; `lay_carcass` hands it the chunk cache.
pub(crate) fn carcass_cell(
    at: (f32, f32, f32),
    block_at: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
) -> Option<(i32, i32, i32)> {
    use primitive_shared::blocks::{definition, Shape};
    use primitive_shared::types::{has_full_top, is_air};

    let free = |block: primitive_shared::types::BlockId| {
        is_air(block) || matches!(definition(block).shape, Shape::Cross | Shape::Flat)
    };
    let (x, z) = (at.0.floor() as i32, at.2.floor() as i32);
    // A hair up, so an animal resting exactly on a block boundary --
    // which is where a standing one is -- reads as *in* the cell above
    // the floor rather than in the floor.
    let feet = (at.1 + 0.01).floor() as i32;
    // **On a whole floor**, which is what the carcass is drawn lying on:
    // the floor of its own cell. It asked only for something solid, and a
    // slab, a step, a campfire or a drift of snow is solid with its top
    // short of the cell's -- a boar killed on a stair lay on the air over
    // the tread. Where there is no whole floor the meat goes to the heap,
    // the answer this already gives over water.
    (0..=3).map(|down| feet - down).find_map(|y| {
        let here = block_at(x, y, z)?;
        let under = block_at(x, y - 1, z)?;
        (free(here) && has_full_top(under)).then_some((x, y, z))
    })
}

/// What a death leaves behind.
///
/// **A carcass where it fell, and the old heap where a carcass cannot
/// lie.** The block goes into the world through the same three calls a
/// tilled field does -- `set_block`, the edit metric, the broadcast --
/// so a carcass is a block change like any other to the mods, the
/// mesher and the save. The heap is kept for the cells `carcass_cell`
/// refuses: an animal that dies over water still leaves its meat,
/// through the item path, rather than leaving nothing because the
/// nicer answer had nowhere to go. The heap is `Species::drops` -- no
/// sinew, no bone -- which is the small price of shooting a deer off a
/// cliff into a lake.
///
/// One function for the three ways an animal dies (a player's blow, a
/// mod's `entity_damage`, a mod's `entity_kill`), because the first
/// version of this lived in `attack_animal` alone and a mod that killed
/// a deer got a heap while a player got a carcass.
pub(crate) fn lay_carcass(ctx: &Arc<Context>, death: animals::Death) {
    let animals::Death { species, at, growth, shorn } = death;
    // **A young one leaves a heap of what it had, not its species' carcass**
    // -- see `youth::leaves_carcass`, which is where the reason lives: the
    // carcass is the adult's, drawn and butchered at the adult's size.
    if !primitive_shared::youth::leaves_carcass(growth) {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        let unshorn = |&(block, _): &(primitive_shared::types::BlockId, u32)| !(shorn && block == primitive_shared::types::BLOCK_WOOL);
        for (block, count) in primitive_shared::youth::drops(species, growth).into_iter().filter(unshorn) {
            items.spawn(
                block,
                count,
                (f64::from(at.0), f64::from(at.1 + 0.5), f64::from(at.2)),
                (0.0, 0.0, 0.0),
                None,
                Instant::now(),
            );
        }
        return;
    }
    // **A fish leaves no carcass** (`Species::carcass`), so it goes straight
    // to the heap: the fish itself, where it died. Asking for a cell first
    // would find one for a fish killed stranded on a bank, and write the
    // air `carcass_at_stage` answers for a species with no body into it.
    let cell = species
        .carcass()
        .and_then(|_| carcass_cell(at, |x, y, z| ctx.world.cached_block(x, y, z)));
    if let Some(cell) = cell {
        // A shorn sheep's carcass starts with its first cut made: the fleece
        // is the sheep's first cut (`Species::butchering`) and it is already
        // off. See `animals::Death::shorn`.
        let carcass = primitive_shared::animals::carcass_at_stage(species, usize::from(shorn));
        if ctx.world.set_block(cell.0, cell.1, cell.2, carcass) {
            ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
            notify_mechanics(ctx, cell.0, cell.1, cell.2);
            broadcast_block(ctx, cell, carcass);
            return;
        }
    }
    // Through the same item path a broken block uses, so it bobs,
    // merges, expires and is picked up by walking over it like
    // everything else.
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
    for &(block, count) in species.drops().iter().filter(|(block, _)| !(shorn && *block == primitive_shared::types::BLOCK_WOOL)) {
        items.spawn(
            block,
            count,
            (f64::from(at.0), f64::from(at.1 + 0.5), f64::from(at.2)),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
}

/// One cut off a carcass: what a right click on one does.
///
/// The decision -- which cut, what it yields, what the cell holds
/// afterwards -- is `primitive_shared::animals::butcher`, and is tested
/// there; this is the doing. The yield goes on the ground at the
/// carcass rather than into the pack, through the same item path a
/// broken block uses, so a full pack is not a reason the cut cannot be
/// made and the meat is where the animal is rather than wherever the
/// player was standing.
///
/// The tool wears one point a cut, through the same call a blow on the
/// animal costs it, and the cut is completed even if that point was the
/// last one: the knife broke *on* the cut, not instead of it, and a cut
/// that vanished with the tool would read as the game taking the meat
/// back.
fn butcher_carcass(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    outcome: primitive_shared::animals::Butchered,
) {
    use primitive_shared::animals::Butchered;

    let (took, next) = match outcome {
        Butchered::NeedsATool => {
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NeedsAKnife });
            return;
        }
        Butchered::Cut { took, next } => (took, next),
    };
    if !ctx.world.set_block(at.0, at.1, at.2, next) {
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, next);

    if let Some((block, count)) = took {
        // **Out of the carcass and towards the butcher, not into the
        // heap.** The cut used to appear at the middle of the cell, which
        // is *inside* the lying model: a bone showed as a white square
        // under the boar's belly, a hide as a tan one under the wolf,
        // the sinews as yellow specks between the legs, and the player
        // reported them as rendering faults -- reasonably, because
        // nothing about a thing poking out of a dead animal says
        // "pick me up". So the cut is thrown from above the body
        // towards where the butcher stands, and lands at their feet, a
        // step clear of the carcass. The player's position is read and
        // released before the items are locked, in the lock order the
        // rest of the file keeps.
        let feet = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
        let centre = (at.0 as f32 + 0.5, at.2 as f32 + 0.5);
        let (dx, dz) = (feet.0 - f64::from(centre.0), feet.2 - f64::from(centre.1));
        let len = (dx * dx + dz * dz).sqrt();
        let towards = if len > 1e-3 { (dx / len, dz / len) } else { (1.0, 0.0) };
        // The cell beside the carcass on the butcher's side, if it is
        // open: the cut is set down there and falls to the floor beside
        // the animal. Thrown from above the body it came down on the
        // carcass's own collider -- the slab its table row is -- and lay
        // on top of it, inside the model, which is where it started.
        // Against a wall or a chest it falls back to the throw.
        let side = (
            at.0 + towards.0.round() as i32,
            at.1,
            at.2 + towards.1.round() as i32,
        );
        let side_open = ctx
            .world
            .cached_block(side.0, side.1, side.2)
            .is_some_and(|b| !primitive_shared::types::is_collidable(b));
        let (spawn_at, direction) = if side_open {
            ((f64::from(side.0) + 0.5, f64::from(at.1) + 0.5, f64::from(side.2) + 0.5), (0.0, 0.0, 0.0))
        } else {
            ((f64::from(centre.0), f64::from(at.1) + 0.9, f64::from(centre.1)), (towards.0 as f32, 0.3, towards.1 as f32))
        };
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn(block, count, spawn_at, direction, None, Instant::now());
    }

    let wear = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        let wear = state.inventory.wear_tool(slot);
        state.inventory_dirty |= !matches!(wear, primitive_shared::inventory::Wear::None);
        wear
    };
    match wear {
        primitive_shared::inventory::Wear::None => {}
        primitive_shared::inventory::Wear::Worn => send_inventory(handle),
        primitive_shared::inventory::Wear::Broke => {
            send_inventory(handle);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::ToolBroke });
        }
    }
}

/// What a swing at an animal is worth, with that in hand.
///
/// **A knife is the hunting tool**, and this is the only place in the
/// game where `Work::Plant` means something other than cutting grass:
/// what kills an animal is an edge, and the axe and the pick are weight
/// on a stick. A tier multiplies it, so a bronze knife is a real
/// improvement on a flint one -- which is the first reason to want metal
/// that is not about digging.
///
/// The bare-handed figure is the same punch a player throws at another
/// player, so nothing here makes a fist better than it was.
fn hunting_damage(held: Option<primitive_shared::types::BlockId>) -> f32 {
    use primitive_shared::blocks::{definition, Work};
    use primitive_shared::combat::MELEE_DAMAGE;

    let Some(held) = held else { return MELEE_DAMAGE };
    // **The spear, which is the only thing here that is a weapon and
    // not a tool.** It has no tier -- it opens no block -- so the
    // ladder below never sees it, and it is answered by name first.
    //
    // Above every knife, the iron one included, and that is the point
    // of it rather than a number to tune: a knife's stroke is an edge
    // driven by a wrist, and a better metal makes a better edge; a
    // spear's thrust is a flint point with two metres of haft and the
    // hunter's weight behind it, and no metal in the point would make
    // that heavier. Two and a half times the knife's factor at the top
    // of the ladder, so a hunter with a spear does not wish for an iron
    // knife -- the knife is what the carcass is for. What it costs is a
    // knapped head that shatters half the time, a cord, and seventy
    // thrusts before the point is gone (`durability`, in `blocks`).
    if primitive_shared::types::is_weapon(held) {
        // **Four spears now, and the head is what differs.** The
        // paragraph above is still the argument for the *shape*: what a
        // spear does is a point with two metres of haft and the
        // hunter's weight behind it, and that part is the same whatever
        // the point is made of. What a better point adds is depth --
        // it goes in further before it stops -- so the material moves
        // the number by a quarter at a time rather than doubling it.
        //
        // Bone is *below* flint on purpose. It is the spear you have on
        // the first evening, before anything has been knapped, and a
        // player who finds it as good as flint has no reason to knap.
        let head = match primitive_shared::types::block_kind(held) {
            primitive_shared::types::BLOCK_BONE_SPEAR => 0.8,
            primitive_shared::types::BLOCK_COPPER_SPEAR => 1.15,
            primitive_shared::types::BLOCK_BRONZE_SPEAR => 1.3,
            primitive_shared::types::BLOCK_IRON_SPEAR => 1.5,
            // Flint, which is what the 2.5 was measured against.
            _ => 1.0,
        };
        // **A named thrust, and it was a mining speed.** This read
        // `2.5 * Tier::Iron.speed()`: the haft and the hunter multiplied by
        // how fast an iron pick opens rock, so the day the pick was made
        // honest every spear lost a quarter of its thrust. Fifteen is what
        // that came to, kept exactly, and now it is its own number. See
        // `tools::SPEAR_THRUST`.
        return MELEE_DAMAGE * primitive_shared::tools::SPEAR_THRUST * head;
    }
    let def = definition(held);
    let Some(tier) = def.tool else { return MELEE_DAMAGE };
    if def.work == Work::Plant {
        // An edge, and a good one is a better one -- and a blunt one a
        // worse one. The metal's factor is the hunt's own table rather than
        // the mining ladder, for the spear's reason above: see
        // `tools::blade_factor`.
        MELEE_DAMAGE * 2.0 * primitive_shared::tools::blade_factor(tier, held)
    } else {
        // A haft with a stone on the end of it. Better than a fist and
        // not by much.
        MELEE_DAMAGE * 1.5
    }
}

/// What a person's swing leaves on another person, by what is in the hand.
///
/// **An edge cuts and everything else bruises**, and "an edge" is the same
/// question `hunting_damage` asks: a spear, or a tool whose work is cutting
/// (`Work::Plant` -- the knife, the axe, the hoe's blade). A pick is weight
/// on a stick, and so is a fist.
fn blow_of(held: Option<primitive_shared::types::BlockId>) -> primitive_shared::injury::Blow {
    use primitive_shared::blocks::{definition, Work};
    use primitive_shared::injury::Blow;
    match held {
        Some(block) if primitive_shared::types::is_weapon(block) => Blow::Edge,
        Some(block) if definition(block).tool.is_some() && definition(block).work == Work::Plant => {
            Blow::Edge
        }
        _ => Blow::Blunt,
    }
}

/// Tells the client how full it is, and notes that it has been told.
fn send_nourishment(handle: &Arc<players::PlayerHandle>) {
    let fraction = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.mark_food_reported();
        state.vitals.nourishment_fraction()
    };
    handle.send(ServerMessage::Nourishment { fraction });
}

/// Recomputes what a player is carrying, which is what a fall costs.
fn refresh_carried_weight(handle: &Arc<players::PlayerHandle>) {
    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
    let kilograms = state.inventory.total_weight();
    state.vitals.set_carried_weight(kilograms);
}

/// Moves what was poured into a jug into the container store at the cell
/// it has just been set down in.
///
/// `damage` is the jug stack's own field, where its contents lived while
/// it was carried (`inventory::jug_contents`). Called by the placement
/// after the cell has been written, so a refused placement never leaves
/// grain behind in a cell with no jug in it.
///
/// **Whatever was already stored at the cell is spilled first.** A cell
/// only has a store entry while a container stands in it, but not every
/// path that replaces a container empties it -- a mod writing a cell, a
/// tree landing -- and an orphan left there would be read as this jug's
/// contents: forty slots of somebody's chest turning up inside a jug of
/// seeds. Spilled rather than deleted, because it was somebody's.
pub(crate) fn set_down_vessel(ctx: &Arc<Context>, at: containers::ChestPos, damage: u32) {
    let carried = primitive_shared::inventory::Stack::worn(
        primitive_shared::types::BLOCK_JUG,
        1,
        damage,
    );
    let orphan = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        let orphan = chests.take(at);
        if let Some((goods, count)) = primitive_shared::inventory::jug_contents(&carried) {
            chests.edit(at, |store| {
                store.put_in_slot(
                    primitive_shared::inventory::VESSEL_SLOT,
                    primitive_shared::inventory::Stack::new(goods, count),
                )
            });
        }
        orphan
    };
    if let Some(orphan) = orphan {
        let centre = (at.0 as f32 + 0.5, at.1 as f32 + 1.0, at.2 as f32 + 0.5);
        spill_inventory(ctx, &orphan, centre);
    }
}

/// The jug that drops out of a cell a jug stood in, with what was in it
/// folded back inside.
///
/// The other half of `set_down_vessel`, and the reason a jug of grain can
/// be set down and picked up again and still be a jug of grain. Called
/// from `spawn_block_drop`, which every way a block turns into an item
/// goes through -- a player's break, a mod's, and the collapse of a jug
/// whose table was taken out from under it -- so no path can drop an empty
/// jug and leave the grain in a store entry nobody can reach.
///
/// Anything in the store a jug could not hold (see
/// `inventory::vessel_store_contents`) is spilled beside it rather than
/// squeezed in or deleted. Anyone looking into the jug is shut out of it
/// first, for the reason `spill_chest` gives.
fn pick_up_vessel(ctx: &Arc<Context>, at: containers::ChestPos) -> primitive_shared::inventory::Stack {
    close_chest_for_everyone(ctx, at);
    let stored = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.take(at)
    };
    let Some(mut stored) = stored else {
        return primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_JUG, 1);
    };
    let jug = match primitive_shared::inventory::vessel_store_contents(&stored) {
        Some((goods, count)) => {
            stored.take_slot(primitive_shared::inventory::VESSEL_SLOT);
            primitive_shared::inventory::filled_jug(goods, count)
        }
        None => primitive_shared::inventory::Stack::new(primitive_shared::types::BLOCK_JUG, 1),
    };
    if !stored.is_empty() {
        let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
        spill_inventory(ctx, &stored, centre);
    }
    jug
}

/// Pours what a broken vessel held onto the floor where it stood: a barrel
/// of water (`types::barrel_contents`), or every jug of water in a thing set
/// down (`BLOCK_SET_DOWN`), which drops back as the empty jug it now is.
///
/// **Called from `spawn_block_drop`, which every way a block turns into an
/// item goes through** -- a player's break, a mod's, a floor dug out from
/// under it -- so no path loses the water. The player's break also calls it
/// before the set-down's store is spilled (`net::connection`), because that
/// path empties the store first; a second call finds no water left in it,
/// and a barrel has no store to find twice.
///
/// Rejected for a set-down jug: dropping it full, as it was. Knocking a jug
/// of water over and picking it up full is the one thing a jug on its side
/// does not do, and taking it back by hand (`pick_up_set_down`) is still
/// there for a player who wants the water.
pub(crate) fn tip_out_water(ctx: &Arc<Context>, broken: primitive_shared::types::BlockId, at: (i32, i32, i32)) {
    use primitive_shared::types::{barrel_contents, block_kind, is_set_down, BLOCK_JUG, BLOCK_JUG_WATER};
    let jugs = if let Some((_, jugs)) = barrel_contents(broken) {
        u32::from(jugs)
    } else if is_set_down(broken) {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |store| {
            let mut jugs = 0;
            for slot in 0..store.slots().len() {
                if store.block_in(slot).is_some_and(|block| block_kind(block) == BLOCK_JUG_WATER) {
                    if let Some(full) = store.take_slot(slot) {
                        jugs += full.count;
                        // A plain jug, not the full one's wear carried over: an
                        // empty jug's wear is what is in it (`jug_contents`),
                        // and a water jug's would read back as grain.
                        store.put_in_slot(slot, primitive_shared::inventory::Stack::new(BLOCK_JUG, full.count));
                    }
                }
            }
            jugs
        })
    } else {
        0
    };
    if jugs == 0 {
        return;
    }
    let written = water::spill(&*ctx.world, at, jugs.saturating_mul(water::SPILL_EIGHTHS_PER_JUG));
    if written.is_empty() {
        return;
    }
    ctx.spills.lock().unwrap_or_else(|e| e.into_inner()).spilled(at);
    let changes: Vec<BlockChange> = written
        .into_iter()
        .map(|((x, y, z), block_id)| {
            notify_mechanics(ctx, x, y, z);
            BlockChange { global_x: x, global_y: y, global_z: z, block_id }
        })
        .collect();
    broadcast_changes(ctx, changes);
}

pub(crate) fn spawn_block_drop(ctx: &Arc<Context>, broken: u16, at: (i32, i32, i32)) {
    // The water a vessel held goes on the floor first, before the vessel's
    // own drop reads what is left in it. See `tip_out_water`.
    tip_out_water(ctx, broken, at);
    // **A thing set down drops the thing**, however the cell went: a
    // player's break has already spilled it, and the floor dug out from
    // under it (`collapse_unsupported`) comes only through here.
    if primitive_shared::types::is_set_down(broken) {
        spill_chest(ctx, at);
        return;
    }
    // **A jug drops as the jug it was**, contents and all -- see
    // `pick_up_vessel`. Before the drop table, whose answer is a plain jug.
    if primitive_shared::types::opens_as_vessel(broken) {
        let jug = pick_up_vessel(ctx, at);
        let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        items.spawn_worn(
            jug.block,
            jug.count,
            jug.damage,
            primitive_shared::geometry::wide(centre),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
        return;
    }
    // **A trap lifted with fish in it spills them**, beside the basket. They
    // are in its variant (`types::trap_catch`), which the drop table's plain
    // trap does not carry, and a trap broken rather than emptied was a catch
    // deleted by the wrong button.
    let caught = primitive_shared::types::trap_catch(broken);
    if caught > 0 {
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            primitive_shared::types::BLOCK_RAW_FISH,
            u32::from(caught),
            (f64::from(at.0 as f32 + 0.5), f64::from(at.1 as f32 + 0.5), f64::from(at.2 as f32 + 0.5)),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
    // **A barrel of grain broken spills the grain**, beside the staves, on
    // the trap's reasoning: the level is its variant, the drop table's empty
    // barrel cannot carry it, and a barrel knocked over is a heap on the
    // ground and not a harvest deleted. Water in a barrel goes into the
    // ground, which is where water goes.
    if let Some((goods, jugs)) = primitive_shared::types::barrel_goods(broken) {
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            goods,
            u32::from(jugs) * primitive_shared::inventory::JUG_UNITS,
            (f64::from(at.0 as f32 + 0.5), f64::from(at.1 as f32 + 0.5), f64::from(at.2 as f32 + 0.5)),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
    // **A wall in stages gives back what was laid into it** (`build::refund`),
    // which its row cannot say: how many bricks depends on how many courses.
    if let Some(refund) = primitive_shared::build::refund(broken) {
        let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        for (block, count) in refund {
            items.spawn(block, count, primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
        }
        return;
    }
    let Some(drop) = primitive_shared::types::block_drop(broken) else {
        return; // water and air leave nothing behind
    };
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());

    // **Where a field comes from -- and it is not this.** Seed used to
    // fall out of one tuft of grass in four, right here, and that made
    // agriculture free and invisible: a player pulling up fibre in
    // their first minute was already holding a field, so the moment of
    // *becoming a farmer* never happened.
    //
    // Seed comes off wild wheat now -- a stand of real cereal that
    // grows in open, warm, dry country and nowhere else (see
    // `worldgen::Biome::wild_wheat_spacing`). Finding it is the work,
    // and the work is the point. What a tuft of grass gives is fibre,
    // which is what a tuft of grass is.

    // **Half the tufts give no fibre.** Fibre is what a cord is twisted
    // from, six to the cord, and the cord is what every stone tool is
    // lashed with -- so fibre is where the price of a tool is paid, and
    // a meadow that gave a strand per tuft paid it in a minute. One tuft
    // in two, decided by where it grew for the same reason the seed is
    // (above): the same tuft always answers the same way, so nothing is
    // re-rolled and nothing is stored. A reed gives two fibre for one
    // (the "reed fibre" recipe), which is what makes a riverbank the
    // fibre country and a meadow the place you make do. The seed is
    // unaffected: a tuft that carried seed and no fibre still drops the
    // seed, above.
    // Dry grass is the same grass dried, and gives fibre on the same rule.
    if matches!(
        primitive_shared::types::block_kind(broken),
        primitive_shared::types::BLOCK_TALL_GRASS | primitive_shared::types::BLOCK_DRY_GRASS
    ) && !fibrous(at)
    {
        // ...but a tuft with no fibre in it can still have had something
        // living in it. See `bait_from_the_ground`, which is asked before
        // this returns for exactly that reason: half the meadow would
        // otherwise be silently barren of grubs as well.
        if let Some(bait) = bait_from_the_ground(broken, at) {
            items.spawn(bait, 1, primitive_shared::geometry::wide(
                (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5),
            ), (0.0, 0.0, 0.0), None, Instant::now());
        }
        return;
    }

    // **What was living in the ground.** A worm out of dug soil or a dung
    // heap, a grub out of a tuft of grass -- see `bait_from_the_ground` for
    // why it is decided by the cell and not by a die.
    if let Some(bait) = bait_from_the_ground(broken, at) {
        items.spawn(bait, 1, primitive_shared::geometry::wide(
            (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5),
        ), (0.0, 0.0, 0.0), None, Instant::now());
    }

    // Centre of the cell that was just emptied, so the drop pops out of
    // the hole rather than out of its floor.
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    // **The last quarter of a bitten heap of loose ground is a handful**, and
    // so is every quarter still standing when a bite is broken some other
    // way -- the other three came off one a swing (`dig_one_slice`). A whole
    // block broken whole still drops the block. See `build`.
    if let Some((handful, count)) = primitive_shared::build::handfuls_left(broken) {
        items.spawn(handful, count, primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
        return;
    }
    items.spawn(
        drop,
        u32::from(primitive_shared::types::block_drop_count(broken)),
        primitive_shared::geometry::wide(centre),
        (0.0, 0.0, 0.0),
        None,
        Instant::now(),
    );
    // **The seed a harvest carries**, which is what lets a field sow
    // itself again -- see `types::also_drops` for why it is one seed and
    // why it lives there rather than here. Out of the same hole as the
    // harvest: two stacks of two kinds never merge, so the player sees
    // both come out of the plant.
    if let Some((seed, count)) = primitive_shared::types::also_drops(broken) {
        items.spawn(seed, u32::from(count), primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
    }
    // **A lodestone out of one iron cell in eight**, beside the ore. See
    // `lodestone_in`.
    if let Some(stone) = lodestone_in(broken, at) {
        items.spawn(stone, 1, primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
    }
}

/// **Whether this iron ore was a lodestone**, and so gives one beside its
/// ore: one cell in eight, off the hash `fibrous` and `bait_from_the_ground`
/// use, and **read off bits 40 to 42** -- clear of the fibre's ninth and the
/// bait's twentieth, the trap `fibrous` names. By the cell rather than a
/// die, for the bait's reason: the same vein cell answers the same way
/// however often it is asked.
///
/// The honest hole, the same as the worm's: ore set down in a new cell and
/// broken again is a new cell asked. It is left open because what it buys
/// is one stone, and one stone is every compass a household makes (the
/// recipe gives it back). Rejected: a lodestone block of its own in the
/// generator -- a second ore to find where the point is that it is found
/// *in the iron*, which is what puts the compass in the iron age.
fn lodestone_in(broken: u16, at: (i32, i32, i32)) -> Option<primitive_shared::types::BlockId> {
    use primitive_shared::types::{block_kind, BLOCK_IRON_ORE, BLOCK_LODESTONE};
    if block_kind(broken) != BLOCK_IRON_ORE {
        return None;
    }
    let mut h = (at.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (at.1 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (at.2 as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    ((h >> 40) & 7 == 0).then_some(BLOCK_LODESTONE)
}

/// **What bait was in this cell**, if any: a worm in soil, a grub in grass.
///
/// One cell in four, off the same hash as `fibrous` and for the same reason:
/// it has to be the same answer every time it is asked about the same cell,
/// or a player could stand over one patch of dirt, put it back and dig it
/// again until a worm came out. **Read off different bits from the fibre**
/// -- the trap named in `fibrous`'s own note, fallen into once already: read
/// off the same bits and every fibrous tuft would be a grubby one, and the
/// meadow would sort itself into rich tufts and empty ones.
///
/// A dung heap always has worms in it, which is the only thing dung has ever
/// been good for and the reason it is worth not clearing away.
fn bait_from_the_ground(broken: u16, at: (i32, i32, i32)) -> Option<primitive_shared::types::BlockId> {
    use primitive_shared::types::{
        block_kind, BLOCK_DIRT, BLOCK_DRY_GRASS, BLOCK_DUNG, BLOCK_FARMLAND, BLOCK_GRASS, BLOCK_GRUB,
        BLOCK_TALL_GRASS, BLOCK_WORM,
    };
    let kind = block_kind(broken);
    if kind == BLOCK_DUNG {
        return Some(BLOCK_WORM);
    }
    let bait = match kind {
        BLOCK_DIRT | BLOCK_GRASS | BLOCK_FARMLAND => BLOCK_WORM,
        BLOCK_TALL_GRASS | BLOCK_DRY_GRASS => BLOCK_GRUB,
        _ => return None,
    };
    let mut h = (at.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (at.1 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (at.2 as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    // Bits 20 and 21, well clear of the fibre's ninth.
    ((h >> 20) & 3 == 0).then_some(bait)
}

/// Did this tuft of grass give fibre?
///
/// One in two, by a hash of the cell rather than a die roll: it has to
/// be the same answer every time it is asked about the same tuft, or a
/// player standing over one cell could re-roll it. The mixing is the one
/// from `logic::rng`'s splitmix, cut down to the three coordinates, and
/// the answer is read off the ninth bit.
///
/// **There used to be a second answer read off the low two bits** -- one
/// tuft in four carried seed. Seed comes off wild wheat now (see the
/// note in `spawn_block_drop`), and the reason the two answers were
/// taken from different bits is recorded here because it is the trap
/// anybody adding a third one will fall into: read off the same bits,
/// every seeded tuft would have been a fibrous one, and the meadow would
/// have been sorted into rich tufts and empty ones.
fn fibrous(at: (i32, i32, i32)) -> bool {
    let mut h = (at.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (at.1 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (at.2 as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    (h >> 8) & 1 == 0
}

/// Throws part or all of a slot into the world in front of the player.
pub(crate) fn drop_from_slot(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    slot: usize,
    whole_stack: bool,
) -> bool {
    // Asked before the stack leaves the pack, and with nothing locked --
    // a mod that refuses this must leave the player holding what they
    // were holding, and a veto that arrived after the take would have
    // to put it back.
    {
        let (block, count) = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(block) = state.inventory.block_in(slot) else {
                return false;
            };
            let count = if whole_stack {
                state.inventory.count_in(slot)
            } else {
                1
            };
            (block, count)
        };
        if !item_dropped(ctx, handle.id, block, count) {
            return false;
        }
    }

    let thrown = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(block) = state.inventory.block_in(slot) else {
            return false;
        };
        // **Read before the stack is taken, and carried to the ground
        // with it.** Wear is a fact about the object; a half-spent axe
        // thrown down and picked back up used to come back new, which
        // is a free repair for two taps. See `items::Item::damage`,
        // which is the field this fills, and `Items::spawn_worn`, which
        // is the door it has to come through.
        let damage = state
            .inventory
            .slots()
            .get(slot)
            .copied()
            .flatten()
            .map_or(0, |stack| stack.damage);
        let want = if whole_stack {
            state.inventory.count_in(slot)
        } else {
            1
        };
        let count = state.inventory.take_from(slot, want);
        if count == 0 {
            return false;
        }
        state.inventory_dirty = true;
        let (x, y, z) = state.position;
        (block, count, damage, (x, y, z), state.yaw, state.pitch)
    };
    let (block, count, damage, position, yaw, pitch) = thrown;

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
        items.spawn_worn(
            block,
            count,
            damage,
            // From eye height and a little way along the look direction,
            // so it leaves from where the player is aiming rather than
            // from inside them -- and, more to the point, so a throw at
            // a wall one step away does not spawn on the far side of it.
            (
                (position.0 + f64::from(look.0 * 0.4)),
                (position.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT) + f64::from(look.1 * 0.4)),
                (position.2 + f64::from(look.2 * 0.4)),
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
        //
        // **As it went, wear and all, and into the square it left.** This
        // was `add(block, count)`, which makes a new stack: a half-spent
        // pick thrown while the world was at its item cap came back
        // whole -- the same free repair the ground was cured of above, by
        // the one route that did not go through the ground. `put_in_slot`
        // carries the damage; `add_worn` takes whatever that refuses,
        // which needs the square to have been filled in the moment the
        // lock was let go.
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(refused) = state.inventory.put_in_slot(
            slot,
            primitive_shared::inventory::Stack::worn(block, count, damage),
        ) {
            state.inventory.add_worn(refused.block, refused.count, refused.damage);
        }
        drop(state);
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::TooMuchLyingAround });
    }
    send_inventory(handle);
    thrown
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
    // **A thing set down is not a chest to look into**: its store is how it
    // is kept, not a lid. Taken back by hand (`pick_up_set_down`), and a
    // client that asked to open one would be a client moving forty stacks
    // into a knife.
    ctx.world.cached_block(at.0, at.1, at.2).is_some_and(|block| {
        primitive_shared::types::is_container(block) && !primitive_shared::types::is_set_down(block)
    })
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
    // **A rack is opened at its anchor**, whichever of its four cells was
    // clicked (`types::rack_anchor`): the frame is one thing, and its
    // contents hang on the ridge rather than in a cell. A lone cell from an
    // older save is its own anchor, so nothing there changes.
    let at = match ctx.world.cached_block(at.0, at.1, at.2) {
        Some(block) if primitive_shared::rack::is_rack(block) => primitive_shared::types::rack_anchor(at, block),
        _ => at,
    };
    if !chest_in_use(ctx, primitive_shared::geometry::narrow(position), at) {
        return;
    }
    unseal_ruin_chest(ctx, at);
    // **A stall nobody owns is claimed by the first player to open it.** A
    // player's placement records its owner (`stall_placed`), so the only
    // stalls without one were written by something else -- a mod, an
    // operator's tool, a world edited by hand -- and a stall nobody may
    // price is a counter nobody can use. The first to reach it is the only
    // person the server can tell apart from everybody else.
    if is_stall(ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0)) {
        ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).claim(at, &handle.username);
    }
    // **Asked before the screen is opened**, so a mod that refuses is a
    // lock rather than a screen that closes itself half a second later.
    // With nothing held, which is why the position was read and the lock
    // let go above.
    let block = ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0);
    if !container_opened(ctx, handle.id, at, block) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::WillNotOpen });
        return;
    }
    let left = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.open_chest.replace(at)
    };
    // **The one they were at, if they are somehow at another already.** The
    // client shuts its screen before it opens the next (`CloseChest`), so
    // this is nearly always nothing; a lid left up by the message that never
    // came is a chest standing open with nobody at it for ever.
    if let Some(left) = left.filter(|&left| left != at) {
        tell_chest_lid(ctx, left);
    }
    tell_chest_lid(ctx, at);
    send_chest_state(ctx, handle, at);
}

// ---- the barter stall ----
//
// **A chest with an owner and a price list.** Everything a chest does -- the
// screen, the store, the save, the spill -- a stall does by being a
// container (`primitive_shared::stall`); what is here is the three things a
// chest does not have: who put it down, what it asks, and a trade.

/// Is this block a barter stall?
pub(crate) fn is_stall(block: primitive_shared::types::BlockId) -> bool {
    primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_STALL
}

/// Sends one player at a stall whose it is and what it asks, if the
/// container at `at` is a stall. Nothing for anything else.
fn tell_stall_offers(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: containers::ChestPos) {
    if !is_stall(ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0)) {
        return;
    }
    let stall = ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).get(at).cloned();
    let Some(stall) = stall else {
        return;
    };
    handle.send(ServerMessage::StallOffers {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        yours: stall.owner == handle.username,
        owner: stall.owner,
        offers: stall.offers,
    });
}

/// A stall has just been put down by this player: it is theirs.
///
/// **Whatever was stored at the cell is spilled first**, for
/// `set_down_vessel`'s reason: not every path that replaces a container
/// empties it, and an orphan left there would be read as this stall's stock
/// -- somebody's chest turning up for sale on a stranger's counter.
pub(crate) fn stall_placed(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: containers::ChestPos) {
    let orphan = ctx.chests.lock().unwrap_or_else(|e| e.into_inner()).take(at);
    if let Some(orphan) = orphan {
        spill_inventory(ctx, &orphan, (at.0 as f32 + 0.5, at.1 as f32 + 1.0, at.2 as f32 + 0.5));
    }
    ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).place(at, &handle.username);
}

/// A player has broken a stall. Called before the container spill.
///
/// **Its owner takes it down; anybody else breaks it open.** The owner's
/// counter and till go into their pack -- what does not fit falls at the
/// stall, which is the only other place it can go -- and the spill that
/// follows finds nothing. Anybody else's break leaves the goods where
/// they are, and the spill tips them out like a chest's.
///
/// That second half is the decision, and the three ways it could have gone:
///
/// * **Unbreakable by anybody but the owner** -- rejected. A block nobody can
///   remove is a claim on land: a stall dropped in somebody's doorway, or on
///   the one ford across a river, would stand there for ever. Nothing else in
///   this world is anybody's by law, and the stall would be the first thing
///   that was.
/// * **Breakable, and the goods lost** -- rejected. That punishes the owner
///   for somebody else's act and rewards nobody, which is spite with no
///   decision in it.
/// * **Breakable, and the goods spilled** -- chosen. A stall is a counter, not
///   a safe: what is on it can be taken by force by anybody willing to be
///   seen doing it, as a chest can. What that asks of the owner is the
///   question the stall exists to ask -- how much to put out, and where. A
///   little flint by the road; the winter's copper in a chest at home.
pub(crate) fn stall_broken(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: containers::ChestPos) {
    let owned = ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).is_owner(at, &handle.username);
    if !owned {
        return;
    }
    close_chest_for_everyone(ctx, at);
    let Some(goods) = ctx.chests.lock().unwrap_or_else(|e| e.into_inner()).take(at) else {
        return;
    };
    let mut left_over = primitive_shared::inventory::Inventory::chest();
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        for stack in goods.slots().iter().flatten() {
            // As the thing it is, wear and all: see `stall::trade`.
            let left = state.inventory.add_worn(stack.block, stack.count, stack.damage);
            if left > 0 {
                left_over.add_worn(stack.block, left, stack.damage);
            }
        }
        state.inventory_dirty = true;
    }
    send_inventory(handle);
    refresh_carried_weight(handle);
    if !left_over.is_empty() {
        spill_inventory(ctx, &left_over, (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5));
    }
}

/// The owner of the open stall setting, or taking down, one row's price.
pub(crate) fn stall_offer(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    row: u8,
    offer: Option<primitive_shared::stall::Offer>,
) {
    // The owner, within reach, alive, at a stall: `usable_chest` is every
    // one of those, and says `NotYours` to anybody else.
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    if !is_stall(ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0)) {
        return;
    }
    let set = ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).set_offer(at, row as usize, offer);
    if set {
        // Everybody at it, the owner included: a buyer looking at the old
        // price has to see the new one before their click is refused for it.
        broadcast_chest_state(ctx, at);
    } else {
        handle.send(ServerMessage::StallRefused { why: primitive_shared::stall::Refusal::BadOffer });
    }
}

/// A player taking one lot of row `row` at the open stall, at the price
/// they saw.
///
/// **Atomic by its locks**: the buyer's own state, then the container store,
/// then the stall records -- the order every container gesture takes the
/// first two in -- held together from the check to the write. Two buyers
/// reaching for the last lot at once are two calls here, and the store's
/// lock makes one of them run whole before the other begins: the second
/// finds the counter short and is refused having paid nothing
/// (`stall::trade` works on copies). A buyer whose connection drops has
/// either been through here or not; there is no half of a trade held
/// anywhere between two messages, because there is no second message. And
/// a stall broken mid-trade cannot be: the break takes the store's lock to
/// spill it, so it lands before or after, and a trade after it finds no
/// stall (`reachable_chest`).
pub(crate) fn stall_buy(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    row: u8,
    seen: primitive_shared::stall::Offer,
) {
    use primitive_shared::stall::Refusal;
    let Some(at) = reachable_chest(ctx, handle) else {
        return;
    };
    if !is_stall(ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0)) {
        return;
    }
    let result = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        let stalls = ctx.stalls.lock().unwrap_or_else(|e| e.into_inner());
        match stalls.get(at).and_then(|stall| stall.offer(row as usize)) {
            None => Err(Refusal::NoOffer),
            Some(offer) if offer != seen => Err(Refusal::OfferChanged),
            Some(offer) => {
                let result = chests.edit(at, |store| primitive_shared::stall::trade(&offer, store, &mut state.inventory));
                if result.is_ok() {
                    state.inventory_dirty = true;
                }
                result
            }
        }
    };
    match result {
        Ok(()) => {
            finish_chest_gesture(ctx, handle, at);
            refresh_carried_weight(handle);
        }
        Err(why) => {
            handle.send(ServerMessage::StallRefused { why });
        }
    }
}

// ---- the anvil and the potter's wheel ----
//
// **The screens the server has to be the judge of, and the only ones where
// what it is judging is a moment in time.** The rules are in
// `primitive_shared::minigame`, which both sides read; everything here is the
// part that cannot be shared -- reach, materials, the clock, and the pack.
//
// The shape is the chest's, deliberately: a player opens one station, the
// server remembers *which*, and every message after that carries no position.
// See `ClientMessage::OpenStation`.

/// A player standing at an anvil or a wheel, and the run on it.
pub struct StationSeat {
    /// The cell whose block was opened. Re-checked before a run begins,
    /// because a station can be broken -- possibly by somebody else -- while
    /// its screen is up.
    pub at: (i32, i32, i32),
    pub game: primitive_shared::minigame::Game,
    /// The width of the sweet spot this screen was opened with, from the
    /// hammer that was in the hand at the time. Kept rather than re-read, so
    /// a player cannot widen their own window by swapping to an iron hammer
    /// half way through a run -- and so the screen is scored by exactly the
    /// number it was drawn with.
    pub tolerance: f32,
    /// The job under way: which, when the server agreed to it, and the seed
    /// the sweet spots come out of.
    pub run: Option<(primitive_shared::minigame::Job, Instant, u32)>,
    /// The piece a sawhorse run is cutting, already paid for: what it is (in
    /// its wood), how many, and the boards of that wood a true cut hands
    /// back. See `crafting::begin_piece`.
    pub piece: Option<(primitive_shared::types::BlockId, u32, primitive_shared::types::BlockId)>,
    /// The tool on the honing stone: the slot it was in when the run began
    /// and its kind. **Checked again at the end**, so a player who swapped
    /// the axe for a pick half way through does not get the pick sharpened
    /// for the axe's run.
    pub honing: Option<(usize, primitive_shared::types::BlockId)>,
}

/// How long a run may sit unfinished before it is thrown away.
///
/// Twice the longest run there is, plus a minute for a player who opened the
/// screen and went to make tea. What it stops is the one abuse the clock rule
/// cannot: a client that begins a run, waits an hour and *then* sends a
/// perfect set of timings -- by then `Instant::elapsed` is an hour and every
/// claimed press is trivially "not ahead of the server". A run is a run; an
/// hour at a cooling bar is not.
const STATION_RUN_EXPIRES: Duration = Duration::from_secs(90);

/// A player asking to open the anvil or the wheel they are looking at.
pub(crate) fn open_station(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    use primitive_shared::minigame::{self, Game};
    let (position, held) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        (state.position, state.inventory.block_in(state.selected_slot))
    };
    let Some(game) = station_at(ctx, at) else {
        return;
    };
    if !within_station_reach(ctx, primitive_shared::geometry::narrow(position), at) {
        return;
    }
    // **A hammer in the hand is what makes an anvil an anvil.** Refused here
    // rather than at the first blow, because a screen that opens and then can
    // do nothing is a screen that reads as broken -- and the note says what to
    // go and get. The wheel asks for nothing: a potter works with their hands.
    if game == Game::Anvil && !held.is_some_and(minigame::is_hammer) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NeedHammerAtAnvil });
        return;
    }
    // ...and a saw is what makes a sawhorse one, for the same reason.
    if game == Game::Saw && !held.is_some_and(minigame::is_saw) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NeedSawAtSawhorse });
        return;
    }
    // **The stone sharpens what is in the hand**, so the hand has to hold an
    // edge. Asked at the door rather than at the first stroke, for the
    // anvil's reason.
    if game == Game::Whet && !held.is_some_and(primitive_shared::tools::takes_an_edge) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::HoldBladeToSharpen });
        return;
    }
    let tolerance = minigame::tolerance(match game {
        Game::Anvil | Game::Saw => held,
        Game::Wheel | Game::Whet => None,
    });
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.station = Some(StationSeat { at, game, tolerance, run: None, piece: None, honing: None });
    }
    handle.send(ServerMessage::StationOpen { game, tolerance });
}

/// Which game, if any, the cell at `at` is.
fn station_at(ctx: &Arc<Context>, at: (i32, i32, i32)) -> Option<primitive_shared::minigame::Game> {
    use primitive_shared::minigame::Game;
    use primitive_shared::types::{block_kind, BLOCK_ANVIL, BLOCK_HONING_STONE, BLOCK_POTTERS_WHEEL, BLOCK_SAWHORSE};
    match block_kind(ctx.world.cached_block(at.0, at.1, at.2)?) {
        BLOCK_ANVIL => Some(Game::Anvil),
        BLOCK_POTTERS_WHEEL => Some(Game::Wheel),
        BLOCK_SAWHORSE => Some(Game::Saw),
        BLOCK_HONING_STONE => Some(Game::Whet),
        _ => None,
    }
}

/// The chest's reach, asked of a station: a player who walked away from the
/// anvil is not working at it any more.
fn within_station_reach(ctx: &Arc<Context>, from: (f32, f32, f32), at: (i32, i32, i32)) -> bool {
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    let (dx, dy, dz) = (centre.0 - from.0, centre.1 - from.1, centre.2 - from.2);
    let distance_squared = dx * dx + dy * dy + dz * dz;
    distance_squared.is_finite() && distance_squared <= chest_reach(ctx).powi(2)
}

/// A player asking to start a job. The materials are spent here.
pub(crate) fn station_begin(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    job: primitive_shared::minigame::Job,
) {
    use primitive_shared::minigame::{outcome, Verdict};
    let (at, game, position) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        match state.station.as_ref() {
            // One run at a time. A second "begin" while a run is up is either
            // a double click or a client trying to pay once and score twice.
            Some(seat) if seat.run.is_none() => (seat.at, seat.game, state.position),
            _ => return,
        }
    };
    // **The station is still there and still in reach.** Both can have stopped
    // being true since the screen opened: the player walked off, or somebody
    // else broke the anvil out from under them.
    if job.game() != game
        || station_at(ctx, at) != Some(game)
        || !within_station_reach(ctx, primitive_shared::geometry::narrow(position), at)
    {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NotAtStation });
        return;
    }
    let seed = (logic::rng::Rng::from_clock().next_u64() >> 32) as u32;
    // **Nothing is sent from inside the player's lock.** Every refusal below
    // comes back out as a word and is spoken after it is let go, which is the
    // rule `open_chest` keeps and for its reason: a send that ever grew a lock
    // of its own would deadlock against this one, on a busy server, once.
    let refused = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let state = &mut *state;
        if job == primitive_shared::minigame::Job::Hone {
            // **Nothing is spent at the stone**: the run works the blade in
            // the hand. A sharp one is refused rather than honed for nothing
            // -- the stone would take its metal and give back what it had.
            let slot = state.selected_slot;
            let held = state.inventory.block_in(slot);
            match held {
                Some(block) if primitive_shared::tools::takes_an_edge(block) => {
                    if primitive_shared::tools::blunt_step(block) == 0 {
                        Some("that edge is already sharp")
                    } else {
                        if let Some(seat) = state.station.as_mut() {
                            seat.honing = Some((slot, primitive_shared::types::block_kind(block)));
                            seat.run = Some((job, Instant::now(), seed));
                        }
                        None
                    }
                }
                _ => Some("hold the blade you want to sharpen"),
            }
        } else if let Some(recipe) = job.recipe() {
            // **The row itself, run up to the moment of making** -- any
            // wood's boards, the room checked, all or nothing -- and the
            // piece lifted back off the bench until the cut is judged. The
            // sawhorse stands in for the joiner's bench for the pieces it
            // cuts: it is a second way to a chair, and one that asked for a
            // bench as well would be a toll on the first.
            use primitive_shared::crafting::{begin_piece, Heat, Station};
            match begin_piece(&mut state.inventory, recipe, Heat::NONE.with_workshop(Station::Bench)) {
                Some(piece) => {
                    state.inventory_dirty = true;
                    if let Some(seat) = state.station.as_mut() {
                        seat.piece = Some(piece);
                        seat.run = Some((job, Instant::now(), seed));
                    }
                    None
                }
                None => Some("you are short of what that takes"),
            }
        } else {
        let mut short = false;
        for &(block, amount) in job.inputs() {
            if state.inventory.count(block) < amount {
                short = true;
            }
        }
        if short {
            Some("you are short of what that takes")
        } else {
        // **Room for the best the run could go, checked before a thing is
        // spent.** The crafting table's rule for the crafting table's reason:
        // taking the clay and then finding nowhere to put two pots would
        // destroy them, and "my blocks vanished" is the worst bug report there
        // is. See `crafting::feasibility`.
        let best = outcome(job, Verdict::Fine);
        let mut after = state.inventory.clone();
        for &(block, amount) in job.inputs() {
            after.take_exact(block, amount);
        }
        if best.made.is_some_and(|(block, count)| !after.has_room_for(block, count)) {
            Some("no room for what that would make")
        } else {
            for &(block, amount) in job.inputs() {
                state.inventory.take_exact(block, amount);
            }
            state.inventory_dirty = true;
            // The clock starts when the server says so, and not a moment
            // earlier.
            if let Some(seat) = state.station.as_mut() {
                seat.run = Some((job, Instant::now(), seed));
            }
            None
        }
        }
        }
    };
    if let Some(note) = refused {
        handle.send(ServerMessage::Error(note.to_string()));
        return;
    }
    send_inventory(handle);
    refresh_carried_weight(handle);
    handle.send(ServerMessage::StationBegun { seed });
}

/// A player handing in a whole run: the timing of every blow.
pub(crate) fn station_run(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, presses: Vec<u32>) {
    use primitive_shared::minigame::{self, Game};
    // **Bounded before it is read.** `presses` came off the wire as a `Vec`,
    // and the length check inside `judge` is a rule about the *game*; this one
    // is a rule about this process. A million-element run would be a
    // million-element allocation a client chose.
    if presses.len() > 16 {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NotARun });
        return;
    }
    let taken = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(seat) = state.station.as_mut() else {
            return;
        };
        seat.run.take().map(|run| (seat.game, seat.tolerance, run, seat.piece.take(), seat.honing.take()))
    };
    let Some((game, tolerance, (job, began, seed), piece, honing)) = taken else {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NothingOnAnvil });
        return;
    };
    let elapsed = began.elapsed();
    if elapsed > STATION_RUN_EXPIRES {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::PieceWentCold });
        return;
    }
    let elapsed_ms = elapsed.as_millis().min(u128::from(u32::MAX)) as u32;
    let accuracy = match minigame::judge(game, seed, tolerance, &presses, elapsed_ms) {
        Ok(accuracy) => accuracy,
        Err(refusal) => {
            // The materials stay spent. A refused run is a bar that was heated
            // and a lump that was wedged; what it is not is a free retry, or
            // every cheat in the book would be worth trying once.
            handle.send(ServerMessage::Error(refusal.note().to_string()));
            return;
        }
    };
    let verdict = minigame::verdict(accuracy);
    let mut result = minigame::outcome(job, verdict);
    // **A sawhorse's piece is the one paid for at the start**, in the wood its
    // boards were, and the boards a cut hands back are that wood's too: the
    // table names oak (`minigame::joinery_outcome`), the pack does not.
    if let Some((made, count, boards)) = piece {
        use primitive_shared::types::{block_kind, BLOCK_PLANKS};
        result.made = result.made.map(|_| (made, count));
        for back in &mut result.back {
            if block_kind(back.0) == BLOCK_PLANKS {
                back.0 = boards;
            }
        }
    }
    // **What the pack has no room for falls at the player's feet.** The room
    // was checked when the run began, and a run is seconds long -- long
    // enough to walk over a heap and have the pack filled by it. Both adds
    // below used to drop what they could not place, and a fine run into a
    // full pack paid nothing: the clay spent, and the bowls nowhere.
    let mut no_room: Vec<(primitive_shared::types::BlockId, u32, u32)> = Vec::new();
    let feet = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        // **The stone works the blade that was on it**, in the slot it was
        // in, and only if it is still the same kind of tool. What the run
        // decided is how much metal the edge cost (`tools::hone_by`); the
        // result names the honed tool so the screen can say what it came to.
        if let Some((slot, kind)) = honing {
            let held = state.inventory.slots().get(slot).copied().flatten();
            if let Some(stack) = held.filter(|stack| primitive_shared::types::block_kind(stack.block) == kind) {
                let honed = primitive_shared::tools::hone_by(stack, verdict);
                state.inventory.take_from(slot, stack.count);
                let _ = state.inventory.put_in_slot(slot, honed);
                result.made = Some((honed.block, 1));
            }
        }
        if let Some((block, count)) = result.made.filter(|_| honing.is_none()) {
            // **The run is half of the piece and the smith is the other
            // half** (`quality::Maker::aim`). What the mini-game already
            // decided is *how much* comes out (`minigame::outcome`); what
            // it decides here is how good it is, together with how tired
            // and how comfortable the person swinging the hammer was.
            // Feeding the accuracy in rather than inventing a second
            // scale is the whole reason `Maker::accuracy` is an option.
            //
            // The hammer is the tool for an anvil job; a potter's hands
            // are the right hands, which is `1.0` and not zero -- see
            // `Maker::craft`.
            //
            // At the sawhorse the saw is the tool, and its edge counts as a
            // chisel's does at the bench (`crafting::tool_goodness`): a blunt
            // saw tears the fibres at the end of the cut.
            let held = state.inventory.slots().get(state.selected_slot).copied().flatten();
            let tool = match game {
                Game::Anvil => held.filter(|stack| minigame::is_hammer(stack.block)).map_or(0.0, |stack| {
                    0.75 * stack.condition() + 0.25 * stack.quality().fraction()
                }),
                Game::Saw => held.filter(|stack| minigame::is_saw(stack.block)).map_or(0.0, |stack| {
                    (0.75 * stack.condition() + 0.25 * stack.quality().fraction())
                        * primitive_shared::tools::edge_factor(stack.block)
                }),
                Game::Wheel | Game::Whet => 1.0,
            };
            let maker = maker_of(&state, true, tool, Some(accuracy));
            let quality = maker.judge(logic::rng::Rng::from_clock().range(0.0, 1.0));
            let word = if primitive_shared::quality::takes_quality(block) {
                primitive_shared::inventory::Stack::new(block, 0)
                    .with_quality(quality)
                    .damage
            } else {
                // A handful of nails is a handful of nails. See
                // `quality::takes_quality` for what marking them costs.
                0
            };
            let left = state.inventory.add_worn(block, count, word);
            if left > 0 {
                no_room.push((block, left, word));
            }
        }
        for &(block, amount) in &result.back {
            let left = state.inventory.add(block, amount);
            if left > 0 {
                no_room.push((block, left, 0));
            }
        }
        // **The hammer takes a blow's worth of wear, and more for a bad run.**
        // Worn from the selected slot, which is where the hand is: a player who
        // swapped tools mid-run wears whatever they swapped *to*, and that is
        // the honest answer -- the run was still scored by the head they
        // started with (`StationSeat::tolerance`), which is the half that could
        // have been cheated.
        //
        // A saw at the sawhorse the same way: a point of wear a piece, and
        // the edge dulls on that wear like any other (`wear_tool`).
        let worn_here = match game {
            Game::Anvil => Some(minigame::is_hammer as fn(primitive_shared::types::BlockId) -> bool),
            Game::Saw => Some(minigame::is_saw as fn(primitive_shared::types::BlockId) -> bool),
            Game::Wheel | Game::Whet => None,
        };
        if let Some(is_the_tool) = worn_here {
            let slot = state.selected_slot;
            if state.inventory.block_in(slot).is_some_and(is_the_tool) {
                for _ in 0..=result.extra_wear {
                    state.inventory.wear_tool(slot);
                }
            }
        }
        state.inventory_dirty = true;
        state.position
    };
    if !no_room.is_empty() {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        for (block, count, word) in no_room {
            items.spawn_worn(block, count, word, (feet.0, feet.1 + 0.5, feet.2), (0.0, 0.0, 0.0), None, Instant::now());
        }
    }
    send_inventory(handle);
    refresh_carried_weight(handle);
    handle.send(ServerMessage::StationResult { verdict, made: result.made });
}

/// A player shutting the screen, or having it shut for them.
pub(crate) fn close_station(handle: &Arc<players::PlayerHandle>) {
    // A run abandoned with the screen is a run lost, materials and all. It has
    // to be: the alternative is a player who begins a job, sees the first
    // sweet spot land badly and closes the screen to get their bar back, which
    // would make the game a re-roll rather than a run.
    handle.state.lock().unwrap_or_else(|e| e.into_inner()).station = None;
}

/// What a container at `at` is, and what its fire is doing.
///
/// One function, because the two facts travel together in every message
/// about a container and working them out twice is how a hearth ends up
/// drawn as a chest.
fn describe_container(
    ctx: &Arc<Context>,
    at: containers::ChestPos,
    contents: &primitive_shared::inventory::Inventory,
) -> (
    primitive_shared::protocol::ContainerKind,
    Option<primitive_shared::protocol::HearthState>,
    Option<primitive_shared::protocol::RackState>,
) {
    use primitive_shared::protocol::{ContainerKind, HearthState, RackState};
    let block = ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0);
    if primitive_shared::rack::is_rack(block) {
        // **Every lock taken and let go before the next.** The tick loop
        // holds the fires, the chests and the racks at once, in that
        // order; this runs on a connection thread, and taking any two of
        // them the other way round would be the deadlock that only
        // happens on a busy server. Nothing here needs two at a time.
        let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
        let ambient = {
            let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
            crate::climate::Ambient::of(
                &ctx.world,
                &fires,
                (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
                ctx.clock.world_days(),
                weather,
            )
        };
        let progress = {
            let racks = ctx.drying.lock().unwrap_or_else(|e| e.into_inner());
            racks.progress_at(at)
        };
        // **A frame with nothing on it reports nothing**, because that
        // is what the next step will find as well -- see `Drying::step`,
        // which drops the progress of an empty rack. A frame with a skin
        // on it and a full tray reports how far along that skin is: the
        // wait is kept, and a bar sitting at ninety-nine per cent over
        // the words "the tray is full" is the screen saying exactly what
        // to do about it.
        let on_the_frame = contents.block_in(primitive_shared::rack::HIDE_SLOT);
        let progress = match on_the_frame.and_then(primitive_shared::rack::cures_into) {
            Some(_) => progress,
            None => 0.0,
        };
        return (
            ContainerKind::Rack,
            None,
            Some(RackState {
                progress,
                rate: drying::Drying::rate(&ambient, weather),
                wet: ambient.getting_wet || (weather.is_wet() && !ambient.sheltered),
                near_fire: ambient.near_fire,
            }),
        );
    }
    // A stall's offers travel in a message of their own, just before this
    // one (`tell_stall_offers`): what is in it is all `ChestState` carries.
    if is_stall(block) {
        return (ContainerKind::Stall, None, None);
    }
    // A jug has nothing going on inside it but what is in it: no fire and
    // no weather, so only its kind travels, which is what gives it a
    // one-slot screen rather than forty squares with some seeds in one.
    if primitive_shared::types::opens_as_vessel(block) {
        return (ContainerKind::Vessel, None, None);
    }
    let Some(kind) = primitive_shared::hearth::Kind::of(block) else {
        return (ContainerKind::Chest, None, None);
    };
    let (fuel_left, degrees, wet) = {
        let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
        (
            fires.fuel_left(at).unwrap_or(0.0),
            fires.degrees(at),
            fires.is_wet(at),
        )
    };
    let progress = {
        let smelting = ctx.smelting.lock().unwrap_or_else(|e| e.into_inner());
        smelting.progress(at, contents, kind)
    };
    (
        ContainerKind::Hearth(kind),
        Some(HearthState {
            fuel_left,
            progress: progress.fraction(),
            degrees,
            // The heat the batch is held to, from the same function the
            // tick holds it to -- so the line on the gauge is the line.
            needs: progress.needs,
            wet,
        }),
        None,
    )
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
    let (kind, hearth, rack) = describe_container(ctx, at, &inventory);
    tell_stall_offers(ctx, handle, at);
    handle.send(ServerMessage::ChestState {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        inventory,
        kind,
        hearth,
        rack,
    });
}

/// One tick of the vermin: warm the map, look for somewhere to put a rat,
/// and go through the containers near the ones that are already out.
///
/// Answers which containers changed, so the caller can tell whoever has
/// one open -- the same contract `rot::Rot::pass` has.
///
/// **The light is computed once per chunk and thrown away.** A cell's
/// light level costs one chunk's flood fill (`api_impl::chunk_light` says
/// why it is never cached), and a raid can ask about several chests in the
/// same room; the memo below is for the length of one raid and no longer,
/// so nothing here can ever answer with a light level from a torch that
/// has since burned out.
fn step_vermin(
    ctx: &Arc<Context>,
    vermin: &mut logic::vermin::Vermin,
    where_everyone_is: &[(PlayerId, (f32, f32, f32))],
    dt: f32,
) -> Vec<containers::ChestPos> {
    let standing: Vec<(f32, f32)> = where_everyone_is.iter().map(|&(_, at)| (at.0, at.2)).collect();
    vermin.tick(&standing, dt);
    // Nobody about is nobody to steal from, and an empty server must not
    // be paying for a flood fill every twenty seconds.
    if standing.is_empty() {
        return Vec::new();
    }
    let night = ctx.clock.time_of_day().rem_euclid(1.0);
    let night = !(0.25..0.75).contains(&night);

    if vermin.spawn_due(dt) && night {
        make_a_rat(ctx, vermin);
    }

    if !vermin.raid_due(dt) {
        return Vec::new();
    }
    let rats = {
        let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.where_the(primitive_shared::animals::Species::Rat)
    };
    if rats.is_empty() {
        return Vec::new();
    }
    let mut lights: std::collections::HashMap<primitive_shared::types::ChunkPos, Vec<u8>> =
        std::collections::HashMap::new();
    let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
    logic::vermin::raid(&mut chests, &rats, |at| light_for_vermin(ctx, &mut lights, at))
}

/// Puts one rat in the darkest lived-in corner there is, if the world
/// will take another.
fn make_a_rat(ctx: &Arc<Context>, vermin: &logic::vermin::Vermin) {
    use primitive_shared::animals::Species;
    let living = {
        let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.where_the(Species::Rat).len()
    };
    if logic::vermin::Vermin::room_for_more(living) == 0 {
        return;
    }
    // **One flood fill a chunk, not one a column.** `dark_floor_in` walks
    // up to sixty-four columns of a cell and gives up only when every one
    // of them is lit -- which is exactly what happens to a player who has
    // done the one thing the mechanic asks of them and put a torch in the
    // storeroom. Asking the world about each column on its own meant a
    // chunk decode and a whole-chunk light pass apiece (0.70 ms measured,
    // `lighting::bench_lighting`), so a lit home cost forty-five
    // milliseconds a cell and a world with a few lived-in places stalled
    // the tick for half a second every thirty seconds -- a hitch you
    // earned by defending yourself properly. An eight-block cell covers
    // four chunks at the very most and usually one, so the memo turns
    // sixty-four fills into one or four. It is the same memo
    // `light_for_vermin` keeps for a raid, and it lives exactly as long:
    // one pass, so no answer here can come from a torch that has since
    // burnt out.
    let mut probe = ColumnProbe::new(ctx);
    let at = vermin.somewhere_to_come_out(true, |cell| {
        logic::vermin::dark_floor_in(cell, true, |x, z| probe.standing_room(x, z))
    });
    let Some(at) = at else {
        return;
    };
    let born = {
        let mut animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        animals.spawn_at(Species::Rat, at)
    };
    if let Some(entity) = born {
        entity_appeared(ctx, entity, at, true);
    }
}

/// Somewhere to stand, column by column, with the chunks it has already
/// looked at kept for the length of one spawner pass.
///
/// The memo is the whole reason this is a struct rather than a function:
/// see `make_a_rat`. `fills` is what the test watches -- the number of
/// light passes actually run, which is the cost, as opposed to the number
/// of columns asked about, which is free.
struct ColumnProbe<'a> {
    ctx: &'a Arc<Context>,
    seen: std::collections::HashMap<
        primitive_shared::types::ChunkPos,
        (Vec<primitive_shared::types::BlockId>, Vec<u8>),
    >,
    fills: usize,
}

impl<'a> ColumnProbe<'a> {
    fn new(ctx: &'a Arc<Context>) -> ColumnProbe<'a> {
        ColumnProbe { ctx, seen: std::collections::HashMap::new(), fills: 0 }
    }

    /// A place to stand in this column, and how dark it is there.
    ///
    /// `None` for a column in a chunk nobody has loaded, for one with no
    /// floor, and for one whose floor is under water -- a rat is not a
    /// fish, and a vermin spawner that did not say so would put them in
    /// the bottom of the well.
    fn standing_room(&mut self, x: i32, z: i32) -> Option<(f32, u8)> {
        use primitive_shared::types::{is_air, is_liquid, CHUNK_SIZE_Y};
        let (pos, lx, lz) = primitive_shared::types::ChunkPos::from_global(x, z);
        let (blocks, light) = match self.seen.entry(pos) {
            std::collections::hash_map::Entry::Occupied(held) => held.into_mut(),
            std::collections::hash_map::Entry::Vacant(empty) => {
                // A chunk nobody has loaded is not memoised as "missing":
                // it is a handful of columns at the edge of a cell, and
                // holding a `None` for it would mean a second map to say
                // which chunks were absent. Absent chunks cost a cache
                // lookup and nothing else.
                let chunk = self.ctx.world.cached(pos)?;
                let blocks = chunk.to_blocks();
                let light = primitive_shared::lighting::compute_isolated(&blocks);
                self.fills += 1;
                empty.insert((blocks, light))
            }
        };
        // Downwards from the top: the first air over something solid is the
        // floor of whatever room is highest here, which is the floor a player
        // walking about is on.
        for y in (1..CHUNK_SIZE_Y - 1).rev() {
            let here = primitive_shared::types::Chunk::index(lx, y, lz);
            let below = primitive_shared::types::Chunk::index(lx, y - 1, lz);
            let head = primitive_shared::types::Chunk::index(lx, y + 1, lz);
            let (floor, air, over) = (blocks[below], blocks[here], blocks[head]);
            if is_air(floor) || is_liquid(floor) || !is_air(air) || !is_air(over) {
                continue;
            }
            // Whichever of the two is brighter: a torch and the sun both keep
            // rats out, and reading only the block light would make a doorway
            // at noon a nest.
            let packed = light.get(here).copied().unwrap_or(0);
            let lit = (packed & 0x0F).max((packed >> 4) & 0x0F);
            return Some((y as f32, lit));
        }
        None
    }
}

/// The light over one container, memoised by chunk for the length of one
/// raid. See `step_vermin`.
fn light_for_vermin(
    ctx: &Arc<Context>,
    lights: &mut std::collections::HashMap<primitive_shared::types::ChunkPos, Vec<u8>>,
    at: containers::ChestPos,
) -> u8 {
    use primitive_shared::types::CHUNK_SIZE_Y;
    if at.1 < 0 || at.1 >= CHUNK_SIZE_Y as i32 {
        return 0;
    }
    let (pos, lx, lz) = primitive_shared::types::ChunkPos::from_global(at.0, at.2);
    let light = match lights.entry(pos) {
        std::collections::hash_map::Entry::Occupied(held) => held.into_mut(),
        std::collections::hash_map::Entry::Vacant(empty) => {
            let Some(chunk) = ctx.world.cached(pos) else {
                // A chest in a chunk nobody has loaded is not raided at
                // all: `raid` is only ever asked about chests beside a
                // rat, and a rat is in a loaded chunk by definition. A
                // bright answer is the safe one either way.
                return 15;
            };
            empty.insert(primitive_shared::lighting::compute_isolated(&chunk.to_blocks()))
        }
    };
    let packed = light
        .get(primitive_shared::types::Chunk::index(lx, at.1 as usize, lz))
        .copied()
        .unwrap_or(0);
    (packed & 0x0F).max((packed >> 4) & 0x0F)
}

/// Sends it to *everyone* standing at that chest.
///
/// The reason two players can share one: whoever changed it is told the
/// same way everyone else is, so there is one code path and no chance of
/// the mover seeing something the others do not.
fn broadcast_chest_state(ctx: &Arc<Context>, at: containers::ChestPos) {
    // A body's contents are also what it is drawn wearing, and *that* is
    // for everybody who can see it, watching or not. Every gesture that
    // changes what is in a container ends here, which is why it is asked
    // here and not at each of them.
    tell_body_worn(ctx, at);
    // ...and a thing set down is drawn as what is in it, on the same terms
    // -- the rot clock turning a loaf in one ends here too.
    tell_set_down(ctx, at);
    // **Nobody looking, nothing to send.** Asked first because building
    // the message is the expensive half -- a forty-slot snapshot cloned
    // per watcher, and for a rack a climate sample as well -- and the
    // racks call this every two seconds for every frame in a loaded
    // chunk, which is mostly frames nobody is standing at.
    let watched = ctx.registry.handles().iter().any(|handle| {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.open_chest == Some(at)
    });
    if !watched {
        return;
    }
    let inventory = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at)
    };
    let (kind, hearth, rack) = describe_container(ctx, at, &inventory);
    for handle in ctx.registry.handles() {
        let watching = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.open_chest == Some(at)
        };
        if watching {
            tell_stall_offers(ctx, &handle, at);
            handle.send(ServerMessage::ChestState {
                global_x: at.0,
                global_y: at.1,
                global_z: at.2,
                inventory: inventory.clone(),
                kind,
                hearth,
                rack,
            });
        }
    }
}

/// Shuts the screen of everyone who has this chest open.
///
/// Called when the block goes. Leaving them looking at a chest that no
/// longer exists is how a player puts something into nothing.
pub(crate) fn close_chest_for_everyone(ctx: &Arc<Context>, at: containers::ChestPos) {
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
            container_closed(ctx, handle.id, at);
        }
    }
    // The lid comes down for everyone watching it, which is not the same
    // set: the block may have gone, and then there is nothing to draw a lid
    // on and this says nothing (`tell_chest_lid`).
    tell_chest_lid(ctx, at);
}

/// Which set of slot rules the container at `at` plays by.
///
/// **Three containers, one store.** A chest is forty slots that mean
/// nothing in particular; a hearth is seven that mean something (see
/// `primitive_shared::hearth`); a drying rack is two (see
/// `primitive_shared::rack`). Every gesture below asks this once and
/// then follows one set of rules, which is what stops the third kind of
/// container from being a third copy of the chest code.
///
/// A cell nobody has loaded, or one holding something that is not a
/// container at all, reads as a chest -- the harmless answer: the
/// gesture is refused a moment later by `usable_chest`, which is where
/// "is that still a container" belongs.
#[derive(Clone, Copy, PartialEq)]
enum Roles {
    Chest,
    /// A dead player's body, or the bones it rots to: a chest's rules,
    /// over a chest's forty and -- when the player died wearing a rucksack
    /// -- the rucksack's compartment after them (`CORPSE_COMPARTMENT`).
    ///
    /// **Its own role rather than a chest with a longer limit**, because the
    /// limit is the whole difference and it has to be refused somewhere. A
    /// chest that accepted square forty-five would be a chest whose client
    /// could put things into squares it never draws; a save edited, or a
    /// cell that was a body and is now a chest, would then be a box with a
    /// hidden back room.
    Body,
    Hearth(primitive_shared::hearth::Kind),
    /// A rack, and which of the two: the larder of two by two or the hide
    /// frame (`rack::Trade`). The slots are the same; what goes on the
    /// frame is not.
    Rack(primitive_shared::rack::Trade),
    /// A set-down jug: one slot, loose goods only, a jug's measure of
    /// them. See `inventory::VESSEL_SLOT`.
    Vessel,
    /// A barter stall: the counter takes anything, the till nothing by hand
    /// (`stall::accepts`). Who may touch either is `usable_chest`'s.
    Stall,
    /// A horse's saddlebags: a chest's rules over `horse::BAGS_SLOTS`
    /// squares. Never read off a cell -- the bags are on a horse, not in the
    /// world -- so `roles_at` never answers it; `horses::with_open_bags`
    /// hands it in.
    Bags,
}

fn roles_at(ctx: &Arc<Context>, at: containers::ChestPos) -> Roles {
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return Roles::Chest;
    };
    if let Some(kind) = primitive_shared::hearth::Kind::of(block) {
        return Roles::Hearth(kind);
    }
    if let Some(trade) = primitive_shared::rack::Trade::of(block) {
        return Roles::Rack(trade);
    }
    if primitive_shared::types::opens_as_vessel(block) {
        return Roles::Vessel;
    }
    if is_stall(block) {
        return Roles::Stall;
    }
    if primitive_shared::types::is_corpse(block) {
        return Roles::Body;
    }
    Roles::Chest
}

/// May this block be put into that slot of the container at `at`? See
/// `Roles::accepts`, which is the rule once the world has said which
/// container it is.
fn container_accepts(
    ctx: &Arc<Context>,
    at: containers::ChestPos,
    slot: usize,
    block: primitive_shared::types::BlockId,
) -> bool {
    roles_at(ctx, at).accepts(slot, block)
}

impl Roles {
    /// May this block be put into that slot of a container playing by
    /// these rules?
    ///
    /// **A chest takes anything anywhere; a hearth does not, and neither
    /// does a rack.** Nothing goes into a hearth's output slots -- they are
    /// where it puts things, and a player who can fill them can jam the
    /// furnace -- only fuel goes in the fuel slot, and nothing but a skin
    /// goes on a frame. Checked here as well as drawn on the client, for the
    /// reason every other rule in this file is checked here: the client is
    /// a request, not an authority.
    fn accepts(self, slot: usize, block: primitive_shared::types::BlockId) -> bool {
        match self {
            // `CHEST_SLOTS`, not the player's own count: a box in the
            // world stayed forty squares when the pack was halved, and
            // validating against the pack would have made the back half
            // of every chest unreachable -- with everything already in
            // it still there and no way to get it out.
            Roles::Chest => slot < primitive_shared::inventory::CHEST_SLOTS,
            // Up to the end of a rucksack's compartment. A body that has
            // none is forty long, and a move into a square it does not have
            // comes back from `put_in_slot` whole and goes back where it
            // came from -- so the length of the body is the rest of the
            // rule, and it is not a second thing to keep true here.
            Roles::Body => slot < primitive_shared::inventory::CORPSE_SLOTS,
            Roles::Bags => slot < primitive_shared::horse::BAGS_SLOTS,
            Roles::Hearth(_) => primitive_shared::hearth::accepts(slot, block),
            Roles::Rack(trade) => primitive_shared::rack::accepts_on(trade, slot, block),
            // One slot, and only what a jug in the hand would take -- the
            // same `jug_room` question, so a jug on a shelf is not a way
            // round the rot clock or round a vessel inside a vessel.
            Roles::Vessel => {
                slot == primitive_shared::inventory::VESSEL_SLOT
                    && primitive_shared::inventory::jug_room(None, block) > 0
            }
            Roles::Stall => primitive_shared::stall::accepts(slot, block),
        }
    }
}

/// Where a shift-click from the pack should land in a hearth.
///
/// Fuel goes to the fuel slot and everything else to the ingredients,
/// which is the only routing a player would ever want and saves them
/// dragging charcoal to a particular square. `None` for a block a hearth
/// has no use for at all -- an ingot, say -- which then stays in the
/// pack rather than being swallowed by a slot it will never leave.
fn hearth_target(contents: &primitive_shared::inventory::Inventory, block: primitive_shared::types::BlockId) -> Option<std::ops::Range<usize>> {
    use primitive_shared::hearth::{FUEL_SLOT, INPUT_SLOTS};
    if primitive_shared::hearth::is_fuel(block) {
        let fuel = FUEL_SLOT..FUEL_SLOT + 1;
        // ...unless the fuel slot is full of something else, in which
        // case it is an ingredient: charcoal is both, and a hearth with
        // a full fuel slot should still take coal to smelt with.
        match contents.block_in(FUEL_SLOT) {
            None => return Some(fuel),
            Some(held) if held == block => return Some(fuel),
            Some(_) => {}
        }
    }
    Some(INPUT_SLOTS)
}

/// Where a shift-click from the pack should land in a rack.
///
/// The frame, or nowhere. A rack has exactly one slot a player may put
/// anything in and exactly one kind of thing that goes in it, so the
/// gesture is either "put this skin on the frame" or it is not a gesture
/// at all -- and a stone that vanished into a rack would be a stone the
/// player has to fish back out.
fn rack_target(trade: primitive_shared::rack::Trade, block: primitive_shared::types::BlockId) -> Option<std::ops::Range<usize>> {
    use primitive_shared::rack::HIDE_SLOT;
    trade.takes(block).then_some(HIDE_SLOT..HIDE_SLOT + 1)
}

/// Says why a rack refused what was offered to it, **when the reason is that
/// it is the other rack's work** (`rack::refused_for_its_trade`): a skin at
/// the larder, a fish at the frame. Silent for everything else, which is
/// the refusal a stone always got.
fn tell_rack_refused(handle: &Arc<players::PlayerHandle>, roles: Roles, offered: Option<primitive_shared::types::BlockId>) {
    let (Roles::Rack(trade), Some(offered)) = (roles, offered) else {
        return;
    };
    if primitive_shared::rack::refused_for_its_trade(trade, offered) {
        handle.send(ServerMessage::RackRefused { rack: trade });
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
    // **The saddlebags first**: a player with a horse's bags open is at the
    // bags, and has no chest open (`horses::open_bags` shuts it).
    if horses::with_open_bags(ctx, handle, |pack, bags| {
        container_move(pack, bags, Roles::Bags, (from.0, from.1 as usize), (to.0, to.1 as usize), half)
    })
    .is_some()
    {
        return;
    }
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let (from_side, from_slot) = (from.0, from.1 as usize);
    let (to_side, to_slot) = (to.0, to.1 as usize);
    if from_side == to_side && from_slot == to_slot {
        return;
    }

    // Read once, before either lock: `roles_at` reads the world, and the
    // pure half below must not reach for anything.
    let roles = roles_at(ctx, at);
    // What is being offered to the container, for the one refusal that is
    // said out loud (`tell_rack_refused`). Read before the move, because
    // after it the stack is somewhere else.
    let offered = match (from_side, to_side) {
        (Side::Pack, Side::Chest) => handle.state.lock().unwrap_or_else(|e| e.into_inner()).inventory.block_in(from_slot),
        _ => None,
    };
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| {
            container_move(
                &mut state.inventory,
                chest,
                roles,
                (from_side, from_slot),
                (to_side, to_slot),
                half,
            )
        })
    };

    if changed {
        finish_chest_gesture(ctx, handle, at);
    } else {
        tell_rack_refused(handle, roles, offered);
    }
}

/// The whole of a `ChestMove`, with the world already asked what kind of
/// container this is.
///
/// Pure, so that the rule about which slot may hold what can be tested on
/// two inventories rather than on a server.
fn container_move(
    pack: &mut primitive_shared::inventory::Inventory,
    chest: &mut primitive_shared::inventory::Inventory,
    roles: Roles,
    (from_side, from_slot): (Side, usize),
    (to_side, to_slot): (Side, usize),
    half: bool,
) -> bool {
    use primitive_shared::inventory::{move_between, split_between};
    let allowed = |slot: usize, block: primitive_shared::types::BlockId| roles.accepts(slot, block);
    // **A swap is two moves, and the second one goes into the container
    // too.** Only the first used to be asked: a player lifting supper out
    // of the tray and dropping it on a square of the belt holding sticks
    // got the supper -- and the sticks went into the output tray, which
    // nothing may be put into, and the fire stopped with nowhere to put
    // the next steak. The same on a rack (a stone on the leather peg) and
    // in a jug (an axe inside it). A different block in the way of a move
    // out of a container is fine only if the slot it lands back in takes
    // it; otherwise the gesture does nothing, exactly as a refused drop in
    // does.
    let swap_back_allowed = |slot: usize,
                             moving: primitive_shared::types::BlockId,
                             in_the_way: Option<primitive_shared::types::BlockId>| {
        match in_the_way {
            Some(there) if there != moving => allowed(slot, there),
            _ => true,
        }
    };
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
            // Rearranging inside the container obeys the same
            // rule: sliding ore from an input slot into the
            // output tray is the same jam by another route.
            match chest.block_in(from_slot) {
                Some(block) if allowed(to_slot, block) => {
                    if half {
                        chest.split_into(from_slot, to_slot)
                    } else if swap_back_allowed(from_slot, block, chest.block_in(to_slot)) {
                        chest.move_or_merge(from_slot, to_slot)
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
        // **Into a set-down jug, all that fits, half or not.**
        // `move_between` would put a whole stack of 128 seeds into
        // the jug's slot, because the store's slot limit is the
        // pack's; the jug's measure is `JUG_UNITS`, and only
        // `pour_into_vessel` knows it. The same pour the jug in the
        // hand gets, so the jug on the shelf is not a bigger jug.
        (Side::Pack, Side::Chest) if roles == Roles::Vessel => {
            to_slot == primitive_shared::inventory::VESSEL_SLOT
                && primitive_shared::inventory::pour_into_vessel(pack, from_slot, chest) > 0
        }
        (Side::Pack, Side::Chest) => {
            // The one gesture a hearth can refuse: what the
            // player is holding has to be something that slot
            // takes. Read before the move, because after it the
            // stack is somewhere else.
            match pack.block_in(from_slot) {
                Some(block) if allowed(to_slot, block) => {
                    if half {
                        split_between(pack, from_slot, chest, to_slot)
                    } else {
                        move_between(pack, from_slot, chest, to_slot)
                    }
                }
                _ => false,
            }
        }
        (Side::Chest, Side::Pack) => match chest.block_in(from_slot) {
            Some(_) if half => split_between(chest, from_slot, pack, to_slot),
            Some(block) if swap_back_allowed(from_slot, block, pack.block_in(to_slot)) => {
                move_between(chest, from_slot, pack, to_slot)
            }
            _ => false,
        },
    }
}

/// The shift-click: a whole slot to the other side.
pub(crate) fn chest_quick_move(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    side: Side,
    slot: u8,
) {
    if horses::with_open_bags(ctx, handle, |pack, bags| match side {
        Side::Chest => primitive_shared::inventory::quick_move_between(bags, slot as usize, pack),
        Side::Pack => shift_into_roles(pack, slot as usize, bags, Roles::Bags),
    })
    .is_some()
    {
        return;
    }
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let slot = slot as usize;
    let roles = roles_at(ctx, at);
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| {
            use primitive_shared::inventory::quick_move_between;
            match side {
                // Out of a container: the same whatever it is. A
                // shift-click on the output tray is how a player empties
                // a furnace -- or takes the leather off a rack -- and it
                // must not be refused because the slot it came from does
                // not "accept" what is in it.
                Side::Chest => quick_move_between(chest, slot, &mut state.inventory),
                // ...and in, routed rather than dropped in the first
                // slot with room. See `shift_into_roles`.
                Side::Pack => shift_into_roles(&mut state.inventory, slot, chest, roles),
            }
        })
    };
    if changed {
        finish_chest_gesture(ctx, handle, at);
    } else if side == Side::Pack {
        let offered = handle.state.lock().unwrap_or_else(|e| e.into_inner()).inventory.block_in(slot);
        tell_rack_refused(handle, roles, offered);
    }
}

/// Every stack of one kind across to the other side: the ctrl-click. See
/// `ClientMessage::ChestMoveKind`.
///
/// Each stack goes exactly as its own shift-click would -- out of a
/// container anywhere with room, into one by role -- so this is the same
/// rule as `chest_quick_move` applied to every slot that holds the kind,
/// and a hearth cannot be filled the wrong way round by it.
pub(crate) fn chest_move_kind(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, side: Side, slot: u8) {
    if horses::with_open_bags(ctx, handle, |pack, bags| move_kind(pack, bags, Roles::Bags, side, slot)).is_some() {
        return;
    }
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let roles = roles_at(ctx, at);
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| move_kind(&mut state.inventory, chest, roles, side, slot))
    };
    if changed {
        finish_chest_gesture(ctx, handle, at);
    }
}

/// The whole of a `ChestMoveKind` between a pack and a container: pure, so
/// the chest and the saddlebags share it.
fn move_kind(
    pack: &mut primitive_shared::inventory::Inventory,
    chest: &mut primitive_shared::inventory::Inventory,
    roles: Roles,
    side: Side,
    slot: u8,
) -> bool {
    use primitive_shared::inventory::{quick_move_between, SLOTS};
    use primitive_shared::types::block_kind;
    let from: &primitive_shared::inventory::Inventory = match side {
        Side::Chest => chest,
        Side::Pack => pack,
    };
    let Some(kind) = from.block_in(slot as usize).map(block_kind) else {
        return false;
    };
    let mut moved = false;
    // Every square the side being emptied has -- a body's
    // compartment included, for the reason `chest_bulk_move` gives.
    let squares = match side {
        Side::Chest => chest.slots().len(),
        Side::Pack => SLOTS,
    };
    for each in 0..squares {
        match side {
            Side::Chest => {
                if chest.block_in(each).map(block_kind) == Some(kind) {
                    moved |= quick_move_between(chest, each, pack);
                }
            }
            Side::Pack => {
                if pack.block_in(each).map(block_kind) == Some(kind) {
                    moved |= shift_into_roles(pack, each, chest, roles);
                }
            }
        }
    }
    moved
}

/// A shift-click from the pack into a container with roles.
///
/// Not `quick_move_between`, which fills the first slot with room --
/// in a hearth that is an input slot, so charcoal shift-clicked at a
/// kiln would go in as an *ingredient* and never be burnt, and in a rack
/// it is the frame, so anything at all would land on it. What a player
/// means by the gesture is "put this where it belongs", and where it
/// belongs is a fact about the block.
fn shift_into_roles(
    pack: &mut primitive_shared::inventory::Inventory,
    slot: usize,
    container: &mut primitive_shared::inventory::Inventory,
    roles: Roles,
) -> bool {
    let Some(stack) = pack.slots().get(slot).copied().flatten() else {
        return false;
    };
    let range = match roles {
        // A chest has no roles at all: the first slot with room is
        // exactly where the player means it to go.
        Roles::Chest | Roles::Body => {
            return primitive_shared::inventory::quick_move_between(pack, slot, container)
        }
        Roles::Hearth(_) => hearth_target(container, stack.block),
        Roles::Rack(trade) => rack_target(trade, stack.block),
        // Onto the counter, the only place a hand puts anything.
        Roles::Stall => Some(primitive_shared::stall::STOCK),
        Roles::Bags => Some(0..primitive_shared::horse::BAGS_SLOTS),
        // A jug's slot, capped at a jug's measure -- `add_within` would
        // cap it at a stack. See the same arm in `chest_move`.
        Roles::Vessel => {
            return primitive_shared::inventory::pour_into_vessel(pack, slot, container) > 0
        }
    };
    let Some(range) = range else {
        return false;
    };
    // `add_worn_within`, carrying the stack's `damage`: this was
    // `add_within`, and a worn pick shift-clicked into a campfire came back
    // out of it new -- and a jug of grain came back empty. See
    // `Inventory::add_worn_within`.
    let left = container.add_worn_within(range, stack.block, stack.count, stack.damage);
    let moved = stack.count - left;
    if moved == 0 {
        return false;
    }
    pack.take_from(slot, moved);
    true
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
    if horses::with_open_bags(ctx, handle, |pack, bags| bulk_move(pack, bags, Roles::Bags, to_chest)).is_some() {
        return;
    }
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    let roles = roles_at(ctx, at);
    let changed = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| bulk_move(&mut state.inventory, chest, roles, to_chest))
    };
    if changed {
        finish_chest_gesture(ctx, handle, at);
    }
}

/// The whole of a `ChestBulkMove`: pure, for `move_kind`'s reason.
fn bulk_move(
    pack: &mut primitive_shared::inventory::Inventory,
    chest: &mut primitive_shared::inventory::Inventory,
    roles: Roles,
    to_chest: bool,
) -> bool {
    use primitive_shared::inventory::{quick_move_between, SLOTS};
    let mut moved = false;
    // Out of a container, every square it has: TAKE ALL at a body
    // stopped at the fortieth, and the rucksack's compartment after
    // it was the one part of a dead player's things the button
    // did not reach.
    let squares = if to_chest { SLOTS } else { chest.slots().len() };
    for slot in 0..squares {
        moved |= if to_chest {
            shift_into_roles(pack, slot, chest, roles)
        } else {
            quick_move_between(chest, slot, pack)
        };
    }
    moved
}

/// Folds the part-stacks in an open container together.
///
/// **Not offered for a hearth or a rack**, and the reason is the roles:
/// tidying sorts by block and packs from the top, which would shovel the
/// fuel into an ingredient slot and the results back in with the ore --
/// or slide the leather back onto the frame. Their slots mean things; a
/// chest's are forty of the same.
pub(crate) fn chest_sort(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let Some(at) = usable_chest(ctx, handle) else {
        return;
    };
    // A body tidies like a chest; `sort_all` keeps its rucksack's
    // compartment on its own side of the seam.
    if !matches!(roles_at(ctx, at), Roles::Chest | Roles::Body) {
        return;
    }
    let changed = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.edit(at, |chest| chest.sort_all())
    };
    if changed {
        broadcast_chest_state(ctx, at);
    }
}

/// The chest this player may act on right now, if any.
///
/// **A stall is somebody's**, and every gesture that moves a thing in or
/// out of a container comes through here -- so this is where anybody but
/// its owner is refused, once, rather than in each of six gestures. A
/// buyer's one gesture at a stall is `stall_buy`, which asks
/// `reachable_chest` instead.
fn usable_chest(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
) -> Option<containers::ChestPos> {
    let at = reachable_chest(ctx, handle)?;
    if is_stall(ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0))
        && !ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).is_owner(at, &handle.username)
    {
        handle.send(ServerMessage::StallRefused { why: primitive_shared::stall::Refusal::NotYours });
        return None;
    }
    Some(at)
}

/// The container this player has open, if they can still reach it.
fn reachable_chest(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
) -> Option<containers::ChestPos> {
    let (at, position, dead) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.open_chest?, state.position, state.vitals.is_dead())
    };
    if dead || !chest_in_use(ctx, primitive_shared::geometry::narrow(position), at) {
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
    refresh_rack_block(ctx, at);
}

/// Puts the skin on the rack, or takes it off -- **on the block**.
///
/// A drying rack is drawn as a frame with a hide stretched in it when it
/// has one and as a bare frame when it does not (see
/// `mesh::rack_block`), and which of the two is a bit on the block id
/// (`types::RACK_LOADED`). That bit is what a *chunk* carries, so it is
/// the only part of a container's contents anybody can see from across
/// the camp -- and the whole reason a tannery reads as a tannery rather
/// than as a row of identical frames.
///
/// Called after everything that can change what is on a frame: a
/// player's gesture, and the step that turns a skin into leather. Cheap
/// enough for both -- it compares the two ids and writes nothing when
/// they agree, which is every call but the two that matter.
pub(crate) fn refresh_rack_block(ctx: &Arc<Context>, at: containers::ChestPos) {
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return;
    };
    if !primitive_shared::rack::is_rack(block) {
        return;
    }
    let contents = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at)
    };
    // **A rack of four cells says what hangs on it in its own bits**
    // (`types::rack_goods`): two a cell, four a column, an index into
    // `rack::HANGING`. The near column shows what was hung and the far one
    // what has come off dry (`rack::columns_showing`), so a player across
    // the camp can see a rack of red strips turn dark from the far end.
    //
    // A lone cell from an older save has nowhere to put four bits and keeps
    // the one it always had: `RACK_LOADED`, "there is a skin on it". A hide
    // frame says one thing more, that the skin has cured and is waiting in
    // the tray (`types::HIDE_CURED`), so a frame is drawn bare only when
    // there is nothing on it to take.
    let whole = primitive_shared::types::rack_whole(at, block, |cell| ctx.world.cached_block(cell.0, cell.1, cell.2));
    let mut changes = Vec::new();
    if whole {
        let (near, far) = primitive_shared::rack::columns_showing(&contents);
        let anchor = primitive_shared::types::rack_anchor(at, block);
        for (cell, shape) in primitive_shared::types::rack_cells(anchor, primitive_shared::types::block_facing(block)) {
            let column = if primitive_shared::types::rack_is_far(shape) { far } else { near };
            let bits = if primitive_shared::types::rack_is_top(shape) { column >> 2 } else { column & 0b11 };
            let wanted = primitive_shared::types::with_rack_goods(shape, bits);
            if ctx.world.cached_block(cell.0, cell.1, cell.2) == Some(wanted) {
                continue;
            }
            if !ctx.world.set_block(cell.0, cell.1, cell.2, wanted) {
                continue;
            }
            changes.push(BlockChange { global_x: cell.0, global_y: cell.1, global_z: cell.2, block_id: wanted });
        }
    } else {
        let raw = contents
            .block_in(primitive_shared::rack::HIDE_SLOT)
            .and_then(primitive_shared::rack::cures_into)
            .is_some();
        let cured = contents.block_in(primitive_shared::rack::LEATHER_SLOT).is_some();
        let wanted = primitive_shared::types::hide_frame_showing(block, raw, cured);
        if wanted == block {
            return;
        }
        if !ctx.world.set_block(at.0, at.1, at.2, wanted) {
            return;
        }
        changes.push(BlockChange { global_x: at.0, global_y: at.1, global_z: at.2, block_id: wanted });
    }
    if changes.is_empty() {
        return;
    }
    // Through the ordinary broadcast, so the cells are remeshed for
    // everyone who can see them -- the same path a placed block takes.
    broadcast_changes(ctx, changes);
}

/// Kills whoever is standing where the trunk is about to land.
///
/// **A tree that falls on you kills you**, and it is the one thing in
/// this game that does not take a fraction of your health first. The
/// reasons it is not damage: a five-metre trunk is not a punch, the
/// number would have to be balanced against armour that is not armour
/// against *this*, and a player who survived it at two hearts would
/// have learnt nothing about standing under a tree they are cutting.
///
/// It is avoidable, and that is what makes it fair rather than cruel:
/// which way a tree goes is decided by its lean, the room around it and
/// the ground under it (see `logic::felling`) -- all of it visible from
/// where the player is standing, all of it the same every time, and the
/// feller's own cell is the tie-break that keeps a tree with a choice
/// from picking them.
///
/// The whole player box is tested against the whole cell, rather than
/// the cell their feet are in: a trunk laid through the square next to
/// you catches you if you are leaning into it, which is what a metre of
/// wood at head height does.
fn crush_anyone_under(
    ctx: &Arc<Context>,
    laid: &[((i32, i32, i32), primitive_shared::types::BlockId)],
) {
    use primitive_shared::geometry::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};

    if laid.is_empty() {
        return;
    }
    for handle in ctx.registry.handles() {
        let outcome = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.vitals.is_dead() {
                continue;
            }
            let (px, py, pz) = state.position;
            if !px.is_finite() || !py.is_finite() || !pz.is_finite() {
                continue;
            }
            let hit = laid.iter().any(|&((cx, cy, cz), _)| {
                px + f64::from(PLAYER_HALF_WIDTH) > f64::from(cx as f32)
                    && px - f64::from(PLAYER_HALF_WIDTH) < f64::from(cx as f32 + 1.0)
                    && pz + f64::from(PLAYER_HALF_WIDTH) > f64::from(cz as f32)
                    && pz - f64::from(PLAYER_HALF_WIDTH) < f64::from(cz as f32 + 1.0)
                    && py + f64::from(PLAYER_HEIGHT) > f64::from(cy as f32)
                    && py < f64::from(cy as f32 + 1.0)
            });
            if !hit {
                continue;
            }
            // Whatever they had left. `hurt` is the one path that
            // announces a death, drops the pack and fires the hooks --
            // see `report_vitals` -- and going round it to set health to
            // zero would be a death nobody hears about.
            state.vitals.hurt(f32::MAX, "was crushed by a falling tree")
        };
        report_vitals(ctx, &handle, outcome);
    }
}

/// Empties a chest into the world, because the block holding it is gone.
///
/// One dropped stack per slot rather than one per block: forty items
/// popping out of a broken chest is fine, five thousand is a server
/// falling over. The stacks land in the cell the chest was in, which is
/// now air, so they are reachable.
/// Brings a tree down, because its base was just cut.
///
/// Does nothing unless the cell that was broken was the bottom of a
/// standing trunk, so the break path can call it unconditionally --
/// `felling::fell` is the one that decides, and it decides from the
/// world rather than from anything the client said.
///
/// **What it does is edits**, through exactly the paths a player's own
/// edit takes: the falling sand is told, the cell mechanics are told,
/// and the changes go out through the same batching every other tick's
/// worth of changes uses. A tree that came down by a private route
/// would be a tree the water and the fires never heard about.
pub(crate) fn fell_tree(
    ctx: &Arc<Context>,
    stump: (i32, i32, i32),
    feller: Option<&Arc<players::PlayerHandle>>,
) -> u32 {
    use primitive_shared::types::BLOCK_AIR;

    // Where the axe was swung from, as a cell. The last of the four
    // things that decide which way the tree goes -- see `felling`.
    let from = feller.map(|handle| {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let (px, _, pz) = state.position;
        (px.floor() as i32, pz.floor() as i32)
    });
    // **Whatever the biome.** This asked the biome whether its trees had a
    // crown and did not fell a dead forest's bare poles at all, because
    // felling one cleared seven blocks and laid three ("в мёртвом лесу
    // деревья при рубке пропадают"). The trunk vanished because a fall kept
    // two thirds of it; a fall gives back every log now, so a dead tree
    // comes down as any other does -- and left standing, it was a pole on
    // air over the cut ("вообще не падают"). See `felling`'s module note.
    let plan = felling::fell(stump, |x, y, z| ctx.world.cached_block(x, y, z), from);
    if plan.is_empty() {
        return 0;
    }
    // **Asked before anything moves**, because a veto has to leave the
    // tree standing rather than put it back up. The plan is already
    // worked out, so the count a mod is shown is the real one -- an
    // event that said "a tree is coming down, somewhere, somehow" would
    // not be enough to decide anything with.
    if !tree_felled(
        ctx,
        feller.map(|h| h.id).unwrap_or(0),
        stump,
        (plan.cleared.len() + plan.laid.len()) as u32,
    ) {
        return 0;
    }
    // **Before the world changes**, because being under a tree is a
    // fact about where the trunk is *going* rather than about what the
    // cells hold afterwards -- and because a player killed by it drops
    // their pack, which is an edit of its own.
    crush_anyone_under(ctx, &plan.laid);

    let mut changes: Vec<BlockChange> = Vec::new();
    // **A tree that comes down shakes its apples loose**, onto the ground
    // with the sticks -- what picking each by hand would have given, read
    // before the cell is cleared because afterwards it is air. Without it
    // an apple tree felled in fruit took its apples with the leaves they
    // hung in: the one food a player had walked to, deleted by the axe.
    let mut shaken: Vec<(primitive_shared::types::BlockId, u32)> = Vec::new();
    // Cleared first and laid second, and the order matters for one cell:
    // the butt of a trunk that had nowhere to go lands in the stump's
    // own hole, which is a cell the clearing pass may also have touched.
    for (x, y, z) in plan.cleared {
        let was = ctx.world.cached_block(x, y, z);
        // **A snag felled out of its pool leaves the pool**, the way cutting
        // one piece of it does (`types::block_residue`): its drowned pieces
        // are the water they stood in, and air there was a hole in the swamp.
        let left = if was.is_some_and(primitive_shared::types::stands_in_water) {
            primitive_shared::types::BLOCK_WATER
        } else {
            BLOCK_AIR
        };
        if !ctx.world.set_block(x, y, z, left) {
            continue;
        }
        if let Some((drop, fruit)) = was
            .filter(|&b| primitive_shared::types::picks_by_hand(b))
            .and_then(|b| Some((primitive_shared::types::block_drop(b)?, b)))
        {
            let count = u32::from(primitive_shared::types::block_drop_count(fruit));
            match shaken.iter_mut().find(|(kind, _)| *kind == drop) {
                Some((_, total)) => *total += count,
                None => shaken.push((drop, count)),
            }
        }
        changes.push(BlockChange {
            global_x: x,
            global_y: y,
            global_z: z,
            // What was written, not air: a snag felled out of its pool left
            // water on the server and told every client there was a hole.
            block_id: left,
        });
    }
    for ((x, y, z), block) in plan.laid {
        if !ctx.world.set_block(x, y, z, block) {
            continue;
        }
        changes.push(BlockChange {
            global_x: x,
            global_y: y,
            global_z: z,
            block_id: block,
        });
    }

    // Everything that watches cells hears about every one of them, on
    // the same terms a player's edit gets.
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        for change in &changes {
            sim.on_block_changed(change.global_x, change.global_y, change.global_z);
        }
    }
    for change in &changes {
        notify_mechanics(ctx, change.global_x, change.global_y, change.global_z);
    }
    ctx.metrics
        .block_edits
        .fetch_add(changes.len() as u64, Ordering::Relaxed);

    // Timber there was no room to lay, and whatever the crown was worth.
    // On the ground at the stump rather than in the player's pack: a
    // tree that filled your bag from across the clearing would be a tree
    // you never have to walk to.
    let at = (
        stump.0 as f32 + 0.5,
        stump.1 as f32 + 0.5,
        stump.2 as f32 + 0.5,
    );
    // ...and the twigs of a tree of branches, which are sticks already.
    let sticks = felling::sticks_from(plan.leaves) + felling::sticks_from_twigs(plan.twigs);
    if plan.spare > 0 || sticks > 0 {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        if plan.spare > 0 {
            items.spawn(
                // Off a standing tree, so green (`wood`, "seasoning").
                primitive_shared::wood::green(plan.wood),
                plan.spare,
                primitive_shared::geometry::wide(at),
                (0.0, 0.0, 0.0),
                None,
                Instant::now(),
            );
        }
        if sticks > 0 {
            items.spawn(
                primitive_shared::types::BLOCK_STICK,
                sticks,
                primitive_shared::geometry::wide(at),
                (0.0, 0.0, 0.0),
                None,
                Instant::now(),
            );
        }
    }
    if !shaken.is_empty() {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        for (drop, count) in shaken {
            items.spawn(drop, count, primitive_shared::geometry::wide(at), (0.0, 0.0, 0.0), None, Instant::now());
        }
    }

    let count = changes.len() as u32;
    broadcast_changes(ctx, changes);
    count
}

/// Brings down a palm's crown that the cell just emptied at `broken` was
/// holding up -- its heart, or the top piece of trunk under the heart -- and
/// says how many cells came down.
///
/// Beside `fell_tree` and called straight after it by every break path,
/// because it is the other half of the same picture: felling takes a crown
/// down with a trunk cut *below* the top, and this takes it down when the
/// cut is the top itself. See `felling::unheld_palm_crown` for which cells
/// that is and why.
///
/// **What comes down is what a felled crown gives**, not what breaking each
/// frond by hand would: the coconuts shaken loose onto the ground (a player
/// who broke a heart to get at the nuts must not lose them), and a stick
/// per handful of fronds by `felling::sticks_from`. A frond torn by hand is
/// fibre; twenty of them for one swing at the heart would make the heart
/// the way to farm fibre, which is a chore with one right answer.
pub(crate) fn drop_unheld_palm_crown(ctx: &Arc<Context>, broken: (i32, i32, i32)) -> u32 {
    use primitive_shared::types::{block_drop, block_drop_count, picks_by_hand, BLOCK_AIR, BLOCK_STICK};

    let falling = felling::unheld_palm_crown(broken, |x, y, z| ctx.world.cached_block(x, y, z));
    if falling.is_empty() {
        return 0;
    }
    let mut changes: Vec<BlockChange> = Vec::new();
    let mut shaken: Vec<(primitive_shared::types::BlockId, u32)> = Vec::new();
    for (x, y, z) in falling {
        let was = ctx.world.cached_block(x, y, z);
        if !ctx.world.set_block(x, y, z, BLOCK_AIR) {
            continue;
        }
        if let Some((drop, fruit)) = was.filter(|&b| picks_by_hand(b)).and_then(|b| Some((block_drop(b)?, b))) {
            let count = u32::from(block_drop_count(fruit));
            match shaken.iter_mut().find(|(kind, _)| *kind == drop) {
                Some((_, total)) => *total += count,
                None => shaken.push((drop, count)),
            }
        }
        changes.push(BlockChange {
            global_x: x,
            global_y: y,
            global_z: z,
            block_id: BLOCK_AIR,
        });
    }
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        for change in &changes {
            sim.on_block_changed(change.global_x, change.global_y, change.global_z);
        }
    }
    for change in &changes {
        notify_mechanics(ctx, change.global_x, change.global_y, change.global_z);
    }
    ctx.metrics
        .block_edits
        .fetch_add(changes.len() as u64, Ordering::Relaxed);

    // In the emptied cell, which is air now, so the drops fall out of the
    // crown onto the sand under it rather than appearing at somebody's feet.
    let at = (broken.0 as f32 + 0.5, broken.1 as f32 + 0.5, broken.2 as f32 + 0.5);
    let sticks = felling::sticks_from(changes.len() as u32);
    if sticks > 0 || !shaken.is_empty() {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        if sticks > 0 {
            items.spawn(BLOCK_STICK, sticks, primitive_shared::geometry::wide(at), (0.0, 0.0, 0.0), None, Instant::now());
        }
        for (drop, count) in shaken {
            items.spawn(drop, count, primitive_shared::geometry::wide(at), (0.0, 0.0, 0.0), None, Instant::now());
        }
    }

    let count = changes.len() as u32;
    broadcast_changes(ctx, changes);
    count
}

/// Forgets how far along a rack was, because the block is gone.
///
/// **The skins are not this function's business any more.** They are in
/// the container store, so breaking a rack spills them exactly the way
/// breaking a chest spills a chest -- see `spill_chest`, which the break
/// path calls first. What is left is the float, and a float against a
/// cell that is now air is a number that would be handed to whatever
/// gets built there next.
pub(crate) fn forget_rack(ctx: &Arc<Context>, at: drying::RackPos) {
    let mut racks = ctx.drying.lock().unwrap_or_else(|e| e.into_inner());
    racks.forget(at);
}

/// Fills a ruin's chest the first time anybody touches it, and never again.
///
/// The generator puts the chest in the terrain, and what is in it is a
/// pure function of the seed and the cell (`worldgen::ruin_chest_loot`);
/// this is where the two meet. **The seal is a world edit**: the chest's
/// own block written back over itself, so from this moment the cell is
/// one somebody has touched and the question is never asked of it again.
/// A chest emptied by a player is therefore empty for good, and a chest a
/// player built never had a seal to break. See the note in
/// `worldgen::ruins` for the two stores this was not put in, and why.
///
/// Called from every door into a chest -- opening it, breaking it (from
/// the connection *before* the cell is overwritten, because afterwards
/// the cell is edited and the chest would spill nothing), and a mod
/// looking inside -- and cheap on all of them: a cell anybody has built
/// or opened stops at the edit lookup, and a chest nowhere near a ruin at
/// one hash and a subtraction.
///
/// Under the chest lock the whole way, so two players opening the same
/// ruin in the same tick cannot both find it sealed and stock it twice.
/// The two files are saved separately, and a crash between them is the
/// one hole: the edit without the contents is a ruin chest found empty,
/// and the contents without the edit are a chest stocked once more --
/// both of them a handful of flint.
pub(crate) fn unseal_ruin_chest(ctx: &Arc<Context>, at: containers::ChestPos) {
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return;
    };
    if primitive_shared::types::block_kind(block) != primitive_shared::types::BLOCK_CHEST {
        return;
    }
    let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
    if chests.holds_anything(at) || ctx.world.is_edited(at.0, at.1, at.2) {
        return;
    }
    let Some(loot) = primitive_shared::worldgen::ruin_chest_loot(ctx.world.generator(), at) else {
        return;
    };
    ctx.world.set_block(at.0, at.1, at.2, block);
    chests.edit(at, |inventory| *inventory = loot);
}

/// How far the news of a kiln or a pile carries, in blocks.
///
/// Across a camp, not across a server: a kiln going out is news for the
/// person who built it and whoever stands beside it, and a line in every
/// chat about somebody else's pots would be the line nobody reads.
const PIT_NEWS_REACH: f32 = 48.0;

/// One tick of the pit kilns and log piles: burns them, and sends what
/// changed to whoever can see it and what happened to whoever is near.
/// How far a line about wood catching fire carries, in blocks: the pits'
/// reach, for the pits' reason -- the people who can see the smoke.
const WILDFIRE_NEWS_REACH: f32 = 24.0;

/// One tick of `logic::wildfire`: the hearths the fire map has alight and
/// where everybody is go in; block changes, news and smoke come out.
///
/// The fire map's lock is taken and let go before the wildfire's, and the
/// wildfire's is let go before anything is announced -- `notify_mechanics`
/// takes both. Two locks held at once anywhere else in this file are held
/// in that order or not at all.
pub(crate) fn step_wildfire(ctx: &Arc<Context>, players: &[(PlayerId, (f32, f32, f32))], dt: f32) {
    let (hearths, smouldering) = {
        let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
        (fires.burning_cells(), fires.smouldering_cells())
    };
    // **The fuel, passed through to the smoke** (`wildfire::SMOULDER_SMOKE`),
    // and onto the open hearths' blocks for the plume every client draws
    // (`wildfire::SMOULDERING`): written only where the bit disagrees with the
    // fire, so a fire burning one fuel all evening is two edits, not one a tick.
    for &at in &hearths {
        let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
            continue;
        };
        let wanted = primitive_shared::wildfire::with_smoulder(block, smouldering.contains(&at));
        if wanted != block && ctx.world.set_block(at.0, at.1, at.2, wanted) {
            ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
            broadcast_block(ctx, at, wanted);
        }
    }
    let feet: Vec<(f32, f32, f32)> = players.iter().map(|&(_, at)| at).collect();
    let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
    // The rafts' wind, off the same clock: see `raft::wind` on why it is a
    // function of the hour rather than a thing the server rolls.
    let wind = primitive_shared::raft::wind(ctx.clock.world_days(), weather).vector();
    let stepped = {
        let mut wildfire = ctx.wildfire.lock().unwrap_or_else(|e| e.into_inner());
        wildfire.set_weather(weather, wind);
        wildfire.set_smouldering(smouldering);
        wildfire.step(&*ctx.world, &hearths, &feet, dt, simulation::DEFAULT_TICK_BUDGET)
    };
    for change in &stepped.changes {
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        notify_mechanics(ctx, change.global_x, change.global_y, change.global_z);
    }
    if !stepped.changes.is_empty() {
        broadcast_changes(ctx, stepped.changes);
    }
    if stepped.news.is_empty() {
        return;
    }
    for handle in ctx.registry.handles() {
        let feet = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
        for &(at, text) in &stepped.news {
            let (dx, dy, dz) = (
                at.0 as f32 + 0.5 - feet.0 as f32,
                at.1 as f32 + 0.5 - feet.1 as f32,
                at.2 as f32 + 0.5 - feet.2 as f32,
            );
            if dx * dx + dy * dy + dz * dz <= WILDFIRE_NEWS_REACH * WILDFIRE_NEWS_REACH {
                handle.send(ServerMessage::Chat {
                    from: None,
                    username: "server".to_string(),
                    text: text.to_string(),
                });
            }
        }
    }
}

/// The air at a cell of water, in degrees: what stands in for how cold the
/// water is when fishing asks (`fishing::cold_factor`). The climate's number,
/// so a lake in a snowfield is slow to fish for the reason a player standing
/// at it is cold.
fn water_c_at(ctx: &Arc<Context>, at: (i32, i32, i32), weather: primitive_shared::weather::Weather) -> f32 {
    let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
    climate::Ambient::of(
        &ctx.world,
        &fires,
        (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
        ctx.clock.world_days(),
        weather,
    )
    .temperature_c
}

/// **A rod thrown.** See `logic::fishing` and `primitive_shared::fishing`
/// for the rules; this is the throw, the rolls and the refusals.
///
/// `power` is how long the player held the rod back, as
/// `fishing::cast_power` scores it. Where the float lands is worked out here
/// from the player's own transform and the same arc the client drew
/// (`fishing::cast_target`) -- **the client does not say where its float
/// went**, so a cast cannot be aimed anywhere the server did not already
/// believe the player was looking.
///
/// **The refusals are sent, and a client that did its sums shows its own
/// first.** The client walks the same arc through its own chunks and says
/// "too small" in the player's language without sending anything
/// (`cast_refusal` in the client); these English lines are for the cast it
/// could not judge -- a chunk it has not got, or a float that landed
/// somewhere its own arc did not.
fn cast_line(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, power: f32) {
    use primitive_shared::fishing;
    let (eye, look, slot, bait) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        // The rod has to be in hand. A `CastLine` from a player holding a
        // stone is a client that has drifted or one that is lying, and
        // either way it is not a cast.
        if state.inventory.block_in(slot).map(primitive_shared::types::block_kind)
            != Some(primitive_shared::types::BLOCK_FISHING_ROD)
        {
            drop(state);
            send_line(handle, None);
            return;
        }
        let eye = (
            state.position.0 as f32,
            state.position.1 as f32 + primitive_shared::geometry::EYE_HEIGHT,
            state.position.2 as f32,
        );
        // The client camera's own basis, as `throw_from_slot` uses.
        let look = (
            state.yaw.cos() * state.pitch.cos(),
            state.pitch.sin(),
            state.yaw.sin() * state.pitch.cos(),
        );
        (eye, look, slot, bait_for(&state.inventory, slot))
    };
    // **Every way out of here sends the float back as gone.** The client
    // drew a float the moment it threw, from its own copy of the arc; if
    // this side does not agree there is a cast, the picture has to be taken
    // away, or the player is left watching a float over a line nobody is
    // holding.
    let refuse = |why: Option<&str>| {
        if let Some(why) = why {
            handle.send(ServerMessage::Error(why.to_string()));
        }
        send_line(handle, None);
    };
    let Some(at) = fishing::cast_target(eye, look, power, |x, y, z| ctx.world.cached_block(x, y, z)) else {
        refuse(Some("the line came down short of the water"));
        return;
    };
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        refuse(None);
        return;
    };
    let Some(spot) = fishing::survey(|x, y, z| ctx.world.cached_block(x, y, z), at) else {
        refuse(None);
        return;
    };
    let kind = water_kind(ctx, at, block);
    let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
    let water_c = water_c_at(ctx, at, weather);
    let season = primitive_shared::season::Season::at(ctx.clock.world_days());
    let now = Instant::now();
    let mut fishing_state = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner());
    let taken = fishing_state.pressure_at(at, now);
    let Some(mean) = fishing::rod_wait_seconds(
        spot,
        kind,
        ctx.clock.time_of_day(),
        weather,
        water_c,
        season,
        taken,
        bait.map(|(_, bait)| bait),
    ) else {
        let why = if spot.holds_fish() {
            "too shallow: the float would lie on the bottom"
        } else {
            "no fish live in water this small"
        };
        drop(fishing_state);
        refuse(Some(why));
        return;
    };
    let bite_in = fishing_state.roll_wait(mean);
    let liveliness = liveliness_of(mean);
    fishing_state.cast(
        handle.id,
        crate::logic::fishing::Cast {
            float: at,
            slot,
            bite_in,
            mean,
            bait,
            hooked: None,
            phase: crate::logic::fishing::Phase::Settling(fishing::SETTLE_SECONDS),
            reeling: false,
            liveliness,
            joined_at: handle.joined_at,
        },
    );
    let cast = fishing_state.cast_of(handle.id);
    drop(fishing_state);
    // **The throw, for everybody watching**: the rod they saw drawn back
    // (`Gesture::digging`, while the client wound up) whips forward. Only a
    // cast that went in: a refused one never reached the water, and the
    // thrower was told why in words.
    handle
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .gesture
        .made(primitive_shared::protocol::Action::Cast);
    if let Some(cast) = cast {
        send_line(handle, Some(&cast));
    }
}

/// **How good this water looks, as a byte**, for the float to twitch by.
///
/// The client has no numbers on the screen and is not given one: what it
/// gets is how often to make the float shiver, and a player reads "this is
/// a good spot" off a busy float exactly the way they read it off a real
/// one. Half a minute for a bite is a lively spot (255); three minutes is a
/// dead one (0).
fn liveliness_of(mean_seconds: f32) -> u8 {
    let good = (180.0 - mean_seconds) / 150.0;
    (good.clamp(0.0, 1.0) * 255.0) as u8
}

/// **Which bait goes on the hook**, and the slot it will be spent from.
///
/// The bait nearest the rod in the hotbar, counting outwards from the rod's
/// own slot. **A rule rather than a screen**: putting the worms beside the
/// rod and the meat at the far end is how a player says "fish for what the
/// worms catch", and it is done by dragging one stack, in the pack they
/// already have open, with nothing new drawn anywhere. The alternatives
/// weighed were a bait slot on the inventory screen (a screen, for one
/// item), the best bait for the water (which takes the decision away, and
/// the decision is the feature) and asking every cast (a dialogue in the
/// middle of a throw).
fn bait_for(
    inventory: &primitive_shared::inventory::Inventory,
    rod_slot: usize,
) -> Option<(usize, primitive_shared::fishing::Bait)> {
    let hotbar = primitive_shared::inventory::HOTBAR_SLOTS;
    let mut order: Vec<usize> = (0..hotbar).collect();
    order.sort_by_key(|&slot| slot.abs_diff(rod_slot));
    order.into_iter().find_map(|slot| {
        let block = inventory.block_in(slot)?;
        Some((slot, primitive_shared::fishing::Bait::of_block(block)?))
    })
}

/// What the line is doing, to the hand holding it.
fn send_line(handle: &Arc<players::PlayerHandle>, cast: Option<&crate::logic::fishing::Cast>) {
    let (float, phase, strain, liveliness) = match cast {
        Some(cast) => {
            let strain = match cast.phase {
                crate::logic::fishing::Phase::Fighting(fight) => (fight.strain.clamp(0.0, 1.0) * 255.0) as u8,
                _ => 0,
            };
            (Some(cast.float), cast.phase.code(), strain, cast.liveliness)
        }
        None => (None, 0, 0, 0),
    };
    handle.send(ServerMessage::Line {
        float,
        phase,
        strain,
        liveliness,
    });
}

/// The fish out of a trap and into the pack, and the trap left set.
///
/// Whatever does not fit falls at the player's feet, as the resin off a
/// trunk does: the trap is emptied either way, because a trap that stayed
/// full for want of room in a pack would stop fishing for a reason the
/// player standing at it cannot see.
fn empty_trap(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    use primitive_shared::types::{trap_catch, BLOCK_FISH_TRAP};
    let caught = trap_catch(block);
    if caught == 0 {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::TrapEmpty });
        return;
    }
    if !ctx.world.set_block(at.0, at.1, at.2, BLOCK_FISH_TRAP) {
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, BLOCK_FISH_TRAP);
    land_fish(ctx, handle, u32::from(caught));
}

/// Takes away every pit cover somebody heavy is standing on, if there is a
/// hole under it (`pitfall::gives_way`), and says so to everybody.
///
/// **The cell the feet are in, read a hair below them**: a cover is an
/// eighth of a block deep, so a deer standing on one has its feet an eighth
/// above the cell's floor, inside the cover's cell. Only that cell -- a body
/// is not asked about the four it overlaps, because a trap that caught
/// whatever brushed its edge would be a trap nobody could walk round.
pub(crate) fn collapse_pit_covers(ctx: &Arc<Context>, feet: impl Iterator<Item = (f32, f32, f32)>) {
    let mut written = Vec::new();
    for at in feet {
        let (x, y, z) = (at.0.floor() as i32, (at.1 - 0.05).floor() as i32, at.2.floor() as i32);
        let (Some(cover), Some(below)) = (ctx.world.cached_block(x, y, z), ctx.world.cached_block(x, y - 1, z)) else {
            continue;
        };
        if !primitive_shared::pitfall::gives_way(cover, below) {
            continue;
        }
        if !ctx.world.set_block(x, y, z, primitive_shared::types::BLOCK_AIR) {
            continue;
        }
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        notify_mechanics(ctx, x, y, z);
        written.push(primitive_shared::protocol::BlockChange { global_x: x, global_y: y, global_z: z, block_id: primitive_shared::types::BLOCK_AIR });
    }
    if !written.is_empty() {
        broadcast_changes(ctx, written);
    }
}

/// A hand at a snare: the hare laid on the ground where it hung and the
/// snare into the pack, or a robbed snare set again (`snare`).
///
/// **The hare is a carcass in the snare's own cell**, the one a speared hare
/// would have left, and butchered there with a knife: the snare took the
/// chase out of the hunt, not the knife. The snare goes into the pack rather
/// than staying set, because the place it caught in is the place a player
/// may now want to move it from -- and setting it again is one placement.
pub(crate) fn tend_snare(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    use primitive_shared::types::{block_kind, BLOCK_SNARE, BLOCK_SNARE_CAUGHT, BLOCK_SNARE_SPRUNG};
    let now = match block_kind(block) {
        BLOCK_SNARE_CAUGHT => primitive_shared::animals::carcass_at_stage(primitive_shared::animals::Species::Hare, 0),
        BLOCK_SNARE_SPRUNG => BLOCK_SNARE,
        _ => {
            handle.send(ServerMessage::Error("nothing has come to the snare yet".to_string()));
            return;
        }
    };
    if !ctx.world.set_block(at.0, at.1, at.2, now) {
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, now);
    if block_kind(block) == BLOCK_SNARE_CAUGHT {
        land_catch(ctx, handle, BLOCK_SNARE, 1);
    }
}

/// A hand at a salt pan: a jug of the sea poured into an empty one, or the
/// crust scraped out of a dry one (`saltpan`).
///
/// A jug of river water is refused and said so, as the boiling row refuses
/// it: fresh water dries to nothing, and a pan that took it would be a day
/// of waiting for a player to find that out.
pub(crate) fn tend_pan(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    use primitive_shared::types::{
        block_kind, vessel_water, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_SALT, BLOCK_SALT_PAN, BLOCK_SALT_PAN_SALT,
    };
    match block_kind(block) {
        BLOCK_SALT_PAN_SALT => {
            if !ctx.world.set_block(at.0, at.1, at.2, BLOCK_SALT_PAN) {
                return;
            }
            ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
            notify_mechanics(ctx, at.0, at.1, at.2);
            broadcast_block(ctx, at, BLOCK_SALT_PAN);
            land_catch(ctx, handle, BLOCK_SALT, primitive_shared::saltpan::YIELD);
        }
        BLOCK_SALT_PAN => {
            let (slot, held) = {
                let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                (state.selected_slot, state.inventory.block_in(state.selected_slot))
            };
            let Some(jug) = held.filter(|&b| block_kind(b) == BLOCK_JUG_WATER) else {
                handle.send(ServerMessage::Error("a salt pan is filled from a jug of the sea".to_string()));
                return;
            };
            if vessel_water(jug) != primitive_shared::body::Water::Salt {
                handle.send(ServerMessage::Error("that is fresh water: it dries to nothing".to_string()));
                return;
            }
            let brine = primitive_shared::saltpan::brine(0);
            if !ctx.world.set_block(at.0, at.1, at.2, brine) {
                return;
            }
            ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
            notify_mechanics(ctx, at.0, at.1, at.2);
            broadcast_block(ctx, at, brine);
            let (spare, left, feet) = {
                let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
                state.inventory.take_from(slot, 1);
                // Into the slot the full one left, if it is empty now; a
                // stack of full jugs keeps its slot and the empty one goes
                // wherever there is room.
                let spare = if state.inventory.block_in(slot).is_none() {
                    state.inventory.put_in_slot(slot, primitive_shared::inventory::Stack::new(BLOCK_JUG, 1)).map_or(0, |s| s.count)
                } else {
                    state.inventory.add(BLOCK_JUG, 1)
                };
                state.inventory_dirty = true;
                (spare, state.inventory.block_in(slot), state.position)
            };
            if spare > 0 {
                ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
                    BLOCK_JUG,
                    spare,
                    (feet.0, feet.1 + 0.5, feet.2),
                    (0.0, 0.0, 0.0),
                    None,
                    Instant::now(),
                );
            }
            held_slot_changed(ctx, handle.id, slot, left);
            send_inventory(handle);
            refresh_carried_weight(handle);
        }
        _ => {
            handle.send(ServerMessage::Error("the pan is still drying".to_string()));
        }
    }
}

/// `count` raw fish into a player's pack, and what will not fit at their
/// feet. What a trap ends in; a rod lands a *species*, whose drops are
/// whatever that fish is worth -- see `land_catch`.
fn land_fish(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, count: u32) {
    use primitive_shared::types::BLOCK_RAW_FISH;
    let (spare, feet) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let spare = state.inventory.add(BLOCK_RAW_FISH, count);
        state.inventory_dirty = true;
        (spare, state.position)
    };
    if spare > 0 {
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            BLOCK_RAW_FISH,
            spare,
            (feet.0, (feet.1 + 0.5), feet.2),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// Whatever a fish is worth, into the pack, and the rest at the player's
/// feet. `land_fish`'s body for any block, because a rod lands
/// `Species::drops` now and a species may one day drop something that is not
/// a fish.
fn land_catch(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    block: primitive_shared::types::BlockId,
    count: u32,
) {
    let (spare, feet) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let spare = state.inventory.add(block, count);
        state.inventory_dirty = true;
        (spare, state.position)
    };
    if spare > 0 {
        ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
            block,
            spare,
            (feet.0, (feet.1 + 0.5), feet.2),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );
    }
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// One step of the rot clock over every trap: `rot::Rot::pass` calls it.
///
/// **Two locks, in the rot pass's order**: the fire map (for the air over
/// the water) and then the traps. The blocks are written with both let go,
/// because `notify_mechanics` takes the traps' lock again.
pub(crate) fn fill_traps(ctx: &Arc<Context>, world_time: f32, weather: primitive_shared::weather::Weather) {
    use logic::fishing::Setting;
    use primitive_shared::{fishing, saltpan, snare};
    // Where everybody is, read before the locks below: a hare keeps off a
    // person (`snare::KEEPS_OFF`), and no player's state is held under the
    // traps' lock.
    let people: Vec<(f32, f32, f32)> = ctx
        .registry
        .handles()
        .iter()
        .map(|h| primitive_shared::geometry::narrow(h.state.lock().unwrap_or_else(|e| e.into_inner()).position))
        .collect();
    let changes = {
        let fires = ctx.fires.lock().unwrap_or_else(|e| e.into_inner());
        let mut traps = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner());
        traps.step_traps(|at| {
            let block = ctx.world.cached_block(at.0, at.1, at.2)?;
            let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
            // **A snare**: the cover round it, the snares beside it, and
            // whether anybody is near enough for a hare to smell.
            if snare::is_snare(block) {
                let (mut cover, mut cells, mut neighbours) = (0u32, 0u32, 0u32);
                let reach = snare::COVER_REACH.max(snare::SHARED_RUN);
                for dx in -reach..=reach {
                    for dz in -reach..=reach {
                        if (dx, dz) == (0, 0) {
                            continue;
                        }
                        let (x, z) = (at.0 + dx, at.2 + dz);
                        if dx.abs() <= snare::COVER_REACH && dz.abs() <= snare::COVER_REACH {
                            cells += 1;
                            if (at.1..=at.1 + 1).any(|y| ctx.world.cached_block(x, y, z).is_some_and(snare::is_cover)) {
                                cover += 1;
                            }
                        }
                        if (at.1 - 1..=at.1 + 1).any(|y| ctx.world.cached_block(x, y, z).is_some_and(snare::is_snare)) {
                            neighbours += 1;
                        }
                    }
                }
                let near = people.iter().any(|p| {
                    (p.0 - centre.0).hypot(p.2 - centre.2) < snare::KEEPS_OFF && (p.1 - centre.1).abs() < snare::KEEPS_OFF
                });
                let chance = snare::catch_chance(cover as f32 / cells.max(1) as f32, near, neighbours);
                return Some((block, Setting::Ground(chance)));
            }
            // **A salt pan**: the rain on it, a roof over it, the air.
            if saltpan::is_pan(block) {
                let air = climate::Ambient::of(&ctx.world, &fires, centre, world_time, weather);
                return Some((
                    block,
                    Setting::Sky { rained_on: air.getting_wet, roofed: air.sheltered, air_c: air.temperature_c },
                ));
            }
            let chance = match fishing::trap_water(|x, y, z| ctx.world.cached_block(x, y, z), at) {
                None => 0.0,
                Some((spot, side)) => {
                    let water = ctx
                        .world
                        .cached_block(side.0, side.1, side.2)
                        .unwrap_or(primitive_shared::types::BLOCK_WATER);
                    let air = climate::Ambient::of(
                        &ctx.world,
                        &fires,
                        (side.0 as f32 + 0.5, side.1 as f32, side.2 as f32 + 0.5),
                        world_time,
                        weather,
                    )
                    .temperature_c;
                    fishing::trap_chance(spot, water_kind(ctx, side, water), air)
                }
            };
            Some((block, Setting::Water(chance)))
        })
    };
    let mut written = Vec::with_capacity(changes.len());
    for change in changes {
        if !ctx.world.set_block(change.global_x, change.global_y, change.global_z, change.block_id) {
            continue;
        }
        notify_mechanics(ctx, change.global_x, change.global_y, change.global_z);
        written.push(change);
    }
    if !written.is_empty() {
        broadcast_changes(ctx, written);
    }
}

/// The lines in the water, once a tick: the ones let go of are gone, the
/// ones a fish is nibbling go under, and the ones that were fought up come
/// into the pack.
///
/// **What "let go of" is**, asked of every line (`logic::fishing`): the
/// fisher still connected -- the same connection, not the next one handed the
/// same number -- alive, with the rod still in the slot it was cast from and
/// that slot still in hand, within `fishing::CAST_HOLDS` of the float, and
/// the float still on water. The client asks the same of its own float.
///
/// **No player's state is held under the traps' lock.** Every edit path
/// ends in `notify_mechanics`, which takes the traps' lock, and some of them
/// get there holding a player; so what each fisher is doing is read first,
/// with the traps let go, and the lines are stepped against that copy.
pub(crate) fn step_fishing(ctx: &Arc<Context>, dt: f32) {
    use crate::logic::fishing::CastEvent;
    use primitive_shared::types::{block_kind, is_liquid, BLOCK_FISHING_ROD};
    let fishers = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner()).fishers();
    if fishers.is_empty() {
        return;
    }
    // Player -> (connection began, eye, slot in hand holding a rod, alive).
    let mut seen = std::collections::HashMap::new();
    for player in fishers {
        let Some(handle) = ctx.registry.get(player) else { continue };
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        let rod = state.inventory.block_in(slot).map(block_kind) == Some(BLOCK_FISHING_ROD);
        let eye = (state.position.0, state.position.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT), state.position.2);
        seen.insert(player, (handle.joined_at, eye, rod.then_some(slot), !state.vitals.is_dead()));
    }
    // What the water holds, asked once per cast that needs it and never
    // under the fishing lock: `rod_species` walks the species table and the
    // biome, and the lock is held while every line in the world is stepped.
    let events = {
        let mut fishing = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner());
        fishing.step_casts(
            dt,
            |player, cast| {
                let Some(&(joined_at, eye, rod_slot, alive)) = seen.get(&player) else {
                    return false;
                };
                joined_at == cast.joined_at
                    && alive
                    && rod_slot == Some(cast.slot)
                    && primitive_shared::fishing::cast_holds(primitive_shared::geometry::narrow(eye), cast.float)
                    && ctx.world.cached_block(cast.float.0, cast.float.1, cast.float.2).is_some_and(is_liquid)
            },
            |_, cast, roll| {
                let spot = primitive_shared::fishing::survey(|x, y, z| ctx.world.cached_block(x, y, z), cast.float)?;
                let biome = ctx.world.biome_at(cast.float.0, cast.float.2);
                primitive_shared::fishing::rod_species(biome, spot, cast.bait.map(|(_, bait)| bait), roll)
            },
        )
    };
    for (player, cast, event) in events {
        let Some(handle) = ctx.registry.get(player) else { continue };
        match event {
            // A fish has the bait: the float goes under, and the player has
            // `fishing::STRIKE_SECONDS` to say so.
            CastEvent::Dipped | CastEvent::Straining => send_line(&handle, Some(&cast)),
            // The window closed: something ate the bait and went. The line
            // fishes on, barer than it was.
            CastEvent::Missed => {
                spend_bait(ctx, &handle, &cast, false);
                send_line(&handle, Some(&cast));
            }
            CastEvent::Landed(species) => {
                for &(block, count) in species.drops() {
                    land_catch(ctx, &handle, block, count);
                }
                // ...and what it was, for the client to say in the player's
                // own words. See `ServerMessage::Caught`.
                handle.send(ServerMessage::Caught { species });
                // The bait went with the fish, and the spot is that much
                // more fished out.
                spend_bait(ctx, &handle, &cast, false);
                ctx.fishing
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .fish_taken(cast.float, Instant::now());
                wear_rod(ctx, &handle, cast.slot);
                send_line(&handle, None);
            }
            // **The line parted.** The fish, the bait and (if it was a fly)
            // the fly go with it -- see `fishing::Bait::keeps`.
            //
            // Nothing is said from here. The client knows a line that was
            // fighting and is now out of the water with no fish in the pack,
            // and it says so in the player's own language
            // (`Msg::FishingLineGone`); an English sentence from the server
            // as well would be the same news twice, in the wrong language
            // second.
            CastEvent::Lost => {
                spend_bait(ctx, &handle, &cast, true);
                wear_rod(ctx, &handle, cast.slot);
                send_line(&handle, None);
            }
            CastEvent::Ended => send_line(&handle, None),
        }
    }
}

/// One of whatever the bait was, out of the pack it was in.
///
/// **A fly is not eaten** (`fishing::Bait::keeps`), so it is taken only when
/// the line parts. Taken by *kind* from the whole pack rather than from the
/// slot it was found in, because a stack that ran out and was topped up from
/// the pack is the same bait to the player.
fn spend_bait(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    cast: &crate::logic::fishing::Cast,
    line_parted: bool,
) {
    let Some((_, bait)) = cast.bait else { return };
    // **The fly is the exception, and the broken line is the exception to
    // the exception.** A worm is eaten whether the fish is landed or lost;
    // a fly is taken apart by neither, and only goes when it goes into the
    // lake on the end of a parted line. See `fishing::Bait::keeps`.
    if bait.keeps() && !line_parted {
        return;
    }
    let took = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let took = state.inventory.take_one(bait.block());
        state.inventory_dirty |= took;
        took
    };
    if took {
        send_inventory(handle);
        refresh_carried_weight(handle);
    }
    let _ = ctx;
}

/// **A fish landed is what wears a rod**, and so is a line that parted: a
/// cast that came to nothing has cost the line nothing.
fn wear_rod(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, slot: usize) {
    let wear = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let wear = state.inventory.wear_tool(slot);
        state.inventory_dirty |= !matches!(wear, primitive_shared::inventory::Wear::None);
        wear
    };
    match wear {
        primitive_shared::inventory::Wear::None => {}
        primitive_shared::inventory::Wear::Worn => send_inventory(handle),
        primitive_shared::inventory::Wear::Broke => {
            send_inventory(handle);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::LineParted });
        }
    }
    let _ = ctx;
}

/// **The strike**, from the hand. See `fishing::Strike` for the three
/// answers and `logic::fishing::Fishing::strike` for which one this is.
pub(crate) fn strike_line(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let (answer, cast) = {
        let mut fishing = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner());
        let answer = fishing.strike(handle.id);
        (answer, fishing.cast_of(handle.id))
    };
    match answer {
        primitive_shared::fishing::Strike::Hooked => send_line(handle, cast.as_ref()),
        // **The server refusing an impossible strike is the whole of the
        // timing rule.** A client that struck early loses the cast here,
        // whatever it drew -- and is told only by the float going, because
        // an honest client has already said "you struck at nothing" in the
        // player's own language (`Msg::FishingStruckAtNothing`).
        primitive_shared::fishing::Strike::TooEarly => send_line(handle, cast.as_ref()),
        primitive_shared::fishing::Strike::NotFishing => {}
    }
}

/// The hand pulling, or giving line.
pub(crate) fn reel_line(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, pulling: bool) {
    ctx.fishing.lock().unwrap_or_else(|e| e.into_inner()).reel(handle.id, pulling);
}

/// The line out of the water, because the player asked.
pub(crate) fn reel_in_line(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let had = ctx.fishing.lock().unwrap_or_else(|e| e.into_inner()).reel_in(handle.id);
    if had.is_some() {
        send_line(handle, None);
    }
}

pub(crate) fn step_pits(ctx: &Arc<Context>, dt: f64) {
    let stepped = {
        let mut pits = ctx.pits.lock().unwrap_or_else(|e| e.into_inner());
        pits.step(&*ctx.world, dt, simulation::DEFAULT_TICK_BUDGET)
    };
    // With the pits' lock let go: `notify_mechanics` takes it.
    for change in &stepped.changes {
        notify_mechanics(ctx, change.global_x, change.global_y, change.global_z);
    }
    let cells: Vec<(i32, i32, i32)> = stepped
        .changes
        .iter()
        .map(|change| (change.global_x, change.global_y, change.global_z))
        .collect();
    if !stepped.changes.is_empty() {
        broadcast_changes(ctx, stepped.changes);
    }
    // A kiln that has fired its pottery holds fired pieces now.
    for at in cells {
        tell_pit_pottery(ctx, at);
    }
    if stepped.news.is_empty() {
        return;
    }
    for handle in ctx.registry.handles() {
        let feet = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
        for &(at, text) in &stepped.news {
            let (dx, dy, dz) = (
                at.0 as f32 + 0.5 - feet.0 as f32,
                at.1 as f32 + 0.5 - feet.1 as f32,
                at.2 as f32 + 0.5 - feet.2 as f32,
            );
            if dx * dx + dy * dy + dz * dz <= PIT_NEWS_REACH * PIT_NEWS_REACH {
                handle.send(ServerMessage::Chat {
                    from: None,
                    username: "server".to_string(),
                    text: text.to_string(),
                });
            }
        }
    }
}

/// Does what a gesture at a pit answered: the pack, the world, the words.
///
/// **A refusal is a banner and news is a chat line.** A gesture that
/// changed nothing -- seven logs, a side open, rain -- is the player being
/// told no, which the banner is for; one that did something and has
/// something to say about it goes in the chat, where "in an hour the
/// pottery is fired" can be read again in fifty minutes.
pub(crate) fn apply_pit_outcome(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    outcome: crate::logic::pits::Outcome,
) {
    let changed = outcome.spent.is_some() || outcome.returned.is_some() || !outcome.wrote.is_empty();
    if outcome.spent.is_some() || outcome.returned.is_some() {
        let (slot, left, spill) = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            let slot = state.selected_slot;
            if outcome.spent.is_some() {
                state.inventory.take_from(slot, 1);
            }
            let spill = outcome.returned.map_or(0, |piece| state.inventory.add(piece, 1));
            state.inventory_dirty = true;
            (slot, state.inventory.block_in(slot), spill)
        };
        // A full pack leaves the pot at the player's feet rather than in the
        // pit it was just taken out of: the pit has already let it go.
        if let (Some(piece), true) = (outcome.returned, spill > 0) {
            let feet = handle.state.lock().unwrap_or_else(|e| e.into_inner()).position;
            ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
                piece,
                spill,
                (feet.0, (feet.1 + 0.5), feet.2),
                (0.0, 0.0, 0.0),
                None,
                Instant::now(),
            );
        }
        held_slot_changed(ctx, handle.id, slot, left);
        send_inventory(handle);
        refresh_carried_weight(handle);
    }
    for (at, block) in outcome.wrote {
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        notify_mechanics(ctx, at.0, at.1, at.2);
        broadcast_block(ctx, at, block);
        // After the block, so a client meets the pit before it meets what
        // is in it -- see `ServerMessage::PitPottery`.
        tell_pit_pottery(ctx, at);
    }
    if let Some(said) = outcome.said {
        if changed {
            handle.send(ServerMessage::Chat {
                from: None,
                username: "server".to_string(),
                text: said,
            });
        } else {
            handle.send(ServerMessage::Error(said));
        }
    }
}

/// `ClientMessage::PileLog`: lays the log in the hand as a pile at `at`.
pub(crate) fn pile_log(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    let (feet, held, dead) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.position, state.inventory.block_in(state.selected_slot), state.vitals.is_dead())
    };
    if dead {
        return;
    }
    // The use gesture's reach, for the use gesture's reason.
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    if !primitive_shared::combat::within_reach(
        (feet.0, feet.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT), feet.2),
        primitive_shared::geometry::wide(centre),
        None,
    ) {
        return;
    }
    let outcome = ctx
        .pits
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .lay_pile(&*ctx.world, at, held);
    apply_pit_outcome(ctx, handle, outcome);
}

/// `ClientMessage::SetDown`: lays one of what is in the hand in the empty
/// cell `at`, as a `BLOCK_SET_DOWN` holding it.
///
/// **Every refusal before anything is spent**, the placement's rule: the
/// thing has to be one that is set down (`types::can_be_set_down`), the cell
/// has to be air -- not water, which is where a knife is lost rather than
/// laid, and not a tuft of grass, which a set-down knife would otherwise
/// delete -- and the cell under it has to be a whole floor. What a mod's
/// protection says is asked as it is for a block put down, because a claim
/// that stopped building and let a player strew a stranger's house with
/// bones would not be a claim.
///
/// One thing, whatever the stack: a player laying out a table lays it out,
/// and a stack of forty arrows set down as one is a chest with no lid.
pub(crate) fn set_down_item(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    use primitive_shared::types::{can_be_set_down, faced, has_full_top, is_air, Facing, BLOCK_SET_DOWN};
    let (feet, yaw, slot, held, dead) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let slot = state.selected_slot;
        (
            state.position,
            state.yaw,
            slot,
            state.inventory.slots().get(slot).copied().flatten(),
            state.vitals.is_dead(),
        )
    };
    if dead {
        return;
    }
    let Some(held) = held.filter(|stack| can_be_set_down(stack.block)) else {
        return;
    };
    // The use gesture's reach, for the use gesture's reason.
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    if !primitive_shared::combat::within_reach(
        (feet.0, feet.1 + f64::from(primitive_shared::geometry::EYE_HEIGHT), feet.2),
        primitive_shared::geometry::wide(centre),
        None,
    ) {
        return;
    }
    let empty = ctx.world.cached_block(at.0, at.1, at.2).is_some_and(is_air);
    let floor = ctx.world.cached_block(at.0, at.1 - 1, at.2).is_some_and(has_full_top);
    if !empty || !floor {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::SetDownOnSolidGround });
        return;
    }
    let block = faced(BLOCK_SET_DOWN, Facing::toward_viewer(yaw));
    let hook_args = vec![
        plugins::Value::Int(handle.id as i64),
        plugins::Value::Int(at.0 as i64),
        plugins::Value::Int(at.1 as i64),
        plugins::Value::Int(at.2 as i64),
        plugins::Value::Int(block as i64),
    ];
    if !fire_plugin_hook(ctx, "on_block_place", hook_args, Some(vec![at])) {
        handle.send(ServerMessage::Error("a plugin refused that change".to_string()));
        return;
    }
    // Spent, then written, then filled: a cell that refused the write gives
    // the thing back, and a store is never filled for a cell with no block.
    let spent = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let still = state.inventory.slots().get(slot).copied().flatten();
        let same = still.is_some_and(|stack| stack.block == held.block && stack.damage == held.damage);
        let taken = same && state.selected_slot == slot && state.inventory.take_from(slot, 1) == 1;
        if taken {
            state.inventory_dirty = true;
        }
        taken
    };
    if !spent {
        return;
    }
    if !ctx.world.set_block(at.0, at.1, at.2, block) {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inventory.add_worn(held.block, 1, held.damage);
        state.inventory_dirty = true;
        drop(state);
        send_inventory(handle);
        return;
    }
    // Whatever a store at this cell still held is somebody's: spilled, as
    // `set_down_vessel` spills it, and never read as this thing's.
    let orphan = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        let orphan = chests.take(at);
        chests.edit(at, |store| {
            store.put_in_slot(0, primitive_shared::inventory::Stack::worn(held.block, 1, held.damage))
        });
        orphan
    };
    if let Some(orphan) = orphan {
        spill_inventory(ctx, &orphan, (centre.0, centre.1 + 0.5, centre.2));
    }
    // A sod of peat laid out starts drying; anything else laid here takes the
    // cell off the drying list, whatever lay in it before (`Peat::lay`).
    ctx.peat.lock().unwrap_or_else(|e| e.into_inner()).lay(at, held.block);
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.last_edit = Some(Instant::now());
        state.gesture.made(primitive_shared::protocol::Action::Place);
    }
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, block);
    // After the block, so no client meets the knife before the cell it is in.
    tell_set_down(ctx, at);
    let left = handle.state.lock().unwrap_or_else(|e| e.into_inner()).inventory.block_in(slot);
    held_slot_changed(ctx, handle.id, slot, left);
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// A hand taking back what it set down: `UseBlock` at a `BLOCK_SET_DOWN`.
///
/// **Into the pack, and at the feet when there is no room**, the pit's rule
/// for a pot taken out of it: the cell has already let it go, and a refusal
/// that left it lying would be a knife that could only be broken loose.
/// Everything in the store comes out, not only the first slot -- a store
/// only ever holds one thing, and a cell that somehow held more must not
/// keep the rest when the block goes.
fn pick_up_set_down(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    use primitive_shared::types::BLOCK_AIR;
    close_chest_for_everyone(ctx, at);
    if !ctx.world.set_block(at.0, at.1, at.2, BLOCK_AIR) {
        return;
    }
    let stored = ctx.chests.lock().unwrap_or_else(|e| e.into_inner()).take(at);
    let (feet, slot, spill) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut spill = Vec::new();
        for stack in stored.iter().flat_map(|stored| stored.slots().iter().flatten()) {
            let left = state.inventory.add_worn(stack.block, stack.count, stack.damage);
            if left > 0 {
                spill.push(primitive_shared::inventory::Stack::worn(stack.block, left, stack.damage));
            }
        }
        state.inventory_dirty = true;
        state.last_edit = Some(Instant::now());
        (state.position, state.selected_slot, spill)
    };
    if !spill.is_empty() {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        for stack in spill {
            items.spawn_worn(
                stack.block,
                stack.count,
                stack.damage,
                (feet.0, (feet.1 + 0.5), feet.2),
                (0.0, 0.0, 0.0),
                None,
                Instant::now(),
            );
        }
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    ctx.falling.lock().unwrap_or_else(|e| e.into_inner()).on_block_changed(at.0, at.1, at.2);
    notify_mechanics(ctx, at.0, at.1, at.2);
    broadcast_block(ctx, at, BLOCK_AIR);
    let left = handle.state.lock().unwrap_or_else(|e| e.into_inner()).inventory.block_in(slot);
    held_slot_changed(ctx, handle.id, slot, left);
    send_inventory(handle);
    refresh_carried_weight(handle);
}

/// Flint struck at the ground: a firepit, if three sticks and a log lie on
/// it (`primitive_shared::pit::FIREPIT_STICKS`).
///
/// **TerraFirmaCraft's firepit, without its dice.** Its sticks and log are
/// thrown on the ground and struck, and so are these -- dropped from the
/// pack, lying in the cell over the block the player aims at. It never
/// fails: a fire that catches at random is a chore of re-striking, not a
/// decision. A strike at bare ground says nothing, because that is a
/// player holding flint and clicking at a field; a strike at a cell with
/// *some* of the makings says what is missing.
pub(crate) fn strike_firepit(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    use primitive_shared::pit::{is_log, FIREPIT_LOGS, FIREPIT_STICKS};
    use primitive_shared::types::{block_kind, has_full_top, is_air, BLOCK_FIREPIT_LIT, BLOCK_STICK};
    let cell = (at.0, at.1 + 1, at.2);
    let floor = ctx.world.cached_block(at.0, at.1, at.2).is_some_and(has_full_top);
    let empty = ctx.world.cached_block(cell.0, cell.1, cell.2).is_some_and(is_air);
    if !floor || !empty {
        return;
    }
    // **Only dry wood catches** (`wet`): a firepit laid with sticks carried
    // through a river is laid, and it will not light until they have dried.
    // Counted apart so the refusal can say which it is -- "not enough" and
    // "wet" are two different things to go and fix.
    let dry = |block| !primitive_shared::wet::will_not_light(block);
    let is_stick = |block| block_kind(block) == BLOCK_STICK && dry(block);
    let is_dry_log = |block| is_log(block) && dry(block);
    {
        let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
        let sticks = items.count_lying_in(cell, is_stick);
        let logs = items.count_lying_in(cell, is_dry_log);
        let wet = items.count_lying_in(cell, |block| {
            (block_kind(block) == BLOCK_STICK || is_log(block)) && !dry(block)
        });
        if sticks == 0 && logs == 0 && wet == 0 {
            return;
        }
        if wet > 0 && (sticks < FIREPIT_STICKS || logs < FIREPIT_LOGS) {
            drop(items);
            handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::WoodWet });
            return;
        }
        if sticks < FIREPIT_STICKS || logs < FIREPIT_LOGS {
            drop(items);
            handle.send(ServerMessage::Error(format!(
                "a firepit is {FIREPIT_STICKS} sticks and {FIREPIT_LOGS} log lying on the ground ({sticks} sticks and {logs} logs here)"
            )));
            return;
        }
        items.take_lying_in(cell, is_stick, FIREPIT_STICKS);
        items.take_lying_in(cell, is_dry_log, FIREPIT_LOGS);
    }
    if !ctx.fires.lock().unwrap_or_else(|e| e.into_inner()).light(cell) {
        return;
    }
    if !ctx.world.set_block(cell.0, cell.1, cell.2, BLOCK_FIREPIT_LIT) {
        ctx.fires.lock().unwrap_or_else(|e| e.into_inner()).extinguish(cell);
        return;
    }
    ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
    spend_striker(ctx, handle);
    notify_mechanics(ctx, cell.0, cell.1, cell.2);
    broadcast_block(ctx, cell, BLOCK_FIREPIT_LIT);
}

/// A pit kiln or a log pile broken: what went into it comes out.
pub(crate) fn spill_pit(ctx: &Arc<Context>, at: (i32, i32, i32), broken: primitive_shared::types::BlockId) {
    let out = ctx.pits.lock().unwrap_or_else(|e| e.into_inner()).broken(at, broken);
    let centre = (at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
    for (block, count) in out {
        items.spawn(block, count, primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
    }
}

pub(crate) fn spill_chest(ctx: &Arc<Context>, at: containers::ChestPos) {
    close_chest_for_everyone(ctx, at);
    // Whatever took the cell, a stall's owner and prices go with it. See
    // `Stalls::forget`.
    ctx.stalls.lock().unwrap_or_else(|e| e.into_inner()).forget(at);
    // A mod breaking a ruin chest nobody has opened gets what was in it;
    // the player's break unseals before it writes the cell, which this
    // line could not do for it.
    unseal_ruin_chest(ctx, at);
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
        // `spawn_worn`: a chest full of half-spent tools broken open
        // used to spill a chest full of new ones.
        items.spawn_worn(
            stack.block,
            stack.count,
            stack.damage,
            primitive_shared::geometry::wide(at),
            (0.0, 0.0, 0.0),
            None,
            now,
        );
    }
}

/// Which cell a dead player's body goes into, given a way to look at the
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
/// Two passes, and their order is the decision. A body lying on the
/// ground is a body you can see from where you respawned; a body hanging
/// in the air over the ravine that killed you is a second death. So a
/// cell with a floor under it wins over a nearer one without, and only
/// when nothing in reach has a floor does the first free cell take it.
pub(crate) fn corpse_cell(
    at: (f32, f32, f32),
    look: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
) -> Option<(i32, i32, i32)> {
    use primitive_shared::types::{has_full_top, layer_placement, BLOCK_CORPSE, CHUNK_SIZE_Y};

    if !at.0.is_finite() || !at.1.is_finite() || !at.2.is_finite() {
        return None;
    }
    let (x, z) = (at.0.floor() as i32, at.2.floor() as i32);
    let feet = at.1.floor() as i32;

    // The cell they stood in first, then upwards, then the two below.
    // Up before down because the commonest way to die inside a block is
    // to be crushed or drowned in one, and the commonest way to die over
    // a hole is to fall into it -- in both cases the body wants to be
    // where the player can walk back to, not where they cannot.
    let free = |y: i32| -> bool {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            return false;
        }
        look(x, y, z).is_some_and(|here| layer_placement(here, BLOCK_CORPSE).is_some())
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
/// Returns where the body went and whatever would not fit in it.
///
/// ## Why the lock spans the whole of it
///
/// **This was three steps with nothing joining them, and the gap
/// between them destroyed things.** The old shape was: read the world to
/// pick a free cell, write the block into it, take the chest lock,
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
/// caller, which is the same fallback a body that could not be placed at
/// all already used.
fn stash_corpse(
    chests: &mut containers::Chests,
    contents: &primitive_shared::inventory::Inventory,
    position: (f32, f32, f32),
    look: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
    place: impl FnOnce(i32, i32, i32) -> bool,
) -> Option<(containers::ChestPos, primitive_shared::inventory::Inventory)> {
    use primitive_shared::inventory::Inventory;

    let at = corpse_cell(position, &look)?;
    if !place(at.0, at.1, at.2) {
        return None;
    }
    let mut leftovers = Inventory::new();
    chests.edit(at, |inventory| {
        // **Laid in whole when the cell is empty, which is every death
        // there has ever been.** Merging by `add_worn` re-deals: it tops up
        // matching stacks and then fills the first free square, so a pack
        // arranged square by square arrived arranged by nothing -- and a
        // body's rucksack compartment, which `add_worn` into a forty-square
        // box has no squares for, arrived in the body's forty or not at
        // all. An empty cell has nothing an assignment could delete, which
        // is the one objection the note above makes to it.
        if inventory.is_empty() {
            *inventory = contents.clone();
            return;
        }
        // The belt-and-braces merge: make room for a compartment first,
        // so a second body's rucksack is not squeezed into the first's
        // forty and out into the leftovers.
        if contents.has_compartment() && !inventory.has_compartment() {
            let mut grown = primitive_shared::inventory::Inventory::body(true);
            for (square, stack) in inventory.slots().iter().enumerate() {
                if let Some(stack) = *stack {
                    grown.put_in_slot(square, stack);
                }
            }
            *inventory = grown;
        }
        for stack in contents.slots().iter().flatten() {
            // `add_worn`, not `add`: the body a player leaves where they
            // died holds *their* tools, and `add` puts a pristine one in
            // the slot. Dying with three nearly-spent pickaxes and
            // walking back to collect three new ones is a repair bench
            // whose only cost is a respawn.
            let left = inventory.add_worn(stack.block, stack.count, stack.damage);
            if left > 0 {
                leftovers.add_worn(stack.block, left, stack.damage);
            }
        }
    });
    Some((at, leftovers))
}

/// Lays the player's own body where they fell, with everything they were
/// carrying and wearing still on it.
///
/// ## Why a body and not a bag
///
/// This used to leave a backpack (`types::BLOCK_BACKPACK`, which is
/// still defined because old worlds have bags standing in them). The
/// container underneath is unchanged; what changed is what it is, and
/// the reason is that **a bag is an object the world has no opinion
/// about.** It sat there, perfect, for ever. A body is something the
/// world does something to: it rots, on the same clock as every other
/// piece of meat in the game (`logic::carrion`), and at the end of that
/// it is bones holding the half of the kit rot could not touch
/// (`types::BLOCK_REMAINS`). That turns "walk back for your things" from
/// an errand into a decision with a price on either side -- which is the
/// difference between a mechanic and a formality.
///
/// It is also simply what a player expects to find. The bag was a
/// stand-in for the body, and the question "where did I die" was
/// answered by a picture of luggage.
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
/// body is a chest with a different picture on it and no recipe.
///
/// ## Why the pack is emptied even if the block cannot be placed
///
/// The one outcome nothing here may produce is a player who respawns
/// carrying their things *and* a body in the world holding them, which is
/// how a death doubles somebody's stock. So the inventory is taken
/// first, and every path after that is about where it ends up; the worst
/// case is the heap of drops this exists to avoid, not a duplication.
///
/// ## The way back
///
/// A body that holds its contents is only half of making a death a
/// trip rather than a loss: the other half is finding it. A player who
/// fell into a ravine at dusk, respawned a kilometre away and walked back
/// through a forest that all looks the same did not lose their things to
/// the death, they lost them to the forest. So the cell goes on the
/// player's list (`remember_bag`) and down the wire to them alone
/// (`ServerMessage::Landmarks`), and the client draws a mark on the map
/// until it is emptied (`forget_recovered_bags`). There used to be a
/// compass to it on the HUD as well; it was taken out, and the way back is
/// read off the map (see the client's `ui::journal`).
///
/// ## Why anyone may open it
///
/// **There is no lock on a body, and there never was one on the bag
/// either.** This is stated again here because it reads like an
/// oversight and is a decision: the alternative was the owner only, for
/// some minutes, then anybody, and it was rejected for three reasons
/// that each decide it alone.
///
/// * **The server is small and the players are friends.** The commonest
///   person to reach a body before its owner is the friend who was
///   standing beside them when they died, and a lock turns "I've got your
///   things" into "I can't touch them, walk back yourself".
/// * **The location is already private.** Only the owner is told where
///   the body is. Somebody else finding it found it -- by being there --
///   which is a thing that happens in a world, not a rule being bent.
/// * **A timer is a number to wait out, not a decision.** A thief who
///   knows the lock lasts ten minutes waits ten minutes; the owner who is
///   eleven minutes away has been given a countdown instead of a reason
///   to hurry.
///
/// What a lock would have cost in machinery -- an owner and a clock per
/// container, saved, checked on every gesture -- is the smaller reason.
/// The rot is the clock this mechanic has, and it runs against the
/// world rather than against the other players.
/// What a body with these things in it is drawn wearing: for each slot, the
/// first garment among its contents that goes there, or `BLOCK_AIR`.
///
/// **Read off the contents, not remembered from the death**, and that is a
/// decision rather than a shortcut. A record of what was worn would be a
/// second thing to save beside the container and a second thing to keep true
/// as it is looted; and a body drawn in a cuirass somebody already took is a
/// body that lies about the one thing a player walking back to it wants to
/// know. The cost is that a spare helmet carried in the pack can be the one
/// drawn -- which is still a helmet that is really there.
///
/// Rejected: the variant bits of `BLOCK_CORPSE`. There are three of them,
/// and a worn set is four block ids.
pub(crate) fn body_worn(
    contents: &primitive_shared::inventory::Inventory,
) -> [primitive_shared::types::BlockId; primitive_shared::equipment::SLOTS] {
    let mut worn = [primitive_shared::types::BLOCK_AIR; primitive_shared::equipment::SLOTS];
    // Over what the container actually has rather than over a player's
    // own count: this is a corpse, which is `CHEST_SLOTS` long, and
    // walking a pack's worth of it would miss a helmet that happened to
    // land past the twentieth square.
    for square in 0..contents.slots().len() {
        let Some(block) = contents.block_in(square) else {
            continue;
        };
        if let Some(slot) = primitive_shared::equipment::slot_of(block) {
            if worn[slot.index()] == primitive_shared::types::BLOCK_AIR {
                worn[slot.index()] = block;
            }
        }
    }
    worn
}

/// The `PitPottery` message for a cell, if a pit kiln is there: what is in
/// it, in the order it went in.
pub(crate) fn pit_pottery_message(ctx: &Arc<Context>, at: (i32, i32, i32)) -> Option<ServerMessage> {
    let block = ctx.world.cached_block(at.0, at.1, at.2)?;
    if !primitive_shared::pit::is_pit_kiln(block) {
        return None;
    }
    let pieces = ctx.pits.lock().unwrap_or_else(|e| e.into_inner()).pottery(at).to_vec();
    Some(ServerMessage::PitPottery { x: at.0, y: at.1, z: at.2, pieces })
}

/// Tells everyone who has the chunk what is in the pit kiln at `at` now.
/// Nothing, if there is no pit there -- a client forgets a pit's pottery
/// when the cell stops being a pit, which it hears as a block change.
pub(crate) fn tell_pit_pottery(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    let Some(message) = pit_pottery_message(ctx, at) else {
        return;
    };
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(message.clone());
    }
}

/// **Tells everyone who can see this chest whether its lid is up**, which is
/// whether anybody is standing at it right now.
///
/// Called wherever a player's `open_chest` is set or cleared -- opening,
/// closing, dying, respawning, being made to let go, and leaving the server
/// -- and it works the answer out from the registry rather than being told
/// it. That is the whole of what keeps two players at one chest honest: the
/// second to open it changes nothing, and the first to walk away does not
/// shut the lid in the other one's face.
///
/// Only for a chest. A rack, a hearth and a set-down cell are containers by
/// the same messages (`open_chest`) and have no lid to move.
pub(crate) fn tell_chest_lid(ctx: &Arc<Context>, at: containers::ChestPos) {
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return;
    };
    if primitive_shared::types::block_kind(block) != primitive_shared::types::BLOCK_CHEST {
        return;
    }
    let open = ctx.registry.handles().iter().any(|handle| {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.open_chest == Some(at)
    });
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(ServerMessage::ChestLid { x: at.0, y: at.1, z: at.2, open });
    }
}

/// **The lids standing open in a chunk**, as the messages that say so: one
/// `ChestLid { open: true }` for every chest in it that somebody is at.
///
/// Sent right behind the chunk (`connection`'s chunk pump), for the reason a
/// body's clothes and a pit's pots are: the chunk carries the chest and not
/// who is standing at it, and a client that has only ever heard of a lid by
/// its opening draws it shut -- which was every chest opened before the
/// watcher walked up, and every chest whose chunk was unloaded and loaded
/// again while somebody stood at it (the client forgets a chunk's lids with
/// the chunk, `ChunkManager::forget_bodies`).
///
/// From the players rather than from the chunk's cells: at most one open
/// chest a player, and a server has a handful of players and a chunk has
/// sixteen thousand cells.
pub(crate) fn open_lids_in(ctx: &Arc<Context>, pos: ChunkPos) -> Vec<ServerMessage> {
    let mut open: Vec<containers::ChestPos> = ctx
        .registry
        .handles()
        .iter()
        .filter_map(|handle| handle.state.lock().unwrap_or_else(|e| e.into_inner()).open_chest)
        .filter(|at| ChunkPos::from_global(at.0, at.2).0 == pos)
        .collect();
    // Two players at one chest are one lid.
    open.sort_unstable();
    open.dedup();
    open.into_iter()
        .filter(|at| {
            ctx.world
                .cached_block(at.0, at.1, at.2)
                .is_some_and(|block| primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CHEST)
        })
        .map(|at| ServerMessage::ChestLid { x: at.0, y: at.1, z: at.2, open: true })
        .collect()
}

/// The `SetDownItem` message for a cell, if a hand set something down there:
/// the kind in its store, or air for a store with nothing in it.
pub(crate) fn set_down_item_message(ctx: &Arc<Context>, at: (i32, i32, i32)) -> Option<ServerMessage> {
    use primitive_shared::types::BLOCK_AIR;
    let block = ctx.world.cached_block(at.0, at.1, at.2)?;
    if !primitive_shared::types::is_set_down(block) {
        return None;
    }
    let item = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at).slots().iter().flatten().next().map_or(BLOCK_AIR, |stack| stack.block)
    };
    Some(ServerMessage::SetDownItem { x: at.0, y: at.1, z: at.2, item })
}

/// Tells everyone who has the chunk what lies in the set-down cell at `at`
/// now. Nothing, if nothing was set down there.
///
/// **A cell whose store has gone empty is cleared here**, because this is
/// where every change to a store ends (`broadcast_chest_state`): the rot
/// clock is the one thing that can empty one without a hand, and a loaf
/// that rotted to nothing must not leave an invisible cell nobody can put
/// anything in.
pub(crate) fn tell_set_down(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    use primitive_shared::types::BLOCK_AIR;
    let Some(message) = set_down_item_message(ctx, at) else {
        return;
    };
    if matches!(message, ServerMessage::SetDownItem { item: BLOCK_AIR, .. }) {
        if ctx.world.set_block(at.0, at.1, at.2, BLOCK_AIR) {
            ctx.chests.lock().unwrap_or_else(|e| e.into_inner()).take(at);
            notify_mechanics(ctx, at.0, at.1, at.2);
            broadcast_block(ctx, at, BLOCK_AIR);
        }
        return;
    }
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(message.clone());
    }
}

/// The `BodyWorn` message for a cell, if a body lies there.
pub(crate) fn body_worn_message(ctx: &Arc<Context>, at: (i32, i32, i32)) -> Option<ServerMessage> {
    let block = ctx.world.cached_block(at.0, at.1, at.2)?;
    if primitive_shared::types::block_kind(block) != primitive_shared::types::BLOCK_CORPSE {
        return None;
    }
    let contents = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        chests.contents(at)
    };
    Some(ServerMessage::BodyWorn { x: at.0, y: at.1, z: at.2, worn: body_worn(&contents) })
}

/// Tells everyone who has the chunk what the body at `at` is wearing now.
/// Nothing, if no body lies there.
pub(crate) fn tell_body_worn(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    let Some(message) = body_worn_message(ctx, at) else {
        return;
    };
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(message.clone());
    }
}

#[cfg(test)]
mod body_worn_tests {
    use primitive_shared::equipment::Slot;
    use primitive_shared::inventory::{Inventory, Stack};
    use primitive_shared::types::*;

    /// **A body is drawn in what is on it, slot by slot, and in nothing that
    /// is not.** A pack of a pick, a helmet and boots is a body in a helmet
    /// and boots with bare legs and chest; the first helmet of two is the one
    /// drawn, so the picture does not flicker between them as the pack is
    /// sorted; and a body emptied of its clothes is a bare body.
    #[test]
    fn a_body_wears_the_first_garment_for_each_slot_that_is_in_it() {
        let mut contents = Inventory::new();
        contents.put_in_slot(0, Stack::new(BLOCK_STONE_PICKAXE, 1));
        contents.put_in_slot(3, Stack::new(BLOCK_IRON_HELM, 1));
        contents.put_in_slot(5, Stack::new(BLOCK_BRONZE_HELM, 1));
        contents.put_in_slot(9, Stack::new(BLOCK_LEATHER_BOOTS, 1));
        let worn = super::body_worn(&contents);
        assert_eq!(worn[Slot::Head.index()], BLOCK_IRON_HELM);
        assert_eq!(worn[Slot::Feet.index()], BLOCK_LEATHER_BOOTS);
        assert_eq!(worn[Slot::Chest.index()], BLOCK_AIR);
        assert_eq!(worn[Slot::Legs.index()], BLOCK_AIR);
        assert_eq!(super::body_worn(&Inventory::new()), [BLOCK_AIR; primitive_shared::equipment::SLOTS]);
    }

    /// **A rucksack is on the body too, and it is drawn on the back.**
    /// A death takes the pack and leaves the armour (`leave_corpse`); a
    /// rucksack is neither armour nor loose cargo, and the reason it is
    /// here is that the body a player walks back to has to look like the
    /// player who died. It is also the reason `body_worn` walks the
    /// whole container: a corpse is `CHEST_SLOTS` long, and a rucksack
    /// that landed past a pack's worth of squares would be invisible.
    #[test]
    fn a_body_that_was_wearing_a_rucksack_is_drawn_still_wearing_it() {
        let mut contents = Inventory::chest();
        let last = primitive_shared::inventory::CHEST_SLOTS - 1;
        contents.put_in_slot(last, Stack::new(BLOCK_RUCKSACK, 1));
        let worn = super::body_worn(&contents);
        assert_eq!(worn[Slot::Back.index()], BLOCK_RUCKSACK);
        assert_eq!(worn[Slot::Chest.index()], BLOCK_AIR, "a bag is not a cuirass");
    }
}

#[cfg(test)]
mod rucksack_tests {
    use primitive_shared::inventory::{
        Equipment, Inventory, Stack, BACKPACK_RANGE, BACKPACK_SLOTS, MAX_SLOTS, SLOTS,
    };
    use primitive_shared::types::{BLOCK_RUCKSACK, BLOCK_STONE};

    /// **Putting a rucksack on is the one equip that changes how big the
    /// pack is**, and taking it off is the one that can be refused for a
    /// reason that is not "no room". Both halves are checked against the
    /// same two calls the request handlers make, rather than against the
    /// handlers themselves: those need a whole `Context`, a socket and a
    /// world, and what is worth pinning here is the rule.
    #[test]
    fn a_rucksack_lends_ten_squares_and_takes_them_back_only_when_they_are_empty() {
        let mut pack = Inventory::new();
        let mut worn = Equipment::new();
        assert_eq!(pack.slots().len(), SLOTS);

        // On: the squares appear before anything looks for a home in
        // them -- see `equip_from_slot`.
        assert!(worn.wear(Stack::new(BLOCK_RUCKSACK, 1)).is_none());
        pack.open_backpack();
        assert_eq!(pack.slots().len(), MAX_SLOTS);
        assert_eq!(MAX_SLOTS - SLOTS, BACKPACK_SLOTS);

        // Something in it, and it stays on.
        pack.put_in_slot(BACKPACK_RANGE.start, Stack::new(BLOCK_STONE, 3));
        let mut trial = pack.clone();
        assert!(!trial.close_backpack(), "a loaded rucksack came off");

        // Emptied, and it comes off -- and the pack is back to the
        // body's own squares with nothing lost.
        pack.take_slot(BACKPACK_RANGE.start);
        pack.add(BLOCK_STONE, 3);
        assert!(pack.close_backpack());
        assert_eq!(pack.slots().len(), SLOTS);
        assert_eq!(pack.count(BLOCK_STONE), 3, "the stone went with the rucksack");
        assert!(worn.take(primitive_shared::equipment::Slot::Back).is_some());
    }

    /// **A player who died in a rucksack finds it packed as they packed
    /// it.** Every square of the pack on the body's square of the same
    /// number, every square of the rucksack in the body's compartment in
    /// the same order, the rucksack itself among the body's things, and
    /// nothing on the ground.
    #[test]
    fn a_body_keeps_a_rucksacks_contents_square_for_square() {
        use primitive_shared::inventory::{CORPSE_COMPARTMENT, CORPSE_SLOTS};
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_STICK};
        let mut pack = Inventory::new();
        pack.open_backpack();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE, 9));
        pack.put_in_slot(SLOTS - 1, Stack::new(BLOCK_DIRT, 4));
        pack.put_in_slot(BACKPACK_RANGE.start, Stack::new(BLOCK_STICK, 3));
        pack.put_in_slot(BACKPACK_RANGE.start + 13, Stack::new(BLOCK_STONE, 7));
        pack.put_in_slot(MAX_SLOTS - 1, Stack::new(BLOCK_DIRT, 1));

        let (body, dropped) = super::body_from_pack(&pack, vec![Stack::new(BLOCK_RUCKSACK, 1)]);
        assert!(dropped.is_empty(), "something went on the ground: {dropped:?}");
        assert_eq!(body.slots().len(), CORPSE_SLOTS, "the body has no rucksack compartment");
        for square in 0..SLOTS {
            if square != 1 {
                assert_eq!(body.slots()[square], pack.slots()[square], "pack square {square} moved");
            }
        }
        for (offset, square) in BACKPACK_RANGE.enumerate() {
            assert_eq!(
                body.slots()[CORPSE_COMPARTMENT.start + offset],
                pack.slots()[square],
                "rucksack square {offset} moved"
            );
        }
        // The rucksack itself into the first free square of the body's
        // forty, not into its own compartment.
        assert_eq!(body.block_in(1), Some(BLOCK_RUCKSACK));
    }

    /// **The clothes go into the compartment when the body's forty are
    /// full, and onto the ground only when both are.** See
    /// `body_from_pack` for why the compartment beats the grass.
    #[test]
    fn a_garment_with_no_room_in_the_body_goes_to_its_rucksack_and_then_to_the_ground() {
        use primitive_shared::inventory::CORPSE_COMPARTMENT;
        use primitive_shared::types::BLOCK_IRON_HELM;
        let mut pack = Inventory::new();
        pack.open_backpack();
        for square in 0..SLOTS {
            pack.put_in_slot(square, Stack::new(BLOCK_STONE, 1));
        }
        let (body, dropped) = super::body_from_pack(&pack, vec![Stack::new(BLOCK_IRON_HELM, 1)]);
        assert!(dropped.is_empty());
        assert_eq!(body.block_in(CORPSE_COMPARTMENT.start), Some(BLOCK_IRON_HELM));

        // No rucksack: forty squares, all full, and the helmet is dropped
        // rather than deleted.
        let mut bare = Inventory::new();
        for square in 0..SLOTS {
            bare.put_in_slot(square, Stack::new(BLOCK_STONE, 1));
        }
        let (body, dropped) = super::body_from_pack(&bare, vec![Stack::new(BLOCK_IRON_HELM, 1)]);
        assert_eq!(body.slots().len(), primitive_shared::inventory::CHEST_SLOTS);
        assert_eq!(dropped.len(), 1, "the helmet went nowhere");
    }
}

/// What a dead player's pack and clothes become as a body, and what is left
/// over to be dropped beside it.
///
/// **Square for square, the rucksack included.** A pack's own forty land on
/// the body's squares of the same number, and a worn rucksack's twenty land
/// in the body's compartment after them (`inventory::CORPSE_COMPARTMENT`),
/// in the same order -- so the body a player walks back to is laid out the
/// way their pack was, rucksack and all. It used to be forty squares
/// whatever was worn, and the rucksack's twenty were dealt into whatever
/// was free and the rest spilled on the ground: nothing was deleted, and a
/// player who had packed their rucksack with care found it in the grass.
///
/// Not `add`, anywhere: `add` merges, and a half-worn axe merged into a
/// fresh one loses its wear.
///
/// **The clothes go last, into the first free square: the body's forty
/// first, then the compartment, then the ground.** Forty squares and five
/// garments only runs out when the pack was full to the last square, and
/// the choice then is between a garment in the rucksack's compartment and a
/// garment on the grass. The compartment wins because it is on the body --
/// the grass is where things go missing, and this function exists so that
/// nothing does. What it costs is a helmet in the rucksack's squares, which
/// is a stranger place for a helmet and still a place the player will look.
/// A body without a compartment keeps the old rule: forty, then the ground.
fn body_from_pack(
    pack: &primitive_shared::inventory::Inventory,
    garments: Vec<primitive_shared::inventory::Stack>,
) -> (primitive_shared::inventory::Inventory, Vec<primitive_shared::inventory::Stack>) {
    use primitive_shared::inventory::{Inventory, BACKPACK_RANGE, CORPSE_COMPARTMENT, SLOTS};
    let mut body = Inventory::body(pack.backpack_open());
    let mut homeless = Vec::new();
    for (square, stack) in pack.slots().iter().enumerate() {
        let Some(stack) = *stack else {
            continue;
        };
        let home = if square < SLOTS {
            Some(square)
        } else if BACKPACK_RANGE.contains(&square) {
            Some(CORPSE_COMPARTMENT.start + (square - BACKPACK_RANGE.start))
        } else {
            // Past a worn rucksack's end: a pack no build makes, found
            // first a free square like a garment is.
            None
        };
        match home {
            Some(home) => homeless.extend(body.put_in_slot(home, stack)),
            None => homeless.push(stack),
        }
    }
    let mut overflow = Vec::new();
    for stack in garments.into_iter().chain(homeless) {
        // In index order, so the body's forty before the compartment --
        // see the note above.
        match (0..body.slots().len()).find(|&square| body.block_in(square).is_none()) {
            Some(free) => overflow.extend(body.put_in_slot(free, stack)),
            None => overflow.push(stack),
        }
    }
    (body, overflow)
}

pub(crate) fn leave_corpse(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::BLOCK_CORPSE;

    // An empty pack leaves nothing behind -- not even a body. Checked
    // before anything is moved: a body with nothing in it is a block the
    // player has to walk back to, break, and find empty, which is worse
    // than no body at all.
    //
    // (A corpse for its own sake, empty, as a marker of where you died,
    // was considered and dropped: the map already marks that, and a world
    // where every bad night leaves a body nobody will ever open is a
    // world littered with rubbish that has to be cleared by hand.)
    let (contents, position, stripped, overflow) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.inventory.is_empty() && state.equipment.is_empty() {
            return;
        }
        let taken = std::mem::replace(&mut state.inventory, Inventory::new());
        // **What they were wearing is still on the body.** A death that
        // took the pack and left the armour on would make a full iron
        // set the one thing in the world you cannot lose, which is
        // exactly backwards: the armour is the most valuable thing a
        // player owns and losing it is most of what a death costs.
        //
        // Into the same container rather than a second one, because
        // there is nothing about a body that has to know a cuirass was
        // worn rather than carried -- and one container is one thing to
        // walk back to.
        let garments: Vec<primitive_shared::inventory::Stack> = primitive_shared::equipment::ALL_SLOTS
            .into_iter()
            .filter_map(|slot| state.equipment.take(slot))
            .collect();
        let stripped = !garments.is_empty();
        state.inventory_dirty = true;
        state.equipment_dirty |= stripped;
        let (body, overflow) = body_from_pack(&taken, garments);
        (body, state.position, stripped, overflow)
    };
    send_inventory(handle);
    if stripped {
        send_equipment(handle);
        refresh_carried_weight(handle);
    }

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
    if !overflow.is_empty() {
        let mut spilled = Inventory::chest();
        for (square, stack) in overflow.into_iter().enumerate() {
            spilled.put_in_slot(square, stack);
        }
        spill_inventory(ctx, &spilled, primitive_shared::geometry::narrow(position));
    }

    // Choosing the cell, claiming it and filling it, all under the one
    // lock. See `stash_corpse` for why that is the fix and not an
    // incidental tidy-up.
    let stashed = {
        let mut chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        stash_corpse(
            &mut chests,
            &contents,
            primitive_shared::geometry::narrow(position),
            |x, y, z| ctx.world.cached_block(x, y, z),
            |x, y, z| ctx.world.set_block(x, y, z, BLOCK_CORPSE),
        )
    };
    let Some((at, leftovers)) = stashed else {
        // Nowhere to put it: buried to the horizon, or at the very top
        // of the world. The heap of drops is the fallback rather than
        // the design, and it is still better than silence.
        spill_inventory(ctx, &contents, primitive_shared::geometry::narrow(position));
        return;
    };
    // Whatever would not fit, which needs a bag already holding
    // something *and* a second body landing in it -- see `stash_corpse`.
    // Drops rather than deletion, on the rule this whole function is
    // built on: the one outcome nothing here may produce is things
    // ceasing to exist.
    if !leftovers.is_empty() {
        spill_inventory(ctx, &leftovers, primitive_shared::geometry::narrow(position));
    }

    let change = BlockChange {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        block_id: BLOCK_CORPSE,
    };
    let (chunk_pos, _, _) = ChunkPos::from_global(at.0, at.2);
    for subscriber in ctx.registry.subscribers(chunk_pos) {
        subscriber.send(ServerMessage::BlockUpdate(change));
    }
    // ...and what the body has on, after the block, so no client is told
    // about clothes on a cell it still thinks is grass.
    tell_body_worn(ctx, at);
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(at.0, at.1, at.2);
    }
    notify_mechanics(ctx, at.0, at.1, at.2);

    // ...and the way back to it. After the block is in the world and the
    // things are in the block, so the map never marks a cell the tick
    // could find empty and take straight off it again.
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        remember_bag(&mut state.bags, at);
    }
    send_landmarks(ctx, handle);

    if ctx.options.logging {
        println!(
            "[survival] {}'s body is at ({}, {}, {})",
            handle.username, at.0, at.1, at.2
        );
    }
}

/// How many bodies a player is shown the way back to.
///
/// Four, and it is the oldest that is forgotten. A player who has died
/// four times without going back has given up on the first trip, and a
/// map spotted with ten old graves hides the one they are actually
/// walking to. **The body itself is not touched**: it is still in the
/// world -- as bones by then -- a find for whoever walks past it, which
/// is what a body nobody came back for is.
///
/// The name is the wire's: `ServerMessage::Landmarks` has carried a
/// field called `bags` since before there were bodies, and renaming a
/// protocol field to rename a picture would be a version bump for
/// nothing.
pub(crate) const MAX_BAGS: usize = 4;

/// Puts a body on a player's list, newest last.
///
/// Dying twice in one cell is one mark -- `stash_corpse` merges into a
/// container already there -- so it is one entry, moved to the end
/// rather than listed twice.
fn remember_bag(bags: &mut Vec<(i32, i32, i32)>, at: (i32, i32, i32)) {
    bags.retain(|&bag| bag != at);
    bags.push(at);
    if bags.len() > MAX_BAGS {
        let over = bags.len() - MAX_BAGS;
        bags.drain(..over);
    }
}

/// Whether a body a player was shown is no longer worth walking to.
///
/// `None` is "that chunk is not loaded", and it is *not* gone: the far
/// side of the world is exactly where a body is, and the chunk it lies
/// in is uncached for as long as nobody stands near it. Taking it off
/// the map for that would take every body off the map the moment the
/// player respawned. A cell that is loaded and is no longer anywhere
/// their things could be was broken, burnt or buried; one that still is
/// and holds nothing has been emptied -- by the owner or by anybody, see
/// `leave_corpse`.
///
/// **Three blocks count as "their things could be in there", and none of
/// the three is padding.** The body becomes bones where it lies
/// (`BLOCK_CORPSE` -> `BLOCK_REMAINS`, two days, `logic::carrion`), and
/// the mark has to survive that: forgetting a grave the moment it rots
/// would take it off the map at exactly the hour the player is hurrying
/// to it, and they would be walking to a cell the client no longer
/// draws. The backpack is the third because worlds saved before this
/// have bags standing in them and profiles listing where they are --
/// dropping it here would clear every old bag off every old map on the
/// first tick after the update.
fn bag_is_gone(block: Option<primitive_shared::types::BlockId>, holds_anything: bool) -> bool {
    use primitive_shared::types::{block_kind, is_corpse, BLOCK_BACKPACK};
    match block {
        None => false,
        Some(block) => {
            let still_somewhere_their_things_are =
                is_corpse(block) || block_kind(block) == BLOCK_BACKPACK;
            !still_somewhere_their_things_are || !holds_anything
        }
    }
}

/// Tells a player where the spawn and their bodies are.
pub(crate) fn send_landmarks(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let spawn = ctx.world.spawn_point();
    let bags = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.bags.clone()
    };
    handle.send(ServerMessage::Landmarks {
        spawn: (spawn.0.floor() as i32, spawn.1.floor() as i32, spawn.2.floor() as i32),
        bags,
    });
}

/// Takes the bodies that have been emptied or lost off a player's list.
///
/// **Asked every tick rather than hooked into every way a body can go.**
/// One is emptied by a take-all, by forty single moves, by breaking it,
/// by a fire, by a cave-in, by a mod; a hook in each is a map that marks
/// an empty cell forever the day somebody adds a seventh. The
/// question is a lock, an empty check and -- only for a player who has a
/// body out there -- a map lookup per mark, twenty times a second.
///
/// The player's lock is let go before the chest map is taken, which is
/// the order `leave_corpse` takes them in.
pub(crate) fn forget_recovered_bags(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let bags = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.bags.is_empty() {
            return;
        }
        state.bags.clone()
    };
    let gone: Vec<(i32, i32, i32)> = {
        let chests = ctx.chests.lock().unwrap_or_else(|e| e.into_inner());
        bags.into_iter()
            .filter(|&at| bag_is_gone(ctx.world.cached_block(at.0, at.1, at.2), chests.holds_anything(at)))
            .collect()
    };
    if gone.is_empty() {
        return;
    }
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.bags.retain(|bag| !gone.contains(bag));
    }
    send_landmarks(ctx, handle);
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
    let (from, held, strength) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            return;
        }
        let now = std::time::Instant::now();
        let held = state.inventory.block_in(state.selected_slot);
        let ready = state.last_swing.is_none_or(|last| {
            now.saturating_duration_since(last).as_secs_f32()
                >= combat::swing_seconds(held) - combat::COOLDOWN_SLACK_SECS
        });
        if !ready {
            return;
        }
        state.last_swing = Some(now);
        // Seen by everybody near, hit or miss: the arm moved either way.
        state.gesture.made(primitive_shared::protocol::Action::Strike);
        (state.position, held, state.vitals.strength_factor())
    };

    // The victim's half. Note the lock is taken *after* the attacker's
    // is released: two players punching each other at the same instant
    // on two connection tasks would otherwise be a deadlock, and the
    // only thing needed from the first lock is a position.
    // Reach is judged before anything is taken off the blow, because
    // being out of range is not a hit that was stopped -- it is a miss,
    // and a miss must not wear anybody's armour out.
    {
        let state = victim.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() || !combat::within_reach(from, state.position, held) {
            return;
        }
    }
    let outcome = strike_player(
        ctx,
        &victim,
        // A broken arm swings at half, at a person as at a boar.
        combat::MELEE_DAMAGE * strength,
        &format!("was struck down by {}", handle.username),
        blow_of(held),
    );
    report_vitals(ctx, &victim, outcome);
}

/// Puts a dead player back in the world at full health.
pub(crate) fn respawn_player(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    let spawn = ctx.world.spawn_point();
    let was_at_chest = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.vitals.respawn();
        state.position = primitive_shared::geometry::wide(spawn);
        // Whatever they were rummaging in is a long way away now. Its lid
        // is told below, once this lock is let go of (`tell_chest_lid`
        // takes every player's).
        let was_at_chest = state.open_chest.take();
        // ...and so is whatever they were sitting or lying on. Death woke
        // a sleeper already (a blow does), but a seat was only let go of
        // when the next tick noticed the distance, and until then a fresh
        // spawn was resting on a stool across the map.
        state.sleeping_in = None;
        state.asleep_since = None;
        state.sitting_on = None;
        // The anti-cheat has to be told, or the jump from the death site
        // to the spawn point looks exactly like the teleport hack it
        // exists to catch.
        state.anticheat.reset_to(primitive_shared::geometry::wide(spawn));
        was_at_chest
    };
    if let Some(at) = was_at_chest {
        tell_chest_lid(ctx, at);
    }
    handle.send(ServerMessage::Respawned {
        x: f64::from(spawn.0),
        y: f64::from(spawn.1),
        z: f64::from(spawn.2),
    });
    send_health(handle);
}

/// Grants or withdraws flight for one player.
///
/// **The one place in the server that turns it on**, so there is one
/// place where the three things that have to happen together do:
///
/// * the runtime state, which is what the fall tracker reads;
/// * the anti-cheat, which otherwise starts flagging a player for doing
///   exactly what it was just told they may do;
/// * the client, which is the only side that can actually stop applying
///   gravity.
///
/// Any two of those without the third is a bug with a distinctive shape:
/// forget the anti-cheat and the player is kicked for flying; forget the
/// client and nothing happens at all; forget the state and they take
/// fall damage for hovering.
///
/// Idempotent: granting flight to somebody already flying at the same
/// speed re-sends nothing. That matters because the natural way to write
/// a mod is to assert the state every tick.
///
/// **Behind the `mods` feature, because that is the only thing that
/// calls it.** Nothing in the game grants flight on its own -- there is
/// no `/fly` in `logic::commands` and deliberately so, since whether
/// players may fly is a decision about a particular server rather than
/// about the game. The client's embedded server has mods off (see
/// `primitive_server/Cargo.toml`), so a singleplayer world compiles
/// without this and cannot grant flight, which is the truth about it
/// rather than an oversight.
#[cfg(feature = "mods")]
pub(crate) fn set_flight(
    handle: &Arc<players::PlayerHandle>,
    flying: bool,
    speed: f32,
) -> bool {
    let speed = if speed.is_finite() && speed > 0.0 {
        speed.clamp(1.0, 80.0)
    } else {
        players::DEFAULT_FLY_SPEED
    };
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.flying == flying && (!flying || (state.fly_speed - speed).abs() < 0.01) {
            return false;
        }
        state.flying = flying;
        state.fly_speed = speed;
        state.anticheat.set_flying(flying);
        // Whatever height they were at is not a fall they took. Coming
        // *out* of flight starts a new one from here, which is the
        // honest answer -- switching it off over a canyon should hurt.
        state.vitals.clear_fall();
    }
    handle.send(ServerMessage::Flight { enabled: flying, speed });
    true
}

/// Server-authoritative reposition. Reuses `PositionCorrection`, which
/// the client already obeys unconditionally, so no new message type and
/// no new client code path is needed for teleports.
fn teleport(handle: &Arc<players::PlayerHandle>, x: f32, y: f32, z: f32, why: &str) {
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.position = (f64::from(x), f64::from(y), f64::from(z));
        // Tell the anti-cheat too, or the jump it just authorised looks
        // exactly like the teleport hack it exists to catch.
        state.anticheat.reset_to((f64::from(x), f64::from(y), f64::from(z)));
        // And the fall tracker, or being moved downwards -- by `/tp`, by
        // a plugin, or by a rubber-band correction -- arrives as fall
        // damage for a fall that never happened.
        state.vitals.clear_fall();
    }
    handle.send(ServerMessage::PositionCorrection {
        x: f64::from(x),
        y: f64::from(y),
        z: f64::from(z),
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

// ---- one place events happen ----
//
// There are two extension points -- scripted plugins and native mods --
// and they must not become two lists of events that gradually stop
// agreeing about when a block is broken. So every event goes through
// `fire_event`, which fires both, and `fire_plugin_hook` is what it
// calls for the script half.
//
// The order is plugins first, then mods, and it is arbitrary but fixed:
// what matters is that it is the same order every time, so a veto from
// one and an effect from the other compose the same way on every server.

/// Hands an event to the native mods, if this build has any.
///
/// **A macro rather than a function**, and for one reason: in a build
/// without the `mods` feature the `primitive_modapi` types do not
/// exist, so a function signature that mentioned them could not be
/// written at all. The other arm expands to `true` and never looks at
/// its arguments, so a client's embedded server compiles with no
/// mod API, no `libloading`, and no call sites to edit.
///
/// **No `Context` lock may be held across this.** A mod's handler is
/// allowed to call straight back into the host -- that is what the API
/// is for -- and a handler that asks for a block while the caller holds
/// the world lock is a deadlock in somebody else's code that looks like
/// a hang in ours. See `logic::mods`.
///
/// Expands to `false` if any mod cancelled, on the same convention the
/// plugins use.
///
/// The `wants` check comes first and is the point of the macro's shape:
/// building an `EventData` means borrowing strings and reading state,
/// and none of it should happen for an event nobody subscribed to. The
/// body that builds one is only evaluated if somebody is listening.
#[cfg(feature = "mods")]
macro_rules! notify_mods {
    ($ctx:expr, $event:expr, $data:expr) => {{
        let ctx = $ctx;
        let event = $event;
        let listening = ctx
            .mods
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .wants(event);
        if listening {
            let data = $data;
            // Through the free function, which holds no lock while a mod
            // runs. See `logic::mods` -- this was a method taking
            // `&mut self`, and a method is by construction a lock held
            // across somebody else's code.
            matches!(
                crate::mods::dispatch(ctx, event, &data),
                primitive_modapi::HookResult::Continue
            )
        } else {
            true
        }
    }};
}

#[cfg(not(feature = "mods"))]
#[allow(unused_macros)] // there is nothing to notify in this build
macro_rules! notify_mods {
    ($ctx:expr, $event:expr, $data:expr) => {
        true
    };
}

#[allow(unused_imports)] // ...and so nothing uses it either
pub(crate) use notify_mods;

// ---- the events that have no scripted-plugin counterpart ----
//
// `fire_plugin_hook` above is the path for the events both extension
// points see; these are the ones only a native mod can hear, and there
// is one function per event rather than a general "fire this" for two
// reasons.
//
// **A signature that does not mention `primitive_modapi`.** The client
// embeds this crate with the `mods` feature off, and in that build the
// contract's types do not exist at all -- so a call site that named
// `Event::PlayerHurt` could not be compiled out, it would fail to
// parse. Each of these takes plain numbers and hides the whole of that
// behind a `cfg`, which is why the call sites read the same in both
// builds.
//
// **A name that says what happened.** `notify(ctx, 14, data)` at a call
// site is a number somebody has to go and look up; `player_hurt(...)`
// is not. The cancellable ones return `bool`, and `false` means a mod
// said no -- the same convention `fire_plugin_hook` uses.
//
// None of these may be called with a lock held. That is the one rule of
// this whole subsystem (see `logic::mods`), and it is why every one of
// them is called from a statement of its own with the guard block
// already closed.

/// Builds an `EventData` and hands it to whoever subscribed.
///
/// The one place the contract's types are named. Everything above it
/// takes plain numbers.
#[cfg(feature = "mods")]
fn notify_native(
    ctx: &Arc<Context>,
    event: primitive_modapi::Event,
    data: primitive_modapi::EventData,
) -> bool {
    notify_mods!(ctx, event, data)
}

/// A `BlockPos` from the tuples the rest of this file uses.
#[cfg(feature = "mods")]
fn cell(pos: (i32, i32, i32)) -> primitive_modapi::BlockPos {
    primitive_modapi::BlockPos {
        x: pos.0,
        y: pos.1,
        z: pos.2,
    }
}

/// A blow is about to land on a player. `false` means a mod refused it.
pub(crate) fn player_hurt(
    ctx: &Arc<Context>,
    player: PlayerId,
    damage: f32,
    cause: &str,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            float: damage,
            text: primitive_modapi::Str::borrow(cause),
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::PlayerHurt, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, damage, cause);
        true
    }
}

/// Health went back up, by however much.
pub(crate) fn player_healed(ctx: &Arc<Context>, player: PlayerId, amount: f32) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            float: amount,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::PlayerHealed, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, amount);
}

/// Somebody is about to eat something. `false` means a mod refused.
pub(crate) fn player_ate(
    ctx: &Arc<Context>,
    player: PlayerId,
    block: primitive_shared::types::BlockId,
    slot: usize,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            slot: slot as u32,
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::PlayerAte, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, block, slot);
        true
    }
}

/// A head went under, or came back up.
pub(crate) fn player_water_line(
    ctx: &Arc<Context>,
    player: PlayerId,
    at: (i32, i32, i32),
    entered: bool,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            pos: cell(at),
            ..Default::default()
        };
        let event = if entered {
            primitive_modapi::Event::PlayerEnteredWater
        } else {
            primitive_modapi::Event::PlayerLeftWater
        };
        let _ = notify_native(ctx, event, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, at, entered);
}

/// The selected hotbar slot moved.
pub(crate) fn held_slot_changed(
    ctx: &Arc<Context>,
    player: PlayerId,
    slot: usize,
    block: Option<primitive_shared::types::BlockId>,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            slot: slot as u32,
            block: block.unwrap_or(0),
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::HeldSlotChanged, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, slot, block);
}

/// A tree is about to come down. `false` means a mod refused, and it
/// stays standing.
pub(crate) fn tree_felled(
    ctx: &Arc<Context>,
    player: PlayerId,
    at: (i32, i32, i32),
    cells: u32,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            pos: cell(at),
            count: cells,
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::TreeFelled, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, at, cells);
        true
    }
}

/// A stack came off the ground.
pub(crate) fn item_picked_up(
    ctx: &Arc<Context>,
    player: PlayerId,
    block: primitive_shared::types::BlockId,
    count: u32,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            count,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::ItemPickedUp, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, block, count);
}

/// A stack is about to be thrown down. `false` means a mod refused.
pub(crate) fn item_dropped(
    ctx: &Arc<Context>,
    player: PlayerId,
    block: primitive_shared::types::BlockId,
    count: u32,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            count,
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::ItemDropped, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, block, count);
        true
    }
}

/// A tool wore out in somebody's hands.
pub(crate) fn tool_broke(
    ctx: &Arc<Context>,
    player: PlayerId,
    block: primitive_shared::types::BlockId,
    slot: usize,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            slot: slot as u32,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::ToolBroke, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, block, slot);
}

/// Somebody is opening a container. `false` means a mod refused, which
/// is a lock.
pub(crate) fn container_opened(
    ctx: &Arc<Context>,
    player: PlayerId,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            pos: cell(at),
            block,
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::ContainerOpened, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, at, block);
        true
    }
}

/// ...and closed it, or was made to.
pub(crate) fn container_closed(ctx: &Arc<Context>, player: PlayerId, at: (i32, i32, i32)) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            pos: cell(at),
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::ContainerClosed, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, at);
}

/// A hearth finished a batch.
pub(crate) fn smelting_finished(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            pos: cell(at),
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::SmeltingFinished, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, at);
}

/// Something grew.
pub(crate) fn growth_step(
    ctx: &Arc<Context>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            pos: cell(at),
            block,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::GrowthStep, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, at, block);
}

/// Winds the world forward to dawn and wakes everybody.
///
/// **The hours are lived rather than skipped**, and that is the whole
/// design of this function. A night that cost nothing would make a bed
/// the answer to hunger, to thirst and to the cold -- lie down at dusk,
/// stand up fed. So the clock jumps, and every meter that would have
/// moved during those hours is moved by hand: hunger and thirst by the
/// idle rate, tiredness *down* by what the bed is worth.
///
/// Dawn rather than a fixed number of hours, because what a player wants
/// from a bed is the morning; and forward only -- a player who lies down
/// at breakfast sleeps a whole day round to the next dawn, which is
/// silly of them and is what they asked for.
///
/// **Morning wakes a sleeper and leaves them lying there.** It used to
/// stand everybody up, and with the night passing the tick after the one
/// player in a singleplayer world lay down, that was the whole of sleep as
/// a player met it: click the bed, be on your feet at dawn, having watched
/// the sun jump. Now the eyes open in bed -- `ServerMessage::Asleep` goes
/// false and the posture stays lying -- and the player gets up when they
/// press something (`ClientMessage::StandUp`).
fn sleep_through_to_dawn(ctx: &Arc<Context>, handles: &[Arc<players::PlayerHandle>]) {
    /// When morning is, as a fraction of the day. A quarter: midnight
    /// is zero and noon is a half, so this is six in the morning -- the
    /// hour the light comes back (see the client's `sky`).
    const DAWN: f32 = 0.25;

    let now = ctx.clock.time_of_day();
    // How far forward, never backwards and never zero: a player who
    // lies down exactly at dawn sleeps the day round rather than
    // finding the clock refusing to move.
    let ahead = (DAWN - now).rem_euclid(1.0);
    let seconds = ahead * ctx.clock.day_length_seconds();
    ctx.clock.set_time_of_day(DAWN);

    for handle in handles {
        // **The new hour first, and to this player's own queue.** The
        // periodic sync is up to a couple of seconds away, and the client
        // lifts its dark screen when it is told it is awake: a morning
        // sent after the waking would be a night sky the player opens
        // their eyes on, snapping to day a moment later. One queue keeps
        // the order; a broadcast is a second path to race this one.
        handle.send(ServerMessage::TimeSync {
            tick: ctx.clock.tick(),
            time_of_day: DAWN,
            world_days: ctx.clock.world_days(),
        });
        let outcome = {
            let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            // Awake, and still in the bed -- see the note above.
            state.asleep_since = None;
            let rest = state
                .sleeping_in
                .and_then(|at| ctx.world.cached_block(at.0, at.1, at.2))
                .and_then(primitive_shared::body::Rest::of)
                .unwrap_or(primitive_shared::body::Rest::Straw);
            // The night's tiredness, all at once and down to what this
            // sort of bed leaves behind.
            let floor = 1.0 - rest.recovery();
            state.vitals.rest(seconds, primitive_shared::body::SLEEP_RECOVERY_PER_SECOND, floor);
            // ...and the night's hunger and thirst, at the resting
            // rate. A sleeping body is the cheapest a body gets and it
            // is not free.
            let hunger = state.vitals.digest(survival::Effort::IDLE, seconds);
            let thirst = state
                .vitals
                .drink_down(primitive_shared::body::Exertion::RESTING, seconds);
            // **Whatever the night did, reported as it happened.** Both
            // outcomes used to be thrown away and "changed" sent in their
            // place, so a night that starved a sleeper to death marked
            // them dead on the server and told nobody: no death screen,
            // health at nothing, and -- because a dead body takes no more
            // damage -- a player who could no longer be hurt by anything.
            // A player reported exactly that: lay down, felt a blow, and
            // woke immortal at zero health.
            let died = |outcome: &survival::Outcome| matches!(outcome, survival::Outcome::Died { .. });
            if died(&hunger) {
                hunger
            } else if died(&thirst) {
                thirst
            } else {
                survival::Outcome::Changed
            }
        };
        // **A body the night killed is taken out of the bed; a living one
        // is not.** Death lets go of a seat and not of a bed (a blow wakes
        // a sleeper first, so it never had to), and a night that starves
        // somebody strikes nothing -- so without this the corpse would be
        // drawn lying in the bed on every other screen until the respawn,
        // and the bed would turn the next player away.
        if matches!(outcome, survival::Outcome::Died { .. }) {
            stand_up(ctx, handle, None);
        } else {
            // No line in the chat: "you wake at first light" was English
            // on every screen. The client says it, in the player's
            // language, once the dark has lifted.
            send_asleep(handle);
            // ...and a lean-to slept in falls in with the morning.
            collapse_lean_to(ctx, handle);
        }
        send_body(handle);
        report_vitals(ctx, handle, outcome);
    }
}

/// **A lean-to slept in falls in at dawn** (`types::BLOCK_LEAN_TO`), and the
/// sleeper wakes beside a heap of what is left of it (`LEAN_TO_REMAINS`).
///
/// **At the morning the night passed, not when the sleeper gets up.** Waiting
/// for the gesture would make a lean-to that lasts as long as its sleeper
/// stays lying in it -- a player who never stands up would keep the roof for
/// the day -- and the price is the night, which has been paid by now. The
/// sleeper is stood up first, out of cells that are about to be nothing.
///
/// Rejected: *a lean-to that wears out over nights*, a count of three or
/// five. It is a number nobody could see in a heap of leaves, and "one
/// night" is a rule a player plans a trip around.
fn collapse_lean_to(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>) {
    use primitive_shared::types::{BLOCK_AIR, LEAN_TO_REMAINS};
    let Some(key) = handle.state.lock().unwrap_or_else(|e| e.into_inner()).sleeping_in else {
        return;
    };
    let Some(anchor) = ctx.world.cached_block(key.0, key.1, key.2).filter(|&b| primitive_shared::lean_to::is_lean_to(b)) else {
        return;
    };
    // All fifteen cells that are still this hut, not the bed's two: the walls
    // and the roof fall in with the bed they were over.
    let cells: Vec<(i32, i32, i32)> = primitive_shared::lean_to::cells(
        primitive_shared::lean_to::anchor(key, anchor),
        primitive_shared::types::block_facing(anchor),
    )
    .into_iter()
    .filter(|&(cell, shape)| ctx.world.cached_block(cell.0, cell.1, cell.2) == Some(shape))
    .map(|(cell, _)| cell)
    .collect();
    stand_up(ctx, handle, None);
    for &cell in &cells {
        if !ctx.world.set_block(cell.0, cell.1, cell.2, BLOCK_AIR) {
            continue;
        }
        ctx.metrics.block_edits.fetch_add(1, Ordering::Relaxed);
        broadcast_block(ctx, cell, BLOCK_AIR);
        ctx.falling.lock().unwrap_or_else(|e| e.into_inner()).on_block_changed(cell.0, cell.1, cell.2);
        notify_mechanics(ctx, cell.0, cell.1, cell.2);
    }
    let centre = (key.0 as f32 + 0.5, key.1 as f32 + 0.5, key.2 as f32 + 0.5);
    let mut items = ctx.items.lock().unwrap_or_else(|e| e.into_inner());
    for (block, count) in LEAN_TO_REMAINS {
        items.spawn(block, count, primitive_shared::geometry::wide(centre), (0.0, 0.0, 0.0), None, Instant::now());
    }
}

/// Whether the night may pass now: everybody on the server is asleep, and
/// has been for long enough that their screen is already dark.
///
/// **Everybody, and for a while.** Everybody is the old rule and is kept --
/// one sleeper on a server of five does not decide the hour for the other
/// four, and a sleeper whose friends are awake simply lies there resting,
/// on a black screen that says why the night is not passing. *For a while*
/// is the new half, and it is `body::NIGHT_PASSES_AFTER_SECONDS`: the clock
/// used to jump on the first tick everybody was in bed, which in
/// singleplayer is the tick after lying down, in front of eyes that had not
/// had time to close.
///
/// Rejected: the client asking for the night once its fade has finished.
/// It is one message fewer to trust the other way -- but a server that
/// waited for word from every client would be a server one stalled client
/// can keep in the dark for ever, and a modified one could ask on the first
/// frame. The fade's length is a rule both sides already share.
fn night_may_pass(handles: &[Arc<players::PlayerHandle>]) -> bool {
    let wait = std::time::Duration::from_secs_f32(primitive_shared::body::NIGHT_PASSES_AFTER_SECONDS);
    !handles.is_empty()
        && handles.iter().all(|h| {
            h.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .asleep_since
                .is_some_and(|since| since.elapsed() >= wait)
        })
}

/// A player lies down on a bed or a heap of straw.
///
/// **What sleep is here, and what it deliberately is not.** It is not a
/// cutscene and it is not a skip button: the player lies down, the
/// server stops taking their movement (see `ServerMessage::Asleep`),
/// and tiredness drains at `body::SLEEP_RECOVERY_PER_SECOND` while they
/// lie there. If *everybody* on the server is asleep, the clock is
/// wound forward to dawn -- and the hours it skipped are charged for:
/// hunger, thirst and the body's own drift all advance as if they had
/// been lived through, because a night that fed you would make sleeping
/// the answer to hunger.
///
/// Refused with a reason rather than in silence in three cases: standing
/// too far from the bed to be on it, being in danger, and already being
/// asleep. The middle one is the interesting one -- sleeping through a
/// wolf is not a mechanic anybody wants to discover the morning after.
fn lie_down(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    rest: primitive_shared::body::Rest,
) {
    let block = ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0);
    let key = bed_key(ctx, at, block);
    // **The same gesture gets you up again**, which is why this is a
    // toggle rather than a refusal -- and it is the same bed from either
    // end, because the state names the bed by its key and not by the half
    // that was clicked.
    {
        let already = {
            let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
            state.sleeping_in == Some(key)
        };
        if already {
            // Silently: the player pressed the key and saw themselves stand.
            // A chat line saying so was the game narrating its own
            // animation, and "не пиши всякую херню в чат" was the answer.
            stand_up(ctx, handle, None);
            return;
        }
    }

    let refusal = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.sleeping_in.is_some() {
            Some(primitive_shared::notice::Notice::AlreadyAsleep)
        } else if state.vitals.last_damage_elapsed() < HURT_RECENTLY_SECS {
            // Being hit is the one thing that must stop this, and the
            // check is "were you hit *recently*" rather than "is
            // something near you": the server would otherwise have to
            // scan for animals, and a scan that says "no" the instant
            // before a boar arrives is a scan that bought nothing.
            Some(primitive_shared::notice::Notice::HurtCannotSleep)
        } else if state.vitals.nourishment() <= 0.0 {
            // **An empty stomach or an empty waterskin keeps you up.**
            // The night's hunger and thirst are charged all at once when
            // everybody sleeps (`sleep_through_to_dawn`), and a body that
            // is already starving takes that as damage in one step: the
            // player lies down and is dead by the time the screen goes
            // dark. Refusing with a reason turns it into the decision it
            // should be -- eat first, or stay up.
            Some(primitive_shared::notice::Notice::TooHungryToSleep)
        } else if state.vitals.hydration() <= 0.0 {
            Some(primitive_shared::notice::Notice::TooThirstyToSleep)
        } else {
            None
        }
    };
    // ...and one body to a bed. Two players lying down from its two ends
    // were put in the same place, one inside the other.
    let taken = || {
        ctx.registry.handles().iter().any(|other| {
            other.id != handle.id
                && other.state.lock().unwrap_or_else(|e| e.into_inner()).sleeping_in == Some(key)
        })
    };
    let refusal = refusal.or_else(|| taken().then_some(primitive_shared::notice::Notice::BedTaken));
    if let Some(what) = refusal {
        handle.send(ServerMessage::Notice { what });
        return;
    }

    let player_yaw = handle.state.lock().unwrap_or_else(|e| e.into_inner()).yaw;
    let (place, yaw) = lying_place(ctx, &bed_cells(ctx, key), player_yaw);
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.sleeping_in = Some(key);
        // The moment the eyes close, which the night waits on: see
        // `night_may_pass`.
        state.asleep_since = Some(std::time::Instant::now());
        state.sitting_on = None;
        // On the bed rather than beside it, so a sleeper is where the
        // bed is on every other player's screen -- across the middle of
        // it, lying the way it runs (the snapshot's yaw; see
        // `protocol::Posture`).
        state.position = primitive_shared::geometry::wide(place);
        state.yaw = yaw;
        state.anticheat.reset_to(primitive_shared::geometry::wide(place));
    }
    send_asleep(handle);
    handle.send(ServerMessage::Posture {
        posture: primitive_shared::protocol::Posture::Lying,
        at: Some(primitive_shared::geometry::wide(place)),
        yaw,
    });
    let _ = rest;
}

/// Swings a door: both halves, open if they were shut and shut if they were
/// open, told to everyone who can see either.
///
/// **The client has already done it**, on its own screen, the moment the
/// player clicked (`primitive_client`'s `swing_door_locally`): a door that
/// waited a round trip to move would be a door that sticks on every server
/// further than the next room. This is the authority it predicted, and what
/// it broadcasts is what every client -- the swinger's included -- ends up
/// showing.
///
/// Only the half that is the clicked half's partner goes with it, so a lone
/// half an edit left behind swings alone rather than taking a stranger's
/// door along. Nobody standing in the doorway stops it: a door swung into a
/// player is a door the player is pushed out of the way of, which the
/// collider already does for anything that appears round a body.
pub(crate) fn swing_door(ctx: &Arc<Context>, at: (i32, i32, i32), block: primitive_shared::types::BlockId) {
    use primitive_shared::types::{door_partner, door_swung};
    let mut cells = vec![(at, door_swung(block))];
    if let Some((other, expected)) = door_partner(at, block) {
        if ctx.world.cached_block(other.0, other.1, other.2) == Some(expected) {
            cells.push((other, door_swung(expected)));
        }
    }
    for (cell, swung) in cells {
        if ctx.world.set_block(cell.0, cell.1, cell.2, swung) {
            broadcast_block(ctx, cell, swung);
            notify_mechanics(ctx, cell.0, cell.1, cell.2);
        }
    }
}

/// Tells one player how a door they could not reach really hangs, both
/// halves: the correction for a swing their client predicted and the
/// server refused. Nothing for a cell that is not a door.
fn unswing_door(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, at: (i32, i32, i32)) {
    let Some(block) = ctx.world.cached_block(at.0, at.1, at.2) else {
        return;
    };
    if !primitive_shared::types::is_door(block) {
        return;
    }
    let mut cells = vec![(at, block)];
    if let Some((other, _)) = primitive_shared::types::door_partner(at, block) {
        if let Some(there) = ctx.world.cached_block(other.0, other.1, other.2) {
            cells.push((other, there));
        }
    }
    for ((x, y, z), block_id) in cells {
        handle.send(ServerMessage::BlockUpdate(BlockChange { global_x: x, global_y: y, global_z: z, block_id }));
    }
}

/// The cell a bed is known by, whichever half was clicked: its head, or
/// the cell itself for a heap of straw or for a lone half an older save
/// left behind.
///
/// **One name per bed**, so that clicking the foot of the bed you are
/// asleep in gets you up (the toggle compares cells), a bed broken at its
/// foot is noticed by a sleeper named at its head, and two players cannot
/// lie down in one bed from its two ends.
fn bed_key(
    ctx: &Arc<Context>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) -> (i32, i32, i32) {
    // **A lean-to is known by its middle**, whichever of its fifteen cells was
    // clicked: its walls and its roof are no bed (`types::is_bed`), and a
    // sleeper named by the wall they clicked would be laid in the wall. The
    // middle is the head of the bed it is, so what follows is the bed's.
    if primitive_shared::lean_to::is_lean_to(block) {
        let anchor = primitive_shared::lean_to::anchor(at, block);
        if ctx.world.cached_block(anchor.0, anchor.1, anchor.2).is_some_and(primitive_shared::lean_to::is_anchor) {
            return anchor;
        }
    }
    match primitive_shared::types::bed_partner(at, block) {
        Some((head, expected))
            if !primitive_shared::types::is_bed_head(block)
                && ctx.world.cached_block(head.0, head.1, head.2) == Some(expected) =>
        {
            head
        }
        _ => at,
    }
}

/// The cells of the bed a key names, head first: two for a whole bed,
/// one for anything else.
fn bed_cells(ctx: &Arc<Context>, key: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
    let block = ctx.world.cached_block(key.0, key.1, key.2).unwrap_or(0);
    match primitive_shared::types::bed_partner(key, block) {
        Some((foot, expected))
            if primitive_shared::types::is_bed_head(block)
                && ctx.world.cached_block(foot.0, foot.1, foot.2) == Some(expected) =>
        {
            vec![key, foot]
        }
        _ => vec![key],
    }
}

/// Where a body lies on a bed, and which way its head points.
///
/// Across the seam of a whole bed, head toward the head half: the body is
/// two cells long and lies in both. On one cell -- a lone half an older
/// save left behind, a pallet of straw from before it was two cells among
/// them -- the middle of it, head behind the way it faces. Anything else a
/// body can lie on is laid the way the player was facing, turned to the
/// nearest side, because a body lying across a diagonal of its cell would
/// hang out of two corners.
fn lying_place(
    ctx: &Arc<Context>,
    cells: &[(i32, i32, i32)],
    player_yaw: f32,
) -> ((f32, f32, f32), f32) {
    let key = cells[0];
    let block = ctx.world.cached_block(key.0, key.1, key.2).unwrap_or(0);
    let count = cells.len() as f32;
    let (sx, sz) = cells
        .iter()
        .fold((0.0, 0.0), |(x, z), c| (x + c.0 as f32 + 0.5, z + c.2 as f32 + 0.5));
    // On the bed's top -- which in a lean-to is the bed of leaves inside it,
    // not the top of its cell: the cell is as tall as the thatch over the
    // bed (`types::collision_height`), and a sleeper put there lay on the
    // roof.
    let lies_on = if primitive_shared::lean_to::is_lean_to(block) {
        primitive_shared::lean_to::BED_TOP
    } else {
        primitive_shared::types::collision_height(block)
    };
    let place = (sx / count, key.1 as f32 + lies_on, sz / count);
    let yaw = if let [head, foot] = cells {
        ((head.2 - foot.2) as f32).atan2((head.0 - foot.0) as f32)
    } else if primitive_shared::types::is_bed(block) {
        // A lone half: its head is behind the way it faces.
        let (dx, dz) = primitive_shared::types::block_facing(block).step();
        (-dz as f32).atan2(-dx as f32)
    } else {
        (player_yaw / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2
    };
    (place, yaw)
}

/// A player sits down on a stool.
///
/// **On it, and seen on it.** Sitting used to set a claim and say "you sit
/// down" in the chat, and nothing else happened: the player stood where
/// they had clicked from, a stride away, their camera at standing height,
/// and every other player saw them standing there too. The body goes onto
/// the seat now -- a seat is the top of the stool's collider, so the feet
/// rest on something -- and the client is told, so the eye comes down
/// (`ServerMessage::Posture`).
///
/// Refused where a seated body would not fit (a stool under a low shelf),
/// because putting the collider there would be putting a player inside a
/// block. Sitting is still not a lock: walking away gets you up, and so
/// does the same click.
///
/// **A chair is sat in facing the way it faces**, and that is the one
/// thing it does a stool does not. The server turns the body to the chair
/// (`types::seat_yaw`) and remembers the turn (`PlayerRuntime::seat_facing`),
/// because a sitter's own transforms keep arriving while they look round:
/// the snapshot's yaw would otherwise be wherever their camera points, and
/// every other screen would draw a figure swivelling on a chair with a
/// back. The body is also set back from the middle of the cell toward the
/// back (`CHAIR_SEAT_BACK`), so the figure leans on it rather than
/// perching a hand's width in front of it.
///
/// **One body to a seat.** The stool never asked, and two players clicking
/// one were put in the same place, one inside the other -- the bed's bug,
/// which `lie_down` answered long ago with the same question.
fn sit_down(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    /// How far a chair's sitter is set back from the middle of its cell,
    /// in blocks: a sixteenth, which puts the back of the figure's torso
    /// (two model units deep either side of its middle) against the front
    /// of the uprights at four and three quarter sixteenths from the back
    /// edge (`mesh::CHAIR`). The collider still fits: it reaches three
    /// tenths either side, and the back is not collided with.
    const CHAIR_SEAT_BACK: f32 = 1.0 / 16.0;
    let facing = primitive_shared::types::seat_yaw(block);
    let (back_x, back_z) = facing.map_or((0.0, 0.0), |yaw| {
        (-yaw.cos() * CHAIR_SEAT_BACK, -yaw.sin() * CHAIR_SEAT_BACK)
    });
    let seat = (
        at.0 as f32 + 0.5 + back_x,
        at.1 as f32 + primitive_shared::types::collision_height(block),
        at.2 as f32 + 0.5 + back_z,
    );
    let (already, asleep, looking) = {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.sitting_on == Some(at), state.sleeping_in.is_some(), state.yaw)
    };
    if already {
        stand_up(ctx, handle, None);
        return;
    }
    if asleep {
        return; // a sleeper's clicks belong to the bed they are in
    }
    let taken = ctx.registry.handles().iter().any(|other| {
        other.id != handle.id
            && other.state.lock().unwrap_or_else(|e| e.into_inner()).sitting_on == Some(at)
    });
    if taken {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::SeatTaken });
        return;
    }
    if !body_fits(ctx, seat) {
        handle.send(ServerMessage::Notice { what: primitive_shared::notice::Notice::NoRoomToSit });
        return;
    }
    let yaw = facing.unwrap_or(looking);
    {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.sitting_on = Some(at);
        // Written every time a seat is taken, `None` for a stool, so a
        // chair's turn never outlives the chair into the next seat.
        state.seat_facing = facing;
        state.position = primitive_shared::geometry::wide(seat);
        if let Some(yaw) = facing {
            state.yaw = yaw;
        }
        state.anticheat.reset_to(primitive_shared::geometry::wide(seat));
    }
    handle.send(ServerMessage::Posture {
        posture: primitive_shared::protocol::Posture::Sitting,
        at: Some(primitive_shared::geometry::wide(seat)),
        yaw,
    });
    // **No "you sit down" in the chat.** The posture is the answer; a line
    // repeating what the screen already shows pushed the lines that
    // mattered -- who arrived, what was refused -- up and out of sight.
    // Refusals still speak (`ServerMessage::Error`), because a refusal is
    // the one thing the screen cannot show.
}

/// Gets a player off whatever they are lying or sitting on, whether they
/// asked to or not.
///
/// One place, because there are six ways it happens -- they asked, they
/// walked off, the bed or the stool was broken, morning came, something
/// hurt them -- and every one of them has to do the same things: clear the
/// state, tell the client (which may not be predicting), and put the body
/// somewhere it can stand. That last one is new, and it is the bug: waking
/// left the collider where the server had laid it, on the mattress, so a
/// bed against a wall woke its sleeper with the headboard through their
/// shoulders, and a player woken by a broken bed was standing in the air
/// over the hole. See `standing_place`. Getting off a stool leaves the body
/// where it is: it is already standing on something, the seat or the floor
/// it walked to.
pub(crate) fn stand_up(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, why: Option<&str>) {
    let (bed, seat, rowing, position) = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        // Out of bed is awake, however they got out of it.
        state.asleep_since = None;
        (state.sleeping_in.take(), state.sitting_on.take(), state.rowing.take(), state.position)
    };
    // **Getting up from the oars** is getting up from a seat: the body stays
    // on the deck where it sat (`aboard` is left alone), and the raft has
    // nobody pulling.
    if rowing.is_some() {
        rafts::forget(ctx, handle.id);
        handle.send(ServerMessage::Oars { raft: None });
    }
    // ...and getting up off a horse is getting off it.
    if handle.state.lock().unwrap_or_else(|e| e.into_inner()).riding.is_some() {
        horses::dismount(ctx, handle, why);
        return;
    }
    if bed.is_none() && seat.is_none() && rowing.is_none() {
        return;
    }
    let at = bed.map(|key| {
        let place = standing_place(ctx, &bed_cells(ctx, key), primitive_shared::geometry::narrow(position));
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        state.position = primitive_shared::geometry::wide(place);
        state.anticheat.reset_to(primitive_shared::geometry::wide(place));
        place
    });
    send_asleep(handle);
    handle.send(ServerMessage::Posture {
        posture: primitive_shared::protocol::Posture::Standing,
        at: at.map(primitive_shared::geometry::wide),
        yaw: 0.0,
    });
    if let Some(text) = why {
        handle.send(ServerMessage::Chat {
            from: None,
            username: "server".to_string(),
            text: text.to_string(),
        });
    }
}

/// Whether a player's whole collider fits with its feet at `feet`: nothing
/// solid anywhere inside it, and every cell it reaches loaded. Touching is
/// fitting -- feet resting exactly on a seat are not inside the seat.
fn body_fits(ctx: &Arc<Context>, feet: (f32, f32, f32)) -> bool {
    use primitive_shared::geometry::{for_each_block_box, BOX_OVERHANG, PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
    const TOUCH: f32 = 1e-3;
    let min = [feet.0 - PLAYER_HALF_WIDTH, feet.1, feet.2 - PLAYER_HALF_WIDTH];
    let max = [feet.0 + PLAYER_HALF_WIDTH, feet.1 + PLAYER_HEIGHT, feet.2 + PLAYER_HALF_WIDTH];
    // **The cells round the body as well as under it**: a leaning palm's bark
    // stands out of the piece it grew in by up to `BOX_OVERHANG`, and the
    // client's collider looks that far (`physics::for_each_solid`). Those
    // cells need not be loaded; the ones the body is in still must.
    let inner = |x: i32, z: i32| {
        (min[0].floor() as i32..=max[0].floor() as i32).contains(&x)
            && (min[2].floor() as i32..=max[2].floor() as i32).contains(&z)
    };
    for x in (min[0] - BOX_OVERHANG).floor() as i32..=(max[0] + BOX_OVERHANG).floor() as i32 {
        for y in min[1].floor() as i32..=max[1].floor() as i32 {
            for z in (min[2] - BOX_OVERHANG).floor() as i32..=(max[2] + BOX_OVERHANG).floor() as i32 {
                let Some(block) = ctx.world.cached_block(x, y, z) else {
                    if inner(x, z) {
                        return false;
                    }
                    continue;
                };
                let mut inside = false;
                let near = |dx: i32, dy: i32, dz: i32| {
                    ctx.world.cached_block(x + dx, y + dy, z + dz).unwrap_or(primitive_shared::types::BLOCK_AIR)
                };
                for_each_block_box(block, x, y, z, near, |lo, hi| {
                    inside |= (0..3).all(|a| lo[a] < max[a] - TOUCH && hi[a] > min[a] + TOUCH);
                });
                if inside {
                    return false;
                }
            }
        }
    }
    true
}

/// Where a player getting out of a bed stands.
///
/// **Beside it, on the floor, and never inside anything.** The cells round
/// the bed are tried in turn -- beside it before past its ends, because
/// that is the way out of a bed -- for one with a whole floor under it and
/// room for a body; if none has, the top of the bed, which always has a
/// floor and is where they already were.
fn standing_place(
    ctx: &Arc<Context>,
    cells: &[(i32, i32, i32)],
    fallback: (f32, f32, f32),
) -> (f32, f32, f32) {
    let (ax, az) = match cells {
        [head, foot] => (head.0 - foot.0, head.2 - foot.2),
        _ => (1, 0),
    };
    let around = [(-az, ax), (az, -ax), (ax, az), (-ax, -az)];
    for &(cx, cy, cz) in cells {
        for (dx, dz) in around {
            let (x, z) = (cx + dx, cz + dz);
            if cells.contains(&(x, cy, z)) {
                continue;
            }
            let floor = ctx
                .world
                .cached_block(x, cy - 1, z)
                .map_or(0.0, primitive_shared::types::collision_height);
            let feet = (x as f32 + 0.5, cy as f32, z as f32 + 0.5);
            if floor >= 1.0 && body_fits(ctx, feet) {
                return feet;
            }
        }
    }
    fallback
}

/// Takes the other half of a bed away once one half has gone, without a
/// second drop.
///
/// **One bed, one item, whichever half went first.** A bed is placed as
/// two cells from one item (see `types::BED_HEAD`), so breaking it gives
/// one item back -- the drop of the half that was broken -- and must not
/// leave the other half standing: half a bed is a pillow on the floor that
/// sleeps as well as a whole one.
///
/// Called from every path a cell stops being a bed on: a player's break,
/// the collapse of something whose floor went, a mod's `break_block`. It
/// takes the partner only if it still is the matching half, so a lone half
/// from an older save, or a cell somebody has already built over, is left
/// alone.
///
/// **And a door's other half** (`types::door_partner`), open or shut as the
/// half that went was: a door broken at its top gives one door, and does not
/// leave its bottom half standing as a door a cell tall.
///
/// **And a tall plant's other half, on the same terms** (`types::plant_partner`):
/// a nettle pulled up by its top gives its fibre once and takes its stalk with
/// it, and one cut at the foot does not leave its upper half standing on air
/// for the collapse pass to pay out a second time.
pub(crate) fn break_bed_partner(
    ctx: &Arc<Context>,
    at: (i32, i32, i32),
    removed: primitive_shared::types::BlockId,
) {
    // **A rack comes down whole, all four cells** (`types::rack_partners`):
    // any one of them broken is the frame taken apart, and a cell left
    // standing would be a rack that is a rack to the eye and to nothing
    // else. The goods bits are left out of the match: what hangs on a
    // column does not decide whether a cell is part of this rack.
    // ...and a lean-to all fifteen, on the same terms: one cell taken is the
    // hut taken apart, and the one item it gave is the hut
    // (`lean_to::partners`). Only the cells that still are this hut's go.
    if primitive_shared::lean_to::is_lean_to(removed) {
        for (cell, shape) in primitive_shared::lean_to::partners(at, removed) {
            if ctx.world.cached_block(cell.0, cell.1, cell.2) != Some(shape) {
                continue;
            }
            if !ctx.world.set_block(cell.0, cell.1, cell.2, primitive_shared::types::BLOCK_AIR) {
                continue;
            }
            broadcast_block(ctx, cell, primitive_shared::types::BLOCK_AIR);
            {
                let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                sim.on_block_changed(cell.0, cell.1, cell.2);
            }
            notify_mechanics(ctx, cell.0, cell.1, cell.2);
        }
        return;
    }
    if primitive_shared::rack::is_rack(removed) {
        for (cell, shape) in primitive_shared::types::rack_partners(at, removed) {
            let there = ctx.world.cached_block(cell.0, cell.1, cell.2);
            if there.map(primitive_shared::types::rack_shape) != Some(shape) {
                continue;
            }
            if !ctx.world.set_block(cell.0, cell.1, cell.2, primitive_shared::types::BLOCK_AIR) {
                continue;
            }
            broadcast_block(ctx, cell, primitive_shared::types::BLOCK_AIR);
            {
                let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
                sim.on_block_changed(cell.0, cell.1, cell.2);
            }
            notify_mechanics(ctx, cell.0, cell.1, cell.2);
        }
        return;
    }
    let Some((partner, expected)) = primitive_shared::types::bed_partner(at, removed)
        .or_else(|| primitive_shared::types::plant_partner(at, removed))
        .or_else(|| primitive_shared::wildfire::standing_torch_partner(at, removed))
        .or_else(|| primitive_shared::types::door_partner(at, removed))
    else {
        return;
    };
    let there = ctx.world.cached_block(partner.0, partner.1, partner.2);
    // A standing torch's top is its pole's whether it is burning or burnt
    // out, so the match there is by what the top is, not by the one id
    // `standing_torch_partner` writes on placement.
    let matches = there == Some(expected)
        || (primitive_shared::wildfire::is_standing_torch_top(expected)
            && there.is_some_and(primitive_shared::wildfire::is_standing_torch_top));
    if !matches {
        return;
    }
    if !ctx
        .world
        .set_block(partner.0, partner.1, partner.2, primitive_shared::types::BLOCK_AIR)
    {
        return;
    }
    broadcast_block(ctx, partner, primitive_shared::types::BLOCK_AIR);
    {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.on_block_changed(partner.0, partner.1, partner.2);
    }
    notify_mechanics(ctx, partner.0, partner.1, partner.2);
}

/// How much health a nettle takes from the hand that pulls it up, out of
/// `survival::MAX_HEALTH`.
///
/// **One, a twentieth**: enough that a player who clears a riverbank of
/// nettles by hand has paid for the cord with a real share of their health,
/// and little enough that pulling one in an emergency is still a choice.
pub(crate) const NETTLE_STING: f32 = 1.0;

/// Stings the player who pulled up a nettle with no knife in hand
/// (`types::nettle_stings`).
///
/// **Never the last of a player's health.** A sting that killed would be a
/// death by gardening, and a death screen for pulling a weed teaches nothing
/// but to distrust the world; at two health or less the nettle is pulled and
/// nothing happens, which a player that low has more pressing things to learn
/// from.
/// A nettle cut with a blade: one strip of bast where the stalk stood, in
/// place of the handful of fibre the drop table gives. Answers whether it
/// did, so the break path knows not to drop the fibre as well.
///
/// One strip for the whole plant -- the other half of a tall nettle goes
/// with it giving nothing (`break_bed_partner`), as it always has.
pub(crate) fn strip_nettle(
    ctx: &Arc<Context>,
    broken: primitive_shared::types::BlockId,
    held: Option<primitive_shared::types::BlockId>,
    at: (i32, i32, i32),
) -> bool {
    if !primitive_shared::types::strips_bast(broken, held) {
        return false;
    }
    ctx.items.lock().unwrap_or_else(|e| e.into_inner()).spawn(
        primitive_shared::types::BLOCK_NETTLE_BAST,
        1,
        (f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5),
        (0.0, 0.0, 0.0),
        None,
        Instant::now(),
    );
    true
}

pub(crate) fn sting_from_nettle(
    handle: &Arc<players::PlayerHandle>,
    broken: primitive_shared::types::BlockId,
    held: Option<primitive_shared::types::BlockId>,
) {
    if !primitive_shared::types::nettle_stings(broken, held) {
        return;
    }
    let stung = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() || state.vitals.health() <= NETTLE_STING * 2.0 {
            false
        } else {
            !matches!(state.vitals.hurt(NETTLE_STING, "was stung by nettles"), survival::Outcome::Unchanged)
        }
    };
    if stung {
        send_health(handle);
    }
}

/// Health a raid on a hive never takes a player below, stings and venom
/// together, out of `survival::MAX_HEALTH`.
///
/// **The nettle's floor, for the nettle's reason** (`sting_from_nettle`): a
/// death by honey teaches nothing but to distrust the woods. A player this
/// low is stung for nothing, and still gets the honey.
pub(crate) const BEE_STING_FLOOR: f32 = 2.0;

/// Is anything smoking the hive at `hive`: a lit torch in the hand that took
/// the comb, something burning within `bees::SMOKE_REACH` of it, or a room
/// full of smoke round it?
///
/// **The hand first**, because it is the gesture: a player who has learnt
/// what calms bees walks up with a torch. The fire under the tree is the
/// other way -- slower to set up, and it does not need a hand free. A box of
/// seven by seven, from three under the hive to three over, is a few hundred
/// reads, once per raid.
pub(crate) fn hive_is_smoked(
    ctx: &Arc<Context>,
    hive: (i32, i32, i32),
    held: Option<primitive_shared::types::BlockId>,
) -> bool {
    use primitive_shared::bees::{smokes_bees, SMOKE_REACH};
    if held.is_some_and(primitive_shared::types::is_lit_torch) {
        return true;
    }
    for dy in -SMOKE_REACH..=SMOKE_REACH {
        for dz in -SMOKE_REACH..=SMOKE_REACH {
            for dx in -SMOKE_REACH..=SMOKE_REACH {
                if ctx
                    .world
                    .cached_block(hive.0 + dx, hive.1 + dy, hive.2 + dz)
                    .is_some_and(smokes_bees)
                {
                    return true;
                }
            }
        }
    }
    ctx.wildfire.lock().unwrap_or_else(|e| e.into_inner()).smoke_at(hive) > 0.0
}

/// Stings the player who broke into a wild hive: `bees::stings` of them, for
/// the air at the hive and whether it was smoked (`hive_is_smoked`).
///
/// **Only the raider.** Everybody standing near was weighed, and it makes a
/// friend's raid a thing that happens *to* you, with nothing you did to
/// decide it; the bees go for the hands in the comb.
pub(crate) fn sting_from_bees(
    ctx: &Arc<Context>,
    handle: &Arc<players::PlayerHandle>,
    broken: primitive_shared::types::BlockId,
    at: (i32, i32, i32),
    held: Option<primitive_shared::types::BlockId>,
) {
    if !primitive_shared::bees::is_hive(broken) {
        return;
    }
    let weather = ctx.sky.lock().unwrap_or_else(|e| e.into_inner()).weather();
    let air = water_c_at(ctx, at, weather);
    let count = primitive_shared::bees::stings(broken, hive_is_smoked(ctx, at, held), air);
    if sting_player(handle, count) {
        send_health(handle);
    }
}

/// `count` stings on one player: the health at once, the venom on the
/// illness clock. Says whether anything was taken, for the health message.
///
/// **Held above [`BEE_STING_FLOOR`], venom included.** The stings stop at the
/// floor, and the venom is cut to what the illness clock could take before
/// reaching it -- so no raid ends in a death some seconds later, on the walk
/// away from the tree.
pub(crate) fn sting_player(handle: &Arc<players::PlayerHandle>, count: u8) -> bool {
    use primitive_shared::bees::{STING_HEALTH, VENOM_SECONDS};
    let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
    let mut stung = false;
    for _ in 0..count {
        if state.vitals.is_dead() || state.vitals.health() - STING_HEALTH <= BEE_STING_FLOOR {
            break;
        }
        stung |= !matches!(state.vitals.hurt(STING_HEALTH, "was stung to death by bees"), survival::Outcome::Unchanged);
        // What the illness already owed is counted against the floor first:
        // a player sick from a pond is nearer it than their health says.
        let owed = state.vitals.illness_owed() * primitive_shared::body::SICKNESS_PER_SECOND;
        let room = (state.vitals.health() - BEE_STING_FLOOR - owed).max(0.0);
        let venom = VENOM_SECONDS.min(room / primitive_shared::body::SICKNESS_PER_SECOND);
        state.vitals.envenom(venom);
    }
    stung
}

/// Tells a client whether it is asleep, if that has changed since last
/// time. The same on-change contract `send_body` has, and for the same
/// reason: this is a state, not an event.
///
/// **Eyes closed, not in bed** (`asleep_since`, not `sleeping_in`): the
/// client lifts its dark screen on the change, and a player the morning
/// woke is still lying in the bed.
fn send_asleep(handle: &Arc<players::PlayerHandle>) {
    let asleep = {
        let mut state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        let asleep = state.asleep_since.is_some();
        if state.asleep_reported == asleep {
            return;
        }
        state.asleep_reported = asleep;
        asleep
    };
    handle.send(ServerMessage::Asleep { asleep });
}

/// How recently a blow counts as "something is hurting you", in seconds.
///
/// Ten. Long enough that a player cannot lie down between a boar's
/// charges, short enough that a fall on the way home does not cost them
/// the night.
const HURT_RECENTLY_SECS: f32 = 10.0;

/// The world's own answer to "how good is the ground here, and how warm
/// is the air over it".
///
/// A struct rather than an `impl Soil for World`, because the trait
/// belongs to the growth mechanic and the world belongs to the server:
/// tying the two together directly would mean the mechanic could not be
/// tested without a generator, which is the thing the trait exists to
/// avoid. See `logic::growth::Soil`.
///
/// **The air is a player's air.** `climate::Ambient::of` is what a person
/// standing in that cell would be sampled at -- the biome's field with the
/// altitude in it, the season, the hour, the rain, a roof, a fire -- and a
/// crop reading anything else would be a second weather system that
/// disagreed with the first where a player can see both: a field that
/// froze on a night their own skin called mild.
struct WorldSoil<'a> {
    world: &'a Arc<World>,
    fires: &'a crate::logic::fire::Fires,
    world_days: f32,
    weather: primitive_shared::weather::Weather,
}

impl logic::growth::Soil for WorldSoil<'_> {
    fn growth_factor(&self, gx: i32, gz: i32) -> f32 {
        self.world.fertility_at(gx, gz).growth_factor()
    }

    fn air_c(&self, gx: i32, gy: i32, gz: i32) -> Option<f32> {
        // Asked of the cache first. `Ambient::of` answers a cell nobody
        // has loaded with a comfortable neutral, which is right for a
        // player a tick after a teleport and wrong for a crop: a field
        // nobody has loaded is not having a mild night, and a guess here
        // is a guess that decides whether it freezes.
        self.world.cached_block(gx, gy, gz)?;
        let centre = (gx as f32 + 0.5, gy as f32 + 0.5, gz as f32 + 0.5);
        Some(
            crate::logic::climate::Ambient::of(
                self.world,
                self.fires,
                centre,
                self.world_days,
                self.weather,
            )
            .temperature_c,
        )
    }
}

/// How long the same news about a field waits before it is said again,
/// in seconds. A minute: longer than tilling one field takes, shorter
/// than walking to the next one and back.
const FIELD_NOTE_REPEAT_SECS: f32 = 60.0;

/// What turning a spadeful of earth tells a farmer: the soil, when it is
/// worth naming, and whether water is close enough to keep it wet.
///
/// One line rather than two, so the two facts arrive together and the
/// repeat rule is one comparison. The soil keeps the words it always had,
/// because a player who has read "this is rich soil" a hundred times
/// reads it at a glance. Ordinary soil is still said by saying nothing
/// about it -- most of the world is ordinary.
fn field_note(soil: primitive_shared::worldgen::Fertility, watered: bool) -> &'static str {
    use primitive_shared::worldgen::Fertility;
    match (soil, watered) {
        (Fertility::Rich, true) => "this is rich soil, and water is close by",
        (Fertility::Rich, false) => "this is rich soil, but there is no water close by",
        (Fertility::Poor, true) => "this soil is thin, but water is close by",
        (Fertility::Poor, false) => "this soil is thin, and there is no water close by",
        (Fertility::Ordinary, true) => "water is close by",
        (Fertility::Ordinary, false) => "there is no water close by",
    }
}

/// A fire went out -- burnt through, put out, or broken.
pub(crate) fn fire_died(ctx: &Arc<Context>, at: (i32, i32, i32)) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            pos: cell(at),
            block: ctx.world.cached_block(at.0, at.1, at.2).unwrap_or(0),
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::FireDied, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, at);
}

/// Anything at all changed a cell.
///
/// **The one event that is fired from four places rather than one**, and
/// they are the four places a block change is announced to clients:
/// `broadcast_block`, `broadcast_changes`, the player's own edit, and
/// the mod API's. Routing them all through the two `broadcast_*`
/// functions is what keeps that four down to two -- see those.
pub(crate) fn block_changed(
    ctx: &Arc<Context>,
    at: (i32, i32, i32),
    block: primitive_shared::types::BlockId,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            pos: cell(at),
            block,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::BlockChanged, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, at, block);
}

/// A craft is about to run. `false` means a mod refused, which is how a
/// recipe is taken out of the game.
pub(crate) fn item_crafted(
    ctx: &Arc<Context>,
    player: PlayerId,
    index: usize,
    block: primitive_shared::types::BlockId,
    count: u32,
) -> bool {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            count,
            value: index as i64,
            ..Default::default()
        };
        notify_native(ctx, primitive_modapi::Event::ItemCrafted, data)
    }
    #[cfg(not(feature = "mods"))]
    {
        let _ = (ctx, player, index, block, count);
        true
    }
}

/// A garment went on or came off.
pub(crate) fn equipment_changed(
    ctx: &Arc<Context>,
    player: PlayerId,
    block: primitive_shared::types::BlockId,
    slot: primitive_shared::equipment::Slot,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            player,
            block,
            slot: slot as u32,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::EquipmentChanged, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, player, block, slot);
}

/// The sky changed its mind.
pub(crate) fn weather_changed(
    ctx: &Arc<Context>,
    weather: primitive_shared::weather::Weather,
) {
    #[cfg(feature = "mods")]
    {
        use primitive_shared::weather::Weather as W;
        // The contract's own discriminant rather than the game's: an
        // enum's numbering is not a stable thing to promise across a
        // compiler version, which is the whole reason
        // `primitive_modapi::Weather` exists as a separate type.
        let data = primitive_modapi::EventData {
            value: match weather {
                W::Clear => primitive_modapi::Weather::Clear,
                W::Rain => primitive_modapi::Weather::Rain,
                W::Storm => primitive_modapi::Weather::Storm,
            } as i64,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::WeatherChanged, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, weather);
}

/// Somebody moved the sun. Not fired for time merely passing -- see
/// `Event::TimeChanged`.
pub(crate) fn time_changed(ctx: &Arc<Context>, time_of_day: f32) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            float: time_of_day,
            ..Default::default()
        };
        let _ = notify_native(ctx, primitive_modapi::Event::TimeChanged, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, time_of_day);
}

/// Something that is not a player appeared or went away.
pub(crate) fn entity_appeared(
    ctx: &Arc<Context>,
    entity: u64,
    at: (f32, f32, f32),
    spawned: bool,
) {
    #[cfg(feature = "mods")]
    {
        let data = primitive_modapi::EventData {
            entity,
            pos: cell((
                at.0.floor() as i32,
                at.1.floor() as i32,
                at.2.floor() as i32,
            )),
            ..Default::default()
        };
        let event = if spawned {
            primitive_modapi::Event::EntitySpawned
        } else {
            primitive_modapi::Event::EntityRemoved
        };
        let _ = notify_native(ctx, event, data);
    }
    #[cfg(not(feature = "mods"))]
    let _ = (ctx, entity, at, spawned);
}

/// Gives the world a way to reach the loaded mods when it makes a chunk.
///
/// **This is what makes `GenerationApi::register_decorator` a feature
/// rather than a list.** A mod could register a decorator from the first
/// version of the API and the host wrote it down and never called it:
/// terrain is made by `World::generate`, in a pool of worker threads
/// that hold an `Arc<World>` and nothing else, and there was no path
/// from there to the mod host at all.
///
/// The path is a closure, and it holds a **`Weak`** rather than an
/// `Arc`. The world is owned by the context, so a strong reference here
/// would be the context owning something that owns the context -- and
/// the leak that follows is not academic: the client starts and stops
/// one of these per singleplayer world, so a cycle means every world a
/// player visits stays in memory until the game is closed.
///
/// The upgrade failing is the ordinary shutdown case, not an error: the
/// generator pool is joined after the context is dropped, so a chunk
/// half-made at that moment simply arrives undecorated.
#[cfg(feature = "mods")]
fn install_chunk_decorators(ctx: &Arc<Context>) {
    let weak = Arc::downgrade(ctx);
    let world = Arc::clone(&ctx.world);
    world.set_decorator(Box::new(move |pos, seed, blocks| {
        let Some(ctx) = weak.upgrade() else { return };
        let pos = primitive_modapi::ChunkPos { x: pos.x, z: pos.z };
        mods::decorate(&ctx, pos, seed, blocks);
        // ...and then the event, from the same thread and after the
        // decorators, so a mod that only wants to *know* a chunk was
        // made does not have to register a decorator that does nothing.
        // Fired here rather than where the chunk is cached, because this
        // is the one place every chunk in the world passes through --
        // the pool, the fallback in `ChunkService::take`, and a reload
        // after eviction all end up in `World::generate`.
        let data = primitive_modapi::EventData {
            pos: primitive_modapi::BlockPos {
                x: pos.x,
                y: 0,
                z: pos.z,
            },
            ..Default::default()
        };
        let _ = notify_mods!(&ctx, primitive_modapi::Event::ChunkGenerated, data);
    }));
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
            (state.x as f32, state.y as f32, state.z as f32),
        ));
    }
    for (x, y, z) in blocks {
        if let Some(id) = ctx.world.cached_block(x, y, z) {
            view.blocks.insert((x, y, z), id);
        }
    }
    view
}

/// Everything an event is, in the vocabulary the mod API uses.
///
/// The bridge between the server's own hook names -- which are strings,
/// because that is what a Rhai script matches on -- and
/// `primitive_modapi::Event`, which is an integer because that is what
/// survives a C ABI. One function, so the day a hook is added there is
/// one place that has to learn about it rather than two.
///
/// `None` for a hook no native event corresponds to, which is not a
/// failure: the two extension points do not have to cover exactly the
/// same ground, and a plugin-only hook is simply one no mod hears.
#[cfg(feature = "mods")]
fn native_event(hook: &str) -> Option<primitive_modapi::Event> {
    use primitive_modapi::Event;
    Some(match hook {
        "on_load" => Event::ServerStarted,
        "on_tick" => Event::Tick,
        "on_join" => Event::PlayerJoined,
        "on_leave" => Event::PlayerLeft,
        "on_chat" => Event::PlayerChat,
        "on_death" => Event::PlayerDied,
        "on_block_place" => Event::BlockPlace,
        "on_block_break" => Event::BlockBreak,
        "on_command" => Event::Command,
        "on_craft" => Event::ItemCrafted,
        "on_hide_cured" => Event::HideCured,
        "on_drink" => Event::PlayerDrank,
        "on_equip" => Event::EquipmentChanged,
        "on_weather" => Event::WeatherChanged,
        _ => return None,
    })
}

/// Turns a hook's arguments into the flat payload a native mod reads.
///
/// The argument lists are positional and were designed for scripts, so
/// this is where the positions are given names. It is deliberately
/// forgiving: a hook called with fewer arguments than a variant usually
/// carries leaves the rest at zero rather than refusing, because the
/// alternative is that adding an argument to a plugin hook silently
/// breaks every mod listening to it.
#[cfg(feature = "mods")]
/// A hook's list-valued argument, joined, so a native mod can be handed
/// one.
///
/// **Built by the caller and lent in**, which is the whole reason it is
/// a separate function. `Str` is a pointer and a length that must stay
/// valid for the length of the call; a `String` built inside
/// `native_payload` would be dropped as that function returned, and the
/// mod would read freed memory.
///
/// It was left out entirely rather than solved, and the cost was
/// invisible: `EventData::args` was documented as "space-joined" and was
/// *always empty* for every mod on every server. A mod adding `/home`
/// got the command and never its arguments -- see `mods/flight`, whose
/// `/fly 20` could not have worked outside its own test.
#[cfg(feature = "mods")]
fn native_args(args: &[plugins::Value]) -> String {
    args.iter()
        .find_map(|value| match value {
            plugins::Value::List(items) => Some(items),
            _ => None,
        })
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    plugins::Value::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<&str>>()
                .join(" ")
        })
        .unwrap_or_default()
}

#[cfg(feature = "mods")]
fn native_payload(
    event: primitive_modapi::Event,
    args: &[plugins::Value],
    blocks: &[(i32, i32, i32)],
    joined: &str,
) -> primitive_modapi::EventData {
    use primitive_modapi::{BlockPos, Event, EventData, Str};

    let int = |n: usize| match args.get(n) {
        Some(plugins::Value::Int(v)) => *v,
        _ => 0,
    };
    let text = |n: usize| match args.get(n) {
        Some(plugins::Value::Text(v)) => Str::borrow(v.as_str()),
        _ => Str::EMPTY,
    };
    let mut data = EventData::default();
    if let Some(&(x, y, z)) = blocks.first() {
        data.pos = BlockPos { x, y, z };
    }
    match event {
        Event::Tick => data.tick = int(0).max(0) as u64,
        Event::PlayerDrank => {
            data.player = int(0).max(0) as u64;
        }
        // **The name comes with them, and it used to be dropped here.**
        //
        // `Event::PlayerJoined` has said "`player`, `text`" since 1.0,
        // the hook has been fired with the username as its second
        // argument since 1.0, and this arm read only the first -- so
        // every mod that greeted somebody by name greeted an empty
        // string. Nothing failed, nothing was logged, and it stayed
        // that way until an example mod written to try the boundary
        // said hello to nobody. That is the whole argument for having
        // examples on the far side of an API.
        Event::PlayerJoined | Event::PlayerLeft | Event::PlayerChat | Event::PlayerDied => {
            data.player = int(0).max(0) as u64;
            data.text = text(1);
        }
        Event::BlockPlace => {
            data.player = int(0).max(0) as u64;
            data.pos = BlockPos {
                x: int(1) as i32,
                y: int(2) as i32,
                z: int(3) as i32,
            };
            data.block = int(4).clamp(0, u16::MAX as i64) as u16;
        }
        Event::BlockBreak => {
            data.player = int(0).max(0) as u64;
            data.pos = BlockPos {
                x: int(1) as i32,
                y: int(2) as i32,
                z: int(3) as i32,
            };
        }
        Event::HideCured => {
            data.pos = BlockPos {
                x: int(0) as i32,
                y: int(1) as i32,
                z: int(2) as i32,
            };
        }
        Event::ItemCrafted | Event::EquipmentChanged => {
            data.player = int(0).max(0) as u64;
            data.block = int(1).clamp(0, u16::MAX as i64) as u16;
            data.count = int(2).clamp(0, u32::MAX as i64) as u32;
        }
        Event::Command => {
            data.player = int(0).max(0) as u64;
            data.text = text(1);
            data.args = Str::borrow(joined);
        }
        Event::WeatherChanged => data.value = int(0),
        _ => {}
    }
    data
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
    let cells = blocks.unwrap_or_default();

    // **The native half, first and separately.** Both extension points
    // see every event, in a fixed order, and a veto from either is a
    // veto -- which is what makes a protection mod and a protection
    // plugin compose rather than race.
    //
    // Fired before the scripts rather than after for no reason beyond
    // needing to be one or the other; what matters is that it is the
    // same order on every server.
    #[allow(unused_mut)] // only the `mods` build ever assigns to it
    let mut allowed_by_mods = true;
    #[cfg(feature = "mods")]
    if let Some(event) = native_event(hook) {
        // Joined out here so it outlives the `Str` that borrows it --
        // see `native_args`. Built whether or not anybody is listening,
        // which costs one allocation on the commands nobody handles and
        // is the price of not writing a lifetime into a `#[repr(C)]`
        // struct.
        let joined = native_args(&args);
        allowed_by_mods =
            notify_mods!(ctx, event, native_payload(event, &args, &cells, &joined));
    }

    let view = plugin_view(ctx, cells);
    let (allowed, effects) = {
        let mut host = ctx.plugins.lock().unwrap_or_else(|e| e.into_inner());
        if host.active_count() == 0 {
            return allowed_by_mods;
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
                    world_days: ctx.clock.world_days(),
                });
            }
        }
    }

    allowed && allowed_by_mods
}

/// What comes down when the ground under a plant goes.
///
/// Worldgen refuses to plant grass on rock, but until this existed
/// nothing enforced the rule afterwards: mine the dirt out from under a
/// tuft and it hung in the air with daylight beneath it.
#[cfg(test)]
mod extension_list_tests {
    use super::*;
    use primitive_shared::protocol::ExtensionKind;

    /// A build with no loaders says so, rather than answering an empty
    /// list.
    ///
    /// A screen handed an empty `items` and nothing else could only say
    /// "none installed", which is a different fact from "this build
    /// cannot run any" -- see `protocol::ExtensionList::native_api`.
    ///
    /// **The two halves of that answer stopped agreeing when
    /// singleplayer grew mods**, and this comment used to say they were
    /// one. A local world still carries no scripting engine -- the
    /// client embeds this crate with `default-features = false` and
    /// never links Rhai -- and it now *does* `dlopen`, because
    /// `primitive_client` asks for the `mods` feature back by name. So
    /// `scripts_supported` and `native_api` are read from their own
    /// features here rather than from one idea of "a client build".
    #[test]
    fn a_build_with_no_loaders_says_so_rather_than_answering_an_empty_list() {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let list = extension_list(&ctx);
        assert!(list.items.is_empty(), "a fresh server has nothing loaded");
        assert_eq!(list.scripts_supported, cfg!(feature = "plugins"));
        assert_eq!(list.native_api.is_some(), cfg!(feature = "mods"));
    }

    /// The screen's answer and `/mods`' answer are built from the same
    /// host state and count the same things.
    ///
    /// They are two renderings of one fact, and the whole reason the
    /// second exists is that a screen must not parse a console's lines
    /// -- see `protocol::ExtensionList`. What has to hold is that the
    /// number the console prints in its header is the number of rows
    /// the screen would draw, whatever that number is.
    #[test]
    fn the_screen_and_the_console_count_the_same_extensions() {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let list = extension_list(&ctx);
        let printed = run_command(&ctx, "/mods", commands::Permission::Operator, None);

        // "N plugin(s), M active:" -- the header the console prints.
        let counted = |suffix: &str| -> Option<usize> {
            printed
                .iter()
                .find(|line| line.contains(suffix))
                .and_then(|line| line.split_whitespace().next())
                .and_then(|n| n.parse::<usize>().ok())
        };
        let scripts = list
            .items
            .iter()
            .filter(|item| item.kind == ExtensionKind::Script)
            .count();
        assert_eq!(
            counted("plugin(s)"),
            Some(scripts),
            "the console and the screen disagree about the plugins: {printed:?}",
        );
        #[cfg(feature = "mods")]
        {
            let native = list
                .items
                .iter()
                .filter(|item| item.kind == ExtensionKind::Native)
                .count();
            assert_eq!(
                counted("mod(s)"),
                Some(native),
                "the console and the screen disagree about the mods: {printed:?}",
            );
        }
    }
}

/// What a gesture at an open container may put where. Pure halves of
/// `chest_move` and `chest_quick_move`, driven on two inventories.
#[cfg(test)]
mod container_gesture_tests {
    use super::{container_move, shift_into_roles, Roles};
    use primitive_shared::hearth::{Kind, FUEL_SLOT, INPUT_SLOTS, OUTPUT_SLOTS};
    use primitive_shared::inventory::{filled_jug, jug_contents, quick_move_between, Inventory, Stack};
    use primitive_shared::protocol::Side;
    use primitive_shared::types::{
        BLOCK_COOKED_MEAT, BLOCK_COPPER_PICKAXE, BLOCK_DIRT, BLOCK_IRON_ORE, BLOCK_LEATHER, BLOCK_LOG,
        BLOCK_COAL, BLOCK_SEEDS, BLOCK_STICK, BLOCK_STONE,
    };

    const CAMPFIRE: Roles = Roles::Hearth(Kind::Campfire);

    #[test]
    fn a_worn_pick_shift_clicked_through_a_campfire_comes_back_as_worn_as_it_went() {
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::worn(BLOCK_COPPER_PICKAXE, 1, 150));
        let mut fire = Inventory::new();
        assert!(shift_into_roles(&mut pack, 0, &mut fire, CAMPFIRE), "the pick did not go in");
        let slot = INPUT_SLOTS
            .clone()
            .find(|&slot| fire.block_in(slot).is_some())
            .expect("the pick went into no ingredient slot");
        assert_eq!(fire.slots()[slot].unwrap().damage, 150, "the fire mended the pick");
        assert!(quick_move_between(&mut fire, slot, &mut pack));
        assert_eq!(pack.slots()[0].unwrap().damage, 150, "the pick came home new");
    }

    #[test]
    fn a_jug_of_grain_shift_clicked_into_a_hearth_still_has_its_grain() {
        let mut pack = Inventory::new();
        pack.put_in_slot(0, filled_jug(BLOCK_SEEDS, 9));
        let mut fire = Inventory::new();
        assert!(shift_into_roles(&mut pack, 0, &mut fire, CAMPFIRE));
        let jug = fire.slots()[INPUT_SLOTS.start].expect("the jug went nowhere");
        assert_eq!(jug_contents(&jug), Some((BLOCK_SEEDS, 9)), "the grain was deleted");
    }

    #[test]
    fn dragging_supper_out_of_a_hearth_onto_a_full_square_does_not_put_that_square_in_the_tray() {
        let mut fire = Inventory::new();
        fire.put_in_slot(OUTPUT_SLOTS.start, Stack::new(BLOCK_COOKED_MEAT, 2));
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STICK, 5));
        container_move(&mut pack, &mut fire, CAMPFIRE, (Side::Chest, OUTPUT_SLOTS.start), (Side::Pack, 0), false);
        assert_ne!(
            fire.block_in(OUTPUT_SLOTS.start),
            Some(BLOCK_STICK),
            "a stick was swapped into the output tray, which nothing may be put into"
        );
        assert_eq!(fire.total_items() + pack.total_items(), 7, "something was lost");
    }

    #[test]
    fn rearranging_a_hearth_never_swaps_ore_into_the_fuel_slot() {
        let mut fire = Inventory::new();
        fire.put_in_slot(FUEL_SLOT, Stack::new(BLOCK_COAL, 3));
        fire.put_in_slot(INPUT_SLOTS.start, Stack::new(BLOCK_IRON_ORE, 2));
        let mut pack = Inventory::new();
        container_move(&mut pack, &mut fire, CAMPFIRE, (Side::Chest, FUEL_SLOT), (Side::Chest, INPUT_SLOTS.start), false);
        assert_ne!(fire.block_in(FUEL_SLOT), Some(BLOCK_IRON_ORE), "ore was swapped into the fuel slot");
        // ...while a swap whose both halves are welcome still happens.
        fire.take_slot(INPUT_SLOTS.start);
        fire.put_in_slot(INPUT_SLOTS.start, Stack::new(BLOCK_LOG, 2));
        assert!(container_move(&mut pack, &mut fire, CAMPFIRE, (Side::Chest, FUEL_SLOT), (Side::Chest, INPUT_SLOTS.start), false));
        assert_eq!(fire.block_in(FUEL_SLOT), Some(BLOCK_LOG));
    }

    #[test]
    fn taking_leather_off_a_rack_onto_a_full_square_does_not_hang_that_square_on_the_rack() {
        use primitive_shared::rack::LEATHER_SLOT;
        let mut rack = Inventory::new();
        rack.put_in_slot(LEATHER_SLOT, Stack::new(BLOCK_LEATHER, 1));
        let mut pack = Inventory::new();
        pack.put_in_slot(3, Stack::new(BLOCK_STONE, 4));
        container_move(&mut pack, &mut rack, Roles::Rack(primitive_shared::rack::Trade::Skins), (Side::Chest, LEATHER_SLOT), (Side::Pack, 3), false);
        assert_ne!(rack.block_in(LEATHER_SLOT), Some(BLOCK_STONE), "a stone was hung on the rack");
    }

    #[test]
    fn a_chest_still_swaps_whatever_is_in_the_way() {
        let mut chest = Inventory::new();
        chest.put_in_slot(0, Stack::new(BLOCK_STONE, 4));
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_DIRT, 7));
        assert!(container_move(&mut pack, &mut chest, Roles::Chest, (Side::Chest, 0), (Side::Pack, 0), false));
        assert_eq!(pack.block_in(0), Some(BLOCK_STONE));
        assert_eq!(chest.block_in(0), Some(BLOCK_DIRT));
    }

    /// **Square forty-five is a body's rucksack and nothing of a chest's.**
    /// Asked of a container sixty long on purpose -- the length a body's
    /// compartment gives it -- so the only thing that can refuse the chest
    /// is the rule, not the length.
    #[test]
    fn a_chest_refuses_a_move_into_square_45_and_a_body_with_a_rucksack_takes_it() {
        let mut long = Inventory::body(true);
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE, 4));
        assert!(!container_move(&mut pack, &mut long, Roles::Chest, (Side::Pack, 0), (Side::Chest, 45), false));
        assert_eq!(long.block_in(45), None, "a chest took a square it does not have");

        assert!(container_move(&mut pack, &mut long, Roles::Body, (Side::Pack, 0), (Side::Chest, 45), false));
        assert_eq!(long.block_in(45), Some(BLOCK_STONE));
        // ...and back out of it.
        assert!(container_move(&mut pack, &mut long, Roles::Body, (Side::Chest, 45), (Side::Pack, 2), false));
        assert_eq!(pack.block_in(2), Some(BLOCK_STONE));

        // A body without a compartment has no square forty-five, and the
        // stone stays in the hand it was in.
        let mut bare = Inventory::body(false);
        assert!(!container_move(&mut pack, &mut bare, Roles::Body, (Side::Pack, 2), (Side::Chest, 45), false));
        assert_eq!(pack.block_in(2), Some(BLOCK_STONE), "the stone went into a square that is not there");
    }
}

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
/// The body goes into a cell chosen from where they fell, and every
/// interesting case is a *place to die* rather than anything the server
/// does afterwards -- which is why the chooser takes the world as a
/// closure and this needs no fixture.
/// The way back to a body: what goes on the list and what comes off it.
#[cfg(test)]
mod way_back_tests {
    use super::{bag_is_gone, remember_bag, stash_corpse, MAX_BAGS};
    use crate::logic::containers::Chests;
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::{
        BLOCK_AIR, BLOCK_BACKPACK, BLOCK_CORPSE, BLOCK_DIRT, BLOCK_REMAINS, BLOCK_STICK,
        BLOCK_STONE, CHUNK_SIZE_Y,
    };

    fn floor_at_ten(_x: i32, y: i32, _z: i32) -> Option<primitive_shared::types::BlockId> {
        (0..CHUNK_SIZE_Y as i32)
            .contains(&y)
            .then_some(if y < 10 { BLOCK_STONE } else { BLOCK_AIR })
    }

    fn a_full_pack() -> Inventory {
        let mut carried = Inventory::new();
        assert_eq!(carried.add(BLOCK_DIRT, 23), 0);
        assert_eq!(carried.add(BLOCK_STICK, 5), 0);
        carried
    }

    #[test]
    fn dying_leaves_one_body_holding_exactly_what_was_carried() {
        let mut chests = Chests::new();
        let carried = a_full_pack();
        let (at, leftovers) =
            stash_corpse(&mut chests, &carried, (3.5, 10.0, -2.5), floor_at_ten, |_, _, _| true)
                .expect("there was nowhere to put the body");
        assert!(leftovers.is_empty(), "something was turned away from an empty body");
        assert_eq!(chests.len(), 1, "one death left more than one body");
        let inside = chests.contents(at);
        assert_eq!(inside.count(BLOCK_DIRT), 23);
        assert_eq!(inside.count(BLOCK_STICK), 5);
        assert_eq!(inside.total_items(), carried.total_items());
    }

    #[test]
    fn a_body_has_no_despawn_clock_and_outlasts_a_restart() {
        // A stack on the ground is gone after `items::LIFETIME`; a body
        // is a container, and the container store has no notion of time
        // at all -- the only way its contents leave is the rot, which
        // takes the soft half and nothing else (`carrion`), or a save
        // that loses them, which is what this checks.
        let dir = std::env::temp_dir().join(format!("primitive-body-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut chests = Chests::new();
        let carried = a_full_pack();
        let (at, _) =
            stash_corpse(&mut chests, &carried, (0.5, 10.0, 0.5), floor_at_ten, |_, _, _| true)
                .expect("a body");
        chests.save(&dir).expect("save");

        let mut after_a_restart = Chests::new();
        after_a_restart.load(&dir).expect("load");
        assert_eq!(
            after_a_restart.contents(at).total_items(),
            carried.total_items(),
            "the body came back from disk with less in it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_map_forgets_the_oldest_body_and_never_the_newest() {
        let mut bags = Vec::new();
        for n in 0..(MAX_BAGS as i32 + 2) {
            remember_bag(&mut bags, (n * 100, 60, 0));
        }
        assert_eq!(bags.len(), MAX_BAGS);
        assert_eq!(
            bags.last(),
            Some(&((MAX_BAGS as i32 + 1) * 100, 60, 0)),
            "the newest body was dropped"
        );
        assert!(!bags.contains(&(0, 60, 0)), "the oldest body was kept");
    }

    #[test]
    fn dying_twice_in_one_cell_is_one_mark_to_walk_to() {
        let mut bags = Vec::new();
        remember_bag(&mut bags, (5, 60, 5));
        remember_bag(&mut bags, (9, 60, 9));
        remember_bag(&mut bags, (5, 60, 5));
        assert_eq!(bags, vec![(9, 60, 9), (5, 60, 5)]);
    }

    #[test]
    fn a_body_in_a_chunk_nobody_has_loaded_is_not_given_up_on() {
        // The far side of the world is exactly where a body lies.
        assert!(!bag_is_gone(None, false));
    }

    #[test]
    fn an_emptied_or_broken_body_comes_off_the_map_and_a_full_one_stays() {
        assert!(!bag_is_gone(Some(BLOCK_CORPSE), true), "a body with things in it was given up on");
        assert!(bag_is_gone(Some(BLOCK_CORPSE), false), "an emptied body stayed on the map");
        assert!(bag_is_gone(Some(BLOCK_AIR), false), "a broken body stayed on the map");
        assert!(bag_is_gone(Some(BLOCK_STONE), true), "a buried body stayed on the map");
    }

    /// **The mark has to survive the body rotting.**
    ///
    /// The corpse becomes bones where it lies, two days later
    /// (`carrion::CORPSE_STEPS_TO_ROT`), and the cell does not move. A
    /// check that only knew the fresh body would have taken the grave off
    /// the map at the exact hour the player was hurrying to it -- and
    /// they would have arrived at a cell their own map no longer drew.
    #[test]
    fn a_body_that_has_rotted_to_bones_is_still_on_the_map() {
        assert!(
            !bag_is_gone(Some(BLOCK_REMAINS), true),
            "the grave was forgotten the moment it rotted"
        );
        // ...and bones somebody has emptied are done with, like anything
        // else that has been emptied.
        assert!(bag_is_gone(Some(BLOCK_REMAINS), false));
    }

    /// **A world saved before bodies existed still has bags in it.**
    ///
    /// Nothing makes a new backpack -- `leave_corpse` writes a corpse --
    /// but the profile of everyone who died before the update lists cells
    /// that hold one. Answering "gone" for those would have cleared every
    /// old mark off every old map on the first tick after the update,
    /// with the bags themselves still standing there, full and now
    /// unfindable.
    #[test]
    fn a_backpack_left_by_an_older_build_is_still_worth_walking_to() {
        assert!(!bag_is_gone(Some(BLOCK_BACKPACK), true), "an old bag was given up on");
        assert!(bag_is_gone(Some(BLOCK_BACKPACK), false), "an emptied old bag stayed on the map");
    }
}

#[cfg(test)]
mod corpse_tests {
    use super::corpse_cell;
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
    fn dying_on_your_feet_leaves_the_body_where_you_stood() {
        let at = corpse_cell((4.3, 10.0, 7.8), ground_at(10));
        assert_eq!(at, Some((4, 10, 7)));
    }

    #[test]
    fn a_negative_coordinate_floors_rather_than_truncates() {
        // -7.8 is in cell -8, not -7. Truncation would put the body a
        // block away from the body, on the wrong side of a chunk
        // boundary as often as not.
        let at = corpse_cell((-0.2, 10.0, -7.8), ground_at(10));
        assert_eq!(at, Some((-1, 10, -8)));
    }

    #[test]
    fn dying_inside_a_block_puts_the_body_above_it() {
        // Crushed or suffocated: the cell the body is in is solid, so
        // the body goes up rather than nowhere.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 12 => BLOCK_STONE,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(corpse_cell((0.5, 10.0, 0.5), world), Some((0, 12, 0)));
    }

    #[test]
    fn drowning_leaves_it_in_the_water_rather_than_on_the_shore() {
        // Water is something you build into, so it is somewhere a body
        // can go -- and a body that dodged sideways out of the lake would
        // be a body the player cannot find.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 5 => BLOCK_STONE,
                y if y < 20 => BLOCK_WATER,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(corpse_cell((0.5, 9.0, 0.5), world), Some((0, 9, 0)));
    }

    #[test]
    fn a_body_prefers_a_floor_under_it_to_the_cell_you_died_in() {
        // Falling: the server's last position for a body in mid-air is
        // in mid-air. A body left there hangs over whatever killed them.
        let at = corpse_cell((0.5, 12.0, 0.5), ground_at(11));
        assert_eq!(at, Some((0, 11, 0)), "the body was left hanging");
    }

    #[test]
    fn something_growing_in_the_cell_is_no_obstacle() {
        // A tuft of grass is walked through, built through, and is not a
        // reason to put a body somewhere else.
        let world = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y < 10 => BLOCK_STONE,
                10 => BLOCK_TALL_GRASS,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(corpse_cell((0.5, 10.0, 0.5), world), Some((0, 10, 0)));
    }

    #[test]
    fn a_cell_nobody_has_loaded_is_a_refusal_rather_than_an_empty_one() {
        // `None` means "not loaded". Treating it as air writes a block
        // into a chunk that will be regenerated over the top of it,
        // which loses everything in the body.
        assert_eq!(corpse_cell((0.5, 10.0, 0.5), |_, _, _| None), None);
    }

    #[test]
    fn there_is_no_cell_at_all_inside_solid_rock() {
        // The caller's fallback: the things are spilled as drops rather
        // than left in a block that could not be placed.
        assert_eq!(corpse_cell((0.5, 30.0, 0.5), |_, _, _| Some(BLOCK_STONE)), None);
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
        assert_eq!(corpse_cell((0.5, ceiling as f32, 0.5), world), Some((0, ceiling, 0)));
        // ...and a body on the floor of the world has a floor under it.
        assert_eq!(corpse_cell((0.5, 1.0, 0.5), world), Some((0, 1, 0)));
    }

    #[test]
    fn a_position_that_is_not_a_number_is_refused() {
        // Positions come off a socket, and `f32::NAN as i32` is zero --
        // which would quietly bury somebody's pack at the origin.
        assert_eq!(corpse_cell((f32::NAN, 10.0, 0.0), ground_at(10)), None);
        assert_eq!(corpse_cell((0.0, f32::INFINITY, 0.0), ground_at(10)), None);
    }

    // ---- and what happens when two of them arrive at once ----

    use super::stash_corpse;
    use crate::logic::containers::Chests;
    use primitive_shared::inventory::Inventory;
    use primitive_shared::types::{BLOCK_CORPSE, BLOCK_DIRT, BLOCK_STICK};
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
        /// `leave_corpse` runs it.
        fn die_with(&self, chests: &mut Chests, at: (f32, f32, f32), carried: &Inventory) {
            let stashed = stash_corpse(
                chests,
                carried,
                at,
                |x, y, z| self.look(x, y, z),
                |x, y, z| self.place(x, y, z, BLOCK_CORPSE),
            );
            assert!(stashed.is_some(), "there was nowhere to put a body at all");
            let (_, leftovers) = stashed.unwrap();
            assert!(
                leftovers.is_empty(),
                "a body with room in it turned somebody's things away"
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
        // body in it, so it picks a different cell. It can only do that
        // because it cannot run until the first one has finished -- the
        // signature of `stash_corpse` takes `&mut Chests`, so there is
        // no way to reach it without holding the one lock that
        // serialises the whole gesture.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();

        world.die_with(&mut chests, (0.5, 10.0, 0.5), &carrying(BLOCK_DIRT, 40));
        world.die_with(&mut chests, (0.5, 10.0, 0.5), &carrying(BLOCK_STICK, 7));

        assert_eq!(chests.len(), 2, "the two bodies shared one cell");
        assert_eq!(chests.contents((0, 10, 0)).count(BLOCK_DIRT), 40);
    }

    #[test]
    fn two_bodies_forced_into_one_cell_keep_both_sets_of_things() {
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
        let (at, leftovers) = stash_corpse(
            &mut chests,
            &carrying(BLOCK_STICK, 7),
            (0.5, 10.0, 0.5),
            before_anyone_died,
            |x, y, z| world.place(x, y, z, BLOCK_CORPSE),
        )
        .expect("nowhere to put the second body");

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
    fn a_body_landing_on_a_full_one_merges_rather_than_replaces() {
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

        let stashed = stash_corpse(
            &mut chests,
            &carrying(BLOCK_STICK, 5),
            (0.5, 10.0, 0.5),
            |x, y, z| world.look(x, y, z),
            |x, y, z| world.place(x, y, z, BLOCK_CORPSE),
        );
        let (at, leftovers) = stashed.expect("nowhere to put the body");
        assert_eq!(at, (0, 10, 0));
        assert!(leftovers.is_empty());
        assert_eq!(chests.contents(at).count(BLOCK_DIRT), 12, "the old contents were wiped");
        assert_eq!(chests.contents(at).count(BLOCK_STICK), 5, "the new contents were lost");
    }

    #[test]
    fn a_cell_that_cannot_be_written_is_no_cell_at_all() {
        // `place` refusing is the world saying "not there". Filling the
        // chest map anyway would file somebody's things against a cell
        // holding no body, where nothing will ever find them again.
        let world = FakeWorld::new(10);
        let mut chests = Chests::new();
        let stashed = stash_corpse(
            &mut chests,
            &carrying(BLOCK_DIRT, 3),
            (0.5, 10.0, 0.5),
            |x, y, z| world.look(x, y, z),
            |_, _, _| false,
        );
        assert!(stashed.is_none());
        assert!(chests.is_empty(), "things were filed against a cell with no body in it");
    }
}

/// A death, from the blow to the bones, on a real server.
///
/// The unit tests above are about *where* a body goes and what the map
/// does with it; this is the whole gesture through the code a player's
/// death actually runs: the pack emptied, the block written, the
/// container filled, the mark sent, the carrion pass finding it, and two
/// days later the soft half gone and the iron still there.
#[cfg(test)]
mod dying_tests {
    use super::butchering_tests::a_hunter;
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        BLOCK_COOKED_MEAT, BLOCK_CORPSE, BLOCK_IRON_INGOT, BLOCK_IRON_PICKAXE, BLOCK_LEATHER_BOOTS,
        BLOCK_LEATHER_TUNIC, BLOCK_REMAINS,
    };

    /// Where the body ended up, read off the player's own map.
    ///
    /// Not computed from where they were standing: the cell is chosen by
    /// `corpse_cell`, which looks for a floor and may go a block up or
    /// down, and a test that assumed the feet would be testing the
    /// fixture's terrain rather than the code.
    fn grave_of(handle: &Arc<players::PlayerHandle>) -> (i32, i32, i32) {
        let state = handle.state.lock().unwrap();
        *state.bags.last().expect("the death left no mark on the map")
    }

    /// A player carrying the two halves of the bargain -- a soft half
    /// that the ground takes and a hard half it cannot -- with a pair of
    /// boots on their feet.
    fn kitted_out(handle: &Arc<players::PlayerHandle>) {
        let mut state = handle.state.lock().unwrap();
        state.inventory.put_in_slot(0, Stack::new(BLOCK_IRON_PICKAXE, 1));
        state.inventory.put_in_slot(1, Stack::new(BLOCK_IRON_INGOT, 7));
        state.inventory.put_in_slot(2, Stack::new(BLOCK_LEATHER_TUNIC, 1));
        state.inventory.put_in_slot(3, Stack::new(BLOCK_COOKED_MEAT, 4));
        assert!(state.equipment.wear(Stack::new(BLOCK_LEATHER_BOOTS, 1)).is_none());
    }

    #[test]
    fn dying_leaves_your_own_body_holding_everything_you_had_on() {
        let (ctx, handle, _rx) = a_hunter();
        kitted_out(&handle);

        leave_corpse(&ctx, &handle);

        // The body is in the world, at the cell the player is sent back
        // to.
        let at = grave_of(&handle);
        assert_eq!(
            ctx.world.cached_block(at.0, at.1, at.2),
            Some(BLOCK_CORPSE),
            "there is no body where the player died"
        );
        // ...holding everything, the boots included: armour left on the
        // ghost would make a full set the one thing you cannot lose.
        let inside = ctx.chests.lock().unwrap().contents(at);
        assert_eq!(inside.count(BLOCK_IRON_PICKAXE), 1);
        assert_eq!(inside.count(BLOCK_IRON_INGOT), 7);
        assert_eq!(inside.count(BLOCK_LEATHER_TUNIC), 1);
        assert_eq!(inside.count(BLOCK_COOKED_MEAT), 4);
        assert_eq!(inside.count(BLOCK_LEATHER_BOOTS), 1, "the worn boots stayed on the ghost");
        // ...and the player is carrying none of it, which is the other
        // half of the same rule: a death must not double anybody's stock.
        let state = handle.state.lock().unwrap();
        assert!(state.inventory.is_empty(), "they respawned with their things");
        assert!(state.equipment.is_empty());
        // ...and they are told where it is.
        assert_eq!(state.bags, vec![at], "the way back was not remembered");
    }

    /// **The whole mechanic, end to end.**
    ///
    /// The body joins the carrion map through `notify_mechanics` -- not
    /// through anything the death did on purpose, which is the point:
    /// every other way a corpse can appear (a mod, a restart, a chunk
    /// coming back) goes through the same queue. Two days of the rot
    /// clock later the cell is bones, the leather and the meat are gone
    /// with the flesh, and the iron is lying in them.
    #[test]
    fn a_body_nobody_came_back_for_is_bones_with_the_iron_still_in_it() {
        let (ctx, handle, _rx) = a_hunter();
        kitted_out(&handle);
        leave_corpse(&ctx, &handle);
        let at = grave_of(&handle);

        // The queue, reconciled the way the tick loop reconciles it.
        {
            let mut carrion = ctx.carrion.lock().unwrap();
            carrion.reconcile(64, |cell| ctx.world.cached_block(cell.0, cell.1, cell.2));
            assert_eq!(carrion.len(), 1, "the body never reached the carrion pass");
        }

        // Two days of the clock. Driven through `Rot::pass` rather than
        // through `Carrion::step` directly, because what is being checked
        // is the *seam*: the pass is what rewrites the block and filters
        // the container, and a test that called the parts would have
        // passed while the game did nothing.
        let day = ctx.clock.day_length_seconds();
        // Twice the steps: the meadow's nights are cellar-cool, and a body
        // in cool air ages on every other step (`rot::Keeping::Cool`) as the
        // meat in a pack does. Four days covers any mix of warm and cool.
        let steps_wanted = crate::logic::carrion::CORPSE_STEPS_TO_ROT as f32 * 2.0;
        let seconds = logic::rot::Rot::step_seconds(day) * (steps_wanted + 1.0);
        let mut rot = logic::rot::Rot::new();
        // A second at a time: `due` clamps `dt` to one, so a single huge
        // frame is one step and not a fortnight.
        for _ in 0..(seconds.ceil() as u32) {
            rot.pass(&ctx, 1.0);
        }

        assert_eq!(
            ctx.world.cached_block(at.0, at.1, at.2),
            Some(BLOCK_REMAINS),
            "four days passed and the body is still fresh -- is the spot below freezing?"
        );
        let inside = ctx.chests.lock().unwrap().contents(at);
        // The half a week of ore is still there. This is the promise the
        // mechanic makes to the player who could not get back in time.
        assert_eq!(inside.count(BLOCK_IRON_PICKAXE), 1, "the pick rotted out of the grave");
        assert_eq!(inside.count(BLOCK_IRON_INGOT), 7, "the ingots rotted");
        // ...and the soft half is what hurrying would have bought.
        assert_eq!(inside.count(BLOCK_LEATHER_TUNIC), 0, "the leather outlasted the body");
        assert_eq!(inside.count(BLOCK_LEATHER_BOOTS), 0, "the boots outlasted the body");
        assert_eq!(inside.count(BLOCK_COOKED_MEAT), 0, "two-day-old meat in a corpse");

        // The grave is still on the map: it is still worth walking to,
        // and forgetting it here would strand the player who set out.
        forget_recovered_bags(&ctx, &handle);
        assert_eq!(
            handle.state.lock().unwrap().bags,
            vec![at],
            "the map forgot the grave the moment it rotted"
        );
    }
}

#[cfg(test)]
mod saddlebag_rot_tests {
    use super::butchering_tests::{a_hunter, FLOOR};
    use super::*;
    use primitive_shared::types::BLOCK_COOKED_MEAT;

    /// **A horse is not a larder.** The rot clock walked the packs, the
    /// chests, the ground and the kills, and never the saddlebags: meat
    /// carried on a horse kept for ever.
    #[test]
    fn meat_in_a_horses_saddlebags_goes_off_as_it_would_in_a_chest() {
        let (ctx, _handle, _rx) = a_hunter();
        let horse = {
            let mut animals = ctx.animals.lock().unwrap();
            let horse = animals.spawn(primitive_shared::animals::Species::Horse, (4.5, FLOOR as f32 + 1.0, 4.5)).expect("a horse");
            let mut bags = primitive_shared::inventory::Inventory::new();
            assert_eq!(bags.add(BLOCK_COOKED_MEAT, 4), 0);
            let keep = primitive_shared::husbandry::Keeping { trust: 1.0, tame: true, ..Default::default() };
            animals.put_keeping(horse, keep, Some(primitive_shared::horse::Gear { saddle: true, bags: Some(bags), rides: 0 }));
            horse
        };
        let day = ctx.clock.day_length_seconds();
        let seconds = logic::rot::Rot::step_seconds(day) * (primitive_shared::food::ROT_STEPS_PER_DAY as f32 * 2.0 + 1.0);
        let mut rot = logic::rot::Rot::new();
        for _ in 0..(seconds.ceil() as u32) {
            rot.pass(&ctx, 1.0);
        }
        let bags = ctx.animals.lock().unwrap().gear(horse).and_then(|g| g.bags).expect("the bags went");
        let meat = bags.slots().iter().flatten().next().expect("the meat went");
        assert_ne!(meat.block, BLOCK_COOKED_MEAT, "two days in the saddlebags and the meat is as it went in");
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

    /// A server on an ordinary world, and the chest of the first ruin
    /// with one, generated and in the cache.
    fn ruin_chest_context(tag: &str) -> (TempDir, Arc<Context>, containers::ChestPos) {
        let dir = TempDir::new(tag);
        let settings = ServerSettings {
            bind_addr: "127.0.0.1:0".to_string(),
            world_dir: dir.0.display().to_string(),
            plugin_dir: String::new(),
            ..Default::default()
        };
        let ctx = build_context(settings, RunOptions::embedded()).expect("context");
        let at = primitive_shared::worldgen::ruins_in(ctx.world.generator(), (-960, -960), (960, 960))
            .into_iter()
            .find_map(|ruin| ruin.chest)
            .expect("no ruin with a chest within a kilometre of spawn");
        let (pos, _, _) = primitive_shared::types::ChunkPos::from_global(at.0, at.2);
        ctx.world.insert(ctx.world.generate(pos));
        (dir, ctx, at)
    }

    #[test]
    fn a_ruin_chest_is_filled_once_and_an_emptied_one_stays_empty() {
        let (_dir, ctx, at) = ruin_chest_context("ruin_once");
        assert!(ctx.chests.lock().unwrap().contents(at).is_empty(), "stocked before anybody came");
        unseal_ruin_chest(&ctx, at);
        assert!(!ctx.chests.lock().unwrap().contents(at).is_empty(), "opened a ruin chest and found nothing");
        assert!(ctx.world.is_edited(at.0, at.1, at.2), "the seal was not recorded in the world");

        // Taken out to the last stick -- and the next time it is opened
        // it is still empty, which is the bug a restocking chest would be.
        ctx.chests.lock().unwrap().take(at);
        unseal_ruin_chest(&ctx, at);
        assert!(ctx.chests.lock().unwrap().contents(at).is_empty(), "an emptied ruin chest filled itself again");
    }

    #[test]
    fn breaking_a_ruin_chest_nobody_opened_spills_what_was_in_it() {
        let (_dir, ctx, at) = ruin_chest_context("ruin_spill");
        let before = ctx.items.lock().unwrap().len();
        spill_chest(&ctx, at);
        let after = ctx.items.lock().unwrap().len();
        assert!(after > before, "a sealed ruin chest broke open with nothing in it");
    }

    #[test]
    fn a_chest_a_player_built_is_never_taken_for_a_ruins() {
        let (_dir, ctx, at) = ruin_chest_context("ruin_built");
        // The same cell, but built: the edit is there before any seal is.
        ctx.world.set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_CHEST);
        unseal_ruin_chest(&ctx, at);
        assert!(ctx.chests.lock().unwrap().contents(at).is_empty(), "a built chest came with a ruin's things in it");
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
            profiles.store(
                joined.uuid,
                Inventory::new(),
                (9.0, 8.0, 7.0),
                0.0,
                0.0,
                20.0,
                20.0,
                0,
                crate::profiles::StoredBody::default(),
            );
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
    fn a_save_taken_while_sand_is_falling_writes_the_sand_too() {
        // The seam, not the routine -- the same distinction the test
        // above was written for. `ground_all` is unit-tested in
        // `logic::falling`; what this covers is that the save path
        // actually calls it. Without that call, a player who quits a
        // singleplayer world while a dune is coming down loses every
        // block that happened to be in the air, and the hole they mined
        // it out of is saved perfectly.
        let dir = TempDir::new("mid_fall");
        let ctx = loaded_context(&dir.0);

        // A short tower of sand over a floor, in a chunk that is really
        // loaded: `set_block` only reaches a cached chunk.
        let pos = ChunkPos::from_global(0, 0).0;
        if ctx.world.cached(pos).is_none() {
            let chunk = ctx.world.generate(pos);
            ctx.world.insert(chunk);
        }
        for y in 20..=30 {
            ctx.world.set_block(0, y, 0, primitive_shared::types::BLOCK_AIR);
        }
        ctx.world.set_block(0, 20, 0, BLOCK_STONE);
        for y in 26..=28 {
            ctx.world.set_block(0, y, 0, primitive_shared::types::BLOCK_SAND);
        }
        {
            let mut sim = ctx.falling.lock().unwrap();
            sim.on_block_changed(0, 26, 0);
            // Enough passes to get it airborne and not enough for any
            // of it to have landed.
            for _ in 0..5 {
                sim.step(&*ctx.world, 1.0 / 20.0);
            }
            assert!(sim.entity_count() > 0, "the fixture has to be mid-fall");
        }

        run_command(&ctx, "/save", commands::Permission::Operator, None);

        assert_eq!(
            ctx.falling.lock().unwrap().entity_count(),
            0,
            "the save left blocks in the air, which is where they are lost"
        );
        let landed = (21..=30)
            .filter(|&y| {
                ctx.world.cached_block(0, y, 0) == Some(primitive_shared::types::BLOCK_SAND)
            })
            .count();
        assert_eq!(landed, 3, "{landed} of three blocks of sand survived the save");
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

/// Drinking, and the rule every gesture that turns one item into another
/// has to obey: **nothing is spent unless the gesture happened, and the
/// gesture does not happen unless what it produces has somewhere to go.**
#[cfg(test)]
mod drinking_tests {
    use super::*;
    use primitive_shared::inventory::{MAX_STACK, SLOTS};
    use primitive_shared::types::{BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_STONE};

    fn a_thirsty_player() -> (Arc<Context>, Arc<players::PlayerHandle>) {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        // Kept alive: a dropped receiver closes the queue, and `send`
        // would then count every message as dropped rather than sent.
        std::mem::forget((rx, chunk_rx));
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "thirsty".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            (0.5, 20.0, 0.5),
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                (0.5, 20.0, 0.5),
            ),
        ));
        {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_hydration(0.0);
        }
        (ctx, handle)
    }

    #[test]
    fn a_jug_with_nowhere_to_put_the_empty_one_is_not_drunk_for_free() {
        // The pack is full to the last slot and the first one holds two
        // full jugs, so the empty jug a drink produces has nowhere to
        // go. The gesture has to refuse *before* the water is credited:
        // crediting it first and then bailing out is an unlimited
        // drinking fountain, because the full jug is still there to be
        // clicked again.
        let (ctx, handle) = a_thirsty_player();
        {
            let mut state = handle.state.lock().unwrap();
            assert_eq!(state.inventory.add(BLOCK_JUG_WATER, 2), 0);
            for _ in 1..SLOTS {
                assert_eq!(state.inventory.add(BLOCK_STONE, MAX_STACK), 0);
            }
            assert!(
                !state.inventory.has_room_for(BLOCK_JUG, 1),
                "the pack still has room, so this test proves nothing"
            );
        }

        eat_from_slot(&ctx, &handle, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(
            state.inventory.count(BLOCK_JUG_WATER),
            2,
            "the jug was spent after all"
        );
        assert!(
            state.vitals.hydration_fraction() < 0.01,
            "the water was credited without the jug being emptied"
        );
    }

    #[test]
    fn a_jug_that_does_fit_is_drunk_and_comes_back_empty() {
        // The other half, so the guard above cannot be satisfied by
        // refusing everything.
        let (ctx, handle) = a_thirsty_player();
        {
            let mut state = handle.state.lock().unwrap();
            assert_eq!(state.inventory.add(BLOCK_JUG_WATER, 2), 0);
        }

        eat_from_slot(&ctx, &handle, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_JUG_WATER), 1);
        assert_eq!(state.inventory.count(BLOCK_JUG), 1);
        assert!(state.vitals.hydration_fraction() > 0.4, "the drink did nothing");
    }

    /// **The pack pays for what the animal decided**: one grain for a feed,
    /// wool for the knife's wear, a bowl for a bowl of milk -- and nothing at
    /// all for a feed refused.
    #[test]
    fn tending_a_sheep_spends_the_feed_and_pays_out_the_wool_and_the_milk() {
        use primitive_shared::husbandry::Keeping;
        use primitive_shared::types::{BLOCK_BOWL, BLOCK_BOWL_MILK, BLOCK_FLINT_KNIFE, BLOCK_GRAIN, BLOCK_WOOL};
        let (ctx, handle) = a_thirsty_player();
        let ewe = ctx.animals.lock().unwrap().spawn_at(primitive_shared::animals::Species::Sheep, (1.5, 20.0, 0.5)).unwrap();
        let hold = |block, count| {
            let mut state = handle.state.lock().unwrap();
            state.inventory = primitive_shared::inventory::Inventory::new();
            assert_eq!(state.inventory.add(block, count), 0);
            state.selected_slot = 0;
        };
        hold(BLOCK_GRAIN, 3);
        tend_animal(&ctx, &handle, ewe);
        tend_animal(&ctx, &handle, ewe);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_GRAIN), 2, "a refused feed was spent, or a taken one was not");

        let tame = Keeping { trust: 1.0, tame: true, home: Some((1.5, 20.0, 0.5)), ..Keeping::wild() };
        ctx.animals.lock().unwrap().keep_for_test(ewe, tame);
        hold(BLOCK_FLINT_KNIFE, 1);
        tend_animal(&ctx, &handle, ewe);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_WOOL), primitive_shared::husbandry::FLEECE_WOOL);

        let _lamb = ctx.animals.lock().unwrap().bear_young(ewe).expect("a lamb");
        hold(BLOCK_BOWL, 2);
        tend_animal(&ctx, &handle, ewe);
        let state = handle.state.lock().unwrap();
        assert_eq!((state.inventory.count(BLOCK_BOWL), state.inventory.count(BLOCK_BOWL_MILK)), (1, 1));
    }

    #[test]
    fn a_stew_is_eaten_and_its_bowl_comes_back_to_the_pack() {
        use primitive_shared::types::{BLOCK_BOWL, BLOCK_STEW};
        let (ctx, handle) = a_thirsty_player();
        {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_nourishment(0.0);
            assert_eq!(state.inventory.add(BLOCK_STEW, 2), 0);
        }
        assert!(eat_from_slot(&ctx, &handle, 0), "a hungry player could not eat a stew");
        {
            let state = handle.state.lock().unwrap();
            assert_eq!(state.inventory.count(BLOCK_STEW), 1);
            assert_eq!(state.inventory.count(BLOCK_BOWL), 1, "the bowl was eaten with the stew");
        }
        // The last one leaves its own slot for its bowl, however full the
        // pack is.
        {
            let mut state = handle.state.lock().unwrap();
            for _ in 0..SLOTS {
                let _ = state.inventory.add(BLOCK_STONE, MAX_STACK);
            }
        }
        assert!(eat_from_slot(&ctx, &handle, 0), "the last bowl of stew in a full pack was refused");
        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_STEW), 0);
        assert_eq!(state.inventory.count(BLOCK_BOWL), 2, "a bowl went missing");
    }

    #[test]
    fn a_stew_with_nowhere_to_put_its_bowl_is_not_eaten() {
        use primitive_shared::types::{BLOCK_BOWL, BLOCK_STEW};
        let (ctx, handle) = a_thirsty_player();
        {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_nourishment(0.0);
            assert_eq!(state.inventory.add(BLOCK_STEW, 2), 0);
            for _ in 1..SLOTS {
                assert_eq!(state.inventory.add(BLOCK_STONE, MAX_STACK), 0);
            }
            assert!(!state.inventory.has_room_for(BLOCK_BOWL, 1), "the pack has room, so this proves nothing");
        }
        assert!(!eat_from_slot(&ctx, &handle, 0), "a stew was eaten with nowhere for its bowl");
        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_STEW), 2);
        assert!(state.vitals.nourishment_fraction() < 0.01, "the stew was credited and not spent");
    }
}

/// Pouring dry goods into a jug and tipping them back out, and the one
/// rule both halves obey: **nothing ever ceases to exist.**
#[cfg(test)]
mod jug_tests {
    use super::*;
    use primitive_shared::inventory::{
        filled_jug, jug_contents, Stack, JUG_UNITS, MAX_STACK, SLOTS,
    };
    use primitive_shared::types::{BLOCK_GRAIN, BLOCK_JUG, BLOCK_RAW_MEAT, BLOCK_SAND, BLOCK_STONE};

    fn a_player_with_a_pack() -> Arc<players::PlayerHandle> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        // Kept alive: a dropped receiver closes the queue and every
        // `send` below would count as a drop.
        std::mem::forget((rx, chunk_rx));
        Arc::new(players::PlayerHandle::new(
            1,
            "potter".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            (0.5, 20.0, 0.5),
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                (0.5, 20.0, 0.5),
            ),
        ))
    }

    #[test]
    fn pouring_a_jug_out_gives_back_exactly_what_went_into_it() {
        let handle = a_player_with_a_pack();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, Stack::new(BLOCK_JUG, 1));
            assert_eq!(state.inventory.add(BLOCK_GRAIN, 9), 0);
        }

        pour_into_jug(&handle, 1, 0);
        {
            let state = handle.state.lock().unwrap();
            assert_eq!(
                jug_contents(&state.inventory.slots()[0].unwrap()),
                Some((BLOCK_GRAIN, 9))
            );
            assert_eq!(
                state.inventory.count(BLOCK_GRAIN),
                0,
                "the grain was in the pack and in the jug at once"
            );
        }

        empty_jug(&handle, 0);
        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_GRAIN), 9);
        assert_eq!(state.inventory.count(BLOCK_JUG), 1, "the jug went with it");
        assert_eq!(
            jug_contents(&state.inventory.slots()[0].unwrap()),
            None,
            "the jug kept a copy"
        );
    }

    #[test]
    fn a_jug_takes_what_it_can_hold_and_leaves_the_rest_in_the_pack() {
        let handle = a_player_with_a_pack();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, Stack::new(BLOCK_JUG, 1));
            assert_eq!(state.inventory.add(BLOCK_SAND, JUG_UNITS + 7), 0);
        }

        pour_into_jug(&handle, 1, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(
            jug_contents(&state.inventory.slots()[0].unwrap()),
            Some((BLOCK_SAND, JUG_UNITS))
        );
        assert_eq!(
            state.inventory.count(BLOCK_SAND),
            7,
            "the overflow was poured into the floor"
        );
    }

    #[test]
    fn a_jug_refuses_what_would_stop_rotting_inside_it() {
        // The server asks `types::pours` again rather than trusting the
        // screen that offered the gesture -- and a client that skipped
        // the question would otherwise have a larder that stops time.
        let handle = a_player_with_a_pack();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, Stack::new(BLOCK_JUG, 1));
            assert_eq!(state.inventory.add(BLOCK_RAW_MEAT, 4), 0);
        }

        pour_into_jug(&handle, 1, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(jug_contents(&state.inventory.slots()[0].unwrap()), None);
        assert_eq!(state.inventory.count(BLOCK_RAW_MEAT), 4);
    }

    #[test]
    fn a_jug_of_grain_will_not_swallow_a_handful_of_sand() {
        // One number describes one kind of contents. Replacing them is
        // deleting them, so the pour is refused outright.
        let handle = a_player_with_a_pack();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, filled_jug(BLOCK_GRAIN, 3));
            assert_eq!(state.inventory.add(BLOCK_SAND, 5), 0);
        }

        pour_into_jug(&handle, 1, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(
            jug_contents(&state.inventory.slots()[0].unwrap()),
            Some((BLOCK_GRAIN, 3))
        );
        assert_eq!(state.inventory.count(BLOCK_SAND), 5);
    }

    #[test]
    fn tipping_a_jug_into_a_pack_with_no_room_keeps_the_goods_in_the_jug() {
        // Every slot but the jug's own is full of something else, so
        // there is nowhere for the grain to land. It stays where it is
        // rather than falling out of the world.
        let handle = a_player_with_a_pack();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, filled_jug(BLOCK_GRAIN, 6));
            for slot in 1..SLOTS {
                state
                    .inventory
                    .put_in_slot(slot, Stack::new(BLOCK_STONE, MAX_STACK));
            }
        }

        empty_jug(&handle, 0);

        let state = handle.state.lock().unwrap();
        assert_eq!(
            jug_contents(&state.inventory.slots()[0].unwrap()),
            Some((BLOCK_GRAIN, 6)),
            "the grain was tipped into nowhere"
        );
    }
}

/// A jug keeps what is in it wherever it is, and a barrel can be drunk
/// from by hand -- against the real world, the real container store and
/// the real vitals, rather than against the pure rules
/// (`inventory::jug_room`, `types::barrel_after_drinking`) they are built
/// from, because every bug these features could have is in the doing.
#[cfg(test)]
mod vessel_tests {
    use super::*;
    use primitive_shared::body::Water;
    use primitive_shared::inventory::{
        filled_jug, jug_contents, vessel_store_contents, Stack, JUG_UNITS,
    };
    use primitive_shared::types::{
        barrel_of, BARREL_DRINK_JUGS, BLOCK_AIR, BLOCK_BARREL, BLOCK_GRAIN, BLOCK_JUG,
        BLOCK_RAW_MEAT, BLOCK_SEEDS, BLOCK_STONE,
    };

    /// Where every test here puts its barrel or its jug: open air above
    /// the generated ground of chunk (0, 0), beside the player.
    const AT: (i32, i32, i32) = (3, 200, 3);

    /// A stone floor under `AT`, reaching past where any spill can run.
    fn floor_under_at(ctx: &Arc<Context>) {
        for x in AT.0 - 4..=AT.0 + 4 {
            for z in AT.2 - 4..=AT.2 + 4 {
                assert!(ctx.world.set_block(x, AT.1 - 1, z, BLOCK_STONE));
            }
        }
    }

    /// Every eighth of water on the floor round `AT`.
    fn water_round_at(ctx: &Arc<Context>) -> u32 {
        let mut total = 0;
        for x in AT.0 - 4..=AT.0 + 4 {
            for z in AT.2 - 4..=AT.2 + 4 {
                total += u32::from(primitive_shared::fluid::depth(ctx.world.cached_block(x, AT.1, z).unwrap_or(BLOCK_AIR)));
            }
        }
        total
    }

    #[test]
    fn a_barrel_of_water_broken_floods_the_floor_by_what_it_held_and_drops_empty() {
        let (ctx, _handle) = a_world_and_a_player();
        floor_under_at(&ctx);
        let full = barrel_of(Water::Fresh, 5);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_AIR));
        spawn_block_drop(&ctx, full, AT);
        assert_eq!(water_round_at(&ctx), 5 * logic::water::SPILL_EIGHTHS_PER_JUG, "the floor did not get what the barrel held");
        assert!(!ctx.spills.lock().unwrap().is_empty(), "the spill is not timed to dry");
        let items = ctx.items.lock().unwrap();
        let barrels: u32 = items.iter().filter(|item| item.block == BLOCK_BARREL).map(|item| item.count).sum();
        assert_eq!(barrels, 1, "the staves did not come back as one empty barrel");
    }

    #[test]
    fn a_jug_of_water_set_down_and_knocked_over_wets_its_cell_and_comes_back_empty() {
        use primitive_shared::types::{jug_of, BLOCK_JUG_WATER, BLOCK_SET_DOWN};
        let (ctx, _handle) = a_world_and_a_player();
        floor_under_at(&ctx);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_SET_DOWN));
        ctx.chests.lock().unwrap().edit(AT, |store| store.put_in_slot(0, Stack::new(jug_of(Water::Standing), 1)));
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_AIR));
        // Twice, as the player's break does it: once before the store is
        // spilled and once from the drop. The water goes on the floor once.
        tip_out_water(&ctx, BLOCK_SET_DOWN, AT);
        spawn_block_drop(&ctx, BLOCK_SET_DOWN, AT);
        assert_eq!(water_round_at(&ctx), logic::water::SPILL_EIGHTHS_PER_JUG, "a knocked-over jug spilled the wrong amount");
        let items = ctx.items.lock().unwrap();
        assert!(!items.iter().any(|item| primitive_shared::types::block_kind(item.block) == BLOCK_JUG_WATER), "the jug came back full");
        let jugs: Vec<_> = items.iter().filter(|item| item.block == BLOCK_JUG).collect();
        assert_eq!(jugs.len(), 1, "the jug did not come back");
        assert_eq!(jug_contents(&Stack::worn(jugs[0].block, jugs[0].count, jugs[0].damage)), None, "the empty jug holds something");
    }

    #[test]
    fn a_ctrl_click_sends_every_stack_of_one_kind_into_the_chest_and_nothing_else() {
        // "Сделай сочетания клавиш для работы в хранилищах".
        use primitive_shared::protocol::Side;
        use primitive_shared::types::{BLOCK_BERRIES, BLOCK_CHEST, BLOCK_COBBLESTONE};
        let (ctx, handle) = a_world_and_a_player();
        let chest = (AT.0 + 1, AT.1, AT.2);
        assert!(ctx.world.set_block(chest.0, chest.1, chest.2, BLOCK_CHEST));
        let cobble_slot = {
            let mut state = handle.state.lock().unwrap();
            state.inventory = primitive_shared::inventory::Inventory::new();
            let stack = primitive_shared::types::stack_limit(BLOCK_COBBLESTONE);
            state.inventory.add(BLOCK_COBBLESTONE, stack * 3);
            state.inventory.add(BLOCK_BERRIES, 5);
            (0..primitive_shared::inventory::SLOTS)
                .find(|&s| state.inventory.block_in(s) == Some(BLOCK_COBBLESTONE))
                .expect("cobble in the pack")
        };
        open_chest(&ctx, &handle, chest);
        chest_move_kind(&ctx, &handle, Side::Pack, cobble_slot as u8);
        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_COBBLESTONE), 0, "a stack of cobble stayed in the pack");
        assert_eq!(state.inventory.count(BLOCK_BERRIES), 5, "the berries went with the cobble");
        let chests = ctx.chests.lock().unwrap();
        let stored = chests.contents(chest);
        let in_chest: u32 = (0..primitive_shared::inventory::SLOTS)
            .filter(|&s| stored.block_in(s) == Some(BLOCK_COBBLESTONE))
            .map(|s| stored.count_in(s))
            .sum();
        assert_eq!(in_chest, primitive_shared::types::stack_limit(BLOCK_COBBLESTONE) * 3);
    }

    /// A loaded world and a player standing at `AT`, full of water.
    fn a_world_and_a_player() -> (Arc<Context>, Arc<players::PlayerHandle>) {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let chunk = ctx.world.generate(ChunkPos { x: 0, z: 0 });
        ctx.world.insert(chunk);
        (ctx, a_player())
    }

    fn a_player() -> Arc<players::PlayerHandle> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        // Kept alive: a dropped receiver closes the queue and every `send`
        // below would count as a drop.
        std::mem::forget((rx, chunk_rx));
        let at = (AT.0 as f32 + 0.5, AT.1 as f32, AT.2 as f32 + 0.5);
        Arc::new(players::PlayerHandle::new(
            1,
            "potter".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            primitive_shared::geometry::wide(at),
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                primitive_shared::geometry::wide(at),
            ),
        ))
    }

    fn thirsty(handle: &Arc<players::PlayerHandle>) {
        handle.state.lock().unwrap().vitals.set_hydration(0.0);
    }

    #[test]
    fn drinking_from_a_fresh_barrel_quenches_thirst_and_lowers_it_by_one_drink() {
        let (ctx, handle) = a_world_and_a_player();
        thirsty(&handle);
        let barrel = barrel_of(Water::Fresh, 4);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, barrel));

        use_barrel(&ctx, &handle, AT, barrel, None);

        assert_eq!(
            ctx.world.cached_block(AT.0, AT.1, AT.2),
            Some(barrel_of(Water::Fresh, 4 - BARREL_DRINK_JUGS)),
            "the barrel did not go down by the defined amount"
        );
        let state = handle.state.lock().unwrap();
        assert_eq!(
            state.vitals.hydration(),
            primitive_shared::body::JUG_HYDRATION,
            "a drink from a barrel was not worth the jug it cost"
        );
        assert_eq!(state.vitals.illness_owed(), 0.0, "river water from a barrel made somebody ill");
    }

    #[test]
    fn an_empty_barrel_gives_nothing_to_drink() {
        let (ctx, handle) = a_world_and_a_player();
        thirsty(&handle);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_BARREL));

        use_barrel(&ctx, &handle, AT, BLOCK_BARREL, None);

        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(BLOCK_BARREL));
        assert_eq!(handle.state.lock().unwrap().vitals.hydration(), 0.0, "water out of nothing");
    }

    #[test]
    fn a_full_player_at_a_barrel_leaves_the_water_in_it() {
        // The ordering `drink_from_barrel` is written in: the barrel goes
        // down first and comes back up when the body says no. Got wrong,
        // a sated player clicking a barrel pours it on the ground.
        let (ctx, handle) = a_world_and_a_player();
        let barrel = barrel_of(Water::Fresh, 2);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, barrel));

        use_barrel(&ctx, &handle, AT, barrel, None);

        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(barrel), "a jug was poured away");
    }

    #[test]
    fn salt_water_from_a_barrel_does_what_the_sea_does() {
        // Not "makes you thirstier by some amount": *the same* amount and
        // the same illness a swallow of the sea costs, because the barrel
        // is supposed to have no rules of its own. So the answer is
        // compared with the sea's own call on an identical body.
        let (ctx, from_barrel) = a_world_and_a_player();
        let from_the_sea = a_player();
        for handle in [&from_barrel, &from_the_sea] {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_hydration(primitive_shared::body::MAX_HYDRATION / 2.0);
            // Both unlucky, so the illness is compared rather than two dice.
            state.vitals.catch_the_next_illness();
        }
        let barrel = barrel_of(Water::Salt, 3);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, barrel));

        use_barrel(&ctx, &from_barrel, AT, barrel, None);
        from_the_sea
            .state
            .lock()
            .unwrap()
            .vitals
            .drink_water(Water::Salt, primitive_shared::body::JUG_HYDRATION);

        let barrel_state = from_barrel.state.lock().unwrap();
        let sea_state = from_the_sea.state.lock().unwrap();
        assert!(
            barrel_state.vitals.hydration() < primitive_shared::body::MAX_HYDRATION / 2.0,
            "salt from a barrel quenched a thirst"
        );
        assert_eq!(barrel_state.vitals.hydration(), sea_state.vitals.hydration());
        assert_eq!(barrel_state.vitals.illness_owed(), sea_state.vitals.illness_owed());
        assert!(barrel_state.vitals.illness_owed() > 0.0, "the sea from a barrel did not make anyone ill");
        assert_eq!(
            ctx.world.cached_block(AT.0, AT.1, AT.2),
            Some(barrel_of(Water::Salt, 3 - BARREL_DRINK_JUGS))
        );
    }

    /// A player holding `jug` in the selected slot and nothing else.
    fn holding(handle: &Arc<players::PlayerHandle>, jug: Stack) {
        let mut state = handle.state.lock().unwrap();
        state.inventory = primitive_shared::inventory::Inventory::new();
        let slot = state.selected_slot;
        state.inventory.put_in_slot(slot, jug);
    }

    fn in_hand(handle: &Arc<players::PlayerHandle>) -> Option<Stack> {
        let state = handle.state.lock().unwrap();
        state.inventory.slots()[state.selected_slot]
    }

    /// **A jug dipped in a lake takes nothing out of the world**, so it can
    /// leave no dimple: `fill_vessel` swaps the jug in the pack and never
    /// writes the cell. A jug is two litres and the smallest water the world
    /// holds is an eighth of a cube, a hundred and twenty-five (see
    /// `water::SPILL_EIGHTHS_PER_JUG` for the other direction) -- a jug that
    /// took an eighth would drain a pond sixty times faster than it should,
    /// and one that took nothing is the true answer to the nearest eighth.
    #[test]
    fn a_jug_dipped_in_a_lake_leaves_the_lake_as_it_was() {
        use primitive_shared::types::jug_of;
        let (ctx, handle) = a_world_and_a_player();
        floor_under_at(&ctx);
        let full = primitive_shared::fluid::with_depth(primitive_shared::fluid::SOURCE_DEPTH);
        for x in AT.0 - 4..=AT.0 + 4 {
            for z in AT.2 - 4..=AT.2 + 4 {
                assert!(ctx.world.set_block(x, AT.1, z, full));
            }
        }
        let before = water_round_at(&ctx);
        holding(&handle, Stack::new(BLOCK_JUG, 1));
        use_block(&ctx, &handle, AT);
        let jug = in_hand(&handle).map(|s| s.block);
        assert!(jug.is_some_and(|b| b != BLOCK_JUG && [Water::Standing, Water::Fresh, Water::Salt].iter().any(|&w| b == jug_of(w))), "the jug did not fill: {jug:?}");
        assert_eq!(water_round_at(&ctx), before, "dipping a jug took water out of the lake");
        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(full), "the cell the jug was dipped in changed");
    }

    #[test]
    fn a_full_jug_of_grain_empties_into_a_barrel_and_an_empty_jug_scoops_it_back() {
        use primitive_shared::types::barrel_goods;
        let (ctx, handle) = a_world_and_a_player();
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_BARREL));
        holding(&handle, filled_jug(BLOCK_SEEDS, JUG_UNITS));

        use_barrel(&ctx, &handle, AT, BLOCK_BARREL, Some(BLOCK_JUG));
        let poured = ctx.world.cached_block(AT.0, AT.1, AT.2).unwrap();
        assert_eq!(barrel_goods(poured), Some((BLOCK_SEEDS, 1)), "the seed did not go into the barrel");
        assert_eq!(in_hand(&handle), Some(Stack::new(BLOCK_JUG, 1)), "the jug still has seed in it");

        use_barrel(&ctx, &handle, AT, poured, Some(BLOCK_JUG));
        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(BLOCK_BARREL), "the barrel kept seed it gave out");
        assert_eq!(
            in_hand(&handle).as_ref().and_then(jug_contents),
            Some((BLOCK_SEEDS, JUG_UNITS)),
            "the scooped jug came back short"
        );
    }

    #[test]
    fn a_barrel_refuses_a_part_jug_another_grain_and_water_and_loses_nothing_doing_it() {
        use primitive_shared::types::{barrel_of_goods, jug_of, BLOCK_JUG_WATER, BLOCK_MILLET};
        let (ctx, handle) = a_world_and_a_player();
        let wheat = barrel_of_goods(BLOCK_GRAIN, 3).unwrap();
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, wheat));

        for jug in [filled_jug(BLOCK_MILLET, JUG_UNITS), filled_jug(BLOCK_GRAIN, 9)] {
            holding(&handle, jug);
            use_barrel(&ctx, &handle, AT, wheat, Some(BLOCK_JUG));
            assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(wheat), "the barrel took {jug:?}");
            assert_eq!(in_hand(&handle), Some(jug), "a refused jug was emptied anyway");
        }
        holding(&handle, Stack::new(jug_of(Water::Fresh), 1));
        use_barrel(&ctx, &handle, AT, wheat, Some(BLOCK_JUG_WATER));
        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(wheat), "water was poured onto grain");
        assert_eq!(in_hand(&handle).map(|s| s.block), Some(jug_of(Water::Fresh)));

        // A thirsty bare hand at a barrel of grain drinks nothing out of it.
        thirsty(&handle);
        holding(&handle, Stack::new(BLOCK_STONE, 1));
        use_barrel(&ctx, &handle, AT, wheat, None);
        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(wheat), "somebody drank the grain");
        assert_eq!(handle.state.lock().unwrap().vitals.hydration(), 0.0);

        // ...and grain does not go into water either.
        let river = barrel_of(Water::Fresh, 2);
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, river));
        holding(&handle, filled_jug(BLOCK_GRAIN, JUG_UNITS));
        use_barrel(&ctx, &handle, AT, river, Some(BLOCK_JUG));
        assert_eq!(ctx.world.cached_block(AT.0, AT.1, AT.2), Some(river), "grain was tipped into water");
        assert_eq!(in_hand(&handle).as_ref().and_then(jug_contents), Some((BLOCK_GRAIN, JUG_UNITS)));
    }

    #[test]
    fn a_barrel_of_grain_broken_spills_every_jug_of_it_beside_the_staves() {
        use primitive_shared::types::barrel_of_goods;
        let (ctx, _handle) = a_world_and_a_player();
        let full = barrel_of_goods(BLOCK_GRAIN, 5).unwrap();
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_AIR));
        spawn_block_drop(&ctx, full, AT);

        let items = ctx.items.lock().unwrap();
        let grain: u32 = items.iter().filter(|item| item.block == BLOCK_GRAIN).map(|item| item.count).sum();
        assert_eq!(grain, 5 * JUG_UNITS, "a broken barrel of grain lost some of it");
        let barrels: u32 = items.iter().filter(|item| item.block == BLOCK_BARREL).map(|item| item.count).sum();
        assert_eq!(barrels, 1, "the staves did not come back as one empty barrel");
    }

    #[test]
    fn a_jug_set_down_and_broken_drops_the_same_jug_of_grain() {
        let (ctx, _handle) = a_world_and_a_player();
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_JUG));
        set_down_vessel(&ctx, AT, filled_jug(BLOCK_GRAIN, 12).damage);
        {
            let chests = ctx.chests.lock().unwrap();
            assert_eq!(
                vessel_store_contents(&chests.contents(AT)),
                Some((BLOCK_GRAIN, 12)),
                "setting the jug down lost what was in it"
            );
        }

        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_AIR));
        spawn_block_drop(&ctx, BLOCK_JUG, AT);

        let items = ctx.items.lock().unwrap();
        let jugs: Vec<_> = items.iter().filter(|item| item.block == BLOCK_JUG).collect();
        assert_eq!(jugs.len(), 1, "breaking one jug dropped {} of them", jugs.len());
        assert_eq!(
            jug_contents(&Stack::worn(jugs[0].block, jugs[0].count, jugs[0].damage)),
            Some((BLOCK_GRAIN, 12)),
            "the jug came back empty"
        );
        assert!(
            !items.iter().any(|item| item.block == BLOCK_GRAIN),
            "the grain was spilled beside the jug as well as kept in it"
        );
        assert!(
            !ctx.chests.lock().unwrap().holds_anything(AT),
            "the grain stayed behind in a cell with no jug in it"
        );
    }

    #[test]
    fn a_set_down_jug_takes_a_jugs_measure_and_nothing_that_would_not_pour() {
        // Through the chest gesture a client really sends, because the
        // store's own slot limit is a stack's: without the vessel arm a
        // whole pack slot of seeds would go into a sixteen-unit jug.
        let (ctx, handle) = a_world_and_a_player();
        assert!(ctx.world.set_block(AT.0, AT.1, AT.2, BLOCK_JUG));
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, Stack::new(BLOCK_SEEDS, 40));
            state.inventory.put_in_slot(1, Stack::new(BLOCK_RAW_MEAT, 3));
        }
        open_chest(&ctx, &handle, AT);
        assert_eq!(handle.state.lock().unwrap().open_chest, Some(AT), "the jug did not open");

        chest_move(&ctx, &handle, (Side::Pack, 0), (Side::Chest, 0), false);
        chest_move(&ctx, &handle, (Side::Pack, 1), (Side::Chest, 0), false);

        assert_eq!(
            vessel_store_contents(&ctx.chests.lock().unwrap().contents(AT)),
            Some((BLOCK_SEEDS, JUG_UNITS))
        );
        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_SEEDS), 40 - JUG_UNITS, "seeds were poured into the floor");
        assert_eq!(state.inventory.count(BLOCK_RAW_MEAT), 3, "meat went into a jug on a shelf");
    }

    #[test]
    fn taking_from_a_jug_in_hand_fills_the_chosen_square_and_keeps_the_rest() {
        let handle = a_player();
        {
            let mut state = handle.state.lock().unwrap();
            state.inventory.put_in_slot(0, filled_jug(BLOCK_GRAIN, 9));
            state.inventory.put_in_slot(6, Stack::new(BLOCK_STONE, 1));
        }

        take_from_jug(&handle, 0, 5, true);
        {
            let state = handle.state.lock().unwrap();
            assert_eq!(state.inventory.slots()[5], Some(Stack::new(BLOCK_GRAIN, 5)), "half of nine is five");
            assert_eq!(jug_contents(&state.inventory.slots()[0].unwrap()), Some((BLOCK_GRAIN, 4)));
        }

        // Onto a square holding something else: that would be swapping the
        // stone into the jug, and nothing happens.
        take_from_jug(&handle, 0, 6, false);
        // ...and into the jug's own square, which is no gesture either.
        take_from_jug(&handle, 0, 0, false);
        let state = handle.state.lock().unwrap();
        assert_eq!(jug_contents(&state.inventory.slots()[0].unwrap()), Some((BLOCK_GRAIN, 4)));
        assert_eq!(state.inventory.count(BLOCK_GRAIN), 5, "grain was made or lost");
        assert_eq!(state.inventory.count(BLOCK_STONE), 1);
    }
}

/// Wear travels with the object, wherever the object goes.
#[cfg(test)]
mod wear_tests {
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{tool_durability, BLOCK_STONE_PICKAXE};

    #[test]
    fn throwing_a_worn_tool_on_the_ground_does_not_repair_it() {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget((rx, chunk_rx));
        let feet = (0.5, 20.0, 0.5);
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "butterfingers".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                feet,
            ),
        ));
        let total = tool_durability(BLOCK_STONE_PICKAXE).expect("a pick wears out");
        let half = total / 2;
        {
            let mut state = handle.state.lock().unwrap();
            state
                .inventory
                .put_in_slot(0, Stack::worn(BLOCK_STONE_PICKAXE, 1, half));
        }

        drop_from_slot(&ctx, &handle, 0, true);

        // ...and picked straight back up, through exactly the closure the
        // tick loop uses.
        let later = Instant::now() + std::time::Duration::from_secs(5);
        {
            let mut items = ctx.items.lock().unwrap();
            let mut state = handle.state.lock().unwrap();
            items.collect_near(handle.id, feet, later, |block, count, damage| {
                let left = state.inventory.add_worn(block, count, damage);
                count - left
            });
        }

        let state = handle.state.lock().unwrap();
        let back = state
            .inventory
            .slots()
            .iter()
            .flatten()
            .find(|stack| stack.block == BLOCK_STONE_PICKAXE)
            .copied()
            .expect("the pick was not picked back up");
        assert_eq!(back.damage, half, "the ground repaired the pick");
    }

    #[test]
    fn a_worn_tool_thrown_into_a_world_with_no_room_for_items_comes_back_as_worn_as_it_left() {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget((rx, chunk_rx));
        let feet = (0.5, 20.0, 0.5);
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "hoarder".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                feet,
            ),
        ));
        // The world is at its item cap: every throw from here is refused
        // and the stack has to go back into the pack.
        {
            let mut items = ctx.items.lock().unwrap();
            let now = Instant::now();
            for _ in 0..crate::items::MAX_ITEMS {
                items.spawn_worn(
                    primitive_shared::types::BLOCK_STONE,
                    1,
                    0,
                    (100.5, 20.0, 100.5),
                    (0.0, 0.0, 0.0),
                    None,
                    now,
                );
            }
        }
        let total = tool_durability(BLOCK_STONE_PICKAXE).expect("a pick wears out");
        let half = total / 2;
        {
            let mut state = handle.state.lock().unwrap();
            state
                .inventory
                .put_in_slot(0, Stack::worn(BLOCK_STONE_PICKAXE, 1, half));
        }

        assert!(!drop_from_slot(&ctx, &handle, 0, true), "a throw into a full world was accepted");

        let state = handle.state.lock().unwrap();
        let back = state
            .inventory
            .slots()
            .iter()
            .flatten()
            .find(|stack| stack.block == BLOCK_STONE_PICKAXE)
            .copied()
            .expect("the refused throw deleted the pick");
        assert_eq!(back.damage, half, "a refused throw repaired the pick");
    }

    #[cfg(feature = "mods")]
    #[test]
    fn a_mod_healing_a_dead_player_does_not_raise_them_where_they_fell() {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget((rx, chunk_rx));
        let feet = (0.5, 20.0, 0.5);
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "fallen".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                feet,
            ),
        ));
        {
            let mut state = handle.state.lock().unwrap();
            assert!(matches!(
                state.vitals.hurt(f32::MAX, "a test"),
                survival::Outcome::Died { .. }
            ));
        }

        // A regeneration aura, say, that heals everybody every tick.
        let outcome = heal_player(&ctx, &handle, 5.0);

        let state = handle.state.lock().unwrap();
        assert!(state.vitals.is_dead(), "a heal raised a corpse without a respawn");
        assert_eq!(state.vitals.health(), 0.0, "a corpse was given health");
        assert_eq!(outcome, survival::Outcome::Unchanged);
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    /// A world remembers which hour it was left at.
    ///
    /// **It did not, and nothing else about a world was treated that
    /// way.** Blocks, chests, fires, racks and profiles were all
    /// written down; the clock alone started again from
    /// `start_time_of_day` every time the world was opened. So a player
    /// who logged out at dusk came back to mid-morning, a world had no
    /// history of its own days, and the menu had nothing to show the
    /// hour of -- which is what made the light jump the moment a world
    /// opened.
    #[test]
    fn a_world_reopens_at_the_hour_it_was_left_at() {
        let dir = std::env::temp_dir().join(format!(
            "primitive-clock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("a place to save into");

        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        ctx.clock.set_time_of_day(0.83);
        let written = save_time_of_day(&ctx, &dir);
        assert!((written - 0.83).abs() < 1e-4, "it wrote {written}");

        assert!(
            load_time_of_day(&dir).is_some_and(|read| (read - 0.83).abs() < 1e-4),
            "the hour did not survive the file",
        );

        // ...and a world saved before any of this existed opens on the
        // hour a new world starts at, rather than refusing to open.
        // Every world there is until this ships is one of those.
        std::fs::remove_file(clock_path(&dir)).expect("remove the clock");
        assert_eq!(load_time_of_day(&dir), None, "a missing clock is not an hour");

        // A file somebody has edited into nonsense reads the same way.
        // A corrupted clock is not worth a save nobody can get into.
        std::fs::write(clock_path(&dir), "полдень\n").expect("write nonsense");
        assert_eq!(load_time_of_day(&dir), None, "nonsense was read as an hour");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_autosave_writes_down_the_day_and_the_carcasses_so_a_crash_does_not_rewind_them() {
        use primitive_shared::animals::{carcass_at_stage, Species};
        let dir = std::env::temp_dir().join(format!(
            "primitive-autosave-calendar-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));

        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        ctx.clock.set_time_of_day(0.61);
        {
            let mut carrion = ctx.carrion.lock().unwrap();
            carrion.on_block_changed(4, 30, 4);
            carrion.reconcile(8, |_| Some(carcass_at_stage(Species::Deer, 0)));
        }

        // Into a directory that does not exist yet, which is what a world
        // nobody has edited looks like at its first autosave.
        let (carcasses, day) = autosave_calendar(&ctx, &dir);
        assert_eq!(carcasses, Some(1), "the autosave did not write the carcass down");
        assert!(
            load_time_of_day(&dir).is_some_and(|read| (read - day).abs() < 1e-3),
            "the autosave did not write the day down"
        );
        assert!((day.rem_euclid(1.0) - 0.61).abs() < 1e-3, "it wrote the wrong hour: {day}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Things set down by hand (`BLOCK_SET_DOWN`), through the calls a player's
/// clicks take, with a real world and a real pack.
#[cfg(test)]
mod set_down_tests {
    use super::butchering_tests::{a_hunter, hold, on_the_ground, FLOOR};
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        is_set_down, BLOCK_AIR, BLOCK_BREAD, BLOCK_COPPER_KNIFE, BLOCK_PLANKS, BLOCK_STONE, BLOCK_WATER,
    };

    /// On the floor, a stride from where the hunter stands.
    const SPOT: (i32, i32, i32) = (0, FLOOR + 1, 1);

    /// A stone floor under `SPOT` and air in it.
    fn a_floor(ctx: &Arc<Context>) {
        assert!(ctx.world.set_block(SPOT.0, SPOT.1 - 1, SPOT.2, BLOCK_STONE));
        assert!(ctx.world.set_block(SPOT.0, SPOT.1, SPOT.2, BLOCK_AIR));
    }

    #[test]
    fn a_worn_knife_set_down_lies_in_its_cell_and_comes_back_to_the_hand_as_worn() {
        let (ctx, handle, _rx) = a_hunter();
        a_floor(&ctx);
        hold(&handle, Some(Stack::worn(BLOCK_COPPER_KNIFE, 1, 7)));

        set_down_item(&ctx, &handle, SPOT);

        let cell = ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2).unwrap();
        assert!(is_set_down(cell), "the knife was not set down: the cell holds {cell}");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_COPPER_KNIFE), 0, "the knife is still in the hand");
        let stored = ctx.chests.lock().unwrap().contents(SPOT);
        let lying = stored.slots().iter().flatten().copied().collect::<Vec<_>>();
        assert_eq!(lying, [Stack::worn(BLOCK_COPPER_KNIFE, 1, 7)], "what lies in the cell is not the knife that was held");

        use_block(&ctx, &handle, SPOT);

        assert_eq!(ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2), Some(BLOCK_AIR), "the knife was taken and the cell kept");
        let state = handle.state.lock().unwrap();
        let back = state.inventory.slots().iter().flatten().find(|stack| stack.block == BLOCK_COPPER_KNIFE).copied();
        assert_eq!(back, Some(Stack::worn(BLOCK_COPPER_KNIFE, 1, 7)), "the ground mended the knife, or kept it");
        assert!(ctx.chests.lock().unwrap().take(SPOT).is_none(), "a store was left behind in an empty cell");
    }

    #[test]
    fn one_loaf_of_a_stack_is_set_down_and_the_rest_stays_in_the_hand() {
        let (ctx, handle, _rx) = a_hunter();
        a_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_BREAD, 5)));

        set_down_item(&ctx, &handle, SPOT);

        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_BREAD), 4);
        assert_eq!(ctx.chests.lock().unwrap().contents(SPOT).count(BLOCK_BREAD), 1);
    }

    #[test]
    fn nothing_is_set_down_on_water_into_a_taken_cell_over_nothing_or_as_a_block() {
        let refused = |why: &str, prepare: &dyn Fn(&Arc<Context>), held: Stack| {
            let (ctx, handle, _rx) = a_hunter();
            a_floor(&ctx);
            prepare(&ctx);
            let before = ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2);
            hold(&handle, Some(held));
            set_down_item(&ctx, &handle, SPOT);
            assert_eq!(ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2), before, "{why}: the cell changed");
            assert_eq!(handle.state.lock().unwrap().inventory.count(held.block), held.count, "{why}: it was spent");
        };
        let knife = Stack::new(BLOCK_COPPER_KNIFE, 1);
        refused("on water", &|ctx| assert!(ctx.world.set_block(SPOT.0, SPOT.1, SPOT.2, BLOCK_WATER)), knife);
        refused("into stone", &|ctx| assert!(ctx.world.set_block(SPOT.0, SPOT.1, SPOT.2, BLOCK_STONE)), knife);
        refused("over air", &|ctx| assert!(ctx.world.set_block(SPOT.0, SPOT.1 - 1, SPOT.2, BLOCK_AIR)), knife);
        refused("a plank", &|_| {}, Stack::new(BLOCK_PLANKS, 3));
    }

    #[test]
    fn digging_out_the_floor_under_a_thing_set_down_drops_the_thing() {
        let (ctx, handle, _rx) = a_hunter();
        a_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_COPPER_KNIFE, 1)));
        set_down_item(&ctx, &handle, SPOT);
        assert!(ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2).is_some_and(is_set_down));

        assert!(ctx.world.set_block(SPOT.0, SPOT.1 - 1, SPOT.2, BLOCK_AIR));
        collapse_unsupported(&ctx, SPOT.0, SPOT.1 - 1, SPOT.2);

        assert_eq!(ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2), Some(BLOCK_AIR));
        assert_eq!(on_the_ground(&ctx), [(BLOCK_COPPER_KNIFE, 1)], "the knife went with the floor");
    }

    #[test]
    fn a_thing_set_down_that_rots_to_nothing_leaves_its_cell_empty() {
        // The rot clock is the one hand that can empty a store, and every
        // change to a store ends at `broadcast_chest_state`.
        let (ctx, handle, _rx) = a_hunter();
        a_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_BREAD, 1)));
        set_down_item(&ctx, &handle, SPOT);
        ctx.chests.lock().unwrap().edit(SPOT, |store| store.take_slot(0));

        broadcast_chest_state(&ctx, SPOT);

        assert_eq!(ctx.world.cached_block(SPOT.0, SPOT.1, SPOT.2), Some(BLOCK_AIR), "an empty set-down cell stayed");
    }

    #[test]
    fn a_thing_set_down_is_not_a_chest_to_open() {
        let (ctx, handle, _rx) = a_hunter();
        a_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_COPPER_KNIFE, 1)));
        set_down_item(&ctx, &handle, SPOT);
        let feet = handle.state.lock().unwrap().position;
        assert!(!chest_in_use(&ctx, primitive_shared::geometry::narrow(feet), SPOT), "a knife on the floor opened like a chest");
    }

    // ---- rats at what was set down ----
    //
    // A thing set down keeps itself in the container store (`set_down_item`),
    // so the raid that goes through a larder goes through the floor as well:
    // these check the whole of that path, from the hand to the cell every
    // client is told about, rather than the raid's arithmetic on its own.

    /// A cell two strides off the hunter with a stone floor, walls and a
    /// roof on every side, so it is as dark as a pantry at night -- and a
    /// cell under the open sky is not, which is the lamp rule a raid keeps.
    const PANTRY: (i32, i32, i32) = (2, FLOOR + 1, 2);

    fn a_pantry_floor(ctx: &Arc<Context>) {
        for y in FLOOR + 1..primitive_shared::types::CHUNK_SIZE_Y as i32 {
            assert!(ctx.world.set_block(PANTRY.0, y, PANTRY.2, BLOCK_AIR));
        }
        assert!(ctx.world.set_block(PANTRY.0, PANTRY.1 - 1, PANTRY.2, BLOCK_STONE));
    }

    /// Walls and a roof round `PANTRY`, put up after the thing is set down
    /// (setting down is a reach and not a sight line, but a closed box has
    /// no top to set anything on).
    fn close_the_pantry(ctx: &Arc<Context>) {
        let (x, y, z) = PANTRY;
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)] {
            assert!(ctx.world.set_block(x + dx, y + dy, z + dz, BLOCK_STONE));
        }
        let mut lights = std::collections::HashMap::new();
        assert!(
            light_for_vermin(ctx, &mut lights, PANTRY) < primitive_shared::vermin::LIGHT_KEEPS_THEM_OUT,
            "the test pantry is lit, so nothing in it could ever be raided"
        );
    }

    fn a_rat_beside_the_pantry(ctx: &Arc<Context>) {
        let at = (PANTRY.0 as f32 + 0.5, PANTRY.1 as f32, PANTRY.2 as f32 + 1.5);
        let born = ctx.animals.lock().unwrap().spawn_at(primitive_shared::animals::Species::Rat, at);
        assert!(born.is_some(), "no rat would come");
    }

    /// `seconds` of the server's vermin clock, a second at a time, with the
    /// hunter standing where they stand -- and every container it changed
    /// told about, the way the tick loop tells it.
    fn let_the_night_run(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, vermin: &mut logic::vermin::Vermin, seconds: u32) {
        let feet = primitive_shared::geometry::narrow(handle.state.lock().unwrap().position);
        for _ in 0..seconds {
            for at in step_vermin(ctx, vermin, &[(handle.id, feet)], 1.0) {
                broadcast_chest_state(ctx, at);
            }
        }
    }

    #[test]
    fn a_rat_beside_meat_set_down_in_a_dark_room_eats_it_within_two_raids() {
        use primitive_shared::types::BLOCK_COOKED_MEAT;
        let (ctx, handle, _rx) = a_hunter();
        ctx.clock.set_time_of_day(0.0);
        a_pantry_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_COOKED_MEAT, 1)));
        set_down_item(&ctx, &handle, PANTRY);
        assert!(ctx.world.cached_block(PANTRY.0, PANTRY.1, PANTRY.2).is_some_and(is_set_down), "the meat was never set down");
        close_the_pantry(&ctx);
        a_rat_beside_the_pantry(&ctx);

        let mut vermin = logic::vermin::Vermin::new();
        let_the_night_run(&ctx, &handle, &mut vermin, 2 * logic::vermin::PASS_SECONDS as u32 + 1);

        // The cell, not only the store: the block is what every client draws,
        // and an eaten haunch left as an empty set-down cell would be an
        // invisible thing nobody could put anything in.
        assert_eq!(ctx.world.cached_block(PANTRY.0, PANTRY.1, PANTRY.2), Some(BLOCK_AIR), "the meat is still on the floor");
        assert_eq!(ctx.chests.lock().unwrap().contents(PANTRY).count(BLOCK_COOKED_MEAT), 0);
    }

    #[test]
    fn dried_meat_set_down_beside_a_rat_comes_down_a_rung_and_stays_where_it_lay() {
        use primitive_shared::types::{BLOCK_COOKED_MEAT, BLOCK_DRIED_MEAT};
        let (ctx, handle, _rx) = a_hunter();
        ctx.clock.set_time_of_day(0.0);
        a_pantry_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_DRIED_MEAT, 1)));
        set_down_item(&ctx, &handle, PANTRY);
        close_the_pantry(&ctx);
        a_rat_beside_the_pantry(&ctx);

        let mut vermin = logic::vermin::Vermin::new();
        let_the_night_run(&ctx, &handle, &mut vermin, logic::vermin::PASS_SECONDS as u32 + 1);

        assert!(ctx.world.cached_block(PANTRY.0, PANTRY.1, PANTRY.2).is_some_and(is_set_down), "the last piece was eaten, not spoiled");
        // What a client is told lies there is what the store holds now.
        assert!(
            matches!(set_down_item_message(&ctx, PANTRY), Some(ServerMessage::SetDownItem { item: BLOCK_COOKED_MEAT, .. })),
            "the floor does not show the spoiled strip as cooked meat"
        );
    }

    #[test]
    fn a_knife_set_down_beside_a_rat_is_left_where_it_lay() {
        let (ctx, handle, _rx) = a_hunter();
        ctx.clock.set_time_of_day(0.0);
        a_pantry_floor(&ctx);
        hold(&handle, Some(Stack::worn(BLOCK_COPPER_KNIFE, 1, 3)));
        set_down_item(&ctx, &handle, PANTRY);
        close_the_pantry(&ctx);
        a_rat_beside_the_pantry(&ctx);

        let mut vermin = logic::vermin::Vermin::new();
        let_the_night_run(&ctx, &handle, &mut vermin, 3 * logic::vermin::PASS_SECONDS as u32);

        assert!(ctx.world.cached_block(PANTRY.0, PANTRY.1, PANTRY.2).is_some_and(is_set_down), "the knife went");
        let lying = ctx.chests.lock().unwrap().contents(PANTRY).slots().iter().flatten().copied().collect::<Vec<_>>();
        assert_eq!(lying, [Stack::worn(BLOCK_COPPER_KNIFE, 1, 3)], "a rat chewed a knife");
    }

    #[test]
    fn meat_set_down_in_daylight_wilderness_with_no_rat_about_is_untouched() {
        use primitive_shared::types::BLOCK_COOKED_MEAT;
        let (ctx, handle, _rx) = a_hunter();
        ctx.clock.set_time_of_day(0.5);
        a_pantry_floor(&ctx);
        hold(&handle, Some(Stack::new(BLOCK_COOKED_MEAT, 1)));
        set_down_item(&ctx, &handle, PANTRY);

        let mut vermin = logic::vermin::Vermin::new();
        let_the_night_run(&ctx, &handle, &mut vermin, 5 * logic::vermin::PASS_SECONDS as u32);

        assert!(ctx.animals.lock().unwrap().where_the(primitive_shared::animals::Species::Rat).is_empty(), "a rat came out by day");
        assert_eq!(ctx.chests.lock().unwrap().contents(PANTRY).count(BLOCK_COOKED_MEAT), 1, "the meat went with nothing to take it");
    }
}

/// Kelp on the rack of two by two, the way a player does it: the screen
/// opened by a click, the frond shifted onto the frame, the sun, and the
/// strip taken off -- through the calls those gestures take, with a real
/// world, so a refusal anywhere on the way is the test going red.
#[cfg(test)]
mod kelp_rack_tests {
    use super::butchering_tests::{a_hunter, hold, FLOOR};
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::rack::{hanging_index, HIDE_SLOT, LEATHER_SLOT};
    use primitive_shared::types::{
        rack_cells, rack_column_goods, Facing, BLOCK_AIR, BLOCK_DRIED_KELP, BLOCK_KELP_FROND, BLOCK_STONE,
    };

    /// The rack's near bottom cell, a stride from the hunter.
    const RACK: (i32, i32, i32) = (2, FLOOR + 1, 2);

    /// What the near and the far column of the rack show, as `rack::HANGING`
    /// rows -- what a player sees from across the camp.
    fn columns(ctx: &Arc<Context>, cells: &[((i32, i32, i32), primitive_shared::types::BlockId); 4]) -> (u8, u8) {
        let block = |i: usize| {
            let (x, y, z) = cells[i].0;
            ctx.world.cached_block(x, y, z).expect("a rack cell nobody has loaded")
        };
        (rack_column_goods(block(0), block(1)), rack_column_goods(block(2), block(3)))
    }

    #[test]
    fn a_hide_frame_shows_its_skin_until_the_leather_is_taken_up() {
        // **"Is it dry yet?" is what a frame is looked at to answer.** A lone
        // frame showed its skin while the skin was raw and went bare the
        // moment it cured -- the leather drops into the tray -- so it looked
        // empty exactly when there was something on it to collect. Raw, the
        // frame is loaded; cured with the leather waiting, it is loaded and
        // says so (`types::HIDE_CURED`); taken up, it is bare.
        use primitive_shared::types::{faced, hide_is_cured, rack_is_loaded, BLOCK_HIDE, BLOCK_HIDE_FRAME, BLOCK_LEATHER};
        let (ctx, _handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(RACK.0, RACK.1, RACK.2, faced(BLOCK_HIDE_FRAME, Facing::East)));
        let frame = |ctx: &Arc<Context>| ctx.world.cached_block(RACK.0, RACK.1, RACK.2).expect("the frame's cell is loaded");
        let set = |ctx: &Arc<Context>, hide: Option<Stack>, leather: Option<Stack>| {
            ctx.chests.lock().unwrap().edit(RACK, |contents| {
                contents.take_slot(HIDE_SLOT);
                contents.take_slot(LEATHER_SLOT);
                if let Some(stack) = hide {
                    contents.put_in_slot(HIDE_SLOT, stack);
                }
                if let Some(stack) = leather {
                    contents.put_in_slot(LEATHER_SLOT, stack);
                }
            });
            refresh_rack_block(ctx, RACK);
        };

        set(&ctx, Some(Stack::new(BLOCK_HIDE, 1)), None);
        assert!(rack_is_loaded(frame(&ctx)) && !hide_is_cured(frame(&ctx)), "a raw skin: {:#x}", frame(&ctx));
        set(&ctx, None, Some(Stack::new(BLOCK_LEATHER, 1)));
        assert!(rack_is_loaded(frame(&ctx)) && hide_is_cured(frame(&ctx)), "the leather waiting: {:#x}", frame(&ctx));
        set(&ctx, Some(Stack::new(BLOCK_HIDE, 1)), Some(Stack::new(BLOCK_LEATHER, 1)));
        assert!(rack_is_loaded(frame(&ctx)) && !hide_is_cured(frame(&ctx)), "a second skin drying: {:#x}", frame(&ctx));
        set(&ctx, None, None);
        assert!(!rack_is_loaded(frame(&ctx)) && !hide_is_cured(frame(&ctx)), "taken up: {:#x}", frame(&ctx));
        assert_eq!(primitive_shared::types::block_facing(frame(&ctx)), Facing::East, "the skin turned the frame round");
    }

    #[test]
    fn kelp_hung_on_the_rack_dries_in_the_sun_and_comes_off_as_dried_kelp() {
        let (ctx, handle, _rx) = a_hunter();
        ctx.clock.set_time_of_day(0.5);
        let cells = rack_cells(RACK, Facing::North);
        // Open sky over the whole rack and a stone floor under it: the sun
        // is half of what is being tested.
        for &((x, _, z), _) in &cells {
            assert!(ctx.world.set_block(x, FLOOR, z, BLOCK_STONE));
            for y in FLOOR + 1..primitive_shared::types::CHUNK_SIZE_Y as i32 {
                assert!(ctx.world.set_block(x, y, z, BLOCK_AIR));
            }
        }
        for &((x, y, z), id) in &cells {
            assert!(ctx.world.set_block(x, y, z, id));
        }
        hold(&handle, Some(Stack::new(BLOCK_KELP_FROND, 3)));

        // The click on the rack -- its top far cell, which opens the frame
        // at its anchor -- and the shift-click from the hand.
        open_chest(&ctx, &handle, cells[3].0);
        assert_eq!(handle.state.lock().unwrap().open_chest, Some(RACK), "the rack did not open");
        chest_quick_move(&ctx, &handle, Side::Pack, 0);
        let hung = ctx.chests.lock().unwrap().contents(RACK);
        assert_eq!(hung.block_in(HIDE_SLOT), Some(BLOCK_KELP_FROND), "the rack refused the kelp");
        assert_eq!(hung.count_in(HIDE_SLOT), 3);
        let fronds = hanging_index(BLOCK_KELP_FROND);
        assert_ne!(fronds, 0, "the ridge has no picture for kelp");
        assert_eq!(columns(&ctx, &cells), (fronds, fronds), "the kelp is on the rack and not seen on it");

        // Fair weather at noon, a second at a time, the way the tick steps it.
        ctx.drying.lock().unwrap().set_progress(RACK, 0.99);
        let fires = crate::logic::fire::Fires::new();
        let mut finished = Vec::new();
        for _ in 0..3600 {
            let stepped = {
                let mut chests = ctx.chests.lock().unwrap();
                let mut racks = ctx.drying.lock().unwrap();
                racks.step(&ctx.world, &fires, &mut chests, primitive_shared::weather::Weather::Clear, 0.5, 1.0)
            };
            for &at in &stepped.finished {
                refresh_rack_block(&ctx, at);
            }
            finished.extend(stepped.finished);
            if !finished.is_empty() {
                break;
            }
        }
        assert_eq!(finished, vec![RACK], "the kelp never dried in the sun");
        let dried = ctx.chests.lock().unwrap().contents(RACK);
        assert_eq!(dried.block_in(LEATHER_SLOT), Some(BLOCK_DRIED_KELP), "what came off is not dried kelp");
        assert_eq!(dried.count_in(HIDE_SLOT), 2);
        assert_eq!(
            columns(&ctx, &cells),
            (fronds, hanging_index(BLOCK_DRIED_KELP)),
            "the far end of the ridge does not show the dried strip"
        );

        // ...and off the tray into the pack, by a shift-click on it.
        chest_quick_move(&ctx, &handle, Side::Chest, LEATHER_SLOT as u8);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_DRIED_KELP), 1, "the dried kelp would not come off");
        assert_eq!(ctx.chests.lock().unwrap().contents(RACK).block_in(LEATHER_SLOT), None);
    }
}

/// Torches lit at a fire and fires struck with flint, through the call a
/// player's click takes, with a real world and a real pack.
#[cfg(test)]
mod fire_gesture_tests {
    use super::butchering_tests::{a_hunter, hold, FLOOR};
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_FLINT, BLOCK_TORCH, BLOCK_TORCH_LIT,
    };

    /// On the floor, a stride from where the hunter stands.
    const FIRE: (i32, i32, i32) = (0, FLOOR + 1, 1);

    #[test]
    fn a_torch_held_to_a_burning_campfire_comes_away_lit() {
        let (ctx, handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_CAMPFIRE_LIT));
        hold(&handle, Some(Stack::new(BLOCK_TORCH, 3)));

        use_block(&ctx, &handle, FIRE);

        let state = handle.state.lock().unwrap();
        assert_eq!(state.inventory.count(BLOCK_TORCH_LIT), 1, "the torch did not light at a burning fire");
        assert_eq!(state.inventory.count(BLOCK_TORCH), 2, "more than one torch was taken from the pile");
        drop(state);
        assert_eq!(
            ctx.world.cached_block(FIRE.0, FIRE.1, FIRE.2),
            Some(BLOCK_CAMPFIRE_LIT),
            "lighting a torch put the fire out"
        );
    }

    #[test]
    fn a_torch_at_a_cold_campfire_stays_unlit() {
        let (ctx, handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_CAMPFIRE));
        hold(&handle, Some(Stack::new(BLOCK_TORCH, 1)));
        use_block(&ctx, &handle, FIRE);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_TORCH_LIT), 0);
    }

    #[test]
    fn striking_a_campfire_spends_one_nodule_and_a_refused_strike_spends_none() {
        let (ctx, handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_CAMPFIRE));
        hold(&handle, Some(Stack::new(BLOCK_FLINT, 3)));

        use_block(&ctx, &handle, FIRE);
        assert_eq!(ctx.world.cached_block(FIRE.0, FIRE.1, FIRE.2), Some(BLOCK_CAMPFIRE_LIT), "the fire did not catch");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_FLINT), 2, "the strike that lit it kept the flint");

        // The same strike at the fire now burning lights nothing, and so
        // costs nothing.
        use_block(&ctx, &handle, FIRE);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_FLINT), 2, "a strike at a lit fire spent flint");
    }

    #[test]
    fn a_knife_peels_bark_off_a_birch_and_a_willow_and_resin_off_the_rest() {
        use primitive_shared::types::{
            BLOCK_BIRCH_BARK, BLOCK_BIRCH_LOG, BLOCK_FLINT_KNIFE, BLOCK_RESIN, BLOCK_WILLOW_BARK, BLOCK_WILLOW_LOG,
        };
        for (trunk, yields) in [(BLOCK_BIRCH_LOG, BLOCK_BIRCH_BARK), (BLOCK_WILLOW_LOG, BLOCK_WILLOW_BARK)] {
            let (ctx, handle, _rx) = a_hunter();
            assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, trunk));
            hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
            use_block(&ctx, &handle, FIRE);
            let pack = &handle.state.lock().unwrap().inventory;
            assert_eq!(pack.count(yields), 1, "the trunk gave no bark");
            assert_eq!(pack.count(BLOCK_RESIN), 0, "a hardwood bled resin");
        }
    }

    #[test]
    fn a_knife_scores_a_standing_trunk_for_resin_once_until_it_heals() {
        use primitive_shared::types::{oriented, Axis, BLOCK_FLINT_KNIFE, BLOCK_LOG, BLOCK_RESIN};
        let (ctx, handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_LOG));
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        use_block(&ctx, &handle, FIRE);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_RESIN), 1, "a scored trunk gave no resin");
        assert_eq!(ctx.world.cached_block(FIRE.0, FIRE.1, FIRE.2), Some(BLOCK_LOG), "tapping changed the trunk");
        use_block(&ctx, &handle, FIRE);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_RESIN), 1, "the same wound bled twice");
        // A log lying on the ground is deadfall, not a trunk that bleeds.
        let lying = (FIRE.0, FIRE.1 + 1, FIRE.2);
        assert!(ctx.world.set_block(lying.0, lying.1, lying.2, oriented(BLOCK_LOG, Axis::X)));
        use_block(&ctx, &handle, lying);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_RESIN), 1, "deadfall gave resin");
    }

    #[test]
    fn resin_relights_a_burnt_out_standing_torch_from_either_cell() {
        use primitive_shared::types::{BLOCK_RESIN, BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_LIT, BLOCK_STANDING_TORCH_OUT};
        let (ctx, handle, _rx) = a_hunter();
        let top = (FIRE.0, FIRE.1 + 1, FIRE.2);
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_STANDING_TORCH));
        assert!(ctx.world.set_block(top.0, top.1, top.2, BLOCK_STANDING_TORCH_OUT));
        hold(&handle, Some(Stack::new(BLOCK_RESIN, 2)));
        use_block(&ctx, &handle, FIRE);
        assert_eq!(ctx.world.cached_block(top.0, top.1, top.2), Some(BLOCK_STANDING_TORCH_LIT), "resin at the pole did not light the top");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_RESIN), 1);
        use_block(&ctx, &handle, top);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_RESIN), 1, "a burning torch took more resin");
    }

    #[test]
    fn ash_dug_into_a_furrow_dresses_it_once() {
        use primitive_shared::types::{BLOCK_ASH, BLOCK_FARMLAND};
        let (ctx, handle, _rx) = a_hunter();
        assert!(ctx.world.set_block(FIRE.0, FIRE.1, FIRE.2, BLOCK_FARMLAND));
        hold(&handle, Some(Stack::new(BLOCK_ASH, 2)));
        use_block(&ctx, &handle, FIRE);
        let field = ctx.world.cached_block(FIRE.0, FIRE.1, FIRE.2).unwrap();
        assert!(primitive_shared::wildfire::is_dressed(field), "the ash did not go into the furrow");
        use_block(&ctx, &handle, FIRE);
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_ASH), 1, "a dressed furrow took ash again");
    }

    #[test]
    fn dung_cleared_off_a_tired_furrow_is_dug_into_it() {
        use primitive_shared::types::{BLOCK_FARMLAND, BLOCK_STONE};
        use primitive_shared::wildfire::{after_harvest, is_dressed, is_tired};
        // A pat can land on a furrow at all: `leave_dung` wants ground that
        // holds a roof under the cell it drops into.
        assert!(primitive_shared::types::blocks_the_sky(after_harvest(BLOCK_FARMLAND).unwrap()));
        let (ctx, _handle, _rx) = a_hunter();
        let furrow = (FIRE.0, FIRE.1 - 1, FIRE.2);
        assert!(ctx.world.set_block(furrow.0, furrow.1, furrow.2, after_harvest(BLOCK_FARMLAND).unwrap()));
        manure_the_furrow_under(&ctx, FIRE);
        let field = ctx.world.cached_block(furrow.0, furrow.1, furrow.2).unwrap();
        assert!(is_dressed(field) && !is_tired(field), "the muck did not go into the furrow");
        // ...and on anything that is not a furrow it does nothing.
        assert!(ctx.world.set_block(furrow.0, furrow.1, furrow.2, BLOCK_STONE));
        manure_the_furrow_under(&ctx, FIRE);
        assert_eq!(ctx.world.cached_block(furrow.0, furrow.1, furrow.2), Some(BLOCK_STONE));
    }
}

/// Apples come off a tree into the hand with a right click, and out of a
/// tree that is felled -- on the server, through the calls a player's
/// click and a player's axe take, with a real world and a real pack.
#[cfg(test)]
mod picking_tests {
    use super::butchering_tests::{a_hunter, errors, hold, on_the_ground, FLOOR};
    use super::*;
    use primitive_shared::inventory::{Stack, SLOTS};
    use primitive_shared::types::{
        BLOCK_AIR, BLOCK_APPLE, BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT,
        BLOCK_APPLE_LEAVES_PICKED, BLOCK_BREAD, BLOCK_LOG, BLOCK_STONE,
    };

    /// The cell over the hunter's column, a stride from where they stand
    /// and well inside a punch's reach.
    const IN_THE_TREE: (i32, i32, i32) = (0, FLOOR + 2, 0);

    fn apples(handle: &Arc<players::PlayerHandle>) -> u32 {
        handle.state.lock().unwrap().inventory.count(BLOCK_APPLE)
    }

    #[test]
    fn a_right_click_on_apples_puts_them_in_the_pack_and_leaves_the_leaves_standing() {
        // **What the player asked for, in one test**: the apple is in the
        // pack, the leaves are still in the tree -- as the leaf that grows
        // apples again -- and nothing was knocked to the ground. Bread in
        // hand, because the server must not care what the hand holds.
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_BREAD, 1)));
        let (x, y, z) = IN_THE_TREE;
        assert!(ctx.world.set_block(x, y, z, BLOCK_APPLE_LEAVES_FRUIT));

        use_block(&ctx, &handle, IN_THE_TREE);

        assert_eq!(apples(&handle), 1, "the apple did not reach the pack");
        assert_eq!(
            ctx.world.cached_block(x, y, z),
            Some(BLOCK_APPLE_LEAVES_PICKED),
            "picking the apple did not leave the leaves standing, ready to fruit again"
        );
        assert!(
            on_the_ground(&ctx).is_empty(),
            "the apple was knocked down rather than picked: {:?}",
            on_the_ground(&ctx)
        );
        assert_eq!(
            handle.state.lock().unwrap().inventory.count(BLOCK_BREAD),
            1,
            "the bread in hand was spent on a pick"
        );

        // ...and the same click on the leaves it left is nothing at all.
        use_block(&ctx, &handle, IN_THE_TREE);
        assert_eq!(apples(&handle), 1, "a second apple came out of picked leaves");
        assert_eq!(ctx.world.cached_block(x, y, z), Some(BLOCK_APPLE_LEAVES_PICKED));
    }

    #[test]
    fn a_full_pack_leaves_the_apples_on_the_tree_and_says_so() {
        let (ctx, handle, mut rx) = a_hunter();
        {
            let mut state = handle.state.lock().unwrap();
            for slot in 0..SLOTS {
                let _ = state.inventory.put_in_slot(slot, Stack::new(BLOCK_STONE, 1));
            }
        }
        let (x, y, z) = IN_THE_TREE;
        assert!(ctx.world.set_block(x, y, z, BLOCK_APPLE_LEAVES_FRUIT));

        use_block(&ctx, &handle, IN_THE_TREE);

        assert_eq!(
            ctx.world.cached_block(x, y, z),
            Some(BLOCK_APPLE_LEAVES_FRUIT),
            "the tree was picked bare into a pack with no room"
        );
        assert_eq!(apples(&handle), 0);
        assert!(on_the_ground(&ctx).is_empty(), "the apple was dropped instead of left on the tree");
        let said = errors(&mut rx);
        assert!(
            said.iter().any(|text| text == "PackFull"),
            "the player was not told why nothing came off: {said:?}"
        );
    }

    #[test]
    fn about_one_iron_cell_in_eight_is_a_lodestone_and_always_the_same_one() {
        use primitive_shared::types::{BLOCK_IRON_ORE, BLOCK_LODESTONE, BLOCK_STONE};
        let cells: Vec<(i32, i32, i32)> =
            (0..40).flat_map(|x| (0..40).map(move |z| (x - 20, 30 + (x * z) % 7, z - 20))).collect();
        let found = cells.iter().filter(|&&at| lodestone_in(BLOCK_IRON_ORE, at).is_some()).count();
        let share = found as f32 / cells.len() as f32;
        assert!((0.08..0.17).contains(&share), "{found} lodestones in {} iron cells", cells.len());
        for &at in &cells {
            assert_eq!(lodestone_in(BLOCK_IRON_ORE, at), lodestone_in(BLOCK_IRON_ORE, at), "{at:?} answered twice differently");
            assert_eq!(lodestone_in(BLOCK_STONE, at), None, "plain stone at {at:?} gave a lodestone");
        }
        assert!(cells.iter().any(|&at| lodestone_in(BLOCK_IRON_ORE, at) == Some(BLOCK_LODESTONE)));
    }

    #[test]
    fn a_felled_apple_tree_drops_the_apples_that_were_on_it() {
        // A clearing of stone with a trunk of five in it, a crown of apple
        // leaves, and two apples in the crown. The foot is cut the way an
        // axe leaves it, and the tree comes down.
        let (ctx, _handle, _rx) = a_hunter();
        let (x, z) = (6, 6);
        for cx in x - 3..=x + 3 {
            for cz in z - 3..=z + 3 {
                for y in FLOOR - 1..=FLOOR + 12 {
                    let block = if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR };
                    assert!(ctx.world.set_block(cx, y, cz, block));
                }
            }
        }
        for y in FLOOR + 2..=FLOOR + 6 {
            assert!(ctx.world.set_block(x, y, z, BLOCK_LOG));
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
            assert!(ctx.world.set_block(x + dx, FLOOR + 5, z + dz, BLOCK_APPLE_LEAVES));
        }
        assert!(ctx.world.set_block(x + 1, FLOOR + 6, z, BLOCK_APPLE_LEAVES_FRUIT));
        assert!(ctx.world.set_block(x - 1, FLOOR + 6, z, BLOCK_APPLE_LEAVES_FRUIT));
        assert!(ctx.world.set_block(x, FLOOR + 7, z, BLOCK_APPLE_LEAVES));

        let felled = fell_tree(&ctx, (x, FLOOR + 1, z), None);

        assert!(felled > 0, "the tree did not come down");
        for (cx, cz) in [(x + 1, z), (x - 1, z)] {
            assert_eq!(
                ctx.world.cached_block(cx, FLOOR + 6, cz),
                Some(BLOCK_AIR),
                "an apple was left hanging over the stump"
            );
        }
        let apples_down: u32 = on_the_ground(&ctx)
            .iter()
            .filter(|&&(block, _)| block == BLOCK_APPLE)
            .map(|&(_, count)| count)
            .sum();
        assert_eq!(
            apples_down,
            2,
            "a felled tree with two apples on it left {apples_down} on the ground: {:?}",
            on_the_ground(&ctx)
        );
    }

    /// A clearing of stone round (x, z) with the generator's palm on it --
    /// variant 3, nine pieces and both steps, leaning toward +x -- and where
    /// each of its cells went.
    fn a_palm_on_stone(ctx: &Arc<Context>, (x, z): (i32, i32)) -> Vec<((i32, i32, i32), primitive_shared::types::BlockId)> {
        for cx in x - 6..=x + 6 {
            for cz in z - 6..=z + 6 {
                for y in FLOOR - 1..=FLOOR + 14 {
                    let block = if y <= FLOOR { primitive_shared::types::BLOCK_STONE } else { primitive_shared::types::BLOCK_AIR };
                    assert!(ctx.world.set_block(cx, y, cz, block));
                }
            }
        }
        let cells: Vec<_> = primitive_shared::worldgen::palm_cells(3, (1, 0))
            .into_iter()
            .map(|((dx, dy, dz), id)| ((x + dx, FLOOR + dy, z + dz), id))
            .collect();
        for &((cx, cy, cz), id) in &cells {
            assert!(ctx.world.set_block(cx, cy, cz, id));
        }
        cells
    }

    fn is_trunk(id: primitive_shared::types::BlockId) -> bool {
        primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_PALM_TRUNK
    }

    #[test]
    fn a_palm_crown_comes_down_when_its_heart_or_the_top_of_its_trunk_is_broken() {
        // "пальмовые листья не осыпаются после уничтожения основы листа".
        // Both ways the crown loses its footing, each on a palm of its own:
        // the heart over the trunk broken, and the top piece under the heart
        // broken. Either way not one frond or cluster is left in the air, the
        // trunk below the break stands, and the coconuts are on the ground.
        for breaks_the_heart in [true, false] {
            let (ctx, _handle, _rx) = a_hunter();
            let cells = a_palm_on_stone(&ctx, (7, 7));
            let top = cells
                .iter()
                .filter(|(_, id)| is_trunk(*id))
                .map(|(at, _)| *at)
                .max_by_key(|at| at.1)
                .expect("a palm with no trunk");
            let heart = (top.0, top.1 + 1, top.2);
            assert!(
                cells.iter().any(|(at, id)| *at == heart && !is_trunk(*id)),
                "the generator's palm has no crown cell over its trunk"
            );
            let broken = if breaks_the_heart { heart } else { top };
            // What the break path has done by the time it asks: the cell is air.
            assert!(ctx.world.set_block(broken.0, broken.1, broken.2, primitive_shared::types::BLOCK_AIR));

            let fell = drop_unheld_palm_crown(&ctx, broken);

            let what = if breaks_the_heart { "the heart" } else { "the top of the trunk" };
            assert!(fell > 0, "breaking {what} brought nothing down");
            for &((x, y, z), id) in &cells {
                if (x, y, z) == broken {
                    continue;
                }
                let now = ctx.world.cached_block(x, y, z);
                if is_trunk(id) {
                    assert_eq!(now, Some(id), "breaking {what} took the trunk at {:?}", (x, y, z));
                } else {
                    assert_eq!(
                        now,
                        Some(primitive_shared::types::BLOCK_AIR),
                        "breaking {what} left crown block {id} hanging at {:?}",
                        (x, y, z)
                    );
                }
            }
            let coconuts: u32 = on_the_ground(&ctx)
                .iter()
                .filter(|&&(block, _)| block == primitive_shared::types::BLOCK_COCONUT)
                .map(|&(_, count)| count)
                .sum();
            assert!(coconuts >= 2, "breaking {what} shook down {coconuts} coconuts from two clusters");
        }
    }

    #[test]
    fn breaking_one_frond_of_a_palm_leaves_the_rest_of_its_crown_standing() {
        // The other side of the rule: the heart still sits on its trunk, so a
        // frond's tip torn off is that tip and nothing more.
        let (ctx, _handle, _rx) = a_hunter();
        let cells = a_palm_on_stone(&ctx, (7, 7));
        let tip = cells
            .iter()
            .filter(|(_, id)| primitive_shared::types::block_kind(*id) == primitive_shared::types::BLOCK_PALM_FRONDS)
            .map(|(at, _)| *at)
            .max_by_key(|at| at.0)
            .expect("a crown with no fronds");
        assert!(ctx.world.set_block(tip.0, tip.1, tip.2, primitive_shared::types::BLOCK_AIR));

        assert_eq!(drop_unheld_palm_crown(&ctx, tip), 0, "one frond torn off brought more of the crown down");
        for &((x, y, z), id) in &cells {
            if (x, y, z) != tip {
                assert_eq!(ctx.world.cached_block(x, y, z), Some(id), "the crown lost {:?}", (x, y, z));
            }
        }
    }
}

/// An animal dies into a carcass, and a carcass comes apart under a
/// knife -- on the server, through the same calls a player's click
/// takes, with a real world and a real pack.
#[cfg(test)]
mod butchering_tests {
    use super::*;
    use primitive_shared::animals::{butchering_stage, carcass_at_stage, Species};
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        block_kind, is_air, tool_durability, BlockId, BLOCK_AIR, BLOCK_CARCASS_DEER,
        BLOCK_STONE_AXE, BLOCK_FLINT_KNIFE, BLOCK_HIDE, BLOCK_RAW_MEAT, BLOCK_STONE,
        BLOCK_TALL_GRASS, BLOCK_TORCH, BLOCK_WATER,
    };

    /// The column the tests happen in: stone at 19, air above.
    pub(super) const FLOOR: i32 = 19;

    /// A world with the origin chunk loaded and a player standing two
    /// blocks from the column, with their pack and their outgoing queue
    /// in the test's hands.
    pub(super) fn a_hunter() -> (
        Arc<Context>,
        Arc<players::PlayerHandle>,
        tokio::sync::mpsc::Receiver<players::Outgoing>,
    ) {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        // The chunk has to be *cached*, because `use_block` and
        // `lay_carcass` read the cache and refuse a cell nobody has
        // loaded -- which is what a real server does for a cell no
        // player is near.
        let chunk = ctx.world.generate(ChunkPos { x: 0, z: 0 });
        ctx.world.insert(chunk);
        for y in 0..primitive_shared::types::CHUNK_SIZE_Y as i32 {
            let block = if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR };
            assert!(ctx.world.set_block(0, y, 0, block));
        }
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget(chunk_rx);
        let feet = (0.5, f64::from(FLOOR) + 1.0, 2.5);
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "hunter".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(
                crate::settings::AntiCheatSettings::default(),
                8,
                feet,
            ),
        ));
        (ctx, handle, rx)
    }

    pub(super) fn hold(handle: &Arc<players::PlayerHandle>, stack: Option<Stack>) {
        let mut state = handle.state.lock().unwrap();
        state.selected_slot = 0;
        match stack {
            Some(stack) => {
                state.inventory.put_in_slot(0, stack);
            }
            None => {
                state.inventory.take_from(0, u32::MAX);
            }
        }
    }

    /// Everything lying on the ground, as (block, count), in spawn order.
    pub(super) fn on_the_ground(ctx: &Arc<Context>) -> Vec<(BlockId, u32)> {
        let items = ctx.items.lock().unwrap();
        items.iter().map(|item| (item.block, item.count)).collect()
    }

    /// Swings at `id` until it dies, resetting the swing cooldown
    /// between blows the way a second of waiting would -- and then lets the
    /// body finish going down, which is when its carcass is laid.
    fn kill(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, id: primitive_shared::protocol::EntityId) {
        strike_down(ctx, handle, id);
        let_it_fall(ctx);
    }

    /// Swings at `id` until it dies, and no further: the body is still
    /// falling (`animals::FALL_SECONDS`).
    fn strike_down(ctx: &Arc<Context>, handle: &Arc<players::PlayerHandle>, id: primitive_shared::protocol::EntityId) {
        for _ in 0..50 {
            handle.state.lock().unwrap().last_swing = None;
            attack_animal(ctx, handle, id);
            if ctx.animals.lock().unwrap().is_empty() {
                return;
            }
        }
        panic!("fifty blows and it is still standing");
    }

    /// The tick loop's part of a death, for as long as a fall takes: the
    /// bodies go down, and each that lands is laid as the loop lays it.
    pub(super) fn let_it_fall(ctx: &Arc<Context>) {
        let mut elapsed = 0.0;
        while elapsed < animals::FALL_SECONDS + 0.2 {
            let fallen = {
                let mut animals = ctx.animals.lock().unwrap();
                animals.step(&*ctx.world, &[], 0.05, 0.5);
                animals.take_fallen()
            };
            for death in fallen {
                lay_carcass(ctx, death);
            }
            elapsed += 0.05;
        }
    }

    /// Everything the player was told: an `Error`'s English, and a
    /// `Notice` by its code's name (`notice`), which is what a test can
    /// hold now that the words are the client's.
    pub(super) fn errors(rx: &mut tokio::sync::mpsc::Receiver<players::Outgoing>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            match msg {
                players::Outgoing::Message(ServerMessage::Error(text)) => out.push(text),
                players::Outgoing::Message(ServerMessage::Notice { what }) => out.push(format!("{what:?}")),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn a_spear_kill_lays_its_carcass_once_and_only_after_the_body_has_fallen() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Deer, (0.5, FLOOR as f32 + 1.0, 0.5))
            .expect("a deer");
        strike_down(&ctx, &handle, id);
        // Dead, and still going down: nothing in the cell yet, and the body is
        // still what everybody is sent.
        assert!(is_air(ctx.world.cached_block(0, FLOOR + 1, 0).expect("loaded")), "the carcass came before the fall");
        assert!(ctx.animals.lock().unwrap().states().iter().any(|s| s.id == id), "the body vanished at the blow");
        let_it_fall(&ctx);
        assert_eq!(block_kind(ctx.world.cached_block(0, FLOOR + 1, 0).expect("loaded")), BLOCK_CARCASS_DEER);
        // ...and once: more ticks lay nothing more.
        let_it_fall(&ctx);
        assert_eq!(ctx.metrics.block_edits.load(Ordering::Relaxed), 1, "the carcass was laid twice");
        assert!(on_the_ground(&ctx).is_empty(), "a heap came with the carcass");
    }

    #[test]
    fn a_killed_deer_leaves_a_carcass_where_it_fell_rather_than_a_heap() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Deer, (0.5, FLOOR as f32 + 1.0, 0.5))
            .expect("a deer");

        kill(&ctx, &handle, id);

        let cell = ctx.world.cached_block(0, FLOOR + 1, 0).expect("loaded");
        assert_eq!(block_kind(cell), BLOCK_CARCASS_DEER, "no carcass at its feet");
        assert_eq!(butchering_stage(cell), 0, "a fresh carcass has had nothing cut");
        assert!(
            on_the_ground(&ctx).is_empty(),
            "the old heap turned up beside the carcass: {:?}",
            on_the_ground(&ctx)
        );
        // ...and it is a block edit like any other: counted, and
        // therefore saved.
        assert_eq!(ctx.metrics.block_edits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_animal_that_dies_over_water_still_leaves_its_meat() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        // A pool four deep where the floor was, so there is no ground
        // within three cells of the hare's feet.
        for y in (FLOOR - 3)..=(FLOOR + 1) {
            assert!(ctx.world.set_block(0, y, 0, BLOCK_WATER));
        }
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Hare, (0.5, FLOOR as f32 + 1.0, 0.5))
            .expect("a hare");

        kill(&ctx, &handle, id);

        for y in (FLOOR - 4)..=(FLOOR + 2) {
            let block = ctx.world.cached_block(0, y, 0).expect("loaded");
            assert!(
                Species::of_carcass(block).is_none(),
                "a carcass was laid in water at y={y}"
            );
        }
        assert_eq!(
            on_the_ground(&ctx),
            Species::Hare.drops().to_vec(),
            "the fallback heap is the old drop list"
        );
    }

    #[test]
    fn lying_down_stops_the_world_moving_you_and_getting_up_starts_it_again() {
        // **The state a player must never be stuck in.** Sleep takes
        // their controls away -- the server stops reading transforms
        // entirely -- so the way *out* has to be something a sleeping
        // client can still do. It is the same right click on the same
        // bed, which is why this is a toggle rather than a refusal.
        let (ctx, handle, _rx) = a_hunter();
        let at = (0, FLOOR + 1, 0);
        assert!(ctx
            .world
            .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_BED));

        use_block(&ctx, &handle, at);
        assert_eq!(
            handle.state.lock().unwrap().sleeping_in,
            Some(at),
            "a right click on a bed did not put the player in it"
        );
        use_block(&ctx, &handle, at);
        assert_eq!(
            handle.state.lock().unwrap().sleeping_in,
            None,
            "the same click did not get them up again"
        );
    }

    /// Health lost to `count` stings once the venom has run its course.
    fn after_the_venom(count: u8) -> f32 {
        let (_ctx, handle, _rx) = a_hunter();
        let full = handle.state.lock().unwrap().vitals.health();
        sting_player(&handle, count);
        for _ in 0..120 {
            let _ = handle.state.lock().unwrap().vitals.sicken(1.0);
        }
        let state = handle.state.lock().unwrap();
        assert!(!state.vitals.is_dead(), "{count} stings killed a healthy player");
        full - state.vitals.health()
    }

    #[test]
    fn a_player_with_smoke_at_the_hive_is_stung_less_than_one_without() {
        use primitive_shared::bees::{hive_holding, stings, HIVE_FULL, SMOKE_REACH};
        use primitive_shared::types::{BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_TORCH, BLOCK_TORCH_LIT};
        let (ctx, _handle, _rx) = a_hunter();
        let hive = (8, FLOOR + 6, 8);
        for dy in -SMOKE_REACH..=SMOKE_REACH {
            for dz in -SMOKE_REACH..=SMOKE_REACH {
                for dx in -SMOKE_REACH..=SMOKE_REACH {
                    assert!(ctx.world.set_block(hive.0 + dx, hive.1 + dy, hive.2 + dz, BLOCK_AIR));
                }
            }
        }
        assert!(ctx.world.set_block(hive.0, hive.1, hive.2, hive_holding(HIVE_FULL)));
        assert!(!hive_is_smoked(&ctx, hive, None), "a hive in clear air was smoked");
        assert!(!hive_is_smoked(&ctx, hive, Some(BLOCK_TORCH)), "an unlit torch smoked a hive");
        assert!(hive_is_smoked(&ctx, hive, Some(BLOCK_TORCH_LIT)), "a torch in the hand did not smoke the hive");
        let under = (hive.0, hive.1 - SMOKE_REACH, hive.2);
        assert!(ctx.world.set_block(under.0, under.1, under.2, BLOCK_CAMPFIRE));
        assert!(!hive_is_smoked(&ctx, hive, None), "a cold fire smoked the hive");
        assert!(ctx.world.set_block(under.0, under.1, under.2, BLOCK_CAMPFIRE_LIT));
        assert!(hive_is_smoked(&ctx, hive, None), "a fire under the tree did not smoke the hive");

        // ...and what that is worth, in health, on a robbed hive in summer.
        let robbed = hive_holding(0);
        let bare = after_the_venom(stings(robbed, false, 25.0));
        let smoked = after_the_venom(stings(robbed, true, 25.0));
        assert!(smoked < bare, "smoke saved nothing: {smoked} against {bare}");
        assert!(smoked > 0.0, "a smoked raid cost nothing at all");
    }

    #[test]
    fn stings_never_kill_a_healthy_player_in_one_raid_and_never_take_the_last_of_anybodys_health() {
        use primitive_shared::bees::{hive_holding, stings, HIVE_FULL};
        // The worst raid there is: the empty comb taken apart, bare-handed, in
        // the heat. Under a third of a healthy player, venom included.
        let worst = (0..=HIVE_FULL).map(|h| stings(hive_holding(h), false, 35.0)).max().unwrap();
        let lost = after_the_venom(worst);
        assert!(lost < survival::MAX_HEALTH / 3.0, "the worst raid took {lost} health");
        assert!(lost >= 2.0, "the worst raid took only {lost}: nobody would bring a torch");

        // ...and a player already low is held at the floor, venom and all.
        let (_ctx, handle, _rx) = a_hunter();
        let to_three = handle.state.lock().unwrap().vitals.health() - 3.0;
        let _ = handle.state.lock().unwrap().vitals.hurt(to_three, "a test");
        sting_player(&handle, worst);
        sting_player(&handle, worst);
        for _ in 0..120 {
            let _ = handle.state.lock().unwrap().vitals.sicken(1.0);
        }
        let health = handle.state.lock().unwrap().vitals.health();
        assert!(health >= BEE_STING_FLOOR - 1e-3, "two raids took a weak player to {health}");
    }

    /// A stone floor three by three at the origin with air over it, so a
    /// bed has a second cell, a sleeper has somewhere beside it to stand,
    /// and nothing the generator happened to put there decides a test.
    #[test]
    fn a_nettle_stings_a_bare_hand_and_not_a_knife_and_never_takes_the_last_of_a_players_health() {
        use primitive_shared::types::{BLOCK_FLINT_KNIFE, BLOCK_NETTLE, BLOCK_TALL_GRASS};
        let (_ctx, handle, _rx) = a_hunter();
        let health = || handle.state.lock().unwrap().vitals.health();
        let full = health();
        sting_from_nettle(&handle, BLOCK_NETTLE, None);
        assert_eq!(health(), full - NETTLE_STING, "a nettle pulled up by hand did not sting");
        sting_from_nettle(&handle, BLOCK_NETTLE, Some(BLOCK_FLINT_KNIFE));
        assert_eq!(health(), full - NETTLE_STING, "a nettle cut with a knife stung");
        sting_from_nettle(&handle, BLOCK_TALL_GRASS, None);
        assert_eq!(health(), full - NETTLE_STING, "a tuft of grass stung");
        // Read before the lock is taken to hurt: the guard lives for the whole
        // statement, and asking `health()` inside it locks the same mutex again.
        let down_to_two = health() - NETTLE_STING * 2.0;
        let _ = handle.state.lock().unwrap().vitals.hurt(down_to_two, "a test");
        let low = health();
        sting_from_nettle(&handle, BLOCK_NETTLE, None);
        assert_eq!(health(), low, "a nettle took the last of a player's health");
    }

    fn a_bare_room(ctx: &Arc<Context>) {
        for x in 0..3 {
            for z in 0..3 {
                for y in (FLOOR - 1)..(FLOOR + 5) {
                    let block = if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR };
                    assert!(ctx.world.set_block(x, y, z, block));
                }
            }
        }
    }

    #[test]
    fn a_bed_broken_at_either_end_takes_its_other_half_and_gives_one_bed() {
        // **One item in, one item out, and never half a bed left behind.**
        // A bed is placed as two cells from one bed (`types::BED_HEAD`);
        // breaking the half a player swung at gives the bed, and the other
        // half must go with it and give nothing -- or a bed broken at its
        // foot left a pillow on the floor that slept as well as a whole one.
        use primitive_shared::types::{bed_half, bed_partner, block_kind, Facing, BLOCK_BED};
        for break_head in [false, true] {
            let (ctx, _handle, _rx) = a_hunter();
            a_bare_room(&ctx);
            let foot_at = (1, FLOOR + 1, 0);
            let foot = bed_half(Facing::North, false);
            let (head_at, head) = bed_partner(foot_at, foot).unwrap();
            assert!(ctx.world.set_block(foot_at.0, foot_at.1, foot_at.2, foot));
            assert!(ctx.world.set_block(head_at.0, head_at.1, head_at.2, head));

            let (at, broken) = if break_head { (head_at, head) } else { (foot_at, foot) };
            assert!(ctx.world.set_block(at.0, at.1, at.2, BLOCK_AIR));
            spawn_block_drop(&ctx, broken, at);
            break_bed_partner(&ctx, at, broken);

            for cell in [foot_at, head_at] {
                assert_eq!(
                    ctx.world.cached_block(cell.0, cell.1, cell.2),
                    Some(BLOCK_AIR),
                    "breaking the {} left half a bed at {cell:?}",
                    if break_head { "head" } else { "foot" }
                );
            }
            let beds: u32 = on_the_ground(&ctx)
                .iter()
                .filter(|(block, _)| block_kind(*block) == BLOCK_BED)
                .map(|(_, count)| count)
                .sum();
            assert_eq!(beds, 1, "a broken bed gave {beds} beds");
        }
    }

    #[test]
    fn a_door_comes_down_whole_from_either_half_or_its_floor_and_gives_one_door() {
        // Three ways a door stops being a door -- a swing at the bottom, a
        // swing at the top, the floor dug out from under it -- and each has
        // to take both halves and pay out one door. The third paid two: the
        // collapse read the column before it changed and wrote air over the
        // top half its partner rule had already taken (`collapse_unsupported`).
        use primitive_shared::types::{block_kind, door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        for way in ["bottom", "top", "floor"] {
            for open in [false, true] {
                let (ctx, _handle, _rx) = a_hunter();
                a_bare_room(&ctx);
                let at = (1, FLOOR + 1, 1);
                let mut lower = faced(BLOCK_DOOR, Facing::South);
                if open {
                    lower = door_swung(lower);
                }
                let (top_at, top) = door_partner(at, lower).unwrap();
                assert!(ctx.world.set_block(at.0, at.1, at.2, lower));
                assert!(ctx.world.set_block(top_at.0, top_at.1, top_at.2, top));
                match way {
                    "floor" => {
                        assert!(ctx.world.set_block(at.0, FLOOR, at.2, BLOCK_AIR));
                        collapse_unsupported(&ctx, at.0, FLOOR, at.2);
                    }
                    _ => {
                        let (cell, broken) = if way == "top" { (top_at, top) } else { (at, lower) };
                        assert!(ctx.world.set_block(cell.0, cell.1, cell.2, BLOCK_AIR));
                        spawn_block_drop(&ctx, broken, cell);
                        break_bed_partner(&ctx, cell, broken);
                    }
                }
                for cell in [at, top_at] {
                    assert_eq!(ctx.world.cached_block(cell.0, cell.1, cell.2), Some(BLOCK_AIR), "{way}, open {open}: half a door left at {cell:?}");
                }
                let doors: u32 = on_the_ground(&ctx)
                    .iter()
                    .filter(|(block, _)| block_kind(*block) == BLOCK_DOOR)
                    .map(|(_, count)| count)
                    .sum();
                assert_eq!(doors, 1, "{way}, open {open}: a fallen door gave {doors} doors");
            }
        }
    }

    #[test]
    fn a_right_click_at_either_half_swings_the_whole_door_and_nothing_but_its_own_half() {
        // The swing takes the clicked half's partner and nothing else: a
        // stranger's half over it -- shut over an open bottom -- is left as
        // it is.
        use primitive_shared::types::{door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        let (ctx, handle, _rx) = a_hunter();
        a_bare_room(&ctx);
        let at = (1, FLOOR + 1, 1);
        let lower = faced(BLOCK_DOOR, Facing::East);
        let (top_at, top) = door_partner(at, lower).unwrap();
        assert!(ctx.world.set_block(at.0, at.1, at.2, lower));
        assert!(ctx.world.set_block(top_at.0, top_at.1, top_at.2, top));
        let hanging = |ctx: &Arc<Context>| (ctx.world.cached_block(at.0, at.1, at.2), ctx.world.cached_block(top_at.0, top_at.1, top_at.2));
        use_block(&ctx, &handle, top_at);
        assert_eq!(hanging(&ctx), (Some(door_swung(lower)), Some(door_swung(top))), "a click at the top did not open both halves");
        use_block(&ctx, &handle, at);
        assert_eq!(hanging(&ctx), (Some(lower), Some(top)), "a click at the bottom did not shut both halves");
        // A top that is not this bottom's partner stays put.
        assert!(ctx.world.set_block(top_at.0, top_at.1, top_at.2, door_swung(top)));
        use_block(&ctx, &handle, at);
        assert_eq!(hanging(&ctx), (Some(door_swung(lower)), Some(door_swung(top))), "a stranger's half swung with the door");
    }

    #[test]
    fn a_sleeper_lies_across_the_whole_bed_from_either_end_and_gets_up_beside_it() {
        // Three things that were each wrong. The body was put in the middle
        // of the *cell* clicked, so on a bed two cells long it lay with its
        // head over the floor. Getting up left it there, on the mattress --
        // with a headboard through its shoulders against a wall. And the
        // toggle compared the cell clicked, so the foot of the bed you were
        // asleep in put you to sleep a second time.
        use primitive_shared::types::{bed_half, bed_partner, Facing};
        let (ctx, handle, _rx) = a_hunter();
        a_bare_room(&ctx);
        let foot_at = (1, FLOOR + 1, 0);
        let foot = bed_half(Facing::North, false);
        let (head_at, head) = bed_partner(foot_at, foot).unwrap();
        assert!(ctx.world.set_block(foot_at.0, foot_at.1, foot_at.2, foot));
        assert!(ctx.world.set_block(head_at.0, head_at.1, head_at.2, head));

        use_block(&ctx, &handle, foot_at);
        let (position, yaw, sleeping) = {
            let state = handle.state.lock().unwrap();
            (state.position, state.yaw, state.sleeping_in)
        };
        assert_eq!(sleeping, Some(head_at), "the bed is not known by its head");
        let middle = (1.5, (foot_at.2 + head_at.2) as f32 * 0.5 + 0.5);
        assert!(
            (position.0 - middle.0).abs() < 1e-3 && (position.2 - f64::from(middle.1)).abs() < 1e-3,
            "a sleeper lies at {position:?}, not across the seam at {middle:?}"
        );
        let toward_head = ((head_at.0 - foot_at.0) as f32, (head_at.2 - foot_at.2) as f32);
        assert!(
            yaw.cos() * toward_head.0 + yaw.sin() * toward_head.1 > 0.99,
            "a sleeper's head points along {yaw}, not toward the head of the bed"
        );

        // The other end of the same bed is the same bed.
        use_block(&ctx, &handle, head_at);
        let (position, sleeping) = {
            let state = handle.state.lock().unwrap();
            (state.position, state.sleeping_in)
        };
        assert_eq!(sleeping, None, "the head of the bed you are asleep in did not get you up");
        let stood_in = (position.0.floor() as i32, position.1.floor() as i32, position.2.floor() as i32);
        assert!(
            stood_in != foot_at && stood_in != head_at,
            "got up at {position:?}, inside the bed"
        );
        assert_eq!(position.1, f64::from((FLOOR + 1) as f32), "got up standing on nothing, or on the bed");
        assert!(body_fits(&ctx, primitive_shared::geometry::narrow(position)), "got up inside something at {position:?}");
    }

    #[test]
    fn sitting_puts_you_on_the_seat_and_the_same_click_stands_you_up() {
        // Sitting used to be a claim and a line of chat: the player stood
        // where they had clicked from and nobody, themselves included, saw
        // them sit. The body goes onto the seat now -- and a stool with a
        // shelf over it, where a seated body would be inside the shelf, is
        // refused rather than sat in.
        use primitive_shared::types::BLOCK_STOOL;
        let (ctx, handle, mut rx) = a_hunter();
        a_bare_room(&ctx);
        let at = (1, FLOOR + 1, 1);
        assert!(ctx.world.set_block(at.0, at.1, at.2, BLOCK_STOOL));

        use_block(&ctx, &handle, at);
        let (position, seat) = {
            let state = handle.state.lock().unwrap();
            (state.position, state.sitting_on)
        };
        assert_eq!(seat, Some(at), "a click on a stool did not sit the player on it");
        assert_eq!(
            position,
            (1.5, f64::from((FLOOR + 1) as f32 + primitive_shared::types::collision_height(BLOCK_STOOL)), 1.5),
            "the sitter is not on the seat"
        );
        use_block(&ctx, &handle, at);
        assert_eq!(handle.state.lock().unwrap().sitting_on, None, "the same click did not stand them up");

        // A shelf at head height over the stool.
        assert!(ctx.world.set_block(at.0, at.1 + 2, at.2, BLOCK_STONE));
        let _ = errors(&mut rx);
        use_block(&ctx, &handle, at);
        assert_eq!(handle.state.lock().unwrap().sitting_on, None, "sat with a stone through the head");
        assert!(
            errors(&mut rx).iter().any(|e| e == "NoRoomToSit"),
            "a refused seat said nothing"
        );
    }

    #[test]
    fn a_night_that_starves_a_sleeper_to_death_is_a_death_and_not_immortality() {
        // The report: lay down, felt a blow, woke at zero health and could
        // not be hurt again. The night's hunger killed the sleeper and the
        // death was never reported. Here the body goes to bed nearly spent,
        // the stomach and the waterskin run out overnight, and the night
        // has to end in a `Died` the client can put a screen up for.
        let (ctx, handle, mut rx) = a_hunter();
        let at = (0, FLOOR + 1, 0);
        assert!(ctx
            .world
            .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_BED));
        ctx.clock.set_time_of_day(0.8);
        {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_nourishment(1.0);
            state.vitals.set_hydration(1.0);
        }
        use_block(&ctx, &handle, at);
        assert!(handle.state.lock().unwrap().sleeping_in.is_some(), "a fed player could not lie down");
        {
            // Nearly spent, and hurt a while ago rather than just now, so
            // nothing about the night wakes them before it is over.
            let mut state = handle.state.lock().unwrap();
            state.vitals.hurt(survival::MAX_HEALTH - 0.5, "a test");
        }
        while rx.try_recv().is_ok() {}

        sleep_through_to_dawn(&ctx, std::slice::from_ref(&handle));

        assert!(handle.state.lock().unwrap().vitals.is_dead(), "a night with nothing to eat left 0.5 health");
        let mut died = false;
        while let Ok(msg) = rx.try_recv() {
            died |= matches!(msg, players::Outgoing::Message(ServerMessage::Died { .. }));
        }
        assert!(died, "the sleeper died in the night and the client was never told");
    }

    #[test]
    fn the_night_waits_for_every_sleepers_screen_to_go_dark_and_passes_once() {
        // **The report**: "при сне игрок сразу встает и не засыпает". The
        // night passed on the first tick everybody was in bed -- the tick
        // after lying down, in singleplayer -- and stood the sleeper up. It
        // must wait out the fade, and a sleeper the morning left lying in
        // bed must not sleep the next day through on the tick after.
        use primitive_shared::body::NIGHT_PASSES_AFTER_SECONDS;
        let (ctx, handle, _rx) = a_hunter();
        let handles = [Arc::clone(&handle)];
        assert!(!night_may_pass(&handles), "the night passed with nobody asleep");
        let at = (0, FLOOR + 1, 0);
        assert!(ctx
            .world
            .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_BED));
        ctx.clock.set_time_of_day(0.8);
        use_block(&ctx, &handle, at);
        assert!(handle.state.lock().unwrap().sleeping_in.is_some(), "could not lie down");
        assert!(
            !night_may_pass(&handles),
            "the night passed the moment the sleeper lay down, before the screen could go dark"
        );

        handle.state.lock().unwrap().asleep_since = Some(
            std::time::Instant::now() - std::time::Duration::from_secs_f32(NIGHT_PASSES_AFTER_SECONDS + 0.1),
        );
        assert!(night_may_pass(&handles), "a sleeper whose screen is dark still waits for the night");
        sleep_through_to_dawn(&ctx, &handles);
        assert_eq!(handle.state.lock().unwrap().sleeping_in, Some(at), "morning stood the sleeper up");
        assert!(
            !night_may_pass(&handles),
            "a sleeper left lying at dawn would sleep the next day through as well"
        );
    }

    #[test]
    fn nobody_can_lie_down_on_an_empty_stomach_or_an_empty_waterskin() {
        for (food, water, says, code) in [(0.0, 50.0, "hungry", "TooHungryToSleep"), (50.0, 0.0, "thirsty", "TooThirstyToSleep")] {
            let (ctx, handle, mut rx) = a_hunter();
            let at = (0, FLOOR + 1, 0);
            assert!(ctx
                .world
                .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_BED));
            {
                let mut state = handle.state.lock().unwrap();
                state.vitals.set_nourishment(food);
                state.vitals.set_hydration(water);
            }
            use_block(&ctx, &handle, at);
            assert_eq!(handle.state.lock().unwrap().sleeping_in, None, "slept while {says}");
            assert!(
                errors(&mut rx).iter().any(|e| e == code),
                "the refusal to sleep while {says} said nothing about it"
            );
        }
    }

    #[test]
    fn a_night_in_a_bed_takes_the_tiredness_and_charges_for_the_hours() {
        // The bargain sleep makes, in one test: the night is *lived*
        // rather than skipped. Tiredness goes because that is what a
        // bed is for; hunger and thirst go down as well, because a
        // night that fed you would make a bed the answer to hunger.
        let (ctx, handle, _rx) = a_hunter();
        let at = (0, FLOOR + 1, 0);
        assert!(ctx
            .world
            .set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_BED));
        // Dusk, so there is a night to sleep through.
        ctx.clock.set_time_of_day(0.8);
        {
            let mut state = handle.state.lock().unwrap();
            state.vitals.set_fatigue(1.0);
        }
        use_block(&ctx, &handle, at);
        let (fed_before, watered_before) = {
            let state = handle.state.lock().unwrap();
            (state.vitals.nourishment(), state.vitals.hydration())
        };

        sleep_through_to_dawn(&ctx, std::slice::from_ref(&handle));

        let state = handle.state.lock().unwrap();
        // Woken, and left in the bed: the player gets up when they choose.
        assert_eq!(state.sleeping_in, Some(at), "morning stood the sleeper up out of their bed");
        assert!(state.asleep_since.is_none(), "morning came and the sleeper is still asleep");
        assert_eq!(state.vitals.fatigue(), 0.0, "a whole night in a bed left tiredness");
        assert!(
            state.vitals.nourishment() < fed_before,
            "the night was free: {} against {fed_before}",
            state.vitals.nourishment()
        );
        assert!(
            state.vitals.hydration() < watered_before,
            "nobody got thirsty overnight"
        );
        // ...and it is morning, which is the whole visible half of it.
        let hour = ctx.clock.time_of_day();
        assert!(
            (hour - 0.25).abs() < 0.01,
            "the clock woke up at {hour} rather than at dawn"
        );
    }

    #[test]
    fn the_paste_comes_off_the_spear_on_the_thrust_that_lands_and_not_before() {
        // **What "one use" means, and where it is spent.** The fly
        // agaric lives in the weapon's own id; the server takes it off
        // when a blow lands, because the server is the only thing that
        // knows a blow landed. A miss must not spend it -- two
        // toadstools lost to a misjudged reach is the sort of thing a
        // player never forgives -- and the spear must keep its wear,
        // which is why this goes through `Inventory::retype_slot`
        // rather than through a take and an add.
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        let (ctx, handle, _rx) = a_hunter();
        let poisoned = primitive_shared::types::poisoned(BLOCK_FLINT_SPEAR);
        hold(&handle, Some(Stack::new(poisoned, 1)));
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Hare, (0.5, FLOOR as f32 + 1.0, 40.0))
            .expect("a hare");

        // Out of reach: the swing lands on nothing.
        handle.state.lock().unwrap().last_swing = None;
        attack_animal(&ctx, &handle, id);
        assert_eq!(
            handle.state.lock().unwrap().inventory.block_in(0),
            Some(poisoned),
            "a swing at nothing wiped the paste off"
        );

        // ...and in reach, it is spent -- and the weapon is still the
        // same weapon, worn by exactly the one thrust.
        {
            let mut animals = ctx.animals.lock().unwrap();
            let hare = animals.find_mut_for_test(id).expect("the hare");
            hare.position = (0.5, f64::from(FLOOR as f32 + 1.0), 0.5);
        }
        handle.state.lock().unwrap().last_swing = None;
        attack_animal(&ctx, &handle, id);
        let state = handle.state.lock().unwrap();
        let stack = state.inventory.slots()[0].expect("the spear is still there");
        assert_eq!(stack.block, BLOCK_FLINT_SPEAR, "the paste is still on it");
        assert_eq!(stack.damage, 1, "the thrust did not wear the point");
    }

    #[test]
    fn a_spear_cannot_strike_twice_within_a_second() {
        // The player's second, on the side that decides it. The client
        // waits all of it before starting another thrust; what is held
        // here is that a client which did not wait gets nothing for it,
        // and that one which did is not refused.
        use primitive_shared::combat::{COOLDOWN_SLACK_SECS, SPEAR_COOLDOWN_SECS};
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        use std::time::{Duration, Instant};
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_SPEAR, 1)));
        // A bear, because it lives through a thrust: every blow that lands
        // wears the point by one, so the wear is the count of blows.
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Bear, (0.5, FLOOR as f32 + 1.0, 0.5))
            .expect("a bear");
        let landed = || handle.state.lock().unwrap().inventory.slots()[0].expect("the spear").damage;
        let swung_ago = |seconds: f32| {
            let then = Instant::now()
                .checked_sub(Duration::from_secs_f32(seconds))
                .expect("a clock that has run for a second");
            handle.state.lock().unwrap().last_swing = Some(then);
        };

        handle.state.lock().unwrap().last_swing = None;
        attack_animal(&ctx, &handle, id);
        assert_eq!(landed(), 1, "the first thrust did not land");

        for early in [0.1, 0.5, SPEAR_COOLDOWN_SECS - COOLDOWN_SLACK_SECS - 0.02] {
            swung_ago(early);
            attack_animal(&ctx, &handle, id);
            assert_eq!(landed(), 1, "a second thrust {early}s after the first was taken");
        }

        swung_ago(SPEAR_COOLDOWN_SECS);
        attack_animal(&ctx, &handle, id);
        assert_eq!(landed(), 2, "a thrust a whole second after the last was refused");
    }

    #[test]
    fn a_broken_turned_block_drops_the_plain_item_that_stacks_with_unplaced_ones() {
        // A kiln faced east is a kiln with two bits set. What comes out of
        // it has to be the kiln in the crafting menu, or a camp built
        // facing every way fills a pack with four kinds of kiln.
        use primitive_shared::types::{block_facing, faced, Facing, BLOCK_KILN};
        let (ctx, _handle, _rx) = a_hunter();
        let turned = faced(BLOCK_KILN, Facing::East);
        assert_eq!(block_facing(turned), Facing::East, "the kiln did not turn");
        spawn_block_drop(&ctx, turned, (0, FLOOR + 1, 0));
        let dropped = on_the_ground(&ctx);
        assert_eq!(dropped, vec![(BLOCK_KILN, 1)], "a kiln that faced east dropped something else");

        let mut pack = primitive_shared::inventory::Inventory::new();
        assert_eq!(pack.add(BLOCK_KILN, 1), 0);
        assert_eq!(pack.add(dropped[0].0, dropped[0].1), 0);
        let stacks: Vec<_> = pack.slots().iter().flatten().collect();
        assert_eq!(stacks.len(), 1, "a kiln that was put down and one that was not take two slots");
        assert_eq!(stacks[0].count, 2);
    }

    #[test]
    fn a_knife_takes_a_carcass_apart_one_click_at_a_time_and_the_last_cut_takes_it_away() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        let at = (0, FLOOR + 1, 0);
        assert!(ctx.world.set_block(at.0, at.1, at.2, carcass_at_stage(Species::Deer, 0)));

        let cuts = Species::Deer.butchering();
        for (stage, &cut) in cuts.iter().enumerate() {
            use_block(&ctx, &handle, at);
            let taken = on_the_ground(&ctx);
            assert_eq!(taken.len(), stage + 1, "click {stage} did not yield exactly one cut");
            assert_eq!(*taken.last().unwrap(), cut, "click {stage} took the wrong thing");
            let cell = ctx.world.cached_block(at.0, at.1, at.2).unwrap();
            if stage + 1 < cuts.len() {
                assert_eq!(block_kind(cell), BLOCK_CARCASS_DEER, "the carcass vanished early");
                assert_eq!(butchering_stage(cell), stage + 1);
            } else {
                assert!(is_air(cell), "the last cut left something behind");
            }
        }
        // A click on the empty cell afterwards is nothing at all.
        use_block(&ctx, &handle, at);
        assert_eq!(on_the_ground(&ctx).len(), cuts.len());
        // ...and every cut wore the knife by one.
        let state = handle.state.lock().unwrap();
        let knife = state.inventory.slots()[0].expect("the knife is still there");
        assert_eq!(knife.damage, cuts.len() as u32);
    }

    /// A cut is thrown clear of the carcass towards whoever made it:
    /// it starts above the lying body and comes down on the butcher's
    /// side, a step away. One that appeared at the middle of the cell
    /// sat inside the model and read as a rendering fault.
    #[test]
    fn a_cut_lands_at_the_butchers_feet_and_not_inside_the_carcass() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        // The hunter stands two cells south (+z) of the carcass, and
        // the cell between them is open ground: the fixture clears one
        // column only, and natural terrain is not.
        let at = (0, FLOOR + 1, 0);
        assert!(ctx.world.set_block(0, FLOOR, 1, BLOCK_STONE));
        for y in FLOOR + 1..FLOOR + 4 {
            assert!(ctx.world.set_block(0, y, 1, BLOCK_AIR));
        }
        assert!(ctx.world.set_block(at.0, at.1, at.2, carcass_at_stage(Species::Deer, 0)));
        use_block(&ctx, &handle, at);

        let mut items = ctx.items.lock().unwrap();
        let born = items.states();
        assert_eq!(born.len(), 1, "one cut");
        // Set down in the cell on the hunter's side, not in the carcass.
        assert!(
            (born[0].z - f64::from(at.2 as f32 + 1.5)).abs() < 1e-4 && (born[0].x - f64::from(at.0 as f32 + 0.5)).abs() < 1e-4,
            "the cut appeared at ({}, {}), not beside the carcass on the hunter's side",
            born[0].x,
            born[0].z
        );
        // ...and a moment later it lies on the floor there, not on the
        // carcass's slab.
        let now = Instant::now();
        for step in 0..30 {
            items.step(&ctx.world, 0.05, now + std::time::Duration::from_millis(50 * step));
        }
        let landed = items.states();
        assert!(
            landed[0].z > f64::from(at.2 as f32 + 1.0),
            "the cut ended up back on the carcass: z = {}",
            landed[0].z
        );
    }

    #[test]
    fn an_axe_ruins_the_skin_and_gets_the_rest() {
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_STONE_AXE, 1)));
        let at = (0, FLOOR + 1, 0);
        assert!(ctx.world.set_block(at.0, at.1, at.2, carcass_at_stage(Species::Deer, 0)));

        for _ in Species::Deer.butchering() {
            use_block(&ctx, &handle, at);
        }
        let taken = on_the_ground(&ctx);
        assert!(!taken.iter().any(|&(b, _)| b == BLOCK_HIDE), "the axe got the hide: {taken:?}");
        assert!(taken.iter().any(|&(b, _)| b == BLOCK_RAW_MEAT), "the axe got no meat: {taken:?}");
        assert_eq!(taken.len(), Species::Deer.butchering().len() - 1);
        assert!(is_air(ctx.world.cached_block(at.0, at.1, at.2).unwrap()));
    }

    #[test]
    fn bare_hands_take_nothing_off_a_carcass_and_the_player_is_told_why() {
        let (ctx, handle, mut rx) = a_hunter();
        hold(&handle, None);
        let at = (0, FLOOR + 1, 0);
        let carcass = carcass_at_stage(Species::Deer, 0);
        assert!(ctx.world.set_block(at.0, at.1, at.2, carcass));

        use_block(&ctx, &handle, at);

        assert_eq!(ctx.world.cached_block(at.0, at.1, at.2), Some(carcass));
        assert!(on_the_ground(&ctx).is_empty());
        let said = errors(&mut rx);
        assert_eq!(said.len(), 1, "the player was told {said:?}");
        assert_eq!(said[0], "NeedsAKnife", "the hint does not name the tool: {said:?}");

        // A dead player cannot butcher either, whatever they hold.
        hold(&handle, Some(Stack::new(BLOCK_FLINT_KNIFE, 1)));
        let _ = handle.state.lock().unwrap().vitals.hurt(f32::MAX, "test");
        use_block(&ctx, &handle, at);
        assert_eq!(ctx.world.cached_block(at.0, at.1, at.2), Some(carcass));
    }

    #[test]
    fn the_knife_that_breaks_on_a_cut_still_finishes_that_cut() {
        let (ctx, handle, _rx) = a_hunter();
        let total = tool_durability(BLOCK_FLINT_KNIFE).expect("a knife wears out");
        hold(&handle, Some(Stack::worn(BLOCK_FLINT_KNIFE, 1, total - 1)));
        let at = (0, FLOOR + 1, 0);
        assert!(ctx.world.set_block(at.0, at.1, at.2, carcass_at_stage(Species::Deer, 0)));

        use_block(&ctx, &handle, at);

        assert_eq!(on_the_ground(&ctx), vec![Species::Deer.butchering()[0]]);
        assert_eq!(
            butchering_stage(ctx.world.cached_block(at.0, at.1, at.2).unwrap()),
            1
        );
        let state = handle.state.lock().unwrap();
        assert!(state.inventory.block_in(0).is_none(), "the knife survived its last point");
    }

    #[test]
    fn a_carcass_half_taken_apart_is_the_same_carcass_after_a_save_round_trip() {
        let (ctx, _handle, _rx) = a_hunter();
        let at = (0, FLOOR + 1, 0);
        let half = carcass_at_stage(Species::Deer, 2);
        assert!(ctx.world.set_block(at.0, at.1, at.2, half));

        let dir = std::env::temp_dir().join(format!(
            "primitive-carcass-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        ctx.world.save(&dir).expect("saved");
        let reopened = World::new(ctx.world.seed(), 8);
        reopened.load(&dir).expect("loaded");
        let chunk = reopened.generate(ChunkPos { x: 0, z: 0 });
        let _ = std::fs::remove_dir_all(&dir);

        let (_, lx, lz) = ChunkPos::from_global(at.0, at.2);
        let back = chunk.get(lx, at.1 as usize, lz);
        assert_eq!(back, half, "the stage was lost on the way through the file");
        assert_eq!(Species::of_carcass(back), Some(Species::Deer));
        assert_eq!(butchering_stage(back), 2);
    }

    #[test]
    fn a_deer_that_dies_in_tall_grass_lies_on_the_meadow() {
        // A meadow is mostly tufts. A rule that wanted bare air under
        // every deer would put most kills back into the old heap.
        let column = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y <= FLOOR => BLOCK_STONE,
                y if y == FLOOR + 1 => BLOCK_TALL_GRASS,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(
            carcass_cell((0.5, FLOOR as f32 + 1.0, 0.5), column),
            Some((0, FLOOR + 1, 0))
        );
    }

    #[test]
    fn an_animal_knocked_into_the_air_lands_on_the_ground_under_it() {
        let column = |_x: i32, y: i32, _z: i32| {
            Some(if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR })
        };
        // Two cells up in the middle of a hop: down to the floor.
        assert_eq!(
            carcass_cell((0.5, FLOOR as f32 + 3.0, 0.5), column),
            Some((0, FLOOR + 1, 0))
        );
        // Four cells up is out of the search, and the heap it is.
        assert_eq!(carcass_cell((0.5, FLOOR as f32 + 5.0, 0.5), column), None);
        // A cell nobody has loaded is not a cell a carcass can go in.
        assert_eq!(carcass_cell((0.5, FLOOR as f32 + 1.0, 0.5), |_, _, _| None), None);
    }

    #[test]
    fn a_carcass_never_goes_on_a_torch() {
        // A placed thing is not free ground, and neither is the top of
        // a torch: it goes to the heap rather than putting a light out.
        let column = |_x: i32, y: i32, _z: i32| {
            Some(match y {
                y if y <= FLOOR => BLOCK_STONE,
                y if y == FLOOR + 1 => BLOCK_TORCH,
                _ => BLOCK_AIR,
            })
        };
        assert_eq!(carcass_cell((0.5, FLOOR as f32 + 1.0, 0.5), column), None);
    }
}

/// The honest tool chain on the server: the knapping die, the resin
/// tap, the spear and the fibre a tuft gives -- through the calls a
/// player's click takes, with a real world and a real pack. The pure
/// rules are tested in `primitive_shared::crafting`; this is the doing.
#[cfg(test)]
mod tool_chain_tests {
    use super::butchering_tests::{a_hunter, errors, hold, on_the_ground, FLOOR};
    use super::*;
    use primitive_shared::animals::Species;
    use primitive_shared::crafting::RECIPES;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{
        tool_durability, BlockId, BLOCK_BRONZE_KNIFE, BLOCK_COBBLESTONE, BLOCK_COPPER_KNIFE,
        BLOCK_FIBER, BLOCK_FLINT, BLOCK_FLINT_FLAKE, BLOCK_FLINT_KNIFE, BLOCK_FLINT_SPEAR,
        BLOCK_IRON_KNIFE, BLOCK_PEBBLE, BLOCK_SEEDS, BLOCK_TALL_GRASS, BLOCK_WEDGED_AXE,
    };

    fn index_of(name: &str) -> usize {
        RECIPES
            .iter()
            .position(|r| r.name == name)
            .unwrap_or_else(|| panic!("no recipe called {name:?}"))
    }

    fn count_in_pack(handle: &Arc<players::PlayerHandle>, block: BlockId) -> u32 {
        handle.state.lock().unwrap().inventory.count(block)
    }

    /// How worn the thing in the hand is.
    fn wear_of_held(handle: &Arc<players::PlayerHandle>) -> u32 {
        handle.state.lock().unwrap().inventory.slots()[0]
            .expect("nothing in the hand")
            .damage
    }


    #[test]
    fn knapping_on_the_server_shatters_some_of_the_flint_and_says_so() {
        // The die lives in `craft_for` and nowhere else, so this is the
        // one place it can be seen rolling. Sixty-four blows at a
        // thirty-five per cent chance: the odds of all of them landing
        // are under one in a hundred billion, and of all of them
        // shattering under one in ten to the twenty-ninth, so both
        // counts being non-zero is a certainty and not a flake.
        let (ctx, handle, mut rx) = a_hunter();
        handle.state.lock().unwrap().inventory.add(BLOCK_FLINT, 64);

        let crafts = craft_for(&ctx, &handle, index_of("flint flakes"), 64);

        assert_eq!(crafts.ran(), 64, "a shattered nodule stopped the run");
        assert!(crafts.made > 0, "sixty-four nodules and not one came apart clean");
        assert!(crafts.failed > 0, "sixty-four nodules and not one shattered");
        assert_eq!(count_in_pack(&handle, BLOCK_FLINT), 0, "a shattered nodule was given back");
        assert_eq!(
            count_in_pack(&handle, BLOCK_FLINT_FLAKE),
            3 * crafts.made,
            "the flakes do not match the blows that landed"
        );
        // ...and the player was told, in the channel every refusal uses.
        let said = errors(&mut rx);
        assert!(
            said.iter().any(|e| e.contains("shattered")),
            "flint vanished without a word: {said:?}"
        );

        // A row with no chance on it never fails, however many times it
        // is run: grinding a cobble is not knapping.
        handle.state.lock().unwrap().inventory.add(BLOCK_PEBBLE, 32);
        let ground = craft_for(&ctx, &handle, index_of("knapped stone"), 8);
        assert_eq!(ground, Crafts { made: 8, failed: 0 });
        assert_eq!(count_in_pack(&handle, BLOCK_COBBLESTONE), 8);
        assert!(errors(&mut rx).is_empty(), "grinding stone said something");
    }

    #[test]
    fn a_spear_hurts_more_than_any_knife() {
        // The spear is a weapon and the knives are tools that will do
        // for a hunt: it stands above the whole ladder of them, the iron
        // one included, because a thrust is the haft's weight and no
        // metal in the point adds to that. See `hunting_damage`.
        let spear = hunting_damage(Some(BLOCK_FLINT_SPEAR));
        for knife in [BLOCK_FLINT_KNIFE, BLOCK_COPPER_KNIFE, BLOCK_BRONZE_KNIFE, BLOCK_IRON_KNIFE] {
            assert!(
                spear > hunting_damage(Some(knife)),
                "{} out-hunts a spear",
                primitive_shared::types::block_name(knife)
            );
        }
        assert!(spear > hunting_damage(Some(BLOCK_WEDGED_AXE)), "an axe out-hunts a spear");
        assert!(spear > hunting_damage(None));
        // **A deer used to fall to one thrust, and it does not any
        // more.** This line was `spear >= Species::Deer.health()`, and
        // that was the sentence which made carrying a spear instead of a
        // second tool a decision. `animals::TOUGHNESS` -- five times the
        // health, asked for in those words -- made it seven thrusts, and
        // nothing in `hunting_damage` moved: what a spear is worth is
        // exactly what it was, and what it is worth it against is five
        // times as much.
        //
        // What survives, and what this now says, is the claim the spear
        // exists for: it is far and away the fastest thing a hunter can
        // carry, and a deer is still an animal you take rather than one
        // you wear down. Half a knife's thrusts and under eight of its
        // own; the exact counts are pinned in
        // `animals::tests::what_five_times_the_health_costs_in_blows`,
        // which is where a hunt that has become a chore shows up.
        let thrusts = |damage: f32| Species::Deer.health() / Species::Deer.hurt_by(damage);
        let with_a_spear = thrusts(spear);
        assert!(
            with_a_spear * 2.0 < thrusts(hunting_damage(Some(BLOCK_FLINT_KNIFE))),
            "a spear is {with_a_spear:.1} thrusts on a deer against a flint knife's {:.1}",
            thrusts(hunting_damage(Some(BLOCK_FLINT_KNIFE)))
        );
        assert!(
            with_a_spear < 8.0,
            "a deer takes {with_a_spear:.1} spear thrusts, which is a chore rather than a hunt"
        );

        // ...and it wears like a knife: a thrust that lands costs the
        // point, so seventy thrusts is a spear and then it is a stick.
        assert!(tool_durability(BLOCK_FLINT_SPEAR).is_some(), "a spear never wears out");
        let (ctx, handle, _rx) = a_hunter();
        hold(&handle, Some(Stack::new(BLOCK_FLINT_SPEAR, 1)));
        let id = ctx
            .animals
            .lock()
            .unwrap()
            .spawn(Species::Hare, (0.5, FLOOR as f32 + 1.0, 0.5))
            .expect("a hare");
        handle.state.lock().unwrap().last_swing = None;
        attack_animal(&ctx, &handle, id);
        assert!(ctx.animals.lock().unwrap().is_empty(), "one thrust did not kill a hare");
        assert_eq!(wear_of_held(&handle), 1, "the thrust that landed did not wear the point");
    }

    #[test]
    fn a_tuft_gives_fibre_one_time_in_two_and_nothing_else_at_all() {
        // Decided by where the tuft grew, so the meadow cannot be
        // farmed by standing over one cell and breaking the same grass
        // again. Four hundred tufts, three cells apart so no two drops
        // land close enough to be merged.
        //
        // **And no seed.** One tuft in four used to carry one, which
        // made a field something a player already had rather than
        // something they went and found; seed comes off wild wheat now.
        // This is the test that goes red if it ever comes back.
        let (ctx, _handle, _rx) = a_hunter();
        let cells: Vec<(i32, i32, i32)> = (0..20)
            .flat_map(|i| (0..20).map(move |j| (i * 3, FLOOR + 1, j * 3)))
            .collect();
        for &at in &cells {
            spawn_block_drop(&ctx, BLOCK_TALL_GRASS, at);
        }
        let dropped = on_the_ground(&ctx);
        let fibre: u32 = dropped.iter().filter(|(b, _)| *b == BLOCK_FIBER).map(|(_, n)| n).sum();
        let seeds: u32 = dropped.iter().filter(|(b, _)| *b == BLOCK_SEEDS).map(|(_, n)| n).sum();
        let tufts = cells.len() as u32;
        assert!(
            (tufts * 2 / 5..=tufts * 3 / 5).contains(&fibre),
            "{fibre} fibre from {tufts} tufts is not one in two"
        );
        assert_eq!(seeds, 0, "a meadow is still handing out seed");
        for &at in &cells {
            assert_eq!(fibrous(at), fibrous(at));
        }
    }

    #[test]
    fn a_harvest_gives_back_the_seed_it_grew_from() {
        // Through the one function every broken block's drop goes
        // through, a player's break and a mod's alike. Ripe wheat is grain
        // *and* its seed; green wheat is only its seed; ripe cotton is two
        // bolls and a seed; a wild stand of cotton is a boll and a seed.
        use primitive_shared::types::{
            BlockId, BLOCK_COTTON, BLOCK_COTTON_RIPE, BLOCK_COTTON_SEEDS, BLOCK_GRAIN, BLOCK_WHEAT,
            BLOCK_WHEAT_RIPE, BLOCK_WILD_COTTON,
        };
        let dropped = |broken: BlockId| {
            let (ctx, _handle, _rx) = a_hunter();
            spawn_block_drop(&ctx, broken, (0, FLOOR + 1, 0));
            let mut got = on_the_ground(&ctx);
            got.sort();
            got
        };
        let sorted = |mut want: Vec<(BlockId, u32)>| {
            want.sort();
            want
        };
        assert_eq!(
            dropped(BLOCK_WHEAT_RIPE),
            sorted(vec![(BLOCK_GRAIN, 1), (BLOCK_SEEDS, 1)]),
            "ripe wheat did not give its grain and its seed"
        );
        assert_eq!(dropped(BLOCK_WHEAT), vec![(BLOCK_SEEDS, 1)], "green wheat gave more than its seed");
        assert_eq!(
            dropped(BLOCK_COTTON_RIPE),
            sorted(vec![(BLOCK_COTTON, 2), (BLOCK_COTTON_SEEDS, 1)]),
            "ripe cotton did not give two bolls and a seed"
        );
        assert_eq!(
            dropped(BLOCK_WILD_COTTON),
            sorted(vec![(BLOCK_COTTON, 1), (BLOCK_COTTON_SEEDS, 1)]),
            "a wild stand of cotton kept its seed"
        );
    }

    #[test]
    fn turning_earth_says_whether_there_is_water_close_by() {
        // Every soil, both ways: the wet line and the dry line differ, the
        // dry one says so, and the soil's own words are still in it.
        use primitive_shared::worldgen::Fertility;
        for soil in [Fertility::Rich, Fertility::Ordinary, Fertility::Poor] {
            let (wet, dry) = (field_note(soil, true), field_note(soil, false));
            assert_ne!(wet, dry, "a farmer cannot tell a wet field from a dry one");
            assert!(dry.contains("no water"), "{dry:?} does not say the field is dry");
            assert!(!wet.contains("no water"), "{wet:?} says a watered field is dry");
        }
        assert!(field_note(Fertility::Rich, false).contains("rich soil"));
        assert!(field_note(Fertility::Poor, true).contains("thin"));
    }
}

/// What it costs to look for somewhere to put a rat.
///
/// The spawner's cheap case -- the first column it tries is dark -- was
/// never the interesting one. The expensive case is a player who has lit
/// their storeroom, because that is the one the mechanic *asks* for: then
/// `vermin::dark_floor_in` refuses every column of the cell and the pass
/// pays for all of them. See `make_a_rat`.
#[cfg(test)]
mod vermin_spawn_cost_tests {
    use super::*;

    /// Lighting a room is the answer the game gives you for rats, and it
    /// must not be the answer that makes the server stutter. One cell of
    /// the haunt map is eight blocks square: it lies over one chunk, or
    /// four where it straddles a corner, and the light pass may run that
    /// many times and no more -- however many of its sixty-four columns
    /// the scan has to look at before it gives up.
    #[test]
    fn scanning_a_whole_cell_for_a_rat_lights_each_chunk_once_and_not_each_column() {
        use primitive_shared::haunt::CELL;
        let settings = ServerSettings { world_dir: String::new(), ..ServerSettings::default() };
        let ctx = test_context(settings, RunOptions::embedded());
        let (sx, _, sz) = ctx.world.spawn_point();
        let centre = ChunkPos::from_global(sx.floor() as i32, sz.floor() as i32).0;
        // Generated all round, so no column is cheap for want of a chunk:
        // an absent chunk costs a cache lookup and would hide the bug.
        for dz in -1..=1 {
            for dx in -1..=1 {
                ctx.world.insert(ctx.world.generate(ChunkPos::new(centre.x + dx, centre.z + dz)));
            }
        }
        // A cell sitting on a chunk corner, which is the worst case there
        // is: four chunks under one cell's columns.
        let corner_x = centre.x * primitive_shared::types::CHUNK_SIZE_X as i32;
        let corner_z = centre.z * primitive_shared::types::CHUNK_SIZE_Z as i32;
        let (base_x, base_z) = (corner_x - CELL / 2, corner_z - CELL / 2);
        let mut probe = ColumnProbe::new(&ctx);
        for dz in 0..CELL {
            for dx in 0..CELL {
                probe.standing_room(base_x + dx, base_z + dz);
            }
        }
        assert!(
            probe.fills <= 4,
            "{} light passes for one cell's {} columns -- the memo is not holding",
            probe.fills,
            CELL * CELL
        );
        // ...and it really did straddle a corner, or the four above is a
        // ceiling nothing ever reached.
        assert!(probe.seen.len() > 1, "the cell did not straddle a chunk boundary");
    }
}

/// Who hears about a chest's lid: everybody who can see the chest, and
/// nobody who cannot -- both when it moves and when they arrive.
#[cfg(test)]
mod chest_lid_tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_CHEST, BLOCK_STONE};

    const FLOOR: i32 = 19;
    const CHEST: (i32, i32, i32) = (2, FLOOR + 1, 2);

    /// A world with the origin chunk loaded and a chest standing on a stone
    /// floor in it.
    fn a_room_with_a_chest() -> Arc<Context> {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        ctx.world.insert(ctx.world.generate(ChunkPos { x: 0, z: 0 }));
        for x in 0..5 {
            for z in 0..6 {
                for y in 0..primitive_shared::types::CHUNK_SIZE_Y as i32 {
                    let block = if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR };
                    assert!(ctx.world.set_block(x, y, z, block));
                }
            }
        }
        assert!(ctx.world.set_block(CHEST.0, CHEST.1, CHEST.2, BLOCK_CHEST));
        ctx
    }

    /// A player standing at `feet`, in the registry, and subscribed to the
    /// origin chunk if `sees` -- which is what "can see the chest" is to the
    /// server.
    fn a_player(
        ctx: &Arc<Context>,
        id: PlayerId,
        feet: (f64, f64, f64),
        sees: bool,
    ) -> (Arc<players::PlayerHandle>, tokio::sync::mpsc::Receiver<players::Outgoing>) {
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget(chunk_rx);
        let handle = Arc::new(players::PlayerHandle::new(
            id,
            format!("player{id}"),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(crate::settings::AntiCheatSettings::default(), 8, feet),
        ));
        assert!(ctx.registry.insert_unique(Arc::clone(&handle)));
        if sees {
            ctx.registry.subscribe(id, ChunkPos { x: 0, z: 0 });
        }
        (handle, rx)
    }

    /// Every lid message in a queue, drained.
    fn lids(rx: &mut tokio::sync::mpsc::Receiver<players::Outgoing>) -> Vec<((i32, i32, i32), bool)> {
        let mut seen = Vec::new();
        while let Ok(message) = rx.try_recv() {
            if let players::Outgoing::Message(ServerMessage::ChestLid { x, y, z, open }) = message {
                seen.push(((x, y, z), open));
            }
        }
        seen
    }

    #[test]
    fn a_lid_that_goes_up_or_down_is_told_to_everybody_who_can_see_the_chest_and_nobody_else() {
        let ctx = a_room_with_a_chest();
        let feet = (f64::from(CHEST.0) + 0.5, f64::from(FLOOR + 1), f64::from(CHEST.2) + 2.5);
        let (opener, mut opener_rx) = a_player(&ctx, 1, feet, true);
        let (_watcher, mut watcher_rx) = a_player(&ctx, 2, (4.5, f64::from(FLOOR + 1), 4.5), true);
        let (_stranger, mut stranger_rx) = a_player(&ctx, 3, (900.5, 40.0, 900.5), false);

        open_chest(&ctx, &opener, CHEST);
        assert_eq!(opener.state.lock().unwrap().open_chest, Some(CHEST), "the chest did not open");
        assert_eq!(lids(&mut opener_rx), vec![(CHEST, true)], "the opener was not told the lid went up");
        assert_eq!(lids(&mut watcher_rx), vec![(CHEST, true)], "somebody watching was not told the lid went up");
        assert!(lids(&mut stranger_rx).is_empty(), "a player who cannot see the chest was told about its lid");

        // Walking away shuts it, for the same people.
        opener.state.lock().unwrap().open_chest = None;
        tell_chest_lid(&ctx, CHEST);
        assert_eq!(lids(&mut opener_rx), vec![(CHEST, false)]);
        assert_eq!(lids(&mut watcher_rx), vec![(CHEST, false)], "somebody watching saw the lid stay up");
        assert!(lids(&mut stranger_rx).is_empty());
    }

    #[test]
    fn a_chest_open_before_somebody_arrives_is_told_to_them_with_the_chunk() {
        // The other half of "everybody who can see it": a player whose chunk
        // arrives -- walking up, or the chunk loaded again -- while somebody
        // is already at the chest.
        let ctx = a_room_with_a_chest();
        let feet = (f64::from(CHEST.0) + 0.5, f64::from(FLOOR + 1), f64::from(CHEST.2) + 2.5);
        let (opener, _opener_rx) = a_player(&ctx, 1, feet, true);
        let (other, _other_rx) = a_player(&ctx, 2, feet, true);
        assert!(open_lids_in(&ctx, ChunkPos { x: 0, z: 0 }).is_empty(), "a shut chest was said to be open");

        open_chest(&ctx, &opener, CHEST);
        let said = |ctx: &Arc<Context>| -> Vec<((i32, i32, i32), bool)> {
            open_lids_in(ctx, ChunkPos { x: 0, z: 0 })
                .into_iter()
                .filter_map(|message| match message {
                    ServerMessage::ChestLid { x, y, z, open } => Some(((x, y, z), open)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(said(&ctx), vec![(CHEST, true)], "the chunk goes out without its open lid");
        // Two at one chest are one lid, and another chunk has none of it.
        other.state.lock().unwrap().open_chest = Some(CHEST);
        assert_eq!(said(&ctx).len(), 1, "two players at one chest were sent as two lids");
        assert!(open_lids_in(&ctx, ChunkPos { x: 1, z: 0 }).is_empty(), "a lid was sent with the wrong chunk");
        // ...and a lid nobody is at is not sent at all.
        opener.state.lock().unwrap().open_chest = None;
        other.state.lock().unwrap().open_chest = None;
        assert!(said(&ctx).is_empty(), "a chest nobody is at went out open");
    }
}

/// The anvil's and the wheel's round trip, played the way the client plays
/// it: `OpenStation`, `StationBegin`, the blows timed off `StationBegun`'s
/// seed, `StationRun`, and what comes back.
#[cfg(test)]
mod station_round_trip_tests {
    use super::*;
    use primitive_shared::minigame::{self, Game, Job, Verdict};
    use primitive_shared::types::{BLOCK_AIR, BLOCK_BOWL_RAW, BLOCK_CLAY, BLOCK_POTTERS_WHEEL, BLOCK_STONE};

    const FLOOR: i32 = 19;
    const WHEEL: (i32, i32, i32) = (2, FLOOR + 1, 2);

    fn at_the_wheel() -> (Arc<Context>, Arc<players::PlayerHandle>, tokio::sync::mpsc::Receiver<players::Outgoing>) {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        ctx.world.insert(ctx.world.generate(ChunkPos { x: 0, z: 0 }));
        for x in 0..5 {
            for z in 0..6 {
                for y in 0..primitive_shared::types::CHUNK_SIZE_Y as i32 {
                    assert!(ctx.world.set_block(x, y, z, if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR }));
                }
            }
        }
        assert!(ctx.world.set_block(WHEEL.0, WHEEL.1, WHEEL.2, BLOCK_POTTERS_WHEEL));
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget(chunk_rx);
        let feet = (2.5, f64::from(FLOOR + 1), 4.0);
        let handle = Arc::new(players::PlayerHandle::new(
            1,
            "potter".to_string(),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(crate::settings::AntiCheatSettings::default(), 8, feet),
        ));
        assert_eq!(handle.state.lock().unwrap().inventory.add(BLOCK_CLAY, 8), 0);
        (ctx, handle, rx)
    }

    fn drain(rx: &mut tokio::sync::mpsc::Receiver<players::Outgoing>) -> Vec<ServerMessage> {
        let mut out = Vec::new();
        while let Ok(players::Outgoing::Message(message)) = rx.try_recv() {
            out.push(message);
        }
        out
    }

    /// The blows a player who watched the marker strikes, filtered exactly
    /// as the client's screen filters them (`minigame::press_counts`).
    fn watched_blows(game: Game, seed: u32) -> Vec<u32> {
        let mut presses: Vec<u32> = Vec::new();
        for step in 0..game.presses() {
            let want = minigame::target(seed, step);
            let at = step as u32 * game.step_ms() + (want / 2.0 * game.step_ms() as f32) as u32;
            if minigame::press_counts(game, presses.last().copied(), at) {
                presses.push(at);
            }
        }
        presses
    }

    #[test]
    fn a_run_played_the_way_the_client_plays_it_is_judged_and_paid() {
        let (ctx, handle, mut rx) = at_the_wheel();
        open_station(&ctx, &handle, WHEEL);
        let opened = drain(&mut rx);
        assert!(
            opened.iter().any(|m| matches!(m, ServerMessage::StationOpen { game: Game::Wheel, .. })),
            "the wheel did not open: {opened:?}"
        );
        station_begin(&ctx, &handle, Job::Bowl);
        let seed = drain(&mut rx)
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::StationBegun { seed } => Some(seed),
                _ => None,
            })
            .expect("the run never began");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_CLAY), 6, "the clay was not spent at the start");
        // The whole run has gone by on the server's clock, as it has by the
        // time the client hands it in.
        if let Some(seat) = handle.state.lock().unwrap().station.as_mut() {
            let (job, _, seed) = seat.run.expect("no run on the seat");
            seat.run = Some((job, Instant::now() - Duration::from_millis(u64::from(Game::Wheel.run_ms()) + 100), seed));
        }
        let presses = watched_blows(Game::Wheel, seed);
        assert_eq!(presses.len(), Game::Wheel.presses(), "the client dropped a watched blow");
        station_run(&ctx, &handle, presses);
        let answer = drain(&mut rx);
        assert!(
            answer.iter().any(|m| matches!(m, ServerMessage::StationResult { verdict: Verdict::Fine, .. })),
            "a run struck dead on every spot was not judged fine: {answer:?}"
        );
        assert!(!answer.iter().any(|m| matches!(m, ServerMessage::Error(_))), "the run was refused: {answer:?}");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_BOWL_RAW), 2, "a fine run did not pay two bowls");
        // ...and the seat is free for the next one.
        station_begin(&ctx, &handle, Job::Bowl);
        assert!(
            drain(&mut rx).iter().any(|m| matches!(m, ServerMessage::StationBegun { .. })),
            "a second run could not begin after the first was judged"
        );
    }

    #[test]
    fn bowls_thrown_into_a_pack_filled_during_the_run_fall_at_the_potters_feet() {
        let (ctx, handle, mut rx) = at_the_wheel();
        open_station(&ctx, &handle, WHEEL);
        station_begin(&ctx, &handle, Job::Bowl);
        let seed = drain(&mut rx)
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::StationBegun { seed } => Some(seed),
                _ => None,
            })
            .expect("the run never began");
        // Mid-run the pack fills: a heap walked over while the wheel spun.
        while handle.state.lock().unwrap().inventory.add(BLOCK_STONE, 64) == 0 {}
        if let Some(seat) = handle.state.lock().unwrap().station.as_mut() {
            let (job, _, seed) = seat.run.expect("no run on the seat");
            seat.run = Some((job, Instant::now() - Duration::from_millis(u64::from(Game::Wheel.run_ms()) + 100), seed));
        }
        station_run(&ctx, &handle, watched_blows(Game::Wheel, seed));
        let in_pack = handle.state.lock().unwrap().inventory.count(BLOCK_BOWL_RAW);
        let on_ground: u32 =
            ctx.items.lock().unwrap().iter().filter(|item| item.block == BLOCK_BOWL_RAW).map(|item| item.count).sum();
        assert_eq!(in_pack + on_ground, 2, "a fine run into a full pack paid {in_pack} in the pack and {on_ground} on the ground");
    }

    #[test]
    fn a_run_handed_in_as_soon_as_its_last_blow_lands_is_not_ahead_of_the_server() {
        // The client hands the run in on the last blow, not at the end of the
        // last sweep: the server's clock has only got as far as that blow
        // (plus the message's flight), which is what `CLOCK_SLACK_MS` covers.
        let (ctx, handle, mut rx) = at_the_wheel();
        open_station(&ctx, &handle, WHEEL);
        station_begin(&ctx, &handle, Job::Vessel);
        let seed = drain(&mut rx)
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::StationBegun { seed } => Some(seed),
                _ => None,
            });
        // Short of sand: refused, and the clay kept.
        assert!(seed.is_none(), "a vessel began with no sand");
        assert_eq!(handle.state.lock().unwrap().inventory.count(BLOCK_CLAY), 8);
        station_begin(&ctx, &handle, Job::Jug);
        let seed = drain(&mut rx)
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::StationBegun { seed } => Some(seed),
                _ => None,
            })
            .expect("the jug never began");
        let presses = watched_blows(Game::Wheel, seed);
        let last = *presses.last().unwrap();
        if let Some(seat) = handle.state.lock().unwrap().station.as_mut() {
            let (job, _, seed) = seat.run.unwrap();
            // A frame after the last blow, less a little: the client's clock
            // started a message's flight later than the server's did.
            seat.run = Some((job, Instant::now() - Duration::from_millis(u64::from(last) + 16), seed));
        }
        station_run(&ctx, &handle, presses);
        let answer = drain(&mut rx);
        assert!(
            answer.iter().any(|m| matches!(m, ServerMessage::StationResult { .. })),
            "a run handed in on its last blow was refused: {answer:?}"
        );
    }
}

/// The barter stall on the server: who may do what at one, and that a trade
/// moves exactly the goods it names -- once -- whoever else is reaching for
/// them. The rule of a trade itself is `primitive_shared::stall`'s.
#[cfg(test)]
mod stall_tests {
    use super::*;
    use primitive_shared::stall::{Offer, Refusal, STOCK, TAKINGS};
    use primitive_shared::types::{BLOCK_AIR, BLOCK_FLINT, BLOCK_HIDE, BLOCK_STALL, BLOCK_STONE};

    const FLOOR: i32 = 19;
    const STALL: (i32, i32, i32) = (2, FLOOR + 1, 2);
    const FLINT_FOR_HIDE: Offer = Offer { give: BLOCK_FLINT, give_count: 4, take: BLOCK_HIDE, take_count: 1 };

    fn a_stall() -> Arc<Context> {
        let ctx = test_context(ServerSettings::default(), RunOptions::embedded());
        ctx.world.insert(ctx.world.generate(ChunkPos { x: 0, z: 0 }));
        for x in 0..5 {
            for z in 0..6 {
                for y in 0..primitive_shared::types::CHUNK_SIZE_Y as i32 {
                    let block = if y <= FLOOR { BLOCK_STONE } else { BLOCK_AIR };
                    assert!(ctx.world.set_block(x, y, z, block));
                }
            }
        }
        assert!(ctx.world.set_block(STALL.0, STALL.1, STALL.2, BLOCK_STALL));
        ctx
    }

    fn a_player(ctx: &Arc<Context>, id: PlayerId) -> (Arc<players::PlayerHandle>, tokio::sync::mpsc::Receiver<players::Outgoing>) {
        let feet = (f64::from(STALL.0) + 0.5, f64::from(FLOOR + 1), f64::from(STALL.2) + 2.5);
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel(8);
        std::mem::forget(chunk_rx);
        let handle = Arc::new(players::PlayerHandle::new(
            id,
            format!("player{id}"),
            "127.0.0.1:1".parse().unwrap(),
            tx,
            chunk_tx,
            10_000,
            feet,
            crate::logic::anticheat::AntiCheat::new(crate::settings::AntiCheatSettings::default(), 8, feet),
        ));
        assert!(ctx.registry.insert_unique(Arc::clone(&handle)));
        (handle, rx)
    }

    /// A stall put down by player 1 with `lots` lots of flint on its counter
    /// at four flint for a hide, and the owner's handle.
    fn stocked(ctx: &Arc<Context>, lots: u32) -> Arc<players::PlayerHandle> {
        let (owner, rx) = a_player(ctx, 1);
        std::mem::forget(rx);
        stall_placed(ctx, &owner, STALL);
        if lots > 0 {
            ctx.chests.lock().unwrap().edit(STALL, |store| store.add_within(STOCK, BLOCK_FLINT, 4 * lots));
        }
        open_chest(ctx, &owner, STALL);
        stall_offer(ctx, &owner, 0, Some(FLINT_FOR_HIDE));
        assert_eq!(ctx.stalls.lock().unwrap().get(STALL).unwrap().offer(0), Some(FLINT_FOR_HIDE));
        owner
    }

    fn refusals(rx: &mut tokio::sync::mpsc::Receiver<players::Outgoing>) -> Vec<Refusal> {
        let mut seen = Vec::new();
        while let Ok(message) = rx.try_recv() {
            if let players::Outgoing::Message(ServerMessage::StallRefused { why }) = message {
                seen.push(why);
            }
        }
        seen
    }

    fn pack_count(handle: &Arc<players::PlayerHandle>, block: primitive_shared::types::BlockId) -> u32 {
        handle.state.lock().unwrap().inventory.count(block)
    }

    #[test]
    fn a_trade_at_a_stall_moves_exactly_the_lot_and_the_price() {
        let ctx = a_stall();
        let _owner = stocked(&ctx, 2);
        let (buyer, _rx) = a_player(&ctx, 2);
        buyer.state.lock().unwrap().inventory.add(BLOCK_HIDE, 3);
        open_chest(&ctx, &buyer, STALL);
        stall_buy(&ctx, &buyer, 0, FLINT_FOR_HIDE);
        assert_eq!(pack_count(&buyer, BLOCK_FLINT), 4);
        assert_eq!(pack_count(&buyer, BLOCK_HIDE), 2);
        let store = ctx.chests.lock().unwrap().contents(STALL);
        assert_eq!(store.count_within(STOCK, BLOCK_FLINT), 4);
        assert_eq!(store.count_within(TAKINGS, BLOCK_HIDE), 1);
    }

    #[test]
    fn two_buyers_racing_for_the_last_lot_are_one_sale_and_one_refusal() {
        let ctx = a_stall();
        let _owner = stocked(&ctx, 1);
        let (first, mut first_rx) = a_player(&ctx, 2);
        let (second, mut second_rx) = a_player(&ctx, 3);
        for buyer in [&first, &second] {
            buyer.state.lock().unwrap().inventory.add(BLOCK_HIDE, 1);
            open_chest(&ctx, buyer, STALL);
        }
        // Both at once, from two threads, as two connections would.
        std::thread::scope(|scope| {
            for buyer in [&first, &second] {
                let ctx = &ctx;
                scope.spawn(move || stall_buy(ctx, buyer, 0, FLINT_FOR_HIDE));
            }
        });
        let won: Vec<bool> = [&first, &second].iter().map(|b| pack_count(b, BLOCK_FLINT) == 4).collect();
        let sales = won.iter().filter(|&&w| w).count();
        assert_eq!(sales, 1, "the last lot was sold {sales} times");
        let loser = if won[0] { &second } else { &first };
        assert_eq!(pack_count(loser, BLOCK_FLINT), 0, "the loser got flint");
        assert_eq!(pack_count(loser, BLOCK_HIDE), 1, "the loser paid for nothing");
        let store = ctx.chests.lock().unwrap().contents(STALL);
        assert_eq!(store.count_within(STOCK, BLOCK_FLINT), 0);
        assert_eq!(store.count_within(TAKINGS, BLOCK_HIDE), 1, "the till took two prices for one lot");
        let told = [refusals(&mut first_rx), refusals(&mut second_rx)].concat();
        assert_eq!(told, vec![Refusal::SoldOut], "the loser was not told why");
    }

    #[test]
    fn a_price_changed_under_the_buyers_hand_is_refused_and_nothing_moves() {
        let ctx = a_stall();
        let owner = stocked(&ctx, 2);
        let (buyer, mut rx) = a_player(&ctx, 2);
        buyer.state.lock().unwrap().inventory.add(BLOCK_HIDE, 5);
        open_chest(&ctx, &buyer, STALL);
        let dearer = Offer { take_count: 5, ..FLINT_FOR_HIDE };
        stall_offer(&ctx, &owner, 0, Some(dearer));
        stall_buy(&ctx, &buyer, 0, FLINT_FOR_HIDE);
        assert_eq!(refusals(&mut rx), vec![Refusal::OfferChanged]);
        assert_eq!(pack_count(&buyer, BLOCK_HIDE), 5);
        assert_eq!(pack_count(&buyer, BLOCK_FLINT), 0);
    }

    #[test]
    fn nobody_but_the_owner_can_price_stock_or_empty_a_stall() {
        let ctx = a_stall();
        let _owner = stocked(&ctx, 2);
        let (stranger, mut rx) = a_player(&ctx, 2);
        stranger.state.lock().unwrap().inventory.add(BLOCK_STONE, 5);
        open_chest(&ctx, &stranger, STALL);
        stall_offer(&ctx, &stranger, 0, Some(Offer { give_count: 1, ..FLINT_FOR_HIDE }));
        chest_move(&ctx, &stranger, (Side::Chest, 0), (Side::Pack, 5), false);
        chest_bulk_move(&ctx, &stranger, false);
        chest_move(&ctx, &stranger, (Side::Pack, 0), (Side::Chest, 3), false);
        assert_eq!(pack_count(&stranger, BLOCK_FLINT), 0, "a stranger took goods off the counter");
        assert_eq!(pack_count(&stranger, BLOCK_STONE), 5, "a stranger stocked somebody else's stall");
        assert_eq!(ctx.stalls.lock().unwrap().get(STALL).unwrap().offer(0), Some(FLINT_FOR_HIDE));
        let told = refusals(&mut rx);
        assert!(!told.is_empty() && told.iter().all(|&why| why == Refusal::NotYours), "{told:?}");
    }

    #[test]
    fn the_owner_stocks_the_counter_but_cannot_put_anything_in_the_till() {
        let ctx = a_stall();
        let owner = stocked(&ctx, 0);
        owner.state.lock().unwrap().inventory.add(BLOCK_STONE, 5);
        let from = owner.state.lock().unwrap().inventory.slots().iter().position(|s| s.is_some_and(|s| s.block == BLOCK_STONE)).unwrap();
        chest_move(&ctx, &owner, (Side::Pack, from as u8), (Side::Chest, TAKINGS.start as u8), false);
        assert_eq!(pack_count(&owner, BLOCK_STONE), 5, "the till took something by hand");
        chest_move(&ctx, &owner, (Side::Pack, from as u8), (Side::Chest, STOCK.start as u8), false);
        assert_eq!(ctx.chests.lock().unwrap().contents(STALL).count_within(STOCK, BLOCK_STONE), 5);
    }

    #[test]
    fn the_owner_takes_a_stall_down_into_their_pack_and_anybody_else_breaks_it_open() {
        // The owner's break: everything home, nothing on the floor.
        let ctx = a_stall();
        let owner = stocked(&ctx, 2);
        stall_broken(&ctx, &owner, STALL);
        spill_chest(&ctx, STALL);
        assert_eq!(pack_count(&owner, BLOCK_FLINT), 8, "the owner's goods did not come home");
        assert!(ctx.stalls.lock().unwrap().get(STALL).is_none(), "the stall outlived its block");

        // A stranger's: the goods spill, and not into the stranger's pack.
        let ctx = a_stall();
        let _owner = stocked(&ctx, 2);
        let (stranger, _rx) = a_player(&ctx, 2);
        stall_broken(&ctx, &stranger, STALL);
        assert!(ctx.chests.lock().unwrap().holds_anything(STALL), "a stranger's break emptied the counter");
        spill_chest(&ctx, STALL);
        assert_eq!(pack_count(&stranger, BLOCK_FLINT), 0);
        assert!(!ctx.chests.lock().unwrap().holds_anything(STALL));
        assert!(ctx.stalls.lock().unwrap().get(STALL).is_none());
    }

    #[test]
    fn a_stall_survives_a_save_round_trip_with_its_offers_stock_and_takings() {
        let ctx = a_stall();
        let _owner = stocked(&ctx, 3);
        let (buyer, _rx) = a_player(&ctx, 2);
        buyer.state.lock().unwrap().inventory.add(BLOCK_HIDE, 1);
        open_chest(&ctx, &buyer, STALL);
        stall_buy(&ctx, &buyer, 0, FLINT_FOR_HIDE);

        let dir = std::env::temp_dir().join(format!("primitive_stall_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save_chests(&ctx, &dir);
        save_stalls(&ctx, &dir);

        let mut chests = containers::Chests::new();
        let mut stalls = stalls::Stalls::new();
        chests.load(&dir).expect("chests");
        stalls.load(&dir).expect("stalls");
        let store = chests.contents(STALL);
        assert_eq!(store.count_within(STOCK, BLOCK_FLINT), 8, "the counter came back wrong");
        assert_eq!(store.count_within(TAKINGS, BLOCK_HIDE), 1, "the till came back wrong");
        let stall = stalls.get(STALL).expect("the stall's record did not come back");
        assert_eq!(stall.owner, "player1");
        assert_eq!(stall.offer(0), Some(FLINT_FOR_HIDE));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
