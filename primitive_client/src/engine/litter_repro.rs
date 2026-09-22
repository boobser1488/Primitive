//! **Leaf litter with holes, lying flush on the turf, measured for z-fighting**
//! through the real terrain shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir \
//!     cargo test -p primitive_client --lib how_litter_lies_on_the_ground -- --ignored --nocapture
//! ```
//!
//! Written for "опавшую листву сделай прозрачной". The litter's holes show the
//! face under it again, and it lies on that face exactly -- no lift, which
//! the player saw as leaves hovering -- told apart from it by a nudge of its
//! depth in the vertex shader (`mesh::DECAL_TINT`, `DECAL_DEPTH`).
//!
//! **How a fight is counted, without a person looking.** Three frames of one
//! field from one eye: the litter over turf, the same litter over stone, and
//! the litter over nothing with the sky left out. Where the third is not the
//! backdrop the litter itself was drawn, and there the first two must be the
//! same pixel -- a leaf is a leaf whatever is under it. A pixel that differs
//! is the ground coming through a leaf: a fight. Counted at every eye, for
//! every nudge in `NUDGES` (0 is the frame without one), and printed; the
//! frames are written for the eye too, with the wood of every litter in a
//! row of its own, and a `before_*` with the picture the litter wore before
//! its holes were opened (earth painted in, from git), so the change can be
//! looked at as well as counted.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    in_wood, BlockId, Chunk, BLOCK_AIR, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LEAF_LITTER, BLOCK_STONE, CHUNK_SIZE_X,
    CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;

const SIZE: (u32, u32) = (1280, 720);
/// Chunks either side of the middle one: a field 144 blocks across, so an
/// eye at one edge sees litter a hundred blocks off.
const SPAN: i32 = 4;
/// The height of the turf.
const G: usize = 40;
/// The nudges the fight is counted for, as the shader's `DECAL_DEPTH` and
/// `DECAL_SLOPE`: none, the constant alone at one, four and sixteen of the
/// depth buffer's steps, and the constant with the slope term -- the last
/// is the shader's own.
const NUDGES: [(f32, f32); 5] = [(0.0, 0.0), (6.0e-8, 0.0), (2.4e-7, 0.0), (9.5e-7, 0.0), (9.5e-7, 6.0e-7)];

/// The field: `ground` at `G`, dirt under it, and litter over all of it,
/// each wood in a band of its own across z so a frame shows all six.
fn mesh_field(
    generator: &WorldGen,
    ground: Option<BlockId>,
    layers: &crate::engine::texture::FaceLayers,
) -> Vec<(ChunkPos, MeshBuffers)> {
    let ring: Vec<ChunkPos> = (-SPAN - 1..=SPAN + 1)
        .flat_map(|dz| (-SPAN - 1..=SPAN + 1).map(move |dx| ChunkPos::new(dx, dz)))
        .collect();
    let mut chunks = ChunkManager::new(SPAN + 2);
    for &pos in &ring {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                if let Some(ground) = ground {
                    for y in 0..G {
                        blocks[Chunk::index(x, y, z)] = BLOCK_DIRT;
                    }
                    blocks[Chunk::index(x, G, z)] = ground;
                }
                let gz = pos.z * CHUNK_SIZE_Z as i32 + z as i32;
                let wood = gz.rem_euclid(24) as usize / 4;
                blocks[Chunk::index(x, G + 1, z)] = in_wood(BLOCK_LEAF_LITTER, wood);
            }
        }
        chunks.insert(Chunk { pos, blocks });
    }
    let mut light = LightMap::new();
    for &pos in &ring {
        light.load_chunk(&chunks, pos);
    }
    let mut cache = Neighbourhood::default();
    ring.iter()
        .filter(|pos| pos.x.abs() <= SPAN && pos.z.abs() <= SPAN)
        .map(|&pos| {
            let mut buffers = MeshBuffers::default();
            cache.fill(pos, &chunks, &light);
            build_mesh(pos, &cache, layers, generator, &mut buffers);
            (pos, buffers)
        })
        .collect()
}

/// The eyes: a player's height looking down at the litter at their feet and
/// along the floor of the wood, higher up at a steeper look, and far off.
fn eyes() -> Vec<(&'static str, Vec3, Vec3)> {
    let floor = (G + 1) as f32;
    let edge = -(SPAN as f32) * 16.0 + 2.0;
    vec![
        ("feet", Vec3::new(0.5, floor + 1.62, 0.5), Vec3::new(2.5, floor, 3.5)),
        ("along", Vec3::new(0.5, floor + 1.62, edge), Vec3::new(0.5, floor, edge + 30.0)),
        ("grazing", Vec3::new(0.5, floor + 0.6, edge), Vec3::new(0.5, floor, edge + 90.0)),
        // A body lying on the floor of the wood, looking the length of it:
        // the most glancing look at the litter there is.
        ("skimming", Vec3::new(0.5, floor + 0.25, edge), Vec3::new(0.5, floor + 0.1, edge + 130.0)),
        ("lying", Vec3::new(0.5, floor + 0.1, edge), Vec3::new(0.5, floor + 0.1, edge + 60.0)),
        ("steep", Vec3::new(0.5, floor + 12.0, -10.0), Vec3::new(0.5, floor, 4.0)),
        ("far", Vec3::new(0.5, floor + 30.0, edge), Vec3::new(0.5, floor, edge + 110.0)),
    ]
}

#[test]
#[ignore = "a tool: needs a GPU; photographs leaf litter on the ground and counts where the ground fights it"]
fn how_litter_lies_on_the_ground() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = {
        let mut settings = ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..ClientSettings::default() };
        settings.sanitize();
        settings
    };
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let generator = WorldGen::new(1337);
    let sky = Sky::new(0.32, 900.0);
    let layers = textures.face_layers();
    let turf = mesh_field(&generator, Some(BLOCK_GRASS), &layers);
    let stone = mesh_field(&generator, Some(BLOCK_STONE), &layers);
    let bare = mesh_field(&generator, None, &layers);
    const MAGENTA: wgpu::Color = wgpu::Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };
    let shader = include_str!("shader.wgsl");
    let with_nudge = |(depth, slope): (f32, f32)| {
        let lines = ["const DECAL_DEPTH: f32 = 9.5e-7;", "const DECAL_SLOPE: f32 = 6.0e-7;"];
        assert!(lines.iter().all(|l| shader.contains(l)), "the shader's nudge is not the lines this tool rewrites");
        shader
            .replace(lines[0], &format!("const DECAL_DEPTH: f32 = {depth:e};"))
            .replace(lines[1], &format!("const DECAL_SLOPE: f32 = {slope:e};"))
    };
    let draw = |meshes: &[(ChunkPos, MeshBuffers)], source: &str, eye: Vec3, at: Vec3, backdrop: Option<wgpu::Color>| {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        let dir = (at - eye).normalize();
        camera.yaw = dir.z.atan2(dir.x);
        camera.pitch = dir.y.asin();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        super::offscreen_repro::draw_scene(
            device, queue, &textures, &settings, &camera, &sky, meshes, SIZE, source, None, backdrop, false, 1,
        )
    };
    for (name, eye, at) in eyes() {
        // Where litter is drawn at all, from the frame over nothing.
        let mask = draw(&bare, shader, eye, at, Some(MAGENTA));
        let litter: Vec<bool> = mask.pixels().map(|p| !(p[0] > 240 && p[1] < 16 && p[2] > 240)).collect();
        let covered = litter.iter().filter(|&&l| l).count();
        let mut line = format!("{name:>8}: {covered:>7} litter pixels;");
        for nudge in NUDGES {
            let source = with_nudge(nudge);
            let a = draw(&turf, &source, eye, at, None);
            let b = draw(&stone, &source, eye, at, None);
            let fights = a
                .pixels()
                .zip(b.pixels())
                .zip(&litter)
                .filter(|((pa, pb), &l)| l && (0..3).any(|c| (i32::from(pa[c]) - i32::from(pb[c])).abs() > 6))
                .count();
            line += &format!(" {:.1e}+{:.1e}/h: {fights:>6}", nudge.0, nudge.1);
            // Where they are, red on the frame, for a nudge that still has
            // any: a fight is a question of where as much as of how many.
            if fights > 0 && nudge == NUDGES[NUDGES.len() - 1] {
                let mut marked = a.clone();
                for ((pixel, (pa, pb)), &l) in marked.pixels_mut().zip(a.pixels().zip(b.pixels())).zip(&litter) {
                    if l && (0..3).any(|c| (i32::from(pa[c]) - i32::from(pb[c])).abs() > 6) {
                        *pixel = image::Rgba([255, 0, 0, 255]);
                    }
                }
                marked.save(format!("{out}/fights_{name}.png")).expect("write picture");
            }
            if nudge.0 == 0.0 || nudge == NUDGES[NUDGES.len() - 1] {
                let tag = if nudge.0 == 0.0 { "unnudged" } else { "after" };
                a.save(format!("{out}/{tag}_{name}.png")).expect("write picture");
            }
        }
        println!("{line}");
    }
    // **Before**: the picture the litter wore before its holes were opened,
    // from git, drawn over the same turf -- which, with every hole painted
    // earth, is the frame the player had (the face under it was not drawn,
    // and nothing of it showed).
    let before_assets = std::env::temp_dir().join("primitive_litter_before_assets");
    let _ = std::fs::remove_dir_all(&before_assets);
    let copied = std::process::Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|top| std::path::PathBuf::from(top.trim()));
    let Some(top) = copied else {
        println!("no git: no before pictures");
        return;
    };
    copy_dir(&assets.join("textures"), &before_assets.join("textures"));
    copy_dir(&assets.join("models"), &before_assets.join("models"));
    for wood in ["", "birch/", "fir/", "saxaul/", "pine/", "willow/"] {
        let path = format!("assets/textures/plants/{wood}leaf_litter.png");
        let old = std::process::Command::new("git")
            .args(["-C", top.to_str().unwrap_or("."), "show", &format!("509e293:{path}")])
            .output()
            .expect("git show");
        std::fs::write(before_assets.join(&path["assets/".len()..]), old.stdout).expect("write old picture");
    }
    let old = TextureManager::load(device, queue, &before_assets, settings.anisotropy).expect("old textures load");
    let old_turf = mesh_field(&generator, Some(BLOCK_GRASS), &old.face_layers());
    for (name, eye, at) in eyes() {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        let dir = (at - eye).normalize();
        camera.yaw = dir.z.atan2(dir.x);
        camera.pitch = dir.y.asin();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let picture = super::offscreen_repro::draw_scene(
            device, queue, &old, &settings, &camera, &sky, &old_turf, SIZE, shader, None, None, false, 1,
        );
        picture.save(format!("{out}/before_{name}.png")).expect("write picture");
    }
    println!("pictures in {out}");
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("make directory");
    for entry in std::fs::read_dir(from).expect("read directory").flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).expect("copy picture");
        }
    }
}
