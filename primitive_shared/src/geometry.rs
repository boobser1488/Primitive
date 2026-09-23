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

use crate::types::{block_facing, block_kind, collision_depth, collision_height, is_branch, BlockId, BLOCK_AIR, BLOCK_PALM_TRUNK};

/// How far past the side of its own cell a block's box may reach, in blocks.
///
/// **Nothing reached past it until the palm**, and every gatherer of boxes
/// was written knowing so: the client's collider walked the cells its box
/// spans and nothing more. A palm's trunk is drawn along a line that leans
/// out of the piece it was grown in -- a third of a cell over the middle of
/// a run, half a cell and half the trunk's width at a step (`palm`) -- and
/// the slices it is walked into as are that line. A gatherer that did not
/// look this much further would let a player stand in the next cell with
/// their shoulder in the bark.
///
/// Half a cell because it is the most a slice can reach whatever its width:
/// the course never runs past the shared face of a step, and no piece is
/// wider than its cell. `palm::tests` holds the generator's palms under it.
pub const BOX_OVERHANG: f32 = 0.5;

/// Every box the block at (bx, by, bz) occupies, in world space, handed to
/// `visit`; `near(dx, dy, dz)` is the block at an offset from it.
///
/// **What `block_box` answers when it can see the world.** For everything
/// but a tree it is `block_box`, once. A piece of palm is the slices its
/// course is drawn in (`palm::trunk_slices`), because where its bark is
/// depends on the pieces above, below and beside it, and a box that does not
/// know that is the whole cell the player complained about -- or, asked
/// alone, an upright post the lean has moved away from.
///
/// **Every other piece of a tree is its wood** -- the post and the arms
/// `branch::wood_boxes` finds from the pieces beside it -- and that is a
/// twig as much as a bough: "добавь коллизию веткам". A bough was its whole
/// cell and a twig was nothing; see `branch` for the three ways weighed.
///
/// Callers that gather boxes from a region look `BOX_OVERHANG` past it. A
/// branch's boxes never leave their cell, so it needs no more than that.
pub fn for_each_block_box(
    block: BlockId,
    bx: i32,
    by: i32,
    bz: i32,
    near: impl Fn(i32, i32, i32) -> BlockId,
    mut visit: impl FnMut([f32; 3], [f32; 3]),
) {
    // A bite is one box and nothing round it depends on the world, so the
    // world-aware gatherer and the lone one give the same answer -- which
    // is what keeps a half-dug wall the same wall to the collider, to the
    // dropped stack rolling past it and to the animal walking along it.
    if let Some((min, max)) = crate::dig::part_box(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        return;
    }
    if block_kind(block) == BLOCK_PALM_TRUNK {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        for slice in crate::palm::trunk_slices(block, near) {
            let (min, max) = slice.bounds();
            visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        }
        return;
    }
    if is_branch(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        for wood in crate::branch::wood_boxes(block, crate::branch::joins(near)) {
            let (min, max) = wood.bounds();
            visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        }
        return;
    }
    // **A cell of a lean-to is the thatch it holds** (`lean_to::boxes`): the
    // hut's columns cut to the cell, the hollow over the bed left open. Its
    // cell would be a cube of air round a slope of leaves, and the old
    // pallet's two eighths let a body walk through the roof.
    if crate::lean_to::is_lean_to(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        crate::lean_to::boxes(block, |min, max| {
            visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        });
        return;
    }
    // **A step is its boxes** (`step_boxes`), and never its cell: the
    // cell is a metre wall, and a staircase of walls is a thing a player
    // jumps up rather than walks. `block_box` still answers the whole cell,
    // which is right for what asks it -- a placement beside a body, and the
    // outline. The ray is asked of these boxes (`physics::ray_enters_block`).
    if crate::types::is_step(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        for (min, max) in step_boxes(block, near).iter() {
            visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        }
        return;
    }
    // **A pit prop is a post**, half a cell across: in the middle of its cell
    // or against the wall it faces (`types::PROP_CENTRED`).
    // ...and a window lattice is its panel (`types::lattice_box`).
    if crate::types::is_lattice(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        let (min, max) = crate::types::lattice_box(block);
        visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        return;
    }
    if crate::types::is_prop(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        let (min, max) = crate::types::prop_box(block);
        visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        return;
    }
    // **A pile of logs is its logs**, one box each (`pit::pile_log_boxes`,
    // where the three ways it could have collided are weighed): one log is
    // stepped over, and a full pile is a block.
    if crate::pit::pile_extent(block).is_some() {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        for (min, max) in crate::pit::pile_log_boxes(block) {
            visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        }
        return;
    }

    // **A wild hive is a comb against a trunk**, not a cube of its cell: the
    // box is the half of the cell next to the tree (`types::hive_side`), so
    // a player climbing brushes the comb where the comb is drawn.
    //
    // **The half toward the wall the bits name, and that is the whole of a
    // bug report.** These four arms used to be turned by two quarters --
    // north named the +z half -- so the comb sat against the far wall of its
    // cell and the trunk was seven sixteenths behind it, with daylight in
    // between ("улей не прикреплён к дереву"). `hive_side` is the side it is
    // stuck to, not the way it looks: north is -z, and north is the -z half.
    if crate::bees::is_hive(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        const THIN: f32 = 9.0 / 16.0;
        const EDGE: f32 = 2.0 / 16.0;
        let (min, max) = match block_facing_of_hive(block) {
            0 => ([EDGE, 0.0, 0.0], [1.0 - EDGE, 1.0, THIN]),
            1 => ([1.0 - THIN, 0.0, EDGE], [1.0, 1.0, 1.0 - EDGE]),
            2 => ([EDGE, 0.0, 1.0 - THIN], [1.0 - EDGE, 1.0, 1.0]),
            _ => ([0.0, 0.0, EDGE], [THIN, 1.0, 1.0 - EDGE]),
        };
        visit([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]);
        return;
    }

    // **A standing torch is its pole**, not its two cells: a body walked
    // into an invisible metre-wide pillar round a stick two sixteenths
    // thick. The box is a little fatter than the pole drawn
    // (`mesh::standing_torch_block`, 7..9) so a shoulder does not clip the
    // wad on the top cell, and the top cell stops where the wad does.
    // `block_box` keeps the whole cell for placement and aim, as for a step.
    if let Some(top) = standing_torch_top(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        const LO: f32 = 5.5 / 16.0;
        const HI: f32 = 10.5 / 16.0;
        visit([x + LO, y, z + LO], [x + HI, y + top, z + HI]);
        return;
    }
    if let Some((min, max)) = block_box(block, bx, by, bz) {
        visit(min, max);
    }
}

/// Which quarter turn from north a hive's comb is stuck at, as a number.
fn block_facing_of_hive(block: BlockId) -> u32 {
    crate::types::hive_side(block).quarters()
}

/// How tall a standing torch's collider is in this cell, if it is one: the
/// whole pole in the lower cell, and up to the top of the wad in the upper.
fn standing_torch_top(block: BlockId) -> Option<f32> {
    use crate::types::{BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_LIT, BLOCK_STANDING_TORCH_OUT};
    match block_kind(block) {
        BLOCK_STANDING_TORCH => Some(1.0),
        BLOCK_STANDING_TORCH_LIT | BLOCK_STANDING_TORCH_OUT => Some(0.5),
        _ => None,
    }
}

/// How deep a step's tread is, from its front edge to the riser: half a
/// cell, so the two halves of a step are the same step twice.
///
/// **It was eleven sixteenths, and the upper half five**, to let a body
/// (`PLAYER_HALF_WIDTH` twice, 0.6) stand on the tread without its toes in
/// the riser -- the cure for "it puts me straight onto the second step".
/// That report had another cause, found in the same week:
/// `physics::settle_onto_step` lifted a body onto any box in the cells it
/// spanned, the riser included, touched or not. With that fixed a tread
/// half a cell deep is stood on as a stair is anywhere -- the middle of
/// the body over the tread, the heel over the edge -- and the deep tread
/// was left costing what a player sees first: "вторая ступень слишком
/// маленькая", an upper half a third the size of the lower, a ledge rather
/// than a step, and a roof whose courses did not match.
///
/// Rejected: keeping the deep tread and drawing the riser deeper than it
/// collides -- a step drawn one way and walked into another is the thing
/// `a_step_is_drawn_exactly_where_it_is_walked_into` exists to refuse.
pub const STEP_TREAD: f32 = 0.5;

/// Which shape a step takes from the steps beside it, in its written pose
/// (back to +z, as north's is): `Side` is the side of that pose, -x or +x,
/// the corner turns toward.
///
/// **A step at the corner of two flights is a corner**, the way a mason
/// cuts one. Without this the corner cell was a straight step like its
/// row, and an L of steps had a riser standing out past the other flight
/// by the depth of a tread at an outside corner and a hole the size of one
/// at an inside corner -- drawn, walked into and aimed at alike, which is
/// why it read as a strange model *and* a strange collision.
///
/// The rule, read off the two cells a corner can be made by:
///
/// * **Outside**: the step *behind* this one is a step turned a quarter
///   from it. Its riser and this one's meet in the one quarter they share,
///   and that quarter is all of this riser that is left -- a post the two
///   flights' risers both run into.
/// * **Inside**: the step *in front* is turned a quarter. This riser keeps
///   its whole length, and a second one runs along the side the other
///   flight rises toward, so the two flights' risers join round the corner.
///
/// Either way only if the cell on the far side of the turn is not a step
/// facing this one's way: that is the middle of a straight flight with a
/// crossing flight ending at it, and bending it would put a notch in a
/// flight that was straight.
///
/// Rejected: *corners as blocks of their own* (two more ids per material,
/// ten rows, placed by hand). The player would have to know they exist and
/// which of four to pick, and a flight that was built and then turned would
/// keep a straight step at its corner until somebody broke it -- what this
/// shape does from the neighbours, a row does from the player's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepShape {
    Straight,
    Outside(StepSide),
    Inside(StepSide),
}

/// A side of a step's written pose: toward -x or toward +x.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSide {
    Minus,
    Plus,
}

/// Where a point of a step's written pose lands for a step of this facing,
/// on the x/z plane of the cell. **The table `block_box` turns a frame by**
/// and `mesh::turned_from_north` turns a model by: a quarter from north is
/// east, whose back is -x. A step turned one way and collided another is a
/// staircase whose treads are where its risers are drawn, and
/// `a_step_is_drawn_exactly_where_it_is_walked_into` holds the two together.
fn step_pose_to_cell(quarters: u32, x: f32, z: f32) -> (f32, f32) {
    match quarters % 4 {
        1 => (1.0 - z, x),
        2 => (1.0 - x, 1.0 - z),
        3 => (z, 1.0 - x),
        _ => (x, z),
    }
}

/// The step, if `id` is one, and which way its back is in the world.
fn step_back(id: BlockId) -> Option<(i32, i32)> {
    crate::types::is_step(id).then(|| {
        let (fx, fz) = block_facing(id).step();
        (-fx, -fz)
    })
}

/// Which shape the step `block` takes; `near(dx, dy, dz)` is the block at an
/// offset from it. See [`StepShape`].
pub fn step_shape(block: BlockId, near: impl Fn(i32, i32, i32) -> BlockId) -> StepShape {
    let facing = block_facing(block);
    let (fx, fz) = facing.step();
    let (bx, bz) = (-fx, -fz);
    // The written pose's +x, in the world.
    let (ex, ez) = match facing.quarters() % 4 {
        1 => (0, 1),
        2 => (-1, 0),
        3 => (0, -1),
        _ => (1, 0),
    };
    let side_of = |(x, z): (i32, i32)| if x * ex + z * ez > 0 { StepSide::Plus } else { StepSide::Minus };
    let same_way = |dx: i32, dz: i32| {
        let other = near(dx, 0, dz);
        crate::types::is_step(other) && block_facing(other) == facing
    };
    // Turned a quarter: its back is across this one's, not along it.
    let across = |(x, z): (i32, i32)| x * bx + z * bz == 0;
    if let Some(back) = step_back(near(bx, 0, bz)).filter(|&back| across(back)) {
        if !same_way(-back.0, -back.1) {
            return StepShape::Outside(side_of(back));
        }
    }
    if let Some(back) = step_back(near(fx, 0, fz)).filter(|&back| across(back)) {
        if !same_way(back.0, back.1) {
            return StepShape::Inside(side_of(back));
        }
    }
    StepShape::Straight
}

/// The boxes of a step, at most three.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepBoxes {
    boxes: [([f32; 3], [f32; 3]); 3],
    len: usize,
}

impl StepBoxes {
    pub fn iter(&self) -> impl Iterator<Item = ([f32; 3], [f32; 3])> + '_ {
        self.boxes[..self.len].iter().copied()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A step's boxes in its written pose, back to +z, relative to its cell's
/// corner: the lower half of the cell whole (the tread), and over it the
/// riser behind the tread (`STEP_TREAD`) -- cut to a corner post, or joined
/// by a second riser round an inside corner, as `shape` says.
///
/// **What `mesh::step_block` draws**, turned by its facing there, and what
/// [`step_boxes`] collides, turned by the same table here: one list of
/// boxes, so the drawing and the collider cannot grow apart when a shape is
/// added.
///
/// Half a cell each rise because `PLAYER_STEP_HEIGHT` is half a cell and a
/// hair: the step-up rides both, so a flight is walked.
pub fn step_pose_boxes(shape: StepShape) -> StepBoxes {
    let t = STEP_TREAD;
    let tread = ([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]);
    let riser = ([0.0, 0.5, t], [1.0, 1.0, 1.0]);
    // The strip of the upper half a riser round the corner stands on, on
    // one side of the pose: as deep as a riser, from the front of the cell
    // to the riser it meets.
    let span = |side: StepSide| match side {
        StepSide::Minus => (0.0, 1.0 - t),
        StepSide::Plus => (t, 1.0),
    };
    let mut boxes = [tread, riser, riser];
    let len = match shape {
        StepShape::Straight => 2,
        StepShape::Outside(side) => {
            let (x0, x1) = span(side);
            boxes[1] = ([x0, 0.5, t], [x1, 1.0, 1.0]);
            2
        }
        StepShape::Inside(side) => {
            let (x0, x1) = span(side);
            boxes[2] = ([x0, 0.5, 0.0], [x1, 1.0, t]);
            3
        }
    };
    StepBoxes { boxes, len }
}

/// The boxes of a step, relative to its cell's corner: [`step_pose_boxes`]
/// of its [`step_shape`], turned to face the way it faces -- the low side
/// toward the placer, so the side they walk up from.
pub fn step_boxes(block: BlockId, near: impl Fn(i32, i32, i32) -> BlockId) -> StepBoxes {
    let quarters = block_facing(block).quarters();
    let mut out = step_pose_boxes(step_shape(block, near));
    for (min, max) in out.boxes.iter_mut().take(out.len) {
        let (ax, az) = step_pose_to_cell(quarters, min[0], min[2]);
        let (bx, bz) = step_pose_to_cell(quarters, max[0], max[2]);
        (min[0], max[0]) = (ax.min(bx), ax.max(bx));
        (min[2], max[2]) = (az.min(bz), az.max(bz));
    }
    out
}

/// `for_each_block_box`, in `f64` world coordinates that are exact however
/// far the cell is from zero.
///
/// **The `f32` form cannot say where a face is a long way out.** It adds the
/// cell to the shape in `f32`, and past 16 777 216 there is no `f32` between
/// two whole numbers -- nor, at ten million, any half: a slab's top at
/// `10 000 000.5` came back as a whole block, and a dropped stone rested on
/// air or sank into the slab depending on which way it rounded. A shape never
/// depends on where its cell is (every branch of `block_box` only *adds* the
/// cell), so the shape is asked for at the origin and the cell is added in
/// `f64`. `near` is asked by offset from the cell, as it always was.
pub fn for_each_block_box_f64(
    block: BlockId,
    bx: i32,
    by: i32,
    bz: i32,
    near: impl Fn(i32, i32, i32) -> BlockId,
    mut visit: impl FnMut([f64; 3], [f64; 3]),
) {
    let (x, y, z) = (f64::from(bx), f64::from(by), f64::from(bz));
    for_each_block_box(block, 0, 0, 0, near, |min, max| {
        visit(
            [x + f64::from(min[0]), y + f64::from(min[1]), z + f64::from(min[2])],
            [x + f64::from(max[0]), y + f64::from(max[1]), z + f64::from(max[2])],
        );
    });
}

/// The box round every slice of a piece of palm, relative to its cell's
/// corner: what a ray stops at and what the outline is drawn round.
fn palm_extent(block: BlockId, near: impl Fn(i32, i32, i32) -> BlockId) -> Option<([f32; 3], [f32; 3])> {
    crate::palm::trunk_slices(block, near).map(|slice| slice.bounds()).reduce(|(lo, hi), (min, max)| {
        (
            [lo[0].min(min[0]), lo[1].min(min[1]), lo[2].min(min[2])],
            [hi[0].max(max[0]), hi[1].max(max[1]), hi[2].max(max[2])],
        )
    })
}

/// **Where a thing set down lies**, as an offset from the middle of its own
/// cell's floor: down by [`crate::types::set_down_drop`] of `ground`, the
/// cell under it, and -- on a step -- across onto the tread. `None` where it
/// cannot lie at all. `near` is asked by offset from the *ground* cell, which
/// is what a step's shape is read from.
///
/// **A step's tread is the front half of its cell**, half a cell down; the
/// back half is the riser, whose top is the cell's own top. Lowered and left
/// in the middle, a knife lay half in the riser. So it goes to the middle of
/// the front half -- and round an inside corner, where the second riser takes
/// one side of the front half too, to the quarter that is left. A drawing
/// 0.45 across fits a quarter of a cell. Rejected: *laying it on the riser's
/// top*, which is the cell's own top and so needs no drop: a hand reaching
/// down a staircase puts a thing on the stair it can see, the tread, and the
/// riser's top is a sixteenth-deep lip nobody aims at.
pub fn set_down_rest(ground: BlockId, near: impl Fn(i32, i32, i32) -> BlockId) -> Option<[f32; 3]> {
    let drop = crate::types::set_down_drop(ground)?;
    if !crate::types::is_step(ground) {
        return Some([0.0, -drop, 0.0]);
    }
    // In the step's written pose the riser is toward +z and the tread is
    // z in 0..STEP_TREAD; an inside corner's second riser stands on one side
    // of that strip (`step_pose_boxes`).
    let across = match step_shape(ground, near) {
        StepShape::Inside(StepSide::Minus) => 0.75,
        StepShape::Inside(StepSide::Plus) => 0.25,
        _ => 0.5,
    };
    let (x, z) = step_pose_to_cell(block_facing(ground).quarters(), across, STEP_TREAD * 0.5);
    Some([x - 0.5, -drop, z - 0.5])
}

/// `block_box_for_aim`, for a caller that can see the world round the block.
///
/// **A leaning palm is aimed at where it leans.** One box round the cell's
/// slices rather than the slices themselves: an outline is one box, and the
/// few sixteenths of air it takes in beside a lean are a place a ray that
/// was going to hit the bark anyway stops a hair early.
pub fn block_box_for_aim_near(
    block: BlockId,
    bx: i32,
    by: i32,
    bz: i32,
    include_liquid: bool,
    near: impl Fn(i32, i32, i32) -> BlockId,
) -> Option<([f32; 3], [f32; 3])> {
    if block_kind(block) == BLOCK_PALM_TRUNK {
        let (min, max) = palm_extent(block, near)?;
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // **A piece of branch is aimed at where its wood is**: one box round its
    // post and its arms. Seen alone (`block_box_for_aim`) a twig is the middle
    // half of its cell, and the tip of a limb is mostly *arm* -- a bar from the
    // knot to the next piece that the middle half does not reach -- so a ray at
    // the wood a player could see went on to whatever stood behind it.
    //
    // Never thinner than `AIMABLE` on any axis, centred on the wood and kept
    // in its cell: a stem two sixteenths across is twelve centimetres, and a
    // box that size is a target only a steady hand hits, which is the pebble's
    // argument in `block_box_for_aim`.
    if is_branch(block) {
        const AIMABLE: f32 = 0.25;
        let (mut min, mut max) = crate::branch::wood_boxes(block, crate::branch::joins(near))
            .map(|wood| wood.bounds())
            .reduce(|(lo, hi), (min, max)| {
                ([lo[0].min(min[0]), lo[1].min(min[1]), lo[2].min(min[2])], [hi[0].max(max[0]), hi[1].max(max[1]), hi[2].max(max[2])])
            })?;
        for (lo, hi) in min.iter_mut().zip(max.iter_mut()) {
            if *hi - *lo < AIMABLE {
                let middle = (*lo + *hi) * 0.5;
                *lo = (middle - AIMABLE * 0.5).clamp(0.0, 1.0 - AIMABLE);
                *hi = *lo + AIMABLE;
            }
        }
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // **A thing set down is aimed at where it lies** (`set_down_rest`): on a
    // lip, a slab or a step's tread, and not a wafer of air over it that a
    // ray at the knife passed under.
    if crate::types::is_set_down(block) {
        let rest = set_down_rest(near(0, -1, 0), |dx, dy, dz| near(dx, dy - 1, dz)).unwrap_or([0.0; 3]);
        let (mut min, mut max) = block_box_for_aim(block, bx, by, bz, include_liquid)?;
        // Kept in its column: moved onto a tread, the box a little wider than
        // the drawing would lean a twentieth into the next cell, which the
        // ray walking cells never asks this one about.
        let corner = [bx as f32, by as f32, bz as f32];
        for axis in 0..3 {
            min[axis] += rest[axis];
            max[axis] += rest[axis];
            if axis != 1 {
                min[axis] = min[axis].max(corner[axis]);
                max[axis] = max[axis].min(corner[axis] + 1.0);
            }
        }
        return Some((min, max));
    }
    // **Snow on a lip is aimed at on the lip**, where it is drawn
    // (`types::rest_drop`): the box of its own cell's floor was a wafer of
    // air a quarter of a block over the snow, and a sweep of the hand at the
    // white went through to the turf under it.
    // ...and a flower on a lip is aimed at on the lip, for the same reason
    // (`types::stand_drop`): a plant is drawn on the real top of its ground.
    if crate::types::is_flat(block) || crate::types::is_cross(block) {
        let under = if crate::types::is_plant_top(block) { near(0, -2, 0) } else { crate::types::BLOCK_AIR };
        let drop = crate::types::stand_drop(block, near(0, -1, 0), under);
        let (mut min, mut max) = block_box_for_aim(block, bx, by, bz, include_liquid)?;
        min[1] -= drop;
        max[1] -= drop;
        return Some((min, max));
    }
    block_box_for_aim(block, bx, by, bz, include_liquid)
}

/// Half the collider's width on X and Z.
pub const PLAYER_HALF_WIDTH: f32 = 0.3;
/// Total collider height, feet to crown.
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Camera height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;

/// The tallest lip a walking player rides over instead of stopping
/// dead against.
///
/// **This number existed because layers did**, and layers are gone. Half
/// a metre of drifted snow that a player had to *jump* was not drifted
/// snow, it was a wall the height of a chair; the step let a walker
/// ride over it. Snow fills its cell now and so does every other loose
/// material.
///
/// **It did not stop firing, and a comment here used to say it had.**
/// Two blocks are still shorter than their cell -- a campfire at a
/// quarter and a backpack at a half -- and walking into either of them
/// is a step-up, every time. Half a metre is exactly a backpack, which
/// is why the constant has the epsilon on it: without the `1e-3` a
/// player would walk into their own pack and stop dead against it.
///
/// Under a block, so no wall is ever climbed for free. Shared because
/// the server's anti-cheat has to expect whatever the client does --
/// and a step is the one ordinary move that looks like flight from the
/// server's side, which is what
/// `stepping_onto_a_part_height_block_is_never_mistaken_for_flying`
/// exists to hold down.
pub const PLAYER_STEP_HEIGHT: f32 = 0.5 + 1e-3;

/// The box a block occupies inside its cell, as (min, max) in world
/// space, or `None` if there is nothing solid there.
///
/// Loose material used to fill its cell in eighths and no longer does,
/// so *most* solids are their whole cell. Not all of them: a campfire
/// is a quarter of one and a backpack is a half, and those two are the
/// reason this returns a height rather than a yes. Anything you cannot
/// walk into is 0.0.
///
/// The height is not decoration. It is what the collider rests on, what
/// `PLAYER_STEP_HEIGHT` is measured against, and -- through
/// `collision_height` -- what the server's ground probe expects to find
/// under a player's feet. One answer, one place, because the client
/// walks on this while the server decides what may be built into it.
pub fn block_box(block: BlockId, bx: i32, by: i32, bz: i32) -> Option<([f32; 3], [f32; 3])> {
    // **A piece of branch asked without the world round it is a knot**: the
    // post `branch::wood_boxes` gives a piece with nothing beside it, which is
    // what the mesher draws of one alone. Where its wood really is depends on
    // its neighbours (`for_each_block_box`). Asked before the height, because
    // a twig's row is not collidable -- that line is about what a *cell* is to
    // the rules that rest things on one (`types::is_collidable`) -- and its
    // wood still stops a body.
    // **A block with a bite out of it is the box of what is left**, and it
    // is asked first because everything below assumes a block stands on its
    // own cell floor and reaches the walls of its cell. A bite out of the
    // underside does neither. One box, from `dig::bite_box`, so the thing a
    // player walks into is exactly the thing the mesher drew.
    if let Some((min, max)) = crate::dig::part_box(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    if is_branch(block) && block_kind(block) != BLOCK_PALM_TRUNK {
        let (min, max) = crate::branch::wood_boxes(block, crate::branch::joins(|_, _, _| BLOCK_AIR)).next()?.bounds();
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // **A spike of dripstone is its body**, for the twig's reason and asked
    // before the height for the same one: its row is not collidable, and a
    // stalactite's box hangs from the top of its cell, which a height
    // measured up from the floor cannot describe. See `dripstone::body`.
    // A pit prop is its post, for the aim and the placement beside a body
    // as much as for the collider (`types::prop_box`).
    if crate::types::is_lattice(block) {
        let (min, max) = crate::types::lattice_box(block);
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    if crate::types::is_prop(block) {
        let (min, max) = crate::types::prop_box(block);
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    if crate::dripstone::is_dripstone(block) {
        let (min, max) = crate::dripstone::body(block);
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // A pile of logs is the box round its logs: one log is a third of the
    // cell across, and the whole cell would be a metre of air a placement
    // beside a body is refused for.
    // ...and a cell of a lean-to is the box round the thatch it holds, which
    // is what a ray at it stops at (`lean_to::extent`).
    if let Some((min, max)) = crate::lean_to::extent(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    if let Some((min, max)) = crate::pit::pile_extent(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    let height = collision_height(block);
    if height <= 0.0 {
        return None;
    }
    let (x, y, z) = (bx as f32, by as f32, bz as f32);
    // **A piece of palm, asked without the world round it, is an upright
    // post** -- not its cell. Where its bark really is depends on its
    // neighbours (`for_each_block_box`); the cell was the invisible wall a
    // player walked into beside every trunk.
    if block_kind(block) == BLOCK_PALM_TRUNK {
        let (min, max) = palm_extent(block, |_, _, _| BLOCK_AIR)?;
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // Most solids are their whole cell across, and saying so first
    // keeps the common path a pair of adds.
    let Some((near, far)) = collision_depth(block) else {
        return Some(([x, y, z], [x + 1.0, y + height, z + 1.0]));
    };
    // A frame stands across one axis only, and which axis is which way
    // it was placed. The model is drawn along x when the block faces
    // north (see `mesh::rack_block`), and `push_box` turns it by
    // quarter turns from there -- so the box turns by the same quarters
    // from the same start, or the thing a player walks into is at right
    // angles to the thing they can see.
    // ...and a door turns one quarter further when it is open, about the
    // corner its two positions share (`types::door_quarters`).
    let quarters = if crate::types::is_door(block) {
        crate::types::door_quarters(block)
    } else {
        block_facing(block).quarters()
    };
    let ((x0, x1), (z0, z1)) = match quarters {
        1 => ((1.0 - far, 1.0 - near), (0.0, 1.0)),
        2 => ((0.0, 1.0), (1.0 - far, 1.0 - near)),
        3 => ((near, far), (0.0, 1.0)),
        _ => ((0.0, 1.0), (near, far)),
    };
    Some((
        [x + x0, y, z + z0],
        [x + x1, y + height, z + z1],
    ))
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
    block_box_for_aim(block, bx, by, bz, false)
}

/// What of a cell a stake driven into the wall to its north is drawn over,
/// (min, max) in cells: the extent of `misc/stake_wall.bbmodel`, rounded out
/// to the next quarter of a sixteenth.
pub const DRIVEN_STAKE_AIM: ([f32; 3], [f32; 3]) = ([1.5 / 16.0, 3.25 / 16.0, 0.0], [14.5 / 16.0, 11.75 / 16.0, 12.0 / 16.0]);

/// A box written against the wall to the north (-z), turned so that wall is
/// the one `types::support_at` says holds the block up: `quarters` of the
/// client's `push_box` turn, (x, z) about the middle to (z, -x), which is
/// the turn `wall_behind` makes of (0, -1).
pub fn turned_against_its_wall((lo, hi): ([f32; 3], [f32; 3]), quarters: u32) -> ([f32; 3], [f32; 3]) {
    let turn = |x: f32, z: f32| {
        let (mut cx, mut cz) = (x - 0.5, z - 0.5);
        for _ in 0..quarters {
            (cx, cz) = (cz, -cx);
        }
        (cx + 0.5, cz + 0.5)
    };
    let (ax, az) = turn(lo[0], lo[2]);
    let (bx, bz) = turn(hi[0], hi[2]);
    ([ax.min(bx), lo[1], az.min(bz)], [ax.max(bx), hi[1], az.max(bz)])
}

/// The same box, and whether water counts.
///
/// **This is the whole of "I cannot drink from anything".** The ray
/// under the crosshair stops at what `is_targetable` calls a target, and
/// water has never been one -- correctly, because a ray that stopped at
/// the surface of a lake would be a ray that cannot reach the sand under
/// it, and mining, placing and the crack overlay all ride on this. So a
/// right click at a river reached no cell at all, the client sent
/// nothing, and every drinking rule the server had grown was
/// unreachable: what the player saw was a game that quietly ignored
/// them.
///
/// The fix is not to make water targetable -- it is to let the *reason
/// for looking* decide. Breaking and placing ask the question they
/// always asked; the right click asks a wider one, and gets the surface
/// of the lake it is pointed at.
pub fn block_box_for_aim(
    block: BlockId,
    bx: i32,
    by: i32,
    bz: i32,
    include_liquid: bool,
) -> Option<([f32; 3], [f32; 3])> {
    use crate::types::{block_height, is_cross, is_flat, is_liquid, is_targetable};
    if !is_targetable(block) {
        // A mouthful is taken from the *surface*: the box is the cell's
        // own liquid height, so a player aiming at a river bank does not
        // drink from the block beside them.
        if include_liquid && is_liquid(block) {
            let top = by as f32 + block_height(block).max(0.1);
            return Some((
                [bx as f32, by as f32, bz as f32],
                [bx as f32 + 1.0, top, bz as f32 + 1.0],
            ));
        }
        return None;
    }
    // **A block half quarried is aimed at where the rock still is.** The
    // outline is drawn round this box and the ray stops at it, so the
    // second swing lands on the face the first one opened rather than on
    // the metre of air in front of it -- and a player who has cut a
    // doorway three quarters of the way through a wall can put their
    // crosshair through the gap onto what is behind.
    if let Some((min, max)) = crate::dig::part_box(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // A drying rack is the one block that is thin along *one* axis
    // rather than short or narrow all round: a frame of poles standing
    // on end, flat across the way it faces (see `mesh::rack_block`).
    // Nothing below can express that -- the inset is one number for both
    // horizontal axes -- so it is answered here.
    //
    // Worth answering rather than letting the frame keep the whole cell:
    // this box is what a ray stops at *and* what the breaking cracks are
    // drawn on, so a full cell would put half a metre of crack in the
    // air in front of the frame and let a player break it while aiming
    // at the gap beside it.
    // A pit prop is aimed at where its post is, so a second prop clicked
    // onto the side of the first lands beside the post it was aimed at
    // (`types::prop_box`).
    if crate::types::is_lattice(block) {
        let (min, max) = crate::types::lattice_box(block);
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    if crate::types::is_prop(block) {
        let (min, max) = crate::types::prop_box(block);
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + min[0], y + min[1], z + min[2]], [x + max[0], y + max[1], z + max[2]]));
    }
    // **The hide frame is aimed at where it is walked into**: its drawing is
    // its collider to a quarter of a sixteenth (`types::collision_depth`), and
    // the whole cell would put the cracks in the air in front of the skin.
    if block_kind(block) == crate::types::BLOCK_HIDE_FRAME {
        return block_box(block, bx, by, bz);
    }
    if block_kind(block) == crate::types::BLOCK_DRYING_RACK {
        // **The whole frame, splayed feet and all**: two A-frames whose poles
        // stand 2.5..13.5 of sixteen apart at the floor
        // (`assets/models/misc/drying_rack.bbmodel`), centred in the cell, so
        // one box holds every turn of it. The outline is drawn on this box,
        // and an outline that cut the feet off was a frame drawn in a box too
        // small for it. Do not "correct" it to match `types::collision_depth`:
        // what you *stand against* is the ridge at your chest, and what you
        // *aim at* is everything you can see.
        const THIN: f32 = 2.5 / 16.0;
        const THICK: f32 = 13.5 / 16.0;
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(match crate::types::block_facing(block) {
            crate::types::Facing::North | crate::types::Facing::South => {
                ([x, y, z + THIN], [x + 1.0, y + 1.0, z + THICK])
            }
            _ => ([x + THIN, y, z], [x + THICK, y + 1.0, z + 1.0]),
        });
    }

    // **A twig seen alone is aimed at in the middle of its cell**, half a
    // cell across and the whole of it tall. The whole cell would put a
    // metre of crack in the air round a pencil-thin stem and let a player
    // break a sapling while aiming at the grass beside it. The exact wood is
    // not knowable here -- which way its arms reach depends on the wood next
    // to it -- so the box is the part every twig has, the knot at the centre.
    // A caller that can see the world aims at the wood itself
    // (`block_box_for_aim_near`), and the ray under the crosshair does.
    // **A spike of dripstone is aimed at round its whole drawing**: the foot
    // of the largest is half a cell across, and a box the width of its body
    // would let a ray at the base go on to the rock behind. The full height
    // of its spike, so the cracks are drawn where the spike is.
    if crate::dripstone::is_dripstone(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        let (lo, hi) = crate::dripstone::tiers(block).fold(([16.0f32; 3], [0.0f32; 3]), |(lo, hi), (from, to)| {
            ([lo[0].min(from[0]), lo[1].min(from[1]), lo[2].min(from[2])], [hi[0].max(to[0]), hi[1].max(to[1]), hi[2].max(to[2])])
        });
        return Some((
            [x + lo[0] / 16.0, y + lo[1] / 16.0, z + lo[2] / 16.0],
            [x + hi[0] / 16.0, y + hi[1] / 16.0, z + hi[2] / 16.0],
        ));
    }
    if crate::types::is_twig(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + 0.25, y, z + 0.25], [x + 0.75, y + 1.0, z + 0.75]));
    }
    // **Stakes driven into a wall are aimed at round their spikes**
    // (`misc/stake_wall.bbmodel`): out from the wall three quarters of the
    // cell, from under a quarter up to three quarters. The cross box below
    // is the whole cell, which put the cracks and the click in the empty
    // corners round three points. A stake standing on the ground leans out
    // to every side of its cell and to its top, and the cross box is what
    // is drawn of it already. The client holds both to the drawing
    // (`a_stake_is_aimed_at_round_what_is_drawn_of_it`).
    if crate::types::is_stake(block) && !crate::types::stake_is_upright(block) {
        let (lo, hi) = turned_against_its_wall(DRIVEN_STAKE_AIM, crate::types::block_facing(block).quarters());
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + lo[0], y + lo[1], z + lo[2]], [x + hi[0], y + hi[1], z + hi[2]]));
    }
    // A piece of palm with nothing known round it: its upright post. See
    // `block_box_for_aim_near` for the palm as it stands.
    if block_kind(block) == BLOCK_PALM_TRUNK {
        return block_box(block, bx, by, bz);
    }

    // **A door is aimed at where its boards are**, the box it is walked
    // into: the rack's argument, and a sharper one, because the thing a
    // player aims at through an open doorway is the room beyond it -- a
    // whole-cell box would put the doorway itself in the way of every
    // swing through it, and draw the breaking cracks in the air where the
    // door used to be shut.
    if crate::types::is_door(block) {
        return block_box(block, bx, by, bz);
    }
    // ...and a pile of logs round its logs, for the same reason: a lone log
    // aimed at through a whole cell of outline was the cube it no longer is.
    if crate::pit::pile_extent(block).is_some() {
        return block_box(block, bx, by, bz);
    }
    // ...and a cell of a lean-to round the thatch it holds (`lean_to::extent`):
    // its row's two eighths would be a box on the floor under a roof.
    if crate::lean_to::is_lean_to(block) {
        return block_box(block, bx, by, bz);
    }

    // **A chair is aimed at up to the top of its back**, which is the
    // whole cell tall. Its row collides at the seat -- a sitter's feet
    // rest there -- and the table-row box below would stop at the seat
    // too, so a player aiming at the back of a chair would have hit
    // whatever stood behind it and broken that instead.
    if crate::types::block_kind(block) == crate::types::BLOCK_CHAIR {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x, y, z], [x + 1.0, y + 1.0, z + 1.0]));
    }

    // Fractions of the cell: (inset on x/z, height).
    // A carcass is aimed at as the animal lying on its side, not as the
    // slab its table row collides as: the whole cell across, and as
    // tall as the animal is wide, or the top half of a deer's flank
    // could not be hit and the settling outline drew a plate under it.
    // The client's model has the exact figure; this is the shared
    // approximation both sides agree on.
    if let Some(species) = crate::animals::Species::of_carcass(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        let height = if species == crate::animals::Species::Hare { 0.35 } else { 0.6 };
        return Some(([x, y, z], [x + 1.0, y + height, z + 1.0]));
    }
    // **A thing set down is aimed at round what is drawn**, not across its
    // cell: its row's eighth is the whole cell wide, and a click at the grass
    // beside a knife would take the knife. A little wider than the drawing
    // (`logic::entities::SET_DOWN_DRAWN` is 0.45 across), so a short blade
    // turned any way is still inside it.
    if crate::types::is_set_down(block) {
        let (x, y, z) = (bx as f32, by as f32, bz as f32);
        return Some(([x + 0.2, y, z + 0.2], [x + 0.8, y + 0.125, z + 0.8]));
    }
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
///
/// `feet` is an `f64`, and the test is made from the block's own cell: the
/// feet are moved into it rather than the box out to the world, because a
/// box out at ten million blocks has no half-blocks left in `f32` (see
/// `for_each_block_box_f64`) and a placement beside a player there was
/// refused or let through by a rounding.
pub fn block_overlaps_player(
    feet: (f64, f64, f64),
    bx: i32,
    by: i32,
    bz: i32,
    block: BlockId,
) -> bool {
    let Some((min_b, max_b)) = block_box(block, 0, by, 0) else {
        return false;
    };
    let (px, py, pz) = ((feet.0 - f64::from(bx)) as f32, feet.1 as f32, (feet.2 - f64::from(bz)) as f32);
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
///
/// Measured from the eye: `feet` less `origin` in `f64`, and only then
/// narrowed, for `block_overlaps_player`'s reason.
pub fn ray_hits_player(
    origin: (f64, f64, f64),
    dir: (f32, f32, f32),
    feet: (f64, f64, f64),
    max_distance: f32,
) -> Option<f32> {
    let feet = ((feet.0 - origin.0) as f32, (feet.1 - origin.1) as f32, (feet.2 - origin.2) as f32);
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
    let origin = [0.0f32; 3];
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

/// A world position widened to the `f64` it is kept in. See
/// `PROTOCOL_VERSION`'s fifty-seven for why positions are `f64`.
#[inline]
pub fn wide(p: (f32, f32, f32)) -> (f64, f64, f64) {
    (f64::from(p.0), f64::from(p.1), f64::from(p.2))
}

/// A world position narrowed to `f32`, for the reads that decide rather than
/// move -- an animal choosing where to run, a chest asked whether somebody is
/// at it -- where a sixteenth of a block a million blocks out changes nothing.
/// Nothing that is moved, collided or drawn goes through here.
#[inline]
pub fn narrow(p: (f64, f64, f64)) -> (f32, f32, f32) {
    (p.0 as f32, p.1 as f32, p.2 as f32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wild_hives_comb_is_collided_against_the_very_wall_its_bits_name() {
        // **"Улей не прикреплён к дереву."** `types::hive_side` is the wall
        // the comb is stuck to -- the trunk is on the other side of it -- and
        // these four arms were turned by two quarters, so a hive whose bits
        // said "north" filled the +z half and left seven sixteenths of air
        // between itself and the bark. The property, in the language of the
        // cell: the box reaches the named wall and stops short of the
        // opposite one.
        use crate::types::{hive_against, Facing};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let hive = hive_against(crate::bees::hive_holding(crate::bees::HIVE_FULL), facing);
            let mut boxes = Vec::new();
            for_each_block_box(hive, 0, 0, 0, |_, _, _| crate::types::BLOCK_AIR, |a, b| boxes.push((a, b)));
            assert_eq!(boxes.len(), 1, "a hive is one box of comb");
            let (min, max) = boxes[0];
            let (dx, dz) = facing.step();
            let axis = if dx != 0 { 0 } else { 2 };
            let step = if dx != 0 { dx } else { dz };
            // Toward the wall the facing names, the comb is flush with the
            // cell; away from it, it stops half way.
            let (near, far) = if step > 0 { (max[axis], min[axis]) } else { (min[axis], max[axis]) };
            let wall = if step > 0 { 1.0 } else { 0.0 };
            assert!((near - wall).abs() < 1e-6, "a {facing:?} hive does not touch its wall: {min:?}..{max:?}");
            assert!(
                (far - wall).abs() > 0.4,
                "a {facing:?} hive fills its whole cell instead of the half against the trunk: {min:?}..{max:?}"
            );
        }
    }

    #[test]
    fn a_window_lattice_is_a_thin_panel_across_the_middle_of_its_cell_and_is_collided_where_it_is() {
        use crate::types::{lattice_box, placed, BLOCK_WINDOW_LATTICE};
        for yaw in [0.0f32, 1.6, 3.1, -1.6] {
            let lattice = placed(BLOCK_WINDOW_LATTICE, yaw, (0, 1, 0));
            let (min, max) = lattice_box(lattice);
            let thin: Vec<usize> = (0..3).filter(|&a| max[a] - min[a] < 0.2).collect();
            assert_eq!(thin.len(), 1, "a lattice at yaw {yaw} is not a panel: {min:?}..{max:?}");
            let a = thin[0];
            assert!((min[a] + max[a] - 1.0).abs() < 1e-6, "a lattice at yaw {yaw} is not across the middle of its cell");
            let mut collided = Vec::new();
            for_each_block_box(lattice, 0, 0, 0, |_, _, _| crate::types::BLOCK_AIR, |a, b| collided.push((a, b)));
            assert_eq!(collided, vec![(min, max)]);
            assert_eq!(block_box_for_aim(lattice, 0, 0, 0, false), Some((min, max)));
        }
    }

    #[test]
    fn a_prop_set_against_a_wall_leans_on_that_wall_and_is_collided_and_aimed_where_it_stands() {
        use crate::types::{placed, prop_box, BLOCK_PROP};
        // The clicked face's normal points from the wall into the new cell.
        for (normal, wall_side) in [((1, 0, 0), 0usize), ((-1, 0, 0), 0), ((0, 0, 1), 2), ((0, 0, -1), 2)] {
            for yaw in [0.0f32, 1.0, 2.5, -2.0] {
                let prop = placed(BLOCK_PROP, yaw, normal);
                let (min, max) = prop_box(prop);
                // The wall is at -normal: the post touches that face of its cell.
                let touches = if normal.0 + normal.2 > 0 { min[wall_side] == 0.0 } else { max[wall_side] == 1.0 };
                assert!(touches, "a prop clicked onto a wall at {normal:?} (yaw {yaw}) stands at {min:?}..{max:?}");
                let mut collided = Vec::new();
                for_each_block_box(prop, 0, 0, 0, |_, _, _| crate::types::BLOCK_AIR, |a, b| collided.push((a, b)));
                assert_eq!(collided, vec![(min, max)], "collided where it stands");
                assert_eq!(block_box_for_aim(prop, 0, 0, 0, false), Some((min, max)), "aimed at where it stands");
            }
        }
        let floor = placed(BLOCK_PROP, 0.7, (0, 1, 0));
        let (min, max) = prop_box(floor);
        assert!(min[0] > 0.0 && max[0] < 1.0 && min[2] > 0.0 && max[2] < 1.0, "a prop on a floor stands in the middle");
    }

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

    /// A rack is walked past, not walked round.
    ///
    /// **Its height was right and its footprint was the whole cell.**
    /// The uprights do run floor to ceiling -- that argument is in
    /// `blocks.rs` and is not the one that was wrong -- but the frame
    /// is two and a half sixteenths deep, and collision was sixteen.
    /// A tanner could not step past their own rack: a square of poles
    /// they could see straight through stopped them like stone.
    ///
    /// The test is on the *gap*, not on the numbers: what a player
    /// cares about is that there is room beside it in its own cell, and
    /// that the room is on the side the frame is not.
    #[test]
    fn there_is_room_to_walk_past_a_drying_rack_in_its_own_cell() {
        use crate::types::{faced, Facing, BLOCK_DRYING_RACK};

        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let rack = faced(BLOCK_DRYING_RACK, facing);
            let (min, max) = block_box(rack, 0, 0, 0).expect("a rack is solid");

            // Full height, unchanged: this is the half that was right.
            assert!(
                (max[1] - min[1] - 1.0).abs() < 1e-6,
                "{facing:?}: the rack stopped being a full-height frame",
            );

            let across_x = max[0] - min[0];
            let across_z = max[2] - min[2];
            // One axis is the frame and the other is the way past it.
            let (thin, wide) = if across_x < across_z {
                (across_x, across_z)
            } else {
                (across_z, across_x)
            };
            assert!(
                (wide - 1.0).abs() < 1e-6,
                "{facing:?}: the frame stopped spanning its cell, {wide} across",
            );
            assert!(
                thin < 0.3,
                "{facing:?}: the frame is {thin} deep, which is not something to squeeze past",
            );
            // ...and the room left over is worth having.
            //
            // **It is not a doorway, and this assertion used to say it
            // was.** It asked that `1.0 - thin` clear 0.6, a player's
            // width, as though the free part of the cell were one
            // piece -- and it is not: the frame stands in the *middle*,
            // so the 0.84 left over is two strips of about 0.42 each
            // and nobody walks through a rack. That reading was wrong
            // about what the fix buys.
            //
            // What it buys is walking *along* one. A player hugging the
            // frame stands most of the way into its cell instead of
            // stopping at the cell wall, which is where a full-cell box
            // left them -- so the number that matters is the clearance
            // from each wall to the poles, on both sides, because a
            // rack is approached from both. See the client's
            // `a_row_of_drying_racks_can_be_walked_along_rather_than_only_faced`,
            // which is the same property with a player actually walking.
            let axis = if across_x < across_z { 0 } else { 2 };
            for (side, clear) in [("near", min[axis]), ("far", 1.0 - max[axis])] {
                assert!(
                    clear > 0.35,
                    "{facing:?}: only {clear} of the cell is free on the {side} side, \
                     which is not enough of it to be worth walking into",
                );
            }
        }
    }

    /// ...and the four rotations are four different doorways.
    ///
    /// A box that ignored the facing would pass the test above four
    /// times over and still be at right angles to the rack a player is
    /// looking at, which is worse than no fix: the wall would be
    /// somewhere the poles are not.
    #[test]
    fn turning_a_rack_turns_the_thing_you_walk_into() {
        use crate::types::{faced, Facing, BLOCK_DRYING_RACK};

        let north = block_box(faced(BLOCK_DRYING_RACK, Facing::North), 0, 0, 0).unwrap();
        let east = block_box(faced(BLOCK_DRYING_RACK, Facing::East), 0, 0, 0).unwrap();
        let thin_on_z = (north.1[2] - north.0[2]) < (north.1[0] - north.0[0]);
        let thin_on_x = (east.1[0] - east.0[0]) < (east.1[2] - east.0[2]);
        assert!(
            thin_on_z && thin_on_x,
            "a quarter turn did not move which axis the frame stands across",
        );
    }

    /// **Nothing is a floor that has no whole floor at the top of its cell.**
    ///
    /// `types::has_full_top` is the one question everything that rests on
    /// the block below asks -- a tuft, a torch, a drift of snow, a knife set
    /// down, the snowfall -- and all of them are drawn from the floor of
    /// their own cell, which is the top of the cell under them. A step
    /// answered yes (collidable, eight layers by its row), and a drift of
    /// snow landed on a roof of steps as a sheet at the height of the
    /// ridge, hanging over every tread with nothing under it.
    ///
    /// Asked of the boxes the collider stands on, standing alone: at the
    /// top of the cell, every sixteenth of the square is under a box that
    /// reaches it.
    #[test]
    fn nothing_is_a_floor_that_has_no_whole_floor_at_the_top_of_its_cell() {
        let mut wrong = Vec::new();
        for id in 0..=u16::MAX {
            let id = id as BlockId;
            if !crate::types::is_known_block(id) || !crate::types::has_full_top(id) {
                continue;
            }
            // Tree wood, left out on purpose: see `has_full_top`.
            if is_branch(id) || block_kind(id) == BLOCK_PALM_TRUNK {
                continue;
            }
            let mut boxes = Vec::new();
            for_each_block_box(id, 0, 0, 0, |_, _, _| BLOCK_AIR, |min, max| boxes.push((min, max)));
            let mut bare = 0;
            for i in 0..16 {
                for k in 0..16 {
                    let (x, z) = ((i as f32 + 0.5) / 16.0, (k as f32 + 0.5) / 16.0);
                    let held = boxes
                        .iter()
                        .any(|(min, max)| max[1] >= 1.0 - 1e-4 && (min[0]..=max[0]).contains(&x) && (min[2]..=max[2]).contains(&z));
                    if !held {
                        bare += 1;
                    }
                }
            }
            if bare > 0 {
                wrong.push(format!("{} ({id}): {bare} of 256 sixteenths of its top are air", crate::types::block_name(id)));
            }
        }
        assert!(wrong.is_empty(), "things stood on these hang over air:\n  {}", wrong.join("\n  "));
    }

    #[test]
    fn a_player_can_stand_on_the_lower_half_of_a_step() {
        // The middle of a body over the tread with its front short of the
        // riser: the tread has to be deeper than half a body, which is how
        // a stair is stood on anywhere.
        const { assert!(STEP_TREAD > PLAYER_HALF_WIDTH) };
        use crate::types::{faced, Facing, BLOCK_PLANK_STAIRS};
        let boxes = step_boxes(faced(BLOCK_PLANK_STAIRS, Facing::North), |_, _, _| BLOCK_AIR);
        let [tread, riser] = [boxes.boxes[0], boxes.boxes[1]];
        assert!(tread.1[1] <= 0.5 && riser.0[1] >= 0.5);
        let depth = (0..3).filter(|&a| a != 1).map(|a| riser.0[a].max(1.0 - riser.1[a])).fold(0.0_f32, f32::max);
        assert!(depth > PLAYER_HALF_WIDTH, "a tread {depth} deep is shallower than half a body");
    }

    /// **"вторая ступень слишком маленькая"**: the two halves of a step are
    /// one step twice -- the riser as deep as the tread in front of it and
    /// as tall as it, for every kind, facing and shape. The upper half was
    /// five sixteenths deep against the tread's eleven, a ledge a third the
    /// size of the step under it.
    #[test]
    fn the_upper_half_of_a_step_is_as_big_a_step_as_the_lower() {
        use crate::types::{faced, Facing, BLOCK_PLANK_STAIRS};
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let boxes = step_boxes(faced(BLOCK_PLANK_STAIRS, facing), |_, _, _| BLOCK_AIR);
            let [tread, riser] = [boxes.boxes[0], boxes.boxes[1]];
            let rise = |b: ([f32; 3], [f32; 3])| b.1[1] - b.0[1];
            let run = |b: ([f32; 3], [f32; 3])| (b.1[0] - b.0[0]).min(b.1[2] - b.0[2]);
            // What of the tread shows in front of the riser.
            let open = run(tread) - run(riser);
            assert!((rise(tread) - rise(riser)).abs() < 1e-6, "{facing:?}: rises {} and {}", rise(tread), rise(riser));
            assert!((open - run(riser)).abs() < 1e-6, "{facing:?}: a tread {open} deep under a riser {} deep", run(riser));
        }
    }

    /// **Every riser in a flight meets the riser beside it face to face**,
    /// along a straight flight and round both corners of an L, for every
    /// facing and both ways of turning.
    ///
    /// "у ступенек странная ... модель": the corner of two flights was a
    /// straight step like its row, so at an outside corner its riser stood
    /// out past the other flight by the depth of a tread, and at an inside
    /// corner a riser-sized hole opened where the two flights should have
    /// met. Asked of what is walked into, which
    /// `a_step_is_drawn_exactly_where_it_is_walked_into` holds to what is
    /// drawn: on every face two steps share, the risers touching it from one
    /// side cover exactly what the risers touching it from the other do.
    #[test]
    fn the_risers_of_a_flight_meet_face_to_face_along_it_and_round_its_corners() {
        use crate::types::{faced, is_step, Facing, BLOCK_PLANK_STAIRS};
        use std::collections::HashMap;
        let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
        // What of the risers of the step at `cell` touches its side `dir`,
        // as intervals across the face (the other horizontal axis).
        let touching = |world: &HashMap<(i32, i32), BlockId>, cell: (i32, i32), dir: (i32, i32)| {
            let near = |dx: i32, dy: i32, dz: i32| {
                if dy != 0 {
                    return BLOCK_AIR;
                }
                world.get(&(cell.0 + dx, cell.1 + dz)).copied().unwrap_or(BLOCK_AIR)
            };
            let (axis, across) = if dir.0 != 0 { (0, 2) } else { (2, 0) };
            let plane = if dir.0 + dir.1 > 0 { 1.0 } else { 0.0 };
            let mut spans: Vec<(f32, f32)> = step_boxes(world[&cell], near)
                .iter()
                .filter(|(min, _)| min[1] >= 0.5)
                .filter(|(min, max)| if plane > 0.5 { max[axis] >= 1.0 } else { min[axis] <= 0.0 })
                .map(|(min, max)| (min[across], max[across]))
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Adjacent spans are one span.
            let mut merged: Vec<(f32, f32)> = Vec::new();
            for (lo, hi) in spans {
                match merged.last_mut() {
                    Some(last) if lo <= last.1 + 1e-6 => last.1 = last.1.max(hi),
                    _ => merged.push((lo, hi)),
                }
            }
            merged
        };
        for facing in facings {
            let (fx, fz) = facing.step();
            let (bx, bz) = (-fx, -fz);
            for turn in [1, 3] {
                let other = facings[((facing.quarters() + turn) % 4) as usize];
                let (ox, oz) = other.step();
                // A flight of `facing` running along its side, ending at the
                // corner (0, 0); and a flight of `other` leaving the corner
                // along `facing`'s back (outside: the other flight's risers
                // meet this one's at the back) or its front (inside).
                for (name, leave) in [("outside", (bx, bz)), ("inside", (fx, fz))] {
                    let mut world = HashMap::new();
                    // The first flight runs away from the corner toward the
                    // side the second flight rises to at an outside corner,
                    // and away from it at an inside one: the two flights'
                    // high sides meet inside the L or outside it.
                    let run = if name == "outside" { (-ox, -oz) } else { (ox, oz) };
                    for k in 0..3 {
                        world.insert((run.0 * k, run.1 * k), faced(BLOCK_PLANK_STAIRS, facing));
                    }
                    for k in 1..4 {
                        world.insert((leave.0 * k, leave.1 * k), faced(BLOCK_PLANK_STAIRS, other));
                    }
                    for (&cell, &id) in &world {
                        assert!(is_step(id));
                        for dir in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let beside = (cell.0 + dir.0, cell.1 + dir.1);
                            if !world.contains_key(&beside) {
                                continue;
                            }
                            let here = touching(&world, cell, dir);
                            let there = touching(&world, beside, (-dir.0, -dir.1));
                            let close = here.len() == there.len()
                                && here.iter().zip(&there).all(|(a, b)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5);
                            assert!(
                                close,
                                "{facing:?} turning to {other:?}, {name} corner: the risers at {cell:?} meet the ones at {beside:?} as {here:?} against {there:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **A standing torch is walked into at its pole.** It used to be its whole
    /// cell, a metre-wide invisible pillar round a stick.
    #[test]
    fn a_standing_torch_is_walked_into_at_its_pole_not_its_cell() {
        use crate::types::{BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_LIT};
        for (kind, top) in [(BLOCK_STANDING_TORCH, 1.0), (BLOCK_STANDING_TORCH_LIT, 0.5)] {
            let mut boxes = Vec::new();
            for_each_block_box(kind, 0, 0, 0, |_, _, _| BLOCK_AIR, |min, max| boxes.push((min, max)));
            assert_eq!(boxes.len(), 1);
            let (min, max) = boxes[0];
            assert!(max[0] - min[0] < 0.4 && max[2] - min[2] < 0.4, "{kind}: the collider is wider than a pole");
            assert!((max[1] - top).abs() < 1e-6, "{kind}: the collider is {} tall", max[1]);
        }
    }

    /// **A step is aimed at the box round what is walked into**, for every
    /// kind and facing: its tread spans the whole cell and its riser reaches
    /// the ceiling, so the hull of the two is the cell. Were a step ever cut
    /// shorter or narrower than its cell, the outline would stand round air
    /// and a ray through the air would break the step -- this goes red first.
    #[test]
    fn a_step_is_aimed_at_the_hull_of_the_boxes_it_is_walked_into_by() {
        use crate::types::{
            faced, Facing, BLOCK_BRANCH_ROOF, BLOCK_COBBLESTONE_STAIRS, BLOCK_PLANK_STAIRS, BLOCK_THATCH_ROOF,
            BLOCK_TILE_ROOF,
        };
        for kind in [BLOCK_PLANK_STAIRS, BLOCK_COBBLESTONE_STAIRS, BLOCK_TILE_ROOF, BLOCK_THATCH_ROOF, BLOCK_BRANCH_ROOF] {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let step = faced(kind, facing);
                let mut hull = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
                for_each_block_box(step, 3, 7, 5, |_, _, _| BLOCK_AIR, |min, max| {
                    for a in 0..3 {
                        hull.0[a] = hull.0[a].min(min[a]);
                        hull.1[a] = hull.1[a].max(max[a]);
                    }
                });
                let aim = block_box_for_aim(step, 3, 7, 5, false).expect("a step cannot be aimed at");
                assert_eq!(aim, hull, "{kind} {facing:?}: aimed at {aim:?}, walked into within {hull:?}");
            }
        }
    }

    /// **A step is walked up, from the side it was put down from.** Two
    /// boxes, each rise no more than `PLAYER_STEP_HEIGHT`, and the high half
    /// away from the placer -- or a flight built by walking up it faces the
    /// wrong way and is a wall at every tread.
    #[test]
    fn a_step_rises_away_from_its_placer_in_half_cells_a_walker_rides() {
        use crate::types::{faced, Facing, BLOCK_THATCH_ROOF, BLOCK_TILE_ROOF};
        for kind in [BLOCK_TILE_ROOF, BLOCK_THATCH_ROOF] {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let step = faced(kind, facing);
                let mut boxes = Vec::new();
                for_each_block_box(step, 3, 7, 5, |_, _, _| BLOCK_AIR, |min, max| boxes.push((min, max)));
                assert_eq!(boxes.len(), 2, "{facing:?}: a step is not a tread and a riser");
                let (tread, riser) = (boxes[0], boxes[1]);
                assert!((tread.1[1] - tread.0[1]) <= PLAYER_STEP_HEIGHT, "{facing:?}: the first rise is climbed, not walked");
                assert!((riser.1[1] - tread.1[1]) <= PLAYER_STEP_HEIGHT, "{facing:?}: the second rise is climbed, not walked");
                assert_eq!(riser.1[1], 8.0, "{facing:?}: a step is not a whole cell tall at its back");
                // The high half is on the far side of the cell's middle from
                // the placer, whose side `Facing::step` points at.
                let (sx, sz) = facing.step();
                let middle = ((riser.0[0] + riser.1[0]) * 0.5 - 3.5, (riser.0[2] + riser.1[2]) * 0.5 - 5.5);
                assert!(
                    middle.0 * (sx as f32) + middle.1 * (sz as f32) < -0.2,
                    "{facing:?}: the high half is at {middle:?}, toward the placer"
                );
            }
        }
    }

    /// **A shut door is across the back of its cell, an open one is along a
    /// side, and the two share the hinge.** Every half, every facing: the
    /// shut slab is at the face away from whoever hung it (`Facing::step`
    /// points at the placer), open leaves the doorway clear a player's width
    /// and more, and the two boxes overlap in exactly one corner column --
    /// which is what makes it a door swinging and not a slab jumping round
    /// the cell. What is aimed at is what is walked into.
    #[test]
    fn a_door_swings_about_one_corner_from_the_back_of_its_cell_to_a_side() {
        use crate::types::{door_swung, faced, Facing, BLOCK_DOOR, BLOCK_DOOR_TOP, DOOR_THICKNESS};
        // The doorway an open door leaves is wide enough for a body.
        const { assert!(1.0 - DOOR_THICKNESS > 2.0 * PLAYER_HALF_WIDTH + 0.1) };
        for kind in [BLOCK_DOOR, BLOCK_DOOR_TOP] {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let shut = faced(kind, facing);
                let open = door_swung(shut);
                let (lo, hi) = block_box(shut, 0, 0, 0).expect("a shut door is solid");
                let (olo, ohi) = block_box(open, 0, 0, 0).expect("an open door is solid");
                for (a, b) in [(lo, hi), (olo, ohi)] {
                    assert!((b[1] - a[1] - 1.0).abs() < 1e-6, "{facing:?}: a door half is not a whole cell tall");
                    let thin = (b[0] - a[0]).min(b[2] - a[2]);
                    assert!((thin - DOOR_THICKNESS).abs() < 1e-6, "{facing:?}: a door {thin} thick");
                }
                // The back: the middle of the slab is on the far side of the
                // middle of the cell from the placer.
                let (sx, sz) = facing.step();
                let middle = ((lo[0] + hi[0]) * 0.5 - 0.5, (lo[2] + hi[2]) * 0.5 - 0.5);
                assert!(
                    middle.0 * (sx as f32) + middle.1 * (sz as f32) < -0.3,
                    "{facing:?}: a shut door hangs at {middle:?}, not at the back of its cell"
                );
                // Open is a quarter round: the slab stands along the other axis.
                assert!((hi[0] - lo[0] > 0.5) != (ohi[0] - olo[0] > 0.5), "{facing:?}: opening did not turn the door");
                // ...about one corner the two share.
                let shared = |axis: usize| (hi[axis].min(ohi[axis]) - lo[axis].max(olo[axis])).max(0.0);
                assert!(
                    (shared(0) - DOOR_THICKNESS).abs() < 1e-6 && (shared(2) - DOOR_THICKNESS).abs() < 1e-6,
                    "{facing:?}: shut {lo:?}..{hi:?} and open {olo:?}..{ohi:?} do not share a hinge"
                );
                for door in [shut, open] {
                    assert_eq!(block_target_box(door, 0, 0, 0), block_box(door, 0, 0, 0), "{facing:?}: a door is aimed at somewhere it is not");
                }
            }
        }
    }

    /// **A leaning palm is walked into at its bark, not across its cells.**
    ///
    /// "у пальмы поломаны коллизия она не соответствует модели": every
    /// piece collided as its whole cell while the trunk was drawn along a
    /// line leaning out of them. What a piece holds now is its slices
    /// (`palm::trunk_slices`), asked with the palm round it -- the generator's
    /// own palms, every height and lean -- so a piece is never more than the
    /// post it is drawn as, and somewhere a palm leans the boxes lean with it.
    #[test]
    fn a_leaning_palm_is_walked_into_at_its_bark_and_not_across_its_cells() {
        use crate::types::BLOCK_PALM_TRUNK;
        let mut out_of_the_cell = 0;
        for variant in 0u32..32 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let cells = crate::worldgen::palm_cells(variant, lean);
                let at = |x: i32, y: i32, z: i32| cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id);
                for &((x, y, z), id) in &cells {
                    if block_kind(id) != BLOCK_PALM_TRUNK {
                        continue;
                    }
                    let mut boxes = Vec::new();
                    for_each_block_box(id, x, y, z, |dx, dy, dz| at(x + dx, y + dy, z + dz), |min, max| boxes.push((min, max)));
                    assert!(!boxes.is_empty(), "palm {variant:#x} has a piece at {:?} with nothing to walk into", (x, y, z));
                    // Ten sixteenths squared is 0.39 of the cell: the most bark
                    // one cell of trunk can hold, however it leans.
                    let volume: f32 = boxes.iter().map(|(min, max)| (0..3).map(|a| max[a] - min[a]).product::<f32>()).sum();
                    assert!(volume <= 0.4, "palm {variant:#x} collides as {volume} of its cell at {:?}", (x, y, z));
                    for (min, max) in &boxes {
                        let (cx, cz) = (x as f32, z as f32);
                        if min[0] < cx || max[0] > cx + 1.0 || min[2] < cz || max[2] > cz + 1.0 {
                            out_of_the_cell += 1;
                        }
                        assert!(min[1] >= y as f32 && max[1] <= y as f32 + 1.0, "a slice of palm left its cell's height");
                    }
                }
            }
        }
        assert!(out_of_the_cell > 0, "no palm's boxes lean out of their cells, so they do not follow the drawn lean");
    }

    #[test]
    fn a_twig_is_walked_into_at_its_wood_and_aimed_at_along_its_arm() {
        use crate::types::branch;
        // A twig two sixteenths across at the tip of a limb running along +x:
        // a bar from the bough it grows out of to its own knot.
        let near = |dx: i32, dy: i32, dz: i32| if (dx, dy, dz) == (-1, 0, 0) { branch(8) } else { BLOCK_AIR };
        let mut boxes = Vec::new();
        for_each_block_box(branch(2), 5, 10, 5, near, |min, max| boxes.push((min, max)));
        assert!(!boxes.is_empty(), "a twig is walked through");
        for (min, max) in &boxes {
            assert!(max[1] - min[1] <= 2.0 / 16.0 + 1e-6, "a limb's twig stood up as a post: {min:?}..{max:?}");
            assert!(max[2] - min[2] <= 2.0 / 16.0 + 1e-6, "a twig is wider than it is drawn: {min:?}..{max:?}");
        }
        // Aimed at all the way back to the face it joins, and never thinner
        // than a quarter of a cell.
        let (min, max) = block_box_for_aim_near(branch(2), 5, 10, 5, false, near).expect("a twig can be aimed at");
        assert!(min[0] <= 5.0 + 1e-6, "the arm back to the limb cannot be aimed at: {min:?}..{max:?}");
        assert!(max[1] - min[1] >= 0.25 - 1e-6 && max[2] - min[2] >= 0.25 - 1e-6, "a twig is a needle to aim at");
        // ...and a bough alone is its knot, not its cell.
        let (min, max) = block_box(branch(8), 5, 10, 5).expect("a bough stops a body");
        assert!(max[0] - min[0] < 1.0 && max[2] - min[2] < 1.0, "a bough is still its whole cell");
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
    fn snow_on_a_lip_is_aimed_at_on_the_lip_where_it_is_drawn() {
        use crate::types::{BLOCK_AIR, BLOCK_GRASS, BLOCK_SNOW_COVER};
        for quarters in 1..crate::dig::SLICES {
            let lip = crate::dig::lowered(BLOCK_GRASS, quarters);
            let under = |_: i32, dy: i32, _: i32| if dy == -1 { lip } else { BLOCK_AIR };
            let (min, max) = block_box_for_aim_near(BLOCK_SNOW_COVER, 0, 11, 0, false, under).expect("snow is aimed at");
            let top = 10.0 + f32::from(quarters) / 4.0;
            assert_eq!(min[1], top, "the snow on {quarters} quarters is aimed at above the lip");
            assert!(max[1] > top && max[1] < 11.0, "the snow's box on {quarters} quarters is {min:?}..{max:?}");
        }
        let whole = |_: i32, _: i32, _: i32| BLOCK_GRASS;
        let (min, _) = block_box_for_aim_near(BLOCK_SNOW_COVER, 0, 11, 0, false, whole).unwrap();
        assert_eq!(min[1], 11.0, "snow on whole turf moved");
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
    fn a_rack_is_aimed_at_where_the_frame_is() {
        // The box a ray stops at is the box the cracks are drawn on, and
        // both have to be the frame rather than the cell it stands in.
        use crate::types::{faced, Facing, BLOCK_DRYING_RACK};

        let north = block_target_box(faced(BLOCK_DRYING_RACK, Facing::North), 0, 10, 0)
            .expect("a rack can be aimed at");
        let east = block_target_box(faced(BLOCK_DRYING_RACK, Facing::East), 0, 10, 0)
            .expect("a rack can be aimed at");
        // Full height and full width, and as deep as the splayed feet of
        // its A-frames -- less than its width, so still turned the way it
        // faces. An outline that was a slab through the ridge cut the feet
        // off the frame it was drawn round.
        assert_eq!(north.0[1], 10.0);
        assert_eq!(north.1[1], 11.0);
        assert!(north.1[0] - north.0[0] > 0.9, "the frame lost its width");
        let deep = north.1[2] - north.0[2];
        assert!((11.0 / 16.0..0.9).contains(&deep), "the outline is {deep} deep, and the feet stand 11/16 apart");
        // ...and a quarter turn swaps the two.
        assert!(east.1[2] - east.0[2] > 0.9);
        assert!(east.1[0] - east.0[0] < 0.9);
        // Both stand inside their own cell.
        for corner in [north, east] {
            for axis in 0..3 {
                assert!(corner.0[axis] >= 0.0 && corner.1[axis] <= 11.0);
            }
        }
    }

    #[test]
    fn what_can_be_stepped_over_is_a_short_and_deliberate_list() {
        // The step height existed because layers did: a drift of snow
        // half a block deep had to be walked over rather than into. Then
        // layers went, and for two versions there was nothing in the
        // world short enough to step onto at all.
        //
        // There is again, and it is three blocks: a campfire and the two
        // states of a body, all half a cell tall. That is the whole point
        // of their being half a cell -- you step over your own hearth
        // and over the body you died in rather than being walled in
        // by either -- so this test is the list, and anything that joins
        // it should have to say so here.
        //
        // The drying rack was the fourth until it stopped being a slab:
        // a frame of poles standing on end with a skin stretched in it
        // is a thing you walk *round*. See `mesh::rack_block`.
        let steppable: Vec<&str> = crate::types::ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| {
                let height = crate::types::collision_height(id);
                height > 0.0 && height <= PLAYER_STEP_HEIGHT
            })
            .map(|&(_, name)| name)
            .collect();
        // ...and the carcasses, which are low heaps where an animal
        // fell: a thing you step over on the way to butchering it.
        // ...and the nest, full or empty, which is two eighths of twigs
        // on a branch: a player who climbed to it should be able to
        // stand beside it rather than be stopped by it.
        assert_eq!(
            steppable,
            [
                "backpack",
                "campfire",
                "campfire_lit",
                "carcass_hare",
                "carcass_deer",
                "carcass_boar",
                "carcass_wolf",
                "carcass_sheep",
                // ...a pat of dung, an eighth: walked over, and noticed
                // by the comfort of anybody near it rather than by the feet.
                "dung",
                // ...and the two later ones. A bear is half a cell of
                // dead animal and a bird is an eighth: both are things
                // you step over on the way to butchering them.
                "carcass_bear",
                "carcass_fowl",
                // ...and the savanna's three: a zebra is half a cell of
                // dead animal like the bear, an antelope a quarter.
                "carcass_zebra",
                "carcass_antelope",
                "carcass_lion",
                // ...and the horse's, half a cell like the zebra's.
                "carcass_horse",
                "nest_eggs",
                "nest",
                // ...and the three low pieces of furniture. A bed
                // and a stool are things you step over rather than walk
                // round; a table is waist high and is not on this list,
                // which is the difference between furnishing a room and
                // filling it with obstacles.
                "straw_bed",
                "bed",
                "stool",
                // ...and the chair, whose seat is the stool's height: the
                // back is drawn and aimed at, and a walker steps onto the
                // seat past it rather than stopping at a wall of air.
                "chair",
                // ...and a skeleton, which is the low heap a carcass
                // leaves when nobody came back for it -- both ids of it.
                "bones",
                "bones_2",
                "bones_3",
                "bones_4",
                // ...and a wall one course up, which is a quarter of a cell
                // of bricks, stones or cob: a kerb, walked over on the way to
                // laying the next course (`build`).
                "brick_courses",
                "dry_bricks",
                "dry_stone_wall",
                "cob_wall",
                // ...and the two low stages of a pit kiln, pots and fibre,
                // which sit in a hole a player may walk across rather than
                // fall into; the logs fill it to the rim. And the firepit,
                // lit and cold, which is a campfire's height for a
                // campfire's reason. See `pit`.
                "pit_kiln",
                "pit_kiln_fibre",
                // ...and a pile of one log -- the id with nothing in its count,
                // which is the first log laid. Its height is its logs'
                // (`pit::pile_log_boxes`): one log is a third of a cell and is
                // stepped over; four are two courses and are climbed.
                "log_pile",
                "firepit",
                "firepit_lit",
                // ...and the three roofing slabs, half a cell of tiles,
                // thatch or branches: a slab laid on a floor is a step up,
                // and a roof of them is walked along (`types::BLOCK_TILE_SLAB`).
                "tile_slab",
                "thatch_slab",
                "branch_slab",
                // The lean-to was here while its collider was a pallet of
                // leaves a body walked through the roof over; it is the hut
                // it is drawn as now (`lean_to::boxes`), and its mouth is
                // a cell of thatch and hollow, not a step.
                // ...and a dead player, in both of their states. The
                // body is half a cell where the bag was and the bones
                // two eighths, and a player must be able to step over
                // their own grave: a friend's body walled across a cave
                // mouth would be a door that could not be opened.
                "corpse",
                "remains",
                // ...and a pit's cover, which has to be walked onto as if it
                // were the ground round it -- that is the trap (`pitfall`) --
                // and a salt pan, a tray on the shore stepped into to fill it.
                "pit_cover",
                "salt_pan",
                "salt_pan_brine",
                "salt_pan_salt",
            ]
        );
        const { assert!(PLAYER_STEP_HEIGHT < 1.0) }; // or it walks up walls
    }

    /// ...and what is *narrower* than its cell is a shorter list still.
    ///
    /// The sibling of
    /// `what_can_be_stepped_over_is_a_short_and_deliberate_list`, and
    /// it exists for the same reason with the axes turned. "A solid
    /// block is as wide as its cell" was true of everything until the
    /// rack was drawn as a frame, and nothing in the code said so out
    /// loud -- so the collider went on building its own box out of two
    /// literal ones and a player was stopped by a metre of air.
    ///
    /// One block today. A second is a decision, and this is where it
    /// has to be written down.
    #[test]
    fn what_stands_narrower_than_its_cell_is_a_short_list() {
        let narrow: Vec<&str> = crate::types::ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| crate::types::collision_depth(id).is_some())
            .map(|&(_, name)| name)
            .collect();
        // ...and the door's two halves, a slab of boards across one side of
        // the cell: the second decision, written down. See `types::BLOCK_DOOR`.
        // ...and the hide frame, a skin laced into a standing frame of poles
        // (`types::BLOCK_HIDE_FRAME`), a sixteenth and a half deep each side
        // of the middle of its cell.
        assert_eq!(narrow, ["drying_rack", "door", "door_top", "hide_frame"]);

        // ...and nothing else *can* disagree with its model, because
        // nothing else is drawn as anything but a cube of its own
        // height. A tuft of grass and a pebble are `Shape::Cross` and
        // `Shape::Flat`, which `is_collidable` refuses outright: they
        // are aimed at (see `block_target_box`) and never walked into.
        for &(id, name) in crate::types::ALL_BLOCK_IDS {
            if crate::types::is_cross(id) || crate::types::is_flat(id) {
                assert!(
                    block_box(id, 0, 0, 0).is_none(),
                    "{name} is drawn as something other than a cube and still collides",
                );
            }
        }
    }

    /// Someone standing four blocks north of the origin, feet on the
    /// ground at y = 10.
    fn target() -> (f64, f64, f64) {
        (0.0, 10.0, 4.0)
    }

    #[test]
    fn a_ray_down_the_middle_hits_at_the_near_face() {
        // Eye height on both sides, so the ray is level and enters the
        // collider at its near wall: four blocks less the half width.
        let eye = (0.0, 10.0 + f64::from(EYE_HEIGHT), 0.0);
        let distance = ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 10.0)
            .expect("a level shot at chest height missed");
        assert!(
            (distance - (4.0 - PLAYER_HALF_WIDTH)).abs() < 1e-4,
            "entered at {distance}"
        );
    }

    #[test]
    fn a_ray_that_misses_misses() {
        let eye = (0.0, 10.0 + f64::from(EYE_HEIGHT), 0.0);
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
        // (See `far_from_zero_a_shot_is_aimed_as_it_is_at_home` for the
        // same shot a long way out.)
        let eye = (0.0, 10.0 + f64::from(EYE_HEIGHT), 0.0);
        assert!(ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 4.0).is_some());
        assert_eq!(ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 2.0), None);
    }

    /// **The same shot, ten million blocks out.** The box used to be built
    /// in world `f32`, which has no half-blocks there: a collider 0.6 wide
    /// was a whole block or nothing, and a shot that grazed somebody's
    /// shoulder at home missed them there, or hit the air beside them.
    #[test]
    fn far_from_zero_a_shot_is_aimed_as_it_is_at_home() {
        for far in [1_000_000.0, -10_000_000.0] {
            let shift = |(x, y, z): (f64, f64, f64)| (x + far, y, z - far);
            let eye = (0.0, 10.0 + f64::from(EYE_HEIGHT), 0.0);
            let home = ray_hits_player(eye, (0.0, 0.0, 1.0), target(), 10.0);
            assert_eq!(ray_hits_player(shift(eye), (0.0, 0.0, 1.0), shift(target()), 10.0), home);
            // A hair past the shoulder is a miss out there as it is here.
            let past = (f64::from(PLAYER_HALF_WIDTH) + 0.01, 10.9, 0.0);
            assert_eq!(ray_hits_player(shift(past), (0.0, 0.0, 1.0), shift(target()), 10.0), None);
            let inside = (f64::from(PLAYER_HALF_WIDTH) - 0.01, 10.9, 0.0);
            assert!(ray_hits_player(shift(inside), (0.0, 0.0, 1.0), shift(target()), 10.0).is_some());
            // ...and a block is refused beside a player by the same margin.
            let feet = (0.29 + far, 10.0, 0.5 - far);
            assert!(block_overlaps_player(feet, -1 + far as i32, 10, -far as i32, CUBE));
            let feet = (0.31 + far, 10.0, 0.5 - far);
            assert!(!block_overlaps_player(feet, -1 + far as i32, 10, -far as i32, CUBE));
        }
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
        let flush = (f64::from(PLAYER_HALF_WIDTH), 10.0, 0.0);
        // Whatever the answer is, it must be an answer rather than a
        // panic or a NaN.
        if let Some(d) = ray_hits_player(flush, (0.0, 0.0, 1.0), target(), 10.0) {
            assert!(d.is_finite());
        }
    }
}
