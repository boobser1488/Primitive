//! **Why the picture reads as plastic**, photographed from where a player
//! stands, at four times of day and from four bearings.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/look \
//!     cargo test -p primitive_client --release --lib \
//!     what_the_world_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for the report "игра выглядит пластиково". Plastic is not one
//! fault, it is the absence of several small ones, and each of them is a
//! claim about a picture: whether a pond has a highlight on it, whether a
//! leaf lit from behind glows, whether a hillside at sunset is warm on one
//! face and cool on the other. So this takes the same seats before and after
//! and the pair is compared by eye and by number.
//!
//! **The seats are the player's, not a turntable's.** A meadow at eye height
//! is the picture the complaint is about; an overhead shot flatters every
//! lighting model there is, because it never shows two faces of the same
//! block at once.
//!
//! A child of `renderer` for the reason `lod_repro` is, and it borrows
//! `view_distance_repro`'s streamer, mesher and settings so that "the
//! player's world at the player's settings" means one thing in both files.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory, and a relative one lands there.

use super::view_distance_repro::{fog_for, mesh_world, players_settings, stream};
use super::*;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::types::ChunkPos;
use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

const SEED: u32 = 32;
const SIZE: (u32, u32) = (1280, 720);
const SHADER: &str = include_str!("shader.wgsl");

/// The four surface terms, and what each of them is at nought.
///
/// **"Before" is this build with the terms switched off**, the way
/// `view_distance_repro` draws its old fog: same mesh, same sky, same
/// camera, same pixels everywhere the change does not reach. A separate
/// checkout would have differed in a dozen things nobody was asking about.
///
/// Each line has to be found exactly once, or a control whose edit silently
/// missed would draw the game's own frame and be read as proof that the term
/// changes nothing.
const TERMS: [(&str, &str); 5] = [
    ("const AIR_DEPTH: f32 = 0.34;", "const AIR_DEPTH: f32 = 0.0;"),
    ("const WATER_SHEEN: f32 = 0.55;", "const WATER_SHEEN: f32 = 0.0;"),
    ("const WATER_GLITTER: f32 = 0.75;", "const WATER_GLITTER: f32 = 0.0;"),
    ("const WET_DARKEN: f32 = 0.70;", "const WET_DARKEN: f32 = 1.0;"),
    ("const LEAF_GLOW: f32 = 0.55;", "const LEAF_GLOW: f32 = 0.0;"),
];
/// ...and the fifth, which is in the vertex stage: a model's ambient coming
/// from the sky above it rather than from everywhere at once.
const MODEL_TERM: (&str, &str) = ("const MODEL_SKY_FILL: f32 = 0.7;", "const MODEL_SKY_FILL: f32 = 0.0;");

/// The terrain shader with every surface term at nought: the world as it
/// looked when the report came in.
fn before() -> String {
    let mut source = SHADER.to_string();
    for (on, off) in TERMS.iter().chain(std::iter::once(&MODEL_TERM)) {
        assert_eq!(source.matches(on).count(), 1, "shader.wgsl no longer carries `{on}` exactly once");
        source = source.replace(on, off);
    }
    source
}

/// How many pixels of `a` differ from `b` by more than `threshold` in any
/// channel, and a mask painting them.
fn difference(a: &image::RgbaImage, b: &image::RgbaImage) -> (u32, image::RgbaImage) {
    let mut mask = image::RgbaImage::new(a.width(), a.height());
    let mut changed = 0;
    for (x, y, pixel) in a.enumerate_pixels() {
        let other = b.get_pixel(x, y);
        let apart = (0..3).map(|c| (pixel[c] as i32 - other[c] as i32).abs()).max().unwrap_or(0);
        if apart > 3 {
            changed += 1;
        }
        let heat = (apart * 8).min(255) as u8;
        mask.put_pixel(x, y, image::Rgba([heat, heat / 3, 0, 255]));
    }
    (changed, mask)
}

/// The hours the light is worth looking at: a flat grey-white midday, which
/// is where plastic shows worst; mid-morning, which is the game's ordinary
/// light; golden hour and sunset, where a warm key against a cool fill is
/// the whole of what makes a surface read as a surface.
const HOURS: [(&str, f32); 4] = [("noon", 0.5), ("morning", 0.40), ("golden", 0.30), ("dusk", 0.24)];

/// North, east, south and west, as the camera's yaw in degrees.
const COMPASS: [(&str, f32); 4] = [("n", -90.0), ("e", 0.0), ("s", 90.0), ("w", 180.0)];

#[allow(clippy::too_many_arguments)]
fn seat_shot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    settings: &ClientSettings,
    sky: &Sky,
    meshes: &[(ChunkPos, crate::engine::mesh::MeshBuffers)],
    eye: Vec3,
    (yaw, pitch): (f32, f32),
    source: &str,
) -> image::RgbaImage {
    let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
    camera.yaw = yaw.to_radians();
    camera.pitch = pitch.to_radians();
    camera.fov_y_radians = settings.fov_degrees.to_radians();
    super::offscreen_repro::draw_scene(
        device,
        queue,
        textures,
        settings,
        &camera,
        sky,
        meshes,
        SIZE,
        source,
        Some(ChunkManager::reach_blocks(settings.render_distance_chunks)),
        None,
        false,
        settings.msaa,
    )
}

/// The spread of a frame's luminance and of its saturation.
///
/// **Both, because plastic is low on one and high on the other.** A picture
/// whose whole range of light is fifty levels of 255 is one lit by nothing in
/// particular; a picture whose every pixel has the same saturation is one
/// where the light contributed no colour of its own.
fn spread(picture: &image::RgbaImage) -> (f32, f32) {
    let mut lumas = Vec::with_capacity(picture.len() / 4);
    let mut sats = Vec::with_capacity(picture.len() / 4);
    for p in picture.pixels() {
        let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
        lumas.push(0.2126 * r + 0.7152 * g + 0.0722 * b);
        let hi = r.max(g).max(b);
        let lo = r.min(g).min(b);
        sats.push(if hi > 1.0 { (hi - lo) / hi } else { 0.0 });
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let sd = |v: &[f32]| {
        let m = mean(v);
        (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
    };
    (sd(&lumas), sd(&sats))
}

#[test]
#[ignore = "a tool: needs a GPU; photographs the world at four hours from four bearings"]
fn what_the_world_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");

    let generator = WorldGen::new(SEED);
    let (sx, sz) = generator.spawn_column();
    let spawn_chunk = ChunkPos::from_global(sx, sz).0;
    let world = stream(spawn_chunk, settings.render_distance_chunks);
    let layers = textures.face_layers();
    let meshed = mesh_world(&world, &layers, &settings, spawn_chunk, true);
    let ground = generator.height_at(sx, sz).max(SEA_LEVEL) as f32;
    let eye = Vec3::new(sx as f32 + 0.5, ground + 2.62, sz as f32 + 0.5);
    println!("spawn ({sx}, {sz}), eye at {eye:?}, {} chunks", world.positions.len());

    // `LOOK_ONLY=land|water|rain` photographs one part of it: each half
    // streams and meshes its own two hundred chunks, and trying a number is
    // a minute of that or five.
    let only = std::env::var("LOOK_ONLY").unwrap_or_default();
    let wanted = |part: &str| only.is_empty() || only == part;
    let dull = before();
    for (hour_name, hour) in HOURS {
        if !wanted("land") {
            break;
        }
        let sky = Sky::new(hour, 900.0);
        let fog = fog_for(&settings, &sky, false);
        println!("{hour_name}: fog {:.0}..{:.0}", fog.start, fog.end);
        for (name, yaw) in COMPASS {
            for (seat, from, look) in
                [("", eye, (yaw, -8.0)), ("high_", eye + Vec3::Y * 30.0, (yaw, -12.0))]
            {
            let name = format!("{seat}{name}");
            let shoot = |source: &str| {
                seat_shot(device, queue, &textures, &settings, &sky, &meshed.meshes, from, look, source)
            };
            let old = shoot(&dull);
            let new = shoot(SHADER);
            let (changed, mask) = difference(&new, &old);
            let (old_luma, old_sat) = spread(&old);
            let (luma_sd, sat_sd) = spread(&new);
            println!(
                "  {hour_name}_{name}: luminance sd {old_luma:5.1} -> {luma_sd:5.1}, \
                 saturation sd {old_sat:5.3} -> {sat_sd:5.3}, {:.1}% of the frame changed",
                changed as f32 * 100.0 / (SIZE.0 * SIZE.1) as f32
            );
            old.save(format!("{out}/{hour_name}_{name}_before.png")).expect("write png");
            new.save(format!("{out}/{hour_name}_{name}_after.png")).expect("write png");
            mask.save(format!("{out}/{hour_name}_{name}_changed.png")).expect("write png");
            }
        }
    }

    // **The shore, which is where the water is.** A meadow seat photographs
    // the sheen on nothing: the sea is past the fog from spawn and a pond
    // is a dozen pixels. This walks out to the nearest open water and stands
    // on its bank at the two hours a highlight is worth anything -- a low sun
    // for the path it lays across the surface, and a high one for the patch.
    if let Some((wx, wz)) = open_water(&generator, sx, sz).filter(|_| wanted("water")) {
        let shore_chunk = ChunkPos::from_global(wx, wz).0;
        let shore_world = stream(shore_chunk, settings.render_distance_chunks);
        let shore = mesh_world(&shore_world, &layers, &settings, shore_chunk, true);
        let bank = Vec3::new(wx as f32 + 0.5, SEA_LEVEL as f32 + 3.62, wz as f32 + 0.5);
        println!("water at ({wx}, {wz}), bank eye at {bank:?}");
        for (hour_name, hour) in [("golden", 0.30f32), ("noon", 0.5)] {
            let sky = Sky::new(hour, 900.0);
            for (name, yaw) in COMPASS {
                let shoot = |source: &str| {
                    seat_shot(device, queue, &textures, &settings, &sky, &shore.meshes, bank, (yaw, -12.0), source)
                };
                let old = shoot(&dull);
                let new = shoot(SHADER);
                let (changed, mask) = difference(&new, &old);
                println!(
                    "  water_{hour_name}_{name}: {:.1}% of the frame changed",
                    changed as f32 * 100.0 / (SIZE.0 * SIZE.1) as f32
                );
                old.save(format!("{out}/water_{hour_name}_{name}_before.png")).expect("write png");
                new.save(format!("{out}/water_{hour_name}_{name}_after.png")).expect("write png");
                mask.save(format!("{out}/water_{hour_name}_{name}_changed.png")).expect("write png");
            }
        }
    } else {
        println!("no open water within reach of spawn; the water seats are skipped");
    }

    // **The same meadow, raining.** The sky has to be ticked into the
    // shower rather than told to be in one: the deck closes before the first
    // drop, and `FrameParams::rain` is what has actually arrived.
    if !wanted("rain") {
        println!("pictures in {out}");
        return;
    }
    let mut wet_sky = Sky::new(0.40, 100_000.0);
    wet_sky.set_weather(primitive_shared::weather::Weather::Rain);
    for _ in 0..600 {
        wet_sky.tick(0.5);
    }
    println!("rain: {:.2} arrived, overcast {:.2}", wet_sky.rain_arrived(), wet_sky.overcast());
    for (name, yaw) in COMPASS {
        let dry = Sky::new(0.40, 900.0);
        let dry_picture =
            seat_shot(device, queue, &textures, &settings, &dry, &meshed.meshes, eye, (yaw, -8.0), SHADER);
        let wet_picture =
            seat_shot(device, queue, &textures, &settings, &wet_sky, &meshed.meshes, eye, (yaw, -8.0), SHADER);
        let dull_wet =
            seat_shot(device, queue, &textures, &settings, &wet_sky, &meshed.meshes, eye, (yaw, -8.0), &dull);
        dry_picture.save(format!("{out}/rain_{name}_dry.png")).expect("write png");
        dull_wet.save(format!("{out}/rain_{name}_before.png")).expect("write png");
        wet_picture.save(format!("{out}/rain_{name}_after.png")).expect("write png");
    }
    println!("pictures in {out}");
}

/// The nearest column of open water to `(x, z)`, within four hundred blocks
/// and found on a four-block grid, or `None`.
///
/// **Open water and not a puddle**: a column whose neighbours are water too,
/// so the seat looks out over a surface rather than down a well.
fn open_water(generator: &WorldGen, x: i32, z: i32) -> Option<(i32, i32)> {
    let mut best: Option<(i32, (i32, i32))> = None;
    for dz in (-400..=400).step_by(4) {
        for dx in (-400..=400).step_by(4) {
            let (cx, cz) = (x + dx, z + dz);
            let wide = [(0, 0), (3, 0), (-3, 0), (0, 3), (0, -3)]
                .iter()
                .all(|(ox, oz)| generator.height_at(cx + ox, cz + oz) < SEA_LEVEL - 1);
            if !wide {
                continue;
            }
            let away = dx * dx + dz * dz;
            if best.is_none_or(|(near, _)| away < near) {
                best = Some((away, (cx, cz)));
            }
        }
    }
    // Standing on the bank, not in the sea: back off toward the seat.
    best.map(|(_, (cx, cz))| {
        let (mut bx, mut bz) = (cx, cz);
        while generator.height_at(bx, bz) < SEA_LEVEL && (bx - x).abs() + (bz - z).abs() > 1 {
            bx -= (bx - x).signum();
            bz -= (bz - z).signum();
        }
        (bx, bz)
    })
}

// ---- the properties, on a scene small enough to run in the suite ----
//
// **Nine chunks built by hand, not a world**: the tool above streams six
// hundred of them at the player's render distance, which is a tool's budget
// and not a test's. What these need is a pool with a bank to stand on, a
// sealed chamber with a lamp in it, and open grass; a seat in front of each;
// and the same frame drawn twice, once with the term under test and once
// without.

/// A grass plain, a pool cut into one half of the middle chunk and a sealed
/// lit chamber inside the other, as nine chunks round the origin.
fn pool_and_cave() -> (ChunkManager, primitive_shared::lighting::LightMap) {
    use primitive_shared::types::{
        Chunk, BLOCK_AIR, BLOCK_DIRT, BLOCK_GLOWSTONE, BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER,
        CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
    };
    let mut chunks = ChunkManager::new(16);
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 3, z)] = BLOCK_GRASS;
                    if (cx, cz) != (0, 0) {
                        continue;
                    }
                    if x < 10 {
                        // The pool: three blocks deep, so its lid carries a
                        // depth past one and is the face the sheen is on.
                        for y in 1..=3 {
                            blocks[Chunk::index(x, y, z)] = BLOCK_WATER;
                        }
                    } else {
                        // ...and a hill of stone to cut the chamber out of.
                        for y in 4..=8 {
                            blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                        }
                    }
                }
            }
            if (cx, cz) == (0, 0) {
                // A room with no way out and a lamp in its floor: what the
                // rain must not reach. Its walls are the stone left standing
                // at x = 10 and x = 15 and at both ends of the chunk.
                for z in 1..CHUNK_SIZE_Z - 1 {
                    for x in 11..15 {
                        blocks[Chunk::index(x, 4, z)] = BLOCK_AIR;
                        blocks[Chunk::index(x, 5, z)] = BLOCK_AIR;
                    }
                }
                blocks[Chunk::index(12, 3, 8)] = BLOCK_GLOWSTONE;
            }
            chunks.insert(Chunk { pos, blocks });
        }
    }
    let mut light = primitive_shared::lighting::LightMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    (chunks, light)
}

/// `pool_and_cave`, meshed.
fn pool_meshes(textures: &TextureManager) -> Vec<(ChunkPos, crate::engine::mesh::MeshBuffers)> {
    use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
    let (chunks, light) = pool_and_cave();
    let generator = WorldGen::new(0);
    let layers = textures.face_layers();
    let mut out = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            out.push((pos, buffers));
        }
    }
    out
}

/// The settings these tests draw with, and a frame small enough that nine
/// chunks is the whole of it.
fn plain_settings() -> ClientSettings {
    let mut settings = ClientSettings { render_distance_chunks: 4, msaa: 1, ..ClientSettings::default() };
    settings.sanitize();
    settings
}

const SMALL: (u32, u32) = (320, 180);

/// A frame's mean luminance.
fn mean_luma(picture: &image::RgbaImage) -> f32 {
    let mut total = 0.0;
    for p in picture.pixels() {
        total += 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    }
    total / picture.pixels().len() as f32
}

#[allow(clippy::too_many_arguments)]
fn small_shot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    settings: &ClientSettings,
    sky: &Sky,
    meshes: &[(ChunkPos, crate::engine::mesh::MeshBuffers)],
    eye: Vec3,
    (yaw, pitch): (f32, f32),
    source: &str,
) -> image::RgbaImage {
    let mut camera = Camera::new(eye.as_dvec3(), SMALL.0 as f32 / SMALL.1 as f32);
    camera.yaw = yaw.to_radians();
    camera.pitch = pitch.to_radians();
    camera.fov_y_radians = settings.fov_degrees.to_radians();
    super::offscreen_repro::draw_scene(
        device, queue, textures, settings, &camera, sky, meshes, SMALL, source, None, None, false, 1,
    )
}

/// The one term `off` names, switched off, and nothing else.
fn without(off: (&str, &str)) -> String {
    assert_eq!(SHADER.matches(off.0).count(), 1, "shader.wgsl no longer carries `{}` exactly once", off.0);
    SHADER.replace(off.0, off.1)
}

/// **The Fresnel half of the water sheen.**
///
/// Water is a window looked into and a mirror looked along. Both seats see
/// the same pool through the same shader; what differs is the angle, and the
/// term has to be most of the picture at a grazing one and almost none of it
/// from overhead. A sheen that was a flat tint -- which is what a
/// Fresnel-free highlight is -- would pass a test that looked at one seat.
#[test]
fn water_looked_along_takes_the_sky_and_water_looked_into_keeps_its_bed() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = plain_settings();
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let meshes = pool_meshes(&textures);
    let sky = Sky::new(0.35, 900.0);
    let dull = without(TERMS[1]);
    let shoot = |eye: Vec3, look: (f32, f32), source: &str| {
        mean_luma(&small_shot(device, queue, &textures, &settings, &sky, &meshes, eye, look, source))
    };
    // Along the surface from the west bank, and straight down into it.
    let graze = (Vec3::new(-4.5, 5.2, 8.5), (0.0f32, -6.0));
    let into = (Vec3::new(4.5, 15.0, 8.5), (0.0f32, -80.0));
    let along_change = (shoot(graze.0, graze.1, SHADER) - shoot(graze.0, graze.1, &dull)).abs();
    let down_change = (shoot(into.0, into.1, SHADER) - shoot(into.0, into.1, &dull)).abs();
    assert!(
        along_change > 1.0,
        "water looked along is the same picture with the sheen as without it: {along_change:.2} levels"
    );
    assert!(
        along_change > down_change * 3.0,
        "the sheen is a flat tint rather than a Fresnel one: {along_change:.2} levels along the \
         surface against {down_change:.2} looking into it"
    );
}

/// **Rain wets what it falls on, and only what it falls on.**
///
/// The same shower, drawn twice with nothing between the two frames but the
/// wetting itself -- a storm takes light out of the sky as well, and
/// comparing a wet world with a dry one measures both at once. Outside, the
/// ground has to darken; in a sealed room with a lamp in it, the frame has
/// to come out identical. A term applied to every fragment -- the easy
/// mistake, and the cheaper one -- would dim a cave during a shower on the
/// surface, and nobody standing in it could say why the light had changed.
#[test]
fn a_meadow_in_the_rain_is_darker_and_a_lit_cave_under_it_is_untouched() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = plain_settings();
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let meshes = pool_meshes(&textures);
    let dry = without(TERMS[3]);
    // `rain_arrived` is what the ground is wet by, and it is nought until
    // the deck has closed -- so the sky is ticked into the shower rather
    // than told to be in one.
    let mut sky = Sky::new(0.40, 100_000.0);
    sky.set_weather(primitive_shared::weather::Weather::Rain);
    for _ in 0..600 {
        sky.tick(0.5);
    }
    assert!(sky.rain_arrived() > 0.9, "the shower never arrived: {}", sky.rain_arrived());
    let shoot = |eye: Vec3, look: (f32, f32), source: &str| {
        mean_luma(&small_shot(device, queue, &textures, &settings, &sky, &meshes, eye, look, source))
    };
    let meadow = (Vec3::new(20.5, 5.6, 20.5), (0.0f32, -30.0));
    let cave = (Vec3::new(13.5, 4.6, 8.5), (0.0f32, 0.0));
    let (meadow_dry, meadow_wet) = (shoot(meadow.0, meadow.1, &dry), shoot(meadow.0, meadow.1, SHADER));
    let (cave_dry, cave_wet) = (shoot(cave.0, cave.1, &dry), shoot(cave.0, cave.1, SHADER));
    assert!(
        meadow_wet < meadow_dry * 0.95,
        "the meadow is as bright in the rain as it is dry: {meadow_dry:.1} -> {meadow_wet:.1}"
    );
    assert!(
        (cave_wet - cave_dry).abs() < 0.05,
        "rain on the surface wetted a sealed room under it: {cave_dry:.2} -> {cave_wet:.2}"
    );
}

/// **Every stage that draws a model puts its normal through
/// `model_openness`**, and the ground's does not.
///
/// A model's vertices carry no occlusion to speak of -- there are no blocks
/// round them to count -- so which way a surface faces is the only thing
/// there is to shade a cuboid with, and a pipeline that forgets it draws a
/// flat toy beside three that do not. The ground is the other way round: the
/// mesher counted real neighbours there, and a hemisphere laid over them
/// would darken the underside of every overhang twice.
#[test]
fn every_stage_that_draws_a_model_takes_its_ambient_from_the_sky_above_it() {
    // Without the carriage returns a checkout on Windows carries: a stage
    // is found by the line its closing brace is alone on.
    let source = SHADER.replace('\r', "");
    let stage = |name: &str| {
        let at = source.find(name).unwrap_or_else(|| panic!("{name} is gone from shader.wgsl"));
        let body = &source[at..];
        let end = body.find("\n}\n").expect("the stage ends somewhere");
        body[..end].to_string()
    };
    for name in ["fn item_vertex", "fn vs_held", "fn vs_actor"] {
        assert!(
            stage(name).contains("model_openness("),
            "{name} lights every face of a model the same: no `model_openness`"
        );
    }
    assert!(
        !stage("fn terrain_vertex").contains("model_openness("),
        "the ground is being given a model's hemisphere on top of the occlusion the mesher counted"
    );
}

/// **The before picture is a picture of before.**
///
/// `look_repro` draws "before" by switching the surface terms off in this
/// build's own shader, and a line whose text has drifted would be replaced
/// nowhere at all -- so the tool would photograph the game's own frame twice
/// and the pair would agree, which reads as "the change does nothing".
#[test]
fn every_surface_term_can_still_be_switched_off_for_the_before_picture() {
    let dull = before();
    assert_ne!(dull, SHADER, "nothing was switched off");
    for (_, off) in TERMS.iter().chain(std::iter::once(&MODEL_TERM)) {
        assert!(dull.contains(off), "`{off}` never reached the before shader");
    }
}

/// **What the surface terms cost, in fragment time.**
///
/// ```text
/// GPU_REPRO_DIR=... cargo test -p primitive_client --release --lib \
///     what_the_surfaces_cost -- --ignored --nocapture
/// ```
///
/// **Timed by the slope and not by the clock**, because `draw_scene` builds
/// its pipelines every call and compiling the terrain shader is tens of
/// milliseconds -- twenty times what a pass costs. The one thing that scales
/// with the frame's area is the fragment work, so the same seat is drawn at
/// two sizes and what is reported is the difference divided by the
/// difference in pixels. Everything else in the call -- the compile, the
/// upload, the vertex work, the read-back setup -- is the same at both sizes
/// and cancels exactly.
///
/// The minimum of the runs and not the mean: a GPU shared with five other
/// builds gives a distribution with a hard floor and a long tail, and the
/// floor is the only part of it that is about this shader.
///
/// **What it said**, on a GTX 1050 Ti, release, twenty-four interleaved
/// rounds over the pool seat -- where water is a third of the frame and so
/// pays the dearest of the five terms:
///
/// ```text
///  before:  15.06 ms at 640x360,  22.55 at 1920x1080 ->  8.42 ms
///   after:  15.48 ms at 640x360,  22.88 at 1920x1080 ->  8.33 ms
/// ```
///
/// The two are the same figure: the difference is nine hundredths of a
/// millisecond *the wrong way*, and run to run this method moves by three
/// tenths. So what the terms cost is under what it can resolve, which is
/// what eleven more multiplies on a fragment that already takes a filtered
/// array fetch ought to cost. No new sample, no derivative, no transcendental
/// function; the model term is in a vertex stage.
#[test]
#[ignore = "a tool: needs a GPU; times the fragment cost of the surface terms"]
fn what_the_surfaces_cost() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = plain_settings();
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let meshes = pool_meshes(&textures);
    let sky = Sky::new(0.35, 900.0);
    // Across the pool from the bank, so water, ground and sky all have a
    // share of the frame -- and the water's share is the one paying most.
    let eye = Vec3::new(-4.5, 5.2, 8.5);
    let one = |size: (u32, u32), source: &str| {
        let mut camera = Camera::new(eye.as_dvec3(), size.0 as f32 / size.1 as f32);
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let started = std::time::Instant::now();
        let _ = super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, size, source, None, None,
            false, 1,
        );
        device.poll(wgpu::Maintain::Wait);
        started.elapsed().as_secs_f64() * 1000.0
    };
    let small = (640u32, 360u32);
    let big = (1920u32, 1080u32);
    let pixels = (big.0 * big.1 - small.0 * small.1) as f64;
    let dull = before();
    // **Interleaved**, one round of all four measurements at a time: a
    // machine six builds are sharing drifts over a minute, and two figures
    // taken a minute apart differ by the drift and not by the shader.
    let mut best = [f64::MAX; 4];
    for _ in 0..24 {
        for (i, (size, source)) in
            [(small, dull.as_str()), (big, dull.as_str()), (small, SHADER), (big, SHADER)]
                .into_iter()
                .enumerate()
        {
            best[i] = best[i].min(one(size, source));
        }
    }
    for (name, (a, b)) in [("before", (best[0], best[1])), ("after", (best[2], best[3]))] {
        let per_frame = (b - a) / pixels * (big.0 * big.1) as f64;
        println!("{name:>7}: {a:6.2} ms at 640x360, {b:6.2} at 1920x1080 -> {per_frame:5.2} ms of fragment work a 1080p frame");
    }
}
