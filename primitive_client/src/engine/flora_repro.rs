//! **The forest floor, the birch wood and the wild plants**, photographed
//! through the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir cargo test --release -p primitive_client --lib \
//!     what_the_forest_floor_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for "сделай леса темнее ... убрать траву везде в лесах" and "у
//! березы нету веток", so each picture answers one of those:
//!
//! * `oak_floor_*` and `taiga_floor_*` -- standing under the crowns of a wood,
//!   looking along its floor, **before and after**;
//! * `oak_above_*` -- the same wood from over its canopy, before and after;
//! * `birch_floor_after`, `birch_clearing_after` -- a birch wood of pieces, from
//!   under it and from a gap in it;
//! * `plants_*` -- every new plant on a lawn, tall ones in the back row.
//!
//! **"Before" is this build's wood with its floor taken back by hand, not the
//! old build.** A second binary is the comparison that moves everything else
//! too, so what can be undone in one process is: the forest floor put back to
//! the meadow's grass one column in seven under the crowns, the ferns,
//! bilberries and bracken taken out. **The wood's density cannot be undone
//! that way** -- the oak wood and the taiga grow closer now
//! (`worldgen::Biome::tree_spacing`) -- so how much darker a closed wood is
//! is the number `the_floor_under_a_wood_is_darker_than_the_open_ground_beside_it`
//! prints, not a picture; and the birch's shape cannot be taken back either,
//! so the birch has no "before".
//!
//! Each line printed is a picture's mean luminance in levels of 255.
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
    block_kind, is_branch, is_canopy, is_cross, is_flat, is_leafy, is_liquid, is_palm_crown, plant_shoot,
    BlockId, Chunk, BLOCK_AIR, BLOCK_BILBERRY, BLOCK_BILBERRY_BARE, BLOCK_BRACKEN, BLOCK_CATTAIL, BLOCK_DIRT,
    BLOCK_FERN, BLOCK_FIREWEED, BLOCK_GRASS, BLOCK_NETTLE, BLOCK_PLANTAIN, BLOCK_STONE, BLOCK_STRAWBERRY,
    BLOCK_STRAWBERRY_BARE, BLOCK_SUNDEW, BLOCK_TALL_GRASS, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
    CHUNK_VOLUME, PLANT_TOP,
};
use primitive_shared::worldgen::{Biome, WorldGen};

const SIZE: (u32, u32) = (1280, 720);

/// Chunks either side of the middle one.
const HALF: i32 = 3;

/// The game's own settings at a desktop's defaults, with the field of view a
/// person standing in a wood sees it at.
fn settings() -> ClientSettings {
    let mut settings = ClientSettings {
        fov_degrees: 80.0,
        anisotropy: 16,
        ..ClientSettings::default()
    };
    settings.sanitize();
    settings
}

/// The middle of the largest, flattest patch of this biome within three
/// kilometres of the world's zero.
fn find(generator: &WorldGen, wanted: Biome) -> Option<(i32, i32)> {
    let mut best: Option<((usize, i32), (i32, i32))> = None;
    for gz in (-3000..3000).step_by(128) {
        for gx in (-3000..3000).step_by(128) {
            if generator.biome_at(gx, gz) != wanted {
                continue;
            }
            let (mut count, mut low, mut high) = (0usize, i32::MAX, i32::MIN);
            for dz in (-48..=48).step_by(12) {
                for dx in (-48..=48).step_by(12) {
                    count += usize::from(generator.biome_at(gx + dx, gz + dz) == wanted);
                    let h = generator.height_at(gx + dx, gz + dz);
                    (low, high) = (low.min(h), high.max(h));
                }
            }
            let key = (count, -(high - low));
            if best.is_none_or(|(held, _)| key > held) {
                best = Some((key, (gx, gz)));
            }
        }
    }
    best.map(|(_, at)| at)
}

/// Every chunk round the one `at` is in.
fn generate(generator: &WorldGen, at: (i32, i32)) -> Vec<Chunk> {
    let (cx, cz) = (at.0.div_euclid(CHUNK_SIZE_X as i32), at.1.div_euclid(CHUNK_SIZE_Z as i32));
    (-HALF..=HALF)
        .flat_map(|dz| (-HALF..=HALF).map(move |dx| ChunkPos::new(cx + dx, cz + dz)))
        .map(|pos| generator.generate_chunk(pos))
        .collect()
}

fn manager(chunks: &[Chunk]) -> ChunkManager {
    let mut manager = ChunkManager::new(128);
    for chunk in chunks {
        manager.insert(Chunk { pos: chunk.pos, blocks: chunk.blocks.clone() });
    }
    manager
}

/// The ground of a column: the highest cell that is not air, a plant, a leaf,
/// wood or water.
fn ground(blocks: &[BlockId], x: usize, z: usize) -> Option<usize> {
    (1..CHUNK_SIZE_Y).rev().find(|&y| {
        let b = blocks[Chunk::index(x, y, z)];
        b != BLOCK_AIR && !is_cross(b) && !is_flat(b) && !is_leafy(b) && !is_branch(b) && !is_liquid(b)
    })
}

fn shaded_in(blocks: &[BlockId], x: usize, ground: usize, z: usize) -> bool {
    (ground + 2..(ground + 29).min(CHUNK_SIZE_Y)).any(|y| {
        let b = blocks[Chunk::index(x, y, z)];
        is_canopy(b) && !is_palm_crown(b)
    })
}

/// The wood with its floor as it was: the meadow's grass under the crowns
/// again. See the module note.
fn taken_back(chunks: &[Chunk]) -> Vec<Chunk> {
    chunks
        .iter()
        .map(|chunk| {
            let mut blocks = chunk.blocks.clone();
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let Some(g) = ground(&blocks, x, z) else { continue };
                    if g + 2 >= CHUNK_SIZE_Y || !shaded_in(&blocks, x, g, z) {
                        continue;
                    }
                    let plant = blocks[Chunk::index(x, g + 1, z)];
                    if matches!(block_kind(plant), BLOCK_FERN | BLOCK_BILBERRY | BLOCK_BRACKEN) {
                        blocks[Chunk::index(x, g + 1, z)] = BLOCK_AIR;
                        if blocks[Chunk::index(x, g + 2, z)] == BLOCK_BRACKEN | PLANT_TOP {
                            blocks[Chunk::index(x, g + 2, z)] = BLOCK_AIR;
                        }
                    }
                    let (gx, gz) = (chunk.pos.x * 16 + x as i32, chunk.pos.z * 16 + z as i32);
                    let roll = (gx.wrapping_mul(7349) ^ gz.wrapping_mul(1931)).rem_euclid(7) == 0;
                    if roll && blocks[Chunk::index(x, g, z)] == BLOCK_GRASS && blocks[Chunk::index(x, g + 1, z)] == BLOCK_AIR {
                        blocks[Chunk::index(x, g + 1, z)] = BLOCK_TALL_GRASS;
                    }
                }
            }
            Chunk { pos: chunk.pos, blocks }
        })
        .collect()
}

/// Meshes of `blocks`, lit as `lit` is.
fn meshes(textures: &TextureManager, generator: &WorldGen, blocks: &[Chunk], lit: &[Chunk]) -> Vec<(ChunkPos, MeshBuffers)> {
    let positions: Vec<ChunkPos> = blocks.iter().map(|chunk| chunk.pos).collect();
    let (blocks, lit) = (manager(blocks), manager(lit));
    let mut light = LightMap::new();
    for &pos in &positions {
        light.load_chunk(&lit, pos);
    }
    let layers = textures.face_layers();
    positions
        .iter()
        .map(|&pos| {
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(pos, &blocks, &light);
            let mut out = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, generator, &mut out);
            (pos, out)
        })
        .collect()
}

/// A column in the middle chunk to stand in: shaded or open as asked, two
/// clear cells over its ground, and a clear run of ten cells north of it at
/// eye height -- a view along the floor rather than into a trunk.
fn stand(chunks: &[Chunk], shaded: bool) -> Option<Vec3> {
    let middle = &chunks[chunks.len() / 2];
    let clear = |b: BlockId| b == BLOCK_AIR || is_cross(b);
    let mut spots: Vec<(usize, usize)> = (1..CHUNK_SIZE_Z - 1).flat_map(|z| (1..CHUNK_SIZE_X - 1).map(move |x| (x, z))).collect();
    spots.sort_by_key(|&(x, z)| (x as i32 - 8).abs() + (z as i32 - 12).abs());
    spots.into_iter().find_map(|(x, z)| {
        let blocks = &middle.blocks;
        let g = ground(blocks, x, z)?;
        if g + 3 >= CHUNK_SIZE_Y || shaded_in(blocks, x, g, z) != shaded {
            return None;
        }
        if !(1..=2).all(|dy| clear(blocks[Chunk::index(x, g + dy, z)])) {
            return None;
        }
        let run = (1..=10.min(z)).filter(|&k| clear(blocks[Chunk::index(x, g + 2, z - k)])).count();
        (run >= 8).then(|| Vec3::new((middle.pos.x * 16) as f32 + x as f32 + 0.5, g as f32 + 2.62, (middle.pos.z * 16) as f32 + z as f32 + 0.5))
    })
}

fn luma(image: &image::RgbaImage) -> f64 {
    let sum: f64 = image
        .pixels()
        .map(|p| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
        .sum();
    sum / f64::from(image.width() * image.height())
}

fn side_by_side(left: &image::RgbaImage, right: &image::RgbaImage) -> image::RgbaImage {
    let mut sheet = image::RgbaImage::new(left.width() + right.width(), left.height().max(right.height()));
    image::imageops::overlay(&mut sheet, left, 0, 0);
    image::imageops::overlay(&mut sheet, right, i64::from(left.width()), 0);
    sheet
}

/// Every new plant on a lawn: the tall ones and a nettle shoot in the back
/// row, the low ones and the picked berries in the front.
fn plant_row() -> Vec<Chunk> {
    const GROUND: usize = 3;
    let back: [(usize, BlockId); 5] = [
        (3, BLOCK_FIREWEED),
        (6, BLOCK_CATTAIL),
        (9, BLOCK_NETTLE),
        (12, BLOCK_BRACKEN),
        (14, plant_shoot(BLOCK_NETTLE)),
    ];
    let front: [(usize, BlockId); 7] = [
        (2, BLOCK_BILBERRY),
        (4, BLOCK_BILBERRY_BARE),
        (6, BLOCK_STRAWBERRY),
        (8, BLOCK_STRAWBERRY_BARE),
        (10, BLOCK_PLANTAIN),
        (12, BLOCK_FERN),
        (14, BLOCK_SUNDEW),
    ];
    let mut chunks = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_GRASS;
                }
            }
            if (cx, cz) == (0, 0) {
                for (x, plant) in back {
                    blocks[Chunk::index(x, GROUND, 6)] = plant;
                    if primitive_shared::types::plant_partner((0, 0, 0), plant).is_some() {
                        blocks[Chunk::index(x, GROUND + 1, 6)] = plant | PLANT_TOP;
                    }
                }
                for (x, plant) in front {
                    blocks[Chunk::index(x, GROUND, 10)] = plant;
                }
            }
            chunks.push(Chunk { pos: ChunkPos::new(cx, cz), blocks });
        }
    }
    chunks
}

#[test]
#[ignore = "a tool: needs a GPU; photographs the forest floor, the birch wood and the wild plants"]
fn what_the_forest_floor_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let shader = include_str!("shader.wgsl").replace("\r\n", "\n");
    let generator = WorldGen::new(1337);
    let draw = |meshes: &[(ChunkPos, MeshBuffers)], eye: Vec3, pitch: f32, hour: f32| {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = (-90.0f32).to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let sky = Sky::new(hour, 900.0);
        super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, &shader, None, None, false, settings.msaa,
        )
    };
    let save = |image: &image::RgbaImage, name: &str| {
        image.save(format!("{out}/{name}.png")).expect("write png");
        println!("{name}: luma {:.1}", luma(image));
    };

    for (name, biome) in [("oak", Biome::Forest), ("taiga", Biome::Taiga)] {
        let Some(at) = find(&generator, biome) else {
            println!("no {name} wood within three kilometres; skipping it");
            continue;
        };
        let after = generate(&generator, at);
        let floor = taken_back(&after);
        let after_meshes = meshes(&textures, &generator, &after, &after);
        let before_meshes = meshes(&textures, &generator, &floor, &floor);
        let Some(eye) = stand(&after, true) else {
            println!("{name} at {at:?}: nowhere under the crowns to stand");
            continue;
        };
        println!("{name} wood at {at:?}, standing at {eye:?}");
        for (when, hour) in [("noon", 0.5f32), ("golden", 0.70)] {
            let before = draw(&before_meshes, eye, -8.0, hour);
            let now = draw(&after_meshes, eye, -8.0, hour);
            save(&before, &format!("{name}_floor_before_{when}"));
            save(&now, &format!("{name}_floor_after_{when}"));
            side_by_side(&before, &now).save(format!("{out}/{name}_floor_sheet_{when}.png")).expect("write png");
        }
        let over = eye + Vec3::new(0.0, 20.0, 18.0);
        let before = draw(&before_meshes, over, -35.0, 0.5);
        let now = draw(&after_meshes, over, -35.0, 0.5);
        save(&before, &format!("{name}_above_before_noon"));
        save(&now, &format!("{name}_above_after_noon"));
        side_by_side(&before, &now).save(format!("{out}/{name}_above_sheet_noon.png")).expect("write png");
    }

    if let Some(at) = find(&generator, Biome::BirchForest) {
        let birch = generate(&generator, at);
        let birch_meshes = meshes(&textures, &generator, &birch, &birch);
        println!("birch wood at {at:?}");
        if let Some(eye) = stand(&birch, true) {
            save(&draw(&birch_meshes, eye, 4.0, 0.5), "birch_floor_after_noon");
        }
        if let Some(eye) = stand(&birch, false) {
            save(&draw(&birch_meshes, eye, 10.0, 0.5), "birch_clearing_after_noon");
        }
    }

    let lawn = plant_row();
    let lawn_meshes = meshes(&textures, &generator, &lawn, &lawn);
    save(&draw(&lawn_meshes, Vec3::new(8.5, 4.9, 15.0), -14.0, 0.5), "plants_noon");
    save(&draw(&lawn_meshes, Vec3::new(8.5, 4.2, 12.6), -32.0, 0.5), "plants_close_noon");
    println!("pictures in {out}");
}
