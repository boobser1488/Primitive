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
//! of its own, four dozen quads wide. See `vs_held` in `engine/shader.wgsl` and
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
use std::time::{Duration, Instant};

use crate::engine::item_model::{nearest_face, ItemVertex};
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
/// `HELD_UNTEXTURED` in shader.wgsl.
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
const ITEM_SCALE: f32 = 0.78;

/// ...and how big a torch is, which is a third again. See `held_scale`
/// for why an upright picture needs more scale to be the same length.
/// A third again was tried first and is a torch that fills the frame.
const TORCH_SCALE: f32 = ITEM_SCALE * 1.15;
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
/// **The yaw turns the head away from the player**, and that is what it
/// is for. The tool is held to the right of the eye with its head up and
/// to the *left* of its own middle, so a negative yaw -- the sprite
/// turning about the vertical -- carries the head and the edge forward,
/// towards whatever the crosshair is on. At -0.35 that was barely a
/// turn: the axe was held flat to the camera like a card being shown to
/// the player, blade square across the screen, and it read as a picture
/// of an axe rather than as an axe about to hit something. At -0.80 the
/// head leads, the haft falls back to the near hand, and the tool is
/// aimed where it is swung.
///
/// It does not go further than that, and the reason is the sprite: a
/// plate one texel thick seen at a steep angle is a plate seen edge-on,
/// and past about sixty degrees the foreshortening eats the head --
/// which is the half of the picture that says which tool this is. Half a
/// turn is the trade between "aimed" and "still recognisable".
const ITEM_YAW: f32 = -0.80;
const ITEM_ROLL: f32 = 0.55;

/// Where a spear is held: the two ends of its shaft, in view space.
///
/// **A spear is posed by its ends and not by three angles, and that is
/// the whole fix.** Everything above was tuned on the flint tools --
/// pictures of a head over a haft, held *across* the frame -- and the
/// spear went through the same grip because the code's only question
/// was "does this picture have a model?". A two-metre shaft drawn
/// broadside is a plank lying over the bottom-right of the screen, and
/// that is exactly what the player reported: "выглядит как палка".
///
/// What a spear is held like is the one thing the tool grip cannot say:
/// **down the line of sight**, butt at the near hand, point out towards
/// whatever the crosshair is on. That is a statement about two places
/// in the frame, so it is written as two places in the frame, and every
/// angle and scale below is derived from them by
/// [`spear_transform`]. Nothing here has to be kept in step with
/// anything else: move an end and the pose follows.
///
/// The numbers, measured through the hand's own projection
/// (`HAND_FOV_Y`, 70 degrees, at 16:9), which is what
/// `a_spear_is_held_down_the_line_of_sight` checks:
///
/// * the point lands at about x=0.28, y=0.09 in clip space -- right of
///   the crosshair and a little above it: in view, aimed, and *not*
///   over the thing being aimed at, which is the constraint that keeps
///   the head out of the middle;
/// * the butt is off the bottom-right corner entirely, at about
///   x=1.64, y=-2.23 -- a haft leaving the screen past the near hand,
///   the same place the tool grip puts a haft;
/// * and the two are more than a metre of depth apart, which is what
///   makes the thing read as pointing away rather than as lying across
///   the screen. Perspective does the rest: the near end is drawn twice
///   the size of the far one, so a shaft of even thickness comes out a
///   taper, and a taper is a spear.
///
/// **How far off the corner the butt is, is set by the thrust and not
/// by taste.** It was half of this, so that the butt sat exactly *on*
/// the corner at rest -- and the first photograph of a blow showed why
/// that is wrong: a thrust of a quarter of a unit moves a near end
/// through a third of its own depth, and the whole of that is screen
/// travel towards the shaft's vanishing point. The cut end of the haft
/// came up into the middle of the right-hand side of the frame, hanging
/// over a river with nothing holding it -- a spear flying away from the
/// player rather than a spear being driven forward. Started far enough
/// out, it is still below the bottom edge at full extension, which is
/// what `the_cut_end_of_the_haft_never_comes_into_the_frame` measures
/// at every phase of the blow.
const SPEAR_BUTT: Vec3 = Vec3::new(1.02, -0.78, -0.50);
const SPEAR_POINT: Vec3 = Vec3::new(0.565, 0.102, -1.62);

/// **Where a held rod's butt and tip are**, in view space, the spear's way
/// (`laid_along`): stated as the two ends because what is wrong with a rod
/// is where its ends are.
///
/// "удочка повёрнута не так как надо": the rod was held as a tool is, by
/// `item_transform`'s yaw and roll -- which suit a thing a hand chops with
/// and seen broadside -- so the rod stood up the right edge of the frame
/// like a pole and its painted line ran from the tip *up* out of the top of
/// the frame. A rod is held with its butt in the hand low at the right and
/// its length going up and away over the water, the tip in view above and
/// right of the crosshair where the line leaves it. **Forty-five degrees
/// across the screen**, the angle the rod is drawn at in its picture: the
/// line is painted straight down from the tip, so at that angle it hangs
/// straight down on screen too, beside the crosshair and not across it --
/// steeper and it swung in over the middle of the frame. `a_held_rod_points_forward_and_up_with_its_line_hanging_from_the_tip`
/// holds it there. The wind-up and the throw turn the whole grip about the
/// shoulder on top of this (`Rod`), as they did.
const ROD_GRIP: Vec3 = Vec3::new(0.95, -0.62, -0.6);
const ROD_TIP: Vec3 = Vec3::new(0.6, 0.67, -2.4);

/// How wide, across the frame, a carried *material* is drawn: a lump of
/// native copper, a flint nodule, a handful of fibre, a twig.
///
/// **A material is not a tool, and holding it like one is what this
/// number is for.** Everything above was tuned against the three flint
/// tools, whose pictures are a head and a haft drawn corner to corner
/// and whose whole pose depends on the haft leaving the frame. The pose
/// was then handed to *everything that is not a cube*, because that is
/// the question the code asked -- "does this picture have a model?" --
/// and a lump of ore answers yes. Held that way, native copper's
/// ten-by-seven blob covered nearly twelve percent of the whole frame
/// -- an opaque plate of ore lying across the bottom-right corner at
/// fifty-six screen pixels a texel, which is what it was reported as:
/// "a big tilted plane in a green-brown-grey check, right up against
/// the camera".
///
/// Measured across the *silhouette* rather than the sprite -- see
/// `ItemModel::silhouette` -- so a lump, a nodule and a twig are the
/// same size in the hand whatever margins their pictures were drawn
/// with. In view units at `ITEM_CENTRE`'s depth, a quarter is a thing
/// held up in the fingers: read at a glance, and the corner of the
/// screen still shows the world.
const MATERIAL_WIDTH: f32 = 0.26;

/// A held *block* is a cube, and a cube of the same measurement as a
/// sprite reads much larger -- a plate has no bulk. Same trade the
/// dropped-item code makes, in the other direction.
const BLOCK_SCALE: f32 = 0.30;
const BLOCK_YAW: f32 = -0.60;
const BLOCK_PITCH: f32 = 0.20;
/// How long the longest side of a held *model* is -- a barrel, a jug, a bed
/// (see `mesh::carried_model`) -- in the units `BLOCK_SCALE` gives a cube's.
///
/// **Under a held cube's side, not over it**, and the photograph decided
/// that. The first number was 0.36 -- "a model holds less of the frame than a
/// cube of its longest side" -- and through `what_models_look_like_in_the_light`
/// a barrel held at it filled a quarter of the frame's width and ran off its
/// bottom edge, and the jug's handle did the same: the model is held at the
/// middle of the grip, so what grows is the part below it. At this a barrel
/// sits whole in the corner, about the size the cube of planks was.
const MODEL_SPAN: f32 = 0.28;


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
pub(crate) const SWING_SECONDS: f32 = 0.28;
/// Where in that time the blow *lands*, **with nothing in the hand**.
/// Everything before this is the windup and the strike, everything after
/// is the recovery -- which is two and a half times as long, because
/// that is what a swing feels like and an even one feels like waving.
///
/// What is actually held moves this later: see [`impact_at`].
pub(crate) const IMPACT: f32 = 0.30;

/// How much longer a blow takes with the heaviest head in the game than
/// with an empty hand: a third again, so [`SWING_SECONDS`]' 0.28 becomes
/// 0.36 for an iron pick.
///
/// **Why the blow's length carries the weight and not its amplitude
/// alone.** A tool that swung at exactly the speed of a fist and merely
/// travelled further did not read as heavy -- it read as a bigger fist.
/// What an arm actually does with a mass on the end of a haft is take
/// longer to get it up there and longer to stop it, and *time* is the
/// only channel the eye reads mass through.
///
/// **A third and not double.** This is the rhythm of work: a player at a
/// rock face hears and sees a blow three or four times a second, and
/// halving that turns mining from a rhythm into a wait. The difference
/// has to be felt between two tools held one after the other, not
/// suffered for the length of a shift.
const HEFT_SLOWS: f32 = 0.30;
/// ...and how much later in its own blow the heavy head lands: 0.30 of
/// the way through for a fist, 0.42 for an iron pick.
///
/// The windup and the recovery both grow in seconds -- the blow is
/// longer *and* the landing is later in it -- which is the shape of a
/// heavy swing: slow up, and a long time settling afterwards.
const HEFT_LOADS: f32 = 0.12;
/// ...and how much further the arm travels at full heft: a sixth again.
///
/// Small on purpose. The item is held two thirds of a unit from the
/// shoulder, so every extra radian is two thirds of a unit of screen
/// travel; the arc is the seasoning on the timing, not the dish.
const HEFT_ARC: f32 = 0.16;
/// How far the arm drops at the moment of impact, in radians (about 31
/// degrees). See [`SHOULDER`] for why this is not the 50-odd degrees a
/// swing sounds like it should be.
const SWING_PITCH: f32 = 0.30;
/// ...and how far it rolls, and how far it is thrown forward, which are
/// the two things that keep the blow from looking like a hinge.
const SWING_ROLL: f32 = 0.14;
const SWING_REACH: f32 = 0.07;

/// How far a spear is driven forward along its own shaft, in view-space
/// units, at the furthest point of a thrust.
///
/// **A thrust, not a swing.** A pick is swung: the arm turns about a
/// joint and the head comes down in an arc, which is what
/// [`SWING_PITCH`] and [`SWING_ROLL`] describe. Nobody chops with a
/// spear -- the blow is the point going *out* along the shaft and
/// coming back -- and running the arc on a shaft pointed down the line
/// of sight is a spear waving at the sky.
///
/// A fifth of a unit against a butt held half a unit from the eye: the
/// near end goes a third further away and shrinks by a quarter, which
/// in perspective is the whole of the effect -- the shaft foreshortens
/// and the point slides up towards the crosshair.
///
/// **It is bounded, and not by taste.** A thrust moves the near end
/// through a fraction of its own depth, and all of that is screen
/// travel towards the shaft's vanishing point -- so past a certain
/// reach the cut end of the haft climbs into the frame, and a stump of
/// wood hanging over the landscape reads as the spear being thrown
/// away rather than driven forward. That was photographed at a quarter
/// of a unit. This is what the pose above leaves room for; see
/// `the_cut_end_of_the_haft_never_comes_into_the_frame`, which is the
/// test that fails when either number is opened up.
const LUNGE_REACH: f32 = 0.20;
/// How far the point is pulled *back* first, as a share of the reach.
///
/// The load. A thrust with no draw behind it starts at full speed from
/// nowhere and reads as the spear teleporting forward; a short pull
/// back is what says the blow was intended. Kept small because the butt
/// is already near the eye and drawing it much further is a shaft
/// through the player's own face.
const LUNGE_DRAW: f32 = 0.30;
/// Where in the blow the draw is finished, and where the point is
/// furthest out.
///
/// The strike is the stretch between them, and it is the shortest of
/// the three: a fifth of the blow to load, a quarter to drive, and more
/// than half to come back. That ordering is the difference between a
/// jab and a stir, and it is the same shape [`swing_curve`] uses for
/// the same reason -- what an eye reads as force is the *recovery*
/// being slower than the strike.
///
/// `pub(crate)` because another player's thrust is drawn to the same timing
/// (`player_model::thrust_angle`): the point is out at the same fraction of
/// the second on the screen of the player holding the spear and on the screen
/// of the one it is pointed at.
pub(crate) const LUNGE_COCK: f32 = 0.18;
pub(crate) const LUNGE_HIT: f32 = 0.42;

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

/// How long a blow struck with this in hand lasts on screen, in seconds.
///
/// **A spear's thrust is the spear's cooldown, to the frame**, and that is
/// the whole of the player's request that a thrust take a second: the
/// point is drawn, driven and brought home in exactly the time the server
/// will not take another (`combat::swing_seconds`), so the spear coming to
/// rest *is* the spear being ready. At the shared [`SWING_SECONDS`] it was
/// a third of a second of movement and then a still spear the button did
/// nothing with.
///
/// **Everything else keeps the quick blow, and that is not an
/// oversight.** A fist waits 0.6 s and an axe 0.85, and stretching their
/// swings to match would make every swing at a block -- which is most of
/// what this arm ever does -- a slow-motion one, when the blow there is a
/// rhythm of work rather than a promise about the next hit. A thrust has
/// a draw and a recovery a second can be spent on; a swing is a flick,
/// and a second-long flick reads as underwater.
///
/// Blows the digging rhythm starts are [`SWING_SECONDS`] whatever is
/// held, for the reason `Hand::update` gives.
pub fn blow_seconds(held: Option<BlockId>) -> f32 {
    if held.is_some_and(primitive_shared::types::is_weapon) {
        primitive_shared::combat::swing_seconds(held)
    } else {
        dig_seconds(held)
    }
}

/// How heavy what is in the hand feels, 0 for a fist and 1 for the
/// heaviest head in the game.
///
/// **Not a mass in kilograms, and deliberately not one.** Nothing in
/// this game weighs anything -- a stack of forty logs is carried up a
/// mountain -- and inventing a mass table to drive an animation would be
/// a second set of numbers about every block, arguable in every row, for
/// a thing the player can only ever feel as *more* or *less*. What
/// actually decides how a tool swings is the head: what it is made of,
/// and how big a head that work needs. Both of those are already in
/// `blocks::definition`, and reading them is what keeps this honest --
/// a new tool gets a weight from its tier and its work without anybody
/// adding a row.
///
/// **The tiers are not in density order, and that is the point.** A
/// ground stone axe head is a fist-sized lump of rock at 2.6 g/cm³; a
/// copper one is a casting a smith paid for in ore and made no bigger
/// than it had to be, at 8.9. The two come out close, with stone a
/// little ahead -- which is exactly the thing a player notices when they
/// finally hang up the stone axe. Flint is the lightest because a flint
/// tool is a knapped flake lashed to a haft and most of what is in the
/// hand is the haft. Bronze and iron are the heavy end: a smith who can
/// cast bronze can afford a head that does the work in one blow.
///
/// A held *block* is not a tool and is not swung, but it is not nothing
/// either: it is carried, and setting it down has a weight to it.
pub fn heft(held: Option<BlockId>) -> f32 {
    use primitive_shared::blocks::{Tier, Work};
    let Some(id) = held else { return 0.0 };
    let def = primitive_shared::blocks::definition(primitive_shared::types::block_kind(id));
    let Some(tier) = def.tool else {
        // Anything else in the hand: a block, an ingot, a handful of
        // berries. A sixth, which is under the lightest tool there is.
        return 0.15;
    };
    let head: f32 = match tier {
        // A tool that brings no tier with it is a fist with a handle,
        // and weighs what a fist does. Named rather than swept into a
        // catch-all, so a rung added to the ladder fails to compile here
        // instead of silently arriving with somebody else's weight.
        Tier::Hand => 0.0,
        Tier::Flint => 0.34,
        Tier::Stone => 0.52,
        Tier::Copper => 0.60,
        Tier::Bronze => 0.74,
        Tier::Iron => 0.85,
    };
    // ...and how much head the work needs. A pick and a hammer are all
    // head; an axe nearly so; a spade moves earth and is mostly a blade
    // of nothing; a knife is an edge and a grip, and a rod is a stick.
    let shape = match def.work {
        Work::Stone => 1.15,
        Work::Wood => 1.0,
        Work::Ground => 0.7,
        Work::Plant | Work::Any => 0.55,
    };
    (head * shape).clamp(0.0, 1.0)
}

/// How long one blow at a *block* takes with this in hand, in seconds.
///
/// The digging rhythm, and therefore the rhythm the soundscape knocks at
/// (`audio::soundscape::Soundscape::digging`) and the rate the arm
/// starts its next blow at. Both read this rather than
/// [`SWING_SECONDS`], so the knock cannot walk off the blow it belongs
/// to when a heavier tool slows the arm down.
pub fn dig_seconds(held: Option<BlockId>) -> f32 {
    SWING_SECONDS * (1.0 + HEFT_SLOWS * heft(held))
}

/// How far through its own blow the head lands, 0..1.
///
/// See [`IMPACT`] for the shape this is a fraction of, and [`HEFT_LOADS`]
/// for why a heavy head lands later in a blow that is already longer.
pub(crate) fn impact_at(held: Option<BlockId>) -> f32 {
    IMPACT + HEFT_LOADS * heft(held)
}

/// How long after the click a blow with this in hand *lands*, in seconds:
/// for a spear the moment its point is furthest out, [`LUNGE_HIT`] of the
/// thrust, and for everything else the click itself.
///
/// **Held back on the client, because that is where a hit is decided to
/// have happened.** The client says "I struck that" and the server judges
/// reach and rate against its own positions on arrival (see the server's
/// `attack_animal`). So the only way a thrust can land at its peak is for
/// the message to leave at its peak -- and holding it back costs the rate
/// rule nothing, because it measures the gap between messages and every
/// thrust is held back by the same amount. The target is looked for again
/// when the point is out, so a deer that stepped off the line while the
/// spear was drawn is a miss, which is what a thrust is.
///
/// Rejected: the server holding the blow. A timer per player on the
/// authority, landing a hit judged against positions the attacker never
/// saw, and the client would still have to animate the wait. Rejected
/// too: every blow landing at its peak. A pick's impact is 84 ms into its
/// swing, inside a frame of network jitter, and a delay nobody can see is
/// a queue somebody has to maintain.
pub fn impact_seconds(held: Option<BlockId>) -> f32 {
    if held.is_some_and(primitive_shared::types::is_weapon) {
        blow_seconds(held) * LUNGE_HIT
    } else {
        0.0
    }
}

/// When the player may strike, and when a strike already started lands.
///
/// **The frame loop's cooldown, moved here and given a second clock.** It
/// was one `last_swing` instant, checked and reset at the moment a blow
/// was sent -- which was also the moment it landed, so one clock was
/// enough. A thrust starts when it is clicked and lands when the point is
/// out ([`impact_seconds`]), and the two have to be kept apart: the rate
/// counts from the start, so a click during a thrust starts nothing and
/// has no queue to wait in, and the message leaves at the landing.
///
/// Measured against `combat::swing_seconds` exactly, as the old check
/// was. The server allows `COOLDOWN_SLACK_SECS` under that for jitter; the
/// client asks for none of it, so an honest player is never the one
/// leaning on the slack.
#[derive(Default)]
pub struct Strikes {
    /// When the last blow was started.
    started: Option<Instant>,
    /// A thrust on its way out: when it lands, and what was in the hand.
    landing: Option<(Instant, Option<BlockId>)>,
}

/// What one frame of [`Strikes`] decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Beat {
    /// A blow begins: the arm starts moving.
    pub starts: bool,
    /// A blow lands: it is sent at whoever is under the crosshair.
    pub lands: bool,
}

impl Strikes {
    /// Whether a thrust is due to land by `now`, so the caller looks
    /// under the crosshair even with the button let go.
    pub fn due(&self, now: Instant) -> bool {
        self.landing.is_some_and(|(at, _)| now >= at)
    }

    /// One frame. `wanted` is whether the button is held with somebody
    /// to hit under the crosshair, and `held` is what is in the hand.
    pub fn frame(&mut self, now: Instant, wanted: bool, held: Option<BlockId>) -> Beat {
        let mut beat = Beat::default();
        if let Some((at, with)) = self.landing {
            if with != held {
                // Put away mid-thrust. What is in the hand now was not
                // being driven at anybody, and landing the spear's blow
                // with a handful of berries would be a hit nobody made.
                self.landing = None;
            } else if now >= at {
                self.landing = None;
                beat.lands = true;
            }
        }
        let ready = self.started.is_none_or(|last| {
            now.saturating_duration_since(last).as_secs_f32()
                >= primitive_shared::combat::swing_seconds(held)
        });
        if wanted && ready && self.landing.is_none() && !beat.lands {
            self.started = Some(now);
            beat.starts = true;
            let delay = impact_seconds(held);
            if delay > 0.0 {
                self.landing = Some((now + Duration::from_secs_f32(delay), held));
            } else {
                beat.lands = true;
            }
        }
        beat
    }
}

/// One blow under way: how far into it the arm is, and how long it lasts.
///
/// **The length travels with the blow**, because blows are not all one
/// length any more (see [`blow_seconds`]). Read off what is in the hand
/// each frame instead, a player who spun the wheel mid-thrust would see
/// the arm jump to whatever fraction of a pick's blow the spear's elapsed
/// time happened to be.
#[derive(Clone, Copy, Debug)]
struct Blow {
    elapsed: f32,
    length: f32,
    /// Where in this blow the head lands, and how heavy that head is --
    /// taken from what was in the hand when the blow started, for the
    /// same reason `length` is. See [`impact_at`] and [`heft`].
    impact: f32,
    heft: f32,
}

impl Blow {
    /// The blow that what is in the hand makes, from rest.
    fn of(held: Option<BlockId>) -> Blow {
        Blow {
            elapsed: 0.0,
            length: blow_seconds(held),
            impact: impact_at(held),
            heft: heft(held),
        }
    }

    /// The same, at the *digging* rhythm: a spear held against a rock
    /// face chops at a pick's pace rather than jabbing once a second.
    /// See `Hand::update`.
    fn digging(held: Option<BlockId>) -> Blow {
        Blow { length: dig_seconds(held), ..Blow::of(held) }
    }

    /// 0 at the start of the blow and 1 at its end.
    fn phase(self) -> f32 {
        self.elapsed / self.length
    }
}

/// The animation state of the view model. Geometry is built from it and
/// nothing else.
#[derive(Default)]
pub struct Hand {
    /// The current blow, or `None` at rest.
    swing: Option<Blow>,
    /// Advances with distance travelled, so the bob keeps step with the
    /// stride rather than with the frame rate.
    bob_phase: f32,
    /// 0..1, how much of the bob is currently applied.
    bob_blend: f32,
    /// A block set down, a mouthful or a drink under way, and how many
    /// seconds into it. See [`Hand::gesture`].
    act: Option<(Action, f32)>,
    /// The rod: how far it is drawn back, and the throw under way. See
    /// [`Hand::wind_rod`].
    rod: Rod,
    /// The heft of a blow whose head arrived this frame, waiting to be
    /// taken by the frame loop -- see [`Hand::take_landed`].
    landed: Option<f32>,
}

/// **A rod drawn back while the cast is wound up, and whipped forward when
/// it is let go.** The throw used to be a bar under the crosshair and nothing
/// else: the float appeared on the water and the rod in the hand never moved,
/// so a twelve-block cast and a two-block one looked the same from behind the
/// eyes.
///
/// **Driven by the wind-up itself** (`fishing::Hold::charge`), so how far
/// back the rod is *is* how far the float will go -- the gauge and the arm
/// are one number, and a player can learn the throw off either.
///
/// **A turn of the whole grip about the shoulder, not a bend in the rod.**
/// The rod is a picture given thickness (`item_model`); curving it would be a
/// second model of it that the carried, dropped and held copies would all
/// have to agree with. A rod tipped back over the shoulder and snapped
/// forward past level reads as a cast at the size it is drawn; the rest is
/// the float landing where the arm pointed.
#[derive(Clone, Copy, Debug, Default)]
struct Rod {
    /// How far back it is, 0 at rest and 1 at a full wind-up. Eased toward
    /// the wind-up rather than set to it, so letting go of a screen that
    /// cancelled the throw does not snap the rod home.
    wound: f32,
    /// A throw under way: how far back it started from, and how many
    /// seconds into it.
    whip: Option<(f32, f32)>,
}

/// How far a full wind-up takes the rod back over the shoulder, in radians.
/// Enough to put the tip out of the top of the frame -- a rod drawn back is a
/// rod you mostly cannot see -- and not so far the grip leaves the corner.
const ROD_WIND_PITCH: f32 = 0.85;
/// How far past rest the throw carries the rod forward, in radians: the tip
/// pointing down the line at the water for a moment.
const ROD_WHIP_PITCH: f32 = 0.45;
/// How long a throw is, in seconds, and which share of it is the snap
/// forward: a third of it out, the rest easing back to rest.
pub const ROD_WHIP_SECONDS: f32 = 0.55;
pub(crate) const ROD_WHIP_OUT: f32 = 0.3;
/// How fast the rod follows the wind-up, a share of the way a second: quick,
/// so it is where the gauge says, and not a snap.
const ROD_FOLLOW_PER_SEC: f32 = 14.0;

/// The rod's turn about the shoulder at `t` of a throw (0..1) that started
/// `from` of a full wind-up back: from there to past level, fast, and then
/// home. Positive is back over the shoulder.
pub(crate) fn rod_whip_angle(from: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let back = ROD_WIND_PITCH * from.clamp(0.0, 1.0);
    if t < ROD_WHIP_OUT {
        // Squared: the snap accelerates into its end, the way a blow does.
        let k = t / ROD_WHIP_OUT;
        back + (-ROD_WHIP_PITCH - back) * k * k
    } else {
        let k = (t - ROD_WHIP_OUT) / (1.0 - ROD_WHIP_OUT);
        -ROD_WHIP_PITCH * (1.0 - k * k * (3.0 - 2.0 * k))
    }
}

use primitive_shared::protocol::Action;

/// How far the hand goes for each one-off, in view units: forward and down
/// to set a block, in towards the middle and up to the mouth for food, and
/// a little higher for a drink.
const PLACE_REACH: Vec3 = Vec3::new(-0.04, -0.10, -0.12);
const EAT_TO_MOUTH: Vec3 = Vec3::new(-0.34, 0.20, 0.22);
const DRINK_TO_MOUTH: Vec3 = Vec3::new(-0.34, 0.27, 0.20);
/// How far the hand bobs while chewing, in view units.
const CHEW_BOB: f32 = 0.018;

impl Hand {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a blow with `held` in the hand, if one is not already under
    /// way.
    ///
    /// Deliberately *not* a restart. Clicking twice inside a third of a
    /// second is one flurry, and resnapping the arm back to rest in the
    /// middle of it looks like a dropped frame rather than like haste.
    /// Whether a blow is struck at all is [`Strikes`]'s question, and the
    /// frame loop calls this only when it says one starts -- so a click in
    /// the middle of a thrust has nowhere to go and queues nothing.
    pub fn strike(&mut self, held: Option<BlockId>) {
        if self.swing.is_none() {
            self.swing = Some(Blow::of(held));
        }
    }

    /// Acts out a one-off the server has confirmed: a block set down, a
    /// mouthful, a drink. A blow is not one of them -- that is `strike`,
    /// started on the click because it has to land where the crosshair is.
    ///
    /// **The hand did nothing at all for these.** A player ate, and the
    /// only sign was the bar filling; everybody *watching* them saw the
    /// food go to the mouth (`player_model::Arm::Eat`), and the player did
    /// not. Timed by the same seconds the watchers' figure is
    /// (`player_model::EAT_SECONDS` and the rest), so the two screens agree
    /// about how long a mouthful takes. A new one restarts the gesture: the
    /// second mouthful of a hurried meal is a second lift, not nothing.
    pub fn gesture(&mut self, action: Action) {
        if matches!(action, Action::Place | Action::Eat | Action::Drink) {
            self.act = Some((action, 0.0));
        }
    }

    /// Where the wind-up of a cast has got to, once a frame: `Some` with the
    /// charge (0..1) while the button is held with a rod in hand, `None` at
    /// any other time.
    pub fn wind_rod(&mut self, charge: Option<f32>, dt: f32) {
        let wanted = charge.unwrap_or(0.0).clamp(0.0, 1.0);
        let k = 1.0 - (-ROD_FOLLOW_PER_SEC * dt.clamp(0.0, 0.1)).exp();
        self.rod.wound += (wanted - self.rod.wound) * k;
    }

    /// The line has been let go: the rod whips forward from wherever it was
    /// drawn back to.
    pub fn cast_rod(&mut self) {
        self.rod.whip = Some((self.rod.wound, 0.0));
        self.rod.wound = 0.0;
    }

    /// How far the rod is turned back over the shoulder this frame, in
    /// radians; negative while a throw carries it past level.
    pub fn rod_pitch(&self) -> f32 {
        match self.rod.whip {
            Some((from, elapsed)) => rod_whip_angle(from, elapsed / ROD_WHIP_SECONDS),
            None => ROD_WIND_PITCH * self.rod.wound,
        }
    }

    /// Holds the blow at one phase of itself, for a photograph.
    ///
    /// **A blow is a third of a second long and starts when the player
    /// clicks**, so a screenshot taken at a chosen second can only ever
    /// catch a hand at rest -- and every pose *inside* a blow, which is
    /// where an animation is right or wrong, was reachable only with a
    /// finger on a mouse button. The animation is a pure function of
    /// elapsed time, so setting the time is the whole of what a tool
    /// needs. See `PRIMITIVE_SWING` in `lib.rs`.
    ///
    /// `phase` is 0 at rest and 1 at the end of the blow. It changes
    /// the arm and nothing else: no block comes apart, nothing is sent
    /// to the server.
    pub fn freeze(&mut self, phase: f32) {
        // At the length of the blow under way, if there is one, so a
        // photograph of a thrust is a phase of the spear's second; at
        // rest, the quick blow's. The pose reads only the fraction, so
        // either is the same picture.
        let held = self.swing.unwrap_or(Blow { elapsed: 0.0, length: SWING_SECONDS, impact: IMPACT, heft: 0.0 });
        self.swing = Some(Blow { elapsed: phase.clamp(0.0, 1.0) * held.length, ..held });
    }

    /// Advances the animation by one frame.
    ///
    /// `digging` is whether the player is currently making progress on a
    /// block: while they are, the arm swings again the moment it comes
    /// back to rest, which is what turns a click into a rhythm. `speed`
    /// is horizontal speed in blocks per second and `grounded` whether
    /// the player is actually on the floor -- the bob is a footfall, and
    /// there are none in mid-air. `held` is what is in the hand *now*,
    /// which is what the next blow of a rhythm weighs -- swapping a fist
    /// for an iron pick mid-dig slows the arm from the next blow on
    /// rather than from the next click.
    pub fn update(&mut self, dt: f32, digging: bool, speed: f32, grounded: bool, held: Option<BlockId>) {
        // A stall must not throw the arm through a whole blow in one
        // step; the same clamp physics and the camera shake use.
        let dt = dt.clamp(0.0, 0.1);

        match self.swing {
            Some(blow) => {
                let elapsed = blow.elapsed + dt;
                // **The head arrives once per blow, and this is where
                // that is noticed.** Nothing else in the client knows
                // when a swing is *down* -- the click is up to a third
                // of a second earlier, and for a heavy tool further
                // still. The recoil reads this; see `Shake::on_blow`.
                if blow.elapsed < blow.impact * blow.length
                    && elapsed >= blow.impact * blow.length
                {
                    self.landed = Some(blow.heft);
                }
                self.swing = if elapsed < blow.length {
                    Some(Blow { elapsed, ..blow })
                } else if digging {
                    // Carry the overshoot into the next blow rather than
                    // starting it from zero, or the rhythm slows to
                    // whatever the frame rate happens to be.
                    //
                    // **At the digging rhythm, whatever the last blow
                    // was.** The next one is a swing at a block, and a
                    // block comes apart at the pace its cracks spread and
                    // the soundscape knocks: a spear held while digging
                    // jabs at a pick's rate, rather than once a second
                    // while the cracks run four times as fast. That pace
                    // is `dig_seconds` and not a constant, because what
                    // is in the hand has a weight -- see [`HEFT_SLOWS`].
                    Some(Blow { elapsed: elapsed - blow.length, ..Blow::digging(held) })
                } else {
                    None
                };
            }
            None if digging => self.swing = Some(Blow::digging(held)),
            None => {}
        }

        self.act = self.act.and_then(|(action, elapsed)| {
            let elapsed = elapsed + dt;
            (elapsed < act_seconds(action)).then_some((action, elapsed))
        });
        self.rod.whip = self.rod.whip.and_then(|(from, elapsed)| {
            let elapsed = elapsed + dt;
            (elapsed < ROD_WHIP_SECONDS).then_some((from, elapsed))
        });

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
            Some(blow) => swing_curve(blow.phase(), blow.impact),
            None => 0.0,
        }
    }

    /// The heft of a blow whose head arrived since this was last asked,
    /// or `None`.
    ///
    /// **Taken rather than read**, so one blow is one recoil however
    /// many times a frame loop asks. A blow that landed during a stall
    /// is still reported: the frame was long, the arm went through the
    /// impact inside it, and a view that did not flinch would be the one
    /// dropped frame a player actually notices.
    pub fn take_landed(&mut self) -> Option<f32> {
        self.landed.take()
    }

    /// How far the arm travels this frame as a share of a fist's, which
    /// is 1: a heavier head is swung further. See [`HEFT_ARC`].
    fn arc(&self) -> f32 {
        1.0 + HEFT_ARC * self.swing.map_or(0.0, |blow| blow.heft)
    }

    /// The same blow read as a *thrust*: 0 at rest, 1 at full
    /// extension, and briefly negative while the point is drawn back.
    ///
    /// **One clock, two shapes.** The blow is timed in one place --
    /// `update` counts seconds against the blow's own length and nothing
    /// else -- and what differs between a pick and a spear is how that
    /// fraction is turned into a pose.
    ///
    /// **And one duration per weapon, which this comment used to argue
    /// against.** The fear was a second number drifting from the repeat
    /// rate, and it was the wrong fear: the thrust was a third of a second
    /// against a spear the server would not take again for 0.9, so the
    /// spear jabbed and then stood still, which is the drift. The length
    /// is now *read from* the repeat rate (`blow_seconds`), so the two
    /// cannot come apart.
    pub fn lunge(&self) -> f32 {
        match self.swing {
            Some(blow) => lunge_curve(blow.phase()),
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
        // Which picture of fire to draw over the head of a lit torch,
        // or `None` for everything that is not one.
        //
        // Handed in already chosen rather than worked out here, because
        // choosing it needs a clock and this function has none -- and
        // the clock it would need is the one the campfire is already
        // animated by, so the flame in the hand and the flame in the
        // ring are the same flame at the same moment. See the caller.
        flame: Option<u32>,
        vertices: &mut Vec<HandVertex>,
        indices: &mut Vec<u32>,
    ) {
        self.build_into_as(shown, held, layers, models, light, flame, true, vertices, indices);
    }

    /// `build_into`, told whether a block with a model of its own is held as
    /// that model.
    ///
    /// **`false` is how such a block was held before it was**, kept reachable
    /// so one binary can photograph both (`model_light_repro`): what it falls
    /// through to is the sprite and the cube every other held thing still
    /// gets, so it draws the old picture rather than a copy of it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_into_as(
        &self,
        shown: bool,
        held: Option<BlockId>,
        layers: &FaceLayers,
        models: Option<&TextureManager>,
        light: (u8, u8),
        flame: Option<u32>,
        carried_models: bool,
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

        let group = self.pose(block);

        // --- no arm ---
        //
        // **There is no forearm, and that is the fix rather than a
        // shortcut.** Every version of one was a flat-shaded box a foot
        // from the eye, and a box is what it looked like: first a plank
        // of skin colour across a sixth of the frame, then a stump in
        // the corner, then a fist that read as a brown pebble glued to
        // the haft. The arm had nothing to be made of except a solid
        // colour -- and a solid-colour cuboid does not become an arm at
        // any size.
        //
        // **Half of that is no longer true**, and the next person to
        // want an arm here should know it: there is a character model
        // now, and a skin drawn for it, with a sleeve and a hand already
        // on the sheet (`logic::player_model`, `assets/textures/players`
        // -- the same picture other players wear). What is still true is
        // the argument below, which was never about the missing texture.
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
        //
        // **What the block says about its own colour**, which for
        // everything but a garment is nothing. Twelve pieces of armour
        // share four greyscale pictures and are told apart by a tint
        // alone (see `types::garment_tint`), and the hand was passing
        // white: a bronze cuirass was drawn in the pack in bronze and
        // held in the hand in the raw grey of the leather one. Three
        // materials, one picture, and the only thing that distinguished
        // them switched off in the one place the player is looking.
        //
        // Through `hotbar::icon_tint` rather than a second reading of
        // `garment_tint`, so the hand and the pack cannot come to
        // different answers about the same shirt.
        let tint = crate::ui::hotbar::icon_tint(block, [1.0; 4]);
        // **A block with a model of its own is held as that model** -- a
        // barrel as staves round water, a jug with its handle standing off
        // the belly -- and not as the sprite cut from its icon. See
        // `mesh::carried_model` for the report and the choice. Held the way
        // a cube is, turned to show a top and two sides, and sized by its
        // longest side so that a bed and a jug are both things in a hand.
        if carried_models && crate::engine::mesh::has_carried_model(block) {
            let mut model = (Vec::new(), Vec::new());
            crate::engine::mesh::carried_model(block, layers, &mut model.0, &mut model.1);
            let (low, high) = crate::engine::mesh::extent(&model.0);
            let transform = group
                * Mat4::from_translation(ITEM_CENTRE)
                * Mat4::from_rotation_y(BLOCK_YAW)
                * Mat4::from_rotation_x(BLOCK_PITCH)
                * Mat4::from_scale(Vec3::splat(MODEL_SPAN / (high - low).max_element().max(1.0 / 16.0)))
                * Mat4::from_translation(-(low + high) * 0.5);
            let mut placed = Vec::with_capacity(model.0.len());
            let mut placed_indices = Vec::with_capacity(model.1.len());
            crate::engine::mesh::place_carried(&model.0, &model.1, transform, light, &mut placed, &mut placed_indices);
            // Every model emitter writes quads -- four corners, then their two
            // triangles -- so the quads are the vertices four at a time.
            for quad in placed.chunks_exact(4) {
                let corners = std::array::from_fn(|k| Vec3::from_array(quad[k].position));
                let uv = std::array::from_fn(|k| quad[k].uv());
                // The fire the model carries is already kept in this word.
                let word = quad[0].light();
                let lit = ((word & 15) as u8, ((word >> 4) & 15) as u8);
                push_quad(vertices, indices, corners, uv, quad[0].tex_layer(), tint, lit);
            }
            return;
        }
        match models.and_then(|models| models.item_model(block)) {
            // A tool: the sprite with a thickness that the dropped-item
            // code already knows how to build. Same model, same texture,
            // a different transform -- which is the whole reason
            // `append_transformed` exists.
            Some(model) => {
                let transform = held_transform(group, block, model);
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
                    push_quad(vertices, indices, corners, uv, layer, tint, light);
                }
                // **The flame, and it is the campfire's own.** A torch
                // burning in the hand is the one thing in this game a
                // player looks at for minutes at a time, and a still
                // picture of fire is the one thing fire never is.
                //
                // A quad rather than six more pictures of the torch:
                // the fire is already drawn, already animated, and
                // already in the atlas, so this costs no layer at all
                // and cannot come to disagree with the fire in the ring
                // of stones. It is flat and faces the eye, which is what
                // every flame in every game of this kind is -- there is
                // nothing to be gained by giving a flame a back.
                if let Some(flame) = flame {
                    // **Laid on the torch, in the torch's own space.**
                    // Every corner of this quad comes out of two
                    // measurements of two pictures -- where the fire is
                    // drawn inside its tile, and where the wad is drawn
                    // inside the torch's -- and goes through the same
                    // matrix as the sprite. So the fire is in the
                    // plate's own plane, at the plate's own scale and
                    // angle, covering exactly the texels it is meant to
                    // cover. There is nothing here to tune and nothing
                    // to drift: redraw either picture and the
                    // measurements above are what changes.
                    //
                    // Two earlier versions were billboards -- one
                    // upright in the eye's space, one rolled along the
                    // stick -- and both were a fire sized and placed by
                    // hand against a sprite whose size and place come
                    // from a matrix. Every setting that covered the
                    // wad's tip hung the fire's base out past the
                    // stick, and every setting that did not left the
                    // wad's shoulders showing. That is what guessing
                    // looks like when the two are in different spaces.
                    let tile = flame_tile_on_sprite();
                    // Texels to the sprite's own -0.5..0.5, the same
                    // conversion `item_model` builds its quads with.
                    //
                    // **z is nought: the middle of the plate, not the
                    // front of it.** It was `+FLAME_NEAR`, a plate's
                    // thickness proud of the face, and the player's
                    // report was that the fire is not on the torch --
                    // which is exactly what that is, and it is worse
                    // than it sounds. The torch is *held turned*: the
                    // plate's normal is about forty-six degrees off the
                    // line of sight (`ITEM_YAW`), so an offset along it
                    // is mostly an offset **across the screen**.
                    // Photographed through the real pipeline at 960 by
                    // 540 (`the_hand_through_the_real_pipeline`), the
                    // fire's centre sat nine pixels to the left of the
                    // head it is drawn to cover, and 437 pixels of bare
                    // torch showed beside it above the binding. Moving
                    // it to the middle takes that to 150 -- what is
                    // left is the plate's own one-texel rim, which is a
                    // hair of fibre at the edge of a flame and not a
                    // slice of stick beside one.
                    //
                    // The middle of the plate is the one depth at which
                    // that shift is zero, whatever the torch is turned
                    // to, because it is the depth the wad itself is at.
                    let at = |x: f32, y: f32| {
                        Vec3::new(x / RESOLUTION - 0.5, 0.5 - y / RESOLUTION, 0.0)
                    };
                    let corners = drawn_in_front_of_the_plate(
                        [
                            transform.transform_point3(at(tile[0], tile[3])),
                            transform.transform_point3(at(tile[2], tile[3])),
                            transform.transform_point3(at(tile[2], tile[1])),
                            transform.transform_point3(at(tile[0], tile[1])),
                        ],
                        transform,
                    );
                    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
                    // Full light: a flame is not lit by the room, it is
                    // what is lighting the room. Handing it the cell's
                    // own light would put the fire out in the dark,
                    // which is the one place it has to be brightest.
                    push_quad(vertices, indices, corners, uv, flame, [1.0; 4], (15, 15));
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
                append_box(vertices, indices, transform, Some((block, layers)), tint, light);
            }
        }
    }

    /// This frame's pose of the arm for *what is in it*.
    ///
    /// Two poses and not one, because a blow is not one movement. See
    /// [`Self::thrust`]: a spear is driven along its own shaft and
    /// everything else is swung about a shoulder, and the arc run on a
    /// shaft that points down the line of sight is a spear waving at
    /// the sky rather than stabbing.
    ///
    /// Public because the pose tool draws it -- see
    /// `ui::snapshot::hand_pose`, which photographs the blow at
    /// several phases and would otherwise be photographing a pose the
    /// game does not use.
    pub fn pose(&self, held: BlockId) -> Mat4 {
        if primitive_shared::types::is_weapon(held) {
            self.thrust()
        } else if primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_FISHING_ROD {
            // The rod turns about the same shoulder a blow does, on top of
            // everything else the arm is doing. See `Rod`.
            Mat4::from_translation(SHOULDER)
                * Mat4::from_rotation_x(self.rod_pitch())
                * Mat4::from_translation(-SHOULDER)
                * self.group()
        } else {
            self.group()
        }
    }

    /// The pose of a hand that is *stabbing*: the whole grip slid along
    /// the shaft and back, and nothing turned at all.
    ///
    /// **No rotation, deliberately.** The shoulder arc that makes a
    /// pick land is a rotation about a joint below the frame, and a
    /// spear held down the line of sight has its length along the axis
    /// that arc sweeps -- so a quarter of the pick's pitch throws the
    /// point clean off the top of the screen while moving it barely a
    /// hand's breadth forward. A thrust is a translation, and the
    /// perspective divide turns it into the whole of the animation: the
    /// near end shrinks as it goes, which is what a thing moving away
    /// from an eye does.
    fn thrust(&self) -> Mat4 {
        Mat4::from_translation(self.carried_offset() + spear_shaft() * (LUNGE_REACH * self.lunge()))
    }

    /// This frame's pose of the whole arm, before anything is placed in
    /// it.
    ///
    /// Everything the hand does happens about the shoulder, so the
    /// swing is built once and both parts ride it. Read right to left:
    /// the parts are placed, thrown forward, rotated about the joint,
    /// and finally nudged by the stride.
    ///
    /// Its own function so that a test can build the same pose the
    /// frame does and compare a quad against it. A test that rebuilt
    /// these six matrices from the constants would be a second copy of
    /// the pose, and a second copy is a thing that agrees with the
    /// first until somebody changes one of them.
    pub fn group(&self) -> Mat4 {
        // The weight of what is held reaches the pose here and nowhere
        // else, so the arm, the tool and the forearm cannot disagree
        // about how far the blow went.
        let swing = self.swing() * self.arc();
        Mat4::from_translation(self.carried_offset())
            * Mat4::from_translation(SHOULDER)
            * Mat4::from_rotation_x(-SWING_PITCH * swing)
            * Mat4::from_rotation_z(SWING_ROLL * swing)
            * Mat4::from_translation(-SHOULDER)
            * Mat4::from_translation(Vec3::new(0.0, 0.0, -SWING_REACH * swing))
    }

    /// Where the stride has carried the hand this frame.
    ///
    /// Vertical at twice the rate of horizontal, so the hand traces a
    /// flattened figure of eight -- one dip per footfall, one sway per
    /// pair of them. The same shape the camera bob traces, and for the
    /// same reason: a hand that swayed without dipping would look like
    /// it was being waved.
    /// Where the stride and the one-off under way have carried the hand.
    fn carried_offset(&self) -> Vec3 {
        self.bob_offset() + self.act_offset()
    }

    /// Where a block set down, a mouthful or a drink has taken the hand.
    fn act_offset(&self) -> Vec3 {
        use crate::logic::player_model::held_up;
        let Some((action, elapsed)) = self.act else {
            return Vec3::ZERO;
        };
        let t = (elapsed / act_seconds(action)).clamp(0.0, 1.0);
        match action {
            Action::Place => PLACE_REACH * (std::f32::consts::PI * t).sin(),
            Action::Eat => {
                EAT_TO_MOUTH * held_up(t)
                    + Vec3::Y * (CHEW_BOB * held_up(t) * (t * 4.0 * std::f32::consts::TAU).sin())
            }
            Action::Drink => DRINK_TO_MOUTH * held_up(t),
            _ => Vec3::ZERO,
        }
    }

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

/// How long a one-off lasts in the player's own hand: exactly as long as it
/// does on everybody else's screen.
fn act_seconds(action: Action) -> f32 {
    use crate::logic::player_model::{DRINK_SECONDS, EAT_SECONDS, PLACE_SECONDS};
    match action {
        Action::Place => PLACE_SECONDS,
        Action::Eat => EAT_SECONDS,
        Action::Drink => DRINK_SECONDS,
        _ => 0.0,
    }
}

/// Where the wad sits on a torch, in the item model's own space.
///
/// One side of the sprite grid the torch and its fire are drawn on.
///
/// The pictures are square and this is their edge in texels, which is
/// what every measurement below is written in. `item_model` maps the
/// same grid to -0.5..0.5.
pub(crate) const RESOLUTION: f32 = 16.0;

/// Where the fire is actually drawn inside its own tile: left, top,
/// right, bottom, in texels, over all six frames.
///
/// **Measured, not assumed.** The tile is sixteen wide and the drawing
/// uses seven of them, and the difference is the whole reason the first
/// attempts at this could not be made to work by choosing numbers: a
/// quad sized to the tile is not a fire sized to anything. See
/// `generate_torch_flame`, which draws it.
const FLAME_ART: [f32; 4] = [4.0, 2.0, 11.0, 16.0];

/// ...and where the wad is drawn on the torch, in the same texels.
///
/// Four across and seven deep, ending at the binding. From
/// `generate_torch`; move the head in that drawing and this moves with
/// it, and the fire follows without anything else being touched.
const WAD_FACE: [f32; 4] = [6.0, 2.0, 10.0, 9.0];

/// How far the flame reaches past the wad, in texels of the same grid:
/// over the top, out to either side, and down past the binding.
///
/// **These are the smallest values that cover the head on every frame,
/// and that was computed rather than chosen.** The fire is six pictures
/// of different heights and widths; laying the tallest of them over the
/// wad says nothing about the shortest, and the shortest is the one
/// that used to leave the wad's shoulders showing past the flame --
/// which is what "the edge of the torch is still visible" meant. Every
/// drawn texel of the head is checked against every drawn texel of
/// every frame by
/// `every_frame_of_the_fire_covers_the_whole_head_of_the_torch`. Three
/// Three quarters of a texel aside leaves two texels of the wad bare on
/// the worst frame; a whole one leaves none.
/// **`FLAME_BELOW` is one texel, and it was three for a while.**
/// Sunk into the stick the fire comes *out of* the torch rather than
/// sitting on it, which reads better -- and it costs width, because the
/// deeper the base goes the higher up its own tapering tongue the wad
/// sits and the wider the tongue has to be drawn for its narrow part to
/// still cover the head. Three texels down took a flame seven texels
/// across; one texel takes six. The fire belongs at the end of the
/// torch and the stick belongs in sight, so one texel is what it is:
/// just enough that the drawing's foot laps over the binding.
///
/// **The side is what binds, and the height comes free.** The scale is
/// one number for both axes, and the fire is drawn twice as tall as it
/// is wide, so whatever width the head needs already buys more height
/// than the head has. `FLAME_ABOVE` is therefore not what settles the
/// covering any more -- it only says how far the tongue stands proud of
/// the fibre, which is a matter of what a burning wad looks like.
///
/// They are margins on `WAD_FACE` rather than absolute positions, so a
/// redrawn head moves the fire with it -- and the head *was* redrawn
/// for this: rounding the wad's two square shoulders was worth a texel
/// of margin in every direction, which is a quarter off the width of
/// the flame the player sees. See `generate_torch`.
const FLAME_ABOVE: f32 = 1.0;
const FLAME_SIDE: f32 = 1.0;
const FLAME_BELOW: f32 = 1.0;

/// Where the fire's whole tile lands on the torch's sprite, in that
/// sprite's texels: left, top, right, bottom.
///
/// Its own function because two things need the same answer and must
/// not drift: the quad `build_into` lays on the torch's face, and the
/// test that checks the fire actually covers the fibre. It is all
/// arithmetic on the two measurements above and has no state, which is
/// what makes that test worth anything.
pub(crate) fn flame_tile_on_sprite() -> [f32; 4] {
    // The rectangle the *drawn* fire has to fill.
    let want = [
        WAD_FACE[0] - FLAME_SIDE,
        WAD_FACE[1] - FLAME_ABOVE,
        WAD_FACE[2] + FLAME_SIDE,
        WAD_FACE[3] + FLAME_BELOW,
    ];
    // **One scale for both axes**, and that is not tidiness. Fitting
    // the drawing to the rectangle axis by axis stretches it, and a
    // stretched flame is the fault it looks like: the tongue is drawn
    // seven texels across and fourteen deep, the rectangle wanted eight
    // by ten and a half, and the fire came out squashed into an orange
    // slab lying along the stick. Whichever axis needs the most is what
    // both get, so the fire keeps the shape it was drawn in and the
    // other axis simply reaches further than it had to.
    let scale = ((want[2] - want[0]) / (FLAME_ART[2] - FLAME_ART[0]))
        .max((want[3] - want[1]) / (FLAME_ART[3] - FLAME_ART[1]));
    // Centred across the wad, and standing on the bottom of the
    // rectangle: those are the two edges that have to be right. The
    // fire's own base is the tile's bottom row, so aligning the bottom
    // is what puts the flame on the fibre rather than beside it.
    let middle = (WAD_FACE[0] + WAD_FACE[2]) / 2.0;
    let art_middle = (FLAME_ART[0] + FLAME_ART[2]) / 2.0;
    let x0 = middle - art_middle * scale;
    let y1 = want[3] + (RESOLUTION - FLAME_ART[3]) * scale;
    [x0, y1 - RESOLUTION * scale, x0 + RESOLUTION * scale, y1]
}

/// Moves a quad toward the eye until it clears the plate it lies in,
/// **without moving it a pixel on screen**.
///
/// ## The problem this solves
///
/// The fire belongs at the plate's own mid-depth -- that is where the
/// wad it is a picture of actually is, and it is the only depth at
/// which the fire lands on the head from every angle the swing turns
/// the torch to. But the pipeline it goes through writes depth and
/// tests it with `Less` (see `hand_pipeline`) and the sprite is a
/// *cutout*: at the mid-plane the plate's own front face is half a
/// thickness nearer the eye and opaque over exactly the texels the fire
/// is drawn to cover, so the fire would be hidden behind the very thing
/// it is meant to be burning on.
///
/// ## Why toward the eye and not along the plate's normal
///
/// The plate's normal is not the line of sight -- the torch is held
/// turned by `ITEM_YAW`, some forty-six degrees -- so a step along it
/// moves the quad sideways on screen as well as nearer. That is the
/// whole of the fault this replaces: `FLAME_NEAR`, which is what used
/// to be here, bought its depth clearance with a visible sideways
/// shift, and the fire came out beside the torch rather than on it.
///
/// The hand is built in view space with **the eye at the origin** (see
/// `vs_held` in shader.wgsl), so scaling the quad about the origin slides every
/// corner along its own ray from the eye. A perspective projection
/// cannot see that: the screen position of every corner is unchanged to
/// the last bit, the quad stays planar and keeps its winding, and all
/// that changes is the depth. It is the one direction that is free.
///
/// ## How far
///
/// `CLEARANCE` plate thicknesses, taken from the transform rather than
/// written down, so a change of scale cannot leave a fixed number
/// behind. Two is not tuning: the quad is longer than it is far from
/// the eye is deep, so its far corner is pulled proportionally less
/// than its near one, and the plate is seen at an angle so the
/// half-thickness it has to clear costs more than half a thickness
/// along the ray. Both are measured at thirteen points of a blow by
/// `the_fire_is_drawn_where_the_middle_of_the_torch_is`.
fn drawn_in_front_of_the_plate(corners: [Vec3; 4], transform: Mat4) -> [Vec3; 4] {
    let middle = (corners[0] + corners[1] + corners[2] + corners[3]) / 4.0;
    let distance = middle.length();
    // A quad at the eye has no ray to slide along. It cannot happen --
    // the hand is held most of a metre out -- and dividing by it would
    // be a NaN in a vertex buffer, which draws nothing anywhere.
    if distance < 1e-4 {
        return corners;
    }
    let thickness = transform
        .transform_vector3(Vec3::Z * crate::engine::item_model::THICKNESS)
        .length();
    // One factor for all four corners, so the quad stays a plane: four
    // corners each moved the same *distance* toward the eye is a
    // bilinear patch with a crease down the middle of it.
    let scale = (1.0 - CLEARANCE * thickness / distance).max(0.0);
    corners.map(|corner| corner * scale)
}

/// How many plate thicknesses the fire is slid toward the eye. See
/// `drawn_in_front_of_the_plate`.
const CLEARANCE: f32 = 2.0;

/// Where a held *tool* sits, under the swing group.
///
/// Its own function because two things need it and they must not drift
/// apart: the model built for the screen, and the pose tool that draws
/// the same transform into a picture -- see `ui::snapshot::hand_pose`.
/// A grip is four angles that only mean anything together, and the only
/// honest way to tune one is to look at it.
pub fn held_transform(group: Mat4, block: BlockId, model: &crate::engine::item_model::ItemModel) -> Mat4 {
    // A spear is not held the way a tool is held, and it is the only
    // thing here that is posed by where its *ends* go rather than by
    // angles. See `spear_transform`.
    if primitive_shared::types::is_weapon(block) {
        return spear_transform(group, model);
    }
    if primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_FISHING_ROD {
        return laid_along(group, model, ROD_GRIP, ROD_TIP, true);
    }
    item_transform(group, held_scale(block, model))
        * Mat4::from_translation(Vec3::Y * held_lift(block))
}

/// Which way a spear points, in view space, and how long it is.
///
/// Its own function because three things need the same answer and must
/// not drift: the grip, the direction a thrust travels in, and the test
/// that says the two are the same line. A thrust along anything but the
/// shaft is a spear sliding sideways through the air.
fn spear_shaft() -> Vec3 {
    (SPEAR_POINT - SPEAR_BUTT).normalize()
}

/// A spear laid along [`SPEAR_BUTT`]..[`SPEAR_POINT`].
///
/// ## Why this one is built from a segment and not from Euler angles
///
/// Every other grip in this file is a yaw and a roll chosen by looking
/// at a picture, and that works because those pictures are held
/// broadside: what the angles decide is a lean, and a lean is easy to
/// judge and easy to nudge. A spear is held pointing *away*, and there
/// the same three numbers decide something else entirely -- where the
/// point lands on screen -- through a perspective divide. Nudging a yaw
/// to move a point that is a metre and a half down the view axis is
/// aiming by arithmetic nobody can do in their head, and the two
/// earlier attempts at exactly that are what "выглядит как палка" was
/// a report of.
///
/// So the pose is stated as the two things that can actually be judged
/// -- where the butt is and where the point is -- and everything else
/// is derived:
///
/// * **the scale** is the segment's length over the drawing's own
///   length, so a spear is exactly as long as it was asked to be
///   whatever margins the sprite was drawn with (`ItemModel::drawn_box`
///   -- the same measurement `held_scale` uses for a lump of ore);
/// * **the direction** is the segment;
/// * **the roll about the shaft is not chosen at all.** A sprite is a
///   plate one texel thick, so a roll that turns its face away from the
///   eye is a spear that disappears -- and once the shaft points down
///   the line of sight there is exactly one roll that shows the most of
///   it: the one whose plate normal is the part of "towards the eye"
///   that is perpendicular to the shaft. That is what is computed here,
///   and `the_flat_of_the_spear_is_turned_as_far_towards_the_eye_as_the_shaft_allows`
///   checks against thirty-two other rolls that none of them shows
///   more.
fn spear_transform(group: Mat4, model: &crate::engine::item_model::ItemModel) -> Mat4 {
    laid_along(group, model, SPEAR_BUTT, SPEAR_POINT, false)
}

/// A drawing laid corner to corner along `butt`..`point` in view space, its
/// flat turned as far towards the eye as the shaft allows -- the spear's
/// grip, and the rod's (see [`spear_transform`] for every step of it).
///
/// `lower_right_down`: turn the plate over, if it must, so the part of the
/// picture drawn below and right of the diagonal hangs *below* it on screen.
/// A rod's line is drawn there, hanging from the tip. Either face of the
/// plate turned to the eye shows the same width of it, so this costs the
/// spear's argument nothing -- and the other face was the line standing up
/// out of the tip into the top of the frame.
fn laid_along(
    group: Mat4,
    model: &crate::engine::item_model::ItemModel,
    butt: Vec3,
    point: Vec3,
    lower_right_down: bool,
) -> Mat4 {
    let (low, high) = model.drawn_box();
    // The drawing's own long axis: the diagonal of what is actually
    // drawn, which for the spear is butt in one corner and point in the
    // other -- see `a_spear_is_drawn_corner_to_corner_with_its_head_at_the_top`,
    // which is what says this picture may be read this way at all.
    let drawn = Vec3::new(high[0] - low[0], high[1] - low[1], 0.0);
    // A picture with nothing in it cannot be laid along anything. The
    // texture loader throws those away before they get here; the floor
    // is one texel, so this divides by a sixteenth at worst instead of
    // by zero.
    let drawn_length = drawn.length().max(1.0 / RESOLUTION);
    let drawn_axis = drawn / drawn_length;
    // The other axis of the picture's own plane, right-handed with the
    // sprite's +Z: turning the drawing must not mirror it, or every
    // face of the plate ends up wound inside out and the pipeline culls
    // the side facing the player.
    let drawn_across = Vec3::new(-drawn_axis.y, drawn_axis.x, 0.0);
    let drawn_middle = Vec3::new((low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, 0.0);

    let shaft = point - butt;
    let along = shaft.normalize();
    let middle = (butt + point) * 0.5;
    // The plate's normal: "towards the eye" with the part along the
    // shaft taken out of it.
    //
    // **Towards the eye, not down +z.** The eye is the origin of this
    // space, so the way back to it from the middle of the spear is
    // `-middle` -- and the spear is held a good half-unit off to the
    // right, so that is twenty-seven degrees away from the view axis.
    // Rolling to face +z instead showed 0.399 of the plate where the
    // best roll shows 0.461: a sixth of the width of the thing thrown
    // away by using the axis the camera looks down in place of the
    // direction the spear is actually seen from. The test caught it.
    let towards_eye = -middle.normalize();
    let normal = towards_eye - along * along.dot(towards_eye);
    // Only if the shaft points exactly at the eye, which no setting of
    // the two ends above can produce -- but a NaN basis is a hand that
    // draws nothing at all, and that is worth two lines.
    let normal = if normal.length_squared() > 1e-6 {
        normal.normalize()
    } else {
        Vec3::X
    };
    // `drawn_across` is up and left of the diagonal in the picture, so the
    // lower right lies along `-across` once laid: that has to point down.
    let normal = if lower_right_down && normal.cross(along).y < 0.0 { -normal } else { normal };
    let across = normal.cross(along);

    // The rotation that carries the picture's own basis onto that one.
    // Both are right-handed and orthonormal, so this is a rotation and
    // not a reflection -- see `drawn_across`.
    let rotation = glam::Mat3::from_cols(along, across, normal)
        * glam::Mat3::from_cols(drawn_axis, drawn_across, Vec3::Z).transpose();
    group
        * Mat4::from_translation(middle)
        * Mat4::from_mat3(rotation)
        * Mat4::from_scale(Vec3::splat(shaft.length() / drawn_length))
        // The *drawing* is centred on the segment, not the picture: a
        // margin at one corner would otherwise slide the whole spear
        // down its own axis, which is the same fault `held_scale` fixed
        // for a lump of ore.
        * Mat4::from_translation(-drawn_middle)
}

/// How far up its own length a held thing is slid before it is drawn.
///
/// **Nought for everything but a torch**, and for a torch it is what
/// makes the stick visible. `ITEM_CENTRE` puts the *middle* of a
/// picture at the hand, which is right for a tool -- an axe is held
/// about its middle -- and wrong for a stick a third longer than that,
/// held by its end: lengthening the torch pushed its butt off the
/// bottom corner of the frame, so the player got a flame and no torch
/// under it. Sliding it up its own axis brings the end back into view
/// without moving the fire off the corner or touching the grip angles.
///
/// In the sprite's own units, where the whole picture is one.
fn held_lift(block: BlockId) -> f32 {
    if primitive_shared::types::is_torch(block) {
        TORCH_LIFT
    } else {
        0.0
    }
}

const TORCH_LIFT: f32 = 0.22;

/// The grip alone, without the slide `held_lift` adds.
///
/// Still its own function because two things need the same angles and
/// must not drift apart, and `held_transform` is built on it.
pub fn item_transform(group: Mat4, scale: f32) -> Mat4 {
    group
        * Mat4::from_translation(ITEM_CENTRE)
        * Mat4::from_rotation_y(ITEM_YAW)
        * Mat4::from_rotation_z(ITEM_ROLL)
        * Mat4::from_scale(Vec3::splat(scale))
}

/// How big the thing in the hand is drawn.
///
/// **The table decides, not the picture.** `build_into` picks the pose
/// by asking whether a block has a cut-out model, which is a question
/// about the *shape of the PNG* -- every item, every cross-shaped plant
/// and every flat-lying stone answers yes, and so all of them were held
/// in the grip and at the scale of a flint axe. `durability` is the
/// field that already says "this is a thing you swing": it is `Some`
/// for exactly the tools and `None` for everything else, so it is what
/// gets asked here instead.
///
/// **A torch is drawn longer than a tool**, and it is the one thing
/// here that is sized by what it is rather than by what it measures.
/// `ITEM_SCALE` was tuned on the flint axe, whose picture runs corner to
/// corner of its tile; a torch stands upright, so the same scale draws
/// a shorter object out of the same square -- a diagonal fits half as
/// much again into a sixteen-texel box as a vertical does. This gives
/// back what the upright pose costs, and a torch then reads as the
/// arm's-length stick it is instead of a stub.
///
/// A tool keeps the frame-sized scale it was tuned at. Anything else is
/// sized by its silhouette to `MATERIAL_WIDTH`, which is what stops the
/// next lump of ore from being a wall.
pub fn held_scale(block: BlockId, model: &crate::engine::item_model::ItemModel) -> f32 {
    // **A torch is asked about first, because `durability` no longer
    // answers only one question.** The field means "swings left" for
    // every row in the block table but one: a lit torch spends it as
    // seconds of fibre. So a torch that was alight came out at the tool
    // scale and the same torch unlit came out at the material scale --
    // three times smaller -- and the size of the thing in the player's
    // hand changed at the moment they lit it, which reads as the game
    // swapping one object for another.
    //
    // It takes the tool scale, both ways round: it is a full-length
    // stick held by the end, exactly like an axe haft, and the only
    // thing that must never differ between the three states is this.
    if primitive_shared::types::is_torch(block) {
        return TORCH_SCALE;
    }
    if primitive_shared::blocks::definition(block).durability.is_some() {
        return ITEM_SCALE;
    }
    let [width, height] = model.silhouette();
    // A one-texel sliver would divide by a sixteenth and come out
    // sixteen times life size; the floor is one texel of a 16x16
    // sprite, which is the smallest silhouette that can exist.
    MATERIAL_WIDTH / width.max(height).max(1.0 / 16.0)
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
/// `impact` is where in the blow the head lands -- [`IMPACT`] for an
/// empty hand, later for a heavy one ([`impact_at`]). Passed in rather
/// than read off the constant, because a blow is authored by what was in
/// the hand when it started and the hand can change mid-swing.
fn swing_curve(t: f32, impact: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    // A fraction of a blow, never its ends: a zero would divide by
    // nothing on the strike and a one on the recovery.
    let impact = impact.clamp(0.05, 0.95);
    if t < impact {
        (t / impact).powf(0.55)
    } else {
        let back = (1.0 - t) / (1.0 - impact);
        back * back
    }
}

/// The shape of one *thrust*, over 0..1 of the same duration.
///
/// **Three stretches and not two**, which is the difference between a
/// thrust and the swing above:
///
/// 1. the draw, `0..LUNGE_COCK`: the point comes *back*, to
///    -[`LUNGE_DRAW`] of the reach, easing to a stop -- the load;
/// 2. the strike, to [`LUNGE_HIT`]: out to full extension on a
///    fractional power, so it leaves the loaded position fast and
///    arrives slowing, the way something that has hit an animal does;
/// 3. the recovery, the rest of the blow: squared, so it comes off the
///    hit slowly and settles rather than snapping back.
///
/// The negative first stretch is the whole reason this cannot be
/// [`swing_curve`] with a different constant in front of it: a swing
/// leaves rest in the direction it is going and a thrust does not.
fn lunge_curve(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    if t < LUNGE_COCK {
        let u = t / LUNGE_COCK;
        // Eased out -- u(2-u) -- so the arm arrives at the loaded
        // position and *waits* there for an instant. Linear, it turns
        // the corner into the strike with no pause at all, and the
        // whole blow reads as one wobble.
        -LUNGE_DRAW * u * (2.0 - u)
    } else if t < LUNGE_HIT {
        let u = (t - LUNGE_COCK) / (LUNGE_HIT - LUNGE_COCK);
        -LUNGE_DRAW + (1.0 + LUNGE_DRAW) * u.powf(0.6)
    } else {
        let back = (1.0 - t) / (1.0 - LUNGE_HIT);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn layers() -> FaceLayers {
        FaceLayers::empty_for_test()
    }

    /// A shirt is the same colour in the hand as it is in the pack.
    ///
    /// **Twelve garments share four greyscale pictures**, and the only
    /// thing that tells leather from bronze from iron is a tint
    /// multiplied in when they are drawn (`types::garment_tint`). The
    /// pack and the hotbar did that; the hand passed white. So a bronze
    /// cuirass was bronze in the pack and raw grey in the hand -- the
    /// player's own words were "у брони текстура в инвентаре и в руке
    /// разная".
    ///
    /// Asserted against `hotbar::icon_tint`, which is the pack's own
    /// answer, rather than against a colour written down here: a test
    /// carrying its own copy of the number would go green while the two
    /// screens drifted apart.
    #[test]
    fn every_frame_of_the_fire_covers_the_whole_head_of_the_torch() {
        // **The fault this replaces was three attempts of guessing.**
        // The flame was a billboard sized and placed by hand against a
        // sprite whose size and place come from a matrix, and every
        // setting that swallowed the wad's tip hung the fire's base out
        // past the stick while every setting that did not left the wad
        // showing round the edges. It is one rectangle laid on another
        // now -- see `flame_tile_on_sprite` -- and that is a thing a
        // test can check exactly.
        //
        // Six frames of different heights and widths, so the tallest
        // covering the head says nothing about the shortest. Every
        // drawn texel of the head, against every frame.
        let picture = |name: &str| {
            let bytes = crate::embedded::texture(name).expect("a picture this build ships");
            image::load_from_memory(bytes).expect("a PNG").to_rgba8()
        };
        let torch = picture("tools/torch_lit.png");
        let tile = flame_tile_on_sprite();
        let scale = [
            (tile[2] - tile[0]) / RESOLUTION,
            (tile[3] - tile[1]) / RESOLUTION,
        ];
        for frame in 0..crate::engine::texture::FLAME_FRAMES {
            let fire = picture(&format!("effects/torch_flame.{frame}.png"));
            let mut bare = Vec::new();
            for y in 0..WAD_FACE[3] as u32 {
                for x in 0..torch.width() {
                    if torch.get_pixel(x, y).0[3] < 16 {
                        continue;
                    }
                    // **Its corners, not its middle.** A texel whose
                    // centre lands on fire can still have three
                    // quarters of itself outside it, and three quarters
                    // of a texel of glowing fibre beside a flame is
                    // exactly what a player sees and calls an edge.
                    // Inset a tenth so the check is about the texel and
                    // not about which side of a boundary a float lands
                    // on.
                    let covered = |dx: f32, dy: f32| {
                        let fx = (x as f32 + dx - tile[0]) / scale[0];
                        let fy = (y as f32 + dy - tile[1]) / scale[1];
                        (0.0..RESOLUTION).contains(&fx)
                            && (0.0..RESOLUTION).contains(&fy)
                            && fire.get_pixel(fx as u32, fy as u32).0[3] >= 16
                    };
                    if !(covered(0.1, 0.1)
                        && covered(0.9, 0.1)
                        && covered(0.1, 0.9)
                        && covered(0.9, 0.9))
                    {
                        bare.push((x, y));
                    }
                }
            }
            assert!(
                bare.is_empty(),
                "frame {frame} of the fire leaves {} texels of the torch's head showing: {bare:?}",
                bare.len()
            );
        }
    }


    /// The fire is drawn **where the middle of the torch is**, and it
    /// still wins the depth test against it.
    ///
    /// Two claims, and they pull against each other, which is why they
    /// are measured together at thirteen points of one blow.
    ///
    /// *Where.* The player's report was that the fire is not on the
    /// torch. It was a plate's thickness proud of the sprite's front
    /// face, along the plate's own normal -- and the torch is held
    /// turned by `ITEM_YAW`, so most of that offset was **sideways on
    /// screen**. The fix is to build the quad at the plate's mid-depth,
    /// where the wad it covers actually is, and then buy the depth
    /// clearance along the one direction a perspective projection
    /// cannot see: the ray from the eye. So every corner of the fire
    /// must still lie on the ray through the corner of the *midplane*
    /// quad, exactly, at every phase of the blow. That is what pins the
    /// fire to the head however the swing turns it.
    ///
    /// *In front.* The pipeline writes depth and tests it with `Less`
    /// (see `hand_pipeline`), and the sprite is a cutout, so a fire at
    /// the mid-plane with no clearance is a fire hidden behind the
    /// opaque wad it is drawn to cover. Measured along the rays through
    /// its own corners, against the plate's front plane -- which is the
    /// comparison the depth buffer will make, rather than a distance
    /// along some other axis that happens to be positive.
    ///
    /// The winding is checked from the quad's **corners** rather than
    /// from a stored face index, exactly as `push_quad` takes it: the
    /// hand is pitched, rolled and swung, and a quad that still called
    /// itself "+Z" after that would be describing a direction it no
    /// longer points in.
    #[test]
    fn the_fire_is_drawn_where_the_middle_of_the_torch_is() {
        let Some((device, queue)) = crate::engine::test_gpu() else {
            println!("no GPU adapter on this machine; skipping the held-flame test");
            return;
        };
        // A texture manager is what carries the item models, and a
        // torch with no model is drawn as a cube with no flame at all --
        // which is why the two builds below are compared rather than
        // one of them being trusted.
        let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
        let Ok(textures) =
            crate::engine::texture::TextureManager::load(device, queue, assets, 4)
        else {
            println!("no textures on this machine; skipping the held-flame test");
            return;
        };
        let layers = textures.face_layers();
        let torch = primitive_shared::types::ALL_BLOCK_IDS
            .iter()
            .find(|(_, name)| *name == "torch_lit")
            .map(|&(id, _)| id)
            .expect("a lit torch is a block this build knows");

        for step in 0..=12 {
            let phase = step as f32 / 12.0;
            let mut hand = Hand::new();
            hand.strike(None);
            hand.update(phase * SWING_SECONDS, false, 0.0, true, None);

            let build = |flame: Option<u32>| {
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                hand.build_into(
                    true,
                    Some(torch),
                    &layers,
                    Some(&textures),
                    (15, 15),
                    flame,
                    &mut vertices,
                    &mut indices,
                );
                vertices
            };
            let plate = build(None);
            let lit = build(Some(textures.torch_flame_layer()));
            assert_eq!(
                lit.len(),
                plate.len() + 4,
                "the flame is one quad, appended last -- at phase {phase} it was not",
            );

            let corner = |v: &HandVertex| Vec3::from_array(v.position);
            let fire: Vec<Vec3> = lit[plate.len()..].iter().map(corner).collect();
            let normal = (fire[1] - fire[0]).cross(fire[2] - fire[1]).normalize();
            let middle = (fire[0] + fire[1] + fire[2] + fire[3]) / 4.0;
            // The eye is the origin of this space and looks down -z, so
            // the way back to it from the fire is `-middle`. A normal
            // pointing the other way would mean the quad is wound
            // inside out, and "in front along the normal" would then be
            // "behind" -- the pipeline culls nothing, so nothing else
            // would say so.
            assert!(
                normal.dot(-middle.normalize()) > 0.0,
                "at phase {phase} the flame is wound away from the eye",
            );

            // The same quad the fire is built from, before it was slid
            // along the rays: the middle of the plate, in the torch's
            // own space and through the torch's own matrix.
            let model = textures.item_model(torch).expect("a torch has a sprite");
            let transform = held_transform(hand.group(), torch, model);
            let tile = flame_tile_on_sprite();
            let at = |x: f32, y: f32| {
                transform.transform_point3(Vec3::new(
                    x / RESOLUTION - 0.5,
                    0.5 - y / RESOLUTION,
                    0.0,
                ))
            };
            let midplane = [
                at(tile[0], tile[3]),
                at(tile[2], tile[3]),
                at(tile[2], tile[1]),
                at(tile[0], tile[1]),
            ];
            for (drawn, wanted) in fire.iter().zip(midplane.iter()) {
                // Same direction from the eye is the same pixel on
                // screen, whatever the projection does after it.
                // **The chord between the two directions, not the angle
                // between them.** `acos` of a dot product that is one
                // ulp under one comes out at 0.02 degrees in `f32` --
                // the derivative of `acos` is infinite there -- so an
                // angle in degrees cannot express "these are the same
                // ray" at all. The chord is the same number for small
                // angles and is computed by subtracting two numbers of
                // the same size, which is where floating point is at
                // its best rather than its worst.
                //
                // A ten-thousandth is a fiftieth of a pixel at the far
                // corner of a 1920-wide frame.
                let apart = (drawn.normalize() - wanted.normalize()).length();
                assert!(
                    apart < 1e-4,
                    "at phase {phase} a corner of the fire is drawn {apart} \
                     off the middle of the torch",
                );
                assert!(
                    drawn.length() < wanted.length(),
                    "at phase {phase} the fire was not brought forward at all",
                );
            }

            // ...and along each of those rays it clears the plate's own
            // front face, which is the plane the depth buffer will
            // compare it against.
            let front = transform.transform_point3(Vec3::new(
                0.0,
                0.0,
                crate::engine::item_model::THICKNESS / 2.0,
            ));
            let face_normal = transform.transform_vector3(Vec3::Z).normalize();
            for (drawn, wanted) in fire.iter().zip(midplane.iter()) {
                let ray = wanted.normalize();
                // Where this ray pierces the plate's front face.
                let along = face_normal.dot(ray);
                assert!(
                    along.abs() > 0.2,
                    "at phase {phase} the plate is edge-on ({along}); \
                     the clearance below means nothing there"
                );
                let hits_face = face_normal.dot(front) / along;
                assert!(
                    drawn.length() < hits_face,
                    "at phase {phase} the fire is {:.4} behind the fibre it is drawn on",
                    drawn.length() - hits_face,
                );
            }
        }
    }

    #[test]
    fn a_garment_is_the_same_colour_in_the_hand_as_it_is_in_the_pack() {
        use primitive_shared::types::{BLOCK_BRONZE_CUIRASS, BLOCK_LEATHER_TUNIC};

        for garment in [BLOCK_LEATHER_TUNIC, BLOCK_BRONZE_CUIRASS] {
            let wanted = crate::ui::hotbar::icon_tint(garment, [1.0; 4]);
            let drawn = build(&Hand::new(), true, Some(garment));
            assert!(!drawn.is_empty(), "nothing was drawn for {garment}");
            for vertex in &drawn {
                // The bare arm draws in its own colour and samples
                // nothing, so only the textured quads are the garment.
                if vertex.tint == [1.0; 4] || vertex.tint == wanted {
                    continue;
                }
                panic!(
                    "{garment} is drawn in the hand as {:?} and in the pack as {wanted:?}",
                    vertex.tint,
                );
            }
            if garment == BLOCK_BRONZE_CUIRASS {
                assert!(
                    drawn.iter().any(|v| v.tint == wanted),
                    "the bronze tint never reached the hand at all",
                );
            }
        }
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
            None,
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
                hand.strike(None);
                hand.update(SWING_SECONDS * IMPACT, false, 0.0, true, None);
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
    fn only_a_tool_is_held_across_the_corner_of_the_frame() {
        // **The bug this is written against.** The pose was picked by
        // asking whether a picture has a cut-out model, which every
        // item, every cross-shaped plant and every flat-lying stone
        // answers yes to -- so a lump of native copper was held in the
        // grip of a flint axe, at the scale of one. It came out as an
        // opaque plate of ore across the bottom-right corner of the
        // screen, an eighth of the whole frame (11.6% measured below),
        // magnified to fifty-six screen pixels a texel. The player's word for it was "a big
        // tilted plane in a green-brown-grey check, right up against
        // the camera".
        //
        // Measured the way the frame measures it: the shipped picture,
        // the real model cut from it, `held_scale`, `item_transform`
        // and the renderer's own hand projection (`HAND_FOV_Y`). A
        // number that only agreed with the arithmetic above it would
        // have passed while the plate was on screen.
        use crate::engine::item_model::ItemModel;

        let projection = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.01, 4.0);
        let load = |texture: &str, name: &str| {
            let bytes = crate::embedded::texture(texture).expect("a picture this build ships");
            let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
            let block = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, other)| *other == name)
                .map(|&(id, _)| id)
                .expect("a block this build knows");
            (ItemModel::from_image(&sprite), block)
        };
        // How much of the frame the held thing's bounding box covers,
        // counting only what is inside it: a haft running off the
        // bottom edge is not screen the player has lost.
        let covered = |model: &ItemModel, scale: f32| -> f32 {
            let mut scratch = Vec::new();
            let mut scratch_indices = Vec::new();
            model.append_transformed(
                &mut scratch,
                &mut scratch_indices,
                item_transform(Mat4::IDENTITY, scale),
                0,
                15,
                0,
            );
            let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
            for vertex in &scratch {
                let ndc = projection.project_point3(Vec3::from_array(vertex.position));
                low[0] = low[0].min(ndc.x);
                low[1] = low[1].min(ndc.y);
                high[0] = high[0].max(ndc.x);
                high[1] = high[1].max(ndc.y);
            }
            let width = (high[0].min(1.0) - low[0].max(-1.0)).max(0.0);
            let height = (high[1].min(1.0) - low[1].max(-1.0)).max(0.0);
            width * height / 4.0
        };

        // First: the measurement can see the fault. Native copper held
        // at the tool scale -- which is exactly what shipped -- takes an
        // eighth of the frame. If this ever stops being true the bound
        // below is measuring the wrong thing.
        let (copper, copper_id) = load("metal/native_copper.png", "native_copper");
        let as_shipped = covered(&copper, ITEM_SCALE);
        println!("as shipped, copper covered {:.1}% of the frame", as_shipped * 100.0);
        assert!(
            as_shipped > 0.10,
            "the fault this test exists for measures only {:.0}% of the frame",
            as_shipped * 100.0
        );

        // A material is a thing held up in the fingers. The four of them
        // measure between one and a half and four percent; six is the
        // room that leaves for a picture drawn a little differently,
        // and it is a third of what the fault above measures.
        for (texture, name) in [
            ("metal/native_copper.png", "native_copper"),
            ("tools/flint.png", "flint"),
            ("plants/stick.png", "stick"),
            ("plants/fiber.png", "fiber"),
        ] {
            let (model, block) = load(texture, name);
            let share = covered(&model, held_scale(block, &model));
            println!("{name} covers {:.1}% of the frame", share * 100.0);
            assert!(
                share < 0.06,
                "{name} covers {:.0}% of the frame: it is being held like a tool",
                share * 100.0
            );
        }
        assert!(
            held_scale(copper_id, &copper) < ITEM_SCALE,
            "a lump of ore is still sized like an axe"
        );

        // ...and a torch is not, however far through its life it is.
        //
        // **The size of a thing in the hand must not change when it
        // catches fire.** `held_scale` asks `durability.is_some()` to
        // mean "this is a thing you swing", and a lit torch spends that
        // field on seconds of fibre -- so lighting one used to make it
        // three times bigger, and the game read as swapping one object
        // for another at the moment of the strike. Photographed, then
        // fixed, then written down here so it cannot come back.
        let torches: Vec<f32> = ["torch", "torch_lit", "torch_spent"]
            .iter()
            .map(|name| {
                let (model, block) = load(&format!("tools/{name}.png"), name);
                held_scale(block, &model)
            })
            .collect();
        assert!(
            torches.iter().all(|&scale| scale == torches[0]),
            "a torch changes size as it burns: {torches:?}"
        );

        // ...and a tool is still held at the scale it was tuned at.
        // Shrinking everything is the cheapest way to pass the bound
        // above, and it would undo the one pose in this game that was
        // settled by looking at it. Note that a share cannot say this:
        // the flint knife is a small thin sprite and covers six percent
        // even at full tool scale, which is nearer a lump than an axe.
        for (texture, name) in [
            ("tools/stone_axe.png", "stone_axe"),
            ("tools/stone_pickaxe.png", "stone_pickaxe"),
            ("tools/flint_knife.png", "flint_knife"),
        ] {
            let (model, block) = load(texture, name);
            assert_eq!(
                held_scale(block, &model),
                ITEM_SCALE,
                "{name} is no longer held the way a tool is held"
            );
        }
    }

    #[test]
    fn a_material_is_the_same_size_in_the_hand_however_its_picture_was_drawn() {
        // **The frame is not the object.** A model's coordinates run
        // across the whole PNG, so scaling one scales the artist's
        // margins along with the drawing: native copper fills ten
        // texels of sixteen, fibre fills all sixteen. Sized by the
        // frame they come out nearly two to one; sized by the
        // silhouette -- which is what `held_scale` does -- they are the
        // same thing in the hand.
        use crate::engine::item_model::ItemModel;

        let mut widths = Vec::new();
        for (texture, name) in [
            ("metal/native_copper.png", "native_copper"),
            ("tools/flint.png", "flint"),
            ("plants/fiber.png", "fiber"),
        ] {
            let bytes = crate::embedded::texture(texture).expect("a picture this build ships");
            let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
            let model = ItemModel::from_image(&sprite);
            let block = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, other)| *other == name)
                .map(|&(id, _)| id)
                .expect("a block this build knows");
            let [w, h] = model.silhouette();
            widths.push(w.max(h) * held_scale(block, &model));
        }
        let low = widths.iter().copied().fold(f32::MAX, f32::min);
        let high = widths.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            high - low < 1e-4,
            "carried materials come out {low:.3} to {high:.3} wide: the margins are deciding"
        );
    }

    /// The one thing the picture has to be for the pose above to be
    /// legal to build: a shaft from one corner of the tile to the
    /// other, with the head at the top-right end.
    ///
    /// **`spear_transform` lays the drawing along its own bounding
    /// diagonal**, so a spear redrawn upright, or flipped, or with the
    /// point at the bottom, would come out held by the head with the
    /// butt towards the animal -- a fault nothing else here could
    /// notice, because every quad would still be exactly where the
    /// arithmetic put it. This is the assumption written down where a
    /// redrawn sprite trips over it.
    #[test]
    fn a_spear_is_drawn_corner_to_corner_with_its_head_at_the_top() {
        let bytes = crate::embedded::texture("tools/flint_spear.png")
            .expect("a picture this build ships");
        let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
        let (w, h) = (sprite.width(), sprite.height());
        let drawn = |x: u32, y: u32| sprite.get_pixel(x, y).0[3] >= 128;

        // Corner to corner: something is drawn in the bottom-left
        // quarter of the tile and something in the top-right one.
        let corner = |x0: u32, y0: u32| {
            (y0..y0 + h / 4).any(|y| (x0..x0 + w / 4).any(|x| drawn(x, y)))
        };
        assert!(corner(0, h - h / 4), "nothing is drawn at the butt end of the tile");
        assert!(corner(w - w / 4, 0), "nothing is drawn at the point end of the tile");

        // ...and the top-right end is the fat one, which is what says
        // the head is there and not at the other end.
        let count = |x0: u32, y0: u32| {
            (y0..y0 + h / 3)
                .flat_map(|y| (x0..x0 + w / 3).map(move |x| (x, y)))
                .filter(|&(x, y)| drawn(x, y))
                .count()
        };
        let head = count(w - w / 3, 0);
        let butt = count(0, h - h / 3);
        assert!(
            head > butt,
            "the butt end of the spear ({butt} texels) is drawn fatter than the head ({head}): \
             the pose would hold it backwards"
        );
    }

    /// **A held rod points forward and up, and its line hangs from the tip.**
    /// "удочка повёрнута не так как надо": it stood up the right edge of the
    /// frame, and the line painted beside its tip ran up out of the frame.
    /// Measured through the hand's own projection, as the spear is.
    #[test]
    fn a_held_rod_points_forward_and_up_with_its_line_hanging_from_the_tip() {
        use primitive_shared::types::BLOCK_FISHING_ROD;
        let projection = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.01, 4.0);
        let bytes = crate::embedded::texture("tools/fishing_rod.png").expect("a picture this build ships");
        let model = crate::engine::item_model::ItemModel::from_image(&image::load_from_memory(bytes).expect("a PNG").to_rgba8());
        let transform = held_transform(Mat4::IDENTITY, BLOCK_FISHING_ROD, &model);
        // Texels of the picture: the butt at column 1 row 14, the tip of the
        // rod at 13, 2, and the line hanging from it down column 14.
        let texel = |x: f32, y: f32| transform.transform_point3(Vec3::new((x + 0.5) / 16.0 - 0.5, 0.5 - (y + 0.5) / 16.0, 0.0));
        let screen = |p: Vec3| projection.project_point3(p);
        let (butt, tip, line) = (texel(1.0, 14.0), texel(13.0, 2.0), texel(14.0, 11.0));
        assert!(tip.z < butt.z - 1.0, "the tip is not away from the eye: butt {butt}, tip {tip}");
        assert!(tip.y > butt.y + 0.8, "the rod does not go up: butt {butt}, tip {tip}");
        let (on_screen, low) = (screen(tip), screen(butt));
        assert!(on_screen.x > 0.0 && on_screen.x < 0.6 && on_screen.y > 0.0 && on_screen.y < 0.8, "the tip is drawn at {on_screen}");
        assert!(low.y < -1.0 || low.x > 1.0, "the butt is in the frame at {low}, where no hand holds it");
        assert!(screen(line).y < on_screen.y - 0.1, "the line runs from the tip at {on_screen} up to {}", screen(line));
    }

    /// A spear is held **down the line of sight**, and a tool is not.
    ///
    /// The player's report was that the spear "looks like a stick" --
    /// which is exactly what a two-metre shaft posed by the flint axe's
    /// grip is: a plank lying across the bottom-right of the frame at
    /// one depth. So the property is measured as depth: the two ends of
    /// a spear are the better part of a metre apart *along the view
    /// axis*, where an axe's plate is nearly flat to the screen.
    ///
    /// Measured through the renderer's own hand projection
    /// (`HAND_FOV_Y`, 70 degrees, 16:9) and the shipped picture, so it
    /// is the frame's own arithmetic and not a second copy of it.
    #[test]
    fn a_spear_is_held_down_the_line_of_sight() {
        use crate::engine::item_model::ItemModel;

        let projection = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.01, 4.0);
        let load = |texture: &str, name: &str| {
            let bytes = crate::embedded::texture(texture).expect("a picture this build ships");
            let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
            let block = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, other)| *other == name)
                .map(|&(id, _)| id)
                .expect("a block this build knows");
            (ItemModel::from_image(&sprite), block)
        };
        let corners = |model: &ItemModel, block| {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            model.append_transformed(
                &mut vertices,
                &mut indices,
                held_transform(Mat4::IDENTITY, block, model),
                0,
                15,
                0,
            );
            vertices
                .iter()
                .map(|v| Vec3::from_array(v.position))
                .collect::<Vec<_>>()
        };

        let (spear, spear_id) = load("tools/flint_spear.png", "flint_spear");
        assert!(
            primitive_shared::types::is_weapon(spear_id),
            "the spear stopped being the thing this pose is chosen by"
        );
        let held = corners(&spear, spear_id);
        let depth = |points: &[Vec3]| {
            let near = points.iter().map(|p| p.z).fold(f32::MIN, f32::max);
            let far = points.iter().map(|p| p.z).fold(f32::MAX, f32::min);
            near - far
        };
        let (axe, axe_id) = load("tools/stone_axe.png", "stone_axe");
        let axe_held = corners(&axe, axe_id);
        println!(
            "spear spans {:.2} of depth, the axe {:.2}",
            depth(&held),
            depth(&axe_held)
        );
        assert!(
            depth(&held) > 3.0 * depth(&axe_held),
            "the spear lies as flat to the screen as an axe does: {:.2} against {:.2}",
            depth(&held),
            depth(&axe_held),
        );

        // Where the two ends land in the frame. The point is the end
        // that is furthest from the eye -- that is what "pointing away"
        // means -- and it has to be up near the crosshair without
        // covering it; the butt has to be down in the corner where a
        // near hand is.
        let point = held
            .iter()
            .copied()
            .fold(held[0], |best, p| if p.z < best.z { p } else { best });
        let butt = held
            .iter()
            .copied()
            .fold(held[0], |best, p| if p.z > best.z { p } else { best });
        let ndc = |p: Vec3| projection.project_point3(p);
        let (point_at, butt_at) = (ndc(point), ndc(butt));
        println!("point lands at {point_at}, butt at {butt_at}");
        assert!(
            point_at.x > 0.12 && point_at.x < 0.55,
            "the point of the spear lands at x={}: over the crosshair, or off the side",
            point_at.x
        );
        assert!(
            point_at.y > -0.05 && point_at.y < 0.35,
            "the point of the spear lands at y={}: not the little above centre it was posed for",
            point_at.y
        );
        assert!(
            butt_at.x > point_at.x && butt_at.y < point_at.y,
            "the butt of the spear is not below and outside its point: {butt_at} against {point_at}",
        );

        // ...and nothing of it crosses the crosshair, which is the
        // constraint every held thing in this file is under.
        for corner in &held {
            let at = ndc(*corner);
            assert!(
                at.x > 0.06,
                "a corner of the spear is drawn at x={}, over what is being aimed at",
                at.x,
            );
        }
    }

    /// The flat of the picture is turned as far towards the eye as a
    /// shaft pointing away allows -- and that roll is *computed*, not
    /// chosen.
    ///
    /// **A sprite is a plate one texel thick.** Rolled the wrong way
    /// about a shaft that already points down the line of sight it is
    /// seen edge-on, which is a spear drawn as a hairline or as nothing
    /// at all. There is exactly one best roll and it moves whenever
    /// either end of the shaft moves, so it cannot be a number in the
    /// file; this checks the derivation against thirty-two rolls of the
    /// same plate.
    #[test]
    fn the_flat_of_the_spear_is_turned_as_far_towards_the_eye_as_the_shaft_allows() {
        use crate::engine::item_model::ItemModel;

        let bytes = crate::embedded::texture("tools/flint_spear.png").expect("a shipped picture");
        let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
        let model = ItemModel::from_image(&sprite);
        let spear = primitive_shared::types::ALL_BLOCK_IDS
            .iter()
            .find(|(_, name)| *name == "flint_spear")
            .map(|&(id, _)| id)
            .expect("a block this build knows");

        let transform = held_transform(Mat4::IDENTITY, spear, &model);
        // The plate's own normal, carried through the same matrix the
        // quads go through.
        let normal = transform.transform_vector3(Vec3::Z).normalize();
        let along = spear_shaft();
        let middle = (SPEAR_BUTT + SPEAR_POINT) * 0.5;
        let to_eye = -middle.normalize();
        let shown = normal.dot(to_eye).abs();
        assert!(
            (normal.dot(along)).abs() < 1e-3,
            "the plate is not square to its own shaft: the picture would be sheared",
        );
        for step in 0..32 {
            let roll = step as f32 / 32.0 * std::f32::consts::TAU;
            let turned = glam::Quat::from_axis_angle(along, roll) * normal;
            assert!(
                turned.dot(to_eye).abs() <= shown + 1e-3,
                "rolling the spear by {roll:.2} shows {:.3} of the plate where the pose shows {shown:.3}",
                turned.dot(to_eye).abs(),
            );
        }
        println!("the plate faces the eye at {shown:.3} of full");
    }

    /// A blow with a spear is a **thrust**: every corner of it moves by
    /// the same vector, and that vector is along the shaft.
    ///
    /// Two claims, and the second is what makes it a thrust rather than
    /// a lunge sideways. The first is what makes it not a swing: a
    /// swing is a rotation about a shoulder, so the head of a pick
    /// travels much further than its haft, and if the spear ever picked
    /// up the arc again this is what would see it. Checked against a
    /// pick at the same phases, which is where the contrast comes from.
    #[test]
    fn a_spear_is_thrust_along_its_own_shaft_and_never_swung() {
        use crate::engine::item_model::ItemModel;

        let load = |texture: &str, name: &str| {
            let bytes = crate::embedded::texture(texture).expect("a picture this build ships");
            let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
            let block = primitive_shared::types::ALL_BLOCK_IDS
                .iter()
                .find(|(_, other)| *other == name)
                .map(|&(id, _)| id)
                .expect("a block this build knows");
            (ItemModel::from_image(&sprite), block)
        };
        let corners = |hand: &Hand, model: &ItemModel, block| {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            model.append_transformed(
                &mut vertices,
                &mut indices,
                held_transform(hand.pose(block), block, model),
                0,
                15,
                0,
            );
            vertices
                .iter()
                .map(|v| Vec3::from_array(v.position))
                .collect::<Vec<_>>()
        };

        let (spear, spear_id) = load("tools/flint_spear.png", "flint_spear");
        let (pick, pick_id) = load("tools/stone_pickaxe.png", "stone_pickaxe");
        let rest = corners(&Hand::new(), &spear, spear_id);
        let pick_rest = corners(&Hand::new(), &pick, pick_id);
        let along = spear_shaft();

        let mut furthest: f32 = 0.0;
        let mut drawn_back: f32 = 0.0;
        // Twenty-one points of the blow **and the moment it lands**,
        // which is not one of them: the reach is only exact at
        // `LUNGE_HIT`, and a sweep that steps over it measures the
        // curve either side and reports the thrust short.
        let phases = (0..=20)
            .map(|step| step as f32 / 20.0)
            .chain(std::iter::once(LUNGE_HIT));
        for phase in phases {
            // Held at the phase rather than advanced to it: `update`
            // clamps one step to a tenth of a second so a stall cannot
            // throw the arm through a whole blow, which means a single
            // step can never reach the end of one either. See
            // `Hand::freeze`.
            let mut hand = Hand::new();
            hand.freeze(phase);

            let moved: Vec<Vec3> = corners(&hand, &spear, spear_id)
                .iter()
                .zip(rest.iter())
                .map(|(now, was)| *now - *was)
                .collect();
            let first = moved[0];
            for travelled in &moved {
                assert!(
                    (*travelled - first).length() < 1e-5,
                    "at phase {phase} the spear turned as well as travelled: {travelled} against {first}",
                );
            }
            assert!(
                first.cross(along).length() < 1e-5,
                "at phase {phase} the spear travelled {first}, which is not along its own shaft",
            );
            let reach = first.dot(along);
            furthest = furthest.max(reach);
            drawn_back = drawn_back.min(reach);

            // ...while the pick, at the same phase, is still swinging:
            // its head and its haft do not travel together.
            let pick_moved: Vec<Vec3> = corners(&hand, &pick, pick_id)
                .iter()
                .zip(pick_rest.iter())
                .map(|(now, was)| *now - *was)
                .collect();
            if phase > 0.05 && phase < 0.95 {
                let spread = pick_moved
                    .iter()
                    .map(|step| (*step - pick_moved[0]).length())
                    .fold(0.0, f32::max);
                assert!(
                    spread > 1e-3,
                    "at phase {phase} the pick moved as one piece: it stopped swinging",
                );
            }
        }
        println!("the spear reached {furthest:.3} out and {drawn_back:.3} back");
        assert!(
            (furthest - LUNGE_REACH).abs() < 1e-3,
            "the thrust reached {furthest} where {LUNGE_REACH} was asked for",
        );
        assert!(
            drawn_back < -0.01,
            "the point was never drawn back before the blow: it is a jab with no load",
        );
    }

    /// The cut end of the haft is never in the frame, at any moment of
    /// a blow.
    ///
    /// **Photographed, and it is the reason `SPEAR_BUTT` is where it
    /// is.** There is no arm in this game's view model -- the tool is
    /// the whole of it (see `build_into`) -- so a haft that ends inside
    /// the frame ends in nothing: a stump of wood hanging in mid-air
    /// over the landscape. At rest that never happened, because the
    /// butt sat on the corner; a thrust moves the near end through a
    /// third of its own depth and the whole of that is travel towards
    /// the shaft's vanishing point, so the end came up into the middle
    /// of the right-hand side of the screen at full extension.
    ///
    /// Measured in the vertical only, and that is deliberate: the
    /// hand's field of view is 70 degrees *vertically* whatever the
    /// window's shape (`HAND_FOV_Y`), so "below the bottom edge" is the
    /// one claim that holds on an ultrawide as well as on a phone.
    #[test]
    fn the_cut_end_of_the_haft_never_comes_into_the_frame() {
        use crate::engine::item_model::ItemModel;

        let bytes = crate::embedded::texture("tools/flint_spear.png").expect("a shipped picture");
        let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
        let model = ItemModel::from_image(&sprite);
        let spear = primitive_shared::types::ALL_BLOCK_IDS
            .iter()
            .find(|(_, name)| *name == "flint_spear")
            .map(|&(id, _)| id)
            .expect("a block this build knows");

        // The vertical half-angle of the hand's projection. A point is
        // below the bottom edge when its height over the eye is less
        // than -tan(35 degrees) times its distance down the view axis.
        let edge = (70f32 / 2.0).to_radians().tan();
        for step in 0..=24 {
            let phase = step as f32 / 24.0;
            let mut hand = Hand::new();
            hand.freeze(phase);
            let transform = held_transform(hand.pose(spear), spear, &model);
            // Where the drawing's own butt corner ended up, and every
            // quad corner within a hand's breadth of it: the end cap
            // and the last of the rim, which is what a player would see
            // as a stump.
            let (low, _) = model.drawn_box();
            let end = transform.transform_point3(Vec3::new(low[0], low[1], 0.0));
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            model.append_transformed(&mut vertices, &mut indices, transform, 0, 15, 0);
            let mut checked = 0;
            for vertex in &vertices {
                let at = Vec3::from_array(vertex.position);
                if (at - end).length() > 0.15 {
                    continue;
                }
                checked += 1;
                assert!(
                    at.y < -edge * (-at.z),
                    "at phase {phase} the end of the haft is drawn at {at}, inside the frame",
                );
            }
            assert!(checked > 0, "the butt of the spear was not found at phase {phase}");
        }
    }

    /// The thrust leaves rest, loads, drives and settles -- in that
    /// order, and it is over when the swing is.
    #[test]
    fn the_thrust_loads_before_it_drives_and_comes_back_slowly() {
        assert_eq!(lunge_curve(0.0), 0.0);
        assert_eq!(lunge_curve(1.0), 0.0);
        assert!(
            lunge_curve(LUNGE_COCK) < -0.2,
            "the point is not drawn back at all before the blow",
        );
        assert!(
            (lunge_curve(LUNGE_HIT) - 1.0).abs() < 1e-6,
            "the blow does not land at full extension",
        );
        // Half way through, the strike is over and most of the recovery
        // is still to come -- the same asymmetry a swing has.
        assert!(
            lunge_curve(0.5) > 0.7,
            "the spear is home before the blow has been seen",
        );
        assert!(
            lunge_curve(0.85) < 0.15,
            "the recovery is as slow as a stir",
        );
    }

    /// **A mouthful the server let through goes to the mouth and comes
    /// back, in the time a mouthful takes on everybody else's screen**; a
    /// block set down is a short reach; a blow is not a gesture.
    #[test]
    fn eating_brings_the_hand_to_the_mouth_and_back_in_the_time_a_mouthful_takes() {
        let rest = Hand::new().group().transform_point3(ITEM_CENTRE);
        let at = |action: Action, seconds: f32| {
            let mut hand = Hand::new();
            hand.gesture(action);
            let mut left = seconds;
            while left > 0.0 {
                let step = left.min(0.05);
                hand.update(step, false, 0.0, true, None);
                left -= step;
            }
            hand.group().transform_point3(ITEM_CENTRE)
        };
        let eating = at(Action::Eat, crate::logic::player_model::EAT_SECONDS * 0.5);
        assert!(eating.y > rest.y + 0.12 && eating.x < rest.x - 0.2, "the food went to {eating} from {rest}");
        let done = at(Action::Eat, crate::logic::player_model::EAT_SECONDS + 0.05);
        assert!((done - rest).length() < 1e-5, "the hand stayed at the mouth");
        let placing = at(Action::Place, crate::logic::player_model::PLACE_SECONDS * 0.5);
        assert!(placing.z < rest.z - 0.05, "setting a block down did not reach");
        assert!((at(Action::Strike, 0.1) - rest).length() < 1e-5, "a blow was acted out twice");
    }

    #[test]
    fn a_swing_leaves_rest_and_comes_back_to_it() {
        let mut hand = Hand::new();
        assert_eq!(hand.swing(), 0.0, "an idle hand is already swinging");
        hand.strike(None);
        // Through the blow in small steps, watching that it actually
        // travels and that it is over when it says it is.
        let mut peak: f32 = 0.0;
        for _ in 0..40 {
            hand.update(SWING_SECONDS / 20.0, false, 0.0, true, None);
            peak = peak.max(hand.swing());
        }
        assert!(peak > 0.9, "the arm barely moved: peak {peak}");
        assert_eq!(hand.swing(), 0.0, "the arm never came back to rest");
    }

    #[test]
    fn a_heavier_head_takes_longer_to_swing_and_lands_later_in_its_own_blow() {
        use primitive_shared::types::{
            BLOCK_FLINT_KNIFE, BLOCK_IRON_PICKAXE, BLOCK_STONE_AXE,
        };
        // The ladder as a player climbs it: a knapped flake is mostly
        // haft, a ground stone head is a lump of rock, and a cast iron
        // one is what a smith finally spends the ore on. The order is
        // the whole claim -- see `heft`.
        let fist = heft(None);
        let flake = heft(Some(BLOCK_FLINT_KNIFE));
        let stone = heft(Some(BLOCK_STONE_AXE));
        let iron = heft(Some(BLOCK_IRON_PICKAXE));
        assert_eq!(fist, 0.0, "an empty hand weighs {fist}");
        assert!(
            fist < flake && flake < stone && stone < iron,
            "the ladder came out {fist} {flake} {stone} {iron}"
        );
        assert!(iron <= 1.0, "the heaviest head is off the scale at {iron}");

        // ...and both clocks follow it.
        assert!(dig_seconds(Some(BLOCK_IRON_PICKAXE)) > dig_seconds(Some(BLOCK_FLINT_KNIFE)));
        assert!(impact_at(Some(BLOCK_IRON_PICKAXE)) > impact_at(None));
        // The windup and the recovery both grow in *seconds*, which is
        // the shape of a heavy swing: slow up, and a long time settling
        // afterwards. A blow of a fixed length with a later impact would
        // give the heavy tool a *shorter* recovery, which reads as the
        // arm snapping back.
        let windup = |held| dig_seconds(held) * impact_at(held);
        let recovery = |held| dig_seconds(held) * (1.0 - impact_at(held));
        let iron = Some(BLOCK_IRON_PICKAXE);
        assert!(windup(iron) > windup(None), "the heavy head came up as fast as a fist");
        assert!(recovery(iron) > recovery(None), "the heavy head came home as fast as a fist");
    }

    #[test]
    fn a_blow_lands_exactly_once_and_when_the_arm_is_down() {
        use primitive_shared::types::BLOCK_IRON_PICKAXE;
        // What the camera's recoil is played off. Landing twice is a
        // double kick per swing; landing at the click is the view
        // flinching before the tool arrives.
        let iron = Some(BLOCK_IRON_PICKAXE);
        let mut hand = Hand::new();
        hand.strike(iron);
        let step = 1.0 / 240.0;
        let mut landings = Vec::new();
        let mut elapsed = 0.0;
        for _ in 0..400 {
            hand.update(step, false, 0.0, true, iron);
            elapsed += step;
            if let Some(heft) = hand.take_landed() {
                landings.push((elapsed, heft));
            }
        }
        assert_eq!(landings.len(), 1, "a blow landed {} times", landings.len());
        let (at, weight) = landings[0];
        let wanted = dig_seconds(iron) * impact_at(iron);
        assert!(
            (at - wanted).abs() <= step * 2.0,
            "the blow landed at {at}s against an arm that is down at {wanted}s"
        );
        assert_eq!(weight, heft(iron), "the blow forgot what struck it");
        assert!(hand.take_landed().is_none(), "one blow was taken twice");
    }

    #[test]
    fn a_blow_that_landed_during_a_stall_is_still_reported() {
        // A dropped frame is the one time a player actually notices the
        // view not flinching, so a landing swallowed by a long `dt`
        // would be the bug this is for.
        let mut hand = Hand::new();
        hand.strike(None);
        hand.update(1.0, false, 0.0, true, None);
        assert!(hand.take_landed().is_some(), "the stall swallowed the blow");
    }

    #[test]
    fn the_blow_lands_early_and_recovers_late() {
        // The whole difference between a strike and a wave. Half way
        // through the animation the arm should already be most of the
        // way home from an impact that happened near the start.
        assert!(swing_curve(IMPACT, IMPACT) > 0.99, "the blow does not land at the impact");
        assert!(
            swing_curve(IMPACT * 0.5, IMPACT) > 0.6,
            "the strike is too slow to leave rest"
        );
        assert!(
            swing_curve(0.5, IMPACT) < 0.6,
            "the recovery is as fast as the strike, which reads as a pendulum"
        );
        assert_eq!(swing_curve(0.0, IMPACT), 0.0);
        assert_eq!(swing_curve(1.0, IMPACT), 0.0);
    }

    #[test]
    fn holding_the_button_keeps_the_arm_going() {
        let mut hand = Hand::new();
        let mut moved = 0;
        for _ in 0..60 {
            hand.update(SWING_SECONDS / 20.0, true, 0.0, true, None);
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
            still.update(1.0 / 60.0, false, 0.0, true, None);
            walking.update(1.0 / 60.0, false, 4.3, true, None);
            falling.update(1.0 / 60.0, false, 4.3, false, None);
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
        hand.strike(None);
        hand.update(10.0, false, 0.0, true, None);
        assert!(hand.swing() > 0.0, "one long frame skipped the entire swing");
    }

    #[test]
    fn a_thrust_on_screen_lasts_exactly_as_long_as_the_spear_is_refused_for() {
        // The player's request, as a property: the spear at rest again is
        // the spear ready again. Stepped a sixtieth at a time the way a
        // frame steps it, and measured to within one step.
        use primitive_shared::types::{BLOCK_FLINT_SPEAR, BLOCK_STONE_PICKAXE};
        let spear = Some(BLOCK_FLINT_SPEAR);
        let refused_for = primitive_shared::combat::swing_seconds(spear);
        assert_eq!(refused_for, 1.0, "a thrust is not the second the player asked for");

        let mut hand = Hand::new();
        hand.strike(spear);
        let step = 1.0 / 60.0;
        let mut lasted = 0.0;
        while hand.swing.is_some() {
            hand.update(step, false, 0.0, true, None);
            lasted += step;
            assert!(lasted < 5.0, "the thrust never ended");
        }
        assert!(
            (lasted - refused_for).abs() <= step + 1e-4,
            "the thrust lasted {lasted}s against a {refused_for}s cooldown"
        );
        // ...and a pick's swing is still the quick one.
        // ...and a pick's swing is still the quick one: within a third
        // of a fist's, which is all the weight of its head is worth.
        let pick = blow_seconds(Some(BLOCK_STONE_PICKAXE));
        assert_eq!(pick, dig_seconds(Some(BLOCK_STONE_PICKAXE)));
        assert!(
            pick > SWING_SECONDS && pick < SWING_SECONDS * (1.0 + HEFT_SLOWS) + 1e-6,
            "a stone pick's blow is {pick}s against a fist's {SWING_SECONDS}s"
        );
    }

    #[test]
    fn a_thrust_lands_when_its_point_is_furthest_out() {
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        let spear = Some(BLOCK_FLINT_SPEAR);
        let hit = impact_seconds(spear);
        let mut hand = Hand::new();
        hand.strike(spear);
        // In steps, because one step is clamped to a tenth of a second.
        for _ in 0..50 {
            hand.update(hit / 50.0, false, 0.0, true, None);
        }
        assert!(
            (hand.lunge() - 1.0).abs() < 1e-3,
            "the blow is sent at {} of full extension, not at the point's furthest",
            hand.lunge()
        );
        assert_eq!(impact_seconds(None), 0.0, "a punch waits for a peak it does not have");
    }

    #[test]
    fn a_click_during_a_thrust_starts_nothing_and_queues_nothing() {
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        let spear = Some(BLOCK_FLINT_SPEAR);
        let start = Instant::now();
        let at = |seconds: f32| start + Duration::from_secs_f32(seconds);
        let mut strikes = Strikes::default();

        assert_eq!(
            strikes.frame(at(0.0), true, spear),
            Beat { starts: true, lands: false },
            "the click did not start a thrust, or landed it before the point was out"
        );
        // The button held -- which is a click every frame -- all through it.
        let hit = impact_seconds(spear);
        let (mut landed, mut t) = (0, 0.0);
        while t < 0.95 {
            t += 1.0 / 60.0;
            let beat = strikes.frame(at(t), true, spear);
            assert!(!beat.starts, "a second thrust started {t}s into the first");
            if beat.lands {
                landed += 1;
                assert!(
                    (t - hit).abs() <= 1.0 / 60.0 + 1e-4,
                    "the thrust landed at {t}s, and its point was out at {hit}s"
                );
            }
        }
        assert_eq!(landed, 1, "one thrust landed {landed} times");
        // Let go before the second was up: nothing had been waiting.
        assert_eq!(strikes.frame(at(1.2), false, spear), Beat::default(), "a click from inside the thrust was queued");
        // ...and the next click is a thrust at once.
        assert!(strikes.frame(at(1.3), true, spear).starts, "a spear ready again did not thrust");
    }

    #[test]
    fn a_punch_lands_on_the_click_and_a_spear_put_away_mid_thrust_lands_nothing() {
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        let start = Instant::now();
        let at = |seconds: f32| start + Duration::from_secs_f32(seconds);

        let mut fist = Strikes::default();
        assert_eq!(fist.frame(at(0.0), true, None), Beat { starts: true, lands: true });

        let spear = Some(BLOCK_FLINT_SPEAR);
        let mut strikes = Strikes::default();
        assert!(strikes.frame(at(0.0), true, spear).starts);
        assert_eq!(
            strikes.frame(at(0.2), true, None),
            Beat::default(),
            "a bare hand swapped in mid-thrust punched before a fist's rate allowed"
        );
        assert_eq!(
            strikes.frame(at(0.5), false, spear),
            Beat::default(),
            "a thrust abandoned by spinning the wheel still landed"
        );
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

    /// **The report**: "у предметов нету 3д модели только блок или
    /// текстура". A barrel was held as the sprite cut from its icon, and a
    /// table -- with no icon -- as a cube of planks. Held, each is the model
    /// the world stands it as: every quad of `mesh::carried_model` and
    /// nothing else, and neither the cube's six faces nor a plate.
    #[test]
    fn a_block_with_a_model_of_its_own_is_held_as_that_model() {
        use primitive_shared::types::{BLOCK_BARREL, BLOCK_BED, BLOCK_JUG, BLOCK_TABLE};
        for block in [BLOCK_BARREL, BLOCK_JUG, BLOCK_TABLE, BLOCK_BED] {
            let (mut model, mut model_indices) = (Vec::new(), Vec::new());
            assert!(crate::engine::mesh::carried_model(block, &layers(), &mut model, &mut model_indices));
            let held = build(&Hand::new(), true, Some(block));
            assert_eq!(
                held.len(),
                model.len(),
                "{} is held as {} corners where its model has {}",
                primitive_shared::types::block_name(block),
                held.len(),
                model.len()
            );
            assert!(held.len() > 24, "a model of one box is a cube");
        }
        // ...and a block that is a cube in the world is still one in the hand.
        assert_eq!(build(&Hand::new(), true, Some(primitive_shared::types::BLOCK_STONE)).len(), 24);
    }

    #[test]
    fn a_rod_goes_back_as_far_as_the_throw_is_wound_and_whips_forward_past_level_and_home() {
        let rod = primitive_shared::types::BLOCK_FISHING_ROD;
        let mut hand = Hand::new();
        let tip = |hand: &Hand| hand.pose(rod).transform_point3(ITEM_CENTRE);
        let rest = tip(&hand);
        // Half a wind-up is half as far back as a whole one: the arm is the
        // gauge.
        for _ in 0..60 {
            hand.wind_rod(Some(0.5), 1.0 / 60.0);
        }
        let half = hand.rod_pitch();
        for _ in 0..60 {
            hand.wind_rod(Some(1.0), 1.0 / 60.0);
        }
        let full = hand.rod_pitch();
        assert!((full - ROD_WIND_PITCH).abs() < 0.01, "a full wind-up is {full}, not {ROD_WIND_PITCH}");
        assert!((half - full / 2.0).abs() < 0.02, "half a wind-up drew the rod back {half} of {full}");
        assert!(tip(&hand).y > rest.y + 0.05, "a rod drawn back did not go up over the shoulder");
        // Let go: it starts from where it was, goes past level, and comes
        // home when the throw is over.
        hand.cast_rod();
        assert!((hand.rod_pitch() - full).abs() < 1e-4, "the throw jumped the rod from {full} to {}", hand.rod_pitch());
        let mut furthest = f32::MAX;
        let mut t = 0.0;
        while t < ROD_WHIP_SECONDS + 0.1 {
            hand.update(1.0 / 60.0, false, 0.0, true, None);
            hand.wind_rod(None, 1.0 / 60.0);
            furthest = furthest.min(hand.rod_pitch());
            t += 1.0 / 60.0;
        }
        assert!(furthest < -0.3, "the throw never carried the rod past level (at most {furthest})");
        assert!(hand.rod_pitch().abs() < 1e-4, "the rod did not come home after the throw");
        assert!((tip(&hand) - rest).length() < 1e-4, "the hand is not where it started");
        // ...and nothing else in the hand turns with a rod's wind-up.
        let mut wound = Hand::new();
        wound.wind_rod(Some(1.0), 1.0);
        let stone = primitive_shared::types::BLOCK_STONE;
        assert_eq!(wound.pose(stone), Hand::new().pose(stone), "a stone in hand was drawn back like a rod");
    }
}
