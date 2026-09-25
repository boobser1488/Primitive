//! The solid terrain drawn one way and then the other, and the two
//! pictures compared.
//!
//! ```text
//! cargo test -p primitive_client --lib prepass_repro
//! cargo test -p primitive_client --lib what_a_depth_prepass_costs \
//!     -- --ignored --nocapture
//! ```
//!
//! Two things live here, and they answer two different questions.
//!
//! **Does it draw the same world?** `PRIMITIVE_OPT_DEPTH_PREPASS` makes the
//! solid terrain go out twice -- depth with nothing read and nothing
//! written, then the same triangles shaded with `LessEqual` instead of
//! `Less`. That is only safe if the two draws compute the identical clip
//! position, to the last bit, because `LessEqual` against a depth a unit
//! away is a rejection and a rejection is a hole in the world. The test
//! renders a real patch of the benchmark world both ways, on one device
//! from one mesh, and **no pixel of terrain may come out as the clear
//! colour**: that assertion has no tolerance, because a hole is the only
//! failure this can have.
//!
//! What it does have a tolerance for is the thing the change actually
//! means. Two triangles at *exactly* the same depth -- which the mesher
//! makes wherever a merged rectangle and the quads closing a T-junction
//! overlap along a seam -- are settled by `Less` in favour of whichever
//! went first and by `LessEqual` in favour of whichever went last. On the
//! seat below that is 80 pixels of 921600, each a level or two of one
//! channel, and the budget is a tenth of a per cent. Neither answer is the
//! right one; it is the same surface fetched through two of its own quads.
//!
//! **What does it cost?** The ignored tool times both, on this device, and
//! prints the triangle count and the draw count beside the milliseconds.
//! A desktop's answer is not a phone's -- a tile-based GPU bills a triangle
//! for binning and keeps its depth buffer on the die -- but a prepass that
//! loses on a desktop with two thousand shader cores is not going to be
//! rescued by a phone with a quarter of them, and the tool is here so that
//! sentence is a measurement rather than an opinion.

use super::*;
use crate::engine::mesh::MeshBuffers;
use crate::engine::texture::TextureManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::types::ChunkPos;

/// Big enough that a triangle is smaller than a pixel, which is the
/// condition `lod.rs` measured the solid pass under, and small enough that
/// the readback is one buffer.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// Where the eye stands, and how far it sees.
///
/// **A seat with something in front of something else.** A prepass buys
/// back the shading of fragments that lose the depth test, so a floor
/// photographed from above -- where nothing is behind anything -- measures
/// the cost and none of the saving. This is the shore of the benchmark
/// world at head height looking along it: hills in front of hills, which
/// is what a player is looking at when the frame is slow.
const EYE: Vec3 = Vec3::new(8.0, 76.0, 8.0);
const YAW: f32 = -90.0;
const PITCH: f32 = -8.0;
const RADIUS: i32 = 6;

struct Scene {
    globals_bind: wgpu::BindGroup,
    texture_bind: wgpu::BindGroup,
    /// The game's own solid pipeline: `Less`, depth written, back faces
    /// culled.
    one_pass: wgpu::RenderPipeline,
    /// The prepass pair, built exactly as `LookPipelines` builds them:
    /// `vs_depth` into `fs_depth` with every colour channel masked off,
    /// then the ordinary shading with `LessEqual`.
    depth_only: wgpu::RenderPipeline,
    after_prepass: wgpu::RenderPipeline,
    chunks: Vec<Uploaded>,
    target: wgpu::Texture,
    depth: wgpu::Texture,
    triangles: u32,
}

struct Uploaded {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    instance: wgpu::Buffer,
    solid_indices: u32,
    distance_squared: f32,
}

/// The benchmark world's shore, generated, lit and meshed at full detail.
fn terrain(textures: &TextureManager, settings: &ClientSettings) -> Vec<(ChunkPos, MeshBuffers)> {
    let centre = ChunkPos::new(0, 0);
    let world = super::view_distance_repro::stream(centre, RADIUS);
    super::view_distance_repro::mesh_world(&world, &textures.face_layers(), settings, centre, false)
        .meshes
}

fn scene(device: &wgpu::Device, textures: &TextureManager, settings: &ClientSettings) -> Scene {
    use wgpu::util::DeviceExt;

    let mut camera = Camera::new(EYE.as_dvec3(), WIDTH as f32 / HEIGHT as f32);
    camera.yaw = YAW.to_radians();
    camera.pitch = PITCH.to_radians();
    camera.fov_y_radians = settings.fov_degrees.to_radians();
    let origin = Vec3::new(EYE.x.floor(), EYE.y.floor(), EYE.z.floor());
    let view_proj = camera.view_proj_about(origin);

    let mut globals: Globals = bytemuck::Zeroable::zeroed();
    globals.view_proj = view_proj.to_cols_array_2d();
    globals.inv_view_proj = view_proj.inverse().to_cols_array_2d();
    globals.camera_pos = [EYE.x - origin.x, EYE.y - origin.y, EYE.z - origin.z, 1.0];
    globals.sun = [-0.4, -0.8, -0.3, 1.0];
    globals.sun_color = [1.0, 1.0, 1.0, 1.0];
    globals.fill_color = [1.0, 1.0, 1.0, 1.0];
    globals.extra = [1.0, 0.45, 0.0, 1.0];
    globals.fog_params = [200.0, 400.0, settings.ambient_light, 1.0];
    globals.fog_color = [0.6, 0.7, 0.85, 1.0];
    globals.texture_params = [textures.resolution as f32, 1.0, 0.0, 0.0];

    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("prepass globals"),
        contents: bytemuck::bytes_of(&globals),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("prepass globals layout"),
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
        label: Some("prepass globals bind"),
        layout: &globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("prepass texture layout"),
        entries: &textures.split.layout_entries(),
    });
    let texture_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("prepass texture bind"),
        layout: &texture_layout,
        entries: &textures.bind_entries(&textures.sampler),
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("prepass shader"),
        source: wgpu::ShaderSource::Wgsl(textures.split.specialise(include_str!("shader.wgsl").into())),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("prepass layout"),
        bind_group_layouts: &[&globals_layout, &texture_layout],
        push_constant_ranges: &[],
    });
    // **One closure for all three pipelines**, so the only things that can
    // differ between them are the two the prepass is about: whether there
    // is a fragment stage, and which comparison the depth test uses. A
    // second descriptor written out by hand is a second place for the cull
    // mode to drift, and a picture drawn with a different cull mode would
    // fail this test for a reason that has nothing to do with prepasses.
    let pipeline = |label, shaded: bool, compare| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: if shaded { "vs_main" } else { "vs_depth" },
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            // The depth-only pipeline keeps a colour target with every
            // channel masked off, for the reason `fs_depth` in
            // shader.wgsl gives: wgpu refuses `fragment: None` against a
            // pass that has a colour attachment. Exactly what
            // `LookPipelines` builds.
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: if shaded { "fs_solid" } else { "fs_depth" },
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: shaded.then_some(wgpu::BlendState::REPLACE),
                    write_mask: if shaded {
                        wgpu::ColorWrites::ALL
                    } else {
                        wgpu::ColorWrites::empty()
                    },
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: compare,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        })
    };

    let meshes = terrain(textures, settings);
    let mut triangles = 0;
    let chunks: Vec<Uploaded> = meshes
        .iter()
        .filter(|(_, mesh)| mesh.solid_index_count > 0)
        .map(|(pos, mesh)| {
            triangles += mesh.solid_index_count / 3;
            let centre_x = (pos.x as f32 + 0.5) * 16.0;
            let centre_z = (pos.z as f32 + 0.5) * 16.0;
            let (dx, dz) = (centre_x - EYE.x, centre_z - EYE.z);
            Uploaded {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("prepass vertices"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("prepass indices"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                instance: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("prepass instance"),
                    contents: bytemuck::cast_slice(&[[
                        (pos.x * primitive_shared::types::CHUNK_SIZE_X as i32) as f32 - origin.x,
                        -origin.y,
                        (pos.z * primitive_shared::types::CHUNK_SIZE_Z as i32) as f32 - origin.z,
                        0.0,
                    ]]),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                solid_indices: mesh.solid_index_count,
                distance_squared: dx * dx + dz * dz,
            }
        })
        .collect();

    let size = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
    Scene {
        globals_bind,
        texture_bind,
        one_pass: pipeline("prepass control", true, wgpu::CompareFunction::Less),
        depth_only: pipeline("prepass depth only", false, wgpu::CompareFunction::Less),
        after_prepass: pipeline("prepass shaded", true, wgpu::CompareFunction::LessEqual),
        chunks,
        target: device.create_texture(&wgpu::TextureDescriptor {
            label: Some("prepass target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }),
        depth: device.create_texture(&wgpu::TextureDescriptor {
            label: Some("prepass depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }),
        triangles,
    }
}

/// One frame of `scene`, with the prepass or without it. `read` brings the
/// picture back; without it the frame is drawn and waited for, which is
/// what the timing loop wants.
fn frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    prepass: bool,
    read: bool,
) -> Option<image::RgbaImage> {
    let target_view = scene.target.create_view(&Default::default());
    let depth_view = scene.depth.create_view(&Default::default());
    // Near to far, exactly as `render` sorts it: the order is what makes
    // the depth test throw far fragments away cheaply, and a control drawn
    // in another order would be measuring the sort instead of the prepass.
    let mut order: Vec<&Uploaded> = scene.chunks.iter().collect();
    order.sort_by(|a, b| a.distance_squared.total_cmp(&b.distance_squared));

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("prepass pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        // Both draws inside one render pass, as the frame does it: a
        // second pass over the same attachments is a tile resolve and a
        // tile reload, which on the hardware this is for would be most of
        // what the prepass could save.
        let stages: &[&wgpu::RenderPipeline] = if prepass {
            &[&scene.depth_only, &scene.after_prepass]
        } else {
            &[&scene.one_pass]
        };
        for pipeline in stages {
            pass.set_pipeline(pipeline);
            // Both groups even for the depth-only draw, which reads
            // neither: the pipeline layout is the terrain's, and wgpu
            // requires every group the layout declares to be bound
            // whether the shader looks at it or not. The frame binds them
            // once above the prepass for the same reason.
            pass.set_bind_group(0, &scene.globals_bind, &[]);
            pass.set_bind_group(1, &scene.texture_bind, &[]);
            for mesh in &order {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, mesh.instance.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.solid_indices, 0, 0..1);
            }
        }
    }
    if !read {
        queue.submit(std::iter::once(encoder.finish()));
        device.poll(wgpu::Maintain::Wait);
        return None;
    }
    // 1280 * 4 is 5120, a multiple of the 256 a copy wants.
    let bytes_per_row = WIDTH * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("prepass readback"),
        size: (bytes_per_row * HEIGHT) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        scene.target.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().expect("map never completed").expect("map failed");
    let data = slice.get_mapped_range().to_vec();
    readback.unmap();
    Some(image::RgbaImage::from_raw(WIDTH, HEIGHT, data).expect("buffer is the right size"))
}

/// What the pass clears to, in linear light: a pale sky blue, the way the
/// frame clears to the fog colour. Nothing in this seat is anywhere near
/// it, which is what makes "is this pixel the clear colour?" the same
/// question as "is there a hole here?".
const CLEAR: wgpu::Color = wgpu::Color { r: 0.6, g: 0.7, b: 0.85, a: 1.0 };

/// Whether a pixel read back is the clear colour.
///
/// **Within three levels rather than exactly**, because the target is
/// `Rgba8UnormSrgb` and the clear is given in linear light: the driver
/// encodes it, and which way it rounds the last bit is its own business
/// and differs between them. Three is far tighter than the gap between
/// this sky and the darkest terrain in the seat, which is a hundred and
/// fifty levels.
fn is_clear(pixel: [u8; 4]) -> bool {
    let encoded = |linear: f64| -> f64 {
        let s = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        s * 255.0
    };
    [CLEAR.r, CLEAR.g, CLEAR.b]
        .into_iter()
        .zip(pixel)
        .all(|(linear, got)| (encoded(linear) - f64::from(got)).abs() <= 3.0)
}

/// The settings the tool draws at: the defaults, with the fog held back so
/// the far hills are terrain rather than fog colour -- a picture that is
/// mostly fog compares equal whatever the depth test did.
fn settings() -> ClientSettings {
    ClientSettings { anisotropy: 1, render_distance_chunks: RADIUS, ..Default::default() }
}

#[test]
fn a_depth_prepass_draws_the_same_terrain_as_one_pass() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU: skipped");
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = settings();
    let Ok(textures) = TextureManager::load(device, queue, assets, settings.anisotropy) else {
        println!("no assets: skipped");
        return;
    };
    let scene = scene(device, &textures, &settings);
    assert!(scene.triangles > 10_000, "the seat drew almost nothing: {} triangles", scene.triangles);

    let control = frame(device, queue, &scene, false, true).expect("control frame");
    let prepassed = frame(device, queue, &scene, true, true).expect("prepassed frame");

    let differing: Vec<(u32, u32, [u8; 4], [u8; 4])> = control
        .enumerate_pixels()
        .zip(prepassed.pixels())
        .filter(|((_, _, a), b)| a != b)
        .map(|((x, y, a), b)| (x, y, a.0, b.0))
        .collect();
    // Where on the screen a difference is, and what the two colours were,
    // is the first thing anyone chasing one will want -- so both
    // assertions below print it rather than a bare count.
    let shown = |take: usize| -> String {
        differing
            .iter()
            .take(take)
            .map(|(x, y, a, b)| format!("({x},{y}) {a:?} -> {b:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // **The failure is a hole, and nothing else is.** If the two draws
    // ever computed a different clip position for the same vertex, the
    // `LessEqual` in the second would reject the whole face and what would
    // be drawn there is the clear colour. So that is the assertion with no
    // tolerance at all: not one pixel of terrain may turn into sky.
    let holes = differing.iter().filter(|(_, _, _, after)| is_clear(*after)).count();
    assert_eq!(holes, 0, "the prepass opened {holes} holes in the terrain; {}", shown(8));

    // **What is allowed is a tie broken the other way**, and it has to be
    // allowed because it is what the change means. Two triangles at
    // *exactly* the same depth -- which the mesher produces wherever a
    // merged rectangle and the quads closing a T-junction overlap along a
    // seam (`mesh::MERGE_COPLANAR_FACES`) -- are settled by `Less` in
    // favour of whichever was drawn first and by `LessEqual` in favour of
    // whichever was drawn last. Neither is more correct; they are the same
    // surface seen through two of its own quads, and the texel each
    // fetches differs by the fraction of a cell the seam sits at.
    //
    // Measured on the shore of the benchmark world at 1280x720: 80 pixels
    // of 921600, every one of them a step of one or two levels except at a
    // handful of seams where two materials meet. The budget is a tenth of
    // a per cent, which is an order of magnitude over that -- room for
    // another driver's rasteriser to disagree about a seam, and nowhere
    // near enough to hide a face.
    let budget = (WIDTH * HEIGHT) as usize / 1000;
    assert!(
        differing.len() <= budget,
        "the prepass changed {} pixels of {}, over the {budget} a seam is worth; {}",
        differing.len(),
        WIDTH * HEIGHT,
        shown(8)
    );
}

/// What the prepass costs on this device, in milliseconds a frame.
///
/// Ignored because it is a measurement and not a property: the number is
/// this machine's, and a test that asserted on it would go red on the next
/// one. Run it by name with `--nocapture`.
#[test]
#[ignore]
fn what_a_depth_prepass_costs() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU: skipped");
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = settings();
    let textures =
        TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let scene = scene(device, &textures, &settings);
    let rounds: usize = std::env::var("PREPASS_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    println!(
        "{WIDTH}x{HEIGHT}, {} chunks, {} solid triangles, {} draws a frame one-pass",
        scene.chunks.len(),
        scene.triangles,
        scene.chunks.len(),
    );
    for (label, prepass) in [("one pass", false), ("prepass", true)] {
        // A warm frame first: the first one of a pipeline pays for the
        // driver compiling it, which is not what is being measured.
        frame(device, queue, &scene, prepass, false);
        let started = std::time::Instant::now();
        for _ in 0..rounds {
            frame(device, queue, &scene, prepass, false);
        }
        let each = started.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
        println!("  {label:>8}: {each:.3} ms a frame over {rounds}");
    }
}
