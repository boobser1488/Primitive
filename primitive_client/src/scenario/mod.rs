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
//! ## What it does not share with `run`, and why that is said here
//!
//! `run` is one function around a window, and the frame's body is written
//! inline in it. Everything that could be called is called -- the helpers
//! above are the frame's own -- but the *order* they are called in, and
//! the handful of message arms this file handles, are a copy. The copy is
//! kept to the arms a scenario reads (corrections, block updates, the
//! pack, the containers, the stations, health and death), and each is the
//! same statement `drain_network` makes. If `run` grows a step that
//! changes what the player feels, this file needs the same step, or a
//! scenario will pass on a game that no longer exists.
//!
//! Rejected: pulling the whole frame out of `run` into something both
//! could call. That is the right end state and a refactor of six thousand
//! lines of event handling whose every borrow is load-bearing; a harness
//! that waits for it is a harness that does not exist. Also rejected:
//! driving the real binary with synthetic input. The platform layer has no
//! way in that is not a window, and a window is a GPU, which is exactly
//! what an unattended run on a build machine does not have.
//!
//! ## Time is real
//!
//! The server keeps its own clock and so does the anticheat: a scenario
//! that ran its frames faster than the wall would be a player moving
//! faster than the validator allows. So a frame is never shorter than a
//! sixtieth of a second of wall time, and **never catches up** after a
//! stall -- a burst of catch-up frames is a burst of movement per second
//! that the real game (which steps by the frame's own length) never sends.
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
use crate::ui::keybinds::{Action, Keybinds};
use crate::ui::{chest_screen::ChestScreen, station_screen::StationScreen};
use crate::{Arrivals, MeshQueueSet, UseGesture};

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
    binds: Keybinds,
    pub mining: Mining,
    debug: DebugStats,
    pub audio: crate::Audio,
    soundscape: crate::Soundscape,

    pub inventory: Inventory,
    pub equipment: primitive_shared::inventory::Equipment,
    pub injuries: primitive_shared::injury::Injuries,
    pub health: f32,
    pub dead: Option<String>,
    pub chest_screen: ChestScreen,
    pub station_screen: StationScreen,

    /// Every correction the server has sent that the scenario did not ask
    /// for with [`Scenario::stand_at`], with its reason.
    pub corrections: Vec<String>,
    /// Every message from the server except chunks, in order.
    pub heard: Vec<ServerMessage>,

    world_ready: bool,
    frames: u64,
    last_frame: Instant,
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
    /// How long the last frame really took, in seconds.
    ///
    /// **The horse is predicted by it and not by `FRAME`**, and that is the
    /// one place this harness steps by the wall. The body steps by `FRAME`
    /// because the anticheat only ever sees it go *slower* than it could; a
    /// horse predicted at a sixtieth a frame while a debug frame takes a
    /// twentieth is a horse at a third of the speed of the server's one,
    /// which the rider's reins are moving in real time -- and the
    /// prediction snapped back to it every second. `run` steps both by the
    /// frame's own length, which is what this is.
    frame_dt: f32,
    /// The latest snapshot of every entity the server has sent, by id.
    pub entities: HashMap<primitive_shared::protocol::EntityId, primitive_shared::protocol::EntityState>,
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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let (server, connection) = runtime.block_on(async {
            let server = primitive_server::start(settings, primitive_server::RunOptions::embedded())
                .await
                .expect("the server did not start");
            let connection = network::connect(&server.address().to_string(), "scenario")
                .await
                .expect("the server refused the scenario");
            (server, connection)
        });
        Self::connected(runtime, std::sync::Arc::new(server), connection, "scenario")
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
        Self::connected(runtime, server, connection, name)
    }

    fn connected(
        runtime: tokio::runtime::Runtime,
        server: std::sync::Arc<primitive_server::Server>,
        connection: network::Connection,
        name: &str,
    ) -> Self {
        let welcome = connection.welcome;
        let spawn = DVec3::new(welcome.spawn.0, welcome.spawn.1, welcome.spawn.2);
        let settings = crate::settings::ClientSettings::default();
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
            binds: settings.keybinds.clone(),
            mining: Mining::new(),
            debug: DebugStats::default(),
            audio: crate::Audio::silent(),
            soundscape: crate::Soundscape::new(),
            inventory: Inventory::new(),
            equipment: Default::default(),
            injuries: Default::default(),
            health: 1.0,
            dead: None,
            chest_screen: ChestScreen::new(),
            station_screen: StationScreen::new(),
            corrections: Vec::new(),
            heard: Vec::new(),
            world_ready: false,
            frames: 0,
            last_frame: now,
            last_sent_at: now,
            last_sent_transform: None,
            sequence: 0,
            expected_move: None,
            grace: None,
            jump_edge: false,
            horseback: None,
            frame_dt: FRAME,
            entities: HashMap::new(),
        };
        let ready = scenario.until(20.0, |s| s.world_ready && s.player.grounded);
        assert!(ready, "the world never arrived round the spawn");
        scenario
    }

    pub fn server(&self) -> &primitive_server::Server {
        self.server.as_ref().expect("server")
    }

    // ---- setting the stage ----

    /// Puts blocks into the world as the generator would have, and waits
    /// until this client has been told about every one of them.
    pub fn build(&mut self, cells: &[((i32, i32, i32), BlockId)]) {
        for &((x, y, z), block) in cells {
            self.server().place_block(x, y, z, block);
        }
        let all_there = self.until(5.0, |s| {
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
        self.expected_move = Some(at);
        self.server().teleport_named(&self.name, feet.0 as f32, feet.1 as f32, feet.2 as f32);
        let arrived = self.until(10.0, |s| s.expected_move.is_none() && s.world_ready);
        assert!(arrived, "the teleport to {feet:?} never came back");
        self.seconds(0.3);
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
        let key = self.binds.key(action).expect("an unbound action");
        self.input.set_key(key, true);
        if action == Action::Jump {
            self.jump_edge = true;
        }
    }

    pub fn release(&mut self, action: Action) {
        let key = self.binds.key(action).expect("an unbound action");
        self.input.set_key(key, false);
    }

    pub fn release_all(&mut self) {
        self.input.release_all();
    }

    pub fn key(&mut self, key: Key, down: bool) {
        self.input.set_key(key, down);
    }

    /// The right click, as the frame's `MouseButton::Right` arm makes it.
    ///
    /// **A copy of that arm's dispatch** for the gestures a scenario
    /// makes: `use_gesture` decides (the real function), and each claim
    /// sends what `run` sends for it. See the module note.
    pub fn use_aimed(&mut self) {
        let aimed = self.aimed();
        let held = self.inventory.block_in(self.input.hotbar_slot);
        let claim = crate::use_gesture(aimed.map(|(_, block)| block), held);
        let sprinting = self.input.action_down(&self.binds, Action::Sprint);
        let setting_down = held.is_some_and(primitive_shared::types::can_be_set_down)
            && !aimed.is_some_and(|(_, block)| primitive_shared::types::is_set_down(block))
            && claim != UseGesture::Hearth
            && sprinting;
        if setting_down {
            if let Some(cell) = crate::set_down_cell(&self.chunks, &self.camera) {
                self.net.send(ClientMessage::SetDown { global_x: cell.0, global_y: cell.1, global_z: cell.2 });
            }
            return;
        }
        match (claim, aimed) {
            (UseGesture::Station, Some((cell, _))) => {
                self.net.send(ClientMessage::OpenStation { global_x: cell.0, global_y: cell.1, global_z: cell.2 });
                self.station_screen.asked_to_open();
            }
            (UseGesture::Open, Some((cell, _))) => {
                self.net.send(ClientMessage::OpenChest { global_x: cell.0, global_y: cell.1, global_z: cell.2 });
                self.chest_screen.asked_to_open();
            }
            (
                UseGesture::Hearth
                | UseGesture::Water
                | UseGesture::Pick
                | UseGesture::Tend
                | UseGesture::Rest
                | UseGesture::Pit
                | UseGesture::Swing,
                Some((cell, _)),
            ) => {
                self.net.send(ClientMessage::UseBlock { global_x: cell.0, global_y: cell.1, global_z: cell.2 });
            }
            (UseGesture::Place, _) | (UseGesture::OpenVessel, _) => {
                let others: Vec<DVec3> = Vec::new();
                crate::try_place_block(
                    &self.chunks,
                    &self.camera,
                    &self.input,
                    &self.player,
                    &others,
                    &mut self.net,
                    &self.inventory,
                    &mut self.mining,
                    &mut self.debug,
                );
            }
            (other, _) => panic!("a scenario made a right click the harness does not play: {other:?}"),
        }
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

    /// One frame of the game, as `run` orders it: the socket, the chunks
    /// asked for, the body, the hands, what is sent back.
    pub fn frame(&mut self) {
        // Never shorter than a frame, and never catching up. See the
        // module note.
        let due = self.last_frame + Duration::from_secs_f32(FRAME);
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
        // The frame's own length, as `run` measures it, for the one thing
        // here that is stepped by the wall and not by `FRAME`: see
        // `frame_dt`.
        self.frame_dt = Instant::now().saturating_duration_since(self.last_frame).as_secs_f32().clamp(FRAME, 0.1);
        self.last_frame = Instant::now();
        let now = self.last_frame;

        self.drain();

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
            self.step_hands();
        }

        crate::maybe_send_transform(
            &mut self.net,
            &self.player,
            &self.camera,
            now,
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
        self.frames += 1;
    }

    fn step_body(&mut self) {
        let frozen = self.chest_screen.is_open() || self.station_screen.is_open();
        // **On a horse the keys ride**, exactly as `run` has them (the horse
        // block after the raft's there): the reins off the same wish, the
        // horse predicted, the reins sent, the body put on the saddle and not
        // stepped.
        if let Some(mut horseback) = self.horseback.take() {
            let wish = if frozen { Vec3::ZERO } else { crate::wish_direction(&self.input, &self.camera, &self.binds) };
            let forward = wish.dot(self.camera.forward_horizontal()) * self.input.stick_speed();
            let turn = wish.dot(self.camera.right_horizontal()) * self.input.stick_speed();
            let reins = crate::logic::horseback::Horseback::reins_from_keys(
                forward,
                turn,
                !frozen && self.input.action_down(&self.binds, Action::Sprint),
                !frozen && self.input.action_down(&self.binds, Action::Rein),
                !frozen && (self.jump_edge || self.input.action_pressed(&self.binds, Action::Jump)),
            );
            horseback.predict(reins, &|x, y, z| self.chunks.block_at(x, y, z), self.frame_dt);
            if let Some(message) = horseback.rein_message(Instant::now()) {
                self.net.send(message);
            }
            if !frozen
                && forward.abs() < 0.05
                && horseback.may_get_down()
                && self.input.action_pressed(&self.binds, Action::Rein)
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
        self.player.speed_scale = primitive_shared::load::speed_scale(carried)
            * self.equipment.worn().mobility()
            * self.injuries.speed_factor();
        self.player.snowshoes = self.equipment.snowshoes();
        self.player.buoyancy = primitive_shared::load::buoyancy(carried);
        self.player.treading = frozen;
        let sprinting = !frozen && self.input.action_down(&self.binds, Action::Sprint) && self.injuries.may_sprint();
        let wish = if frozen {
            Vec3::ZERO
        } else {
            crate::wish_direction(&self.input, &self.camera, &self.binds)
        };
        let jump_pressed = !frozen
            && primitive_shared::load::can_jump(carried)
            && (self.jump_edge || self.input.action_pressed(&self.binds, Action::Jump));
        let jump_held = !frozen && self.input.action_down(&self.binds, Action::Jump);
        self.player.update(&self.chunks, &[], wish, self.camera.forward(), jump_pressed, jump_held, sprinting, FRAME);
        self.camera.position = self.player.eye_position();
    }

    fn step_hands(&mut self) {
        let held_tool = self.inventory.block_in(self.input.hotbar_slot);
        let quality = self
            .inventory
            .slots()
            .get(self.input.hotbar_slot)
            .copied()
            .flatten()
            .map_or(primitive_shared::quality::Quality::PLAIN, |s| s.quality());
        let aim = crate::aimed_block_to_mine(&self.chunks, &self.camera)
            .filter(|(_, block)| primitive_shared::types::is_breakable_with(*block, held_tool));
        let dug = self.mining.update(aim, self.input.breaking, FRAME, held_tool, quality);
        if let Some(cell) = dug {
            // The frame's own choice between a slice and a break.
            let face = crate::aimed_face_to_mine(&self.chunks, &self.camera);
            let sliced = match (aim, face) {
                (Some((_, block)), Some(face)) => primitive_shared::dig::Side::from_normal((
                    i32::from(face.0),
                    i32::from(face.1),
                    i32::from(face.2),
                ))
                .and_then(|side| primitive_shared::dig::next_bite(block, side))
                .is_some(),
                _ => false,
            };
            if sliced {
                crate::request_dig(&self.chunks, cell, face.unwrap_or((0, 1, 0)), &mut self.net, &mut self.debug);
            } else {
                crate::request_break(&self.chunks, cell, &mut self.net, &mut self.debug);
            }
        }
    }

    /// The arms of `drain_network` a scenario reads. See the module note.
    fn drain(&mut self) {
        while let Ok(incoming) = self.net.to_game.try_recv() {
            let message = match incoming {
                network::Incoming::Chunk(chunk) => {
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
                ServerMessage::Injuries { injuries } => self.injuries = *injuries,
                ServerMessage::Health { current, max } => {
                    self.health = if *max > 0.0 { current / max } else { 0.0 };
                }
                ServerMessage::Died { cause } => self.dead = Some(format!("{cause:?}")),
                ServerMessage::Respawned { x, y, z } => {
                    self.dead = None;
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
        self.mining.confirm_placement(&change, Instant::now());
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

    /// Runs frames until `done` says so or `seconds` of them have passed,
    /// and says which.
    /// `until`, stepping this client and `other` a frame each in turn, so
    /// neither stops reading its socket while the other waits.
    pub fn until_both(&mut self, other: &mut Scenario, seconds: f32, mut done: impl FnMut(&Self, &Scenario) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed().as_secs_f32() < seconds {
            if done(self, other) {
                return true;
            }
            self.frame();
            other.frame();
        }
        done(self, other)
    }

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
