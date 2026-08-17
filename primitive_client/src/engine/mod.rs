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
//! | `mesher`   | doing that on worker threads                            |
//! | `camera`   | view and projection                                     |
//! | `frustum`  | which chunks the camera can actually see                |
//! | `sky`      | the time of day, and the colours that follow from it    |
//! | `font`     | the bitmap glyphs the UI layer draws with               |
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
pub mod camera;
pub mod font;
pub mod fog;
pub mod frustum;
pub mod gpu_timing;
pub mod item_model;
pub mod mesh;
pub mod mesher;
pub mod renderer;
pub mod sky;
pub mod texture;
