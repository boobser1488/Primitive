//! **A wood at every transparent-leaf distance**, photographed and timed
//! through the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir DRAW_SCENE_PASS_TIMING=40 \
//!     cargo test --release -p primitive_client --lib \
//!     what_a_wood_looks_like_at_every_leaf_distance -- --ignored --nocapture
//! ```
//!
//! Written for "сделай настройку дальности прозрачной листвы". Each variant
//! is one mesh of one patch of an oak wood -- `before`, `solid_everywhere`,
//! `default_6`, `see_through_everywhere` -- seen from the same two eyes at the
//! player's field of view (95) and anisotropy (16):
//!
//! * `floor_*` -- standing on the floor of the wood, eye 1.62 over it, looking
//!   level along it;
//! * `over_*` -- from `LOOKOUT` blocks up, over the crowns, looking across the
//!   wood to its far side, which is where the switch from see-through to solid
//!   happens.
//!
//! With `DRAW_SCENE_PASS_TIMING` set, `draw_scene` also prints what the
//! opaque and the cut-out pass cost on each frame and how many triangles
//! each sent -- the before-and-after numbers in the CHANGELOG are these lines,
//! best of `LEAF_REPRO_ROUNDS` interleaved rounds.
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
use primitive_shared::worldgen::{Biome, WorldGen};

const SIZE: (u32, u32) = (1280, 720);
/// Chunks either side of the eye's: the player's twenty-four would be three
/// thousand chunks to mesh for a picture whose far half is fog.
const RADIUS: i32 = 10;
/// How far over the wood's floor the second eye is: over an oak's crown,
/// which the first attempt at fourteen blocks was inside.
const LOOKOUT: f32 = 26.0;

/// The middle of the largest, flattest oak wood near the world's zero --
/// `flora_repro::find`, for the same reason: a wood on a slope is a picture
/// of the slope.
fn find_wood(generator: &WorldGen) -> (i32, i32) {
    let mut best: Option<((usize, i32), (i32, i32))> = None;
    for gz in (-3000..3000).step_by(128) {
        for gx in (-3000..3000).step_by(128) {
            if generator.biome_at(gx, gz) != Biome::Forest {
                continue;
            }
            let (mut count, mut low, mut high) = (0usize, i32::MAX, i32::MIN);
            for dz in (-96..=96).step_by(16) {
                for dx in (-96..=96).step_by(16) {
                    count += usize::from(generator.biome_at(gx + dx, gz + dz) == Biome::Forest);
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
    best.map(|(_, at)| at).expect("no oak wood within three kilometres")
}

/// How the leaves of each chunk are decided.
#[derive(Clone, Copy)]
enum Leaves {
    /// **The frame before the setting, rebuilt in this binary**: every chunk
    /// meshed with its crowns' insides (a snapshot nobody told, which
    /// `a_chunk_told_its_leaves_are_solid_keeps_only_the_outside_of_its_crowns_and_says_so`
    /// holds byte for byte to the old mesher), and drawn solid past the
    /// renderer's old hidden line at 0.45 of the view distance, measured
    /// from the eye to the chunk's middle as `render` measured it.
    Before,
    /// The setting, in chunks, decided per chunk the way `dispatch_meshing`
    /// decides it for a chunk nobody has meshed yet.
    Setting(i32),
}

fn mesh_patch(
    generator: &WorldGen,
    centre: ChunkPos,
    layers: &crate::engine::texture::FaceLayers,
    leaves: Leaves,
    eye: Vec3,
) -> Vec<(ChunkPos, MeshBuffers)> {
    let ring = |reach: i32| {
        (-reach..=reach)
            .flat_map(move |dz| (-reach..=reach).map(move |dx| ChunkPos::new(centre.x + dx, centre.z + dz)))
            .collect::<Vec<_>>()
    };
    let mut chunks = ChunkManager::new(RADIUS + 2);
    for pos in ring(RADIUS + 1) {
        chunks.insert(generator.generate_chunk(pos));
    }
    let mut light = LightMap::new();
    for pos in ring(RADIUS + 1) {
        light.load_chunk(&chunks, pos);
    }
    let mut cache = Neighbourhood::default();
    ring(RADIUS)
        .into_iter()
        .map(|pos| {
            let mut buffers = MeshBuffers::default();
            cache.fill(pos, &chunks, &light);
            let dx = (pos.x - centre.x) as f32;
            let dz = (pos.z - centre.z) as f32;
            if let Leaves::Setting(chunks) = leaves {
                let see_through = crate::engine::lod::leaves_see_through_at((dx * dx + dz * dz).sqrt(), chunks, true);
                cache.draw_leaves_solid(!see_through);
            }
            build_mesh(pos, &cache, layers, generator, &mut buffers);
            if let Leaves::Before = leaves {
                let view = (RADIUS * 16) as f32;
                let mx = (pos.x as f32 + 0.5) * 16.0 - eye.x;
                let mz = (pos.z as f32 + 0.5) * 16.0 - eye.z;
                buffers.leaves_solid = mx * mx + mz * mz > (view * 0.45).powi(2);
            }
            (pos, buffers)
        })
        .collect()
}

#[test]
#[ignore = "a tool: needs a GPU; photographs and times a wood at each leaf distance"]
fn what_a_wood_looks_like_at_every_leaf_distance() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let base = {
        let mut settings = ClientSettings {
            anisotropy: 16,
            fov_degrees: 95.0,
            render_distance_chunks: RADIUS,
            ..ClientSettings::default()
        };
        settings.sanitize();
        settings
    };
    let textures = TextureManager::load(device, queue, assets, base.anisotropy).expect("textures load");
    let generator = WorldGen::new(1337);
    let (gx, gz) = find_wood(&generator);
    let centre = ChunkPos::new(gx.div_euclid(16), gz.div_euclid(16));
    let ground = generator.height_at(gx, gz) as f32 + 1.0;
    println!("oak wood at {gx}, {gz}, ground {ground}");
    let sky = Sky::new(0.32, 900.0);

    let everywhere = crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE;
    let variants = [
        ("before", Leaves::Before),
        ("solid_everywhere", Leaves::Setting(0)),
        ("default_6", Leaves::Setting(6)),
        ("see_through_everywhere", Leaves::Setting(everywhere)),
    ];
    let floor_eye = Vec3::new(gx as f32 + 0.5, ground + 1.62, gz as f32 + 0.5);
    // Rounds, interleaved: every variant once per round, so a slow stretch
    // of the machine lands on all four rather than on whichever was being
    // timed then. Pictures in the first round only.
    let rounds: usize = std::env::var("LEAF_REPRO_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for (round, (name, leaves)) in (0..rounds).flat_map(|r| variants.iter().map(move |v| (r, *v))) {
        let settings = ClientSettings { transparent_leaves_chunks: everywhere, ..base.clone() };
        let meshes = mesh_patch(&generator, centre, &textures.face_layers(), leaves, floor_eye);
        let (mut leaf, mut leaf_cut) = (0u32, 0u32);
        for (_, mesh) in &meshes {
            let n = (mesh.leaf_end - mesh.solid_index_count) / 3;
            leaf += n;
            if !mesh.leaves_solid {
                leaf_cut += n;
            }
        }
        println!("round {round} {name}: {leaf} leaf triangles, {leaf_cut} of them cut out, {} solid", leaf - leaf_cut);
        for (view, eye, pitch) in [
            ("floor", floor_eye, 0.02f32),
            ("over", Vec3::new(gx as f32 + 0.5, ground + LOOKOUT, gz as f32 + 0.5), -0.2),
        ] {
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = 0.0;
            camera.pitch = pitch;
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            println!("{view}_{name}:");
            let picture = super::offscreen_repro::draw_scene(
                device,
                queue,
                &textures,
                &settings,
                &camera,
                &sky,
                &meshes,
                SIZE,
                include_str!("shader.wgsl"),
                None,
                None,
                false,
                4,
            );
            if round == 0 {
                let path = format!("{out}/{view}_{name}.png");
                picture.save(&path).expect("write picture");
                println!("  {path}");
            }
        }
    }
}
