//! What an animal looks like: a table of boxes.
//!
//! ## Why the model is data and not code
//!
//! The first boar was two cubes written out longhand inside the mesher,
//! and changing it meant editing arithmetic. That is fine for two cubes
//! and hopeless for nine: every proportion is a number buried in an
//! expression, nothing can be moved without moving something else by
//! hand, and adding a second animal means copying the whole function.
//!
//! So a model is a **list of `Part`s** and nothing else, and the mesher
//! is one loop over it. Editing a boar is editing numbers in a table.
//!
//! ## The units are Blockbench's
//!
//! Sixteenths of a block, because that is what anybody sitting in a
//! model editor is already thinking in: a block is 16, a boar stands 16
//! at the shoulder, its body is 13 wide. Every number below is the
//! number you would type into Blockbench, and `SCALE` is the only place
//! that stops being true.
//!
//! The origin is the **centre of the animal**, which is the point the
//! server sends (see `logic::animals::Animal::state`), with -Z forward
//! and +Y up. A part at `at: [0, 0, -12]` is twelve sixteenths in front
//! of the middle -- the head.
//!
//! ## What a part wears
//!
//! One `Skin` per part, and optionally a different one on the face that
//! looks forward. That is the whole texture model, and it is deliberately
//! this coarse: a terrain vertex carries its texture coordinates in two
//! bits (0 or 1, see `mesh::Vertex`), so a face wears a *whole picture*
//! and cannot wear a corner of one. What that costs is that a detailed
//! animal is several small textures rather than one atlas; what it buys
//! is that animals ride the terrain pipeline and get its lighting, its
//! fog and its texture array for nothing.
//!
//! ## What moves
//!
//! `Gait` says how a part answers to walking. The client knows how fast
//! an animal is going -- it has two position samples and the interval
//! between them -- so legs swing without the server sending a single
//! extra byte.

use glam::Vec3;

use primitive_shared::animals::Species;
use primitive_shared::horse::{TACK_BAGS, TACK_HALTER, TACK_RIDDEN, TACK_SADDLE, TACK_SHORN, TACK_STALLION};
use primitive_shared::protocol::Attitude;

use crate::engine::texture::FaceLayers;

/// One sixteenth of a block: the unit every number in this file is in.
pub const SCALE: f32 = 1.0 / 16.0;

/// Which picture a part wears.
///
/// An enum rather than a texture name, so a model row cannot name a file
/// that is not loaded -- and so the whole set of pictures an animal
/// needs is visible in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Skin {
    /// The species' own hide, which is what nearly everything wears.
    Hide,
    /// The sides of the head: where an eye goes.
    Head,
    /// The front of the head. Hide, going dark toward the muzzle -- not
    /// bare skin: the bare part is the snout, and it is a separate box
    /// in front of this one.
    Face,
    /// The snout itself: bare skin, all round.
    Snout,
    /// ...and the end of it, which is the bit with the nostrils.
    Nose,
    /// Pale, for tusks.
    Tusk,
    /// An ear: fur round the rim, bare skin inside.
    ///
    /// Its own picture rather than `Fur`, because what makes an ear read
    /// as an ear at three pixels is that you can see *into* it. Every
    /// ear in the game was a lump of coarse hide before this.
    Ear,
    /// Bone, for the one animal that carries any on its head.
    Antler,
    /// Dark, for the feet.
    Hoof,
    /// The head again, mirrored.
    ///
    /// **The two sides of a box are mirror images of each other**, and
    /// that is not a choice this file makes -- it is what `face_uv`
    /// does, because the +X face maps `u = 1 - z` and the -X face maps
    /// `u = z`. One picture on both therefore puts the eye at the nose
    /// on one side of the animal and **at the back of its skull on the
    /// other**, which is exactly what it looks like.
    ///
    /// A face wears a whole picture and cannot wear a flipped one, so
    /// the flip has to exist as a second picture. See `Part::sides`.
    HeadMirror,
    /// A horn: a ringed sheath over a core of bone, for the one animal that
    /// carries horns rather than antlers.
    ///
    /// **Its own picture rather than `Antler`, in the slot the sheet had
    /// spare (eleven).** A horn is not an antler in both of the ways this
    /// game can see: it is dark and ringed where an antler is pale bone, and
    /// a skeleton keeps only its core -- the sheath rots -- where it keeps
    /// the whole antler. Dressing the antelope in `Antler` would have given
    /// it the deer's bone and made `only_the_deer_carries_bone_on_its_head`
    /// a sentence that was no longer true of the picture.
    Horn,
    /// The hide again, drawn coarse, for the parts that are two or three
    /// pixels across.
    ///
    /// A face wears a whole picture, so a full-detail hide on an ear is
    /// eight times the texel density of the same hide on the body beside
    /// it -- which reads as noise glued to a clean animal rather than as
    /// detail.
    Fur,
}

impl Skin {
    /// Every picture, in `slot` order: `Skin::ALL[skin.slot()] == skin`, so
    /// the square of the sheet a model file's face points at names its skin
    /// (see `logic::models`).
    pub const ALL: [Skin; 12] = [
        Skin::Hide,
        Skin::Head,
        Skin::HeadMirror,
        Skin::Face,
        Skin::Snout,
        Skin::Nose,
        Skin::Ear,
        Skin::Hoof,
        Skin::Fur,
        Skin::Tusk,
        Skin::Antler,
        Skin::Horn,
    ];

    /// Which layer of the texture array this is, for a given species.
    ///
    /// **Anything a species has no picture for falls back to its own
    /// hide**, which is what makes the table above usable: a deer row
    /// can ask for a hoof whether or not anybody has drawn one, and a
    /// hare asking for a tusk gets fur rather than a boar's ivory.
    ///
    /// The alternative -- a full set of pictures per species before any
    /// of them can be drawn -- is what turns "add an animal" into a
    /// morning of pixel art before you can see whether the proportions
    /// are right.
    fn layer(self, species: Species, layers: &FaceLayers) -> u32 {
        layers.animal(species, self.slot())
    }

    /// Where on an animal's sheet this picture sits.
    ///
    /// **Species-independent, and that is the whole change.** There used
    /// to be a match here with one arm per species per picture -- forty
    /// lines that had to be extended by nine every time an animal was
    /// added, and whose failure mode was a wolf wearing a boar's tusks.
    /// A sheet has a fixed grid, this says which square, and whether a
    /// given animal *has* that part is decided by whether the square was
    /// drawn in (see `texture::ANIMAL_SHEETS`).
    ///
    /// The order is the order a person would lay them out: the hide
    /// first because everything falls back to it, then the head and what
    /// is on it, then the feet, then the odd ones out.
    pub fn slot(self) -> usize {
        match self {
            Skin::Hide => 0,
            Skin::Head => 1,
            Skin::HeadMirror => 2,
            Skin::Face => 3,
            Skin::Snout => 4,
            Skin::Nose => 5,
            Skin::Ear => 6,
            Skin::Hoof => 7,
            Skin::Fur => 8,
            Skin::Tusk => 9,
            Skin::Antler => 10,
            Skin::Horn => 11,
        }
    }

    /// Whether this picture is a *material* -- something that tiles, like
    /// hide or bone -- or a *feature*, drawn to fit its face.
    ///
    /// The distinction decides who gets a texture crop (see
    /// `mesh::FINE_UV_BIT`). A material on a small part should show a
    /// small piece of itself: a leg four sixteenths across wearing all
    /// sixteen columns of hide is eight times the texel density of the
    /// body beside it, and that mismatch is what reads as noise glued to
    /// a clean animal. A feature must never be cropped: the eye in the
    /// side of a head and the nostrils on the end of a muzzle are drawn
    /// *for the whole face*, and a crop would cut them off.
    pub fn tiles(self) -> bool {
        match self {
            Skin::Hide
            | Skin::Fur
            | Skin::Hoof
            | Skin::Snout
            | Skin::Tusk
            | Skin::Antler
            | Skin::Horn => true,
            Skin::Head | Skin::HeadMirror | Skin::Face | Skin::Nose | Skin::Ear => false,
        }
    }
}

/// How a part answers to the animal walking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    /// Rides with the body. Most of an animal.
    Still,
    /// Swings back and forth about its top, in phase.
    ///
    /// The two pairs are in *opposite* phase -- see `phase_of` -- which
    /// is what makes four legs read as a walk rather than as a hop.
    LegFront,
    LegBack,
    /// Nods, a little, in time with the legs. A head that is perfectly
    /// still on a moving body is the thing that makes a model read as a
    /// prop being slid along the ground.
    Head,
    /// **A wing, spread**, drawn only while the bird is in the air, and
    /// rolled about the bird's length through its shoulder -- the hinge, at
    /// `x` sixteenths -- by the wingbeat (`wingbeat`). A roll rather than a
    /// swing, and hinged at the inner edge rather than the top, because a
    /// wing beats up and down about the body and a leg swings fore and aft
    /// about its hip.
    Wing(i8),
    /// **A wing, folded**, drawn only while the bird is on its feet. See
    /// `GULL` for why a gull has two pairs of wings rather than one that
    /// folds.
    Folded,
}

impl Gait {
    /// Is a part with this gait drawn, with the bird in the air or not?
    ///
    /// Everything but a wing is drawn either way. Asked by `build` and by
    /// every measure of the animal's size (`half_extents`), which is taken
    /// standing -- a blow lands on a bird on the ground, and a gull's spread
    /// wings are two blocks across.
    pub fn shown(self, aloft: bool) -> bool {
        match self {
            Gait::Wing(_) => aloft,
            Gait::Folded => !aloft,
            _ => true,
        }
    }
}

/// One box of an animal.
///
/// Copy and small, so the whole model of a species is a `const` slice
/// and costs nothing at run time.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    /// What it is.
    ///
    /// Read by a person and by the tests, and by nothing that draws --
    /// which is the point. A model whose parts are unnamed is a list of
    /// numbers, and the whole argument for putting the model in a table
    /// was that somebody can sit down and change it.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Centre of the box, in sixteenths, from the animal's own centre.
    pub at: [f32; 3],
    /// How big, in sixteenths.
    pub size: [f32; 3],
    pub skin: Skin,
    /// What the forward-looking face wears instead, if anything.
    pub front: Option<Skin>,
    /// What the two side faces wear instead, as `(right, left)`.
    ///
    /// Two entries rather than one because the sides of a box are mirror
    /// images -- see `Skin::HeadMirror`. Anything with something *on* it
    /// rather than a repeating hide has to say which side is which, or
    /// it wears the picture backwards on one of them.
    pub sides: Option<(Skin, Skin)>,
    pub gait: Gait,
    /// A turn about the vertical axis through the box's own centre, in
    /// radians, before anything else happens to it: positive turns the
    /// box's +X toward -Z.
    ///
    /// **Zero on every living part; it is there for the skeleton's legs.**
    /// A leg lying on the ground angled forward is a box that is not
    /// square to the body, and the only other ways to draw one were a
    /// staircase of boxes -- which reads as a staircase -- or a corner
    /// routine for bones beside `posed_local`, which is two copies of the
    /// arithmetic that places a corner and lights a face, and the day they
    /// disagree is a leg lit as though it pointed somewhere else. One
    /// field every living part leaves at zero keeps one path;
    /// `world_face_of` turns the normal by it as well.
    pub turn: f32,
    /// Where the part swings from, as `(y, z)` in sixteenths in the
    /// animal's frame, when that is not the middle of its own top. The x
    /// is always the part's own: a swing turns about the across axis, and
    /// that axis does not move.
    ///
    /// **`None` on nearly everything, because a leg that is one box is
    /// right to swing about its own top.** Anything hung off another
    /// moving part is not. A bear's paw is a box of its own -- broader than
    /// the leg and reaching forward of it -- and swung about its *own* top
    /// it turns at the ankle while the leg turns at the shoulder, ten
    /// sixteenths higher: at a full stride that is seven sixteenths of
    /// daylight between the foot and the paw that was on it. So the paw
    /// names the leg's joint and the two turn as one piece. The same for a
    /// head: a muzzle nodding about its own top rather than the neck slides
    /// against the skull by nearly as much as it is sunk into it.
    ///
    /// Held to the geometry by
    /// `every_paw_stays_on_the_foot_of_its_leg_through_the_stride`.
    pub pivot: Option<[f32; 2]>,
}

/// The default row, so a model line only says what is unusual about it.
pub(crate) const PART: Part = Part {
    name: "",
    at: [0.0, 0.0, 0.0],
    size: [1.0, 1.0, 1.0],
    skin: Skin::Hide,
    front: None,
    sides: None,
    gait: Gait::Still,
    turn: 0.0,
    pivot: None,
};

/// Half the size of the box the model actually fills, in blocks.
///
/// **Measured, and used to check the declared box rather than to aim
/// with.** What a swing is tested against is `Species::half_extents` --
/// the shared box, which the server validates blows with -- and this is
/// how the game knows that box really does hold the animal that is
/// drawn: the test below compares them. Aiming at the measured model
/// instead was aiming at something the server had never heard of.
///
/// Derived from the part table rather than written down beside it, so it
/// cannot go stale: move a snout forward and this moves with it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn half_extents(species: Species) -> Vec3 {
    let mut half = Vec3::ZERO;
    for part in parts(species).iter().filter(|part| part.gait.shown(false)) {
        for axis in 0..3 {
            let reach = part.at[axis].abs() + part.size[axis] * 0.5;
            half[axis] = half[axis].max(reach * SCALE);
        }
    }
    half
}

/// The model for a species: `assets/models/animals/<species>.bbmodel`,
/// read once (see `logic::models`, and `logic::model_notes` for why each
/// animal is shaped the way it is).
///
/// **The tables that were here are those files now.** Where a comment in
/// this module names `BOAR` or `GULL`, it means that animal's file.
#[inline]
pub fn parts(species: Species) -> &'static [Part] {
    crate::logic::models::animal(species)
}

/// How fast, in blocks a second, a bird has to be going to be drawn in the
/// air.
///
/// **Speed is the one thing the client knows about a flight**: the server
/// sends a position and a facing, and "it is flying" does not cross the
/// wire. A walk tops out at a gull's 1.3 and a grouse's 1.8 (their paces
/// with the seven per cent spread on top); everything a bird does in the air
/// but the last half-block of a landing is several blocks a second. Two and
/// a half sits between with room either side -- and a gull that slows below
/// it in the last moment before touching down folds its wings as it lands,
/// which is what one does. The soundscape hears take-offs off the same line
/// (`soundscape::in_the_air`).
pub const AIRBORNE_SPEED: f32 = 2.6;

/// Wingbeats per block flown: about three a second at a gull's cruise.
/// Tied to distance, like the legs' `STRIDE`, so a bird that is going
/// nowhere is not beating its wings on the spot.
const WINGBEATS_PER_BLOCK: f32 = 0.6;

/// How far a beating wing goes either side of the glide, in radians.
const FLAP: f32 = 0.75;

/// The glide: wings held a little over level. A gull gliding on flat wings
/// is a paper plane.
const GLIDE: f32 = 0.1;

/// **Beats in bouts, glides between them**: `BEAT_BOUT` blocks of beating in
/// every `GLIDE_BOUT` flown, faded in and out so a bout does not begin with
/// a wing snapping up. A bird that beat without stopping would read as a
/// clockwork toy, and one that never beat would be a kite.
const BEAT_BOUT: f32 = 5.0;
const GLIDE_BOUT: f32 = 16.0;

/// Faster than this a bird beats the whole way: it has been frightened, and
/// a frightened bird is not gliding.
const HARD_FLIGHT: f32 = 6.5;

/// How far a spread wing is raised, `walked` blocks into a flight at `speed`.
fn wingbeat(walked: f32, speed: f32) -> f32 {
    let strength = if speed > HARD_FLIGHT {
        1.0
    } else {
        let into = walked.rem_euclid(GLIDE_BOUT);
        if into < BEAT_BOUT {
            (1.3 * (std::f32::consts::PI * into / BEAT_BOUT).sin()).min(1.0)
        } else {
            0.0
        }
    };
    GLIDE + strength * FLAP * (walked * WINGBEATS_PER_BLOCK * std::f32::consts::TAU).sin()
}

/// How far a leg swings, in radians, at a full run.
const SWING: f32 = 0.8;

/// What an animal is doing, as far as the model needs to know.
///
/// **One struct rather than five arguments**, for the reason
/// `player_model::Pose` is one: `walked` and `speed` are both floats, the
/// difference between them is invisible at a call site, and getting them the
/// wrong way round is an animal that skates.
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    /// How far this animal has gone in total, in blocks: the gait's clock,
    /// measured by the client from the snapshots it is already interpolating
    /// between, so a walk cycle costs the server nothing at all. See
    /// `Entity::gait`.
    pub walked: f32,
    /// How fast it is going now, in blocks a second.
    pub speed: f32,
    /// How recently it was struck, 0..1, or `None`. Both the red flash and
    /// the flinch -- see `STAGGER_ROLL`.
    pub hurt: Option<f32>,
    /// How far the head is carried off level, in radians: negative is down.
    ///
    /// **An angle, not the [`Attitude`] it came from.** The attitude arrives
    /// on a snapshot and changes in one step; a head that crossed the whole
    /// way from level to the grass in one frame is the snap that reads as the
    /// model breaking. So the wire's attitude is turned into an angle by
    /// [`head_carried`] and eased toward by `Entity::head`, which is the one
    /// place on the client that keeps anything between two frames -- this
    /// function is handed the result.
    pub head: f32,
    /// How fast it is turning, in radians a second, **positive to its
    /// right** -- the way a growing yaw turns everything else in this game
    /// (`Camera::right_horizontal`).
    ///
    /// **Measured on this client from the two facings it is easing between**
    /// (`Entity::turning`) and eased there (`Entity::banked`), not sent: the
    /// server already turns the animal at a rate the client can see, and a
    /// number that can be derived from two that are already on the wire has
    /// no business being a third.
    pub turning: f32,
    /// Which way a blow throws the body: `+1` to the animal's right, `-1` to
    /// its left. See `STAGGER_ROLL`, and `Entity::flinch_side` for where the
    /// side comes from when the wire does not carry one.
    ///
    /// One by default, which is what every caller that does not care builds
    /// -- and what the flinch did for every animal, every time, before there
    /// was a side at all.
    pub flinch_side: f32,
    /// Seconds this animal has been on screen.
    ///
    /// **The only thing here driven by a clock, and it has to be**, for the
    /// reason `player_model::Pose::age` gives about a person's breath: what a
    /// tail and an ear do is what an animal does when it is *not* moving, so
    /// distance covered -- which is nought -- cannot drive it. Everything a
    /// walk does still runs on `walked`, which is what keeps the legs honest
    /// and what `standing_still_stands_still` holds.
    pub age: f32,
    /// How far from grown it is: nought for an adult, one for a newborn --
    /// `1 - growth`, so that `Motion::default()`, which every caller that
    /// predates the young builds with, is a full-grown animal. Drawn at
    /// `youth::size` of the model, about its middle.
    ///
    /// **Scaled as a whole, not re-proportioned.** A real fawn is mostly leg
    /// and head, and a model for that would be a second model per species
    /// for the few days it lasts. The whole model at a little over half size
    /// reads as young at any distance a player sees one from, and it is the
    /// same box the server collides and the client aims at.
    pub youth: f32,
    /// How far through its death fall it is, nought standing to one lying:
    /// see `FALL_SECONDS`. The body rolls onto its side with its legs gone
    /// stiff and ends in the carcass's own pose (`fallen_pose`), so the block
    /// that replaces it takes over without a jump.
    pub fallen: f32,
    /// What a horse is wearing: `horse::TACK_*` bits, straight off the
    /// snapshot (`EntityKind::Animal::tack`). Nought on every other animal and
    /// on a wild horse, which is what `Motion::default()` draws. See `tack`.
    pub tack: u8,
}

impl Default for Motion {
    /// A full-grown animal standing still, unhurt and square to the world.
    ///
    /// Written out rather than derived for one field: a `flinch_side` of
    /// nought would be a blow that threw the body nowhere, and every test
    /// and tool that builds a struck animal from `..Default::default()`
    /// would quietly stop testing the flinch.
    fn default() -> Self {
        Motion {
            walked: 0.0,
            speed: 0.0,
            hurt: None,
            head: 0.0,
            turning: 0.0,
            flinch_side: 1.0,
            age: 0.0,
            youth: 0.0,
            fallen: 0.0,
            tack: 0,
        }
    }
}

/// How long the client takes to roll a dying animal over, in seconds: a
/// little under `protocol::DEATH_FALL_SECONDS`, which is how long the server
/// keeps the body before the carcass is laid -- so the body is lying still for
/// the last tick or two before the carcass appears, rather than still turning
/// over when it does.
pub const FALL_SECONDS: f32 = primitive_shared::protocol::DEATH_FALL_SECONDS * 0.85;

/// How far a dying animal's legs kick out as it goes over, in radians, at the
/// middle of the fall: the front forward and the back behind.
const STIFF_LEGS: f32 = 0.35;

/// How far the head is carried off level in each attitude, in radians:
/// negative puts the muzzle down.
///
/// **Down is where the food is, and nothing else about an animal says so.**
/// A deer at a river with its head level is a deer standing beside a river.
/// See `protocol::Attitude` for what crosses the wire and why.
///
/// Drinking is lower than feeding because the water is lower than the grass:
/// a drinking animal has reached over the bank. Alert is a little *up*, which
/// is the pose a herd takes between mouthfuls and the one warning a stalking
/// wolf gives. Dozing is halfway down and still. Stalking is a hunter's head
/// carried low and level with its shoulders.
pub fn head_carried(attitude: Attitude) -> f32 {
    match attitude {
        Attitude::Easy => 0.0,
        Attitude::Alert => 0.22,
        Attitude::Feeding => -0.85,
        Attitude::Drinking => -1.05,
        Attitude::Stalking => -0.3,
        Attitude::Dozing => -0.5,
        // Dropped, and let go: the neck gives with the rest of it.
        Attitude::Dying => -0.35,
    }
}

/// How fast the head moves between two attitudes, in radians a second.
///
/// **A head that arrived at the grass in one frame was a glitch, not a bite.**
/// The attitude changes on a snapshot, which lands five times a second, and
/// the whole distance from level to the ground in one of those is the
/// twenty-millisecond snap that reads as the model breaking. Two and a half
/// radians a second puts a deer's nose in the grass in about a third of a
/// second, which is roughly how long a deer takes.
pub const HEAD_RATE: f32 = 2.5;

/// How far a body leans into a turn, in radians per radian a second of turn.
///
/// **A thing that changes direction without leaning is a thing on rails.** A
/// running animal banks into its turn because it has to -- the ground pushes
/// it round and the mass above the feet lags -- and the lean is the one part
/// of that a box model can show. Scaled by how fast it is going as well as by
/// how hard it is turning, because an animal turning on the spot is not
/// leaning anywhere: it is pivoting.
///
/// **Negative, and that is the fix and not a tidy-up.** A roll takes the top
/// of the model toward local `-x` (see `posed_local`), which is the animal's
/// *left*; `Motion::turning` is positive when the yaw grows, which is a turn
/// to its *right*. Multiplying the two together leant every animal in the
/// world out of every corner it took -- a deer banking away from its turn
/// like a car body -- which is what a player saw as "наклоняются в одну
/// сторону": whichever way they went, the lean went the other.
const LEAN_PER_TURN: f32 = -0.16;

/// ...and how far it is allowed to go, in radians. A fifth is enough to read
/// from thirty blocks and little enough that four feet stay on the ground.
const LEAN_MOST: f32 = 0.2;

/// How far a blow throws the body, in radians of roll and blocks of shove.
///
/// **A struck animal flashed red and went on walking.** The flash has been on
/// the wire since anything could be hit (`EntityKind::Animal::hurt`) and it is
/// the whole of what a landed blow looked like: a tint, on a body that did not
/// move. A quarter of a radian away from the blow and a few sixteenths back
/// off it, decaying with the same number the tint decays with, is a flinch --
/// and it costs nothing, because the number was already being sent.
///
/// Away from the blow would need to know which side it came from, which is not
/// on the wire. Sideways is what is left, and it is enough: what an eye reads
/// is that the animal was moved by something, not which way.
///
/// **Which sideways is `Motion::flinch_side`.** It used to be the same side
/// every time, on every animal, for every blow -- so a herd being driven all
/// tipped one way together, and a player hitting the same deer twice watched
/// it lean the same way twice. The side is now drawn per blow on the client
/// that sees it (`Entity::flinch_side`); nothing new crosses the wire.
const STAGGER_ROLL: f32 = 0.26;
const STAGGER_SHOVE: f32 = 0.09;

/// How fast a tail swings when the animal is standing, in swishes a second,
/// and how far, in radians. On the clock rather than on the ground covered --
/// see `Motion::age`, which is what it is for.
///
/// **The tails were welded on.** Every part that is not a leg or a head rides
/// with the body (`Gait::Still`), which is right for a flank and wrong for the
/// one part of an animal that moves when nothing else does. A slow swing at
/// rest and a bounce in time with the stride is most of the difference between
/// a grazing deer and a garden ornament.
const TAIL_SWISHES: f32 = 0.55;
const TAIL_SWING: f32 = 0.22;

/// ...and an ear, which flicks rather than swings: mostly still, with a short
/// turn every few seconds. `EAR_FLICKS` is per second and `EAR_SHARE` is how
/// much of that time the ear is actually moving.
const EAR_FLICKS: f32 = 0.31;
const EAR_SHARE: f32 = 0.18;
const EAR_TURN: f32 = 0.33;

/// Below this, in blocks per second, an animal is standing still.
///
/// **A dead zone, and it is most of the fix for the fidgeting.** An
/// animal that has decided to stop does not stop dead: it has momentum
/// and bleeds it off, so for a second afterwards it is drifting at a
/// tenth of a block a second. With no floor under the swing that second
/// is a full-amplitude walk cycle performed on the spot, which is
/// exactly what "it squirms" describes.
const STANDING: f32 = 0.6;

/// How fast the legs go over, per block travelled.
///
/// Tied to distance rather than to time, which is what stops an animal
/// from moon-walking: something that has stopped has legs that have
/// stopped, and something running has them going twice as fast as
/// something walking without anybody choosing a second number.
const STRIDE: f32 = 2.0;

/// How much faster than `STANDING` an animal has to be going before its legs
/// swing their full amount, in blocks a second: the shoulder on the dead
/// zone. See the `pace` it is used in.
const STANDING_BAND: f32 = 0.5;

/// The whole model, appended to a terrain mesh.
///
/// `walked` is how far this animal has gone in total and `speed` is how
/// fast it is going now -- both measured by the client from the
/// snapshots it is already interpolating between, so a walk cycle costs
/// the server nothing at all. See `Entity::gait`.
#[allow(clippy::too_many_arguments)]
pub fn build(
    species: Species,
    centre: Vec3,
    yaw: f32,
    motion: Motion,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    build_parts(parts(species), species, centre, yaw, motion, layers, light, vertices, indices);
}

/// `build`, for a model that is not necessarily the one loaded for the
/// species: a test holding a model read from an edited file beside the
/// shipped one draws both through here, so what it compares is what the
/// game would draw.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_parts(
    model: &[Part],
    species: Species,
    centre: Vec3,
    yaw: f32,
    motion: Motion,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    let Motion { walked, speed, hurt, head, turning, flinch_side, age, youth, fallen, tack } = motion;
    // A sheep's coat off (`horse::TACK_SHORN`), read before the byte is
    // narrowed to the horse's: it is the one bit that means something on a
    // sheep.
    let shorn = species == Species::Sheep && tack & TACK_SHORN != 0;
    // Only a horse wears anything; a stray bit on another animal draws nothing.
    let tack = if species == Species::Horse { tack } else { 0 };
    // The death fall, eased so it goes slowly at first and hits the ground.
    // See `Motion::fallen`.
    let falling = fallen.clamp(0.0, 1.0);
    let falling = falling * falling;
    // ...and a dead animal walks nowhere, whatever speed the snapshots say it
    // is sliding at.
    let speed = if falling > 0.0 { 0.0 } else { speed };
    // **Two things keep the legs honest**, and each one was a way the
    // animals looked wrong.
    //
    // The dead zone: an animal bleeding off its momentum is not walking,
    // and without a floor it performs a full stride while drifting to a
    // halt. The amplitude: legs at a crawl swing as far as legs at a
    // gallop unless the swing is scaled by how fast the animal is
    // actually going. And the third is not here at all -- it is the
    // server refusing to let a stopped animal keep creeping, which is
    // what put it in the dead zone in the first place.
    //
    // **The dead zone has a shoulder on it**, and that was the second half
    // of the twitch. A hard edge at `STANDING` meant the legs went from a
    // third of a stride to nothing between two frames every time an animal
    // set off or pulled up -- a snap, on the one part of the model a player
    // is watching. Over the sixteenth of a block a second above the floor,
    // the swing is faded in instead. Nothing creeps through: the server
    // stops a standing animal dead (`walk`'s "an animal that has decided to
    // stand still stands still"), so the band is crossed by something that
    // really is setting off.
    let pace = if speed < STANDING {
        0.0
    } else {
        let eased = ((speed - STANDING) / STANDING_BAND).clamp(0.0, 1.0);
        let eased = eased * eased * (3.0 - 2.0 * eased);
        (speed / 4.0).clamp(0.35, 1.0) * eased
    };
    let swing = SWING * pace;
    let phase = walked * STRIDE;
    // **A bird in the air is a different drawing**: wings spread and
    // beating, legs tucked. See `GULL` and `AIRBORNE_SPEED`.
    let aloft = species.flies() && speed > AIRBORNE_SPEED;
    let flap = if aloft { wingbeat(walked, speed) } else { 0.0 };
    // **The lean.** A turn is a bank, and only while there is speed to bank
    // with: see `LEAN_PER_TURN`.
    let lean = (turning * LEAN_PER_TURN * pace).clamp(-LEAN_MOST, LEAN_MOST);
    // **The flinch.** See `STAGGER_ROLL`: the number was already on the wire
    // for the red tint, and this is the body moving with the blow.
    let flash = hurt.unwrap_or(0.0);
    // Which way this blow threw it -- see `Motion::flinch_side`. Taken as a
    // sign rather than trusted as a number, so nothing off the wire can roll
    // an animal further than a blow is allowed to.
    let side = if flinch_side < 0.0 { -1.0 } else { 1.0 };
    let standing = Pose {
        roll: lean + STAGGER_ROLL * flash * side,
        // Sideways along the flinch, so the body is shoved rather than
        // pivoted on the spot. Negative because `Pose::shift` is taken *off*
        // every local point -- see its own note.
        shift: Vec3::new(-STAGGER_SHOVE * flash * side, 0.0, 0.0),
        scale: primitive_shared::youth::size(1.0 - youth),
    };
    // **Going down: from the standing pose to the carcass's**, roll and shift
    // together. The carcass is placed by its underside on the ground and the
    // living by their middle, so the lying pose is lifted by half the height
    // to be placed from the same centre -- which is what makes the last frame
    // of the fall the carcass the mesher will draw in its place.
    let pose = if falling > 0.0 {
        let rest = fallen_pose(species);
        let lying = rest.shift + Vec3::Y * (species.height() * 0.5);
        Pose {
            roll: standing.roll + (rest.roll - standing.roll) * falling,
            shift: standing.shift + (lying - standing.shift) * falling,
            scale: standing.scale,
        }
    } else {
        standing
    };

    // A nod, at the *same* rate as the legs and a fraction of the amplitude.
    // It used to run at twice the rate, which is not a nod, it is a tic: a
    // head bobs once per stride, because it is the stride that bobs it.
    //
    // ...over wherever the attitude is carrying the head. The two add because
    // they are two different things happening to one neck: a grazing animal
    // that takes a step still bobs, and a head that has reached the grass has
    // stopped bobbing already, because it has stopped walking and `swing` is
    // zero. Named once because a halter nods with the head it is on (`tack`).
    let nod = head + phase.sin() * swing * 0.10;

    for part in model {
        if !part.gait.shown(aloft) {
            continue;
        }
        // The herd's stallion: the same boxes, a heavier neck and mane. See
        // `stallion`.
        let heavier;
        let part = if tack & TACK_STALLION != 0 {
            heavier = stallion(part);
            &heavier
        } else if shorn && part.name == FLEECE_PART {
            heavier = cropped(part);
            &heavier
        } else {
            part
        };
        // The cropped fleece wears the hide under the wool, as the carcass's
        // second stage does (`Dressing::Hide`): one look for "the wool is off"
        // alive and dead.
        let dressing = if shorn && part.name == FLEECE_PART { Dressing::Hide } else { Dressing::Coat };
        let angle = match part.gait {
            Gait::Folded => 0.0,
            // Not running on nothing, and not nodding to a stride it is not
            // taking: a bird's head in the air is the steadiest part of it.
            Gait::LegFront | Gait::LegBack | Gait::Head if aloft => 0.0,
            Gait::Wing(_) => flap,
            // Stiff, when it is dead: the stride goes, the legs kick out as it
            // goes over and lock straight as it lands -- straight, because
            // that is how the carcass lies (`fallen_pose`, every swing
            // nought), and a body that landed splayed would snap its legs in
            // when the block took over.
            Gait::LegFront => phase.sin() * swing * (1.0 - falling) + STIFF_LEGS * (std::f32::consts::PI * falling).sin(),
            Gait::LegBack => -phase.sin() * swing * (1.0 - falling) - STIFF_LEGS * (std::f32::consts::PI * falling).sin(),
            // See `nod`.
            Gait::Head => nod,
            // **Still, unless the model named it something that is not.** See
            // `secondary`: a tail and an ear are the two parts that move when
            // nothing else does, and the files already say which is which.
            Gait::Still => secondary(part.name, age, phase, swing),
        };
        append_part_posed(
            part, species, centre, yaw, angle, pose, dressing, hurt, layers, light,
            vertices, indices,
        );
    }

    if tack & (TACK_SADDLE | TACK_BAGS | TACK_HALTER) != 0 {
        append_tack(model, tack, centre, yaw, nod, pose, hurt, layers, light, vertices, indices);
    }
}

/// How far the top of a saddle's seat is over the horse's back, in
/// sixteenths: the leather over the pad.
const SADDLE_SEAT: f32 = 1.4;

/// How high the top of a saddle's seat is, in blocks over a grown horse's
/// feet: what a rider sits on.
///
/// **Measured off the loaded model, not written down beside it**, for the
/// reason `half_extents` is: the saddle is laid on the top of the horse's
/// `body` box (`append_tack`), so a horse made taller in Blockbench carries
/// its saddle up with it -- and the rider, whose seat this is
/// (`player_model::mount_drop`), goes up with the saddle rather than being
/// left sitting in the horse's back.
pub fn saddle_top() -> f32 {
    let body = parts(Species::Horse).iter().find(|part| part.name == "body");
    let back = body.map_or(0.0, |body| body.at[1] + body.size[1] * 0.5);
    (back + SADDLE_SEAT) * SCALE + Species::Horse.height() * 0.5
}

/// The herd stallion's version of a part: the same box, heavier where a
/// stallion is heavier.
///
/// **Scaled in code rather than a second model file**, because what makes
/// a stallion is two proportions and not a shape: a crest of neck a
/// sixteenth wider and a mane half again as thick and standing higher. A
/// `horse_stallion.bbmodel` would be twenty-three boxes copied to change
/// seven of them, and the day the mare's legs are moved the stallion's
/// would stay where they were. Rejected too: **a darker picture**, which is
/// a second sheet and a layer for a difference the bay's mane already
/// makes black -- at forty blocks what a player picks out is the outline.
fn stallion(part: &Part) -> Part {
    let mut heavier = *part;
    if part.name.starts_with("mane") || part.name == "forelock" {
        heavier.size[0] *= 1.6;
        // Grown upward only -- its foot stays in the crest -- and by a whole
        // sixteenth, because by less its top lands in the plane of the neck
        // box it stands on.
        heavier.size[1] += 1.0;
        heavier.at[1] += 0.5;
    } else if part.name == "neck" || part.name == "neck middle" {
        // The base and the middle of the neck, where a stallion's crest is.
        // Not the top of it: the upper neck is a fifth of a sixteenth inside
        // the skull's sides, and grown it would be in their plane.
        heavier.size[0] += 0.8;
    }
    heavier
}

/// The sheep's part that is its fleece (`assets/models/animals/sheep.bbmodel`):
/// the body box, which the legs and the head hang off.
const FLEECE_PART: &str = "body";

/// A shorn sheep's body: the fleece's box close-cropped -- a sixteenth off
/// each flank and off the back and the belly, and nothing off its length.
///
/// **Not off its length**, because the head is hung at the body's front and
/// the tail at its back: a shorter box opened a gap at the neck. And from the
/// belly as well as the back, so more of the legs shows under it -- which is
/// what a sheep just sheared looks like, and what tells one from a ewe in full
/// fleece across a pen before the colour does.
fn cropped(part: &Part) -> Part {
    let mut bare = *part;
    bare.size[0] -= 2.0;
    bare.size[1] -= 2.0;
    bare
}

/// What a piece of tack is made of.
#[derive(Clone, Copy)]
enum Tack {
    /// The cured skin of the hide frame (`Material::Leather`), cut from
    /// inside its laced margin -- see `append_tack`.
    Leather,
    /// A saddle pad: the wool block's own picture.
    Fleece,
    /// Stirrups, bit rings and buckles.
    Iron,
}

/// One box of tack, in sixteenths, as `[x0, y0, z0]..[x1, y1, z1]` in the
/// horse's own frame (its centre, -Z forward), and what it is made of.
struct Strap {
    name: &'static str,
    from: [f32; 3],
    to: [f32; 3],
    of: Tack,
    /// Rides the head -- nods with it, about the same joint -- rather than
    /// the body.
    on_head: bool,
}

/// A strap, and the same strap on the horse's other side.
fn both_sides(out: &mut Vec<Strap>, strap: Strap) {
    let mirrored = Strap { from: [-strap.to[0], strap.from[1], strap.from[2]], to: [-strap.from[0], strap.to[1], strap.to[2]], ..strap };
    out.push(strap);
    out.push(mirrored);
}

/// The tack on a horse, from the bits of `horse::TACK_*` it is wearing.
///
/// **Laid on the model's own boxes, not on numbers of its own.** The
/// saddle's height is the top of `body`, its skirts are the width of it,
/// the halter goes round whatever `muzzle` and `head` are -- so a horse
/// reshaped in Blockbench keeps its saddle on its back and its halter on
/// its face. Along the body the saddle is fixed at the middle, because the
/// middle is where the server puts the rider (`horse::RIDER_LIFT`, at the
/// horse's own x and z) whatever shape the horse is.
///
/// **Drawn in code rather than as more boxes in the model file**, toggled by
/// group name the way `loaded` and `bare` toggle a rack's skin. Rejected,
/// because the file's pictures are the *animal's* sheet: a saddle in it would
/// have to be painted into the horse's squares -- a leather tile in the slot
/// the sheet calls `Tusk` -- which is the confusion `Skin` exists to stop,
/// and what the tack is made of is already in the atlas as the hide frame's
/// leather, the wool block and the furniture's iron. Rejected too: **tack
/// as a second entity** riding the horse, which is a snapshot and an
/// interpolation for boxes that never leave it.
///
/// Every face kept clear of every other it could lie flush with by a fifth
/// of a sixteenth or more, for `model_overlap`'s reason: a skirt flush with
/// the pad under it is two pictures the depth buffer picks between.
#[allow(clippy::too_many_arguments)]
fn append_tack(
    model: &[Part],
    tack: u8,
    centre: Vec3,
    yaw: f32,
    nod: f32,
    pose: Pose,
    hurt: Option<f32>,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::engine::mesh::Material;
    let named = |name: &str| model.iter().find(|part| part.name == name);
    let (Some(body), Some(head), Some(muzzle)) = (named("body"), named("head"), named("muzzle")) else {
        return;
    };
    let top = |part: &Part| part.at[1] + part.size[1] * 0.5;
    let bottom = |part: &Part| part.at[1] - part.size[1] * 0.5;
    let back = top(body);
    let belly = bottom(body);
    let flank = body.size[0] * 0.5;
    let mut straps = Vec::new();
    let strap = |name, from, to, of| Strap { name, from, to, of, on_head: false };

    if tack & TACK_SADDLE != 0 {
        // The seat, with a pommel before it and a higher cantle behind: the
        // three boxes that make a saddle rather than a blanket.
        straps.push(strap("seat", [-3.6, back - 0.2, -4.0], [3.6, back + SADDLE_SEAT, 3.6], Tack::Leather));
        straps.push(strap("pommel", [-2.2, back + 1.0, -4.5], [2.2, back + 2.5, -3.1], Tack::Leather));
        straps.push(strap("cantle", [-2.8, back + 1.0, 2.4], [2.8, back + 2.9, 3.9], Tack::Leather));
        // The pale fleece under it, showing at the edges: without it a
        // leather saddle on a bay horse is brown on brown.
        straps.push(strap("pad", [-flank - 0.4, back - 1.5, -5.0], [flank + 0.4, back + 0.5, 4.6], Tack::Fleece));
        // The girth, round the barrel behind the forelegs.
        straps.push(strap("girth", [-flank - 0.2, belly - 0.2, -2.6], [flank + 0.2, back - 1.0, -1.4], Tack::Leather));
        both_sides(&mut straps, strap("skirt", [-flank - 0.7, back - 6.0, -3.8], [-flank + 0.15, back + 0.25, 2.8], Tack::Leather));
        // The stirrups hang whether anybody is in them: an empty saddle
        // with its irons down is a horse waiting for somebody. With a rider
        // on, each iron is under a boot -- where the rider's own model says
        // the sole is (`player_model::rider_foot`), out beside the flank
        // where the leg has to be to clear the barrel -- and not down
        // against the horse's side, a hand's breadth from the foot.
        both_sides(&mut straps, strap("stirrup leather", [-flank - 0.95, back - 7.5, -0.3], [-flank - 0.7, back - 0.5, 0.3], Tack::Leather));
        if tack & TACK_RIDDEN != 0 {
            let sole = crate::logic::player_model::rider_foot() / SCALE;
            let lift = (primitive_shared::horse::RIDER_LIFT - Species::Horse.height() * 0.5) / SCALE;
            let (x, y, z) = (sole.x, sole.y + lift, sole.z);
            both_sides(&mut straps, strap("stirrup", [-x - 0.9, y - 0.7, z - 1.0], [-x + 0.9, y + 0.2, z + 1.0], Tack::Iron));
        } else {
            both_sides(&mut straps, strap("stirrup", [-flank - 1.75, back - 8.3, -1.0], [-flank - 0.45, back - 7.3, 1.0], Tack::Iron));
        }
    }
    if tack & TACK_BAGS != 0 {
        // Behind the saddle, over the loins, where a pack animal carries
        // its load -- and where a rider's legs are not.
        both_sides(&mut straps, strap("bag", [-flank - 2.1, back - 7.0, 4.2], [-flank + 0.2, back - 1.0, 9.6], Tack::Leather));
        both_sides(&mut straps, strap("bag lid", [-flank - 2.3, back - 2.6, 4.0], [-flank - 0.1, back - 0.6, 9.8], Tack::Leather));
        both_sides(&mut straps, strap("bag buckle", [-flank - 2.55, back - 3.6, 6.5], [-flank - 2.2, back - 2.4, 7.3], Tack::Iron));
        straps.push(strap("bag strap", [-flank - 0.3, back - 1.0, 6.2], [flank + 0.3, back + 1.0, 7.6], Tack::Leather));
    }
    if tack & TACK_HALTER != 0 {
        let head_strap = |name, from, to, of| Strap { name, from, to, of, on_head: true };
        let (mw, hw) = (muzzle.size[0] * 0.5, head.size[0] * 0.5);
        let muzzle_front = muzzle.at[2] - muzzle.size[2] * 0.5;
        let head_front = head.at[2] - head.size[2] * 0.5;
        let head_back = head.at[2] + head.size[2] * 0.5;
        // Across the upper third of the long face, and half a sixteenth
        // clear of the skull's underside, whose plane the cheek strap's own
        // underside would otherwise lie in.
        let cheek = top(muzzle) - 1.6;
        // A band round the nose, a strap up each cheek, and one over the
        // poll behind the ears: what reads as "somebody's horse" across a
        // field, which is the halter's whole job (`horse::TACK_HALTER`).
        straps.push(head_strap(
            "noseband",
            [-mw - 0.25, bottom(muzzle) - 0.25, muzzle_front + 2.3],
            [mw + 0.25, top(muzzle) + 0.25, muzzle_front + 3.3],
            Tack::Leather,
        ));
        both_sides(&mut straps, head_strap("nose cheek", [-mw - 0.2, cheek, muzzle_front + 3.3], [-mw + 0.15, cheek + 0.7, head_front], Tack::Leather));
        both_sides(&mut straps, head_strap("cheek", [-hw - 0.15, cheek, head_front + 0.2], [-hw + 0.2, cheek + 0.7, head_back - 1.0], Tack::Leather));
        straps.push(head_strap(
            "headpiece",
            [-hw - 0.35, bottom(head) - 0.2, head_back - 1.0],
            [hw + 0.35, top(head) + 0.3, head_back - 0.2],
            Tack::Leather,
        ));
        if tack & TACK_RIDDEN != 0 {
            // Reins from the rings of the noseband back along the neck to
            // where a rider's hands are, over the withers.
            both_sides(&mut straps, head_strap("bit ring", [-3.35, cheek - 0.1, muzzle_front + 1.9], [-mw - 0.2, cheek + 0.7, muzzle_front + 2.7], Tack::Iron));
            both_sides(&mut straps, head_strap("rein", [-3.15, cheek + 0.1, muzzle_front + 2.3], [-2.95, cheek + 0.5, body.at[2] - body.size[2] * 0.5 + 0.5], Tack::Leather));
        }
    }

    // The head's own joint, so the halter nods exactly as the head does.
    let head_joint = head.pivot.unwrap_or([top(head), head.at[2]]);
    let (sky, block_light) = light;
    let block_light = match hurt {
        Some(flash) => block_light.max((primitive_shared::types::MAX_LIGHT as f32 * flash) as u8),
        None => block_light,
    };
    let leather = Material::Leather.layer(layers);
    let fleece = Material::Wool.layer(layers);
    let iron = Material::Iron.layer(layers);
    for strap in &straps {
        let size = std::array::from_fn(|a| strap.to[a] - strap.from[a]);
        let part = Part {
            name: strap.name,
            at: std::array::from_fn(|a| (strap.from[a] + strap.to[a]) * 0.5),
            size,
            gait: if strap.on_head { Gait::Head } else { Gait::Still },
            pivot: strap.on_head.then_some(head_joint),
            ..PART
        };
        let swing = if strap.on_head { nod } else { 0.0 };
        push_posed_box(
            &part,
            centre,
            yaw,
            swing,
            pose,
            |face| {
                let [_, _, du, dv] = material_cut(size, face);
                match strap.of {
                    // **From inside the laced margin**: the cured skin's
                    // picture is drawn for a whole slab, cords and all, and
                    // `material_cut`'s piece from the corner is the cord --
                    // a girth one sixteenth wide was a dark line of lacing.
                    // Two texels in, and never more than the twelve the
                    // margin leaves.
                    Tack::Leather => (leather, Some([2.0 / 16.0, 2.0 / 16.0, du.min(0.75), dv.min(0.75)])),
                    Tack::Fleece => (fleece, Some([0.0, 0.0, du, dv])),
                    Tack::Iron => (iron, Some([0.0, 0.0, du, dv])),
                }
            },
            (sky, block_light),
            vertices,
            indices,
        );
    }
}

/// What a part that rides with the body does anyway, in radians.
///
/// **Read off the part's own name, and that is deliberate.** A tail and an ear
/// are one box each in twenty-odd model files, and giving them a gait of their
/// own would mean re-exporting every one of those files to record a fact the
/// name in them already records -- a twenty-file diff nobody can review, for
/// two numbers. `Part::name` was carried for people and for the tests and read
/// by nothing that draws; now one thing reads it, and what that costs is a
/// couple of byte compares per box, beside the twenty-four vertices each box
/// is about to push.
///
/// Rejected: **a second pair of gaits in the file format**, above. Rejected
/// too: **moving everything that is not a leg**, which is a flank that wobbles.
fn secondary(name: &str, age: f32, phase: f32, swing: f32) -> f32 {
    if name.contains("tail") {
        // A bounce in time with the stride while it walks, and a slow swish
        // underneath that carries on when it stops -- which is the half of it
        // that needed a clock. See `Motion::age`.
        return phase.sin() * swing * 0.35
            + (age * TAIL_SWISHES * std::f32::consts::TAU).sin() * TAIL_SWING;
    }
    if name.contains("ear") {
        // **A flick, not a swing.** An ear that waved continuously is a fly on
        // the animal's head; what one does is stand still and then turn once,
        // quickly. So: for `EAR_SHARE` of each cycle the ear moves through
        // half a sine, and for the rest of it there is nothing at all.
        let into = (age * EAR_FLICKS).rem_euclid(1.0);
        if into < EAR_SHARE {
            return (std::f32::consts::PI * into / EAR_SHARE).sin() * EAR_TURN;
        }
        return 0.0;
    }
    0.0
}

/// One box, swung about its own top, turned to face the animal's yaw,
/// and put where the animal is.
///
/// The order matters and is the usual one: **swing, then turn, then
/// place**. Turning before swinging would swing a leg sideways.
/// Which of the six world directions a model face points once the part
/// has been swung and the animal turned.
///
/// The same two rotations `append_part` applies to a corner, applied to
/// the face's own normal, and then snapped to the nearest axis --
/// because the light word has room for six directions and a yaw is
/// continuous. See the note at the call site.
/// The animal lying on its side where it fell: the carcass.
///
/// **The same model, rolled over, not a box with a fur texture.** A
/// carcass was first drawn as a low cube, and it read as a crate; what
/// a player recognises on the ground is the animal, so the animal is
/// what is drawn -- the very parts `build` walks around on, rolled a
/// quarter turn about their own length so the flank faces the sky and
/// the legs stick out sideways. Baked into the chunk mesh by the mesher
/// (`mesh::build_mesh`), so it is lit by the chunk's light, culled with
/// the chunk, and costs nothing per frame -- which is right for a thing
/// that never moves again.
///
/// Placed by its underside: the rolled model's lowest point sits a hair
/// above `ground` (so it does not fight the ground's top face for the
/// same pixels) and the middle of its footprint on the cell's centre.
/// The yaw is the caller's -- the mesher hashes it from the cell, so a
/// field of kills does not line up like a parade.
///
/// `stage` is how many cuts have been made (`animals::butchering_stage`):
/// once the skin is off the parts wear flesh, and a fleeced sheep wears
/// its hide for the one cut between. What is drawn is what is left.
#[allow(clippy::too_many_arguments)]
pub fn build_fallen(
    species: Species,
    ground: Vec3,
    yaw: f32,
    stage: usize,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    let dressing = match (species, stage) {
        (_, 0) => Dressing::Coat,
        (Species::Sheep, 1) => Dressing::Hide,
        _ => Dressing::Flesh,
    };
    let pose = fallen_pose(species);
    // **A bird lies with its wings folded**: the pair it has on the ground,
    // not both. Every part used to be drawn, so a dead gull or grouse wore
    // its spread wings through its folded ones -- four wings, the two pairs
    // pressed into each other face to face (`model_overlap` found them).
    for part in parts(species).iter().filter(|part| part.gait.shown(false)) {
        append_part_posed(part, species, ground, yaw, 0.0, pose, dressing, None, layers, light, vertices, indices);
    }
}

/// The skeleton a carcass nobody came back for turns into, lying where the
/// animal fell.
///
/// Split from `build_fallen` rather than folded into it as a fourth
/// stage, because it is not a stage: butchering walks a carcass through
/// `stage`, and a skeleton is what happens to one nobody butchered at
/// all. Same cell, same yaw hash, so it lies along the line the carcass
/// lay along with its skull at the same end.
///
/// **On its side, the way the carcass lay -- built lying, not rolled.**
/// The bones are laid out in the carcass's own rolled frame: legs out to
/// +X, the right flank to the sky, the skull toward -Z. So a carcass that
/// rots into a skeleton does not turn over as it goes; the legs stay out
/// on the side they lay on. They are built in that frame
/// (`skeleton_parts`) and placed with no roll of their own, because a
/// quarter roll of bones built standing lays the ribcage's edge to the
/// sky and stacks the legs one on another -- which is why the skeleton
/// before this lay on its belly instead, splayed like a frog.
#[allow(clippy::too_many_arguments)]
pub fn build_bones(
    species: Species,
    ground: Vec3,
    yaw: f32,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    let bones = skeleton_parts(species);
    let pose = resting_pose(&bones, 0.0, |_| 0.0);
    for part in &bones {
        append_part_posed(part, species, ground, yaw, 0.0, pose, Dressing::Bones, None, layers, light, vertices, indices);
    }
}

/// The least distance, in sixteenths, between two faces of a skeleton
/// that point the same way and cover one another.
///
/// **A skeleton is nothing but joints, and every joint is a place for two
/// surfaces to fight.** Where a rib meets the spine, where a rib's side
/// meets its arch, where an antler roots in the skull, one bone runs
/// *into* another -- and if a face of each lands in the same plane, the
/// depth buffer picks a winner by rounding and changes its mind as the
/// camera moves. `SEAM_BITE` settles it for the living model, whose boxes
/// only *touch*; it cannot settle it here, because it grows both boxes by
/// the same amount and two coplanar faces stay coplanar.
///
/// So the bones are built so that it never arises: a bone that runs into
/// another is thinner than it by `NEST` and set inside it, which puts
/// every face of the thin one at least this far from the thick one's. Measured
/// against the depth buffer rather than chosen by eye: `Depth32Float`,
/// a near plane of 0.05 and ordinary z resolve two surfaces
/// `d² × 2⁻²⁴ / 0.05` blocks apart at `d` blocks, which is 0.005 at
/// sixty-four -- and this is 0.0094, so the thin bone stays buried at
/// every distance a skeleton is more than a few pixels at.
pub(crate) const CLEARANCE: f32 = 0.15;

/// How much thinner a bone is than the one it runs into: a `CLEARANCE`
/// on either side.
const NEST: f32 = 2.0 * CLEARANCE;

/// The gap between two bones that are *not* grown into each other --
/// neighbouring vertebrae, the two halves of a limb, the segments of a
/// tail.
///
/// **A gap rather than a joint, on purpose.** The ligaments are the first
/// thing to go, and a skeleton whose long bones run unbroken from
/// shoulder to toe is a stick figure; a hair of daylight between them is
/// what makes a limb read as *bones*. It is also the cheapest joint there
/// is: two boxes that do not touch cannot share a plane.
const JOINT: f32 = 0.25;

/// One bone. Always the same material -- `Dressing::Bones` dresses every
/// face in it -- so the skin named here is only for a person reading the
/// list: bone is ivory.
fn bone(name: &'static str, at: [f32; 3], size: [f32; 3]) -> Part {
    Part {
        name,
        at,
        size,
        skin: Skin::Tusk,
        ..PART
    }
}

/// How a skeleton's legs lie on the ground: the turn of the upper bone and
/// of the lower one, in radians, positive toward the skull (see
/// `Part::turn`). The lower bone turns a quarter-radian further than the
/// upper, which is the bend at the knee or the hock -- enough to read as a
/// joint, not so much that the leg folds.
///
/// **Forward for a foreleg and back for a hind one, by about a sixth of a
/// right angle.** That is how an animal that died lying down lies: legs
/// stretched out from the belly, the front pair toward the head and the
/// back pair toward the tail. Square to the body is a table's legs, and a
/// leg turned most of the way along the spine is the splay this replaced,
/// seen from the other side.
const FORELEG_TURNS: (f32, f32) = (0.3, 0.55);
const HIND_LEG_TURNS: (f32, f32) = (-0.3, -0.55);

/// The bird's lower wing, pinned under the body and lying half open on the
/// ground behind its back: pointing away from the belly and swept back
/// toward the tail, the hand further back than the arm.
const OPEN_WING_TURNS: (f32, f32) = (-(std::f32::consts::PI - 0.35), -(std::f32::consts::PI - 0.75));

/// A bone lying on the ground, from `from` (x, z) and `length` long along
/// `turn`.
fn stick(name: &'static str, from: [f32; 2], turn: f32, length: f32, thick: f32) -> Part {
    let (sin, cos) = turn.sin_cos();
    Part {
        turn,
        ..bone(
            name,
            [from[0] + cos * length * 0.5, thick * 0.5, from[1] - sin * length * 0.5],
            [length, thick, thick],
        )
    }
}

/// One limb of a skeleton lying on the ground: an upper bone from `from`
/// (x, z) along the first turn, and a thinner lower bone from the knee
/// along the second.
///
/// **The knee is a gap, and the gap grows with the bend.** Two boxes at
/// different turns with only a `JOINT` between their ends cross at the
/// corner on the inside of the bend -- it swings back toward the upper bone
/// by half a width times the sine of the bend -- and where they cross,
/// their tops are a fifth of a thickness apart, which is inside
/// `CLEARANCE`. A thickness times that sine more keeps the corner out.
fn lay_limb(
    bones: &mut Vec<Part>,
    names: (&'static str, &'static str),
    from: [f32; 2],
    reach: f32,
    thick: f32,
    turns: (f32, f32),
) {
    let (upper, lower) = (reach * 0.45, reach * 0.55);
    let thin = (thick * 0.8).max(0.4);
    let knee = upper + JOINT + thick * (turns.1 - turns.0).sin().abs();
    let (sin, cos) = turns.0.sin_cos();
    bones.push(stick(names.0, from, turns.0, upper, thick));
    bones.push(stick(names.1, [from[0] + cos * knee, from[1] - sin * knee], turns.1, lower, thin));
}

/// The bones of an animal, as boxes, lying on its side the way its carcass
/// lay: y is up from the ground the skeleton lies on, -Z is toward the
/// skull as in the living model, and **+X is the belly side** -- the side
/// the legs lie out on. That is the frame `fallen_pose` rolls the living
/// model into (its legs go to +X, its back to -X and its right flank to
/// the sky), so the bones lie the way the carcass lay.
///
/// **What the player reported, and why it was true.** The skeleton used
/// to be cut out of the living model's own boxes: the head shrunk by a
/// fifth and called a skull, the muzzle narrowed and called a jaw, every
/// leg kept its length and lost two thirds of its thickness, and four
/// planks hung down each flank for ribs -- all of it in the rolled
/// carcass's pose, wearing the bone *item's* picture. What lay in the
/// grass was the animal's body parts with a bone texture on each one,
/// which is exactly how it was described. Shrinking a head does not make
/// a skull; it makes a smaller head.
///
/// So nothing here is a living part made smaller. The living model is
/// **measured** -- how wide and deep and long the body is, how long the
/// head and the muzzle, how long and thick the legs, whether there are
/// tusks or antlers or a tail -- and a skeleton is **built** from those
/// numbers out of bone-sized pieces.
///
/// **...and then that it lay like a frog.** The first skeleton built from
/// bones lay on its belly with a limb splayed out to either side at the
/// shoulder and at the hip, forelegs bent forward and hind legs back.
/// Every bone in it was right and the pose was not, and the pose is what
/// the eye reads first: nothing four-legged dies spread-eagled, but a frog
/// sits that way. It also turned the carcass over as it rotted -- a
/// carcass on its side with its legs out one way became bones on their
/// belly with legs out both. It had been laid on its belly for a reason
/// that was real: a skeleton built standing and *rolled* onto its side
/// shows the edge of its ribcage and its legs stacked one on another. The
/// answer to that is to build the bones lying on their side, not to roll
/// a standing frame, and that is what this does:
///
/// * a **spine** of separate vertebrae lying along the ground on the back
///   side, each with its spinous process pointing away from the belly;
/// * a **ribcage** on its side: each rib a post up from the spine, an arch
///   over the upper flank and a post down to the ground at the breastbone,
///   the middle ribs the tallest so the cage is a barrel and not a crate,
///   with daylight between one rib and the next;
/// * a **pelvis** on its side: the upper hip bone a plate over the loins,
///   on a post beside the spine and a seat bone at the hip socket;
/// * a **neck** along the ground to a **skull** on its side -- built
///   upright and turned a quarter the way the body is, so its eye socket
///   looks at the sky: a cranium, a brow and a cheekbone with the socket
///   open between them, a narrower snout, a lower jaw of two bars with the
///   mouth open -- and whatever that species carries on its head in bone,
///   tusks and antlers, where they grew;
/// * **legs in parallel pairs, flat on the ground and all out from the
///   belly**, two bones each with a gentle bend: forelegs angled forward,
///   hind legs back (`FORELEG_TURNS`). The bird has its two legs the same
///   way, one wing folded along its upper flank and the other half open on
///   the ground behind its back.
///
/// **Derived rather than drawn per species**, for the reason the models
/// are a table at all: ten hand-made skeletons are ten more things to
/// keep in step with the living shape, and a deer whose skeleton is a
/// boar's size is a skeleton of the wrong animal. Measured from the same
/// table, a bear's skeleton is a bear's size and a hare's is a hare's.
///
/// Thirty-two boxes for the bird to fifty for the deer, where the living
/// models are six to fifteen -- baked once into a chunk and never touched
/// again, and a skeleton is a landmark a meadow has one of, not a flock.
pub fn skeleton_parts(species: Species) -> Vec<Part> {
    let living = parts(species);
    let named = |name: &str| living.iter().find(|part| part.name == name);
    let body = named("body").expect("every animal has a body");
    let head = named("head").expect("every animal has a head");
    let muzzle = living
        .iter()
        .find(|part| matches!(part.name, "muzzle" | "snout" | "beak"))
        .expect("every animal has a muzzle");
    let [width, height, length] = body.size;
    let front = body.at[2] - length * 0.5;
    let back = body.at[2] + length * 0.5;
    let mut bones = Vec::new();

    // **How stout this animal's bones are**, from the smaller of its
    // body's two cross-section sides: a bear's ribs are three times a
    // hare's. The floors hold the thinnest rib at three tenths of a
    // sixteenth and its arch at six, which is the hare's: about a pixel
    // at four blocks in a 512-pixel picture. Thinner, and a rib drops in
    // and out of the picture as the camera moves.
    let stout = (0.12 * width.min(height)).clamp(0.5, 1.5);
    let rib = (0.6 * stout).max(0.3);
    let arch = rib + NEST;
    let spine = arch + NEST;
    // **The spinous process stands out of the vertebra by more than the
    // clearance.** On the belly skeleton it once sat flush, `CLEARANCE`
    // proud of the arches: never a fight, but a step of a hundredth of a
    // block, which at three blocks is under a pixel and came out as a
    // dotted seam. Half an arch is an edge, not a stipple -- and on a
    // skeleton on its side it is the row of knuckles along the back.
    let process = (0.5 * arch).max(NEST);
    // **The chest on its side.** Its depth, back to breastbone, lies along
    // the ground, and a little over half the living body's depth is left
    // once it has fallen in. Its width now stands up, and a little over
    // half of that is what rises above the ground.
    let cage_depth = height * 0.55;
    let cage_rise = width * 0.55;

    // Along the body, back to front: the pelvis over the hind quarter,
    // the ribs over what is in front of it.
    let pelvis_length = length * 0.22;
    let pelvis_back = back - length * 0.05;
    let pelvis_front = pelvis_back - pelvis_length;
    let hip = pelvis_back - pelvis_length * 0.5;
    let cage_front = front + length * 0.06;
    let cage_back = pelvis_front - rib;
    let span = cage_back - cage_front - arch;
    // **As many ribs as leave a rib's width of daylight between two of
    // them**, and never fewer than three pairs -- two is a pair of hoops,
    // not a ribcage.
    let ribs = ((span / (2.0 * arch)).floor() as usize + 1).clamp(3, 6);
    let pitch = span / (ribs - 1) as f32;
    let first = cage_front + arch * 0.5;

    // **The spine: one vertebra per rib, and on over the loins to the
    // back of the pelvis**, lying along the ground with its processes
    // toward the back. Separate rather than one bar, for two reasons. A
    // column of knuckles is what a spine looks like. And a single bar the
    // length of a boar wears its picture repeated along its length (see
    // `mesh::FINE_UV_BIT`), a strip of seams, where every vertebra wears
    // the same short piece every other bone wears. Centred on the ribs, so a gap between
    // two vertebrae never lands on a rib.
    let vertebra = pitch - JOINT;
    let last_rib = first + (ribs - 1) as f32 * pitch;
    let lumbar = ((pelvis_back - (last_rib + pitch * 0.5)) / pitch).round().max(0.0) as usize;
    let vertebrae = ribs + lumbar;
    for i in 0..vertebrae {
        bones.push(bone(
            "vertebra",
            [-process * 0.5, spine * 0.5, first + i as f32 * pitch],
            [spine + process, spine, vertebra],
        ));
    }
    let spine_front = first - vertebra * 0.5;
    let spine_back = first + (vertebrae - 1) as f32 * pitch + vertebra * 0.5;

    // **The ribs: a post up out of the spine, an arch over the upper flank
    // and a post down to the breastbone.** Both posts stand on the ground.
    // The arch is `NEST` thicker than the posts and runs half an arch past
    // each, so every face of a post is `CLEARANCE` inside the arch's; the
    // post out of the spine is `NEST` thinner than the vertebra it rises
    // through (see `CLEARANCE`).
    //
    // **The middle ribs are the tallest** -- four fifths of the rise at
    // either end of the cage and all of it in the middle -- because a row
    // of equal hoops seen from the side is a crate, and a chest is a
    // barrel.
    let mut tallest = 0.0f32;
    for i in 0..ribs {
        let z = first + i as f32 * pitch;
        let barrel = (std::f32::consts::PI * (i as f32 + 0.5) / ribs as f32).sin();
        let rise = cage_rise * (0.8 + 0.2 * barrel);
        tallest = tallest.max(rise);
        let axis = rise - arch * 0.5;
        bones.push(bone("rib arch", [cage_depth * 0.5, axis, z], [cage_depth + arch, arch, arch]));
        for x in [0.0, cage_depth] {
            bones.push(bone("rib", [x, axis * 0.5, z], [rib, axis, rib]));
        }
    }

    // **The pelvis on its side: the upper hip bone a plate over the loins,
    // on a post beside the spine and a seat bone at the socket.** Lower
    // than the ribcage and not as deep, which is the silhouette that tells
    // the hind end from the front at a glance. The post beside the spine
    // stands a `JOINT` clear of the vertebrae rather than in them: it is as
    // long as the pelvis, crosses the gaps between vertebrae, and would
    // sooner or later land a face on one. Both posts are `NEST` thinner and
    // shorter than the plate, as a rib's are than its arch.
    let pelvis_rise = cage_rise * 0.75;
    let pelvis_depth = cage_depth * 0.7;
    let blade = pelvis_rise - arch * 0.5;
    let plate = pelvis_depth + arch * 0.5;
    bones.push(bone("pelvis", [plate * 0.5, blade, hip], [plate, arch, pelvis_length]));
    for x in [spine * 0.5 + JOINT + rib * 0.5, pelvis_depth] {
        bones.push(bone("pelvis", [x, blade * 0.5, hip], [rib, blade, pelvis_length - NEST]));
    }

    // **The legs: parallel pairs, out from the belly, flat on the
    // ground.** Both legs of a pair take the same turns and lie side by
    // side, far enough out that the inner corner of the nearer one,
    // turned, is still a `JOINT` clear of the belly. As long as the leg
    // that shows *plus* the part of it inside the living body -- a third
    // of the body's depth -- because the bones of a leg start at the
    // shoulder, not at the belly fur.
    //
    // **Half a leg's width of daylight between the two of a pair**, and a
    // `JOINT`. A `JOINT` alone was the first try, and at arm's length a
    // pair read as one doubled bone with a seam down it -- a leg and its
    // shadow -- rather than as two legs.
    let reach = |leg: &Part| leg.size[1].max(leg.size[2]) + 0.35 * height;
    let thickness = |leg: &Part| (leg.size[0].min(leg.size[2]) * 0.4).max(0.5);
    let pair = |bones: &mut Vec<Part>,
                names: (&'static str, &'static str),
                belly: f32,
                z: f32,
                leg: &Part,
                turns: (f32, f32)| {
        let thick = thickness(leg);
        let spacing = thick * 1.5 + JOINT;
        let (sin, cos) = turns.0.sin_cos();
        let x = belly + JOINT + sin.abs() * (spacing + thick) * 0.5;
        for side in [-0.5f32, 0.5] {
            lay_limb(bones, names, [x + sin * side * spacing, z + cos * side * spacing], reach(leg), thick, turns);
        }
    };
    let foreleg = living.iter().find(|part| part.name.starts_with("foreleg"));
    let hind_leg = living.iter().find(|part| {
        part.name.starts_with("hind leg") || part.name.starts_with("haunch") || part.name.starts_with("leg ")
    });
    match foreleg {
        Some(leg) => pair(
            &mut bones,
            ("upper foreleg", "lower foreleg"),
            cage_depth + arch * 0.5,
            leg.at[2],
            leg,
            FORELEG_TURNS,
        ),
        // **The bird's forelimbs are its wings.** Bent like a foreleg they
        // were the first thing drawn, and a bird with four limbs round a
        // ribcage is a lizard. On its side, the upper wing lies folded
        // along the flank it is on, resting on the tallest rib, and the
        // lower one -- pinned under the body -- lies half open on the
        // ground behind its back. Nothing in the living model to measure,
        // its wings are folded into its picture, so each is most of a body
        // long and thinner than its legs.
        None => {
            let reach = length * 0.8;
            let (thick, thin) = (0.45, 0.4);
            let fold = cage_depth * 0.3;
            let shoulder = first - arch;
            bones.push(bone(
                "upper wing",
                [fold, tallest + thick * 0.5, shoulder + reach * 0.225],
                [thick, thick, reach * 0.45],
            ));
            bones.push(bone(
                "lower wing",
                [fold + (thick + thin) * 0.5 + JOINT, tallest + thin * 0.5, shoulder + arch + reach * 0.25],
                [thin, thin, reach * 0.5],
            ));
            let behind = spine * 0.5 + process + JOINT + OPEN_WING_TURNS.0.sin().abs() * thick * 0.5;
            lay_limb(&mut bones, ("upper wing", "lower wing"), [-behind, first], reach, thick, OPEN_WING_TURNS);
        }
    }
    if let Some(leg) = hind_leg {
        pair(&mut bones, ("upper hind leg", "lower hind leg"), plate, hip, leg, HIND_LEG_TURNS);
    }

    // **The tail**, if the animal has one: vertebrae along the ground on
    // from the back of the spine, thinning as they go. As long as the
    // living tail -- the bird's fan is feathers, and what is left of it is
    // one small bone.
    if let Some(tail) = named("tail") {
        let reach = tail.size[2] * 1.2;
        let count = ((reach / (2.0 * arch)).round() as usize).clamp(1, 4);
        let segment = (reach - count as f32 * JOINT) / count as f32;
        for i in 0..count {
            let along = (i as f32 + 0.5) / count as f32;
            let thick = arch - (arch - rib) * along;
            let z = spine_back + JOINT + i as f32 * (segment + JOINT) + segment * 0.5;
            bones.push(bone("tail", [0.0, thick * 0.5, z], [thick, thick, segment]));
        }
    }

    // **The skull, built upright and then laid on its side.** Upright it
    // lies on its jaw with y up from the jaw; `quarter` below turns it the
    // way the body is turned. As long as the living head and muzzle
    // together, less the back of the head that was muscle; its pieces are
    // fractions of that length.
    let mut skull = Vec::new();
    let [head_width, head_height, head_length] = head.size;
    let skull_length = (head.at[2] + head_length * 0.4) - (muzzle.at[2] - muzzle.size[2] * 0.5);
    // The neck: the living neck's longest side where there is one -- a
    // deer's is long, and its skeleton is -- and a vertebra and a half
    // where the head sat straight on the shoulders.
    let neck_length = named("neck").map_or(spine * 1.5, |neck| neck.size[1].max(neck.size[2]) * 0.6);
    let skull_back = spine_front - neck_length;
    let skull_front = skull_back - skull_length;

    let cranium = [head_width * 0.7, head_height * 0.6, skull_length * 0.45];
    let socket = skull_length * 0.2;
    let snout_length = skull_length * 0.35;
    let jaw = 0.12 * head_height;
    let mouth = 0.08 * head_height;
    let brow = 0.1 * head_height;
    let palate = jaw + mouth;
    let cranium_z = skull_back - cranium[2] * 0.5;
    let socket_z = skull_back - cranium[2] - socket * 0.5;
    let snout = [
        (muzzle.size[0] * 0.7).min(cranium[0] - 2.0 * NEST),
        (muzzle.size[1] * 0.7).min(cranium[1] * 0.55),
        snout_length,
    ];
    skull.push(bone("skull", [0.0, cranium[1] * 0.5, cranium_z], cranium));
    // **The eye socket is a hole, and the hole is what makes a skull.**
    // A box of bone the shape of a head is a head. Between the cranium
    // and the snout the skull is only a brow along the top and a
    // cheekbone along the bottom, and the daylight between them goes
    // right through -- on a skull on its side, it looks at the sky.
    let bar = cranium[0] - NEST;
    skull.push(bone(
        "skull",
        [0.0, cranium[1] - 0.05 * head_height - brow * 0.5, socket_z],
        [bar, brow, socket],
    ));
    skull.push(bone("skull", [0.0, palate + brow * 0.5, socket_z], [bar, brow, socket]));
    skull.push(bone("snout", [0.0, palate + snout[1] * 0.5, skull_front + snout_length * 0.5], snout));
    // The lower jaw: a bar under each side of the snout, running back
    // into the cranium, with the mouth open above it. One bar in the
    // middle where two would not fit side by side -- which on the bird
    // is the lower half of its beak.
    //
    // **It stands a `CLEARANCE` off the underside of the cranium.** Its
    // back half runs into the cranium, and while the skull lay on its jaw
    // the two undersides shared the ground, where nothing can see them.
    // Laid on its side that plane is the side of the skull facing the
    // belly, in plain view, and the two faces would fight over it.
    let jaw_front = skull_front + NEST;
    let jaw_length = cranium_z - jaw_front;
    let jaw_side = snout[0] * 0.5 - jaw * 0.5 - CLEARANCE;
    let jaw_z = jaw_front + jaw_length * 0.5;
    let jaw_y = CLEARANCE + jaw * 0.5;
    if jaw_side > jaw * 0.5 + 0.1 {
        for side in [-1.0f32, 1.0] {
            skull.push(bone("jaw", [side * jaw_side, jaw_y, jaw_z], [jaw, jaw, jaw_length]));
        }
    } else {
        skull.push(bone("jaw", [0.0, jaw_y, jaw_z], [jaw, jaw, jaw_length]));
    }
    if muzzle.name == "beak" {
        // A beak comes to a point: a thinner bone out of the end of the
        // snout, rooted `NEST` inside it.
        let tip = [snout[0] - NEST, snout[1] - NEST, skull_length * 0.3];
        skull.push(bone("beak", [0.0, palate + snout[1] * 0.5, skull_front + NEST - tip[2] * 0.5], tip));
    }

    // **What a species is known by, where it was bone already.** The
    // living tusks and antlers keep their own sizes -- they never had
    // flesh on them -- and are put on the skull where they grew: tusks
    // up beside the snout from the jaw, antlers rooted in the crown
    // with the tine forward. Each one a group of its own, for the lift
    // below.
    let mut features: Vec<Vec<Part>> = Vec::new();
    for tusk in living.iter().filter(|part| part.skin == Skin::Tusk) {
        let side = tusk.at[0].signum();
        features.push(vec![bone(
            "tusk",
            [side * (snout[0] * 0.5 + tusk.size[0] * 0.5), tusk.size[1] * 0.5, skull_front + snout_length * 0.3],
            tusk.size,
        )]);
    }
    for beam in living.iter().filter(|part| part.skin == Skin::Antler && part.name.ends_with("beam")) {
        let side = beam.at[0].signum();
        let [beam_width, beam_height, beam_length] = beam.size;
        // Astride the side of the cranium, a quarter of it sunk in.
        let x = side * cranium[0] * 0.5;
        let base = cranium[1] - beam_height * 0.25;
        let mut antler = vec![bone("antler", [x, base + beam_height * 0.5, cranium_z], beam.size)];
        let tine = living
            .iter()
            .find(|part| part.skin == Skin::Antler && part.name.ends_with("tine") && part.at[0].signum() == side);
        if let Some(tine) = tine {
            let [tine_width, tine_height, tine_length] = tine.size;
            antler.push(bone(
                "antler",
                [
                    x + side * (beam_width + tine_width) * 0.5,
                    base + beam_height - NEST - tine_height * 0.5,
                    cranium_z + beam_length * 0.5 - NEST - tine_length * 0.5,
                ],
                tine.size,
            ));
        }
        features.push(antler);
    }
    // **Horns keep their core, which is bone, and lose the ringed sheath,
    // which is not.** One straight bone a side out of the crown, four fifths
    // of the living horn's upright part and none of its laid-back tip --
    // astride the side of the cranium, a quarter sunk in, as an antler's beam
    // is and for the same reason: a face a horn's width off the cranium's is
    // not a plane the two can fight over (`CLEARANCE`).
    for horn in living.iter().filter(|part| part.skin == Skin::Horn && !part.name.ends_with("tip")) {
        let side = horn.at[0].signum();
        let core = [horn.size[0], horn.size[1] * 0.8, horn.size[2]];
        let base = cranium[1] - core[1] * 0.25;
        features.push(vec![bone("horn", [side * cranium[0] * 0.5, base + core[1] * 0.5, cranium_z], core)]);
    }

    // **Laid on its side: a quarter turn about its length, the way
    // `fallen_pose` turns the living model** -- (x, y) to (-y, x), the
    // sizes swapped to match. Exact for a box, so every clearance built
    // into the upright skull survives it. The crown ends up facing the
    // back, the jaw the belly and the skull's right side the sky; the
    // cranium is centred on the line of the spine and its underside put on
    // the ground.
    let quarter = |part: Part| Part {
        at: [cranium[1] * 0.5 - part.at[1], part.at[0] + cranium[0] * 0.5, part.at[2]],
        size: [part.size[1], part.size[0], part.size[2]],
        ..part
    };
    bones.extend(skull.into_iter().map(quarter));
    // **A feature that ends up under the skull is lifted onto the ground,
    // whole.** On its side the deer's lower antler reaches a sixteenth and
    // a half below the cranium, and a skeleton is placed by its lowest
    // point -- so the whole animal would hang in the air on one antler. It
    // is lifted as a unit, beam and tine together, so the tine stays beside
    // its beam rather than sliding into it.
    for feature in features {
        let turned: Vec<Part> = feature.into_iter().map(quarter).collect();
        let low = turned.iter().map(|part| part.at[1] - part.size[1] * 0.5).fold(0.0f32, f32::min);
        bones.extend(turned.into_iter().map(|part| Part { at: [part.at[0], part.at[1] - low, part.at[2]], ..part }));
    }

    // **The neck**: vertebrae along the ground from the front of the spine
    // to the back of the skull, rising from the middle of the one to the
    // middle of the other.
    let count = ((neck_length / (2.5 * arch)).round() as usize).clamp(1, 4);
    let segment = (neck_length - (count + 1) as f32 * JOINT) / count as f32;
    for i in 0..count {
        let along = (i as f32 + 0.5) / count as f32;
        let y = spine * 0.5 + (cranium[0] * 0.5 - spine * 0.5) * along;
        let z = spine_front - JOINT - segment * 0.5 - i as f32 * (segment + JOINT);
        bones.push(bone("neck", [0.0, y, z], [arch, arch, segment]));
    }

    bones
}

/// Which way a carcass lies, from the cell it lies in.
///
/// Hashed from the world cell so that two kills in one meadow do not
/// lie parallel, and so the same carcass faces the same way after every
/// remesh -- and, because the mining cracks are drawn on the same model
/// (`mesh::build_mesh` and `mining::build_break_mesh_into` both ask
/// this), so that the cracks lie on the animal and not beside it.
pub fn carcass_yaw(wx: i32, wy: i32, wz: i32) -> f32 {
    let hash = (wx as u32).wrapping_mul(0x9E37_79B9)
        ^ (wz as u32).wrapping_mul(0x85EB_CA6B)
        ^ (wy as u32).wrapping_mul(0xC2B2_AE35);
    (hash >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU
}

/// The pose of a fallen animal: rolled a quarter turn about its own
/// length, and shifted so that the rolled model's lowest point sits a
/// hair above the ground and the middle of its footprint on the point
/// it is placed by.
fn fallen_pose(species: Species) -> Pose {
    resting_pose(parts(species), std::f32::consts::FRAC_PI_2, |_| 0.0)
}

/// A hair above the ground, so the underside of whatever lies there
/// does not z-fight the block it lies on. One sixty-fourth of a block is
/// well over a texel of depth at any distance a carcass is legible from.
pub(crate) const LIFT: f32 = 1.0 / 64.0;

/// Any model lying on the ground: rolled by `roll` about its own length,
/// then shifted so its lowest point is `LIFT` above the ground and the
/// middle of its footprint is on the point it is placed by.
///
/// **Measured from the model that is drawn**, which is why the model is
/// an argument. A skeleton placed by the living animal's measurements
/// is placed by a body that is not there: its bones are narrower than
/// the flank they came out of, and it would hang in the air by the
/// difference.
///
/// **Measured with the swings it is drawn with**, which is why `swing_of`
/// is an argument rather than a zero written inside. Every animal lies
/// with its parts square to the body and hands in `|_| 0.0`; a person
/// does not (`player_model::LIMP`), and a figure measured square and then
/// drawn with an arm thrown out is a figure placed by a limb that is
/// somewhere else -- which buries whatever swung by however far it swung.
pub(crate) fn resting_pose(model: &[Part], roll: f32, swing_of: impl Fn(usize) -> f32) -> Pose {
    use crate::engine::mesh::faces;

    // Measure the rolled model to know where its underside and its
    // middle are; the shift then puts that underside on the ground.
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (index, part) in model.iter().enumerate() {
        for face in faces().iter() {
            for corner in face.corners.iter() {
                let point = posed_local(part, *corner, swing_of(index), roll);
                low = low.min(point);
                high = high.max(point);
            }
        }
    }
    Pose {
        roll,
        shift: Vec3::new((low.x + high.x) * 0.5, low.y - LIFT, (low.z + high.z) * 0.5),
        scale: 1.0,
    }
}

/// The quads of a fallen animal, corners in the face order
/// `append_part_posed` emits them in, and nothing else: no skins, no
/// light. What the mining cracks are drawn on. The corner arithmetic is
/// the same as `append_part_posed`'s -- posed, shifted, turned by the
/// yaw and put at `ground` -- and a test holds the two together, because
/// cracks a texel off the flank are cracks floating in the air.
pub fn fallen_quads(species: Species, ground: Vec3, yaw: f32, out: &mut Vec<[[f32; 3]; 4]>) {
    let pose = fallen_pose(species);
    // The parts `build_fallen` draws, and no others: its folded wings and
    // not the spread ones.
    let drawn: Vec<Part> =
        parts(species).iter().filter(|part| part.gait.shown(false)).copied().collect();
    posed_quads(&drawn, pose, ground, yaw, |_| 0.0, out);
}

/// The quads of a posed model, in the face order `push_posed_box` emits
/// them in and with the same corner arithmetic -- posed, shifted, turned
/// by the yaw and put at `ground` -- and nothing else: no skins, no light.
///
/// **What the mining cracks are drawn on**, for everything the mesher
/// bakes into a chunk as a model lying on the ground: a carcass, a
/// skeleton, a dead player. Its own function so there is one copy of the
/// arithmetic rather than one per family -- cracks a texel off the flank
/// are cracks floating in the air, and that is the fault a second copy
/// produces the first time either is edited.
pub(crate) fn posed_quads(
    model: &[Part],
    pose: Pose,
    ground: Vec3,
    yaw: f32,
    swing_of: impl Fn(usize) -> f32,
    out: &mut Vec<[[f32; 3]; 4]>,
) {
    use crate::engine::mesh::faces;
    let (yaw_sin, yaw_cos) = yaw.sin_cos();
    for (index, part) in model.iter().enumerate() {
        for face in faces().iter() {
            let mut quad = [[0.0f32; 3]; 4];
            for (slot, corner) in face.corners.iter().enumerate() {
                let swung = posed_local(part, *corner, swing_of(index), pose.roll) - pose.shift;
                let (fx, fz) = (-swung.z, swung.x);
                quad[slot] = [
                    ground.x + fx * yaw_cos - fz * yaw_sin,
                    ground.y + swung.y,
                    ground.z + fx * yaw_sin + fz * yaw_cos,
                ];
            }
            out.push(quad);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn world_face_of(
    face_index: usize,
    turn_sin: f32,
    turn_cos: f32,
    swing_sin: f32,
    swing_cos: f32,
    roll_sin: f32,
    roll_cos: f32,
    yaw_sin: f32,
    yaw_cos: f32,
) -> u8 {
    // The mesher's face order: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
    let normal = match face_index {
        0 => Vec3::Y,
        1 => Vec3::NEG_Y,
        2 => Vec3::X,
        3 => Vec3::NEG_X,
        4 => Vec3::Z,
        _ => Vec3::NEG_Z,
    };
    // Turned about the vertical by the part's own turn, as `posed_local`
    // turns its corners -- or a skeleton's angled leg lights its sides as
    // though it still lay square to the body...
    let normal = Vec3::new(
        normal.x * turn_cos + normal.z * turn_sin,
        normal.y,
        normal.z * turn_cos - normal.x * turn_sin,
    );
    // ...swung in the plane the animal walks in...
    let swung = Vec3::new(
        normal.x,
        normal.y * swing_cos - normal.z * swing_sin,
        normal.y * swing_sin + normal.z * swing_cos,
    );
    // ...rolled about the animal's own length -- a fallen animal's back
    // faces sideways and its flank faces the sky, and the light has to
    // agree with the geometry below...
    let swung = Vec3::new(
        swung.x * roll_cos - swung.y * roll_sin,
        swung.x * roll_sin + swung.y * roll_cos,
        swung.z,
    );
    // ...then the quarter turn that maps the model's -Z to +X, then the
    // yaw. Exactly what happens to a corner, minus the translation.
    let (fx, fz) = (-swung.z, swung.x);
    let world = Vec3::new(
        fx * yaw_cos - fz * yaw_sin,
        swung.y,
        fx * yaw_sin + fz * yaw_cos,
    );
    // The dominant axis, with its sign.
    let (mut best, mut face) = (world.x.abs(), if world.x >= 0.0 { 2 } else { 3 });
    if world.y.abs() > best {
        best = world.y.abs();
        face = if world.y >= 0.0 { 0 } else { 1 };
    }
    if world.z.abs() > best {
        face = if world.z >= 0.0 { 4 } else { 5 };
    }
    face
}

/// How a carcass is dressed: what its parts are wearing at this stage
/// of butchering. See `build_fallen`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dressing {
    /// Whole: every part in its own skin.
    Coat,
    /// The fleece is off and the skin under it shows -- the sheep's
    /// second stage, and nobody else's.
    Hide,
    /// Skinned: flesh everywhere.
    Flesh,
    /// **Nothing left but the frame.** What a carcass nobody came back
    /// for turns into -- drawn with `skeleton_parts`, not with the living
    /// parts, and in bone as a *material*, cut to each face's size the
    /// way `Flesh` wears raw meat.
    ///
    /// **The picture is the ivory of the boar's tusks, not
    /// `hide/bone.png`.** That file is the bone *item*: a bone drawn on a
    /// transparent square, for an inventory slot. A material on a small
    /// face shows a corner of its picture (`mesh::FINE_UV_BIT`), and the
    /// corners of that icon are empty -- so every thin
    /// bone wore a crop of transparent texels and outline, and every big
    /// one had the whole icon stamped on it, which is half of what made
    /// the old skeleton read as "a bone texture on each body part". The
    /// tusk tile is solid ivory, drawn as a material already, and costs
    /// no layer. See `a_skeleton_wears_solid_ivory_rather_than_the_bone_icon`.
    Bones,
}

/// The pose of a model: the roll about its own length, and where its
/// local origin is put before the yaw. A standing animal has neither.
///
/// **Where the boxes are, and nothing about what they wear.** It carried
/// the `Dressing` too, which was convenient while the only thing drawn
/// this way was an animal and wrong as soon as something else was: a
/// dead player's parts each name their own piece of a picture
/// (`player_model::Swatch`), and there is no one dressing to put in the
/// field. `push_posed_box` reads this and asks the caller what each face
/// wears, which is the split the two things actually have.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pose {
    pub(crate) roll: f32,
    /// Subtracted from every local point after the roll, so that the
    /// point handed in lands on `centre`: a fallen animal is placed by
    /// the middle of its underside, not by the body centre it walks
    /// around.
    pub(crate) shift: Vec3,
    /// How big the model is drawn, about the point it is placed by: one for
    /// everything but the young (`Motion::youth`). Applied last, after the
    /// shift, so a young animal lying down is placed exactly as an adult is,
    /// only smaller.
    pub(crate) scale: f32,
}

/// A model with nothing done to it: what a standing, unhurt, straight-running
/// animal is drawn with.
///
/// **Only the tests name it now.** It used to be baked into `append_part`,
/// because a walking animal had no pose at all; now every part goes through
/// `append_part_posed` with a pose the frame computed, and this is what the
/// geometry tests measure against.
#[cfg(test)]
const STANDING_POSE: Pose = Pose { roll: 0.0, shift: Vec3::ZERO, scale: 1.0 };

// (`append_part`, which was this pair of lines with `STANDING_POSE` baked in,
// is gone: a walking animal has a pose of its own now -- it leans into its
// turns and flinches when it is hit -- so every part goes through
// `append_part_posed` and there is no longer a caller for whom the pose is a
// constant.)

/// One local corner of a part after its swing and the pose's roll, in
/// the animal's own frame (x sideways, y up, z forward) -- the same
/// arithmetic `append_part_posed` uses, kept in one place so
/// `build_fallen` can measure a model with it before placing it.
/// How much every box is grown, in sixteenths.
///
/// **Two boxes that touch exactly share a plane, and a shared plane
/// flickers.** A leg's top face and the belly's bottom face at the same
/// height are two surfaces the depth buffer cannot separate: which one
/// wins is decided by rounding, it changes as the camera moves, and what
/// a player sees is an animal with a shimmering seam across it -- which
/// is exactly what was reported ("textures overlapping each other").
///
/// Growing every box by a fiftieth of a sixteenth pushes the buried face
/// *inside* its neighbour, where the depth test throws it away cleanly
/// and for ever. It is one eight-hundredth of a block: nothing to see,
/// and it costs no geometry, no pass and no sorting.
///
/// The alternative -- culling faces that are inside another box -- means
/// every model knowing about every other box in it, which is a solver
/// where this is a constant.
const SEAM_BITE: f32 = 0.02;

fn posed_local(part: &Part, corner: [f32; 3], swing: f32, roll: f32) -> Vec3 {
    let half = Vec3::from(part.size) * (0.5 * SCALE);
    let at = Vec3::from(part.at) * SCALE;
    // Its own top, unless the table names the joint it hangs from (see
    // `Part::pivot`).
    let pivot = match part.pivot {
        Some([y, z]) => Vec3::new(at.x, y * SCALE, z * SCALE),
        None => Vec3::new(at.x, at.y + half.y, at.z),
    };
    let (swing_sin, swing_cos) = swing.sin_cos();
    let (roll_sin, roll_cos) = roll.sin_cos();
    let offset = Vec3::new(
        (corner[0] - 0.5) * (part.size[0] + SEAM_BITE) * SCALE,
        (corner[1] - 0.5) * (part.size[1] + SEAM_BITE) * SCALE,
        (corner[2] - 0.5) * (part.size[2] + SEAM_BITE) * SCALE,
    );
    // The part's own turn about its centre comes first, so a turned bone
    // is still a box of its own size, only not square to the body. A turn
    // of zero is exactly the corner this was before there were turns:
    // the cosine is one and the sine is zero.
    let (turn_sin, turn_cos) = part.turn.sin_cos();
    let local = at
        + Vec3::new(
            offset.x * turn_cos + offset.z * turn_sin,
            offset.y,
            offset.z * turn_cos - offset.x * turn_sin,
        );
    let swung = match part.gait {
        // **A spread wing rolls about the bird's length, through its
        // shoulder** (`Gait::Wing`): the hinge is the wing's inner edge, and
        // which side of it the wing is on says which way is up, so one
        // number raises both wings together.
        Gait::Wing(hinge) => {
            let hinge = Vec3::new(hinge as f32 * SCALE, at.y, 0.0);
            let side = if at.x >= hinge.x { 1.0 } else { -1.0 };
            let (sin, cos) = (swing * side).sin_cos();
            let from = local - hinge;
            Vec3::new(hinge.x + from.x * cos - from.y * sin, hinge.y + from.x * sin + from.y * cos, local.z)
        }
        _ => {
            let from_pivot = local - pivot;
            Vec3::new(
                local.x,
                pivot.y + from_pivot.y * swing_cos - from_pivot.z * swing_sin,
                pivot.z + from_pivot.y * swing_sin + from_pivot.z * swing_cos,
            )
        }
    };
    Vec3::new(
        swung.x * roll_cos - swung.y * roll_sin,
        swung.x * roll_sin + swung.y * roll_cos,
        swung.z,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_part_posed(
    part: &Part,
    species: Species,
    centre: Vec3,
    yaw: f32,
    swing: f32,
    pose: Pose,
    dressing: Dressing,
    hurt: Option<f32>,
    layers: &FaceLayers,
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    let (sky, block_light) = light;
    let block_light = match hurt {
        Some(flash) => block_light.max((primitive_shared::types::MAX_LIGHT as f32 * flash) as u8),
        None => block_light,
    };

    // Which of the box's own faces looks forward. The mesher's face
    // order is 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z, and an animal faces
    // -Z in its own space.
    const FRONT_FACE: usize = 5;
    /// ...and the two sides, in the same order.
    const RIGHT_FACE: usize = 2;
    const LEFT_FACE: usize = 3;

    push_posed_box(
        part,
        centre,
        yaw,
        swing,
        pose,
        |face_index| {
            let skin = match (face_index, part.front, part.sides) {
                (FRONT_FACE, Some(front), _) => front,
                (RIGHT_FACE, _, Some((right, _))) => right,
                (LEFT_FACE, _, Some((_, left))) => left,
                _ => part.skin,
            };
            // What the part wears: its own skin, or -- on a carcass that
            // has been skinned -- flesh, and on a fleeced sheep the hide
            // under the wool. A whole picture per face rather than the
            // skin's crop, because meat and hide are plain tiles.
            let (layer, tiles) = match dressing {
                // **A feature nobody drew is hide, and hide is cut.** An
                // animal's sheet falls back to its hide for any square left
                // empty (`texture::ANIMAL_SHEETS`), so a zebra's ear and nose
                // wore the whole striped hide -- sixteen texels of stripes
                // squeezed onto a face two texels across, a smear of noise on
                // a clean head. Asked of the layer it actually got, not of
                // the square it asked for.
                Dressing::Coat => {
                    let layer = skin.layer(species, layers);
                    let fell_back = skin != Skin::Hide && layer == Skin::Hide.layer(species, layers);
                    (layer, skin.tiles() || fell_back)
                }
                Dressing::Hide => (layers.layer_for_face(primitive_shared::types::BLOCK_HIDE, 0), true),
                Dressing::Flesh => (layers.layer_for_face(primitive_shared::types::BLOCK_RAW_MEAT, 0), true),
                Dressing::Bones => (ivory(layers), true),
            };
            // A material on a small part wears a piece of itself cut to the
            // face's size, so a tusk shows three texels of ivory rather
            // than the whole picture squeezed into three sixteenths -- and
            // exactly its size now, where the old crop code snapped to a power
            // of two and a five-sixteenth leg wore four (see
            // `mesh::FINE_UV_BIT`). The face's two axes follow `face_uv`: u
            // and v run along different world axes per face, and the size has
            // to be measured along the same ones or a leg wears its width up
            // its length. Part sizes are sixteenths already (`SCALE`).
            //
            // Always from the picture's own corner, which is what a tiling
            // material may do and a *piece* of a picture may not: see
            // `push_posed_box`'s `face_wears`.
            (layer, tiles.then(|| material_cut(part.size, face_index)))
        },
        (sky, block_light),
        vertices,
        indices,
    );
}

/// The piece of a *material* a face of a box this size wears.
///
/// A material on a small part shows a piece of itself cut to the face, so
/// a tusk shows three texels of ivory rather than the whole picture
/// squeezed into three sixteenths -- and exactly its size, where the crop
/// code this replaced snapped to a power of two and a five-sixteenth leg
/// wore four (see `mesh::FINE_UV_BIT`). The face's two axes follow
/// `face_uv`: u and v run along different world axes per face, and the
/// size has to be measured along the same ones or a leg wears its width
/// up its length. Part sizes are sixteenths already (`SCALE`).
///
/// Its own function because a skeleton that is not an animal's wants the
/// same cut of the same ivory (`player_model::Wear::Bone`).
pub(crate) fn material_cut(size: [f32; 3], face_index: usize) -> [f32; 4] {
    let [sx, sy, sz] = size;
    let (u, v) = match face_index {
        0 | 1 => (sx, sz),
        2 | 3 => (sz, sy),
        _ => (sx, sy),
    };
    [0.0, 0.0, u / 16.0, v / 16.0]
}

/// The one bone material in the game: the ivory of the boar's tusks.
///
/// Every skeleton wears it -- an animal's (`Dressing::Bones`) and a
/// player's (`player_model::build_fallen_bones`) -- from one tile, which
/// is why a second bone picture has never been needed. See
/// `Dressing::Bones` for why it is the tusk and not `hide/bone.png`.
pub(crate) fn ivory(layers: &FaceLayers) -> u32 {
    layers.animal(Species::Boar, Skin::Tusk.slot())
}

/// One box of a model: swung about its joint, rolled and shifted by the
/// pose, turned by the yaw and put at `centre`.
///
/// **Split out of `append_part_posed` so that something which is not an
/// animal can be drawn by the same arithmetic.** A dead player lies in
/// the world the way a carcass does -- the figure's own boxes rolled onto
/// their side and baked into the chunk mesh (`player_model::build_fallen`)
/// -- and the only thing it does differently is what each face wears.
/// Everything here is the part that must not be written twice: the swing
/// about the pivot, the roll, the shift onto the ground, the quarter turn
/// that maps the model's -Z to +X, the yaw, the normal taken from the
/// emitted geometry, and above all `world_face_of` -- a second copy of
/// which is a body lit as though it were facing north for ever.
///
/// `face` answers, per face index, which layer that face wears and which
/// piece of that picture, as `[u, v, du, dv]` in pictures. `None` is the
/// whole picture over the whole face, which is what a *feature* -- an eye,
/// a muzzle -- wants; a rectangle is what a material wants, either a cut
/// of itself the size of the face (`append_part_posed`) or a named patch
/// of a larger drawing (`player_model::Swatch`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_posed_box(
    part: &Part,
    centre: Vec3,
    yaw: f32,
    swing: f32,
    pose: Pose,
    face_wears: impl Fn(usize) -> (u32, Option<[f32; 4]>),
    light: (u8, u8),
    vertices: &mut Vec<crate::engine::mesh::Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::engine::mesh::{face_uv, faces, pack_light, Vertex};

    let (sky, block_light) = light;

    let (turn_sin, turn_cos) = part.turn.sin_cos();
    // A spread wing's beat is a roll about the length (`posed_local`), so
    // its faces are turned by the roll and not by the swing. The two are
    // turns about the same axis as the pose's roll, so they simply add --
    // and a wing lit by the swing would be lit as a leg kicked forward.
    let (face_swing, face_roll) = match part.gait {
        Gait::Wing(hinge) => (0.0, pose.roll + if part.at[0] >= hinge as f32 { swing } else { -swing }),
        _ => (swing, pose.roll),
    };
    let (swing_sin, swing_cos) = face_swing.sin_cos();
    let (roll_sin, roll_cos) = face_roll.sin_cos();
    let (yaw_sin, yaw_cos) = yaw.sin_cos();

    // Every point a vertex of this part goes at, placed once for all six
    // faces: see `placed_corners`.
    let placed = placed_corners(part, swing, pose);

    for (face_index, face) in faces().iter().enumerate() {
        let (layer, cut) = face_wears(face_index);
        // **Which way this face ends up pointing in the world.**
        //
        // The light word carries a face index and the shader turns it
        // into a normal (`face_normal` in shader.wgsl), so it has to
        // describe the face *after* the part has been swung and the
        // animal turned -- not before. Writing the model-space index,
        // which is what this did, lit every animal as though it were
        // facing north: a boar walking east had its sunlit flank shaded
        // as its shadowed one, and -- worse -- the shading never
        // changed as the animal turned, because the indices could not.
        // A creature whose light is welded to itself rather than to the
        // world is the plastic look this renderer works hard to avoid
        // everywhere else.
        //
        // Yaw is continuous and the light word holds one of six
        // directions, so the answer is the nearest axis. That is exact
        // at the quarter turns and never more than a little off between
        // them, which is all a lambert term needs.
        let world_face = world_face_of(face_index, turn_sin, turn_cos, swing_sin, swing_cos, roll_sin, roll_cos, yaw_sin, yaw_cos);
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            // The corner in the part's own space, swung about the pivot
            // and rolled by the pose (`posed_local`), less the pose's
            // shift -- placed before the faces were walked...
            let swung = placed[corner_bits(*corner)];
            // ...turned to face the way the animal is looking, and put
            // where the animal is.
            //
            // **The model faces -Z and a yaw of zero points along +X**,
            // so the model's own axes have to be turned a quarter before
            // the yaw is applied: front (-Z) becomes +X, and the
            // animal's right (+X) becomes +Z.
            //
            // This had the sign the other way round and the result was
            // exactly what it sounds like -- every animal in the world
            // walked backwards, facing where it had come from. Worth
            // stating rather than fixing quietly: the mapping is a
            // rotation and not a reflection, so `(x, z) -> (-z, x)` and
            // never `(z, -x)`, which mirrors the animal as well as
            // turning it.
            let (fx, fz) = (-swung.z, swung.x);
            let uv = face_uv(face_index, *corner);
            let vertex = Vertex::tinted(
                [
                    centre.x + fx * yaw_cos - fz * yaw_sin,
                    centre.y + swung.y,
                    centre.z + fx * yaw_sin + fz * yaw_cos,
                ],
                uv,
                layer,
                // Unoccluded: a thing standing in the open air is not in
                // anybody's corner.
                pack_light(sky, block_light, 3, world_face),
                0,
            );
            vertices.push(match cut {
                Some([u, v, du, dv]) => vertex.with_fine_uv([u + uv[0] * du, v + uv[1] * dv]),
                None => vertex,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Which of a box's eight corners a unit-cube corner is, as three bits.
///
/// Exact, because `mesh::faces` names its corners with nought and one and
/// nothing else; `placing_the_corners_once_changes_no_vertex` holds it to
/// that.
#[inline]
fn corner_bits(corner: [f32; 3]) -> usize {
    (corner[0] as usize) | ((corner[1] as usize) << 1) | ((corner[2] as usize) << 2)
}

/// The eight corners of a part, swung, rolled and shifted by the pose:
/// every point `append_part_posed` puts a vertex at, indexed by
/// [`corner_bits`].
///
/// **Eight calls to `posed_local`, not twenty-four.** Six faces of four
/// corners each ask for the same eight points three times over, and every
/// ask is three `sin_cos` and a dozen multiplies -- for every part of every
/// animal on screen, each time the moving geometry is rebuilt
/// (`DYNAMIC_REBUILD_HZ`). Placing each corner once and looking it up gives
/// the same bits, because it is the same call with the same arguments.
///
/// Rejected: taking the sines out of `posed_local` into a struct built once
/// per part. Fewer calls still, but a second copy of the arithmetic that
/// places a corner -- the seam bite, the pivot, the wing's hinge -- beside
/// the one `build_fallen` and the skeleton tests measure with, and the day
/// the two disagree is an animal that is not drawn where it is measured.
fn placed_corners(part: &Part, swing: f32, pose: Pose) -> [Vec3; 8] {
    std::array::from_fn(|bits| {
        let corner = [(bits & 1) as f32, ((bits >> 1) & 1) as f32, ((bits >> 2) & 1) as f32];
        (posed_local(part, corner, swing, pose.roll) - pose.shift) * pose.scale
    })
}

#[cfg(test)]
mod tests {
    /// **Every leg swings from its hip.** A box swings about its own top
    /// unless the file names a joint (`Part::pivot`), and a leg whose joint
    /// was left at the bottom of it -- or on the lower box of a two-part leg
    /// -- would scuff its foot along the ground and wave its shoulder, which
    /// is the one thing that makes a walk read as a puppet. Measured on the
    /// drawn geometry, face against face: through a stride the sole of a leg
    /// has to travel further than its top does.
    #[test]
    fn every_leg_swings_from_its_hip_and_not_from_its_foot() {
        use crate::engine::mesh::Vertex;
        // A box is 24 vertices in table order and its faces go +Y, -Y first
        // (see `append_part_posed`), so face 0 is the top and face 1 the
        // sole -- the same reading `every_paw_stays_on_the_foot_of_its_leg`
        // takes.
        let face_middle = |boxed: &[Vertex], face: usize| {
            boxed[face * 4..face * 4 + 4].iter().map(|v| Vec3::from_array(v.position)).sum::<Vec3>() / 4.0
        };
        let mut legs = 0;
        for &species in Species::ALL {
            let model = parts(species);
            // A quarter of a stride in, where the swing is at its widest,
            // and at a walk rather than a run -- a bird past
            // `AIRBORNE_SPEED` tucks its legs up and swings nothing.
            let standing = mesh(species, 0.0, 0.0, 0.0);
            let striding = mesh(species, 0.0, 2.0, std::f32::consts::FRAC_PI_2 / STRIDE);
            // Vertices are in the order the parts were *drawn*, and a
            // folded wing or a spread one is left out of it.
            let mut index = 0;
            for part in model {
                if !part.gait.shown(false) {
                    continue;
                }
                let drawn = index;
                index += 1;
                if !matches!(part.gait, Gait::LegFront | Gait::LegBack) {
                    continue;
                }
                legs += 1;
                let index = drawn;
                let (a, b) = (&standing[index * 24..index * 24 + 24], &striding[index * 24..index * 24 + 24]);
                let sole = (face_middle(b, 1) - face_middle(a, 1)).length();
                let top = (face_middle(b, 0) - face_middle(a, 0)).length();
                assert!(
                    sole > top + 1e-4,
                    "{}: {} moved its top {top:.4} and its sole {sole:.4} through a stride,                      which is a leg swinging about the wrong end",
                    species.name(),
                    part.name
                );
            }
        }
        assert!(legs >= 60, "only {legs} legs in the whole bestiary");
    }

    /// **A blow throws the body both ways.** The side a hit came from is not
    /// on the wire, so the flinch used to roll every animal the same way,
    /// every time: `Motion::flinch_side` is what the client draws instead,
    /// and the two sides have to be mirror images or the "side" is a fudge.
    #[test]
    fn a_blow_from_either_side_throws_the_body_the_other_way() {
        let struck = |side: f32| {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            build(
                Species::Boar,
                TEST_CENTRE,
                // Facing +X, so the animal's own left and right are -Z and
                // +Z (`Camera::right_horizontal`).
                0.0,
                Motion { hurt: Some(1.0), flinch_side: side, ..Default::default() },
                &FaceLayers::empty_for_test(),
                (15, 0),
                &mut vertices,
                &mut indices,
            );
            vertices
        };
        let right = struck(1.0);
        let left = struck(-1.0);
        let whole = {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            build(
                Species::Boar,
                TEST_CENTRE,
                0.0,
                Motion::default(),
                &FaceLayers::empty_for_test(),
                (15, 0),
                &mut vertices,
                &mut indices,
            );
            vertices
        };
        let across = |v: &[crate::engine::mesh::Vertex]| {
            v.iter().map(|p| p.position[2]).sum::<f32>() / v.len() as f32
        };
        let (middle, thrown_right, thrown_left) = (across(&whole), across(&right), across(&left));
        assert!(
            thrown_right > middle + 0.01,
            "a blow that throws a boar to its right left it at {thrown_right:.3} against {middle:.3}"
        );
        assert!(
            (thrown_left - middle) + (thrown_right - middle) < 1e-3,
            "the two sides of the flinch are not mirror images: {thrown_left:.3} and {thrown_right:.3} about {middle:.3}"
        );
    }

    /// **The legs fade in rather than snapping on.** The dead zone that
    /// stops a drifting animal from performing a walk on the spot used to be
    /// a cliff: at a hair under `STANDING` the legs were still, at a hair
    /// over they were a third of the way through their swing, and every
    /// animal setting off or pulling up jerked once. See `STANDING_BAND`.
    #[test]
    fn legs_come_up_to_speed_rather_than_snapping_into_a_stride() {
        // One foreleg, a quarter stride in, where the swing is widest: how
        // far along the animal the leg has reached is the amplitude, and the
        // body around it would drown the measurement.
        let leg = parts(Species::Deer)
            .iter()
            .position(|part| part.gait == Gait::LegFront)
            .expect("a deer has forelegs");
        let reach = |speed: f32| {
            let v = mesh(Species::Deer, 0.0, speed, std::f32::consts::FRAC_PI_2 / STRIDE);
            // Along the animal, which at a yaw of nought is the world's x:
            // the model faces -Z in its own frame and a yaw of nought puts
            // that on +X (see `push_posed_box`).
            let (lo, hi) = range_of(&v[leg * 24..leg * 24 + 24], 0);
            hi - lo
        };
        let still = reach(0.0);
        let mut previous = still;
        // Every hundredth of a block a second from a stop to a walk.
        for step in 0..=120 {
            let speed = step as f32 * 0.01;
            let now = reach(speed);
            assert!(
                (now - previous).abs() < 0.01,
                "the deer's leg jumped {:.3} blocks between {:.2} and {:.2} blocks a second",
                now - previous,
                speed - 0.01,
                speed
            );
            previous = now;
        }
        assert!(
            previous > still + 0.05,
            "a deer walking reaches {previous:.2} against {still:.2} standing: nothing swings at all"
        );
    }


    use super::*;

    /// **Placing a part's corners once changes no vertex**: every corner of
    /// every face, looked up, is the very bits `posed_local` gives for it --
    /// every part of every species, standing, swung and rolled over.
    #[test]
    fn placing_the_corners_once_changes_no_vertex() {
        for &species in Species::ALL {
            for part in parts(species) {
                for swing in [0.0f32, 0.37, -0.8] {
                    for pose in [
                        STANDING_POSE,
                        Pose { roll: 0.4, shift: Vec3::new(0.1, -0.2, 0.3), scale: 1.0 },
                    ] {
                        let placed = placed_corners(part, swing, pose);
                        for face in crate::engine::mesh::faces() {
                            for corner in face.corners {
                                let asked = posed_local(part, corner, swing, pose.roll) - pose.shift;
                                assert_eq!(
                                    placed[corner_bits(corner)].to_array().map(f32::to_bits),
                                    asked.to_array().map(f32::to_bits),
                                    "{}'s {} at corner {corner:?}, swing {swing}",
                                    species.name(),
                                    part.name
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// What placing the corners once saves on a full field, against asking
    /// `posed_local` for every corner of every face -- the two interleaved
    /// in rounds and the medians taken, because the machine this runs on is
    /// usually compiling something else.
    ///
    /// ```text
    /// cargo test -p primitive_client --release --lib what_placing_the_corners_once_saves -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_placing_the_corners_once_saves() {
        use primitive_shared::animals::MAX_ANIMALS;
        let field: Vec<&Part> = (0..MAX_ANIMALS)
            .flat_map(|i| parts(Species::ALL[i % Species::ALL.len()]).iter())
            .collect();
        let faces = crate::engine::mesh::faces();
        let (mut every, mut once) = (Vec::new(), Vec::new());
        for round in 0..21 {
            let swing = round as f32 * 0.05;
            let started = std::time::Instant::now();
            for part in &field {
                for face in &faces {
                    for corner in &face.corners {
                        std::hint::black_box(posed_local(part, *corner, swing, 0.0) - STANDING_POSE.shift);
                    }
                }
            }
            every.push(started.elapsed().as_secs_f64() * 1e6);
            let started = std::time::Instant::now();
            for part in &field {
                let placed = placed_corners(part, swing, STANDING_POSE);
                for face in &faces {
                    for corner in &face.corners {
                        std::hint::black_box(placed[corner_bits(*corner)]);
                    }
                }
            }
            once.push(started.elapsed().as_secs_f64() * 1e6);
        }
        let median = |times: &mut Vec<f64>| {
            times.sort_by(f64::total_cmp);
            times[times.len() / 2]
        };
        println!(
            "[corners] {} parts on a field of {MAX_ANIMALS}: every corner asked {:.1} us, each corner once {:.1} us ({} posed_local calls against {})",
            field.len(),
            median(&mut every),
            median(&mut once),
            field.len() * 24,
            field.len() * 8
        );
    }

    // ---- the skeleton ----

    /// The skeleton of a species, and the living body it was measured from.
    fn skeleton_with_body(species: Species) -> (Vec<Part>, Part) {
        let body = *parts(species)
            .iter()
            .find(|part| part.name == "body")
            .expect("every animal has a body");
        (skeleton_parts(species), body)
    }

    fn sorted_sides(part: &Part) -> [f32; 3] {
        let mut sides = part.size;
        sides.sort_by(f32::total_cmp);
        sides
    }

    fn volume(part: &Part) -> f32 {
        part.size[0] * part.size[1] * part.size[2]
    }

    /// **Not the animal with a bone texture on each part.**
    ///
    /// That is how a player described the skeleton this replaced, and it
    /// was accurate: its skull was the head shrunk by a fifth, its jaw the
    /// muzzle narrowed, its legs the legs made thinner -- solid bone the
    /// shape of the living parts. What a skeleton has that a body has not
    /// is *emptiness*, so that is the property: the bones together fill no
    /// more than a third of the body they are the frame of, and a skull --
    /// mostly an eye socket and an open mouth -- no more than two fifths
    /// of the head and muzzle it was inside. The old skull filled half.
    /// Every species that can lie in the world as a carcass and so rot to a
    /// skeleton -- which is every species but the two that swim
    /// (`Species::carcass`). The skeleton and carcass tests are sentences
    /// about a body on the ground; a fish never leaves one, and a skeleton
    /// built from a fish's fins would be a test of a thing no world holds.
    fn with_bones() -> impl Iterator<Item = Species> {
        Species::ALL.iter().copied().filter(|s| s.carcass().is_some())
    }

    #[test]
    fn a_skeleton_is_not_the_living_animal_painted_in_bone() {
        for species in with_bones() {
            let (bones, body) = skeleton_with_body(species);
            let living = parts(species);
            let total: f32 = bones.iter().map(volume).sum();
            assert!(
                total <= volume(&body) / 3.0,
                "{}'s bones fill {:.0}% of its body",
                species.name(),
                100.0 * total / volume(&body)
            );
            let head: f32 = living
                .iter()
                .filter(|part| matches!(part.name, "head" | "muzzle" | "snout" | "beak"))
                .map(volume)
                .sum();
            let skull: f32 = bones
                .iter()
                .filter(|bone| matches!(bone.name, "skull" | "snout" | "jaw" | "beak"))
                .map(volume)
                .sum();
            assert!(
                skull <= head * 0.4,
                "{}'s skull fills {:.0}% of its head: that is a head",
                species.name(),
                100.0 * skull / head
            );
            // ...and nothing soft survives the rot.
            assert!(
                !bones.iter().any(|bone| bone.name.contains("ear") || bone.name == "hump"),
                "{} kept its ears or its fat",
                species.name()
            );
        }
    }

    /// **A ribcage you can see the grass through.**
    ///
    /// The old "ribs" were four planks hung down each flank, as deep as the
    /// chest. What makes a chest read as ribs is that each one is thin and
    /// there is daylight between one and the next: at least three arches,
    /// a post at either end of each, every piece far longer than it is
    /// thick, and between two neighbours a gap at least as wide as a rib.
    #[test]
    fn a_ribcage_is_separate_thin_ribs_with_daylight_between_them() {
        for species in with_bones() {
            let bones = skeleton_parts(species);
            let mut arches: Vec<&Part> = bones.iter().filter(|bone| bone.name == "rib arch").collect();
            let sides = bones.iter().filter(|bone| bone.name == "rib").count();
            assert!(arches.len() >= 3, "{} has {} ribs: that is not a ribcage", species.name(), arches.len());
            assert_eq!(sides, arches.len() * 2, "{}: not a rib down each side of every arch", species.name());
            arches.sort_by(|a, b| a.at[2].total_cmp(&b.at[2]));
            for pair in arches.windows(2) {
                let gap = (pair[1].at[2] - pair[1].size[2] * 0.5) - (pair[0].at[2] + pair[0].size[2] * 0.5);
                assert!(
                    gap >= pair[0].size[2],
                    "{}: {gap:.2} of daylight between ribs {:.2} thick",
                    species.name(),
                    pair[0].size[2]
                );
            }
            for rib in bones.iter().filter(|bone| bone.name.starts_with("rib")) {
                let [thin, _, long] = sorted_sides(rib);
                assert!(long >= 3.0 * thin, "{}'s {} is a slab, not a rib: {:?}", species.name(), rib.name, rib.size);
            }
        }
    }

    /// **A spine is a column of knuckles along the body, not a bar.**
    ///
    /// Separate vertebrae, in one straight line, reaching over at least
    /// three quarters of the living body's length and many times longer
    /// than they are thick.
    #[test]
    fn a_spine_is_a_column_of_vertebrae_the_length_of_the_body() {
        for species in with_bones() {
            let (bones, body) = skeleton_with_body(species);
            let vertebrae: Vec<&Part> = bones.iter().filter(|bone| bone.name == "vertebra").collect();
            assert!(vertebrae.len() >= 3, "{}'s spine is {} bones", species.name(), vertebrae.len());
            let from = vertebrae.iter().map(|v| v.at[2] - v.size[2] * 0.5).fold(f32::MAX, f32::min);
            let to = vertebrae.iter().map(|v| v.at[2] + v.size[2] * 0.5).fold(f32::MIN, f32::max);
            // Up, not across: lying on its side a vertebra is wider across
            // the body than it is tall by its spinous process, and a spike
            // is not the thickness of a column.
            let thick = vertebrae[0].size[1];
            assert!(
                to - from >= 0.75 * body.size[2],
                "{}'s spine runs {:.1} of a body {:.1} long",
                species.name(),
                to - from,
                body.size[2]
            );
            assert!(to - from >= 6.0 * thick, "{}'s spine is a stub: {:.1} long, {thick:.1} thick", species.name(), to - from);
            for v in &vertebrae {
                assert!(
                    (v.at[0] - vertebrae[0].at[0]).abs() < 1e-6 && (v.at[1] - vertebrae[0].at[1]).abs() < 1e-6,
                    "{}'s spine is not in a line",
                    species.name()
                );
            }
        }
    }

    /// **A skull, a pelvis, and four limbs of two long bones each** -- the
    /// bird's front pair being its wings. Long means long: three times as
    /// long as thick would be a stick, two and a half is the least a leg
    /// bone reads as one at.
    #[test]
    fn a_skeleton_has_a_skull_a_pelvis_and_four_limbs_of_two_bones() {
        for species in with_bones() {
            let bones = skeleton_parts(species);
            assert!(bones.iter().any(|bone| bone.name == "skull"), "{} has no skull", species.name());
            assert!(bones.iter().filter(|bone| bone.name == "pelvis").count() >= 2, "{} has no pelvis", species.name());
            let limbs: Vec<&Part> = bones
                .iter()
                .filter(|bone| bone.name.starts_with("upper ") || bone.name.starts_with("lower "))
                .collect();
            assert_eq!(limbs.len(), 8, "{} has {} limb bones, not two for each of four limbs", species.name(), limbs.len());
            for limb in limbs {
                let [thin, _, long] = sorted_sides(limb);
                assert!(long >= 2.5 * thin, "{}'s {} is a lump: {:?}", species.name(), limb.name, limb.size);
            }
        }
    }

    /// Every corner of a bone as it is drawn, in sixteenths: through
    /// `posed_local`, the arithmetic that places the corners, rather than
    /// read back off the numbers that went into it.
    fn drawn_corners(bone: &Part) -> Vec<Vec3> {
        let mut corners = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    corners.push(posed_local(bone, [x, y, z], 0.0, 0.0) / SCALE);
                }
            }
        }
        corners
    }

    /// Which way a bone lying on the ground points as it is drawn: from the
    /// middle of its -X end to the middle of its +X end.
    fn drawn_axis(bone: &Part) -> Vec3 {
        (posed_local(bone, [1.0, 0.5, 0.5], 0.0, 0.0) - posed_local(bone, [0.0, 0.5, 0.5], 0.0, 0.0)).normalize()
    }

    /// **The legs lie out from the belly in parallel pairs, not splayed
    /// like a frog's.**
    ///
    /// The skeleton before this lay on its belly with a leg out to either
    /// side at the shoulder and at the hip, and that is how a player
    /// described it: "like frogs". An animal that died lying down lies on
    /// its side with its legs together, stretched out from its belly. So,
    /// of every leg bone as drawn: every corner is on the belly side of the
    /// spine, and it lies on the ground; it points away from the belly,
    /// less than an eighth of a turn from square to the body -- a leg
    /// pointing along the spine is the splay again, seen from the side;
    /// the two bones of a pair point the same way; and the forelegs lean
    /// toward the skull and the hind legs toward the tail, which is what
    /// says the animal stretched out rather than curled up.
    #[test]
    fn a_skeletons_legs_lie_out_from_its_belly_in_parallel_pairs() {
        for species in with_bones() {
            let bones = skeleton_parts(species);
            // The belly face of the spine: the side of it the legs are on.
            let spine = bones
                .iter()
                .filter(|bone| bone.name == "vertebra")
                .map(|bone| bone.at[0] + bone.size[0] * 0.5)
                .fold(f32::MIN, f32::max);
            let legs: Vec<&Part> = bones.iter().filter(|bone| bone.name.contains("leg")).collect();
            let wanted = if species.flies() { 4 } else { 8 };
            assert_eq!(legs.len(), wanted, "{} has {} leg bones", species.name(), legs.len());
            for leg in &legs {
                let corners = drawn_corners(leg);
                assert!(
                    corners.iter().all(|corner| corner.x > spine),
                    "{}'s {} reaches back past its spine: a leg on the wrong side of the body",
                    species.name(),
                    leg.name
                );
                let low = corners.iter().map(|corner| corner.y).fold(f32::MAX, f32::min);
                assert!(low.abs() < SEAM_BITE, "{}'s {} is not lying on the ground", species.name(), leg.name);
                let axis = drawn_axis(leg);
                assert!(
                    axis.y.abs() < 1e-4 && axis.x > std::f32::consts::FRAC_PI_4.cos(),
                    "{}'s {} points {axis:?}, not out from the belly",
                    species.name(),
                    leg.name
                );
                let (lean, toward) = if leg.name.contains("fore") { (-axis.z, "skull") } else { (axis.z, "tail") };
                assert!(lean > 0.0, "{}'s {} does not lean toward the {toward}", species.name(), leg.name);
            }
            for leg in &legs {
                let axis = drawn_axis(leg);
                let twins: Vec<&&Part> = legs.iter().filter(|other| other.name == leg.name).collect();
                assert_eq!(twins.len(), 2, "{}'s {} is not one of a pair", species.name(), leg.name);
                for twin in twins {
                    assert!(
                        drawn_axis(twin).dot(axis) > 0.9999,
                        "{}'s two {}s do not lie parallel",
                        species.name(),
                        leg.name
                    );
                }
            }
        }
    }

    /// **A skeleton lies the way its carcass lay.**
    ///
    /// It takes the carcass's place in the same cell with the same yaw, and
    /// the skeleton before this -- on its belly, legs out both ways -- was
    /// the animal turning over in the grass as it rotted. Measured in the
    /// world, from what each of them draws: from the middle of the
    /// carcass's body its legs lie one way, and from the middle of the
    /// skeleton's spine its legs have to lie the same way, give or take an
    /// eighth of a turn; along the body, at right angles to that, the skull
    /// has to be at the end the head was. Two directions fix a lying body
    /// entirely -- the only turn that keeps both is none, and a mirror
    /// flips one -- so that is the whole of "the same side".
    #[test]
    fn a_skeleton_lies_on_the_side_its_carcass_lay_on() {
        // Every box is 24 vertices, in the order its parts are listed
        // (`append_part_posed`); the middle of a box is the mean of them.
        fn middles(vertices: &[crate::engine::mesh::Vertex]) -> Vec<Vec3> {
            vertices
                .chunks_exact(24)
                .map(|corners| corners.iter().map(|x| Vec3::from_array(x.position)).sum::<Vec3>() / 24.0)
                .collect()
        }
        fn mean_of(parts: &[Part], middles: &[Vec3], wanted: impl Fn(&str) -> bool) -> Vec3 {
            let chosen: Vec<Vec3> =
                parts.iter().zip(middles).filter(|(part, _)| wanted(part.name)).map(|(_, middle)| *middle).collect();
            assert!(!chosen.is_empty(), "no part matched");
            chosen.iter().sum::<Vec3>() / chosen.len() as f32
        }
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize();
        let layers = FaceLayers::empty_for_test();
        let ground = Vec3::new(3.5, 7.0, -2.5);
        for species in with_bones() {
            for yaw in [0.0f32, 1.1, 2.6, 4.4] {
                let (mut v, mut i) = (Vec::new(), Vec::new());
                build_fallen(species, ground, yaw, 0, &layers, (15, 0), &mut v, &mut i);
                // A carcass is drawn with the parts an animal on the ground
                // shows, so those are what its boxes line up with.
                let living: Vec<Part> = parts(species).iter().copied().filter(|part| part.gait.shown(false)).collect();
                let living = living.as_slice();
                let carcass = middles(&v);
                let body = mean_of(living, &carcass, |name| name == "body");
                let head = mean_of(living, &carcass, |name| name == "head");
                let hooves = mean_of(living, &carcass, |name| {
                    name.starts_with("foreleg")
                        || name.starts_with("hind leg")
                        || name.starts_with("haunch")
                        || name.starts_with("leg ")
                });

                let (mut v, mut i) = (Vec::new(), Vec::new());
                build_bones(species, ground, yaw, &layers, (15, 0), &mut v, &mut i);
                let bones = skeleton_parts(species);
                let skeleton = middles(&v);
                let spine = mean_of(&bones, &skeleton, |name| name == "vertebra");
                let skull = mean_of(&bones, &skeleton, |name| name == "skull");
                let legs = mean_of(&bones, &skeleton, |name| name.contains("leg"));

                let out = flat(hooves - body);
                assert!(
                    flat(legs - spine).dot(out) > std::f32::consts::FRAC_PI_4.cos(),
                    "{} at yaw {yaw}: the carcass's legs lie toward {out:?} and its skeleton's toward {:?}",
                    species.name(),
                    flat(legs - spine)
                );
                let along = Vec3::new(-out.z, 0.0, out.x);
                assert!(
                    (head - body).dot(along) * (skull - spine).dot(along) > 0.0,
                    "{} at yaw {yaw}: the skull lies at the other end from where the carcass's head was",
                    species.name()
                );
            }
        }
    }

    /// **A skeleton still says which animal it was.** Bone that grew on the
    /// living head is bone still: the deer's skeleton carries its antlers
    /// standing out of the crown of its skull -- which, on a skull lying on
    /// its side, faces the back -- the boar's its tusks, the bird's skull
    /// ends in a beak that reaches past its snout -- and the bird's
    /// forelimbs are wings, where everybody else's are legs.
    #[test]
    fn a_skeleton_keeps_what_its_species_is_known_by() {
        for species in with_bones() {
            let bones = skeleton_parts(species);
            let count = |name: &str| bones.iter().filter(|bone| bone.name == name).count();
            let dorsal = |bone: &Part| bone.at[0] - bone.size[0] * 0.5;
            let crown = bones.iter().filter(|bone| bone.name == "skull").map(dorsal).fold(f32::MAX, f32::min);
            let (antlers, tusks, horns) = (count("antler"), count("tusk"), count("horn"));
            let wings = count("upper wing") + count("lower wing");
            match species {
                // The horn cores, out of the crown like the deer's antlers.
                Species::Antelope => {
                    assert_eq!(horns, 2, "the antelope's skeleton has lost its horns");
                    assert!(
                        bones.iter().filter(|bone| bone.name == "horn").all(|bone| dorsal(bone) < crown),
                        "the antelope's horns do not stand out of its crown"
                    );
                }
                Species::Deer => {
                    assert_eq!(antlers, 4, "the deer's skeleton has lost its antlers");
                    assert!(
                        bones.iter().filter(|bone| bone.name == "antler").all(|bone| dorsal(bone) < crown),
                        "the deer's antlers do not stand out of its crown"
                    );
                }
                Species::Boar => assert_eq!(tusks, 2, "the boar's skeleton has lost its tusks"),
                // Every bird, the gull as well as the grouse.
                bird if bird.flies() => {
                    let snout = bones
                        .iter()
                        .filter(|bone| bone.name == "snout")
                        .map(|bone| bone.at[2] - bone.size[2] * 0.5)
                        .fold(f32::MAX, f32::min);
                    let beak = bones.iter().find(|bone| bone.name == "beak").expect("the bird's skull has no beak");
                    assert!(beak.at[2] - beak.size[2] * 0.5 < snout, "the bird's beak does not reach past its snout");
                    assert_eq!(wings, 4, "the bird's skeleton has no wings");
                }
                _ => {}
            }
            if species != Species::Deer {
                assert_eq!(antlers, 0, "{} grew antlers in death", species.name());
            }
            if species != Species::Boar {
                assert_eq!(tusks, 0, "{} grew tusks in death", species.name());
            }
            if !species.flies() {
                assert_eq!(wings, 0, "{} grew wings in death", species.name());
            }
            if species != Species::Antelope {
                assert_eq!(horns, 0, "{} grew horns in death", species.name());
            }
        }
    }

    /// **No two bones fight over one plane.**
    ///
    /// A skeleton is all joints, and a joint is two boxes running into each
    /// other; if a face of each lands in the same plane, facing the same
    /// way, over the same patch, the depth buffer picks a winner by rounding
    /// and the joint shimmers as the camera moves. `SEAM_BITE` cannot help,
    /// because it grows both boxes alike. So: any two same-facing faces that
    /// cover one another are at least `CLEARANCE` apart -- except the
    /// undersides lying on the ground, which nothing above the ground can
    /// see.
    ///
    /// **Measured on the faces as drawn** (`posed_local`), not on centres
    /// and sizes. It was centres and sizes while every bone was square to
    /// the body; a leg lying at an angle is a box whose sides are on no
    /// axis, and a comparison of `at` and `size` cannot see them. "Cover"
    /// means by more than twice the `SEAM_BITE` the drawn boxes are grown
    /// by -- two boxes that only touch overlap by one.
    #[test]
    fn no_two_bones_of_a_skeleton_fight_over_one_plane() {
        use crate::engine::mesh::faces;
        type Face = (Vec3, [Vec3; 4]);
        // Each face of a bone as drawn, in sixteenths: its outward normal
        // and its corners.
        let drawn = |bone: &Part| -> Vec<Face> {
            faces()
                .iter()
                .map(|face| {
                    let c = face.corners.map(|corner| posed_local(bone, corner, 0.0, 0.0) / SCALE);
                    ((c[1] - c[0]).cross(c[2] - c[1]).normalize(), c)
                })
                .collect()
        };
        // How far two quads in parallel planes overlap, seen along their
        // normal: the least overlap of their shadows on any edge of either
        // (separating axes -- for rectangles, the edges are the axes).
        let overlap = |a: &[Vec3; 4], b: &[Vec3; 4]| -> f32 {
            let mut least = f32::MAX;
            for quad in [a, b] {
                for k in 0..4 {
                    let edge = (quad[(k + 1) % 4] - quad[k]).normalize();
                    let shadow = |q: &[Vec3; 4]| {
                        q.iter().map(|p| p.dot(edge)).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)))
                    };
                    let ((a_lo, a_hi), (b_lo, b_hi)) = (shadow(a), shadow(b));
                    least = least.min(a_hi.min(b_hi) - a_lo.max(b_lo));
                }
            }
            least
        };
        for species in with_bones() {
            let bones = skeleton_parts(species);
            let boxes: Vec<Vec<Face>> = bones.iter().map(drawn).collect();
            let ground = boxes.iter().flatten().flat_map(|(_, c)| c.map(|p| p.y)).fold(f32::MAX, f32::min);
            for (index, a) in boxes.iter().enumerate() {
                for (other, b) in boxes.iter().enumerate().skip(index + 1) {
                    for (normal, quad_a) in a {
                        for (normal_b, quad_b) in b {
                            if normal.dot(*normal_b) < 0.9999 {
                                continue;
                            }
                            let apart = normal.dot(quad_b[0] - quad_a[0]).abs();
                            if apart >= CLEARANCE - 1e-3 {
                                continue;
                            }
                            let on_the_ground = normal.y < -0.9999
                                && quad_a.iter().chain(quad_b.iter()).all(|p| (p.y - ground).abs() < 1e-3);
                            if on_the_ground {
                                continue;
                            }
                            let covered = overlap(quad_a, quad_b);
                            assert!(
                                covered <= 2.0 * SEAM_BITE,
                                "{}: the {} at {:?} and the {} at {:?} have faces {apart:.3} apart facing {normal:?}, overlapping by {covered:.3}",
                                species.name(),
                                bones[index].name,
                                bones[index].at,
                                bones[other].name,
                                bones[other].at
                            );
                        }
                    }
                }
            }
        }
    }

    /// **A skeleton wears solid ivory, not the bone item's icon.**
    ///
    /// It wore `hide/bone.png`: a bone drawn on a transparent square. A
    /// material on a small face shows the top-left corner of its picture,
    /// and that corner of the icon is empty, so thin bones wore crops of
    /// transparent texels and outline and big ones wore the whole icon.
    /// Two halves: every face of every skeleton names the tusk tile, and
    /// that tile on disk has no transparent texel for a crop to land on.
    #[test]
    fn a_skeleton_wears_solid_ivory_rather_than_the_bone_icon() {
        use crate::engine::texture::{SHEET_COLUMNS, SHEET_ROWS};
        let layers = FaceLayers::empty_for_test();
        let ivory = layers.animal(Species::Boar, Skin::Tusk.slot());
        assert_ne!(ivory, layers.layer_for_face(primitive_shared::types::BLOCK_BONE, 0));
        for species in with_bones() {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            build_bones(species, Vec3::ZERO, 0.0, &layers, (15, 0), &mut v, &mut i);
            assert!(!v.is_empty() && v.iter().all(|x| x.tex_layer() == ivory), "{} is not in ivory", species.name());
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/textures/animals/boar.png");
        let sheet = image::open(path).expect("the boar's sheet loads").to_rgba8();
        let (w, h) = (sheet.width() / SHEET_COLUMNS, sheet.height() / SHEET_ROWS);
        let slot = Skin::Tusk.slot() as u32;
        let (x0, y0) = (slot % SHEET_COLUMNS * w, slot / SHEET_COLUMNS * h);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                assert_eq!(sheet.get_pixel(x, y).0[3], 255, "the ivory tile has a hole at ({x}, {y})");
            }
        }
    }

    /// **A skeleton lies on the cell it is placed in**: its lowest point a
    /// hair above the ground, not in it and not hanging over it, and the
    /// middle of its footprint on the cell's centre -- measured from the
    /// bones, not from the living body they were a narrower part of.
    /// **A sheep sheared yesterday does not look like one ready today**
    /// (`horse::TACK_SHORN`): its body is narrower and lower-backed, and the
    /// same bit on any other animal changes nothing.
    #[test]
    fn a_shorn_sheep_is_drawn_slimmer_than_one_in_full_fleece() {
        let layers = FaceLayers::empty_for_test();
        let extent = |species: Species, tack: u8| {
            let (mut v, mut i) = (Vec::new(), Vec::new());
            build(species, Vec3::ZERO, 0.0, Motion { tack, ..Motion::default() }, &layers, (15, 0), &mut v, &mut i);
            let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for vertex in &v {
                low = low.min(Vec3::from_array(vertex.position));
                high = high.max(Vec3::from_array(vertex.position));
            }
            // Across the animal: at a yaw of nought its length lies along x.
            (high.z - low.z, high.y)
        };
        let (full_width, full_top) = extent(Species::Sheep, 0);
        let (bare_width, bare_top) = extent(Species::Sheep, TACK_SHORN);
        assert!(bare_width < full_width - 0.05, "a shorn sheep is as broad as a fleeced one: {bare_width} against {full_width}");
        assert!(bare_top <= full_top, "shearing a sheep made it taller");
        assert_eq!(extent(Species::Deer, TACK_SHORN), extent(Species::Deer, 0), "the shorn bit reshaped a deer");
    }

    #[test]
    fn a_skeleton_lies_on_the_ground_it_is_placed_on() {
        let layers = FaceLayers::empty_for_test();
        let ground = Vec3::new(3.5, 7.0, -2.5);
        for species in with_bones() {
            for turn in 0..4 {
                let yaw = turn as f32 * std::f32::consts::FRAC_PI_2;
                let (mut v, mut i) = (Vec::new(), Vec::new());
                build_bones(species, ground, yaw, &layers, (15, 0), &mut v, &mut i);
                let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for vertex in &v {
                    low = low.min(Vec3::from_array(vertex.position));
                    high = high.max(Vec3::from_array(vertex.position));
                }
                assert!(
                    low.y > ground.y && low.y - ground.y < 1.0 / 32.0,
                    "{} at a turn of {turn}: its lowest point is {:.4} above the ground",
                    species.name(),
                    low.y - ground.y
                );
                let middle = (low + high) * 0.5;
                assert!(
                    (middle.x - ground.x).abs() < 1e-3 && (middle.z - ground.z).abs() < 1e-3,
                    "{} at a turn of {turn} is not centred on its cell: {middle:?}",
                    species.name()
                );
            }
        }
    }

    /// **A skeleton is a prop, and costs like one.** A few dozen boxes --
    /// the big animals about four times their living model -- baked once
    /// into a chunk. The number is here so that adding a bone is a
    /// decision somebody sees.
    #[test]
    fn a_skeleton_is_a_few_dozen_boxes_rather_than_a_sculpture() {
        for species in with_bones() {
            let bones = skeleton_parts(species).len();
            println!("{}: {bones} boxes, {} for the living model", species.name(), parts(species).len());
            assert!(bones <= 60, "{}'s skeleton is {bones} boxes", species.name());
        }
    }

    /// The quads the cracks are drawn on are the quads the carcass is
    /// drawn with, corner for corner: two copies of the same arithmetic,
    /// held together here so a crack can never float beside the flank.
    #[test]
    fn the_cracks_of_a_carcass_lie_exactly_on_its_quads() {
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        for species in Species::ALL {
            let ground = Vec3::new(3.5, 7.0, -2.5);
            let yaw = carcass_yaw(3, 7, -3);
            let (mut v, mut i) = (Vec::new(), Vec::new());
            build_fallen(*species, ground, yaw, 0, &layers, (15, 0), &mut v, &mut i);
            let mut quads = Vec::new();
            fallen_quads(*species, ground, yaw, &mut quads);
            assert_eq!(v.len(), quads.len() * 4, "{species:?}: a different number of quads");
            for (quad, drawn) in quads.iter().zip(v.chunks_exact(4)) {
                for (corner, vertex) in quad.iter().zip(drawn) {
                    for axis in 0..3 {
                        assert!(
                            (corner[axis] - vertex.position[axis]).abs() < 1e-6,
                            "{species:?}: crack corner {corner:?} is not vertex {:?}",
                            vertex.position
                        );
                    }
                }
            }
        }
    }

    /// The size of the box the model actually fills, as (width, height,
    /// length) in blocks.
    ///
    /// The *span* rather than `half_extents`, which measures from the
    /// model's own origin: a boar's head is all at one end, so the two
    /// are not the same number and the tests below want both.
    fn extent(species: Species) -> (f32, f32, f32) {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        // Standing: see `Gait::shown`.
        for part in parts(species).iter().filter(|part| part.gait.shown(false)) {
            for axis in 0..3 {
                low[axis] = low[axis].min(part.at[axis] - part.size[axis] * 0.5);
                high[axis] = high[axis].max(part.at[axis] + part.size[axis] * 0.5);
            }
        }
        (
            (high[0] - low[0]) * SCALE,
            (high[1] - low[1]) * SCALE,
            (high[2] - low[2]) * SCALE,
        )
    }

    /// Where the test fixtures stand.
    const TEST_CENTRE: Vec3 = Vec3::new(10.0, 0.0, -5.0);

    /// One animal's vertices, for the tests that read them.
    ///
    /// `speed` before `walked`, which is the order the tests read best
    /// in: how fast it is going, and then how far it has come.
    fn mesh(species: Species, yaw: f32, speed: f32, walked: f32) -> Vec<crate::engine::mesh::Vertex> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        build(
            species,
            // Somewhere that is not the origin, so a test can tell "put
            // where the server said" from "built around zero".
            TEST_CENTRE,
            yaw,
            Motion { walked, speed, ..Default::default() },
            &FaceLayers::empty_for_test(),
            (15, 0),
            &mut vertices,
            &mut indices,
        );
        vertices
    }

    /// Lowest and highest of one coordinate over a mesh.
    fn range_of(vertices: &[crate::engine::mesh::Vertex], axis: usize) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.position[axis]), hi.max(v.position[axis])))
    }

    #[test]
    fn a_newborn_is_drawn_at_its_birth_size_about_its_middle() {
        // The client draws what the server collides: `youth::size` of the
        // adult, every dimension, about the centre the snapshot carries --
        // so a fawn stands on the ground its smaller box stands on.
        for &species in Species::ALL {
            let adult = posed(species, Motion::default());
            let newborn = posed(species, Motion { youth: 1.0, ..Default::default() });
            for axis in 0..3 {
                let (a, b) = (range_of(&adult, axis), range_of(&newborn, axis));
                let ratio = (b.1 - b.0) / (a.1 - a.0);
                assert!(
                    (ratio - primitive_shared::youth::BIRTH_SIZE).abs() < 1e-3,
                    "a newborn {} is {ratio:.3} of the adult along axis {axis}",
                    species.name()
                );
                let (mid_a, mid_b) = ((a.0 + a.1) * 0.5, (b.0 + b.1) * 0.5);
                let off = (mid_a - TEST_CENTRE[axis]) * primitive_shared::youth::BIRTH_SIZE - (mid_b - TEST_CENTRE[axis]);
                assert!(off.abs() < 1e-3, "a newborn {} was shrunk about somewhere else", species.name());
            }
        }
    }

    #[test]
    fn a_dying_animal_ends_on_its_side_in_the_carcass_pose() {
        // The roll starts upright and ends lying: lower than it stood, and
        // with its underside on the ground under its middle -- the pose the
        // carcass block is drawn in, so the handover has nothing to jump.
        for &species in [Species::Deer, Species::Boar, Species::Sheep].iter() {
            let standing = posed(species, Motion::default());
            let lying = posed(species, Motion { fallen: 1.0, ..Default::default() });
            let lie = range_of(&lying, 1);
            // Lying down, and not merely lowered: the flank is where the back
            // was, so the model's own width is now its height.
            let (across, _, _) = species.half_extents();
            assert!(
                ((lie.1 - lie.0) - 2.0 * across).abs() < (lie.1 - lie.0) * 0.5,
                "a dead {} is {:.2} tall and {:.2} across: not on its side",
                species.name(),
                lie.1 - lie.0,
                2.0 * across
            );
            assert!(standing.iter().zip(&lying).any(|(a, b)| (a.position[1] - b.position[1]).abs() > 0.1));
            let ground = TEST_CENTRE[1] - species.height() * 0.5;
            assert!((lie.0 - ground - LIFT).abs() < 0.02, "a dead {} lies at {} over ground {ground}", species.name(), lie.0);
            // The very pose `build_fallen` lays the carcass in, placed on the
            // same ground under the same middle.
            let (mut v, mut i) = (Vec::new(), Vec::new());
            let under = Vec3::new(TEST_CENTRE[0], ground, TEST_CENTRE[2]);
            build_fallen(species, under, 0.0, 0, &FaceLayers::empty_for_test(), (15, 0), &mut v, &mut i);
            for axis in 0..3 {
                let (a, b) = (range_of(&lying, axis), range_of(&v, axis));
                assert!(
                    (a.0 - b.0).abs() < 0.03 && (a.1 - b.1).abs() < 0.03,
                    "a dead {} lies {a:?} and its carcass {b:?} along axis {axis}",
                    species.name()
                );
            }
        }
    }

    #[test]
    fn the_shared_hit_box_is_the_model_that_is_drawn() {
        // **The one check that keeps aiming honest.** The client aims
        // with `Species::half_extents` and the server validates a blow
        // with the same numbers, so if the drawn model outgrows them
        // there is a part of the animal you can see, aim at, and never
        // hit -- and if they outgrow the model, you hit it by aiming at
        // the grass beside it.
        //
        // The mapping matters as much as the sizes: the model's own x is
        // *across* the animal and its z is *along* it.
        for &species in Species::ALL {
            let drawn = half_extents(species);
            let (across, up, along) = species.half_extents();
            for (name, declared, measured) in [
                ("across", across, drawn.x),
                ("up", up, drawn.y),
                ("along", along, drawn.z),
            ] {
                assert!(
                    (declared - measured).abs() <= 0.02,
                    "{}: the hit box is {declared:.2} {name} and the model is {measured:.2}",
                    species.name()
                );
            }
        }
    }

    #[test]
    fn what_you_can_hit_is_the_size_of_what_you_can_see() {
        // The aim box is centred on the point the server sends and has
        // to *contain* the model -- which is not the same as being half
        // its bounding box, because an animal is not symmetric about its
        // own centre: a boar's head is all at one end, so the box that
        // holds it is bigger than half the span.
        //
        // Both halves are checked. Too small and there are parts of the
        // animal a swing passes through; much too large and you hit it
        // by aiming at the grass beside it.
        for &species in Species::ALL {
            let half = half_extents(species);
            let (width, height, length) = extent(species);
            for (name, box_side, span) in [
                ("width", half.x * 2.0, width),
                ("height", half.y * 2.0, height),
                ("length", half.z * 2.0, length),
            ] {
                assert!(
                    box_side >= span - 1e-3,
                    "{}: the aim box is {box_side:.2} across and the model is {span:.2} ({name})",
                    species.name()
                );
                assert!(
                    box_side <= span * 2.0 + 0.1,
                    "{}: the aim box is {box_side:.2} across for a model {span:.2} ({name})",
                    species.name()
                );
            }
        }
    }

    #[test]
    fn every_species_has_a_model_and_every_part_has_a_size() {
        for &species in Species::ALL {
            let parts = parts(species);
            assert!(!parts.is_empty(), "{} has no model", species.name());
            for part in parts {
                assert!(!part.name.is_empty(), "{}: an unnamed part", species.name());
                for axis in 0..3 {
                    assert!(
                        part.size[axis] > 0.0,
                        "{}: {} is flat on axis {axis}",
                        species.name(),
                        part.name
                    );
                }
            }
            // Legs, and an even number of them: three legs is a bug that
            // is hard to see in a table and impossible to miss in play.
            let legs = parts
                .iter()
                .filter(|p| matches!(p.gait, Gait::LegFront | Gait::LegBack))
                .count();
            // Four, and two for the bird -- the one thing in this world
            // that walks on two, and the reason this is a floor per
            // species rather than a flat four.
            // ...and two for a fish, whose "legs" are its pectoral fins
            // paddling in opposite phase (`FISH`).
            let fewest = if species.flies() || species.swims() { 2 } else { 4 };
            assert!(legs >= fewest, "{} has {legs} legs", species.name());
            assert_eq!(legs % 2, 0, "{} has an odd number of legs", species.name());
            // ...and they swing in opposite pairs, or four legs read as
            // a hop.
            let front = parts.iter().filter(|p| p.gait == Gait::LegFront).count();
            assert_eq!(front * 2, legs, "{}: the legs are all in step", species.name());
        }
    }

    #[test]
    fn a_model_is_the_size_of_the_animal_the_server_is_simulating() {
        // A model much smaller than its collider is an animal that looks
        // like a toy standing in a hole; one much larger is an animal
        // whose body is somewhere you cannot hit it.
        //
        // The slack is deliberately one-sided at the top. Ears and
        // antlers stand above the shoulder and the collider is the
        // *shoulder* height -- a hare whose ears had to fit inside its
        // own collider would be a hare with no ears.
        for &species in Species::ALL {
            let (width, height, _) = extent(species);
            assert!(
                height >= species.height() * 0.7,
                "{} is {height:.2} blocks tall against a collider of {:.2}",
                species.name(),
                species.height()
            );
            assert!(
                height <= species.height() + 0.3,
                "{} is {height:.2} blocks tall, well over its collider of {:.2}",
                species.name(),
                species.height()
            );
            assert!(
                width <= species.width() + 0.1,
                "{} is {width:.2} wide against a collider of {:.2}",
                species.name(),
                species.width()
            );
        }
    }

    #[test]
    fn an_animal_has_a_long_way_and_a_short_way_and_the_crab_lies_across_its_own() {
        // The one proportion that makes a box read as a body rather than as
        // a crate: a third longer one way than the other, so the silhouette
        // has a front and a flank.
        //
        // **The crab is the same proportion with the sign turned round**,
        // which is the animal: its long axis runs *across* its heading
        // (`Species::sidles`), and the shared hit box is turned the same way
        // -- see `every_animal_has_a_long_way_and_a_short_way_and_the_crab_is
        // _the_one_turned_sideways`, which is this property stated of the
        // numbers the server validates a blow with.
        for &species in Species::ALL {
            let (width, _, length) = extent(species);
            let (long, short) = if species.sidles() { (width, length) } else { (length, width) };
            assert!(
                long > short * 1.3,
                "{} is {length:.2} long and {width:.2} wide",
                species.name()
            );
        }
    }

    #[test]
    fn an_animal_faces_the_way_it_is_going() {
        // **The bug this test exists for**: every animal in the world
        // walked backwards, because the model's own axes were turned
        // into the world with a sign that mirrored them.
        //
        // The server's convention is the one to check against: a yaw of
        // zero moves an animal toward +X (see `logic::animals::walk`,
        // which steps by `cos(yaw)` along x), so at yaw zero the nose
        // has to be the +X end of the mesh.
        //
        // The nose is found by geometry rather than by name: the front
        // of a boar is the *narrow* end, so of the two extremes along
        // the direction of travel, the nose is the one with less of the
        // animal around it.
        let v = mesh(Species::Boar, 0.0, 0.0, 0.0);
        let lo = v.iter().map(|p| p.position[0]).fold(f32::MAX, f32::min);
        let hi = v.iter().map(|p| p.position[0]).fold(f32::MIN, f32::max);
        let width_at = |from: f32, to: f32| {
            let slab: Vec<f32> = v
                .iter()
                .filter(|p| p.position[0] >= from.min(to) && p.position[0] <= from.max(to))
                .map(|p| p.position[2])
                .collect();
            slab.iter().cloned().fold(f32::MIN, f32::max)
                - slab.iter().cloned().fold(f32::MAX, f32::min)
        };
        let front = width_at(hi, hi - 0.15);
        let back = width_at(lo, lo + 0.15);
        assert!(
            front < back,
            "the narrow end is at the back: {front:.2} wide in front, {back:.2} behind -- \
             the animal is facing the way it came from"
        );
    }

    #[test]
    fn turning_an_animal_turns_the_whole_of_it() {
        // Every vertex rides the same yaw. A part left in the model's own
        // space is a head that stays pointing north while the body turns.
        let straight = mesh(Species::Boar, 0.0, 0.0, 0.0);
        let turned = mesh(Species::Boar, std::f32::consts::FRAC_PI_2, 0.0, 0.0);
        assert_eq!(straight.len(), turned.len());
        let moved = straight
            .iter()
            .zip(&turned)
            .filter(|(a, b)| (a.position[0] - b.position[0]).abs() > 1e-3)
            .count();
        assert!(
            moved > straight.len() / 2,
            "only {moved} of {} corners moved when it turned",
            straight.len()
        );
        // ...and it turned *about itself* rather than swinging round the
        // world. Not by comparing bounding boxes -- an animal is not
        // symmetric front to back, so turning one genuinely moves the
        // middle of its box -- but by the thing that actually matters:
        // the point the server sent stays inside the animal, whichever
        // way it is facing.
        for mesh in [&straight, &turned] {
            for (axis, centre) in [(0usize, 10.0f32), (2, -5.0)] {
                let lo = mesh.iter().map(|v| v.position[axis]).fold(f32::MAX, f32::min);
                let hi = mesh.iter().map(|v| v.position[axis]).fold(f32::MIN, f32::max);
                assert!(
                    lo <= centre && centre <= hi,
                    "the animal is not standing where the server put it on axis {axis}"
                );
            }
        }
    }


    /// The same animal, drawn with something other than a plain walk.
    fn posed(species: Species, motion: Motion) -> Vec<crate::engine::mesh::Vertex> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        build(
            species,
            TEST_CENTRE,
            0.0,
            motion,
            &FaceLayers::empty_for_test(),
            (15, 0),
            &mut vertices,
            &mut indices,
        );
        vertices
    }

    /// How far the lowest-moving and highest-moving vertex of one drawing
    /// has shifted against another, vertically.
    ///
    /// The head is one box in the middle of a dozen, and it never reaches
    /// below the feet however far it is lowered -- so the extent of the model
    /// says nothing about where the head is, and what moved does.
    fn shifted(from: &[crate::engine::mesh::Vertex], to: &[crate::engine::mesh::Vertex]) -> (f32, f32) {
        from.iter()
            .zip(to)
            .map(|(a, b)| b.position[1] - a.position[1])
            .fold((0.0f32, 0.0f32), |(lo, hi), dy| (lo.min(dy), hi.max(dy)))
    }

    #[test]
    fn the_legs_go_round_once_per_stride_of_ground_and_never_once_per_second() {
        // **The promise the whole gait rests on**, and the one a player reads
        // as skating when it is broken: the pose is a function of `walked`
        // and of nothing else that moves. Same distance, every speed from a
        // crawl to a bolt -- the legs are in the same place, because it is
        // the ground that turns them. What speed decides is how *far* they
        // swing (`STANDING`, and the amplitude above it), never how fast.
        let reference = mesh(Species::Deer, 0.0, 4.0, 3.0);
        for speed in [4.0f32, 5.5, 7.0] {
            let same = mesh(Species::Deer, 0.0, speed, 3.0);
            for (a, b) in reference.iter().zip(&same) {
                assert_eq!(a.position, b.position, "the same ground covered drew a different leg at {speed}");
            }
        }
        // ...and a whole stride on is the same pose again. `STRIDE` turns
        // `walked` into radians, so a full turn of the cycle is a whole
        // number of blocks and the legs come back to where they started.
        let cycle = std::f32::consts::TAU / STRIDE;
        let started = mesh(Species::Deer, 0.0, 4.0, 3.0);
        let round_again = mesh(Species::Deer, 0.0, 4.0, 3.0 + cycle);
        for (a, b) in started.iter().zip(&round_again) {
            for axis in 0..3 {
                assert!(
                    (a.position[axis] - b.position[axis]).abs() < 1e-3,
                    "a whole stride on, the leg was somewhere else"
                );
            }
        }
    }

    #[test]
    fn a_grazing_animal_has_its_head_in_the_grass_and_a_watchful_one_has_it_up() {
        // What `Attitude` is for. The head is the only part that answers to
        // it, so the test is about how low the lowest thing drawn is: a deer
        // feeding reaches the ground, a deer looking about does not.
        let level = posed(Species::Deer, Motion::default());
        let feeding = posed(
            Species::Deer,
            Motion { head: head_carried(Attitude::Feeding), ..Default::default() },
        );
        let alert = posed(
            Species::Deer,
            Motion { head: head_carried(Attitude::Alert), ..Default::default() },
        );
        let (fed_down, _) = shifted(&level, &feeding);
        assert!(
            fed_down < -0.15,
            "the lowest thing a feeding deer moved went down {fed_down:.2}, which is not a head in the grass"
        );
        // Up on the whole. Turning a box about its own top always sends one
        // corner down as it sends another up -- what says which way the head
        // went is which of the two is bigger, and how far the muzzle got.
        let (alert_down, alert_up) = shifted(&level, &alert);
        assert!(
            alert_up > 0.02 && alert_up > -alert_down,
            "a deer looking about lifted {alert_up:.2} and dropped {alert_down:.2}, which is not a head coming up"
        );
        // ...and a drink is lower than a mouthful, because the water is.
        let drinking = posed(
            Species::Deer,
            Motion { head: head_carried(Attitude::Drinking), ..Default::default() },
        );
        assert!(shifted(&level, &drinking).0 < fed_down, "a drink was no lower than a graze");
    }

    #[test]
    fn an_animal_running_into_a_turn_leans_and_one_running_straight_does_not() {
        // See `LEAN_PER_TURN`. Measured across the body rather than along it:
        // a roll about the animal's own length lifts one flank and drops the
        // other, so what changes is how tall the drawing is and where its
        // widest points are.
        let straight = posed(Species::Deer, Motion { speed: 6.0, walked: 1.0, ..Default::default() });
        let banking = posed(
            Species::Deer,
            Motion { speed: 6.0, walked: 1.0, turning: 2.0, ..Default::default() },
        );
        let moved = straight
            .iter()
            .zip(&banking)
            .filter(|(a, b)| (a.position[2] - b.position[2]).abs() > 1e-3)
            .count();
        assert!(moved > 0, "a deer turning at six blocks a second was drawn bolt upright");
        // ...and a standing animal does not lean, whatever its facing is
        // doing: a lean is something speed does.
        let pivoting = posed(Species::Deer, Motion { turning: 2.0, ..Default::default() });
        let still = posed(Species::Deer, Motion::default());
        for (a, b) in pivoting.iter().zip(&still) {
            assert_eq!(a.position, b.position, "an animal turning on the spot leaned into it");
        }
    }

    #[test]
    fn a_struck_animal_is_knocked_about_and_an_untouched_one_is_not() {
        // See `STAGGER_ROLL`. The flash was on the wire and did nothing but
        // tint the box red; this is the body moving with the blow, off the
        // same number.
        let whole = posed(Species::Boar, Motion::default());
        let hit = posed(Species::Boar, Motion { hurt: Some(1.0), ..Default::default() });
        let shifted = whole
            .iter()
            .zip(&hit)
            .filter(|(a, b)| a.position != b.position)
            .count();
        assert!(shifted > 0, "a boar took a spear and did not move a hair");
        // ...and it comes back. A flinch that did not decay with the flash
        // would leave the animal leaning for the rest of its life.
        let recovering = posed(Species::Boar, Motion { hurt: Some(0.05), ..Default::default() });
        for (a, b) in whole.iter().zip(&recovering) {
            for axis in 0..3 {
                assert!(
                    (a.position[axis] - b.position[axis]).abs() < 0.05,
                    "the flinch had not worn off with the flash"
                );
            }
        }
    }

    #[test]
    fn a_tail_moves_while_the_animal_it_is_on_stands_still() {
        // See `secondary`. Everything but the legs and the head rides with
        // the body, which is right for a flank and wrong for the one part of
        // an animal that moves when nothing else does.
        let with_a_tail: Vec<Species> = Species::ALL
            .iter()
            .copied()
            .filter(|&s| parts(s).iter().any(|p| p.name.contains("tail")))
            .collect();
        assert!(
            !with_a_tail.is_empty(),
            "no model in the game has a part named tail, so this test is measuring nothing"
        );
        for species in with_a_tail {
            // Standing still at two moments of the same standing still:
            // nothing about the legs can differ, because `speed` is zero and
            // `STANDING` puts the swing at nought. Only the clock has moved.
            let a = posed(species, Motion::default());
            let b = posed(species, Motion { age: 0.9, ..Default::default() });
            assert!(
                a.iter().zip(&b).any(|(x, y)| x.position != y.position),
                "{species:?} has a tail and nothing on it ever moves"
            );
        }
    }

    #[test]
    fn standing_still_stands_still() {
        // A walk cycle that runs on a clock rather than on distance is an
        // animal marching on the spot.
        let a = mesh(Species::Boar, 0.0, 0.0, 3.0);
        let b = mesh(Species::Boar, 0.0, 0.0, 40.0);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.position, y.position);
        }
    }

    #[test]
    fn walking_swings_the_legs_and_leaves_the_body_alone() {
        let still = mesh(Species::Boar, 0.0, 0.0, 0.0);
        let mid_stride = mesh(Species::Boar, 0.0, 4.0, 0.4);
        // Along world X, because that is where the model's own length
        // lies at yaw zero -- see `extent`. A leg swings fore and aft,
        // which is along the animal, not across it.
        let moved: Vec<usize> = still
            .iter()
            .zip(&mid_stride)
            .enumerate()
            .filter(|(_, (a, b))| (a.position[0] - b.position[0]).abs() > 1e-3)
            .map(|(i, _)| i)
            .collect();
        assert!(!moved.is_empty(), "nothing moved while it walked");
        // The body is the first part in every model, and it must not be
        // among them.
        assert!(
            moved.iter().all(|&i| i >= 24),
            "the body swung with the legs"
        );
    }

    #[test]
    fn a_leg_swings_about_its_top_rather_than_its_middle() {
        // A leg pivoting on its knee walks through the ground.
        let still = mesh(Species::Boar, 0.0, 0.0, 0.0);
        let swung = mesh(Species::Boar, 0.0, 4.0, 0.4);
        let lowest = |v: &[crate::engine::mesh::Vertex]| {
            v.iter().map(|x| x.position[1]).fold(f32::MAX, f32::min)
        };
        // Swinging lifts the foot -- but not every corner of it. A box
        // turning about the middle of its top face swings its far
        // *bottom* corner on a radius longer than the leg is deep, so
        // that one corner dips a little below where it stood. That is
        // real, it is what a rotating box does, and at a tenth of a
        // block it is under the depth of the grass. What must not happen
        // is a leg pivoting on its knee, which buries the foot by half
        // its length.
        assert!(
            lowest(&swung) >= lowest(&still) - 0.15,
            "a swung leg sank {:.2} blocks into the ground",
            lowest(&still) - lowest(&swung)
        );
    }

    #[test]
    fn a_boar_wears_more_than_one_picture() {
        // The point of the `Skin` column: an animal drawn in a single
        // colour is the box it is made of.
        let skins: std::collections::HashSet<Skin> =
            parts(Species::Boar).iter().flat_map(|p| [p.skin].into_iter().chain(p.front)).collect();
        assert!(skins.len() >= 4, "the boar wears {} pictures", skins.len());
        assert!(skins.contains(&Skin::Tusk), "a boar with no tusks");
        assert!(skins.contains(&Skin::Face), "a boar with no face");
    }

    /// **The one animal in this world with nothing to hang an ear on.**
    ///
    /// Not an omission. A crab hears through the water and the sand, and a
    /// pair of fur-rimmed ears on a carapace would be a crab drawn as a
    /// small dog. It has a head and a muzzle like everything else -- the eye
    /// bar and the mouthparts (`animals/crab.bbmodel`) -- because those are
    /// real parts of a crab and because a body the rest of the game cannot
    /// find a head on is a body with no skeleton (`skeleton_parts`).
    ///
    /// The ear test names this rather than quietly skipping it, so the day
    /// somebody adds a second shelled animal they decide it on purpose.
    fn is_shelled(species: Species) -> bool {
        species == Species::Crab
    }

    /// The part every animal has one of.
    fn head_of(species: Species) -> &'static Part {
        parts(species)
            .iter()
            .find(|p| p.name == "head")
            .unwrap_or_else(|| panic!("{} has no head", species.name()))
    }

    #[test]
    fn a_head_wears_a_face_on_its_front_and_no_eye_on_its_top() {
        // **The bug this test exists for.** A head box has six faces and
        // exactly two of them have an eye in them. Three of the four
        // animals wore `Skin::Head` -- the eyed side view -- as the base
        // skin of the head, which is what the top of the skull, the back
        // of the neck and the underside of the jaw all get; and, having
        // no `front`, the front of the muzzle got it too. A deer seen
        // head-on had an eye where its nose is and a second eye on top.
        for &species in Species::ALL {
            let head = head_of(species);
            assert_ne!(
                head.skin,
                Skin::Head,
                "{}: the eyed side view is also on the top and back of the skull",
                species.name()
            );
            assert_eq!(
                head.front,
                Some(Skin::Face),
                "{}: the front of the head is a side view of the head",
                species.name()
            );
            assert_eq!(
                head.sides,
                Some((Skin::Head, Skin::HeadMirror)),
                "{}: the two sides of the skull are not a mirrored pair",
                species.name()
            );
        }
    }

    #[test]
    fn every_animal_has_a_muzzle_in_front_of_its_face() {
        // A face picture alone is a head that shades toward a nose that
        // is not there. The nostrils live on the end of a box of their
        // own, and that box is most of what separates a wolf's
        // silhouette from a deer's.
        for &species in Species::ALL {
            let head = head_of(species);
            let muzzle = parts(species)
                .iter()
                .find(|p| p.skin == Skin::Snout)
                .unwrap_or_else(|| panic!("{} has no muzzle", species.name()));
            assert_eq!(
                muzzle.front,
                Some(Skin::Nose),
                "{}: the end of the muzzle has no nostrils on it",
                species.name()
            );
            // In front of the head rather than inside it: -Z is forward.
            assert!(
                muzzle.at[2] < head.at[2],
                "{}: the muzzle is behind the face",
                species.name()
            );
            assert!(
                muzzle.size[0] < head.size[0],
                "{}: the muzzle is as wide as the skull",
                species.name()
            );
            // ...and *out* of the face, not merely centred in front of the
            // head's centre. That weaker check is all there was, and the
            // bear passed it for its whole life with a snout whose front
            // face was the head's front face: a flat face, and the capybara
            // a player called it. A quarter of the muzzle clear of the skull
            // at the least -- the bird's beak, the smallest, is three tenths.
            let front = |part: &Part| part.at[2] - part.size[2] * 0.5;
            let clear = front(head) - front(muzzle);
            assert!(
                clear >= muzzle.size[2] * 0.25,
                "{}: {clear:.2} of a muzzle {:.2} long stands out of the face",
                species.name(),
                muzzle.size[2]
            );
            // ...on the face, rather than standing over the brow.
            assert!(
                muzzle.at[1] + muzzle.size[1] * 0.5 <= head.at[1] + head.size[1] * 0.5,
                "{}: the muzzle stands over the top of the skull",
                species.name()
            );
        }
    }

    /// The top and the underside of an animal's trunk over a point along
    /// it: the highest top and the lowest bottom among the parts that ride
    /// with the body and cover `z`. The tail is not trunk.
    fn trunk_over(model: &[Part], z: f32) -> (f32, f32) {
        model
            .iter()
            .filter(|part| part.gait == Gait::Still && part.name != "tail")
            .filter(|part| (part.at[2] - part.size[2] * 0.5..=part.at[2] + part.size[2] * 0.5).contains(&z))
            .fold((f32::MIN, f32::MAX), |(top, bottom), part| {
                (top.max(part.at[1] + part.size[1] * 0.5), bottom.min(part.at[1] - part.size[1] * 0.5))
            })
    }

    /// **A bear is humped at the shoulder, carries its head low, and stands
    /// on long thick forelegs under a deep chest.**
    ///
    /// Written for "it looks like a capybara". The bear it replaced had a
    /// hump half a sixteenth over its back, a head as high as the back,
    /// five sixteenths of leg showing under a body fifteen deep, and a
    /// muzzle flush with its face; every line below is one of those, turned
    /// into the proportion a bear actually has.
    #[test]
    fn a_bear_is_humped_at_the_shoulder_with_its_head_low_on_long_forelegs() {
        let named = |name: &str| {
            parts(Species::Bear).iter().find(|part| part.name == name).unwrap_or_else(|| panic!("the bear has no {name}"))
        };
        let top = |part: &Part| part.at[1] + part.size[1] * 0.5;
        let ground = parts(Species::Bear).iter().map(|part| part.at[1] - part.size[1] * 0.5).fold(f32::MAX, f32::min);
        let highest = parts(Species::Bear).iter().map(top).fold(f32::MIN, f32::max);
        let height = highest - ground;
        let (foreleg, hind_leg, head, body) =
            (named("foreleg left"), named("hind leg left"), named("head"), named("body"));
        let (shoulder, chest) = trunk_over(parts(Species::Bear), foreleg.at[2]);
        let (rump, belly) = trunk_over(parts(Species::Bear), hind_leg.at[2]);

        // The hump is the top of the animal, over the forelegs...
        assert!(
            shoulder >= highest - 1e-3,
            "the highest point is {highest:.1} and the back over the forelegs {shoulder:.1}"
        );
        // ...and well over the rump, so the back slopes to the tail.
        assert!(shoulder - rump >= 2.0, "the shoulder stands {:.1} over the rump", shoulder - rump);
        // The head is carried under the line of the back, even at the rump.
        assert!(top(head) <= rump, "the top of the head, {:.1}, is over the rump, {rump:.1}", top(head));
        // A deep chest, hanging below the belly.
        assert!(chest < belly, "the chest, {chest:.1}, hangs no lower than the belly, {belly:.1}");
        assert!(
            shoulder - chest >= 0.55 * height,
            "the chest is {:.1} deep on a bear {height:.1} tall",
            shoulder - chest
        );
        // Long forelegs: a third of the animal shows under the chest...
        let showing = chest - ground;
        assert!(showing >= height / 3.0, "{showing:.1} of foreleg shows under a bear {height:.1} tall");
        // ...and thick ones, both ways.
        assert!(
            foreleg.size[0].min(foreleg.size[2]) >= body.size[0] * 0.3,
            "forelegs {:.1} thick under a body {:.1} wide are sticks",
            foreleg.size[0].min(foreleg.size[2]),
            body.size[0]
        );
        // A short broad muzzle: shorter for its head than the wolf's, and
        // broader.
        let muzzle = |model: &[Part]| *model.iter().find(|part| part.skin == Skin::Snout).expect("a muzzle");
        let wolf_head = head_of(Species::Wolf);
        assert!(
            muzzle(parts(Species::Bear)).size[2] / head.size[2] < muzzle(parts(Species::Wolf)).size[2] / wolf_head.size[2],
            "the bear's muzzle is as long for its head as a wolf's"
        );
        assert!(muzzle(parts(Species::Bear)).size[0] > muzzle(parts(Species::Wolf)).size[0], "the bear's muzzle is as narrow as a wolf's");
        // Small round ears.
        for ear in parts(Species::Bear).iter().filter(|part| part.name.starts_with("ear ")) {
            assert!((ear.size[0] - ear.size[1]).abs() <= 0.5, "{} is pointed rather than round", ear.name);
            assert!(
                ear.size[1] <= head.size[1] * 0.4,
                "{} is {:.1} tall on a head {:.1} tall",
                ear.name,
                ear.size[1],
                head.size[1]
            );
        }
        // A stub of a tail.
        let tail = named("tail");
        let (_, _, length) = extent(Species::Bear);
        assert!(
            tail.size[2] * SCALE <= length * 0.1,
            "a tail {:.2} long on a bear {length:.2} long",
            tail.size[2] * SCALE
        );
    }

    /// How much room a model fills standing still, in cubic blocks: the
    /// union of its parts counted on a grid of half-sixteenths, so a leg
    /// sunk into the body is not counted twice.
    fn filled(species: Species) -> f32 {
        const STEP: f32 = 0.5;
        let model = parts(species);
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for part in model {
            for axis in 0..3 {
                low[axis] = low[axis].min(part.at[axis] - part.size[axis] * 0.5);
                high[axis] = high[axis].max(part.at[axis] + part.size[axis] * 0.5);
            }
        }
        let steps = |axis: usize| ((high[axis] - low[axis]) / STEP).ceil() as usize;
        let mut cells = 0usize;
        for i in 0..steps(0) {
            for j in 0..steps(1) {
                for k in 0..steps(2) {
                    let point = [i, j, k].map(|n| n as f32);
                    let point = [0, 1, 2].map(|axis| low[axis] + (point[axis] + 0.5) * STEP);
                    let inside = |part: &Part| (0..3).all(|axis| (point[axis] - part.at[axis]).abs() <= part.size[axis] * 0.5);
                    if model.iter().any(inside) {
                        cells += 1;
                    }
                }
            }
        }
        cells as f32 * (STEP * SCALE).powi(3)
    }

    /// **A bear is bigger than a boar and a wolf** -- taller, longer, and
    /// with a good deal more animal in it.
    ///
    /// **A guard on the capybara's fix, not a test of its fault.** The bear
    /// it replaced was big enough -- 1.77 boars of filled room -- and read
    /// wrong because it was a block. Shaping it took room out of the barrel
    /// (1.42 cubic blocks to 1.25), and a bear pared down to the size of
    /// what it hunts is the next way to get it wrong. Measured as filled
    /// room rather than as a bounding box, because a box is what a tall
    /// thin animal and a big one have in common.
    #[test]
    fn a_bear_is_bigger_than_a_boar_and_a_wolf() {
        let (_, bear_height, bear_length) = extent(Species::Bear);
        let bear_room = filled(Species::Bear);
        for (other, more_room) in [(Species::Boar, 1.4), (Species::Wolf, 2.5)] {
            let (_, height, length) = extent(other);
            let room = filled(other);
            assert!(
                bear_height >= height * 1.2,
                "the bear is {bear_height:.2} tall and a {} {height:.2}",
                other.name()
            );
            assert!(
                bear_length > length,
                "the bear is {bear_length:.2} long and a {} {length:.2}",
                other.name()
            );
            assert!(
                bear_room >= room * more_room,
                "the bear fills {bear_room:.3} cubic blocks and a {} {room:.3}",
                other.name()
            );
        }
        // ...and the animal the server simulates is the bigger one too.
        for other in [Species::Boar, Species::Wolf] {
            assert!(Species::Bear.height() > other.height() && Species::Bear.length() > other.length());
        }
    }

    /// **Every paw stays on the foot of its leg through the whole stride.**
    ///
    /// A paw is a box of its own under a leg that swings. Left to swing
    /// about its own top it turns at the ankle while its leg turns at the
    /// shoulder, and mid-stride the two part company -- the paw planted a
    /// few inches from the foot it belongs to. Checked on the drawn
    /// vertices, not on `Part::pivot`: the distance from the middle of the
    /// leg's sole to the middle of the paw's top is the same at every point
    /// of the stride as standing still, which is only true of two boxes
    /// that move as one.
    #[test]
    fn every_paw_stays_on_the_foot_of_its_leg_through_the_stride() {
        use crate::engine::mesh::Vertex;
        // A box is 24 vertices in table order, and its faces go +Y, -Y
        // first (see `append_part_posed`).
        let face_middle = |boxed: &[Vertex], face: usize| {
            boxed[face * 4..face * 4 + 4].iter().map(|v| Vec3::from_array(v.position)).sum::<Vec3>() / 4.0
        };
        let mut paws = 0;
        for &species in Species::ALL {
            let model = parts(species);
            for (index, paw) in model.iter().enumerate().filter(|(_, part)| part.name.contains("paw")) {
                paws += 1;
                // Its leg: the same side, the same gait, and nearest along
                // the animal.
                let (leg_index, _) = model
                    .iter()
                    .enumerate()
                    .filter(|(_, part)| {
                        part.name.contains("leg") && part.gait == paw.gait && part.at[0].signum() == paw.at[0].signum()
                    })
                    .min_by(|a, b| (a.1.at[2] - paw.at[2]).abs().total_cmp(&(b.1.at[2] - paw.at[2]).abs()))
                    .unwrap_or_else(|| panic!("{}: {} has no leg", species.name(), paw.name));
                let apart = |v: &[Vertex]| {
                    let sole = face_middle(&v[leg_index * 24..leg_index * 24 + 24], 1);
                    let paw_top = face_middle(&v[index * 24..index * 24 + 24], 0);
                    (sole - paw_top).length()
                };
                let standing = apart(&mesh(species, 0.0, 0.0, 0.0));
                assert!(standing < 0.1, "{}: {} stands {standing:.2} from its leg", species.name(), paw.name);
                for step in 0..32 {
                    let walked = step as f32 * 0.1;
                    let striding = apart(&mesh(species, 0.0, 4.0, walked));
                    assert!(
                        (striding - standing).abs() < 1e-3,
                        "{}: {} is {striding:.3} from its leg {walked:.1} blocks into a walk and {standing:.3} standing",
                        species.name(),
                        paw.name
                    );
                }
            }
        }
        assert!(paws >= 4, "the bear has {paws} paws");
    }

    #[test]
    fn every_ear_is_an_ear_rather_than_a_lump_of_fur() {
        for &species in Species::ALL {
            let ears: Vec<&Part> = parts(species)
                .iter()
                .filter(|p| p.name.starts_with("ear "))
                .collect();
            // Two, except on the bird, which has none: a grouse's ear
            // is a hole under the feathers, and drawing one would be
            // drawing something nobody has ever seen on a bird.
            // ...nor on a fish, which hears through its skin.
            // ...nor on the crab, which has no head to put them on
            // (`is_shelled`).
            let wanted = if species.flies() || species.swims() || is_shelled(species) { 0 } else { 2 };
            assert_eq!(ears.len(), wanted, "{} has {} ears", species.name(), ears.len());
            for e in ears {
                assert_eq!(
                    e.skin,
                    Skin::Ear,
                    "{}: {} is coarse hide rather than an ear",
                    species.name(),
                    e.name
                );
                // Ears move with the head, or the animal turns and leaves
                // them behind.
                assert_eq!(e.gait, Gait::Head, "{}: {} does not follow the head", species.name(), e.name);
            }
        }
    }

    #[test]
    fn small_parts_wear_a_piece_of_the_hide_and_faces_wear_the_whole_picture() {
        // The texel-density fix: a tusk three sixteenths tall shows
        // three texels of ivory, not the whole picture squeezed on --
        // and the eyed side of a head keeps the whole picture, because
        // the eye is drawn *in* it and a crop would cut it off.
        use crate::engine::mesh::FINE_UV_BIT;
        let layers = FaceLayers::empty_for_test();
        let v = mesh(Species::Boar, 0.0, 0.0, 0.0);

        let cropped = v.iter().filter(|x| x.uv & FINE_UV_BIT != 0).count();
        assert!(cropped > 0, "no part of the boar is cropped to its own size");

        for feature in [Skin::Head, Skin::HeadMirror, Skin::Face, Skin::Nose, Skin::Ear] {
            let layer = layers.animal(Species::Boar, feature.slot());
            for vertex in v.iter().filter(|x| x.tex_layer() == layer) {
                assert_eq!(
                    vertex.uv & FINE_UV_BIT,
                    0,
                    "{feature:?} is cropped: its drawn feature is cut off"
                );
            }
        }
    }

    /// Every quad of a model mesh is wound toward the outside of the box it
    /// belongs to, and its light word names the axis it is wound toward.
    ///
    /// **The bug this exists for.** The light word carries a face index
    /// and the shader turns it into a normal, so it has to describe the
    /// face *after* the animal has been turned. It carried the model-space
    /// index instead: a boar walking east was lit as though it faced
    /// north, and -- the part that gave it away -- the shading never
    /// changed as the animal turned, because the indices could not.
    ///
    /// Checked against the geometry rather than against the arithmetic
    /// that produced it: the direction a face is wound toward is the one
    /// fact here that cannot itself be wrong. Every box is six quads in
    /// a row (`append_part_posed`), which is how its middle is found.
    fn assert_faces_point_where_they_say(v: &[crate::engine::mesh::Vertex], what: &str) {
        const AXES: [Vec3; 6] = [
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Z,
            Vec3::NEG_Z,
        ];
        assert_eq!(v.len() % 24, 0, "{what}: not whole boxes");
        for boxed in v.chunks_exact(24) {
            let middle = boxed.iter().map(|x| Vec3::from_array(x.position)).sum::<Vec3>() / 24.0;
            for quad in boxed.chunks_exact(4) {
                let p = |k: usize| Vec3::from_array(quad[k].position);
                let wound = (p(1) - p(0)).cross(p(2) - p(1)).normalize();
                let declared = AXES[(((quad[0].light() >> 10) & 7) as usize).min(5)];
                // The light word holds one of six directions and a yaw is
                // continuous, so the most that can be asked is that it
                // names the axis the face leans toward.
                let nearest = AXES.into_iter().map(|axis| wound.dot(axis)).fold(f32::MIN, f32::max);
                assert!(
                    wound.dot(declared) >= nearest - 1e-3,
                    "{what}: a face wound toward {wound:?} claims to point {declared:?}"
                );
                let centre = (p(0) + p(1) + p(2) + p(3)) / 4.0;
                assert!(wound.dot(centre - middle) > 0.0, "{what}: a face is wound toward the inside of its box");
            }
        }
    }

    #[test]
    fn every_face_of_an_animal_carries_the_direction_it_actually_points() {
        for &species in Species::ALL {
            // Eighths of a turn, and mid-stride, so the legs are swung
            // as well as the animal turned.
            for turn in 0..8 {
                let yaw = turn as f32 * std::f32::consts::FRAC_PI_4;
                let v = mesh(species, yaw, 4.0, 1.3);
                assert_faces_point_where_they_say(&v, &format!("{} at yaw {yaw:.2}", species.name()));
            }
        }
    }

    /// The same, for the skeletons: they are laid in a pose of their own
    /// (built lying, their legs turned off the axes, placed by their own
    /// bones), and a pose -- and a turn most of all -- is exactly where a
    /// face index and its geometry part company.
    #[test]
    fn every_face_of_a_skeleton_is_wound_outward_and_lit_the_way_it_points() {
        let layers = FaceLayers::empty_for_test();
        for species in with_bones() {
            for turn in 0..8 {
                let yaw = turn as f32 * std::f32::consts::FRAC_PI_4;
                let (mut v, mut i) = (Vec::new(), Vec::new());
                build_bones(species, Vec3::new(3.5, 7.0, -2.5), yaw, &layers, (15, 0), &mut v, &mut i);
                assert_faces_point_where_they_say(&v, &format!("{}'s skeleton at yaw {yaw:.2}", species.name()));
            }
        }
    }

    #[test]
    fn only_the_deer_carries_bone_on_its_head() {
        for &species in Species::ALL {
            let antlers = parts(species).iter().filter(|p| p.skin == Skin::Antler).count();
            let wanted = if species == Species::Deer { 4 } else { 0 };
            assert_eq!(antlers, wanted, "{} has {antlers} antler parts", species.name());
        }
    }

    /// ...and only the antelope wears horns, in a skin and a slot of their
    /// own: a horn drawn in the antler tile is the deer's pale bone on the
    /// wrong animal, and the skeleton would keep the whole of it.
    #[test]
    fn only_the_antelope_wears_horns_and_a_horn_is_not_an_antler() {
        for &species in Species::ALL {
            let horns = parts(species).iter().filter(|p| p.skin == Skin::Horn).count();
            let wanted = if species == Species::Antelope { 4 } else { 0 };
            assert_eq!(horns, wanted, "{} has {horns} horn parts", species.name());
        }
        assert_ne!(Skin::Horn.slot(), Skin::Antler.slot());
        assert!(Skin::Horn.slot() < crate::engine::texture::SHEET_SLOTS, "the horn is off the sheet");
    }

    /// **A lion's mane is round its head and behind its face.** The mane is
    /// the lion at a distance, and it is also the one part here big enough
    /// to swallow the part next to it: a mane box over the whole head puts
    /// the eye drawn on the head's side inside the hair, and a lion with no
    /// face is a brown barrel. So the head stands out of the mane by at least
    /// half its length, and the mane is wider and taller than the head, or it
    /// is a collar.
    #[test]
    fn a_lions_mane_is_round_its_head_and_behind_its_face() {
        let named = |name: &str| parts(Species::Lion).iter().find(|part| part.name == name).expect("a lion part");
        let (head, mane) = (named("head"), named("mane"));
        let front = |part: &Part| part.at[2] - part.size[2] * 0.5;
        assert!(
            front(mane) - front(head) >= head.size[2] * 0.5,
            "the mane covers {:.1} of a head {:.1} long",
            head.size[2] - (front(mane) - front(head)),
            head.size[2]
        );
        assert!(mane.size[0] > head.size[0] && mane.size[1] > head.size[1], "the mane is a collar");
        // ...and the ears are in front of it, where they can be seen.
        for ear in parts(Species::Lion).iter().filter(|part| part.name.starts_with("ear ")) {
            assert!(ear.at[2] + ear.size[2] * 0.5 < front(mane), "a lion's ear is inside its mane");
        }
    }

    #[test]
    fn a_gull_spreads_its_wings_in_the_air_and_folds_them_on_the_sand() {
        // Across the bird is world z at yaw zero (`an_animal_faces_the_way_it_is_going`).
        let across = |v: &[crate::engine::mesh::Vertex]| {
            let (low, high) = v
                .iter()
                .fold((f32::MAX, f32::MIN), |(low, high), x| (low.min(x.position[2]), high.max(x.position[2])));
            high - low
        };
        let standing = across(&mesh(Species::Gull, 0.0, 0.0, 0.0));
        let walking = across(&mesh(Species::Gull, 0.0, Species::Gull.walk_speed() * 1.07, 1.0));
        let flying = across(&mesh(Species::Gull, 0.0, 5.0, BEAT_BOUT + 2.0));
        assert!(standing <= Species::Gull.width() + 0.1, "a gull on the sand is {standing:.2} across");
        assert!((walking - standing).abs() < 1e-3, "a walking gull opened its wings");
        assert!(flying > standing * 3.0, "a gull in the air is only {flying:.2} across against {standing:.2} standing");
    }

    #[test]
    fn a_flushed_grouse_flies_on_wings_and_folds_them_when_it_lands() {
        // `Species::flies` sent the fowl up into the trees long before its
        // model had anything to fly with, and a player watched a brown box
        // with legs flush off a bush.
        let across = |v: &[crate::engine::mesh::Vertex]| {
            let (low, high) = v
                .iter()
                .fold((f32::MAX, f32::MIN), |(low, high), x| (low.min(x.position[2]), high.max(x.position[2])));
            high - low
        };
        let standing = across(&mesh(Species::Fowl, 0.0, 0.0, 0.0));
        let walking = across(&mesh(Species::Fowl, 0.0, Species::Fowl.walk_speed() * 1.07, 1.0));
        let flushed = across(&mesh(Species::Fowl, 0.0, Species::Fowl.run_speed(), BEAT_BOUT + 2.0));
        assert!(standing <= Species::Fowl.width() + 0.1, "a grouse on the ground is {standing:.2} across");
        assert!((walking - standing).abs() < 1e-3, "a walking grouse opened its wings");
        assert!(flushed > standing * 2.5, "a flushed grouse is only {flushed:.2} across against {standing:.2} standing");
    }

    #[test]
    fn a_gliding_gull_holds_its_wings_still_and_a_frightened_one_beats_them() {
        let top = |speed: f32, walked: f32| {
            mesh(Species::Gull, 0.0, speed, walked).iter().map(|v| v.position[1]).fold(f32::MIN, f32::max)
        };
        let gliding: Vec<f32> = (0..10).map(|i| top(5.0, BEAT_BOUT + 0.5 + i as f32)).collect();
        let (low, high) = gliding.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
        assert!(high - low < 1e-3, "a gliding gull's wingtips moved {:.3} blocks", high - low);
        let beating: Vec<f32> = (0..20).map(|i| top(7.2, i as f32 * 0.1)).collect();
        let (low, high) = beating.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
        assert!(high - low > 0.3, "a frightened gull's wingtips moved only {:.3} blocks", high - low);
    }

    #[test]
    fn a_beating_wing_stays_joined_at_the_shoulder_and_at_the_wrist() {
        // The hinge does not wander off the body, and the black hand stays
        // on the end of the grey arm, whatever the angle: a wing that parted
        // company with itself at the top of a beat would be two birds' worth
        // of plates.
        let part = |name: &str| parts(Species::Gull).iter().find(|p| p.name == name).expect("a gull part");
        for (arm, hand, hinge) in [("wing right", "wingtip right", 2.0f32), ("wing left", "wingtip left", -2.0)] {
            let (arm, hand) = (part(arm), part(hand));
            let inner = if hinge > 0.0 { 0.0 } else { 1.0 };
            for step in 0..16 {
                let angle = -FLAP + step as f32 * (2.0 * FLAP / 15.0);
                for (y, z) in [(0.0, 0.0), (1.0, 1.0)] {
                    // The plate's middle, not its top or bottom corner: a plate
                    // with any thickness swings those a hair either side of
                    // the hinge as it turns, which is a plate and not a wing
                    // coming loose.
                    let shoulder = posed_local(arm, [inner, 0.5, z], angle, 0.0);
                    assert!(
                        (shoulder.x - hinge * SCALE).abs() < 0.01,
                        "the {} left its shoulder: {:.3} against {:.3}",
                        arm.name,
                        shoulder.x,
                        hinge * SCALE
                    );
                    // The arm's outer edge and the hand's inner edge, at the
                    // same corner of the plate, stay within a seam of each
                    // other.
                    let wrist = posed_local(arm, [1.0 - inner, y, 0.5], angle, 0.0);
                    let hand_inner = posed_local(hand, [inner, y, 0.5], angle, 0.0);
                    let apart = Vec3::new(wrist.x - hand_inner.x, wrist.y - hand_inner.y, 0.0).length();
                    assert!(apart < 0.01, "the {} came off its wing by {apart:.3} at {angle:.2}", hand.name);
                }
            }
        }
    }

    // ---- what a field full of animals costs to draw ----
    //
    // Run with:
    // cargo test -p primitive_client --release --lib -- --ignored --nocapture a_field_of_a_hundred_and_twenty
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn a_field_of_a_hundred_and_twenty_animals_costs_this_much_to_build() {
        // **Measured, not guessed.** `build` runs once a frame for every
        // animal on screen -- it is the CPU half of drawing them, the
        // other half being whatever the vertices cost to upload and
        // shade -- and `primitive_shared::animals::MAX_ANIMALS` is a
        // hundred and twenty, so that is the worst a frame can ask of
        // it. Comparing this number against a future change is the
        // whole point of writing it down: "should be faster" is not a
        // measurement, and this is the fixture that makes it one.
        let layers = FaceLayers::empty_for_test();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        const ANIMALS: usize = 120;
        const ROUNDS: usize = 20;
        let mut total_vertices = 0usize;
        let mut best = f64::MAX;
        for _ in 0..ROUNDS {
            vertices.clear();
            indices.clear();
            let started = std::time::Instant::now();
            for i in 0..ANIMALS {
                let species = Species::ALL[i % Species::ALL.len()];
                build(
                    species,
                    Vec3::new((i % 20) as f32, 0.0, (i / 20) as f32),
                    i as f32 * 0.3,
                    Motion { walked: i as f32 * 2.0, speed: 4.0, ..Default::default() },
                    &layers,
                    (15, 0),
                    &mut vertices,
                    &mut indices,
                );
            }
            best = best.min(started.elapsed().as_secs_f64() * 1000.0 / ANIMALS as f64);
            total_vertices = vertices.len();
        }
        println!(
            "build: {best:.4} ms/animal over {ANIMALS} animals ({} vertices/animal, \
             {:.3} ms for the whole field)",
            total_vertices / ANIMALS,
            best * ANIMALS as f64,
        );
    }
}
