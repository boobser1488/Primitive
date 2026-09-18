//! Pieces of world standing in the sky past the edge of the streamed disc,
//! photographed through the real passes.
//!
//! ```text
//! GPU_REPRO_DIR=<absolute dir> cargo test -p primitive_client --lib \
//!     what_a_walk_leaves_standing_past_the_edge -- --ignored --nocapture
//! ```
//!
//! Written for a report of trunks, clumps of leaves, flat blue plates and dark
//! specks standing in a ring in the sky beyond where the world ends, with the
//! fog off -- and, with it on, "sometimes I can see chunks appear".
//!
//! **Streamed, not generated in one go**, because what the report shows is a
//! product of timing. `view_distance_repro` meshes a disc that is already
//! there; this runs the frame's own streaming functions in the frame's own
//! order -- `restripe_detail_levels`, `dispatch_meshing`, the mesh and light
//! arms of `collect_worker_results`, `queue_settled`, the unload -- on the
//! real mesher threads, against a generator that answers requests at the
//! singleplayer server's rate, while the player walks. Only the renderer is
//! replaced, by two maps of what it would be holding.
//!
//! **One walk, two pictures.** The only place the old frame and the new one
//! differ is what the mesh arm does with a result whose chunk is no longer
//! loaded (`landing`): the new one drops it, the old one uploaded it when its
//! version matched. So `after` is what the renderer holds now, and `before` is
//! that plus every mesh the old arm would have uploaded and nothing would ever
//! have taken down. Same meshes, same camera, same shader.
//!
//! The player's settings (render 24, detail 22, fog from 0.75 of the reach,
//! msaa 4) in the world the report came from: seed 1337 laid in the tropics,
//! from where the player stood. Walks east at eight blocks a second, then
//! turns round to look at what it left behind, from sixty blocks up.

use super::*;
use crate::engine::mesh::MeshBuffers;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::{ChunkManager, NEIGHBOUR_OFFSETS};
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::packed::PackedChunk;
use primitive_shared::worldgen::{Preset, WorldGen, Zone};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SEED: u32 = 1337;
const ZONE: Zone = Zone::Tropics;
/// Where the player of the world `24` stood when the report came in.
const START: (f32, f32) = (-493.0, -591.0);
const SIZE: (u32, u32) = (1280, 720);
const SPEED: f32 = 8.0;
const WALK_SECONDS: f32 = 30.0;

fn settings() -> ClientSettings {
    let mut settings = ClientSettings {
        render_distance_chunks: 24,
        lod_distance_chunks: 22,
        fog_start_share: 0.75,
        fov_degrees: 90.0,
        anisotropy: 16,
        msaa: 4,
        detail_distance: 1.0,
        transparent_leaves_chunks: crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE,
        ..ClientSettings::default()
    };
    settings.sanitize();
    settings
}

/// What a walk leaves the renderer holding.
struct Walked {
    /// Every mesh the frame as it is now would be drawing.
    after: HashMap<ChunkPos, MeshBuffers>,
    /// Meshes the old mesh arm would also have put up, for chunks that were
    /// already unloaded when they landed.
    leaked: HashMap<ChunkPos, MeshBuffers>,
    player: (f32, f32),
}

fn walk(layers: crate::engine::texture::FaceLayers, settings: &ClientSettings) -> Walked {
    let radius = settings.render_distance_chunks;
    let lod = settings.lod_distance_chunks;

    // ---- a generator that answers requests at singleplayer's rate ----
    let player_at = Arc::new(Mutex::new(START));
    let (req_tx, req_rx) = mpsc::channel::<Vec<ChunkPos>>();
    let (chunk_tx, chunk_rx) = mpsc::channel::<PackedChunk>();
    {
        let player_at = Arc::clone(&player_at);
        std::thread::spawn(move || {
            let generator = WorldGen::with_zone(SEED, Preset::Normal, ZONE);
            let mut waiting: Vec<ChunkPos> = Vec::new();
            // The client asks again after three seconds; the server answers
            // from its cache, so an ask already queued is not a second job.
            let mut queued: HashSet<ChunkPos> = HashSet::new();
            loop {
                let tick = Instant::now();
                loop {
                    match req_rx.try_recv() {
                        Ok(list) => waiting.extend(list.into_iter().filter(|p| queued.insert(*p))),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                let (px, pz) = *player_at.lock().unwrap();
                let at = ChunkPos::from_world(px, pz);
                waiting.sort_by_key(|p| {
                    let (dx, dz) = ((p.x - at.x) as i64, (p.z - at.z) as i64);
                    std::cmp::Reverse(dx * dx + dz * dz)
                });
                // `singleplayer_server`: 64 a tick at 20 ticks.
                let batch: Vec<ChunkPos> = waiting.split_off(waiting.len() - waiting.len().min(64));
                let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).max(2) / 2;
                let per = batch.len().div_ceil(threads).max(1);
                let generated: Vec<PackedChunk> = std::thread::scope(|scope| {
                    let generator = &generator;
                    let handles: Vec<_> = batch
                        .chunks(per)
                        .map(|part| {
                            scope.spawn(move || {
                                part.iter().map(|&p| PackedChunk::pack(&generator.generate_chunk(p))).collect::<Vec<_>>()
                            })
                        })
                        .collect();
                    handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
                });
                for chunk in generated {
                    queued.remove(&chunk.pos);
                    if chunk_tx.send(chunk).is_err() {
                        return;
                    }
                }
                if let Some(rest) = Duration::from_millis(50).checked_sub(tick.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        });
    }

    // ---- the client's streaming state, as `run` holds it ----
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).max(1));
    let mut mesher = crate::engine::mesher::Mesher::new(layers, workers);
    mesher.set_world(WorldGen::with_zone(SEED, Preset::Normal, ZONE));
    let mut chunks = ChunkManager::new(radius);
    let mut light = LightMap::new();
    let mut arrivals: VecDeque<PackedChunk> = VecDeque::new();
    let (mut urgent, mut dirty) = (VecDeque::new(), VecDeque::new());
    let mut dirty_set: crate::MeshQueueSet = HashMap::new();
    let mut versions: HashMap<ChunkPos, u64> = HashMap::new();
    let mut chunk_lod: HashMap<ChunkPos, crate::Detail> = HashMap::new();
    let mut lod_scanned_from: Option<ChunkPos> = None;
    let mut stats = crate::ui::debug::DebugStats::default();
    let mut after: HashMap<ChunkPos, MeshBuffers> = HashMap::new();
    let mut leaked: HashMap<ChunkPos, MeshBuffers> = HashMap::new();

    let disc = {
        let probe = ChunkManager::new(radius);
        (-radius..=radius)
            .flat_map(|dx| (-radius..=radius).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| probe.inside(dx, dz))
            .count()
    };
    let (mut x, z) = START;
    let began = Instant::now();
    let mut last_frame = Instant::now();
    let mut set_off: Option<Instant> = None;
    loop {
        let frame_began = Instant::now();
        // Sets off once the whole disc is in, lit and meshed: the report is
        // about walking through a world, not about one arriving.
        let settled = chunks.loaded_count() >= disc
            && chunks.pending_count() == 0
            && arrivals.is_empty()
            && mesher.in_flight() == 0
            && mesher.lighting_in_flight() == 0
            && dirty.is_empty()
            && urgent.is_empty();
        if set_off.is_none() && (settled || began.elapsed() > Duration::from_secs(240)) {
            println!("settled after {:.1}s ({} of {disc} chunks); walking east", began.elapsed().as_secs_f32(), chunks.loaded_count());
            set_off = Some(Instant::now());
        }
        // Wall time: the frame here is slower than the game's, and a walk
        // timed in frames would be a slow walk, which never races anything.
        let dt = last_frame.elapsed().as_secs_f32().min(0.1);
        last_frame = Instant::now();
        if let Some(from) = set_off {
            if from.elapsed().as_secs_f32() > WALK_SECONDS {
                break;
            }
            x += SPEED * dt;
        }
        *player_at.lock().unwrap() = (x, z);
        let player_chunk = ChunkManager::chunk_for_world_pos(x, z);

        // `drain_network`
        while let Ok(chunk) = chunk_rx.try_recv() {
            chunks.note_arrival(chunk.pos);
            arrivals.push_back(chunk);
        }
        // `integrate_chunks`, at `chunk_budget_ms`
        let started = Instant::now();
        while let Some(chunk) = arrivals.pop_front() {
            let pos = chunk.pos;
            let shared = chunks.insert(chunk);
            mesher.submit_lighting(pos, shared);
            if started.elapsed() >= Duration::from_millis(3) {
                break;
            }
        }
        if lod_scanned_from != Some(player_chunk) {
            lod_scanned_from = Some(player_chunk);
            crate::restripe_detail_levels(false, &chunks, player_chunk, lod, settings.relief_chunks, settings.transparent_leaves_chunks, &mut chunk_lod, &mut versions, &mut dirty, &mut dirty_set);
        }
        crate::dispatch_meshing(
            &mut urgent,
            &mut dirty,
            &mut dirty_set,
            &versions,
            &mut mesher,
            &chunks,
            &light,
            player_chunk,
            lod,
            settings.lod_quality,
            settings.relief_chunks,
            settings.transparent_leaves_chunks,
            &mut chunk_lod,
            settings.mesh_budget_ms,
            &mut stats,
        );
        // `collect_worker_results`, with the renderer replaced by the maps.
        {
            let started = Instant::now();
            mesher.drain(player_chunk);
            let mut newly_lit: Vec<ChunkPos> = Vec::new();
            while let Some(finished) = mesher.take_pending() {
                match finished {
                    crate::engine::mesher::Finished::Mesh { pos, version, buffers, cache } => {
                        let current = versions.get(&pos).copied().unwrap_or(0) == version;
                        match crate::landing(&chunks, &versions, pos, version) {
                            crate::Landing::Upload => {
                                leaked.remove(&pos);
                                if buffers.indices.is_empty() {
                                    after.remove(&pos);
                                } else {
                                    after.insert(pos, *buffers);
                                }
                                drop(cache);
                            }
                            crate::Landing::Stale => {
                                crate::mark_urgent(&mut urgent, &mut dirty_set, pos);
                                mesher.recycle(cache, buffers);
                            }
                            crate::Landing::Evicted => {
                                // The old arm asked only the version.
                                if current && !buffers.indices.is_empty() {
                                    leaked.insert(pos, *buffers);
                                    drop(cache);
                                } else {
                                    mesher.recycle(cache, buffers);
                                }
                            }
                        }
                    }
                    crate::engine::mesher::Finished::Light { pos, data } => {
                        if chunks.is_loaded(pos) {
                            newly_lit.extend(light.insert_packed(&chunks, pos, *data));
                            newly_lit.push(pos);
                            for (dx, dz) in NEIGHBOUR_OFFSETS {
                                newly_lit.push(ChunkPos::new(pos.x + dx, pos.z + dz));
                            }
                        }
                    }
                }
                if started.elapsed().as_secs_f32() * 1000.0 >= settings.mesh_budget_ms {
                    break;
                }
            }
            crate::queue_settled(newly_lit, &chunks, &light, &mut dirty, &mut dirty_set, player_chunk);
        }
        // `request_and_unload`: `drop_chunk_mesh` takes down whatever is up
        // for the position, under either arm.
        {
            let (to_request, to_unload) = chunks.update(player_chunk, Instant::now());
            if !to_request.is_empty() {
                let _ = req_tx.send(to_request);
            }
            for pos in to_unload {
                chunks.unload(pos);
                light.unload_chunk(pos);
                after.remove(&pos);
                leaked.remove(&pos);
            }
        }
        if let Some(rest) = Duration::from_micros(13_333).checked_sub(frame_began.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Walked { after, leaked, player: (x, z) }
}

/// Pixels that differ by more than `threshold` over the three channels, and
/// `a` with them painted magenta.
fn difference(a: &image::RgbaImage, b: &image::RgbaImage, threshold: i32) -> (u32, image::RgbaImage) {
    let mut marked = a.clone();
    let mut changed = 0;
    for (x, y, p) in a.enumerate_pixels() {
        let q = b.get_pixel(x, y);
        if (0..3).map(|i| (p[i] as i32 - q[i] as i32).abs()).sum::<i32>() > threshold {
            changed += 1;
            marked.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        }
    }
    (changed, marked)
}

#[test]
#[ignore = "a tool: needs a GPU and streams a world for a minute or two; photographs what a walk leaves past the edge"]
fn what_a_walk_leaves_standing_past_the_edge() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");

    let walked = walk(textures.face_layers(), &settings);
    let player_chunk = ChunkPos::from_world(walked.player.0, walked.player.1);
    println!(
        "walked {WALK_SECONDS}s east at {SPEED} blocks a second: the renderer holds {} meshes; the old arm would also hold {} for chunks already unloaded",
        walked.after.len(),
        walked.leaked.len()
    );
    let mut aim = Vec3::ZERO;
    for (pos, mesh) in &walked.leaked {
        let g = mesh.solid_groups;
        let centre = Vec3::new((pos.x * 16 + 8) as f32, 0.0, (pos.z * 16 + 8) as f32);
        aim += centre;
        println!(
            "  {pos:?}: {:.1} chunks from the player, {} triangles ({} of them ground facing up, {} leaves, {} branch and model)",
            crate::chunk_distance(*pos, player_chunk),
            mesh.indices.len() / 3,
            (g[1] - g[0]) / 3,
            (mesh.leaf_end - mesh.solid_index_count) / 3,
            g[0] / 3,
        );
    }
    if walked.leaked.is_empty() {
        println!("nothing leaked on this walk; the pictures would be the same and are not taken");
        return;
    }
    aim /= walked.leaked.len() as f32;

    // From sixty blocks over the sea, facing what was left behind.
    let eye = Vec3::new(walked.player.0, primitive_shared::worldgen::SEA_LEVEL as f32 + 60.0, walked.player.1);
    let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
    camera.yaw = (aim.z - eye.z).atan2(aim.x - eye.x);
    camera.pitch = (-6.0f32).to_radians();
    camera.fov_y_radians = settings.fov_degrees.to_radians();
    let sky = Sky::new(0.42, 900.0);

    let shader = include_str!("shader.wgsl");
    let fog_switch = "if (globals.extra.w > 0.5) {";
    assert!(shader.contains(fog_switch), "shader.wgsl no longer switches its fog on `extra.w`");
    // The fog toggle, as the player pressed it: every place the shader asks
    // `extra.w` (the terrain and the models both do) reads it as off.
    let nofog = shader.replace(fog_switch, "if (globals.extra.w > 1.5) {");
    let reach = ChunkManager::reach_blocks(settings.render_distance_chunks);

    let mut meshes: Vec<(ChunkPos, MeshBuffers)> = walked.after.into_iter().collect();
    let draw = |meshes: &[(ChunkPos, MeshBuffers)], source: &str| {
        super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, source, Some(reach), None, false, settings.msaa,
        )
    };
    let after = draw(&meshes, &nofog);
    meshes.extend(walked.leaked);
    let before = draw(&meshes, &nofog);
    let before_fog = draw(&meshes, shader);
    let (changed, mask) = difference(&before, &after, 24);

    let save = |picture: &image::RgbaImage, name: &str| {
        let path = format!("{out}/{name}.png");
        picture.save(&path).expect("write png");
        println!("  {path}");
    };
    save(&before, "before_nofog");
    save(&after, "after_nofog");
    save(&mask, "before_minus_after");
    save(&before_fog, "before_fog");
    println!(
        "{changed} pixels of {} ({:.2}%) differ between before and after, all of them the meshes past the edge",
        SIZE.0 * SIZE.1,
        changed as f32 * 100.0 / (SIZE.0 * SIZE.1) as f32
    );
}
