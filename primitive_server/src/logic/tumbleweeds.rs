//! Tumbleweeds: balls of dry stems the wind rolls across open dry country.
//!
//! ## What one is, and why it is the server's
//!
//! Three homes were weighed, and the choice decides what a tumbleweed *is*:
//!
//! * **A particle** (`engine::particles` on the client). Free, and a picture:
//!   it could not collide with a wall worth the name, two players would see
//!   two different ones, and nothing could be done with it. Scenery, which a
//!   mechanic here is not allowed to be.
//! * **A light entity of the client's own**, as the leaves blown off a crown
//!   are (`engine::breeze`). It could roll and stop at walls against the
//!   client's copy of the world, and it still could not be *picked up*: the
//!   server would have to be told about a thing it never made, which is the
//!   one direction nothing in this game is allowed to flow.
//! * **A dropped item the wind pushes (chosen).** A tumbleweed is an
//!   `EntityKind::Item` of `BLOCK_TUMBLEWEED`, and `logic::items` already
//!   does everything else one needs: it is replicated to everyone near, it
//!   collides with the world on the server, it floats, it expires, and a
//!   player who walks into it picks it up. What this module adds is the one
//!   thing an item did not do -- **roll with the wind** (`roll`) -- and where
//!   the wind brings them from (`blow_in`). Nothing new crosses the wire.
//!
//! ## Why it is worth running after
//!
//! **Fire where there is no wood.** The steppe and the dry country have no
//! trees to speak of, and a tumbleweed is kindling that burns hot and
//! quick (`hearth::fuel_seconds`) or three strands of fibre for the kiln and
//! the torch (`crafting`, "tumbleweed tow"). It comes to the player on the
//! wind and goes past them at a run: stand in its path, chase it down, or
//! let it go and find it caught in the next thorn bush downwind. A wind
//! strong enough to bring them is also the wind that will carry a grass
//! fire, which is the other thing a player in that country is deciding.
//!
//! ## How it moves
//!
//! Pushed toward the wind's own speed, quickly on the ground and slowly in
//! the air; over a step of one block it hops, at a wall of two it stops; it
//! skips a little as it goes, which is what a ball of stems does on uneven
//! ground; **a thorn bush, a hedge, a stake or a cactus holds it** -- it is
//! snagged and stays. It crumbles after a few minutes on the ground, as the
//! drop it is expires, only sooner (`LIFE`).

use std::time::Duration;

use primitive_shared::types::{self, BlockId};

/// How long one lasts before it falls apart, from when it broke loose.
pub const LIFE: Duration = Duration::from_secs(180);

/// Half the ball's width, in blocks. It is drawn this size
/// (`logic::tumbleweed_model` on the client), and it collides this size, so
/// what stops at a wall is what is seen to stop there.
pub const RADIUS: f32 = 0.32;

/// The wind strength (`raft::Wind::strength`) under which nothing breaks
/// loose and nothing rolls: a clear day's breeze (0.45 before its lull) moves
/// them now and then, a front's wind all the time.
pub const STILL: f32 = 0.3;

/// How fast a ball rolls in a full gale, in blocks a second: about a run
/// (`physics::SPRINT_SPEED` is a little under seven). A strong wind's ball is
/// caught by a player who runs at it, not by one who walks after it.
const ROLL_SPEED: f32 = 6.5;
/// How quickly it takes up the wind's speed, per second: on the ground, and
/// in the air, where there is nothing to roll against.
const GRIP: f32 = 2.2;
const AIR_GRIP: f32 = 0.7;
/// How quickly it stops when the wind drops, per second.
const DRAG: f32 = 1.8;
const GRAVITY: f32 = -22.0;
const TERMINAL: f32 = -20.0;
/// What a hop over a one-block step starts at, in blocks a second: v²/2g a
/// little over a block.
const HOP: f32 = 7.0;
/// ...and the little skip it makes every `SKIP_EVERY` blocks it rolls.
const SKIP: f32 = 2.8;
const SKIP_EVERY: f32 = 1.7;
/// How much of its fall it keeps on a hard landing.
const BOUNCE: f32 = 0.35;

/// Is this a thing the wind rolls?
#[inline]
pub fn is_tumbleweed(block: BlockId) -> bool {
    types::block_kind(block) == types::BLOCK_TUMBLEWEED
}

/// What a ball is doing, as `logic::items` keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rolling {
    /// The middle of the ball.
    pub position: (f64, f64, f64),
    pub velocity: (f32, f32, f32),
    pub on_ground: bool,
    /// How far it has rolled since it last skipped.
    pub travelled: f32,
    /// Held by a bush or a fence; it stays until it crumbles.
    pub snagged: bool,
}

/// What a ball asks of the world.
pub trait Ground {
    /// The top of the highest box a ball of `half` round `centre` overlaps,
    /// or `None` if it is clear -- the question `items::overlap_top` answers
    /// for a drop.
    fn top(&self, centre: (f64, f64, f64), half: f32) -> Option<f64>;
    /// The block in a cell, for what snags it.
    fn block(&self, cell: (i32, i32, i32)) -> BlockId;
}

/// Does this hold a ball that rolls into it? The thorny and the woody: a
/// bush in fruit or bare, a dog rose, heather, a thistle, a cactus, a
/// hedge's leaves, a stake -- "застревают в кустах и заборах". Not grass:
/// grass is what it rolls over.
pub fn snags(block: BlockId) -> bool {
    matches!(
        types::block_kind(block),
        types::BLOCK_BERRY_BUSH
            | types::BLOCK_BARE_BUSH
            | types::BLOCK_ROSEHIP_BUSH
            | types::BLOCK_ROSEHIP_BARE
            | types::BLOCK_HEATHER
            | types::BLOCK_THISTLE
            | types::BLOCK_CACTUS
            | types::BLOCK_BUSH_LEAVES
            | types::BLOCK_STAKE
            | types::BLOCK_NETTLE
            | types::BLOCK_ARUNDO
    )
}

/// One tick of a ball in a wind (`raft::Wind::vector`, x and z, as long as
/// it is strong).
pub fn roll(ball: &mut Rolling, wind: (f32, f32), dt: f32, ground: &impl Ground) {
    if ball.snagged {
        ball.velocity = (0.0, 0.0, 0.0);
        return;
    }
    let half = RADIUS;
    let dt = dt.clamp(0.0, 0.1);
    ball.velocity.1 = (ball.velocity.1 + GRAVITY * dt).max(TERMINAL);

    // **Toward the wind's speed, not by a force.** A ball of stems weighs
    // nothing and goes at what the air goes at; what differs is how soon,
    // which is the grip.
    let strength = (wind.0 * wind.0 + wind.1 * wind.1).sqrt();
    if strength >= STILL {
        let speed = ROLL_SPEED * ((strength - STILL) / (1.0 - STILL)).clamp(0.15, 1.0);
        let target = (wind.0 / strength * speed, wind.1 / strength * speed);
        let rate = if ball.on_ground { GRIP } else { AIR_GRIP };
        let k = 1.0 - (-rate * dt).exp();
        ball.velocity.0 += (target.0 - ball.velocity.0) * k;
        ball.velocity.2 += (target.1 - ball.velocity.2) * k;
    } else if ball.on_ground {
        let keep = (-DRAG * dt).exp();
        ball.velocity.0 *= keep;
        ball.velocity.2 *= keep;
    }

    // Across, an axis at a time, as a drop moves: a wall stops the axis it
    // is across and lets the other go on, so a ball against a wall at an
    // angle slides along it.
    let start = ball.position;
    let mut hop = false;
    for axis in [0, 2] {
        let step = f64::from(if axis == 0 { ball.velocity.0 } else { ball.velocity.2 }) * f64::from(dt);
        if step == 0.0 {
            continue;
        }
        let mut next = ball.position;
        if axis == 0 {
            next.0 += step;
        } else {
            next.2 += step;
        }
        if ground.top(next, half).is_none() {
            ball.position = next;
            continue;
        }
        // **A step of one block is hopped; two is a wall.** Asked a block
        // up: clear there means the thing in the way is a step.
        let over = (next.0, next.1 + 1.05, next.2);
        if ball.on_ground && ground.top(over, half).is_none() && ground.top((ball.position.0, ball.position.1 + 1.05, ball.position.2), half).is_none() {
            hop = true;
        } else if !ball.on_ground {
            // In the air against a step it is still climbing: it waits
            // there, still pushed, and goes on over the top.
        } else if axis == 0 {
            ball.velocity.0 = 0.0;
        } else {
            ball.velocity.2 = 0.0;
        }
    }
    if hop {
        ball.velocity.1 = ball.velocity.1.max(HOP);
    }

    let next = (ball.position.0, ball.position.1 + f64::from(ball.velocity.1 * dt), ball.position.2);
    match ground.top(next, half) {
        Some(top) if ball.velocity.1 <= 0.0 => {
            ball.position.1 = top + f64::from(half);
            ball.velocity.1 = if ball.velocity.1 < -5.0 { -ball.velocity.1 * BOUNCE } else { 0.0 };
            ball.on_ground = true;
        }
        Some(_) => ball.velocity.1 = 0.0,
        None => {
            ball.position = next;
            ball.on_ground = false;
        }
    }

    // A little skip every so often, which is how a ball of stems crosses
    // ground that is not a floor.
    let moved = ((ball.position.0 - start.0).powi(2) + (ball.position.2 - start.2).powi(2)).sqrt() as f32;
    ball.travelled += moved;
    if ball.on_ground && ball.travelled > SKIP_EVERY && moved > 0.5 * dt {
        ball.velocity.1 = ball.velocity.1.max(SKIP);
        ball.travelled = 0.0;
    }

    // **Snagged**: rolled into a bush or against a stake. The cell its
    // middle is in, and the one ahead of it at its own height.
    let cell = |p: (f64, f64, f64)| (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
    let (vx, vz) = (ball.velocity.0, ball.velocity.2);
    let ahead = if vx.abs() + vz.abs() > 0.05 {
        let length = (vx * vx + vz * vz).sqrt();
        let reach = f64::from(half + 0.05);
        Some((ball.position.0 + f64::from(vx / length) * reach, ball.position.1, ball.position.2 + f64::from(vz / length) * reach))
    } else {
        None
    };
    if snags(ground.block(cell(ball.position))) || ahead.is_some_and(|p| snags(ground.block(cell(p)))) {
        ball.snagged = true;
        ball.velocity = (0.0, 0.0, 0.0);
    }
}

/// Where a ball could break loose: open, dry ground a steppe or a desert
/// grows saltwort on. `biome` is the column's.
pub fn country(biome: primitive_shared::worldgen::Biome) -> bool {
    use primitive_shared::worldgen::Biome;
    matches!(biome, Biome::Steppe | Biome::Savanna | Biome::Desert)
}

/// The most balls near one player at once, and how near "near" is.
pub const MOST_NEAR: usize = 4;
pub const NEAR: f64 = 64.0;
/// How many a second break loose round a player in a full gale, before the
/// cap: one every twenty seconds or so, fewer in a lesser wind.
const PER_SECOND: f32 = 0.05;
/// How far upwind of the player they break loose, in blocks: out of the
/// render distance's nearest ring, in sight across open country.
const UPWIND: (f32, f32) = (22.0, 40.0);

/// Where a ball should break loose this tick round a player at `feet`, if
/// one should. `near` is how many are already within `NEAR`; `surface` finds
/// the ground's top in a column and the block it is (or `None` in water or an
/// unloaded chunk); `biome` names a column's country.
pub fn loose_ball(
    feet: (f64, f64, f64),
    wind: (f32, f32),
    near: usize,
    dt: f32,
    rng: &mut crate::logic::rng::Rng,
    surface: impl Fn(i32, i32) -> Option<(i32, BlockId)>,
    biome: impl Fn(i32, i32) -> primitive_shared::worldgen::Biome,
) -> Option<(f64, f64, f64)> {
    let strength = (wind.0 * wind.0 + wind.1 * wind.1).sqrt();
    if strength < STILL || near >= MOST_NEAR {
        return None;
    }
    if !rng.chance(PER_SECOND * strength * dt) {
        return None;
    }
    let (ux, uz) = (wind.0 / strength, wind.1 / strength);
    let back = rng.range(UPWIND.0, UPWIND.1);
    let aside = rng.range(-20.0, 20.0);
    let x = feet.0 - f64::from(ux * back) + f64::from(-uz * aside);
    let z = feet.2 - f64::from(uz * back) + f64::from(ux * aside);
    let (gx, gz) = (x.floor() as i32, z.floor() as i32);
    if !country(biome(gx, gz)) {
        return None;
    }
    let (top, ground) = surface(gx, gz)?;
    // Dry open ground: the dry turf, the sand and the sandy soil, and a
    // steppe's green turf. Not snow, not rock.
    let common = primitive_shared::ground::as_common(ground);
    if !matches!(common, types::BLOCK_DRY_TURF | types::BLOCK_SANDY_SOIL | types::BLOCK_SAND | types::BLOCK_GRASS | types::BLOCK_DIRT) {
        return None;
    }
    Some((x, f64::from(top) + 1.0 + f64::from(RADIUS) + 0.05, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A floor at y = 10 (its top), and whatever else a test puts in.
    struct Field {
        blocks: HashMap<(i32, i32, i32), BlockId>,
    }

    impl Field {
        fn floor() -> Self {
            let mut blocks = HashMap::new();
            for x in -40..80 {
                for z in -20..20 {
                    blocks.insert((x, 9, z), types::BLOCK_DRY_TURF);
                }
            }
            Field { blocks }
        }
    }

    impl Ground for Field {
        fn top(&self, centre: (f64, f64, f64), half: f32) -> Option<f64> {
            let e = f64::from(half) - 1e-3;
            let mut best: Option<f64> = None;
            for x in (centre.0 - e).floor() as i32..=(centre.0 + e).floor() as i32 {
                for y in (centre.1 - e).floor() as i32..=(centre.1 + e).floor() as i32 {
                    for z in (centre.2 - e).floor() as i32..=(centre.2 + e).floor() as i32 {
                        let block = self.block((x, y, z));
                        if types::is_collidable(block) {
                            best = Some(best.map_or(f64::from(y + 1), |b: f64| b.max(f64::from(y + 1))));
                        }
                    }
                }
            }
            best
        }
        fn block(&self, cell: (i32, i32, i32)) -> BlockId {
            self.blocks.get(&cell).copied().unwrap_or(types::BLOCK_AIR)
        }
    }

    fn ball() -> Rolling {
        Rolling { position: (0.5, 10.0 + f64::from(RADIUS), 0.5), velocity: (0.0, 0.0, 0.0), on_ground: true, travelled: 0.0, snagged: false }
    }

    fn run(field: &Field, wind: (f32, f32), seconds: f32) -> Rolling {
        let mut b = ball();
        for _ in 0..(seconds * 20.0) as usize {
            roll(&mut b, wind, 0.05, field);
        }
        b
    }

    #[test]
    fn a_tumbleweed_rolls_the_way_the_wind_blows() {
        let field = Field::floor();
        for wind in [(0.9, 0.0), (0.0, -0.7), (-0.6, 0.6)] {
            let b = run(&field, wind, 3.0);
            let (dx, dz) = (b.position.0 - 0.5, b.position.2 - 0.5);
            let along = dx * f64::from(wind.0) + dz * f64::from(wind.1);
            assert!(along > 3.0, "in a wind of {wind:?} it went {dx:.2}, {dz:.2}");
            assert!(b.position.1 >= 10.0 + f64::from(RADIUS) - 1e-6, "it sank into the ground: {:?}", b.position);
        }
    }

    #[test]
    fn a_still_day_leaves_it_where_it_lies() {
        let b = run(&Field::floor(), (0.1, 0.0), 5.0);
        assert!((b.position.0 - 0.5).abs() < 0.01, "a breeze under the line rolled it to {:?}", b.position);
    }

    #[test]
    fn a_tumbleweed_does_not_roll_through_a_wall() {
        let mut field = Field::floor();
        for y in 10..13 {
            for z in -20..20 {
                field.blocks.insert((8, y, z), types::BLOCK_COBBLESTONE);
            }
        }
        let b = run(&field, (1.0, 0.0), 10.0);
        assert!(b.position.0 + f64::from(RADIUS) <= 8.0 + 1e-6, "it went through the wall to {:?}", b.position);
        assert!(b.position.0 > 6.5, "it never reached the wall: {:?}", b.position);
    }

    #[test]
    fn a_tumbleweed_hops_a_step_it_meets() {
        let mut field = Field::floor();
        for x in 8..80 {
            for z in -20..20 {
                field.blocks.insert((x, 10, z), types::BLOCK_DRY_TURF);
            }
        }
        let b = run(&field, (1.0, 0.0), 8.0);
        assert!(b.position.0 > 12.0, "it stopped at a step of one block: {:?}", b.position);
        assert!(b.position.1 > 11.0, "it went on at the lower level: {:?}", b.position);
    }

    #[test]
    fn a_thorn_bush_catches_a_tumbleweed_and_holds_it() {
        let mut field = Field::floor();
        field.blocks.insert((6, 10, 0), types::BLOCK_ROSEHIP_BUSH);
        let b = run(&field, (1.0, 0.0), 10.0);
        assert!(b.snagged, "it rolled past the dog rose: {:?}", b.position);
        assert!(b.position.0 < 7.0, "it went through the bush to {:?}", b.position);
    }

    #[test]
    fn balls_break_loose_upwind_in_dry_country_and_nowhere_else() {
        use primitive_shared::worldgen::Biome;
        let mut rng = crate::logic::rng::Rng::seeded(7);
        let feet = (0.0, 20.0, 0.0);
        let dry = |_: i32, _: i32| Some((19, types::BLOCK_DRY_TURF));
        let mut found = 0;
        for _ in 0..2000 {
            if let Some(at) = loose_ball(feet, (0.9, 0.0), 0, 1.0, &mut rng, dry, |_, _| Biome::Steppe) {
                assert!(at.0 < -15.0, "a ball broke loose downwind of the player, at {at:?}");
                found += 1;
            }
            assert!(loose_ball(feet, (0.9, 0.0), 0, 1.0, &mut rng, dry, |_, _| Biome::Forest).is_none());
            assert!(loose_ball(feet, (0.9, 0.0), MOST_NEAR, 1.0, &mut rng, dry, |_, _| Biome::Steppe).is_none());
            assert!(loose_ball(feet, (0.1, 0.0), 0, 1.0, &mut rng, dry, |_, _| Biome::Steppe).is_none());
        }
        assert!(found > 20, "only {found} balls in two thousand seconds of a steppe gale");
    }
}
