//! Branching trees: wood of more than one thickness, and the saplings
//! that grow on the same logic.
//!
//! Every ordinary world grows its broadleaf trees, birches, acacias and
//! baobabs this way (`Preset::grows_branches`). They began as an experiment
//! behind a world type of their own and became the world when the player
//! asked for it. Firs are still columns of log cubes, and the test field
//! builds its own.
//!
//! **The birch came last, and in its own bark.** "у березы нету веток": a
//! birch wood was the one broadleaf wood still made of log cubes, standing
//! beside oaks of limbs and twigs. It is a slender stem of pieces now, with
//! short upturned limbs and leaves hanging off their ends (`birch_tree_cells`),
//! and its pieces are white: the width steps above the oak's carry the bark
//! (`types::birch_branch`), so it fells as birch timber and a sapling of it
//! grows into a birch.
//!
//! ## What a branching tree is
//!
//! A trunk that is thick at the foot and thinner up the stem, three or four
//! limbs off its upper half that thin again as they reach out and turn up
//! at the tip, a leader twig out of the top, and a clump of leaves over the
//! crown and over every tip. The pieces are `types::BLOCK_BOUGH` (eight
//! sixteenths and up: timber, an axe, a log) and `types::BLOCK_TWIG` (under
//! eight: a stick, by hand), each carrying its width; the mesher draws each
//! as a post and joins it to the pieces beside it (`mesh::branch_block`).
//!
//! The crown is clumps rather than one ball **because the branches are the
//! point**. The ordinary canopy is five rows of leaves round the top of the
//! trunk, and a trunk of posts inside it would be a tree nobody ever sees
//! the inside of. Clumps over the tips leave gaps a player looks through at
//! the limbs holding them up.
//!
//! ## A tree, not a lattice
//!
//! **No piece may touch wood other than the piece it grows from.** The
//! mesher joins every piece to every piece beside it -- it cannot know
//! which of two neighbours is the parent -- so a limb's upturned tip one
//! column from the trunk is joined to the trunk as well, and the tree grows
//! a closed square of bark in the air. `Grower::grow` refuses such a piece
//! and the limb stops short; `a_branch_tree_is_a_tree_and_not_a_lattice`
//! counts the joins.
//!
//! ## Chunk borders
//!
//! Everything here is decided from the root's hash, the column cache and
//! offsets from the root, never from what is already in the block array --
//! the contract `WorldGen::place_trees` describes. A branching tree reaches
//! `MAX_CANOPY_RADIUS` like the broadleaf it replaces, so the pass that
//! roots it needed no wider border; a sapling reaches one column.
//!
//! ## What was not done in this first version
//!
//! Firs, birches and acacias keep their log trunks; so do old trees and the
//! dead wood, whose snags have no crown for branches to hold. A tree felled
//! in one of these worlds comes down by `felling`'s support rule for
//! branches rather than by the trunk walk.

use std::collections::HashSet;

use super::{
    acacia_lean, hash2, put_block, Biome, Column, ColumnCache, WorldGen, MAX_CANOPY_RADIUS,
};
use crate::types::{
    block_kind, branch, in_bark_of, is_branch, is_leafy, BlockId, Chunk, BIRCH_WIDEST, BLOCK_ACACIA_LEAVES,
    BLOCK_AIR, BLOCK_BIRCH_LOG, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG, CHUNK_SIZE_X,
    CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

/// How many blocks of trunk a branching broadleaf carries over the biome's
/// own range.
///
/// **Taller, because a tree of posts is a tree you look up into.** The
/// first version grew the ordinary trunk, and an oak's four to six blocks
/// with the limbs off the top three of them put the crown at a player's
/// eyes: the branches the whole preset exists to show were behind the
/// leaves they held, and the player asked for the trees and the limbs on
/// them to be higher. Three more puts clear trunk between the ground and
/// the lowest limb. Not wider as well -- the crown's reach is the border
/// `place_trees` walks, see `a_branch_tree_stays_inside_the_border_the_tree_pass_walks`.
pub(super) const BRANCH_TALLER: i32 = 3;

/// How far a branching broadleaf reaches from its root, on either axis:
/// limbs three columns out and the leaves round their ends two more.
///
/// **Five, and it was three.** Three -- the ordinary canopy's radius -- left
/// a limb two columns long, which is a stub with a ball on it: there is no
/// room in two columns for a limb to rise, fork and thin, and a crown built
/// in that space is a cube of leaves with posts under it, which is what the
/// player called unrealistic. Five is as far as an old tree or an acacia
/// already reaches, so `place_trees` walks that border already and the
/// column cache is already sized for it (`super::FEATURE_MARGIN`); only the
/// broadleaf of a branch world is asked to use it.
pub const BRANCH_REACH: i32 = 5;
const _: () = assert!(BRANCH_REACH <= super::OLD_TREE_REACH);

/// How far out along its heading a limb's last piece reaches.
const LIMB_OUT: i32 = 3;

/// How near another tree's root can stand to a piece of a limb and the
/// piece still grow there, on either axis.
///
/// **One more than a limb reaches, which is the ladder.** The mesher joins
/// any two pieces that share a face, so two trees whose limbs pointed at
/// each other across a gap met in the middle and were drawn joined -- a bar
/// of bark hung between two crowns, the "ladders" the third set of pictures
/// found in a close wood. A piece that would stand within this of a
/// neighbour's root is not grown, and the limb stops short. Decided from
/// the root's hash rather than from the blocks either tree has written, so
/// both chunks of a seam decide it the same way.
const NEIGHBOUR_REACH: i32 = LIMB_OUT + 1;

/// A cell of a tree as an offset from the ground under its root --
/// `(0, 1, 0)` is the foot of the trunk -- and what stands in it.
pub type Cell = ((i32, i32, i32), BlockId);

/// The four ways a limb can reach, each a quarter turn anticlockwise from
/// the one before -- which is what makes `(-dz, dx)` the next one along.
const AROUND: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// How far outside the chunk a sapling may be rooted and still reach into
/// it: the stem, and a side shoot or a leaf one column off it.
const SAPLING_REACH: i32 = 1;

/// How many columns round a sapling must hold no grown tree's root.
///
/// **A sapling grows where the grown trees leave light**, and that is not
/// only a story: a limb reaches `LIMB_OUT` columns from its trunk, and a
/// stem one column from a limb's tip would be joined to it by the mesher --
/// a bar of bark from a young tree into an old one -- or one column more,
/// by a side shoot. Two past the limb is the nearest a root can stand with
/// no piece of its tree beside anything the sapling grows.
const SAPLING_CLEARANCE: i32 = LIMB_OUT + 2;

/// ...and round a grown birch: two past a birch's limb, which is one column
/// shorter than an oak's.
///
/// **Its own, because a birch wood had no saplings in it.** Asked with the
/// oak's five, a sapling needed an eleven-by-eleven square with no grown tree
/// in it, and a birch wood holds about four trees in that square: one sapling
/// in fifty was let stand, and eight chunks of birch wood came up with none.
/// A birch's limbs cannot reach a stem five columns off, so the oak's clearance
/// was refusing room a birch never takes.
const BIRCH_SAPLING_CLEARANCE: i32 = BIRCH_LIMB_OUT + 2;

/// ...and round another sapling: two, so neither's side shoot touches the
/// other's stem.
const SAPLING_SPACING_CLEARANCE: i32 = 2;

/// How wide the trunk is at `level` (1 is the piece on the ground) of a
/// trunk `height` tall, in sixteenths.
///
/// A straight taper from the foot to six at the top, so the top of every
/// trunk is a twig a player can reach and pull from. A foot of fourteen for
/// the tall ones (a maple's trunk is two longer than an oak's) and twelve
/// for the rest: a full sixteen reads as the log cube this replaces, which
/// is the thing a player is meant to notice is gone.
pub(super) fn trunk_width(level: i32, height: i32) -> u8 {
    const TOP: i32 = 6;
    let foot = if height >= 7 { 14 } else { 12 };
    let span = (height - 1).max(1);
    (foot - (foot - TOP) * (level - 1).clamp(0, span) / span) as u8
}

/// A tree's wood as it is grown: every piece remembers nothing but where
/// it is, and a new one is refused if it would touch any piece but its
/// parent. See the module note, "A tree, not a lattice".
#[derive(Default)]
struct Grower {
    wood: Vec<Cell>,
    taken: HashSet<(i32, i32, i32)>,
}

impl Grower {
    /// Adds a piece `width16` across at `at`, grown from `parent`. False,
    /// and nothing added, if the cell is taken or touches other wood.
    fn grow(&mut self, at: (i32, i32, i32), parent: Option<(i32, i32, i32)>, width16: u8) -> bool {
        if self.taken.contains(&at) {
            return false;
        }
        let (x, y, z) = at;
        let touches = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
            .iter()
            .map(|&(dx, dy, dz)| (x + dx, y + dy, z + dz))
            .any(|near| Some(near) != parent && self.taken.contains(&near));
        if touches {
            return false;
        }
        self.taken.insert(at);
        self.wood.push((at, branch(width16)));
        true
    }
}

/// A mass of leaves round `centre`: an ellipsoid `across` wide each way from
/// the middle and `up` high, with a ragged skin and a few holes in it.
///
/// **An ellipsoid with the rim torn, not a rounded square.** The clumps of
/// the first version were a square slab of three rows with the corners off,
/// and a crown of them read as cubes of leaves parked on the ends of posts.
/// Leaves in a real crown gather into flattened masses round the ends of the
/// limbs -- wider than they are tall -- with daylight through the edge and a
/// gap here and there inside. One in three of the outer half goes missing and
/// one in eleven of the inner, hashed from the offset and `salt` so both
/// chunks a mass straddles leave out the same leaves.
fn blob(out: &mut Vec<Cell>, (cx, cy, cz): (i32, i32, i32), across: i32, up: i32, salt: u32, leaves: BlockId) {
    let (a, u) = (across.max(1), up.max(1));
    let limit = a * a * u * u;
    for dy in -u..=u {
        for dz in -a..=a {
            for dx in -a..=a {
                let reach = (dx * dx + dz * dz) * u * u + dy * dy * a * a;
                // `+ a * u` rounds the surface out, as the acacia's plate
                // does: without it every mass has a single leaf standing
                // proud at each compass point.
                if reach > limit + a * u {
                    continue;
                }
                let roll = hash2(dx * 7 + dy * 31, dz * 13 - dy * 3, salt);
                let outer = reach * 2 > limit;
                if (outer && roll.is_multiple_of(3)) || (!outer && roll.is_multiple_of(11)) {
                    continue;
                }
                out.push(((cx + dx, cy + dy, cz + dz), leaves));
            }
        }
    }
}

/// Every cell of a branching tree, leaves first and wood after -- the order
/// they are written in, so a piece of wood replaces a leaf a clump put in
/// its way, the rule `place_tree` keeps.
///
/// ## The shape
///
/// * **A trunk** tapering from twelve or fourteen sixteenths to six, and on
///   one tree in three **forked** three below its top: two leaders stepping
///   out a column either side and up, each with its own crown.
/// * **Four to seven limbs** in a spiral a quarter turn apart, spread over
///   the trunk from under half way up to the fork or the top -- not a whorl
///   off the last three blocks. Each goes out two columns, rises one, goes
///   on a third column (straight on, or turning aside), and rises to a tip:
///   a staircase, which is the nearest a grid of posts comes to a limb
///   growing out *and up*. It thins as it goes, and a side shoot leaves it
///   near the trunk with a small mass of its own.
/// * **Leaves in masses** (`blob`): a flattened one round every tip and a
///   deeper one over each leader, overlapping into a crown with gaps in it.
///
/// Stays inside `BRANCH_REACH` of the root on both axes: a limb's tip is
/// `LIMB_OUT` columns out and its mass two more. See
/// `a_branch_tree_stays_inside_the_border_the_tree_pass_walks`.
///
/// `rise(dx, dz)` is how far the ground under a column stands above the
/// ground under the root. **A limb only grows where there is air under
/// it.** The first pictures of a wood on rolling ground had limbs lying on
/// the grass of the rise beside their tree -- joined to that grass by the
/// mesher as though it were a foot -- and, where the rise was higher, a
/// limb written straight into the hillside, because wood overwrites what
/// it lands in. Asked of the column cache, which both chunks of a border
/// hold the same copy of; see `Grower` for why the limb then stops short
/// rather than bending round.
///
/// `neighbour(dx, dz)` is whether another grown tree is rooted at that
/// offset; see `NEIGHBOUR_REACH`.
pub(super) fn branch_tree_cells(
    height: i32,
    canopy: i32,
    variant: u32,
    leaves: BlockId,
    rise: impl Fn(i32, i32) -> i32,
    neighbour: impl Fn(i32, i32) -> bool,
) -> Vec<Cell> {
    let height = height + BRANCH_TALLER;
    let mut crown: Vec<Cell> = Vec::new();
    let mut tree = Grower::default();
    let over_air = |(dx, dy, dz): (i32, i32, i32)| dy >= rise(dx, dz) + 2;
    let crowded = |(x, _, z): (i32, i32, i32)| {
        (-NEIGHBOUR_REACH..=NEIGHBOUR_REACH).any(|oz| {
            (-NEIGHBOUR_REACH..=NEIGHBOUR_REACH)
                .any(|ox| (x + ox, z + oz) != (0, 0) && neighbour(x + ox, z + oz))
        })
    };

    // Everything off the trunk has to have air under it and room from the
    // trees round it; the trunk itself stands on the root's own ground.
    let may_grow = |at: (i32, i32, i32)| over_air(at) && !crowded(at);

    // The trunk, to the fork or to the top.
    let fork = (variant.is_multiple_of(3) && height >= 8).then_some(height - 3);
    let trunk_top = fork.unwrap_or(height);
    for level in 1..=trunk_top {
        let parent = (level > 1).then_some((0, level - 1, 0));
        tree.grow((0, level, 0), parent, trunk_width(level, height));
    }
    // The leaders: one twig on up out of the top, or two out of the fork.
    let mut leaders: Vec<(i32, i32, i32)> = Vec::new();
    match fork {
        None => {
            if tree.grow((0, height + 1, 0), Some((0, height, 0)), 4) {
                leaders.push((0, height + 1, 0));
            }
        }
        Some(at) => {
            let (ax, az) = AROUND[(variant >> 11) as usize % 4];
            for (lx, lz) in [(ax, az), (-ax, -az)] {
                let side = (lx, at, lz);
                if !may_grow(side) || !tree.grow(side, Some((0, at, 0)), trunk_width(at + 1, height)) {
                    continue;
                }
                let mut last = side;
                for level in at + 1..=height + 1 {
                    let up = (lx, level, lz);
                    let width = if level > height { 4 } else { trunk_width(level, height) };
                    if !tree.grow(up, Some(last), width) {
                        break;
                    }
                    last = up;
                }
                leaders.push(last);
            }
        }
    }
    // A deep mass over each leader: the biome's canopy says how wide.
    let across = if canopy >= MAX_CANOPY_RADIUS { 3 } else { 2 };
    for (n, &(lx, ly, lz)) in leaders.iter().enumerate() {
        blob(&mut crown, (lx, ly, lz), across, 2, variant ^ (0x51 * (n as u32 + 1)), leaves);
    }

    // The limbs, in a spiral up the trunk.
    let limbs = 4 + ((variant >> 3) & 3) as usize;
    let first = (variant >> 5) as usize;
    // Nine twentieths up, rounded up: rounded down, an eight-block trunk
    // put its lowest limb at three, under two fifths of the stem.
    let lowest = ((height * 9 + 19) / 20).max(3);
    let highest = (trunk_top - 1).max(lowest);
    for i in 0..limbs {
        let (dx, dz) = AROUND[(first + i) % 4];
        // Which way a limb turns aside, and a side shoot the other way.
        let turn = if (variant >> (8 + i)) & 1 == 1 { 1 } else { -1 };
        let (px, pz) = (-dz * turn, dx * turn);
        let level = lowest + (i as i32 * (highest - lowest + 1)) / limbs as i32;
        // Four narrower than the trunk it leaves, and between four and
        // eight: a limb out of a twelve-wide foot is a bough you need an axe
        // for, one out of the thin top is a twig.
        let out = (trunk_width(level, height) as i32 - 4).clamp(4, 8) as u8;
        let middle = (out - 2).max(4);
        // **Never thinner than four sixteenths.** Two was the tip the first
        // versions ended on, and from twenty blocks a two-sixteenths post is
        // under a pixel: the leaves on it hung in the air.
        let tip_width = 4;
        // Hashed rather than read off a bit: the variant's low bits are
        // already spent on the count, the spiral and the turns, and a bit
        // past the thirteenth is one no small variant has -- which is how
        // the border test found every limb in its sample turning aside.
        let straight_on = hash2(i as i32, 0, variant ^ 0x57A1).is_multiple_of(2);
        let third = if straight_on { (LIMB_OUT * dx, level + 1, LIMB_OUT * dz) } else { (2 * dx + px, level + 1, 2 * dz + pz) };
        let path = [
            ((dx, level, dz), out),
            ((2 * dx, level, 2 * dz), middle),
            ((2 * dx, level + 1, 2 * dz), middle),
            (third, tip_width),
            ((third.0, level + 2, third.2), tip_width),
        ];
        let mut last = (0, level, 0);
        for &(at, width) in &path {
            if !may_grow(at) || !tree.grow(at, Some(last), width) {
                break;
            }
            last = at;
        }
        if last.0 == 0 && last.2 == 0 {
            continue;
        }
        blob(&mut crown, last, 2, 1, variant ^ (0x9E37 * (i as u32 + 1)), leaves);
        // A side shoot off the first piece, the other way from the turn:
        // one out and one up, never beside the trunk or the limb it leaves.
        let base = (dx, level, dz);
        if last != base && tree.taken.contains(&base) {
            let shoot = (dx - px, level, dz - pz);
            let rise = (shoot.0, level + 1, shoot.2);
            if may_grow(shoot) && tree.grow(shoot, Some(base), tip_width) {
                let end = if may_grow(rise) && tree.grow(rise, Some(shoot), tip_width) { rise } else { shoot };
                blob(&mut crown, end, 1, 1, variant ^ (0x7C1 * (i as u32 + 1)), leaves);
            }
        }
    }
    crown.extend(tree.wood);
    crown
}

/// How many blocks of trunk a birch of pieces carries over the birch wood's
/// own range (`Biome::tree_shape`).
///
/// **Two, where an oak takes `BRANCH_TALLER`'s three.** A birch is already the
/// tallest broadleaf in the world and a mast is what it is; what the pieces
/// buy it is limbs, and limbs start two fifths up a stem, so two more blocks
/// keep its lowest leaves clear of a walking player's head.
pub(super) const BIRCH_TALLER: i32 = 2;

/// How far a birch of pieces reaches from its root, on either axis: a limb
/// `BIRCH_LIMB_OUT` columns out and the leaves hanging one past it.
///
/// **Inside the birch's own border and not the oak's**: `place_trees` walks
/// `MAX_CANOPY_RADIUS` round a birch root, and a birch that spread like an oak
/// would be an oak in white bark -- the confusion `place_birch` was written
/// to avoid.
pub const BIRCH_REACH: i32 = 3;
const _: () = assert!(BIRCH_REACH <= super::MAX_CANOPY_RADIUS);

/// How far out a birch's limb goes before it turns up.
const BIRCH_LIMB_OUT: i32 = 2;

/// How wide a birch's trunk is at `level` of a trunk `height` tall: twelve
/// at the foot, which is `GROWN_FOOT` and the widest a birch's bark comes in
/// (`BIRCH_WIDEST`), down to four at the top.
fn birch_trunk_width(level: i32, height: i32) -> u8 {
    let foot = BIRCH_WIDEST as i32;
    let span = (height - 1).max(1);
    (foot - (foot - 4) * (level - 1).clamp(0, span) / span) as u8
}

/// Every cell of a birch built of pieces, leaves first and wood after --
/// in the oak's bark, which `in_bark` turns white.
///
/// ## The shape
///
/// * **A slender stem**, twelve sixteenths at the foot to a four-wide leader
///   out of the top, with a narrow deep mass of leaves round its tip.
/// * **Five to eight short limbs** in a spiral from two fifths of the way up
///   to two under the top: one column out, one more, and up -- a birch's
///   branches rise from the stem rather than spreading from it.
/// * **Leaves hanging off each end**, a mass one across and two deep centred
///   under the tip: the weeping twigs a birch is recognised by, drawn in the
///   only way a grid of cells has of drooping.
///
/// Rejected: *`branch_tree_cells` with a narrower crown.* An oak's limbs go
/// out three columns and fork; cut to a birch's reach they are stubs with
/// balls on, which is the cube-on-a-post the oak's own reach was widened to
/// get rid of. A birch is a different silhouette -- a mast with its bark
/// showing between small hanging masses -- and the shape has to be its own.
///
/// `rise` and `neighbour` as for `branch_tree_cells`: a limb only over air,
/// and never within a limb's reach of another tree's root.
pub(super) fn birch_tree_cells(
    height: i32,
    variant: u32,
    leaves: BlockId,
    rise: impl Fn(i32, i32) -> i32,
    neighbour: impl Fn(i32, i32) -> bool,
) -> Vec<Cell> {
    let height = height + BIRCH_TALLER;
    let mut crown: Vec<Cell> = Vec::new();
    let mut tree = Grower::default();
    let over_air = |(dx, dy, dz): (i32, i32, i32)| dy >= rise(dx, dz) + 2;
    // One past a limb, for `NEIGHBOUR_REACH`'s reason: two birches whose limbs
    // pointed at each other would meet in a bar of bark.
    let near = BIRCH_LIMB_OUT + 1;
    let crowded = |(x, _, z): (i32, i32, i32)| {
        (-near..=near).any(|oz| (-near..=near).any(|ox| (x + ox, z + oz) != (0, 0) && neighbour(x + ox, z + oz)))
    };
    let may_grow = |at: (i32, i32, i32)| over_air(at) && !crowded(at);

    for level in 1..=height {
        let parent = (level > 1).then_some((0, level - 1, 0));
        tree.grow((0, level, 0), parent, birch_trunk_width(level, height));
    }
    tree.grow((0, height + 1, 0), Some((0, height, 0)), 4);
    blob(&mut crown, (0, height, 0), 1, 2, variant ^ 0xB1C4, leaves);

    let limbs = 5 + ((variant >> 3) & 3) as usize;
    let first = (variant >> 5) as usize;
    // Five at the lowest: the mass under a limb's tip hangs two cells, and a
    // limb at five leaves it at three, over a standing player's head.
    let lowest = ((height * 2 + 4) / 5).max(5);
    let highest = (height - 2).max(lowest);
    for i in 0..limbs {
        let (dx, dz) = AROUND[(first + i) % 4];
        let level = lowest + (i as i32 * (highest - lowest + 1)) / limbs as i32;
        let out = (birch_trunk_width(level, height) as i32 - 4).clamp(4, 6) as u8;
        let path = [
            ((dx, level, dz), out),
            ((BIRCH_LIMB_OUT * dx, level, BIRCH_LIMB_OUT * dz), 4),
            ((BIRCH_LIMB_OUT * dx, level + 1, BIRCH_LIMB_OUT * dz), 4),
        ];
        let mut last = (0, level, 0);
        for &(at, width) in &path {
            if !may_grow(at) || !tree.grow(at, Some(last), width) {
                break;
            }
            last = at;
        }
        if (last.0, last.2) == (0, 0) {
            continue;
        }
        blob(&mut crown, (last.0, last.1 - 1, last.2), 1, 2, variant ^ (0xB12C * (i as u32 + 1)), leaves);
    }
    crown.extend(tree.wood);
    crown
}

/// The timber a tree in these leaves is: a birch's for birch leaves, an oak's
/// for every other leaf -- the apple, the maple, the acacia and the oak are
/// all an oak's timber (`Biome::tree_wood`).
///
/// **The leaf decides, because the leaf is what the server can see.** A young
/// tree is grown from nothing but the blocks round its root
/// (`tree_stage_cells`), and the one thing that says which wood it is, before
/// its stem is anything but twigs, is the tuft on top.
pub fn bark_for(leaves: BlockId) -> BlockId {
    // A wood's own crown says its wood (`wood::WOODS`) -- a birch's, a
    // willow's; every other leaf is on an oak's timber.
    crate::wood::WOODS.iter().find(|w| w.leaves == block_kind(leaves)).map_or(BLOCK_LOG, |w| w.log)
}

/// A tree's cells with every piece in the bark of `log`. See
/// `types::in_bark_of`.
pub(super) fn in_bark(cells: Vec<Cell>, log: BlockId) -> Vec<Cell> {
    if block_kind(log) == BLOCK_LOG {
        return cells;
    }
    cells.into_iter().map(|(at, id)| (at, in_bark_of(id, log))).collect()
}

/// Writes a birch built of pieces into a chunk. See `birch_tree_cells`.
#[allow(clippy::too_many_arguments)] // a shape, a place, its ground and its wood
pub(super) fn place_birch_tree(
    blocks: &mut [BlockId],
    columns: &ColumnCache,
    lx: i32,
    ground: i32,
    lz: i32,
    height: i32,
    variant: u32,
    (log, leaves): (BlockId, BlockId),
    neighbour: impl Fn(i32, i32) -> bool,
) {
    // The leader and the mass round it reach three over the trunk.
    if ground + height + BIRCH_TALLER + 3 >= CHUNK_SIZE_Y as i32 {
        return;
    }
    let rise = |dx: i32, dz: i32| columns.at(lx + dx, lz + dz).height - ground;
    for ((dx, dy, dz), id) in in_bark(birch_tree_cells(height, variant, leaves, rise, neighbour), log) {
        put_block(blocks, lx + dx, ground + dy, lz + dz, id, is_branch(id));
    }
}

/// Every cell of a sapling, leaves first and wood after.
///
/// **Two to four twigs of stem and a tuft of leaves on the end.** Thin
/// enough everywhere to take by hand -- six sixteenths at the foot of the
/// tallest -- which is the whole reason a sapling is in the world: sticks
/// in the first minute, from something that grows in the wood, before
/// there is an axe to take a limb.
///
/// Reaches one column from its root: a side shoot on the taller ones and
/// the leaves round the top. See `SAPLING_REACH`. The side shoot keeps air
/// under it, for the limb's reason -- see `branch_tree_cells`.
pub(super) fn sapling_cells(variant: u32, leaves: BlockId, rise: impl Fn(i32, i32) -> i32) -> Vec<Cell> {
    const STEMS: [&[u8]; 3] = [&[4, 2], &[4, 4, 2], &[6, 4, 4, 2]];
    let stem = STEMS[(variant % 3) as usize];
    let height = stem.len() as i32;
    let mut crown: Vec<Cell> = Vec::new();
    let mut tree = Grower::default();
    for (i, &width) in stem.iter().enumerate() {
        let level = i as i32 + 1;
        tree.grow((0, level, 0), (level > 1).then_some((0, level - 1, 0)), width);
    }
    // A leaf over the top, and the ring round the last twig with one in
    // three gone -- ragged, and never so full it reads as a bush.
    crown.push(((0, height + 1, 0), leaves));
    for (n, &(dx, dz)) in AROUND.iter().enumerate() {
        if hash2(n as i32, height, variant ^ 0x5A91).is_multiple_of(3) {
            continue;
        }
        crown.push(((dx, height, dz), leaves));
    }
    // A side shoot on half of the taller ones, with a leaf beside its end.
    if height >= 3 && (variant >> 4) & 1 == 1 {
        let (dx, dz) = AROUND[(variant >> 6) as usize % 4];
        let level = height - 1;
        if level >= rise(dx, dz) + 2 && tree.grow((dx, level, dz), Some((0, level, 0)), 2) {
            crown.push(((dx - dz, level, dz + dx), leaves));
        }
    }
    crown.extend(tree.wood);
    crown
}

/// How wide a tree's foot is once it has finished growing, in sixteenths.
///
/// **The one thing that tells a young tree from a grown one on the blocks
/// alone.** Every grown tree this generator plants stands on a foot of
/// twelve or more (`trunk_width`, the acacia's and the baobab's too), and
/// every stage of a young one on less -- so the server can ask a root "are
/// you still growing" without a record of which trees the world planted
/// grown, and a grown tree is never rebuilt into something else under a
/// player who has been cutting its limbs.
pub const GROWN_FOOT: u8 = 12;

/// How many shapes a young tree takes, the last of them a grown tree: a
/// sapling, two young trees, and the tree.
pub const TREE_STAGES: u8 = 4;

/// Which young tree grows at a column, whatever world it is in.
///
/// **Not seeded**, where every other roll in the generator is. The server
/// grows a sapling by working out the shape it has and the shape it gets,
/// and it does that with nothing but the root's position -- the growth
/// mechanic is handed a world of blocks, not the generator. A sapling the
/// generator put down from a seeded hash would be a shape the server could
/// not reproduce, and it would never grow.
pub fn young_tree_variant(gx: i32, gz: i32) -> u32 {
    hash2(gx, gz, 0x5A_911A)
}

/// Every cell of a young tree at `stage`, leaves first and wood after, as
/// offsets from the ground under its root; `None` past the last stage.
///
/// * **0**, the sapling the generator plants (`sapling_cells`).
/// * **1**, five blocks of stem six sixteenths at the foot -- still taken by
///   hand -- with two or three short limbs and a mass of leaves.
/// * **2**, seven blocks on a foot of ten, an axe's work now, the same limbs
///   on a taller stem.
/// * **3**, the grown branching tree (`branch_tree_cells`), on a foot of
///   `GROWN_FOOT` or more, after which nothing grows -- or, in birch leaves,
///   the grown birch (`birch_tree_cells`), on a foot of exactly that.
///
/// **In the bark the leaves say** (`bark_for`): a birch sapling is twigs of
/// birch and grows into a birch. The first three stages are the same shape
/// in either wood, so the server can tell a young birch's stage by the stem
/// alone exactly as it tells an oak's.
///
/// `rise` as for every tree here: a limb only grows over air.
pub fn tree_stage_cells(
    stage: u8,
    variant: u32,
    leaves: BlockId,
    rise: impl Fn(i32, i32) -> i32,
) -> Option<Vec<Cell>> {
    let log = bark_for(leaves);
    let cells = match stage {
        0 => sapling_cells(variant, leaves, rise),
        1 => young_cells(5, 6, variant, leaves, rise),
        2 => young_cells(7, 10, variant, leaves, rise),
        3 if log == BLOCK_BIRCH_LOG => birch_tree_cells(6 + (variant % 5) as i32, variant, leaves, rise, |_, _| false),
        3 => branch_tree_cells(4 + (variant % 3) as i32, 2, variant, leaves, rise, |_, _| false),
        _ => return None,
    };
    Some(in_bark(cells, log))
}

/// How many pieces of wood stand in a straight column up from the root, and
/// how wide the foot is: the two numbers a stage is told apart by, measured
/// the way the server measures a tree standing in the world.
pub fn stem_of(cells: &[Cell]) -> (i32, u8) {
    let wood: HashSet<(i32, i32, i32)> =
        cells.iter().filter(|(_, id)| is_branch(*id)).map(|&(at, _)| at).collect();
    let length = (1..).take_while(|&y| wood.contains(&(0, y, 0))).count() as i32;
    let foot = cells
        .iter()
        .rev()
        .find(|(at, id)| *at == (0, 1, 0) && is_branch(*id))
        .and_then(|&(_, id)| crate::types::branch_width(id))
        .unwrap_or(0);
    (length, foot)
}

/// A young tree: a stem `height` tall tapering from `foot` to four, two or
/// three limbs two columns long off its upper half, and small masses of
/// leaves over the stem and the limbs. See `tree_stage_cells`.
fn young_cells(height: i32, foot: u8, variant: u32, leaves: BlockId, rise: impl Fn(i32, i32) -> i32) -> Vec<Cell> {
    let mut crown: Vec<Cell> = Vec::new();
    let mut tree = Grower::default();
    let over_air = |(dx, dy, dz): (i32, i32, i32)| dy >= rise(dx, dz) + 2;
    for level in 1..=height {
        let width = foot as i32 - (foot as i32 - 4) * (level - 1) / (height - 1).max(1);
        tree.grow((0, level, 0), (level > 1).then_some((0, level - 1, 0)), width as u8);
    }
    blob(&mut crown, (0, height, 0), 2, 1, variant, leaves);
    let limbs = 2 + ((variant >> 3) & 1) as usize;
    let first = (variant >> 5) as usize;
    for i in 0..limbs {
        let (dx, dz) = AROUND[(first + i) % 4];
        let level = (height - 1 - i as i32).max(2);
        let mut last = (0, level, 0);
        for at in [(dx, level, dz), (2 * dx, level, 2 * dz), (2 * dx, level + 1, 2 * dz)] {
            if !over_air(at) || !tree.grow(at, Some(last), 4) {
                break;
            }
            last = at;
        }
        if (last.0, last.2) != (0, 0) {
            blob(&mut crown, last, 1, 1, variant ^ (0x9E37 * (i as u32 + 1)), leaves);
        }
    }
    crown.extend(tree.wood);
    crown
}

/// Writes a branching tree into a chunk's block array. `lx`/`lz` may be
/// outside the chunk, as for every tree: see `place_tree`.
#[allow(clippy::too_many_arguments)] // a shape, a place, its ground and a material
pub(super) fn place_branch_tree(
    blocks: &mut [BlockId],
    columns: &ColumnCache,
    lx: i32,
    ground: i32,
    lz: i32,
    height: i32,
    canopy: i32,
    variant: u32,
    leaves: BlockId,
    neighbour: impl Fn(i32, i32) -> bool,
) {
    // The leader and the crown clump over it reach three above the trunk.
    if ground + height + BRANCH_TALLER + 3 >= CHUNK_SIZE_Y as i32 {
        return;
    }
    let rise = |dx: i32, dz: i32| columns.at(lx + dx, lz + dz).height - ground;
    // In the bark of the wood whose crown it wears: a willow's limbs are
    // willow (`bark_for`), an oak's are left as they were built.
    let cells = in_bark(branch_tree_cells(height, canopy, variant, leaves, rise, neighbour), bark_for(leaves));
    for ((dx, dy, dz), id) in cells {
        // Wood overwrites and leaves fill air, as in `place_tree`.
        put_block(blocks, lx + dx, ground + dy, lz + dz, id, is_branch(id));
    }
}

/// A flat plate of acacia leaves round `(cx, cy, cz)`, one deep at the rim
/// and two over the middle, with a ragged edge -- `place_acacia`'s plate,
/// written as cells. See it for the circle and the rim.
fn plate(out: &mut Vec<Cell>, (cx, cy, cz): (i32, i32, i32), radius: i32, salt: u32) {
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let d = dx * dx + dz * dz;
            if d > radius * radius + 1 {
                continue;
            }
            let rim = d > (radius - 1) * (radius - 1) + 1;
            if rim && hash2(dx, dz, salt).is_multiple_of(3) {
                continue;
            }
            out.push(((cx + dx, cy, cz + dz), BLOCK_ACACIA_LEAVES));
            if radius >= 3 && d <= (radius - 2) * (radius - 2) + 1 {
                out.push(((cx + dx, cy + 1, cz + dz), BLOCK_ACACIA_LEAVES));
            }
        }
    }
}

/// Every cell of an acacia built of branches, leaves first and wood after.
///
/// **The same tree `place_acacia` draws in logs, and the bend is where it
/// has to differ.** The log acacia steps across a corner, which joins two
/// cubes at an edge; a post joins only through a face, so a corner step
/// would leave the upper stem hanging beside the lower one with nothing
/// between them. The crook is walked instead -- out along one axis, up,
/// out along the other, and up to the plate -- which is a zig-zag a player
/// reads as a bent stem, and every piece of it thinner than the one below.
///
/// Forked trees split at the bend: the long limb takes the plate, and a
/// short one goes the other way, two columns out, with a small plate a
/// block lower -- the notched crown `place_acacia` draws.
///
/// Inside `super::ACACIA_REACH` on both axes: the long limb ends one column
/// out and its plate is four wide; the short limb ends two out and its
/// plate two. `a_branch_acacia_is_a_crooked_stem_of_pieces_under_a_plate_inside_its_reach`.
pub(super) fn acacia_branch_cells(
    trunk: i32,
    canopy: i32,
    variant: u32,
    rise: impl Fn(i32, i32) -> i32,
) -> Vec<Cell> {
    let mut crown: Vec<Cell> = Vec::new();
    let mut tree = Grower::default();
    let over_air = |(dx, dy, dz): (i32, i32, i32)| dy >= rise(dx, dz) + 2;
    let (sx, sz) = acacia_lean(variant);
    let forked = (variant >> 7) & 1 == 1;
    // Two or three up to the bend, as the log tree, and never lower: a
    // piece off the stem needs a block of air under it.
    let bend = 2 + ((variant >> 12) & 1) as i32;
    let top = trunk.max(bend + 2);
    let canopy = canopy.clamp(1, super::ACACIA_REACH - 1);

    for level in 1..=bend {
        let parent = (level > 1).then_some((0, level - 1, 0));
        let width = 12 - 4 * (level - 1) / (bend - 1).max(1);
        tree.grow((0, level, 0), parent, width as u8);
    }

    // The crook, then straight up to the top from wherever it got to.
    let mut last = (0, bend, 0);
    for (at, width) in [((sx, bend, 0), 8), ((sx, bend + 1, 0), 8), ((sx, bend + 1, sz), 6)] {
        if !over_air(at) || !tree.grow(at, Some(last), width) {
            break;
        }
        last = at;
    }
    for level in last.1 + 1..=top {
        let at = (last.0, level, last.2);
        let width = if level == top { 4 } else { 6 };
        if !tree.grow(at, Some(last), width.min(branch_width_of(&tree, last))) {
            break;
        }
        last = at;
    }
    plate(&mut crown, (last.0, last.1 + 1, last.2), canopy, variant);

    if forked {
        let short_top = (top - 1).max(bend + 2);
        let mut limb = (0, bend, 0);
        for (at, width) in [((-sx, bend, 0), 6), ((-sx, bend + 1, 0), 6), ((-2 * sx, bend + 1, 0), 4)] {
            if !over_air(at) || !tree.grow(at, Some(limb), width) {
                break;
            }
            limb = at;
        }
        if limb.0 == -2 * sx {
            for level in limb.1 + 1..=short_top {
                let at = (limb.0, level, limb.2);
                if !tree.grow(at, Some(limb), 4) {
                    break;
                }
                limb = at;
            }
            plate(&mut crown, (limb.0, limb.1 + 1, limb.2), (canopy - 2).max(1), variant ^ 0x5A11);
        }
    }
    crown.extend(tree.wood);
    crown
}

/// The width of a piece a `Grower` has already placed.
fn branch_width_of(tree: &Grower, at: (i32, i32, i32)) -> u8 {
    tree.wood
        .iter()
        .find(|(cell, _)| *cell == at)
        .and_then(|&(_, id)| crate::types::branch_width(id))
        .unwrap_or(16)
}

/// Writes a branching acacia into a chunk. See `acacia_branch_cells`.
#[allow(clippy::too_many_arguments)] // a shape, a place and its ground
pub(super) fn place_branch_acacia(
    blocks: &mut [BlockId],
    columns: &ColumnCache,
    lx: i32,
    ground: i32,
    lz: i32,
    trunk: i32,
    canopy: i32,
    variant: u32,
) {
    if ground + trunk.max(5) + 3 >= CHUNK_SIZE_Y as i32 {
        return;
    }
    let rise = |dx: i32, dz: i32| columns.at(lx + dx, lz + dz).height - ground;
    for ((dx, dy, dz), id) in acacia_branch_cells(trunk, canopy, variant, rise) {
        put_block(blocks, lx + dx, ground + dy, lz + dz, id, is_branch(id));
    }
}

/// How tall a baobab's bole is, for this variant: seven to nine, as the
/// log baobab.
fn baobab_height(variant: u32) -> i32 {
    7 + (variant % 3) as i32
}

/// Every cell of a baobab built of branches, leaves first and wood after.
///
/// **The bole is the widest piece there is, side by side**, which the
/// mesher joins into solid bark -- a baobab is all trunk, and a bottle of
/// posts with daylight between them would be a cage. It narrows to twelve
/// over its last two levels, so the top of the bottle is rounded where the
/// log tree's is a flat lid. The swelling round the middle is the same
/// ring `place_baobab` lays, full width, and only where the ground leaves
/// it air.
///
/// **The limbs are what the preset adds**: three or four, each out from
/// the top of the bole, up, out again and up, ten to four sixteenths --
/// short thick arms that thin to a twig under a tuft, where the log tree
/// had one log at the root of a branch and one at its end. Walked through
/// faces for the acacia's reason.
///
/// Not a `Grower` tree: the bole is a block of pieces that touch each
/// other on purpose, which is exactly the lattice that type refuses.
///
/// Inside `super::OLD_TREE_REACH`: a limb's tip is three columns out at
/// most and its tuft one more.
pub(super) fn baobab_branch_cells(variant: u32, rise: impl Fn(i32, i32) -> i32) -> Vec<Cell> {
    let height = baobab_height(variant);
    let mut crown: Vec<Cell> = Vec::new();
    let mut wood: Vec<Cell> = Vec::new();
    let inner = |dx: i32, dz: i32| (0..=1).contains(&dx) && (0..=1).contains(&dz);
    for level in 1..=height {
        let swollen = (2..=height - 2).contains(&level);
        for dz in -1..=2 {
            for dx in -1..=2 {
                if inner(dx, dz) {
                    let width = if level >= height - 1 { 12 } else { 16 };
                    wood.push(((dx, level, dz), branch(width)));
                } else if swollen
                    && ((0..=1).contains(&dx) || (0..=1).contains(&dz))
                    && level > rise(dx, dz)
                {
                    wood.push(((dx, level, dz), branch(16)));
                }
            }
        }
    }
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        crown.push(((dx, height + 1, dz), BLOCK_ACACIA_LEAVES));
    }
    const OUT: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let limbs = 3 + ((variant >> 4) & 1) as usize;
    for i in 0..limbs {
        let (dx, dz) = OUT[((variant >> 6) as usize + i) % 4];
        let side = ((variant >> (10 + i as u32)) & 1) as i32;
        let from = (
            if dx > 0 { 1 } else if dx < 0 { 0 } else { side },
            if dz > 0 { 1 } else if dz < 0 { 0 } else { side },
        );
        let path = [
            ((from.0 + dx, height, from.1 + dz), 10),
            ((from.0 + dx, height + 1, from.1 + dz), 8),
            ((from.0 + 2 * dx, height + 1, from.1 + 2 * dz), 6),
            ((from.0 + 2 * dx, height + 2, from.1 + 2 * dz), 4),
        ];
        if path.iter().any(|&((x, y, z), _)| y < rise(x, z) + 2) {
            continue;
        }
        let tip = path[3].0;
        for (tx, ty, tz) in [
            (tip.0, tip.1 + 1, tip.2),
            (tip.0 + 1, tip.1, tip.2),
            (tip.0 - 1, tip.1, tip.2),
            (tip.0, tip.1, tip.2 + 1),
            (tip.0, tip.1, tip.2 - 1),
        ] {
            crown.push(((tx, ty, tz), BLOCK_ACACIA_LEAVES));
        }
        wood.extend(path.iter().map(|&(at, width)| (at, branch(width))));
    }
    crown.extend(wood);
    crown
}

/// Writes a branching baobab into a chunk. See `baobab_branch_cells`.
///
/// The swelling only fills air, as `place_baobab`'s does; the bole and the
/// limbs overwrite.
pub(super) fn place_branch_baobab(
    blocks: &mut [BlockId],
    columns: &ColumnCache,
    lx: i32,
    ground: i32,
    lz: i32,
    variant: u32,
) {
    let height = baobab_height(variant);
    if ground + height + 4 >= CHUNK_SIZE_Y as i32 {
        return;
    }
    let rise = |dx: i32, dz: i32| columns.at(lx + dx, lz + dz).height - ground;
    for ((dx, dy, dz), id) in baobab_branch_cells(variant, rise) {
        let ring = !((0..=1).contains(&dx) && (0..=1).contains(&dz)) && dy <= height;
        put_block(blocks, lx + dx, ground + dy, lz + dz, id, is_branch(id) && !ring);
    }
}

/// Where saplings grow, one root in how many columns.
///
/// **The four broadleaf countries**: the forest, the plain, the swamp, and
/// the birch wood now that a birch's young tree is in birch bark
/// (`tree_stage_cells`). Not the taiga, whose young trees would be the wrong
/// shape as well as the wrong bark; not the bog or the dead wood, where
/// nothing is growing.
///
/// Twice as many roots as the grown trees there, because the clearance
/// round a grown tree refuses most of them in a closed wood -- about one in
/// nine survives in a forest, most of them on a plain -- and what is left
/// is a couple of saplings a chunk in either.
fn sapling_spacing(biome: Biome) -> Option<u32> {
    if !matches!(biome, Biome::Forest | Biome::Plains | Biome::Swamp | Biome::BirchForest) {
        return None;
    }
    biome.tree_spacing().map(|trees| (trees / 2).max(8))
}

impl WorldGen {
    /// Whether a sapling is rooted at this column, before the ground has
    /// had its say. On its own salt, for the reason `old_tree_at` gives.
    fn sapling_at(&self, gx: i32, gz: i32, biome: Biome) -> bool {
        let Some(spacing) = sapling_spacing(biome) else {
            return false;
        };
        hash2(gx, gz, self.seed.wrapping_add(0x5A_9119)).is_multiple_of(spacing)
    }

    /// Saplings, including the ones rooted just outside this chunk. See
    /// `place_trees` for why the border is walked.
    ///
    /// **After every grown tree, and it only fills air and leaves**: a
    /// sapling is undergrowth, and one written through a trunk would be
    /// the mistake `place_bushes` is ordered to avoid. Every chunk writes
    /// the trees first and then the saplings in the same order, so the
    /// chunks either side of a border agree about every cell.
    pub(super) fn place_small_trees(
        &self,
        blocks: &mut [BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &ColumnCache,
    ) {
        for lz in -SAPLING_REACH..CHUNK_SIZE_Z as i32 + SAPLING_REACH {
            for lx in -SAPLING_REACH..CHUNK_SIZE_X as i32 + SAPLING_REACH {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let Column {
                    height: ground,
                    biome,
                    surface,
                    water,
                    ..
                } = columns.at(lx, lz);
                if !self.sapling_at(gx, gz, biome) {
                    continue;
                }
                // The ground a grown tree takes, less the ash: a burnt
                // wood has trunks standing in it, not young growth.
                if ground < water
                    || !matches!(block_kind(surface.top), BLOCK_GRASS | BLOCK_DIRT)
                    || self.is_cave(gx, ground, gz)
                    || ground + 6 >= CHUNK_SIZE_Y as i32
                {
                    continue;
                }
                // Room: no grown tree's root near enough for a limb to
                // touch the stem, no other sapling near enough for a shoot
                // to. From the hash and the column cache alone, so both
                // chunks of a border decide the same -- see the module note.
                let crowded = (-SAPLING_CLEARANCE..=SAPLING_CLEARANCE).any(|dz| {
                    (-SAPLING_CLEARANCE..=SAPLING_CLEARANCE).any(|dx| {
                        let near = columns.at(lx + dx, lz + dz).biome;
                        let (nx, nz) = (gx + dx, gz + dz);
                        let apart = dx.abs().max(dz.abs());
                        let sapling_near = (dx, dz) != (0, 0)
                            && apart <= SAPLING_SPACING_CLEARANCE
                            && self.sapling_at(nx, nz, near);
                        // A grown tree's own clearance: a birch's limbs are
                        // shorter. See `BIRCH_SAPLING_CLEARANCE`.
                        let clearance = if near.tree_kind() == super::TreeKind::Birch {
                            BIRCH_SAPLING_CLEARANCE
                        } else {
                            SAPLING_CLEARANCE
                        };
                        (apart <= clearance && self.tree_at(nx, nz, near)) || sapling_near
                    })
                });
                if crowded {
                    continue;
                }
                // The server's variant, so it can grow what it finds here.
                let variant = young_tree_variant(gx, gz);
                let rise = |dx: i32, dz: i32| columns.at(lx + dx, lz + dz).height - ground;
                let (log, leaves) = biome.tree_wood();
                for ((dx, dy, dz), id) in in_bark(sapling_cells(variant, leaves, rise), log) {
                    let (x, y, z) = (lx + dx, ground + dy, lz + dz);
                    if x < 0 || z < 0 || x >= CHUNK_SIZE_X as i32 || z >= CHUNK_SIZE_Z as i32 {
                        continue; // the neighbour's to draw
                    }
                    let index = Chunk::index(x as usize, y as usize, z as usize);
                    let here = blocks[index];
                    // Air for anything; a leaf of a grown crown for a twig,
                    // so a sapling under a canopy keeps its stem.
                    if here == BLOCK_AIR || (is_branch(id) && is_leafy(here)) {
                        blocks[index] = id;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{branch_width, has_full_top, ChunkPos, BLOCK_BIRCH_LEAVES, BLOCK_BOUGH, BLOCK_LEAVES, BLOCK_LOG, BLOCK_TWIG};
    use std::collections::{HashMap, VecDeque};

    /// Flat ground all round the root.
    fn level_ground(_dx: i32, _dz: i32) -> i32 {
        0
    }

    /// No other tree rooted anywhere near.
    fn no_neighbours(_dx: i32, _dz: i32) -> bool {
        false
    }

    /// Every piece reachable from the foot through faces, each no wider than
    /// the one it was reached from -- the taper, asked of any tree's cells.
    /// The message names the first piece that breaks it.
    fn tapers_from_the_foot(cells: &[Cell]) -> Result<(), String> {
        let wood = wood_of(cells);
        let root = (0, 1, 0);
        if !wood.contains_key(&root) {
            return Err("no wood at the foot".into());
        }
        let mut seen = HashSet::from([root]);
        let mut queue = VecDeque::from([root]);
        while let Some(cell) = queue.pop_front() {
            let width = branch_width(wood[&cell]).unwrap();
            for near in faces(cell) {
                if wood.contains_key(&near) && seen.insert(near) {
                    let child = branch_width(wood[&near]).unwrap();
                    if child > width {
                        return Err(format!("{child} wide at {near:?} out of {width} at {cell:?}"));
                    }
                    queue.push_back(near);
                }
            }
        }
        match wood.keys().find(|at| !seen.contains(at)) {
            Some(at) => Err(format!("the piece at {at:?} hangs from nothing")),
            None => Ok(()),
        }
    }

    #[test]
    fn a_young_tree_is_taller_and_thicker_at_every_stage_and_grown_at_the_last() {
        // What the server relies on to tell the stages apart and to know
        // when to stop: every stage stands higher than the one before on a
        // foot at least as wide, only the last is on `GROWN_FOOT`, and each
        // is a proper tree -- tapering, a tree and not a lattice, inside the
        // reach a grown one has. In an oak's leaves and a birch's.
        for (leaves, variant) in [BLOCK_LEAVES, BLOCK_BIRCH_LEAVES]
            .into_iter()
            .flat_map(|leaves| (0..65536u32).step_by(211).map(move |variant| (leaves, variant)))
        {
            let mut before = (0, 0u8);
            for stage in 0..TREE_STAGES {
                let cells = tree_stage_cells(stage, variant, leaves, level_ground).expect("a stage");
                let wood = wood_of(&cells);
                let top = wood.keys().map(|&(_, y, _)| y).max().unwrap_or(0);
                let (_, foot) = stem_of(&cells);
                assert!(
                    top > before.0 && foot >= before.1,
                    "stage {stage} of {variant:#x} is {top} tall on {foot} after {before:?}"
                );
                assert_eq!(
                    foot >= GROWN_FOOT,
                    stage + 1 == TREE_STAGES,
                    "stage {stage} of {variant:#x} stands on a foot of {foot}"
                );
                if let Err(why) = tapers_from_the_foot(&cells) {
                    panic!("stage {stage} of {variant:#x}: {why}");
                }
                for &((x, _, z), _) in &cells {
                    assert!(x.abs() <= BRANCH_REACH && z.abs() <= BRANCH_REACH);
                }
                before = (top, foot);
            }
            assert!(tree_stage_cells(TREE_STAGES, variant, leaves, level_ground).is_none());
        }
    }

    /// Every (height, variant) a birch of pieces is built with.
    fn every_birch() -> impl Iterator<Item = (i32, u32)> {
        (6..=10).flat_map(|height| (0..8192u32).step_by(29).map(move |variant| (height, variant)))
    }

    #[test]
    fn a_birch_is_a_slender_tree_of_pieces_inside_its_reach_that_tapers_and_never_closes_a_loop() {
        // The oak's three promises -- a taper from the foot, a tree and not a
        // lattice, inside the border the tree pass walks -- asked of the
        // birch's own shape, and the birch's own two: nothing wider than a
        // birch's bark comes in, and it has limbs.
        let mut furthest = 0;
        for (height, variant) in every_birch() {
            let cells = in_bark(
                birch_tree_cells(height, variant, BLOCK_BIRCH_LEAVES, level_ground, no_neighbours),
                BLOCK_BIRCH_LOG,
            );
            if let Err(why) = tapers_from_the_foot(&cells) {
                panic!("birch {height}/{variant:#x}: {why}");
            }
            let wood = wood_of(&cells);
            let joins: usize =
                wood.keys().map(|&at| faces(at).iter().filter(|near| wood.contains_key(near)).count()).sum::<usize>() / 2;
            assert_eq!(joins, wood.len() - 1, "birch {height}/{variant:#x} closes a loop of bark");
            assert!(
                wood.values().all(|&id| crate::types::is_birch_wood(id)),
                "birch {height}/{variant:#x} has a piece in oak bark"
            );
            assert_eq!(branch_width(wood[&(0, 1, 0)]), Some(GROWN_FOOT), "a grown birch not on a grown foot");
            let limbs = wood.keys().filter(|&&(x, _, z)| (x, z) != (0, 0)).count();
            assert!(limbs >= 6, "birch {height}/{variant:#x} has {limbs} pieces of limb");
            for &((x, _, z), _) in &cells {
                assert!(x.abs() <= BIRCH_REACH && z.abs() <= BIRCH_REACH, "birch {height}/{variant:#x} reaches ({x},{z})");
                furthest = furthest.max(x.abs()).max(z.abs());
            }
        }
        assert_eq!(furthest, BIRCH_REACH, "no birch reaches its border -- are the leaves hanging off the limbs?");
    }

    #[test]
    fn a_birch_wood_grows_birches_of_birch_pieces_and_birch_saplings() {
        // "у березы нету веток", asked of the world: a birch wood's trees are
        // pieces in birch bark rather than log cubes, and what grows between
        // them is birch.
        use crate::types::{is_birch_wood, BLOCK_BIRCH_LOG};
        let generator = WorldGen::new(2024);
        let (mut birch_pieces, mut logs, mut saplings) = (0, 0, 0);
        for chunk in crate::worldgen::tests::chunks_in(&generator, Biome::BirchForest, 12) {
            for (i, &b) in chunk.blocks.iter().enumerate() {
                if block_kind(b) == BLOCK_BIRCH_LOG {
                    logs += 1;
                } else if is_birch_wood(b) {
                    birch_pieces += 1;
                    // A birch twig standing on the ground is a sapling's foot:
                    // a limb only ever grows over air.
                    let under = chunk.blocks.get(i.wrapping_sub(CHUNK_SIZE_X * CHUNK_SIZE_Z)).copied().unwrap_or(BLOCK_AIR);
                    if block_kind(b) == BLOCK_TWIG && matches!(block_kind(under), BLOCK_GRASS | BLOCK_DIRT) {
                        saplings += 1;
                    }
                }
            }
        }
        assert!(birch_pieces > logs, "the birch wood stands on {logs} logs against {birch_pieces} pieces");
        assert!(saplings > 0, "twelve chunks of birch wood with no birch sapling in them");
    }

    #[test]
    fn two_trees_whose_limbs_point_at_each_other_do_not_meet_in_a_bar_of_bark() {
        // The ladders: a close wood had trees five columns apart with limbs
        // pointing at each other, and the mesher joined the tips across the
        // gap. With a root five columns east, every piece of this tree has
        // to stay more than `NEIGHBOUR_REACH` from it -- the room that
        // tree's own limbs, `LIMB_OUT` columns long, need to end a column
        // short of this one's.
        let east = |dx: i32, dz: i32| (dx, dz) == (5, 0);
        for (height, canopy, variant) in every_branch_tree() {
            let wood = wood_of(&branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, level_ground, east));
            for &(x, y, z) in wood.keys() {
                assert!(
                    (x - 5).abs().max(z.abs()) > NEIGHBOUR_REACH,
                    "tree {height}/{canopy}/{variant:#x} grows a piece at ({x},{y},{z}), \
                     in reach of the tree rooted at (5,0)"
                );
            }
        }
    }

    #[test]
    fn a_branch_tree_is_taller_than_the_trunk_the_biome_asked_for_and_its_limbs_are_up_the_stem() {
        // The player's ask, as numbers: the wood stands `BRANCH_TALLER` over
        // the biome's trunk, and no limb leaves the trunk lower than two
        // fifths of the way up -- a clear stem under the crown.
        for (height, canopy, variant) in every_branch_tree() {
            let wood = wood_of(&branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, level_ground, no_neighbours));
            let total = height + BRANCH_TALLER;
            let top = wood.keys().map(|&(_, y, _)| y).max().unwrap_or(0);
            assert!(top > total, "tree {height}/{canopy}/{variant:#x} tops out at {top}");
            let lowest_limb = wood.keys().filter(|&&(x, _, z)| (x, z) != (0, 0)).map(|&(_, y, _)| y).min();
            if let Some(y) = lowest_limb {
                assert!(
                    y * 5 >= total * 2,
                    "tree {height}/{canopy}/{variant:#x} has a limb at {y} of a {total}-block trunk"
                );
            }
        }
    }

    #[test]
    fn a_branch_acacia_is_a_crooked_stem_of_pieces_under_a_plate_inside_its_reach() {
        let mut crooked = 0;
        let mut forked = 0;
        for variant in (0..65536u32).step_by(97) {
            for trunk in 2..=6 {
                for canopy in 1..=4 {
                    let cells = acacia_branch_cells(trunk, canopy, variant, level_ground);
                    for &((x, y, z), _) in &cells {
                        assert!(
                            x.abs() <= super::super::ACACIA_REACH && z.abs() <= super::super::ACACIA_REACH,
                            "acacia {trunk}/{canopy}/{variant:#x} reaches ({x},{z})"
                        );
                        assert!(y >= 1, "acacia {variant:#x} has a cell under its own root");
                    }
                    if let Err(why) = tapers_from_the_foot(&cells) {
                        panic!("acacia {trunk}/{canopy}/{variant:#x}: {why}");
                    }
                    assert!(
                        cells.iter().any(|&(_, id)| id == BLOCK_ACACIA_LEAVES),
                        "acacia {variant:#x} has no plate"
                    );
                    let wood = wood_of(&cells);
                    crooked += usize::from(wood.keys().any(|&(x, _, z)| x != 0 && z != 0));
                    forked += usize::from(wood.keys().any(|&(x, _, _)| x.abs() == 2));
                }
            }
        }
        assert!(crooked > 0, "no branch acacia leans");
        assert!(forked > 0, "no branch acacia forks");
    }

    #[test]
    fn a_branch_baobab_is_a_bole_of_the_widest_pieces_with_limbs_inside_the_old_tree_border() {
        for variant in (0..65536u32).step_by(61) {
            let cells = baobab_branch_cells(variant, level_ground);
            let height = baobab_height(variant);
            for &((x, _, z), _) in &cells {
                assert!(
                    x.abs() <= super::super::OLD_TREE_REACH && z.abs() <= super::super::OLD_TREE_REACH,
                    "baobab {variant:#x} reaches ({x},{z})"
                );
            }
            if let Err(why) = tapers_from_the_foot(&cells) {
                panic!("baobab {variant:#x}: {why}");
            }
            let wood = wood_of(&cells);
            assert_eq!(branch_width(wood[&(0, 1, 0)]), Some(16), "baobab {variant:#x} is thin at the foot");
            let limbs = wood.keys().filter(|&&(_, y, _)| y > height).count();
            assert!(limbs >= 3 * 3, "baobab {variant:#x} has {limbs} pieces of limb over its bole");
        }
    }

    #[test]
    fn a_savanna_grows_its_acacias_out_of_pieces() {
        // Twelve chunks, not one: open savanna holds a tree in two thousand
        // columns between its groves (`SAVANNA_OPEN_SPACING`), and the first
        // version of this test asked a single chunk that had none.
        //
        // It used to compare the ordinary world against the branch world; the
        // two are one world now, so it asks the one world that its acacias are
        // wood of pieces and not columns of logs. A log or two may still stand
        // in a savanna -- a fallen giant, a ruin -- which is why the bar is
        // "more pieces than logs" and not "no logs".
        // A savanna world: a temperate one has no savanna near enough to
        // search since the Earth's scale (`worldgen::tests::world_for`).
        let world = crate::worldgen::tests::world_for(1337, Biome::Savanna);
        let pieces = |chunk: &Chunk| chunk.blocks.iter().filter(|&&b| is_branch(b)).count();
        let logs = |chunk: &Chunk| chunk.blocks.iter().filter(|&&b| b == BLOCK_LOG).count();
        let (mut all_pieces, mut all_logs) = (0, 0);
        for chunk in crate::worldgen::tests::chunks_in(&world, Biome::Savanna, 12) {
            all_pieces += pieces(&chunk);
            all_logs += logs(&chunk);
        }
        assert!(all_pieces > 0, "twelve chunks of savanna grew no branches");
        assert!(all_pieces > all_logs, "the savanna stands on {all_logs} logs against {all_pieces} pieces");
    }

    #[test]
    fn a_limb_never_grows_into_or_onto_the_ground_beside_its_tree() {
        // The bug the first pictures showed: a wood on rolling ground had
        // limbs lying on the rise beside their tree, and limbs written into
        // the hillside. Asked against a slope climbing to the north-east,
        // steep enough to reach every limb level: every piece off the trunk
        // has at least one cell of air between it and the ground under it.
        let slope = |dx: i32, dz: i32| 2 * (dx + dz);
        for (height, canopy, variant) in every_branch_tree() {
            for ((dx, dy, dz), id) in branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, slope, no_neighbours) {
                if is_branch(id) && (dx, dz) != (0, 0) {
                    assert!(
                        dy >= slope(dx, dz) + 2,
                        "tree {height}/{canopy}/{variant:#x} grows a piece at ({dx},{dy},{dz}) \
                         over ground at {}",
                        slope(dx, dz)
                    );
                }
            }
        }
        for variant in 0..4096 {
            for ((dx, dy, dz), id) in sapling_cells(variant, BLOCK_LEAVES, slope) {
                if is_branch(id) && (dx, dz) != (0, 0) {
                    assert!(dy >= slope(dx, dz) + 2, "sapling {variant:#x} has a shoot in the ground");
                }
            }
        }
    }

    /// The wood of a tree as the chunk will hold it: later writes win.
    fn wood_of(cells: &[Cell]) -> HashMap<(i32, i32, i32), BlockId> {
        cells.iter().filter(|(_, id)| is_branch(*id)).copied().collect()
    }

    fn faces((x, y, z): (i32, i32, i32)) -> [(i32, i32, i32); 6] {
        [
            (x + 1, y, z),
            (x - 1, y, z),
            (x, y + 1, z),
            (x, y - 1, z),
            (x, y, z + 1),
            (x, y, z - 1),
        ]
    }

    /// Every (height, canopy, variant) a branching tree is built with, and
    /// a spread of variants past them.
    fn every_branch_tree() -> impl Iterator<Item = (i32, i32, u32)> {
        (4..=9).flat_map(|height| {
            [2, MAX_CANOPY_RADIUS]
                .into_iter()
                .flat_map(move |canopy| (0..8192u32).step_by(37).map(move |variant| (height, canopy, variant)))
        })
    }

    #[test]
    fn every_piece_of_a_branch_tree_hangs_from_the_trunk_and_is_no_thicker_than_what_holds_it() {
        // The taper is the whole of what this tree is: a limb thicker than
        // the trunk it grows out of, or a twig holding up a bough, is a
        // tree drawn upside down. Asked by walking out from the foot, so a
        // piece that hangs from nothing is caught as well.
        for (height, canopy, variant) in every_branch_tree() {
            let wood = wood_of(&branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, level_ground, no_neighbours));
            let root = (0, 1, 0);
            assert_eq!(
                wood.get(&root).map(|&id| block_kind(id)),
                Some(BLOCK_BOUGH),
                "tree {height}/{canopy}/{variant:#x} has no bough at its foot"
            );
            let mut seen: HashMap<(i32, i32, i32), u32> = HashMap::from([(root, 0)]);
            let mut queue = VecDeque::from([root]);
            while let Some(cell) = queue.pop_front() {
                let width = branch_width(wood[&cell]).unwrap();
                for near in faces(cell) {
                    if wood.contains_key(&near) && !seen.contains_key(&near) {
                        let child = branch_width(wood[&near]).unwrap();
                        assert!(
                            child <= width,
                            "tree {height}/{canopy}/{variant:#x}: a piece {child} wide at {near:?} \
                             grows out of one {width} wide at {cell:?}"
                        );
                        seen.insert(near, seen[&cell] + 1);
                        queue.push_back(near);
                    }
                }
            }
            for at in wood.keys() {
                assert!(
                    seen.contains_key(at),
                    "tree {height}/{canopy}/{variant:#x}: the piece at {at:?} hangs from nothing"
                );
            }
        }
    }

    #[test]
    fn a_branch_tree_is_a_tree_and_not_a_lattice() {
        // The mesher joins every piece to every piece beside it. A tree
        // whose pieces touch in a loop is drawn with a closed square of
        // bark in its crown -- a limb's tip turned up beside the trunk and
        // joined straight back to it. A tree of n pieces has n - 1 joins.
        let joins = |wood: &HashMap<(i32, i32, i32), BlockId>| {
            wood.keys()
                .map(|&at| faces(at).iter().filter(|near| wood.contains_key(near)).count())
                .sum::<usize>()
                / 2
        };
        for (height, canopy, variant) in every_branch_tree() {
            let wood = wood_of(&branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, level_ground, no_neighbours));
            assert_eq!(
                joins(&wood),
                wood.len() - 1,
                "tree {height}/{canopy}/{variant:#x} closes a loop of bark"
            );
        }
        for variant in 0..4096 {
            let wood = wood_of(&sapling_cells(variant, BLOCK_LEAVES, level_ground));
            assert_eq!(joins(&wood), wood.len() - 1, "sapling {variant:#x} closes a loop");
        }
    }

    #[test]
    fn a_branch_tree_stays_inside_the_border_the_tree_pass_walks() {
        // `place_trees` roots a broadleaf up to `MAX_CANOPY_RADIUS` outside
        // the chunk and no further. A leaf past that is a leaf missing from
        // one side of a tree on every chunk seam.
        let mut furthest = 0;
        for (height, canopy, variant) in every_branch_tree() {
            for ((x, y, z), _) in
                branch_tree_cells(height, canopy, variant, BLOCK_LEAVES, level_ground, no_neighbours)
            {
                assert!(
                    x.abs() <= BRANCH_REACH && z.abs() <= BRANCH_REACH,
                    "tree {height}/{canopy}/{variant:#x} reaches ({x},{z})"
                );
                assert!(
                    y >= 1 && y <= height + BRANCH_TALLER + 3,
                    "tree {height} puts a cell at height {y}"
                );
                furthest = furthest.max(x.abs()).max(z.abs());
            }
        }
        assert_eq!(furthest, BRANCH_REACH, "no branch tree reaches its border -- is the crown there?");
    }

    #[test]
    fn a_sapling_is_a_few_twigs_a_hand_can_take_and_a_tuft_of_leaves() {
        let mut heights = HashSet::new();
        for variant in 0..4096 {
            let cells = sapling_cells(variant, BLOCK_LEAVES, level_ground);
            let wood = wood_of(&cells);
            for (&(x, _, z), &id) in &wood {
                assert_eq!(block_kind(id), BLOCK_TWIG, "sapling {variant:#x} has wood an axe is needed for");
                assert!(x.abs() <= SAPLING_REACH && z.abs() <= SAPLING_REACH);
            }
            let stem = (1..).take_while(|&y| wood.contains_key(&(0, y, 0))).count();
            assert!((2..=4).contains(&stem), "sapling {variant:#x} is {stem} twigs tall");
            heights.insert(stem);
            assert!(
                cells.iter().any(|&(_, id)| id == BLOCK_LEAVES),
                "sapling {variant:#x} has no leaves"
            );
            for ((x, _, z), _) in &cells {
                assert!(x.abs() <= SAPLING_REACH && z.abs() <= SAPLING_REACH);
            }
        }
        assert_eq!(heights.len(), 3, "saplings come in {heights:?} heights, not two, three and four");
    }

    /// The first forest the transect finds, and the generator that made it.
    fn a_forest_in_both_worlds() -> (Chunk, WorldGen) {
        let ordinary = WorldGen::new(2024);
        let forest = crate::worldgen::tests::chunk_in(&ordinary, Biome::Forest)
            .expect("no forest anywhere on the transect for seed 2024");
        (forest, WorldGen::new(2024))
    }

    #[test]
    fn a_forest_grows_its_broadleaf_out_of_pieces() {
        // This compared an ordinary forest of logs with a branch world's
        // forest of pieces, and promised the ordinary one was untouched. The
        // branch world is the ordinary world now (`Preset::Normal`), so the one
        // forest is asked what the branch world was: its trees are wood of
        // pieces, and the same chunk made twice is the same chunk.
        let (forest, generator) = a_forest_in_both_worlds();
        let pieces = forest.blocks.iter().filter(|&&b| is_branch(b)).count();
        assert!(pieces > 0, "a forest grew no branches");
        assert!(
            generator.generate_chunk(forest.pos).blocks == forest.blocks,
            "the same forest came out different the second time"
        );
    }

    #[test]
    fn saplings_grow_in_a_branch_world_and_stand_on_the_ground() {
        let generator = WorldGen::new(2024);
        let mut saplings = 0;
        for biome in [Biome::Forest, Biome::Plains] {
            for chunk in crate::worldgen::tests::chunks_in(&generator, biome, 6) {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        for y in 1..CHUNK_SIZE_Y - 1 {
                            let here = chunk.get(x, y, z);
                            if block_kind(here) != BLOCK_TWIG || is_branch(chunk.get(x, y - 1, z)) {
                                continue;
                            }
                            // A twig with no wood under it: the foot of a
                            // sapling, or the end of a limb over the grass.
                            let under = chunk.get(x, y - 1, z);
                            if !matches!(block_kind(under), BLOCK_GRASS | BLOCK_DIRT) {
                                continue;
                            }
                            let stem = (y..CHUNK_SIZE_Y).take_while(|&up| is_branch(chunk.get(x, up, z))).count();
                            assert!(stem <= 4, "a sapling {stem} tall at ({x},{y},{z}) of {:?}", chunk.pos);
                            // ...and it is the stage the server will grow it from.
                            let (gx, gz) = (
                                chunk.pos.x * CHUNK_SIZE_X as i32 + x as i32,
                                chunk.pos.z * CHUNK_SIZE_Z as i32 + z as i32,
                            );
                            let stage =
                                tree_stage_cells(0, young_tree_variant(gx, gz), BLOCK_LEAVES, level_ground).unwrap();
                            assert_eq!(
                                stem as i32,
                                stem_of(&stage).0,
                                "the sapling at ({gx},{gz}) is not the one the server would grow"
                            );
                            saplings += 1;
                        }
                    }
                }
            }
        }
        assert!(saplings > 0, "twelve chunks of forest and plain and not one sapling");
    }

    #[test]
    fn every_piece_of_wood_in_a_branch_wood_is_joined_to_the_ground() {
        // **The seam test.** Both chunks of a border decide a tree from the
        // same hash, and if they ever disagreed the part one of them drew
        // would hang from a trunk the other did not. So the middle chunk of
        // a square of nine is walked, and every piece in it has to reach,
        // through pieces, one that stands on something solid -- wherever in
        // the nine that foot is.
        let (forest, generator) = a_forest_in_both_worlds();
        let mut chunks = HashMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let pos = ChunkPos::new(forest.pos.x + dx, forest.pos.z + dz);
                chunks.insert((dx, dz), generator.generate_chunk(pos));
            }
        }
        let size = CHUNK_SIZE_X as i32;
        let look = |x: i32, y: i32, z: i32| -> Option<BlockId> {
            if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
                return None;
            }
            let chunk = chunks.get(&(x.div_euclid(size), z.div_euclid(size)))?;
            Some(chunk.get(x.rem_euclid(size) as usize, y as usize, z.rem_euclid(size) as usize))
        };
        let mut walked = 0;
        for y in 1..CHUNK_SIZE_Y as i32 {
            for z in 0..size {
                for x in 0..size {
                    if !look(x, y, z).is_some_and(is_branch) {
                        continue;
                    }
                    walked += 1;
                    let mut seen = HashSet::from([(x, y, z)]);
                    let mut queue = VecDeque::from([(x, y, z)]);
                    let mut grounded = false;
                    while let Some((cx, cy, cz)) = queue.pop_front() {
                        let under = look(cx, cy - 1, cz);
                        if under.is_some_and(|b| !is_branch(b) && has_full_top(b)) {
                            grounded = true;
                            break;
                        }
                        for near in faces((cx, cy, cz)) {
                            if look(near.0, near.1, near.2).is_some_and(is_branch) && seen.insert(near) {
                                queue.push_back(near);
                            }
                        }
                    }
                    assert!(
                        grounded,
                        "the piece at ({x},{y},{z}) of {:?} hangs from nothing in the chunks round it",
                        forest.pos
                    );
                }
            }
        }
        assert!(walked > 0, "no wood in the middle chunk to walk");
    }
}
