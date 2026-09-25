//! **The numbers the frame is measured by.**
//!
//! The F3 panel's page of figures, the window title, and the fire within
//! working range that the crafting column greys itself out by.
//!
//! **Not every frame**, and that is the point of gathering them in one
//! place. The title bar is a window-manager call and a fresh `format!`
//! of a dozen numbers; the memory readout walks every section of every
//! loaded chunk twice -- fifty-seven thousand sections a frame at render
//! distance twenty-four, for a number printed once a second. The fire
//! scan is three hundred and forty-three cells through the chunk map for
//! a menu that is usually shut. Each of those was frame time the
//! benchmark charged to the game and a player with F3 open paid for.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::audio::Audio;
use crate::engine::mesher;
use crate::engine::particles::Particles;
use crate::engine::renderer::GraphicsState;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::inventory::Inventory;
use crate::logic::mining::Mining;
use crate::logic::physics::Player;
use crate::logic::entities;
use crate::net::remote_players::RemotePlayers;
use crate::platform;
use crate::settings::ClientSettings;
use crate::ui;
use crate::ui::debug::{DebugStats, FrameInfo};
use crate::ui::input;
use crate::ui::inventory_screen::InventoryScreen;
use crate::{falling_on, fire_within_reach, Arrivals, Detail};
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{block_name, ChunkPos};

/// Gathers this frame's readout, writes the title if one is due, and
/// dumps the console line if the F3 stats are on.
///
/// Returns the page of figures -- `None` when nobody is going to look at
/// it -- and the heat the crafting column should be drawn against.
#[allow(clippy::too_many_arguments)]
pub fn gather(
    now: Instant,
    settings: &ClientSettings,
    window: &dyn platform::Window,
    graphics: &GraphicsState,
    player: &Player,
    chunks: &ChunkManager,
    light: &LightMap,
    mesher: &mesher::Mesher,
    dirty: &VecDeque<ChunkPos>,
    urgent: &VecDeque<ChunkPos>,
    arrivals: &Arrivals,
    remote_players: &RemotePlayers,
    entities: &entities::Entities,
    sky: &Sky,
    worldgen: &primitive_shared::worldgen::WorldGen,
    weather: primitive_shared::weather::Weather,
    world_seed: u32,
    nourishment: f32,
    particles: &Particles,
    audio: &Audio,
    inventory: &Inventory,
    input: &input::InputState,
    chunk_lod: &HashMap<ChunkPos, Detail>,
    underwater: bool,
    health: f32,
    max_health: f32,
    mining: &Mining,
    inventory_screen: &mut InventoryScreen,
    debug_stats: &mut DebugStats,
    last_title_update: &mut Instant,
    menu_title_set: &mut bool,
    counted_bytes: &mut (usize, usize),
    last_heat: &mut primitive_shared::crafting::Heat,
    heat_was_open: &mut bool,
) -> (Option<FrameInfo>, primitive_shared::crafting::Heat) {
    // The title bar is a window-manager call and a fresh
    // format! of a dozen numbers; the readout behind it
    // samples the biome generator. Neither is worth
    // doing every frame -- nobody reads a title bar at
    // 200 Hz -- so both are built only when something is
    // going to look at them.
    const TITLE_INTERVAL: Duration = Duration::from_millis(250);
    let title_due = now.duration_since(*last_title_update) >= TITLE_INTERVAL;
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
    let heat_due = inventory_screen.open || *heat_was_open;
    *heat_was_open = inventory_screen.open;
    if heat_due {
        let heat = fire_within_reach(chunks, player.position.as_vec3());
        *last_heat = heat;
        inventory_screen.set_heat(heat);
    }
    let heat = *last_heat;

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
        *counted_bytes = (chunks.heap_bytes(), light.heap_bytes());
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
        // The size the frame is *drawn* at, not the
        // window: every millisecond on this line is a
        // millisecond per those pixels, and on a phone
        // drawing at seven tenths the two differ by
        // half the area.
        surface: (graphics.render_size().width, graphics.render_size().height),
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
        falling: falling_on(worldgen, sky, weather, player.position).0,
        heat,
        // In the player's language: the one line of this
        // panel a player reads for the game rather than
        // for a bug report.
        biome: ui::names::biome(
            worldgen.biome_at(
                player.position.x.floor() as i32,
                player.position.z.floor() as i32,
            ),
            settings.language,
        ),
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
        cutout_indices: graphics.cutout_indices_last_frame,
        chunks_culled: graphics.chunks_culled_last_frame,
        // What the mesher actually built, counted here
        // because `chunk_lod` is where the answer is: one
        // `Detail` per loaded chunk, holding the level it
        // was meshed at. A walk over a few hundred entries
        // once a frame, and it is the only way from outside
        // a phone to tell a coarse band that moved from one
        // that did not. See `ui::debug::Info::chunk_levels`.
        chunk_levels: chunk_lod.values().fold([0usize; 3], |mut counts, built| {
            counts[(built.level as usize).min(2)] += 1;
            counts
        }),
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
            *last_title_update = now;
            *menu_title_set = false;
        }
        debug_stats.maybe_dump_console(info);
    }
    (info, heat)
}
