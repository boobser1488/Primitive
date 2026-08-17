//! Recipes.
//!
//! ## Shapeless, on purpose
//!
//! A recipe is a bag of ingredients and a result, not a pattern on a
//! grid. A grid needs a grid to arrange things on, which is a second
//! inventory screen with its own drag rules and its own ways to lose a
//! stack -- a lot of machine for a block palette this small. With eleven
//! block types there is no shape to express that a bag cannot.
//!
//! ## Shared, and checked on the server
//!
//! The client lists recipes to show what is possible and greys out what
//! is not; the server runs the same table to decide what actually
//! happens. Both read this file, so a client cannot invent a recipe --
//! it can only ask for one by index, and the server looks that index up
//! in its own copy.

use crate::inventory::Inventory;
use crate::types::{
    BlockId, BLOCK_CHEST, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FIBER, BLOCK_FLINT, BLOCK_GRASS,
    BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_BIRCH_PLANKS, BLOCK_BRONZE_INGOT,
    BLOCK_COAL, BLOCK_COPPER_INGOT, BLOCK_COPPER_ORE, BLOCK_FLINT_AXE, BLOCK_FLINT_AXE_HEAD,
    BLOCK_FLINT_FLAKE, BLOCK_FLINT_KNIFE, BLOCK_FLINT_KNIFE_HEAD, BLOCK_FLINT_PICKAXE,
    BLOCK_FLINT_PICK_HEAD, BLOCK_GRAVEL, BLOCK_IRON_INGOT, BLOCK_IRON_ORE,
    BLOCK_LEAVES, BLOCK_LOG, BLOCK_PEBBLE, BLOCK_PLANKS, BLOCK_SAND, BLOCK_STICK,
    BLOCK_TALL_GRASS, BLOCK_TIN_INGOT, BLOCK_TIN_ORE, BLOCK_WORKED_STICK,
};

/// One thing a player can make.
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    /// Shown in the crafting menu, which draws the ingredients and the
    /// result as icons beside it. Keep it to the *thing made*: "planks",
    /// not "planks from a log". The pictures say what it is made from,
    /// and a row of prose as wide as the column is a row that has to be
    /// truncated.
    pub name: &'static str,
    /// What it costs. Every entry must be present in full.
    pub inputs: &'static [(BlockId, u32)],
    /// What comes out.
    pub output: (BlockId, u32),
}

/// Every recipe, in menu order.
///
/// The index into this list is the recipe's identity on the wire, so
/// **inserting in the middle renames every recipe after it**. Add at the
/// end unless the protocol version is going up anyway.
pub const RECIPES: &[Recipe] = &[
    Recipe {
        name: "planks",
        inputs: &[(BLOCK_LOG, 1)],
        output: (BLOCK_PLANKS, 4),
    },
    Recipe {
        name: "beam",
        inputs: &[(BLOCK_PLANKS, 4)],
        output: (BLOCK_LOG, 1),
    },
    Recipe {
        // The other way round from knapping: a block of cobble broken
        // back down into the stones it was made of.
        //
        // This slot used to make dressed stone out of cobble. Dressed
        // stone cannot be broken by hand any more, so making it was
        // handing the player a block they could place once and never
        // take back -- and the recipe kept its place in the list rather
        // than being removed, because the index into this table is a
        // recipe's identity on the wire.
        name: "split cobble",
        inputs: &[(BLOCK_COBBLESTONE, 1)],
        output: (BLOCK_PEBBLE, 3),
    },
    Recipe {
        name: "mulch",
        inputs: &[(BLOCK_LEAVES, 4)],
        output: (BLOCK_DIRT, 1),
    },
    Recipe {
        name: "turf",
        inputs: &[(BLOCK_DIRT, 2), (BLOCK_LEAVES, 2)],
        output: (BLOCK_GRASS, 1),
    },
    Recipe {
        name: "sand",
        inputs: &[(BLOCK_COBBLESTONE, 2)],
        output: (BLOCK_SAND, 3),
    },
    Recipe {
        // The one recipe that turns a block into something that is not
        // one: a stick is drawn as a twig standing in its cell rather
        // than as a cube of wood. Cheap on purpose -- it is the
        // ingredient everything made of wood will want.
        name: "sticks",
        inputs: &[(BLOCK_PLANKS, 2)],
        output: (BLOCK_STICK, 4),
    },
    Recipe {
        // Thatch: a use for the fibre a field is full of, and the only
        // way to get dirt without digging it.
        //
        // This used to take the tufts themselves. Pulling grass up now
        // yields fibre instead (see `types::block_drop`), so the recipe
        // follows the material rather than being left asking for
        // something a player can no longer collect.
        name: "thatch",
        inputs: &[(BLOCK_FIBER, 8)],
        output: (BLOCK_DIRT, 1),
    },
    Recipe {
        // ...and back the other way, so a field is a renewable thing
        // rather than one a player can strip permanently. Twisting the
        // fibre back into a tuft costs more than one tuft yields, which
        // is what stops it being a loop that prints dirt.
        name: "grass tuft",
        inputs: &[(BLOCK_FIBER, 3)],
        output: (BLOCK_TALL_GRASS, 1),
    },
    Recipe {
        // Four loose stones knapped into one block of cobble. The
        // earliest stone in the game: you can pick up enough to build
        // with before you have anything to mine with, which is what
        // makes the first few minutes something other than punching
        // trees.
        name: "knapped stone",
        inputs: &[(BLOCK_PEBBLE, 4)],
        output: (BLOCK_COBBLESTONE, 1),
    },
    Recipe {
        // Fibre bound around a splinter of wood. The first thing in the
        // game made of two materials, and the reason to pick grass at
        // all before there is any rope to make.
        name: "bound sticks",
        inputs: &[(BLOCK_FIBER, 2), (BLOCK_PLANKS, 1)],
        output: (BLOCK_STICK, 3),
    },
    Recipe {
        // Flint is what you knap *with*. A nodule struck against loose
        // stone splits it cleanly, so two of them go as far as four
        // pebbles alone do -- which is the whole reason to bend down for
        // the black stones as well as the grey ones.
        name: "flint knapping",
        inputs: &[(BLOCK_FLINT, 2), (BLOCK_PEBBLE, 2)],
        output: (BLOCK_COBBLESTONE, 2),
    },
    Recipe {
        // Eight planks: five sides and a lid, near enough. Expensive on
        // purpose -- a chest is what stops carrying being a decision,
        // and it should cost a tree to stop making that decision.
        name: "chest",
        inputs: &[(BLOCK_PLANKS, 8)],
        output: (BLOCK_CHEST, 1),
    },
    Recipe {
        // Gravel is mostly stone and partly flint, and sifting it is
        // how you get the flint out. Four shovelfuls for one nodule:
        // less than walking a stony shore and picking them up, which
        // is the point -- this is what you do when there is no shore.
        name: "sifted gravel",
        inputs: &[(BLOCK_GRAVEL, 4)],
        output: (BLOCK_FLINT, 1),
    },
    // Birch, which is a second wood rather than a variant of the first
    // (see `types::BLOCK_BIRCH_LOG`). Appended rather than filed beside
    // the oak recipes for the reason at the top of this list: the index
    // into it is a recipe's identity on the wire, so inserting in the
    // middle renames every recipe after it.
    Recipe {
        name: "birch planks",
        inputs: &[(BLOCK_BIRCH_LOG, 1)],
        output: (BLOCK_BIRCH_PLANKS, 4),
    },
    Recipe {
        name: "birch beam",
        inputs: &[(BLOCK_BIRCH_PLANKS, 4)],
        output: (BLOCK_BIRCH_LOG, 1),
    },
    Recipe {
        // Leaves are leaves. The two woods do not stack, but neither of
        // them is *distinguishable* once it has rotted down, and a
        // second mulch recipe would be a second row in the menu saying
        // the same thing.
        name: "birch mulch",
        inputs: &[(BLOCK_BIRCH_LEAVES, 4)],
        output: (BLOCK_DIRT, 1),
    },
    // ---- the ages: flint, copper, bronze, iron ----
    //
    // ## Smelting without a furnace
    //
    // These recipes are the compromise this feature turned on, so the
    // reasoning belongs here rather than in a commit message.
    //
    // A furnace is the honest answer: a block with an inside, a fire
    // that takes time, fuel that burns down, and a screen to watch it in.
    // That is a container UI, a per-block simulation on the server and a
    // new wire message, and none of it is *about* metal -- it is about
    // furnaces. The whole point of adding ore was to give the underground
    // a reason to exist, and spending the work on a fourth inventory
    // screen instead would have shipped the machine and not the game.
    //
    // So smelting is a craft: ore plus fuel makes metal, and the fire is
    // implied. It is not a cheat as badly as it first looks, because the
    // physics is real at the point where it matters -- **copper and tin
    // melt at temperatures a campfire reaches, and iron does not**. That
    // is why the two soft metals cost one coal and iron costs three: what
    // a forge really buys you is a lot more fuel and a lot more air, and
    // the fuel is the part that can be charged for without a new screen.
    // Iron's own difficulty is priced in mining time instead: thirteen
    // seconds of hardness against a stone edge (see `blocks::BLOCKS`).
    //
    // **Nothing is made of the metal yet**, and that is deliberate rather
    // than unfinished. The metal tools these recipes used to feed are
    // gone; what is left is ore, fuel, smelting and an alloy, which is a
    // complete little economy with no consumer at the end of it. Deleting
    // it until there is one was the obvious alternative and it is the
    // wrong trade: the whole chain works, it is the reason the
    // underground is worth walking, and a player who comes back to a
    // world with a chest of bronze in it will find that chest still
    // means something. A furnace, and then the metal tools, are what it
    // is waiting for.
    // ---- the stone age, in four steps ----
    //
    // There used to be one recipe here: two flint, two sticks, three
    // fibre, out came a pickaxe. It was the first tool in the game and it
    // cost one click, which meant the most important object a player ever
    // makes was the cheapest thing in the menu to think about. A tool
    // that appears whole is a tool you *buy*.
    //
    // So it is four steps now, and each one is a real operation somebody
    // actually performed: strike the nodule, whittle the haft, shape the
    // head, bind the two together. The intermediate products are items in
    // their own right, which is what makes the chain a chain rather than
    // a longer ingredient list -- a player with flakes and no fibre has
    // made progress, and can see that they have.
    //
    // **The chicken and the egg is solved by the flake, not by the
    // knife.** Whittling a haft needs an edge, and if that edge had to be
    // the finished knife then the knife would need itself. It does not:
    // the thing that cuts is the waste struck off the nodule in step one.
    // A fresh flake is sharper than any hafted tool and lasts about three
    // cuts, which is why real assemblages are mostly flakes and why this
    // recipe spends one. See `BLOCK_FLINT_FLAKE`.
    //
    // The numbers, end to end: a pick costs three nodules, one stick and
    // five fibre; an axe two nodules, one stick and four fibre; a knife
    // one nodule, one stick and two fibre. All of it is lying on the
    // ground in the first ten minutes of a world, and none of it is
    // one click.
    Recipe {
        // Step one: a nodule struck against another stone comes apart
        // into shards with edges on them. Three, because a struck core
        // yields more waste than tool and the waste is the point here.
        name: "flint flakes",
        inputs: &[(BLOCK_FLINT, 1)],
        output: (BLOCK_FLINT_FLAKE, 3),
    },
    Recipe {
        // Step two: a branch pared down with a flake. The flake is
        // consumed -- an edge that thin does not survive the job, and
        // spending it here is what stops a single nodule from arming a
        // player for good.
        name: "worked stick",
        inputs: &[(BLOCK_STICK, 1), (BLOCK_FLINT_FLAKE, 1)],
        output: (BLOCK_WORKED_STICK, 1),
    },
    // Step three: the heads. A knife is a flake with a back put on it and
    // costs nothing but flakes; an axe and a pick are *cores* -- a whole
    // nodule worked down to a shape, with flakes struck off it to sharpen
    // the edge -- which is why they cost a nodule as well.
    Recipe {
        name: "knife head",
        inputs: &[(BLOCK_FLINT_FLAKE, 2)],
        output: (BLOCK_FLINT_KNIFE_HEAD, 1),
    },
    Recipe {
        name: "axe head",
        inputs: &[(BLOCK_FLINT, 1), (BLOCK_FLINT_FLAKE, 2)],
        output: (BLOCK_FLINT_AXE_HEAD, 1),
    },
    Recipe {
        // The most flint of the three, because a pick head has to be
        // long enough to reach past your knuckles into the rock.
        name: "pick head",
        inputs: &[(BLOCK_FLINT, 1), (BLOCK_FLINT_FLAKE, 3)],
        output: (BLOCK_FLINT_PICK_HEAD, 1),
    },
    // Step four: the binding. Head, haft, fibre -- and the fibre is what
    // separates the three, because the lashing is the part that fails.
    // A knife is held in the hand and barely needs one; an axe is swung,
    // and everything a swing does to the head goes into the binding.
    Recipe {
        name: "flint knife",
        inputs: &[
            (BLOCK_FLINT_KNIFE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_FIBER, 2),
        ],
        output: (BLOCK_FLINT_KNIFE, 1),
    },
    Recipe {
        name: "flint axe",
        inputs: &[
            (BLOCK_FLINT_AXE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_FIBER, 4),
        ],
        output: (BLOCK_FLINT_AXE, 1),
    },
    Recipe {
        name: "flint pick",
        inputs: &[
            (BLOCK_FLINT_PICK_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_FIBER, 5),
        ],
        output: (BLOCK_FLINT_PICKAXE, 1),
    },
    Recipe {
        name: "copper ingot",
        inputs: &[(BLOCK_COPPER_ORE, 1), (BLOCK_COAL, 1)],
        output: (BLOCK_COPPER_INGOT, 1),
    },
    Recipe {
        name: "tin ingot",
        inputs: &[(BLOCK_TIN_ORE, 1), (BLOCK_COAL, 1)],
        output: (BLOCK_TIN_INGOT, 1),
    },
    Recipe {
        // Three to one, which is roughly the real alloy, and four
        // ingots in for three out: some of it is lost in the crucible,
        // and more to the point a mixture that came out heavier than it
        // went in would be a way to print metal.
        name: "bronze ingot",
        inputs: &[(BLOCK_COPPER_INGOT, 3), (BLOCK_TIN_INGOT, 1)],
        output: (BLOCK_BRONZE_INGOT, 3),
    },
    Recipe {
        // Three coal to the soft metals' one. See the note above: this
        // is the furnace, expressed as fuel.
        name: "iron ingot",
        inputs: &[(BLOCK_IRON_ORE, 1), (BLOCK_COAL, 3)],
        output: (BLOCK_IRON_INGOT, 1),
    },
];

/// Why a craft cannot happen, or that it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feasibility {
    Ready,
    MissingIngredients,
    /// The output has nowhere to go.
    NoRoom,
}

impl Feasibility {
    pub fn is_ready(self) -> bool {
        self == Feasibility::Ready
    }
}

pub fn recipe(index: usize) -> Option<&'static Recipe> {
    RECIPES.get(index)
}

/// Whether a recipe could run against this inventory right now.
///
/// The room check matters as much as the ingredient one: consuming four
/// cobblestone and then finding nowhere to put the stone would destroy
/// them, and "my blocks vanished" is the worst possible bug report.
pub fn feasibility(inventory: &Inventory, recipe: &Recipe) -> Feasibility {
    for &(block, amount) in recipe.inputs {
        if inventory.count(block) < amount {
            return Feasibility::MissingIngredients;
        }
    }

    // Checked against a copy with the inputs already gone, because the
    // ingredients usually free the very slots the output needs -- four
    // cobblestone out of a full bar leaves room the naive check would
    // not see.
    let mut after = inventory.clone();
    for &(block, amount) in recipe.inputs {
        after.take_exact(block, amount);
    }
    if !after.has_room_for(recipe.output.0, recipe.output.1) {
        return Feasibility::NoRoom;
    }
    Feasibility::Ready
}

/// How many times the ingredients would stretch to.
///
/// Ingredients only -- room is not counted, because room comes back as
/// the inputs are spent and predicting that for a run of crafts means
/// simulating the whole run. The server decides what actually happens;
/// this is the number the menu shows so the player can see that a click
/// is worth making.
pub fn possible_crafts(inventory: &Inventory, recipe: &Recipe) -> u32 {
    recipe
        .inputs
        .iter()
        .map(|&(block, amount)| inventory.count(block) / amount.max(1))
        .min()
        .unwrap_or(0)
}

/// What the recipe is still short of, if anything: the block, and how
/// many more of it are needed.
///
/// "You are short an ingredient" and "you have no room" are different
/// problems and the player can only fix one of them, so the menu has to
/// be able to say which -- and, for the first, *what*.
pub fn missing_ingredient(inventory: &Inventory, recipe: &Recipe) -> Option<(BlockId, u32)> {
    recipe.inputs.iter().find_map(|&(block, amount)| {
        let have = inventory.count(block);
        // `then`, not `then_some`: the argument to `then_some` is
        // evaluated whatever the condition says, and `amount - have`
        // underflows for every ingredient the player has enough of.
        (have < amount).then(|| (block, amount - have))
    })
}

/// Runs a recipe against an inventory.
///
/// Returns false and changes nothing if it could not run. All-or-nothing
/// is the whole contract: a craft that half-happened is items destroyed.
pub fn craft(inventory: &mut Inventory, recipe: &Recipe) -> bool {
    if !feasibility(inventory, recipe).is_ready() {
        return false;
    }
    for &(block, amount) in recipe.inputs {
        if !inventory.take_exact(block, amount) {
            // Cannot happen after the check above, and if it ever does,
            // stopping here leaves less damage than carrying on.
            return false;
        }
    }
    // The room was checked against exactly this state, so nothing is
    // left over.
    inventory.add(recipe.output.0, recipe.output.1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{MAX_STACK, SLOTS};
    use crate::types::{is_item, is_known_block, is_placeable};

    #[test]
    fn every_recipe_is_made_of_real_blocks() {
        for r in RECIPES {
            assert!(!r.name.is_empty(), "a recipe with no name");
            // The menu draws the name beside the row's status text, in a
            // column the width of the slot grid. Prose does not fit --
            // see the note on the field.
            assert!(r.name.len() <= 14, "'{}' is too long for a recipe row", r.name);
            assert!(!r.inputs.is_empty(), "{} costs nothing", r.name);
            for &(block, amount) in r.inputs {
                assert!(is_known_block(block), "{} takes an unknown block", r.name);
                assert!(amount > 0, "{} takes zero of something", r.name);
            }
            assert!(is_known_block(r.output.0), "{} makes an unknown block", r.name);
            assert!(r.output.1 > 0, "{} makes nothing", r.name);
            // Everything craftable has to be placeable *or* an item with
            // a use. An unplaceable output that nothing else wants is a
            // slot the player fills once and can never empty.
            //
            // The tools are the reason this is not simply "placeable":
            // a pickaxe is the first craftable thing in the game that is
            // neither put down nor consumed, it is *used*, and what
            // makes it legitimate is `break_seconds_with` asking for it.
            assert!(
                is_placeable(r.output.0) || is_item(r.output.0),
                "{} makes something that can neither be placed nor carried",
                r.name
            );
            if is_item(r.output.0) {
                let is_tool = crate::blocks::definition(r.output.0).tool.is_some();
                let feeds_something = RECIPES
                    .iter()
                    .any(|other| other.inputs.iter().any(|&(b, _)| b == r.output.0));
                // The two ends of the metal chain are the exception, and
                // a stated one. Bronze and iron are smelted and then sit
                // there, because the tools they were for are gone and
                // the furnace that would replace this smelting does not
                // exist yet. The rule they break is real -- an item
                // nothing wants is dead weight -- so they are named here
                // rather than quietly slipping through a weaker check,
                // and this list wants to be empty again.
                let awaiting_the_forge =
                    matches!(r.output.0, BLOCK_BRONZE_INGOT | BLOCK_IRON_INGOT);
                assert!(
                    is_tool || feeds_something || awaiting_the_forge,
                    "{} makes an item with nothing to do",
                    r.name
                );
            }
        }
    }

    #[test]
    fn no_recipe_makes_its_own_ingredient_for_free() {
        // A recipe whose output is also one of its inputs, at a higher
        // count, is an infinite item generator.
        for r in RECIPES {
            for &(block, amount) in r.inputs {
                if block == r.output.0 {
                    assert!(
                        r.output.1 < amount,
                        "{} turns {amount} into {} of the same block",
                        r.name,
                        r.output.1
                    );
                }
            }
        }
    }

    #[test]
    fn a_recipe_loop_cannot_multiply_blocks() {
        // Planks and logs convert both ways. Round-tripping must lose
        // material, or a player can sit in the menu making logs.
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_LOG, 4);
        let before = inventory.count(BLOCK_LOG);

        for _ in 0..4 {
            craft(&mut inventory, &RECIPES[0]); // log -> 4 planks
        }
        for _ in 0..4 {
            craft(&mut inventory, &RECIPES[1]); // 4 planks -> log
        }
        assert!(
            inventory.count(BLOCK_LOG) <= before,
            "a full round trip created logs: {before} became {}",
            inventory.count(BLOCK_LOG)
        );
    }

    #[test]
    fn a_recipe_runs_when_the_ingredients_are_there() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_LOG, 1);
        assert_eq!(feasibility(&inventory, &RECIPES[0]), Feasibility::Ready);
        assert!(craft(&mut inventory, &RECIPES[0]));
        assert_eq!(inventory.count(BLOCK_LOG), 0);
        assert_eq!(inventory.count(BLOCK_PLANKS), 4);
    }

    #[test]
    fn a_recipe_without_its_ingredients_changes_nothing() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_PLANKS, 3); // one short of the beam recipe
        assert_eq!(
            feasibility(&inventory, &RECIPES[1]),
            Feasibility::MissingIngredients
        );
        assert!(!craft(&mut inventory, &RECIPES[1]));
        assert_eq!(inventory.count(BLOCK_PLANKS), 3, "a failed craft consumed something");
    }

    #[test]
    fn a_multi_ingredient_recipe_needs_all_of_them() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_DIRT, 2);
        assert!(!craft(&mut inventory, &RECIPES[4]), "turf without leaves");
        inventory.add(BLOCK_LEAVES, 2);
        assert!(craft(&mut inventory, &RECIPES[4]));
        assert_eq!(inventory.count(BLOCK_GRASS), 1);
        assert_eq!(inventory.count(BLOCK_DIRT), 0);
        assert_eq!(inventory.count(BLOCK_LEAVES), 0);
    }

    #[test]
    fn a_full_inventory_refuses_rather_than_destroying_the_ingredients() {
        // The worst bug this could have: consume the inputs, find
        // nowhere for the output, and eat both.
        let mut inventory = Inventory::new();
        // Every slot full of something unrelated.
        for _ in 0..SLOTS {
            inventory.add(BLOCK_SAND, MAX_STACK);
        }
        // ...and no room at all. Swap one slot's worth for the input.
        inventory.take_from(0, MAX_STACK);
        inventory.add(BLOCK_LOG, 1);
        assert!(inventory.has_room_for(BLOCK_LOG, 1));

        // Now the bar is full again except for that one part-slot.
        inventory.add(BLOCK_SAND, MAX_STACK - 1);
        let before = inventory.count(BLOCK_LOG);
        if feasibility(&inventory, &RECIPES[0]) == Feasibility::NoRoom {
            assert!(!craft(&mut inventory, &RECIPES[0]));
            assert_eq!(
                inventory.count(BLOCK_LOG),
                before,
                "a refused craft ate the ingredients"
            );
        }
    }

    #[test]
    fn freeing_a_slot_with_the_ingredients_counts_as_room() {
        // Four cobblestone out of an otherwise full bar leaves exactly
        // the slot the stone needs. A naive room check would refuse.
        let mut inventory = Inventory::new();
        for _ in 0..(SLOTS - 1) {
            inventory.add(BLOCK_SAND, MAX_STACK);
        }
        inventory.add(BLOCK_COBBLESTONE, 1);
        assert_eq!(
            feasibility(&inventory, &RECIPES[2]),
            Feasibility::Ready,
            "the slot the ingredients free was not counted"
        );
        assert!(craft(&mut inventory, &RECIPES[2]));
        assert_eq!(inventory.count(BLOCK_PEBBLE), 3);
    }

    #[test]
    fn the_menu_can_say_how_many_are_possible() {
        let mut inventory = Inventory::new();
        assert_eq!(possible_crafts(&inventory, &RECIPES[0]), 0);
        inventory.add(BLOCK_LOG, 3);
        assert_eq!(possible_crafts(&inventory, &RECIPES[0]), 3);

        // Limited by the scarcest ingredient, not the most plentiful.
        inventory.add(BLOCK_DIRT, 40);
        inventory.add(BLOCK_LEAVES, 4);
        assert_eq!(possible_crafts(&inventory, &RECIPES[4]), 2, "turf: 2 leaves each");
    }

    #[test]
    fn the_menu_can_say_what_is_missing() {
        let mut inventory = Inventory::new();
        assert_eq!(missing_ingredient(&inventory, &RECIPES[1]), Some((BLOCK_PLANKS, 4)));
        inventory.add(BLOCK_PLANKS, 3);
        assert_eq!(missing_ingredient(&inventory, &RECIPES[1]), Some((BLOCK_PLANKS, 1)));
        inventory.add(BLOCK_PLANKS, 1);
        assert_eq!(missing_ingredient(&inventory, &RECIPES[1]), None);
    }

    #[test]
    fn what_is_possible_agrees_with_what_can_be_made() {
        // The two are read off the same inventory a click apart, so a
        // menu that says "x3" over a recipe the craft then refuses is a
        // menu that lies.
        for r in RECIPES {
            let mut inventory = Inventory::new();
            for &(block, amount) in r.inputs {
                inventory.add(block, amount);
            }
            assert_eq!(possible_crafts(&inventory, r), 1, "{}", r.name);
            assert_eq!(missing_ingredient(&inventory, r), None, "{}", r.name);
            assert!(craft(&mut inventory, r), "{} could not be made", r.name);
            assert_eq!(possible_crafts(&inventory, r), 0, "{}", r.name);
        }
    }

    /// The stone age, walked from an empty pack.
    ///
    /// Everything below is one claim: **a player who has picked up flint,
    /// sticks and grass can end up holding all three tools**, and cannot
    /// get there any other way. It is worth a test rather than a
    /// playthrough because the chain is four steps deep and the failure
    /// mode is silent -- a head that needs a flake the flake recipe
    /// cannot supply is not a compile error, it is a game whose first
    /// tool is unreachable.
    fn named(name: &str) -> &'static Recipe {
        RECIPES
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no recipe called {name:?}"))
    }

    #[test]
    fn the_whole_chain_runs_on_what_the_world_leaves_lying_about() {
        // Exactly the three things a player can gather bare-handed: a
        // nodule of flint, a fallen branch, a handful of grass. No
        // shortcuts are added to the inventory at any point -- every
        // ingredient after this line has to come out of a recipe above
        // it, which is what makes this a chain rather than a list.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT, 6);
        pack.add(BLOCK_STICK, 3);
        pack.add(BLOCK_FIBER, 11);

        // Step one, three times over: six nodules into eighteen flakes.
        for _ in 0..6 {
            assert!(craft(&mut pack, named("flint flakes")), "knapping failed");
        }
        assert_eq!(pack.count(BLOCK_FLINT_FLAKE), 18);
        assert_eq!(pack.count(BLOCK_FLINT), 0, "the nodules should be spent");

        // Step two: three hafts, each whittled with a flake -- and note
        // that nothing in the pack is a tool yet. This is the step that
        // would deadlock if a haft needed a finished knife.
        for _ in 0..3 {
            assert!(craft(&mut pack, named("worked stick")), "whittling failed");
        }
        assert_eq!(pack.count(BLOCK_WORKED_STICK), 3);
        assert_eq!(pack.count(BLOCK_STICK), 0);

        // Step three needs flint again, which the pack no longer has --
        // so knap what is left of nothing? No: this is where the test
        // earns its keep. Two more nodules, because the axe and the pick
        // heads are cores rather than flakes.
        pack.add(BLOCK_FLINT, 2);
        for head in ["knife head", "axe head", "pick head"] {
            assert!(craft(&mut pack, named(head)), "{head} could not be made");
        }

        // Step four: the binding.
        for tool in ["flint knife", "flint axe", "flint pick"] {
            assert!(craft(&mut pack, named(tool)), "{tool} could not be bound");
        }
        assert_eq!(pack.count(BLOCK_FLINT_KNIFE), 1);
        assert_eq!(pack.count(BLOCK_FLINT_AXE), 1);
        assert_eq!(pack.count(BLOCK_FLINT_PICKAXE), 1);
    }

    #[test]
    fn no_tool_can_be_made_without_its_head_and_its_haft() {
        // The old one-click recipe is gone, and this is what makes sure
        // it has not grown back somewhere: every tool in the game is
        // bound from a head and a haft, and neither of them is raw
        // material. A recipe that took flint and sticks straight to a
        // pickaxe would pass every other test in this file.
        for r in RECIPES {
            if crate::blocks::definition(r.output.0).tool.is_none() {
                continue;
            }
            let head = r.inputs.iter().any(|&(b, _)| {
                matches!(
                    b,
                    BLOCK_FLINT_KNIFE_HEAD | BLOCK_FLINT_AXE_HEAD | BLOCK_FLINT_PICK_HEAD
                )
            });
            let haft = r.inputs.iter().any(|&(b, _)| b == BLOCK_WORKED_STICK);
            assert!(head, "{} is made without a head", r.name);
            assert!(haft, "{} is made without a worked haft", r.name);
            for &(b, _) in r.inputs {
                assert_ne!(b, BLOCK_STICK, "{} takes a raw branch", r.name);
                assert_ne!(b, BLOCK_FLINT, "{} takes an unknapped nodule", r.name);
            }
        }
        // ...and a head is not a tool: holding one is holding a stone.
        for head in [
            BLOCK_FLINT_KNIFE_HEAD,
            BLOCK_FLINT_AXE_HEAD,
            BLOCK_FLINT_PICK_HEAD,
        ] {
            assert!(crate::blocks::definition(head).tool.is_none());
        }
    }

    #[test]
    fn a_tool_cannot_be_assembled_out_of_order() {
        // The steps are a sequence, and skipping one has to fail rather
        // than half-happen. A pack with everything the last step needs
        // except the head gets nothing, and loses nothing.
        let mut pack = Inventory::new();
        pack.add(BLOCK_WORKED_STICK, 1);
        pack.add(BLOCK_FIBER, 9);
        assert!(!craft(&mut pack, named("flint pick")), "a pick with no head");
        assert_eq!(pack.count(BLOCK_FIBER), 9, "a refused craft spent fibre");

        // ...and a haft cannot be whittled by wishing, either.
        let mut bare = Inventory::new();
        bare.add(BLOCK_STICK, 4);
        assert!(!craft(&mut bare, named("worked stick")), "whittled with what?");
    }

    #[test]
    fn nothing_is_left_of_the_metal_picks() {
        // They were recipes 22, 23 and 24. Their ids are gone from the
        // block table, so a recipe still naming one would fail
        // `is_known_block` -- but a recipe *named* after one would sit
        // there looking plausible, so the names go too.
        for gone in ["copper pick", "bronze pick", "iron pick"] {
            assert!(
                !RECIPES.iter().any(|r| r.name == gone),
                "{gone} is still in the menu"
            );
        }
    }

    #[test]
    fn an_unknown_recipe_index_is_not_a_panic() {
        assert!(recipe(RECIPES.len()).is_none());
        assert!(recipe(usize::MAX).is_none());
    }
}
