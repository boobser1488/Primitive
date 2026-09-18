//! A horse under a rider: the body the rider's keys move, and the rules both
//! ends move it by.
//!
//! ## Why this is shared, and what it is the twin of
//!
//! **The raft's rower, said again on land** (`raft`, the client's `riding`,
//! the server's `rafts`). The rider's client moves its horse on the frame a
//! key is pressed, through [`step`]; the server moves *its* horse through the
//! same [`step`] with the reins the rider last sent (`ClientMessage::Rein`),
//! and the client eases its prediction toward the server's horse rather than
//! snapping to it. A rule written twice is a rule the two sides disagree
//! about, and a horse the client gallops one way and the server another is a
//! horse that rubber-bands on every snapshot.
//!
//! What the server does *not* do is take the rider's word for where they
//! are. The rider is put on the server's horse every tick (the saddle,
//! [`RIDER_LIFT`] above its back), exactly as a rower is put on the seat of
//! the server's raft -- so a modified client can send whatever reins it likes
//! and still only ever goes where a horse can go. The anti-cheat's part is the
//! rider's own transforms, which it still reads with a mounted allowance
//! (`anticheat::Mount`): a galloping rider is inside it, a flying one is not.
//!
//! ## The decision a horse is
//!
//! **Distance and weight, for a mouth and a roof.** On foot a player walks at
//! four blocks a second and carries ninety kilos; on a horse they cross the
//! plain at eleven and the saddlebags carry the ore home. What that costs is
//! everything a horse needs: a gallop spends its wind ([`GALLOP_SECONDS`]),
//! the load in the bags slows it ([`load_factor`]), a hungry horse will not
//! gallop at all and a horse kept out in the rain loses condition
//! (`husbandry`), and deep water is a bank it will not go down ([`REFUSES_DEEPER`]).
//! A horse is the fastest way anywhere that has grass and a ford -- and the
//! slowest way up a mountain, because a slope takes its pace a step at a time
//! ([`CLIMB_KEEPS`]).

use serde::{Deserialize, Serialize};

use crate::fluid;
use crate::geometry::block_box;
use crate::types::{is_liquid, BlockId, BLOCK_AIR};

// ---- the tack byte (`EntityKind::Animal::tack`) ----

/// Wearing a saddle.
pub const TACK_SADDLE: u8 = 1;
/// Wearing saddlebags.
pub const TACK_BAGS: u8 = 2;
/// Somebody is on it.
pub const TACK_RIDDEN: u8 = 4;
/// Tame: a halter on its head, which is what tells a player's horse from a
/// wild one across a field.
pub const TACK_HALTER: u8 = 8;
/// The stallion of a wild herd (the server's `stallion`): drawn with the
/// heavier neck and the darker mane, because the herd's keeper is the one a
/// player should be able to pick out.
pub const TACK_STALLION: u8 = 16;

// ---- the gaits ----

/// Blocks a second at a walk: under a walking player, the pace of a horse
/// that is being led or picking its way.
pub const WALK: f32 = 3.0;
/// At a trot: a little under a sprinting player, and a pace the horse holds
/// all day ([`STAMINA_BACK_TROTTING`] is more than nothing).
pub const TROT: f32 = 6.0;
/// **At a gallop: well over anything on foot in this world**, the bear
/// included (`Species::run_speed`, 7.0), which is the one number that makes
/// a horse a way out as well as a way across. Paid for in wind.
pub const GALLOP: f32 = 11.0;

/// Seconds of gallop in a fresh, well-kept horse.
///
/// **Half a minute, then a trot.** Three hundred blocks at a gallop is a
/// long way to be chased and a short way to cross a country, so the gallop is
/// something a rider *spends* -- on the wolves, on the last stretch home --
/// and the trot is how distance is covered. Longer and the trot would never be
/// chosen; shorter and the gallop would be a sprint key with a horse drawn
/// round it.
pub const GALLOP_SECONDS: f32 = 30.0;
/// Wind back a second at a walk or standing.
pub const STAMINA_BACK_WALKING: f32 = 0.5;
/// ...and at a trot, a little: a trot is the pace a horse *rests* at on the
/// move, which is what makes it the pace a journey is made at.
pub const STAMINA_BACK_TROTTING: f32 = 0.15;
/// What a jump costs in wind, in seconds of gallop.
pub const JUMP_COST: f32 = 1.0;
/// Wind a blown horse needs back before it will gallop again.
///
/// **Three seconds, so blown means blown.** Without it a horse whose wind
/// ran out galloped again the moment a tenth of a second came back, and a
/// rider holding the gallop got a horse lurching between a trot and a gallop
/// five times a second -- the gauge flickering at empty rather than a horse
/// that has to be walked.
pub const GALLOP_RESUME: f32 = 3.0;

/// How fast it gets up to the pace asked for, in blocks a second a second.
pub const ACCELERATION: f32 = 5.0;
/// ...and down from it, which is quicker: a horse stops sooner than it starts.
pub const BRAKING: f32 = 9.0;
/// How fast it turns at a walk, in radians a second.
pub const TURN_WALKING: f32 = 2.4;
/// ...and at a full gallop. **A horse turns wide**, and at eleven blocks a
/// second it has to: a rider who could spin a galloping horse on the spot
/// would be steering a cursor.
pub const TURN_GALLOPING: f32 = 1.1;
/// How much of any sideways slide is gone each second: the hooves grip.
pub const GRIP: f32 = 10.0;

/// How high a jump lifts its feet, in blocks: a block and a bit, so a
/// one-block wall or bank is cleared and a two-block wall is not.
pub const JUMP_HEIGHT: f32 = 1.25;
/// Gravity, the animals' own (the server's `logic::animals::GRAVITY`).
pub const GRAVITY: f32 = -22.0;
/// The fastest it falls.
pub const TERMINAL: f32 = -30.0;
/// The highest rise it walks up without a jump, in blocks.
///
/// **A block, like every animal here** (`logic::animals::STEP_HEIGHT`): the
/// ground in this world is a staircase, and a horse that needed a jump for
/// every step of a hillside would be a jump key held down all the way up a
/// hill. What a step costs instead is pace -- see [`CLIMB_KEEPS`].
pub const STEP_UP: f32 = 1.05;
/// What is left of its pace after walking up a step.
///
/// **This is the slope.** Each block of rise takes a third of the speed off,
/// so a gallop up a hill is a trot by the third step and a walk by the fifth,
/// and a horse that is asked to gallop up it spends wind it does not turn
/// into ground. A jump does not pay this: that is what a jump is for.
pub const CLIMB_KEEPS: f32 = 0.65;

/// Water past this, in blocks above its hooves, and it will not go in.
///
/// **A horse refuses deep water**: a ford to its belly is a ford, and a river
/// to its back is a river it stops at the edge of, whatever the rider asks.
/// So a river is a detour to a ford, or the raft -- which is the decision the
/// water is for. A horse already out of its depth (fallen in, stood in a
/// rising flood) is not stuck: it swims, slowly, and may go wherever takes it
/// shallower.
pub const REFUSES_DEEPER: f32 = 1.1;
/// Its pace wading, as a fraction.
pub const WADING: f32 = 0.45;
/// Its pace swimming, in blocks a second.
pub const SWIMMING: f32 = 1.2;

/// Half its footprint, in blocks: `Species::Horse.width()` over two.
pub const HALF_WIDTH: f32 = 0.4;
/// Its collider's height: `Species::Horse.height()`.
pub const HEIGHT: f32 = 1.6;
/// **Where a rider's feet are put, above the horse's**: the saddle is at
/// about a block and a half, and a seated figure's hips are
/// `player_model::SEAT_DROP` under a standing one's. A rider's position is
/// therefore the horse's plus this, and the eye is where a seated eye is
/// above that -- a little over two blocks up, which is half of what a horse
/// is for on a plain.
pub const RIDER_LIFT: f32 = 0.95;
/// How far a player may be from a horse and still get on it, in blocks,
/// measured by the server from where it has them to the horse's middle.
pub const MOUNT_REACH: f32 = 3.0;

// ---- the bags ----

/// Slots in a pair of saddlebags.
///
/// **Twelve, not forty.** A chest's forty on a horse would make the horse a
/// chest that walks, and then the trip is made once; twelve stacks is a
/// trip's ore and some food, and the weight ([`load_factor`]) is what
/// decides how much of it the horse carries at a pace worth riding.
pub const BAGS_SLOTS: usize = 12;
/// Kilos in the bags a horse carries without noticing.
pub const EASY_LOAD_KG: f32 = 40.0;
/// Kilos at which it is down to a walk, whatever it is asked.
///
/// A hundred and sixty: nearly twice what a person carries
/// (`load::CARRY_CAPACITY_KG`), so a laden horse still brings home more than
/// its rider could -- slowly.
pub const HEAVY_LOAD_KG: f32 = 160.0;

/// How much of its pace a horse keeps under `kg` in its bags: one up to
/// [`EASY_LOAD_KG`], then down in a straight line to a walk's share of a
/// gallop at [`HEAVY_LOAD_KG`].
///
/// **Pace, and not a refusal.** A horse that would not move at a hundred and
/// sixty-one kilos would be an inventory rule with a horse drawn round it; a
/// horse at a walk under a load of copper is a choice about how much copper.
pub fn load_factor(kg: f32) -> f32 {
    if !kg.is_finite() || kg <= EASY_LOAD_KG {
        return 1.0;
    }
    let floor = WALK / GALLOP;
    let over = ((kg - EASY_LOAD_KG) / (HEAVY_LOAD_KG - EASY_LOAD_KG)).clamp(0.0, 1.0);
    1.0 - over * (1.0 - floor)
}

/// What the rider is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reins {
    /// -1 (backing) to 1 (on).
    pub forward: f32,
    /// -1 (left) to 1 (right).
    pub turn: f32,
    /// The pace asked for while `forward` is held. See [`Gait`].
    pub gait: Gait,
    /// Jump, if it has its feet under it.
    pub jump: bool,
}

impl Reins {
    /// Nobody asking for anything: it stands.
    pub const SLACK: Reins = Reins { forward: 0.0, turn: 0.0, gait: Gait::Walk, jump: false };

    /// The same, within what a rider's hands can ask. Off a socket, for the
    /// reason `raft::Oars::clamped` gives: a forward of ten is a horse at a
    /// hundred blocks a second.
    pub fn clamped(self) -> Reins {
        let tame = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        Reins { forward: tame(self.forward), turn: tame(self.turn), gait: self.gait, jump: self.jump }
    }
}

/// How fast the rider has asked it to go.
///
/// Three, and the rider picks: the walk key held with the sneak key walks, on
/// its own trots, with the sprint key gallops. **Three rather than a speed
/// that rises the longer the key is held**, because each is a different
/// promise -- the walk is quiet, the trot is free, the gallop costs wind --
/// and a rider has to be able to *choose* which one they are making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Gait {
    #[default]
    Walk,
    Trot,
    Gallop,
}

impl Gait {
    /// Its pace, in blocks a second.
    pub fn speed(self) -> f32 {
        match self {
            Gait::Walk => WALK,
            Gait::Trot => TROT,
            Gait::Gallop => GALLOP,
        }
    }

    /// One byte on the wire.
    pub fn to_wire(self) -> u8 {
        self as u8
    }

    /// ...and back; anything unknown is a walk, the one reading that cannot
    /// run a horse into a river.
    pub fn from_wire(byte: u8) -> Gait {
        match byte {
            1 => Gait::Trot,
            2 => Gait::Gallop,
            _ => Gait::Walk,
        }
    }
}

/// A horse's body while somebody rides it: where it is and how it moves.
///
/// `x`, `y`, `z` are its feet, at the middle of its footprint, in `f64` for
/// the reason every body here is (`raft::Body`, the server's `Animal`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mount {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Which way its nose points, in the `Camera::forward` convention.
    pub yaw: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub on_ground: bool,
    /// Seconds of gallop left.
    pub wind: f32,
}

/// What the horse brings to a ride that is not the ride itself: how much
/// wind it has at best, whether it will gallop, and what it carries.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fettle {
    /// Its most wind, in seconds of gallop: [`GALLOP_SECONDS`] for a horse in
    /// condition, less for one that has gone without (`husbandry`).
    pub most_wind: f32,
    /// Whether it will gallop at all: a hungry horse will not.
    pub will_gallop: bool,
    /// Kilos in its bags.
    pub load_kg: f32,
}

impl Fettle {
    /// A fresh horse with empty bags.
    pub const FRESH: Fettle = Fettle { most_wind: GALLOP_SECONDS, will_gallop: true, load_kg: 0.0 };

    /// Off a socket: in range, and a number.
    pub fn clamped(self) -> Fettle {
        let most_wind = if self.most_wind.is_finite() { self.most_wind.clamp(0.0, GALLOP_SECONDS) } else { 0.0 };
        let load_kg = if self.load_kg.is_finite() { self.load_kg.max(0.0) } else { 0.0 };
        Fettle { most_wind, will_gallop: self.will_gallop, load_kg }
    }
}

impl Mount {
    /// Standing still where it is.
    pub fn standing(x: f64, y: f64, z: f64, yaw: f32, wind: f32) -> Mount {
        Mount { x, y, z, yaw, vx: 0.0, vy: 0.0, vz: 0.0, on_ground: true, wind }
    }

    /// Its speed over the ground, in blocks a second.
    pub fn speed(&self) -> f32 {
        self.vx.hypot(self.vz)
    }

    /// Where a rider's feet are.
    pub fn saddle(&self) -> [f64; 3] {
        [self.x, self.y + f64::from(RIDER_LIFT), self.z]
    }

    /// The unit vector its nose points along, as (x, z).
    #[inline]
    pub fn forward(&self) -> (f32, f32) {
        let (sin, cos) = self.yaw.sin_cos();
        (cos, sin)
    }

    /// Everything about it is a number, checked on anything off a socket.
    pub fn is_sane(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
            && [self.yaw, self.vx, self.vy, self.vz, self.wind].iter().all(|v| v.is_finite())
    }
}

/// Whether a horse's box fits with its feet at this point: nothing solid
/// anywhere in it.
///
/// **A cell nobody has loaded is solid.** On the server that stops a horse at
/// the edge of the world anyone has seen, and on the client it stops the
/// prediction there rather than galloping it into terrain the server will
/// then find it inside -- the raft's rule (`raft::fits`).
pub fn fits<F>(x: f64, y: f64, z: f64, block: &F) -> bool
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let lo = [x - f64::from(HALF_WIDTH), y, z - f64::from(HALF_WIDTH)];
    let hi = [x + f64::from(HALF_WIDTH), y + f64::from(HEIGHT), z + f64::from(HALF_WIDTH)];
    for cx in lo[0].floor() as i32..=(hi[0] - 1e-6).floor() as i32 {
        for cz in lo[2].floor() as i32..=(hi[2] - 1e-6).floor() as i32 {
            for cy in (lo[1] + 1e-6).floor() as i32 - 1..=(hi[1] - 1e-6).floor() as i32 {
                let Some(cell) = block(cx, cy, cz) else {
                    return false;
                };
                // Asked in the cell's own frame, for
                // `geometry::for_each_block_box_f64`'s reason: a box built out
                // in the world has no halves left far away.
                let Some((bl, bh)) = block_box(cell, 0, 0, 0) else {
                    continue;
                };
                let (ox, oy, oz) = (f64::from(cx), f64::from(cy), f64::from(cz));
                let overlaps = |l: f64, h: f64, bl: f32, bh: f32, o: f64| l < o + f64::from(bh) - 1e-4 && h > o + f64::from(bl) + 1e-4;
                if overlaps(lo[0], hi[0], bl[0], bh[0], ox)
                    && overlaps(lo[1], hi[1], bl[1], bh[1], oy)
                    && overlaps(lo[2], hi[2], bl[2], bh[2], oz)
                {
                    return false;
                }
            }
        }
    }
    true
}

/// How deep the water is over a horse's hooves standing at this point, in
/// blocks: nought on dry ground.
///
/// Read off the column at the middle of its footprint, to the drawn surface
/// (`fluid::surface_height_with_above`) -- the same line the mesher draws and
/// the swimmer swims at, so the depth a horse refuses is the depth a player
/// can see.
pub fn water_over<F>(x: f64, y: f64, z: f64, block: &F) -> f32
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (x.floor() as i32, z.floor() as i32);
    // A hair up, because hooves resting on a bed are at a whole number and
    // the settling that put them there can leave them a hair under it.
    let foot = (y + 0.05).floor() as i32;
    let mut surface = None;
    for cy in foot..foot + 4 {
        let here = block(ix, cy, iz).unwrap_or(BLOCK_AIR);
        if !is_liquid(here) {
            break;
        }
        let above = block(ix, cy + 1, iz).unwrap_or(BLOCK_AIR);
        surface = Some(cy as f64 + f64::from(fluid::surface_height_with_above(here, above)));
    }
    surface.map_or(0.0, |top| (top - y).max(0.0) as f32)
}

/// How deep the water is in the column at this point, bed to surface, looking
/// round `near_y`: nought where there is none.
///
/// **The whole column, and not what is over the hooves**, because the bank of
/// a river is higher than its water: a horse standing on the grass has no
/// water over its hooves at the edge, and a rule that measured from its feet
/// walked it off the bank into a river over its head.
pub fn water_depth<F>(x: f64, near_y: f64, z: f64, block: &F) -> f32
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let (ix, iz) = (x.floor() as i32, z.floor() as i32);
    let top = near_y.floor() as i32 + 1;
    for cy in (top - 3..=top).rev() {
        let here = block(ix, cy, iz).unwrap_or(BLOCK_AIR);
        if !is_liquid(here) {
            continue;
        }
        let above = block(ix, cy + 1, iz).unwrap_or(BLOCK_AIR);
        let surface = cy as f32 + fluid::surface_height_with_above(here, above);
        let mut bed = cy;
        while bed > cy - 8 && block(ix, bed - 1, iz).is_some_and(is_liquid) {
            bed -= 1;
        }
        return surface - bed as f32;
    }
    0.0
}

/// Where the ground is under a point a horse might step to, as the height its
/// feet would stand at, looking from `from_y` down a few blocks. `None` if
/// there is nothing to stand on that near.
fn ground_at<F>(x: f64, from_y: f64, z: f64, block: &F) -> Option<f64>
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    let mut y = from_y;
    for _ in 0..16 {
        if !fits(x, y - 0.25, z, block) {
            // Settle onto it: the last quarter by halves.
            let (mut lo, mut hi) = (y - 0.25, y);
            for _ in 0..10 {
                let mid = (lo + hi) * 0.5;
                if fits(x, mid, z, block) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(hi);
        }
        y -= 0.25;
    }
    None
}

/// Moves a ridden horse on by `dt` seconds. Answers whether it walked into
/// something it could not climb or would not enter -- the refusal the client
/// plays a snort for.
///
/// Axis at a time, like the raft and the player: a horse grazing a wall along
/// its flank keeps going along it. The rise is tried before the stop, so a
/// step is climbed and a wall is not.
pub fn step<F>(body: &mut Mount, reins: Reins, fettle: Fettle, block: &F, dt: f32) -> bool
where
    F: Fn(i32, i32, i32) -> Option<BlockId>,
{
    if !(dt.is_finite() && dt > 0.0) || !body.is_sane() {
        return false;
    }
    let dt = dt.min(0.25);
    let reins = reins.clamped();
    let fettle = fettle.clamped();
    // **A horse that does not fit where it stands does not move.** That is a
    // horse somebody built a block into; moving it would only move it through
    // the block.
    if !fits(body.x, body.y, body.z, block) {
        body.vx = 0.0;
        body.vz = 0.0;
        body.vy = 0.0;
        return false;
    }

    let depth = water_over(body.x, body.y, body.z, block);
    let swimming = depth > REFUSES_DEEPER;
    // ---- the pace asked for, and what it can give ----
    let mut gait = reins.gait;
    let (fx0, fz0) = body.forward();
    let already_galloping = body.vx * fx0 + body.vz * fz0 > TROT + 0.5;
    let has_wind = body.wind > 0.0 && (already_galloping || body.wind >= GALLOP_RESUME);
    if gait == Gait::Gallop && (!fettle.will_gallop || !has_wind) {
        // Blown, or hungry: the gallop is a trot. Not a stop -- a horse
        // that halted under a rider when its wind ran out would be the gauge
        // punishing the rider rather than the rider spending the horse.
        gait = Gait::Trot;
    }
    let mut pace = gait.speed() * load_factor(fettle.load_kg);
    if depth > 0.2 && !swimming {
        pace *= WADING;
    }
    if swimming {
        pace = pace.min(SWIMMING);
    }
    let wanted = if reins.forward >= 0.0 { pace * reins.forward } else { WALK * 0.5 * reins.forward };

    // ---- the turn: wide at speed ----
    let how_fast = (body.speed() / GALLOP).clamp(0.0, 1.0);
    let turn_rate = TURN_WALKING + (TURN_GALLOPING - TURN_WALKING) * how_fast;
    body.yaw = (body.yaw + reins.turn * turn_rate * dt).rem_euclid(std::f32::consts::TAU);

    // ---- along and across its own length ----
    let (fx, fz) = body.forward();
    let along = body.vx * fx + body.vz * fz;
    let across = -body.vx * fz + body.vz * fx;
    let rate = if wanted.abs() < along.abs() || wanted.signum() != along.signum() { BRAKING } else { ACCELERATION };
    let along = if (wanted - along).abs() <= rate * dt { wanted } else { along + (wanted - along).signum() * rate * dt };
    let across = across * (1.0 - GRIP * dt).max(0.0);
    body.vx = along * fx - across * fz;
    body.vz = along * fz + across * fx;

    // ---- wind ----
    let galloping = gait == Gait::Gallop && along > TROT + 0.5;
    if galloping {
        body.wind = (body.wind - dt).max(0.0);
    } else if along.abs() <= WALK + 0.1 {
        body.wind = (body.wind + STAMINA_BACK_WALKING * dt).min(fettle.most_wind);
    } else {
        body.wind = (body.wind + STAMINA_BACK_TROTTING * dt).min(fettle.most_wind);
    }
    body.wind = body.wind.min(fettle.most_wind);

    // ---- up and down ----
    if swimming {
        // Held at the surface with its head out, which is where a swimming
        // horse is: the water's surface less most of its height.
        let float_at = body.y + f64::from(depth) - f64::from(REFUSES_DEEPER);
        body.vy = ((float_at - body.y) as f32 * 4.0).clamp(-2.0, 2.0);
        body.on_ground = false;
    } else {
        if reins.jump && body.on_ground && body.wind >= JUMP_COST * 0.5 {
            body.vy = (2.0 * -GRAVITY * JUMP_HEIGHT).sqrt();
            body.wind = (body.wind - JUMP_COST).max(0.0);
            body.on_ground = false;
        }
        body.vy = (body.vy + GRAVITY * dt).max(TERMINAL);
    }

    // ---- the move, an axis at a time ----
    let mut balked = false;
    for axis in [0usize, 2] {
        let delta = f64::from(if axis == 0 { body.vx } else { body.vz } * dt);
        if delta == 0.0 {
            continue;
        }
        let (nx, nz) = if axis == 0 { (body.x + delta, body.z) } else { (body.x, body.z + delta) };
        // **The bank of a deep river is a wall.** Only where it is deeper
        // ahead than here: a horse already out of its depth may go anywhere
        // that is shallower, which is how it gets out.
        let ahead = water_depth(nx, body.y, nz, block);
        if ahead > REFUSES_DEEPER && ahead > water_depth(body.x, body.y, body.z, block) + 0.05 {
            if axis == 0 { body.vx = 0.0 } else { body.vz = 0.0 }
            balked = true;
            continue;
        }
        if fits(nx, body.y, nz, block) {
            body.x = nx;
            body.z = nz;
            continue;
        }
        // A step: up it if it is a step, at the price of pace.
        let climbed = if body.on_ground {
            ground_at(nx, body.y + f64::from(STEP_UP), nz, block)
                .filter(|&top| top > body.y && top - body.y <= f64::from(STEP_UP))
                .filter(|&top| fits(nx, top, nz, block) && fits(body.x, top, body.z, block))
        } else {
            None
        };
        if let Some(top) = climbed {
            body.x = nx;
            body.z = nz;
            body.y = top;
            body.vx *= CLIMB_KEEPS;
            body.vz *= CLIMB_KEEPS;
            continue;
        }
        if axis == 0 { body.vx = 0.0 } else { body.vz = 0.0 }
        balked = true;
    }
    let dy = f64::from(body.vy * dt);
    if dy != 0.0 {
        if fits(body.x, body.y + dy, body.z, block) {
            body.y += dy;
            body.on_ground = false;
        } else {
            // Up against something, or down onto it: as far as it goes, by
            // halves, so a landing is on the ground and not a sliver above it.
            let (mut ok, mut bad) = (0.0f64, dy);
            for _ in 0..12 {
                let mid = (ok + bad) * 0.5;
                if fits(body.x, body.y + mid, body.z, block) {
                    ok = mid;
                } else {
                    bad = mid;
                }
            }
            body.y += ok;
            if dy < 0.0 {
                body.on_ground = true;
            }
            body.vy = 0.0;
        }
    }
    if !swimming && body.vy <= 0.0 && !fits(body.x, body.y - 0.02, body.z, block) {
        body.on_ground = true;
        body.vy = body.vy.max(0.0);
    }
    balked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER};
    use std::collections::HashMap;

    /// A world of a grass floor at y = 19 with what a test puts on it, and
    /// nothing loaded past sixty blocks out.
    struct Ground(HashMap<(i32, i32, i32), BlockId>);

    impl Ground {
        fn flat() -> Ground {
            Ground(HashMap::new())
        }
        fn at(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
            if x.abs() > 60 || z.abs() > 60 {
                return None;
            }
            if let Some(&b) = self.0.get(&(x, y, z)) {
                return Some(b);
            }
            Some(if y <= 19 { BLOCK_GRASS } else { BLOCK_AIR })
        }
    }

    fn ride(ground: &Ground, body: &mut Mount, reins: Reins, seconds: f32) {
        let block = |x, y, z| ground.at(x, y, z);
        for _ in 0..(seconds / 0.05) as usize {
            step(body, reins, Fettle::FRESH, &block, 0.05);
        }
    }

    fn on(gait: Gait) -> Reins {
        Reins { forward: 1.0, turn: 0.0, gait, jump: false }
    }

    #[test]
    fn a_galloping_horse_outruns_a_sprinting_player_and_a_bear_and_a_walk_does_not() {
        let ground = Ground::flat();
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut body, on(Gait::Gallop), 3.0);
        assert!(body.speed() > crate::animals::NOMINAL_SPRINT_SPEED * 1.5, "a gallop is {}", body.speed());
        assert!(body.speed() > crate::animals::Species::Bear.run_speed());
        let mut walking = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut walking, on(Gait::Walk), 3.0);
        // A walking player is 4.3 (`NOMINAL_SPRINT_SPEED` over the sprint's
        // one and a half): a horse at a walk is one somebody can lead.
        assert!(walking.speed() < crate::animals::NOMINAL_SPRINT_SPEED / 1.5, "a walk is {}", walking.speed());
    }

    #[test]
    fn a_gallop_spends_its_wind_and_then_is_a_trot_and_standing_gives_it_back() {
        let ground = Ground::flat();
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        // Round and round, so the ground never runs out.
        let circling = Reins { turn: 0.6, ..on(Gait::Gallop) };
        ride(&ground, &mut body, circling, GALLOP_SECONDS + 5.0);
        assert!(body.wind < GALLOP_RESUME, "half a minute of gallop left {} wind", body.wind);
        assert!(body.speed() <= TROT + 0.3, "a blown horse still galloped at {}", body.speed());
        ride(&ground, &mut body, Reins::SLACK, 10.0);
        assert!(body.wind >= 4.9, "ten seconds' rest gave back {}", body.wind);
    }

    #[test]
    fn a_hungry_horse_will_not_gallop() {
        let ground = Ground::flat();
        let block = |x, y, z| ground.at(x, y, z);
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        let hungry = Fettle { will_gallop: false, ..Fettle::FRESH };
        for _ in 0..60 {
            step(&mut body, on(Gait::Gallop), hungry, &block, 0.05);
        }
        assert!(body.speed() <= TROT + 0.01);
    }

    #[test]
    fn a_heavy_load_slows_it_to_a_walk_and_a_light_one_not_at_all() {
        assert_eq!(load_factor(0.0), 1.0);
        assert_eq!(load_factor(EASY_LOAD_KG), 1.0);
        assert!((load_factor(HEAVY_LOAD_KG) * GALLOP - WALK).abs() < 1e-4);
        assert!(load_factor(100.0) < 1.0 && load_factor(100.0) > load_factor(150.0));
        assert_eq!(load_factor(f32::NAN), 1.0);
    }

    #[test]
    fn it_walks_up_a_step_and_the_step_costs_it_pace() {
        let mut ground = Ground::flat();
        for z in -3..=3 {
            for x in 5..20 {
                ground.0.insert((x, 20, z), BLOCK_STONE);
            }
        }
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        let block = |x, y, z| ground.at(x, y, z);
        let mut before = 0.0;
        for _ in 0..200 {
            let y = body.y;
            let speed = body.speed();
            step(&mut body, on(Gait::Gallop), Fettle::FRESH, &block, 0.05);
            if body.y > y + 0.5 {
                before = speed;
                break;
            }
        }
        assert!(body.y >= 21.0 - 1e-3, "it never got up the step: {}", body.y);
        assert!(body.speed() < before * 0.8, "the step cost nothing: {} then {}", before, body.speed());
    }

    #[test]
    fn it_does_not_walk_up_two_blocks_and_jumps_one_without_losing_its_pace() {
        let mut ground = Ground::flat();
        for z in -3..=3 {
            ground.0.insert((6, 20, z), BLOCK_STONE);
            ground.0.insert((6, 21, z), BLOCK_STONE);
        }
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut body, on(Gait::Trot), 4.0);
        assert!(body.x < 6.0 - HALF_WIDTH as f64 + 0.01, "it went through a two-block wall to {}", body.x);

        // A single block in the way, jumped at a gallop.
        let mut ground = Ground::flat();
        for z in -3..=3 {
            ground.0.insert((12, 20, z), BLOCK_STONE);
        }
        let block = |x, y, z| ground.at(x, y, z);
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        let mut jumped = false;
        let mut slowest_after = f32::MAX;
        for _ in 0..120 {
            // Taken off a stride and a half out, which is where a rider at a
            // gallop has to ask: any later and the forelegs meet the wall.
            let near = body.x > 12.0 - 3.0 - HALF_WIDTH as f64 && body.x < 12.0;
            let reins = Reins { jump: near && !jumped, ..on(Gait::Gallop) };
            jumped |= reins.jump && body.on_ground;
            step(&mut body, reins, Fettle::FRESH, &block, 0.05);
            if body.x > 13.5 {
                slowest_after = slowest_after.min(body.speed());
            }
        }
        assert!(body.x > 14.0, "the jump did not clear the block: {}", body.x);
        assert!(slowest_after > TROT, "clearing it at a jump cost the gallop: {slowest_after}");
    }

    #[test]
    fn a_horse_will_not_go_into_deep_water_and_wades_a_ford() {
        let mut ground = Ground::flat();
        // A river two deep across x = 8..12, a ford a block deep at x = 20..32.
        for z in -10..=10 {
            for x in 8..12 {
                ground.0.insert((x, 19, z), BLOCK_WATER);
                ground.0.insert((x, 18, z), BLOCK_WATER);
            }
            for x in 20..32 {
                ground.0.insert((x, 19, z), BLOCK_WATER);
            }
        }
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut body, on(Gait::Trot), 5.0);
        assert!(body.x < 8.0, "it went into the river to {}", body.x);
        let mut ford = Mount::standing(15.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut ford, on(Gait::Trot), 1.2);
        let dry = ford.speed();
        let block = |x, y, z| ground.at(x, y, z);
        for _ in 0..100 {
            if ford.x > 25.0 {
                break;
            }
            step(&mut ford, on(Gait::Trot), Fettle::FRESH, &block, 0.05);
        }
        ride(&ground, &mut ford, on(Gait::Trot), 0.3);
        assert!(ford.x > 25.0 && ford.x < 32.0, "it did not get into the ford: {}", ford.x);
        assert!(ford.speed() < dry * 0.7, "wading cost nothing: {} then {}", dry, ford.speed());
    }

    #[test]
    fn a_horse_turns_wider_at_a_gallop_than_at_a_walk() {
        let ground = Ground::flat();
        let turning = |gait| {
            let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
            ride(&ground, &mut body, on(gait), 3.0);
            let before = body.yaw;
            ride(&ground, &mut body, Reins { turn: 1.0, ..on(gait) }, 0.5);
            (body.yaw - before).abs()
        };
        assert!(turning(Gait::Gallop) < turning(Gait::Walk) * 0.7);
    }

    #[test]
    fn it_stops_at_the_edge_of_the_world_that_is_loaded() {
        let ground = Ground::flat();
        let mut body = Mount::standing(50.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        ride(&ground, &mut body, on(Gait::Gallop), 4.0);
        assert!(body.x < 61.0, "it galloped off the loaded world to {}", body.x);
    }

    #[test]
    fn nonsense_reins_are_a_standing_horse_and_nonsense_bodies_do_not_move() {
        let ground = Ground::flat();
        let block = |x, y, z| ground.at(x, y, z);
        let mut body = Mount::standing(0.5, 20.0, 0.5, 0.0, GALLOP_SECONDS);
        let mad = Reins { forward: f32::NAN, turn: f32::INFINITY, gait: Gait::Gallop, jump: false };
        step(&mut body, mad, Fettle::FRESH, &block, 0.05);
        assert!(body.is_sane() && body.speed() == 0.0);
        let mut broken = Mount { x: f64::NAN, ..body };
        step(&mut broken, on(Gait::Gallop), Fettle::FRESH, &block, 0.05);
        assert!(broken.x.is_nan());
        assert_eq!(Gait::from_wire(Gait::Gallop.to_wire()), Gait::Gallop);
        assert_eq!(Gait::from_wire(200), Gait::Walk);
    }
}
