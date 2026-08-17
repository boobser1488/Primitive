//! Client entry point: window, event loop, and the per-frame pipeline of
//! input -> physics -> network -> mesh -> render.
//!
//! ## Shape of a session
//!
//! The window opens on the main menu, not on a world. From there:
//!
//! * **Singleplayer** starts the real server in this process, on
//!   loopback, and connects to it. There is no offline code path -- see
//!   `primitive_server`'s crate docs for why that is worth the loopback
//!   socket.
//! * **Multiplayer** opens the server list, which is editable in the
//!   game and persisted to `servers.toml`.
//!
//! Either way the result is a `network::Connection`, and everything past
//! that point is identical.
//!
//! ## Notes on the frame
//!
//! * **The server configures the client.** Spawn point, render distance
//!   cap, day length and time of day all come from `Welcome`; the client
//!   no longer guesses a spawn and free-falls.
//! * **Sky/fog are recomputed every frame** from a server-synced clock,
//!   and the fog colour doubles as the clear colour, which is what makes
//!   the edge of the loaded world dissolve into the horizon instead of
//!   ending in a visible wall.
//! * **Meshing is budgeted in milliseconds**, not in a fixed chunk count:
//!   an empty sky chunk and a cave system differ by an order of magnitude
//!   in cost, so "3 per frame" was either wasteful or a stutter depending
//!   on which you got.
//! * **Edits and arrivals re-mesh neighbours.** Now that the mesher reads
//!   across chunk borders for light and face culling, a change at the
//!   edge of one chunk affects the chunk next to it too.

// The four layers the client is built out of. Each is a directory with
// its own `mod.rs` explaining what belongs in it and what does not:
//
//   engine  -- the GPU, and everything that exists to feed it
//   net     -- the socket, and the state that arrives over it
//   ui      -- what the player reads, clicks and presses
//   logic   -- the world as the client understands it
//
// Everything below is what is left over: the entry point itself, the
// settings file, the crash handler, and the assets baked into the
// binary. They belong to no layer because every layer uses them.
mod engine;
mod logic;
mod net;
mod ui;

mod crash;
mod embedded;
mod settings;

// Every layer meets here, and only here -- so the modules are pulled in
// by name and the file below reads the way it did before the split. The
// block itself is the map: anything used unqualified in this file is on
// one of these four lines.
use engine::{fog, mesh, mesher, texture};
use logic::{entities, hand, mining, physics, shake, stamina, worlds};
use net::{network, remote_players};
use ui::{
    chat, chest_screen, death, hotbar, hud, input, inventory_screen, keybinds, menu, widgets,
};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, WindowBuilder};

use engine::camera::Camera;
use logic::chunk_manager::{ChunkManager, NEIGHBOUR_OFFSETS};
use ui::debug::{DebugStats, FrameInfo};
use ui::menu::{Action, Menu, Screen};
use primitive_shared::geometry::block_overlaps_player;
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{ClientMessage, PlayerId, ServerMessage};
use primitive_shared::types::{block_name, BlockId, ChunkPos, BLOCK_AIR};
use logic::inventory::Inventory;
use logic::physics::Player;

/// What the client assumes about health before the server has said
/// anything. Overwritten by the first `Health` message, which arrives
/// with the handshake -- these only cover the gap.
mod survival_defaults {
    pub const MAX: f32 = 20.0;
}
use net::remote_players::RemotePlayers;
use engine::renderer::{FrameParams, GraphicsState};
use settings::ClientSettings;
use engine::sky::Sky;

/// How often the things that genuinely *move* every frame are rebuilt.
///
/// The dropped items and the other players are regenerated from nothing
/// each frame: quads emitted, buffers uploaded. That was written when a
/// frame was sixteen milliseconds. At the frame rates this now runs at
/// it means rebuilding the same bobbing item a thousand times a second,
/// and measurement put it at a fifth of the frame -- the largest single
/// piece of CPU work left in one.
///
/// A hundred and twenty times a second is past what anyone can see (an
/// item's bob and spin included) and past most monitors. Below that rate
/// nothing changes at all: a frame slower than 8 ms rebuilds every time,
/// which is exactly the case where the old behaviour was right.
///
/// The interface used to be on this clock too. It is now rebuilt on
/// *change* -- see [`UiKey`] -- and the clock only survives there as the
/// pace for the few elements that animate on time alone (a fading chat
/// line, the death screen settling in).
const DYNAMIC_REBUILD_HZ: f32 = 120.0;

/// Everything the interface is drawn from, reduced to something cheap
/// to compare.
///
/// The interface used to be rebuilt on a timer -- laid out on the CPU
/// and uploaded to the GPU 120 times a second whether anything on it had
/// changed or not, and almost every frame nothing had. A full-screen
/// menu is tens of thousands of vertices (text here is one quad per lit
/// font pixel), so the timer was spending hundreds of kilobytes of
/// layout and upload per frame on producing the identical picture.
///
/// Now each frame reduces the build's *inputs* to this key and rebuilds
/// only when it differs from last frame's. Stateful widgets contribute
/// their own fingerprint (see the `ui_key` methods on chat, the menus
/// and the screens) rather than being enumerated here field by field --
/// they own their state, and a field added to one of them should not
/// need this struct to hear about it.
///
/// What is deliberately *not* in the key is anything that moves every
/// frame on time alone -- fade alphas, the death screen's opening --
/// because keying on those would rebuild every frame and be the old
/// behaviour with extra steps. Widgets report those phases as
/// "animating" instead, and the frame loop falls back to the
/// [`DYNAMIC_REBUILD_HZ`] clock for exactly as long as one is running.
/// The bias is deliberate: when a state is hard to capture, the widget
/// says "animating" and pays some rebuilds, because a stale interface
/// is a bug and a rebuilt one is only a cost.
#[derive(PartialEq, Default)]
struct UiKey {
    /// Which of the two frame paths built it: the menus outside a
    /// session, or the world's overlay. Comparing across the boundary
    /// must always fail, whatever the other fields happen to hold.
    in_game: bool,
    /// `f32::to_bits` of the window's aspect, which anchors chat, the
    /// debug panel and the menu wallpaper to the window's edges.
    aspect: u32,
    /// Behind the loading screen there is no interface at all.
    loading: bool,
    hotbar_slot: usize,
    /// Fingerprint of the whole pack: the hotbar icons, the stack
    /// counts and the inventory screen all draw from it.
    inventory: u64,
    health: u32,
    max_health: u32,
    /// The ghost strip on the health bar. It drains a little every
    /// frame after a hit, so for those moments its bits change -- and
    /// that is correct, because the bar genuinely looks different.
    recent_health: u32,
    stamina: u32,
    exhausted: bool,
    breath: u32,
    /// The server's last refusal: a fingerprint of the text, and
    /// whether it is still fully opaque. The fade itself stays out --
    /// while it runs, the notice reports as animating instead.
    notice: Option<(u64, bool)>,
    chat: u64,
    inventory_screen: u64,
    chest_screen: u64,
    death: u64,
    /// Only presence. The panel's numbers change every frame it is on
    /// screen, so while it is up the interface reports as animating
    /// rather than hashing a page of figures to learn what it already
    /// knows.
    debug_panel: bool,
    /// `Some` whenever a menu is on screen -- the pause screen in a
    /// session, every screen outside one.
    menu: Option<u64>,
    /// The interface language. The in-game screens draw their words in
    /// it, and it can change under them: settings are reachable from
    /// the pause menu while the inventory or the chest sits behind it.
    language: ui::lang::Language,
}

impl UiKey {
    /// The key for the out-of-session path, where the menu is the whole
    /// of the interface.
    fn menu_only(menu: u64, aspect: f32) -> Self {
        Self {
            in_game: false,
            aspect: aspect.to_bits(),
            menu: Some(menu),
            ..Self::default()
        }
    }
}

/// Fingerprint of what the player is carrying, for [`UiKey`].
///
/// The type lives in `primitive_shared` and does not hash itself, so
/// the reduction happens here: forty small slots, a few dozen
/// nanoseconds, cheap enough to take every frame.
fn inventory_fingerprint(inventory: &Inventory) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for slot in inventory.slots() {
        slot.map(|stack| (stack.block, stack.count)).hash(&mut h);
    }
    h.finish()
}

/// Fingerprint of a piece of text, for [`UiKey`].
fn text_fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// How far the player can reach to break or place.
///
/// Four blocks rather than six. Six is far enough to build a floor out
/// from under yourself without moving, and to dig a shaft while standing
/// well clear of it; four makes you stand where you are working, which
/// is most of what makes placing a block a decision about where you are.
///
/// The anti-cheat allows a little more than this (see
/// `primitive_server::settings::AntiCheatSettings::max_reach`), because
/// it is bounding what a *cheat* can do rather than what an honest
/// client does, and a client's idea of where it stands is always a
/// little behind the server's.
const INTERACT_RANGE: f32 = 4.0;
/// Chunk requests are batched into one message per scan; this caps a
/// single batch so a huge render distance can't produce an oversized
/// frame the server would reject.
const MAX_REQUEST_BATCH: usize = 512;
/// Shown on the main menu and in the window title.
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// A connection plus, for singleplayer, the server it is connected to.
struct Session {
    connection: network::Connection,
    /// The in-process server, if this is a singleplayer world. Owned by
    /// the client so that leaving the world stops it and saves it.
    local_server: Option<primitive_server::Server>,
}

/// Deliberately not `#[tokio::main]`.
///
/// The winit event loop takes over the main thread and has to be able to
/// block on tokio work at two points -- starting a local server, and
/// stopping one so the world is saved before the process exits. Both are
/// `Runtime::block_on`, which panics if it is called from inside a
/// runtime, which is exactly what `#[tokio::main]` would put us in.
fn main() -> std::process::ExitCode {
    crash::install_panic_handler();

    match start() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            // Not `?` out of `main`: that prints a Debug-formatted error
            // to a console the player does not have open, and the window
            // simply never appears. Everything fatal is written to
            // `crash.log` instead, which is something a bug report can
            // contain.
            crash::report_fatal("could not start", &e);
            std::process::ExitCode::FAILURE
        }
    }
}

fn start() -> anyhow::Result<()> {
    let settings = ClientSettings::load_or_default();
    let servers = menu::ServerList::load_or_default(&settings.server_addr);
    let worlds = worlds::Worlds::load(&settings.singleplayer_world_dir);
    println!("{} singleplayer world(s)", worlds.list().len());
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| anyhow::anyhow!("could not start the async runtime: {e}"))?;
    run(settings, servers, worlds, runtime)
}

/// Sync entry point for the winit event loop. Runs on the main thread;
/// the network reader/writer tasks spawned inside `network::connect` keep
/// running concurrently on the tokio runtime's worker threads.
fn run(
    settings: ClientSettings,
    servers: menu::ServerList,
    worlds: worlds::Worlds,
    runtime: tokio::runtime::Runtime,
) -> anyhow::Result<()> {
    // No connection yet: the player is on the main menu. Everything
    // session-specific below starts empty and is replaced wholesale when
    // a server answers.
    // Both are edited from the menus, so neither can be a plain `let`
    // any more. `settings_dirty` tracks whether the file needs
    // rewriting, so leaving the settings screen without touching
    // anything doesn't rewrite it.
    let mut settings = settings;
    let mut worlds = worlds;
    let mut settings_dirty = false;

    let mut net: Option<network::NetworkHandle> = None;
    let mut local_server: Option<primitive_server::Server> = None;
    let mut menu = Menu::new(servers);
    let mut pending_connect: Option<tokio::sync::oneshot::Receiver<Result<Session, String>>> = None;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let assets_dir = texture::resolve_assets_dir(&settings.assets_dir);

    // Built before the window so the icon is there from the first frame
    // rather than appearing a moment later.
    let icon = texture::load_window_icon(&assets_dir).and_then(|(pixels, w, h)| {
        winit::window::Icon::from_rgba(pixels, w, h)
            .map_err(|e| eprintln!("window icon rejected: {e}"))
            .ok()
    });

    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Primitive")
            .with_window_icon(icon)
            .with_inner_size(winit::dpi::LogicalSize::new(
                settings.window_width as f64,
                settings.window_height as f64,
            ))
            // Straight into fullscreen if that is how the game was left,
            // rather than a windowed flash first: the size above is what
            // it returns to when fullscreen is turned off.
            .with_fullscreen(
                settings
                    .fullscreen
                    .then_some(winit::window::Fullscreen::Borderless(None)),
            )
            .build(&event_loop)?,
    );

    println!("assets dir: {}", assets_dir.display());
    // The likeliest failure a player will ever hit, and the one worth
    // naming: no usable GPU, or drivers too old for the backend.
    let mut graphics = pollster::block_on(GraphicsState::new(
        window.clone(),
        &assets_dir,
        settings.vsync,
        settings.anisotropy,
    ))
    .map_err(|e| anyhow::anyhow!("graphics could not start: {e}"))?;
    // Not a constructor argument: the sky target has to be built from
    // the surface size, which the state only knows about itself.
    graphics.set_sky_scale(settings.sky_scale);
    println!("loaded {} block texture(s)", graphics.textures.layer_count);

    // Session state. These are replaced wholesale when a connection is
    // established; the values here just let the browser screen run
    // before there is a session at all.
    let mut render_distance = settings.render_distance_chunks;
    // What the server said it would stream. Until one says otherwise,
    // the player's own setting is the only limit.
    let mut server_view_distance = i32::MAX;

    let mut chunks = ChunkManager::new(render_distance);
    // World-space light, computed once per chunk and updated
    // incrementally afterwards (see primitive_shared::lighting).
    let mut light = LightMap::new();
    // Meshing runs on worker threads; this owns them and the pooled
    // buffers that travel back and forth.
    let mut mesher = mesher::Mesher::new(
        graphics.textures.face_layers(),
        settings.worker_threads,
    );
    // Positions whose mesh needs (re)building, deduplicated: several edits
    // in one chunk in the same frame should cost one re-mesh, not one per
    // edit.
    // Two queues. `urgent` holds chunks the player just changed -- those
    // must be rebuilt now, not behind a hundred chunks of terrain
    // streaming, or breaking a block leaves a visible hole until the
    // queue drains.
    let mut urgent: VecDeque<ChunkPos> = VecDeque::new();
    let mut dirty: VecDeque<ChunkPos> = VecDeque::new();
    let mut dirty_set: MeshQueueSet = MeshQueueSet::new();
    // Per-chunk edit counter.
    //
    // Meshing happens on several threads, so two jobs for the same chunk
    // can be in flight at once and finish in either order. Without this,
    // a mesh built *before* a block was broken could land *after* the
    // one built from the edited chunk and overwrite it -- the block
    // reappears, or its neighbours' newly exposed faces are missing and
    // you see a hole. The counter lets a stale result be dropped and the
    // chunk re-queued.
    let mut chunk_versions: HashMap<ChunkPos, u64> = HashMap::new();
    // Chunks that have arrived but aren't integrated yet. Receiving is
    // cheap; integrating (lighting a chunk) is not, so the two are
    // separated and only integration is budgeted -- see `integrate_chunks`.
    let mut arrivals = Arrivals::default();

    let mut remote_players = RemotePlayers::default();
    let mut entities = entities::Entities::default();
    let face_layers = graphics.textures.face_layers();
    let mut my_id: PlayerId = 0;
    let mut world_seed: u32 = 0;
    // The same generator the server runs, for the things the client can
    // work out for itself rather than being told: the biome readout, and
    // the climate that colours grass and leaves. A message per column
    // would be a lot of protocol for something both sides can derive
    // from the seed they already share.
    let mut worldgen = primitive_shared::worldgen::WorldGen::new(world_seed);

    let mut sky = Sky::new(0.3, 900.0);

    let mut player = Player::new(Vec3::new(0.5, 40.0, 0.5), settings.move_speed);
    let mut camera = Camera::new(player.eye_position(), graphics.aspect());
    camera.fov_y_radians = settings.fov_degrees.to_radians();

    // Rebuilt every frame because they move every frame; the storage
    // for them is not.
    let mut entity_mesh = graphics.new_dynamic_mesh();
    let mut actor_mesh = graphics.new_dynamic_mesh();
    // The cracks on the block being mined. Its own mesh because it is
    // textured and blended, where the outline around the same block is
    // flat geometry on the actor pipeline.
    let mut break_mesh = graphics.new_dynamic_mesh();
    let mut break_vertices: Vec<mesh::Vertex> = Vec::new();
    let mut break_indices: Vec<u32> = Vec::new();
    let mut entity_vertices: Vec<mesh::Vertex> = Vec::new();
    let mut entity_indices: Vec<u32> = Vec::new();
    // Dropped items are sprites with a thickness rather than cubes, so
    // they have their own vertex format and their own buffer. See
    // `engine::item_model`.
    let mut item_mesh = graphics.new_dynamic_mesh();
    let mut item_vertices: Vec<engine::item_model::ItemVertex> = Vec::new();
    let mut item_indices: Vec<u32> = Vec::new();
    // The player's own arm. Its vertices are in view space rather than
    // in the world -- see `logic::hand` -- which is why it is a buffer of
    // its own rather than more geometry in the item one.
    let mut hand = hand::Hand::new();
    let mut hand_mesh = graphics.new_dynamic_mesh();
    let mut hand_vertices: Vec<hand::HandVertex> = Vec::new();
    let mut hand_indices: Vec<u32> = Vec::new();
    let mut actor_vertices: Vec<remote_players::ActorVertex> = Vec::new();
    let mut actor_indices: Vec<u32> = Vec::new();
    // Where the other players are, for the collision pass. See the
    // note where it is filled.
    let mut other_positions: Vec<Vec3> = Vec::new();
    // When the moving geometry was last rebuilt. See
    // `DYNAMIC_REBUILD_HZ`.
    let mut last_rebuild: Option<Instant> = None;
    // The interface's vertices, and the inputs they were built from.
    // Both persist across frames: the vertices so an unchanged frame
    // re-uses them (and their allocation), the key so "unchanged" is
    // something a frame can actually establish. See `UiKey`.
    let mut ui_vertices: Vec<hotbar::HotbarVertex> = Vec::new();
    let mut ui_key: Option<UiKey> = None;
    // Whether the constant menu title is already on the window, so the
    // menu path doesn't pay a format! and a window-manager call per
    // frame for a string that never changes. Reset when a session
    // starts, because the in-game path overwrites the title.
    let mut menu_title_set = false;

    let mut input = input::InputState::default();
    // Survival state. All of it is the server's to decide; the client
    // only draws what it is told, and resets to full on a fresh session
    // so a previous world's health never shows in a new one.
    let mut inventory = Inventory::new();
    let mut mining = mining::Mining::new();
    let mut health = survival_defaults::MAX;
    let mut max_health = survival_defaults::MAX;
    // Lags `health` downward so a hit leaves a draining strip on the
    // bar. Purely a display value.
    let mut recent_health = survival_defaults::MAX;
    // When the last punch was thrown, so holding the button is a rhythm
    // rather than a packet a frame. The server has its own copy of the
    // same rule -- see `primitive_shared::combat`.
    let mut last_swing: Option<Instant> = None;
    // What is in the chest the player has open, if any. The screen owns
    // "is a chest open" as well as its contents -- see `ui::chest_screen`.
    let mut chest_screen = chest_screen::ChestScreen::new();
    // Dying, and the screen it puts up. The screen owns the fact of
    // being dead as well as the drawing of it -- see `ui::death`.
    let mut death = death::DeathScreen::new();
    // What the cursor was doing before, so the grab is only changed on
    // the frame the answer changes.
    let mut was_dead = false;
    // The same, for the chest screen -- which is opened by the server's
    // answer rather than by a keypress, so nothing else can do it.
    let mut chest_was_open = false;
    // The last thing the server refused, and when. Errors used to go to
    // stderr only, so on a released build the game silently did nothing
    // and never said why.
    let mut notice: Option<(String, Instant)> = None;
    let mut shake = shake::Shake::new();
    let mut stamina = stamina::Stamina::new();
    // Air, as the server last reported it. One while anybody's head is
    // above water, which is almost always.
    let mut breath: f32 = 1.0;
    let mut inventory_screen = inventory_screen::InventoryScreen::new();
    // What people have said, and the line being typed. Opened with
    // Enter; see the `chat` module.
    let mut chat = chat::Chat::new();
    // The last hotbar slot the server was told about. Resent only on a
    // change: the server needs it to know what a placement spends.
    let mut reported_slot = usize::MAX;
    let mut last_frame = Instant::now();
    // When the window title was last rewritten. See `TITLE_INTERVAL`.
    let mut last_title_update = Instant::now() - Duration::from_secs(1);
    let mut debug_stats = DebugStats::default();
    debug_stats.console_enabled = settings.debug_overlay_on_start;

    let mut fog_enabled = settings.fog_enabled;
    let mut sequence: u32 = 0;
    // Physics stays frozen until the ground under the player exists.
    // Without this the player spawns into empty space, falls through the
    // world while the first chunks are still in flight, and the server's
    // anti-cheat sees a 60 m/s descent.
    let mut world_ready = false;

    let player_update_interval = Duration::from_secs_f32(1.0 / settings.player_update_hz.max(1.0));
    let mut last_player_update_sent = Instant::now() - player_update_interval;
    let mut last_sent_transform: Option<(Vec3, f32, f32)> = None;
    // True while the pause screen is up. The world keeps rendering and
    // the network keeps draining -- pausing a client of an authoritative
    // server does not pause the world, and pretending otherwise would
    // just mean a backlog to catch up on.
    let mut paused = false;
    let mut last_menu_frame = Instant::now();
    // What "retry" should retry. Without it the retry button on a failed
    // singleplayer start would try to reconnect to whatever server
    // happened to be selected in the list.
    let mut last_attempt = Attempt::None;

    // **`PRIMITIVE_BENCH=<seconds>` quits once it has run that long.**
    //
    // The other half of `PRIMITIVE_AUTOSTART` below, and useless without
    // it: that one opens a world and turns the dump on, this one closes
    // the client again so a measurement can be taken by a script rather
    // than by a person deciding when to stop.
    //
    // The pair is what makes "is this change faster" answerable without
    // anybody looking at anything: the world save holds the player's
    // position *and* their view direction, so a run that opens it and
    // touches nothing starts from exactly where the last one did. The
    // viewpoint is fixed by not being controlled.
    let bench_seconds: Option<f32> = std::env::var("PRIMITIVE_BENCH")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|seconds| *seconds > 0.0);
    let mut bench_started: Option<Instant> = None;

    // Set when the server ends the session under us -- a kick, a
    // shutdown, a dropped connection. Handled after the frame's
    // borrows have ended.
    let mut end_session: Option<String> = None;

    // Dev affordance: `PRIMITIVE_AUTOSTART=<world name>` opens straight
    // into a singleplayer world, and turns the F3 dump on so frame
    // timings land in the terminal.
    //
    // It exists because the alternative way to measure the frame rate is
    // to drive the menu by hand every time, which makes "is this change
    // faster or slower" a question nobody bothers to answer.
    if let Ok(wanted) = std::env::var("PRIMITIVE_AUTOSTART") {
        let index = worlds
            .list()
            .iter()
            .position(|w| w.name.eq_ignore_ascii_case(wanted.trim()));
        match index.and_then(|i| worlds.get(i).cloned().map(|w| (i, w))) {
            Some((i, world)) => {
                println!("[autostart] opening \"{}\"", world.name);
                last_attempt = Attempt::Singleplayer(i);
                worlds.mark_played(i);
                pending_connect = Some(spawn_singleplayer(&runtime, &settings, world));
                // Set rather than toggled. It was a toggle, which is
                // the same thing only when the dump starts off -- and
                // `debug_overlay_on_start` is a setting, so it does not
                // always. Autostarting with that setting already on
                // turned the dump *off* and the run produced nothing,
                // which is a silent failure in the one tool whose whole
                // job is to produce a number.
                debug_stats.console_enabled = true;
            }
            None => eprintln!("[autostart] no world called \"{wanted}\""),
        }
    }

    println!(
        "controls: WASD move | Shift sprint | Space jump | mouse look (click to grab) | \
         I inventory | Esc pause | hold LMB to mine | RMB place | 1-9 or wheel pick slot | \
         R respawn | F fog | F3 stats"
    );

    event_loop.run(move |event, elwt| {
        // Everything that has to happen exactly once on the way out,
        // whichever way the player leaves: a clean disconnect, and a
        // saved singleplayer world. `Runtime::block_on` is safe here
        // because this thread is not inside the runtime -- see `main`.
        macro_rules! quit {
            () => {{
                if let Some(net) = net.as_ref() {
                    net.send(ClientMessage::Disconnect);
                }
                if let Some(server) = local_server.take() {
                    println!("saving the world...");
                    runtime.block_on(server.stop());
                }
                elwt.exit();
                return;
            }};
        }

        /// Cancels an in-flight connection attempt.
        ///
        /// Dropping the receiver is not enough: the task behind it may
        /// still be *starting a server*, and a server nobody holds a
        /// handle to keeps running and never saves. So the receiver is
        /// handed to a task that waits for whatever comes out and shuts
        /// it down properly.
        macro_rules! abandon_pending {
            () => {
                if let Some(receiver) = pending_connect.take() {
                    runtime.spawn(async move {
                        if let Ok(Ok(session)) = receiver.await {
                            if let Some(server) = session.local_server {
                                server.stop().await;
                            }
                        }
                    });
                }
            };
        }

        macro_rules! handle_action {
            ($action:expr) => {{
                // "Retry" is not an action of its own -- it is whichever
                // attempt failed. Resolved here, before the match, so it
                // doesn't have to re-enter this macro.
                let action = match $action {
                    Action::Retry => match last_attempt {
                        Attempt::Singleplayer(index) => Action::PlayWorld(index),
                        Attempt::Server(index) => Action::Connect(index),
                        Attempt::None => Action::Back,
                    },
                    other => other,
                };
                match action {
                    Action::PlayWorld(index) => {
                        if pending_connect.is_none() {
                            if let Some(world) = worlds.get(index).cloned() {
                                menu.begin_connecting(world.name.clone());
                                last_attempt = Attempt::Singleplayer(index);
                                worlds.mark_played(index);
                                pending_connect =
                                    Some(spawn_singleplayer(&runtime, &settings, world));
                            }
                        }
                    }

                    Action::CreateWorld => {
                        let name = menu.name_input.trim().to_string();
                        let seed = if menu.seed_input.is_empty() {
                            settings.singleplayer_seed
                        } else {
                            menu.seed_input.parse::<u32>().unwrap_or(settings.singleplayer_seed)
                        };
                        match worlds.create(&name, seed) {
                            Ok(index) => {
                                menu.world_selected = index;
                                menu.screen = Screen::Worlds;
                                menu.notice = Some((format!("created {name}").into(), true));
                            }
                            Err(reason) => {
                                // Stay on the form: the player has just
                                // typed something and needs to see why
                                // it was refused, next to what they
                                // typed.
                                menu.notice = Some((reason.into(), false));
                            }
                        }
                    }

                    Action::AskDeleteWorld(index) => {
                        // The question names the world, so it can't be
                        // read as being about a different row.
                        if let Some(world) = worlds.get(index) {
                            menu.set_confirm_detail(world.name.clone());
                        }
                    }

                    Action::ConfirmedDeleteWorld(index) => match worlds.delete(index) {
                        Ok(name) => menu.notice = Some((format!("deleted {name}").into(), true)),
                        Err(reason) => menu.notice = Some((reason.into(), false)),
                    },

                    Action::EditUsername => menu.begin_username_edit(settings.username.clone()),

                    Action::CommitUsername => {
                        let typed = menu.name_input.trim();
                        if !typed.is_empty() {
                            settings.username = typed.to_string();
                            settings.sanitize();
                            settings_dirty = true;
                        }
                    }

                    Action::Tweak(setting, delta) => {
                        setting.step(&mut settings, delta);
                        settings_dirty = true;
                        apply_settings(
                            &settings,
                            &mut graphics,
                            &mut camera,
                            &mut chunks,
                            &mut render_distance,
                            server_view_distance,
                            &mut fog_enabled,
                        );
                    }
                    Action::ResetKeys => {
                        settings.keybinds.reset();
                        settings_dirty = true;
                    }
                    // Handled entirely inside the menu: it only sets the
                    // "listening" state, and the keypress that follows is
                    // caught above.
                    Action::OpenControls | Action::RebindKey(_) => {}
                    Action::Connect(index) => {
                        if pending_connect.is_none() {
                            if let Some(entry) = menu.servers.servers.get(index).cloned() {
                                menu.begin_connecting(entry.name.clone());
                                last_attempt = Attempt::Server(index);
                                pending_connect = Some(spawn_connect(
                                    &runtime,
                                    entry.address,
                                    settings.username.clone(),
                                ));
                            }
                        }
                    }
                    Action::Cancel => abandon_pending!(),
                    Action::Back => {
                        // Leaving the settings screen is the save. There
                        // is no separate button to forget to press.
                        if settings_dirty {
                            settings_dirty = false;
                            match settings.save() {
                                Ok(()) => println!("settings saved"),
                                Err(e) => {
                                    eprintln!("{e}");
                                    menu.notice = Some((e.into(), false));
                                }
                            }
                        }
                        abandon_pending!()
                    }
                    Action::Resume => {
                        paused = false;
                        // Not while the death screen is up: it has its
                        // own buttons, and they need the pointer.
                        if !death.is_open() {
                            grab_cursor(&window, &mut input);
                        }
                    }
                    Action::LeaveWorld => {
                        if let Some(handle) = net.take() {
                            handle.send(ClientMessage::Disconnect);
                        }
                        if let Some(server) = local_server.take() {
                            println!("saving the world...");
                            runtime.block_on(server.stop());
                        }
                        paused = false;
                        release_cursor(&window, &mut input);
                        menu.open(Screen::Main);
                    }
                    Action::Quit => quit!(),
                    // Everything else is pure menu navigation, already
                    // carried out by `Menu::apply`.
                    _ => {}
                }
            }};
        }

        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => quit!(),

                WindowEvent::Resized(new_size) => {
                    graphics.resize(new_size);
                    camera.aspect = graphics.aspect();
                }

                WindowEvent::CursorMoved { position, .. } => {
                    // Only meaningful when something is up that wants a
                    // pointer: during play the cursor is grabbed and
                    // motion arrives as a `DeviceEvent` instead.
                    let size = graphics.size;
                    let at = widgets::cursor_to_ui((position.x, position.y), (size.width, size.height));
                    if net.is_none() || paused {
                        menu.set_cursor(Some(at));
                    } else if death.is_open() {
                        death.set_cursor(Some(at));
                    } else if chest_screen.is_open() {
                        chest_screen.set_cursor(Some(at));
                    } else if inventory_screen.open {
                        inventory_screen.set_cursor(Some(at));
                    }
                }

                WindowEvent::CursorLeft { .. } => menu.set_cursor(None),

                WindowEvent::KeyboardInput { event, .. } => {
                    let is_pressed = event.state == ElementState::Pressed;
                    let in_menu = net.is_none() || paused;

                    if in_menu {
                        if !is_pressed {
                            return;
                        }
                        // Rebinding swallows the next key whatever it
                        // is, so it has to come before every other
                        // reading of the keyboard -- otherwise binding
                        // an action to Escape or to a menu shortcut
                        // would navigate instead of binding.
                        if let Some(action) = menu.awaiting_key() {
                            if let PhysicalKey::Code(code) = event.physical_key {
                                if code == KeyCode::Escape {
                                    // Escape cancels rather than binds:
                                    // it is the way out of every screen,
                                    // and an action bound to it would
                                    // have no way back.
                                    menu.finish_rebind(false);
                                } else if keybinds::is_bindable(code) {
                                    settings.keybinds.bind(action, code);
                                    settings_dirty = true;
                                    menu.finish_rebind(true);
                                } else {
                                    menu.finish_rebind(false);
                                }
                            }
                            return;
                        }
                        // Text first: a character typed into a field must
                        // not also be read as a shortcut.
                        if menu.accepts_text() {
                            if let Some(text) = event.text.as_ref() {
                                let mut typed = false;
                                for c in text.chars() {
                                    if crate::engine::texture::has_glyph(c) {
                                        menu.type_char(c);
                                        typed = true;
                                    }
                                }
                                if typed {
                                    return;
                                }
                            }
                        }
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if let Some(key) = menu_key(code, event.text.as_deref()) {
                                if let Some(action) = menu.key(key) {
                                    handle_action! { action }
                                }
                            }
                        }
                        return;
                    }

                    // The chat box owns the keyboard while it is open:
                    // every letter is text, not a shortcut, or walking
                    // keys would move the player as they type.
                    if chat.is_typing() {
                        if !is_pressed {
                            return;
                        }
                        if let Some(text) = event.text.as_ref() {
                            for c in text.chars() {
                                chat.type_char(c);
                            }
                        }
                        if let PhysicalKey::Code(code) = event.physical_key {
                            match code {
                                KeyCode::Enter | KeyCode::NumpadEnter if !event.repeat => {
                                    if let (Some(line), Some(net)) =
                                        (chat.submit(), net.as_ref())
                                    {
                                        net.send(ClientMessage::Chat(line));
                                        debug_stats.network_messages_out_this_second += 1;
                                    }
                                    close_chat(&mut chat, &window, &mut input, paused);
                                }
                                KeyCode::Escape => {
                                    close_chat(&mut chat, &window, &mut input, paused);
                                }
                                KeyCode::Backspace => chat.backspace(),
                                _ => {}
                            }
                        }
                        return;
                    }

                    if let PhysicalKey::Code(code) = event.physical_key {
                        if is_pressed {
                            // Escape is deliberately not rebindable: it
                            // is the way out of every screen, including
                            // the one where keys are rebound, and a
                            // player who bound it away would have no way
                            // back.
                            let binds = &settings.keybinds;
                            let action = keybinds::Action::ALL
                                .into_iter()
                                .find(|a| binds.key(*a) == Some(code));
                            match (code, action) {
                                // Enter opens the chat box. Not
                                // rebindable, for the same reason
                                // Escape is not: it is the way out of
                                // what it opens.
                                //
                                // Not while the inventory has the
                                // screen: two things claiming the cursor
                                // and the keyboard at once ends with the
                                // inventory unusable behind a grabbed
                                // pointer. And not on a key repeat --
                                // holding Enter would open and close the
                                // box tens of times a second.
                                (KeyCode::Enter | KeyCode::NumpadEnter, _)
                                    if net.is_some()
                                        && !paused
                                        && !inventory_screen.open
                                        && !chest_screen.is_open()
                                        && !death.is_open()
                                        && !event.repeat =>
                                {
                                    chat.open(Instant::now());
                                    release_cursor(&window, &mut input);
                                    input.release_all();
                                }
                                // The chest closes on the same two
                                // keys the inventory does, and tells the
                                // server -- which stops sending updates
                                // for it and stops accepting gestures
                                // against it.
                                (KeyCode::Escape, _) | (_, Some(keybinds::Action::Inventory))
                                    if chest_screen.is_open() =>
                                {
                                    close_chest(
                                        &mut chest_screen,
                                        net.as_ref(),
                                        &mut debug_stats,
                                    );
                                }
                                (KeyCode::Escape, _) if inventory_screen.open => {
                                    // Esc backs out of the inventory
                                    // before it reaches for the pause
                                    // menu: one screen at a time.
                                    inventory_screen.close();
                                    grab_cursor(&window, &mut input);
                                }
                                (KeyCode::Escape, _) => {
                                    paused = true;
                                    menu.open(Screen::Paused);
                                    release_cursor(&window, &mut input);
                                    input.release_all();
                                }
                                (_, Some(keybinds::Action::ToggleStats)) => {
                                    debug_stats.toggle_console()
                                }
                                (_, Some(keybinds::Action::Respawn))
                                    if death.is_open() =>
                                {
                                    if let Some(net) = net.as_ref() {
                                        net.send(ClientMessage::Respawn);
                                    }
                                }
                                // The death screen's buttons, from the
                                // keyboard. Every other screen in the
                                // game can be driven without the mouse,
                                // and the one that arrives uninvited is
                                // the worst one to make an exception of.
                                (KeyCode::ArrowUp, _) if death.is_open() => {
                                    death.move_focus(-1)
                                }
                                (KeyCode::ArrowDown, _) if death.is_open() => {
                                    death.move_focus(1)
                                }
                                (KeyCode::Enter | KeyCode::NumpadEnter, _)
                                    if death.is_open() =>
                                {
                                    match death.focused() {
                                        Some(death::Choice::Respawn) => {
                                            if let Some(net) = net.as_ref() {
                                                net.send(ClientMessage::Respawn);
                                            }
                                        }
                                        Some(death::Choice::LeaveWorld) => {
                                            handle_action! { Action::LeaveWorld }
                                        }
                                        None => {}
                                    }
                                }
                                (_, Some(keybinds::Action::Inventory)) => {
                                    if inventory_screen.open {
                                        inventory_screen.close();
                                        grab_cursor(&window, &mut input);
                                    } else {
                                        // One screen at a time: the chat
                                        // box and the inventory both
                                        // want the cursor and the keys.
                                        chat.close();
                                        release_cursor(&window, &mut input);
                                        input.release_all();
                                        // Seeded with the middle of the
                                        // screen: with no starting
                                        // position the first click does
                                        // nothing, which reads as the
                                        // inventory ignoring the mouse.
                                        inventory_screen.open_at(Some((0.0, 0.0)));
                                    }
                                }
                                (_, Some(keybinds::Action::Drop)) => {
                                    // The hovered slot while a screen
                                    // is open, the selected one
                                    // otherwise. Sprint modifier for the
                                    // whole stack.
                                    let slot = if inventory_screen.open {
                                        inventory_screen.hovered_slot()
                                    } else if chest_screen.is_open() {
                                        // Only out of the pack: there is
                                        // no message for throwing
                                        // something out of a chest, and
                                        // a chest is somewhere you put
                                        // things rather than a bin.
                                        chest_screen.hovered().and_then(|(side, slot)| {
                                            (side == primitive_shared::protocol::Side::Pack)
                                                .then_some(slot)
                                        })
                                    } else {
                                        Some(input.hotbar_slot)
                                    };
                                    if let (Some(slot), Some(net)) = (slot, net.as_ref()) {
                                        net.send(ClientMessage::DropSlot {
                                            slot: slot as u8,
                                            whole_stack: input.action_down(
                                                binds,
                                                keybinds::Action::Sprint,
                                            ),
                                        });
                                        debug_stats.network_messages_out_this_second += 1;
                                    }
                                }
                                // A number key over a slot sends what is
                                // in it to that place on the bar. The
                                // gesture everyone brings with them from
                                // other games, and the fastest way to
                                // lay a bar out: point, press, done.
                                (key, _)
                                    if inventory_screen.open
                                        && input::hotbar_slot_for(key).is_some() =>
                                {
                                    let from = inventory_screen.hovered_slot();
                                    let to = input::hotbar_slot_for(key);
                                    if let (Some(from), Some(to), Some(net)) =
                                        (from, to, net.as_ref())
                                    {
                                        if from != to {
                                            net.send(ClientMessage::MoveSlots {
                                                from: from as u8,
                                                to: to as u8,
                                            });
                                            debug_stats.network_messages_out_this_second += 1;
                                        }
                                    }
                                }
                                (_, Some(keybinds::Action::ToggleFullscreen)) => {
                                    // Borderless: a window the size of
                                    // the screen with no frame. Toggled
                                    // here and *remembered*, because a
                                    // player who plays fullscreen plays
                                    // fullscreen tomorrow as well.
                                    settings.fullscreen = !settings.fullscreen;
                                    window.set_fullscreen(
                                        settings
                                            .fullscreen
                                            .then_some(winit::window::Fullscreen::Borderless(None)),
                                    );
                                    settings_dirty = true;
                                }
                                (_, Some(keybinds::Action::ToggleFog)) => {
                                    // Change the setting, not a separate
                                    // flag. They used to be two truths:
                                    // pressing F turned fog off, and the
                                    // next tweak of any setting at all
                                    // silently turned it back on.
                                    settings.fog_enabled = !settings.fog_enabled;
                                    fog_enabled = settings.fog_enabled;
                                    settings_dirty = true;
                                }
                                _ => input.set_key(code, true),
                            }
                        } else {
                            input.set_key(code, false);
                        }
                    }
                }

                WindowEvent::MouseWheel { delta, .. } => {
                    let forward = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                        MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                    };
                    // On a menu the wheel belongs to whatever list is on
                    // screen. It used to belong to nothing at all: the
                    // handler returned before looking, so the only way
                    // down a list longer than its panel was the arrow
                    // keys, and the world list gave no sign there was
                    // anything below the last row it had drawn.
                    if net.is_none() || paused {
                        menu.scroll(if forward { 1 } else { -1 });
                        return;
                    }
                    // With the inventory open the wheel belongs to the
                    // recipe list, which is longer than the window on
                    // it. It must *not* reach the hotbar there: the
                    // bar's selection is not on screen, so scrolling
                    // moved it invisibly and the player found out later,
                    // having placed the wrong block.
                    if inventory_screen.open {
                        inventory_screen.scroll_recipes(if forward { 1 } else { -1 });
                        return;
                    }
                    input.cycle_hotbar(forward);
                }

                WindowEvent::MouseInput { state, button, .. } => {
                    let pressed = state == ElementState::Pressed;
                    // Tracked whatever the game is doing, so releasing
                    // the button over a menu doesn't leave the world
                    // thinking it is still held.
                    if button == MouseButton::Left {
                        input.breaking = pressed
                            && input.mouse_grabbed
                            && !paused
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !chat.is_typing();
                    }
                    // Clicking while typing is a misclick, not a swing:
                    // the cursor is loose because the chat box has it.
                    if chat.is_typing() {
                        return;
                    }
                    if !pressed {
                        return;
                    }
                    // The death screen takes the click before anything
                    // else does. A dead player has nothing else to
                    // click on, and the world behind is not theirs to
                    // touch until they are back in it.
                    if death.is_open() && net.is_some() && !paused {
                        if button == MouseButton::Left {
                            match death.click() {
                                Some(death::Choice::Respawn) => {
                                    if let Some(net) = net.as_ref() {
                                        net.send(ClientMessage::Respawn);
                                    }
                                }
                                // Straight through the pause menu's own
                                // path, so leaving from here saves and
                                // tears down exactly as leaving from
                                // there does.
                                Some(death::Choice::LeaveWorld) => {
                                    handle_action! { Action::LeaveWorld }
                                }
                                None => {}
                            }
                        }
                        return;
                    }
                    // The chest screen, on the same footing as the
                    // inventory: it has the cursor, so it has the click.
                    if chest_screen.is_open() && net.is_some() && !paused {
                        let click = match button {
                            MouseButton::Left => Some(inventory_screen::Button::Left),
                            MouseButton::Right => Some(inventory_screen::Button::Right),
                            _ => None,
                        };
                        if let Some(click) = click {
                            let quick = input
                                .action_down(&settings.keybinds, keybinds::Action::Sprint);
                            let intent = chest_screen.click(&inventory, click, quick);
                            if let (Some(intent), Some(net)) = (intent, net.as_ref()) {
                                use chest_screen::Intent;
                                net.send(match intent {
                                    Intent::Move { from, to, half } => ClientMessage::ChestMove {
                                        from: (from.0, from.1 as u8),
                                        to: (to.0, to.1 as u8),
                                        half,
                                    },
                                    Intent::QuickMove(side, slot) => {
                                        ClientMessage::ChestQuickMove {
                                            side,
                                            slot: slot as u8,
                                        }
                                    }
                                    Intent::BulkMove { to_chest } => {
                                        ClientMessage::ChestBulkMove { to_chest }
                                    }
                                });
                                debug_stats.network_messages_out_this_second += 1;
                            }
                        }
                        return;
                    }
                    // The inventory takes the click before the world
                    // does; it is the reason the cursor is loose.
                    if inventory_screen.open && net.is_some() && !paused {
                        let click = match button {
                            MouseButton::Left => Some(inventory_screen::Button::Left),
                            MouseButton::Right => Some(inventory_screen::Button::Right),
                            _ => None,
                        };
                        if let Some(click) = click {
                            // The screen decides *what* to ask for; the
                            // server decides whether it happens. Nothing
                            // moves locally, so there is no prediction to
                            // be undone by the next snapshot.
                            let quick = input
                                .action_down(&settings.keybinds, keybinds::Action::Sprint);
                            let intent = inventory_screen.click(&inventory, click, quick);
                            if let (Some(intent), Some(net)) = (intent, net.as_ref()) {
                                use inventory_screen::Intent;
                                net.send(match intent {
                                    Intent::Move { from, to } => ClientMessage::MoveSlots {
                                        from: from as u8,
                                        to: to as u8,
                                    },
                                    Intent::Split { from, to } => ClientMessage::SplitSlot {
                                        from: from as u8,
                                        to: to as u8,
                                    },
                                    Intent::QuickMove(slot) => {
                                        ClientMessage::QuickMoveSlot { slot: slot as u8 }
                                    }
                                    Intent::Sort => ClientMessage::SortInventory,
                                    Intent::Craft { index, times } => ClientMessage::Craft {
                                        index: index as u16,
                                        times,
                                    },
                                });
                                debug_stats.network_messages_out_this_second += 1;
                            }
                        }
                        return;
                    }
                    if net.is_none() || paused {
                        if button == MouseButton::Left {
                            if let Some(action) = menu.click() {
                                handle_action! { action }
                            }
                        }
                        return;
                    }
                    if !input.mouse_grabbed {
                        grab_cursor(&window, &mut input);
                        // The click that grabs the cursor is not also a
                        // swing at whatever happens to be under the
                        // crosshair.
                        input.breaking = false;
                    } else if button == MouseButton::Right {
                        // A block you can *open* takes the right click
                        // before a block you could place does. Otherwise
                        // the only way to use a chest with something in
                        // hand would be to empty your hand first, and
                        // the block would go on the front of it.
                        let opening = aimed_block(&chunks, &camera).filter(|(_, block)| {
                            primitive_shared::types::is_container(*block)
                        });
                        if let (Some((cell, _)), Some(net)) = (opening, net.as_ref()) {
                            net.send(ClientMessage::OpenChest {
                                global_x: cell.0,
                                global_y: cell.1,
                                global_z: cell.2,
                            });
                            debug_stats.network_messages_out_this_second += 1;
                            // The screen opens when the answer arrives.
                            // Everything else waits for that, including
                            // the cursor -- see the hand-off in the frame
                            // loop.
                            return;
                        }
                        // Placing is still instant; only breaking takes
                        // time. Held-to-repeat placement would need its
                        // own cooldown, and the server rate-limits edits
                        // anyway.
                        let others: Vec<Vec3> = remote_players.iter_positions().collect();
                        if let Some(net) = net.as_mut() {
                            try_place_block(
                                &chunks,
                                &camera,
                                &input,
                                &player,
                                &others,
                                net,
                                &inventory,
                                &mut debug_stats,
                            );
                        }
                    }
                }

                WindowEvent::RedrawRequested => {
                    // The server ended the session last frame. Close it
                    // down and put the reason on screen, rather than
                    // leaving a frozen world or closing the game.
                    if let Some(reason) = end_session.take() {
                        net = None;
                        if let Some(server) = local_server.take() {
                            runtime.block_on(server.stop());
                        }
                        // The world's geometry belongs to the session.
                        // The renderer owns it now, so leaving has to
                        // say so explicitly -- otherwise the menu would
                        // be drawn over the terrain of the world that
                        // just ended, and its space would stay held in
                        // the arena until the next world reclaimed it.
                        graphics.clear_chunk_meshes();
                        paused = false;
                        chat.close();
                        release_cursor(&window, &mut input);
                        menu.open(Screen::Main);
                        menu.fail(reason);
                    }

                    // --- menus ---
                    if net.is_none() {
                        let now = Instant::now();
                        menu.tick((now - last_menu_frame).as_secs_f32().min(0.25));
                        last_menu_frame = now;

                        // Has the connection attempt finished?
                        if let Some(receiver) = pending_connect.as_mut() {
                            match receiver.try_recv() {
                                Ok(Ok(session)) => {
                                    pending_connect = None;
                                    let welcome = session.connection.welcome.clone();
                                    println!(
                                        "connected to \"{}\" as player {} ({} Hz tick, server view distance {})",
                                        welcome.server_name,
                                        welcome.your_id,
                                        welcome.tick_rate_hz,
                                        welcome.server_view_distance,
                                    );

                                    // Reset every scrap of session state:
                                    // a second connection must not
                                    // inherit the first world's chunks,
                                    // light or player position.
                                    server_view_distance =
                                        welcome.server_view_distance.max(1);
                                    render_distance = settings
                                        .render_distance_chunks
                                        .min(welcome.server_view_distance.max(1));
                                    chunks = ChunkManager::new(render_distance);
                                    light = LightMap::new();
                                    graphics.clear_chunk_meshes();
                                    dirty.clear();
                                    urgent.clear();
                                    dirty_set.clear();
                                    arrivals.clear();
                                    chunk_versions.clear();
                                    remote_players = RemotePlayers::default();
                                    entities = entities::Entities::default();
                                    my_id = welcome.your_id;
                                    world_seed = welcome.world_seed;
                                    worldgen =
                                        primitive_shared::worldgen::WorldGen::new(world_seed);
                                    // The mesher colours foliage from
                                    // the same generator, so it has to
                                    // learn the new seed before the
                                    // first chunk of the new world is
                                    // submitted -- a chunk meshed with
                                    // the old one would wear another
                                    // world's climate until something
                                    // happened to dirty it.
                                    mesher.set_world(
                                        primitive_shared::worldgen::WorldGen::new(world_seed),
                                    );
                                    sky = Sky::new(welcome.time_of_day, welcome.day_length_seconds);
                                    player = Player::new(
                                        Vec3::new(welcome.spawn.0, welcome.spawn.1, welcome.spawn.2),
                                        settings.move_speed,
                                    );
                                    camera = Camera::new(player.eye_position(), graphics.aspect());
                                    camera.fov_y_radians = settings.fov_degrees.to_radians();
                                    world_ready = false;
                                    sequence = 0;
                                    last_sent_transform = None;
                                    last_frame = Instant::now();
                                    paused = false;
                                    input.release_all();
                                    // Survival state belongs to the
                                    // session, not to the process. The
                                    // server sends the real health with
                                    // the handshake; these values only
                                    // cover the frames before it lands,
                                    // and carrying the last world's
                                    // inventory into a new one would be
                                    // a duplication bug.
                                    inventory = Inventory::new();
                                    inventory_screen.close();
                                    mining.reset();
                                    stamina.reset();
                                    reported_slot = usize::MAX;
                                    health = survival_defaults::MAX;
                                    max_health = survival_defaults::MAX;
                                    recent_health = survival_defaults::MAX;
                                    death.close();
                                    was_dead = false;

                                    local_server = session.local_server;
                                    net = Some(session.connection.handle);
                                    grab_cursor(&window, &mut input);
                                }
                                Ok(Err(reason)) => {
                                    pending_connect = None;
                                    eprintln!("connection failed: {reason}");
                                    menu.fail(reason);
                                }
                                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                                Err(_) => {
                                    pending_connect = None;
                                    menu.fail("the connection task stopped".to_string());
                                }
                            }
                        }

                        if !menu_title_set {
                            window.set_title(&format!("Primitive {VERSION}"));
                            menu_title_set = true;
                        }
                        // Rebuilt only when something on it changed --
                        // an idle menu is the stillest screen in the
                        // game, and it used to be relaid and re-uploaded
                        // every frame. See `UiKey`.
                        let ui_rebuilt = {
                            let ctx = menu_context(&settings, &worlds, &graphics, false);
                            let key =
                                UiKey::menu_only(menu.ui_key(&ctx), graphics.aspect());
                            let changed = ui_key.as_ref() != Some(&key);
                            if changed {
                                ui_key = Some(key);
                                ui_vertices.clear();
                                menu.build_into(&ctx, &mut ui_vertices);
                            }
                            changed
                        };
                        // Full health: there is no player behind the
                        // menu to be hurt.
                        let params =
                            frame_params(&settings, &sky, render_distance, false, false, 1.0);
                        match graphics.render(
                            &camera,
                            &params,
                            None,
                            None,
                            None,
                            None,
                            // No world, so no hand: there is nobody
                            // standing behind the menu holding anything.
                            None,
                            None,
                            &ui_vertices,
                            ui_rebuilt,
                        ) {
                            Ok(()) => {}
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                graphics.resize(graphics.size)
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                            Err(e) => eprintln!("render error: {e:?}"),
                        }
                        return;
                    }

                    // Past this point there is a connection; the early
                    // return above is what guarantees it.
                    let net = net.as_mut().expect("connected");

                    let now = Instant::now();
                    // Clamp so a long stall (window drag, first-frame
                    // shader compile) doesn't teleport the player through
                    // the floor -- and doesn't look like speed-hacking to
                    // the server's anti-cheat either.
                    let frame_time = now - last_frame;
                    // Physics gets a clamped step so a long stall can't
                    // teleport the player through the floor. The debug
                    // stats get the *real* frame time -- recording the
                    // clamped value made every slow frame report as
                    // exactly 100 ms, which hid how slow they really were.
                    let dt = frame_time.as_secs_f32().min(0.1);
                    last_frame = now;
                    debug_stats.record_frame(frame_time);

                    let mut disconnected: Option<String> = None;
                    drain_network(
                        net,
                        &mut chunks,
                        &mut light,
                        &mut sky,
                        my_id,
                        &mut player,
                        &mut remote_players,
                        &mut entities,
                        &mut arrivals,
                        &mut urgent,
                        &mut dirty_set,
                        &mut chunk_versions,
                        &mut debug_stats,
                        &mut disconnected,
                        &mut inventory,
                        &mut inventory_screen,
                        &mut mining,
                        &mut health,
                        &mut max_health,
                        &mut recent_health,
                        &mut breath,
                        &mut shake,
                        &mut stamina,
                        &mut death,
                        &mut chest_screen,
                        &mut notice,
                        &mut chat,
                        &mut world_ready,
                    );

                    // Dying and coming back are the two moments the
                    // cursor changes hands without the player pressing
                    // anything, and both of them arrive as a message
                    // rather than as an event -- so the hand-off is done
                    // here, once, on the frame the answer changes.
                    death.tick(dt);
                    if death.is_open() != was_dead {
                        was_dead = death.is_open();
                        if was_dead {
                            // One screen at a time, and this one is not
                            // optional.
                            chat.close();
                            inventory_screen.close();
                            chest_screen.close();
                            release_cursor(&window, &mut input);
                            input.release_all();
                        } else if !paused && !chat.is_typing() {
                            grab_cursor(&window, &mut input);
                        }
                    }

                    // A chest opens when the server answers, not when
                    // the player clicks, so the cursor changes hands
                    // here for the same reason dying does.
                    if chest_screen.is_open() != chest_was_open {
                        chest_was_open = chest_screen.is_open();
                        if chest_was_open {
                            // One screen at a time.
                            inventory_screen.close();
                            chat.close();
                            release_cursor(&window, &mut input);
                            input.release_all();
                        } else if !paused && !death.is_open() && !chat.is_typing() {
                            grab_cursor(&window, &mut input);
                        }
                    }

                    // The session ended without the player asking. Tear
                    // it down here, at the top of the frame, rather than
                    // rendering a world that is no longer connected to
                    // anything.
                    if let Some(reason) = disconnected {
                        eprintln!("{reason}");
                        end_session = Some(reason);
                    }

                    // How much of *this* frame streaming may take.
                    //
                    // The configured budgets are 3 ms and 4 ms, which on
                    // a 60 Hz frame is nearly half of it: terrain
                    // arriving while the player walks turned into a
                    // visible hitch every few frames. Capping the pair
                    // at a share of the frame the machine is actually
                    // achieving keeps the hitch proportional -- and on a
                    // fast machine it *raises* throughput, because two
                    // hundred small slices a second is more work than
                    // sixty large ones.
                    //
                    // Not while the world is still loading: there is
                    // nothing to be smooth for yet, and the whole
                    // configured budget gets the player into the world
                    // sooner.
                    let chunk_ms =
                        streaming_budget(settings.chunk_budget_ms, dt, world_ready);
                    let mesh_ms = streaming_budget(settings.mesh_budget_ms, dt, world_ready);

                    integrate_chunks(
                        &mut arrivals,
                        &mut chunks,
                        &mut mesher,
                        chunk_ms,
                        &mut debug_stats,
                    );

                    sky.tick(dt);

                    dispatch_meshing(
                        &mut urgent,
                        &mut dirty,
                        &mut dirty_set,
                        &chunk_versions,
                        &mut mesher,
                        &chunks,
                        &light,
                        mesh_ms,
                        &mut debug_stats,
                    );
                    collect_worker_results(
                        &mut mesher,
                        &mut graphics,
                        &chunks,
                        &mut light,
                        &mut urgent,
                        &mut dirty,
                        &mut dirty_set,
                        &chunk_versions,
                        ChunkManager::chunk_for_world_pos(player.position.x, player.position.z),
                        &mut debug_stats,
                    );

                    if input.mouse_grabbed && !paused {
                        camera.apply_mouse_delta(
                            input.mouse_dx,
                            input.mouse_dy,
                            settings.mouse_sensitivity,
                        );
                    }
                    menu.tick(dt);

                    remote_players.tick(dt);
                    entities.tick(dt);
                    // Into a buffer that lives across frames. It is
                    // empty in singleplayer and a handful of entries
                    // otherwise, but this runs every frame, and a heap
                    // allocation per frame for nothing is exactly the
                    // sort of cost that only shows up once the frame
                    // rate is high enough for it to matter.
                    other_positions.clear();
                    other_positions.extend(remote_players.iter_positions());

                    let player_chunk =
                        ChunkManager::chunk_for_world_pos(player.position.x, player.position.z);
                    let (area_loaded, area_needed) = chunks.spawn_area_progress(player_chunk);
                    if !world_ready && area_loaded == area_needed {
                        world_ready = true;
                        println!("world ready -- spawning");
                    }

                    if world_ready {
                        // Physics still runs while paused -- gravity does
                        // not stop for a menu on an authoritative server
                        // -- but the player stops steering.
                        // A dead player steers no more than a paused one
                        // does. Gravity still applies to both -- the
                        // server is authoritative about where bodies
                        // are, and a corpse hovering where it died would
                        // rubber-band the moment it respawned.
                        let frozen = paused
                            || death.is_open()
                            || inventory_screen.open
                            || chest_screen.is_open()
                            || chat.is_typing();
                        let wish_dir = if frozen {
                            Vec3::ZERO
                        } else {
                            wish_direction(&input, &camera, &settings.keybinds)
                        };
                        // Weight slows you down and stamina decides
                        // whether the sprint is available at all. Both
                        // are folded in here so physics only ever sees
                        // one speed and one flag.
                        let carried = inventory.total_weight();
                        player.speed_scale = primitive_shared::load::speed_scale(carried);
                        let wants_sprint = !frozen && input.action_down(&settings.keybinds, keybinds::Action::Sprint);
                        let sprinting = wants_sprint && stamina.can_sprint();
                        // Physics in fixed slices, not one step of
                        // however long the frame was.
                        //
                        // A step resolves collisions by moving and then
                        // pushing back out, so its size bounds how far
                        // the player may travel inside one: at 100 ms
                        // and terminal velocity that is several blocks,
                        // which goes *through* a floor. The server then
                        // rejects the position and rubber-bands them
                        // back, and what the player feels is the physics
                        // lurching every time the frame rate hiccups.
                        //
                        // Bounded, so a stall cannot turn into a spiral
                        // of catch-up steps that causes the next one.
                        // A jump is a push, and an exhausted player has
                        // nothing to push with. Refused here rather than
                        // inside physics, which has no idea what stamina
                        // is -- and refused rather than weakened, since
                        // half a jump is a way to end up stuck in a hole.
                        let may_jump = stamina.can_jump();
                        let mut left = dt;
                        let mut first = true;
                        while left > 0.0 {
                            let step = left.min(PHYSICS_STEP);
                            player.update(
                                &chunks,
                                &other_positions,
                                wish_dir,
                                // Where the camera points, not where the
                                // player faces. Only water reads it, and
                                // it is what lets a swimmer dive: see
                                // `physics::stroke_direction`.
                                camera.forward(),
                                // Only the first slice may jump: the
                                // press edge is one event, and firing it
                                // in every slice is a jump that scales
                                // with how bad the frame was.
                                first
                                    && !frozen
                                    && may_jump
                                    && input.action_pressed(
                                        &settings.keybinds,
                                        keybinds::Action::Jump,
                                    ),
                                !frozen
                                    && input.action_down(
                                        &settings.keybinds,
                                        keybinds::Action::Jump,
                                    ),
                                sprinting,
                                step,
                            );
                            // Billed per push that actually happened,
                            // not per press: physics refuses a jump in
                            // mid-air, and swimming up is not one at all.
                            if player.jumped {
                                stamina.spend_jump();
                            }
                            left -= step;
                            first = false;
                        }

                        // Billed for the sprint the player actually got,
                        // not the one they asked for: physics refuses it
                        // in water and standing still, and charging for
                        // a sprint that did not happen is the sort of
                        // thing players notice and cannot explain.
                        let really_running =
                            sprinting && player.grounded && !player.swimming
                                && player.horizontal_speed() > 0.5;
                        stamina.update(
                            dt,
                            primitive_shared::load::load_fraction(carried),
                            really_running,
                        );
                    }
                    camera.position = player.eye_position();

                    // --- camera motion that is not the player moving ---
                    //
                    // The bob follows the *actual* speed and only counts
                    // while running on the ground, so walking into a
                    // wall or swimming does not sway the view.
                    let sprinting_now = input.action_down(&settings.keybinds, keybinds::Action::Sprint)
                        && !paused
                        && !death.is_open()
                        && player.grounded
                        && !player.swimming;
                    shake.update(dt, player.horizontal_speed(), sprinting_now);
                    // The step lag rides with the bob rather than with
                    // the position, and that is the point: `camera.shake`
                    // moves the *view* only, so walking up onto a drift
                    // of snow rises smoothly while the interaction ray
                    // -- and therefore what the server is told was
                    // clicked -- stays exactly where the player is.
                    camera.shake = shake.offset(camera.right_horizontal(), Vec3::Y)
                        - Vec3::Y * player.view_step_lag();
                    camera.shake_angles = shake.angles();

                    // The ghost on the health bar drains back to the
                    // real value. Fast enough to be over before the next
                    // hit, slow enough to be seen.
                    const HEALTH_GHOST_DRAIN_PER_SEC: f32 = 8.0;
                    recent_health =
                        (recent_health - HEALTH_GHOST_DRAIN_PER_SEC * dt).max(health);

                    request_and_unload(
                        &mut chunks,
                        &mut light,
                        net,
                        &player,
                        now,
                        &mut graphics,
                        &mut debug_stats,
                    );

                    maybe_send_transform(
                        net,
                        &player,
                        &camera,
                        now,
                        player_update_interval,
                        &mut last_player_update_sent,
                        &mut last_sent_transform,
                        &mut sequence,
                        &mut debug_stats,
                    );

                    // Which slot is selected is the server's business
                    // too: it decides what a placement spends and which
                    // blocks a swing can get through. Sent only on a
                    // change.
                    //
                    // **Before the mining below, not after it.** It used
                    // to be sent at the end of the frame, which meant a
                    // player who switched to a pick and finished a block
                    // in the same frame sent the break first and the
                    // slot second: the server judged the break against
                    // the slot they had a moment ago, refused it as
                    // impossible with bare hands, and the block came
                    // back. Rare, and infuriating when it happened,
                    // because everything on screen said the block was
                    // gone. The client's own prediction reads
                    // `input.hotbar_slot` directly and so was always a
                    // frame ahead of what the server had been told.
                    if input.hotbar_slot != reported_slot {
                        reported_slot = input.hotbar_slot;
                        net.send(ClientMessage::SelectSlot {
                            slot: reported_slot as u8,
                        });
                        debug_stats.network_messages_out_this_second += 1;
                    }

                    // --- mining, and hitting people ---
                    //
                    // Breaking takes time, so it advances here rather
                    // than on the click. A dead or paused player is not
                    // swinging at anything.
                    let can_mine =
                        world_ready && !paused && !death.is_open() && input.mouse_grabbed;

                    // Someone under the crosshair takes the swing before
                    // the world behind them does. Nearer than whatever
                    // block is there, or a punch through a wall would
                    // land -- the server checks the distance, but it has
                    // no idea what is between the two of them.
                    let struck = if can_mine && input.breaking {
                        swing_at_player(
                            &remote_players,
                            &chunks,
                            &camera,
                            now,
                            &mut last_swing,
                            net,
                            &mut debug_stats,
                        )
                    } else {
                        false
                    };

                    // Anything a bare hand cannot get through is not
                    // aimed at for the purpose of mining: the progress
                    // bar never starts and the cracks never appear,
                    // rather than filling up and achieving nothing.
                    //
                    // Nor is anything behind a player being hit: one
                    // button, one thing at a time, or a fight in front
                    // of a wall quietly digs it out.
                    //
                    // What "a bare hand" means now depends on what is in
                    // the selected slot: the same rock that ignores
                    // fingers gives way to a pick. The client predicts
                    // this so the bar and the cracks agree with what the
                    // server will allow -- both sides read the same
                    // `break_seconds_with`, which is the whole reason it
                    // lives in `primitive_shared`.
                    let held_tool = inventory.block_in(input.hotbar_slot);
                    let aim = if can_mine && !struck {
                        aimed_block(&chunks, &camera).filter(|(_, block)| {
                            primitive_shared::types::is_breakable_with(*block, held_tool)
                        })
                    } else {
                        None
                    };
                    // Digging is work, and it comes out of the same tank
                    // running and jumping do. An exhausted player digs
                    // slower rather than not at all -- see
                    // `stamina::EXHAUSTED_DIG_RATE` -- so the progress
                    // the swing makes is scaled here and the bill is
                    // paid on the swing that finished.
                    let dug = mining.update(
                        aim,
                        can_mine && input.breaking,
                        dt * stamina.dig_rate(),
                        held_tool,
                    );
                    if let Some(cell) = dug {
                        // Billed for the work the swing actually was: a
                        // pick makes a block cheaper in seconds, and the
                        // tank is measured in seconds, so a better tool
                        // is less tiring as well as faster.
                        if let Some(seconds) = aim.and_then(|(_, block)| {
                            primitive_shared::types::break_seconds_with(block, held_tool)
                        }) {
                            stamina.spend_dig(seconds);
                        }
                        request_break(&chunks, cell, net, &mut debug_stats);
                    }
                    // The arm follows from all of that rather than
                    // deciding any of it: a blow lands on a player, or a
                    // block is coming apart, and the hand is what that
                    // looks like. Advanced every frame even though the
                    // geometry is only rebuilt at `DYNAMIC_REBUILD_HZ` --
                    // the state is a few floats, and letting it skip
                    // frames would make the swing's length depend on the
                    // frame rate.
                    if struck {
                        hand.strike();
                    }
                    hand.update(
                        dt,
                        // Not "the button is down": swinging at thin air
                        // or at bedrock is a click, not a rhythm. `aim`
                        // is what the client believes is actually coming
                        // apart under the crosshair.
                        can_mine && input.breaking && aim.is_some(),
                        player.velocity.with_y(0.0).length(),
                        player.grounded,
                    );

                    input.end_frame();

                    // Decided by the head, not the feet: standing
                    // waist-deep in a lake shouldn't tint the screen.
                    // Physics already samples this, so there's one
                    // definition of "under water" rather than two.
                    let underwater = player.submerged;

                    let params = frame_params(
                        &settings,
                        &sky,
                        render_distance,
                        fog_enabled,
                        underwater,
                        if max_health > 0.0 { health / max_health } else { 1.0 },
                    );
                    let loading = if world_ready {
                        None
                    } else {
                        Some(area_loaded as f32 / area_needed.max(1) as f32)
                    };

                    // The title bar is a window-manager call and a fresh
                    // format! of a dozen numbers; the readout behind it
                    // samples the biome generator. Neither is worth
                    // doing every frame -- nobody reads a title bar at
                    // 200 Hz -- so both are built only when something is
                    // going to look at them.
                    const TITLE_INTERVAL: Duration = Duration::from_millis(250);
                    let title_due = now.duration_since(last_title_update) >= TITLE_INTERVAL;
                    let info = (title_due || debug_stats.console_enabled).then(|| FrameInfo {
                        position: player.position,
                        chunk: ChunkManager::chunk_for_world_pos(
                            player.position.x,
                            player.position.z,
                        ),
                        grounded: player.grounded,
                        loaded_chunks: chunks.loaded_count(),
                        pending_chunks: chunks.pending_count(),
                        surface: (graphics.size.width, graphics.size.height),
                        anisotropy: settings.anisotropy,
                        sky_scale: settings.sky_scale,
                        render_distance: chunks.render_distance(),
                        queued_meshes: dirty.len() + urgent.len() + mesher.in_flight(),
                        queued_arrivals: arrivals.len(),
                        lighting_jobs: mesher.lighting_in_flight(),
                        remote_players: remote_players.len(),
                        entities: entities.len(),
                        clock: sky.clock_string(),
                        sun_intensity: sky.sun_intensity(),
                        seed: world_seed,
                        biome: worldgen
                            .biome_at(
                                player.position.x.floor() as i32,
                                player.position.z.floor() as i32,
                            )
                            .name(),
                        selected_block: inventory
                            .block_in(input.hotbar_slot)
                            .map(block_name)
                            .unwrap_or("nothing"),
                        draw_calls: graphics.draw_calls_last_frame,
                        chunks_culled: graphics.chunks_culled_last_frame,
                        underwater,
                        health,
                        max_health,
                        held: inventory.count_in(input.hotbar_slot),
                        carried: inventory.total_items(),
                        mining: mining.target().map(|cell| (cell, mining.progress())),
                    });
                    if let Some(info) = info.as_ref() {
                        if title_due {
                            window.set_title(&debug_stats.title(info));
                            last_title_update = now;
                            menu_title_set = false;
                        }
                        debug_stats.maybe_dump_console(info);

                        // The clock starts on the first frame with a
                        // world in front of it, not at launch: loading
                        // one takes seconds and none of them are frame
                        // time. See `bench_seconds`.
                        if let Some(seconds) = bench_seconds {
                            let started = *bench_started.get_or_insert_with(Instant::now);
                            if started.elapsed().as_secs_f32() >= seconds {
                                println!("bench: done");
                                elwt.exit();
                            }
                        }
                    }

                    // The clock the *moving* geometry below rebuilds on,
                    // and the pace the interface falls back to while
                    // something on it is animating.
                    let rebuild_due = last_rebuild.is_none_or(|last| {
                        now.duration_since(last).as_secs_f32() >= 1.0 / DYNAMIC_REBUILD_HZ
                    });
                    if rebuild_due {
                        last_rebuild = Some(now);
                    }

                    // --- UI ---
                    //
                    // One vertex list for the whole overlay: hotbar,
                    // then the F3 panel, then the pause screen on top.
                    // They share a pipeline and a buffer, so the order
                    // they are appended in is the order they stack.
                    // The hotbar is hidden behind the loading screen --
                    // there is nothing to place yet, and it would sit on
                    // top of the dim.
                    //
                    // Rebuilt when its inputs changed, not on a clock:
                    // see `UiKey`. The clock survives only as the pace
                    // for the elements that animate on time alone.

                    // Whatever the server last refused, until it has
                    // been on screen long enough to read.
                    let notice_drawn = notice.as_ref().and_then(|(text, at)| {
                        let age = now.duration_since(*at).as_secs_f32();
                        let left = hud::NOTICE_SECONDS - age;
                        (left > 0.0)
                            .then(|| (text.as_str(), (left / hud::NOTICE_FADE_SECONDS).min(1.0)))
                    });
                    let debug_panel_shown = info.is_some() && debug_stats.console_enabled;
                    let key = UiKey {
                        in_game: true,
                        aspect: graphics.aspect().to_bits(),
                        loading: loading.is_some(),
                        hotbar_slot: input.hotbar_slot,
                        inventory: inventory_fingerprint(&inventory),
                        health: health.to_bits(),
                        max_health: max_health.to_bits(),
                        recent_health: recent_health.to_bits(),
                        stamina: stamina.fraction().to_bits(),
                        exhausted: stamina.is_exhausted(),
                        breath: breath.to_bits(),
                        notice: notice_drawn
                            .map(|(text, fade)| (text_fingerprint(text), fade >= 1.0)),
                        chat: chat.ui_key(now),
                        inventory_screen: inventory_screen.ui_key(),
                        chest_screen: chest_screen.ui_key(),
                        death: death.ui_key(),
                        debug_panel: debug_panel_shown,
                        menu: paused.then(|| {
                            menu.ui_key(&menu_context(&settings, &worlds, &graphics, true))
                        }),
                        language: settings.language,
                    };
                    // The parts that change with no event behind them,
                    // for which time is the only trigger there is.
                    let ui_animating = chat.is_fading(now)
                        || matches!(notice_drawn, Some((_, fade)) if fade < 1.0)
                        || death.is_animating()
                        || debug_panel_shown;
                    let ui_rebuilt =
                        ui_key.as_ref() != Some(&key) || (ui_animating && rebuild_due);
                    if ui_rebuilt {
                    ui_key = Some(key);
                    ui_vertices.clear();
                    if loading.is_none() {
                        hotbar::build_into(
                            &graphics.textures,
                            &inventory,
                            input.hotbar_slot,
                            &mut ui_vertices,
                        );
                        // Stack counts and the health bar sit on top of
                        // the bar, so they are appended after it.
                        hud::build_into(
                            graphics.textures.font,
                            health,
                            max_health,
                            recent_health,
                            stamina.fraction(),
                            stamina.is_exhausted(),
                            breath,
                            &inventory,
                            notice_drawn,
                            &mut ui_vertices,
                        );
                        // Chat sits over the HUD and under the
                        // inventory: it is readable while playing, and
                        // it is not what a player opening their pack is
                        // looking at.
                        if chat.has_anything_to_draw(now) {
                            chat.build_into(
                                graphics.textures.font,
                                graphics.aspect(),
                                now,
                                &mut ui_vertices,
                            );
                        }

                        // The inventory sits over the HUD, and the pause
                        // screen (appended below) over both. Guarded
                        // rather than left to `build_into`'s early
                        // return, because assembling the argument clones
                        // the texture table -- an allocation a frame for
                        // a screen that is almost always shut.
                        if inventory_screen.open {
                            // The face table is the one built at startup,
                            // not a fresh copy: `face_layers()` clones
                            // the whole lookup, and doing that once a
                            // frame for a screen that is open for
                            // seconds at a time is an allocation nobody
                            // asked for.
                            inventory_screen.build_into(
                                graphics.textures.font,
                                &face_layers,
                                &inventory,
                                stamina.fraction(),
                                settings.language,
                                &mut ui_vertices,
                            );
                        }

                        // The chest sits where the inventory does,
                        // and the two are never open at once -- opening
                        // either closes the other.
                        if chest_screen.is_open() {
                            chest_screen.build_into(
                                graphics.textures.font,
                                &face_layers,
                                &inventory,
                                settings.language,
                                &mut ui_vertices,
                            );
                        }

                        // The death screen goes over all of it. The
                        // pause menu is drawn after this whole block and
                        // therefore still sits on top, which is right:
                        // it is the only screen that can leave the
                        // world, and a dead player is exactly who wants
                        // to.
                        if death.is_open() {
                            death.build_into(graphics.textures.font, settings.language, &mut ui_vertices);
                        }
                    }
                    if let Some(info) = info.as_ref().filter(|_| debug_stats.console_enabled) {
                        debug_panel_into(
                            &debug_stats.overlay_lines(info),
                            graphics.aspect(),
                            graphics.textures.font,
                            &mut ui_vertices,
                        );
                    }
                    if paused {
                        menu.build_into(
                            &menu_context(&settings, &worlds, &graphics, true),
                            &mut ui_vertices,
                        );
                    }
                    } // ui_rebuilt

                    // Entities are drawn with the terrain pipeline, so
                    // they get the same textures, lighting and fog as
                    // the blocks they came from.
                    // Written into buffers that persist between frames
                    // rather than freshly allocated ones -- both on the
                    // CPU and on the GPU. See `write_dynamic_mesh`.
                    if rebuild_due {
                    entity_vertices.clear();
                    entity_indices.clear();
                    item_vertices.clear();
                    item_indices.clear();
                    if !entities.is_empty() {
                        entities.build_meshes_into(
                            &face_layers,
                            &light,
                            Some(&graphics.textures),
                            &mut entity_vertices,
                            &mut entity_indices,
                            &mut item_vertices,
                            &mut item_indices,
                        );
                    }
                    graphics.write_dynamic_mesh(
                        &mut entity_mesh,
                        &entity_vertices,
                        &entity_indices,
                    );
                    graphics.write_dynamic_mesh(&mut item_mesh, &item_vertices, &item_indices);

                    // The player's own hand, and what is in it. Hidden
                    // behind anything that has taken over the screen:
                    // a menu, a container, the death screen, or the
                    // loading bar with no world behind it yet. An arm
                    // waving over the inventory is worse than no arm.
                    hand_vertices.clear();
                    hand_indices.clear();
                    hand.build_into(
                        world_ready
                            && loading.is_none()
                            && !paused
                            && !death.is_open()
                            && !inventory_screen.open
                            && !chest_screen.is_open(),
                        inventory.block_in(input.hotbar_slot),
                        &face_layers,
                        Some(&graphics.textures),
                        // Lit by the cell the player's own head is in,
                        // the same way a dropped item is lit by the cell
                        // it lies in.
                        entities::sampled_light(camera.position, &light),
                        &mut hand_vertices,
                        &mut hand_indices,
                    );
                    graphics.write_dynamic_mesh(&mut hand_mesh, &hand_vertices, &hand_indices);

                    actor_vertices.clear();
                    actor_indices.clear();
                    remote_players::build_actor_mesh_into(
                        &remote_players,
                        &mut actor_vertices,
                        &mut actor_indices,
                    );
                    // The block outline and its cracks are untextured
                    // lit triangles, which is exactly what the actor
                    // pipeline already draws -- so they ride along in
                    // the same buffer rather than needing a pass of
                    // their own.
                    mining.build_overlay_into(&mut actor_vertices, &mut actor_indices);
                    graphics.write_dynamic_mesh(&mut actor_mesh, &actor_vertices, &actor_indices);

                    break_vertices.clear();
                    break_indices.clear();
                    if let Some(stage) = mining.break_stage() {
                        mining.build_break_mesh_into(
                            graphics.textures.break_layer(stage),
                            &mut break_vertices,
                            &mut break_indices,
                        );
                    }
                    graphics.write_dynamic_mesh(
                        &mut break_mesh,
                        &break_vertices,
                        &break_indices,
                    );
                    } // rebuild_due

                    // Everything above was this frame's own work; what
                    // follows is the renderer's. See
                    // `DebugStats::record_phases`.
                    let simulation = now.elapsed();
                    match graphics.render(
                        &camera,
                        &params,
                        Some(&actor_mesh),
                        Some(&entity_mesh),
                        Some(&item_mesh),
                        Some(&break_mesh),
                        Some(&hand_mesh),
                        loading,
                        &ui_vertices,
                        ui_rebuilt,
                    ) {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            graphics.resize(graphics.size)
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                        Err(e) => eprintln!("render error: {e:?}"),
                    }
                    debug_stats.record_phases(
                        simulation,
                        graphics.encode_time_last_frame,
                        graphics.acquire_time_last_frame,
                        graphics.present_time_last_frame,
                        graphics.gpu_time_last_frame(),
                        graphics.gpu_stage_ms_last_frame(),
                    );
                }

                _ => {}
            },

            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if input.mouse_grabbed {
                    input.accumulate_mouse(delta.0 as f32, delta.1 as f32);
                }
            }

            Event::AboutToWait => window.request_redraw(),

            _ => {}
        }
    })?;

    Ok(())
}

/// How much of this frame chunk streaming may take.
///
/// The configured budgets are milliseconds of *main-thread* work per
/// frame -- 3 for integrating arrived chunks, 4 for handing chunks to
/// the mesher. On a 60 Hz frame that pair is nearly half of it, and
/// terrain arriving while the player walks turned into a visible hitch
/// every few frames.
///
/// Capping them at a share of the frame the machine is actually
/// achieving keeps the hitch proportional to the frame instead of
/// fixed. On a fast machine it also *raises* throughput: two hundred
/// small slices a second is more work than sixty large ones.
///
/// While the world is still loading the cap is off. There is nothing to
/// be smooth for yet, and the full budget is what gets the player into
/// the world sooner.
fn streaming_budget(configured_ms: f32, frame_seconds: f32, world_ready: bool) -> f32 {
    /// Share of a frame streaming may spend.
    const SHARE: f32 = 0.30;
    /// ...but never so little that streaming stops making progress on a
    /// machine running at three hundred frames a second.
    const FLOOR_MS: f32 = 0.5;

    if !world_ready {
        return configured_ms;
    }
    configured_ms.min((frame_seconds * 1000.0 * SHARE).max(FLOOR_MS))
}

/// Longest slice of time physics is allowed to advance in one go.
///
/// Sixty a second: fine enough that nothing moves more than a fraction
/// of a block per step at any speed the game can produce, coarse enough
/// that a fast machine still runs one step per frame.
const PHYSICS_STEP: f32 = 1.0 / 60.0;

/// Shuts the chat box and decides who gets the cursor back.
///
/// Not simply "grab": the box can be closed while something else wants
/// the pointer loose, and handing it back to the world with the pause
/// menu up leaves a menu nobody can click.
fn close_chat(
    chat: &mut chat::Chat,
    window: &winit::window::Window,
    input: &mut input::InputState,
    paused: bool,
) {
    chat.close();
    if !paused {
        grab_cursor(window, input);
    }
}

/// Everything the shaders need for this frame, derived from the
/// server-synced sky plus local render settings.
fn frame_params(
    settings: &ClientSettings,
    sky: &Sky,
    render_distance: i32,
    fog_enabled: bool,
    underwater: bool,
    health_fraction: f32,
) -> FrameParams {
    // What distance looks like this frame, worked out in one place --
    // see `engine::fog`, which exists because the colour, the range and
    // the underwater case used to live in three files that each held a
    // third of the answer.
    let fog = fog::Fog::for_frame(settings, sky, render_distance, fog_enabled, underwater);

    FrameParams {
        sun_direction: sky.sun_direction(),
        sun_intensity: sky.sun_intensity(),
        fog_color: fog.color,
        fog_start: fog.start,
        fog_end: fog.end,
        // What is drawn, as opposed to what fades: see `view_distance`.
        // Taken from the render distance actually in force, which is
        // the player's setting capped by what the server streams --
        // never from the fog, which the player can switch off.
        view_distance: (render_distance as f32) * 16.0,
        ambient: settings.ambient_light,
        block_light_boost: settings.block_light_boost,
        ao_strength: settings.ambient_occlusion,
        fog_enabled: fog.enabled,
        underwater: fog.underwater,
        fog_cull_distance: fog.cull_distance(),
        transparent_leaves: settings.transparent_leaves,
        time_of_day: sky.time_of_day,
        cloudiness: settings.cloudiness,
        hurt: hurt_from(health_fraction),
        elapsed_seconds: sky.elapsed(),
        detail_distance: settings.detail_distance,
    }
}

/// How dark the edges of the screen go, from how much health is left.
///
/// Nothing at all above two thirds: a frame that is always there is not
/// information, it is the picture. Below that it comes in with the
/// square of how far the bar has fallen, so the first sign of it is a
/// hint in the corners and being nearly dead is unmistakable.
///
/// Here rather than in the renderer because it is a design decision
/// about health, and it belongs next to the other ones.
fn hurt_from(health_fraction: f32) -> f32 {
    const STARTS_AT: f32 = 0.65;
    if !health_fraction.is_finite() {
        return 0.0;
    }
    ((STARTS_AT - health_fraction.clamp(0.0, 1.0)) / STARTS_AT).clamp(0.0, 1.0)
}

fn wish_direction(
    input: &input::InputState,
    camera: &Camera,
    binds: &keybinds::Keybinds,
) -> Vec3 {
    use keybinds::Action;
    let mut dir = Vec3::ZERO;
    if input.action_down(binds, Action::Forward) {
        dir += camera.forward_horizontal();
    }
    if input.action_down(binds, Action::Back) {
        dir -= camera.forward_horizontal();
    }
    if input.action_down(binds, Action::Right) {
        dir += camera.right_horizontal();
    }
    if input.action_down(binds, Action::Left) {
        dir -= camera.right_horizontal();
    }
    if dir.length_squared() > 0.0 {
        dir.normalize()
    } else {
        dir
    }
}

fn grab_cursor(window: &winit::window::Window, input: &mut input::InputState) {
    if window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok()
    {
        window.set_cursor_visible(false);
        input.mouse_grabbed = true;
    }
}

fn release_cursor(window: &winit::window::Window, input: &mut input::InputState) {
    let _ = window.set_cursor_grab(CursorGrabMode::None);
    window.set_cursor_visible(true);
    input.mouse_grabbed = false;
}

/// What the crosshair is on, as (cell, block in it).
/// Shuts the chest screen and says so.
///
/// The message matters as much as the screen does: until the server
/// hears it, this player is still counted as standing at that chest and
/// is still sent an update every time anyone else changes it.
fn close_chest(
    screen: &mut chest_screen::ChestScreen,
    net: Option<&network::NetworkHandle>,
    debug_stats: &mut DebugStats,
) {
    if !screen.is_open() {
        return;
    }
    screen.close();
    if let Some(net) = net {
        net.send(ClientMessage::CloseChest);
        debug_stats.network_messages_out_this_second += 1;
    }
}

/// Hits whoever is under the crosshair, at most once per cooldown.
///
/// Returns whether a player is under the crosshair *at all*, not whether
/// a punch was sent. The caller uses it to keep mining out of the way,
/// and the wall behind someone must not quietly start coming apart in
/// the gaps between swings.
///
/// Two things are decided here and neither is trusted by the server:
/// the cooldown, which stops the button being a packet per frame, and
/// line of sight, which the server cannot check -- it knows how far
/// apart two players are and nothing about what is between them.
#[allow(clippy::too_many_arguments)]
fn swing_at_player(
    players: &RemotePlayers,
    chunks: &ChunkManager,
    camera: &Camera,
    now: Instant,
    last_swing: &mut Option<Instant>,
    net: &mut network::NetworkHandle,
    debug_stats: &mut DebugStats,
) -> bool {
    use primitive_shared::combat;

    let forward = camera.forward();
    let Some((target, distance)) =
        players.aimed_at(camera.position, forward, combat::MELEE_REACH)
    else {
        return false;
    };
    // Whatever the ray meets first wins. A punch that lands through a
    // wall is the sort of thing a player remembers about a game.
    if physics::raycast_block(chunks, camera.position, forward, distance).is_some() {
        return false;
    }

    let ready = last_swing.is_none_or(|last| {
        now.duration_since(last).as_secs_f32() >= combat::MELEE_COOLDOWN_SECS
    });
    if ready {
        *last_swing = Some(now);
        net.send(ClientMessage::Attack { target });
        debug_stats.network_messages_out_this_second += 1;
    }
    true
}

fn aimed_block(chunks: &ChunkManager, camera: &Camera) -> Option<((i32, i32, i32), BlockId)> {
    let (hit, _) =
        physics::raycast_block(chunks, camera.position, camera.forward(), INTERACT_RANGE)?;
    let block = chunks.block_at(hit.0, hit.1, hit.2)?;
    Some((hit, block))
}

/// Asks the server to remove a block the player has finished mining.
///
/// The inventory is told what we *asked* for, not what we got: the drop
/// is only credited when the server confirms the cell became air. See
/// `inventory` for why that distinction matters.
fn request_break(
    chunks: &ChunkManager,
    cell: (i32, i32, i32),
    net: &mut network::NetworkHandle,
    debug_stats: &mut DebugStats,
) {
    if chunks.block_at(cell.0, cell.1, cell.2).is_none() {
        return; // the chunk went away mid-swing
    }
    // Nothing is credited here. The server drops what the block yields
    // into the world, and it is picked up by walking over it.
    net.send(ClientMessage::SetBlock {
        global_x: cell.0,
        global_y: cell.1,
        global_z: cell.2,
        block_id: BLOCK_AIR,
    });
    debug_stats.network_messages_out_this_second += 1;
}

#[allow(clippy::too_many_arguments)]
fn try_place_block(
    chunks: &ChunkManager,
    camera: &Camera,
    input: &input::InputState,
    player: &Player,
    others: &[Vec3],
    net: &mut network::NetworkHandle,
    inventory: &Inventory,
    debug_stats: &mut DebugStats,
) {
    let Some((hit, before)) =
        physics::raycast_block(chunks, camera.position, camera.forward(), INTERACT_RANGE)
    else {
        return;
    };
    // An empty slot places nothing. There is no fallback block: the bar
    // shows what you are carrying, and a slot with nothing in it means
    // exactly that.
    let Some(block_id) = inventory.block_in(input.hotbar_slot) else {
        return;
    };
    // An item is not a block: there is no cell of the world that could
    // hold a handful of fibre. The server refuses this too, but doing
    // it here means right-clicking with one selected does nothing at
    // all, rather than producing an error message a round trip later.
    if !primitive_shared::types::is_placeable(block_id) {
        return;
    }
    // A tuft of grass is something you build *through*, not against.
    //
    // Now that a ray stops at one -- it has to, or grass could never be
    // broken -- the cell in front of it is where a block would go, and
    // that is a block hanging in the air with a blade of grass behind
    // it. Anything walk-through is replaced instead, which is both what
    // every other game does and what the player is plainly asking for.
    let aimed_at = chunks.block_at(hit.0, hit.1, hit.2).unwrap_or(BLOCK_AIR);
    let replaces_target = primitive_shared::types::is_cross(aimed_at);

    let target = if replaces_target { hit } else { before };

    let block_id = {
        // Which way it lies, for anything that has a way to lie.
        //
        // The axis of the face you built against, which is the rule
        // everyone already knows from placing a log: click the top of a
        // block and the log stands, click a side and it lies pointing
        // at you. `before` and `hit` differ by exactly one cell along
        // the face's normal, so the face is the difference.
        let normal = (before.0 - hit.0, before.1 - hit.1, before.2 - hit.2);
        match primitive_shared::types::Axis::of_normal(normal.0, normal.1, normal.2) {
            Some(axis) => primitive_shared::types::oriented(block_id, axis),
            None => block_id,
        }
    };

    // The same question the server will ask, asked here so a placement
    // it would refuse is never sent.
    let occupying = chunks
        .block_at(target.0, target.1, target.2)
        .unwrap_or(BLOCK_AIR);
    if primitive_shared::types::layer_placement(occupying, block_id).is_none() {
        return;
    }

    // ...and nothing that needs the ground goes in the air. Refused
    // here as well as on the server so the block never flickers into
    // view only to be taken back a round trip later.
    if primitive_shared::types::needs_support(block_id) {
        let under = chunks
            .block_at(target.0, target.1 - 1, target.2)
            .unwrap_or(BLOCK_AIR);
        if !primitive_shared::types::can_grow_on(block_id, under) {
            return;
        }
    }

    // Don't place a block inside a player -- yourself included. Looking
    // down and right-clicking would otherwise wall you into the ground.
    // The server enforces this too; checking here means the block never
    // flickers into view only to be taken back a round trip later.
    let feet = (player.position.x, player.position.y, player.position.z);
    if block_overlaps_player(feet, target.0, target.1, target.2, block_id) {
        return;
    }
    for other in others {
        if block_overlaps_player(
            (other.x, other.y, other.z),
            target.0,
            target.1,
            target.2,
            block_id,
        ) {
            return;
        }
    }

    // Note: no local prediction. The world only changes when the server
    // confirms it with a `BlockUpdate` -- which is also what keeps the
    // view honest when the server's anti-cheat rejects an edit.
    net.send(ClientMessage::SetBlock {
        global_x: target.0,
        global_y: target.1,
        global_z: target.2,
        block_id,
    });
    debug_stats.network_messages_out_this_second += 1;
}

/// Asks for anything newly in range (batched into one message) and drops
/// whatever fell out of range.
fn request_and_unload(
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    net: &mut network::NetworkHandle,
    player: &Player,
    now: Instant,
    graphics: &mut GraphicsState,
    debug_stats: &mut DebugStats,
) {
    let player_chunk = ChunkManager::chunk_for_world_pos(player.position.x, player.position.z);
    let (to_request, to_unload) = chunks.update(player_chunk, now);

    for batch in to_request.chunks(MAX_REQUEST_BATCH) {
        net.send(ClientMessage::RequestChunks(batch.to_vec()));
        debug_stats.network_messages_out_this_second += 1;
    }

    for pos in to_unload {
        chunks.unload(pos);
        light.unload_chunk(pos);
        graphics.drop_chunk_mesh(pos);
    }
}

/// Throttled at `player_update_hz`, and skipped entirely if nothing
/// changed -- no point spamming the server (and every nearby player's
/// snapshot) while standing still staring at a wall.
#[allow(clippy::too_many_arguments)]
fn maybe_send_transform(
    net: &mut network::NetworkHandle,
    player: &Player,
    camera: &Camera,
    now: Instant,
    interval: Duration,
    last_sent_at: &mut Instant,
    last_sent_transform: &mut Option<(Vec3, f32, f32)>,
    sequence: &mut u32,
    debug_stats: &mut DebugStats,
) {
    if now.duration_since(*last_sent_at) < interval {
        return;
    }
    let current = (player.position, camera.yaw, camera.pitch);
    if let Some(prev) = last_sent_transform {
        let moved = prev.0.distance(current.0) > 0.01;
        let turned = (prev.1 - current.1).abs() > 0.01 || (prev.2 - current.2).abs() > 0.01;
        if !moved && !turned {
            return;
        }
    }

    *sequence = sequence.wrapping_add(1);
    net.send(ClientMessage::UpdateTransform {
        x: current.0.x,
        y: current.0.y,
        z: current.0.z,
        yaw: current.1,
        pitch: current.2,
        on_ground: player.grounded,
        sequence: *sequence,
    });
    debug_stats.network_messages_out_this_second += 1;
    *last_sent_at = now;
    *last_sent_transform = Some(current);
}

#[allow(clippy::too_many_arguments)]
/// Re-arms the "wait for the ground" gate after the server has moved
/// the player somewhere.
///
/// **This is the whole of the fix for respawning underground.** The gate
/// exists because an unloaded chunk answers `None` for every cell in it,
/// and the physics reads that as air -- so a player standing where the
/// world has not arrived falls through it. Entering a world holds the
/// simulation until the nine chunks around the player are in, and that
/// has always worked.
///
/// Respawning is the same situation and was not gated. You die a long
/// way from the spawn point, the chunks there were evicted hours ago,
/// and the server's answer -- a correct position, on solid ground -- is
/// handed to a client that already believes the world is ready. Gravity
/// runs while the spawn chunks are still crossing the network: a second
/// of that is eleven blocks, and when the stone finally arrives the
/// player is inside it. `escape_solids` lifts at most 1.2 blocks, so
/// anything deeper stays buried -- and the client then reports that
/// position to the server, which believes it and saves it.
///
/// Same argument for `PositionCorrection`: the anticheat can snap a
/// player across a chunk boundary into terrain that is not loaded here.
fn respawn_gate(chunks: &ChunkManager, x: f32, z: f32, world_ready: &mut bool) {
    *world_ready = chunks.is_area_ready(ChunkManager::chunk_for_world_pos(x, z));
}

/// **Every argument is a piece of the frame's own state**, and that is
/// why there are so many of them.
///
/// This is the seam between the socket and the game: a message arrives
/// and lands in the chunk map, the light map, the inventory, the death
/// screen, the chat, the player's body. Naming them one by one is what
/// makes the borrow checker prove, at the call site, that the frame is
/// not handing the same thing to two places at once.
///
/// Bundling them into a `struct Frame<'a>` was tried and reverted: it
/// moves the same fields behind one more name, the borrows become
/// whole-struct rather than per-field, and the loop that owns them then
/// cannot touch any of them while this runs. That is a real loss of
/// checking in exchange for a shorter signature.
#[allow(clippy::too_many_arguments)]
fn drain_network(
    net: &mut network::NetworkHandle,
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    sky: &mut Sky,
    my_id: PlayerId,
    player: &mut Player,
    remote_players: &mut RemotePlayers,
    entities: &mut entities::Entities,
    arrivals: &mut Arrivals,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    versions: &mut HashMap<ChunkPos, u64>,
    debug_stats: &mut DebugStats,
    // Set when the session has ended, and why. The caller tears it down
    // and shows this on the menu.
    disconnected: &mut Option<String>,
    inventory: &mut Inventory,
    // Needed here so a snapshot can cancel a pick-up it invalidates --
    // see `InventoryScreen::sync`.
    inventory_screen: &mut inventory_screen::InventoryScreen,
    mining: &mut mining::Mining,
    health: &mut f32,
    max_health: &mut f32,
    recent_health: &mut f32,
    // How much air is left, 0..1. Only ever below 1 under water.
    breath: &mut f32,
    shake: &mut shake::Shake,
    stamina: &mut stamina::Stamina,
    // Set while the player is dead, and what killed them.
    death: &mut death::DeathScreen,
    // The chest the player has open, and what is in it.
    chest_screen: &mut chest_screen::ChestScreen,
    // The last thing the server refused, for the HUD to show.
    notice: &mut Option<(String, Instant)>,
    // Everything anyone said, including the server.
    chat: &mut chat::Chat,
    // Whether the ground under the player exists yet. Cleared by every
    // teleport, and that is not housekeeping -- see `respawn_gate`.
    world_ready: &mut bool,
) {
    use tokio::sync::mpsc::error::TryRecvError;

    loop {
        let msg = match net.to_game.try_recv() {
            Ok(msg) => msg,
            Err(TryRecvError::Empty) => break,
            // The socket task is gone: the server closed the connection
            // or the network dropped. Without this the client sat in a
            // frozen world forever, with no chunks and no explanation.
            Err(TryRecvError::Disconnected) => {
                if disconnected.is_none() {
                    *disconnected = Some("connection lost".to_string());
                }
                break;
            }
        };
        debug_stats.network_messages_in_this_second += 1;
        match msg {
            // Just queue it. Integrating a chunk means lighting it,
            // which is far too expensive to do for every chunk that
            // happens to be sitting in the socket buffer this frame.
            ServerMessage::ChunkData(chunk) => {
                // Mark it satisfied immediately, or the retry logic will
                // keep asking for a chunk that's already in the queue.
                chunks.note_arrival(chunk.pos);
                // Nothing else holds this one -- it was just built by
                // the decoder -- so taking it out of the `Arc` is a move
                // rather than another copy of the block array. The
                // fallback cannot happen and costs nothing to keep.
                arrivals.push(
                    std::sync::Arc::try_unwrap(chunk).unwrap_or_else(|shared| (*shared).clone()),
                );
            }

            ServerMessage::BlockUpdate(change) => {
                entities.on_block_placed(
                    change.global_x,
                    change.global_y,
                    change.global_z,
                    change.block_id,
                );
                apply_change(chunks, light, arrivals, urgent, dirty_set, versions, change);
            }

            ServerMessage::BlockUpdates(changes) => {
                for change in changes {
                    entities.on_block_placed(
                        change.global_x,
                        change.global_y,
                        change.global_z,
                        change.block_id,
                    );
                    apply_change(chunks, light, arrivals, urgent, dirty_set, versions, change);
                }
            }

            ServerMessage::Snapshot { states, .. } => {
                remote_players.apply_snapshot(&states, Some(my_id));
            }

            ServerMessage::Entities { states, .. } => {
                entities.apply_snapshot(&states);
            }

            ServerMessage::PlayerJoined { id, username } => {
                chat.note(&format!("{username} joined"), Instant::now());
                if id != my_id {
                    println!("[chat] {username} joined");
                    remote_players.on_join(id, username);
                }
            }

            ServerMessage::PlayerLeft { id } => {
                if let Some(name) = remote_players.name_of(id) {
                    println!("[chat] {name} left");
                    chat.note(&format!("{name} left"), Instant::now());
                }
                remote_players.remove(id);
            }

            ServerMessage::Chat { from, username, text } => {
                println!("[chat] <{username}> {text}");
                // `from: None` is the server speaking in its own name --
                // command replies, deaths, join and leave notices.
                chat.push(
                    from.is_some().then_some(username.as_str()),
                    &text,
                    Instant::now(),
                );
            }

            ServerMessage::TimeSync { time_of_day, .. } => {
                sky.on_time_sync(time_of_day);
            }

            ServerMessage::PositionCorrection { x, y, z, reason } => {
                // The server is authoritative: snap, don't argue.
                //
                // ...but say so. Being moved somewhere you did not ask to
                // go is the most alarming thing that can happen to a
                // player, and the reason was going to stderr -- a console
                // nobody running the game has open. "It teleports me at
                // walls" is a bug report that could have been "it says
                // *sustained speed above 12 b/s* when I hit a wall",
                // which is the same sentence with the answer in it.
                eprintln!("[anticheat] position corrected: {reason}");
                *notice = Some((format!("moved back: {reason}"), Instant::now()));
                player.teleport(Vec3::new(x, y, z));
                respawn_gate(chunks, x, z, world_ready);
                debug_stats.corrections_received += 1;
            }

            ServerMessage::Ping { nonce } => {
                net.send(ClientMessage::Pong { nonce });
                debug_stats.network_messages_out_this_second += 1;
            }

            ServerMessage::InventoryState { inventory: mut state } => {
                // Repaired before it is trusted: slot counts and stack
                // limits change between versions, and this arrived over
                // a wire.
                state.sanitize();
                *inventory = state;
                // A pick-up in progress is only a slot index, and this
                // may have just changed what is in it.
                inventory_screen.sync(inventory);
                chest_screen.sync_with(inventory);
            }

            ServerMessage::ChestState {
                global_x,
                global_y,
                global_z,
                inventory: mut contents,
            } => {
                contents.sanitize();
                // The answer to "open that chest" is also what opens the
                // screen: showing an empty one for a round trip would be
                // showing something a player will act on.
                // The block is passed along so the screen can call itself
                // a backpack rather than a chest -- the message says
                // where, and only the world says what.
                chest_screen.show(
                    (global_x, global_y, global_z),
                    contents,
                    chunks.block_at(global_x, global_y, global_z),
                );
                chest_screen.sync_with(inventory);
            }

            ServerMessage::ChestClosed => {
                // Broken, or walked away from. Either way there is
                // nothing to look at any more.
                chest_screen.close();
            }

            ServerMessage::Breath { fraction } => {
                *breath = fraction.clamp(0.0, 1.0);
            }

            ServerMessage::Health { current, max } => {
                *max_health = max;
                if current < *health {
                    // Took a hit. The bar keeps the old value as the
                    // ghost that drains away, and the view is kicked in
                    // proportion -- both exist so damage is noticed
                    // rather than merely recorded.
                    *recent_health = recent_health.max(*health);
                    shake.on_damage(*health - current);
                } else {
                    // Healing has no ghost to leave behind.
                    *recent_health = current;
                }
                *health = current;
            }

            ServerMessage::Died { cause } => {
                println!("[survival] you died: {cause}");
                death.open(cause);
                // Whatever was half-mined is not half-mined any more.
                mining.reset();
            }

            ServerMessage::Respawned { x, y, z } => {
                player.teleport(Vec3::new(x, y, z));
                respawn_gate(chunks, x, z, world_ready);
                death.close();
                mining.reset();
                // Coming back winded would mean dying, respawning and
                // immediately being unable to run away from whatever it
                // was.
                stamina.reset();
                // ...and coming back holding your breath would mean a
                // meter on screen saying so. The server sends the reading
                // when it changes, but an outgoing queue can drop a
                // message under load, and the one message this would
                // lose is the one that clears the bar.
                *breath = 1.0;
            }

            // Neither of these is a reason to close the game. They
            // used to be: being kicked from a server, or refused by one,
            // shut the whole client down with the explanation going to a
            // console nobody had open.
            ServerMessage::Kick(reason) => {
                *disconnected = Some(format!("disconnected: {reason}"));
            }

            ServerMessage::Rejected(reason) => {
                *disconnected = Some(format!("refused by the server: {reason}"));
            }

            ServerMessage::Welcome { .. } => {
                // Already consumed during the handshake; a second one is
                // a server bug, not something to act on.
            }

            // On screen as well as in the log: an error the player
            // caused (nothing in hand, a recipe they cannot make) is a
            // reply to what they just did, and stderr is not where they
            // are looking.
            ServerMessage::Error(e) => {
                eprintln!("server error: {e}");
                *notice = Some((e, Instant::now()));
            }
        }
    }
}

/// Applies a confirmed block change. Everything it dirties goes on the
/// **urgent** queue: a block edit is something the player is looking at
/// right now, and the sand simulation's updates are visible motion.
/// The un-integrated chunk queue, plus an index of which positions are
/// in it.
///
/// The index exists for `apply_change`: every block update asks "is this
/// edit for a chunk still waiting in the queue?", and the answer is
/// almost always no. Asking with a scan of the whole queue was
/// O(changes × arrivals) exactly when both are large -- terrain
/// streaming in while the sand and water simulations are running -- so
/// the common no is now a hash probe.
#[derive(Default)]
struct Arrivals {
    queue: VecDeque<primitive_shared::types::Chunk>,
    /// How many queued chunks sit at each position -- a count rather
    /// than a set because a re-requested chunk can be queued twice.
    index: HashMap<ChunkPos, u32>,
}

impl Arrivals {
    fn push(&mut self, chunk: primitive_shared::types::Chunk) {
        *self.index.entry(chunk.pos).or_insert(0) += 1;
        self.queue.push_back(chunk);
    }

    fn pop(&mut self) -> Option<primitive_shared::types::Chunk> {
        let chunk = self.queue.pop_front()?;
        if let Some(count) = self.index.get_mut(&chunk.pos) {
            *count -= 1;
            if *count == 0 {
                self.index.remove(&chunk.pos);
            }
        }
        Some(chunk)
    }

    /// The queued chunk at `pos`, if any -- the probe is the fast path,
    /// the scan behind it runs only on a hit.
    fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut primitive_shared::types::Chunk> {
        if !self.index.contains_key(&pos) {
            return None;
        }
        self.queue.iter_mut().find(|c| c.pos == pos)
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.index.clear();
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_change(
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    arrivals: &mut Arrivals,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    versions: &mut HashMap<ChunkPos, u64>,
    change: primitive_shared::protocol::BlockChange,
) {
    // The edit may land on a chunk that has arrived but hasn't been
    // integrated yet. Applying it to the queued copy keeps it -- without
    // this, edits made during a burst of chunk loading would silently
    // vanish, which is exactly the kind of desync that's miserable to
    // debug later.
    if change.global_y >= 0 && (change.global_y as usize) < primitive_shared::types::CHUNK_SIZE_Y {
        let (pos, lx, lz) = ChunkPos::from_global(change.global_x, change.global_z);
        if let Some(pending) = arrivals.get_mut(pos) {
            pending.set(lx, change.global_y as usize, lz, change.block_id);
            return;
        }
    }

    let Some(pos) = chunks.apply_block_update(
        change.global_x,
        change.global_y,
        change.global_z,
        change.block_id,
    ) else {
        return; // not loaded, or nothing actually changed
    };
    bump_version(versions, pos);
    mark_urgent(urgent, dirty_set, pos);

    // Incremental relight: only the cells this edit actually reaches are
    // recomputed, and only the chunks whose light changed come back.
    for changed in light.set_block(
        &*chunks,
        change.global_x,
        change.global_y,
        change.global_z,
        change.block_id,
    ) {
        mark_urgent(urgent, dirty_set, changed);
    }

    // An edit on a chunk border changes the neighbour's face culling and
    // lighting too -- otherwise breaking a block at the seam leaves a
    // hole you can see straight through into the void.
    let local_x = change.global_x.rem_euclid(16);
    let local_z = change.global_z.rem_euclid(16);
    let dx = if local_x == 0 {
        -1
    } else if local_x == 15 {
        1
    } else {
        0
    };
    let dz = if local_z == 0 {
        -1
    } else if local_z == 15 {
        1
    } else {
        0
    };
    for (ox, oz) in [(dx, 0), (0, dz), (dx, dz)] {
        if ox == 0 && oz == 0 {
            continue;
        }
        let neighbour = ChunkPos::new(pos.x + ox, pos.z + oz);
        if chunks.is_loaded(neighbour) {
            bump_version(versions, neighbour);
            mark_urgent(urgent, dirty_set, neighbour);
        }
    }
}

/// Which chunks are waiting to be meshed, and which of them are waiting
/// on the *urgent* queue.
///
/// A set would do for the first question, and that is what this was.
/// The second question is the one that cost: promoting a chunk that is
/// already queued has to know whether it is already in `urgent`, and
/// asking a `VecDeque` costs a scan of it. That is fine at the handful
/// of edits a player makes by hand and stops being fine the moment
/// something in the world edits blocks on its own -- a tick of flowing
/// water is dozens of changes across a dozen chunks, each one scanning
/// a queue the others are filling. Quadratic, in the frame loop, and
/// invisible until there is a flood on screen.
///
/// So the answer is carried instead of searched for: `true` means "this
/// position is in the urgent deque". The invariant is exactly that, and
/// it holds because the flag is set only when something is pushed and
/// cleared only by removing the entry outright.
type MeshQueueSet = HashMap<ChunkPos, bool>;

fn mark_dirty(dirty: &mut VecDeque<ChunkPos>, dirty_set: &mut MeshQueueSet, pos: ChunkPos) {
    if let std::collections::hash_map::Entry::Vacant(slot) = dirty_set.entry(pos) {
        slot.insert(false);
        dirty.push_back(pos);
    }
}

/// Bumps a chunk's version. Anything already being meshed for it is now
/// stale and will be discarded when it comes back.
fn bump_version(versions: &mut HashMap<ChunkPos, u64>, pos: ChunkPos) {
    *versions.entry(pos).or_insert(0) += 1;
}

/// Same, but for chunks the player just edited: they go on the urgent
/// queue, which is drained first.
fn mark_urgent(
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    pos: ChunkPos,
) {
    // Already on the urgent queue: nothing to do. Otherwise it goes on,
    // whether it was queued as ordinary work (promoted -- the stale
    // entry in `dirty` is skipped when it comes up, because the set no
    // longer holds the position by then) or not queued at all.
    let already_urgent = dirty_set.entry(pos).or_insert(false);
    if !*already_urgent {
        *already_urgent = true;
        urgent.push_back(pos);
    }
}

/// Fills neighbourhoods for dirty chunks and hands them to the mesher
/// threads.
///
/// The main thread's share of meshing is now just this fill (a few
/// hundred lookups per chunk), so the budget it spends is small and
/// predictable -- which is the point: the old version could not bound
/// its own cost, because a single chunk's mesh exceeded the whole
/// budget and the check only happened between chunks.
///
/// The urgent queue goes first: a chunk the player just edited must not
/// wait behind terrain streaming.
#[allow(clippy::too_many_arguments)]
fn dispatch_meshing(
    urgent: &mut VecDeque<ChunkPos>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    versions: &HashMap<ChunkPos, u64>,
    mesher: &mut mesher::Mesher,
    chunks: &ChunkManager,
    light: &LightMap,
    budget_ms: f32,
    debug_stats: &mut DebugStats,
) {
    let started = Instant::now();
    let budget = Duration::from_secs_f32(budget_ms / 1000.0);
    // Don't run far ahead of the workers: queued work that's already
    // stale by the time it's meshed is wasted, and the pooled buffers
    // are finite.
    let max_in_flight = mesher.workers() * 2;

    loop {
        if mesher.in_flight() >= max_in_flight || started.elapsed() >= budget {
            break;
        }
        let Some(pos) = urgent.pop_front().or_else(|| dirty.pop_front()) else {
            break;
        };
        // A position can sit in both queues after being promoted; the
        // set is the authority on whether it still needs work.
        //
        // Taking the entry out is also what clears its urgent flag, and
        // the two cannot get out of step: `dirty` is only ever popped
        // once `urgent` is empty, so nothing popped from it can still be
        // flagged as waiting there.
        if dirty_set.remove(&pos).is_none() {
            continue;
        }
        if chunks.get(pos).is_none() {
            continue; // unloaded while queued
        }

        let version = versions.get(&pos).copied().unwrap_or(0);
        let mut cache = mesher.take_cache();
        cache.fill(pos, chunks, light);
        mesher.submit(pos, version, cache);
        debug_stats.chunks_meshed_this_second += 1;
    }

    debug_stats.mesh_time_ms_this_second += started.elapsed().as_secs_f32() * 1000.0;
}

/// Uploads whatever the mesher threads finished and returns their
/// buffers to the pool.
#[allow(clippy::too_many_arguments)]
fn collect_worker_results(
    mesher: &mut mesher::Mesher,
    graphics: &mut GraphicsState,
    chunks: &ChunkManager,
    light: &mut LightMap,
    urgent: &mut VecDeque<ChunkPos>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    versions: &HashMap<ChunkPos, u64>,
    player_chunk: ChunkPos,
    debug_stats: &mut DebugStats,
) {
    let started = Instant::now();
    // Chunks whose lighting just landed, plus their neighbours. Each is
    // queued for meshing only once its own neighbourhood has settled --
    // see `ChunkManager::neighbourhood_settled`.
    let mut newly_lit: Vec<ChunkPos> = Vec::new();

    for finished in mesher.collect() {
        match finished {
            mesher::Finished::Mesh {
                pos,
                version,
                buffers,
                cache,
            } => {
                // Drop a mesh the world has moved past, and queue the
                // chunk again so it gets a fresh one. Without this a
                // slower worker's stale result can overwrite a newer
                // mesh and leave holes where a block was just broken.
                let current = versions.get(&pos).copied().unwrap_or(0);
                if version != current {
                    debug_stats.stale_meshes_discarded += 1;
                    mark_urgent(urgent, dirty_set, pos);
                    mesher.recycle(cache, buffers);
                    continue;
                }

                if buffers.indices.is_empty() {
                    // An all-air chunk still has to drop any stale mesh,
                    // or the blocks you just mined stay on screen.
                    graphics.drop_chunk_mesh(pos);
                } else {
                    graphics.set_chunk_mesh(pos, &buffers);
                }
                mesher.recycle(cache, buffers);
            }

            mesher::Finished::Light { pos, data } => {
                // The worker did the isolated pass; the seam
                // reconciliation touches the shared map and stays here.
                if chunks.is_loaded(pos) {
                    for changed in light.insert_precomputed(chunks, pos, data) {
                        newly_lit.push(changed);
                    }
                    newly_lit.push(pos);
                    // This chunk's arrival may be the last thing its
                    // neighbours were waiting for.
                    for (dx, dz) in NEIGHBOUR_OFFSETS {
                        newly_lit.push(ChunkPos::new(pos.x + dx, pos.z + dz));
                    }
                    debug_stats.chunks_integrated_this_second += 1;
                }
                let _ = &urgent;
            }
        }
    }

    // Queue meshing only for chunks whose neighbourhood has settled.
    // Meshing a chunk before its neighbours arrive means meshing it
    // again for each one -- which is what made the frame rate sag while
    // terrain streamed in.
    for pos in newly_lit {
        if chunks.is_loaded(pos)
            && light.is_lit(pos)
            && chunks.neighbourhood_settled(pos, player_chunk)
        {
            mark_dirty(dirty, dirty_set, pos);
        }
    }

    debug_stats.upload_time_ms_this_second += started.elapsed().as_secs_f32() * 1000.0;
}

/// Moves arrived chunks into the world, within a per-frame time budget.
///
/// **This is the fix for the freezes.** Receiving a chunk is cheap;
/// integrating one means computing its lighting, which walks 256 columns
/// and flood-fills from every seam and light source. The server streams
/// several chunks per tick, so after any brief stall a whole burst was
/// sitting in the channel and got integrated in a single frame -- tens
/// to hundreds of milliseconds of the game simply not responding.
///
/// Now arrival and integration are separate: the socket is drained
/// eagerly (so the server never sees a stalled reader), and only this
/// step is rationed. The world streams in a few chunks slower; the frame
/// rate stops collapsing.
#[allow(clippy::too_many_arguments)]
fn integrate_chunks(
    arrivals: &mut Arrivals,
    chunks: &mut ChunkManager,
    mesher: &mut mesher::Mesher,
    budget_ms: f32,
    debug_stats: &mut DebugStats,
) {
    let started = Instant::now();
    let budget = Duration::from_secs_f32(budget_ms / 1000.0);

    while let Some(chunk) = arrivals.pop() {
        let pos = chunk.pos;

        // Copy the blocks straight out of the arriving chunk, *before*
        // handing it to the world.
        //
        // The obvious-looking alternative -- read them back out of the
        // ChunkManager -- costs a hash lookup per cell, 16,384 of them
        // per chunk. That measured at ~19 ms per chunk and was the whole
        // of the remaining frame-rate sag while terrain streamed in.
        // The chunk goes in and comes back shared, so the worker gets
        // the same blocks rather than a 32 KB copy of them.
        let shared = chunks.insert(chunk);

        // The pure, expensive half of lighting goes to a worker; the
        // seam reconciliation happens in `collect_worker_results`.
        mesher.submit_lighting(pos, shared);

        if started.elapsed() >= budget {
            break;
        }
    }

    debug_stats.chunk_time_ms_this_second += started.elapsed().as_secs_f32() * 1000.0;
}

#[cfg(test)]
mod ui_key_tests {
    use super::*;

    fn game_key() -> UiKey {
        UiKey {
            in_game: true,
            aspect: 1.78f32.to_bits(),
            health: 20.0f32.to_bits(),
            max_health: 20.0f32.to_bits(),
            recent_health: 20.0f32.to_bits(),
            stamina: 1.0f32.to_bits(),
            breath: 1.0f32.to_bits(),
            inventory: inventory_fingerprint(&Inventory::new()),
            ..UiKey::default()
        }
    }

    #[test]
    fn the_same_frame_twice_compares_equal() {
        // The whole mechanism: no change, no rebuild.
        assert!(game_key() == game_key());
    }

    #[test]
    fn a_hit_changes_the_key() {
        let hurt = UiKey {
            health: 14.0f32.to_bits(),
            ..game_key()
        };
        assert!(game_key() != hurt, "losing health was invisible to the key");
    }

    #[test]
    fn picking_something_up_changes_the_key() {
        let mut carrying = Inventory::new();
        carrying.add(primitive_shared::types::BLOCK_STONE, 3);
        let key = UiKey {
            inventory: inventory_fingerprint(&carrying),
            ..game_key()
        };
        assert!(game_key() != key, "a changed pack was invisible to the key");
    }

    #[test]
    fn the_menu_path_and_the_game_path_never_collide() {
        // Leaving a world swaps which path builds the interface. The two
        // must not compare equal whatever their fields hold, or the
        // first menu frame would show the world's hotbar.
        let menu = UiKey::menu_only(0, 1.78);
        let mut game = game_key();
        game.menu = Some(0);
        assert!(menu != game);
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::streaming_budget;

    #[test]
    fn a_slow_frame_gets_the_whole_configured_budget() {
        // At 60 Hz the configured 3 ms is under the 30% share, so
        // nothing is taken away: the cap is there for fast machines.
        assert_eq!(streaming_budget(3.0, 1.0 / 60.0, true), 3.0);
    }

    #[test]
    fn a_fast_frame_is_not_half_spent_on_terrain() {
        // 200 fps: a 5 ms frame must not hand 3 ms to streaming, or the
        // frame rate is decided by how much terrain happens to be
        // arriving.
        let budget = streaming_budget(3.0, 1.0 / 200.0, true);
        assert!((0.5..3.0).contains(&budget), "{budget}");
        assert!(budget <= 5.0 * 0.31, "{budget} ms is a third of a 5 ms frame");
    }

    #[test]
    fn streaming_never_stops_entirely() {
        // However fast the frames, some progress has to be made, or the
        // world never finishes arriving.
        assert!(streaming_budget(3.0, 1.0 / 2000.0, true) >= 0.5);
    }

    #[test]
    fn loading_a_world_uses_the_full_budget() {
        // Nothing to be smooth for yet, and the player is waiting.
        assert_eq!(streaming_budget(4.0, 1.0 / 300.0, false), 4.0);
    }
}

#[cfg(test)]
mod respawn_gate_tests {
    use super::respawn_gate;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::types::{Chunk, ChunkPos, BLOCK_STONE, CHUNK_VOLUME};

    fn world_around(centre: ChunkPos) -> ChunkManager {
        let mut chunks = ChunkManager::new(8);
        for dx in -1..=1 {
            for dz in -1..=1 {
                chunks.insert(Chunk {
                    pos: ChunkPos::new(centre.x + dx, centre.z + dz),
                    blocks: vec![BLOCK_STONE; CHUNK_VOLUME],
                });
            }
        }
        chunks
    }

    #[test]
    fn respawning_into_unloaded_ground_makes_the_player_wait_for_it() {
        // **The underground-respawn bug.** Dying far from the spawn
        // point sends you back to chunks that were evicted long ago, and
        // an unloaded chunk reads as air: the physics would run, gravity
        // would take a second or two to bury the player several blocks
        // into the terrain that had not arrived yet, and `escape_solids`
        // only lifts 1.2 of them back out.
        let mut ready = true;
        let chunks = world_around(ChunkPos::new(0, 0));
        // Spawn is a hundred chunks away, where nothing is loaded.
        respawn_gate(&chunks, 1600.0, 1600.0, &mut ready);
        assert!(!ready, "physics would have run over ground that is not there");
    }

    #[test]
    fn respawning_where_the_ground_already_is_does_not_stall_the_player() {
        // The other half: re-arming unconditionally would put a loading
        // screen in front of every death near home, and every anticheat
        // correction, for a world that is already under the player.
        let mut ready = false;
        let chunks = world_around(ChunkPos::new(0, 0));
        respawn_gate(&chunks, 8.0, 8.0, &mut ready);
        assert!(ready, "the ground is loaded and the player was made to wait");
    }

    #[test]
    fn a_hole_in_the_neighbourhood_still_counts_as_not_ready() {
        // The centre chunk alone is not enough -- one step sideways off
        // the spawn block is an unloaded chunk, and the same fall.
        let mut chunks = ChunkManager::new(8);
        chunks.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks: vec![BLOCK_STONE; CHUNK_VOLUME],
        });
        let mut ready = true;
        respawn_gate(&chunks, 8.0, 8.0, &mut ready);
        assert!(!ready);
    }
}

#[cfg(test)]
mod meshing_priority_tests {
    use super::*;
    use primitive_shared::types::{Chunk, CHUNK_VOLUME};

    fn world_with_one_chunk(pos: ChunkPos) -> (ChunkManager, LightMap) {
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        });
        let mut light = LightMap::new();
        light.load_chunk(&chunks, pos);
        (chunks, light)
    }

    /// The fix for "breaking a block leaves a hole for a second": an
    /// edited chunk must be meshed before a backlog of streamed terrain,
    /// not after it.
    #[test]
    fn an_edited_chunk_is_meshed_before_a_backlog_of_streamed_terrain() {
        let edited = ChunkPos::new(0, 0);
        let (mut chunks, light) = world_with_one_chunk(edited);

        // A long queue of ordinary streaming work, all of it loaded so
        // none of it gets skipped.
        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        for i in 1..60 {
            let pos = ChunkPos::new(i, 0);
            chunks.insert(Chunk {
                pos,
                blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
            });
            mark_dirty(&mut dirty, &mut dirty_set, pos);
        }

        // Then the player breaks a block.
        let mut urgent = VecDeque::new();
        mark_urgent(&mut urgent, &mut dirty_set, edited);

        let mut mesher = mesher::Mesher::new(crate::engine::texture::FaceLayers::empty_for_test(), 2);
        let mut stats = DebugStats::default();
        dispatch_meshing(
            &mut urgent,
            &mut dirty,
            &mut dirty_set,
            &HashMap::new(),
            &mut mesher,
            &chunks,
            &light,
            8.0,
            &mut stats,
        );

        // Whatever else got dispatched, the edited chunk must be among
        // the first results rather than 60 chunks later.
        let mut seen = Vec::new();
        for _ in 0..300 {
            for finished in mesher.collect() {
                if let mesher::Finished::Mesh {
                    pos, cache, buffers, ..
                } = finished
                {
                    seen.push(pos);
                    mesher.recycle(cache, buffers);
                }
            }
            if !seen.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            seen.first(),
            Some(&edited),
            "the edited chunk should be meshed first, got {seen:?}"
        );
    }

    #[test]
    fn a_chunk_queued_twice_is_only_meshed_once() {
        let pos = ChunkPos::new(0, 0);
        let (chunks, light) = world_with_one_chunk(pos);

        let mut dirty = VecDeque::new();
        let mut urgent = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        // Marked as ordinary work, then promoted by an edit: it now sits
        // in both queues, but must not be meshed twice.
        mark_dirty(&mut dirty, &mut dirty_set, pos);
        mark_urgent(&mut urgent, &mut dirty_set, pos);

        let mut mesher = mesher::Mesher::new(crate::engine::texture::FaceLayers::empty_for_test(), 2);
        let mut stats = DebugStats::default();
        dispatch_meshing(
            &mut urgent,
            &mut dirty,
            &mut dirty_set,
            &HashMap::new(),
            &mut mesher,
            &chunks,
            &light,
            8.0,
            &mut stats,
        );
        assert_eq!(mesher.in_flight(), 1, "duplicate queue entry caused extra work");
        assert!(dirty_set.is_empty());
    }

    #[test]
    fn unloaded_chunks_are_dropped_from_the_queue() {
        let (chunks, light) = world_with_one_chunk(ChunkPos::new(0, 0));
        let mut dirty = VecDeque::new();
        let mut urgent = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        mark_dirty(&mut dirty, &mut dirty_set, ChunkPos::new(99, 99));

        let mut mesher = mesher::Mesher::new(crate::engine::texture::FaceLayers::empty_for_test(), 2);
        let mut stats = DebugStats::default();
        dispatch_meshing(
            &mut urgent,
            &mut dirty,
            &mut dirty_set,
            &HashMap::new(),
            &mut mesher,
            &chunks,
            &light,
            8.0,
            &mut stats,
        );
        assert_eq!(mesher.in_flight(), 0, "meshed a chunk that isn't loaded");
    }
}

/// What the menus need in order to draw a frame.
///
/// `paused` decides the backdrop: with a world behind the screen the
/// wallpaper is both pointless and worse than what it would cover.
fn menu_context<'a>(
    settings: &'a ClientSettings,
    worlds: &'a worlds::Worlds,
    graphics: &GraphicsState,
    paused: bool,
) -> menu::MenuContext<'a> {
    let background = if settings.menu_background && !paused {
        let block = settings.menu_background_block();
        Some((
            graphics
                .textures
                .layer_for_face(block, texture::FACE_SOUTH),
            graphics.aspect(),
        ))
    } else {
        None
    };
    menu::MenuContext {
        version: VERSION,
        font: graphics.textures.font,
        settings,
        worlds,
        background,
    }
}

/// Pushes settings that can change mid-session into the things that
/// hold a copy of them.
///
/// Called every time a setting is stepped, so the effect is visible
/// while the player is still looking at the row that caused it. That is
/// the whole reason these live in the game rather than in a file: a
/// field-of-view slider you have to restart to judge is a number, not a
/// setting.
///
/// Render distance is deliberately absent: it is a property of the
/// session (the server caps it in `Welcome`), so it applies to the next
/// world rather than to this one.
#[allow(clippy::too_many_arguments)]
fn apply_settings(
    settings: &ClientSettings,
    graphics: &mut GraphicsState,
    camera: &mut Camera,
    chunks: &mut ChunkManager,
    render_distance: &mut i32,
    server_cap: i32,
    fog_enabled: &mut bool,
) {
    camera.fov_y_radians = settings.fov_degrees.to_radians();
    graphics.set_vsync(settings.vsync);
    graphics.set_anisotropy(settings.anisotropy);
    graphics.set_sky_scale(settings.sky_scale);
    *fog_enabled = settings.fog_enabled;

    // Still capped by whatever the server said it would stream in
    // `Welcome`: asking for more than that gets the request ignored and
    // the client flagged, so the number the player sets is a request,
    // not a promise.
    *render_distance = settings.render_distance_chunks.min(server_cap.max(1));
    chunks.set_render_distance(*render_distance);
}

/// What a "retry" should retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attempt {
    None,
    Singleplayer(usize),
    Server(usize),
}

/// Starts a connection attempt on the tokio runtime and hands back a
/// channel for its result.
///
/// It has to be off the main thread: the event loop must keep drawing
/// (and stay responsive) while a server that isn't there takes ten
/// seconds to time out.
fn spawn_connect(
    runtime: &tokio::runtime::Runtime,
    address: String,
    username: String,
) -> tokio::sync::oneshot::Receiver<Result<Session, String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    runtime.spawn(async move {
        let result = network::connect(&address, &username)
            .await
            .map(|connection| Session {
                connection,
                local_server: None,
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx
}

/// Starts a singleplayer world: a local server, then a connection to it.
///
/// Both halves happen in one task so the client only ever sees a
/// finished session or an error. Splitting them would leave a state
/// where a server exists but nothing is connected to it, and every path
/// out of that state has to remember to shut it down.
///
/// If the connection fails the server is stopped here, rather than left
/// running with its port held for the rest of the process.
fn spawn_singleplayer(
    runtime: &tokio::runtime::Runtime,
    settings: &ClientSettings,
    world: worlds::World,
) -> tokio::sync::oneshot::Receiver<Result<Session, String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let server_settings = settings.singleplayer_server(&world);
    let username = settings.username.clone();
    runtime.spawn(async move {
        let result = async {
            let server = primitive_server::start(
                server_settings,
                primitive_server::RunOptions::embedded(),
            )
            .await
            .map_err(|e| format!("could not start the local world: {e}"))?;

            let address = server.address().to_string();
            match network::connect(&address, &username).await {
                Ok(connection) => Ok(Session {
                    connection,
                    local_server: Some(server),
                }),
                Err(e) => {
                    server.stop().await;
                    Err(format!("the local world started but refused us: {e}"))
                }
            }
        }
        .await;
        let _ = tx.send(result);
    });
    rx
}

/// Translates a physical key into the menu's own key vocabulary.
///
/// Letter shortcuts come from `text` rather than the key code, so they
/// follow the player's keyboard layout: `KeyCode::KeyA` is the physical
/// key marked A on QWERTY and the one marked Q on AZERTY.
fn menu_key(code: KeyCode, text: Option<&str>) -> Option<menu::Key> {
    match code {
        KeyCode::ArrowUp => Some(menu::Key::Up),
        KeyCode::ArrowDown => Some(menu::Key::Down),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(menu::Key::Enter),
        KeyCode::Escape => Some(menu::Key::Escape),
        KeyCode::Tab => Some(menu::Key::Tab),
        KeyCode::Backspace => Some(menu::Key::Backspace),
        KeyCode::Delete => Some(menu::Key::Delete),
        _ => text
            .and_then(|t| t.chars().next())
            .filter(|c| c.is_ascii_graphic())
            .map(menu::Key::Char),
    }
}

/// The F3 panel: a translucent card in the top-left corner with one line
/// of statistics per row.
///
/// Top-left because that is the one part of the screen nothing else uses
/// -- the crosshair is centred, the hotbar is at the bottom -- so the
/// panel never covers something the player is aiming at.
fn debug_panel_into(
    lines: &[String],
    aspect: f32,
    font: crate::engine::texture::FontAtlas,
    out: &mut Vec<hotbar::HotbarVertex>,
) {
    const SCALE: f32 = 0.85;
    const PAD: f32 = 0.02;

    let mut painter = widgets::Painter::onto(font, std::mem::take(out));
    let widest = lines
        .iter()
        .map(|line| widgets::measure(line, SCALE))
        .fold(0.0f32, f32::max);
    let height = lines.len() as f32 * widgets::line_height(SCALE);

    // Anchored to the window's actual left edge, which needs the aspect
    // ratio: UI x runs from -aspect to +aspect. It used to be a constant
    // -1.68, which is the left edge of a 16:9 window and *off-screen* on
    // anything narrower -- on a 4:3 window the panel hung past the edge
    // and the first characters of every line were cut off.
    let left = -aspect.max(0.1) + PAD * 2.0;
    let top = 0.94;
    let panel = widgets::Rect::new(left - PAD, top - height - PAD, left + widest + PAD, top + PAD);
    painter.quad(panel, [0.03, 0.04, 0.06, 0.72]);
    painter.border(panel, 0.003, widgets::PANEL_EDGE);

    let mut y = top;
    for line in lines {
        painter.text(line, left, y, SCALE, widgets::TEXT);
        y -= widgets::line_height(SCALE);
    }
    *out = painter.into_vertices();
}
