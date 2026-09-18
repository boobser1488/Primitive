//! **Stones, sticks and flint lying on the ground, before and after they got
//! a thickness**, photographed through the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir cargo test --release -p primitive_client --lib \
//!     what_lies_on_the_ground_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for "сделай палки камни и прочее 3д моделями а не просто
//! наложениями". Each picture is taken twice from the same eye, in the same
//! process: `*_before` meshed with a picture table that has no reliefs
//! (`FaceLayers::without_reliefs`, which is exactly the flat quad the mesher
//! drew before), `*_after` with the one the game loads. So the only
//! difference between a pair is the change.
//!
//! * `row_*` -- every kind of thing that lies on the ground, in a row on a
//!   lawn, from where a player stands (eye 1.62 over the grass, three blocks
//!   back, the player's own field of view and anisotropy).
//! * `close_*` -- the same row from a step away, looking down.
//! * `turn_{n,e,s,w}` -- the pebble and the flint walked round, after only:
//!   the light has to move across the sides as the eye does, or the face
//!   index did not turn with the stone.
//! * `forest_*` -- the floor of a generated oak wood, sticks and stones where
//!   worldgen put them.
//! * `*_loupe` -- the middle of a picture eight times, nearest neighbour.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::engine::texture::FaceLayers;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    is_canopy, is_cross, is_flat, is_leafy, is_liquid, BlockId, Chunk, BLOCK_AIR, BLOCK_DIRT, BLOCK_FLINT,
    BLOCK_FLINT_FLAKE, BLOCK_GRASS, BLOCK_NATIVE_COPPER, BLOCK_PEBBLE, BLOCK_RUSTY_STONE, BLOCK_SHELL, BLOCK_STICK,
    BLOCK_STONE, BLOCK_STREAM_TIN, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::{Biome, WorldGen};

const SIZE: (u32, u32) = (1280, 720);

/// The player's settings file if there is one beside the repository, the
/// desktop defaults at the player's field of view otherwise.
fn players_settings() -> ClientSettings {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../client_settings.toml");
    let mut settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<ClientSettings>(&text).ok())
        .unwrap_or(ClientSettings { fov_degrees: 90.0, anisotropy: 16, ..ClientSettings::default() });
    settings.sanitize();
    settings
}

const KINDS: [BlockId; 8] = [
    BLOCK_STICK,
    BLOCK_PEBBLE,
    BLOCK_FLINT,
    BLOCK_FLINT_FLAKE,
    BLOCK_NATIVE_COPPER,
    BLOCK_RUSTY_STONE,
    BLOCK_STREAM_TIN,
    BLOCK_SHELL,
];

/// The lawn's surface is at y = 3; things lie in that cell.
const LYING: usize = 3;

/// A three-by-three of lawn with every kind in a row across the middle
/// chunk at z = 8, and a handful of pebbles and sticks behind it.
fn lawn() -> Vec<Chunk> {
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
                for (i, &kind) in KINDS.iter().enumerate() {
                    blocks[Chunk::index(4 + i, LYING, 8)] = kind;
                }
                for (x, z, kind) in [(3, 5, BLOCK_PEBBLE), (6, 4, BLOCK_STICK), (9, 5, BLOCK_PEBBLE), (11, 3, BLOCK_STICK), (13, 5, BLOCK_FLINT)] {
                    blocks[Chunk::index(x, LYING, z)] = kind;
                }
            }
            chunks.push(Chunk { pos: ChunkPos::new(cx, cz), blocks });
        }
    }
    chunks
}

fn meshes(chunks: &[Chunk], layers: &FaceLayers, generator: &WorldGen) -> Vec<(ChunkPos, MeshBuffers)> {
    let mut manager = ChunkManager::new(128);
    for chunk in chunks {
        manager.insert(Chunk { pos: chunk.pos, blocks: chunk.blocks.clone() });
    }
    let mut light = LightMap::new();
    for chunk in chunks {
        light.load_chunk(&manager, chunk.pos);
    }
    chunks
        .iter()
        .map(|chunk| {
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(chunk.pos, &manager, &light);
            let mut out = MeshBuffers::default();
            build_mesh(chunk.pos, &cache, layers, generator, &mut out);
            (chunk.pos, out)
        })
        .collect()
}

/// The middle of `image`, `factor` times, nearest neighbour.
fn loupe(image: &image::RgbaImage, centre: (u32, u32), size: (u32, u32), factor: u32) -> image::RgbaImage {
    let x0 = centre.0.saturating_sub(size.0 / 2).min(image.width() - size.0);
    let y0 = centre.1.saturating_sub(size.1 / 2).min(image.height() - size.1);
    let crop = image::imageops::crop_imm(image, x0, y0, size.0, size.1).to_image();
    image::imageops::resize(&crop, size.0 * factor, size.1 * factor, image::imageops::FilterType::Nearest)
}

fn side_by_side(left: &image::RgbaImage, right: &image::RgbaImage) -> image::RgbaImage {
    let mut sheet = image::RgbaImage::new(left.width() + right.width() + 8, left.height().max(right.height()));
    image::imageops::overlay(&mut sheet, left, 0, 0);
    image::imageops::overlay(&mut sheet, right, i64::from(left.width()) + 8, 0);
    sheet
}

/// A column of a generated wood to stand in: under the crowns, clear to eye
/// height, with something lying on the ground within six blocks ahead.
fn under_the_crowns(chunks: &[Chunk]) -> Option<Vec3> {
    let middle = &chunks[chunks.len() / 2];
    let blocks = &middle.blocks;
    let ground = |x: usize, z: usize| {
        (1..CHUNK_SIZE_Y).rev().find(|&y| {
            let b = blocks[Chunk::index(x, y, z)];
            b != BLOCK_AIR && !is_cross(b) && !is_flat(b) && !is_leafy(b) && !is_liquid(b)
        })
    };
    let mut best: Option<(usize, Vec3)> = None;
    for z in 8..CHUNK_SIZE_Z {
        for x in 1..CHUNK_SIZE_X - 1 {
            let Some(g) = ground(x, z) else { continue };
            if g + 30 >= CHUNK_SIZE_Y {
                continue;
            }
            let clear = (1..=2).all(|dy| {
                let b = blocks[Chunk::index(x, g + dy, z)];
                b == BLOCK_AIR || is_cross(b) || is_flat(b)
            });
            let shaded = (g + 3..g + 30).any(|y| is_canopy(blocks[Chunk::index(x, y, z)]));
            if !clear || !shaded {
                continue;
            }
            // Things lying ahead: north, a few blocks either side.
            let ahead = (2..=6.min(z))
                .flat_map(|k| (x.saturating_sub(3)..(x + 4).min(CHUNK_SIZE_X)).map(move |xx| (xx, z - k)))
                .filter(|&(xx, zz)| (1..CHUNK_SIZE_Y).any(|y| crate::engine::relief::has_relief(blocks[Chunk::index(xx, y, zz)])))
                .count();
            let eye = Vec3::new((middle.pos.x * 16) as f32 + x as f32 + 0.5, g as f32 + 2.62, (middle.pos.z * 16) as f32 + z as f32 + 0.5);
            if best.is_none_or(|(held, _)| ahead > held) {
                best = Some((ahead, eye));
            }
        }
    }
    best.filter(|(ahead, _)| *ahead > 0).map(|(_, eye)| eye)
}

#[test]
#[ignore = "a tool: needs a GPU; photographs stones and sticks on the ground, flat and with a thickness"]
fn what_lies_on_the_ground_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    println!("fov {}, anisotropy {}, msaa {}", settings.fov_degrees, settings.anisotropy, settings.msaa);
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let after_layers = textures.face_layers();
    let before_layers = textures.face_layers().without_reliefs();
    let shader = include_str!("shader.wgsl").replace("\r\n", "\n");
    let generator = WorldGen::new(1337);
    // Mid-morning: the sun off to one side, so a side facing it and a side
    // facing away are different brightnesses -- at noon every side is the
    // same half-lambert and a lump can only be told by its outline.
    let hour = 0.36;
    let draw_at = |meshes: &[(ChunkPos, MeshBuffers)], eye: Vec3, yaw: f32, pitch: f32, samples: u32| {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let sky = Sky::new(hour, 900.0);
        super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, &shader, None, None, false, samples,
        )
    };
    let draw = |meshes: &[(ChunkPos, MeshBuffers)], eye: Vec3, yaw: f32, pitch: f32| draw_at(meshes, eye, yaw, pitch, settings.msaa);
    let save = |image: &image::RgbaImage, name: &str| {
        let path = format!("{out}/{name}.png");
        image.save(&path).expect("write png");
        println!("{path}");
    };

    let lawn = lawn();
    let before = meshes(&lawn, &before_layers, &generator);
    let after = meshes(&lawn, &after_layers, &generator);
    let triangles = |m: &[(ChunkPos, MeshBuffers)]| m.iter().map(|(_, b)| b.indices.len() / 3).sum::<usize>();
    println!("lawn: {} triangles flat, {} with reliefs", triangles(&before), triangles(&after));

    // The row is at z = 8.5, x = 4..12; the player three blocks south of it.
    let shots = [
        ("row", Vec3::new(8.0, LYING as f32 + 1.62, 11.6), -90.0, -28.0),
        ("close", Vec3::new(7.0, LYING as f32 + 1.2, 9.9), -90.0, -48.0),
    ];
    for (name, eye, yaw, pitch) in shots {
        let was = draw(&before, eye, yaw, pitch);
        let now = draw(&after, eye, yaw, pitch);
        save(&was, &format!("{name}_before"));
        save(&now, &format!("{name}_after"));
        save(&side_by_side(&was, &now), &format!("{name}_sheet"));
        let centre = (SIZE.0 / 2, SIZE.1 / 2);
        save(
            &side_by_side(&loupe(&was, centre, (240, 135), 4), &loupe(&now, centre, (240, 135), 4)),
            &format!("{name}_loupe"),
        );
    }

    // Walked round the pebble (x 5) and the flint (x 6).
    let target = Vec3::new(6.0, LYING as f32, 8.5);
    for (name, yaw) in [("n", 90.0f32), ("e", 180.0), ("s", -90.0), ("w", 0.0)] {
        let forward = Vec3::new(yaw.to_radians().cos(), 0.0, yaw.to_radians().sin());
        let eye = target - forward * 1.6 + Vec3::new(0.0, 1.3, 0.0);
        let now = draw(&after, eye, yaw, -38.0);
        save(&now, &format!("turn_{name}"));
        // ...and at one sample a pixel, which tells an edge-coverage effect
        // (a pale line along the top edge of a side, found through the loupe)
        // from one in the picture.
        if name == "s" {
            save(&draw_at(&after, eye, yaw, -38.0, 1), "turn_s_one_sample");
        }
    }

    // A generated oak wood.
    let nearest_wood = (-3000..3000)
        .step_by(128)
        .flat_map(|gz| (-3000..3000).step_by(128).map(move |gx| (gx, gz)))
        .filter(|&(gx, gz)| {
            [(-24, -24), (24, -24), (-24, 24), (24, 24), (0, 0)]
                .iter()
                .all(|&(dx, dz)| generator.biome_at(gx + dx, gz + dz) == Biome::Forest)
        })
        .min_by_key(|&(gx, gz)| i64::from(gx).pow(2) + i64::from(gz).pow(2));
    if let Some(at) = nearest_wood {
        let (cx, cz) = (at.0.div_euclid(16), at.1.div_euclid(16));
        let wood: Vec<Chunk> = (-2..=2)
            .flat_map(|dz| (-2..=2).map(move |dx| ChunkPos::new(cx + dx, cz + dz)))
            .map(|pos| generator.generate_chunk(pos))
            .collect();
        match under_the_crowns(&wood) {
            Some(eye) => {
                println!("oak wood at {at:?}, standing at {eye:?}");
                let before = meshes(&wood, &before_layers, &generator);
                let after = meshes(&wood, &after_layers, &generator);
                println!("wood: {} triangles flat, {} with reliefs", triangles(&before), triangles(&after));
                let was = draw(&before, eye, -90.0, -30.0);
                let now = draw(&after, eye, -90.0, -30.0);
                save(&was, "forest_before");
                save(&now, "forest_after");
                save(&side_by_side(&was, &now), "forest_sheet");
            }
            None => println!("oak wood at {at:?}: nothing lying under the crowns in its middle chunk"),
        }
    }
}

/// **Where the seams on a stone come from**: the grass showing through a
/// stone's corners and the line along its top edges, taken apart one term
/// at a time.
///
/// ```text
/// GPU_REPRO_DIR=C:/absolute/dir cargo test -p primitive_client --lib ///     where_the_seams_on_a_stone_come_from -- --ignored --nocapture
/// ```
///
/// The row from a step away and from two, at one sample and at the player's,
/// each of four ways -- the two halves of the fix taken back one at a time,
/// in this binary:
///
/// * `before` -- each stone as it shipped (`FaceLayers::with_reliefs_as_they_shipped`:
///   the rim topped by the whole picture, sides carried across their line)
///   and the cut-out decided on the filtered alpha;
/// * `geometry` -- the exact surface, the alpha as it was;
/// * `alpha` -- the shipped surface, the cut-out decided on the alpha of the
///   texel under the fragment (`sample_cutout`);
/// * `after` -- both, which is the game now.
///
/// Each is drawn twice: textured, and with the solid pass -- the lawn --
/// painted magenta, so a pixel of ground seen through a stone is a magenta
/// pixel inside it and can be counted rather than squinted at. The count is
/// printed: magenta pixels with stone on both sides, across or along.
#[test]
#[ignore = "a tool: needs a GPU; photographs a stone through variants of the cut-out"]
fn where_the_seams_on_a_stone_come_from() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let generator = WorldGen::new(1337);
    let game = include_str!("shader.wgsl").replace("
", "
");
    let swap = |source: &str, from: &str, to: &str| {
        assert!(source.contains(from), "the shader no longer has {from:?}");
        source.replacen(from, to, 1)
    };
    let filtered_alpha = swap(
        &game,
        "    return vec4<f32>(filtered.rgb, select(filtered.a, under, magnified));
",
        "    return vec4<f32>(filtered.rgb, filtered.a + 0.0 * under * f32(magnified));
",
    );
    let magenta = |source: &str| {
        swap(
            source,
            "    return shade(in, filled, cell_footprint(in.uv));
",
            "    let lit = shade(in, filled, cell_footprint(in.uv));
    return vec4<f32>(1.0, 0.0, 1.0, lit.a);
",
        )
    };
    let lawn = lawn();
    let cut = meshes(&lawn, &textures.face_layers(), &generator);
    let shipped = meshes(&lawn, &textures.face_layers().with_reliefs_as_they_shipped(), &generator);
    let triangles = |m: &[(ChunkPos, MeshBuffers)]| m.iter().map(|(_, b)| b.indices.len() / 3).sum::<usize>();
    println!("lawn: {} triangles as the stones shipped, {} exact", triangles(&shipped), triangles(&cut));
    // A name, a terrain shader and the lawn meshed for it.
    type Variant<'a> = (&'a str, &'a str, &'a [(ChunkPos, MeshBuffers)]);
    let variants: [Variant; 4] = [
        ("before", &filtered_alpha, &shipped),
        ("geometry", &filtered_alpha, &cut),
        ("alpha", &game, &shipped),
        ("after", &game, &cut),
    ];
    let sky = Sky::new(0.36, 900.0);
    let target = Vec3::new(5.5, LYING as f32, 8.5);
    for (view, back, up) in [("near", 0.9f32, 0.8f32), ("step", 1.6, 1.3)] {
        let mut camera = Camera::new((target + Vec3::new(0.0, up, back)).as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = (-90.0f32).to_radians();
        camera.pitch = (-38.0f32).to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        for (name, source, meshes) in variants {
            for samples in [1, settings.msaa] {
                let draw = |source: &str| {
                    super::offscreen_repro::draw_scene(
                        device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, source, None, None, false, samples,
                    )
                };
                let picture = draw(source);
                let stem = format!("{out}/{view}_{name}_{samples}x");
                picture.save(format!("{stem}.png")).expect("write png");
                loupe(&picture, (SIZE.0 / 2, SIZE.1 / 2), (320, 180), 5).save(format!("{stem}_loupe.png")).expect("write png");
                // A crack is ground with stone on both sides of it, across or
                // along: a speck in a corner or a line under an edge. Ground
                // beside a stone has stone on one side only.
                let flagged = draw(&magenta(source));
                let ground = |x: u32, y: u32| {
                    let p = flagged.get_pixel(x, y).0;
                    p[0] > 200 && p[1] < 60 && p[2] > 200
                };
                let mut through = 0usize;
                for y in 1..flagged.height() - 1 {
                    for x in 1..flagged.width() - 1 {
                        if ground(x, y)
                            && ((!ground(x - 1, y) && !ground(x + 1, y)) || (!ground(x, y - 1) && !ground(x, y + 1)))
                        {
                            through += 1;
                        }
                    }
                }
                flagged.save(format!("{stem}_ground.png")).expect("write png");
                println!("{view} {name} {samples}x: {through} pixels of ground with stone either side");
            }
        }
    }
}
