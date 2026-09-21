//! **Fire, photographed**: what the fire work of this release looks like
//! through the real terrain shader, at noon and at night, with nobody at a
//! keyboard.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/fire \
//!     cargo test -p primitive_client --lib \
//!     what_fire_leaves_in_the_world -- --ignored --nocapture
//! ```
//!
//! One row of the camp, left to right: a campfire with planks laid on it
//! (the black block of the report), boards and a log alight, the char they
//! leave, a standing torch alight and one burnt out, a pit kiln holding a
//! brick, a mould, a jug and a pot, the same pit under its first armful of
//! fibre, a cobble ceiling at each stage of soot over three posts, and a
//! furrow with ash dug into it beside one without.
//!
//! Particles and the smoke fog are not in these pictures: they are not
//! terrain, and the tests in `particles` and `fog` hold them.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::pit::Stage;
use primitive_shared::types::{
    oriented, Axis, BlockId, Chunk, BLOCK_AIR, BLOCK_BRICK_RAW, BLOCK_BURNING_LOG, BLOCK_BURNING_PLANKS,
    BLOCK_CAMPFIRE_LIT, BLOCK_CHARRED_LOG, BLOCK_CHARRED_PLANKS, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FARMLAND,
    BLOCK_GRASS, BLOCK_JUG_RAW, BLOCK_MOULD_RAW, BLOCK_PLANKS, BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_LIT,
    BLOCK_STANDING_TORCH_OUT, BLOCK_STONE, BLOCK_VESSEL_RAW, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

/// The top of the ground: things stand in y = 3.
const GROUND: usize = 3;

/// A pit's pottery, by where it is in the scene.
const POTTERY: [BlockId; 4] = [BLOCK_BRICK_RAW, BLOCK_MOULD_RAW, BLOCK_JUG_RAW, BLOCK_VESSEL_RAW];

fn scene(textures: &TextureManager) -> Vec<(ChunkPos, MeshBuffers)> {
    let mut chunks = ChunkManager::new(8);
    let centre = ChunkPos::new(0, 0);
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_GRASS;
                }
            }
            if pos == centre {
                let mut put = |x: usize, y: usize, z: usize, block: BlockId| blocks[Chunk::index(x, y, z)] = block;
                // The report: planks laid on a campfire.
                put(1, GROUND, 8, BLOCK_CAMPFIRE_LIT);
                put(1, GROUND + 1, 8, BLOCK_PLANKS);
                // Alight, and what is left.
                put(3, GROUND, 8, BLOCK_BURNING_PLANKS);
                put(4, GROUND, 8, oriented(BLOCK_BURNING_LOG, Axis::X));
                put(3, GROUND, 10, BLOCK_CHARRED_PLANKS);
                put(4, GROUND, 10, oriented(BLOCK_CHARRED_LOG, Axis::X));
                // Standing torches.
                put(6, GROUND, 8, BLOCK_STANDING_TORCH);
                put(6, GROUND + 1, 8, BLOCK_STANDING_TORCH_LIT);
                put(7, GROUND, 10, BLOCK_STANDING_TORCH);
                put(7, GROUND + 1, 10, BLOCK_STANDING_TORCH_OUT);
                // Two pits: pottery, and pottery under an armful of fibre.
                for x in [9usize, 11] {
                    put(x, 2, 8, BLOCK_AIR);
                    put(x, 1, 8, BLOCK_STONE);
                }
                put(9, 2, 8, Stage::Pottery { pieces: 4, fired: false }.block());
                put(11, 2, 8, Stage::Fibre(1).block());
                // Soot: a row of cobble at stages nought to three.
                for (stage, x) in [12usize, 13, 14, 15].into_iter().enumerate() {
                    put(x, GROUND, 11, primitive_shared::wildfire::with_soot(BLOCK_COBBLESTONE, stage as u8));
                }
                // A furrow with ash, and one without.
                put(13, 2, 6, primitive_shared::wildfire::dressed(BLOCK_FARMLAND).unwrap());
                put(14, 2, 6, BLOCK_FARMLAND);
            }
            chunks.insert(Chunk { pos, blocks });
        }
    }
    for x in [9, 11] {
        chunks.note_pit_pottery((x, 2, 8), POTTERY.to_vec());
    }
    let mut light = LightMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let generator = WorldGen::new(0);
    let layers = textures.face_layers();
    let mut meshes = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(pos, &chunks, &light);
            cache.set_pottery(chunks.pit_pottery_in(pos));
            let mut out = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut out);
            meshes.push((pos, out));
        }
    }
    meshes
}

#[test]
#[ignore = "a tool: needs a GPU; photographs fire, char, torches, pits and soot"]
fn what_fire_leaves_in_the_world() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let mut settings = ClientSettings::default();
    settings.sanitize();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let meshes = scene(&textures);
    let shader = include_str!("shader.wgsl").replace("\r\n", "\n");

    let seats: [(&str, Vec3, f32, f32); 3] = [
        ("camp", Vec3::new(8.0, GROUND as f32 + 2.2, 16.5), -90.0, -18.0),
        ("pits", Vec3::new(10.0, GROUND as f32 + 1.2, 9.6), -90.0, -75.0),
        ("soot", Vec3::new(14.0, GROUND as f32 + 1.2, 15.5), -90.0, -10.0),
    ];
    for (hour_name, hour) in [("noon", 0.5f32), ("night", 0.0)] {
        let sky = Sky::new(hour, 900.0);
        for (seat, position, yaw, pitch) in seats {
            let mut camera = Camera::new(position.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = yaw.to_radians();
            camera.pitch = pitch.to_radians();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let picture = super::offscreen_repro::draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE, &shader, None, None, false,
                settings.msaa,
            );
            picture.save(format!("{out}/{seat}_{hour_name}.png")).expect("write png");
        }
    }
    println!("pictures in {out}");
}
