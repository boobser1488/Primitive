//! **What a grassy slope of the landforms looks like**, from where a player
//! stands, from the middle distance and from far off, under a low sun --
//! through the real mesher and the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir TERRAIN_TAG=after \
//!     cargo test -p primitive_client --lib what_the_slopes_look_like -- --ignored --nocapture
//! ```
//!
//! `TERRAIN_ONLY=<subject>` takes one subject of `SUBJECTS`; `TERRAIN_TAG`
//! ends every file name, so a "before" and an "after" land side by side;
//! `TERRAIN_FIND=1` looks for new subjects round each world's spawn instead
//! of photographing the pinned ones. Each subject prints a `[triangles]`
//! line: the disc's triangles a chunk, which is what the ground costs a
//! phone's solid pass.
//!
//! **The "before" of the pictures in `CHANGELOG.md`** was this file run
//! against the engine as it was -- `lips.rs`, `landforms.rs`,
//! `worldgen.rs`, `texture.rs` and `mesh.rs` reversed to the parent commit
//! and nothing else -- because a switch for every one of those in the
//! shipping client would be five switches kept for one comparison.
//!
//! Written for "плавные переходы на траве выглядят как ступеньки какие-то
//! однотипные и сливаются в кашу". The lips (`worldgen::lips`) are the
//! quarter-block turf steps the generator ramps every rise with; the report
//! is about how a hillside of them *reads*, and a hillside reads differently
//! at every distance -- a step is geometry at three blocks, a stripe at
//! thirty and a texture at a hundred -- so one picture from one seat proves
//! nothing about the other two. Every subject is taken from three seats on
//! one line down the fall of the ground, looking up it, which is the way the
//! risers face.
//!
//! **The seats are pinned, not searched for**, so a picture taken before a
//! change and one taken after are of the same ground from the same eye: the
//! subjects are world coordinates in `SUBJECTS`, and the eye's height is
//! read off the ground under it in whichever build is running.
//!
//! The player's own settings are read from `client_settings.toml` beside the
//! repository when there is one (fov, anisotropy, multisampling, render
//! distance): a picture at the defaults is a picture from a seat nobody who
//! reported the fault was sitting in.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::view_distance_repro::{in_parallel, mesh_world, players_settings, World};
use super::*;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{block_kind, is_collidable, is_liquid, ChunkPos, CHUNK_SIZE_Y};
use primitive_shared::worldgen::{Preset, WorldGen, Zone};

const SIZE: (u32, u32) = (1280, 720);
const SHADER: &str = include_str!("shader.wgsl");

/// A low morning sun, which is the light a step is seen by or lost in: at
/// noon every face that is not a top is lit by the fill alone and a riser is
/// a dark line whatever it is made of.
const HOURS: [(&str, f32); 2] = [("low", 0.30), ("noon", 0.5)];

/// One subject: a world, a point on its ground, and which way the eye looks
/// at it from (the yaw from the eye to the point, in degrees).
#[derive(Clone, Copy)]
struct Subject {
    name: &'static str,
    seed: u32,
    zone: Zone,
    at: (i32, i32),
    bearing: f32,
}

/// Found by `TERRAIN_FIND=1` on the ground as it was when the report came
/// in, and pinned from then on so the pictures after a change are of the
/// same places.
const SUBJECTS: &[Subject] = &[
    // The gentle meadow slope with the most lips round the spawn of 1337.
    Subject { name: "meadow", seed: 1337, zone: Zone::Temperate, at: (16, -52), bearing: 27.0 },
    // A down of the hill country: slopes that are lips from foot to crown.
    Subject { name: "downs", seed: 1337, zone: Zone::Temperate, at: (-29_946, -29_986), bearing: 180.0 },
    Subject { name: "hill", seed: 1337, zone: Zone::Temperate, at: (-29_934, -29_974), bearing: 45.0 },
    // A brook's bank with turf running down to it.
    Subject { name: "bank", seed: 1337, zone: Zone::Temperate, at: (0, 64), bearing: -45.0 },
    // The places (`worldgen::landforms`, "What makes a place a place"),
    // found by `probe_places`: a tor in the open country of 1337, one in its
    // hill country and a combe there. Nothing stands at any before the change, which is
    // what the "before" of these pictures shows.
    Subject { name: "tor", seed: 1337, zone: Zone::Temperate, at: (72, -184), bearing: 30.0 },
    Subject { name: "hilltor", seed: 1337, zone: Zone::Temperate, at: (-29_554, -30_568), bearing: 60.0 },
    Subject { name: "combe", seed: 1337, zone: Zone::Temperate, at: (-29_898, -30_590), bearing: 90.0 },
    // The player's own last world, by its spawn.
    Subject { name: "owner", seed: 4_235_635_179, zone: Zone::Tropics, at: (16, -16), bearing: -117.0 },
];

/// Four seats on one line toward the subject: near (eye height, seven blocks
/// off), middle (thirty off, a little raised), far (ninety off, from a rise),
/// and over (thirty-five off and twenty-two up), which is the one that shows
/// the shape of a place over the trees that stand between it and the others.
const SEATS: [(&str, f32, f32); 4] =
    [("near", 7.0, 1.62), ("mid", 30.0, 4.0), ("far", 90.0, 14.0), ("over", 35.0, 22.0)];

fn stream_world(generator: WorldGen, centre: ChunkPos, radius: i32) -> World {
    let probe = ChunkManager::new(radius);
    let mut positions = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if probe.inside(dx, dz) {
                positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
            }
        }
    }
    let generated = in_parallel(&positions, |&pos| generator.generate_chunk(pos));
    let isolated = in_parallel(&generated, |chunk| primitive_shared::lighting::compute_isolated(&chunk.blocks));
    let mut chunks = ChunkManager::new(radius);
    for chunk in generated {
        chunks.insert(chunk);
    }
    let mut light = LightMap::new();
    for (pos, data) in positions.iter().zip(isolated) {
        light.insert_precomputed(&chunks, *pos, data);
    }
    World { generator, chunks, light, positions }
}

/// The top of the ground at a column of a streamed world: the highest cell a
/// body stands on or water fills, and the block there.
fn top(world: &World, x: i32, z: i32) -> Option<(i32, primitive_shared::types::BlockId)> {
    (1..CHUNK_SIZE_Y as i32).rev().find_map(|y| {
        let block = world.chunks.block_at(x, y, z)?;
        (is_collidable(block) || is_liquid(block)).then_some((y, block))
    })
}

/// How many turf lips lie within `reach` of a column, and how much the
/// ground rises and falls there: the score a slope subject is found by.
fn lips_round(world: &World, x: i32, z: i32, reach: i32) -> (u32, i32, bool) {
    let (mut lips, mut low, mut high, mut wet) = (0, i32::MAX, i32::MIN, false);
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let Some((y, block)) = top(world, x + dx, z + dz) else { continue };
            wet |= is_liquid(block);
            lips += u32::from(primitive_shared::dig::is_turf_lip(block));
            (low, high) = (low.min(y), high.max(y));
        }
    }
    (lips, high - low, wet)
}

/// The yaw from the lower side of a slope to its upper: the risers face
/// down it, so that is the way they are seen.
fn uphill(generator: &WorldGen, (x, z): (i32, i32)) -> f32 {
    let h = |dx: i32, dz: i32| generator.height_at(x + dx, z + dz) as f32;
    let (gx, gz) = (h(8, 0) - h(-8, 0), h(0, 8) - h(0, -8));
    gz.atan2(gx).to_degrees()
}

#[allow(clippy::too_many_arguments)]
fn shot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    settings: &ClientSettings,
    sky: &Sky,
    meshes: &[(ChunkPos, crate::engine::mesh::MeshBuffers)],
    eye: Vec3,
    at: Vec3,
) -> image::RgbaImage {
    let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
    let d = at - eye;
    camera.yaw = d.z.atan2(d.x);
    camera.pitch = d.y.atan2(d.x.hypot(d.z));
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
        SHADER,
        Some(ChunkManager::reach_blocks(settings.render_distance_chunks)),
        None,
        false,
        settings.msaa.max(1),
    )
}

/// The middle of a picture, `factor` times, nearest neighbour: a step a few
/// pixels tall is not something to argue about by eye at 1:1.
fn loupe(picture: &image::RgbaImage, factor: u32) -> image::RgbaImage {
    let (w, h) = (picture.width() / factor, picture.height() / factor);
    let (x0, y0) = ((picture.width() - w) / 2, (picture.height() - h) / 2);
    image::RgbaImage::from_fn(w * factor, h * factor, |x, y| *picture.get_pixel(x0 + x / factor, y0 + y / factor))
}

/// A world to look in: a label, a seed, a zone, and where to look if not
/// round its spawn.
type LookIn = (&'static str, u32, Zone, Option<(i32, i32)>);

/// The worlds the subjects are looked for in when `TERRAIN_FIND=1`: the
/// player's own last world, and the seed every landforms measurement in
/// `CHANGELOG.md` was taken on.
const WORLDS: [LookIn; 3] = [
    ("owner", 4_235_635_179, Zone::Tropics, None),
    ("s1337", 1337, Zone::Temperate, None),
    // The hill country `what_the_landforms_cost_a_chunk` times and the
    // golden prints hold: downs, where a gentle slope is most of the ground.
    ("downs", 1337, Zone::Temperate, Some((-29_950, -29_950))),
];

#[test]
#[ignore = "a tool: needs a GPU; photographs grassy slopes near, middle and far"]
fn what_the_slopes_look_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    let tag = std::env::var("TERRAIN_TAG").unwrap_or_else(|_| "now".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    println!(
        "fov {} aniso {} msaa {} distance {}",
        settings.fov_degrees, settings.anisotropy, settings.msaa, settings.render_distance_chunks
    );
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();

    let finding = std::env::var("TERRAIN_FIND").is_ok();
    let mut subjects: Vec<Subject> = Vec::new();
    if finding {
        for (label, seed, zone, centre) in WORLDS {
            let generator = WorldGen::with_zone(seed, Preset::Normal, zone);
            let (sx, sz) = centre.unwrap_or_else(|| generator.spawn_column());
            let world = stream_world(generator, ChunkPos::from_global(sx, sz).0, 5);
            // The slope: the most lips in a disc of ten, on ground that
            // rises four blocks or more and has no water in it; the bank:
            // the most with water in it; the hill: the most rise.
            let mut best_slope: Option<(u32, (i32, i32))> = None;
            let mut best_bank: Option<(u32, (i32, i32))> = None;
            let mut best_hill: Option<(u32, (i32, i32))> = None;
            for z in (sz - 64..=sz + 64).step_by(4) {
                for x in (sx - 64..=sx + 64).step_by(4) {
                    let Some((_, block)) = top(&world, x, z) else { continue };
                    if !primitive_shared::ground::is_turf(block_kind(block)) {
                        continue;
                    }
                    let (lips, rise, wet) = lips_round(&world, x, z, 10);
                    if !wet && rise >= 4 && best_slope.is_none_or(|(held, _)| lips > held) {
                        best_slope = Some((lips, (x, z)));
                    }
                    if wet && best_bank.is_none_or(|(held, _)| lips > held) {
                        best_bank = Some((lips, (x, z)));
                    }
                    let (_, wide_rise, wide_wet) = lips_round(&world, x, z, 20);
                    if !wide_wet && best_hill.is_none_or(|(held, _)| wide_rise as u32 > held) {
                        best_hill = Some((wide_rise as u32, (x, z)));
                    }
                }
            }
            for (kind, found) in [("slope", best_slope), ("bank", best_bank), ("hill", best_hill)] {
                if let Some((score, at)) = found {
                    let bearing = uphill(&world.generator, at);
                    println!("{label} {kind}: {score} round {at:?}, uphill {bearing:.0}");
                }
            }
        }
        return;
    }
    subjects.extend_from_slice(SUBJECTS);
    let only = std::env::var("TERRAIN_ONLY").unwrap_or_default();
    for subject in subjects.iter().filter(|s| only.is_empty() || only == s.name) {
        let generator = WorldGen::with_zone(subject.seed, Preset::Normal, subject.zone);
        let centre = ChunkPos::from_global(subject.at.0, subject.at.1).0;
        let world = stream_world(generator, centre, settings.render_distance_chunks);
        let meshed = mesh_world(&world, &layers, &settings, centre, true);
        // What the ground costs the frame: the lips are geometry, and the
        // solid pass of a phone is sensitive to it (CLAUDE.md, "Where the
        // frame goes"). Counted over the same disc in both builds.
        let triangles: usize = meshed.meshes.iter().map(|(_, mesh)| mesh.indices.len() / 3).sum();
        println!(
            "[triangles] {}: {} chunks, {triangles} triangles, {:.0} a chunk",
            subject.name,
            meshed.meshes.len(),
            triangles as f64 / meshed.meshes.len().max(1) as f64
        );
        let (tx, tz) = subject.at;
        let ground = |x: f32, z: f32| {
            top(&world, x.floor() as i32, z.floor() as i32).map_or(0.0, |(y, _)| y as f32 + 1.0)
        };
        let at = Vec3::new(tx as f32 + 0.5, ground(tx as f32 + 0.5, tz as f32 + 0.5), tz as f32 + 0.5);
        let (sin, cos) = subject.bearing.to_radians().sin_cos();
        for (hour_name, hour) in HOURS {
            let sky = Sky::new(hour, 900.0);
            for (seat, back, over) in SEATS {
                let (ex, ez) = (at.x - cos * back, at.z - sin * back);
                // Over the ground, and out of any crown standing on it: a
                // seat inside a tree photographed a wall of leaves.
                let clear = (-1..=1)
                    .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
                    .filter_map(|(dx, dz)| {
                        let (x, z) = (ex.floor() as i32 + dx, ez.floor() as i32 + dz);
                        (1..CHUNK_SIZE_Y as i32).rev().find(|&y| {
                            world.chunks.block_at(x, y, z).is_some_and(|b| b != primitive_shared::types::BLOCK_AIR)
                        })
                    })
                    .max()
                    .map_or(0.0, |y| y as f32 + 1.6);
                let eye = Vec3::new(ex, (ground(ex, ez).max(at.y - back * 0.2) + over).max(clear), ez);
                let picture = shot(device, queue, &textures, &settings, &sky, &meshed.meshes, eye, at);
                let name = format!("{out}/{}_{seat}_{hour_name}_{tag}.png", subject.name);
                picture.save(&name).expect("write png");
                if seat != "near" {
                    loupe(&picture, 4).save(format!("{out}/{}_{seat}_{hour_name}_{tag}_loupe.png", subject.name)).expect("write png");
                }
                println!("{name}");
            }
        }
    }
}
