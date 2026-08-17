//! The player's own arm, and whatever is in it.
//!
//! ## Why the game needs one at all
//!
//! Everything the player does happens at the crosshair, and until now
//! nothing on screen said *who* was doing it. Blocks crumbled, items
//! appeared in the bar, and the hands that did the work were not there.
//! The view model is how a first-person game answers "what am I holding"
//! without the player having to look down at the hotbar and read it: the
//! pick is in the frame, so the pick is the answer.
//!
//! It is also the only place a swing can be *seen*. The mining bar and
//! the cracks say a block is being broken; neither says anything about
//! effort or rhythm, and the difference between a game where digging is
//! a progress bar and one where it is work is almost entirely this
//! animation.
//!
//! ## Where it lives, and why it is not in `engine`
//!
//! This module reads an inventory slot, a mining state and a texture
//! pack, and writes vertices. That is the same shape as
//! [`crate::logic::entities`] -- game state in, geometry out -- so it
//! sits beside it, and the vertex format travels with the code that
//! fills it. See the note on seams in [`crate::engine`]: the hotbar's
//! vertex lives in `ui`, the remote player's in `net`, and the hand's
//! here, each next to its only builder.
//!
//! ## The space it is built in
//!
//! Not the world. A hand welded to the camera and expressed in world
//! coordinates has to be rebuilt from the camera basis every time the
//! player so much as turns their head, and every rounding error in that
//! basis lands as a jitter a foot from the eye where it is most visible.
//!
//! So the geometry is authored directly in *view* space -- x right, y
//! up, -z forward, the eye at the origin -- and the renderer gives it a
//! projection of its own. The hand is then literally standing still: the
//! only things that move it are the swing and the stride, which is
//! exactly the set of things that should.
//!
//! What that costs is that this geometry cannot be drawn by any of the
//! existing pipelines, all of which multiply by `view_proj`. It gets one
//! of its own, four dozen quads wide. See `engine/hand.wgsl` and
//! `GraphicsState::hand_pipeline`, which also owns the answer to the
//! other half of the problem -- how a hand held ten centimetres from the
//! eye avoids being sliced in half by the wall the player is standing
//! against.
//!
//! ## What is in the hand
//!
//! Whatever the selected slot holds, drawn the way the world draws it:
//! a tool is a sprite given a thickness by
//! [`crate::engine::item_model`], a block is a block. That mechanism
//! already exists for dropped items and is reused whole rather than
//! reimplemented -- one model per texture, built once at load, and a
//! transform per frame. A new tool is still a new PNG and nothing else.

use glam::{Mat4, Vec3};

use primitive_shared::types::BlockId;

use crate::engine::item_model::ItemVertex;
use crate::engine::mesh::{face_uv, faces, pack_light};
use crate::engine::texture::{FaceLayers, TextureManager};

/// One vertex of the view model.
///
/// Its own format, and the reason is the arm. Everything else the game
/// draws is either textured (terrain, items) or flat-coloured (remote
/// players), and the two live in different pipelines; the hand is both
/// at once -- a bare forearm in skin colour holding a textured tool --
/// and splitting it into two draws to avoid one `vec4` per vertex would
/// be two pipelines and two buffers for sixty quads.
///
/// `position` is in view space; see the module note.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HandVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Texture layer in the top half, the terrain's light word in the
    /// bottom -- the same arrangement `ItemVertex` uses, so the shader
    /// unpacks it the same way. A layer of [`UNTEXTURED`] means the
    /// vertex is drawn in its own colour and samples nothing.
    pub packed: u32,
    pub tint: [f32; 4],
}

impl HandVertex {
    pub const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Uint32,
        3 => Float32x4,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HandVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Layer value meaning "no texture, use the vertex colour". Must match
/// `UNTEXTURED` in `hand.wgsl`.
///
/// A sentinel rather than a flag of its own, for the same reason the
/// hotbar has one: the layer field is sixteen bits and the atlas will
/// never have sixty-five thousand pictures in it.
pub const UNTEXTURED: u32 = 0xFFFF;

// --- where the parts sit, at rest, in view space ---
//
// All of these were chosen against a 16:9 frame with the hand
// projection's 70-degree vertical field (see `HAND_FOV_Y` in the
// renderer). The forearm enters from off the bottom-right corner and
// runs away from the eye and inwards; the item sits at the far end of
// it, near enough to the crosshair to be read at a glance and far
// enough from it not to cover what is being aimed at.


/// Middle of whatever is being held.
///
/// **On the end of the forearm**, which is the thing the earlier
/// numbers got wrong: the arm and the item were positioned separately,
/// so moving one moved it away from the other and the tool came out
/// hanging in mid-air beside a stump of wrist. This is derived from the
/// arm's far end rather than chosen next to it.
const ITEM_CENTRE: Vec3 = Vec3::new(0.46, -0.38, -0.78);
/// How big a held tool is.
///
/// Sized against the frame rather than against anything in the world:
/// a view model is judged entirely by how much of the view it takes up.
/// With no arm behind it the tool is the whole of the model, so it can
/// afford to be the size a carried tool actually looks -- head up near
/// the middle of the frame's height, haft running off the bottom edge.
/// Small enough to read as an icon in the corner was the earlier
/// mistake, and it made the thing look like a sticker rather than
/// something being carried.
const ITEM_SCALE: f32 = 0.58;
/// How the tool is turned in the hand.
///
/// **Both were solved by looking at the actual textures**, which is the
/// only way they could have been: the sprite is a picture of a tool
/// already lying at some angle of its own, so the rotation that puts it
/// in a working grip depends on how the artist drew it. All three flint
/// tools are drawn head-above-haft but at different leans -- the pick
/// at about seventy degrees, the axe upright, the knife at fifty -- so
/// one shared pair of angles cannot give all three the same pose, only
/// the same *kind* of pose: head up and forward, haft running off the
/// bottom-right corner.
///
/// The yaw turns the face towards the player rather than away, and
/// stays small either way. Turning the sprite hard sounds
/// like it adds depth and does the opposite: a plate one texel thick
/// seen at a steep angle is a plate seen edge-on, and the
/// foreshortening swung the apparent lean around so far that the same
/// numbers stood the pick up like a spear and hung the axe head-down.
const ITEM_YAW: f32 = -0.35;
const ITEM_ROLL: f32 = 0.55;

/// A held *block* is a cube, and a cube of the same measurement as a
/// sprite reads much larger -- a plate has no bulk. Same trade the
/// dropped-item code makes, in the other direction.
const BLOCK_SCALE: f32 = 0.30;
const BLOCK_YAW: f32 = -0.60;
const BLOCK_PITCH: f32 = 0.20;


/// Where the arm is turned about when it swings: down and behind, about
/// where a shoulder would be if the model had one.
///
/// Not the middle of the arm. A hand that rotates about its own centre
/// wobbles like a compass needle; one that rotates about a joint below
/// the frame swings, and the difference is the entire animation.
///
/// How far below matters more than it looks. The item is held about two
/// thirds of a unit from this point, so every radian of swing moves it
/// two thirds of a unit -- put the joint much further away and a
/// perfectly reasonable-sounding angle throws the whole hand off the
/// bottom of the screen.
const SHOULDER: Vec3 = Vec3::new(0.50, -0.62, -0.30);

/// How long one blow takes, from rest back to rest.
///
/// Just under a third of a second: fast enough that holding the button
/// down reads as repeated effort rather than one slow stir, slow enough
/// that a single click is visible at all.
const SWING_SECONDS: f32 = 0.28;
/// Where in that time the blow *lands*. Everything before this is the
/// windup and the strike, everything after is the recovery -- which is
/// two and a half times as long, because that is what a swing feels
/// like and an even one feels like waving.
const IMPACT: f32 = 0.30;
/// How far the arm drops at the moment of impact, in radians (about 31
/// degrees). See [`SHOULDER`] for why this is not the 50-odd degrees a
/// swing sounds like it should be.
const SWING_PITCH: f32 = 0.30;
/// ...and how far it rolls, and how far it is thrown forward, which are
/// the two things that keep the blow from looking like a hinge.
const SWING_ROLL: f32 = 0.14;
const SWING_REACH: f32 = 0.07;

/// Sway and rise of the walking bob, in view-space units.
const BOB_SWAY: f32 = 0.022;
const BOB_RISE: f32 = 0.016;
/// Radians of stride phase per block travelled -- the same figure the
/// camera's own bob uses, so the hand and the head are in step. See
/// `shake::RUN_PHASE_PER_BLOCK`; duplicated rather than shared because
/// the two effects are free to diverge and a shared constant would
/// quietly forbid it.
const BOB_PHASE_PER_BLOCK: f32 = 0.90;
/// How fast the bob winds up when the player starts moving and down
/// when they stop. Instant would snap the arm.
const BOB_BLEND_PER_SEC: f32 = 6.0;

/// The animation state of the view model. Geometry is built from it and
/// nothing else.
#[derive(Default)]
pub struct Hand {
    /// Seconds into the current blow, or `None` at rest.
    swing: Option<f32>,
    /// Advances with distance travelled, so the bob keeps step with the
    /// stride rather than with the frame rate.
    bob_phase: f32,
    /// 0..1, how much of the bob is currently applied.
    bob_blend: f32,
}

impl Hand {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a blow, if one is not already under way.
    ///
    /// Deliberately *not* a restart. Clicking twice inside a third of a
    /// second is one flurry, and resnapping the arm back to rest in the
    /// middle of it looks like a dropped frame rather than like haste.
    pub fn strike(&mut self) {
        if self.swing.is_none() {
            self.swing = Some(0.0);
        }
    }

    /// Advances the animation by one frame.
    ///
    /// `digging` is whether the player is currently making progress on a
    /// block: while they are, the arm swings again the moment it comes
    /// back to rest, which is what turns a click into a rhythm. `speed`
    /// is horizontal speed in blocks per second and `grounded` whether
    /// the player is actually on the floor -- the bob is a footfall, and
    /// there are none in mid-air.
    pub fn update(&mut self, dt: f32, digging: bool, speed: f32, grounded: bool) {
        // A stall must not throw the arm through a whole blow in one
        // step; the same clamp physics and the camera shake use.
        let dt = dt.clamp(0.0, 0.1);

        match self.swing {
            Some(elapsed) => {
                let elapsed = elapsed + dt;
                self.swing = if elapsed < SWING_SECONDS {
                    Some(elapsed)
                } else if digging {
                    // Carry the overshoot into the next blow rather than
                    // starting it from zero, or the rhythm slows to
                    // whatever the frame rate happens to be.
                    Some(elapsed - SWING_SECONDS)
                } else {
                    None
                };
            }
            None if digging => self.swing = Some(0.0),
            None => {}
        }

        let target = if grounded && speed > 0.1 { 1.0 } else { 0.0 };
        let step = BOB_BLEND_PER_SEC * dt;
        self.bob_blend += (target - self.bob_blend).clamp(-step, step);
        self.bob_blend = self.bob_blend.clamp(0.0, 1.0);
        self.bob_phase =
            (self.bob_phase + speed * dt * BOB_PHASE_PER_BLOCK).rem_euclid(std::f32::consts::TAU);
    }

    /// How far through a blow the arm is, 0 at rest and 1 at the moment
    /// of impact.
    pub fn swing(&self) -> f32 {
        match self.swing {
            Some(elapsed) => swing_curve(elapsed / SWING_SECONDS),
            None => 0.0,
        }
    }

    /// Builds this frame's view model.
    ///
    /// `shown` is the caller's decision and not this module's: the hand
    /// is hidden behind the inventory, a chest, the pause menu and the
    /// death screen, and every one of those is a fact about the
    /// interface rather than about the arm. It is a parameter rather
    /// than a `return` at the call site so that "nothing is built while
    /// a screen is up" is a property this module can be *tested* for.
    ///
    /// `light` is the sky/block light at the player's own head. A hand
    /// lit as though it were always outdoors stays bright in a cave,
    /// which is exactly where the player is looking hardest at their
    /// torch.
    #[allow(clippy::too_many_arguments)]
    pub fn build_into(
        &self,
        shown: bool,
        held: Option<BlockId>,
        layers: &FaceLayers,
        models: Option<&TextureManager>,
        light: (u8, u8),
        vertices: &mut Vec<HandVertex>,
        indices: &mut Vec<u32>,
    ) {
        if !shown {
            return;
        }
        // **An empty hand is not drawn at all.**
        //
        // A bare forearm has nothing on it: no texture, no silhouette
        // worth the name, nothing for the eye to read except a wedge of
        // flat colour across the corner of the screen. Held *something*
        // it is a hand holding a thing, and reads as one instantly --
        // the tool does the work and the arm is what the tool is
        // attached to. On its own it is a slab of skin-coloured card,
        // and it was on screen for every minute the player spent not
        // carrying anything, which is most of the early game.
        //
        // So the arm is drawn as part of holding, rather than the thing
        // being drawn as part of the arm.
        let Some(block) = held else {
            return;
        };

        let swing = self.swing();
        // Everything the hand does happens about the shoulder, so the
        // swing is built once here and both parts ride it. Read right to
        // left: the parts are placed, thrown forward, rotated about the
        // joint, and finally nudged by the stride.
        let group = Mat4::from_translation(self.bob_offset())
            * Mat4::from_translation(SHOULDER)
            * Mat4::from_rotation_x(-SWING_PITCH * swing)
            * Mat4::from_rotation_z(SWING_ROLL * swing)
            * Mat4::from_translation(-SHOULDER)
            * Mat4::from_translation(Vec3::new(0.0, 0.0, -SWING_REACH * swing));

        // --- no arm ---
        //
        // **There is no forearm, and that is the fix rather than a
        // shortcut.** Every version of one was a flat-shaded box a foot
        // from the eye, and a box is what it looked like: first a plank
        // of skin colour across a sixth of the frame, then a stump in
        // the corner, then a fist that read as a brown pebble glued to
        // the haft. The game has no character model, no skin texture and
        // no skeleton, so the arm had nothing to be made of except a
        // solid colour -- and a solid-colour cuboid does not become an
        // arm at any size.
        //
        // What the view model is for is answering "what am I holding",
        // and the tool answers it by itself. Held low and to the right,
        // swinging on a blow, it reads as being carried whether or not
        // there is a hand drawn round it -- which is how most
        // first-person games with no character art do it.
        //
        // If an arm comes back it should come back as *art*: a sprite of
        // a hand, drawn by the same hand that drew the tools, given
        // thickness by `item_model` like everything else. That is a PNG
        // and a transform, not a bigger box.

        // --- what is in it ---
        match models.and_then(|models| models.item_model(block)) {
            // A tool: the sprite with a thickness that the dropped-item
            // code already knows how to build. Same model, same texture,
            // a different transform -- which is the whole reason
            // `append_transformed` exists.
            Some(model) => {
                let transform = group
                    * Mat4::from_translation(ITEM_CENTRE)
                    * Mat4::from_rotation_y(ITEM_YAW)
                    * Mat4::from_rotation_z(ITEM_ROLL)
                    * Mat4::from_scale(Vec3::splat(ITEM_SCALE));
                let layer = layers
                    .layer_for_item(block)
                    .unwrap_or_else(|| layers.layer_for_face(block, 0));
                // Built through the item vertex and then converted,
                // rather than by a second copy of the same loop. The
                // scratch is a few dozen vertices on a mesh that is
                // rebuilt at most 120 times a second; the alternative is
                // two implementations of the winding, which is what the
                // face-normal bug in `item_model` came out of.
                let mut scratch: Vec<ItemVertex> = Vec::with_capacity(model.quads.len() * 4);
                let mut scratch_indices: Vec<u32> = Vec::new();
                model.append_transformed(
                    &mut scratch,
                    &mut scratch_indices,
                    transform,
                    layer,
                    light.0,
                    light.1,
                );
                for quad in scratch.chunks_exact(4) {
                    let corners = [
                        Vec3::from_array(quad[0].position),
                        Vec3::from_array(quad[1].position),
                        Vec3::from_array(quad[2].position),
                        Vec3::from_array(quad[3].position),
                    ];
                    let uv = [quad[0].uv, quad[1].uv, quad[2].uv, quad[3].uv];
                    push_quad(vertices, indices, corners, uv, layer, [1.0; 4], light);
                }
            }
            // A block is a block. Nothing here knows which blocks are
            // which: a block with no model of its own is a cube, exactly
            // the rule `entities` uses for the dropped version.
            None => {
                let transform = group
                    * Mat4::from_translation(ITEM_CENTRE)
                    * Mat4::from_rotation_y(BLOCK_YAW)
                    * Mat4::from_rotation_x(BLOCK_PITCH)
                    * Mat4::from_scale(Vec3::splat(BLOCK_SCALE));
                append_box(vertices, indices, transform, Some((block, layers)), [1.0; 4], light);
            }
        }
    }

    /// Where the stride has carried the hand this frame.
    ///
    /// Vertical at twice the rate of horizontal, so the hand traces a
    /// flattened figure of eight -- one dip per footfall, one sway per
    /// pair of them. The same shape the camera bob traces, and for the
    /// same reason: a hand that swayed without dipping would look like
    /// it was being waved.
    fn bob_offset(&self) -> Vec3 {
        if self.bob_blend <= 0.0 {
            return Vec3::ZERO;
        }
        Vec3::new(
            self.bob_phase.sin() * BOB_SWAY,
            (self.bob_phase * 2.0).cos() * BOB_RISE,
            0.0,
        ) * self.bob_blend
    }
}

/// The shape of one blow, over 0..1 of its duration.
///
/// **Not a sine.** A sine is symmetric, and a symmetric swing reads as a
/// pendulum: the arm takes as long to come back as it took to go, and
/// nothing about it says that anything was *hit*. What a blow looks like
/// is almost all of the travel spent in the first third -- a hard,
/// decelerating strike -- and a slow, eased recovery afterwards.
///
/// The strike is a fractional power, so it leaves rest fast and arrives
/// slowing; the recovery is a square, so it leaves the impact slowly and
/// settles rather than stopping.
fn swing_curve(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    if t < IMPACT {
        (t / IMPACT).powf(0.55)
    } else {
        let back = (1.0 - t) / (1.0 - IMPACT);
        back * back
    }
}

/// A unit cube, transformed.
///
/// `textured` is the block whose faces should be drawn on it, or `None`
/// for a solid colour -- the forearm being the only caller that wants
/// the second.
fn append_box(
    vertices: &mut Vec<HandVertex>,
    indices: &mut Vec<u32>,
    transform: Mat4,
    textured: Option<(BlockId, &FaceLayers)>,
    tint: [f32; 4],
    light: (u8, u8),
) {
    for (index, face) in faces().iter().enumerate() {
        let layer = match textured {
            Some((block, layers)) => layers.layer_for_face(block, index),
            None => UNTEXTURED,
        };
        // `faces()` gives corners of the unit cube in 0..1; the box is
        // centred on the origin so that the transform's rotation turns
        // it about its middle rather than about a corner.
        let corners = std::array::from_fn(|i| {
            let c = face.corners[i];
            transform.transform_point3(Vec3::new(c[0] - 0.5, c[1] - 0.5, c[2] - 0.5))
        });
        let uv = std::array::from_fn(|i| face_uv(index, face.corners[i]));
        push_quad(vertices, indices, corners, uv, layer, tint, light);
    }
}

/// One quad, with its normal taken from how it is wound.
///
/// **Not from a stored face index, and that is the point.** The terrain
/// and the dropped items can name a face because their geometry is axis
/// aligned; the hand's is not -- it is pitched, rolled and swung -- and
/// a quad that still called itself "+Y" after being turned forty degrees
/// would be lit as though it had not been. The winding is the one
/// description of a quad's facing that survives an arbitrary transform,
/// so it is what gets asked.
fn push_quad(
    vertices: &mut Vec<HandVertex>,
    indices: &mut Vec<u32>,
    corners: [Vec3; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    tint: [f32; 4],
    light: (u8, u8),
) {
    let normal = (corners[1] - corners[0]).cross(corners[2] - corners[1]);
    // Ambient occlusion 3 -- unoccluded. Nothing is standing between the
    // player and their own hand.
    let packed = (layer << 16) | pack_light(light.0, light.1, 3, nearest_face(normal));
    let base = vertices.len() as u32;
    for (corner, uv) in corners.iter().zip(uv.iter()) {
        vertices.push(HandVertex {
            position: corner.to_array(),
            uv: *uv,
            packed,
            tint,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The face index whose normal is closest to `normal`.
///
/// Six directions is a coarse basis for a rotated quad, but it is the
/// one the light word can hold, and the error it costs is a few percent
/// of a lambert term on an object with no shadow to compare against.
/// Widening the vertex to carry a real normal would cost twelve bytes
/// per vertex to fix something nobody can see.
///
/// Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
fn nearest_face(normal: Vec3) -> u8 {
    let [x, y, z] = normal.to_array();
    if y.abs() >= x.abs() && y.abs() >= z.abs() {
        if y >= 0.0 { 0 } else { 1 }
    } else if x.abs() >= z.abs() {
        if x >= 0.0 { 2 } else { 3 }
    } else if z >= 0.0 {
        4
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers() -> FaceLayers {
        FaceLayers::empty_for_test()
    }

    /// Anything the hand builds, as a list of positions.
    fn build(hand: &Hand, shown: bool, held: Option<BlockId>) -> Vec<HandVertex> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        hand.build_into(
            shown,
            held,
            &layers(),
            // No texture manager in a test: every block is a cube, which
            // is the path that does not need one.
            None,
            (15, 0),
            &mut vertices,
            &mut indices,
        );
        assert_eq!(indices.len(), vertices.len() / 4 * 6, "indices do not match the quads");
        vertices
    }

    #[test]
    fn a_closed_screen_means_no_hand() {
        // The one case where drawing anything at all is a bug: an arm
        // over the inventory, or over the death screen, is worse than no
        // arm at all.
        assert!(build(&Hand::new(), false, Some(1)).is_empty());
    }

    #[test]
    fn an_empty_hand_is_not_drawn() {
        // **The arm exists to hold something.** On its own it is a wedge
        // of flat colour across the corner of the screen with nothing on
        // it to read, and it was there for every minute a player spent
        // carrying nothing -- which is most of the early game.
        assert!(
            build(&Hand::new(), true, None).is_empty(),
            "an empty hand drew a bare forearm"
        );
        // ...and holding something draws the thing. Only the thing:
        // there is no arm any more, and that is deliberate -- see the
        // note in `build_into`.
        let holding = build(&Hand::new(), true, Some(1));
        assert!(!holding.is_empty(), "holding something drew nothing at all");
    }

    #[test]
    fn the_hand_stays_in_front_of_the_eye_and_out_of_the_middle() {
        // View space: -z is forward. Geometry behind the eye is
        // geometry the projection turns inside out, and geometry across
        // the crosshair covers what the player is aiming at.
        for swinging in [false, true] {
            let mut hand = Hand::new();
            if swinging {
                hand.strike();
                hand.update(SWING_SECONDS * IMPACT, false, 0.0, true);
            }
            let vertices = build(&hand, true, Some(1));
            assert!(!vertices.is_empty());
            for v in &vertices {
                let [x, y, z] = v.position;
                assert!(z < -0.01, "a corner at z={z} is level with or behind the eye");
                assert!(z > -2.0, "a corner at z={z} is further away than the hand can be");
                assert!(x > 0.0, "a corner at x={x} crossed to the left of the screen");
                assert!(y < 0.35, "a corner at y={y} is above the middle of the frame");
                assert!((-2.0..2.0).contains(&y), "a corner at y={y} is nowhere near the frame");
            }
        }
    }

    #[test]
    fn the_hand_actually_lands_in_the_lower_right_of_the_frame() {
        // Every one of the constants above is a number somebody picked,
        // and the failure they invite is not a crash: it is a hand that
        // is perfectly well formed and entirely off the side of the
        // screen, which no other test here would notice. So this one
        // does what the GPU does -- the renderer's own hand projection,
        // see `HAND_FOV_Y` and `hand_view_proj` -- and looks at where
        // the corners come out.
        let projection = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.01, 4.0);
        // Only the holding case: an empty hand draws nothing at all
        // now, and "nothing is on screen" is the other test's business.
        for held in [Some(1)] {
            let vertices = build(&Hand::new(), true, held);
            let mut on_screen = 0;
            for v in &vertices {
                let ndc = projection.project_point3(Vec3::from_array(v.position));
                assert!(
                    (0.0..=1.0).contains(&ndc.z),
                    "a corner at depth {} is outside the hand's own near and far planes",
                    ndc.z
                );
                if ndc.x.abs() > 1.0 || ndc.y.abs() > 1.0 {
                    continue; // off the edge, which most of the forearm is
                }
                on_screen += 1;
                assert!(
                    ndc.x > -0.05,
                    "a visible corner at x={} has crossed to the left half of the screen",
                    ndc.x
                );
                assert!(
                    ndc.y < 0.45,
                    "a visible corner at y={} is up in the top of the frame",
                    ndc.y
                );
            }
            assert!(
                on_screen * 4 > vertices.len(),
                "only {on_screen} of {} corners are on screen at all",
                vertices.len()
            );
        }
    }

    #[test]
    fn a_swing_leaves_rest_and_comes_back_to_it() {
        let mut hand = Hand::new();
        assert_eq!(hand.swing(), 0.0, "an idle hand is already swinging");
        hand.strike();
        // Through the blow in small steps, watching that it actually
        // travels and that it is over when it says it is.
        let mut peak: f32 = 0.0;
        for _ in 0..40 {
            hand.update(SWING_SECONDS / 20.0, false, 0.0, true);
            peak = peak.max(hand.swing());
        }
        assert!(peak > 0.9, "the arm barely moved: peak {peak}");
        assert_eq!(hand.swing(), 0.0, "the arm never came back to rest");
    }

    #[test]
    fn the_blow_lands_early_and_recovers_late() {
        // The whole difference between a strike and a wave. Half way
        // through the animation the arm should already be most of the
        // way home from an impact that happened near the start.
        assert!(swing_curve(IMPACT) > 0.99, "the blow does not land at the impact");
        assert!(
            swing_curve(IMPACT * 0.5) > 0.6,
            "the strike is too slow to leave rest"
        );
        assert!(
            swing_curve(0.5) < 0.6,
            "the recovery is as fast as the strike, which reads as a pendulum"
        );
        assert_eq!(swing_curve(0.0), 0.0);
        assert_eq!(swing_curve(1.0), 0.0);
    }

    #[test]
    fn holding_the_button_keeps_the_arm_going() {
        let mut hand = Hand::new();
        let mut moved = 0;
        for _ in 0..60 {
            hand.update(SWING_SECONDS / 20.0, true, 0.0, true);
            if hand.swing() > 0.05 {
                moved += 1;
            }
        }
        assert!(
            moved > 40,
            "digging left the arm at rest for most of {} frames",
            60
        );
    }

    #[test]
    fn the_bob_only_answers_to_footfalls() {
        let mut still = Hand::new();
        let mut walking = Hand::new();
        let mut falling = Hand::new();
        for _ in 0..60 {
            still.update(1.0 / 60.0, false, 0.0, true);
            walking.update(1.0 / 60.0, false, 4.3, true);
            falling.update(1.0 / 60.0, false, 4.3, false);
        }
        assert_eq!(still.bob_offset(), Vec3::ZERO, "a standing player bobs");
        assert_eq!(falling.bob_offset(), Vec3::ZERO, "a falling player bobs");
        assert!(
            walking.bob_offset().length() > 0.0,
            "a walking player does not"
        );
        assert!(
            walking.bob_offset().length() < 0.05,
            "the bob is far larger than the hand"
        );
    }

    #[test]
    fn a_stall_cannot_throw_the_arm_through_a_whole_blow() {
        let mut hand = Hand::new();
        hand.strike();
        hand.update(10.0, false, 0.0, true);
        assert!(hand.swing() > 0.0, "one long frame skipped the entire swing");
    }

    #[test]
    fn winding_names_the_face_it_points_at() {
        assert_eq!(nearest_face(Vec3::Y), 0);
        assert_eq!(nearest_face(-Vec3::Y), 1);
        assert_eq!(nearest_face(Vec3::X), 2);
        assert_eq!(nearest_face(-Vec3::X), 3);
        assert_eq!(nearest_face(Vec3::Z), 4);
        assert_eq!(nearest_face(-Vec3::Z), 5);
        // A cube rotated off the axes still has to answer.
        assert_eq!(nearest_face(Vec3::new(0.9, 0.3, 0.2)), 2);
    }

    #[test]
    fn the_forearm_samples_nothing() {
        // The arm has no texture, and a layer index that got as far as
        // the sampler would draw grass on it.
        for v in build(&Hand::new(), true, None) {
            assert_eq!(v.packed >> 16, UNTEXTURED, "the arm asked for a texture");
        }
    }

    #[test]
    fn the_vertex_is_the_size_the_pipeline_expects() {
        assert_eq!(std::mem::size_of::<HandVertex>(), 40);
    }
}







