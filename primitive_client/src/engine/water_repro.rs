//! Water in a generated world, photographed through the real passes: the
//! sea from its shore and from under its lid, a river, the shallows, and a
//! lake in a cave.
//!
//! ```text
//! GPU_REPRO_DIR=C:/abs/shots WATER_TAG=before cargo test -p primitive_client --lib \
//!     what_water_looks_like_in_a_generated_world -- --ignored --nocapture
//! ```
//!
//! Written for a report of two sentences and no screenshot: "рендер воды
//! сломан полностью", and before it "под водой полосы". `water_through_the_
//! real_passes` photographs a hand-built pond, and a hand-built pond is
//! full cells at one level with nothing flowing in it; the generator's
//! water is not that any more -- rivers carry a current, caves hold lakes
//! under a roof, and the sea is deeper -- so this goes where the generator
//! put the water and stands a player's eye there.
//!
//! **Seats are found, not typed.** Each place is searched for round the
//! spawn of the seed, and the tool prints where it stood, so a picture can
//! be taken again from the same eye after a change.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory.

use super::offscreen_repro::draw_scene;
use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{is_liquid, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

const SIZE: (u32, u32) = (1280, 720);

fn seed() -> u32 {
    std::env::var("WATER_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1337)
}

/// The player's settings file, sanitised as the game reads it, with the
/// report's lens and filtering on top.
fn settings() -> ClientSettings {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../client_settings.toml");
    let mut settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<ClientSettings>(&text).ok())
        .unwrap_or_default();
    settings.fov_degrees = 95.0;
    settings.anisotropy = 16;
    settings.sanitize();
    settings
}

/// A square of chunks round `centre`, generated, lit and meshed.
fn patch(world: &WorldGen, centre: ChunkPos, radius: i32, layers: &crate::engine::texture::FaceLayers) -> Vec<(ChunkPos, MeshBuffers)> {
    let mut positions = Vec::new();
    for dz in -(radius + 1)..=(radius + 1) {
        for dx in -(radius + 1)..=(radius + 1) {
            positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
        }
    }
    let generated: Vec<Chunk> = std::thread::scope(|scope| {
        let handles: Vec<_> = positions
            .chunks(positions.len().div_ceil(8))
            .map(|batch| scope.spawn(move || batch.iter().map(|p| world.generate_chunk(*p)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let mut chunks = ChunkManager::new(radius + 2);
    for chunk in generated {
        chunks.insert(chunk);
    }
    let mut light = LightMap::new();
    for pos in &positions {
        light.load_chunk(&chunks, *pos);
    }
    let mut out = Vec::new();
    let mut cache = Box::<Neighbourhood>::default();
    for pos in positions {
        if (pos.x - centre.x).abs() > radius || (pos.z - centre.z).abs() > radius {
            continue;
        }
        cache.fill(pos, &chunks, &light);
        // The detail level `dispatch_meshing` picks, so the far water is
        // the far water the game draws.
        let settings = settings();
        let (dx, dz) = ((pos.x - centre.x) as f32, (pos.z - centre.z) as f32);
        let level = crate::engine::lod::level_at(
            (dx * dx + dz * dz).sqrt(),
            crate::engine::lod::band_start(settings.lod_distance_chunks, cache.ceiling()),
            0,
        );
        crate::engine::lod::coarsen(&mut cache, level, settings.lod_quality);
        let mut buffers = MeshBuffers::default();
        build_mesh(pos, &cache, layers, world, &mut buffers);
        out.push((pos, buffers));
    }
    out
}

/// The kinds of water looked for, by what a column of one chunk holds.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    /// Twelve deep at least.
    Sea,
    /// One or two deep, beside land.
    Shallows,
    /// A surface above the sea's level: a river or a lake in the hills.
    River,
    /// Water with rock over it well under the ground.
    Cave,
}

/// The first column of each kind found in the chunks round spawn, as
/// (x, top of the water, z).
fn find(world: &WorldGen, want: Place) -> Option<(i32, i32, i32)> {
    let (sx, sz) = world.spawn_column();
    let spawn = ChunkPos::from_global(sx, sz).0;
    for ring in 0..40i32 {
        let mut ring_positions = Vec::new();
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() == ring || dz.abs() == ring {
                    ring_positions.push(ChunkPos::new(spawn.x + dx, spawn.z + dz));
                }
            }
        }
        // Cheap tests on the height map first, so the sea is not paid for
        // by generating chunks.
        for pos in ring_positions {
            let (mx, mz) = (pos.x * 16 + 8, pos.z * 16 + 8);
            let h = world.height_at(mx, mz);
            let plausible = match want {
                Place::Sea => h <= SEA_LEVEL - 12,
                Place::Shallows => (SEA_LEVEL - 2..SEA_LEVEL).contains(&h),
                Place::River => h > SEA_LEVEL + 2,
                Place::Cave => h > SEA_LEVEL,
            };
            if !plausible {
                continue;
            }
            if want == Place::Sea {
                let deep = |x: i32, z: i32| world.height_at(x, z) <= SEA_LEVEL - 12;
                if [(24, 0), (-24, 0), (0, 24), (0, -24)].iter().all(|(dx, dz)| deep(mx + dx, mz + dz)) {
                    return Some((mx, SEA_LEVEL - 1, mz));
                }
                continue;
            }
            if want == Place::Shallows {
                return Some((mx, SEA_LEVEL - 1, mz));
            }
            let chunk = world.generate_chunk(pos);
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let surface = world.height_at(pos.x * 16 + x as i32, pos.z * 16 + z as i32);
                    for y in (1..CHUNK_SIZE_Y - 1).rev() {
                        let id = chunk.get(x, y, z);
                        if !is_liquid(id) || is_liquid(chunk.get(x, y + 1, z)) {
                            continue;
                        }
                        let (gx, gz) = (pos.x * 16 + x as i32, pos.z * 16 + z as i32);
                        let y = y as i32;
                        let hit = match want {
                            Place::River => y >= SEA_LEVEL && chunk.get(x, y as usize + 1, z) == BLOCK_AIR && y >= surface - 2,
                            Place::Cave => y < surface - 12 && chunk.get(x, y as usize + 1, z) == BLOCK_AIR && x > 3 && x < 12 && z > 3 && z < 12,
                            _ => false,
                        };
                        if hit {
                            return Some((gx, y, gz));
                        }
                    }
                }
            }
        }
    }
    None
}

#[test]
#[ignore = "a tool: needs a GPU; photographs generated water from above, beside and below"]
fn what_water_looks_like_in_a_generated_world() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("WATER_TAG").unwrap_or_else(|_| "now".to_string());
    let only = std::env::var("WATER_ONLY").ok();
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let hour = std::env::var("WATER_HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(0.40);
    let sky = Sky::new(hour, 900.0);
    let world = WorldGen::new(seed());

    for place in [Place::Sea, Place::Shallows, Place::River, Place::Cave] {
        let name = format!("{place:?}").to_lowercase();
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let Some((x, top, z)) = find(&world, place) else {
            println!("{name}: none found round spawn");
            continue;
        };
        println!("{name}: water surface at ({x}, {top}, {z})");
        {
            // Water with air beside it at its own level: water that runs
            // the moment anything wakes it.
            let centre = ChunkPos::from_global(x, z).0;
            let mut open = 0;
            let mut cells = 0;
            let chunks: std::collections::HashMap<(i32, i32), Chunk> = (-3..=3)
                .flat_map(|dz| (-3..=3).map(move |dx| (dx, dz)))
                .map(|(dx, dz)| {
                    let p = ChunkPos::new(centre.x + dx, centre.z + dz);
                    ((p.x, p.z), world.generate_chunk(p))
                })
                .collect();
            let at = |gx: i32, y: usize, gz: i32| {
                let (p, lx, lz) = ChunkPos::from_global(gx, gz);
                chunks.get(&(p.x, p.z)).map(|c| c.get(lx, y, lz))
            };
            let mut shown = 0;
            for gz in (centre.z - 2) * 16..(centre.z + 3) * 16 {
                for gx in (centre.x - 2) * 16..(centre.x + 3) * 16 {
                    for y in 1..CHUNK_SIZE_Y - 1 {
                        if !at(gx, y, gz).is_some_and(is_liquid) {
                            continue;
                        }
                        cells += 1;
                        if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| at(gx + dx, y, gz + dz) == Some(BLOCK_AIR))
                            || at(gx, y - 1, gz) == Some(BLOCK_AIR)
                        {
                            open += 1;
                            if shown < 5 {
                                println!("  water open to air at ({gx}, {y}, {gz})");
                                shown += 1;
                            }
                        }
                    }
                }
            }
            println!("  {open} of {cells} water cells have air beside or below them");
        }
        let radius = std::env::var("WATER_RADIUS").ok().and_then(|r| r.parse().ok()).unwrap_or(2);
        let meshes = patch(&world, ChunkPos::from_global(x, z).0, radius, &layers);
        {
            // Blended quads that share a plane and overlap: every pixel of
            // the overlap is blended twice.
            let mut planes: std::collections::HashMap<(u8, i64), Vec<[f32; 4]>> = Default::default();
            let mut low_tops = 0;
            for (pos, mesh) in &meshes {
                for quad in mesh.indices[mesh.sprite_end as usize..].chunks(6) {
                    let mut corners: Vec<u32> = quad.to_vec();
                    corners.sort();
                    corners.dedup();
                    let p: Vec<[f32; 3]> = corners
                        .iter()
                        .map(|&i| {
                            let v = mesh.vertices[i as usize].position;
                            [v[0] + pos.x as f32 * 16.0, v[1], v[2] + pos.z as f32 * 16.0]
                        })
                        .collect();
                    let span = |a: usize| {
                        let lo = p.iter().map(|q| q[a]).fold(f32::MAX, f32::min);
                        let hi = p.iter().map(|q| q[a]).fold(f32::MIN, f32::max);
                        (lo, hi)
                    };
                    let (sx, sy, sz) = (span(0), span(1), span(2));
                    let (axis, at, rect) = if sx.1 - sx.0 < 1e-4 {
                        (0u8, sx.0, [sy.0, sy.1, sz.0, sz.1])
                    } else if sz.1 - sz.0 < 1e-4 {
                        (2u8, sz.0, [sx.0, sx.1, sy.0, sy.1])
                    } else {
                        if sy.1 - sy.0 < 1e-4 && (sy.0.fract() - 0.88).abs() > 0.01 {
                            low_tops += 1;
                        }
                        (1u8, sy.0, [sx.0, sx.1, sz.0, sz.1])
                    };
                    planes.entry((axis, (at * 1000.0).round() as i64)).or_default().push(rect);
                }
            }
            let mut overlaps = 0;
            for rects in planes.values() {
                for i in 0..rects.len() {
                    for j in i + 1..rects.len() {
                        let (a, b) = (rects[i], rects[j]);
                        if a[0].max(b[0]) + 1e-3 < a[1].min(b[1]) && a[2].max(b[2]) + 1e-3 < a[3].min(b[3]) {
                            if overlaps < 10 {
                                println!("  overlap {a:?} {b:?}");
                            }
                            overlaps += 1;
                        }
                    }
                }
            }
            println!("  {overlaps} overlapping blended pairs; {low_tops} flat water tops not at .88");
        }
        if let Ok(window) = std::env::var("WATER_DUMP") {
            // "x0,x1,z0,z1": every blended quad whose corner falls in it.
            let w: Vec<f32> = window.split(',').map(|s| s.parse().unwrap()).collect();
            for (pos, mesh) in &meshes {
                for quad in mesh.indices[mesh.sprite_end as usize..].chunks(6) {
                    let mut corners: Vec<u32> = quad.to_vec();
                    corners.sort();
                    corners.dedup();
                    let verts: Vec<_> = corners.iter().map(|&i| mesh.vertices[i as usize]).collect();
                    let world_of = |p: [f32; 3]| (p[0] + pos.x as f32 * 16.0, p[1], p[2] + pos.z as f32 * 16.0);
                    let (x0, _, z0) = world_of(verts[0].position);
                    if x0 < w[0] || x0 > w[1] || z0 < w[2] || z0 > w[3] {
                        continue;
                    }
                    let described: Vec<String> = verts
                        .iter()
                        .map(|v| {
                            let (a, b, c) = world_of(v.position);
                            format!("({a:.2},{b:.2},{c:.2}) f{} d{}", (v.light() >> 10) & 7, v.tint())
                        })
                        .collect();
                    println!("  chunk {:?}: {}", (pos.x, pos.z), described.join("  "));
                }
            }
        }
        let (fx, fy, fz) = (x as f32 + 0.5, top as f32, z as f32 + 0.5);
        // (seat, eye, yaw, pitch, under water)
        let mut seats: Vec<(String, Vec3, f32, f32, bool)> = Vec::new();
        for yaw in [0.0f32, 90.0, 180.0, 270.0] {
            let back = Vec3::new(-yaw.to_radians().cos(), 0.0, -yaw.to_radians().sin()) * 6.0;
            seats.push((format!("above_{yaw:.0}"), Vec3::new(fx, fy + 2.6, fz) + back, yaw, -22.0, false));
        }
        let lid = Vec3::new(fx, fy + 0.3, fz);
        for yaw in [0.0f32, 90.0, 180.0, 270.0] {
            seats.push((format!("under_{yaw:.0}"), lid, yaw, 0.0, true));
        }
        seats.push(("under_up".into(), lid, 30.0, 40.0, true));
        seats.push(("under_down".into(), lid, 30.0, -35.0, true));
        for (seat, eye, yaw, pitch, submerged) in seats {
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = yaw.to_radians();
            camera.pitch = pitch.to_radians();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let png = draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
                include_str!("shader.wgsl"), None, None, submerged, settings.msaa.max(1),
            );
            let path = format!("{out}/gen_{tag}_{name}_{seat}.png");
            png.save(&path).expect("write png");
            println!("  {seat:>10} -> {path}");
        }
    }
}

/// A world of blocks the server's own water simulation can run over.
struct Sandbox {
    blocks: std::sync::Mutex<std::collections::HashMap<(i32, i32, i32), primitive_shared::types::BlockId>>,
    span: i32,
}

impl primitive_server::logic::falling::BlockWorld for Sandbox {
    fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<primitive_shared::types::BlockId> {
        if gx < 0 || gz < 0 || gx >= self.span * 16 || gz >= self.span * 16 || !(0..CHUNK_SIZE_Y as i32).contains(&gy) {
            return None;
        }
        Some(*self.blocks.lock().unwrap().get(&(gx, gy, gz)).unwrap_or(&BLOCK_AIR))
    }
    fn set(&self, gx: i32, gy: i32, gz: i32, block: primitive_shared::types::BlockId) {
        self.blocks.lock().unwrap().insert((gx, gy, gz), block);
    }
}

/// **Water the player has set running**, through the server's own flow rule
/// and then the real passes: a lake on a shelf, a notch cut through its lip,
/// the fall down the cliff and the spill across the ground below into a
/// basin. Photographed part way and later, from above, beside the fall and
/// under both waters.
///
/// ```text
/// GPU_REPRO_DIR=C:/abs/shots WATER_TAG=before cargo test -p primitive_client --lib \
///     what_running_water_looks_like -- --ignored --nocapture
/// ```
///
/// `WATER_SCENE=dig` digs a pit in the lake's bed instead of cutting its lip,
/// the way a player mines under water: the lake settles into a shallow crater
/// of seven-, six-, five-eighths cells that is a rest state of the flow rule
/// (`fluid::level_transfer` moves nothing across a difference of one).
///
/// **What it found.** The fall down the cliff is held by the simulation as
/// drops in every other cell, so with depths drawn as heights it is a ladder
/// of thin slabs hanging in the air -- the column print says which cells.
/// Fixed in the fall rule, not here: a fed cell keeps its last eighth
/// (`fluid::fall_keeping`), and the column prints unbroken now.
///
/// Generated water is sealed and still, so it never shows what depths drawn
/// as heights (`fluid::surface_height`, the corner averaging in
/// `build_mesh`) do to a picture; only water that moves does.
#[test]
#[ignore = "a tool: needs a GPU; runs the server's water over a cut lake and photographs it"]
fn what_running_water_looks_like() {
    use primitive_server::logic::simulation::CellMechanic;
    use primitive_shared::types::{BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("WATER_TAG").unwrap_or_else(|_| "now".to_string());
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.40, 900.0);

    const SPAN: i32 = 4;
    let world = Sandbox { blocks: Default::default(), span: SPAN };
    {
        let mut blocks = world.blocks.lock().unwrap();
        for gx in 0..SPAN * 16 {
            for gz in 0..SPAN * 16 {
                // A shelf to x = 23 whose top is at 30, the lowland at 20.
                let top = if gx < 24 { 30 } else { 20 };
                for y in 0..=top {
                    blocks.insert((gx, y, gz), if y == top { BLOCK_GRASS } else { BLOCK_STONE });
                }
                // The lake on the shelf: bed at 26, full to 30.
                if (4..=20).contains(&gx) && (16..=40).contains(&gz) {
                    for y in 27..=30 {
                        blocks.insert((gx, y, gz), BLOCK_WATER);
                    }
                }
                // A basin in the lowland, bed at 16, empty.
                if (34..=54).contains(&gx) && (12..=44).contains(&gz) {
                    for y in 17..=20 {
                        blocks.remove(&(gx, y, gz));
                    }
                }
            }
        }
    }
    let mut sim = primitive_server::logic::water::Water::new();
    let dig = std::env::var("WATER_SCENE").as_deref() == Ok("dig");
    if dig {
        // A pit dug in the lake's bed, the way a player mines under water.
        for gx in 11..=13 {
            for gz in 27..=29 {
                for y in 22..=26 {
                    world.blocks.lock().unwrap().remove(&(gx, y, gz));
                    sim.on_block_changed(gx, y, gz);
                }
            }
        }
    } else {
        // The cut: two blocks deep through the lip, from the lake to the cliff.
        for gx in 21..=23 {
            for gz in 27..=28 {
                for y in 29..=30 {
                    world.blocks.lock().unwrap().remove(&(gx, y, gz));
                    sim.on_block_changed(gx, y, gz);
                }
            }
        }
    }

    let photograph = |stage: &str| {
        let mut chunks = ChunkManager::new(SPAN + 2);
        let blocks = world.blocks.lock().unwrap();
        let (mut partial, mut total) = (0, 0);
        for cz in 0..SPAN {
            for cx in 0..SPAN {
                let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..CHUNK_SIZE_Y {
                            if let Some(&id) = blocks.get(&(cx * 16 + x as i32, y as i32, cz * 16 + z as i32)) {
                                data[Chunk::index(x, y, z)] = id;
                                if is_liquid(id) {
                                    total += 1;
                                    partial += (primitive_shared::fluid::depth(id) < 8) as usize;
                                }
                            }
                        }
                    }
                }
                chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
            }
        }
        for gx in 23..=25 {
            let column: Vec<String> = (19..=31)
                .map(|y| {
                    let id = blocks.get(&(gx, y, 27)).copied().unwrap_or(BLOCK_AIR);
                    if is_liquid(id) {
                        format!("{y}:w{}", primitive_shared::fluid::depth(id))
                    } else {
                        format!("{y}:{}", primitive_shared::types::block_name(id))
                    }
                })
                .collect();
            println!("  column x={gx} z=27: {}", column.join(" "));
        }
        drop(blocks);
        println!("{stage}: {total} water cells, {partial} of them partial");
        let mut light = LightMap::new();
        for cz in 0..SPAN {
            for cx in 0..SPAN {
                light.load_chunk(&chunks, ChunkPos::new(cx, cz));
            }
        }
        let mut cache = Box::<Neighbourhood>::default();
        let generator = WorldGen::new(0);
        let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..SPAN * SPAN)
            .map(|i| {
                let pos = ChunkPos::new(i % SPAN, i / SPAN);
                cache.fill(pos, &chunks, &light);
                let mut buffers = MeshBuffers::default();
                build_mesh(pos, &cache, &layers, &generator, &mut buffers);
                (pos, buffers)
            })
            .collect();
        // (seat, eye, yaw, pitch, under water)
        let mut surface = std::collections::BTreeMap::<u8, usize>::new();
        {
            let blocks = world.blocks.lock().unwrap();
            for gx in 4..=20 {
                for gz in 16..=40 {
                    if let Some(&id) = blocks.get(&(gx, 30, gz)) {
                        *surface.entry(primitive_shared::fluid::depth(id)).or_default() += 1;
                    } else {
                        *surface.entry(0).or_default() += 1;
                    }
                }
            }
        }
        println!("  the lake's top layer by depth in eighths: {surface:?}");
        let seats: [(&str, Vec3, f32, f32, bool); 11] = [
            ("crater", Vec3::new(5.5, 32.0, 28.5), 0.0, -22.0, false),
            ("crater_under", Vec3::new(9.5, 29.0, 24.5), 45.0, 25.0, true),
            ("lake_shore", Vec3::new(2.5, 32.4, 20.5), 45.0, -18.0, false),
            ("lake_low", Vec3::new(3.5, 31.3, 28.5), 0.0, -6.0, false),
            ("overview", Vec3::new(10.0, 40.0, 10.0), 45.0, -35.0, false),
            ("fall_front", Vec3::new(34.5, 23.5, 27.5), 180.0, 10.0, false),
            ("fall_side", Vec3::new(27.5, 24.5, 18.5), 90.0, 0.0, false),
            ("spill_low", Vec3::new(30.5, 22.2, 34.5), 0.0, -20.0, false),
            ("lake_under", Vec3::new(12.5, 28.3, 28.0), 0.0, 5.0, true),
            ("lake_under_up", Vec3::new(14.5, 27.6, 28.0), 0.0, 45.0, true),
            ("basin_under", Vec3::new(40.5, 17.8, 28.0), 180.0, 10.0, true),
        ];
        for (seat, eye, yaw, pitch, submerged) in seats {
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = yaw.to_radians();
            camera.pitch = pitch.to_radians();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let png = draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
                include_str!("shader.wgsl"), None, None, submerged, settings.msaa.max(1),
            );
            let path = format!("{out}/run_{tag}_{}{stage}_{seat}.png", if dig { "dig_" } else { "" });
            png.save(&path).expect("write png");
            println!("  {seat:>14} -> {path}");
        }
    };

    for step in 0..400 {
        sim.step(&world, 1.0, 4096);
        if step == 40 {
            photograph("running");
        }
    }
    println!("pending after 400 steps: {}", sim.pending());
    photograph("later");
}

/// **Ice on a pond**, the way `water::Frost` leaves it: the top layer of
/// full water cells turned to ice over half the pond, a hole kept open in
/// it, and the other half open water. Photographed from under the lid, from
/// under water at the lid's edge, and from the open water beside it.
///
/// ```text
/// GPU_REPRO_DIR=C:/abs/shots WATER_TAG=before cargo test -p primitive_client --lib \
///     what_ice_on_water_looks_like -- --ignored --nocapture
/// ```
///
/// Written for "лёд в воде выглядит сломанным, а именно его подводные
/// грани". What it showed is on `mesh::face_visible` (water against ice) and
/// `fluid::is_lid` (the slot under the ice).
#[test]
#[ignore = "a tool: needs a GPU; photographs a half-frozen pond"]
fn what_ice_on_water_looks_like() {
    use primitive_shared::types::{BLOCK_GRASS, BLOCK_ICE, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("WATER_TAG").unwrap_or_else(|_| "now".to_string());
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.40, 900.0);

    const SPAN: i32 = 3;
    let mut chunks = ChunkManager::new(SPAN + 2);
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    let pond = (8..=39).contains(&gx) && (8..=39).contains(&gz);
                    for y in 0..=21 {
                        let id = if pond && y >= 15 {
                            if y == 21 {
                                BLOCK_AIR
                            } else if y == 20 && gx <= 23 && !((14..=16).contains(&gx) && (22..=24).contains(&gz)) {
                                // The lid, with a hole kept open in it.
                                BLOCK_ICE
                            } else {
                                BLOCK_WATER
                            }
                        } else if y == 21 {
                            BLOCK_GRASS
                        } else {
                            BLOCK_STONE
                        };
                        data[Chunk::index(x, y, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..SPAN * SPAN)
        .map(|i| {
            let pos = ChunkPos::new(i % SPAN, i / SPAN);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();
    // (seat, eye, yaw, pitch, under water)
    let seats: [(&str, Vec3, f32, f32, bool); 5] = [
        ("under_lid", Vec3::new(12.5, 18.2, 30.5), 0.0, 30.0, true),
        ("under_edge", Vec3::new(30.5, 19.4, 30.0), 180.0, 8.0, true),
        ("under_hole", Vec3::new(15.5, 18.5, 18.5), 90.0, 35.0, true),
        ("open_edge", Vec3::new(31.5, 21.8, 30.0), 180.0, -18.0, false),
        ("over", Vec3::new(44.0, 27.0, 44.0), 225.0, -30.0, false),
    ];
    for (seat, eye, yaw, pitch, submerged) in seats {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let png = draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
            include_str!("shader.wgsl"), None, None, submerged, settings.msaa.max(1),
        );
        let path = format!("{out}/ice_{tag}_{seat}.png");
        png.save(&path).expect("write png");
        println!("  {seat:>12} -> {path}");
    }
}

/// **Fish traps set in a pond**, photographed through the real passes: one
/// on the bed of deep water, one in water a cell deep with the air over it,
/// from under the water and from above its surface.
///
/// ```text
/// GPU_REPRO_DIR=C:/abs/shots/fish_trap WATER_TAG=before cargo test -p primitive_client --lib \
///     what_a_fish_trap_in_water_looks_like -- --ignored --nocapture
/// ```
///
/// Written for "у ловушки для рыб проблемы с рендером под водой". The trap
/// is a solid block that displaces its water on purpose
/// (`types::BLOCK_FISH_TRAP`), and wicker you can see into.
#[test]
#[ignore = "a tool: needs a GPU; photographs fish traps in a pond"]
fn what_a_fish_trap_in_water_looks_like() {
    use primitive_shared::types::{BLOCK_FISH_TRAP, BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("WATER_TAG").unwrap_or_else(|_| "now".to_string());
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.40, 900.0);

    const SPAN: i32 = 3;
    // The deep trap on the bed, and the shallow one on a shelf a cell under
    // the surface.
    let traps = [(20, 15, 24), (34, 20, 24)];
    let mut chunks = ChunkManager::new(SPAN + 2);
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    let pond = (8..=39).contains(&gx) && (8..=39).contains(&gz);
                    let bed = if gx >= 31 { 19 } else { 14 };
                    for y in 0..=21 {
                        let id = if traps.contains(&(gx, y, gz)) {
                            BLOCK_FISH_TRAP
                        } else if pond && y > bed {
                            if y == 21 {
                                BLOCK_AIR
                            } else {
                                BLOCK_WATER
                            }
                        } else if y == 21 {
                            BLOCK_GRASS
                        } else {
                            BLOCK_STONE
                        };
                        data[Chunk::index(x, y, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..SPAN * SPAN)
        .map(|i| {
            let pos = ChunkPos::new(i % SPAN, i / SPAN);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();
    // (seat, eye, yaw, pitch, under water). Yaw 0 looks along +x.
    let seats: [(&str, Vec3, f32, f32, bool); 6] = [
        ("under_level", Vec3::new(17.5, 16.0, 24.5), 0.0, -12.0, true),
        ("under_close", Vec3::new(18.6, 16.9, 23.2), 30.0, -35.0, true),
        ("under_far", Vec3::new(13.5, 17.5, 22.0), 18.0, -14.0, true),
        ("above_deep", Vec3::new(17.5, 23.0, 24.5), 0.0, -60.0, false),
        ("above_shallow", Vec3::new(31.5, 22.4, 24.5), 0.0, -35.0, false),
        ("under_shallow", Vec3::new(26.5, 20.3, 24.5), 0.0, 0.0, true),
    ];
    for (seat, eye, yaw, pitch, submerged) in seats {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let png = draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
            include_str!("shader.wgsl"), None, None, submerged, settings.msaa.max(1),
        );
        let path = format!("{out}/trap_{tag}_{seat}.png");
        png.save(&path).expect("write png");
        println!("  {seat:>14} -> {path}");
    }
}

/// **Leaves and ice, everywhere the recent changes touched them**, through
/// the real passes: a crown in the air with snow on it, a bush sunk in a
/// pond and one poking out of it, leaves at the shore, a lid of ice over
/// water, a pile of ice on land beside a lamp, by day and by night.
///
/// ```text
/// GPU_REPRO_DIR=C:/abs/shots/leaves_ice WATER_TAG=before cargo test -p primitive_client --lib \
///     what_leaves_and_ice_look_like -- --ignored --nocapture
/// ```
///
/// `LEAFICE_ONLY=seat,seat` takes those seats alone, `LEAFICE_BACKDROP=1`
/// clears to magenta so a pixel nothing covered cannot pass for sky, and
/// `LEAFICE_CROWN_TOP=20` sinks the pond's crown to the brim.
///
/// Written for "у листвы и льда есть проблемы с рендером", which came with
/// no screenshot -- so every arrangement the flooded crown, the leaf wood
/// bits, the bite and the lid rules could have broken is put in one yard.
#[test]
#[ignore = "a tool: needs a GPU; photographs leaves and ice in the air, in water and under light"]
fn what_leaves_and_ice_look_like() {
    use primitive_shared::types::{
        BLOCK_APPLE_LEAVES_FRUIT, BLOCK_BIRCH_LEAVES, BLOCK_BUSH_LEAVES, BLOCK_GLOWSTONE, BLOCK_GRASS, BLOCK_ICE,
        BLOCK_LEAVES, BLOCK_LOG, BLOCK_SNOW, BLOCK_SNOW_COVER, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("WATER_TAG").unwrap_or_else(|_| "now".to_string());
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();

    const SPAN: i32 = 3;
    // `LEAFICE_CROWN_TOP=20` sinks the pond's crown whole, for telling what
    // its dry top does from what the water in it does.
    let crown_top: i32 = std::env::var("LEAFICE_CROWN_TOP").ok().and_then(|v| v.parse().ok()).unwrap_or(21);
    let place = |gx: i32, y: i32, gz: i32| -> Option<primitive_shared::types::BlockId> {
        // A tree on the land in the far corner, oak and birch grown into
        // each other, an apple cell in it, snow on half its top.
        if gx == 43 && gz == 43 && (22..=24).contains(&y) {
            return Some(BLOCK_LOG);
        }
        if (41..=45).contains(&gx) && (41..=45).contains(&gz) && (25..=27).contains(&y) {
            if (gx, y, gz) == (45, 26, 43) {
                return Some(BLOCK_APPLE_LEAVES_FRUIT);
            }
            return Some(if gx >= 44 { BLOCK_BIRCH_LEAVES } else { BLOCK_LEAVES });
        }
        if (41..=45).contains(&gx) && (41..=43).contains(&gz) && y == 28 {
            return Some(if gx == 42 && gz == 42 { BLOCK_SNOW } else { BLOCK_SNOW_COVER });
        }
        // A crown sunk in the pond, its top cell out of the water.
        if (28..=30).contains(&gx) && (20..=22).contains(&gz) && (17..=crown_top).contains(&y) {
            return Some(BLOCK_LEAVES);
        }
        // A bush wholly under the surface.
        if (32..=33).contains(&gx) && (30..=31).contains(&gz) && (16..=18).contains(&y) {
            return Some(BLOCK_BUSH_LEAVES);
        }
        // Leaves at the shore: overhanging the water and standing in it.
        if (37..=39).contains(&gx) && (30..=32).contains(&gz) && (20..=22).contains(&y) {
            return Some(BLOCK_LEAVES);
        }
        // A pile of ice on land beside a lamp, and a leaf on the lid.
        if (1..=4).contains(&gx) && (1..=4).contains(&gz) && (22..=23).contains(&y) {
            return Some(BLOCK_ICE);
        }
        if (gx, y, gz) == (6, 22, 2) {
            return Some(BLOCK_GLOWSTONE);
        }
        if (gx, y, gz) == (12, 21, 12) {
            return Some(BLOCK_LEAVES);
        }
        // A lamp set down on the lid, for the light on the ice at night.
        if (gx, y, gz) == (14, 21, 18) {
            return Some(BLOCK_GLOWSTONE);
        }
        None
    };
    let mut chunks = ChunkManager::new(SPAN + 2);
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    let pond = (8..=39).contains(&gx) && (8..=39).contains(&gz);
                    for y in 0..32 {
                        let id = if let Some(id) = place(gx, y, gz) {
                            id
                        } else if y > 21 {
                            BLOCK_AIR
                        } else if pond && y >= 15 {
                            if y == 21 {
                                BLOCK_AIR
                            } else if y == 20 && gx <= 20 {
                                BLOCK_ICE
                            } else {
                                BLOCK_WATER
                            }
                        } else if y == 21 {
                            BLOCK_GRASS
                        } else {
                            BLOCK_STONE
                        };
                        data[Chunk::index(x, y as usize, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..SPAN {
        for cx in 0..SPAN {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..SPAN * SPAN)
        .map(|i| {
            let pos = ChunkPos::new(i % SPAN, i / SPAN);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();
    // (seat, eye, yaw, pitch, under water, time of day). Yaw 0 looks along +x.
    let seats: [(&str, Vec3, f32, f32, bool, f32); 14] = [
        ("tree", Vec3::new(37.5, 25.5, 38.5), 40.0, 5.0, false, 0.5),
        ("tree_under", Vec3::new(43.2, 22.6, 41.0), 90.0, 60.0, false, 0.5),
        ("tree_snow", Vec3::new(38.0, 31.5, 38.0), 45.0, -35.0, false, 0.5),
        ("crown_above", Vec3::new(24.5, 24.0, 21.5), 0.0, -30.0, false, 0.5),
        ("crown_under", Vec3::new(24.5, 18.5, 21.5), 0.0, 0.0, true, 0.5),
        ("bush_under", Vec3::new(28.5, 17.5, 30.5), 0.0, -5.0, true, 0.5),
        ("shore", Vec3::new(31.5, 23.0, 31.0), 0.0, -25.0, false, 0.5),
        ("shore_under", Vec3::new(33.5, 19.0, 31.0), 0.0, 5.0, true, 0.5),
        ("ice_pile", Vec3::new(9.5, 24.5, 9.5), 225.0, -25.0, false, 0.5),
        ("ice_pile_night", Vec3::new(9.5, 24.5, 9.5), 225.0, -25.0, false, 0.0),
        ("ice_lid", Vec3::new(26.5, 23.5, 14.5), 180.0, -25.0, false, 0.5),
        ("ice_under", Vec3::new(26.5, 18.5, 14.5), 180.0, 15.0, true, 0.5),
        ("ice_lid_night", Vec3::new(20.5, 24.0, 18.5), 180.0, -40.0, false, 0.0),
        ("leaf_on_ice", Vec3::new(14.2, 22.4, 14.2), 225.0, -30.0, false, 0.5),
    ];
    // `LEAFICE_BACKDROP=1` clears to magenta and leaves the sky out, so a
    // pixel nothing covered is told from a pale texel.
    let backdrop = std::env::var("LEAFICE_BACKDROP")
        .is_ok()
        .then_some(wgpu::Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 });
    let only = std::env::var("LEAFICE_ONLY").ok();
    for (seat, eye, yaw, pitch, submerged, time) in seats {
        if only.as_deref().is_some_and(|only| !only.split(',').any(|s| s == seat)) {
            continue;
        }
        let sky = Sky::new(time, 900.0);
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let png = draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
            include_str!("shader.wgsl"), None, backdrop, submerged, settings.msaa.max(1),
        );
        // **Saved opaque, as the window shows it.** The blended pass leaves
        // the target's alpha short of one where a leaf's cut-out edge was
        // drawn under water, and the swapchain ignores it -- but a picture
        // viewer composites it over white, which drew pale specks along
        // every leaf seam that are not in the frame at all.
        let mut png = png;
        for pixel in png.pixels_mut() {
            pixel.0[3] = 255;
        }
        let path = format!("{out}/leafice_{tag}_{seat}.png");
        png.save(&path).expect("write png");
        println!("  {seat:>14} -> {path}");
    }
}
