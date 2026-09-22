//! The two-by-two drying rack, through the chunk mesher and the real passes.
//!
//! ```text
//! GPU_REPRO_DIR=C:/abs/shots/leaves_ice RACK_TAG=before cargo test -p primitive_client --lib \
//!     what_a_whole_rack_looks_like -- --ignored --nocapture
//! ```
//!
//! ```text
//! GPU_REPRO_DIR=C:/abs/shots/rack_goods RACK_TAG=after RACK_GOODS=5:6,3:4,11:12 cargo test \
//!     -p primitive_client --lib what_a_whole_rack_looks_like -- --ignored --nocapture
//! ```
//!
//! `RACK_GOODS` hangs other goods: one rack per `near:far` pair of rows of
//! `rack::HANGING` (1 hide, 2 leather, 3/4 meat raw/dried, 5/6 fish, 7/8
//! salted meat, 9/10 salted fish, 11/12 peat, 13/14 kelp), the facings
//! taken round in turn. Unset, the camp is the one this was written with:
//! two bare racks and two with a hide and raw meat.
//!
//! Written for "текстура сушилки 2x2 -- месиво". `rack_through_the_real_shader`
//! draws the one-cell frame through the model emitter; the whole rack is a
//! model written to thirty-two sixteenths that stands out of its own cell
//! (`mesh::rack_block`, `RackColumns::Whole`), and only the mesher draws it,
//! so this builds a camp of four -- one per facing, bare and hung -- and
//! walks round each.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory.

use super::offscreen_repro::draw_scene;
use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    rack_cells, rack_far_step, Chunk, ChunkPos, Facing, BLOCK_AIR, BLOCK_GRASS, BLOCK_STONE, CHUNK_VOLUME,
    RACK_GOODS_SHIFT,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

/// One rack of the camp: its facing, its anchor, and the rows of
/// `rack::HANGING` on its near and far columns.
type Rack = (Facing, (i32, i32, i32), u16, u16);

#[test]
#[ignore = "a tool: needs a GPU; walks round a two-by-two rack in each facing, bare and hung"]
fn what_a_whole_rack_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("RACK_TAG").unwrap_or_else(|_| "now".to_string());
    // The reporting player's lens and filtering.
    let settings = crate::settings::ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..Default::default() };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.42, 900.0);

    const GROUND: i32 = 10;
    // (facing, anchor, near goods, far goods). Unset: a rack in each facing,
    // the first two bare and the last two with a hide on the near column and
    // raw meat on the far one (1 and 3 of `rack::HANGING`).
    let racks: Vec<Rack> = match std::env::var("RACK_GOODS") {
        Ok(list) => list
            .split(',')
            .filter_map(|pair| {
                let (near, far) = pair.trim().split_once(':')?;
                Some((near.parse().ok()?, far.parse().ok()?))
            })
            .enumerate()
            .map(|(k, (near, far))| {
                let facing = [Facing::North, Facing::East, Facing::South, Facing::West][k % 4];
                let (col, row) = ((k % 3) as i32, (k / 3) as i32);
                (facing, (10 + col * 20, GROUND + 1, 10 + row * 20), near, far)
            })
            .collect(),
        Err(_) => vec![
            (Facing::North, (10, GROUND + 1, 10), 0, 0),
            (Facing::East, (30, GROUND + 1, 10), 0, 0),
            (Facing::South, (12, GROUND + 1, 30), 1, 3),
            (Facing::West, (30, GROUND + 1, 32), 1, 3),
        ],
    };
    let mut placed = Vec::new();
    for &(facing, anchor, near, far) in &racks {
        for (i, (at, id)) in rack_cells(anchor, facing).into_iter().enumerate() {
            // A column's goods are four bits split over its two cells: the
            // low two on the bottom, the high two on the top
            // (`types::rack_column_goods`). Written whole into the bottom
            // cell, a row from 4 up spilled into the bits beside them and the
            // rack came apart into two lone frames.
            let goods = match i {
                0 => near & 3,
                1 => near >> 2,
                2 => far & 3,
                _ => far >> 2,
            };
            placed.push((at, id | (goods << RACK_GOODS_SHIFT)));
        }
    }
    // `RACK_FRAMES=1` stands two hide frames among the racks, one bare and
    // one with a skin laced in it, to see the two racks side by side
    // (`what_a_laced_hide_looks_like` photographs the frame itself).
    if std::env::var("RACK_FRAMES").is_ok() {
        use primitive_shared::types::{faced, rack_with_hide, BLOCK_HIDE_FRAME};
        placed.push(((20, GROUND + 1, 20), faced(BLOCK_HIDE_FRAME, Facing::South)));
        placed.push(((22, GROUND + 1, 20), rack_with_hide(faced(BLOCK_HIDE_FRAME, Facing::South), true)));
    }

    let reach = racks.iter().map(|&(_, (x, _, z), _, _)| x.max(z)).max().unwrap_or(0);
    let span: i32 = (reach + 8) / 16 + 2;
    let mut chunks = ChunkManager::new(span + 2);
    for cz in 0..span {
        for cx in 0..span {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    for y in 0..=GROUND + 2 {
                        let id = if let Some(&(_, id)) = placed.iter().find(|(at, _)| *at == (gx, y, gz)) {
                            id
                        } else if y < GROUND {
                            BLOCK_STONE
                        } else if y == GROUND {
                            BLOCK_GRASS
                        } else {
                            BLOCK_AIR
                        };
                        data[Chunk::index(x, y as usize, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..span {
        for cx in 0..span {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..span * span)
        .map(|i| {
            let pos = ChunkPos::new(i % span, i / span);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();

    // Round each rack: in front of its broad side, behind it, off one end,
    // and close up at a pole's foot.
    for &(facing, (ax, ay, az), near, far) in &racks {
        let (dx, dz) = rack_far_step(facing);
        let centre = Vec3::new(ax as f32 + 0.5 + dx as f32 * 0.5, ay as f32 + 1.0, az as f32 + 0.5 + dz as f32 * 0.5);
        // Across the ridge: the broad side faces along (-dz, dx).
        let across = Vec3::new(-dz as f32, 0.0, dx as f32);
        let along = Vec3::new(dx as f32, 0.0, dz as f32);
        let seats = [
            ("front", centre + across * 3.2 + Vec3::new(0.0, 0.6, 0.0)),
            ("back", centre - across * 3.2 + Vec3::new(0.0, 0.9, 0.0)),
            ("end", centre + along * 3.4 + across * 0.8 + Vec3::new(0.0, 0.4, 0.0)),
            ("close", centre + across * 1.4 - along * 0.6 + Vec3::new(0.0, -0.2, 0.0)),
        ];
        for (seat, eye) in seats {
            let look = (centre - eye).normalize();
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = look.z.atan2(look.x);
            camera.pitch = look.y.asin();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let png = draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
                include_str!("shader.wgsl"), None, None, false, settings.msaa.max(1),
            );
            // Opaque, as the window shows it: see `what_leaves_and_ice_look_like`.
            let mut png = png;
            for pixel in png.pixels_mut() {
                pixel.0[3] = 255;
            }
            let load = if near == 0 && far == 0 { "bare".to_string() } else { format!("hung{near}-{far}") };
            let path = format!("{out}/rack_{tag}_{facing:?}_{load}_{seat}.png").to_lowercase();
            png.save(&path).expect("write png");
            println!("  {path}");
        }
    }
}

/// **The hide frame: a skin laced into a standing frame of poles**, bare, raw
/// and cured, in each facing, through the mesher and the real passes.
///
/// ```text
/// GPU_REPRO_DIR=C:/abs/shots/hide_frame HIDE_TAG=laced cargo test -p primitive_client --lib \
///     what_a_laced_hide_looks_like -- --ignored --nocapture
/// ```
///
/// A camp of twelve frames, a row per state and a column per facing, with
/// the reporting player's lens and filtering; every file is prefixed with
/// `HIDE_TAG` (default `laced`), so a before and an after sit side by side.
/// Each row is photographed from where a player walks up to it, each frame
/// from a pace in front, and the raw one facing south walked round on all
/// four sides and close up at the lacing.
#[test]
#[ignore = "a tool: needs a GPU; photographs the hide frame bare, raw and cured in each facing"]
fn what_a_laced_hide_looks_like() {
    use primitive_shared::types::{faced, hide_frame_showing, BLOCK_HIDE_FRAME};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("HIDE_TAG").unwrap_or_else(|_| "laced".to_string());
    let settings = crate::settings::ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..Default::default() };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.42, 900.0);

    const GROUND: i32 = 10;
    const FACINGS: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];
    const STATES: [(&str, bool, bool); 3] = [("bare", false, false), ("raw", true, false), ("cured", false, true)];
    let cell = |state: usize, facing: usize| (8 + facing as i32 * 3, GROUND + 1, 8 + state as i32 * 5);
    let mut placed = Vec::new();
    for (s, &(_, raw, cured)) in STATES.iter().enumerate() {
        for (f, &facing) in FACINGS.iter().enumerate() {
            placed.push((cell(s, f), hide_frame_showing(faced(BLOCK_HIDE_FRAME, facing), raw, cured)));
        }
    }
    let span: i32 = 3;
    let mut chunks = ChunkManager::new(span + 2);
    for cz in 0..span {
        for cx in 0..span {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    for y in 0..=GROUND + 2 {
                        let id = if let Some(&(_, id)) = placed.iter().find(|(at, _)| *at == (gx, y, gz)) {
                            id
                        } else if y < GROUND {
                            BLOCK_STONE
                        } else if y == GROUND {
                            BLOCK_GRASS
                        } else {
                            BLOCK_AIR
                        };
                        data[Chunk::index(x, y as usize, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..span {
        for cx in 0..span {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..span * span)
        .map(|i| {
            let pos = ChunkPos::new(i % span, i / span);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();

    let shoot = |name: String, eye: Vec3, target: Vec3| {
        let look = (target - eye).normalize();
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = look.z.atan2(look.x);
        camera.pitch = look.y.asin();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let mut png = draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
            include_str!("shader.wgsl"), None, None, false, settings.msaa.max(1),
        );
        for pixel in png.pixels_mut() {
            pixel.0[3] = 255;
        }
        let path = format!("{out}/{tag}_{name}.png").to_lowercase();
        png.save(&path).expect("write png");
        println!("  {path}");
    };
    // A player standing, eyes 1.62 over the floor.
    const EYE: f32 = 1.62;
    let floor = (GROUND + 1) as f32;
    let centre = |(x, y, z): (i32, i32, i32)| Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
    for (s, &(state, _, _)) in STATES.iter().enumerate() {
        let middle = (centre(cell(s, 1)) + centre(cell(s, 2))) * 0.5;
        shoot(format!("row_{state}"), Vec3::new(middle.x, floor + EYE, middle.z + 3.5), middle);
        for (f, facing) in FACINGS.iter().enumerate() {
            let at = centre(cell(s, f));
            shoot(format!("{state}_{facing:?}_stand"), Vec3::new(at.x + 0.3, floor + EYE, at.z + 1.8), at);
        }
    }
    let at = centre(cell(1, 2));
    for (side, dx, dz) in [("south", 0.0, 1.0), ("east", 1.0, 0.0), ("north", 0.0, -1.0), ("west", -1.0, 0.0)] {
        shoot(format!("raw_south_round_{side}"), Vec3::new(at.x + dx * 2.0 + dz * 0.4, floor + EYE, at.z + dz * 2.0 - dx * 0.4), at);
    }
    shoot("raw_south_close".to_string(), Vec3::new(at.x + 0.2, floor + 0.9, at.z + 1.0), at + Vec3::new(-0.2, 0.0, 0.0));
    let cured = centre(cell(2, 2));
    shoot("cured_south_close".to_string(), Vec3::new(cured.x + 0.2, floor + 0.9, cured.z + 1.0), cured + Vec3::new(-0.2, 0.0, 0.0));
}
