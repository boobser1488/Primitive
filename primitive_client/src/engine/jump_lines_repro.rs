//! Horizontal lines at eye level while jumping, photographed through the
//! real passes.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/primitive_client/shots-review/jump-lines //!     cargo test -p primitive_client --lib jump_lines_at_eye_level -- --ignored --nocapture
//! ```
//!
//! The report: "while jumping, green or white lines appear along the plane
//! level with the player's eyes, on far and not so far faces". A jump is
//! the one time the eye sweeps through whole-block heights, so the eye is
//! set just under, on and just over a whole y and the frames compared.
//!
//! * Default: terraces (grass and snow plateaus two blocks over a meadow)
//!   to the edge of the disc, pairs of eyes 0.004 apart clear of the whole
//!   y and either side of it, plus a centroid-varyings control.
//! * `JUMP_GENERATED=x,z,ground` -- seed 32 from that column instead.
//! * `JUMP_SWEEP=yaw:x:y:w:h` -- eleven heights through a jump, one crop
//!   of each stacked into a sheet; `JUMP_SS=n` draws each n times larger
//!   and averages it down, the ground truth a thin face is judged by.
//! * `JUMP_RADIUS`, `JUMP_SETTINGS` -- the disc and the settings file (the
//!   player's last, `dist/Primitive-1.5.0`, by default).
//!
//! What it found, September 2026: nothing flips as the eye crosses a whole
//! y -- the pixels that change between y-0.002 and y+0.002 are as many as
//! between two eyes a tenth below it, and no row at the horizon stands
//! out; centroid varyings change nothing on open ground. The lines that do
//! appear a few hundredths to a few tenths *above* a plane are the tops of
//! that plane seen edge-on (a leaf layer inside a bush, a terrace), and the
//! same frame drawn three times larger and averaged has them too.

use super::view_distance_repro::{fog_for, cull_past_the_fog, mesh_world, players_settings, stream};
use super::*;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use glam::Vec3;
use primitive_shared::types::ChunkPos;

/// The player runs full screen.
const SIZE: (u32, u32) = (1920, 1080);

/// Pixels that stand out from both the row above and the row below in the
/// same direction: a line one pixel tall. Returns the count and the rows
/// that carry more than `min_run` of them.
fn thin_rows(picture: &image::RgbaImage, threshold: i32, min_run: u32) -> (u32, Vec<(u32, u32)>) {
    let luma = |x: u32, y: u32| {
        let p = picture.get_pixel(x, y);
        (p[0] as i32 * 3 + p[1] as i32 * 6 + p[2] as i32) / 10
    };
    let (mut total, mut rows) = (0, Vec::new());
    for y in 1..picture.height() - 1 {
        let mut count = 0;
        for x in 0..picture.width() {
            let (up, here, down) = (luma(x, y - 1), luma(x, y), luma(x, y + 1));
            if (here - up > threshold && here - down > threshold) || (up - here > threshold && down - here > threshold) {
                count += 1;
            }
        }
        total += count;
        if count >= min_run {
            rows.push((y, count));
        }
    }
    (total, rows)
}

/// The terrain shader with every plain interpolant of the three fragment
/// structs evaluated at the centroid, the way `uv` already is.
fn every_interpolant_centroid(source: &str) -> String {
    let mut out = source.to_string();
    for field in ["@location(2) view_distance", "@location(3) light_terms", "@location(7) shade_cell"] {
        assert!(out.contains(field), "shader.wgsl no longer declares `{field}`");
        let at = field.replace(") ", ") @interpolate(perspective, centroid) ");
        out = out.replace(field, &at);
    }
    out
}

/// Pixels whose largest channel differs by more than `threshold`, and a
/// mask painting them magenta over a dimmed `a`.
fn difference(a: &image::RgbaImage, b: &image::RgbaImage, threshold: i32) -> (u32, image::RgbaImage) {
    let mut mask = a.clone();
    let mut changed = 0;
    for (x, y, p) in mask.enumerate_pixels_mut() {
        let q = b.get_pixel(x, y);
        let d = (0..3).map(|c| (p[c] as i32 - q[c] as i32).abs()).max().unwrap_or(0);
        if d > threshold {
            *p = image::Rgba([255, 0, 255, 255]);
            changed += 1;
        } else {
            *p = image::Rgba([p[0] / 3, p[1] / 3, p[2] / 3, 255]);
        }
    }
    (changed, mask)
}

/// Flat meadow at `MEADOW` with square plateaus two blocks higher -- grass
/// and snow -- scattered to the edge of the disc: the whole y a jump carries
/// the eye through, at every distance, with nothing in the way.
const MEADOW: usize = 68;

fn terraced_world(radius: i32) -> super::view_distance_repro::World {
    use primitive_shared::types::{BLOCK_AIR, BLOCK_DIRT, BLOCK_GRASS, BLOCK_SNOW};
    let generator = primitive_shared::worldgen::WorldGen::new(32);
    let probe = ChunkManager::new(radius);
    let mut positions = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if probe.inside(dx, dz) {
                positions.push(ChunkPos::new(dx, dz));
            }
        }
    }
    let mut chunks = ChunkManager::new(radius);
    let mut isolated = Vec::new();
    for &pos in &positions {
        let mut chunk = generator.generate_chunk(pos);
        for z in 0..16 {
            for x in 0..16 {
                let (gx, gz) = (pos.x * 16 + x as i32, pos.z * 16 + z as i32);
                let (cx, cz) = (gx.div_euclid(12), gz.div_euclid(12));
                let near = gx * gx + gz * gz < 64;
                let plateau = !near && (cx + 2 * cz).rem_euclid(3) == 0;
                let top = if plateau { MEADOW + 2 } else { MEADOW };
                let surface = if plateau && cx.rem_euclid(2) == 0 { BLOCK_SNOW } else { BLOCK_GRASS };
                for y in 1..primitive_shared::types::CHUNK_SIZE_Y {
                    let id = if y < top {
                        BLOCK_DIRT
                    } else if y == top {
                        surface
                    } else {
                        BLOCK_AIR
                    };
                    {
                        chunk.set(x, y, z, id);
                    }
                }
            }
        }
        isolated.push(primitive_shared::lighting::compute_isolated(&chunk.blocks));
        chunks.insert(chunk);
    }
    let mut light = primitive_shared::lighting::LightMap::new();
    for (pos, data) in positions.iter().zip(isolated) {
        light.insert_precomputed(&chunks, *pos, data);
    }
    super::view_distance_repro::World { generator, chunks, light, positions }
}

#[test]
#[ignore = "a tool: needs a GPU; photographs the eye sweeping through a whole y"]
fn jump_lines_at_eye_level() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    // The file the player last ran the game with (dist/Primitive-1.5.0,
    // written the night of the report): anisotropy 1, msaa 4, detail from
    // ten chunks. `JUMP_SETTINGS` points somewhere else.
    let path = std::env::var("JUMP_SETTINGS").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../dist/Primitive-1.5.0/client_settings.toml").to_string()
    });
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut read = toml::from_str::<crate::settings::ClientSettings>(&text).expect("settings parse");
            read.sanitize();
            read
        }
        Err(_) => players_settings(),
    };
    if let Some(radius) = std::env::var("JUMP_RADIUS").ok().and_then(|r| r.parse().ok()) {
        settings.render_distance_chunks = radius;
    }
    println!(
        "settings from {path}: distance {}, fov {}, aniso {}, msaa {}, lod {}",
        settings.render_distance_chunks, settings.fov_degrees, settings.anisotropy, settings.msaa, settings.lod_distance_chunks
    );
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let sky = Sky::new(0.40, 900.0);

    let started = std::time::Instant::now();
    // `JUMP_GENERATED=x,z,ground` photographs seed 32 from that column
    // instead of the terraces.
    let generated: Option<Vec<i32>> = std::env::var("JUMP_GENERATED")
        .ok()
        .map(|v| v.split(',').map(|p| p.parse().expect("x,z,ground")).collect());
    let (world, sx, sz, ground) = match &generated {
        Some(seat) => (
            stream(ChunkPos::from_global(seat[0], seat[1]).0, settings.render_distance_chunks),
            seat[0],
            seat[1],
            seat[2] as f32,
        ),
        None => (terraced_world(settings.render_distance_chunks), 0, 0, MEADOW as f32),
    };
    let spawn_chunk = ChunkPos::from_global(sx, sz).0;
    let standing = Vec3::new(sx as f32 + 0.5, ground + 2.62, sz as f32 + 0.5);
    let layers = textures.face_layers();
    let mut game = mesh_world(&world, &layers, &settings, spawn_chunk, true);
    let fog = fog_for(&settings, &sky, false);
    cull_past_the_fog(&mut game.meshes, standing, &fog);
    println!("streamed and meshed in {:.1}s; ground {ground}", started.elapsed().as_secs_f32());

    let only: Option<String> = std::env::var("JUMP_ONLY").ok();
    let game_source = include_str!("shader.wgsl");
    let centroid_source = every_interpolant_centroid(game_source);
    if let Ok(sweep) = std::env::var("JUMP_SWEEP") {
        // `JUMP_SWEEP=yaw:x:y:w:h` -- one bearing, the eye stepped through a
        // jump, and the same crop of every frame stacked top to bottom,
        // doubled, so a line that comes and goes is seen coming and going.
        let parts: Vec<f32> = sweep.split(':').map(|p| p.parse().expect("number")).collect();
        let (yaw, x, y, w, h) = (parts[0], parts[1] as u32, parts[2] as u32, parts[3] as u32, parts[4] as u32);
        let offsets = [-0.3f32, -0.1, -0.03, -0.01, -0.001, 0.0, 0.001, 0.01, 0.03, 0.1, 0.3];
        let mut sheet = image::RgbaImage::new(w * 2, (h * 2 + 4) * offsets.len() as u32);
        for (i, offset) in offsets.iter().enumerate() {
            let eye = Vec3::new(standing.x, ground + 3.0 + offset, standing.z);
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = yaw.to_radians();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            // `JUMP_SS=n` draws n times larger and averages back down: the
            // frame a pixel would be if it were n x n pixels, which is the
            // ground truth a thin face is judged against.
            let ss: u32 = std::env::var("JUMP_SS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            let big = super::offscreen_repro::draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &game.meshes, (SIZE.0 * ss, SIZE.1 * ss), game_source,
                Some(ChunkManager::reach_blocks(settings.render_distance_chunks)), None, false, settings.msaa,
            );
            let mut picture = image::RgbaImage::from_fn(SIZE.0, SIZE.1, |px, py| {
                let mut sum = [0u32; 4];
                for dy in 0..ss {
                    for dx in 0..ss {
                        let q = big.get_pixel(px * ss + dx, py * ss + dy);
                        for (total, channel) in sum.iter_mut().zip(q.0) {
                            *total += channel as u32;
                        }
                    }
                }
                image::Rgba(sum.map(|v| (v / (ss * ss)) as u8))
            });
            for p in picture.pixels_mut() {
                p[3] = 255;
            }
            picture.save(format!("{out}/sweep_{yaw}_{offset:+.3}_ss{ss}.png")).expect("write png");
            let crop = image::imageops::crop_imm(&picture, x, y, w, h).to_image();
            let big = image::imageops::resize(&crop, w * 2, h * 2, image::imageops::FilterType::Nearest);
            image::imageops::replace(&mut sheet, &big, 0, (i as u32 * (h * 2 + 4)) as i64);
        }
        sheet.save(format!("{out}/sweep_{yaw}_sheet_ss{}.png", std::env::var("JUMP_SS").unwrap_or_else(|_| "1".into()))).expect("write png");
        return;
    }
    // Pairs of eyes 0.004 apart: one pair well clear of the whole y, one
    // either side of it. The geometry moves the same sub-pixel amount in
    // both, so whatever differs in the second pair and not in the first is
    // something that flips as the eye crosses the plane.
    let pairs: [(&str, f32, f32); 2] = [("clear", -0.104, -0.100), ("cross", -0.002, 0.002)];
    let whole = ground + 3.0;
    for (name, yaw) in [("n", -90.0f32), ("e", 0.0), ("s", 90.0), ("w", 180.0)] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        for (pair, below, above) in pairs {
            let take = |offset: f32, source: &str, samples: u32| {
                let eye = Vec3::new(standing.x, whole + offset, standing.z);
                let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
                camera.yaw = yaw.to_radians();
                camera.pitch = 0.0;
                camera.fov_y_radians = settings.fov_degrees.to_radians();
                let mut picture = super::offscreen_repro::draw_scene(
                    device,
                    queue,
                    &textures,
                    &settings,
                    &camera,
                    &sky,
                    &game.meshes,
                    SIZE,
                    source,
                    Some(ChunkManager::reach_blocks(settings.render_distance_chunks)),
                    None,
                    false,
                    samples,
                );
                for p in picture.pixels_mut() {
                    p[3] = 255;
                }
                picture
            };
            let (a, b) = (take(below, game_source, settings.msaa), take(above, game_source, settings.msaa));
            let (changed, mask) = difference(&a, &b, 40);
            let (thin_a, _) = thin_rows(&a, 25, 60);
            let (thin_b, _) = thin_rows(&b, 25, 60);
            println!("  {name} {pair}: {changed} pixels flip; thin pixels {thin_a} -> {thin_b}");
            let stem = format!("{out}/jump_{name}_{pair}");
            a.save(format!("{stem}_below.png")).expect("write png");
            b.save(format!("{stem}_above.png")).expect("write png");
            mask.save(format!("{stem}_mask.png")).expect("write png");
            // The control for extrapolated interpolants: the same eye with
            // every plain varying taken at the centroid.
            let (extrapolated, _) = difference(&b, &take(above, &centroid_source, settings.msaa), 40);
            println!("      {extrapolated} pixels change when every varying is centroid");
        }
    }
    println!("pictures in {out}");
}
