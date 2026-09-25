//! **The order a frame happens in, and the pieces it happens in.**
//!
//! ## Why this directory exists
//!
//! `run` in `lib.rs` used to be one function of six thousand lines: the
//! event match and the whole body of a frame written inline, every local
//! in scope for every statement. That was not merely long. It was the
//! reason `primitive_client/src/scenario/` had to keep a *second copy* of
//! the frame -- its own right click, its own body step, its own mining --
//! because there was nothing to call. The harness said so itself, in as
//! many words: "Rejected: pulling the whole frame out of `run` into
//! something both could call. That is the right end state." A scenario
//! that plays its own copy of a step is a scenario that passes on a game
//! that no longer exists, and that is a whole class of bug rather than a
//! tidiness complaint.
//!
//! So the phases live here, as free functions over the frame's own state,
//! and both `run` and a scenario call *them*. Nothing in this directory
//! owns a window, a swapchain or a socket; it is handed what it needs.
//!
//! ## The order, and why it is that order
//!
//! Written down here because the order is the design, and a phase moved
//! is a bug that looks like a rendering fault:
//!
//! 1. **The wait, at the very top.** `GraphicsState::acquire` blocks for
//!    a swapchain image *before* the mouse is read. Waiting at the end --
//!    which is where it used to be -- means everything drawn was decided
//!    a whole frame before it appeared. It stays in `run`, because it is
//!    the one step that needs the window.
//! 2. **The socket** (`drain_network`), then the hand-offs the messages
//!    caused: dying, a chest the server opened, a station's seat. Those
//!    arrive as messages rather than as events, so the cursor changes
//!    hands here and nowhere else.
//! 3. **Streaming**, rationed ([`streaming::stream`]): chunk integration,
//!    the map's survey, the detail levels, mesh dispatch and mesh upload
//!    each get a slice of the frame. An unbudgeted phase is a phase that
//!    lands forty chunks in one frame and stutters. Results are handled
//!    nearest-the-player first, so the chunk someone just edited is never
//!    starved.
//! 4. **What the fingers are still doing** ([`events::poll_touch`]) --
//!    the held half of touch, read beside the mouse delta because that is
//!    what it is.
//! 5. **The body** ([`body::step`]): the raft, the horse, the collider in
//!    fixed slices, stamina. Prediction only; the server decides.
//! 6. **Everything small that moves** ([`effects::step`]): particles, the
//!    weather, the critters, the breeze, the line in the water. After
//!    physics and before the camera is used to draw them.
//! 7. **The hands** ([`hands::step`]): mining, blows, the cut, the
//!    mouthful, and the arm that follows from all of it.
//! 8. **Sound** ([`sound::update`]), which reads the results of
//!    everything above it.
//! 9. **The interface** ([`interface::build`]) and the moving geometry
//!    ([`scene::build`]), both on the rebuild clock.
//! 10. **The draw**, back in `run`, because it needs the frame acquired
//!     in step 1.
//!
//! ## What is still in `lib.rs`, and why
//!
//! The parts that need the window, the swapchain, the tokio runtime or
//! the event loop's own `exit`: opening a session, tearing one down,
//! acquiring and presenting. Those are not frame phases; they are what a
//! frame is hosted by.

pub mod body;
pub mod effects;
pub mod events;
pub mod hands;
pub mod interface;
pub mod menu_frame;
pub mod readout;
pub mod scene;
pub mod screens;
pub mod sound;
pub mod streaming;
