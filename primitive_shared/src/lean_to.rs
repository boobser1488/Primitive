//! **The lean-to as a thing of fifteen cells**: a debris hut three cells long,
//! three wide and two high, whose one model is drawn by the cell in its
//! middle and whose every cell collides as the part of the hut it holds.
//!
//! "какого хера шалаш размером с 2 блока и не имеет нормальной модели?" It
//! was the straw pallet's two cells with a roof drawn over them -- a tent
//! three quarters of a cell high that a sleeper's head and feet stuck out of,
//! wearing leaves as flat plates. What it is now is what a debris hut is: a
//! **ridge pole** propped in a **fork** at the mouth and running down to the
//! ground behind, **ribs** of sticks leaned on it from both sides, a **thatch**
//! of leaves laid over the ribs in courses from the ground up with sticks
//! thrown over them to hold them down, and a **bed of leaves** inside -- the
//! shape in every bushcraft manual and on Wikipedia's "Debris hut", because a wedge is
//! the least thatch that keeps a body's warmth in.
//!
//! ## The size, and the shapes rejected
//!
//! **Three cells long**: a body is 1.8 and lies in the two front cells
//! (`BED_HEAD` and the mouth are the straw pallet's two halves, so sleeping
//! is the pallet's), and the third cell is the tail where the ridge comes
//! down to the ground. **Three wide**: the ribs splay to a cell and a half
//! across at the mouth, and a hut one cell wide was the tent this replaces.
//! **Two high over its front two rows**: the fork stands a cell and a half,
//! the thatch over it a little more than a body's height, and the back row
//! is under a cell. The top row over the back cell would hold nothing.
//!
//! * *A lean-to proper* -- one sloped roof on a ridge between two trees,
//!   open along its side -- is the shelter of a fire in front of it, and
//!   keeps rain off only the half nearest the roof. It is a camp for a
//!   night in summer, not the shelter the rain and the cold here ask for.
//! * *A hut you stand up in* would need a ridge at 2.7 blocks for a body of
//!   1.8 to fit under an A three wide: three rows of cells, twenty-seven of
//!   them, and a house's worth of thatch for one night.
//! * *A hut you crawl into* would need a crawl -- a posture with its own
//!   collider, its own camera and its own case in the anti-cheat. There is
//!   no crouch in this game ("There is no crouch key", `physics`), and a
//!   mechanic that big for one block is the wrong way round.
//!
//! So **the inside is for sleeping in**: the hollow under the ribs is left
//! open to the collider (`boxes`), which is why it can be seen into and
//! aimed at and a sleeper lies in it, and it is a cell high at the mouth --
//! lower than a standing body, so nobody walks in. The gesture that lies a
//! body down is the way in, as it is for a bed.
//!
//! ## Which cell is which
//!
//! Every cell says which of the fifteen it is in its own id, the rack's
//! arrangement (`types::RACK_TOP`): the facing takes the two orientation
//! bits, and the four bits the hut spends on its part are the third variant
//! bit (`BED_HEAD`) and the three a furniture's wood takes (`WOOD_MASK`) --
//! a hut is not wooden furniture and never wet (`wet::gets_wet`). The two
//! cells a body lies across are the parts whose wood bits are nought, so the
//! mouth reads as the foot of a bed and the middle as its head, and
//! `types::bed_partner` pairs them without knowing about huts.
//!
//! **The middle cell is the anchor**: it draws the whole model
//! (`mesh::lean_to_block` in the client) and keys the sleeper. Not the mouth,
//! the cell a player puts down: every part of the hut is within one cell of
//! the middle, which is as far as the mesher can see round a cell.

use crate::types::{block_facing, block_kind, faced, BlockId, Facing, BED_HEAD, BLOCK_LEAN_TO, WOOD_HIGH_BIT, WOOD_LOW_SHIFT};

/// A sixteenth of a cell: every number below is written in them, as the
/// model is (`assets/models/furniture/lean_to.bbmodel`).
const T: f32 = 1.0 / 16.0;

/// Every bit of an id that says which part of the hut a cell is.
pub const PART_MASK: BlockId = (0b11 << WOOD_LOW_SHIFT) | WOOD_HIGH_BIT | BED_HEAD;

/// The fifteen parts: a part's number, and where its cell is from the
/// anchor as the model is written -- across (the model's x), up, and along
/// (the model's z, toward the mouth). The model is written with the mouth
/// toward +z, the way a bed is written with its foot there (`bed_quarters`
/// in the client's mesh), and turned by the same count.
///
/// Numbers 0 and 8 are the mouth and the anchor, the foot and the head of
/// the bed the hut is; the rest are the walls and the roof.
pub const PARTS: [(u8, (i32, i32, i32)); 15] = [
    (0, (0, 0, 1)),
    (8, (0, 0, 0)),
    (1, (-1, 0, 1)),
    (2, (1, 0, 1)),
    (3, (-1, 0, 0)),
    (4, (1, 0, 0)),
    (5, (-1, 0, -1)),
    (6, (0, 0, -1)),
    (7, (1, 0, -1)),
    (9, (-1, 1, 1)),
    (10, (0, 1, 1)),
    (11, (1, 1, 1)),
    (12, (-1, 1, 0)),
    (13, (0, 1, 0)),
    (14, (1, 1, 0)),
];

/// The part number's bits, where an id carries them: the low three in the
/// wood bits ten, eleven and fifteen, the fourth in `BED_HEAD`.
const fn part_bits(part: u8) -> BlockId {
    let mut bits = 0;
    if part & 1 != 0 {
        bits |= 1 << WOOD_LOW_SHIFT;
    }
    if part & 2 != 0 {
        bits |= 1 << (WOOD_LOW_SHIFT + 1);
    }
    if part & 4 != 0 {
        bits |= WOOD_HIGH_BIT;
    }
    if part & 8 != 0 {
        bits |= BED_HEAD;
    }
    bits
}

/// Which part a cell of a hut is, by its bits.
#[inline]
pub fn part_of(id: BlockId) -> u8 {
    let mut part = 0;
    if id & (1 << WOOD_LOW_SHIFT) != 0 {
        part |= 1;
    }
    if id & (1 << (WOOD_LOW_SHIFT + 1)) != 0 {
        part |= 2;
    }
    if id & WOOD_HIGH_BIT != 0 {
        part |= 4;
    }
    if id & BED_HEAD != 0 {
        part |= 8;
    }
    part
}

/// Is this a cell of a lean-to?
#[inline]
pub fn is_lean_to(id: BlockId) -> bool {
    block_kind(id) == BLOCK_LEAN_TO
}

/// Is this cell one of the two a body lies across -- the mouth or the
/// anchor? The rest are walls and roof, and are no bed.
#[inline]
pub fn is_bed_part(id: BlockId) -> bool {
    is_lean_to(id) && id & (PART_MASK & !BED_HEAD) == 0
}

/// Is this the anchor, the cell that draws the hut?
#[inline]
pub fn is_anchor(id: BlockId) -> bool {
    is_lean_to(id) && part_of(id) == 8
}

/// Where this part's cell is from the anchor as the model is written, or
/// `None` for a number that is no part (fifteen) or a cell that is no hut.
#[inline]
pub fn written_offset(id: BlockId) -> Option<(i32, i32, i32)> {
    if !is_lean_to(id) {
        return None;
    }
    let part = part_of(id);
    PARTS.iter().find(|&&(p, _)| p == part).map(|&(_, at)| at)
}

/// How many of the client's `push_box` quarter turns lay the model, written
/// mouth toward +z, with its mouth toward whoever put it down: the bed's
/// count (`bed_quarters` in the client's mesh), because the mouth is where
/// the bed's foot is. Held to the drawing by the client's
/// `a_lean_to_is_drawn_where_it_is_walked_into_whichever_way_it_faces`.
#[inline]
pub fn quarters(facing: Facing) -> u32 {
    match facing {
        Facing::South => 0,
        Facing::East => 1,
        Facing::North => 2,
        Facing::West => 3,
    }
}

/// A place in the anchor's plane turned by `push_box`'s quarter turn: about
/// the middle of the anchor cell, (x, z) to (z, -x) per quarter. `centre`
/// is the middle in the unit the place is written in.
#[inline]
fn turn(quarters: u32, (x, z): (f32, f32), centre: f32) -> (f32, f32) {
    let (cx, cz) = (x - centre, z - centre);
    let (rx, rz) = match quarters % 4 {
        1 => (cz, -cx),
        2 => (-cx, -cz),
        3 => (-cz, cx),
        _ => (cx, cz),
    };
    (rx + centre, rz + centre)
}

/// Where a part's cell is from the anchor, in the world, for a hut facing
/// this way.
#[inline]
pub fn offset(facing: Facing, (across, up, along): (i32, i32, i32)) -> (i32, i32, i32) {
    // Whole cells turn about the anchor's middle as its corner does about
    // nought: turned as the places half a cell further on, then taken back.
    let (x, z) = turn(quarters(facing), (across as f32 + 0.5, along as f32 + 0.5), 0.5);
    (x.floor() as i32, up, z.floor() as i32)
}

/// One cell of a hut facing this way.
#[inline]
pub fn cell(facing: Facing, part: u8) -> BlockId {
    faced(BLOCK_LEAN_TO, facing) | part_bits(part)
}

/// The anchor of the hut this cell belongs to, worked out from its own bits.
#[inline]
pub fn anchor(at: (i32, i32, i32), id: BlockId) -> (i32, i32, i32) {
    let Some(written) = written_offset(id) else {
        return at;
    };
    let (dx, dy, dz) = offset(block_facing(id), written);
    (at.0 - dx, at.1 - dy, at.2 - dz)
}

/// Every cell of a hut whose anchor is at `anchor`, the mouth first and the
/// anchor second.
pub fn cells(anchor: (i32, i32, i32), facing: Facing) -> [((i32, i32, i32), BlockId); 15] {
    PARTS.map(|(part, written)| {
        let (dx, dy, dz) = offset(facing, written);
        ((anchor.0 + dx, anchor.1 + dy, anchor.2 + dz), cell(facing, part))
    })
}

/// Every cell of a hut put down with its mouth at `mouth`: the cell a player
/// clicks is the mouth, and the hut runs back from it, away from them.
pub fn cells_from_mouth(mouth: (i32, i32, i32), facing: Facing) -> [((i32, i32, i32), BlockId); 15] {
    cells(anchor(mouth, cell(facing, 0)), facing)
}

/// The other fourteen cells of the hut this cell belongs to, and what each
/// must be. Empty for anything that is not a hut.
pub fn partners(at: (i32, i32, i32), id: BlockId) -> Vec<((i32, i32, i32), BlockId)> {
    if written_offset(id).is_none() {
        return Vec::new();
    }
    cells(anchor(at, id), block_facing(id)).into_iter().filter(|&(cell, _)| cell != at).collect()
}

/// Is the hut this cell belongs to standing whole? `block_at` answers `None`
/// for a cell nobody has, which is not whole.
pub fn whole(at: (i32, i32, i32), id: BlockId, block_at: impl Fn((i32, i32, i32)) -> Option<BlockId>) -> bool {
    written_offset(id).is_some() && partners(at, id).into_iter().all(|(cell, want)| block_at(cell) == Some(want))
}

// ---- what a body walks into ----
//
// **The hut in half-cell slices along its length, each an A**: a flat top at
// the highest thing drawn in the slice, sides pitched down to the ground as
// far out as anything is drawn, and under them the hollow the same shape at
// its own pitch -- a flat top at the underside of the ridge pole. The numbers
// are the model's, fitted over every point of its faces when it was written
// (the pitch that rides closest over the outside, the hollow that leaves the
// most room under the inside), and the client's
// `a_lean_to_is_drawn_where_it_is_walked_into_whichever_way_it_faces` holds
// every face of the model inside what collides and every face of what
// collides within seven sixteenths of the thatch, so an edit of the file in
// Blockbench that forgets these goes red rather than into a wall of air.
//
// **Columns two sixteenths across**, each one box from the hollow's roof to
// the thatch's top (or from the ground, where the hollow has run out). The
// roof pitches up to 1.7 in a sixteenth, so a column is a step of three and
// a half sixteenths at most under a slope -- a hand's width of air a body is
// stopped by, where a whole-cell step would be half a cell of it. Rejected:
// the model's own boxes as colliders. A leaf course is a slab turned about
// its length, and a collider is square to the world; the thatch would have to
// be cut into cubes to be walked into, and a heap of leaves of cubes is what
// the old model looked like.
//
// Rejected too: *the whole hollow solid*, a hut that is a heap to the
// collider. The mouth would be a wall a hand's width in front of the dark
// inside, a ray at the bed would stop at the air over it, and a sleeper
// would be put down inside a solid.

/// One half-cell slice of the hut along its length, in sixteenths as the
/// model is written: its extent along (z); the top of its thatch, how steeply
/// its outside falls and how far from the ridge that fall meets the ground,
/// and how far out anything is drawn at all; the top of the hollow, how
/// steeply it falls and how far from the ridge it meets the floor.
struct Slice {
    along: (f32, f32),
    roof: f32,
    pitch: f32,
    reach: f32,
    width: f32,
    hollow: f32,
    hollow_pitch: f32,
    hollow_reach: f32,
}

/// The ridge is over x = 8, the anchor cell's middle.
const RIDGE: f32 = 8.0;

const SLICES: [Slice; 6] = [
    Slice { along: (24.0, 32.0), roof: 26.75, pitch: 1.615, reach: 23.5, width: 22.0, hollow: 15.5, hollow_pitch: 1.105, hollow_reach: 14.75 },
    Slice { along: (16.0, 24.0), roof: 24.5, pitch: 1.16, reach: 23.75, width: 22.0, hollow: 11.5, hollow_pitch: 0.93, hollow_reach: 13.25 },
    Slice { along: (8.0, 16.0), roof: 22.5, pitch: 1.495, reach: 20.5, width: 18.0, hollow: 7.75, hollow_pitch: 0.68, hollow_reach: 12.0 },
    Slice { along: (0.0, 8.0), roof: 16.5, pitch: 0.825, reach: 22.5, width: 18.0, hollow: 4.0, hollow_pitch: 0.43, hollow_reach: 10.25 },
    Slice { along: (-8.0, 0.0), roof: 14.25, pitch: 1.0, reach: 18.5, width: 14.0, hollow: 2.25, hollow_pitch: 0.39, hollow_reach: 6.25 },
    Slice { along: (-16.0, -8.0), roof: 8.5, pitch: 0.32, reach: 29.0, width: 14.0, hollow: 2.25, hollow_pitch: 0.395, hollow_reach: 6.25 },
];

/// How wide one column of the collider is, in sixteenths.
const COLUMN: f32 = 2.0;

/// The bed of leaves: how far either side of the ridge, how high, and from
/// where to where along. Its own box, because the hollow's floor is the bed
/// and not the ground: a column over it keeps the air over the bed open.
const BED: (f32, f32, (f32, f32)) = (5.0, 2.0, (-6.0, 29.0));

/// The leaves raked over the rest of the hollow's floor: how far either
/// side of the ridge, how deep, and from where to where along. A carpet a
/// body does not notice, collided so that what is drawn is.
const CARPET: (f32, f32, (f32, f32)) = (12.5, 0.375, (-2.0, 29.5));

/// The top of the bed of leaves over its cell's floor, in cells: where a
/// sleeper lies (`lying_place` on the server).
pub const BED_TOP: f32 = BED.1 * T;

/// Every box of the whole hut as the model is written, in sixteenths from
/// the anchor cell's corner.
fn written_boxes(mut visit: impl FnMut([f32; 3], [f32; 3])) {
    for slice in &SLICES {
        let (z0, z1) = slice.along;
        let columns = (2.0 * 24.0 / COLUMN) as i32;
        for i in 0..columns {
            let u0 = -24.0 + i as f32 * COLUMN;
            let u1 = u0 + COLUMN;
            let near = if u0 < 0.0 && u1 > 0.0 { 0.0 } else { u0.abs().min(u1.abs()) };
            let far = u0.abs().max(u1.abs());
            let top = slice.roof.min(slice.pitch * (slice.reach - near));
            if top <= 0.05 || near >= slice.width {
                continue;
            }
            let hollow = slice.hollow.min(slice.hollow_pitch * (slice.hollow_reach - far));
            let floor = if far <= BED.0 { BED.1 } else { 0.0 };
            // Where the hollow has run down to within half a sixteenth of
            // its floor, the column is solid from the ground: a gap that low
            // is not a gap anything goes into.
            let bottom = if hollow <= floor + 0.5 { 0.0 } else { hollow };
            visit([RIDGE + u0, bottom, z0], [RIDGE + u1, top, z1]);
        }
    }
    for (half, depth, (z0, z1)) in [BED, CARPET] {
        visit([RIDGE - half, 0.0, z0], [RIDGE + half, depth, z1]);
    }
}

/// Every box this cell of a hut is walked into as, in cells from the cell's
/// own corner: the hut's boxes cut to the part of it the cell holds, turned
/// the way the hut faces.
pub fn boxes(id: BlockId, mut visit: impl FnMut([f32; 3], [f32; 3])) {
    let Some((across, up, along)) = written_offset(id) else {
        return;
    };
    let quarters = quarters(block_facing(id));
    let (wx, wy, wz) = offset(block_facing(id), (across, up, along));
    // The part of the written hut this cell holds.
    let lo = [across as f32 * 16.0, up as f32 * 16.0, along as f32 * 16.0];
    let hi = [lo[0] + 16.0, lo[1] + 16.0, lo[2] + 16.0];
    written_boxes(|from, to| {
        let from = [from[0].max(lo[0]), from[1].max(lo[1]), from[2].max(lo[2])];
        let to = [to[0].min(hi[0]), to[1].min(hi[1]), to[2].min(hi[2])];
        if (0..3).any(|a| to[a] - from[a] <= 1e-4) {
            return;
        }
        let (ax, az) = turn(quarters, (from[0], from[2]), 8.0);
        let (bx, bz) = turn(quarters, (to[0], to[2]), 8.0);
        let (x0, x1) = (ax.min(bx) * T - wx as f32, ax.max(bx) * T - wx as f32);
        let (z0, z1) = (az.min(bz) * T - wz as f32, az.max(bz) * T - wz as f32);
        visit([x0, from[1] * T - wy as f32, z0], [x1, to[1] * T - wy as f32, z1]);
    });
}

/// The box round every box of this cell, in cells from its corner: what a
/// ray stops at, what a placement beside a body asks, and -- its top -- how
/// tall the cell is to anything resting on it. `None` for no hut.
pub fn extent(id: BlockId) -> Option<([f32; 3], [f32; 3])> {
    let mut found: Option<([f32; 3], [f32; 3])> = None;
    boxes(id, |from, to| {
        found = Some(match found {
            None => (from, to),
            Some((lo, hi)) => (
                [lo[0].min(from[0]), lo[1].min(from[1]), lo[2].min(from[2])],
                [hi[0].max(to[0]), hi[1].max(to[1]), hi[2].max(to[2])],
            ),
        });
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FACINGS: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

    /// **Every cell of a hut names the same anchor and the same fifteen
    /// cells**, whichever way it faces, and the mouth is the cell toward
    /// whoever put it down -- `Facing::step` is toward the placer -- with the
    /// anchor behind it, as a bed's head is behind its foot.
    #[test]
    fn every_cell_of_a_lean_to_names_the_same_anchor_and_the_mouth_faces_its_builder() {
        for facing in FACINGS {
            let anchor_at = (10, 4, -7);
            let all = cells(anchor_at, facing);
            let mut places: Vec<_> = all.iter().map(|&(at, _)| at).collect();
            places.sort();
            places.dedup();
            assert_eq!(places.len(), 15, "{facing:?}: two parts in one cell");
            for (at, id) in all {
                assert_eq!(anchor(at, id), anchor_at, "{facing:?}: {id:#x} lost its anchor");
                assert!(whole(at, id, |c| all.iter().find(|(p, _)| *p == c).map(|&(_, b)| b)), "{facing:?}");
                assert_eq!(partners(at, id).len(), 14);
            }
            let (mouth, foot) = all[0];
            let (dx, dz) = facing.step();
            assert_eq!(mouth, (anchor_at.0 + dx, anchor_at.1, anchor_at.2 + dz), "{facing:?}: the mouth is not toward the builder");
            assert_eq!(cells_from_mouth(mouth, facing), all);
            // ...and the two cells a body lies across are a bed's two halves.
            assert_eq!(crate::types::bed_partner(mouth, foot), Some((anchor_at, all[1].1)), "{facing:?}");
            assert!(is_bed_part(foot) && is_bed_part(all[1].1) && is_anchor(all[1].1));
            assert!(all[2..].iter().all(|&(_, id)| !is_bed_part(id) && !crate::types::is_bed(id)), "{facing:?}: a wall is a bed");
        }
    }

    /// Every cell's id is one the rules accept, so the anti-cheat and a
    /// save take it -- and a part number that is no part is refused.
    #[test]
    fn every_part_of_a_lean_to_is_a_known_block_and_no_other_number_is() {
        for facing in FACINGS {
            for (_, id) in cells((0, 0, 0), facing) {
                assert!(crate::types::is_known_block(id), "{id:#x} is not a block");
            }
        }
        assert!(!crate::types::is_known_block(faced(BLOCK_LEAN_TO, Facing::East) | part_bits(15)));
    }

    /// **The hut is walked into, and the hollow in it is not**: every cell
    /// that is thatch has something to stop a body, nothing stands outside
    /// the hut's three by three, and over the middle of the bed there is a
    /// cell of air up to the ridge at the mouth -- the inside a sleeper is
    /// put down in and a player looks into.
    #[test]
    fn a_lean_to_is_solid_where_it_is_thatched_and_open_over_its_bed() {
        for facing in FACINGS {
            for (_, id) in cells((0, 0, 0), facing) {
                let (lo, hi) = extent(id).unwrap_or_else(|| panic!("{facing:?}: part {} collides as nothing", part_of(id)));
                for a in 0..3 {
                    assert!(lo[a] >= -1e-4 && hi[a] <= 1.0 + 1e-4, "{facing:?}: part {} stands out of its cell: {lo:?}..{hi:?}", part_of(id));
                }
            }
            // Points over the middle of the bed, a hand over it, in the mouth
            // and in the anchor.
            for part in [0u8, 8] {
                let id = cell(facing, part);
                for point in [[0.5, 0.2, 0.25], [0.5, 0.2, 0.75], [0.25, 0.2, 0.5], [0.75, 0.2, 0.5]] {
                    let mut inside = false;
                    boxes(id, |lo, hi| inside |= (0..3).all(|a| lo[a] < point[a] && hi[a] > point[a]));
                    assert!(!inside, "{facing:?}: the hollow over the bed of part {part} is solid at {point:?}");
                }
                let mut bed = false;
                boxes(id, |lo, hi| bed |= lo[1] <= 0.0 && (hi[1] - BED_TOP).abs() < 1e-4 && lo[0] < 0.5 && hi[0] > 0.5);
                assert!(bed, "{facing:?}: part {part} has no bed to lie on");
            }
        }
    }

    /// **A hut falls in to half of what it was built of**, whatever the
    /// recipe comes to say: the remains were three sticks and four leaves of
    /// six and eight, and a recipe that grew with the hut and remains that did
    /// not would be a night's camp that got dearer by accident.
    #[test]
    fn a_fallen_lean_to_gives_back_half_of_what_it_took() {
        let recipe = crate::crafting::RECIPES
            .iter()
            .find(|r| r.output.0 == BLOCK_LEAN_TO)
            .expect("a lean-to is made somewhere");
        for &(block, count) in recipe.inputs {
            let back = crate::types::LEAN_TO_REMAINS.iter().find(|&&(b, _)| b == block).map_or(0, |&(_, n)| n);
            assert_eq!(back * 2, count, "{block}: {count} go in and {back} come back");
        }
        assert_eq!(crate::types::LEAN_TO_REMAINS.len(), recipe.inputs.len());
    }

    /// **The hollow at the mouth is lower than a body**, which is what makes
    /// the inside a place to sleep and not a room: a player walking at the
    /// mouth is stopped there, whichever way it faces.
    #[test]
    fn nobody_walks_upright_into_a_lean_to() {
        let tallest_gap = SLICES.iter().map(|s| s.hollow).fold(0.0, f32::max) * T;
        assert!(tallest_gap < crate::geometry::PLAYER_HEIGHT, "the hollow is {tallest_gap} high");
        assert!(tallest_gap > 0.8, "the hollow is too low to lie in: {tallest_gap}");
    }
}
