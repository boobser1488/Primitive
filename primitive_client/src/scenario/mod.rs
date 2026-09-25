//! **Scenarios: the real game, played by a script.**
//!
//! Why this exists: for days the player found bugs that every unit test
//! had missed, because each of those tests checked one part on its own --
//! a staircase that lifted you a whole block, a rack drawn four times, a
//! hide frame whose collider was an invisible wall, a station screen that
//! did not redraw, a hover false-positive when jumping out of water. Each
//! part was right by itself. What was wrong was where two of them met:
//! the collider and the mesher disagreeing about one block, the client's
//! physics and the server's anticheat disagreeing about one jump.
//!
//! So a scenario is the whole of it at once: a **real in-process server**
//! (`primitive_server::start`, the same call singleplayer makes) with the
//! **movement validator switched on**, the **real network client**
//! (`network::connect`), and on this side the real `ChunkManager`, the
//! real `Player` physics stepped the way the frame steps it, the real
//! `Mining`, `use_gesture`, `try_place_block`, `apply_change`, the real
//! chest and station screens, and the real mesher -- driven by held keys,
//! a look and a click, sixty frames to the second, and asked afterwards
//! what a player would have seen and felt.
//!
//! ## What it shares with `run`, and what it still does not
//!
//! This note used to say the opposite. `run` was one function of six
//! thousand lines with the frame's body written inline in it, so there
//! was nothing to call: the harness kept its own right click, its own
//! step of the hands, its own arms of the socket, and the note here
//! recorded that as a thing rejected for being too large -- "that is the
//! right end state and a refactor of six thousand lines of event handling
//! whose every borrow is load-bearing".
//!
//! The frame lives in `crate::frame` now, and what a scenario plays is
//! what the game runs:
//!
//! * the right click is [`crate::frame::interact::right_click_on_the_world`],
//! * one frame of the hands is [`crate::frame::hands::step`],
//! * and the helpers around them were already shared.
//!
//! What is still a copy, and said plainly so the next person can finish
//! it: **the order of the frame** below (which is `run`'s order, kept by
//! hand), **the body's own step** (`step_body`, which is the collider and
//! the horse without the raft, the posture or the sail), and **the arms of
//! the socket** this file reads (corrections, block updates, the pack, the
//! containers, the stations, health and death) -- each of them the same
//! statement `drain_network` makes. If `run` grows a step that changes
//! what the player feels, those need the same step, or a scenario will
//! pass on a game that no longer exists.
//!
//! Still rejected: driving the real binary with synthetic input. The
//! platform layer has no way in that is not a window, and a window is a
//! GPU, which is exactly what an unattended run on a build machine does
//! not have.
//!
//! ## Time is counted in frames, and the floor under a frame is real
//!
//! **A scenario's clock is its frame counter.** `seconds(n)` is `n * 60`
//! frames, `until(n)` gives something `n * 60` frames to happen in, the
//! body steps by `FRAME`, and -- this is the part that took the longest
//! to arrive -- **the server runs one tick per frame it is owed and not
//! one because the wall said so** (`RunOptions::ticks_by_hand`, and
//! `Scenario::pace_the_world` below). So a run of a scenario simulates
//! exactly the same world on a free machine and on one with six builds
//! on it; what a loaded machine changes is how long the run takes, and
//! nothing else.
//!
//! That is not how it was, and the symptom is worth writing down. The
//! server ticked on its own 20 Hz wall clock while the client counted
//! frames, so on a busy machine sixty frames of the client took three
//! seconds of the wall and the world aged three times as fast as the
//! player did. One to five scenarios out of seventy went red per run,
//! never the same ones -- a downed player "died before the clock ran
//! out", a horse threw its rider before the rider had got on, a fire
//! burned out during a night that the client had not finished walking
//! through. Every one of them passed on its own. "The tests are green"
//! had stopped being a fact.
//!
//! **What is still measured by the wall, and why.** A frame is never
//! shorter than a sixtieth of a second of wall time, and never catches
//! up after a stall. The anticheat measures a player against
//! `Instant::now()` -- speed budgets, the hover clock -- so a scenario
//! that ran its frames *faster* than the wall would be a player moving
//! faster than the validator allows, and would be corrected for it.
//! Slower is always safe (the budget only refills), and slower is what a
//! loaded machine now produces. So the floor stays: it is the harness's
//! guarantee that it never asks the server for a second of movement in
//! less than a second.
//!
//! The floor is also what gives the real parts of this their real time.
//! The sockets are real, the chunk generation is real and runs on its own
//! threads, and a wait for a chunk to arrive is a wait for work nobody's
//! frame counter can hurry. `until(20.0)` is 1200 frames and therefore at
//! least twenty seconds of wall -- exactly what it was before -- so the
//! arrivals have not lost a moment of the time they had.
//!
//! The transform the client sends the server keeps the wall for the same
//! family of reasons: it is a keepalive as well as a position, and the
//! server's patience (`client_timeout_secs`) is counted in the server's
//! seconds. See the `maybe_send_transform` call in [`Scenario::frame`].
//!
//! **The one clock a scenario still reads directly** is the station
//! minigame's: `StationScreen` times its own bar with `Instant::now()`
//! and sends the millisecond of each blow up the wire, so a test that
//! plays a run on the beat has to watch the same wall the screen does.
//! `a_run_on_the_marker` does, deliberately, and says so.
//!
//! ## Pictures
//!
//! `PRIMITIVE_SCENARIO_SHOTS=<absolute dir>` writes what a scenario looked
//! at through the real renderer, offscreen, whenever it calls `shot` --
//! see [`Scenario::shot`]. Off by default, and a no-op on a machine
//! without a GPU adapter: the assertions are the test, the pictures are
//! for a person who wants to look.

// A toolkit: not every scenario uses every verb, and a verb nobody calls
// yet is still the harness's vocabulary rather than dead code.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use glam::{DVec3, Vec3};
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{ClientMessage, ServerMessage};
use primitive_shared::types::{BlockId, ChunkPos};

use crate::engine::camera::Camera;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::mining::Mining;
use crate::logic::physics::Player;
use crate::net::network::{self, NetworkHandle, WelcomeInfo};
use crate::platform::Key;
use crate::ui::debug::DebugStats;
use crate::ui::input::InputState;
use crate::logic::{hand, shake, stamina};
use crate::net::remote_players::RemotePlayers;
use crate::settings::ClientSettings;
use crate::ui::hud::BodyGauges;
use crate::ui::keybinds::Action;
use crate::ui::{chest_screen::ChestScreen, death::DeathScreen, station_screen::StationScreen};
use crate::{Arrivals, Cut, DigSignal, Meal, MeshQueueSet};

mod tests;

/// One frame, as the game aims for it.
pub const FRAME: f32 = 1.0 / 60.0;

/// How many chunks round the player the client keeps. Small: a scenario
/// is about the few metres in front of it, and every chunk is one more
/// the server generates before the first frame can run.
const RENDER_DISTANCE: i32 = 3;

pub struct Scenario {
    runtime: tokio::runtime::Runtime,
    /// The server, shared with any client that joined this one's
    /// ([`Scenario::join`]). Shut down by whichever of them is dropped last
    /// -- which in a test is the one declared first.
    server: Option<std::sync::Arc<primitive_server::Server>>,
    /// Who this client is on the server, for the doors that act on a named
    /// player (`Server::give_to`): with two clients connected, "the one
    /// connected player" is nobody in particular.
    pub name: String,
    net: NetworkHandle,
    pub welcome: WelcomeInfo,

    pub chunks: ChunkManager,
    light: LightMap,
    arrivals: Arrivals,
    urgent: VecDeque<ChunkPos>,
    dirty_set: MeshQueueSet,
    versions: HashMap<ChunkPos, u64>,

    pub player: Player,
    pub camera: Camera,
    pub input: InputState,
    /// **The player's settings, whole, and not just their keys.** The
    /// frame's own functions are handed a `&ClientSettings` -- the
    /// bindings, the language a refusal is said in, the reach -- and a
    /// harness that kept only the bindings would have had to invent the
    /// rest, which is the same thing as guessing what the game does.
    settings: ClientSettings,
    pub mining: Mining,
    debug: DebugStats,
    pub audio: crate::Audio,
    soundscape: crate::Soundscape,
    /// **The rest of what one frame of the hands needs**, all of it the
    /// frame's own state and none of it invented here: the arm, the rhythm
    /// a blow comes at, the recoil, the "this player is working" flag the
    /// server is told about, the tank a swing is billed to, the cut and
    /// the mouthful under way, and the rod being wound back.
    ///
    /// They are fields because [`crate::frame::hands::step`] is what runs
    /// them. A harness playing its own copy of a swing could pass while
    /// the game struck twice for every blow -- which the game once did.
    hand: hand::Hand,
    strikes: hand::Strikes,
    shake: shake::Shake,
    dig_signal: DigSignal,
    stamina: stamina::Stamina,
    rod_hold: crate::logic::fishing::Hold,
    cut: Option<Cut>,
    meal: Option<Meal>,
    meal_sent: Option<Instant>,
    /// What the server last refused, in the player's own language --
    /// or what this side refused before asking. Several gestures never
    /// reach the wire at all and say so here instead (a throw onto the
    /// bank, a set-down with nowhere to set it down), so a scenario that
    /// asserted only on messages could not see them.
    pub notice: Option<(String, Instant)>,
    /// Everybody else, and everything that is not a block. Empty in most
    /// scenarios, and the frame's own types all the same, so the frame's
    /// own functions take them without a shim.
    remote: RemotePlayers,
    sim: crate::logic::entities::Entities,
    death: DeathScreen,

    pub inventory: Inventory,
    pub equipment: primitive_shared::inventory::Equipment,
    pub health: f32,
    pub dead: Option<String>,
    /// Everything the server says about the body: how tired, how hurt,
    /// how thirsty, and whether it is on the ground. The frame's own
    /// type, because the frame's own functions read it -- see
    /// [`Scenario::downed`], which is the one field of it tests ask about
    /// often enough to deserve a name.
    pub body: BodyGauges,
    pub chest_screen: ChestScreen,
    pub station_screen: StationScreen,
    /// The map: the client's survey of the chunks it has been sent, and
    /// the walk the server says this player has made (`logic::map`). Kept
    /// here rather than in a test's own local, because both halves of it
    /// arrive through the frame -- the chunks and the `Trail` -- and a
    /// scenario that built one by hand would be testing neither.
    pub explored: crate::logic::map::ExploredMap,

    /// Every correction the server has sent that the scenario did not ask
    /// for with [`Scenario::stand_at`], with its reason.
    pub corrections: Vec<String>,
    /// Every message from the server except chunks, in order.
    pub heard: Vec<ServerMessage>,

    world_ready: bool,
    frames: u64,
    /// Where this scenario's clock started. Every "now" the frame hands
    /// to the client's own machinery is this plus a whole number of
    /// frames -- see [`Scenario::now`].
    epoch: Instant,
    /// When the last frame ran **by the wall**, and the only field here
    /// that is: it holds the floor under a frame's length. See the
    /// module note.
    last_frame: Instant,
    /// The one clock every client on this server shares. See
    /// [`WorldClock`].
    clock: std::sync::Arc<WorldClock>,
    /// Ticks a second the server was started with -- how many of them a
    /// frame owes it.
    tick_hz: f32,
    /// **By the wall**, like `last_frame` and unlike the rest: see the
    /// `maybe_send_transform` call in `frame`.
    last_sent_at: Instant,
    last_sent_transform: Option<(DVec3, f32, f32)>,
    sequence: u32,
    /// A teleport the scenario asked for, and so not a correction.
    expected_move: Option<DVec3>,
    /// ...and, for a second after it lands, the same place again: see the
    /// correction arm of `drain`.
    grace: Option<(DVec3, u64)>,
    /// Frames the jump key is to be reported as freshly pressed.
    jump_edge: bool,
    /// The horse this client is riding, ahead of the server: the frame's own
    /// `Entities::horseback`, stepped by `step_body` the way `run` steps it.
    pub horseback: Option<crate::logic::horseback::Horseback>,
    /// The latest snapshot of every entity the server has sent, by id.
    pub entities: HashMap<primitive_shared::protocol::EntityId, primitive_shared::protocol::EntityState>,
}

/// **How far the world has got, in frames, and how many ticks that has
/// already bought.** One of these per server, shared by every client on
/// it (`Scenario::join`).
///
/// Why it is shared rather than one per client: two clients on one server
/// are two people in one world, and the world cannot be at two times at
/// once. A per-client counter gets both of the patterns tests use wrong
/// -- interleaved (`until_both`, a trade at a stall) the world would age
/// twice as fast as either player, and taking turns (a guest joins, does
/// something while the host stands still, and leaves) the client standing
/// still would find the world frozen the moment it started stepping
/// again, because the other one had already spent that time. That second
/// one is not hypothetical: it is what
/// `a_rider_who_disconnects_in_the_saddle` did -- the guest's frames
/// bought no ticks at all, so the server never sent an entity snapshot
/// and the horse "never reached the client".
///
/// So: a frame of any client moves the world to one past wherever it
/// already was *for that client*, and never backwards for anyone. A
/// client that has been idle picks the world up where the other left it.
struct WorldClock {
    inner: std::sync::Mutex<WorldClockState>,
}

#[derive(Default)]
struct WorldClockState {
    /// Frames of scenario time the world has lived through.
    frames: u64,
    /// Ticks already asked of the server for them.
    asked: u64,
}

/// What one client's frame did to the shared clock: where the world is
/// now, and how many ticks the server has been asked for in all.
struct Stepped {
    frames: u64,
    ask: u32,
    ticks: u64,
}

impl WorldClock {
    fn new() -> Self {
        Self { inner: std::sync::Mutex::new(WorldClockState::default()) }
    }

    /// One frame on, for a client that was at `mine`.
    fn step(&self, mine: u64, tick_hz: f32) -> Stepped {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let frames = (mine + 1).max(state.frames);
        state.frames = frames;
        let ticks = (frames as f64 * f64::from(tick_hz) * f64::from(FRAME)) as u64;
        let ask = ticks.saturating_sub(state.asked);
        state.asked = ticks.max(state.asked);
        Stepped { frames, ask: ask as u32, ticks }
    }

    fn frames(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).frames
    }
}

/// The server a scenario plays on: the test world, the validator on.
///
/// **The anticheat is on**, unlike a singleplayer world
/// (`ClientSettings::singleplayer_server` turns it off). A scenario is the
/// strictest reading of "did that feel right": a server that corrects the
/// player is a server that would have rubber-banded them in multiplayer,
/// and the hover false-positive out of water was exactly that.
pub fn scenario_settings() -> primitive_server::settings::ServerSettings {
    primitive_server::settings::ServerSettings {
        bind_addr: "127.0.0.1:0".to_string(),
        server_name: "scenario".to_string(),
        world_dir: String::new(),
        plugin_dir: String::new(),
        mod_dir: String::new(),
        stats_interval_secs: 0.0,
        world_preset: primitive_shared::worldgen::Preset::Test,
        view_distance_chunks: RENDER_DISTANCE + 1,
        // Noon, and a day long enough that it stays noon: the peat test
        // wants the sun, and nothing else here wants the night.
        start_time_of_day: 0.5,
        ..Default::default()
    }
}

impl Scenario {
    /// The test world, a player in it, and the ground under them loaded.
    pub fn new() -> Self {
        Self::with(scenario_settings())
    }

    pub fn with(settings: primitive_server::settings::ServerSettings) -> Self {
        Self::started(settings, true)
    }

    /// The same, but **the server keeps its own 20 Hz wall clock** rather
    /// than taking its ticks from the frame.
    ///
    /// For the two scenarios whose subject *is* that clock: the phone put
    /// in a pocket and the computer put to sleep, which assert about what
    /// the world does while nobody is drawing frames at all. A server
    /// that ticked only when a frame asked would make both of them pass
    /// by saying nothing -- there would be no frames, so of course the
    /// world stood still. Everything else wants the frame's clock; see
    /// the module note.
    pub fn with_a_server_on_its_own_clock(settings: primitive_server::settings::ServerSettings) -> Self {
        Self::started(settings, false)
    }

    fn started(settings: primitive_server::settings::ServerSettings, by_hand: bool) -> Self {
        let tick_hz = settings.tick_rate_hz;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let options = primitive_server::RunOptions::embedded();
        let options = if by_hand { options.ticks_by_hand() } else { options };
        let (server, connection) = runtime.block_on(async {
            let server = primitive_server::start(settings, options)
                .await
                .expect("the server did not start");
            let connection = network::connect(&server.address().to_string(), "scenario")
                .await
                .expect("the server refused the scenario");
            (server, connection)
        });
        Self::connected(
            runtime,
            std::sync::Arc::new(server),
            connection,
            "scenario",
            tick_hz,
            std::sync::Arc::new(WorldClock::new()),
        )
    }

    /// **A second player on the same server**, called `name`: its own
    /// connection, its own chunks, body, pack and screens, played frame by
    /// frame exactly as this one is.
    ///
    /// Why it exists: a trade at a stall is between two people, and every
    /// rule that matters about one -- who may touch the counter, what the
    /// buyer sees when the owner changes a price -- is about what one client
    /// is told because of what the *other* did. One client could only test
    /// the server's half of that. Rejected: a second connection driven by
    /// raw messages, with no screens. It would pass while the buyer's screen
    /// drew a TRADE button in a place its click did not land, which is the
    /// half of the stall a player touches.
    ///
    /// Frames are the caller's to interleave (`until_both`): a client that
    /// is not stepped still has its socket read in the background, it just
    /// does not look at what arrived.
    pub fn join(&self, name: &str) -> Scenario {
        let server = std::sync::Arc::clone(self.server.as_ref().expect("a scenario without a server has nobody to join"));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let address = server.address().to_string();
        let connection = runtime
            .block_on(network::connect(&address, name))
            .expect("the server refused the second player");
        // **The guest joins the world's clock, not a new one.** Its
        // frames are the same frames; a second counter starting at zero
        // would be a second player living in a world that is already an
        // hour old. See `WorldClock`.
        Self::connected(runtime, server, connection, name, self.tick_hz, std::sync::Arc::clone(&self.clock))
    }

    fn connected(
        runtime: tokio::runtime::Runtime,
        server: std::sync::Arc<primitive_server::Server>,
        connection: network::Connection,
        name: &str,
        tick_hz: f32,
        clock: std::sync::Arc<WorldClock>,
    ) -> Self {
        let welcome = connection.welcome;
        let spawn = DVec3::new(welcome.spawn.0, welcome.spawn.1, welcome.spawn.2);
        let settings = ClientSettings::default();
        let player = Player::new(spawn, settings.move_speed);
        let mut camera = Camera::new(player.eye_position(), 16.0 / 9.0);
        camera.yaw = 0.0;
        let now = Instant::now();
        let mut scenario = Scenario {
            runtime,
            server: Some(server),
            name: name.to_string(),
            net: connection.handle,
            welcome,
            chunks: ChunkManager::new(RENDER_DISTANCE),
            light: LightMap::new(),
            arrivals: Arrivals::default(),
            urgent: VecDeque::new(),
            dirty_set: MeshQueueSet::new(),
            versions: HashMap::new(),
            player,
            camera,
            input: InputState::default(),
            settings,
            mining: Mining::new(),
            debug: DebugStats::default(),
            audio: crate::Audio::silent(),
            soundscape: crate::Soundscape::new(),
            hand: hand::Hand::new(),
            strikes: hand::Strikes::default(),
            shake: shake::Shake::new(0.0),
            dig_signal: DigSignal::default(),
            stamina: stamina::Stamina::new(),
            rod_hold: crate::logic::fishing::Hold::default(),
            cut: None,
            meal: None,
            meal_sent: None,
            notice: None,
            remote: RemotePlayers::default(),
            sim: crate::logic::entities::Entities::default(),
            death: DeathScreen::new(),
            inventory: Inventory::new(),
            equipment: Default::default(),
            health: 1.0,
            dead: None,
            body: BodyGauges::default(),
            chest_screen: ChestScreen::new(),
            explored: crate::logic::map::ExploredMap::default(),
            station_screen: StationScreen::new(),
            corrections: Vec::new(),
            heard: Vec::new(),
            world_ready: false,
            // Where the world already is, so a guest's first frame is the
            // next one and not the first one ever. See `WorldClock`.
            frames: clock.frames(),
            // ...and the epoch put back behind it by as much, so this
            // client's "now" is still a real-looking instant rather than
            // one an hour in its own future.
            epoch: now
                .checked_sub(Duration::from_secs_f64(clock.frames() as f64 * f64::from(FRAME)))
                .unwrap_or(now),
            clock,
            last_frame: now,
            tick_hz,
            last_sent_at: now,
            last_sent_transform: None,
            sequence: 0,
            expected_move: None,
            grace: None,
            jump_edge: false,
            horseback: None,
            entities: HashMap::new(),
        };
        // **A player in a world has the cursor.** The frame refuses to
        // mine without it (`can_mine`), which is right -- the click that
        // grabs the pointer is not also a swing -- and a harness that
        // left it false is a harness in which nothing can ever be dug.
        scenario.input.mouse_grabbed = true;
        let ready = scenario.until(20.0, |s| s.world_ready && s.player.grounded);
        assert!(ready, "the world never arrived round the spawn");
        scenario
    }

    pub fn server(&self) -> &primitive_server::Server {
        self.server.as_ref().expect("server")
    }

    /// On the ground, as the server last said. A reading of [`Scenario::body`]
    /// rather than a field of its own: the frame keeps one answer to
    /// "what is this body doing", and two would be two to keep in step.
    pub fn downed(&self) -> Option<primitive_shared::downed::Down> {
        self.body.downed
    }

    // ---- setting the stage ----

    /// Puts blocks into the world as the generator would have, and waits
    /// until this client has been told about every one of them.
    pub fn build(&mut self, cells: &[((i32, i32, i32), BlockId)]) {
        for &((x, y, z), block) in cells {
            self.server().place_block(x, y, z, block);
        }
        // **Twelve hundred frames, and it is not a guess about the world.**
        // What is waited for is the server's own edit coming back down the
        // wire and through chunk integration -- real work on real threads,
        // paced by how busy this machine is and not by anything the test
        // is asserting. A frame is never shorter than a sixtieth of a
        // second of wall time, so this is still at least twenty seconds of
        // real time for the arrivals to use, and more when the machine is
        // loaded. At five seconds it failed about one run in three on a
        // machine with half a dozen builds on it, in whichever scenario
        // happened to be unlucky; a run where the blocks arrive costs the
        // same either way, because `until` returns the moment they do.
        let all_there = self.until(20.0, |s| {
            cells.iter().all(|&((x, y, z), block)| s.chunks.block_at(x, y, z) == Some(block))
        });
        let missing = cells.iter().find(|&&((x, y, z), block)| self.chunks.block_at(x, y, z) != Some(block));
        let has = missing.map(|&((x, y, z), _)| (self.chunks.block_at(x, y, z), self.server().block_at(x, y, z)));
        assert!(all_there, "the client was never told about what was built: {missing:?}, it has {has:?}");
    }

    /// A box of one block, corners inclusive.
    pub fn fill(&mut self, from: (i32, i32, i32), to: (i32, i32, i32), block: BlockId) {
        let mut cells = Vec::new();
        for x in from.0.min(to.0)..=from.0.max(to.0) {
            for y in from.1.min(to.1)..=from.1.max(to.1) {
                for z in from.2.min(to.2)..=from.2.max(to.2) {
                    cells.push(((x, y, z), block));
                }
            }
        }
        self.build(&cells);
    }

    /// Stands the player at a place, the way `/tp` does, and waits until
    /// both sides agree they are there and on their feet. **Not counted
    /// as a correction**: the scenario asked for it.
    pub fn stand_at(&mut self, feet: (f64, f64, f64)) {
        let at = DVec3::new(feet.0, feet.1, feet.2);
        // **Whatever the set-down itself costs is the harness's, not the
        // player's.** A body put down where the chunk under it has not
        // arrived yet falls until it does, and the anticheat reads that as
        // a hundred blocks a second -- a scenario turned red by a busy
        // machine rather than by anything a player did. So: wait for the
        // body to be standing (or swimming, which several scenarios set
        // down into), and forget the corrections that waiting produced.
        // Corrections from here on are the scenario's own.
        let before = self.corrections.len();
        self.expected_move = Some(at);
        self.server().teleport_named(&self.name, feet.0 as f32, feet.1 as f32, feet.2 as f32);
        let arrived = self.until(10.0, |s| s.expected_move.is_none() && s.world_ready);
        assert!(arrived, "the teleport to {feet:?} never came back");
        self.seconds(0.3);
        self.until(2.0, |s| s.player.grounded || s.player.in_water);
        self.corrections.truncate(before);
    }

    /// Gives the player something, and waits until the pack says so.
    pub fn give(&mut self, block: BlockId, count: u32) {
        let before = self.inventory.count(block);
        assert_eq!(self.server().give_to(&self.name, block, count), 0, "{count} of it did not fit");
        let got = self.until(5.0, |s| s.inventory.count(block) >= before + count);
        assert!(got, "the pack never showed what was given");
    }

    /// Selects the hotbar square holding `block`.
    pub fn select(&mut self, block: BlockId) {
        let slot = (0..crate::logic::inventory::HOTBAR_SLOTS)
            .find(|&s| self.inventory.block_in(s).is_some_and(|b| primitive_shared::types::block_kind(b) == primitive_shared::types::block_kind(block)))
            .unwrap_or_else(|| panic!("{} is not on the hotbar", primitive_shared::types::block_name(block)));
        self.input.hotbar_slot = slot;
        // The frame tells the server whenever the square changes; a
        // placement spends from the square the *server* thinks is held.
        self.net.send(ClientMessage::SelectSlot { slot: slot as u8 });
        self.frames(3);
    }

    // ---- looking ----

    /// Turns the head to look at a point.
    pub fn look_at(&mut self, point: DVec3) {
        let dir = (point - self.player.eye_position()).as_vec3().normalize();
        self.camera.yaw = dir.z.atan2(dir.x);
        self.camera.pitch = dir.y.asin();
        self.camera.position = self.player.eye_position();
    }

    /// Turns to look at the middle of a face of a cell: `face` is the
    /// outward normal, (0, 1, 0) for the top.
    pub fn look_at_face(&mut self, cell: (i32, i32, i32), face: (i32, i32, i32)) {
        let middle = DVec3::new(cell.0 as f64 + 0.5, cell.1 as f64 + 0.5, cell.2 as f64 + 0.5);
        let offset = DVec3::new(face.0 as f64, face.1 as f64, face.2 as f64) * 0.49;
        self.look_at(middle + offset);
    }

    /// Faces a compass direction along the ground: yaw 0 is +x.
    pub fn face(&mut self, yaw: f32) {
        self.camera.yaw = yaw;
        self.camera.pitch = 0.0;
    }

    /// What the crosshair is on, as the right click sees it.
    pub fn aimed(&self) -> Option<((i32, i32, i32), BlockId)> {
        crate::aimed_block(&self.chunks, &self.camera)
    }

    // ---- hands ----

    pub fn hold(&mut self, action: Action) {
        let key = self.settings.keybinds.key(action).expect("an unbound action");
        self.input.set_key(key, true);
        if action == Action::Jump {
            self.jump_edge = true;
        }
    }

    pub fn release(&mut self, action: Action) {
        let key = self.settings.keybinds.key(action).expect("an unbound action");
        self.input.set_key(key, false);
    }

    pub fn release_all(&mut self) {
        self.input.release_all();
    }

    pub fn key(&mut self, key: Key, down: bool) {
        self.input.set_key(key, down);
    }

    /// **The right click, through the frame's own dispatch.**
    ///
    /// It was a copy of that arm: the harness decided for itself what a
    /// blaze, a set-down, a station, a chest and a placement sent, and
    /// panicked on everything it had not been taught. So a scenario
    /// could pass while the game did something else with the same
    /// click, and a gesture the game grew was a gesture the harness
    /// refused to play.
    ///
    /// Now it is [`crate::frame::interact::right_click_on_the_world`],
    /// which is the function the game calls. What stays outside it is
    /// what is about an entity rather than a block -- a raft, a horse,
    /// an animal, somebody lying on the ground -- and no scenario plays
    /// those through here.
    pub fn use_aimed(&mut self) {
        let held = self.inventory.block_in(self.input.hotbar_slot);
        crate::frame::interact::right_click_on_the_world(
            // No second finger on a test's glass: the modifier a
            // scenario means is the sprint key it is holding.
            false,
            held,
            &self.settings,
            Some(&self.net),
            &self.audio,
            &self.player,
            &self.camera,
            &self.body,
            &self.sim,
            &[],
            &self.input,
            &self.inventory,
            &mut self.chunks,
            &mut self.light,
            &mut self.arrivals,
            &mut self.urgent,
            &mut self.dirty_set,
            &mut self.versions,
            &mut self.chest_screen,
            &mut self.station_screen,
            &mut self.mining,
            &mut self.hand,
            &mut self.cut,
            &mut self.meal,
            &mut self.notice,
            &mut self.debug,
        );
    }

    /// Shuts whatever container screen is open, as Escape does.
    pub fn close_screens(&mut self) {
        crate::close_chest(&mut self.chest_screen, Some(&self.net), &mut self.debug);
        crate::close_station(&mut self.station_screen, Some(&self.net), &mut self.debug);
    }

    /// A click at `cursor` on the open container screen, through the
    /// screen's own hit test and `click`, and whatever it asks sent the way
    /// the frame sends it (`crate::chest_intent_message`). `cursor` is in
    /// the screen's own space -- a rect's centre from `chest_screen`.
    pub fn chest_click(&mut self, cursor: (f32, f32)) {
        self.chest_screen.set_cursor(Some(cursor));
        let intent = self.chest_screen.click(&self.inventory, crate::ui::inventory_screen::Button::Left, false, false);
        if let Some(message) = intent.and_then(crate::chest_intent_message) {
            self.net.send(message);
        }
    }

    /// The same, shift held: a stack straight across.
    pub fn chest_shift_click(&mut self, cursor: (f32, f32)) {
        self.chest_screen.set_cursor(Some(cursor));
        let intent = self.chest_screen.click(&self.inventory, crate::ui::inventory_screen::Button::Left, true, false);
        if let Some(message) = intent.and_then(crate::chest_intent_message) {
            self.net.send(message);
        }
    }

    /// Sends a message the way a screen's click would have. For the
    /// gestures whose screen logic is its own test (a shift-click in the
    /// chest grid): the scenario says what was clicked, the wire is real.
    pub fn send(&mut self, message: ClientMessage) {
        self.net.send(message);
    }

    /// The space bar at an open station, through the screen's own `press`.
    pub fn station_press(&mut self) {
        if let Some(intent) = self.station_screen.press() {
            crate::send_station_intent(intent, &mut self.station_screen, &self.net, &mut self.debug, &self.audio);
        }
    }

    /// A click on a job's row of the open station, through the screen's
    /// own hit-test and `click` -- the cursor put in the middle of the row
    /// the screen draws for it.
    ///
    /// **Through `click`, not by sending `Begin`**: the screen remembers
    /// which job it asked for (`pending`) and only starts its bar when the
    /// server's seed comes back for it. The first version of this sent the
    /// message itself, the server began, and the screen sat still -- the
    /// same screen, from the player's side, as a station that never
    /// answers.
    pub fn station_begin(&mut self, job: primitive_shared::minigame::Job) {
        use crate::ui::station_screen::{jobs_of, Panel};
        let game = job.game();
        let index = jobs_of(game).iter().position(|&j| j == job).expect("a job this station does not offer");
        let row = Panel::for_game(game).row(index);
        self.station_screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        let intent = self.station_screen.click();
        self.station_screen.set_cursor(None);
        let intent = intent.expect("the job's row did not answer a click");
        crate::send_station_intent(intent, &mut self.station_screen, &self.net, &mut self.debug, &self.audio);
    }

    // ---- time ----

    /// **The scenario's own clock**: the frame counter, in the shape the
    /// client's machinery expects a clock in.
    ///
    /// Everything the frame hands a "now" to -- the transform's send
    /// rate, the chunk manager's unload timers, the reins' resend, the
    /// placement's settle -- takes an `Instant`, so a counter can be one
    /// without a single signature changing. Monotonic and always behind
    /// the wall, because a frame is never shorter than `FRAME` of it.
    fn now(&self) -> Instant {
        self.epoch + Duration::from_secs_f64(self.frames as f64 * f64::from(FRAME))
    }

    /// **The world ages by the frame, not by the wall.**
    ///
    /// The server was started with its ticks in this client's hands
    /// (`RunOptions::ticks_by_hand`); this is the hand. After frame *n*
    /// the server has run exactly `n * tick_hz / 60` ticks -- on an idle
    /// machine, which is what it did before, and on a machine with half a
    /// dozen builds on it, which is what it did not.
    ///
    /// **Waited for, not merely asked for.** A scenario reads the
    /// server's own state the line after it stops stepping
    /// (`s.server().asleep(...)`, `horse_gear`, `player_riding`), and a
    /// pile of unspent asks would mean reading a world a dozen ticks
    /// behind the one the frames drove. So the frame does not return
    /// until the tick it bought has been run.
    fn pace_the_world(&mut self) {
        let stepped = self.clock.step(self.frames, self.tick_hz);
        // Picked up where another client left it, if one has been
        // stepping while this one stood still. See `WorldClock`.
        self.frames = stepped.frames;
        let server = self.server.as_ref().expect("a scenario without a server has no world to age");
        if stepped.ask > 0 {
            server.ask_for_ticks(stepped.ask);
        }
        // The cap is wall time and it is not a timing assertion: it is
        // the difference between a test that fails and a suite that
        // hangs. Nothing asserts on it, and a tick that took thirty
        // seconds is a broken server, not a busy machine.
        const GIVE_UP: Duration = Duration::from_secs(30);
        assert!(
            server.wait_for_ticks(stepped.ticks, GIVE_UP),
            "the server did not run tick {} within {GIVE_UP:?} -- it is not ticking at all",
            stepped.ticks
        );
    }

    /// One frame of the game, as `run` orders it: the world aged by one
    /// frame's worth, the socket, the chunks asked for, the body, the
    /// hands, what is sent back.
    pub fn frame(&mut self) {
        // **Never shorter than a frame of wall time, and never catching
        // up.** The only thing here that reads the wall, and the module
        // note says what it is for: the anticheat bills a player by
        // `Instant::now()`, so a harness that ran faster than the wall
        // would be corrected for speed.
        let due = self.last_frame + Duration::from_secs_f32(FRAME);
        let wall = Instant::now();
        if due > wall {
            std::thread::sleep(due - wall);
        }
        self.last_frame = Instant::now();
        // One frame on, and the world with it: `pace_the_world` is what
        // moves `frames`, because the world's clock is shared and this
        // client's is only its own view of it.
        self.pace_the_world();
        let now = self.now();

        self.drain();
        // The map's own streaming phase, as the frame runs it -- with no
        // budget, because a scenario has no frame to be late for and a
        // half-surveyed map is a test that passes on a fast machine.
        self.explored.catch_up(&self.chunks, Duration::from_secs(1));
        for at in self.explored.take_lost_marks() {
            self.net.send(ClientMessage::ForgetMark { global_x: at.0, global_y: at.1, global_z: at.2 });
        }

        let player_chunk = ChunkManager::chunk_for_world_pos(self.player.position.x, self.player.position.z);
        let (to_request, to_unload) = self.chunks.update(player_chunk, now);
        for batch in to_request.chunks(crate::MAX_REQUEST_BATCH) {
            self.net.send(ClientMessage::RequestChunks(batch.to_vec()));
        }
        for pos in to_unload {
            self.chunks.unload(pos);
            self.light.unload_chunk(pos);
        }
        if !self.world_ready && self.chunks.is_area_ready(player_chunk) {
            self.world_ready = true;
        }

        if self.world_ready && self.dead.is_none() {
            self.step_body();
            // No pick on the ground: `run`'s `can_mine`.
            if self.body.downed.is_none() {
                self.step_hands();
            }
        }

        // **This one keeps the wall, and it is the keepalive that says
        // why.** Everything else the frame times is the client talking to
        // itself; this is the client talking to the server, and the
        // server decides a player is gone after `client_timeout_secs` of
        // *its* seconds. `run` sends ten a second by the wall, so a
        // scenario that sent ten a simulated second would, on a loaded
        // machine where a frame takes a fifth of a second, speak once
        // every one and a half -- and a world with a short fuse
        // (`impatient_world`) kicked it for silence. Sending oftener
        // than the body moves is safe by construction: the validator's
        // speed budget is blocks against wall seconds, and smaller steps
        // over the same wall are a slower player, never a faster one.
        crate::maybe_send_transform(
            &mut self.net,
            &self.player,
            &self.camera,
            self.last_frame,
            Duration::from_secs_f32(1.0 / crate::settings::ClientSettings::default().player_update_hz),
            &mut self.last_sent_at,
            &mut self.last_sent_transform,
            &mut self.sequence,
            &mut self.debug,
        );
        // What the frame's mesher does with an edited chunk, done here and
        // now: rebuilt, and the lids it left out handed to the frame
        // (`ChunkManager::note_meshed_lids`). Without this a lid would
        // never swing, exactly as in the game if the mesh never landed.
        while let Some(pos) = self.urgent.pop_front() {
            self.dirty_set.remove(&pos);
            let (_, left_out) = self.build_mesh(pos);
            self.chunks.note_meshed_lids(pos, &left_out);
        }
        if let Some(intent) = self.station_screen.poll() {
            crate::send_station_intent(intent, &mut self.station_screen, &self.net, &mut self.debug, &self.audio);
        }
        self.chunks.advance_lids(FRAME);
        self.input.end_frame();
        self.jump_edge = false;
    }

    fn step_body(&mut self) {
        let frozen = self.chest_screen.is_open() || self.station_screen.is_open();
        // **On a horse the keys ride**, exactly as `run` has them (the horse
        // block after the raft's there): the reins off the same wish, the
        // horse predicted, the reins sent, the body put on the saddle and not
        // stepped.
        if let Some(mut horseback) = self.horseback.take() {
            let wish = if frozen { Vec3::ZERO } else { crate::wish_direction(&self.input, &self.camera, &self.settings.keybinds) };
            let forward = wish.dot(self.camera.forward_horizontal()) * self.input.stick_speed();
            let turn = wish.dot(self.camera.right_horizontal()) * self.input.stick_speed();
            let reins = crate::logic::horseback::Horseback::reins_from_keys(
                forward,
                turn,
                !frozen && self.input.action_down(&self.settings.keybinds, Action::Sprint),
                !frozen && self.input.action_down(&self.settings.keybinds, Action::Rein),
                !frozen && (self.jump_edge || self.input.action_pressed(&self.settings.keybinds, Action::Jump)),
            );
            // **By `FRAME`, like the body, and it used to be by the wall.**
            // It had to be: the server's horse moved on the server's own
            // clock, so a horse predicted at a sixtieth of a second while a
            // debug frame took a twentieth was a horse at a third of the
            // speed of the real one, snapped back to it every second. Now
            // the server's horse moves one tick per three frames, which is
            // exactly this -- and on a loaded machine too, which the wall
            // never managed.
            horseback.predict(reins, &|x, y, z| self.chunks.block_at(x, y, z), FRAME);
            if let Some(message) = horseback.rein_message(self.now()) {
                self.net.send(message);
            }
            if !frozen
                && forward.abs() < 0.05
                && horseback.may_get_down()
                && self.input.action_pressed(&self.settings.keybinds, Action::Rein)
            {
                self.net.send(ClientMessage::Dismount);
            }
            self.player.position = horseback.rider_feet();
            self.player.velocity = Vec3::ZERO;
            self.player.grounded = horseback.body.on_ground;
            self.camera.position = self.player.eye_position();
            self.horseback = Some(horseback);
            return;
        }
        let carried = self.inventory.total_weight() + self.equipment.weight();
        // The crawl in place of the limp, as `run` has it.
        self.player.speed_scale = primitive_shared::load::speed_scale(carried)
            * self.equipment.worn().mobility()
            * self.body.downed.map_or(self.body.injuries.speed_factor(), |down| down.crawl());
        self.player.snowshoes = self.equipment.snowshoes();
        self.player.buoyancy = primitive_shared::load::buoyancy(carried);
        self.player.treading = frozen;
        let sprinting = !frozen
            && self.input.action_down(&self.settings.keybinds, Action::Sprint)
            && self.body.injuries.may_sprint()
            && self.body.downed.is_none();
        let wish = if frozen {
            Vec3::ZERO
        } else {
            crate::wish_direction(&self.input, &self.camera, &self.settings.keybinds)
        };
        let jump_pressed = !frozen
            && primitive_shared::load::can_jump(carried)
            && self.body.downed.is_none()
            && (self.jump_edge || self.input.action_pressed(&self.settings.keybinds, Action::Jump));
        let jump_held = !frozen && self.input.action_down(&self.settings.keybinds, Action::Jump);
        self.player.update(&self.chunks, &[], wish, self.camera.forward(), jump_pressed, jump_held, sprinting, FRAME);
        self.camera.position = self.player.eye_position();
        if let Some(down) = self.body.downed.as_mut() {
            self.camera.position = self.player.position
                + DVec3::new(0.0, f64::from(primitive_shared::downed::CRAWL_EYE), 0.0);
            down.tick(FRAME);
        }
    }

    /// **One frame of the hands, through the frame's own.**
    ///
    /// It was a copy of the mining half and nothing else: the harness
    /// dug at a flat `FRAME` a swing, never billed the tank, never
    /// struck anybody, never finished a cut or a mouthful, and never
    /// told the server this player was working. Every one of those is a
    /// thing a player feels and a thing a scenario could not see.
    fn step_hands(&mut self) {
        crate::frame::hands::step(
            FRAME,
            self.now(),
            self.world_ready,
            // A scenario is never paused and never trimming a sail: it
            // has no pause menu, and no scenario plays the sheets.
            false,
            None,
            &self.death,
            &self.body,
            &self.input,
            &self.inventory,
            &self.player,
            &mut self.remote,
            &self.chunks,
            &self.camera,
            &self.sim,
            &self.rod_hold,
            &self.net,
            &self.audio,
            &mut self.soundscape,
            &mut self.mining,
            &mut self.stamina,
            &mut self.strikes,
            &mut self.hand,
            &mut self.shake,
            &mut self.dig_signal,
            &mut self.cut,
            &mut self.meal,
            &mut self.meal_sent,
            &mut self.debug,
        );
    }

    /// The arms of `drain_network` a scenario reads. See the module note.
    fn drain(&mut self) {
        while let Ok(incoming) = self.net.to_game.try_recv() {
            let message = match incoming {
                network::Incoming::Chunk(chunk) => {
                    self.explored.note_chunk(chunk.pos);
                    self.chunks.insert(*chunk);
                    continue;
                }
                network::Incoming::Message(message) => message,
            };
            match &message {
                ServerMessage::BlockUpdate(change) => self.apply(*change),
                ServerMessage::BlockUpdates(changes) => {
                    for change in changes.clone() {
                        self.apply(change);
                    }
                }
                ServerMessage::PositionCorrection { x, y, z, reason } => {
                    let at = DVec3::new(*x, *y, *z);
                    let asked_for = |wanted: Option<DVec3>| wanted.is_some_and(|w| w.distance(at) < 0.01);
                    if asked_for(self.expected_move) {
                        self.expected_move = None;
                        self.grace = Some((at, self.frames + 60));
                    } else if asked_for(self.grace.map(|(w, _)| w)) && self.grace.is_some_and(|(_, until)| self.frames < until) {
                        // The transform that was already in the socket when
                        // the teleport went out, from where the player *was*:
                        // the validator sends them back to the new place, and
                        // the real client races `/tp` exactly the same way.
                    } else {
                        self.corrections.push(format!(
                            "{reason} (from {:.2},{:.2},{:.2} to {x:.2},{y:.2},{z:.2}, frame {})",
                            self.player.position.x, self.player.position.y, self.player.position.z, self.frames
                        ));
                    }
                    self.player.teleport(at);
                    self.camera.position = self.player.eye_position();
                    crate::respawn_gate(&self.chunks, *x, *z, &mut self.world_ready);
                }
                ServerMessage::Ping { nonce } => {
                    self.net.send(ClientMessage::Pong { nonce: *nonce });
                }
                ServerMessage::InventoryState { inventory } => {
                    let mut state = inventory.clone();
                    state.sanitize();
                    self.inventory = state;
                    self.chest_screen.sync_with(&self.inventory);
                }
                ServerMessage::EquipmentState { equipment } => self.equipment = equipment.clone(),
                ServerMessage::Injuries { injuries } => self.body.injuries = *injuries,
                ServerMessage::Health { current, max } => {
                    self.health = if *max > 0.0 { current / max } else { 0.0 };
                }
                ServerMessage::Downed { down } => self.body.downed = *down,
                ServerMessage::Died { cause } => {
                    self.body.downed = None;
                    self.dead = Some(format!("{cause:?}"));
                }
                ServerMessage::Respawned { x, y, z } => {
                    self.dead = None;
                    self.body.downed = None;
                    self.player.teleport(DVec3::new(*x, *y, *z));
                    crate::respawn_gate(&self.chunks, *x, *z, &mut self.world_ready);
                }
                ServerMessage::StationOpen { game, tolerance } => {
                    self.audio.play(crate::audio::Sfx::StationOpen);
                    self.station_screen.show(*game, *tolerance);
                }
                ServerMessage::StationBegun { seed } => self.station_screen.begun(*seed),
                ServerMessage::StationResult { verdict, made } => self.station_screen.finished(*verdict, *made),
                ServerMessage::ChestState { global_x, global_y, global_z, inventory, kind, hearth, rack } => {
                    let at = (*global_x, *global_y, *global_z);
                    if self.chest_screen.wants_state_for(at) {
                        let mut contents = inventory.clone();
                        contents.sanitize();
                        self.chest_screen.show(at, contents, self.chunks.block_at(at.0, at.1, at.2), *kind, *hearth, *rack);
                        self.chest_screen.sync_with(&self.inventory);
                    }
                }
                ServerMessage::ChestClosed => self.chest_screen.close(),
                ServerMessage::StallOffers { global_x, global_y, global_z, owner, yours, offers } => {
                    self.chest_screen.show_stall(crate::ui::chest_screen::StallView {
                        at: (*global_x, *global_y, *global_z),
                        owner: owner.clone(),
                        yours: *yours,
                        offers: offers.clone(),
                    });
                }
                ServerMessage::ChestLid { x, y, z, open } => {
                    if self.chunks.note_chest_lid((*x, *y, *z), *open) {
                        let pos = ChunkPos::from_world(*x, *z);
                        crate::bump_version(&mut self.versions, pos);
                        crate::mark_urgent(&mut self.urgent, &mut self.dirty_set, pos);
                    }
                    if self.chest_screen.at() != Some((*x, *y, *z)) {
                        let sfx = if *open { crate::audio::Sfx::ChestOpen } else { crate::audio::Sfx::ChestClose };
                        self.audio.play_at_block(sfx, (*x, *y, *z), 0.9, 1.0);
                    }
                }
                ServerMessage::SetDownItem { x, y, z, item } => self.chunks.note_set_down((*x, *y, *z), *item),
                ServerMessage::Trail { cells, marks, whole } => {
                    self.explored.trail_arrived(cells.clone(), marks.clone(), *whole);
                }
                // The horse arms of `drain_network`, the same statements.
                ServerMessage::Mounted { horse, at, yaw, wind, fettle } => match horse {
                    Some(id) => match self.horseback.as_mut().filter(|h| h.horse == *id) {
                        Some(riding) => riding.told(*wind, *fettle),
                        None => {
                            self.horseback = Some(crate::logic::horseback::Horseback::new(
                                *id,
                                DVec3::new(at.0, at.1, at.2),
                                *yaw,
                                *wind,
                                *fettle,
                            ));
                        }
                    },
                    None => self.horseback = None,
                },
                ServerMessage::Posture { at: Some((x, y, z)), .. } => {
                    self.player.teleport(DVec3::new(*x, *y, *z));
                    self.camera.position = self.player.eye_position();
                }
                ServerMessage::Entities { states, .. } => {
                    if let Some(riding) = self.horseback.as_mut() {
                        riding.server_saw(states);
                    }
                    for state in states {
                        self.entities.insert(state.id, *state);
                    }
                    // Snapshots are twenty a second: not kept in `heard`.
                    continue;
                }
                _ => {}
            }
            self.heard.push(message);
        }
    }

    fn apply(&mut self, change: primitive_shared::protocol::BlockChange) {
        self.mining.confirm_placement(&change, self.now());
        self.explored.note_edit(change.global_x, change.global_z);
        crate::sound_for(&self.audio, &mut self.soundscape, &self.chunks, &change);
        crate::apply_change(
            &mut self.chunks,
            &mut self.light,
            &mut self.arrivals,
            &mut self.urgent,
            &mut self.dirty_set,
            &mut self.versions,
            change,
        );
    }

    pub fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.frame();
        }
    }

    pub fn seconds(&mut self, seconds: f32) {
        self.frames((seconds / FRAME).round() as u32);
    }

    /// [`Scenario::until`], stepping this client and `other` a frame each
    /// in turn, so neither stops reading its socket while the other waits.
    ///
    /// **Counted in frames, as `until` is, and it used to be a deadline
    /// of `Instant::now`.** A wall-clock window is a different number of
    /// frames on a busy machine than on a quiet one, which is a stall
    /// scenario turning red because something else was compiling. See
    /// the module note.
    pub fn until_both(&mut self, other: &mut Scenario, seconds: f32, mut done: impl FnMut(&Self, &Scenario) -> bool) -> bool {
        let frames = (seconds / FRAME).ceil() as u32;
        for _ in 0..frames {
            if done(self, other) {
                return true;
            }
            self.frame();
            other.frame();
        }
        done(self, other)
    }

    /// Runs frames until `done` says so or `seconds` of them have passed,
    /// and says which. `seconds` is the scenario's own seconds: sixty
    /// frames to one, whatever the wall is doing.
    pub fn until(&mut self, seconds: f32, mut done: impl FnMut(&Self) -> bool) -> bool {
        let frames = (seconds / FRAME).ceil() as u32;
        for _ in 0..frames {
            self.frame();
            if done(self) {
                return true;
            }
        }
        false
    }

    // ---- what a player would see ----

    pub fn feet(&self) -> DVec3 {
        self.player.position
    }

    pub fn block(&self, cell: (i32, i32, i32)) -> Option<BlockId> {
        self.chunks.block_at(cell.0, cell.1, cell.2)
    }

    /// Every sound asked for since the scenario began.
    pub fn sounds(&self) -> Vec<crate::audio::Sfx> {
        self.audio.heard.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The chunk as the mesher draws it, now: the real `build_mesh` over
    /// what this client has, lit from scratch.
    pub fn mesh(&self, pos: ChunkPos) -> MeshBuffers {
        self.build_mesh(pos).0
    }

    fn build_mesh(&self, pos: ChunkPos) -> (MeshBuffers, Vec<(i32, i32, i32)>) {
        let mut light = LightMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let around = ChunkPos::new(pos.x + dx, pos.z + dz);
                if self.chunks.is_loaded(around) {
                    light.load_chunk(&self.chunks, around);
                }
            }
        }
        let mut cache = Box::<Neighbourhood>::default();
        cache.fill(pos, &self.chunks, &light);
        cache.set_pottery(self.chunks.pit_pottery_in(pos));
        cache.set_swung_lids(self.chunks.lids_in(pos));
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let generator = primitive_shared::worldgen::WorldGen::new(self.welcome.world_seed);
        let mut out = MeshBuffers::default();
        build_mesh(pos, &cache, &layers, &generator, &mut out);
        let left_out = cache.swung_lids().to_vec();
        (out, left_out)
    }

    /// The box, in world blocks, round everything the mesher drew in the
    /// cells from `low` to `high` (inclusive) -- which is what a player sees
    /// standing there.
    ///
    /// **Only triangles wholly inside the cells, and none lying flat on
    /// their boundary.** The first version took every vertex in the box and
    /// measured the neighbours: the top of the grass under a prop and the
    /// face of the wall behind it both have corners on the cell's edge, and
    /// every shape came out as the whole cell.
    pub fn drawn_bounds(&self, low: (i32, i32, i32), high: (i32, i32, i32)) -> Option<([f32; 3], [f32; 3])> {
        let pos = ChunkPos::from_world(low.0, low.2);
        let mesh = self.mesh(pos);
        let origin = [pos.x as f32 * 16.0, 0.0, pos.z as f32 * 16.0];
        let from = [low.0 as f32, low.1 as f32, low.2 as f32];
        let to = [high.0 as f32 + 1.0, high.1 as f32 + 1.0, high.2 as f32 + 1.0];
        // A hundredth: models are drawn a hair proud of their box (an
        // eight-hundredth, so coplanar faces do not fight), and a tighter
        // tolerance threw a prop out of its own cell.
        const E: f32 = 0.01;
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let mut any = false;
        // Every pass: a model may land in the solid range or among the
        // sprites, and what the eye sees is all of them.
        for tri in mesh.indices.chunks(3) {
            let corners: Vec<[f32; 3]> = tri
                .iter()
                .map(|&i| {
                    let v = mesh.vertices[i as usize].position;
                    [v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]]
                })
                .collect();
            let inside = corners.iter().all(|c| (0..3).all(|i| c[i] >= from[i] - E && c[i] <= to[i] + E));
            let on_boundary = (0..3).any(|i| {
                corners.iter().all(|c| (c[i] - from[i]).abs() < E) || corners.iter().all(|c| (c[i] - to[i]).abs() < E)
            });
            if inside && !on_boundary {
                any = true;
                for c in &corners {
                    for i in 0..3 {
                        lo[i] = lo[i].min(c[i]);
                        hi[i] = hi[i].max(c[i]);
                    }
                }
            }
        }
        any.then_some((lo, hi))
    }

    /// The first message that matches, of those heard so far.
    pub fn heard_any(&self, want: impl Fn(&ServerMessage) -> bool) -> bool {
        self.heard.iter().any(want)
    }

    pub fn physics_solid(&self, cell: (i32, i32, i32)) -> bool {
        self.chunks.is_solid(cell.0, cell.1, cell.2)
    }

    /// Writes what the camera sees to `<PRIMITIVE_SCENARIO_SHOTS>/<name>.png`
    /// through the real renderer, when that variable is set. See the
    /// module note.
    pub fn shot(&self, name: &str) {
        let Ok(dir) = std::env::var("PRIMITIVE_SCENARIO_SHOTS") else {
            return;
        };
        shots::write(self, std::path::Path::new(&dir), name);
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        let _ = self.net.send(ClientMessage::Disconnect);
        // Only the last client holding the server stops it: a guest that
        // leaves first leaves the host's world running.
        if let Some(server) = self.server.take().and_then(|server| std::sync::Arc::try_unwrap(server).ok()) {
            self.runtime.block_on(server.stop());
        }
    }
}

mod shots;

/// The cell under a point.
pub fn cell_of(p: DVec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

/// Where a player stands on the test world's field.
pub const GROUND: i32 = primitive_shared::showcase::GROUND_Y;

/// The start of a strip of plain field, clear of every plot: the test
/// world's structures fill a block of chunks round the spawn and
/// everything past it is grass (see `showcase`).
pub const FIELD: (i32, i32) = (200, 8);

pub fn feet_on(x: i32, z: i32) -> (f64, f64, f64) {
    (x as f64 + 0.5, (GROUND + 1) as f64, z as f64 + 0.5)
}
