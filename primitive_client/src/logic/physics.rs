//! Этап 4: "Простая физика: гравитация, прыжок, столкновение с блоками."
//! Plan's suggested starting point: "проверяйте, не находится ли новая
//! позиция игрока внутри блока. Если находится -- откатывайте движение."
//! This does exactly that, one axis at a time (so sliding along a wall
//! still lets the other axis keep moving, instead of a single combined
//! check freezing all motion the instant any axis touches a block).
//!
//! Changes this pass:
//! - `move_speed` comes from settings instead of being a hard-coded
//!   constant the settings file only *pretended* to control;
//! - `teleport()` exists so the server's anti-cheat can rubber-band the
//!   player back without the client fighting it;
//! - `grounded` is reported to the server, which cross-checks it against
//!   the real world.

use glam::{DVec3, Vec3};

use crate::logic::chunk_manager::ChunkManager;

// Re-exported from the shared crate so the client and the server can't
// drift apart on what counts as "inside a player".
pub use primitive_shared::geometry::{
    EYE_HEIGHT, PLAYER_HALF_WIDTH, PLAYER_HEIGHT, PLAYER_STEP_HEIGHT,
};

/// How much clearance a resolved collision leaves.
///
/// A sweep that puts the collider exactly against a face leaves the two
/// touching, and the next frame's overlap test then has to decide what
/// "exactly touching" means with numbers that have already been rounded
/// twice. A tenth of a millimetre is invisible and removes the question.
const CONTACT_SKIN: f32 = 1e-4;

/// How long the view takes to catch up with a step.
///
/// The collider is lifted onto a layer in one frame -- it has to be, or
/// it would be inside the layer for a frame and the world would have to
/// decide what that means. The *camera* rising a fifth of a metre in one
/// frame is a jolt, so it keeps the old height and closes the gap over
/// this long. Long enough to read as walking up onto something, short
/// enough that the view is never meaningfully behind where the player
/// is.
const STEP_SMOOTHING_SECONDS: f32 = 0.12;

/// How far the camera may trail the collider after steps: two of them.
///
/// **A step's tread is half a cell and the player is 0.6 wide**, so a body
/// walking onto a stair or a roof step meets the riser behind the tread
/// before its middle is over the tread at all: the second lift comes a
/// tenth of a second after the first. With the trail capped at one step,
/// the first lift glided and the second was thrown into the view whole --
/// "it puts me straight onto the second step". Two steps' trail lets a
/// flight be walked as the smooth climb it is; the collider is where it
/// always was.
const STEP_LAG_MAX: f32 = 2.0 * PLAYER_STEP_HEIGHT;

/// The furthest a player is moved to get them out of a block they are
/// already inside.
///
/// A whole block and a bit: enough to step out of anything a single
/// block can bury you in, short enough that it can never read as a
/// teleport. Anything deeper than this is a hole somebody dug around
/// you, and being left in it is the honest answer -- see
/// `escape_solids`.
const MAX_ESCAPE: f32 = 1.2;

const GRAVITY: f32 = -22.0;
const JUMP_VELOCITY: f32 = 8.0;
const TERMINAL_VELOCITY: f32 = -50.0;

// --- water ---
//
// The old model was "weak gravity, a lot of drag, and a sink speed you
// could not escape without holding jump". Three things were wrong with
// it:
//
// * **You could never rest.** Doing nothing sank you until you drowned,
//   so staying alive in a lake meant holding jump for as long as you
//   were in it. Water was a thing to get out of, not to be in.
// * **There was no inertia.** Horizontal velocity was *assigned* from
//   the input, so a swimmer reached full speed and stopped dead inside
//   one frame -- in water, of all places, and exactly the thing ground
//   movement was rewritten to stop doing.
// * **A puddle was a lake.** Any water touching any part of the
//   collider put the player in swimming mode, so an eighth-deep film
//   left behind by a receding puddle turned walking into treading
//   water: a third of your speed, no friction, and the jump key
//   swimming you upward out of it. That was invisible while every cell
//   of water was full. It is not now.
//
// What replaces all three is one measurement -- how far up the player
// the water comes -- and buoyancy, which is a force rather than a mode.

/// **A loaded swimmer goes down, and an empty one floats.** The other
/// half of `load::buoyancy` -- the server halves the breath and this is
/// what puts the head under it. Both halves are the same shared number
/// so the picture and the drowning agree.
#[cfg(test)]
mod load_tests {
    use super::*;

    /// Where a swimmer ends up after ten seconds of doing nothing, in
    /// the ten-deep lake the other water tests use.
    fn resting_depth(buoyancy: f32) -> f32 {
        let chunks = super::tests::lake_world();
        let mut player = Player::new((Vec3::new(0.5, 18.0, 0.5)).as_dvec3(), 5.5);
        player.buoyancy = buoyancy;
        for _ in 0..200 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 0.05);
        }
        player.position.y as f32
    }

    #[test]
    fn an_empty_pack_floats_and_a_stone_one_goes_to_the_bottom() {
        let floating = resting_depth(1.0);
        let sinking = resting_depth(0.0);
        assert!(
            sinking < floating - 2.0,
            "a swimmer with no buoyancy rested at {sinking:.1} and one with all of it at \
             {floating:.1}"
        );
        // ...and a part load rides *lower* rather than either bobbing
        // like an empty pack or going to the bottom like a full one:
        // the ramp is a ramp while the body can still float at all.
        let laden = resting_depth(0.8);
        assert!(laden < floating - 0.05, "a part load floated as high as nothing");
        assert!(laden > sinking + 1.0, "a part load sank like a full one");
    }

    #[test]
    fn a_swimmer_who_opens_their_pack_stops_sinking_and_still_does_not_rise() {
        // **"Если игрок задыхается, то рюкзак нельзя нормально достать."**
        // The one way out of being too heavy to swim was to open the pack
        // and drop something, and opening the pack took the keys away --
        // so the seconds spent choosing were seconds spent sinking faster
        // into the thing being escaped. See `Player::treading`.
        let chunks = super::tests::lake_world();
        let sink_while = |treading: bool| {
            let mut player = Player::new((Vec3::new(0.5, 18.0, 0.5)).as_dvec3(), 5.5);
            player.buoyancy = 0.0;
            // Two seconds of going down, which is where a player notices
            // they are in trouble and reaches for the pack.
            for _ in 0..40 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 0.05);
            }
            let reached_for_it = player.position.y;
            // ...and three seconds of rummaging.
            player.treading = treading;
            for _ in 0..60 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 0.05);
            }
            (reached_for_it, player.position.y)
        };
        let (was, sank) = sink_while(false);
        let (also_was, held) = sink_while(true);
        assert!((was - also_was).abs() < 1e-4, "the two runs did not start together");
        assert!(was - sank > 1.0, "a full load did not sink at all, so this proves nothing");
        assert!(
            was - held < 0.35,
            "three seconds in the pack cost {} blocks of depth, against {} without treading",
            was - held,
            was - sank
        );
        // **And it is a hold, not a lift.** A player who could rise by
        // opening their pack would have a free way out of the decision that
        // sinking under a load exists to pose.
        assert!(held <= was + 1e-3, "opening the pack carried a loaded swimmer upward to {held} from {was}");
    }
}

/// How deep the water has to be before walking becomes swimming.
///
/// Waist deep. Below it the feet are still on the floor and the player
/// is *wading*: ordinary walking, slowed by what they are pushing
/// through. Above it there is more water than legs, and nothing to push
/// against.
const SWIM_DEPTH: f32 = PLAYER_HEIGHT * 0.5;

/// **A single cell of water has to stay a ford, and the margin is two
/// centimetres.**
///
/// A player standing on the floor of a cell of water is
/// `1.0 - fluid::SURFACE_DROP` = 0.88 deep in it -- whatever the flow
/// simulation thinks that cell holds, because every cell of water is
/// drawn and collided at one height on purpose (see
/// `fluid::surface_height`). `SWIM_DEPTH` is 0.90. Six hundredths of a
/// block is the whole of what keeps a brook from being something you
/// have to swim across.
///
/// Nothing else says so, and the three constants that decide it live in
/// three files. Raise `SURFACE_DROP` to 0.10, shorten `PLAYER_HEIGHT`,
/// or take this fraction below a half, and every ford in the world
/// quietly becomes a lake: the player stops being grounded in
/// ankle-deep water, the jump key turns into a stroke, and crossing a
/// stream carries them off their feet. Nobody would connect that to the
/// constant they touched, so it is caught here -- at compile time,
/// where it cannot be filtered out of a test run.
const _: () = assert!(
    1.0 - primitive_shared::fluid::SURFACE_DROP < SWIM_DEPTH,
    "a single cell of water is now deeper than SWIM_DEPTH: every ford has become a lake",
);

/// How slowly you wade when the water is as deep as it can be and still
/// be walked through.
const WADE_SPEED: f32 = 0.45;

/// How much of a jump's take-off the water takes from a body standing in it
/// waist deep. See [`Player::wade_jump`] for why a tenth.
const WATER_JUMP_LOSS: f32 = 0.1;

/// How much of the player ends up under the surface once they have
/// settled with nothing pressed.
///
/// The eyes are at 1.62 and the crown at 1.8, so this floats a player
/// with their head clear of the water by a hand's breadth. It has to
/// clear `EYE_HEIGHT` by more than the surface moves about, or a
/// floating player's view dips under and the fog flickers on and off.
const FLOAT_SUBMERSION: f32 = 1.5;

/// How hard a fully submerged player is pushed upward, net of gravity.
///
/// About a metre a second once drag has settled it: enough that falling
/// in and doing nothing brings you back to air, never so much that the
/// water reads as a lift.
const RISE_ACCEL: f32 = 4.0;

/// Buoyancy per unit of submerged fraction, and the gravity it works
/// against.
///
/// Derived from the two numbers above rather than tuned beside them.
/// Tuned beside them they drift apart, and the pair has to cancel at
/// `FLOAT_SUBMERSION` *exactly*: a few percent out and a floating
/// player creeps up out of the water or sinks under it, slowly enough
/// that it looks like a bug in something else entirely.
const BUOYANCY: f32 = RISE_ACCEL * PLAYER_HEIGHT / (PLAYER_HEIGHT - FLOAT_SUBMERSION);

/// How long a body takes to kick most of a sink out of itself once the
/// client has taken its keys away. See [`Player::treading`].
///
/// A third of a second: fast enough that opening a pack a metre down does
/// not cost another metre, slow enough that it reads as a body arresting
/// itself rather than as the game freezing one axis. Instant would be the
/// second of those, and would also mean a dive interrupted by the
/// inventory key stops dead in open water, which no body does.
const TREAD_SECONDS: f32 = 0.35;
// The downward half of the pair used to be its own constant,
// `WATER_GRAVITY = -BUOYANCY * FLOAT_SUBMERSION / PLAYER_HEIGHT`. It is
// gone because the resting depth is no longer fixed: a load moves it
// (see the buoyancy term in `update`), so the pair cancels at a depth
// that is worked out per tick rather than baked into a second constant
// that could only ever describe the empty-handed case.

/// How fast a fully submerged body rises once it has stopped
/// accelerating, in blocks per second.
///
/// A metre and a bit. The figure the comment on [`RISE_ACCEL`] has
/// always claimed -- and it is *enforced* now rather than hoped for,
/// because the drag coefficient below is derived from it.
const RISE_SPEED: f32 = 1.25;

/// Water's resistance, as **v-squared per block**.
///
/// **The single change that made water stop feeling like a menu.** It
/// used to be `velocity *= 0.02^dt` -- ninety-eight per cent of your
/// speed gone every second, in proportion to the speed you had. Two
/// things follow from that and both of them are wrong:
///
/// * **Drag proportional to velocity has no scale.** Moving slowly is
///   resisted exactly as hard, in proportion, as moving fast, so water
///   never feels *thick* -- it feels like a low ceiling on speed.
///   Real fluid drag at any speed a body swims at goes as the square,
///   which is why a slow stroke is nearly free and a fast one is not.
/// * **At ninety-eight per cent a second you stop dead.** Let go and
///   you are motionless in a fifth of a second. Nothing with mass does
///   that in water; the glide is most of what swimming *is*.
///
/// The vertical figure is not chosen, it is derived: it is the drag
/// that makes a submerged body settle at [`RISE_SPEED`] against
/// [`RISE_ACCEL`]. Sideways is far lighter, because a body pushed
/// along its own length is a great deal more slippery than one pushed
/// broadside -- and because that is where the glide lives.
/// ...and the **linear** half of the same law, which dominates when
/// barely moving.
///
/// Real drag is the sum of two terms, and leaving either one out breaks
/// something specific. Without the linear term a body in water never
/// actually stops -- a v-squared law decays as `1/t`, so a floating
/// player bobs on the buoyancy spring for ever with nothing to damp it,
/// which is precisely what the settling test caught the moment the
/// quadratic law went in on its own.
///
/// The vertical figure is the one that matters: buoyancy is a spring
/// (see [`BUOYANCY`]) and this is its damper. At two it puts the
/// damping ratio near a third, so a body dropped in settles in about
/// three seconds instead of nodding indefinitely.
const WATER_LINEAR_VERTICAL: f32 = 2.0;
const WATER_LINEAR_SIDEWAYS: f32 = 1.0;

/// The quadratic half. Derived so that the two together settle a
/// submerged body at [`RISE_SPEED`] against [`RISE_ACCEL`], which is
/// what makes that constant a statement rather than a wish.
const WATER_DRAG_VERTICAL: f32 =
    (RISE_ACCEL - WATER_LINEAR_VERTICAL * RISE_SPEED) / (RISE_SPEED * RISE_SPEED);
const WATER_DRAG_SIDEWAYS: f32 = 0.35;

/// How hard a swimmer pulls, per unit of the speed they are pulling
/// toward. A fifth of a second to full speed, against the ground's
/// twelfth: the difference between pushing off something and pulling
/// against nothing.
///
/// **Up from three, and diving is why.** A stroke has to beat buoyancy
/// *and* drag to go downward, and with a real drag law those two
/// together come to about twelve blocks a second squared -- so at three
/// a diver settled at eight tenths of a block a second and the bottom
/// of a lake stopped being somewhere you could get to. This is what a
/// swimmer can actually pull; the top speed is still
/// [`WATER_MOVE_FACTOR`]'s to set, because `swim` caps at it.
const SWIM_ACCEL: f32 = 5.0;

/// Top swimming speed, as a fraction of walking speed.
///
/// **Down from 0.55, and the drag is why.** At 0.55 this was a target
/// the old drag never let anybody reach -- water took ninety-eight per
/// cent of your speed every second, so the number in the constant and
/// the speed on the screen were different things. With a drag that
/// leaves room to move, the target became the answer, and swimming came
/// out *faster than wading*: a body with its feet on the bottom
/// overtaken by one with nothing to push against.
///
/// Two fifths is about what a fast swimmer does against a runner, and
/// it restores the order the three regimes have to be in -- path,
/// ford, lake.
const WATER_MOVE_FACTOR: f32 = 0.38;

/// Upward acceleration from holding jump under water, and the speed it
/// tops out at.
///
/// Acceleration rather than an assignment: setting the velocity made
/// the stroke a step change, so tapping jump under water jerked. The
/// cap is what stops it from becoming a launch.
const SWIM_STROKE_ACCEL: f32 = 12.0;
const SWIM_UP_SPEED: f32 = 3.0;

// **Water has no speed limit of its own any more, and that is the
// point.** There used to be one at three blocks a second, and it was
// doing two jobs: stopping a long fall carrying somebody through the
// bed of a shallow lake, and -- in practice -- being the only reason
// water slowed anybody down at all.
//
// Quadratic drag does the first properly. Speed decays as `e^(-k x)`,
// so how deep you go is set by how fast you arrived, which is what
// entering water is; a clamp made a step off a kerb and a fifty-block
// fall reach the same depth. And air already bounds the worst case at
// `TERMINAL_VELOCITY`, which the collider has always survived, so there
// is nothing left for a second clamp to guard against.

/// How fast a swimmer pressed against something rises along it.
///
/// **This is how you get out of a lake.** A swimmer is never grounded,
/// so the step-up that walks a player over a kerb never fires for them:
/// the bank of a river is a wall to somebody in it, and the only way
/// out was to hold jump until you cleared the top and then swim
/// forward, which nobody discovers by accident. Swimming into something
/// now climbs it -- and cannot become a way to climb *anything*,
/// because it stops the moment the player is no longer swimming, which
/// is to say at the surface.
const LEDGE_CLIMB_SPEED: f32 = 3.0;
/// How fast a player walks, in blocks a second.
///
/// **Five and a half was a run, and the whole game was played at it.**
/// A block is a metre here, so 5.5 m/s is a fast jog and the sprint on
/// top of it was 8.8 -- which is roughly the world record for the
/// hundred metres, held indefinitely, while carrying eighty kilograms
/// of rock. The consequence was not that anybody complained about the
/// number; it was that distance stopped meaning anything. A hill you
/// can be on top of in nine seconds is not a hill you decide to climb,
/// and a world crossed at a sprint is a world with no middle distance
/// in it -- which is where every mechanic in this game lives: the walk
/// to the ore, the walk back to the tannery, the choice to camp rather
/// than push on.
///
/// **4.3 is the number, and it is chosen rather than halved.** It is
/// still faster than a person really walks, because a game where the
/// meadow takes as long to cross as a real meadow is a game nobody
/// plays; it is slow enough that the ground has size again. Below about
/// four the controls start to read as sticky rather than heavy, which
/// is the failure at the other end and the reason this is not 3.
///
/// Everything downstream of it is a ratio and needed no change: the
/// anti-cheat ceiling (12 b/s) keeps its headroom, load slows you by a
/// fraction of this, and the animals are compared against it through
/// `animals::NOMINAL_SPRINT_SPEED` -- which is derived from this and
/// the sprint multiplier and has to be updated with them.
#[allow(dead_code)] // fallback used by tests and by callers without a settings file
pub const DEFAULT_MOVE_SPEED: f32 = 4.3;

/// How much faster sprinting is than walking.
///
/// Half again, so a sprint is 6.45 blocks a second -- a hard run rather
/// than the flat-out 8.8 it used to be. The gap matters more than
/// either number: sprinting has to be visibly worth the food it burns
/// (see `body::SPRINT_THIRST_PER_SECOND`) and must not be the speed a
/// player simply travels at, which is what a 1.6 multiplier on a fast
/// walk had made it.
///
/// Bounded by the server: the anti-cheat allows 12 blocks per second
/// horizontally, which leaves plenty of headroom for the lag spikes the
/// limit exists to tolerate. Raising this without raising
/// `max_horizontal_speed` would get sprinting players rubber-banded.
pub const SPRINT_MULTIPLIER: f32 = 1.5;

// --- ground movement ---
//
// Velocity used to be assigned straight from the input direction, which
// meant the player reached full speed and stopped dead within one frame,
// and -- the part that mattered -- could do that in mid-air. Steering
// with no ground under you was as effective as steering on it, so a jump
// was a free change of direction and a free change of speed.
//
// The model here is the standard one: friction bleeds speed off, and an
// acceleration step tops it back up toward what the player asked for,
// but only up to a cap on how fast they may go *in that direction*.
// Everything interesting falls out of choosing different caps for ground
// and air.

/// How hard the player accelerates on the ground, per unit of desired
/// speed.
///
/// **Eleven rather than fourteen, and it is the other half of making
/// the walk feel like a walk.** Fourteen brought a standing player to
/// full speed inside a tenth of a second, which is not walking, it is
/// a switch; the body reads as weightless whatever its top speed is.
/// At eleven it takes about a fifth of a second -- still immediate to
/// the hand, and long enough that starting to move is a thing that
/// happens rather than a state change. Much below this and the controls
/// go soft, which is the failure this number is bounded by at the
/// bottom.
///
/// **It must stay above `GROUND_FRICTION`, and that is not a matter of
/// taste.** Friction and acceleration are integrated one step at a
/// time; while acceleration wins, the player reaches the speed cap and
/// is *clamped* there, which is the same number at every frame rate.
/// Drop it below friction and there is no clamp any more -- the two
/// balance somewhere short of the cap, and where they balance depends
/// on the size of the step. The first attempt at this change set 11
/// against a friction of 12 and the two frame-rate independence tests
/// went red immediately: a running jump carried 4.49 blocks at 144 Hz
/// against 4.73 at 60. So both numbers came down together.
const GROUND_ACCEL: f32 = 11.0;
/// Speed bled off per second while standing on something.
///
/// Lowered with `GROUND_ACCEL` and for two reasons: the ordering above
/// has to hold, and a heavier walk that stopped dead would be a body
/// with mass in one direction only.
const GROUND_FRICTION: f32 = 9.0;
/// Acceleration available in mid-air.
const AIR_ACCEL: f32 = 8.0;
/// The cap that stops mid-air acceleration.
///
/// This is a limit on the component of velocity *along the direction the
/// player is asking for*, not on total speed. Jump while sprinting and
/// your forward speed is already far past it, so pressing forward adds
/// nothing: momentum is preserved and cannot be added to. Press sideways
/// and you can still nudge the arc, because sideways speed starts near
/// zero. That is the difference between steering and accelerating.
const AIR_CONTROL_SPEED: f32 = 1.6;

/// How long a flying player takes to reach the speed they asked for, or
/// to come back to rest, as a time constant in seconds.
///
/// **Flight steers toward a velocity rather than being pushed and
/// dragged**, which is a real difference and not a shortcut. Walking is
/// a body: an acceleration fights a friction, and where the two balance
/// *is* the top speed -- which is why `move_speed` and `GROUND_FRICTION`
/// have to be chosen together. Flight has no ground to push off and
/// nothing to rub against, so the same construction gives an
/// equilibrium that has nothing to do with the number anybody typed. It
/// was tried, and `/fly 12` flew at eight.
///
/// So the target velocity is the answer and this is only how quickly it
/// is approached. Eighty milliseconds reaches nineteen twentieths of it
/// in a quarter of a second, in both directions: immediate to a player,
/// and not so immediate that a stop is a wall.
const FLY_RESPONSE_SECONDS: f32 = 0.08;

/// Vertical speed while flying, as a fraction of the horizontal one.
///
/// Below one, because a player looking straight down and holding
/// descend should not outrun their own render distance.
const FLY_CLIMB_FACTOR: f32 = 0.8;

/// What the client flies at until a server says otherwise.
///
/// Only ever used if a `Flight` message arrives with a speed that makes
/// no sense; the granting side always names one. See
/// `protocol::ServerMessage::Flight`.
pub const DEFAULT_FLY_SPEED: f32 = 12.0;

/// What the collider asks of the world: a column of cells, or one cell.
///
/// A trait rather than the chunk store itself so that the same sweep can be
/// run against the store as it is (the tests, which stand near zero) and
/// against the store seen from beside the player ([`Local`]), which is what
/// `update` does.
pub trait Solids {
    fn column(&self, bx: i32, bz: i32) -> Option<crate::logic::chunk_manager::Column<'_>>;
    fn block_at(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId>;
    /// A raft, as this view of the world measures it.
    fn deck(&self, deck: &primitive_shared::raft::Body) -> primitive_shared::raft::Body {
        *deck
    }
}

impl Solids for ChunkManager {
    #[inline]
    fn column(&self, bx: i32, bz: i32) -> Option<crate::logic::chunk_manager::Column<'_>> {
        ChunkManager::column(self, bx, bz)
    }
    #[inline]
    fn block_at(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
        ChunkManager::block_at(self, x, y, z)
    }
}

/// The chunk store seen from the corner of the block the player is in.
///
/// ## The failure
///
/// "на больших координатах начинаются проблемы с движением". The feet were
/// an `f32` in world space, and an `f32` a million blocks out has neighbours
/// 0.0625 apart; at two million, 0.125; at ten million, a whole block. Every
/// frame adds a velocity times a sixtieth of a second to that, and the sum
/// rounds to the grid: a sneak of 0.065 a frame at two million blocks is
/// either nothing or 0.125, so the player stood still or ran at twice the
/// pace. The collider was worse. `CONTACT_SKIN` is a ten-thousandth of a
/// block, a player is 0.3 either side of their middle, and a wall's face is
/// a whole number -- none of which the numbers could say out there, so a
/// wall was met a sixteenth early or late and `escape_solids` flung the
/// player out of walls they had only rounded into.
///
/// ## What is done instead
///
/// The position is kept in `f64` ([`Player::position`]), and a move is
/// worked out in a frame whose corner is the whole block under the feet:
/// the feet become a number between nought and one, the cells are asked for
/// by their offset from the corner, and a block's box -- which depends on
/// the block and never on where it is (`geometry::block_box` only adds the
/// cell) -- comes back as small numbers too. Height is left as it is: it
/// never leaves a few hundred.
///
/// Weighed:
///
/// * *Every position in physics an `f64`.* The collider, the sweep, the
///   step and the escape would each be rewritten in a second float type,
///   and the boxes `geometry` hands over are `f32` already -- in world
///   space, which is exactly where they cannot be precise.
/// * *A frame per session, moved when the player strays far.* Every piece
///   of client state measured in it would have to move with it, and a
///   piece that was forgotten would be a teleport.
/// * **A frame per move (chosen).** Nothing outside `update` ever sees it,
///   so nothing outside can be left in the wrong one.
pub struct Local<'a> {
    chunks: &'a ChunkManager,
    x: i32,
    z: i32,
}

impl<'a> Local<'a> {
    /// The frame whose corner is the block `feet` stands in.
    pub fn around(chunks: &'a ChunkManager, feet: DVec3) -> Self {
        Self { chunks, x: feet.x.floor() as i32, z: feet.z.floor() as i32 }
    }

    /// A world position, measured from the corner.
    #[inline]
    pub fn local(&self, world: DVec3) -> Vec3 {
        Vec3::new((world.x - f64::from(self.x)) as f32, world.y as f32, (world.z - f64::from(self.z)) as f32)
    }

    /// A position measured from the corner, back in the world.
    #[inline]
    pub fn world(&self, local: Vec3) -> DVec3 {
        DVec3::new(f64::from(self.x) + f64::from(local.x), f64::from(local.y), f64::from(self.z) + f64::from(local.z))
    }

}

impl Solids for Local<'_> {
    /// A raft, moved into the frame.
    fn deck(&self, deck: &primitive_shared::raft::Body) -> primitive_shared::raft::Body {
        primitive_shared::raft::Body {
            x: deck.x - f64::from(self.x),
            z: deck.z - f64::from(self.z),
            ..*deck
        }
    }
    #[inline]
    fn column(&self, bx: i32, bz: i32) -> Option<crate::logic::chunk_manager::Column<'_>> {
        self.chunks.column(bx + self.x, bz + self.z)
    }
    #[inline]
    fn block_at(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
        self.chunks.block_at(x + self.x, y, z + self.z)
    }
}

pub struct Player {
    /// Feet position (bottom-centre of the collider), world space.
    ///
    /// **`f64`, and nothing that moves the player is done in it.** A body
    /// a million blocks out is a number whose `f32` neighbours are six
    /// centimetres apart, so a position kept in `f32` walks on a grid:
    /// see [`Local`] for what that did and where the arithmetic happens
    /// instead.
    pub position: DVec3,
    /// The feet measured from the corner of the frame `update` is working
    /// in. Only meaningful inside `update`; see [`Local`].
    at: Vec3,
    /// Other players' feet in that frame, kept to be refilled rather than
    /// allocated every frame.
    others: Vec<Vec3>,
    pub velocity: Vec3,
    pub grounded: bool,
    pub move_speed: f32,
    /// How far up the player the water comes, in blocks, capped at their
    /// own height.
    ///
    /// **The one measurement everything about water is derived from.**
    /// It used to be three independent samples answering three
    /// yes-or-nos, which is the same thing only while every cell of
    /// water is full to the brim: a cell can hold an eighth now, so
    /// "there is water at my feet" and "I am in water" stopped being
    /// the same statement. A depth answers both, and buoyancy needs it
    /// anyway.
    pub submersion: f32,
    /// Any part of the collider is in water. Wading counts.
    pub in_water: bool,
    /// There is more water than legs: no walking, only swimming.
    pub swimming: bool,
    /// The head is under water (drives the underwater fog).
    pub submerged: bool,
    /// Multiplier on walking and sprinting speed, from carried weight.
    ///
    /// Applied here rather than by scaling `move_speed` directly,
    /// because `move_speed` is the player's *setting* and overwriting it
    /// with a derived value means the setting is gone the moment
    /// anything recomputes it.
    pub speed_scale: f32,
    /// Snowshoes on the feet: a drift is walked over rather than waded
    /// (`types::surface_drag_shod`). Set beside `speed_scale`, from the same
    /// equipment, by whoever drives this body.
    pub snowshoes: bool,
    /// How much of the water's lift the player still gets, 1 (bobs) to
    /// 0 (goes straight down).
    ///
    /// **The same number the server drowns them by**: it comes from
    /// `load::buoyancy` off the same carried weight `speed_scale` comes
    /// from, and it is set beside it (`lib.rs`). A client that floated
    /// while the server thought the player was on the bottom would be a
    /// player drowning at the surface.
    pub buoyancy: f32,
    /// Whether the last `update` actually pushed off the ground.
    ///
    /// Not the same question as "was the jump key down", which is what
    /// the caller already knows: a jump is refused in mid-air, and
    /// swimming up is not a jump at all. Stamina is billed for the ones
    /// that happened, for the same reason the sprint is -- being charged
    /// for a jump that physics refused is the kind of thing a player
    /// notices and cannot explain.
    pub jumped: bool,
    /// How far the view is still behind the feet after a step up.
    ///
    /// Always positive, always shrinking. See `STEP_SMOOTHING_SECONDS`.
    step_lag: f32,

    /// Gravity does not apply, and the player moves where they point.
    ///
    /// **Granted by the server and never by the client.** The client
    /// does not decide this and cannot: it is set by
    /// `protocol::ServerMessage::Flight` and by nothing else, and the
    /// anti-cheat on the other end is what makes that statement mean
    /// something. See that message for why a mode that changes the
    /// client's own physics is not a hole in an authoritative model.
    pub flying: bool,

    /// How fast, in blocks per second, when flying. Server-supplied.
    pub fly_speed: f32,

    /// What the player is asking flight to do vertically this frame:
    /// `1.0` up, `-1.0` down, `0.0` hold. Ignored unless `flying`.
    ///
    /// A field set before `update` rather than an eighth argument to it,
    /// for the same reason `speed_scale` above is one: it means nothing
    /// in the fifty-odd calls that are not flying, and an argument that
    /// is `false` at every call site but one is noise at all of them.
    pub climb: f32,

    /// The rafts near enough to stand on, as they are this frame.
    ///
    /// **A floor that is not in the chunks.** A raft's deck moves and turns,
    /// so it cannot be cells, and the sweep that walks the player through
    /// cells cannot see it (see `raft` for why a raft is a body). What it
    /// gets instead is a floor test after the vertical move: feet over a deck
    /// and at or a little under its top are put on it (`stand_on_decks`).
    /// Set before `update`, like `climb`, and for the same reason: it means
    /// nothing in the calls that are not near water.
    pub decks: Vec<primitive_shared::raft::Body>,

    /// Whether the *client* has taken the movement keys away this frame --
    /// a screen is open, the game is paused -- rather than the player
    /// having let go of them.
    ///
    /// ## The trap this closes
    ///
    /// A player reported that with the breath meter running they could not
    /// get at their pack. They could open it; what they could not do was
    /// survive doing so. A loaded swimmer sinks (`load::buoyancy`, and the
    /// spring in `update`), and the only way to stop being a loaded swimmer
    /// is to open the pack and drop something -- which zeroes the wish
    /// direction and releases every key (`InputState::release_all`), so the
    /// seconds spent deciding what to throw away are seconds spent sinking
    /// faster into the thing being escaped. **The trouble was never that a
    /// loaded player sinks. It is that the one way out was itself the
    /// fastest way down.**
    ///
    /// So a body whose keys have been taken treads water: it holds the depth
    /// it is at instead of settling to the depth its load asks for. A person
    /// rummaging in a bag in deep water is not holding still, and this is
    /// what "not holding still" is worth.
    ///
    /// **It is a hold, not a lift**, and that is the whole of why the
    /// mechanic survives. Nobody rises by opening their pack, nobody travels
    /// with it open, and the breath meter runs the entire time. What it buys
    /// is that the decision costs what it takes to make, and not more.
    ///
    /// Rejected: leaving the keys live under an open screen, so a swimmer
    /// could keep stroking while sorting. It puts every key back in two
    /// places at once -- W is a stroke and the letter W in the search field
    /// beside the recipe list -- for a fix that is about one axis.
    /// Rejected too: a quick drop with no screen. That exists already
    /// (`Action::Drop`), and it throws away whatever is in hand rather than
    /// whatever is heaviest, which is the choice the player opened the pack
    /// to make.
    pub treading: bool,

    /// Which way the water the player is in is running, in blocks a second
    /// as (x, z): a river's current (`WorldGen::river_current`), asked of
    /// the generator by the frame before `update`, like `decks`. Nothing
    /// outside a river.
    ///
    /// **A swimmer is carried; a wader is not.** The stroke and the water's
    /// drag are both measured against the moving water rather than against
    /// the ground -- the argument `raft::step` makes for a hull -- so a body
    /// nobody is swimming ends up going where the river goes, and a swimmer's
    /// own speed is added to it: across a lazy reach they drift a few metres
    /// downstream and land on the far bank, and against a rapid faster than
    /// they swim they go backwards however hard they try. Feet on the bottom
    /// are not swimming (`swimming`), so a ford is exactly the place a river
    /// can be walked across whatever it is doing, and that is the decision a
    /// river puts to a player: where to cross, not whether.
    ///
    /// Rejected: a push added to the velocity every frame, the way a wind
    /// would be. Against drag that is quadratic in the speed through the
    /// water it has no fixed point at the river's speed -- a drifting body
    /// ran at a fraction of the current and a swimmer with the current at
    /// more than both together, depending on the frame rate.
    pub current: (f32, f32),
}

impl Player {
    pub fn new(spawn: DVec3, move_speed: f32) -> Self {
        Self {
            position: spawn,
            at: Vec3::ZERO,
            others: Vec::new(),
            velocity: Vec3::ZERO,
            grounded: false,
            move_speed: move_speed.clamp(0.5, 20.0),
            submersion: 0.0,
            in_water: false,
            swimming: false,
            submerged: false,
            speed_scale: 1.0,
            snowshoes: false,
            buoyancy: 1.0,
            jumped: false,
            step_lag: 0.0,
            flying: false,
            fly_speed: DEFAULT_FLY_SPEED,
            climb: 0.0,
            decks: Vec::new(),
            treading: false,
            current: (0.0, 0.0),
        }
    }

    pub fn eye_position(&self) -> DVec3 {
        self.position + DVec3::new(0.0, f64::from(EYE_HEIGHT), 0.0)
    }

    /// How far *below* the eye the view should be drawn while a step
    /// catches up. Zero except in the tenth of a second after one.
    ///
    /// Deliberately not folded into `eye_position`: that is where the
    /// player is, and it is what the interaction ray is cast from and
    /// what reach is measured against. Only the view lags -- an aim that
    /// lagged with it would put the crosshair somewhere the player is
    /// not looking, and the server, which knows nothing of any of this,
    /// would disagree about what was clicked.
    pub fn view_step_lag(&self) -> f32 {
        self.step_lag
    }

    /// Server-authoritative reposition (anti-cheat correction, or a future
    /// spawn/respawn). Velocity is cleared so the player doesn't
    /// immediately continue the move that got them corrected.
    /// Turns flight on or off. See the [`flying`](Self::flying) field.
    ///
    /// Leaving it drops the player: velocity is kept, so switching off
    /// mid-air is a fall from wherever you were rather than a
    /// teleport to the ground. That is the honest behaviour and it is
    /// also the one that cannot be used to cross a gap for free.
    pub fn set_flying(&mut self, flying: bool, speed: f32) {
        self.flying = flying;
        if speed.is_finite() && speed > 0.0 {
            self.fly_speed = speed.clamp(1.0, 80.0);
        }
        if !flying {
            self.climb = 0.0;
        }
    }

    /// `refresh_fluid_state` from outside `update`, for the tests that ask
    /// about water without walking into it.
    #[cfg(test)]
    fn refresh_fluid_state_here(&mut self, chunks: &ChunkManager) {
        let frame = Local::around(chunks, self.position);
        self.at = frame.local(self.position);
        self.refresh_fluid_state(&frame);
    }

    pub fn teleport(&mut self, position: DVec3) {
        self.position = position;
        self.velocity = Vec3::ZERO;
    }

    /// Horizontal speed this frame, in blocks per second.
    ///
    /// Read by the camera bob, which has to know how fast the player is
    /// actually moving rather than how fast they asked to move -- a
    /// player running into a wall should not bob.
    pub fn horizontal_speed(&self) -> f32 {
        Vec3::new(self.velocity.x, 0.0, self.velocity.z).length()
    }

    /// `wish_dir` is a normalized (or zero) horizontal move direction in
    /// world space, already combining WASD with camera yaw. `look` is
    /// where the camera is pointing, which matters only in water -- see
    /// `stroke_direction`. `other_players` are other players' current
    /// feet positions -- their hitboxes are solid obstacles too, same as
    /// blocks.
    ///
    /// Jump takes two flags because water and land want different ones:
    /// on land a jump fires on the press edge (holding the key must not
    /// auto-hop), in water holding it swims upward continuously.
    ///
    /// **Everything below is worked out beside the player, not at the
    /// planet's origin.** See [`Local`] for the failure this prevents: the
    /// world position is `f64`, and the move is measured in a frame whose
    /// corner is the whole block the feet are in, so every number the
    /// collider compares is under a few blocks whatever the coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        chunks: &ChunkManager,
        other_players: &[DVec3],
        wish_dir: Vec3,
        look: Vec3,
        jump_pressed: bool,
        jump_held: bool,
        sprinting: bool,
        dt: f32,
    ) {
        let frame = Local::around(chunks, self.position);
        self.at = frame.local(self.position);
        let mut others = std::mem::take(&mut self.others);
        others.clear();
        others.extend(other_players.iter().map(|&other| frame.local(other)));
        self.step(&frame, &others, wish_dir, look, jump_pressed, jump_held, sprinting, dt);
        self.others = others;
        self.position = frame.world(self.at);
    }

    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        chunks: &impl Solids,
        other_players: &[Vec3],
        wish_dir: Vec3,
        look: Vec3,
        jump_pressed: bool,
        jump_held: bool,
        sprinting: bool,
        dt: f32,
    ) {
        self.jumped = false;
        self.refresh_fluid_state(chunks);

        // Sprinting is a land move. Swimming has its own top speed and
        // its own drag, and letting the multiplier through would make
        // swimming faster than running -- but *wading* is running, in
        // water up to your knees, and refusing to sprint through a
        // puddle was one of the things that made shallow water feel
        // like a trap.
        //
        // What is underfoot slows you down: snow is the one surface
        // you go *through* rather than over, and the deeper it is the
        // more of a stride it takes. Sampled once per frame, from the
        // cell the feet are in and the one below them -- a player
        // standing in a drift is in its cell, and one standing on a
        // full block of it is in the air above it. Water shallow enough
        // to walk through is the same idea and is folded in the same
        // way.
        let underfoot = self.surface_drag(chunks) * self.wade_drag() * self.stake_drag(chunks);
        let base = self.move_speed * self.speed_scale.clamp(0.05, 1.0) * underfoot;
        let speed = if sprinting && !self.swimming {
            base * SPRINT_MULTIPLIER
        } else {
            base
        };

        // How much of this step's *constant* vertical acceleration --
        // gravity, and only gravity -- has already been added to
        // `velocity.y`. Read back below to integrate the fall exactly
        // instead of to first order; see the note over `delta`.
        //
        // Measured rather than assumed to be `GRAVITY * dt`, because
        // terminal velocity can cut it short and then half of a
        // deceleration that never happened would be handed to the
        // position.
        let mut gravity_gained = 0.0f32;

        if self.flying {
            // **Flight is the first case and it wins outright**, water
            // included. A flying player in a lake is a flying player;
            // handing them buoyancy as well would mean the two systems
            // fighting over the same axis, and the one that lost would
            // do so intermittently.
            //
            // Collisions are *not* skipped. Flying through walls is a
            // separate power -- it needs the chunk streamer to keep up
            // with somebody inside solid rock, which it does not -- and
            // conflating the two would mean a mod that wanted to lift
            // somebody over a river also let them into the middle of a
            // mountain.
            //
            // Weight still tells: a player carrying half their own
            // body in rock flies slower, for the same reason they walk
            // slower. It is the one thing about a body that survives
            // into this mode, and leaving it out would make flight the
            // way to move a mountain.
            let speed = self.fly_speed * self.speed_scale.clamp(0.05, 1.0);
            let target = wish_dir * speed
                + Vec3::Y * self.climb.clamp(-1.0, 1.0) * speed * FLY_CLIMB_FACTOR;
            // Exponential, so the approach is framerate-independent:
            // the same quarter of a second at thirty frames and at six
            // hundred.
            let blend = 1.0 - (-dt / FLY_RESPONSE_SECONDS).exp();
            self.velocity += (target - self.velocity) * blend;
        } else if self.swimming {
            // **Entry is not a special case any more.** It used to be
            // one multiply on the frame the water was first touched --
            // `velocity.y *= 0.25` -- which is a number, not a splash:
            // the same instant stop whether you stepped off a kerb or
            // fell fifty blocks. Quadratic drag makes the depth you
            // reach a function of the speed you arrived at, which is
            // what entering water is, and it needed no code at all.
            //
            // **Buoyancy, not weak gravity.** A body less submerged
            // than it wants to be sinks and a body more submerged than
            // it wants to be rises, so a swimmer who presses nothing
            // ends up at the surface with their head out and stays
            // there. That is the whole difference between water you can
            // be in and water you have to get out of.
            // **Feet on the bottom push off it.** A body too loaded to float
            // stands on the bed of a lake, and there the jump key was only
            // the stroke -- three blocks a second against its own weight,
            // which is a slow wade upward, not a jump. The push is a jump
            // damped by the water round it (`wade_jump`), and the drag below
            // takes the rest: well under a land jump, and a jump.
            if self.grounded && jump_pressed {
                self.velocity.y = self.velocity.y.max(JUMP_VELOCITY * self.wade_jump());
                self.jumped = true;
            }
            let float = self.submersion / PLAYER_HEIGHT;
            // **What is carried decides how deep the player rides.**
            //
            // The lift is a spring toward a *depth*, and the load moves
            // the depth rather than weakening the spring. Weakening it
            // was the first shape and it was wrong in a way worth
            // recording: the resting point is where lift and gravity
            // cancel, and scaling only the lift moves that point past
            // full submersion at the first hint of weight -- so a
            // quarter of a load and a full one both went straight to
            // the bottom, and the ramp `load::buoyancy` describes did
            // not exist in the water.
            //
            // Moving the target instead gives the whole ramp: empty,
            // the player rides with their head out (`FLOAT_SUBMERSION`);
            // loaded, they ride lower; and past about two thirds of the
            // way down the target is deeper than the player is tall,
            // which is a body that cannot float at any depth and sinks.
            // See `load::buoyancy` for where the number comes from, and
            // `load_tests` for the three cases.
            let wanted = FLOAT_SUBMERSION
                + (1.0 - self.buoyancy.clamp(0.0, 1.0)) * (PLAYER_HEIGHT + 0.6 - FLOAT_SUBMERSION);
            if self.treading {
                // ...unless the client has taken the keys away, in which
                // case the body holds the depth it is at instead of settling
                // to the one its load asks for: treading water. See
                // `treading` for the trap this closes and for why it is a
                // hold and not a lift.
                //
                // The vertical is kicked out rather than left to the water's
                // own drag below, which is quadratic and therefore does
                // almost nothing to a slow sink -- a swimmer who opened
                // their pack while already going down would have gone on
                // going down for the whole time it was open, which is the
                // fault this is here to fix.
                self.velocity.y *= (-dt / TREAD_SECONDS).exp();
            } else {
                self.velocity.y += BUOYANCY * (float - wanted / PLAYER_HEIGHT) * dt;
            }

            // In the water's own frame from here to the drag: a river's
            // current is ground that slides. See `current`.
            //
            // **Plus whatever the flow simulation is moving where the body
            // is** (`fluid::running`), which the generator's river knows
            // nothing about: swimming in a pond somebody has cut into, you
            // are drawn toward the cut, and a lip with the pond pouring
            // over it takes you with it if you let it. Read here from the
            // chunks rather than handed in like `current`, because it is
            // the cell the body is in and the collider already has them;
            // it is nothing at all in water at rest, which is every sea.
            let running = self.running_water(chunks);
            let flow = Vec3::new(
                if self.current.0.is_finite() { self.current.0 } else { 0.0 } + running.0,
                0.0,
                if self.current.1.is_finite() { self.current.1 } else { 0.0 } + running.1,
            );
            self.velocity -= flow;

            // The stroke, which is where the player is looking rather
            // than merely where they are facing -- the only way down
            // there is.
            self.swim(stroke_direction(wish_dir, look), speed * WATER_MOVE_FACTOR, dt);

            // ...and the water pushes back, as the square of how fast
            // it is being pushed through. This is what sets every top
            // speed down here: the stroke pulls, the drag holds, and
            // letting go *coasts* rather than stopping dead.
            self.apply_water_drag(dt);
            self.velocity += flow;
        } else if self.grounded {
            // Friction first, then acceleration: releasing the keys has
            // to actually slow you down, and holding them has to win
            // against the friction that is trying to.
            //
            // **Both scaled by what is underfoot.** Grip is not a top
            // speed -- that is what `drag` above is -- it is how quickly
            // speed can be changed at all, so taking it away leaves a
            // player's top speed alone and stretches everything either
            // side of it: slow to get going, slow to stop, and a corner
            // you have to steer through rather than turn on the spot.
            // See `types::surface_grip`.
            let grip = self.surface_grip(chunks);
            self.apply_friction(GROUND_FRICTION * grip, dt);
            self.accelerate(wish_dir, speed, GROUND_ACCEL * grip, dt);

            if jump_pressed {
                self.velocity.y = JUMP_VELOCITY * self.wade_jump();
                self.jumped = true;
            }
            let before = self.velocity.y;
            self.velocity.y = (before + GRAVITY * dt).max(TERMINAL_VELOCITY);
            gravity_gained = self.velocity.y - before;
        } else {
            // Airborne. No friction -- there is nothing to rub against
            // -- and acceleration capped so hard that it can redirect a
            // jump but never speed one up. See `AIR_CONTROL_SPEED`.
            self.accelerate(wish_dir, AIR_CONTROL_SPEED, AIR_ACCEL, dt);
            let before = self.velocity.y;
            self.velocity.y = (before + GRAVITY * dt).max(TERMINAL_VELOCITY);
            gravity_gained = self.velocity.y - before;
        }

        // --- the jump key in water ---
        //
        // **Outside the swimming branch, and that is the whole of the
        // fix.** The stroke used to live inside it, so it stopped the
        // instant `swimming` did -- which is to say the instant the
        // water was no longer waist deep. That is exactly the moment a
        // player still needs it: they are half out of a lake, there is
        // nothing under their feet to jump off, and the one key that was
        // lifting them stops working. Measured against a bank: ten
        // seconds of holding jump and swimming into it left the player
        // at y=19.22 with the water at 19.88 and the shelf at 21 --
        // bobbing at the waterline for as long as anybody cared to
        // watch. "В воде невозможно прыгать" is what that feels like.
        //
        // Gated on `afloat` rather than on `swimming` for that reason,
        // and bounded by the same thing it always was: `submersion`
        // reaches zero at the surface, so the stroke ends there. It
        // cannot become a way to climb air.
        //
        // Both flags, because the two exist to tell an auto-hop from a
        // hold and there is no auto-hop to prevent down here -- a press
        // is a frame in which the key is down, and a tap has to do
        // something.
        //
        // After the branch above rather than inside it, which puts it
        // after `apply_water_drag`: the drag used to eat a third of
        // every stroke before it was ever applied, so `SWIM_UP_SPEED`
        // was a cap nobody reached and holding jump rose at 2.76 rather
        // than at the three the constant promises.
        //
        // **A stroke adds to a rise and never takes from one.** It was an
        // assignment capped at `SWIM_UP_SPEED`, so the frame after a jump from
        // the bottom of a ford -- off the ground, in the water, key still
        // down -- cut eight blocks a second to three: a hop of a fifth of a
        // block, and no way out of a ford onto the bank beside it. "В воде
        // прыгать нельзя нормально" was that clamp.
        if self.afloat(self.grounded) && (jump_held || jump_pressed) {
            if self.velocity.y < SWIM_UP_SPEED {
                self.velocity.y = (self.velocity.y + SWIM_STROKE_ACCEL * dt).min(SWIM_UP_SPEED);
            }
            if !self.swimming {
                self.clamber(chunks, other_players, wish_dir);
            }
        }

        // Before anything moves: if the player is *already* inside
        // something, get them out. See `escape_solids`.
        //
        // It can overrule the vertical velocity outright -- standing a
        // buried player on the roof of whatever buried them stops the
        // fall that put them there. When it does, the velocity below is
        // no longer the one gravity produced this step, and handing back
        // half of a fall that is no longer happening would leave the
        // move upward by a few millimetres: a player standing still
        // inside something would creep up out of it.
        let after_gravity = self.velocity.y;
        self.escape_solids(chunks, other_players);
        if self.velocity.y != after_gravity {
            gravity_gained = 0.0;
        }

        let mut delta = self.velocity * dt;
        // **A jump has to be the same jump on every machine, and it was
        // not.** Gravity is applied above and the position was then
        // advanced by the velocity that came *out* of it, which is a
        // whole step of falling charged to a body that only fell for
        // part of it -- so every step lost `gravity_gained * dt / 2` of
        // height. Over a jump that came to a measured apex of 1.32
        // blocks at thirty frames a second, 1.39 at sixty and 1.45 on a
        // machine fast enough not to need `PHYSICS_STEP` at all, with
        // the airtime and therefore the length of a running jump
        // stretching to match. The key does the same thing; the game
        // did not.
        //
        // Half of the step's own acceleration handed back is the
        // average of the velocity either side of it, which is exact for
        // a constant acceleration rather than merely better -- the
        // sampled positions now lie *on* the parabola, so the only
        // remaining spread is where a sample happens to fall relative
        // to the true apex, which is under a centimetre.
        //
        // Only gravity: `gravity_gained` is zero while flying and in
        // water, where the vertical acceleration is not constant (see
        // `BUOYANCY`) and this correction would be a guess rather than
        // an identity. A jump is *not* included either -- an impulse is
        // a change of velocity, not an acceleration acting across the
        // step, and averaging it would give away half the jump.
        delta.y -= gravity_gained * dt * 0.5;
        // Read before it is cleared: whether a step up is allowed
        // depends on having been on the ground when the move started,
        // and the horizontal axes are resolved before the vertical one
        // has decided anything about this frame.
        let was_grounded = self.grounded;
        self.grounded = false;

        // Resolve one axis at a time so hitting a wall doesn't also kill
        // vertical/other-horizontal motion in the same step.
        self.move_axis(chunks, other_players, delta.x, X, was_grounded);
        self.move_axis(chunks, other_players, delta.z, Z, was_grounded);
        self.move_axis(chunks, other_players, delta.y, Y, was_grounded);
        self.stand_on_decks(chunks, dt);

        // Landing on something while flying is contact, not ground: the
        // player has not stopped flying, and calling it ground would
        // give them footsteps and a jump they cannot use.
        if self.flying {
            self.grounded = false;
        }

        // Anything shallow enough to stand on that the player is
        // nonetheless inside -- a layer laid under their own feet is the
        // whole of this case -- lifts them rather than trapping them.
        self.settle_onto_step(chunks, other_players);

        // A frame that moved the player no distance downward -- standing
        // still in water, or coming to rest exactly on a surface --
        // never asks the sweep about the floor, so ground contact is
        // confirmed here instead. Only while not rising: a player one
        // frame into a jump is still a hair off the ground, and calling
        // that grounded would hand them a second jump.
        if !self.grounded && !self.flying && self.velocity.y <= 0.0 {
            let probe = self.at - Vec3::new(0.0, 4.0 * CONTACT_SKIN, 0.0);
            self.grounded = aabb_intersects_solid(chunks, probe);
        }

        self.step_lag = (self.step_lag - dt / STEP_SMOOTHING_SECONDS * PLAYER_STEP_HEIGHT).max(0.0);

        // Recompute after moving so the caller (fog, HUD) sees this
        // frame's state, not last frame's.
        self.refresh_fluid_state(chunks);
    }

    /// Puts feet that are over a raft's deck, and at or a little under its
    /// top, onto it. See `decks`.
    ///
    /// **A little under, not only exactly at**, and the depth is a decision.
    /// Exactly at would be a deck that can only be landed on from above:
    /// a swimmer beside a raft has their feet a body's length under its top
    /// and no ledge to climb, and would bob against its side for ever -- the
    /// bank the swimmer's ledge climb was written for, without the ledge.
    ///
    /// **Two reaches.** Feet within `STEP_ONTO` are put on the deck whatever
    /// the body is doing -- a step, as onto a block. A swimmer who is
    /// swimming *up* is lifted from as deep as a floating body's feet ride
    /// (`CLIMB_ABOARD`). The one reach of 0.9 this began with was measured
    /// short: a swimmer holding jump beside a deck rides with their feet 0.84
    /// under the water and so 1.14 under the planks, and never arrived
    /// (`a_swimmer_who_swims_up_beside_a_deck_climbs_onto_it`). The deep
    /// reach is for a stroke and not for a float, so somebody treading water
    /// beside a raft, or under it, is left where they are rather than pulled
    /// up through the logs.
    ///
    /// No sides and no underside: a raft is a floor. Walking into its edge
    /// from the water is not stopped, and standing up under it is not either
    /// -- a raft that could trap a swimmer beneath it is a raft that drowns
    /// people for being near it.
    ///
    /// **Stepped onto or climbed onto, never lifted onto.** Feet within a
    /// step's height (`PLAYER_STEP_HEIGHT`) are put on the planks at once and
    /// the view is left a step behind to catch up, exactly as a step onto a
    /// block is (`try_step`). Anything deeper -- a swimmer's stroke, a wader's
    /// jump from the shallows -- is a climb: the body is sent up at
    /// `LEDGE_CLIMB_SPEED`, the pace a bank is climbed at, through planks that
    /// have no underside, and is put on the top only once the top is less than
    /// this frame's rise away. Both used to be put on the top in the frame the
    /// feet came in reach, which moved the view 1.44 blocks in one frame for a
    /// swimmer, 1.18 for a wader and 0.19 for a walker off a bank -- what a
    /// player reported as the raft teleporting them onto it
    /// (`a_swimmer_climbing_onto_a_deck_arrives_where_the_last_stroke_left_them`
    /// and its two neighbours in `riding`).
    fn stand_on_decks(&mut self, frame: &impl Solids, dt: f32) {
        /// How far under a deck's top a pair of feet is reached for at all.
        const STEP_ONTO: f32 = 0.9;
        /// How far under it a swimmer swimming up is reached for: the depth
        /// a floating body's feet ride at, the deck's height over the water,
        /// and a hand besides.
        const CLIMB_ABOARD: f32 = FLOAT_SUBMERSION + primitive_shared::raft::FREEBOARD + 0.3;
        /// How fast a swimmer has to be rising to count as climbing: a
        /// stroke, and not the bob of a body settling at its float.
        const CLIMBING_SPEED: f32 = 0.5;
        if self.flying {
            return;
        }
        let rising = self.velocity.y > CLIMBING_SPEED;
        let reach = if self.in_water && rising { CLIMB_ABOARD } else { STEP_ONTO };
        for deck in &self.decks {
            let deck = frame.deck(deck);
            let local = deck.local_of(self.at.as_dvec3().to_array());
            if !primitive_shared::raft::Body::over_deck(local, primitive_shared::raft::DECK_SLACK) {
                continue;
            }
            let under = -local[1];
            if !(0.0..=reach).contains(&under) {
                continue;
            }
            let still_climbing = rising && under > self.velocity.y * dt;
            if under <= PLAYER_STEP_HEIGHT && !still_climbing {
                self.at.y = deck.deck_top();
                // Arrived: a climber is not still rising once they are on top,
                // or the last of the climb would hop them off the planks.
                self.velocity.y = 0.0;
                self.grounded = true;
                self.step_lag = (self.step_lag + under).min(STEP_LAG_MAX);
            } else {
                self.velocity.y = self.velocity.y.max(LEDGE_CLIMB_SPEED);
            }
            return;
        }
    }

    /// How much the surface underfoot slows walking, 0..1.
    ///
    /// The cell the feet are in first: a player wading through a drift
    /// is *inside* it, and that is the case that matters. Falling back
    /// to the cell below covers standing on top of a full block of the
    /// How much sharpened stakes hold a body back: all the way to
    /// `spikes::THROUGH_STAKES` while any of it is among the points.
    ///
    /// **A bundle of points is pushed through, not walked through.** The
    /// stakes were a cross the body passed at full stride and a cut on the
    /// way ("сделай движение в шипах медленным"): a wade, then, like snow
    /// and water are, so a ring of stakes costs time as well as blood and a
    /// charge through one is a charge that stops. The server reads the
    /// speed it measures, and a slower body is never one it doubts.
    fn stake_drag(&self, chunks: &impl Solids) -> f32 {
        // **`self.at`, not `self.position`**: `chunks` here is the move's own
        // frame (`Local`), which answers in blocks from the corner the feet
        // are in. The world position read through it looked a whole world
        // position further on -- at x 200 it asked about x 400 -- so stakes
        // slowed nobody anywhere but within a block of the origin, where
        // every unit test stood. The scenario runner found it by walking
        // through a real stand of them (`scenario::tests`).
        let p = self.at.as_dvec3();
        let half = f64::from(primitive_shared::geometry::PLAYER_HALF_WIDTH);
        let among = primitive_shared::spikes::touches(
            [p.x - half, p.y, p.z - half],
            [p.x + half, p.y + f64::from(primitive_shared::geometry::PLAYER_HEIGHT), p.z + half],
            |x, y, z| chunks.block_at(x, y, z).unwrap_or(primitive_shared::types::BLOCK_AIR),
        );
        if among {
            primitive_shared::spikes::THROUGH_STAKES
        } else {
            1.0
        }
    }

    /// stuff.
    fn surface_drag(&self, chunks: &impl Solids) -> f32 {
        let feet = self.at;
        let (x, z) = (feet.x.floor() as i32, feet.z.floor() as i32);
        let Some(column) = chunks.column(x, z) else {
            return 1.0;
        };
        let y = feet.y.floor() as i32;
        let inside = primitive_shared::types::surface_drag_shod(column.block(y), self.snowshoes);
        if inside < 1.0 {
            return inside;
        }
        // A hair below the feet, so standing exactly on top of a block
        // reads as standing on it rather than as standing in the air
        // over it.
        primitive_shared::types::surface_drag_shod(column.block((feet.y - 0.05).floor() as i32), self.snowshoes)
    }

    /// How well the surface underfoot holds a foot, 0..1.
    ///
    /// The cell *below* the feet rather than the one they are in, which
    /// is the opposite of `surface_drag` and right for the opposite
    /// reason: drag is what you are wading through and grip is what you
    /// are standing on. A player standing on ice with their ankles in a
    /// film of water is standing on ice.
    ///
    /// Falls back to the cell the feet are in, so that a player inside
    /// something -- the case a drift of snow makes ordinary -- is
    /// gripping that rather than whatever is buried under it.
    fn surface_grip(&self, chunks: &impl Solids) -> f32 {
        let feet = self.at;
        let Some(column) = chunks.column(feet.x.floor() as i32, feet.z.floor() as i32) else {
            return 1.0;
        };
        // A hair below the feet, so standing exactly on top of a block
        // reads as standing on it rather than as standing in the air
        // above it -- the same offset `surface_drag` uses, and for the
        // same reason.
        let below = primitive_shared::types::surface_grip(column.block((feet.y - 0.05).floor() as i32));
        if below < 1.0 {
            return below;
        }
        primitive_shared::types::surface_grip(column.block(feet.y.floor() as i32))
    }

    /// Is what the player stands on part of a tree -- a bough, a twig, a
    /// crown of leaves, or a trunk lying or standing?
    ///
    /// Asked by the frame loop before a jump, which it bills as a climb when
    /// this says so (`Stamina::climb_cost`). Read a hair below the feet, the
    /// offset `surface_grip` uses, so standing on top of a bough is standing
    /// on it. The ground under a tree is not the tree: a player at the foot
    /// of one jumps like anybody else, and pays the climb from the first
    /// bough up.
    pub fn footing_is_a_tree(&self, chunks: &ChunkManager) -> bool {
        use primitive_shared::types as t;
        if !self.grounded {
            return false;
        }
        let feet = self.position;
        let Some(column) = chunks.column(feet.x.floor() as i32, feet.z.floor() as i32) else {
            return false;
        };
        let under = column.block((feet.y - 0.05).floor() as i32);
        // A trunk is whatever lies as deadfall when it is felled
        // (`BlockDef::felled`): every log of every kind, and a kind added
        // later without a line here.
        t::is_branch(under) || t::is_leafy(under) || primitive_shared::blocks::definition(under).felled.is_some()
    }

    /// Is the player held up by water rather than by the ground?
    ///
    /// **The question everything that gets you *out* of a lake has to
    /// ask, and `swimming` is not it.** Swimming is a movement style,
    /// and it is deliberately false in water shallower than
    /// `SWIM_DEPTH` -- otherwise an ankle-deep puddle would be
    /// something to swim in, which is the fault
    /// `a_shallow_puddle_is_walked_through_rather_than_swum_in` pins
    /// down. But the way *up* -- the jump stroke and the ledge climb --
    /// was gated on that same flag, so both switched off at waist depth:
    /// half out of the water, nothing under the feet, and the key that
    /// had been lifting you doing nothing at all.
    ///
    /// So: any water at all, and no ground. Bounded by the water itself
    /// -- `in_water` is `submersion > 0`, which ends at the surface --
    /// so nothing here can lift anybody through air.
    ///
    /// The caller supplies whether the player is on the ground because
    /// the two callers mean different instants by it: `update` asks
    /// before the flag is cleared for the frame, and `move_axis` asks
    /// about the frame the move *started* in, which is the same
    /// `was_grounded` that decides a step-up.
    fn afloat(&self, on_ground: bool) -> bool {
        self.in_water && !on_ground && !self.flying
    }

    /// How much of a jump survives the water the legs are in, 0..1: all of
    /// it at the waterline and `1 - WATER_JUMP_LOSS` from waist deep down.
    ///
    /// **A tenth, and the ford decides it.** Standing on the bottom of one
    /// cell of water the feet are 0.88 under the surface and the bank beside
    /// is a block up; a jump has to carry more than a block from there or a
    /// ford is a pit. A tenth off the take-off is a fifth off the height --
    /// 1.18 against 1.45 on land -- which is a jump that feels the water and
    /// still gets out of it. Rejected: the water's own drag on a wader. It is
    /// quadratic, so it takes most of an eight-block-a-second take-off in the
    /// first tenth of a second and the ford became a pit again.
    fn wade_jump(&self) -> f32 {
        1.0 - WATER_JUMP_LOSS * (self.submersion / SWIM_DEPTH).clamp(0.0, 1.0)
    }

    /// A swimmer at the surface, pressing jump and swimming at a bank, is
    /// thrown up onto it -- if a jump from here would carry them there.
    ///
    /// **The way out of a lake onto a bank a block high.** The ledge climb
    /// (`LEDGE_CLIMB_SPEED`) lifts a swimmer along a bank only while they are
    /// in the water, and stops at the surface on purpose; the step-up needs
    /// ground. So a bank whose top stood a block over the water was a wall:
    /// the swimmer bobbed at its foot for as long as they held the keys.
    ///
    /// What this hands out is exactly a jump and never more: the speed that
    /// puts the feet a hand over the lip, and only when that is no faster
    /// than `JUMP_VELOCITY` -- so nothing is reached from the water that
    /// could not be reached by jumping from a floor at the surface. Only in
    /// the band above `SWIM_DEPTH`, where nothing but gravity acts on the
    /// rise and the speed worked out here is the one the body flies at.
    /// Only once per rise: a body already moving up faster than a stroke is
    /// already on its way, and billing it again every frame would charge the
    /// stamina of ten jumps for one.
    ///
    /// Rejected: letting the ledge climb run past the surface. Any bank
    /// would then be climbed at any height, which is the cliff the climb was
    /// bounded to keep swimmers off.
    fn clamber(&mut self, chunks: &impl Solids, other_players: &[Vec3], wish_dir: Vec3) {
        /// How far ahead a bank is felt for: a hand, not a stride.
        const REACH: f32 = 0.1;
        /// How far over the lip the feet are thrown, so the body has a
        /// moment above it to move forward onto it.
        const CLEARANCE: f32 = 0.15;
        if self.velocity.y > SWIM_UP_SPEED {
            return;
        }
        let Some(ahead) = Vec3::new(wish_dir.x, 0.0, wish_dir.z).try_normalize() else {
            return;
        };
        let reached = self.at + ahead * REACH;
        let (min, max) = player_box(reached);
        let mut top = f32::NEG_INFINITY;
        for_each_solid_tagged(chunks, min, max, |bmin, bmax, bark| {
            let touched = min.x < bmax[0] - CONTACT_SKIN
                && max.x > bmin[0] + CONTACT_SKIN
                && min.z < bmax[2] - CONTACT_SKIN
                && max.z > bmin[2] + CONTACT_SKIN
                && min.y < bmax[1] - CONTACT_SKIN
                && max.y > bmin[1] + CONTACT_SKIN;
            if touched {
                top = top.max(if bark { f32::INFINITY } else { bmax[1] });
            }
        });
        let rise = top - self.at.y;
        if !rise.is_finite() || rise <= 0.0 {
            return;
        }
        // **Measured from the surface, not from the feet.** Measured from the
        // feet, a body falling back through the band after one clamber was
        // offered another from lower down, and two of them climbed a bank
        // two blocks over the water.
        let surface = self.at.y + self.submersion;
        if top - surface + CLEARANCE > JUMP_VELOCITY * JUMP_VELOCITY / (2.0 * -GRAVITY) {
            return; // out of a jump's reach: keep swimming up
        }
        // **Not for a wader.** A body that would touch the bottom before it
        // was waist deep can stand, and a standing body jumps off the bottom
        // (`wade_jump`) -- once, from the ground. Offered this too, a ford
        // was a place to jump again in mid-air on the way down.
        if aabb_intersects_solid(chunks, self.at - Vec3::new(0.0, (SWIM_DEPTH - self.submersion).max(0.0) + CONTACT_SKIN, 0.0)) {
            return;
        }
        let needed = (2.0 * -GRAVITY * (rise + CLEARANCE)).sqrt();
        if needed > JUMP_VELOCITY {
            return;
        }
        let lifted = Vec3::new(0.0, rise + CONTACT_SKIN, 0.0);
        if overlaps_anything(chunks, other_players, self.at + lifted)
            || overlaps_anything(chunks, other_players, reached + lifted)
        {
            return; // no room over the lip, or none to rise into
        }
        self.velocity.y = needed;
        self.jumped = true;
    }

    /// How much the water you are wading through slows you, 0..1.
    ///
    /// One at the waterline and `WADE_SPEED` at the depth where wading
    /// becomes swimming, so the two meet without a step: the last stride
    /// before you start swimming is the slowest one, and it is not
    /// suddenly slower than the first stroke.
    fn wade_drag(&self) -> f32 {
        if self.swimming || self.submersion <= 0.0 {
            return 1.0;
        }
        1.0 - (1.0 - WADE_SPEED) * (self.submersion / SWIM_DEPTH).clamp(0.0, 1.0)
    }

    /// Adds speed along `stroke`, but never past `target` *in that
    /// direction*.
    ///
    /// The same projection `accelerate` uses on the ground and for the
    /// same reason -- see the long note there. Three axes rather than
    /// two, because in water there is no privileged horizontal plane:
    /// diving is a move like any other, and the cap has to apply to the
    /// dive as well or looking down would be a way to go faster than
    /// swimming.
    fn swim(&mut self, stroke: Vec3, target: f32, dt: f32) {
        if stroke.length_squared() < 1e-6 || target <= 0.0 {
            return;
        }
        let along = self.velocity.dot(stroke);
        let missing = target - along;
        if missing <= 0.0 {
            return;
        }
        self.velocity += stroke * (SWIM_ACCEL * target * dt).min(missing);
    }

    /// Quadratic drag, in proportion to how much of the body is in the
    /// water.
    ///
    /// Two coefficients rather than one, because a body is not the same
    /// shape from every direction -- see [`WATER_DRAG_SIDEWAYS`]. And
    /// scaled by the submerged fraction, so wading out of a lake is a
    /// continuous thing rather than a cliff at the waist: half a body
    /// in the water is half the water pushing back on it.
    ///
    /// **The loss is clamped to the speed itself**, which is the whole
    /// of what makes an explicit integration of a v-squared law safe. A
    /// body arriving at twenty blocks a second inside one step would
    /// otherwise be handed a deceleration larger than its own velocity
    /// and come out of the frame travelling *backwards* -- fast. That is
    /// not a rounding error, it is a body fired back out of a lake, and
    /// it is the one failure mode this model has.
    fn apply_water_drag(&mut self, dt: f32) {
        let wet = (self.submersion / PLAYER_HEIGHT).clamp(0.0, 1.0);
        if wet <= 0.0 {
            return;
        }

        let sideways = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
        let speed = sideways.length();
        if speed > 1e-4 {
            let resistance = WATER_LINEAR_SIDEWAYS * speed + WATER_DRAG_SIDEWAYS * speed * speed;
            let lost = (resistance * wet * dt).min(speed);
            let kept = (speed - lost) / speed;
            self.velocity.x *= kept;
            self.velocity.z *= kept;
        }

        let rising = self.velocity.y.abs();
        if rising > 1e-4 {
            let resistance = WATER_LINEAR_VERTICAL * rising + WATER_DRAG_VERTICAL * rising * rising;
            let lost = (resistance * wet * dt).min(rising);
            self.velocity.y -= lost * self.velocity.y.signum();
        }
    }

    /// Bleeds horizontal speed off, framerate-independently.
    ///
    /// **The exponential is the whole of that claim, and the claim used
    /// to be false.** This was `v * (1 - friction * dt)`, which is the
    /// first-order approximation of the same law -- and first order is
    /// not good enough for the thing a player actually feels here, which
    /// is *how far they slide after letting go*. That distance came out
    /// as `v / friction * (1 - friction * dt)`: 0.37 blocks at sixty
    /// frames a second against 0.46 at three hundred, on the same stone,
    /// from the same speed. A quarter of the stopping distance decided by
    /// the machine, and decided the wrong way round -- the worse the
    /// frame rate, the more abruptly the player stopped, so the game got
    /// *twitchier* exactly when it was struggling.
    ///
    /// Worse at the far end: past `1 / friction` seconds in one step the
    /// factor went negative and was clamped to zero, so on stone
    /// (friction 12) any frame longer than a twelfth of a second stopped
    /// a sprinter dead. `PHYSICS_STEP` in `lib.rs` keeps the shipped game
    /// off that cliff; the collider's own tests, which run frames of a
    /// tenth of a second on purpose, were over it.
    ///
    /// The construction is the one flight already uses for the same
    /// reason -- see [`FLY_RESPONSE_SECONDS`].
    ///
    /// The *top* speed is unaffected, and that is worth saying because it
    /// is the number the anti-cheat is calibrated against: walking is an
    /// acceleration fighting this friction, and the acceleration step is
    /// capped by how much speed is actually missing (see `accelerate`),
    /// so the balance still lands exactly on `move_speed` whatever `dt`
    /// is.
    fn apply_friction(&mut self, friction: f32, dt: f32) {
        let speed = self.horizontal_speed();
        if speed < 1e-4 {
            self.velocity.x = 0.0;
            self.velocity.z = 0.0;
            return;
        }
        let scale = (-friction * dt).exp();
        self.velocity.x *= scale;
        self.velocity.z *= scale;
    }

    /// Adds speed in `wish_dir`, but never past `wish_speed` *in that
    /// direction*.
    ///
    /// The projection is the whole mechanism. Because the cap applies to
    /// the component of velocity along `wish_dir` rather than to the
    /// total, a player already moving faster than the cap in the
    /// direction they are asking for gets nothing at all, while one
    /// moving across it can still turn. Ground movement sets the cap to
    /// full walking speed and air movement to almost nothing, and that
    /// single number is the difference between the two.
    fn accelerate(&mut self, wish_dir: Vec3, wish_speed: f32, accel: f32, dt: f32) {
        if wish_dir.length_squared() < 1e-6 || wish_speed <= 0.0 {
            return;
        }
        let before = self.horizontal_speed();
        let along = self.velocity.x * wish_dir.x + self.velocity.z * wish_dir.z;
        let missing = wish_speed - along;
        if missing <= 0.0 {
            return;
        }
        let step = (accel * wish_speed * dt).min(missing);
        self.velocity.x += wish_dir.x * step;
        self.velocity.z += wish_dir.z * step;

        // **Hitting a wall must never make you faster.**
        //
        // The cap above is on the component of velocity *along the
        // direction asked for*, which is exactly what makes momentum
        // work -- and it has a hole in it that a wall opens. Press
        // almost-parallel into a wall and the blocked axis is zeroed
        // every frame, so that projection reads far below the real
        // speed; the cap then keeps handing out acceleration, and the
        // component *along the wall* grows without limit. At a shallow
        // enough angle it reaches tens of blocks a second: the player
        // is flung along the wall, and on a server the anti-cheat
        // rightly reads that as speed hacking and rubber-bands them
        // back -- which is what "it teleports me when I hit a wall"
        // was.
        //
        // So the total is capped as well: acceleration may bring a
        // player up to their own top speed and never past it, while
        // anything they *arrived* with is theirs to keep (a running
        // jump has to stay a running jump).
        let cap = wish_speed.max(before);
        let after = self.horizontal_speed();
        if after > cap && after > 1e-6 {
            let scale = cap / after;
            self.velocity.x *= scale;
            self.velocity.z *= scale;
        }
    }

    /// Samples the world for water at the feet and at eye level.
    ///
    /// **How deep the cell is, not merely whether it holds water.** Both
    /// halves of that matter now that water flows: an eighth-deep film
    /// left behind by a receding puddle is not something to swim in, and
    /// even a *full* cell of water stops a little short of the top of
    /// its cell (see `fluid::SURFACE_DROP`) -- so asking `is_liquid`
    /// turned the underwater fog on a hand's breadth above the surface
    /// the mesher had drawn. `fluid::covers` is the line the mesher
    /// draws, which is the whole reason it lives in the shared crate.
    ///
    /// **And the cell above each one**, which is the half of that rule
    /// this used to be missing. `fluid::surface_height` is the
    /// *shoreline* answer; underneath a surface the drop belongs to the
    /// cell where the air starts and to nobody else, which is what
    /// `fluid::surface_height_with_above` says and what the server's
    /// drowning check and the anti-cheat have both read since the day a
    /// player drowned on the sea floor found it. See the note at the
    /// maximum below for what the client's copy of that mistake cost.
    /// What the flow simulation is moving past the body, in blocks a second
    /// as (x, z): `fluid::running` read at the cell the feet are in, or the
    /// one over it where the feet are below a full cell's water. Directions
    /// are the same in the local frame as in the world's, so nothing needs
    /// turning back.
    fn running_water(&self, chunks: &impl Solids) -> (f32, f32) {
        let (x, z) = (self.at.x.floor() as i32, self.at.z.floor() as i32);
        let feet = self.at.y.floor() as i32;
        for y in [feet, feet + 1] {
            let push = primitive_shared::fluid::running(x, y, z, &|x, y, z| chunks.block_at(x, y, z));
            if push != (0.0, 0.0) {
                return primitive_shared::fluid::running_velocity(push);
            }
        }
        (0.0, 0.0)
    }

    fn refresh_fluid_state(&mut self, chunks: &impl Solids) {
        let feet = self.at;
        let Some(column) = chunks.column(feet.x.floor() as i32, feet.z.floor() as i32) else {
            self.submersion = 0.0;
            self.in_water = false;
            self.swimming = false;
            self.submerged = false;
            return;
        };

        // The highest water surface anywhere inside the collider, which
        // is not always the cell the feet are in: a player standing in
        // a doorway of a flooded room has water over their head and air
        // around their ankles, and what they are in is the water.
        //
        // One column rather than three cell lookups. The block store is
        // a hash map keyed by chunk, and this runs twice per physics
        // step; finding the column once and reading down it is the same
        // move the collider makes for the same reason.
        let first = feet.y.floor() as i32;
        let last = (feet.y + PLAYER_HEIGHT).floor() as i32;
        let mut surface = f32::NEG_INFINITY;
        for gy in first..=last {
            let block = column.block(gy);
            if primitive_shared::types::is_liquid(block) {
                // **With the cell above in hand, which is the whole of
                // what `surface_height` alone gets wrong.** A full cell
                // of water stops `fluid::SURFACE_DROP` short of its
                // ceiling, and that drop belongs to the cell where the
                // air starts and to no other -- so asking
                // `surface_height` of a *submerged* cell put the top of
                // the water an eighth of a block below where the mesher
                // draws it and where the server's drowning check and the
                // anti-cheat both read it (`fluid::covers_with_above`,
                // which is where the same twelve per cent was found and
                // closed on the other side of the wire).
                //
                // Nothing about the *waterline* moved: the topmost wet
                // cell has air over it and still answers 0.88, and the
                // maximum below picks that cell whenever the player's
                // head is anywhere near it. What moved is a player
                // wholly under water, whose submersion used to swing
                // between 1.68 and 1.8 with nothing but the fraction of
                // a block their feet happened to be standing at -- and
                // buoyancy is a spring in that number (see `BUOYANCY`),
                // so the lift on a diver pulsed between 2.4 and 4.0
                // blocks a second squared, once a block, for as long as
                // they swam. It read as the water being lumpy.
                let above = column.block(gy + 1);
                surface = surface
                    .max(gy as f32 + primitive_shared::fluid::surface_height_with_above(block, above));
            }
        }

        self.submersion = (surface - feet.y).clamp(0.0, PLAYER_HEIGHT);
        self.in_water = self.submersion > 0.0;
        self.swimming = self.submersion >= SWIM_DEPTH;
        // Asked of the depth rather than of the eye's own cell, so the
        // one number decides all three. The answer is the same in every
        // case either would call ordinary.
        self.submerged = self.submersion > EYE_HEIGHT;
    }

    /// Moves along one axis, stopping *against* whatever is in the way
    /// rather than giving up on the whole step.
    ///
    /// **This used to revert the move entirely.** Reverting is the
    /// plan's suggested starting point and it was fine while every solid
    /// thing was a whole cell: the player ends the frame up to one
    /// frame's travel short of the surface, and the next frame closes a
    /// little more of the gap, so the error is invisible on a floor
    /// whose height is always an integer. It stops being fine the moment
    /// surfaces sit at eighths of a block -- the player hovers a
    /// centimetre or two above a drift of snow, the gap changes with
    /// frame rate, and the same drift reads as a different height
    /// depending on how fast the machine is. Sweeping to contact puts
    /// the feet exactly on the surface, whatever height it is.
    fn move_axis(
        &mut self,
        chunks: &impl Solids,
        other_players: &[Vec3],
        delta: f32,
        axis: usize,
        was_grounded: bool,
    ) {
        if delta == 0.0 {
            return;
        }
        let hit = sweep_axis(chunks, other_players, self.at, delta, axis);
        if !hit.blocked {
            self.at[axis] += hit.allowed;
            return;
        }

        // Blocked horizontally, on the ground, by something low: walk up
        // it instead of into it. Without this a single layer of ash
        // across a path stops a running player dead, which is not what
        // an ankle-deep drift does.
        //
        // **Not while rising.** A step is a walk-up, and a player on the
        // way up out of a jump is not walking. Letting it fire there
        // added half a block to a jump that hit a ledge -- and, worse,
        // it set `grounded` in the middle of a climb, which is the
        // client telling the server it is standing on something while
        // demonstrably ascending over air. That is the signature of a
        // flight cheat, the anti-cheat flags it, and the correction it
        // sends *is* the teleport players were seeing at walls.
        //
        // Tried before the partial move is applied, so the step starts
        // from where the player was rather than from where they were
        // stopped -- otherwise a successful step travels the blocked
        // distance twice.
        if axis != Y
            && was_grounded
            && self.velocity.y <= 0.0
            && self.try_step(chunks, other_players, delta, axis, hit.top)
        {
            return;
        }
        self.at[axis] += hit.allowed;

        // Zero the offending component so gravity doesn't keep trying to
        // push us through the floor every frame.
        if axis == Y {
            if self.velocity.y < 0.0 {
                self.grounded = true;
            }
            self.velocity.y = 0.0;
        } else {
            // A swimmer pressed against something climbs it instead of
            // stopping against it. See `LEDGE_CLIMB_SPEED`: this is the
            // step-up a swimmer cannot have, because a step-up needs
            // ground to have been standing on.
            //
            // Set to a floor rather than added, so it is a steady rise
            // along the obstacle and not an accumulating shove -- and
            // only while the player is actually asking to go that way,
            // which is what `hit.blocked` on a non-zero delta means.
            //
            // `afloat` and not `swimming`, for the reason written out
            // over the stroke in `update`: the climb used to stop where
            // the water stopped being waist deep, which left the player
            // pinned against the bank with their head out and no way up
            // it. It ends at the surface either way -- that is what
            // `in_water` means -- and `was_grounded` keeps it from
            // becoming a free ride up a cliff for somebody merely
            // *standing* in a ford.
            if self.afloat(was_grounded) {
                self.velocity.y = self.velocity.y.max(LEDGE_CLIMB_SPEED);
            }
            self.velocity[axis] = 0.0;
        }
    }

    /// Tries to finish a blocked horizontal move by rising over what
    /// blocked it. Returns whether it worked.
    ///
    /// Three things have to hold, and each of them is a way this could
    /// otherwise become a cheat rather than a convenience: the obstacle
    /// has to be low enough to step onto, there has to be room for the
    /// player at the raised height (or a step into a one-block gap would
    /// push their head into the ceiling), and the move has to actually
    /// complete up there (or the player would be lifted for nothing and
    /// then walk into the same wall).
    fn try_step(
        &mut self,
        chunks: &impl Solids,
        other_players: &[Vec3],
        delta: f32,
        axis: usize,
        obstacle_top: f32,
    ) -> bool {
        let lift = obstacle_top - self.at.y;
        if lift <= 0.0 || lift > PLAYER_STEP_HEIGHT {
            return false;
        }
        let raised = self.at + Vec3::new(0.0, lift + CONTACT_SKIN, 0.0);
        if overlaps_anything(chunks, other_players, raised) {
            return false;
        }
        let hit = sweep_axis(chunks, other_players, raised, delta, axis);
        if hit.blocked {
            return false;
        }
        self.at = raised;
        self.at[axis] += hit.allowed;
        self.grounded = true;
        self.velocity.y = self.velocity.y.max(0.0);
        self.step_lag = (self.step_lag + lift).min(STEP_LAG_MAX);
        true
    }

    /// Pushes the player out of anything they are *already* inside.
    ///
    /// **The bug this fixes is being stuck in a wall.** Collision
    /// resolution answers "may I move there", and it answers it
    /// correctly -- but it has nothing to say about a player who is
    /// inside a block before the frame starts, and every direction they
    /// then try to move is blocked by the very block they are in. The
    /// player is welded in place, and since the floor of that block is
    /// under their feet the whole time, it reads exactly like having
    /// sunk into the wall.
    ///
    /// It is not a rare state. A block can appear around a player: sand
    /// lands on them, terrain arrives late while they are falling
    /// through where it will be, another player builds against them at
    /// the moment they step back, or the server rubber-bands them into
    /// geometry. Placement refuses to build *into* a player, which
    /// covers the deliberate case and none of the others.
    ///
    /// **Another player is one of the things you can be inside**, and
    /// the commonest one in a game with more than one person in it.
    /// A remote player's box is carried by the network rather than by
    /// this collider, so it walks straight through anybody standing
    /// still -- and this used to look only at blocks, so whoever it
    /// landed on was blocked on all six sides by the very box they were
    /// in. Not stuck in a wall: stuck in mid-air, unable to walk and
    /// unable to fall, until the other player wandered off.
    ///
    /// Up first, and by a wide margin: standing on the block that
    /// arrived is what a person would do, and it is the only escape
    /// that never drops anybody through a floor. Sideways is the
    /// fallback, shallowest side first. Somebody sealed in solid rock
    /// gets neither, and has to dig -- but they are not being dragged
    /// anywhere either.
    fn escape_solids(&mut self, chunks: &impl Solids, other_players: &[Vec3]) {
        let (min, max) = player_box(self.at);
        // How far to move along each direction to clear everything the
        // collider currently overlaps.
        let mut trapped = false;
        let mut up = 0.0f32;
        let mut push = [0.0f32; 4]; // +x, -x, +z, -z
        for_each_overlap(
            chunks,
            other_players,
            self.at,
            |bmin, bmax, bark| {
                trapped = true;
                // **Never up out of bark.** A player a hair inside a palm's
                // lean is beside a trunk, not buried under it, and standing
                // them on the slice they touch is the quarter-cell ratchet
                // `for_each_solid_tagged` describes, by the rescue door.
                // Sideways is always the way out of a trunk.
                up = up.max(if bark { f32::INFINITY } else { bmax[1] - min[1] });
                push[0] = push[0].max(bmax[0] - min[0]);
                push[1] = push[1].max(max[0] - bmin[0]);
                push[2] = push[2].max(bmax[2] - min[2]);
                push[3] = push[3].max(max[2] - bmin[2]);
            },
        );
        if !trapped {
            return;
        }

        let clear = |player: &Self, offset: Vec3| {
            !overlaps_anything(chunks, other_players, player.at + offset)
        };
        let step = CONTACT_SKIN * 2.0;
        // Sideways, shallowest first, so the player leaves by the face
        // they are nearest to rather than crossing the block.
        let mut sides = [
            (push[0], Vec3::X),
            (push[1], -Vec3::X),
            (push[2], Vec3::Z),
            (push[3], -Vec3::Z),
        ];
        sides.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Up first -- but only while it is *a way out*, rather than
        // merely *some* way out.
        //
        // Standing on the block that arrived is what a person would do
        // and it is the only escape that never drops anybody through a
        // floor, so it is worth going out of the way for: up to a
        // step's worth further than simply stepping aside would cost,
        // which is exactly the amount of climbing this game already
        // hands out for free. Sand landing at your feet is a metre up
        // against half a metre sideways, and up still wins.
        //
        // Past that it stops being a rescue. A player who has drifted a
        // fraction of a millimetre into the side of a block is not
        // buried in it, and putting them on its roof to resolve that is
        // the "выскакивает на них" half of the complaint -- a metre of
        // free climb, out of nowhere, for walking into a wall.
        let nearest_side = sides
            .iter()
            .map(|&(distance, _)| distance)
            .filter(|distance| *distance > 0.0)
            .fold(f32::INFINITY, f32::min);
        if up > 0.0 && up <= MAX_ESCAPE && up <= nearest_side + PLAYER_STEP_HEIGHT {
            let offset = Vec3::new(0.0, up + step, 0.0);
            if clear(self, offset) {
                self.at += offset;
                self.velocity.y = self.velocity.y.max(0.0);
                self.grounded = true;
                return;
            }
        }
        for (distance, direction) in sides {
            if distance <= 0.0 || distance > MAX_ESCAPE {
                continue;
            }
            let offset = direction * (distance + step);
            if clear(self, offset) {
                self.at += offset;
                return;
            }
        }
        // Entombed. Leave them where they are: a player who cannot get
        // out by moving can get out by digging, and shoving them
        // through a wall to somewhere "free" is how someone ends up
        // inside a mountain.
    }

    /// Lifts the player out of anything shallow they are standing in.
    ///
    /// One case, and it is the commonest placement in the game: looking
    /// down and laying a layer of material on the ground you are
    /// standing on. The cell your feet are in is the cell it goes in, so
    /// for one frame the player is inside it. The placement check
    /// deliberately allows that (see `block_overlaps_player`) on the
    /// promise that physics resolves it, and this is where the promise
    /// is kept.
    fn settle_onto_step(&mut self, chunks: &impl Solids, other_players: &[Vec3]) {
        let (min, max) = player_box(self.at);
        let mut top = self.at.y;
        // Never onto bark: the ratchet `try_step` had on a palm (see
        // `Contact::top`) is the same ratchet here, a quarter slice at a time,
        // for a player who ends a frame a hair inside the lean.
        //
        // **Only a box the collider is actually inside.** `for_each_solid_tagged`
        // hands over every box in the cells the body spans, touched or not
        // (see `for_each_overlap` for why it is coarse), and this used to lift
        // onto any of them. While every solid filled its cell that was the
        // same question; a shaped block's boxes are not. A step's riser shares
        // its tread's cell, so the frame that stepped a player onto the tread
        // stood them on the riser five sixteenths behind them -- a whole block
        // in one frame, "у них нету ступени и меня сразу поднимает". A drying
        // rack's upper frame is two cells up, and a jump whose apex came
        // within half a block of it put the jumper on top of a thing two
        // blocks tall ("могу взбираться на любые не полные блоки, в том числе
        // на сушилку, за 1 прыжок") -- every model-shaped block the same way.
        // Rejected: settling only while not rising. That closes the jump and
        // leaves the step, which is walked.
        for_each_solid_tagged(chunks, min, max, |bmin, bmax, bark| {
            let inside = min.x < bmax[0] - CONTACT_SKIN
                && max.x > bmin[0] + CONTACT_SKIN
                && min.z < bmax[2] - CONTACT_SKIN
                && max.z > bmin[2] + CONTACT_SKIN
                && min.y < bmax[1] - CONTACT_SKIN
                && max.y > bmin[1] + CONTACT_SKIN;
            if inside && !bark && bmax[1] > top && bmax[1] - self.at.y <= PLAYER_STEP_HEIGHT {
                top = bmax[1];
            }
        });
        let lift = top - self.at.y;
        if lift <= 0.0 {
            return;
        }
        let raised = self.at + Vec3::new(0.0, lift + CONTACT_SKIN, 0.0);
        if overlaps_anything(chunks, other_players, raised) {
            return; // no room above: leave them where they are
        }
        self.at = raised;
        self.grounded = true;
        self.velocity.y = self.velocity.y.max(0.0);
        self.step_lag = (self.step_lag + lift).min(STEP_LAG_MAX);
    }
}

/// Which way a swimmer actually moves, given what they pressed and
/// where they are looking.
///
/// **Swimming follows the view; walking does not.** On the ground the
/// move direction is deliberately flattened -- looking at your feet must
/// not make you walk into the floor -- and `wish_dir` arrives already
/// flattened for exactly that reason. In water that flattening is the
/// difference between a game you can dive in and one you cannot: there
/// is no crouch key here, so *pressing forward while looking down* is
/// the only way down there is, and without it a swimmer can rise (jump)
/// and stay level and nothing else.
///
/// So the horizontal part is what the keys asked for, and the vertical
/// part is how much of that ask was "forward" times how steeply the
/// player is looking. Strafing stays level however you look, which is
/// what a player expects from a key that means "sideways"; pressing
/// back while looking down swims *up*, which is what backing away from
/// something on the sea floor should do.
fn stroke_direction(wish_dir: Vec3, look: Vec3) -> Vec3 {
    let flat = Vec3::new(look.x, 0.0, look.z);
    let Some(flat) = flat.try_normalize() else {
        // Looking exactly along the vertical: there is no "forward" to
        // measure the input against. Cannot happen with the camera's
        // pitch limit, and answering "no dive" beats answering with a
        // divide by zero.
        return wish_dir;
    };
    // Rise per unit of horizontal travel along the view. Clamped
    // because it goes to infinity as the view approaches vertical, and
    // an infinity here would come back as a NaN position.
    let slope = (look.y / Vec3::new(look.x, 0.0, look.z).length().max(1e-3)).clamp(-64.0, 64.0);
    let forward = wish_dir.dot(flat);
    Vec3::new(wish_dir.x, forward * slope, wish_dir.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO)
}

/// Axis indices, matching `Vec3`'s own component order so a coordinate
/// can be addressed by number instead of by three near-identical
/// branches.
const X: usize = 0;
const Y: usize = 1;
const Z: usize = 2;

/// What a build/break ray found: the cell it stopped in, and the cell it
/// came from -- which is where a new block would go.
pub type BlockHit = ((i32, i32, i32), (i32, i32, i32));

/// A voxel raycast for breaking and placing blocks: returns the block
/// hit and the cell in front of it, which is where a new block would go.
///
/// **This used to sample the ray every five centimetres.** Two things
/// were wrong with that, and only one of them was speed. It cost 120
/// chunk lookups per cast at a six-block reach -- three casts a frame,
/// for mining, placing and checking whether a punch would land through a
/// wall -- against the ten or so cells a six-block ray actually crosses.
/// And it was *approximate*: the cell in front of the hit was whichever
/// cell the previous sample happened to be in, so a ray crossing two
/// boundaries within one step reported a placement cell diagonally
/// adjacent to the face the player clicked. Rare, unreproducible, and
/// exactly the sort of thing that reads as the game misbehaving.
///
/// This walks cell by cell (the standard grid traversal: keep the
/// distance to the next boundary on each axis, always advance the
/// nearest), so every cell the ray enters is visited exactly once, in
/// order, and the cell it came from is known rather than guessed.
///
/// Loose material is why the hit test is not simply "is this cell
/// targetable". A layer fills part of its cell, so a ray passing over
/// the top of a drift of snow must carry on to whatever is behind it
/// instead of stopping at a block of air the snow is not in.
pub fn raycast_block(
    chunks: &ChunkManager,
    origin: DVec3,
    dir: Vec3,
    max_distance: f32,
) -> Option<BlockHit> {
    raycast_block_for(chunks, origin, dir, max_distance, false)
}

/// The same ray, told whether water counts as something to stop at.
///
/// **Breaking and placing say no; the right click says yes.** Water is
/// not a target -- a ray that stopped at the surface of a lake could
/// never reach the sand under it -- and that rule made drinking from a
/// river impossible: the click found no cell, so the client sent
/// nothing and every rule the server had about mouthfuls, ponds and
/// salt was unreachable. See `geometry::block_box_for_aim`.
///
/// **Cast from beside the eye, not from the planet's origin**, for the
/// reason `Local` gives for the collider: an eye a million blocks out is a
/// number with sixteenths between its neighbours, and a ray that starts a
/// sixteenth off picks the block beside the one under the crosshair. The
/// walk is done in the frame of the eye's own block and the cells it finds
/// are handed back in the world.
pub fn raycast_block_for(
    chunks: &ChunkManager,
    origin: DVec3,
    dir: Vec3,
    max_distance: f32,
    include_liquid: bool,
) -> Option<BlockHit> {
    let frame = Local::around(chunks, origin);
    let ((hx, hy, hz), (px, py, pz)) = raycast_in(&frame, frame.local(origin), dir, max_distance, include_liquid)?;
    Some(((hx + frame.x, hy, hz + frame.z), (px + frame.x, py, pz + frame.z)))
}

fn raycast_in(
    chunks: &impl Solids,
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
    include_liquid: bool,
) -> Option<BlockHit> {
    let mut cell = [
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    ];
    let origin = [origin.x, origin.y, origin.z];
    let dir = [dir.x, dir.y, dir.z];

    // Per axis: which way we are stepping, how far along the ray one
    // whole cell of that axis is, and how far to the first boundary.
    let mut step = [0i32; 3];
    let mut delta = [f32::INFINITY; 3];
    let mut next = [f32::INFINITY; 3];
    for axis in 0..3 {
        if dir[axis] > 0.0 {
            step[axis] = 1;
            delta[axis] = 1.0 / dir[axis];
            next[axis] = (cell[axis] as f32 + 1.0 - origin[axis]) / dir[axis];
        } else if dir[axis] < 0.0 {
            step[axis] = -1;
            delta[axis] = -1.0 / dir[axis];
            next[axis] = (cell[axis] as f32 - origin[axis]) / dir[axis];
        }
    }

    let mut travelled = 0.0f32;
    let mut previous = cell;
    loop {
        if let Some(face) =
            ray_enters_block(chunks, origin, dir, cell, travelled, max_distance, include_liquid)
        {
            // Where a new block would go: across the face that was
            // actually hit, which for anything that fills its cell
            // *is* the cell the ray came from and for anything else
            // is not. Looking along the ground at a drift of snow
            // enters it through the top, and the cell the ray came
            // from is the drift next door -- which is where a block
            // placed on top of the drift used to end up, and why a
            // log built against it lay down instead of standing.
            let mut place = previous;
            if let Some(axis) = face {
                if step[axis] != 0 {
                    place = cell;
                    place[axis] -= step[axis];
                }
            }
            return Some((
                (cell[0], cell[1], cell[2]),
                (place[0], place[1], place[2]),
            ));
        }

        // Advance into the next cell across the nearest boundary.
        let axis = if next[0] < next[1] && next[0] < next[2] {
            0
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        if next[axis] > max_distance || !next[axis].is_finite() {
            return None;
        }
        previous = cell;
        travelled = next[axis];
        cell[axis] += step[axis];
        next[axis] += delta[axis];
    }
}

/// Does the ray actually pass through the *block* in this cell, rather
/// than merely through the cell?
///
/// It used to be enough to ask whether the cell held something
/// targetable, because everything targetable filled its cell. Now a
/// layer of snow is a slab on the floor, a tuft of grass is a pair of
/// planes inset from the walls, and a stone lying on the ground is
/// barely there at all -- so the ray is tested against the block's own
/// box (see `geometry::block_target_box`), and a ray that crosses the
/// empty part of the cell carries on to whatever is behind it.
///
/// `entered` is how far along the ray this cell begins, which is what
/// stops the test from finding a box the ray was already past: the
/// traversal visits cells in order, and a hit before the cell started is
/// a hit in some earlier cell that has already been ruled out.
///
/// Returns *which face* the ray came in through when it hits, since that
/// -- and not the cell the ray came from -- is what decides where a
/// block placed against it goes. `Some(None)` is a ray that began inside
/// the block, which has no face to name.
fn ray_enters_block(
    chunks: &impl Solids,
    origin: [f32; 3],
    dir: [f32; 3],
    cell: [i32; 3],
    entered: f32,
    max_distance: f32,
    include_liquid: bool,
) -> Option<Option<usize>> {
    if entered > max_distance {
        return None;
    }
    let block = chunks.block_at(cell[0], cell[1], cell[2])?;
    // **With the world round it**, because a leaning palm is aimed at where
    // it leans (`geometry::block_box_for_aim_near`): its piece, seen alone,
    // is an upright post the bark has moved away from. Everything else is
    // the box it always was.
    let near = |dx: i32, dy: i32, dz: i32| {
        chunks.block_at(cell[0] + dx, cell[1] + dy, cell[2] + dz).unwrap_or(primitive_shared::types::BLOCK_AIR)
    };
    let (min, max) = primitive_shared::geometry::block_box_for_aim_near(
        block,
        cell[0],
        cell[1],
        cell[2],
        include_liquid,
        near,
    )?;
    match primitive_shared::geometry::ray_box_entry(origin, dir, min, max, max_distance) {
        Some((distance, face)) if distance <= max_distance => Some(face),
        _ => None,
    }
}

/// The player's collider as a world-space box.
fn player_box(feet: Vec3) -> (Vec3, Vec3) {
    (
        feet - Vec3::new(PLAYER_HALF_WIDTH, 0.0, PLAYER_HALF_WIDTH),
        feet + Vec3::new(PLAYER_HALF_WIDTH, PLAYER_HEIGHT, PLAYER_HALF_WIDTH),
    )
}

/// Hands every solid block box overlapping the world-space region to
/// `visit`.
///
/// Column by column rather than cell by cell. The block store is a hash
/// map keyed by chunk, so asking it for a cell costs a hash; a player's
/// collider spans four columns and three cells of height, which is
/// twelve hashes for four chunks' worth of answer. Physics runs this
/// three times a frame (once per axis) plus once more to settle, so the
/// difference is not academic.
///
/// **Every collision the player has comes through here**, which is why
/// the box itself is not computed here. `sweep_axis`, `escape_solids`,
/// `settle_onto_step`, the ground probe and the penetration assert all
/// call this and nothing else, so one shape written in one place is the
/// whole of what the client believes it can walk into -- and the server
/// believes the same thing because `geometry::block_box` is what
/// `block_overlaps_player` reads when it refuses a placement.
///
/// **And the ring of columns round the region, for a palm.** A leaning palm
/// is walked into at its bark (`geometry::for_each_block_box`), and its bark
/// stands out of the piece it grew in by up to `geometry::BOX_OVERHANG` --
/// so a player whose collider was wholly in the cell beside a trunk walked
/// through the lean, because that cell was all this looked at. The ring is
/// asked for palms and nothing else, and a palm's slice is handed over only
/// where it reaches into the region: `settle_onto_step` lifts onto any box
/// top within a step and asks no more, and a slice a quarter of a cell tall
/// standing beside the player is not something to be lifted onto.
fn for_each_solid(
    chunks: &impl Solids,
    min: Vec3,
    max: Vec3,
    mut visit: impl FnMut([f32; 3], [f32; 3]),
) {
    for_each_solid_tagged(chunks, min, max, |bmin, bmax, _bark| visit(bmin, bmax));
}

/// `for_each_solid`, saying of each box whether it is **bark** -- a slice of
/// a palm's trunk -- which is walked into like anything else and is never
/// stepped or lifted onto.
///
/// "когда пытался залезть на пальму от 1 шага влез почти на верхушку с
/// огромной скоростью". A palm's trunk is collided as quarter-cell slices
/// that move sideways with its lean (`palm::trunk_slices`), so its side is a
/// staircase of treads a sixteenth wide and a quarter high -- and a quarter is
/// under `PLAYER_STEP_HEIGHT`. Walking into it zeroed the speed along the
/// lean; the next frame's travel, re-accelerating from nothing, was shorter
/// than the offset to the next slice, so `try_step`'s "the move has to finish
/// up there" held and the player went up a quarter. Every frame or two: four
/// and three-quarter blocks in twenty-two frames, measured, with the client
/// honestly reporting ground under its feet the whole way -- which is why
/// the server's ascent check never said a word.
///
/// Three ways out were weighed:
///
/// * *One box per cell for stepping, the slices for walking into.* A player
///   would then step onto the air beside a lean, the invisible ledge the
///   slices were brought in to remove.
/// * *A minimum tread for any step* -- nothing narrower than some fraction
///   of the collider can be stepped onto. It is a new number every solid in
///   the game would be measured against, for one trunk, and the half-block
///   slab a player lays against a wall is exactly as narrow at its edge.
/// * **Bark is not a step (chosen).** Nothing in the game climbs a palm --
///   coconuts are shaken down (`GUIDE.md`) -- so the trunk is what a wall is
///   to the step, the settle and the escape upward: something to walk into
///   and round. The tag rides out of the one function that already knows a
///   box is a palm's.
///
/// **The wood of every other tree is not bark**, though it is walked into at
/// its shape the same way ("добавь коллизию веткам", `branch`). The ratchet
/// needs treads that climb: a palm's quarter slices stand a quarter apart all
/// the way up. A branch's horizontal wood is centred on the middle of its cell,
/// so its tops lie between nine and sixteen sixteenths of that cell -- two
/// pieces at one height are at most seven sixteenths apart, and the next
/// height is a whole cell further, which no step reaches. What tagging it would
/// cost is real: a limb tapers a sixteenth a piece toward its tip, and a player
/// walking along one toward the trunk would be stopped by every taper.
/// `walking_into_a_tree_from_any_side_never_lifts_a_player_more_than_a_step`
/// holds the argument to the generator's own trees.
fn for_each_solid_tagged(
    chunks: &impl Solids,
    min: Vec3,
    max: Vec3,
    mut visit: impl FnMut([f32; 3], [f32; 3], bool),
) {
    use primitive_shared::geometry::BOX_OVERHANG;
    use primitive_shared::types::{block_kind, BLOCK_AIR, BLOCK_PALM_TRUNK};
    let (x0, x1) = (min.x.floor() as i32, (max.x - CONTACT_SKIN).floor() as i32);
    let (y0, y1) = (min.y.floor() as i32, (max.y - CONTACT_SKIN).floor() as i32);
    let (z0, z1) = (min.z.floor() as i32, (max.z - CONTACT_SKIN).floor() as i32);
    let (ring_x0, ring_x1) = ((min.x - BOX_OVERHANG).floor() as i32, (max.x - CONTACT_SKIN + BOX_OVERHANG).floor() as i32);
    let (ring_z0, ring_z1) = ((min.z - BOX_OVERHANG).floor() as i32, (max.z - CONTACT_SKIN + BOX_OVERHANG).floor() as i32);
    let reaches_in = |bmin: [f32; 3], bmax: [f32; 3]| {
        bmin[0] < max.x - CONTACT_SKIN && bmax[0] > min.x && bmin[2] < max.z - CONTACT_SKIN && bmax[2] > min.z
    };

    for bz in ring_z0..=ring_z1 {
        for bx in ring_x0..=ring_x1 {
            let ring = !(x0..=x1).contains(&bx) || !(z0..=z1).contains(&bz);
            let Some(column) = chunks.column(bx, bz) else {
                // A chunk that has not arrived is nothing to collide
                // with, and it has to stay that way.
                //
                // **This was briefly a wall**, on the argument that
                // falling out of the world at the edge of the loaded
                // area is worse than being stopped by something
                // invisible. It made things very much worse. A player
                // walking normally straddles a chunk seam several times
                // a minute, and the column on the far side of it is
                // *routinely* a frame or two behind -- so the collider
                // was inside a wall constantly, and `escape_solids`,
                // whose whole job is to push a player out of a wall,
                // teleported them a metre sideways every time it
                // happened. A rule that fires on the exception has to
                // be right about the exception; this one turned the
                // ordinary case into the exception.
                //
                // Falling through an unarrived chunk is what the
                // loading gate is for: physics does not run until the
                // 3x3 around the player is in.
                continue;
            };
            for by in y0..=y1 {
                let block = column.block(by);
                if block_kind(block) == BLOCK_PALM_TRUNK {
                    let near = |dx: i32, dy: i32, dz: i32| chunks.block_at(bx + dx, by + dy, bz + dz).unwrap_or(BLOCK_AIR);
                    primitive_shared::geometry::for_each_block_box(block, bx, by, bz, near, |bmin, bmax| {
                        if reaches_in(bmin, bmax) {
                            visit(bmin, bmax, true);
                        }
                    });
                    continue;
                }
                if ring {
                    continue;
                }
                // **A piece of branch is its wood**, a post and an arm to each
                // piece beside it, read off the cells round it
                // (`geometry::for_each_block_box`). Inside its own cell, so not
                // in the ring -- and not bark: see the note on this function.
                // ...and a step is its tread and its riser, for the same
                // reason: its `block_box` is the whole cell, a metre wall the
                // step-up would never ride (`geometry::step_boxes`).
                if primitive_shared::types::is_branch(block) || primitive_shared::types::is_step(block) {
                    let near = |dx: i32, dy: i32, dz: i32| chunks.block_at(bx + dx, by + dy, bz + dz).unwrap_or(BLOCK_AIR);
                    primitive_shared::geometry::for_each_block_box(block, bx, by, bz, near, |bmin, bmax| {
                        visit(bmin, bmax, false);
                    });
                    continue;
                }
                // **The block's box, not its cell.** This was built
                // here out of `collision_height` and two literal ones,
                // which said that everything solid is as wide as its
                // cell -- and one block is not. A drying rack is a
                // frame of poles two and a half sixteenths deep
                // standing in the middle of its cell, and a tanner
                // could not walk past their own rack: they could see
                // straight through it and it stopped them like stone.
                // Reported as "the rack has the collision of a full
                // block when it is smaller".
                //
                // Asking `geometry::block_box` instead is also what
                // keeps the height right -- a campfire is a quarter of
                // a cell and a backpack half, and reading either as a
                // boolean would stand a player in mid-air over their
                // own pack. One function answers both, and it is the
                // one the server answers placements with.
                let Some((min, max)) =
                    primitive_shared::geometry::block_box(block, bx, by, bz)
                else {
                    continue;
                };
                visit(min, max, false);
            }
        }
    }
}

/// Hands every box the collider at `feet` is actually *inside* to
/// `visit` -- blocks and other players alike.
///
/// `for_each_solid` answers a coarser question, and deliberately: it
/// walks the cells the collider spans and hands over every solid box in
/// them, touched or not, because a sweep does its own overlap test per
/// axis and wants the candidates rather than the answers.
///
/// It is the wrong question for "get me out of here": measuring the way
/// out against a box the collider is not touching asks the player to
/// travel a distance that means nothing, and past `MAX_ESCAPE` that
/// side is refused outright -- so the escape leaves by some other face
/// and the player is flung across the room.
///
/// **Now that every solid fills its cell, blocks cannot produce that.**
/// The cell range is derived from the collider's own box, so every box
/// in it overlaps by construction; the filter costs a comparison and
/// finds nothing. Other players are the case that survives. Their boxes
/// are nearly two blocks tall and are placed by the network rather than
/// by this grid, so one can sit squarely inside the cells the collider
/// spans while touching none of it.
///
/// Each box comes with whether it is bark, as `for_each_solid_tagged` says;
/// another player never is.
fn for_each_overlap(
    chunks: &impl Solids,
    other_players: &[Vec3],
    feet: Vec3,
    mut visit: impl FnMut([f32; 3], [f32; 3], bool),
) {
    let (min, max) = player_box(feet);
    // The same "touching is not overlapping" margin the rest of the
    // collider uses: a sweep leaves the two flush on purpose, and
    // calling that an overlap would have every landing push the player
    // somewhere.
    let overlaps = |bmin: [f32; 3], bmax: [f32; 3]| {
        min.x < bmax[0] - CONTACT_SKIN
            && max.x > bmin[0] + CONTACT_SKIN
            && min.y < bmax[1] - CONTACT_SKIN
            && max.y > bmin[1] + CONTACT_SKIN
            && min.z < bmax[2] - CONTACT_SKIN
            && max.z > bmin[2] + CONTACT_SKIN
    };
    for_each_solid_tagged(chunks, min, max, |bmin, bmax, bark| {
        if overlaps(bmin, bmax) {
            visit(bmin, bmax, bark);
        }
    });
    for &other_feet in other_players {
        let (omin, omax) = player_box(other_feet);
        let (bmin, bmax): ([f32; 3], [f32; 3]) = (omin.into(), omax.into());
        if overlaps(bmin, bmax) {
            visit(bmin, bmax, false);
        }
    }
}

/// What a sweep along one axis found.
struct Contact {
    /// How far the move may go, in the same sign as it was asked for.
    allowed: f32,
    /// Whether anything stopped it short.
    blocked: bool,
    /// The highest surface among the things that stopped it -- what a
    /// step up would have to climb. Infinite when bark stopped it, which no
    /// step climbs: see `for_each_solid_tagged`.
    top: f32,
}

/// How far the player may travel along one axis before something stops
/// them.
///
/// Only boxes that overlap on the *other* two axes can be hit, and that
/// overlap is strict: a player standing exactly on a floor is not
/// blocked from walking along it, and one flush against a wall is not
/// blocked from sliding down it.
fn sweep_axis(
    chunks: &impl Solids,
    other_players: &[Vec3],
    feet: Vec3,
    delta: f32,
    axis: usize,
) -> Contact {
    let (min, max) = player_box(feet);
    let mut lo = min;
    let mut hi = max;
    if delta > 0.0 {
        hi[axis] += delta;
    } else {
        lo[axis] += delta;
    }

    let mut contact = Contact {
        allowed: delta,
        blocked: false,
        top: f32::NEG_INFINITY,
    };
    let mut consider = |bmin: [f32; 3], bmax: [f32; 3], bark: bool| {
        let surface = if bark { f32::INFINITY } else { bmax[1] };
        for other in 0..3 {
            if other == axis {
                continue;
            }
            if min[other] >= bmax[other] - CONTACT_SKIN || max[other] <= bmin[other] + CONTACT_SKIN {
                return;
            }
        }
        // **And the same question on the axis being swept: a box the
        // collider has already gone past cannot stop it going further.**
        //
        // The clamp below turns a negative gap -- "this is behind me" --
        // into zero, and a zero gap is exactly what "touching, stop
        // here" looks like. While every solid filled its cell that could
        // never fire, because the box holding a player up is in the cell
        // *below* their feet and `for_each_solid` starts at the cell the
        // feet are in. A campfire is a quarter of a cell and a backpack
        // half of one, so the thing you stand on top of is in the very
        // cell your feet occupy -- and every upward sweep was handed it
        // as an obstacle overhead.
        //
        // **That is why jumping off a campfire did nothing.** The jump
        // fired, the velocity was set, the sweep reported blocked with
        // zero travel, and `move_axis` zeroed the velocity again. Not a
        // short hop: no movement at all, every time, on the three blocks
        // in the game that are shorter than their cell.
        //
        // Only the side being moved away from is rejected. A box the
        // collider is *inside* still blocks, which is what keeps
        // somebody buried in a wall from walking out through it and
        // leaves that case to `escape_solids`, where it belongs.
        let behind = if delta > 0.0 {
            bmax[axis] <= max[axis] + CONTACT_SKIN
        } else {
            bmin[axis] >= min[axis] - CONTACT_SKIN
        };
        if behind {
            return;
        }
        let gap = if delta > 0.0 {
            (bmin[axis] - max[axis] - CONTACT_SKIN).max(0.0)
        } else {
            (bmax[axis] - min[axis] + CONTACT_SKIN).min(0.0)
        };
        // **No slack for a short step.** This used to require the gap
        // to be a contact skin *nearer* than the move before it counted
        // as blocking, which quietly means a move shorter than the skin
        // can never be blocked by anything. A player pressed against a
        // wall and sliding along it asks for exactly such a move on the
        // blocked axis every frame -- and each one takes them a
        // ten-thousandth of a block further in. It is invisible until
        // the total passes the skin, at which point they are *inside*
        // the wall as far as `escape_solids` is concerned, and it
        // launches them onto its roof.
        if gap.abs() < contact.allowed.abs() {
            // Nearer than anything found so far: it alone decides both
            // where the move stops and how high a step would have to be.
            contact.allowed = gap;
            contact.blocked = true;
            contact.top = surface;
        } else if contact.blocked && gap.abs() <= contact.allowed.abs() + CONTACT_SKIN {
            // Level with the nearest: a wall of two blocks is stepped
            // over only if the *taller* of them can be.
            contact.top = contact.top.max(surface);
        }
    };

    for_each_solid_tagged(chunks, lo, hi, &mut consider);
    for &other_feet in other_players {
        let (omin, omax) = player_box(other_feet);
        if omin.x < hi.x && omax.x > lo.x && omin.y < hi.y && omax.y > lo.y && omin.z < hi.z
            && omax.z > lo.z
        {
            consider(omin.into(), omax.into(), false);
        }
    }
    contact
}

/// Would the player standing here be inside anything at all?
fn overlaps_anything(
    chunks: &impl Solids,
    other_players: &[Vec3],
    feet: Vec3,
) -> bool {
    aabb_intersects_solid(chunks, feet)
        || other_players
            .iter()
            .any(|&other_feet| aabb_overlaps_player(feet, other_feet))
}

/// True if the player's collider at `feet_pos` overlaps any solid block.
fn aabb_intersects_solid(chunks: &impl Solids, feet_pos: Vec3) -> bool {
    let (min, max) = player_box(feet_pos);
    let mut hit = false;
    for_each_solid(chunks, min, max, |bmin, bmax| {
        hit = hit
            || (min.x < bmax[0] - CONTACT_SKIN
                && max.x > bmin[0] + CONTACT_SKIN
                && min.y < bmax[1] - CONTACT_SKIN
                && max.y > bmin[1] + CONTACT_SKIN
                && min.z < bmax[2] - CONTACT_SKIN
                && max.z > bmin[2] + CONTACT_SKIN);
    });
    hit
}

/// "Хитбоксы игрокам": AABB-vs-AABB overlap between the local player at
/// `feet_pos` and another player standing at `other_feet` -- both using
/// the same PLAYER_HALF_WIDTH/PLAYER_HEIGHT box, so two players simply
/// can't occupy the same space.
fn aabb_overlaps_player(feet_pos: Vec3, other_feet: Vec3) -> bool {
    let (min_a, max_a) = player_box(feet_pos);
    let (min_b, max_b) = player_box(other_feet);

    min_a.x < max_b.x
        && max_a.x > min_b.x
        && min_a.y < max_b.y
        && max_a.y > min_b.y
        && min_a.z < max_b.z
        && max_a.z > min_b.z
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use primitive_shared::types::{
        Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };

    /// A floor with water filling y = 10..=19 over it.
    pub fn lake_world() -> ChunkManager {
        world_of(|y| {
            if y < 10 {
                BLOCK_STONE
            } else if y < 20 {
                BLOCK_WATER
            } else {
                BLOCK_AIR
            }
        })
    }

    // ------------------------------------------------------- flight

    /// A flying player in mid-air, with nothing pressed.
    fn flyer(at: Vec3) -> Player {
        let mut player = Player::new(at.as_dvec3(), 5.5);
        player.set_flying(true, 12.0);
        player
    }

    /// One second of pressing whatever the caller says.
    fn fly_for(chunks: &ChunkManager, player: &mut Player, climb: f32, dir: Vec3, seconds: f32) {
        let step = 1.0 / 60.0;
        let mut left = seconds;
        while left > 0.0 {
            player.climb = climb;
            player.update(chunks, &[], dir, Vec3::X, false, false, false, step);
            left -= step;
        }
    }

    /// The whole point: gravity does not apply.
    #[test]
    fn a_flying_player_does_not_fall() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut player, 0.0, Vec3::ZERO, 3.0);
        assert!(
            (player.position.y - 40.0).abs() < 0.05,
            "drifted to {} instead of holding 40",
            player.position.y
        );
    }

    #[test]
    fn a_flying_player_rises_and_descends_on_command() {
        let chunks = floor_world();

        let mut up = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut up, 1.0, Vec3::ZERO, 1.0);
        assert!(up.position.y > 45.0, "only reached {}", up.position.y);

        let mut down = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut down, -1.0, Vec3::ZERO, 1.0);
        assert!(down.position.y < 35.0, "only reached {}", down.position.y);
    }

    /// A flyer reaches **the speed they were granted**, not whatever
    /// falls out of an acceleration fighting a drag. That was the first
    /// attempt and `/fly 12` flew at eight -- see `FLY_RESPONSE_SECONDS`.
    #[test]
    fn a_flyer_reaches_the_speed_it_was_given() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut player, 0.0, Vec3::X, 1.0);
        let speed = player.velocity.length();
        assert!(
            (speed - 12.0).abs() < 0.2,
            "asked for 12 blocks a second and got {speed}"
        );
    }

    /// ...and letting go stops you where you are. Without this a player
    /// who reached top speed would keep it forever and every arrival
    /// would be a collision.
    #[test]
    fn letting_go_of_everything_stops_a_flyer() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut player, 0.0, Vec3::X, 1.0);
        assert!(player.velocity.length() > 5.0, "never got going");
        fly_for(&chunks, &mut player, 0.0, Vec3::ZERO, 0.5);
        assert!(
            player.velocity.length() < 0.5,
            "still moving at {}",
            player.velocity.length()
        );
    }

    /// Flight is not standing on something, even at ground level. If it
    /// were, a hovering player would get footsteps and a jump.
    #[test]
    fn a_flyer_at_ground_level_is_never_grounded() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 10.0, 8.0));
        fly_for(&chunks, &mut player, -1.0, Vec3::X, 1.0);
        assert!(!player.grounded, "a flyer was called grounded");
    }

    /// Flight is not noclip. The two are separate powers on purpose --
    /// see the note in `update`.
    #[test]
    fn a_flyer_cannot_fly_through_rock() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 12.0, 8.0));
        fly_for(&chunks, &mut player, -1.0, Vec3::ZERO, 2.0);
        assert!(
            player.position.y >= 9.9,
            "sank to {} -- through the floor",
            player.position.y
        );
    }

    /// Switching it off drops you. Not to the ground -- from wherever
    /// you were, at whatever speed gravity gives you, which is the
    /// honest behaviour and the one that cannot be used to cross a gap
    /// for free.
    #[test]
    fn leaving_flight_starts_a_fall() {
        let chunks = floor_world();
        let mut player = flyer(Vec3::new(8.0, 40.0, 8.0));
        fly_for(&chunks, &mut player, 0.0, Vec3::ZERO, 0.5);
        player.set_flying(false, 0.0);
        fly_for(&chunks, &mut player, 0.0, Vec3::ZERO, 1.0);
        assert!(
            player.position.y < 35.0,
            "still at {} a second after flight ended",
            player.position.y
        );
    }

    /// Water does not get a say. A flying player in a lake is a flying
    /// player; letting buoyancy have the vertical axis as well would be
    /// two systems fighting over it.
    #[test]
    fn flight_beats_buoyancy() {
        let chunks = lake_world();
        let mut player = flyer(Vec3::new(8.0, 14.0, 8.0));
        fly_for(&chunks, &mut player, 0.0, Vec3::ZERO, 2.0);
        assert!(
            (player.position.y - 14.0).abs() < 0.2,
            "floated to {} instead of holding 14",
            player.position.y
        );
    }

    /// A speed that makes no sense leaves the old one alone rather than
    /// producing a player who cannot move or one who crosses the world.
    #[test]
    fn a_nonsense_speed_is_ignored() {
        let mut player = Player::new((Vec3::ZERO).as_dvec3(), 5.5);
        player.set_flying(true, 20.0);
        assert_eq!(player.fly_speed, 20.0);
        player.set_flying(true, f32::NAN);
        assert_eq!(player.fly_speed, 20.0);
        player.set_flying(true, -3.0);
        assert_eq!(player.fly_speed, 20.0);
        player.set_flying(true, 10_000.0);
        assert_eq!(player.fly_speed, 80.0, "an absurd speed should clamp");
    }

    /// A stone floor at y = 0..=9.
    pub fn floor_world() -> ChunkManager {
        world_of(|y| if y < 10 { BLOCK_STONE } else { BLOCK_AIR })
    }

    /// Nine chunks of the same column, so a test can walk.
    ///
    /// **Not one chunk.** An unloaded chunk is a wall to the collider
    /// now (see `for_each_solid`), so a one-chunk fixture is a room
    /// eight metres from the middle in every direction -- and a test
    /// that runs a player in a straight line for a second is testing
    /// that wall rather than whatever it meant to test.
    pub fn world_of(column: impl Fn(usize) -> primitive_shared::types::BlockId) -> ChunkManager {
        let mut cm = ChunkManager::new(4);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..primitive_shared::types::CHUNK_SIZE_Y {
            let id = column(y);
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = id;
                }
            }
        }
        for cx in -1..=1 {
            for cz in -1..=1 {
                cm.insert(Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: blocks.clone(),
                });
            }
        }
        cm
    }

    /// The stone floor of `floor_world`, with `block` put at (x, 10, z).
    pub fn floor_with(x: i32, z: i32, block: primitive_shared::types::BlockId) -> ChunkManager {
        let mut chunks = floor_world();
        let mut chunk = chunks
            .get(primitive_shared::types::ChunkPos::new(0, 0))
            .unwrap()
            .clone();
        chunk.set(x as usize, 10, z as usize, block);
        chunks.insert(chunk);
        chunks
    }

    /// Where each piece of a palm's trunk went, and what it is.
    type Trunk = Vec<((i32, i32, i32), primitive_shared::types::BlockId)>;

    /// The generator's palm `variant` leaning `lean`, rooted on the stone
    /// of `floor_world` at (4, 7): its trunk and nothing else, and where
    /// each piece went.
    fn palm_world(variant: u32, lean: (i32, i32)) -> (ChunkManager, Trunk) {
        use primitive_shared::types::{block_kind, BLOCK_PALM_TRUNK};
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        let trunk: Vec<_> = primitive_shared::worldgen::palm_cells(variant, lean)
            .into_iter()
            .filter(|(_, id)| block_kind(*id) == BLOCK_PALM_TRUNK)
            .map(|((dx, dy, dz), id)| ((4 + dx, 9 + dy, 7 + dz), id))
            .collect();
        for &((x, y, z), id) in &trunk {
            chunk.set(x as usize, y as usize, z as usize, id);
        }
        chunks.insert(chunk);
        (chunks, trunk)
    }

    #[test]
    fn a_player_is_stopped_by_the_bark_of_a_leaning_palm_and_not_by_the_cells_it_grew_in() {
        // **"у пальмы поломаны коллизия она не соответствует модели".** Two
        // halves, both asked through `for_each_solid`, which every collision
        // the player has comes through. A collider standing wholly in the air
        // cell a trunk leans into meets the lean -- which it cannot, if the
        // cells round the region are not asked -- and a collider in the half
        // of a step's lower piece the bark has left meets nothing, where the
        // whole cell used to stop it.
        let (chunks, trunk) = palm_world(0, (1, 0));
        let palm_at = |x: i32, y: i32, z: i32| trunk.iter().any(|(c, _)| *c == (x, y, z));
        // A reference, so each neighbour closure borrows the world rather
        // than moving it out from under `for_each_solid` below.
        let world = &chunks;
        let near_of = |x: i32, y: i32, z: i32| {
            move |dx: i32, dy: i32, dz: i32| world.block_at(x + dx, y + dy, z + dz).unwrap_or(BLOCK_AIR)
        };
        // A slice leaning out through the +x face into a cell with no palm.
        let mut lean = None;
        for &((x, y, z), id) in &trunk {
            primitive_shared::geometry::for_each_block_box(id, x, y, z, near_of(x, y, z), |min, max| {
                if max[0] > (x + 1) as f32 + 0.05 && !palm_at(x + 1, y, z) {
                    lean = Some((min, max));
                }
            });
        }
        let (lo, hi) = lean.expect("the palm never leans out of a piece into air");
        let cell = (hi[0].floor(), lo[1].floor(), lo[2].floor());
        let probe = (Vec3::new(cell.0 + 0.001, lo[1] + 0.01, cell.2 + 0.2), Vec3::new(cell.0 + 0.3, hi[1] - 0.01, cell.2 + 0.8));
        let overlaps = |bmin: [f32; 3], bmax: [f32; 3], (min, max): (Vec3, Vec3)| {
            (0..3).all(|a| bmin[a] < max[a] - CONTACT_SKIN && bmax[a] > min[a] + CONTACT_SKIN)
        };
        let mut met = false;
        for_each_solid(&chunks, probe.0, probe.1, |bmin, bmax| met |= overlaps(bmin, bmax, probe));
        assert!(met, "a collider in the cell beside the trunk walks through the lean at {lo:?}..{hi:?}");

        // The first step's lower piece, (4, 12, 7): its upper half on the
        // side away from the lean is air.
        assert!(palm_at(4, 12, 7) && palm_at(5, 12, 7), "the palm's first step moved");
        let empty = (Vec3::new(4.05, 12.55, 7.2), Vec3::new(4.5, 12.95, 7.8));
        let mut met = false;
        for_each_solid(&chunks, empty.0, empty.1, |bmin, bmax| met |= overlaps(bmin, bmax, empty));
        assert!(!met, "the half of a step the bark has left still stops a collider");
    }

    #[test]
    fn walking_into_a_palm_trunk_from_any_side_never_lifts_a_player_more_than_a_step() {
        // "когда пытался залезть на пальму от 1 шага влез почти на верхушку
        // с огромной скоростью". Measured before the fix on variant 18 leaning
        // +x, walked into from the -x side: `try_step` fired on the quarter
        // slices nineteen times in twenty-two frames and stood the player 4.75
        // blocks up the bark. Palms of every height and both step patterns,
        // four leans, twenty-four headings each, walking and sprinting at the
        // root for four seconds: the feet never leave the sand by more than a
        // step -- and a step is only ever the floor here, so in practice not
        // at all.
        let mut worst = (0.0f32, String::new());
        for variant in [0u32, 1, 2, 3, 17, 18, 33, 50] {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let (chunks, _) = palm_world(variant, lean);
                for side in 0..24 {
                    let angle = side as f32 / 24.0 * std::f32::consts::TAU;
                    let root = Vec3::new(4.5, 10.0, 7.5);
                    let start = root + Vec3::new(angle.cos(), 0.0, angle.sin()) * 2.5;
                    let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
                    for _ in 0..30 {
                        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                    }
                    for frame in 0..240 {
                        let toward = (root - player.position.as_vec3()).with_y(0.0).normalize_or_zero();
                        // Sprinting on alternate frames, so both speeds meet the bark.
                        player.update(&chunks, &[], toward, Vec3::X, false, false, frame % 2 == 0, 1.0 / 60.0);
                        let rose = player.position.y - 10.0;
                        if rose > f64::from(worst.0) {
                            worst = (rose as f32, format!("variant {variant} lean {lean:?} side {side} frame {frame} at {:?}", player.position));
                        }
                    }
                }
            }
        }
        assert!(worst.0 <= PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0, "rose {} : {}", worst.0, worst.1);
    }

    /// A stone floor with a column of one piece of branch standing on it at
    /// (x, 10.., z), `height` pieces tall.
    fn floor_with_a_column(x: usize, z: usize, height: usize, piece: primitive_shared::types::BlockId) -> ChunkManager {
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for y in 10..10 + height {
            chunk.set(x, y, z, piece);
        }
        chunks.insert(chunk);
        chunks
    }

    /// Where a player walking along +x at `z` from x = 5.5 has got to after
    /// two seconds.
    fn walked_along_x(chunks: &ChunkManager, z: f32) -> f32 {
        let mut player = Player::new((Vec3::new(5.5, 10.0, z)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        player.position.x as f32
    }

    #[test]
    fn a_player_walks_into_the_wood_of_a_tree_and_past_the_air_beside_it() {
        // **"добавь коллизию веткам".** A trunk eight sixteenths wide was a
        // whole cell of wall, and a sapling's stem was nothing at all. Each
        // walked at down its middle, and past the edge of its bark.
        use primitive_shared::types::branch;
        let bough = floor_with_a_column(8, 8, 3, branch(8));
        let stopped = walked_along_x(&bough, 8.5);
        let bark = 8.0 + 4.0 / 16.0 - PLAYER_HALF_WIDTH;
        assert!((stopped - bark).abs() < 0.01, "a trunk eight sixteenths wide stopped a player at x = {stopped}, and its bark is at {bark}");
        let beside = 8.0 + 12.0 / 16.0 + PLAYER_HALF_WIDTH + 0.02;
        assert!(walked_along_x(&bough, beside) > 10.0, "a player walking past a trunk was stopped by the air in its cell");

        let sapling = floor_with_a_column(8, 8, 2, branch(2));
        let stopped = walked_along_x(&sapling, 8.5);
        let bark = 8.0 + 7.0 / 16.0 - PLAYER_HALF_WIDTH;
        assert!((stopped - bark).abs() < 0.01, "a sapling's stem stopped a player at x = {stopped}, and its bark is at {bark}");
        let beside = 8.0 + 9.0 / 16.0 + PLAYER_HALF_WIDTH + 0.02;
        assert!(walked_along_x(&sapling, beside) > 10.0, "a player walking past a sapling was stopped by the air round its stem");
    }

    #[test]
    fn a_player_walks_up_a_flight_of_steps_without_jumping() {
        // **"сделай ступеньки".** Three steps rising along +x, each put down
        // by someone walking up them (looking along +x is `Facing::West`,
        // the low side toward -x), with stone under the upper two. Every
        // rise is half a cell, which the step-up rides; asked through the
        // whole cell `block_box` gives, the first riser was a metre wall.
        use primitive_shared::types::{faced, Facing, BLOCK_TILE_ROOF};
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        let step = faced(BLOCK_TILE_ROOF, Facing::West);
        for (x, top) in [(8usize, 10usize), (9, 11), (10, 12)] {
            for y in 10..top {
                chunk.set(x, y, 8, BLOCK_STONE);
            }
            chunk.set(x, top, 8, step);
        }
        chunks.insert(chunk);
        let mut player = Player::new((Vec3::new(5.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        // The highest they got, since past the last step is a drop.
        let mut highest = player.position.y;
        for _ in 0..240 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
            highest = highest.max(player.position.y);
        }
        assert!(highest > 12.9, "a player walking at a flight of steps got no higher than {highest}, and is at {:?}", player.position);
    }

    /// A stone floor with one step of `kind` facing `facing` on it at
    /// (8, 10, 8), and nothing under it or behind it.
    fn floor_with_a_step(kind: primitive_shared::types::BlockId, facing: primitive_shared::types::Facing) -> ChunkManager {
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(8, 10, 8, primitive_shared::types::faced(kind, facing));
        chunks.insert(chunk);
        chunks
    }

    /// Walks a player from `start` along `heading` for `seconds`, and
    /// returns every height their feet stood at after each frame.
    fn heights_walking(chunks: &ChunkManager, start: Vec3, heading: Vec3, seconds: f32) -> Vec<f32> {
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        (0..(seconds * 60.0) as usize)
            .map(|_| {
                player.update(chunks, &[], heading, heading, false, false, false, 1.0 / 60.0);
                player.position.y as f32
            })
            .collect()
    }

    const EVERY_STEP: [primitive_shared::types::BlockId; 5] = [
        primitive_shared::types::BLOCK_PLANK_STAIRS,
        primitive_shared::types::BLOCK_COBBLESTONE_STAIRS,
        primitive_shared::types::BLOCK_TILE_ROOF,
        primitive_shared::types::BLOCK_THATCH_ROOF,
        primitive_shared::types::BLOCK_BRANCH_ROOF,
    ];

    #[test]
    fn walking_at_a_step_from_its_low_side_stands_on_the_tread_before_the_riser() {
        // **"ступеньки сломаны у них нету ступени и меня сразу поднимает как
        // подхожу к ним".** Measured before the fix: the frame that stepped a
        // player onto the tread (feet at 10.5) also stood them on the riser
        // (11.0) -- a whole block in one frame, the tread never stood on.
        // `settle_onto_step` took every box in the cells the collider spans
        // as something it was standing in, and the riser shares the tread's
        // cell; it lifted onto a box five sixteenths behind the body.
        use primitive_shared::types::Facing;
        for kind in EVERY_STEP {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let chunks = floor_with_a_step(kind, facing);
                // From three cells out on the low side, which is the side
                // `Facing::step` points at, straight at the middle.
                let (sx, sz) = facing.step();
                let middle = Vec3::new(8.5, 10.0, 8.5);
                let start = middle + Vec3::new(sx as f32, 0.0, sz as f32) * 3.0;
                let heading = (middle - start).normalize();
                let heights = heights_walking(&chunks, start, heading, 2.0);
                let mut last = 10.0f32;
                for (frame, &y) in heights.iter().enumerate() {
                    assert!(
                        y - last <= PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0,
                        "{kind} {facing:?}: frame {frame} rose {} in one frame, from {last} to {y}",
                        y - last
                    );
                    last = y;
                }
                let on_tread = heights.iter().filter(|&&y| (y - 10.5).abs() < 0.01).count();
                assert!(on_tread >= 3, "{kind} {facing:?}: stood on the tread for {on_tread} frames: {heights:?}");
                assert!(
                    heights.iter().any(|&y| (y - 11.0).abs() < 0.01),
                    "{kind} {facing:?}: never got onto the riser: {heights:?}"
                );
            }
        }
    }

    #[test]
    fn a_player_who_stops_on_the_tread_of_a_step_stays_on_the_tread() {
        // The other half of the same report: the lower half of a step is a
        // place to stand (`geometry::STEP_TREAD`), and with the riser lifting
        // anybody in its cell it was not -- stopping there stood the player
        // on the upper half the next frame.
        use primitive_shared::types::{Facing, BLOCK_PLANK_STAIRS};
        let chunks = floor_with_a_step(BLOCK_PLANK_STAIRS, Facing::North);
        // The low side of a north-facing step is -z: stand the player on the
        // front of the tread, their back edge over its front edge.
        let mut player = Player::new(Vec3::new(8.5, 10.5, 8.0 + PLAYER_HALF_WIDTH + 0.01).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::Z, false, false, false, 1.0 / 60.0);
        }
        assert!((player.position.y - 10.5).abs() < 0.01, "a player standing on the tread ended at {:?}", player.position);
    }

    #[test]
    fn nothing_two_blocks_tall_is_climbed_by_jumping_at_it_from_the_ground() {
        // **"могу взбираться на любые не полные блоки, в том числе на
        // сушилку, за 1 прыжок".** Before the fix `settle_onto_step` stood a
        // jumper on any box in the cells their body spanned, touched or not,
        // once its top was within a step of their feet -- and a jump's apex is
        // within a step of two blocks. A drying rack (two by two), two upright
        // stakes one on the other, and a chest on a chest; run at from sixteen
        // headings, jumping on every other frame for three seconds. The feet
        // never get higher than a jump carries them.
        use primitive_shared::types::{faced, rack_cells, Facing, BLOCK_CHEST, BLOCK_STAKE, STAKE_UPRIGHT};
        let stake = BLOCK_STAKE | STAKE_UPRIGHT;
        type Cells = Vec<((i32, i32, i32), primitive_shared::types::BlockId)>;
        let tall: [(&str, Cells); 3] = [
            ("a drying rack", rack_cells((8, 10, 8), Facing::North).to_vec()),
            ("two stakes", vec![((8, 10, 8), stake), ((8, 11, 8), stake)]),
            ("two chests", vec![((8, 10, 8), faced(BLOCK_CHEST, Facing::North)), ((8, 11, 8), faced(BLOCK_CHEST, Facing::North))]),
        ];
        for (name, cells) in tall {
            let mut chunks = floor_world();
            let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
            for &((x, y, z), id) in &cells {
                chunk.set(x as usize, y as usize, z as usize, id);
            }
            chunks.insert(chunk);
            let middle = Vec3::new(8.5, 10.0, 8.5);
            for side in 0..16 {
                let angle = side as f32 / 16.0 * std::f32::consts::TAU;
                let start = middle + Vec3::new(angle.cos(), 0.0, angle.sin()) * 2.5;
                let heading = (middle - start).normalize();
                let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
                let mut highest = 10.0f64;
                for frame in 0..180 {
                    let jump = frame % 2 == 0;
                    player.update(&chunks, &[], heading, heading, jump, jump, false, 1.0 / 60.0);
                    highest = highest.max(player.position.y);
                }
                assert!(highest < 11.6, "{name}, heading {side}: a jump from the ground got the feet to {highest}, and they ended at {:?}", player.position);
            }
        }
    }

    #[test]
    fn walking_at_a_step_from_any_side_never_climbs_more_than_a_half_block_at_once() {
        // From the flanks the tread is a half-block kerb and the riser a
        // metre wall; from the back the whole cell is a wall. Every heading
        // round the step, at the tread's middle and at the riser's: no frame
        // lifts the player more than a step, and nothing but the tread and
        // the riser is ever stood on.
        use primitive_shared::types::Facing;
        for kind in EVERY_STEP {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let chunks = floor_with_a_step(kind, facing);
                for side in 0..16 {
                    let angle = side as f32 / 16.0 * std::f32::consts::TAU;
                    let middle = Vec3::new(8.5, 10.0, 8.5);
                    let start = middle + Vec3::new(angle.cos(), 0.0, angle.sin()) * 2.5;
                    let heading = (middle - start).normalize();
                    let heights = heights_walking(&chunks, start, heading, 1.5);
                    let mut last = 10.0f32;
                    for &y in &heights {
                        assert!(
                            y - last <= PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0,
                            "{kind} {facing:?} from heading {side}: rose from {last} to {y} in one frame"
                        );
                        let resting = [10.0, 10.5, 11.0].iter().any(|h| (y - h).abs() < 0.02);
                        assert!(resting || y < last, "{kind} {facing:?} from heading {side}: stood at {y}");
                        last = y;
                    }
                }
            }
        }
    }

    #[test]
    fn walking_into_a_tree_from_any_side_never_lifts_a_player_more_than_a_step() {
        // The palm's ratchet (`for_each_solid_tagged`), asked of the wood of
        // every other tree, which is not tagged as bark: its tops at one
        // height lie within seven sixteenths of each other and the next height
        // is a cell on, so no run of steps climbs it. The generator's
        // saplings, young trees and grown trees, walked at from sixteen
        // headings, walking and sprinting, for three seconds each.
        use primitive_shared::types::{is_branch, BLOCK_LEAVES};
        let mut worst = (0.0f32, String::new());
        for stage in 0..primitive_shared::worldgen::TREE_STAGES {
            for variant in [0u32, 3, 7, 40] {
                let mut chunks = floor_world();
                let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
                let cells = primitive_shared::worldgen::tree_stage_cells(stage, variant, BLOCK_LEAVES, |_, _| 0).expect("a stage");
                for ((dx, dy, dz), id) in cells {
                    let (x, y, z) = (8 + dx, 9 + dy, 8 + dz);
                    if is_branch(id) && (0..16).contains(&x) && (0..16).contains(&z) {
                        chunk.set(x as usize, y as usize, z as usize, id);
                    }
                }
                chunks.insert(chunk);
                for side in 0..16 {
                    let angle = side as f32 / 16.0 * std::f32::consts::TAU;
                    let root = Vec3::new(8.5, 10.0, 8.5);
                    let mut player = Player::new((root + Vec3::new(angle.cos(), 0.0, angle.sin()) * 2.5).as_dvec3(), DEFAULT_MOVE_SPEED);
                    for frame in 0..180 {
                        let toward = (root - player.position.as_vec3()).with_y(0.0).normalize_or_zero();
                        player.update(&chunks, &[], toward, Vec3::X, false, false, frame % 2 == 0, 1.0 / 60.0);
                        let rose = player.position.y - 10.0;
                        if rose > f64::from(worst.0) {
                            worst = (rose as f32, format!("stage {stage} tree {variant:#x} side {side} frame {frame} at {:?}", player.position));
                        }
                    }
                }
            }
        }
        assert!(worst.0 <= PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0, "rose {} : {}", worst.0, worst.1);
    }

    /// **What looking a ring of columns further costs**, against the scan it
    /// replaced, in the same binary on the same fixture.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib \
    ///     what_the_palm_ring_costs_the_collider -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement: run in release"]
    fn what_the_palm_ring_costs_the_collider() {
        let chunks = floor_world();
        let (min, max) = player_box(Vec3::new(8.5, 10.0, 8.5));
        // The scan as it was: the cells the region spans and their boxes.
        let narrow = |visit: &mut dyn FnMut([f32; 3], [f32; 3])| {
            let (x0, x1) = (min.x.floor() as i32, (max.x - CONTACT_SKIN).floor() as i32);
            let (y0, y1) = (min.y.floor() as i32, (max.y - CONTACT_SKIN).floor() as i32);
            let (z0, z1) = (min.z.floor() as i32, (max.z - CONTACT_SKIN).floor() as i32);
            for bz in z0..=z1 {
                for bx in x0..=x1 {
                    let Some(column) = chunks.column(bx, bz) else { continue };
                    for by in y0..=y1 {
                        if let Some((bmin, bmax)) = primitive_shared::geometry::block_box(column.block(by), bx, by, bz) {
                            visit(bmin, bmax);
                        }
                    }
                }
            }
        };
        const CALLS: u32 = 2_000_000;
        let mut sink = 0usize;
        let started = std::time::Instant::now();
        for _ in 0..CALLS {
            narrow(&mut |_, _| sink += 1);
        }
        let before = started.elapsed().as_secs_f64() * 1e9 / CALLS as f64;
        let started = std::time::Instant::now();
        for _ in 0..CALLS {
            for_each_solid(&chunks, min, max, |_, _| sink += 1);
        }
        let after = started.elapsed().as_secs_f64() * 1e9 / CALLS as f64;
        println!("for_each_solid on open floor: {before:.1} ns a call before the ring, {after:.1} ns with it ({sink} boxes)");
    }

    /// A floor of ice at y = 0..=9, so the whole fixture is a rink.
    fn ice_world() -> ChunkManager {
        world_of(|y| {
            if y < 10 {
                primitive_shared::types::BLOCK_ICE
            } else {
                BLOCK_AIR
            }
        })
    }

    /// Runs a player forward for `seconds`, then lets go of the keys for
    /// `coast` seconds, and answers how far they travelled after letting
    /// go.
    fn coast_distance(chunks: &ChunkManager, seconds: f32, coast: f32) -> f32 {
        coast_distance_at(chunks, seconds, coast, 1.0 / 60.0)
    }

    /// The same, at a chosen frame time -- which is the point of
    /// `letting_go_stops_you_in_the_same_distance_at_every_frame_rate`.
    fn coast_distance_at(chunks: &ChunkManager, seconds: f32, coast: f32, dt: f32) -> f32 {
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..(seconds / dt) as usize {
            player.update(chunks, &[], Vec3::X, Vec3::X, false, false, false, dt);
        }
        let from = player.position.x;
        for _ in 0..(coast / dt) as usize {
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
        }
        (player.position.x - from) as f32
    }

    #[test]
    fn letting_go_bleeds_the_same_speed_away_at_every_frame_rate() {
        // **A frame rate is not a difficulty setting.** Friction was
        // integrated to first order -- `v * (1 - friction * dt)` -- so
        // how much of a walk survived a tenth of a second of letting go
        // depended on how the frames fell inside it: 0.88 blocks a
        // second at twenty frames, 1.44 at sixty, 1.65 on a machine fast
        // enough not to need `PHYSICS_STEP` at all, against a true
        // answer of 1.656. Nearly twice the speed retained, decided by
        // the machine -- and the wrong way round, so a stuttering
        // computer also handled twitchier.
        //
        // Twenty is in the list on purpose even though `PHYSICS_STEP`
        // keeps the shipped game at a sixtieth: this is the collider's
        // own property, and the substepping in `lib.rs` is a second
        // guard rather than the reason there is one.
        let chunks = floor_world();
        let remaining = |dt: f32| {
            let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            for _ in 0..(2.0 / dt) as usize {
                player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, dt);
            }
            for _ in 0..(0.1 / dt).round() as usize {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
            }
            player.horizontal_speed()
        };
        // Frame rates that divide the tenth of a second exactly: a step
        // that has to be rounded to fit would be comparing a slightly
        // different length of coast, which is a measurement error
        // dressed up as a failure.
        let reference = remaining(1.0 / 60.0);
        for hz in [20.0f32, 30.0, 100.0, 200.0, 300.0, 2000.0] {
            let speed = remaining(1.0 / hz);
            assert!(
                (speed - reference).abs() < 0.01,
                "a tenth of a second of letting go left {speed} b/s at {hz} frames a second, \
                 against {reference} at sixty"
            );
        }
    }

    #[test]
    fn letting_go_stops_you_in_much_the_same_distance_at_every_frame_rate() {
        // The half of the same number a player actually sees, and the
        // reason the tolerance here is a whole five centimetres rather
        // than the millimetre above.
        //
        // Friction is exact now; the *position* is still advanced by the
        // velocity at the end of a step rather than by the integral
        // across it, so a slide still comes out a little short and how
        // short depends on `dt`. Across the frame times the game can
        // actually produce -- `PHYSICS_STEP` caps a step at a sixtieth
        // of a second -- that residual is 0.41 blocks against a true
        // 0.46, and it used to be 0.37 against 0.46. It was left there
        // deliberately: making the horizontal integration exact as well
        // means the acceleration, the friction and the drag of water all
        // have to agree about what a step means, and half a centimetre
        // of slide is not worth reopening water for. This test is what
        // stops it getting worse again.
        let chunks = floor_world();
        let reference = coast_distance_at(&chunks, 2.0, 2.0, 1.0 / 60.0);
        for hz in [60.0f32, 144.0, 300.0, 2000.0] {
            let coast = coast_distance_at(&chunks, 2.0, 2.0, 1.0 / hz);
            assert!(
                (coast - reference).abs() < 0.05,
                "at {hz} frames a second a player slid {coast} blocks after letting go, \
                 against {reference} at sixty"
            );
        }
    }

    #[test]
    fn one_long_frame_does_not_stop_a_sprinter_dead() {
        // The far end of the same bug. `1 - friction * dt` goes negative
        // once a step is longer than `1 / friction`, and it was clamped
        // to zero -- so on stone (friction 12) any frame longer than a
        // twelfth of a second took *all* of a player's speed, at once.
        // The collider's own solidity tests run frames of a tenth of a
        // second on purpose, which is over that cliff.
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.velocity = Vec3::new(DEFAULT_MOVE_SPEED, 0.0, 0.0);
        player.apply_friction(GROUND_FRICTION, 0.25);
        // Three seconds' worth of friction in one step still leaves
        // something; what it must not leave is nothing.
        assert!(
            player.horizontal_speed() > 0.0,
            "one long frame took every last bit of a sprint"
        );

        // ...and the bite is the same however the frames fall, which is
        // the property the exponential is there for.
        let mut long = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        long.velocity = Vec3::new(DEFAULT_MOVE_SPEED, 0.0, 0.0);
        long.apply_friction(GROUND_FRICTION, 0.1);
        let mut short = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        short.velocity = Vec3::new(DEFAULT_MOVE_SPEED, 0.0, 0.0);
        for _ in 0..20 {
            short.apply_friction(GROUND_FRICTION, 0.005);
        }
        assert!(
            (long.horizontal_speed() - short.horizontal_speed()).abs() < 1e-3,
            "one tenth-of-a-second frame left {} and twenty short ones left {}",
            long.horizontal_speed(),
            short.horizontal_speed()
        );
    }

    /// The highest the feet get, and how long they are off the ground,
    /// for a standing jump taken at `dt` per step.
    fn standing_jump(chunks: &ChunkManager, dt: f32) -> (f32, f32) {
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        // One settling step, so the jump is taken from the ground
        // rather than from wherever `new` put the feet.
        player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
        let floor = player.position.y;
        player.update(chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, dt);
        let mut apex = player.position.y;
        let mut airtime = dt;
        for _ in 0..10_000 {
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
            apex = apex.max(player.position.y);
            if player.grounded {
                break;
            }
            airtime += dt;
        }
        ((apex - floor) as f32, airtime)
    }

    #[test]
    fn a_jump_reaches_the_same_height_at_every_frame_rate() {
        // **The same key must do the same thing on every machine.** The
        // position used to be advanced by the velocity gravity had
        // *already* been applied to, which charges a whole step of
        // falling to a body that only fell for part of it. What came out
        // was a jump that measured 1.32 blocks at thirty frames a
        // second, 1.39 at sixty and 1.45 at a thousand -- a tenth of a
        // block of jump handed out for owning a better computer, with
        // the airtime, and therefore the length of a running jump,
        // stretching with it.
        //
        // The tolerance is a centimetre because that is all that is
        // left: the trajectory is exact now, so the only spread is where
        // a sample lands relative to the true apex.
        let chunks = floor_world();
        let (height, airtime) = standing_jump(&chunks, 1.0 / 60.0);
        for hz in [20.0f32, 30.0, 120.0, 144.0, 300.0, 1000.0] {
            let (h, t) = standing_jump(&chunks, 1.0 / hz);
            assert!(
                (h - height).abs() < 0.01,
                "a jump at {hz} frames a second reached {h} against {height} at sixty"
            );
            assert!(
                (t - airtime).abs() < 0.05,
                "a jump at {hz} frames a second lasted {t}s against {airtime}s at sixty"
            );
        }
    }

    #[test]
    fn a_running_jump_covers_the_same_ground_at_every_frame_rate() {
        // The half of the same bug a player would actually notice: a gap
        // you can clear is a gap you can clear. Airtime followed the
        // frame rate, so the distance did too -- 6.31 blocks at sixty
        // against 6.40 at a thousand.
        let chunks = floor_world();
        let jump_length = |dt: f32| {
            let mut player = Player::new((Vec3::new(2.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            for _ in 0..(1.0 / dt) as usize {
                player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, dt);
            }
            let from = player.position.x;
            player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, true, dt);
            for _ in 0..10_000 {
                player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, dt);
                if player.grounded {
                    break;
                }
            }
            player.position.x - from
        };
        let reference = jump_length(1.0 / 60.0);
        for hz in [30.0f32, 144.0, 300.0, 1000.0] {
            let distance = jump_length(1.0 / hz);
            assert!(
                (distance - reference).abs() < 0.1,
                "a running jump at {hz} frames a second carried {distance} blocks \
                 against {reference} at sixty"
            );
        }
    }

    #[test]
    fn a_player_carries_further_on_ice_than_on_stone() {
        // The whole of what grip is for. Same walk, same release, and
        // the only difference is what is under the feet.
        let on_stone = coast_distance(&floor_world(), 2.0, 1.5);
        let on_ice = coast_distance(&ice_world(), 2.0, 1.5);
        assert!(
            on_stone < 0.6,
            "stone let a player coast {on_stone} blocks after letting go"
        );
        assert!(
            on_ice > on_stone * 4.0,
            "ice ({on_ice}) is barely different from stone ({on_stone})"
        );
    }

    #[test]
    fn ice_does_not_make_a_player_faster() {
        // Grip stretches the time either side of top speed; it must not
        // change the top speed itself, or a frozen lake becomes a
        // shortcut and the server's anti-cheat starts arguing with a
        // client that is doing nothing wrong.
        let chunks = ice_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let dt = 1.0 / 60.0;
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, dt);
        }
        assert!(
            player.horizontal_speed() <= DEFAULT_MOVE_SPEED + 0.01,
            "ice ran a player up to {}",
            player.horizontal_speed()
        );
    }

    #[test]
    fn ice_is_slower_to_get_going_than_stone() {
        // The other half of the same number, and the half that stops
        // "slippery" being a free head start: the first stride on ice
        // has to be worse than the first stride on stone.
        let sprint = |chunks: &ChunkManager| {
            let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            for _ in 0..12 {
                player.update(chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
            }
            player.horizontal_speed()
        };
        assert!(
            sprint(&ice_world()) < sprint(&floor_world()) * 0.5,
            "the first fifth of a second on ice was not slower than on stone"
        );
    }

    #[test]
    fn a_player_falls_and_lands_on_the_floor() {
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 25.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..400 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(player.grounded, "player never landed");
        assert!(
            (player.position.y - 10.0).abs() < 0.1,
            "landed at {} instead of on top of the floor",
            player.position.y
        );
    }

    /// Every block a player can stand on that does not fill its cell,
    /// with the height its top sits at.
    ///
    /// **Read off the block table rather than written out here.** The
    /// bug this list exists for was a property of the *shape*, not of
    /// the campfire, and a list typed by hand would have to be
    /// remembered on the day a fourth short block is added -- which is
    /// exactly the day it would not be. Three today: a campfire, lit or
    /// not, at a quarter of a cell, and a dropped pack at half of one.
    ///
    /// Shared with `anticheat_agreement_tests`, which asks the other
    /// half of the same question: the collider here decides where these
    /// blocks put a player, and the server has to be able to explain
    /// the answer.
    pub(crate) fn part_height_blocks() -> Vec<(primitive_shared::types::BlockId, &'static str, f32)>
    {
        let found: Vec<_> = primitive_shared::blocks::BLOCKS
            .iter()
            .map(|def| {
                (def.id, def.name, primitive_shared::types::collision_height(def.id))
            })
            .filter(|&(_, _, height)| height > 0.0 && height < 1.0)
            .collect();
        assert!(
            !found.is_empty(),
            "nothing in the block table is shorter than its cell any more, \
             so these tests are checking nothing"
        );
        found
    }

    #[test]
    fn a_block_that_does_not_fill_its_cell_is_still_jumped_off() {
        // **"Прыжок на костре или рюкзаке не работает."** Standing on
        // anything shorter than its cell, the jump key did nothing at
        // all -- not a short hop, nothing. See the note in `sweep_axis`
        // about a box the collider has already gone past.
        for (block, name, height) in part_height_blocks() {
            let chunks = floor_with(8, 8, block);
            let mut player = Player::new((Vec3::new(8.5, 11.5, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            for _ in 0..60 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            }
            let rest = player.position.y;
            assert!(
                (rest - f64::from(10.0 + height)).abs() < 0.05,
                "a player dropped onto a {name} settled at {rest} rather than {}",
                10.0 + height
            );
            assert!(player.grounded, "standing on a {name} did not read as ground");

            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
            assert!(player.jumped, "the jump key on a {name} did nothing");
            let mut apex = player.position.y;
            for _ in 0..120 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                apex = apex.max(player.position.y);
            }
            assert!(
                apex - rest > 1.0,
                "a jump off a {name} rose {:.2} blocks, which is not a jump",
                apex - rest
            );
        }
    }

    #[test]
    fn a_fall_that_outruns_a_frame_still_stops_at_the_floor() {
        // **The classic collision failure, pinned at the collider rather
        // than at its caller.** The frame loop steps physics in fixed
        // slices (`PHYSICS_STEP` in `lib.rs`), so a stutter cannot hand
        // this a huge `dt` today -- and this game does stutter while
        // terrain streams. That substepping is one guard; the sweep is
        // the other, and it is the one a refactor could quietly lose.
        //
        // At terminal velocity a half-second step is twenty-five blocks
        // of travel against a floor one block thick. A collider that
        // tested where the player *ends up* rather than where they went
        // would find air on both sides of the floor and drop them out of
        // the world.
        let chunks = world_of(|y| if y == 9 { BLOCK_STONE } else { BLOCK_AIR });
        let mut player = Player::new((Vec3::new(8.0, 40.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..20 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 0.5);
        }
        assert!(
            (player.position.y - 10.0).abs() < 0.1,
            "fell through a one-block floor: ended at {}",
            player.position.y
        );
        assert!(player.grounded, "landed but did not notice");
    }

    #[test]
    fn move_speed_from_settings_is_actually_used() {
        let chunks = floor_world();
        let mut slow = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), 1.0);
        let mut fast = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), 8.0);
        for _ in 0..30 {
            slow.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
            fast.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            fast.position.x > slow.position.x + 0.5,
            "the configured speed had no effect"
        );
    }

    #[test]
    fn players_cannot_stand_inside_each_other() {
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let other = Vec3::new(8.6, 10.0, 8.0);
        for _ in 0..60 {
            player.update(&chunks, &[other.as_dvec3()], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            !aabb_overlaps_player(player.position.as_vec3(), other),
            "walked straight through another player"
        );
    }

    /// Jumps, then holds a direction for `frames` while airborne.
    ///
    /// The launch frame is deliberately steered with nothing at all. It
    /// is still a *grounded* frame, so any input on it accelerates at
    /// ground rates and would be counted as air control by whatever the
    /// test measures afterwards.
    fn jump_then_steer(chunks: &ChunkManager, player: &mut Player, dir: Vec3, frames: usize) {
        player.update(chunks, &[], Vec3::ZERO, Vec3::X, true, false, false, 1.0 / 60.0);
        for _ in 0..frames {
            player.update(chunks, &[], dir, Vec3::X, false, false, true, 1.0 / 60.0);
        }
    }

    /// Brings the player up to steady speed on flat ground.
    fn run_up(chunks: &ChunkManager, player: &mut Player, dir: Vec3, sprinting: bool) {
        for _ in 0..60 {
            player.update(chunks, &[], dir, Vec3::X, false, false, sprinting, 1.0 / 60.0);
        }
    }

    #[test]
    fn you_cannot_speed_up_in_mid_air() {
        // The whole point of the air cap. Jump while sprinting and hold
        // forward: you keep what you had and gain nothing.
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        run_up(&chunks, &mut player, Vec3::X, true);
        let ground_speed = player.horizontal_speed();
        assert!(ground_speed > DEFAULT_MOVE_SPEED, "never reached a sprint");

        jump_then_steer(&chunks, &mut player, Vec3::X, 20);
        assert!(!player.grounded, "should still be airborne");
        assert!(
            player.horizontal_speed() <= ground_speed + 0.01,
            "gained speed in the air: {} then {}",
            ground_speed,
            player.horizontal_speed()
        );
    }

    #[test]
    fn you_can_still_steer_a_little_in_mid_air() {
        // Capped is not frozen: a jump with no control at all feels
        // broken in the other direction.
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        run_up(&chunks, &mut player, Vec3::X, false);
        jump_then_steer(&chunks, &mut player, Vec3::Z, 20);
        assert!(
            player.velocity.z > 0.2,
            "no air steering at all: z velocity {}",
            player.velocity.z
        );
        assert!(
            player.velocity.z <= AIR_CONTROL_SPEED + 0.01,
            "air steering ran past its cap: {}",
            player.velocity.z
        );
    }

    #[test]
    fn a_standing_jump_does_not_become_a_sprint() {
        // Jumping from a standstill and holding forward must not build
        // up to walking speed in the air.
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        jump_then_steer(&chunks, &mut player, Vec3::X, 25);
        assert!(
            player.horizontal_speed() <= AIR_CONTROL_SPEED + 0.01,
            "a standing jump reached {} blocks per second",
            player.horizontal_speed()
        );
    }

    #[test]
    fn a_jump_keeps_the_run_that_launched_it() {
        // The other half: momentum survives leaving the ground, so a
        // running jump goes further than a standing one.
        let chunks = floor_world();
        let launch = |sprinting: bool| {
            let mut player = Player::new((Vec3::new(2.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            run_up(&chunks, &mut player, Vec3::X, sprinting);
            let start = player.position.x;
            jump_then_steer(&chunks, &mut player, Vec3::X, 30);
            player.position.x - start
        };
        assert!(
            launch(true) > launch(false),
            "a sprinting jump covered no more ground than a walking one"
        );
    }

    #[test]
    fn letting_go_slows_you_down_instead_of_stopping_you_dead() {
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let moving = player.horizontal_speed();
        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        let after_one_frame = player.horizontal_speed();
        assert!(after_one_frame < moving, "friction did nothing");
        assert!(after_one_frame > 0.0, "stopped dead in a single frame");

        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            player.horizontal_speed() < 0.05,
            "never came to rest: {}",
            player.horizontal_speed()
        );
    }

    #[test]
    fn walking_still_reaches_full_speed_quickly() {
        // Acceleration must not read as sluggishness: a fifth of a
        // second to full speed is about the limit before it feels like
        // wading.
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..12 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            player.horizontal_speed() > DEFAULT_MOVE_SPEED * 0.9,
            "only reached {} of {DEFAULT_MOVE_SPEED} after a fifth of a second",
            player.horizontal_speed()
        );
    }

    #[test]
    fn teleport_clears_momentum() {
        let mut player = Player::new((Vec3::new(0.0, 50.0, 0.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.velocity = Vec3::new(3.0, -20.0, 1.0);
        player.teleport((Vec3::new(1.0, 2.0, 3.0)).as_dvec3());
        assert_eq!(player.position, (Vec3::new(1.0, 2.0, 3.0)).as_dvec3());
        assert_eq!(player.velocity, Vec3::ZERO);
    }

    #[test]
    fn somebody_walking_into_you_shoves_you_out_instead_of_welding_you() {
        // A remote player's box is carried by the network, not by this
        // collider, so it walks straight through anyone standing still.
        // Whoever it landed on was then blocked on every axis at once
        // by the very box they were inside: unable to walk, unable to
        // fall, stuck in mid-air until the other player wandered off.
        let chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let other = Vec3::new(8.5, 10.0, 8.5); // standing exactly on them
        player.update(&chunks, &[other.as_dvec3()], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(
            !aabb_overlaps_player(player.position.as_vec3(), other),
            "left standing inside another player at {:?}",
            player.position
        );
        // Pushed aside, not dropped through the floor or launched.
        assert!(
            (player.position.y - 10.0).abs() < 0.05,
            "left the floor while getting out of the way, y={}",
            player.position.y
        );

        // ...and once out, they can walk again.
        let before = player.position.x;
        for _ in 0..30 {
            player.update(&chunks, &[other.as_dvec3()], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            player.position.x > before + 0.5,
            "still welded in place: {before} then {}",
            player.position.x
        );
    }

    #[test]
    fn raycast_finds_the_block_and_the_cell_in_front_of_it() {
        let chunks = floor_world();
        // Looking straight down from above the floor.
        let hit = raycast_block(&chunks, (Vec3::new(8.5, 12.0, 8.5)).as_dvec3(), -Vec3::Y, 6.0);
        let (block, before) = hit.expect("ray should have hit the floor");
        assert_eq!(block, (8, 9, 8));
        assert_eq!(before, (8, 10, 8), "placement cell must be above the hit");
    }
}

/// The one thing a collider must never do.
///
/// Everything else in physics is a matter of feel; being inside a wall
/// is a bug you can see from across the room, and the way it happens is
/// never the way anyone guessed. So this walks a player into walls,
/// corners, ledges and ceilings from every angle, at every speed, and
/// checks the one invariant after every single frame.
#[cfg(test)]
mod solidity_tests {
    use super::tests::{floor_with, floor_world};
    use super::*;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_SNOW, BLOCK_STONE, ChunkPos};

    /// A stone floor at y = 0..=9 with a scattering of walls, pillars,
    /// ledges and low ceilings on top of it.
    ///
    /// Nine chunks, not one. A player let loose for ten seconds walks
    /// out of a single chunk, and an unloaded chunk is *nothing* to
    /// collide with -- so a one-chunk fixture tests falling out of the
    /// world rather than walking into walls.
    fn obstacle_course() -> ChunkManager {
        let mut chunks = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                chunks.insert(course_chunk(ChunkPos::new(cx, cz)));
            }
        }
        chunks
    }

    fn course_chunk(pos: ChunkPos) -> primitive_shared::types::Chunk {
        use primitive_shared::types::{Chunk, BLOCK_AIR, CHUNK_VOLUME};
        let mut chunk = Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        };
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    chunk.set(x, y, z, BLOCK_STONE);
                }
            }
        }
        {
            let chunk = &mut chunk;
            for x in 0..16usize {
                for z in 0..16usize {
                // A cheap deterministic scatter -- the shapes matter,
                // not their distribution.
                let h = (x * 7 + z * 13) % 11;
                match h {
                    0 | 1 => {
                        // A full-height pillar.
                        for y in 10..13 {
                            chunk.set(x, y, z, BLOCK_STONE);
                        }
                    }
                    2 => {
                        // A single block to climb, where a shallow
                        // ledge used to be. Nothing is steppable now.
                        chunk.set(x, 10, z, BLOCK_SAND);
                    }
                    3 => {
                        // A block with a gap over it, then a ceiling.
                        chunk.set(x, 10, z, BLOCK_STONE);
                        chunk.set(x, 12, z, BLOCK_STONE);
                    }
                    4 => {
                        // A one-block hole in the floor.
                        chunk.set(x, 9, z, primitive_shared::types::BLOCK_AIR);
                    }
                    _ => {}
                    }
                }
            }
        }
        chunk
    }

    /// How far into a solid the collider reaches, in blocks. Zero is the
    /// only acceptable answer.
    fn penetration(chunks: &ChunkManager, feet: Vec3) -> f32 {
        let (min, max) = player_box(feet);
        let mut worst = 0.0f32;
        for_each_solid(chunks, min, max, |bmin, bmax| {
            // How deep the two boxes overlap on each axis; the shallowest
            // of the three is how far in the player actually is.
            let mut deepest = f32::MAX;
            for axis in 0..3 {
                let lo = min[axis].max(bmin[axis]);
                let hi = max[axis].min(bmax[axis]);
                deepest = deepest.min(hi - lo);
            }
            if deepest > 0.0 {
                worst = worst.max(deepest);
            }
        });
        worst
    }

    /// A tiny deterministic generator: the same walk every run, because
    /// a failure nobody can reproduce is a failure nobody can fix.
    pub(super) struct Wander(pub(super) u32);

    impl Wander {
        pub(super) fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as f32 / (1 << 24) as f32
        }
    }

    #[test]
    fn a_player_never_ends_a_frame_inside_a_wall() {
        let chunks = obstacle_course();
        let mut rng = Wander(0x5EED);

        // Several starting points, because where you enter a shape
        // decides which of its faces you meet first.
        for start in 0..12 {
            let mut player = Player::new(
                (Vec3::new(2.5 + start as f32, 13.5, 3.5 + (start % 5) as f32)).as_dvec3(),
                DEFAULT_MOVE_SPEED,
            );
            let mut dir = Vec3::X;
            for frame in 0..600 {
                if frame % 7 == 0 {
                    // A new direction now and then, including into the
                    // wall the player is already against.
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                // Turned back at the edge of the loaded area: past it
                // there is nothing to collide with, and a player
                // falling through *unloaded* space is a streaming
                // question rather than a collision one.
                let centre = Vec3::new(8.0, player.position.y as f32, 8.0);
                if (player.position.x - 8.0).abs() > 12.0
                    || (player.position.z - 8.0).abs() > 12.0
                {
                    let back = centre - player.position.as_vec3();
                    dir = Vec3::new(back.x, 0.0, back.z).normalize_or_zero();
                }
                // Frame times from a fast machine to a bad hitch: a
                // sweep that only works at 60 fps is a sweep that fails
                // on somebody's laptop.
                let dt = 1.0 / 240.0 + rng.next() * 0.1;
                let jump = rng.next() > 0.85;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, rng.next() > 0.5, dt);

                let inside = penetration(&chunks, player.position.as_vec3());
                assert!(
                    inside <= CONTACT_SKIN * 2.0,
                    "start {start}, frame {frame}: {inside} blocks inside a wall at {:?} \
                     (velocity {:?}, grounded {})",
                    player.position,
                    player.velocity,
                    player.grounded
                );
                assert!(
                    player.position.y > -1.0,
                    "start {start}, frame {frame}: fell out of the world"
                );
            }
        }
    }

    #[test]
    fn a_player_pressed_into_a_corner_stays_out_of_both_walls() {
        // The case a one-axis-at-a-time resolver gets wrong: two walls
        // meeting, approached diagonally, so each axis is blocked by a
        // different block and the order they are resolved in decides
        // the answer.
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for y in 10..13 {
            for i in 0..8usize {
                chunk.set(8, y, i, BLOCK_STONE);
                chunk.set(i, y, 8, BLOCK_STONE);
            }
        }
        chunks.insert(chunk);

        for &speed in &[1.0f32, 6.0, 20.0] {
            let mut player = Player::new((Vec3::new(4.5, 10.0, 4.5)).as_dvec3(), speed);
            for _ in 0..240 {
                player.update(
                    &chunks,
                    &[],
                    Vec3::new(1.0, 0.0, 1.0).normalize(),
                    Vec3::X,
                    false,
                    false,
                    true,
                    1.0 / 60.0,
                );
            }
            let inside = penetration(&chunks, player.position.as_vec3());
            assert!(
                inside <= CONTACT_SKIN * 2.0,
                "at speed {speed} the player ended {inside} blocks inside the corner at {:?}",
                player.position
            );
        }
    }

    /// The furthest a frame may legitimately carry a player sideways.
    ///
    /// Measured against the game's own speed limit rather than against
    /// the velocity the frame started with: ground acceleration is
    /// deliberately near-instant, so a player who changes direction
    /// reaches full speed *within* the frame. What cannot happen is
    /// exceeding the limit itself -- a sprint along one axis plus a
    /// sprint along another, because the acceleration cap applies to
    /// each direction separately.
    fn allowed_sideways(dt: f32) -> f32 {
        DEFAULT_MOVE_SPEED * SPRINT_MULTIPLIER * std::f32::consts::SQRT_2 * dt
            + CONTACT_SKIN * 8.0
    }

    #[test]
    fn walking_into_a_wall_never_moves_a_player_further_than_they_can_walk() {
        // The complaint this exists for: running at a wall and being
        // thrown somewhere. Walls at every angle, at every speed, with
        // ledges and layers on them, and the invariant checked on every
        // single frame -- because a teleport is not a state you can see
        // afterwards, it is a *step* you have to catch happening.
        let chunks = obstacle_course();
        let mut rng = Wander(0xC0FFEE);

        for start in 0..16 {
            let mut player = Player::new(
                (Vec3::new(3.5 + (start % 9) as f32, 13.0, 2.5 + (start % 7) as f32)).as_dvec3(),
                DEFAULT_MOVE_SPEED,
            );
            let mut dir = Vec3::X;
            for frame in 0..900 {
                if frame % 5 == 0 {
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                if (player.position.x - 8.0).abs() > 12.0
                    || (player.position.z - 8.0).abs() > 12.0
                {
                    let back = Vec3::new((8.0 - player.position.x) as f32, 0.0, (8.0 - player.position.z) as f32);
                    dir = back.normalize_or_zero();
                }
                // A fixed step, and a short one: the distance a frame
                // may legitimately cover is proportional to it, so a
                // long frame hides exactly the jump this is looking for.
                let dt = if frame % 2 == 0 { 1.0 / 60.0 } else { 1.0 / 144.0 };
                let before = player.position;
                let jump = rng.next() > 0.85;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, true, dt);

                let sideways = Vec3::new(
                    (player.position.x - before.x) as f32,
                    0.0,
                    (player.position.z - before.z) as f32,
                )
                .length();
                assert!(
                    sideways <= allowed_sideways(dt),
                    "start {start}, frame {frame}: thrown {sideways} sideways in one frame                      (a sprint covers {}) from {before:?} to {:?}",
                    allowed_sideways(dt),
                    player.position
                );
            }
        }
    }

    #[test]
    fn running_at_a_flat_wall_from_every_angle_is_just_a_stop() {
        // The complaint in its plainest form: a wall, a player running
        // into it, and nothing else. A room rather than the obstacle
        // course, because a *face* behaves differently from a pillar --
        // the player slides along it, and sliding is where a resolver
        // gets to disagree with itself between one axis and the next.
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for y in 10..14 {
            for i in 0..16usize {
                chunk.set(i, y, 3, BLOCK_STONE);
                chunk.set(i, y, 12, BLOCK_STONE);
                chunk.set(3, y, i, BLOCK_STONE);
                chunk.set(12, y, i, BLOCK_STONE);
            }
        }
        chunks.insert(chunk);

        for angle_step in 0..16 {
            let angle = angle_step as f32 / 16.0 * std::f32::consts::TAU;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            for &speed in &[1.0f32, 5.5, 12.0] {
                let mut player = Player::new((Vec3::new(7.5, 10.0, 7.5)).as_dvec3(), speed);
                for frame in 0..400 {
                    let dt = if frame % 3 == 0 { 1.0 / 60.0 } else { 1.0 / 300.0 };
                    let before = player.position;
                    player.update(&chunks, &[], dir, Vec3::X, frame % 37 == 0, false, true, dt);

                    let sideways =
                        Vec3::new((player.position.x - before.x) as f32, 0.0, (player.position.z - before.z) as f32)
                            .length();
                    // Air control has a cap of its own, and it does not
                    // scale with the walking speed -- a very slow player
                    // can still steer at `AIR_CONTROL_SPEED` in each of
                    // two directions while airborne.
                    let fastest = (speed * SPRINT_MULTIPLIER).max(AIR_CONTROL_SPEED);
                    let budget = fastest * std::f32::consts::SQRT_2 * dt + CONTACT_SKIN * 8.0;
                    assert!(
                        sideways <= budget,
                        "angle {angle_step}, speed {speed}, frame {frame}: thrown {sideways} \
                         sideways (budget {budget}) from {before:?} to {:?}",
                        player.position
                    );
                    assert!(
                        penetration(&chunks, player.position.as_vec3()) <= CONTACT_SKIN * 2.0,
                        "angle {angle_step}, speed {speed}, frame {frame}: inside the wall at {:?}",
                        player.position
                    );
                }
            }
        }
    }

    #[test]
    fn a_shut_door_stops_a_runner_from_either_side_at_every_angle_and_an_open_one_lets_them_through() {
        // A wall with a doorway and a door hung in it, every way round: the
        // wall runs across the way the door faces, as it does when a player
        // hangs one looking through the doorway -- across z = 8 for a door
        // facing north or south, across x = 8 for east or west. Shut, three
        // sixteenths of boards are a wall: run at from inside or out,
        // straight or glancing, at a walk or a sprint, the player never ends
        // up past it and never inside it. Open, the same run straight at the
        // doorway goes through.
        use primitive_shared::types::{door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            // Along x (the wall's own axis) and across it, as world x and z.
            let swap = matches!(facing, Facing::East | Facing::West);
            let world = |along: f32, across: f32| if swap { (across, along) } else { (along, across) };
            for open in [false, true] {
                let mut chunks = floor_world();
                let mut lower = faced(BLOCK_DOOR, facing);
                if open {
                    lower = door_swung(lower);
                }
                let (_, top) = door_partner((0, 0, 0), lower).unwrap();
                // The wall through the chunks either side as well, or a
                // glancing run slides along it and round its end.
                for c in [-1, 0, 1] {
                    let pos = if swap { ChunkPos::new(0, c) } else { ChunkPos::new(c, 0) };
                    let mut chunk = chunks.get(pos).unwrap().clone();
                    for y in 10..13 {
                        for i in 0..16usize {
                            let (x, z) = if swap { (8, i) } else { (i, 8) };
                            chunk.set(x, y, z, BLOCK_STONE);
                        }
                    }
                    if c == 0 {
                        let (x, z) = if swap { (8, 7) } else { (7, 8) };
                        chunk.set(x, 10, z, lower);
                        chunk.set(x, 11, z, top);
                    }
                    chunks.insert(chunk);
                }

                for (start, sign) in [(5.5f32, 1.0f32), (11.5, -1.0)] {
                    let angles: &[f32] = if open { &[0.0] } else { &[0.0, 0.3, -0.3, 0.7, -0.7] };
                    for &angle in angles {
                        let (dx, dz) = world(angle.sin(), sign * angle.cos());
                        let dir = Vec3::new(dx, 0.0, dz);
                        for &speed in &[4.3f32, 12.0] {
                            let (px, pz) = world(7.5, start);
                            let mut player = Player::new((Vec3::new(px, 10.0, pz)).as_dvec3(), speed);
                            for frame in 0..240 {
                                let dt = if frame % 3 == 0 { 1.0 / 60.0 } else { 1.0 / 300.0 };
                                player.update(&chunks, &[], dir, Vec3::X, false, false, true, dt);
                                if !open {
                                    assert!(
                                        penetration(&chunks, player.position.as_vec3()) <= CONTACT_SKIN * 2.0,
                                        "{facing:?}, from {start}, angle {angle}, speed {speed}: inside the door at {:?}",
                                        player.position
                                    );
                                }
                            }
                            let across = if swap { player.position.x } else { player.position.z };
                            let past = if sign > 0.0 { across > 9.0 } else { across < 8.0 };
                            assert_eq!(
                                past, open,
                                "{facing:?} {}: from {start}, angle {angle}, speed {speed}, the runner ended at {:?}",
                                if open { "open" } else { "shut" },
                                player.position
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sliding_along_a_wall_never_makes_you_faster_than_running() {
        // The bug in its purest form, and the one that produced the
        // teleports: press almost *parallel* into a wall. The blocked
        // axis is zeroed every frame, so the "speed along the direction
        // asked for" reads near zero however fast the player is
        // actually going, and the acceleration cap keeps paying out.
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for y in 10..14 {
            for z in 0..16usize {
                chunk.set(12, y, z, BLOCK_STONE);
            }
        }
        chunks.insert(chunk);

        // A hair off parallel: almost all of the push goes into the
        // wall, and a sliver of it along the wall.
        let dir = Vec3::new(0.995, 0.0, 0.0998).normalize();
        let mut player = Player::new((Vec3::new(8.5, 10.0, 4.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let top_speed = DEFAULT_MOVE_SPEED * SPRINT_MULTIPLIER;
        for frame in 0..600 {
            // Jumping, because the ground has friction and the air does
            // not: on the ground friction bleeds the runaway away as
            // fast as it builds, and it is in the air -- hopping along a
            // wall, which is a thing players do constantly -- that it
            // has nothing to stop it.
            let jump = player.grounded;
            player.update(&chunks, &[], dir, Vec3::X, jump, jump, true, 1.0 / 60.0);
            assert!(
                player.horizontal_speed() <= top_speed + 0.01,
                "frame {frame}: sliding along the wall reached {} b/s against a sprint of                  {top_speed}",
                player.horizontal_speed()
            );
        }
    }

    #[test]
    fn a_jump_into_a_ledge_never_claims_to_be_standing_on_it() {
        // What the server sees is what gets a player teleported: a
        // client that says "on the ground" while it is climbing, over
        // air, is a client running a flight cheat as far as the
        // anti-cheat is concerned -- and the correction it sends back is
        // the teleport. The step-up used to do exactly that when a jump
        // met a ledge.
        let chunks = floor_with(9, 8, BLOCK_SAND);
        let mut player = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        // Run at the ledge and jump into it.
        for frame in 0..90 {
            let jump = frame == 10;
            player.update(&chunks, &[], Vec3::X, Vec3::X, jump, jump, true, 1.0 / 60.0);
            if player.velocity.y > 0.01 {
                assert!(
                    !player.grounded,
                    "frame {frame}: claimed ground contact while rising at {:?}",
                    player.position
                );
            }
        }
        // ...and a walker is stopped by it rather than lifted over it,
        // which is what a block being a block means. You jump, or you
        // go round.
        let mut walker = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..90 {
            walker.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(walker.position.x < 9.0, "walked up a whole block");
        assert!((walker.position.y - 10.0).abs() < 0.01, "rose without jumping");
    }

    #[test]
    fn a_player_let_loose_on_real_terrain_stays_out_of_it() {
        // The obstacle course is made of the shapes I thought to build.
        // Terrain is made of the shapes the generator actually produces
        // -- overhangs, cave mouths, one-block ledges, drifts of snow on
        // slopes, gravel banks at the waterline -- and it is what
        // players walk on. Several seeds, because a seed is a world.
        use primitive_shared::worldgen::WorldGen;
        for seed in [1337u32, 7, 2024] {
            let generator = WorldGen::new(seed);
            let mut chunks = ChunkManager::new(4);
            for cx in -1..=1 {
                for cz in -1..=1 {
                    chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
                }
            }

            let ground = generator.height_at(8, 8) as f32 + 2.0;
            let mut rng = Wander(seed ^ 0xA11CE);
            let mut player = Player::new((Vec3::new(8.5, ground, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            let mut dir = Vec3::X;
            for frame in 0..1200 {
                if frame % 11 == 0 {
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                if (player.position.x - 8.0).abs() > 10.0
                    || (player.position.z - 8.0).abs() > 10.0
                {
                    let back = Vec3::new((8.0 - player.position.x) as f32, 0.0, (8.0 - player.position.z) as f32);
                    dir = back.normalize_or_zero();
                }
                let dt = 1.0 / 240.0 + rng.next() * 0.06;
                let jump = rng.next() > 0.8;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, rng.next() > 0.5, dt);

                let inside = penetration(&chunks, player.position.as_vec3());
                assert!(
                    inside <= CONTACT_SKIN * 2.0,
                    "seed {seed}, frame {frame}: {inside} blocks inside the world at {:?}",
                    player.position
                );
            }
        }
    }

    #[test]
    fn standing_still_comes_to_rest_instead_of_bobbing() {
        // "Проваливается в блоки и выскакивает на них": a player who is
        // not moving must not move. Anything that lowers the ground
        // under somebody and anything that lifts them back onto it are
        // two halves of a loop, and a loop that closes is a player
        // sinking and popping out, over and over, standing still.
        use primitive_shared::worldgen::WorldGen;
        for seed in [1337u32, 7, 2024] {
            let generator = WorldGen::new(seed);
            let mut chunks = ChunkManager::new(4);
            for cx in -1..=1 {
                for cz in -1..=1 {
                    chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
                }
            }
            for spot in 0..24 {
                let (x, z) = (4.5 + (spot % 8) as f32, 4.5 + (spot / 8) as f32);
                let ground = generator.height_at(x.floor() as i32, z.floor() as i32) as f32 + 3.0;
                let mut player = Player::new((Vec3::new(x, ground, z)).as_dvec3(), DEFAULT_MOVE_SPEED);
                // Land, and let everything that settles finish settling
                // -- snow is meant to give way underfoot, and that is
                // not what this is looking for.
                for _ in 0..600 {
                    player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                }
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for _ in 0..300 {
                    player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                    lo = lo.min(player.position.y as f32);
                    hi = hi.max(player.position.y as f32);
                }
                assert!(
                    hi - lo < 0.01,
                    "seed {seed}, standing at ({x}, {z}): bobbing {} blocks, \
                     between y={lo} and y={hi}",
                    hi - lo
                );
            }
        }
    }

    #[test]
    fn a_step_too_short_to_be_blocked_is_still_blocked() {
        // The creep that fed the catapult. The sweep used to want the
        // obstacle to be a contact skin *nearer* than the move before
        // it counted as blocking -- which says that a move shorter than
        // the skin can never be blocked by anything at all. A player
        // pressed into a wall asks for exactly that on the blocked axis
        // every frame, because the axis was zeroed the frame before.
        let chunks = floor_with(9, 8, BLOCK_STONE);
        // A fifth of a millimetre into the block's -X face: past the
        // tolerance `for_each_solid` spends on "touching", so the
        // column is genuinely in range.
        let feet = Vec3::new(9.0002 - PLAYER_HALF_WIDTH, 10.0, 8.5);
        let hit = sweep_axis(&chunks, &[], feet, CONTACT_SKIN / 4.0, X);
        assert!(hit.blocked, "a very short step into a wall was not blocked");
        assert_eq!(hit.allowed, 0.0, "allowed {} further in", hit.allowed);
    }

    #[test]
    fn a_hairs_breadth_inside_a_wall_is_a_nudge_aside_not_a_climb_onto_it() {
        // The other half. Being buried in a block that arrived around
        // you is worth standing on top of; being a fifth of a
        // millimetre into the side of one you walked up against is
        // worth a nudge of a fifth of a millimetre. Answering the
        // second with the first is a free metre of climb for touching a
        // wall -- and it is what "выскакивает на них" was.
        let chunks = floor_with(9, 8, BLOCK_STONE);
        let mut player = Player::new((Vec3::new(9.0002 - PLAYER_HALF_WIDTH, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(
            player.position.y < 10.5,
            "climbed onto the block it was barely touching: y={}",
            player.position.y
        );
        assert!(
            penetration(&chunks, player.position.as_vec3()) <= CONTACT_SKIN * 2.0,
            "left inside the wall at {:?}",
            player.position
        );
    }

    /// Where along the cell a north-facing rack's poles stand, read off
    /// `assets/models/misc/drying_rack.bbmodel` -- and asserted against
    /// `types::collision_depth`, so that a model moved without moving
    /// the collider fails here rather than in a player's camp.
    const RACK_FRAME: (f32, f32) = (7.0 / 16.0, 9.5 / 16.0);

    /// **"У сушилки для кожи коллизия как у блока полного, хотя она
    /// меньше."**
    ///
    /// A rack is a square of poles standing on end, two and a half
    /// sixteenths deep in the middle of its cell -- and the collider
    /// built its own box out of `collision_height` and two literal
    /// ones, so it was a metre of stone in every direction. A tanner
    /// was stopped an arm's length short of a frame they could see
    /// straight through, and could not walk *along* a row of their own
    /// racks at all.
    ///
    /// The height is not what was wrong. The argument for the full cell
    /// is in `blocks.rs` and still stands: a frame is walked round, not
    /// stepped over. What this holds down is the footprint.
    #[test]
    fn a_tanner_walks_up_to_a_drying_rack_instead_of_stopping_a_cell_short() {
        use primitive_shared::types::{
            collision_depth, faced, Facing, BLOCK_DRYING_RACK,
        };

        let rack = faced(BLOCK_DRYING_RACK, Facing::North);
        assert_eq!(
            collision_depth(rack),
            Some(RACK_FRAME),
            "the frame moved and this test's arithmetic did not"
        );
        let chunks = floor_with(8, 8, rack);

        // Walking at the frame, from the cell in front of it.
        let mut player = Player::new((Vec3::new(8.5, 10.0, 6.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::Z, Vec3::Z, false, false, false, 1.0 / 60.0);
        }
        let stopped = player.position.z;
        let poles = 8.0 + RACK_FRAME.0 - PLAYER_HALF_WIDTH;
        assert!(
            (stopped - f64::from(poles)).abs() < 0.05,
            "stopped at z={stopped}; the poles are at {poles} and the cell wall at \
             {}, which is where a full block would have stopped them",
            8.0 - PLAYER_HALF_WIDTH,
        );
    }

    /// ...and the other half, which is the half a lazy fix would fail.
    ///
    /// Shrinking the box on *both* horizontal axes would pass the test
    /// above and still be wrong: what a player actually wants is to walk
    /// along the front of a row of racks, and that is a move *across*
    /// the frame's thin axis rather than into it. A frame that keeps a
    /// full-cell footprint on the wide axis is still a wall to that
    /// walk.
    #[test]
    fn a_row_of_drying_racks_can_be_walked_along_rather_than_only_faced() {
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK};

        let chunks = floor_with(8, 8, faced(BLOCK_DRYING_RACK, Facing::North));
        // Hugging the frame: the collider spans 7.75..8.35 on z, which
        // is inside the rack's own cell and clear of the poles at
        // 8.4375. Under a full-cell box this was a wall at x = 7.7.
        let mut player = Player::new((Vec3::new(5.5, 10.0, 8.05)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            player.position.x > 9.5,
            "walking along the rack got as far as x={} and no further",
            player.position.x,
        );
        assert!(
            (player.position.z - 8.05).abs() < 0.05,
            "walking along the rack pushed the player sideways to z={}",
            player.position.z,
        );
    }

    /// A rack turned a quarter turn turns the side you are stopped on.
    ///
    /// A box that ignored the facing would pass both tests above and
    /// still put the wall at right angles to the poles a player is
    /// looking at, which is worse than the full cell was: at least a
    /// full cell was wrong in a way that matched every rotation.
    #[test]
    fn turning_a_drying_rack_turns_the_side_a_player_is_stopped_on() {
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK};

        for (facing, thin) in [
            (Facing::North, Z),
            (Facing::East, X),
            (Facing::South, Z),
            (Facing::West, X),
        ] {
            let block = faced(BLOCK_DRYING_RACK, facing);
            let chunks = floor_with(8, 8, block);
            let wide = if thin == X { Z } else { X };
            // The near face read back rather than worked out here. The
            // frame stands at 7..9.5 of sixteen, which is not centred in
            // its cell, so a half turn shifts it half a sixteenth --
            // arithmetic worth having in one place and not two.
            let (bmin, _) =
                primitive_shared::geometry::block_box(block, 8, 10, 8).expect("a rack is solid");
            // Standing a hair off the frame's near face, walking on.
            let mut feet = Vec3::new(8.5, 10.0, 8.5);
            feet[thin] = bmin[thin] - PLAYER_HALF_WIDTH - 1e-3;
            assert!(
                sweep_axis(&chunks, &[], feet, 0.2, thin).blocked,
                "{facing:?}: walking into the poles was not stopped",
            );
            // ...and the same hair off the frame, walking along it.
            assert!(
                !sweep_axis(&chunks, &[], feet, 0.2, wide).blocked,
                "{facing:?}: walking along the poles was stopped",
            );
        }
    }

    #[test]
    fn ground_that_is_not_moving_never_lifts_anyone_more_than_a_step() {
        // "Проваливается в блоки и выскакивает на них."
        //
        // Snow gives way underfoot and springs back, which means the
        // ground under a player genuinely moves -- and a surface rising
        // under somebody is indistinguishable from a step up, so
        // `settle_onto_step` lifts them onto it. That lift is a
        // ratchet: each one raises the feet, and raised feet bring the
        // next thing along within the step budget. Walk the edge of a
        // drift and it climbs you out of the snow and onto the rock
        // beside it, half a metre at a time, over and over.
        //
        // The invariant that catches it: on ground that nobody is
        // editing, with no jump pressed, nothing may raise a player
        // further in one frame than they could step. A rise past that
        // is the escape hatch firing on ordinary terrain, which is
        // what being flung out of a drift actually was.
        let mut chunks = floor_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        // Drifts of every depth, half-block ledges, whole blocks and
        // holes, jumbled together: the drift edges are the case, and a
        // field of nothing but snow has no edges.
        for z in 0..16usize {
            for x in 0..16usize {
                match (x * 7 + z * 13) % 9 {
                    0..=3 => chunk.set(x, 10, z, BLOCK_SNOW),
                    4..=5 => chunk.set(x, 10, z, BLOCK_SAND),
                    6 => chunk.set(x, 10, z, BLOCK_STONE),
                    _ => {} // a hole down to the floor
                }
            }
        }
        chunks.insert(chunk);

        let mut rng = Wander(0xD1B5);
        for start in 0..8 {
            let mut player = Player::new(
                (Vec3::new(5.5 + start as f32, 14.0, 6.5)).as_dvec3(),
                DEFAULT_MOVE_SPEED,
            );
            for _ in 0..240 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            }
            let mut dir = Vec3::X;
            for frame in 0..900 {
                if frame % 9 == 0 {
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                if (player.position.x - 8.0).abs() > 6.0 || (player.position.z - 8.0).abs() > 6.0 {
                    let back = Vec3::new((8.0 - player.position.x) as f32, 0.0, (8.0 - player.position.z) as f32);
                    dir = back.normalize_or_zero();
                }
                let before = player.position.y;
                // Never jumping: every metre gained here was given to
                // the player by the collider rather than taken by them.
                player.update(&chunks, &[], dir, Vec3::X, false, false, false, 1.0 / 60.0);
                let rose = player.position.y - before;
                assert!(
                    rose <= f64::from(PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0),
                    "start {start}, frame {frame}: lifted {rose} blocks in one frame                      (a step is {PLAYER_STEP_HEIGHT}) to {:?}",
                    player.position
                );
            }
        }
    }

    #[test]
    fn a_seam_with_an_unloaded_chunk_beyond_it_moves_nobody() {
        // The regression that made this rule a rule: while a chunk is
        // still on its way, the collider spans loaded and unloaded
        // columns at once -- which happens every time anybody walks
        // across a seam. Nothing about that may move the player, and in
        // particular `escape_solids` must not treat the missing side as
        // something to be pushed out of.
        let mut chunks = ChunkManager::new(4);
        chunks.insert(course_chunk(ChunkPos::new(0, 0)));

        // Standing at the very edge of the loaded chunk, with the next
        // one not yet arrived.
        let mut player = Player::new((Vec3::new(15.9, 13.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let settled = player.position;
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            assert!(
                (player.position.x - settled.x).abs() < 1e-3
                    && (player.position.z - settled.z).abs() < 1e-3,
                "standing still at a seam moved the player from {settled:?} to {:?}",
                player.position
            );
        }
    }

    #[test]
    fn a_wall_that_appears_around_a_player_pushes_them_out_rather_than_swallowing_them() {
        // Terrain arrives late, another player builds, sand falls: a
        // solid block can appear where the player already is, and the
        // frame after it must not leave them able to walk *through* it.
        let mut chunks = floor_world();
        let mut player = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);

        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for y in 10..12 {
            chunk.set(8, y, 8, BLOCK_STONE);
        }
        chunks.insert(chunk);

        // Walk hard into where the block now is for a second.
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, 1.0 / 60.0);
        }
        // Being pushed out is fine; being inside it is not.
        let inside = penetration(&chunks, player.position.as_vec3());
        assert!(
            inside <= 0.6,
            "buried in a block that appeared around them: {inside} at {:?}",
            player.position
        );
    }
}

/// What the *server* makes of how this client moves.
///
/// A teleport in play is almost never the client moving someone: it is
/// the server deciding the client cheated and snapping it back. So the
/// invariant worth testing is not "does physics look sensible" but
/// **"does the anti-cheat accept what physics produces"** -- run against
/// the real `AntiCheat`, the real limits and the real world, because a
/// second copy of either would agree with itself and prove nothing.
#[cfg(test)]
mod anticheat_agreement_tests {
    use super::*;
    use primitive_server::logic::anticheat::{AntiCheat, Verdict};
    use primitive_server::logic::world::World;
    use primitive_server::settings::AntiCheatSettings;
    use primitive_shared::types::ChunkPos;

    /// The world, from the same generator, in both crates' terms.
    fn matching_worlds(seed: u32) -> (ChunkManager, World) {
        let generator = primitive_shared::worldgen::WorldGen::new(seed);
        let mut chunks = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        // The anti-cheat reads the *cached* world, so the chunks it
        // will ask about have to be in it -- inserted from the same
        // generator, so both sides are looking at the same ground.
        let server_world = World::new(seed, 64);
        for cx in -1..=1 {
            for cz in -1..=1 {
                server_world.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        (chunks, server_world)
    }

    /// **The client's clock, read off the wall the way the server's is.**
    ///
    /// These tests used to step a fixed three physics frames per message
    /// and sleep 50 ms between messages, which on a quiet machine is the
    /// same thing. Under `cargo test` with every core busy it is not: the
    /// sleep overshoots, by hundreds of milliseconds at a time, and the
    /// anti-cheat -- which times everything with `Instant::now()` -- saw a
    /// body stay in the air several times longer than the simulation had
    /// kept it there. A swimmer treading water at the surface for one
    /// simulated second was *hovering* for three real ones; the test went
    /// red with `W_HOVER`, blaming the physics for the scheduler, and green
    /// on the rerun.
    ///
    /// A real client has no such gap: its frame time is measured with the
    /// same clock the server reads. So the frames here are counted off it
    /// too. **A stall past a second is forgiven rather than repaid**: the
    /// anti-cheat's own `dt` is clamped to a second, and stepping the rest
    /// on the next message would move the player further in one update
    /// than its speed budget allows, which is a violation this helper
    /// invented rather than one the physics made.
    struct WallClock {
        last: std::time::Instant,
        /// Frames the wall clock has handed out and physics has not
        /// stepped yet, fractions included.
        owed: f32,
        stepped: usize,
    }

    impl WallClock {
        /// Started beside the anti-cheat, which is when its clock starts.
        fn start() -> Self {
            Self {
                last: std::time::Instant::now(),
                owed: 0.0,
                stepped: 0,
            }
        }

        /// Waits one send interval and hands back the physics frames that
        /// really passed, numbered on from where the last call stopped.
        /// Never empty, so a loop driven by it always moves.
        fn next_send(&mut self, frames_per_update: usize) -> std::ops::Range<usize> {
            std::thread::sleep(std::time::Duration::from_secs_f32(
                frames_per_update as f32 / 60.0,
            ));
            let now = std::time::Instant::now();
            self.owed += now.duration_since(self.last).as_secs_f32() * 60.0;
            self.last = now;
            let frames = (self.owed as usize).clamp(1, 60);
            self.owed = (self.owed - frames as f32).clamp(0.0, 1.0);
            let from = self.stepped;
            self.stepped += frames;
            from..self.stepped
        }

        fn stepped(&self) -> usize {
            self.stepped
        }
    }

    /// Walks a player around and hands every position to the anti-cheat
    /// the way the client would.
    fn walk_and_judge(seed: u32, wall_hugging: bool, seconds: f32) -> Vec<String> {
        let (chunks, world) = matching_worlds(seed);
        let start = {
            let generator = primitive_shared::worldgen::WorldGen::new(seed);
            Vec3::new(8.5, generator.height_at(8, 8) as f32 + 2.0, 8.5)
        };
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        // Every limit at its shipped value. The speed budget and the
        // rate limits are token buckets that refill against the wall
        // clock, so this test runs in **real time** -- a simulation
        // that ran a minute of walking in a millisecond would empty
        // every bucket and report violations that say nothing about how
        // the player moved.
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (f64::from(start.x), f64::from(start.y), f64::from(start.z)),
        );

        let mut complaints = Vec::new();
        let mut rng = super::solidity_tests::Wander(seed ^ 0xBEEF);
        let mut dir = Vec3::X;
        let mut sequence = 0u32;
        // The client sends its position at `player_update_hz`, not every
        // frame, so the anti-cheat sees one message per several frames --
        // and that is what its speed budget is calibrated against.
        let frames_per_update = 3;
        let frames = (seconds * 60.0) as usize;

        // The client sends at `player_update_hz` and the frame loop runs
        // at the frame rate; both are real time, and so is the budget
        // being tested -- see `WallClock`.
        let mut clock = WallClock::start();
        while clock.stepped() < frames {
            for frame in clock.next_send(frames_per_update) {
                if frame % 7 == 0 {
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                if wall_hugging {
                    // Straight at whatever is nearest, over and over: the
                    // case the complaint is about.
                    dir = Vec3::new(
                        (frame as f32 * 0.11).cos(),
                        0.0,
                        (frame as f32 * 0.11).sin(),
                    );
                }
                let jump = rng.next() > 0.9;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, true, 1.0 / 60.0);
            }
            sequence += 1;
            let verdict = anticheat.check_transform(
                player.position.x,
                player.position.y,
                player.position.z,
                player.grounded,
                sequence,
                &world,
            );
            match verdict {
                Verdict::Allow => {}
                Verdict::Reject { reason, .. } => complaints.push(reason),
                Verdict::Kick(reason) => complaints.push(format!("kick: {reason}")),
            }
        }
        complaints
    }

    /// **The three numbers the client and the server both have an
    /// opinion about, checked against each other.**
    ///
    /// Everything else in this module walks a player around and asks the
    /// real anti-cheat what it thought. That catches a disagreement in
    /// the shape of the movement; it cannot catch a disagreement in the
    /// *limits*, because a walk that never approaches one proves nothing
    /// about where it is. And the limits are exactly the thing written
    /// down twice: `move_speed` and `SPRINT_MULTIPLIER` live here,
    /// `max_horizontal_speed` lives in the server's settings file, and
    /// the only thing holding them together is a comment on
    /// [`SPRINT_MULTIPLIER`] saying what the other one is.
    ///
    /// Raise the sprint without raising the budget and every sprinting
    /// player on a server is rubber-banded -- and what they report is
    /// "it teleports me when I run", which names neither number. So the
    /// client's fastest honest move is *measured*, by running the real
    /// collider, and held against the server's own default.
    #[test]
    fn the_fastest_an_honest_client_can_move_fits_inside_the_servers_limits() {
        let limits = AntiCheatSettings::default();
        let chunks = super::tests::floor_world();

        // Flat out, on the flattest, grippiest thing there is.
        let mut runner = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..600 {
            runner.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, 1.0 / 60.0);
        }
        let sprint = runner.horizontal_speed();
        assert!(
            sprint < limits.max_horizontal_speed,
            "a sprint of {sprint} b/s is at or past the server's budget of {}",
            limits.max_horizontal_speed
        );

        // A jump is a climb with no ground under it, which is the shape
        // of the flight cheat: it has to fit inside the run the server
        // allows before it calls the client a liar.
        let mut jumper = Player::new((Vec3::new(8.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        jumper.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        let floor = jumper.position.y;
        jumper.update(&chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
        let mut apex = jumper.position.y;
        for _ in 0..600 {
            jumper.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            apex = apex.max(jumper.position.y);
            if jumper.grounded {
                break;
            }
        }
        let climb = apex - floor;
        assert!(
            climb < f64::from(limits.max_airborne_ascent),
            "a jump climbs {climb} blocks against an allowance of {}",
            limits.max_airborne_ascent
        );

        // ...and the fastest a body ever falls, which is a constant here
        // and a limit there.
        assert!(
            TERMINAL_VELOCITY.abs() < limits.max_vertical_speed,
            "terminal velocity {} is past the server's vertical limit of {}",
            TERMINAL_VELOCITY.abs(),
            limits.max_vertical_speed
        );
    }

    #[test]
    fn ordinary_movement_is_never_mistaken_for_cheating() {
        for seed in [1337u32, 2024] {
            let complaints = walk_and_judge(seed, false, 1.5);
            assert!(
                complaints.is_empty(),
                "seed {seed}: the server would have corrected an ordinary player: {complaints:?}"
            );
        }
    }

    #[test]
    fn running_into_walls_is_never_mistaken_for_cheating() {
        // The reported symptom, in the form the server sees it: a player
        // pressed into terrain, over and over, from every angle.
        for seed in [1337u32, 2024] {
            let complaints = walk_and_judge(seed, true, 1.5);
            assert!(
                complaints.is_empty(),
                "seed {seed}: running into walls got the player corrected: {complaints:?}"
            );
        }
    }

    /// A stone floor to y = 10 with `block` standing on the far half of
    /// it, in both crates' terms.
    ///
    /// Half rather than all of it because the case worth testing is the
    /// *transition*: a player who starts on top of the thing never asks
    /// the collider to lift them onto it, and lifting is the half of
    /// this the anti-cheat has an opinion about.
    fn matching_ledge(block: primitive_shared::types::BlockId) -> (ChunkManager, World) {
        use primitive_shared::types::{Chunk, CHUNK_VOLUME};
        let mut blocks = vec![primitive_shared::types::BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..=10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = primitive_shared::types::BLOCK_STONE;
                }
            }
        }
        for z in 0..16 {
            for x in 8..16 {
                blocks[Chunk::index(x, 11, z)] = block;
            }
        }
        let mut chunks = ChunkManager::new(4);
        let world = World::new(1, 64);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let chunk = Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: blocks.clone(),
                };
                chunks.insert(chunk.clone());
                world.insert(chunk);
            }
        }
        (chunks, world)
    }

    /// Walks east off the bare floor and onto the ledge, reporting
    /// whatever the server made of it.
    fn walk_onto_ledge(block: primitive_shared::types::BlockId) -> Vec<String> {
        let (chunks, world) = matching_ledge(block);
        let start = Vec3::new(2.5, 11.0, 8.5);
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (f64::from(start.x), f64::from(start.y), f64::from(start.z)),
        );

        let mut complaints = Vec::new();
        let mut sequence = 0u32;
        let frames_per_update = 3;
        // Two seconds at a walk: five and a half blocks to the ledge
        // against the eight and a half a walk covers, which leaves room
        // for the acceleration at the start without leaving so much
        // that a stuck player looks like a slow one.
        // Real time, for the same reason `walk_and_judge` runs in it: the
        // speed budget is a token bucket refilling against the wall clock.
        let mut clock = WallClock::start();
        while clock.stepped() < 120 {
            for _ in clock.next_send(frames_per_update) {
                // Straight at the ledge and over it, never jumping: a jump
                // would clear the lip on its own and prove nothing about
                // the step.
                player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, 1.0 / 60.0);
            }
            sequence += 1;
            match anticheat.check_transform(
                player.position.x,
                player.position.y,
                player.position.z,
                player.grounded,
                sequence,
                &world,
            ) {
                Verdict::Allow => {}
                Verdict::Reject { reason, .. } => complaints.push(reason),
                Verdict::Kick(reason) => complaints.push(format!("kick: {reason}")),
            }
        }
        assert!(
            player.position.x > 8.0,
            "the player never reached the ledge (x = {}), so nothing was tested",
            player.position.x
        );
        complaints
    }

    #[test]
    fn stepping_onto_a_part_height_block_is_never_mistaken_for_flying() {
        // **The two blocks in the world that are not a whole cell**, and
        // the exact case the anti-cheat's four-corner ground probe was
        // written for. Stepping up, the client raises the player before
        // gravity has anything to say about it: for a frame they are
        // above the floor they left, moving upward, and claiming to be
        // grounded -- which is what a flight cheat looks like from the
        // server's side.
        //
        // Nothing tested it. The probe was fixed on the strength of an
        // argument, and an argument is what this replaces.
        // **Only what a step can actually climb.** The list is derived
        // from the block table, and the table has furniture in it now:
        // a table is six eighths tall, which is above
        // `PLAYER_STEP_HEIGHT` and therefore something a player walks
        // *into* rather than onto -- correctly, and the same as a wall.
        // Walking at one and asserting nothing went wrong tested the
        // wall, not the step, and the guard inside `walk_onto_ledge`
        // said so out loud.
        for (block, name, height) in super::tests::part_height_blocks() {
            if height > primitive_shared::geometry::PLAYER_STEP_HEIGHT {
                continue;
            }
            let complaints = walk_onto_ledge(block);
            assert!(
                complaints.is_empty(),
                "walking onto a {name} got the player corrected: {complaints:?}"
            );
        }
    }

    #[test]
    fn jumping_out_of_the_water_is_never_mistaken_for_flying() {
        // The clamber onto a bank and the jump off a ford's bottom are both
        // a rise that goes on after the water ends, with no ground claimed
        // -- the flight check's own signature -- and the server's copy of
        // "is there water here" has to carry them to where a jump would.
        // A ford a cell deep with a bank a block over its floor, and water
        // two and ten deep with one a block over the water, waded and swum at
        // with jump held.
        use primitive_shared::types::{Chunk, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};
        for (depth, bank, start) in [(1usize, 11usize, Vec3::new(8.5, 10.0, 8.5)), (2, 13, Vec3::new(8.5, 10.0, 8.5)), (10, 21, Vec3::new(8.0, 15.0, 8.5))] {
            let mut blocks = vec![primitive_shared::types::BLOCK_AIR; CHUNK_VOLUME];
            for y in 0..bank {
                for z in 0..16 {
                    for x in 0..16 {
                        let shore = x >= 10 && y < bank;
                        let id = if y < 10 || shore {
                            BLOCK_STONE
                        } else if y < 10 + depth {
                            BLOCK_WATER
                        } else {
                            continue;
                        };
                        blocks[Chunk::index(x, y, z)] = id;
                    }
                }
            }
            let mut chunks = ChunkManager::new(4);
            let world = World::new(1, 64);
            for cx in -1..=1 {
                for cz in -1..=1 {
                    let chunk = Chunk { pos: ChunkPos::new(cx, cz), blocks: blocks.clone() };
                    chunks.insert(chunk.clone());
                    world.insert(chunk);
                }
            }
            let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
            let mut anticheat =
                AntiCheat::new(AntiCheatSettings::default(), 8, (f64::from(start.x), f64::from(start.y), f64::from(start.z)));
            let mut complaints = Vec::new();
            let mut sequence = 0u32;
            let mut frame = 0usize;
            let mut stood = false;
            let mut clock = WallClock::start();
            while clock.stepped() < 360 {
                for _ in clock.next_send(3) {
                    player.update(&chunks, &[], Vec3::X, Vec3::X, frame.is_multiple_of(30) || depth > 1, true, false, 1.0 / 60.0);
                    frame += 1;
                    stood |= player.grounded && (player.position.y - bank as f64).abs() < 0.01;
                }
                sequence += 1;
                match anticheat.check_transform(player.position.x, player.position.y, player.position.z, player.grounded, sequence, &world) {
                    Verdict::Allow => {}
                    Verdict::Reject { reason, .. } => complaints.push(format!("at {:?}: {reason}", player.position)),
                    Verdict::Kick(reason) => complaints.push(format!("kick at {:?}: {reason}", player.position)),
                }
            }
            assert!(stood, "{depth} deep: never got out onto the bank, ended at {:?}", player.position);
            assert!(complaints.is_empty(), "{depth} deep: getting out of the water was corrected: {complaints:?}");
        }
    }

    /// A stone floor to y = 9 with water over it to y = 19, in both
    /// crates' terms.
    fn matching_lake() -> (ChunkManager, World) {
        use primitive_shared::types::{Chunk, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};
        let mut blocks = vec![primitive_shared::types::BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..primitive_shared::types::CHUNK_SIZE_Y {
            let id = if y < 10 {
                BLOCK_STONE
            } else if y < 20 {
                BLOCK_WATER
            } else {
                primitive_shared::types::BLOCK_AIR
            };
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = id;
                }
            }
        }
        let mut chunks = ChunkManager::new(4);
        let world = World::new(1, 64);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let chunk = Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: blocks.clone(),
                };
                chunks.insert(chunk.clone());
                world.insert(chunk);
            }
        }
        (chunks, world)
    }

    #[test]
    fn swimming_up_a_lake_is_never_mistaken_for_flying() {
        // **Rising is what a cheat does and what a swimmer does**, and
        // the only thing telling them apart is that both sides agree
        // there is water here. The client decides that from the deepest
        // water anywhere inside the collider; the server decides it from
        // three sample heights up the same column. Two rules for one
        // question, and nothing held them together -- every test in this
        // module walked.
        //
        // A disagreement here does not look like a bug. It looks like
        // lag: the swimmer is snapped back down, once, somewhere near
        // the surface, and only sometimes.
        let (chunks, world) = matching_lake();
        // Feet on the bed, head well under: the whole climb, surfacing
        // included, is what has to survive.
        let start = Vec3::new(8.5, 10.0, 8.5);
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (f64::from(start.x), f64::from(start.y), f64::from(start.z)),
        );

        let mut complaints = Vec::new();
        let mut sequence = 0u32;
        let frames_per_update = 3;
        let mut highest = start.y;
        let mut clock = WallClock::start();
        while clock.stepped() < 300 {
            for _ in clock.next_send(frames_per_update) {
                // Holding jump, which is how you swim upward, and pressing
                // forward so the hover check's horizontal requirement is
                // met -- a swimmer treading water in one spot is a case the
                // server deliberately leaves alone.
                player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, true, 1.0 / 60.0);
                highest = highest.max(player.position.y as f32);
            }
            sequence += 1;
            match anticheat.check_transform(
                player.position.x,
                player.position.y,
                player.position.z,
                player.grounded,
                sequence,
                &world,
            ) {
                Verdict::Allow => {}
                Verdict::Reject { reason, .. } => {
                    complaints.push(format!("at y={:.2}: {reason}", player.position.y))
                }
                Verdict::Kick(reason) => {
                    complaints.push(format!("kick at y={:.2}: {reason}", player.position.y))
                }
            }
        }
        assert!(
            highest > 17.0,
            "the swimmer never climbed (highest y = {highest}), so nothing was tested"
        );
        assert!(
            complaints.is_empty(),
            "swimming up got the player corrected: {complaints:?}"
        );
    }

    #[test]
    fn climbing_out_of_a_lake_onto_a_shore_is_never_mistaken_for_flying() {
        // **A new way to rise is a new thing for the anti-cheat to
        // dislike.** The ledge climb used to stop where `swimming` did,
        // which is under the surface; it now carries the player up to
        // the waterline and a little past it on the coast (see
        // `Player::afloat`). Those last few frames are the dangerous
        // ones: the client is rising, is not on the ground, and the
        // server's own water samples are about to stop finding water.
        //
        // A correction here would look like the lake spitting the player
        // back in, once, at the last moment, and only sometimes -- which
        // is the shape of complaint that takes a week to place.
        let (chunks, world) = matching_lake();
        let (chunks, world) = {
            use primitive_shared::types::{ChunkPos, BLOCK_STONE};
            // The same shore both tests use: solid to the waterline at
            // x >= 10, in both crates' terms.
            let mut chunks = chunks;
            let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
            for x in 10..16 {
                for z in 0..16 {
                    for y in 0..20 {
                        chunk.set(x, y, z, BLOCK_STONE);
                    }
                }
            }
            chunks.insert(chunk.clone());
            world.insert(chunk.unpack());
            (chunks, world)
        };

        let start = Vec3::new(8.5, 15.0, 8.5);
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (f64::from(start.x), f64::from(start.y), f64::from(start.z)),
        );

        let mut complaints = Vec::new();
        let mut sequence = 0u32;
        let frames_per_update = 3;
        let mut escaped = false;
        let mut clock = WallClock::start();
        while clock.stepped() < 300 {
            for _ in clock.next_send(frames_per_update) {
                player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, false, 1.0 / 60.0);
                escaped |= player.grounded && !player.in_water;
            }
            sequence += 1;
            match anticheat.check_transform(
                player.position.x,
                player.position.y,
                player.position.z,
                player.grounded,
                sequence,
                &world,
            ) {
                Verdict::Allow => {}
                Verdict::Reject { reason, .. } => {
                    complaints.push(format!("at y={:.2}: {reason}", player.position.y))
                }
                Verdict::Kick(reason) => {
                    complaints.push(format!("kick at y={:.2}: {reason}", player.position.y))
                }
            }
        }
        assert!(
            escaped,
            "the player never got out of the water, so nothing was tested"
        );
        assert!(
            complaints.is_empty(),
            "climbing out of the lake got the player corrected: {complaints:?}"
        );
    }

    #[test]
    fn floating_at_the_surface_is_never_mistaken_for_hovering() {
        // The other half, and the one the hover check can reach: a
        // swimmer who has arrived at the surface stops climbing, keeps
        // moving horizontally, and has nothing but water underneath --
        // airborne, level, and over air, which is three of the four
        // things `W_HOVER` is looking for. The fourth is that the server
        // still sees water at one of its sample heights, and a swimmer
        // floats with most of the body *out* of it.
        let (chunks, world) = matching_lake();
        let start = Vec3::new(8.5, 18.0, 8.5);
        let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (f64::from(start.x), f64::from(start.y), f64::from(start.z)),
        );

        // Long enough to pass `max_hover_seconds` several times over,
        // because a hover flag needs the clock as well as the geometry.
        let seconds = AntiCheatSettings::default().max_hover_seconds * 3.0 + 1.0;
        let updates = (seconds * 20.0) as usize;
        let mut complaints = Vec::new();
        let mut sequence = 0u32;
        let mut clock = WallClock::start();
        while clock.stepped() < updates * 3 {
            for _ in clock.next_send(3) {
                player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, true, 1.0 / 60.0);
            }
            sequence += 1;
            match anticheat.check_transform(
                player.position.x,
                player.position.y,
                player.position.z,
                player.grounded,
                sequence,
                &world,
            ) {
                Verdict::Allow => {}
                Verdict::Reject { reason, .. } => {
                    complaints.push(format!("at y={:.2}: {reason}", player.position.y))
                }
                Verdict::Kick(reason) => {
                    complaints.push(format!("kick at y={:.2}: {reason}", player.position.y))
                }
            }
        }
        assert!(
            complaints.is_empty(),
            "floating at the surface got the player corrected: {complaints:?}"
        );
    }


    #[test]
    fn a_part_height_block_is_stood_on_at_its_own_height() {
        // What the step is *for*, and the number both sides have to
        // agree about: the collider rests on the top of the box, not on
        // the top of the cell. If these ever came apart the player would
        // stand a quarter of a block inside a campfire or a half block
        // above it, and the server would see a position its own ground
        // probe could not explain.
        for (block, name, height) in super::tests::part_height_blocks() {
            let (chunks, _) = matching_ledge(block);
            let mut player = Player::new((Vec3::new(12.5, 13.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            for _ in 0..90 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, true, 1.0 / 60.0);
            }
            assert!(
                (player.position.y - f64::from(11.0 + height)).abs() < 0.05,
                "a player dropped onto a {name} settled at {} rather than {}",
                player.position.y,
                11.0 + height
            );
            assert!(player.grounded, "...and did not think they had landed");
        }
    }
}

/// Snow still slows you down -- that part was never a layer.
#[cfg(test)]
mod snow_tests {
    use super::tests::floor_world;
    use super::*;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_SNOW};

    /// A floor made entirely of one block at y=10, over the usual stone.
    fn field_of(block: primitive_shared::types::BlockId) -> ChunkManager {
        let mut chunks = floor_world();
        let mut chunk = chunks
            .get(primitive_shared::types::ChunkPos::new(0, 0))
            .unwrap()
            .clone();
        for z in 0..16 {
            for x in 0..16 {
                chunk.set(x, 10, z, block);
            }
        }
        chunks.insert(chunk);
        chunks
    }

    /// How far the player gets in two seconds of walking.
    fn distance_in_two_seconds(chunks: &ChunkManager) -> f32 {
        let mut player = Player::new((Vec3::new(2.5, 13.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let start = player.position.x;
        for _ in 0..120 {
            player.update(chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        (player.position.x - start) as f32
    }

    #[test]
    fn walking_over_snow_is_slower_than_walking_over_sand() {
        // `types::surface_drag`, which is about what the surface is
        // rather than how deep it is -- so it outlived the layers.
        let over_snow = distance_in_two_seconds(&field_of(BLOCK_SNOW));
        let over_sand = distance_in_two_seconds(&field_of(BLOCK_SAND));
        assert!(
            over_snow < over_sand * 0.8,
            "snow ({over_snow}) barely slowed anyone against sand ({over_sand})"
        );
        assert!(over_snow > over_sand * 0.4, "snow read as being stuck");
    }

    #[test]
    fn a_field_of_snow_is_a_floor_at_a_whole_block() {
        // The point of the removal: snow is a block. You stand on top
        // of it, at a whole number, and you do not sink into it.
        let chunks = field_of(BLOCK_SNOW);
        let mut player = Player::new((Vec3::new(8.5, 14.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..300 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(player.grounded, "never landed");
        assert!(
            (player.position.y - 11.0).abs() < 0.01,
            "came to rest at {} rather than on top of the snow",
            player.position.y
        );
    }
}

/// A wall-clock measurement of the two things physics does every frame.
///
/// Same shape as the mesher's benchmark and run the same way -- an
/// ignored test rather than a nightly `#[bench]` or a criterion
/// dependency:
///
/// ```text
/// cargo test --release -p primitive_client --lib \
///     -- --ignored --nocapture bench
/// ```
///
/// Both of these are small next to meshing, and both are paid on the
/// main thread every single frame, several times: the interaction ray is
/// cast for mining, for placing and for checking whether a punch would
/// land through a wall, and the collider is swept once per axis.
#[cfg(test)]
mod bench {
    use super::*;
    use primitive_shared::types::ChunkPos;
    use primitive_shared::worldgen::WorldGen;
    use std::time::Instant;

    /// Nine chunks of real terrain around the origin, which is what the
    /// collider and the ray actually run against.
    fn world() -> ChunkManager {
        let generator = WorldGen::new(1337);
        let mut chunks = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        chunks
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn bench_physics() {
        const ROUNDS: usize = 20_000;
        const BATCHES: usize = 7;

        let chunks = world();
        let generator = WorldGen::new(1337);
        let ground = generator.height_at(8, 8) as f32 + 1.0;

        // The fastest batch rather than the mean: interruptions only
        // ever make a batch slower, so the minimum is the closest thing
        // to the cost of the code itself.
        let time = |rounds: usize, f: &mut dyn FnMut()| {
            let mut best = f64::MAX;
            for _ in 0..BATCHES {
                let started = Instant::now();
                for _ in 0..rounds {
                    f();
                }
                best = best.min(started.elapsed().as_secs_f64() * 1e6 / rounds as f64);
            }
            best
        };

        // A ray at the angle a player actually looks at the ground, from
        // eye height, over the six blocks of interaction range.
        let eye = Vec3::new(8.5, ground + EYE_HEIGHT, 8.5);
        let dir = Vec3::new(0.4, -0.7, 0.35).normalize();
        let per_ray = time(ROUNDS, &mut || {
            std::hint::black_box(raycast_block(&chunks, eye.as_dvec3(), dir, 6.0));
        });

        // A frame of movement: three swept axes plus the settle pass and
        // the fluid checks, walking forward on the ground.
        let mut player = Player::new((Vec3::new(8.5, ground, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let start = player.position;
        let per_update = time(ROUNDS, &mut || {
            player.position = start;
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        });

        // **The same frame without the frame of reference**, as `update` was
        // before positions were `f64`: the collider run straight against the
        // world, with the feet an `f32` in it. Near zero that is the same
        // arithmetic, so the difference is the whole cost of `Local`.
        let at = player.position.as_vec3();
        let per_step = time(ROUNDS, &mut || {
            player.at = at;
            player.step(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        });
        println!("
without the local frame  {per_step:.2} us/update");
        println!("\nray      {per_ray:.2} us/cast   (x3 per frame)");
        println!("collide  {per_update:.2} us/update (once per frame)");
    }
}

#[cfg(test)]
mod water_tests {
    use super::tests::*;
    use super::*;

    #[test]
    fn you_do_not_walk_on_water() {
        // Regression: water used to be collidable, so the surface of a
        // lake behaved like a solid floor.
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(8.0, 25.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            player.position.y < 20.0,
            "player stayed on the surface at y={}",
            player.position.y
        );
        assert!(player.in_water, "should be in the water by now");
    }

    #[test]
    fn you_sink_slowly_rather_than_falling() {
        let chunks = lake_world();
        let mut swimmer = Player::new((Vec3::new(8.0, 18.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut faller = Player::new((Vec3::new(8.0, 18.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let air = floor_world();

        for _ in 0..30 {
            swimmer.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            faller.update(&air, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            swimmer.position.y > faller.position.y,
            "sinking ({}) should be slower than falling ({})",
            swimmer.position.y,
            faller.position.y
        );
        // **The property, rather than the constant that used to
        // enforce it.** There is no cap on sinking any more -- see the
        // note where `WATER_SINK_SPEED` was -- so what is checked is
        // what the cap was there for: water is not air, and a body in
        // it is going nowhere near as fast.
        assert!(
            swimmer.velocity.y.abs() < faller.velocity.y.abs() * 0.5,
            "sinking at {} against falling at {}",
            swimmer.velocity.y,
            faller.velocity.y
        );
    }

    /// Runs a player for `seconds` with the given input.
    fn run(
        chunks: &ChunkManager,
        player: &mut Player,
        wish: Vec3,
        look: Vec3,
        jump: bool,
        seconds: f32,
    ) {
        for _ in 0..(seconds * 60.0) as usize {
            player.update(chunks, &[], wish, look, false, jump, false, 1.0 / 60.0);
        }
    }

    #[test]
    fn a_swimmer_left_alone_floats_with_their_head_out() {
        // **The one that matters.** Doing nothing used to sink you until
        // you drowned, so staying alive in a lake meant holding jump for
        // as long as you were in it.
        let chunks = lake_world(); // water to the top of y = 19
        let surface = 19.0 + primitive_shared::fluid::surface_height(
            primitive_shared::types::BLOCK_WATER,
        );

        // From well under, and from a fall in: both end up at the same
        // place, because it is an equilibrium and not a starting state.
        for start in [12.0f32, 25.0] {
            let mut player = Player::new((Vec3::new(8.0, start, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            run(&chunks, &mut player, Vec3::ZERO, Vec3::X, false, 15.0);

            let submersion = surface - player.position.y as f32;
            assert!(
                (submersion - FLOAT_SUBMERSION).abs() < 0.15,
                "from {start} it settled {submersion} deep, not {FLOAT_SUBMERSION}"
            );
            assert!(!player.submerged, "a floating player must be able to breathe");
            assert!(player.swimming, "...while still being in the water");
        }
    }

    #[test]
    fn a_shallow_puddle_is_walked_through_rather_than_swum_in() {
        // Regression, and the reason `submersion` is a number rather
        // than three yes-or-nos: a cell of water can hold an eighth now,
        // and an eighth-deep film used to put the player into swimming
        // mode -- a third of their speed, no friction underfoot, and the
        // jump key swimming them upward out of the puddle.
        use primitive_shared::types::{with_layers, BLOCK_WATER};
        let ankle_deep = floor_with(8, 8, with_layers(BLOCK_WATER, 1));
        let mut player = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        player.refresh_fluid_state_here(&ankle_deep);

        assert!(player.in_water, "the feet are in it");
        assert!(!player.swimming, "an eighth of a block is not something to swim in");

        // ...and it is still walking: on the ground, and a jump is a
        // jump rather than a stroke.
        run(&ankle_deep, &mut player, Vec3::ZERO, Vec3::X, false, 0.5);
        assert!(player.grounded, "a wading player stands on the bottom");
        player.update(&ankle_deep, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
        assert!(player.jumped, "the jump key must still jump in a puddle");
    }

    #[test]
    fn one_cell_of_water_is_a_ford_and_two_is_a_lake() {
        // **The boundary the whole wading regime turns on, and it is
        // two centimetres wide.** A player standing on the floor of a
        // cell of water is `1.0 - fluid::SURFACE_DROP` = 0.88 deep in
        // it, whatever the simulation thinks that cell's depth is --
        // every cell of water is drawn and collided at one height, on
        // purpose (see `fluid::surface_height`). `SWIM_DEPTH` is 0.90.
        //
        // So six hundredths of a block of clearance is all that keeps a
        // stream from being something you have to swim across. The
        // arithmetic of that is asserted where the constant is, at
        // compile time (see the `const _` under `SWIM_DEPTH`); what is
        // here is the same statement made through the collider, against
        // a floor of real water rather than against three numbers.
        use primitive_shared::types::BLOCK_WATER;
        let one_deep = flooded_floor(1);
        let mut wader = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        run(&one_deep, &mut wader, Vec3::ZERO, Vec3::X, false, 0.5);
        assert!(wader.in_water && !wader.swimming, "one block of water is a ford");
        assert!(wader.grounded, "a wading player stands on the bottom");

        let two_deep = flooded_floor(2);
        let mut swimmer = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
        swimmer.refresh_fluid_state_here(&two_deep);
        assert!(swimmer.swimming, "two blocks of water is a lake");

        // The depth in the cell is not what decides it, and must not
        // start being: the thin end of a spill is a ford exactly like
        // the full cell beside it, because that is how both are drawn.
        for layers in 1..=primitive_shared::fluid::SOURCE_DEPTH {
            let chunks = floor_with(8, 8, primitive_shared::types::with_layers(BLOCK_WATER, layers));
            let mut player = Player::new((Vec3::new(8.5, 10.0, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            player.refresh_fluid_state_here(&chunks);
            assert!(
                !player.swimming,
                "a cell {layers} eighths deep read as somewhere to swim"
            );
        }
    }

    /// The stone floor of `floor_world` with `cells` of water standing
    /// on it, everywhere a walker can reach.
    fn flooded_floor(cells: usize) -> ChunkManager {
        use primitive_shared::types::{ChunkPos, BLOCK_WATER};
        let mut chunks = floor_world();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut chunk = chunks.get(ChunkPos::new(cx, cz)).unwrap().clone();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 10..10 + cells {
                            chunk.set(x, y, z, BLOCK_WATER);
                        }
                    }
                }
                chunks.insert(chunk);
            }
        }
        chunks
    }

    #[test]
    fn a_diver_is_lifted_the_same_however_far_into_a_cell_their_feet_are() {
        // **The client's copy of the twelve per cent at the top of a
        // submerged cell**, which the server's drowning check and the
        // anti-cheat had already closed and this had not. A full cell of
        // water stops `SURFACE_DROP` short of its ceiling, but that drop
        // belongs to the cell where the *air* starts -- so a cell with
        // more water above it is wet to the top of itself, which is what
        // `fluid::surface_height_with_above` says.
        //
        // Asking `surface_height` instead put the surface an eighth of a
        // block low, and because the answer is capped at the player's own
        // height the error showed up only sometimes: submersion swung
        // between 1.68 and 1.80 with nothing but where in a cell the
        // feet happened to be. Buoyancy is a spring in that number (see
        // `BUOYANCY`), so a diver's lift pulsed between 2.4 and 4.0
        // blocks a second squared once every block they rose, and the
        // drag pulsed with it. What is left of the margin matters too:
        // 1.68 is only six hundredths clear of `EYE_HEIGHT`, so the
        // underwater fog and the muffled sound were that far from
        // flickering on and off a metre under the surface.
        let chunks = lake_world(); // water fills y = 10..=19
        for step in 0..40 {
            let feet = 13.0 + step as f32 / 40.0;
            let mut player = Player::new((Vec3::new(8.5, feet, 8.5)).as_dvec3(), DEFAULT_MOVE_SPEED);
            player.refresh_fluid_state_here(&chunks);
            assert_eq!(
                player.submersion, PLAYER_HEIGHT,
                "a player at {feet}, three blocks under the surface, read as \
                 {} submerged out of {PLAYER_HEIGHT}",
                player.submersion,
            );
            assert!(player.submerged, "...and could see out of the water at {feet}");
        }
    }

    #[test]
    fn wading_is_slower_than_walking_and_faster_than_swimming() {
        // The three regimes in one line, and the order is the whole
        // point: crossing a ford must not be either as quick as the path
        // beside it or as slow as the lake beyond it.
        use primitive_shared::types::{with_layers, BLOCK_WATER};
        let distance = |chunks: &ChunkManager, start: f32| {
            let mut player = Player::new((Vec3::new(2.0, start, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            run(chunks, &mut player, Vec3::X, Vec3::X, false, 1.5);
            player.position.x - 2.0
        };

        let dry = distance(&floor_world(), 10.0);
        // Knee deep, everywhere the player might walk.
        let mut ford = floor_world();
        let mut chunk = ford.get(primitive_shared::types::ChunkPos::new(0, 0)).unwrap().clone();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 10, z, with_layers(BLOCK_WATER, 5));
            }
        }
        ford.insert(chunk);
        let waded = distance(&ford, 10.0);
        let swum = distance(&lake_world(), 15.0);

        assert!(waded < dry, "wading ({waded}) was as quick as walking ({dry})");
        assert!(waded > swum, "wading ({waded}) was as slow as swimming ({swum})");
    }

    #[test]
    fn a_stroke_carries_you_on_after_you_stop_pressing() {
        // Water has inertia. Velocity used to be *assigned* from the
        // input, so a swimmer reached full speed and stopped dead inside
        // one frame -- the exact thing ground movement was rewritten to
        // stop doing, in the one place where it is least believable.
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(2.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        run(&chunks, &mut player, Vec3::X, Vec3::X, false, 2.0);
        let cruising = player.velocity.x;
        assert!(cruising > 0.5, "never got going: {cruising}");

        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(player.velocity.x < cruising, "the water did not slow them");
        assert!(player.velocity.x > 0.0, "stopped dead in a single frame");

        // **The glide, which is the property this model was rewritten
        // for.** A second after letting go a swimmer is still moving at
        // a quarter of a block a second or better. Under the old
        // exponential law -- ninety-eight per cent gone every second --
        // they were down to four hundredths by now, which is a dead
        // stop dressed up as drag.
        run(&chunks, &mut player, Vec3::ZERO, Vec3::X, false, 1.0);
        assert!(
            player.velocity.x > 0.25,
            "the coast lasted no time at all: {}",
            player.velocity.x
        );

        // ...and it does end. A quadratic law on its own decays as
        // `1/t` and never actually stops, which is why there is a
        // linear term beside it -- see `WATER_LINEAR_SIDEWAYS`.
        run(&chunks, &mut player, Vec3::ZERO, Vec3::X, false, 4.0);
        assert!(
            player.velocity.x.abs() < 0.05,
            "never came to rest: {}",
            player.velocity.x
        );
    }

    /// **How deep you go is how fast you arrived.** Under the old model
    /// entry was one multiply -- `velocity.y *= 0.25` on the frame the
    /// water was touched -- so stepping off a kerb and falling fifty
    /// blocks put you at the same depth. That is a number, not a splash.
    ///
    /// A v-squared law gives this for nothing: the speed decays as
    /// `e^(-k x)`, so the distance travelled scales with the log of the
    /// entry speed. It is not much deeper for a much longer fall, which
    /// is also what water does.
    #[test]
    fn falling_further_puts_you_deeper() {
        let chunks = lake_world();
        // The surface is at 20; drop from well above it so the fall has
        // somewhere to build up.
        let depth_after = |from: f32| {
            let mut player = Player::new((Vec3::new(8.0, from, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            let mut deepest = from;
            for _ in 0..240 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 120.0);
                deepest = deepest.min(player.position.y as f32);
            }
            20.0 - deepest
        };

        let stepped = depth_after(21.0);
        let dropped = depth_after(40.0);
        assert!(
            dropped > stepped + 0.4,
            "a fifty-block fall ({dropped:.2}) went no deeper than a step ({stepped:.2})"
        );
        // ...and it is still a lake rather than a hole: the bed is at
        // 10, and nothing may reach it by falling.
        assert!(dropped < 9.0, "a fall carried through {dropped:.2} blocks of water");
    }

    /// Drag is in proportion to how much of the body is in the water, so
    /// walking out of a lake is continuous rather than a cliff at the
    /// waist.
    #[test]
    fn half_a_body_in_the_water_gets_half_the_water() {
        let resistance = |submersion: f32| {
            let mut player = Player::new((Vec3::new(8.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            player.submersion = submersion;
            player.velocity = Vec3::new(3.0, 0.0, 0.0);
            player.apply_water_drag(1.0 / 60.0);
            3.0 - player.velocity.x
        };
        let deep = resistance(PLAYER_HEIGHT);
        let half = resistance(PLAYER_HEIGHT * 0.5);
        assert!(deep > 0.0, "fully under the water and nothing slowed it");
        assert!(
            (half / deep - 0.5).abs() < 0.02,
            "half a body took {:.0}% of the drag rather than half",
            half / deep * 100.0
        );
    }

    #[test]
    fn looking_down_and_swimming_forward_is_how_you_dive() {
        // There is no crouch key, so this is the only way down there is
        // -- and without it buoyancy would make the bottom of a lake
        // unreachable.
        let chunks = lake_world();
        let steep = Vec3::new(0.17, -0.985, 0.0).normalize();
        let mut diver = Player::new((Vec3::new(8.0, 18.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut floater = Player::new((Vec3::new(8.0, 18.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);

        run(&chunks, &mut diver, Vec3::X, steep, false, 3.0);
        run(&chunks, &mut floater, Vec3::ZERO, Vec3::X, false, 3.0);

        assert!(
            diver.position.y < floater.position.y - 3.0,
            "diving ({}) went nowhere against floating ({})",
            diver.position.y,
            floater.position.y
        );
        assert!(diver.position.y > 10.0, "dived through the lake bed");

        // ...and strafing is level however steeply you look, or every
        // sideways stroke would be a dive.
        let mut strafer = Player::new((Vec3::new(8.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let before = strafer.position.y;
        run(&chunks, &mut strafer, Vec3::Z, steep, false, 1.0);
        assert!(
            strafer.position.y > before - 0.5,
            "a strafe dived: {} to {}",
            before,
            strafer.position.y
        );
    }

    #[test]
    fn swimming_into_a_bank_climbs_out_of_the_water() {
        // A swimmer is never grounded, so the step-up that walks a
        // player over a kerb never fires for them: the bank of a river
        // was a wall, and the only way out was to hold jump until you
        // cleared the top and then swim forward.
        //
        // A lake with a solid shelf standing in it at x >= 10, whose top
        // is above the water.
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_STONE};
        let mut chunks = lake_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for x in 10..16 {
            for z in 0..16 {
                for y in 0..=20 {
                    chunk.set(x, y, z, BLOCK_STONE);
                }
            }
        }
        let _ = Chunk::index(0, 0, 0);
        chunks.insert(chunk);

        let mut player = Player::new((Vec3::new(8.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let start = player.position.y;
        run(&chunks, &mut player, Vec3::X, Vec3::X, false, 6.0);

        assert!(
            player.position.y > start + 1.0,
            "swimming into the bank did not climb it: {} to {}",
            start,
            player.position.y
        );
        // ...and the climb stops at the surface rather than walking up
        // the cliff into the sky.
        assert!(
            player.position.y < 22.0,
            "climbed straight out of the world: {}",
            player.position.y
        );
    }

    /// A lake with a shore standing in it at x >= 10, whose top is
    /// `top`, and the nine chunks a walker needs.
    fn lake_with_a_shore(top: usize) -> ChunkManager {
        use primitive_shared::types::{ChunkPos, BLOCK_STONE};
        let mut chunks = lake_world();
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        for x in 10..16 {
            for z in 0..16 {
                for y in 0..top {
                    chunk.set(x, y, z, BLOCK_STONE);
                }
            }
        }
        chunks.insert(chunk);
        chunks
    }

    #[test]
    fn a_swimmer_can_get_out_of_the_water_onto_a_shore() {
        // **"В воде невозможно прыгать."** Everything that lifts a
        // player out of water -- the jump stroke and the ledge climb --
        // used to be gated on `swimming`, which is false the moment the
        // water is no longer waist deep. So the climb carried the
        // swimmer to their waist and stopped, `grounded` was false
        // because the bank is beside them rather than under them, and
        // the step-up that walks a player over a kerb needs ground to
        // have been standing on. Measured against a shore at the
        // waterline: ten seconds of swimming into it left the player at
        // y=19.14 with the water surface at 19.88, bobbing there for as
        // long as anybody cared to watch, and the jump key did nothing
        // about it.
        //
        // The shore's top is the waterline, which is what a shore is.
        // A cliff standing a metre out of the lake is not climbable from
        // the water and is not meant to be -- see `LEDGE_CLIMB_SPEED`.
        let chunks = lake_with_a_shore(20);
        let mut player = Player::new((Vec3::new(8.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut escaped = false;
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, false, 1.0 / 60.0);
            escaped |= player.grounded && !player.in_water;
        }
        assert!(
            escaped,
            "never got out of the water: ended at {:?}, grounded {}, in water {}",
            player.position, player.grounded, player.in_water
        );
    }

    #[test]
    fn the_jump_key_still_lifts_you_when_the_water_is_below_your_waist() {
        // The other half of the same gate, without a bank in it. A
        // player surfacing is off the ground, in the water, and *not*
        // swimming -- the band between `SWIM_DEPTH` and the surface --
        // and in that band the jump key used to fall through to the
        // airborne branch, which reads neither jump flag. Half out of a
        // lake with nothing to push off, holding jump did nothing at
        // all.
        let chunks = lake_world();
        // Feet a little under the surface (19.88): shallower than
        // `SWIM_DEPTH`, so this is the band and not swimming.
        let start = Vec3::new(8.0, 19.4, 8.0);
        //
        // Ten frames and no more. The player is sinking the whole time,
        // and left alone they are back below `SWIM_DEPTH` inside a third
        // of a second -- at which point the old gate opens again and the
        // test stops being about the band at all. The assertion at the
        // end is what says it never left it.
        let lift = |jump: bool| {
            let mut player = Player::new(start.as_dvec3(), DEFAULT_MOVE_SPEED);
            player.refresh_fluid_state_here(&chunks);
            assert!(player.in_water, "the fixture is dry");
            assert!(!player.swimming, "the fixture is deep enough to swim in");
            for _ in 0..10 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, jump, jump, false, 1.0 / 60.0);
            }
            assert!(!player.swimming, "sank out of the band being tested");
            player.position.y
        };
        let held = lift(true);
        let idle = lift(false);
        assert!(
            held > idle + 0.05,
            "holding jump at the waterline changed nothing: {held} against {idle}"
        );
    }

    #[test]
    fn a_swimmer_never_looks_like_a_runner_to_the_view_bob() {
        // `lib.rs` bills a sprint -- and bobs the view to it -- on
        // "sprinting and grounded and not swimming and actually
        // moving". The two flags in the middle come from here, so a
        // change to what counts as ground or as swimming can switch the
        // run bob on in a lake without anything in `lib.rs` changing.
        // Nothing would fail; the camera would just start lurching
        // underwater.
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(2.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, true, 1.0 / 60.0);
            assert!(
                !(player.grounded && !player.swimming && player.horizontal_speed() > 0.5),
                "a swimmer read as a runner at {:?}: grounded {}, swimming {}, speed {}",
                player.position,
                player.grounded,
                player.swimming,
                player.horizontal_speed()
            );
        }
    }

    #[test]
    fn holding_jump_swims_upward() {
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(8.0, 12.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let start = player.position.y;
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
        }
        assert!(
            player.position.y > start + 1.0,
            "should have risen, went from {start} to {}",
            player.position.y
        );
    }

    #[test]
    fn swimming_up_does_not_launch_you_out_of_the_lake() {
        // Set-don't-add: holding jump rises steadily instead of
        // accelerating into orbit.
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(8.0, 12.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
            assert!(
                player.velocity.y <= SWIM_UP_SPEED + 0.01,
                "swim speed ran away: {}",
                player.velocity.y
            );
        }
    }

    /// Stone to y = 9, `depth` cells of water over it, and -- when `bank` is
    /// given -- stone at x >= 10 up to that height (exclusive).
    fn water_with_a_bank(depth: usize, bank: Option<usize>) -> ChunkManager {
        use primitive_shared::types::{ChunkPos, BLOCK_AIR, BLOCK_STONE, BLOCK_WATER};
        let mut chunks = world_of(|y| {
            if y < 10 {
                BLOCK_STONE
            } else if y < 10 + depth {
                BLOCK_WATER
            } else {
                BLOCK_AIR
            }
        });
        if let Some(top) = bank {
            let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
            for x in 10..16 {
                for z in 0..16 {
                    for y in 0..top {
                        chunk.set(x, y, z, BLOCK_STONE);
                    }
                }
            }
            chunks.insert(chunk);
        }
        chunks
    }

    /// The highest the feet get after one press of jump from standing
    /// still at `start`, the key held for a third of a second as a person
    /// holds it.
    fn apex_of_a_jump(chunks: &ChunkManager, player: &mut Player) -> f64 {
        for _ in 0..60 {
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let floor = player.position.y;
        let mut highest = floor;
        for frame in 0..90 {
            let held = frame < 20;
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, frame == 0, held, false, 1.0 / 60.0);
            highest = highest.max(player.position.y);
        }
        highest - floor
    }

    #[test]
    fn a_jump_from_the_bottom_of_a_ford_is_a_jump_and_lower_than_one_on_land() {
        // "В воде прыгать нельзя нормально." Measured before the fix: a jump
        // from the floor of one cell of water, key held as a person holds it,
        // rose 0.2 of a block -- the stroke's cap cut the take-off to three
        // blocks a second on the second frame.
        let land = apex_of_a_jump(&floor_world(), &mut Player::new(DVec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED));
        let ford = water_with_a_bank(1, None);
        let wet = apex_of_a_jump(&ford, &mut Player::new(DVec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED));
        assert!(wet > 1.05, "a jump from the bottom of a ford rose {wet}, which is not out of it");
        assert!(wet < land - 0.1, "a jump in a ford rose {wet} and one on land {land}: the water took nothing");
    }

    #[test]
    fn a_wader_jumps_out_of_a_ford_onto_a_bank_a_block_high() {
        let chunks = water_with_a_bank(1, Some(11));
        // The bank is six cells wide and the ford goes on past it, so what
        // is asked is whether the wader ever stood on it.
        let mut player = Player::new(DVec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
        let mut stood = false;
        for frame in 0..240 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, frame % 30 == 0, true, false, 1.0 / 60.0);
            stood |= player.grounded && (player.position.y - 11.0).abs() < 0.01;
        }
        assert!(stood, "a wader jumping at a bank a block high never stood on it, and ended at {:?}", player.position);
    }

    #[test]
    fn a_swimmer_jumps_out_onto_a_bank_a_block_over_the_water() {
        // A lake ten deep, its surface at 19.88, and a bank whose top is at 21
        // -- a block over the water's own cell. The ledge climb stops at the
        // surface on purpose, and before the clamber this was a wall: the
        // swimmer bobbed at its foot, holding the keys, for ever.
        let chunks = water_with_a_bank(10, Some(21));
        let mut player = Player::new(DVec3::new(8.0, 15.0, 8.5), DEFAULT_MOVE_SPEED);
        let mut stood = false;
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, true, true, false, 1.0 / 60.0);
            stood |= player.grounded && !player.in_water && (player.position.y - 21.0).abs() < 0.01;
        }
        assert!(stood, "a swimmer never got out onto the bank, and ended at {:?}", player.position);
    }

    #[test]
    fn a_bank_two_blocks_over_the_water_is_not_jumped_onto_from_the_lake_or_the_ford() {
        // What the clamber hands out is a jump from the surface, and a jump
        // does not reach two blocks -- from a lake, or from a ford, where
        // falling back through the water must not be a second jump.
        for (depth, top) in [(10, 22), (1, 12)] {
            let chunks = water_with_a_bank(depth, Some(top));
            let mut player = Player::new(DVec3::new(8.5, 10.5, 8.5), DEFAULT_MOVE_SPEED);
            let mut highest = 0.0f64;
            for frame in 0..600usize {
                player.update(&chunks, &[], Vec3::X, Vec3::X, frame.is_multiple_of(30) || depth > 1, true, false, 1.0 / 60.0);
                highest = highest.max(player.position.y);
            }
            assert!(highest < top as f64 - 0.3, "{depth} deep: a bank topping at {top} was climbed to {highest}");
        }
    }

    #[test]
    fn a_loaded_swimmer_on_the_bottom_pushes_off_it_in_a_damped_jump() {
        // Two cells of water and a body too heavy to float: it stands on the
        // bed, and the jump key was the stroke alone, a slow crawl up.
        let chunks = water_with_a_bank(2, None);
        let mut player = Player::new(DVec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
        player.buoyancy = 0.0;
        let rise = apex_of_a_jump(&chunks, &mut player);
        assert!(rise > 0.4, "a push off the bottom of deep water rose {rise}");
        assert!(rise < 1.2, "a push off the bottom of deep water rose {rise}, as far as a jump on land");
    }

    #[test]
    fn water_breaks_a_long_fall() {
        let chunks = lake_world();
        let mut player = Player::new((Vec3::new(8.0, 55.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        // Fall until we're in the water.
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            if player.in_water {
                break;
            }
        }
        assert!(player.in_water, "never reached the water");
        // A few frames later the speed must be back to swimming pace.
        for _ in 0..10 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        // Arrived at something like thirty blocks a second; a sixth of
        // a second later the water has taken nearly all of it. Stated
        // as a speed rather than against a clamp, because there is no
        // clamp -- the drag is what does this now.
        assert!(
            player.velocity.y > -8.0,
            "fall speed survived the splash: {}",
            player.velocity.y
        );
    }

    /// **"Быстрые реки": a rapid carries a swimmer off.** In water running
    /// faster than anybody swims, a swimmer stroking straight upstream for ten
    /// seconds ends up downstream of where they started.
    #[test]
    fn a_swimmer_in_a_rapid_is_carried_downstream_faster_than_they_can_swim_up() {
        let chunks = lake_world();
        let rapid = 3.2;
        assert!(rapid > primitive_shared::worldgen::RAPID_SPEED);
        let mut swimmer = Player::new((Vec3::new(-8.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        swimmer.current = (rapid, 0.0);
        let start = swimmer.position.x;
        for _ in 0..600 {
            swimmer.update(&chunks, &[], -Vec3::X, -Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(swimmer.swimming, "the test lake is not deep enough to swim in");
        let carried = swimmer.position.x - start;
        assert!(carried > 5.0, "ten seconds swimming against a rapid moved the swimmer {carried} blocks downstream");
    }

    /// **...and a lazy reach is crossed.** A swimmer striking straight
    /// across twenty blocks of river running at half a block a second reaches
    /// the far side, carried downstream by less than the river is wide: the
    /// calm reach is where a river is swum.
    #[test]
    fn a_swimmer_crosses_a_calm_reach_and_drifts_less_than_the_river_is_wide() {
        let chunks = lake_world();
        let mut swimmer = Player::new((Vec3::new(-8.0, 15.0, -12.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        swimmer.current = (0.5, 0.0);
        let (x0, z0) = (swimmer.position.x, swimmer.position.z);
        let mut seconds = 0.0;
        while swimmer.position.z < z0 + 20.0 && seconds < 30.0 {
            swimmer.update(&chunks, &[], Vec3::Z, Vec3::Z, false, false, false, 1.0 / 60.0);
            seconds += 1.0 / 60.0;
        }
        assert!(swimmer.position.z >= z0 + 20.0, "thirty seconds did not get a swimmer across twenty blocks of a lazy river");
        let drift = swimmer.position.x - x0;
        assert!((0.5..20.0).contains(&drift), "crossing a lazy river drifted the swimmer {drift} blocks");
    }

    /// **Water nobody swims in still carries them**: a body left alone in a
    /// river goes at the river's speed, not at some fraction of it the drag
    /// and the frame rate agreed on.
    #[test]
    fn a_body_left_alone_in_a_river_goes_at_the_river_s_speed() {
        let chunks = lake_world();
        for dt in [1.0 / 30.0, 1.0 / 144.0] {
            let mut floater = Player::new((Vec3::new(-10.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
            floater.current = (0.0, 0.9);
            floater.position.z = -12.0;
            let steps = (8.0 / dt) as usize;
            for _ in 0..steps {
                floater.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, dt);
            }
            assert!(
                (floater.velocity.z - 0.9).abs() < 0.1 && floater.velocity.x.abs() < 0.05,
                "a floater at {dt} s a frame runs at {:?} in a river running at 0.9",
                floater.velocity
            );
        }
    }

    #[test]
    fn swimming_is_slower_than_walking() {
        let chunks = lake_world();
        let air = floor_world();
        let mut swimmer = Player::new((Vec3::new(2.0, 15.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        let mut walker = Player::new((Vec3::new(2.0, 10.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        for _ in 0..30 {
            swimmer.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
            walker.update(&air, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            swimmer.position.x < walker.position.x,
            "swimming ({}) should be slower than walking ({})",
            swimmer.position.x,
            walker.position.x
        );
    }

    #[test]
    fn submerged_only_counts_when_the_head_is_under() {
        let chunks = lake_world();
        // Standing on the lake bed with the head well above the surface.
        let mut shallow = Player::new((Vec3::new(8.0, 19.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        shallow.refresh_fluid_state_here(&chunks);
        assert!(shallow.in_water, "feet are in the water");
        assert!(!shallow.submerged, "head is in open air");

        let mut deep = Player::new((Vec3::new(8.0, 12.0, 8.0)).as_dvec3(), DEFAULT_MOVE_SPEED);
        deep.refresh_fluid_state_here(&chunks);
        assert!(deep.submerged, "head should be under water");
    }
}

/// Physics against the *real* world generator, not a hand-built test
/// fixture. A synthetic lake can accidentally be shaped to pass; actual
/// terrain is what players walk on.
#[cfg(test)]
mod real_terrain_tests {
    use super::*;
    use primitive_shared::types::{ChunkPos, BLOCK_WATER};
    use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

    /// Finds a column of open water somewhere in the world, and returns
    /// it with its own chunk and all eight neighbours loaded.
    ///
    /// The search sweeps a wide area of chunks rather than just the ones
    /// around the origin. Oceans come from the generator's lowest-frequency
    /// field, so they are hundreds of blocks across and hundreds of blocks
    /// apart -- whether one happens to sit on the origin is down to the
    /// seed, and a test that assumed it did was testing the seed.
    fn ocean_column(seed: u32) -> Option<(ChunkManager, i32, i32)> {
        let generator = WorldGen::new(seed);

        // Deep enough that the player cannot simply stand on the bottom.
        let is_open_water = |gx: i32, gz: i32| {
            let column_top = generator.height_at(gx, gz);
            column_top <= SEA_LEVEL - 4
        };

        for cx in -24..=24 {
            for cz in -24..=24 {
                let centre_x = cx * 16 + 8;
                let centre_z = cz * 16 + 8;
                if !is_open_water(centre_x, centre_z) {
                    continue;
                }

                let mut cm = ChunkManager::new(4);
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        cm.insert(generator.generate_chunk(ChunkPos::new(cx + dx, cz + dz)));
                    }
                }

                // Confirm against the generated blocks, not just the
                // height field: the surface must be water with air over
                // it, and there must be real depth underneath.
                let surface = cm.block_at(centre_x, SEA_LEVEL, centre_z);
                let above = cm.block_at(centre_x, SEA_LEVEL + 1, centre_z);
                let deep = (1..=3)
                    .all(|d| cm.block_at(centre_x, SEA_LEVEL - d, centre_z) == Some(BLOCK_WATER));
                if surface == Some(BLOCK_WATER)
                    && above == Some(primitive_shared::types::BLOCK_AIR)
                    && deep
                {
                    return Some((cm, centre_x, centre_z));
                }
            }
        }
        None
    }

    #[test]
    fn you_cannot_stand_on_the_surface_of_a_real_ocean() {
        let mut found_any = false;
        for seed in [1337u32, 42, 7, 2024] {
            let Some((chunks, gx, gz)) = ocean_column(seed) else {
                continue;
            };
            found_any = true;

            // Drop in from just above the surface, walking forward the
            // whole time -- the way a player runs off a beach.
            let mut player = Player::new(
                (Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5)).as_dvec3(),
                DEFAULT_MOVE_SPEED,
            );
            for _ in 0..180 {
                player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
            }

            assert!(
                player.in_water,
                "seed {seed}: player at ({gx},{gz}) never entered the water (y={})",
                player.position.y
            );
            assert!(
                player.position.y < f64::from(SEA_LEVEL as f32),
                "seed {seed}: player is standing on the water surface at y={}",
                player.position.y
            );
        }
        assert!(found_any, "no ocean column found in any test seed");
    }

    #[test]
    fn the_seabed_still_stops_you() {
        // The other half of the bug: going down must not continue
        // through the floor of the ocean.
        //
        // The player has to *swim* down now -- buoyancy floats anyone
        // who presses nothing -- which is what makes this a test of the
        // dive against real terrain rather than of gravity. Steeply
        // down, but not exactly vertical: the camera cannot point
        // exactly vertically either.
        let Some((chunks, gx, gz)) = ocean_column(1337) else {
            return;
        };
        let down = Vec3::new(0.17, -0.985, 0.0).normalize();
        let mut player = Player::new(
            (Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5)).as_dvec3(),
            DEFAULT_MOVE_SPEED,
        );
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, down, false, false, false, 1.0 / 60.0);
        }
        assert!(player.position.y > 0.0, "player fell through the seabed");
        assert!(player.grounded, "player never reached the bottom");
    }
}

/// Where a thrown stack ends up, and whether the player who threw it
/// can see it there.
///
/// **Two crates and one question, which is why this is here rather than
/// beside either half.** The flight is the server's -- `logic::items`
/// integrates it, because where a thing in the world is is not the
/// client's opinion. The frame it has to land in is the client's:
/// `Camera` and the field of view out of the settings file. Neither
/// crate can state the property alone, and a test that lived in one of
/// them would have to hard-code the other's numbers and go stale
/// quietly. The anti-cheat agreement tests above are here for the same
/// reason.
#[cfg(test)]
mod thrown_item_tests {
    use super::*;
    use primitive_server::logic::items::{Items, ITEM_SIZE};
    use primitive_server::logic::world::World;
    use primitive_shared::types::{
        Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_SIZE_Y, CHUNK_VOLUME,
    };

    /// How much of the frame a player gives up to look at something
    /// without taking their eyes off where they are going.
    ///
    /// A quarter of the field of view, and a *fraction* rather than an
    /// angle on purpose: what counts as a glance is decided by how much
    /// the player can see, so the same test means the same thing at
    /// every setting between the 30 and the 120 the slider allows. An
    /// angle written here would silently become a different demand the
    /// day the default changed.
    const GLANCE: f32 = 0.25;

    /// Stone to y = 9, so the floor a thrower stands on is y = 10, in
    /// the nine chunks around the origin.
    fn floor_world() -> World {
        let world = World::new(1, 256);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..CHUNK_SIZE_Y.min(10) {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for cx in -1..=1 {
            for cz in -1..=1 {
                world.insert(Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: blocks.clone(),
                });
            }
        }
        world
    }

    /// Throws a stack the way the server does when a player flicks one
    /// out of the bar, and runs it until it stops moving.
    ///
    /// The spawn point and the direction are copied from `handle_drop`
    /// rather than approximated: eye height, four tenths of a block
    /// along the look, and the look itself as the throw direction.
    fn throw_from(feet: Vec3, pitch: f32) -> Vec3 {
        let world = floor_world();
        // Yaw zero, which is +X in the basis both the server's throw and
        // the client's camera use.
        let look = (pitch.cos(), pitch.sin(), 0.0);
        let mut items = Items::new();
        assert!(
            items.spawn(
                BLOCK_STONE,
                1,
                (
                    f64::from(feet.x + look.0 * 0.4),
                    f64::from(feet.y + EYE_HEIGHT + look.1 * 0.4),
                    f64::from(feet.z + look.2 * 0.4),
                ),
                look,
                None,
                std::time::Instant::now(),
            ),
            "the world refused the drop"
        );
        // Twenty ticks a second, which is what the server runs at, and
        // long enough for a lob to land and stop bouncing.
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, std::time::Instant::now());
        }
        let item = items.iter().next().expect("the drop vanished");
        Vec3::new(item.position.0 as f32, item.position.1 as f32, item.position.2 as f32)
    }

    /// Whether a world-space point is inside the frame, using the same
    /// matrix the renderer builds.
    fn on_screen(eye: Vec3, pitch: f32, point: Vec3) -> bool {
        let settings = crate::settings::ClientSettings::default();
        let mut camera = crate::engine::camera::Camera::new(eye.as_dvec3(), 16.0 / 9.0);
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        camera.yaw = 0.0;
        camera.pitch = pitch;
        let clip = camera.view_proj_about(Vec3::ZERO)
            * glam::Vec4::new(point.x, point.y, point.z, 1.0);
        clip.w > 0.0
            && (clip.y / clip.w).abs() <= 1.0
            && (clip.x / clip.w).abs() <= 1.0
    }

    #[test]
    fn a_stack_thrown_at_the_horizon_lands_where_a_glance_finds_it() {
        // **"Выкидываю предметы -- их не видно."** A drop was losing its
        // horizontal speed to a friction that ran every tick, flight
        // included, so it arrived with eighteen per cent of the speed it
        // left with and came down at the thrower's feet -- 57 degrees
        // below the horizon, past the bottom edge of the frame, in the
        // strip of screen the hand is drawn over.
        //
        // Stated as what the player sees rather than as how far it
        // travelled: a distance would have to be re-derived every time
        // the throw, the tick rate or the field of view moved, and the
        // thing that was actually wrong is that the item was off screen.
        let feet = Vec3::new(8.5, 10.0, 8.5);
        let eye = feet + Vec3::new(0.0, EYE_HEIGHT, 0.0);
        let settings = crate::settings::ClientSettings::default();
        let glance = -settings.fov_degrees.to_radians() * GLANCE;

        let at = throw_from(feet, 0.0);
        assert!(
            at.x > feet.x,
            "the throw went nowhere: landed at {at:?} from {feet:?}"
        );

        // The whole cube, not a sliver of its top edge at the very
        // bottom of the screen. Half of it showing is what the old
        // behaviour already managed at the widest setting, and it is
        // not what anybody means by seeing where their things went.
        let half = ITEM_SIZE / 2.0;
        for (corner, side) in [
            (at + Vec3::new(0.0, half, 0.0), "top"),
            (at - Vec3::new(0.0, half, 0.0), "bottom"),
        ] {
            assert!(
                on_screen(eye, glance, corner),
                "the {side} of a thrown stack was off screen at a glance: \
                 it landed at {at:?}, {:.2} blocks ahead and {:.1} degrees \
                 below the horizon, with a {} degree field of view",
                at.x - feet.x,
                (eye.y - at.y).atan2(at.x - feet.x).to_degrees(),
                settings.fov_degrees,
            );
        }
    }
}

/// "на больших координатах начинаются проблемы с движением". The same room,
/// the same keys and the same frames, at home and a long way out: see
/// [`Local`] for what an `f32` position did out there.
#[cfg(test)]
mod far_from_zero_tests {
    use super::*;
    use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_VOLUME};

    /// Where each test is run, in chunks: home, a million blocks out either
    /// way, and ten million -- past where an `f32` has any fraction at all.
    const OUT: [(i32, i32); 5] = [(0, 0), (62_500, 62_500), (-62_500, 62_500), (625_000, -625_000), (-625_000, -625_000)];

    /// A stone floor at y 0..=9 and a stone wall round the middle chunk at
    /// its cells 3 and 12, in the nine chunks round `(cx, cz)`.
    fn room(cx: i32, cz: i32) -> ChunkManager {
        let mut chunks = ChunkManager::new(4);
        let mut floor = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    floor[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut blocks = floor.clone();
                if dx == 0 && dz == 0 {
                    for y in 10..14 {
                        for i in 0..16 {
                            for (x, z) in [(i, 3), (i, 12), (3, i), (12, i)] {
                                blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                            }
                        }
                    }
                }
                chunks.insert(Chunk { pos: ChunkPos::new(cx + dx, cz + dz), blocks });
            }
        }
        chunks
    }

    fn corner(cx: i32, cz: i32) -> DVec3 {
        DVec3::new(f64::from(cx) * 16.0, 0.0, f64::from(cz) * 16.0)
    }

    /// A walk about the room at an uneven frame rate -- sneaking, sprinting,
    /// jumping, into the walls and along them -- as the feet measured from
    /// the room's corner after every frame.
    fn walk(cx: i32, cz: i32) -> Vec<DVec3> {
        let chunks = room(cx, cz);
        let start = corner(cx, cz) + DVec3::new(7.5, 10.0, 7.5);
        let mut player = Player::new(start, 5.5);
        let mut trace = Vec::new();
        for frame in 0..900 {
            let angle = (frame / 60) as f32 * 1.1;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            player.speed_scale = if (frame / 90) % 3 == 0 { 0.3 } else { 1.0 };
            let dt = [1.0 / 60.0, 1.0 / 144.0, 1.0 / 37.0][frame % 3];
            player.update(&chunks, &[], dir, Vec3::X, frame % 71 == 0, false, frame % 5 == 0, dt);
            trace.push(player.position - corner(cx, cz));
        }
        trace
    }

    /// **The mechanism, stated as arithmetic.** A sneaking step a million
    /// blocks out is lost in `f32` and kept in `f64`.
    #[test]
    fn a_sneaking_step_a_million_blocks_out_is_lost_in_f32_and_kept_in_f64() {
        let (at, step) = (1_000_000.5f32, 0.03f32);
        assert_eq!(at + step, at, "the premise: an f32 out there has no room for a step");
        assert!(((1_000_000.5f64 + f64::from(step)) - 1_000_000.53).abs() < 1e-6);
    }

    /// **The same walk, to the last bit, wherever the room is.** Every frame
    /// is worked out from the corner of the block the feet are in, so the
    /// numbers the collider sees are the same numbers at home and ten
    /// million blocks out -- and a difference here is a player who walks,
    /// jumps or stops at a wall differently for being far away.
    #[test]
    fn a_walk_far_from_zero_is_the_walk_it_is_at_home() {
        let home = walk(0, 0);
        let moved = home.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert!(moved > 800, "the walk at home hardly moved ({moved} frames of 900)");
        for (cx, cz) in OUT.into_iter().skip(1) {
            let far = walk(cx, cz);
            for (frame, (a, b)) in home.iter().zip(&far).enumerate() {
                assert!(
                    (*a - *b).abs().max_element() < 1e-6,
                    "chunk ({cx}, {cz}), frame {frame}: at home the feet were at {a:?} from the corner, out there at {b:?}"
                );
            }
        }
    }

    /// A wall is met at its face, a long way out as at home: run at it for a
    /// second from every side and the collider ends flush with the stone and
    /// not a sixteenth into it or short of it.
    #[test]
    fn a_wall_far_from_zero_stops_a_player_at_its_face() {
        for (cx, cz) in OUT {
            let chunks = room(cx, cz);
            for (dir, face) in [(Vec3::X, 12.0 - f64::from(PLAYER_HALF_WIDTH)), (-Vec3::X, 4.0 + f64::from(PLAYER_HALF_WIDTH))] {
                let mut player = Player::new(corner(cx, cz) + DVec3::new(7.5, 10.0, 7.5), 5.5);
                for _ in 0..120 {
                    player.update(&chunks, &[], dir, Vec3::X, false, false, true, 1.0 / 60.0);
                }
                let x = player.position.x - corner(cx, cz).x;
                assert!(
                    (x - face).abs() < 2e-3,
                    "chunk ({cx}, {cz}), running {dir:?}: stopped at {x} from the corner, the face is at {face}"
                );
                assert!(player.grounded, "chunk ({cx}, {cz}): not standing on the floor");
            }
        }
    }

    /// The crosshair picks the same block a long way out: a ray down at the
    /// floor from a hair inside a cell's edge.
    #[test]
    fn a_ray_far_from_zero_picks_the_block_it_picks_at_home() {
        for (cx, cz) in OUT {
            let chunks = room(cx, cz);
            let (bx, bz) = (cx * 16 + 6, cz * 16 + 9);
            let eye = corner(cx, cz) + DVec3::new(6.97, 11.62, 9.02);
            let hit = raycast_block(&chunks, eye, Vec3::new(0.0, -1.0, 0.0), 5.0);
            assert_eq!(hit, Some(((bx, 9, bz), (bx, 10, bz))), "chunk ({cx}, {cz})");
        }
    }
}
