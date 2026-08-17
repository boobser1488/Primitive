//! wgpu renderer.
//!
//! Three pipelines share one `Globals` uniform buffer:
//! - **chunk**: textured terrain with baked light + AO + fog;
//! - **actor**: flat-coloured remote-player boxes, lit by the same sun
//!   and fogged by the same curve;
//! - **overlay**: screen-space crosshair, depth test off.
//!
//! Sharing the uniform is deliberate. When the sun and fog live in one
//! buffer updated once per frame, it's structurally impossible for the
//! terrain and the players to disagree about what time of day it is --
//! the class of bug where one thing gets fogged and another doesn't
//! simply can't be expressed.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bytemuck::Pod;
use glam::Vec3;
use winit::window::Window;

use primitive_shared::types::ChunkPos;

use crate::engine::camera::Camera;
use crate::engine::gpu_timing::GpuTiming;
use crate::engine::frustum::Frustum;
use crate::ui::hotbar::{HotbarVertex, MAX_HOTBAR_VERTICES};
use crate::engine::mesh::Vertex;
use crate::net::remote_players::ActorVertex;
use crate::engine::texture::TextureManager;

/// Geometry that is rewritten every frame into buffers that persist.
///
/// See `GraphicsState::write_dynamic_mesh` for why this exists rather
/// than a fresh `GpuMesh` per frame.
pub struct DynamicMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// Bytes the buffers can hold, which is at least what they were last
    /// asked to hold.
    vertex_capacity: usize,
    index_capacity: usize,
    index_count: u32,
}

impl DynamicMesh {
    /// Nothing to draw this frame.
    pub fn is_empty(&self) -> bool {
        self.index_count == 0
    }
}

/// One chunk's geometry, as a pair of ranges inside the shared arena.
///
/// It owns no buffers. Freeing it means handing its two blocks back to
/// the arena, which is why the map of these lives in `GraphicsState`
/// rather than in the frame loop: a `GpuMesh` that is merely dropped
/// leaks its space, and the way to make that unwriteable is to give the
/// only code that can drop one the arena to return it to.
pub struct GpuMesh {
    vertex_block: crate::engine::arena::Block,
    index_block: crate::engine::arena::Block,
    pub num_indices: u32,
    /// Where the four ranges end, relative to the start of this chunk's
    /// indices: solid, then leaves, then sprites, then blended. See
    /// `mesh::MeshBuffers` for why they are split, and `render` for what
    /// the renderer does to leaves that it must not do to sprites.
    pub solid_indices: u32,
    pub leaf_end: u32,
    pub sprite_end: u32,
}

impl GpuMesh {
    /// The chunk's index range inside the shared buffer, for one pass.
    #[inline]
    fn indices(&self, from: u32, to: u32) -> std::ops::Range<u32> {
        let base = self.index_block.start as u32;
        (base + from)..(base + to)
    }

    /// What `draw_indexed` should add to every index it reads.
    #[inline]
    fn base_vertex(&self) -> i32 {
        self.vertex_block.start as i32
    }
}

/// Everything the shaders need that changes per frame. Grouped into one
/// struct so `main.rs` computes it in one place and the renderer stays
/// free of game logic.
#[derive(Debug, Clone, Copy)]
pub struct FrameParams {
    pub sun_direction: Vec3,
    pub sun_intensity: f32,
    /// Also the clear colour. Terrain fades into exactly what is already
    /// behind it, which is what makes the render-distance edge dissolve
    /// instead of showing as a wall.
    pub fog_color: Vec3,
    pub fog_start: f32,
    pub fog_end: f32,
    /// How far the world is actually drawn, in blocks.
    ///
    /// **Not the same number as `fog_end`, and the difference is a bug
    /// that was visible.** Everything that thins out with distance --
    /// which leaves are drawn as cutouts, how far the grass reaches --
    /// used to be measured against the fog, on the reasonable ground
    /// that nothing past the fog can be seen anyway. Then the fog is
    /// switched off (F), the terrain draws to the edge of what is
    /// loaded, and the grass and the leaf detail still stop where the
    /// fog used to end: a ring of bare, solid-canopied ground around
    /// the player with a full world beyond it.
    ///
    /// So the fog is what fades, and this is what is *there*.
    pub view_distance: f32,
    pub ambient: f32,
    pub block_light_boost: f32,
    pub ao_strength: f32,
    pub fog_enabled: bool,
    /// How far a chunk may be before none of it can change a pixel, or
    /// `None` where there is no such distance. Worked out by
    /// `engine::fog`, which owns the reasoning; the renderer only culls
    /// against it.
    pub fog_cull_distance: Option<f32>,
    pub underwater: bool,
    /// Whether a canopy is see-through. See
    /// `settings::ClientSettings::transparent_leaves` for what it costs
    /// either way.
    pub transparent_leaves: bool,
    /// How far out tufts of grass and loose stones are drawn, as a
    /// fraction of the fog distance. See
    /// `settings::ClientSettings::detail_distance`.
    pub detail_distance: f32,
    /// Where the world clock stands, 0 = midnight. The sky pass places
    /// the sun, the moon and the stars from it, and drifts the cloud.
    pub time_of_day: f32,
    /// How much of the sky the cloud layer covers, 0..1.
    pub cloudiness: f32,
    /// How badly hurt the player is: 0 while they are fine, 1 when they
    /// are about to die. Drives the vignette, and nothing else.
    ///
    /// A *derived* number rather than the health itself, because the
    /// renderer has no business knowing what a hit point is -- and
    /// because where the darkness starts is a design decision that
    /// belongs next to the health bar, not next to the swapchain.
    pub hurt: f32,
    /// Seconds since the client started, and the *only* thing the cloud
    /// layer drifts on.
    ///
    /// It used to drift on `time_of_day`, which is a number that wraps:
    /// every midnight the whole sky jumped back to the pattern it had at
    /// the previous one. A clock that only ever counts up cannot do that
    /// -- and because clouds are the one part of the sky nobody expects
    /// to be synchronised between players, it costs nothing to have it
    /// be a local one.
    pub elapsed_seconds: f32,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self {
            sun_direction: Vec3::new(0.0, -1.0, 0.0),
            sun_intensity: 1.0,
            fog_color: Vec3::new(0.53, 0.80, 0.92),
            fog_start: 40.0,
            fog_end: 120.0,
            view_distance: 128.0,
            ambient: 0.06,
            block_light_boost: 1.0,
            ao_strength: 0.45,
            fog_enabled: true,
            fog_cull_distance: Some(120.0),
            underwater: false,
            transparent_leaves: true,
            detail_distance: 0.7,
            time_of_day: 0.5,
            cloudiness: 0.45,
            hurt: 0.0,
            elapsed_seconds: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    sun: [f32; 4],
    fog_color: [f32; 4],
    fog_params: [f32; 4],
    extra: [f32; 4],
    /// x: texture resolution in texels. The terrain shader needs it to
    /// snap UVs to texel centres -- see `crisp_uv` in shader.wgsl.
    texture_params: [f32; 4],
    /// Turns a clip-space point back into a world-space direction. The
    /// sky pass rests on it entirely: it draws one triangle over the
    /// screen and has to ask, per pixel, which way that pixel looks.
    inv_view_proj: [[f32; 4]; 4],
    /// x: time of day 0..1, y: how cloudy, z/w spare.
    sky_params: [f32; 4],
    /// View space straight to clip space, for the first-person hand --
    /// which is the one thing in the frame that is not in the world and
    /// so has no business going through `view_proj`. See `hand.wgsl`.
    hand_view_proj: [[f32; 4]; 4],
}

pub struct GraphicsState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,

    chunk_pipeline: wgpu::RenderPipeline,
    /// Leaves: the only geometry whose shader may `discard`.
    cutout_pipeline: wgpu::RenderPipeline,
    /// Dropped items, which are sprites with a thickness rather than
    /// cubes. Same shading as the terrain, different vertex format --
    /// see `engine::item_model`.
    item_pipeline: wgpu::RenderPipeline,
    /// The sky: one triangle, no vertex buffer, everything worked out
    /// per fragment. See `sky.wgsl`.
    sky_pipeline: wgpu::RenderPipeline,
    /// The cloud field, bound at group 1 by both sky pipelines.
    cloud_bind_group: wgpu::BindGroup,
    /// Same shader and vertex layout as `chunk_pipeline`, but blended and
    /// with depth writes off -- the second pass, for water.
    transparent_pipeline: wgpu::RenderPipeline,
    crack_pipeline: wgpu::RenderPipeline,
    actor_pipeline: wgpu::RenderPipeline,
    /// The player's own arm and what is in it. Its own pipeline because
    /// its vertices are in view space rather than in the world; see
    /// `hand.wgsl` and `HAND_DEPTH_SLICE`.
    hand_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_buffer: wgpu::Buffer,
    hotbar_pipeline: wgpu::RenderPipeline,
    hotbar_buffer: wgpu::Buffer,
    /// Vertices the hotbar/UI buffer can currently hold. The buffer
    /// grows when a bigger screen needs it -- the hotbar is a few
    /// hundred vertices, but the server browser draws text as one quad
    /// per lit font pixel and runs to tens of thousands.
    hotbar_capacity: usize,
    /// How many of those vertices the last upload actually filled, which
    /// is what the draw uses. Not the length of the slice `render` was
    /// handed: on the frames where the caller says the interface has not
    /// changed, the upload is skipped, and the buffer's contents -- and
    /// this count -- are still those of the last rebuild.
    hotbar_vertex_count: u32,
    depth_view: wgpu::TextureView,

    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
    /// The same texture with the always-nearest sampler, for the flat
    /// UI. See `texture::build_ui_sampler`.
    ui_texture_bind_group: wgpu::BindGroup,
    /// Kept so the bind group can be rebuilt when the sampler changes.
    /// Anisotropy is a sampler property, and a sampler cannot be edited
    /// in place -- changing the setting means a new sampler and a new
    /// bind group pointing at it.
    texture_bind_group_layout: wgpu::BindGroupLayout,
    anisotropy: u16,
    /// What the surface said it could do, kept so the vsync toggle can
    /// pick from the same list the first choice was made from.
    present_modes: Vec<wgpu::PresentMode>,

    /// How long the GPU spent on the render pass. `None` where the
    /// driver has no timestamps. See `engine::gpu_timing`.
    gpu_timing: Option<crate::engine::gpu_timing::GpuTiming>,

    // --- the small sky. See `sky_blit.wgsl`. ---
    /// 1 draws the sky straight into the frame, as it always did.
    /// Anything more divides its width and height by that much.
    sky_scale: u32,
    /// The sky pipeline again, targeting the small texture: same
    /// shaders, no depth attachment, because there is no terrain in
    /// that pass to test against.
    sky_offscreen_pipeline: wgpu::RenderPipeline,
    /// Stretches the small texture over the frame, depth-tested exactly
    /// as the sky draw it stands in for.
    sky_blit_pipeline: wgpu::RenderPipeline,
    sky_blit_layout: wgpu::BindGroupLayout,
    sky_blit_sampler: wgpu::Sampler,
    /// `None` at full scale, and rebuilt on every resize.
    sky_target: Option<(wgpu::TextureView, wgpu::BindGroup)>,

    /// The overlay's geometry, rebuilt every frame into a buffer that
    /// persists. Two quads most frames; a `Vec` returned by value was a
    /// heap allocation per frame for sixty-four vertices.
    overlay_scratch: Vec<OverlayVertex>,
    /// The `(loading, aspect, hurt)` the overlay buffer currently holds,
    /// as bits so it can be compared exactly. In the steady state --
    /// no loading screen, no damage -- the crosshair is identical every
    /// frame, and rebuilding plus re-uploading it was pure waste on
    /// 100% of frames.
    overlay_key: Option<(Option<u32>, u32, u32)>,

    /// The terrain, in two shared buffers. See `engine::arena`.
    arena: crate::engine::arena::Arena,
    /// Every chunk that has geometry on the GPU.
    ///
    /// Owned here rather than by the frame loop because each entry holds
    /// space in the arena that has to be given back. With the map
    /// private, the only ways to remove one are `drop_chunk_mesh` and
    /// `clear_chunk_meshes`, and both return the space -- there is no
    /// spelling of "forget a chunk" that leaks.
    chunk_meshes: HashMap<ChunkPos, GpuMesh>,

    pub textures: TextureManager,
    pub draw_calls_last_frame: u32,
    pub chunks_culled_last_frame: usize,
    /// How long the last frame spent recording draws, and how long it
    /// then spent handing them over. Split because they answer different
    /// questions: the first is work this code does, the second is mostly
    /// waiting for the GPU, and a frame that is short on the first and
    /// long on the second is one that no amount of CPU work will speed
    /// up. Read by the F3 panel.
    pub encode_time_last_frame: std::time::Duration,
    pub present_time_last_frame: std::time::Duration,
    /// How long the frame waited for a swapchain image to draw into.
    ///
    /// Kept apart from the recording it sits in the middle of, because
    /// the two mean opposite things. Recording is work; this is the
    /// frame rate being handed back by the GPU -- when the card is
    /// behind, `get_current_texture` is where the CPU stands and waits,
    /// and counting that as "encoding" makes a GPU-bound frame look like
    /// a CPU-bound one.
    pub acquire_time_last_frame: std::time::Duration,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The slice of the depth buffer the first-person hand is drawn into.
///
/// **This is the answer to "why does the hand not sink into the wall".**
/// A view model sits ten to fifty centimetres from the eye, which is
/// nearer than most of what the player can walk up to -- so drawn with
/// the world's own depth values it is sliced in half by every doorway
/// and buried by every block the player stands against.
///
/// Three ways out were considered.
///
///   * *Depth test off.* One line, and wrong: with no test among its own
///     fragments the arm shows through the item it is holding, and the
///     back of a swung tool draws over its front.
///   * *A second render pass with the depth buffer cleared.* The usual
///     answer, and the one this would have used if the frame were not
///     already one pass with the GPU's own timestamps written at its
///     boundaries. Splitting it would cost the frame its measurement and
///     an extra load/store of a full-screen depth attachment, to buy
///     nothing this does not.
///   * *This.* The viewport's depth range is narrowed to the first few
///     percent of the buffer for the hand's draw and put back
///     afterwards. Every hand fragment therefore lands nearer than
///     anything the world can have written -- the world's near plane is
///     five centimetres, and a wall would have to be within about five
///     and a quarter to reach this far in -- while inside the slice the
///     hand tests and writes against itself exactly as normal geometry
///     does. One extra `set_viewport` per frame, no extra pass, and the
///     self-occlusion comes out right for free.
///
/// Five percent rather than one: the slice has to hold the hand's whole
/// depth range with enough resolution left that the item does not z-fight
/// the arm. A 32-bit depth buffer has plenty to spare either way.
const HAND_DEPTH_SLICE: f32 = 0.05;

/// Vertical field of view for the hand's own projection, in radians.
///
/// Narrower than the world's, and fixed rather than following the
/// player's FOV setting. A view model rendered at 90 or 110 degrees is
/// stretched and skewed towards the corner it sits in -- the wide angle
/// that makes the world feel fast makes anything near the lens look
/// wrong -- and pinning it here means the hand looks the same to every
/// player whatever they have the slider on.
const HAND_FOV_Y: f32 = 70.0 * std::f32::consts::PI / 180.0;

impl GraphicsState {
    pub async fn new(
        window: Arc<Window>,
        assets_dir: &Path,
        vsync: bool,
        anisotropy: u16,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("no compatible wgpu adapter found"))?;

        let info = adapter.get_info();
        println!("gpu: {} ({:?}, {:?})", info.name, info.device_type, info.backend);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Primitive device"),
                    // Only what the adapter actually offers, so a
                    // driver without timestamps still gets a device.
                    required_features: crate::engine::gpu_timing::GpuTiming::wanted(&adapter),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await?;

        // Before the device is moved into the struct.
        let gpu_timing = crate::engine::gpu_timing::GpuTiming::new(&device, &queue);

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            // FIX: the `vsync` setting used to be parsed and then ignored.
            present_mode: choose_present_mode(vsync, &surface_caps.present_modes),
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, &config);

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals uniform buffer"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("globals bind group layout"),
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

        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bind group"),
            layout: &globals_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let textures = TextureManager::load(&device, &queue, assets_dir, anisotropy)?;

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture bind group layout"),
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

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bind group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&textures.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&textures.sampler),
                },
            ],
        });

        let ui_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui texture bind group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&textures.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&textures.ui_sampler),
                },
            ],
        });

        let chunk_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let actor_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("actor shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("actor.wgsl").into()),
        });
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });

        let chunk_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk pipeline layout"),
            bind_group_layouts: &[&globals_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Terrain, with a fragment shader that cannot discard, so the
        // GPU keeps early depth rejection for the bulk of the frame.
        let chunk_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk pipeline"),
            layout: Some(&chunk_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_solid",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // The transparent pass. Three differences from the opaque one,
        // each load-bearing:
        //
        //   * alpha blending, so water actually shows what's behind it;
        //   * `depth_write_enabled: false`, or the nearest water surface
        //     would occlude the water and terrain behind it -- a lake
        //     would render as a flat sheet with a hole in the world
        //     under it;
        //   * `cull_mode: None`, so the underside of the surface is
        //     drawn when you're swimming and looking up.
        let transparent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transparent chunk pipeline"),
            layout: Some(&chunk_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_cutout",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
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

        // The cracks on the block being mined.
        //
        // The same geometry the transparent pipeline would have drawn,
        // and the same number of draws -- one, once, while a block is
        // being hit -- but it **multiplies** instead of blending. That
        // is the whole of "the damage is made of the block's texture
        // rather than laid over it": the shader returns a multiplier and
        // the blend applies it to what is already in the frame, which is
        // the block with its own light and its own fog already on it.
        // See `fs_crack`.
        //
        // Costs a pipeline object at startup and nothing per frame. The
        // crack draw already switched pipeline; it switches to a
        // different one.
        let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mining crack pipeline"),
            layout: Some(&chunk_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_crack",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    // dst * src, and nothing of the destination kept
                    // besides: the shader's output *is* the factor.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // As the transparent pipeline: the cracks on a tuft of
                // grass sit on planes that are drawn from both sides.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Tests but does not write, so the quad lies on the face
                // without taking the depth slot from it.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                // The cracks are *exactly* coplanar with the face they
                // damage -- the geometry is built from the same corners
                // the mesher used. Coplanar against `Less` loses the
                // depth test outright, and the old fix was to float the
                // quads a few millimetres off the block, which read as
                // exactly that: damage hovering on a cushion of air the
                // moment you saw the face at an angle. A depth bias
                // nudges the *comparison* toward the camera instead of
                // the geometry, so the cracks win the test while lying
                // flat on the surface. Slope scale matters more than the
                // constant here: it is glancing faces, where depth
                // changes fastest per pixel, that need the most room.
                bias: wgpu::DepthBiasState {
                    constant: -2,
                    slope_scale: -2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Leaves. Identical state to the solid pipeline -- it writes
        // depth and needs no sorting -- but with the discarding entry
        // point, so only the chunks containing a tree pay for it.
        let cutout_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cutout chunk pipeline"),
            layout: Some(&chunk_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_cutout",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Both sides: a leaf face seen from inside the canopy is
                // the back of a quad, and culling it hollows the tree
                // out again from within.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Dropped items. The cutout entry point, because a sprite is
        // mostly empty and the empty part has to be discarded; both
        // sides, because a plate one texel thick is seen from either.
        let item_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("item pipeline"),
            layout: Some(&chunk_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_item",
                buffers: &[crate::engine::item_model::ItemVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_cutout",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
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
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // The sky. Only the globals bind group -- it samples nothing.
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        // The cloud layer reads a picture of its own, at a size of its
        // own, so it cannot ride in the block array the way the font and
        // the crack stages do. One extra bind group, bound once a frame
        // for the one draw that uses it.
        let cloud_layout = crate::engine::texture::CloudTexture::bind_group_layout(&device);
        let cloud_bind_group = textures.clouds().bind_group(&device, &cloud_layout);

        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky pipeline layout"),
            bind_group_layouts: &[&globals_bind_group_layout, &cloud_layout],
            push_constant_ranges: &[],
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky pipeline"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: "vs_sky",
                // No vertex buffer at all: the triangle is three points
                // computed from the vertex index.
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: "fs_sky",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // The winding of a triangle generated from an index is
                // not something worth reasoning about, so it is not
                // culled.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Writes nothing, and survives only where the depth
                // buffer is still at the far plane -- which is exactly
                // where no terrain was drawn. Sky first with terrain
                // over it would shade every pixel of sky and then throw
                // most of it away.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // --- the small sky ---
        //
        // The same shaders again with two differences: no depth
        // attachment (there is no terrain in that pass to hide behind)
        // and a target that is a fraction of the frame. See
        // `sky_blit.wgsl`.
        let sky_offscreen_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky offscreen pipeline"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: "vs_sky",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: "fs_sky",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let sky_blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky_blit.wgsl").into()),
        });
        let sky_blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky blit bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
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
        // Linear, so the small texture's own pixels do not show as a
        // grid over the sky.
        let sky_blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky blit sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let sky_blit_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky blit pipeline layout"),
                bind_group_layouts: &[&sky_blit_layout],
                push_constant_ranges: &[],
            });
        let sky_blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky blit pipeline"),
            layout: Some(&sky_blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sky_blit_shader,
                entry_point: "vs_blit",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_blit_shader,
                entry_point: "fs_blit",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
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
            // Exactly the sky's own depth state: survives only where the
            // depth buffer is still at the far plane, and writes nothing.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let actor_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("actor pipeline layout"),
            bind_group_layouts: &[&globals_bind_group_layout],
            push_constant_ranges: &[],
        });

        let actor_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actor pipeline"),
            layout: Some(&actor_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &actor_shader,
                entry_point: "vs_main",
                buffers: &[ActorVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &actor_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay pipeline layout"),
                bind_group_layouts: &[&globals_bind_group_layout],
                push_constant_ranges: &[],
            });

        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay pipeline"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<OverlayVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &OVERLAY_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // No culling: a 2D quad's winding depends on the aspect
                // divide, and getting culled on a wide window would be a
                // baffling bug to chase.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            // The pass has a depth attachment, so the pipeline needs a
            // depth state -- but with the test always passing and no
            // writes, so the crosshair sits on top of everything.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let hotbar_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hotbar shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hotbar.wgsl").into()),
        });

        let hotbar_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("hotbar pipeline layout"),
                // Same texture bind group as the terrain: hotbar icons
                // are the block textures themselves, not a second atlas.
                bind_group_layouts: &[&globals_bind_group_layout, &texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        let hotbar_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hotbar pipeline"),
            layout: Some(&hotbar_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &hotbar_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<HotbarVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &HOTBAR_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &hotbar_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
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
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // The first-person hand.
        //
        // Shares the hotbar's layout -- globals, plus the block textures
        // -- and is bound to the *UI* texture bind group when it draws:
        // a held pick is a 16x16 picture magnified to a quarter of the
        // screen, which is the one case where the always-nearest sampler
        // is unambiguously right and the terrain's filtered one would be
        // a smear. See `build_ui_sampler`.
        let hand_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hand shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hand.wgsl").into()),
        });
        let hand_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hand pipeline"),
            layout: Some(&hotbar_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &hand_shader,
                entry_point: "vs_main",
                buffers: &[crate::logic::hand::HandVertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &hand_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Both sides, for the same reason the dropped items are:
                // a tool is a plate one texel thick and the swing turns
                // it far enough to be seen from behind.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            // **Depth tested and written, in a slice of the buffer
            // reserved for it.** The hand is drawn last of the solid
            // geometry, into the viewport depth range 0..`HAND_DEPTH_SLICE`
            // -- see the draw itself for why that beats every
            // alternative -- and inside that slice it tests and writes
            // normally, which is what makes the arm occlude the item it
            // is holding rather than the two interleaving.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let hotbar_capacity = MAX_HOTBAR_VERTICES;
        let hotbar_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hotbar/UI vertex buffer"),
            size: (hotbar_capacity * std::mem::size_of::<HotbarVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Before the struct literal, which moves `device` in.
        let arena =
            crate::engine::arena::Arena::new(&device, std::mem::size_of::<Vertex>() as u64);

        let overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay vertex buffer"),
            size: (MAX_OVERLAY_VERTICES * std::mem::size_of::<OverlayVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            chunk_pipeline,
            cutout_pipeline,
            item_pipeline,
            sky_pipeline,
            cloud_bind_group,
            transparent_pipeline,
            crack_pipeline,
            actor_pipeline,
            hand_pipeline,
            overlay_pipeline,
            overlay_buffer,
            hotbar_pipeline,
            hotbar_buffer,
            hotbar_capacity,
            hotbar_vertex_count: 0,
            depth_view,
            globals_buffer,
            globals_bind_group,
            texture_bind_group,
            ui_texture_bind_group,
            texture_bind_group_layout,
            anisotropy,
            present_modes: surface_caps.present_modes,
            gpu_timing,
            // Off until asked for: the sky is the one thing in the
            // frame this softens, and softening it is the player's call.
            sky_scale: 1,
            sky_offscreen_pipeline,
            sky_blit_pipeline,
            sky_blit_layout,
            sky_blit_sampler,
            sky_target: None,
            overlay_scratch: Vec::with_capacity(MAX_OVERLAY_VERTICES),
            overlay_key: None,
            arena,
            chunk_meshes: HashMap::new(),
            textures,
            draw_calls_last_frame: 0,
            chunks_culled_last_frame: 0,
            encode_time_last_frame: std::time::Duration::ZERO,
            present_time_last_frame: std::time::Duration::ZERO,
            acquire_time_last_frame: std::time::Duration::ZERO,
        })
    }

    /// Rebuilds the block sampler for a new anisotropy setting.
    ///
    /// Cheap enough to do from the settings screen -- one sampler and
    /// one bind group -- so the player sees the change immediately
    /// rather than being told to restart.
    pub fn set_anisotropy(&mut self, anisotropy: u16) {
        if anisotropy == self.anisotropy {
            return;
        }
        self.anisotropy = anisotropy;
        self.textures.sampler = crate::engine::texture::build_sampler(&self.device, anisotropy);
        self.texture_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bind group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.textures.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.textures.sampler),
                },
            ],
        });
    }

    pub fn aspect(&self) -> f32 {
        self.config.width.max(1) as f32 / self.config.height.max(1) as f32
    }

    /// How long the GPU spent on the last measured render pass, in
    /// milliseconds.
    ///
    /// `None` on a driver without timestamp queries, and for the first
    /// frames of a session before one has come back. Deliberately not
    /// folded into the frame breakdown: `simulation`, `encode` and
    /// `present` sum to the frame, and this one does not -- it overlaps
    /// all three, because that is what a GPU does.
    pub fn gpu_time_last_frame(&self) -> Option<f32> {
        self.gpu_timing.as_ref().and_then(|timing| timing.last_ms())
    }

    /// The same, split into the stages of the pass -- see
    /// `gpu_timing::STAGES`. `None` where the driver will not write
    /// timestamps inside a pass, which is the case the whole-pass
    /// number exists to still serve.
    pub fn gpu_stage_ms_last_frame(&self) -> Option<[f32; crate::engine::gpu_timing::STAGES.len()]> {
        self.gpu_timing
            .as_ref()
            .and_then(|timing| timing.last_stage_ms())
            .copied()
    }

    /// Switches vsync on or off without restarting.
    ///
    /// Reconfiguring the surface is all it takes, and it is what makes
    /// the setting worth having in the game at all -- a vsync toggle you
    /// have to restart to evaluate is one nobody uses. No-ops when the
    /// mode is already what was asked for, since reconfiguring drops the
    /// swapchain and blinks the window.
    pub fn set_vsync(&mut self, vsync: bool) {
        let wanted = choose_present_mode(vsync, &self.present_modes);
        if self.config.present_mode == wanted {
            return;
        }
        self.config.present_mode = wanted;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config);
        self.sky_target = create_sky_target(
            &self.device,
            &self.config,
            &self.sky_blit_layout,
            &self.sky_blit_sampler,
            self.sky_scale,
        );
    }

    /// How much smaller than the frame the sky is drawn.
    ///
    /// 1 puts it straight into the frame, as it always went. Anything
    /// more divides its width and height by that much and stretches the
    /// result back -- a quarter of the pixels at 2, a ninth at 3. See
    /// `sky_blit.wgsl` for what that costs and why it is the only
    /// saving left on this pass.
    pub fn set_sky_scale(&mut self, scale: u32) {
        let scale = scale.clamp(1, 4);
        if scale == self.sky_scale {
            return;
        }
        self.sky_scale = scale;
        self.sky_target = create_sky_target(
            &self.device,
            &self.config,
            &self.sky_blit_layout,
            &self.sky_blit_sampler,
            scale,
        );
    }

    /// Uploads geometry that is entirely opaque (entities, remote
    /// players). Chunks go through `upload_chunk_mesh`, which carries the
    /// opaque/translucent split.
    /// Writes geometry that changes every frame into buffers that don't.
    ///
    /// Falling blocks and other players are rebuilt each frame because
    /// they move each frame. Uploading them with `upload_mesh` meant
    /// *creating two GPU buffers per mesh per frame* -- at sixty frames
    /// a second with both present, more than two hundred allocations a
    /// second, each of which the driver has to track and eventually
    /// free, for data whose size barely changes.
    ///
    /// Here the buffers persist and only their contents are rewritten.
    /// They grow to the next power of two when something bigger arrives
    /// -- a sandslide of a thousand blocks does reallocate, once -- and
    /// never shrink, because the next frame usually wants the same room
    /// again.
    pub fn write_dynamic_mesh<V: Pod>(
        &self,
        target: &mut DynamicMesh,
        vertices: &[V],
        indices: &[u32],
    ) {
        let vertex_bytes = std::mem::size_of_val(vertices);
        let index_bytes = std::mem::size_of_val(indices);
        if vertex_bytes == 0 || index_bytes == 0 {
            target.index_count = 0;
            return;
        }

        if vertex_bytes > target.vertex_capacity {
            target.vertex_capacity = vertex_bytes.next_power_of_two();
            target.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic mesh vertex buffer"),
                size: target.vertex_capacity as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if index_bytes > target.index_capacity {
            target.index_capacity = index_bytes.next_power_of_two();
            target.index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic mesh index buffer"),
                size: target.index_capacity as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        self.queue
            .write_buffer(&target.vertex_buffer, 0, bytemuck::cast_slice(vertices));
        self.queue
            .write_buffer(&target.index_buffer, 0, bytemuck::cast_slice(indices));
        target.index_count = indices.len() as u32;
    }

    /// An empty dynamic mesh, ready to be written into.
    pub fn new_dynamic_mesh(&self) -> DynamicMesh {
        let empty = |label, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 0,
                usage,
                mapped_at_creation: false,
            })
        };
        DynamicMesh {
            vertex_buffer: empty(
                "dynamic mesh vertex buffer",
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            ),
            index_buffer: empty(
                "dynamic mesh index buffer",
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            ),
            vertex_capacity: 0,
            index_capacity: 0,
            index_count: 0,
        }
    }

    /// Puts a chunk's geometry on the GPU, replacing whatever was there
    /// for that position.
    ///
    /// Replacing rather than adding matters: a re-meshed chunk arrives
    /// through here every time a block near it changes, and the space
    /// its previous mesh held has to go back to the arena. Doing that
    /// anywhere but inside this call is how a session slowly eats its
    /// own vertex buffer.
    pub fn set_chunk_mesh(&mut self, pos: ChunkPos, buffers: &crate::engine::mesh::MeshBuffers) {
        let (vertex_block, index_block) = self.arena.upload(
            &self.device,
            &self.queue,
            &buffers.vertices,
            &buffers.indices,
        );
        let count = buffers.indices.len() as u32;
        let mesh = GpuMesh {
            vertex_block,
            index_block,
            num_indices: count,
            solid_indices: buffers.solid_index_count.min(count),
            leaf_end: buffers.leaf_end.min(count),
            sprite_end: buffers.sprite_end.min(count),
        };
        if let Some(old) = self.chunk_meshes.insert(pos, mesh) {
            self.arena.free(old.vertex_block, old.index_block);
        }
    }

    /// Forgets one chunk, giving its space back.
    pub fn drop_chunk_mesh(&mut self, pos: ChunkPos) {
        if let Some(old) = self.chunk_meshes.remove(&pos) {
            self.arena.free(old.vertex_block, old.index_block);
        }
    }

    /// Forgets the whole world -- leaving a session, or joining another.
    pub fn clear_chunk_meshes(&mut self) {
        for (_, mesh) in self.chunk_meshes.drain() {
            self.arena.free(mesh.vertex_block, mesh.index_block);
        }
        self.arena.release_all();
    }


    // One argument per thing that can be drawn this frame. Bundling
    // them into a struct would move the same list somewhere else and
    // add a lifetime to it.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        camera: &Camera,
        params: &FrameParams,
        actor_mesh: Option<&DynamicMesh>,
        entity_mesh: Option<&DynamicMesh>,
        item_mesh: Option<&DynamicMesh>,
        break_mesh: Option<&DynamicMesh>,
        // The player's own arm, in view space. Empty whenever the hand
        // should not be seen -- see `logic::hand`.
        hand_mesh: Option<&DynamicMesh>,
        loading: Option<f32>,
        hotbar: &[HotbarVertex],
        // Whether `hotbar` was rebuilt since the last upload. The
        // interface is only rebuilt when something on it changed -- see
        // the UI key in `main` -- so most frames hand over the identical
        // vertices, and re-uploading them is a memcpy of tens (menus:
        // hundreds) of kilobytes for nothing.
        ui_changed: bool,
    ) -> Result<(), wgpu::SurfaceError> {
        let started = std::time::Instant::now();

        // A frame has started, so space freed a few frames ago -- long
        // enough for the GPU to be done with it -- can be handed out
        // again. See `arena::QUARANTINE_FRAMES`.
        self.arena.begin_frame();

        let mut overlay = std::mem::take(&mut self.overlay_scratch);
        let overlay_key = (
            loading.map(f32::to_bits),
            self.aspect().to_bits(),
            params.hurt.to_bits(),
        );
        if self.overlay_key != Some(overlay_key) {
            self.overlay_key = Some(overlay_key);
            overlay_into(&mut overlay, loading, self.aspect(), params.hurt);
            self.queue
                .write_buffer(&self.overlay_buffer, 0, bytemuck::cast_slice(&overlay));
        }
        if ui_changed {
            // Grow rather than truncate. Silently dropping the tail of
            // the UI would show half a server list with no hint why.
            if hotbar.len() > self.hotbar_capacity {
                self.hotbar_capacity = hotbar.len().next_power_of_two();
                self.hotbar_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("hotbar/UI vertex buffer"),
                    size: (self.hotbar_capacity * std::mem::size_of::<HotbarVertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if !hotbar.is_empty() {
                self.queue
                    .write_buffer(&self.hotbar_buffer, 0, bytemuck::cast_slice(hotbar));
            }
            // Recorded even for an empty list: "the interface is now
            // nothing" is a change like any other, and drawing the old
            // count would leave the previous screen's ghost up.
            self.hotbar_vertex_count = hotbar.len() as u32;
        }

        // Built once. It is two matrix constructions (with the trig for
        // the view direction in them) and a multiply, and it was being
        // asked for three times a frame -- for the globals, for its own
        // inverse, and again for the frustum.
        let view_proj = camera.view_proj();

        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            camera_pos: [camera.position.x, camera.position.y, camera.position.z, 0.0],
            sun: [
                params.sun_direction.x,
                params.sun_direction.y,
                params.sun_direction.z,
                params.sun_intensity,
            ],
            fog_color: [
                params.fog_color.x,
                params.fog_color.y,
                params.fog_color.z,
                1.0,
            ],
            fog_params: [
                params.fog_start,
                params.fog_end,
                params.ambient,
                self.aspect(),
            ],
            extra: [
                params.block_light_boost,
                params.ao_strength,
                if params.underwater { 1.0 } else { 0.0 },
                if params.fog_enabled { 1.0 } else { 0.0 },
            ],
            // Zero when filtering is off, which is the shader's signal
            // to take the plain nearest path instead of the snapped,
            // explicit-gradient one.
            texture_params: [
                if self.anisotropy > 1 {
                    self.textures.resolution as f32
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ],
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            sky_params: [
                params.time_of_day,
                params.cloudiness,
                params.elapsed_seconds,
                0.0,
            ],
            // A projection and nothing else: the hand's geometry is
            // already in view space, so there is no view matrix to
            // apply. A one-centimetre near plane, because the nearest
            // knuckle is closer than the world's near plane would allow,
            // and a far plane of four blocks, because nothing drawn by
            // this pipeline is further away than an arm's length.
            hand_view_proj: glam::Mat4::perspective_rh(
                HAND_FOV_Y,
                self.aspect(),
                0.01,
                4.0,
            )
            .to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let waited = std::time::Instant::now();
        let output = self.surface.get_current_texture()?;
        self.acquire_time_last_frame = waited.elapsed();
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        // Clearing to the fog colour rather than a fixed sky blue is what
        // makes the fog actually hide the render-distance edge: terrain
        // fades into exactly the colour that's already behind it.
        let clear = params.fog_color;
        let mut draw_calls = 0u32;

        // Taken out of `self` for the length of both passes: the marks
        // borrow the query set while the passes borrow the pipelines,
        // and all of it lives here. Put back below, the same way
        // `overlay_scratch` is.
        let mut gpu_timing = self.gpu_timing.take();
        let (timestamp_writes, marks, sky_timestamps) =
            match gpu_timing.as_mut().and_then(|t| t.pass_writes()) {
                Some((writes, marks, sky)) => (Some(writes), marks, Some(sky)),
                None => (None, None, None),
            };
        // Closes a stage: the GPU writes the mark when it reaches this
        // point in the pass, so the gap to the previous one is what that
        // stage actually cost. A no-op where the driver will not take
        // timestamps mid-pass.
        let mark = |pass: &mut wgpu::RenderPass<'_>, stage: usize| {
            if let Some(query_set) = marks {
                pass.write_timestamp(query_set, GpuTiming::stage_query(stage));
            }
        };

        // The sky, into its own small texture, before the frame proper.
        // Its own pass because it has its own target and no depth; the
        // main pass then stretches the result over whatever the terrain
        // did not cover. See `sky_blit.wgsl`.
        if let Some((sky_view, _)) = &self.sky_target {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: sky_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Nothing is read from underneath: the sky
                        // covers every pixel of its own target.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: sky_timestamps,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.sky_offscreen_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.cloud_bind_group, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.x as f64,
                            g: clear.y as f64,
                            b: clear.z as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.chunk_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.texture_bind_group, &[]);
            // Only draw what the camera can actually see. At render
            // distance 8 that's typically a third of the loaded chunks;
            // the rest used to be transformed and clipped for nothing.
            let frustum = Frustum::from_view_proj(view_proj);

            // Everything that survives culling, with its squared distance
            // from the camera. Collected once and used by all three
            // passes: frustum-testing every chunk again for the second
            // one would be repeat work, and the list has to be sorted
            // anyway.
            //
            // The mesh itself rather than its position. This used to
            // carry a `ChunkPos` and look the mesh back up in the map on
            // every pass -- three hashes per visible chunk per frame,
            // which at a few hundred chunks is a thousand hash lookups a
            // frame to re-find data we had already found. One `Vec` of
            // borrowed references costs a single allocation instead.
            // Past this, fog has already turned everything into the
            // clear colour, so drawing it changes no pixel.
            //
            // **Why this is free rather than a trade.** The fog is
            // opaque at `fog_end` -- `shade` mixes all the way to the fog
            // colour -- and the buffer is *cleared* to that same colour,
            // which is the whole reason the render-distance edge
            // dissolves instead of showing as a wall. So a chunk beyond
            // it produces exactly the colour that is already there. The
            // sky pass, which fills whatever the terrain did not, agrees
            // too: toward the horizon its gradient becomes the fog
            // colour exactly, for the same reason.
            //
            // It matters because the loaded area is a *square* and sight
            // is a circle: at a render distance of sixteen the corners
            // reach 360 blocks against a fog end of 240, and it is those
            // corners -- a third of the chunks -- that were being drawn
            // to no effect. Measured at 584 draws a frame before and 373
            // after, with the picture identical.
            //
            // Off entirely when the player has turned fog off, where
            // there is no distance past which things stop being visible.
            // The chunk's own radius is added so a chunk is only dropped
            // once *all* of it is past the line.
            //
            // Off under water too, and that one is not obvious: the
            // terrain shader dims everything toward the water colour
            // *before* the fog mix, so at maximum distance a submerged
            // chunk lands on the fog colour while the sky behind it
            // lands on 55% of it. On land the two agree exactly; here
            // they do not, and the difference would show as a dark band
            // where the far terrain used to be. Nothing is lost by
            // keeping it: under water the fog end is eighteen blocks, so
            // the frustum has already thrown away almost everything.
            let fog_cull_squared = match params.fog_cull_distance {
                // The chunk's own radius is added so a chunk is dropped
                // only once *all* of it is past the line.
                Some(limit) => (limit + CHUNK_RADIUS) * (limit + CHUNK_RADIUS),
                None => f32::INFINITY,
            };

            let mut draw_order: Vec<(f32, &GpuMesh)> =
                Vec::with_capacity(self.chunk_meshes.len());
            for (pos, gpu_mesh) in self.chunk_meshes.iter() {
                if gpu_mesh.num_indices == 0 || !frustum.contains_chunk(*pos) {
                    continue;
                }
                let centre_x = (pos.x as f32 + 0.5) * 16.0;
                let centre_z = (pos.z as f32 + 0.5) * 16.0;
                let dx = centre_x - camera.position.x;
                let dz = centre_z - camera.position.z;
                let distance_squared = dx * dx + dz * dz;
                if distance_squared > fog_cull_squared {
                    continue;
                }
                draw_order.push((distance_squared, gpu_mesh));
            }
            self.chunks_culled_last_frame =
                self.chunk_meshes.len().saturating_sub(draw_order.len());

            // Near to far.
            //
            // The order does not change what an opaque pass *produces* --
            // the depth test settles that -- but it changes how much of it
            // the GPU has to shade. Nearest first, the depth buffer is
            // already populated when the far chunks arrive and most of
            // their fragments are rejected before the fragment shader
            // runs. This used to draw in hash order, which is to say
            // randomly, so a large share of the terrain was shaded and
            // then overwritten by something in front of it.
            //
            // Sorting a few hundred f32 keys is microseconds and does not
            // allocate.
            draw_order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

            // The terrain lives in two shared buffers, so the binds
            // happen once for the whole world instead of twice per
            // chunk. Everything below is one `draw_indexed` per chunk
            // per pass and nothing else -- which is the entire reason
            // the arena exists. See `engine::arena`.
            pass.set_vertex_buffer(0, self.arena.vertex_buffer.slice(..));
            pass.set_index_buffer(
                self.arena.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );

            // How far out leaves stop being drawn as cutouts. Computed
            // here because the solid pass uses it too -- see below.
            let leaf_cutout_limit = if params.transparent_leaves {
                (params.view_distance * 0.45).powi(2)
            } else {
                -1.0
            };

            for (distance, gpu_mesh) in &draw_order {
                // A chunk whose leaves are drawn solid can have them in
                // *this* draw.
                //
                // The two ranges are adjacent in the index buffer --
                // solid first, leaves immediately after -- so when the
                // leaves are going through this same pipeline anyway,
                // one draw over both is the same triangles for one call
                // instead of two. At a render distance of eight that is
                // forty-odd calls a frame, and a call measured at 0.85
                // microseconds.
                let solid_leaves = *distance > leaf_cutout_limit;
                let end = if solid_leaves {
                    gpu_mesh.leaf_end
                } else {
                    gpu_mesh.solid_indices
                };
                if end == 0 {
                    continue;
                }
                pass.draw_indexed(gpu_mesh.indices(0, end), gpu_mesh.base_vertex(), 0..1);
                draw_calls += 1;
            }
            mark(&mut pass, 0); // solid terrain

            // --- leaves ---
            //
            // Still near to far: they write depth like the solid pass,
            // so the same ordering helps, and by now the solid terrain
            // has already filled the depth buffer in front of most of
            // them.
            //
            // **Only the near ones are drawn as cutouts.** A shader
            // that can `discard` costs the GPU early depth rejection on
            // every draw that uses it, and a canopy is the worst thing
            // to pay that on: layer over layer of fragments, each one
            // shaded before the depth test throws it away. Past the
            // limit below, the holes in a leaf texture are smaller than
            // a pixel -- the mip chain has already averaged them away --
            // so those chunks go through the solid pipeline and get
            // their early-Z back. Nothing changes on screen: what would
            // have been a hole is filled with the colour the filter was
            // returning for it anyway.
            //
            // The list is sorted, so the near ones are a prefix of it
            // and the pipeline changes exactly once.
            // Off entirely when the player has asked for solid leaves,
            // which is the same switch as "everything is too far away"
            // and needs no second code path to express.
            // Only the near ones are left: the far ones went out with
            // the solid pass above, in the same draw as the terrain they
            // sit on.
            // Leaves and sprites share this pipeline and sit in
            // adjacent index ranges, so a chunk near enough for both
            // goes out as **one** draw over the two of them. Near the
            // player -- which is where the trees and the grass both are
            // -- that is half the cutout calls in the frame, for an
            // `if`. The same trick the solid pass plays on distant
            // leaves, in the other direction.
            let sprite_limit = (params.view_distance * params.detail_distance).powi(2);
            let mut bound_leaves = false;
            for (distance, mesh) in &draw_order {
                if *distance > leaf_cutout_limit {
                    break; // sorted near to far
                }
                let end = if *distance <= sprite_limit {
                    mesh.sprite_end
                } else {
                    mesh.leaf_end
                };
                if end <= mesh.solid_indices {
                    continue; // nothing cut out in this chunk
                }
                if !bound_leaves {
                    pass.set_pipeline(&self.cutout_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    bound_leaves = true;
                }
                pass.draw_indexed(
                    mesh.indices(mesh.solid_indices, end),
                    mesh.base_vertex(),
                    0..1,
                );
                draw_calls += 1;
            }

            // --- sprites: tufts of grass, twigs ---
            //
            // These cannot take the treatment above. A leaf texture is
            // three-quarters solid and filling its holes at distance
            // changes nothing; a tuft of grass is nine-tenths empty, and
            // drawing one without the cutout would put a solid green
            // square in the air.
            //
            // What they get instead is a distance of their own, and it
            // is the player's to set. A field is a tuft every three
            // columns -- much the densest thing in the world, and much
            // the least worth drawing at range, where each one is a
            // couple of pixels the fog is already washing out. Loose
            // stones ride in the same range for the same reason: one
            // quad each, but in every biome and every chunk.
            let mut bound_sprites = false;
            for (distance, mesh) in &draw_order {
                if *distance > sprite_limit {
                    break; // sorted near to far: everything after is further
                }
                if *distance <= leaf_cutout_limit {
                    continue; // already drawn, merged with that chunk's leaves
                }
                if mesh.sprite_end <= mesh.leaf_end {
                    continue;
                }
                if !bound_sprites {
                    pass.set_pipeline(&self.cutout_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    bound_sprites = true;
                }
                pass.draw_indexed(
                    mesh.indices(mesh.leaf_end, mesh.sprite_end),
                    mesh.base_vertex(),
                    0..1,
                );
                draw_calls += 1;
            }

            mark(&mut pass, 1); // leaves and sprites, the cutout passes

            // Anything after those passes has to rebind the solid
            // pipeline before drawing with it again.
            if bound_sprites || bound_leaves {
                pass.set_pipeline(&self.chunk_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
            }

            // --- the sky ---
            //
            // After the opaque terrain, so the depth test throws away
            // every pixel the world already covers; before anything
            // blended, so water and the mining cracks composite over it.
            match &self.sky_target {
                // Already drawn, small: stretch it over the gap.
                Some((_, blit)) => {
                    pass.set_pipeline(&self.sky_blit_pipeline);
                    pass.set_bind_group(0, blit, &[]);
                }
                // Full scale: straight into the frame, as it always was.
                None => {
                    pass.set_pipeline(&self.sky_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.cloud_bind_group, &[]);
                }
            }
            pass.draw(0..3, 0..1);
            draw_calls += 1;
            mark(&mut pass, 2); // sky

            // Back to the terrain pipeline for what follows.
            pass.set_pipeline(&self.chunk_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.texture_bind_group, &[]);

            // Entities share the chunk pipeline and its bind groups, so
            // they slot in here with no state change beyond the buffers.
            if let Some(mesh) = entity_mesh {
                if !mesh.is_empty() {
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }

            // Dropped items: their own pipeline and vertex format, so
            // they go after the entities that share the terrain's.
            if let Some(items) = item_mesh {
                if !items.is_empty() {
                    pass.set_pipeline(&self.item_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    pass.set_vertex_buffer(0, items.vertex_buffer.slice(..));
                    pass.set_index_buffer(items.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..items.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }

            if let Some(actors) = actor_mesh {
                if !actors.is_empty() {
                    pass.set_pipeline(&self.actor_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_vertex_buffer(0, actors.vertex_buffer.slice(..));
                    pass.set_index_buffer(actors.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..actors.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }

            // The cracks on the block being mined: quads lying exactly
            // on the surface (a depth bias, not an offset, keeps them
            // in front of it), which *multiply* the face already drawn
            // there rather than covering it. See
            // `crack_pipeline`. Depth is tested and not written, so they
            // lie on the face without fighting it and the water pass
            // below still composites correctly over them.
            //
            // After the terrain and before the water, which is where a
            // multiply has to go: over the block it damages, under
            // anything that has to be seen through.
            if let Some(cracks) = break_mesh {
                if !cracks.is_empty() {
                    pass.set_pipeline(&self.crack_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    pass.set_vertex_buffer(0, cracks.vertex_buffer.slice(..));
                    pass.set_index_buffer(cracks.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..cracks.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }

            mark(&mut pass, 3); // entities, items, other players, cracks

            // --- transparent pass ---
            //
            // Last, so everything it blends over has already been drawn:
            // terrain, falling blocks and other players. Sorted
            // far-to-near, because with blending the draw order *is* the
            // composite order -- two lakes at different distances come
            // out wrong if the near one is blended in first.
            //
            // The same list the opaque pass used, walked backwards: it is
            // already sorted near to far, and far to near is exactly what
            // this pass wants.
            let mut bound_transparent = false;
            for (_, mesh) in draw_order.iter().rev() {
                if mesh.sprite_end >= mesh.num_indices {
                    continue; // no water in this chunk
                }
                if !bound_transparent {
                    pass.set_pipeline(&self.transparent_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    // Entities, other players and the mining cracks each
                    // have buffers of their own and any of them may have
                    // drawn since the solid pass, so the shared terrain
                    // buffers are bound again here. Once per frame, not
                    // once per chunk.
                    pass.set_vertex_buffer(0, self.arena.vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        self.arena.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    bound_transparent = true;
                }
                pass.draw_indexed(
                    mesh.indices(mesh.sprite_end, mesh.num_indices),
                    mesh.base_vertex(),
                    0..1,
                );
                draw_calls += 1;
            }

            mark(&mut pass, 4); // blended water

            // --- the player's own hand ---
            //
            // After everything in the world and before everything on the
            // glass, which is exactly where a view model belongs: it is
            // occluded by nothing and occludes nothing, and the crosshair
            // and the hotbar still sit on top of it.
            //
            // The narrowed depth range is what keeps it out of the walls;
            // see `HAND_DEPTH_SLICE` for the reasoning and for the two
            // alternatives it was chosen over. The viewport is put back
            // immediately afterwards -- the interface below does not test
            // depth and would not notice, but a viewport left narrowed is
            // the kind of state that the *next* thing drawn here will
            // notice, at a distance, months from now.
            if let Some(hand) = hand_mesh {
                if !hand.is_empty() {
                    let (width, height) = (self.config.width as f32, self.config.height as f32);
                    pass.set_viewport(0.0, 0.0, width, height, 0.0, HAND_DEPTH_SLICE);
                    pass.set_pipeline(&self.hand_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.ui_texture_bind_group, &[]);
                    pass.set_vertex_buffer(0, hand.vertex_buffer.slice(..));
                    pass.set_index_buffer(hand.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..hand.index_count, 0, 0..1);
                    draw_calls += 1;
                    pass.set_viewport(0.0, 0.0, width, height, 0.0, 1.0);
                }
            }

            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_vertex_buffer(0, self.overlay_buffer.slice(..));
            pass.draw(0..overlay.len() as u32, 0..1);
            draw_calls += 1;

            // The UI layer: hotbar, debug panel, menus. Drawn last and
            // unconditionally -- what belongs on screen is the caller's
            // decision, and gating it here once meant the pause screen
            // disappeared if you opened it while the world was still
            // loading.
            // The *stored* count, not the slice's length. On a frame
            // with no rebuild the slice is whatever the caller kept
            // around, and the buffer holds the last upload -- the count
            // taken then is the one that matches what is in it.
            if self.hotbar_vertex_count > 0 {
                pass.set_pipeline(&self.hotbar_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_bind_group(1, &self.ui_texture_bind_group, &[]);
                pass.set_vertex_buffer(0, self.hotbar_buffer.slice(..));
                pass.draw(0..self.hotbar_vertex_count, 0..1);
                draw_calls += 1;
            }
            mark(&mut pass, 5); // the hand, crosshair, hotbar, panels
        }

        // The GPU's own answer, copied out on the same encoder as the
        // pass it measures.
        if let Some(timing) = &mut gpu_timing {
            timing.resolve(&mut encoder, self.sky_target.is_some());
        }

        self.draw_calls_last_frame = draw_calls;
        self.encode_time_last_frame = started
            .elapsed()
            .saturating_sub(self.acquire_time_last_frame);

        let handover = std::time::Instant::now();
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        self.present_time_last_frame = handover.elapsed();

        if let Some(timing) = &mut gpu_timing {
            timing.after_submit(&self.device);
        }
        self.gpu_timing = gpu_timing;
        // The scratch buffer goes back where it came from, capacity and
        // all -- that is the whole point of taking it rather than
        // building a fresh one.
        self.overlay_scratch = overlay;
        Ok(())
    }
}

const HOTBAR_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32, 3 => Float32x4];

const OVERLAY_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

/// Screen-space overlay geometry: the crosshair, and the loading bar
/// shown while the world under the player streams in.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

/// Half the diagonal of a chunk's footprint, in blocks.
///
/// What has to be added to any distance test done against a chunk's
/// *centre* before it can be used to decide that none of the chunk is
/// near enough to matter. 16x16 across, so the corner is 8*sqrt(2) out.
const CHUNK_RADIUS: f32 = 11.32;

/// Enough room for the crosshair, the loading screen's backdrop, bar
/// frame and fill, and the four bands of the hurt vignette.
const MAX_OVERLAY_VERTICES: usize = 128;

/// Two thin bars in NDC. The vertex shader divides x by the aspect ratio,
/// so this is authored as if the viewport were square.
fn push_quad(v: &mut Vec<OverlayVertex>, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]) {
    for position in [
        [x0, y0],
        [x1, y0],
        [x1, y1],
        [x0, y0],
        [x1, y1],
        [x0, y1],
    ] {
        v.push(OverlayVertex { position, color });
    }
}

/// Builds this frame's overlay.
///
/// `loading` is `Some(progress 0..1)` while the world around the player
/// is still streaming in. There's no font pipeline in the game, so the
/// loading screen is geometry: a dimming backdrop, a bar frame and a
/// fill that grows. That's enough to answer the only question the player
/// has ("is it stuck, or is it working?") without a glyph atlas.
#[cfg(test)]
pub fn overlay_vertices(loading: Option<f32>, aspect: f32, hurt: f32) -> Vec<OverlayVertex> {
    let mut v = Vec::with_capacity(MAX_OVERLAY_VERTICES);
    overlay_into(&mut v, loading, aspect, hurt);
    v
}

/// The same, into a buffer that already exists. See `overlay_scratch`.
///
/// `hurt` is 0 (fine) to 1 (about to die) and draws the vignette; see
/// `vignette` for what it is and why it is at the edges.
fn overlay_into(v: &mut Vec<OverlayVertex>, loading: Option<f32>, aspect: f32, hurt: f32) {
    v.clear();

    if let Some(progress) = loading {
        let progress = progress.clamp(0.0, 1.0);
        // Backdrop, drawn wide enough to cover the screen after the
        // aspect divide in the vertex shader.
        push_quad(v, -4.0, -4.0, 4.0, 4.0, [0.02, 0.03, 0.05, 0.72]);

        const HALF_W: f32 = 0.34;
        const HALF_H: f32 = 0.022;
        const BORDER: f32 = 0.006;
        // Frame.
        push_quad(
            v,
            -HALF_W - BORDER,
            -HALF_H - BORDER,
            HALF_W + BORDER,
            HALF_H + BORDER,
            [0.85, 0.88, 0.92, 0.85],
        );
        // Trough.
        push_quad(v, -HALF_W, -HALF_H, HALF_W, HALF_H, [0.06, 0.08, 0.11, 0.95]);
        // Fill.
        if progress > 0.0 {
            let right = -HALF_W + 2.0 * HALF_W * progress;
            push_quad(v, -HALF_W, -HALF_H, right, HALF_H, [0.42, 0.78, 0.45, 1.0]);
        }
        return;
    }

    // The vignette goes under the crosshair: the crosshair is the one
    // thing on this screen that must stay legible whatever else is
    // happening.
    vignette(v, aspect, hurt);

    // A crosshair with an outline, and a hole in the middle.
    //
    // Plain white was invisible against snow, sand and the sky -- which
    // between them are most of what a player aims at outdoors. The dark
    // bar underneath is a pixel wider on every side, so whichever of the
    // two the background swallows, the other one shows. The hole is
    // there because the centre is exactly where the thing being aimed at
    // is, and a solid cross covers it.
    const ARM: f32 = 0.019;
    const HOLE: f32 = 0.0055;
    const THICK: f32 = 0.0026;
    const EDGE: f32 = 0.0016;
    const CROSSHAIR: [f32; 4] = [1.0, 1.0, 1.0, 0.9];
    const OUTLINE: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
    for (thick, colour) in [(THICK + EDGE, OUTLINE), (THICK, CROSSHAIR)] {
        push_quad(v, -ARM, -thick, -HOLE, thick, colour);
        push_quad(v, HOLE, -thick, ARM, thick, colour);
        push_quad(v, -thick, -ARM, thick, -HOLE, colour);
        push_quad(v, -thick, HOLE, thick, ARM, colour);
    }
}

/// Darkens the edges of the screen as health goes.
///
/// Four bands, each fading from the edge inward, drawn in a colour that
/// is nearly black with just enough red in it to read as blood rather
/// than as night. The corners are covered by two bands at once and so
/// come out darkest, which is what a vignette wants anyway.
///
/// **Why the edges rather than the whole screen.** A wash over
/// everything is a filter over the game; a vignette closes the *view*
/// in, which is what being badly hurt does. It also leaves the middle of
/// the screen -- where the crosshair, the thing being aimed at, and any
/// oncoming trouble are -- exactly as legible as it was.
///
/// Nothing at all above two thirds health: a permanent frame around the
/// picture stops being information and becomes the picture.
fn vignette(v: &mut Vec<OverlayVertex>, aspect: f32, hurt: f32) {
    /// How far in the darkness reaches, as a fraction of the half-height.
    const REACH: f32 = 0.62;
    const COLOUR: [f32; 3] = [0.26, 0.01, 0.02];

    let hurt = if hurt.is_finite() { hurt.clamp(0.0, 1.0) } else { 0.0 };
    if hurt <= 0.0 {
        return;
    }
    // Eased, so the first sign of it is a hint at the corners rather
    // than a band appearing.
    let alpha = hurt * hurt * 0.86;
    let outer = [COLOUR[0], COLOUR[1], COLOUR[2], alpha];
    let inner = [COLOUR[0], COLOUR[1], COLOUR[2], 0.0];

    // Authored as if the viewport were square, so the x edges sit at
    // ±aspect: the shader divides x by it. Reaching past them costs
    // nothing and guarantees no bright seam at the very edge.
    let x = aspect.max(0.1) + 0.05;
    let reach_x = REACH * aspect.max(0.1);

    // (x0, y0, x1, y1, which two corners are the *outer* edge)
    let bands: [(f32, f32, f32, f32, bool, bool); 4] = [
        // left, right: the gradient runs along x.
        (-x, -1.05, -x + reach_x, 1.05, true, false),
        (x - reach_x, -1.05, x, 1.05, true, true),
        // top, bottom: along y.
        (-x, 1.0 - REACH, x, 1.05, false, true),
        (-x, -1.05, x, -1.0 + REACH, false, false),
    ];
    for (x0, y0, x1, y1, along_x, flipped) in bands {
        let (near, far) = if flipped { (inner, outer) } else { (outer, inner) };
        // Corner order matches `push_quad`: (x0,y0) (x1,y0) (x1,y1)
        // twice more for the second triangle.
        let colour_at = |x: f32, y: f32| if along_x {
            if x == x0 { near } else { far }
        } else if y == y0 {
            near
        } else {
            far
        };
        for (px, py) in [
            (x0, y0),
            (x1, y0),
            (x1, y1),
            (x0, y0),
            (x1, y1),
            (x0, y1),
        ] {
            v.push(OverlayVertex {
                position: [px, py],
                color: colour_at(px, py),
            });
        }
    }
}

/// Which presentation mode to ask the surface for.
///
/// `AutoNoVsync` takes whatever the backend offers first, which need not
/// be the mode "vsync off" actually means: Mailbox still queues finished
/// frames and hands them on in order, where Immediate sends each one
/// straight out. Asking for Immediate by name says what is wanted rather
/// than hoping the automatic choice agrees.
///
/// **Measured neutral.** This was tried against the half of a frame that
/// goes into submit-and-present, on the theory that the queueing was
/// what cost it; present came out at 0.256-0.285 ms either way, against
/// 0.258-0.284 before. The fixed cost is the submission itself, not the
/// policy for handing frames to the compositor. Kept because it is the
/// more honest request, not because it made anything faster.
///
/// Falls back through Mailbox to whatever the surface will take.
fn choose_present_mode(vsync: bool, available: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    let supports = |mode| available.contains(&mode);
    if vsync {
        return wgpu::PresentMode::AutoVsync;
    }
    for mode in [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox] {
        if supports(mode) {
            return mode;
        }
    }
    wgpu::PresentMode::AutoNoVsync
}

/// The small texture the sky is drawn into, and the bind group that
/// reads it back. `None` at full scale, where the sky goes straight
/// into the frame as it always did.
///
/// See `sky_blit.wgsl` for why this exists at all.
fn create_sky_target(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    scale: u32,
) -> Option<(wgpu::TextureView, wgpu::BindGroup)> {
    if scale <= 1 {
        return None;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sky target"),
        size: wgpu::Extent3d {
            // At least one pixel each way: a window dragged to nothing
            // must not ask for a zero-sized texture.
            width: (config.width / scale).max(1),
            height: (config.height / scale).max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky blit bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Some((view, bind_group))
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d {
            width: config.width.max(1),
            height: config.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_globals_struct_stays_uniform_buffer_aligned() {
        // A uniform buffer needs 16-byte alignment; getting this wrong
        // shows up as garbled lighting rather than a clean error.
        assert_eq!(std::mem::size_of::<Globals>() % 16, 0);
    }

    #[test]
    fn a_healthy_player_gets_a_crosshair_and_nothing_else() {
        // Four arms, each with an outline behind it: eight quads. The
        // count matters less than what it is *not* -- a vignette on a
        // full health bar would be a frame round the picture.
        assert_eq!(overlay_vertices(None, 1.78, 0.0).len(), 8 * 6);
    }

    #[test]
    fn the_crosshair_leaves_the_middle_of_the_screen_alone() {
        // The centre is where the thing being aimed at is.
        let hole = overlay_vertices(None, 1.78, 0.0)
            .iter()
            .map(|v| v.position[0].abs().min(v.position[1].abs()))
            .fold(f32::MAX, f32::min);
        assert!(hole > 0.0, "the crosshair covers what it is aimed at");
    }

    #[test]
    fn being_hurt_darkens_the_edges_and_only_the_edges() {
        let hurt = overlay_vertices(None, 1.78, 1.0);
        assert!(hurt.len() > overlay_vertices(None, 1.78, 0.0).len(), "no vignette");
        assert!(hurt.len() <= MAX_OVERLAY_VERTICES, "the overlay outgrew its buffer");

        // Every part of it that is actually opaque has to be out at the
        // edge: a dark quad over the middle of the screen is a filter,
        // not a vignette.
        for vertex in hurt.iter().filter(|v| is_vignette(v) && v.color[3] > 0.2) {
            assert!(
                v_at_edge(vertex.position, 1.78),
                "something solid at {:?}",
                vertex.position
            );
        }
    }

    #[test]
    fn the_darkness_comes_on_with_the_damage() {
        let darkest = |hurt: f32| {
            overlay_vertices(None, 1.78, hurt)
                .iter()
                .filter(|v| is_vignette(v))
                .map(|v| v.color[3])
                .fold(0.0f32, f32::max)
        };
        assert_eq!(darkest(0.0), 0.0, "a healthy player sees a vignette");
        assert!(darkest(0.5) > 0.0);
        assert!(darkest(1.0) > darkest(0.5), "it does not deepen");
        assert!(darkest(1.0) < 1.0, "it goes fully opaque");
        // Nonsense from a malformed health message must not draw a black
        // screen.
        assert_eq!(darkest(f32::NAN), 0.0);
        assert_eq!(darkest(-5.0), 0.0);
        assert!(darkest(5.0) <= darkest(1.0));
    }

    /// The vignette is the only red thing the overlay draws -- the
    /// crosshair is white and its outline is black -- so its colour is
    /// what tells the two apart without counting vertices.
    fn is_vignette(v: &OverlayVertex) -> bool {
        v.color[0] > 0.1 && v.color[1] < 0.05
    }

    /// Whether a point is out in the band the vignette is allowed to
    /// cover, in the square space the overlay is authored in.
    fn v_at_edge(position: [f32; 2], aspect: f32) -> bool {
        position[0].abs() >= aspect * 0.36 || position[1].abs() >= 0.36
    }

    #[test]
    fn the_loading_screen_replaces_the_crosshair_and_fits_the_buffer() {
        let empty = overlay_vertices(Some(0.0), 1.78, 0.0);
        let half = overlay_vertices(Some(0.5), 1.78, 0.0);
        let full = overlay_vertices(Some(1.0), 1.78, 0.0);
        assert!(empty.len() < half.len(), "an empty bar draws no fill quad");
        assert_eq!(half.len(), full.len());
        assert!(
            full.len() <= MAX_OVERLAY_VERTICES,
            "overlay must fit its preallocated buffer"
        );
    }

    #[test]
    fn the_loading_bar_fill_grows_with_progress() {
        let right_edge = |p: f32| {
            overlay_vertices(Some(p), 1.78, 0.0)
                .iter()
                .map(|v| v.position[0])
                .fold(f32::MIN, f32::max)
        };
        // Compare the fill quad's extent at two progress values by
        // looking at the widest vertex inside the bar region.
        let quarter = overlay_vertices(Some(0.25), 1.78, 0.0);
        let three_quarters = overlay_vertices(Some(0.75), 1.78, 0.0);
        let fill_right = |v: &[OverlayVertex]| {
            v.iter()
                .filter(|vert| vert.color[1] > 0.7 && vert.color[0] < 0.6)
                .map(|vert| vert.position[0])
                .fold(f32::MIN, f32::max)
        };
        assert!(
            fill_right(&quarter) < fill_right(&three_quarters),
            "the bar must actually advance"
        );
        let _ = right_edge(1.0);
    }

    #[test]
    fn progress_outside_zero_to_one_does_not_overflow_the_bar() {
        let clamped = overlay_vertices(Some(5.0), 1.78, 0.0);
        let full = overlay_vertices(Some(1.0), 1.78, 0.0);
        assert_eq!(clamped.len(), full.len());
    }
}
