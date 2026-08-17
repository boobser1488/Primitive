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

use glam::Vec3;

use primitive_shared::types::{collision_height, BlockId};

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

/// How deep the water has to be before walking becomes swimming.
///
/// Waist deep. Below it the feet are still on the floor and the player
/// is *wading*: ordinary walking, slowed by what they are pushing
/// through. Above it there is more water than legs, and nothing to push
/// against.
const SWIM_DEPTH: f32 = PLAYER_HEIGHT * 0.5;

/// How slowly you wade when the water is as deep as it can be and still
/// be walked through.
const WADE_SPEED: f32 = 0.45;

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
const WATER_GRAVITY: f32 = -BUOYANCY * FLOAT_SUBMERSION / PLAYER_HEIGHT;

/// Fraction of velocity kept per second while swimming. Water is
/// viscous: without this you accelerate to terminal velocity anyway and
/// "swimming" just means falling slowly.
///
/// Applied to all three axes now, not only to the vertical. That is
/// what gives a swimmer inertia -- a stroke carries you on after you
/// stop pressing, and you cannot turn on the spot.
const WATER_DRAG_PER_SECOND: f32 = 0.02;

/// How hard a swimmer pulls, per unit of the speed they are pulling
/// toward. A third of a second to full speed, against the ground's
/// twelfth: the difference between pushing off something and pulling
/// against nothing.
const SWIM_ACCEL: f32 = 3.0;

/// Top swimming speed, as a fraction of walking speed.
const WATER_MOVE_FACTOR: f32 = 0.55;

/// Upward acceleration from holding jump under water, and the speed it
/// tops out at.
///
/// Acceleration rather than an assignment: setting the velocity made
/// the stroke a step change, so tapping jump under water jerked. The
/// cap is what stops it from becoming a launch.
const SWIM_STROKE_ACCEL: f32 = 12.0;
const SWIM_UP_SPEED: f32 = 3.0;

/// You can't sink faster than this, however far you fell to get here.
///
/// Drag alone would take about a second to bleed off the speed of a
/// fifty-block fall, and a second at that speed is several blocks
/// straight down -- through the lake bed on a shallow lake. Clamping is
/// the same guarantee `MAX_STEP` gives falling blocks: whatever the
/// state before, one frame cannot travel further than the checks can
/// see.
const WATER_SINK_SPEED: f32 = -3.0;

/// Entering water kills most of your fall speed immediately -- this is
/// what makes water break a long fall.
const WATER_ENTRY_DAMPING: f32 = 0.25;

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
#[allow(dead_code)] // fallback used by tests and by callers without a settings file
pub const DEFAULT_MOVE_SPEED: f32 = 5.5;

/// How much faster sprinting is than walking.
///
/// Bounded by the server: the anti-cheat allows 12 blocks per second
/// horizontally against a 5.5 walk, and 1.6x of that is 8.8 -- fast
/// enough to feel like a sprint with headroom left for the lag spikes
/// the limit exists to tolerate. Raising this without raising
/// `max_horizontal_speed` would get sprinting players rubber-banded.
pub const SPRINT_MULTIPLIER: f32 = 1.6;

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
/// speed. High enough that walking still feels immediate -- full speed
/// arrives in well under a tenth of a second -- without being a
/// teleport.
const GROUND_ACCEL: f32 = 14.0;
/// Speed bled off per second while standing on something.
const GROUND_FRICTION: f32 = 12.0;
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

pub struct Player {
    /// Feet position (bottom-centre of the collider), world space.
    pub position: Vec3,
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
}

impl Player {
    pub fn new(spawn: Vec3, move_speed: f32) -> Self {
        Self {
            position: spawn,
            velocity: Vec3::ZERO,
            grounded: false,
            move_speed: move_speed.clamp(0.5, 20.0),
            submersion: 0.0,
            in_water: false,
            swimming: false,
            submerged: false,
            speed_scale: 1.0,
            jumped: false,
            step_lag: 0.0,
        }
    }

    pub fn eye_position(&self) -> Vec3 {
        self.position + Vec3::new(0.0, EYE_HEIGHT, 0.0)
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
    pub fn teleport(&mut self, position: Vec3) {
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
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        chunks: &ChunkManager,
        other_players: &[Vec3],
        wish_dir: Vec3,
        look: Vec3,
        jump_pressed: bool,
        jump_held: bool,
        sprinting: bool,
        dt: f32,
    ) {
        let was_in_water = self.in_water;
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
        let underfoot = self.surface_drag(chunks) * self.wade_drag();
        let base = self.move_speed * self.speed_scale.clamp(0.05, 1.0) * underfoot;
        let speed = if sprinting && !self.swimming {
            base * SPRINT_MULTIPLIER
        } else {
            base
        };

        if self.swimming {
            // Hitting water kills most of the fall. Only on *entry*: a
            // continuous damping would make sinking feel like syrup.
            if !was_in_water && self.velocity.y < 0.0 {
                self.velocity.y *= WATER_ENTRY_DAMPING;
            }

            // **Buoyancy, not weak gravity.** A body less submerged
            // than it wants to be sinks and a body more submerged than
            // it wants to be rises, so a swimmer who presses nothing
            // ends up at the surface with their head out and stays
            // there. That is the whole difference between water you can
            // be in and water you have to get out of.
            let float = self.submersion / PLAYER_HEIGHT;
            self.velocity.y += (WATER_GRAVITY + BUOYANCY * float) * dt;

            // The stroke, which is where the player is looking rather
            // than merely where they are facing -- the only way down
            // there is.
            self.swim(stroke_direction(wish_dir, look), speed * WATER_MOVE_FACTOR, dt);

            if jump_held {
                self.velocity.y = (self.velocity.y + SWIM_STROKE_ACCEL * dt).min(SWIM_UP_SPEED);
            }

            // Exponential drag on all three axes, framerate-independent.
            // This is what sets every top speed in the water: the stroke
            // pushes, the drag holds, and letting go coasts to a stop
            // instead of stopping dead.
            self.velocity *= WATER_DRAG_PER_SECOND.powf(dt);
            self.velocity.y = self.velocity.y.max(WATER_SINK_SPEED);
        } else if self.grounded {
            // Friction first, then acceleration: releasing the keys has
            // to actually slow you down, and holding them has to win
            // against the friction that is trying to.
            self.apply_friction(GROUND_FRICTION, dt);
            self.accelerate(wish_dir, speed, GROUND_ACCEL, dt);

            if jump_pressed {
                self.velocity.y = JUMP_VELOCITY;
                self.jumped = true;
            }
            self.velocity.y = (self.velocity.y + GRAVITY * dt).max(TERMINAL_VELOCITY);
        } else {
            // Airborne. No friction -- there is nothing to rub against
            // -- and acceleration capped so hard that it can redirect a
            // jump but never speed one up. See `AIR_CONTROL_SPEED`.
            self.accelerate(wish_dir, AIR_CONTROL_SPEED, AIR_ACCEL, dt);
            self.velocity.y = (self.velocity.y + GRAVITY * dt).max(TERMINAL_VELOCITY);
        }

        // Before anything moves: if the player is *already* inside
        // something, get them out. See `escape_solids`.
        self.escape_solids(chunks, other_players);

        let delta = self.velocity * dt;
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
        if !self.grounded && self.velocity.y <= 0.0 {
            let probe = self.position - Vec3::new(0.0, 4.0 * CONTACT_SKIN, 0.0);
            self.grounded = aabb_intersects_solid(chunks, probe);
        }

        self.step_lag = (self.step_lag - dt / STEP_SMOOTHING_SECONDS * PLAYER_STEP_HEIGHT).max(0.0);

        // Recompute after moving so the caller (fog, HUD) sees this
        // frame's state, not last frame's.
        self.refresh_fluid_state(chunks);
    }

    /// How much the surface underfoot slows walking, 0..1.
    ///
    /// The cell the feet are in first: a player wading through a drift
    /// is *inside* it, and that is the case that matters. Falling back
    /// to the cell below covers standing on top of a full block of the
    /// stuff.
    fn surface_drag(&self, chunks: &ChunkManager) -> f32 {
        let feet = self.position;
        let (x, z) = (feet.x.floor() as i32, feet.z.floor() as i32);
        let Some(column) = chunks.column(x, z) else {
            return 1.0;
        };
        let y = feet.y.floor() as i32;
        let inside = primitive_shared::types::surface_drag(column.block(y));
        if inside < 1.0 {
            return inside;
        }
        // A hair below the feet, so standing exactly on top of a block
        // reads as standing on it rather than as standing in the air
        // over it.
        primitive_shared::types::surface_drag(column.block((feet.y - 0.05).floor() as i32))
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

    /// Bleeds horizontal speed off, framerate-independently.
    fn apply_friction(&mut self, friction: f32, dt: f32) {
        let speed = self.horizontal_speed();
        if speed < 1e-4 {
            self.velocity.x = 0.0;
            self.velocity.z = 0.0;
            return;
        }
        let scale = ((speed - speed * friction * dt) / speed).max(0.0);
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
    fn refresh_fluid_state(&mut self, chunks: &ChunkManager) {
        let feet = self.position;
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
                surface = surface.max(gy as f32 + primitive_shared::fluid::surface_height(block));
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
        chunks: &ChunkManager,
        other_players: &[Vec3],
        delta: f32,
        axis: usize,
        was_grounded: bool,
    ) {
        if delta == 0.0 {
            return;
        }
        let hit = sweep_axis(chunks, other_players, self.position, delta, axis);
        if !hit.blocked {
            self.position[axis] += hit.allowed;
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
        self.position[axis] += hit.allowed;

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
            if self.swimming {
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
        chunks: &ChunkManager,
        other_players: &[Vec3],
        delta: f32,
        axis: usize,
        obstacle_top: f32,
    ) -> bool {
        let lift = obstacle_top - self.position.y;
        if lift <= 0.0 || lift > PLAYER_STEP_HEIGHT {
            return false;
        }
        let raised = self.position + Vec3::new(0.0, lift + CONTACT_SKIN, 0.0);
        if overlaps_anything(chunks, other_players, raised) {
            return false;
        }
        let hit = sweep_axis(chunks, other_players, raised, delta, axis);
        if hit.blocked {
            return false;
        }
        self.position = raised;
        self.position[axis] += hit.allowed;
        self.grounded = true;
        self.velocity.y = self.velocity.y.max(0.0);
        self.step_lag = (self.step_lag + lift).min(PLAYER_STEP_HEIGHT);
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
    fn escape_solids(&mut self, chunks: &ChunkManager, other_players: &[Vec3]) {
        let (min, max) = player_box(self.position);
        // How far to move along each direction to clear everything the
        // collider currently overlaps.
        let mut trapped = false;
        let mut up = 0.0f32;
        let mut push = [0.0f32; 4]; // +x, -x, +z, -z
        for_each_overlap(
            chunks,
            other_players,
            self.position,
            |bmin, bmax| {
                trapped = true;
                up = up.max(bmax[1] - min[1]);
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
            !overlaps_anything(chunks, other_players, player.position + offset)
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
                self.position += offset;
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
                self.position += offset;
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
    fn settle_onto_step(&mut self, chunks: &ChunkManager, other_players: &[Vec3]) {
        let (min, max) = player_box(self.position);
        let mut top = self.position.y;
        for_each_solid(chunks, min, max, |_bmin, bmax| {
            if bmax[1] > top && bmax[1] - self.position.y <= PLAYER_STEP_HEIGHT {
                top = bmax[1];
            }
        });
        let lift = top - self.position.y;
        if lift <= 0.0 {
            return;
        }
        let raised = self.position + Vec3::new(0.0, lift + CONTACT_SKIN, 0.0);
        if overlaps_anything(chunks, other_players, raised) {
            return; // no room above: leave them where they are
        }
        self.position = raised;
        self.grounded = true;
        self.velocity.y = self.velocity.y.max(0.0);
        self.step_lag = (self.step_lag + lift).min(PLAYER_STEP_HEIGHT);
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
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
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
        if let Some(block) = chunks.block_at(cell[0], cell[1], cell[2]) {
            if let Some(face) = ray_enters_block(origin, dir, cell, block, travelled, max_distance)
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
    origin: [f32; 3],
    dir: [f32; 3],
    cell: [i32; 3],
    block: BlockId,
    entered: f32,
    max_distance: f32,
) -> Option<Option<usize>> {
    if entered > max_distance {
        return None;
    }
    let (min, max) =
        primitive_shared::geometry::block_target_box(block, cell[0], cell[1], cell[2])?;
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
fn for_each_solid(
    chunks: &ChunkManager,
    min: Vec3,
    max: Vec3,
    mut visit: impl FnMut([f32; 3], [f32; 3]),
) {
    let (x0, x1) = (min.x.floor() as i32, (max.x - CONTACT_SKIN).floor() as i32);
    let (y0, y1) = (min.y.floor() as i32, (max.y - CONTACT_SKIN).floor() as i32);
    let (z0, z1) = (min.z.floor() as i32, (max.z - CONTACT_SKIN).floor() as i32);

    for bz in z0..=z1 {
        for bx in x0..=x1 {
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
                // Snow gives way under whoever is standing in it. The
                // top of the *box* drops, so the player settles into it
                // under ordinary gravity rather than being pushed --
                // and a drift shallower than the sink is trodden flat,
                // which is what happens to a thin drift.
                let height = collision_height(block);
                if height <= 0.0 {
                    continue;
                }
                visit(
                    [bx as f32, by as f32, bz as f32],
                    [bx as f32 + 1.0, by as f32 + height, bz as f32 + 1.0],
                );
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
fn for_each_overlap(
    chunks: &ChunkManager,
    other_players: &[Vec3],
    feet: Vec3,
    mut visit: impl FnMut([f32; 3], [f32; 3]),
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
    for_each_solid(chunks, min, max, |bmin, bmax| {
        if overlaps(bmin, bmax) {
            visit(bmin, bmax);
        }
    });
    for &other_feet in other_players {
        let (omin, omax) = player_box(other_feet);
        let (bmin, bmax): ([f32; 3], [f32; 3]) = (omin.into(), omax.into());
        if overlaps(bmin, bmax) {
            visit(bmin, bmax);
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
    /// step up would have to climb.
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
    chunks: &ChunkManager,
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
    let mut consider = |bmin: [f32; 3], bmax: [f32; 3]| {
        for other in 0..3 {
            if other == axis {
                continue;
            }
            if min[other] >= bmax[other] - CONTACT_SKIN || max[other] <= bmin[other] + CONTACT_SKIN {
                return;
            }
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
            contact.top = bmax[1];
        } else if contact.blocked && gap.abs() <= contact.allowed.abs() + CONTACT_SKIN {
            // Level with the nearest: a wall of two blocks is stepped
            // over only if the *taller* of them can be.
            contact.top = contact.top.max(bmax[1]);
        }
    };

    for_each_solid(chunks, lo, hi, &mut consider);
    for &other_feet in other_players {
        let (omin, omax) = player_box(other_feet);
        if omin.x < hi.x && omax.x > lo.x && omin.y < hi.y && omax.y > lo.y && omin.z < hi.z
            && omax.z > lo.z
        {
            consider(omin.into(), omax.into());
        }
    }
    contact
}

/// Would the player standing here be inside anything at all?
fn overlaps_anything(
    chunks: &ChunkManager,
    other_players: &[Vec3],
    feet: Vec3,
) -> bool {
    aabb_intersects_solid(chunks, feet)
        || other_players
            .iter()
            .any(|&other_feet| aabb_overlaps_player(feet, other_feet))
}

/// True if the player's collider at `feet_pos` overlaps any solid block.
fn aabb_intersects_solid(chunks: &ChunkManager, feet_pos: Vec3) -> bool {
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

    #[test]
    fn a_player_falls_and_lands_on_the_floor() {
        let chunks = floor_world();
        let mut player = Player::new(Vec3::new(8.0, 25.0, 8.0), DEFAULT_MOVE_SPEED);
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

    #[test]
    fn move_speed_from_settings_is_actually_used() {
        let chunks = floor_world();
        let mut slow = Player::new(Vec3::new(8.0, 10.0, 8.0), 1.0);
        let mut fast = Player::new(Vec3::new(8.0, 10.0, 8.0), 8.0);
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
        let other = Vec3::new(8.6, 10.0, 8.0);
        for _ in 0..60 {
            player.update(&chunks, &[other], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        assert!(
            !aabb_overlaps_player(player.position, other),
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
            let mut player = Player::new(Vec3::new(2.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(0.0, 50.0, 0.0), DEFAULT_MOVE_SPEED);
        player.velocity = Vec3::new(3.0, -20.0, 1.0);
        player.teleport(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(player.position, Vec3::new(1.0, 2.0, 3.0));
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
        let mut player = Player::new(Vec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
        let other = Vec3::new(8.5, 10.0, 8.5); // standing exactly on them
        player.update(&chunks, &[other], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(
            !aabb_overlaps_player(player.position, other),
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
            player.update(&chunks, &[other], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
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
        let hit = raycast_block(&chunks, Vec3::new(8.5, 12.0, 8.5), -Vec3::Y, 6.0);
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
                Vec3::new(2.5 + start as f32, 13.5, 3.5 + (start % 5) as f32),
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
                let centre = Vec3::new(8.0, player.position.y, 8.0);
                if (player.position.x - 8.0).abs() > 12.0
                    || (player.position.z - 8.0).abs() > 12.0
                {
                    let back = centre - player.position;
                    dir = Vec3::new(back.x, 0.0, back.z).normalize_or_zero();
                }
                // Frame times from a fast machine to a bad hitch: a
                // sweep that only works at 60 fps is a sweep that fails
                // on somebody's laptop.
                let dt = 1.0 / 240.0 + rng.next() * 0.1;
                let jump = rng.next() > 0.85;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, rng.next() > 0.5, dt);

                let inside = penetration(&chunks, player.position);
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
            let mut player = Player::new(Vec3::new(4.5, 10.0, 4.5), speed);
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
            let inside = penetration(&chunks, player.position);
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
                Vec3::new(3.5 + (start % 9) as f32, 13.0, 2.5 + (start % 7) as f32),
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
                    let back = Vec3::new(8.0 - player.position.x, 0.0, 8.0 - player.position.z);
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
                    player.position.x - before.x,
                    0.0,
                    player.position.z - before.z,
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
                let mut player = Player::new(Vec3::new(7.5, 10.0, 7.5), speed);
                for frame in 0..400 {
                    let dt = if frame % 3 == 0 { 1.0 / 60.0 } else { 1.0 / 300.0 };
                    let before = player.position;
                    player.update(&chunks, &[], dir, Vec3::X, frame % 37 == 0, false, true, dt);

                    let sideways =
                        Vec3::new(player.position.x - before.x, 0.0, player.position.z - before.z)
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
                        penetration(&chunks, player.position) <= CONTACT_SKIN * 2.0,
                        "angle {angle_step}, speed {speed}, frame {frame}: inside the wall at {:?}",
                        player.position
                    );
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
        let mut player = Player::new(Vec3::new(8.5, 10.0, 4.5), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
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
        let mut walker = Player::new(Vec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
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
            let mut player = Player::new(Vec3::new(8.5, ground, 8.5), DEFAULT_MOVE_SPEED);
            let mut dir = Vec3::X;
            for frame in 0..1200 {
                if frame % 11 == 0 {
                    let angle = rng.next() * std::f32::consts::TAU;
                    dir = Vec3::new(angle.cos(), 0.0, angle.sin());
                }
                if (player.position.x - 8.0).abs() > 10.0
                    || (player.position.z - 8.0).abs() > 10.0
                {
                    let back = Vec3::new(8.0 - player.position.x, 0.0, 8.0 - player.position.z);
                    dir = back.normalize_or_zero();
                }
                let dt = 1.0 / 240.0 + rng.next() * 0.06;
                let jump = rng.next() > 0.8;
                player.update(&chunks, &[], dir, Vec3::X, jump, jump, rng.next() > 0.5, dt);

                let inside = penetration(&chunks, player.position);
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
                let mut player = Player::new(Vec3::new(x, ground, z), DEFAULT_MOVE_SPEED);
                // Land, and let everything that settles finish settling
                // -- snow is meant to give way underfoot, and that is
                // not what this is looking for.
                for _ in 0..600 {
                    player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                }
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for _ in 0..300 {
                    player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
                    lo = lo.min(player.position.y);
                    hi = hi.max(player.position.y);
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
        let mut player = Player::new(Vec3::new(9.0002 - PLAYER_HALF_WIDTH, 10.0, 8.5), DEFAULT_MOVE_SPEED);
        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(
            player.position.y < 10.5,
            "climbed onto the block it was barely touching: y={}",
            player.position.y
        );
        assert!(
            penetration(&chunks, player.position) <= CONTACT_SKIN * 2.0,
            "left inside the wall at {:?}",
            player.position
        );
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
                Vec3::new(5.5 + start as f32, 14.0, 6.5),
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
                    let back = Vec3::new(8.0 - player.position.x, 0.0, 8.0 - player.position.z);
                    dir = back.normalize_or_zero();
                }
                let before = player.position.y;
                // Never jumping: every metre gained here was given to
                // the player by the collider rather than taken by them.
                player.update(&chunks, &[], dir, Vec3::X, false, false, false, 1.0 / 60.0);
                let rose = player.position.y - before;
                assert!(
                    rose <= PLAYER_STEP_HEIGHT + CONTACT_SKIN * 4.0,
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
        let mut player = Player::new(Vec3::new(15.9, 13.0, 8.5), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
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
        let inside = penetration(&chunks, player.position);
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

    /// Walks a player around and hands every position to the anti-cheat
    /// the way the client would.
    fn walk_and_judge(seed: u32, wall_hugging: bool, seconds: f32) -> Vec<String> {
        let (chunks, world) = matching_worlds(seed);
        let start = {
            let generator = primitive_shared::worldgen::WorldGen::new(seed);
            Vec3::new(8.5, generator.height_at(8, 8) as f32 + 2.0, 8.5)
        };
        let mut player = Player::new(start, DEFAULT_MOVE_SPEED);
        // Every limit at its shipped value. The speed budget and the
        // rate limits are token buckets that refill against the wall
        // clock, so this test runs in **real time** -- a simulation
        // that ran a minute of walking in a millisecond would empty
        // every bucket and report violations that say nothing about how
        // the player moved.
        let mut anticheat = AntiCheat::new(
            AntiCheatSettings::default(),
            8,
            (start.x, start.y, start.z),
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

        for frame in 0..frames {
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

            if frame % frames_per_update == 0 {
                // The client sends at `player_update_hz` and the frame
                // loop runs at the frame rate; both are real time, and
                // so is the budget being tested.
                std::thread::sleep(std::time::Duration::from_millis(
                    (1000.0 / 60.0 * frames_per_update as f32) as u64,
                ));
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
        }
        complaints
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
        let mut player = Player::new(Vec3::new(2.5, 13.0, 8.5), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let start = player.position.x;
        for _ in 0..120 {
            player.update(chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        player.position.x - start
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
        let mut player = Player::new(Vec3::new(8.5, 14.0, 8.5), DEFAULT_MOVE_SPEED);
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
/// cargo test --release -p primitive_client --bin primitive_client \
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
            std::hint::black_box(raycast_block(&chunks, eye, dir, 6.0));
        });

        // A frame of movement: three swept axes plus the settle pass and
        // the fluid checks, walking forward on the ground.
        let mut player = Player::new(Vec3::new(8.5, ground, 8.5), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        }
        let start = player.position;
        let per_update = time(ROUNDS, &mut || {
            player.position = start;
            player.update(&chunks, &[], Vec3::X, Vec3::X, false, false, false, 1.0 / 60.0);
        });

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
        let mut player = Player::new(Vec3::new(8.0, 25.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut swimmer = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);
        let mut faller = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);
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
        assert!(
            swimmer.velocity.y >= WATER_SINK_SPEED - 0.01,
            "sink speed must be capped, got {}",
            swimmer.velocity.y
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
            let mut player = Player::new(Vec3::new(8.0, start, 8.0), DEFAULT_MOVE_SPEED);
            run(&chunks, &mut player, Vec3::ZERO, Vec3::X, false, 15.0);

            let submersion = surface - player.position.y;
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
        let mut player = Player::new(Vec3::new(8.5, 10.0, 8.5), DEFAULT_MOVE_SPEED);
        player.refresh_fluid_state(&ankle_deep);

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
    fn wading_is_slower_than_walking_and_faster_than_swimming() {
        // The three regimes in one line, and the order is the whole
        // point: crossing a ford must not be either as quick as the path
        // beside it or as slow as the lake beyond it.
        use primitive_shared::types::{with_layers, BLOCK_WATER};
        let distance = |chunks: &ChunkManager, start: f32| {
            let mut player = Player::new(Vec3::new(2.0, start, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(2.0, 15.0, 8.0), DEFAULT_MOVE_SPEED);
        run(&chunks, &mut player, Vec3::X, Vec3::X, false, 2.0);
        let cruising = player.velocity.x;
        assert!(cruising > 0.5, "never got going: {cruising}");

        player.update(&chunks, &[], Vec3::ZERO, Vec3::X, false, false, false, 1.0 / 60.0);
        assert!(player.velocity.x < cruising, "the water did not slow them");
        assert!(player.velocity.x > 0.0, "stopped dead in a single frame");

        run(&chunks, &mut player, Vec3::ZERO, Vec3::X, false, 2.0);
        assert!(
            player.velocity.x.abs() < 0.05,
            "never came to rest: {}",
            player.velocity.x
        );
    }

    #[test]
    fn looking_down_and_swimming_forward_is_how_you_dive() {
        // There is no crouch key, so this is the only way down there is
        // -- and without it buoyancy would make the bottom of a lake
        // unreachable.
        let chunks = lake_world();
        let steep = Vec3::new(0.17, -0.985, 0.0).normalize();
        let mut diver = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);
        let mut floater = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);

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
        let mut strafer = Player::new(Vec3::new(8.0, 15.0, 8.0), DEFAULT_MOVE_SPEED);
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

        let mut player = Player::new(Vec3::new(8.0, 15.0, 8.0), DEFAULT_MOVE_SPEED);
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

    #[test]
    fn holding_jump_swims_upward() {
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut player = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, Vec3::X, true, true, false, 1.0 / 60.0);
            assert!(
                player.velocity.y <= SWIM_UP_SPEED + 0.01,
                "swim speed ran away: {}",
                player.velocity.y
            );
        }
    }

    #[test]
    fn water_breaks_a_long_fall() {
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 55.0, 8.0), DEFAULT_MOVE_SPEED);
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
        assert!(
            player.velocity.y >= WATER_SINK_SPEED - 0.01,
            "fall speed survived the splash: {}",
            player.velocity.y
        );
    }

    #[test]
    fn swimming_is_slower_than_walking() {
        let chunks = lake_world();
        let air = floor_world();
        let mut swimmer = Player::new(Vec3::new(2.0, 15.0, 8.0), DEFAULT_MOVE_SPEED);
        let mut walker = Player::new(Vec3::new(2.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
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
        let mut shallow = Player::new(Vec3::new(8.0, 19.0, 8.0), DEFAULT_MOVE_SPEED);
        shallow.refresh_fluid_state(&chunks);
        assert!(shallow.in_water, "feet are in the water");
        assert!(!shallow.submerged, "head is in open air");

        let mut deep = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
        deep.refresh_fluid_state(&chunks);
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
                Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5),
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
                player.position.y < SEA_LEVEL as f32,
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
            Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5),
            DEFAULT_MOVE_SPEED,
        );
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::X, down, false, false, false, 1.0 / 60.0);
        }
        assert!(player.position.y > 0.0, "player fell through the seabed");
        assert!(player.grounded, "player never reached the bottom");
    }
}
