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

mod camera;
mod chunk_manager;
mod crash;
mod debug;
mod embedded;
mod entities;
mod font;
mod frustum;
mod hotbar;
mod input;
mod menu;
mod mesh;
mod mesher;
mod network;
mod physics;
mod remote_players;
mod renderer;
mod settings;
mod sky;
mod texture;
mod ui;
mod worlds;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, WindowBuilder};

use camera::Camera;
use chunk_manager::{ChunkManager, NEIGHBOUR_OFFSETS};
use debug::{DebugStats, FrameInfo};
use menu::{Action, Menu, Screen};
use primitive_shared::geometry::block_overlaps_player;
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{ClientMessage, PlayerId, ServerMessage};
use primitive_shared::types::{block_name, ChunkPos, BLOCK_AIR};
use physics::Player;
use remote_players::RemotePlayers;
use renderer::{FrameParams, GpuMesh, GraphicsState};
use settings::ClientSettings;
use sky::Sky;

const INTERACT_RANGE: f32 = 6.0;
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
            .build(&event_loop)?,
    );

    println!("assets dir: {}", assets_dir.display());
    // The likeliest failure a player will ever hit, and the one worth
    // naming: no usable GPU, or drivers too old for the backend.
    let mut graphics = pollster::block_on(GraphicsState::new(
        window.clone(),
        &assets_dir,
        settings.vsync,
    ))
    .map_err(|e| anyhow::anyhow!("graphics could not start: {e}"))?;
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
    let mut meshes: HashMap<ChunkPos, GpuMesh> = HashMap::new();
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
    let mut dirty_set: HashSet<ChunkPos> = HashSet::new();
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
    let mut arrivals: VecDeque<primitive_shared::types::Chunk> = VecDeque::new();

    let mut remote_players = RemotePlayers::default();
    let mut entities = entities::Entities::default();
    let face_layers = graphics.textures.face_layers();
    let mut my_id: PlayerId = 0;
    let mut world_seed: u32 = 0;

    let mut sky = Sky::new(0.3, 900.0);

    let mut player = Player::new(Vec3::new(0.5, 40.0, 0.5), settings.move_speed);
    let mut camera = Camera::new(player.eye_position(), graphics.aspect());
    camera.fov_y_radians = settings.fov_degrees.to_radians();

    // Rebuilt every frame because they move every frame; the storage
    // for them is not.
    let mut entity_mesh = graphics.new_dynamic_mesh();
    let mut actor_mesh = graphics.new_dynamic_mesh();
    let mut entity_vertices: Vec<mesh::Vertex> = Vec::new();
    let mut entity_indices: Vec<u32> = Vec::new();
    let mut actor_vertices: Vec<remote_players::ActorVertex> = Vec::new();
    let mut actor_indices: Vec<u32> = Vec::new();

    let mut input = input::InputState::default();
    let mut last_frame = Instant::now();
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
    // Set when the server ends the session under us -- a kick, a
    // shutdown, a dropped connection. Handled after the frame's
    // borrows have ended.
    let mut end_session: Option<String> = None;

    println!(
        "controls: WASD move | Space jump | mouse look (click to grab) | Esc pause | \
         LMB break | RMB place | 1-9 or wheel pick block | F fog | F3 stats"
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
                                menu.notice = Some((format!("created {name}"), true));
                            }
                            Err(reason) => {
                                // Stay on the form: the player has just
                                // typed something and needs to see why
                                // it was refused, next to what they
                                // typed.
                                menu.notice = Some((reason, false));
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
                        Ok(name) => menu.notice = Some((format!("deleted {name}"), true)),
                        Err(reason) => menu.notice = Some((reason, false)),
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
                                    menu.notice = Some((e, false));
                                }
                            }
                        }
                        abandon_pending!()
                    }
                    Action::Resume => {
                        paused = false;
                        grab_cursor(&window, &mut input);
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
                    // Only meaningful when a menu is up: during play the
                    // cursor is grabbed and motion arrives as a
                    // `DeviceEvent` instead.
                    if net.is_none() || paused {
                        let size = graphics.size;
                        menu.set_cursor(Some(ui::cursor_to_ui(
                            (position.x, position.y),
                            (size.width, size.height),
                        )));
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
                        // Text first: a character typed into a field must
                        // not also be read as a shortcut.
                        if menu.accepts_text() {
                            if let Some(text) = event.text.as_ref() {
                                let mut typed = false;
                                for c in text.chars() {
                                    if c.is_ascii_graphic() || c == ' ' {
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

                    if let PhysicalKey::Code(code) = event.physical_key {
                        if is_pressed {
                            match code {
                                KeyCode::Escape => {
                                    // Esc pauses rather than merely
                                    // releasing the cursor: releasing it
                                    // silently left players with no way
                                    // to leave a world short of closing
                                    // the window.
                                    paused = true;
                                    menu.open(Screen::Paused);
                                    release_cursor(&window, &mut input);
                                    input.release_all();
                                }
                                KeyCode::F3 => debug_stats.toggle_console(),
                                KeyCode::KeyF => {
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
                    if net.is_none() || paused {
                        return;
                    }
                    let forward = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                        MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                    };
                    input.cycle_hotbar(forward);
                }

                WindowEvent::MouseInput { state, button, .. } => {
                    if state != ElementState::Pressed {
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
                    } else {
                        let others: Vec<Vec3> = remote_players.iter_positions().collect();
                        if let Some(net) = net.as_mut() {
                            handle_click(
                                button,
                                &chunks,
                                &camera,
                                &input,
                                &player,
                                &others,
                                net,
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
                        paused = false;
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
                                    meshes.clear();
                                    dirty.clear();
                                    urgent.clear();
                                    dirty_set.clear();
                                    arrivals.clear();
                                    chunk_versions.clear();
                                    remote_players = RemotePlayers::default();
                                    entities = entities::Entities::default();
                                    my_id = welcome.your_id;
                                    world_seed = welcome.world_seed;
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

                        window.set_title(&format!("Primitive {VERSION}"));
                        let ui = menu.build(&menu_context(&settings, &worlds, &graphics, false));
                        let params = frame_params(&settings, &sky, render_distance, false, false);
                        match graphics.render(
                            &camera,
                            &params,
                            &HashMap::new(),
                            None,
                            None,
                            None,
                            &ui,
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
                    );

                    // The session ended without the player asking. Tear
                    // it down here, at the top of the frame, rather than
                    // rendering a world that is no longer connected to
                    // anything.
                    if let Some(reason) = disconnected {
                        eprintln!("{reason}");
                        end_session = Some(reason);
                    }

                    integrate_chunks(
                        &mut arrivals,
                        &mut chunks,
                        &mut mesher,
                        settings.chunk_budget_ms,
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
                        settings.mesh_budget_ms,
                        &mut debug_stats,
                    );
                    collect_worker_results(
                        &mut mesher,
                        &graphics,
                        &chunks,
                        &mut light,
                        &mut meshes,
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
                    let other_positions: Vec<Vec3> = remote_players.iter_positions().collect();

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
                        let wish_dir = if paused {
                            Vec3::ZERO
                        } else {
                            wish_direction(&input, &camera)
                        };
                        player.update(
                            &chunks,
                            &other_positions,
                            wish_dir,
                            !paused && input.jump_pressed_this_frame,
                            !paused && input.is_down(KeyCode::Space),
                            dt,
                        );
                    }
                    camera.position = player.eye_position();

                    request_and_unload(
                        &mut chunks,
                        &mut light,
                        net,
                        &player,
                        now,
                        &mut meshes,
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

                    input.end_frame();

                    // Decided by the head, not the feet: standing
                    // waist-deep in a lake shouldn't tint the screen.
                    // Physics already samples this, so there's one
                    // definition of "under water" rather than two.
                    let underwater = player.submerged;

                    let params =
                        frame_params(&settings, &sky, render_distance, fog_enabled, underwater);
                    let loading = if world_ready {
                        None
                    } else {
                        Some(area_loaded as f32 / area_needed.max(1) as f32)
                    };

                    let info = FrameInfo {
                        position: player.position,
                        chunk: ChunkManager::chunk_for_world_pos(
                            player.position.x,
                            player.position.z,
                        ),
                        grounded: player.grounded,
                        loaded_chunks: chunks.loaded_count(),
                        pending_chunks: chunks.pending_count(),
                        render_distance: chunks.render_distance(),
                        queued_meshes: dirty.len() + urgent.len() + mesher.in_flight(),
                        queued_arrivals: arrivals.len(),
                        lighting_jobs: mesher.lighting_in_flight(),
                        remote_players: remote_players.len(),
                        entities: entities.len(),
                        clock: sky.clock_string(),
                        sun_intensity: sky.sun_intensity(),
                        seed: world_seed,
                        selected_block: block_name(input.selected_block()),
                        draw_calls: graphics.draw_calls_last_frame,
                        chunks_culled: graphics.chunks_culled_last_frame,
                        underwater,
                    };
                    window.set_title(&debug_stats.title(&info));
                    debug_stats.maybe_dump_console(&info);

                    // --- UI ---
                    //
                    // One vertex list for the whole overlay: hotbar,
                    // then the F3 panel, then the pause screen on top.
                    // They share a pipeline and a buffer, so the order
                    // they are appended in is the order they stack.
                    // The hotbar is hidden behind the loading screen --
                    // there is nothing to place yet, and it would sit on
                    // top of the dim.
                    let mut ui_vertices = if loading.is_none() {
                        hotbar::build(&graphics.textures, input.hotbar_slot)
                    } else {
                        Vec::new()
                    };
                    if debug_stats.console_enabled {
                        ui_vertices.extend(debug_panel(
                            &debug_stats.overlay_lines(&info),
                            graphics.aspect(),
                            graphics.textures.font,
                        ));
                    }
                    if paused {
                        ui_vertices.extend(menu.build(&menu_context(
                            &settings, &worlds, &graphics, true,
                        )));
                    }

                    // Entities are drawn with the terrain pipeline, so
                    // they get the same textures, lighting and fog as
                    // the blocks they came from.
                    // Written into buffers that persist between frames
                    // rather than freshly allocated ones -- both on the
                    // CPU and on the GPU. See `write_dynamic_mesh`.
                    entity_vertices.clear();
                    entity_indices.clear();
                    if !entities.is_empty() {
                        entities.build_mesh_into(
                            &face_layers,
                            &light,
                            &mut entity_vertices,
                            &mut entity_indices,
                        );
                    }
                    graphics.write_dynamic_mesh(
                        &mut entity_mesh,
                        &entity_vertices,
                        &entity_indices,
                    );

                    actor_vertices.clear();
                    actor_indices.clear();
                    remote_players::build_actor_mesh_into(
                        &remote_players,
                        &mut actor_vertices,
                        &mut actor_indices,
                    );
                    graphics.write_dynamic_mesh(&mut actor_mesh, &actor_vertices, &actor_indices);

                    match graphics.render(
                        &camera,
                        &params,
                        &meshes,
                        Some(&actor_mesh),
                        Some(&entity_mesh),
                        loading,
                        &ui_vertices,
                    ) {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            graphics.resize(graphics.size)
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                        Err(e) => eprintln!("render error: {e:?}"),
                    }
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

/// Everything the shaders need for this frame, derived from the
/// server-synced sky plus local render settings.
fn frame_params(
    settings: &ClientSettings,
    sky: &Sky,
    render_distance: i32,
    fog_enabled: bool,
    underwater: bool,
) -> FrameParams {
    let (mut fog_start, mut fog_end) = settings.fog_range(render_distance);
    if underwater {
        // Water closes the world in a lot faster than air does.
        fog_end = settings.underwater_fog_distance;
        fog_start = fog_end * 0.15;
    }

    FrameParams {
        sun_direction: sky.sun_direction(),
        sun_intensity: sky.sun_intensity(),
        fog_color: sky.fog_color(underwater),
        fog_start,
        fog_end,
        ambient: settings.ambient_light,
        block_light_boost: settings.block_light_boost,
        ao_strength: settings.ambient_occlusion,
        fog_enabled,
        underwater,
    }
}

fn wish_direction(input: &input::InputState, camera: &Camera) -> Vec3 {
    let mut dir = Vec3::ZERO;
    if input.is_down(KeyCode::KeyW) {
        dir += camera.forward_horizontal();
    }
    if input.is_down(KeyCode::KeyS) {
        dir -= camera.forward_horizontal();
    }
    if input.is_down(KeyCode::KeyD) {
        dir += camera.right_horizontal();
    }
    if input.is_down(KeyCode::KeyA) {
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

#[allow(clippy::too_many_arguments)]
fn handle_click(
    button: MouseButton,
    chunks: &ChunkManager,
    camera: &Camera,
    input: &input::InputState,
    player: &Player,
    others: &[Vec3],
    net: &mut network::NetworkHandle,
    debug_stats: &mut DebugStats,
) {
    let Some((hit, before)) =
        physics::raycast_block(chunks, camera.position, camera.forward(), INTERACT_RANGE)
    else {
        return;
    };

    let (target, block_id) = match button {
        MouseButton::Left => (hit, BLOCK_AIR),
        MouseButton::Right => (before, input.selected_block()),
        _ => return,
    };

    // Don't place a block inside a player -- yourself included. Looking
    // down and right-clicking would otherwise wall you into the ground.
    // The server enforces this too; checking here means the block never
    // flickers into view only to be taken back a round trip later.
    if block_id != BLOCK_AIR {
        let feet = (player.position.x, player.position.y, player.position.z);
        if block_overlaps_player(feet, target.0, target.1, target.2) {
            return;
        }
        for other in others {
            if block_overlaps_player((other.x, other.y, other.z), target.0, target.1, target.2) {
                return;
            }
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
    meshes: &mut HashMap<ChunkPos, GpuMesh>,
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
        meshes.remove(&pos);
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
fn drain_network(
    net: &mut network::NetworkHandle,
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    sky: &mut Sky,
    my_id: PlayerId,
    player: &mut Player,
    remote_players: &mut RemotePlayers,
    entities: &mut entities::Entities,
    arrivals: &mut VecDeque<primitive_shared::types::Chunk>,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut HashSet<ChunkPos>,
    versions: &mut HashMap<ChunkPos, u64>,
    debug_stats: &mut DebugStats,
    // Set when the session has ended, and why. The caller tears it down
    // and shows this on the menu.
    disconnected: &mut Option<String>,
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
                arrivals.push_back(chunk);
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
                if id != my_id {
                    println!("[chat] {username} joined");
                    remote_players.on_join(id, username);
                }
            }

            ServerMessage::PlayerLeft { id } => {
                if let Some(name) = remote_players.name_of(id) {
                    println!("[chat] {name} left");
                }
                remote_players.remove(id);
            }

            ServerMessage::Chat { username, text, .. } => {
                println!("[chat] <{username}> {text}");
            }

            ServerMessage::TimeSync { time_of_day, .. } => {
                sky.on_time_sync(time_of_day);
            }

            ServerMessage::PositionCorrection { x, y, z, reason } => {
                // The server is authoritative: snap, don't argue.
                eprintln!("[anticheat] position corrected: {reason}");
                player.teleport(Vec3::new(x, y, z));
                debug_stats.corrections_received += 1;
            }

            ServerMessage::Ping { nonce } => {
                net.send(ClientMessage::Pong { nonce });
                debug_stats.network_messages_out_this_second += 1;
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

            ServerMessage::Error(e) => eprintln!("server error: {e}"),
        }
    }
}

/// Applies a confirmed block change. Everything it dirties goes on the
/// **urgent** queue: a block edit is something the player is looking at
/// right now, and the sand simulation's updates are visible motion.
#[allow(clippy::too_many_arguments)]
fn apply_change(
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    arrivals: &mut VecDeque<primitive_shared::types::Chunk>,
    urgent: &mut VecDeque<ChunkPos>,
    dirty_set: &mut HashSet<ChunkPos>,
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
        if let Some(pending) = arrivals.iter_mut().find(|c| c.pos == pos) {
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

fn mark_dirty(dirty: &mut VecDeque<ChunkPos>, dirty_set: &mut HashSet<ChunkPos>, pos: ChunkPos) {
    if dirty_set.insert(pos) {
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
    dirty_set: &mut HashSet<ChunkPos>,
    pos: ChunkPos,
) {
    if dirty_set.insert(pos) {
        urgent.push_back(pos);
    } else if !urgent.contains(&pos) {
        // Already queued as ordinary work -- promote it. The stale entry
        // in `dirty` is skipped when it comes up, because the set no
        // longer contains the position by then.
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
    dirty_set: &mut HashSet<ChunkPos>,
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
        if !dirty_set.remove(&pos) {
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
    graphics: &GraphicsState,
    chunks: &ChunkManager,
    light: &mut LightMap,
    meshes: &mut HashMap<ChunkPos, GpuMesh>,
    urgent: &mut VecDeque<ChunkPos>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut HashSet<ChunkPos>,
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
                    meshes.remove(&pos);
                } else {
                    meshes.insert(pos, graphics.upload_chunk_mesh(&buffers));
                }
                mesher.recycle(cache, buffers);
            }

            mesher::Finished::Light { pos, data } => {
                // The worker did the isolated pass; the seam
                // reconciliation touches the shared map and stays here.
                if chunks.is_loaded(pos) {
                    for changed in light.insert_precomputed(&*chunks, pos, data) {
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
    arrivals: &mut VecDeque<primitive_shared::types::Chunk>,
    chunks: &mut ChunkManager,
    mesher: &mut mesher::Mesher,
    budget_ms: f32,
    debug_stats: &mut DebugStats,
) {
    let started = Instant::now();
    let budget = Duration::from_secs_f32(budget_ms / 1000.0);

    while let Some(chunk) = arrivals.pop_front() {
        let pos = chunk.pos;

        // Copy the blocks straight out of the arriving chunk, *before*
        // handing it to the world.
        //
        // The obvious-looking alternative -- read them back out of the
        // ChunkManager -- costs a hash lookup per cell, 16,384 of them
        // per chunk. That measured at ~19 ms per chunk and was the whole
        // of the remaining frame-rate sag while terrain streamed in.
        let blocks = chunk.blocks.clone();
        chunks.insert(chunk);

        // The pure, expensive half of lighting goes to a worker; the
        // seam reconciliation happens in `collect_worker_results`.
        mesher.submit_lighting(pos, blocks);

        if started.elapsed() >= budget {
            break;
        }
    }

    debug_stats.chunk_time_ms_this_second += started.elapsed().as_secs_f32() * 1000.0;
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
        let mut dirty_set = HashSet::new();
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

        let mut mesher = mesher::Mesher::new(crate::texture::FaceLayers::empty_for_test(), 2);
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
        let mut dirty_set = HashSet::new();
        // Marked as ordinary work, then promoted by an edit: it now sits
        // in both queues, but must not be meshed twice.
        mark_dirty(&mut dirty, &mut dirty_set, pos);
        mark_urgent(&mut urgent, &mut dirty_set, pos);

        let mut mesher = mesher::Mesher::new(crate::texture::FaceLayers::empty_for_test(), 2);
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
        let mut dirty_set = HashSet::new();
        mark_dirty(&mut dirty, &mut dirty_set, ChunkPos::new(99, 99));

        let mut mesher = mesher::Mesher::new(crate::texture::FaceLayers::empty_for_test(), 2);
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
fn debug_panel(
    lines: &[String],
    aspect: f32,
    font: crate::texture::FontAtlas,
) -> Vec<hotbar::HotbarVertex> {
    const SCALE: f32 = 0.85;
    const PAD: f32 = 0.02;

    let mut painter = ui::Painter::new(font);
    let widest = lines
        .iter()
        .map(|line| ui::measure(line, SCALE))
        .fold(0.0f32, f32::max);
    let height = lines.len() as f32 * ui::line_height(SCALE);

    // Anchored to the window's actual left edge, which needs the aspect
    // ratio: UI x runs from -aspect to +aspect. It used to be a constant
    // -1.68, which is the left edge of a 16:9 window and *off-screen* on
    // anything narrower -- on a 4:3 window the panel hung past the edge
    // and the first characters of every line were cut off.
    let left = -aspect.max(0.1) + PAD * 2.0;
    let top = 0.94;
    let panel = ui::Rect::new(left - PAD, top - height - PAD, left + widest + PAD, top + PAD);
    painter.quad(panel, [0.03, 0.04, 0.06, 0.72]);
    painter.border(panel, 0.003, ui::PANEL_EDGE);

    let mut y = top;
    for line in lines {
        painter.text(line, left, y, SCALE, ui::TEXT);
        y -= ui::line_height(SCALE);
    }
    painter.into_vertices()
}
