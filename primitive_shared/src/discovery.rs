//! What a player has come across, and therefore which recipes they know.
//!
//! ## Why the book is not the whole table
//!
//! The ladder from flint to iron is deep -- clay, a kiln, charcoal, ore, a
//! crucible, a mould, a second metal, a bloomery -- and a game that hides
//! every rung makes the player keep a wiki open, while a game that prints
//! every rung on the first evening turns the ladder into a shopping list.
//! Neither is a decision. What the recipe book shows is what *this*
//! player's hands have been near, so it grows the way the player's
//! knowledge would: you learn what copper is for by carrying copper.
//!
//! ## The rule, and the two rejected
//!
//! A kind of block is **held** once it has been in the player's pack. A
//! recipe is:
//!
//! * **known** when every one of its ingredients has been held -- now or
//!   at any time before;
//! * a **lead** when all but one of them have, and at least one has: the
//!   book lists it with the one ingredient drawn as a question mark;
//! * **hidden** otherwise.
//!
//! *Known only when made* was the first thing rejected. It is the rule a
//! crafting journal usually has, and here it would mean the book can never
//! tell a player anything they did not already know -- the one thing it is
//! for.
//!
//! *Known only when every ingredient has been held* was the second, and
//! it is the more tempting one because it is simpler. It fails at exactly
//! the rung that matters: a player carrying copper and nothing else would
//! never see that bronze exists, because bronze wants tin -- and "go and
//! find a second metal" is the whole of what the bronze age asks. A lead
//! says *there is one more thing, and it is not in your pack*, which is a
//! reason to go somewhere. It does not say what the thing is: that is
//! still the hills' to answer.
//!
//! "All but one", and not "any one": a stick is in thirty recipes, and a
//! player who has picked one up does not want thirty rows of question
//! marks. One missing ingredient is a lead; two missing is a guess.
//!
//! ## Why kinds held, and not recipes known
//!
//! **What is stored is the set of block kinds, never recipe indices.** A
//! recipe's index is its identity on the wire and it moves every time the
//! table grows in the middle -- see the protocol notes from v16 and v27 --
//! so a saved list of indices would silently rewire what a veteran player
//! knows the first time somebody added a recipe. Block ids are already a
//! save-format identifier and do not move. The knowledge is *derived* from
//! what was held, so a new recipe appears in the book of everybody who has
//! already carried its ingredients, which is the truth about them.
//!
//! Crafting needs no rule of its own: to make something you held every
//! ingredient, and what you made lands in your pack and is held too.

use serde::{Deserialize, Serialize};

use crate::crafting::Recipe;
use crate::inventory::Inventory;
use crate::types::{block_kind, is_known_block, BlockId};

/// How much of a recipe a player knows. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knowledge {
    /// Nothing in it has been in their hands, or too little.
    Hidden,
    /// Every ingredient but this one has been held.
    Lead { missing: BlockId },
    /// Every ingredient has been held.
    Known,
}

/// The kinds of block a player has held, kept sorted.
///
/// A sorted `Vec` rather than a set: it is a few hundred ids at the very
/// most, it is written to disk and sent over the wire as it stands, and a
/// binary search over a slice that size is as fast as a hash and has a
/// stable byte order, so two saves of the same knowledge are the same
/// bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovered {
    kinds: Vec<BlockId>,
}

impl Discovered {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds knowledge from a list that came off a disk or a socket.
    ///
    /// Repaired on the way in, because both of those are somewhere a list
    /// can be wrong: state bits are stripped (a lit kiln and a cold one are
    /// one kind of thing to have carried), duplicates go, and an id this
    /// build has no row for is dropped rather than kept -- a block removed
    /// from the game is not knowledge anybody can use, and a hand-edited
    /// file is not a way to learn the iron age.
    pub fn from_kinds(kinds: impl IntoIterator<Item = BlockId>) -> Self {
        let mut kinds: Vec<BlockId> = kinds
            .into_iter()
            .map(block_kind)
            .filter(|&kind| is_known_block(kind))
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        Self { kinds }
    }

    /// Every kind held, in id order.
    pub fn kinds(&self) -> &[BlockId] {
        &self.kinds
    }

    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Whether this kind of block has ever been in the pack.
    pub fn has_held(&self, block: BlockId) -> bool {
        self.kinds.binary_search(&block_kind(block)).is_ok()
    }

    /// Records one block. Answers whether that was news.
    pub fn note(&mut self, block: BlockId) -> bool {
        let kind = block_kind(block);
        if !is_known_block(kind) {
            return false;
        }
        match self.kinds.binary_search(&kind) {
            Ok(_) => false,
            Err(at) => {
                self.kinds.insert(at, kind);
                true
            }
        }
    }

    /// Records everything in a pack. Answers whether any of it was news,
    /// which is what decides whether the client is told.
    pub fn note_inventory(&mut self, inventory: &Inventory) -> bool {
        let mut learned = false;
        for stack in inventory.slots().iter().flatten() {
            learned |= self.note(stack.block);
        }
        learned
    }

    /// How much of `recipe` these hands know.
    pub fn of(&self, recipe: &Recipe) -> Knowledge {
        let mut held = 0usize;
        let mut missing: Option<BlockId> = None;
        let mut missing_kinds = 0usize;
        // By kind, and once per kind: a recipe that lists the same thing
        // twice is not two things to find.
        let mut seen: Vec<BlockId> = Vec::with_capacity(recipe.inputs.len());
        for &(block, _) in recipe.inputs {
            let kind = block_kind(block);
            if seen.contains(&kind) {
                continue;
            }
            seen.push(kind);
            if self.has_held(kind) {
                held += 1;
            } else {
                missing_kinds += 1;
                missing = Some(kind);
            }
        }
        match (missing_kinds, missing) {
            (0, _) if held > 0 => Knowledge::Known,
            (1, Some(missing)) if held > 0 => Knowledge::Lead { missing },
            _ => Knowledge::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crafting::RECIPES;
    use crate::inventory::Inventory;
    use crate::types::{
        BLOCK_BRONZE_INGOT, BLOCK_COPPER_INGOT, BLOCK_KILN_LIT, BLOCK_LOG,
        BLOCK_PLANKS, BLOCK_STICK, BLOCK_TIN_INGOT,
    };

    fn recipe_making(output: BlockId, inputs: &[BlockId]) -> &'static Recipe {
        RECIPES
            .iter()
            .find(|recipe| {
                recipe.output.0 == output
                    && inputs
                        .iter()
                        .all(|wanted| recipe.inputs.iter().any(|&(block, _)| block == *wanted))
            })
            .unwrap_or_else(|| panic!("no recipe makes {output} from {inputs:?}"))
    }

    #[test]
    fn a_recipe_is_hidden_until_something_in_it_has_been_held() {
        let planks = recipe_making(BLOCK_PLANKS, &[BLOCK_LOG]);
        let mut hands = Discovered::new();
        assert_eq!(hands.of(planks), Knowledge::Hidden, "an empty-handed player read a recipe");

        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 1);
        assert!(hands.note_inventory(&pack), "picking up a log was not news");
        assert_eq!(hands.of(planks), Knowledge::Known);
    }

    #[test]
    fn carrying_copper_alone_says_bronze_wants_one_more_thing_and_not_what() {
        // The bronze age in one assertion: the lead is the reason to go to
        // the hills, and naming tin would be the wiki again.
        let bronze = recipe_making(BLOCK_BRONZE_INGOT, &[BLOCK_COPPER_INGOT, BLOCK_TIN_INGOT]);
        let mut hands = Discovered::new();
        hands.note(BLOCK_COPPER_INGOT);
        let other_kinds = bronze
            .inputs
            .iter()
            .map(|&(block, _)| block_kind(block))
            .filter(|&kind| kind != BLOCK_COPPER_INGOT && kind != BLOCK_TIN_INGOT)
            .collect::<Vec<_>>();
        for kind in other_kinds {
            hands.note(kind);
        }
        assert_eq!(hands.of(bronze), Knowledge::Lead { missing: BLOCK_TIN_INGOT });

        hands.note(BLOCK_TIN_INGOT);
        assert_eq!(hands.of(bronze), Knowledge::Known);
    }

    #[test]
    fn two_things_missing_is_a_guess_and_not_a_lead() {
        // Otherwise one stick in the pack puts thirty question marks in
        // the book.
        let mut hands = Discovered::new();
        hands.note(BLOCK_STICK);
        let listed = RECIPES
            .iter()
            .filter(|recipe| !matches!(hands.of(recipe), Knowledge::Hidden))
            .count();
        let with_a_stick = RECIPES
            .iter()
            .filter(|recipe| recipe.inputs.iter().any(|&(block, _)| block == BLOCK_STICK))
            .count();
        assert!(
            listed < with_a_stick,
            "a single stick listed {listed} of the {with_a_stick} recipes that use one"
        );
    }

    #[test]
    fn knowledge_outlasts_the_pack_that_taught_it() {
        let planks = recipe_making(BLOCK_PLANKS, &[BLOCK_LOG]);
        let mut hands = Discovered::new();
        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 1);
        hands.note_inventory(&pack);
        pack.take_exact(BLOCK_LOG, 1);
        assert!(!hands.note_inventory(&pack), "an emptier pack taught something");
        assert_eq!(hands.of(planks), Knowledge::Known, "burning the log forgot the recipe");
    }

    #[test]
    fn a_log_lying_down_and_one_standing_are_one_thing_to_have_carried() {
        // The orientation lives in the id's variant bits, and a book that
        // taught planks twice -- once per way the trunk fell -- would be
        // counting states rather than things.
        let lying = BLOCK_LOG | crate::types::ORIENTATION_MASK;
        let mut hands = Discovered::new();
        assert!(hands.note(lying));
        assert!(hands.has_held(BLOCK_LOG));
        assert!(!hands.note(BLOCK_LOG), "the same kind was news twice");
    }

    #[test]
    fn a_list_off_a_disk_comes_back_sorted_single_and_real() {
        let unknown: BlockId = crate::types::KIND_MASK;
        let lying = BLOCK_LOG | crate::types::ORIENTATION_MASK;
        let hands = Discovered::from_kinds([BLOCK_STICK, lying, BLOCK_STICK, unknown, BLOCK_KILN_LIT]);
        let mut expected = vec![BLOCK_STICK, BLOCK_LOG, BLOCK_KILN_LIT];
        expected.sort_unstable();
        assert_eq!(hands.kinds(), expected.as_slice());
    }

    #[test]
    fn every_recipe_in_the_game_can_become_known() {
        // A recipe with an ingredient nobody can hold is a row the book
        // can never show, which is a hole in the ladder rather than in the
        // book.
        let everything = Discovered::from_kinds(
            RECIPES.iter().flat_map(|recipe| recipe.inputs.iter().map(|&(block, _)| block)),
        );
        for recipe in RECIPES {
            assert_eq!(everything.of(recipe), Knowledge::Known, "{} can never be known", recipe.name);
        }
    }
}
