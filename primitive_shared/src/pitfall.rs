//! A pit trap: a hole dug on a game trail, covered with boughs and leaves.
//!
//! ## The decision
//!
//! A deer sees a hunter at twelve blocks and a boar charges one at three
//! (`animals::Species::awareness`), and the whole of big game has been a
//! stalk or a fight. A pit is the third way, and the oldest: dig where the
//! animals walk, lay a cover over the hole (`BLOCK_PIT_COVER`), and the
//! first animal heavy enough to break it ([`breaks_through`]) is at the
//! bottom of a hole it cannot climb out of -- the server's animals step up
//! a block and no more -- waiting for a spear.
//!
//! **And the cover does not know who is standing on it.** A person weighs
//! what a deer weighs, so a player who walks over their own pit is in it,
//! with the fall to pay for; on a server, so is anybody else. The cover
//! wears the forest floor's own picture on purpose (`blocks.toml`): a pit
//! that could be seen would be a pit nothing fell into. So a pit is a
//! decision about **where** -- on the trail to the water, far enough from
//! camp that nobody walks it in the dark -- and about remembering: a cairn
//! beside it is the player's own warning sign (`types::BLOCK_CAIRN`).
//!
//! ## What "gives way" means
//!
//! Only over a hole ([`gives_way`]): a cover laid on solid ground is a heap
//! of leaves, and stays one. The hole is the player's to dig, and **two
//! deep** is what holds a deer -- the rule a player learns from the first
//! boar that walked out of a shallow one.
//!
//! ## Rejected
//!
//! * **Stakes at the bottom.** They exist (`types::BLOCK_STAKE`), and a pit
//!   with stakes in it already kills whatever falls on them by the stakes'
//!   own rule. Nothing new was needed, and nothing was added.
//! * **A pit that only animals fall into.** A trap that knew friend from
//!   prey would be a free kill with no risk, and the risk -- that it is the
//!   same hole for everybody -- is what makes where it is dug a decision.
//! * **Hares and birds breaking it.** A cover strong enough to hold a man
//!   for a step would hold a hare, and a pit full of hares is the snare's
//!   work (`snare`) done badly.

use crate::animals::Species;
use crate::types::{block_kind, BlockId, BLOCK_PIT_COVER};

/// Does this animal break through a pit's cover when it walks over one?
///
/// **Everything the size of a person or bigger**; nothing that is not.
pub fn breaks_through(species: Species) -> bool {
    match species {
        Species::Deer
        | Species::Boar
        | Species::Bear
        | Species::Wolf
        | Species::Sheep
        | Species::Zebra
        | Species::Antelope
        | Species::Lion
        | Species::Horse => true,
        Species::Hare
        | Species::Rat
        | Species::Fowl
        | Species::Gull
        | Species::Fish
        | Species::Cod
        | Species::Trout
        | Species::Pike
        | Species::Herring => false,
    }
}

/// Does a cover over `below` give way under a heavy foot?
///
/// Only a cover, and only with nothing that would hold a body under it.
pub fn gives_way(cover: BlockId, below: BlockId) -> bool {
    block_kind(cover) == BLOCK_PIT_COVER && !crate::types::is_collidable(below)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_AIR, BLOCK_DIRT};

    #[test]
    fn a_cover_over_a_hole_gives_way_and_one_on_the_ground_does_not() {
        assert!(gives_way(BLOCK_PIT_COVER, BLOCK_AIR));
        assert!(!gives_way(BLOCK_PIT_COVER, BLOCK_DIRT));
        assert!(!gives_way(BLOCK_DIRT, BLOCK_AIR), "the ground gave way under a deer");
    }

    #[test]
    fn a_deer_breaks_a_cover_and_a_hare_runs_over_it() {
        assert!(breaks_through(Species::Deer));
        assert!(breaks_through(Species::Boar));
        assert!(!breaks_through(Species::Hare));
        assert!(!breaks_through(Species::Fowl));
    }

    #[test]
    fn nothing_the_server_walks_on_land_is_left_out_of_the_list() {
        // Every species is named in `breaks_through`'s match, so a new one is
        // a compile error there; this says what the answer should be for the
        // walkers a player hunts.
        let heavy = Species::ALL.iter().filter(|s| breaks_through(**s)).count();
        assert!(heavy >= 8, "only {heavy} species break a cover");
    }
}
