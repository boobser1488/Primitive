//! **Where the wood of a piece of branch is**: the post and the arms a twig or
//! a bough is drawn as, and the boxes a body walks into.
//!
//! The report this module answers: "добавь коллизию веткам". A bough was
//! walked into as its whole cell -- a trunk eight sixteenths wide stood in a
//! metre of invisible wall, and a limb running sideways through the air was a
//! cube a player could not pass under -- and a twig was walked through
//! altogether, so a sapling's stem was a picture with nothing in it.
//!
//! Three ways to make them agree were weighed:
//!
//! * *One box per cell, sized to the width*: an upright post over the middle
//!   of every piece. Right for a trunk, and wrong for everything else a tree
//!   is -- a limb is a bar lying along its cell, and the post would have been
//!   a pillar of air under it while its arm to the next piece was walked
//!   through.
//! * *The collider reading the mesher's quads back.* The mesher is the
//!   client's, and the server has to know where a body fits too
//!   (`body_fits`, an item coming to rest on a limb): a shape only one side
//!   can compute is the disagreement `geometry` exists to prevent.
//! * **One computation for both (chosen)**, as `palm` did for a palm's lean:
//!   which wood is beside a piece ([`joins`]) and the boxes that makes it
//!   ([`wood_boxes`]) live here, `mesh::branch_block` draws exactly those
//!   boxes, and `geometry::for_each_block_box` collides them.
//!   `a_piece_of_branch_is_drawn_exactly_where_it_is_walked_into` holds the
//!   two together.
//!
//! **Every box stays inside its own cell**, unlike a palm's slices: an arm
//! reaches the face it joins and stops there, and the neighbour draws its half.
//! So the gatherers of boxes need not look past the cells a body spans for a
//! branch, and the palm's ring (`geometry::BOX_OVERHANG`) is not widened.

use crate::types::{branch_width, has_full_top, is_branch, is_leafy, BlockId};

/// How wide the wood across each face of a piece is, in sixteenths, in the
/// mesher's face order (+Y, -Y, +X, -X, +Z, -Z) -- `None` where there is none.
pub type Joins = [Option<u8>; 6];

/// For each face, whether the wood across it goes on past both ends of this
/// piece's post, `[plus, minus]` along the post. See [`wood_boxes`] for the
/// hole in a thick trunk this closes.
pub type Beside = [[bool; 2]; 6];

/// Nothing beside a piece goes on anywhere: a piece drawn outside the world,
/// in a hand or a test.
pub const ALONE: Beside = [[false; 2]; 6];

/// The step to the cell across each face, in face order.
const STEPS: [(i32, i32, i32); 6] = [(0, 1, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

/// Which axis a face is across, and whether it is the positive side.
const FACE_AXIS: [(usize, bool); 6] = [(1, true), (1, false), (0, true), (0, false), (2, true), (2, false)];

/// What a piece of branch is joined to, read off the cells round it.
/// `near(dx, dy, dz)` is the block at an offset from the piece: one cell on
/// each face, and one up and one down beside each of the four sides.
///
/// **A foot stands on its ground.** A post that stopped at the middle of its
/// cell over the grass would be a trunk hovering half a metre up.
///
/// **A stem runs up into the leaves over it.** A sapling's stem and the top of
/// a trunk under its crown ended as a knot, at the middle of their cell and a
/// half-width over it, and the cube of leaves above sat on nothing a third of a
/// block up -- the second picture of "деревья странные". Joined to "a piece
/// nought wide", the post runs to its ceiling and keeps its cut end, which the
/// leaves hide. Only a stem with nothing beside it: one that forks under the
/// crown has its arms to hold the leaves, and turning a limb's end upright
/// would bend the limb.
///
/// Whether the wood beside an upright post goes on up and down with it -- a
/// column of trunk beside another -- is read here too. Down counts the ground
/// for the reason the foot does: the neighbour's post stands on it.
pub fn joins(near: impl Fn(i32, i32, i32) -> BlockId) -> (Joins, Beside) {
    let mut joins = [None; 6];
    for (face, (dx, dy, dz)) in STEPS.into_iter().enumerate() {
        joins[face] = branch_width(near(dx, dy, dz));
    }
    // **A dead twig on a fir grows out of the fir's trunk**, which is a
    // column of log cubes and not a piece (`worldgen::place_conifer`). Joined
    // to the log of its own wood across a side, as to a piece sixteen wide,
    // so it is drawn and walked into as a stub out of the bark rather than as
    // a knot standing in the air beside it. Only a bark with ids of its own
    // (`types::OWN_BARK`): an oak's or a birch's pieces never touch a log the
    // generator wrote, and are left exactly as they were.
    if let Some(log) = crate::types::piece_log(near(0, 0, 0)).filter(|&log| {
        crate::types::OWN_BARK.iter().any(|&(own, _, _)| own == log)
    }) {
        for (face, (dx, dy, dz)) in STEPS.into_iter().enumerate().skip(2) {
            if joins[face].is_none() && crate::types::block_kind(near(dx, dy, dz)) == log {
                joins[face] = Some(16);
            }
        }
    }
    if joins[1].is_none() && has_full_top(near(0, -1, 0)) {
        joins[1] = Some(16);
    }
    if joins[0].is_none() && joins[1].is_some() && joins[2..].iter().all(Option::is_none) && is_leafy(near(0, 1, 0)) {
        joins[0] = Some(0);
    }
    let mut beside = ALONE;
    if post_axis(&joins) == 1 {
        for (face, (dx, dy, dz)) in STEPS.into_iter().enumerate().skip(2) {
            if joins[face].is_some() {
                let below = near(dx, dy - 1, dz);
                beside[face] = [is_branch(near(dx, dy + 1, dz)), is_branch(below) || has_full_top(below)];
            }
        }
    }
    (joins, beside)
}

/// Which way a piece of branch's post runs, from its joins -- up if anything
/// joins it above or below, otherwise along the limb, and a knot stands up.
pub fn post_axis(joins: &Joins) -> usize {
    if joins[0].is_some() || joins[1].is_some() {
        1
    } else if joins[2].is_some() || joins[3].is_some() {
        0
    } else if joins[4].is_some() || joins[5].is_some() {
        2
    } else {
        1
    }
}

/// One box of wood in a piece of branch, in sixteenths of its cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodBox {
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// The faces buried in other wood, as bits in face order: not drawn, and
    /// nothing to do with where the box is. See [`wood_boxes`].
    pub open: u8,
}

impl WoodBox {
    /// The box as (min, max) corners relative to its cell's corner, in cells.
    /// Never past 0 or 1 on any axis.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.from.map(|v| v / 16.0), self.to.map(|v| v / 16.0))
    }
}

/// The boxes a piece of branch is: a square post of bark as wide as the piece
/// is, and an arm out to every piece of branch beside it.
///
/// **The post runs along the way the wood goes.** Up and down if anything
/// joins it from above or below -- a trunk, a sapling's stem, the upturned tip
/// of a limb -- and otherwise along the limb it is part of; a piece joined to
/// nothing stands upright as a knot. Every other join is an arm from the
/// post's side to the cell's face.
///
/// **An arm is as wide as the thinner of the two pieces.** Both cells make
/// their half of a join, and each can only see its own width and its
/// neighbour's: taking the smaller on both sides is what makes the two halves
/// meet at the cell face as one bar, where a limb the width of its own post
/// would step out of a thin twig as a collar. The trunk's post is left as wide
/// as the trunk -- a limb grows out of the side of something thicker, and the
/// step is what a fork looks like.
///
/// **Faces buried in the wood beside them are open** ([`WoodBox::open`]). The
/// end of a post against a piece at least as wide, and both ends of every arm,
/// are inside something else; drawn, they were two thirds of a trunk's quads
/// and nothing a player could see. The end of a thick post against a thinner
/// one stays: that ring of cut wood is the taper.
///
/// **An arm between two upright posts runs the height they run.** A trunk
/// wider than a cell is columns of pieces side by side, each a post a little
/// narrower than its cell, joined by arms. An arm centred in its cell stops
/// short of the floor and the ceiling by as much as the post is narrow, and
/// at every cell boundary up the trunk the two short ends left a hole two
/// sixteenths square through the seam between the columns: "деревья
/// странные", a thick trunk with a slit of sky in it at every block. Where the
/// wood across the face goes on up (or down) as this post does, the arm goes
/// on to the ceiling (or the floor) and meets the arm over it. Only for upright
/// posts: a limb's neighbour running the same way is rare, and one that is
/// really an upright with an arm out would be given a fin.
///
/// At most five: the post, and an arm on each of the four faces it does not
/// run through. A fixed array rather than a `Vec`, because the collider asks
/// this for every piece of tree round a player several times a frame.
pub fn wood_boxes(block: BlockId, (joins, beside): (Joins, Beside)) -> impl Iterator<Item = WoodBox> {
    let width = branch_width(block).unwrap_or(2) as f32;
    let (lo, hi) = (8.0 - width / 2.0, 8.0 + width / 2.0);
    let joined = |face: usize| joins[face].is_some();
    let axis = post_axis(&joins);
    // The two faces the post runs out through: +/- along `axis`.
    let (plus, minus) = match axis {
        0 => (2, 3),
        1 => (0, 1),
        _ => (4, 5),
    };

    let mut out = [None; 5];
    let mut from = [lo; 3];
    let mut to = [hi; 3];
    if joined(minus) {
        from[axis] = 0.0;
    }
    if joined(plus) {
        to[axis] = 16.0;
    }
    let mut open = 0u8;
    for face in [plus, minus] {
        if joins[face].is_some_and(|theirs| theirs as f32 >= width) {
            open |= 1 << face;
        }
    }
    out[0] = Some(WoodBox { from, to, open });

    let mut next = 1;
    for (face, &(arm_axis, positive)) in FACE_AXIS.iter().enumerate() {
        if face == plus || face == minus {
            continue;
        }
        let Some(theirs) = joins[face] else {
            continue;
        };
        let arm = (theirs as f32).min(width);
        let mut from = [8.0 - arm / 2.0; 3];
        let mut to = [8.0 + arm / 2.0; 3];
        if positive {
            from[arm_axis] = hi;
            to[arm_axis] = 16.0;
        } else {
            from[arm_axis] = 0.0;
            to[arm_axis] = lo;
        }
        // Both ends are buried: one in the post, one in the neighbour's half
        // of the same bar. `face ^ 1` is the opposite face.
        let mut open = (1u8 << face) | (1u8 << (face ^ 1));
        let [on_up, on_down] = beside[face];
        if on_up && joined(plus) {
            to[axis] = 16.0;
            open |= 1 << plus;
        }
        if on_down && joined(minus) {
            from[axis] = 0.0;
            open |= 1 << minus;
        }
        out[next] = Some(WoodBox { from, to, open });
        next += 1;
    }
    out.into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{branch, BLOCK_AIR, BLOCK_DIRT, BLOCK_LEAVES};

    /// A knot, a limb and a trunk foot, each with the wood a player sees.
    #[test]
    fn a_piece_of_branch_is_a_post_along_its_wood_and_an_arm_to_each_neighbour() {
        // Joined to nothing: a knot, as wide as it is, in the middle.
        let knot: Vec<WoodBox> = wood_boxes(branch(4), joins(|_, _, _| BLOCK_AIR)).collect();
        assert_eq!(knot.len(), 1);
        assert_eq!(knot[0].bounds(), ([6.0 / 16.0; 3], [10.0 / 16.0; 3]));

        // A limb along x between two thicker pieces: one bar from face to face.
        let limb = |dx: i32, dy: i32, dz: i32| if dy == 0 && dz == 0 && dx != 0 { branch(12) } else { BLOCK_AIR };
        let bar: Vec<WoodBox> = wood_boxes(branch(8), joins(limb)).collect();
        assert_eq!(bar.len(), 1, "a straight limb grew arms: {bar:?}");
        assert_eq!(bar[0].bounds(), ([0.0, 0.25, 0.25], [1.0, 0.75, 0.75]));

        // A trunk's foot on dirt with a twig out of its side: the post stands
        // on the floor, and the arm is the twig's width, not the trunk's.
        let foot = |dx: i32, dy: i32, dz: i32| match (dx, dy, dz) {
            (0, -1, 0) => BLOCK_DIRT,
            (0, 1, 0) => branch(10),
            (1, 0, 0) => branch(2),
            _ => BLOCK_AIR,
        };
        let boxes: Vec<WoodBox> = wood_boxes(branch(12), joins(foot)).collect();
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].bounds(), ([2.0 / 16.0, 0.0, 2.0 / 16.0], [14.0 / 16.0, 1.0, 14.0 / 16.0]));
        assert_eq!(boxes[1].from, [14.0, 7.0, 7.0]);
        assert_eq!(boxes[1].to, [16.0, 9.0, 9.0]);
    }

    #[test]
    fn a_twig_snaps_in_a_bare_hand_quicker_than_leaves_strip_and_a_bough_does_not() {
        // "возможность ломать тонкие ветки руками".
        use crate::types::{break_seconds, break_seconds_with, BLOCK_STICK};
        let leaves = break_seconds(BLOCK_LEAVES).expect("leaves come away by hand");
        for width in [2, 4, 6] {
            let twig = branch(width);
            let seconds = break_seconds_with(twig, None).unwrap_or_else(|| panic!("a twig {width} wide refused a bare hand"));
            // In the table's own units: `types::work_slowdown` charges a
            // branch three times and a leaf twice, on the player's word,
            // and what this test holds is the table's ordering under it.
            let table = seconds / crate::types::work_slowdown(twig);
            let leaves_table = leaves / crate::types::work_slowdown(BLOCK_LEAVES);
            assert!(table < leaves_table, "a twig {width} wide took {table}s by hand in the table, longer than leaves ({leaves_table}s)");
            assert_eq!(crate::blocks::definition(twig).drop, Some(BLOCK_STICK), "a twig gave something but a stick");
        }
        assert_eq!(break_seconds_with(branch(8), None), None, "a bough came apart in a bare hand");
    }

    #[test]
    fn a_stem_under_leaves_runs_up_into_them() {
        let stem = |dx: i32, dy: i32, dz: i32| match (dx, dy, dz) {
            (0, -1, 0) => BLOCK_DIRT,
            (0, 1, 0) => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        };
        let boxes: Vec<WoodBox> = wood_boxes(branch(4), joins(stem)).collect();
        assert_eq!(boxes[0].bounds().1[1], 1.0, "the stem stopped short of the leaves over it");
    }

    #[test]
    fn no_box_of_wood_leaves_its_own_cell() {
        // What lets the collider look no further than the cells a body spans
        // for a tree. Every width, every combination of the six joins.
        for width in (2..=16).step_by(2) {
            for mask in 0u32..64 {
                let near = |dx: i32, dy: i32, dz: i32| {
                    let face = STEPS.iter().position(|&s| s == (dx, dy, dz));
                    match face {
                        Some(face) if mask & (1 << face) != 0 => branch(((face as u8 + 1) * 2).min(16)),
                        _ => BLOCK_AIR,
                    }
                };
                for wood in wood_boxes(branch(width), joins(near)) {
                    let (min, max) = wood.bounds();
                    for a in 0..3 {
                        assert!(min[a] >= 0.0 && max[a] <= 1.0 && min[a] <= max[a], "{wood:?} left its cell");
                    }
                }
            }
        }
    }
}
