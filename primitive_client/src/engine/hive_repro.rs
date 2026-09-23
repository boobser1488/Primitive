//! A wild hive on a trunk, through the chunk mesher and the real passes.
//!
//! ```text
//! GPU_REPRO_DIR=C:/abs/shots/hive HIVE_TAG=before cargo test -p primitive_client --bin primitive_client \
//!     what_a_wild_hive_on_a_trunk_looks_like -- --ignored --nocapture
//! ```
//!
//! Written for "улей не прикреплён к дереву". A hive is half a cell of comb
//! stuck to the wall its bits name (`types::hive_side`), so which wall that
//! is only shows through the mesher: the model is drawn in the cell *beside*
//! the trunk, and against the wrong wall of that cell it hangs seven
//! sixteenths off the bark with daylight in between. This grows the four
//! trunks a hive can hang on -- one per side, exactly as `place_hives`
//! writes them -- and looks at each from where a player walking up to the
//! tree stands, from both flanks, and from above where the gap is widest.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory.

use super::offscreen_repro::draw_scene;
use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    hive_against, Chunk, ChunkPos, Facing, BLOCK_AIR, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_LOG,
    BLOCK_STONE, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);

#[test]
#[ignore = "a tool: needs a GPU; walks round a hive on a trunk"]
fn what_a_wild_hive_on_a_trunk_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let tag = std::env::var("HIVE_TAG").unwrap_or_else(|_| "now".to_string());
    // The reporting player's lens and filtering.
    let settings = crate::settings::ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..Default::default() };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let layers = textures.face_layers();
    let sky = Sky::new(0.42, 900.0);

    const GROUND: i32 = 10;
    /// `worldgen::place_hives`' own rise: five cells over the ground.
    const RISE: i32 = 5;
    // One trunk per side of the cell, so every quarter turn of the model is
    // in the picture. `(dx, dz)` is the step from the trunk to the hive's
    // cell, which is what `place_hives` walks.
    let trees = [
        ("east", (12, 12), (1i32, 0i32)),
        ("west", (28, 12), (-1, 0)),
        ("south", (12, 28), (0, 1)),
        ("north", (28, 28), (0, -1)),
    ];
    let mut placed: Vec<((i32, i32, i32), primitive_shared::types::BlockId)> = Vec::new();
    for &(_, (tx, tz), (dx, dz)) in &trees {
        for dy in 1..=8 {
            placed.push(((tx, GROUND + dy, tz), BLOCK_LOG));
        }
        // A crown, so the trunk reads as a tree and the hive is lit as it
        // is in a wood: under leaves, not in open sky.
        for ly in 7..=9 {
            for ox in -2i32..=2 {
                for oz in -2i32..=2 {
                    if (ox, oz) == (0, 0) && ly <= 8 {
                        continue;
                    }
                    if ox.abs() + oz.abs() > 3 - (ly - 7) {
                        continue;
                    }
                    placed.push(((tx + ox, GROUND + ly, tz + oz), BLOCK_LEAVES));
                }
            }
        }
        // The hive exactly as the generator writes it: in the cell beside
        // the trunk, carrying the facing `place_hives` picks.
        let toward_trunk = match (dx, dz) {
            (1, 0) => Facing::West,
            (-1, 0) => Facing::East,
            (0, 1) => Facing::North,
            _ => Facing::South,
        };
        let hive = hive_against(
            primitive_shared::bees::hive_holding(primitive_shared::bees::HIVE_FULL),
            toward_trunk,
        );
        placed.push(((tx + dx, GROUND + RISE, tz + dz), hive));
    }

    let span: i32 = 3;
    let mut chunks = ChunkManager::new(span + 2);
    for cz in 0..span {
        for cx in 0..span {
            let mut data = vec![BLOCK_AIR; CHUNK_VOLUME];
            for x in 0..16 {
                for z in 0..16 {
                    let (gx, gz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    for y in 0..=GROUND + 12 {
                        let id = if let Some(&(_, id)) = placed.iter().find(|(at, _)| *at == (gx, y, gz)) {
                            id
                        } else if y < GROUND {
                            BLOCK_STONE
                        } else if y == GROUND {
                            BLOCK_GRASS
                        } else {
                            BLOCK_AIR
                        };
                        data[Chunk::index(x, y as usize, z)] = id;
                    }
                }
            }
            chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks: data });
        }
    }
    let mut light = LightMap::new();
    for cz in 0..span {
        for cx in 0..span {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let mut cache = Box::<Neighbourhood>::default();
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..span * span)
        .map(|i| {
            let pos = ChunkPos::new(i % span, i / span);
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();

    let shoot = |name: String, eye: Vec3, target: Vec3| {
        let look = (target - eye).normalize();
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = look.z.atan2(look.x);
        camera.pitch = look.y.asin();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let mut png = draw_scene(
            device, queue, &textures, &settings, &camera, &sky, &meshes, SIZE,
            include_str!("shader.wgsl"), None, None, false, settings.msaa.max(1),
        );
        for pixel in png.pixels_mut() {
            pixel.0[3] = 255;
        }
        let path = format!("{out}/hive_{tag}_{name}.png").to_lowercase();
        png.save(&path).expect("write png");
        println!("  {path}");
    };
    for (name, (tx, tz), (dx, dz)) in trees {
        // The comb's own cell, and the two directions that matter: out from
        // the trunk (where a player walks up) and along the bark (where a
        // gap between comb and trunk is a stripe of sky).
        let comb = Vec3::new((tx + dx) as f32 + 0.5, (GROUND + RISE) as f32 + 0.5, (tz + dz) as f32 + 0.5);
        let out_of = Vec3::new(dx as f32, 0.0, dz as f32);
        let along = Vec3::new(-dz as f32, 0.0, dx as f32);
        // A player standing on the ground under the tree: eyes 1.62 up, and
        // the hive five cells over their feet, so they look up at it.
        let feet = (GROUND + 1) as f32;
        let seats = [
            ("front", comb + out_of * 4.0 + Vec3::new(0.0, feet + 1.62 - comb.y, 0.0), comb),
            ("front_near", comb + out_of * 1.8 + Vec3::new(0.0, feet + 1.62 - comb.y, 0.0), comb),
            ("flank_left", comb + along * 3.5 + out_of * 1.0 + Vec3::new(0.0, 0.2, 0.0), comb),
            ("flank_right", comb - along * 3.5 + out_of * 1.0 + Vec3::new(0.0, 0.2, 0.0), comb),
            ("level", comb + out_of * 3.0 + along * 0.4, comb),
            // Over the comb but under the crown: the leaves start two cells
            // higher, and a seat inside them is a picture of leaves.
            ("above", comb + out_of * 3.0 + along * 1.2 + Vec3::new(0.0, 1.3, 0.0), comb),
            ("below", comb + out_of * 2.0 + Vec3::new(0.0, -2.5, 0.0), comb),
        ];
        for (seat, eye, target) in seats {
            shoot(format!("{name}_{seat}"), eye, target);
        }
    }
}
