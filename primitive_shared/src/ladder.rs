//! **The ladder, as the player can see it.** Seven ages from bare hands to
//! iron, which of them these hands have climbed, what the next one wants,
//! and where in the world those things are.
//!
//! ## Why this exists at all
//!
//! The player's words were "не понятно развитие абсолютно" -- the
//! progression is completely unclear. Everything needed to see it was
//! already in the game and none of it was ever put in one place: the recipe
//! book lists four hundred rows in table order, the crafting grid answers
//! only "now", and `progression` walks the whole ladder in a *test*, where
//! nobody playing will ever read it. A player could hold copper and have no
//! way to learn that copper is the fifth of seven rungs rather than the end.
//!
//! ## What a rung is, and what it is not
//!
//! A rung is an **age**, not a recipe and not a quest. It names one thing
//! that *is* that age -- a knapped flake, a lit fire, a fired pot, a copper
//! ingot -- and the handful of things that age wants, and the kind of place
//! they come from. It has no marker, no arrow and no reward: what it adds
//! is a page that says "there is a rung above this one and it wants tin",
//! which is the sentence the hills are the answer to.
//!
//! ## How a step counts as taken, and the two rules rejected
//!
//! **A rung is taken when one of the things that *is* that age has been in
//! the player's pack** -- `discovery::Discovered::has_held`, the same
//! knowledge the recipe book is built out of. Making the thing puts it in
//! the pack, so making it takes the rung; and so does finding one in a
//! ruin, or being handed one by a friend on a server, which is honest: a
//! player holding a copper ingot *is* in the copper age however it got
//! there, and a page that told them otherwise would be arguing with what is
//! in their hands.
//!
//! *Counting recipes crafted* was the first rejected. It is the obvious
//! rule and it needs a second thing saved -- a set of recipe indices -- and
//! `discovery`'s module note says at length why indices are the one thing
//! this game must not write down: they move whenever the table grows in the
//! middle, so a veteran's ladder would quietly re-rung itself on an update.
//! Kinds held are already saved, already sent, and already repaired on the
//! way in.
//!
//! *Counting a rung taken when its ingredients are held* was the second,
//! and it is wrong in the direction that matters: a player carrying copper
//! ore and charcoal has not smelted anything, and a page that congratulated
//! them for the copper age would be the page that made the ladder unclear
//! in the first place.
//!
//! ## The first three things
//!
//! [`first_step`] is the other end of the same ladder: what a player who
//! has *just* woken up might do in the next two minutes. Three facts, in
//! the order the world offers them -- a stone off the ground, fibre out of
//! the grass, a flake off a flint. It is driven by the same `Discovered`,
//! so it cannot disagree with the page, and a player who was handed a
//! flake by a friend is not told to go and knap one.
//!
//! **Where those three are shown changed.** All three used to go over the
//! belt, one after another, phrased as orders, and they read as
//! condescending. They live on the pack's path page now
//! (`ui::ladder_screen`); over the belt there is one line, for a player
//! who has held nothing *and* is carrying nothing, and it is a statement
//! about the world rather than an instruction. See
//! `ui::journal::Journal::first_minute`.

use crate::discovery::Discovered;
use crate::types::*;

/// One age of the world, in the order they are climbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Age {
    /// Nothing but hands: a stone, a stick, a tuft of grass.
    BareHands,
    /// Struck stone: flakes, and the first edge.
    Flint,
    /// A fire that stays lit, and everything cooked on it.
    Fire,
    /// Clay dug, thrown and fired: the pot that metal is melted in.
    Clay,
    /// The first metal.
    Copper,
    /// The second metal, and the alloy that wants both.
    Bronze,
    /// Iron, reduced rather than melted.
    Iron,
}

/// The kind of place a rung's materials come from.
///
/// A *kind* of place and never a coordinate: "clay by the river" is
/// knowledge a player can act on anywhere in any world, and an arrow to the
/// nearest clay bank would be the compass this game deliberately removed
/// (see the note at the foot of `ui::journal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Found {
    /// Underfoot, in any meadow: stones, sticks, grass.
    Underfoot,
    /// Gravel and riverbanks, where the water sorts the stone.
    Riverbank,
    /// The hills, where the rock is bare and the ore is in it.
    Hills,
    /// Deep rock, down a shaft, below where the light goes.
    DeepRock,
    /// Standing timber.
    Woods,
    /// **A country of its own, far from home**: tin, which is in districts
    /// none of which is within a few hundred blocks of where a player wakes
    /// up (`worldgen`'s `far_tin_country`). Its own place and not the
    /// riverbank the stream tin lies on, because "by the river" sent a
    /// player to the river beside their camp -- and in a world with landforms
    /// that river has no tin in it. What the page can honestly say is that
    /// the bronze age is a journey.
    FarCountry,
}

/// One rung of the ladder.
pub struct Rung {
    pub age: Age,
    /// The things that **are** this age. Holding any one of them takes
    /// the rung. See the module note.
    pub marks: &'static [BlockId],
    /// The handful of things this rung wants, in the order a player gets
    /// them. Not the full recipe -- the book has that -- but what to go
    /// and fetch.
    pub wants: &'static [BlockId],
    /// Where those things come from.
    pub found: Found,
}

/// The whole ladder, bottom to top.
///
/// Seven rungs and not the dozen the walk in `progression` has: the
/// workshops, the furniture, the farm and the tannery are all *beside* the
/// ladder rather than on it -- a player can reach iron without ever
/// building a potter's wheel -- and a page with twelve rows on it is a
/// checklist, which is the chore this codebase refuses. What is on the page
/// is the spine: the things you cannot reach iron without.
pub const LADDER: [Rung; 7] = [
    Rung {
        age: Age::BareHands,
        // A stone, a stick, a tuft. Any of the three is a player who has
        // picked something up, which is the whole of this rung.
        marks: &[BLOCK_PEBBLE, BLOCK_STICK, BLOCK_FIBER],
        wants: &[BLOCK_PEBBLE, BLOCK_STICK, BLOCK_FIBER],
        found: Found::Underfoot,
    },
    Rung {
        age: Age::Flint,
        // The flake, and the edge it becomes: a player who found a knife
        // in a ruin has the flint age whether or not they knapped it.
        marks: &[BLOCK_FLINT_FLAKE, BLOCK_FLINT_KNIFE],
        wants: &[BLOCK_FLINT, BLOCK_STICK, BLOCK_FIBER],
        found: Found::Riverbank,
    },
    Rung {
        age: Age::Fire,
        // **The firepit too**, which is the fire a player makes *first*:
        // three sticks and a log on the ground, struck with flint, needing
        // no cobblestone -- and cobblestone wants a pick the flint age has
        // not got yet. Nothing about a firepit ever passes through the pack,
        // so the server notes it at the strike (`strike_firepit`); without
        // that, a player sitting by the fire they lit was told by this page
        // that they were still in the flint age.
        marks: &[BLOCK_CAMPFIRE, BLOCK_COAL, BLOCK_FIREPIT],
        // **What the firepit wants, not what the campfire does.** This
        // listed cobblestone, stick and log -- the campfire's recipe -- and
        // so sent a player with a flint knife off to find a stone they
        // cannot break yet, for a fire they could already have lit. The
        // real first fire is sticks and a log dropped on the ground and
        // struck with flint; the page says how to lay it under the list
        // (`ladder_screen::how_to_lay`).
        wants: &[BLOCK_STICK, BLOCK_LOG, BLOCK_FLINT],
        found: Found::Woods,
    },
    Rung {
        age: Age::Clay,
        // The kiln is the rung; the pot and the mould are what it is for,
        // and a player who has one of those has fired something.
        marks: &[BLOCK_KILN, BLOCK_VESSEL, BLOCK_MOULD],
        wants: &[BLOCK_CLAY, BLOCK_COBBLESTONE, BLOCK_SAND],
        found: Found::Riverbank,
    },
    Rung {
        age: Age::Copper,
        marks: &[BLOCK_COPPER_INGOT, BLOCK_NATIVE_COPPER],
        wants: &[BLOCK_COPPER_ORE, BLOCK_COAL, BLOCK_VESSEL, BLOCK_MOULD],
        found: Found::Hills,
    },
    Rung {
        age: Age::Bronze,
        marks: &[BLOCK_BRONZE_INGOT],
        // Tin is the whole of this rung, and it is the one thing the book
        // will not name for a player who has never held it (see
        // `discovery`'s note on leads). Here it *is* named, because this
        // page is the knowledge a player has worked out about the world
        // rather than a hint about what is in their pack -- and a page
        // whose fifth row said "one more thing, and we will not say what"
        // would be the wiki-in-another-tab the book exists to avoid.
        wants: &[BLOCK_TIN_ORE, BLOCK_COPPER_INGOT, BLOCK_VESSEL, BLOCK_MOULD],
        found: Found::FarCountry,
    },
    Rung {
        age: Age::Iron,
        marks: &[BLOCK_IRON_BLOOM, BLOCK_IRON_INGOT],
        wants: &[BLOCK_IRON_ORE, BLOCK_COAL, BLOCK_BLOOMERY],
        found: Found::DeepRock,
    },
];

impl Rung {
    /// Has this player climbed this rung? See the module note.
    pub fn taken(&self, held: &Discovered) -> bool {
        self.marks.iter().any(|&mark| held.has_held(mark))
    }

    /// Of what this rung wants, what has never been in the pack.
    ///
    /// The other half of the page: a rung that says "copper ore, charcoal,
    /// a pot and a mould" to a player who already has three of the four is
    /// a rung that has told them nothing.
    pub fn still_to_find<'a>(&'a self, held: &'a Discovered) -> impl Iterator<Item = BlockId> + 'a {
        // `has_held` and not a copy of the list: it is a binary search over
        // the sorted slice the knowledge already is, and this is called
        // once a frame while the page is open.
        self.wants.iter().copied().filter(move |&want| !held.has_held(want))
    }
}

/// The rung this player is standing on: the highest one taken.
///
/// `None` for a player who has held nothing at all, which is the first
/// two minutes and is [`first_step`]'s to answer.
///
/// **The highest and not the last unbroken one.** A player given a copper
/// ingot without ever lighting a fire is in the copper age, and a page that
/// insisted they were still in the stone age because of a gap three rungs
/// below would be arguing with their pack.
pub fn standing_on(held: &Discovered) -> Option<&'static Rung> {
    LADDER.iter().rev().find(|rung| rung.taken(held))
}

/// What this player is working towards: the rung **above the one they are
/// standing on**, or the bottom rung for somebody who has held nothing.
///
/// `None` only for a player standing on the top rung.
///
/// **Above where they stand, and not the lowest gap.** The lowest gap was
/// the first thing written and it is wrong in the one case the page exists
/// for: a player who was handed a copper ingot, or who smelted copper over
/// a fire somebody else built, has a hole at "fire" -- and being told, at
/// forty hours in, to go and light a campfire is the page failing at
/// exactly its job. The gaps below are still drawn as gaps in the list;
/// what they are not is the thing to do next.
pub fn working_towards(held: &Discovered) -> Option<&'static Rung> {
    match standing_on(held) {
        Some(here) => {
            let at = LADDER.iter().position(|rung| rung.age == here.age)?;
            LADDER.get(at + 1)
        }
        None => LADDER.first(),
    }
}

/// One of the three things a player should do in their first two minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirstStep {
    /// Pick a stone off the ground.
    Stone,
    /// Tear a tuft of grass for fibre.
    Fibre,
    /// Strike a flint and take a flake off it.
    Flake,
}

/// The first three steps, in order.
pub const FIRST_STEPS: [FirstStep; 3] = [FirstStep::Stone, FirstStep::Fibre, FirstStep::Flake];

impl FirstStep {
    /// The thing that having done it puts in the pack.
    pub fn done_when_held(self) -> BlockId {
        match self {
            FirstStep::Stone => BLOCK_PEBBLE,
            FirstStep::Fibre => BLOCK_FIBER,
            FirstStep::Flake => BLOCK_FLINT_FLAKE,
        }
    }
}

/// What to mention, for a player who has not met all three yet.
///
/// **One at a time, and the first one not met.** Three lines at once is a
/// tutorial, which the player asked for the opposite of. Out of order is
/// fine and deliberate -- a player who starts by tearing grass is simply
/// shown the stone instead, and one who was handed a flake is shown
/// nothing at all, because this is about what is *missing* and not about
/// obedience.
///
/// The caller decides whether this is worth saying at all: the path page
/// asks it only below the flint rung, and the line over the belt asks a
/// stricter question of its own.
pub fn first_step(held: &Discovered) -> Option<FirstStep> {
    FIRST_STEPS.into_iter().find(|step| !held.has_held(step.done_when_held()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crafting::{craft, Attempt, Heat, RECIPES};
    use crate::inventory::Inventory;

    fn holding(kinds: &[BlockId]) -> Discovered {
        Discovered::from_kinds(kinds.iter().copied())
    }

    #[test]
    fn an_empty_handed_player_stands_on_no_rung_and_is_shown_the_first_thing_to_do() {
        let nobody = Discovered::new();
        assert_eq!(standing_on(&nobody).map(|r| r.age), None);
        assert_eq!(working_towards(&nobody).map(|r| r.age), Some(Age::BareHands));
        assert_eq!(first_step(&nobody), Some(FirstStep::Stone));
    }

    /// **The rung is marked when the thing is made**, through the real
    /// craft path and the real knowledge: this is the promise the page
    /// makes, and the one it would be worst to get wrong.
    #[test]
    fn knapping_a_flake_marks_the_flint_age_as_taken() {
        let flakes = RECIPES.iter().find(|r| r.name == "flint flakes").expect("knapping");
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT, 1);
        let mut held = Discovered::new();
        held.note_inventory(&pack);
        let flint = LADDER.iter().find(|r| r.age == Age::Flint).expect("the flint age");
        assert!(!flint.taken(&held), "holding a flint is not having knapped it");

        assert!(craft(&mut pack, flakes, Heat::NONE, Attempt::Succeeds).is_made());
        held.note_inventory(&pack);
        assert!(flint.taken(&held), "a knapped flake did not mark the flint age");
        assert_eq!(standing_on(&held).map(|r| r.age), Some(Age::Flint));
    }

    /// A rung that fails silently the moment its marks go out of the game
    /// is a page that quietly stops working, so the marks are checked to
    /// be real blocks -- and a rung with no marks could never be taken.
    #[test]
    fn every_rung_is_marked_by_real_things_and_wants_real_things() {
        for rung in &LADDER {
            assert!(!rung.marks.is_empty(), "{:?} can never be taken", rung.age);
            assert!(!rung.wants.is_empty(), "{:?} asks for nothing", rung.age);
            for &block in rung.marks.iter().chain(rung.wants) {
                assert!(is_known_block(block_kind(block)), "{:?} names {block}, which is not a block", rung.age);
            }
        }
    }

    /// Two rungs sharing a mark would light up together, and the page
    /// would skip an age the player never reached.
    #[test]
    fn the_fire_rung_asks_for_a_firepit_and_not_for_stone_a_flint_age_player_cannot_break() {
        let fire = LADDER.iter().find(|rung| rung.age == Age::Fire).unwrap();
        assert!(
            !fire.wants.contains(&BLOCK_COBBLESTONE),
            "the fire rung wants cobblestone, which needs a pick the flint age has not got"
        );
        for want in [BLOCK_STICK, BLOCK_LOG, BLOCK_FLINT] {
            assert!(fire.wants.contains(&want), "the firepit's {want} is missing from the fire rung");
        }
    }

    #[test]
    fn no_two_rungs_are_marked_by_the_same_thing() {
        for (a, rung) in LADDER.iter().enumerate() {
            for other in &LADDER[a + 1..] {
                for &mark in rung.marks {
                    assert!(
                        !other.marks.contains(&mark),
                        "{} marks both {:?} and {:?}",
                        block_name(mark),
                        rung.age,
                        other.age,
                    );
                }
            }
        }
    }

    /// The whole point of the page in one assertion: a copper-age player
    /// is told there is more above them, and what it wants.
    #[test]
    fn a_player_holding_copper_is_told_bronze_is_next_and_that_it_wants_tin() {
        let held = holding(&[BLOCK_PEBBLE, BLOCK_FLINT_FLAKE, BLOCK_CAMPFIRE, BLOCK_KILN, BLOCK_COPPER_INGOT]);
        assert_eq!(standing_on(&held).map(|r| r.age), Some(Age::Copper));
        let next = working_towards(&held).expect("bronze is above copper");
        assert_eq!(next.age, Age::Bronze);
        assert_eq!(next.found, Found::FarCountry, "the page does not say tin is a journey");
        let to_find: Vec<BlockId> = next.still_to_find(&held).collect();
        assert!(to_find.contains(&BLOCK_TIN_ORE), "the bronze rung did not say what it is short of");
        assert!(!to_find.contains(&BLOCK_COPPER_INGOT), "a thing already held was listed as still to find");
    }

    #[test]
    fn a_player_with_iron_in_the_pack_has_no_rung_left_to_climb() {
        let held = holding(&[BLOCK_IRON_INGOT]);
        assert_eq!(standing_on(&held).map(|r| r.age), Some(Age::Iron));
        // **And not sent back down to the meadow for the fire they never
        // lit.** A player handed iron has gaps under them, and the page
        // draws those gaps; what it must not do is call one of them the
        // thing to do next.
        assert!(working_towards(&held).is_none(), "a smith was told to go and pick up a stone");
    }

    /// The prompts step forward as the player does them, and then stop --
    /// a line that never goes away is a line a player learns to look past.
    #[test]
    fn the_first_three_prompts_step_forward_and_then_stop_for_good() {
        let mut held = Discovered::new();
        for step in FIRST_STEPS {
            assert_eq!(first_step(&held), Some(step), "the prompts came out of order");
            held.note(step.done_when_held());
        }
        assert_eq!(first_step(&held), None, "the game was still telling a knapper to pick up a stone");
    }

    /// Doing them out of order shows the one still undone, not the next
    /// in the list: the world does not deal them out in order either.
    #[test]
    fn a_player_who_tears_grass_first_is_still_shown_the_stone() {
        let held = holding(&[BLOCK_FIBER]);
        assert_eq!(first_step(&held), Some(FirstStep::Stone));
        assert_eq!(first_step(&holding(&[BLOCK_FIBER, BLOCK_PEBBLE])), Some(FirstStep::Flake));
    }
}
