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

pub(super) fn write(scenario: &Scenario, dir: &Path, name: &str, specks: &[(glam::DVec3, f32, primitive_shared::types::BlockId)]) {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("[scenario] no GPU adapter; no picture of {name}");
        return;
    };
    static TEXTURES: OnceLock<TextureManager> = OnceLock::new();
    // **A reporting player's fov and filtering, when asked**:
    // `PRIMITIVE_SCENARIO_FOV=95 PRIMITIVE_SCENARIO_ANISOTROPY=16`. A
    // picture at the default seventy degrees is a picture from a place no
    // player who reported the fault was standing.
    let read = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok());
    let mut settings = crate::settings::ClientSettings::default();
    if let Some(fov) = read("PRIMITIVE_SCENARIO_FOV") {
        settings.fov_degrees = fov;
    }
    if let Some(anisotropy) = read("PRIMITIVE_SCENARIO_ANISOTROPY") {
        settings.anisotropy = anisotropy as _;
    }
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
            // ...and whatever the scenario asked to be seen in this chunk.
            let (ox, oz) = (f64::from(pos.x) * 16.0, f64::from(pos.z) * 16.0);
            let mine: Vec<([f32; 3], f32, u32)> = specks
                .iter()
                .filter(|(at, _, _)| ChunkPos::from_world(at.x.floor() as i32, at.z.floor() as i32) == pos)
                .map(|&(at, half, block)| {
                    let local = [(at.x - ox) as f32, at.y as f32, (at.z - oz) as f32];
                    (local, half, layers.layer_for_face(block, crate::engine::texture::FACE_TOP))
                })
                .collect();
            crate::engine::mesh::add_specks(&mut buffers, &mine);
            // **...and the animals**, which the offscreen tool does not draw
            // either: every entity this client has been told about, built by
            // the game's own entity list around this chunk's corner and put in
            // the chunk the camera stands in. Built fresh from the last states
            // heard rather than from a list that has been easing them, so what
            // is drawn is the pose the snapshot says -- lying, head up, in the
            // water -- and not somewhere between two.
            if pos == centre {
                let states: Vec<_> = scenario.entities.values().copied().collect();
                let mut drawn = crate::logic::entities::Entities::default();
                drawn.apply_snapshot(1, &states);
                drawn.tick(0.0);
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                drawn.build_meshes_into(
                    glam::Vec3::new(ox as f32, 0.0, oz as f32),
                    &layers,
                    &light,
                    Some(textures),
                    &mut vertices,
                    &mut indices,
                    &mut Vec::new(),
                    &mut Vec::new(),
                );
                crate::engine::mesh::add_geometry(&mut buffers, &vertices, &indices);
            }
            (pos, buffers)
        })
        .collect();
    let sky = crate::engine::sky::Sky::new(0.5, 900.0);
    let mut camera = crate::engine::camera::Camera::new(scenario.camera.position, SIZE.0 as f32 / SIZE.1 as f32);
    camera.yaw = scenario.camera.yaw;
    camera.pitch = scenario.camera.pitch;
    camera.fov_y_radians = settings.fov_degrees.to_radians();
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
    let path = dir.join(format!("{name}.png"));
    // The *file's* folder, not the one that was asked for: a name may
    // carry a path of its own (`caves/wall-before`), and creating only
    // the top folder left those two scenarios panicking on a write to a
    // directory that was never made -- but only when shots were turned
    // on, which is why it went unnoticed.
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(dir));
    png.save(&path).expect("write png");
    println!("[scenario] {}", path.display());
}
