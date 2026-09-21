//! **Where a palm's trunk is**: the line its bark follows through its cells,
//! and the boxes that line is drawn and walked into as.
//!
//! A palm leans by stepping sideways at one height (`worldgen::palm_cells`):
//! the piece at the top of one straight run and the piece at the bottom of
//! the next stand side by side, because a trunk that climbed across a corner
//! would not be joined face to face and could not be felled as one tree. The
//! mesher reads those steps back and draws one leaning line through them --
//! see [`PalmCourse`] for why -- and that line leaves the cells it was grown
//! in: over the middle of a run it is a third of a cell off the column's
//! middle, and at a step it crosses into the next column half way up.
//!
//! **The report this module answers**: "у пальмы поломаны коллизия она не
//! соответствует модели". The collider still took every piece as its whole
//! cell. A player walked into a metre of air beside a trunk ten sixteenths
//! wide, stood on the empty half of a step, and walked through the part of
//! the bark that leaned out over the sand -- the picture had moved and the
//! box had not.
//!
//! Three ways to make them agree were weighed:
//!
//! * *Draw the trunk back inside its cells* -- a post over the middle of each
//!   piece, as every other bough. The collider would have been right about
//!   the cells and wrong about the post in every one of them, which is the
//!   invisible wall `types::BOUGH_THINNEST` already apologises for on a
//!   straight trunk, and the lean would have been the staircase again.
//! * *Clamp the line* so the post never leaves a cell except into the
//!   neighbour of a step. It stays inside boxes a cell can compute alone, and
//!   the trunk becomes vertical runs with a lurch of two thirds of a cell at
//!   every step -- a Z with its corners filed off.
//! * **One computation for both (chosen).** The course and the slices come
//!   from here; `mesh::palm_trunk_block` draws [`trunk_slices`] and
//!   `geometry::for_each_block_box` collides and aims at the same slices. A
//!   slice can stand out of its own cell by up to `geometry::BOX_OVERHANG`,
//!   and everything that gathers boxes from cells looks that much further --
//!   see the client's `for_each_solid`.

use crate::types::{block_kind, branch_width, is_air, BlockId, BLOCK_AIR, BLOCK_PALM_TRUNK};

/// How many slices of trunk one cell of a palm is drawn and collided in.
///
/// Four, so the trunk moves out a sixteenth or two a slice where its lean is
/// steepest -- a ledge the eye reads as the edge of a curve. Two left steps
/// of three and four sixteenths, which is a staircase again at arm's length;
/// eight doubled the quads of every palm on a coast for a difference nobody
/// could point to. The collider takes the same four, so the ledges a player
/// brushes against are the ones they can see.
pub const PALM_SLICES: usize = 4;

/// **Where the middle of a palm's trunk runs through one of its cells.**
///
/// Drawn as `mesh::branch_block` draws every other piece -- a post, and an
/// arm to each neighbour -- the two steps of a palm were a right-angled Z of
/// bark, and a coast of them a row of staircases with crowns on.
///
/// Three ways to draw a lean were weighed:
///
/// * *Thinner pieces, as a twig is.* A narrower Z is still a Z, and eight
///   sixteenths is the thinnest a trunk goes before it reads as a twig
///   (`types::branch_width`).
/// * *A lean in the block id.* The variant is three bits and a palm's already
///   carries its width; offset bits would have to be taught to felling, to the
///   drop and to every rule that reads a width, for a picture.
/// * **The steps read back (chosen).** Each cell of a palm walks up and down
///   its own straight run -- the mesher's padding ring holds one column on
///   every side, which is as far as a step reaches -- and finds whether the
///   run begins at the root or at a step, and ends at the crown or at a step.
///   The trunk's middle is then a straight line from the middle of one to the
///   middle of the other: over the root's column at the ground, on the face
///   the two pieces of a step share half way up them, and over the top
///   piece's column where the crown sits. Every cell of the palm finds the
///   same line through the same pieces, so the slices meet across cells,
///   across steps and across a chunk border.
///
/// **Of the two pieces of a step, the lower draws the lower half of the cell
/// and the upper the upper.** Each draws what of the line lies in its own
/// column, and the line crosses the shared face half way up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PalmCourse {
    /// The line's lower end: a height above this cell's floor and an offset
    /// from its middle, both in cells.
    pub low: (f32, [f32; 2]),
    /// ...and its upper end.
    pub high: (f32, [f32; 2]),
    /// The part of this cell's height it draws, in cells.
    pub drawn: (f32, f32),
}

impl PalmCourse {
    /// The offset of the line from the cell's middle at a height above its
    /// floor, in cells. Never more than half a cell on either axis: the line
    /// ends on a shared face at most.
    pub fn offset_at(&self, height: f32) -> [f32; 2] {
        let span = self.high.0 - self.low.0;
        if span <= 0.0 {
            return [0.0; 2];
        }
        let t = ((height - self.low.0) / span).clamp(0.0, 1.0);
        [
            self.low.1[0] + (self.high.1[0] - self.low.1[0]) * t,
            self.low.1[1] + (self.high.1[1] - self.low.1[1]) * t,
        ]
    }
}

/// See [`PalmCourse`]. `near` is the block at an offset from the cell being
/// asked about: any height, and at most one column to each side.
pub fn palm_course(near: impl Fn(i32, i32, i32) -> BlockId) -> PalmCourse {
    const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    // Longer than any palm, so the walk ends on the palm's own ends; it is
    // only there so a column of palm pieces somebody stacked cannot make it
    // walk the height of the world for every cell.
    const REACH: i32 = 24;
    let palm = |dx: i32, dy: i32, dz: i32| block_kind(near(dx, dy, dz)) == BLOCK_PALM_TRUNK;
    let mut bottom = 0;
    while bottom > -REACH && palm(0, bottom - 1, 0) {
        bottom -= 1;
    }
    let mut top = 0;
    while top < REACH && palm(0, top + 1, 0) {
        top += 1;
    }
    // A step into this run from below is a piece beside its foot that goes on
    // *down*; a step out of it above is a piece beside its head that goes on
    // *up*. Asking for the continuation is what tells a step from a coconut
    // cluster or a neighbour's frond beside the trunk.
    let below = SIDES.into_iter().find(|&(sx, sz)| palm(sx, bottom, sz) && palm(sx, bottom - 1, sz));
    let above = SIDES.into_iter().find(|&(sx, sz)| palm(sx, top, sz) && palm(sx, top + 1, sz));
    let half = |(sx, sz): (i32, i32)| [sx as f32 * 0.5, sz as f32 * 0.5];
    let low = match below {
        Some(side) => (bottom as f32 + 0.5, half(side)),
        None => (bottom as f32, [0.0; 2]),
    };
    let high = match above {
        Some(side) => (top as f32 + 0.5, half(side)),
        None => (top as f32 + 1.0, [0.0; 2]),
    };
    let drawn = (
        if below.is_some() && bottom == 0 { 0.5 } else { 0.0 },
        if above.is_some() && top == 0 { 0.5 } else { 1.0 },
    );
    PalmCourse { low, high, drawn }
}

/// One slice of a palm's trunk: a short upright box of bark, moved off the
/// middle of its cell along the course.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrunkSlice {
    /// Its floor and its top, as heights above the cell's floor in cells.
    pub bottom: f32,
    pub top: f32,
    /// How far its middle is from the cell's middle, in cells, on x and z.
    pub offset: [f32; 2],
    /// Half its width, in cells.
    pub half_width: f32,
}

impl TrunkSlice {
    /// The box, as (min, max) corners relative to the cell's own corner, in
    /// cells. On x and z it may reach past 0 or 1 -- by at most
    /// `geometry::BOX_OVERHANG` -- and on y it never does.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [ox, oz] = self.offset;
        let hw = self.half_width;
        (
            [0.5 + ox - hw, self.bottom, 0.5 + oz - hw],
            [0.5 + ox + hw, self.top, 0.5 + oz + hw],
        )
    }
}

/// The slices a piece of palm is drawn and collided as, bottom to top.
///
/// **Each slice sits where the course is at its own middle height**, which is
/// what the mesher drew before any of this was shared, and the collider has
/// the same four rather than one box round them: a box round a cell's slices
/// is up to a fifth of a cell wider than the bark where the lean is steepest,
/// and that fifth is the invisible wall this module exists to take away.
///
/// A piece with nothing of its palm round it -- `near` answering air -- is an
/// upright post the width of its variant, over the middle of its cell.
pub fn trunk_slices(block: BlockId, near: impl Fn(i32, i32, i32) -> BlockId) -> impl Iterator<Item = TrunkSlice> {
    let course = palm_course(near);
    let half_width = branch_width(block).unwrap_or(8) as f32 / 32.0;
    (0..PALM_SLICES).filter_map(move |slice| {
        let bottom = (slice as f32 / PALM_SLICES as f32).max(course.drawn.0);
        let top = ((slice + 1) as f32 / PALM_SLICES as f32).min(course.drawn.1);
        if top <= bottom + 1e-4 {
            return None;
        }
        Some(TrunkSlice { bottom, top, offset: course.offset_at((bottom + top) * 0.5), half_width })
    })
}

/// **The slices of the piece of trunk beside a cell, as that cell can see
/// them.** `away` is the cell's side away from the trunk, so the trunk is at
/// `(-away.0, 0, -away.1)`; `near` is only ever asked one column round the
/// asking cell, which is what the mesher's padding holds.
///
/// A bunch of coconuts hangs against the top piece of its palm and has to put
/// its nuts where that piece's bark is drawn (`mesh::bunch_nuts`) -- over a
/// leaning trunk that is up to a fifth of a cell off the middle, and against
/// the middle the nuts hung in the air beside the bark. [`trunk_slices`]
/// reads the trunk's own ring, and one column of it, the far side, is two
/// columns from the bunch: past the padding at a chunk's edge. Three ways
/// were weighed:
///
/// * *Ask the far side only when it is inside the chunk.* The nuts would
///   move by a sixteenth or two across every chunk border -- a seam a player
///   finds on the one coast that has them.
/// * *Put the bunch where the trunk is always seen*, by the generator. The
///   worlds already grown keep their bunches where they are.
/// * **Infer the far column (chosen)** and hand [`trunk_slices`] everything
///   else as it is. That column only matters when it holds the step the
///   trunk's run starts from, and a run that starts at a step has open air
///   under its foot -- the step's lower piece stands beside it, not under it
///   -- where the run a palm is rooted by stands on the sand. A foot over air
///   with no step on any side the cell can see is a step on the side it
///   cannot. `a_bunch_finds_the_slices_its_trunk_is_drawn_as` holds that to
///   every palm the generator grows.
pub fn slices_beside((ax, az): (i32, i32), near: impl Fn(i32, i32, i32) -> BlockId) -> impl Iterator<Item = TrunkSlice> {
    const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    const REACH: i32 = 24;
    // The trunk from the cell, and -- the same step again -- the far column
    // from the trunk.
    let (tx, tz) = (-ax, -az);
    let palm = |dx: i32, dy: i32, dz: i32| block_kind(near(dx, dy, dz)) == BLOCK_PALM_TRUNK;
    let mut foot = 0;
    while foot > -REACH && palm(tx, foot - 1, tz) {
        foot -= 1;
    }
    let seen = SIDES
        .into_iter()
        .filter(|&side| side != (tx, tz))
        .any(|(sx, sz)| palm(tx + sx, foot, tz + sz) && palm(tx + sx, foot - 1, tz + sz));
    let step_out_of_sight = !seen && is_air(near(tx, foot - 1, tz));
    let block = near(tx, 0, tz);
    trunk_slices(block, move |dx, dy, dz| {
        if (dx, dz) == (tx, tz) {
            return if step_out_of_sight && (dy == foot || dy == foot - 1) { BLOCK_PALM_TRUNK } else { BLOCK_AIR };
        }
        let (cx, cz) = (tx + dx, tz + dz);
        if cx.abs() > 1 || cz.abs() > 1 {
            return BLOCK_AIR;
        }
        near(cx, dy, cz)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BLOCK_AIR;

    #[test]
    fn a_lone_piece_of_palm_is_an_upright_post_over_the_middle_of_its_cell() {
        // What `geometry::block_box` answers when it cannot see the world:
        // a piece with no palm round it has no lean to follow.
        let piece = crate::worldgen::palm_cells(0, (1, 0))[0].1;
        let slices: Vec<TrunkSlice> = trunk_slices(piece, |_, _, _| BLOCK_AIR).collect();
        assert_eq!(slices.len(), PALM_SLICES);
        assert_eq!(slices[0].bottom, 0.0);
        assert_eq!(slices[PALM_SLICES - 1].top, 1.0);
        for slice in slices {
            assert_eq!(slice.offset, [0.0, 0.0], "a lone piece leans");
            let (min, max) = slice.bounds();
            assert!(min[0] > 0.0 && max[0] < 1.0 && min[2] > 0.0 && max[2] < 1.0, "a lone post fills its cell");
        }
    }

    #[test]
    fn a_bunch_finds_the_slices_its_trunk_is_drawn_as() {
        // A bunch of coconuts sees one column round itself -- asked further,
        // this panics -- and has to find the very slices its trunk piece is
        // drawn and collided as. Every height, both bends, every count and
        // first side of bunches, leaning each way, on the ground; and a
        // straight trunk, whose foot on the sand must not read as a step.
        use crate::types::{BLOCK_PALM_COCONUTS, BLOCK_SAND};
        const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let straight = crate::worldgen::palm_cells(0, (1, 0))[0].1;
        type Cells = Vec<((i32, i32, i32), BlockId)>;
        let mut worlds: Vec<(String, Cells)> = Vec::new();
        for height in 0u32..4 {
            for bend in 0u32..2 {
                for bunches in 0u32..16 {
                    let variant = height | bend << 4 | bunches << 10;
                    for lean in SIDES {
                        worlds.push((format!("palm {variant:#x} leaning {lean:?}"), crate::worldgen::palm_cells(variant, lean)));
                    }
                }
            }
        }
        let mut upright: Vec<((i32, i32, i32), BlockId)> = (1..=6).map(|y| ((0, y, 0), straight)).collect();
        upright.extend(SIDES.map(|(sx, sz)| ((sx, 6, sz), BLOCK_PALM_COCONUTS)));
        worlds.push(("a straight trunk".to_string(), upright));
        let mut away_from_the_lean = 0;
        for (name, cells) in &worlds {
            let at = |x: i32, y: i32, z: i32| {
                if y <= 0 {
                    return BLOCK_SAND;
                }
                cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)
            };
            for &((x, y, z), id) in cells {
                if id != BLOCK_PALM_COCONUTS {
                    continue;
                }
                let away = SIDES
                    .into_iter()
                    .find(|&(sx, sz)| block_kind(at(x - sx, y, z - sz)) == BLOCK_PALM_TRUNK)
                    .expect("a bunch beside no trunk");
                let seen: Vec<TrunkSlice> = slices_beside(away, |dx, dy, dz| {
                    assert!(dx.abs() <= 1 && dz.abs() <= 1, "{name}: a bunch asked {:?}, past the padding", (dx, dz));
                    at(x + dx, y + dy, z + dz)
                })
                .collect();
                let (tx, tz) = (x - away.0, z - away.1);
                let drawn: Vec<TrunkSlice> = trunk_slices(at(tx, y, tz), |dx, dy, dz| at(tx + dx, y + dy, tz + dz)).collect();
                assert_eq!(seen, drawn, "{name}: the bunch at {:?} sees its trunk's bark somewhere it is not drawn", (x, y, z));
                let towards_the_bunch = drawn[0].offset[0] * away.0 as f32 + drawn[0].offset[1] * away.1 as f32;
                away_from_the_lean += usize::from(towards_the_bunch < 0.0);
            }
        }
        assert!(away_from_the_lean > 0, "no bunch hung where its step is out of sight, and the inference went untested");
    }

    #[test]
    fn no_slice_of_any_palm_stands_further_out_of_its_cell_than_the_overhang() {
        // The number every gatherer of boxes widens its search by
        // (`geometry::BOX_OVERHANG`) has to be the most a slice can reach,
        // or a player standing just past it walks into bark nobody looked for.
        let mut reach = 0.0f32;
        for variant in 0u32..64 {
            for lean in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let cells = crate::worldgen::palm_cells(variant, lean);
                let at = |x: i32, y: i32, z: i32| {
                    cells.iter().find(|(c, _)| *c == (x, y, z)).map_or(BLOCK_AIR, |(_, id)| *id)
                };
                for &((x, y, z), id) in &cells {
                    if block_kind(id) != BLOCK_PALM_TRUNK {
                        continue;
                    }
                    for slice in trunk_slices(id, |dx, dy, dz| at(x + dx, y + dy, z + dz)) {
                        let (min, max) = slice.bounds();
                        assert!(min[1] >= 0.0 && max[1] <= 1.0, "a slice left its cell's height: {slice:?}");
                        for axis in [0, 2] {
                            reach = reach.max(-min[axis]).max(max[axis] - 1.0);
                        }
                    }
                }
            }
        }
        assert!(reach > 0.0, "no palm leans out of a cell, and the overhang is for nothing");
        assert!(
            reach <= crate::geometry::BOX_OVERHANG,
            "a slice stands {reach} out of its cell, past the overhang of {}",
            crate::geometry::BOX_OVERHANG
        );
    }
}
