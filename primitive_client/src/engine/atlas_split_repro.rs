//! The atlas split across arrays, photographed beside the atlas in one.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/primitive_client/shots-review/atlas-split \
//!     cargo test -p primitive_client --lib what_a_split_atlas_draws -- --ignored --nocapture
//! ```
//!
//! A desktop grants 2048 layers an array and never splits, so the split a
//! GLES phone draws with cannot be seen on it -- unless it is asked for.
//! `TextureManager::load_split_at` loads the same pack as though the device
//! held 256 layers an array, and this draws one floor both ways: every cell
//! a different picture, the pictures at the edges of the arrays (255, 256,
//! 511, 512 and the last) among them, through `fs_solid` and `fs_cutout`
//! (which also reads `textureLoad`). The two have to be the same picture to
//! the byte, and the tool says how many pixels are not.
//!
//! It also times both: the frame through each, on this device, drawn
//! `ATLAS_SPLIT_FRAMES` times (default 300) at 1920x1080, and the memory each
//! atlas takes. **Give `GPU_REPRO_DIR` an absolute directory.**

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood, Vertex};
use crate::engine::texture::{TextureManager, MIN_PER_ARRAY};
use crate::logic::chunk_manager::ChunkManager;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_DIRT, BLOCK_STONE, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

/// A floor of 16x16 cells, checkered so no two neighbours merge, each cell
/// then repainted with its own layer -- the layers the split has edges at
/// first, the rest spread over the whole atlas.
fn floor(textures: &TextureManager) -> (Vec<Vertex>, Vec<u32>) {
    let layer_count = textures.layer_count;
    let pos = ChunkPos::new(0, 0);
    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
    for z in 0..CHUNK_SIZE_Z {
        for x in 0..CHUNK_SIZE_X {
            blocks[Chunk::index(x, 0, z)] = if (x + z) % 2 == 0 { BLOCK_STONE } else { BLOCK_DIRT };
        }
    }
    let mut chunks = ChunkManager::new(4);
    chunks.insert(Chunk { pos, blocks });
    let mut light = LightMap::new();
    light.load_chunk(&chunks, pos);
    let mut cache = Neighbourhood::default();
    cache.fill(pos, &chunks, &light);
    let mut mesh = MeshBuffers::default();
    build_mesh(pos, &cache, &textures.face_layers(), &primitive_shared::worldgen::WorldGen::new(0), &mut mesh);

    let last = layer_count - 1;
    let mut chosen: Vec<u32> = [0, 1, 254, 255, 256, 257, 510, 511, 512, 513, last - 1, last]
        .into_iter()
        .filter(|&l| l < layer_count)
        .collect();
    let spread = 256 - chosen.len() as u32;
    chosen.extend((0..spread).map(|i| i * last / spread.max(1)));
    let cell_layer = |x: f32, z: f32| {
        let (cx, cz) = ((x.floor() as i32).clamp(0, 15) as usize, (z.floor() as i32).clamp(0, 15) as usize);
        chosen[cz * 16 + cx]
    };
    let mut vertices = mesh.vertices.clone();
    for quad in vertices.chunks_exact_mut(4) {
        let cx = quad.iter().map(|v| v.position[0]).sum::<f32>() / 4.0;
        let cz = quad.iter().map(|v| v.position[2]).sum::<f32>() / 4.0;
        let layer = cell_layer(cx, cz);
        for v in quad.iter_mut() {
            let fine = v.uv & crate::engine::mesh::FINE_UV_BIT != 0;
            let uv = v.uv();
            let mut repainted = Vertex::tinted(v.position, if fine { [0.0; 2] } else { uv }, layer, v.light(), 0);
            repainted.packed |= v.packed & crate::engine::mesh::MOTTLED_BIT;
            if fine {
                repainted = repainted.with_fine_uv(uv);
            }
            *v = repainted;
        }
    }
    (vertices, mesh.indices[..mesh.leaf_end as usize].to_vec())
}

struct Scene {
    globals_bind: wgpu::BindGroup,
    texture_bind: wgpu::BindGroup,
    solid: wgpu::RenderPipeline,
    cutout: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instance: wgpu::Buffer,
    target: wgpu::Texture,
    depth: wgpu::Texture,
}

fn scene(device: &wgpu::Device, textures: &TextureManager, vertices: &[Vertex], indices: &[u32]) -> Scene {
    use wgpu::util::DeviceExt;
    let eye = Vec3::new(8.0, 9.0, 17.5);
    let at = Vec3::new(8.0, 0.0, 6.5);
    let view = glam::Mat4::look_at_rh(eye, at, Vec3::Y);
    let proj = glam::Mat4::perspective_rh(80f32.to_radians(), WIDTH as f32 / HEIGHT as f32, 0.05, 100.0);
    let mut globals: Globals = bytemuck::Zeroable::zeroed();
    globals.view_proj = (proj * view).to_cols_array_2d();
    globals.inv_view_proj = (proj * view).inverse().to_cols_array_2d();
    globals.camera_pos = [eye.x, eye.y, eye.z, 1.0];
    globals.sun = [-0.4, -0.8, -0.3, 1.0];
    globals.sun_color = [1.0, 1.0, 1.0, 1.0];
    globals.fill_color = [1.0, 1.0, 1.0, 1.0];
    globals.extra = [1.0, 0.45, 0.0, 0.0];
    globals.fog_params = [1000.0, 2000.0, crate::settings::ClientSettings::default().ambient_light, 1.0];
    globals.texture_params = [textures.resolution as f32, 0.0, 0.0, 0.0];
    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("atlas split globals"),
        contents: bytemuck::bytes_of(&globals),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas split globals layout"),
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
        label: Some("atlas split globals bind"),
        layout: &globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() }],
    });
    // The game's own layout and shader for this split: exactly what
    // `GraphicsState::new` builds from `textures.split`.
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas split texture layout"),
        entries: &textures.split.layout_entries(),
    });
    let texture_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("atlas split texture bind"),
        layout: &texture_layout,
        entries: &textures.bind_entries(&textures.sampler),
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("atlas split shader"),
        source: wgpu::ShaderSource::Wgsl(textures.split.specialise(include_str!("shader.wgsl").into())),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("atlas split layout"),
        bind_group_layouts: &[&globals_layout, &texture_layout],
        push_constant_ranges: &[],
    });
    let pipeline = |entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("atlas split pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: entry,
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
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
        })
    };
    let size = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
    Scene {
        globals_bind,
        texture_bind,
        solid: pipeline("fs_solid"),
        cutout: pipeline("fs_cutout"),
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("atlas split vertices"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("atlas split indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: indices.len() as u32,
        instance: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("atlas split instance"),
            contents: bytemuck::cast_slice(&[[0.0f32; 4]]),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        target: device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas split target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }),
        depth: device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas split depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }),
    }
}

/// One frame of `scene`, solid or cutout; with `read` the picture comes back.
fn frame(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, cutout: bool, read: bool) -> Option<image::RgbaImage> {
    let target_view = scene.target.create_view(&Default::default());
    let depth_view = scene.depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("atlas split pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(if cutout { &scene.cutout } else { &scene.solid });
        pass.set_bind_group(0, &scene.globals_bind, &[]);
        pass.set_bind_group(1, &scene.texture_bind, &[]);
        pass.set_vertex_buffer(0, scene.vertices.slice(..));
        pass.set_vertex_buffer(1, scene.instance.slice(..));
        pass.set_index_buffer(scene.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..scene.index_count, 0, 0..1);
    }
    if !read {
        queue.submit(std::iter::once(encoder.finish()));
        device.poll(wgpu::Maintain::Wait);
        return None;
    }
    let bytes_per_row = WIDTH * 4; // 7680, a multiple of 256
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("atlas split readback"),
        size: (bytes_per_row * HEIGHT) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        scene.target.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(HEIGHT) },
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

/// What an atlas costs the card: every array, every level of its mips.
fn atlas_bytes(textures: &TextureManager) -> u64 {
    let r = textures.resolution as u64;
    let per_layer: u64 = (0..=r.max(1).ilog2()).map(|mip| (r >> mip).max(1).pow(2) * 4).sum();
    textures.layer_count as u64 * per_layer
}

#[test]
#[ignore = "a tool: needs a GPU; draws the atlas split at 256 beside the atlas whole"]
fn what_a_split_atlas_draws() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let frames: usize = std::env::var("ATLAS_SPLIT_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(300);
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));

    let whole = TextureManager::load(device, queue, assets, 16).expect("textures load");
    let split = TextureManager::load_split_at(device, queue, assets, 16, MIN_PER_ARRAY).expect("textures load split");
    assert_eq!(whole.split.arrays, 1, "this device splits the atlas on its own: {:?}", whole.split);
    assert!(split.split.arrays > 1, "{} layers did not split at {MIN_PER_ARRAY}", split.layer_count);
    assert_eq!(whole.layer_count, split.layer_count);
    println!(
        "atlas: {} layers at {}px; whole {:?}, split {:?} ({} bytes a vertex)",
        whole.layer_count,
        whole.resolution,
        whole.split,
        split.split,
        std::mem::size_of::<Vertex>()
    );
    for array in 0..split.split.arrays {
        println!("  array {array}: {} layers", split.split.depth(array, split.layer_count));
    }
    println!(
        "atlas memory: whole {:.2} MB, split {:.2} MB",
        atlas_bytes(&whole) as f64 / 1048576.0,
        atlas_bytes(&split) as f64 / 1048576.0
    );

    // The same vertices for both: the layer is one number above the shader.
    let (vertices, indices) = floor(&whole);
    let one = scene(device, &whole, &vertices, &indices);
    let many = scene(device, &split, &vertices, &indices);

    let mut differing_total = 0;
    for (pass, cutout) in [("solid", false), ("cutout", true)] {
        let a = frame(device, queue, &one, cutout, true).expect("picture");
        let b = frame(device, queue, &many, cutout, true).expect("picture");
        let mut diff = image::RgbaImage::new(WIDTH, HEIGHT);
        let mut differing = 0;
        let mut worst = 0u8;
        for (x, y, p) in a.enumerate_pixels() {
            let q = b.get_pixel(x, y);
            let d = (0..3).map(|c| p.0[c].abs_diff(q.0[c])).max().unwrap_or(0);
            worst = worst.max(d);
            if d > 0 {
                differing += 1;
            }
            let shown = d.saturating_mul(16);
            diff.put_pixel(x, y, image::Rgba([shown, shown, shown, 255]));
        }
        let lit = a.pixels().filter(|p| p.0[0] > 8 || p.0[1] > 8 || p.0[2] > 8).count();
        println!("{pass}: {differing} of {} pixels differ (worst {worst}); {lit} lit", WIDTH * HEIGHT);
        assert!(lit > (WIDTH * HEIGHT / 4) as usize, "{pass}: the floor did not fill the picture");
        a.save(format!("{out}/atlas_{pass}_one_array.png")).expect("save");
        b.save(format!("{out}/atlas_{pass}_split_256.png")).expect("save");
        diff.save(format!("{out}/atlas_{pass}_difference_x16.png")).expect("save");
        differing_total += differing;
    }

    // The frame, both ways, alternated so drift in the machine lands on both.
    let mut times: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    for _ in 0..10 {
        frame(device, queue, &one, false, false);
        frame(device, queue, &many, false, false);
    }
    for _ in 0..frames {
        for (i, s) in [&one, &many].into_iter().enumerate() {
            let start = std::time::Instant::now();
            frame(device, queue, s, false, false);
            times[i].push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    for (name, t) in ["one array", "split at 256"].iter().zip(times.iter_mut()) {
        t.sort_by(|a, b| a.partial_cmp(b).expect("a time is a number"));
        let median = t[t.len() / 2];
        let p95 = t[t.len() * 95 / 100];
        let mean = t.iter().sum::<f64>() / t.len() as f64;
        println!("frame, {name}: mean {mean:.3} ms, median {median:.3} ms, p95 {p95:.3} ms over {frames}");
    }
    println!("pictures in {out}");
    assert_eq!(differing_total, 0, "the split atlas drew a different picture from the whole one");
}
