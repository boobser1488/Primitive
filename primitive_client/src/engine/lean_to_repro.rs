//! The lean-to, through the chunk mesher and the real passes.
//!
//! ```text
//! GPU_REPRO_DIR=C:/abs/shots/lean_to LEAN_TAG=after cargo test -p primitive_client --bin primitive_client \
//!     what_a_lean_to_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for "какого хера шалаш размером с 2 блока и не имеет нормальной
//! модели?". A lean-to is several cells and one of them draws the whole
//! model (`mesh::lean_to_block`), so only the mesher shows it as a player
//! does: this stands one in each facing on a meadow, with a plank bed and a
//! door beside the south one for a body's length and a body's height, and
//! walks round each -- from the mouth, from both sides, from behind, from a
//! crouch at the mouth looking in, and from above.
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
    bed_half, door_partner, faced, Chunk, ChunkPos, Facing, BLOCK_AIR, BLOCK_DOOR, BLOCK_GRASS,
    BLOCK_STONE, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

#[test]
#[ignore = "a tool: needs a GPU; walks round a lean-to in each facing"]
fn what_a_lean_to_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("LEAN_TAG").unwrap_or_else(|_| "now".to_string());
    // The reporting player's lens and filtering.
    let settings = crate::settings::ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..Default::default() };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.42, 900.0);

    const GROUND: i32 = 10;
    // The cell a player puts a lean-to down in, and the way they looked.
    let huts = [
        (Facing::North, (12, GROUND + 1, 14)),
        (Facing::East, (28, GROUND + 1, 12)),
        (Facing::South, (12, GROUND + 1, 28)),
        (Facing::West, (30, GROUND + 1, 30)),
    ];
    let mut placed = Vec::new();
    for &(facing, at) in &huts {
        placed.extend(primitive_shared::lean_to::cells_from_mouth(at, facing));
    }
    // A plank bed and a door beside the south one: a body's length and a
    // body's height, to read the hut's size against.
    let (bx, by, bz) = (16, GROUND + 1, 28);
    placed.push(((bx, by, bz), bed_half(Facing::South, false)));
    placed.push(((bx, by, bz + 1), bed_half(Facing::South, true)));
    let door = faced(BLOCK_DOOR, Facing::South);
    placed.push(((bx + 2, by, bz), door));
    if let Some((top_at, top)) = door_partner((bx + 2, by, bz), door) {
        placed.push((top_at, top));
    }

    let span: i32 = 3;
    let mut chunks = ChunkManager::new(span + 2);
    for cz in 0..span {
        for cx in 0..span {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    for y in 0..=GROUND + 3 {
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
        let path = format!("{out}/lean_to_{tag}_{name}.png").to_lowercase();
        png.save(&path).expect("write png");
        println!("  {path}");
    };
    // A player standing, eyes 1.62 over the floor.
    const EYE: f32 = 1.62;
    let floor = (GROUND + 1) as f32;
    for &(facing, (x, _, z)) in &huts {
        // `Facing::step` is toward the placer: the mouth is at `at`, and the
        // hut runs back from it the other way.
        let (dx, dz) = facing.step();
        let back = Vec3::new(-dx as f32, 0.0, -dz as f32);
        let side = Vec3::new(-dz as f32, 0.0, dx as f32);
        let mouth = Vec3::new(x as f32 + 0.5, floor, z as f32 + 0.5);
        let middle = mouth + back * 1.0 + Vec3::new(0.0, 0.6, 0.0);
        let seats = [
            ("mouth", mouth - back * 3.5 + side * 0.6 + Vec3::new(0.0, EYE, 0.0), middle),
            ("left", middle + side * 4.0 - back * 0.5 + Vec3::new(0.0, EYE - 0.6, 0.0), middle),
            ("right", middle - side * 4.0 + back * 0.3 + Vec3::new(0.0, EYE - 0.6, 0.0), middle),
            ("behind", middle + back * 4.5 + side * 0.8 + Vec3::new(0.0, EYE - 0.6, 0.0), middle),
            ("corner", mouth - back * 2.6 + side * 2.6 + Vec3::new(0.0, EYE, 0.0), middle),
            ("near", mouth - back * 1.6 + side * 1.2 + Vec3::new(0.0, EYE, 0.0), middle - back * 0.4),
            ("far_corner", mouth + back * 3.2 - side * 2.4 + Vec3::new(0.0, EYE, 0.0), middle),
            ("crouch", mouth - back * 1.3 + Vec3::new(0.0, 0.7, 0.0), mouth + back * 1.6 + Vec3::new(0.0, 0.3, 0.0)),
            ("above", middle + side * 2.5 - back * 2.5 + Vec3::new(0.0, 5.0, 0.0), middle),
        ];
        for (seat, eye, target) in seats {
            shoot(format!("{facing:?}_{seat}"), eye, target);
        }
    }
}
