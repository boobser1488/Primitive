//! Stalagmites and stalactites: what dripping water leaves in a cave, and
//! the one thing it does to a player.
//!
//! ## What they are for
//!
//! A cave was rock, air, and what lies on the floor. Dripstone is the
//! first thing in one that says *which* cave this is: they grow thick where
//! the rock is limestone and water comes down through it, so a roof hung
//! with them is a limestone country under a wet sky, and a bare one is not.
//!
//! **What they do is cut a falling body.** A drop onto the point of a
//! stalagmite is a cut in the leg (`SPIKE_FROM_BLOCKS`, and the survival
//! pass on the server), on top of whatever the fall itself costs -- and a
//! cut bleeds until it is bandaged. So the floor of a chamber is a thing to
//! look at before jumping into it: a shaft that opens over a bare floor is
//! a drop, and one that opens over a field of spikes is a climb, or a
//! bandage in the pack. That is a decision and not a number, which is the
//! rule every block here answers to.
//!
//! Three other uses were weighed and put down:
//!
//! * *A source of lime -- snapped off, burnt, a mortar or a flux.* The
//!   game has no lime, no mortar and no flux, and giving dripstone a use
//!   means writing all three. That is a branch of content, and the reef
//!   already turned the same shortcut down for the same reason
//!   (`types::BLOCK_BRAIN_CORAL`).
//! * *Drinking water: a jug under a stalactite fills.* The honest version
//!   needs a dripping tick, a vessel that fills in place and a reason a
//!   player underground has no water -- three systems for a cave that is
//!   already a short walk from a river.
//! * *A stalactite that falls on whoever breaks the floor under it.* Unfair
//!   in the way that teaches nothing: the roof is dark, the trigger is
//!   something the player cannot see, and the lesson is "caves kill you at
//!   random".
//!
//! Broken, one gives a pebble. Rubble is what a snapped-off spike is, and
//! anything more would be a quarry a player reaches without a pick.
//!
//! ## Why one id per direction and the size in the variant
//!
//! Which way it grows decides the support (`types::support_at`), the model's
//! orientation and whether it cuts: rules that read the row. How big it is
//! decides only the drawing and the box -- so the size is in the variant,
//! three steps, and a stalagmite of every size is one row in every table.

use crate::types::{
    block_kind, BlockId, BLOCK_BASALT, BLOCK_GRANITE, BLOCK_LIMESTONE, BLOCK_SANDSTONE, BLOCK_STALACTITE,
    BLOCK_STALAGMITE, BLOCK_STONE, VARIANT_MASK, VARIANT_SHIFT,
};

/// How many sizes a piece of dripstone comes in.
pub const SIZES: u8 = 3;

/// A stalagmite's tiers, by size, as (width, bottom, top) in sixteenths of
/// a cell, standing on the floor. A stalactite is the same numbers hung
/// from the roof ([`tiers`] turns them over).
///
/// **Stepped, not sloped.** Every model in this world is boxes on whole
/// sixteenths, and a cone of three or four boxes reads as a cone from a
/// step away. Even widths only, so each tier is centred on whole sixteenths
/// and its picture lands on texels (`mesh::push_box`).
///
/// The largest fills its cell to the roof: a column of two -- a stalagmite
/// under a stalactite, which is where the generator puts most of them --
/// then reads as one spike reaching for the other.
const TIERS: [&[(u8, u8, u8)]; SIZES as usize] = [
    &[(4, 0, 3), (2, 3, 6)],
    &[(6, 0, 4), (4, 4, 8), (2, 8, 11)],
    &[(8, 0, 5), (6, 5, 10), (4, 10, 14), (2, 14, 16)],
];

/// How wide the box a body meets is, by size, in sixteenths.
///
/// **One box, and narrower than the foot of the spike.** A collider that
/// walked the tiers would be three boxes where every other gatherer of
/// boxes expects one (`geometry::block_box`), and the first one to forget
/// the other two is a player standing inside the base. The foot's own
/// width was the other choice, and it put a solid ledge in the air beside
/// the tip that a player could stand on without touching anything drawn.
/// The middle is what an eye takes a spike's thickness to be.
const BODY_WIDTH: [u8; SIZES as usize] = [4, 4, 6];

/// The shortest fall onto a point that cuts.
///
/// **Two blocks, over a jump.** A standing jump peaks at about a block and
/// a half (`physics::JUMP_VELOCITY` against its gravity), and a player who
/// hops off the top of a small stalagmite has not fallen on anything. Two
/// is the first ledge somebody chose to drop from -- and it is under the
/// three blocks a fall starts to hurt at, which is the point: a drop that
/// is free onto rock is not free onto a spike.
pub const SPIKE_FROM_BLOCKS: f32 = 2.0;

/// Health a spike costs, on top of the fall's own.
pub const SPIKE_DAMAGE: f32 = 2.0;

/// How bad the cut is from the shortest fall that cuts, on the scale
/// `injury::Injuries::inflict` takes: past `injury::CUT_CLOTS_BELOW`, so it
/// bleeds until it is dressed. A spike that cut and clotted on its own
/// would be a scratch nobody changed their plans for.
pub const SPIKE_CUT: f32 = 0.4;

/// ...and how much worse for every block fallen past that.
pub const SPIKE_CUT_PER_BLOCK: f32 = 0.1;

/// Is this a stalagmite or a stalactite, of any size?
#[inline]
pub fn is_dripstone(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_STALAGMITE | BLOCK_STALACTITE)
}

/// Does this one hang from the roof rather than stand on the floor?
#[inline]
pub fn hangs(id: BlockId) -> bool {
    block_kind(id) == BLOCK_STALACTITE
}

/// Which of the [`SIZES`], 0 the smallest.
#[inline]
pub fn size(id: BlockId) -> u8 {
    (((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8).min(SIZES - 1)
}

/// `kind` at `size`.
#[inline]
pub fn sized(kind: BlockId, size: u8) -> BlockId {
    block_kind(kind) | (BlockId::from(size.min(SIZES - 1)) << VARIANT_SHIFT)
}

/// Can a piece of dripstone grow from this?
///
/// **The rocks under the soil and nothing else.** Dripstone is rock the
/// water put back, and it grows out of the rock the water came through --
/// never out of soil, a player's wall or a tree. Basalt too: a dyke crosses
/// a cave now and then, and a stalactite that fell off the roof the moment
/// the rock over it changed colour would be the grass-on-stone mistake.
#[inline]
pub fn grows_from(ground: BlockId) -> bool {
    matches!(
        block_kind(ground),
        BLOCK_STONE | BLOCK_LIMESTONE | BLOCK_SANDSTONE | BLOCK_GRANITE | BLOCK_BASALT
    )
}

/// The boxes a piece of dripstone is drawn as, (from, to) in sixteenths of
/// its cell, foot first.
pub fn tiers(id: BlockId) -> impl Iterator<Item = ([f32; 3], [f32; 3])> {
    let hangs = hangs(id);
    TIERS[size(id) as usize].iter().map(move |&(width, bottom, top)| {
        let (lo, hi) = (8.0 - f32::from(width) / 2.0, 8.0 + f32::from(width) / 2.0);
        let (y0, y1) = if hangs {
            (16.0 - f32::from(top), 16.0 - f32::from(bottom))
        } else {
            (f32::from(bottom), f32::from(top))
        };
        ([lo, y0, lo], [hi, y1, hi])
    })
}

/// How far from its root to its tip, in cells.
pub fn length(id: BlockId) -> f32 {
    TIERS[size(id) as usize].iter().map(|&(_, _, top)| f32::from(top)).fold(0.0, f32::max) / 16.0
}

/// The box a body meets, (min, max) in cells from the corner of its cell.
/// See [`BODY_WIDTH`] for why it is one box and how wide.
pub fn body(id: BlockId) -> ([f32; 3], [f32; 3]) {
    let half = f32::from(BODY_WIDTH[size(id) as usize]) / 32.0;
    let (lo, hi) = (0.5 - half, 0.5 + half);
    let (y0, y1) = if hangs(id) { (1.0 - length(id), 1.0) } else { (0.0, length(id)) };
    ([lo, y0, lo], [hi, y1, hi])
}

/// Is a player standing at `feet` standing on the point of a stalagmite?
///
/// **On its tip, not beside it.** The cells under the player's footprint
/// are asked, and one counts when it is a stalagmite whose top is where the
/// feet are and whose box is under them: a player who landed on the floor
/// next to a spike landed on the floor. A stalactite never counts -- nobody
/// lands on the underside of one.
pub fn spike_under(feet: (f64, f64, f64), block_at: impl Fn(i32, i32, i32) -> BlockId) -> bool {
    /// How far off the tip the feet may be and still be on it: the
    /// collider rests on the box within a contact skin, and a server
    /// reading a transform a tick late sees a hair of settling.
    const ON_TOP: f64 = 0.1;
    const PLAYER_HALF_WIDTH: f64 = crate::geometry::PLAYER_HALF_WIDTH as f64;
    let (x, y, z) = feet;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return false;
    }
    let cy = (y - ON_TOP).floor() as i32;
    let (x0, x1) = ((x - PLAYER_HALF_WIDTH).floor() as i32, (x + PLAYER_HALF_WIDTH).floor() as i32);
    let (z0, z1) = ((z - PLAYER_HALF_WIDTH).floor() as i32, (z + PLAYER_HALF_WIDTH).floor() as i32);
    for cz in z0..=z1 {
        for cx in x0..=x1 {
            let block = block_at(cx, cy, cz);
            if block_kind(block) != BLOCK_STALAGMITE {
                continue;
            }
            let (min, max) = body(block);
            let top = f64::from(cy) + f64::from(max[1]);
            let under = x + PLAYER_HALF_WIDTH > f64::from(cx) + f64::from(min[0])
                && x - PLAYER_HALF_WIDTH < f64::from(cx) + f64::from(max[0])
                && z + PLAYER_HALF_WIDTH > f64::from(cz) + f64::from(min[2])
                && z - PLAYER_HALF_WIDTH < f64::from(cz) + f64::from(max[2]);
            if under && (y - top).abs() <= ON_TOP {
                return true;
            }
        }
    }
    false
}

/// How bad a cut a fall of `distance` blocks onto a spike leaves, or `None`
/// for a drop too short to cut. See [`SPIKE_FROM_BLOCKS`].
pub fn spike_cut(distance: f32) -> Option<f32> {
    if !distance.is_finite() || distance < SPIKE_FROM_BLOCKS {
        return None;
    }
    Some((SPIKE_CUT + (distance - SPIKE_FROM_BLOCKS) * SPIKE_CUT_PER_BLOCK).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{is_known_block, BLOCK_AIR};

    fn every_piece() -> impl Iterator<Item = BlockId> {
        [BLOCK_STALAGMITE, BLOCK_STALACTITE].into_iter().flat_map(|kind| (0..SIZES).map(move |s| sized(kind, s)))
    }

    #[test]
    fn every_size_of_dripstone_is_a_block_the_game_could_have_written_and_no_other_is() {
        for piece in every_piece() {
            assert!(is_known_block(piece), "{piece:#x} is refused");
        }
        for kind in [BLOCK_STALAGMITE, BLOCK_STALACTITE] {
            for step in SIZES..8 {
                let invented = kind | (BlockId::from(step) << VARIANT_SHIFT);
                assert!(!is_known_block(invented), "{invented:#x} is a second id for the largest size");
            }
        }
    }

    #[test]
    fn a_stalagmite_rises_from_its_floor_and_a_stalactite_hangs_from_its_roof_inside_the_cell() {
        for piece in every_piece() {
            let boxes: Vec<_> = tiers(piece).collect();
            let root = boxes[0];
            if hangs(piece) {
                assert_eq!(root.1[1], 16.0, "{piece:#x} does not touch its roof");
            } else {
                assert_eq!(root.0[1], 0.0, "{piece:#x} does not touch its floor");
            }
            for (from, to) in &boxes {
                for axis in 0..3 {
                    assert!(from[axis] >= 0.0 && to[axis] <= 16.0 && from[axis] < to[axis], "{piece:#x} leaves its cell");
                }
            }
            // Thinner towards the tip, every step of the way.
            for pair in boxes.windows(2) {
                assert!(pair[1].1[0] - pair[1].0[0] < pair[0].1[0] - pair[0].0[0], "{piece:#x} is not a spike");
            }
        }
    }

    #[test]
    fn the_box_a_body_meets_is_inside_what_is_drawn() {
        for piece in every_piece() {
            let (min, max) = body(piece);
            let (lo, hi) = tiers(piece).fold(([16.0f32; 3], [0.0f32; 3]), |(lo, hi), (from, to)| {
                (
                    [lo[0].min(from[0]), lo[1].min(from[1]), lo[2].min(from[2])],
                    [hi[0].max(to[0]), hi[1].max(to[1]), hi[2].max(to[2])],
                )
            });
            for axis in 0..3 {
                assert!(min[axis] * 16.0 >= lo[axis] - 1e-4 && max[axis] * 16.0 <= hi[axis] + 1e-4, "{piece:#x} is walked into where nothing is drawn");
            }
        }
    }

    #[test]
    fn bigger_dripstone_is_longer() {
        for kind in [BLOCK_STALAGMITE, BLOCK_STALACTITE] {
            for s in 1..SIZES {
                assert!(length(sized(kind, s)) > length(sized(kind, s - 1)));
            }
        }
    }

    #[test]
    fn landing_on_the_point_of_a_stalagmite_is_on_it_and_landing_beside_it_is_not() {
        let spike = sized(BLOCK_STALAGMITE, 1);
        let world = move |x: i32, y: i32, z: i32| if (x, y, z) == (4, 10, 4) { spike } else { BLOCK_AIR };
        let top = 10.0 + f64::from(length(spike));
        assert!(spike_under((4.5, top, 4.5), world));
        // A shoulder over the tip counts: the collider is resting on it.
        assert!(spike_under((4.5 + 0.35, top, 4.5), world));
        // On the floor of the same cell, beside the spike.
        assert!(!spike_under((4.5, 10.0, 4.5), world));
        // Level with the tip, but a whole body away from it.
        assert!(!spike_under((6.5, top, 4.5), world));
        // The underside of a stalactite is not a point anybody lands on.
        let hanging = sized(BLOCK_STALACTITE, 2);
        let roof = move |x: i32, y: i32, z: i32| if (x, y, z) == (4, 10, 4) { hanging } else { BLOCK_AIR };
        assert!(!spike_under((4.5, 11.0, 4.5), roof));
        assert!(!spike_under((4.5, 10.0, 4.5), roof));
    }

    #[test]
    fn a_hop_onto_a_spike_does_not_cut_and_a_drop_does_and_a_longer_drop_cuts_deeper() {
        assert_eq!(spike_cut(1.4), None);
        let short = spike_cut(SPIKE_FROM_BLOCKS).expect("the shortest drop that cuts");
        assert!(short > crate::injury::CUT_CLOTS_BELOW, "a spike cut that stops bleeding on its own");
        assert!(spike_cut(6.0).unwrap() > short);
        assert!(spike_cut(100.0).unwrap() <= 1.0);
        assert_eq!(spike_cut(f32::NAN), None);
    }
}
