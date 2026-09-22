//! **Anisotropy off, 4 and 16, through the real terrain shader**: plants,
//! fire and blocks up close and a field running away to the fog.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir cargo test -p primitive_client --lib \
//!     what_anisotropy_does_to_plants_fire_and_blocks -- --ignored --nocapture
//! ```
//!
//! The report it was written for: "with anisotropic filtering off plants,
//! fire and campfires break, and with it on the textures go soapy". Each
//! setting is one column of `sheet.png`, and every picture is also cut out
//! and magnified nearest-neighbour into `loupe.png`, so a difference of a
//! texel is argued over at a size where it can be seen.
//!
//! `SHADER_BEFORE=<path to a shader.wgsl>` adds a row drawn with that shader
//! above the row drawn with the one in the tree -- a before and after from
//! one binary, one scene and one seat. `LOUPE=x,y,w,h;x,y,w,h` picks what
//! the loupe cuts out.
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
use primitive_shared::types::{
    oriented, Axis, BlockId, Chunk, BLOCK_AIR, BLOCK_BERRY_BUSH, BLOCK_BURNING_LOG, BLOCK_CAMPFIRE_LIT,
    BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FERN, BLOCK_FIREPIT_LIT, BLOCK_FIREWEED, BLOCK_FLOWER, BLOCK_GRASS,
    BLOCK_LEAVES, BLOCK_LOG, BLOCK_FLINT, BLOCK_PEBBLE, BLOCK_STONE, BLOCK_TALL_GRASS, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (960, 540);

/// Things stand in y = 3; the grass is y = 2.
const GROUND: i32 = 3;

fn scene(textures: &TextureManager) -> Vec<(ChunkPos, MeshBuffers)> {
    let mut grids = std::collections::HashMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_GRASS;
                    // A scatter of tufts out to the fog: the distance is
                    // where filtering earns its keep.
                    let wx = x as i32 + cx * CHUNK_SIZE_X as i32;
                    let wz = z as i32 + cz * CHUNK_SIZE_Z as i32;
                    if (wx * 7 + wz * 13).rem_euclid(5) == 0 && wz < 24 {
                        blocks[Chunk::index(x, GROUND as usize, z)] = BLOCK_TALL_GRASS;
                    }
                }
            }
            grids.insert(pos, blocks);
        }
    }
    let mut put = |wx: i32, wy: i32, wz: i32, block: BlockId| {
        let cx = wx.div_euclid(CHUNK_SIZE_X as i32);
        let cz = wz.div_euclid(CHUNK_SIZE_Z as i32);
        let blocks: &mut Vec<BlockId> = grids.get_mut(&ChunkPos::new(cx, cz)).expect("chunk in the scene");
        let x = wx.rem_euclid(CHUNK_SIZE_X as i32) as usize;
        let z = wz.rem_euclid(CHUNK_SIZE_Z as i32) as usize;
        blocks[Chunk::index(x, wy as usize, z)] = block;
    };
    // The near row, left to right from the seat.
    let near: [BlockId; 12] = [
        BLOCK_TALL_GRASS,
        BLOCK_FLOWER,
        BLOCK_FERN,
        BLOCK_FIREWEED,
        BLOCK_BERRY_BUSH,
        BLOCK_CAMPFIRE_LIT,
        BLOCK_FIREPIT_LIT,
        oriented(BLOCK_BURNING_LOG, Axis::X),
        BLOCK_LEAVES,
        BLOCK_COBBLESTONE,
        BLOCK_PEBBLE,
        BLOCK_FLINT,
    ];
    // Stones lying in the grass a step from the seat, where `engine::relief`
    // gives them a thickness.
    put(6, GROUND, 28, BLOCK_PEBBLE);
    put(9, GROUND, 28, BLOCK_FLINT);
    for (i, block) in near.into_iter().enumerate() {
        put(2 + i as i32, GROUND, 26, block);
    }
    // A wall standing in the grass, and a far one at the back of the field.
    for wx in 1..15 {
        put(wx, GROUND, 25, BLOCK_AIR);
        put(wx, GROUND, 24, BLOCK_AIR);
    }
    put(2, GROUND, 26, BLOCK_LOG);
    put(2, GROUND + 1, 26, BLOCK_COBBLESTONE);
    for wx in -16..32 {
        for wy in GROUND..GROUND + 4 {
            put(wx, wy, -14, if wy % 2 == 0 { BLOCK_COBBLESTONE } else { BLOCK_LOG });
        }
    }
    let mut chunks = ChunkManager::new(8);
    for (pos, blocks) in grids {
        chunks.insert(Chunk { pos, blocks });
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

fn loupes() -> Vec<(u32, u32, u32, u32)> {
    let spec = std::env::var("LOUPE").unwrap_or_else(|_| "300,250,160,110;560,120,160,110".to_string());
    spec.split(';')
        .filter_map(|part| {
            let n: Vec<u32> = part.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
        })
        .collect()
}

#[test]
#[ignore = "a tool: needs a GPU; photographs plants, fire and blocks at anisotropy off, 4 and 16"]
fn what_anisotropy_does_to_plants_fire_and_blocks() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    // The player's own: fov 95, and the setting under test swept.
    let mut settings = ClientSettings { fov_degrees: 95.0, ..Default::default() };
    settings.sanitize();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let mut textures = TextureManager::load(device, queue, assets, 16).expect("textures load");
    let meshes = scene(&textures);
    let mut shaders = Vec::new();
    if let Ok(path) = std::env::var("SHADER_BEFORE") {
        shaders.push(("before", std::fs::read_to_string(path).expect("SHADER_BEFORE").replace("\r\n", "\n")));
    }
    shaders.push(("after", include_str!("shader.wgsl").replace("\r\n", "\n")));
    let sky = Sky::new(0.45, 900.0);
    let mut camera = Camera::new((Vec3::new(8.0, GROUND as f32 + 1.1, 29.6)).as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
    // `SEAT_YAW`: straight down the field is -90. A seat turned off the
    // block grid matters -- a texel seam crossing the pixel grid at a slant
    // is where a ramp measured too wide shows.
    let yaw: f32 = std::env::var("SEAT_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(-90.0);
    camera.yaw = yaw.to_radians();
    camera.pitch = (-14.0f32).to_radians();
    camera.fov_y_radians = settings.fov_degrees.to_radians();

    let settings_swept = [1u16, 4, 16];
    let crops = loupes();
    const ZOOM: u32 = 3;
    let crop_w: u32 = crops.iter().map(|c| c.2 * ZOOM + 4).sum();
    let crop_h: u32 = crops.iter().map(|c| c.3 * ZOOM).max().unwrap_or(0);
    let mut sheet = image::RgbaImage::new(SIZE.0 * settings_swept.len() as u32, SIZE.1 * shaders.len() as u32);
    let mut loupe = image::RgbaImage::new(crop_w * settings_swept.len() as u32, (crop_h + 4) * shaders.len() as u32);
    for (row, (name, shader)) in shaders.iter().enumerate() {
        for (col, &aniso) in settings_swept.iter().enumerate() {
            settings.anisotropy = aniso;
            textures.sampler = crate::engine::texture::build_sampler(device, aniso);
            let picture = super::offscreen_repro::draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE, shader, None, None, false,
                settings.msaa,
            );
            picture.save(format!("{out}/{name}_aniso{aniso}.png")).expect("write png");
            image::imageops::replace(&mut sheet, &picture, (col as u32 * SIZE.0) as i64, (row as u32 * SIZE.1) as i64);
            let mut x = col as u32 * crop_w;
            for &(cx, cy, cw, ch) in &crops {
                let cut = image::imageops::crop_imm(&picture, cx, cy, cw, ch).to_image();
                let big = image::imageops::resize(&cut, cw * ZOOM, ch * ZOOM, image::imageops::FilterType::Nearest);
                image::imageops::replace(&mut loupe, &big, x as i64, (row as u32 * (crop_h + 4)) as i64);
                x += cw * ZOOM + 4;
            }
        }
    }
    sheet.save(format!("{out}/sheet.png")).expect("write sheet");
    loupe.save(format!("{out}/loupe.png")).expect("write loupe");
    println!("pictures in {out}: columns are anisotropy {settings_swept:?}, rows {:?}", shaders.iter().map(|s| s.0).collect::<Vec<_>>());
}
