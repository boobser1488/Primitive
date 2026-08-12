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

/// Half the collider's width on X and Z.
pub const PLAYER_HALF_WIDTH: f32 = 0.3;
/// Total collider height, feet to crown.
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Camera height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;

/// Does the unit cube at (bx, by, bz) overlap the player standing with
/// their feet at `feet`?
///
/// Touching exactly counts as not overlapping: standing precisely on top
/// of a block must not make the block you're standing on unplaceable,
/// and the same goes for a wall you're flush against.
pub fn block_overlaps_player(feet: (f32, f32, f32), bx: i32, by: i32, bz: i32) -> bool {
    let (px, py, pz) = feet;
    let min_x = px - PLAYER_HALF_WIDTH;
    let max_x = px + PLAYER_HALF_WIDTH;
    let min_y = py;
    let max_y = py + PLAYER_HEIGHT;
    let min_z = pz - PLAYER_HALF_WIDTH;
    let max_z = pz + PLAYER_HALF_WIDTH;

    let (bx, by, bz) = (bx as f32, by as f32, bz as f32);

    min_x < bx + 1.0
        && max_x > bx
        && min_y < by + 1.0
        && max_y > by
        && min_z < bz + 1.0
        && max_z > bz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_at_the_players_feet_overlaps() {
        assert!(block_overlaps_player((0.5, 10.0, 0.5), 0, 10, 0));
    }

    #[test]
    fn a_block_at_head_height_overlaps() {
        // Feet at y=10 means the collider spans 10.0..11.8, so the cube
        // at y=11 is inside the player's chest/head.
        assert!(block_overlaps_player((0.5, 10.0, 0.5), 0, 11, 0));
    }

    #[test]
    fn the_block_being_stood_on_is_placeable() {
        // Feet exactly on top of the block at y=9 (which spans 9..10).
        assert!(
            !block_overlaps_player((0.5, 10.0, 0.5), 0, 9, 0),
            "the floor must not count as inside the player"
        );
    }

    #[test]
    fn a_block_just_above_the_head_is_placeable() {
        // Collider tops out at 11.8, so the cube spanning 12..13 is clear.
        assert!(!block_overlaps_player((0.5, 10.0, 0.5), 0, 12, 0));
    }

    #[test]
    fn a_block_flush_against_the_side_is_placeable() {
        // Standing at x=0.5 with half-width 0.3 spans 0.2..0.8, so the
        // cube spanning -1..0 only touches, never overlaps.
        assert!(!block_overlaps_player((0.5, 10.0, 0.5), -1, 10, 0));
    }

    #[test]
    fn a_block_the_player_is_clipping_into_overlaps() {
        // Standing at x=0.1: the collider spans -0.2..0.4 and does reach
        // into the cube at x=-1.
        assert!(block_overlaps_player((0.1, 10.0, 0.5), -1, 10, 0));
    }

    #[test]
    fn negative_coordinates_behave_the_same() {
        assert!(block_overlaps_player((-7.5, 3.0, -2.5), -8, 3, -3));
        assert!(!block_overlaps_player((-7.5, 3.0, -2.5), -8, 1, -3));
    }
}
