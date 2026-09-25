//! **Streaming, rationed.**
//!
//! The world arrives while the player walks, and every part of making it
//! visible -- lighting an arrived chunk, surveying it for the map,
//! handing it to a mesher, copying the finished mesh to the card -- is
//! main-thread work that happens in the middle of a frame. Unbudgeted,
//! one of them lands forty chunks at once and the frame the player is
//! looking at is the one that stutters.
//!
//! So each phase gets a slice of the frame (`streaming_budget`), the
//! slice is a share of the frame the machine is *actually* achieving
//! rather than a fixed figure, and results are handled nearest-the-player
//! first so the chunk someone just edited is never starved behind a
//! hundred chunks of horizon.
//!
//! Two functions rather than one, and the gap between them is not an
//! accident: the sky's own tick sits between them in the frame, because
//! the detail levels are chosen against where the player is standing and
//! the weather has to have been applied before anything is meshed for it.
//! See `run`.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::engine::mesher;
use crate::engine::renderer::GraphicsState;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::map::ExploredMap;
use crate::logic::physics::Player;
use crate::net::network;
use crate::settings::ClientSettings;
use crate::ui::debug::DebugStats;
use crate::{
    collect_worker_results, dispatch_meshing, integrate_chunks, restripe_detail_levels,
    streaming_budget, Arrivals, Detail, MeshQueueSet,
};
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::ClientMessage;
use primitive_shared::types::ChunkPos;

/// Chunks that have arrived, lit and put into the world -- and the map's
/// survey of them, which is a streaming phase with a ration of its own.
#[allow(clippy::too_many_arguments)]
pub fn integrate(
    settings: &ClientSettings,
    dt: f32,
    world_ready: bool,
    net: &network::NetworkHandle,
    arrivals: &mut Arrivals,
    chunks: &mut ChunkManager,
    mesher: &mut mesher::Mesher,
    explored: &mut ExploredMap,
    debug_stats: &mut DebugStats,
) {
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

    integrate_chunks(
        arrivals,
        chunks,
        mesher,
        chunk_ms,
        debug_stats,
        explored,
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
    explored.catch_up(
        chunks,
        Duration::from_secs_f32((chunk_ms * 0.25).clamp(0.2, 1.0) / 1000.0),
    );
    // ...and a mark whose cairn is gone is told to the
    // server, which is where this player's marks live.
    // Only what a survey just looked at, so this is empty
    // on all but a handful of frames in a session.
    for at in explored.take_lost_marks() {
        net.send(ClientMessage::ForgetMark {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
        });
        debug_stats.network_messages_out_this_second += 1;
    }
}

/// Which detail each loaded chunk deserves, what goes to the workers,
/// and what comes back from them.
#[allow(clippy::too_many_arguments)]
pub fn mesh(
    settings: &ClientSettings,
    dt: f32,
    world_ready: bool,
    player: &Player,
    chunks: &mut ChunkManager,
    light: &mut LightMap,
    mesher: &mut mesher::Mesher,
    graphics: &mut GraphicsState,
    urgent: &mut VecDeque<ChunkPos>,
    dirty: &mut VecDeque<ChunkPos>,
    dirty_set: &mut MeshQueueSet,
    chunk_versions: &mut HashMap<ChunkPos, u64>,
    chunk_lod: &mut HashMap<ChunkPos, Detail>,
    lod_scanned_from: &mut Option<(ChunkPos, i32, crate::engine::lod::Quality, (i32, i32))>,
    debug_stats: &mut DebugStats,
) {
    // The same number `integrate` worked out for its own half, from the
    // same pure function rather than carried across: two calls to
    // `streaming_budget` with the same frame cannot disagree, and a
    // budget threaded through as an argument is a budget that can be
    // handed to the wrong phase.
    let mesh_ms = streaming_budget(settings.mesh_budget_ms, dt, world_ready);

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
    if *lod_scanned_from != Some(lod_key) {
        let quality_changed = lod_scanned_from
            .is_some_and(|(_, _, was, _)| was != settings.lod_quality);
        *lod_scanned_from = Some(lod_key);
        restripe_detail_levels(
            quality_changed,
            chunks,
            player_chunk,
            settings.lod_distance_chunks,
            settings.relief_chunks,
            settings.transparent_leaves_chunks,
            chunk_lod,
            chunk_versions,
            dirty,
            dirty_set,
        );
    }
    dispatch_meshing(
        urgent,
        dirty,
        dirty_set,
        chunk_versions,
        mesher,
        chunks,
        light,
        player_chunk,
        settings.lod_distance_chunks,
        settings.lod_quality,
        settings.relief_chunks,
        settings.transparent_leaves_chunks,
        chunk_lod,
        mesh_ms,
        debug_stats,
    );
    collect_worker_results(
        mesher,
        graphics,
        chunks,
        light,
        urgent,
        dirty,
        dirty_set,
        chunk_versions,
        ChunkManager::chunk_for_world_pos(player.position.x, player.position.z),
        // The same budget the dispatch half gets, and
        // for the same reason: both are meshing, and a
        // burst landing in one frame is a frame the
        // player feels. See `streaming_budget`.
        mesh_ms,
        debug_stats,
    );

}
