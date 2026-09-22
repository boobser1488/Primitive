//! A mountain at every detail level, photographed through the real shader.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/primitive_client/shots-review/mountain-lod \
//!     cargo test -p primitive_client --release --lib \
//!     what_a_mountain_looks_like_at_every_detail_level -- --ignored --nocapture
//! ```
//!
//! Written for a report of pale streaks running along the terraces of a
//! grey cliff and bare trunks standing on the rock, which arrived the day
//! a mountain started leaving full detail at half the setting
//! (`lod::band_start`) -- so the question it answers is which of the
//! levels draws what the player saw, from where the player stood.
//!
//! **The backdrop is magenta and the fog is off** in the pictures that
//! judge holes. A streak the colour of the sky could be sky through a
//! crack, snow in shade or the fog, and a magenta pixel with terrain above
//! it in the same column can only be the first; the tool counts those.
//! `*_fog.png` puts the sky colour and the player's fog back, for putting
//! beside the screenshot.
//!
//! Seed 32 (`saves/mountain`), the default detail setting (ten chunks,
//! normal), the player's field of view (90) and anisotropy (16), levels
//! chosen per chunk the way `dispatch_meshing` chooses them.
//!
//! Environment, all optional:
//!
//! * `LOD_REPRO_MODES=fine,game,1,2` -- which levels to mesh the scene at;
//!   `game` is `band_start` + `level_at` from the eye's chunk.
//! * `LOD_REPRO_VIEWS="name:ex,ey,ez:lx,ly,lz;..."` -- extra cameras, in
//!   world coordinates.
//! * `LOD_REPRO_ONLY=<substring>` -- only the views whose name holds it.
//! * `LOD_REPRO_SPAN=<chunks>` and `LOD_REPRO_CENTRE="x,z"` -- the scene.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the
//! crate's own directory, and a relative one lands there.

use super::*;
use crate::engine::lod::{band_start, coarsen, level_at, Quality};
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood, Vertex};
use crate::logic::chunk_manager::ChunkManager;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{block_kind, block_name, is_leafy, BLOCK_LOG, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

const SEED: u32 = 32;
/// `settings::default_lod_distance`: the player's settings file has no
/// detail entry, so the player runs the default.
const LOD_SETTING: i32 = 10;
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
/// The player's `fov_degrees`.
const FOV: f32 = 90.0;
/// The player's render distance, 24 chunks, for the fog.
const VIEW_BLOCKS: f32 = 24.0 * 16.0;
const SKY: [f32; 3] = [0.62, 0.72, 0.86];

pub(super) struct Scene {
    generator: WorldGen,
    chunks: ChunkManager,
    light: LightMap,
    first: ChunkPos,
    span: i32,
}

impl Scene {
    pub(super) fn corner(&self) -> Vec3 {
        Vec3::new((self.first.x * CHUNK_SIZE_X as i32) as f32, 0.0, (self.first.z * CHUNK_SIZE_Z as i32) as f32)
    }
}

/// What level each chunk is meshed at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Levels {
    Fine,
    /// As the game would pick it for a player standing in this chunk.
    Game(ChunkPos),
    All(u8),
}

pub(super) struct Meshed {
    vertices: Vec<Vertex>,
    solid: Vec<u32>,
    cutout: Vec<u32>,
}

fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let work = &work;
        let handles: Vec<_> = items
            .chunks(per)
            .map(|batch| scope.spawn(move || batch.iter().map(work).collect::<Vec<R>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

pub(super) fn build_scene(centre: (i32, i32), span: i32) -> Scene {
    let generator = WorldGen::new(SEED);
    let first = ChunkPos::new(
        centre.0.div_euclid(CHUNK_SIZE_X as i32) - span / 2,
        centre.1.div_euclid(CHUNK_SIZE_Z as i32) - span / 2,
    );
    let positions: Vec<ChunkPos> = (0..span)
        .flat_map(|dz| (0..span).map(move |dx| ChunkPos::new(first.x + dx, first.z + dz)))
        .collect();
    let generated = in_parallel(&positions, |&pos| generator.generate_chunk(pos));
    let isolated = in_parallel(&generated, |chunk| primitive_shared::lighting::compute_isolated(&chunk.blocks));
    let mut chunks = ChunkManager::new(256);
    for chunk in generated {
        chunks.insert(chunk);
    }
    let mut light = LightMap::new();
    for (pos, data) in positions.iter().zip(isolated) {
        light.insert_precomputed(&chunks, *pos, data);
    }
    Scene { generator, chunks, light, first, span }
}

fn chunk_distance(a: ChunkPos, b: ChunkPos) -> f32 {
    let (dx, dz) = ((a.x - b.x) as f32, (a.z - b.z) as f32);
    (dx * dx + dz * dz).sqrt()
}

/// The scene's chunks less its outer ring (whose neighbours were never
/// generated), meshed at `levels` and moved into scene coordinates.
pub(super) fn mesh_scene(scene: &Scene, layers: &crate::engine::texture::FaceLayers, levels: Levels) -> Meshed {
    let positions: Vec<ChunkPos> = (1..scene.span - 1)
        .flat_map(|dz| (1..scene.span - 1).map(move |dx| ChunkPos::new(scene.first.x + dx, scene.first.z + dz)))
        .collect();
    let built = in_parallel(&positions, |&pos| {
        let mut cache = Box::<Neighbourhood>::default();
        cache.fill(pos, &scene.chunks, &scene.light);
        let level = match levels {
            Levels::Fine => 0,
            Levels::All(level) => level,
            Levels::Game(eye) => level_at(chunk_distance(pos, eye), band_start(LOD_SETTING, cache.ceiling()), 0),
        };
        coarsen(&mut cache, level, Quality::Normal);
        let mut out = Box::<MeshBuffers>::default();
        build_mesh(pos, &cache, layers, &scene.generator, &mut out);
        (pos, out)
    });
    let mut meshed = Meshed { vertices: Vec::new(), solid: Vec::new(), cutout: Vec::new() };
    for (pos, out) in built {
        let base = meshed.vertices.len() as u32;
        let (dx, dz) = (
            ((pos.x - scene.first.x) * CHUNK_SIZE_X as i32) as f32,
            ((pos.z - scene.first.z) * CHUNK_SIZE_Z as i32) as f32,
        );
        meshed.vertices.extend(out.vertices.iter().map(|v| {
            let mut v = *v;
            v.position[0] += dx;
            v.position[2] += dz;
            v
        }));
        let solid = out.solid_index_count as usize;
        meshed.solid.extend(out.indices[..solid].iter().map(|i| i + base));
        meshed.cutout.extend(out.indices[solid..out.sprite_end as usize].iter().map(|i| i + base));
    }
    meshed
}

/// One frame of `meshed` from `eye`, the solid pass culled and the cutout
/// pass not, as `GraphicsState::new` builds them.
pub(super) fn shoot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    textures: &TextureManager,
    meshed: &Meshed,
    eye: Vec3,
    at: Vec3,
    fog: bool,
) -> image::RgbaImage {
    use wgpu::util::DeviceExt;
    let view = glam::Mat4::look_at_rh(eye, at, Vec3::Y);
    let proj = glam::Mat4::perspective_rh(FOV.to_radians(), WIDTH as f32 / HEIGHT as f32, 0.1, 1200.0);
    let settings = crate::settings::ClientSettings::default();
    let mut globals: Globals = bytemuck::Zeroable::zeroed();
    globals.view_proj = (proj * view).to_cols_array_2d();
    globals.inv_view_proj = (proj * view).inverse().to_cols_array_2d();
    globals.camera_pos = [eye.x, eye.y, eye.z, 1.0];
    globals.sun = [-0.4, -0.8, -0.3, 1.0];
    globals.sun_color = [1.0, 1.0, 1.0, 1.0];
    globals.fill_color = [1.0, 1.0, 1.0, 1.0];
    globals.extra = [1.0, settings.ambient_occlusion, 0.0, if fog { 1.0 } else { 0.0 }];
    globals.fog_params = [VIEW_BLOCKS * 0.55, VIEW_BLOCKS * 0.95, settings.ambient_light, 1.0];
    globals.fog_color = [SKY[0], SKY[1], SKY[2], 1.0];
    globals.texture_params = [textures.resolution as f32, 0.0, 0.0, 0.0];
    let backdrop = if fog {
        wgpu::Color { r: SKY[0] as f64, g: SKY[1] as f64, b: SKY[2] as f64, a: 1.0 }
    } else {
        wgpu::Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }
    };

    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod repro globals"),
        contents: bytemuck::bytes_of(&globals),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lod repro globals layout"),
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
        label: Some("lod repro globals bind"),
        layout: &globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() }],
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lod repro texture layout"),
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
        label: Some("lod repro texture bind"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&textures.texture_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&textures.sampler) },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lod repro shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lod repro layout"),
        bind_group_layouts: &[&globals_layout, &texture_layout],
        push_constant_ranges: &[],
    });
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let pipeline = |entry: &str, cull: Option<wgpu::Face>| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lod repro pipeline"),
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
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: cull,
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
        })
    };
    let solid_pipeline = pipeline("fs_solid", Some(wgpu::Face::Back));
    let cutout_pipeline = pipeline("fs_cutout", None);

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod repro vertices"),
        contents: bytemuck::cast_slice(&meshed.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = |indices: &[u32]| {
        (!indices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lod repro indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            })
        })
    };
    let solid_indices = index_buffer(&meshed.solid);
    let cutout_indices = index_buffer(&meshed.cutout);
    let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod repro instance"),
        contents: bytemuck::cast_slice(&[[0.0f32, 0.0, 0.0, 0.0]]),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let size = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lod repro target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lod repro depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lod repro pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(backdrop), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &globals_bind, &[]);
        pass.set_bind_group(1, &texture_bind, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        for (pipeline, buffer, count) in [
            (&solid_pipeline, &solid_indices, meshed.solid.len()),
            (&cutout_pipeline, &cutout_indices, meshed.cutout.len()),
        ] {
            if let Some(buffer) = buffer {
                pass.set_pipeline(pipeline);
                pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..count as u32, 0, 0..1);
            }
        }
    }
    let bytes_per_row = WIDTH * 4; // 5120, a multiple of 256
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lod repro readback"),
        size: (bytes_per_row * HEIGHT) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(HEIGHT) },
        },
        size,
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
    image::RgbaImage::from_raw(WIDTH, HEIGHT, data).expect("buffer is the right size")
}

/// Backdrop pixels with terrain above them in the same column: holes, and
/// painted green on the returned copy so they can be found.
pub(super) fn holes(picture: &image::RgbaImage) -> (u32, image::RgbaImage) {
    let mut marked = picture.clone();
    let mut count = 0;
    for x in 0..picture.width() {
        let mut under_terrain = false;
        for y in 0..picture.height() {
            let p = picture.get_pixel(x, y);
            let backdrop = p[0] > 240 && p[1] < 16 && p[2] > 240;
            if !backdrop {
                under_terrain = true;
            } else if under_terrain {
                count += 1;
                marked.put_pixel(x, y, image::Rgba([0, 255, 0, 255]));
            }
        }
    }
    (count, marked)
}

#[test]
#[ignore = "a tool: needs a GPU; photographs a mountain at every detail level"]
fn what_a_mountain_looks_like_at_every_detail_level() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, 16).expect("textures load");
    let layers = textures.face_layers();

    let generator = WorldGen::new(SEED);
    let centre = std::env::var("LOD_REPRO_CENTRE")
        .ok()
        .and_then(|s| s.split_once(',').and_then(|(x, z)| Some((x.trim().parse().ok()?, z.trim().parse().ok()?))))
        .unwrap_or_else(|| generator.spawn_column());
    let span: i32 = std::env::var("LOD_REPRO_SPAN").ok().and_then(|s| s.parse().ok()).unwrap_or(25);
    let started = std::time::Instant::now();
    let scene = build_scene(centre, span);
    let corner = scene.corner();
    let size = span * CHUNK_SIZE_X as i32;
    println!("seed {SEED}, centre {centre:?}, {span}x{span} chunks, built in {:.1}s", started.elapsed().as_secs_f32());

    // The ground, every fourth column, and the thinnest high things in it:
    // the report's "knife-edge spire" is a place to point a camera.
    let height = |x: i32, z: i32| scene.generator.height_at(corner.x as i32 + x, corner.z as i32 + z);
    let ramp = b" .:-=+*#%@";
    for z in (0..size).step_by(8) {
        let row: String = (0..size)
            .step_by(4)
            .map(|x| {
                let h = height(x, z) - SEA_LEVEL;
                if h < 0 {
                    '~'
                } else {
                    ramp[((h / 14) as usize).min(ramp.len() - 1)] as char
                }
            })
            .collect();
        println!("{row}");
    }
    let mut spires: Vec<(i32, i32, i32, i32)> = Vec::new();
    for z in (20..size - 20).step_by(2) {
        for x in (20..size - 20).step_by(2) {
            let h = height(x, z);
            let around = [(-4, 0), (4, 0), (0, -4), (0, 4)].iter().map(|(dx, dz)| height(x + dx, z + dz)).sum::<i32>() / 4;
            spires.push((h - around, x, h, z));
        }
    }
    spires.sort_unstable_by_key(|spire| std::cmp::Reverse(spire.0));
    for (prominence, x, h, z) in spires.iter().take(6) {
        println!("spire at scene ({x}, {h}, {z}), world ({}, {}), {prominence} over the ring four out", corner.x as i32 + x, corner.z as i32 + z);
    }

    // What the trunks stand on, and whether they have a crown: the other
    // half of the report, counted in the generated blocks, before any
    // detail level has touched them.
    let mut census: std::collections::BTreeMap<(&'static str, bool), u32> = Default::default();
    let mut bare_on_rock: Vec<(i32, i32, i32)> = Vec::new();
    for dz in 1..span - 1 {
        for dx in 1..span - 1 {
            let pos = ChunkPos::new(scene.first.x + dx, scene.first.z + dz);
            if scene.chunks.get(pos).is_none() {
                continue;
            }
            for lz in 0..CHUNK_SIZE_Z as i32 {
                for lx in 0..CHUNK_SIZE_X as i32 {
                    let (gx, gz) = (pos.x * CHUNK_SIZE_X as i32 + lx, pos.z * CHUNK_SIZE_Z as i32 + lz);
                    let Some(column) = scene.chunks.column(gx, gz) else { continue };
                    for y in 1..CHUNK_SIZE_Y as i32 - 12 {
                        if block_kind(column.block(y)) != BLOCK_LOG || block_kind(column.block(y - 1)) == BLOCK_LOG {
                            continue;
                        }
                        let crowned = (-3..=3).any(|ox: i32| {
                            (-3..=3).any(|oz: i32| {
                                scene.chunks.column(gx + ox, gz + oz).is_some_and(|c| (y..y + 12).any(|up| is_leafy(c.block(up))))
                            })
                        });
                        let below = block_name(column.block(y - 1));
                        *census.entry((below, crowned)).or_default() += 1;
                        if !crowned && bare_on_rock.len() < 12 {
                            bare_on_rock.push((gx, y, gz));
                        }
                    }
                }
            }
        }
    }
    println!("trunk feet by what is under them, and whether leaves are within three columns and twelve blocks:");
    for ((below, crowned), count) in &census {
        println!("  on {below:<20} crowned {crowned:<5} {count}");
    }
    println!("uncrowned feet: {bare_on_rock:?}");

    let modes: Vec<String> = std::env::var("LOD_REPRO_MODES")
        .unwrap_or_else(|_| "fine,game".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let only = std::env::var("LOD_REPRO_ONLY").ok();

    // Cameras: the eight compass points from the spawn, the highest spire
    // from a hundred blocks off, and whatever the environment adds.
    let spawn = generator.spawn_column();
    let spawn_eye = Vec3::new(spawn.0 as f32 + 0.5, generator.height_at(spawn.0, spawn.1) as f32 + 2.62, spawn.1 as f32 + 0.5);
    let mut views: Vec<(String, Vec3, Vec3)> = Vec::new();
    for (i, name) in ["n", "ne", "e", "se", "s", "sw", "w", "nw"].iter().enumerate() {
        let angle = i as f32 * std::f32::consts::FRAC_PI_4;
        let look = Vec3::new(angle.sin(), 0.12, -angle.cos());
        views.push((format!("spawn_{name}"), spawn_eye, spawn_eye + look * 60.0));
    }
    if let Some((_, x, h, z)) = spires.first() {
        let spire = corner + Vec3::new(*x as f32 + 0.5, *h as f32 * 0.8, *z as f32 + 0.5);
        for (name, dx, dz) in [("spire_from_s", 0.0, 100.0), ("spire_from_w", -100.0, 0.0), ("spire_from_n", 0.0, -100.0), ("spire_from_e", 100.0, 0.0)] {
            let (ex, ez) = (spire.x + dx, spire.z + dz);
            let ground = generator.height_at(ex as i32, ez as i32) as f32;
            views.push((name.to_string(), Vec3::new(ex, ground.max(SEA_LEVEL as f32) + 12.0, ez), spire));
        }
    }
    if let Ok(extra) = std::env::var("LOD_REPRO_VIEWS") {
        for spec in extra.split(';').filter(|s| !s.trim().is_empty()) {
            let parts: Vec<&str> = spec.trim().split(':').collect();
            let vec = |s: &str| {
                let v: Vec<f32> = s.split(',').map(|n| n.trim().parse().expect("a number")).collect();
                Vec3::new(v[0], v[1], v[2])
            };
            views.push((parts[0].to_string(), vec(parts[1]), vec(parts[2])));
        }
    }

    let mut cache: HashMap<String, Meshed> = HashMap::new();
    for (name, eye, at) in &views {
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let eye_chunk = ChunkPos::from_global(eye.x.floor() as i32, eye.z.floor() as i32).0;
        for mode in &modes {
            let levels = match mode.as_str() {
                "fine" => Levels::Fine,
                "game" => Levels::Game(eye_chunk),
                level => Levels::All(level.parse().expect("a mode is fine, game or a level")),
            };
            let key = format!("{levels:?}");
            let meshed = cache.entry(key).or_insert_with(|| mesh_scene(&scene, &layers, levels));
            let (scene_eye, scene_at) = (*eye - corner, *at - corner);
            let picture = shoot(device, queue, &textures, meshed, scene_eye, scene_at, false);
            let (count, marked) = holes(&picture);
            let _ = picture.save(format!("{out}/{name}_{mode}.png"));
            if count > 0 {
                let _ = marked.save(format!("{out}/{name}_{mode}_holes.png"));
            }
            let fogged = shoot(device, queue, &textures, meshed, scene_eye, scene_at, true);
            let _ = fogged.save(format!("{out}/{name}_{mode}_fog.png"));
            println!(
                "{name:>16} {mode:>5}: {count:6} hole pixels, {} solid + {} cutout triangles",
                meshed.solid.len() / 3,
                meshed.cutout.len() / 3
            );
        }
    }
    println!("pictures in {out}");
}
