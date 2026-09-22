//! A raft: a deck of lashed timber that floats, carries people, and goes
//! where it is rowed or blown.
//!
//! ## Why a body and not blocks
//!
//! A raft built out of placed planks would be a raft that cannot move. The
//! world is a grid of cells that stay where they are put, and carrying a
//! six-cell deck one block would be twelve edits a step, a relight and a
//! remesh of every chunk it crossed -- with whoever was standing on it
//! falling through the gap between one edit and the next. So a raft is a
//! body with a position and a heading, the way an animal is, and what a
//! player stands on is its *deck*: a rectangle this module answers
//! questions about, rather than cells in the world.
//!
//! ## Why the rules live here
//!
//! The server moves every raft, and the client moves the one it is rowing
//! ahead of the server, so that a stroke answers the key on the frame it is
//! pressed. Both call [`step`] with the same numbers against their own copy
//! of the same world. A rule written twice is a rule the two sides disagree
//! about, and a raft the client rows one way and the server another is a
//! raft that rubber-bands on every snapshot.
//!
//! ## What moves it, and the decision that makes
//!
//! Two engines and a nuisance. **The oars** are steady: the same pace in any
//! wind, steerable, and paid for out of the rower's breath and food. **The
//! sail** costs nothing and is fast with the wind behind it -- and worse
//! than nothing against it, because a square of hide held up to a head wind
//! is a brake that pushes backwards. **The wind and the current** move an
//! unattended raft, slowly, so where one is left is a decision about where
//! it will be found.
//!
//! Crossing a lake with the wind is a sail and a rest. Crossing it against
//! the wind is furling the sail and rowing, hungry. Neither is the right
//! answer every time, which is what makes it a choice rather than a stat.
//!
//! ## The angle of the sail, which is the decision
//!
//! The sail used to be a switch -- up or down -- and a switch has one
//! correct setting, which by this codebase's own rule is not a feature yet.
//! It carries an angle now ([`Body::sail_angle`], braced by hand: see
//! `riding`), and the angle decides everything about what the wind is worth:
//!
//! * **Square to a following wind** and the sail takes all of it.
//! * **Edge-on to the wind** and it takes none of it: a sail braced along
//!   the wind is a sail that is not there.
//! * **Halfway round, with the wind on the beam**, and the push is forward
//!   and sideways at once -- a reach, the fastest a raft goes, and the one
//!   that needs a hand on the sheets.
//! * **Straight into the wind**, at no angle at all, because the forward
//!   part of a flat sheet's push is `cos(angle)` of a number that is itself
//!   `cos(angle)` of a head wind. Squares and cubes cannot change a sign.
//!
//! So where a raft can go is a question about where the wind is, and
//! getting there is a question the player answers with their hand. That is
//! what the sail is for.
//!
//! ## The deck's own frame
//!
//! Everything about standing on a raft is said in `[along, up, across]`:
//! `along` toward the bow, `up` above the deck's top, `across` toward the
//! right-hand side looking forward. A player on a raft is a point in that
//! frame, and the world position is worked out from the raft every time --
//! which is what lets a rider travel with the deck instead of chasing it.

use serde::{Deserialize, Serialize};

use crate::fluid;
use crate::geometry::block_box;
use crate::types::{is_liquid, BlockId, BLOCK_AIR, BLOCK_OAR, BLOCK_PLANKS, BLOCK_SAIL, BLOCK_STICK};
use crate::weather::Weather;

/// Half the deck's length, bow to stern, in blocks.
///
/// **Three blocks long and two wide**: room for a rower at the stern and a
/// passenger standing forward of the mast without either being inside the
/// other, which is the "two players can ride one" the raft was asked for.
/// Longer than it is wide so that it has a front -- a square raft points
/// nowhere, and a sail that is fast with the wind *behind* needs a behind.
pub const HALF_LENGTH: f32 = 1.5;
/// Half the deck's width, side to side, in blocks.
pub const HALF_WIDTH: f32 = 1.0;
/// How far the top of the deck stands above the waterline.
///
/// Enough that a player standing on it is dry -- the collider measures
/// water from the feet, and feet at the waterline would be wading -- and
/// low enough that a swimmer can reach it.
pub const FREEBOARD: f32 = 0.3;
/// How far the bottom of the hull sits below the waterline. Water shallower
/// than this under any part of it is a beach, and the raft stops.
pub const DRAFT: f32 = 0.2;
/// How far past the deck's edge a foot is still held up.
///
/// Half a player's width: a body whose middle is over the edge has half of
/// itself on the timber, and a collider that dropped it there would be a
/// deck that is narrower to stand on than it is drawn.
pub const DECK_SLACK: f32 = crate::geometry::PLAYER_HALF_WIDTH;
/// Where the rower sits, in the deck's frame: at the stern, on the middle
/// line, facing the bow. The oars' model is hung from the same point.
pub const SEAT: [f32; 3] = [-1.05, 0.0, 0.0];
/// How far forward of the middle the mast stands, in blocks.
///
/// **Here rather than in the model**, which is where it was, because the
/// server has to answer "is this player standing at the sail" and the
/// client has to draw the mast in the same place it answered about. A mast
/// drawn at one place and reached from another is a player told to stand
/// where nothing is.
pub const MAST_ALONG: f32 = 0.55;
/// How far from the foot of the mast a hand still reaches the sheets.
///
/// A stride and a bit: the width of the deck. Enough that "go to the sail"
/// is walking to the front half of the raft rather than finding a pixel,
/// and short enough that a passenger sitting on the stern is not trimming
/// the sail over the rower's head.
pub const SAIL_REACH: f32 = 1.25;
/// The furthest the yard may be braced round from square, in radians.
///
/// **The same number the yard is drawn at** (`raft_model::pieces`), and it
/// has to be: the angle the player sees is the angle that drives the raft,
/// or the sail is a dial that lies. Past this a square sail wraps round its
/// own mast.
///
/// Just short of a right angle on purpose. Braced all the way, a sail could
/// be trimmed edge-on to any wind and a head wind would cost nothing --
/// and then there would be no reason ever to furl. At 1.2 radians a head
/// wind still finds `cos(1.2)^2`, an eighth, of the sail: sailing into the
/// wind is a thing you do with the sail *down* and the oars out.
pub const SAIL_MAX_ANGLE: f32 = 1.2;

/// How hard a full stroke pulls, in blocks per second per second.
///
/// With [`DRAG_ALONG`] this settles at two blocks a second: slower than a
/// walk and a little faster than a swim, which is what a raft rowed by one
/// person is -- and it is the same two blocks a second into any wind.
pub const ROW_THRUST: f32 = 1.8;
/// How hard the oars turn it, in radians per second per second. Settles
/// under a radian a second against [`SPIN_DRAG`]: a raft is turned, not
/// spun.
pub const ROW_TURN: f32 = 1.6;
/// The water's drag along the raft's length, per second.
pub const DRAG_ALONG: f32 = 0.9;
/// ...and across it, per second.
///
/// **Stronger than along**, and that is the whole of why a raft goes where
/// it points rather than where it is pushed: a flat deck slides sideways
/// more easily than a hull would, but its logs still run fore and aft.
/// Equal drags would make the sail a thing that blows the raft about
/// rather than a thing that drives it.
pub const DRAG_ACROSS: f32 = 2.4;
/// How quickly a turn dies away, per second.
pub const SPIN_DRAG: f32 = 2.0;
/// How hard a full wind square behind the sail drives it.
///
/// With the leeway beside it this settles at about two and a half blocks a
/// second in a fair wind and five and a half in a storm: in fair weather a
/// sail is the oars' pace for nothing, and in a storm it is faster than a
/// person can run -- and a storm is also when the sky is doing everything
/// else it does to a person out on the water.
pub const SAIL_THRUST: f32 = 4.2;
/// How hard the wind pushes a raft with its sail up, whichever way it is
/// pointed. This is what makes a sail against the wind a brake.
pub const SAIL_LEEWAY: f32 = 0.9;
/// ...and with the sail furled: the deck and whoever is standing on it.
/// An eighth of the sail's, which is a drift of a few centimetres a second.
pub const HULL_LEEWAY: f32 = 0.12;
/// How hard a difference in the water's depth pushes, per whole cell of
/// difference between neighbours.
pub const CURRENT_PUSH: f32 = 0.6;
/// How quickly the raft settles to the surface under it, per second.
pub const SETTLE_PER_SECOND: f32 = 4.0;
/// What a stroke is worth from a rower with no breath left.
///
/// **Weaker, not refused.** A rower who cannot row at all in the middle of
/// a lake is a player stranded by a gauge, which is a chore; one who rows
/// at a crawl until they have their wind back is a player who spent it too
/// early, which is a decision they made.
pub const TIRED_STROKE: f32 = 0.4;
/// How many blows break a raft apart.
///
/// Several, so that a swing at something beside the raft that lands on it
/// by accident costs a moment and not the raft.
pub const HITS_TO_BREAK: u32 = 5;
/// What a broken raft gives back.
///
/// **The things that were *made* come back whole and the lashing does
/// not.** A sail and two oars are objects; cutting the cords to take a raft
/// apart ruins the cord, and the rawhide that held the logs is cut through
/// too. Half the timber comes back with it. Taking a raft apart to carry it
/// over a ridge to the next lake is therefore possible and costs a second
/// lashing -- which is a decision about whether the next lake is worth it.
pub const SALVAGE: &[(BlockId, u32)] =
    &[(BLOCK_PLANKS, 8), (BLOCK_STICK, 4), (BLOCK_SAIL, 1), (BLOCK_OAR, 2)];
/// The furthest a player may be from a deck and still step onto it, in
/// blocks, measured by the server from where it last had them.
///
/// A jump from the bank is about two; the rest is the latency between the
/// raft the client stepped onto and the raft the server has moved on.
pub const BOARDING_REACH: f32 = 3.5;

/// Where a raft is and how it is moving.
///
/// `y` is the **waterline**, not the deck: the one height the water decides
/// and everything else is measured from. The deck's top is
/// [`Body::deck_top`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// Where the middle of the hull is. `x` and `z` are `f64`: a raft is the
    /// thing in this game most likely to be a long way from anywhere, and an
    /// `f32` a million blocks out moves in sixteenths -- a hull at a walking
    /// pace stood still for three ticks and jumped on the fourth, and
    /// everybody standing on it jumped with it. `y` is a waterline, and never
    /// leaves a few hundred.
    pub x: f64,
    pub y: f32,
    pub z: f64,
    /// Which way the bow points, in the `Camera::forward` convention: zero
    /// looks along +X, a quarter turn along +Z.
    pub yaw: f32,
    pub vx: f32,
    pub vz: f32,
    /// How fast it is turning, in radians per second.
    pub spin: f32,
    /// Whether the sail is up.
    pub sail: bool,
    /// How far the yard is braced round from square, in radians, positive
    /// toward the starboard bow. Zero is a square sail, whose face looks
    /// straight over the bow; `SAIL_MAX_ANGLE` is braced as sharp as the
    /// mast allows. Means nothing while `sail` is false.
    pub sail_angle: f32,
}

impl Body {
    /// A raft sitting still on the water, sail furled and square.
    pub fn at_rest(x: f64, y: f32, z: f64, yaw: f32) -> Self {
        Self { x, y, z, yaw, vx: 0.0, vz: 0.0, spin: 0.0, sail: false, sail_angle: 0.0 }
    }

    /// The way the sail's face looks, as (x, z): the heading turned by the
    /// yard's angle.
    ///
    /// One definition, read by the rule that moves the raft ([`step`]) and
    /// by the model that draws the yard (`raft_model::pieces`). Two copies
    /// of this turn is a sail pointing one way on the screen and pulling
    /// another, which is the one thing a dial must never do.
    #[inline]
    pub fn sail_normal(&self) -> (f32, f32) {
        let (fx, fz) = self.forward();
        let (rx, rz) = (-fz, fx);
        let (sin, cos) = trim_clamped(self.sail_angle).sin_cos();
        (fx * cos + rx * sin, fz * cos + rz * sin)
    }

    /// The unit vector the bow points along, as (x, z).
    #[inline]
    pub fn forward(&self) -> (f32, f32) {
        let (sin, cos) = self.yaw.sin_cos();
        (cos, sin)
    }

    /// The height of the top of the deck.
    #[inline]
    pub fn deck_top(&self) -> f32 {
        self.y + FREEBOARD
    }

    /// How fast it is going, in blocks per second.
    #[inline]
    pub fn speed(&self) -> f32 {
        (self.vx * self.vx + self.vz * self.vz).sqrt()
    }

    /// A point in the deck's frame, `[along, up, across]`, in the world.
    #[inline]
    pub fn world_of(&self, local: [f32; 3]) -> [f64; 3] {
        let (sin, cos) = self.yaw.sin_cos();
        [
            self.x + f64::from(local[0] * cos - local[2] * sin),
            f64::from(self.deck_top() + local[1]),
            self.z + f64::from(local[0] * sin + local[2] * cos),
        ]
    }

    /// A point in the world, in the deck's frame. The exact inverse of
    /// [`world_of`](Self::world_of): a rider's place on the deck is carried
    /// through one and back through the other every tick, so any
    /// disagreement between them would walk the rider across the deck.
    #[inline]
    pub fn local_of(&self, world: [f64; 3]) -> [f32; 3] {
        let (sin, cos) = self.yaw.sin_cos();
        let (dx, dz) = ((world[0] - self.x) as f32, (world[2] - self.z) as f32);
        [dx * cos + dz * sin, (world[1] - f64::from(self.deck_top())) as f32, -dx * sin + dz * cos]
    }

    /// Whether a point in the deck's frame is over the deck, allowing
    /// `slack` past every edge.
    #[inline]
    pub fn over_deck(local: [f32; 3], slack: f32) -> bool {
        local[0].abs() <= HALF_LENGTH + slack && local[2].abs() <= HALF_WIDTH + slack
    }

    /// Everything about it is a number. Checked on anything read off a disk
    /// or a socket: a raft at `NaN` fails every collision test and is then
    /// immovable and invisible for ever.
    pub fn is_sane(&self) -> bool {
        self.x.is_finite()
            && self.z.is_finite()
            && [self.y, self.yaw, self.vx, self.vz, self.spin, self.sail_angle].iter().all(|v| v.is_finite())
    }
}

/// A sail angle within what the mast allows, and a number.
///
/// **Off a socket as well as off a hand.** An angle of a hundred radians
/// would be a sail braced through its own mast, and a `NaN` one would put
/// the raft's whole velocity at `NaN` on the next tick -- which is a raft
/// that fails every collision test and can never be moved or seen again.
/// So the server clamps what it is told and the client clamps what it
/// predicts with, and the two agree: the same argument [`Oars::clamped`]
/// makes.
#[inline]
pub fn trim_clamped(angle: f32) -> f32 {
    if angle.is_finite() {
        angle.clamp(-SAIL_MAX_ANGLE, SAIL_MAX_ANGLE)
    } else {
        0.0
    }
}

/// Whether somebody standing at this place on the deck can reach the
/// sheets.
///
/// Measured from the foot of the mast, flat: how high they are standing
/// does not decide whether they can reach a rope that runs to the deck, and
/// a rider mid-jump is still at the sail.
#[inline]
pub fn at_the_sail(local: [f32; 3]) -> bool {
    (local[0] - MAST_ALONG).hypot(local[2]) <= SAIL_REACH
}

/// What the rower is asking the oars to do.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Oars {
    /// -1 (backing water) to 1 (a full stroke forward).
    pub stroke: f32,
    /// -1 (turning left) to 1 (turning right).
    pub turn: f32,
}

impl Oars {
    /// Nobody pulling.
    pub const REST: Oars = Oars { stroke: 0.0, turn: 0.0 };

    /// The same, within what two arms can do.
    ///
    /// These arrive off a socket, and a stroke of ten would be a raft rowed
    /// at twenty blocks a second -- so the server clamps what it is told,
    /// and the client clamps what it predicts with, and the two agree.
    pub fn clamped(self) -> Oars {
        let tame = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        Oars { stroke: tame(self.stroke), turn: tame(self.turn) }
    }

    /// Whether anyone is actually pulling, which is what the rower is billed
    /// for.
    pub fn pulling(self) -> bool {
        let oars = self.clamped();
        oars.stroke.abs() > 0.05 || oars.turn.abs() > 0.05
    }
}

/// Which way the wind blows and how hard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wind {
    /// The direction it blows *toward*, in the `Camera::forward` convention.
    pub toward: f32,
    /// 0 (still) to 1 (a storm).
    pub strength: f32,
}

impl Wind {
    pub const CALM: Wind = Wind { toward: 0.0, strength: 0.0 };

    /// The wind as a vector, as (x, z), as long as it is strong.
    #[inline]
    pub fn vector(self) -> (f32, f32) {
        let (sin, cos) = self.toward.sin_cos();
        (cos * self.strength, sin * self.strength)
    }
}

/// How many hours the wind's own clock counts to a world day.
const HOURS_PER_DAY: f32 = 24.0;
/// How long the slow wander takes to reach for a fresh direction, in world
/// hours -- and how far it may swing when it gets there.
///
/// A whole turn over about a day, which is a wind that has changed its mind
/// by the time you row back. Any faster and the passage you planned round
/// it would be wrong before you had finished making it; any slower and it
/// would be the old two sine waves again by another name.
const VEER_HOURS: f32 = 24.0;
const VEER_SWING: f32 = std::f32::consts::TAU;
/// ...and the flaw on top of it: a small shift every few hours, which is
/// what makes holding a course something you keep doing rather than
/// something you did once.
const FLAW_HOURS: f32 = 4.0;
const FLAW_SWING: f32 = 0.35;
/// How long a lull or a blow lasts, in world hours, and how fast the gusts
/// inside it come.
const LULL_HOURS: f32 = 6.0;
const GUST_HOURS: f32 = 0.7;
/// What the weather is worth before the lull is applied to it.
///
/// Multiplied rather than added, so a calm hour under a storm sky is still
/// a harder wind than a calm hour under a clear one -- and so the three
/// skies never trade places, whatever the noise is doing
/// (`a_storms_wind_is_always_harder_than_a_fair_days_at_the_same_hour`).
fn weather_wind(weather: Weather) -> f32 {
    match weather {
        Weather::Clear => 0.45,
        Weather::Rain => 0.68,
        Weather::Storm => 1.0,
    }
}
/// The ends of the lull: dead calm at one and a squall at the other.
const LULL_RANGE: (f32, f32) = (0.05, 1.45);

/// The wind at an hour of the world, in its weather.
///
/// **A function of the clock rather than a thing the server rolls and
/// sends**, and that is the decision worth writing down. The weather has no
/// wind of its own, and adding one to the sky would be a message and a
/// saved field for a number that only rafts, the rain and a sail's picture
/// ever read. Both sides already know the world's age
/// (`TimeSync::world_days`) and the weather (`WeatherSync`), so a wind
/// worked out from those two is the same wind on the server that moves the
/// raft and on the client that predicts it, draws its sail and slants its
/// rain -- with nothing on the wire, and with no way for the rower's
/// prediction to disagree with the server about what pushed them.
///
/// ## Why noise and not two sine waves
///
/// It *was* two sine waves that never line up, and the note here said so:
/// "the price is that it is not random, which a player cannot tell from a
/// wind that is." A player told. Two waves have a period, and a period is
/// something you learn -- the wind came round to the north every time the
/// second morning came round, and the crossing you could not make at noon
/// was the crossing you could always make at dusk.
///
/// So the waves are gone and what is left is **value noise in time**: a
/// fresh roll at every step of the wind's own clock, eased between rolls so
/// the wind veers rather than flicks. It keeps every property the sine
/// waves were chosen for -- pure, cheap, identical on both sides, nothing
/// on the wire -- and loses the one that was wrong with them. Rolling it on
/// the server and broadcasting it would buy nothing this does not already
/// have, and would cost a message, a saved field, and a rower whose
/// prediction is a message behind the raft it is predicting.
///
/// The direction wanders a whole turn over about a day with a small flaw
/// on top of it. The strength is the weather's, scaled by a lull that
/// reaches dead calm at one end and a squall at the other: the wind is an
/// event, not a background. A calm is a reason to take the oars out, and a
/// squall is a reason to be somewhere else.
pub fn wind(world_days: f32, weather: Weather) -> Wind {
    use std::f32::consts::TAU;
    let days = if world_days.is_finite() { world_days } else { 0.0 };
    let hours = days * HOURS_PER_DAY;
    let toward = (VEER_SWING * noise(hours / VEER_HOURS, 0) + FLAW_SWING * noise(hours / FLAW_HOURS, 1))
        .rem_euclid(TAU);
    let lull = (0.75 + 0.70 * noise(hours / LULL_HOURS, 2) + 0.30 * noise(hours / GUST_HOURS, 3))
        .clamp(LULL_RANGE.0, LULL_RANGE.1);
    Wind { toward, strength: (weather_wind(weather) * lull).clamp(0.0, 1.0) }
}

/// One value of the wind's noise: a number in -1..1 that is a fresh roll at
/// every whole step of `t` and eased between them.
///
/// **Eased with a smoothstep and not with a straight line.** A straight
/// blend between rolls is continuous in the value and not in its slope, so
/// the wind would change *how fast it is veering* on the hour, every hour,
/// and a sail braced to it twitches at each of those corners. The
/// smoothstep costs two multiplies and has no corners.
fn noise(t: f32, salt: u32) -> f32 {
    let step = t.floor();
    let frac = t - step;
    // `f32 as i64` saturates rather than wrapping, which is what keeps a
    // world at an absurd age from folding its wind back onto hour zero.
    let cell = (step as i64 as u32).wrapping_mul(2654435761).wrapping_add(salt);
    let (a, b) = (roll(cell), roll(cell.wrapping_add(2654435761)));
    a + (b - a) * (frac * frac * (3.0 - 2.0 * frac))
}

/// The roll itself: a number in -1..1 from an integer, with no pattern in
/// it that a player could learn.
///
/// An integer hash rather than `rand`: this is called from both sides of
/// the wire and from a pure function, and the two sides must get the same
/// bits out of it on every machine. A generator with a state could not be,
/// and `sin(x) * 43758.5453` -- the usual shader trick -- is not the same
/// number on two machines with different transcendental libraries.
fn roll(n: u32) -> f32 {
    let mut h = n.wrapping_mul(747796405).wrapping_add(2891336453);
    h = ((h >> ((h >> 28).wrapping_add(4))) ^ h).wrapping_mul(277803737);
    h = (h >> 22) ^ h;
    (h >> 8) as f32 / (1 << 23) as f32 * 2.0 - 1.0
}

/// The height of the water's surface under a point, looking a cell or two
/// either side of `near_y`, or `None` if there is no water there.
///
/// The topmost water in that stretch of the column, measured to its drawn
/// surface -- the same line `fluid::surface_height_with_above` gives the
/// mesher and the swimmer, so a raft floats on the water it is drawn on.
pub fn surface_under<F>(x: f64, near_y: f32, z: f64, block: &F) -> Option<f32>
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (x.floor() as i32, z.floor() as i32);
    let top = near_y.floor() as i32 + 1;
    for cy in (top - 3..=top).rev() {
        let here = block(ix, cy, iz)?;
        if !is_liquid(here) {
            continue;
        }
        let above = block(ix, cy + 1, iz).unwrap_or(BLOCK_AIR);
        if is_liquid(above) {
            // The surface is higher than the stretch being searched.
            return None;
        }
        return Some(cy as f32 + fluid::surface_height_with_above(here, above));
    }
    None
}

/// Whether the hull fits at this pose: water under every part of it deep
/// enough to float in, and nothing solid anywhere from the keel to the deck.
///
/// **Sampled every half block over the deck**, not only at its corners. A
/// corner test lets a raft straddle a single rock in the middle of a lake,
/// or push its middle up a spit of sand between two corners that are both
/// still over water.
///
/// A cell nobody has loaded is not water. On the server that stops a raft at
/// the edge of the world anyone has seen, rather than letting it sail into
/// terrain that is generated under it.
pub fn fits<F>(x: f64, y: f32, z: f64, yaw: f32, block: &F) -> bool
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    const SPACING: f32 = 0.5;
    let (sin, cos) = yaw.sin_cos();
    let along_steps = (HALF_LENGTH * 2.0 / SPACING).round() as i32;
    let across_steps = (HALF_WIDTH * 2.0 / SPACING).round() as i32;
    for i in 0..=along_steps {
        let along = -HALF_LENGTH + i as f32 * SPACING;
        for j in 0..=across_steps {
            let across = -HALF_WIDTH + j as f32 * SPACING;
            let px = x + f64::from(along * cos - across * sin);
            let pz = z + f64::from(along * sin + across * cos);
            if !floats_at(px, pz, y, block) {
                return false;
            }
        }
    }
    true
}

/// One sample of [`fits`].
fn floats_at<F>(px: f64, pz: f64, y: f32, block: &F) -> bool
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (px.floor() as i32, pz.floor() as i32);
    let waterline = (y - 0.05).floor() as i32;
    let Some(here) = block(ix, waterline, iz) else {
        return false;
    };
    if !is_liquid(here) {
        return false; // a bank, or a beach at the waterline
    }
    let above = block(ix, waterline + 1, iz).unwrap_or(BLOCK_AIR);
    // **Shallows are shore.** A film of water an eighth deep is still
    // water, and a raft that floated over it would be sliding across a
    // beach with its keel on the sand.
    if (waterline as f32 + fluid::surface_height_with_above(here, above)) < y - DRAFT {
        return false;
    }
    for cy in (y - DRAFT).floor() as i32..=(y + FREEBOARD).floor() as i32 {
        let Some(cell) = block(ix, cy, iz) else {
            return false;
        };
        // Asked in the cell's own frame, for `geometry::for_each_block_box_f64`'s
        // reason: a box built out in the world has no halves left far away.
        if let Some((lo, hi)) = block_box(cell, 0, cy, 0) {
            let (px, pz) = ((px - f64::from(ix)) as f32, (pz - f64::from(iz)) as f32);
            let inside = (lo[0]..hi[0]).contains(&px) && (lo[2]..hi[2]).contains(&pz);
            if inside && lo[1] < y + FREEBOARD && hi[1] > y - DRAFT {
                return false;
            }
        }
    }
    true
}

/// Where a raft goes when it is launched from a cell of water, looking along
/// `yaw`, or `None` if there is no room for one.
///
/// **Pushed out along the look before it is refused.** The cell a player
/// aims at from a bank is the cell against the bank, and a raft three
/// blocks long centred there is half up the beach. Trying a little further
/// out, along the way they are looking, puts it on the water they meant;
/// refusing would teach them to wade out to launch a boat, which is a chore.
pub fn launch<F>(aim: [f64; 3], yaw: f32, block: &F) -> Option<Body>
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (sin, cos) = yaw.sin_cos();
    for reach in [0.0, 0.5, 1.0, 1.5, 2.0] {
        let (x, z) = (aim[0] + f64::from(cos * reach), aim[2] + f64::from(sin * reach));
        let Some(y) = surface_under(x, aim[1] as f32, z, block) else {
            continue;
        };
        if fits(x, y, z, yaw, block) {
            return Some(Body::at_rest(x, y, z, yaw));
        }
    }
    None
}

/// Which way the water under the raft is running, as an acceleration.
///
/// Water here is conserved and runs from fuller cells into emptier ones, so
/// a difference in depth between neighbours *is* the current -- no second
/// flow field to keep in step with the one the simulation already has.
/// Standing water pushes nothing.
///
/// **Read through `fluid::running`, which counts only what the rules would
/// actually move.** This used to take every difference between neighbours,
/// and a settled pond keeps differences of one eighth that whole eighths
/// cannot split -- so a raft left on a pond somebody had once drained into
/// drifted, slowly and for ever, toward wherever the odd eighths lay.
/// `running` hands on half of a difference, so it is doubled back here to
/// keep `CURRENT_PUSH` meaning what it says: per whole cell of difference.
fn current<F>(body: &Body, block: &F) -> (f32, f32)
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (body.x.floor() as i32, body.z.floor() as i32);
    let cy = (body.y - 0.05).floor() as i32;
    let (ex, ez) = fluid::running(ix, cy, iz, block);
    let per_cell = 2.0 / fluid::SOURCE_DEPTH as f32 * CURRENT_PUSH;
    (ex * per_cell, ez * per_cell)
}

/// **Where the open sea is carrying this raft**, in blocks a second.
///
/// The circulation is `fluid::current_at`; what this adds is the one thing
/// that function cannot know without a world -- how deep the water under
/// the raft is. Counted downward rather than read from a height map,
/// because a raft is handed a `block` lookup and nothing else, which is
/// what lets the same step run on the server and inside the client's
/// prediction. It stops at the first thing that is not water, so a pond
/// over a cavern is a pond, and it looks no deeper than the sea needs to
/// make up its mind.
fn sea_drift<F>(body: &Body, block: &F, world_days: f32) -> (f32, f32)
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (body.x.floor() as i32, body.z.floor() as i32);
    let top = (body.y - 0.05).floor() as i32;
    let mut depth = 0.0;
    for step in 0..=SEA_PROBE {
        match block(ix, top - step, iz) {
            Some(b) if is_liquid(b) => depth = (step + 1) as f32,
            // Unloaded is not deep: a raft at the edge of what the client
            // has been sent must not be pushed by a sea nobody can see.
            _ => break,
        }
    }
    fluid::current_at(body.x as f32, body.z as f32, depth, world_days)
}

/// How far down `sea_drift` bothers to look, in cells: one past the depth
/// at which `fluid::current_at` is already at full strength, so the answer
/// is the same as an unbounded count wherever it differs from nothing.
const SEA_PROBE: i32 = 29;

/// Moves a raft on by `dt` seconds. Answers whether it ran into anything.
///
/// Axis at a time, the way the items and the player collide: a raft
/// grazing a bank along its side keeps going along it rather than stopping
/// dead, because only the axis that hit is stopped. The turn is tried last
/// and on its own, so a raft pressed against a shore can still be rowed
/// along it and cannot be turned into it.
///
/// **A raft that does not fit where it already is does not move.** That is
/// a raft somebody built a block into or drained the lake from under, and
/// letting it move would let it be rowed across dry land. It settles and
/// waits for the water to come back or for somebody to break it up.
///
/// `river` is the current of the river the raft is on, where it is, in
/// blocks a second: `WorldGen::river_current`, which the server and the
/// rower's client each ask their own generator. Passed in rather than looked
/// up for the reason `block` is -- this module has no world -- and nothing at
/// all on a lake, the sea or a player's own pond.
#[allow(clippy::too_many_arguments)]
pub fn step<F>(body: &mut Body, oars: Oars, wind: Wind, world_days: f32, river: (f32, f32), block: &F, dt: f32) -> bool
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    if !dt.is_finite() || dt <= 0.0 || !body.is_sane() {
        return false;
    }
    let oars = oars.clamped();
    let (fx, fz) = body.forward();
    let (rx, rz) = (-fz, fx);
    let (wx, wz) = wind.vector();

    let mut ax = fx * oars.stroke * ROW_THRUST;
    let mut az = fz * oars.stroke * ROW_THRUST;
    if body.sail {
        // **A flat sheet of hide feels only the wind across its face.** The
        // drive is the part of the wind along the sail's normal, pushed
        // back out along that normal -- which is the whole of the sail's
        // behaviour and not three cases with a rule each:
        //
        // * square to a following wind, the normal is the heading and the
        //   push is all forward, as it was when the sail was a switch;
        // * braced round with the wind on the beam, the push is forward and
        //   sideways at once and the water's grip across the deck
        //   (`DRAG_ACROSS`) turns the sideways half into leeway -- a reach;
        // * edge-on, the face feels nothing and the sail is not there;
        // * and into the wind the forward part is `cos` of a number that is
        //   itself `cos` of a head wind, so it is never positive at any
        //   angle. **There is no trim that sails into the wind**, which is
        //   what makes a head wind a decision (furl and row) rather than a
        //   button.
        //
        // It used to be `max(0.0)` of the wind behind, which made a head
        // wind cost only the leeway below; now a squared sail held up to
        // one is the brake it looks like, and bracing it round is how a
        // rower gets that brake off without lowering the sail.
        let (nx, nz) = body.sail_normal();
        let press = wx * nx + wz * nz;
        ax += nx * press * SAIL_THRUST + wx * SAIL_LEEWAY;
        az += nz * press * SAIL_THRUST + wz * SAIL_LEEWAY;
    } else {
        ax += wx * HULL_LEEWAY;
        az += wz * HULL_LEEWAY;
    }
    let (cx, cz) = current(body, block);
    body.vx += (ax + cx) * dt;
    body.vz += (az + cz) * dt;

    // **Which way the water itself is going**: out in the open sea, the
    // circulation (`fluid::current_at`), and down a river, the river's own
    // current (`WorldGen::river_current`). Nothing at all in a pond or the
    // shallows, so everything below is exactly what it was there -- and a
    // raft left on a river goes where the river goes, which is what a raft
    // on a river is for: downstream for nothing, and upstream against a
    // rapid not at all.
    let (sea_x, sea_z) = sea_drift(body, block, world_days);
    let river = if river.0.is_finite() && river.1.is_finite() { river } else { (0.0, 0.0) };
    let (sx, sz) = (sea_x + river.0, sea_z + river.1);

    // Drag split along and across the deck (see `DRAG_ACROSS`) -- and
    // **measured against the water rather than against the ground**.
    //
    // That distinction is the whole of how a current carries anything. Drag
    // is friction between a hull and the water it sits in; a raft already
    // moving with the water feels none of it. Taken against the ground, as
    // it was, the drag would fight the current every tick and a raft let go
    // in mid-ocean would come to a halt in water that is itself moving --
    // which is the one thing a player would call broken. Subtracted first
    // and added back after, a raft nobody is rowing ends up going where the
    // sea goes, and a rower feels the sea as ground that slides.
    let (vx, vz) = (body.vx - sx, body.vz - sz);
    let along = (vx * fx + vz * fz) * (-DRAG_ALONG * dt).exp();
    let across = (vx * rx + vz * rz) * (-DRAG_ACROSS * dt).exp();
    body.vx = fx * along + rx * across + sx;
    body.vz = fz * along + rz * across + sz;
    body.spin = (body.spin + oars.turn * ROW_TURN * dt) * (-SPIN_DRAG * dt).exp();

    if !fits(body.x, body.y, body.z, body.yaw, block) {
        body.vx = 0.0;
        body.vz = 0.0;
        body.spin = 0.0;
        settle(body, block, dt);
        return true;
    }

    let mut struck = false;
    let x = body.x + f64::from(body.vx * dt);
    if fits(x, body.y, body.z, body.yaw, block) {
        body.x = x;
    } else {
        body.vx = 0.0;
        struck = true;
    }
    let z = body.z + f64::from(body.vz * dt);
    if fits(body.x, body.y, z, body.yaw, block) {
        body.z = z;
    } else {
        body.vz = 0.0;
        struck = true;
    }
    if body.spin != 0.0 {
        let yaw = (body.yaw + body.spin * dt).rem_euclid(std::f32::consts::TAU);
        if fits(body.x, body.y, body.z, yaw, block) {
            body.yaw = yaw;
        } else {
            body.spin = 0.0;
            struck = true;
        }
    }
    settle(body, block, dt);
    struck
}

/// Eases the waterline toward the surface under the middle of the raft.
///
/// Eased rather than snapped, because the surface steps by an eighth where
/// water is running, and a deck that jumped an eighth of a block would jolt
/// everyone standing on it.
fn settle<F>(body: &mut Body, block: &F, dt: f32)
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let Some(surface) = surface_under(body.x, body.y, body.z, block) else {
        return;
    };
    let y = body.y + (surface - body.y) * (1.0 - (-SETTLE_PER_SECOND * dt).exp());
    if (y - body.y).abs() > 1e-5 && fits(body.x, y, body.z, body.yaw, block) {
        body.y = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_SAND, BLOCK_STONE};
    use std::f32::consts::{FRAC_PI_2, PI};

    /// A lake sixty-four blocks square, one cell deep at y = 10, with a
    /// beach of sand round it and stone under both.
    fn lake(x: i32, y: i32, z: i32) -> Option<BlockId> {
        let inside = (0..64).contains(&x) && (0..64).contains(&z);
        Some(match y {
            y if y < 10 => BLOCK_STONE,
            10 if inside => fluid::with_depth(fluid::SOURCE_DEPTH),
            10 => BLOCK_SAND,
            _ => BLOCK_AIR,
        })
    }

    fn afloat(x: f32, z: f32, yaw: f32) -> Body {
        launch([f64::from(x), 10.5, f64::from(z)], yaw, &lake).expect("the middle of a lake has room for a raft")
    }

    fn run(body: &mut Body, oars: Oars, wind: Wind, seconds: f32) {
        for _ in 0..(seconds * 20.0) as u32 {
            // Hour nought, and the lake is a cell deep: the sea's own
            // circulation is nothing in either (`fluid::current_at`), so
            // every test on this helper measures what it always measured.
            step(body, oars, wind, 0.0, (0.0, 0.0), &lake, 0.05);
        }
    }

    /// The open sea: water from the surface down past the depth at which
    /// the circulation reaches full strength.
    fn ocean(_x: i32, y: i32, _z: i32) -> Option<BlockId> {
        Some(match y {
            y if y < -30 => BLOCK_STONE,
            y if y <= 10 => fluid::with_depth(fluid::SOURCE_DEPTH),
            _ => BLOCK_AIR,
        })
    }

    #[test]
    fn a_raft_let_go_in_the_open_sea_ends_up_going_where_the_sea_goes() {
        // **"Добавь течение в море".** The sea is the frame the hull's drag
        // is measured against, so a raft nobody is rowing ends up at the
        // water's own speed rather than stopping dead in water that is
        // itself moving. Two minutes is long enough for the drag to settle
        // (`DRAG_ALONG`), which is the point being made.
        const HOUR: f32 = 3.5;
        let mut raft = launch([1_500.0, 10.5, 900.0], 0.0, &ocean).expect("the open sea has room for a raft");
        for _ in 0..(120.0 * 20.0) as u32 {
            step(&mut raft, Oars::REST, Wind::CALM, HOUR, (0.0, 0.0), &ocean, 0.05);
        }
        let (want_x, want_z) = fluid::current_at(raft.x as f32, raft.z as f32, 31.0, HOUR);
        let wanted = want_x.hypot(want_z);
        assert!(wanted > 0.1, "the test picked a slack-water spot: {wanted}");
        let carried = (raft.vx - want_x).hypot(raft.vz - want_z);
        assert!(
            carried < wanted * 0.2,
            "the raft is going {:?} where the sea goes ({want_x}, {want_z})",
            (raft.vx, raft.vz)
        );
    }

    /// **"Быстрые реки с течением": a raft on a river goes downstream.** Let
    /// go on a river running at a walking pace it is carried at the river's
    /// own speed; rowed hard against a rapid it still loses ground, which is
    /// what makes a rapid a place to carry a raft round rather than row up.
    #[test]
    fn a_raft_let_go_on_a_river_drifts_downstream_and_cannot_be_rowed_up_a_rapid() {
        // The lake is one cell deep, so the sea's circulation is nothing and
        // every metre moved is the river's.
        let lazy = (0.7f32, 0.0f32);
        let mut raft = afloat(20.5, 32.5, 0.0);
        let start = raft.x;
        for _ in 0..(60.0 * 20.0) as u32 {
            step(&mut raft, Oars::REST, Wind::CALM, 0.0, lazy, &lake, 0.05);
        }
        assert!((raft.vx - lazy.0).abs() < lazy.0 * 0.2, "a raft let go on a river runs at {} with the river at {}", raft.vx, lazy.0);
        assert!(raft.x > start + 20.0, "a minute on a river carried the raft {} blocks", raft.x - start);

        // Bow upstream into a rapid, both oars pulling.
        let rapid = (3.2f32, 0.0f32);
        let mut raft = afloat(40.5, 32.5, PI);
        let start = raft.x;
        for _ in 0..(20.0 * 20.0) as u32 {
            step(&mut raft, Oars { stroke: 1.0, turn: 0.0 }, Wind::CALM, 0.0, rapid, &lake, 0.05);
        }
        assert!(raft.x > start, "a raft rowed up a rapid made {} blocks against it", start - raft.x);
    }

    #[test]
    fn a_raft_in_the_shallows_still_comes_to_a_stop() {
        // ...and the other half: a current in a pond, a river mouth or a
        // bay would drag a moored raft off a shore it was tied to. The lake
        // is one cell deep, which is every water a player builds beside.
        const HOUR: f32 = 3.5;
        let mut raft = afloat(32.5, 32.5, 0.0);
        raft.vx = 1.0;
        for _ in 0..(60.0 * 20.0) as u32 {
            step(&mut raft, Oars::REST, Wind::CALM, HOUR, (0.0, 0.0), &lake, 0.05);
        }
        let speed = raft.vx.hypot(raft.vz);
        assert!(speed < 0.01, "a raft in a pond is still moving at {speed} blocks a second");
    }

    #[test]
    fn a_raft_cannot_be_launched_on_dry_land() {
        assert!(launch([80.5, 10.5, 80.5], 0.0, &lake).is_none(), "a raft was launched onto sand");
        let raft = afloat(32.5, 32.5, 0.0);
        let surface = 10.0 + fluid::surface_height(fluid::with_depth(fluid::SOURCE_DEPTH));
        assert!((raft.y - surface).abs() < 1e-4, "the raft does not float at the surface: {}", raft.y);
    }

    #[test]
    fn a_raft_launched_at_the_waters_edge_is_pushed_out_along_the_look_rather_than_refused() {
        // Aimed at the first cell of water from the western beach, looking
        // out over the lake: the raft goes onto the water beyond it.
        let raft = launch([0.5, 10.5, 32.5], 0.0, &lake).expect("the edge of a lake refused a raft");
        assert!(raft.x - f64::from(HALF_LENGTH) >= 0.0, "the raft is up the beach: {raft:?}");
        // ...and looking back at the beach, there is nowhere to put it.
        assert!(launch([0.5, 10.5, 32.5], PI, &lake).is_none(), "a raft was launched onto the shore");
    }

    #[test]
    fn rowing_moves_the_raft_and_a_sail_downwind_moves_it_faster_than_upwind() {
        let calm_row = {
            let mut raft = afloat(8.0, 32.5, 0.0);
            run(&mut raft, Oars { stroke: 1.0, turn: 0.0 }, Wind::CALM, 6.0);
            raft.x - 8.0
        };
        assert!(calm_row > 6.0, "six seconds at the oars moved the raft {calm_row} blocks");

        let wind_behind = Wind { toward: 0.0, strength: 0.45 };
        let wind_ahead = Wind { toward: PI, strength: 0.45 };
        let sail = |wind: Wind, stroke: f32| {
            let mut raft = afloat(20.0, 32.5, 0.0);
            raft.sail = true;
            run(&mut raft, Oars { stroke, turn: 0.0 }, wind, 6.0);
            raft.x - 20.0
        };
        let downwind = sail(wind_behind, 0.0);
        let upwind = sail(wind_ahead, 0.0);
        assert!(downwind > 6.0, "a fair wind behind the sail moved the raft {downwind} blocks");
        assert!(upwind < 0.0, "a sail held up to a head wind still drove the raft forward: {upwind}");
        assert!(downwind > calm_row, "the sail downwind was no faster than rowing");

        // The decision: into the wind, the sail is worse than furling it.
        let rowing_with_sail_up = sail(wind_ahead, 1.0);
        let rowing_furled = {
            let mut raft = afloat(20.0, 32.5, 0.0);
            run(&mut raft, Oars { stroke: 1.0, turn: 0.0 }, wind_ahead, 6.0);
            raft.x - 20.0
        };
        assert!(
            rowing_furled > rowing_with_sail_up + 1.0,
            "rowing into the wind with the sail up ({rowing_with_sail_up}) was not slower than furled ({rowing_furled})"
        );
    }

    #[test]
    fn a_raft_stops_at_a_shore() {
        let mut raft = afloat(56.0, 32.5, 0.0);
        run(&mut raft, Oars { stroke: 1.0, turn: 0.0 }, Wind::CALM, 20.0);
        assert!(raft.x + f64::from(HALF_LENGTH) <= 64.0 + 1e-3, "the raft ran up the beach: {raft:?}");
        assert!(raft.x + f64::from(HALF_LENGTH) > 63.0, "the raft stopped short of the shore: {raft:?}");
        assert!(fits(raft.x, raft.y, raft.z, raft.yaw, &lake), "the raft ended inside the bank");
        // And it cannot be turned into the bank it is against.
        run(&mut raft, Oars { stroke: 1.0, turn: 1.0 }, Wind::CALM, 5.0);
        assert!(fits(raft.x, raft.y, raft.z, raft.yaw, &lake), "turning pushed the raft into the bank");
    }

    #[test]
    fn a_raft_left_alone_drifts_downwind_but_slowly() {
        let mut raft = afloat(32.5, 32.5, FRAC_PI_2);
        run(&mut raft, Oars::REST, Wind { toward: 0.0, strength: 1.0 }, 10.0);
        let moved = raft.x - 32.5;
        assert!(moved > 0.05, "an unattended raft did not drift with a storm: {moved}");
        assert!(moved < 2.0, "an unattended raft with its sail down raced off: {moved}");
    }

    #[test]
    fn a_point_on_the_deck_stays_on_the_deck_as_the_raft_moves_and_turns() {
        let mut raft = afloat(32.5, 32.5, 0.3);
        let rider = [0.8, 0.0, -0.4];
        for _ in 0..40 {
            step(&mut raft, Oars { stroke: 1.0, turn: 1.0 }, Wind::CALM, 0.0, (0.0, 0.0), &lake, 0.05);
            let world = raft.world_of(rider);
            let back = raft.local_of(world);
            for axis in 0..3 {
                assert!((back[axis] - rider[axis]).abs() < 1e-3, "{back:?} is not {rider:?}");
            }
        }
        assert!(Body::over_deck(rider, 0.0));
        assert!(!Body::over_deck([HALF_LENGTH + 0.5, 0.0, 0.0], DECK_SLACK));
    }

    #[test]
    fn the_wind_is_one_wind_for_everyone_at_the_same_hour_and_veers_rather_than_jumps() {
        for day in [0.0f32, 1.3, 17.25, 300.9] {
            assert_eq!(wind(day, Weather::Rain), wind(day, Weather::Rain));
            let (a, b) = (wind(day, Weather::Clear), wind(day + 0.001, Weather::Clear));
            let turned = (a.toward - b.toward).rem_euclid(std::f32::consts::TAU);
            let turned = turned.min(std::f32::consts::TAU - turned);
            assert!(turned < 0.05, "the wind swung {turned} radians in a minute and a half");
            assert!(wind(day, Weather::Storm).strength > wind(day, Weather::Clear).strength);
        }
        // ...and it never stops veering smoothly, not only at four chosen
        // hours: a step in the noise that was blended straight rather than
        // eased would pass the four above and flick on every whole hour
        // between them.
        let mut worst: f32 = 0.0;
        let mut day = 0.0f32;
        while day < 40.0 {
            let (a, b) = (wind(day, Weather::Storm), wind(day + 0.001, Weather::Storm));
            let turned = (a.toward - b.toward).rem_euclid(std::f32::consts::TAU);
            worst = worst.max(turned.min(std::f32::consts::TAU - turned));
            day += 0.001;
        }
        assert!(worst < 0.05, "somewhere in forty days the wind jumped {worst} radians in a minute and a half");
    }

    #[test]
    fn the_wind_boxes_the_compass_and_does_not_come_round_again_on_a_timetable() {
        // The fault this replaces: two sine waves have a period, and a
        // period is a timetable a player learns. "The crossing you cannot
        // make at noon is the one you can always make at dusk" is the thing
        // that must not be true.
        let toward = |day: f32| wind(day, Weather::Clear).toward;
        let mut sectors = [false; 8];
        let mut day = 0.0f32;
        while day < 60.0 {
            sectors[(toward(day) / std::f32::consts::TAU * 8.0) as usize % 8] = true;
            day += 0.01;
        }
        assert!(sectors.iter().all(|&seen| seen), "in sixty days the wind never blew from some quarters: {sectors:?}");
        // No period worth having: the same hour of two different days, and
        // of two different weeks, is not the same wind. A sine pair with a
        // period of a day or a week would fail at whichever it had.
        for span in [1.0f32, 2.0, 7.0] {
            let mut same = 0;
            for step in 0..200 {
                let day = step as f32 * 0.17;
                let apart = (toward(day) - toward(day + span)).abs();
                if apart.min(std::f32::consts::TAU - apart) < 0.1 {
                    same += 1;
                }
            }
            assert!(same < 60, "the wind {span} days later was the same wind {same} times in 200");
        }
    }

    #[test]
    fn the_wind_brings_calms_and_squalls_rather_than_one_steady_breeze() {
        // A wind that is always three quarters of a gale is weather
        // wallpaper. What makes it an event is that it drops to nothing and
        // gets up again, and that neither is on a timetable.
        let mut calm = 0;
        let mut squall = 0;
        let mut day = 0.0f32;
        while day < 30.0 {
            let strength = wind(day, Weather::Clear).strength;
            if strength < 0.12 {
                calm += 1;
            }
            if strength > 0.55 {
                squall += 1;
            }
            day += 0.01;
        }
        assert!(calm > 50, "in thirty days there were {calm} sampled moments of calm");
        assert!(squall > 50, "in thirty days there were {squall} sampled moments of hard wind");
    }

    #[test]
    fn a_storms_wind_is_always_harder_than_a_fair_days_at_the_same_hour() {
        // The lull multiplies the weather rather than being added to it, so
        // the three skies keep their order at every hour -- including the
        // hours the lull is at one end of itself. Added, a gust under a
        // clear sky would out-blow a lull under a storm, and "it is blowing
        // a gale" would stop meaning anything about the sky.
        let mut day = 0.0f32;
        while day < 20.0 {
            let (clear, rain, storm) = (
                wind(day, Weather::Clear).strength,
                wind(day, Weather::Rain).strength,
                wind(day, Weather::Storm).strength,
            );
            assert!(clear < rain && rain < storm, "at day {day}: clear {clear}, rain {rain}, storm {storm}");
            assert!((0.0..=1.0).contains(&storm), "a storm's wind is off the scale at {storm}");
            day += 0.013;
        }
    }

    #[test]
    fn a_sail_braced_across_the_wind_catches_it_and_a_sail_along_the_wind_does_not() {
        // The player's own words for what the angle is for: "поперёк ветра
        // ловит его, вдоль -- не ловит".
        let following = Wind { toward: 0.0, strength: 1.0 };
        let sailed = |angle: f32| {
            let mut raft = afloat(20.0, 32.5, 0.0);
            raft.sail = true;
            raft.sail_angle = angle;
            run(&mut raft, Oars::REST, following, 6.0);
            raft.x - 20.0
        };
        let square = sailed(0.0);
        let braced = sailed(SAIL_MAX_ANGLE);
        assert!(square > 6.0, "a sail square across a following wind moved the raft {square} blocks");
        assert!(braced < square * 0.4, "bracing the yard along the wind still drove the raft {braced} blocks of {square}");
    }

    #[test]
    fn there_is_no_trim_that_sails_a_raft_into_the_wind() {
        // A flat sheet's forward push is cos(angle) of a number that is
        // itself cos(angle) of the head wind, so it is never positive --
        // and a player who could find an angle that was would never furl a
        // sail again, which is half of the raft's decisions gone.
        let head = Wind { toward: PI, strength: 1.0 };
        for step in -12..=12 {
            let angle = step as f32 / 12.0 * SAIL_MAX_ANGLE;
            let mut raft = afloat(32.5, 32.5, 0.0);
            raft.sail = true;
            raft.sail_angle = angle;
            run(&mut raft, Oars::REST, head, 8.0);
            assert!(raft.x <= 32.5 + 1e-3, "a sail at {angle} radians sailed {} blocks into the wind", raft.x - 32.5);
        }
    }

    #[test]
    fn across_the_wind_the_yard_braced_round_is_the_difference_between_going_and_drifting() {
        // The reach, which is where the angle earns its place: with the
        // wind on the beam a square sail feels nothing on its face at all
        // and the raft only slides sideways, while the same sail braced
        // halfway round drives it forward.
        let beam = Wind { toward: FRAC_PI_2, strength: 1.0 };
        let sailed = |angle: f32| {
            let mut raft = afloat(20.0, 24.0, 0.0);
            raft.sail = true;
            raft.sail_angle = angle;
            run(&mut raft, Oars::REST, beam, 6.0);
            raft.x - 20.0
        };
        let square = sailed(0.0);
        let reach = sailed(0.7);
        assert!(square.abs() < 0.5, "a square sail with the wind abeam drove the raft {square} blocks along its own heading");
        assert!(reach > 2.0, "the yard braced round on a beam wind moved the raft {reach} blocks");
    }

    #[test]
    fn a_sail_angle_off_a_socket_cannot_brace_the_yard_through_its_own_mast() {
        assert_eq!(trim_clamped(40.0), SAIL_MAX_ANGLE);
        assert_eq!(trim_clamped(-40.0), -SAIL_MAX_ANGLE);
        assert_eq!(trim_clamped(f32::NAN), 0.0);
        // ...and the rule that moves the raft clamps too, so a body that
        // got an absurd angle past the wire still sails like a braced one
        // rather than filling the world with NaN.
        let wind = Wind { toward: 0.0, strength: 1.0 };
        let sailed = |angle: f32| {
            let mut raft = afloat(20.0, 32.5, 0.0);
            raft.sail = true;
            raft.sail_angle = angle;
            run(&mut raft, Oars::REST, wind, 4.0);
            assert!(raft.is_sane(), "a raft with a {angle} radian sail stopped being a number");
            raft.x - 20.0
        };
        assert!((sailed(40.0) - sailed(SAIL_MAX_ANGLE)).abs() < 1e-3);
    }

    #[test]
    fn a_hand_at_the_mast_reaches_the_sheets_and_one_at_the_stern_does_not() {
        assert!(at_the_sail([MAST_ALONG, 0.0, 0.0]), "standing at the mast is not at the sail");
        assert!(at_the_sail([MAST_ALONG, 1.0, 0.8]), "a jump beside the mast is not at the sail");
        assert!(!at_the_sail(SEAT), "the rower reaches the sheets from the stern by hand");
        assert!(!at_the_sail([-HALF_LENGTH, 0.0, -HALF_WIDTH]), "a corner of the stern is at the sail");
    }

    #[test]
    fn oars_off_a_socket_cannot_row_harder_than_a_pair_of_arms() {
        let wild = Oars { stroke: 40.0, turn: f32::NAN }.clamped();
        assert_eq!(wild, Oars { stroke: 1.0, turn: 0.0 });
        let mut honest = afloat(20.0, 32.5, 0.0);
        let mut liar = honest;
        run(&mut honest, Oars { stroke: 1.0, turn: 0.0 }, Wind::CALM, 3.0);
        run(&mut liar, Oars { stroke: 40.0, turn: 0.0 }, Wind::CALM, 3.0);
        assert_eq!(honest, liar);
    }
}
