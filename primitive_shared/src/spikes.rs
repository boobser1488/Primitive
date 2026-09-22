//! Sharpened stakes that cut whoever goes through them.
//!
//! **A stake is walked through, and that is what makes it a weapon.** It was
//! always passable (`blocks::STAKE_ROW` is a cross, not a wall) because a
//! palisade that stopped a body like stone would be a wall nobody asked for.
//! What a bundle of points does instead is *cost*: anything that pushes
//! through at more than a shuffle comes out cut, and the faster it came the
//! worse. So a ring of stakes round a camp is a decision rather than a
//! fence -- a boar that charges it is bleeding when it arrives, a wolf turns
//! away, and the player who planted it pays the same price every time they
//! cut through their own defences instead of walking round to the gap.
//!
//! Rejected: stakes as a solid wall with damage on contact. It is the same
//! wall with a number on it, and a player stood against it would take damage
//! for standing still. Rejected too: harm only while sprinting. There is no
//! crouch in this game, so "slow enough to be safe" has to be a speed a body
//! only reaches when it is barely moving, or stakes cost nothing at a walk.

use crate::geometry::block_box_for_aim;
use crate::types::{is_stake, BlockId};

/// Below this, blocks a second across the ground, a body is edging through
/// the points rather than pushing into them, and nothing is cut.
pub const SHUFFLE: f32 = 1.0;

/// How fast a body goes among the points, as a share of its stride: pushed
/// through rather than walked. A walk among stakes is still past `SHUFFLE`,
/// so it still cuts; see `harm` for why the numbers are set for this speed.
pub const THROUGH_STAKES: f32 = 0.35;

/// Seconds between two cuts from stakes, so a body stood among them is cut
/// once a second while it keeps moving and not sixty times.
pub const COOLDOWN: f32 = 1.0;

/// Does a body in this box touch the points of a stake?
///
/// The stake's own box -- the spikes, as the aim has them -- and not its
/// cell: a player brushing the empty half of a cell a stake is driven into
/// the far wall of is not in the points.
pub fn touches(min: [f64; 3], max: [f64; 3], block_at: impl Fn(i32, i32, i32) -> BlockId) -> bool {
    for y in min[1].floor() as i32..=max[1].floor() as i32 {
        for z in min[2].floor() as i32..=max[2].floor() as i32 {
            for x in min[0].floor() as i32..=max[0].floor() as i32 {
                let block = block_at(x, y, z);
                if !is_stake(block) {
                    continue;
                }
                let Some((lo, hi)) = block_box_for_aim(block, x, y, z, false) else {
                    continue;
                };
                let overlaps = (0..3).all(|a| min[a] < f64::from(hi[a]) && max[a] > f64::from(lo[a]));
                if overlaps {
                    return true;
                }
            }
        }
    }
    false
}

/// What going through stakes at `speed` does: the damage, and how bad a cut
/// it leaves on the scale `injury::Injuries::inflict` takes. `None` below a
/// shuffle.
///
/// At a walk the cut is past `injury::CUT_CLOTS_BELOW`, so it bleeds until it
/// is dressed: a cut that clotted on its own is a scratch, and a scratch is
/// not a reason to walk round.
pub fn harm(speed: f32) -> Option<(f32, f32)> {
    if !speed.is_finite() || speed <= SHUFFLE {
        return None;
    }
    // Set for the speeds a body actually has among stakes, which is a third
    // of its stride (`THROUGH_STAKES`): a walk there is about one and a
    // half blocks a second and costs two points and a bleeding cut; a
    // charge from outside comes in at its full speed for the first cut and
    // is worse.
    let damage = 1.5 + 0.35 * speed;
    let cut = (0.35 + 0.04 * speed).min(0.8);
    Some((damage, cut))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{placed, BLOCK_AIR, BLOCK_STAKE};

    #[test]
    fn edging_through_stakes_is_safe_and_walking_through_them_bleeds() {
        assert_eq!(harm(0.6), None, "a shuffle is not cut");
        let (walk_damage, walk_cut) = harm(4.3).expect("a walk is cut");
        assert!(walk_cut > crate::injury::CUT_CLOTS_BELOW, "a walk's cut clots by itself -- a scratch");
        let (run_damage, run_cut) = harm(6.45).expect("a run is cut");
        assert!(run_damage > walk_damage && run_cut > walk_cut, "a run through the points is no worse than a walk");
    }

    #[test]
    fn a_body_touches_the_points_of_a_standing_stake_and_not_the_cell_beside_it() {
        let stake = placed(BLOCK_STAKE, 0.0, (0, 1, 0));
        let at = |x: i32, y: i32, z: i32| if (x, y, z) == (0, 0, 0) { stake } else { BLOCK_AIR };
        assert!(touches([0.2, 0.0, 0.2], [0.8, 1.8, 0.8], at), "a body stood in the stake is not in its points");
        assert!(!touches([1.2, 0.0, 0.2], [1.8, 1.8, 0.8], at), "a body in the next cell is in the points");
    }
}
