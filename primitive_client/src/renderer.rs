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
use wgpu::util::DeviceExt;
use winit::window::Window;

use primitive_shared::types::ChunkPos;

use crate::camera::Camera;
use crate::frustum::Frustum;
use crate::hotbar::{HotbarVertex, MAX_HOTBAR_VERTICES};
use crate::mesh::Vertex;
use crate::remote_players::ActorVertex;
use crate::texture::TextureManager;

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

pub struct GpuMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    /// Where the three ranges end: `0..solid_indices` is the solid pass,
    /// `solid_indices..cutout_end` the cutout pass, and the rest is
    /// blended. See `mesh::MeshBuffers` for why they are split.
    pub solid_indices: u32,
    pub cutout_end: u32,
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
    pub ambient: f32,
    pub block_light_boost: f32,
    pub ao_strength: f32,
    pub fog_enabled: bool,
    pub underwater: bool,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self {
            sun_direction: Vec3::new(0.0, -1.0, 0.0),
            sun_intensity: 1.0,
            fog_color: Vec3::new(0.53, 0.80, 0.92),
            fog_start: 40.0,
            fog_end: 120.0,
            ambient: 0.06,
            block_light_boost: 1.0,
            ao_strength: 0.45,
            fog_enabled: true,
            underwater: false,
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
    /// Same shader and vertex layout as `chunk_pipeline`, but blended and
    /// with depth writes off -- the second pass, for water.
    transparent_pipeline: wgpu::RenderPipeline,
    actor_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_buffer: wgpu::Buffer,
    hotbar_pipeline: wgpu::RenderPipeline,
    hotbar_buffer: wgpu::Buffer,
    /// Vertices the hotbar/UI buffer can currently hold. The buffer
    /// grows when a bigger screen needs it -- the hotbar is a few
    /// hundred vertices, but the server browser draws text as one quad
    /// per lit font pixel and runs to tens of thousands.
    hotbar_capacity: usize,
    depth_view: wgpu::TextureView,

    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,

    /// Visible chunks with their squared distance, rebuilt and sorted
    /// every frame. A field so the allocation happens once rather than
    /// once a frame.
    draw_order: Vec<(f32, ChunkPos)>,

    pub textures: TextureManager,
    pub draw_calls_last_frame: u32,
    pub chunks_culled_last_frame: usize,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl GraphicsState {
    pub async fn new(window: Arc<Window>, assets_dir: &Path, vsync: bool) -> anyhow::Result<Self> {
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
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await?;

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
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
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

        let textures = TextureManager::load(&device, &queue, assets_dir)?;

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

        let hotbar_capacity = MAX_HOTBAR_VERTICES;
        let hotbar_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hotbar/UI vertex buffer"),
            size: (hotbar_capacity * std::mem::size_of::<HotbarVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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
            transparent_pipeline,
            actor_pipeline,
            overlay_pipeline,
            overlay_buffer,
            hotbar_pipeline,
            hotbar_buffer,
            hotbar_capacity,
            depth_view,
            globals_buffer,
            globals_bind_group,
            texture_bind_group,
            draw_order: Vec::new(),
            textures,
            draw_calls_last_frame: 0,
            chunks_culled_last_frame: 0,
        })
    }

    pub fn aspect(&self) -> f32 {
        self.config.width.max(1) as f32 / self.config.height.max(1) as f32
    }

    /// Switches vsync on or off without restarting.
    ///
    /// Reconfiguring the surface is all it takes, and it is what makes
    /// the setting worth having in the game at all -- a vsync toggle you
    /// have to restart to evaluate is one nobody uses. No-ops when the
    /// mode is already what was asked for, since reconfiguring drops the
    /// swapchain and blinks the window.
    pub fn set_vsync(&mut self, vsync: bool) {
        let wanted = if vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
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

    /// Uploads a chunk, preserving where its opaque triangles end and its
    /// blended ones begin.
    pub fn upload_chunk_mesh(&self, buffers: &crate::mesh::MeshBuffers) -> GpuMesh {
        self.upload_split_mesh(
            &buffers.vertices,
            &buffers.indices,
            buffers.solid_index_count,
            buffers.cutout_end,
        )
    }

    fn upload_split_mesh<V: Pod>(
        &self,
        vertices: &[V],
        indices: &[u32],
        solid_indices: u32,
        cutout_end: u32,
    ) -> GpuMesh {
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertex buffer"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        GpuMesh {
            vertex_buffer,
            index_buffer,
            num_indices: indices.len() as u32,
            solid_indices: solid_indices.min(indices.len() as u32),
            cutout_end: cutout_end.min(indices.len() as u32),
        }
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        params: &FrameParams,
        chunk_meshes: &HashMap<ChunkPos, GpuMesh>,
        actor_mesh: Option<&DynamicMesh>,
        entity_mesh: Option<&DynamicMesh>,
        loading: Option<f32>,
        hotbar: &[HotbarVertex],
    ) -> Result<(), wgpu::SurfaceError> {
        let overlay = overlay_vertices(loading);
        self.queue
            .write_buffer(&self.overlay_buffer, 0, bytemuck::cast_slice(&overlay));
        if !hotbar.is_empty() {
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
            self.queue
                .write_buffer(&self.hotbar_buffer, 0, bytemuck::cast_slice(hotbar));
        }

        let globals = Globals {
            view_proj: camera.view_proj().to_cols_array_2d(),
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
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let output = self.surface.get_current_texture()?;
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
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.chunk_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.texture_bind_group, &[]);
            // Only draw what the camera can actually see. At render
            // distance 8 that's typically a third of the loaded chunks;
            // the rest used to be transformed and clipped for nothing.
            let frustum = Frustum::from_view_proj(camera.view_proj());

            // Everything that survives culling, with its squared distance
            // from the camera. Collected once and used by both passes:
            // frustum-testing every chunk again for the second one would
            // be repeat work, and the list has to be sorted anyway.
            //
            // A field rather than a local, so the allocation happens once
            // in the life of the process instead of once a frame.
            self.draw_order.clear();
            for (pos, gpu_mesh) in chunk_meshes.iter() {
                if gpu_mesh.num_indices == 0 || !frustum.contains_chunk(*pos) {
                    continue;
                }
                let centre_x = (pos.x as f32 + 0.5) * 16.0;
                let centre_z = (pos.z as f32 + 0.5) * 16.0;
                let dx = centre_x - camera.position.x;
                let dz = centre_z - camera.position.z;
                self.draw_order.push((dx * dx + dz * dz, *pos));
            }
            self.chunks_culled_last_frame =
                chunk_meshes.len().saturating_sub(self.draw_order.len());

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
            self.draw_order.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

            for (_, pos) in &self.draw_order {
                let Some(gpu_mesh) = chunk_meshes.get(pos) else {
                    continue;
                };
                if gpu_mesh.solid_indices == 0 {
                    continue;
                }
                pass.set_vertex_buffer(0, gpu_mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(gpu_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu_mesh.solid_indices, 0, 0..1);
                draw_calls += 1;
            }

            // Leaves, still near to far: they write depth like the solid
            // pass, so the same ordering helps, and by now the solid
            // terrain has already filled the depth buffer in front of
            // most of them.
            let mut bound_cutout = false;
            for (_, pos) in &self.draw_order {
                let Some(mesh) = chunk_meshes.get(pos) else {
                    continue;
                };
                if mesh.cutout_end <= mesh.solid_indices {
                    continue; // no leaves in this chunk
                }
                if !bound_cutout {
                    pass.set_pipeline(&self.cutout_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    bound_cutout = true;
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.solid_indices..mesh.cutout_end, 0, 0..1);
                draw_calls += 1;
            }

            // Anything after the cutout pass has to rebind the solid
            // pipeline before drawing with it again.
            if bound_cutout {
                pass.set_pipeline(&self.chunk_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
            }

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
            for (_, pos) in self.draw_order.iter().rev() {
                let Some(mesh) = chunk_meshes.get(pos) else {
                    continue;
                };
                if mesh.cutout_end >= mesh.num_indices {
                    continue; // no water in this chunk
                }
                if !bound_transparent {
                    pass.set_pipeline(&self.transparent_pipeline);
                    pass.set_bind_group(0, &self.globals_bind_group, &[]);
                    pass.set_bind_group(1, &self.texture_bind_group, &[]);
                    bound_transparent = true;
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(mesh.cutout_end..mesh.num_indices, 0, 0..1);
                draw_calls += 1;
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
            if !hotbar.is_empty() {
                pass.set_pipeline(&self.hotbar_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_bind_group(1, &self.texture_bind_group, &[]);
                pass.set_vertex_buffer(0, self.hotbar_buffer.slice(..));
                pass.draw(0..hotbar.len() as u32, 0..1);
                draw_calls += 1;
            }
        }

        self.draw_calls_last_frame = draw_calls;
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
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

/// Enough room for the crosshair plus the loading screen's backdrop,
/// bar frame and fill.
const MAX_OVERLAY_VERTICES: usize = 64;

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
pub fn overlay_vertices(loading: Option<f32>) -> Vec<OverlayVertex> {
    let mut v = Vec::with_capacity(MAX_OVERLAY_VERTICES);

    if let Some(progress) = loading {
        let progress = progress.clamp(0.0, 1.0);
        // Backdrop, drawn wide enough to cover the screen after the
        // aspect divide in the vertex shader.
        push_quad(&mut v, -4.0, -4.0, 4.0, 4.0, [0.02, 0.03, 0.05, 0.72]);

        const HALF_W: f32 = 0.34;
        const HALF_H: f32 = 0.022;
        const BORDER: f32 = 0.006;
        // Frame.
        push_quad(
            &mut v,
            -HALF_W - BORDER,
            -HALF_H - BORDER,
            HALF_W + BORDER,
            HALF_H + BORDER,
            [0.85, 0.88, 0.92, 0.85],
        );
        // Trough.
        push_quad(&mut v, -HALF_W, -HALF_H, HALF_W, HALF_H, [0.06, 0.08, 0.11, 0.95]);
        // Fill.
        if progress > 0.0 {
            let right = -HALF_W + 2.0 * HALF_W * progress;
            push_quad(&mut v, -HALF_W, -HALF_H, right, HALF_H, [0.42, 0.78, 0.45, 1.0]);
        }
        return v;
    }

    const ARM: f32 = 0.018;
    const THICK: f32 = 0.0028;
    const CROSSHAIR: [f32; 4] = [1.0, 1.0, 1.0, 0.85];
    push_quad(&mut v, -ARM, -THICK, ARM, THICK, CROSSHAIR);
    push_quad(&mut v, -THICK, -ARM, THICK, ARM, CROSSHAIR);
    v
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
    fn crosshair_is_two_complete_quads() {
        assert_eq!(overlay_vertices(None).len(), 12);
    }

    #[test]
    fn the_loading_screen_replaces_the_crosshair_and_fits_the_buffer() {
        let empty = overlay_vertices(Some(0.0));
        let half = overlay_vertices(Some(0.5));
        let full = overlay_vertices(Some(1.0));
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
            overlay_vertices(Some(p))
                .iter()
                .map(|v| v.position[0])
                .fold(f32::MIN, f32::max)
        };
        // Compare the fill quad's extent at two progress values by
        // looking at the widest vertex inside the bar region.
        let quarter = overlay_vertices(Some(0.25));
        let three_quarters = overlay_vertices(Some(0.75));
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
        let clamped = overlay_vertices(Some(5.0));
        let full = overlay_vertices(Some(1.0));
        assert_eq!(clamped.len(), full.len());
    }
}
