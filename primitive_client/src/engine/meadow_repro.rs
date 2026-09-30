//! **The meadow, the steppe and the wood, before and after the vegetation
//! pass**, photographed through the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir cargo test --release -p primitive_client --lib \
//!     what_the_meadow_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for "сделай так, чтобы трава и растительность была густой, как в
//! жизни", "добавь перекати-поле" and "добавь больше растений", so each
//! sheet answers one of those, before on the left and after on the right:
//!
//! * `meadow_sheet` -- open grassland at every grass step, OFF (the game as
//!   it was) to LUSH;
//! * `meadow_gale` -- the same meadow standing still and in a gale
//!   (`engine::sway`), the wind blowing across the picture;
//! * `steppe_sheet` -- the steppe before, and after with its new plants,
//!   its blades and three tumbleweeds rolling toward the eye;
//! * `forest_sheet` -- a wood's floor in spring, before and after: the blades
//!   thin to a sprinkling under the crowns, and the bluebells;
//! * `plants` -- every new plant on a lawn, in flower and out of it.
//!
//! **"Before" is this build with the new things taken back by hand**: the
//! second round of plants cut out of the chunks, the blades off, the wind
//! still, no tumbleweeds. A second binary would move everything else too.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::meadow::Density;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::season::Season;
use primitive_shared::types::{
    self as t, block_kind, is_branch, is_canopy, is_cross, is_flat, is_leafy, is_liquid, is_palm_crown, BlockId, Chunk,
    BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::{Biome, Preset, WorldGen, Zone};

const SIZE: (u32, u32) = (1280, 720);
/// Chunks either side of the middle one.
const HALF: i32 = 3;

const SECOND_ROUND: [BlockId; 12] = [
    t::BLOCK_LICHEN,
    t::BLOCK_HEATHER,
    t::BLOCK_ROSEHIP_BUSH,
    t::BLOCK_ROSEHIP_BARE,
    t::BLOCK_YARROW,
    t::BLOCK_THISTLE,
    t::BLOCK_POPPY,
    t::BLOCK_CORNFLOWER,
    t::BLOCK_BLUEBELL,
    t::BLOCK_BUTTERCUP,
    t::BLOCK_TUMBLEWEED,
    t::BLOCK_ROSEHIPS,
];

fn settings(density: Density) -> ClientSettings {
    let mut settings = ClientSettings { fov_degrees: 75.0, anisotropy: 16, ..ClientSettings::default() };
    settings.grass_density = density;
    settings.sanitize();
    settings
}

/// The middle of the largest, flattest patch of this biome within three
/// kilometres of the world's zero (`flora_repro::find`'s rule).
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

fn generate(generator: &WorldGen, at: (i32, i32)) -> Vec<Chunk> {
    let (cx, cz) = (at.0.div_euclid(CHUNK_SIZE_X as i32), at.1.div_euclid(CHUNK_SIZE_Z as i32));
    (-HALF..=HALF)
        .flat_map(|dz| (-HALF..=HALF).map(move |dx| ChunkPos::new(cx + dx, cz + dz)))
        .map(|pos| generator.generate_chunk(pos))
        .collect()
}

/// The chunks with the second round cut out: the world as it grew before.
fn taken_back(chunks: &[Chunk]) -> Vec<Chunk> {
    chunks
        .iter()
        .map(|chunk| Chunk {
            pos: chunk.pos,
            blocks: chunk.blocks.iter().map(|&b| if SECOND_ROUND.contains(&block_kind(b)) { BLOCK_AIR } else { b }).collect(),
        })
        .collect()
}

fn manager(chunks: &[Chunk]) -> ChunkManager {
    let mut manager = ChunkManager::new(128);
    for chunk in chunks {
        manager.insert(Chunk { pos: chunk.pos, blocks: chunk.blocks.clone() });
    }
    manager
}

/// Meshes with the blades at `density` and the flowers in `season`.
fn meshes(
    textures: &TextureManager,
    generator: &WorldGen,
    chunks: &[Chunk],
    density: Density,
    season: Option<Season>,
) -> Vec<(ChunkPos, MeshBuffers)> {
    let positions: Vec<ChunkPos> = chunks.iter().map(|chunk| chunk.pos).collect();
    let blocks = manager(chunks);
    let mut light = LightMap::new();
    for &pos in &positions {
        light.load_chunk(&blocks, pos);
    }
    let layers = textures.face_layers();
    positions
        .iter()
        .map(|&pos| {
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(pos, &blocks, &light);
            cache.grow_blades(density);
            if let Some(season) = season {
                cache.in_season(season);
            }
            let mut out = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, generator, &mut out);
            (pos, out)
        })
        .collect()
}

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

/// A column of the middle chunk to stand in, shaded or open as asked, with a
/// clear run north of it at eye height.
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

/// Tumbleweeds lying on the ground ahead of `eye`, as a mesh of their own.
fn tumbleweeds(textures: &TextureManager, chunks: &[Chunk], eye: Vec3) -> (ChunkPos, MeshBuffers) {
    let pos = ChunkPos::from_world(eye.x.floor() as i32, eye.z.floor() as i32);
    let (ox, oz) = ((pos.x * 16) as f32, (pos.z * 16) as f32);
    let layer = textures.face_layers().layer_for_face(t::BLOCK_TUMBLEWEED, 0);
    let mut out = MeshBuffers::default();
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for (k, (dx, dz)) in [(-1.5f32, -3.2f32), (1.2, -5.0), (3.5, -8.5)].into_iter().enumerate() {
        let (x, z) = ((eye.x + dx).floor() as i32, (eye.z + dz).floor() as i32);
        let Some(chunk) = chunks.iter().find(|c| c.pos == ChunkPos::from_world(x, z)) else { continue };
        let Some(g) = ground(&chunk.blocks, x.rem_euclid(16) as usize, z.rem_euclid(16) as usize) else { continue };
        let middle = Vec3::new(eye.x + dx - ox, g as f32 + 1.0 + crate::logic::tumbleweed_model::RADIUS, eye.z + dz - oz);
        let at = glam::DVec3::new(f64::from(eye.x + dx), f64::from(middle.y), f64::from(eye.z + dz));
        crate::logic::tumbleweed_model::append(middle, crate::logic::tumbleweed_model::turned(at, k as u64), layer, (15, 0), &mut vertices, &mut indices);
    }
    out.vertices = vertices;
    out.leaf_end = 0;
    out.solid_index_count = 0;
    out.sprite_end = indices.len() as u32;
    out.indices = indices;
    (pos, out)
}

fn side_by_side(images: &[&image::RgbaImage]) -> image::RgbaImage {
    let width = images.iter().map(|i| i.width()).sum();
    let height = images.iter().map(|i| i.height()).max().unwrap_or(0);
    let mut sheet = image::RgbaImage::new(width, height);
    let mut x = 0i64;
    for image in images {
        image::imageops::overlay(&mut sheet, *image, x, 0);
        x += i64::from(image.width());
    }
    sheet
}

/// Every new plant on a lawn, a row in flower and a row in winter.
fn plant_rows() -> Vec<Chunk> {
    let row = [
        t::BLOCK_LICHEN,
        t::BLOCK_HEATHER,
        t::BLOCK_ROSEHIP_BUSH,
        t::BLOCK_ROSEHIP_BARE,
        t::BLOCK_YARROW,
        t::BLOCK_THISTLE,
        t::BLOCK_POPPY,
        t::BLOCK_CORNFLOWER,
        t::BLOCK_BLUEBELL,
        t::BLOCK_BUTTERCUP,
    ];
    let mut chunks = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = t::BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = t::BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = t::BLOCK_GRASS;
                }
            }
            if (cx, cz) == (0, 0) {
                for (i, plant) in row.into_iter().enumerate() {
                    blocks[Chunk::index(3 + i, 3, 8)] = plant;
                }
            }
            chunks.push(Chunk { pos: ChunkPos::new(cx, cz), blocks });
        }
    }
    chunks
}

#[test]
#[ignore = "a tool: needs a GPU; photographs the meadow, the steppe and the wood before and after"]
fn what_the_meadow_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, 16).expect("textures load");
    let shader = include_str!("shader.wgsl").replace("\r\n", "\n");
    let seed: u32 = std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1337);
    let draw = |meshes: &[(ChunkPos, MeshBuffers)], eye: Vec3, yaw: f32, pitch: f32, density: Density| {
        let settings = settings(density);
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let sky = Sky::new(0.42, 900.0);
        super::offscreen_repro::draw_scene(device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, &shader, None, None, false, 4)
    };
    let save = |image: &image::RgbaImage, name: &str| {
        image.save(format!("{out}/{name}.png")).expect("write png");
        println!("{name}");
    };
    let temperate = WorldGen::with_zone(seed, Preset::Normal, Zone::Temperate);
    let dry = WorldGen::with_zone(seed, Preset::Normal, Zone::DryBelt);

    // `LAWN_ONLY=1` draws the lawn sheets and stops: the quick look while a
    // picture of a blade is being drawn.
    if std::env::var("LAWN_ONLY").is_ok() {
        let lawn = plant_rows();
        let close = Vec3::new(8.0, 4.4, 14.0);
        let lawns: Vec<image::RgbaImage> = Density::ALL
            .into_iter()
            .map(|d| draw(&meshes(&textures, &temperate, &lawn, d, Some(Season::Summer)), close, -90.0, -30.0, d))
            .collect();
        save(&side_by_side(&lawns.iter().collect::<Vec<_>>()), "lawn_steps");
        return;
    }

    // The meadow at every step, and in a gale.
    if let Some(at) = find(&temperate, Biome::Plains) {
        let after = generate(&temperate, at);
        let before = taken_back(&after);
        if let Some(eye) = stand(&after, false) {
            println!("meadow at {at:?}, eye {eye:?}");
            let old = draw(&meshes(&textures, &temperate, &before, Density::Off, None), eye, -90.0, -12.0, Density::Off);
            let steps: Vec<image::RgbaImage> = [Density::Sparse, Density::Thick, Density::Lush]
                .into_iter()
                .map(|d| draw(&meshes(&textures, &temperate, &after, d, Some(Season::Summer)), eye, -90.0, -12.0, d))
                .collect();
            save(&side_by_side(&[&old, &steps[1]]), "meadow_sheet");
            save(&side_by_side(&[&old, &steps[0], &steps[1], &steps[2]]), "meadow_steps");
            let thick = meshes(&textures, &temperate, &after, Density::Thick, Some(Season::Summer));
            let still = draw(&thick, eye, -90.0, -12.0, Density::Thick);
            let gale = crate::engine::sway::Breeze::at(
                primitive_shared::raft::Wind { toward: 0.0, strength: 1.0 },
                crate::engine::sway::Sway::Full,
                [0.0; 3],
                3.0,
            );
            super::offscreen_repro::OFFSCREEN_BREEZE.with(|b| b.set(Some(gale)));
            let blown = draw(&thick, eye, -90.0, -12.0, Density::Thick);
            super::offscreen_repro::OFFSCREEN_BREEZE.with(|b| b.set(None));
            save(&side_by_side(&[&still, &blown]), "meadow_gale");
        }
    }

    // The steppe, with its tumbleweeds.
    for (generator, zone) in [(&dry, "dry belt"), (&temperate, "temperate")] {
        let Some(at) = find(generator, Biome::Steppe) else { continue };
        let after = generate(generator, at);
        let before = taken_back(&after);
        let Some(eye) = stand(&after, false) else { continue };
        println!("steppe ({zone}) at {at:?}, eye {eye:?}");
        let old = draw(&meshes(&textures, generator, &before, Density::Off, None), eye, -90.0, -10.0, Density::Off);
        let mut new = meshes(&textures, generator, &after, Density::Thick, Some(Season::Spring));
        new.push(tumbleweeds(&textures, &after, eye));
        let now = draw(&new, eye, -90.0, -10.0, Density::Thick);
        save(&side_by_side(&[&old, &now]), "steppe_sheet");
        break;
    }

    // A wood's floor in spring.
    if let Some(at) = find(&temperate, Biome::Forest) {
        let after = generate(&temperate, at);
        let before = taken_back(&after);
        if let Some(eye) = stand(&after, true) {
            println!("wood at {at:?}, eye {eye:?}");
            let old = draw(&meshes(&textures, &temperate, &before, Density::Off, None), eye, -90.0, -10.0, Density::Off);
            let now = draw(&meshes(&textures, &temperate, &after, Density::Thick, Some(Season::Spring)), eye, -90.0, -10.0, Density::Thick);
            save(&side_by_side(&[&old, &now]), "forest_sheet");
        }
    }

    // Every new plant, in flower and in winter.
    let lawn = plant_rows();
    let eye = Vec3::new(8.0, 4.4, 14.5);
    let summer = draw(&meshes(&textures, &temperate, &lawn, Density::Off, Some(Season::Summer)), eye, -90.0, -16.0, Density::Off);
    let winter = draw(&meshes(&textures, &temperate, &lawn, Density::Off, Some(Season::Winter)), eye, -90.0, -16.0, Density::Off);
    save(&side_by_side(&[&summer, &winter]), "plants");
    // ...and the same lawn at every grass step, close, looking down: the
    // blades themselves, where nothing else is in the way.
    let close = Vec3::new(8.0, 4.4, 14.0);
    let lawns: Vec<image::RgbaImage> = Density::ALL
        .into_iter()
        .map(|d| draw(&meshes(&textures, &temperate, &lawn, d, Some(Season::Summer)), close, -90.0, -30.0, d))
        .collect();
    save(&side_by_side(&lawns.iter().collect::<Vec<_>>()), "lawn_steps");
    println!("pictures in {out}");
}
