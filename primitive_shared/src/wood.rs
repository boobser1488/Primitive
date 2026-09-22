//! The woods: which blocks are one tree's timber, and what stands in for
//! what when a recipe asks for "a log" or "planks".
//!
//! ## Why a table
//!
//! A wood is four blocks that go together -- a log, the crown it grows, the
//! boards sawn from it and those boards pegged -- and every rule that is
//! about *wood* rather than about *oak* has to know all four of every wood.
//! While there were two woods that knowledge was a `BLOCK_LOG |
//! BLOCK_BIRCH_LOG` written wherever it was needed, forty times across four
//! crates, and a third wood would have been forty edits of which the one
//! forgotten is a fir that does not burn or a saxaul chest that cannot be
//! made. So the woods are one table, and the questions are asked of it.
//!
//! Rejected: *a wood as a variant of the oak's ids*, the three spare bits
//! every block carries. It would make every wood stack with every other --
//! and the reason the woods exist is that a house built of pale fir is
//! visibly not a house built of oak (`types::BLOCK_BIRCH_LOG` argues this
//! for the birch). The variant field of a log is its axis besides.
//!
//! ## Which woods, and where they grow
//!
//! | wood   | log         | crown          | where                     |
//! |--------|-------------|----------------|---------------------------|
//! | oak    | `log`       | `leaves` (and the apple, maple and acacia crowns) | the temperate woods, the swamp, the savanna |
//! | birch  | `birch_log` | `birch_leaves` | the birch wood            |
//! | fir    | `fir_log`   | `fir_needles`  | the taiga and the treeline |
//! | saxaul | `saxaul_log`| `saxaul_leaves`| the desert                |
//! | pine   | `pine_log`  | `pine_needles` | the dry pinewoods of the taiga, on sand |
//! | willow | `willow_log`| `willow_leaves`| the swamp                 |
//!
//! **Every wood has its own twig and bough now** (`types::OWN_BARK`): the
//! paragraph below weighed two ids a bark and turned them down for a tree a
//! player tells by its crown. What changed is that a player asked for them --
//! a fir felled for fir timber whose limbs came down oak is a lie told at the
//! moment of cutting -- and that the price turned out to be one predicate
//! (`types::is_twig`, `is_bough`) asked in the dozen places that named the
//! ids, not a second arm on every rule.
//!
//! **The palm is not a wood here**: its trunk is a piece of branch that
//! gives an oak's log (`types::BLOCK_PALM_TRUNK`). **Nor are the savanna's
//! acacia or the swamp's trees**, and that was weighed: both are trees of
//! branches, and a branch's bark lives in the three bits of its variant
//! (`types::birch_branch`), which the oak and the birch already fill. A
//! third bark is two new ids for a twig and a bough and every rule that
//! names a piece of branch learning them -- the collider, the mesher's
//! joins, the growth of a sapling -- for a tree a player tells from an oak
//! by its crown already. The fir and the saxaul are trees of log cubes, and
//! cost exactly their four blocks.

use crate::types::{
    block_kind, BlockId, BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_BIRCH_PLANKS, BLOCK_FIR_LOG,
    BLOCK_FIR_NEEDLES, BLOCK_FIR_PLANKS, BLOCK_LEAVES, BLOCK_LOG, BLOCK_PEGGED_BIRCH_PLANKS,
    BLOCK_PEGGED_FIR_PLANKS, BLOCK_PEGGED_PLANKS, BLOCK_PEGGED_SAXAUL_PLANKS, BLOCK_PLANKS,
    BLOCK_SAXAUL_LEAVES, BLOCK_SAXAUL_LOG, BLOCK_SAXAUL_PLANKS, BLOCK_PEGGED_PINE_PLANKS, BLOCK_PEGGED_WILLOW_PLANKS,
    BLOCK_PINE_LOG, BLOCK_PINE_NEEDLES, BLOCK_PINE_PLANKS, BLOCK_WILLOW_LEAVES, BLOCK_WILLOW_LOG, BLOCK_WILLOW_PLANKS,
};

/// One wood's four blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wood {
    pub log: BlockId,
    pub leaves: BlockId,
    pub planks: BlockId,
    pub pegged: BlockId,
}

/// Every wood, the oak first: the oak is the wood a recipe names when it
/// means any wood (see [`stands_in_for`]).
pub const WOODS: [Wood; 6] = [
    Wood { log: BLOCK_LOG, leaves: BLOCK_LEAVES, planks: BLOCK_PLANKS, pegged: BLOCK_PEGGED_PLANKS },
    Wood {
        log: BLOCK_BIRCH_LOG,
        leaves: BLOCK_BIRCH_LEAVES,
        planks: BLOCK_BIRCH_PLANKS,
        pegged: BLOCK_PEGGED_BIRCH_PLANKS,
    },
    Wood { log: BLOCK_FIR_LOG, leaves: BLOCK_FIR_NEEDLES, planks: BLOCK_FIR_PLANKS, pegged: BLOCK_PEGGED_FIR_PLANKS },
    Wood {
        log: BLOCK_SAXAUL_LOG,
        leaves: BLOCK_SAXAUL_LEAVES,
        planks: BLOCK_SAXAUL_PLANKS,
        pegged: BLOCK_PEGGED_SAXAUL_PLANKS,
    },
    Wood {
        log: BLOCK_PINE_LOG,
        leaves: BLOCK_PINE_NEEDLES,
        planks: BLOCK_PINE_PLANKS,
        pegged: BLOCK_PEGGED_PINE_PLANKS,
    },
    Wood {
        log: BLOCK_WILLOW_LOG,
        leaves: BLOCK_WILLOW_LEAVES,
        planks: BLOCK_WILLOW_PLANKS,
        pegged: BLOCK_PEGGED_WILLOW_PLANKS,
    },
];

/// The wood a block is one of the four blocks of, if any.
#[inline]
pub fn wood_of(id: BlockId) -> Option<&'static Wood> {
    let kind = block_kind(id);
    WOODS.iter().find(|w| kind == w.log || kind == w.leaves || kind == w.planks || kind == w.pegged)
}

/// Is this a log of any wood, standing or lying?
#[inline]
pub fn is_log(id: BlockId) -> bool {
    let kind = block_kind(id);
    WOODS.iter().any(|w| w.log == kind)
}

// ---- seasoning ----
//
// **A log off a standing tree is green**, half its weight water, and it
// burns like it: a sullen, smoky fire that boils its own sap before it gives
// any heat. Stacked in the air where the rain does not lie on it, it seasons
// in days (in the real world, a summer). The player asked for the decision
// that follows: cut ahead and stack, or burn it green now.
//
// **Where it is kept.** The two bits a piece of furniture spends on its wood
// (`types::WOOD_LOW_SHIFT`), which a log has never had a use for: the log's
// own variant field is its axis and its moss, both taken. It is a count of
// how green -- [`GREENEST`] off the stump, nought seasoned -- so a stack
// seasons a stage at a time on the world's slow clock (`rot` on the server,
// the same four-a-day steps the larder ages on) with nothing kept per slot.
//
// **Nought is seasoned**, so every log already in a chest, in a save written
// before this, and every `BLOCK_LOG` a recipe or a test names, is dry wood --
// and what makes a log green is being *cut*: `types::block_drop` of a trunk,
// and the spare logs of a felled tree. Deadfall, a bough lying in the wood,
// is dead wood, dry already, and gives a seasoned log.
//
// **A log in the world carries no seasoning.** `types::placed` takes it
// off: a log in a wall is a building, and breaking one out of it gives a
// green log back like any other trunk. The alternative was the bits in the
// world as well, which is every rule about a standing log -- the mesher,
// the felling, the moss -- learning that a trunk has a second field.

/// How green a freshly cut log is: the top of the two bits.
pub const GREENEST: u8 = 3;

/// Where the greenness sits in a log's id: the furniture's wood bits.
pub const GREEN_SHIFT: u32 = crate::types::WOOD_LOW_SHIFT;
/// ...and the field itself.
pub const GREEN_MASK: BlockId = 0b11 << GREEN_SHIFT;

/// How many of the world's four-a-day steps it takes a log in a **log pile**
/// to season by one stage, and [`SEASONS_EVERY_UNDER_A_ROOF`] for one kept
/// under a roof in a chest or a pack.
///
/// **Three days in a pile, six in a chest.** A pile is what firewood is
/// stacked in: air through it on every side, and rain only stopping it on
/// the steps it falls (`logic::rot` on the server skips those). A chest
/// keeps the rain off and the air out, and a log in one dries at half the
/// pace -- which is what makes the pile worth building. Out in the open and
/// not in a pile, a log lying in the grass seasons not at all: it lies in
/// the dew and goes soft at the bottom.
pub const SEASONS_EVERY_IN_A_PILE: u32 = 4;
/// See [`SEASONS_EVERY_IN_A_PILE`].
pub const SEASONS_EVERY_UNDER_A_ROOF: u32 = 8;

/// How green this is, [`GREENEST`] to nought. Nought for anything that is
/// not a log.
#[inline]
pub fn greenness(id: BlockId) -> u8 {
    if !is_log(id) {
        return 0;
    }
    ((id & GREEN_MASK) >> GREEN_SHIFT) as u8
}

/// Is this a log that has not finished seasoning? What burns badly
/// (`hearth::GREEN_HEAT`) -- the whole way, not a third less at each stage:
/// "green until it is seasoned" is a rule a player can hold, where a heat
/// that crept up a stage at a time is a number they cannot see.
#[inline]
pub fn is_green(id: BlockId) -> bool {
    greenness(id) > 0
}

/// The same log, fresh off the stump. Anything else unchanged.
#[inline]
pub fn green(id: BlockId) -> BlockId {
    if !is_log(id) {
        return id;
    }
    (id & !GREEN_MASK) | (BlockId::from(GREENEST) << GREEN_SHIFT)
}

/// The same log, seasoned -- or the same id, for anything else.
#[inline]
pub fn seasoned(id: BlockId) -> BlockId {
    if !is_log(id) {
        return id;
    }
    id & !GREEN_MASK
}

/// One stage further seasoned; a seasoned log stays one.
#[inline]
pub fn season_a_stage(id: BlockId) -> BlockId {
    let g = greenness(id);
    if g == 0 {
        return id;
    }
    (id & !GREEN_MASK) | (BlockId::from(g - 1) << GREEN_SHIFT)
}

/// The word the tooltip puts after a log's name, or `None` for a seasoned
/// one: green off the stump, seasoning on the way.
pub fn seasoning_label(id: BlockId) -> Option<&'static str> {
    match greenness(id) {
        0 => None,
        GREENEST => Some("green"),
        _ => Some("seasoning"),
    }
}

/// Is this a plank of any wood -- not pegged?
#[inline]
pub fn is_planks(id: BlockId) -> bool {
    let kind = block_kind(id);
    WOODS.iter().any(|w| w.planks == kind)
}

/// Is this the plain crown of a wood: oak or birch leaves, fir needles, a
/// saxaul's twigs? Not the apple's, maple's or acacia's, which are an oak's
/// timber under a crown of their own (`types::is_canopy` has all of them).
#[inline]
pub fn is_wood_leaves(id: BlockId) -> bool {
    let kind = block_kind(id);
    WOODS.iter().any(|w| w.leaves == kind)
}

/// The same block in the oak: `fir_planks` to `planks`. Anything that is not
/// one of a wood's four blocks comes back as it was.
#[inline]
pub fn as_oak(id: BlockId) -> BlockId {
    let kind = block_kind(id);
    let Some(wood) = wood_of(kind) else {
        return kind;
    };
    let oak = WOODS[0];
    if kind == wood.log {
        oak.log
    } else if kind == wood.leaves {
        oak.leaves
    } else if kind == wood.planks {
        oak.planks
    } else {
        oak.pegged
    }
}

/// May a recipe that asks for `asked` take `held` in its place?
///
/// **A recipe that names oak means any wood** -- a chest of fir boards is a
/// chest -- unless the thing asked for *is* the oak: the same block, or the
/// oak's form of a different wood's block. That is the whole rule for the
/// ingredient; whether the *row* lets it (a row that turns oak logs into oak
/// planks must not take a fir log) is `crafting`'s question.
///
/// Only the oak stands for the others: a row that names birch planks means
/// birch.
#[inline]
pub fn stands_in_for(asked: BlockId, held: BlockId) -> bool {
    let (asked, held) = (block_kind(asked), block_kind(held));
    asked == held || (wood_of(asked) == Some(&WOODS[0]) && as_oak(held) == asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_green_log_seasons_a_stage_at_a_time_and_stays_the_same_wood() {
        for wood in WOODS {
            let cut = green(wood.log);
            assert!(is_green(cut) && crate::types::is_known_block(cut), "a green {} is invented", crate::types::block_name(wood.log));
            assert_eq!(block_kind(cut), wood.log, "cutting changed the wood");
            let mut log = cut;
            for _ in 0..GREENEST {
                assert!(is_green(log));
                log = season_a_stage(log);
                assert!(crate::types::is_known_block(log));
            }
            assert_eq!(log, wood.log, "three stages did not season it");
            assert_eq!(season_a_stage(log), log, "a seasoned log went further");
        }
        // A plank is never green: it was cut small and dried.
        assert!(!is_green(green(BLOCK_PLANKS)));
        assert_eq!(green(BLOCK_PLANKS), BLOCK_PLANKS);
    }

    #[test]
    fn a_log_cut_off_a_tree_is_green_and_a_bough_off_the_forest_floor_is_not() {
        use crate::types::{block_drop, BLOCK_BOUGH};
        assert!(block_drop(BLOCK_LOG).is_some_and(is_green), "a trunk gave a seasoned log");
        assert!(block_drop(BLOCK_FIR_LOG).is_some_and(is_green));
        if let Some(dead) = block_drop(BLOCK_BOUGH).filter(|&d| is_log(d)) {
            assert!(!is_green(dead), "deadfall came out green");
        }
        // ...and a green log put into a wall is a plain log in the world.
        let laid = crate::types::placed(green(BLOCK_LOG), 0.0, (0, 1, 0));
        assert_eq!(greenness(laid), 0, "a wall of logs carries sap in the world");
    }

    #[test]
    fn every_wood_is_four_different_blocks_and_no_block_is_two_woods() {
        let mut seen = std::collections::HashSet::new();
        for wood in WOODS {
            for id in [wood.log, wood.leaves, wood.planks, wood.pegged] {
                assert!(seen.insert(id), "block {id} is in two woods, or twice in one");
                assert_eq!(wood_of(id), Some(&wood));
            }
        }
    }

    #[test]
    fn every_wood_has_its_own_twig_and_bough_and_its_bough_is_its_own_timber() {
        use crate::types::{block_drop, branch_width, is_bough, is_known_block, is_twig, piece_in, piece_log, BLOCK_STICK};
        let mut pieces = std::collections::HashSet::new();
        for wood in WOODS {
            for width in (2..=12u8).step_by(2) {
                let piece = piece_in(wood.log, width);
                let name = crate::types::block_name(piece);
                assert!(is_known_block(piece), "a {name} {width} wide is an invented id");
                assert_eq!(branch_width(piece), Some(width), "a {name} lost its width");
                assert_eq!(piece_log(piece), Some(wood.log), "a {name} is in another wood's bark");
                assert_eq!(is_twig(piece), width < 8);
                assert_eq!(is_bough(piece), width >= 8);
                let timber = if width < 8 { BLOCK_STICK } else { wood.log };
                assert_eq!(block_drop(piece), Some(timber), "a {name} {width} wide gave the wrong timber");
                pieces.insert(piece);
            }
        }
        assert_eq!(pieces.len(), WOODS.len() * 6, "two woods share a piece");
    }

    #[test]
    fn an_oak_recipe_takes_any_wood_and_a_birch_recipe_takes_only_birch() {
        assert!(stands_in_for(BLOCK_PLANKS, BLOCK_FIR_PLANKS));
        assert!(stands_in_for(BLOCK_LOG, BLOCK_SAXAUL_LOG));
        assert!(stands_in_for(BLOCK_LEAVES, BLOCK_FIR_NEEDLES));
        assert!(!stands_in_for(BLOCK_BIRCH_PLANKS, BLOCK_PLANKS), "birch planks were asked for and oak taken");
        assert!(!stands_in_for(BLOCK_PLANKS, BLOCK_FIR_LOG), "a log stood in for planks");
        assert!(!stands_in_for(BLOCK_PLANKS, BLOCK_PEGGED_PLANKS), "pegged boards stood in for loose ones");
    }
}
