//! Player collider geometry, shared so the client and the server can't
//! disagree about it.
//!
//! This exists because of one rule: **you can't place a block inside a
//! player.** The client needs it to grey out the placement locally (so
//! the block never flickers into existence), and the server needs the
//! exact same numbers to enforce it authoritatively. Two copies of
//! "how wide is a player" that drift apart would mean the client
//! predicting one thing and the server rejecting it, which looks like
//! random lag to the player.

use crate::types::{collision_height, BlockId};

/// Half the collider's width on X and Z.
pub const PLAYER_HALF_WIDTH: f32 = 0.3;
/// Total collider height, feet to crown.
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Camera height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;

/// The tallest lip a walking player rides over instead of stopping
/// dead against.
///
/// **This number existed because layers did**, and they are gone. Half
/// a metre of drifted snow that a player had to *jump* was not drifted
/// snow, it was a wall the height of a chair; the step let a walker
/// ride over it. Nothing in the world is that short any more -- every
/// solid fills its cell -- so the step-up never fires and the constant
/// survives as the bound that keeps it that way. Under a block, so no
/// wall is ever climbed for free.
///
/// Kept rather than deleted because the collider still measures against
/// it in two places (`settle_onto_step`, and the escape's preference
/// for going up), and because a world that grows a short block again
/// should get the old behaviour rather than a new bug. Shared because
/// the server's anti-cheat has to expect whatever the client does.
pub const PLAYER_STEP_HEIGHT: f32 = 0.5 + 1e-3;

/// The box a block occupies inside its cell, as (min, max) in world
/// space, or `None` if there is nothing solid there.
///
/// Every solid block is its whole cell again -- loose material used to
/// fill its cell in eighths and no longer does -- so the height here is
/// 1.0 for anything you can walk into and 0.0 for anything you cannot.
/// Kept as a function rather than folded away because water still has a
/// level, and because the client walks on this while the server decides
/// what may be built into it: one answer, one place.
pub fn block_box(block: BlockId, bx: i32, by: i32, bz: i32) -> Option<([f32; 3], [f32; 3])> {
    let height = collision_height(block);
    if height <= 0.0 {
        return None;
    }
    let (x, y, z) = (bx as f32, by as f32, bz as f32);
    Some(([x, y, z], [x + 1.0, y + height, z + 1.0]))
}

/// The box a build/break ray may stop at, or `None` for something a ray
/// goes straight through.
///
/// **Not the same box as `block_box`, and not the same question.** What
/// you collide with is what holds you up; what you aim at is what you
/// can see. A tuft of grass holds nothing up and is very much aimable,
/// and a stone lying on the ground is a quad two centimetres thick that
/// you must nonetheless be able to pick up.
///
/// The sizes track what the mesher draws, because a selection box that
/// does not fit what is inside it is worse than no selection box: the
/// old one was the whole cell for everything, so a blade of grass came
/// with a metre cube around it, and aiming at the empty air beside a
/// pebble picked the pebble up from a block away.
pub fn block_target_box(block: BlockId, bx: i32, by: i32, bz: i32) -> Option<([f32; 3], [f32; 3])> {
    use crate::types::{block_height, is_cross, is_flat, is_targetable};
    if !is_targetable(block) {
        return None;
    }
    // Fractions of the cell: (inset on x/z, height).
    let (inset, height) = if is_flat(block) {
        // A stone lying on the ground. Thin, but not infinitely thin --
        // a zero-height box can only be hit by a ray exactly level with
        // it, which is a box nobody can click. The inset comes from the
        // block, because a coating of ash covers its cell and a pebble
        // does not: see `types::flat_inset`.
        //
        // The height starts where the quad does, so the box a coating
        // offers begins at the floor of its cell and the box an object
        // offers begins at the lift it is drawn at. Aiming at the ash
        // *under* a pebble is then a question the geometry can answer.
        (crate::types::flat_inset(block), crate::types::flat_lift(block) + 0.08)
    } else if is_cross(block) {
        // Two crossed planes that wander a little inside their cell and
        // vary in height (see the mesher's `cross_block`: an inset of
        // 0.08, a jitter of 0.07, and a height of 0.94 that may gain an
        // eighth of itself). The box is drawn around the *tallest and
        // widest* a tuft may come out, not around the average one --
        // aiming at the top of a blade and hitting nothing is exactly
        // the complaint this box exists to answer.
        (0.01, 1.0)
    } else {
        (0.0, block_height(block))
    };
    let (x, y, z) = (bx as f32, by as f32, bz as f32);
    Some((
        [x + inset, y, z + inset],
        [x + 1.0 - inset, y + height, z + 1.0 - inset],
    ))
}

/// Where along a ray it enters an axis-aligned box, if it does at all.
///
/// The slab method, the same one `ray_hits_player` uses on a player: a
/// box is the intersection of three pairs of parallel planes, so the ray
/// is inside it over the intersection of the three intervals. Shared by
/// the client's block raycast, which needs it per candidate cell.
pub fn ray_hits_box(
    origin: [f32; 3],
    dir: [f32; 3],
    min: [f32; 3],
    max: [f32; 3],
    max_distance: f32,
) -> Option<f32> {
    ray_box_entry(origin, dir, min, max, max_distance).map(|(distance, _)| distance)
}

/// `ray_hits_box`, and **which face it came in through**.
///
/// The face is the axis whose near plane the entry distance came from,
/// or `None` for a ray that began inside the box, where there is no face
/// to name.
///
/// Placing a block is what needs it. A new block goes in the cell across
/// the face that was clicked, and while everything filled its cell that
/// was the same thing as the cell the ray came from -- so the caller
/// could simply remember the previous cell and never ask. It stopped
/// being the same thing when blocks started filling part of a cell: a
/// shallow look along a drift of snow crosses several cells *above* the
/// drift and then enters one of them through the drift's top, so the
/// cell the ray came from is the drift next door and the face that was
/// hit points at the sky.
pub fn ray_box_entry(
    origin: [f32; 3],
    dir: [f32; 3],
    min: [f32; 3],
    max: [f32; 3],
    max_distance: f32,
) -> Option<(f32, Option<usize>)> {
    let mut enter = 0.0f32;
    let mut leave = max_distance;
    let mut face = None;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-9 {
            // Parallel to this pair of planes: either always between
            // them or never.
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let inverse = 1.0 / dir[axis];
        let mut near = (min[axis] - origin[axis]) * inverse;
        let mut far = (max[axis] - origin[axis]) * inverse;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        // The last axis to push the entry back is the one whose plane
        // the ray is actually crossing when it goes in; the others were
        // already behind it. An entry still at zero means the ray
        // started inside, and no face was crossed at all.
        if near > enter {
            enter = near;
            face = Some(axis);
        }
        leave = leave.min(far);
        if enter > leave {
            return None;
        }
    }
    Some((enter, face))
}

/// Does the block at (bx, by, bz) overlap the player standing with
/// their feet at `feet`?
///
/// Touching exactly counts as not overlapping: standing precisely on top
/// of a block must not make the block you're standing on unplaceable,
/// and the same goes for a wall you're flush against.
///
/// Takes the block rather than only its cell because a layer of snow at
/// your feet is not inside you and a full block is. Refusing to lay a
/// single layer on the ground you are standing on would make the
/// commonest placement in the game impossible.
///
/// **What you can step onto is not inside you.** A block whose top ends
/// up no more than `PLAYER_STEP_HEIGHT` above the feet is something the
/// player stands *on*: the physics lifts them onto it in the same frame
/// it appears (see the client's `settle_onto_step`), so calling it an
/// obstruction would refuse the one placement players make constantly
/// -- looking down and laying material where they stand -- to prevent a
/// burial that cannot happen. A whole block is a metre tall and stays
/// refused, which is the case this check was written for.
pub fn block_overlaps_player(
    feet: (f32, f32, f32),
    bx: i32,
    by: i32,
    bz: i32,
    block: BlockId,
) -> bool {
    let Some((min_b, max_b)) = block_box(block, bx, by, bz) else {
        return false;
    };
    let (px, py, pz) = feet;
    if max_b[1] - py <= PLAYER_STEP_HEIGHT {
        return false;
    }
    let min_x = px - PLAYER_HALF_WIDTH;
    let max_x = px + PLAYER_HALF_WIDTH;
    let min_y = py;
    let max_y = py + PLAYER_HEIGHT;
    let min_z = pz - PLAYER_HALF_WIDTH;
    let max_z = pz + PLAYER_HALF_WIDTH;

    min_x < max_b[0]
        && max_x > min_b[0]
        && min_y < max_b[1]
        && max_y > min_b[1]
        && min_z < max_b[2]
        && max_z > min_b[2]
}

/// How far along a ray a player's collider starts, if the ray reaches
/// it at all.
///
/// The slab method: a box is the intersection of three pairs of parallel
/// planes, so the ray is inside it over the intersection of the three
/// intervals it spends between each pair. Cheap, exact, and it needs no
/// special case for a ray parallel to an axis -- the division by zero
/// gives an infinite interval, which is the right answer.
///
/// Here rather than in the client because it answers the same question
/// `block_overlaps_player` does -- where a player *is* -- and the client
/// aiming at someone the server does not think is there is exactly the
/// disagreement this module exists to prevent. `dir` need not be
/// normalised; the distance comes back in units of it, so pass a unit
/// vector if the answer is to be in blocks.
pub fn ray_hits_player(
    origin: (f32, f32, f32),
    dir: (f32, f32, f32),
    feet: (f32, f32, f32),
    max_distance: f32,
) -> Option<f32> {
    let min = [
        feet.0 - PLAYER_HALF_WIDTH,
        feet.1,
        feet.2 - PLAYER_HALF_WIDTH,
    ];
    let max = [
        feet.0 + PLAYER_HALF_WIDTH,
        feet.1 + PLAYER_HEIGHT,
        feet.2 + PLAYER_HALF_WIDTH,
    ];
    let origin = [origin.0, origin.1, origin.2];
    let dir = [dir.0, dir.1, dir.2];

    let mut enter = 0.0f32;
    let mut leave = max_distance;
    for axis in 0..3 {
        let inverse = 1.0 / dir[axis];
        let mut near = (min[axis] - origin[axis]) * inverse;
        let mut far = (max[axis] - origin[axis]) * inverse;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        // NaN, which is what a zero direction on an axis the origin is
        // exactly on produces, must not widen the interval.
        if near.is_nan() || far.is_nan() {
            return None;
        }
        enter = enter.max(near);
        leave = leave.min(far);
        if enter > leave {
            return None;
        }
    }
    Some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_AIR, BLOCK_SNOW, BLOCK_STONE, BLOCK_WATER};

    /// The block every one of these tests used before blocks had
    /// shapes: a plain solid cube.
    const CUBE: BlockId = BLOCK_STONE;

    #[test]
    fn a_block_at_the_players_feet_overlaps() {
        assert!(block_overlaps_player((0.5, 10.0, 0.5), 0, 10, 0, CUBE));
    }

    #[test]
    fn a_block_at_head_height_overlaps() {
        // Feet at y=10 means the collider spans 10.0..11.8, so the cube
        // at y=11 is inside the player's chest/head.
        assert!(block_overlaps_player((0.5, 10.0, 0.5), 0, 11, 0, CUBE));
    }

    #[test]
    fn the_block_being_stood_on_is_placeable() {
        // Feet exactly on top of the block at y=9 (which spans 9..10).
        assert!(
            !block_overlaps_player((0.5, 10.0, 0.5), 0, 9, 0, CUBE),
            "the floor must not count as inside the player"
        );
    }

    #[test]
    fn a_block_just_above_the_head_is_placeable() {
        // Collider tops out at 11.8, so the cube spanning 12..13 is clear.
        assert!(!block_overlaps_player((0.5, 10.0, 0.5), 0, 12, 0, CUBE));
    }

    #[test]
    fn a_block_flush_against_the_side_is_placeable() {
        // Standing at x=0.5 with half-width 0.3 spans 0.2..0.8, so the
        // cube spanning -1..0 only touches, never overlaps.
        assert!(!block_overlaps_player((0.5, 10.0, 0.5), -1, 10, 0, CUBE));
    }

    #[test]
    fn a_block_the_player_is_clipping_into_overlaps() {
        // Standing at x=0.1: the collider spans -0.2..0.4 and does reach
        // into the cube at x=-1.
        assert!(block_overlaps_player((0.1, 10.0, 0.5), -1, 10, 0, CUBE));
    }

    #[test]
    fn negative_coordinates_behave_the_same() {
        assert!(block_overlaps_player((-7.5, 3.0, -2.5), -8, 3, -3, CUBE));
        assert!(!block_overlaps_player((-7.5, 3.0, -2.5), -8, 1, -3, CUBE));
    }

    #[test]
    fn a_block_at_your_feet_is_inside_you_whatever_it_is_made_of() {
        // Laying material in the cell your feet are in used to be the
        // commonest placement in the game, and it worked because a
        // layer was shorter than a step. Nothing is any more, so the
        // cell your feet are in is refused for everything alike --
        // which is the rule the hitbox always had for whole blocks.
        let feet = (0.5, 10.0, 0.5);
        for id in [CUBE, BLOCK_SNOW, crate::types::BLOCK_SAND] {
            assert!(
                block_overlaps_player(feet, 0, 10, 0, id),
                "{} could be built into a player",
                crate::types::block_name(id)
            );
        }
    }

    #[test]
    fn nothing_you_can_walk_through_has_a_box_at_all() {
        for id in [BLOCK_AIR, BLOCK_WATER] {
            assert!(block_box(id, 0, 0, 0).is_none());
            assert!(!block_overlaps_player((0.5, 0.0, 0.5), 0, 0, 0, id));
        }
        let (min, max) = block_box(BLOCK_SNOW, 3, 4, 5).unwrap();
        assert_eq!(min, [3.0, 4.0, 5.0]);
        assert_eq!(max, [4.0, 5.0, 6.0], "snow is a whole block now");
    }

    #[test]
    fn what_you_aim_at_is_the_size_of_what_is_drawn() {
        use crate::types::{BLOCK_PEBBLE, BLOCK_TALL_GRASS};
        // A blade of grass with a metre cube around it is a selection
        // box around mostly nothing, and it lies about what a click
        // will hit.
        let (min, max) = block_target_box(BLOCK_TALL_GRASS, 0, 0, 0).unwrap();
        assert!(min[0] > 0.0 && max[0] < 1.0, "a tuft filled its whole cell");
        // As tall as the tallest a tuft is drawn, and no taller: a box
        // that stops short of the blade cannot be clicked at the top,
        // and one that runs past the cell would be clickable from the
        // block above.
        assert_eq!(max[1], 1.0, "a tuft is drawn up to the cell it stands in");
        // A stone lying on the ground is thin, but not so thin that
        // only a ray exactly level with it can hit it.
        let (min, max) = block_target_box(BLOCK_PEBBLE, 0, 0, 0).unwrap();
        assert!(max[1] - min[1] > 0.0 && max[1] - min[1] < 0.2);
        // A block is aimed at exactly as high as it is walked on.
        assert_eq!(
            block_target_box(BLOCK_SNOW, 0, 0, 0).unwrap().1[1],
            block_box(BLOCK_SNOW, 0, 0, 0).unwrap().1[1]
        );
        // ...and a ray goes through what is not there.
        for id in [BLOCK_AIR, BLOCK_WATER] {
            assert!(block_target_box(id, 0, 0, 0).is_none());
        }
    }

    #[test]
    fn a_ray_finds_a_box_it_starts_outside_and_misses_one_beside_it() {
        let hit = ray_hits_box([0.0, 0.5, 0.5], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 1.0], 10.0);
        assert_eq!(hit, Some(2.0));
        // Level with the box but pointing past it.
        assert_eq!(
            ray_hits_box([0.0, 0.5, 5.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 1.0], 10.0),
            None
        );
        // Parallel to a pair of planes and outside them: the division
        // this method exists to survive.
        assert_eq!(
            ray_hits_box([0.0, 9.0, 0.5], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 1.0], 10.0),
            None
        );
        // Out of reach.
        assert_eq!(
            ray_hits_box([0.0, 0.5, 0.5], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 1.0], 1.0),
            None
        );
    }

    #[test]
    fn nothing_in_the_world_can_be_stepped_over_any_more() {
        // The step height existed because layers did: a drift of snow
        // half a block deep had to be walked over rather than into.
        // With every solid filling its cell there is nothing left short
        // enough to step onto, and the constant survives only as the
        // bound that keeps it that way -- under a block, so no wall is
        // ever climbed for free.
        for &(id, name) in crate::types::ALL_BLOCK_IDS {
            let height = crate::types::collision_height(id);
            assert!(
                height == 0.0 || height > PLAYER_STEP_HEIGHT,
                "{name} is short enough to be walked up"
            );
        }
        const { assert!(PLAYER_STEP_HEIGHT < 1.0) }; // or it walks up walls
    }

    /// Someone standing four blocks north of the origin, feet on the
    /// ground at y = 10.
    fn target() -> (f32, f32, f32) {
        (0.0, 10.0, 4.0)
    }

    #[test]
    fn a_ray_down_the_middle_hits_at_the_near_face() {
        // Eye height on both sides, so the ray is level and enters the
        // collider at its near wall: four blocks less the half width.
        let eye = (0.0, 10.0 + EYE_HEIGHT, 0.0);
        let distance = ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 10.0)
            .expect("a level shot at chest height missed");
        assert!(
            (distance - (4.0 - PLAYER_HALF_WIDTH)).abs() < 1e-4,
            "entered at {distance}"
        );
    }

    #[test]
    fn a_ray_that_misses_misses() {
        let eye = (0.0, 10.0 + EYE_HEIGHT, 0.0);
        assert_eq!(ray_hits_player(eye, (1.0, 0.0, 0.0), target(), 10.0), None);
        assert_eq!(
            ray_hits_player(eye, (0.0, 0.0, -1.0), target(), 10.0),
            None,
            "a ray pointing the other way hit someone behind the player"
        );
        // Level with the feet but a metre to the side.
        assert_eq!(
            ray_hits_player((2.0, 10.9, 0.0), (0.0, 0.0, 1.0), target(), 10.0),
            None
        );
    }

    #[test]
    fn range_is_where_the_ray_stops() {
        let eye = (0.0, 10.0 + EYE_HEIGHT, 0.0);
        assert!(ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 4.0).is_some());
        assert_eq!(ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 2.0), None);
    }

    #[test]
    fn aiming_over_a_head_or_under_a_foot_is_a_miss() {
        // The collider is 1.8 tall, so a shot from three blocks up at a
        // shallow angle passes over it.
        let above = (0.0, 14.0, 0.0);
        assert_eq!(ray_hits_player(above, (0.0, 0.0, 1.0), target(), 10.0), None);
        let below = (0.0, 9.0, 0.0);
        assert_eq!(ray_hits_player(below, (0.0, 0.0, 1.0), target(), 10.0), None);
    }

    #[test]
    fn standing_inside_someone_counts_as_looking_at_them() {
        // Two players in the same cell: whatever way the ray goes, it
        // starts inside the box, and the distance is zero rather than
        // negative or nothing.
        let inside = (0.0, 11.0, 4.0);
        let distance = ray_hits_player(inside, (0.0, 0.0, 1.0), target(), 5.0)
            .expect("a ray starting inside the box missed it");
        assert_eq!(distance, 0.0);
    }

    #[test]
    fn a_ray_along_an_axis_it_is_flush_with_does_not_break_the_maths() {
        // The division by zero this exists to survive: exactly level
        // with a face, pointing along it.
        let flush = (PLAYER_HALF_WIDTH, 10.0, 0.0);
        // Whatever the answer is, it must be an answer rather than a
        // panic or a NaN.
        if let Some(d) = ray_hits_player(flush, (0.0, 0.0, 1.0), target(), 10.0) {
            assert!(d.is_finite());
        }
    }
}
