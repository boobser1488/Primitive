//! **The small cross over a tuft of grass, a flower and a fire**, photographed
//! through the real terrain shader at the player's settings and taken apart
//! one term at a time.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/cross-top \
//!     cargo test -p primitive_client --lib \
//!     what_hangs_over_a_cross_plant -- --ignored --nocapture
//! ```
//!
//! Written for "над креста образными объектами по типу травы или огня есть
//! какой то крестик который мы чинили но не починили". The first repair
//! (`centroid` on `VertexOutput::uv`) was right about one way a plant's top
//! edge can fetch the bottom of its own picture and silent about the other,
//! so this tool asks the question of every term that could put something
//! there:
//!
//! * `game` -- the shader as it is, at the player's multisampling.
//! * `one_sample` -- one sample a pixel: is it an edge-coverage effect?
//! * `unfiltered` -- the cut-out fetches the nearest texel of the full-size
//!   picture: no filter can reach past the quad's edge.
//! * `mip0` -- bilinear, wrapping, but no mip chain and no anisotropy.
//! * `wrapped` -- the cut-out samples the way it did before the fix below:
//!   `sample_block` straight, the sampler's `Repeat` free to fetch across the
//!   quad's top edge into the picture's bottom row.
//! * `probe_*` -- the same frames with every cut-out fragment that survives
//!   in the top quarter of its picture while the texel under it is empty
//!   painted magenta, and counted. For every picture in the scene that
//!   quarter is empty at every level of the mip chain, so a fragment kept
//!   there took its opacity from somewhere that is not above it.
//!
//! Shadows are not a term here, and that is the code's answer rather than an
//! omission: the shadow casters draw `solid..leaf_end` (see
//! `ShadowCasters`), and the crosses and the flames are the sprite range
//! after it, so nothing a plant is can reach the shadow map.
//!
//! Each camera also gets a loupe sheet: the top of every near plant, eight
//! times over, one column per variant.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::{Vec3, Vec4};
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    BlockId, Chunk, BLOCK_AIR, BLOCK_CAMPFIRE_LIT, BLOCK_DIRT, BLOCK_FLOWER, BLOCK_GRASS, BLOCK_REEDS,
    BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_WHEAT, BLOCK_WHEAT_RIPE, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

/// The shader, with its line endings made one kind so an anchor written
/// here matches whichever way the checkout stores them.
fn shader() -> String {
    include_str!("shader.wgsl").replace("\r\n", "\n")
}

/// The settings the report was made with, read the way the game reads them.
fn players_settings() -> ClientSettings {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../client_settings.toml");
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<ClientSettings>(&text).expect("the player's settings file parses"),
        Err(_) => ClientSettings {
            render_distance_chunks: 24,
            fov_degrees: 90.0,
            anisotropy: 16,
            ambient_occlusion: 0.45,
            ..ClientSettings::default()
        },
    };
    settings.sanitize();
    settings
}

/// The top of the ground is y = 3; the near plants stand in the middle
/// chunk, north of an eye at z = 15.5.
const GROUND: i32 = 3;
const NEAR: [(usize, usize, BlockId, &str); 9] = [
    (6, 12, BLOCK_TALL_GRASS, "grass"),
    (8, 12, BLOCK_FLOWER, "flower"),
    (10, 12, BLOCK_WHEAT_RIPE, "wheat_ripe"),
    (6, 10, BLOCK_REEDS, "reeds"),
    (8, 10, BLOCK_CAMPFIRE_LIT, "fire"),
    (10, 10, BLOCK_WHEAT, "wheat"),
    (6, 4, BLOCK_TALL_GRASS, "grass_11m"),
    (8, 4, BLOCK_CAMPFIRE_LIT, "fire_11m"),
    (10, 4, BLOCK_FLOWER, "flower_11m"),
];

/// Nine chunks of turf with a tuft every few columns, and the near plants.
fn scene(textures: &TextureManager) -> Vec<(ChunkPos, MeshBuffers)> {
    let mut chunks = ChunkManager::new(8);
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_GRASS;
                    let clearing = (cx, cz) == (0, 0) && (4..=12).contains(&x) && z >= 2;
                    if !clearing && (x * 7 + z * 3) % 5 == 0 {
                        blocks[Chunk::index(x, GROUND as usize, z)] = BLOCK_TALL_GRASS;
                    }
                }
            }
            if (cx, cz) == (0, 0) {
                for (x, z, block, _) in NEAR {
                    blocks[Chunk::index(x, GROUND as usize, z)] = block;
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

/// The terrain shader with `fs_cutout` replaced by one whose fetch is
/// `sample` and which, when `probe` is set, paints a kept fragment in the
/// empty top quarter of its picture magenta.
fn cutout_variant(sample: &str, probe: bool, extra: &str) -> String {
    let source = shader();
    let header = "fn fs_cutout(in: VertexOutput) -> @location(0) vec4<f32> {";
    assert_eq!(source.matches(header).count(), 1, "shader.wgsl no longer declares `fs_cutout` once");
    let paint = if probe {
        "    let resolution = max(globals.texture_params.x, 1.0);\n\
         \x20   let under = textureSampleLevel(block_textures, block_sampler, \
         (floor(in.uv * resolution) + 0.5) / resolution, i32(animated(in.tex_layer)), 0.0).a;\n\
         \x20   if (under < ALPHA_CUTOFF && in.uv.y < 0.25) {\n\
         \x20       return vec4<f32>(1.0, 0.0, 1.0, 1.0);\n\
         \x20   }\n"
    } else {
        ""
    };
    let replacement = "fn fs_cutout_as_written(in: VertexOutput) -> @location(0) vec4<f32> {".to_string();
    let tail = format!(
            "\n{extra}\n@fragment\nfn fs_cutout(in: VertexOutput) -> @location(0) vec4<f32> {{\n\
             \x20   let sampled = {sample};\n\
             \x20   if (sampled.a < ALPHA_CUTOFF) {{\n        discard;\n    }}\n\
             {paint}\
             \x20   return shade(in, sampled);\n}}\n"
    );
    source.replacen(header, &replacement, 1) + tail.as_str()
}

/// The candidate cure, as a control: the fetch held inside the picture by
/// as much as the filter reaches. Magnified, half a texel -- the centre of
/// the outermost row, which `crisp_uv` then never ramps past. Minified, half
/// the footprint's extent along each axis (where the anisotropic taps go)
/// plus a texel of the level the hardware reads the coarser of.
const CLAMPED_SAMPLE: &str = "
fn probe_clamped_sample(uv: vec2<f32>, named: u32) -> vec4<f32> {
    let plain = sample_block(uv, named);
    let resolution = globals.texture_params.x;
    if (resolution <= 0.0) {
        return plain;
    }
    let ddx = dpdx(uv);
    let ddy = dpdy(uv);
    let texel = 1.0 / resolution;
    let spread = abs(ddx) + abs(ddy);
    let ramp = max(spread * resolution, vec2<f32>(1e-5));
    let layer = i32(animated(named));
    if (max(ramp.x, ramp.y) < 1.0) {
        let inside = clamp(uv, vec2<f32>(0.5 * texel), vec2<f32>(1.0 - 0.5 * texel));
        if (all(inside == uv)) {
            return plain;
        }
        return textureSampleGrad(block_textures, block_sampler, crisp_uv(inside, resolution, ramp), layer, ddx, ddy);
    }
    let lx = length(ddx);
    let ly = length(ddy);
    let footprint = max(min(lx, ly), max(lx, ly) / 16.0);
    let reach = min(0.5 * spread + vec2<f32>(max(0.5 * texel, footprint)), vec2<f32>(0.5));
    let inside = clamp(uv, reach, vec2<f32>(1.0) - reach);
    if (all(inside == uv)) {
        return plain;
    }
    return textureSampleGrad(block_textures, block_sampler, inside, layer, ddx, ddy);
}
";

fn project(camera: &Camera, point: Vec3) -> Option<(f32, f32)> {
    let origin = camera.position.floor();
    let clip = camera.view_proj_about(origin.as_vec3()) * Vec4::from((point - origin.as_vec3(), 1.0));
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(((ndc.x * 0.5 + 0.5) * SIZE.0 as f32, (0.5 - ndc.y * 0.5) * SIZE.1 as f32))
}

fn is_magenta(p: &image::Rgba<u8>) -> bool {
    p[0] as i32 - p[1] as i32 > 110 && p[2] as i32 - p[1] as i32 > 110
}

/// Pixels magenta in `probed` and not in `plain`.
fn painted(probed: &image::RgbaImage, plain: &image::RgbaImage) -> (u32, image::RgbaImage) {
    let mut marked = plain.clone();
    let mut count = 0;
    for (x, y, p) in probed.enumerate_pixels() {
        if is_magenta(p) && !is_magenta(plain.get_pixel(x, y)) {
            count += 1;
            marked.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        }
    }
    (count, marked)
}

const LOUPE: (u32, u32) = (36, 26);
const ZOOM: u32 = 8;

/// One row per near plant in view, one column per picture: the top of the
/// plant, eight times over.
fn loupe_sheet(camera: &Camera, columns: &[&image::RgbaImage]) -> image::RgbaImage {
    let tile = (LOUPE.0 * ZOOM, LOUPE.1 * ZOOM);
    let rows: Vec<(f32, f32)> = NEAR
        .iter()
        .filter_map(|&(x, z, block, _)| {
            let top = if block == BLOCK_CAMPFIRE_LIT { 0.98 } else { 0.94 };
            project(camera, Vec3::new(x as f32 + 0.5, GROUND as f32 + top, z as f32 + 0.5))
        })
        .filter(|&(px, py)| px >= 0.0 && py >= 0.0 && px < SIZE.0 as f32 && py < SIZE.1 as f32)
        .collect();
    let gap = 4;
    let mut sheet = image::RgbaImage::from_pixel(
        columns.len() as u32 * (tile.0 + gap),
        rows.len().max(1) as u32 * (tile.1 + gap),
        image::Rgba([255, 255, 255, 255]),
    );
    for (row, (px, py)) in rows.iter().enumerate() {
        let left = (*px as i64 - LOUPE.0 as i64 / 2).clamp(0, (SIZE.0 - LOUPE.0) as i64) as u32;
        let top = (*py as i64 - LOUPE.1 as i64 / 2).clamp(0, (SIZE.1 - LOUPE.1) as i64) as u32;
        for (column, picture) in columns.iter().enumerate() {
            let crop = image::imageops::crop_imm(*picture, left, top, LOUPE.0, LOUPE.1).to_image();
            let big = image::imageops::resize(&crop, tile.0, tile.1, image::imageops::FilterType::Nearest);
            image::imageops::replace(
                &mut sheet,
                &big,
                (column as u32 * (tile.0 + gap)) as i64,
                (row as u32 * (tile.1 + gap)) as i64,
            );
        }
    }
    sheet
}

#[test]
#[ignore = "a tool: needs a GPU; takes apart the cross over a cross plant"]
fn what_hangs_over_a_cross_plant() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    println!(
        "settings: fov {}, anisotropy {}, msaa {}, ao {}, render distance {}, see-through leaves to {} chunks",
        settings.fov_degrees,
        settings.anisotropy,
        settings.msaa,
        settings.ambient_occlusion,
        settings.render_distance_chunks,
        settings.transparent_leaves_chunks
    );
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let meshes = scene(&textures);

    let game = shader();
    let wrapped = cutout_variant("sample_block(in.uv, in.tex_layer)", false, "");
    let probe_wrapped = cutout_variant("sample_block(in.uv, in.tex_layer)", true, "");
    let unfiltered = cutout_variant(
        "textureSampleLevel(block_textures, block_sampler, (floor(in.uv * max(globals.texture_params.x, 1.0)) + 0.5) \
         / max(globals.texture_params.x, 1.0), i32(animated(in.tex_layer)), 0.0)",
        false,
        "",
    );
    let mip0 = cutout_variant(
        "textureSampleLevel(block_textures, block_sampler, in.uv, i32(animated(in.tex_layer)), 0.0)",
        false,
        "",
    );
    let clamped = cutout_variant("probe_clamped_sample(in.uv, in.tex_layer)", false, CLAMPED_SAMPLE);
    let probe_clamped = cutout_variant("probe_clamped_sample(in.uv, in.tex_layer)", true, CLAMPED_SAMPLE);

    let eye = Vec3::new(8.5, GROUND as f32 + 1.62, 15.5);
    let seats: [(&str, Vec3, f32, f32); 5] = [
        ("stand_15", eye, -90.0, -15.0),
        ("stand_35", eye, -90.0, -35.0),
        ("far_field", eye, -90.0, -6.0),
        ("over_grass", Vec3::new(7.4, GROUND as f32 + 1.62, 13.1), -90.0, -89.0),
        ("over_fire", Vec3::new(8.5, GROUND as f32 + 1.62, 11.2), -90.0, -89.0),
    ];
    println!("{:>10} {:>5} | kept in the empty top quarter: wrapped  clamped | game vs clamped differ", "seat", "hour");
    for (hour_name, hour) in [("noon", 0.5f32), ("night", 0.0)] {
        let sky = Sky::new(hour, 900.0);
        for (seat, position, yaw, pitch) in seats {
            let mut camera = Camera::new(position.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = yaw.to_radians();
            camera.pitch = pitch.to_radians();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let draw = |source: &str, samples: u32| {
                super::offscreen_repro::draw_scene(
                    device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE, source, None, None, false,
                    samples,
                )
            };
            let name = |variant: &str| format!("{out}/{seat}_{hour_name}_{variant}.png");
            let game_picture = draw(&game, settings.msaa);
            let wrapped_picture = draw(&wrapped, settings.msaa);
            let clamped_picture = draw(&clamped, settings.msaa);
            let one_sample = draw(&game, 1);
            let unfiltered_picture = draw(&unfiltered, settings.msaa);
            let mip0_picture = draw(&mip0, settings.msaa);
            let (kept_wrapped, marked_wrapped) = painted(&draw(&probe_wrapped, settings.msaa), &wrapped_picture);
            let (kept_clamped, marked_clamped) = painted(&draw(&probe_clamped, settings.msaa), &clamped_picture);
            // ...and the shader as it ships, `sample_cutout`, probed the same way.
            let probe_game = cutout_variant("sample_cutout(in.uv, in.tex_layer)", true, "");
            let (kept_game, marked_game) = painted(&draw(&probe_game, settings.msaa), &game_picture);
            println!("{seat:>10} {hour_name:>5} | kept in the empty top quarter by the game: {kept_game}");
            marked_game.save(name("probe_game")).expect("write png");
            let differ = game_picture
                .pixels()
                .zip(clamped_picture.pixels())
                .filter(|(a, b)| (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum::<i32>() > 24)
                .count();
            println!("{seat:>10} {hour_name:>5} | {kept_wrapped:>35} {kept_clamped:>8} | {differ:>8}");
            let sheet = loupe_sheet(
                &camera,
                &[&wrapped_picture, &marked_wrapped, &one_sample, &unfiltered_picture, &mip0_picture, &clamped_picture, &game_picture],
            );
            sheet.save(name("loupes")).expect("write png");
            game_picture.save(name("game")).expect("write png");
            wrapped_picture.save(name("wrapped")).expect("write png");
            clamped_picture.save(name("clamped")).expect("write png");
            marked_wrapped.save(name("probe_wrapped")).expect("write png");
            marked_clamped.save(name("probe_clamped")).expect("write png");
        }
    }
    println!("loupe columns: wrapped | probe (wrapped) | one sample | unfiltered | mip0 | clamped | game");
    println!("pictures in {out}");
}
