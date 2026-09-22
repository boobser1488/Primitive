//! **Dripstone, photographed**: every size of stalagmite and stalactite
//! through the real terrain shader, with nobody at a keyboard.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/dripstone \
//!     cargo test -p primitive_client --lib \
//!     dripstone_under_a_limestone_roof -- --ignored --nocapture
//! ```
//!
//! A limestone slab on stone pillars over a limestone floor, open at the
//! sides so the noon sky lights it -- a cave that could be seen into
//! without a torch. Three pairs under it, smallest on the left, each
//! stalagmite under its stalactite the way the generator pairs them, and a
//! lone stalagmite of each size in front.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::dripstone::sized;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    BlockId, Chunk, BLOCK_AIR, BLOCK_LIMESTONE, BLOCK_STALACTITE, BLOCK_STALAGMITE, BLOCK_STONE, CHUNK_SIZE_X, CHUNK_SIZE_Z,
    CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

/// The floor's top course: things stand in y = 3.
const FLOOR: usize = 3;
/// The roof: two cells of air under it.
const ROOF: usize = FLOOR + 2;

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
                    blocks[Chunk::index(x, 1, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_LIMESTONE;
                }
            }
            if pos == centre {
                let mut put = |x: usize, y: usize, z: usize, block: BlockId| blocks[Chunk::index(x, y, z)] = block;
                for z in 4..12 {
                    for x in 3..13 {
                        put(x, ROOF, z, BLOCK_LIMESTONE);
                    }
                }
                for (x, z) in [(3, 4), (12, 4), (3, 11), (12, 11)] {
                    put(x, FLOOR, z, BLOCK_STONE);
                    put(x, FLOOR + 1, z, BLOCK_STONE);
                }
                for size in 0..3u8 {
                    let x = 5 + 3 * size as usize;
                    put(x, FLOOR + 1, 7, sized(BLOCK_STALACTITE, size));
                    put(x, FLOOR, 7, sized(BLOCK_STALAGMITE, size));
                    put(x, FLOOR, 10, sized(BLOCK_STALAGMITE, size));
                }
            }
            chunks.insert(Chunk { pos, blocks });
        }
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
            let mut out = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut out);
            meshes.push((pos, out));
        }
    }
    meshes
}

#[test]
#[ignore = "a tool: needs a GPU; photographs stalagmites and stalactites of every size"]
fn dripstone_under_a_limestone_roof() {
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

    let seats: [(&str, Vec3, f32, f32); 2] = [
        ("front", Vec3::new(8.0, FLOOR as f32 + 1.0, 15.0), -90.0, 0.0),
        ("close", Vec3::new(7.5, FLOOR as f32 + 1.0, 12.2), -90.0, 5.0),
    ];
    let sky = Sky::new(0.5, 900.0);
    for (seat, position, yaw, pitch) in seats {
        let mut camera = Camera::new(position.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let picture = super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE, &shader, None, None, false,
            settings.msaa,
        );
        picture.save(format!("{out}/{seat}.png")).expect("write png");
    }
    println!("pictures in {out}");
}
