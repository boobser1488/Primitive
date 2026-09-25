//! **Models in the light**: whether the things standing in the world that
//! are not the world -- an animal, a dropped barrel, somebody else, the
//! thing in the player's own hand -- are lit by the sun the ground under
//! them is lit by, and whether a barrel is a barrel.
//!
//! ```text
//! MODEL_LIGHT_BEFORE_DIR=<a directory holding the old actor.wgsl and hand.wgsl> \
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/model-light \
//!     cargo test -p primitive_client --release --lib \
//!     what_models_look_like_in_the_light -- --ignored --nocapture
//!
//! MODEL_LIGHT_BEFORE_DIR=<same> cargo test -p primitive_client --release --lib \
//!     what_lit_models_cost -- --ignored --nocapture
//! ```
//!
//! Written for the report "освещение не работает на 3д модели, у предметов
//! нету 3д модели только блок или текстура". The lighting step's colours --
//! a golden key, a cool sky fill, the moonlit floor, the shoulder -- live in
//! `shade_lit` in `shader.wgsl`, and a model is lit by them only if the
//! pipeline that draws it runs that function. So the question is put to
//! every pipeline that draws a model, in one frame, beside terrain wearing
//! the same pictures: a barrel standing as a block next to a barrel dropped
//! on the grass, a deer on the same grass, a player in the shadow of a wall,
//! and the hand in the corner of the frame -- at golden hour and at sunset,
//! with the shadows off and on.
//!
//! **Before and after, from one binary.** The shaders other players and the
//! hand were drawn with before are gone from the tree, so
//! `MODEL_LIGHT_BEFORE_DIR` names a copy of them; with it every picture is
//! taken both ways and the cost is timed interleaved. The geometry half of
//! "before" is not a copy: `Entities::build_meshes_as` and
//! `Hand::build_into_as` with `carried_models: false` take the arms every
//! block without a model still takes, which are the arms a barrel took.
//!
//! A child of `renderer` for the reason `lod_repro` is: it fills the private
//! `Globals` and builds the private `DynamicMesh`.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory, and a relative one lands there.

use super::*;
use crate::engine::lighting::Quality;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood, Vertex};
use crate::engine::shadow::{LightView, ShadowMap};
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::entities::Entities;
use crate::net::remote_players::RemotePlayers;
use primitive_shared::animals::Species;
use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{EntityKind, EntityState};
use primitive_shared::types::{
    bed_half, BlockId, Chunk, Facing, BLOCK_AIR, BLOCK_BARREL, BLOCK_BED, BLOCK_COBBLESTONE,
    BLOCK_DIRT, BLOCK_GRASS, BLOCK_JUG, BLOCK_SAND, BLOCK_STONE, BLOCK_TABLE, CHUNK_SIZE_X,
    CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// For the fog: eight chunks puts its start well past the scene, so nothing
/// in these pictures is hazed and a colour is the light's and not the fog's.
const RENDER_DISTANCE: i32 = 8;
/// The player's field of view.
const FOV_DEGREES: f32 = 90.0;
/// The grass is the top of cell y = 3, so everything stands at four.
const GROUND: f32 = 4.0;
/// The corner of the nine chunks, which is also the render origin.
const ORIGIN: Vec3 = Vec3::new(-16.0, 0.0, -16.0);
/// Where the other player stands: in the shadow of the wall at golden hour.
const FIGURE: Vec3 = Vec3::new(3.2, GROUND, 13.5);

/// The blocks of the middle chunk, in its own coordinates, which for chunk
/// (0, 0) are the world's.
fn features() -> Vec<(usize, usize, usize, BlockId)> {
    let mut out = Vec::new();
    // A wall three high along the west edge. The sun sets in the west, so
    // at golden hour its shadow lies east across the first eight blocks.
    for z in 6..=16usize.min(CHUNK_SIZE_Z - 1) {
        for y in 4..=6 {
            out.push((1, y, z, BLOCK_COBBLESTONE));
        }
    }
    // The reference row, out of the shadow: the same pictures the dropped
    // things wear, standing as blocks.
    out.push((10, 4, 5, BLOCK_STONE));
    out.push((11, 4, 5, BLOCK_BARREL));
    out.push((12, 4, 5, BLOCK_JUG));
    out.push((13, 4, 5, BLOCK_TABLE));
    out.push((14, 4, 5, bed_half(Facing::South, false)));
    out.push((14, 4, 4, bed_half(Facing::South, true)));
    // Sand, which is where a warm light shows first.
    for x in 9..=13 {
        for z in 13..=15 {
            out.push((x, 3, z, BLOCK_SAND));
        }
    }
    out
}

struct World {
    arena: crate::engine::arena::Arena,
    meshes: HashMap<ChunkPos, GpuMesh>,
    light: LightMap,
}

fn world(device: &wgpu::Device, queue: &wgpu::Queue, textures: &TextureManager) -> World {
    world_with(device, queue, textures, &[])
}

/// `world`, with more blocks set in the middle chunk -- a body on the grass,
/// for `what_a_player_holds_and_how_a_body_lies`.
fn world_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    extra: &[(usize, usize, usize, BlockId)],
) -> World {
    world_leaving_out(device, queue, textures, extra, &[])
}

/// `world_with`, meshed leaving these chests' lids out, as a chunk mesh does
/// while the frame is drawing them (`Neighbourhood::set_swung_lids`).
fn world_leaving_out(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    extra: &[(usize, usize, usize, BlockId)],
    lids: &[(i32, i32, i32)],
) -> World {
    let mut chunks = ChunkManager::new(16);
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, 0, z)] = BLOCK_STONE;
                    blocks[Chunk::index(x, 1, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 2, z)] = BLOCK_DIRT;
                    blocks[Chunk::index(x, 3, z)] = BLOCK_GRASS;
                }
            }
            if (cx, cz) == (0, 0) {
                for (x, y, z, block) in features().into_iter().chain(extra.iter().copied()) {
                    blocks[Chunk::index(x, y, z)] = block;
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
    let mut arena = crate::engine::arena::Arena::new(device, std::mem::size_of::<Vertex>() as u64);
    let mut meshes = HashMap::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let pos = ChunkPos::new(cx, cz);
            let mut cache = Neighbourhood::default();
            cache.fill(pos, &chunks, &light);
            cache.set_swung_lids(lids.to_vec());
            let mut buffers = MeshBuffers::default();
            build_mesh(pos, &cache, &layers, &generator, &mut buffers);
            if buffers.indices.is_empty() {
                continue;
            }
            // `GraphicsState::set_chunk_mesh`, field for field.
            let (vertex_block, index_block) = arena
                .upload(device, queue, &buffers.vertices, &buffers.indices)
                .expect("the scene fits the arena");
            let count = buffers.indices.len() as u32;
            meshes.insert(
                pos,
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
            );
        }
    }
    World { arena, meshes, light }
}

/// The entity list as the network would leave it: an animal in the sun, one
/// in the shade, and a row of dropped things, all at rest.
fn settled_entities() -> Entities {
    let item = |id: u64, x: f32, z: f32, block: BlockId| EntityState {
        id,
        x: f64::from(x),
        // The server rests an item's centre half its side above the floor.
        y: f64::from(GROUND + 0.15),
        z: f64::from(z),
        kind: EntityKind::Item { block, count: 1 },
    };
    let animal = |id: u64, species: Species, x: f32, z: f32| EntityState {
        id,
        x: f64::from(x),
        y: f64::from(GROUND + species.height() * 0.5),
        z: f64::from(z),
        kind: EntityKind::Animal { species, yaw: 0.0, hurt: 0.0, attitude: primitive_shared::protocol::Attitude::Easy, growth: u8::MAX, tack: 0 },
    };
    let states = vec![
        // In the sun, beside the reference row.
        animal(1, Species::Deer, 15.0, 10.5),
        item(2, 10.5, 8.5, BLOCK_BARREL),
        item(3, 11.5, 8.5, BLOCK_JUG),
        item(4, 12.7, 8.5, BLOCK_TABLE),
        item(5, 14.2, 8.5, BLOCK_BED),
        // In the wall's shadow.
        item(6, 3.5, 9.5, BLOCK_BARREL),
        animal(7, Species::Boar, 4.5, 11.5),
    ];
    let mut entities = Entities::default();
    entities.set_tick_rate(20.0);
    // Twice in one place: that is how the client learns a thing has come to
    // rest, and a resting sprite lies down over a quarter of a second.
    entities.apply_snapshot(1, &states);
    entities.apply_snapshot(2, &states);
    std::thread::sleep(std::time::Duration::from_millis(300));
    entities
}

/// Somebody standing at `FIGURE`, turned to face `eye`.
fn posed_player(eye: Vec3) -> RemotePlayers {
    let mut players = RemotePlayers::default();
    let to = FIGURE - eye;
    let yaw = to.z.atan2(to.x);
    let forward = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    // `pose_for_a_photograph` stands the figure four blocks along the
    // camera's facing from the feet it is handed.
    players.pose_for_a_photograph(FIGURE - forward * 4.0, yaw);
    players
}

fn upload<V: bytemuck::Pod>(device: &wgpu::Device, vertices: &[V], indices: &[u32]) -> DynamicMesh {
    let buffer = |label: &str, contents: &[u8], usage: wgpu::BufferUsages| {
        if contents.is_empty() {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 0, usage, mapped_at_creation: false })
        } else {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        }
    };
    DynamicMesh {
        vertex_buffer: buffer("model light vertices", bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX),
        index_buffer: buffer("model light indices", bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX),
        vertex_capacity: std::mem::size_of_val(vertices),
        index_capacity: std::mem::size_of_val(indices),
        index_count: indices.len() as u32,
    }
}

/// Everything a frame draws that is not terrain, as the game's own builders
/// leave it on the CPU, in the order `lib.rs` calls them.
#[derive(Default)]
struct Geometry {
    entity: (Vec<Vertex>, Vec<u32>),
    item: (Vec<crate::engine::item_model::ItemVertex>, Vec<u32>),
    actor: (Vec<crate::net::remote_players::ActorVertex>, Vec<u32>),
    hand: (Vec<crate::logic::hand::HandVertex>, Vec<u32>),
}

#[allow(clippy::too_many_arguments)]
fn geometry(
    textures: &TextureManager,
    world: &World,
    entities: &Entities,
    players: &RemotePlayers,
    eye: Vec3,
    held: BlockId,
    carried_models: bool,
) -> Geometry {
    let layers = textures.face_layers();
    let mut g = Geometry::default();
    entities.build_meshes_as(
        ORIGIN,
        &layers,
        &world.light,
        Some(textures),
        carried_models,
        &mut g.entity.0,
        &mut g.entity.1,
        &mut g.item.0,
        &mut g.item.1,
    );
    crate::net::remote_players::build_held_items_into(
        players,
        ORIGIN,
        &layers,
        &world.light,
        Some(textures),
        &mut g.entity.0,
        &mut g.entity.1,
        &mut g.item.0,
        &mut g.item.1,
    );
    crate::net::remote_players::build_actor_mesh_into(players, None, ORIGIN, &world.light, &mut g.actor.0, &mut g.actor.1);
    crate::logic::hand::Hand::new().build_into_as(
        true,
        Some(held),
        &layers,
        Some(textures),
        crate::logic::entities::sampled_light(eye.as_dvec3(), &world.light),
        None,
        carried_models,
        &mut g.hand.0,
        &mut g.hand.1,
    );
    g
}

struct Models {
    entity: DynamicMesh,
    item: DynamicMesh,
    actor: DynamicMesh,
    hand: DynamicMesh,
}

fn models(device: &wgpu::Device, g: &Geometry) -> Models {
    Models {
        entity: upload(device, &g.entity.0, &g.entity.1),
        item: upload(device, &g.item.0, &g.item.1),
        actor: upload(device, &g.actor.0, &g.actor.1),
        hand: upload(device, &g.hand.0, &g.hand.1),
    }
}

/// Which code draws the models in a frame.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Look {
    /// Other players and the hand through the shaders they had
    /// (`MODEL_LIGHT_BEFORE_DIR`), dropped items with no shadowed twin, and
    /// a block with a model of its own dropped and held as a sprite or a
    /// cube.
    Before,
    /// This build.
    After,
}

impl Look {
    fn name(self) -> &'static str {
        match self {
            Look::Before => "before",
            Look::After => "after",
        }
    }
}

/// The actor and hand pipelines as `GraphicsState::new` built them before
/// they were lit like the ground: their own shaders, the same at every step.
struct Legacy {
    actor: wgpu::RenderPipeline,
    hand: wgpu::RenderPipeline,
}

fn legacy(device: &wgpu::Device, layout: &wgpu::PipelineLayout, dir: &str, multisample: wgpu::MultisampleState) -> Legacy {
    let module = |file: &str| {
        let path = format!("{dir}/{file}");
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(file), source: wgpu::ShaderSource::Wgsl(source.into()) })
    };
    let (actor, hand) = (module("actor.wgsl"), module("hand.wgsl"));
    // Their descriptors were `model_pipeline`'s in every field that
    // matters -- opaque, depth written, the actor culled and the hand not --
    // so that function builds them, with their old entry points. The old
    // actor shader reads four of the vertex's five attributes, which a
    // pipeline allows.
    Legacy {
        actor: model_pipeline(
            device,
            "legacy actor",
            layout,
            &actor,
            ("vs_main", "fs_main"),
            crate::net::remote_players::ActorVertex::layout(),
            Some(wgpu::Face::Back),
            FORMAT,
            multisample,
        ),
        hand: model_pipeline(
            device,
            "legacy hand",
            layout,
            &hand,
            ("vs_main", "fs_main"),
            crate::logic::hand::HandVertex::layout(),
            None,
            FORMAT,
            multisample,
        ),
    }
}

struct Step {
    quality: Quality,
    look: LookPipelines,
    map: ShadowMap,
    receivers: ModelReceivers,
}

struct Rig<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    width: u32,
    height: u32,
    resolution: f32,
    flame_layer: f32,
    globals_buffer: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    texture_bind: wgpu::BindGroup,
    ui_texture_bind: wgpu::BindGroup,
    skin_bind: wgpu::BindGroup,
    cloud_bind: wgpu::BindGroup,
    zero_offset: wgpu::Buffer,
    chunk_offsets: wgpu::Buffer,
    steps: Vec<Step>,
    legacy: Option<Legacy>,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    sample_view: Option<wgpu::TextureView>,
    depth_view: wgpu::TextureView,
}

impl<'a> Rig<'a> {
    fn new(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        textures: &TextureManager,
        assets: &std::path::Path,
        (width, height): (u32, u32),
        samples: u32,
    ) -> Self {
        let multisample = wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false };
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model light globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model light globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model light globals bind"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model light texture layout"),
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
        let atlas_bind = |label: &str, sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&textures.texture_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        let texture_bind = atlas_bind("model light textures", &textures.sampler);
        // The hand's pass reads the atlas through the always-nearest
        // interface sampler, as `render` binds it.
        let ui_texture_bind = atlas_bind("model light ui textures", &textures.ui_sampler);
        let skin_bind = player_skin(device, queue, assets, &textures.sampler, &texture_layout, crate::engine::texture::AtlasSplit::ONE);
        let cloud_layout = crate::engine::texture::CloudTexture::bind_group_layout(device);
        let cloud_bind = textures.clouds().bind_group(device, &cloud_layout);
        let terrain_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model light terrain layout"),
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model light sky layout"),
            bind_group_layouts: &[&globals_layout, &cloud_layout],
            push_constant_ranges: &[],
        });
        // `LookPipelines::new` wants a surface's configuration for its
        // format and nothing else.
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: FORMAT,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        // The game's own constructors for everything that is this build:
        // a copy of their descriptors here would be a copy free to differ.
        use crate::engine::texture::AtlasSplit;
        let steps = Quality::ALL
            .iter()
            .map(|&quality| {
                let look = LookPipelines::new(device, &config, &terrain_layout, &sky_layout, multisample, quality, AtlasSplit::ONE);
                let map = ShadowMap::new(
                    device,
                    &globals_layout,
                    &texture_layout,
                    FORMAT,
                    depth_format(),
                    samples,
                    crate::engine::shadow::RESOLUTION,
                    (MAX_CHUNK_DRAWS * std::mem::size_of::<[f32; 4]>()) as u64,
                    (MAX_CHUNK_DRAWS * Ranges::MAX * std::mem::size_of::<IndirectDraw>()) as u64,
                    quality,
                    AtlasSplit::ONE,
                );
                let receivers = ModelReceivers::new(device, &globals_layout, &texture_layout, &map, FORMAT, multisample, quality, AtlasSplit::ONE);
                Step { quality, look, map, receivers }
            })
            .collect();
        let legacy = std::env::var("MODEL_LIGHT_BEFORE_DIR").ok().map(|dir| legacy(device, &terrain_layout, &dir, multisample));
        let zero_offset = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model light zero offset"),
            contents: bytemuck::cast_slice(&[[0.0f32; 4]]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let chunk_offsets = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model light chunk offsets"),
            size: (MAX_CHUNK_DRAWS * std::mem::size_of::<[f32; 4]>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = |label: &str, count: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let target = texture("model light target", 1, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let target_view = target.create_view(&Default::default());
        let sample_view = (samples > 1)
            .then(|| texture("model light samples", samples, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default()));
        let depth_view =
            texture("model light depth", samples, depth_format(), wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        Self {
            device,
            queue,
            width,
            height,
            resolution: textures.resolution as f32,
            flame_layer: textures.flame_layer() as f32,
            globals_buffer,
            globals_bind,
            texture_bind,
            ui_texture_bind,
            skin_bind,
            cloud_bind,
            zero_offset,
            chunk_offsets,
            steps,
            legacy,
            target,
            target_view,
            sample_view,
            depth_view,
        }
    }

    fn view_proj(&self, (eye, at): (Vec3, Vec3)) -> glam::Mat4 {
        glam::Mat4::perspective_rh(FOV_DEGREES.to_radians(), self.width as f32 / self.height as f32, 0.05, 1000.0)
            * glam::Mat4::look_at_rh(eye - ORIGIN, at - ORIGIN, Vec3::Y)
    }

    /// One frame, in the order `render` draws one: the shadow map, solid
    /// terrain, cut-outs, the sky, the entities, dropped items, other
    /// players, and the hand in its slice of the depth range.
    #[allow(clippy::too_many_arguments)]
    fn frame(&self, look: Look, quality: Quality, shadows: bool, world: &World, models: &Models, camera: (Vec3, Vec3), time_of_day: f32) {
        let step = self.steps.iter().find(|s| s.quality == quality).expect("every step is built");
        let (eye, _) = camera;
        let settings = crate::settings::ClientSettings { lighting: quality, ..Default::default() };
        let sky = crate::engine::sky::Sky::new(time_of_day, 900.0);
        let sun = sky.sun_direction();
        let fog = crate::engine::fog::Fog::for_frame(&settings, &sky, RENDER_DISTANCE, true, None);
        let aspect = self.width as f32 / self.height as f32;
        let eye_rel = eye - ORIGIN;
        let view_proj = self.view_proj(camera);

        let mut globals: Globals = bytemuck::Zeroable::zeroed();
        globals.view_proj = view_proj.to_cols_array_2d();
        globals.camera_pos = [eye_rel.x, eye_rel.y, eye_rel.z, 0.0];
        globals.sun = [sun.x, sun.y, sun.z, sky.sun_intensity()];
        globals.fog_color = [fog.color.x, fog.color.y, fog.color.z, 1.0];
        globals.fog_params = [fog.start, fog.end, settings.ambient_light, aspect];
        globals.extra = [settings.block_light_boost, settings.ambient_occlusion, 0.0, 1.0];
        globals.texture_params = [self.resolution, 1.0, 0.0, 0.0];
        globals.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        globals.sky_params = [time_of_day, settings.cloudiness, 40.0, 0.0];
        globals.render_origin = [ORIGIN.x, ORIGIN.y, ORIGIN.z, 0.0];
        globals.hand_view_proj = glam::Mat4::perspective_rh(HAND_FOV_Y, aspect, 0.01, 4.0).to_cols_array_2d();
        globals.anim = [
            self.flame_layer,
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
        let light = shadows
            .then(|| crate::engine::shadow::strength(sun, 0.0))
            .filter(|strength| *strength > 0.0)
            .map(|strength| LightView::new(sun, eye, ORIGIN, crate::engine::shadow::RADIUS, step.map.resolution, strength));
        if let Some(light) = &light {
            (globals.shadow_view_proj, globals.shadow_params, globals.shadow_bias) = light.globals(step.map.resolution);
        }
        self.queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let mut order: Vec<(f32, [f32; 3], &GpuMesh)> = world
            .meshes
            .iter()
            .map(|(pos, mesh)| {
                let min = [(pos.x * CHUNK_SIZE_X as i32) as f32, 0.0, (pos.z * CHUNK_SIZE_Z as i32) as f32];
                let (dx, dz) = (min[0] + 8.0 - eye.x, min[2] + 8.0 - eye.z);
                (dx * dx + dz * dz, min, mesh)
            })
            .collect();
        order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let offsets: Vec<[f32; 4]> = order.iter().map(|(_, min, _)| [min[0] - ORIGIN.x, -ORIGIN.y, min[2] - ORIGIN.z, 0.0]).collect();
        self.queue.write_buffer(&self.chunk_offsets, 0, bytemuck::cast_slice(&offsets));

        let mut encoder = self.device.create_command_encoder(&Default::default());
        if let Some(light) = &light {
            ShadowPass {
                queue: self.queue,
                map: &step.map,
                globals: &self.globals_bind,
                textures: &self.texture_bind,
                arena: &world.arena,
                zero_offset: &self.zero_offset,
                multi_draw: self
                    .device
                    .features()
                    .contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE),
                chunks: &world.meshes,
                mode: crate::engine::shadow::Mode::Soft,
                cast_leaves: true,
                plants: Default::default(),
            }
            .draw(&mut encoder, light, eye, ORIGIN, Some(&models.entity), None);
        }
        {
            let clear = wgpu::LoadOp::Clear(wgpu::Color {
                r: f64::from(fog.color.x),
                g: f64::from(fog.color.y),
                b: f64::from(fog.color.z),
                a: 1.0,
            });
            let colour = match &self.sample_view {
                Some(samples) => wgpu::RenderPassColorAttachment {
                    view: samples,
                    resolve_target: Some(&self.target_view),
                    ops: wgpu::Operations { load: clear, store: wgpu::StoreOp::Discard },
                },
                None => wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: clear, store: wgpu::StoreOp::Store },
                },
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model light main pass"),
                color_attachments: &[Some(colour)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let group = light.as_ref().map(|_| &step.map.bind_group);
            let (solid, cutout) = match group {
                Some(_) => (&step.map.solid, &step.map.cutout),
                None => (&step.look.chunk_pipeline, &step.look.cutout_pipeline),
            };
            pass.set_vertex_buffer(0, world.arena.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.chunk_offsets.slice(..));
            pass.set_index_buffer(world.arena.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            for (pipeline, cut) in [(solid, false), (cutout, true)] {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.globals_bind, &[]);
                pass.set_bind_group(1, &self.texture_bind, &[]);
                if let Some(group) = group {
                    pass.set_bind_group(2, group, &[]);
                }
                for (slot, (_, min, mesh)) in order.iter().enumerate() {
                    if cut {
                        if mesh.sprite_end > mesh.solid_indices {
                            pass.draw_indexed(mesh.indices(mesh.solid_indices, mesh.sprite_end), mesh.base_vertex(), instance(slot));
                        }
                        continue;
                    }
                    let ranges = solid_ranges_facing(eye.into(), *min, &mesh.solid_groups, (mesh.up_faces_from, mesh.down_faces_to), None);
                    for (from, to) in ranges.iter() {
                        pass.draw_indexed(mesh.indices(from, to), mesh.base_vertex(), instance(slot));
                    }
                }
            }
            pass.set_pipeline(&step.look.sky_pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_bind_group(1, &self.cloud_bind, &[]);
            pass.draw(0..3, 0..1);

            // What draws each kind of model, and whether it reads the shadow:
            // `render`'s choice for this build, and the old pipelines for
            // the look before -- which had no shadowed model at all.
            let legacy = || self.legacy.as_ref().expect("MODEL_LIGHT_BEFORE_DIR names the old shaders");
            let shadowed = if look == Look::After { group } else { None };
            let item = shadowed.map_or(&step.look.item_pipeline, |_| &step.receivers.item);
            let actor = match look {
                Look::Before => &legacy().actor,
                Look::After => shadowed.map_or(&step.look.actor_pipeline, |_| &step.receivers.actor),
            };
            let hand = match look {
                Look::Before => &legacy().hand,
                Look::After => shadowed.map_or(&step.look.hand_pipeline, |_| &step.receivers.hand),
            };
            // Each draw names its pipeline, its groups and its buffers in
            // full, so none of them depends on what the one before it left
            // bound -- which is a question about wgpu, not about the light.
            let globals = &self.globals_bind;
            draw_mesh(&mut pass, &models.entity, cutout, globals, &self.texture_bind, group, Some(&self.zero_offset));
            draw_mesh(&mut pass, &models.item, item, globals, &self.texture_bind, shadowed, None);
            draw_mesh(&mut pass, &models.actor, actor, globals, &self.skin_bind, shadowed, None);
            pass.set_viewport(0.0, 0.0, self.width as f32, self.height as f32, 0.0, HAND_DEPTH_SLICE);
            draw_mesh(&mut pass, &models.hand, hand, globals, &self.ui_texture_bind, shadowed, None);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn read(&self) -> image::RgbaImage {
        let (width, height) = (self.width, self.height);
        let bytes_per_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model light readback"),
            size: u64::from(bytes_per_row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().expect("map never completed").expect("map failed");
        let data = slice.get_mapped_range();
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let at = (y * bytes_per_row + x * 4) as usize;
            image::Rgba([data[at], data[at + 1], data[at + 2], 255])
        });
        drop(data);
        readback.unmap();
        image
    }
}

/// One dynamic mesh, with everything it needs bound in full.
#[allow(clippy::too_many_arguments)]
fn draw_mesh<'p>(
    pass: &mut wgpu::RenderPass<'p>,
    mesh: &'p DynamicMesh,
    pipeline: &'p wgpu::RenderPipeline,
    globals: &'p wgpu::BindGroup,
    atlas: &'p wgpu::BindGroup,
    shadow: Option<&'p wgpu::BindGroup>,
    offsets: Option<&'p wgpu::Buffer>,
) {
    if mesh.is_empty() {
        return;
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, globals, &[]);
    pass.set_bind_group(1, atlas, &[]);
    if let Some(shadow) = shadow {
        pass.set_bind_group(2, shadow, &[]);
    }
    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
    if let Some(offsets) = offsets {
        pass.set_vertex_buffer(1, offsets.slice(..));
    }
    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
}

/// Pictures at half size on one sheet, two to a row, in the order given.
fn sheet(shots: &[&image::RgbaImage]) -> image::RgbaImage {
    let (w, h) = (shots[0].width() / 2, shots[0].height() / 2);
    let rows = (shots.len() as u32).div_ceil(2);
    let mut sheet = image::RgbaImage::new(w * 2, h * rows);
    for (i, shot) in shots.iter().enumerate() {
        let small = image::imageops::resize(*shot, w, h, image::imageops::FilterType::Triangle);
        image::imageops::replace(&mut sheet, &small, i64::from((i as u32 % 2) * w), i64::from((i as u32 / 2) * h));
    }
    sheet
}

/// The lower right of a frame, where the hand is, twice the size and nearest
/// neighbour, so a held model can be judged texel by texel.
fn hand_corner(shot: &image::RgbaImage) -> image::RgbaImage {
    let (x, y) = (shot.width() * 11 / 20, shot.height() * 9 / 20);
    let corner = image::imageops::crop_imm(shot, x, y, shot.width() - x, shot.height() - y).to_image();
    image::imageops::resize(&corner, corner.width() * 2, corner.height() * 2, image::imageops::FilterType::Nearest)
}

/// The mean colour of a small square of the picture around where a world
/// point lands, and its warmth (red less blue). Supporting numbers for the
/// pictures, not a replacement for looking at them: the square can catch
/// the background at a silhouette's edge.
fn probe(rig: &Rig, shot: &image::RgbaImage, camera: (Vec3, Vec3), point: Vec3) -> Option<[f64; 4]> {
    let clip = rig.view_proj(camera) * (point - ORIGIN).extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let (x, y) = ((clip.x / clip.w * 0.5 + 0.5) * rig.width as f32, (0.5 - clip.y / clip.w * 0.5) * rig.height as f32);
    let (mut sum, mut count) = ([0.0f64; 3], 0.0f64);
    for dy in -3..=3 {
        for dx in -3..=3 {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px < 0 || py < 0 || px >= rig.width as i32 || py >= rig.height as i32 {
                continue;
            }
            let pixel = shot.get_pixel(px as u32, py as u32).0;
            for c in 0..3 {
                sum[c] += f64::from(pixel[c]);
            }
            count += 1.0;
        }
    }
    (count > 0.0).then(|| [sum[0] / count, sum[1] / count, sum[2] / count, (sum[0] - sum[2]) / count])
}

/// Each view holds something different in the hand, so the frames show
/// several held models.
///
/// **`by_the_wall` is beside the wall's shadow and not in it**: the wall runs
/// to z fifteen and that eye stands at sixteen and a half, which the first run
/// showed as a table in the hand lit like the lawn under it -- right, and no
/// evidence about the hand's shadow either way. `under_the_wall` is the view
/// that is: the eye at z eleven and a head's height, well under the line the
/// wall's top throws at golden hour, and so the point the hand looks its
/// shadow up at (`HELD_BELOW_EYE`) with it, beside the barrel dropped there.
const VIEWS: [(&str, Vec3, Vec3, BlockId); 4] = [
    ("wide", Vec3::new(9.5, 6.4, 19.5), Vec3::new(9.0, 4.2, 8.5), BLOCK_BARREL),
    ("drops", Vec3::new(12.4, 5.2, 11.8), Vec3::new(12.4, 4.2, 7.0), BLOCK_JUG),
    ("by_the_wall", Vec3::new(2.6, 5.4, 16.5), Vec3::new(6.5, 4.3, 9.5), BLOCK_TABLE),
    ("under_the_wall", Vec3::new(3.2, 5.2, 11.0), Vec3::new(9.0, 4.3, 8.5), BLOCK_BARREL),
];

fn assets() -> &'static std::path::Path {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"))
}

#[test]
#[ignore = "a tool: needs a GPU; photographs models beside the terrain before and after, shadows off and on"]
fn what_models_look_like_in_the_light() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    // The photograph hook's own knob, read once by the pose code: one figure.
    std::env::set_var("PRIMITIVE_POSE_PLAYERS", "1");
    // Sixteen, the player's anisotropy.
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let world = world(device, queue, &textures);
    let entities = settled_entities();
    let rig = Rig::new(device, queue, &textures, assets(), (1280, 720), 4);
    let looks: Vec<Look> = if rig.legacy.is_some() {
        vec![Look::Before, Look::After]
    } else {
        println!("MODEL_LIGHT_BEFORE_DIR not set: photographing this build only");
        vec![Look::After]
    };
    let modes = [(Quality::Balanced, false), (Quality::Balanced, true), (Quality::High, true)];
    let times = [("golden", 0.70f32), ("sunset", 0.745)];
    // What the numbers are about: a model, and terrain wearing the same
    // picture or standing in the same light.
    let probes: [(&str, Vec3); 7] = [
        ("barrel block", Vec3::new(11.5, 4.45, 5.95)),
        ("dropped barrel", Vec3::new(10.5, 4.12, 8.5)),
        ("barrel in shade", Vec3::new(3.5, 4.12, 9.5)),
        ("grass by it", Vec3::new(10.0, 4.0, 9.6)),
        ("deer", Vec3::new(15.0, 4.0 + Species::Deer.height() * 0.55, 10.5)),
        ("player", FIGURE + Vec3::new(0.0, 1.1, 0.0)),
        ("boar in shade", Vec3::new(4.5, 4.0 + Species::Boar.height() * 0.5, 11.5)),
    ];
    for (view, eye, at, held) in VIEWS {
        let players = posed_player(eye);
        let built: Vec<(Look, Models)> = looks
            .iter()
            .map(|&look| (look, models(device, &geometry(&textures, &world, &entities, &players, eye, held, look == Look::After))))
            .collect();
        for (when, t) in times {
            let mut shots: HashMap<(Look, usize), image::RgbaImage> = HashMap::new();
            for (look, models) in &built {
                for (m, (quality, shadows)) in modes.iter().enumerate() {
                    rig.frame(*look, *quality, *shadows, &world, models, (eye, at), t);
                    let shot = rig.read();
                    let mode = format!("{}{}", format!("{quality:?}").to_lowercase(), if *shadows { "+shadows" } else { "" });
                    shot.save(format!("{out}/{view}_{when}_{mode}_{}.png", look.name())).expect("write png");
                    hand_corner(&shot).save(format!("{out}/hand_{view}_{when}_{mode}_{}.png", look.name())).expect("write png");
                    let line: Vec<String> = probes
                        .iter()
                        .filter_map(|(name, point)| {
                            probe(&rig, &shot, (eye, at), *point).map(|[r, g, b, warm]| format!("{name} ({r:.0},{g:.0},{b:.0}) warm {warm:+.0}"))
                        })
                        .collect();
                    println!("{view} {when} {mode} {}: {}", look.name(), line.join(" | "));
                    shots.insert((*look, m), shot);
                }
            }
            // Before on the left and after on the right; Balanced above,
            // Balanced with the shadows below.
            let ordered: Vec<&image::RgbaImage> = [0usize, 1]
                .iter()
                .flat_map(|&m| looks.iter().map(move |&look| (look, m)))
                .filter_map(|key| shots.get(&key))
                .collect();
            sheet(&ordered).save(format!("{out}/sheet_{view}_{when}.png")).expect("write png");
        }
    }
    println!("pictures in {out}");
}

/// **What lighting the models like the ground costs, measured.**
///
/// Two cameras at golden hour -- the wide one, where the models are a small
/// part of the frame, and one a few paces from the other player, standing in
/// the wall's shadow with the hand in shadow too, where they are as much of it
/// as they are going to be -- at Balanced without shadows and High with them,
/// before and after interleaved in rounds so whatever the card's temperature
/// does it does to both. Each frame is timed from the first command to the
/// GPU reporting everything done, at 1920x1080 and four samples, the desktop
/// default. The geometry is this build's in both looks, so what differs is the
/// shading; the geometry's own price is the CPU line at the end.
#[test]
#[ignore = "a tool: needs a GPU and MODEL_LIGHT_BEFORE_DIR; times models lit the old way and the new"]
fn what_lit_models_cost() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    std::env::set_var("PRIMITIVE_POSE_PLAYERS", "1");
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let world = world(device, queue, &textures);
    let entities = settled_entities();
    let rig = Rig::new(device, queue, &textures, assets(), (1920, 1080), 4);
    if rig.legacy.is_none() {
        println!("MODEL_LIGHT_BEFORE_DIR not set: nothing to compare against");
        return;
    }
    let cameras = [
        ("wide", VIEWS[0].1, VIEWS[0].2, BLOCK_BARREL),
        // Under the wall and not beside it -- see `VIEWS` for the first run's
        // eye, which stood past the wall's end in the sun.
        ("close_in_shadow", Vec3::new(3.4, 5.2, 12.0), FIGURE + Vec3::new(0.0, 1.0, 0.0), BLOCK_JUG),
    ];
    let (rounds, frames) = (8, 30);
    for (name, eye, at, held) in cameras {
        let players = posed_player(eye);
        let models = models(device, &geometry(&textures, &world, &entities, &players, eye, held, true));
        for (quality, shadows) in [(Quality::Balanced, false), (Quality::High, true)] {
            let looks = [Look::Before, Look::After];
            for look in looks {
                for _ in 0..10 {
                    rig.frame(look, quality, shadows, &world, &models, (eye, at), 0.70);
                    device.poll(wgpu::Maintain::Wait);
                }
            }
            let mut samples: Vec<Vec<f64>> = vec![Vec::new(); looks.len()];
            for _ in 0..rounds {
                for (i, look) in looks.iter().enumerate() {
                    for _ in 0..frames {
                        let started = std::time::Instant::now();
                        rig.frame(*look, quality, shadows, &world, &models, (eye, at), 0.70);
                        device.poll(wgpu::Maintain::Wait);
                        samples[i].push(started.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
            let medians: Vec<(f64, f64)> = samples
                .iter_mut()
                .map(|times| {
                    times.sort_by(f64::total_cmp);
                    (times[times.len() / 2], times[times.len() * 9 / 10])
                })
                .collect();
            println!(
                "{name} {quality:?}{}: before {:.3} ms (p90 {:.3}) | after {:.3} ms (p90 {:.3}) | {:+.3} ms",
                if shadows { "+shadows" } else { "" },
                medians[0].0,
                medians[0].1,
                medians[1].0,
                medians[1].1,
                medians[1].0 - medians[0].0
            );
        }
        // The geometry, on the CPU: what `lib.rs` rebuilds every frame, with
        // the models and without them.
        for carried in [false, true] {
            let runs = 2000;
            let started = std::time::Instant::now();
            let mut vertices = 0;
            for _ in 0..runs {
                let g = geometry(&textures, &world, &entities, &players, eye, held, carried);
                vertices = g.entity.0.len() + g.item.0.len() + g.actor.0.len() + g.hand.0.len();
            }
            println!(
                "{name} cpu, carried models {carried}: {:.1} us a frame for {vertices} vertices",
                started.elapsed().as_secs_f64() * 1e6 / f64::from(runs)
            );
        }
    }
}

/// **What somebody else looks like holding a thing, and dead.**
///
/// ```text
/// GPU_REPRO_DIR=<absolute dir> BODY_TAG=before|after \
///     cargo test -p primitive_client --release --lib \
///     what_a_player_holds_and_how_a_body_lies -- --ignored --nocapture
/// ```
///
/// Written for "игрок держит предметы странно и неправильно" and "при смерти
/// тело игрока всё ещё стоит, у трупа нет текстуры игрока". Four figures in the
/// morning sun, each holding one of the four shapes a hand has to hold -- an
/// axe, a spear, a stick and a piece of food -- photographed from the front
/// and from the side of the hand that holds it, at rest and in the middle of
/// what that thing is used for; and a player who has just died, from two
/// sides, over the body the death left.
///
/// Every figure is its own `RemotePlayers` fed real snapshots and stepped by
/// `tick`, so a gesture is where the frame loop would have it and not where a
/// test wrote a phase. Nothing is drawn in the first-person corner.
#[test]
#[ignore = "a tool: needs a GPU; photographs figures holding things, and a dead one, to GPU_REPRO_DIR"]
fn what_a_player_holds_and_how_a_body_lies() {
    use primitive_shared::protocol::{Action, Gesture, Outfit, PlayerState, Posture};
    use primitive_shared::types::{BLOCK_CORPSE, BLOCK_COOKED_MEAT, BLOCK_FLINT_SPEAR, BLOCK_LEATHER_TUNIC, BLOCK_STICK, BLOCK_STONE_AXE};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    let tag = std::env::var("BODY_TAG").unwrap_or_else(|_| "now".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    // The body the death left, in the column the player fell in -- where
    // `corpse_cell` always puts it. The figure itself is then not drawn (see
    // `build_actor_mesh_into`); before `Posture::Fallen` it stood over it.
    let dead_at = Vec3::new(8.5, GROUND, 1.5);
    let world = world_with(device, queue, &textures, &[(8, 4, 1, BLOCK_CORPSE)]);
    // The client's own record of where bodies lie and what they wear, as the
    // chunk and `ServerMessage::BodyWorn` would leave it: the tunic and the
    // boots are still in it.
    let mut bodies = ChunkManager::new(4);
    {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        blocks[Chunk::index(8, 4, 1)] = BLOCK_CORPSE;
        bodies.insert(Chunk { pos: ChunkPos::new(0, 0), blocks });
        let mut worn = [BLOCK_AIR; primitive_shared::equipment::SLOTS];
        worn[primitive_shared::equipment::Slot::Chest.index()] = BLOCK_LEATHER_TUNIC;
        worn[primitive_shared::equipment::Slot::Feet.index()] = primitive_shared::types::BLOCK_LEATHER_BOOTS;
        bodies.note_body_worn((8, 4, 1), worn);
    }
    let rig = Rig::new(device, queue, &textures, assets(), (960, 720), 4);
    let layers = textures.face_layers();
    let morning = 0.36f32;

    // One figure: snapshots as the server sends them, then `seconds` of frames.
    let figure = |at: Vec3, holding, action: Option<Action>, digging: bool, seconds: f32, posture: Posture| {
        let mut outfit = Outfit::BARE;
        outfit.holding = holding;
        outfit.worn[primitive_shared::equipment::Slot::Chest.index()] = BLOCK_LEATHER_TUNIC;
        let mut state = PlayerState {
            id: 7,
            x: f64::from(at.x),
            y: f64::from(at.y),
            z: f64::from(at.z),
            // Facing +X, so the right hand is on the +Z side.
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            outfit,
            posture,
            gesture: Gesture::default(),
            limp: 0,
            limp_left: false,
        };
        let mut players = RemotePlayers::default();
        players.set_tick_rate(20.0);
        players.apply_snapshot(1, &[state], None);
        if let Some(action) = action {
            state.gesture.made(action);
        }
        state.gesture.digging = digging;
        players.apply_snapshot(2, &[state], None);
        let frames = (seconds * 120.0).round() as usize;
        for _ in 0..frames {
            players.tick(1.0 / 120.0);
        }
        players
    };
    let shoot = |players: &RemotePlayers, name: &str, eye: Vec3, at: Vec3| {
        let mut g = Geometry::default();
        crate::net::remote_players::build_held_items_into(
            players,
            ORIGIN,
            &layers,
            &world.light,
            Some(&textures),
            &mut g.entity.0,
            &mut g.entity.1,
            &mut g.item.0,
            &mut g.item.1,
        );
        crate::net::remote_players::build_actor_mesh_into(players, Some(&bodies), ORIGIN, &world.light, &mut g.actor.0, &mut g.actor.1);
        let models = models(device, &g);
        rig.frame(Look::After, Quality::Balanced, true, &world, &models, (eye, at), morning);
        let shot = rig.read();
        shot.save(format!("{out}/{name}_{tag}.png")).expect("write png");
        shot
    };

    let mut sheet_shots = Vec::new();
    let things = [
        ("axe", BLOCK_STONE_AXE, Action::Strike, 0.09f32),
        ("spear", BLOCK_FLINT_SPEAR, Action::Strike, 0.42),
        ("stick", BLOCK_STICK, Action::Strike, 0.09),
        ("food", BLOCK_COOKED_MEAT, Action::Eat, 0.6),
    ];
    for (i, (name, block, action, into)) in things.into_iter().enumerate() {
        let feet = Vec3::new(5.5 + i as f32 * 4.0, GROUND, 11.5);
        let chest = feet + Vec3::new(0.0, 1.05, 0.0);
        let resting = figure(feet, block, None, false, 0.5, Posture::Standing);
        let front = shoot(&resting, &format!("hold_{name}_front"), chest + Vec3::new(2.4, 0.35, 0.25), chest);
        let side = shoot(&resting, &format!("hold_{name}_side"), chest + Vec3::new(0.2, 0.3, 2.4), chest);
        let busy = figure(feet, block, Some(action), false, into, Posture::Standing);
        let doing = shoot(&busy, &format!("use_{name}_side"), chest + Vec3::new(0.2, 0.3, 2.6), chest + Vec3::new(0.2, 0.2, 0.0));
        sheet_shots.push(front);
        sheet_shots.push(side);
        sheet_shots.push(doing);
    }
    // Digging with an axe, a few blows in.
    let feet = Vec3::new(5.5, GROUND, 11.5);
    let chest = feet + Vec3::new(0.0, 1.05, 0.0);
    for (k, seconds) in [0.52f32, 0.6].into_iter().enumerate() {
        let digger = figure(feet, BLOCK_STONE_AXE, None, true, seconds, Posture::Standing);
        shoot(&digger, &format!("dig_axe_{k}_side"), chest + Vec3::new(0.2, 0.3, 2.6), chest);
    }

    // Dead: the player's own snapshot, which is all anybody else is told.
    let dead = figure(dead_at, BLOCK_AIR, None, false, 0.5, Posture::Fallen);
    let body = dead_at + Vec3::new(0.5, 0.4, 0.0);
    let from_side = shoot(&dead, "dead_side", body + Vec3::new(0.3, 1.4, -3.0), body);
    let from_end = shoot(&dead, "dead_end", body + Vec3::new(3.0, 1.6, 0.4), body);
    sheet_shots.push(from_side);
    sheet_shots.push(from_end);

    let refs: Vec<&image::RgbaImage> = sheet_shots.iter().collect();
    sheet(&refs).save(format!("{out}/sheet_{tag}.png")).expect("write png");
    println!("pictures in {out}");
}

/// **The furniture and the four workshops, photographed.** Written for "сделай
/// модели и текстуры для мебели, а то сейчас это просто дерево" and for the
/// workshops that came with it: a row of every piece standing on the grass,
/// facing the camera, at noon and at golden hour, from the front and close.
///
/// ```text
/// GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/furniture \
///     cargo test -p primitive_client --lib what_the_furniture_and_the_workshops_look_like -- --ignored --nocapture
/// ```
///
/// **Before is a second run, not a second look**: the models are read once
/// per process (`models::load`), so `FURNITURE_ASSETS=<a directory holding
/// the old models/>` with `FURNITURE_LOOK=before` draws the pieces from
/// those files instead and names the pictures so. A piece the old directory
/// has no file for -- the chest and the workshops -- is drawn from this
/// build either way.
#[test]
#[ignore = "a tool: needs a GPU; photographs the furniture and the workshops to GPU_REPRO_DIR"]
fn what_the_furniture_and_the_workshops_look_like() {
    use primitive_shared::types::{
        faced, BLOCK_CHAIR, BLOCK_CHEST, BLOCK_LEATHER_BENCH, BLOCK_MASON_BLOCK, BLOCK_POTTERS_WHEEL, BLOCK_STOOL,
        BLOCK_STRAW_BED, BLOCK_WORKBENCH,
    };
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let look = std::env::var("FURNITURE_LOOK").unwrap_or_else(|_| "after".to_string());
    match std::env::var("FURNITURE_ASSETS") {
        Ok(dir) => crate::logic::models::load(std::path::Path::new(&dir)),
        Err(_) => crate::logic::models::load(assets()),
    }
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let south = |block| faced(block, Facing::South);
    let row = [
        (3, 10, south(BLOCK_STOOL)),
        (4, 10, south(BLOCK_CHAIR)),
        (5, 10, BLOCK_TABLE),
        (6, 10, south(BLOCK_CHEST)),
        (7, 9, bed_half(Facing::South, true)),
        (7, 10, bed_half(Facing::South, false)),
        (8, 9, primitive_shared::types::bed_half_of(BLOCK_STRAW_BED, Facing::South, true)),
        (8, 10, primitive_shared::types::bed_half_of(BLOCK_STRAW_BED, Facing::South, false)),
        (10, 10, south(BLOCK_WORKBENCH)),
        (11, 10, south(BLOCK_MASON_BLOCK)),
        (12, 10, south(BLOCK_POTTERS_WHEEL)),
        (13, 10, south(BLOCK_LEATHER_BENCH)),
        // The drying rack, bare and with a skin on, for "сушилка как на
        // фото": two crossed A-frames and a pole laid across them.
        (14, 10, south(primitive_shared::types::BLOCK_DRYING_RACK)),
        (15, 10, primitive_shared::types::rack_with_hide(south(primitive_shared::types::BLOCK_DRYING_RACK), true)),
    ];
    let extra: Vec<(usize, usize, usize, BlockId)> = row.iter().map(|&(x, z, block)| (x, 4, z, block)).collect();
    let world = world_with(device, queue, &textures, &extra);
    let rig = Rig::new(device, queue, &textures, assets(), (1280, 720), 4);
    let entities = Entities::default();
    let players = RemotePlayers::default();
    let views = [
        ("row", Vec3::new(8.5, 6.2, 15.5), Vec3::new(8.5, 4.4, 9.5)),
        ("furniture", Vec3::new(5.4, 5.7, 13.2), Vec3::new(5.6, 4.4, 10.2)),
        ("workshops", Vec3::new(11.8, 5.8, 13.4), Vec3::new(11.6, 4.5, 10.3)),
        ("from_above", Vec3::new(9.0, 8.5, 13.5), Vec3::new(9.0, 4.3, 9.8)),
        ("chest", Vec3::new(6.2, 5.6, 12.0), Vec3::new(6.5, 4.4, 10.5)),
        ("chest_back", Vec3::new(7.4, 5.8, 8.3), Vec3::new(6.5, 4.4, 10.5)),
        ("racks", Vec3::new(14.2, 5.9, 13.6), Vec3::new(15.0, 4.7, 10.5)),
        ("racks_side", Vec3::new(17.6, 5.7, 12.6), Vec3::new(14.8, 4.6, 10.4)),
        ("racks_end", Vec3::new(18.2, 5.2, 10.5), Vec3::new(14.5, 4.6, 10.5)),
        // From above and east of the workshops, where a low western sun lays
        // their shadows: "верстак и прочие не отбрасывают тень".
        ("shadows_above", Vec3::new(13.0, 9.5, 13.5), Vec3::new(11.5, 4.0, 10.0)),
    ];
    let mut shots = Vec::new();
    for (view, eye, at) in views {
        let models = models(device, &geometry(&textures, &world, &entities, &players, eye, BLOCK_WORKBENCH, true));
        for (when, t) in [("noon", 0.55f32), ("golden", 0.70)] {
            rig.frame(Look::After, Quality::Balanced, true, &world, &models, (eye, at), t);
            let shot = rig.read();
            shot.save(format!("{out}/{view}_{when}_{look}.png")).expect("write png");
            // The same frame with the sun's map off, and where the two differ
            // painted red: a model that casts shows as a red shape on the
            // grass beside it, one that does not shows nothing there.
            if view == "shadows_above" {
                rig.frame(Look::After, Quality::Balanced, false, &world, &models, (eye, at), t);
                let unshadowed = rig.read();
                let mut diff = shot.clone();
                for (lit, (dark, into)) in unshadowed.pixels().zip(shot.pixels().zip(diff.pixels_mut())) {
                    let drop: i32 = (0..3).map(|c| i32::from(lit[c]) - i32::from(dark[c])).sum();
                    if drop > 24 {
                        *into = image::Rgba([230, 30, 30, 255]);
                    }
                }
                diff.save(format!("{out}/{view}_{when}_{look}_cast.png")).expect("write png");
            }
            if when == "noon" {
                shots.push(shot);
            }
        }
    }
    let refs: Vec<&image::RgbaImage> = shots.iter().collect();
    sheet(&refs).save(format!("{out}/sheet_{look}.png")).expect("write png");
    println!("pictures in {out}");
}

/// **Things set down by hand**, photographed: a row laid on the grass by a
/// player looking north and a row by one looking east, and one thing on top
/// of a block. The report: "добавь возможность ставить любые небольшие
/// предметы на землю, зажав Shift".
///
/// ```text
/// GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/set-down \
///     cargo test -p primitive_client --lib what_is_set_down_by_hand -- --ignored --nocapture
/// ```
///
/// Built the way the frame builds it: the cells in a `ChunkManager`, what
/// lies in them told to it as `ServerMessage::SetDownItem` would, and drawn by
/// `entities::build_set_down_into`.
#[test]
#[ignore = "a tool: needs a GPU; photographs things set down on the ground to GPU_REPRO_DIR"]
fn what_is_set_down_by_hand() {
    use primitive_shared::types::{
        faced, BLOCK_BONE, BLOCK_BREAD, BLOCK_COPPER_INGOT, BLOCK_COPPER_KNIFE, BLOCK_HIDE, BLOCK_RAW_MEAT,
        BLOCK_SET_DOWN, BLOCK_STONE_AXE,
    };
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let things = [BLOCK_COPPER_KNIFE, BLOCK_STONE_AXE, BLOCK_BREAD, BLOCK_BONE, BLOCK_COPPER_INGOT, BLOCK_RAW_MEAT, BLOCK_HIDE];
    // Two rows, by a player looking north (-z) and one looking east (+x), and
    // a knife on a block of stone at the end of the first.
    let mut laid: Vec<(usize, usize, usize, BlockId, BlockId)> = Vec::new();
    for (i, &thing) in things.iter().enumerate() {
        laid.push((6 + i, 4, 2, faced(BLOCK_SET_DOWN, Facing::toward_viewer(-std::f32::consts::FRAC_PI_2)), thing));
        laid.push((6 + i, 4, 4, faced(BLOCK_SET_DOWN, Facing::toward_viewer(0.0)), thing));
    }
    laid.push((14, 5, 2, faced(BLOCK_SET_DOWN, Facing::toward_viewer(0.0)), BLOCK_COPPER_KNIFE));
    // ...and a knife on each partial top it lies on (`types::set_down_drop`),
    // for "через шифт можно ставить только на полные блоки": a turf lip a
    // quarter down, a slab, and the tread of a stair climbing west.
    let partial = [
        primitive_shared::dig::lowered(primitive_shared::types::BLOCK_GRASS, 1),
        primitive_shared::types::BLOCK_TILE_SLAB,
        faced(primitive_shared::types::BLOCK_PLANK_STAIRS, Facing::West),
    ];
    for (i, _) in partial.iter().enumerate() {
        laid.push((7 + 2 * i, 5, 7, faced(BLOCK_SET_DOWN, Facing::toward_viewer(-std::f32::consts::FRAC_PI_2)), BLOCK_COPPER_KNIFE));
    }
    let mut extra: Vec<(usize, usize, usize, BlockId)> = laid.iter().map(|&(x, y, z, cell, _)| (x, y, z, cell)).collect();
    extra.push((14, 4, 2, BLOCK_STONE));
    for (i, &ground) in partial.iter().enumerate() {
        extra.push((7 + 2 * i, 4, 7, ground));
    }
    let world = world_with(device, queue, &textures, &extra);
    let mut chunks = ChunkManager::new(4);
    {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for &(x, y, z, block) in &extra {
            blocks[Chunk::index(x, y, z)] = block;
        }
        chunks.insert(Chunk { pos: ChunkPos::new(0, 0), blocks });
        for &(x, y, z, _, thing) in &laid {
            chunks.note_set_down((x as i32, y as i32, z as i32), thing);
        }
    }
    let layers = textures.face_layers();
    let mut g = Geometry::default();
    crate::logic::entities::build_set_down_into(
        chunks.set_down_laid(),
        ORIGIN,
        &layers,
        &world.light,
        Some(&textures),
        &mut g.entity.0,
        &mut g.entity.1,
        &mut g.item.0,
        &mut g.item.1,
    );
    let models = models(device, &g);
    let rig = Rig::new(device, queue, &textures, assets(), (1280, 720), 4);
    let mut shots = Vec::new();
    for (name, eye, at) in [
        ("above", Vec3::new(9.5, 7.4, 7.2), Vec3::new(9.5, 4.0, 3.0)),
        ("low", Vec3::new(9.5, 5.0, 7.5), Vec3::new(9.5, 4.1, 3.0)),
        ("partial", Vec3::new(9.5, 6.3, 10.0), Vec3::new(9.5, 4.6, 7.0)),
        ("partial_side", Vec3::new(5.0, 5.4, 9.5), Vec3::new(9.5, 4.6, 7.0)),
    ] {
        rig.frame(Look::After, Quality::Balanced, true, &world, &models, (eye, at), 0.4);
        let shot = rig.read();
        shot.save(format!("{out}/set_down_{name}.png")).expect("write png");
        shots.push(shot);
    }
    let refs: Vec<&image::RgbaImage> = shots.iter().collect();
    sheet(&refs).save(format!("{out}/set_down_sheet.png")).expect("write png");
    println!("pictures in {out}");
}

/// **A chest opened and shut, frame by frame**, the way the frame loop hands
/// the lid between the chunk mesh and its own geometry -- for "сундук мерцает
/// при анимации".
///
/// ```text
/// GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/bits \
///     cargo test -p primitive_client --lib how_a_chest_lid_changes_hands_frame_by_frame -- --ignored --nocapture
/// ```
///
/// The chunk mesh on screen is always one of two meshes -- lid in, lid out --
/// and a mesh asked for lands `MESH_LATE` frames later, which is what a
/// worker thread and `streaming_budget` make of an urgent chunk. Two
/// filmstrips: `before`, the rule the frame used to keep (the lid taken the
/// moment the server says open and given back the moment it comes to rest,
/// whatever the mesh on screen is doing), and `after`, `ChunkManager`'s own
/// handover (`note_meshed_lids`). Each frame is the chest, cropped and
/// enlarged; a frame where both draw the lid, or neither does, is marked with
/// a red bar under it.
#[test]
#[ignore = "a tool: needs a GPU; photographs a chest lid opening and shutting frame by frame to GPU_REPRO_DIR"]
fn how_a_chest_lid_changes_hands_frame_by_frame() {
    use primitive_shared::types::{faced, BLOCK_CHEST};
    const MESH_LATE: usize = 3;
    const AT: (i32, i32, i32) = (6, 4, 10);
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    crate::logic::models::load(assets());
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let chest = faced(BLOCK_CHEST, Facing::South);
    let extra = [(AT.0 as usize, AT.1 as usize, AT.2 as usize, chest)];
    let lid_in = world_leaving_out(device, queue, &textures, &extra, &[]);
    let lid_out = world_leaving_out(device, queue, &textures, &extra, &[AT]);
    let rig = Rig::new(device, queue, &textures, assets(), (640, 360), 4);
    let layers = textures.face_layers();
    let camera = (Vec3::new(6.5, 6.4, 13.2), Vec3::new(6.5, 4.6, 10.5));
    let frame = 1.0 / 60.0;
    let (open_at, shut_at, frames) = (4usize, 34usize, 64usize);

    // The frame's side of it: which lid it draws, at what angle, for either
    // rule. `None` is no lid drawn by the frame.
    let mut after = ChunkManager::new(4);
    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
    blocks[Chunk::index(AT.0 as usize, AT.1 as usize, AT.2 as usize)] = chest;
    after.insert(Chunk { pos: ChunkPos::new(0, 0), blocks });
    for (name, handover) in [("before", false), ("after", true)] {
        let mut on_screen_out = false;
        let mut in_flight: Vec<(usize, bool)> = Vec::new();
        let mut old_swing: Option<f32> = None;
        let mut old_open = false;
        let mut strip: Vec<(image::RgbaImage, bool)> = Vec::new();
        for step in 0..frames {
            let told = if step == open_at { Some(true) } else if step == shut_at { Some(false) } else { None };
            // What the frame draws, and which meshes are asked for.
            if handover {
                if let Some(open) = told {
                    if after.note_chest_lid(AT, open) {
                        in_flight.push((MESH_LATE, !after.lids_in(ChunkPos::new(0, 0)).is_empty()));
                    }
                }
            } else if let Some(open) = told {
                old_open = open;
                if open && old_swing.is_none() {
                    old_swing = Some(0.0);
                    in_flight.push((MESH_LATE, true));
                }
            }
            for &(due, out) in &in_flight {
                if due == 0 {
                    on_screen_out = out;
                    if handover {
                        after.note_meshed_lids(ChunkPos::new(0, 0), if out { &[AT] } else { &[] });
                    }
                }
            }
            in_flight.retain(|&(due, _)| due > 0);
            for (due, _) in in_flight.iter_mut() {
                *due -= 1;
            }
            let drawn: Option<f32> = if handover {
                if !after.advance_lids(frame).is_empty() {
                    in_flight.push((MESH_LATE, false));
                }
                after.open_lids().next().map(|(_, _, swing)| swing)
            } else {
                // The old rule: the swing moved from the message on, and
                // the lid was let go of the frame it came to rest.
                if let Some(swing) = old_swing.as_mut() {
                    let step = frame * crate::engine::mesh::LID_OPEN / crate::engine::mesh::LID_SWING_SECONDS;
                    *swing = if old_open {
                        (*swing + step).min(crate::engine::mesh::LID_OPEN)
                    } else {
                        (*swing - step).max(0.0)
                    };
                    if !old_open && *swing <= 0.0 {
                        old_swing = None;
                        in_flight.push((MESH_LATE, false));
                    }
                }
                old_swing
            };
            let mut g = Geometry::default();
            if let Some(angle) = drawn {
                let corner = Vec3::new(AT.0 as f32, AT.1 as f32, AT.2 as f32) - ORIGIN;
                crate::engine::mesh::chest_lid_block(
                    [corner.x, corner.y, corner.z],
                    chest,
                    angle,
                    &layers,
                    15,
                    &mut g.entity.0,
                    &mut g.entity.1,
                );
            }
            let models = models(device, &g);
            let world = if on_screen_out { &lid_out } else { &lid_in };
            rig.frame(Look::After, Quality::Balanced, false, world, &models, camera, 0.55);
            let shot = rig.read();
            let crop = image::imageops::crop_imm(&shot, 200, 90, 240, 200).to_image();
            let lids = usize::from(!on_screen_out) + usize::from(drawn.is_some());
            strip.push((crop, lids != 1));
        }
        let (w, h) = (strip[0].0.width(), strip[0].0.height());
        let columns = 16u32;
        let rows = (strip.len() as u32).div_ceil(columns);
        let mut sheet = image::RgbaImage::from_pixel(w * columns, (h + 6) * rows, image::Rgba([20, 20, 24, 255]));
        let mut wrong = 0;
        for (i, (crop, bad)) in strip.iter().enumerate() {
            let (x, y) = ((i as u32 % columns) * w, (i as u32 / columns) * (h + 6));
            image::imageops::replace(&mut sheet, crop, i64::from(x), i64::from(y));
            if *bad {
                wrong += 1;
                for dx in 0..w {
                    for dy in h..h + 5 {
                        sheet.put_pixel(x + dx, y + dy, image::Rgba([230, 30, 30, 255]));
                    }
                }
            }
        }
        sheet.save(format!("{out}/chest_lid_{name}.png")).expect("write png");
        println!("{name}: {wrong} of {frames} frames drew the lid twice or not at all");
    }
    println!("pictures in {out}");
}

/// **The chest open and shut, the stake standing and driven in, and a rod in
/// the hand**, from the same places every run -- for the reports "проблема с
/// текстурой и у сундука", "сундук внутри не полый", "у кола нету модели" and
/// "удочка повёрнута не так как надо".
///
/// ```text
/// GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/shots/chest2 CHEST2_TAG=before \
///     cargo test -p primitive_client --lib what_the_chest_the_stake_and_the_rod_look_like -- --ignored --nocapture
/// ```
///
/// The open chest is drawn the way the frame draws it: the chunk meshed with
/// its lid left out (`world_leaving_out`) and the lid standing at `LID_OPEN`
/// (`mesh::chest_lid_block`).
#[test]
#[ignore = "a tool: needs a GPU; photographs the chest, the stake and the rod to GPU_REPRO_DIR"]
fn what_the_chest_the_stake_and_the_rod_look_like() {
    use primitive_shared::protocol::{Gesture, Outfit, PlayerState, Posture};
    use primitive_shared::types::{faced, BLOCK_CHEST, BLOCK_FISHING_ROD, BLOCK_STAKE, STAKE_UPRIGHT};
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    let tag = std::env::var("CHEST2_TAG").unwrap_or_else(|_| "now".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    // `CHEST2_ASSETS=<a directory holding the old models/>` draws the chest
    // from those files: the before half of a before and after.
    match std::env::var("CHEST2_ASSETS") {
        Ok(dir) => crate::logic::models::load(std::path::Path::new(&dir)),
        Err(_) => crate::logic::models::load(assets()),
    }
    let textures = TextureManager::load(device, queue, assets(), 16).expect("textures load");
    let layers = textures.face_layers();
    let shut = (6usize, 4usize, 10usize);
    let open = (8usize, 4usize, 10usize);
    let chest = faced(BLOCK_CHEST, Facing::South);
    let extra = [
        (shut.0, shut.1, shut.2, chest),
        (open.0, open.1, open.2, chest),
        // Stakes: two standing, and two driven into a wall of cobbles on
        // their north.
        (11, 4, 10, BLOCK_STAKE | STAKE_UPRIGHT),
        (12, 4, 11, BLOCK_STAKE | STAKE_UPRIGHT),
        (13, 4, 9, BLOCK_COBBLESTONE),
        (13, 5, 9, BLOCK_COBBLESTONE),
        (14, 4, 9, BLOCK_COBBLESTONE),
        (14, 5, 9, BLOCK_COBBLESTONE),
        (13, 5, 10, faced(BLOCK_STAKE, Facing::North)),
        (14, 4, 10, faced(BLOCK_STAKE, Facing::North)),
        // ...and one driven into a wall on its west, which faces east.
        (12, 5, 13, BLOCK_COBBLESTONE),
        (13, 5, 13, faced(BLOCK_STAKE, Facing::East)),
        // A chest in each of four other woods, which wear their own boards
        // (`texture::WoodPiece::ChestSide`).
        (3, 4, 2, primitive_shared::types::in_wood(chest, 1)),
        (4, 4, 2, primitive_shared::types::in_wood(chest, 2)),
        (5, 4, 2, primitive_shared::types::in_wood(chest, 3)),
        (6, 4, 2, primitive_shared::types::in_wood(chest, 4)),
    ];
    let lid = [(open.0 as i32, open.1 as i32, open.2 as i32)];
    let world = world_leaving_out(device, queue, &textures, &extra, &lid);
    let rig = Rig::new(device, queue, &textures, assets(), (960, 720), 4);

    // Somebody holding a rod, facing +X.
    let feet = Vec3::new(4.5, GROUND, 14.5);
    let mut outfit = Outfit::BARE;
    outfit.holding = BLOCK_FISHING_ROD;
    let state = PlayerState {
        id: 7,
        x: f64::from(feet.x),
        y: f64::from(feet.y),
        z: f64::from(feet.z),
        yaw: 0.0,
        pitch: 0.0,
        on_ground: true,
        outfit,
        posture: Posture::Standing,
        gesture: Gesture::default(),
        limp: 0,
        limp_left: false,
    };
    let mut players = RemotePlayers::default();
    players.set_tick_rate(20.0);
    players.apply_snapshot(1, &[state], None);
    players.apply_snapshot(2, &[state], None);
    for _ in 0..60 {
        players.tick(1.0 / 120.0);
    }

    let entities = Entities::default();
    let chest_at = |cell: (usize, usize, usize)| Vec3::new(cell.0 as f32 + 0.5, cell.1 as f32 + 0.45, cell.2 as f32 + 0.5);
    let views = [
        ("chests_front", Vec3::new(7.0, 6.1, 13.2), (chest_at(shut) + chest_at(open)) * 0.5),
        ("chest_open_above", Vec3::new(8.5, 6.4, 12.0), chest_at(open)),
        ("chest_open_side", Vec3::new(10.6, 5.6, 11.4), chest_at(open)),
        ("chest_shut_close", Vec3::new(6.9, 5.2, 11.9), chest_at(shut)),
        ("stakes", Vec3::new(12.6, 5.6, 13.4), Vec3::new(12.6, 4.5, 10.3)),
        ("stakes_side", Vec3::new(16.2, 5.3, 11.6), Vec3::new(13.0, 4.6, 10.3)),
        ("chest_woods", Vec3::new(5.0, 5.7, 5.0), Vec3::new(5.0, 4.4, 2.5)),
        ("rod_front", feet + Vec3::new(2.6, 1.4, 0.3), feet + Vec3::new(0.3, 1.2, 0.0)),
        ("rod_side", feet + Vec3::new(0.3, 1.4, 2.8), feet + Vec3::new(0.3, 1.3, 0.0)),
    ];
    let mut shots = Vec::new();
    for (name, eye, at) in views {
        let mut g = geometry(&textures, &world, &entities, &players, eye, BLOCK_FISHING_ROD, true);
        let corner = Vec3::new(open.0 as f32, open.1 as f32, open.2 as f32) - ORIGIN;
        crate::engine::mesh::chest_lid_block(
            [corner.x, corner.y, corner.z],
            chest,
            crate::engine::mesh::LID_OPEN,
            &layers,
            15,
            &mut g.entity.0,
            &mut g.entity.1,
        );
        let models = models(device, &g);
        rig.frame(Look::After, Quality::Balanced, true, &world, &models, (eye, at), 0.4);
        let shot = rig.read();
        shot.save(format!("{out}/{name}_{tag}.png")).expect("write png");
        if name == "rod_front" {
            hand_corner(&shot).save(format!("{out}/rod_hand_{tag}.png")).expect("write png");
        }
        shots.push(shot);
    }
    let refs: Vec<&image::RgbaImage> = shots.iter().collect();
    sheet(&refs).save(format!("{out}/sheet_{tag}.png")).expect("write png");
    println!("pictures in {out}");
}
