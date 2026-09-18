//! **Particles through the frame's own matrices**: blood, chips, sparks,
//! smoke and rain round a pond, photographed the way `GraphicsState::render`
//! draws them rather than the way a tool finds convenient.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir PARTICLE_TAG=before cargo test -p primitive_client --lib \
//!     what_particles_look_like_where_they_are -- --ignored --nocapture
//! ```
//!
//! Written for "почини рендер частиц и их положение". The fix that took the
//! particles out of the sky (`Particles::build_into` subtracting the render
//! origin) had never been looked at in a frame, and the one other picture of
//! particles in this crate -- `what_gulls_and_the_small_life_look_like` --
//! builds its matrix at the world's zero with `look_at_rh`, which is exactly
//! the space that fault lived in: a tool drawn that way cannot see it. So this
//! one does what the frame does and nothing else:
//!
//! * the matrix is `Camera::view_proj_about(ORIGIN)` and `camera_pos` is the
//!   camera measured from the same origin -- and the origin is **not** zero,
//!   nor the camera's own block, but a sticky one sixty blocks back, which is
//!   a state `render_origin_for` leaves a walking player in all the time;
//! * the terrain arrives shifted by its chunk's corner less that origin, the
//!   instance offset `GraphicsState::set_chunk_mesh` writes;
//! * the particles come out of the real pool (`Particles::blood`,
//!   `block_broken`, `fires`, `weather`, `update`, all stepped against the
//!   same chunks) and are laid out with `particles::billboard_axes`, the one
//!   function the frame calls for them;
//! * the order is `render`'s: solid, cut-outs, sky, blended water, and then
//!   the particle pass, depth-tested and not depth-writing.
//!
//! **One timeline and four cameras**, so the same drops are photographed from
//! standing height, looking down at the player's own feet, from the bank of the
//! pond and from under its surface. A blow on a pillar, a block coming apart,
//! a campfire, a light rain, a blow inside a dark hut, the player's own blood
//! and a drop born under water are all on it.
//!
//! A child of `renderer` for the reason `lod_repro` is: it fills the private
//! `Globals` and builds the private `GpuMesh`.
//!
//! **`PARTICLE_SCENE=gale`** photographs the same place in an autumn storm
//! instead: the sky overcast and its deck carried by the wind
//! (`Sky::cloud_drift`), and forty seconds of `engine::breeze` -- streaks of
//! air, blown grass, and leaves off the oak -- drawn after the particles as
//! the frame draws them. PNGs are named `particles_gale_*`.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::camera::Camera;
use crate::engine::lighting::Quality;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::particles::{billboard_axes, ParticleVertex, Particles};
use crate::logic::chunk_manager::ChunkManager;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    BlockId, Chunk, BLOCK_AIR, BLOCK_CAMPFIRE_LIT, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_GRASS,
    BLOCK_LEAVES, BLOCK_LOG, BLOCK_SAND, BLOCK_STONE, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const RENDER_DISTANCE: i32 = 8;
/// Where the frame is drawn from. See the module note for why it is neither
/// zero nor the camera's block.
const ORIGIN: Vec3 = Vec3::new(-41.0, -17.0, 38.0);
/// The shot is taken this far into the timeline, in seconds.
const SHOT_AT: f32 = 4.0;
/// Where the player stands, and their eye.
const FEET: Vec3 = Vec3::new(4.5, 4.0, 1.5);
const EYE: Vec3 = Vec3::new(4.5, 4.0 + primitive_shared::geometry::EYE_HEIGHT, 1.5);
/// The top of the pillar a blow lands on, a little above it.
const STRUCK: Vec3 = Vec3::new(5.5, 5.3, 7.5);

/// The scene, as a function of the cell.
///
/// Grass with its top at y = 4; a pond two deep to the right of the player; a
/// cobblestone pillar ahead; a lit campfire beyond it; a tree behind; and on the
/// left a closed hut with a doorway facing the player, so a blow struck deep
/// inside it is struck somewhere the sky does not reach.
fn cell(x: i32, y: i32, z: i32) -> BlockId {
    let water = primitive_shared::fluid::with_depth(primitive_shared::fluid::SOURCE_DEPTH);
    let pond = (9..15).contains(&x) && (2..9).contains(&z);
    // Twelve deep, because sky light only fades a step a block: the first pair
    // of pictures struck the blow four blocks in, where the doorway still
    // lights it at thirteen of fifteen, and the difference between a speck
    // lit where it is and one lit by the open sky was a sixth of its red.
    let hut = (1..=4).contains(&x) && (6..=18).contains(&z);
    let hut_wall = hut && (x == 1 || x == 4 || z == 6 || z == 18);
    let doorway = z == 6 && (2..=3).contains(&x);
    match y {
        0 => BLOCK_STONE,
        1 if pond => BLOCK_SAND,
        2 | 3 if pond => water,
        1 | 2 => BLOCK_DIRT,
        3 => BLOCK_GRASS,
        4 | 5 if hut_wall && !doorway => BLOCK_COBBLESTONE,
        6 if hut => BLOCK_COBBLESTONE,
        4 if (x, z) == (5, 7) => BLOCK_COBBLESTONE,
        4 if (x, z) == (8, 11) => BLOCK_CAMPFIRE_LIT,
        4..=6 if (x, z) == (6, 14) => BLOCK_LOG,
        7 | 8 if (4..=8).contains(&x) && (12..=16).contains(&z) => BLOCK_LEAVES,
        _ => BLOCK_AIR,
    }
}

/// For `PARTICLE_SCENE=gale`: a small oak wood past the pond, on the left of
/// the standing view, so there are crowns enough in reach for the wind to
/// strip. One oak is a leaf every few seconds, which is right and is not a
/// picture.
fn grove(x: i32, y: i32, z: i32) -> Option<BlockId> {
    const TRUNKS: [(i32, i32); 5] = [(18, 14), (23, 13), (18, 20), (24, 19), (28, 16)];
    for (tx, tz) in TRUNKS {
        if (x, z) == (tx, tz) && (4..=6).contains(&y) {
            return Some(BLOCK_LOG);
        }
        if (x - tx).abs() <= 2 && (z - tz).abs() <= 2 && (7..=8).contains(&y) {
            return Some(BLOCK_LEAVES);
        }
    }
    None
}

/// Where the gale's pool is centred: in front of the wood.
const GALE_PLAYER: Vec3 = Vec3::new(15.0, 4.0, 13.0);

fn scene() -> ChunkManager {
    let gale = std::env::var("PARTICLE_SCENE").as_deref() == Ok("gale");
    let mut chunks = ChunkManager::new(16);
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    for y in 0..12 {
                        let (wx, wy, wz) = (cx * CHUNK_SIZE_X as i32 + x as i32, y as i32, cz * CHUNK_SIZE_Z as i32 + z as i32);
                        blocks[Chunk::index(x, y, z)] =
                            gale.then(|| grove(wx, wy, wz)).flatten().unwrap_or_else(|| cell(wx, wy, wz));
                    }
                }
            }
            chunks.insert(Chunk { pos, blocks });
        }
    }
    chunks
}

/// Four seconds of the pool, stepped at sixty frames against the scene, with
/// every emitter the game has fired at its moment on the way.
fn timeline(chunks: &ChunkManager) -> Particles {
    let mut particles = Particles::new();
    let dt = 1.0 / 60.0;
    let frames = (SHOT_AT / dt).round() as usize;
    let at = |seconds: f32| (seconds / dt).round() as usize;
    let wind = Vec3::new(1.2, 0.0, 0.6);
    for frame in 0..frames {
        // A blow two seconds before the shot, whose drops have landed by
        // then, and another a moment before it, whose drops are in the air.
        if frame == at(2.0) || frame == at(SHOT_AT - 0.12) {
            particles.blood(STRUCK);
        }
        if frame == at(SHOT_AT - 0.10) {
            particles.block_broken((7, 4, 6), BLOCK_STONE);
            // A block just in front of the player's feet, for the camera
            // looking down. **In a cell that is air in this scene**: the
            // frame throws chips out of a cell the server has already
            // emptied, and chips thrown out of the middle of a grass block
            // the picture still has in it are under the ground they came out
            // of -- which is how the first pair of pictures showed no chips
            // at the feet either way.
            particles.block_broken((4, 4, 3), BLOCK_GRASS);
        }
        // Into the pond from above, and one born under its surface.
        if frame == at(SHOT_AT - 0.35) {
            particles.blood(Vec3::new(11.5, 4.4, 5.5));
        }
        if frame == at(SHOT_AT - 0.6) {
            particles.blood(Vec3::new(12.0, 3.0, 4.5));
        }
        // Deep in the hut, ten blocks from the doorway, where the sky does
        // not reach.
        if frame == at(SHOT_AT - 0.15) {
            particles.blood(Vec3::new(2.5, 5.2, 16.5));
        }
        // The player's own, where `lib.rs` puts it: a third of a metre
        // below the eye.
        if frame == at(SHOT_AT - 0.25) {
            particles.blood(EYE - Vec3::Y * 0.35);
        }
        particles.weather(0.18, false, FEET, wind, dt);
        particles.fires(chunks, FEET, dt);
        particles.update(chunks, dt);
    }
    particles
}

/// Forty seconds of an autumn gale over the scene, for `PARTICLE_SCENE=gale`:
/// long enough for leaves to come off the oak behind the player and lie.
fn gale(chunks: &ChunkManager, light: &LightMap) -> crate::engine::breeze::Breeze {
    let mut breeze = crate::engine::breeze::Breeze::new();
    let autumn = primitive_shared::season::MIDSUMMER_WORLD_TIME + primitive_shared::season::SEASON_DAYS + 0.42;
    for _ in 0..(40 * 60) {
        let air = crate::engine::breeze::Air {
            chunks,
            light,
            player: GALE_PLAYER,
            // Toward -x: out of the wood and across the standing view.
            wind: primitive_shared::raft::Wind { toward: std::f32::consts::PI - 0.3, strength: 0.8 },
            world_time: autumn,
            wet: false,
        };
        breeze.update(&air, 1.0 / 60.0);
    }
    use crate::engine::breeze::Kind;
    println!(
        "  gale: {} streaks, {} specks, {} leaves",
        breeze.count(Kind::Streak),
        breeze.count(Kind::Dust),
        breeze.count(Kind::Leaf)
    );
    for (at, landed) in breeze.leaves() {
        println!("    leaf at ({:.1}, {:.1}, {:.1}){}", at.x, at.y, at.z, if landed { ", lying" } else { "" });
    }
    breeze
}

/// The chunk meshes and what they are drawn with, for one range at a time. A
/// struct with a lifetime for the reason `raft_repro::Terrain` gives.
struct Terrain<'p> {
    globals: &'p wgpu::BindGroup,
    textures: &'p wgpu::BindGroup,
    arena: &'p crate::engine::arena::Arena,
    offsets: &'p wgpu::Buffer,
    meshes: &'p [([f32; 3], GpuMesh)],
}

impl<'p> Terrain<'p> {
    fn draw(&self, pass: &mut wgpu::RenderPass<'p>, pipeline: &'p wgpu::RenderPipeline, range: fn(&GpuMesh) -> (u32, u32)) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, self.globals, &[]);
        pass.set_bind_group(1, self.textures, &[]);
        pass.set_vertex_buffer(0, self.arena.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.offsets.slice(..));
        pass.set_index_buffer(self.arena.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for (slot, (_, mesh)) in self.meshes.iter().enumerate() {
            let (from, to) = range(mesh);
            if to > from {
                pass.draw_indexed(mesh.indices(from, to), mesh.base_vertex(), instance(slot));
            }
        }
    }
}

fn buffer(device: &wgpu::Device, label: &str, contents: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
}

/// One camera: a name, where the eye is, which way it faces, and whether it is
/// under water.
struct View {
    name: &'static str,
    eye: Vec3,
    yaw: f32,
    pitch: f32,
    underwater: bool,
}

#[test]
#[ignore = "a tool: needs a GPU; writes pictures of particles through the frame's matrices to GPU_REPRO_DIR"]
fn what_particles_look_like_where_they_are() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let scene_gale = std::env::var("PARTICLE_SCENE").as_deref() == Ok("gale");
    let tag = if scene_gale { "gale".to_string() } else { std::env::var("PARTICLE_TAG").unwrap_or_else(|_| "shot".to_string()) };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, 16).expect("textures load");
    let layers = textures.face_layers();

    // ---- the world ----
    let chunks = scene();
    let mut light = LightMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            light.load_chunk(&chunks, ChunkPos::new(cx, cz));
        }
    }
    let generator = primitive_shared::worldgen::WorldGen::new(0);
    let mut arena = crate::engine::arena::Arena::new(device, std::mem::size_of::<Vertex>() as u64);
    let mut meshes: Vec<([f32; 3], GpuMesh)> = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut cache = Neighbourhood::default();
            cache.fill(pos, &chunks, &light);
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            if buffers.indices.is_empty() {
                continue;
            }
            let (vertex_block, index_block) =
                arena.upload(device, queue, &buffers.vertices, &buffers.indices).expect("the scene fits the arena");
            let count = buffers.indices.len() as u32;
            let min = [(cx * CHUNK_SIZE_X as i32) as f32, 0.0, (cz * CHUNK_SIZE_Z as i32) as f32];
            meshes.push((
                min,
                GpuMesh {
                    vertex_block,
                    index_block,
                    num_indices: count,
                    solid_indices: buffers.solid_index_count.min(count),
                    leaf_end: buffers.leaf_end.min(count),
                    sprite_end: buffers.sprite_end.min(count),
                    leaves_solid: buffers.leaves_solid,
                    solid_groups: buffers.solid_groups.map(|end| end.min(count)),
                    up_faces_from: buffers.up_faces_from,
                    down_faces_to: buffers.down_faces_to,
                    top: buffers.top,
                },
            ));
        }
    }
    let particles = timeline(&chunks);
    let breeze = scene_gale.then(|| gale(&chunks, &light));
    println!("  {} particles alive at the shot", particles.len());

    // ---- the pipelines, as `GraphicsState::new` builds them ----
    let quality: Quality = crate::settings::ClientSettings::default().lighting;
    let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle repro globals"),
        size: std::mem::size_of::<Globals>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("particle repro globals layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particle repro globals bind"),
        layout: &globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("particle repro texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let texture_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particle repro textures"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&textures.texture_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&textures.sampler) },
        ],
    });
    let cloud_layout = crate::engine::texture::CloudTexture::bind_group_layout(device);
    let cloud_bind = textures.clouds().bind_group(device, &cloud_layout);
    let terrain_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("particle repro terrain layout"),
        bind_group_layouts: &[&globals_layout, &texture_layout],
        push_constant_ranges: &[],
    });
    let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("particle repro sky layout"),
        bind_group_layouts: &[&globals_layout, &cloud_layout],
        push_constant_ranges: &[],
    });
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: FORMAT,
        width: WIDTH,
        height: HEIGHT,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 1,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
    };
    let look = LookPipelines::new(device, &config, &terrain_layout, &sky_layout, wgpu::MultisampleState::default(), quality, crate::engine::texture::AtlasSplit::ONE);
    // `GraphicsState::new`'s particle pipeline, down to the depth write. Its
    // layout there is the interface's, whose two groups are these two.
    let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("particle repro particle shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("particles.wgsl").into()),
    });
    let particle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("particle repro particle pipeline"),
        layout: Some(&terrain_layout),
        vertex: wgpu::VertexState {
            module: &particle_shader,
            entry_point: "vs_main",
            buffers: &[ParticleVertex::layout()],
        },
        fragment: Some(wgpu::FragmentState {
            module: &particle_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    let offsets: Vec<[f32; 4]> = meshes.iter().map(|(min, _)| [min[0] - ORIGIN.x, -ORIGIN.y, min[2] - ORIGIN.z, 0.0]).collect();
    let chunk_offsets = buffer(device, "particle repro chunk offsets", bytemuck::cast_slice(&offsets), wgpu::BufferUsages::VERTEX);
    let extent = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("particle repro target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let depth_view = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("particle repro depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());

    use std::f32::consts::{FRAC_PI_2, PI};
    let views = [
        View { name: "standing", eye: EYE, yaw: FRAC_PI_2, pitch: -0.18, underwater: false },
        View { name: "looking_at_the_feet", eye: EYE, yaw: FRAC_PI_2, pitch: -1.25, underwater: false },
        View { name: "pond_from_the_bank", eye: Vec3::new(8.0, 4.0 + primitive_shared::geometry::EYE_HEIGHT, 5.5), yaw: 0.0, pitch: -0.5, underwater: false },
        View { name: "under_the_pond", eye: Vec3::new(14.2, 3.45, 5.0), yaw: PI, pitch: 0.12, underwater: true },
        // The gale's own: downwind of the wood, where the leaves come down,
        // looking back up the wind at it.
        View { name: "into_the_wind", eye: Vec3::new(4.0, 4.0 + primitive_shared::geometry::EYE_HEIGHT, 26.0), yaw: -0.5, pitch: -0.25, underwater: false },
    ];
    let views: Vec<View> = views.into_iter().filter(|view| scene_gale || view.name != "into_the_wind").collect();
    let time_of_day = 0.42f32;
    let settings = crate::settings::ClientSettings { lighting: quality, ..Default::default() };
    for view in views {
        let mut camera = Camera::new(view.eye.as_dvec3(), WIDTH as f32 / HEIGHT as f32);
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        camera.yaw = view.yaw;
        camera.pitch = view.pitch;

        // The particles, laid out for this camera exactly as the frame lays
        // them out -- or, with `PARTICLE_TAG=before`, as it laid them out
        // before this tool was written. Both of those faults were arguments
        // and nothing else, so they are reproduced exactly rather than
        // approximated: `(right_horizontal, Y)` is what `billboard_axes`
        // returned, and a light map with nothing lit in it answers the full
        // sky and no block light everywhere, which is the one word the old
        // `Look::light` handed every particle but an ember. What cannot be
        // reproduced from here is the drops sinking through the pond and
        // marking its bed: that was the pool's own step, and the step in the
        // tree is the one that clouds them.
        let before = tag == "before";
        let (right, up) = if before { (camera.right_horizontal(), Vec3::Y) } else { billboard_axes(&camera) };
        let unlit = LightMap::new();
        let (mut pv, mut pi) = (Vec::new(), Vec::new());
        particles.build_into(ORIGIN, right, up, &layers, if before { &unlit } else { &light }, &mut pv, &mut pi);
        // Those across a water surface from the eye go before the water, as
        // `render` draws them -- or, with `PARTICLE_WATER=after`, every one of
        // them after it, the order that drew rain over a lake onto the
        // underside of the lake. See `Particles::behind_water_first`.
        let behind_water = if std::env::var("PARTICLE_WATER").as_deref() == Ok("after") {
            0
        } else {
            particles.behind_water_first(&chunks, view.eye, &mut pi)
        };
        // The wind rides the buffer after the particles, as in `lib.rs`.
        if let Some(breeze) = breeze.as_ref() {
            breeze.build_into(ORIGIN, right, up, &layers, &light, &mut pv, &mut pi);
        }

        // The globals exactly as `render` fills them for this camera.
        let mut sky = crate::engine::sky::Sky::new(time_of_day, 900.0);
        if scene_gale {
            sky.set_weather(primitive_shared::weather::Weather::Storm);
            for _ in 0..(20 * 30) {
                sky.tick(1.0 / 30.0);
            }
        }
        let sun = sky.sun_direction();
        let fog = crate::engine::fog::Fog::for_frame(&settings, &sky, RENDER_DISTANCE, true, view.underwater);
        let aspect = WIDTH as f32 / HEIGHT as f32;
        let view_proj = camera.view_proj_about(ORIGIN);
        let eye_rel = (camera.position - ORIGIN.as_dvec3()).as_vec3();
        let mut globals: Globals = bytemuck::Zeroable::zeroed();
        globals.view_proj = view_proj.to_cols_array_2d();
        globals.camera_pos = [eye_rel.x, eye_rel.y, eye_rel.z, 0.0];
        globals.sun = [sun.x, sun.y, sun.z, sky.sun_intensity()];
        globals.fog_color = [fog.color.x, fog.color.y, fog.color.z, 1.0];
        globals.fog_params = [fog.start, fog.end, settings.ambient_light, aspect];
        globals.extra = [
            settings.block_light_boost,
            settings.ambient_occlusion,
            if view.underwater { 1.0 } else { 0.0 },
            1.0,
        ];
        globals.texture_params = [textures.resolution as f32, 1.0, 0.0, 0.0];
        globals.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        globals.sky_params = [sky.time_of_day, settings.cloudiness, 40.0, sky.overcast()];
        globals.render_origin = [ORIGIN.x, ORIGIN.y, ORIGIN.z, 0.0];
        globals.hand_view_proj = glam::Mat4::perspective_rh(HAND_FOV_Y, aspect, 0.01, 4.0).to_cols_array_2d();
        globals.anim = [
            textures.flame_layer() as f32,
            crate::engine::texture::FLAME_FRAMES as f32,
            crate::engine::texture::FLAME_FPS,
            (crate::engine::texture::FLAME_FRAMES * crate::engine::texture::FLAME_SHEETS) as f32,
        ];
        let (sun_colour, fill) = (sky.sun_color_for(quality), sky.fill_color_for(quality));
        globals.sun_color = [sun_colour.x, sun_colour.y, sun_colour.z, 0.0];
        globals.fill_color = [fill.x, fill.y, fill.z, 0.0];
        globals.horizon_glow = fog.glow.to_array();
        globals.sun_haze = fog.haze.to_array();
        globals.glow_dir = glow_dir(sun);
        globals.glow_dir[1] = sky.cloud_drift().x;
        globals.glow_dir[3] = sky.cloud_drift().y;
        queue.write_buffer(&globals_buffer, 0, bytemuck::bytes_of(&globals));

        // Where the blow on the pillar is on this picture, so the drops can
        // be found and judged against the block they came off.
        let clip = view_proj * (STRUCK - ORIGIN).extend(1.0);
        if clip.w > 0.0 {
            let ndc = clip.truncate() / clip.w;
            println!(
                "  {}: {} quads; the blow on the pillar is at pixel ({:.0}, {:.0})",
                view.name,
                pi.len() / 12,
                (ndc.x * 0.5 + 0.5) * WIDTH as f32,
                (0.5 - ndc.y * 0.5) * HEIGHT as f32,
            );
        }

        let particle_buffers = (!pi.is_empty()).then(|| {
            (
                buffer(device, "particle repro vertices", bytemuck::cast_slice(&pv), wgpu::BufferUsages::VERTEX),
                buffer(device, "particle repro indices", bytemuck::cast_slice(&pi), wgpu::BufferUsages::INDEX),
            )
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle repro main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(fog.color.x),
                            g: f64::from(fog.color.y),
                            b: f64::from(fog.color.z),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let chunk = Terrain { globals: &globals_bind, textures: &texture_bind, arena: &arena, offsets: &chunk_offsets, meshes: &meshes };
            chunk.draw(&mut pass, &look.chunk_pipeline, |mesh| (0, mesh.solid_indices));
            chunk.draw(&mut pass, &look.cutout_pipeline, |mesh| (mesh.solid_indices, mesh.sprite_end));
            pass.set_pipeline(&look.sky_pipeline);
            pass.set_bind_group(0, &globals_bind, &[]);
            pass.set_bind_group(1, &cloud_bind, &[]);
            pass.draw(0..3, 0..1);
            // The particles across the water first, then the water, then the
            // rest, as `render` draws them.
            for (stage, range) in [(0, 0..behind_water), (1, behind_water..pi.len() as u32)] {
                if stage == 1 {
                    chunk.draw(&mut pass, &look.transparent_pipeline, |mesh| (mesh.sprite_end, mesh.num_indices));
                }
                if let Some((vertices, indices)) = particle_buffers.as_ref().filter(|_| !range.is_empty()) {
                    pass.set_pipeline(&particle_pipeline);
                    pass.set_bind_group(0, &globals_bind, &[]);
                    pass.set_bind_group(1, &texture_bind, &[]);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(range, 0, 0..1);
                }
            }
        }
        queue.submit(std::iter::once(encoder.finish()));

        let bytes_per_row = (WIDTH * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle repro readback"),
            size: u64::from(bytes_per_row * HEIGHT),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(HEIGHT) },
            },
            extent,
        );
        queue.submit(std::iter::once(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().expect("map never completed").expect("map failed");
        let data = slice.get_mapped_range();
        let image = image::RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
            let at = (y * bytes_per_row + x * 4) as usize;
            image::Rgba([data[at], data[at + 1], data[at + 2], 255])
        });
        drop(data);
        readback.unmap();
        image.save(format!("{out}/particles_{tag}_{}.png", view.name)).expect("write png");
    }
    println!("pictures in {out}");
}
