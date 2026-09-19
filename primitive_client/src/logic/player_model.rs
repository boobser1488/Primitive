//! What a player looks like: a table of boxes wearing one skin.
//!
//! ## What this replaces
//!
//! Six flat-coloured slabs. A head, a wide plate for the chest, two
//! more slabs for the arms drawn in the *same colour as the chest they
//! touch*, and two legs -- all of it one salmon pink, because the actor
//! pipeline had no texture to sample and nothing but a per-part
//! brightness to tell one part from another. What that reads as, at the
//! ten metres a player is usually seen from, is a pink blank with no
//! arms and no face, which is exactly how it was reported.
//!
//! ## Why a table, and why here
//!
//! The same argument [`crate::logic::animal_model`] makes at length:
//! a model written out longhand inside a mesh builder is arithmetic
//! nobody can edit, and every proportion is buried in an expression.
//! A model is a list of [`Part`]s and the builder is one loop over it,
//! so making the shoulders broader is changing one number.
//!
//! ## The units
//!
//! Thirty-seconds of the player's own height, which is the grid every
//! blocky humanoid since 2009 has been drawn on: head 8, torso 12,
//! limbs 12. `SCALE` is the only place that stops being true, and it is
//! written as a fraction of `PLAYER_HEIGHT` on purpose -- the collider,
//! the anti-cheat and the aim all read that constant, and a model
//! measured in absolute blocks would drift away from the box it is
//! supposed to be filling the first time anybody changed it.
//!
//! The origin is the **feet**, not the middle: that is the point the
//! server sends for a player (`PlayerState::y`), and converting between
//! the two in two places is how a model ends up buried to the shins.
//! Animals differ here, and they differ because their own origin is
//! their centre.
//!
//! ## Why the skin is its own picture and not atlas layers
//!
//! An animal wears one *whole picture per face*, because a terrain
//! vertex carries its texture coordinates in two bits and cannot name a
//! corner of an image. That works for a boar -- hide, an eye, a snout,
//! nine pictures and done -- and it does not work for a face. A human
//! head needs the eyes on the front, hair on the sides and the back, and
//! nothing on the underside of the jaw; on the terrain path that is four
//! more atlas layers before anybody draws a shirt, out of the sixty-odd
//! that are left in an array the hardware caps at 256.
//!
//! So the player rides the actor pipeline, which is already its own
//! shader, and gets **one 64x32 skin sheet of its own** -- a texture
//! outside the block array entirely, costing none of its layers, with
//! ordinary floating-point UVs that can address a rectangle of it per
//! face. See `net`, `vs_actor` in `engine/shader.wgsl` and
//! `GraphicsState::player_skin`.
//!
//! Rejected: putting the player in `ANIMAL_SHEETS` as a sixth animal.
//! It would have cost twelve atlas layers, given every face one picture
//! stretched over it, and made "the player" a species -- and the model
//! would still have had no way to put an eye anywhere but in the middle
//! of a face.
//!
//! ## Who is drawn with this, and who is not
//!
//! **Other players, and only other players.** There is no third-person
//! camera in this game, so the local player is never drawn in the world
//! -- and the first-person view deliberately draws no arm either (see
//! `logic::hand`, which argues the point at length). So there is no
//! second copy of this figure anywhere to drift out of step with it,
//! and nothing to reconcile if a third-person view is added later: it
//! would call `append` with the local player's own pose and get exactly
//! what everybody else already sees.
//!
//! ## What moves
//!
//! `Joint` says how a part answers to walking, jumping and striking a
//! blow. The client knows how fast another player is going -- it has two
//! snapshots and the interval between them -- so the legs swing without
//! the server sending a single extra byte, exactly as an animal's do.

use glam::{Mat3, Mat4, Vec3};

use primitive_shared::equipment::Slot;
use primitive_shared::geometry::PLAYER_HEIGHT;
use primitive_shared::protocol::{Outfit, Posture};
use primitive_shared::types::{BlockId, BLOCK_AIR};

use crate::engine::item_model::ItemVertex;
use crate::engine::mesh::{face_uv, faces, Vertex};
use crate::engine::texture::FaceLayers;
use crate::net::remote_players::ActorVertex;

/// How tall the model is in its own units. The whole figure is exactly
/// this, so `SCALE` puts the top of the head at `PLAYER_HEIGHT`.
pub const TALL: f32 = 32.0;

/// One model unit, in blocks.
pub const SCALE: f32 = PLAYER_HEIGHT / TALL;

/// The skin sheet, in texels.
///
/// A resource pack may hand over a bigger one -- the UVs below are
/// fractions of these two numbers, so a 128x64 redraw of the same layout
/// works with no code change at all. What may *not* change is the
/// layout: see `net`.
pub const SHEET_WIDTH: f32 = 64.0;
pub const SHEET_HEIGHT: f32 = 32.0;

/// How a part answers to what the player is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    /// Rides with the body: the torso, and nothing else.
    Fixed,
    /// Nods with the player's pitch, about the neck.
    ///
    /// **The head carries the whole of the look angle.** A body that
    /// leaned when a player looked down would read as a bow, and at
    /// this scale a bow and a stumble are the same picture.
    Head,
    /// Swings about the shoulder or the hip. The four are named rather
    /// than given a sign, because which limb leads is the entire
    /// difference between a walk and a hop and it should be readable in
    /// the table.
    ArmRight,
    ArmLeft,
    LegRight,
    LegLeft,
}

/// One box of a player.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    /// What it is. Read by a person and by the tests; nothing that
    /// draws looks at it. A table of unnamed numbers is the thing this
    /// module exists to avoid.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Centre of the box, in units, from the point between the feet.
    pub at: [f32; 3],
    /// How big, in units.
    pub size: [f32; 3],
    /// Top-left corner of this box's net on the skin sheet, in texels.
    /// See `net`.
    pub sheet: [f32; 2],
    pub joint: Joint,
    /// The height the part turns about, in units.
    ///
    /// A limb swings about its *top*: a leg pivoting on its own middle
    /// puts its foot through the ground on the way back, which is the
    /// fault `animal_model` documents at length and the reason this is
    /// a column rather than a guess.
    pub pivot: f32,
}

/// The player.
///
/// **Six boxes, and every number in here answers a complaint.**
///
/// * The head is 8 across on a torso 7 across. Slightly over-sized, the
///   way every readable blocky figure is: what identifies a person at
///   ten metres is the head, and a head in proportion is four pixels on
///   screen at that distance.
/// * The arms are 3 across, not Minecraft's 4, and the whole figure
///   spans 0.73 blocks against a collider 0.6 through. Some overhang at
///   the shoulder is unavoidable in any model with arms beside a torso
///   -- Minecraft's own is 0.9 wide in the same 0.6 box -- and this is a
///   third of it. What overhangs is the outer face of an arm, which is
///   the least of a person to be unable to hit.
/// * Parts meet flush, and that is safe *here* for a reason worth
///   writing down: this pipeline culls back faces, and where two boxes
///   share a plane they face opposite ways, so exactly one of the pair
///   survives culling from any viewpoint. There is no z-fighting to
///   avoid and no need for the overlaps a model on a two-sided pass
///   would want.
///
/// The sizes are also the layout of the skin: `net` cuts the picture up
/// by the same numbers, so a part that changes size takes its share of
/// the sheet with it.
pub const PARTS: &[Part] = &[
    Part {
        name: "head",
        at: [0.0, 28.0, 0.0],
        size: [8.0, 8.0, 8.0],
        sheet: [0.0, 0.0],
        joint: Joint::Head,
        // The neck: where the head meets the shoulders, so looking down
        // turns the head over the collar rather than about the ears.
        pivot: 24.0,
    },
    Part {
        name: "torso",
        at: [0.0, 18.0, 0.0],
        size: [7.0, 12.0, 4.0],
        sheet: [0.0, 16.0],
        joint: Joint::Fixed,
        pivot: 24.0,
    },
    // Beside the chest rather than in front of it, and swinging: an arm
    // drawn the same colour as the chest it touches has no silhouette at
    // all, which is how six boxes read as four. "Рук не видно вовсе."
    Part {
        name: "arm right",
        at: [5.0, 18.0, 0.0],
        size: [3.0, 12.0, 3.0],
        sheet: [22.0, 16.0],
        joint: Joint::ArmRight,
        pivot: 24.0,
    },
    Part {
        name: "arm left",
        at: [-5.0, 18.0, 0.0],
        size: [3.0, 12.0, 3.0],
        sheet: [22.0, 16.0],
        joint: Joint::ArmLeft,
        pivot: 24.0,
    },
    Part {
        name: "leg right",
        at: [1.5, 6.0, 0.0],
        size: [3.0, 12.0, 3.0],
        sheet: [34.0, 16.0],
        joint: Joint::LegRight,
        pivot: 12.0,
    },
    Part {
        name: "leg left",
        at: [-1.5, 6.0, 0.0],
        size: [3.0, 12.0, 3.0],
        sheet: [34.0, 16.0],
        joint: Joint::LegLeft,
        pivot: 12.0,
    },
];

// Which row of `PARTS` is which. Named because [`OVERLAYS`] and
// [`hand_point`] both index into it, and a table addressed by bare
// integers is a table nobody can safely reorder -- there is a test below
// that these still name what they say they name.
pub const HEAD: usize = 0;
pub const TORSO: usize = 1;
pub const ARM_RIGHT: usize = 2;
pub const ARM_LEFT: usize = 3;
pub const LEG_RIGHT: usize = 4;
pub const LEG_LEFT: usize = 5;

/// One piece of clothing: a copy of a body part, a little larger, drawn
/// in the material's own colour.
///
/// ## Why a shell and not a picture
///
/// **There is no room for a picture.** Twelve garments already share
/// four greyscale images in the block atlas and are told apart by a tint
/// (`types::garment_tint`); the atlas is capped at 256 layers by the
/// hardware and this build is within a handful of that ceiling, and the
/// player skin is a separate 64x32 sheet with no space on it for a
/// second set of limbs. Drawing armour properly -- its own texture, its
/// own net -- costs either a redrawn skin sheet per material or a page
/// of the atlas, and neither exists.
///
/// What does exist is the tint. The actor shader multiplies the skin by
/// the vertex colour, so a box a couple of centimetres outside a limb,
/// wearing that limb's own picture in leather brown or iron blue-grey,
/// reads at the distance a player is actually seen from as somebody
/// dressed -- and it costs nothing but geometry. The material is
/// distinguishable at a glance, which is the only job the tint has in
/// the pack screen either.
///
/// ## Why a span rather than one shell per part
///
/// There are four slots and six body parts, and they do not line up: a
/// chest covers the torso *and* both upper arms, and the legs and the
/// feet share one box each. Without the span, boots would be the one
/// garment in the game that cannot be seen -- there is no foot in
/// `PARTS` to hang them on, and adding one means a redrawn skin sheet.
#[derive(Debug, Clone, Copy)]
pub struct Overlay {
    /// Which body part it rides on, as an index into [`PARTS`].
    pub part: usize,
    /// Which worn slot fills it.
    pub slot: Slot,
    /// The band of the part it covers, measured from the top of the box
    /// downward, 0..1.
    pub span: (f32, f32),
}

/// What is drawn over what.
///
/// The sleeves stop at three quarters of the arm: a garment that ran to
/// the end of it would be a glove, and none of the twelve is one.
pub const OVERLAYS: &[Overlay] = &[
    Overlay { part: HEAD, slot: Slot::Head, span: (0.0, 1.0) },
    Overlay { part: TORSO, slot: Slot::Chest, span: (0.0, 1.0) },
    Overlay { part: ARM_RIGHT, slot: Slot::Chest, span: (0.0, 0.75) },
    Overlay { part: ARM_LEFT, slot: Slot::Chest, span: (0.0, 0.75) },
    Overlay { part: LEG_RIGHT, slot: Slot::Legs, span: (0.0, 0.7) },
    Overlay { part: LEG_LEFT, slot: Slot::Legs, span: (0.0, 0.7) },
    Overlay { part: LEG_RIGHT, slot: Slot::Feet, span: (0.7, 1.0) },
    Overlay { part: LEG_LEFT, slot: Slot::Feet, span: (0.7, 1.0) },
];

/// The rucksack on a player's back: where it is and how big, in model
/// units.
///
/// ## Why it is not an [`Overlay`]
///
/// An overlay is a *shell*: a copy of a body part, a little larger, so a
/// tunic is the torso wearing the torso's own picture. A rucksack is not
/// a copy of anything -- it is a box that hangs off the back of the
/// torso, and putting it in that table would have meant a span that runs
/// from 0 to 1 and a `PADDING` of two and a half units on all six sides,
/// which is a bag that swallows the wearer.
///
/// ## Why it is not a `.bbmodel` either
///
/// Every `.bbmodel` in `assets/models` is read by the *terrain* path:
/// its faces name pictures in the block atlas and the boxes end up in a
/// chunk mesh. A player is not on that path and cannot be -- see the
/// note at the top of this file about the 64x32 skin sheet and why the
/// figure rides the actor pipeline. A rucksack drawn from a `.bbmodel`
/// would need a second loader, a second vertex type and a picture in the
/// atlas, and what it would buy is one box that anybody can already move
/// by editing the four numbers below.
///
/// ## The numbers
///
/// `at` is the centre and `size` is the whole box, both in thirty-seconds
/// of the player's height like everything else here. The torso is four
/// units deep and centred on z = 0, so its back face is at z = 2; a worn
/// chest piece grows that to 2.35 (`PADDING`). The pack's front face is
/// at 2.35 exactly, which is the one number here that is not a taste:
/// two faces pointing the same way at one depth flicker, and a pack
/// *inside* a cuirass is a pack that vanishes when its owner puts armour
/// on.
///
/// It sits high on the back -- y 15.25 to 22.75 against a torso of 12 to
/// 24 -- because that is where a framed pack rides, and because a bag
/// hung level with the hips is a bag that clips through the legs as they
/// swing.
///
/// Its front face lands at z = 2.40, which is a twentieth of a unit
/// clear of the 2.35 a worn cuirass reaches. It was exactly 2.35, and
/// coplanar is the one thing a face must not be: here the two point in
/// opposite directions, so culling would keep one of them -- but the
/// actor pass does not cull, and two faces at one depth flicker.
const PACK_AT: [f32; 3] = [0.0, 19.0, 4.10];
const PACK_SIZE: [f32; 3] = [6.0, 7.5, 3.4];

/// What colour the rucksack is drawn.
///
/// **A constant here rather than a row in `types::garment_tint`**, and
/// the reason is the same one `equipment::slot_of` gives for keeping the
/// rucksack out of the garment table: that table is read by the systems
/// that decide how warm and how protected a player is, and a rucksack
/// has nothing to say to either. The item picture is drawn in its own
/// colours and is deliberately *not* tinted (`hotbar::icon_tint` leaves
/// it alone); this is the tint for the shell on a back, which wears the
/// torso's greyscale skin and needs a colour to become leather.
///
/// Hide and a stick frame, which is what the recipe is made of.
const PACK_TINT: [f32; 3] = [0.52, 0.37, 0.24];

/// How far off the armour's back a pack may float before it stops
/// reading as worn, in model units. A test's bound, not a drawing one.
#[cfg(test)]
const CELL_CLEARANCE: f32 = 0.5;

/// How far a garment stands off the body it covers, in model units.
///
/// A third of a unit is about two centimetres on a 1.8 m figure: enough
/// that a cuirass has bulk and reads as worn rather than painted on,
/// small enough that the whole dressed figure is still inside the 0.8
/// blocks the bare one is checked against -- see
/// `a_player_stands_in_the_box_the_rest_of_the_game_gives_them`, whose
/// dressed counterpart holds the same line.
const PADDING: f32 = 0.35;

/// How far the *bottom* of a garment is pulled up inside the part it
/// covers, in model units.
///
/// **Not a preference -- the rule is that a garment box must never share
/// a plane with a face pointing the same way.** Where two spans meet in
/// the middle of a limb (a legging's hem against a boot's cuff) the two
/// faces point in opposite directions and back-face culling keeps
/// exactly one, which is the same argument [`PARTS`] makes about parts
/// meeting flush. At the *ends* of a part they point the same way, and
/// two coplanar faces at one depth flicker.
///
/// The top is solved by [`PADDING`] -- a hat sits on top of a head, so
/// letting the crown rise two centimetres is what it should do anyway.
/// The bottom cannot be solved the same way: a sole two centimetres
/// below the feet reads as a player sunk into the ground. So it is
/// lifted a hair *inside* the leg instead, where the leg's own opaque
/// underside is in front of it -- and the only face involved is a sole
/// nobody ever looks at.
const SEAM: f32 = 0.02;

/// Where one face of one part sits on the skin sheet, in texels, as
/// `[x, y, width, height]`.
///
/// **The net is the classic unfolded box**: the four sides in a strip,
/// with the top and the bottom above them, offset by the box's depth.
/// Written as arithmetic on the part's own size rather than as a table
/// of rectangles, so a part that changes size takes its picture with it
/// and the sheet cannot silently start showing the wrong region.
///
/// **Both arms share one net, and both legs share another.** The two
/// sides of a box are mirror images of each other -- `face_uv` maps
/// `u = 1 - z` on +X and `u = z` on -X -- so one picture on a left and
/// a right limb comes out flipped on one of them. On an animal's head
/// that is fatal and needs a second, mirrored picture (see
/// `animal_model::Skin::HeadMirror`); on a sleeve it is invisible,
/// because a sleeve is symmetric about its own middle. Two more nets
/// would be a quarter of the sheet spent on a difference nobody can
/// see.
///
/// `gen_placeholder_textures.rs` draws into the same net, and has its
/// own copy of this arithmetic because an example can only see the
/// crate's public surface. The two are pinned together by
/// `every_face_of_the_model_lands_on_a_painted_part_of_the_skin`, which
/// reads the shipped picture and checks that every rectangle this
/// returns has paint in it -- a drift in either copy makes a face
/// transparent, and the test sees it before a player does.
pub fn net(part: &Part, face_index: usize) -> [f32; 4] {
    let [x, y] = part.sheet;
    let [w, h, d] = part.size;
    // The mesher's face order is 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z, and
    // a player faces -Z in their own space -- so face 5 is the face.
    match face_index {
        0 => [x + d, y, w, d],                 // +Y  crown
        1 => [x + d + w, y, w, d],             // -Y  underside
        2 => [x, y + d, d, h],                 // +X  their right
        3 => [x + d + w, y + d, d, h],         // -X  their left
        4 => [x + d + w + d, y + d, w, h],     // +Z  back
        _ => [x + d, y + d, w, h],             // -Z  front
    }
}

/// How far a limb swings at a walking pace, in radians.
///
/// Scaled by speed and capped, so a sprint is a longer stride and not a
/// windmill. A third of a right angle is about what a person does;
/// more reads as a march.
const SWING: f32 = 0.55;

/// The speed a full swing belongs to, in blocks a second: the game's
/// walk. Below it the stride shortens in proportion, above it the swing
/// stops growing.
pub const WALKING: f32 = 4.3;

/// Below this, in blocks a second, a player is standing still.
///
/// A dead zone, and it is the whole of the fix for the fidgeting. A
/// player who has stopped is still being interpolated toward the last
/// snapshot for a fraction of a second, and with no floor under the
/// swing that fraction is a full-amplitude stride performed on the spot.
const STANDING: f32 = 0.35;

/// Whole strides -- a step with each foot -- per block walked.
///
/// Tied to distance rather than to time, which is what stops a player
/// from moon-walking: somebody who has stopped has legs that have
/// stopped, and somebody sprinting has them going twice as fast as
/// somebody walking without anybody choosing a second number.
///
/// **It was 2.2, and 2.2 is the footstep's length the wrong way up.** The
/// soundscape plays a footfall every `STEP_BLOCKS` walked; this read the
/// same number as strides *per block*, so at a walk the legs went through
/// nine and a half strides a second -- nineteen steps -- against the two
/// footfalls a second the player hears. That was the figure "animated too
/// fast". Now it is derived from the step and cannot come apart from it:
/// a stride is two steps, so a walking figure plants a foot on every
/// footfall, about two a second, which is a person's cadence.
pub const PACES_PER_BLOCK: f32 = 0.5 / STEP_BLOCKS;

/// How far a player walks between footfalls, in blocks: the one length both
/// the legs (`PACES_PER_BLOCK`) and the footsteps (`audio::soundscape`) are
/// measured by.
pub const STEP_BLOCKS: f32 = 2.2;

/// How far the arms hang forward when nothing is happening, in radians.
///
/// **A rest pose is not attention.** Arms hanging dead straight against
/// the sides is a soldier on parade, and it is the first thing that
/// makes a figure read as a prop rather than a person. Eight degrees
/// forward, with the shoulders unlevel by a breath, is what a standing
/// human actually does.
const REST_LEAN: f32 = 0.14;

/// How far the breath moves the arms, in radians, and how often.
///
/// Small enough that nobody can point at it and slow enough that it is
/// not a fidget: twelve breaths a minute is a person standing still.
const BREATH: f32 = 0.035;
const BREATHS_PER_SECOND: f32 = 0.2;

/// How far the arms come up while off the ground, in radians.
///
/// Backwards, which is what a body does when its feet stop carrying it.
const AIRBORNE_ARM: f32 = -0.5;

/// How far a limp bends the walk cycle out of shape, as a fraction of a
/// radian of phase added to it at the worst.
///
/// **A limp is uneven timing before it is anything else.** The tell is not
/// that the step is small, it is that the two halves of the stride take
/// different lengths of time: the sound leg holds the weight and the bad one
/// is got over and put down again quickly. A sine added to the phase is
/// exactly that -- it runs the cycle fast through one half and slow through
/// the other, without a second clock and without the legs ever leaving each
/// other's opposite.
///
/// Rejected: **two phases, one per leg.** It is the obvious shape and it
/// breaks the one thing a walk cannot get wrong -- the legs would drift until
/// both were forward at once, which is a figure falling over rather than one
/// limping.
const LIMP_SKEW: f32 = 0.9;

/// ...and how much of the swing the bad leg gives up. A short step on one
/// side, which is the other half of what an eye reads.
const LIMP_SHORT: f32 = 0.45;

/// How far the body drops onto the bad leg, in model units.
///
/// Three sixteenths of the figure's height at the worst: a dip a watcher sees
/// at twenty blocks and does not see as the model sinking into the ground,
/// because the legs do not go with it (see `place`).
const LIMP_DIP: f32 = 1.6;

/// What the player is doing, as far as the model needs to know.
///
/// One struct rather than eight arguments, because the difference
/// between `walked` and `speed` is not visible at a call site and
/// getting them the wrong way round is a figure that skates.
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    /// Which way they are facing, in radians. Zero looks along +X, the
    /// same convention `Camera::forward` uses.
    pub yaw: f32,
    /// Where they are looking, positive upward. Only the head takes it.
    pub pitch: f32,
    /// How far they have walked in total, in blocks: the gait's clock.
    pub walked: f32,
    /// How fast they are going now, in blocks a second: how *far* the
    /// legs swing, and whether they swing at all.
    pub speed: f32,
    /// Off the ground. Legs that keep walking in mid-air are the second
    /// most obvious thing a model can get wrong, after walking on the
    /// spot.
    pub airborne: bool,
    /// What the right arm is doing besides walking, and how far into it.
    ///
    /// **On the wire now** (`protocol::Gesture`), which is what this note
    /// used to say was missing: the server judged mining and combat and told
    /// nobody, so another player breaking a block was a statue. The swing
    /// used to be the only shape here; what each gesture looks like is
    /// [`Arm`], and how long it lasts is `RemotePlayer::pose`'s -- where a
    /// blow is a fraction of `hand::blow_seconds` for what they hold, so a
    /// spear's thrust takes the same second on everybody's screen that it
    /// takes in its owner's hand.
    ///
    /// Still never *invented* from something the client can see, like a
    /// block changing nearby: that is a figure flailing at things other
    /// people did.
    pub arm: Option<Arm>,
    /// Seconds this figure has been on screen, for the breath.
    pub age: f32,
    /// What they have on and what is in their hand, straight off the
    /// snapshot -- see `protocol::Outfit`.
    ///
    /// Taken as it is rather than eased: what somebody is wearing changes
    /// rarely and is the first thing another player looks at.
    pub outfit: Outfit,
    /// On their feet, on a seat, or lying down -- see `postured`.
    pub posture: Posture,
    /// How badly they are limping, 0 sound to 1 barely walking.
    ///
    /// **Off the wire** (`protocol::PlayerState::limp`), which is the whole
    /// point of it: a broken leg, exhaustion and a body down to its last
    /// points of health have always slowed a player down, and on every other
    /// screen the figure walked home at that slower pace with a perfectly
    /// even stride. Nobody watching could tell somebody hurt from somebody
    /// strolling. See `LIMP_SKEW` for what it does to the walk.
    pub limp: f32,
    /// The limp is on the left leg rather than the right. Off the wire too
    /// (`protocol::PlayerState::limp_left`); see `limp_side`.
    pub limp_left: bool,
}

impl Default for Pose {
    fn default() -> Self {
        Pose {
            yaw: 0.0,
            pitch: 0.0,
            walked: 0.0,
            speed: 0.0,
            airborne: false,
            arm: None,
            age: 0.0,
            outfit: Outfit::BARE,
            posture: Posture::Standing,
            limp: 0.0,
            limp_left: false,
        }
    }
}

/// One thing the right arm does, and how far through it, 0..1.
///
/// **Five shapes, and not one blow played at five speeds**, because each is a
/// movement an eye tells apart from across a clearing: a swing comes over the
/// shoulder and down, a thrust drives out level, setting a block down is a
/// short reach, a mouthful goes to the face and stays there chewing, and a
/// drink goes higher with the head tipped back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arm {
    /// A blow, or a swing at a block: wind up, strike, recover.
    Swing(f32),
    /// A spear's thrust: drawn back, driven out, brought home.
    Thrust(f32),
    /// A block set down: a reach forward and back.
    Place(f32),
    /// To the mouth, chewing, and down.
    Eat(f32),
    /// Higher than a mouthful, the head tipped back.
    Drink(f32),
    /// Working at a block: the arm held up in front and chopping, `phase`
    /// through one blow of the digging rhythm, `raised` how far into the
    /// work the arm has come (0 at rest, 1 working).
    ///
    /// **Its own shape, and not `Swing` played over and over**, which is
    /// what it was. A swing is a blow from rest, over the shoulder and back
    /// to the side -- and at the digging rhythm (`hand::SWING_SECONDS`, the
    /// pace the cracks spread and the knocks sound) that is the whole arm
    /// windmilling through nine radians three and a half times a second.
    /// Somebody chopping keeps the arm up and moves it through the last
    /// part of the blow; `raised` eases it up at the start of the work and
    /// down at the end, so the first and last blows do not snap.
    Chop { phase: f32, raised: f32 },
    /// A fishing rod drawn back over the shoulder for a cast, `raised` how
    /// far (0 at rest, 1 all the way). What `Gesture::digging` means with a
    /// rod in the hand: the watcher's picture of `hand::Rod`'s wind-up.
    Wind(f32),
    /// The throw off a wind-up, 0..1 over `hand::ROD_WHIP_SECONDS`: from
    /// drawn back to pointing down the line, fast, and home. Starts where
    /// [`Arm::Wind`] at full ends, because that is where the arm is.
    Cast(f32),
}

/// Where the arm is at a full wind-up, in radians forward: up past the top
/// of the head and a little behind it, the rod laid back over the shoulder.
const ROD_BACK: f32 = 2.7;
/// Where the throw carries it: a little under level, the rod pointing down
/// the line at the water.
const ROD_OUT: f32 = 1.35;
/// The share of a throw spent getting there; the rest is the arm coming
/// home. The player's own hand's split, read rather than copied, so the rod
/// is out at the same moment on both screens.
const ROD_CAST_OUT: f32 = crate::logic::hand::ROD_WHIP_OUT;

/// How long another player's placing, eating and drinking take on screen,
/// in seconds.
///
/// **Long enough to be seen at the snapshot rate and no longer than the
/// thing takes.** A placement is a flick of the wrist, but a tenth of a second
/// is two snapshots and reads as a twitch; a third is a reach. A mouthful is a
/// second and a bit -- hand up, a few chews, hand down -- and a drink a little
/// longer, because tipping a jug is slower than biting. None of them gates
/// anything: the server has already eaten the food by the time this starts.
pub const PLACE_SECONDS: f32 = 0.3;
pub const EAT_SECONDS: f32 = 1.2;
pub const DRINK_SECONDS: f32 = 1.4;

/// How far a placement reaches, in radians forward.
const PLACE_REACH: f32 = 0.9;
/// Where a hand at the mouth is: forward and up past level, the fist at the
/// height of the mouth -- and how far it bobs while chewing, and how many
/// chews a mouthful is.
///
/// **It was 2.2, and 2.2 is over the top of the head.** An arm with no elbow
/// that turns at the shoulder puts its fist 10.5 units from the joint; at
/// 2.2 radians that is 6.2 units above the shoulder, and the top of the head
/// is 8. The mouth is two units up -- `cos⁻¹(-2/10.5)` is 1.76 -- which is
/// what `a_mouthful_is_held_in_the_hand_and_lifted_to_the_mouth_and_not_over_the_head`
/// measures. The whole angle of the arm, not an addition to its rest: see
/// `arm_angle`.
const EAT_RAISE: f32 = 1.76;
const CHEW: f32 = 0.12;
const CHEWS: f32 = 4.0;
/// A drink is held higher, to the lips with the vessel tipped, and the head
/// goes back to meet it. It was 2.6: a jug held up over the head like a
/// trophy.
const DRINK_RAISE: f32 = 2.0;
const DRINK_TILT: f32 = 0.45;
/// Where a chopping arm works between, in radians forward: from about level
/// with the chest at the end of a blow to up past the face at the top of the
/// next.
const CHOP_LOW: f32 = 0.75;
const CHOP_HIGH: f32 = 2.1;
/// A thrust with an arm that only turns at the shoulder: brought up nearly
/// level and then driven from below level to level -- see `thrust_angle` --
/// with the draw going back a share of the drive first.
const THRUST_LEVEL: f32 = 1.1;
const THRUST_DRIVE: f32 = 0.45;
const THRUST_DRAW: f32 = 0.3;

/// How far a seated figure's legs are swung forward, in radians: level.
///
/// A one-box leg has no knee, so a sitter's legs stick out in front of them
/// the way a doll sits. The alternative -- legs hanging straight down --
/// is a figure standing inside the stool with its hips at the seat.
const SEATED_LEGS: f32 = std::f32::consts::FRAC_PI_2;

/// How far a seated figure is let down, in model units: a leg's length
/// less half its thickness, so the thighs rest on the seat the server put
/// the feet on rather than a leg's length above it.
const SEAT_DROP: f32 = 10.0;

/// How far a rider is let down, in model units: so the seat of their
/// trousers -- the bottom of the torso, at the hip, twelve units up -- is on
/// the top of the saddle.
///
/// **Measured off the horse, not written down here.** The server puts a
/// rider's feet `horse::RIDER_LIFT` over the horse's (`horse::Horse::seat`),
/// and the saddle is laid on the horse model's back (`animal_model::saddle_top`),
/// so the gap between the two is the only number that decides where a rider
/// sits -- and a horse made taller in Blockbench takes its rider up with it
/// rather than sitting them inside its back. Usually a little over a unit:
/// the lift is a block less than a leg under the saddle.
fn mount_drop() -> f32 {
    let seat = crate::logic::animal_model::saddle_top() - primitive_shared::horse::RIDER_LIFT;
    PARTS[LEG_RIGHT].pivot - seat / SCALE
}

/// How far a rider's arms are carried forward, in radians: the hands low
/// over the withers, where the reins come back to.
const REINS: f32 = 0.75;

/// Where a leg bends, in model units up from the foot: half way.
///
/// **A rider is the one pose a one-box leg cannot make.** Sitting on a
/// chair, a straight leg stuck out in front reads as a doll sitting; astride,
/// a straight leg either goes forward along the horse's neck -- the leg of
/// a toy on a rocking horse -- or straight down through its barrel. What
/// a rider's leg does is go *out* over the back and then *down* the flank,
/// and that is two directions, so a mounted leg is drawn as two boxes: the
/// top half of the same leg, laid along `THIGH`, and the bottom half along
/// `SHIN`, each wearing its own half of the leg's picture (`astride`).
///
/// Rejected: **a knee on every figure**, which is a second joint in every
/// walk cycle, a redrawn skin net and a test suite that indexes parts by
/// arithmetic -- for a pose only a rider takes. Rejected too: **splaying the
/// straight leg outward** until it clears the barrel, which needs the feet a
/// block apart: a rider doing the splits.
const KNEE: f32 = 6.0;

/// How far each hip is moved out for a rider, in model units: the pelvis
/// opening round the saddle.
const HIP_SPREAD: f32 = 1.0;

/// Which way a rider's right thigh runs from the hip, in model units:
/// mostly out over the horse's back, a little down and a little forward.
///
/// **Out far enough that the knee is past the saddle's skirt**: the knee
/// ends a little over seven units from the middle, and a horse's barrel and
/// skirt are about six -- `a_rider_s_feet_hang_either_side_of_a_horse_and_not_through_it`
/// holds the two apart. The left is this mirrored.
const THIGH: Vec3 = Vec3::new(6.0, -2.0, -2.2);

/// ...and the shin from the knee: straight down the flank, heel a little back
/// and out, which is where the stirrup is.
const SHIN: Vec3 = Vec3::new(0.6, -6.0, 0.6);

/// How far each half of a bent leg runs past the knee, in model units, so the
/// two boxes meet in a joint rather than at an edge with daylight in the angle.
const KNEE_OVERLAP: f32 = 0.75;

/// Where a lying figure's middle is, in model units up from its feet: the
/// hip. The server lays a sleeper's feet at the middle of the bed, and the
/// body is laid out either side of that point.
const LYING_MIDDLE: f32 = 12.0;

/// How far a lying figure is lifted, in model units: half the depth of the
/// *head*, the deepest part of the figure, so the back of the head rests on
/// the mattress rather than in it.
///
/// It was half the torso's depth, which laid the back flat on the bed and
/// sank the back of an eight-unit head a ninth of a block into it --
/// `a_lying_figure_lies_flat_on_the_mattress_as_long_as_it_is_tall` caught
/// it. Lifted by the head, the body lies a hair above the blanket and the
/// head at about the height of the pillow under it.
const LYING_LIFT: f32 = 4.0;

/// The fire on a held torch's sprite, drawn through the sprite's own
/// `transform`: the rectangle `hand::flame_tile_on_sprite` measures, on both
/// faces of the plate. See the caller in `append_held`.
pub(crate) fn push_held_flame(transform: Mat4, flame: u32, item_vertices: &mut Vec<ItemVertex>, item_indices: &mut Vec<u32>) {
    use crate::logic::hand::RESOLUTION;
    let tile = crate::logic::hand::flame_tile_on_sprite();
    let proud = crate::engine::item_model::THICKNESS * 0.5 + 0.01;
    for z in [proud, -proud] {
        let at = |x: f32, y: f32| transform.transform_point3(Vec3::new(x / RESOLUTION - 0.5, 0.5 - y / RESOLUTION, z));
        let corners = [at(tile[0], tile[3]), at(tile[2], tile[3]), at(tile[2], tile[1]), at(tile[0], tile[1])];
        let base = item_vertices.len() as u32;
        let face = crate::engine::item_model::nearest_face((corners[1] - corners[0]).cross(corners[2] - corners[1]));
        let packed = (flame << 16) | crate::engine::mesh::pack_light(15, 15, 3, face);
        for (corner, uv) in corners.iter().zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]) {
            item_vertices.push(ItemVertex { position: corner.to_array(), uv, packed });
        }
        item_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// How far a swimmer's body is lifted off their feet, in model units: to
/// the water line a floating body rides at (`physics::FLOAT_SUBMERSION`,
/// a block and a half over the feet), less half the torso, so the back
/// is at the surface and the head just out of it -- which is what a swimmer
/// seen from the bank looks like, and a figure drawn at the feet would be a
/// swimmer lying on the lake bed.
const SWIM_LIFT: f32 = 22.0;

/// How fast a swimmer's arms go round, in strokes a block swum: slower than
/// a stride, because an arm pulls further than a leg steps.
const STROKES_PER_BLOCK: f32 = 0.45;

/// A point of the figure after its posture, before it is turned to its
/// yaw, in model units.
///
/// **Lying is a quarter turn about the figure's left-right axis**, and a
/// turn rather than a swap of two coordinates for the reason `place`
/// gives: a swap is a reflection, and a reflected box is wound inside out.
/// The turn sends the head from up to behind the figure and the face from
/// front to up -- on its back -- which is why a lying figure's yaw is half
/// a turn from the way its head points (see `RemotePlayer::pose`).
fn postured(posture: Posture, unit: Vec3) -> Vec3 {
    match posture {
        Posture::Standing => unit,
        Posture::Sitting => unit - Vec3::Y * SEAT_DROP,
        Posture::Mounted => unit - Vec3::Y * mount_drop(),
        // (x, y, z) -> (x, -z, y): up goes to +z, front (-z) goes to up.
        Posture::Lying => Vec3::new(unit.x, -unit.z + LYING_LIFT, unit.y - LYING_MIDDLE),
        // A dead figure is not drawn with this -- it lies on its side through
        // `append_lying`, as the body its death left does -- so there is
        // nothing to arrange.
        Posture::Fallen => unit,
        // (x, y, z) -> (x, z, -y): the other quarter turn from lying, so up
        // goes to front (-z) and the face goes down -- face down in the water,
        // head first the way they are swimming, the hips over the feet.
        Posture::Swimming => Vec3::new(unit.x, unit.z + SWIM_LIFT, -unit.y + LYING_MIDDLE),
    }
}

/// Where in the walk cycle they are, in radians, limp and all.
///
/// **One function because two things read it**: the legs, and the dip the
/// body takes onto the bad one (`limp_dip`). Two copies of the skew is a dip
/// that drifts out of phase with the step it belongs to, which reads as a
/// figure bobbing for no reason rather than as one favouring a leg.
fn stride_phase(pose: &Pose) -> f32 {
    let raw = pose.walked * PACES_PER_BLOCK * std::f32::consts::TAU;
    // Fast through one half of the cycle and slow through the other: see
    // `LIMP_SKEW`. A limp of zero leaves the phase exactly as it was.
    raw + limp_side(pose) * LIMP_SKEW * pose.limp.clamp(0.0, 1.0) * raw.sin()
}

/// Plus one for a limp on the right leg, minus one for the left.
///
/// **A sign on the skew and the dip, and nothing else**, because a left limp
/// is the right one half a stride later: `stride_phase` with its skew negated
/// at `raw + pi` is exactly the right-leg phase at `raw`, shifted by the half
/// turn that swaps which leg is forward. So the timing, the dip onto the bad
/// leg and the short step all mirror by flipping this one number and
/// swapping which leg is shortened -- no second copy of the gait.
fn limp_side(pose: &Pose) -> f32 {
    if pose.limp_left {
        -1.0
    } else {
        1.0
    }
}

/// How far the body drops onto the bad leg right now, in model units.
///
/// Deepest when the right leg is behind them -- which is where the weight is
/// as it takes the step -- and nothing at all when they are standing still,
/// sitting, lying, swimming or off the ground: a dip is a thing that happens
/// to a walk, and a figure that sank while standing on a ladder would read as
/// the model slipping.
fn limp_dip(pose: &Pose) -> f32 {
    let limp = pose.limp.clamp(0.0, 1.0);
    if limp <= 0.0 || pose.airborne || pose.posture != Posture::Standing || pose.speed < STANDING {
        return 0.0;
    }
    LIMP_DIP * limp * (0.5 - 0.5 * limp_side(pose) * stride_phase(pose).sin())
}

/// How far one joint is turned, in radians, positive swinging the far
/// end of the limb *forward*.
///
/// Public because the tests read it, and because it is the whole of the
/// animation: everything else in this file is geometry.
pub fn joint_angle(joint: Joint, pose: &Pose) -> f32 {
    // A stride, damped by how fast they are actually going: a walk and
    // a sprint are one cycle at two amplitudes.
    let pace = if pose.speed < STANDING {
        0.0
    } else {
        (pose.speed / WALKING).clamp(0.35, 1.0)
    };
    // Where in the cycle they are, from ground covered rather than from
    // a clock. Read by the limbs that are on the ground; the airborne
    // arms below never look at it, because a figure that keeps pedalling
    // through a jump is a cartoon.
    let stride = stride_phase(pose).sin() * SWING * pace;
    // ...and what a limp takes off the bad leg's half of it. See `LIMP_SHORT`.
    let short = 1.0 - LIMP_SHORT * pose.limp.clamp(0.0, 1.0);
    // The rest of the pose, which only shows through when the stride
    // does not: a standing figure leans its arms forward and breathes,
    // a walking one has no room for either.
    let idle = 1.0 - pace;
    let breath =
        (pose.age * BREATHS_PER_SECOND * std::f32::consts::TAU).sin() * BREATH * idle;

    // A body at rest does not walk. The legs of a seated figure are level
    // in front of it, and a lying figure's limbs lie along it -- whatever
    // the snapshot's speed says, which is the speed of being put on the
    // furniture and nothing a leg should answer.
    match (pose.posture, joint) {
        (Posture::Sitting, Joint::LegRight | Joint::LegLeft) => return SEATED_LEGS,
        // Astride, the legs are not swung at all: they are bent at the knee
        // round the horse (`astride`), which a swing about the hip cannot do.
        (Posture::Mounted, Joint::LegRight | Joint::LegLeft) => return 0.0,
        // Both hands forward and low, on the reins over the withers -- and the
        // right still carries whatever it is doing on top, so a rider can
        // strike from the saddle.
        (Posture::Mounted, Joint::ArmLeft) => return REINS,
        (Posture::Mounted, Joint::ArmRight) => {
            let base = REINS + held_lift(pose);
            return base + arm_angle(pose, base);
        }
        (Posture::Lying, Joint::LegRight | Joint::LegLeft | Joint::ArmRight | Joint::ArmLeft) => {
            return 0.0
        }
        // **A crawl**, from the water swum rather than from a clock, like the
        // stride -- and a slow scull on top of it, because a swimmer holding
        // still in deep water is treading it and not a statue. The arms go
        // round opposite each other from overhead (pi, which the quarter turn
        // makes ahead of the swimmer) to the hip (nought); the legs flutter;
        // the head is lifted to look where it is going.
        (Posture::Swimming, _) => {
            let stroke = (pose.walked * STROKES_PER_BLOCK + pose.age * 0.35) * std::f32::consts::TAU;
            let half = std::f32::consts::FRAC_PI_2;
            return match joint {
                Joint::Fixed => 0.0,
                Joint::Head => -0.9,
                Joint::ArmRight => half + half * stroke.sin(),
                Joint::ArmLeft => half - half * stroke.sin(),
                Joint::LegRight => 0.3 * (stroke * 3.0).sin(),
                Joint::LegLeft => -0.3 * (stroke * 3.0).sin(),
            };
        }
        _ => {}
    }
    match joint {
        Joint::Fixed => 0.0,
        Joint::Head => (pose.pitch + head_tilt(pose)).clamp(-1.4, 1.4),
        // In the air the legs split and the arms come up, which is the
        // one pose that reads as "not standing on anything" from any
        // angle. The split is uneven on purpose: a symmetric one is a
        // star jump.
        Joint::LegRight if pose.airborne => 0.45,
        Joint::LegLeft if pose.airborne => -0.30,
        Joint::ArmRight if pose.airborne => {
            let base = AIRBORNE_ARM + held_lift(pose);
            base + arm_angle(pose, base)
        }
        Joint::ArmLeft if pose.airborne => AIRBORNE_ARM,
        // **The bad leg takes the short step, and the wire says which one
        // it is** (`Pose::limp_left`). This was the right leg always, on the
        // argument that nobody could tell at twenty blocks which side the
        // server meant -- and the person who could tell was the one who had
        // just watched their friend land on the left leg, and saw them limp
        // home on the right.
        Joint::LegRight => stride * if pose.limp_left { 1.0 } else { short },
        Joint::LegLeft => -stride * if pose.limp_left { short } else { 1.0 },
        // **The arm swings opposite the leg on its own side.** That is
        // what a walk is; arms in phase with the legs on the same side
        // is a march, and it is instantly wrong to look at even when
        // nobody can say why.
        Joint::ArmRight => {
            let base = -stride + REST_LEAN * idle + breath + held_lift(pose);
            base + arm_angle(pose, base)
        }
        Joint::ArmLeft => stride + REST_LEAN * idle - breath,
    }
}

/// How far the right arm is carried forward for what is in its hand. See
/// `HOLD_LIFT`.
fn held_lift(pose: &Pose) -> f32 {
    if pose.outfit.holding == BLOCK_AIR {
        0.0
    } else {
        HOLD_LIFT
    }
}

/// What the gesture under way adds to the right arm, in radians forward.
///
/// `base` is where the arm already is -- walking, leaning, carrying. A blow
/// or a reach is *added* to that, but a hand going to the mouth goes to the
/// mouth: the rest pose and the carrying lift are taken back out as it comes
/// up, or a player holding their food would lift it a third of a radian over
/// their head, which is exactly what the carrying lift did the first time.
fn arm_angle(pose: &Pose, base: f32) -> f32 {
    match pose.arm {
        Some(Arm::Chop { phase, raised }) => raised.clamp(0.0, 1.0) * chop_angle(phase),
        None => 0.0,
        Some(Arm::Swing(t)) => swing_angle(t),
        Some(Arm::Thrust(t)) => thrust_angle(t),
        Some(Arm::Place(t)) => PLACE_REACH * (std::f32::consts::PI * t.clamp(0.0, 1.0)).sin(),
        Some(Arm::Eat(t)) => {
            held_up(t) * (EAT_RAISE - base + CHEW * (t * CHEWS * std::f32::consts::TAU).sin())
        }
        Some(Arm::Drink(t)) => held_up(t) * (DRINK_RAISE - base),
        // Absolute, like a mouthful: the rod goes back over the shoulder
        // whatever the walk was doing with the arm.
        Some(Arm::Wind(raised)) => raised.clamp(0.0, 1.0) * (ROD_BACK - base),
        Some(Arm::Cast(t)) => cast_angle(t, base),
    }
}

/// A throw off a full wind-up at `t` (0..1), as an addition to `base`: from
/// [`ROD_BACK`] to [`ROD_OUT`] on a square, so the snap accelerates into its
/// end the way a blow does, and then eased home to nothing added.
fn cast_angle(t: f32, base: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < ROD_CAST_OUT {
        let k = t / ROD_CAST_OUT;
        ROD_BACK + (ROD_OUT - ROD_BACK) * k * k - base
    } else {
        (ROD_OUT - base) * (1.0 - smoothstep((t - ROD_CAST_OUT) / (1.0 - ROD_CAST_OUT)))
    }
}

/// How far the head is tipped back to meet a drink.
fn head_tilt(pose: &Pose) -> f32 {
    match pose.arm {
        Some(Arm::Drink(t)) => held_up(t) * DRINK_TILT,
        _ => 0.0,
    }
}

/// 0 at the start and the end of a gesture held up somewhere, 1 in between:
/// up over the first sixth and down over the last quarter, so the hand arrives
/// with a little snap and goes back without one.
pub(crate) fn held_up(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    smoothstep(t / 0.15).min(smoothstep((1.0 - t) / 0.25))
}

/// A thrust, as an arm with no elbow can make one.
///
/// **The hand in the player's own view moves forward; a one-box arm can only
/// turn at the shoulder.** So the arm is brought up to nearly level and the
/// drive is a turn from below level to level: the fist travels forward and a
/// little up, which is what a spear driven from the hip does. Its three
/// stretches -- draw, drive, recover -- end where `hand::lunge_curve`'s do
/// (`LUNGE_COCK`, `LUNGE_HIT`), so the point is out at the same moment on the
/// thrower's screen and on the target's.
fn thrust_angle(t: f32) -> f32 {
    use crate::logic::hand::{LUNGE_COCK, LUNGE_HIT};
    let t = t.clamp(0.0, 1.0);
    let lunge = if t < LUNGE_COCK {
        -THRUST_DRAW * smoothstep(t / LUNGE_COCK)
    } else if t < LUNGE_HIT {
        -THRUST_DRAW + (1.0 + THRUST_DRAW) * smoothstep((t - LUNGE_COCK) / (LUNGE_HIT - LUNGE_COCK))
    } else {
        1.0 - smoothstep((t - LUNGE_HIT) / (1.0 - LUNGE_HIT))
    };
    held_up(t) * (THRUST_LEVEL + THRUST_DRIVE * lunge)
}

/// One blow of the digging rhythm, 0..1: the arm drawn up over most of it
/// and brought down fast at the end, from `CHOP_LOW` to `CHOP_HIGH` and back.
///
/// Raised over the first two thirds and struck in the last third, on a
/// square so the strike *accelerates* into the block: the same asymmetry
/// `swing_angle` argues for, turned round because here the blow ends the
/// cycle rather than starting it.
fn chop_angle(phase: f32) -> f32 {
    const DRAWN: f32 = 0.66;
    let phase = phase.rem_euclid(1.0);
    let up = if phase < DRAWN {
        smoothstep(phase / DRAWN)
    } else {
        let u = (phase - DRAWN) / (1.0 - DRAWN);
        1.0 - u * u
    };
    CHOP_LOW + (CHOP_HIGH - CHOP_LOW) * up
}

/// The right arm through one blow: wind up, strike, recover.
///
/// Three phases rather than one sine, because a blow is not symmetric.
/// The arm comes up over a third of the time, down through the target
/// fast, and returns slowly -- and it is the asymmetry that makes it read
/// as effort rather than as waving.
///
/// **Up in front, not back behind.** The wind-up was -2 radians: the arm
/// thrown *backwards* until it pointed up behind the player, then brought
/// round underarm through the legs to the front. Photographed, it is a
/// bowler, not somebody hitting anything, and what was in the hand trailed
/// the other way. A blow with an arm that only turns at the shoulder is an
/// overhead one: up in front of the face, down through the target.
fn swing_angle(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    /// Up past the face, and down through the blow to below level.
    const RAISE: f32 = 2.3;
    const STRIKE: f32 = 0.5;
    const WIND_UP: f32 = 0.35;
    const DOWN: f32 = 0.60;
    if t < WIND_UP {
        RAISE * smoothstep(t / WIND_UP)
    } else if t < DOWN {
        RAISE + (STRIKE - RAISE) * smoothstep((t - WIND_UP) / (DOWN - WIND_UP))
    } else {
        STRIKE * (1.0 - smoothstep((t - DOWN) / (1.0 - DOWN)))
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where a point in the model's own units ends up, in the space the
/// caller gave `feet` in.
///
/// The model faces -Z and a yaw of zero looks along +X, so the model's
/// own axes turn a quarter before the yaw is applied: front (-Z) becomes
/// +X and the player's right (+X) becomes +Z.
///
/// The same mapping `animal_model::append_part` uses, and stated there
/// at length for the same reason: it is a rotation and not a reflection,
/// so `(x, z) -> (-z, x)` and never `(z, -x)` -- the second mirrors the
/// figure as well as turning it, which puts the parting in its hair on
/// the wrong side and, worse, reverses the winding of every face it
/// touches.
///
/// **Its own function because two things need it.** Every box in the
/// figure goes through here, and so does whatever the player is carrying
/// (see [`hand_point`]). A second copy of this arithmetic is a tool that
/// hangs a foot away from the hand holding it, and it would only be
/// wrong while the arm was moving.
pub fn place(pose: &Pose, feet: Vec3, joint: Joint, pivot: f32, unit: Vec3) -> Vec3 {
    let (yaw_sin, yaw_cos) = pose.yaw.sin_cos();
    let (swing_sin, swing_cos) = joint_angle(joint, pose).sin_cos();
    // Swung about the joint, in the plane a limb swings in...
    let dy = unit.y - pivot;
    let swung = postured(
        pose.posture,
        Vec3::new(
            unit.x,
            pivot + dy * swing_cos - unit.z * swing_sin,
            dy * swing_sin + unit.z * swing_cos,
        ),
    ) * SCALE;
    // ...turned to face the way they are looking, and put where the
    // server says they are.
    //
    // **The dip is taken off everything but the legs.** A limp drops the hips
    // as the weight goes onto the bad leg, and a drop applied to the legs as
    // well would put the feet through the ground twice a stride -- which is
    // the one thing a watcher would read as a bug rather than as an injury.
    // What the legs do instead is the short step and the uneven timing (see
    // `LIMP_SKEW`); this is the body riding down over them.
    let dip = match joint {
        Joint::LegRight | Joint::LegLeft => 0.0,
        _ => limp_dip(pose) * SCALE,
    };
    let (fx, fz) = (-swung.z, swung.x);
    Vec3::new(
        feet.x + fx * yaw_cos - fz * yaw_sin,
        feet.y + swung.y - dip,
        feet.z + fx * yaw_sin + fz * yaw_cos,
    )
}

/// One box of the figure, swung with its joint and turned with the
/// player.
///
/// `sheet` answers, per face index, which rectangle of the skin sheet
/// this box wears -- [`net`] for a body part, and a *band* of the same
/// net for the garment over it, which is what lets a boot wear the
/// bottom of a leg's picture rather than the whole of it stretched.
#[allow(clippy::too_many_arguments)]
fn push_box(
    pose: &Pose,
    feet: Vec3,
    joint: Joint,
    pivot: f32,
    at: [f32; 3],
    size: [f32; 3],
    sheet: impl Fn(usize) -> [f32; 4],
    tint: [f32; 3],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    push_box_with(|unit| place(pose, feet, joint, pivot, unit), at, size, sheet, tint, vertices, indices);
}

/// `push_box`, with where each corner goes handed in: `place` for a part
/// swinging on its joint, and `astride`'s bend for the halves of a rider's
/// leg -- one copy of the faces, the net and the normals for both.
fn push_box_with(
    placed: impl Fn(Vec3) -> Vec3,
    at: [f32; 3],
    size: [f32; 3],
    sheet: impl Fn(usize) -> [f32; 4],
    tint: [f32; 3],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    for (face_index, face) in faces().iter().enumerate() {
        let [rx, ry, rw, rh] = sheet(face_index);
        let base = vertices.len() as u32;
        let mut corners = [[0.0f32; 3]; 4];
        for (slot, corner) in face.corners.iter().enumerate() {
            corners[slot] = placed(Vec3::new(
                at[0] + (corner[0] - 0.5) * size[0],
                at[1] + (corner[1] - 0.5) * size[1],
                at[2] + (corner[2] - 0.5) * size[2],
            ))
            .to_array();
        }
        // **The normal is taken from the geometry that was just
        // emitted, not from a table.**
        //
        // A table of six normals turned by the same rotations as the
        // corners is the same answer written twice, and the failure
        // it invites is the one this codebase has already paid for:
        // a face whose lighting is welded to the model rather than
        // to the world, so a figure that turns never changes its
        // shading. Cross product of the emitted edges, and the two
        // cannot disagree -- if a box is ever wound inside out, its
        // light goes with it and the test below sees both.
        let normal = face_normal(&corners);
        for (slot, corner) in face.corners.iter().enumerate() {
            let [u, v] = face_uv(face_index, *corner);
            vertices.push(ActorVertex {
                position: corners[slot],
                color: tint,
                normal,
                uv: [
                    (rx + u * rw) / SHEET_WIDTH,
                    (ry + v * rh) / SHEET_HEIGHT,
                ],
                // Replaced with where the figure stands by
                // `remote_players::build_actor_mesh_into`, which knows the
                // light map and this does not.
                light: crate::net::remote_players::OPEN_AIR,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The whole figure, appended to an actor mesh.
///
/// `feet` is the point the server sends -- the bottom centre of the
/// collider -- already in render-origin space.
///
/// **The body first, then everything worn over it.** Not interleaved,
/// so a dressed figure's body boxes are still the first `PARTS.len()`
/// of them in part order -- which is what lets the tests below index a
/// part by arithmetic instead of hunting for it, dressed or not.
pub fn append(
    pose: &Pose,
    feet: Vec3,
    tint: [f32; 3],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let mounted = pose.posture == Posture::Mounted;
    let is_leg = |part: &Part| matches!(part.joint, Joint::LegRight | Joint::LegLeft);
    for part in PARTS {
        if mounted && is_leg(part) {
            astride(pose, feet, part, part.at, part.size, |face| net(part, face), tint, vertices, indices);
            continue;
        }
        push_box(
            pose,
            feet,
            part.joint,
            part.pivot,
            part.at,
            part.size,
            |face| net(part, face),
            tint,
            vertices,
            indices,
        );
    }

    if pose.outfit.is_bare() {
        // The common case, and the one worth spelling out: a player who
        // has nothing on costs exactly what they cost before any of
        // this existed.
        return;
    }

    // The pack first, so a chest piece drawn after it wins any argument
    // about a shared edge -- the straps go under the armour, which is
    // also how they go on.
    if pose.outfit.worn_in(Slot::Back) != BLOCK_AIR {
        let torso = &PARTS[TORSO];
        push_box(
            pose,
            feet,
            torso.joint,
            torso.pivot,
            PACK_AT,
            PACK_SIZE,
            // The torso's own net: the pack wears the skin's chest
            // picture, tinted. It is the same bargain `Overlay` makes at
            // length -- there is no room on a 64x32 sheet for a bag, and
            // a box in leather brown at the distance a player is seen
            // from reads as a pack rather than as a slab.
            |face| net(torso, face),
            [
                PACK_TINT[0] * tint[0],
                PACK_TINT[1] * tint[1],
                PACK_TINT[2] * tint[2],
            ],
            vertices,
            indices,
        );
    }

    for overlay in OVERLAYS {
        let block = pose.outfit.worn_in(overlay.slot);
        if block == BLOCK_AIR {
            continue;
        }
        let Some(part) = PARTS.get(overlay.part) else {
            continue;
        };
        let (at, size) = garment_box(part, overlay.span);
        if mounted && is_leg(part) {
            // Trousers and boots bend with the leg in them.
            let colour = garment_colour(block, tint);
            astride(pose, feet, part, at, size, |face| garment_net(part, face, overlay.span), colour, vertices, indices);
            continue;
        }
        push_box(
            pose,
            feet,
            part.joint,
            part.pivot,
            at,
            size,
            |face| garment_net(part, face, overlay.span),
            garment_colour(block, tint),
            vertices,
            indices,
        );
    }
}

/// A turn that lays a box's own downward axis along `along`: how a half of a
/// rider's leg is pointed (`astride`).
///
/// Built as three axes, each crossed from the last, so it is always a turn
/// and never a mirror -- a mirrored box is wound inside out, and the left
/// leg's direction is the right's mirrored, which is exactly the input that
/// would build one if the axes were mirrored with it.
fn pointing(along: Vec3) -> Mat3 {
    let up = -along.normalize();
    let across = up.cross(Vec3::Z).normalize();
    Mat3::from_cols(across, up, across.cross(up))
}

/// Where the hip and the knee of a rider's leg are, in model units, before
/// the posture's drop: the two points the halves of the leg hang from.
fn astride_joints(leg: &Part) -> (Vec3, Vec3) {
    let side = leg.at[0].signum();
    let hip = Vec3::new(leg.at[0] + side * HIP_SPREAD, leg.pivot, leg.at[2]);
    let thigh = THIGH * Vec3::new(side, 1.0, 1.0);
    let knee_up = leg.at[1] - leg.size[1] * 0.5 + KNEE;
    (hip, hip + thigh.normalize() * (leg.pivot - knee_up))
}

/// Where the sole of a rider's right foot is, in blocks from the point the
/// server puts their feet (`horse::RIDER_LIFT` over the horse's), in the
/// figure's own frame: x to their right, -z ahead, as the horse's model is.
///
/// **What the stirrup under a rider is hung from** (`animal_model::append_tack`),
/// so the iron is under the boot however `THIGH` and `SHIN` are changed: a
/// stirrup placed by a number of its own was a stirrup a hand's breadth
/// behind the heel the first time anybody looked. The left foot is this
/// mirrored.
pub(crate) fn rider_foot() -> Vec3 {
    let leg = &PARTS[LEG_RIGHT];
    let (_, knee) = astride_joints(leg);
    (knee + SHIN.normalize() * KNEE - Vec3::Y * mount_drop()) * SCALE
}

/// A box on a rider's leg -- the leg itself, or a garment over it -- bent at
/// the knee round a horse. See `KNEE` for why a mounted leg is two boxes.
///
/// The box is cut at the knee into the band above it and the band below,
/// each a little past it (`KNEE_OVERLAP`); the band above is turned about
/// the hip to lie along `THIGH` and the band below about the knee to lie
/// along `SHIN`. Each wears the matching band of the picture it would have
/// worn whole, cut the way `garment_net` cuts a boot from a leg -- so the
/// boot on a rider is still on the shin and the knee of the trousers is
/// still at the knee.
#[allow(clippy::too_many_arguments)]
fn astride(
    pose: &Pose,
    feet: Vec3,
    leg: &Part,
    at: [f32; 3],
    size: [f32; 3],
    sheet: impl Fn(usize) -> [f32; 4],
    tint: [f32; 3],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let side = leg.at[0].signum();
    let mirror = Vec3::new(side, 1.0, 1.0);
    let (hip, knee) = astride_joints(leg);
    let knee_up = leg.at[1] - leg.size[1] * 0.5 + KNEE;
    let root = Vec3::new(leg.at[0], leg.pivot, leg.at[2]);
    let bend = Vec3::new(leg.at[0], knee_up, leg.at[2]);
    let (low, high) = (at[1] - size[1] * 0.5, at[1] + size[1] * 0.5);
    let halves = [
        (low.max(knee_up - KNEE_OVERLAP), high, root, hip, pointing(THIGH * mirror)),
        (low, high.min(knee_up + KNEE_OVERLAP), bend, knee, pointing(SHIN * mirror)),
    ];
    for (bottom, top, from, to, turn) in halves {
        if top <= bottom {
            continue;
        }
        // The band of the picture, measured from the top as a net measures
        // it; the crown and the sole are left whole, as `garment_net` leaves
        // them.
        let (t0, t1) = ((high - top) / size[1], (high - bottom) / size[1]);
        push_box_with(
            |unit| place(pose, feet, Joint::Fixed, 0.0, to + turn * (unit - from)),
            [at[0], (top + bottom) * 0.5, at[2]],
            [size[0], top - bottom, size[2]],
            |face| {
                let [x, y, w, h] = sheet(face);
                if face < 2 {
                    [x, y, w, h]
                } else {
                    [x, y + t0 * h, w, (t1 - t0) * h]
                }
            },
            tint,
            vertices,
            indices,
        );
    }
}

/// Where the shell of a garment covering `span` of a part is, in model
/// units: centre and size.
///
/// The band of the box this garment covers, padded outward -- see
/// `PADDING` and `SEAM` for which edges may grow and which may not. Its
/// own function because a standing figure and a body lying on the ground
/// both dress in it, and a shell measured twice is a cuirass that changes
/// size when its wearer falls over.
fn garment_box(part: &Part, span: (f32, f32)) -> ([f32; 3], [f32; 3]) {
    let (top, bottom) = span;
    let box_top = part.at[1] + part.size[1] * 0.5 - top * part.size[1];
    let box_bottom = part.at[1] + part.size[1] * 0.5 - bottom * part.size[1];
    let grown_top = box_top + if top <= 0.0 { PADDING } else { 0.0 };
    let grown_bottom = box_bottom + if bottom >= 1.0 { SEAM } else { 0.0 };
    let size = [part.size[0] + PADDING * 2.0, grown_top - grown_bottom, part.size[2] + PADDING * 2.0];
    let at = [part.at[0], (grown_top + grown_bottom) * 0.5, part.at[2]];
    (at, size)
}

/// The band of a part's own net that a garment covering `span` wears.
///
/// Only the four sides are cut: on those, [`net`]'s fourth number is the
/// box's height, so a band of the box is the same band of the picture.
/// The crown and the underside are left whole -- their fourth number is
/// the box's *depth*, which the span says nothing about, and a boot's
/// two caps are a cuff nobody sees and a sole nobody sees.
fn garment_net(part: &Part, face_index: usize, span: (f32, f32)) -> [f32; 4] {
    let [x, y, w, h] = net(part, face_index);
    if face_index < 2 {
        return [x, y, w, h];
    }
    let (top, bottom) = span;
    [x, y + top * h, w, (bottom - top) * h]
}

/// What colour a garment is drawn in, over whatever tint the figure
/// already carries.
///
/// **Through `hotbar::icon_tint`, which is the one place this decision
/// is made**, rather than a second reading of `types::garment_tint`.
/// The pack screen, the hotbar and the first-person hand all ask that
/// function what a bronze cuirass looks like; a model that asked
/// separately would be a fourth answer waiting to disagree, and the
/// symptom -- armour that is one colour in the inventory and another on
/// the wearer -- is exactly the complaint `logic::hand` was fixed for.
///
/// A garment with no colour of its own comes back white, which draws the
/// shell in the skin's own colours. That is deliberate: a thirteenth
/// garment added without a tint should look like clothing nobody
/// coloured, not like nothing at all.
fn garment_colour(block: BlockId, base: [f32; 3]) -> [f32; 3] {
    let material = crate::ui::hotbar::icon_tint(block, [1.0; 4]);
    [
        base[0] * material[0],
        base[1] * material[1],
        base[2] * material[2],
    ]
}

// ---- what is in the other player's hand ----
//
// The first-person hand (`logic::hand`) answers "what am I holding" for
// the person holding it, in view space, at a size chosen against the
// frame. This answers the same question about somebody else, in the
// world, at a size chosen against a body -- so the two share the fact
// (`Outfit::holding`) and the picture (`engine::item_model`) and nothing
// else. Trying to share the geometry would mean one function whose
// numbers are all conditional on which of two completely different
// framings it is drawing for.
//
// ## How a thing is held, and what was wrong with it
//
// **Everything used to be held the same way**: the *middle* of the picture
// put two units in front of the fist, turned edge-on, half a block across
// whatever it was. Photographed ("игрок держит предметы странно и
// неправильно", `what_a_player_holds_and_how_a_body_lies`) that was an axe
// with its haft sticking out ahead of the hand and its head trailing behind
// the wrist, a spear held by its middle with the point behind the player, a
// steak half a block wide hanging off the knuckles, and a block that did not
// move when the arm did, because it was placed by the body's facing and not
// by the arm's swing.
//
// A hand holds four kinds of thing, and each is placed by the part of it
// that is *in* the hand:
//
// * **a haft** -- a tool, a stick, a torch: gripped near the butt, the shaft
//   leaving the fist forward and a little up, square to the forearm, so the
//   head leads and follows the arm through a blow;
// * **a spear**: gripped a little behind its middle and carried pointing
//   forward and up, and levelled -- not swung -- as it is driven, because a
//   one-box arm that turned a spear with it would thrust at the sky;
// * **something small** -- food, a lump of ore, a handful of fibre: in front
//   of the knuckles, at the size of the thing;
// * **a block or a model**: held out in front of the fist, turning with the
//   arm.
//
// Every one of them is built in the fist's own frame ([`fist_frame`]), the
// arm's rotation written as a matrix -- so nothing held can be anywhere the
// hand is not, mid-blow or at rest.

/// Where in the right arm the fist is, in model units above the bottom of
/// the arm box. The grip is on the arm's own centre line: a haft goes
/// *through* the fist that closes round it.
const FIST: f32 = 1.5;

/// How far the arm holding something is carried forward, in radians, over
/// the arms' rest.
///
/// An arm hanging straight puts whatever it holds against the thigh, and a
/// haft square to it through the leg. A fifth of a right angle clears the
/// leg, and is what somebody carrying a tool does.
const HOLD_LIFT: f32 = 0.3;

/// How a haft leaves the fist: this far up from square to the forearm, in
/// radians. With the arm at rest and `HOLD_LIFT` that is about forty degrees
/// above level, head forward and up.
const HAFT_RISE: f32 = 0.35;
/// Where along the drawing's diagonal from the butt the fist closes, as a
/// fraction of it. Near the end: a tool is swung from the end of its haft.
const HAFT_GRIP: f32 = 0.18;
/// How long a haft is drawn, butt to head, in blocks: a forearm and a hand
/// again. A flint axe or a stick of kindling, not a pole.
const HAFT_LENGTH: f32 = 0.72;

/// **How a rod is held**: steeply up, far longer than a tool, and by the
/// very end of its butt.
///
/// "удочка повёрнута не так как надо": the rod was a `Haft` -- an axe's
/// forty degrees, three quarters of a block long, gripped a fifth of the
/// way up -- so a fisher stood holding what looked like a switch pointed at
/// the water a step in front of them. A rod is held from its butt with its
/// length up and out over the water, and its line hangs from the tip; at
/// this rise the tip is above the fisher's head and a body length out,
/// which is where a watcher looks for the line. The wind-up and the throw
/// turn the arm (`Arm::Wind`, `Arm::Cast`), and the rod rides the fist.
///
/// Rejected: lengthening `HAFT_LENGTH` for everything. Every axe and pick
/// would have grown with it, and a tool is the right length now.
const ROD_RISE: f32 = 0.5;
const ROD_GRIP: f32 = 0.06;
const ROD_LENGTH: f32 = 1.6;

/// A spear's length in blocks, and where along it from the butt the fist is.
///
/// **Towards the butt, not the middle.** At 0.42 the first photograph of a
/// thrust put the butt out behind the thrower's head: the fist is out at
/// arm's length when the point is, and what is behind the fist has to fit
/// between it and the shoulder.
const SPEAR_LENGTH: f32 = 1.7;
const SPEAR_GRIP: f32 = 0.3;
/// How a spear is carried, in radians above level, and how it is held while
/// it is driven: nearly level, the point at the height of what it is for.
const SPEAR_CARRY: f32 = 1.0;
const SPEAR_LEVEL: f32 = 0.08;

/// How wide something held in the palm is drawn, across its silhouette, in
/// blocks: a hand's width. It was half a block, and a steak that size is a
/// shield.
const PALM_ACROSS: f32 = 0.24;
/// Where in the fist's frame it sits, in model units: at the fingers' end of
/// the fist and just in front of the knuckles, so it is not inside the arm.
const PALM_AT: [f32; 3] = [0.0, -1.0, -2.0];

/// How big a *block* is in the hand, in blocks, and where its middle is in
/// the fist's frame, in model units: held out in front, the fist's front
/// buried in its back face.
///
/// Smaller than a sprite, because a cube has bulk where a plate has none --
/// the same trade `entities` makes in the other direction for a dropped
/// sprite.
const HELD_BLOCK: f32 = 0.35;
const BLOCK_AT: [f32; 3] = [0.0, -1.0, -3.8];

/// How a thing in the hand is gripped. See the note at the head of this
/// section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    Haft,
    /// A fishing rod: a haft held by its end and far longer (`ROD_RISE`).
    Rod,
    Spear,
    Palm,
    Block,
}

impl Grip {
    /// **The table decides, not the picture**, for the reason
    /// `hand::held_scale` gives: `durability` is `Some` for exactly the
    /// things that are swung. A torch spends it as seconds of fibre and is a
    /// haft either way; a stick has none and is one too. A garment is worn
    /// out rather than swung, and goes in the palm.
    pub fn of(block: BlockId, has_sprite: bool) -> Grip {
        use primitive_shared::types as t;
        if !has_sprite || crate::engine::mesh::has_carried_model(block) {
            Grip::Block
        } else if t::is_weapon(block) {
            Grip::Spear
        } else if t::block_kind(block) == t::BLOCK_FISHING_ROD {
            Grip::Rod
        } else if t::is_torch(block)
            || t::block_kind(block) == t::BLOCK_STICK
            || (primitive_shared::blocks::definition(block).durability.is_some()
                && !primitive_shared::equipment::is_wearable(block))
        {
            Grip::Haft
        } else {
            Grip::Palm
        }
    }
}

/// The fist, in model units of the figure's own frame.
fn fist_local() -> Vec3 {
    let arm = &PARTS[ARM_RIGHT];
    Vec3::new(arm.at[0], arm.at[1] - arm.size[1] * 0.5 + FIST, arm.at[2])
}

/// Where the right hand is, in whatever space `feet` was given in.
pub fn hand_point(pose: &Pose, feet: Vec3) -> Vec3 {
    let arm = &PARTS[ARM_RIGHT];
    place(pose, feet, arm.joint, arm.pivot, fist_local())
}

/// The fist's frame: origin at `at` (the hand point), axes the arm's own --
/// +Y back up the arm, -Z the way the knuckles face when it hangs, +X the
/// figure's right -- in blocks.
///
/// The rotation is the one [`place`] applies to a corner, written as a
/// matrix: a sprite needs an orientation and not only a point. Kept beside
/// `place` so the two are read together, and a test holds them to each
/// other, because two spellings of one rotation is the arrangement that
/// drifts.
fn fist_frame(pose: &Pose, at: Vec3) -> Mat4 {
    body_frame(pose, at) * Mat4::from_rotation_x(joint_angle(Joint::ArmRight, pose))
}

/// The same, without the arm's swing: the body's own axes at `at`.
fn body_frame(pose: &Pose, at: Vec3) -> Mat4 {
    // The player's facing, and the quarter turn that maps the model's front
    // (-Z) onto +X.
    Mat4::from_translation(at) * Mat4::from_rotation_y(-pose.yaw - std::f32::consts::FRAC_PI_2)
}

/// The rotation carrying a sprite's own plane onto one where `along` is the
/// way the drawing's `drawn` direction points and `normal` is, as nearly as
/// it can be, the way its face looks.
///
/// Both bases right-handed and orthonormal, so this is a rotation and never
/// a reflection: a mirrored plate is wound inside out and culled from the
/// side it should be seen from -- the argument `hand::spear_transform` makes.
fn laid_along(drawn: Vec3, along: Vec3, normal: Vec3) -> Mat4 {
    let drawn_axis = drawn.normalize_or_zero();
    let drawn_across = Vec3::new(-drawn_axis.y, drawn_axis.x, 0.0);
    let normal = (normal - along * along.dot(normal)).normalize_or_zero();
    let across = normal.cross(along);
    Mat4::from_mat3(
        glam::Mat3::from_cols(along, across, normal)
            * glam::Mat3::from_cols(drawn_axis, drawn_across, Vec3::Z).transpose(),
    )
}

/// Where a sprite held with `grip` goes: the transform from the sprite's own
/// space to the world less `origin`, for a figure in `pose` at `feet`.
///
/// Its own function so the tests can ask where a haft's butt and head end
/// up without a GPU: the picture is an `ItemModel` cut from the shipped PNG,
/// and the grip is arithmetic on its drawn box.
pub fn held_sprite_transform(
    pose: &Pose,
    feet: Vec3,
    origin: Vec3,
    grip: Grip,
    model: &crate::engine::item_model::ItemModel,
) -> Mat4 {
    let hand = hand_point(pose, feet) - origin;
    let (low, high) = model.drawn_box();
    let (low, high) = (Vec3::new(low[0], low[1], 0.0), Vec3::new(high[0], high[1], 0.0));
    // The drawing's own long axis, butt (bottom left) to head (top right):
    // how every tool and spear in the pack is drawn, and what
    // `a_spear_is_drawn_corner_to_corner_with_its_head_at_the_top` pins.
    let drawn = high - low;
    let length = drawn.length().max(1.0 / 16.0);
    match grip {
        Grip::Haft | Grip::Rod | Grip::Spear => {
            let (frame, rise, reach, grip_at) = if grip == Grip::Haft {
                (fist_frame(pose, hand), HAFT_RISE, HAFT_LENGTH, HAFT_GRIP)
            } else if grip == Grip::Rod {
                (fist_frame(pose, hand), ROD_RISE, ROD_LENGTH, ROD_GRIP)
            } else {
                // Levelled as the thrust comes up to its point, and carried
                // high again as the arm comes home.
                let driving = match pose.arm {
                    Some(Arm::Thrust(t)) => held_up(t),
                    _ => 0.0,
                };
                let rise = SPEAR_CARRY + (SPEAR_LEVEL - SPEAR_CARRY) * driving;
                (body_frame(pose, hand), rise, SPEAR_LENGTH, SPEAR_GRIP)
            };
            let along = Vec3::new(0.0, rise.sin(), -rise.cos());
            // The picture's face to the figure's right, so the side a
            // right-handed tool is watched from shows it the way it was
            // drawn.
            frame
                * laid_along(drawn, along, Vec3::X)
                * Mat4::from_scale(Vec3::splat(reach / length))
                * Mat4::from_translation(-(low + drawn * grip_at))
        }
        Grip::Palm | Grip::Block => {
            let [width, height] = model.silhouette();
            let middle = (low + high) * 0.5;
            // Upright in the hand and turned a little towards the front, so
            // it is seen from ahead as well as from the side: flat to the
            // side it is a line to anybody the player walks towards.
            let normal = Vec3::new(0.8, 0.0, -0.6);
            fist_frame(pose, hand)
                * Mat4::from_translation(Vec3::from(PALM_AT) * SCALE)
                * laid_along(Vec3::X, Vec3::Y.cross(normal).normalize(), normal)
                * Mat4::from_scale(Vec3::splat(PALM_ACROSS / width.max(height).max(1.0 / 16.0)))
                * Mat4::from_translation(-middle)
        }
    }
}

/// A unit cube of a block's own six pictures, 0 to 1 on every axis -- the
/// shape `mesh::carried_model` hands over for a model, so a plain block is
/// placed by the same arithmetic as a barrel.
fn cube_model(block: BlockId, layers: &FaceLayers) -> (Vec<Vertex>, Vec<u32>) {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for (face_index, face) in faces().iter().enumerate() {
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            vertices.push(Vertex::new(
                *corner,
                face_uv(face_index, *corner),
                layers.layer_for_face(block, face_index),
                crate::engine::mesh::pack_light(15, 0, 3, face_index as u8),
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// Whatever this player is carrying, in their right hand.
///
/// Two output pairs for the same reason `entities::build_meshes_into` has
/// two: a tool is a sprite with a thickness and goes down the item pipeline,
/// a block or a model is geometry in the terrain's format and goes down that
/// one. Which is which is asked of `models` -- a block with no picture of its
/// own is a cube, and that is the whole rule.
///
/// `feet` is in world coordinates and `origin` is the frame's render origin:
/// the light has to be sampled where the hand actually is, and the geometry
/// has to be measured from the origin. The same split `entities` makes, and
/// for the same reason.
#[allow(clippy::too_many_arguments)]
pub fn append_held(
    pose: &Pose,
    feet: glam::DVec3,
    origin: Vec3,
    layers: &FaceLayers,
    light: &primitive_shared::lighting::LightMap,
    models: Option<&crate::engine::texture::TextureManager>,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    item_vertices: &mut Vec<ItemVertex>,
    item_indices: &mut Vec<u32>,
) {
    let block = pose.outfit.holding;
    // Nothing in the hand of a sleeper or of the dead: the carried thing is
    // placed by the arm and not by the body's quarter turn, so on a lying
    // figure it stood upright in the air above the pillow.
    if block == BLOCK_AIR || matches!(pose.posture, Posture::Lying | Posture::Fallen) {
        return;
    }
    // Measured from the frame's origin in `f64` and only then narrowed, so
    // the hand is where the figure is however far out; the light is read
    // back in the world, where the cells are.
    let feet = (feet - origin.as_dvec3()).as_vec3();
    let hand = hand_point(pose, feet);
    let lit = crate::logic::entities::sampled_light(origin.as_dvec3() + hand.as_dvec3(), light);
    let origin = Vec3::ZERO;
    let sprite = models.and_then(|m| m.item_model(block));
    match sprite.map(|model| (Grip::of(block, true), model)) {
        Some((grip, model)) if grip != Grip::Block => {
            let transform = held_sprite_transform(pose, feet, origin, grip, model);
            // **A torch somebody else is holding burns.** Only the player's
            // own hand drew a flame (`hand::build_into`), so on every other
            // screen a lit torch was a stick with a wad on it, in the dark,
            // in a hand that was lighting nobody's way. The same fire on the
            // same rectangle of the sprite the hand lays it on, and on both
            // faces of the plate a hair proud of each: the hand's is pulled
            // toward the one eye it is ever seen from, and a figure is seen
            // from every side. Full light, for the hand's reason.
            if let (Some(textures), true) = (models, primitive_shared::types::is_lit_torch(block)) {
                use crate::engine::texture::{FLAME_FPS, FLAME_FRAMES};
                let flame = textures.torch_flame_layer() + (pose.age * FLAME_FPS) as u32 % FLAME_FRAMES;
                push_held_flame(transform, flame, item_vertices, item_indices);
            }
            model.append_transformed(
                item_vertices,
                item_indices,
                transform,
                // The *carried* picture where a block has one, and a face
                // only as a fallback -- because that is the picture the
                // silhouette was cut from. The same choice `entities` makes
                // for a dropped stack, and getting it the other way round
                // puts a paving slab of ash on a model shaped like a handful
                // of it.
                layers
                    .layer_for_item(block)
                    .unwrap_or_else(|| layers.layer_for_face(block, 0)),
                lit.0,
                lit.1,
            );
        }
        _ => {
            // **A block with a model of its own is carried as that model**,
            // the way the player holding it sees it in their own hand and the
            // way it stands when they put it down -- see
            // `mesh::carried_model`. Anything else with no picture is a cube,
            // and both turn with the arm now: the cube used to be placed by
            // the body's facing alone, and stayed put while the arm reached.
            let mut model = (Vec::new(), Vec::new());
            if !crate::engine::mesh::carried_model(block, layers, &mut model.0, &mut model.1) {
                model = cube_model(block, layers);
            }
            let (low, high) = crate::engine::mesh::extent(&model.0);
            // A cube is `HELD_BLOCK` across; a model is that across its
            // longest side, so a bed is not two blocks of bed in one hand.
            let scale = HELD_BLOCK / (high - low).max_element().max(1.0 / 16.0);
            let transform = fist_frame(pose, hand - origin)
                * Mat4::from_translation(Vec3::from(BLOCK_AT) * SCALE)
                * Mat4::from_scale(Vec3::splat(scale))
                * Mat4::from_translation(-(low + high) * 0.5);
            crate::engine::mesh::place_carried(&model.0, &model.1, transform, lit, vertices, indices);
        }
    }
}

// ---- a body on the ground ----
//
// **What a death leaves is the figure above, lying down**, wearing the
// player's own skin and whatever of their clothes is still in it.
//
// The cell is a block (`types::BLOCK_CORPSE`), not an entity. It was baked
// into the chunk mesh the way a carcass is (`animal_model::build_fallen`),
// and that was the right *place* for a thing that never moves and the wrong
// *pipeline*: a terrain vertex names a layer of the block atlas and cannot
// reach the 64x32 skin sheet, so the body was dressed in five flat patches
// cut out of the block's old picture -- a tan box for a face, a brown slab
// for a tunic, and nothing a player was wearing. "У трупа нет текстуры
// игрока." Three ways were weighed again:
//
// * **Put the skin sheet in the atlas.** Eight sixteen-texel layers, a net
//   that does not respect tile edges (the torso's underside runs from texel
//   11 to 18 and is cut in half by the boundary), and the garments are
//   tints the terrain vertex has no room for either.
// * **Keep the patches, painted better.** Still no face, still no clothes:
//   the thing a player walks back to would never be recognisably *theirs*.
// * **Draw it on the actor pipeline (chosen)**, the one every other player
//   is drawn with: the same sheet, the same `net`, the same garment shells
//   in the same `hotbar::icon_tint` colours, from the same `PARTS` -- so the
//   body on the ground is the person who was walking about, and cannot
//   drift from them. The price is that it is rebuilt with the figures
//   rather than baked once: a few hundred vertices per body in the loaded
//   world, on the clock the other figures already run on, and nothing at
//   all where nobody has died.
//
// The bones it rots into stay in the chunk mesh (`build_fallen`): a skeleton
// is ivory, which *is* an atlas tile, and wants none of this.
//
// The lying-down itself is the carcass's arithmetic, not a second copy of
// it: `animal_model::resting_pose` measures the posed boxes and puts their
// underside on the floor, and `posed_quads` places the corners.

/// One model unit in sixteenths of a block, which is the unit the
/// lying-down machinery in `animal_model` measures in.
///
/// The figure is written in thirty-seconds of its own height (see the
/// head of this file) and an animal in sixteenths of a block; the two
/// meet here and nowhere else.
const SIXTEENTHS: f32 = SCALE * 16.0;

/// **How a fallen body lies**: how far each part is swung about its own
/// joint, in radians, before the whole figure is rolled onto its side.
///
/// A swing moves the free end of a limb along the figure's own forward
/// axis, and that axis stays horizontal through the roll -- so these lay
/// the arms and the legs *out on the ground*, in front of the body and
/// behind it, rather than lifting anything.
///
/// **Because a plank is not a corpse.** The animals lie with every part
/// square to the body, and that is right for a boar: a quadruped's legs
/// come off its trunk sideways and lie where they were. A person's do
/// not. A figure rolled onto its side with both arms flat against its
/// ribs and both legs together is a shop mannequin on a floor -- and that
/// is what the first drawing of this was. One arm thrown out in front,
/// the other fallen behind the back, the top leg drawn up and the head
/// tipped toward the chest is what a body on its side looks like, and it
/// is four numbers.
///
/// In `PARTS` order. `resting_pose` is measured with these, so the figure
/// still lands on the ground rather than through it -- see its note.
const LIMP: [f32; 6] = [0.26, 0.0, 0.45, -0.28, 0.26, -0.06];

/// Which stage of a dead player a cell holds.
///
/// The mesher's way in, and shaped like `animals::Species::of_carcass` on
/// purpose: one question of the block id, one call, and the two families
/// of thing-lying-in-the-grass are reached the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dead {
    /// A body, with everything they were carrying still on it.
    Body,
    /// ...and what two days of nobody coming back leaves.
    Bones,
}

impl Dead {
    /// Is this cell a dead player, and at which stage?
    pub fn of(block: BlockId) -> Option<Dead> {
        use primitive_shared::types as t;
        match t::block_kind(block) {
            t::BLOCK_CORPSE => Some(Dead::Body),
            t::BLOCK_REMAINS => Some(Dead::Bones),
            _ => None,
        }
    }
}

/// One box of a fallen figure: where it is and how far it is swung.
struct Limb {
    part: crate::logic::animal_model::Part,
    swing: f32,
}

/// ...and, for a body, what it wears: where on the skin sheet each of its
/// six faces reads, and the tint over it.
struct Dressed {
    limb: Limb,
    sheet: [[f32; 4]; 6],
    tint: [f32; 3],
}

/// A box of the figure, written in the model's own units and handed over
/// in the sixteenths `animal_model` measures in.
fn limb(name: &'static str, at: [f32; 3], size: [f32; 3], pivot: f32, swing: f32) -> Limb {
    Limb {
        part: crate::logic::animal_model::Part {
            name,
            at: [at[0] * SIXTEENTHS, at[1] * SIXTEENTHS, at[2] * SIXTEENTHS],
            size: [size[0] * SIXTEENTHS, size[1] * SIXTEENTHS, size[2] * SIXTEENTHS],
            // **The joint, not the box's own top.** A limb swings about
            // the shoulder or the hip, and `PARTS` already says where
            // that is; a bone hung below another (a forearm, a shin)
            // names the same joint as the bone above it, so the two turn
            // as one piece rather than folding at the knee.
            //
            // Always on the figure's own centre plane in z, whatever the
            // box's own z is. Every joint in a human model is: the neck,
            // the shoulders and the hips are all at nought, and a jaw
            // that swung about its *own* depth would leave the skull it
            // hangs in the moment the head tipped.
            pivot: Some([pivot * SIXTEENTHS, 0.0]),
            ..crate::logic::animal_model::PART
        },
        swing,
    }
}

/// The body, as boxes: the living figure's own parts wearing its own skin,
/// and a shell over them for every slot something is worn in.
///
/// **`PARTS` and `OVERLAYS`, and not a second table**, so the thing on the
/// ground is exactly the person who was walking about a moment ago, and a
/// garment lies on it where it was worn (`garment_box`, which `append` uses
/// too). Each shell swings with the part it covers.
fn dressed(worn: &[BlockId; primitive_shared::equipment::SLOTS]) -> Vec<Dressed> {
    let white = [1.0; 3];
    let mut boxes: Vec<Dressed> = PARTS
        .iter()
        .enumerate()
        .map(|(i, part)| Dressed {
            limb: limb(part.name, part.at, part.size, part.pivot, LIMP[i]),
            sheet: std::array::from_fn(|face| net(part, face)),
            tint: white,
        })
        .collect();
    // A rucksack is still on the body it was worn on -- a death takes
    // the pack and leaves the armour (see `leave_corpse`), and it leaves
    // this too. Drawn from the same two constants the standing figure
    // uses, so a player who dies does not change shape.
    if worn[Slot::Back.index()] != BLOCK_AIR {
        let torso = &PARTS[TORSO];
        boxes.push(Dressed {
            limb: limb("rucksack", PACK_AT, PACK_SIZE, torso.pivot, LIMP[TORSO]),
            sheet: std::array::from_fn(|face| net(torso, face)),
            tint: PACK_TINT,
        });
    }

    for overlay in OVERLAYS {
        let block = worn[overlay.slot.index()];
        if block == BLOCK_AIR {
            continue;
        }
        let part = &PARTS[overlay.part];
        let (at, size) = garment_box(part, overlay.span);
        boxes.push(Dressed {
            limb: limb("garment", at, size, part.pivot, LIMP[overlay.part]),
            sheet: std::array::from_fn(|face| garment_net(part, face, overlay.span)),
            tint: garment_colour(block, white),
        });
    }
    boxes
}

/// What is left of the body two days later: a skeleton, in the pose the
/// body was in.
///
/// ## Why bones at all, and not the picture that was there
///
/// `BLOCK_REMAINS` is not a different object -- it is the same body,
/// later, in the same cell, with the same things in it. Drawing the body
/// as a figure and the bones as a flat tile would put the two halves of
/// one event in two registers: you would walk up to a person lying in the
/// grass on Monday and to a sticker on the ground on Wednesday, and the
/// change a player is meant to read ("I am too late") would arrive as a
/// change of *medium*. The animals settled this the same way and for the
/// same reason (`animal_model::build_bones`).
///
/// ## Why it is built standing and rolled, where an animal's is not
///
/// A quadruped's skeleton is laid out already lying, because rolling one
/// built standing stacks its four legs one on another and turns its
/// ribcage edge-on to the sky. **A biped has no such problem**: a person
/// on their side genuinely does have one leg resting on the other and
/// their ribcage genuinely is seen from the flank. So the bones are
/// written upright, in the living figure's own frame and measurements,
/// and rolled by the same quarter turn the body is -- which is also what
/// keeps the two in the same pose, limb for limb, through `LIMP`.
///
/// ## What is here and what is not
///
/// A skull and a jaw, a column of vertebrae, four ribs on a breastbone, a
/// pelvis, two bones to each arm and leg, and feet. The ribs are bands
/// across the chest rather than the hoops an animal's skeleton is built
/// out of, and that is a decision rather than a shortcut: a hoop is worth
/// its boxes when you can see *through* the cage, and the only view of a
/// body lying on its side that could is straight down the axis of the
/// spine. From every view anybody will have, a band and a hoop are the
/// same four bars with daylight between them.
///
/// Nothing touches nothing: a bone that runs into another is thinner than
/// it, so no two faces are ever in one plane -- the rule
/// `animal_model::CLEARANCE` states at length, applied here by hand
/// because there are twenty-three boxes and not fifty.
fn bones() -> Vec<Limb> {
    let mut bones = Vec::new();
    let mut bone = |name, at: [f32; 3], size: [f32; 3], pivot: f32, swing: f32| {
        bones.push(limb(name, at, size, pivot, swing));
    };
    let (head, torso) = (LIMP[HEAD], LIMP[TORSO]);
    bone("skull", [0.0, 27.5, -0.6], [5.0, 5.5, 6.0], 24.0, head);
    // Set back inside the skull rather than flush with its chin: the two
    // overlap, and two faces of overlapping boxes in one plane is the
    // fight `CLEARANCE` is about.
    bone("jaw", [0.0, 24.2, -1.1], [3.4, 1.4, 4.2], 24.0, head);
    // The spine, at the back of where the chest was, one knuckle per rib
    // and two more over the loins. Separate boxes rather than one bar:
    // a column of knuckles is what a spine looks like, and a single bar
    // wears its short piece of ivory repeated along its length.
    for i in 0..5 {
        bone("vertebra", [0.0, 13.5 + i as f32 * 2.5, 1.1], [1.9, 1.9, 1.9], 24.0, torso);
    }
    // Four ribs into the spine behind and the breastbone in front, each
    // thinner than both so no face of it lands in either one's plane.
    for i in 0..4 {
        bone("rib", [0.0, 15.2 + i as f32 * 2.2, -0.2], [5.6, 1.1, 3.4], 24.0, torso);
    }
    bone("breastbone", [0.0, 18.45, -2.0], [1.7, 8.9, 0.9], 24.0, torso);
    bone("pelvis", [0.0, 12.6, 0.0], [5.2, 2.6, 3.4], 24.0, torso);
    for (side, arm) in [(1.0, ARM_RIGHT), (-1.0, ARM_LEFT)] {
        let x = PARTS[arm].at[0].abs() * side;
        bone("upper arm", [x, 21.0, 0.0], [1.5, 5.6, 1.5], 24.0, LIMP[arm]);
        bone("forearm", [x, 14.8, 0.0], [1.2, 5.6, 1.2], 24.0, LIMP[arm]);
    }
    for (side, leg) in [(1.0, LEG_RIGHT), (-1.0, LEG_LEFT)] {
        let x = PARTS[leg].at[0].abs() * side;
        bone("thigh", [x, 9.0, 0.0], [1.9, 5.8, 1.9], 12.0, LIMP[leg]);
        // A tenth off the ground rather than on it: the foot's underside
        // is the lowest face there is, and a shin's underside in the same
        // plane as it is two surfaces the depth buffer decides between by
        // rounding.
        bone("shin", [x, 3.0, 0.0], [1.5, 5.6, 1.5], 12.0, LIMP[leg]);
        bone("foot", [x, 0.4, -1.4], [1.0, 0.8, 3.2], 12.0, LIMP[leg]);
    }
    bones
}

/// The pose that lays these boxes on the ground: rolled a quarter turn onto
/// their side, the way a carcass is (`animal_model::fallen_pose`), and
/// shifted so the lowest point of the posed figure rests on the ground.
///
/// **On its side and not face up.** A body drawn flat on its back is a
/// figure whose whole silhouette is its own footprint: from the one angle a
/// player meets it at -- standing over it -- it is as thin as a tile. On its
/// side it has a profile, which is the entire reason for drawing a model at
/// all, and it lies the way every other dead thing in the world lies.
fn lying_pose(limbs: &[&Limb]) -> crate::logic::animal_model::Pose {
    let parts: Vec<_> = limbs.iter().map(|limb| limb.part).collect();
    crate::logic::animal_model::resting_pose(&parts, std::f32::consts::FRAC_PI_2, |i| limbs[i].swing)
}

/// The boxes of a dead player at a stage, bare, with the pose that lays
/// them down. A body's clothes are left off: what the cracks are drawn on
/// and what the tests measure is the figure, and a shell a third of a unit
/// outside it changes neither.
fn fallen(stage: Dead) -> (Vec<Limb>, crate::logic::animal_model::Pose) {
    let limbs = match stage {
        Dead::Body => dressed(&[BLOCK_AIR; primitive_shared::equipment::SLOTS])
            .into_iter()
            .map(|dressed| dressed.limb)
            .collect(),
        Dead::Bones => bones(),
    };
    let pose = lying_pose(&limbs.iter().collect::<Vec<_>>());
    (limbs, pose)
}

/// A dead player's *bones*, lying where they fell, appended to a chunk mesh.
///
/// `ground` is the floor of the cell, the middle of it in x and z; `yaw` is
/// the mesher's hash of the cell (`animal_model::carcass_yaw`), so two
/// deaths in one clearing do not lie parallel and the same body faces the
/// same way after every remesh.
///
/// **A body is not drawn here**, and emits nothing: it wears the skin sheet,
/// which the terrain cannot reach, and is drawn with the other figures
/// (`append_lying`, and the note at the head of this section).
#[allow(clippy::too_many_arguments)]
pub fn build_fallen(
    stage: Dead,
    ground: Vec3,
    yaw: f32,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::logic::animal_model::{ivory, material_cut, push_posed_box};
    if stage == Dead::Body {
        return;
    }
    let (limbs, pose) = fallen(stage);
    let bone = ivory(layers);
    for limb in &limbs {
        push_posed_box(
            &limb.part,
            ground,
            yaw,
            limb.swing,
            pose,
            |face| (bone, Some(material_cut(limb.part.size, face))),
            light,
            vertices,
            indices,
        );
    }
}

/// A dead player's body lying on the ground, in their own skin and in
/// `worn`, appended to an actor mesh.
///
/// `ground` is the point under the middle of the body, already measured
/// from the render origin; `yaw` which way the figure's front faces, in the
/// convention of `place` -- for a body in a cell the mesher's
/// `carcass_yaw`, so the cracks from `fallen_quads` land on it. The light is
/// left open air for the caller to write, as `append` leaves it.
pub fn append_lying(
    ground: Vec3,
    yaw: f32,
    worn: &[BlockId; primitive_shared::equipment::SLOTS],
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    let boxes = dressed(worn);
    let limbs: Vec<&Limb> = boxes.iter().map(|dressed| &dressed.limb).collect();
    let pose = lying_pose(&limbs);
    let parts: Vec<_> = limbs.iter().map(|limb| limb.part).collect();
    let mut quads = Vec::with_capacity(parts.len() * 6);
    crate::logic::animal_model::posed_quads(&parts, pose, ground, yaw, |i| limbs[i].swing, &mut quads);
    // `posed_quads` emits six faces a box, in `faces()` order with the
    // corners in each face's own order -- the order `face_uv` reads.
    for (index, quad) in quads.iter().enumerate() {
        let (dressed, face_index) = (&boxes[index / 6], index % 6);
        let [rx, ry, rw, rh] = dressed.sheet[face_index];
        let normal = face_normal(quad);
        let base = vertices.len() as u32;
        for (slot, corner) in faces()[face_index].corners.iter().enumerate() {
            let [u, v] = face_uv(face_index, *corner);
            vertices.push(ActorVertex {
                position: quad[slot],
                color: dressed.tint,
                normal,
                uv: [(rx + u * rw) / SHEET_WIDTH, (ry + v * rh) / SHEET_HEIGHT],
                light: crate::net::remote_players::OPEN_AIR,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The quads a fallen player is drawn with, and nothing else: no
/// pictures, no light. What the mining cracks go on -- see
/// `animal_model::posed_quads`, whose arithmetic this is.
pub fn fallen_quads(stage: Dead, ground: Vec3, yaw: f32, out: &mut Vec<[[f32; 3]; 4]>) {
    let (limbs, pose) = fallen(stage);
    let parts: Vec<_> = limbs.iter().map(|limb| limb.part).collect();
    crate::logic::animal_model::posed_quads(&parts, pose, ground, yaw, |i| limbs[i].swing, out);
}

/// The outward normal of a quad, from the order its corners are in.
///
/// Counter-clockwise seen from outside is what `FrontFace::Ccw` means
/// and what `mesh::faces` emits; the cross product of the first two
/// edges is then the outward normal by construction.
fn face_normal(corners: &[[f32; 3]; 4]) -> [f32; 3] {
    let a = Vec3::from(corners[0]);
    let edge1 = Vec3::from(corners[1]) - a;
    let edge2 = Vec3::from(corners[2]) - a;
    edge1.cross(edge2).normalize_or_zero().to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(pose: &Pose) -> Vec<ActorVertex> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        append(pose, Vec3::ZERO, [1.0; 3], &mut vertices, &mut indices);
        vertices
    }

    /// The lowest and highest point of a drawn figure along one axis.
    fn span(vertices: &[ActorVertex], axis: usize) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.position[axis]), hi.max(v.position[axis]))
        })
    }


    /// Somebody walking at a steady pace, `walked` blocks into it.
    fn mid_walk(walked: f32, limp: f32) -> Pose {
        Pose { walked, speed: WALKING, limp, ..Pose::default() }
    }

    #[test]
    fn a_limping_figure_drops_onto_its_bad_leg_and_a_sound_one_walks_level() {
        // **The bug this exists for**: nobody could see anybody hurt. A
        // broken leg has slowed a player down since fractures existed and the
        // pace was the whole of it -- on another screen the figure walked
        // home evenly, just more slowly, which reads as somebody taking their
        // time. See `protocol::PlayerState::limp` and `LIMP_DIP`.
        //
        // Measured on the head, because the head is what the dip moves: the
        // legs are left where they are on purpose, or the feet would go
        // through the ground twice a stride.
        let head_height = |pose: &Pose| {
            let drawn = drawn(pose);
            span(&drawn[..24], 1).1
        };
        let sound: Vec<f32> = (0..24).map(|i| head_height(&mid_walk(i as f32 * 0.2, 0.0))).collect();
        let hurt: Vec<f32> = (0..24).map(|i| head_height(&mid_walk(i as f32 * 0.2, 1.0))).collect();
        let level = sound.iter().cloned().fold(f32::MIN, f32::max)
            - sound.iter().cloned().fold(f32::MAX, f32::min);
        let dipping = hurt.iter().cloned().fold(f32::MIN, f32::max)
            - hurt.iter().cloned().fold(f32::MAX, f32::min);
        assert!(level < 1e-4, "a sound walk bobbed by {level:.3}, which is not what this model does");
        assert!(dipping > 0.05, "a limping walk dipped by {dipping:.3}, which nobody would see");
        // ...and only while they are walking. A figure standing still that
        // sank into the ground would read as the model slipping, not as an
        // injury.
        let standing = Pose { limp: 1.0, ..Pose::default() };
        assert_eq!(
            drawn(&standing).iter().map(|v| v.position[1]).fold(f32::MIN, f32::max),
            drawn(&Pose::default()).iter().map(|v| v.position[1]).fold(f32::MIN, f32::max),
            "somebody standing still with a broken leg sank into the ground"
        );
    }

    #[test]
    fn a_limp_makes_the_two_halves_of_a_stride_take_different_lengths_of_ground() {
        // The tell, and it is timing before it is anything else: the sound
        // leg holds the weight for longer and the bad one is got over and put
        // down quickly. See `LIMP_SKEW`. Measured as where in the *ground
        // covered* the right leg reaches each end of its swing -- evenly
        // spaced for a sound walk, and not for a limp.
        let reach = |limp: f32| {
            let mut forward = 0.0f32;
            let mut back = 0.0f32;
            let (mut at_forward, mut at_back) = (0.0f32, 0.0f32);
            // One whole stride of ground: see `PACES_PER_BLOCK`.
            let cycle = 1.0 / PACES_PER_BLOCK;
            for step in 0..400 {
                let walked = step as f32 * cycle / 400.0;
                let angle = joint_angle(Joint::LegRight, &mid_walk(walked, limp));
                if angle > forward {
                    forward = angle;
                    at_forward = walked;
                }
                if angle < back {
                    back = angle;
                    at_back = walked;
                }
            }
            (at_forward, at_back, cycle)
        };
        let (sound_forward, sound_back, cycle) = reach(0.0);
        let (hurt_forward, hurt_back, _) = reach(1.0);
        let evenness = |a: f32, b: f32| ((a - b).abs() / cycle - 0.5).abs();
        assert!(
            evenness(sound_forward, sound_back) < 0.02,
            "a sound stride is not two even halves: {sound_forward:.2} and {sound_back:.2} of {cycle:.2}"
        );
        assert!(
            evenness(hurt_forward, hurt_back) > 0.08,
            "a limping stride was as even as a sound one: {hurt_forward:.2} and {hurt_back:.2} of {cycle:.2}"
        );
        // ...and the bad leg takes a shorter step besides. See `LIMP_SHORT`.
        let longest = |limp: f32| {
            (0..400)
                .map(|step| {
                    let walked = step as f32 * cycle / 400.0;
                    joint_angle(Joint::LegRight, &mid_walk(walked, limp)).abs()
                })
                .fold(f32::MIN, f32::max)
        };
        assert!(longest(1.0) < longest(0.0) * 0.8, "the bad leg took a full step");
    }

    #[test]
    fn a_broken_left_leg_takes_the_short_step_on_the_left_and_a_right_one_on_the_right() {
        // The limp favoured the right leg whichever was broken. Both sides:
        // the bad leg's longest swing is the shorter of the two, and the
        // other leg keeps its full step.
        let cycle = 1.0 / PACES_PER_BLOCK;
        let longest = |joint: Joint, left: bool| {
            (0..400)
                .map(|step| {
                    let pose = Pose { limp_left: left, ..mid_walk(step as f32 * cycle / 400.0, 1.0) };
                    joint_angle(joint, &pose).abs()
                })
                .fold(f32::MIN, f32::max)
        };
        for (left, bad, good) in [(false, Joint::LegRight, Joint::LegLeft), (true, Joint::LegLeft, Joint::LegRight)] {
            assert!(
                longest(bad, left) < longest(good, left) * 0.8,
                "with the limp on the {} the bad leg took as long a step as the good one",
                if left { "left" } else { "right" }
            );
        }
        // ...and the dip comes as the *bad* leg takes the weight, which is half
        // a stride apart for the two sides.
        let dip = |walked: f32, left: bool| limp_dip(&Pose { limp_left: left, ..mid_walk(walked, 1.0) });
        let deepest = |left: bool| {
            (0..400)
                .map(|step| step as f32 * cycle / 400.0)
                .max_by(|a, b| dip(*a, left).total_cmp(&dip(*b, left)))
                .unwrap()
        };
        let apart = ((deepest(true) - deepest(false)).abs() / cycle - 0.5).abs();
        assert!(apart < 0.05, "the two limps dipped {apart:.2} of a stride away from half a stride apart");
    }

    #[test]
    fn a_seated_figure_rests_on_the_seat_with_its_legs_out_in_front() {
        // **The bug this exists for**: nobody could see anybody sit. A
        // player on a stool was drawn standing, because nothing on the wire
        // said otherwise. Seated, the figure is let down onto the point the
        // server put its feet on -- the seat -- and not through it, and its
        // legs reach forward, which with a yaw of zero is along +X.
        let standing = drawn(&Pose::default());
        let seated = drawn(&Pose { posture: Posture::Sitting, ..Pose::default() });
        let (stand_bottom, stand_top) = span(&standing, 1);
        let (seat_bottom, seat_top) = span(&seated, 1);
        assert!(seat_top < stand_top - 0.4, "sitting did not lower the figure ({seat_top} vs {stand_top})");
        assert!(seat_bottom > stand_bottom - 0.05, "a seated figure hangs down through the seat");
        assert!(
            span(&seated, 0).1 > span(&standing, 0).1 + 0.3,
            "a seated figure's legs do not reach out in front of it"
        );
    }

    /// **A swimmer is drawn swimming.** Nobody could see anybody swim: a
    /// player in a lake was drawn upright and striding on the bed of it. Face
    /// down, the figure lies along the way it is swimming (+X at a yaw of
    /// nought), at the water line a floating body rides at rather than at the
    /// feet the server put under it, and its arms go round as it swims.
    /// The flame on somebody else's torch is on the torch's head, on both
    /// faces of the plate, in the flame's own layer and at full light -- a
    /// fire in the dark is the brightest thing there, not the darkest.
    #[test]
    fn a_torch_somebody_else_holds_burns_on_both_faces_of_its_head() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        push_held_flame(Mat4::IDENTITY, 321, &mut vertices, &mut indices);
        assert_eq!((vertices.len(), indices.len()), (8, 12), "two quads, one a face");
        for v in &vertices {
            assert_eq!(v.packed >> 16, 321, "a flame in another layer");
            assert_eq!(v.packed & 0xff, 0xff, "a flame lit by the room it is lighting");
        }
        // The wad is the top half of the torch's picture (`hand::WAD_FACE`).
        let middle = vertices.iter().map(|v| v.position[1]).sum::<f32>() / 8.0;
        assert!(middle > 0.05, "the fire is on the stick, not the head: its middle is at {middle}");
        assert!(vertices[0].position[2] > 0.0 && vertices[4].position[2] < 0.0, "not on both faces of the plate");
    }

    #[test]
    fn a_swimming_figure_lies_along_its_stroke_at_the_water_line() {
        let swimming = |walked: f32| drawn(&Pose { posture: Posture::Swimming, walked, speed: 2.0, ..Pose::default() });
        let figure = swimming(0.0);
        let (bottom, top) = span(&figure, 1);
        let (back, front) = span(&figure, 0);
        assert!(front - back > 1.4, "a swimmer is not stretched out along the stroke: {back}..{front}");
        assert!(front > 0.8, "a swimmer is not head first the way it faces: {back}..{front}");
        assert!(bottom > 0.3, "a swimmer lies on the bed under the water: its lowest point is {bottom}");
        assert!(top > 1.2 && top < 1.9, "a swimmer is not at the water line: its highest point is {top}");
        // The arms go round with the water swum.
        let later = swimming(0.55);
        assert!(
            figure.iter().zip(&later).any(|(a, b)| (Vec3::from(a.position) - Vec3::from(b.position)).length() > 0.2),
            "a swimmer's arms do not move as it swims"
        );
    }

    #[test]
    fn a_lying_figure_lies_flat_on_the_mattress_as_long_as_it_is_tall() {
        // A sleeper was drawn standing upright in the middle of their bed.
        // Lying, the figure is laid along its yaw, as long as it stands tall,
        // no higher off the bed than its own depth, and not into the bed
        // under the point its feet were put on.
        let lying = drawn(&Pose { posture: Posture::Lying, ..Pose::default() });
        let (bottom, top) = span(&lying, 1);
        let (x0, x1) = span(&lying, 0);
        assert!(bottom > -0.05, "a lying figure sinks into the bed ({bottom})");
        assert!(top - bottom < 0.6, "a lying figure stands {} tall", top - bottom);
        assert!(x1 - x0 > PLAYER_HEIGHT * 0.9, "a lying figure is {} long", x1 - x0);
        // ...and laid across its feet rather than off to one side of them:
        // the server's point is the middle of the bed.
        assert!(x0 < -0.5 && x1 > 0.5, "the body is not laid either side of its middle ({x0}..{x1})");
    }

    fn walking() -> Pose {
        Pose {
            speed: WALKING,
            ..Pose::default()
        }
    }

    /// Standing still, wearing one thing.
    fn wearing(slot: Slot, block: BlockId) -> Pose {
        let mut outfit = Outfit::BARE;
        outfit.worn[slot.index()] = block;
        Pose {
            outfit,
            ..Pose::default()
        }
    }

    /// A full iron set, which is the loudest thing a player can be
    /// wearing and therefore the one worth checking the geometry of.
    fn in_full_iron() -> Pose {
        use primitive_shared::types::*;
        let mut outfit = Outfit::BARE;
        for block in [
            BLOCK_IRON_HELM,
            BLOCK_IRON_CUIRASS,
            BLOCK_IRON_GREAVES,
            BLOCK_IRON_BOOTS,
        ] {
            let slot = primitive_shared::equipment::slot_of(block).expect("a garment");
            outfit.worn[slot.index()] = block;
        }
        Pose {
            outfit,
            ..Pose::default()
        }
    }

    /// Every box in a mesh, as 24-vertex runs. What `append` guarantees
    /// -- six faces of four corners, per box, in order.
    fn boxes(vertices: &[ActorVertex]) -> impl Iterator<Item = &[ActorVertex]> {
        vertices.chunks_exact(24)
    }

    #[test]
    fn the_named_part_indices_still_name_those_parts() {
        // `OVERLAYS` and `hand_point` address `PARTS` by number, so a
        // reordering of that table would silently put a helmet on a
        // thigh and a pickaxe in a foot. Nothing else in this file can
        // notice that.
        for (index, name) in [
            (HEAD, "head"),
            (TORSO, "torso"),
            (ARM_RIGHT, "arm right"),
            (ARM_LEFT, "arm left"),
            (LEG_RIGHT, "leg right"),
            (LEG_LEFT, "leg left"),
        ] {
            assert_eq!(PARTS[index].name, name, "PARTS[{index}] moved");
        }
        // ...and every overlay rides on a part that exists.
        for overlay in OVERLAYS {
            assert!(overlay.part < PARTS.len(), "an overlay rides on nothing");
            let (top, bottom) = overlay.span;
            assert!(
                (0.0..1.0).contains(&top) && top < bottom && bottom <= 1.0,
                "{:?} covers a band that is not a band",
                overlay.slot,
            );
        }
    }

    /// A garment is drawn in the colour its wearer reads off their own
    /// pack.
    ///
    /// **The property, not the value.** Twelve garments share four
    /// greyscale pictures and are told apart by a tint
    /// (`types::garment_tint`); `hotbar::icon_tint` is the one place
    /// that decision is made, and the pack screen, the hotbar and the
    /// first-person hand all go through it. A model that read
    /// `garment_tint` for itself would be a fourth answer waiting to
    /// disagree -- which is exactly the bug `logic::hand` was fixed for,
    /// where a bronze cuirass was bronze in the pack and raw grey in the
    /// hand.
    #[test]
    fn a_garment_is_drawn_in_the_colour_its_wearer_sees_in_their_own_pack() {
        use primitive_shared::types::BLOCK_IRON_HELM;
        let vertices = drawn(&wearing(Slot::Head, BLOCK_IRON_HELM));
        // The body is emitted first and is exactly `PARTS.len()` boxes;
        // what follows is what is worn over it.
        let shell = &vertices[PARTS.len() * 24..];
        assert_eq!(shell.len(), 24, "a helmet should be one box");

        let wearer = crate::ui::hotbar::icon_tint(BLOCK_IRON_HELM, [1.0; 4]);
        for v in shell {
            assert_eq!(v.color, [wearer[0], wearer[1], wearer[2]]);
        }
        // ...and it is a colour rather than white, or this would pass on
        // a model that ignored the material entirely.
        assert_ne!(shell[0].color, [1.0; 3], "iron is drawn as no material at all");
        // ...and the body under it is untouched: the tint is the
        // garment's, not the figure's.
        assert_eq!(vertices[0].color, [1.0; 3]);
    }

    #[test]
    fn two_materials_in_one_slot_are_two_different_colours() {
        // The whole of what a player can tell apart at ten metres. If
        // leather and iron came out the same colour there would be no
        // reading of "he is in iron" available at all.
        use primitive_shared::types::{BLOCK_IRON_HELM, BLOCK_LEATHER_CAP, BLOCK_WOOL_CAP};
        let colour = |block| drawn(&wearing(Slot::Head, block))[PARTS.len() * 24].color;
        let (iron, leather, wool) = (
            colour(BLOCK_IRON_HELM),
            colour(BLOCK_LEATHER_CAP),
            colour(BLOCK_WOOL_CAP),
        );
        assert_ne!(iron, leather);
        assert_ne!(iron, wool);
        assert_ne!(leather, wool);
    }

    #[test]
    fn every_slot_of_the_worn_set_shows_somewhere_on_the_figure() {
        // **The reason `Overlay` carries a span at all.** There are four
        // slots and six body parts and they do not line up: without the
        // span there is no foot in `PARTS` to hang a boot on, and boots
        // would be the one garment in the game nobody can see.
        use primitive_shared::types::*;
        let bare = drawn(&Pose::default()).len();
        for block in [
            BLOCK_IRON_HELM,
            BLOCK_IRON_CUIRASS,
            BLOCK_IRON_GREAVES,
            BLOCK_IRON_BOOTS,
        ] {
            let slot = primitive_shared::equipment::slot_of(block).expect("a garment");
            assert!(
                drawn(&wearing(slot, block)).len() > bare,
                "the {} slot is worn and cannot be seen",
                slot.name(),
            );
        }
    }

    #[test]
    fn a_boot_is_at_the_foot_and_a_helmet_is_on_the_head() {
        // The span table is the only thing between "wearing boots" and
        // "wearing boots on your head", and a garment on the wrong limb
        // is a fault no test of the geometry as a whole can see.
        use primitive_shared::types::{BLOCK_IRON_BOOTS, BLOCK_IRON_HELM};
        let shell = |slot, block| {
            let vertices = drawn(&wearing(slot, block));
            let worn = vertices[PARTS.len() * 24..].to_vec();
            let low = worn.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let high = worn.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            (low, high)
        };
        let (_, boot_top) = shell(Slot::Feet, BLOCK_IRON_BOOTS);
        assert!(
            boot_top < PLAYER_HEIGHT * 0.3,
            "the boots reach {boot_top} of a {PLAYER_HEIGHT} player",
        );
        let (helm_low, _) = shell(Slot::Head, BLOCK_IRON_HELM);
        assert!(
            helm_low > PLAYER_HEIGHT * 0.6,
            "the helmet starts at {helm_low}, which is somewhere round the waist",
        );
    }

    #[test]
    fn a_boot_wears_the_bottom_of_the_leg_and_a_legging_the_top() {
        // The other half of the span: the *picture*. Both garments ride
        // on one leg box, so if they wore the same net a boot would be
        // a full-length legging squashed into a third of the room --
        // which looks like a texture bug and is a table bug.
        let leg = &PARTS[LEG_RIGHT];
        // Face 5 is the front, whose fourth number is the box's height.
        let [_, legging_y, _, legging_h] = garment_net(leg, 5, (0.0, 0.7));
        let [_, boot_y, _, boot_h] = garment_net(leg, 5, (0.7, 1.0));
        let [_, whole_y, _, whole_h] = net(leg, 5);
        assert!(boot_y > legging_y, "the boot reads above the legging");
        assert!((legging_h + boot_h - whole_h).abs() < 1e-5, "the two do not add up to a leg");
        assert!((legging_y - whole_y).abs() < 1e-5);
        assert!((boot_y + boot_h - (whole_y + whole_h)).abs() < 1e-5);
        // The crown and the underside are left whole -- their fourth
        // number is the box's *depth*, which a span says nothing about.
        for cap in 0..2 {
            assert_eq!(garment_net(leg, cap, (0.7, 1.0)), net(leg, cap));
        }
    }

    #[test]
    fn a_dressed_player_is_still_wound_to_face_out_of_every_box() {
        // The same fault `every_quad_of_the_player_is_wound_to_face_out`
        // exists for, over the boxes that did not exist when it was
        // written: a garment shell wound inside out is a suit of armour
        // you can see the inside of from every angle but one.
        for pose in [
            in_full_iron(),
            Pose {
                yaw: 2.1,
                pitch: -0.7,
                walked: 0.31,
                speed: WALKING,
                arm: Some(Arm::Swing(0.4)),
                ..in_full_iron()
            },
        ] {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            append(&pose, Vec3::new(3.0, 70.0, -8.0), [1.0; 3], &mut vertices, &mut indices);
            assert!(
                vertices.len() > PARTS.len() * 24,
                "a figure in full iron is drawn as a naked one",
            );
            for (index, one) in boxes(&vertices).enumerate() {
                let centre: Vec3 =
                    one.iter().fold(Vec3::ZERO, |sum, v| sum + Vec3::from(v.position)) / 24.0;
                for face in 0..6 {
                    let quad = &one[face * 4..face * 4 + 4];
                    let a = Vec3::from(quad[0].position);
                    let normal = (Vec3::from(quad[1].position) - a)
                        .cross(Vec3::from(quad[2].position) - a)
                        .normalize();
                    let outward = (a + Vec3::from(quad[2].position)) * 0.5 - centre;
                    assert!(
                        normal.dot(outward) > 0.0,
                        "box {index}, face {face} is wound inside out",
                    );
                }
            }
        }
    }

    #[test]
    fn a_player_in_full_iron_still_stands_in_very_nearly_the_same_box() {
        // The dressed counterpart of
        // `a_player_stands_in_the_box_the_rest_of_the_game_gives_them`,
        // and the reason `PADDING` is as small as it is. Two things must
        // hold: the figure does not get so wide that a player is a
        // silhouette you cannot hit, and the *feet* stay on the ground
        // -- a sole two centimetres under the floor reads as somebody
        // sinking into it, which is what `SEAM` is for.
        let vertices = drawn(&in_full_iron());
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &vertices {
            for axis in 0..3 {
                low[axis] = low[axis].min(v.position[axis]);
                high[axis] = high[axis].max(v.position[axis]);
            }
        }
        assert!(low[1] >= -0.001, "the soles are {} below the ground", low[1]);
        for axis in [0, 2] {
            let span = high[axis] - low[axis];
            assert!(span < 0.8, "a dressed player is {span} across axis {axis}");
        }
        // A hat sits on *top* of a head, so the crown is allowed past
        // the collider -- by the padding and not a millimetre more.
        assert!(
            high[1] <= PLAYER_HEIGHT + PADDING * SCALE + 1e-4,
            "the helmet reaches {} of a {PLAYER_HEIGHT} player",
            high[1],
        );
    }

    #[test]
    fn a_rucksack_is_one_box_behind_the_torso_and_clear_of_a_cuirass() {
        // **Behind, not around.** The reason it is not an `Overlay` is
        // that an overlay is a shell over a part, and a bag is a box
        // hanging off one -- so the two things worth pinning are that it
        // is exactly one box, and that its front face is behind where a
        // chest piece's back face lands. Two faces pointing the same way
        // at one depth flicker, and a pack drawn *inside* a cuirass is a
        // pack that vanishes the moment its owner puts armour on.
        use primitive_shared::types::{BLOCK_IRON_CUIRASS, BLOCK_RUCKSACK};
        let bare = drawn(&Pose::default()).len();
        let packed = drawn(&wearing(Slot::Back, BLOCK_RUCKSACK));
        assert_eq!(packed.len(), bare + 24, "a rucksack is not exactly one box");

        // Measured in the figure's own frame rather than in the world,
        // because the world's is turned by the yaw and what is being
        // pinned here is the four numbers in the table. A player faces
        // -Z in their own space (see `net`), so the back is +Z.
        let torso = &PARTS[TORSO];
        let torso_back = torso.at[2] + torso.size[2] / 2.0;
        let (cuirass_at, cuirass_size) = garment_box(torso, (0.0, 1.0));
        let cuirass_back = cuirass_at[2] + cuirass_size[2] / 2.0;
        let pack_front = PACK_AT[2] - PACK_SIZE[2] / 2.0;
        assert!(pack_front > torso_back, "the pack is inside the body");
        assert!(pack_front > cuirass_back, "the pack is inside a cuirass");
        // ...and not so far off the back that it floats.
        assert!(pack_front - cuirass_back < CELL_CLEARANCE, "the pack hovers behind the player");
        // It is a cuirass on a body, so the same check holds of the
        // drawn figure: adding a pack makes it deeper than adding
        // armour does.
        let _ = BLOCK_IRON_CUIRASS;

        // ...and it rides the torso, so it does not swing with a leg or
        // an arm: the whole box moves as one with the body.
        let mut walking = wearing(Slot::Back, BLOCK_RUCKSACK);
        walking.walked = 3.7;
        walking.speed = WALKING;
        let still = drawn(&wearing(Slot::Back, BLOCK_RUCKSACK));
        let moving = drawn(&walking);
        let pack_box = |v: &[ActorVertex]| v[PARTS.len() * 24..].to_vec();
        let (a, b) = (pack_box(&still), pack_box(&moving));
        assert_eq!(a.len(), 24, "the pack is not the last box drawn");
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x.position[1] - y.position[1]).abs() < 1e-5),
            "the pack swung with a limb"
        );
    }

    #[test]
    fn a_body_on_the_ground_is_still_wearing_the_rucksack_it_died_in() {
        // A death takes the pack and leaves what was worn
        // (`leave_corpse`), and the figure lying in the grass is built
        // from the same two constants the standing one uses -- so a
        // player does not change shape when they fall over.
        use primitive_shared::types::{BLOCK_AIR, BLOCK_RUCKSACK};
        let mut worn = [BLOCK_AIR; primitive_shared::equipment::SLOTS];
        let bare = dressed(&worn).len();
        worn[Slot::Back.index()] = BLOCK_RUCKSACK;
        let packed = dressed(&worn);
        assert_eq!(packed.len(), bare + 1, "a body on the ground lost its rucksack");
        assert_eq!(
            packed.last().map(|d| d.tint),
            Some(PACK_TINT),
            "the rucksack on the ground is not the colour it was on the back"
        );
    }

    #[test]
    fn wearing_nothing_costs_exactly_what_it_used_to() {
        // The overlays are skipped whole for a bare figure, which is
        // most figures on most worlds. A per-part branch that emitted a
        // degenerate box for an empty slot would double the vertex
        // count of an empty server for nothing.
        assert_eq!(drawn(&Pose::default()).len(), PARTS.len() * 24);
    }

    #[test]
    fn a_carried_thing_hangs_off_the_hand_and_swings_with_the_arm() {
        // **What a second copy of `place` would get wrong**, and it would
        // only be wrong while the arm was moving: a tool a foot away from the
        // fist holding it. The fist's frame and the corners of the arm are the
        // same rotation, so a point of the arm put through either lands in
        // the same place.
        let holding = |arm: Option<Arm>, yaw: f32| {
            let mut outfit = Outfit::BARE;
            outfit.holding = primitive_shared::types::BLOCK_STONE_AXE;
            Pose { outfit, arm, yaw, ..Pose::default() }
        };
        let feet = Vec3::new(3.0, 70.0, -8.0);
        for (arm, yaw) in [(None, 0.0), (Some(Arm::Swing(0.3)), 1.1), (Some(Arm::Eat(0.5)), -2.0)] {
            let pose = holding(arm, yaw);
            let hand = hand_point(&pose, feet);
            let frame = fist_frame(&pose, hand);
            let elbow = fist_local() + Vec3::new(0.0, 6.0, 0.0);
            let by_place = place(&pose, feet, Joint::ArmRight, PARTS[ARM_RIGHT].pivot, elbow);
            let by_frame = frame.transform_point3(Vec3::new(0.0, 6.0, 0.0) * SCALE);
            assert!((by_place - by_frame).length() < 1e-4, "the fist's frame is not the arm's ({by_place} against {by_frame})");
        }
        let rest = hand_point(&holding(None, 0.0), feet);
        // It hangs about hip height on a standing figure rather than
        // somewhere inside the chest or below the feet.
        let above = rest.y - feet.y;
        assert!((0.5..1.2).contains(&above), "the hand is {above} blocks off the ground");
        // Mid-blow the hand is somewhere else entirely, which is the whole
        // point of drawing it at all: a player watching another player mine
        // sees the pick move.
        let struck = hand_point(&holding(Some(Arm::Swing(0.3)), 0.0), feet);
        assert!((struck - rest).length() > 0.3, "the pick stays put through a blow ({})", (struck - rest).length());
        // ...and it turns with the player, rather than facing one way forever.
        let turned = hand_point(&holding(None, std::f32::consts::FRAC_PI_2), feet);
        assert!((turned - rest).length() > 0.2, "the hand did not turn with them");
    }

    /// The model of a shipped picture, cut the way the texture loader cuts it.
    fn sprite(path: &str) -> crate::engine::item_model::ItemModel {
        let bytes = crate::embedded::texture(path).unwrap_or_else(|| panic!("{path} is not in this build"));
        let image = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
        crate::engine::item_model::ItemModel::from_image(&image)
    }

    /// The butt and the head of a drawing held with `grip`, in the world.
    fn ends(pose: &Pose, grip: Grip, model: &crate::engine::item_model::ItemModel) -> (Vec3, Vec3) {
        let transform = held_sprite_transform(pose, Vec3::ZERO, Vec3::ZERO, grip, model);
        let (low, high) = model.drawn_box();
        (
            transform.transform_point3(Vec3::new(low[0], low[1], 0.0)),
            transform.transform_point3(Vec3::new(high[0], high[1], 0.0)),
        )
    }

    fn holding(block: BlockId) -> Pose {
        let mut outfit = Outfit::BARE;
        outfit.holding = block;
        Pose { outfit, ..Pose::default() }
    }

    /// **An axe is held by its haft, with its head forward and up.**
    ///
    /// "Игрок держит предметы странно": the picture's *middle* was put in
    /// front of the fist, so the haft stuck out ahead of the hand and the
    /// head trailed behind the wrist. Now the fist closes near the butt --
    /// the grip point is at the hand to the millimetre -- and the head is in
    /// front of the figure (+X for a yaw of nought) and above the hand, at
    /// rest and through a blow, where it comes up over the head and down in
    /// front.
    #[test]
    fn a_tool_is_gripped_near_the_butt_with_its_head_forward_and_up() {
        use primitive_shared::types::{BLOCK_STICK, BLOCK_STONE_AXE};
        for (block, path) in [(BLOCK_STONE_AXE, "tools/stone_axe.png"), (BLOCK_STICK, "plants/stick.png")] {
            let Some(_) = crate::embedded::texture(path) else {
                panic!("{path} is not in this build");
            };
            let model = sprite(path);
            assert_eq!(Grip::of(block, true), Grip::Haft, "{path} is not held by a haft");
            let pose = holding(block);
            let (butt, head) = ends(&pose, Grip::Haft, &model);
            let hand = hand_point(&pose, Vec3::ZERO);
            let along = (head - butt).normalize();
            let gripped = butt + (head - butt) * HAFT_GRIP;
            assert!((gripped - hand).length() < 1e-3, "{path}: the fist is {} from the grip", (gripped - hand).length());
            assert!(along.x > 0.4, "{path}: the head points back ({along})");
            assert!(along.y > 0.3, "{path}: the head points down ({along})");
            assert!(((head - butt).length() - HAFT_LENGTH).abs() < 1e-3);
            // At the top of the blow the head is up over the shoulder, and
            // where it lands it is out in front of the chest.
            let top = ends(&Pose { arm: Some(Arm::Swing(0.35)), ..pose }, Grip::Haft, &model).1;
            let landed = ends(&Pose { arm: Some(Arm::Swing(0.6)), ..pose }, Grip::Haft, &model).1;
            assert!(top.y > PLAYER_HEIGHT, "{path}: the head only reaches {} at the top of a blow", top.y);
            assert!(landed.x > 0.5 && landed.y > 0.6, "{path}: the blow lands at {landed}");
        }
    }

    /// **A spear is carried point up and forward, and levelled as it is
    /// driven.** It was held by its middle with the point dragging behind;
    /// and an arm that turned it would thrust it at the sky, so it is levelled
    /// rather than swung.
    #[test]
    fn a_spear_is_carried_point_up_and_driven_level() {
        use primitive_shared::types::BLOCK_FLINT_SPEAR;
        let model = sprite("tools/flint_spear.png");
        assert_eq!(Grip::of(BLOCK_FLINT_SPEAR, true), Grip::Spear);
        let pose = holding(BLOCK_FLINT_SPEAR);
        let (butt, point) = ends(&pose, Grip::Spear, &model);
        let along = (point - butt).normalize();
        assert!(along.x > 0.3 && along.y > 0.7, "a carried spear points {along}");
        assert!(butt.y > 0.0, "the butt is in the ground at {}", butt.y);
        let driven = Pose { arm: Some(Arm::Thrust(crate::logic::hand::LUNGE_HIT)), ..pose };
        let (butt, point) = ends(&driven, Grip::Spear, &model);
        let along = (point - butt).normalize();
        assert!(along.x > 0.95, "a driven spear points {along}");
        assert!(point.x > hand_point(&driven, Vec3::ZERO).x + 0.7, "the point is not out in front");
    }

    /// **A rod is held by its butt, its tip up over the fisher's head and out
    /// in front, and its painted line hangs down from the tip** -- for
    /// "удочка повёрнута не так как надо", where it was held like an axe: a
    /// switch three quarters of a block long pointed at the ground ahead.
    #[test]
    fn a_rod_is_held_by_its_butt_with_its_tip_up_and_out_and_its_line_hanging_from_the_tip() {
        use primitive_shared::types::BLOCK_FISHING_ROD;
        let model = sprite("tools/fishing_rod.png");
        assert_eq!(Grip::of(BLOCK_FISHING_ROD, true), Grip::Rod);
        let pose = holding(BLOCK_FISHING_ROD);
        let transform = held_sprite_transform(&pose, Vec3::ZERO, Vec3::ZERO, Grip::Rod, &model);
        let (butt, tip) = ends(&pose, Grip::Rod, &model);
        let hand = hand_point(&pose, Vec3::ZERO);
        assert!((butt + (tip - butt) * ROD_GRIP - hand).length() < 1e-3, "the fist is not at the butt of the rod");
        assert!(tip.y > PLAYER_HEIGHT, "the tip of a held rod is at {}, under the fisher's head", tip.y);
        assert!(tip.x > hand.x + 0.6, "the tip of a held rod is not out in front ({tip})");
        // Texels of the picture (`tools/fishing_rod.png`): the tip of the rod
        // at column 13 row 2, and the line hanging from it down column 14.
        let texel = |x: f32, y: f32| transform.transform_point3(Vec3::new((x + 0.5) / 16.0 - 0.5, 0.5 - (y + 0.5) / 16.0, 0.0));
        let (rod_tip, line) = (texel(13.0, 2.0), texel(14.0, 11.0));
        assert!(line.y < rod_tip.y - 0.3, "the line runs from the tip at {rod_tip} to {line}: not down");
    }

    /// **Food is a handful, in front of the knuckles, and it goes to the
    /// mouth.** It was half a block across -- a steak like a shield -- and the
    /// mouthful was lifted over the top of the head.
    #[test]
    fn a_mouthful_is_held_in_the_hand_and_lifted_to_the_mouth_and_not_over_the_head() {
        use primitive_shared::types::BLOCK_COOKED_MEAT;
        let model = sprite("food/cooked_meat.png");
        assert_eq!(Grip::of(BLOCK_COOKED_MEAT, true), Grip::Palm);
        let pose = holding(BLOCK_COOKED_MEAT);
        let transform = held_sprite_transform(&pose, Vec3::ZERO, Vec3::ZERO, Grip::Palm, &model);
        let (low, high) = model.drawn_box();
        let middle = transform.transform_point3(Vec3::new((low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5, 0.0));
        let across = (transform.transform_point3(Vec3::new(low[0], low[1], 0.0))
            - transform.transform_point3(Vec3::new(high[0], low[1], 0.0)))
        .length()
        .max(
            (transform.transform_point3(Vec3::new(low[0], low[1], 0.0))
                - transform.transform_point3(Vec3::new(low[0], high[1], 0.0)))
            .length(),
        );
        assert!((across - PALM_ACROSS).abs() < 1e-3, "a mouthful is {across} blocks across");
        assert!((middle - hand_point(&pose, Vec3::ZERO)).length() < 0.2, "the food is {middle}, away from the hand");

        let mouth = 26.0 * SCALE;
        let eating = hand_point(&Pose { arm: Some(Arm::Eat(0.5)), ..pose }, Vec3::ZERO);
        assert!((eating.y - mouth).abs() < 0.12, "the hand eats at {} where the mouth is at {mouth}", eating.y);
        let drinking = hand_point(&Pose { arm: Some(Arm::Drink(0.5)), ..pose }, Vec3::ZERO);
        assert!(drinking.y > eating.y && drinking.y < PLAYER_HEIGHT, "a drink is held at {}", drinking.y);
    }

    /// **The legs take a step for every footstep the game plays**, about two
    /// a second at a walk. They took nineteen: the step's length was read as
    /// strides per block.
    #[test]
    fn a_walking_figure_takes_a_step_for_every_footstep() {
        let strides_per_second = WALKING * PACES_PER_BLOCK;
        assert!((PACES_PER_BLOCK * 2.0 * STEP_BLOCKS - 1.0).abs() < 1e-6);
        assert!(
            (1.4..2.6).contains(&(strides_per_second * 2.0)),
            "a walking figure takes {} steps a second",
            strides_per_second * 2.0
        );
    }

    /// **Working at a block keeps the arm up and chopping**, eased up at the
    /// start and down at the end, and never the whole-arm windmill a blow
    /// from rest made at that rhythm.
    #[test]
    fn digging_chops_in_front_and_does_not_windmill() {
        let arm = |phase: f32, raised: f32| {
            joint_angle(Joint::ArmRight, &Pose { arm: Some(Arm::Chop { phase, raised }), ..Pose::default() })
        };
        let rest = joint_angle(Joint::ArmRight, &Pose::default());
        let angles: Vec<f32> = (0..40).map(|i| arm(i as f32 / 40.0, 1.0)).collect();
        let (low, high) = angles.iter().fold((f32::MAX, f32::MIN), |(l, h), a| (l.min(*a), h.max(*a)));
        assert!(low > 0.5, "a chop drops the arm to {low}");
        assert!(high < 2.5 && high - low > 1.0, "a chop works between {low} and {high}");
        assert!((arm(0.3, 0.0) - rest).abs() < 1e-5, "an arm not yet raised is not at rest");
        // The blow is faster coming down than going up.
        let up = (arm(0.66, 1.0) - arm(0.0, 1.0)) / 0.66;
        let down = (arm(0.999, 1.0) - arm(0.66, 1.0)) / 0.34;
        assert!(down.abs() > up.abs(), "the chop is raised faster ({up} a blow) than it falls ({down})");
    }

    /// A block in the hand is a cube where the hand is.
    ///
    /// **The sprite half of `append_held` is not tested here and cannot
    /// be**: which picture a tool wears comes out of a `TextureManager`,
    /// which is built from a GPU device, so a test without one can only
    /// reach the `models: None` arm. What that arm shares with the other
    /// -- the hand point, the transform, the light -- is checked above
    /// and here; the sprite's own placement is checked by eye, with
    /// `PRIMITIVE_POSE_PLAYERS` and `PRIMITIVE_POSE_HOLDING` (see
    /// `remote_players::posed_outfit`).
    #[test]
    fn a_block_in_the_hand_is_a_cube_at_the_hand() {
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::BLOCK_DIRT;

        let mut outfit = Outfit::BARE;
        outfit.holding = BLOCK_DIRT;
        let pose = Pose {
            outfit,
            ..Pose::default()
        };
        let feet = Vec3::new(3.0, 70.0, -8.0);
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let (mut item_vertices, mut item_indices) = (Vec::new(), Vec::new());
        append_held(
            &pose,
            feet.as_dvec3(),
            Vec3::ZERO,
            &FaceLayers::empty_for_test(),
            &LightMap::new(),
            None,
            &mut vertices,
            &mut indices,
            &mut item_vertices,
            &mut item_indices,
        );
        assert_eq!(vertices.len(), 24, "6 faces x 4 corners");
        assert_eq!(indices.len(), 36);
        assert!(item_vertices.is_empty(), "a cube went down the sprite pass");

        // Held out in front of the fist, near enough to be in it...
        let hand = hand_point(&pose, feet);
        let middle = vertices.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / vertices.len() as f32;
        assert!((middle - hand).length() < HELD_BLOCK, "the cube is {} from the hand", (middle - hand).length());
        // ...and moving with the arm: it used to be placed by the body's
        // facing alone, and stayed where it was while the arm reached out
        // to set it down.
        let (mut reached, mut reached_indices) = (Vec::new(), Vec::new());
        append_held(
            &Pose { arm: Some(Arm::Place(0.5)), ..pose },
            feet.as_dvec3(),
            Vec3::ZERO,
            &FaceLayers::empty_for_test(),
            &LightMap::new(),
            None,
            &mut reached,
            &mut reached_indices,
            &mut item_vertices,
            &mut item_indices,
        );
        let moved = reached.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / reached.len() as f32 - middle;
        assert!(moved.length() > 0.2, "the block stayed put while the arm reached ({moved})");
    }

    #[test]
    fn an_empty_hand_draws_nothing_at_all() {
        use primitive_shared::lighting::LightMap;
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let (mut item_vertices, mut item_indices) = (Vec::new(), Vec::new());
        append_held(
            &Pose::default(),
            (Vec3::ZERO).as_dvec3(),
            Vec3::ZERO,
            &FaceLayers::empty_for_test(),
            &LightMap::new(),
            None,
            &mut vertices,
            &mut indices,
            &mut item_vertices,
            &mut item_indices,
        );
        assert!(vertices.is_empty() && item_vertices.is_empty());
        assert!(indices.is_empty() && item_indices.is_empty());
    }

    /// Every quad faces out of the box it belongs to.
    ///
    /// **The fault this exists for**: `push_box` once wound its +Z and
    /// -Z faces the other way round, and the GPU -- which culls back
    /// faces on this pipeline -- then threw away the side of the model
    /// facing the player and drew the far side instead. The model was
    /// inside out, and every coordinate in it was correct, so nothing
    /// that checked the numbers could see it.
    ///
    /// Checked against the *emitted* winding rather than against the
    /// normals in the vertices, because those are computed from the
    /// winding too: this compares the geometry with where the box
    /// actually is.
    #[test]
    fn every_quad_of_the_player_is_wound_to_face_out_of_its_own_box() {
        // Turned and mid-stride, because a rotation with a sign error
        // reflects rather than rotates, and a reflection reverses every
        // winding in the model while leaving it looking plausible.
        for pose in [
            Pose::default(),
            walking(),
            Pose {
                yaw: 2.1,
                pitch: -0.7,
                walked: 0.31,
                speed: WALKING,
                arm: Some(Arm::Swing(0.4)),
                ..Pose::default()
            },
        ] {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            append(&pose, Vec3::new(3.0, 70.0, -8.0), [1.0; 3], &mut vertices, &mut indices);

            for (index, part) in PARTS.iter().enumerate() {
                // Every box is 24 vertices, in part order, six faces of
                // four -- so a quad's own box is arithmetic and not a
                // guess at which vertices are near each other.
                let box_vertices = &vertices[index * 24..(index + 1) * 24];
                let centre: Vec3 = box_vertices
                    .iter()
                    .fold(Vec3::ZERO, |sum, v| sum + Vec3::from(v.position))
                    / 24.0;
                for face in 0..6 {
                    let quad = &box_vertices[face * 4..face * 4 + 4];
                    let a = Vec3::from(quad[0].position);
                    let normal = (Vec3::from(quad[1].position) - a)
                        .cross(Vec3::from(quad[2].position) - a)
                        .normalize();
                    let outward = (a + Vec3::from(quad[2].position)) * 0.5 - centre;
                    assert!(
                        normal.dot(outward) > 0.0,
                        "{}: face {face} is wound inside out",
                        part.name,
                    );
                }
            }
        }
    }

    /// A player stands in the box the rest of the game gives them.
    ///
    /// The model is the only part of a player that is not the collider.
    /// The physics, the anti-cheat and the aim all use `PLAYER_HEIGHT`;
    /// a model that grew past it is a body you can see and walk through,
    /// and one that fell short is a player floating over their own feet.
    #[test]
    fn a_player_stands_in_the_box_the_rest_of_the_game_gives_them() {
        let vertices = drawn(&Pose::default());
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &vertices {
            for axis in 0..3 {
                low[axis] = low[axis].min(v.position[axis]);
                high[axis] = high[axis].max(v.position[axis]);
            }
        }
        assert!(
            low[1] >= -0.001 && low[1] < 0.05,
            "the feet are at {}, not on the ground",
            low[1]
        );
        assert!(
            (high[1] - PLAYER_HEIGHT).abs() < 0.001,
            "the head reaches {} of a {PLAYER_HEIGHT} player",
            high[1]
        );
        // Across and along: a figure with arms is always a little wider
        // than a cylinder 0.6 blocks through, and "a little" is the
        // whole question. A quarter of a block of overhang is a hand you
        // can see and cannot hit.
        for axis in [0, 2] {
            let span = high[axis] - low[axis];
            assert!(
                span < 0.8,
                "the model is {span} across axis {axis}, well past the 0.6 block collider",
            );
        }
    }

    /// Standing still is standing still -- except for the breath.
    ///
    /// A gait driven by a clock rather than by distance walks on the
    /// spot, which is the single most obvious thing a model can get
    /// wrong. Checked with the breath held (`age` fixed) so that this
    /// says what it means.
    #[test]
    fn a_player_who_is_not_moving_does_not_walk_on_the_spot() {
        let still = drawn(&Pose {
            walked: 3.5,
            ..Pose::default()
        });
        let also_still = drawn(&Pose {
            walked: 9.25,
            ..Pose::default()
        });
        for (a, b) in still.iter().zip(also_still.iter()) {
            for axis in 0..3 {
                assert!(
                    (a.position[axis] - b.position[axis]).abs() < 1e-6,
                    "the legs moved while the player stood still",
                );
            }
        }
    }

    /// ...and a standing player is not at attention.
    ///
    /// The complaint this answers is that the figure did not move at
    /// all. Legs that are still when somebody is still is correct; a
    /// body with nothing alive about it anywhere is not.
    #[test]
    fn a_standing_player_still_breathes() {
        let arm = |age: f32| joint_angle(Joint::ArmRight, &Pose { age, ..Pose::default() });
        // A quarter of a breath apart, which is where the difference is
        // largest -- and it is still small, which is the point.
        let apart = (arm(0.0) - arm(1.25)).abs();
        assert!(apart > 0.005, "nothing about a standing player moves ({apart})");
        assert!(apart < 0.1, "a standing player is fidgeting ({apart})");
        // ...and the arms hang forward rather than flat against the
        // sides.
        assert!(arm(0.0) > 0.05, "the arms are at attention");
    }

    /// A walking player swings the arm opposite the leg.
    ///
    /// The one property that makes a walk a walk. Arms in phase with the
    /// legs on the same side is a march, and it is wrong to look at from
    /// any distance.
    #[test]
    fn a_walking_player_swings_the_arm_opposite_the_leg() {
        // A quarter of the way through a stride, where the swing is at
        // its fullest -- at the crossing points every angle is zero and
        // the test would pass on a model that did nothing.
        let pose = Pose {
            walked: 0.25 / PACES_PER_BLOCK,
            ..walking()
        };
        let leg = joint_angle(Joint::LegRight, &pose);
        let arm = joint_angle(Joint::ArmRight, &pose);
        assert!(leg.abs() > 0.2, "the legs barely move at a walk ({leg})");
        assert!(
            leg * arm < 0.0,
            "the right arm ({arm}) swings with the right leg ({leg}), which is a march",
        );
        // ...and the two legs are opposite each other, which is what
        // separates a walk from a hop.
        let other = joint_angle(Joint::LegLeft, &pose);
        assert!(leg * other < 0.0, "both legs swing together");
    }

    /// A sprint is a longer stride, not a faster wave.
    #[test]
    fn a_running_player_swings_further_than_a_walking_one() {
        let at = |speed: f32| {
            joint_angle(
                Joint::LegRight,
                &Pose {
                    walked: 0.25 / PACES_PER_BLOCK,
                    speed,
                    ..Pose::default()
                },
            )
            .abs()
        };
        assert!(at(WALKING * 2.0) > at(WALKING * 0.5) + 0.05);
    }

    /// Legs stop cycling when the feet leave the ground.
    #[test]
    fn a_player_in_the_air_is_not_walking() {
        let pose = Pose {
            walked: 0.25 / PACES_PER_BLOCK,
            airborne: true,
            ..walking()
        };
        // Both legs forward-and-back in a fixed split, not the mirror
        // image of each other that a stride gives.
        let (right, left) = (
            joint_angle(Joint::LegRight, &pose),
            joint_angle(Joint::LegLeft, &pose),
        );
        assert!(right > 0.3 && left < 0.0, "a jump looks like a walk ({right}, {left})");
    }

    /// A blow comes up before it comes down, and ends where it began.
    ///
    /// The ending matters as much as the swing: an arm that does not
    /// come back to rest leaves the figure with one shoulder hunched
    /// until they next hit something.
    #[test]
    fn a_blow_comes_up_in_front_before_it_lands_and_returns_to_rest() {
        let arm = |t: f32| {
            joint_angle(
                Joint::ArmRight,
                &Pose {
                    arm: Some(Arm::Swing(t)),
                    ..Pose::default()
                },
            )
        };
        let rest = joint_angle(Joint::ArmRight, &Pose::default());
        // Up in front of the face -- not thrown back behind the shoulder,
        // which is a bowler -- and down through the target.
        assert!(arm(0.35) > 1.8, "the arm never comes up ({})", arm(0.35));
        assert!(arm(0.6) > 0.3 && arm(0.6) < 1.0, "the blow never lands ({})", arm(0.6));
        assert!((0..40).all(|i| arm(i as f32 / 40.0) > rest - 0.2), "the arm swings back behind the body");
        assert!(
            (arm(1.0) - rest).abs() < 1e-5,
            "the arm is left hanging at {} instead of {rest}",
            arm(1.0)
        );
        // ...and the left arm does not join in.
        assert!(
            (joint_angle(Joint::ArmLeft, &Pose { arm: Some(Arm::Swing(0.3)), ..Pose::default() })
                - joint_angle(Joint::ArmLeft, &Pose::default()))
            .abs()
                < 1e-6,
        );
    }

    /// Every gesture another player can be seen making leaves the arm at
    /// rest, goes somewhere, and gives the arm back.
    ///
    /// The ending is the half that goes wrong quietly: a figure left with a
    /// hand at its mouth after a meal is the hunched shoulder of the test above,
    /// five ways. A drink tips the head back and gives it back too, and a
    /// sleeper makes none of them.
    #[test]
    fn every_gesture_goes_somewhere_and_gives_the_arm_back() {
        let rest = joint_angle(Joint::ArmRight, &Pose::default());
        let arm = |motion: Arm| {
            joint_angle(Joint::ArmRight, &Pose { arm: Some(motion), ..Pose::default() })
        };
        type Shape = fn(f32) -> Arm;
        let shapes: [(&str, Shape); 5] = [
            ("swing", Arm::Swing),
            ("thrust", Arm::Thrust),
            ("placement", Arm::Place),
            ("mouthful", Arm::Eat),
            ("drink", Arm::Drink),
        ];
        for (name, shape) in shapes {
            assert!((arm(shape(0.0)) - rest).abs() < 1e-4, "a {name} starts away from rest");
            assert!(
                (arm(shape(1.0)) - rest).abs() < 1e-4,
                "a {name} leaves the arm at {} instead of {rest}",
                arm(shape(1.0))
            );
            let furthest = (1..40)
                .map(|step| (arm(shape(step as f32 / 40.0)) - rest).abs())
                .fold(0.0f32, f32::max);
            assert!(furthest > 0.5, "a {name} never moved the arm (at most {furthest})");
        }
        // A mouthful is at the face -- forward and past level -- and a thrust
        // is out in front at its point rather than over the shoulder.
        assert!(arm(Arm::Eat(0.5)) > std::f32::consts::FRAC_PI_2, "the food never reached the mouth");
        let point = arm(Arm::Thrust(crate::logic::hand::LUNGE_HIT));
        assert!(point > 1.0 && point < 2.0, "a thrust's point is out at {point} radians");

        let head = |pose: &Pose| joint_angle(Joint::Head, pose);
        let drinking = |t: f32| Pose { arm: Some(Arm::Drink(t)), ..Pose::default() };
        assert!(head(&drinking(0.5)) > head(&Pose::default()) + 0.2, "a drink never tipped the head");
        assert!((head(&drinking(1.0)) - head(&Pose::default())).abs() < 1e-4, "the head stayed tipped");

        let lying = Pose { posture: Posture::Lying, ..Pose::default() };
        assert_eq!(
            joint_angle(Joint::ArmRight, &Pose { arm: Some(Arm::Eat(0.5)), ..lying }),
            joint_angle(Joint::ArmRight, &lying),
            "a sleeper ate"
        );
    }

    /// Turning turns the whole of them, the light with it.
    ///
    /// A model whose normals did not turn is lit as though it had never
    /// moved, and a player walking a circle brightens and darkens for no
    /// reason anybody can name. This is the "plastic" look the rest of
    /// this renderer works to avoid.
    #[test]
    fn turning_a_player_turns_what_the_light_falls_on() {
        let north = drawn(&Pose::default());
        let east = drawn(&Pose {
            yaw: std::f32::consts::FRAC_PI_2,
            ..Pose::default()
        });
        let turned = north
            .iter()
            .zip(east.iter())
            .any(|(a, b)| (a.normal[0] - b.normal[0]).abs() > 0.5);
        assert!(turned, "the model turned but its faces were lit as before");
    }

    /// The figure faces the way the server says they are facing.
    ///
    /// A sign error in the quarter turn that maps the model's front to
    /// +X is a whole world of players walking backwards, and it has
    /// happened here before -- to the animals, in exactly this line of
    /// arithmetic.
    #[test]
    fn a_player_faces_the_way_they_are_looking() {
        // Yaw zero looks along +X, so the face -- the -Z face of the
        // head, the only quad wearing the front of the head's net --
        // must be the furthest part of the model along +X.
        let vertices = drawn(&Pose::default());
        let head = &vertices[0..24];
        let front = &head[5 * 4..5 * 4 + 4];
        let front_x = front.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        let any_x = head.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        assert!(
            (front_x - any_x).abs() < 1e-6,
            "the face is at x={front_x} and the back of the head at {any_x}",
        );
    }

    /// Every face lands on a painted part of the skin.
    ///
    /// **The mechanism this pins down**: the picture is drawn by
    /// `examples/gen_placeholder_textures.rs`, which cannot see this
    /// module -- an example only gets the crate's public surface -- so
    /// the net arithmetic exists twice. If the two ever disagree, a face
    /// samples a part of the sheet nobody painted, and the symptom is a
    /// transparent or wrongly-coloured limb that no test of the geometry
    /// can see.
    #[test]
    fn every_face_of_the_model_lands_on_a_painted_part_of_the_skin() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/textures/players/player.png");
        let skin = image::open(&path)
            .unwrap_or_else(|e| panic!("{} is missing ({e})", path.display()))
            .to_rgba8();
        // In texels of *this* picture, which may be a resource pack's
        // larger redraw of the same layout.
        let sx = skin.width() as f32 / SHEET_WIDTH;
        let sy = skin.height() as f32 / SHEET_HEIGHT;
        for part in PARTS {
            for face in 0..6 {
                let [x, y, w, h] = net(part, face);
                let mut opaque = 0;
                let mut total = 0;
                for ty in 0..(h * sy) as u32 {
                    for tx in 0..(w * sx) as u32 {
                        let px = (x * sx) as u32 + tx;
                        let py = (y * sy) as u32 + ty;
                        total += 1;
                        if skin.get_pixel(px, py).0[3] == 255 {
                            opaque += 1;
                        }
                    }
                }
                assert_eq!(
                    opaque, total,
                    "{}: face {face} reads texels ({x},{y},{w},{h}) and {} of them are \
                     transparent -- the sheet and the net disagree",
                    part.name,
                    total - opaque,
                );
            }
        }
    }
    // ---- the body on the ground ----

    /// The corners of every box a stage of a dead player is drawn with.
    fn fallen_corners(stage: Dead) -> Vec<[f32; 3]> {
        let mut quads = Vec::new();
        fallen_quads(stage, Vec3::new(0.5, 0.0, 0.5), 0.0, &mut quads);
        quads.into_iter().flatten().collect()
    }

    fn extent(points: &[[f32; 3]], axis: usize) -> (f32, f32) {
        points
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[axis]), hi.max(p[axis])))
    }

    /// **A body lies on the ground, on its side, as long as the player was
    /// tall.**
    ///
    /// The three things that go wrong when a figure built standing is laid
    /// down: it stays standing; it is placed by a point that is no longer
    /// its underside and sinks into the floor; or it is measured square,
    /// drawn with its limbs thrown out, and hangs in the air by the
    /// difference. All three are one piece of arithmetic
    /// (`animal_model::resting_pose`) and this is the sentence that says
    /// it worked, for the body and for the bones alike.
    #[test]
    fn a_body_lies_on_the_ground_as_long_as_the_player_was_tall() {
        for stage in [Dead::Body, Dead::Bones] {
            let corners = fallen_corners(stage);
            let (low, high) = extent(&corners, 1);
            assert!(
                (0.0..=0.03).contains(&low),
                "{stage:?} rests {low} above the floor of its cell",
            );
            assert!(high < 0.8, "{stage:?} stands {high} blocks tall: it did not lie down");
            let along = (extent(&corners, 0).1 - extent(&corners, 0).0)
                .max(extent(&corners, 2).1 - extent(&corners, 2).0);
            assert!(
                (along - PLAYER_HEIGHT).abs() < 0.4,
                "{stage:?} is {along} blocks long where the player it was is {PLAYER_HEIGHT} tall",
            );
        }
    }

    /// **A dead player's body wears their skin and their clothes, and faces
    /// out of every box.** It was dressed in five flat patches of a block's
    /// picture and nothing they had on. Every corner of the lying figure reads
    /// the skin sheet inside its own net; a helmet in the body adds one shell,
    /// in the helmet's colour; and every quad is wound to face away from the
    /// middle of its box, turned or not -- the actor pipeline culls back
    /// faces, and a body wound inside out is drawn from within.
    #[test]
    fn a_body_on_the_ground_wears_its_skin_and_its_clothes_and_faces_out() {
        use primitive_shared::types::BLOCK_IRON_HELM;
        let bare = [BLOCK_AIR; primitive_shared::equipment::SLOTS];
        let mut helmed = bare;
        helmed[Slot::Head.index()] = BLOCK_IRON_HELM;
        for (worn, yaw) in [(bare, 0.0f32), (helmed, 2.3)] {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            append_lying(Vec3::new(4.5, 10.0, -2.5), yaw, &worn, &mut vertices, &mut indices);
            let shells = if worn == bare { 0 } else { 1 };
            assert_eq!(vertices.len(), (PARTS.len() + shells) * 24);
            assert_eq!(indices.len(), (PARTS.len() + shells) * 36);
            for v in &vertices {
                assert!((0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1]), "a corner reads {:?} off the sheet", v.uv);
            }
            let low = vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let high = vertices.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            assert!((10.0..10.03).contains(&low), "the body rests at {low} on a floor at 10");
            assert!(high < 10.8, "the body stands {} tall", high - 10.0);
            for faces in vertices.chunks_exact(24) {
                let middle = faces.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 24.0;
                for quad in faces.chunks_exact(4) {
                    let centre = quad.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 4.0;
                    let corners = [quad[0].position, quad[1].position, quad[2].position, quad[3].position];
                    let normal = Vec3::from(face_normal(&corners));
                    assert!(normal.dot(centre - middle) > 0.0, "a face of the body is wound inside out");
                }
            }
            if shells == 1 {
                let tint = vertices[PARTS.len() * 24].color;
                assert_eq!(tint, garment_colour(BLOCK_IRON_HELM, [1.0; 3]), "the helmet is not its own colour");
            }
        }
    }

    /// Where along the figure's length one named box of one stage ends up.
    fn along(stage: Dead, name: &str) -> f32 {
        let (limbs, pose) = fallen(stage);
        let index = limbs
            .iter()
            .position(|limb| limb.part.name == name)
            .unwrap_or_else(|| panic!("{stage:?} has no {name}"));
        let mut quads = Vec::new();
        crate::logic::animal_model::posed_quads(
            std::slice::from_ref(&limbs[index].part),
            pose,
            Vec3::new(0.5, 0.0, 0.5),
            0.0,
            |_| limbs[index].swing,
            &mut quads,
        );
        let corners: Vec<[f32; 3]> = quads.into_iter().flatten().collect();
        let (lo, hi) = extent(&corners, 2);
        (lo + hi) * 0.5
    }

    /// **The bones lie where the body lay, with the skull at the end the
    /// head was at.**
    ///
    /// Two days pass and the same cell is drawn a different way; what a
    /// player must not see is the thing turning over or swapping ends as
    /// it rots. The animals' skeletons had exactly that fault, and it is
    /// written up at length in `animal_model::build_bones`. This is the
    /// same property for a person, and it holds here for a different
    /// reason: the bones are built in the living figure's own frame and
    /// rolled by the same quarter turn, wearing the same `LIMP`.
    #[test]
    fn the_bones_of_a_body_lie_where_the_body_lay() {
        let (head, foot) = (along(Dead::Body, "head"), along(Dead::Body, "leg right"));
        let (skull, shin) = (along(Dead::Bones, "skull"), along(Dead::Bones, "shin"));
        assert!(
            (head - skull).abs() < 0.25,
            "the skull is {:.2} blocks from where the head was",
            (head - skull).abs(),
        );
        assert!(
            (foot - shin).abs() < 0.25,
            "the legs moved {:.2} blocks as the body rotted",
            (foot - shin).abs(),
        );
        assert!(
            (head - foot).signum() == (skull - shin).signum(),
            "the body rotted head over heels: it lay {head:.2}..{foot:.2} and its bones lie \
             {skull:.2}..{shin:.2}"
        );
    }

    /// **No two bones of a dead player fight over one plane.**
    ///
    /// Where a rib runs into the spine, a jaw into the skull or a thigh
    /// into the pelvis, two boxes overlap -- and if a face of each lands
    /// in the same plane the depth buffer picks a winner by rounding and
    /// changes its mind as the camera moves. `animal_model::SEAM_BITE`
    /// cannot settle it: it grows both boxes alike, and two coplanar faces
    /// stay coplanar. The rule is the animals'
    /// (`animal_model::CLEARANCE`): a bone that runs into another is a
    /// different size on every axis by more than the depth buffer can
    /// resolve at the range a body is legible from.
    ///
    /// Twenty-three boxes placed by hand is twenty-three chances to line
    /// two faces up, and the one that was there when this was written was
    /// a shin with its underside in the same plane as the foot's.
    ///
    /// Asked in the figure's own frame and only of bones that turn
    /// together: two that are swung by different amounts are no longer
    /// parallel, so they have no planes to share.
    #[test]
    fn no_two_bones_of_a_dead_player_fight_over_one_plane() {
        use crate::logic::animal_model::CLEARANCE;
        let bones = bones();
        let bounds = |part: &crate::logic::animal_model::Part, axis: usize| {
            let half = part.size[axis] * 0.5;
            (part.at[axis] - half, part.at[axis] + half)
        };
        for (i, a) in bones.iter().enumerate() {
            for b in bones.iter().skip(i + 1) {
                let overlaps = (0..3).all(|axis| {
                    let (alo, ahi) = bounds(&a.part, axis);
                    let (blo, bhi) = bounds(&b.part, axis);
                    alo < bhi && blo < ahi
                });
                if !overlaps || a.swing != b.swing {
                    continue;
                }
                for axis in 0..3 {
                    let (alo, ahi) = bounds(&a.part, axis);
                    let (blo, bhi) = bounds(&b.part, axis);
                    for (one, other, side) in [(alo, blo, "underside"), (ahi, bhi, "top")] {
                        assert!(
                            (one - other).abs() >= CLEARANCE,
                            "the {side} of the {} and of the {} are {:.3} apart on axis {axis}: \
                             they will flicker against each other",
                            a.part.name,
                            b.part.name,
                            (one - other).abs(),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_watched_cast_goes_back_over_the_shoulder_out_along_the_line_and_home() {
        let arm = |motion: Option<Arm>| joint_angle(Joint::ArmRight, &Pose { arm: motion, ..Pose::default() });
        let rest = arm(None);
        let back = arm(Some(Arm::Wind(1.0)));
        assert!((back - ROD_BACK).abs() < 1e-4, "a full wind-up holds the arm at {back}, not {ROD_BACK}");
        assert!(arm(Some(Arm::Wind(0.5))) < back && arm(Some(Arm::Wind(0.5))) > rest, "half a wind-up is not between");
        // The throw starts where the wind-up is, or the arm jumps as the
        // line goes; it is out along the line at the moment the player's
        // own rod is; and it gives the arm back.
        assert!((arm(Some(Arm::Cast(0.0))) - back).abs() < 1e-4, "the throw does not start from the wind-up");
        assert!((arm(Some(Arm::Cast(ROD_CAST_OUT))) - ROD_OUT).abs() < 1e-3, "the throw is not out along the line");
        assert!((arm(Some(Arm::Cast(1.0))) - rest).abs() < 1e-4, "the throw leaves the arm out");
        // Fast out and slow home: the snap is a third of the time.
        let out = (0..=10).map(|k| arm(Some(Arm::Cast(ROD_CAST_OUT * k as f32 / 10.0))));
        assert!(out.clone().zip(out.skip(1)).all(|(a, b)| b <= a + 1e-5), "the snap forward turned back on itself");
    }

    #[test]
    fn a_rider_s_feet_hang_either_side_of_a_horse_and_not_through_it() {
        use primitive_shared::animals::Species;
        // A horse standing at the origin with its feet on y = 0, and its
        // rider where the server puts one: `RIDER_LIFT` over the horse's
        // feet, at its middle, facing its way. Both models turn their own x
        // into world z at a yaw of nought, so "across the horse" is z here.
        let horse = crate::logic::animal_model::parts(Species::Horse);
        let body = horse.iter().find(|part| part.name == "body").expect("the horse has a body");
        let sixteenth = 1.0 / 16.0;
        let middle = Species::Horse.height() * 0.5;
        let back = middle + (body.at[1] + body.size[1] * 0.5) * sixteenth;
        let belly = middle + (body.at[1] - body.size[1] * 0.5) * sixteenth;
        // The barrel, and the saddle's skirt over it (`animal_model::append_tack`).
        let flank = (body.size[0] * 0.5 + 0.7) * sixteenth;

        let pose = Pose { posture: Posture::Mounted, ..Pose::default() };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        append(&pose, Vec3::Y * primitive_shared::horse::RIDER_LIFT, [1.0; 3], &mut vertices, &mut indices);
        // The body's boxes in `PARTS` order, and each leg as two: head,
        // torso, two arms, then thigh and shin of the right and the left.
        let boxed = |index: usize| &vertices[index * 24..index * 24 + 24];
        assert_eq!(vertices.len(), (PARTS.len() + 2) * 24, "a mounted leg is not a thigh and a shin");

        // Seated on the saddle, not in the horse and not hovering over it.
        let (seat, _) = span(boxed(TORSO), 1);
        let saddle = crate::logic::animal_model::saddle_top();
        assert!((seat - saddle).abs() < 0.02, "the rider sits at {seat:.3} on a saddle whose top is {saddle:.3}");

        for (shin, which) in [(5, "right"), (7, "left")] {
            let (low, _) = span(boxed(shin), 1);
            assert!(low < back - 0.2, "the {which} foot is at {low:.2}, which is not hanging down a back at {back:.2}");
            assert!(low > belly * 0.5, "the {which} foot is at {low:.2}, dangling to the horse's knees");
            let (near, far) = span(boxed(shin), 2);
            let clear = if which == "right" { near } else { -far };
            assert!(
                clear > flank,
                "the {which} shin comes within {clear:.3} of the middle, inside a barrel and skirt {flank:.3} across"
            );
        }
        // ...and a thigh goes out over the back, starting on it.
        for (thigh, which) in [(4, "right"), (6, "left")] {
            let (low, _) = span(boxed(thigh), 1);
            assert!(low > back - 0.25, "the {which} thigh is at {low:.2}, under a back at {back:.2}");
        }
    }
}
