//! The engine: everything that talks to the GPU, and everything that
//! exists only to feed it.
//!
//! This layer knows about wgpu, vertices, texture arrays, matrices and
//! shaders. It knows nothing about players, inventories, servers or
//! menus -- what it draws arrives as plain geometry and plain
//! parameters, and it hands back nothing but a rendered frame.
//!
//! That boundary is the point of the split. A renderer that reaches into
//! game state ends up with a copy of the rules inside it, and the two
//! drift; here the rules live in [`crate::logic`] and the drawing lives
//! here, with a vertex buffer between them.
//!
//! ## What is in it
//!
//! | module     | what it owns                                          |
//! |------------|-------------------------------------------------------|
//! | `renderer` | the device, the pipelines, one frame                   |
//! | `texture`  | the block/glyph texture array and the face lookup      |
//! | `mesh`     | the vertex format, and turning a chunk into triangles   |
//! | `lod`      | the same chunk out of bigger blocks, when it is far away |
//! | `mesher`   | doing that on worker threads                            |
//! | `camera`   | view and projection                                     |
//! | `frustum`  | which chunks the camera can actually see                |
//! | `sky`      | the time of day, and the colours that follow from it    |
//! | `shadow`   | the sun's shadow map, when the player has asked for one |
//! | `lighting` | how much colour the light carries, step by step          |
//! | `opt`      | the frame-cost experiments, one environment switch each  |
//! | `font`     | the bitmap glyphs the UI layer draws with               |
//! | `capture`  | one frame of it, written to a PNG                        |
//!
//! ## The three seams that go the other way
//!
//! `renderer` imports the vertex layouts of the hotbar, the remote
//! players and the first-person hand from [`crate::ui`], [`crate::net`]
//! and [`crate::logic`]. Those are GPU vertex formats living beside the
//! code that fills them, which is the useful place for them; the
//! alternative -- moving three `#[repr(C)]` structs in here and leaving
//! their builders outside -- would split each of them from its only
//! caller to satisfy the diagram. Noted rather than hidden: they are the
//! only upward references in this layer, and they carry no behaviour.

pub mod arena;
pub mod breeze;
pub mod capture;
pub mod camera;
pub mod critters;
pub mod pebble_art;
pub mod font;
pub mod fog;
pub mod frustum;
pub mod gpu_timing;
pub mod item_model;
pub mod lamp_shadow;
/// The lean-to's model held to its colliders, its aim and its light.
#[cfg(test)]
mod lean_to_tests;
pub mod lighting;
pub mod lod;
pub mod mesh;
pub mod mesher;
pub mod opt;
pub mod particles;
pub mod relief;
pub mod renderer;
pub mod shadow;
pub mod sky;
pub mod texture;
pub mod water;

/// One graphics device, shared by every test that needs one.
///
/// ## Why one, and why it is not a tidy-up
///
/// Because the suite was crashing. Each GPU test used to build its own
/// `Instance`, adapter and `Device` -- two copies of the same twenty
/// lines, in `arena` and in `renderer` -- and cargo runs tests on
/// several threads at once. Creating and dropping graphics devices
/// concurrently is the thing drivers are worst at: the run died with
/// `STATUS_ACCESS_VIOLATION` about one time in three, somewhere inside
/// the driver and never in the same test twice.
///
/// A suite that fails a third of the time is worse than a suite that
/// fails: "the tests are green" stops being a fact and becomes a thing
/// you re-run until it is true, and a real failure hides in the noise.
/// This project's standard is that the tests pass, so the flake had to
/// go.
///
/// One device, made once, handed out by reference. It is never dropped
/// -- the process ends holding it, which is the one time letting a
/// resource leak is right: there is no thread left to race with.
///
/// `None` on a machine with no GPU at all, which is a CI runner. The
/// tests that need one say so and pass, because a suite that cannot be
/// run where it is most wanted is a suite nobody runs.
#[cfg(test)]
pub fn test_gpu() -> Option<&'static (wgpu::Device, wgpu::Queue)> {
    static DEVICE: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue)>> =
        std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            });
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))?;
            // **A software rasteriser is not a GPU.** A Windows runner has
            // no graphics card but does have WARP, and wgpu hands it out as
            // an adapter like any other: every GPU test then ran on the
            // processor, and the client's test binary died on the runner
            // with a bare exit code and no test named -- while passing on
            // any machine with a real card. `None` is what this function
            // promises a machine without one.
            if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                return None;
            }
            // **The limits the game asks for, not wgpu's defaults.**
            // A test device made with the defaults is capped at 256
            // texture array layers while the real one is not (see
            // `renderer::terrain_limits`), and the difference is not
            // academic: the atlas is larger than that, so every GPU test
            // would fail to load the textures the game loads fine.
            pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("the test device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: crate::engine::renderer::terrain_limits(&adapter),
                },
                None,
            ))
            .ok()
        })
        .as_ref()
}
