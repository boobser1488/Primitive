//! What a scenario looked at, through the real renderer.
//!
//! The same offscreen path every `*_repro` uses (`draw_scene`: the game's
//! `shader.wgsl`, its texture array and its sampler) over the meshes the
//! real mesher builds from what *this client* has been told -- so a
//! picture of a scenario is a picture of the world the assertions were
//! made against, not a world rebuilt for the camera.
//!
//! Loaded once per process: the texture array is the slow part, and a run
//! of scenarios that each loaded it would spend its time there.

use std::path::Path;
use std::sync::OnceLock;

use primitive_shared::types::ChunkPos;

use super::Scenario;
use crate::engine::texture::TextureManager;

const SIZE: (u32, u32) = (960, 540);

pub(super) fn write(scenario: &Scenario, dir: &Path, name: &str) {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("[scenario] no GPU adapter; no picture of {name}");
        return;
    };
    static TEXTURES: OnceLock<TextureManager> = OnceLock::new();
    let settings = crate::settings::ClientSettings::default();
    let textures = TEXTURES.get_or_init(|| {
        let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
        TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load")
    });
    // The meshes, with the real face layers this time: the ones the
    // assertions read use empty layers because they ask where, not what.
    let layers = textures.face_layers();
    let centre = ChunkPos::from_world(scenario.player.position.x.floor() as i32, scenario.player.position.z.floor() as i32);
    let mut light = primitive_shared::lighting::LightMap::new();
    let mut around = Vec::new();
    for dz in -2..=2 {
        for dx in -2..=2 {
            let pos = ChunkPos::new(centre.x + dx, centre.z + dz);
            if scenario.chunks.is_loaded(pos) {
                light.load_chunk(&scenario.chunks, pos);
                around.push(pos);
            }
        }
    }
    let generator = primitive_shared::worldgen::WorldGen::new(scenario.welcome.world_seed);
    let mut cache = Box::<crate::engine::mesh::Neighbourhood>::default();
    let meshes: Vec<_> = around
        .into_iter()
        .map(|pos| {
            cache.fill(pos, &scenario.chunks, &light);
            let mut buffers = crate::engine::mesh::MeshBuffers::default();
            crate::engine::mesh::build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            (pos, buffers)
        })
        .collect();
    let sky = crate::engine::sky::Sky::new(0.5, 900.0);
    let mut camera = crate::engine::camera::Camera::new(scenario.camera.position, SIZE.0 as f32 / SIZE.1 as f32);
    camera.yaw = scenario.camera.yaw;
    camera.pitch = scenario.camera.pitch;
    let mut png = crate::engine::renderer::offscreen_repro::draw_scene(
        device,
        queue,
        textures,
        &settings,
        &camera,
        &sky,
        &meshes,
        SIZE,
        include_str!("../engine/shader.wgsl"),
        None,
        None,
        scenario.player.in_water,
        settings.msaa.max(1),
    );
    for pixel in png.pixels_mut() {
        pixel.0[3] = 255;
    }
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!("{name}.png"));
    png.save(&path).expect("write png");
    println!("[scenario] {}", path.display());
}
