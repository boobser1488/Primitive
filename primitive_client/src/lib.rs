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

// The five layers the client is built out of. Each is a directory with
// its own `mod.rs` explaining what belongs in it and what does not:
//
//   engine  -- the GPU, and everything that exists to feed it
//   audio   -- the speaker, and everything that exists to feed it
//   net     -- the socket, and the state that arrives over it
//   ui      -- what the player reads, clicks and presses
//   logic   -- the world as the client understands it
//
// Everything below is what is left over: the entry point itself, the
// settings file, the crash handler, and the assets baked into the
// binary. They belong to no layer because every layer uses them.
mod audio;
mod engine;
mod platform;
mod logic;
mod net;
mod ui;

mod crash;
mod embedded;
mod settings;
// The real game played by a script -- see the module note.
#[cfg(test)]
mod scenario;

// Every layer meets here, and only here -- so the modules are pulled in
// by name and the file below reads the way it did before the split. The
// block itself is the map: anything used unqualified in this file is on
// one of these five lines.
use audio::{Audio, Soundscape};
use engine::{fog, mesh, mesher, texture};
use logic::{entities, hand, menu_scene, mining, physics, shake, stamina, worlds};
use net::{network, remote_players};
use ui::{
    chat, chest_screen, death, hotbar, hud, ime, input, inventory_screen, keybinds, menu,
    station_screen, widgets,
};
use ui::ime::Fields as _;

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use glam::Vec3;
// The game's own input vocabulary. `KeyCode` is an alias rather than a
// rename because every use below means what it always meant -- a
// physical key position -- and only the type behind the name changed.
use platform::{Key as KeyCode, MouseButton};

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
use engine::renderer::{FrameParams, GraphicsState, Scene};
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
    /// How full the player is. Its own bar, so its own bits.
    nourishment: u32,
    /// What is burning within working range, which decides which
    /// recipes the crafting column offers -- so the screen genuinely
    /// looks different on either side of it. A campfire and a kiln open
    /// different halves of the table, so it is the pair rather than a
    /// yes or no.
    heat: primitive_shared::crafting::Heat,
    /// The server's last refusal: a fingerprint of the text, and
    /// whether it is still fully opaque. The fade itself stays out --
    /// while it runs, the notice reports as animating instead.
    notice: Option<(u64, bool)>,
    chat: u64,
    inventory_screen: u64,
    /// Fingerprint of every wound: the figure in the pack is coloured by
    /// them and the marks beside the health bar come and go with them, so
    /// a bandage that goes on or a cut that fades is a changed screen.
    wounds: u64,
    chest_screen: u64,
    /// The anvil's and the wheel's screen: open or not, what is under the
    /// pointer, the blows struck and the last verdict. **It was missing**, so
    /// the screen was drawn only when something else on this key happened to
    /// change -- it opened a frame or a minute late, its buttons did not light
    /// under the pointer, the verdict never appeared and the marker stood
    /// still for the whole run ("меню мини игр сломано полностью"). The
    /// marker itself moves with no event behind it and is rebuilt every frame
    /// a run is up (`StationScreen::is_running`); see `ui_rebuilt`.
    station_screen: u64,
    death: u64,
    /// The dark a sleeper's screen goes. Changes every frame it is falling
    /// or lifting, and not at all otherwise -- see `Sleep::ui_key`.
    sleep: Option<(u8, bool, bool)>,
    /// The journal, open or shut. See `Journal::ui_key` for what moves it.
    journal: u64,
    /// Only presence. The panel's numbers change every frame it is on
    /// screen, so while it is up the interface reports as animating
    /// rather than hashing a page of figures to learn what it already
    /// knows.
    debug_panel: bool,
    /// The heads-up display hidden with its key (`Action::ToggleHud`).
    hud_hidden: bool,
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

/// ...and of every wound, on the same terms: the type is shared and does
/// not hash itself. The severities by their bits, because the server only
/// sends a new set when one has moved far enough to see
/// (`Injuries::worth_reporting`), so every change that arrives is one worth
/// redrawing for.
fn wounds_fingerprint(injuries: &primitive_shared::injury::Injuries) -> u64 {
    use primitive_shared::injury::{Kind, Part};
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for part in Part::ALL {
        for kind in Kind::ALL {
            let wound = injuries.wound(part, kind);
            (wound.severity.to_bits(), wound.dressed).hash(&mut h);
        }
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
/// How fast a full gale carries a raindrop sideways, in blocks a second.
///
/// **Read off the fall, not chosen for looks.** A drop falls at
/// `particles::RAIN_SPEED`, twenty-two blocks a second, and the slant a
/// player sees is the ratio of the two: at this figure a full gale lays the
/// rain about thirty degrees off vertical, which is a storm you lean into,
/// and a light air leaves it all but upright. A larger number is rain going
/// sideways as fast as it falls, which reads as spray rather than as
/// weather; a smaller one is a storm that cannot be told from a drizzle with
/// the sound off.
const RAIN_WIND_SPEED: f32 = 12.0;
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
/// The platform's event loop takes over the main thread and has to be
/// block on tokio work at two points -- starting a local server, and
/// stopping one so the world is saved before the process exits. Both are
/// `Runtime::block_on`, which panics if it is called from inside a
/// runtime, which is exactly what `#[tokio::main]` would put us in.
/// The desktop entry point, called by `src/main.rs`.
///
/// Public because the binary is a shim now: the game is a library so
/// that Android can load it, and a library's `main` is just a function
/// with a name nobody calls. See the `[lib]` note in `Cargo.toml`.
pub fn desktop_main() -> std::process::ExitCode {
    crash::install_panic_handler();

    // `--export-models <dir>`: write the animals out as `.obj` and stop.
    //
    // A flag on the game rather than an example beside it, and not by
    // choice: `primitive_client` is a binary, so an example cannot reach
    // the model tables at all -- and re-typing them into a script is the
    // second copy `logic::obj_export` exists to avoid.
    let args: Vec<String> = std::env::args().collect();
    if let Some(index) = args.iter().position(|a| a == "--export-models") {
        let dir = args.get(index + 1).map(std::path::PathBuf::from);
        let Some(dir) = dir else {
            eprintln!("--export-models needs a folder to write into");
            return std::process::ExitCode::FAILURE;
        };
        let both = logic::obj_export::write_models(&dir)
            .and_then(|obj| Ok((obj, logic::bbmodel::write_models(&dir)?)));
        return match both {
            Ok((obj, bb)) => {
                for path in obj.iter().chain(&bb) {
                    println!("wrote {}", path.display());
                }
                println!("...and the pictures they use, in {}", dir.join("textures").display());
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("could not export the models: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    // `--export-sounds <dir>`: write every recording, as the game decodes
    // it, and a sample of the generated music out as `.wav` and stop.
    //
    // Here for the same reason as `--export-models`: the music is composed
    // in the binary and the recordings are compiled into it. It is how a
    // sound gets listened to at the level the game plays it, and it is the
    // first half of the resource-pack story -- what comes out is named
    // exactly what `audio::bank` looks for on the way back in.
    if let Some(index) = args.iter().position(|a| a == "--export-sounds") {
        let dir = args.get(index + 1).map(std::path::PathBuf::from);
        let Some(dir) = dir else {
            eprintln!("--export-sounds needs a folder to write into");
            return std::process::ExitCode::FAILURE;
        };
        // Forty seconds of each mood: long enough to hear a piece
        // start, run and stop, short enough that six of them is not a
        // hundred megabytes.
        return match audio::export(&dir, 40.0) {
            Ok(paths) => {
                println!("wrote {} file(s) to {}", paths.len(), dir.display());
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("could not export the sounds: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    // `--export-icon <file>`: write the launcher icon out as a PNG and
    // stop.
    //
    // For the Android packaging script, which needs a `res/mipmap`
    // entry and cannot use the game's own 16x16 texture: Android draws
    // a launcher icon at up to 192 pixels, and asking it to scale a
    // sixteen-pixel picture gives a blurred smear rather than the
    // crisp blocks the game is made of. The upscale is nearest-
    // neighbour, which is the whole point.
    //
    // A flag rather than a step in the script for the same reason
    // `--export-models` is one: the picture and the scaling rule are
    // both in the binary, and a second copy in a shell script is a
    // second copy to keep in step.
    if let Some(index) = args.iter().position(|a| a == "--export-icon") {
        let Some(path) = args.get(index + 1).map(std::path::PathBuf::from) else {
            eprintln!("--export-icon needs a file to write");
            return std::process::ExitCode::FAILURE;
        };
        // Optional second argument: how big. 192 is what Android asks
        // for at the highest density it draws a launcher icon at.
        let size = args
            .get(index + 2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(192);
        return match texture::export_icon(&path, size) {
            Ok(()) => {
                println!("wrote {}", path.display());
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("could not export the icon: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    match start(None) {
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

/// The Android entry point.
///
/// The system loads this library into a process it already started and
/// calls here; there is no `main`, no command line and no working
/// directory worth having. Everything this does before `start` is
/// making those three things true enough for the rest of the game --
/// see `platform::android`.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    // First, before anything can have something to say. A Rust
    // program's stdout on Android goes nowhere at all, so until this
    // runs the game is mute -- including about why it did not start.
    platform::android::log_to_logcat();
    crash::install_panic_handler();

    // The activity, stashed before anything can need it: winit cannot
    // build an event loop on Android without it, and that happens
    // several layers inside `start`.
    platform::set_android_app(app.clone());

    // Assets out of the package and the working directory somewhere
    // writable. Fatal if it fails -- there is no game without textures,
    // and no way to say so on screen before there is a screen.
    let assets_dir = match platform::android::bootstrap(&app) {
        Ok(dir) => dir,
        Err(e) => {
            crash::report_fatal("could not unpack the game's files", &e);
            return;
        }
    };

    if let Err(e) = start(Some(assets_dir)) {
        crash::report_fatal("could not start", &e);
    }
}

/// Loads everything off disk and opens the window.
///
/// `assets_override` is how Android says where it put the files it
/// unpacked from the package. A desktop passes `None` and lets the
/// settings file and the usual search decide -- see
/// `texture::resolve_assets_dir`.
fn start(assets_override: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let mut settings = ClientSettings::load_or_default();
    if let Some(dir) = assets_override {
        settings.assets_dir = dir.to_string_lossy().into_owned();
    }
    let settings = settings;
    let servers = menu::ServerList::load_or_default(&settings.server_addr);
    let worlds = worlds::Worlds::load(&settings.singleplayer_world_dir);
    println!("{} singleplayer world(s)", worlds.list().len());
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| anyhow::anyhow!("could not start the async runtime: {e}"))?;
    run(settings, servers, worlds, runtime)
}

/// Sync entry point for the platform's event loop. Runs on the main thread;
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

    let assets_dir = texture::resolve_assets_dir(&settings.assets_dir);
    // Before anything can mesh an animal or a bed: the first to ask would
    // otherwise fix the built-in models for the whole run, and a player's
    // edited file beside the game would be read and then never drawn.
    logic::models::load(&assets_dir);

    // Built before the window so the icon is there from the first frame
    // rather than appearing a moment later.
    let icon = texture::load_window_icon(&assets_dir);

    // The one place the game names a backend. Everything below holds
    // the window through `platform::Window` and reads `platform::Event`,
    // so swapping this line is what porting to something that is not
    // winit costs.
    use platform::Window as _;
    let (mut app, window) = platform::backend::App::new(&platform::WindowConfig {
        title: "Primitive".to_string(),
        width: settings.window_width,
        height: settings.window_height,
        fullscreen: settings.fullscreen,
        icon,
    })?;

    println!("assets dir: {}", assets_dir.display());
    // The likeliest failure a player will ever hit, and the one worth
    // naming: no usable GPU, or drivers too old for the backend.
    let mut graphics = pollster::block_on(GraphicsState::new(
        window.raw(),
        window.size(),
        &assets_dir,
        settings.vsync,
        settings.anisotropy,
        settings.msaa,
        settings.lighting,
    ))
    .map_err(|e| anyhow::anyhow!("graphics could not start: {e}"))?;
    // Not a constructor argument: the sky target has to be built from
    // the surface size, which the state only knows about itself.
    graphics.set_sky_scale(settings.sky_scale);
    graphics.set_ui_scale(settings.ui_scale);
    // Off by default, in which case this builds nothing at all.
    graphics.set_shadows(settings.shadows);
    graphics.set_shadow_distance(settings.shadow_distance);
    graphics.set_plant_shadows(settings.plant_shadows);
    // **Android is waiting for an answer while this runs.**
    //
    // Starting up takes a second or two -- a GPU device, a texture
    // atlas, a sound bank, a pool of worker threads -- and none of it
    // touches the event loop. On a desktop that is merely a pause
    // before the window appears. On Android the system is asking the
    // process questions the whole time (`onPause`, `onStop`, a
    // configuration change), the Java thread blocks until they are
    // answered, and a watchdog kills anything that has not answered in
    // five seconds. It killed this game seven times in one evening.
    //
    // So the loop is serviced between the phases rather than only
    // after them. `pump_idle` buffers what it drains and replays it
    // into the frame loop when that starts, because a `Resized`
    // dropped here is a swapchain built at the wrong size and a
    // `Suspended` dropped here is a renderer drawing into a window
    // that has gone. Costs one syscall that finds nothing when nothing
    // is waiting.
    app.pump_idle();
    println!("loaded {} block texture(s)", graphics.textures.layer_count);
    // What every fill-rate figure in the F3 panel is *per*.
    //
    // On a desktop it is the window and the player chose it. On a phone
    // it is whatever the display is, which is not a number anybody
    // picked and is routinely larger than the desktop it was developed
    // on -- this device is 2712x1220, which is more pixels than 1080p.
    // A frame time without it beside it cannot be compared with
    // anything.
    let size = graphics.size;
    println!(
        "surface: {}x{} ({:.1} Mpx)",
        size.width,
        size.height,
        (size.width as f32 * size.height as f32) / 1.0e6,
    );

    // Sound. After the graphics rather than before, so the two startup
    // lines appear in the order the two subsystems matter in -- and
    // because a machine with no audio device should still have got as
    // far as a window by the time it says so.
    //
    // Never fails: see `audio::Audio::start`. A player with no sound
    // card gets their game.
    let audio = Audio::start(&assets_dir);
    audio.set_volumes(settings.master_volume, settings.music_volume);
    app.pump_idle();
    // The timers behind footsteps, weather and the choice of music.
    let mut soundscape = Soundscape::new();

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
    // The last phase long enough to matter: spawning workers is where
    // the remaining hundreds of milliseconds go.
    app.pump_idle();
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
    // What detail level each loaded chunk's mesh was built at. See
    // `engine::lod`, and `restripe_detail_levels` for who changes it.
    //
    // Kept here rather than beside the chunk, because it is not a fact
    // about the chunk at all -- it is a fact about where the player is
    // standing, and the same chunk is fine one minute and coarse the
    // next without a single block in it changing.
    let mut chunk_lod: HashMap<ChunkPos, Detail> = HashMap::new();
    // The chunk the player was standing in when the levels were last
    // worked out. The scan is a walk over every loaded chunk -- eighteen
    // hundred of them at render distance 24 -- and nothing about it can
    // change until the player crosses into another chunk, so it runs
    // once per crossing rather than once per frame.
    let mut lod_scanned_from: Option<(ChunkPos, i32, crate::engine::lod::Quality, (i32, i32))> = None;
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

    // **The menu's sky, and it is the menu's for as long as there is
    // no world.** It used to start at 0.3 -- mid-morning, for no reason
    // anybody wrote down -- while the backdrop behind the menu is
    // authored, and its veil *measured*, at dusk (see
    // `menu_scene::TIME_OF_DAY`, where the reason is that small text has
    // to stay readable over it). So the menu was lit one way on the
    // first run, another way after leaving a world, and a third way
    // again when a world opened: from the player's seat the menu's time
    // of day simply kept changing.
    //
    // One time, set here and set back on leaving a world, so the only
    // clock a player ever sees move is a world's own.
    let mut sky = Sky::new(menu_scene::TIME_OF_DAY, 900.0);

    // The world standing behind the menus, and what it was asked for.
    //
    // Outside the session state above on purpose: this is the one piece
    // of world the client makes for itself, it exists only while there
    // is *no* session, and it is thrown away the moment there is one.
    // See `logic::menu_scene`.
    let mut menu_scene: Option<menu_scene::MenuScene> = None;
    let mut menu_scene_asked_for: Option<menu_scene::Place> = None;
    // When the current backdrop was asked for. `PRIMITIVE_SHOT` waits
    // on it the way the world's shot waits on the terrain arriving.
    let mut menu_scene_started: Option<Instant> = None;
    // How many frames the menu has drawn, and since when. The world has
    // the F3 line for this; the menu had nothing at all, because until
    // there was a world behind it there was nothing on it worth timing.
    let mut menu_frames: u32 = 0;
    let mut menu_bench_started: Option<Instant> = None;

    let mut player = Player::new((Vec3::new(0.5, 40.0, 0.5)).as_dvec3(), settings.move_speed);
    let mut camera = Camera::new(player.eye_position(), graphics.aspect());
    camera.fov_y_radians = settings.fov_degrees.to_radians();

    // Rebuilt every frame because they move every frame; the storage
    // for them is not.
    // Where the frame is drawn from. Starts at the world's origin and
    // follows the player from the first frame -- see `render_origin_for`.
    let mut render_origin = Vec3::ZERO;
    // What was burning within reach when the crafting column last
    // looked, and whether it was looking. See the scan below.
    let mut last_heat = primitive_shared::crafting::Heat::NONE;
    let mut heat_was_open = false;
    let mut entity_mesh = graphics.new_dynamic_mesh();
    let mut particle_mesh = graphics.new_dynamic_mesh();
    // How many of `particle_mesh`'s indices go before the water. Kept with
    // the mesh, because the mesh is not rebuilt every frame and the two have
    // to describe the same buffer. See `Particles::behind_water_first`.
    let mut particles_behind_water: u32 = 0;
    let mut actor_mesh = graphics.new_dynamic_mesh();
    // The cracks on the block being mined. Its own mesh because it is
    // textured and blended, where the outline around the same block is
    // flat geometry on the actor pipeline.
    let mut break_mesh = graphics.new_dynamic_mesh();
    let mut break_vertices: Vec<mesh::Vertex> = Vec::new();
    let mut break_indices: Vec<u32> = Vec::new();
    let mut entity_vertices: Vec<mesh::Vertex> = Vec::new();
    let mut particle_vertices: Vec<engine::particles::ParticleVertex> = Vec::new();
    let mut particle_indices: Vec<u32> = Vec::new();
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
    // A mouthful under way, and when the last one went to the server. See
    // `Meal`.
    let mut meal: Option<Meal> = None;
    // A cut into a carcass under way, sent when the knife has done it. See
    // `Cut`.
    let mut cut: Option<Cut> = None;
    let mut meal_sent: Option<Instant> = None;
    let mut hand_mesh = graphics.new_dynamic_mesh();
    let mut hand_vertices: Vec<hand::HandVertex> = Vec::new();
    let mut hand_indices: Vec<u32> = Vec::new();
    let mut actor_vertices: Vec<remote_players::ActorVertex> = Vec::new();
    let mut actor_indices: Vec<u32> = Vec::new();
    // Where the other players are, for the collision pass. See the
    // note where it is filled.
    let mut other_positions: Vec<glam::DVec3> = Vec::new();
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
    // The on-screen controls. Empty and free on a desktop -- no finger
    // ever touches it, so it holds nothing and does nothing -- and the
    // whole of the control scheme on a phone. See `platform::touch`.
    // **Fixed, and no longer a setting.** There used to be a screen for
    // arranging these and a saved copy in `client_settings.toml`, and
    // between them they were how half the controls came to be missing:
    // four of the eight shipped switched off, the only way to switch
    // one on was that screen, and a saved arrangement kept the old
    // four even after the default changed. Worked out once here rather
    // than read every frame, because nothing can change it any more.
    // **The player's arrangement, not the shipped one.** This asked for
    // the default outright, which meant the whole of `TouchLayout` --
    // its corners, its RESET, its exact `same_as` comparison -- was a
    // type nothing could ever put a value into.
    let mut touch_layout = settings.touch_layout;
    // Whether the arrangement has been moved since it was last written
    // down. See where it is applied, in the menu branch of the frame.
    let mut arrangement_unsaved = false;
    let mut touch = platform::touch::Touch::default();
    // The bar reads its own finger; see `hotbar::Gestures`.
    let mut bar_gestures = ui::hotbar::Gestures::default();
    // ...and the same glass while a screen is up, where a finger means
    // something else entirely: a drag scrolls the list rather than
    // steering, and a press is not a click until the finger comes up
    // without having travelled. See `platform::touch::Pointer`.
    let mut pointer = platform::touch::Pointer::default();
    // How long the gauges have had nothing to say. See
    // `hud::Attention`: the stack fades out when the player is well,
    // fed, watered and rested, and comes back the moment any of that
    // stops being true.
    let mut hud_attention = hud::Attention::default();
    // Whether the click being delivered *right now* is carrying the
    // modifier a second finger stands for -- shift, as far as the
    // screens are concerned. See `platform::touch::Chord`.
    //
    // **It lives exactly as long as the click does.** The obvious
    // alternative was to hold the sprint key down in `InputState` for
    // as long as the modifier finger is on the glass, so that
    // everything reading `action_down(Sprint)` would see it without
    // being told. That leaks: the finger's lift is delivered to
    // whatever owns the glass *then*, and a screen closing in between
    // leaves the key held and the player sprinting across a field they
    // never asked to cross. A flag written by the touch that produced
    // the click and read by the two screens that act on it cannot
    // outlive either.
    let mut thumb_quick = false;
    // Whether to draw them at all. A property of the platform rather
    // than a setting: a phone has no W key to walk forward with.
    let touch_controls = window.is_touch_primary();
    // Whether the on-screen keyboard is up.
    //
    // **Which field the on-screen keyboard is for, and what it and the
    // game last agreed that field said.** See `ui::ime`, which is
    // where the whole of the reconciliation lives and where the reason
    // it is keyed to a *field* rather than to a screen is written
    // down: keying it to a screen is what made the seed box on the
    // create-world form take no text.
    //
    // Derived once a frame rather than set at each place a field is
    // entered and left, because there are a dozen of those -- clicking
    // the row, tabbing off it, pressing escape, pressing DONE, opening
    // the chat, dying with the chat open -- and every one that was
    // forgotten is a phone left with half its screen under a keyboard
    // for a field that is no longer there.
    let mut ime = ime::Mirror::default();
    // **`PRIMITIVE_IME_TYPE=<text>` types that text into a new world's
    // name and creates it, and `PRIMITIVE_IME_TYPE=seed:<digits>` does
    // the same for the box beside it.** See `ui::ime::Probe` for what
    // the second spelling is for -- the seed box was the one the player
    // reported, and the name box was the only one anything ever tried.
    //
    // ## Why the game has to be able to do this to itself
    //
    // A phone cannot be driven. This one answers
    // `adb shell input keyevent` with
    // `Injecting input events requires the caller to have the
    // INJECT_EVENTS permission` -- MIUI refuses the shell that
    // permission outright, so there is no tap, no swipe and no
    // keystroke a script can deliver. Everything that has to be checked
    // on a device therefore has to be reachable from the environment,
    // or it cannot be checked twice by the same method.
    //
    // What this checks is the whole text path and nothing else: a
    // string is handed to the platform's editor exactly as an input
    // method commits one (`set_ime_text`), and then the frame loop's
    // own mirror has to notice it, filter it, put it in the field and
    // let the form be submitted. If Cyrillic can survive that, the only
    // thing left between it and a player is the keyboard's own keys.
    //
    // ## Why it is staged over seconds rather than done at once
    //
    // Because each step is a different frame's work. Opening the form
    // is what makes `AboutToWait` ask for a keyboard and seed the
    // editor; the commit has to come after that or it is overwritten by
    // the seeding; and reading the field back has to come after the
    // mirror has run at least once. Sleeping between them is how a
    // sequence of frames is expressed from outside the frame loop.
    let ime_probe = std::env::var("PRIMITIVE_IME_TYPE").ok().map(|v| ime::Probe::parse(&v));
    let mut ime_probe_step = 0u8;
    let ime_probe_started = Instant::now();
    // **`PRIMITIVE_IME_TRACE=1` narrates *both* halves of the mirror.**
    //
    // The probe above prints what the editor said. Nothing ever printed
    // what the game said *back*, and that is the half a duplicated
    // character can come from: the game filters what it was offered,
    // pushes its own text into the editor to say so, and an input
    // method handed a fresh buffer is entitled to restart its
    // composition against it -- which some do by re-committing the
    // character just typed. From the outside that is indistinguishable
    // from a keyboard that simply sent the key twice, and the two have
    // opposite fixes. One direction of trace cannot tell them apart.
    //
    // Its own variable rather than a verbose flag, and off unless
    // asked for: it is a line per keystroke. On a device it goes in
    // `primitive.env` beside the app, because `adb shell run-as` is
    // refused on this phone and an activity the system started has an
    // environment nobody chose.
    let ime_trace = std::env::var("PRIMITIVE_IME_TRACE").is_ok_and(|v| v != "0");
    // A scripted run is a diagnostic run, so it traces without being
    // asked twice: the probe's own lines say what was *offered* to the
    // field and these say what the two copies did about it, and reading
    // one without the other is how the doubled-character bug survived a
    // year. See `ui::ime`.
    let traced = ime_probe.is_some() || ime_trace;
    // Survival state. All of it is the server's to decide; the client
    // only draws what it is told, and resets to full on a fresh session
    // so a previous world's health never shows in a new one.
    let mut inventory = Inventory::new();
    let mut mining = mining::Mining::new();
    // Everything small and moving: rain, the chips off a broken block,
    // sparks over a fire. See `engine::particles`.
    let mut particles = engine::particles::Particles::new();
    // Whether the server has been told this player is swinging at a block.
    let mut dig_signal = DigSignal::default();
    // ...and the small life round the player, which has a mind where a
    // particle has none. See `engine::critters`.
    let mut critters = engine::critters::Critters::new();
    // ...and the wind made visible: streaks and dust in a gale, leaves off
    // the broadleaf crowns. See `engine::breeze` for why it is neither.
    let mut breeze = engine::breeze::Breeze::new();
    // Seconds until the next self-inflicted wound, for
    // `PRIMITIVE_BLEED`. Zero means "now", so a photograph taken six
    // seconds in already has blood on the ground under it.
    let mut bleed_in = 0.0f32;
    let mut health = survival_defaults::MAX;
    let mut max_health = survival_defaults::MAX;
    // Lags `health` downward so a hit leaves a draining strip on the
    // bar. Purely a display value.
    let mut recent_health = survival_defaults::MAX;
    // When the last blow was started and when the one under way lands, so
    // holding the button is a rhythm rather than a packet a frame and a
    // thrust leaves when its point is out. The server has its own copy of
    // the rate -- see `primitive_shared::combat` and `hand::Strikes`.
    let mut strikes = hand::Strikes::default();
    // What is in the chest the player has open, if any. The screen owns
    // "is a chest open" as well as its contents -- see `ui::chest_screen`.
    // The map, the recipe book, and what both are drawn from -- the land
    // seen, the kinds held, the way back to a bag. See `ui::journal`.
    let mut journal = ui::journal::Journal::new();
    let mut chest_screen = chest_screen::ChestScreen::new();
    // The anvil's and the wheel's screen. On the chest's footing everywhere
    // below: it owns the cursor while it is up, it opens when the server
    // answers rather than when the player clicks, and it is shut by the same
    // two keys. See `ui::station_screen`.
    let mut station_screen = station_screen::StationScreen::new();
    // Dying, and the screen it puts up. The screen owns the fact of
    // being dead as well as the drawing of it -- see `ui::death`.
    let mut death = death::DeathScreen::new();
    // What the cursor was doing before, so the grab is only changed on
    // the frame the answer changes.
    let mut was_dead = false;
    // The same, for the chest screen -- which is opened by the server's
    // answer rather than by a keypress, so nothing else can do it.
    let mut chest_was_open = false;
    let mut station_was_open = false;
    // Whether the screen now open belongs to something with a lid. The same
    // screen shows a hearth, a kiln and a pit, and every one of them used to
    // creak open like the chest; asked on opening, since by the close the
    // screen has already forgotten where it was.
    let mut chest_lid = false;
    // The last thing the server refused, and when. Errors used to go to
    // stderr only, so on a released build the game silently did nothing
    // and never said why.
    let mut notice: Option<(String, Instant)> = None;
    // The float of a line this client cast, while it thinks the line is in
    // the water. See `logic::fishing`.
    let mut fishing_float: Option<logic::fishing::Float> = None;
    // The rod being wound back, and what the hand did last frame. See
    // `logic::fishing` and `step_rod`.
    let mut rod_hold = logic::fishing::Hold::default();
    let mut shake = shake::Shake::new(settings.view_bob);
    let mut stamina = stamina::Stamina::new();
    // Air, as the server last reported it. One while anybody's head is
    // above water, which is almost always.
    let mut breath: f32 = 1.0;
    // How thick the smoke is where the eyes are, as the server last said,
    // and what the fog is showing of it -- eased, because the server steps
    // it by twentieths and a fog that thickens in jumps reads as flicker.
    // See `ServerMessage::Smoke` and `Fog::fill_with_smoke`.
    let mut smoke: f32 = 0.0;
    let mut smoke_shown: f32 = 0.0;
    // How full the player is, 0..1, as the server last reported it.
    // Server-owned exactly the way health is: the client draws the bar
    // and never decides what it says.
    let mut nourishment: f32 = 1.0;
    // How warm and how watered, as the server last reported. Both
    // server-owned exactly the way health and hunger are: the client
    // draws the gauges and never decides what they say -- a client that
    // decided its own temperature would be a client that is never cold.
    let mut body = ui::hud::BodyGauges::default();
    // Whether the player is asleep, as the server last said, and how dark
    // that has made the screen. While they are, the body is not simulated
    // and the keys do nothing but get them up -- see
    // `ServerMessage::Asleep` and `logic::posture::Sleep`.
    let mut sleep = logic::posture::Sleep::default();
    // Sitting, lying or on their feet, as the server last said, and where
    // the eye goes for each -- see `logic::posture`. `rising` is what
    // stops a held key sending "get up" sixty times a second while the
    // server's answer is on its way, and a key held from walking to the
    // bed sending it at all; `looked_along_bed` turns the
    // camera to face the foot once per lying down, and never again while
    // the sleeper looks round.
    let mut resting = logic::posture::Resting::Standing;
    // Which raft's deck the feet are on, carried from frame to frame; see
    // `logic::riding`. The raft being rowed lives in `entities`.
    let mut riding = logic::riding::Riding::default();
    let mut rising = logic::posture::Rising::default();
    let mut looked_along_bed = false;
    // ...and `faced_the_chair` does the same for sitting down in a chair:
    // the view is turned to the way the chair faces once, and the sitter
    // looks round freely after that.
    let mut faced_the_chair = false;
    // What the player has on. Drawn in the inventory screen and read
    // for the speed a full set of plate costs; the server owns it.
    let mut equipment = primitive_shared::inventory::Equipment::new();
    // What the sky is doing. Sent on join and whenever it changes, so
    // this is right from the first frame rather than clear until the
    // first roll.
    let mut weather = primitive_shared::weather::Weather::Clear;
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
    // What the chunks and the light hold, as last counted. See where
    // `FrameInfo` is built for why it is not counted every frame.
    let mut counted_bytes = (0usize, 0usize);
    let mut debug_stats = DebugStats::default();
    // Tab's hidden interface. Not saved: a player who comes back to a world
    // should see their bars, not wonder where they went.
    let mut hud_hidden = false;
    debug_stats.console_enabled = settings.debug_overlay_on_start;

    let mut fog_enabled = settings.fog_enabled;
    // How shut in the eye is, carried between frames because it settles
    // over about two seconds rather than switching -- see
    // `engine::fog::Underground`.
    let mut underground = fog::Underground::default();
    let mut sequence: u32 = 0;
    // Physics stays frozen until the ground under the player exists.
    // Without this the player spawns into empty space, falls through the
    // world while the first chunks are still in flight, and the server's
    // anti-cheat sees a 60 m/s descent.
    let mut world_ready = false;

    let player_update_interval = Duration::from_secs_f32(1.0 / settings.player_update_hz.max(1.0));
    let mut last_player_update_sent = Instant::now() - player_update_interval;
    let mut last_sent_transform: Option<(glam::DVec3, f32, f32)> = None;
    // True while the pause screen is up. The world keeps rendering and
    // the network keeps draining -- pausing a client of an authoritative
    // server does not pause the world, and pretending otherwise would
    // just mean a backlog to catch up on.
    let mut paused = false;
    // Shift and control, held. **Not read off `InputState`**, which is
    // emptied whenever a screen takes the keyboard -- so in the one
    // place these two decide anything, which is editing text in a form,
    // it would always answer "no". See the `Keyboard` arm.
    let mut held_shift = false;
    let mut held_ctrl = false;
    // Where the pointer last was, in interface space.
    //
    // **Kept here because a button event carries no position.** Every
    // screen that wants one is handed it on `CursorMoved` and remembers
    // it (`Menu::cursor`, and the same in the pack and the chest); the
    // chat box is the one widget that is clicked *while the world has
    // the screen*, so there was nobody holding it for that case. Only
    // ever read while the chat box is open, which is also the only time
    // the cursor is loose during play.
    let mut last_cursor: Option<(f32, f32)> = None;
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
    // **`PRIMITIVE_SHOT=<file>` keeps one frame.**
    //
    // The world's half of `ui::snapshot`, and it exists for the same
    // reason: what can only be looked at by launching the game and
    // taking a photograph gets looked at once. See `engine::capture`.
    let mut pending_shot = engine::capture::Pending::from_env();
    let mut world_ready_since: Option<Instant> = None;
    // Whether `PRIMITIVE_CHAT_SEND`'s line has already gone. See the
    // send site: the world becoming ready is not a once-per-run event.
    let mut chat_line_sent = false;

    // **`PRIMITIVE_LOOK=<yaw> <pitch>` aims the camera.**
    //
    // A screenshot of a world is a screenshot of whatever direction the
    // save happened to be left facing, and the faults worth
    // photographing are not all in that direction. With
    // `PRIMITIVE_LOOK_SWEEP` it is also the only way to get two frames
    // that differ by a fraction of a degree, which is what a flicker
    // *is* -- see `engine::capture::Look`.
    let forced_look = engine::capture::Look::from_env();
    let look_frames = forced_look.as_ref().map_or(1, |look| look.frames);
    let mut shots_taken: u32 = 0;

    let bench_seconds: Option<f32> = std::env::var("PRIMITIVE_BENCH")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|seconds| *seconds > 0.0);
    let mut bench_started: Option<Instant> = None;

    // **`PRIMITIVE_DEBUG_PANEL=0` keeps the numbers off the screen.**
    // `PRIMITIVE_AUTOSTART` turns the per-second stats on, because a
    // benchmark that prints nothing answered nothing -- and the on-screen
    // panel rides the same flag, so every photograph of the world taken
    // with `PRIMITIVE_SHOT` came out with a third of it covered in
    // diagnostics. The console line still prints; only the panel goes.
    let debug_panel_allowed =
        std::env::var("PRIMITIVE_DEBUG_PANEL").map_or(true, |value| value.trim() != "0");

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

    // Dev affordance: `PRIMITIVE_AUTOCONNECT=<host:port>` joins a server
    // straight away, the way `PRIMITIVE_AUTOSTART` opens a world.
    //
    // **It exists because a phone cannot be driven.** Every other way
    // into a multiplayer session is a tap on a menu, and a tap is
    // exactly what an Android device under test does not offer: MIUI
    // refuses synthetic input outright, so "connect, place a block,
    // pull the network out, suspend the activity, come back" is a
    // sequence nobody can run more than once by hand and nobody can run
    // at all unattended. With this it is a script, and cross-play
    // becomes something that can be *measured* rather than demonstrated.
    //
    // An address rather than an index into the server list: a test
    // should say where it is going, and a list is a thing that changes
    // under it. The address is added to the list on the way past so the
    // session has a name to show and a row to come back to.
    if let Ok(address) = std::env::var("PRIMITIVE_AUTOCONNECT") {
        let address = address.trim().to_string();
        if address.is_empty() {
            eprintln!("[autoconnect] no address given");
        } else {
            let index = match menu
                .servers
                .servers
                .iter()
                .position(|entry| entry.address == address)
            {
                Some(index) => index,
                None => {
                    menu.servers.servers.push(ui::menu::ServerEntry {
                        name: format!("auto {address}"),
                        address: address.clone(),
                    });
                    menu.servers.servers.len() - 1
                }
            };
            println!("[autoconnect] joining {address}");
            menu.begin_connecting(format!("auto {address}"));
            last_attempt = Attempt::Server(index);
            pending_connect = Some(spawn_connect(
                &runtime,
                address,
                settings.username.clone(),
            ));
            // For the same reason autostart does it: a run that produces
            // no numbers is a run that answered nothing.
            debug_stats.console_enabled = true;
        }
    }

    // What the player has to work with, which is not the same list on
    // every machine. Printing the keyboard one on a phone is printing
    // instructions for hardware that is not there.
    if touch_controls {
        println!(
            "controls: left thumb to walk (push to the rim to run) |              right side: drag to look, tap to place, hold to mine |              the bar: tap to choose, slide to step, hold to eat, flick up to drop |              five buttons: JUMP, PACK, MENU, CHAT, F3"
        );
    } else {
        println!(
            "controls: WASD move | Shift sprint | Space jump | mouse look (click to grab) |              I inventory | Esc pause | hold LMB to mine | RMB place | 1-9 or wheel pick slot |              R respawn | F fog | F3 stats"
        );
    }

    app.run(move |event, elwt| {
        // Everything that has to happen exactly once on the way out,
        // whichever way the player leaves: a clean disconnect, and a
        // saved singleplayer world. `Runtime::block_on` is safe here
        // because this thread is not inside the runtime -- see `main`.
        macro_rules! quit {
            () => {{
                if let Some(net) = net.as_ref() {
                    net.send(ClientMessage::Disconnect);
                }
                journal.end_session();
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
                        let name = menu.name_input.text().trim().to_string();
                        let seed = if menu.seed_input.is_empty() {
                            settings.singleplayer_seed
                        } else {
                            menu.seed_input.text().parse::<u32>().unwrap_or(settings.singleplayer_seed)
                        };
                        match worlds.create_in(&name, seed, menu.world_preset, menu.world_zone) {
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
                        let typed = menu.name_input.text().trim();
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
                        // A name typed into the settings and not
                        // committed with a key. See
                        // `Menu::take_typed_username`: pressing Enter
                        // used to be the only way a username was ever
                        // stored, which on a phone meant it never was.
                        if let Some(typed) = menu.take_typed_username() {
                            let typed = typed.trim();
                            if !typed.is_empty() && typed != settings.username {
                                settings.username = typed.to_string();
                                settings.sanitize();
                                settings_dirty = true;
                            }
                        }
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
                    Action::OpenExtensions => {
                        // The menu has already set itself to "waiting";
                        // the socket is here. Nothing is sent when the
                        // answer is already in hand, so re-opening the
                        // screen costs no traffic -- see
                        // `Menu::extensions_awaited`.
                        if menu.extensions_awaited() {
                            if let Some(handle) = net.as_ref() {
                                handle.send(ClientMessage::RequestExtensions);
                            }
                        }
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
                        // The list described *that* server. Kept across
                        // the leave it would be shown again on the next
                        // one, correct-looking and wrong, until an
                        // answer happened to arrive.
                        menu.forget_extensions();
                        if let Some(handle) = net.take() {
                            handle.send(ClientMessage::Disconnect);
                        }
                        if let Some(server) = local_server.take() {
                            println!("saving the world...");
                            runtime.block_on(server.stop());
                        }
                        // The map goes to disk with the world, and
                        // everything the journal knew about it is
                        // forgotten before another one opens.
                        journal.end_session();
                        paused = false;
                        release_cursor(&window, &mut input);
                        menu.open(Screen::Main);
                    }
                    Action::Quit => quit!(),
                    // **A tap on a text box is a request for a
                    // keyboard, even when it is the box that already
                    // had the focus.** The player's way out of a soft
                    // keyboard is the back gesture, which the game
                    // never hears about -- and it is the gesture they
                    // have to make to reach the buttons under it. So
                    // "the focus did not move" is not the same as
                    // "nothing to ask for", and treating it as such was
                    // a form that could be typed into exactly once.
                    //
                    // Forgetting rather than raising it here, so that
                    // the one place that decides what the keyboard is
                    // doing stays the one place: `ui::ime` re-reads the
                    // field on the next frame and seeds the editor with
                    // it as well, which is the other half of a tap.
                    Action::Focus(_) => ime.forget(),
                    // Everything else is pure menu navigation, already
                    // carried out by `Menu::apply`.
                    _ => {}
                }
            }};
        }

        // A finger, turned into whatever a hand would have done.
        //
        // The buttons that stand for a *click* or a *keystroke* become
        // exactly that event and fall through the match below with the
        // mouse and keyboard, so there is one path for placing a block
        // and not two. The ones that stand for a *held* state -- the
        // dig button, the movement stick, the drag that turns the
        // camera -- are not events at all; they are read off `touch`
        // once a frame, further down, where the keyboard's own held
        // keys are read.
        let event = match event {
            platform::Event::Touch { id, phase, x, y } => {
                // **A finger is two different things, and which one
                // depends on what is on screen.**
                //
                // In the world it is a gamepad: the left of the glass
                // is a stick, the right turns the camera, and the
                // buttons dig and place. On a *menu* it is a mouse --
                // and it has to be, because a menu has no stick and no
                // camera. This used to be missing, and the effect was
                // that the whole interface did nothing on a phone:
                // every tap went into the movement stick and no button
                // was ever pressed, because there is no world behind
                // the main menu for a stick to move anybody in.
                //
                // The same condition the rest of the frame uses to
                // decide whether the world has the input.
                let world_has_input = world_owns_the_glass(
                    net.is_some(),
                    paused,
                    inventory_screen.open,
                    chest_screen.is_open() || station_screen.is_open(),
                    death.is_open(),
                    chat.is_typing(),
                ) && !journal.is_open();

                if !world_has_input {
                    // **The journal takes the finger whole.** The map is
                    // dragged in two directions and `Pointer` hands out
                    // vertical scrolls only, so while the journal is open
                    // it reads the raw touch itself -- see `ui::journal`.
                    if journal.is_open() && net.is_some() && !paused && !death.is_open() {
                        let at = widgets::cursor_to_ui(
                            (x as f64, y as f64),
                            (graphics.size.width, graphics.size.height),
                            1.0,
                        );
                        let mark = player_mark(player.position.as_vec3(), camera.yaw);
                        if journal.touch(id, phase, at, graphics.aspect(), mark)
                            == ui::journal::Outcome::Closed
                        {
                            audio.play(audio::Sfx::Click);
                            grab_cursor(&window, &mut input);
                        }
                        send_journal_command(&mut journal, net.as_ref(), &mut debug_stats);
                        return;
                    }
                    // A pointer that is wherever the finger is. Placed
                    // first and directly rather than as a queued
                    // `CursorMoved`, because a tap is a move *and* a
                    // click on the same screen at the same instant, and
                    // a click at the last frame's cursor position is a
                    // click on whatever the player was pointing at
                    // before.
                    let size = graphics.size;
                    // Undivided, because the interface no longer has one
                    // scale: each screen grows by what fits *it*. The
                    // division that used to be here happens per screen
                    // in `place_cursor`, against the same number that
                    // screen's geometry was multiplied by.
                    let at = widgets::cursor_to_ui(
                        (x as f64, y as f64),
                        (size.width, size.height),
                        1.0,
                    );
                    // **What the finger turned out to mean.** A press
                    // used to be sent through as a mouse button the
                    // instant it landed, which decided the question
                    // before the answer existed: a finger that goes on
                    // to travel was scrolling a list, and pressing what
                    // it started on as well is worse than not scrolling
                    // at all. `Pointer` waits.
                    let gesture = pointer.handle(
                        graphics.size,
                        id,
                        phase,
                        x,
                        y,
                        Instant::now(),
                    );
                    // Set from this gesture and from nothing else, so a
                    // plain tap that follows a modified one clears it.
                    thumb_quick = matches!(
                        gesture,
                        platform::touch::Gesture::Tapped(platform::touch::Chord::Quick),
                    );
                    // The pointer goes where the finger is, and leaves
                    // when it does -- so a row does not stay lit under a
                    // thumb that is no longer on the glass. A tap counts
                    // as being over it: see `Gesture::carries_a_point`,
                    // which is a method rather than a list here because
                    // getting that list wrong made every tap in the game
                    // do nothing.
                    place_cursor(
                        gesture.carries_a_point().then_some(at),
                        net.is_none() || paused,
                        widgets::Layout::for_screen(graphics.aspect(), graphics.ui_scale()),
                        &mut menu,
                        &mut death,
                        &mut chest_screen,
                        &mut station_screen,
                        &mut inventory_screen,
                    );
                    // **The chat box before anything else, because
                    // there is no Enter key on a phone.** A player
                    // could open the box with the button on the glass,
                    // type into it with the on-screen keyboard, and
                    // then neither send nor leave: sending hangs on
                    // `Enter` and leaving on `Escape`, and an input
                    // method may send neither -- its action key is
                    // often "Done" and often produces nothing a game
                    // can read. The box carries its own two answers on
                    // a touch device; see `chat::Tap`.
                    //
                    // Taken back through the growth the widget was
                    // drawn with, exactly as the hotbar's own hit-test
                    // is, because a finger lands in the grown picture
                    // and the rectangles are authored in the small one.
                    if chat.is_typing() && matches!(gesture, platform::touch::Gesture::Tapped(_)) {
                        let grown = widgets::Layout::for_screen(
                            graphics.aspect(),
                            graphics.ui_scale(),
                        )
                        .fit_from_corner(chat::EXTENT);
                        // The lift comes off first, because it went on
                        // last: the widget was grown and then pushed up
                        // the glass, so a finger has to come down the
                        // glass and then be shrunk. Doing it the other
                        // way round misses by the lift times the scale,
                        // which on a phone is most of a button.
                        let lifted = (
                            at.0,
                            at.1 - chat::keyboard_lift(touch_controls, true, grown),
                        );
                        let authored = widgets::unscale_about(
                            lifted,
                            widgets::anchor::BOTTOM_LEFT(graphics.aspect()),
                            grown,
                        );
                        match chat.tapped(graphics.aspect(), touch_controls, authored) {
                            Some(chat::Tap::Send) => {
                                submit_chat(&mut chat, net.as_ref(), &mut journal, &mut debug_stats);
                                close_chat(&mut chat, &window, &mut input, paused);
                            }
                            Some(chat::Tap::Leave) => {
                                close_chat(&mut chat, &window, &mut input, paused);
                            }
                            // ...and a tap on the line itself puts the
                            // caret in it, which is the first thing
                            // anybody tries on text they can see.
                            // ...unless the platform's own editor holds
                            // the line, in which case it holds the
                            // caret too and ours would be drawn where
                            // the next character is *not* going. The
                            // same guard the form fields carry -- see
                            // `Menu::place_caret_under_cursor`.
                            Some(chat::Tap::Caret(at)) if !window.ime_owns_text() => {
                                chat.place_caret(at)
                            }
                            // ...and nothing at all where it does: the
                            // editor holds the caret with the text, and
                            // ours would be a bar drawn where the next
                            // character is not going.
                            Some(chat::Tap::Caret(_)) => {}
                            // A tap anywhere else while the box is up is
                            // a misclick, which is what it has always
                            // been -- see the `MouseButton` arm.
                            None => {}
                        }
                        return;
                    }
                    match gesture {
                        // A tap, at the point the finger came *up*.
                        // Only the press is sent: every screen in this
                        // game acts on the press and returns on the
                        // release -- see the `MouseButton` arm -- so a
                        // release would be a second event that does
                        // nothing.
                        // A rested press is the right button -- but
                        // **only where a right button means anything**.
                        // Everywhere else in this game a click is a
                        // click, and a player who pressed PLAY slowly
                        // would otherwise have pressed nothing at all:
                        // the menus act on the left button and drop the
                        // right one in silence. The screens that do use
                        // it are the two that hold stacks of things,
                        // which is where the gesture was invented for.
                        platform::touch::Gesture::Tapped(chord) => {
                            let containers_are_open =
                                inventory_screen.open || chest_screen.is_open() || station_screen.is_open();
                            let secondary = matches!(
                                chord,
                                platform::touch::Chord::Secondary,
                            ) && containers_are_open;
                            platform::Event::MouseButton {
                                button: if secondary {
                                    MouseButton::Right
                                } else {
                                    MouseButton::Left
                                },
                                pressed: true,
                            }
                        }
                        // ...and a drag, in the units the wheel already
                        // speaks, so every list that could be scrolled
                        // with a wheel can now be scrolled with a thumb
                        // and none of them had to learn anything.
                        platform::touch::Gesture::Scrolled(lines) => {
                            platform::Event::MouseWheel { lines }
                        }
                        _ => return,
                    }
                } else {
                    let mut return_event = None;
                    // **The bar first, and only where it is drawn.**
                    //
                    // A phone has no number row and no wheel, so the
                    // hotbar was the one part of the HUD a thumb could
                    // see and not use: the only way to change what you
                    // were holding was to open the pack and drag. It is
                    // checked before the thumb controls because it sits
                    // over them in the drawing -- and what is drawn on
                    // top has to be what is pressed, or the interface
                    // is lying about which thing the finger is on.
                    //
                    // **Four gestures, not a tap.** The bar answers for
                    // the number row, the wheel, `E` and `Q` -- seven
                    // controls a phone has nowhere to put, all of which
                    // point at the ten squares already on screen. What
                    // tells them apart is what the finger does; see
                    // `hotbar::Gestures`.
                    {
                        let point = widgets::cursor_to_ui(
                            (x as f64, y as f64),
                            (graphics.size.width, graphics.size.height),
                            1.0,
                        );
                        // Back through the growth the HUD was drawn
                        // with. See `widgets::unscale_about`: the bar
                        // is authored at one size and grown about the
                        // bottom of the screen, so a finger lands in
                        // the grown picture and has to be asked about
                        // in the authored one.
                        let authored = widgets::unscale_about(
                            point,
                            widgets::anchor::BOTTOM(graphics.aspect()),
                            graphics.ui_scale(),
                        );
                        // The same count the bar is drawn with, from
                        // the same constant, so the hit-test cannot be
                        // asking about a bar of a different width.
                        let slot = hotbar::slot_at(
                            authored.0,
                            authored.1,
                            crate::logic::inventory::HOTBAR_SLOTS,
                        );
                        // Whether this finger is the bar's, asked
                        // *before* the gesture is fed in: a lift is the
                        // event that ends the bar's ownership, and
                        // asking afterwards would let the lift fall
                        // through to the controls underneath.
                        let claimed = slot.is_some() || bar_gestures.owns(id);
                        let touched = bar_gestures.handle(
                            phase,
                            id,
                            slot,
                            authored,
                            Instant::now(),
                        );
                        if let Some(event) =
                            hotbar_gesture_event(touched, &mut input, &settings.keybinds)
                        {
                            return_event = Some(event);
                        } else if claimed {
                            // The bar has the finger and made nothing of
                            // it yet; nothing else may also have it.
                            return;
                        }
                    }
                    if let Some(event) = return_event {
                        event
                    } else {

                    touch.resize(graphics.size, touch_layout, graphics.ui_scale());
                    // **A button on the glass is a key.** What it sends
                    // is carried in the arrangement, not decided here,
                    // and both edges are sent -- down when the thumb
                    // lands, up when it lifts -- so what reaches the
                    // game is indistinguishable from a keyboard. That
                    // is the whole point: every binding the game has
                    // works on a phone without being taught to, and so
                    // does every one it grows later.
                    //
                    // This used to be a `match` on four named controls
                    // turning each into the action it "meant", which is
                    // why a touch player could not eat, drop, or reach
                    // anything nobody had thought to name.
                    let hit = touch.handle(id, phase, x, y, Instant::now());
                    // A short tap in the look area is the right mouse
                    // button, and it goes down the *same* path a real
                    // one does -- placing a block, opening a chest,
                    // striking a fire are one piece of code with one
                    // set of rules, and a second copy for phones is a
                    // second copy to keep in step. See
                    // `touch::Touch::is_mining` for why the hands
                    // moved off the glass.
                    if touch.take_place() {
                        platform::Event::MouseButton {
                            button: MouseButton::Right,
                            pressed: true,
                        }
                    } else {
                    let (slot, pressed) = match hit {
                        platform::touch::Hit::Pressed(slot) => (slot, true),
                        platform::touch::Hit::Released(slot) => (slot, false),
                        platform::touch::Hit::Nothing => return,
                    };
                    match touch_layout.buttons[slot].emits {
                        // The wheel is about the glass rather than about
                        // the game: it has already opened or closed
                        // itself inside `touch`, and there is nothing
                        // downstream to tell. See `settings::Emits::More`.
                        settings::Emits::More => return,
                        settings::Emits::Mine => platform::Event::MouseButton {
                            button: MouseButton::Left,
                            pressed,
                        },
                        settings::Emits::Place => platform::Event::MouseButton {
                            button: MouseButton::Right,
                            pressed,
                        },
                        settings::Emits::Key(key) => platform::Event::Keyboard {
                            key: Some(key),
                            // No text: a button on the glass is a key
                            // being *pressed*, not a character being
                            // typed. Sending text as well would put a
                            // letter in whatever field had focus every
                            // time the player jumped.
                            text: platform::Text::default(),
                            pressed,
                            repeat: false,
                        },
                    }
                    }
                    }
                }
            }
            other => other,
        };

        // Written in the game's own events, never the backend's --
        // the translation happened at the door, in `App::run`.
        match event {
            platform::Event::CloseRequested => quit!(),

            platform::Event::Resized(new_size) => {
                graphics.resize(new_size);
                camera.aspect = graphics.aspect();
            }

            platform::Event::CursorMoved { x, y } => {
                    // Only meaningful when something is up that wants a
                    // pointer: during play the cursor is grabbed and
                    // motion arrives as raw deltas instead.
                    let size = graphics.size;
                    // Undivided -- see the touch path above.
                    let at = widgets::cursor_to_ui(
                        (x as f64, y as f64),
                        (size.width, size.height),
                        1.0,
                    );
                    last_cursor = Some(at);
                    place_cursor(
                        Some(at),
                        net.is_none() || paused,
                        widgets::Layout::for_screen(graphics.aspect(), graphics.ui_scale()),
                        &mut menu,
                        &mut death,
                        &mut chest_screen,
                        &mut station_screen,
                        &mut inventory_screen,
                    );
                    if journal.is_open() {
                        journal.set_cursor(Some(at), graphics.aspect(), player_mark(player.position.as_vec3(), camera.yaw));
                    }
                }

            platform::Event::CursorLeft => menu.set_cursor(None),

            platform::Event::Keyboard { key, text, pressed, repeat } => {
                    // **Shift and control are tracked here, not read off
                    // `input`.** `InputState` is deliberately emptied
                    // whenever a screen takes the keyboard
                    // (`release_all`), so in the one place the two
                    // modifiers mean something -- editing text in a
                    // form -- it knows nothing about them. Two bools in
                    // the frame loop, updated before anything branches
                    // on the screen, so a key pressed in a menu and
                    // released in the world cannot leave one stuck.
                    if let Some(code) = key {
                        match code {
                            KeyCode::ShiftLeft | KeyCode::ShiftRight => held_shift = pressed,
                            KeyCode::ControlLeft | KeyCode::ControlRight => held_ctrl = pressed,
                            _ => {}
                        }
                    }
                    let is_pressed = pressed;
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
                            if let Some(code) = key {
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
                        // **Whether keys may edit this field at all.**
                        //
                        // On a phone they may not, and the reason is
                        // double entry rather than tidiness. While the
                        // input method owns the field (see
                        // `platform::Window::ime_owns_text`) every edit
                        // has already been made in *its* copy and
                        // arrives through the mirror in `AboutToWait`.
                        // A soft keyboard that also sends a Backspace
                        // as a key event -- and several do -- would
                        // then delete one character here and one there,
                        // and the player would watch two letters
                        // vanish for one tap.
                        //
                        // Only the editing keys are held back. Escape,
                        // Enter and Tab are navigation and still get
                        // through: they are how a form is left,
                        // submitted and moved around, and the input
                        // method has no opinion about any of that.
                        let ime_holds_the_field =
                            menu.accepts_text() && window.ime_owns_text();
                        // Text first: a character typed into a field must
                        // not also be read as a shortcut.
                        if keys_may_type_into_the_field(
                            menu.accepts_text(),
                            window.ime_owns_text(),
                        ) {
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
                        if let Some(code) = key {
                            // Every key that edits or moves within the
                            // text, not just the two that delete: while
                            // an input method owns the field it owns
                            // the caret too, and a Home sent through
                            // here would move the game's copy out from
                            // under the one the player can see in their
                            // keyboard's own strip.
                            if ime_holds_the_field
                                && matches!(
                                    code,
                                    KeyCode::Backspace
                                        | KeyCode::Delete
                                        | KeyCode::Home
                                        | KeyCode::End
                                        | KeyCode::ArrowLeft
                                        | KeyCode::ArrowRight
                                )
                            {
                                return;
                            }
                            if let Some(key) =
                                menu_key(code, text.first(), ime_holds_the_field, held_shift, held_ctrl)
                            {
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
                        // The same division as the menu forms above:
                        // while the input method owns the line, the
                        // letters and the deletions arrive through the
                        // mirror and a key that also made them would
                        // make them twice.
                        let ime_holds_the_line = window.ime_owns_text();
                        if !ime_holds_the_line {
                            for c in text.chars() {
                                chat.type_char(c);
                            }
                        }
                        if let Some(code) = key {
                            // The whole editing set, not just
                            // Backspace: while an input method owns the
                            // line it owns the caret in it too, and a
                            // Home sent through here would move the
                            // game's copy out from under the one the
                            // player can see in their keyboard's strip.
                            // The same guard the menu forms carry --
                            // see `menu_key`.
                            if ime_holds_the_line
                                && matches!(
                                    code,
                                    KeyCode::Backspace
                                        | KeyCode::Delete
                                        | KeyCode::Home
                                        | KeyCode::End
                                        | KeyCode::ArrowLeft
                                        | KeyCode::ArrowRight
                                )
                            {
                                return;
                            }
                            match code {
                                KeyCode::Enter | KeyCode::NumpadEnter if !repeat => {
                                    submit_chat(&mut chat, net.as_ref(), &mut journal, &mut debug_stats);
                                    close_chat(&mut chat, &window, &mut input, paused);
                                }
                                KeyCode::Escape => {
                                    close_chat(&mut chat, &window, &mut input, paused);
                                }
                                // The caret keys, on the same terms the
                                // forms have them -- see `ui::field`.
                                KeyCode::Backspace => {
                                    chat.edit(chat::Edit::Backspace { word: held_ctrl })
                                }
                                KeyCode::Delete => {
                                    chat.edit(chat::Edit::Delete { word: held_ctrl })
                                }
                                KeyCode::ArrowLeft => chat.edit(chat::Edit::Move {
                                    motion: if held_ctrl {
                                        ui::field::Motion::WordLeft
                                    } else {
                                        ui::field::Motion::Left
                                    },
                                    extend: held_shift,
                                }),
                                KeyCode::ArrowRight => chat.edit(chat::Edit::Move {
                                    motion: if held_ctrl {
                                        ui::field::Motion::WordRight
                                    } else {
                                        ui::field::Motion::Right
                                    },
                                    extend: held_shift,
                                }),
                                KeyCode::Home => chat.edit(chat::Edit::Move {
                                    motion: ui::field::Motion::Home,
                                    extend: held_shift,
                                }),
                                KeyCode::End => chat.edit(chat::Edit::Move {
                                    motion: ui::field::Motion::End,
                                    extend: held_shift,
                                }),
                                KeyCode::KeyA if held_ctrl => chat.edit(chat::Edit::SelectAll),
                                // Up and down are the sent lines, not
                                // the log: the log is scrolled with the
                                // wheel, and a player holding a
                                // keyboard is reaching for the arrows
                                // to get a command back. See
                                // `Chat::recall`.
                                KeyCode::ArrowUp => chat.recall(-1),
                                KeyCode::ArrowDown => chat.recall(1),
                                _ => {}
                            }
                        }
                        return;
                    }

                    // The journal, while it is open, has the keyboard the
                    // way the chat box does: Escape shuts it, Tab turns the
                    // page, and on the recipe page letters are a search
                    // rather than shortcuts -- or typing "bronze" would
                    // throw away whatever is in the hand.
                    if journal.is_open() && net.is_some() && !paused && !death.is_open() {
                        if !is_pressed {
                            return;
                        }
                        let binds = &settings.keybinds;
                        let action = key.and_then(|code| {
                            keybinds::Action::ALL.into_iter().find(|a| binds.key(*a) == Some(code))
                        });
                        match key {
                            Some(KeyCode::Escape) => {
                                journal.close();
                                grab_cursor(&window, &mut input);
                            }
                            Some(KeyCode::Tab) => journal.switch_tab(),
                            Some(KeyCode::Backspace) if journal.takes_text() => journal.backspace(),
                            _ => {
                                let mut typed = false;
                                if journal.takes_text() {
                                    for c in text.chars().filter(|c| !c.is_control()) {
                                        journal.type_char(c);
                                        typed = true;
                                    }
                                }
                                let tab = match action {
                                    Some(keybinds::Action::Map) => Some(ui::journal::Tab::Map),
                                    Some(keybinds::Action::Recipes) => Some(ui::journal::Tab::Recipes),
                                    Some(keybinds::Action::Give) => Some(ui::journal::Tab::Give),
                                    _ => None,
                                };
                                if let (false, Some(tab)) = (typed, tab) {
                                    ask_if_operator(net.as_ref());
                                    if !journal.toggle(tab) {
                                        grab_cursor(&window, &mut input);
                                    }
                                }
                            }
                        }
                        return;
                    }

                    if let Some(code) = key {
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
                            && !station_screen.is_open()
                                        && !death.is_open()
                                        && !repeat =>
                                {
                                    chat.open(Instant::now());
                                    // A phone has no keyboard until it
                                    // is asked for one. Nothing on a
                                    // desktop, where it is already
                                    // there.
                                    window.set_ime_visible(true);
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
                                // The station screen, on the same two keys and
                                // for the same reason: the server is holding a
                                // seat open for this player and has to be told
                                // the player has left it.
                                (KeyCode::Escape, _) | (_, Some(keybinds::Action::Inventory))
                                    if station_screen.is_open() =>
                                {
                                    close_station(
                                        &mut station_screen,
                                        net.as_ref(),
                                        &mut debug_stats,
                                    );
                                }
                                // **The blow, on the space bar.** No keybind of
                                // its own: a run is four presses over five
                                // seconds on a screen that says what to press,
                                // and a binding nobody would ever change is a
                                // row in the keybinds screen that only makes it
                                // longer. Jump is what space does in the world,
                                // and the world does not have the keyboard while
                                // this screen is up. Key repeat is refused --
                                // a held space is one blow, not forty.
                                (KeyCode::Space, _) if station_screen.is_open() && !repeat => {
                                    // Asked before the press, because the
                                    // press is what ends the run -- see
                                    // `StationScreen::striking`.
                                    let blow = station_screen.striking();
                                    let intent = station_screen.press();
                                    if let Some(game) = blow {
                                        audio.play_flat(audio::bank::station_blow(game), 0.7, 1.0);
                                    }
                                    if let (Some(intent), Some(net)) = (intent, net.as_ref()) {
                                        send_station_intent(
                                            intent,
                                            &mut station_screen,
                                            net,
                                            &mut debug_stats,
                                            &audio,
                                        );
                                    }
                                }
                                (KeyCode::Escape, _) if inventory_screen.open => {
                                    // Esc backs out of the inventory
                                    // before it reaches for the pause
                                    // menu: one screen at a time.
                                    inventory_screen.close();
                                    grab_cursor(&window, &mut input);
                                }
                                // **A forced look is a photograph, and
                                // a photograph cannot be paused.**
                                //
                                // A capture run is unattended and the
                                // window it opens is not the one the
                                // desktop has focus on; a single stray
                                // Escape -- from a focus change, from
                                // the terminal it was launched out of --
                                // put the pause menu over every frame of
                                // a seventy-frame sweep and the run
                                // produced seventy photographs of a
                                // menu. The failure is silent: the files
                                // are written, the count is right, and
                                // the picture is of the wrong thing.
                                (KeyCode::Escape, _) if forced_look.is_some() => {}
                                (KeyCode::Escape, _) => {
                                    paused = true;
                                    menu.open(Screen::Paused);
                                    release_cursor(&window, &mut input);
                                    input.release_all();
                                }
                                (_, Some(keybinds::Action::ToggleStats)) => {
                                    debug_stats.toggle_console()
                                }
                                (_, Some(keybinds::Action::ToggleHud)) => hud_hidden = !hud_hidden,
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
                                // **Dead hands hold nothing.** See
                                // `works_while_dead` for what fell through
                                // the death screen before this arm.
                                (_, Some(bound))
                                    if death.is_open() && !works_while_dead(bound) => {}
                                (
                                    _,
                                    Some(
                                        keybinds::Action::Map
                                        | keybinds::Action::Recipes
                                        | keybinds::Action::Give,
                                    ),
                                ) if !chest_screen.is_open() && !station_screen.is_open() => {
                                    let tab = match action {
                                        Some(keybinds::Action::Map) => ui::journal::Tab::Map,
                                        Some(keybinds::Action::Give) => ui::journal::Tab::Give,
                                        _ => ui::journal::Tab::Recipes,
                                    };
                                    // One screen at a time, for the reason
                                    // the pack gives below.
                                    inventory_screen.close();
                                    chat.close();
                                    ask_if_operator(net.as_ref());
                                    // Only a journal that actually opened
                                    // takes the mouse. The give key of a
                                    // player who is not an operator is
                                    // refused by `toggle`, and releasing
                                    // the cursor anyway left the camera
                                    // dead with no screen to show for it
                                    // -- the player saw the mouse come
                                    // loose "for a window" that never came.
                                    if journal.toggle(tab) {
                                        release_cursor(&window, &mut input);
                                        input.release_all();
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
                                (_, Some(keybinds::Action::Eat)) => {
                                    // The hovered slot while a screen is
                                    // open, the selected one otherwise
                                    // -- exactly the rule the drop key
                                    // follows, because it is the same
                                    // question about the same slot.
                                    let slot = if inventory_screen.open {
                                        inventory_screen.hovered_slot()
                                    } else {
                                        Some(input.hotbar_slot)
                                    };
                                    eat_from(
                                        slot,
                                        &inventory,
                                        &mut meal,
                                        &mut hand,
                                    );
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
                                    let slot = something_to_throw(slot, &inventory);
                                    if let (Some(slot), Some(net)) = (slot, net.as_ref()) {
                                        net.send(ClientMessage::DropSlot {
                                            slot: slot as u8,
                                            whole_stack: input.action_down(
                                                binds,
                                                keybinds::Action::Sprint,
                                            ),
                                        });
                                        audio.play(audio::Sfx::Drop);
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
                                    window.set_fullscreen(settings.fullscreen);
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
                                // **A number over a slot swaps it with that
                                // square of the bar**, in the pack and at
                                // an open container alike: "сделай
                                // сочетания клавиш для работы в
                                // хранилищах". Picking a stack up and
                                // carrying it to the bar was two clicks and
                                // an aim for the thing a player does most.
                                // The bar's selection is left alone while a
                                // screen is up, as the wheel's is.
                                (_, _) if hover_swap(code, &inventory_screen, &chest_screen).is_some() => {
                                    if let (Some(message), Some(net)) =
                                        (hover_swap(code, &inventory_screen, &chest_screen), net.as_ref())
                                    {
                                        audio.play(audio::Sfx::Click);
                                        net.send(message);
                                        debug_stats.network_messages_out_this_second += 1;
                                    }
                                }
                                _ => input.set_key(code, true),
                            }
                        } else {
                            input.set_key(code, false);
                        }
                    }
                }

            platform::Event::MouseWheel { lines } => {
                    // Wheel away from the player moves *up* the bar, so
                    // "forward" through the slots is the wheel coming
                    // back. Normalised to lines by the backend; the
                    // hotbar only ever wants the sign.
                    let forward = lines < 0.0;
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
                    // With the box open the wheel belongs to the log,
                    // which is longer than the twelve rows on screen.
                    // A thumb drag arrives here as well (see the
                    // `Gesture::Scrolled` arm above), so the phone got
                    // a scrollable chat without a control of its own.
                    if chat.is_typing() {
                        chat.scroll_by(if forward { -1 } else { 1 });
                        return;
                    }
                    // The journal: zoom on the map, rows in the book.
                    if journal.is_open() {
                        journal.wheel(lines, graphics.aspect(), player_mark(player.position.as_vec3(), camera.yaw));
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

            platform::Event::MouseButton { button, pressed } => {
                    // Tracked whatever the game is doing, so releasing
                    // the button over a menu doesn't leave the world
                    // thinking it is still held.
                    if button == MouseButton::Left {
                        input.breaking = pressed
                            && input.mouse_grabbed
                            && !paused
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !chat.is_typing();
                    }
                    // The same, for the button the rod is wound back with
                    // (`logic::fishing::Hold`). Tracked wherever the game
                    // is, for `breaking`'s reason: a button released over a
                    // menu must not leave the rod winding for ever.
                    if button == MouseButton::Right {
                        input.using = pressed
                            && input.mouse_grabbed
                            && !paused
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !chat.is_typing();
                    }
                    // Clicking while typing is a misclick, not a swing:
                    // the cursor is loose because the chat box has it.
                    // **Except in the box itself**, where it is a player
                    // pointing at the letter they want to fix -- the
                    // same gesture the form fields answer. Taken back
                    // through the growth the widget was drawn with, on
                    // exactly the terms the finger is: see the touch
                    // path above, which is where that arithmetic is
                    // explained.
                    if chat.is_typing() {
                        if button == MouseButton::Left && pressed {
                            if let Some(at) = last_cursor {
                                let grown = widgets::Layout::for_screen(
                                    graphics.aspect(),
                                    graphics.ui_scale(),
                                )
                                .fit_from_corner(chat::EXTENT);
                                let lifted = (
                                    at.0,
                                    at.1 - chat::keyboard_lift(
                                        touch_controls,
                                        true,
                                        grown,
                                    ),
                                );
                                let authored = widgets::unscale_about(
                                    lifted,
                                    widgets::anchor::BOTTOM_LEFT(graphics.aspect()),
                                    grown,
                                );
                                if let Some(chat::Tap::Caret(caret)) =
                                    chat.tapped(graphics.aspect(), touch_controls, authored)
                                {
                                    if !window.ime_owns_text() {
                                        chat.place_caret(caret);
                                    }
                                }
                            }
                        }
                        return;
                    }
                    // **A release matters on exactly one screen.** The
                    // arrangement editor is the only place in the menus
                    // where a press and a release are different events:
                    // everywhere else a click is decided when the button
                    // goes down. Letting go is how a control is put
                    // down, so it cannot be dropped here with the rest.
                    if !pressed {
                        if button == MouseButton::Left {
                            menu.release_control();
                        }
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
                    // The journal has the cursor while it is open, so it
                    // has the click: a button, a row, or the start of a
                    // drag across the map.
                    if journal.is_open() && net.is_some() && !paused {
                        if button == MouseButton::Left {
                            if pressed {
                                let mark = player_mark(player.position.as_vec3(), camera.yaw);
                                if journal.press(graphics.aspect(), mark) == ui::journal::Outcome::Closed {
                                    audio.play(audio::Sfx::Click);
                                    grab_cursor(&window, &mut input);
                                }
                                send_journal_command(&mut journal, net.as_ref(), &mut debug_stats);
                            } else {
                                journal.release();
                            }
                        }
                        return;
                    }
                    // The station screen, before the chest's: the two are
                    // never open at once, and this one wants the click
                    // wherever it lands (a blow is not aimed).
                    if station_screen.is_open() && !paused && button == MouseButton::Left {
                        // The same blow the strike key makes, and the same
                        // reason it is asked first: while a run is up, the
                        // whole panel is the hammer.
                        let blow = station_screen.striking();
                        let clicked = station_screen.click();
                        if let Some(game) = blow {
                            audio.play_flat(audio::bank::station_blow(game), 0.7, 1.0);
                        }
                        if let Some(intent) = clicked {
                            if intent == station_screen::Intent::Close {
                                audio.play(audio::Sfx::Back);
                                close_station(&mut station_screen, net.as_ref(), &mut debug_stats);
                                return;
                            }
                            if let Some(net) = net.as_ref() {
                                send_station_intent(
                                    intent,
                                    &mut station_screen,
                                    net,
                                    &mut debug_stats,
                                    &audio,
                                );
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
                            let quick = thumb_quick
                                || input.action_down(
                                    &settings.keybinds,
                                    keybinds::Action::Sprint,
                                );
                            let intent = chest_screen.click(&inventory, click, quick, held_ctrl);
                            // Closing goes through `close_chest`, which
                            // also tells the server to stop sending
                            // updates for the container. Handled before
                            // the arm below so that it still works when
                            // the connection has gone: a screen that
                            // traps the player when the server drops is
                            // the worst time to trap them.
                            if matches!(intent, Some(chest_screen::Intent::Close)) {
                                audio.play(audio::Sfx::Back);
                                close_chest(&mut chest_screen, net.as_ref(), &mut debug_stats);
                                return;
                            }
                            if let (Some(intent), Some(net)) = (intent, net.as_ref()) {
                                use chest_screen::Intent;
                                // **A jug in the hand is not a container
                                // the server has open**, so none of the
                                // container messages below mean anything
                                // for it: every gesture on it becomes one
                                // of the pack's own jug messages, against
                                // the jug's slot. See
                                // `chest_screen::held_vessel_message`.
                                if let Some(jug) = chest_screen.held_vessel() {
                                    if let Some(message) =
                                        chest_screen::held_vessel_message(intent, jug)
                                    {
                                        audio.play(audio::Sfx::Click);
                                        net.send(message);
                                        debug_stats.network_messages_out_this_second += 1;
                                    }
                                    return;
                                }
                                audio.play(audio::Sfx::Click);
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
                                    Intent::MoveKind(side, slot) => ClientMessage::ChestMoveKind {
                                        side,
                                        slot: slot as u8,
                                    },
                                    Intent::Sort => ClientMessage::SortChest,
                                    // Dealt with above, before the
                                    // connection was unwrapped.
                                    Intent::Close => unreachable!(
                                        "closing a container is handled before the server is asked"
                                    ),
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
                            let quick = thumb_quick
                                || input.action_down(
                                    &settings.keybinds,
                                    keybinds::Action::Sprint,
                                );
                            let intent = inventory_screen.click(&inventory, click, quick);
                            // Closing is the screen's own business and
                            // needs no server: handled here, before the
                            // arm below that needs a connection, because
                            // a way out that stops working when the
                            // connection drops is the wrong way out.
                            if matches!(intent, Some(inventory_screen::Intent::Close)) {
                                audio.play(audio::Sfx::Click);
                                inventory_screen.close();
                                grab_cursor(&window, &mut input);
                                return;
                            }
                            if let (Some(intent), Some(net)) = (intent, net.as_ref()) {
                                use inventory_screen::Intent;
                                // Three sounds for a dozen gestures, and
                                // the split is by what the gesture *is*
                                // rather than by which message it sends:
                                // putting armour on is a different event
                                // from moving a stack, and making
                                // something is a different event again.
                                audio.play(match &intent {
                                    // A dressing going on is a thing put on
                                    // the body, which is what the equip
                                    // sound already is.
                                    Intent::Equip(_) | Intent::Unequip(_) | Intent::Treat { .. } => {
                                        audio::Sfx::Equip
                                    }
                                    // **A workshop is heard as itself.**
                                    // The saw at the bench, the chisel at
                                    // the mason's block, the wheel, the
                                    // knife at the currier's -- so that
                                    // carrying a bench into the woods is
                                    // audible and not just a row in the
                                    // menu turning white. Anything made
                                    // in the hands or at a fire keeps the
                                    // generic craft sound; see
                                    // `bank::workshop_of`.
                                    Intent::Craft { index, .. } => {
                                        primitive_shared::crafting::RECIPES
                                            .get(*index)
                                            .and_then(|recipe| audio::bank::workshop_of(recipe.station))
                                            .unwrap_or(audio::Sfx::Craft)
                                    }
                                    _ => audio::Sfx::Click,
                                });
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
                                    Intent::Equip(slot) => {
                                        ClientMessage::Equip { slot: slot as u8 }
                                    }
                                    Intent::Unequip(slot) => {
                                        ClientMessage::Unequip { slot: slot as u8 }
                                    }
                                    Intent::PourIntoJug { from, jug } => {
                                        ClientMessage::PourIntoJug {
                                            from: from as u8,
                                            jug: jug as u8,
                                        }
                                    }
                                    Intent::EmptyJug(slot) => {
                                        ClientMessage::EmptyJug { slot: slot as u8 }
                                    }
                                    Intent::Treat { slot, part } => ClientMessage::TreatInjury {
                                        slot: slot as u8,
                                        part: part.index() as u8,
                                    },
                                    Intent::Craft { index, times } => ClientMessage::Craft {
                                        index: index as u16,
                                        times,
                                    },
                                    // Dealt with above, before the
                                    // connection was unwrapped: closing
                                    // a screen is not something to tell
                                    // a server about.
                                    Intent::Close => unreachable!(
                                        "closing the pack is handled before the server is asked"
                                    ),
                                });
                                debug_stats.network_messages_out_this_second += 1;
                            }
                        }
                        return;
                    }
                    if net.is_none() || paused {
                        if button == MouseButton::Left {
                            // A press that lands on none of the
                            // screen's own buttons, on the arrangement
                            // screen, is a press on a control -- so it
                            // is offered to the controls only after the
                            // buttons have had their say.
                            let arranging = menu.is_arranging();
                            // **The editor is read before the click is
                            // acted on, and that ordering is the whole
                            // of it.** The platform's copy of the field
                            // is *polled* once a frame, not delivered as
                            // an event, so a character committed after
                            // the last poll and before this press is
                            // still sitting in the editor when the press
                            // arrives -- and the press may be the one
                            // that moves the focus to the next box.
                            // Reconciled afterwards, that character is
                            // either read into the box the player has
                            // just tapped or thrown away; reconciled
                            // here, it lands in the field it was typed
                            // into. A phone is where this happens: the
                            // last letter of a world's name, and then a
                            // finger on the seed.
                            reconcile_the_editor(&mut ime, &window, &mut menu, &mut chat, traced);
                            let clicked = menu.click();
                            if clicked.is_none()
                                && arranging
                                && menu.grab_at_cursor(settings.ui_scale)
                            {
                                return;
                            }
                            if let Some(action) = clicked {
                                // Going back sounds different from going
                                // in: one clip is a rising interval and
                                // the other a falling one, and that is
                                // the cheapest way there is to tell a
                                // player which direction they just
                                // moved.
                                audio.play(match action {
                                    Action::Back | Action::Cancel => audio::Sfx::Back,
                                    _ => audio::Sfx::Click,
                                });
                                handle_action! { action }
                            }
                        }
                        return;
                    }
                    // Never while a screen that wants the pointer is up:
                    // a click there taking the mouse for the camera is
                    // what left the map undraggable. See `Journal::press`.
                    if !input.mouse_grabbed
                        && !window.is_touch_primary()
                        && !journal.is_open()
                        && !inventory_screen.open
                        && !chest_screen.is_open()
                            && !station_screen.is_open()
                    {
                        grab_cursor(&window, &mut input);
                        // The click that grabs the cursor is not also a
                        // swing at whatever happens to be under the
                        // crosshair.
                        input.breaking = false;
                    // **Never on a phone**, where there is no pointer to
                    // capture and this branch would only eat the first
                    // tap of every session -- and, since the hands moved
                    // into the look area, that tap is a block the player
                    // asked to place. `set_cursor_grabbed` answers
                    // "granted" on Android without doing anything, so
                    // the flag is normally true and this was normally
                    // skipped; normally is not a guarantee, and the cost
                    // of the exception is one silently lost action.
                    } else if button == MouseButton::Right {
                        // A block you can *open* takes the right click
                        // before a block you could place does. Otherwise
                        // the only way to use a chest with something in
                        // hand would be to empty your hand first, and
                        // the block would go on the front of it.
                        // ...with one exception, and it is the fire.
                        // A hearth is a container now, so a right click
                        // opens it -- but *striking flint on it* is
                        // still a gesture, and it is the one gesture
                        // that is not about what is inside. Holding the
                        // striker means lighting; holding anything else
                        // means opening.
                        //
                        // Which of the four this gesture is, decided in
                        // one place so the order can be checked by a
                        // test rather than read down the page. See
                        // `UseGesture`.
                        // **A raft takes the click before anything behind it**:
                        // the timber is in front of the lake, and a rower
                        // reaching for the sail is not reaching for a drink.
                        // What it does -- the oars, or the sail -- is the
                        // server's to decide (`ClientMessage::UseRaft`). A tap
                        // on the glass is this same right click, so a phone
                        // takes the oars and raises the sail the same way.
                        if let (Some((raft, distance)), Some(net)) = (
                            entities.aimed_raft(camera.position, camera.forward(), INTERACT_RANGE),
                            net.as_ref(),
                        ) {
                            if physics::raycast_block(&chunks, camera.position, camera.forward(), distance)
                                .is_none()
                            {
                                net.send(ClientMessage::UseRaft { raft });
                                debug_stats.network_messages_out_this_second += 1;
                                return;
                            }
                        }
                        let held = inventory.block_in(input.hotbar_slot);
                        // **A horse takes a click with nothing to tend it in
                        // hand**: a leg up, or -- with the rein key held --
                        // a hand into its saddlebags. Whether it will have
                        // you is the server's (`ClientMessage::Mount`), and a
                        // wild horse says so; feed, a saddle or the bags in
                        // hand go on to the tending below instead.
                        if let Some(net) = net.as_ref() {
                            let tending = held.is_some_and(primitive_shared::husbandry::is_tending_tool);
                            let aimed = entities.aimed_at(camera.position, camera.forward(), INTERACT_RANGE);
                            if let Some((horse, distance)) = aimed.filter(|_| !tending && entities.horseback.is_none()) {
                                if entities.species_of(horse) == Some(primitive_shared::animals::Species::Horse)
                                    && physics::raycast_block(&chunks, camera.position, camera.forward(), distance).is_none()
                                {
                                    let bags = input.action_down(&settings.keybinds, keybinds::Action::Rein);
                                    net.send(if bags {
                                        ClientMessage::OpenBags { horse }
                                    } else {
                                        ClientMessage::Mount { horse }
                                    });
                                    debug_stats.network_messages_out_this_second += 1;
                                    return;
                                }
                            }
                        }
                        // **An animal takes the click next, with something to
                        // tend it with in hand** -- feed, a knife, a bowl
                        // (`husbandry::is_tending_tool`) -- and nothing else
                        // does: a player walking planks past a sheep is
                        // building, not asking it anything. What the click
                        // does is the server's (`ClientMessage::TendAnimal`).
                        if let (Some(held), Some(net)) = (held, net.as_ref()) {
                            if primitive_shared::husbandry::is_tending_tool(held) {
                                if let Some((animal, distance)) =
                                    entities.aimed_at(camera.position, camera.forward(), INTERACT_RANGE)
                                {
                                    let is_animal = primitive_shared::protocol::entity_source(animal)
                                        == Some(primitive_shared::protocol::EntitySource::Animal);
                                    if is_animal
                                        && physics::raycast_block(&chunks, camera.position, camera.forward(), distance)
                                            .is_none()
                                    {
                                        net.send(ClientMessage::TendAnimal { animal });
                                        debug_stats.network_messages_out_this_second += 1;
                                        return;
                                    }
                                }
                            }
                        }
                        let aimed = aimed_block(&chunks, &camera);
                        let mut claim = use_gesture(aimed.map(|(_, block)| block), held);
                        // **Set down, with the modifier held**: anything not
                        // built with, one at a time, on the top of a block --
                        // the jug's gesture (`UseGesture::OpenVessel`) for
                        // everything else a hand carries. First, before the
                        // food is eaten and the knife scores a trunk: the
                        // modifier is the player saying "not that". The cost
                        // is the jug's too: the modifier is the sprint key,
                        // so a player running with bread who right-clicks
                        // lays the loaf down; the eat key still eats.
                        //
                        // Not at a thing already set down, which the plain
                        // click takes back (`use_gesture`): a knife laid on a
                        // knife has nothing to lie on.
                        // ...and not a torch held to a fire: Shift and the
                        // click there light it, as the plain click does. A
                        // torch laid on a burning campfire is a torch nobody
                        // meant to put down.
                        let setting_down = held.is_some_and(primitive_shared::types::can_be_set_down)
                            && !aimed.is_some_and(|(_, block)| primitive_shared::types::is_set_down(block))
                            && claim != UseGesture::Hearth
                            && (thumb_quick
                                || input.action_down(&settings.keybinds, keybinds::Action::Sprint));
                        if setting_down {
                            if let Some(net) = net.as_ref() {
                                match set_down_cell(&chunks, &camera) {
                                    Some(cell) => {
                                        net.send(ClientMessage::SetDown {
                                            global_x: cell.0,
                                            global_y: cell.1,
                                            global_z: cell.2,
                                        });
                                        debug_stats.network_messages_out_this_second += 1;
                                    }
                                    // Said here, in the player's language, and
                                    // never sent: the server would refuse it
                                    // in English.
                                    None => {
                                        notice = Some((
                                            settings.language.text(ui::lang::Msg::SetDownWhere).to_string(),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            }
                            return;
                        }
                        // **A rod in hand takes the press out of this path
                        // altogether.** The throw is a hold and a release and
                        // the strike is a tap, both of them driven off the
                        // button's *state* once a frame (`rod_hold` below),
                        // because a press here is an event and a wind-up is a
                        // duration. Nothing is sent from here with a rod in
                        // hand, and nothing must be: this is where a cast
                        // used to turn into a `UseBlock`, and leaving it
                        // would mean every throw also drank the lake.
                        if held.is_some_and(|held| {
                            primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_FISHING_ROD
                        }) {
                            return;
                        }
                        // **Fishing: what is said instead of a reach into an
                        // empty trap**, in the player's language, from the
                        // survey the server makes too -- and never sent. See
                        // `logic::fishing`.
                        if let Some((cell, block)) = aimed {
                            if held.is_none() {
                                if let Some(said) =
                                    logic::fishing::trap_notice(|x, y, z| chunks.block_at(x, y, z), cell, block)
                                {
                                    notice = Some((settings.language.text(said.msg()).to_string(), Instant::now()));
                                    return;
                                }
                            }
                        }
                        // **Fires in the ground, where the world decides.**
                        // `use_gesture` answers a pit kiln or a pile by its
                        // block; these two need the cells round the aim --
                        // pottery at the floor of an empty pit, and flint
                        // at the sticks and log a firepit is laid from --
                        // so they are asked here, and only of what would
                        // otherwise be a placement. See `ground_fire_claim`.
                        if claim == UseGesture::Place && ground_fire_claim(&chunks, &entities, aimed, held) {
                            claim = UseGesture::Pit;
                        }
                        // ...and a log laid with the modifier held, which is
                        // TerraFirmaCraft's log pile: shift and a right
                        // click. Into the cell a placement would have used.
                        if claim == UseGesture::Place
                            && held.is_some_and(primitive_shared::pit::is_log)
                            && (thumb_quick
                                || input.action_down(&settings.keybinds, keybinds::Action::Sprint))
                        {
                            if let (Some((_, before)), Some(net)) = (
                                physics::raycast_block(&chunks, camera.position, camera.forward(), INTERACT_RANGE),
                                net.as_ref(),
                            ) {
                                net.send(ClientMessage::PileLog {
                                    global_x: before.0,
                                    global_y: before.1,
                                    global_z: before.2,
                                });
                                debug_stats.network_messages_out_this_second += 1;
                                return;
                            }
                        }
                        // **A door swings here first, and is told to the
                        // server after.** Everything else a right click does
                        // waits for the server's answer; a door that waited
                        // a round trip would stick on every server further
                        // away than the next room, and the player walking
                        // through it would walk into it. The server swings
                        // the same two cells (`swing_door`) and puts a door
                        // it refused back the way it hangs, so what this
                        // predicts is only ever corrected, never kept wrong.
                        if let (UseGesture::Swing, Some((cell, block)), Some(net)) =
                            (claim, aimed, net.as_ref())
                        {
                            let swung = door_swing(&chunks, cell, block);
                            let opening = primitive_shared::types::door_is_open(swung[0].block_id);
                            audio.play_at_block(
                                if opening { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose },
                                cell,
                                0.9,
                                1.0,
                            );
                            for change in swung {
                                apply_change(
                                    &mut chunks,
                                    &mut light,
                                    &mut arrivals,
                                    &mut urgent,
                                    &mut dirty_set,
                                    &mut chunk_versions,
                                    change,
                                );
                            }
                            net.send(ClientMessage::UseBlock {
                                global_x: cell.0,
                                global_y: cell.1,
                                global_z: cell.2,
                            });
                            debug_stats.network_messages_out_this_second += 1;
                            return;
                        }
                        // The anvil and the wheel: a question, like a chest's,
                        // and the screen opens when the answer comes back. The
                        // server decides whether there is a hammer in the hand
                        // and how wide the sweet spot is, so a client cannot
                        // open a forgiving anvil for itself.
                        if let (UseGesture::Station, Some((cell, _)), Some(net)) =
                            (claim, aimed, net.as_ref())
                        {
                            net.send(ClientMessage::OpenStation {
                                global_x: cell.0,
                                global_y: cell.1,
                                global_z: cell.2,
                            });
                            station_screen.asked_to_open();
                            debug_stats.network_messages_out_this_second += 1;
                            return;
                        }
                        if let (UseGesture::Open, Some((cell, _)), Some(net)) =
                            (claim, aimed, net.as_ref())
                        {
                            net.send(ClientMessage::OpenChest {
                                global_x: cell.0,
                                global_y: cell.1,
                                global_z: cell.2,
                            });
                            // A new question: whatever it answers is
                            // wanted, even about a chest just shut.
                            chest_screen.asked_to_open();
                            debug_stats.network_messages_out_this_second += 1;
                            // The screen opens when the answer arrives.
                            // Everything else waits for that, including
                            // the cursor -- see the hand-off in the frame
                            // loop.
                            return;
                        }
                        // ...and a fire takes it before a placement too,
                        // for exactly the same reason a chest does: the
                        // only way to light one otherwise would be to
                        // empty your hand first, and the flint you were
                        // holding would go on the ground beside it.
                        //
                        // What the gesture *does* is the server's
                        // business entirely -- strike a spark, or feed
                        // the fire what is in your hand -- so the
                        // message carries neither the effect nor the
                        // item. See `ClientMessage::UseBlock`.
                        //
                        // A carcass goes the same way: the message says
                        // *which cell*, and the server decides from what
                        // it believes is in the hand whether that is a
                        // cut, a ruined skin, or a hint about needing a
                        // knife.
                        //
                        // Water joins them for the same reason: what a
                        // click at a river does -- a mouthful, a filled
                        // jug, or a warning that the sea is salt -- is
                        // decided from the vitals and the water's kind,
                        // and both live on the server.
                        //
                        // A pick goes the same way: whether there is room
                        // in the pack for the apple is the server's
                        // answer, and a client that predicted the leaves
                        // bare would show a tree picked into a full pack.
                        // **A cut takes the knife's time.** Held here and
                        // sent when it is done (`Cut`), not sent on the
                        // click; a second click while cutting is nothing.
                        if let (UseGesture::Butcher, Some((cell, block))) = (claim, aimed) {
                            if cut.is_none() {
                                cut = Some(Cut { cell, block, slot: input.hotbar_slot, started: Instant::now() });
                                hand.strike(held);
                            }
                            return;
                        }
                        if let (
                            UseGesture::Hearth
                            | UseGesture::Butcher
                            | UseGesture::Water
                            | UseGesture::Pick
                            | UseGesture::Tend
                            | UseGesture::Rest
                            | UseGesture::Pit,
                            Some((cell, _)),
                            Some(net),
                        ) = (claim, aimed, net.as_ref())
                        {
                            // The swallow, heard when the hand moves. See
                            // `swallow_expected` for why the client
                            // guesses at a sound it cannot be told.
                            if claim == UseGesture::Water
                                && swallow_expected(
                                    aimed.map(|(_, block)| block),
                                    held,
                                    body.hydration,
                                )
                            {
                                audio.play(audio::Sfx::Drink);
                            }
                            net.send(ClientMessage::UseBlock {
                                global_x: cell.0,
                                global_y: cell.1,
                                global_z: cell.2,
                            });
                            debug_stats.network_messages_out_this_second += 1;
                            return;
                        }
                        // **Food in the hand is eaten by using it.**
                        // The player asked for this in as many words:
                        // "сделай возможность есть взяв в руку, а не
                        // через HUD". Eating hung on a key and, on a
                        // phone, on resting a finger on the hotbar slot
                        // -- a gesture on the interface for something
                        // the hand does. Both of those stay; this is the
                        // one the hand already makes.
                        if claim == UseGesture::Eat {
                            eat_from(
                                Some(input.hotbar_slot),
                                &inventory,
                                &mut meal,
                                &mut hand,
                            );
                            return;
                        }
                        // **A jug in the hand opens**, unless the
                        // modifier is held -- which is how one is set
                        // down now. See `UseGesture::OpenVessel`. Nothing
                        // goes to the server: the jug and what is in it
                        // are already in the pack snapshot.
                        if claim == UseGesture::OpenVessel {
                            let setting_down = thumb_quick
                                || input.action_down(
                                    &settings.keybinds,
                                    keybinds::Action::Sprint,
                                );
                            if !setting_down {
                                chest_screen.show_held_vessel(input.hotbar_slot, &inventory);
                                return;
                            }
                        }
                        // Placing is still instant; only breaking takes
                        // time. Held-to-repeat placement would need its
                        // own cooldown, and the server rate-limits edits
                        // anyway.
                        let others: Vec<glam::DVec3> = remote_players.iter_positions().collect();
                        if let Some(net) = net.as_mut() {
                            try_place_block(
                                &chunks,
                                &camera,
                                &input,
                                &player,
                                &others,
                                net,
                                &inventory,
                                &mut mining,
                                &mut debug_stats,
                            );
                        }
                    }
                }

            platform::Event::RedrawRequested => {
                    // Nothing to draw into. An Android activity that
                    // has been backgrounded is here until it is
                    // resumed; a desktop never is. Skipped rather than
                    // attempted, because `acquire` against a dead
                    // surface fails once per frame and says so once per
                    // frame.
                    if !graphics.surface_ready() {
                        return;
                    }
                    // The server ended the session last frame. Close it
                    // down and put the reason on screen, rather than
                    // leaving a frozen world or closing the game.
                    if let Some(reason) = end_session.take() {
                        net = None;
                        if let Some(server) = local_server.take() {
                            runtime.block_on(server.stop());
                        }
                        journal.end_session();
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
                        let menu_dt = (now - last_menu_frame).as_secs_f32().min(0.25);
                        menu.tick(menu_dt);
                        last_menu_frame = now;

                        // The menus have music and nothing else -- there
                        // is no body to make footsteps and no world to
                        // rain on. Volumes are pushed every frame rather
                        // than on change: three atomic stores against
                        // one more thing to remember to do whenever a
                        // settings row moves.
                        audio.set_volumes(settings.master_volume, settings.music_volume);
                        soundscape.update(
                            &audio,
                            &audio::soundscape::Frame {
                                dt: menu_dt,
                                player: &player,
                                camera: &camera,
                                chunks: &chunks,
                                sky: &sky,
                                weather,
                                health_fraction: 1.0,
                                in_world: false,
                                digging: None,
                                swinging: false,
                            },
                        );

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
                                    // Takes the menu's own patch of
                                    // world with it, which is the whole
                                    // reason this call is here rather
                                    // than only where a session ends.
                                    graphics.clear_chunk_meshes();
                                    menu_scene = None;
                                    menu_scene_asked_for = None;
                                    dirty.clear();
                                    urgent.clear();
                                    dirty_set.clear();
                                    arrivals.clear();
                                    chunk_versions.clear();
                                    chunk_lod.clear();
                                    lod_scanned_from = None;
                                    remote_players = RemotePlayers::default();
                                    entities = entities::Entities::default();
                                    // How long a server tick is, which is
                                    // what animals and other players are
                                    // played back in -- see
                                    // `Entities::apply_snapshot`. **After the
                                    // reset, not before it**: it was set a few
                                    // lines up and thrown away with the old
                                    // `Entities`, so a server ticking at any
                                    // rate but twenty had its animals eased as
                                    // though it ticked at twenty.
                                    entities.set_tick_rate(welcome.tick_rate_hz);
                                    remote_players.set_tick_rate(welcome.tick_rate_hz);
                                    my_id = welcome.your_id;
                                    world_seed = welcome.world_seed;
                                    // The preset as well as the seed:
                                    // the two together are the world,
                                    // and a client that took only the
                                    // seed would tint the test world
                                    // from noise the server never read.
                                    // ...and the zone: the same seed laid in
                                    // the tropics is another planet's worth
                                    // of climate.
                                    // ...and the scale, or an old world's
                                    // provinces are tinted from a planet
                                    // its server never drew.
                                    worldgen = primitive_shared::worldgen::WorldGen::with_scale(
                                        world_seed,
                                        welcome.preset,
                                        welcome.zone,
                                        welcome.scale,
                                    );
                                    // The mesher colours foliage from
                                    // the same generator, so it has to
                                    // learn the new seed before the
                                    // first chunk of the new world is
                                    // submitted -- a chunk meshed with
                                    // the old one would wear another
                                    // world's climate until something
                                    // happened to dirty it.
                                    mesher.set_world(
                                        primitive_shared::worldgen::WorldGen::with_scale(
                                            world_seed,
                                            welcome.preset,
                                            welcome.zone,
                                            welcome.scale,
                                        ),
                                    );
                                    sky = Sky::new(welcome.time_of_day, welcome.day_length_seconds);
                                    sky.on_time_sync(welcome.time_of_day, welcome.world_days);
                                    player = Player::new(
                                        glam::DVec3::new(welcome.spawn.0, welcome.spawn.1, welcome.spawn.2),
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
                                    // **...and everything the server only
                                    // mentions when it changes.** Asleep,
                                    // posture and breath are sent on change
                                    // against what *that* server last said,
                                    // and a fresh player on a fresh server
                                    // has said nothing -- so the new world
                                    // never corrected what the old one left
                                    // here. Lying in a bed and leaving
                                    // through the pause menu opened the next
                                    // world asleep: frozen, the camera on a
                                    // pillow that was not there, with no
                                    // message ever coming to wake it. A
                                    // connection lost at a chest opened the
                                    // next world with that chest on screen,
                                    // taking clicks for a container this
                                    // server never opened; and a breath bar
                                    // left half empty under water stayed on
                                    // the HUD, because full breath is the
                                    // one reading nobody sends.
                                    chest_screen = chest_screen::ChestScreen::new();
                                    chest_was_open = false;
                                    sleep = logic::posture::Sleep::default();
                                    resting = logic::posture::Resting::Standing;
                                    rising = logic::posture::Rising::default();
                                    looked_along_bed = false;
                                    breath = 1.0;
                                    equipment = primitive_shared::inventory::Equipment::new();
                                    notice = None;

                                    // Where this world's map is kept: beside
                                    // a singleplayer world, and under the
                                    // address, seed and name for a server
                                    // -- see `logic::map::cache_for_server`
                                    // for why all three.
                                    let map_cache = match last_attempt {
                                        Attempt::Singleplayer(index) => worlds
                                            .get(index)
                                            .map(|world| logic::map::cache_for_world(&world.directory)),
                                        Attempt::Server(index) => menu.servers.servers.get(index).map(|entry| {
                                            logic::map::cache_for_server(
                                                &entry.address,
                                                session.connection.welcome.world_seed,
                                                &settings.username,
                                            )
                                        }),
                                        Attempt::None => None,
                                    };
                                    journal.begin_session(map_cache);
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

                        // **The backdrop, and the only place in the
                        // client that makes world without being told
                        // to.** Started the first frame the menu is on
                        // screen with the setting on, rebuilt when the
                        // player changes which place they want, and
                        // dropped -- meshes and all -- when they switch
                        // it off. The switch has to reach the card as
                        // well as the flag: the geometry is the
                        // renderer's now, so forgetting to clear it
                        // would leave a shore behind a menu that says
                        // the backdrop is off.
                        //
                        // **And only while there is still no session.**
                        // The connection above completes *inside* this
                        // block -- the `net.is_none()` that opened it
                        // was tested at the top of the frame -- so on
                        // the one frame a world opens, `menu_scene` has
                        // just been cleared and `sky` has just been set
                        // to the server's clock, and this would build a
                        // fresh backdrop over the top of both and put
                        // `menu_scene::TIME_OF_DAY` back. What a player
                        // saw was a world that opened at dusk and
                        // jumped to morning a second later, when the
                        // first `TimeSync` landed and `Sky::on_time_sync`
                        // snapped a delta too big to be drift. What the
                        // machine did was mesh a patch of world nobody
                        // would ever see, in the frame the real one
                        // started streaming.
                        if settings.menu_background && net.is_none() {
                            if menu_scene.is_some()
                                && menu_scene_asked_for != settings.menu_background_place()
                            {
                                menu_scene = None;
                                graphics.clear_chunk_meshes();
                            }
                            let scene = match menu_scene.as_mut() {
                                Some(scene) => scene,
                                None => {
                                    menu_scene_asked_for = settings.menu_background_place();
                                    // Dusk, and fixed: the veil over the
                                    // scene is measured against how
                                    // bright the scene can get, so the
                                    // menu's clock does not run. See
                                    // `menu_scene::TIME_OF_DAY`.
                                    sky = Sky::new(menu_scene::TIME_OF_DAY, 900.0);
                                    let fresh = menu_scene::MenuScene::spawn(
                                        menu_scene_asked_for,
                                        menu_scene_roll(),
                                        graphics.textures.face_layers(),
                                    );
                                    // Printed because a backdrop that
                                    // came out wrong is otherwise a
                                    // report with nothing in it: the
                                    // place and the seed are between
                                    // them the whole of what was
                                    // chosen, and `look_for` is pure,
                                    // so those two numbers reproduce
                                    // the picture exactly.
                                    menu_scene_started = Some(Instant::now());
                                    let spot = fresh.spot();
                                    println!(
                                        "menu backdrop: {} at {}, {} in seed {}",
                                        spot.place.name(),
                                        spot.eye.x.round(),
                                        spot.eye.z.round(),
                                        spot.seed,
                                    );
                                    menu_scene.insert(fresh)
                                }
                            };
                            scene.tick(menu_dt);
                            // Landing a mesh is a copy to the card, so
                            // it is rationed here exactly as it is for
                            // the streamed world -- twenty-five of them
                            // in the frame they all happen to be ready
                            // in is a hitch on the one screen where
                            // nothing else is going on.
                            let landing = Instant::now();
                            let budget =
                                Duration::from_secs_f32(settings.mesh_budget_ms / 1000.0);
                            while let Some(built) = scene.poll() {
                                if built.buffers.indices.is_empty() {
                                    graphics.drop_chunk_mesh(built.pos);
                                } else {
                                    graphics.set_chunk_mesh(built.pos, &built.buffers);
                                }
                                if landing.elapsed() >= budget {
                                    break;
                                }
                            }
                            camera.position = scene.eye().as_dvec3();
                            camera.yaw = scene.yaw();
                            camera.pitch = scene.pitch();
                            camera.aspect = graphics.aspect();
                            camera.fov_y_radians = settings.fov_degrees.to_radians();
                            render_origin = render_origin_for(camera.position, render_origin);
                        } else if menu_scene.take().is_some() {
                            graphics.clear_chunk_meshes();
                            menu_scene_asked_for = None;
                        }
                        let scene_behind = menu_scene.as_ref().map(|s| s.spot().place);

                        // **`PRIMITIVE_SHOT` on the menu, which it did
                        // not use to answer.** The shot below the world
                        // frame waits for the terrain to arrive, which
                        // is measured from `world_ready_since` -- and a
                        // menu never has one, so asking for a picture of
                        // the opening screen quietly produced nothing.
                        // That was fine while the menu was a colour;
                        // there is a world behind it now, it takes a
                        // fraction of a second to arrive, and it is a
                        // thing that has to be looked at on a device
                        // where nobody can drive the menu by hand.
                        //
                        // Measured from the scene being asked for, for
                        // the same reason the world's is measured from
                        // the world arriving: what makes the picture
                        // worth having is that the backdrop is in it.
                        if let (Some(shot), Some(started)) =
                            (&pending_shot, menu_scene_started)
                        {
                            if started.elapsed().as_secs_f32() >= shot.after {
                                graphics.request_shot(shot.path.clone());
                                pending_shot = None;
                            }
                        }
                        // Rebuilt only when something on it changed --
                        // an idle menu is the stillest screen in the
                        // game, and it used to be relaid and re-uploaded
                        // every frame. See `UiKey`.
                        // The arrangement screen is the one that needs
                        // pixels: a thumb control is sized against the
                        // physical screen, not against interface space.
                        menu.set_screen_size(graphics.size.width, graphics.size.height);
                        // **Applied while the player watches.** Moving a
                        // control and only seeing it land after leaving
                        // the screen is arranging blind. `resize` is
                        // free when nothing changed -- it compares the
                        // arrangement it was last given -- so this costs
                        // nothing on every other frame.
                        if menu.is_arranging() {
                            let wanted = menu.arrangement();
                            if !wanted.same_as(&settings.touch_layout) {
                                settings.touch_layout = wanted;
                                touch_layout = wanted;
                                touch.resize(graphics.size, touch_layout, graphics.ui_scale());
                                arrangement_unsaved = true;
                            }
                        } else {
                            // **Kept in step while the screen is shut**,
                            // so that opening it starts from what the
                            // player actually has rather than from what
                            // the game shipped. Done here rather than in
                            // the action that opens the screen, because
                            // the menu has no settings of its own to
                            // read and handing it a stale copy once is
                            // how an editor comes to be editing a
                            // layout nobody is using.
                            menu.begin_arranging(settings.touch_layout);
                            if arrangement_unsaved {
                                // Written down once, on the way out,
                                // rather than on every frame of a drag:
                                // a settings file rewritten sixty times
                                // a second while a thumb moves is a lot
                                // of disk for one decision.
                                arrangement_unsaved = false;
                                if let Err(e) = settings.save() {
                                    eprintln!("could not save the arrangement: {e}");
                                }
                            }
                        }
                        let ui_rebuilt = {
                            let ctx = menu_context(&settings, &worlds, &graphics, scene_behind);
                            let key =
                                UiKey::menu_only(menu.ui_key(&ctx), graphics.aspect());
                            let changed = ui_key.as_ref() != Some(&key);
                            if changed {
                                ui_key = Some(key);
                                ui_vertices.clear();
                                // Not grown afterwards: the menu is the
                                // one screen that *lays itself out* for
                                // the window rather than being drawn for
                                // a desktop and multiplied. See
                                // `widgets::Layout`, and `MenuContext`,
                                // which carries it.
                                menu.build_into(&ctx, &mut ui_vertices);
                            }
                            changed
                        };
                        // Full health: there is no player behind the
                        // menu to be hurt.
                        let params = frame_params(
                            &settings,
                            &sky,
                            render_distance,
                            // With a scene behind the menu the fog *is*
                            // the horizon: the patch is eighty blocks
                            // across and ends in mid-air, so the fade
                            // has to finish inside it or the backdrop
                            // reads as a diorama on a table. With no
                            // scene there is nothing streamed for the
                            // fog to run past.
                            if scene_behind.is_some() {
                                menu_scene::view_radius_blocks()
                            } else {
                                f32::INFINITY
                            },
                            // ...and it is on whatever the player set
                            // it to, for the same reason. Fog off is a
                            // choice about a world you walk through,
                            // where the alternative to haze is seeing
                            // further; here the alternative is seeing
                            // the edge of the ground.
                            scene_behind.is_some(),
                            if scene_behind.is_some() { render_origin } else { Vec3::ZERO },
                            Eye {
                                underwater: false,
                                // The backdrop is a hillside in the open
                                // air, and there is no eye in a world to
                                // ask about a roof over it.
                                underground: 0.0,
                                smoke: 0.0,
                                health_fraction: 1.0,
                                // Nobody is holding anything behind a menu.
                                held: None,
                            },
                        );
                        // The menus are not a place anyone is aiming,
                        // so the wait stays where it always was: right
                        // before the draw. Only the world frame below
                        // pays to move it.
                        let frame = match graphics.acquire() {
                            Ok(frame) => frame,
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                graphics.resize(graphics.size);
                                return;
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => {
                                elwt.exit();
                                return;
                            }
                            Err(e) => {
                                eprintln!("render error: {e:?}");
                                return;
                            }
                        };
                        // No world, so no particles and no hand:
                        // there is nobody standing behind the menu
                        // holding anything, and nothing is raining on
                        // them. Every mesh field defaults to `None`.
                        graphics.render(
                            frame,
                            &camera,
                            &params,
                            &Scene {
                                hotbar: &ui_vertices,
                                ui_changed: ui_rebuilt,
                                ..Default::default()
                            },
                        );

                        // **`PRIMITIVE_BENCH` on the menu, which used to
                        // hang there.** The world's bench clock starts
                        // at the first frame with terrain in front of it
                        // and there is never one here, so a benched run
                        // that stayed on the menu ran until somebody
                        // killed it -- and that is now the run somebody
                        // wants, because the menu draws a world.
                        //
                        // Only when nothing is on its way into a
                        // session: with `PRIMITIVE_AUTOSTART` the menu
                        // is on screen for a second or two while the
                        // local server comes up, and ending the run
                        // there would turn every world benchmark into a
                        // measurement of the loading screen.
                        if let Some(seconds) = bench_seconds {
                            if pending_connect.is_none() && last_attempt == Attempt::None {
                                let since = *menu_bench_started.get_or_insert_with(Instant::now);
                                menu_frames += 1;
                                let ran = since.elapsed().as_secs_f32();
                                if ran >= seconds {
                                    println!(
                                        "bench: {menu_frames} menu frames in {ran:.1}s                                          ({:.0} fps, {:.2} ms a frame), backdrop {}",
                                        menu_frames as f32 / ran,
                                        ran * 1000.0 / menu_frames as f32,
                                        match scene_behind {
                                            Some(place) => place.name(),
                                            None => "off",
                                        },
                                    );
                                    elwt.exit();
                                }
                            }
                        }
                        return;
                    }

                    // Past this point there is a connection; the early
                    // return above is what guarantees it.
                    let net = net.as_mut().expect("connected");

                    // The frame's one deliberate block, and it is first
                    // on purpose. Under vsync this sleeps until the
                    // display is ready for another image; doing that
                    // here means the mouse is read *after* the sleep
                    // rather than before it, so what gets drawn is
                    // where the player is pointing now and not where
                    // they were pointing a frame ago. See
                    // `GraphicsState::acquire`.
                    let frame = match graphics.acquire() {
                        Ok(frame) => frame,
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            // The swapchain went stale -- a resize, or
                            // the window moving to another monitor.
                            // Rebuild it and let the next frame have
                            // the image; there is nothing to draw into
                            // this one.
                            graphics.resize(graphics.size);
                            return;
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            elwt.exit();
                            return;
                        }
                        Err(e) => {
                            eprintln!("render error: {e:?}");
                            return;
                        }
                    };

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
                        &mut particles,
                        &mut health,
                        &mut max_health,
                        &mut recent_health,
                        &mut breath,
                        &mut smoke,
                        &mut nourishment,
                        &mut body,
                        &mut sleep,
                        &mut resting,
                        &mut equipment,
                        &mut weather,
                        &mut shake,
                        &mut stamina,
                        &mut death,
                        &mut chest_screen,
                        &mut station_screen,
                        &mut notice,
                        settings.language,
                        &mut chat,
                        &mut world_ready,
                        &audio,
                        &mut soundscape,
                        &mut menu,
                        &mut journal,
                        &mut fishing_float,
                    );

                    // **A cairn the server has just agreed to asks for its
                    // name**, in the chat box (see `Chat::open_naming` for
                    // why that box). Not over a screen the player has open
                    // or a line they are typing: the heap is a mark on the
                    // map without a name, and the survey has it already.
                    if let Some(cell) = mining.take_piled_cairn() {
                        if !paused
                            && !chat.is_typing()
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !death.is_open()
                        {
                            let current = journal.explored.mark_name(cell).unwrap_or("").to_string();
                            chat.open_naming(cell, &current, Instant::now());
                            window.set_ime_visible(true);
                            release_cursor(&window, &mut input);
                            input.release_all();
                        }
                    }

                    // Dying and coming back are the two moments the
                    // cursor changes hands without the player pressing
                    // anything, and both of them arrive as a message
                    // rather than as an event -- so the hand-off is done
                    // here, once, on the frame the answer changes.
                    // The dark a sleeper's screen goes, and the morning it
                    // lifts on. The morning is said once the dark has gone
                    // rather than when the server wakes the player, or the
                    // line would spend its three seconds printed on black --
                    // and only to a player still lying there: one a blow
                    // woke is on their feet by now and has no morning to be
                    // told about.
                    if sleep.tick(dt) && matches!(resting, logic::posture::Resting::Lying { .. }) {
                        notice = Some((
                            settings.language.text(ui::lang::Msg::SleepMorning).to_string(),
                            Instant::now(),
                        ));
                    }

                    death.tick(dt);
                    if death.is_open() != was_dead {
                        was_dead = death.is_open();
                        if was_dead {
                            // One screen at a time, and this one is not
                            // optional.
                            chat.close();
                            inventory_screen.close();
                            chest_screen.close();
                            station_screen.close();
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
                        // The lid, on the frame the answer changes --
                        // which is when the server says so, not when the
                        // player clicked.
                        if chest_was_open {
                            chest_lid = chest_screen.at().and_then(|(x, y, z)| chunks.block_at(x, y, z)).is_some_and(
                                |block| primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CHEST,
                            );
                        }
                        if chest_lid {
                            audio.play(if chest_was_open { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose });
                        }
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

                    // ...and the station screen, which opens on the server's
                    // answer exactly as the chest does.
                    if station_screen.is_open() != station_was_open {
                        station_was_open = station_screen.is_open();
                        if station_was_open {
                            inventory_screen.close();
                            chat.close();
                            release_cursor(&window, &mut input);
                            input.release_all();
                        } else if !paused && !death.is_open() && !chat.is_typing() {
                            grab_cursor(&window, &mut input);
                        }
                    }
                    // A run whose last window has gone by is handed in without
                    // the player doing anything: the blows they did not strike
                    // are misses, and a screen that waited for ever for a
                    // fourth press would be a screen holding their bar.
                    if let Some(intent) = station_screen.poll() {
                        send_station_intent(intent, &mut station_screen, net, &mut debug_stats, &audio);
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
                        &mut journal.explored,
                    );
                    // **The map is a streaming phase with a ration of its
                    // own.** A quarter of what integration was given, and
                    // never more than a millisecond: surveying a chunk
                    // is a fraction of a millisecond (see
                    // `logic::map::a_survey_is_cheap_enough_to_run_during_streaming`),
                    // so this keeps up with the world arriving without
                    // ever being the reason a frame is late. A map a few
                    // frames behind the terrain is a map nobody can tell
                    // is behind.
                    journal.explored.catch_up(
                        &chunks,
                        Duration::from_secs_f32((chunk_ms * 0.25).clamp(0.2, 1.0) / 1000.0),
                    );

                    sky.tick(dt);
                    // What a line said now will be stamped with. Once a
                    // frame from the sky rather than read by the chat
                    // itself, which has no business knowing there is a
                    // sky -- and pushed rather than pulled because the
                    // stamp has to be taken at the moment the line
                    // arrives, not at the moment it is drawn.
                    chat.set_world_time(sky.time_of_day);
                    // The sky is what the weather darkens, so it is the
                    // one thing that has to be told. Everything else --
                    // the sun on the blocks, the colour behind them, the
                    // fog drawn from that colour -- follows from it. See
                    // `Sky::set_weather`.
                    sky.set_weather(weather);

                    // Which chunks are near enough to deserve their
                    // detail back, and which have fallen far enough to
                    // lose some. Only when the player has crossed into
                    // another chunk (or changed the setting): nothing
                    // else can move a chunk across a threshold, and the
                    // scan walks every loaded chunk. See `engine::lod`.
                    let player_chunk = ChunkManager::chunk_for_world_pos(
                        player.position.x,
                        player.position.z,
                    );
                    // **The quality is part of the key**, not only the
                    // distance: changing it changes what a coarse chunk
                    // is made of (the light it keeps, the grass it
                    // draws) without moving a single chunk across a
                    // threshold, so a scan keyed on distance alone
                    // would leave the world built the old way until the
                    // player walked out of the chunk they were standing
                    // in. Bumping the version of every coarse chunk is
                    // what `restripe_detail_levels` does about it.
                    let lod_key = (
                        player_chunk,
                        settings.lod_distance_chunks,
                        settings.lod_quality,
                        // The two lines a player moves from the same
                        // screen: without them in the key, a changed
                        // setting waited for the player to leave the chunk.
                        (settings.relief_chunks, settings.transparent_leaves_chunks),
                    );
                    if lod_scanned_from != Some(lod_key) {
                        let quality_changed = lod_scanned_from
                            .is_some_and(|(_, _, was, _)| was != settings.lod_quality);
                        lod_scanned_from = Some(lod_key);
                        restripe_detail_levels(
                            quality_changed,
                            &chunks,
                            player_chunk,
                            settings.lod_distance_chunks,
                            settings.relief_chunks,
                            settings.transparent_leaves_chunks,
                            &mut chunk_lod,
                            &mut chunk_versions,
                            &mut dirty,
                            &mut dirty_set,
                        );
                    }
                    dispatch_meshing(
                        &mut urgent,
                        &mut dirty,
                        &mut dirty_set,
                        &chunk_versions,
                        &mut mesher,
                        &chunks,
                        &light,
                        player_chunk,
                        settings.lod_distance_chunks,
                        settings.lod_quality,
                        settings.relief_chunks,
                        settings.transparent_leaves_chunks,
                        &mut chunk_lod,
                        mesh_ms,
                        &mut debug_stats,
                    );
                    collect_worker_results(
                        &mut mesher,
                        &mut graphics,
                        &mut chunks,
                        &mut light,
                        &mut urgent,
                        &mut dirty,
                        &mut dirty_set,
                        &chunk_versions,
                        ChunkManager::chunk_for_world_pos(player.position.x, player.position.z),
                        // The same budget the dispatch half gets, and
                        // for the same reason: both are meshing, and a
                        // burst landing in one frame is a frame the
                        // player feels. See `streaming_budget`.
                        mesh_ms,
                        &mut debug_stats,
                    );

                    // What the fingers are *still* doing, as opposed to
                    // what they just did -- which came through as events
                    // above. Read here, beside the mouse delta, because
                    // that is what these are: the stick is a direction
                    // the way held keys are, and the look drag is a
                    // mouse delta in the same pixels.
                    //
                    // Nothing happens on a desktop: no finger has ever
                    // touched `touch`, so the stick is centred, the
                    // buttons are up and the drag is zero.
                    if touch_controls {
                        touch.resize(graphics.size, touch_layout, graphics.ui_scale());
                        // **Every button is up the moment a screen takes
                        // the glass, whether or not the lift arrived.**
                        //
                        // A finger is routed by what is on screen *at
                        // the moment of the event* -- see the comment on
                        // `Event::Touch`. So a thumb that presses the
                        // button opening the inventory is delivered here
                        // as a press, the screen opens, and the lift is
                        // delivered to the menu pointer instead: this
                        // never hears the finger leave. The slot stayed
                        // in `held` for ever, and since a button became
                        // a key that means a key held down for ever --
                        // the player comes back out of the inventory
                        // still mining.
                        //
                        // Said as a fact about the world rather than as
                        // a repair: while a screen is up the world's
                        // buttons *are* all up, so this is the truth
                        // being restated every frame rather than a lift
                        // being guessed at.
                        if !world_owns_the_glass(
                            // In a world by construction: this whole
                            // branch is inside the one that unwrapped
                            // the connection.
                            true,
                            paused,
                            inventory_screen.open,
                            chest_screen.is_open() || station_screen.is_open(),
                            death.is_open(),
                            chat.is_typing(),
                        ) || journal.is_open()
                        {
                            touch.release_all();
                            // The bar's finger goes with the rest. A
                            // hold that matured while the player was in
                            // their inventory would feed them out of a
                            // slot they are no longer looking at.
                            bar_gestures.release_all();
                        }
                        let (dx, dy) = touch.take_look_delta();
                        input.accumulate_mouse(dx, dy);

                        let stick = touch.stick();
                        input.stick = (stick != (0.0, 0.0)).then_some(stick);

                        // **Only mining is polled here now.** Jump and
                        // sprint used to be, each with its own copy of
                        // "which key does this button stand for"; they
                        // are ordinary key events since a button became
                        // a key, and go down the same path a keyboard
                        // does. What is left is the one thing that
                        // cannot: `breaking` is a level rather than an
                        // edge, and the event path gates it on the
                        // cursor being grabbed -- which never happens
                        // on a phone, because there is no cursor to
                        // grab.
                        //
                        // The conditions are the ones the left mouse
                        // button is held under: a thumb on the mine
                        // button while a screen is open is a thumb on
                        // a screen.
                        // A finger resting on the bar is eating. Polled
                        // for the reason every rest is: a still finger
                        // sends no events, so the moment it stops being
                        // a tap arrives when the platform has nothing
                        // to say.
                        if let ui::hotbar::Touched::Eat(slot) =
                            bar_gestures.resting(Instant::now())
                        {
                            eat_from(
                                Some(slot),
                                &inventory,
                                &mut meal,
                                &mut hand,
                            );
                        }
                        // The hand in the look area, plus any button a
                        // player still has bound to mining. The hand is
                        // the ordinary way now -- see
                        // `touch::Touch::is_mining` -- and the button
                        // path stays because `Emits::Mine` is still a
                        // thing an arrangement may carry.
                        let mining = touch.is_mining(Instant::now())
                            || touch.held_slots().any(|slot| {
                                matches!(touch_layout.buttons[slot].emits, settings::Emits::Mine)
                            });
                        input.breaking = mining
                            && !paused
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !chat.is_typing();
                    }

                    // --- the sail takes the mouse, or the head does ---
                    //
                    // **Held down at the sail, the look moves the yard.** The
                    // player's own words: "зажав ЛКМ движение от мышки
                    // перейдёт к парусу". One button and one gesture, because
                    // a raft has no spare keys to learn and a phone has none
                    // at all -- on glass this is the same finger held in the
                    // look area that mines, and the same accumulated look it
                    // feeds (`Touch::take_look_delta`), so trimming a sail
                    // needs no touch control of its own.
                    //
                    // Read from where the feet were at the end of the last
                    // frame, which is a frame old and has to be: this is
                    // ahead of the collider, and the alternative is turning
                    // the head for one frame every time a player steps up to
                    // the mast.
                    //
                    // Rejected: a key. A key that braces the yard needs two
                    // (round and back), or three with a "square it" -- and a
                    // sail set by tapping is a sail nobody aims, because the
                    // thing being aimed is an angle against the wind and the
                    // hand already knows how to sweep an angle.
                    let trimming = (input.breaking
                        && world_ready
                        && !paused
                        && !death.is_open()
                        && !chat.is_typing())
                    .then(|| {
                        riding.sail_at_hand(
                            entities.steering().map(|steering| steering.id),
                            player.position,
                        )
                    })
                    .flatten();
                    if input.mouse_grabbed && !paused {
                        match trimming {
                            // Every frame the hand is down, even one the mouse
                            // did not move on: that is what keeps the held
                            // angle alive (`riding::TRIM_HOLD`) rather than
                            // letting it lapse under a still hand.
                            Some(raft) => {
                                riding.turn_sail(
                                    raft,
                                    input.mouse_dx
                                        * settings.mouse_sensitivity
                                        * logic::riding::TRIM_PER_LOOK,
                                    now,
                                );
                            }
                            None => camera.apply_mouse_delta(
                                input.mouse_dx,
                                input.mouse_dy,
                                settings.mouse_sensitivity,
                            ),
                        }
                    }

                    // The forced view, set after the mouse rather than
                    // instead of it, so the pin holds however the aim
                    // got there.
                    //
                    // The shake goes with it. Two frames meant to differ
                    // by a fifth of a degree and nothing else cannot
                    // also differ by an idle sway: the whole point of
                    // the sweep is that the *only* thing that moved is
                    // the number that was asked to move.
                    if let Some(look) = &forced_look {
                        let (yaw, pitch) = look.angles(shots_taken);
                        camera.yaw = yaw;
                        camera.pitch = pitch;
                        camera.shake = Vec3::ZERO;
                        camera.shake_angles = Vec3::ZERO;
                    }
                    menu.tick(dt);

                    remote_players.tick(dt);
                    // ...and then, if the environment asked for them, a
                    // rank of posed ones in front of the camera. After
                    // `tick` rather than before it: that is what drops
                    // anybody who has gone quiet, and these arrive in no
                    // snapshot at all. See `pose_for_a_photograph`.
                    remote_players.pose_for_a_photograph(player.position.as_vec3(), camera.yaw);
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
                        world_ready_since = Some(Instant::now());
                        println!("world ready -- spawning");

                        // **`PRIMITIVE_CHAT_SEND=<line>` says one line
                        // to the server the moment the world is there.**
                        //
                        // The only way to photograph anywhere but a
                        // spawn point without a person at the keyboard.
                        // A render fault reported underground -- pale
                        // specks on a mine ceiling -- cannot be
                        // reproduced from a beach, and driving the game
                        // to the mine means a mouse and a keyboard,
                        // which is exactly the "hand-run experiment
                        // nobody can repeat" this project refuses. With
                        // `/tp` on the far end this is a seat in a
                        // screenshot, in one command line.
                        //
                        // Sent here rather than at connect: the server
                        // has to know who is asking, and until the
                        // spawn area is loaded there is nowhere to
                        // teleport *to*.
                        //
                        // **Latched, because `world_ready` is not a
                        // one-way door.** `respawn_gate` puts it back to
                        // false on a respawn and on every position
                        // correction, which is right for what it is for
                        // -- gravity must not run over terrain that has
                        // not arrived -- and it means this arm runs again
                        // the moment the new ground is there. The line
                        // being `/tp` hid it: the second teleport went to
                        // the same place. `/give flint 32` does not hide
                        // it, and neither does a walk that starts over,
                        // so a measurement driven from the environment
                        // quietly stopped being the measurement asked
                        // for.
                        if !chat_line_sent {
                            chat_line_sent = true;
                            if let Ok(line) = std::env::var("PRIMITIVE_CHAT_SEND") {
                                println!("[chat] sending {line:?}");
                                net.send(ClientMessage::Chat(line));
                            }
                        }
                    }

                    // Measured from the world arriving rather than from
                    // the process starting: what makes a screenshot
                    // worth having is that the terrain is in it, and how
                    // long that takes depends on the machine.
                    if let (Some(shot), Some(since)) = (&pending_shot, world_ready_since) {
                        if since.elapsed().as_secs_f32() >= shot.after {
                            // One shot per frame through the sweep. The
                            // camera above is already aimed at
                            // `shots_taken`, so the file and the angle
                            // cannot come apart.
                            graphics.request_shot(engine::capture::frame_path(
                                &shot.path,
                                shots_taken,
                                look_frames,
                            ));
                            shots_taken += 1;
                            if shots_taken >= look_frames {
                                pending_shot = None;
                            }
                        }
                    }

                    // Whether the player is *actually* running, decided
                    // once and used twice: stamina is billed for it and
                    // the view bobs to it.
                    //
                    // **One value rather than two lists of conditions.**
                    // The bob used to build its own -- the Sprint key,
                    // not paused, not dead, grounded, not swimming --
                    // and the two lists disagreed in both directions. A
                    // phone has no Sprint key at all (the stick sprints
                    // when it is pushed to the rim), so the view never
                    // bobbed on Android; and a player holding Shift with
                    // no stamina left bobbed while walking, which is the
                    // one thing `walking_without_sprinting_does_not_bob`
                    // exists to forbid. Read from the value that is
                    // charged for and the bob cannot mean anything other
                    // than "you are paying to run".
                    let mut really_running = false;
                    if world_ready {
                        // Physics still runs while paused -- gravity does
                        // not stop for a menu on an authoritative server
                        // -- but the player stops steering.
                        // A dead player steers no more than a paused one
                        // does. Gravity still applies to both -- the
                        // server is authoritative about where bodies
                        // are, and a corpse hovering where it died would
                        // rubber-band the moment it respawned.
                        // ...and a sleeping player steers least of
                        // all. The server has stopped reading their
                        // transforms entirely (see
                        // `ServerMessage::Asleep`), so a client that
                        // kept walking would be walking a body nobody
                        // else can see move -- and would then be
                        // snapped back the moment they woke.
                        let frozen = paused
                            || death.is_open()
                            || sleep.is_asleep()
                            || inventory_screen.open
                            || chest_screen.is_open()
                            || station_screen.is_open()
                            || journal.is_open()
                            || chat.is_typing();
                        let wish_dir = if frozen {
                            Vec3::ZERO
                        } else {
                            wish_direction(&input, &camera, &settings.keybinds)
                        };
                        // --- a raft: the oars, and the deck underfoot ---
                        //
                        // **At the oars, the keys that walk row instead**, and so
                        // does the stick: forward and back is the stroke, left
                        // and right the turn, measured against where the camera
                        // looks the way walking is. A breathless rower pulls at
                        // `raft::TIRED_STROKE` rather than not at all. See
                        // `riding::oars_from_axes`.
                        let rowing = entities.steering().map(|steering| steering.id);
                        let oars = match rowing {
                            Some(_) if !frozen => logic::riding::oars_from_axes(
                                wish_dir.dot(camera.forward_horizontal()) * input.stick_speed(),
                                wish_dir.dot(camera.right_horizontal()) * input.stick_speed(),
                                !stamina.can_sprint(),
                            ),
                            _ => primitive_shared::raft::Oars::REST,
                        };
                        let wish_dir = if rowing.is_some() { Vec3::ZERO } else { wish_dir };
                        // The yard as this client's own hand is holding it,
                        // before the prediction that pushes against it: the
                        // sail's angle is what the wind is measured against
                        // (`raft::Body::sail_normal`), so the drag has to be
                        // in the raft *this frame steps*, not in the one the
                        // next snapshot brings.
                        entities.set_trim(riding.held_trim(now));
                        // ...and the river under the rowed raft, which the
                        // server's step reads off its own generator: see
                        // `WorldGen::river_current`.
                        entities.set_river_current(entities.steering().map_or((0.0, 0.0), |steering| {
                            worldgen.river_current(steering.body.x as f32, steering.body.y, steering.body.z as f32)
                        }));
                        entities.predict(
                            oars,
                            primitive_shared::raft::wind(sky.world_days(), weather),
                            sky.world_days(),
                            &chunks,
                            dt,
                        );
                        if let Some(message) = rowing.and_then(|raft| riding.row_message(raft, oars, now)) {
                            net.send(message);
                            debug_stats.network_messages_out_this_second += 1;
                        }
                        // ...and the angle itself, which the server is the
                        // authority on: it decides whether this player is
                        // standing where a hand reaches the sheets.
                        if let Some(message) = riding.trim_message(now) {
                            net.send(message);
                            debug_stats.network_messages_out_this_second += 1;
                        }
                        // Carried before the collider runs -- see
                        // `Riding::carry_player` for the shudder the other order
                        // makes -- and the decks handed to it as they are now.
                        let decks = entities.rafts();
                        riding.carry_player(&decks, &mut player.position, &mut camera.yaw);
                        player.decks = decks.iter().map(|pose| pose.now).collect();
                        // --- a horse: the reins, and the body on its saddle ---
                        //
                        // **On a horse the keys that walk ride**, the oars'
                        // rule: forward and back, left and right off the same
                        // wish the walk is made of, measured against the
                        // camera so a rider looking over their shoulder still
                        // rides where the keys say (`Horseback::reins_from_keys`).
                        // The body is put on the predicted saddle and not
                        // stepped at all -- see the physics loop below.
                        let on_horse = entities.horseback.is_some();
                        if let Some(mut horseback) = entities.horseback.take() {
                            let (forward, turn) = if frozen {
                                (0.0, 0.0)
                            } else {
                                (
                                    wish_dir.dot(camera.forward_horizontal()) * input.stick_speed(),
                                    wish_dir.dot(camera.right_horizontal()) * input.stick_speed(),
                                )
                            };
                            let reins = logic::horseback::Horseback::reins_from_keys(
                                forward,
                                turn,
                                !frozen && input.action_down(&settings.keybinds, keybinds::Action::Sprint),
                                !frozen && input.action_down(&settings.keybinds, keybinds::Action::Rein),
                                !frozen && input.action_pressed(&settings.keybinds, keybinds::Action::Jump),
                            );
                            horseback.predict(reins, &|x, y, z| chunks.block_at(x, y, z), dt);
                            if let Some(message) = horseback.rein_message(now) {
                                net.send(message);
                                debug_stats.network_messages_out_this_second += 1;
                            }
                            // **Getting down**: the rein key with the horse
                            // standing and nothing asked of it. At a trot the
                            // same key is a walk, which is how a rider comes
                            // to a stop and then off.
                            if !frozen
                                && forward.abs() < 0.05
                                && horseback.may_get_down()
                                && input.action_pressed(&settings.keybinds, keybinds::Action::Rein)
                            {
                                net.send(ClientMessage::Dismount);
                                debug_stats.network_messages_out_this_second += 1;
                            }
                            player.position = horseback.rider_feet();
                            player.velocity = Vec3::ZERO;
                            player.grounded = horseback.body.on_ground;
                            entities.set_ridden(Some((horseback.horse, horseback.feet(), horseback.body.yaw)));
                            entities.horseback = Some(horseback);
                        }
                        // **Getting up.** The screen said "asleep -- press
                        // any key to get up" for as long as sleep existed,
                        // and nothing behind it listened: the only way out of
                        // a bed was a right click on that bed. A step or a
                        // jump asks now, lying or sitting, and the dark says
                        // so in those words (`ui::sleep`). A sitter is
                        // already on their feet as far as the client is
                        // concerned -- sitting is not a lock, and the step
                        // they asked for is the one that gets them off the
                        // stool -- while a sleeper waits for the server to
                        // say where they stand (`ServerMessage::Posture`).
                        //
                        // **Not a key that was already down when the body
                        // came to rest**, which is `Rising`'s whole reason: a
                        // player who walked up to a bed holding forward was
                        // stood back up by that same key on the first frame
                        // they lay in it.
                        let controls_free = !paused
                            && !death.is_open()
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !chat.is_typing();
                        // A rower's movement keys are the oars, so only the
                        // jump gets a rower up off the stern -- and a rider's
                        // are the reins and the jump is the horse's, so
                        // nothing here gets a rider down (the rein key does,
                        // above).
                        let asked = controls_free
                            && !on_horse
                            && ((rowing.is_none()
                                && wish_direction(&input, &camera, &settings.keybinds) != Vec3::ZERO)
                                || input.action_pressed(&settings.keybinds, keybinds::Action::Jump));
                        if rising.ask(resting, asked) {
                            net.send(ClientMessage::StandUp);
                            debug_stats.network_messages_out_this_second += 1;
                            if resting.is_sitting() {
                                resting = logic::posture::Resting::Standing;
                            }
                        }
                        // Weight slows you down and stamina decides
                        // whether the sprint is available at all. Both
                        // are folded in here so physics only ever sees
                        // one speed and one flag.
                        //
                        // **Armour costs twice, and the two costs are
                        // different things.** Its *weight* goes through
                        // the same load rules a heavy pack does, which is
                        // why it is added to the carried total rather
                        // than handled apart; its *bulk* is that plate
                        // is stiff, which has nothing to do with how much
                        // it weighs and is a second multiplier. A player
                        // in full iron is slow because they are carrying
                        // thirty kilos and slower still because they
                        // cannot bend -- see `equipment::Worn::mobility`.
                        let carried = inventory.total_weight() + equipment.weight();
                        player.speed_scale = primitive_shared::load::speed_scale(carried)
                            * equipment.worn().mobility()
                            // ...and how tired they are, which is the
                            // fourth thing that decides a pace. Read
                            // off the same number the server keeps (it
                            // arrives with the other gauges), so the
                            // two sides agree without a second rule:
                            // the client is *applying* the server's
                            // fatigue, not inventing one.
                            * tiredness_speed(body.fatigue)
                            // ...and a broken leg, set or not: a splint
                            // is what lets it knit, not a leg to walk on.
                            // See `Injuries::speed_factor`.
                            * body.injuries.speed_factor()
                            // How hard a thumb is pushing, and 1.0 on
                            // anything with a keyboard. It belongs in
                            // the same multiplier that weight and
                            // armour use, so physics still sees one
                            // speed however the player asked for it.
                            // Only ever *reduces* the scale, which is
                            // why it needs nothing from the server: no
                            // anti-cheat check has ever complained
                            // about someone moving too slowly.
                            * input.stick_speed();
                        // ...and how much of the water's lift is left,
                        // off the same weight. **Not multiplied by the
                        // armour or the thumb**: those are about how
                        // fast a body moves, and this is about whether
                        // it floats -- a player in iron floats exactly
                        // as well as the same weight of stone would.
                        // See `load::buoyancy`, which the server bills
                        // the breath by.
                        player.buoyancy = primitive_shared::load::buoyancy(carried);
                        // ...and whether it was the *client* that just took
                        // the movement keys away, in which case the body
                        // treads water rather than settling to the depth its
                        // load asks for.
                        //
                        // Read off `frozen` -- the one value that already
                        // means "the player is not steering because a screen
                        // is up" -- rather than from a fresh list of screens
                        // beside it. A second list is a second list to keep
                        // in step, and the one screen left off it would be
                        // the one a drowning player had open.
                        //
                        // See `Player::treading`: a player with a heavy pack
                        // in deep water could only lighten it by opening the
                        // pack, and opening the pack was what sank them.
                        player.treading = frozen;
                        let wants_sprint = !frozen
                            && (input.action_down(&settings.keybinds, keybinds::Action::Sprint)
                                // A thumb pushed to the rim, for a
                                // screen with no Shift on it.
                                || input.stick_sprinting());
                        // ...and legs that will take a run. A broken one
                        // will not, and the jump stays -- see
                        // `Injuries::may_sprint` for why the one goes and
                        // not the other.
                        let sprinting =
                            wants_sprint && stamina.can_sprint() && body.injuries.may_sprint();
                        // **Up on jump, down on sprint** -- the two keys
                        // a hand is already on, and neither of them does
                        // anything else while flying: there is no ground
                        // to jump off and no stamina to spend running.
                        //
                        // Read off the *keys* rather than off
                        // `sprinting` above, which is gated on having
                        // stamina left. An exhausted player who could
                        // not descend would be stuck in the air with no
                        // way to understand why.
                        player.climb = if player.flying && !frozen {
                            let up = input
                                .action_down(&settings.keybinds, keybinds::Action::Jump);
                            let down = input
                                .action_down(&settings.keybinds, keybinds::Action::Sprint);
                            (up as i32 - down as i32) as f32
                        } else {
                            0.0
                        };
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
                        //
                        // **Up a tree a jump is a climb**: dearer, slower
                        // between pulls, and heavier under a load (see
                        // `Stamina::climb_cost`). And nobody jumps at all
                        // under more than they can carry (`load::can_jump`).
                        let climbing = player.footing_is_a_tree(&chunks);
                        let may_jump = primitive_shared::load::can_jump(carried)
                            && if climbing { stamina.can_climb(carried) } else { stamina.can_jump() };
                        // The river the player is in, once a frame: a
                        // current changes over metres, not over the slices
                        // of one frame. Asked only in water, because it is
                        // a few columns of the generator. See
                        // `physics::Player::current`.
                        player.current = if player.in_water {
                            worldgen.river_current(player.position.x as f32, player.position.y as f32, player.position.z as f32)
                        } else {
                            (0.0, 0.0)
                        };
                        let mut left = dt;
                        let mut first = true;
                        // **A sleeper's body is pinned, not simulated.** The
                        // server lays it across the bed with an upright
                        // collider, and under a roof two blocks up that
                        // collider is inside the ceiling; the push-out that
                        // follows walked sleepers out of their beds and
                        // through the wall beside them. The server moves
                        // nobody who is asleep, so nothing is lost by not
                        // stepping.
                        // ...and a rider's is carried by the horse: it was put
                        // on the saddle above, and a collider stepped under it
                        // would drop it through the horse's back.
                        while left > 0.0 && !on_horse && !matches!(resting, logic::posture::Resting::Lying { .. }) {
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
                                if climbing {
                                    stamina.spend_climb(carried);
                                } else {
                                    stamina.spend_jump();
                                }
                            }
                            left -= step;
                            first = false;
                        }

                        // Billed for the sprint the player actually got,
                        // not the one they asked for: physics refuses it
                        // in water and standing still, and charging for
                        // a sprint that did not happen is the sort of
                        // thing players notice and cannot explain.
                        // Which deck the feet ended on, and everyone else put on
                        // the decks as they are drawn this frame (see
                        // `RemotePlayers::ride`).
                        riding.settle(&decks, player.position);
                        remote_players.ride(&decks, dt);
                        // ...and everyone astride a horse put on its saddle as
                        // it is drawn this frame (`RemotePlayers::mount`), the
                        // decks' reason again.
                        remote_players.mount(&entities.ridden_horses(Instant::now()));
                        really_running = (sprinting
                            && player.grounded
                            && !player.swimming
                            && player.horizontal_speed() > 0.5)
                            // **Rowing costs breath as running does**, which is
                            // what makes the sail's free pace worth its four
                            // leathers. Not a bob, though: `footed` below is read
                            // off the body's own speed, which a rower has none of.
                            || oars.pulling();
                        stamina.update(
                            dt,
                            primitive_shared::load::load_fraction(carried),
                            really_running,
                        );
                    }
                    // **Particles, once a frame.**
                    //
                    // After physics and before the camera is used to
                    // draw them, so what is on screen is where they are
                    // this instant rather than where they were last one.
                    // The weather emits into the same pool: rain is
                    // particles now, spawned above the player at a rate
                    // and dying on whatever they land on -- see
                    // `engine::particles`.
                    {
                        // Snow or rain is the season's call as much as
                        // the climate's: see `season::falls_as_snow`.
                        // ...and the latitude's: a tropical winter is no
                        // winter (`season::seasonal_swing`), and rain in
                        // the tropics must not turn to snow when the server
                        // says the air is warm.
                        let snowing = primitive_shared::season::falls_as_snow_with_swing(
                            worldgen
                                .climate_at(
                                    player.position.x.floor() as i32,
                                    player.position.y.floor() as i32,
                                    player.position.z.floor() as i32,
                                )
                                .0,
                            sky.world_days(),
                            primitive_shared::season::seasonal_swing(
                                worldgen.latitude_degrees(player.position.z.floor() as i32),
                            ),
                        );
                        // **The rain falls in the world's own wind**, the
                        // one that moves the rafts
                        // (`raft::wind`) -- not a drift of its own.
                        //
                        // It *was* a drift of its own: a slow circle on the
                        // frame clock, chosen so the rain was not a set of
                        // vertical lines. Two weathers in one sky is what
                        // that was. A player watching their sail braced hard
                        // over to a wind out of the north, in rain falling
                        // straight down the other way, is being told by the
                        // game that one of the two is a decoration -- and the
                        // sail is the one that costs four leathers, so the
                        // rain is the one that has to give.
                        //
                        // Nothing is sent for this: both sides work the wind
                        // out from the clock and the weather, which is the
                        // whole reason `raft::wind` is a function and not a
                        // message.
                        let wind = primitive_shared::raft::wind(sky.world_days(), weather);
                        let (wx, wz) = wind.vector();
                        let drift = glam::Vec3::new(wx * RAIN_WIND_SPEED, 0.0, wz * RAIN_WIND_SPEED);
                        particles.weather(
                            // ...and it comes down harder in a squall and
                            // eases off in a lull, so a calm reads as a calm
                            // through the window as well as on the water.
                            // Never to nothing, because a rain that stopped
                            // would be the sky saying "clear" while the fires
                            // were still going out.
                            //
                            // **And only as much of it as the clouds have
                            // brought** (`Sky::rain_arrived`): the deck
                            // closes first and the rain follows it.
                            weather.intensity() * (0.65 + 0.5 * wind.strength) * sky.rain_arrived(),
                            snowing,
                            player.position.as_vec3(),
                            drift,
                            dt,
                        );
                        // ...and sparks over anything burning nearby.
                        particles.fires(&chunks, player.position.as_vec3(), dt);
                        // ...and white water on any rapid in view: the
                        // river's current is the generator's, and it is
                        // what tells a player from the bank where not to
                        // swim. See `WorldGen::river_current`.
                        particles.rapids(&chunks, player.position.as_vec3(), |x, y, z| worldgen.river_current(x, y, z), dt);
                        // ...and the float of a line in the water, until the
                        // line would be out of it or a fish comes up on it.
                        // The fish in the pack is the bite: a count higher
                        // than when the line went in. See `logic::fishing`.
                        if let Some(float) = fishing_float.as_mut() {
                            let landed = float.age == 0.0;
                            float.age += dt;
                            float.since += dt;
                            let block_at = |x, y, z| chunks.block_at(x, y, z);
                            let eye = (camera.position.x, camera.position.y, camera.position.z);
                            let at = float.position(block_at);
                            let at = glam::Vec3::new(at.0, at.1, at.2);
                            // **The float's own plop, where it came down**, on
                            // the first frame it is drawn -- which is the frame
                            // it appears on the water, so the eye and the ear
                            // agree. Up in pitch and down in level against the
                            // hand in a pool the same recording once was: this
                            // is a cork, a dozen blocks off. See `Sfx::FloatPlop`.
                            if landed {
                                audio.play_at(audio::Sfx::FloatPlop, at.as_dvec3(), 0.8, 1.35);
                            }
                            // The splash of the fish coming out is drawn
                            // where the float goes (`ServerMessage::Line`),
                            // which is the frame the pack has grown in.
                            if float.holds(primitive_shared::geometry::narrow(eye), input.hotbar_slot, inventory.block_in(input.hotbar_slot), block_at) {
                                particles.float(at, dt);
                                // The ring a fish makes taking the bait, and
                                // the water's own blip under it -- quieter
                                // than a splash, because the whole of the
                                // strike is noticing something small.
                                // Placed at the float, not in the player's
                                // head: it was the hand-in-water sound at full
                                // level, which said "you" about something a
                                // dozen blocks off. See `Sfx::FloatBite`.
                                if float.phase == logic::fishing::Phase::Dipping && float.since < dt * 1.5 {
                                    particles.float_ring(at);
                                    audio.play_at(audio::Sfx::FloatBite, at.as_dvec3(), 1.0, 1.0);
                                }
                                // ...and the water breaking over a fish that
                                // is on and running.
                                if float.phase == logic::fishing::Phase::Fighting && float.strain > 0.6 {
                                    particles.float_ring(at);
                                }
                            } else {
                                fishing_float = None;
                            }
                        }
                        // **The rod itself**: winding up, throwing, striking
                        // and reeling, all off one button. See
                        // `logic::fishing::Hold::step` for why this is state
                        // and not events.
                        {
                            let holding_rod = inventory
                                .block_in(input.hotbar_slot)
                                .map(primitive_shared::types::block_kind)
                                == Some(primitive_shared::types::BLOCK_FISHING_ROD);
                            let fighting = fishing_float
                                .is_some_and(|float| float.phase == logic::fishing::Phase::Fighting);
                            let giving_way = input.action_down(&settings.keybinds, keybinds::Action::Sprint);
                            // A screen up means the button's state has
                            // stopped arriving: the wind-up is dropped
                            // rather than let go of. See `Hold::cancel`.
                            if !input.mouse_grabbed {
                                rod_hold.cancel();
                            }
                            for order in rod_hold.step(
                                dt,
                                input.using && input.mouse_grabbed,
                                holding_rod,
                                fishing_float.is_some(),
                                fighting,
                                giving_way,
                            ) {
                                use logic::fishing::Order;
                                let message = match order {
                                    Order::Cast(power) => {
                                        // Judged here first, in the player's
                                        // own language, against this side's
                                        // own chunks: a throw onto the bank
                                        // is never sent.
                                        let eye = (
                                            camera.position.x as f32,
                                            camera.position.y as f32,
                                            camera.position.z as f32,
                                        );
                                        let look = camera.forward();
                                        match logic::fishing::cast_along(
                                            eye,
                                            (look.x, look.y, look.z),
                                            power,
                                            |x, y, z| chunks.block_at(x, y, z),
                                        ) {
                                            Ok(at) => {
                                                fishing_float = Some(logic::fishing::Float {
                                                    at,
                                                    slot: input.hotbar_slot,
                                                    fish_before: inventory
                                                        .count(primitive_shared::types::BLOCK_RAW_FISH),
                                                    age: 0.0,
                                                    phase: logic::fishing::Phase::Settling,
                                                    strain: 0.0,
                                                    liveliness: 0.0,
                                                    since: 0.0,
                                                });
                                                audio.play(audio::Sfx::Swing);
                                                // ...and the rod whips forward
                                                // from wherever it was drawn
                                                // back to. See `hand::Rod`.
                                                hand.cast_rod();
                                                Some(ClientMessage::CastLine { power })
                                            }
                                            Err(said) => {
                                                notice = Some((
                                                    settings.language.text(said.msg()).to_string(),
                                                    Instant::now(),
                                                ));
                                                None
                                            }
                                        }
                                    }
                                    Order::Strike => {
                                        // **Said here, not by the server.**
                                        // This side knows whether the float
                                        // was under; the server refuses the
                                        // strike either way
                                        // (`Fishing::strike`), and what it
                                        // sends back is the float going.
                                        if fishing_float
                                            .is_some_and(|float| float.phase != logic::fishing::Phase::Dipping)
                                        {
                                            notice = Some((
                                                settings
                                                    .language
                                                    .text(ui::lang::Msg::FishingStruckAtNothing)
                                                    .to_string(),
                                                Instant::now(),
                                            ));
                                        }
                                        Some(ClientMessage::Strike)
                                    }
                                    Order::Reel(pulling) => Some(ClientMessage::Reel { pulling }),
                                    Order::In => {
                                        fishing_float = None;
                                        Some(ClientMessage::ReelIn)
                                    }
                                };
                                if let Some(message) = message {
                                    net.send(message);
                                    debug_stats.network_messages_out_this_second += 1;
                                }
                            }
                        }
                        // ...and blood wherever the server says an
                        // animal was struck since the last frame. Here
                        // rather than in `drain_network` because a
                        // snapshot is applied there and this is a
                        // *drawing* of it: several snapshots can arrive
                        // in one frame, and one burst per blow is a
                        // property of the frame rather than of the
                        // socket. See `Entities::take_blows`.
                        for at in entities.take_blows() {
                            particles.blood(at);
                        }
                        // ...and the player's own, on a timer, when
                        // somebody is photographing it. See
                        // `bleeding_for_a_photograph`.
                        if bleeding_for_a_photograph() {
                            bleed_in -= dt;
                            if bleed_in <= 0.0 {
                                bleed_in = BLEED_EVERY;
                                particles.blood(
                                    player.eye_position().as_vec3() - glam::Vec3::Y * 0.35,
                                );
                            }
                        }
                        particles.update(&chunks, dt);
                        // ...and the butterflies, fireflies and frogs. The
                        // wolves the frogs fall quiet for are the ones the
                        // last snapshot brought -- behind a hill or in the
                        // dark, but not unknown to the client.
                        let dangers: Vec<glam::Vec3> = entities
                            .heard()
                            .into_iter()
                            .filter(|animal| animal.species.is_hostile())
                            .map(|animal| animal.at.as_vec3())
                            .collect();
                        let country = |gx: i32, gy: i32, gz: i32| {
                            (
                                worldgen.biome_at(gx, gz),
                                worldgen.climate_at(gx, gy, gz).0,
                                primitive_shared::season::seasonal_swing(worldgen.latitude_degrees(gz)),
                            )
                        };
                        critters.update(
                            &engine::critters::Surroundings {
                                chunks: &chunks,
                                player: player.position.as_vec3(),
                                world_time: sky.world_days(),
                                wet: weather.is_wet(),
                                dangers: &dangers,
                                country: &country,
                                light: &light,
                                lantern: inventory
                                    .block_in(input.hotbar_slot)
                                    .is_some_and(primitive_shared::types::is_lit_torch),
                            },
                            dt,
                        );
                        breeze.update(
                            &engine::breeze::Air {
                                chunks: &chunks,
                                light: &light,
                                player: player.position.as_vec3(),
                                wind: primitive_shared::raft::wind(sky.world_days(), weather),
                                world_time: sky.world_days(),
                                wet: weather.is_wet(),
                            },
                            dt,
                        );
                    }
                    // The eye of a body that is sitting or lying is not at
                    // standing height: see `logic::posture`. Only the view
                    // moves -- the aim still comes from `eye_position`,
                    // the point the server measures reach from.
                    camera.position = resting.eye(player.position);
                    match resting.look_on_lying_down() {
                        Some((yaw, pitch)) if !looked_along_bed => {
                            camera.yaw = yaw;
                            camera.pitch = pitch;
                            looked_along_bed = true;
                        }
                        Some(_) => {}
                        None => looked_along_bed = false,
                    }
                    match resting.face_on_sitting_down() {
                        Some(yaw) if !faced_the_chair => {
                            camera.yaw = yaw;
                            faced_the_chair = true;
                        }
                        Some(_) => {}
                        None => faced_the_chair = false,
                    }

                    // --- camera motion that is not the player moving ---
                    //
                    // The bob follows the *actual* speed and only counts
                    // while the player is on their feet and getting
                    // somewhere, so walking into a wall, jumping and
                    // swimming do not sway the view.
                    //
                    // Two flags rather than one: a walk bobs at
                    // `WALK_SHARE` of a sprint's, which is what makes
                    // the gait readable without watching the stamina
                    // bar. `really_running` already carries the
                    // grounded and not-swimming half of the test, so
                    // this is the same question asked one notch lower.
                    let footed = player.grounded
                        && !player.swimming
                        && player.horizontal_speed() > 0.5;
                    shake.update(dt, player.horizontal_speed(), footed, really_running);
                    // The step lag rides with the bob rather than with
                    // the position, and that is the point: `camera.shake`
                    // moves the *view* only, so walking up onto a drift
                    // of snow rises smoothly while the interaction ray
                    // -- and therefore what the server is told was
                    // clicked -- stays exactly where the player is.
                    camera.shake = shake.offset(camera.right_horizontal(), Vec3::Y)
                        - Vec3::Y * player.view_step_lag();
                    camera.shake_angles = shake.angles();
                    // The view is settled here, which is the only place
                    // it can be compared with the last one. See
                    // `DebugStats::record_view`: a frame that aims
                    // exactly where the frame before it aimed is a frame
                    // the player cannot tell apart from the frame before
                    // it, however quickly it was drawn.
                    debug_stats.record_aim(
                        camera.yaw + camera.shake_angles.y,
                        camera.pitch + camera.shake_angles.x,
                    );

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

                    // On a raft, the feet are sent as a place on its deck; see
                    // `ClientMessage::Deck` for why a world position is the
                    // wrong thing to tell the server about somebody on a
                    // moving floor.
                    match riding.deck_message(
                        player.position,
                        camera.yaw,
                        camera.pitch,
                        player.grounded,
                        now,
                        player_update_interval,
                        &mut sequence,
                    ) {
                        logic::riding::DeckTransform::Send(message) => {
                            net.send(message);
                            debug_stats.network_messages_out_this_second += 1;
                        }
                        logic::riding::DeckTransform::Quiet => {}
                        logic::riding::DeckTransform::NotAboard => maybe_send_transform(
                            net,
                            &player,
                            &camera,
                            now,
                            player_update_interval,
                            &mut last_player_update_sent,
                            &mut last_sent_transform,
                            &mut sequence,
                            &mut debug_stats,
                        ),
                    }

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
                    // ...and a hand on the sheets is not a hand on a pick.
                    // The same button does both, so the one the player is
                    // actually using has to take it: without this, bracing
                    // the yard from the stern would also be swinging at the
                    // planks under the rower, and five of those break the
                    // raft (`raft::HITS_TO_BREAK`).
                    let can_mine = world_ready
                        && !paused
                        && !death.is_open()
                        && input.mouse_grabbed
                        && trimming.is_none();

                    // Someone under the crosshair takes the swing before
                    // the world behind them does. Nearer than whatever
                    // block is there, or a punch through a wall would
                    // land -- the server checks the distance, but it has
                    // no idea what is between the two of them.
                    // ...and an animal takes it before the world does,
                    // for the same reason and one rung down: a person in
                    // front of a deer takes the blow, and a deer in
                    // front of a wall stops the wall coming apart.
                    //
                    // Looked for when a blow could start *or land*: a
                    // thrust lands when its point is out
                    // (`hand::impact_seconds`), which can be after the
                    // button was let go, so the button is not the only
                    // reason to cast the ray any more.
                    let held_now = inventory.block_in(input.hotbar_slot);
                    let under_crosshair = if can_mine && (input.breaking || strikes.due(now)) {
                        player_under_crosshair(&remote_players, &chunks, &camera, held_now)
                            .map(Aimed::Player)
                            .or_else(|| {
                                animal_under_crosshair(&entities, &chunks, &camera, held_now)
                                    .map(Aimed::Animal)
                            })
                    } else {
                        None
                    };
                    // Someone is being swung at, which keeps mining out of
                    // the way: the wall behind them must not come apart in
                    // the gaps between blows.
                    let struck = input.breaking && under_crosshair.is_some();
                    // ...and whether a blow starts or lands this frame.
                    // The message leaves on the landing, aimed at whoever
                    // is under the crosshair *then*: a deer that stepped
                    // off the line while the spear was drawn is a miss,
                    // and a miss sends nothing.
                    let beat = strikes.frame(now, struck, held_now);
                    if beat.lands {
                        if let Some(target) = under_crosshair {
                            send_blow(target, net, &mut debug_stats);
                            soundscape.on_strike(&audio, true);
                        }
                    }

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
                    // ...and how well that tool was made, which is a small
                    // divisor on the time (`quality::speed_scale`) and the
                    // stamina both. Off the stack and not the id: a fine
                    // pick and a poor one are the same block.
                    let tool_quality = inventory
                        .slots()
                        .get(input.hotbar_slot)
                        .copied()
                        .flatten()
                        .map_or(primitive_shared::quality::Quality::PLAIN, |s| s.quality());
                    //
                    // **And the ray this one casts is blind to water**, which
                    // the right click's is not: see `aimed_block_to_mine` for
                    // the player report that was, and for why one ray cannot
                    // answer both gestures.
                    let aim = if can_mine && !struck {
                        aimed_block_to_mine(&chunks, &camera).filter(|(_, block)| {
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
                    // ...and a broken arm digs at half, on the same terms:
                    // the swing is slower, the block is not softer. See
                    // `injury::BROKEN_ARM_STRENGTH`.
                    let dug = mining.update(
                        aim,
                        can_mine && input.breaking,
                        dt * stamina.dig_rate() * body.injuries.strength_factor(),
                        held_tool,
                        tool_quality,
                    );
                    // The outline and the cracks go where the target is in
                    // the world -- a leaning palm where it leans. See
                    // `Mining::fit_outline`.
                    mining.fit_outline(|cell, block| {
                        primitive_shared::geometry::block_box_for_aim_near(block, cell.0, cell.1, cell.2, false, |dx, dy, dz| {
                            chunks
                                .block_at(cell.0 + dx, cell.1 + dy, cell.2 + dz)
                                .unwrap_or(primitive_shared::types::BLOCK_AIR)
                        })
                    });
                    if let Some(cell) = dug {
                        // Billed for the work the swing actually was: a
                        // pick makes a block cheaper in seconds, and the
                        // tank is measured in seconds, so a better tool
                        // is less tiring as well as faster.
                        // ...and for the work the *swing* was: a block that
                        // comes away in quarters is four swings of a
                        // quarter of the bill, which adds up to the bill it
                        // always was. See `dig::swing_seconds`.
                        if let Some(seconds) = aim.and_then(|(_, block)| {
                            primitive_shared::dig::swing_seconds_made(block, held_tool, tool_quality)
                        }) {
                            stamina.spend_dig(seconds);
                        }
                        // **A slice or a break, and the block decides
                        // which.** Everything that happens when a block
                        // finally goes -- the drop, the tool's wear, the
                        // collapse -- is the break path, so the last slice
                        // is the break message it always was and only the
                        // ones before it are `Dig`. See `dig::next_bite`.
                        let face = aimed_face_to_mine(&chunks, &camera);
                        let sliced = match (aim, face) {
                            (Some((_, block)), Some(face)) => {
                                primitive_shared::dig::Side::from_normal((
                                    i32::from(face.0),
                                    i32::from(face.1),
                                    i32::from(face.2),
                                ))
                                .and_then(|side| primitive_shared::dig::next_bite(block, side))
                                .is_some()
                            }
                            _ => false,
                        };
                        if sliced {
                            request_dig(&chunks, cell, face.unwrap_or((0, 1, 0)), net, &mut debug_stats);
                        } else {
                            request_break(&chunks, cell, net, &mut debug_stats);
                        }
                    }
                    // The arm follows from all of that rather than
                    // deciding any of it: a blow lands on a player, or a
                    // block is coming apart, and the hand is what that
                    // looks like. Advanced every frame even though the
                    // geometry is only rebuilt at `DYNAMIC_REBUILD_HZ` --
                    // the state is a few floats, and letting it skip
                    // frames would make the swing's length depend on the
                    // frame rate.
                    //
                    // **One blow on screen per blow struck.** The arm used
                    // to be started on every frame someone was under the
                    // crosshair, so a fist fight swung twice for each punch
                    // the cooldown let through and a spear jabbed three
                    // times for each thrust. It starts when `Strikes` says
                    // a blow does, and lasts as long as the blow is refused
                    // for (`hand::blow_seconds`).
                    if beat.starts {
                        hand.strike(held_now);
                    }
                    // Not "the button is down": swinging at thin air or at
                    // bedrock is a click, not a rhythm. `aim` is what the
                    // client believes is actually coming apart under the
                    // crosshair.
                    let digging_now = can_mine && input.breaking && aim.is_some();
                    // A block set down, a mouthful or a drink the server let
                    // through, acted out by the player's own hand. See
                    // `Hand::gesture`.
                    if let Some(action) = remote_players.take_own_gesture() {
                        // A mouthful this hand already lifted for (`Meal`)
                        // is not lifted a second time when the server says
                        // it went down.
                        let echoed = matches!(action, protocol_action::Eat | protocol_action::Drink)
                            && meal_sent.is_some_and(|at| at.elapsed().as_secs_f32() < 1.5);
                        if !echoed {
                            hand.gesture(action);
                        }
                    }
                    // ...and the cut under way, sent when the knife is through.
                    if let Some(cutting) = cut {
                        let aimed_now = aimed_block(&chunks, &camera);
                        match cutting.due(aimed_now, input.hotbar_slot, Instant::now()) {
                            CutStep::Cutting => {
                                // The knife keeps working: a stroke whenever
                                // the last one has finished.
                                hand.strike(inventory.block_in(input.hotbar_slot));
                            }
                            CutStep::Abandoned => cut = None,
                            CutStep::Through => {
                                cut = None;
                                net.send(ClientMessage::UseBlock {
                                    global_x: cutting.cell.0,
                                    global_y: cutting.cell.1,
                                    global_z: cutting.cell.2,
                                });
                                debug_stats.network_messages_out_this_second += 1;
                            }
                        }
                    }
                    // ...and the mouthful under way, sent when it is chewed.
                    if let Some(taking) = meal {
                        match taking.due(&inventory, Instant::now()) {
                            MealStep::Chewing => {}
                            MealStep::Abandoned => meal = None,
                            MealStep::Swallow => {
                                meal = None;
                                // The crunch or the swallow, when it goes down.
                                audio.play(if taking.drinking { audio::Sfx::Drink } else { audio::Sfx::Eat });
                                net.send(ClientMessage::Eat { slot: taking.slot as u8 });
                                debug_stats.network_messages_out_this_second += 1;
                                meal_sent = Some(Instant::now());
                            }
                        }
                    }
                    hand.update(
                        dt,
                        digging_now,
                        player.velocity.with_y(0.0).length(),
                        player.grounded,
                    );
                    // ...and the same swing, for everybody else's picture
                    // of this player. See `DigSignal`.
                    // The rod drawn back follows the wind-up. See `hand::Rod`.
                    hand.wind_rod(rod_hold.charge(), dt);
                    // **A rod being wound up is said the way a swing is.**
                    // `Gesture::digging` is "working what is in the hand" to
                    // everybody watching, and with a rod in it that work is
                    // drawing the rod back (`RemotePlayer::arm`). A second
                    // flag would be a byte a player a tick and a message of
                    // its own for a thing that is never true at the same
                    // time as the first.
                    let working = digging_now || rod_hold.charge().is_some();
                    if let Some(digging) = dig_signal.frame(Instant::now(), working) {
                        net.send(ClientMessage::Digging { digging });
                        debug_stats.network_messages_out_this_second += 1;
                    }
                    // ...and then, if somebody is photographing the
                    // blow, held at the phase they asked for. After
                    // `update` rather than instead of it, so the bob
                    // and everything else still run. See
                    // `photographed_swing`.
                    if let Some(phase) = photographed_swing() {
                        hand.freeze(phase);
                    }

                    // What the world sounds like this frame: footsteps,
                    // the rhythm of a swing that is still going,
                    // weather, anything burning nearby, and which of the
                    // six moods the composer should be in.
                    //
                    // Here rather than earlier because it reads the
                    // results of everything above it -- where physics
                    // put the player, and what the mining code decided
                    // was under the crosshair.
                    audio.set_volumes(settings.master_volume, settings.music_volume);
                    soundscape.update(
                        &audio,
                        &audio::soundscape::Frame {
                            dt,
                            player: &player,
                            camera: &camera,
                            chunks: &chunks,
                            sky: &sky,
                            weather,
                            health_fraction: if max_health > 0.0 {
                                health / max_health
                            } else {
                                1.0
                            },
                            // Not "is there a connection": a player
                            // falling through a world whose floor has
                            // not arrived yet is not walking on
                            // anything.
                            in_world: world_ready,
                            digging: (can_mine && input.breaking)
                                .then_some(aim)
                                .flatten()
                                .map(|(cell, _)| cell),
                            // Swinging at nothing. The same button, and
                            // deliberately a different sound: a swing
                            // that misses should be audible as a miss.
                            swinging: can_mine
                                && input.breaking
                                && aim.is_none()
                                && !struck,
                        },
                    );
                    // ...and what lives near the player: gulls, a bird going
                    // up, the frogs. A call of its own for the reason
                    // `Soundscape::wildlife` gives.
                    if world_ready {
                        soundscape.wildlife(&audio, dt, &chunks, &entities.heard(), &critters.croaking(), &critters.buzzing(), &critters.flapping());
                    }
                    // ...and whatever let go this frame. Drained here
                    // rather than where the snapshot is applied for the
                    // reason the blood is: several snapshots can land in
                    // one frame, and one collapse is one sound.
                    for (at, block) in entities.take_gave_way() {
                        soundscape.on_gave_way(&audio, at, block);
                    }

                    input.end_frame();

                    // Decided by the head, not the feet: standing
                    // waist-deep in a lake shouldn't tint the screen.
                    // Physics already samples this, so there's one
                    // definition of "under water" rather than two.
                    let underwater = player.submerged;

                    // Asked of the eye and not of the feet, because
                    // this is a fact about what the *view* runs into: a
                    // player standing in a pit with their head above
                    // the rim is looking across a meadow, and one whose
                    // head is under the rim is looking at earth.
                    let underground_fraction =
                        underground.advance(&chunks, camera.position.as_vec3(), dt);

                    let mut params = frame_params(
                        &settings,
                        &sky,
                        render_distance,
                        chunks.loaded_radius_blocks(),
                        fog_enabled,
                        render_origin,
                        Eye {
                            underwater,
                            underground: underground_fraction,
                            smoke: {
                                smoke_shown += (smoke - smoke_shown) * (dt * 1.5).min(1.0);
                                smoke_shown
                            },
                            health_fraction: if max_health > 0.0 {
                                health / max_health
                            } else {
                                1.0
                            },
                            held: shown_in_hand(inventory.block_in(input.hotbar_slot)),
                        },
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
                    // Is there a fire within working range?
                    //
                    // Asked of the client's own copy of the world, and
                    // it is only ever *advice*: the server asks the same
                    // question against its own copy of where the player
                    // is standing and refuses a craft that fails it.
                    // What this buys is that the crafting column greys
                    // out the fireside recipes as you walk away from the
                    // fire rather than a round trip later.
                    //
                    // **Not every frame.** It is a scan of three hundred
                    // and forty-three cells through the chunk map -- a
                    // hash lookup per cell -- and the only thing that
                    // reads the answer is a menu that is shut. At two
                    // hundred frames a second that is seventy thousand
                    // lookups a second spent on nothing. Now: only while
                    // the inventory is open, and once as it opens so the
                    // first frame of it is already right.
                    let heat_due = inventory_screen.open || heat_was_open;
                    heat_was_open = inventory_screen.open;
                    if heat_due {
                        let heat = fire_within_reach(&chunks, player.position.as_vec3());
                        last_heat = heat;
                        inventory_screen.set_heat(heat);
                    }
                    let heat = last_heat;

                    // **The memory readout is counted at the title's pace,
                    // not the frame's.** With the F3 readout on -- which is
                    // every benchmark -- `FrameInfo` is built every frame,
                    // and these two sums walk every section of every loaded
                    // chunk twice: at a render distance of twenty-four,
                    // fifty-seven thousand sections a frame for a number
                    // printed once a second. That was frame time the
                    // benchmark charged to the game and a player with F3
                    // open paid for.
                    if title_due {
                        counted_bytes = (chunks.heap_bytes(), light.heap_bytes());
                    }
                    let info = (title_due || debug_stats.console_enabled).then(|| FrameInfo {
                        position: player.position.as_vec3(),
                        chunk: ChunkManager::chunk_for_world_pos(
                            player.position.x,
                            player.position.z,
                        ),
                        grounded: player.grounded,
                        loaded_chunks: chunks.loaded_count(),
                        pending_chunks: chunks.pending_count(),
                        chunk_bytes: counted_bytes.0,
                        light_bytes: counted_bytes.1,
                        arena_bytes: graphics.arena_usage(),
                        surface: (graphics.size.width, graphics.size.height),
                        anisotropy: settings.anisotropy,
                        // What the pass is drawn at, not the setting:
                        // an adapter without the asked-for count runs
                        // at a lower one, and the number a frame time
                        // has to be read against is the one in force.
                        msaa: graphics.sample_count(),
                        sky_scale: settings.sky_scale,
                        // The mode in force, not the setting that asked
                        // for it: on Android those are routinely
                        // different, and the difference is the whole
                        // question when the frame rate reads high and
                        // the motion does not.
                        present_mode: match graphics.present_mode() {
                            wgpu::PresentMode::Fifo => "fifo",
                            wgpu::PresentMode::FifoRelaxed => "fifo-relaxed",
                            wgpu::PresentMode::Mailbox => "mailbox",
                            wgpu::PresentMode::Immediate => "immediate",
                            wgpu::PresentMode::AutoVsync => "auto-vsync",
                            wgpu::PresentMode::AutoNoVsync => "auto-novsync",
                        },
                        render_distance: chunks.render_distance(),
                        // Everything between "this chunk needs a mesh"
                        // and "the card has it": waiting to be
                        // dispatched, out with a worker, and -- since
                        // landing one is rationed -- finished and
                        // waiting for frame time. A number that only
                        // ever grows means the budget is set below what
                        // this machine can keep up with, and that is
                        // worth being able to see.
                        queued_meshes: dirty.len()
                            + urgent.len()
                            + mesher.in_flight()
                            + mesher.pending_count(),
                        queued_arrivals: arrivals.len(),
                        lighting_jobs: mesher.lighting_in_flight(),
                        remote_players: remote_players.len(),
                        entities: entities.len(),
                        clock: sky.clock_string(),
                        season: primitive_shared::season::Season::at(sky.world_days()).name(),
                        sun_intensity: sky.sun_intensity(),
                        seed: world_seed,
                        nourishment,
                        weather: weather.name(),
                        heat,
                        biome: worldgen
                            .biome_at(
                                player.position.x.floor() as i32,
                                player.position.z.floor() as i32,
                            )
                            .name(),
                        latitude: worldgen.latitude_degrees(player.position.z.floor() as i32),
                        particles: particles.len(),
                        audio: audio.status(),
                        selected_block: inventory
                            .block_in(input.hotbar_slot)
                            .map(block_name)
                            .unwrap_or("nothing"),
                        draw_calls: graphics.draw_calls_last_frame,
                        solid_indices: graphics.solid_indices_last_frame,
                        solid_indices_in_view: graphics.solid_indices_in_view_last_frame,
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

                    // **Where the frame is drawn from.**
                    //
                    // Everything that reaches the GPU is measured from
                    // this point, so that no coordinate on the card is
                    // ever a seven-digit number -- see
                    // `engine::mesh::Vertex::instance_layout` for what
                    // that was costing. It follows the player in whole
                    // blocks and only when they have gone far enough,
                    // because the geometry built against it has to stay
                    // valid between rebuilds.
                    let wanted_origin = render_origin_for(camera.position, render_origin);
                    let origin_moved = wanted_origin != render_origin;
                    render_origin = wanted_origin;
                    // **The frame is drawn from the origin its meshes
                    // are about to be built against.** `params` was
                    // filled in above, before this moved, and every
                    // dynamic mesh below is rebuilt from the new origin
                    // on the frame it moves -- so for that one frame the
                    // animals, the items, the other players and every
                    // particle were drawn through the old origin's
                    // matrix, sixty-four blocks or more from where they
                    // are.
                    params.render_origin = render_origin;

                    // The clock the *moving* geometry below rebuilds on,
                    // and the pace the interface falls back to while
                    // something on it is animating. A frame the origin
                    // moved in is always a rebuild: the geometry that
                    // exists was measured from somewhere else, and
                    // drawing it unchanged would put every entity a
                    // hundred blocks from where it is.
                    // ...and a frame a chest lid changed hands between the
                    // chunk mesh and this geometry, which has to draw its
                    // half on the frame the mesh stops drawing its own. See
                    // `ChunkManager::note_meshed_lids`.
                    let rebuild_due = dynamic_rebuild_due(
                        origin_moved,
                        riding.is_aboard(),
                        last_rebuild,
                        now,
                    ) | chunks.take_lid_handover();
                    // How long the moving geometry has stood as it is. What a
                    // chest's lid swings by on the frame it is rebuilt: it is
                    // drawn on this clock, so it moves on this clock, and the
                    // swing takes the same third of a second whether the
                    // clock ticks thirty times a second or the origin moved.
                    let since_rebuild = last_rebuild.map_or(0.0, |last| now.duration_since(last).as_secs_f32());
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
                    let debug_panel_shown =
                        info.is_some() && debug_stats.console_enabled && debug_panel_allowed;
                    let key = UiKey {
                        in_game: true,
                        aspect: graphics.aspect().to_bits(),
                        loading: loading.is_some(),
                        hotbar_slot: input.hotbar_slot,
                        inventory: inventory_fingerprint(&inventory),
                        health: health.to_bits(),
                        max_health: max_health.to_bits(),
                        recent_health: recent_health.to_bits(),
                        // The horse's wind while riding, as the strip draws it.
                        stamina: entities.horseback.as_ref().map_or(stamina.fraction(), |h| h.wind_fraction()).to_bits(),
                        exhausted: entities.horseback.as_ref().map_or(stamina.is_exhausted(), |h| h.body.wind <= 0.0),
                        breath: breath.to_bits(),
                        nourishment: nourishment.to_bits(),
                        heat,
                        notice: notice_drawn
                            .map(|(text, fade)| (text_fingerprint(text), fade >= 1.0)),
                        chat: chat.ui_key(now),
                        inventory_screen: inventory_screen.ui_key(),
                        wounds: wounds_fingerprint(&body.injuries),
                        chest_screen: chest_screen.ui_key(),
                        station_screen: station_screen.ui_key(),
                        death: death.ui_key(),
                        sleep: sleep.ui_key(),
                        journal: journal.ui_key(
                            player_mark(player.position.as_vec3(), camera.yaw),
                            graphics.aspect(),
                        ),
                        debug_panel: debug_panel_shown,
                        hud_hidden,
                        menu: paused.then(|| {
                            menu.ui_key(&menu_context(&settings, &worlds, &graphics, None))
                        }),
                        language: settings.language,
                    };
                    // The parts that change with no event behind them,
                    // for which time is the only trigger there is.
                    let ui_animating = chat.is_fading(now)
                        || matches!(notice_drawn, Some((_, fade)) if fade < 1.0)
                        || death.is_animating()
                        || debug_panel_shown
                        // A meter that is killing the player flashes.
                        || hud::alarming(nourishment, breath, body);
                    // **A running bar every frame, not at the animation
                    // rate.** The marker is what a blow is timed against,
                    // and one drawn a thirtieth of a second stale is a
                    // blow aimed at where it was.
                    let ui_rebuilt = ui_key.as_ref() != Some(&key)
                        || (ui_animating && rebuild_due)
                        || station_screen.is_running();
                    if ui_rebuilt {
                    ui_key = Some(key);
                    ui_vertices.clear();
                    // How much bigger than it was drawn, and where each
                    // piece grows from. See `widgets::scale_about`: the
                    // origin is the decision, not the factor.
                    //
                    // The factor is now **per screen**, not one number
                    // for the whole interface: the hotbar is pinned to
                    // the bottom edge with a screen of room above it and
                    // takes the size asked for, while a centred screen
                    // takes whatever its own extent leaves. One cap for
                    // all of them is what made INTERFACE SIZE do
                    // nothing -- see `widgets::Layout`.
                    let ui_aspect = graphics.aspect();
                    let ui_scale = graphics.ui_scale();
                    let layout = widgets::Layout::for_screen(ui_aspect, ui_scale);
                    if loading.is_none() {
                        // The dark a sleeper's screen goes, first, so the
                        // hotbar, the gauges and the thumb controls are all
                        // drawn over it -- see `ui::sleep` for why a phone
                        // needs them there.
                        ui::sleep::build_into(
                            graphics.textures.font,
                            &sleep,
                            settings.language,
                            ui_aspect,
                            ui_scale,
                            &mut ui_vertices,
                        );
                        let hud_from = ui_vertices.len();
                        let bar_from = ui_vertices.len();
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
                            // **On a horse the strip is the horse's wind**:
                            // the rider is not spending their own, and the
                            // number that decides whether the next stretch
                            // can be a gallop is the horse's.
                            entities.horseback.as_ref().map_or(stamina.fraction(), |h| h.wind_fraction()),
                            entities.horseback.as_ref().map_or(stamina.is_exhausted(), |h| h.body.wind <= 0.0),
                            breath,
                            nourishment,
                            body,
                            &inventory,
                            notice_drawn,
                            &mut hud_attention,
                            now,
                            &mut ui_vertices,
                        );
                        // The bar and its gauges are one thing pinned to
                        // the bottom of the screen, so they grow as one
                        // and upward -- the HUD is laid out against
                        // `hotbar::BOTTOM` and would come apart from it
                        // otherwise.
                        widgets::scale_about(
                            &mut ui_vertices[bar_from..],
                            widgets::anchor::BOTTOM(ui_aspect),
                            ui_scale,
                        );

                        // **The line**: the rod being wound back, or the
                        // strain on a fish. Pinned to the crosshair with the
                        // sail's dial rather than stacked with the gauges of
                        // the body, and for the sail's reason: it is being
                        // read exactly when nothing is wrong, so it must not
                        // fade with them. See `hud::line_gauge`.
                        if rod_hold.charge().is_some()
                            || fishing_float.is_some_and(|float| float.phase == logic::fishing::Phase::Fighting)
                        {
                            let from = ui_vertices.len();
                            let mut painter = widgets::Painter::onto(
                                graphics.textures.font,
                                std::mem::take(&mut ui_vertices),
                            );
                            hud::line_gauge(
                                &mut painter,
                                rod_hold.charge(),
                                fishing_float
                                    .filter(|float| float.phase == logic::fishing::Phase::Fighting)
                                    .map(|float| float.strain),
                            );
                            ui_vertices = painter.into_vertices();
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::CENTRE(ui_aspect),
                                ui_scale,
                            );
                        }

                        // Aboard a raft with its sail up, the dial
                        // that says what the trim is doing against the wind.
                        //
                        // Not part of the stack above: it is pinned to the
                        // top of the screen, it is not a gauge of the body,
                        // and it must not fade out when the player is well --
                        // a sail is being read exactly when nothing is wrong.
                        if let Some((raft, body)) = riding.aboard_raft().filter(|(_, body)| body.sail) {
                            let from = ui_vertices.len();
                            let mut painter = widgets::Painter::onto(
                                graphics.textures.font,
                                std::mem::take(&mut ui_vertices),
                            );
                            let wind = primitive_shared::raft::wind(sky.world_days(), weather);
                            hud::sail_gauge(
                                &mut painter,
                                wind.toward - body.yaw,
                                body.sail_angle,
                                wind.strength,
                                trimming == Some(raft),
                            );
                            ui_vertices = painter.into_vertices();
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::TOP(ui_aspect),
                                ui_scale,
                            );
                        }

                        // **Which way is north**: a needle while a water
                        // compass is in the hand, and a line off the sky
                        // while the player is looking at it. Pinned to the
                        // top with the sail's dial and never faded, for the
                        // sail's reason. See `logic::bearing` for why the
                        // sky's reading is a look and not an instrument.
                        {
                            let from = ui_vertices.len();
                            let mut painter = widgets::Painter::onto(
                                graphics.textures.font,
                                std::mem::take(&mut ui_vertices),
                            );
                            let held_compass = inventory
                                .block_in(input.hotbar_slot)
                                .is_some_and(|held| primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_WATER_COMPASS);
                            if held_compass {
                                let sailing = riding.aboard_raft().is_some_and(|(_, body)| body.sail);
                                hud::compass_dial(
                                    &mut painter,
                                    logic::bearing::needle(camera.yaw),
                                    if sailing { hud::COMPASS_BESIDE_SAIL } else { 0.0 },
                                );
                            }
                            let eye = camera.position.floor();
                            let reading = logic::bearing::read_sky(&logic::bearing::SkyView {
                                look: camera.forward(),
                                to_sun: -sky.sun_direction(),
                                to_moon: -sky.moon_direction(),
                                moon_lit: primitive_shared::moon::illumination(sky.world_days()),
                                overcast: sky.overcast(),
                                open_sky: light.sky(eye.x as i32, eye.y as i32, eye.z as i32)
                                    >= primitive_shared::types::MAX_LIGHT,
                            });
                            if let Some(reading) = reading {
                                use logic::bearing::{Guide, Side};
                                use ui::lang::Msg;
                                let by = settings.language.text(match reading.guide {
                                    Guide::Sun => Msg::SkyBySun,
                                    Guide::Moon => Msg::SkyByMoon,
                                    Guide::Stars => Msg::SkyByStars,
                                });
                                let north = settings.language.text(match reading.north {
                                    Side::Ahead => Msg::NorthAhead,
                                    Side::Right => Msg::NorthRight,
                                    Side::Behind => Msg::NorthBehind,
                                    Side::Left => Msg::NorthLeft,
                                });
                                hud::sky_hint(&mut painter, &format!("{by}: {north}"));
                            }
                            ui_vertices = painter.into_vertices();
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::TOP(ui_aspect),
                                ui_scale,
                            );
                        }

                        // The thumb controls, over the HUD and under
                        // everything that can be opened: a player with
                        // their pack open is not steering. Never drawn
                        // on a desktop -- see `is_touch_primary`. (The
                        // compass to a bag that stood here is gone; the way
                        // back is the map's -- see `ui::journal`.)
                        if touch_controls
                            && !paused
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open()
                            && !death.is_open()
                        {
                            let mut painter = widgets::Painter::onto(
                                graphics.textures.font,
                                std::mem::take(&mut ui_vertices),
                            );
                            hud::touch_controls(&mut painter, touch.layout(), |control| {
                                touch.is_held(control)
                            });
                            ui_vertices = painter.into_vertices();
                        }

                        // Hidden with Tab: everything since the sleeper's
                        // dark goes -- hotbar, gauges, notices, thumbs -- and
                        // chat, the screens and the menus drawn after this
                        // still come. A phone keeps its thumb controls,
                        // which are how it would ever press the key again.
                        if hud_hidden && !touch_controls {
                            ui_vertices.truncate(hud_from);
                        }

                        // Chat sits over the HUD and under the
                        // inventory: it is readable while playing, and
                        // it is not what a player opening their pack is
                        // looking at.
                        if chat.has_anything_to_draw(now) {
                            let from = ui_vertices.len();
                            chat.build_into(
                                graphics.textures.font,
                                graphics.aspect(),
                                // The box carries its own send and
                                // leave buttons where there is no
                                // keyboard to press Enter and Escape
                                // on. See `chat::input_row`.
                                touch_controls,
                                settings.language,
                                now,
                                &mut ui_vertices,
                            );
                            // Pinned to the bottom-left: the box is
                            // typed there and the log fills upward
                            // from it.
                            let grown = layout.fit_from_corner(chat::EXTENT);
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::BOTTOM_LEFT(ui_aspect),
                                grown,
                            );
                            // ...and then up, clear of the on-screen
                            // keyboard, which the corner it is pinned to
                            // knows nothing about. See
                            // `chat::keyboard_lift`: the hit-test below
                            // subtracts the same number, and it is the
                            // same call so the two cannot drift.
                            widgets::lift(
                                &mut ui_vertices[from..],
                                chat::keyboard_lift(touch_controls, chat.is_typing(), grown),
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
                            let from = ui_vertices.len();
                            inventory_screen.build_into(
                                graphics.textures.font,
                                &face_layers,
                                &inventory,
                                &equipment,
                                &body.injuries,
                                // The health page's readings, gathered
                                // here because this is the one place
                                // that has all four: health and
                                // nourishment come in their own
                                // messages, stamina is predicted on the
                                // client, and the rest ride `body`.
                                &ui::inventory_screen::Vitals {
                                    health: if max_health > 0.0 {
                                        (health / max_health).clamp(0.0, 1.0)
                                    } else {
                                        0.0
                                    },
                                    nourishment,
                                    stamina: stamina.fraction(),
                                    body,
                                },
                                settings.language,
                                &mut ui_vertices,
                            );
                            // A screen the player opens grows outward
                            // from the middle, which is also the point
                            // `cursor_to_ui` inverts -- so a
                            // click still lands on the slot under it.
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::CENTRE(ui_aspect),
                                inventory_screen::grow_by(layout),
                            );
                        }

                        // The chest sits where the inventory does,
                        // and the two are never open at once -- opening
                        // either closes the other.
                        if chest_screen.is_open() {
                            let from = ui_vertices.len();
                            chest_screen.build_into(
                                graphics.textures.font,
                                &face_layers,
                                &inventory,
                                settings.language,
                                &mut ui_vertices,
                            );
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::CENTRE(ui_aspect),
                                chest_screen.grow_by(layout),
                            );
                        }

                        if station_screen.is_open() {
                            let from = ui_vertices.len();
                            station_screen.build_into(
                                graphics.textures.font,
                                &face_layers,
                                settings.language,
                                &mut ui_vertices,
                            );
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::CENTRE(ui_aspect),
                                station_screen.grow_by(layout),
                            );
                        }

                        // The death screen goes over all of it. The
                        // pause menu is drawn after this whole block and
                        // therefore still sits on top, which is right:
                        // it is the only screen that can leave the
                        // world, and a dead player is exactly who wants
                        // to.
                        // The journal is laid out against the window, like
                        // the menu, rather than grown about the middle:
                        // it fills the glass and has nowhere to grow to.
                        if journal.is_open() && !death.is_open() {
                            journal.build_into(
                                graphics.textures.font,
                                &face_layers,
                                &inventory,
                                player_mark(player.position.as_vec3(), camera.yaw),
                                ui_aspect,
                                settings.language,
                                &mut ui_vertices,
                            );
                        }

                        if death.is_open() {
                            let from = ui_vertices.len();
                            death.build_into(graphics.textures.font, settings.language, &mut ui_vertices);
                            widgets::scale_about(
                                &mut ui_vertices[from..],
                                widgets::anchor::CENTRE(ui_aspect),
                                layout.fit(death::EXTENT),
                            );
                        }
                    }
                    if let Some(info) = info
                        .as_ref()
                        .filter(|_| debug_stats.console_enabled && debug_panel_allowed)
                    {
                        // **Deliberately not scaled.** It is a readout,
                        // not a screen: the player asking for a bigger
                        // interface is asking about the things they
                        // press, and thirty lines of diagnostics grown
                        // half again covered two thirds of a phone. It
                        // fits itself to the window instead -- see
                        // `debug::panel_into`.
                        ui::debug::panel_into(
                            &debug_stats.overlay_lines(info),
                            ui_aspect,
                            graphics.textures.font,
                            &mut ui_vertices,
                        );
                    }
                    if paused {
                        // Laid out rather than scaled -- see the menu
                        // built for the title screen above.
                        menu.build_into(
                            &menu_context(&settings, &worlds, &graphics, None),
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
                            render_origin,
                            &face_layers,
                            &light,
                            Some(&graphics.textures),
                            &mut entity_vertices,
                            &mut entity_indices,
                            &mut item_vertices,
                            &mut item_indices,
                        );
                    }
                    // ...and what other players are carrying, into the
                    // same two buffers. A pick in somebody's hand is
                    // the same sprite as a pick on the ground and wants
                    // the same pass; the actor pipeline the *body* is
                    // drawn with samples the player skin and cannot
                    // reach the block atlas at all. See
                    // `remote_players::build_held_items_into`.
                    // ...and every thing set down by hand, lying where it
                    // was put. Asked of the chunks rather than the entities:
                    // it is a cell in the world, not a stack the server moves.
                    entities::build_set_down_into(
                        chunks.set_down_items(),
                        render_origin,
                        &face_layers,
                        &light,
                        Some(&graphics.textures),
                        &mut entity_vertices,
                        &mut entity_indices,
                        &mut item_vertices,
                        &mut item_indices,
                    );
                    remote_players::build_held_items_into(
                        &remote_players,
                        render_origin,
                        &face_layers,
                        &light,
                        Some(&graphics.textures),
                        &mut entity_vertices,
                        &mut entity_indices,
                        &mut item_vertices,
                        &mut item_indices,
                    );
                    // ...and the lid of every chest somebody has open, which
                    // is here for the set-down items' reason and one more:
                    // it *moves*. A lid swings for a third of a second
                    // (`mesh::LID_SWING_SECONDS`), and a chunk meshed again
                    // on every frame of that is forty rebuilds of sixteen
                    // thousand cells to turn four boxes. It is drawn on this
                    // clock, so it is stepped on this clock: the angle drawn
                    // is the angle the swing has reached.
                    for cell in chunks.advance_lids(since_rebuild) {
                        // Rested shut: the mesh takes the lid back. Until
                        // that mesh lands the frame goes on drawing it,
                        // shut (`note_meshed_lids`); urgent all the same,
                        // because it is a chest somebody is standing at.
                        let pos = ChunkPos::from_world(cell.0, cell.2);
                        bump_version(&mut chunk_versions, pos);
                        mark_urgent(&mut urgent, &mut dirty_set, pos);
                    }
                    for (cell, block, swing) in chunks.open_lids() {
                        let corner = glam::DVec3::new(f64::from(cell.0), f64::from(cell.1), f64::from(cell.2));
                        let (sky, block_light) = entities::sampled_light(corner, &light);
                        let at = (corner - render_origin.as_dvec3()).as_vec3();
                        engine::mesh::chest_lid_block(
                            [at.x, at.y, at.z],
                            block,
                            swing,
                            &face_layers,
                            sky | (block_light << 4),
                            &mut entity_vertices,
                            &mut entity_indices,
                        );
                    }
                    graphics.write_dynamic_mesh(
                        &mut entity_mesh,
                        &entity_vertices,
                        &entity_indices,
                    );

                    // ...and every particle in the world, into a buffer
                    // and a pass of its own.
                    //
                    // Not the entity buffer any more: a particle wears
                    // one *texel* of a block rather than the whole
                    // picture of it, and the terrain vertex has two bits
                    // of texture coordinate. See `engine::particles`.
                    //
                    // Measured from the render origin like everything
                    // else in the world. It used to be uploaded in world
                    // coordinates, "because the shader uses `view_proj`
                    // directly" -- and `view_proj` is built around the
                    // origin, so every particle was drawn the player's
                    // own position further on: blood seventy blocks up
                    // in the sky. See `Particles::build_into`.
                    particle_vertices.clear();
                    particle_indices.clear();
                    let (billboard_right, billboard_up) =
                        engine::particles::billboard_axes(&camera);
                    particles.build_into(
                        render_origin,
                        billboard_right,
                        billboard_up,
                        &face_layers,
                        &light,
                        &mut particle_vertices,
                        &mut particle_indices,
                    );
                    // Those with a water surface between them and the eye
                    // go first and are drawn before the water. See
                    // `Particles::behind_water_first`.
                    particles_behind_water =
                        particles.behind_water_first(&chunks, camera.position.as_vec3(), &mut particle_indices);
                    // The small life rides the same buffer and the same
                    // pass: a wing is a soft quad like a flake of snow.
                    critters.build_into(
                        render_origin,
                        camera.right_horizontal(),
                        glam::Vec3::Y,
                        &face_layers,
                        &light,
                        &mut particle_vertices,
                        &mut particle_indices,
                    );
                    // ...and so does the wind, laid out along the particles'
                    // own axes: a streak seen from above must not be a hairline.
                    breeze.build_into(
                        render_origin,
                        billboard_right,
                        billboard_up,
                        &face_layers,
                        &light,
                        &mut particle_vertices,
                        &mut particle_indices,
                    );
                    graphics.write_dynamic_mesh(
                        &mut particle_mesh,
                        &particle_vertices,
                        &particle_indices,
                    );
                    graphics.write_dynamic_mesh(&mut item_mesh, &item_vertices, &item_indices);

                    // The player's own hand, and what is in it. Hidden
                    // behind anything that has taken over the screen:
                    // a menu, a container, the death screen, or the
                    // loading bar with no world behind it yet. An arm
                    // waving over the inventory is worse than no arm.
                    hand_vertices.clear();
                    hand_indices.clear();
                    let shown_in_hand = shown_in_hand(inventory.block_in(input.hotbar_slot));
                    hand.build_into(
                        world_ready
                            && loading.is_none()
                            && !paused
                            && !death.is_open()
                            && !inventory_screen.open
                            && !chest_screen.is_open()
                            && !station_screen.is_open()
                            && !journal.is_open(),
                        shown_in_hand,
                        &face_layers,
                        Some(&graphics.textures),
                        // Lit by the cell the player's own head is in,
                        // the same way a dropped item is lit by the cell
                        // it lies in.
                        entities::sampled_light(camera.position, &light),
                        // **The same clock the campfire runs on**, so a
                        // torch in the hand and a fire in the ring are
                        // in step. Read from the sky's own elapsed
                        // seconds, which is what the shader animates the
                        // hearth by -- a second clock here would drift
                        // against it, and two fires flickering out of
                        // step is exactly the sort of thing a player
                        // sees without being able to say what is wrong.
                        //
                        // The *pictures* are the torch's own, and that
                        // is not a change of mind about sharing: a
                        // hearth's fire is drawn to fill a cell and a
                        // torch's has to sit on four texels of fibre.
                        // See `Textures::torch_flame_layer`.
                        shown_in_hand
                            .filter(|&block| primitive_shared::types::is_lit_torch(block))
                            .map(|_| {
                                let frames = crate::engine::texture::FLAME_FRAMES;
                                let step = (sky.elapsed() * crate::engine::texture::FLAME_FPS)
                                    as u32
                                    % frames;
                                graphics.textures.torch_flame_layer() + step
                            }),
                        &mut hand_vertices,
                        &mut hand_indices,
                    );
                    graphics.write_dynamic_mesh(&mut hand_mesh, &hand_vertices, &hand_indices);
                    } // rebuild_due

                    // Other players on every frame they are drawn, off the
                    // clock above: see `figures_rebuild_due`.
                    if figures_rebuild_due(rebuild_due, !remote_players.is_empty()) {
                    actor_vertices.clear();
                    actor_indices.clear();
                    remote_players::build_actor_mesh_into(
                        &remote_players,
                        // Every dead player's body in the loaded world is
                        // drawn here too, in their own skin: see
                        // `player_model::append_lying`.
                        Some(&chunks),
                        render_origin,
                        &light,
                        &mut actor_vertices,
                        &mut actor_indices,
                    );
                    // The block outline and its cracks are untextured
                    // lit triangles, which is exactly what the actor
                    // pipeline already draws -- so they ride along in
                    // the same buffer rather than needing a pass of
                    // their own.
                    mining.build_overlay_into(render_origin, &mut actor_vertices, &mut actor_indices);
                    graphics.write_dynamic_mesh(&mut actor_mesh, &actor_vertices, &actor_indices);

                    break_vertices.clear();
                    break_indices.clear();
                    if let Some(stage) = mining.break_stage() {
                        // The lid as the frame is drawing it, so the cracks
                        // go on the lid where it is. See `mining::model_faces`.
                        let lid = mining.target().and_then(|cell| {
                            chunks.open_lids().find(|&(at, _, _)| at == cell).map(|(_, _, swing)| swing)
                        });
                        mining.build_break_mesh_into(
                            render_origin,
                            graphics.textures.break_layer(stage),
                            &face_layers,
                            lid,
                            &mut break_vertices,
                            &mut break_indices,
                        );
                    }
                    graphics.write_dynamic_mesh(
                        &mut break_mesh,
                        &break_vertices,
                        &break_indices,
                    );
                    } // figures_rebuild_due

                    // The fires' shadows walk the blocks round the player,
                    // read again only when the eye has walked out of the
                    // middle of the last lot or a chunk under it has a new
                    // mesh. See `engine::lamp_shadow`.
                    if let Some(min) = graphics.lamp_volume_wanted(camera.position) {
                        // The plants as the player asked for their shadows,
                        // so the walk and the map agree about a crown.
                        let volume = engine::lamp_shadow::Volume::gather_for(min, graphics.plant_shadows(), |gx, gz, y0, out| {
                            match chunks.column(gx, gz) {
                                Some(column) => {
                                    for (dy, slot) in out.iter_mut().enumerate() {
                                        *slot = column.block(y0 + dy as i32);
                                    }
                                }
                                None => out.fill(primitive_shared::types::BLOCK_AIR),
                            }
                        });
                        graphics.set_lamp_volume(volume);
                    }

                    // Everything above was this frame's own work; what
                    // follows is the renderer's. See
                    // `DebugStats::record_phases`.
                    let simulation = now.elapsed();
                    graphics.render(
                        frame,
                        &camera,
                        &params,
                        &Scene {
                            actor_mesh: Some(&actor_mesh),
                            entity_mesh: Some(&entity_mesh),
                            item_mesh: Some(&item_mesh),
                            break_mesh: Some(&break_mesh),
                            particle_mesh: Some(&particle_mesh),
                            particles_behind_water,
                            hand_mesh: Some(&hand_mesh),
                            loading,
                            hotbar: &ui_vertices,
                            ui_changed: ui_rebuilt,
                        },
                    );
                    debug_stats.record_phases(
                        simulation,
                        graphics.encode_time_last_frame,
                        graphics.acquire_time_last_frame,
                        graphics.present_time_last_frame,
                        graphics.gpu_time_last_frame(),
                        graphics.gpu_stage_ms_last_frame(),
                    );
                }

            // FIX: nothing was listening for this, and the game has
            // always claimed it was. `InputState::release_all` says in
            // its own doc comment that it is called when the player
            // "alt-tabs away" -- and it was called when the *cursor*
            // was released, which is not the same event and does not
            // happen when a window loses focus. So a player who
            // alt-tabbed mid-stride came back with the walk key still
            // held: key-up is delivered to whichever window has focus,
            // and that was no longer this one. The player returned to
            // find themselves running into a wall, and the only way out
            // was to press and release the key again.
            platform::Event::Focused(false) => {
                input.release_all();
                touch.release_all();
                pointer.release();
            }

            // **Ask again for whatever the interface already wanted.**
            //
            // The keyboard is requested on an edge -- when the answer to
            // "is something taking text" changes -- which is right,
            // because asking every frame is asking sixty times a second
            // for a thing that has not moved. What it misses is that the
            // edge can fall at a moment when the request cannot be
            // granted: Android refuses to raise a keyboard for a window
            // that does not have focus, and this game opens straight
            // onto whichever screen it was last on, text field and all.
            // So the one request went out on the first frame, before the
            // activity had focus, and was dropped without a word -- and
            // nothing asked again, because as far as the edge was
            // concerned the keyboard was already up.
            //
            // Forgetting what the platform is holding rather than
            // calling directly: the next `AboutToWait` re-reads what the
            // interface actually wants, which is one place deciding it
            // rather than two.
            platform::Event::Focused(true) => {
                ime.forget();
            }

            // The activity is going away and the surface with it.
            //
            // Two things to do. The fingers are forgotten for the same
            // reason focus loss forgets them, and more urgently: on a
            // phone there is no key to press again to clear a stuck
            // one. And the renderer is told it has nothing to draw
            // into -- Android destroys the native window here, and
            // every frame attempted against a dead surface is an error
            // logged sixty times a second for as long as the player is
            // reading their messages.
            platform::Event::Suspended => {
                // **The world goes to disk here, and this is the only
                // warning there will be.**
                //
                // `Suspended` is the last thing Android delivers before
                // the activity is destroyed, before a low-memory kill,
                // and before the watchdog kills a process that stopped
                // answering -- and not one of those three ever delivers
                // anything afterwards. A world saved only on the way out
                // of the pause menu is a world lost every time the
                // player takes a call, and that is what was happening:
                // the saves directory on a phone that had been played in
                // held a world with no edits in it at all.
                //
                // Synchronously, on this thread, and never spawned onto
                // the runtime: the process can stop existing the moment
                // `onStop` returns, and a save that was still being
                // waited on somewhere else is a save that did not
                // happen.
                //
                // The server is *not* taken or stopped -- checking a
                // message must not end the session. `/save` is the same
                // command the console runs and reaches
                // `save_everything`: profiles, world, chests, fires and
                // racks.
                if let Some(server) = local_server.as_ref() {
                    for line in server.console_command("/save") {
                        println!("{line}");
                    }
                }
                // ...and the map of it, for the same reason: this is the
                // last word Android gives, and a walk that was never
                // written down is a walk the map forgets.
                journal.save_map();
                input.release_all();
                touch.release_all();
                pointer.release();
                graphics.surface_lost();
            }

            // ...and a *new* window on the way back, not the old one.
            //
            // This is the half that is easy to miss, because on a
            // desktop it never happens: the surface is as long-lived as
            // the process. On Android an activity that has been away
            // comes back with a different native window, so the old
            // surface names something that no longer exists -- and a
            // game that only handled `Suspended` would resume to a
            // black screen it could never recover from.
            //
            // Only the surface is rebuilt. The device, the pipelines
            // and every chunk mesh already on the card are untouched,
            // so glancing at a notification costs a frame rather than a
            // reload of the world.
            platform::Event::Resumed => {
                if let Err(e) = graphics.recreate_surface(window.raw(), window.size()) {
                    crash::report_fatal("the display could not be reattached", &e);
                    quit!();
                }
                camera.aspect = graphics.aspect();
            }

            platform::Event::MouseMotion { dx, dy } => {
                if input.mouse_grabbed {
                    input.accumulate_mouse(dx, dy);
                }
            }

            platform::Event::AboutToWait => {
                // The scripted run of the text path, if one was asked
                // for. Before the keyboard sync below rather than after,
                // because each step is meant to be picked up by the
                // sync on the frame it happens. See `ime_probe`.
                if let Some(probe) = ime_probe.as_ref() {
                    let elapsed = ime_probe_started.elapsed();
                    match ime_probe_step {
                        // Nothing until there is a menu to open a form
                        // on. The game comes up on whichever screen it
                        // was last on, and that can be a world.
                        0 if elapsed >= Duration::from_secs(3) => {
                            if net.is_some() && !paused {
                                println!(
                                    "[ime-probe] waiting: the game is in a world, not the menu"
                                );
                            } else {
                                println!("[ime-probe] opening the new-world form");
                                menu.apply(Action::NewWorld);
                                ime_probe_step = 1;
                            }
                        }
                        // The name, all but its last character, so that
                        // the editor and the mirror are agreed on
                        // something before the interesting frame.
                        1 if elapsed >= Duration::from_secs(5) => {
                            let mut stem: String = probe.name.chars().collect();
                            if probe.seed.is_some() {
                                stem.pop();
                            }
                            println!("[ime-probe] committing {stem:?} as an input method would");
                            window.set_ime_text(&stem);
                            ime_probe_step = 2;
                        }
                        // **The frame the seed bug lived on**, and it
                        // has to be one frame. The input method's copy
                        // is polled rather than delivered, so a
                        // character committed between two frames arrives
                        // on the *same* frame as the tap that moved the
                        // focus -- and the mirror, which had no idea
                        // whose text it was holding, read it as
                        // something typed into the box just tapped.
                        // Committing and focusing separately would let
                        // the game off exactly the hook this is for.
                        2 if elapsed >= Duration::from_secs(7) => {
                            if probe.seed.is_some() {
                                println!(
                                    "[ime-probe] committing {:?} and tapping the seed box                                      on the same frame",
                                    probe.name,
                                );
                                window.set_ime_text(&probe.name);
                                // **The three things a press on that box
                                // does, in the order the mouse arm does
                                // them**, because the one thing this
                                // hook cannot do is produce a finger:
                                // the editor is read before the focus
                                // moves, the menu takes the action, and
                                // the frame loop answers it. Skipping
                                // the first would let the probe pass
                                // against a game that still loses the
                                // last letter of the name; skipping the
                                // third would let it pass against one
                                // that never asks for a keyboard.
                                reconcile_the_editor(
                                    &mut ime, &window, &mut menu, &mut chat, traced,
                                );
                                let tap = Action::Focus(menu::Field::Seed);
                                menu.apply(tap.clone());
                                handle_action! { tap }
                            }
                            ime_probe_step = 3;
                        }
                        3 if elapsed >= Duration::from_secs(9) => {
                            if let Some(seed) = probe.seed.as_deref() {
                                println!("[ime-probe] committing {seed:?} into the seed box");
                                window.set_ime_text(seed);
                            }
                            ime_probe_step = 4;
                        }
                        4 if elapsed >= Duration::from_secs(11) => {
                            // Both fields, always, because the failure
                            // this catches moves text from one to the
                            // other: a name that lost its last
                            // character is the same bug as a seed that
                            // gained one.
                            println!(
                                "[ime-probe] the name holds {:?} and the seed {:?}",
                                menu.name_input.text(),
                                menu.seed_input.text(),
                            );
                            handle_action! { Action::CreateWorld }
                            ime_probe_step = 5;
                        }
                        _ => {}
                    }
                }
                // Whether the platform's own editor holds the focused
                // field, told to the menu once a frame: it decides
                // whether a tap in a field may move the caret. See
                // `Menu::place_caret_under_cursor`.
                menu.set_ime_owns_text(menu.accepts_text() && window.ime_owns_text());
                // Two copies of one text field, reconciled once a
                // frame. All of the thinking is in `ui::ime`; what is
                // left here is asking the platform and doing what it
                // is told, which is the only part that needs a window.
                reconcile_the_editor(&mut ime, &window, &mut menu, &mut chat, traced);
                window.request_redraw()
            }

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
/// Puts the pointer wherever it now is, on whichever screen owns it.
///
/// One place rather than two, because two callers ask the same question
/// for different reasons: a mouse that moved, and a finger that landed.
/// `None` means the pointer has left -- a finger lifted, a cursor gone
/// out of the window -- and every screen reads that as "nothing is
/// hovered".
///
/// The order is the order the screens sit in: a pause menu is over a
/// death screen is over a chest is over the pack. Only one can have it,
/// and it is the topmost.
#[allow(clippy::too_many_arguments)]
/// Hands the pointer to whichever screen is in front of it, in that
/// screen's own coordinates.
///
/// **Where the aspect divide's inverse now lives.** Each centred screen
/// is grown by what fits *it* (see `widgets::Layout::fit`), so there is
/// no longer one number a click can be divided by on the way in. It is
/// divided here instead, per screen, against the same extent the frame
/// loop grew that screen's geometry by -- which is what keeps a button
/// clickable where it is drawn. `Layout::hit` is the one place that
/// division is written, and it is tested against `scale_about`.
///
/// The menu is the exception and takes the point untouched: it is laid
/// out for the window rather than drawn for a desktop and multiplied.
fn place_cursor(
    at: Option<(f32, f32)>,
    in_menu: bool,
    layout: widgets::Layout,
    menu: &mut Menu,
    death: &mut death::DeathScreen,
    chest_screen: &mut chest_screen::ChestScreen,
    station_screen: &mut station_screen::StationScreen,
    inventory_screen: &mut inventory_screen::InventoryScreen,
) {
    let into = |scale| at.map(|point| layout.hit(point, scale));
    if in_menu {
        menu.set_cursor(at);
    } else if death.is_open() {
        death.set_cursor(into(layout.fit(death::EXTENT)));
    } else if station_screen.is_open() {
        station_screen.set_cursor(into(station_screen.grow_by(layout)));
    } else if chest_screen.is_open() {
        chest_screen.set_cursor(into(chest_screen.grow_by(layout)));
    } else if inventory_screen.open {
        inventory_screen.set_cursor(into(inventory_screen::grow_by(layout)));
    }
}

/// One pass of the mirror, for the two places that need one.
///
/// The frame loop reconciles once at `AboutToWait`, which is the right
/// place for a poll. The other caller is the mouse press, which has to
/// read the editor *before* it can move the focus -- see the call site.
/// A function rather than a second copy, because two copies of "who
/// changed the text" is how the two would come to disagree.
fn reconcile_the_editor(
    ime: &mut ime::Mirror,
    window: &dyn platform::Window,
    menu: &mut Menu,
    chat: &mut chat::Chat,
    traced: bool,
) {
    let mut typing = ime::Typing { menu, chat };
    let editor = typing.focused().and_then(|_| window.ime_text());
    let request = ime.sync(&mut typing, editor.as_deref(), traced);
    for note in &request.notes {
        println!("{note}");
    }
    if let Some(up) = request.keyboard {
        window.set_ime_visible(up);
    }
    if let Some(text) = &request.editor {
        window.set_ime_text(text);
    }
}

/// Sends the line typed in the chat box -- or, when the box was asking for
/// a cairn's name, gives the cairn that name on this player's map and sends
/// nothing (see `Chat::open_naming`). One function for the Enter key and the
/// touch box's send button, so the two cannot come to disagree about where
/// a name goes.
fn submit_chat(
    chat: &mut chat::Chat,
    net: Option<&network::NetworkHandle>,
    journal: &mut ui::journal::Journal,
    debug_stats: &mut DebugStats,
) {
    let naming = chat.naming();
    match (chat.submit(), naming, net) {
        (Some(name), Some(at), _) => journal.explored.name_mark(at, &name),
        (Some(line), None, Some(net)) => {
            net.send(ClientMessage::Chat(line));
            debug_stats.network_messages_out_this_second += 1;
        }
        _ => {}
    }
}

fn close_chat(
    chat: &mut chat::Chat,
    window: &dyn platform::Window,
    input: &mut input::InputState,
    paused: bool,
) {
    chat.close();
    // The on-screen keyboard goes away with the box it was typing
    // into. A no-op on a desktop; on a phone, leaving it up would
    // cover half the world.
    window.set_ime_visible(false);
    if !paused {
        grab_cursor(window, input);
    }
}

/// Everything the shaders need for this frame, derived from the
/// server-synced sky plus local render settings.
#[allow(clippy::too_many_arguments)]
/// The handful of facts about where the eye *is* that a frame's
/// parameters depend on.
///
/// Together rather than as four more arguments, and the reason is
/// arithmetic: `frame_params` began at six arguments and grew to ten as
/// the water, the caves, the hurt flash and the torch each turned out
/// to change how a frame is lit. Ten positional arguments, four of them
/// `bool` and `f32`, is a call nobody can read and two can be swapped
/// in without the compiler noticing.
///
/// These four and not the others because they are the ones that answer
/// one question -- what is the player standing in, and what are they
/// carrying. The settings, the sky and the render distance are facts
/// about the *world*, and putting them in the same bag would be tidying
/// rather than grouping.
struct Eye {
    underwater: bool,
    /// How much of the sky is shut out where the eye is, 0..1, already
    /// settled over time by `fog::Underground`.
    underground: f32,
    /// How thick the smoke of a closed room is where the eye is, 0..1,
    /// already eased. See `Fog::fill_with_smoke`.
    smoke: f32,
    health_fraction: f32,
    /// What is in the player's hand, so a lit torch can light the world
    /// around them. See `FrameParams::carried_light`.
    held: Option<primitive_shared::types::BlockId>,
}

/// What the camera is to be shown in the hand, which is normally just
/// what the player is holding.
///
/// **`PRIMITIVE_HOLD=<block>` overrides it, and only for the drawing.**
/// A hook rather than a convenience, on the rule CLAUDE.md states: what
/// cannot be reached from the environment cannot be checked unattended.
/// A held item is a grip, a scale, a flame quad and -- for a lit torch
/// -- the light it throws on the ground, and photographing any of that
/// meant `/give`, which is operator-only, in a singleplayer world that
/// has no operator.
///
/// It changes *what is drawn* and nothing else: the inventory belongs
/// to the server and is not touched, so this cannot desync anything or
/// put an item in a save. What the player is really holding is what
/// they mine with a moment later.
///
/// **Both the model and the light go through here**, because the first
/// draft fed only the model and the photograph came back with a torch
/// burning in the hand and a meadow as dark as it was without one --
/// which looks exactly like a broken shader and was a broken hook.
///
/// Read once. The variable cannot change while the game runs, and this
/// is asked twice a frame.
fn shown_in_hand(
    held: Option<primitive_shared::types::BlockId>,
) -> Option<primitive_shared::types::BlockId> {
    static OVERRIDE: std::sync::OnceLock<Option<primitive_shared::types::BlockId>> =
        std::sync::OnceLock::new();
    OVERRIDE
        .get_or_init(|| {
            let name = std::env::var("PRIMITIVE_HOLD").ok()?;
            let name = name.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let found = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, known)| *known == name)
                .map(|&(id, _)| id);
            if found.is_none() {
                println!("[hold] no block called {name:?}");
            }
            found
        })
        .or(held)
}

/// Which phase of a blow the hand is to be held at, for a photograph.
///
/// **`PRIMITIVE_SWING=<0..1>`.** A blow is under a third of a second
/// long and starts when the player clicks, so `PRIMITIVE_SHOT` -- which
/// photographs a frame at a chosen second -- can only ever catch a hand
/// at rest. Every pose *inside* a blow, which is where an animation is
/// right or wrong, was reachable only with a finger on a mouse button.
///
/// A phase and not a flag, because the frame rate is the enemy here: at
/// a thousand frames a second a sweep of sixteen shots covers a
/// sixtieth of one blow, so "keep swinging and photograph every frame"
/// photographs the same pose sixteen times. `PRIMITIVE_SWING=0.42` is
/// the moment a thrust lands, every time, on any machine.
///
/// It moves the *arm* and nothing else: no block is broken, nothing is
/// sent to the server, and the ray under the crosshair is the untouched
/// camera's -- the hand's geometry lives in a space of its own and
/// touches nothing the ray is built from. See
/// `camera::tests::the_bob_moves_the_picture_and_never_the_ray_the_crosshair_casts`,
/// which is the same property for the other thing that moves the view.
fn photographed_swing() -> Option<f32> {
    static PHASE: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *PHASE.get_or_init(|| {
        let raw = std::env::var("PRIMITIVE_SWING").ok()?;
        let phase: f32 = raw.trim().parse().ok()?;
        if !phase.is_finite() || !(0.0..=1.0).contains(&phase) {
            println!("[swing] PRIMITIVE_SWING wants a phase of a blow, 0 to 1");
            return None;
        }
        Some(phase)
    })
}

/// Is the player to bleed on a timer for a photograph?
///
/// **`PRIMITIVE_BLEED=1`.** Blood, and the marks it leaves on the
/// ground, happen when something takes damage -- and taking damage on
/// purpose means finding a wolf, letting it bite, and holding still
/// while it does. That is a hand-run experiment nobody can repeat,
/// which is exactly what this project's rule says to replace with a
/// hook.
///
/// It emits the same burst the `Health` message does, at the same place
/// on the player's own body, every [`BLEED_EVERY`] seconds. Nothing
/// about the world or the player's health is touched: this is a
/// drawing, not damage.
fn bleeding_for_a_photograph() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("PRIMITIVE_BLEED")
            .map(|value| matches!(value.trim(), "1" | "true" | "yes"))
            .unwrap_or(false)
    })
}

/// How often `PRIMITIVE_BLEED` throws a burst.
///
/// Slower than a wolf and fast enough to have laid several marks by the
/// time a screenshot is due: at three quarters of a second a shot taken
/// six seconds in has eight blows' worth of spatter under the player,
/// which is a picture of the budget as well as of the effect.
const BLEED_EVERY: f32 = 0.75;

fn frame_params(
    settings: &ClientSettings,
    sky: &Sky,
    render_distance: i32,
    // How far the streamed disc reaches, in blocks. The fog is clamped
    // to it so the fade is finished before the terrain is -- see
    // `Fog::clamp_to`. Infinite where there is no world to stream, which
    // is the menu.
    loaded_radius: f32,
    fog_enabled: bool,
    render_origin: Vec3,
    eye: Eye,
) -> FrameParams {
    let Eye { underwater, underground, smoke, health_fraction, held } = eye;
    // What distance looks like this frame, worked out in one place --
    // see `engine::fog`, which exists because the colour, the range and
    // the underwater case used to live in three files that each held a
    // third of the answer.
    let mut fog = fog::Fog::for_frame(settings, sky, render_distance, fog_enabled, underwater);
    // Before the clamp, because both only ever pull the fade *in* and
    // the clamp is the one that knows where the streamed world stops:
    // taking the cave's twenty-four blocks and then letting the disc
    // push them back out would be a fog that ends past the terrain,
    // which is the line round the edge of the world all over again.
    fog.go_underground(underground);
    fog.fill_with_smoke(smoke);
    fog.clamp_to(loaded_radius);

    FrameParams {
        sun_direction: sky.sun_direction(),
        sun_intensity: sky.sun_intensity(),
        // Hue only -- how bright it is, is the line above. See
        // `Sky::sun_color` for why the two are kept apart, and
        // `Sky::sun_color_for` for why the step changes the ramp.
        sun_color: sky.sun_color_for(settings.lighting),
        fill_color: sky.fill_color_for(settings.lighting),
        // Off the fog rather than off the sky, because the fog is what
        // knows the eye is under water or under ground -- where there is
        // no horizon for a sunset to be on.
        horizon_glow: fog.glow,
        sun_haze: fog.haze,
        fog_color: fog.color,
        fog_start: fog.start,
        fog_end: fog.end,
        // What is drawn, as opposed to what fades: see `view_distance`.
        // Taken from the render distance actually in force, which is
        // the player's setting capped by what the server streams --
        // never from the fog, which the player can switch off.
        view_distance: (render_distance as f32) * 16.0,
        ambient: settings.ambient_light,
        // **The one light that walks.** Read off what is actually in the
        // hand rather than off a flag somebody has to remember to clear:
        // a torch that went out is a different block, so putting it out
        // and putting this to zero are the same event and cannot come
        // apart. The strength is the block's own `emission` **in
        // levels** -- the same number the fire would pour into the light
        // map if a torch were ever placed, handed over unscaled because
        // the shader falls off by one level a block exactly as the map
        // floods. Divide it here and the reach stops meaning anything.
        // **`PRIMITIVE_SPECKS=1` turns the world flat black on white.**
        // A hook rather than a setting, on the rule CLAUDE.md states:
        // what cannot be reached from the environment cannot be checked
        // unattended. See `FrameParams::speck_hunt` for what it draws
        // and why the three instruments before it measured the sky.
        speck_hunt: {
            static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            *ON.get_or_init(|| std::env::var("PRIMITIVE_SPECKS").is_ok())
        },
        carried_light: held
            .filter(|&block| primitive_shared::types::is_lit_torch(block))
            .map(|block| f32::from(primitive_shared::blocks::definition(block).emission))
            .unwrap_or(0.0),
        block_light_boost: settings.block_light_boost,
        ao_strength: settings.ambient_occlusion,
        fog_enabled: fog.enabled,
        underwater: fog.underwater,
        fog_cull_distance: fog.cull_distance(),
        time_of_day: sky.time_of_day,
        cloudiness: settings.cloudiness,
        overcast: sky.overcast(),
        render_origin,
        hurt: hurt_from(health_fraction),
        elapsed_seconds: sky.elapsed(),
        cloud_drift: sky.cloud_drift(),
        detail_distance: settings.detail_distance,
        // Where the moon is and how dark its night is: `primitive_shared::moon`.
        moon: sky.moon_uniform(),
    }
}

/// How far the player may walk before the frame's origin moves with
/// them, in blocks.
///
/// Sixty-four. The two costs pull opposite ways: a bigger step means
/// larger numbers reaching the card (at 64 the worst case is a few
/// hundred, which an `f32` holds to a thousandth of a millimetre), and a
/// smaller one means the moving geometry is rebuilt more often than it
/// otherwise would be. Sixty-four blocks is four chunks -- rare enough
/// that the forced rebuild is lost among the ones the clock was going to
/// ask for anyway.
const ORIGIN_STEP: f32 = 64.0;

/// Tells the server whether this player is swinging at a block, for the
/// figure everybody else sees (`ClientMessage::Digging`).
///
/// **On change, and again every [`DIGGING_REPEAT`] while it lasts.** The
/// server lets a swing lapse (`players::DIGGING_LAPSES_AFTER`) so a client
/// that vanished mid-swing does not leave a figure hammering at nothing; the
/// repeat is what keeps an honest one swinging through a lost message. Sent
/// every frame it would be sixty messages a second for one flag.
#[derive(Default)]
struct DigSignal {
    told: bool,
    last_sent: Option<Instant>,
}

/// How often a swing that is still going is said again. A third of the time
/// the server waits before letting it lapse.
const DIGGING_REPEAT: Duration = Duration::from_millis(500);

impl DigSignal {
    /// What to send this frame, if anything.
    fn frame(&mut self, now: Instant, digging: bool) -> Option<bool> {
        let again = digging
            && self
                .last_sent
                .is_none_or(|last| now.saturating_duration_since(last) >= DIGGING_REPEAT);
        if digging == self.told && !again {
            return None;
        }
        self.told = digging;
        self.last_sent = Some(now);
        Some(digging)
    }
}

#[cfg(test)]
mod dig_signal_tests {
    use super::*;

    #[test]
    fn digging_is_told_when_it_starts_again_while_it_lasts_and_when_it_stops() {
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        let mut signal = DigSignal::default();
        assert_eq!(signal.frame(start, false), None, "standing still was reported");
        assert_eq!(signal.frame(start, true), Some(true), "a swing starting was not told");
        assert_eq!(signal.frame(start + frame, true), None, "a swing was told every frame");
        assert_eq!(
            signal.frame(start + DIGGING_REPEAT, true),
            Some(true),
            "a long swing was never said again, and would lapse on the server"
        );
        assert_eq!(signal.frame(start + DIGGING_REPEAT + frame, false), Some(false), "stopping was not told");
        assert_eq!(signal.frame(start + DIGGING_REPEAT * 3, false), None, "not digging was repeated");
    }
}

/// Where to draw this frame from, given where the player is and where it
/// was drawn from last.
///
/// **Whole blocks, and sticky.** Whole, so that the offsets built from
/// it are exact and a chunk boundary lands where a chunk boundary is;
/// sticky, so that walking about does not move it every frame and force
/// the entity meshes to be rebuilt every frame with it.
///
/// Measured in `f64`: the eye is kept in it, and the whole blocks this
/// answers are exact in `f32` everywhere a world is (below sixteen million).
fn render_origin_for(at: glam::DVec3, current: Vec3) -> Vec3 {
    if (at - current.as_dvec3()).length_squared() <= f64::from(ORIGIN_STEP * ORIGIN_STEP) {
        return current;
    }
    at.floor().as_vec3()
}

/// Whether the moving geometry -- entities, rafts, other players, particles --
/// is built again this frame. See [`DYNAMIC_REBUILD_HZ`].
///
/// **On every frame a raft carries the view**, whatever the clock says. The
/// clock is right for things that move by themselves in front of a camera
/// that is still or walking: an animal drawn as it was a frame ago is where it
/// was a frame ago, and nobody can see that. A deck under a passenger is the
/// other way round. The camera rides the deck as it is *this* frame
/// (`riding::Riding::carry_player`), so a deck drawn as it was a frame ago
/// slides a frame's travel back and forth under the view on every frame past
/// a hundred and twenty a second -- measured over a socket as a deck drawn up
/// to 1.5 cm from the one the feet stood on. The rebuild is paid only while
/// standing on a raft.
fn dynamic_rebuild_due(origin_moved: bool, carried: bool, last: Option<Instant>, now: Instant) -> bool {
    origin_moved
        || carried
        || last.is_none_or(|last| now.duration_since(last).as_secs_f32() >= 1.0 / DYNAMIC_REBUILD_HZ)
}

/// Whether other players' figures -- and the block outline that rides in
/// their buffer -- are built again this frame: when the moving geometry is
/// (`dynamic_rebuild_due`), and on every frame anybody else is drawn at all.
///
/// "модель игрока рендерится будто в 30 фпс". A figure is played back between
/// snapshots so that it moves on every frame
/// (`remote_players::PLAYBACK_DELAY_TICKS`), and past a hundred and twenty
/// frames a second the clock above still drew it on every other one: 72
/// pictures a second of a walking figure at 144, measured
/// (`what_a_walking_remote_player_looks_like_frame_by_frame`). A person crossing
/// the view is what the eye follows, which is not the bobbing item the clock
/// was set for.
///
/// Two ways were weighed, measured in release on the same machine:
///
/// * *Everything that moves, every frame, while somebody else is near.* What a
///   player carries rides the entities' buffers (`build_held_items_into`), so
///   this is the only way to keep a tool in step with the hand holding it --
///   and those buffers are the animals: 1.08 ms a rebuild for a full field of
///   sixty (`entities::what_a_full_field_costs_a_frame`). At 144 frames a
///   second that is 72 more of them a second, half a millisecond on the average
///   frame of 6.9, for animals nobody asked about.
/// * **The figures alone (chosen).** 0.006 ms a frame for one player, 0.018 for
///   four, 0.080 for sixteen, playback included
///   (`what_drawing_other_players_costs_a_frame`). What it gives up, above 120
///   frames a second only: a tool in somebody else's hand can trail that hand by
///   one frame, three centimetres at a walking pace at 144.
///
/// Singleplayer has nobody else and pays nothing.
fn figures_rebuild_due(dynamic_due: bool, others_drawn: bool) -> bool {
    dynamic_due || others_drawn
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

/// Which way the player is asking to go, as a unit vector.
///
/// A direction only. *How fast* is `input::stick_speed` and the
/// sprint flag, both folded into physics at the call site -- this
/// answers one question so that a keyboard and a thumb can answer it
/// the same way.
/// What tiredness does to a pace, as a multiplier on speed.
///
/// The client's copy of the server's ramp (`survival::tiredness_factor`
/// against `body::EXHAUSTED_SPEED`), and it is a copy for the reason
/// every predicted number is one: the player has to *feel* the slowdown
/// at their own frame rate rather than be corrected into it a fifth of a
/// second later. The server still owns the fatigue itself -- this reads
/// the number it sent.
fn tiredness_speed(fatigue: f32) -> f32 {
    use primitive_shared::body::{EXHAUSTED_SPEED, TIRED_AT};
    if !fatigue.is_finite() || fatigue <= TIRED_AT {
        return 1.0;
    }
    let past = ((fatigue - TIRED_AT) / (1.0 - TIRED_AT)).clamp(0.0, 1.0);
    1.0 - past * (1.0 - EXHAUSTED_SPEED)
}

fn wish_direction(
    input: &input::InputState,
    camera: &Camera,
    binds: &keybinds::Keybinds,
) -> Vec3 {
    use keybinds::Action;

    // A thumb, if there is one. Checked first and returned from: a
    // device with a stick has no keys to fall through to, and one with
    // keys never sets it.
    if let Some((right, forward)) = input.stick {
        let dir = camera.forward_horizontal() * forward + camera.right_horizontal() * right;
        return if dir.length_squared() > 0.0 {
            dir.normalize()
        } else {
            Vec3::ZERO
        };
    }

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

/// Takes the pointer, if the platform will give it.
///
/// `mouse_grabbed` is set from what actually happened rather than from
/// what was asked for. A game that believes it has the mouse when it
/// does not turns every cursor movement into a camera spin the player
/// cannot stop -- and on a platform with no pointer at all (a phone),
/// asking is not an error, it is a question with no meaning.
fn grab_cursor(window: &dyn platform::Window, input: &mut input::InputState) {
    input.mouse_grabbed = window.set_cursor_grabbed(true);
}

fn release_cursor(window: &dyn platform::Window, input: &mut input::InputState) {
    window.set_cursor_grabbed(false);
    input.mouse_grabbed = false;
}

/// What the crosshair is on, as (cell, block in it).
/// Shuts the chest screen and says so.
///
/// The message matters as much as the screen does: until the server
/// hears it, this player is still counted as standing at that chest and
/// is still sent an update every time anyone else changes it.
/// Shuts the station screen and tells the server, which drops the seat it
/// was holding -- and with it any run the player walked out on.
///
/// `close_chest`'s shape, for `close_chest`'s reason: a screen that traps the
/// player when the connection drops traps them at the worst moment, so the
/// local close happens whether or not there is anybody to tell.
fn close_station(
    screen: &mut station_screen::StationScreen,
    net: Option<&network::NetworkHandle>,
    debug_stats: &mut DebugStats,
) {
    if !screen.is_open() {
        return;
    }
    screen.close();
    if let Some(net) = net {
        net.send(ClientMessage::CloseStation);
        debug_stats.network_messages_out_this_second += 1;
    }
}

/// One place where a station gesture becomes a message.
///
/// Three callers -- the click, the space bar and the frame's own poll -- and
/// the shape of what they send must not drift: a run handed in by the timer
/// and a run handed in by the fourth blow are the same message.
fn send_station_intent(
    intent: station_screen::Intent,
    screen: &mut station_screen::StationScreen,
    net: &network::NetworkHandle,
    debug_stats: &mut DebugStats,
    audio: &audio::Audio,
) {
    match intent {
        station_screen::Intent::Begin(job) => {
            audio.play(audio::Sfx::Click);
            net.send(ClientMessage::StationBegin { job });
        }
        station_screen::Intent::Run(presses) => {
            net.send(ClientMessage::StationRun { presses });
        }
        // Handled by the callers, which have the connection to tell as well.
        station_screen::Intent::Close => {
            screen.close();
            net.send(ClientMessage::CloseStation);
        }
    }
    debug_stats.network_messages_out_this_second += 1;
}

fn close_chest(
    screen: &mut chest_screen::ChestScreen,
    net: Option<&network::NetworkHandle>,
    debug_stats: &mut DebugStats,
) {
    if !screen.is_open() {
        return;
    }
    // A jug looked into in the hand was never opened on the server, so
    // there is nothing there to close -- and a `CloseChest` sent for it
    // would shut whatever container the server does think is open.
    let server_side = screen.held_vessel().is_none();
    // Remembered as the player's own doing, so the update the server may
    // already have sent for this container does not open it again before
    // the `CloseChest` below reaches it -- see `ChestScreen::dismissed`.
    screen.close_by_player();
    if let (true, Some(net)) = (server_side, net) {
        net.send(ClientMessage::CloseChest);
        debug_stats.network_messages_out_this_second += 1;
    }
}

/// Who a blow is aimed at.
///
/// Players before animals, which is the order a swing should resolve in:
/// a person in front of a deer takes the blow, and a deer in front of a
/// wall stops the wall coming apart.
#[derive(Clone, Copy)]
enum Aimed {
    Player(PlayerId),
    Animal(primitive_shared::protocol::EntityId),
}

/// Sends a blow that has just landed.
///
/// Nothing is decided here. How often a blow comes is `hand::Strikes`'s,
/// and reach, damage and whether the target is still alive are the
/// server's, judged against its own positions on arrival.
fn send_blow(target: Aimed, net: &mut network::NetworkHandle, debug_stats: &mut DebugStats) {
    net.send(match target {
        Aimed::Player(target) => ClientMessage::Attack { target },
        Aimed::Animal(target) => ClientMessage::AttackEntity { target },
    });
    debug_stats.network_messages_out_this_second += 1;
}

/// The animal under the crosshair that a blow with `held` would reach.
///
/// The twin of `player_under_crosshair`, and it asks the one thing the
/// server cannot: line of sight. The server knows where everything is
/// and nothing about what is between them.
fn animal_under_crosshair(
    entities: &logic::entities::Entities,
    chunks: &ChunkManager,
    camera: &Camera,
    held: Option<primitive_shared::types::BlockId>,
) -> Option<primitive_shared::protocol::EntityId> {
    use primitive_shared::combat;

    let forward = camera.forward();
    // The weapon decides how far the swing carries: a spear is two
    // metres of haft, a knife an arm's length. See `combat::reach_with`.
    let (target, distance) =
        entities.aimed_at(camera.position, forward, combat::reach_with(held))?;
    if physics::raycast_block(chunks, camera.position, forward, distance).is_some() {
        return None;
    }
    Some(target)
}

/// The player under the crosshair that a blow with `held` would reach.
///
/// Whether anyone is there *at all*, not whether a punch is due: the
/// caller keeps mining out of the way on it, and the wall behind someone
/// must not quietly start coming apart in the gaps between swings.
///
/// Line of sight is decided here and not trusted by the server, which
/// knows how far apart two players are and nothing about what is between
/// them. The rate used to be decided here as well, against a `last_swing`
/// instant; it is `hand::Strikes` now, because a thrust that lands late
/// has to be timed from when it started and not from when it was sent.
fn player_under_crosshair(
    players: &RemotePlayers,
    chunks: &ChunkManager,
    camera: &Camera,
    held: Option<primitive_shared::types::BlockId>,
) -> Option<PlayerId> {
    use primitive_shared::combat;

    let forward = camera.forward();
    let (target, distance) =
        players.aimed_at(camera.position, forward, combat::reach_with(held))?;
    // Whatever the ray meets first wins. A punch that lands through a
    // wall is the sort of thing a player remembers about a game.
    if physics::raycast_block(chunks, camera.position, forward, distance).is_some() {
        return None;
    }
    Some(target)
}

/// What the right click is pointed at.
///
/// **Water counts here and nowhere else.** Everything else that casts a
/// ray -- mining, placing, the crack overlay -- must pass straight
/// through a lake to the bed under it; the right click is the one
/// gesture whose answer *is* the lake, because drinking from a river is
/// a right click at a river. See `geometry::block_box_for_aim` for what
/// this cost while it was not true: every drinking rule on the server
/// was unreachable, and the game simply did nothing when a thirsty
/// player clicked at water.
fn aimed_block(chunks: &ChunkManager, camera: &Camera) -> Option<((i32, i32, i32), BlockId)> {
    // **Under water the right click looks through the water**, the pick's
    // way (`aimed_block_to_mine`). A swimmer's ray began in water and stopped
    // at the first cell of it, so a fish trap an arm's length away on the
    // bed could not be emptied from beside it -- the click drank the lake.
    // Only when nothing solid is in reach is the water itself the answer,
    // so a drink is still a click away from the surface as it always was.
    let eye = camera.position;
    let submerged = chunks
        .block_at(eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32)
        .is_some_and(primitive_shared::types::is_liquid);
    if submerged {
        if let Some(hit) = aimed_block_for(chunks, camera, false) {
            return Some(hit);
        }
    }
    aimed_block_for(chunks, camera, true)
}

/// What the swing is pointed at: the same ray, **blind to water**.
///
/// ## The bug this exists to end
///
/// A player reported that nothing can be broken under water. It could not,
/// and the cause is one flag two gestures were sharing. Water was let into
/// the aim ray so that a right click at a river could be a drink -- the
/// comment on [`aimed_block`] says so -- and the pick was aiming down the
/// same ray. Under water, or across the surface of a lake from the bank,
/// that ray stops at the first cell of water; `is_breakable_with` then
/// throws the water away because water is not a thing a pick gets through,
/// and the frame ends with no target at all. No cracks, no progress bar,
/// nothing sent: a player swinging at stone they are standing on, with the
/// game doing nothing and saying nothing.
///
/// The two gestures want opposite answers from the same ray, which is why
/// this is two calls and not a cleverer filter. The right click's answer to
/// "what is in front of me" genuinely *is* the lake. The pick's answer never
/// is -- a ray that stopped at water could not reach the bed under it, and
/// the bed is the whole of what is down there worth breaking.
fn aimed_block_to_mine(chunks: &ChunkManager, camera: &Camera) -> Option<((i32, i32, i32), BlockId)> {
    aimed_block_for(chunks, camera, false)
}

/// Which face of that block the swing is landing on: the step from the
/// block struck to the empty cell in front of it, which is what
/// `dig::Side::from_normal` reads.
///
/// **The same ray, run again, and only on the frame a swing lands.** The
/// alternative is threading the face through `Mining::update` and every
/// one of its forty test call sites, for a value the bar itself has no use
/// for -- what the bar counts is seconds against a block, and which way
/// the digger is facing is not part of that. A raycast is a few dozen cell
/// lookups and this runs once per slice, not once per frame.
fn aimed_face_to_mine(chunks: &ChunkManager, camera: &Camera) -> Option<(i8, i8, i8)> {
    let (hit, before) = physics::raycast_block_for(
        chunks,
        camera.position,
        camera.forward(),
        INTERACT_RANGE,
        false,
    )?;
    Some((
        (before.0 - hit.0) as i8,
        (before.1 - hit.1) as i8,
        (before.2 - hit.2) as i8,
    ))
}

fn aimed_block_for(
    chunks: &ChunkManager,
    camera: &Camera,
    include_liquid: bool,
) -> Option<((i32, i32, i32), BlockId)> {
    let (hit, _) = physics::raycast_block_for(
        chunks,
        camera.position,
        camera.forward(),
        INTERACT_RANGE,
        include_liquid,
    )?;
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

/// Asks the server to take one slice off a block the player is quarrying.
///
/// **Not a break, and deliberately a different message.** The cell does
/// not become air, nothing drops and the tool is not worn: what the server
/// does with this is work out the next shape itself and write it back
/// (`net::connection::dig_one_slice`). The client predicts none of it --
/// the block changes when a `BlockUpdate` says it has, which is the rule
/// `request_break` states and for the same reason.
fn request_dig(
    chunks: &ChunkManager,
    cell: (i32, i32, i32),
    face: (i8, i8, i8),
    net: &mut network::NetworkHandle,
    debug_stats: &mut DebugStats,
) {
    if chunks.block_at(cell.0, cell.1, cell.2).is_none() {
        return; // the chunk went away mid-swing
    }
    net.send(ClientMessage::Dig {
        global_x: cell.0,
        global_y: cell.1,
        global_z: cell.2,
        face,
    });
    debug_stats.network_messages_out_this_second += 1;
}

/// Is a lit campfire close enough to work at -- and which workshops are?
///
/// A cube of cells around the player's feet, of exactly the radius the
/// server measures -- see `types::FIRE_WORKING_RANGE`. A scan rather
/// than an index because the volume is seven cells on a side at three
/// metres, which is under four hundred block lookups once a frame, and
/// an index over "where are the fires" is a structure the client would
/// have to keep in step with every block update for the sake of a
/// question asked sixty times a second.
///
/// The distance test is to the *centre* of the fire's cell and from the
/// player's feet, which is what the server does too: two sides using
/// different corners of the same cell is a metre of disagreement, and a
/// metre is the difference between a recipe the menu offers and one the
/// server refuses.
fn fire_within_reach(chunks: &ChunkManager, feet: Vec3) -> primitive_shared::crafting::Heat {
    use primitive_shared::types::{block_kind, BLOCK_BLOOMERY_LIT, BLOCK_KILN_LIT};
    let mut heat = primitive_shared::crafting::Heat::NONE;
    let range = primitive_shared::types::FIRE_WORKING_RANGE;
    let reach = range.ceil() as i32;
    let (fx, fy, fz) = (
        feet.x.floor() as i32,
        feet.y.floor() as i32,
        feet.z.floor() as i32,
    );
    for dy in -reach..=reach {
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (x, y, z) = (fx + dx, fy + dy, fz + dz);
                let Some(block) = chunks.block_at(x, y, z) else {
                    continue;
                };
                // A workshop is asked in the same pass and by the same
                // distance, because the server asks it that way too
                // (`heat_within_reach`) -- see `crafting::Station::Bench`.
                let workshop = primitive_shared::crafting::Station::of_workshop(block);
                if workshop.is_none() && !primitive_shared::types::is_burning(block) {
                    continue;
                }
                let (ox, oy, oz) = (
                    x as f32 + 0.5 - feet.x,
                    y as f32 + 0.5 - feet.y,
                    z as f32 + 0.5 - feet.z,
                );
                if ox * ox + oy * oy + oz * oz > range * range {
                    continue;
                }
                // Which fire it is decides which recipes light up. Both
                // are kept looking rather than returning at the first
                // hit: a player standing between a campfire and a kiln
                // has both, and the menu has to say so.
                if let Some(station) = workshop {
                    heat = heat.with_workshop(station);
                    continue;
                }
                match block_kind(block) {
                    BLOCK_KILN_LIT => heat.kiln = true,
                    BLOCK_BLOOMERY_LIT => heat.bloomery = true,
                    _ => heat.fire = true,
                }
            }
        }
    }
    heat
}

#[allow(clippy::too_many_arguments)]
fn try_place_block(
    chunks: &ChunkManager,
    camera: &Camera,
    input: &input::InputState,
    player: &Player,
    others: &[glam::DVec3],
    net: &mut network::NetworkHandle,
    inventory: &Inventory,
    // Told what was asked for, so that the *confirmation* can be
    // animated. See `Mining::note_placed`: nothing is drawn here,
    // because nothing has happened yet.
    mining: &mut mining::Mining,
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

    // Which way it lies and which way it looks: along the clicked face's
    // axis for a length of timber, toward the placer for anything with a
    // front, out of the clicked face for a shelf held up from the side.
    // One rule in `types::placed` rather than the two it used to be here,
    // one of which could not hang a bracket fungus on half of a trunk.
    // `before` and `hit` differ by exactly one cell along the face's
    // normal, so the face is the difference.
    let block_id = primitive_shared::types::placed(
        block_id,
        camera.yaw,
        (before.0 - hit.0, before.1 - hit.1, before.2 - hit.2),
    );

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
        // The cell that holds it, which is beneath it for everything but
        // a bracket fungus -- see `types::support_at`.
        let (dx, dy, dz) = primitive_shared::types::support_at(block_id);
        let holding = chunks
            .block_at(target.0 + dx, target.1 + dy, target.2 + dz)
            .unwrap_or(BLOCK_AIR);
        if !primitive_shared::types::can_grow_on(block_id, holding) {
            return;
        }
    }

    // ...and a bed needs its second cell, behind the first, on the same
    // terms. The server refuses it with a reason; asked here as well so a
    // bed that cannot go down is never sent. See `types::BED_HEAD`.
    if let Some((head_at, head)) = primitive_shared::types::bed_partner(target, block_id) {
        let there = chunks.block_at(head_at.0, head_at.1, head_at.2);
        let under = chunks.block_at(head_at.0, head_at.1 - 1, head_at.2).unwrap_or(BLOCK_AIR);
        let free = there.is_some_and(|b| primitive_shared::types::layer_placement(b, head).is_some());
        if !free || !primitive_shared::types::can_grow_on(head, under) {
            return;
        }
        let feet = (player.position.x, player.position.y, player.position.z);
        if block_overlaps_player(feet, head_at.0, head_at.1, head_at.2, head) {
            return;
        }
    }

    // ...and a door needs the cell over it for its top half, on the same
    // terms again. See `types::BLOCK_DOOR`.
    if let Some((top_at, top)) = primitive_shared::types::door_partner(target, block_id)
        .filter(|_| primitive_shared::types::block_kind(block_id) == primitive_shared::types::BLOCK_DOOR)
    {
        let free = chunks
            .block_at(top_at.0, top_at.1, top_at.2)
            .is_some_and(|b| primitive_shared::types::layer_placement(b, top).is_some());
        if !free {
            return;
        }
        let feet = (player.position.x, player.position.y, player.position.z);
        if block_overlaps_player(feet, top_at.0, top_at.1, top_at.2, top) {
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
    // Remembered, not drawn. The settle plays when the server agrees --
    // see `Mining::await_placement`.
    mining.await_placement(target, block_id);
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
    last_sent_transform: &mut Option<(glam::DVec3, f32, f32)>,
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
fn respawn_gate(chunks: &ChunkManager, x: f64, z: f64, world_ready: &mut bool) {
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
    // Chips fly off a block the moment the server says it is gone.
    particles: &mut engine::particles::Particles,
    health: &mut f32,
    max_health: &mut f32,
    recent_health: &mut f32,
    // How much air is left, 0..1. Only ever below 1 under water.
    breath: &mut f32,
    // How thick the smoke is where the eyes are, 0..1.
    smoke: &mut f32,
    // How full the player is, 0..1.
    nourishment: &mut f32,
    // How warm and how watered. See the declaration in `run`.
    body: &mut ui::hud::BodyGauges,
    // Whether the player is asleep, and the dark that follows it.
    // Server-owned: while they are, the client stops simulating the body
    // at all, because the server has stopped listening to it. See
    // `ServerMessage::Asleep`.
    sleep: &mut logic::posture::Sleep,
    // Sitting, lying or standing, as the server last said. See
    // `ServerMessage::Posture` and `logic::posture`.
    resting: &mut logic::posture::Resting,
    // What the player has on.
    equipment: &mut primitive_shared::inventory::Equipment,
    // What the sky is doing, for the renderer and the debug panel.
    weather: &mut primitive_shared::weather::Weather,
    shake: &mut shake::Shake,
    stamina: &mut stamina::Stamina,
    // Set while the player is dead, and what killed them.
    death: &mut death::DeathScreen,
    // The chest the player has open, and what is in it.
    chest_screen: &mut chest_screen::ChestScreen,
    // The anvil or wheel the player has open, and the run on it.
    station_screen: &mut station_screen::StationScreen,
    // The last thing the server refused, for the HUD to show.
    notice: &mut Option<(String, Instant)>,
    // The interface language, for the lines this loop writes into
    // `notice` itself rather than passing on from the server -- the heat
    // warnings (`hud::heat_notice`).
    language: ui::lang::Language,
    // Everything anyone said, including the server.
    chat: &mut chat::Chat,
    // Whether the ground under the player exists yet. Cleared by every
    // teleport, and that is not housekeeping -- see `respawn_gate`.
    world_ready: &mut bool,
    // A block coming apart makes a noise as well as chips, and the
    // messages carrying damage, death and chat are where those are
    // heard.
    audio: &Audio,
    soundscape: &mut Soundscape,
    // Where the extensions list lands. The menu is the only thing that
    // reads it, and it arrives here because this is where every other
    // answer from the server does.
    menu: &mut ui::menu::Menu,
    // What the map and the recipe book are drawn from: the server's word
    // on what was held and where the bags are, and every edit, which
    // may have changed what the top of a column is.
    journal: &mut ui::journal::Journal,
    // The line in the water, as the server last described it. See
    // `ServerMessage::Line` and `logic::fishing`.
    fishing_float: &mut Option<logic::fishing::Float>,
) {
    use tokio::sync::mpsc::error::TryRecvError;

    loop {
        let msg = match net.to_game.try_recv() {
            // Just queue it. Integrating a chunk means lighting it,
            // which is far too expensive to do for every chunk that
            // happens to be sitting in the socket buffer this frame.
            // It is already packed -- see `network::Incoming`.
            Ok(network::Incoming::Chunk(chunk)) => {
                debug_stats.network_messages_in_this_second += 1;
                // Mark it satisfied immediately, or the retry logic will
                // keep asking for a chunk that's already in the queue.
                chunks.note_arrival(chunk.pos);
                arrivals.push(*chunk);
                continue;
            }
            Ok(network::Incoming::Message(msg)) => msg,
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
            // The network thread turns every one of these into
            // `Incoming::Chunk` before it gets here. Handled rather than
            // left to a catch-all, so a chunk that ever does arrive this
            // way is packed and kept instead of silently becoming a hole.
            ServerMessage::ChunkData(chunk) => {
                chunks.note_arrival(chunk.pos);
                arrivals.push(primitive_shared::packed::PackedChunk::pack(&chunk));
            }

            ServerMessage::BlockUpdate(change) => {
                mining.confirm_placement(&change, Instant::now());
                burst_for(particles, chunks, &change);
                sound_for(audio, soundscape, chunks, &change);
                entities.on_block_placed(
                    change.global_x,
                    change.global_y,
                    change.global_z,
                    change.block_id,
                );
                journal.explored.note_edit(change.global_x, change.global_z);
                apply_change(chunks, light, arrivals, urgent, dirty_set, versions, change);
            }

            ServerMessage::BlockUpdates(changes) => {
                for change in changes {
                    mining.confirm_placement(&change, Instant::now());
                    burst_for(particles, chunks, &change);
                    sound_for(audio, soundscape, chunks, &change);
                    entities.on_block_placed(
                        change.global_x,
                        change.global_y,
                        change.global_z,
                        change.block_id,
                    );
                    journal.explored.note_edit(change.global_x, change.global_z);
                    apply_change(chunks, light, arrivals, urgent, dirty_set, versions, change);
                }
            }

            // The tick, not the clock: see `RemotePlayers::apply_snapshot`.
            ServerMessage::Snapshot { tick, states } => {
                remote_players.apply_snapshot(tick, &states, Some(my_id));
            }

            ServerMessage::Entities { tick, states } => {
                // The horse under this client, as the server has it: what the
                // prediction is eased toward (`Horseback::server_saw`).
                if let Some(riding) = entities.horseback.as_mut() {
                    riding.server_saw(&states);
                }
                // The tick, not the clock: see `Entities::apply_snapshot`
                // for why measuring the gap between arrivals made
                // animals spin.
                entities.apply_snapshot(tick, &states);
            }

            ServerMessage::PlayerJoined { id, username } => {
                chat.note(&format!("{username} joined"), Instant::now());
                if id != my_id {
                    println!("[chat] {username} joined");
                    remote_players.on_join(id, username);
                }
            }

            // Already here when this client came: a name, and no "joined".
            ServerMessage::PlayerPresent { id, username } => {
                if id != my_id {
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
                // **The give menu's own answer, taken out before the log
                // sees it.** A command sent from a screen is answered the
                // way a typed one is -- there is no protocol message for
                // it -- and the answer belongs on that screen: a player
                // who is not an operator has to be told *there*, or the
                // menu reads as broken. Letting it through as well would
                // put refusals back in the chat log, which is exactly
                // what "Отказы больше не пишутся в чат" took out. Only
                // ever true while the menu is waiting for one, so a
                // typed `/time` still answers into the log.
                // `continue` and not `return`: this is one message of a
                // drain, and leaving early would drop everything still
                // queued behind it.
                if from.is_none() && journal.take_server_note(&text) {
                    continue;
                }
                audio.play(audio::Sfx::Message);
                match from {
                    // By id and not by name: two players may be called
                    // the same thing on a server that allows it, and a
                    // log that highlighted by name would show one of
                    // them the other's lines as their own.
                    Some(id) => chat.said(&username, id == my_id, &text, Instant::now()),
                    // `from: None` is the server speaking in its own
                    // name -- command replies, deaths, join and leave
                    // notices.
                    None => chat.note(&text, Instant::now()),
                }
            }

            ServerMessage::Extensions(list) => {
                // Straight onto the menu, which is the only thing that
                // reads it. Arrives unsolicited only in the sense that
                // the player asked a few frames ago -- see
                // `Action::OpenExtensions`.
                println!("[extensions] {} loaded", list.items.len());
                menu.set_extensions(list);
            }

            ServerMessage::Discovered { kinds } => {
                // Repaired on the way in: a list off a socket is a claim.
                journal.discovered = primitive_shared::discovery::Discovered::from_kinds(kinds);
            }

            ServerMessage::Landmarks { spawn, bags } => {
                journal.landmarks = logic::map::Landmarks {
                    spawn: Some(spawn),
                    bags,
                };
            }

            ServerMessage::TimeSync { time_of_day, world_days, .. } => {
                sky.on_time_sync(time_of_day, world_days);
            }

            ServerMessage::Flight { enabled, speed } => {
                // Only ever obeyed, never decided. See
                // `Player::flying` and the protocol note on this
                // message. Printed rather than shown on the HUD:
                // whatever granted it has a player to talk to and will
                // have said so in its own words.
                if enabled != player.flying {
                    println!(
                        "[flight] {}",
                        if enabled { "granted" } else { "withdrawn" }
                    );
                }
                player.set_flying(enabled, speed);
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
                player.teleport(glam::DVec3::new(x, y, z));
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
                // Something arrived in the pack that was not there
                // before. With both screens shut that can only be
                // something walked over, which is the one case worth a
                // sound of its own -- a stack moved between two squares
                // of an open inventory already has one, and crafting
                // has its own.
                if state.total_items() > inventory.total_items()
                    && !inventory_screen.open
                    && !chest_screen.is_open()
                            && !station_screen.is_open()
                {
                    audio.play(audio::Sfx::Pickup);
                }
                *inventory = state;
                // A pick-up in progress is only a slot index, and this
                // may have just changed what is in it.
                inventory_screen.sync(inventory);
                chest_screen.sync_with(inventory);
            }

            // ---- the anvil and the potter's wheel ----
            //
            // Three answers and no state: the screen is a bar and a list, and
            // everything that is *true* -- what the run cost, what it made,
            // how it went -- lives on the server and arrives as the pack.
            ServerMessage::StationOpen { game, tolerance } => {
                // **On the answer, not on the click** -- the chest's own
                // rule, and for the chest's reason: the server decides
                // whether there is a hammer in the hand, and a sound
                // played when the player clicked would be a workshop
                // opening for somebody it then refused.
                audio.play(audio::Sfx::StationOpen);
                station_screen.show(game, tolerance);
            }
            ServerMessage::StationBegun { seed } => {
                station_screen.begun(seed);
            }
            ServerMessage::StationResult { verdict, made } => {
                station_screen.finished(verdict, made);
            }

            // ---- the line in the water ----
            //
            // **The float is the server's**, phase and all: the dip is a
            // moment the player has under a second to answer, and a client
            // that worked it out for itself would be answering its own
            // guess. What this side keeps is how it is *drawn* -- the bob,
            // the twitches, the splash -- see `logic::fishing`.
            ServerMessage::Line {
                float,
                phase,
                strain,
                liveliness,
            } => match float {
                Some(at) => {
                    let phase = logic::fishing::Phase::of_code(phase);
                    let strain = f32::from(strain) / 255.0;
                    let liveliness = f32::from(liveliness) / 255.0;
                    match fishing_float.as_mut() {
                        // The same line, further along: only the phase's own
                        // clock restarts, so the bob and the twitches do not
                        // jump every time the strain moves.
                        Some(old) if old.at == at => {
                            if old.phase != phase {
                                old.since = 0.0;
                            }
                            old.phase = phase;
                            old.strain = strain;
                            old.liveliness = liveliness;
                        }
                        _ => {
                            *fishing_float = Some(logic::fishing::Float {
                                at,
                                // The slot the rod is in, found by
                                // looking: this side normally set the
                                // float itself when it threw, and this is
                                // the other case -- a line the server is
                                // describing that this client did not
                                // draw, after a reconnect.
                                slot: (0..primitive_shared::inventory::HOTBAR_SLOTS)
                                    .find(|&slot| {
                                        inventory.block_in(slot).map(primitive_shared::types::block_kind)
                                            == Some(primitive_shared::types::BLOCK_FISHING_ROD)
                                    })
                                    .unwrap_or(0),
                                fish_before: inventory.count(primitive_shared::types::BLOCK_RAW_FISH),
                                age: 0.0,
                                phase,
                                strain,
                                liveliness,
                                since: 0.0,
                            });
                        }
                    }
                }
                // **Out of the water, however it came out** -- and this is
                // where a catch is told from a loss, because it is the only
                // place both facts are in hand at once. A fish in the pack
                // is what a catch *is* (the inventory arrives before this
                // message does, so the count is already the new one); a line
                // that was fighting and came back with nothing is a line
                // that parted, and that is said here in the player's
                // language rather than sent from the server in English.
                None => {
                    if let Some(gone) = fishing_float.take() {
                        let at = gone.position(|x, y, z| chunks.block_at(x, y, z));
                        let at = glam::Vec3::new(at.0, at.1, at.2);
                        if inventory.count(primitive_shared::types::BLOCK_RAW_FISH) > gone.fish_before {
                            particles.catch_splash(at);
                            audio.play(audio::Sfx::Splash);
                        } else if gone.phase == logic::fishing::Phase::Fighting {
                            *notice = Some((language.text(ui::lang::Msg::FishingLineGone).to_string(), Instant::now()));
                        }
                    }
                }
            },

            ServerMessage::ChestState {
                global_x,
                global_y,
                global_z,
                inventory: mut contents,
                kind,
                hearth,
                rack,
            } => {
                contents.sanitize();
                // Not for a container the player shut and has not asked
                // for since: an update already in the socket when they
                // pressed Escape is not an answer. See
                // `ChestScreen::dismissed`.
                if !chest_screen.wants_state_for((global_x, global_y, global_z)) {
                    continue;
                }
                // The answer to "open that chest" is also what opens the
                // screen: showing an empty one for a round trip would be
                // showing something a player will act on.
                // The block is passed along so the screen can call itself
                // a backpack rather than a chest -- the message says
                // where, and only the world says what.
                chest_screen.show(
                    (global_x, global_y, global_z),
                    contents,
                    // A horse's bags are not in a cell: the screen is told
                    // what it is by the kind, which is all there is to say.
                    if kind == primitive_shared::protocol::ContainerKind::Saddlebags {
                        Some(primitive_shared::types::BLOCK_SADDLEBAGS)
                    } else {
                        chunks.block_at(global_x, global_y, global_z)
                    },
                    kind,
                    hearth,
                    rack,
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

            ServerMessage::Smoke { thickness } => {
                *smoke = if thickness.is_finite() { thickness.clamp(0.0, 1.0) } else { 0.0 };
                body.smoke = *smoke;
            }

            // The place, for the health page. Cleaned like everything off a
            // socket: a NaN would print in the middle of a page of numbers.
            ServerMessage::Shelter { reading } => {
                let clean = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
                body.shelter = primitive_shared::shelter::Reading {
                    air_c: clean(reading.air_c, primitive_shared::body::NEUTRAL_C).clamp(-80.0, 80.0),
                    draught: clean(reading.draught, 0.0).clamp(0.0, 1.0),
                    keeps_out: clean(reading.keeps_out, 0.0).clamp(0.0, 1.0),
                    ..reading
                };
            }

            // A fish on the bank, by name, in the player's words. The splash
            // and the count in the pack are the `Line` message's; this is only
            // the telling. See `ServerMessage::Caught`.
            ServerMessage::Caught { species } => {
                *notice = Some((
                    format!(
                        "{}: {}",
                        language.text(ui::lang::Msg::FishingCaught),
                        ui::names::animal(species, language)
                    ),
                    Instant::now(),
                ));
            }

            // Somebody -- this player, another, or an animal at the
            // palisade -- run onto sharpened stakes. Placed, even for the
            // victim: at their own knee it is as good as in their head, and
            // one path means a player cannot hear it where nobody else does.
            // The blow itself is still `Hurt`, off `Health`; this is what the
            // blow was.
            ServerMessage::Staked { at } => {
                audio.play_at(audio::Sfx::Staked, glam::DVec3::new(at.0, at.1, at.2), 0.9, 1.0);
            }

            // A rack refused something because it is the other rack's work.
            // Said here, in the player's language, where every refusal
            // lands (the banner the `Error` arm writes).
            ServerMessage::RackRefused { rack } => {
                let line = match rack {
                    primitive_shared::rack::Trade::Larder => ui::lang::Msg::LarderRefusesSkins,
                    primitive_shared::rack::Trade::Skins => ui::lang::Msg::FrameRefusesFood,
                };
                *notice = Some((language.text(line).to_string(), Instant::now()));
            }

            ServerMessage::ChestLid { x, y, z, open } => {
                // The lid changes hands between the mesh and the frame here
                // and nowhere else (`ChunkManager::note_chest_lid`), so the
                // chunk is meshed again on exactly the frames it does --
                // urgently, because it is a chest somebody is standing at
                // and the lid does not start to swing until that mesh lands
                // (`ChunkManager::note_meshed_lids`).
                if chunks.note_chest_lid((x, y, z), open) {
                    let pos = ChunkPos::from_world(x, z);
                    bump_version(versions, pos);
                    mark_urgent(urgent, dirty_set, pos);
                }
                // ...and it is heard where it is. **Not by whoever opened
                // it**: their own screen plays the same sound as it opens
                // (`chest_was_open`), and playing it here as well is the
                // chest that creaks twice.
                if chest_screen.at() != Some((x, y, z)) {
                    let sfx = if open { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose };
                    audio.play_at_block(sfx, (x, y, z), 0.9, 1.0);
                }
            }

            ServerMessage::SetDownItem { x, y, z, item } => {
                // What lies in a cell a hand set something down in. Drawn by
                // the frame, not the mesher (`entities::build_set_down_into`),
                // so there is no chunk to mesh again.
                chunks.note_set_down((x, y, z), item);
            }

            ServerMessage::PitPottery { x, y, z, pieces } => {
                // What is in a pit kiln, for the mesher to draw each piece as
                // itself -- see `ChunkManager::pit_pottery`. The chunk is
                // meshed again, urgently: it is a pit somebody is standing
                // over, putting things in.
                chunks.note_pit_pottery((x, y, z), pieces);
                let pos = ChunkPos::from_world(x, z);
                bump_version(versions, pos);
                mark_urgent(urgent, dirty_set, pos);
            }

            ServerMessage::Nourishment { fraction } => {
                *nourishment = fraction.clamp(0.0, 1.0);
            }

            ServerMessage::Body {
                temperature_c,
                comfort,
                hydration,
                fatigue,
                recovery,
                wetness,
                grime,
                diet_groups,
            } => {
                // Comfort itself is never sent; what it is worth to the
                // breath is, because stamina is predicted here.
                stamina.set_recovery(recovery);
                // ...and the same number is kept for the health page,
                // which shows comfort as this multiplier rather than as a
                // hidden score. Clamped like everything else off a
                // socket: a NaN would print as "NaN" in the middle of a
                // page of numbers.
                let clean = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
                body.recovery = clean(recovery, 1.0).clamp(0.0, 4.0);
                body.wetness = clean(wetness, 0.0).clamp(0.0, 1.0);
                body.grime = clean(grime, 0.0).clamp(0.0, 1.0);
                body.diet_groups = diet_groups.min(4);
                // Sanitised on the way in like everything else off a
                // socket: a NaN here would put a gauge at an
                // unpredictable width and, worse, would compare false
                // against every threshold.
                body.temperature_c = if temperature_c.is_finite() {
                    temperature_c
                } else {
                    primitive_shared::body::NEUTRAL_C
                };
                // Said on the crossing, from the server's own band, so the
                // words and the gauge cannot disagree about when it
                // happened. See `hud::heat_notice` for which crossings
                // speak and `hud::heat_notice_allowed` for why a body
                // sitting on a line does not make the strip chatter.
                if let Some(line) = ui::hud::heat_notice(body.comfort, comfort) {
                    let now = Instant::now();
                    if ui::hud::heat_notice_allowed(line, notice.as_ref(), language, now) {
                        notice.replace((language.text(line).to_string(), now));
                    }
                }
                body.comfort = comfort;
                body.hydration = if hydration.is_finite() {
                    hydration.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                body.fatigue = if fatigue.is_finite() {
                    fatigue.clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }

            // **Every wound, whole, and a limp is predicted from it.** The
            // client applies a broken leg's pace, a broken arm's slower dig
            // and the lost sprint itself (see the `speed_scale` and the
            // mining step), because a server that refused the difference
            // would drag a limping player back twenty times a second. The
            // same value is what the mannequin in the pack is drawn from.
            //
            // Said on the change, in the player's language, from
            // `mannequin::notice` -- which used to be two hard-coded English
            // lines about a leg, one of them telling a player with a broken
            // leg to "get home and sleep", advice this change made wrong.
            ServerMessage::Injuries { mut injuries } => {
                // Off a socket, so repaired before anything reads it.
                injuries.sanitize();
                if let Some(line) = ui::mannequin::notice(&body.injuries, &injuries) {
                    notice.replace((language.text(line).to_string(), Instant::now()));
                }
                body.injuries = injuries;
            }

            // **Asleep, and the client stops predicting.** The server
            // ignores a sleeper's transforms, so a client that went on
            // walking the body would send positions nobody reads and
            // draw a player who is not where the world says they are.
            // **Sat down, lay down or got up**, and where the body was put
            // to do it. Not a correction -- see the message -- so no notice
            // about being moved: the camera coming down onto the stool is
            // the notice.
            // **The oars, taken or let go.** What turns the movement keys into
            // strokes, and starts or stops predicting the raft (see
            // `Entities::steer`). The `Posture` beside it moves the body.
            ServerMessage::Oars { raft } => {
                entities.steer(raft);
            }
            // **On a horse or off it**, and what it has in it. The first names
            // the horse and starts the prediction where the server has it; the
            // ones after it, twice a second, only correct the wind
            // (`Horseback::told`). The `Posture` beside the first has already
            // put the body on the saddle.
            ServerMessage::Mounted { horse, at, yaw, wind, fettle } => match horse {
                Some(id) => match entities.horseback.as_mut().filter(|h| h.horse == id) {
                    Some(riding) => riding.told(wind, fettle),
                    None => {
                        entities.horseback = Some(logic::horseback::Horseback::new(
                            id,
                            glam::DVec3::new(at.0, at.1, at.2),
                            yaw,
                            wind,
                            fettle,
                        ));
                    }
                },
                None => {
                    entities.horseback = None;
                    entities.set_ridden(None);
                }
            },
            ServerMessage::Posture { posture, at, yaw } => {
                // The block the body was put on, for whether the seat has a
                // front to face (`Resting::from_wire`). A seat's top is the
                // body's feet, so the cell is the one a hair below them.
                let seat = at.and_then(|(x, y, z)| {
                    chunks.block_at(x.floor() as i32, (y - 0.01).floor() as i32, z.floor() as i32)
                });
                *resting = logic::posture::Resting::from_wire(posture, yaw, seat);
                if let Some((x, y, z)) = at {
                    player.teleport(glam::DVec3::new(x, y, z));
                    respawn_gate(chunks, x, z, world_ready);
                }
            }

            // **Asleep or awake, and the screen follows.** This used to put
            // "asleep -- press any key to get up" and then "awake" in the
            // notice, in English, fifty milliseconds apart. The dark says the
            // first now (`ui::sleep`), in the player's language, and the
            // morning is said once the dark has lifted -- see the frame loop.
            ServerMessage::Asleep { asleep } => {
                sleep.set_asleep(asleep);
            }

            ServerMessage::EquipmentState { equipment: worn } => {
                *equipment = worn;
                // Repaired rather than trusted, on the same terms the
                // pack is: this came off a socket, and a garment filed
                // against the wrong body part would be drawn in the
                // wrong square.
                equipment.sanitize();
            }

            ServerMessage::WeatherSync { weather: new } => {
                *weather = new;
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
                    soundscape.on_hurt(audio, *health - current);
                    // **No blood here any more.** This drew a burst at
                    // the chest for every drop in health -- and health
                    // also goes to illness, hunger, thirst and the cold,
                    // a crumb a tick. A raw fish was twenty bursts a
                    // second for as long as it made the player ill, which
                    // a player reported as a fountain of blood. Blood is
                    // `ServerMessage::Blood` now, sent for a blow or an
                    // open cut and for nothing else.
                } else {
                    // Healing has no ghost to leave behind.
                    *recent_health = current;
                }
                *health = current;
            }

            // A blow landed or a cut dripped, on this player or on
            // somebody near -- drawn where the server says, once, for
            // everybody. See the message for why nothing predicts it.
            ServerMessage::Blood { at, drops } => {
                particles.spray(Vec3::new(at.0 as f32, at.1 as f32, at.2 as f32), usize::from(drops));
            }

            ServerMessage::Died { cause } => {
                // Dead is not seated: the server lets go of a seat and a bed
                // at the respawn without a word, and a camera left at seat
                // height would come back into the world at the spawn point
                // looking out of the ground.
                *resting = logic::posture::Resting::Standing;
                println!("[survival] you died: {cause}");
                audio.play(audio::Sfx::Death);
                // An animal's words said again in the player's language; any
                // other cause as the server wrote it. See `names::death_cause`.
                death.open(ui::names::death_cause(&cause, language).into_owned());
                // Whatever was half-mined is not half-mined any more.
                mining.reset();
            }

            ServerMessage::Respawned { x, y, z } => {
                player.teleport(glam::DVec3::new(x, y, z));
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
                // ...and whole, before the server says so. The body a
                // player comes back in has no wounds (`Vitals::respawn`),
                // and the message that says it arrives a tick later -- by
                // which time the notice would compare it with the corpse
                // and announce "the bleeding has stopped" to somebody who
                // bled to death. Cleared here, the whole body that arrives
                // is no change at all.
                body.injuries = primitive_shared::injury::Injuries::default();
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

            // Whether this player has the give page at all. Asked every
            // time the journal opens -- see `ask_if_operator`.
            ServerMessage::Operator { yes } => journal.set_operator(yes),

            // On screen as well as in the log: an error the player
            // caused (nothing in hand, a recipe they cannot make) is a
            // reply to what they just did, and stderr is not where they
            // are looking.
            ServerMessage::Error(e) => {
                eprintln!("server error: {e}");
                // **On the HUD and not in the chat.** Every refusal used to
                // be written into the log as well, so an afternoon of
                // building left a column of "that does not go there" and
                // "you need a better tool for that" over whatever the other
                // players had said -- "удали эти тупые уведомления в чате о
                // том что я что то не так сделал". A refusal is about the
                // gesture just made, and the banner in front of it is where
                // that is read; a typed command's answer is a chat line from
                // the server (`run_command`) and is not this message.
                // ...and a refused run clears the bar, whatever the refusal
                // was. Every station refusal ends the run on the server
                // (`station_run`), so a client left drawing a marker would be
                // drawing a run nobody is judging any more. The words are
                // already in the notice above, which is where every other
                // refusal in the game lands.
                station_screen.run_refused();
                *notice = Some((e, Instant::now()));
            }

            ServerMessage::BodyWorn { x, y, z, worn } => {
                // What a dead player's body has on, for the figure the frame
                // draws it as. Kept whether or not the chunk has been
                // integrated yet -- see `ChunkManager::body_worn`.
                chunks.note_body_worn((x, y, z), worn);
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
    /// Packed, like everything the client keeps: a burst of streaming
    /// can leave hundreds waiting here, and at 131 KB a flat chunk that
    /// was tens of megabytes of queue.
    queue: VecDeque<primitive_shared::packed::PackedChunk>,
    /// How many queued chunks sit at each position -- a count rather
    /// than a set because a re-requested chunk can be queued twice.
    index: HashMap<ChunkPos, u32>,
}

impl Arrivals {
    fn push(&mut self, chunk: primitive_shared::packed::PackedChunk) {
        *self.index.entry(chunk.pos).or_insert(0) += 1;
        self.queue.push_back(chunk);
    }

    fn pop(&mut self) -> Option<primitive_shared::packed::PackedChunk> {
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
    fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut primitive_shared::packed::PackedChunk> {
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
/// A handful of chips where a block used to be.
///
/// **Read before the change is applied**, because what the burst is made
/// of is the block that *was* there -- afterwards the cell holds air and
/// there is nothing to take a texture from. Only for a cell that
/// actually emptied: a placement is a block arriving, and blocks do not
/// arrive in a shower of themselves.
fn burst_for(
    particles: &mut engine::particles::Particles,
    chunks: &ChunkManager,
    change: &primitive_shared::protocol::BlockChange,
) {
    if !primitive_shared::types::is_air(change.block_id) {
        return;
    }
    let Some(was) = chunks.block_at(change.global_x, change.global_y, change.global_z) else {
        return;
    };
    if primitive_shared::types::is_air(was) || primitive_shared::types::is_liquid(was) {
        return;
    }
    let cell = (change.global_x, change.global_y, change.global_z);
    // **A thing set down throws chips of the thing**, not of its cell's id.
    // `BLOCK_SET_DOWN` has no picture of its own -- the item is drawn from
    // what the server said lies there -- so its chips were the missing
    // texture's magenta and black.
    let was = if primitive_shared::types::is_set_down(was) {
        match chunks.set_down_items().find(|&(at, _, _)| at == cell) {
            Some((_, _, item)) => item,
            None => return,
        }
    } else {
        was
    };
    particles.block_broken(cell, was);
}

/// The noise a block update makes, if it makes one.
///
/// The twin of [`burst_for`], and it reads the world the same way: the
/// chunk still holds the *old* block when this runs, so a break knows
/// what it broke. `apply_change` is what overwrites it, immediately
/// afterwards.
///
/// Three updates in four make no sound at all, and the filtering is the
/// substance of this function rather than an optimisation:
///
/// * **Water finding its level** sends an update per cell per tick. A
///   stream would be a machine gun of bloops.
/// * **A block set to what it already was** happens whenever the server
///   re-affirms a cell, and is not an event.
/// * **A fire catching** is a block update whose new value is a lit
///   hearth, and it is not the sound of something being put down.
fn sound_for(
    audio: &Audio,
    soundscape: &mut Soundscape,
    chunks: &ChunkManager,
    change: &primitive_shared::protocol::BlockChange,
) {
    use primitive_shared::types as t;

    let cell = (change.global_x, change.global_y, change.global_z);
    let Some(was) = chunks.block_at(cell.0, cell.1, cell.2) else {
        // Out past what this client has loaded. Nothing to hear and
        // nothing to know about what used to be there.
        return;
    };
    if was == change.block_id {
        return;
    }

    if t::is_air(change.block_id) {
        // Water draining away, and air replacing air.
        if t::is_air(was) || t::is_liquid(was) {
            return;
        }
        soundscape.on_block_broken(audio, cell, was);
        return;
    }

    if t::is_liquid(change.block_id) {
        return;
    }
    // A door swung by somebody else: the lid of a chest is the same knock of
    // wood, opening and shutting. Once for the two halves, at the lower one.
    // The swinger's own client heard it when it swung (`door_swing`), and
    // hears nothing here: the server's word arrives as what it already shows.
    if t::is_door(was) && t::block_kind(was) == t::block_kind(change.block_id) {
        if t::block_kind(was) == t::BLOCK_DOOR {
            let sfx = if t::door_is_open(change.block_id) { audio::Sfx::ChestOpen } else { audio::Sfx::ChestClose };
            audio.play_at_block(sfx, cell, 0.9, 1.0);
        }
        return;
    }
    if t::is_burning(change.block_id) && !t::is_burning(was) {
        audio.play_at_block(audio::Sfx::Ignite, cell, 0.9, 1.0);
        return;
    }
    // **A board that burned through is not a board somebody set down.**
    // A burning log or a burning plank ends as its charred self, and that
    // update used to go down the `on_block_placed` path -- so a house
    // burning down sounded like somebody quietly building it, one plank a
    // second. What it is is timber giving way: the crack and the rumble,
    // and no blow in front of it, because nobody swung anything.
    if primitive_shared::wildfire::charred(was) == Some(change.block_id) {
        let middle =
            glam::DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        soundscape.on_gave_way(audio, middle, was);
        return;
    }
    // **A quarter off a block being dug is not a block set down.** It
    // arrives as a change from one rock to less of the same rock, and it
    // went down the path below -- so every slice knocked like a stone laid
    // on a wall. See `dig::took_a_slice`.
    if primitive_shared::dig::took_a_slice(was, change.block_id) {
        soundscape.on_slice_taken(audio, cell, was);
        return;
    }
    soundscape.on_block_placed(audio, cell, change.block_id);
}

/// What swinging the door at `at` changes: that half, and its partner if
/// its partner is standing there -- the server's `swing_door`, asked of the
/// world this client can see, so the prediction is the same two cells the
/// server will write. Never empty: the clicked half comes first.
fn door_swing(
    chunks: &ChunkManager,
    at: (i32, i32, i32),
    block: BlockId,
) -> Vec<primitive_shared::protocol::BlockChange> {
    use primitive_shared::types::{door_partner, door_swung};
    let change = |(x, y, z): (i32, i32, i32), block_id| primitive_shared::protocol::BlockChange {
        global_x: x,
        global_y: y,
        global_z: z,
        block_id,
    };
    let mut changes = vec![change(at, door_swung(block))];
    if let Some((other, expected)) = door_partner(at, block) {
        if chunks.block_at(other.0, other.1, other.2) == Some(expected) {
            changes.push(change(other, door_swung(expected)));
        }
    }
    changes
}

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

/// What becomes of a mesh a worker hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    /// Built from the chunk as it still is: onto the screen.
    Upload,
    /// Built from an older copy of a chunk that is still loaded: thrown
    /// away, and the chunk queued again.
    Stale,
    /// Built for a chunk that has since been unloaded: thrown away, and
    /// nothing queued.
    Evicted,
}

/// Decides what a finished mesh is, before anything is uploaded.
///
/// **A chunk unloaded while its mesh was being built used to come back
/// onto the screen and stay there for good.** The version is the only
/// thing this used to ask, and unloading a chunk does not change its
/// version -- so a job still on a worker when `request_and_unload` let the
/// chunk go landed with a matching number and went into the renderer's
/// map. Nothing ever took it out again: the unload had already happened,
/// and it only ever visits chunks the manager still holds. What stood
/// there was a piece of the world past the edge of the streamed disc,
/// with no chunk behind it and nothing to replace it until the player came
/// back that way. The fog culls everything past its end, so with the fog
/// on it could not be seen; with the fog off it was exactly what the
/// report photographed: things standing in the sky beyond where the world
/// ends.
///
/// Asked here rather than prevented at the unload, and that was weighed:
///
/// * **Bumping the version on unload** would make the late mesh `Stale`
///   and throw it away too. It also queues the position again for a chunk
///   nobody holds -- which `dispatch_meshing` skips -- and when the chunk
///   comes back before its light has, the queued job meshes it lit as open
///   sky. Two paths through the stale arm for one fact it was not written
///   about.
/// * **Not drawing meshes past the disc in `render`** hides the picture
///   and keeps the leak: every one of them holds its space in the arena
///   for the rest of the session, and a trail of them grows behind a
///   player on a raft.
///
/// Evicted before stale: a chunk that is gone is gone whatever its number
/// says.
fn landing(chunks: &ChunkManager, versions: &HashMap<ChunkPos, u64>, pos: ChunkPos, version: u64) -> Landing {
    if !chunks.is_loaded(pos) {
        return Landing::Evicted;
    }
    if versions.get(&pos).copied().unwrap_or(0) != version {
        return Landing::Stale;
    }
    Landing::Upload
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

/// What the mesh of a chunk on screen was built at, and the skyline that
/// decided it -- a mountain's bands start nearer (`lod::band_start`), so
/// the level alone is not enough to say whether it still holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Detail {
    level: u8,
    skyline: i32,
    /// Whether its stones were given their thickness (`lod::relief_at`).
    /// A line of its own rather than a level: it does not coarsen anything,
    /// and it is much nearer than the first band.
    relief: bool,
    /// Whether its canopy was built see-through, with the insides of its
    /// crowns (`lod::leaves_see_through_at`). Another line of its own, for
    /// `relief`'s reason, and recorded for coarse chunks too so a chunk
    /// coming back to full detail is judged against what it last was.
    see_through: bool,
}

/// How far a chunk is from the player's, in chunks.
///
/// One definition, used by the scan below and by the dispatch that
/// submits the job: two expressions of "how far away is it" that drifted
/// apart would show as a ring of chunks rebuilding every frame, each
/// side insisting the other had the level wrong.
fn chunk_distance(pos: ChunkPos, player_chunk: ChunkPos) -> f32 {
    let dx = (pos.x - player_chunk.x) as f32;
    let dz = (pos.z - player_chunk.z) as f32;
    (dx * dx + dz * dz).sqrt()
}

/// Re-levels the world around a player who has moved, queueing whatever
/// now wants a different mesh.
///
/// **Through the ordinary dirty queue, deliberately.** A ring of chunks
/// crossing a threshold together is exactly the kind of burst
/// `mesh_budget_ms` exists to spread out, and giving detail levels a
/// path of their own would be a second budget to get wrong. What the
/// player sees is the horizon sharpening over a second or so as they
/// walk toward it, which is what it should look like.
///
/// The version bump is what makes this safe against a player who turns
/// around: a job for the level we no longer want is already in flight,
/// and this is the same machinery that throws away a mesh overtaken by
/// an edit. Without it, the coarse mesh could land *after* the fine one
/// and the chunk would stay blurred until something else disturbed it.
// Eight, and the eighth is the flag below. Gathering them into a struct
// would be a struct with one caller, built at the call site out of the
// same eight values -- see `dispatch_meshing` beside it, which carries
// thirteen for the same reason: these are the things the function needs
// and every one of them is decided somewhere else.
#[allow(clippy::too_many_arguments)]
fn restripe_detail_levels(
    // Whether the *quality* changed rather than the player moving. A
    // quality change moves no chunk across a threshold and yet changes
    // what every coarse chunk is made of, so it is the one case where a
    // chunk whose level is unchanged still has to be rebuilt.
    quality_changed: bool,
    chunks: &ChunkManager,
    player_chunk: ChunkPos,
    lod_chunks: i32,
    // The two lines the player moves: `ClientSettings::relief_chunks` and
    // `ClientSettings::transparent_leaves_chunks`. A changed setting needs
    // no flag of its own, unlike the quality: it moves chunks across its
    // line, and the comparison below finds exactly those.
    relief_chunks: i32,
    leaf_chunks: i32,
    chunk_lod: &mut HashMap<ChunkPos, Detail>,
    versions: &mut HashMap<ChunkPos, u64>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
) {
    // A chunk that has fallen out of range is not a level any more, and
    // one that comes back has to be able to arrive without inheriting
    // the level it had two hundred blocks ago.
    chunk_lod.retain(|pos, _| chunks.is_loaded(*pos));

    let mut changed: Vec<(ChunkPos, f32)> = Vec::new();
    for (&pos, built) in chunk_lod.iter() {
        let distance = chunk_distance(pos, player_chunk);
        // **The skyline the chunk was last meshed with, not the one it
        // has now.** Asking the chunk means walking its top section, for
        // every loaded chunk, on every step across a boundary -- on the
        // main thread. And nothing is lost: an edit that moves a skyline
        // queues a remesh, and the dispatch records the new one.
        let start = crate::engine::lod::band_start(lod_chunks, built.skyline);
        let moved = crate::engine::lod::level_at(distance, start, built.level) != built.level;
        // ...and the nearer line where stones lose their thickness
        // (`lod::RELIEF_CHUNKS`), which moves no level and still changes
        // the mesh.
        let relief_moved = crate::engine::lod::relief_at(distance, relief_chunks, built.relief) != built.relief;
        // ...and the see-through canopy's line. **Only for a chunk at full
        // detail**: a coarse chunk's crowns are shells whichever side of it
        // the chunk is, and rebuilding one for a flag its mesh ignores is a
        // remesh for nothing. The dispatch still records the answer, so the
        // chunk is judged correctly the moment it comes back to level 0.
        let leaves_moved = built.level == 0
            && crate::engine::lod::leaves_see_through_at(distance, leaf_chunks, built.see_through) != built.see_through;
        // **A coarse chunk is rebuilt when the quality changes even
        // though its level did not.** The level says how blocky the
        // ground is; the quality says what else that chunk gives up --
        // its real lighting, its grass -- and neither is visible in the
        // number this loop compares. A fine chunk is not touched: there
        // is nothing about it that the quality has an opinion on.
        if moved || relief_moved || leaves_moved || (quality_changed && built.level > 0) {
            changed.push((pos, distance));
        }
    }
    // Nearest first. The chunks that just came *into* the fine band are
    // the near ones and the ones the player is walking at; the ones
    // dropping to coarse are behind them and can wait. The queue is a
    // plain FIFO, so this order is the only say we get.
    changed.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    for (pos, _) in changed {
        bump_version(versions, pos);
        mark_dirty(dirty, dirty_set, pos);
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
    player_chunk: ChunkPos,
    lod_chunks: i32,
    // How much a coarse chunk may give up. Passed rather than read from
    // the settings here, for the reason every other number in this
    // signature is: the tests that call this have no settings file.
    lod_quality: crate::engine::lod::Quality,
    // The two lines of `restripe_detail_levels`, passed for the same reason.
    relief_chunks: i32,
    leaf_chunks: i32,
    chunk_lod: &mut HashMap<ChunkPos, Detail>,
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
        // **Decided here rather than looked up**, so that a chunk
        // arriving on the horizon is built coarse the first time
        // instead of being built fine and then rebuilt. The level it is
        // currently drawn at is what makes the threshold sticky; a
        // chunk nobody has meshed yet has none, and full detail is the
        // right assumption for the one case that matters -- the ground
        // the player is standing on. See `engine::lod::level_at`.
        //
        // After the fill, because the fill is what finds the skyline, and
        // a mountain's bands start nearer than a meadow's
        // (`lod::band_start`).
        let mut cache = mesher.take_cache();
        cache.fill(pos, chunks, light);
        // What is in the pit kilns here, which the blocks do not say.
        cache.set_pottery(chunks.pit_pottery_in(pos));
        // ...and which chests here have their lid in the frame's hands
        // rather than in this mesh (`ServerMessage::ChestLid`).
        cache.set_swung_lids(chunks.lids_in(pos));
        let skyline = cache.ceiling();
        let level = crate::engine::lod::level_at(
            chunk_distance(pos, player_chunk),
            crate::engine::lod::band_start(lod_chunks, skyline),
            chunk_lod.get(&pos).map_or(0, |built| built.level),
        );
        // Near enough for a stone's thickness to be a pixel, or laid flat. A
        // chunk nobody has meshed yet counts as having it, for the reason a
        // level starts at full detail.
        let relief = crate::engine::lod::relief_at(
            chunk_distance(pos, player_chunk),
            relief_chunks,
            chunk_lod.get(&pos).is_none_or(|built| built.relief),
        );
        cache.lay_stones_flat(!relief);
        // See-through near, a shell past the player's line; a chunk nobody
        // has meshed counts as see-through, for the stones' reason.
        let see_through = crate::engine::lod::leaves_see_through_at(
            chunk_distance(pos, player_chunk),
            leaf_chunks,
            chunk_lod.get(&pos).is_none_or(|built| built.see_through),
        );
        cache.draw_leaves_solid(!see_through);
        chunk_lod.insert(pos, Detail { level, skyline, relief, see_through });
        mesher.submit(pos, version, level, lod_quality, cache);
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
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    urgent: &mut VecDeque<ChunkPos>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    versions: &HashMap<ChunkPos, u64>,
    player_chunk: ChunkPos,
    budget_ms: f32,
    debug_stats: &mut DebugStats,
) {
    let started = Instant::now();
    let budget = Duration::from_secs_f32(budget_ms / 1000.0);

    // The channel is emptied whatever happens -- see `Mesher::drain`.
    // What is rationed below is *landing* the results, not receiving
    // them.
    mesher.drain(player_chunk);
    // Chunks whose lighting just landed, plus their neighbours. Each is
    // queued for meshing only once its own neighbourhood has settled --
    // see `ChunkManager::neighbourhood_settled`.
    let mut newly_lit: Vec<ChunkPos> = Vec::new();

    while let Some(finished) = mesher.take_pending() {
        match finished {
            mesher::Finished::Mesh {
                pos,
                version,
                buffers,
                cache,
            } => {
                match landing(chunks, versions, pos, version) {
                    Landing::Upload => {}
                    // Drop a mesh the world has moved past, and queue the
                    // chunk again so it gets a fresh one. Without this a
                    // slower worker's stale result can overwrite a newer
                    // mesh and leave holes where a block was just broken.
                    Landing::Stale => {
                        debug_stats.stale_meshes_discarded += 1;
                        mark_urgent(urgent, dirty_set, pos);
                        mesher.recycle(cache, buffers);
                        continue;
                    }
                    // Nothing to queue: the chunk is gone, and if it comes
                    // back its light landing queues it like any arrival.
                    Landing::Evicted => {
                        debug_stats.stale_meshes_discarded += 1;
                        mesher.recycle(cache, buffers);
                        continue;
                    }
                }

                if buffers.indices.is_empty() {
                    // An all-air chunk still has to drop any stale mesh,
                    // or the blocks you just mined stay on screen.
                    graphics.drop_chunk_mesh(pos);
                } else {
                    graphics.set_chunk_mesh(pos, &buffers);
                }
                // The lids this mesh leaves out change hands now, on the
                // frame it goes on screen. See `ChunkManager::note_meshed_lids`.
                chunks.note_meshed_lids(pos, cache.swung_lids());
                mesher.recycle(cache, buffers);
            }

            mesher::Finished::Light { pos, data } => {
                // The worker did the isolated pass; the seam
                // reconciliation touches the shared map and stays here.
                if chunks.is_loaded(pos) {
                    for changed in light.insert_packed(chunks, pos, *data) {
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

        // After at least one, never before: a frame that landed nothing
        // is a frame the queue grew in, and a queue that only grows is
        // terrain that never arrives. The check is at the bottom of the
        // loop for exactly that reason.
        if started.elapsed() >= budget {
            break;
        }
    }

    queue_settled(newly_lit, chunks, light, dirty, dirty_set, player_chunk);

    debug_stats.upload_time_ms_this_second += started.elapsed().as_secs_f32() * 1000.0;
}

/// Queues meshing only for chunks whose neighbourhood has settled.
/// Meshing a chunk before its neighbours arrive means meshing it again
/// for each one -- which is what made the frame rate sag while terrain
/// streamed in. "Settled" means lit as well as loaded: see
/// [`neighbourhood_lit`].
fn queue_settled(
    newly_lit: Vec<ChunkPos>,
    chunks: &ChunkManager,
    light: &LightMap,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    player_chunk: ChunkPos,
) {
    for pos in newly_lit {
        if chunks.is_loaded(pos) && light.is_lit(pos) && neighbourhood_lit(chunks, light, pos, player_chunk) {
            mark_dirty(dirty, dirty_set, pos);
        }
    }
}

/// Whether a chunk can be meshed once and for all: every neighbour inside
/// the streamed area is loaded **and lit**.
///
/// **`ChunkManager::neighbourhood_settled` asks only for loaded**, and
/// loading and lighting are two moments now: a chunk goes into the world
/// the frame it arrives (`integrate_chunks`), and its light comes back from
/// a worker some frames later, in whatever order the workers finish. A
/// neighbour that is loaded but not yet lit is copied into the fill as full
/// sky (`Neighbourhood::fill`), so a chunk meshed in that gap was built with
/// a seam lit as open sky, certain to be wrong -- and each neighbour's light
/// landing queued it again, because the Light arm pushes all eight
/// neighbours. Up to eight mesh jobs for one chunk, of which only the last
/// was ever kept. `a_streamed_chunk_is_meshed_once_rather_than_once_per_neighbour_lit_after_it`
/// counts them.
///
/// Waiting costs the player nothing they could see: the mesh that survives
/// is the one that would have been built anyway, built on the frame the
/// last neighbour lights instead of being built three times on the way.
///
/// Rejected: keeping the early mesh and refusing the later ones. That
/// leaves the seam lit as sky on screen for good, which is the one thing the
/// later mesh was for.
fn neighbourhood_lit(chunks: &ChunkManager, light: &LightMap, pos: ChunkPos, player_chunk: ChunkPos) -> bool {
    chunks.neighbourhood_settled(pos, player_chunk)
        && NEIGHBOUR_OFFSETS.iter().all(|&(dx, dz)| {
            let neighbour = ChunkPos::new(pos.x + dx, pos.z + dz);
            !chunks.is_loaded(neighbour) || light.is_lit(neighbour)
        })
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
    // Told about every chunk that goes in, so it can be surveyed on the
    // map's own budget -- see `logic::map::ExploredMap::catch_up`.
    explored: &mut logic::map::ExploredMap,
) {
    let started = Instant::now();
    let budget = Duration::from_secs_f32(budget_ms / 1000.0);

    while let Some(chunk) = arrivals.pop() {
        let pos = chunk.pos;
        explored.note_chunk(pos);

        // Copy the blocks straight out of the arriving chunk, *before*
        // handing it to the world.
        //
        // The obvious-looking alternative -- read them back out of the
        // ChunkManager -- costs a hash lookup per cell, 16,384 of them
        // per chunk. That measured at ~19 ms per chunk and was the whole
        // of the remaining frame-rate sag while terrain streamed in.
        // The chunk goes in and comes back shared, so the worker gets
        // the same packed blocks rather than a copy of them; it decodes
        // them itself, off this thread.
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
    fn a_bandage_going_on_changes_the_key() {
        // The figure in an open pack is drawn from the wounds and nothing
        // else; a key blind to them would leave an arm red on screen after
        // the bandage the player just watched go on.
        use primitive_shared::injury::{Injuries, Kind, Part};
        let mut cut = Injuries::default();
        cut.inflict(Part::LeftArm, Kind::Cut, 0.6);
        let mut dressed = cut;
        dressed
            .treat(Part::LeftArm, primitive_shared::types::BLOCK_BANDAGE)
            .expect("a cut takes a bandage");
        let key = |injuries: &Injuries| UiKey {
            wounds: wounds_fingerprint(injuries),
            ..game_key()
        };
        assert!(key(&cut) != key(&dressed), "a bandage going on was invisible to the key");
        assert!(key(&cut) != key(&Injuries::default()), "a cut was invisible to the key");
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

    /// One chunk's light landing, as the Light arm of
    /// `collect_worker_results` lands it, and every mesh job that queues.
    ///
    /// `old_rule` queues by `neighbourhood_settled` alone, which is the rule
    /// `neighbourhood_lit` replaced, written out here as a record of it;
    /// the new rule goes through `queue_settled`, the function the frame
    /// loop calls. The workers are taken to mesh everything queued at once,
    /// so each entry popped is one mesh job.
    #[allow(clippy::too_many_arguments)]
    fn land_light(
        pos: ChunkPos,
        old_rule: bool,
        centre: ChunkPos,
        chunks: &ChunkManager,
        light: &mut LightMap,
        dirty: &mut VecDeque<ChunkPos>,
        dirty_set: &mut MeshQueueSet,
        jobs: &mut HashMap<ChunkPos, usize>,
    ) {
        let mut newly_lit: Vec<ChunkPos> = light.load_chunk(chunks, pos).into_iter().collect();
        newly_lit.push(pos);
        for (dx, dz) in NEIGHBOUR_OFFSETS {
            newly_lit.push(ChunkPos::new(pos.x + dx, pos.z + dz));
        }
        if old_rule {
            for p in newly_lit {
                if chunks.is_loaded(p) && light.is_lit(p) && chunks.neighbourhood_settled(p, centre) {
                    mark_dirty(dirty, dirty_set, p);
                }
            }
        } else {
            queue_settled(newly_lit, chunks, light, dirty, dirty_set, centre);
        }
        while let Some(p) = dirty.pop_front() {
            if dirty_set.remove(&p).is_some() {
                *jobs.entry(p).or_default() += 1;
            }
        }
    }

    /// **A streamed chunk is meshed once, not once per neighbour that lights
    /// after it.**
    ///
    /// Streaming played out on a disc of chunks: they arrive nearest first,
    /// each is loaded the moment it arrives, and the light workers hand
    /// their results back a few arrivals late and not in order. What is
    /// counted is mesh jobs -- a count, so it says the same thing on a
    /// machine that is compiling something else.
    #[test]
    fn a_streamed_chunk_is_meshed_once_rather_than_once_per_neighbour_lit_after_it() {
        let radius = 5;
        let centre = ChunkPos::new(0, 0);
        let probe = ChunkManager::new(radius);
        let mut arrivals: Vec<ChunkPos> = (-radius..=radius)
            .flat_map(|dz| (-radius..=radius).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| probe.inside(dx, dz))
            .map(|(dx, dz)| ChunkPos::new(dx, dz))
            .collect();
        arrivals.sort_by(|a, b| chunk_distance(*a, centre).total_cmp(&chunk_distance(*b, centre)));

        let stream = |old_rule: bool| -> HashMap<ChunkPos, usize> {
            let mut chunks = ChunkManager::new(radius);
            let mut light = LightMap::new();
            let (mut dirty, mut dirty_set, mut jobs) = (VecDeque::new(), MeshQueueSet::new(), HashMap::new());
            let mut in_flight: Vec<ChunkPos> = Vec::new();
            let mut seed = 0x2545_F491u32;
            for &pos in &arrivals {
                chunks.insert(Chunk { pos, blocks: vec![BLOCK_AIR; CHUNK_VOLUME] });
                in_flight.push(pos);
                // Three jobs behind, and whichever of the oldest three a
                // worker happens to finish first.
                if in_flight.len() > 3 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let done = in_flight.remove(seed as usize % 3);
                    land_light(done, old_rule, centre, &chunks, &mut light, &mut dirty, &mut dirty_set, &mut jobs);
                }
            }
            for done in in_flight {
                land_light(done, old_rule, centre, &chunks, &mut light, &mut dirty, &mut dirty_set, &mut jobs);
            }
            jobs
        };

        let before: usize = stream(true).values().sum();
        let after = stream(false);
        let total: usize = after.values().sum();
        println!("[stream] {} chunks: {before} mesh jobs by neighbours loaded, {total} by neighbours lit", arrivals.len());
        assert_eq!(after.len(), arrivals.len(), "a chunk was never meshed at all");
        assert!(
            after.values().all(|&n| n == 1),
            "a chunk was meshed more than once with every neighbour lit: {:?}",
            after.iter().filter(|(_, &n)| n > 1).collect::<Vec<_>>()
        );
        assert!(before > total, "the old rule wasted nothing on this stream, so the test proves nothing");
    }

    /// A square of loaded chunks around the origin, each already meshed
    /// at whatever level it deserves from where the player starts.
    fn a_settled_world(
        radius: i32,
        player: ChunkPos,
        lod: i32,
    ) -> (ChunkManager, HashMap<ChunkPos, Detail>) {
        let mut chunks = ChunkManager::new(radius);
        let mut chunk_lod = HashMap::new();
        for x in -radius..=radius {
            for z in -radius..=radius {
                let pos = ChunkPos::new(x, z);
                chunks.insert(Chunk {
                    pos,
                    blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
                });
                chunk_lod.insert(
                    pos,
                    Detail {
                        level: crate::engine::lod::level_at(chunk_distance(pos, player), lod, 0),
                        skyline: 0,
                        relief: crate::engine::lod::relief_at(chunk_distance(pos, player), crate::engine::lod::RELIEF_CHUNKS, true),
                        see_through: true,
                    },
                );
            }
        }
        (chunks, chunk_lod)
    }

    #[test]
    fn walking_a_chunk_re_meshes_only_what_crossed_a_detail_threshold() {
        // **The cost of the feature while the player is moving**, and
        // the answer has to be "a ring, not the world". Every chunk on
        // screen is a candidate for a different level every time the
        // player crosses a chunk boundary; queueing all of them would be
        // eighteen hundred re-meshes for a step, which no budget can
        // hide.
        let lod = 10;
        let start = ChunkPos::new(0, 0);
        let (chunks, mut chunk_lod) = a_settled_world(14, start, lod);

        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        let mut versions = HashMap::new();
        restripe_detail_levels(
            false,
            &chunks,
            ChunkPos::new(1, 0),
            lod,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );

        assert!(
            !dirty.is_empty(),
            "a step of a whole chunk moved nothing across a threshold"
        );
        assert!(
            dirty.len() < chunks.loaded_count() / 8,
            "{} of {} chunks were re-meshed for one chunk of walking",
            dirty.len(),
            chunks.loaded_count()
        );
        // Nearest first: the chunks that just came *into* detail are the
        // ones the player is walking at.
        let distances: Vec<f32> = dirty
            .iter()
            .map(|pos| chunk_distance(*pos, ChunkPos::new(1, 0)))
            .collect();
        assert!(
            distances.windows(2).all(|w| w[0] <= w[1]),
            "the queue is not in nearest-first order: {distances:?}"
        );
        // And every one of them is stale as far as an in-flight job is
        // concerned, or a coarse mesh could land on top of a fine one.
        for pos in &dirty {
            assert_eq!(versions.get(pos), Some(&1), "{pos:?} kept its version");
        }
    }

    #[test]
    fn standing_still_on_a_threshold_re_meshes_nothing_at_all() {
        // **The dithering the hysteresis exists to stop, seen from the
        // outside.** A player standing where the first band begins moves
        // a few centimetres and the scan runs again; if it queued the
        // ring every time, the horizon would rebuild for ever and the
        // mesh queue would never empty.
        let lod = 10;
        let player = ChunkPos::new(0, 0);
        let (chunks, mut chunk_lod) = a_settled_world(12, player, lod);

        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        let mut versions = HashMap::new();
        // The same position, twice, and then one chunk back and forth
        // across the line -- which is what a player walking a boundary
        // actually does.
        for step in [player, player, ChunkPos::new(0, 0)] {
            restripe_detail_levels(
                false,
                &chunks,
                step,
                lod,
                crate::engine::lod::RELIEF_CHUNKS,
                crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
                &mut chunk_lod,
                &mut versions,
                &mut dirty,
                &mut dirty_set,
            );
        }
        assert!(
            dirty.is_empty(),
            "{} chunks queued for a player who has not moved",
            dirty.len()
        );
    }

    #[test]
    fn turning_the_setting_off_brings_the_whole_world_back_to_full_detail() {
        // The row is in the settings screen, so it can be changed with a
        // world on screen -- and what it has to do then is re-mesh
        // everything that was coarse, not wait for the player to walk
        // past it.
        let lod = 10;
        let player = ChunkPos::new(0, 0);
        let (chunks, mut chunk_lod) = a_settled_world(14, player, lod);
        let coarse = chunk_lod.values().filter(|built| built.level > 0).count();
        assert!(coarse > 100, "the fixture has no coarse chunks in it");

        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        let mut versions = HashMap::new();
        restripe_detail_levels(
            false,
            &chunks,
            player,
            0,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );
        assert_eq!(dirty.len(), coarse, "not every coarse chunk was queued");
    }

    #[test]
    fn a_chunk_that_falls_out_of_range_takes_its_detail_level_with_it() {
        // A level is a fact about a *loaded* chunk. Leaving stale
        // entries behind would be a map that grows for as long as the
        // session does, and worse: a chunk that came back would be
        // meshed at the level it had two hundred blocks ago.
        let lod = 10;
        let (mut chunks, mut chunk_lod) = a_settled_world(4, ChunkPos::new(0, 0), lod);
        let gone = ChunkPos::new(4, 4);
        chunks.unload(gone);

        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        let mut versions = HashMap::new();
        restripe_detail_levels(
            false,
            &chunks,
            ChunkPos::new(0, 0),
            lod,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );
        assert!(!chunk_lod.contains_key(&gone));
    }

    #[test]
    fn a_mountain_goes_coarse_nearer_than_the_meadow_beside_it() {
        // Two chunks seven out, both meshed fine. Then one of them turns
        // out to be a mountain -- as the dispatch would record it -- and
        // the scan, from the same spot, has to queue that one and only
        // that one. From the recorded skyline: the chunks here are all
        // air, so a scan that asked the chunk would see a meadow.
        let lod = 10;
        let player = ChunkPos::new(0, 0);
        let (chunks, mut chunk_lod) = a_settled_world(12, player, lod);
        let (meadow, mountain) = (ChunkPos::new(7, 0), ChunkPos::new(0, 7));
        assert_eq!(chunk_lod[&meadow].level, 0);
        assert_eq!(chunk_lod[&mountain].level, 0);
        chunk_lod.get_mut(&mountain).unwrap().skyline = crate::engine::lod::TALL_SKYLINE;

        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        let mut versions = HashMap::new();
        restripe_detail_levels(
            false,
            &chunks,
            player,
            lod,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );
        assert_eq!(
            dirty.iter().copied().collect::<Vec<_>>(),
            vec![mountain],
            "the scan did not single out the mountain"
        );
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
            ChunkPos::new(0, 0),
            // Detail levels off: these three are about the queue, and a
            // chunk arriving coarse would be a second thing under test.
            0,
            crate::engine::lod::Quality::Normal,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut HashMap::new(),
            8.0,
            &mut stats,
        );

        // Whatever else got dispatched, the edited chunk must be among
        // the first results rather than 60 chunks later.
        let mut seen = Vec::new();
        for _ in 0..300 {
            mesher.drain(edited);
            while let Some(finished) = mesher.take_pending() {
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

    /// Dispatches the one chunk of `world_with_one_chunk` at the given
    /// lines and waits for its mesh: what the renderer would be handed,
    /// and what the chunk was recorded as.
    fn mesh_one_chunk_at(relief_chunks: i32, leaf_chunks: i32) -> (crate::engine::mesh::MeshBuffers, Detail) {
        let pos = ChunkPos::new(0, 0);
        let (chunks, light) = world_with_one_chunk(pos);
        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        mark_dirty(&mut dirty, &mut dirty_set, pos);
        let mut mesher = mesher::Mesher::new(crate::engine::texture::FaceLayers::empty_for_test(), 1);
        let mut chunk_lod = HashMap::new();
        dispatch_meshing(
            &mut VecDeque::new(),
            &mut dirty,
            &mut dirty_set,
            &HashMap::new(),
            &mut mesher,
            &chunks,
            &light,
            pos,
            0,
            crate::engine::lod::Quality::Normal,
            relief_chunks,
            leaf_chunks,
            &mut chunk_lod,
            1000.0,
            &mut DebugStats::default(),
        );
        for _ in 0..600 {
            mesher.drain(pos);
            if let Some(mesher::Finished::Mesh { buffers, .. }) = mesher.take_pending() {
                return (*buffers, chunk_lod[&pos]);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the chunk was never meshed");
    }

    #[test]
    fn at_solid_everywhere_the_chunk_under_the_player_is_handed_over_with_solid_leaves() {
        // The setting's near end: nothing see-through, not even the
        // canopy over the player's head. What reaches the renderer is the
        // mesh's own flag, and it is what keeps its leaves out of the
        // cut-out pass (`renderer::cutout_range`).
        let (mesh, built) = mesh_one_chunk_at(crate::engine::lod::RELIEF_CHUNKS, 0);
        assert!(mesh.leaves_solid, "a chunk under the player was built see-through at solid everywhere");
        assert!(!built.see_through);
        // ...and the far end changes nothing about the chunk: see-through,
        // as every chunk was meshed before the setting.
        let (mesh, built) = mesh_one_chunk_at(
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
        );
        assert!(!mesh.leaves_solid && built.see_through);
    }

    #[test]
    fn a_relief_distance_of_zero_meshes_the_chunk_under_the_player_with_flat_stones() {
        // "0 = flat quads everywhere", asked of the dispatch rather than
        // of `lod::relief_at` alone: the chunk the player stands in is the
        // one a nearest-first rule would most want to give a thickness.
        let (_, built) = mesh_one_chunk_at(0, crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE);
        assert!(!built.relief, "the chunk under the player kept its stones' thickness at zero");
        let (_, built) = mesh_one_chunk_at(
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
        );
        assert!(built.relief);
    }

    #[test]
    fn moving_the_see_through_leaf_line_re_meshes_only_the_chunks_it_moved_past() {
        // A settled world, every chunk see-through as a player who had the
        // setting at "everywhere" would have it. Pulling the line in to six
        // queues exactly the chunks past it -- a chunk of hysteresis out,
        // since they were built see-through -- and leaves the rest alone;
        // putting it back at everywhere from there queues nothing further.
        let start = ChunkPos::new(0, 0);
        let (chunks, mut chunk_lod) = a_settled_world(10, start, 0);
        let mut versions = HashMap::new();
        let mut dirty = VecDeque::new();
        let mut dirty_set = MeshQueueSet::new();
        restripe_detail_levels(
            false,
            &chunks,
            start,
            0,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );
        assert!(dirty.is_empty(), "nothing moved and {} chunks were queued", dirty.len());

        restripe_detail_levels(
            false,
            &chunks,
            start,
            0,
            crate::engine::lod::RELIEF_CHUNKS,
            6,
            &mut chunk_lod,
            &mut versions,
            &mut dirty,
            &mut dirty_set,
        );
        assert!(!dirty.is_empty(), "pulling the line in queued nothing");
        for pos in &dirty {
            assert!(chunk_distance(*pos, start) >= 7.0, "{pos:?} is inside the line and was queued");
        }
        let past = chunk_lod.keys().filter(|pos| chunk_distance(**pos, start) >= 7.0).count();
        assert_eq!(dirty.len(), past, "some chunks past the line were not queued");
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
            ChunkPos::new(0, 0),
            // Detail levels off: these three are about the queue, and a
            // chunk arriving coarse would be a second thing under test.
            0,
            crate::engine::lod::Quality::Normal,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut HashMap::new(),
            8.0,
            &mut stats,
        );
        assert_eq!(mesher.in_flight(), 1, "duplicate queue entry caused extra work");
        assert!(dirty_set.is_empty());
    }

    #[test]
    fn a_mesh_that_comes_back_after_its_chunk_was_unloaded_never_reaches_the_screen() {
        // **The pieces of world standing in the sky past the edge.** A job
        // still on a worker when the player walked the chunk out of range
        // came back with the version it left with -- unloading changes no
        // version -- and went onto the screen, where nothing ever took it
        // off again. See `landing`.
        //
        // Played out with the real workers, in the frame's order: the
        // chunk is meshed, the unload happens while the job is out, and the
        // result is then asked about the way `collect_worker_results` asks.
        // The same fixture left loaded must upload, and with its version
        // bumped must be stale, or this is not reaching the arm at all.
        fn mesh_and_wait(unload_first: bool, bump: bool) -> Landing {
            let pos = ChunkPos::new(3, -2);
            let (mut chunks, mut light) = world_with_one_chunk(pos);
            let (mut urgent, mut dirty, mut dirty_set) = (VecDeque::new(), VecDeque::new(), MeshQueueSet::new());
            let mut versions = HashMap::new();
            mark_dirty(&mut dirty, &mut dirty_set, pos);
            let mut mesher = mesher::Mesher::new(crate::engine::texture::FaceLayers::empty_for_test(), 1);
            dispatch_meshing(
                &mut urgent,
                &mut dirty,
                &mut dirty_set,
                &versions,
                &mut mesher,
                &chunks,
                &light,
                pos,
                0,
                crate::engine::lod::Quality::Normal,
                crate::engine::lod::RELIEF_CHUNKS,
                crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
                &mut HashMap::new(),
                8.0,
                &mut DebugStats::default(),
            );
            assert_eq!(mesher.in_flight(), 1, "the fixture never sent a job");
            // What `request_and_unload` does to a chunk that left the disc.
            if unload_first {
                chunks.unload(pos);
                light.unload_chunk(pos);
            }
            if bump {
                bump_version(&mut versions, pos);
            }
            for _ in 0..400 {
                mesher.drain(pos);
                while let Some(finished) = mesher.take_pending() {
                    if let mesher::Finished::Mesh { pos: landed, version, .. } = finished {
                        assert_eq!(landed, pos);
                        return landing(&chunks, &versions, landed, version);
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("the worker never handed the mesh back");
        }

        assert_eq!(mesh_and_wait(false, false), Landing::Upload, "a loaded, current chunk was not uploaded");
        assert_eq!(mesh_and_wait(false, true), Landing::Stale, "an edited chunk's old mesh was not stale");
        assert_eq!(
            mesh_and_wait(true, false),
            Landing::Evicted,
            "a mesh for a chunk that was unloaded while it was built would have gone on screen"
        );
        // ...and gone is gone, whatever the number says.
        assert_eq!(mesh_and_wait(true, true), Landing::Evicted);
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
            ChunkPos::new(0, 0),
            // Detail levels off: these three are about the queue, and a
            // chunk arriving coarse would be a second thing under test.
            0,
            crate::engine::lod::Quality::Normal,
            crate::engine::lod::RELIEF_CHUNKS,
            crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
            &mut HashMap::new(),
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
/// Whether a finger on the glass belongs to the world or to a screen.
///
/// **Written once because it is asked twice**, and the two askings are
/// a frame apart: the touch handler routes each event by it, and the
/// frame body uses it to know that the world's buttons are all up. Two
/// copies of this drifted is a thumb whose press is delivered to the
/// world and whose lift is delivered to a menu -- which is exactly the
/// bug that left a key held down for ever. See the call in the frame
/// body.
/// Where the player is and which way they face, as the map wants it: on
/// the ground, with the camera's yaw.
fn player_mark(position: Vec3, yaw: f32) -> ui::map_screen::PlayerMark {
    ui::map_screen::PlayerMark {
        x: position.x,
        z: position.z,
        yaw,
    }
}

#[allow(clippy::fn_params_excessive_bools)]
fn world_owns_the_glass(
    in_a_world: bool,
    paused: bool,
    inventory_open: bool,
    chest_open: bool,
    dead: bool,
    typing: bool,
) -> bool {
    in_a_world && !paused && !inventory_open && !chest_open && !dead && !typing
}

/// A different number every time the menu builds itself a world.
///
/// **The wall clock, and it is the right source here.** Nothing about
/// the backdrop has to be reproducible between launches -- it is
/// scenery -- and the client carries no random number generator worth
/// the dependency for one number a session. What it does have to be is
/// *different*: a menu that opens on the same beach every time is a
/// picture, and the whole argument for generating this rather than
/// drawing it is that it is a different place each time.
///
/// The nanoseconds rather than the seconds, because two launches a
/// second apart would otherwise start on neighbouring seeds -- and the
/// search mixes its seed hard enough that neighbouring is not similar,
/// but a clock that only moves once a second is a clock that hands the
/// same number to a game restarted quickly twice.
fn menu_scene_roll() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos() ^ since.as_secs() as u32)
        .unwrap_or(0)
}

/// `scene` is which kind of place is standing behind this screen, if
/// one is. **The place and not merely a flag**, because the veil the
/// menu draws over it is sized to how bright that place measures, and a
/// cave and a sunset are not within an order of magnitude of each other
/// -- see `menu::veil_for`.
///
/// Passed in rather than derived from the settings, because the scene
/// is not up the instant the switch is: what the settings hold is a
/// *request*, and what is actually behind the menu is whatever the
/// background thread has built. It is also how the paused menu says
/// "nothing of mine is behind this" while the setting is still on.
fn menu_context<'a>(
    settings: &'a ClientSettings,
    worlds: &'a worlds::Worlds,
    graphics: &GraphicsState,
    scene: Option<menu_scene::Place>,
) -> menu::MenuContext<'a> {
    let background = match scene {
        Some(place) => menu::Backdrop::Scene(place),
        None => menu::Backdrop::Bare,
    };
    menu::MenuContext {
        version: VERSION,
        font: graphics.textures.font,
        settings,
        worlds,
        background,
        // What shape of screen this menu is being laid out on, and how
        // big the player asked for it. See `widgets::Layout`: the menu
        // is the one part of the interface that adapts rather than
        // being multiplied afterwards.
        layout: widgets::Layout::for_screen(graphics.aspect(), graphics.ui_scale()),
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
/// Render distance applies at once too, under the cap the server named in
/// `Welcome`. In singleplayer that cap is the row's own ceiling
/// (`ClientSettings::singleplayer_server`), so the row is the whole answer;
/// this note used to say the distance waited for the next world, which was
/// true only because the local server had a smaller number of its own.
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
    graphics.set_ui_scale(settings.ui_scale);
    // Before the shadows: a step change rebuilds a shadow map that is
    // already on, and a map being switched on at the same moment is then
    // built once, for the new step, rather than twice.
    graphics.set_lighting(settings.lighting);
    graphics.set_shadows(settings.shadows);
    graphics.set_shadow_distance(settings.shadow_distance);
    graphics.set_plant_shadows(settings.plant_shadows);
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

/// What a use-gesture claims, in the order the claims are honoured.
///
/// **Written out because the order is the whole of the design**, and it
/// used to live as three `filter`s reading down the frame loop where
/// nothing could check it. Every one of these is somebody's reasonable
/// expectation, and they collide:
///
/// * a **chest** takes the gesture first, whatever is in the hand. The
///   only way to open one otherwise would be to put down what you are
///   carrying, and it would land on the ground in front of the chest.
/// * a **hearth** takes it next, for the same reason, and with one
///   exception of its own: a hearth is a container, so a use opens it --
///   but *striking flint on it* is a gesture about the fire rather than
///   about what is inside, so holding the striker means lighting.
/// * a **carcass** takes it next, whatever is in the hand -- a knife,
///   an axe, or nothing. Which of those it was is the server's
///   decision (`animals::butcher`), and a bare hand is sent on purpose:
///   the server answers it with a hint, and a client that stayed silent
///   would leave the player clicking at a heap that ignores them. It
///   comes before food for the reason the chest does: a player at a
///   dead deer holding their lunch meant the deer.
/// * **apples on a tree** take it next, whatever is in the hand -- the
///   player asked for picking "через пкм … без ломания", and a player at
///   an apple tree holding bread or a stone meant the apples. Placing
///   against the tree is not lost: every leaf round the fruit is still a
///   face to build on.
/// * **food in hand** is eaten. This is the one that was missing, and
///   the player asked for it in as many words: "сделай возможность есть
///   взяв в руку, а не через HUD". It comes after the blocks on purpose
///   -- a player who walks to a chest holding dinner meant the chest.
/// * a **barrel** takes it for a jug in hand -- to pour or dip -- and for
///   an empty hand, to drink from; with anything else in hand it is a thing
///   to build against.
/// * a **jug in hand** opens, after every block above has had its say: a
///   jug at a river fills and at a barrel pours, which is what it was
///   carried there for. Setting one down moved to the modifier -- see
///   [`UseGesture::OpenVessel`].
/// * anything else **places**.
///
/// Eating costs the placement nothing: food is not a block, so
/// `try_place_block` already did nothing whatever with a loaf selected.
/// The gesture was simply idle in the hand that had food in it. Neither
/// does the carcass: a knife is not placeable, so before this branch a
/// right click on one with a knife in hand did nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UseGesture {
    /// Open the container being looked at.
    Open,
    /// Open the anvil or the potter's wheel being looked at: the mini-game
    /// screens (`ui::station_screen`).
    ///
    /// **Before placement, on the chest's terms**, and the argument is the
    /// chest's: the only other way to work at an anvil would be to empty your
    /// hand first -- and at the anvil the thing you are holding is the hammer
    /// it wants. It is asked *after* the container, the hearth and the door,
    /// because none of those is a station and a block that is both does not
    /// exist.
    Station,
    /// Build, light, empty or ask after a fire in the ground: a pit kiln
    /// or a log pile, or pottery into an empty pit, or flint at the makings
    /// of a firepit. Everything it does is the server's to decide
    /// (`logic::pits`), so it is a `UseBlock` like the hearth's.
    Pit,
    /// Strike or feed the hearth being looked at.
    Hearth,
    /// Take a cut off the carcass being looked at.
    Butcher,
    /// Work what is aimed at with what is in the hand, where the world
    /// changes and nothing is put down: a knife scoring a standing trunk
    /// for resin, ash dug into a furrow, resin wadded onto a burnt-out
    /// standing torch or a torch lit at one that burns. See `tends`.
    Tend,
    /// Pick the apples off the leaves being looked at, leaving the leaves.
    Pick,
    /// Eat what is in the hand.
    Eat,
    /// Drink from the water being looked at, or fill the vessel in hand
    /// from it.
    Water,
    /// Lie down on the bed, or sit on the stool, being looked at.
    ///
    /// **This gesture did not exist, and so neither did sleep.** The server
    /// has answered a `UseBlock` on a bed or a stool since furniture was
    /// added, and the client never sent one: a right click on a bed with an
    /// empty hand fell through to placing nothing, and with a block in hand
    /// it built on the bed. Before placement for the reason a chest is --
    /// the only other way to use a bed would be to empty your hand first.
    Rest,
    /// Look inside the jug in the hand.
    ///
    /// **It takes the placement's gesture, and the placement moves to
    /// the modifier.** A jug was placeable before it could be opened, so
    /// a right click with one in hand used to set it down -- and "open it
    /// by right-clicking while holding it" is the request this answers.
    /// Opening is what a player does to a jug many times an hour and
    /// setting one on a shelf is what they do once, so the plain click
    /// went to the frequent thing and shift-right-click (or a held tap on
    /// glass) sets it down. The rejected split was by *aim* -- open when
    /// looking at the sky, set down when looking at a block -- which is a
    /// rule a player would only ever discover by accident.
    OpenVessel,
    /// Swing the door being looked at, open or shut, whatever is in the
    /// hand -- and on this screen at once, before the server has said so.
    /// See `door_swing`.
    Swing,
    /// Put what is in the hand into the world.
    Place,
}

/// Is a use-gesture that would otherwise place something a fire in the
/// ground instead?
///
/// Two cases, and both need more of the world than the aimed block:
///
/// * **Pottery at the floor of an empty pit** -- a raw pot is not
///   placeable, so without this the click did nothing at all. The same
///   question the server asks (`pit::takes_pottery_above`).
/// * **Flint at a cell with sticks or a log lying on it** -- flint *is*
///   placeable, so the plain click sets a nodule down, and it must go on
///   doing that at bare ground. Only where the makings of a firepit lie is
///   the strike a strike; the server says what is missing if not all of it
///   is there.
fn ground_fire_claim(
    chunks: &ChunkManager,
    entities: &crate::logic::entities::Entities,
    aimed: Option<((i32, i32, i32), BlockId)>,
    held: Option<BlockId>,
) -> bool {
    use primitive_shared::types::{block_kind, BLOCK_FLINT, BLOCK_STICK};
    let (Some((cell, _)), Some(held)) = (aimed, held) else {
        return false;
    };
    if primitive_shared::pit::is_raw_pottery(held) {
        return primitive_shared::pit::takes_pottery_above(|x, y, z| chunks.block_at(x, y, z), cell);
    }
    block_kind(held) == BLOCK_FLINT
        && entities.count_lying_in((cell.0, cell.1 + 1, cell.2), |block| {
            block_kind(block) == BLOCK_STICK || primitive_shared::pit::is_log(block)
        }) > 0
}

/// Is `held` at `block` one of the gestures that work a thing where it
/// stands (`UseGesture::Tend`)? The server's `use_block` decides what each
/// does and refuses what it will not; this has to agree with it only about
/// which clicks are not placements.
///
/// **Every one of them was a placement before**, or nothing: ash is a
/// coating a player can lay, so ash at a furrow would have been laid *on*
/// the furrow; a knife is not placeable, so a knife at a trunk did nothing
/// at all; and resin and a torch at a standing torch went nowhere.
fn tends(block: BlockId, held: BlockId) -> bool {
    use primitive_shared::types::{
        block_axis, block_kind, is_knife, Axis, BLOCK_ASH, BLOCK_RESIN, BLOCK_STANDING_TORCH_LIT,
        BLOCK_STRIPPED_LOG, BLOCK_TORCH,
    };
    use primitive_shared::wildfire::{dressed, fuel, is_standing_torch, Fuel};
    let standing_trunk =
        fuel(block) == Some(Fuel::Log) && block_axis(block) == Axis::Y && block_kind(block) != BLOCK_STRIPPED_LOG;
    match block_kind(held) {
        _ if is_knife(held) => standing_trunk,
        BLOCK_ASH => dressed(block).is_some() || primitive_shared::wildfire::is_dressed(block),
        BLOCK_RESIN => is_standing_torch(block),
        BLOCK_TORCH => block_kind(block) == BLOCK_STANDING_TORCH_LIT,
        _ => false,
    }
}

/// The empty cell a thing would be set down in, from where the camera looks:
/// the one over the top of the block the placement ray stops at, if that
/// cell is air and the block is a whole floor. `None` for a side or an
/// underside, for water, and for a drift or a slab.
///
/// The server asks the same of its own world (`set_down_item`); this is only
/// so a gesture it would refuse is said in words instead of sent.
fn set_down_cell(chunks: &ChunkManager, camera: &Camera) -> Option<(i32, i32, i32)> {
    use primitive_shared::types::{has_full_top, is_air};
    let (hit, before) = physics::raycast_block(chunks, camera.position, camera.forward(), INTERACT_RANGE)?;
    let over_the_top = before == (hit.0, hit.1 + 1, hit.2);
    let empty = chunks.block_at(before.0, before.1, before.2).is_some_and(is_air);
    let floor = chunks.block_at(hit.0, hit.1, hit.2).is_some_and(has_full_top);
    (over_the_top && empty && floor).then_some(before)
}

/// Which of the five a use-gesture is, given what is aimed at and what
/// is held. See [`UseGesture`] for why the order is what it is.
fn use_gesture(aimed: Option<BlockId>, held: Option<BlockId>) -> UseGesture {
    use primitive_shared::types::{block_kind, is_carcass, is_container, is_hearth, BLOCK_FLINT};
    let striking = held.is_some_and(|held| block_kind(held) == BLOCK_FLINT);
    if let Some(block) = aimed {
        // **An unlit torch at any fire is lit from it**, before the pit kiln
        // takes it as fuel and the burning wall is built against: the
        // server's first gesture (`types::lights_a_torch`). A `UseBlock`,
        // the hearth's message, whatever the fire is.
        if held.is_some_and(|held| block_kind(held) == primitive_shared::types::BLOCK_TORCH)
            && primitive_shared::types::lights_a_torch(block)
        {
            return UseGesture::Hearth;
        }
        // A pit kiln or a log pile, with anything or nothing in hand:
        // pottery, fibre and logs go in, flint lights it, an empty hand takes
        // a pot back or asks how long is left. Before placement for the
        // chest's reason -- a log in hand at a kiln is a log for the kiln.
        if primitive_shared::pit::is_pit_kiln(block) || primitive_shared::pit::is_log_pile(block) {
            return UseGesture::Pit;
        }
        // **A thing set down is taken back, whatever is in the hand**, before
        // its store makes it a chest to open (`is_container`). The server's
        // `pick_up_set_down`.
        if primitive_shared::types::is_set_down(block) {
            return UseGesture::Pick;
        }
        // **A fish trap with fish in it is emptied, whatever is in the
        // hand** -- the server's `empty_trap`, and its argument: a player
        // back at the river is carrying the day. An empty one falls through
        // and is built against like any block, because the empty-handed
        // "nothing in it yet" is said before this is asked
        // (`fishing::trap_notice`).
        if block_kind(block) == primitive_shared::types::BLOCK_FISH_TRAP
            && primitive_shared::types::trap_catch(block) > 0
        {
            return UseGesture::Pick;
        }
        // **A torch held to a burning fire is about the fire, as flint
        // is.** The report: "the torch is impossible to light, e.g. from a
        // campfire". The server has lit a torch at a burning hearth since
        // torches existed (`use_block`), and it never heard the gesture: a
        // hearth is a container, so the click opened its fuel screen and
        // the only exception here was the striker. The rejected fix was to
        // let the fuel screen light what is dragged over it, which would
        // make lighting a torch a drag instead of the reach of the arm it
        // is.
        let lighting_a_torch = held.is_some_and(|held| block_kind(held) == primitive_shared::types::BLOCK_TORCH)
            && primitive_shared::types::is_burning(block);
        // **A knife at a dead player's body cuts it**, and anything else opens
        // it to take their things: the body is a container first, and a
        // knife is the one thing in the hand that says otherwise. See
        // `animals::cut_body`.
        if block_kind(block) == primitive_shared::types::BLOCK_CORPSE
            && held.is_some_and(primitive_shared::types::is_knife)
        {
            return UseGesture::Butcher;
        }
        // **A door swings, whatever is in the hand**, before placement for
        // the chest's reason: a player carrying boards home would otherwise
        // board their own door up.
        if primitive_shared::types::is_door(block) {
            return UseGesture::Swing;
        }
        if is_container(block)
            && !((striking || lighting_a_torch) && primitive_shared::hearth::Kind::of(block).is_some())
        {
            return UseGesture::Open;
        }
        // ...and the two stations with a screen behind them. A wheel is also a
        // workshop the crafting menu asks about by *proximity*, and that is
        // untouched: the menu row is still there, and this is the other way to
        // the same pots (see `minigame`).
        if matches!(
            block_kind(block),
            primitive_shared::types::BLOCK_ANVIL | primitive_shared::types::BLOCK_POTTERS_WHEEL
        ) {
            return UseGesture::Station;
        }
        if is_hearth(block) {
            return UseGesture::Hearth;
        }
        if is_carcass(block) {
            return UseGesture::Butcher;
        }
        if held.is_some_and(|held| tends(block, held)) {
            return UseGesture::Tend;
        }
        if primitive_shared::types::picks_by_hand(block) {
            return UseGesture::Pick;
        }
        // ...and moss on a stone or a trunk, with an empty hand: see the
        // server's `pick_by_hand`.
        if held.is_none() && primitive_shared::ground::is_mossy(block) {
            return UseGesture::Pick;
        }
        // ...and a palm's trunk with an empty hand, which is shaken for its
        // coconuts: the same reach into a tree, a trunk's height lower. The
        // server decides what comes down (`shake_palm`).
        if held.is_none() && block_kind(block) == primitive_shared::types::BLOCK_PALM_TRUNK {
            return UseGesture::Pick;
        }
        if primitive_shared::body::Rest::of(block).is_some() || primitive_shared::types::is_seat(block) {
            return UseGesture::Rest;
        }
        // **Water, and only for an empty hand or an empty vessel.**
        //
        // The ray has seen lakes since the drinking fix (`aimed_block`),
        // and nothing read the answer: every rule about drinking and
        // filling lived on the server behind a message the client never
        // sent, so a thirsty player clicking at a river still got
        // silence. This is the half that was missing.
        //
        // Narrow on purpose. A player holding stone at the edge of a
        // lake is building, not drinking -- the placement ray goes
        // through water to the bed under it and always did -- and a
        // player holding bread is eating. What is left is the two cases
        // where the water *is* the target: nothing in the hand, or a
        // vessel with room in it.
        // **A barrel, with a jug in hand**, full to pour or empty to dip --
        // **or with nothing in hand, to drink from.** The server decides
        // which (`use_barrel`) from what it believes is held; all this has
        // to know is that a jug at a barrel is not a jug being set down on
        // top of one, and that an empty hand at a barrel is the same
        // thirsty player it is at a pond. With anything else in hand the
        // barrel is a thing to build against, like a table.
        if primitive_shared::types::is_barrel(block)
            && held.is_none_or(primitive_shared::types::is_vessel)
        {
            return UseGesture::Water;
        }
        if primitive_shared::types::is_liquid(block)
            && held.is_none_or(|held| {
                primitive_shared::types::filled_vessel(held).is_some()
                    // ...or a raft, which a lake is where it is carried to.
                    // The server launches it (`rafts::launch`).
                    || block_kind(held) == primitive_shared::types::BLOCK_RAFT
                    // ...or a rod, which is cast (`fishing`).
                    || block_kind(held) == primitive_shared::types::BLOCK_FISHING_ROD
            })
        {
            return UseGesture::Water;
        }
    }
    // **A full jug is drunk from, and this is the gesture a player
    // reaches for**: "не получается пить из сосуда с водой". Nothing here
    // refused them -- the eat key was deaf to a jug (see
    // `goes_in_the_mouth`) and the right hand opened the vessel screen
    // instead, which for a jug of water lists what may be poured *in* and
    // is therefore the one screen with nothing in it to do.
    //
    // After every block has had its say, so the jug carried to a river is
    // still filled and the jug carried to a barrel is still poured: those
    // are what it was carried there for, and a drink is what is left when
    // the hand is pointed at nothing in particular.
    if held.is_some_and(goes_in_the_mouth) {
        return UseGesture::Eat;
    }
    // An empty jug in the hand opens. See `UseGesture::OpenVessel` for
    // where setting one down went.
    if held.is_some_and(primitive_shared::types::opens_as_vessel) {
        return UseGesture::OpenVessel;
    }
    UseGesture::Place
}

/// Whether a use-gesture at `aimed` with `held` in hand is about to be a
/// swallow the player should hear.
///
/// **A guess, made where the hand moves, and on purpose.** Whether a drink
/// happened is the server's answer, and it has no message that says so:
/// the pack or the body changes and nothing else does. Waiting for the
/// thirst bar to rise would put the sound a round trip behind the click,
/// and would miss the sea entirely, which takes water rather than giving
/// it. So the client asks the two things it does know -- is there water
/// in front of the hand, and is this body thirsty enough to take it (the
/// server's own sip of room at the top, `Vitals::drink`) -- and a wrong
/// guess costs one swallow with no drink, once, at the edge of full.
///
/// Only the bare hand: a jug at a river is filled and a jug at a barrel is
/// poured or dipped, and neither is a mouthful.
fn swallow_expected(aimed: Option<BlockId>, held: Option<BlockId>, hydration: f32) -> bool {
    if held.is_some() {
        return false;
    }
    let Some(block) = aimed else {
        return false;
    };
    let water = match primitive_shared::types::barrel_contents(block) {
        Some((kind, _)) => {
            if primitive_shared::types::barrel_after_drinking(block).is_none() {
                return false; // an empty barrel is a hand on dry staves
            }
            kind
        }
        None if primitive_shared::types::is_liquid(block) => {
            // Which water the client cannot tell -- the biome and the flow
            // are the server's to read -- so a pond counts as drinkable,
            // which it is.
            primitive_shared::body::Water::Fresh
        }
        None => return false,
    };
    // The sea is swallowed at any fullness; everything else needs room.
    water == primitive_shared::body::Water::Salt
        || hydration < 1.0 - 1.0 / primitive_shared::body::MAX_HYDRATION
}

/// Eats what is in a slot, if it is food.
///
/// **One copy, because there are two ways to ask now.** The key does it
/// and so does a finger resting on the hotbar (see
/// `hotbar::Touched::Eat`), and the rule about *which* slot and whether
/// it is edible is the kind of thing that goes quietly out of step when
/// it is written twice.
///
/// Sent only if it really is food, so asking on a stack of stone is
/// silence rather than a refusal a round trip later. The server checks
/// it again -- and checks that eating it would do anything at all,
/// which the client cannot, because how full the player is belongs to
/// the server.
/// Whether a bound key still does its job while the death screen is up.
///
/// **Three did not stop, and one of them locked the screen.** The keys
/// were matched with no thought for the death screen, so on it `I` opened
/// the pack behind the wash -- and the `I` or Escape that shut it again
/// grabbed the cursor, on the one screen whose only controls are buttons.
/// The player was left turning a dead man's head with no pointer to press
/// RESPAWN with, until they found that R still worked. Eating and throwing
/// went to the server from a body that was not there, and a throw played
/// the drop sound for something that never left the pack.
///
/// Everything else keeps working: Respawn is the point of the screen, and
/// the fog, the stats panel and fullscreen are about the window rather
/// than the body. Movement keys reach nothing -- the body is frozen -- but
/// they still have to be *tracked*, or a key held across the respawn
/// arrives as a release with no press.
/// Puts the give menu's command on the wire, if it has written one.
///
/// **The one place the menu reaches a server, and it is here rather than
/// in the menu.** `ui::give_screen` decides what to ask for and writes
/// down the line an operator would have typed; this sends it, through
/// exactly the door the chat box uses (`ClientMessage::Chat`). So the
/// interface still decides nothing about the world -- see the note at the
/// top of `ui/mod.rs` -- and the server cannot tell a tap in the menu from
/// a command someone typed, which is why no rule had to learn about this
/// screen at all.
/// Ask the server whether this player is an operator.
///
/// **Every time the journal opens, rather than once at the handshake.**
/// `/op` takes effect on the caller's next command and not on their next
/// login (see the server's `permission_of`), so a right settled at the
/// door would be wrong for the rest of the session -- a player given
/// operator by a friend would go on being told there is no such page.
/// One message of two bytes against a screen a player opens by hand is
/// not worth caching.
fn ask_if_operator(net: Option<&network::NetworkHandle>) {
    if let Some(net) = net {
        net.send(ClientMessage::AmIAnOperator);
    }
}

fn send_journal_command(
    journal: &mut ui::journal::Journal,
    net: Option<&network::NetworkHandle>,
    debug_stats: &mut DebugStats,
) {
    // The socket first: taking the line and then finding nowhere to send
    // it would leave the menu saying "asking the server" about a command
    // that never left. Unreachable while the journal only takes input
    // with a session open, which is the sort of thing that stops being
    // true quietly.
    let (Some(net), Some(line)) = (net, journal.take_command()) else {
        return;
    };
    println!("[give] {line}");
    net.send(ClientMessage::Chat(line));
    debug_stats.network_messages_out_this_second += 1;
}

fn works_while_dead(action: keybinds::Action) -> bool {
    !matches!(
        action,
        keybinds::Action::Inventory | keybinds::Action::Eat | keybinds::Action::Drop
    )
}

/// The pack square a throw would come out of, if there is anything in it.
///
/// **Nothing in the hand is nothing to throw.** Q on an empty square used
/// to send `DropSlot` anyway and play the drop sound -- the throw a player
/// hears and cannot see, which reads as the pack losing something. The
/// server refused it quietly, so the only thing that happened was the lie.
/// The same shape as [`eat_from`], which has always asked first.
fn something_to_throw(slot: Option<usize>, inventory: &Inventory) -> Option<usize> {
    slot.filter(|slot| inventory.block_in(*slot).is_some())
}

/// Whether the eat key has anything to do with this, in hand.
///
/// **"Не получается пить из сосуда с водой".** Food and a full jug both
/// go in the mouth by the same key, and the server has always known it:
/// `eat_from_slot` hands a filled vessel straight to `drink_from_slot`,
/// for the reason written there -- from the player's side it is one
/// gesture. The client asked a narrower question, `food::is_food`, which
/// is nutrition or harm; water is neither, so the key did nothing at all
/// with a jug of water in hand and the server never heard about it.
///
/// The two halves of the rule are named from the same two functions the
/// server uses, so they cannot drift apart again: a new drinkable vessel
/// is drinkable here the day it is added there.
fn goes_in_the_mouth(held: BlockId) -> bool {
    primitive_shared::food::is_food(held) || primitive_shared::types::emptied_vessel(held).is_some()
}

fn eat_from(slot: Option<usize>, inventory: &Inventory, meal: &mut Option<Meal>, hand: &mut hand::Hand) {
    if meal.is_some() {
        return;
    }
    let Some((slot, block)) = slot.and_then(|slot| Some((slot, inventory.block_in(slot)?))) else {
        return;
    };
    if !goes_in_the_mouth(block) {
        return;
    }
    let drinking = primitive_shared::types::emptied_vessel(block).is_some();
    hand.gesture(if drinking { protocol_action::Drink } else { protocol_action::Eat });
    *meal = Some(Meal { slot, block, started: Instant::now(), drinking });
}

use primitive_shared::protocol::Action as protocol_action;

/// The message a number key sends while the pointer is over a slot of an
/// open screen: that slot swapped with the matching square of the bar.
/// `None` when no screen is open, nothing is under the pointer, the key is
/// not a number, or the slot already is that square.
fn hover_swap(
    code: KeyCode,
    pack_screen: &inventory_screen::InventoryScreen,
    chest: &chest_screen::ChestScreen,
) -> Option<ClientMessage> {
    use primitive_shared::protocol::Side;
    let square = ui::input::hotbar_slot_for(code).filter(|square| *square < ui::hotbar::MAX_SLOTS)?;
    if chest.is_open() {
        // A jug held open is not a container the server has; its moves
        // are the jug's own messages, and a number key is not one of them.
        if chest.held_vessel().is_some() {
            return None;
        }
        let (side, slot) = chest.hovered()?;
        if (side, slot) == (Side::Pack, square) {
            return None;
        }
        return Some(ClientMessage::ChestMove { from: (side, slot as u8), to: (Side::Pack, square as u8), half: false });
    }
    if pack_screen.open {
        let slot = pack_screen.hovered_slot()?;
        return (slot != square).then_some(ClientMessage::MoveSlots { from: slot as u8, to: square as u8 });
    }
    None
}

/// A mouthful being taken: the hand is at the mouth, and the food goes to the
/// server when the gesture is done.
///
/// **"Сделай задержку при поедании чтобы это не было мгновенно."** Eating
/// was the key going down and the bar filling on the same frame, so a stack
/// of berries was emptied into a player mid-fight at the speed of a key
/// repeat. Now a mouthful takes what the hand's lift takes
/// (`player_model::EAT_SECONDS`, a drink `DRINK_SECONDS`) -- the same seconds
/// everybody watching sees -- and a second press while chewing does nothing.
///
/// **Held on the client, not refused by the server.** A server-side rate
/// limit was the other choice; what a modified client buys by skipping the
/// wait is a second or two, never food it did not have, and a limit would
/// have turned a laggy honest client's mouthful into a refusal.
#[derive(Debug, Clone, Copy)]
struct Meal {
    slot: usize,
    block: BlockId,
    started: Instant,
    drinking: bool,
}

impl Meal {
    fn seconds(&self) -> f32 {
        if self.drinking {
            logic::player_model::DRINK_SECONDS
        } else {
            logic::player_model::EAT_SECONDS
        }
    }

    /// What to do with it now: nothing yet, send it, or drop it because the
    /// thing in that slot is no longer the thing being eaten.
    fn due(&self, inventory: &Inventory, now: Instant) -> MealStep {
        if inventory.block_in(self.slot) != Some(self.block) {
            return MealStep::Abandoned;
        }
        if now.saturating_duration_since(self.started).as_secs_f32() >= self.seconds() {
            MealStep::Swallow
        } else {
            MealStep::Chewing
        }
    }
}

/// A cut into a carcass or a body being made: the knife works for
/// [`CUT_SECONDS`], and the cut goes to the server when it is through.
///
/// **"Добавь задержку при разделывании."** A cut was the click and the meat
/// on the ground in the same frame, so a deer came apart as fast as a mouse
/// button repeats -- five clicks, a carcass gone, which is a chore with no
/// time in it rather than the exposed minute at a kill it should be, with the
/// wolves that come to carrion (`Animals`) still out there.
///
/// On the client, like the meal and for the meal's reason (`Meal`): what a
/// modified client buys by skipping the wait is a second a cut, never meat
/// the carcass did not have. Looking away, changing what is in the hand, or
/// the carcass changing under the knife -- another butcher's cut landing --
/// drops the cut rather than sending it at whatever is aimed at now.
#[derive(Debug, Clone, Copy)]
struct Cut {
    cell: (i32, i32, i32),
    block: BlockId,
    slot: usize,
    started: Instant,
}

/// How long one cut takes. Longer than a mouthful and a good deal longer than
/// a blow: a haunch is jointed, not struck off, and a deer of five cuts is
/// most of ten seconds at the body.
const CUT_SECONDS: f32 = 1.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutStep {
    Cutting,
    Through,
    Abandoned,
}

impl Cut {
    fn due(&self, aimed: Option<((i32, i32, i32), BlockId)>, slot: usize, now: Instant) -> CutStep {
        if aimed != Some((self.cell, self.block)) || slot != self.slot {
            return CutStep::Abandoned;
        }
        if now.saturating_duration_since(self.started).as_secs_f32() >= CUT_SECONDS {
            CutStep::Through
        } else {
            CutStep::Cutting
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MealStep {
    Chewing,
    Swallow,
    Abandoned,
}

/// Turns a gesture on the hotbar into the thing a keyboard would have
/// sent.
///
/// **Choosing a slot is done here and the rest is sent as a key.** The
/// difference is which of them the game already has a path for: the
/// selected slot is a number the client owns outright, while eating and
/// throwing are actions with bindings, a server message and a set of
/// rules about when they are allowed -- all of which exist and none of
/// which should be written a second time for a phone. So the gesture
/// becomes the *key the player has bound*, and everything downstream
/// cannot tell a thumb from a keyboard. Rebinding `E` moves the hold
/// gesture with it, for free.
///
/// The wheel is a wheel for the same reason: `Event::MouseWheel` is
/// already how the bar is stepped, with the wrapping and the clamping
/// written once.
fn hotbar_gesture_event(
    touched: ui::hotbar::Touched,
    input: &mut input::InputState,
    keybinds: &keybinds::Keybinds,
) -> Option<platform::Event> {
    use ui::hotbar::Touched;
    // A key the player has unbound sends nothing, which is what being
    // unbound means. The gesture is still spent -- it did what the
    // binding said, and the binding said nothing.
    let as_key = |action| {
        keybinds.key(action).map(|key| platform::Event::Keyboard {
            key: Some(key),
            // No text: this is a key going down, not a character being
            // typed. See the same note beside the thumb buttons.
            text: platform::Text::default(),
            pressed: true,
            repeat: false,
        })
    };
    match touched {
        Touched::Nothing => None,
        Touched::Pick(slot) => {
            input.hotbar_slot = slot;
            None
        }
        Touched::Step(by) => Some(platform::Event::MouseWheel { lines: by as f32 }),
        // **Never arrives here, and must not become a key if it ever
        // does.** Eating is the one thing the bar reports from
        // `resting` rather than from `handle`, and the frame loop takes
        // it there, where the slot survives into `eat_from`. Turned
        // into a key it would lose the slot and eat from whatever is
        // *selected* instead of from the square under the finger --
        // silently, and only on a phone. The `Throw` arm below has to
        // select the slot first for exactly this reason; the difference
        // is that eating deliberately does not change the selection.
        Touched::Eat(_) => None,
        Touched::Throw(slot) => {
            // The slot thrown from is the slot chosen: `Q` throws from
            // whatever is held, so a flick off a square the player was
            // not holding has to select it first or it would throw the
            // wrong thing. The selection is theirs afterwards, which is
            // also what a keyboard player gets.
            input.hotbar_slot = slot;
            as_key(keybinds::Action::Drop)
        }
    }
}

/// Translates a physical key into the menu's own key vocabulary.
///
/// Letter shortcuts come from `text` rather than the key code, so they
/// follow the player's keyboard layout: `KeyCode::KeyA` is the physical
/// key marked A on QWERTY and the one marked Q on AZERTY.
///
/// ## Why the input method can veto the character
///
/// Because `menu::Key::Char` is not a shortcut once a form is open --
/// `Menu::key` types it into the focused field -- and while an Android
/// input method owns that field the character has already been put
/// there through the mirror. Letting it through here writes it twice.
///
/// **This is the bug that showed up as digits duplicating and nothing
/// else duplicating**, which is what made it look like a numeric field
/// problem. It is not: it is about which characters arrive as *keys*.
/// A soft keyboard commits its letters as text and sends no key code
/// for them -- there is no key code for `щ`, which is the whole reason
/// this game runs on GameActivity -- so a letter never reached this
/// function at all. Digits have key codes. So winit reported
/// `Character("5")` beside the commit, `typed_text`'s Android fallback
/// turned it into text, and the digit went into the field once through
/// the input method and once more through here. `5` came out `55` and
/// `q` came out `q`.
///
/// The caller already withholds the *text* loop and the deletions for
/// the same reason; this is the third door into the same field and the
/// one that was left open. Navigation is unaffected: Escape, Enter,
/// Tab and the arrows are matched above and never consult `text`.
/// Whether the game may put a keystroke's *characters* into the field
/// itself.
///
/// **The condition this replaces was a double negative, and this bug
/// has been chased three times.** It read
/// `accepts_text && !(accepts_text && ime_owns_text)`, spread over two
/// statements twenty lines apart, and what it actually says is the line
/// below. Written out, the phone case is obvious: while an input method
/// owns the field, `ime_owns_text` is true, so this is false, so the
/// game never types anything itself -- every character arrives through
/// the mirror in `AboutToWait` instead.
///
/// Named and tested rather than left inline because the two doors into
/// a field have to agree, and the other one ([`menu_key`]) is right
/// here with its own tests. A guard nobody can point at is a guard the
/// next edit removes by accident.
fn keys_may_type_into_the_field(accepts_text: bool, ime_owns_text: bool) -> bool {
    accepts_text && !ime_owns_text
}

fn menu_key(
    code: KeyCode,
    text: Option<char>,
    ime_owns_the_field: bool,
    shift: bool,
    ctrl: bool,
) -> Option<menu::Key> {
    match code {
        KeyCode::ArrowUp => Some(menu::Key::Up),
        KeyCode::ArrowDown => Some(menu::Key::Down),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(menu::Key::Enter),
        KeyCode::Escape => Some(menu::Key::Escape),
        KeyCode::Tab => Some(menu::Key::Tab),
        KeyCode::Backspace => Some(menu::Key::Backspace { word: ctrl }),
        KeyCode::Delete => Some(menu::Key::Delete { word: ctrl }),
        KeyCode::ArrowLeft => Some(menu::Key::Left { word: ctrl, extend: shift }),
        KeyCode::ArrowRight => Some(menu::Key::Right { word: ctrl, extend: shift }),
        KeyCode::Home => Some(menu::Key::Home { extend: shift }),
        KeyCode::End => Some(menu::Key::End { extend: shift }),
        // Control-A, and only with control: a bare `a` is a letter, and
        // on the server list it is also the shortcut for ADD.
        KeyCode::KeyA if ctrl => Some(menu::Key::SelectAll),
        _ if ime_owns_the_field => None,
        _ => text.filter(|c| c.is_ascii_graphic()).map(menu::Key::Char),
    }
}


#[cfg(test)]
mod aiming_tests {
    use super::{aimed_block, aimed_block_to_mine, use_gesture, UseGesture};
    use crate::engine::camera::Camera;
    use crate::logic::physics::tests::lake_world;
    use glam::Vec3;
    use primitive_shared::types::{is_liquid, BLOCK_STONE};

    /// A camera in the middle of the test lake -- stone to y = 10, water to
    /// y = 20 -- looking `dir`.
    fn looking(at: Vec3, dir: Vec3) -> Camera {
        let mut camera = Camera::new(at.as_dvec3(), 1.0);
        // The look is built from the angles, so the angles are what has to be
        // set: a forward vector written into the camera would be worked out
        // again from yaw and pitch on the next call and thrown away.
        let dir = dir.normalize();
        camera.yaw = dir.z.atan2(dir.x);
        camera.pitch = dir.y.clamp(-1.0, 1.0).asin();
        assert!((camera.forward() - dir).length() < 1e-4, "the test camera is not looking where it was pointed");
        camera
    }

    #[test]
    fn a_diver_swinging_at_the_lake_bed_is_aiming_at_the_bed_and_not_at_the_water() {
        // **"Под водой нельзя ломать".** The pick and the right click were
        // casting one ray with water switched on, so under water the swing's
        // target was always the first cell of water in front of the player,
        // which `is_breakable_with` then threw away -- no target, no cracks,
        // nothing sent, and no way to tell that from a broken game.
        let chunks = lake_world();
        let down = looking(Vec3::new(8.0, 12.5, 8.0), Vec3::NEG_Y);
        let (cell, block) = aimed_block_to_mine(&chunks, &down).expect("a diver aimed at nothing at all");
        assert_eq!(block, BLOCK_STONE, "the swing found {block} at {cell:?} instead of the bed");
        assert_eq!(cell.1, 9, "the swing found the bed at {cell:?}");

        // ...and at a slant, which is every swing that is not straight down:
        // the ray crosses several cells of water on its way to the bed.
        let slanted = looking(Vec3::new(8.0, 12.5, 8.0), Vec3::new(0.4, -0.9, 0.2));
        let (_, block) = aimed_block_to_mine(&chunks, &slanted).expect("nothing was aimed at through the water");
        assert_eq!(block, BLOCK_STONE, "a slanted swing under water found {block}");
    }

    #[test]
    fn the_right_click_still_sees_the_river_it_was_taught_to_drink_from() {
        // The other side of the same flag, and the reason it cannot simply
        // be turned off: a thirsty player clicking at water has to find
        // water, or every rule the server has about mouthfuls is unreachable
        // again (`aimed_block`).
        let chunks = lake_world();
        let down = looking(Vec3::new(8.0, 22.0, 8.0), Vec3::NEG_Y);
        let (_, block) = aimed_block(&chunks, &down).expect("a right click at a lake met nothing");
        assert!(is_liquid(block), "the right click looked straight through the lake at {block}");
        assert_eq!(use_gesture(Some(block), None), UseGesture::Water, "the lake stopped being a drink");
    }
}

#[cfg(test)]
mod drinking_tests {
    use super::goes_in_the_mouth;

    #[test]
    fn a_cut_takes_the_knifes_time_and_looking_away_drops_it() {
        use super::{Cut, CutStep};
        use primitive_shared::types::BLOCK_CARCASS_DEER;
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let at = ((4, 10, 4), BLOCK_CARCASS_DEER);
        let cut = Cut { cell: at.0, block: at.1, slot: 2, started: start };
        assert_eq!(cut.due(Some(at), 2, start), CutStep::Cutting, "a cut was instant");
        assert_eq!(cut.due(Some(at), 2, start + Duration::from_millis(900)), CutStep::Cutting);
        assert_eq!(cut.due(Some(at), 2, start + Duration::from_secs(2)), CutStep::Through, "the knife never got through");
        assert_eq!(cut.due(None, 2, start + Duration::from_secs(2)), CutStep::Abandoned, "a cut sent at nothing");
        assert_eq!(cut.due(Some(at), 3, start + Duration::from_secs(2)), CutStep::Abandoned, "a cut made with a different hand");
        let cut_by_someone_else = ((4, 10, 4), BLOCK_CARCASS_DEER | (1 << primitive_shared::types::VARIANT_SHIFT));
        assert_eq!(cut.due(Some(cut_by_someone_else), 2, start + Duration::from_secs(2)), CutStep::Abandoned);
    }

    #[test]
    fn a_mouthful_waits_for_the_hand_and_is_dropped_if_the_food_leaves_the_slot() {
        use super::{Meal, MealStep};
        use primitive_shared::types::BLOCK_BERRIES;
        use std::time::{Duration, Instant};
        let mut pack = primitive_shared::inventory::Inventory::new();
        pack.add(BLOCK_BERRIES, 3);
        let slot = (0..40).find(|&s| pack.block_in(s) == Some(BLOCK_BERRIES)).expect("berries in the pack");
        let start = Instant::now();
        let meal = Meal { slot, block: BLOCK_BERRIES, started: start, drinking: false };
        assert_eq!(meal.due(&pack, start), MealStep::Chewing, "eating was instant");
        assert_eq!(meal.due(&pack, start + Duration::from_millis(300)), MealStep::Chewing);
        assert_eq!(meal.due(&pack, start + Duration::from_secs(3)), MealStep::Swallow, "the mouthful never went down");
        pack.take_from(slot, 3);
        assert_eq!(meal.due(&pack, start + Duration::from_secs(3)), MealStep::Abandoned, "food eaten out of an empty slot");
    }
    use primitive_shared::types::{BLOCK_BERRIES, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_STONE};

    #[test]
    fn the_eat_key_drinks_a_full_jug_and_still_eats_food() {
        // **"Не получается пить из сосуда с водой".** The server would
        // have poured it (`drink_from_slot`); the client decided on
        // `food::is_food` and never sent the message, so the key was
        // dead in the hand that most needed it. Every water a jug can
        // hold, because the kind of water rides in the id's variant
        // (`types::vessel_water`) and a test on one water would have
        // passed while river water stayed undrinkable.
        for water in 0..4u16 {
            let jug = BLOCK_JUG_WATER | (water << primitive_shared::types::VARIANT_SHIFT);
            assert!(goes_in_the_mouth(jug), "a jug of water {water} cannot be drunk");
        }
        assert!(goes_in_the_mouth(BLOCK_BERRIES), "the key stopped being an eat key");
        assert!(!goes_in_the_mouth(BLOCK_JUG), "an empty jug is a gesture that does nothing");
        assert!(!goes_in_the_mouth(BLOCK_STONE), "a stone went in the mouth");
    }
}

#[cfg(test)]
mod dead_hands_tests {
    use super::{keybinds, something_to_throw, works_while_dead, Inventory};

    #[test]
    fn a_dead_player_cannot_open_the_pack_eat_or_throw() {
        // The pack opening behind the death screen is what left the
        // cursor grabbed on a screen made of buttons.
        for action in [
            keybinds::Action::Inventory,
            keybinds::Action::Eat,
            keybinds::Action::Drop,
        ] {
            assert!(!works_while_dead(action), "{action:?} still reached a dead body");
        }
    }

    #[test]
    fn the_death_screen_still_answers_respawn_and_every_other_key() {
        assert!(
            works_while_dead(keybinds::Action::Respawn),
            "the one key the death screen exists for was swallowed"
        );
        for action in keybinds::Action::ALL {
            if matches!(
                action,
                keybinds::Action::Inventory | keybinds::Action::Eat | keybinds::Action::Drop
            ) {
                continue;
            }
            assert!(works_while_dead(action), "{action:?} stopped working on the death screen");
        }
    }

    #[test]
    fn throwing_from_an_empty_square_sends_nothing_and_makes_no_sound() {
        let mut pack = Inventory::new();
        pack.add(primitive_shared::types::BLOCK_STONE, 5);
        let full = (0..crate::logic::inventory::SLOTS)
            .find(|slot| pack.block_in(*slot).is_some())
            .expect("the stone went somewhere");
        let empty = (0..crate::logic::inventory::SLOTS)
            .find(|slot| pack.block_in(*slot).is_none())
            .expect("a pack with one stack in it has room");
        assert_eq!(something_to_throw(Some(full), &pack), Some(full));
        assert_eq!(
            something_to_throw(Some(empty), &pack),
            None,
            "a throw from nothing would still be sent and heard"
        );
        assert_eq!(something_to_throw(None, &pack), None);
    }
}

#[cfg(test)]
mod form_key_tests {
    use super::{keys_may_type_into_the_field, menu, menu_key, KeyCode};

    /// On a phone the game never types into a field itself.
    ///
    /// **Both doors, asserted together, because the bug is that they
    /// disagree.** A character reaches a form field two ways: the text
    /// loop, guarded by [`keys_may_type_into_the_field`], and the
    /// shortcut lookup, guarded inside [`menu_key`]. While an Android
    /// input method owns the field it has *already* put the character
    /// there and the mirror will bring it across; either door left open
    /// writes it a second time.
    ///
    /// The asymmetry that made this look like a numeric-field problem
    /// is in the last line: `menu_key` builds a character out of
    /// `is_ascii_graphic`, so Cyrillic never came through that door at
    /// all and only Latin and digits ever doubled. That is a fact about
    /// which characters have key codes, not about which fields are
    /// numeric.
    #[test]
    fn on_a_phone_the_game_never_types_into_a_field_itself() {
        // The phone: an input method owns the field.
        assert!(
            !keys_may_type_into_the_field(true, true),
            "the text loop would type a character the mirror is already bringing",
        );
        for typed in ['q', '5', 'ф'] {
            assert_eq!(
                menu_key(KeyCode::KeyQ, Some(typed), true, false, false),
                None,
                "{typed:?} came through the shortcut door as well",
            );
        }

        // The desktop: there is no other copy of the field, so the game
        // is the only thing that can fill it.
        assert!(
            keys_may_type_into_the_field(true, false),
            "a desktop keystroke was refused and the field would stay empty",
        );
        assert_eq!(menu_key(KeyCode::KeyQ, Some('q'), false, false, false), Some(menu::Key::Char('q')));

        // ...and neither door opens for a screen with no field on it.
        assert!(!keys_may_type_into_the_field(false, false));
        assert!(!keys_may_type_into_the_field(false, true));
    }

    /// A digit typed into a form on a phone lands in it once.
    ///
    /// The bug this is here for read as "digits duplicate and letters
    /// do not", which sounds like a numeric field and is not. Both
    /// halves come out of the same fact: an Android input method
    /// commits its letters as *text* and sends no key code for them --
    /// there is no key code for `щ` -- while digits have key codes and
    /// arrive as keys as well as in the commit. So the letter reached
    /// the field once and the digit reached it twice, once through the
    /// input method's mirror and once through `Menu::key`, which types
    /// a `Key::Char` into the focused field.
    ///
    /// Both directions are asserted. A digit that stopped being a
    /// shortcut on a *desktop* would break every letter shortcut on
    /// every menu, which is a worse bug than the one being fixed.
    #[test]
    fn a_digit_typed_into_a_form_on_a_phone_is_not_also_typed_as_a_shortcut() {
        // The phone: the input method owns the field, so no character
        // may travel as a key.
        assert_eq!(menu_key(KeyCode::Digit5, Some('5'), true, false, false), None);
        assert_eq!(menu_key(KeyCode::KeyQ, Some('q'), true, false, false), None);

        // The desktop: it may, and must, or the shortcuts are gone.
        assert_eq!(
            menu_key(KeyCode::Digit5, Some('5'), false, false, false),
            Some(menu::Key::Char('5')),
        );
        assert_eq!(
            menu_key(KeyCode::KeyQ, Some('q'), false, false, false),
            Some(menu::Key::Char('q')),
        );
    }

    /// Getting out of a form still works while the keyboard owns it.
    ///
    /// The veto above is on *writing*, not on the keys a form is left,
    /// submitted and moved around by. Held back with the characters,
    /// a player on a phone could open the create-world form and never
    /// close it.
    #[test]
    fn navigation_keys_still_work_while_the_input_method_owns_the_field() {
        for (code, expected) in [
            (KeyCode::Escape, menu::Key::Escape),
            (KeyCode::Enter, menu::Key::Enter),
            (KeyCode::Tab, menu::Key::Tab),
            (KeyCode::ArrowUp, menu::Key::Up),
            (KeyCode::ArrowDown, menu::Key::Down),
        ] {
            assert_eq!(
                menu_key(code, None, true, false, false),
                Some(expected),
                "{code:?} was swallowed along with the characters",
            );
        }
    }
}

#[cfg(test)]
mod use_gesture_tests {
    use super::{swallow_expected, use_gesture, UseGesture};
    use primitive_shared::types::{
        BLOCK_BREAD, BLOCK_CHEST, BLOCK_COBBLESTONE, BLOCK_FLINT, BLOCK_KILN, BLOCK_STONE,
    };

    /// Food in your hand is eaten by using it.
    ///
    /// **What the player asked for, in as many words**: "сделай
    /// возможность есть взяв в руку, а не через HUD". Eating hung on a
    /// key and, on a phone, on resting a finger on a hotbar slot -- a
    /// gesture on the interface for something the hand does. The hand
    /// was already making the right gesture and it did nothing at all,
    /// because food is not a block and placing a loaf is not a thing.
    #[test]
    fn using_bread_that_is_in_your_hand_eats_it_instead_of_doing_nothing() {
        assert_eq!(use_gesture(None, Some(BLOCK_BREAD)), UseGesture::Eat);
        // ...and aiming at ordinary ground changes nothing: what is
        // eaten is what is held, not what is looked at.
        assert_eq!(
            use_gesture(Some(BLOCK_STONE), Some(BLOCK_BREAD)),
            UseGesture::Eat,
        );
    }

    /// A chest opens even when you are carrying your dinner.
    ///
    /// **The collision this order exists to settle.** If food took the
    /// gesture first, the only way to open a chest while holding bread
    /// would be to put the bread down -- and it would land on the
    /// ground in front of the chest, which is the exact argument the
    /// container branch was written with in the first place. A player
    /// who walks to a chest holding dinner meant the chest.
    #[test]
    fn using_a_chest_with_food_in_hand_opens_the_chest() {
        assert_eq!(
            use_gesture(Some(BLOCK_CHEST), Some(BLOCK_BREAD)),
            UseGesture::Open,
        );
    }

    /// A rack clicked with kelp in hand opens its screen, for the chest's
    /// reason: kelp is food, and a frond eaten at the rack it was carried to
    /// is a rack nobody can load without emptying their hands first. Asked
    /// of a cell of the rack of two by two with goods already hanging on it,
    /// because a loaded rack is a different id from a bare one.
    #[test]
    fn a_rack_clicked_with_kelp_in_hand_opens_rather_than_eating_the_kelp() {
        use primitive_shared::types::{rack_cells, with_rack_goods, Facing, BLOCK_KELP_FROND};
        for (_, cell) in rack_cells((0, 64, 0), Facing::North) {
            let loaded = with_rack_goods(cell, 0b11);
            assert_eq!(use_gesture(Some(cell), Some(BLOCK_KELP_FROND)), UseGesture::Open);
            assert_eq!(use_gesture(Some(loaded), Some(BLOCK_KELP_FROND)), UseGesture::Open);
        }
    }

    /// Striking flint on a kiln still lights it rather than opening it.
    ///
    /// The one exception a hearth carries, and it survived the rewrite:
    /// a hearth is a container, so a use opens it -- but the striker in
    /// hand means the gesture is about the fire and not about what is
    /// inside.
    #[test]
    fn striking_flint_on_a_kiln_is_still_about_the_fire() {
        assert_eq!(
            use_gesture(Some(BLOCK_KILN), Some(BLOCK_FLINT)),
            UseGesture::Hearth,
        );
        assert_eq!(
            use_gesture(Some(BLOCK_KILN), Some(BLOCK_BREAD)),
            UseGesture::Open,
        );
    }

    /// The clicks that work a thing where it stands, and never put anything
    /// down: see `tends`.
    #[test]
    fn a_knife_at_a_trunk_ash_at_a_furrow_and_resin_at_a_torch_are_not_placements() {
        use primitive_shared::types::{
            oriented, Axis, BLOCK_ASH, BLOCK_FARMLAND, BLOCK_FLINT_KNIFE, BLOCK_LOG, BLOCK_RESIN, BLOCK_STANDING_TORCH,
            BLOCK_STANDING_TORCH_LIT, BLOCK_STANDING_TORCH_OUT, BLOCK_TORCH,
        };
        assert_eq!(use_gesture(Some(BLOCK_LOG), Some(BLOCK_FLINT_KNIFE)), UseGesture::Tend);
        assert_ne!(use_gesture(Some(oriented(BLOCK_LOG, Axis::X)), Some(BLOCK_FLINT_KNIFE)), UseGesture::Tend, "deadfall is scored for resin");
        assert_eq!(use_gesture(Some(BLOCK_FARMLAND), Some(BLOCK_ASH)), UseGesture::Tend);
        assert_eq!(use_gesture(Some(BLOCK_STONE), Some(BLOCK_ASH)), UseGesture::Place, "ash can no longer be laid");
        for torch in [BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_OUT] {
            assert_eq!(use_gesture(Some(torch), Some(BLOCK_RESIN)), UseGesture::Tend);
        }
        // A torch at a standing torch alight is lit from it, as at any fire.
        assert_eq!(use_gesture(Some(BLOCK_STANDING_TORCH_LIT), Some(BLOCK_TORCH)), UseGesture::Hearth);
    }

    /// An unlit torch at a burning fire lights, rather than opening the
    /// fire's screen -- and at a cold one it opens it, because there is no
    /// flame to take.
    #[test]
    fn a_torch_held_to_a_burning_fire_is_lit_rather_than_opening_it() {
        use primitive_shared::types::{
            BLOCK_BURNING_LOG, BLOCK_BURNING_PLANKS, BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_FIREPIT_LIT,
            BLOCK_KILN_LIT, BLOCK_LOG_PILE_LIT, BLOCK_PIT_KILN_LIT, BLOCK_STANDING_TORCH_LIT, BLOCK_TORCH,
        };
        for fire in [
            BLOCK_CAMPFIRE_LIT,
            BLOCK_FIREPIT_LIT,
            BLOCK_KILN_LIT,
            BLOCK_PIT_KILN_LIT,
            BLOCK_LOG_PILE_LIT,
            BLOCK_BURNING_LOG,
            BLOCK_BURNING_PLANKS,
            BLOCK_STANDING_TORCH_LIT,
        ] {
            assert_eq!(use_gesture(Some(fire), Some(BLOCK_TORCH)), UseGesture::Hearth, "fire {fire}");
        }
        assert_eq!(use_gesture(Some(BLOCK_CAMPFIRE), Some(BLOCK_TORCH)), UseGesture::Open);
    }

    /// A log, fibre, flint or an empty hand at a pit kiln or a log pile goes
    /// *into* it, and never builds on top of it.
    ///
    /// Without this a log in hand at a kiln with seven logs on it was a
    /// placement: a log block laid over the pit, which smothers the kiln it
    /// was carried there to finish.
    #[test]
    fn a_log_at_a_pit_kiln_goes_into_the_kiln_rather_than_onto_it() {
        use primitive_shared::pit::{log_pile, Stage};
        use primitive_shared::types::{BLOCK_FIBER, BLOCK_LOG};
        for block in [Stage::Fibre(8).block(), Stage::Logs(7).block(), Stage::Burning.block(), log_pile(3)] {
            for held in [Some(BLOCK_LOG), Some(BLOCK_FIBER), Some(BLOCK_FLINT), Some(BLOCK_BREAD), None] {
                assert_eq!(use_gesture(Some(block), held), UseGesture::Pit, "block {block} with {held:?}");
            }
        }
    }

    /// A knife at a carcass is a cut, and not a placement that does
    /// nothing.
    ///
    /// **The bug this order prevents**: a knife is not placeable, so
    /// before the carcass had a claim of its own, right-clicking one
    /// with a knife in hand fell through to `Place` and did nothing --
    /// the server never heard about it. The bare hand is sent too, so
    /// the server can say why nothing came off.
    #[test]
    fn a_knife_at_a_carcass_is_a_cut_and_not_a_placement() {
        use primitive_shared::animals::carcass_at_stage;
        use primitive_shared::animals::Species;
        use primitive_shared::types::{BLOCK_CARCASS_DEER, BLOCK_FLINT_KNIFE};
        assert_eq!(
            use_gesture(Some(BLOCK_CARCASS_DEER), Some(BLOCK_FLINT_KNIFE)),
            UseGesture::Butcher,
        );
        assert_eq!(use_gesture(Some(BLOCK_CARCASS_DEER), None), UseGesture::Butcher);
        // Half-butchered is still a carcass: the stage lives in the
        // variant bits and the claim has to look past them.
        assert_eq!(
            use_gesture(Some(carcass_at_stage(Species::Deer, 2)), Some(BLOCK_FLINT_KNIFE)),
            UseGesture::Butcher,
        );
        // ...and a player at a dead deer holding lunch meant the deer.
        assert_eq!(
            use_gesture(Some(BLOCK_CARCASS_DEER), Some(BLOCK_BREAD)),
            UseGesture::Butcher,
        );
    }

    /// A right click on apples picks them, whatever is in the hand, and a
    /// right click on the leaves round them does not.
    ///
    /// **What the player asked for**: picking "через пкм … без ломания".
    /// Before this, a right click on a fruiting leaf with a stone in hand
    /// put the stone on the tree, and with bread in hand ate the bread --
    /// the apple could only be had by breaking the leaf.
    #[test]
    fn a_right_click_on_apples_picks_them_whatever_is_in_the_hand() {
        use primitive_shared::types::{
            BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES_PICKED,
        };
        for held in [None, Some(BLOCK_BREAD), Some(BLOCK_COBBLESTONE), Some(BLOCK_FLINT)] {
            assert_eq!(
                use_gesture(Some(BLOCK_APPLE_LEAVES_FRUIT), held),
                UseGesture::Pick,
                "apples with {held:?} in hand were not picked"
            );
        }
        // The leaves themselves -- bare, or picked and filling -- are
        // something to build against, as they always were.
        for leaves in [BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_PICKED] {
            assert_eq!(use_gesture(Some(leaves), Some(BLOCK_COBBLESTONE)), UseGesture::Place);
            assert_eq!(use_gesture(Some(leaves), Some(BLOCK_BREAD)), UseGesture::Eat);
        }
    }

    /// Everything that is not one of the four still goes in the world.
    #[test]
    fn using_a_block_that_is_not_food_still_places_it() {
        assert_eq!(
            use_gesture(Some(BLOCK_STONE), Some(BLOCK_COBBLESTONE)),
            UseGesture::Place,
        );
        assert_eq!(use_gesture(None, None), UseGesture::Place);
    }

    #[test]
    fn a_right_click_at_either_half_of_a_door_swings_it_whatever_is_in_hand_and_swings_both_halves_here() {
        // The gesture first: open or shut, top or bottom, a hand full of
        // boards or of nothing -- a door is swung, never built against.
        use primitive_shared::types::{
            door_partner, door_swung, faced, ChunkPos, Facing, BLOCK_COBBLESTONE, BLOCK_DOOR, BLOCK_PLANKS,
        };
        let lower = faced(BLOCK_DOOR, Facing::South);
        let (_, top) = door_partner((0, 0, 0), lower).unwrap();
        for block in [lower, top, door_swung(lower), door_swung(top)] {
            for held in [None, Some(BLOCK_COBBLESTONE), Some(BLOCK_PLANKS), Some(BLOCK_DOOR)] {
                assert_eq!(use_gesture(Some(block), held), UseGesture::Swing, "{block} with {held:?}");
            }
        }
        // ...and the prediction is the server's two cells: the half clicked
        // and its partner, and not a stranger's half over it.
        let mut chunks = crate::logic::chunk_manager::ChunkManager::new(4);
        let mut blocks = vec![primitive_shared::types::BLOCK_AIR; primitive_shared::types::CHUNK_VOLUME];
        let at = (3, 40, 3);
        blocks[primitive_shared::types::Chunk::index(3, 40, 3)] = lower;
        blocks[primitive_shared::types::Chunk::index(3, 41, 3)] = top;
        chunks.insert(primitive_shared::types::Chunk { pos: ChunkPos::new(0, 0), blocks });
        let swung = crate::door_swing(&chunks, (at.0, at.1 + 1, at.2), top);
        let cells: Vec<_> = swung.iter().map(|c| ((c.global_x, c.global_y, c.global_z), c.block_id)).collect();
        assert_eq!(cells, vec![((3, 41, 3), door_swung(top)), (at, door_swung(lower))]);
        chunks.apply_block_update(3, 41, 3, door_swung(top));
        assert_eq!(crate::door_swing(&chunks, at, lower).len(), 1, "a stranger's half was swung with the door");
    }

    #[test]
    fn a_right_click_on_a_bed_or_a_stool_rests_whatever_is_in_hand() {
        // **The bug this exists for**: nobody could sleep. The server has
        // answered a use on a bed since beds existed, and the client never
        // sent one -- an empty hand at a bed placed nothing, and a block in
        // hand was built onto the bed. Either half of a bed, any way round.
        use primitive_shared::types::{
            bed_half, Facing, BLOCK_BREAD, BLOCK_COBBLESTONE, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE,
        };
        for block in [
            bed_half(Facing::East, false),
            bed_half(Facing::West, true),
            BLOCK_STRAW_BED,
            BLOCK_STOOL,
            // ...and a chair, any way round: a seat added to the server and
            // not here is a chair a right click builds on.
            primitive_shared::types::faced(primitive_shared::types::BLOCK_CHAIR, Facing::North),
            primitive_shared::types::faced(primitive_shared::types::BLOCK_CHAIR, Facing::West),
        ] {
            for held in [None, Some(BLOCK_COBBLESTONE), Some(BLOCK_BREAD)] {
                assert_eq!(use_gesture(Some(block), held), UseGesture::Rest, "{block} with {held:?}");
            }
        }
        // A table rests nobody, so it is built against like any block.
        assert_eq!(use_gesture(Some(BLOCK_TABLE), Some(BLOCK_COBBLESTONE)), UseGesture::Place);
    }

    /// **A click at a river has to reach the server, and for two
    /// versions it did not.**
    ///
    /// The drinking rules -- a mouthful from fresh water, sickness from
    /// stale, thirst from salt, a jug filled and carrying the answer
    /// away -- were all written, tested and unreachable: the ray was
    /// taught to see water and this table was not, so the gesture came
    /// out `Place` and a thirsty player clicking at a lake got silence.
    ///
    /// The narrowness is the other half. Water wins only for an empty
    /// hand or an empty vessel; a player holding stone at a lake edge is
    /// building, and the placement ray goes through water to the bed as
    /// it always did.
    #[test]
    fn a_click_at_water_drinks_or_fills_but_never_stops_a_player_building() {
        use primitive_shared::types::{BLOCK_JUG, BLOCK_STONE, BLOCK_WATER};
        assert_eq!(use_gesture(Some(BLOCK_WATER), None), UseGesture::Water);
        assert_eq!(
            use_gesture(Some(BLOCK_WATER), Some(BLOCK_JUG)),
            UseGesture::Water,
            "a jug at a river is the whole point of carrying one"
        );
        assert_eq!(
            use_gesture(Some(BLOCK_WATER), Some(BLOCK_STONE)),
            UseGesture::Place,
            "a lake is not a reason to refuse a block"
        );
        assert_eq!(
            use_gesture(Some(BLOCK_WATER), Some(primitive_shared::types::BLOCK_BREAD)),
            UseGesture::Eat,
            "bread is not a vessel, and standing at a lake does not stop you eating it"
        );
    }

    #[test]
    fn a_jug_at_a_barrel_pours_or_dips_and_anything_else_builds_against_it() {
        use primitive_shared::types::{
            barrel_of, BLOCK_BARREL, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_STONE,
        };
        let half_full = barrel_of(primitive_shared::body::Water::Fresh, 3);
        for barrel in [BLOCK_BARREL, half_full] {
            assert_eq!(
                use_gesture(Some(barrel), Some(BLOCK_JUG)),
                UseGesture::Water,
                "an empty jug at a barrel was set down on it instead of dipped"
            );
            assert_eq!(
                use_gesture(Some(barrel), Some(BLOCK_JUG_WATER)),
                UseGesture::Water,
                "a full jug at a barrel was not poured"
            );
            assert_eq!(use_gesture(Some(barrel), Some(BLOCK_STONE)), UseGesture::Place);
            // **An empty hand drinks.** This line used to assert `Place`,
            // on the reading that barrel water is reached with a jug the
            // way it is carried -- which left the barrel the one water a
            // thirsty player could not drink from with a bare hand while
            // the pond beside it let them. Whether there is anything to
            // drink is the server's answer (`use_barrel`), so an empty
            // barrel is sent too and answered in words.
            assert_eq!(
                use_gesture(Some(barrel), None),
                UseGesture::Water,
                "a bare hand at a barrel built against it instead of drinking"
            );
        }
    }

    #[test]
    fn a_jug_at_a_barrel_of_grain_is_a_pour_or_a_scoop_and_a_bare_hand_there_swallows_nothing() {
        // The client cannot see what is in the jug -- that is in the stack's
        // `damage`, and `held` is an id -- so a jug of seed and an empty jug
        // make the same claim at a barrel and the server decides which it
        // is (`use_barrel`). What the client must not do is play a swallow
        // at a heap of grain.
        use primitive_shared::types::{barrel_of_goods, BLOCK_JUG, BLOCK_SEEDS};
        let seed = barrel_of_goods(BLOCK_SEEDS, 4).unwrap();
        assert_eq!(use_gesture(Some(seed), Some(BLOCK_JUG)), UseGesture::Water, "a jug at a barrel of seed was set down on it");
        assert!(!swallow_expected(Some(seed), None, 0.0), "a thirsty hand at a barrel of seed was heard drinking");
    }

    #[test]
    fn a_thing_set_down_is_taken_back_whatever_is_in_the_hand() {
        // Its store would make it a chest to open (`is_container`), and a
        // knife in the hand would be eaten or scored into it; the plain click
        // takes it back, and only the modifier sets another down.
        use primitive_shared::types::{faced, Facing, BLOCK_BREAD, BLOCK_COPPER_KNIFE, BLOCK_SET_DOWN};
        let lying = faced(BLOCK_SET_DOWN, Facing::West);
        for held in [None, Some(BLOCK_BREAD), Some(BLOCK_COPPER_KNIFE)] {
            assert_eq!(use_gesture(Some(lying), held), UseGesture::Pick, "with {held:?} in hand");
        }
    }

    #[test]
    fn a_jug_in_the_hand_opens_unless_a_block_wants_it_first() {
        use primitive_shared::types::{
            BLOCK_BARREL, BLOCK_CHEST, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_STONE, BLOCK_WATER,
        };
        assert_eq!(use_gesture(None, Some(BLOCK_JUG)), UseGesture::OpenVessel);
        assert_eq!(
            use_gesture(Some(BLOCK_STONE), Some(BLOCK_JUG)),
            UseGesture::OpenVessel,
            "a jug aimed at the floor was set down rather than opened -- setting down is \
             the modifier's now"
        );
        // What the jug was carried to a block *for* wins over opening it.
        assert_eq!(use_gesture(Some(BLOCK_WATER), Some(BLOCK_JUG)), UseGesture::Water);
        assert_eq!(use_gesture(Some(BLOCK_BARREL), Some(BLOCK_JUG)), UseGesture::Water);
        assert_eq!(use_gesture(Some(BLOCK_CHEST), Some(BLOCK_JUG)), UseGesture::Open);
        // ...and a jug on a table is a container like any other.
        assert_eq!(use_gesture(Some(BLOCK_JUG), None), UseGesture::Open);
        assert_ne!(
            use_gesture(None, Some(BLOCK_JUG_WATER)),
            UseGesture::OpenVessel,
            "a jug of water opened: it holds one measure of one river and nothing to look at"
        );
    }

    /// **A rod at water casts and a trap with fish in it empties**, whatever
    /// else the hand or the block would have meant: a rod at a lake is not a
    /// thing to place against the lake, and a trap full of fish is not a wall
    /// to build a block onto.
    #[test]
    fn a_rod_at_the_water_casts_and_a_full_trap_is_emptied_before_it_is_built_on() {
        use primitive_shared::types::{
            trap_holding, BLOCK_FISHING_ROD, BLOCK_FISH_TRAP, BLOCK_RAW_FISH, BLOCK_STONE, BLOCK_WATER,
        };
        assert_eq!(use_gesture(Some(BLOCK_WATER), Some(BLOCK_FISHING_ROD)), UseGesture::Water);
        assert_ne!(use_gesture(Some(BLOCK_STONE), Some(BLOCK_FISHING_ROD)), UseGesture::Water, "a cast at stone");
        for held in [None, Some(BLOCK_STONE), Some(BLOCK_RAW_FISH)] {
            assert_eq!(use_gesture(Some(trap_holding(2)), held), UseGesture::Pick, "a full trap with {held:?}");
        }
        assert_ne!(use_gesture(Some(BLOCK_FISH_TRAP), Some(BLOCK_STONE)), UseGesture::Pick, "an empty trap was emptied");
    }

    #[test]
    fn a_swallow_is_heard_only_for_a_bare_hand_at_water_it_can_drink() {
        use primitive_shared::body::Water;
        use primitive_shared::types::{barrel_of, BLOCK_BARREL, BLOCK_JUG, BLOCK_STONE, BLOCK_WATER};
        assert!(swallow_expected(Some(BLOCK_WATER), None, 0.5));
        assert!(swallow_expected(Some(barrel_of(Water::Standing, 2)), None, 0.5));
        assert!(!swallow_expected(Some(BLOCK_BARREL), None, 0.5), "an empty barrel was swallowed");
        assert!(
            !swallow_expected(Some(BLOCK_WATER), Some(BLOCK_JUG), 0.5),
            "filling a jug sounded like drinking"
        );
        assert!(!swallow_expected(Some(BLOCK_STONE), None, 0.5));
        assert!(!swallow_expected(Some(BLOCK_WATER), None, 1.0), "a full player was heard drinking");
        assert!(
            swallow_expected(Some(barrel_of(Water::Salt, 1)), None, 1.0),
            "the sea is swallowed however full the player is, and it should sound like it"
        );
    }
}
