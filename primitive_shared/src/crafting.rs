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
    BLOCK_BIRCH_LOG, BLOCK_BIRCH_PLANKS, BLOCK_BRONZE_INGOT,
    BLOCK_COAL, BLOCK_COPPER_INGOT, BLOCK_COPPER_ORE, BLOCK_STONE_AXE, BLOCK_STONE_AXE_HEAD,
    BLOCK_FLINT_FLAKE, BLOCK_FLINT_KNIFE, BLOCK_FLINT_KNIFE_HEAD, BLOCK_STONE_PICKAXE,
    BLOCK_STONE_PICK_HEAD, BLOCK_GRAVEL, BLOCK_IRON_INGOT, BLOCK_IRON_ORE,
    BLOCK_LEAVES, BLOCK_LOG, BLOCK_PEBBLE, BLOCK_PLANKS, BLOCK_SAND, BLOCK_STICK,
    BLOCK_TALL_GRASS, BLOCK_TIN_INGOT, BLOCK_TIN_ORE, BLOCK_WORKED_STICK,
    // 1.5: the peg, and what an animal gives.
    BLOCK_PEG, BLOCK_BEAR_MEAT, BLOCK_FAT, BLOCK_FOWL_MEAT, BLOCK_HARE_MEAT, BLOCK_RIBS,
    BLOCK_ROASTED_RIBS, BLOCK_WOLF_MEAT,
    // 1.5: the fire, what grows beside it, and what the metal is for.
    BLOCK_BRONZE_AXE, BLOCK_BRONZE_KNIFE, BLOCK_BRONZE_PICKAXE, BLOCK_CAMPFIRE,
    BLOCK_COOKED_MEAT, BLOCK_COPPER_AXE, BLOCK_COPPER_KNIFE, BLOCK_COPPER_PICKAXE,
    BLOCK_IRON_AXE, BLOCK_IRON_KNIFE, BLOCK_IRON_PICKAXE, BLOCK_RAW_MEAT, BLOCK_REEDS,
    // 1.6: the fire that is hot enough, and what it fires.
    BLOCK_BRICK, BLOCK_BRICK_RAW, BLOCK_CLAY, BLOCK_KILN,
    // ...and the field.
    BLOCK_BREAD, BLOCK_DOUGH, BLOCK_GRAIN, BLOCK_HOE,
    // ...and the pot the copper age runs on.
    BLOCK_BLOOMERY, BLOCK_IRON_BLOOM, BLOCK_MOULD, BLOCK_MOULD_RAW, BLOCK_NATIVE_COPPER,
    BLOCK_VESSEL, BLOCK_VESSEL_RAW,
    // 1.9: the torch.
    BLOCK_BRACKET_FUNGUS, BLOCK_RESIN, BLOCK_STANDING_TORCH, BLOCK_TORCH, BLOCK_TORCH_SPENT,
    // 1.9: the copper bench.
    BLOCK_COPPER_AXE_HEAD, BLOCK_COPPER_HOE, BLOCK_COPPER_HOE_HEAD, BLOCK_COPPER_PICK_HEAD,
    BLOCK_COPPER_SHOVEL, BLOCK_COPPER_SHOVEL_HEAD,
    // 1.7: the tannery, what a person wears, and the jug that answers
    // thirst. See `equipment` and `body` for what any of it is worth.
    BLOCK_BRONZE_BOOTS, BLOCK_BRONZE_CUIRASS, BLOCK_BRONZE_GREAVES, BLOCK_BRONZE_HELM,
    BLOCK_DRYING_RACK, BLOCK_IRON_BOOTS, BLOCK_IRON_CUIRASS, BLOCK_IRON_GREAVES, BLOCK_ROASTED_ROOT,
    BLOCK_ROOT,
    BLOCK_IRON_HELM, BLOCK_JUG, BLOCK_JUG_RAW, BLOCK_LEATHER, BLOCK_LEATHER_BOOTS,
    BLOCK_LEATHER_CAP, BLOCK_LEATHER_LEGGINGS, BLOCK_LEATHER_TUNIC,
    // ...and what a fleece becomes.
    BLOCK_WOOL, BLOCK_WOOL_BOOTS, BLOCK_WOOL_CAP, BLOCK_WOOL_LEGGINGS, BLOCK_WOOL_TUNIC,
    // ...and what a field in warm country is woven into.
    BLOCK_CLOTH, BLOCK_CLOTH_CAP, BLOCK_CLOTH_TROUSERS, BLOCK_CLOTH_TUNIC, BLOCK_CLOTH_WRAPS,
    BLOCK_COTTON,
    // ...and what a skin becomes with no tannery at all.
    BLOCK_BEAR_HIDE, BLOCK_FUR_CLOAK, BLOCK_FUR_HOOD, BLOCK_PELT,
    // ...and the middle of the bread chain, with the jug that wets it.
    BLOCK_FLOUR, BLOCK_JUG_WATER,
    // ...and what a room is furnished with.
    BLOCK_BARREL, BLOCK_BED, BLOCK_CHAIR, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE,
    // ...and the four spears, and what is smeared on them.
    BLOCK_BONE_SPEAR, BLOCK_BRONZE_SPEAR, BLOCK_COPPER_SPEAR, BLOCK_IRON_SPEAR, BLOCK_TOADSTOOL,
    // ...and what a felled tree leaves on the ground.
    BLOCK_STRIPPED_LOG,
    BLOCK_SINEW,
    BLOCK_BONE,
    BLOCK_CORD, BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, BLOCK_FLINT_SPEAR, BLOCK_RUSTY_STONE, BLOCK_IRON_DUST,
    // ...and what a wound is dressed with.
    BLOCK_BANDAGE, BLOCK_POULTICE, BLOCK_SPLINT,
    // ...and the meadow's dressing. See the "leaf poultice" row.
    BLOCK_PLANTAIN,
    // ...and the raft, and the two things it is lashed round.
    BLOCK_HIDE, BLOCK_OAR, BLOCK_RAFT, BLOCK_SAIL,
};

/// Where a recipe can be run.
///
/// Two values, and the second one is the whole of what 1.5 added to
/// crafting. A recipe used to be something you could do standing
/// anywhere, which is right for tying fibre around a stick and is a lie
/// about smelting copper -- and the lie was *written down* in the old
/// note below, which said the fire was "implied".
///
/// It is not implied any more. `Heat` means the player has to be beside
/// a burning campfire, and that turns every metal recipe from a menu
/// entry into a place you have to build and keep alight -- in the rain,
/// with fuel you had to go and get. Which is what a forge is, minus the
/// second inventory screen a forge would have cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Station {
    /// Hands, anywhere. Most of the table.
    Hands,
    /// Beside a lit fire of any kind. Cooking, and burning wood down to
    /// charcoal.
    Heat,
    /// Beside a lit **kiln**. Everything that melts or fires.
    ///
    /// **The rung 1.5 was missing.** Every metal recipe used to say
    /// `Heat`, which put the whole of metallurgy over a ring of stones
    /// in a meadow -- and a campfire is about six hundred degrees where
    /// copper melts at a thousand and eighty-five. It was the one step
    /// in this world's progression that could not have happened.
    ///
    /// So there is a second fire, built out of the riverbank, and it is
    /// what every ingot now goes through. What it costs the player is a
    /// clay pit, a load of charcoal and a decision about where to put
    /// the thing; what it buys is that the order they climb the ladder
    /// in is the order it was actually climbed: fire, then pottery, then
    /// metal. See `types::BLOCK_KILN`.
    Forge,
    /// Beside a lit **bloomery**. Iron, and only iron.
    ///
    /// A third fire, because iron is not copper and cannot be treated as
    /// it. Copper melts at 1085 degrees and a pot in a good fire reaches
    /// that; iron melts at 1538, which nothing in the ancient world
    /// reached at all. What a bloomery does instead is *reduce* the ore
    /// below its melting point in a tall charcoal shaft -- see
    /// `types::BLOCK_BLOOMERY` -- and what comes out is not an ingot but
    /// a bloom that still has the slag in it.
    Bloomery,
    /// Beside a **joiner's bench** (`types::BLOCK_WORKBENCH`).
    ///
    /// **The four workshops are not hearths**, and the difference is the
    /// whole of how they run: nothing is loaded, nothing burns and nothing
    /// waits. A workshop row is crafted from the pack, at once, by the same
    /// path a hand row takes (`craft`), and all the station adds is the
    /// question "is one within reach" -- asked of the world the way a fire
    /// is, by the client for the menu and by the server for the verdict.
    ///
    /// **What a workshop is for is a better way, never the only way to
    /// survive.** The rows that keep a player alive in a field -- a stool, a
    /// straw bed, a chest, a frame, a coil pot, a pebble's grinding -- stay in
    /// the hands. What moves to the bench is what a bench is: joinery square
    /// enough to hang a door on, and boards sawn rather than split.
    ///
    /// Rejected, and written down because the first version of this answer
    /// was "no stations at all" (see the changelog):
    ///
    /// * **Every wooden row at the bench.** It would turn the first night's
    ///   chest into a walk home, and a chest is what a far camp needs most.
    ///   A station that is merely a toll on things the field already made is
    ///   the chore this codebase refuses.
    /// * **The bench as a bonus with nothing behind it** -- the same rows,
    ///   cheaper. Better, and still not enough: the player asked for a place
    ///   where *harder* things are made, and a door, a bed, a chair and a
    ///   table are the pieces that make a camp a house.
    /// * **One "workshop" block for all four trades.** One flag instead of
    ///   four, and one decision instead of four: a potter's yard by the clay
    ///   bank and a mason's block at the quarry are places a player chooses,
    ///   and one bench that does everything would be built once, at home,
    ///   and never thought about again.
    Bench,
    /// Beside a **mason's block** (`types::BLOCK_MASON_BLOCK`). See `Bench`.
    Mason,
    /// Beside a **potter's wheel** (`types::BLOCK_POTTERS_WHEEL`). See `Bench`.
    Wheel,
    /// Beside a **currier's bench** (`types::BLOCK_LEATHER_BENCH`). See `Bench`.
    Leather,
}

impl Station {
    /// The four workshops, in the order the menu offers them.
    pub const WORKSHOPS: [Station; 4] = [Station::Bench, Station::Mason, Station::Wheel, Station::Leather];

    /// Whether a hearth runs this row, rather than the player's hands.
    ///
    /// **Its own question now, and it used to be `!= Hands`.** Every row that
    /// was not a hand row was a fire row, so "not by hand" meant "load it
    /// into a hearth" all over the tree -- the craft path refused it, the
    /// menu hid it, the pit looked for it. A workshop row is neither: it is
    /// crafted from the pack like a hand row and only asks where the player
    /// stands. Left as `!= Hands`, every one of those places would have sent
    /// a door to the kiln.
    pub fn is_hearth(self) -> bool {
        matches!(self, Station::Heat | Station::Forge | Station::Bloomery)
    }

    /// The block that is this workshop, if it is one.
    pub fn workshop_block(self) -> Option<BlockId> {
        match self {
            Station::Bench => Some(crate::types::BLOCK_WORKBENCH),
            Station::Mason => Some(crate::types::BLOCK_MASON_BLOCK),
            Station::Wheel => Some(crate::types::BLOCK_POTTERS_WHEEL),
            Station::Leather => Some(crate::types::BLOCK_LEATHER_BENCH),
            Station::Hands | Station::Heat | Station::Forge | Station::Bloomery => None,
        }
    }

    /// The workshop a block is, if it is one. The inverse of `workshop_block`,
    /// and the one question both sides' scans of the world ask of a cell.
    pub fn of_workshop(block: BlockId) -> Option<Station> {
        let kind = crate::types::block_kind(block);
        Station::WORKSHOPS.into_iter().find(|s| s.workshop_block() == Some(kind))
    }
}

/// What a player has within reach to work at.
///
/// A pair of bools rather than the single `beside_fire` this replaced,
/// because there are two hearths now and the recipes distinguish them.
/// One value carried end to end -- the client works it out from the
/// chunks it has, the server from its own fire map, and both hand it to
/// `feasibility` -- so a menu row that is offered is a craft that will
/// be allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Heat {
    /// A lit campfire within `types::FIRE_WORKING_RANGE`.
    pub fire: bool,
    /// A lit kiln within the same.
    pub kiln: bool,
    /// A lit bloomery within the same.
    pub bloomery: bool,
    /// The workshops within the same range, as bits by their place in
    /// `Station::WORKSHOPS`.
    ///
    /// **Still called `Heat`, and it is not only heat any more.** The name is
    /// carried by the craft path on both sides, the hearth tests and the mod
    /// API's `heat_at`; renaming it would touch all of them to say what this
    /// comment says. A mask rather than four more bools because a workshop
    /// is found by one scan that asks `Station::of_workshop` of every cell,
    /// and a bit by index is what that scan can set without a match of its
    /// own to keep in step with the list.
    pub workshops: u8,
}

impl Heat {
    /// Nothing burning anywhere near: a player standing in a field.
    pub const NONE: Heat = Heat { fire: false, kiln: false, bloomery: false, workshops: 0 };

    /// Marks a workshop as within reach.
    pub fn with_workshop(mut self, station: Station) -> Heat {
        if let Some(bit) = Station::WORKSHOPS.iter().position(|&s| s == station) {
            self.workshops |= 1 << bit;
        }
        self
    }

    /// Is this workshop within reach?
    pub fn has_workshop(self, station: Station) -> bool {
        Station::WORKSHOPS.iter().position(|&s| s == station).is_some_and(|bit| self.workshops & (1 << bit) != 0)
    }

    /// Can work be done here?
    ///
    /// **A kiln counts as a fire.** It is the hotter of the two and it
    /// is lit; a player who has built one and is asked to lay a campfire
    /// beside it to cook their dinner has been given a chore rather than
    /// a decision. The reverse is not true and is the whole point.
    pub fn allows(self, station: Station) -> bool {
        match station {
            Station::Hands => true,
            // Any fire at all, a bloomery included: they are all fires,
            // and a player standing at one they can cook over should be
            // able to cook.
            Station::Heat => self.fire || self.kiln || self.bloomery,
            Station::Forge => self.kiln,
            Station::Bloomery => self.bloomery,
            Station::Bench | Station::Mason | Station::Wheel | Station::Leather => self.has_workshop(station),
        }
    }
}

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
    /// Where it can be done. See `Station`.
    ///
    /// A field on the recipe rather than a separate list of "hot"
    /// recipes, for the reason the block table gives at the top of
    /// `blocks`: two lists that have to agree are a bug waiting for
    /// somebody to add a row to one of them.
    pub station: Station,
    /// What the player gets *back* on top of the output.
    ///
    /// **For equipment rather than ingredients.** Melting ore consumes
    /// the ore and the charcoal and uses up neither the crucible nor the
    /// mould -- a pot you had to throw away after one pour would not be
    /// a pot, it would be a very slow ingredient. Without this the only
    /// way to express that was to leave the vessel out of the recipe
    /// altogether, which is to say to pretend it is not needed.
    ///
    /// Empty for nearly every row, so it costs a word per recipe and
    /// nothing to read past.
    pub returns: &'static [(BlockId, u32)],
    /// The chance, in 0..1, that the attempt spends its inputs and
    /// makes nothing.
    ///
    /// **Knapping, and only knapping.** A nodule struck wrong does not
    /// give a smaller blade; it gives gravel, and the next nodule is the
    /// only way on. That is what the player asked for in as many words
    /// -- "you take a flint, work it, it breaks, again, until you get a
    /// blade" -- and it is what makes a knife head a thing you *tried
    /// for* rather than a line in a menu. Nothing else in the table
    /// fails: a pot is thrown badly and thrown again from the same
    /// clay, and a haft is a stick until it is a haft.
    ///
    /// A field on every recipe rather than a list of the ones that can
    /// fail, for the reason `station` gives: every row states its
    /// chance, and a new knapping row cannot forget to. The roll itself
    /// is the caller's -- see `craft` and `Attempt` -- so the table
    /// stays a table and a test can say what a failure costs without
    /// dice.
    pub failure: f32,
}

/// How the roll against `Recipe::failure` came out.
///
/// Decided by whoever holds the dice and handed in, rather than rolled
/// inside `craft`: the shared crate has no random source on purpose
/// (the client would need one that agrees with the server's, and it
/// cannot), and a `craft` that rolled for itself would be a `craft` no
/// test could call twice and get the same pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// The blow landed. The ordinary case, and the only one a recipe
    /// with `failure: 0.0` can have.
    Succeeds,
    /// The nodule shattered: the inputs are spent and nothing is made.
    Fails,
}

/// What a call to `craft` did to the pack.
///
/// Three answers where there used to be two, because a failed knapping
/// is neither: the flint is gone, which "refused" would deny, and there
/// is no blade, which "made" would claim. The server tells the player
/// something different for each, and a mod counting what it made must
/// not count a shattered nodule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crafted {
    /// Inputs spent, output in the pack, equipment back.
    Made,
    /// Inputs spent, nothing made -- the equipment still comes back,
    /// because a pot survives a spoiled pour even if the pour does not.
    Failed,
    /// Nothing happened and nothing was spent.
    Refused,
}

impl Crafted {
    pub fn is_made(self) -> bool {
        self == Crafted::Made
    }
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
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Six boards a beam, not four**, and the saw is why. A log split with
    // an axe is four boards ("planks"); a log sawn is six ("sawn planks", at
    // the end of the table). While a beam took four back, a saw was a mill:
    // six boards out, four back in, two boards out of nothing every turn --
    // the loop `a_recipe_loop_cannot_multiply_blocks` exists to catch. Six is
    // what a log's worth of boards is once there is a way to get all of them,
    // and boards split by hand simply do not go back into a whole log.
    Recipe {
        name: "beam",
        inputs: &[(BLOCK_PLANKS, 6)],
        output: (BLOCK_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
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
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "mulch",
        inputs: &[(crate::types::BLOCK_LEAF_HANDFUL, 4)],
        output: (BLOCK_DIRT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "turf",
        inputs: &[(BLOCK_DIRT, 2), (crate::types::BLOCK_LEAF_HANDFUL, 2)],
        output: (BLOCK_GRASS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "sand",
        inputs: &[(BLOCK_COBBLESTONE, 2)],
        output: (BLOCK_SAND, 3),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The one recipe that turns a block into something that is not
        // one: a stick is drawn as a twig standing in its cell rather
        // than as a cube of wood. Cheap on purpose -- it is the
        // ingredient everything made of wood will want.
        name: "sticks",
        inputs: &[(BLOCK_PLANKS, 2)],
        output: (BLOCK_STICK, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
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
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // ...and back the other way, so a field is a renewable thing
        // rather than one a player can strip permanently. Twisting the
        // fibre back into a tuft costs more than one tuft yields, which
        // is what stops it being a loop that prints dirt.
        name: "grass tuft",
        inputs: &[(BLOCK_FIBER, 3)],
        output: (BLOCK_TALL_GRASS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
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
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Fibre bound around a splinter of wood. The first thing in the
        // game made of two materials, and the reason to pick grass at
        // all before there is any rope to make.
        name: "bound sticks",
        inputs: &[(BLOCK_FIBER, 2), (BLOCK_PLANKS, 1)],
        output: (BLOCK_STICK, 3),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Flint is what you knap *with*. A nodule struck against loose
        // stone splits it cleanly, so two of them go as far as four
        // pebbles alone do -- which is the whole reason to bend down for
        // the black stones as well as the grey ones.
        name: "flint knapping",
        inputs: &[(BLOCK_FLINT, 2), (BLOCK_PEBBLE, 2)],
        output: (BLOCK_COBBLESTONE, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // **The joined chest**: boards let into a pegged frame. This row
        // used to be eight planks by hand, which made a chest the thing a
        // player had before they had a knife; it is now the stone-age
        // chest of the furniture rows (see "---- furniture ----" below for
        // the whole argument), and the nailed one is at the end of the
        // table. Still dear on purpose -- a chest is what stops carrying
        // being a decision, and it should cost a tree and a morning's
        // flint to stop making it.
        name: "chest",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 6), (BLOCK_PEG, 4)],
        output: (BLOCK_CHEST, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Gravel is mostly stone and partly flint, and sifting it is
        // how you get the flint out. Four shovelfuls for one nodule:
        // less than walking a stony shore and picking them up, which
        // is the point -- this is what you do when there is no shore.
        name: "sifted gravel",
        inputs: &[(BLOCK_GRAVEL, 4)],
        output: (BLOCK_FLINT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
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
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "birch beam",
        inputs: &[(BLOCK_BIRCH_PLANKS, 6)],
        output: (BLOCK_BIRCH_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Four handfuls pressed into a block of leaves**, for the player who
    // wants a hedge or a roof of green. Breaking leaves gives the handful
    // (`types::BLOCK_LEAF_HANDFUL`), so without this row a leaf block
    // would be a thing the world has and a player can never place. Plain
    // oak leaves whatever tree they came from: the handful does not
    // remember its tree, and a birch-leaf hedge is not a decision anyone
    // is waiting to make.
    //
    // This row was "birch mulch", four birch leaves to a dirt. Once every
    // crown gave the same handful it would have been "mulch" twice, and it
    // could not simply go: a row's index is its identity on the wire, so
    // the row was given the new job instead of a new row being added.
    Recipe {
        name: "bundle leaves",
        inputs: &[(crate::types::BLOCK_LEAF_HANDFUL, 4)],
        output: (BLOCK_LEAVES, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the ages: flint, copper, bronze, iron ----
    //
    // ## Smelting, and the fire that was once implied
    //
    // These recipes shipped without a fire. The note that used to stand
    // here said so plainly -- "smelting is a craft: ore plus fuel makes
    // metal, and the fire is implied" -- and gave the honest reason: a
    // furnace is a block with an inside, a burn timer and a screen to
    // watch it in, and none of that is *about* metal.
    //
    // It still is not, and there is still no furnace. What there is now
    // is a campfire, which is a block you build, light, feed and keep
    // out of the rain, and `Station::Heat` is the whole of the interface
    // to it: stand beside a burning one and the hot recipes light up.
    // No second inventory, no burn timer to watch, no fuel slot -- and
    // yet every one of the things a furnace was supposed to buy is
    // there, because the fuel goes *into the fire* rather than into a
    // recipe, and the fire is a place in the world that can go out.
    //
    // The physics that was already honest stays honest: **copper and tin
    // melt at temperatures a campfire reaches, and iron does not**,
    // which is why the two soft metals cost one coal at the fireside and
    // iron costs three. What a forge really buys is more fuel and more
    // air, and the fuel is the part that can be charged for.
    //
    // **And the metal is finally made into something.** The nine tools
    // at the end of this table are what this little economy was missing:
    // it used to smelt bronze that nothing wanted. See
    // `types::BLOCK_COPPER_KNIFE` for why a metal tool is one step where
    // a flint one is four.
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
    // **Flint is knapped and stone is ground, and they are not the same
    // craft.** The table used to make the axe and pick heads out of
    // flint too, and the player said what was wrong with that in one
    // line: flint is knives and spears, because it splits sharp and
    // pierces; an axe is a hard stone, because it has to be *hard*,
    // and a knapped edge shatters the first time it meets a trunk. So
    // the knife head (and the spear's point, which is the same head)
    // is struck off a nodule and can fail in the striking, and the axe
    // and pick heads are a lump of stone ground against another stone
    // until it has a bit -- slow, dull work that cannot fail and needs
    // no flint at all.
    //
    // The numbers, end to end: a knife head is two flakes off a nodule
    // that shatters half the time; an axe head is a cobble and two
    // pebbles, a pick head a cobble and three; and every one of them is
    // lashed to its haft with a *cord*, which is six fibre twisted
    // together, or with a sinew. The cord is the real price -- see
    // `types::BLOCK_CORD` -- and it is the one part of the chain that
    // sends the player to a riverbank rather than to the nearest tuft.
    Recipe {
        // Step one: a nodule struck against another stone comes apart
        // into shards with edges on them. Three, because a struck core
        // yields more waste than tool and the waste is the point here.
        //
        // **And a third of the time it comes apart wrong.** A nodule
        // with a flaw in it, or a blow off the platform, is a handful
        // of gravel with no edge on anything: the flint is gone and
        // there are no flakes. Lower than the knife head's chance
        // because striking flakes *off* is the forgiving half of
        // knapping -- it is shaping the piece that is left that breaks
        // it.
        name: "flint flakes",
        inputs: &[(BLOCK_FLINT, 1)],
        output: (BLOCK_FLINT_FLAKE, 3),
        station: Station::Hands,
        returns: &[],
        failure: 0.35,
    },
    Recipe {
        // Step two: a branch pared down with a flake. The flake is
        // consumed -- an edge that thin does not survive the job, and
        // spending it here is what stops a single nodule from arming a
        // player for good.
        name: "worked stick",
        inputs: &[(BLOCK_STICK, 1), (BLOCK_FLINT_FLAKE, 1)],
        output: (BLOCK_WORKED_STICK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // Step three: the heads. A knife head is a flake with a back put on
    // it, and it is the one head that is *knapped*: struck, thinned,
    // and half the time struck once too often. The axe and the pick
    // heads are ground, not struck -- see below -- and cannot fail.
    Recipe {
        // **Half of them break.** Thinning a flake to a blade is the
        // part of knapping that goes wrong: one blow past the last
        // good one and the piece is in two, and neither half has an
        // edge worth hafting. The flakes are spent either way. What
        // that buys is that a knife head is *attempted*, and the second
        // nodule a player picks up is picked up for a reason.
        //
        // One in two rather than worse, because the flakes are cheap
        // (three off a nodule that mostly does not shatter) and a chain
        // that ate five nodules for one blade would be grinding, not
        // knapping. See `Recipe::failure`.
        name: "knife head",
        inputs: &[(BLOCK_FLINT_FLAKE, 2)],
        output: (BLOCK_FLINT_KNIFE_HEAD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.5,
    },
    // **Ground stone.** An axe head is a lump of hard stone rubbed
    // against another stone until it has a bit on it: the cobble is
    // the lump, the pebbles are what it is ground with, and both are
    // used up -- a grindstone wears as fast as the thing on it. No
    // flint anywhere: a knapped axe is the thing this whole rework
    // exists to stop pretending about. Slow in life and cheap here,
    // because the grinding is not where the difficulty of an axe is:
    // the haft, the lashing and the glue are, and those are three
    // further rows.
    Recipe {
        name: "axe head",
        inputs: &[(BLOCK_COBBLESTONE, 1), (BLOCK_PEBBLE, 2)],
        output: (BLOCK_STONE_AXE_HEAD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The most grinding of the two, because a pick head has to be
        // long enough to reach past your knuckles into the rock, and a
        // point takes longer to bring up than a bit.
        name: "pick head",
        inputs: &[(BLOCK_COBBLESTONE, 1), (BLOCK_PEBBLE, 3)],
        output: (BLOCK_STONE_PICK_HEAD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // Step four: the binding. Head, haft, and a **cord** -- not loose
    // fibre. A handful of grass wound round a joint is what this used to
    // ask for, and it is a thing nobody has ever hafted a tool with:
    // the fibre has to be twisted into a length first, and that twisting
    // is the slow part of the whole business (six fibre a cord, see
    // `types::BLOCK_CORD`). One cord for every stone tool, because the
    // number that used to separate them -- two for a knife, four for an
    // axe -- was a way of saying "an axe is bound harder" that a player
    // read as "an axe costs more grass". What separates them now is the
    // glue: a knife is held in the hand and a lashing is enough for it;
    // an axe is swung, and a lashing alone stops at brush (see
    // `blocks::Tier::Stone` and the glued rows below).
    Recipe {
        name: "flint knife",
        inputs: &[
            (BLOCK_FLINT_KNIFE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_CORD, 1),
        ],
        output: (BLOCK_FLINT_KNIFE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- 1.9: the torch ----
    //
    // **A wad of fibre on a stick, and either stick will do.** Two rows
    // rather than one because a recipe names a block and there is no
    // "any stick" to name -- and both are worth having: the raw stick is
    // what a player has in their first minute, the trimmed haft is what
    // they have once there are tools, and neither should be the wrong
    // answer. They cost the same, because the difference between the two
    // sticks is the bark, and a torch does not care about bark.
    //
    // By hand and nowhere near a fire: winding fibre onto a stick is not
    // hot work. What is hot is *lighting* it, and that is a gesture at a
    // burning hearth rather than a row here -- see `use_block`.
    Recipe {
        name: "torch",
        inputs: &[(BLOCK_STICK, 1), (BLOCK_FIBER, 1)],
        output: (BLOCK_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "haft torch",
        inputs: &[(BLOCK_WORKED_STICK, 1), (BLOCK_FIBER, 1)],
        output: (BLOCK_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and what a burnt-out one is worth: the stick back, and another
    // wad. The whole point of `BLOCK_TORCH_SPENT` being its own id is
    // that this row exists -- a torch you cannot re-wad is a torch you
    // throw away, and the fibre is the part that burns.
    Recipe {
        name: "rewad torch",
        inputs: &[(BLOCK_TORCH_SPENT, 1), (BLOCK_FIBER, 1)],
        output: (BLOCK_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The same torch, wadded with fungus instead of grass**, and the
    // two rows below are the reason the bracket is in the game at all.
    //
    // Fibre comes off tall grass, and tall grass does not grow in the
    // dead forest, the bog or the deep taiga (see
    // `worldgen::Biome::grass_spacing`) -- which are the three places a
    // player most wants a light. Before this, going into the dark
    // meant remembering to bring the dark's answer with you from a
    // meadow, and forgetting was a walk back rather than a problem to
    // solve. Now the answer grows where the problem is: on the fallen
    // trunks of exactly those woods.
    //
    // The same torch, not a better one. A fungus torch that burned
    // longer would make the fibre row the wrong answer everywhere, and
    // then this would not be a second solution to one problem, it would
    // be a replacement for the first.
    Recipe {
        name: "fungus torch",
        inputs: &[(BLOCK_STICK, 1), (BLOCK_BRACKET_FUNGUS, 1)],
        output: (BLOCK_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fungus rewad",
        inputs: &[(BLOCK_TORCH_SPENT, 1), (BLOCK_BRACKET_FUNGUS, 1)],
        output: (BLOCK_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The lashed axe: a ground head, a haft and a cord. What comes
        // out is `Tier::Stone` -- brush and deadfall, never a standing
        // trunk -- until it is set in glue (the "glued axe" row below).
        // Named for what it is made of now; it was "flint axe" while
        // the head was flint, and the old name in the menu would have
        // been the old lie in the menu.
        name: "stone axe",
        inputs: &[
            (BLOCK_STONE_AXE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_CORD, 1),
        ],
        output: (BLOCK_STONE_AXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The same three tools lashed with sinew instead of cord: one
    // tendon does what six strands of twisted grass do, and it comes
    // off a carcass rather than out of an afternoon on a riverbank --
    // which is the reason to butcher a hare before the first metal.
    // See the copper bench for why every metal haft asks for it
    // outright.
    Recipe {
        name: "sinewed axe",
        inputs: &[(BLOCK_STONE_AXE_HEAD, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_STONE_AXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "sinewed pick",
        inputs: &[(BLOCK_STONE_PICK_HEAD, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_STONE_PICKAXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "sinewed knife",
        inputs: &[(BLOCK_FLINT_KNIFE_HEAD, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_FLINT_KNIFE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the honest tool chain ----
    //
    // See `types::BLOCK_CORD`. Fibre is twisted into cord, a head is
    // ground and lashed, and only a head **wedged** into its haft takes
    // the shock of a tree.
    //
    // **The glue chain used to be here and is gone.** It was: tap a
    // standing trunk with a knife for resin, char a log for coal, cook
    // the two together at a fire, then set the lashed joint in the
    // result. Three items, one gesture and a fire, to buy one rung of
    // the ladder -- and the same rung is bought by driving a wooden peg
    // through the joint, which is one row, wants nothing the player is
    // not already making, and is what actually held a stone axe
    // together. Resin and glue are deleted; ids 137 and 138 are left
    // unused (see `types`).
    Recipe {
        // **Six fibre a cord, and fibre is slow.** A tuft gives fibre
        // one time in two (see the server's `spawn_block_drop`) and a
        // reed gives two, so a cord is a dozen tufts or three reeds --
        // which is what makes a riverbank the fibre country and a meadow
        // the place you make do. Kept deliberately dear: the cord is
        // the whole of what a stone tool *costs* once the head is a
        // cobble and two pebbles.
        name: "cord",
        inputs: &[(BLOCK_FIBER, 6)],
        output: (BLOCK_CORD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Four pegs from one worked stick**, and the stick is the dear
    // part: it already cost a flake and a branch. A peg is split off a
    // length of hardwood and shaved to a taper, which is the one
    // woodworking job in this game that yields *several* of a thing --
    // and it has to, because a roof is pegged at every joint and a
    // one-for-one peg would price a shelter out of the stone age. See
    // `types::BLOCK_PEG` for what driving one does.
    Recipe {
        name: "pegs",
        inputs: &[(BLOCK_WORKED_STICK, 1)],
        output: (BLOCK_PEG, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The lashed tool wedged: the head stops shifting in its seat**,
    // and the tool goes from `Tier::Stone` to `Tier::Flint` -- the
    // first axe that fells a tree and the first pick that breaks stone.
    // The haft is split at the eye, the head driven down it and two
    // pegs driven in beside it, which is how a stone axe was actually
    // held together and why it stops rattling loose.
    //
    // Takes the *finished* lashed tool rather than the parts, so the
    // step is an upgrade to a thing in the hand and not a second
    // parallel way of making an axe -- and the lashing stays on, which
    // is why the picture keeps its cord.
    Recipe {
        name: "wedged axe",
        inputs: &[(BLOCK_STONE_AXE, 1), (BLOCK_PEG, 2)],
        output: (BLOCK_WEDGED_AXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "wedged pick",
        inputs: &[(BLOCK_STONE_PICKAXE, 1), (BLOCK_PEG, 2)],
        output: (BLOCK_WEDGED_PICKAXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The knife's head on a long haft: the same knapped point, because
    // piercing is what flint is for, and a cord, because a point that
    // turns in its seat on the first thrust is a stick. What it is
    // worth is the server's `hunting_damage`.
    Recipe {
        name: "flint spear",
        inputs: &[(BLOCK_FLINT_KNIFE_HEAD, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (BLOCK_FLINT_SPEAR, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The spear before the flint one**, and the reason it exists is
    // the first evening: a bone off the first thing you butcher, a haft
    // and a sinew, no knapping and no failed rolls. It is worse than
    // flint at everything (see `blocks` for the forty thrusts and
    // `hunting_damage` for the thrust itself), which is what makes it a
    // rung rather than a shortcut -- a player who has one still wants
    // the flint one.
    // ---- fly agaric on a point ----
    //
    // **A poison that is one thrust long**, and it is four rows because
    // a recipe names one block: the paste goes on whichever spear you
    // have, and "whichever" is not a thing this table can say.
    //
    // What it costs is two toadstools, which are the one thing in the
    // world that is deliberately *not* food (`food::harm`): a player
    // who has been picking them by mistake all game finally has a use
    // for the pile. What it buys is a few seconds of poison after a hit
    // that lands (see the server's `poison`), which is not enough to
    // kill anything by itself -- it is what you put on before going
    // after a bear, so the fight is a little shorter than the bear
    // expected.
    //
    // The output is the *same spear* with a bit set in its variant
    // field (`types::poisoned`), so the paste survives a save, a chest
    // and a dropped stack, and needed no new item and no new picture.
    // One thrust takes it off again -- the server does that, because
    // the server is what knows the thrust landed.
    Recipe {
        name: "poison spear",
        inputs: &[(BLOCK_FLINT_SPEAR, 1), (BLOCK_TOADSTOOL, 2)],
        output: (BLOCK_FLINT_SPEAR | crate::types::POISONED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "poison bone",
        inputs: &[(BLOCK_BONE_SPEAR, 1), (BLOCK_TOADSTOOL, 2)],
        output: (BLOCK_BONE_SPEAR | crate::types::POISONED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "poison copper",
        inputs: &[(BLOCK_COPPER_SPEAR, 1), (BLOCK_TOADSTOOL, 2)],
        output: (BLOCK_COPPER_SPEAR | crate::types::POISONED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "poison bronze",
        inputs: &[(BLOCK_BRONZE_SPEAR, 1), (BLOCK_TOADSTOOL, 2)],
        output: (BLOCK_BRONZE_SPEAR | crate::types::POISONED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "poison iron",
        inputs: &[(BLOCK_IRON_SPEAR, 1), (BLOCK_TOADSTOOL, 2)],
        output: (BLOCK_IRON_SPEAR | crate::types::POISONED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bone spear",
        inputs: &[(BLOCK_BONE, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (BLOCK_BONE_SPEAR, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and the three metal ones, forged rather than cast, for exactly
    // the reason the knife is (see the note over "copper knife"): a
    // point is nothing but its thin part, and what cools in a mould is
    // coarse where it is thinnest.
    //
    // **A cord rather than a sinew, and that is not decoration.** The
    // metal knives are an ingot, a haft and a sinew; with the same
    // three the spear would be the *same recipe* as the knife, and a
    // hearth loaded with them would make whichever row came first --
    // which is exactly what happened, and what
    // `the_only_recipes_that_shadow_each_other_are_the_ones_that_should`
    // caught. A cord is also what the flint spear is bound with, for
    // the reason written there: a point that turns in its seat on the
    // first thrust is a stick.
    Recipe {
        name: "copper spear",
        inputs: &[(BLOCK_COPPER_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (BLOCK_COPPER_SPEAR, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze spear",
        inputs: &[(BLOCK_BRONZE_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (BLOCK_BRONZE_SPEAR, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron spear",
        inputs: &[(BLOCK_IRON_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (BLOCK_IRON_SPEAR, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The lashed pick, on the axe's terms: earth and gravel until it
        // is glued. "flint pick" until the head stopped being flint.
        name: "stone pick",
        inputs: &[
            (BLOCK_STONE_PICK_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_CORD, 1),
        ],
        output: (BLOCK_STONE_PICKAXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // **Ore melted in a pot and poured into a mould.**
        //
        // This row used to be "ore plus fuel, at a hot enough fire", and
        // that is not how anybody ever got copper out of rock. The
        // crucible and the mould are the equipment, not the ingredients:
        // both come back (see `Recipe::returns`), and getting the first
        // pair is what the whole clay chain above is for.
        //
        // A campfire will do it. The pot is what concentrates the heat,
        // which is exactly why the pot had to be invented -- and it is
        // also why the kiln is needed to *make* the pot but not to use
        // it.
        //
        // **Three ore and two charcoal, and it was two and one.** Malachite
        // is more than half copper, but ore off a hillside is a lump of rock
        // with the green running through it, and a crucible smelt burnt more
        // charcoal than it held ore -- one to three by weight in the
        // excavated furnaces. What is not copper comes out beside the ingot
        // as slag (`hearth::byproduct`), and the smelt takes two hours of the
        // world rather than eight seconds (`hearth::batch_seconds`).
        name: "copper ingot",
        inputs: &[
            (BLOCK_COPPER_ORE, 3),
            (BLOCK_COAL, 2),
            (BLOCK_VESSEL, 1),
            (BLOCK_MOULD, 1),
        ],
        output: (BLOCK_COPPER_INGOT, 1),
        station: Station::Heat,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "tin ingot",
        inputs: &[
            (BLOCK_TIN_ORE, 2),
            (BLOCK_COAL, 1),
            (BLOCK_VESSEL, 1),
            (BLOCK_MOULD, 1),
        ],
        output: (BLOCK_TIN_INGOT, 1),
        station: Station::Heat,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        // **Four to one, five out, and it was three to one with a loss.**
        // Three to one is a quarter tin: bell metal, which rings and
        // shatters, and it made tin the dearest thing in the bronze age by
        // a factor the history never had. Tool bronze was eight to twelve
        // per cent. Ten per cent here would be nine copper ingots in one
        // pot, which is a pour no crucible in this world holds and a first
        // bronze nobody reaches; four and one is a fifth tin, the hard end
        // of working bronze, and what the pot can take. The metal is
        // conserved -- five in, five out -- because nothing about a
        // mixture creates metal and nothing about this one loses it.
        //
        // At the kiln rather than at a campfire, and that is the one
        // place this world's fires are ranked by anything but heat:
        // melting a metal that is already metal is easy, and getting the
        // *proportion* right is not. An alloy poured at the wrong
        // temperature is a lump of the wrong thing.
        name: "bronze ingot",
        inputs: &[
            (BLOCK_COPPER_INGOT, 4),
            (BLOCK_TIN_INGOT, 1),
            (BLOCK_VESSEL, 1),
            (BLOCK_MOULD, 1),
        ],
        output: (BLOCK_BRONZE_INGOT, 5),
        station: Station::Forge,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        // **Iron does not go through the pot, and this is the row that
        // says so.** Copper melts at a temperature a crucible in a good
        // fire reaches; iron melts at half again as much, which nothing
        // in the ancient world reached. What a bloomery does is reduce
        // the ore *below* its melting point in a shaft of burning
        // charcoal, and what comes out is a bloom -- iron and slag
        // together, in a lump.
        //
        // **Three ore and four charcoal, eight hours of the world.** A
        // bloomery burnt its own weight of charcoal in ore and more, for six
        // to ten hours, and gave back a bloom of a fifth of what went in --
        // the rest is the slag that runs out beside it. It was two and three
        // in twenty seconds, which is a bloom a player watches rather than a
        // shaft they load at noon and come back to.
        name: "iron bloom",
        inputs: &[(BLOCK_IRON_ORE, 3), (BLOCK_COAL, 4)],
        output: (BLOCK_IRON_BLOOM, 1),
        station: Station::Bloomery,
        returns: &[],
        failure: 0.0,
    },
    // Bog iron: many rusty stones crushed to dust, smelted as long as
    // ore is. See `types::BLOCK_RUSTY_STONE`. Listed *after* the ore
    // bloom on purpose: `makes` answers with the first recipe for a
    // block, and the test that walks an ingot back to its ore walks
    // through it.
    Recipe {
        name: "iron dust",
        inputs: &[(BLOCK_RUSTY_STONE, 4)],
        output: (BLOCK_IRON_DUST, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bog iron bloom",
        inputs: &[(BLOCK_IRON_DUST, 3), (BLOCK_COAL, 4)],
        output: (BLOCK_IRON_BLOOM, 1),
        station: Station::Bloomery,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // ...and the bloom worked down to metal. In life this is an hour
        // at an anvil, beating the slag out while the lump is yellow;
        // here it is a second pass at the shaft, which is the shortest
        // honest way to say "the bloom is not the iron yet".
        name: "wrought iron",
        inputs: &[(BLOCK_IRON_BLOOM, 1), (BLOCK_COAL, 1)],
        output: (BLOCK_IRON_INGOT, 1),
        station: Station::Bloomery,
        returns: &[],
        failure: 0.0,
    },
    // ---- 1.5: the fire, what grows, and what the metal is for ----
    //
    // Appended rather than filed beside their relatives, for the reason
    // at the top of this list: the index into it is a recipe's identity
    // on the wire.
    Recipe {
        // The fire itself: a ring of stones and something to burn in it.
        //
        // Deliberately cheap, and deliberately not free. Cobble and
        // sticks are both lying on the ground in the first ten minutes
        // of a world, so a fire is available on the first evening --
        // which is the evening it matters. What it costs is a decision
        // about *where*: it is a block, it stays where you put it, and
        // everything hot you ever do happens next to it.
        name: "campfire",
        inputs: &[(BLOCK_COBBLESTONE, 3), (BLOCK_STICK, 4)],
        output: (BLOCK_CAMPFIRE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Reeds are a stand of fibre you can cut without stripping a
        // meadow of its grass one tuft at a time. Two for one, which is
        // twice what a tuft gives -- a reed is a metre of stem and a
        // tuft is a handful.
        name: "reed fibre",
        inputs: &[(BLOCK_REEDS, 1)],
        output: (BLOCK_FIBER, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Thatch from reeds, which is what thatch actually is. The
        // fibre recipe above is the general one; this is the shortcut
        // for somebody standing in a marsh with nothing else to do.
        name: "reed thatch",
        inputs: &[(BLOCK_REEDS, 4)],
        output: (BLOCK_DIRT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // **The recipe the whole of hunger turns on.** Meat over a fire
        // is worth four times meat in the hand (see `food::nutrition`),
        // and there is no other way to get it: this row is why a player
        // builds a fire, keeps it out of the rain, and goes looking for
        // wood at dusk.
        name: "cooked meat",
        inputs: &[(BLOCK_RAW_MEAT, 1)],
        output: (BLOCK_COOKED_MEAT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **Every meat cooks into the same cooked meat**, and the four rows
    // that do it are four rows because a recipe takes one list of
    // ingredients, not because the results differ. What differs is the
    // *raw* stage: a hare is a poor mouthful and a bear is the best one
    // in the world (`food::nutrition`), and the fire is what levels
    // them -- which is the honest thing for a fire to do, and it keeps
    // the pack from filling with four kinds of cooked haunch that mean
    // the same thing.
    Recipe {
        name: "cook hare",
        inputs: &[(BLOCK_HARE_MEAT, 1)],
        output: (BLOCK_COOKED_MEAT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cook fowl",
        inputs: &[(BLOCK_FOWL_MEAT, 1)],
        output: (BLOCK_COOKED_MEAT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cook bear",
        inputs: &[(BLOCK_BEAR_MEAT, 1)],
        output: (BLOCK_COOKED_MEAT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cook wolf",
        inputs: &[(BLOCK_WOLF_MEAT, 1)],
        output: (BLOCK_COOKED_MEAT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **Not cooked meat.** Roasted, a haunch of anybody's is a haunch -- and
    // this one keeps its own name and its own illness in the pack, so a
    // player can never forget which it was. See `types::BLOCK_HUMAN_FLESH`.
    Recipe {
        name: "roast flesh",
        inputs: &[(crate::types::BLOCK_HUMAN_FLESH, 1)],
        output: (crate::types::BLOCK_ROAST_HUMAN_FLESH, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **A fish cooks into cooked fish, not cooked meat**, which is the one
    // exception to the four rows above and has a reason: those four are
    // one haunch in four coats, and a fish is not a haunch -- it is a
    // different meal, eaten on a different shore, and the pack is allowed
    // to say so.
    // ---- salt, and what it keeps ----
    //
    // **Salt is boiled out of the sea, in a jug, at a fire**, and the jug
    // comes back (`types::BLOCK_SALT` for why not the solonchak's crust).
    // Only a jug of *sea* water: a river boiled dry leaves a jug with nothing
    // in it -- see `refuses`, which is where the kind of water is asked.
    Recipe {
        name: "boil salt",
        inputs: &[(BLOCK_JUG_WATER, 1)],
        output: (crate::types::BLOCK_SALT, 1),
        station: Station::Heat,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **Salting is by hand**, a haunch rubbed in a handful of salt, and the
    // salt is spent: it went into the meat. Two to a salt, because a
    // handful boiled out of a whole jug covers more than one cut -- and a
    // larder that cost a jug of the sea a haunch would be a chore.
    Recipe {
        name: "salt meat",
        inputs: &[(BLOCK_RAW_MEAT, 2), (crate::types::BLOCK_SALT, 1)],
        output: (crate::types::BLOCK_SALTED_MEAT, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "salt fish",
        inputs: &[(crate::types::BLOCK_RAW_FISH, 2), (crate::types::BLOCK_SALT, 1)],
        output: (crate::types::BLOCK_SALTED_FISH, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cook fish",
        inputs: &[(crate::types::BLOCK_RAW_FISH, 1)],
        output: (crate::types::BLOCK_COOKED_FISH, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **One rack of ribs per carcass, and it is the biggest meal in the
    // game.** Fourteen points against the cooked haunch's ten, because
    // it is more meat on more bone -- and it cannot be stockpiled the
    // way haunches can: the animal has exactly one ribcage.
    Recipe {
        name: "roast ribs",
        inputs: &[(BLOCK_RIBS, 1)],
        output: (BLOCK_ROASTED_RIBS, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **The standing torch: two hafts end to end, a wad of fibre, and resin
    // worked into the wad.** "Add resin from trees for the torch" -- and the
    // resin is what makes it a *standing* torch rather than a tall one: a
    // wad of grass burns out in forty-five seconds and drowns in the first
    // shower, and a torch stood beside a path has to last an evening and
    // take the weather (`wildfire::TORCH_SECONDS`). Two hafts rather than a
    // long pole of its own, because a pole is a thing the player already
    // knows how to make twice. Resin is scored off a standing trunk with a
    // knife; one lump re-wads a torch that has burnt out, where it stands.
    Recipe {
        name: "standing torch",
        inputs: &[(BLOCK_WORKED_STICK, 2), (BLOCK_FIBER, 1), (BLOCK_RESIN, 1)],
        output: (BLOCK_STANDING_TORCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Tallow on a stick.** The plain torch is a stick with a wad of
    // dry grass on it; smear the wad with rendered fat and one lump
    // makes two of them. That is what fat was actually for before
    // anybody had oil, and it is the reason to keep the fat off a bear
    // rather than eat it.
    Recipe {
        name: "fat torch",
        inputs: &[(BLOCK_STICK, 2), (BLOCK_FAT, 1)],
        output: (BLOCK_TORCH, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A spent torch re-wadded with tallow**, which is the row the
    // other two wads already had and this one did not: fibre and fungus
    // both re-wad a burnt-out stick, and fat -- the wad worth having --
    // could only be used on fresh sticks. So a player with a pocket of
    // spent torches and a lump of lard had to go and cut wood.
    //
    // One lump for two, the same rate as the fresh recipe, because the
    // saving is meant to be the *stick* and not the fat.
    Recipe {
        name: "fat rewad",
        inputs: &[(BLOCK_TORCH_SPENT, 2), (BLOCK_FAT, 1)],
        output: (BLOCK_TORCH, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The nine metal tools. Each is an ingot, a haft and a strap of
    // hide, worked at the fire -- one step, where a flint tool is four,
    // because casting *is* one step and knapping is not. See
    // `types::BLOCK_COPPER_KNIFE`.
    //
    // The metal cost climbs with the size of the head and not with the
    // age: a knife is one ingot at every tier and a pick is three. What
    // makes iron expensive is that iron ore is thirteen seconds of
    // hardness behind a copper pick, not that its recipes ask for more.
    //
    // The hide is what ties the age to the animals. Fibre held a flint
    // head on; it will not hold a metal one, and the strap comes off a
    // deer or a boar -- so a player who wants bronze has to have hunted
    // something bigger than a hare.
    // **The knife stays whole, and it is not an oversight.** A cast
    // edge is a bad edge: metal that cools in a mould is coarse and
    // brittle where it is thinnest, and a blade is nothing but its thin
    // part. What you do to a blade is beat it -- hammering closes the
    // grain and hardens it, which is the one thing casting cannot do.
    // So the knife is forged from the bar in a single row at the fire,
    // and the four things that are *mass* rather than edge -- an axe, a
    // pick, a shovel, a hoe -- are poured. See the copper bench below.
    Recipe {
        name: "copper knife",
        inputs: &[(BLOCK_COPPER_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_COPPER_KNIFE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // ---- 1.9: the copper bench ----
    //
    // **Two rows where there was one**, and the mould is the reason.
    // What comes out of a mould is a *casting*; a casting is not a tool
    // until it is on a stick. The single row asked for a worked stick
    // and a strap of hide and never showed you the piece they were
    // binding, which made them read as a tax on the ingot rather than
    // as the second half of a job. See `types::BLOCK_COPPER_AXE_HEAD`.
    //
    // **Why the crucible is in three of these four rows and not the
    // fourth**, which is the part that looks arbitrary and is not.
    // A hearth is not told which recipe to run: it *infers* one from
    // what is loaded into it (see `hearth::next_recipe`), and two
    // recipes with the same load are one recipe -- the second is
    // unreachable for ever, silently. Four castings that all read
    // "some copper and a mould" would be one casting. So each has to
    // ask for something the others do not, and the honest difference
    // between them is size: an axe, a pick and a shovel are poured from
    // a crucible, and a hoe blade is a finger of metal that goes
    // straight into the mould.
    //
    // That leaves a clean ladder -- hoe, shovel, axe, pick -- where each
    // asks for everything the one below it asks for and more, which is
    // exactly the relation the hearth already uses to tell a bronze axe
    // from an iron one. Load one ingot for a hoe blade and three for a
    // pick.
    //
    // The crucible and the mould both come back, as they do from every
    // other pour. **The metal price is unchanged**: a copper axe was two
    // ingots and is two ingots, a pick was three and is three. This
    // changes what the recipe *shows*, not what it charges.
    Recipe {
        name: "hoe casting",
        inputs: &[(BLOCK_COPPER_INGOT, 1), (BLOCK_MOULD, 1)],
        output: (BLOCK_COPPER_HOE_HEAD, 1),
        station: Station::Forge,
        returns: &[(BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "shovel casting",
        inputs: &[(BLOCK_COPPER_INGOT, 1), (BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        output: (BLOCK_COPPER_SHOVEL_HEAD, 1),
        station: Station::Forge,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "axe casting",
        inputs: &[(BLOCK_COPPER_INGOT, 2), (BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        output: (BLOCK_COPPER_AXE_HEAD, 1),
        station: Station::Forge,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "pick casting",
        inputs: &[(BLOCK_COPPER_INGOT, 3), (BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        output: (BLOCK_COPPER_PICK_HEAD, 1),
        station: Station::Forge,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },
    // The hafting. **By hand and away from the fire**, which is the half
    // of the job the forge was hiding: tying a head to a stick wants a
    // dry seat and a length of sinew, not a thousand degrees. It is also
    // the station the flint chain hafts at, and that parallel is the
    // point.
    //
    // **Sinew, not hide, and every metal haft is the same.** A strip of
    // hide was the lashing before there were carcasses; now an animal is
    // butchered and the tendon along its back is what a smith actually
    // lashed a head on with. It also makes the hunt matter to the
    // metalworker: a copper pick needs a deer as well as a mine. Flint
    // tools keep their fibre recipe and gain a sinew one beside it, so
    // the first axe still needs nothing but grass.
    Recipe {
        name: "copper axe",
        inputs: &[
            (BLOCK_COPPER_AXE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 1),
        ],
        output: (BLOCK_COPPER_AXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "copper pick",
        inputs: &[
            (BLOCK_COPPER_PICK_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 1),
        ],
        output: (BLOCK_COPPER_PICKAXE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "copper shovel",
        inputs: &[
            (BLOCK_COPPER_SHOVEL_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 1),
        ],
        output: (BLOCK_COPPER_SHOVEL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "copper hoe",
        inputs: &[
            (BLOCK_COPPER_HOE_HEAD, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 1),
        ],
        output: (BLOCK_COPPER_HOE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze knife",
        inputs: &[(BLOCK_BRONZE_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_BRONZE_KNIFE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze axe",
        inputs: &[(BLOCK_BRONZE_INGOT, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_BRONZE_AXE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze pick",
        inputs: &[(BLOCK_BRONZE_INGOT, 3), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_BRONZE_PICKAXE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron knife",
        inputs: &[(BLOCK_IRON_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 2)],
        output: (BLOCK_IRON_KNIFE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron axe",
        inputs: &[(BLOCK_IRON_INGOT, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 2)],
        output: (BLOCK_IRON_AXE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron pick",
        inputs: &[(BLOCK_IRON_INGOT, 3), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 2)],
        output: (BLOCK_IRON_PICKAXE, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // ---- 1.6: charcoal, the kiln, and brick ----
    //
    // Three rows that between them close the one hole in the ladder:
    // there was no fuel a player could *make*, and no fire hot enough to
    // melt anything. Appended rather than filed beside their relatives,
    // for the reason at the top of this list.
    Recipe {
        // **The fuel that comes before mining.** Coal used to be the
        // only fuel there was, and coal is a seam some way underground
        // behind a flint pick -- so the first metal a player smelted was
        // gated on a mine, which is backwards. Charcoal is wood burnt
        // slowly in its own smoke, it is what every furnace in the
        // ancient world ran on, and it comes out of the campfire a
        // player already has on their first evening.
        //
        // Three logs for one, because that is roughly what a charcoal
        // burn actually yields, and because mined coal has to stay worth
        // the walk: a seam is one swing for one lump.
        name: "charcoal",
        inputs: &[(BLOCK_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The kiln itself: clay dug from a riverbank, daubed over a
        // footing of stone.
        //
        // Deliberately expensive in a material that is only found in one
        // place. A campfire is what the world gives you; a kiln is
        // somewhere you went and something you carried back, and that
        // walk is what makes the bronze on the other side of it feel
        // like an age rather than a menu entry.
        name: "kiln",
        inputs: &[(BLOCK_CLAY, 8), (BLOCK_COBBLESTONE, 4)],
        output: (BLOCK_KILN, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Clay fired hard. Four at a time because a kiln is loaded and
        // lit, not fed one brick at a time.
        name: "bricks",
        inputs: &[(BLOCK_CLAY, 4)],
        output: (BLOCK_BRICK, 4),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // **Mud mortar**, the first there was: clay beaten into sand. This
        // slot made a cube of brickwork out of four bricks, and a wall of
        // bricks is laid now, a course at a time where it stands (`build`);
        // the row kept its place because its place is its name on the wire,
        // and what it makes is the other half of a course.
        name: "clay mortar",
        inputs: &[(crate::types::BLOCK_HANDFUL_CLAY, 2), (crate::types::BLOCK_HANDFUL_SAND, 1)],
        output: (crate::types::BLOCK_MORTAR, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Sandstone squared along its bedding and laid in courses: two
        // rough blocks to one of bricks, and no fire. See
        // `types::BLOCK_SANDSTONE_BRICKS`.
        name: "sand brickwork",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 2)],
        output: (crate::types::BLOCK_SANDSTONE_BRICKS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the field ----
    //
    // Three rows, and between them the first food in this world that a
    // player *makes* rather than finds. See `types::BLOCK_SEEDS`.
    Recipe {
        // A blade of flint lashed across the end of a haft rather than
        // along it -- which is the only difference between this and a
        // knife, and is exactly the difference between cutting and
        // turning earth.
        //
        // Cheap on purpose. The hoe is not an achievement, it is the
        // thing you need before the interesting part starts, and a
        // player who has knapped one knife has everything this asks for.
        name: "hoe",
        inputs: &[(BLOCK_FLINT_FLAKE, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_FIBER, 3)],
        output: (BLOCK_HOE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The same tool with a bone blade: the digging stick every people
    // without flint had. What a butchered animal leaves that is not
    // food goes into the ground it was hunted on, and a player who
    // settles far from any flint still gets a field.
    Recipe {
        name: "bone hoe",
        inputs: &[(BLOCK_BONE, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_FIBER, 3)],
        output: (BLOCK_HOE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Bread is three steps now, and the middle one is the point.**
    //
    // It used to be two: three grain became dough in the hand, and
    // dough became bread at a fire. The comment on that row said what
    // it was -- threshing, grinding and wetting collapsed together
    // because a quern would have been another block, another station
    // and another screen. That argument is still right about a quern
    // and it was wrong about the water.
    //
    // Grinding is done with a stone the player already carries and the
    // stone comes back, so the step costs a row in the menu and nothing
    // else. What it buys is that **dough needs water**, and water means
    // the jug -- which until now was a thing you filled for thirst and
    // for no other reason in the game. The jug comes back empty, so a
    // baker's loop is: fill at the river, grind, wet, bake.
    Recipe {
        name: "grind grain",
        inputs: &[(BLOCK_GRAIN, 3), (BLOCK_PEBBLE, 1)],
        output: (BLOCK_FLOUR, 2),
        station: Station::Hands,
        // The stone is a tool, not an ingredient.
        returns: &[(BLOCK_PEBBLE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "dough",
        inputs: &[(BLOCK_FLOUR, 2), (BLOCK_JUG_WATER, 1)],
        output: (BLOCK_DOUGH, 1),
        station: Station::Hands,
        // ...and so is the jug. What is spent is what was in it.
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    Recipe {
        // Baked. A campfire will do it -- bread wants an oven's heat,
        // not a furnace's, and the kiln has enough to do.
        name: "bread",
        inputs: &[(BLOCK_DOUGH, 1)],
        output: (BLOCK_BREAD, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // ---- pottery, which is to say the copper age ----
    //
    // Five rows, and between them the reason the ceramic age and the
    // copper age are the same age: the pot *is* the smelting equipment.
    // See `types::BLOCK_NATIVE_COPPER`.
    Recipe {
        // Thrown wet, by hand, out of five clay. Nothing is fired yet --
        // this is the shape, and it will fall apart in the rain.
        //
        // **Four clay and a sand, and it was five clay.** A crucible is the
        // one pot that is heated to a thousand degrees with metal in it, and
        // pure clay shrinks and cracks long before that; every crucible ever
        // dug up is clay opened with grit. Sand is what lies beside the clay
        // on the same bank, so the temper costs a handful, not a trip.
        name: "clay vessel",
        inputs: &[(BLOCK_CLAY, 4), (BLOCK_SAND, 1)],
        output: (BLOCK_VESSEL_RAW, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // ...and hardened. The kiln's real job, and the reason a player
        // builds one before they have any metal to put in it.
        //
        // **No charcoal in it any more.** The lump used to be an
        // ingredient, from before a fire had a temperature: it was the only
        // way to say "this needs a hot fire". The fuel slot says that now
        // (`hearth::needs_degrees`), and a pot that ate a lump of charcoal
        // as if it were clay was an ingredient nobody could have put in a
        // pot. What firing costs instead is three hours of the kiln
        // (`hearth::batch_seconds`) and the wood that burns through them.
        name: "fire vessel",
        inputs: &[(BLOCK_VESSEL_RAW, 1)],
        output: (BLOCK_VESSEL, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The other half of the pair: a trough of the right size for one
        // ingot. Cheaper than the pot, because it is a shape pressed into
        // a slab rather than a vessel that has to hold molten metal.
        name: "ingot mould",
        inputs: &[(BLOCK_CLAY, 3)],
        output: (BLOCK_MOULD_RAW, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fire mould",
        inputs: &[(BLOCK_MOULD_RAW, 1)],
        output: (BLOCK_MOULD, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // The shaft: stone lined with fired brick. Built rather than
        // fired, like the kiln -- and expensive in brick, which means it
        // is built by somebody who already has a kiln and has been
        // running it. That is the correct order: pottery, then copper,
        // then the thing that wins iron.
        name: "bloomery",
        // Four courses of brick in mortar, as the wall it was built of
        // before a cube of brickwork stopped being a thing in the pack.
        inputs: &[(BLOCK_BRICK, 16), (crate::types::BLOCK_MORTAR, 4), (BLOCK_COBBLESTONE, 8)],
        output: (BLOCK_BLOOMERY, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        // Native copper is metal already: it wants melting, not
        // smelting, so there is no ore to reduce and no charcoal spent
        // on reducing it. Two nodules to the ingot, the same as ore --
        // what a player saves is the mine, not the pouring.
        name: "melt nuggets",
        inputs: &[
            (BLOCK_NATIVE_COPPER, 2),
            (BLOCK_VESSEL, 1),
            (BLOCK_MOULD, 1),
        ],
        output: (BLOCK_COPPER_INGOT, 1),
        station: Station::Heat,
        returns: &[(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)],
        failure: 0.0,
    },

    // ---- the tannery ----
    //
    // Two rows only, and the gap between them is the point: there is no
    // recipe that turns a hide into leather. That is the drying rack's
    // job, and it takes hours of world time in weather that is neither
    // wet nor freezing -- see `primitive_server::drying`. What is here
    // is the frame you build and the things you cut out of what comes
    // off it.
    Recipe {
        // Sticks lashed with fibre. Cheap on purpose: a tannery is a row
        // of these, and a rack that cost anything real would mean one
        // rack and a queue.
        //
        // **Six and three since it stands two cells long and two tall**
        // (`types::rack_cells`): two poles to an A at each end and a ridge
        // of two lengths between them, lashed at the crotches and the seam.
        // Still no board and no tool -- a rack is the first thing a hunter
        // builds, and poles and cord are what a hunter has.
        name: "drying rack",
        inputs: &[(BLOCK_STICK, 6), (BLOCK_FIBER, 3)],
        output: (BLOCK_DRYING_RACK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- what leather is for ----
    //
    // Four garments, and they are the *first* clothing there is, which
    // is why they cost leather and fibre and nothing else. A player who
    // has killed a boar, built a rack and waited has everything they
    // need for a coat, and no metal at all.
    Recipe {
        name: "hide cap",
        inputs: &[(BLOCK_LEATHER, 2), (BLOCK_FIBER, 1)],
        output: (BLOCK_LEATHER_CAP, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "hide tunic",
        inputs: &[(BLOCK_LEATHER, 5), (BLOCK_FIBER, 2)],
        output: (BLOCK_LEATHER_TUNIC, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "hide leggings",
        inputs: &[(BLOCK_LEATHER, 4), (BLOCK_FIBER, 2)],
        output: (BLOCK_LEATHER_LEGGINGS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "hide boots",
        inputs: &[(BLOCK_LEATHER, 3), (BLOCK_FIBER, 1)],
        output: (BLOCK_LEATHER_BOOTS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- furniture, and the straw before it ----
    //
    // **The straw bed is the important row here**, because it is the
    // one a player can make in their first hour: five handfuls of dry
    // grass and two sticks to hold it, by hand, with no fire and no
    // tools at all. Sleep is a mechanic that must not be gated behind a
    // tech tree -- a player who cannot sleep until they have planks and
    // a tannery is a player who spends their first three nights
    // standing in a hole, which is exactly the night this was added to
    // fix. The bed is the *upgrade*: it takes the whole of a night's
    // tiredness where straw leaves a fifth (see `body::Rest`).
    Recipe {
        name: "straw bed",
        inputs: &[(BLOCK_FIBER, 5), (BLOCK_STICK, 2)],
        output: (BLOCK_STRAW_BED, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The rest of the furniture is joinery, and it goes through a frame.**
    // The stool, the chair, the table and the bed used to be planks and
    // sticks by hand, which made a furnished house the first thing a player
    // could build and put nothing between a camp and a home but a log.
    // What stands between them now is the thing that actually did: a joint
    // that holds weight.
    //
    // * **A frame** (`types::BLOCK_FRAME`) is four rails and their joints,
    //   and it is the one intermediate. It is made one of two ways -- four
    //   *worked* sticks and four pegs ("pegged frame"), or four plain sticks
    //   and four nails ("nailed frame", at the end of the table) -- and it
    //   comes out the same frame. The bed, the chair, the table and the
    //   joined chest take a frame and never ask how it was held.
    // * **The stool is pegged and takes no frame**: three legs wedged into a
    //   slab is the whole of a stool, and it stays the camp's seat, made
    //   the day there are pegs.
    // * **The chest is the one piece made both ways**, because it is the one
    //   a player wants several of. The joined chest ("chest") is a pegged
    //   frame boarded in and pinned -- about six worked sticks of flint work
    //   in all. The nailed chest is six boards butted and nailed, no frame:
    //   two thirds of a bar of iron and no flint at all.
    // * **The barrel stays bound with cord.** A nail through a stave is a
    //   leak, and the thing that replaced cord on a barrel was an iron hoop,
    //   which is a smithing chain of its own and not this one.
    //
    // **What that decides.** Before iron there is one way, and it is flint
    // and time: every frame is five knapped flakes and a morning. After
    // iron there are two, and they spend different things -- a nailed
    // frame spends a third of a bar, and that bar is also a knife, a third
    // of a helm, or a steeled edge. A player furnishing one room pegs it;
    // a player with a store of twenty chests to fill nails them, and a
    // player saving for armour goes back to knapping. Neither is the right
    // answer everywhere, which is the test a mechanic has to pass here.
    //
    // Rejected, and why:
    //
    // * **Boards, legs and rails as items of their own.** Three new things
    //   in the pack, each made from planks by hand with nothing else in the
    //   recipe, is three extra clicks and no extra decision -- a chore,
    //   exactly. A frame earns its place because the fastening choice lives
    //   in it; a board would only be a plank with a longer name.
    // * **A pegged chest that holds less.** The honest version of "the
    //   early chest is worse", and not done: a chest's size is the
    //   inventory's forty slots in the server's move rules, the mod API's
    //   slot count and the chest screen alike, and a second size is a
    //   second container in all three. The joined chest costs more instead,
    //   in the material the stone age has least of, which is flint work.
    // * **Nails in every row.** It would put a bed and a table behind the
    //   bloomery, which is the whole iron age behind a good night's sleep.
    //   Pegged joinery furnished houses for thousands of years before a
    //   nail was cheap; iron here is a second way, not the only one.
    //
    // **The bed, the chair, the table and the door are made at the joiner's
    // bench** (`Station::Bench`), and this is where a refusal used to stand:
    // "a carpenter's bench as a station -- a new block and a new flag for
    // work that needs a knife and a flat place, which is anywhere". A player
    // asked for the bench anyway, and the refusal was wrong about the work.
    // A frame is a knife and a flat place; hanging boards square on it so a
    // door two cells tall swings without racking, or so a bed carries a
    // sleeper, is the thing a bench is for. So the frame, the stool, the
    // straw bed, both chests and the barrel stay in the hands -- the camp's
    // pieces, made where the camp is -- and the four pieces that make a
    // house wait for the bench that is built in one. The bench costs a log,
    // sticks and pegs: stone-age work, on the stone-age side of the ladder.
    Recipe {
        name: "bed",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 3), (BLOCK_LEATHER, 2)],
        output: (BLOCK_BED, 1),
        station: Station::Bench,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "stool",
        inputs: &[(BLOCK_PLANKS, 2), (BLOCK_STICK, 3), (BLOCK_PEG, 3)],
        output: (BLOCK_STOOL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The chair is a frame with a seat on it and a back: two boards and two
    // poles for a seat that faces a way and rests half again as fast (see
    // `body::sitting_recovery`) -- and a frame more than the stool, which
    // is what makes it the house's seat rather than the camp's.
    Recipe {
        name: "chair",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 2), (BLOCK_STICK, 2)],
        output: (BLOCK_CHAIR, 1),
        station: Station::Bench,
        returns: &[],
        failure: 0.0,
    },
    // A frame for the apron, four legs under it and four boards over it.
    Recipe {
        name: "table",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 4), (BLOCK_STICK, 4)],
        output: (BLOCK_TABLE, 1),
        station: Station::Bench,
        returns: &[],
        failure: 0.0,
    },
    // **A door is a frame boarded over**: the frame is the ledge-and-brace
    // that keeps boards two cells tall from racking, and four boards are
    // what covers it. The frame is where the fastening was chosen, as it is
    // for the chair, so a door is pegged before there is iron and nailed
    // after without a second row -- and boards of any wood hang it (see
    // "any wood" below), so a fir house has a fir door.
    //
    // Rejected: *a door of planks and sticks by hand, before the frame*.
    // A door is what makes a shelter a house -- smoke that stays in, a wolf
    // that stays out -- and a house is the thing the frame is the price of.
    // A first night is spent behind a wall of dirt, and that is a choice the
    // stone age already has.
    Recipe {
        name: "door",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 4)],
        output: (crate::types::BLOCK_DOOR, 1),
        station: Station::Bench,
        returns: &[],
        failure: 0.0,
    },
    // Staves are boards, and before there was iron to hoop a barrel
    // with it was bound with cord. Nothing here needs a bench or a fire:
    // the reason to make one is a camp far from the river, and that camp
    // is usually a first one. No nails, for the reason above.
    Recipe {
        name: "barrel",
        inputs: &[(BLOCK_PLANKS, 6), (BLOCK_CORD, 2)],
        output: (BLOCK_BARREL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- what a raw pelt is for ----
    //
    // **The coat you make on the day you kill something**, and the four
    // rows are two garments with two materials each.
    //
    // Everything above this needs a rack and a wait: a skin becomes
    // leather on the drying rack (`rack::cures_into`) and a player's
    // first cold night comes long before their first tannery. Sinew
    // rather than fibre, because sinew comes off the same animal --
    // this is the one clothing recipe that asks for nothing a meadow
    // grows, which is what makes it the answer in the places that have
    // no meadow.
    //
    // A bear does it in one. Its hide is the biggest single piece of
    // skin in the world, and a player who has killed a bear has earned
    // a coat out of it without going back for a second one.
    //
    // What this deliberately does *not* do is make the tannery
    // pointless: fur is warmer than leather and worse at everything
    // else -- heavier, stiffer, wetter, and it wears out sooner. See
    // `equipment::garment`.
    Recipe {
        name: "fur hood",
        inputs: &[(BLOCK_PELT, 2), (BLOCK_SINEW, 1)],
        output: (BLOCK_FUR_HOOD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bear hood",
        inputs: &[(BLOCK_BEAR_HIDE, 1), (BLOCK_SINEW, 1)],
        output: (BLOCK_FUR_HOOD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fur cloak",
        inputs: &[(BLOCK_PELT, 4), (BLOCK_SINEW, 2)],
        output: (BLOCK_FUR_CLOAK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bear cloak",
        inputs: &[(BLOCK_BEAR_HIDE, 2), (BLOCK_SINEW, 2)],
        output: (BLOCK_FUR_CLOAK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- what wool is for ----
    //
    // The same four slots again, and deliberately *cheaper* than the
    // leather set in everything but the material.
    //
    // Leather costs a hunt, a rack and a wait. Wool costs shearing a
    // sheep, which is a thing a player does on the way past. That is
    // the right way round: wool is not an upgrade over leather, it is a
    // different trade -- much warmer, no protection, and ruined by rain
    // (see `equipment::garment`) -- and a trade the player should be
    // able to take early, while the cold is still the thing most likely
    // to kill them.
    //
    // Fibre in every row for the same reason it is in the leather ones:
    // something has to hold the pieces together, and it is the one
    // material this world has that is thread.
    Recipe {
        name: "wool cap",
        inputs: &[(BLOCK_WOOL, 2), (BLOCK_FIBER, 1)],
        output: (BLOCK_WOOL_CAP, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "wool tunic",
        inputs: &[(BLOCK_WOOL, 5), (BLOCK_FIBER, 2)],
        output: (BLOCK_WOOL_TUNIC, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "wool leggings",
        inputs: &[(BLOCK_WOOL, 4), (BLOCK_FIBER, 2)],
        output: (BLOCK_WOOL_LEGGINGS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "wool boots",
        inputs: &[(BLOCK_WOOL, 3), (BLOCK_FIBER, 1)],
        output: (BLOCK_WOOL_BOOTS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- what cotton is for ----
    //
    // **Four bolls to a bolt, in the hands.** Spinning and weaving are two
    // crafts and two tools -- a spindle and a loom -- and this row is both
    // of them folded into one, the way threshing, grinding and wetting
    // were once folded into dough. Until there is a loom worth building, a
    // station here would be a block that exists to be stood next to, and
    // standing next to something is not a decision. A loom that arrives
    // later should take this row over and make it cheaper, which is what
    // a tool is for.
    //
    // Four because a ripe plant gives two: a bolt is two plants, and a
    // tunic is ten -- a field, not a patch by the door.
    Recipe {
        name: "cloth",
        inputs: &[(BLOCK_COTTON, 4)],
        output: (BLOCK_CLOTH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The four garments mirror the wool set's counts exactly -- cloth where
    // the fleece was, fibre for the thread -- so the two sets cost the same
    // to sew and differ only in what they are *for*, which is the
    // comparison a player should be making.
    Recipe {
        name: "cloth cap",
        inputs: &[(BLOCK_CLOTH, 2), (BLOCK_FIBER, 1)],
        output: (BLOCK_CLOTH_CAP, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cloth tunic",
        inputs: &[(BLOCK_CLOTH, 5), (BLOCK_FIBER, 2)],
        output: (BLOCK_CLOTH_TUNIC, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cloth trousers",
        inputs: &[(BLOCK_CLOTH, 4), (BLOCK_FIBER, 2)],
        output: (BLOCK_CLOTH_TROUSERS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cloth wraps",
        inputs: &[(BLOCK_CLOTH, 3), (BLOCK_FIBER, 1)],
        output: (BLOCK_CLOTH_WRAPS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- armour ----
    //
    // Beaten at a lit hearth rather than cast, which is why the station
    // is `Heat` and not `Forge`: an ingot is already metal, and what
    // turns one into a plate is a fire and a hammer rather than a
    // crucible. The leather in every row is the lining, and it is what
    // keeps armour from being wearable by anybody who has not first
    // learned to tan a skin -- the two chains meet here on purpose.
    Recipe {
        name: "bronze helm",
        inputs: &[(BLOCK_BRONZE_INGOT, 2), (BLOCK_LEATHER, 1)],
        output: (BLOCK_BRONZE_HELM, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "br. cuirass",
        inputs: &[(BLOCK_BRONZE_INGOT, 5), (BLOCK_LEATHER, 2)],
        output: (BLOCK_BRONZE_CUIRASS, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "br. greaves",
        inputs: &[(BLOCK_BRONZE_INGOT, 4), (BLOCK_LEATHER, 1)],
        output: (BLOCK_BRONZE_GREAVES, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze boots",
        inputs: &[(BLOCK_BRONZE_INGOT, 3), (BLOCK_LEATHER, 1)],
        output: (BLOCK_BRONZE_BOOTS, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron helm",
        inputs: &[(BLOCK_IRON_INGOT, 2), (BLOCK_LEATHER, 1)],
        output: (BLOCK_IRON_HELM, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron cuirass",
        inputs: &[(BLOCK_IRON_INGOT, 5), (BLOCK_LEATHER, 2)],
        output: (BLOCK_IRON_CUIRASS, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron greaves",
        inputs: &[(BLOCK_IRON_INGOT, 4), (BLOCK_LEATHER, 1)],
        output: (BLOCK_IRON_GREAVES, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron boots",
        inputs: &[(BLOCK_IRON_INGOT, 3), (BLOCK_LEATHER, 1)],
        output: (BLOCK_IRON_BOOTS, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },

    // ---- the jug ----
    //
    // The pottery chain's third product, and the only one that is not
    // about metal. Thrown wet like the crucible and the mould, and fired
    // in the same kiln -- so a player who has already made a pot to melt
    // copper in has, without knowing it, learned how to cross a desert.
    Recipe {
        name: "clay jug",
        inputs: &[(BLOCK_CLAY, 4)],
        output: (BLOCK_JUG_RAW, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fire jug",
        inputs: &[(BLOCK_JUG_RAW, 1)],
        output: (BLOCK_JUG, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },

    // ---- what a felled tree is for ----
    //
    // The same four planks a log gives, from timber that has already had
    // its bark taken off. Appended rather than folded into the `planks`
    // row above it, because a recipe's *index* is its identity on the
    // wire -- see the note on `RECIPES`.
    Recipe {
        name: "bare planks",
        inputs: &[(BLOCK_STRIPPED_LOG, 1)],
        output: (BLOCK_PLANKS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- taking the bark off ----
    //
    // **Where stripped timber comes from now.** It used to be what a
    // felled tree left: the fall took the bark with it. That rule went
    // (see `primitive_server::felling` -- a tree that landed as
    // different wood from the deadfall beside it was a tree that had
    // forgotten what it was), and without these two rows the block and
    // the recipe above it would have had no source at all.
    //
    // Two rows because there are two woods and **one** result, which is
    // the honest part of the old rule kept: once the bark is off there
    // is nothing left to tell an oak from a birch.
    Recipe {
        name: "strip a log",
        inputs: &[(BLOCK_LOG, 1)],
        output: (BLOCK_STRIPPED_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "strip birch",
        inputs: &[(BLOCK_BIRCH_LOG, 1)],
        output: (BLOCK_STRIPPED_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- what a fire does to a root ----
    //
    // The second customer the campfire ever had, and the argument is the
    // one the meat already makes: a root out of the ground is a
    // mouthful, and a root out of a fire is most of a meal (see
    // `food::nutrition`). What it buys that the meat does not is that a
    // player who has killed nothing all day still has a reason to build
    // one.
    //
    // `Heat` rather than `Forge`: this is a root in the embers, not
    // something that has to be melted.
    Recipe {
        name: "roast a root",
        inputs: &[(BLOCK_ROOT, 1)],
        output: (BLOCK_ROASTED_ROOT, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },

    // ---- what a wound is dressed with ----
    //
    // See `injury` for what each one is for. All four rows are `Hands`
    // and all of them are stone-age materials, because a player is hurt
    // long before they have a fire hot enough for anything: a dressing
    // that needed a kiln would be a dressing for the second week, and the
    // wolf comes in the first.
    //
    // **Two bandage rows and one bandage.** Fibre is the first evening's,
    // grass twisted into a strip, and it is dear -- three fibre for one. A
    // bolt of cloth makes three. What the cotton field buys is *more*
    // bandages, not better ones: a bandage stops a cut bleeding or it does
    // not, and a "good" and a "poor" one would be a second item with one
    // correct answer.
    Recipe {
        name: "bandage",
        inputs: &[(BLOCK_FIBER, 3)],
        output: (BLOCK_BANDAGE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cloth bandages",
        inputs: &[(BLOCK_CLOTH, 1)],
        output: (BLOCK_BANDAGE, 3),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // Two sticks with fibre round them -- **loose grass on purpose**, and
    // the one binding in the table that is. The cord rule
    // (`a_cord_is_six_fibre_and_a_reed_bank_is_where_fibre_is`) is about
    // a head that must not work loose from a haft under a blow; a splint
    // is tied once and left alone, and costing it a cord would make the
    // answer to a broken leg dearer than the knife that butchers the boar
    // that broke it.
    Recipe {
        name: "splint",
        inputs: &[(BLOCK_STICK, 2), (BLOCK_FIBER, 2)],
        output: (BLOCK_SPLINT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // The fungus off a fallen trunk, pounded and tied on with a strand.
    // The dark woods grow no grass, so this is also the one dressing a
    // player caught out in the taiga can nearly make there -- one fibre
    // short, which is a thing to have carried in.
    Recipe {
        name: "poultice",
        inputs: &[(BLOCK_BRACKET_FUNGUS, 1), (BLOCK_FIBER, 1)],
        output: (BLOCK_POULTICE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },

    // ---- the raft ----
    //
    // **Worked towards, and the cost is the point.** A raft is the first
    // thing that takes a player where they could not walk, and it was asked
    // for as a hard-won thing: a great many planks, sticks, cords and
    // hides, and a sail and oars besides. So the two parts are made first,
    // each a row of its own -- a sail is four leathers, which is four skins
    // through the drying rack -- and the raft is lashed out of them and a
    // heap of timber, cord and rawhide. Rawhide rather than leather for the
    // lashing, because that is what lashed logs: it shrinks as it dries and
    // pulls the knots tight.
    //
    // **By hand and anywhere, not at the water's edge.** A station for
    // "beside water" was the other shape, and it was rejected. Stations are
    // carried end to end (`Heat`: the client works them out from its chunks
    // and the server from its fire map), and a fourth would be one more
    // fact both sides must agree on -- for a rule the launch already
    // enforces, since a raft only goes onto water. What makes the place
    // matter instead is the weight (`types::BLOCK_RAFT`): lash it in the
    // woods and haul a hundred and fifty kilos to the shore, or carry the
    // timber down first. That is a decision; a menu row that greys out ten
    // metres from a lake is a rule to be discovered.
    //
    // **Two sail rows and one sail**, the bandages' argument: leather from
    // the hunt or cloth from the cotton field, and neither sail is better.
    Recipe {
        name: "sail",
        inputs: &[(BLOCK_LEATHER, 4), (BLOCK_CORD, 2), (BLOCK_STICK, 2)],
        output: (BLOCK_SAIL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cloth sail",
        inputs: &[(BLOCK_CLOTH, 4), (BLOCK_CORD, 2), (BLOCK_STICK, 2)],
        output: (BLOCK_SAIL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "oar",
        inputs: &[(BLOCK_PLANKS, 2), (BLOCK_STICK, 2)],
        output: (BLOCK_OAR, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "raft",
        inputs: &[
            (BLOCK_PLANKS, 16),
            (BLOCK_STICK, 8),
            (BLOCK_CORD, 8),
            (BLOCK_HIDE, 2),
            (BLOCK_SAIL, 1),
            (BLOCK_OAR, 2),
        ],
        output: (BLOCK_RAFT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the brick, before it is fired ----
    //
    // **A raw state for the one piece of pottery that had none.** The kiln
    // made bricks straight out of clay, which was fine while the kiln was
    // the only fire that fired anything. A pit kiln is fired by what is
    // laid in it (`pit`), and clay in the hand is a block a right click
    // builds with -- so a brick has to be shaped first to be something you
    // can lay in a pit. The kiln's own "bricks" row is left as it was: a
    // player with a kiln need not shape what the kiln shapes for them.
    Recipe {
        name: "raw bricks",
        inputs: &[(BLOCK_CLAY, 4)],
        output: (BLOCK_BRICK_RAW, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and fired in a kiln as well as in a pit, because a raw brick a
    // kiln refused would be a brick a player shaped for a kiln they
    // already had and could not use.
    Recipe {
        name: "fire bricks",
        inputs: &[(BLOCK_BRICK_RAW, 4)],
        output: (BLOCK_BRICK, 4),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // ---- the wild plants ----
    //
    // **The meadow's poultice**: two plantain leaves bruised and tied on with
    // a strand, the fungus poultice's dressing from the other end of the
    // country. The fungus grows on fallen trunks in the dark woods; plantain
    // grows where the ground is open and trodden. So a wound is dressed with
    // what is near, and a player learns which is near from where they are --
    // rather than one recipe that sends everybody into the same wood.
    //
    // Appended, not beside the fungus row: a recipe is sent by its place in
    // this table, and one inserted in the middle renumbers everything after.
    Recipe {
        name: "leaf poultice",
        inputs: &[(BLOCK_PLANTAIN, 2), (BLOCK_FIBER, 1)],
        output: (BLOCK_POULTICE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- charcoal, steel and the edge ----
    //
    // Appended for the reason every row is: a recipe travels as its place in
    // this table. See `tools` for the edge and the steel,
    // `hearth::batch_seconds` for how long each of these takes and
    // `hearth::byproduct` for the slag.
    //
    // **Birch chars as oak does.** The campfire row asked for an oak log and
    // nothing else, while the charcoal pit took either -- so the same wood
    // was fuel for a pile and not for a fire, which no charcoal burner would
    // recognise: the birch woods of the north were coaled for iron for a
    // thousand years. A second row is how this table says "either".
    Recipe {
        name: "birch charcoal",
        inputs: &[(BLOCK_BIRCH_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **A whetstone is a bar of sandstone**, and a block of sandstone is a
    // great many bars; two is the handful a person splits off and rubs flat.
    // It comes back from every hone below, so the cost of the edge is never
    // the stone: it is the metal (`tools::hone`).
    Recipe {
        name: "whetstone",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 1)],
        output: (crate::types::BLOCK_WHETSTONE, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Cementation: a bar packed in charcoal in a closed kiln until the
    // carbon has gone into it.** Two charcoal a bar, because the packing is
    // burnt through, and five minutes of kiln, a working day of the world.
    // In the kiln rather than the bloomery: a bloom wants a draught and a bar
    // wants to be shut away from one, and a closed clay box is what a kiln
    // already is. What comes out is steel that is still soft -- the hardness
    // is the quench, in the tool rows under this one.
    Recipe {
        name: "steel",
        inputs: &[(BLOCK_IRON_INGOT, 1), (BLOCK_COAL, 2)],
        output: (crate::types::BLOCK_STEEL_INGOT, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **Forged from steel and quenched.** The iron tools' own rows, with
    // steel bars where the iron was and a jug of water to plunge the edge
    // into. What comes out is the *iron* tool with its hardened bit set
    // (`tools::HARDENED`) -- the same axe, steeled, which is what a steeled
    // axe is -- and the jug comes back empty. Any water quenches: a smith's
    // brine was salt on purpose.
    //
    // Rejected: a separate quench row taking a finished iron tool. A hearth
    // counts its ingredients by kind and hands back fresh stacks, so it
    // would mend the tool's wear on the way through, and a player would
    // steel a worn-out axe to get a new one.
    Recipe {
        name: "steel knife",
        inputs: &[
            (crate::types::BLOCK_STEEL_INGOT, 1),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 2),
            (BLOCK_JUG_WATER, 1),
        ],
        output: (BLOCK_IRON_KNIFE | crate::tools::HARDENED, 1),
        station: Station::Forge,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "steel axe",
        inputs: &[
            (crate::types::BLOCK_STEEL_INGOT, 2),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 2),
            (BLOCK_JUG_WATER, 1),
        ],
        output: (BLOCK_IRON_AXE | crate::tools::HARDENED, 1),
        station: Station::Forge,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "steel pick",
        inputs: &[
            (crate::types::BLOCK_STEEL_INGOT, 3),
            (BLOCK_WORKED_STICK, 1),
            (BLOCK_SINEW, 2),
            (BLOCK_JUG_WATER, 1),
        ],
        output: (BLOCK_IRON_PICKAXE | crate::tools::HARDENED, 1),
        station: Station::Forge,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **Honing.** A tool and a whetstone, by hand, and the stone comes back.
    // One row a tool because a recipe names one block -- the reason the
    // poison is five rows -- and every one of them is called "hone": the
    // menu draws the tool beside the name, and fourteen names for one
    // gesture would be a vocabulary rather than a label. What a hone does is
    // `tools::hone`, applied by `craft` to the bluntest one in the pack.
    //
    // Not the flint knife, whose edge chips rather than dulls, and not the
    // spears or the hoes (see `tools::edge_swings`).
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_STONE_AXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_STONE_AXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_STONE_PICKAXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_STONE_PICKAXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_WEDGED_AXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_WEDGED_AXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_WEDGED_PICKAXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_WEDGED_PICKAXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_COPPER_KNIFE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_COPPER_KNIFE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_COPPER_AXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_COPPER_AXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_COPPER_PICKAXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_COPPER_PICKAXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_COPPER_SHOVEL, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_COPPER_SHOVEL, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_BRONZE_KNIFE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_BRONZE_KNIFE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_BRONZE_AXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_BRONZE_AXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_BRONZE_PICKAXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_BRONZE_PICKAXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_IRON_KNIFE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_IRON_KNIFE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_IRON_AXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_IRON_AXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(BLOCK_IRON_PICKAXE, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (BLOCK_IRON_PICKAXE, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    // ---- the fir and the saxaul ----
    //
    // Appended, for the reason every row is. **Only the rows that make a
    // wood's own timber, and the ones a fire runs.** Every other row that
    // names oak -- a chest, a stool, sticks, mulch, stripping a log -- takes
    // any wood without a row of its own (`takes_other_woods`), which is what
    // a third and a fourth wood would otherwise have cost: a copy of every
    // one of those rows apiece. Boards sawn from a fir are fir boards, so
    // sawing and joining have to name their wood; and a hearth reads its
    // slots by kind (`hearth::next_recipe`), so the charcoal a fire makes
    // out of a wood has to be a row as the birch's is.
    Recipe {
        name: "fir planks",
        inputs: &[(crate::types::BLOCK_FIR_LOG, 1)],
        output: (crate::types::BLOCK_FIR_PLANKS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fir beam",
        inputs: &[(crate::types::BLOCK_FIR_PLANKS, 6)],
        output: (crate::types::BLOCK_FIR_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "saxaul planks",
        inputs: &[(crate::types::BLOCK_SAXAUL_LOG, 1)],
        output: (crate::types::BLOCK_SAXAUL_PLANKS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "saxaul beam",
        inputs: &[(crate::types::BLOCK_SAXAUL_PLANKS, 6)],
        output: (crate::types::BLOCK_SAXAUL_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "fir charcoal",
        inputs: &[(crate::types::BLOCK_FIR_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "saxaul coal",
        inputs: &[(crate::types::BLOCK_SAXAUL_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // ---- joinery: the frame, and nails for the iron age ----
    //
    // See "---- furniture ----" above for what a frame is for and why there
    // are two ways to make one. At the end of the table because a row's
    // index is its identity on the wire.
    //
    // **A dozen nails to a bar**, drawn out and cut at a kiln at a forging
    // heat, as a knife is. A dozen rather than more because a nail is a
    // real piece of iron -- a hand-cut nail weighed as much as a coin did --
    // and a bar that nailed a whole house would make the choice between a
    // nailed frame and a knife no choice at all.
    //
    // **A bar alone, and that is what a kiln loaded with a bar alone
    // makes.** Every other iron row at the kiln asks for the bar *and*
    // something (coal to steel it, a haft and cord for a knife), so they are
    // supersets of this one and `hearth::next_recipe` prefers them the
    // moment the rest is in; a player loading a bar and then its haft has
    // the whole of a forty-five second batch to do it in. Rejected: a second
    // ingredient only to keep nails out of the kiln's way -- a pebble for an
    // "anvil" -- which would be a thing to carry that stands for nothing.
    Recipe {
        name: "nails",
        inputs: &[(BLOCK_IRON_INGOT, 1)],
        output: (crate::types::BLOCK_NAILS, 12),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // Worked sticks, because a pegged joint is a tenon shaved to fit a
    // mortise and a peg through both -- the knife work *is* the joint.
    Recipe {
        name: "pegged frame",
        inputs: &[(BLOCK_WORKED_STICK, 4), (BLOCK_PEG, 4)],
        output: (crate::types::BLOCK_FRAME, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and plain sticks, because a nail holds a butt joint and nothing
    // has to be shaped to take it. What the iron buys is the flint.
    Recipe {
        name: "nailed frame",
        inputs: &[(BLOCK_STICK, 4), (crate::types::BLOCK_NAILS, 4)],
        output: (crate::types::BLOCK_FRAME, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // Six boards butted at the corners and nailed through: no frame, which
    // is the point of a nail. The joined chest is the "chest" row.
    Recipe {
        name: "nailed chest",
        inputs: &[(BLOCK_PLANKS, 6), (crate::types::BLOCK_NAILS, 8)],
        output: (BLOCK_CHEST, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- fishing ----
    //
    // See `fishing` for the two ways and the ones left out.
    //
    // **A trap is reeds and a cord and nothing else**: no knife, no fire, no
    // metal. What it costs is the riverbank it is set in -- the reeds grow
    // there and the cord is three reeds' fibre -- so the place that pays
    // for a trap is the place it is for. Six, because a basket with a funnel
    // mouth a fish can get into and not out of is most of an armful.
    Recipe {
        name: "fish trap",
        inputs: &[(BLOCK_REEDS, 6), (BLOCK_CORD, 1)],
        output: (crate::types::BLOCK_FISH_TRAP, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Four hooks from an ingot, at the forge.** A hook is a bent wire,
    // hammered, and the one thing about a rod that copper is needed for; a
    // whole ingot a rod would price fishing against a knife. A load of one
    // bar and nothing else, as the nails are, so the copper knife and spear
    // -- which load the same bar and more -- are supersets the hearth
    // prefers once the rest is in (`hearth::next_recipe`).
    Recipe {
        name: "copper hooks",
        inputs: &[(BLOCK_COPPER_INGOT, 1)],
        output: (crate::types::BLOCK_COPPER_HOOK, 4),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // A worked haft for the stiffness a rod needs, two cords of line, and the
    // hook. By hand: tying a line is not a job for a fire.
    Recipe {
        name: "fishing rod",
        inputs: &[(BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 2), (crate::types::BLOCK_COPPER_HOOK, 1)],
        output: (crate::types::BLOCK_FISHING_ROD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Porridge: grain and a jug of water at a fire, and nothing else.**
    // Millet is boiled whole, so there is no mill and no dough, and the jug
    // comes back as it does from the dough. Three grains for two bowls,
    // which is the harvest of one ripe head. See `types::BLOCK_WILD_MILLET`
    // for why a second grain, and `food::nutrition` for why it feeds less
    // than bread.
    Recipe {
        name: "porridge",
        inputs: &[(crate::types::BLOCK_MILLET, 3), (BLOCK_JUG_WATER, 1)],
        output: (crate::types::BLOCK_MILLET_PORRIDGE, 2),
        station: Station::Heat,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **A frame lashed from cane**, the third way to the one frame. Pegs
    // cost flint and a knife's work, nails cost iron; cane costs a walk to a
    // hot country's river, where it stands already straight and jointed, and
    // cord to tie it. Six, because a cane is lighter than a worked stick and
    // takes doubling at the corners; two cords, because a lashing is most of
    // what holds it. See `types::BLOCK_ARUNDO`.
    Recipe {
        name: "cane frame",
        inputs: &[(crate::types::BLOCK_CANE, 6), (BLOCK_CORD, 2)],
        output: (crate::types::BLOCK_FRAME, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Beeswax on a stick: the fat torch's two rows with the other wad.**
    // Wax is what a torch was dipped in wherever there were bees, and it is
    // the one honest thing the comb of a hive does in a game with no candles
    // and no casting. The same rate as tallow -- one lump, two torches -- and
    // the same torch: a wax torch that burned longer would make the fat row
    // the wrong answer, and the choice here is meant to be *which animal*,
    // the bear or the bees, not which wad is best. See `bees` for what the
    // wax cost.
    Recipe {
        name: "wax torch",
        inputs: &[(BLOCK_STICK, 2), (crate::types::BLOCK_BEESWAX, 1)],
        output: (BLOCK_TORCH, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "wax rewad",
        inputs: &[(BLOCK_TORCH_SPENT, 2), (crate::types::BLOCK_BEESWAX, 1)],
        output: (BLOCK_TORCH, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and the rod every fishing people with a reed bed made: a cane is
    // straight, light and springy already, where a stick has to be worked
    // to be any of those. The same rod (`BLOCK_FISHING_ROD`), because a
    // rod's wear is its line and hook, not its pole -- what the cane saves
    // is the flint knife's work, and it costs the walk to a hot river.
    Recipe {
        name: "cane rod",
        inputs: &[(crate::types::BLOCK_CANE, 1), (BLOCK_CORD, 2), (crate::types::BLOCK_COPPER_HOOK, 1)],
        output: (crate::types::BLOCK_FISHING_ROD, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the pine and the willow ----
    //
    // The fir's three rows for each, for the fir's reason: a wood's own
    // timber and a fire's charcoal are the rows a wood cannot share.
    Recipe {
        name: "pine planks",
        inputs: &[(crate::types::BLOCK_PINE_LOG, 1)],
        output: (crate::types::BLOCK_PINE_PLANKS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "pine beam",
        inputs: &[(crate::types::BLOCK_PINE_PLANKS, 6)],
        output: (crate::types::BLOCK_PINE_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "pine charcoal",
        inputs: &[(crate::types::BLOCK_PINE_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "willow planks",
        inputs: &[(crate::types::BLOCK_WILLOW_LOG, 1)],
        output: (crate::types::BLOCK_WILLOW_PLANKS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "willow beam",
        inputs: &[(crate::types::BLOCK_WILLOW_PLANKS, 6)],
        output: (crate::types::BLOCK_WILLOW_LOG, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "willow coal",
        inputs: &[(crate::types::BLOCK_WILLOW_LOG, 3)],
        output: (BLOCK_COAL, 1),
        station: Station::Heat,
        returns: &[],
        failure: 0.0,
    },
    // **A moss dressing**: two wads of moss for a bandage, where fibre takes
    // three. Dried sphagnum is a field dressing that soaks up more than
    // cotton, and it is what a wet wood gives a hurt player who has no
    // fibre plant near -- the same bandage (`BLOCK_BANDAGE`), because a
    // "moss bandage" with rules of its own would be a second correct answer.
    Recipe {
        name: "moss dressing",
        inputs: &[(crate::types::BLOCK_MOSS, 2)],
        output: (BLOCK_BANDAGE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the four workshops ----
    //
    // See `Station::Bench` for what a workshop is and is not, and the
    // furniture rows above for what moved to the bench. At the end of the
    // table because a row's index is its identity on the wire.
    //
    // **Each is built by hand, out of what its trade already handles**, so
    // the workshop arrives in the age its trade does: the bench is a log on
    // pegged legs, the mason's block a slab of knapped stone on a stump, the
    // wheel boards on a pegged spindle with a stone to keep it turning, and
    // the currier's bench a board frame with a hide lashed over it.
    //
    // **And each is a better way, not a gate.** Every row a workshop runs
    // below has a hand row that makes the same thing worse -- more flint,
    // more clay, more leather, grain left in the husk -- except the four
    // pieces of joinery above, which are the "harder things" the player
    // asked a bench to open. Rejected: *a workshop row that yields more
    // boards from a log*. It was the obvious first bonus and it is a mill:
    // six boards a log against "beam" taking four back is two boards out of
    // nothing every turn of the loop. (The saw now does yield six, and the
    // beam costs six for it -- see "beam". What a saw is, a bench is not: a
    // tool bought with two ingots, not a place.)
    Recipe {
        name: "workbench",
        inputs: &[(BLOCK_LOG, 1), (BLOCK_STICK, 4), (BLOCK_PEG, 4)],
        output: (crate::types::BLOCK_WORKBENCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "mason block",
        inputs: &[(BLOCK_COBBLESTONE, 4), (BLOCK_LOG, 1)],
        output: (crate::types::BLOCK_MASON_BLOCK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "potter's wheel",
        inputs: &[(BLOCK_PLANKS, 3), (BLOCK_PEG, 2), (BLOCK_COBBLESTONE, 1)],
        output: (crate::types::BLOCK_POTTERS_WHEEL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "leather bench",
        inputs: &[(BLOCK_PLANKS, 2), (BLOCK_STICK, 4), (BLOCK_LEATHER, 1), (BLOCK_CORD, 1)],
        output: (crate::types::BLOCK_LEATHER_BENCH, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A frame without the flint.** In the field a joint is a tenon shaved
    // to its mortise with a flake, which is what a worked stick is; at a
    // bench a chisel and a vice cut it square out of a plain stick. The
    // pegs are still pegs. What it decides: flint is the stone age's
    // scarcest work, and the frame is where most of it went, so a player
    // who carries sticks home saves four strikes a frame -- and one who
    // needs a door at a far camp still has the flint way.
    Recipe {
        name: "bench frame",
        inputs: &[(BLOCK_STICK, 4), (BLOCK_PEG, 4)],
        output: (crate::types::BLOCK_FRAME, 1),
        station: Station::Bench,
        returns: &[],
        failure: 0.0,
    },
    // **A quern rather than a pebble.** Grain rubbed with a stone in the hand
    // leaves a third of it in the husk; a saddle quern -- a hollowed slab
    // and a rubber, which is what the mason's block has on it -- grinds it
    // all, and nothing is carried to do it.
    Recipe {
        name: "quern flour",
        inputs: &[(BLOCK_GRAIN, 3)],
        output: (BLOCK_FLOUR, 3),
        station: Station::Mason,
        returns: &[],
        failure: 0.0,
    },
    // Dressed rather than knocked square: three blocks of sandstone make two
    // of ashlar where the hand gets one from two, and a hone split along the
    // bedding plane comes off in threes.
    Recipe {
        name: "ashlar",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 3)],
        output: (crate::types::BLOCK_SANDSTONE_BRICKS, 2),
        station: Station::Mason,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "split hones",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 1)],
        output: (crate::types::BLOCK_WHETSTONE, 3),
        station: Station::Mason,
        returns: &[],
        failure: 0.0,
    },
    // **Thrown rather than coiled.** A pot built up in coils by hand is thick
    // where the coils join; a wheel draws the wall thin and even. A clay
    // less for each, and the crucible keeps its sand, because the sand is
    // what keeps it whole in the kiln and not what gives it its shape.
    Recipe {
        name: "thrown vessel",
        inputs: &[(BLOCK_CLAY, 3), (BLOCK_SAND, 1)],
        output: (BLOCK_VESSEL_RAW, 1),
        station: Station::Wheel,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "thrown jug",
        inputs: &[(BLOCK_CLAY, 3)],
        output: (BLOCK_JUG_RAW, 1),
        station: Station::Wheel,
        returns: &[],
        failure: 0.0,
    },
    // **Cut to a pattern.** A garment cut on the ground from a hide laid out
    // flat wastes the corners; laid over a bench and cut to a pattern it
    // does not. A skin less for each of the four -- which on the tunic is a
    // fifth of a coat, and on the bed-and-armour side of the ladder is what
    // a tannery runs short of.
    Recipe {
        name: "cut cap",
        inputs: &[(BLOCK_LEATHER, 1), (BLOCK_FIBER, 1)],
        output: (BLOCK_LEATHER_CAP, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cut tunic",
        inputs: &[(BLOCK_LEATHER, 4), (BLOCK_FIBER, 2)],
        output: (BLOCK_LEATHER_TUNIC, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cut leggings",
        inputs: &[(BLOCK_LEATHER, 3), (BLOCK_FIBER, 2)],
        output: (BLOCK_LEATHER_LEGGINGS, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "cut boots",
        inputs: &[(BLOCK_LEATHER, 2), (BLOCK_FIBER, 1)],
        output: (BLOCK_LEATHER_BOOTS, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    // ---- steps, and three roofs built of steps ----
    //
    // **Appended**, for the reason the poultice gives: a row's place is its
    // identity on the wire.
    //
    // **Every step is three quarters of a cell, and costs that**: three
    // boards make four steps and three slabs make two, and
    // nothing is gained or lost by cutting a roof into steps and back. A
    // cheaper step would be a way to multiply boards; a dearer one a reason
    // never to build a staircase.
    //
    // **Three roofs, and each is a decision rather than a tier.** Branches
    // over leaves are the first night's roof out of what the wood drops, and
    // they burn like the canopy they were. Thatch wants a meadow's worth of
    // fibre and a lath, and it takes from a spark faster than anything in a
    // house (`wildfire::fuel`). Tiles want clay and a lit kiln and never
    // burn. The hearth indoors is what decides between them.
    //
    // **By hand, all of them**, the bench's and the mason's block's included.
    // A workshop row is a cheaper way to something the field can already
    // make (`a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house`), and a
    // step is a board or a cobble set on another: a staircase that waited on
    // a bench would be a gate on getting up a hill, which is the chore a
    // workshop is not allowed to be. A roof is laid where the house is.
    Recipe {
        name: "plank stairs",
        inputs: &[(BLOCK_PLANKS, 3)],
        output: (crate::types::BLOCK_PLANK_STAIRS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "stone stairs",
        inputs: &[(BLOCK_COBBLESTONE, 3)],
        output: (crate::types::BLOCK_COBBLESTONE_STAIRS, 4),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Tiles are fired straight from clay, in the kiln**, the way the kiln's
    // own bricks row fires them: a tile is a thin brick, and a pit kiln's
    // pots are not what a roof is laid from. Rejected: a raw tile to shape
    // by hand and fire in either kiln, as the raw brick is. It is one more
    // row, one more picture and one more item in a pack for a roof that
    // wants a kiln's heat to shed rain anyway.
    //
    // **Tempered with sand**, as a thin sheet of clay has to be or it cracks
    // in the firing -- and that sand is also what keeps this row apart from
    // the kiln's bricks. A hearth reads its recipe off the tray
    // (`hearth::the_only_recipes_that_shadow_each_other_are_the_ones_that_should`),
    // and clay alone in the tray would have been bricks or tiles by how many
    // lumps a player happened to load.
    Recipe {
        name: "roof tiles",
        inputs: &[(BLOCK_CLAY, 2), (BLOCK_SAND, 1)],
        output: (crate::types::BLOCK_TILE_SLAB, 2),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "tiled roof",
        inputs: &[(crate::types::BLOCK_TILE_SLAB, 3)],
        output: (crate::types::BLOCK_TILE_ROOF, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // Bundles of dry grass tied onto a stick: fibre is what the meadow's
    // grasses give (`blocks`), and dried grass is what thatch is.
    Recipe {
        // Not "thatch": that name is the older fibre-to-soil row, and the
        // client's name table is keyed by it, so two rows would share one
        // translation and the second would never be read.
        name: "thatch roof",
        inputs: &[(BLOCK_FIBER, 4), (BLOCK_STICK, 1)],
        output: (crate::types::BLOCK_THATCH_SLAB, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "thatched roof",
        inputs: &[(crate::types::BLOCK_THATCH_SLAB, 3)],
        output: (crate::types::BLOCK_THATCH_ROOF, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "branch roofing",
        inputs: &[(BLOCK_STICK, 3), (crate::types::BLOCK_LEAF_HANDFUL, 2)],
        output: (crate::types::BLOCK_BRANCH_SLAB, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "branch roof",
        inputs: &[(crate::types::BLOCK_BRANCH_SLAB, 3)],
        output: (crate::types::BLOCK_BRANCH_ROOF, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A pit prop is a post cut from boards**: three of them, by hand, in the
    // gallery it is going to hold up.
    Recipe {
        name: "pit prop",
        inputs: &[(BLOCK_PLANKS, 3)],
        output: (crate::types::BLOCK_PROP, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A stake is a stick with a point on it**, and the point is what the
    // flake is for: two sticks and a flake give two stakes, and the flake
    // comes back, because sharpening a pole does not use a flint up.
    Recipe {
        name: "stakes",
        inputs: &[(BLOCK_STICK, 2), (crate::types::BLOCK_FLINT_FLAKE, 1)],
        output: (crate::types::BLOCK_STAKE, 2),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_FLINT_FLAKE, 1)],
        failure: 0.0,
    },
    // **Earth in a fired pot.** The crucible is a pot, and outside a furnace
    // that is all it is; a spadeful of any soil in one is a place to grow
    // what a windowsill grows (`types::grows_in_a_pot`). Any earth, as every
    // row that names dirt takes any (`other_ground`).
    Recipe {
        name: "planter",
        inputs: &[(BLOCK_VESSEL, 1), (BLOCK_DIRT, 1)],
        output: (crate::types::BLOCK_PLANTER, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A lattice is slats crossed and lashed**: six sticks and a cord for a
    // window that lets the light in and keeps the wolves out. By hand: it is
    // a thing made in the doorway of the hut it goes in, not at a bench.
    Recipe {
        name: "window lattice",
        inputs: &[(BLOCK_STICK, 6), (BLOCK_CORD, 1)],
        output: (crate::types::BLOCK_WINDOW_LATTICE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- the hammer, the chisel and the anvil ----
    //
    // At the end of the table, with the nails, because a row's index is its
    // identity on the wire.
    //
    // **The hammer is pecked, not knapped.** A hammer stone is the one stone
    // tool that is not an edge: you want a lump that will not shatter, and
    // what you do to it is peck a waist round it for the lashing. That is
    // mason's work, so it is a mason's row -- the first thing many players
    // will make at the block they built for ashlar.
    Recipe {
        name: "stone hammer",
        inputs: &[(BLOCK_COBBLESTONE, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
        output: (crate::types::BLOCK_STONE_HAMMER, 1),
        station: Station::Mason,
        returns: &[],
        failure: 0.0,
    },
    // The metal two, on the metal tools' own terms: an ingot and a haft, at
    // the kiln, in one step. Two ingots, because a hammer is all head -- the
    // whole of it is the mass, where a knife is an edge.
    //
    // **No sinew, and that is not a saving, it is a joint.** Every other
    // metal head in the table is *lashed* on, because an axe or a pick is
    // pulled away from its haft by the work. A hammer is driven *into* its
    // haft by every blow: the eye is drifted through the head and the haft
    // wedged in it, and a strip of tendon round the outside would do nothing
    // at all. The same is true of a chisel, whose tang is driven into a
    // handle. It also keeps their loads their own -- a hearth infers its
    // recipe from the tray, and a hammer lashed like an axe would have been
    // *the same load as the bronze axe* and so unmakeable for ever. See
    // `hearth::next_recipe`.
    Recipe {
        name: "bronze hammer",
        inputs: &[(BLOCK_BRONZE_INGOT, 2), (BLOCK_WORKED_STICK, 1)],
        output: (crate::types::BLOCK_BRONZE_HAMMER, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron hammer",
        inputs: &[(BLOCK_IRON_INGOT, 2), (BLOCK_WORKED_STICK, 1)],
        output: (crate::types::BLOCK_IRON_HAMMER, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **The chisel is a flake set square in a handle**, and it is a hand row
    // on purpose: it is the tool that makes the bench rows cheaper, and a
    // tool you had to have a bench to make would arrive after it was wanted.
    Recipe {
        name: "flint chisel",
        inputs: &[(crate::types::BLOCK_FLINT_FLAKE, 2), (BLOCK_WORKED_STICK, 1), (BLOCK_FIBER, 1)],
        output: (crate::types::BLOCK_FLINT_CHISEL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze chisel",
        inputs: &[(BLOCK_BRONZE_INGOT, 1), (BLOCK_WORKED_STICK, 1)],
        output: (crate::types::BLOCK_BRONZE_CHISEL, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **Five ingots and a stump.** The most expensive thing in the bronze age
    // and it makes nothing by itself -- what it buys is the mini-game
    // (`minigame`), where a smith who strikes well gets more out of a bar
    // than the kiln's flat row ever gives. Built in bronze and paid off in
    // iron, which is a reason to keep going rather than a number going up.
    Recipe {
        name: "anvil",
        inputs: &[(BLOCK_BRONZE_INGOT, 5), (BLOCK_LOG, 1)],
        output: (crate::types::BLOCK_ANVIL, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // ---- what a chisel is for ----
    //
    // **Four rows that are the bench's four rows with less wood in them.**
    // Every one has its chisel-less twin above it and always will: the
    // stone age makes a door out of four boards and a chisel makes it out of
    // two, because a mortise pared square holds where a butt joint has to be
    // doubled. The chisel comes back a point blunter (`used_tool`).
    //
    // Rejected: **the chisel as a requirement on the bench rows**. It would
    // have taken the door, the bed, the chair and the table away from every
    // save that has them, to give them back for the price of a flake -- a
    // toll, which is not a decision.
    Recipe {
        name: "pared door",
        inputs: &[(crate::types::BLOCK_FRAME, 1), (BLOCK_PLANKS, 2), (crate::types::BLOCK_FLINT_CHISEL, 1)],
        output: (crate::types::BLOCK_DOOR, 1),
        station: Station::Bench,
        returns: &[(crate::types::BLOCK_FLINT_CHISEL, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "pared chair",
        inputs: &[
            (crate::types::BLOCK_FRAME, 1),
            (BLOCK_PLANKS, 1),
            (BLOCK_STICK, 1),
            (crate::types::BLOCK_FLINT_CHISEL, 1),
        ],
        output: (BLOCK_CHAIR, 1),
        station: Station::Bench,
        returns: &[(crate::types::BLOCK_FLINT_CHISEL, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "pared table",
        inputs: &[
            (crate::types::BLOCK_FRAME, 1),
            (BLOCK_PLANKS, 2),
            (BLOCK_STICK, 2),
            (crate::types::BLOCK_FLINT_CHISEL, 1),
        ],
        output: (BLOCK_TABLE, 1),
        station: Station::Bench,
        returns: &[(crate::types::BLOCK_FLINT_CHISEL, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "pared bed",
        inputs: &[
            (crate::types::BLOCK_FRAME, 1),
            (BLOCK_PLANKS, 1),
            (BLOCK_LEATHER, 2),
            (crate::types::BLOCK_FLINT_CHISEL, 1),
        ],
        output: (BLOCK_BED, 1),
        station: Station::Bench,
        returns: &[(crate::types::BLOCK_FLINT_CHISEL, 1)],
        failure: 0.0,
    },
    // ---- ...and what a hammer is for, away from the anvil ----
    //
    // A hammer and a stone are the mason's pair as a hammer and a chisel are
    // the joiner's: what the hammer buys at the block is that the blow lands
    // where it was aimed, so a course of ashlar comes out of three stones
    // instead of two and a sandstone splits into four hones instead of three.
    Recipe {
        name: "dressed ashlar",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 3), (crate::types::BLOCK_STONE_HAMMER, 1)],
        output: (crate::types::BLOCK_SANDSTONE_BRICKS, 3),
        station: Station::Mason,
        returns: &[(crate::types::BLOCK_STONE_HAMMER, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "struck hones",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 1), (crate::types::BLOCK_STONE_HAMMER, 1)],
        output: (crate::types::BLOCK_WHETSTONE, 4),
        station: Station::Mason,
        returns: &[(crate::types::BLOCK_STONE_HAMMER, 1)],
        failure: 0.0,
    },
    // ---- the one bait that is made ----
    //
    // Feathers and a length of cord, tied to look like something alive. Two
    // to the craft, because the fly is not eaten off the hook and what takes
    // one away is a broken line (`fishing::Bait::keeps`) -- a player who
    // fights well fishes all day on one, and a player who does not is back
    // here. The cord is the price: it is the same cord every stone tool is
    // lashed with, so a fly is a real choice about six fibre.
    //
    // In hand, like the cord itself: knots are not a station's work.
    Recipe {
        name: "fishing fly",
        inputs: &[(crate::types::BLOCK_FEATHER, 2), (crate::types::BLOCK_CORD, 1)],
        output: (crate::types::BLOCK_FISHING_FLY, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- what is slung over the shoulders ----
    //
    // **Appended**, for the reason every row above gives: a row's place in
    // this table is its identity on the wire.
    //
    // Six hide, four cord and two worked sticks for the frame, at the
    // tanner's table -- which is where every other piece of leather is
    // cut, and which puts the rucksack squarely after the first big kill
    // rather than on the first afternoon. That is the point of the
    // price: the pack was halved (`inventory::STORAGE_ROWS`), and the
    // ten squares back are meant to be a *journey* -- find the animals,
    // build the tannery, make the cord -- and not a recipe you tick off
    // before breakfast.
    //
    // Rejected: **making it at the workbench out of fibre**. It would
    // have been reachable in the first ten minutes, which makes halving
    // the pack a ten-minute inconvenience instead of a stretch of the
    // game with its own shape.
    // By hand first, and at the tanner's table for two hides less --
    // exactly the shape every garment above has, and for the reason the
    // rule in `a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house`
    // states: a workshop row that is the *only* way to a thing is a
    // workshop that gates content, and this game's workshops buy
    // material back, not permission.
    Recipe {
        name: "hide rucksack",
        inputs: &[
            (crate::types::BLOCK_LEATHER, 8),
            (crate::types::BLOCK_CORD, 4),
            (crate::types::BLOCK_WORKED_STICK, 2),
        ],
        output: (crate::types::BLOCK_RUCKSACK, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "sewn rucksack",
        inputs: &[
            (crate::types::BLOCK_LEATHER, 6),
            (crate::types::BLOCK_CORD, 4),
            (crate::types::BLOCK_WORKED_STICK, 2),
        ],
        output: (crate::types::BLOCK_RUCKSACK, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    // ---- the bowl, and what it is for ----
    //
    // **Pinched by hand, and thrown on the wheel for a clay less**, the
    // shape every piece of pottery here has: the wheel buys material back and
    // never permission (`a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house`).
    // A pinched bowl is the oldest pot there is -- a thumb pressed into a
    // ball of clay -- which is why it is three clay and not the jug's four.
    // The wheel's mini-game makes two from the same two clay for a potter
    // who keeps time (`minigame::Job::Bowl`).
    Recipe {
        name: "pinched bowl",
        inputs: &[(BLOCK_CLAY, 3)],
        output: (crate::types::BLOCK_BOWL_RAW, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "thrown bowl",
        inputs: &[(BLOCK_CLAY, 2)],
        output: (crate::types::BLOCK_BOWL_RAW, 1),
        station: Station::Wheel,
        returns: &[],
        failure: 0.0,
    },
    // Fired in the kiln like the pot and the jug, for the reason their rows
    // give: clay that has not been fired is clay.
    Recipe {
        name: "fire bowl",
        inputs: &[(crate::types::BLOCK_BOWL_RAW, 1)],
        output: (crate::types::BLOCK_BOWL, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **A haunch and a root stewed into two bowls**, the jug given back
    // with its water gone into the pot. What this is against -- the same
    // haunch and root roasted and carried -- is argued at
    // `types::BLOCK_STEW`. Raw meat only: the smaller meats cook into cooked
    // meat and are roasted on the way, which is where a hare is eaten.
    Recipe {
        name: "stew in bowls",
        inputs: &[
            (BLOCK_RAW_MEAT, 1),
            (crate::types::BLOCK_ROOT, 1),
            (BLOCK_JUG_WATER, 1),
            (crate::types::BLOCK_BOWL, 2),
        ],
        output: (crate::types::BLOCK_STEW, 2),
        station: Station::Heat,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **The hide frame**: four poles lashed into a square at the corners,
    // which is the whole of it and why it costs less than the rack of two by
    // two (six and three) -- one cell, one skin, and a tannery is a row of
    // them in the sun. Appended rather than beside the rack, because a
    // recipe's place in this list is its number on the wire.
    Recipe {
        name: "hide frame",
        inputs: &[(BLOCK_STICK, 4), (BLOCK_FIBER, 2)],
        output: (crate::types::BLOCK_HIDE_FRAME, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A cairn**: six loose stones, piled by hand where it stands -- see
    // `types::BLOCK_CAIRN` for why a mark on the map is a heap in the world.
    // Six rather than the four a block of cobble takes, because a cairn has
    // to be *seen* from a distance and a knee-high heap of four is a rock.
    Recipe {
        name: "cairn",
        inputs: &[(BLOCK_PEBBLE, 6)],
        output: (crate::types::BLOCK_CAIRN, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **A water compass**: an iron nail stroked on a lodestone until it is a
    // needle, laid on a leaf in a bowl the jug's water goes into. The
    // lodestone and the jug come back -- the stone is what magnetises the
    // needle, not what it is made of, so one lodestone found is every
    // compass a household will make (`returns` is for exactly that). A nail
    // rather than an ingot, because an ingot in a hand row is "smelted in a
    // meadow" (`nothing_that_melts_is_offered_at_a_campfire`), and a nail is
    // already iron worked at the anvil: the needle is iron-age by what it
    // is, and the lodestone by where it is found.
    Recipe {
        name: "water compass",
        inputs: &[
            (crate::types::BLOCK_NAILS, 1),
            (crate::types::BLOCK_LODESTONE, 1),
            (crate::types::BLOCK_LEAF_HANDFUL, 1),
            (crate::types::BLOCK_BOWL, 1),
            (BLOCK_JUG_WATER, 1),
        ],
        output: (crate::types::BLOCK_WATER_COMPASS, 1),
        // **By hand: not the forge, not the bench.** A forge row is a batch
        // in a hearth with a heat and a fuel (`hearth`), and nothing here is
        // heated; a bench row must be a cheaper twin of a hand row
        // (`a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house`), and
        // a compass only at the bench would be a gate.
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_LODESTONE, 1), (BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **A barter stall** (`types::BLOCK_STALL`): a counter of boards on
    // sticks, under a bolt of cloth. The cloth is the price of it, and
    // deliberately: a stall is worth building once there is somebody to
    // trade with, which on a server is about when flax is first spun --
    // and a stone-age player with nothing to sell has no use for one yet.
    // Appended, because a recipe's place here is its number on the wire.
    Recipe {
        name: "stall",
        inputs: &[(BLOCK_PLANKS, 3), (BLOCK_STICK, 4), (BLOCK_CLOTH, 1)],
        output: (crate::types::BLOCK_STALL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ---- building in stages (`build`) ----
    //
    // **Four handfuls packed into a block, by hand, and the block is the
    // common one.** The other way back is to heap them where they go, which
    // keeps their rock; this is the quick way for a recipe that wants a
    // block, and what it costs is the rock. See `build`'s module doc.
    Recipe {
        name: "pack earth",
        inputs: &[(crate::types::BLOCK_HANDFUL_EARTH, 4)],
        output: (BLOCK_DIRT, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "pack sand",
        inputs: &[(crate::types::BLOCK_HANDFUL_SAND, 4)],
        output: (BLOCK_SAND, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "pack gravel",
        inputs: &[(crate::types::BLOCK_HANDFUL_GRAVEL, 4)],
        output: (crate::types::BLOCK_GRAVEL, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "pack clay",
        inputs: &[(crate::types::BLOCK_HANDFUL_CLAY, 4)],
        output: (BLOCK_CLAY, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "heap chips",
        inputs: &[(crate::types::BLOCK_STONE_CHIPS, 4)],
        output: (BLOCK_COBBLESTONE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Lime: limestone or chalk burnt in the kiln.** A kiln and not a
    // campfire, because lime wants the heat a kiln holds for hours -- and the
    // walk to white ground, which is what the decision between clay mortar
    // and lime mortar is made of.
    Recipe {
        name: "burn limestone",
        inputs: &[(crate::types::BLOCK_LIMESTONE, 1)],
        output: (crate::types::BLOCK_QUICKLIME, 2),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "burn chalk",
        inputs: &[(crate::types::BLOCK_CHALK, 1)],
        output: (crate::types::BLOCK_QUICKLIME, 2),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **Lime mortar**: lime slaked in a jug of water and beaten into sand,
    // four trowels where clay makes one ("clay mortar", in brickwork's old
    // slot). Early walls are clay-mortared because clay is what there is; a
    // town is lime-mortared because lime goes four times as far.
    Recipe {
        name: "lime mortar",
        inputs: &[(crate::types::BLOCK_QUICKLIME, 1), (crate::types::BLOCK_HANDFUL_SAND, 3), (BLOCK_JUG_WATER, 1)],
        output: (crate::types::BLOCK_MORTAR, 4),
        station: Station::Hands,
        returns: &[(BLOCK_JUG, 1)],
        failure: 0.0,
    },
    // **Daub: clay and earth and straw.** Rejected: dung in it, which is what
    // every wattle-and-daub wall from Kent to the Caucasus has had -- dung is
    // not an item here, on purpose (`types::BLOCK_DUNG`), and a daub row
    // would be the one reason to carry it.
    Recipe {
        name: "daub",
        inputs: &[(crate::types::BLOCK_HANDFUL_CLAY, 1), (crate::types::BLOCK_HANDFUL_EARTH, 1), (BLOCK_FIBER, 1)],
        output: (crate::types::BLOCK_DAUB, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **Cob: earth and clay kneaded with straw.** One lump is one lift of a
    // wall, so a cell of cob is two of these rows -- earth by the handful,
    // which is what a cob house has always been dug out of the ground it
    // stands on.
    Recipe {
        name: "cob",
        inputs: &[(crate::types::BLOCK_HANDFUL_EARTH, 2), (crate::types::BLOCK_HANDFUL_CLAY, 1), (BLOCK_FIBER, 1)],
        output: (crate::types::BLOCK_COB, 2),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },    // ---- the saw and the edge ----
    //
    // **A saw is two ingots of blade in a bow of two sticks**, drawn thin at
    // the kiln like the hammers. Two plain sticks and not a worked haft,
    // because a hearth knows its recipe by the tray (`hearth::next_recipe`):
    // two bars and a worked stick is already the hammer, and a saw loaded the
    // same way would have come out of the fire as one. What it buys is wood: six boards a log where the
    // axe splits four (see "beam" for why the beam went up with it). Its
    // rows name the copper saw and take any saw above it (`TOOL_LADDERS`),
    // and it comes back a point more worn from each log (`used_tool`).
    Recipe {
        name: "copper saw",
        inputs: &[(BLOCK_COPPER_INGOT, 2), (BLOCK_STICK, 2)],
        output: (crate::types::BLOCK_COPPER_SAW, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "bronze saw",
        inputs: &[(BLOCK_BRONZE_INGOT, 2), (BLOCK_STICK, 2)],
        output: (crate::types::BLOCK_BRONZE_SAW, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "iron saw",
        inputs: &[(BLOCK_IRON_INGOT, 2), (BLOCK_STICK, 2)],
        output: (crate::types::BLOCK_IRON_SAW, 1),
        station: Station::Forge,
        returns: &[],
        failure: 0.0,
    },
    // **Sawn, a log is six boards.** One row a wood, because boards are the
    // boards of the log they came from (`own_rows`). By hand and anywhere: a
    // log and a saw are all it takes, and the sawhorse is for joinery, not a
    // toll on boards. Rejected: *seven or eight*, which would make the saw
    // the only way anyone ever made a board -- a tool that answers every
    // question the same way is a tax, and at six a player far from the kiln
    // with an axe in hand still splits the log where it fell.
    Recipe {
        name: "sawn planks",
        inputs: &[(BLOCK_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (BLOCK_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "sawn birch",
        inputs: &[(BLOCK_BIRCH_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (BLOCK_BIRCH_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "sawn fir",
        inputs: &[(crate::types::BLOCK_FIR_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (crate::types::BLOCK_FIR_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "sawn saxaul",
        inputs: &[(crate::types::BLOCK_SAXAUL_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (crate::types::BLOCK_SAXAUL_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "sawn pine",
        inputs: &[(crate::types::BLOCK_PINE_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (crate::types::BLOCK_PINE_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "sawn willow",
        inputs: &[(crate::types::BLOCK_WILLOW_LOG, 1), (crate::types::BLOCK_COPPER_SAW, 1)],
        output: (crate::types::BLOCK_WILLOW_PLANKS, 6),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_COPPER_SAW, 1)],
        failure: 0.0,
    },
    // **The sawhorse** is two trestles and a beam: boards and pegs, hands and
    // anywhere, stone-age work that waits for a saw. See `minigame::Game::Saw`.
    Recipe {
        name: "sawhorse",
        inputs: &[(BLOCK_PLANKS, 3), (BLOCK_STICK, 4), (BLOCK_PEG, 4)],
        output: (crate::types::BLOCK_SAWHORSE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The honing stone** is a block of sandstone dressed flat on a stump:
    // the whetstone's rock, two blocks of it, rubbed flat on each other by
    // hand. Not the mason's: a workshop row has to be a cheaper way to
    // something the hands make, and nothing else makes one of these. See `minigame::Game::Whet` for why a player builds one
    // when a whetstone in the pack already sharpens.
    Recipe {
        name: "honing stone",
        inputs: &[(crate::types::BLOCK_SANDSTONE, 2), (BLOCK_LOG, 1)],
        output: (crate::types::BLOCK_HONING_STONE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // ...and the whetstone's own hone for the four new edges, one row a kind
    // as the others are ("hone").
    Recipe {
        name: "hone",
        inputs: &[(crate::types::BLOCK_COPPER_SAW, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (crate::types::BLOCK_COPPER_SAW, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(crate::types::BLOCK_BRONZE_SAW, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (crate::types::BLOCK_BRONZE_SAW, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(crate::types::BLOCK_IRON_SAW, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (crate::types::BLOCK_IRON_SAW, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    Recipe {
        name: "hone",
        inputs: &[(crate::types::BLOCK_BRONZE_CHISEL, 1), (crate::types::BLOCK_WHETSTONE, 1)],
        output: (crate::types::BLOCK_BRONZE_CHISEL, 1),
        station: Station::Hands,
        returns: &[(crate::types::BLOCK_WHETSTONE, 1)],
        failure: 0.0,
    },
    // ---- the horse's tack ----
    //
    // **A saddle is a tanner's piece**: a wooden tree, a seat and a girth cut
    // to a pattern and laced with cord. It is made at the leather bench, and
    // the bench row is the one a player is meant to find. The hand rows under
    // it are the rule every workshop keeps
    // (`a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house`): a
    // workshop is a cheaper way, never a gate. A saddle cut on the ground
    // wastes the corners of two more skins -- which is a real price for a
    // player with a horse and no bench, and not a wall.
    //
    // Appended, at the end, because a row's place is its number on the wire.
    Recipe {
        name: "saddle",
        inputs: &[(BLOCK_LEATHER, 3), (BLOCK_CORD, 2), (BLOCK_PLANKS, 2)],
        output: (crate::types::BLOCK_SADDLE, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "saddlebags",
        inputs: &[(BLOCK_LEATHER, 3), (BLOCK_CORD, 2)],
        output: (crate::types::BLOCK_SADDLEBAGS, 1),
        station: Station::Leather,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "rough saddle",
        inputs: &[(BLOCK_LEATHER, 5), (BLOCK_CORD, 3), (BLOCK_PLANKS, 2)],
        output: (crate::types::BLOCK_SADDLE, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    Recipe {
        name: "rough bags",
        inputs: &[(BLOCK_LEATHER, 4), (BLOCK_CORD, 3)],
        output: (crate::types::BLOCK_SADDLEBAGS, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
    // **The lean-to** (`types::BLOCK_LEAN_TO`): a night's roof by hand, from
    // what any wood gives -- sticks for the frame, an armful of leaves for
    // the thatch and the bed. Dearer than the straw pallet by the roof, and
    // cheap beside a hut, because it is gone in the morning: the price is
    // paid again every night a player sleeps away from home.
    Recipe {
        name: "lean-to",
        inputs: &[(BLOCK_STICK, 6), (crate::types::BLOCK_LEAF_HANDFUL, 8)],
        output: (crate::types::BLOCK_LEAN_TO, 1),
        station: Station::Hands,
        returns: &[],
        failure: 0.0,
    },
];

/// Why a craft cannot happen, or that it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feasibility {
    Ready,
    MissingIngredients,
    /// The output has nowhere to go.
    NoRoom,
    /// It wants a fire, and there is not one within reach.
    ///
    /// Its own answer rather than folded into `MissingIngredients`,
    /// because it is a completely different thing for the player to do
    /// about it: one of them means "go and find more tin" and the other
    /// means "you have everything, go and stand by the fire". A menu
    /// that said "missing ingredients" to somebody holding all of them
    /// would be a menu that lies.
    NeedsFire,
    /// It wants a lit kiln, and a campfire is not one.
    ///
    /// Split from `NeedsFire` for exactly the reason `NeedsFire` was
    /// split from `MissingIngredients`: a player standing at a blazing
    /// campfire holding an ore they cannot smelt needs to be told that
    /// the *fire* is wrong, not that there is no fire.
    NeedsForge,
    /// It wants a lit bloomery, and neither of the other two is one.
    NeedsBloomery,
    /// It wants a workshop, and that one is not within reach.
    ///
    /// One answer carrying the station rather than four answers, because
    /// what the player is told is the same sentence with a different bench
    /// in it -- and the menu draws the bench.
    NeedsWorkshop(Station),
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
pub fn feasibility(inventory: &Inventory, recipe: &Recipe, heat: Heat) -> Feasibility {
    // Asked first, and deliberately: a player standing in a field with a
    // pack full of ore should be told what is *wrong* -- that there is
    // no fire -- rather than have the ore counted and then be told the
    // same thing anyway. It is also the cheapest of the three checks.
    if !heat.allows(recipe.station) {
        return match recipe.station {
            Station::Forge => Feasibility::NeedsForge,
            Station::Bloomery => Feasibility::NeedsBloomery,
            Station::Bench | Station::Mason | Station::Wheel | Station::Leather => {
                Feasibility::NeedsWorkshop(recipe.station)
            }
            Station::Hands | Station::Heat => Feasibility::NeedsFire,
        };
    }
    if !has_ingredients(inventory, recipe) {
        return Feasibility::MissingIngredients;
    }
    // ...and a hone is short of the one ingredient that matters when every
    // tool it could take is already sharp: honing a sharp edge would grind
    // off metal for nothing. See `tools::hone`.
    if is_rework(recipe) && worked_slot(inventory, recipe).is_none() {
        return Feasibility::MissingIngredients;
    }

    // Checked against a copy with the inputs already gone, because the
    // ingredients usually free the very slots the output needs -- four
    // cobblestone out of a full bar leaves room the naive check would
    // not see.
    let mut after = inventory.clone();
    let tool = used_tool(recipe);
    for &(block, amount) in recipe.inputs {
        // The tool stays in the pack and in its slot, so the room the craft
        // has is the room it has with the chisel still lying there -- which
        // is stricter than pretending the chisel's slot frees up, and it is
        // the truth. See `used_tool`.
        if tool == Some(block) {
            continue;
        }
        take_for(&mut after, recipe, block, amount);
    }
    if !after.has_room_for(recipe.output.0, recipe.output.1) {
        return Feasibility::NoRoom;
    }
    // What comes back needs somewhere to go as well, and it is checked
    // *after* the output has notionally landed -- otherwise a pack with
    // exactly one free slot would accept a craft that produces an ingot
    // and hands a crucible back, and one of the two would vanish.
    after.add(recipe.output.0, recipe.output.1);
    for &(block, amount) in recipe.returns {
        if tool.is_some_and(|tool| crate::types::block_kind(tool) == crate::types::block_kind(block)) {
            continue;
        }
        if !after.has_room_for(block, amount) {
            return Feasibility::NoRoom;
        }
        after.add(block, amount);
    }
    // ...and the tool itself has to still be there. Asked last, because it is
    // the one thing on the list a player cannot fix by emptying their pack.
    if tool.is_some() && tool_slot(inventory, recipe).is_none() {
        return Feasibility::MissingIngredients;
    }
    Feasibility::Ready
}

/// Is everything this recipe asks for in the pack?
///
/// Split out of `feasibility` because the two questions are asked by
/// different people. `feasibility` answers "what is stopping me", and it
/// deliberately reports the *station* first -- a player holding four
/// ingots wants to be told to go and stand at the kiln, not to have the
/// ingots counted at them. But the crafting menu asks something else
/// entirely: "is this worth listing at all", and that is this.
///
/// **They were the same call, and it was a bug that showed.** A player
/// standing in an empty field with an empty pack was offered every metal
/// recipe in the game -- because the station check fired first, so none
/// of them ever reported missing ingredients -- and then watched them
/// all *disappear* the moment they lit a fire, which is the one moment
/// they were closer to making one.
pub fn has_ingredients(inventory: &Inventory, recipe: &Recipe) -> bool {
    recipe
        .inputs
        .iter()
        .all(|&(block, amount)| count_for(inventory, recipe, block) >= amount)
}

// ---- which stacks a recipe will take ----
//
// **By kind, with one exception, and it is water.** A jug's variant is what
// is in it (`types::vessel_water`), and every recipe used to count a jug of
// the sea as a jug of the river -- so dough was kneaded with sea water. That
// one is wrong in a way a baker would notice: sea water is three and a half
// per cent salt, twice what bread can carry. Pond water is *not* refused: the
// oven takes the loaf past two hundred degrees, which kills whatever made the
// pond unsafe, and refusing it would be the game pretending bread is not
// cooked. The quench in the steel rows takes any water on purpose.

/// Does this recipe refuse this particular stack of an ingredient it names?
///
/// `pub(crate)` for the hearth, which counts what is in its tray itself
/// (`hearth::next_recipe`): a fire loaded with a jug of the river used to
/// read as loaded for the salt row, and the boil that jug was put there for
/// never started.
pub(crate) fn refuses(recipe: &Recipe, id: BlockId) -> bool {
    use crate::types::{block_kind, vessel_water, BLOCK_DOUGH, BLOCK_SALT};
    // **Wet leather, pelt, cloth and wool wait until they are dry** (`wet`),
    // whatever the row: a sodden hide does not cut and wet wool does not
    // spin. Refused here, so the row counts only the dry ones and takes only
    // those -- a pack with a wet hide and a dry one makes one coat, from the
    // dry one, and the menu says it is short of the other.
    if crate::wet::must_dry_first(id) {
        return true;
    }
    if block_kind(id) != BLOCK_JUG_WATER {
        return false;
    }
    match block_kind(recipe.output.0) {
        BLOCK_DOUGH => vessel_water(id) == crate::body::Water::Salt,
        // ...and the other way round for salt: boiling the river dry leaves
        // nothing, and a recipe that made salt out of it would make the coast
        // a detour nobody takes.
        BLOCK_SALT => vessel_water(id) != crate::body::Water::Salt,
        _ => false,
    }
}

/// Every slot holding `block`'s kind that this recipe refuses.
fn refused_slots(inventory: &Inventory, recipe: &Recipe, block: BlockId) -> Vec<usize> {
    let kind = crate::types::block_kind(block);
    inventory
        .slots()
        .iter()
        .enumerate()
        .filter(|(_, stack)| {
            stack.is_some_and(|stack| {
                crate::types::block_kind(stack.block) == kind && refuses(recipe, stack.block)
            })
        })
        .map(|(slot, _)| slot)
        .collect()
}

/// How many of `block` in this pack the recipe would take.
fn count_for(inventory: &Inventory, recipe: &Recipe, block: BlockId) -> u32 {
    let refused: u32 = refused_slots(inventory, recipe, block)
        .into_iter()
        .map(|slot| inventory.count_in(slot))
        .sum();
    let own = inventory.count(block).saturating_sub(refused);
    own + other_woods(recipe, block)
        .chain(other_ground(recipe, block))
        // ...and the better rungs of a tool's ladder, so a player who
        // replaced their flint chisel with a bronze one does not find the
        // rows it unlocked greyed out again. See `other_tools`.
        .chain(other_tools(block).filter(|_| used_tool(recipe) == Some(block)))
        .map(|stand_in| inventory.count(stand_in))
        .sum::<u32>()
}

// ---- any wood ----
//
// **A row that names oak takes any wood that has no row of its own.** A
// chest of fir boards is a chest, and a stool of saxaul is a stool; while
// there were two woods the birch said so by a copy of each row it wanted,
// and two more woods would have been two more copies of every row that
// names a log, a plank or a leaf. So the oak's row stands for them, and the
// copies the birch already has keep their meaning -- a birch log is stripped
// by "strip birch", not by both rows at once, which would list one craft
// twice in the menu.
//
// Rejected: *any wood in every oak row*. The rows that make a wood's own
// timber would then saw a fir log into oak planks. They have rows of their
// own for every wood, and that is exactly what excludes them: the rule is
// one rule, "not where this wood has a row", with no list of exceptions to
// keep true.

/// The other woods' forms of `block` this row takes in its place, in the
/// order `wood::WOODS` lists them. Empty for anything that is not an oak's
/// log, leaf, plank or pegged plank.
fn other_woods(recipe: &Recipe, block: BlockId) -> impl Iterator<Item = BlockId> + '_ {
    use crate::wood::{stands_in_for, WOODS};
    let kind = crate::types::block_kind(block);
    WOODS
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(move |(index, wood)| {
            let form = [wood.log, wood.leaves, wood.planks, wood.pegged]
                .into_iter()
                .find(|&candidate| stands_in_for(kind, candidate) && candidate != kind)?;
            takes_other_woods(recipe, index).then_some(form)
        })
}

// ---- any stone, any earth ----
//
// **A row that names common rubble or dirt takes any rock's rubble of that
// form, or any soil** (`ground::stands_in_for`): a fire ring of granite cobble
// is a fire ring. The same exception as the woods' and for the same reason,
// said once: **a row that makes rubble or earth takes only what it names**,
// so crushing a cobble into pebbles cannot turn granite into common stone
// by way of a recipe. There are no per-rock rows to defer to, so that is the
// whole rule.

/// The other rocks' rubble and the soils this row takes for `block`, in
/// `ground`'s table order. Empty for anything that is not common rubble or
/// dirt, and for a row that makes rubble or earth.
fn other_ground(recipe: &Recipe, block: BlockId) -> impl Iterator<Item = BlockId> {
    let takes = !crate::ground::is_ground_form(recipe.output.0);
    crate::ground::stand_ins(block).filter(move |_| takes)
}

/// Does this row take the wood at `index` of `wood::WOODS` for the oak it
/// names -- is there no row making the same thing out of that wood by name?
///
/// **Worked out once**, a mask per row: the menu asks it of every row
/// every time the pack changes.
fn takes_other_woods(recipe: &Recipe, index: usize) -> bool {
    use std::sync::OnceLock;
    static MASKS: OnceLock<Vec<u8>> = OnceLock::new();
    let masks = MASKS.get_or_init(|| RECIPES.iter().map(own_rows).collect());
    // A row that is not in the table (a test's own) has no siblings to
    // defer to; its own timber is still its own.
    let at = RECIPES.iter().position(|row| std::ptr::eq(row, recipe));
    let mask = at.map_or_else(|| own_rows(recipe), |at| masks[at]);
    mask & (1 << index) == 0
}

/// The woods that have a row of their own for what `row` makes: bit `i` for
/// `wood::WOODS[i]`. **And every wood, for a row that makes a wood's own
/// timber** -- boards are the boards of the log they were sawn from.
fn own_rows(row: &Recipe) -> u8 {
    use crate::wood::{as_oak, wood_of, WOODS};
    if wood_of(row.output.0).is_some() {
        return u8::MAX;
    }
    let mut mask = 0u8;
    for (index, wood) in WOODS.iter().enumerate().skip(1) {
        let named = |block: BlockId| wood_of(block) == Some(wood);
        let sibling = RECIPES.iter().any(|other| {
            !std::ptr::eq(other, row)
                && other.output == row.output
                && other.station == row.station
                && other.inputs.len() == row.inputs.len()
                && other.inputs.iter().any(|&(block, _)| named(block))
                && other
                    .inputs
                    .iter()
                    .zip(row.inputs)
                    .all(|(&(theirs, a), &(ours, b))| a == b && as_oak(theirs) == crate::types::block_kind(ours))
        });
        if sibling {
            mask |= 1 << index;
        }
    }
    mask
}

/// `Inventory::take_exact`, stepping round the stacks the recipe refuses.
///
/// Those are lifted out, the take runs exactly as it always has, and they
/// go back into the slots they came from -- so a pack with no refused
/// stack in it, which is every pack for every recipe but one, goes through
/// the one path it always went through.
fn take_for(inventory: &mut Inventory, recipe: &Recipe, block: BlockId, amount: u32) -> bool {
    // **The named wood first, then the others in table order**, and all of
    // it or none: an oak row short of oak takes the rest in fir, and one
    // short of both takes nothing.
    let stand_ins: Vec<BlockId> = other_woods(recipe, block).chain(other_ground(recipe, block)).collect();
    if !stand_ins.is_empty() {
        if count_for(inventory, recipe, block) < amount {
            return false;
        }
        let mut left = amount;
        for kind in std::iter::once(block).chain(stand_ins) {
            let here = inventory.count(kind).min(left);
            if here > 0 && inventory.take_exact(kind, here) {
                left -= here;
            }
        }
        return left == 0;
    }
    let refused = refused_slots(inventory, recipe, block);
    if refused.is_empty() {
        return inventory.take_exact(block, amount);
    }
    let aside: Vec<(usize, crate::inventory::Stack)> = refused
        .into_iter()
        .filter_map(|slot| {
            let stack = inventory.slots()[slot]?;
            inventory.take_from(slot, stack.count);
            Some((slot, stack))
        })
        .collect();
    let taken = inventory.take_exact(block, amount);
    for (slot, stack) in aside {
        inventory.put_in_slot(slot, stack);
    }
    taken
}

// ---- working a thing rather than making one ----
//
// **Two kinds of row hand back the thing they took**: the poison, which
// smears paste on a spear, and the hone, which puts an edge back on a tool.
// `craft` used to treat both as making a *new* item out of the old one, and a
// new item is unworn -- so poisoning a spear with two toadstools mended it,
// and a hone would have been a free new axe. Both now take one particular
// stack and hand the same stack back, worked: its wear kept (the poison) or
// carried on to the next edge (`tools::hone`).

/// The ingredient a recipe *works* rather than spends: the one of the same
/// kind as what comes out. `None` for every row that makes something.
pub fn worked_kind(recipe: &Recipe) -> Option<BlockId> {
    let made = crate::types::block_kind(recipe.output.0);
    recipe
        .inputs
        .iter()
        .map(|&(block, _)| crate::types::block_kind(block))
        .find(|&kind| kind == made)
}

/// Does this row work a thing rather than make one? See [`worked_kind`].
pub fn is_rework(recipe: &Recipe) -> bool {
    worked_kind(recipe).is_some()
}

fn is_hone(recipe: &Recipe) -> bool {
    recipe
        .inputs
        .iter()
        .any(|&(block, _)| crate::types::block_kind(block) == crate::types::BLOCK_WHETSTONE)
}

/// Which stack a rework would take: for a hone the bluntest tool, and among
/// equally blunt ones the most worn; for the poison a spear that has none on
/// it yet. `None` when there is nothing it would do anything to -- a hone
/// with every tool of that kind already sharp.
fn worked_slot(inventory: &Inventory, recipe: &Recipe) -> Option<usize> {
    let kind = worked_kind(recipe)?;
    let hone = is_hone(recipe);
    let adds = recipe.output.0 & crate::types::VARIANT_MASK;
    inventory
        .slots()
        .iter()
        .enumerate()
        .filter_map(|(slot, stack)| stack.map(|stack| (slot, stack)))
        .filter(|(_, stack)| crate::types::block_kind(stack.block) == kind)
        .filter(|(_, stack)| !hone || crate::tools::blunt_step(stack.block) > 0)
        .max_by_key(|&(slot, stack)| {
            let wants_it = if hone {
                u32::from(crate::tools::blunt_step(stack.block))
            } else {
                u32::from(stack.block & adds != adds)
            };
            (wants_it, stack.damage, std::cmp::Reverse(slot))
        })
        .map(|(slot, _)| slot)
}

// ---- a tool held while a thing is made ----
//
// **A third kind of row: one that *uses* a tool without spending it.** The
// crucible and the mould were already in `inputs` and `returns` at once, and
// that was enough for them, because a pot has no wear -- it comes back
// exactly as it went in. A chisel does have wear, and handing one back
// through `returns` would hand back a *new* one: the same bug the hone had
// (see `worked_kind` above), one step along. A chisel that mended itself
// every time it was used would be a chisel bought once, and then the cheaper
// furniture rows would simply be the furniture rows.
//
// So a tool named in both lists is taken out of the two of them and carried
// through `craft` by its slot: it is never taken, never re-added, and one
// point of wear is put on it if the craft went through -- `wear_tool`, the
// same call a swing makes, so a chisel worn to nothing disappears exactly as
// an axe does.
//
// Rejected: **a `tool` field on `Recipe`**. Four hundred rows would have
// grown a word saying `None`, and the fact is already written twice in the
// two rows that have it. Rejected: **wearing the tool in the server's
// `craft_for`**. The client has a copy of this table and works out for itself
// what a row would cost, so a rule the server alone knew would be a menu that
// promises a free chisel and a pack that disagrees a frame later.

/// The tools a row's named one stands for: a row that asks for a flint
/// chisel takes a bronze one too, and a row that asks for a stone hammer
/// takes any of the three.
///
/// **A ladder rather than a set**, and it is read from the named rung
/// upwards: the row names the cheapest tool that will do the job, and a
/// better one is allowed but never *required*. A row per metal was the
/// alternative -- six furniture rows instead of three -- and it is the copy
/// the woods were collapsed out of for exactly this reason (`other_woods`).
const TOOL_LADDERS: [&[BlockId]; 3] = [
    &[
        crate::types::BLOCK_STONE_HAMMER,
        crate::types::BLOCK_BRONZE_HAMMER,
        crate::types::BLOCK_IRON_HAMMER,
    ],
    &[crate::types::BLOCK_FLINT_CHISEL, crate::types::BLOCK_BRONZE_CHISEL],
    &[crate::types::BLOCK_COPPER_SAW, crate::types::BLOCK_BRONZE_SAW, crate::types::BLOCK_IRON_SAW],
];

/// The better tools that stand in for `block`, from the rung above it up.
/// Empty for everything that is not on a ladder.
fn other_tools(block: BlockId) -> impl Iterator<Item = BlockId> {
    let kind = crate::types::block_kind(block);
    TOOL_LADDERS
        .iter()
        .find_map(|ladder| ladder.iter().position(|&rung| rung == kind).map(|at| &ladder[at + 1..]))
        .unwrap_or(&[])
        .iter()
        .copied()
}

/// The tool a row *uses*: named in `inputs` and handed back in `returns`,
/// and worn rather than unchanged by the work. `None` for every other row,
/// the crucible's and the mould's included -- a pot has no durability.
pub fn used_tool(recipe: &Recipe) -> Option<BlockId> {
    recipe.inputs.iter().map(|&(block, _)| block).find(|&block| {
        recipe.returns.iter().any(|&(back, _)| crate::types::block_kind(back) == crate::types::block_kind(block))
            && crate::types::tool_durability(block).is_some()
    })
}

/// How good the tool this row is worked with is, 0..1, for
/// `quality::Maker::tool`.
///
/// One for a row that needs no tool, and that is not the insult it looks
/// like: `Maker::tool` asks "how good is what you are working with", and
/// for a row worked with your hands the answer is your hands. Zero is for
/// a row that *wants* a chisel and is being attempted without one, which
/// `feasibility` refuses anyway -- it is here so the number is total.
///
/// For a row that does use one, two things about that particular tool:
/// **how blunt it is** and **how well it was made**. A blunt chisel makes
/// worse work, which is the one place the edge reaches the bench, and it
/// is the half a player can do something about tonight (`tools::hone`).
pub fn tool_goodness(inventory: &Inventory, recipe: &Recipe) -> f32 {
    if used_tool(recipe).is_none() {
        return 1.0;
    }
    let Some(stack) = tool_slot(inventory, recipe).and_then(|slot| inventory.slots()[slot]) else {
        return 0.0;
    };
    // Three quarters condition, a quarter how it was made: a fine chisel
    // worn to nothing is still worn to nothing, and a poor sharp one still
    // cuts. **Times the edge**: this is the doc's "how blunt it is", which
    // for years read only the wear -- a blunt bronze chisel made work as
    // good as a sharp one, and the whetstone had nothing to say at a bench.
    let edge = crate::tools::edge_factor(stack.block);
    ((0.75 * stack.condition() + 0.25 * stack.quality().fraction()) * edge).clamp(0.0, 1.0)
}

/// Which stack the tool would come out of: the lowest rung of the ladder the
/// pack holds, and among equals the most worn.
///
/// **The cheap one wears out first**, which is the choice a player would make
/// for themselves: a flint chisel is a flake and a stick, and a bronze one is
/// an ingot. A rule that reached for the best tool in the pack would quietly
/// grind the expensive one away while the cheap one sat beside it.
fn tool_slot(inventory: &Inventory, recipe: &Recipe) -> Option<usize> {
    let named = used_tool(recipe)?;
    let ladder: Vec<BlockId> =
        std::iter::once(crate::types::block_kind(named)).chain(other_tools(named)).collect();
    inventory
        .slots()
        .iter()
        .enumerate()
        .filter_map(|(slot, stack)| stack.map(|stack| (slot, stack)))
        .filter_map(|(slot, stack)| {
            let rung = ladder.iter().position(|&rung| rung == crate::types::block_kind(stack.block))?;
            Some((slot, rung, stack.damage))
        })
        .min_by_key(|&(slot, rung, damage)| (rung, std::cmp::Reverse(damage), slot))
        .map(|(slot, _, _)| slot)
}

/// What the worked stack comes back as.
fn reworked(recipe: &Recipe, old: crate::inventory::Stack) -> crate::inventory::Stack {
    if is_hone(recipe) {
        crate::tools::hone(old)
    } else {
        crate::inventory::Stack::worn(
            old.block | (recipe.output.0 & crate::types::VARIANT_MASK),
            1,
            old.damage,
        )
    }
}

/// How many times the ingredients would stretch to.
///
/// Ingredients only -- room is not counted, because room comes back as
/// the inputs are spent and predicting that for a run of crafts means
/// simulating the whole run. The server decides what actually happens;
/// this is the number the menu shows so the player can see that a click
/// is worth making.
pub fn possible_crafts(inventory: &Inventory, recipe: &Recipe) -> u32 {
    // A hone with nothing blunt to hone is not a craft, however many sharp
    // axes and whetstones there are.
    if is_rework(recipe) && worked_slot(inventory, recipe).is_none() {
        return 0;
    }
    // A row with no tool in the pack at all makes nothing, however much
    // timber there is.
    let tool = used_tool(recipe);
    if tool.is_some() && tool_slot(inventory, recipe).is_none() {
        return 0;
    }
    recipe
        .inputs
        .iter()
        // **The tool is not a limit on how many.** One chisel makes four
        // chairs in one click and comes back four points blunter; counting it
        // as an ingredient would have offered "×1" to a player with a full
        // pack of boards, which reads as a shortage that is not there.
        .filter(|&&(block, _)| tool != Some(block))
        .map(|&(block, amount)| count_for(inventory, recipe, block) / amount.max(1))
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
        let have = count_for(inventory, recipe, block);
        // `then`, not `then_some`: the argument to `then_some` is
        // evaluated whatever the condition says, and `amount - have`
        // underflows for every ingredient the player has enough of.
        (have < amount).then(|| (block, amount - have))
    })
}

/// Boards of each wood in the pack, in `wood::WOODS` order.
fn planks_by_wood(inventory: &Inventory) -> Vec<u32> {
    crate::wood::WOODS.iter().map(|wood| inventory.count(wood.planks) + inventory.count(wood.pegged)).collect()
}

/// **A piece of furniture is of the wood its boards were**: the wood the craft
/// took the most boards of (`take_for` takes the named oak first, then the
/// others in table order). Oak when no boards went in at all, as a nailed
/// chest's do, and anything that is not furniture as it was.
fn in_wood_of(output: BlockId, before: &[u32], after: &Inventory) -> BlockId {
    if !crate::types::is_wooden_furniture(output) {
        return output;
    }
    let taken = planks_by_wood(after);
    let wood = before
        .iter()
        .zip(&taken)
        .map(|(was, is)| was.saturating_sub(*is))
        .enumerate()
        .max_by_key(|&(index, used)| (used, std::cmp::Reverse(index)))
        .filter(|&(_, used)| used > 0)
        .map_or(0, |(index, _)| index);
    crate::types::in_wood(output, wood)
}

/// **A row run up to the moment of making, and the piece taken back off the
/// bench**: what the sawhorse spends when a run begins (`minigame::Job::recipe`).
///
/// The inputs go exactly as the menu row would take them -- any wood's boards,
/// the chisel's wear, the room checked -- because it *is* the menu row, run by
/// `craft_made`. What comes back is the piece it made (in the wood of its
/// boards, `in_wood_of`), lifted out again for the run to hand back or not,
/// and the boards of that wood, which is what a true cut returns and a
/// spoiled one gives half of. `None` changes nothing.
///
/// Rejected: *spending `Job::inputs` with `take_exact`*, the anvil's way. It
/// takes oak and only oak, so a pine joiner would have been told they were
/// short of boards with a pack full of them -- and the bench row beside the
/// sawhorse would have taken the same pine without a word.
pub fn begin_piece(
    inventory: &mut Inventory,
    recipe: &Recipe,
    heat: Heat,
) -> Option<(crate::types::BlockId, u32, crate::types::BlockId)> {
    let kind = crate::types::block_kind(recipe.output.0);
    let before = inventory.clone();
    let boards_before = planks_by_wood(inventory);
    if craft_made(inventory, recipe, heat, Attempt::Succeeds, crate::quality::Quality::PLAIN) != Crafted::Made {
        *inventory = before;
        return None;
    }
    let made = inventory
        .slots()
        .iter()
        .flatten()
        .map(|stack| stack.block)
        .find(|&block| crate::types::block_kind(block) == kind && inventory.count(block) > before.count(block));
    let Some(made) = made else {
        *inventory = before;
        return None;
    };
    let count = inventory.count(made) - before.count(made);
    inventory.take_exact(made, count);
    let boards = planks_by_wood(inventory);
    let wood = boards_before
        .iter()
        .zip(&boards)
        .map(|(was, is)| was.saturating_sub(*is))
        .enumerate()
        .max_by_key(|&(index, used)| (used, std::cmp::Reverse(index)))
        .filter(|&(_, used)| used > 0)
        .map_or(0, |(index, _)| index);
    Some((made, count, crate::wood::WOODS[wood].planks))
}

/// Runs a recipe against an inventory, with the die already rolled.
///
/// `Refused` changes nothing. All-or-nothing is the whole contract: a
/// craft that half-happened is items destroyed. `Failed` is *not* a
/// half: it is the whole of the inputs gone and nothing made, which is
/// what a shattered nodule is, and it is only ever the answer when the
/// attempt would otherwise have been `Made` -- a player without the
/// flakes is refused, not robbed.
///
/// `attempt` is the caller's roll against `recipe.failure`. See
/// `Attempt` for why the roll is not made here.
pub fn craft(inventory: &mut Inventory, recipe: &Recipe, heat: Heat, attempt: Attempt) -> Crafted {
    craft_made(inventory, recipe, heat, attempt, crate::quality::Quality::PLAIN)
}

/// The same, by somebody in a particular state.
///
/// **A second entry point rather than a fifth argument on the first**,
/// and the reason is the callers: `craft` is called from a hundred tests
/// and from the mod ABI, and every one of them is about *what a recipe
/// costs and makes* rather than about who made it. Threading
/// `Quality::PLAIN` through all of them would be a hundred edits that say
/// nothing, and an unjudged piece behaves exactly as it always did (see
/// `quality::PLAIN_FRACTION`), so the two functions are the same function
/// for everything that does not care.
///
/// `quality` is stamped on the **output** and on nothing else. A rework
/// -- honing, mending -- keeps the quality the piece already had, because
/// sharpening an axe does not re-forge it; that falls out of `reworked`
/// carrying the old stack's whole word.
pub fn craft_made(
    inventory: &mut Inventory,
    recipe: &Recipe,
    heat: Heat,
    attempt: Attempt,
    quality: crate::quality::Quality,
) -> Crafted {
    // **No hearth rows.** Everything that needs a fire is run *by the fire*
    // now -- you load a hearth and it works while you do something else
    // (see `crate::hearth`) -- and a hand-crafting path that still
    // accepted those recipes would be a second, instant, fuel-free way
    // to smelt for anyone standing close enough to a flame.
    //
    // A workshop row is not a hearth row (`Station::is_hearth`): it is made
    // from the pack like this one, and `feasibility` below asks whether the
    // bench is within reach.
    if recipe.station.is_hearth() {
        return Crafted::Refused;
    }
    // Checked in full even when the roll says the attempt fails,
    // including the room for an output that will not be made: a player
    // who could not have made the thing is not charged for failing to.
    // Checking less on a failure would also mean the two outcomes ran
    // different code up to the take, which is how one of them gets a bug
    // the other does not.
    if !feasibility(inventory, recipe, heat).is_ready() {
        return Crafted::Refused;
    }
    // The one stack a rework works, read before anything is taken: it has to
    // be *that* tool that comes back honed, not whichever axe `take_exact`
    // reaches first.
    let worked = worked_slot(inventory, recipe)
        .and_then(|slot| inventory.slots()[slot].map(|stack| (slot, stack)));
    // How many boards of each wood the pack held, so the furniture made can be
    // of the wood that went into it (`in_wood_of`).
    let planks_before = planks_by_wood(inventory);
    // The chisel or the hammer this row is *held with*: found before anything
    // moves, because its slot is what it is handed back through. See
    // `used_tool`.
    let tool = used_tool(recipe);
    let tool_at = tool.and_then(|_| tool_slot(inventory, recipe));
    for &(block, amount) in recipe.inputs {
        if tool == Some(block) {
            continue;
        }
        let mut amount = amount;
        if let Some((slot, stack)) = worked {
            if crate::types::block_kind(block) == crate::types::block_kind(stack.block) {
                inventory.take_from(slot, 1);
                amount -= 1;
            }
        }
        if amount > 0 && !take_for(inventory, recipe, block, amount) {
            // Cannot happen after the check above, and if it ever does,
            // stopping here leaves less damage than carrying on.
            return Crafted::Refused;
        }
    }
    // **A recipe that cannot fail does not, whatever was rolled.** The
    // chance belongs to the row, and a caller that rolled a die against
    // a row with no chance on it has made a mistake the player should
    // not pay a haft for.
    let failed = attempt == Attempt::Fails && recipe.failure > 0.0;
    if !failed {
        // The room was checked against exactly this state, so nothing
        // is left over.
        match worked {
            Some((_, old)) => {
                let back = reworked(recipe, old);
                inventory.add_worn(back.block, back.count, back.damage);
            }
            None => {
                let made = in_wood_of(recipe.output.0, &planks_before, inventory);
                // `add_worn` and not `add`, because the quality rides in
                // the same word the wear does (`quality`): a fresh piece
                // has no wear, so the word *is* the quality.
                //
                // ...and only for the things a judgement changes
                // (`quality::takes_quality`), which is what keeps a batch
                // of bricks one stack instead of sixty-four.
                let word = if crate::quality::takes_quality(made) {
                    crate::inventory::Stack::new(made, 0).with_quality(quality).damage
                } else {
                    0
                };
                inventory.add_worn(made, recipe.output.1, word);
            }
        }
    }
    // ...and the equipment comes back out of the fire with it -- made or
    // not, because a pot survives a spoiled pour. After the output,
    // because the output is what the room was checked for first -- and
    // `returns` is always something that was in the pack a moment ago,
    // so the slot it came out of is still there.
    for &(block, amount) in recipe.returns {
        if tool.is_some_and(|tool| crate::types::block_kind(tool) == crate::types::block_kind(block)) {
            continue;
        }
        inventory.add(block, amount);
    }
    // **The tool is worn once whichever way the attempt went**, because it was
    // swung either way: a chisel that only blunted on the cuts that came out
    // right would be a chisel sharpened by mistakes.
    if let Some(slot) = tool_at {
        inventory.wear_tool(slot);
    }
    if failed {
        Crafted::Failed
    } else {
        Crafted::Made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{MAX_STACK, SLOTS};

    /// `craft` with the blow landing, as a bool: did it come out?
    ///
    /// Nearly every test here is about *what a recipe costs and makes*,
    /// not about the knapping roll, and for those the roll is noise.
    /// The tests that are about the roll call `craft` themselves with
    /// `Attempt::Fails`.
    fn made(pack: &mut Inventory, recipe: &Recipe, heat: Heat) -> bool {
        craft(pack, recipe, heat, Attempt::Succeeds).is_made()
    }

    #[test]
    fn a_light_can_be_made_out_of_a_wood_that_has_no_grass_growing_in_it() {
        // The hole the fungus was added to close. Every torch row asked
        // for fibre, fibre comes off tall grass, and the dead forest,
        // the bog and the deep taiga grow none -- so the dark places
        // were the places a player could not make a light, and the
        // answer was to remember to bring one.
        //
        // Nothing here mentions fibre or a fire: a stick, a fungus off
        // a fallen trunk, and hands.
        //
        // The rows that make a *light* out of a fungus. The poultice
        // spends a fungus too (see `injury`) and is not a light, so it is
        // left out rather than counted as a third torch that is not one.
        let fungus_rows: Vec<&Recipe> = RECIPES
            .iter()
            .filter(|r| r.output.0 != BLOCK_POULTICE)
            .filter(|r| r.inputs.iter().any(|&(b, _)| b == BLOCK_BRACKET_FUNGUS))
            .collect();
        assert_eq!(fungus_rows.len(), 2, "the wad and the re-wad");
        for row in &fungus_rows {
            assert_eq!(row.station, Station::Hands);
            assert!(!row.inputs.iter().any(|&(b, _)| b == BLOCK_FIBER));
            assert_eq!(row.output.0, BLOCK_TORCH);
        }

        let mut pack = Inventory::new();
        pack.add(BLOCK_STICK, 1);
        pack.add(BLOCK_BRACKET_FUNGUS, 1);
        let wad = RECIPES
            .iter()
            .find(|r| r.name == "fungus torch")
            .expect("the fungus torch");
        assert!(made(&mut pack, wad, Heat::NONE));
        assert_eq!(pack.count(BLOCK_TORCH), 1);

        // ...and it is the *same* torch the fibre makes. A better one
        // would make the grass row the wrong answer everywhere, and
        // then this would be a replacement rather than a second way.
        let grass = RECIPES
            .iter()
            .find(|r| r.name == "torch")
            .expect("the fibre torch");
        assert_eq!(wad.output, grass.output);
    }

    #[test]
    fn fly_agaric_goes_onto_a_spear_and_the_spear_is_still_the_same_spear() {
        // **The poison is a bit of the weapon's own id**, which is what
        // makes it survive a save, a chest and a dropped stack without
        // a new item or a new field anywhere. What this holds is the
        // two halves of that: every spear has a row that puts the paste
        // on, and what comes out is the *same weapon* with one bit set
        // -- not a different weapon that happens to look similar.
        use crate::types::{is_poisoned, is_weapon, block_kind, unpoisoned};
        let spears = [
            BLOCK_FLINT_SPEAR,
            BLOCK_BONE_SPEAR,
            BLOCK_COPPER_SPEAR,
            BLOCK_BRONZE_SPEAR,
            BLOCK_IRON_SPEAR,
        ];
        for spear in spears {
            let row = RECIPES
                .iter()
                .find(|r| {
                    r.inputs.iter().any(|&(b, _)| b == spear)
                        && r.inputs.iter().any(|&(b, _)| b == BLOCK_TOADSTOOL)
                })
                .unwrap_or_else(|| panic!("{} cannot be poisoned", crate::types::block_name(spear)));
            assert_eq!(row.station, Station::Hands, "smearing a mushroom is not hot work");
            assert!(is_poisoned(row.output.0), "{} came out clean", row.name);
            assert_eq!(
                unpoisoned(row.output.0),
                spear,
                "{} came out as a different weapon",
                row.name
            );
            assert!(is_weapon(row.output.0) && block_kind(row.output.0) == spear);
        }

        // ...and it is made by hand, out of two toadstools, which are
        // the one thing in the world that is deliberately not food.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT_SPEAR, 1);
        pack.add(BLOCK_TOADSTOOL, 2);
        let row = RECIPES.iter().find(|r| r.name == "poison spear").expect("the row");
        assert!(made(&mut pack, row, Heat::NONE));
        assert_eq!(pack.count(BLOCK_TOADSTOOL), 0, "the toadstools were not spent");
        // Counted by *exact id* rather than by `count`, which answers
        // by kind: a poisoned spear is still a flint spear as far as
        // stacking, weight and the hotbar are concerned, and that is
        // the point of putting the paste in the variant field. What
        // must be true is that the pack now holds the poisoned one and
        // not the clean one.
        let held: Vec<crate::types::BlockId> = pack
            .slots()
            .iter()
            .flatten()
            .filter(|s| crate::types::is_weapon(s.block))
            .map(|s| s.block)
            .collect();
        assert_eq!(held, vec![crate::types::poisoned(BLOCK_FLINT_SPEAR)]);
    }

    #[test]
    fn every_ingredient_in_the_game_can_actually_be_got() {
        // **The progression audit, as one closure.** A recipe that asks
        // for something nothing in the world produces is a dead end,
        // and it is the kind of dead end nobody notices: the row is in
        // the menu, it is greyed out for ever, and the player assumes
        // they have not found the thing yet.
        //
        // So: everything a player can *come by* is gathered here --
        // what a block drops when it is broken, what it leaves behind,
        // what an animal gives under a knife, what a rack cures, what a
        // hearth cooks, and what every recipe makes or hands back --
        // and then every ingredient of every recipe has to be in that
        // set. Nothing here is about *when* a thing becomes available,
        // which is the tech tree's business; this is the weaker and
        // more important question of whether it becomes available at
        // all.
        use crate::types::{block_kind, block_name};
        let mut gettable: std::collections::HashSet<BlockId> = std::collections::HashSet::new();

        // What the world gives when you break it, and what a picked
        // plant leaves standing.
        for def in crate::blocks::BLOCKS {
            if let Some(drop) = def.drop {
                gettable.insert(block_kind(drop));
            }
            if let Some(rest) = def.leaves_behind {
                gettable.insert(block_kind(rest));
            }
        }
        // ...what an animal is worth under a knife, and what it drops
        // where it fell.
        for species in crate::animals::Species::ALL {
            for &(block, _) in species.butchering() {
                gettable.insert(block_kind(block));
            }
            for &(block, _) in species.drops() {
                gettable.insert(block_kind(block));
            }
        }
        // ...and what a thing becomes *on its own*, which is the source
        // that is neither a drop nor a recipe: a lit torch burns down to
        // a spent one in the pack (see the torch clock in `inventory`),
        // a picked bush fills again, a robbed nest is laid in again. The
        // first run of this audit forgot the torch and called the two
        // re-wadding rows unreachable -- the audit being wrong about the
        // world rather than the world being wrong, which is exactly the
        // mistake a list like this invites.
        gettable.insert(block_kind(crate::types::BLOCK_TORCH_SPENT));
        for &(id, _) in crate::types::ALL_BLOCK_IDS {
            if let Some(ripe) = crate::types::ripens_into(id) {
                gettable.insert(block_kind(ripe));
            }
        }


        // ...what a rack cures and a hearth cooks.
        for &(id, _) in crate::types::ALL_BLOCK_IDS {
            if let Some(cured) = crate::rack::cures_into(id) {
                gettable.insert(block_kind(cured));
            }
        }
        // ...and what every recipe makes or gives back. Recipes feed
        // each other, so this is a closure rather than one pass: flour
        // is only gettable because grain is, and dough only because
        // flour is.
        loop {
            let before = gettable.len();
            for recipe in RECIPES {
                let inputs_known = recipe
                    .inputs
                    .iter()
                    .all(|&(block, _)| gettable.contains(&block_kind(block)));
                if !inputs_known {
                    continue;
                }
                gettable.insert(block_kind(recipe.output.0));
                for &(block, _) in recipe.returns {
                    gettable.insert(block_kind(block));
                }
            }
            if gettable.len() == before {
                break;
            }
        }

        let missing: Vec<&str> = RECIPES
            .iter()
            .flat_map(|r| r.inputs.iter())
            .map(|&(block, _)| block_kind(block))
            .filter(|block| !gettable.contains(block))
            .map(block_name)
            .collect();
        assert!(
            missing.is_empty(),
            "nothing in the world produces: {missing:?}"
        );

        // ...and the other half of the same question: every recipe has
        // to be reachable, or it is a row nobody can ever run.
        let stranded: Vec<&str> = RECIPES
            .iter()
            .filter(|r| {
                !r.inputs
                    .iter()
                    .all(|&(block, _)| gettable.contains(&block_kind(block)))
            })
            .map(|r| r.name)
            .collect();
        assert!(stranded.is_empty(), "these rows can never be run: {stranded:?}");
    }

    #[test]
    fn there_is_a_spear_for_every_age_and_the_bone_one_is_the_worst() {
        // The ladder the four spears exist to make. Bone comes before
        // flint and is worse than it, which is the whole reason to knap
        // one; each metal outlasts the last. Damage is the server's
        // (`hunting_damage`), so what this holds is the half that lives
        // here: what each costs and how long it lasts.
        use crate::blocks::definition;
        let order = [
            BLOCK_BONE_SPEAR,
            BLOCK_FLINT_SPEAR,
            BLOCK_COPPER_SPEAR,
            BLOCK_BRONZE_SPEAR,
            BLOCK_IRON_SPEAR,
        ];
        let mut last = 0;
        for spear in order {
            let thrusts = definition(spear).durability.expect("a spear wears out");
            assert!(
                thrusts > last,
                "{} lasts {thrusts} thrusts, which is no better than the one before it",
                crate::types::block_name(spear)
            );
            last = thrusts;
            assert!(crate::types::is_weapon(spear));
        }

        // ...and the bone one costs nothing that has to be knapped: a
        // bone, a haft and a cord, all of which a player has on their
        // first evening. That is the rung it exists to be.
        let bone = RECIPES.iter().find(|r| r.name == "bone spear").expect("the row");
        assert_eq!(bone.station, Station::Hands);
        assert!(bone.inputs.iter().any(|&(b, _)| b == BLOCK_BONE));
        assert!(!bone.inputs.iter().any(|&(b, _)| b == BLOCK_FLINT));
    }

    #[test]
    fn a_hunter_can_dress_in_fur_before_there_is_a_tannery() {
        // What the fur set is for. Leather is a kill, a rack and a
        // wait; wool is a sheep and shears. Both are answers a player
        // has on their third day and neither is an answer they have on
        // their first cold night -- so the coat that costs one animal
        // and its own sinew is the rung the ladder was missing.
        //
        // No fire, no fibre, no rack: everything this asks for came off
        // the animal.
        let mut pack = Inventory::new();
        pack.add(BLOCK_PELT, 4);
        pack.add(BLOCK_SINEW, 2);
        let cloak = RECIPES
            .iter()
            .find(|r| r.name == "fur cloak")
            .expect("the fur cloak");
        assert_eq!(cloak.station, Station::Hands);
        assert!(made(&mut pack, cloak, Heat::NONE));
        assert_eq!(pack.count(BLOCK_FUR_CLOAK), 1);
        assert!(crate::equipment::is_wearable(BLOCK_FUR_CLOAK));

        // A bear does it in one, because a bear's hide is one piece of
        // skin the size of four.
        let mut from_a_bear = Inventory::new();
        from_a_bear.add(BLOCK_BEAR_HIDE, 2);
        from_a_bear.add(BLOCK_SINEW, 2);
        let bear = RECIPES
            .iter()
            .find(|r| r.name == "bear cloak")
            .expect("the bear cloak");
        assert!(made(&mut from_a_bear, bear, Heat::NONE));
        assert_eq!(from_a_bear.count(BLOCK_FUR_CLOAK), 1);
    }

    #[test]
    fn a_stand_of_wild_cotton_is_the_start_of_a_shirt() {
        // The whole chain, link by link, so the day a link is lost the
        // assertion that fails names it. A wild stand gives a boll and its
        // seed; the seed goes in tilled earth and nowhere wilder; it ripens
        // in two stages; the ripe plant gives two bolls and its seed back;
        // four bolls are a bolt; a bolt and thread are each of the four
        // garments, by hand; and each of those goes on in a slot of its own.
        use crate::blocks::definition;
        use crate::equipment::{slot_of, Slot};
        use crate::types::{
            also_drops, block_drop_count, can_grow_on, is_placeable, ripens_into,
            BLOCK_COTTON_PLANT, BLOCK_COTTON_RIPE, BLOCK_COTTON_SEEDS, BLOCK_FARMLAND,
            BLOCK_GRASS, BLOCK_WILD_COTTON,
        };
        assert_eq!(definition(BLOCK_WILD_COTTON).drop, Some(BLOCK_COTTON));
        assert_eq!(also_drops(BLOCK_WILD_COTTON), Some((BLOCK_COTTON_SEEDS, 1)));
        assert_eq!(ripens_into(BLOCK_WILD_COTTON), None, "a wild stand of cotton grows back");
        assert!(can_grow_on(BLOCK_WILD_COTTON, BLOCK_GRASS));
        assert!(!can_grow_on(BLOCK_WILD_COTTON, BLOCK_FARMLAND), "a wild stand can be sown as a crop");

        assert!(is_placeable(BLOCK_COTTON_SEEDS), "cotton seed cannot be sown");
        assert!(can_grow_on(BLOCK_COTTON_SEEDS, BLOCK_FARMLAND));
        assert!(!can_grow_on(BLOCK_COTTON_SEEDS, BLOCK_GRASS), "cotton grows without a hoe");
        assert_eq!(ripens_into(BLOCK_COTTON_SEEDS), Some(BLOCK_COTTON_PLANT));
        assert_eq!(ripens_into(BLOCK_COTTON_PLANT), Some(BLOCK_COTTON_RIPE));
        assert_eq!(ripens_into(BLOCK_COTTON_RIPE), None);
        assert_eq!(definition(BLOCK_COTTON_PLANT).drop, Some(BLOCK_COTTON_SEEDS));
        assert_eq!(definition(BLOCK_COTTON_RIPE).drop, Some(BLOCK_COTTON));
        assert_eq!(block_drop_count(BLOCK_COTTON_RIPE), 2);
        assert_eq!(also_drops(BLOCK_COTTON_RIPE), Some((BLOCK_COTTON_SEEDS, 1)));

        let bolt = RECIPES
            .iter()
            .find(|r| r.output.0 == BLOCK_CLOTH)
            .expect("nothing weaves cloth");
        assert_eq!(bolt.inputs.to_vec(), vec![(BLOCK_COTTON, 4)]);
        assert_eq!(bolt.station, Station::Hands, "weaving wants a station nobody can build");
        for (garment, slot) in [
            (BLOCK_CLOTH_CAP, Slot::Head),
            (BLOCK_CLOTH_TUNIC, Slot::Chest),
            (BLOCK_CLOTH_TROUSERS, Slot::Legs),
            (BLOCK_CLOTH_WRAPS, Slot::Feet),
        ] {
            let recipe = RECIPES
                .iter()
                .find(|r| r.output.0 == garment)
                .unwrap_or_else(|| panic!("nothing sews {garment}"));
            assert_eq!(recipe.station, Station::Hands, "{} needs a station", recipe.name);
            for (input, _) in recipe.inputs {
                assert!(
                    matches!(*input, BLOCK_CLOTH | BLOCK_FIBER),
                    "{} wants {input}, which a cotton field does not give",
                    recipe.name,
                );
            }
            assert_eq!(slot_of(garment), Some(slot), "{} goes on nowhere", recipe.name);
        }
    }

    #[test]
    fn every_crop_gives_back_the_seed_it_was_sown_from() {
        // A field has to be able to sow itself again, or every harvest
        // sends the player back to the wild stand -- see
        // `types::also_drops`. Walked from every seed a player can put in
        // tilled earth to the last stage it ripens into, so a third crop
        // added without its seed row goes red here the day it is added.
        use crate::types::{
            also_drops, block_name, can_grow_on, ripens_into, BLOCK_FARMLAND, BLOCK_GRASS,
            PLACEABLE_BLOCKS,
        };
        let mut crops = 0;
        for &seed in PLACEABLE_BLOCKS {
            let sown = can_grow_on(seed, BLOCK_FARMLAND)
                && !can_grow_on(seed, BLOCK_GRASS)
                && ripens_into(seed).is_some();
            if !sown {
                continue;
            }
            let mut ripe = seed;
            while let Some(next) = ripens_into(ripe) {
                ripe = next;
            }
            assert_eq!(
                also_drops(ripe),
                Some((seed, 1)),
                "a ripe {} does not give back one {}",
                block_name(ripe),
                block_name(seed)
            );
            crops += 1;
        }
        assert!(crops >= 2, "only {crops} crop(s) found; the walk is reading the wrong list");
    }

    #[test]
    fn wool_dresses_a_player_with_no_fire_and_no_metal() {
        // **The gate this material exists to open.** Warm clothing used
        // to sit behind the hunt: leather means a kill, a rack and a
        // wait, and the north means leather. Wool is meant to be
        // reachable by somebody who has done none of that -- so every
        // one of the four has to be `Hands`, not a fire and certainly
        // not a forge. One row moved to `Heat` and the whole point of
        // the chain is gone, silently.
        let woollens = [
            BLOCK_WOOL_CAP,
            BLOCK_WOOL_TUNIC,
            BLOCK_WOOL_LEGGINGS,
            BLOCK_WOOL_BOOTS,
        ];
        for want in woollens {
            let recipe = RECIPES
                .iter()
                .find(|r| r.output.0 == want)
                .unwrap_or_else(|| panic!("nothing makes {want}"));
            assert_eq!(
                recipe.station,
                Station::Hands,
                "{} needs a station a cold player has not got",
                recipe.name,
            );
            // ...and out of wool and thread, with nothing smelted,
            // fired or tanned anywhere in it.
            for (input, _) in recipe.inputs {
                assert!(
                    matches!(*input, BLOCK_WOOL | BLOCK_FIBER),
                    "{} wants {input}, which is not something a sheep and a field provide",
                    recipe.name,
                );
            }
        }
    }

    /// A lit campfire within reach and nothing else.
    const FIRE: Heat = Heat { fire: true, ..Heat::NONE };
    /// A lit kiln, which is a fire as well -- see `Heat::allows`. Most
    /// of the tests below want "every station available" and this is it.
    const KILN: Heat = Heat { kiln: true, ..Heat::NONE };
    /// Standing between all three, which no player ever is and every
    /// test that is not *about* stations wants.
    const ANY_FIRE: Heat = Heat { fire: true, kiln: true, bloomery: true, workshops: 0 };
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
                // **The exception list is empty, and that is the news.**
                // Bronze and iron used to be smelted and then sit there:
                // the tools they were for had been taken out, so the
                // metal chain ended in a chest of ingots nothing wanted.
                // The nine metal tools are what closed it. Anything an
                // item is made for is now either a tool or an
                // ingredient, with no third case -- and if a fourth
                // dead-end item ever appears, this is where it will
                // fail rather than in a player's pack.
                //
                // Food is a tool in the sense that matters here: it is
                // used rather than placed or consumed by a recipe.
                let is_food = crate::food::is_food(r.output.0);
                // ...and the fourth case the note above predicted, which
                // arrived with the hoe: a thing that is used *on the
                // world* rather than mined with, built from or eaten.
                // See `types::is_implement`, which is a list rather than
                // a name checked here.
                let is_implement = crate::types::is_implement(r.output.0);
                // ...and the fifth and sixth, which arrived together in
                // 1.7: a garment, which is spent by stopping blows until
                // it is gone, and a jug, which is spent by being drunk
                // from. Both are "used rather than placed or consumed by
                // a recipe", which is the category `is_tool` and
                // `is_food` were already standing in for.
                let is_worn = crate::equipment::is_wearable(r.output.0);
                let is_vessel = crate::types::is_vessel(r.output.0);
                // ...and the seventh, which arrived with the dark: a
                // torch, which is spent by *burning* and is the only
                // thing here whose use is neither a swing nor a
                // mouthful. See `types::is_torch`.
                let is_torch = crate::types::is_torch(r.output.0);
                // ...and the eighth: a weapon, spent on animals. See
                // `types::is_weapon`.
                let is_weapon = crate::types::is_weapon(r.output.0);
                // ...and the ninth: a raft, spent by being launched onto
                // water (`raft::launch`). A single id rather than a list,
                // because nothing else in the game becomes a body.
                let is_boat = crate::types::block_kind(r.output.0) == crate::types::BLOCK_RAFT;
                // ...and the tenth: a dressing, spent by being put on a
                // wound (`injury::Injuries::treat`). Checked through
                // `Treatment::of`, which is the one list of what dresses
                // what, so a new dressing is a row there and not a name
                // here.
                let is_dressing = crate::injury::Treatment::of(r.output.0).is_some();
                // ...and the eleventh: a rod, spent by the fish it lands,
                // and the bait, spent by the fish that takes it
                // (`fishing`). Not an implement: `is_implement` is also what
                // butchery asks for an edge, and a rod has none.
                // ...and the bait on its hook, spent by the fish that takes
                // it (`fishing::Bait`). Asked through `Bait::of_block`, so a
                // bait added there needs nothing here.
                let is_tackle = crate::types::block_kind(r.output.0) == crate::types::BLOCK_FISHING_ROD
                    || crate::fishing::Bait::of_block(r.output.0).is_some();
                // ...and the twelfth: a hammer or a chisel, which is held
                // *while* something else is made -- at the bench, at the
                // mason's block and at the anvil. See
                // `types::is_workshop_tool` for why it is not any of the
                // eleven above.
                let is_workshop_tool = crate::types::is_workshop_tool(r.output.0);
                // ...and the thirteenth: an instrument, read in the hand
                // and spent on nothing. See `types::is_instrument`.
                let is_instrument = crate::types::is_instrument(r.output.0);
                // ...and the fourteenth: a lump of daub or cob, spent by being
                // laid into a wall where it stands (`build::is_laid`).
                let is_laid = crate::build::is_laid(r.output.0);
                // ...and the fifteenth: tack, spent by being put on a horse.
                // See `types::is_tack`.
                let is_tack = crate::types::is_tack(r.output.0);
                assert!(
                    is_tool
                        || is_tack
                        || is_instrument
                        || is_laid
                        || is_workshop_tool
                        || is_tackle
                        || is_dressing
                        || is_boat
                        || is_weapon
                        || feeds_something
                        || is_food
                        || is_implement
                        || is_worn
                        || is_vessel
                        || is_torch,
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
                    // A hone takes one tool and hands back that one tool,
                    // worked: the same count, and not a generator.
                    assert!(
                        r.output.1 < amount || (is_rework(r) && r.output.1 == amount),
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
            made(&mut inventory, &RECIPES[0], KILN); // log -> 4 planks
        }
        for _ in 0..4 {
            made(&mut inventory, &RECIPES[1], KILN); // 4 planks -> log
        }
        assert!(
            inventory.count(BLOCK_LOG) <= before,
            "a full round trip created logs: {before} became {}",
            inventory.count(BLOCK_LOG)
        );
    }

    #[test]
    fn a_campfire_of_any_rocks_cobble_is_a_campfire_and_no_rock_is_crushed_into_common_stone() {
        use crate::ground::{Form, ROCKS};
        let campfire = named("campfire");
        for rock in &ROCKS[1..] {
            let cobble = rock.form(Form::Cobble);
            let name = crate::types::block_name(cobble);
            let mut pack = Inventory::new();
            pack.add(cobble, 3);
            pack.add(BLOCK_STICK, 4);
            assert_eq!(feasibility(&pack, campfire, Heat::NONE), Feasibility::Ready, "{name} is not a fire ring");
            assert!(made(&mut pack, campfire, Heat::NONE), "{name} would not make a campfire");
            assert_eq!(pack.count(cobble), 0, "the campfire left {name} behind");
            assert_eq!(pack.count(crate::types::BLOCK_CAMPFIRE), 1);
            // ...and a row that *makes* rubble takes only the common stone it
            // names: a granite cobble crushed into common pebbles would be
            // a rock turned into another rock by a menu.
            let mut pack = Inventory::new();
            pack.add(cobble, 4);
            for row in RECIPES.iter().filter(|r| crate::ground::is_ground_form(r.output.0)) {
                assert_ne!(feasibility(&pack, row, Heat::NONE), Feasibility::Ready, "'{}' took {name}", row.name);
            }
        }
        // Any soil stands in for dirt, too.
        assert!(crate::ground::SOILS.iter().all(|&soil| crate::ground::stand_ins(crate::types::BLOCK_DIRT).any(|b| b == soil)));
    }

    /// **Furniture is the wood it was made of**, one table in six woods rather
    /// than six tables (`types::furniture_wood`), and it is that wood again
    /// when it is broken.
    #[test]
    fn a_table_of_pine_boards_is_a_pine_table_and_breaks_back_into_one() {
        use crate::types::{block_drop, block_kind, furniture_wood, in_wood, is_known_block, BLOCK_PINE_PLANKS};
        let pine = crate::wood::WOODS.iter().position(|wood| wood.planks == BLOCK_PINE_PLANKS).unwrap();
        let mut pack = Inventory::new();
        pack.add(BLOCK_PINE_PLANKS, 4);
        pack.add(crate::types::BLOCK_FRAME, 1);
        pack.add(BLOCK_STICK, 4);
        let table = named("table");
        let at_bench = Heat::NONE.with_workshop(Station::Bench);
        assert!(made(&mut pack, table, at_bench), "pine boards at a bench are not a table");
        let made_table = in_wood(BLOCK_TABLE, pine);
        assert_eq!(pack.count(made_table), 1, "the table did not come out pine");
        assert_eq!(block_kind(made_table), BLOCK_TABLE, "a pine table is not a table");
        assert_eq!(furniture_wood(made_table), pine);
        assert!(is_known_block(made_table), "a pine table is an id the anti-cheat refuses");
        assert_eq!(block_drop(made_table), Some(made_table), "a pine table broke into another wood");
        // Oak planks, or none at all, are the oak's.
        assert_eq!(furniture_wood(BLOCK_TABLE), 0);
        // ...and nothing else carries a wood.
        //
        // **A leaf and not a stone, and the difference is the point.** The
        // wood field is the three bits `dig` spends on a bite -- the flag
        // and how many quarters have gone -- and which of the two an id
        // means is decided by its kind. On a leaf, which is neither
        // furniture nor anything anybody quarries, the bits mean nothing
        // and the anti-cheat says so; on a stone the same bits are a block
        // three quarters of the way through being dug, which is a real id
        // the server writes. Asked of a stone, this test read "a stone
        // carries a wood" and was looking at a quarry.
        let wood_bits = in_wood(BLOCK_TABLE, pine) & crate::types::WOOD_MASK;
        assert!(!is_known_block(wood_bits | crate::types::BLOCK_LEAVES), "a leaf carries a wood");
        assert!(
            !crate::types::is_wooden_furniture(wood_bits | crate::types::BLOCK_STONE),
            "a stone carries a wood"
        );
    }

    #[test]
    fn a_chest_of_fir_boards_is_a_chest_and_a_fir_log_is_never_sawn_into_oak() {
        use crate::types::{BLOCK_FIR_LOG, BLOCK_FIR_PLANKS, BLOCK_SAXAUL_PLANKS};
        // Any wood stands in where a row names oak and the wood has no row
        // of its own: six boards of two woods, round a frame, are a chest.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FIR_PLANKS, 4);
        pack.add(BLOCK_SAXAUL_PLANKS, 2);
        pack.add(crate::types::BLOCK_FRAME, 1);
        pack.add(BLOCK_PEG, 4);
        let chest = named("chest");
        assert_eq!(feasibility(&pack, chest, Heat::NONE), Feasibility::Ready, "fir and saxaul boards are not a chest");
        assert!(made(&mut pack, chest, Heat::NONE));
        assert_eq!(pack.count(BLOCK_FIR_PLANKS) + pack.count(BLOCK_SAXAUL_PLANKS), 0, "the chest left boards behind");
        // ...and it is a fir chest: fir is most of what went into it.
        assert_eq!(pack.count(crate::types::in_wood(BLOCK_CHEST, 2)), 1, "four fir boards of six made no fir chest");

        // ...but boards are the boards of the log they came from.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FIR_LOG, 1);
        assert_eq!(possible_crafts(&pack, named("planks")), 0, "a fir log was offered as oak planks");
        assert_eq!(possible_crafts(&pack, named("fir planks")), 1);
        assert!(made(&mut pack, named("fir planks"), Heat::NONE));
        assert_eq!(pack.count(BLOCK_FIR_PLANKS), 4);
        assert_eq!(pack.count(BLOCK_PLANKS), 0);
    }

    #[test]
    fn a_wood_with_a_row_of_its_own_is_not_offered_the_oak_row_as_well() {
        // Birch had its own rows before any wood stood in for another. A
        // birch log listed under both "strip a log" and "strip birch" would
        // be one craft shown twice.
        let mut pack = Inventory::new();
        pack.add(BLOCK_BIRCH_LOG, 1);
        assert_eq!(possible_crafts(&pack, named("strip a log")), 0, "a birch log was offered the oak row too");
        assert_eq!(possible_crafts(&pack, named("strip birch")), 1);
        // A fir has no row for it, so the oak row takes it.
        let mut pack = Inventory::new();
        pack.add(crate::types::BLOCK_FIR_LOG, 1);
        assert_eq!(possible_crafts(&pack, named("strip a log")), 1, "a fir log cannot be stripped");
    }

    #[test]
    fn a_recipe_runs_when_the_ingredients_are_there() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_LOG, 1);
        assert_eq!(feasibility(&inventory, &RECIPES[0], KILN), Feasibility::Ready);
        assert!(made(&mut inventory, &RECIPES[0], KILN));
        assert_eq!(inventory.count(BLOCK_LOG), 0);
        assert_eq!(inventory.count(BLOCK_PLANKS), 4);
    }

    #[test]
    fn a_recipe_without_its_ingredients_changes_nothing() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_PLANKS, 5); // one short of the beam recipe
        assert_eq!(
            feasibility(&inventory, &RECIPES[1], KILN),
            Feasibility::MissingIngredients
        );
        assert!(!made(&mut inventory, &RECIPES[1], KILN));
        assert_eq!(inventory.count(BLOCK_PLANKS), 5, "a failed craft consumed something");
    }

    #[test]
    fn porridge_is_one_step_at_a_fire_and_gives_the_jug_back_and_bread_is_four() {
        use crate::types::{BLOCK_MILLET, BLOCK_MILLET_PORRIDGE};
        let row = |name: &str| RECIPES.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no '{name}' row"));
        let porridge = row("porridge");
        assert_eq!(porridge.station, Station::Heat);
        assert_eq!(porridge.output.0, BLOCK_MILLET_PORRIDGE);
        assert!(porridge.inputs.contains(&(BLOCK_MILLET, 3)) && porridge.inputs.contains(&(BLOCK_JUG_WATER, 1)));
        assert_eq!(porridge.returns, &[(BLOCK_JUG, 1)], "the pot kept the jug");
        // Bread is grind, wet, bake: three rows between grain and a meal,
        // and the meal is the bigger one.
        for step in ["grind grain", "dough", "bread"] {
            row(step);
        }
        let bread = crate::food::nutrition(crate::types::BLOCK_BREAD).unwrap();
        let bowl = crate::food::nutrition(BLOCK_MILLET_PORRIDGE).unwrap();
        assert!(bowl < bread, "porridge fed as well as bread, and the quicker meal won outright");
        // Both go off, porridge in two days and a loaf in four; the grain
        // they are made of keeps, which is why it is the thing to store.
        assert!(crate::food::is_perishable(BLOCK_MILLET_PORRIDGE), "porridge keeps for ever");
        assert!(crate::food::is_perishable(crate::types::BLOCK_BREAD), "bread keeps for ever");
        assert!(crate::food::rot_every(crate::types::BLOCK_BREAD) > crate::food::rot_every(BLOCK_MILLET_PORRIDGE), "porridge keeps as long as a loaf");
        assert!(!crate::food::is_perishable(crate::types::BLOCK_GRAIN), "the grain went off");
    }

    #[test]
    fn a_frame_can_be_lashed_from_cane_with_no_flint_and_no_iron() {
        use crate::types::{BLOCK_CANE, BLOCK_FRAME};
        let mut pack = Inventory::new();
        pack.add(BLOCK_CANE, 6);
        pack.add(BLOCK_CORD, 2);
        let row = RECIPES.iter().find(|r| r.name == "cane frame").expect("a cane frame row");
        assert!(made(&mut pack, row, KILN));
        assert_eq!(pack.count(BLOCK_FRAME), 1);
        assert_eq!(crate::types::block_drop(crate::types::BLOCK_ARUNDO), Some(BLOCK_CANE));
        assert_eq!(crate::types::block_drop_count(crate::types::BLOCK_ARUNDO), 2);
    }

    #[test]
    fn a_multi_ingredient_recipe_needs_all_of_them() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_DIRT, 2);
        assert!(!made(&mut inventory, &RECIPES[4], KILN), "turf without leaves");
        inventory.add(crate::types::BLOCK_LEAF_HANDFUL, 2);
        assert!(made(&mut inventory, &RECIPES[4], KILN));
        assert_eq!(inventory.count(BLOCK_GRASS), 1);
        assert_eq!(inventory.count(BLOCK_DIRT), 0);
        assert_eq!(inventory.count(crate::types::BLOCK_LEAF_HANDFUL), 0);
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
        if feasibility(&inventory, &RECIPES[0], KILN) == Feasibility::NoRoom {
            assert!(!made(&mut inventory, &RECIPES[0], KILN));
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
            feasibility(&inventory, &RECIPES[2], KILN),
            Feasibility::Ready,
            "the slot the ingredients free was not counted"
        );
        assert!(made(&mut inventory, &RECIPES[2], KILN));
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
        inventory.add(crate::types::BLOCK_LEAF_HANDFUL, 4);
        assert_eq!(possible_crafts(&inventory, &RECIPES[4]), 2, "turf: 2 leaves each");
    }

    #[test]
    fn the_menu_can_say_what_is_missing() {
        let mut inventory = Inventory::new();
        assert_eq!(missing_ingredient(&inventory, &RECIPES[1]), Some((BLOCK_PLANKS, 6)));
        inventory.add(BLOCK_PLANKS, 5);
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
                // A hone is only a craft with something blunt to hone.
                let block = match worked_kind(r) {
                    Some(kind) if kind == crate::types::block_kind(block) => {
                        crate::tools::with_edge(block, crate::tools::BLUNTEST)
                    }
                    _ => block,
                };
                // ...and a jug of the water the row wants: the salt row is
                // the sea's and refuses the river (`refuses`).
                let block = if refuses(r, block) { crate::types::jug_of(crate::body::Water::Salt) } else { block };
                inventory.add(block, amount);
            }
            assert_eq!(possible_crafts(&inventory, r), 1, "{}", r.name);
            assert_eq!(missing_ingredient(&inventory, r), None, "{}", r.name);
            // Run it the way the world runs it: by hand if it is a hand
            // recipe, and in the hearth that serves it if it is not.
            let made = match r.station {
                Station::Hands => made(&mut inventory, r, ANY_FIRE),
                // ...and beside every workshop, for a workshop row.
                Station::Bench | Station::Mason | Station::Wheel | Station::Leather => {
                    made(&mut inventory, r, Heat { workshops: u8::MAX, ..ANY_FIRE })
                }
                Station::Heat => smelt(&mut inventory, r, crate::hearth::Kind::Campfire),
                Station::Forge => smelt(&mut inventory, r, crate::hearth::Kind::Kiln),
                Station::Bloomery => smelt(&mut inventory, r, crate::hearth::Kind::Bloomery),
            };
            assert!(made, "{} could not be made", r.name);
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
        // Exactly the four things a player can gather bare-handed: a
        // nodule of flint, a fallen branch, loose stones, a handful of
        // grass. No shortcuts are added to the inventory at any point --
        // every ingredient after this line has to come out of a recipe
        // above it, which is what makes this a chain rather than a list.
        //
        // With the blow landing every time: what the roll costs is the
        // next test's business, and a chain test that could fail on a
        // die would be a chain test nobody trusted when it went red.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT, 2);
        pack.add(BLOCK_STICK, 3);
        pack.add(BLOCK_PEBBLE, 13);
        pack.add(BLOCK_FIBER, 18);

        // Step one: two nodules into six flakes. Two, not six: the only
        // flint in a stone tool kit now is the knife's blade, and the
        // flakes that whittle the hafts.
        for _ in 0..2 {
            assert!(made(&mut pack, named("flint flakes"), KILN), "knapping failed");
        }
        assert_eq!(pack.count(BLOCK_FLINT_FLAKE), 6);
        assert_eq!(pack.count(BLOCK_FLINT), 0, "the nodules should be spent");

        // Step two: three hafts, each whittled with a flake -- and note
        // that nothing in the pack is a tool yet. This is the step that
        // would deadlock if a haft needed a finished knife.
        for _ in 0..3 {
            assert!(made(&mut pack, named("worked stick"), KILN), "whittling failed");
        }
        assert_eq!(pack.count(BLOCK_WORKED_STICK), 3);
        assert_eq!(pack.count(BLOCK_STICK), 0);

        // Step three: the heads. The knife's is knapped from what is
        // left of the flakes; the axe's and the pick's are ground from
        // stone, so the stones go into cobble first -- eight pebbles for
        // the two lumps, five more to grind them with.
        assert!(made(&mut pack, named("knife head"), KILN), "the blade could not be knapped");
        assert_eq!(pack.count(BLOCK_FLINT_FLAKE), 1, "one flake over, and no more flint anywhere");
        for _ in 0..2 {
            assert!(made(&mut pack, named("knapped stone"), KILN), "cobble could not be made");
        }
        for head in ["axe head", "pick head"] {
            assert!(made(&mut pack, named(head), KILN), "{head} could not be ground");
        }
        assert_eq!(pack.count(BLOCK_PEBBLE), 0, "the grinding stones should be worn away");
        assert_eq!(pack.count(BLOCK_COBBLESTONE), 0);

        // Step four: the cord, then the binding. Eighteen fibre is three
        // cords, one a tool -- which is the whole grass cost of the kit.
        for _ in 0..3 {
            assert!(made(&mut pack, named("cord"), KILN), "cord could not be twisted");
        }
        assert_eq!(pack.count(BLOCK_FIBER), 0, "the cord is where the grass goes");
        for tool in ["flint knife", "stone axe", "stone pick"] {
            assert!(made(&mut pack, named(tool), KILN), "{tool} could not be bound");
        }
        assert_eq!(pack.count(BLOCK_FLINT_KNIFE), 1);
        assert_eq!(pack.count(BLOCK_STONE_AXE), 1);
        assert_eq!(pack.count(BLOCK_STONE_PICKAXE), 1);
        assert_eq!(pack.count(BLOCK_CORD), 0);
    }

    #[test]
    fn a_stone_head_is_ground_from_stone_and_a_flint_one_is_knapped() {
        // The player's rule, as a table check: flint pierces, so the
        // knife head and the spear's point are flint; stone is hard, so
        // the axe and the pick heads are stone -- and nothing about a
        // stone head touches flint at all, or the "flint axe" is back
        // under another name.
        for head in ["axe head", "pick head"] {
            let r = named(head);
            for &(b, _) in r.inputs {
                assert!(
                    matches!(b, BLOCK_COBBLESTONE | BLOCK_PEBBLE),
                    "{head} is ground from {b}, which is not stone"
                );
            }
            assert_eq!(r.failure, 0.0, "{head} is ground, and grinding does not shatter");
        }
        let knife = named("knife head");
        assert!(
            knife.inputs.iter().all(|&(b, _)| b == BLOCK_FLINT_FLAKE),
            "a knife head is struck from flint and nothing else"
        );
        // ...and the spear's point is the knife's head: the same knapped
        // flint, because piercing is the one thing it is for.
        assert!(
            named("flint spear").inputs.iter().any(|&(b, _)| b == BLOCK_FLINT_KNIFE_HEAD),
            "the spear's point is not the knapped blade"
        );
    }

    #[test]
    fn knapping_a_knife_head_fails_about_half_the_time_and_eats_the_flint_when_it_does() {
        // The chance is on the row, and it is the row's whole character:
        // a knife head that never failed would be the menu entry it used
        // to be. Half, near enough -- and the flakes are lower, because
        // striking them off is the forgiving half of the craft.
        let knife = named("knife head");
        let flakes = named("flint flakes");
        assert!((0.4..=0.6).contains(&knife.failure), "a knife head fails {}", knife.failure);
        assert!((0.25..=0.45).contains(&flakes.failure), "flakes fail {}", flakes.failure);
        assert!(flakes.failure < knife.failure, "shaping should be the harder blow");

        // Knapping is the *only* thing that fails. A pot thrown badly is
        // thrown again from the same clay; a haft is a stick until it is
        // a haft. A row elsewhere in the table growing a chance would be
        // a player losing an ingot to a die, and this is where that is
        // refused.
        for r in RECIPES {
            let knaps = matches!(r.name, "knife head" | "flint flakes");
            assert_eq!(r.failure > 0.0, knaps, "{} has a failure chance of {}", r.name, r.failure);
            assert!((0.0..1.0).contains(&r.failure), "{} can never succeed", r.name);
        }

        // And what a failure costs: the flakes, and nothing else -- no
        // head, and nothing in the pack that was not there.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLINT_FLAKE, 2);
        assert_eq!(craft(&mut pack, knife, Heat::NONE, Attempt::Fails), Crafted::Failed);
        assert_eq!(pack.count(BLOCK_FLINT_FLAKE), 0, "a shattered blade gives its flakes back");
        assert_eq!(pack.count(BLOCK_FLINT_KNIFE_HEAD), 0, "a shattered blade is a blade");
        assert!(pack.slots().iter().all(Option::is_none), "something appeared out of a failure");

        // A failure is only ever where a success would have been: with
        // one flake short, the roll is not consulted and nothing is
        // spent -- a player without the flakes is refused, not robbed.
        let mut short = Inventory::new();
        short.add(BLOCK_FLINT_FLAKE, 1);
        assert_eq!(craft(&mut short, knife, Heat::NONE, Attempt::Fails), Crafted::Refused);
        assert_eq!(short.count(BLOCK_FLINT_FLAKE), 1, "a refused attempt spent a flake");

        // ...and a row that cannot fail does not, whatever was rolled:
        // the die belongs to the recipe, not to the caller.
        let mut sticks = Inventory::new();
        sticks.add(BLOCK_PLANKS, 2);
        assert_eq!(craft(&mut sticks, named("sticks"), Heat::NONE, Attempt::Fails), Crafted::Made);
        assert_eq!(sticks.count(BLOCK_STICK), 4);
    }

    #[test]
    fn a_cord_is_six_fibre_and_a_reed_bank_is_where_fibre_is() {
        // The price of a hafted tool is the cord, and the cord is the
        // grass. Six fibre a cord; a tuft yields fibre one time in two
        // (the server's `spawn_block_drop`) and a reed yields two, so a
        // cord is a dozen tufts or three reeds -- the arithmetic that
        // makes the riverbank the place to go for it.
        let cord = named("cord");
        assert_eq!(cord.inputs, &[(BLOCK_FIBER, 6)]);
        assert_eq!(cord.output, (BLOCK_CORD, 1));
        let reed = named("reed fibre");
        assert_eq!(reed.inputs, &[(BLOCK_REEDS, 1)]);
        assert_eq!(reed.output.0, BLOCK_FIBER);
        let reeds_a_cord = 6u32.div_ceil(reed.output.1);
        assert_eq!(reeds_a_cord, 3, "a cord should be three reeds, not {reeds_a_cord}");

        // And every stone-age haft is bound with a cord or a sinew,
        // never with loose fibre: a handful of grass round a joint is
        // the thing this replaced.
        for tool in ["flint knife", "stone axe", "stone pick", "flint spear"] {
            let r = named(tool);
            assert!(
                r.inputs.iter().any(|&(b, _)| b == BLOCK_CORD),
                "{tool} is not bound with a cord"
            );
            assert!(
                !r.inputs.iter().any(|&(b, _)| b == BLOCK_FIBER),
                "{tool} is lashed with loose grass"
            );
        }
        for tool in ["sinewed knife", "sinewed axe", "sinewed pick"] {
            assert!(
                named(tool).inputs.iter().any(|&(b, _)| b == BLOCK_SINEW),
                "{tool} is not bound with sinew"
            );
        }
    }

    #[test]
    fn a_lashed_tool_is_wedged_into_the_one_that_fells_a_tree() {
        // The wedging step takes the finished lashed tool and hands
        // back the same tool one tier up -- not a new tool from parts.
        // What it costs is pegs, which the player is already splitting
        // for their roof, and it costs them *in the hand*: no fire, no
        // third item, no wait. That is the whole of what replaced the
        // glue chain.
        for (row, wedged, lashed) in [
            ("wedged axe", BLOCK_WEDGED_AXE, BLOCK_STONE_AXE),
            ("wedged pick", BLOCK_WEDGED_PICKAXE, BLOCK_STONE_PICKAXE),
        ] {
            let r = named(row);
            assert_eq!(r.output, (wedged, 1));
            assert!(r.inputs.iter().any(|&(b, _)| b == lashed), "{row} is not made from the lashed tool");
            assert!(r.inputs.iter().any(|&(b, _)| b == BLOCK_PEG), "{row} is not wedged with pegs");
            assert_eq!(r.station, Station::Hands, "{row} needs a fire it should not");
            assert_eq!(
                crate::blocks::definition(wedged).tool,
                Some(crate::blocks::Tier::Flint),
                "{row} is not the tier that fells a tree"
            );
            assert_eq!(crate::blocks::definition(lashed).tool, Some(crate::blocks::Tier::Stone));
        }
        // ...and the chain it replaced is gone rather than orphaned: no
        // row makes glue or resin, and nothing asks for either.
        for name in ["glue", "resin"] {
            assert!(
                !RECIPES.iter().any(|r| r.name == name),
                "the {name} recipe is still in the table"
            );
        }
    }

    #[test]
    fn a_bed_a_chair_a_table_and_a_joined_chest_are_built_round_a_frame_and_a_stool_is_not() {
        use crate::types::{BLOCK_FRAME, BLOCK_NAILS};
        let takes = |row: &str, block: BlockId| named(row).inputs.iter().any(|&(b, _)| b == block);
        for row in ["bed", "chair", "table", "chest"] {
            assert!(takes(row, BLOCK_FRAME), "{row} is not built round a frame");
            // The frame is where the fastening was chosen; a row that asked
            // for nails as well would put the piece behind iron either way.
            assert!(!takes(row, BLOCK_NAILS), "{row} asks for nails on top of its frame");
        }
        assert!(!takes("stool", BLOCK_FRAME), "the camp's seat needs a frame");
        assert!(takes("stool", BLOCK_PEG), "the stool's legs are not wedged");
        // A nail through a stave is a leak.
        assert!(!takes("barrel", BLOCK_NAILS), "the barrel is nailed");
        assert_eq!(named("nailed chest").output, (BLOCK_CHEST, 1));
        assert!(!takes("nailed chest", BLOCK_FRAME), "a nailed box still wants a frame");
    }

    #[test]
    fn a_frame_is_one_frame_whether_it_was_pegged_or_nailed() {
        use crate::types::{BLOCK_FRAME, BLOCK_NAILS};
        let pegged = named("pegged frame");
        let nailed = named("nailed frame");
        assert_eq!(pegged.output, (BLOCK_FRAME, 1));
        assert_eq!(nailed.output, pegged.output, "the two fastenings make different things");
        let has = |r: &Recipe, block: BlockId| r.inputs.iter().any(|&(b, _)| b == block);
        // The whole of the decision: flint work one way, iron the other.
        assert!(has(pegged, BLOCK_WORKED_STICK) && has(pegged, BLOCK_PEG));
        assert!(!has(pegged, BLOCK_NAILS), "the stone-age frame needs iron");
        assert!(has(nailed, BLOCK_NAILS));
        assert!(
            !has(nailed, BLOCK_WORKED_STICK) && !has(nailed, BLOCK_PEG),
            "a nailed frame still costs the flint the nails were meant to save"
        );
        assert_eq!((pegged.station, nailed.station), (Station::Hands, Station::Hands));
    }

    #[test]
    fn a_door_is_a_frame_boarded_over_at_a_bench_in_any_wood_pegged_or_nailed() {
        use crate::types::{BLOCK_BIRCH_PLANKS, BLOCK_DOOR, BLOCK_FIR_PLANKS, BLOCK_FRAME, BLOCK_NAILS};
        let door = named("door");
        assert_eq!(door.station, Station::Bench, "a door is hung anywhere but at a bench");
        assert!(door.inputs.iter().any(|&(b, _)| b == BLOCK_FRAME), "a door is not built round a frame");
        assert!(!door.inputs.iter().any(|&(b, _)| b == BLOCK_NAILS), "a door asks for nails on top of its frame");
        // Fir and birch boards round a frame are a door.
        let mut pack = Inventory::new();
        pack.add(BLOCK_FIR_PLANKS, 2);
        pack.add(BLOCK_BIRCH_PLANKS, 2);
        pack.add(BLOCK_FRAME, 1);
        assert_eq!(feasibility(&pack, door, Heat::NONE), Feasibility::NeedsWorkshop(Station::Bench), "a door hung in a field");
        let bench = Heat::NONE.with_workshop(Station::Bench);
        assert_eq!(feasibility(&pack, door, bench), Feasibility::Ready, "fir and birch boards are not a door");
        assert!(made(&mut pack, door, bench));
        assert_eq!(pack.count(BLOCK_DOOR), 1);
        // ...and a frame nailed out of plain sticks hangs one as well: the
        // iron age's door is the same door.
        let mut pack = Inventory::new();
        pack.add(BLOCK_STICK, 4);
        pack.add(BLOCK_NAILS, 4);
        pack.add(BLOCK_PLANKS, 4);
        assert!(made(&mut pack, named("nailed frame"), Heat::NONE));
        assert!(made(&mut pack, door, bench), "a nailed frame hangs no door");
        assert_eq!(pack.count(BLOCK_DOOR), 1);
    }

    #[test]
    fn a_house_is_furnished_before_there_is_any_iron_and_the_camp_by_hand() {
        use crate::types::BLOCK_FRAME;
        // Everything a room has, out of what the stone age has: boards,
        // sticks, flakes, pegs and a cured hide. No fire at all.
        let mut pack = Inventory::new();
        // Worked sticks: four to a frame for four frames, and six for the
        // twenty-four pegs the frames, the chest and the stool drive.
        let worked = 4 * 4 + 6;
        pack.add(BLOCK_PLANKS, 6 + 3 + 2 + 4 + 2);
        pack.add(BLOCK_STICK, worked + 2 + 4 + 3);
        pack.add(BLOCK_FLINT_FLAKE, worked);
        pack.add(BLOCK_LEATHER, 2);
        for _ in 0..worked {
            assert!(made(&mut pack, named("worked stick"), Heat::NONE));
        }
        for _ in 0..6 {
            assert!(made(&mut pack, named("pegs"), Heat::NONE));
        }
        for _ in 0..4 {
            assert!(made(&mut pack, named("pegged frame"), Heat::NONE), "no pegged frame by hand");
        }
        assert_eq!(pack.count(BLOCK_FRAME), 4);
        for (row, piece) in [
            ("chest", BLOCK_CHEST),
            ("bed", BLOCK_BED),
            ("chair", BLOCK_CHAIR),
            ("table", BLOCK_TABLE),
            ("stool", BLOCK_STOOL),
        ] {
            // The camp's pieces by hand; the house's at a bench, which is
            // itself stone-age work (`every_workshop_is_built_by_hand_without_fire_or_metal`).
            let at = match named(row).station {
                Station::Hands => Heat::NONE,
                station => {
                    assert!(!matches!(row, "chest" | "stool"), "a {row} wants {station:?}, and the camp needs one");
                    Heat::NONE.with_workshop(station)
                }
            };
            assert!(made(&mut pack, named(row), at), "no {row}: {:?}", feasibility(&pack, named(row), at));
            assert_eq!(pack.count(piece), 1);
        }
        assert_eq!(pack.count(BLOCK_FRAME), 0, "a frame was left over");
    }

    #[test]
    fn every_workshop_is_built_by_hand_without_fire_or_metal() {
        use crate::types::{BLOCK_NAILS, BLOCK_TIN_INGOT};
        for station in Station::WORKSHOPS {
            let block = station.workshop_block().expect("a workshop is a block");
            assert_eq!(Station::of_workshop(block), Some(station), "{station:?} is not found by its own block");
            let row = RECIPES
                .iter()
                .find(|r| r.output.0 == block)
                .unwrap_or_else(|| panic!("nothing builds {station:?}"));
            assert_eq!(row.station, Station::Hands, "{station:?} needs a station to build");
            for &(input, _) in row.inputs {
                assert!(
                    ![BLOCK_NAILS, BLOCK_IRON_INGOT, BLOCK_COPPER_INGOT, BLOCK_TIN_INGOT, BLOCK_BRONZE_INGOT].contains(&input),
                    "{station:?} is built out of metal"
                );
            }
        }
        assert_eq!(Station::of_workshop(BLOCK_PLANKS), None);
        assert!(Station::WORKSHOPS.iter().all(|s| !s.is_hearth()), "a workshop was taken for a fire");
    }

    /// **The rule that keeps a workshop a decision.** Every row a workshop
    /// runs either has a hand row making the same thing at a higher price --
    /// so the field can always do it, worse -- or is one of the four pieces
    /// of joinery the bench exists to open. A workshop row with no hand row
    /// and not in that list is a new gate, and a gate on survival is the
    /// chore `Station::Bench` refuses.
    #[test]
    fn a_workshop_row_is_a_cheaper_way_or_a_piece_of_the_house() {
        use crate::types::{BLOCK_DOOR, BLOCK_FRAME};
        // **A tool held is not an ingredient spent**, so it is not counted:
        // a chisel goes back in the pack a point blunter (`used_tool`), and
        // counting it would say a row that saves two boards costs one more
        // thing than the row it replaces.
        let total = |r: &Recipe| {
            let tool = used_tool(r);
            r.inputs.iter().filter(|&&(b, _)| tool != Some(b)).map(|&(_, n)| n).sum::<u32>() as f32
                / r.output.1 as f32
        };
        for row in RECIPES.iter().filter(|r| Station::WORKSHOPS.contains(&r.station)) {
            if [BLOCK_BED, BLOCK_CHAIR, BLOCK_TABLE, BLOCK_DOOR].contains(&row.output.0) {
                continue;
            }
            // **...and the third case: the workshop's own tool.** A stone
            // hammer is neither a cheaper way to a thing nor a piece of the
            // house -- it is the thing a mason's block is for, and it is
            // pecked on one. Nothing is locked behind it: the block itself is
            // a hand row (four cobbles and a log), and every row the hammer
            // cheapens has its hammerless twin. See `types::BLOCK_STONE_HAMMER`.
            if crate::types::is_workshop_tool(row.output.0) {
                continue;
            }
            let by_hand: Vec<&Recipe> =
                RECIPES.iter().filter(|r| r.station == Station::Hands && r.output.0 == row.output.0).collect();
            assert!(!by_hand.is_empty(), "{} can only be made at {:?}", row.name, row.station);
            // A frame is priced in flint, not in count: the bench's saves the
            // worked sticks, and is checked by name below.
            if row.output.0 == BLOCK_FRAME {
                assert!(!row.inputs.iter().any(|&(b, _)| b == BLOCK_WORKED_STICK), "the bench frame still costs flint");
                continue;
            }
            assert!(
                by_hand.iter().all(|hand| total(hand) > total(row)),
                "{} at {:?} is no cheaper than by hand",
                row.name,
                row.station
            );
        }
    }

    #[test]
    fn a_workshop_row_is_refused_in_a_field_and_at_the_wrong_workshop_and_made_at_its_own() {
        let quern = named("quern flour");
        let mut pack = Inventory::new();
        pack.add(BLOCK_GRAIN, 3);
        assert_eq!(feasibility(&pack, quern, Heat::NONE), Feasibility::NeedsWorkshop(Station::Mason));
        let wrong = Heat { fire: true, kiln: true, bloomery: true, ..Heat::NONE }.with_workshop(Station::Bench);
        assert_eq!(feasibility(&pack, quern, wrong), Feasibility::NeedsWorkshop(Station::Mason), "a bench ground grain");
        assert_eq!(craft(&mut pack, quern, wrong, Attempt::Succeeds), Crafted::Refused);
        let mason = Heat::NONE.with_workshop(Station::Mason);
        assert!(made(&mut pack, quern, mason));
        assert_eq!(pack.count(BLOCK_FLOUR), 3, "the quern left grain in the husk");
        assert!(mason.has_workshop(Station::Mason) && !mason.has_workshop(Station::Wheel));
    }

    /// The chisel and the hammer: the rows they cheapen, what they cost the
    /// tool, and the promise that neither of them locks anything.
    #[test]
    fn a_chisel_makes_a_door_out_of_less_wood_and_comes_back_a_point_blunter() {
        use crate::types::{BLOCK_DOOR, BLOCK_FLINT_CHISEL, BLOCK_FRAME};
        let plain = named("door");
        let pared = named("pared door");
        let boards = |r: &Recipe| {
            r.inputs.iter().find(|&&(b, _)| b == BLOCK_PLANKS).map(|&(_, n)| n).unwrap_or(0)
        };
        assert!(boards(pared) < boards(plain), "the chisel saved no wood");

        let bench = Heat::NONE.with_workshop(Station::Bench);
        let mut pack = Inventory::new();
        pack.add(BLOCK_FRAME, 1);
        pack.add(BLOCK_PLANKS, 2);
        pack.add(BLOCK_FLINT_CHISEL, 1);
        assert!(made(&mut pack, pared, bench), "a chisel and two boards made no door");
        assert_eq!(pack.count(BLOCK_DOOR), 1);
        // The chisel is still there, and it is one point nearer the end of
        // its life: this is the whole of `used_tool`.
        assert_eq!(pack.count(BLOCK_FLINT_CHISEL), 1, "the chisel was spent");
        let worn = pack
            .slots()
            .iter()
            .flatten()
            .find(|stack| stack.block == BLOCK_FLINT_CHISEL)
            .expect("the chisel");
        assert_eq!(worn.damage, 1, "the chisel came back new");

        // ...and the plain row is still there for a player with no chisel at
        // all: nothing was taken away to add this.
        let mut hands = Inventory::new();
        hands.add(BLOCK_FRAME, 1);
        hands.add(BLOCK_PLANKS, boards(plain));
        assert!(made(&mut hands, plain, bench), "the chisel-less door is gone");
    }

    #[test]
    fn a_row_that_names_the_cheap_tool_takes_the_dear_one_and_wears_the_cheap_one_first() {
        use crate::types::{BLOCK_BRONZE_CHISEL, BLOCK_FLINT_CHISEL, BLOCK_FRAME};
        let pared = named("pared door");
        let bench = Heat::NONE.with_workshop(Station::Bench);
        // A bronze chisel alone satisfies a row that names the flint one.
        let mut bronze = Inventory::new();
        bronze.add(BLOCK_FRAME, 1);
        bronze.add(BLOCK_PLANKS, 2);
        bronze.add(BLOCK_BRONZE_CHISEL, 1);
        assert_eq!(missing_ingredient(&bronze, pared), None, "a bronze chisel is not a chisel");
        assert!(made(&mut bronze, pared, bench), "a bronze chisel would not pare a door");
        assert_eq!(bronze.count(BLOCK_BRONZE_CHISEL), 1);

        // ...and with both in the pack it is the flint one that wears, which
        // is the choice a player would make for themselves.
        let mut both = Inventory::new();
        both.add(BLOCK_FRAME, 1);
        both.add(BLOCK_PLANKS, 2);
        both.add(BLOCK_FLINT_CHISEL, 1);
        both.add(BLOCK_BRONZE_CHISEL, 1);
        assert!(made(&mut both, pared, bench));
        let damage = |kind| {
            both.slots().iter().flatten().find(|s| s.block == kind).map(|s| s.damage).expect("the tool")
        };
        assert_eq!(damage(BLOCK_FLINT_CHISEL), 1, "the cheap chisel was spared");
        assert_eq!(damage(BLOCK_BRONZE_CHISEL), 0, "the dear chisel was ground away");
    }

    #[test]
    fn one_hammer_dresses_a_whole_quarry_and_is_not_counted_as_a_stone() {
        use crate::types::{BLOCK_SANDSTONE, BLOCK_SANDSTONE_BRICKS, BLOCK_STONE_HAMMER};
        let dressed = named("dressed ashlar");
        let mason = Heat::NONE.with_workshop(Station::Mason);
        let mut pack = Inventory::new();
        pack.add(BLOCK_SANDSTONE, 9);
        pack.add(BLOCK_STONE_HAMMER, 1);
        // Three courses off one hammer, not one: a tool that is handed back
        // is not a limit on how many (`possible_crafts`).
        assert_eq!(possible_crafts(&pack, dressed), 3, "one hammer offered one course");
        for _ in 0..3 {
            assert!(made(&mut pack, dressed, mason));
        }
        assert_eq!(pack.count(BLOCK_SANDSTONE_BRICKS), 9, "the hammer bought no stone");
        assert_eq!(pack.count(BLOCK_SANDSTONE), 0);
        assert_eq!(pack.count(BLOCK_STONE_HAMMER), 1, "the hammer was spent");

        // ...and with no hammer at all the row is simply not offered, while
        // the hammerless one still is.
        let mut bare = Inventory::new();
        bare.add(BLOCK_SANDSTONE, 3);
        assert_eq!(possible_crafts(&bare, dressed), 0);
        assert_eq!(feasibility(&bare, dressed, mason), Feasibility::MissingIngredients);
        assert!(made(&mut bare, named("ashlar"), mason), "the hammerless course is gone");
    }

    #[test]
    fn a_tool_handed_back_is_only_ever_a_tool_and_never_an_ingredient() {
        // The rule `used_tool` turns on, held to the whole table: the
        // crucible and the mould are in `inputs` and `returns` together and
        // must *not* be picked up by it, because a pot has no wear and
        // wearing one would be an item that quietly disappears.
        for row in RECIPES {
            let Some(tool) = used_tool(row) else { continue };
            assert!(
                crate::types::tool_durability(tool).is_some(),
                "{} works a {tool} that cannot wear out",
                row.name
            );
            assert!(
                crate::types::is_workshop_tool(tool),
                "{} holds a {tool}, which is a tool used on the world and not at a bench",
                row.name
            );
            // One tool a row, or `tool_slot` would have to choose.
            let held = row
                .inputs
                .iter()
                .filter(|&&(b, _)| crate::types::tool_durability(b).is_some())
                .filter(|&&(b, _)| row.returns.iter().any(|&(back, _)| back == b))
                .count();
            assert_eq!(held, 1, "{} is worked with more than one tool", row.name);
        }
        for name in ["hoe casting", "shovel casting"] {
            assert_eq!(used_tool(named(name)), None, "{name} wears its crucible out");
        }
    }

    #[test]
    fn nails_are_a_dozen_to_a_bar_at_the_kiln_and_a_nailed_chest_needs_no_flint() {
        use crate::types::BLOCK_NAILS;
        let nails = named("nails");
        let mut pack = Inventory::new();
        pack.add(BLOCK_IRON_INGOT, 1);
        assert_eq!(feasibility(&pack, nails, Heat::NONE), Feasibility::NeedsForge);
        let campfire = Heat { fire: true, ..Heat::NONE };
        assert_eq!(feasibility(&pack, nails, campfire), Feasibility::NeedsForge, "a campfire draws iron");
        assert!(smelt(&mut pack, nails, crate::hearth::Kind::Kiln), "a kiln loaded with a bar made no nails");
        assert_eq!(pack.count(BLOCK_NAILS), 12);
        assert!(crate::hearth::needs_degrees(nails).is_some(), "a kiln would run nails at no heat");

        pack.add(BLOCK_PLANKS, 6);
        assert!(made(&mut pack, named("nailed chest"), Heat::NONE));
        assert_eq!(pack.count(BLOCK_CHEST), 1);
        pack.add(BLOCK_STICK, 4);
        assert!(made(&mut pack, named("nailed frame"), Heat::NONE));
        assert_eq!(pack.count(BLOCK_NAILS), 0, "a bar is a chest and a frame, exactly");
    }

    /// **A copper bar alone is hooks, and a copper bar meant for anything
    /// else is not.** The hooks row asks for one bar and nothing more, so the
    /// knife, the spear and the castings that load the same bar and more must
    /// all still be what a hearth makes of their loads.
    #[test]
    fn a_hearth_loaded_for_a_copper_knife_or_a_casting_does_not_bend_the_bar_into_hooks() {
        use crate::hearth::{next_recipe, Kind, INPUT_SLOTS};
        let loads: [&[(BlockId, u32)]; 3] = [
            &[(BLOCK_COPPER_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_SINEW, 1)],
            &[(BLOCK_COPPER_INGOT, 1), (BLOCK_WORKED_STICK, 1), (BLOCK_CORD, 1)],
            &[(BLOCK_COPPER_INGOT, 1), (BLOCK_MOULD, 1)],
        ];
        for load in loads {
            let mut tray = Inventory::new();
            for &(block, count) in load {
                tray.add_within(INPUT_SLOTS, block, count);
            }
            let made = next_recipe(Kind::Kiln, &tray).map(|(_, r)| r.name);
            assert!(made.is_some(), "a kiln loaded with {load:?} made nothing");
            assert_ne!(made, Some("copper hooks"), "a bar loaded with {load:?} was bent into hooks");
        }
        let mut bar = Inventory::new();
        bar.add_within(INPUT_SLOTS, BLOCK_COPPER_INGOT, 1);
        assert_eq!(next_recipe(Kind::Kiln, &bar).map(|(_, r)| r.name), Some("copper hooks"));
        assert_eq!(named("copper hooks").output, (crate::types::BLOCK_COPPER_HOOK, 4));
    }

    #[test]
    fn a_kiln_loaded_for_steel_or_a_knife_does_not_cut_the_bar_into_nails() {
        use crate::hearth::{next_recipe, Kind, INPUT_SLOTS};
        let mut steel = Inventory::new();
        steel.add_within(INPUT_SLOTS, BLOCK_IRON_INGOT, 1);
        steel.add_within(INPUT_SLOTS, BLOCK_COAL, 2);
        assert_eq!(next_recipe(Kind::Kiln, &steel).map(|(_, r)| r.name), Some("steel"));

        let mut knife = Inventory::new();
        knife.add_within(INPUT_SLOTS, BLOCK_IRON_INGOT, 1);
        knife.add_within(INPUT_SLOTS, BLOCK_WORKED_STICK, 1);
        knife.add_within(INPUT_SLOTS, BLOCK_CORD, 1);
        let loaded = next_recipe(Kind::Kiln, &knife).map(|(_, r)| r.name);
        assert_ne!(loaded, Some("nails"), "a hafted bar was cut into nails");

        let mut bar = Inventory::new();
        bar.add_within(INPUT_SLOTS, BLOCK_IRON_INGOT, 1);
        assert_eq!(next_recipe(Kind::Kiln, &bar).map(|(_, r)| r.name), Some("nails"));
    }

    #[test]
    fn bog_iron_costs_twelve_rusty_stones_a_bloom() {
        // The effort the player asked for, in numbers: a bloom without a
        // mine is many rusty stones crushed and smelted long -- four
        // stones a dust, three dust a bloom, and the same three charcoal
        // the ore bloom burns. Twelve stones at twenty columns apart on
        // a riverbank is a long walk, which is the point of it.
        let dust = named("iron dust");
        let bloom = named("bog iron bloom");
        assert_eq!(dust.inputs, &[(BLOCK_RUSTY_STONE, 4)]);
        assert_eq!(dust.output, (BLOCK_IRON_DUST, 1));
        let dust_a_bloom = bloom
            .inputs
            .iter()
            .find(|&&(b, _)| b == BLOCK_IRON_DUST)
            .map(|&(_, n)| n)
            .expect("the bog bloom is not made of dust");
        assert_eq!(dust_a_bloom * dust.inputs[0].1, 12, "a bloom is not twelve stones");
        assert_eq!(bloom.output, (BLOCK_IRON_BLOOM, 1));
        assert_eq!(bloom.station, Station::Bloomery, "bog iron is smelted in the shaft like any iron");
        let ore = named("iron bloom");
        assert_eq!(
            bloom.inputs.iter().find(|&&(b, _)| b == BLOCK_COAL),
            ore.inputs.iter().find(|&&(b, _)| b == BLOCK_COAL),
            "the bog bloom burns a different amount of charcoal from the ore bloom"
        );
        // Crushing is hand work, anywhere: the stones are knocked
        // together, not fired.
        assert_eq!(dust.station, Station::Hands);
    }

    #[test]
    fn no_tool_can_be_made_without_its_head_and_its_haft() {
        // The old one-click recipe is gone, and this is what makes sure
        // it has not grown back somewhere: every tool in the game is
        // bound from a head and a haft, and neither of them is raw
        // material. A recipe that took flint and sticks straight to a
        // pickaxe would pass every other test in this file.
        for r in RECIPES {
            // A hone hands back a tool that already has its head and haft.
            if is_rework(r) {
                continue;
            }
            // **The stone-age tools, and only them.** A metal tool is
            // cast rather than knapped or ground: one step, at a fire,
            // out of an ingot -- see `types::BLOCK_COPPER_KNIFE`. Asking
            // a copper knife for a knapped head would be asking the
            // bronze age to chip stone, which is the thing it stopped
            // doing. Both stone tiers are in: the lashed tool and the
            // glued one it becomes.
            if !matches!(
                crate::blocks::definition(r.output.0).tool,
                Some(crate::blocks::Tier::Stone | crate::blocks::Tier::Flint)
            ) {
                continue;
            }
            // A glued tool is a lashed one set in glue: its head and
            // its haft came in with the lashed tool. See `BLOCK_GLUE`.
            let lashed = r
                .inputs
                .iter()
                .any(|&(b, _)| crate::blocks::definition(b).tool == Some(crate::blocks::Tier::Stone));
            let head = lashed
                || r.inputs.iter().any(|&(b, _)| {
                    matches!(
                        b,
                        BLOCK_FLINT_KNIFE_HEAD | BLOCK_STONE_AXE_HEAD | BLOCK_STONE_PICK_HEAD
                    )
                });
            let haft = lashed || r.inputs.iter().any(|&(b, _)| b == BLOCK_WORKED_STICK);
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
            BLOCK_STONE_AXE_HEAD,
            BLOCK_STONE_PICK_HEAD,
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
        pack.add(BLOCK_CORD, 2);
        assert!(!made(&mut pack, named("stone pick"), KILN), "a pick with no head");
        assert_eq!(pack.count(BLOCK_CORD), 2, "a refused craft spent cord");

        // ...and a haft cannot be whittled by wishing, either.
        let mut bare = Inventory::new();
        bare.add(BLOCK_STICK, 4);
        assert!(!made(&mut bare, named("worked stick"), KILN), "whittled with what?");
    }

    #[test]
    fn the_metal_picks_came_back_at_the_end_of_the_table() {
        // They were recipes 22, 23 and 24 once, then they were nothing,
        // and now they are at the end -- which is the only place a
        // recipe may ever come back to, because the index into this
        // table is a recipe's identity on the wire. A returning name in
        // its *old* slot would be an older client spending a player's
        // flint on the wrong thing.
        for back in ["copper pick", "bronze pick", "iron pick"] {
            let at = RECIPES
                .iter()
                .position(|r| r.name == back)
                .unwrap_or_else(|| panic!("{back} is missing"));
            assert!(at > 24, "{back} is at index {at}, where an older one used to be");
        }
    }

    #[test]
    fn everything_that_melts_or_cooks_asks_for_a_fire() {
        // The rule stated as a list, because the failure mode is silent:
        // a smelting recipe left at `Station::Hands` is a player casting
        // bronze in an empty field, and nothing else in the game would
        // ever complain.
        for r in RECIPES {
            // **Melting is named by its apparatus, not by its output**,
            // and that is a correction rather than a refinement. The
            // rule used to read the output: an ingot, or anything whose
            // tier was above flint. That was a proxy for "this recipe
            // melts metal" and it stopped being one the moment a copper
            // axe became a head tied to a stick -- hafting produces a
            // copper tool and wants no fire at all, and the old rule
            // called it a bronze pour in a meadow.
            //
            // What actually cannot happen without a fire is metal going
            // liquid, and the crucible and the mould are exactly the
            // things a recipe reaches for when it does. So they are the
            // test. Every row this used to catch is still caught: a
            // smelt left at `Hands` has a vessel in it.
            // ...and the pot of earth is the exception the proxy has to name:
            // a crucible in that row is a *pot*, filled with soil and grown
            // in (`types::BLOCK_PLANTER`), and nothing about it is molten.
            let melts = r
                .inputs
                .iter()
                .any(|&(b, _)| b == BLOCK_VESSEL || b == BLOCK_MOULD)
                && r.output.0 != crate::types::BLOCK_PLANTER;
            let is_metal = melts
                || r.output.0 == BLOCK_COPPER_INGOT
                || r.output.0 == BLOCK_TIN_INGOT
                || r.output.0 == BLOCK_BRONZE_INGOT
                || r.output.0 == BLOCK_IRON_INGOT;
            let is_cooking = r.output.0 == BLOCK_COOKED_MEAT;
            if is_metal {
                // **Never by hand, and never all at the same fire.**
                // Melting ore happens in a crucible, which a campfire
                // will heat; alloying wants the kiln; iron wants the
                // bloomery. What the rule has to catch is a metal recipe
                // that needs no fire at all.
                assert_ne!(r.station, Station::Hands, "{} is melted in a meadow", r.name);
                if r.station == Station::Heat {
                    assert!(
                        r.inputs.iter().any(|&(b, _)| b == BLOCK_VESSEL),
                        "{} melts over an open fire with nothing to melt it in",
                        r.name
                    );
                }
            } else if is_cooking {
                assert_eq!(r.station, Station::Heat, "{} happens without a fire", r.name);
            }
        }
        // ...and the other way: nothing a player needs in the first ten
        // minutes may ask for one, or the fire is a wall rather than a
        // step. The campfire itself is the sharp end of that.
        for r in RECIPES {
            if r.output.0 == BLOCK_CAMPFIRE {
                assert_eq!(r.station, Station::Hands, "the fire needs a fire to build");
            }
        }
        for r in RECIPES.iter().filter(|r| {
            crate::blocks::definition(r.output.0).tool == Some(crate::blocks::Tier::Flint)
        }) {
            assert_eq!(r.station, Station::Hands, "{} needs a fire", r.name);
        }
    }

    /// Runs a fire recipe **the way the world runs it now**: load a
    /// hearth with the ingredients, let it finish the batch, and take
    /// what came out back into the pack.
    ///
    /// The tests below check the *chain* -- that clay becomes a pot and
    /// the pot melts ore and the ore becomes an ingot -- and the chain
    /// did not change when the fire grew an inside. What changed is who
    /// runs the step, so this is the two lines that used to be `craft`.
    fn smelt(pack: &mut Inventory, recipe: &Recipe, kind: crate::hearth::Kind) -> bool {
        use crate::hearth::{complete, next_recipe, INPUT_SLOTS};
        let mut fire = Inventory::new();
        for &(block, count) in recipe.inputs {
            // The stack the row accepts, not the id it names: the salt row
            // names a jug of water and means the sea (`refuses`), and a fire
            // loaded with the river is not loaded for it.
            let block = pack
                .slots()
                .iter()
                .flatten()
                .map(|stack| stack.block)
                .find(|&held| crate::types::block_kind(held) == crate::types::block_kind(block) && !refuses(recipe, held))
                .unwrap_or(block);
            if !pack.take_exact(block, count) {
                return false;
            }
            fire.add_within(INPUT_SLOTS, block, count);
        }
        // **Through `next_recipe`, not past it.** This helper is the
        // tests' model of a fire, and a model that ran the recipe it was
        // handed was a model of something the game does not have: the
        // player cannot name a recipe, they can only load a hearth, and
        // `next_recipe` is the whole of what reads the load.
        //
        // It mattered. For two versions the hearth answered a loaded
        // pick with a knife (see `hearth::next_recipe`), and the chain
        // tests below -- including the one that says the metal age is
        // reachable -- were green throughout, because they never asked
        // the question the world asks.
        // By name rather than by address: `RECIPES` is a `const`, so a
        // reference taken at one use site is not the same pointer as one
        // taken at another, and `std::ptr::eq` on two of them is false
        // even when they name the same row.
        let asked_for = next_recipe(kind, &fire).is_some_and(|(_, found)| found.name == recipe.name);
        if !kind.runs(recipe.station) || !asked_for || !complete(&mut fire, recipe) {
            // Put the ingredients back: a refused batch costs nothing,
            // which is the same promise the old hand-craft made.
            for &(block, count) in recipe.inputs {
                pack.add(block, count);
            }
            return false;
        }
        // Everything the fire has when the batch is done: the result
        // from the output tray, and the apparatus that went back into
        // the input slots.
        for slot in 0..crate::hearth::USED_SLOTS {
            if let Some(stack) = fire.slots()[slot] {
                pack.add(stack.block, stack.count);
            }
        }
        true
    }

    #[test]
    fn a_hot_recipe_in_a_cold_field_is_refused_and_costs_nothing() {
        let mut pack = Inventory::new();
        pack.add(BLOCK_COPPER_ORE, 4);
        pack.add(BLOCK_COAL, 4);
        pack.add(BLOCK_VESSEL, 1);
        pack.add(BLOCK_MOULD, 1);
        let recipe = named("copper ingot");

        assert_eq!(feasibility(&pack, recipe, Heat::NONE), Feasibility::NeedsFire);
        // **By hand, never** -- not in a meadow and not standing in the
        // flames. What a fire makes is made in the fire now.
        assert!(!made(&mut pack, recipe, Heat::NONE), "smelted copper in a meadow");
        assert!(!made(&mut pack, recipe, FIRE), "smelted copper by hand at a fire");
        assert_eq!(pack.count(BLOCK_COPPER_ORE), 4, "a refused craft ate the ore");
        assert_eq!(pack.count(BLOCK_COAL), 4);
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "a refused craft ate the crucible");

        // ...and in a hearth it simply works -- and the equipment comes
        // back out of it.
        assert!(smelt(&mut pack, recipe, crate::hearth::Kind::Campfire));
        assert_eq!(pack.count(BLOCK_COPPER_INGOT), 1);
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "the crucible was spent like an ingredient");
        assert_eq!(pack.count(BLOCK_MOULD), 1, "the mould was spent like an ingredient");
    }

    #[test]
    fn what_is_worth_listing_does_not_depend_on_standing_at_a_fire() {
        // The menu asks `has_ingredients`, not `feasibility`, and this
        // is why: the station check short-circuits, so an empty-handed
        // player in a field "passes" every metal recipe and a player who
        // lights a fire watches them all vanish.
        let empty = Inventory::new();
        let smelt = named("bronze ingot");
        assert!(!has_ingredients(&empty, smelt));
        assert_eq!(feasibility(&empty, smelt, Heat::NONE), Feasibility::NeedsForge);
        assert_eq!(
            feasibility(&empty, smelt, KILN),
            Feasibility::MissingIngredients,
            "the answer changed with the fire, so the list must not"
        );
    }

    #[test]
    fn a_campfire_smelts_only_with_a_pot_in_it() {
        // The rung the kiln adds, as the two answers a player gets while
        // standing at a blazing campfire holding ore.
        let mut pack = Inventory::new();
        pack.add(BLOCK_COPPER_ORE, 3);
        pack.add(BLOCK_COAL, 2);
        pack.add(BLOCK_RAW_MEAT, 1);

        // **The equipment is the gate, not the fire.** A player with ore
        // and a blazing campfire and no crucible is short of a crucible,
        // and the menu has to say so rather than sending them off to
        // build a hotter fire.
        assert_eq!(
            feasibility(&pack, named("copper ingot"), FIRE),
            Feasibility::MissingIngredients,
            "it melted copper in a bare fire"
        );
        pack.add(BLOCK_VESSEL, 1);
        pack.add(BLOCK_MOULD, 1);
        assert_eq!(
            feasibility(&pack, named("copper ingot"), FIRE),
            Feasibility::Ready,
            "a pot in a fire is how copper was got for three thousand years"
        );
        assert_eq!(
            feasibility(&pack, named("copper ingot"), Heat::NONE),
            Feasibility::NeedsFire,
            "the pot melted its own ore"
        );
        assert_eq!(
            feasibility(&pack, named("cooked meat"), FIRE),
            Feasibility::Ready,
            "a campfire could not cook dinner"
        );
        // ...and the kiln does both, because it is the hotter of the
        // two and asking a player to lay a second fire beside it would
        // be a chore rather than a decision.
        assert_eq!(feasibility(&pack, named("cooked meat"), KILN), Feasibility::Ready);
    }

    #[test]
    fn a_field_feeds_you_end_to_end() {
        // The whole of farming as one walk: a stand of wild wheat, the
        // seed off it, three stages of crop, grain, flour, dough,
        // bread. Every arrow in it is either a block table row or a
        // recipe, and this is the only place that checks they join up.
        use crate::types::{ripens_into, BLOCK_SEEDS, BLOCK_WHEAT, BLOCK_WHEAT_RIPE};
        // **Where the first seed comes from**, now that a tuft of grass
        // does not give one. If this row ever goes, farming becomes
        // unreachable in a new world and every test below it still
        // passes -- which is exactly why the check is here.
        assert_eq!(
            crate::blocks::definition(crate::types::BLOCK_WILD_WHEAT).drop,
            Some(BLOCK_SEEDS),
            "nothing in the world gives a seed"
        );
        assert_eq!(ripens_into(BLOCK_SEEDS), Some(BLOCK_WHEAT));
        assert_eq!(ripens_into(BLOCK_WHEAT), Some(BLOCK_WHEAT_RIPE));
        assert_eq!(ripens_into(BLOCK_WHEAT_RIPE), None, "ripe wheat keeps growing");
        assert_eq!(
            crate::blocks::definition(BLOCK_WHEAT_RIPE).drop,
            Some(BLOCK_GRAIN),
            "cutting a ripe field yields nothing"
        );

        let mut pack = Inventory::new();
        pack.add(BLOCK_GRAIN, 3);
        // Ground with a stone, and the stone comes back: a player who
        // spends their last pebble on flour has been robbed rather than
        // charged.
        pack.add(crate::types::BLOCK_PEBBLE, 1);
        assert!(made(&mut pack, named("grind grain"), Heat::NONE), "flour by hand");
        assert_eq!(pack.count(crate::types::BLOCK_PEBBLE), 1, "the quern-stone was eaten");
        assert_eq!(pack.count(crate::types::BLOCK_FLOUR), 2);
        // ...and wetted, which is what makes the jug a baker's tool as
        // well as a canteen. The jug comes back empty.
        pack.add(crate::types::BLOCK_JUG_WATER, 1);
        assert!(made(&mut pack, named("dough"), Heat::NONE), "dough by hand");
        assert_eq!(pack.count(crate::types::BLOCK_JUG), 1, "the jug went into the dough");
        assert_eq!(pack.count(crate::types::BLOCK_JUG_WATER), 0, "the water was not spent");
        assert_eq!(
            feasibility(&pack, named("bread"), Heat::NONE),
            Feasibility::NeedsFire,
            "bread baked itself in a field"
        );
        assert!(smelt(&mut pack, named("bread"), crate::hearth::Kind::Campfire), "bread in a fire");
        assert_eq!(pack.count(BLOCK_BREAD), 1);
        // ...and it is worth eating, which is the point of all of it.
        assert!(crate::food::is_food(BLOCK_BREAD));
        assert!(
            crate::food::nutrition(BLOCK_BREAD) < crate::food::nutrition(BLOCK_COOKED_MEAT),
            "a loaf should not beat a roast"
        );
        assert!(
            crate::food::nutrition(BLOCK_BREAD) > crate::food::nutrition(crate::types::BLOCK_BERRIES),
            "a loaf should beat a handful of berries"
        );
    }

    #[test]
    fn nothing_grows_without_being_tilled_first() {
        // The one rule the hoe exists for. Without it a seed goes
        // anywhere a tuft of grass does, and a field is something you
        // scatter rather than something you make.
        use crate::types::{
            can_grow_on, BLOCK_DIRT, BLOCK_FARMLAND, BLOCK_GRASS, BLOCK_SEEDS, BLOCK_STONE,
            BLOCK_WHEAT,
        };
        assert!(can_grow_on(BLOCK_SEEDS, BLOCK_FARMLAND));
        assert!(can_grow_on(BLOCK_WHEAT, BLOCK_FARMLAND));
        for ground in [BLOCK_GRASS, BLOCK_DIRT, BLOCK_STONE] {
            assert!(
                !can_grow_on(BLOCK_SEEDS, ground),
                "seed rooted in {}",
                crate::types::block_name(ground)
            );
        }
        // The hoe is the only implement, and it is not one of the three
        // tools -- see `types::is_implement`.
        assert!(crate::types::is_implement(crate::types::BLOCK_HOE));
        assert_eq!(crate::blocks::definition(crate::types::BLOCK_HOE).tool, None);
    }

    #[test]
    fn the_pot_is_the_smelting_equipment_and_it_survives() {
        // **The claim the whole copper age rests on.** Pottery is not a
        // step before metal, it is the *apparatus*: ore melts inside a
        // fired pot and is poured into a fired mould, and both come out
        // of the fire to be used again. A chain that consumed them would
        // be a chain where the pot is an ingredient, which is another
        // way of saying there is no pot.
        let mut pack = Inventory::new();
        pack.add(BLOCK_CLAY, 8);
        pack.add(BLOCK_SAND, 1);
        assert!(made(&mut pack, named("clay vessel"), Heat::NONE), "thrown by hand");
        assert!(made(&mut pack, named("ingot mould"), Heat::NONE), "pressed by hand");
        assert_eq!(pack.count(BLOCK_VESSEL_RAW), 1);

        // Unfired clay is not equipment. It has to go through the kiln,
        // which is the kiln's real job.
        pack.add(BLOCK_COAL, 4);
        assert_eq!(
            feasibility(&pack, named("fire vessel"), FIRE),
            Feasibility::NeedsForge,
            "a campfire fired pottery"
        );
        assert!(smelt(&mut pack, named("fire vessel"), crate::hearth::Kind::Kiln));
        assert!(smelt(&mut pack, named("fire mould"), crate::hearth::Kind::Kiln));
        assert_eq!(pack.count(BLOCK_VESSEL), 1);

        // Now the ore, at an ordinary campfire -- because what makes the
        // heat is the pot.
        pack.add(BLOCK_COPPER_ORE, 6);
        for _ in 0..2 {
            assert!(
                smelt(&mut pack, named("copper ingot"), crate::hearth::Kind::Campfire),
                "melting"
            );
        }
        assert_eq!(pack.count(BLOCK_COPPER_INGOT), 2);
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "the pot was eaten");
        assert_eq!(pack.count(BLOCK_MOULD), 1, "the mould was eaten");

        // ...and without the pot the same fire does nothing.
        pack.take_exact(BLOCK_VESSEL, 1);
        assert_eq!(
            feasibility(&pack, named("copper ingot"), FIRE),
            Feasibility::MissingIngredients
        );
    }

    #[test]
    fn copper_can_be_had_without_ever_swinging_a_pick() {
        // Native copper is the metal you find rather than mine, and it
        // is the reason the copper age can start above ground. Two
        // nuggets and the pot, and no fuel at all: it is already metal,
        // so there is nothing to reduce.
        let melt = named("melt nuggets");
        assert!(
            !melt.inputs.iter().any(|&(b, _)| b == BLOCK_COAL),
            "melting native copper should not need a reducing fire"
        );
        let mut pack = Inventory::new();
        pack.add(BLOCK_NATIVE_COPPER, 2);
        pack.add(BLOCK_VESSEL, 1);
        pack.add(BLOCK_MOULD, 1);
        assert!(smelt(&mut pack, melt, crate::hearth::Kind::Campfire), "a nugget in a pot over a fire");
        assert_eq!(pack.count(BLOCK_COPPER_INGOT), 1);
        // It is picked up rather than mined -- the whole point of it
        // being the first metal anybody meets.
        assert_eq!(crate::blocks::definition(BLOCK_NATIVE_COPPER).needs, crate::blocks::Tier::Hand);
    }

    #[test]
    fn iron_goes_through_the_shaft_and_not_through_the_pot() {
        // The physical fact this world now models: copper melts at a
        // temperature a crucible reaches and iron does not melt at all
        // at any temperature the ancient world could make. Iron is
        // *reduced* in a shaft and comes out as a bloom.
        let bloom = named("iron bloom");
        assert_eq!(bloom.station, Station::Bloomery);
        assert!(
            !bloom.inputs.iter().any(|&(b, _)| b == BLOCK_VESSEL),
            "iron was melted in a pot"
        );

        let mut pack = Inventory::new();
        pack.add(BLOCK_IRON_ORE, 3);
        pack.add(BLOCK_COAL, 5);
        let shaft = Heat { bloomery: true, ..Heat::NONE };
        assert_eq!(feasibility(&pack, bloom, KILN), Feasibility::NeedsBloomery);
        let _ = shaft;
        assert!(smelt(&mut pack, bloom, crate::hearth::Kind::Bloomery), "the shaft");
        assert_eq!(pack.count(BLOCK_IRON_BLOOM), 1);
        assert!(
            !smelt(&mut pack, bloom, crate::hearth::Kind::Kiln),
            "a kiln won iron"
        );

        // ...and the bloom is not iron yet.
        assert!(
            smelt(&mut pack, named("wrought iron"), crate::hearth::Kind::Bloomery),
            "working the bloom"
        );
        assert_eq!(pack.count(BLOCK_IRON_INGOT), 1);
    }

    #[test]
    fn there_is_a_fuel_that_does_not_need_a_mine() {
        // Coal used to be the only fuel, and coal is a seam behind a
        // flint pick -- so the first ingot a player smelted was gated on
        // digging a mine, which is backwards. Charcoal comes out of the
        // fire they already have.
        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 3);
        let burn = named("charcoal");
        assert_eq!(burn.station, Station::Heat, "charcoal wants a furnace to make fuel");
        assert!(
            smelt(&mut pack, burn, crate::hearth::Kind::Campfire),
            "could not burn wood in a fire"
        );
        assert_eq!(pack.count(BLOCK_COAL), 1);
    }

    #[test]
    fn the_kiln_is_built_by_hand_out_of_the_riverbank() {
        // If the kiln itself needed heat, the whole chain would need a
        // kiln to make a kiln.
        let kiln = named("kiln");
        assert_eq!(kiln.station, Station::Hands);
        let mut pack = Inventory::new();
        pack.add(BLOCK_CLAY, 8);
        pack.add(BLOCK_COBBLESTONE, 4);
        assert!(made(&mut pack, kiln, Heat::NONE), "could not build a kiln in a field");
        assert_eq!(pack.count(BLOCK_KILN), 1);
    }

    #[test]
    fn nothing_that_melts_is_offered_at_a_campfire() {
        // The rule, checked over the whole table rather than at the one
        // recipe a test happened to name: anything whose ingredients are
        // ore or metal is forge work.
        for r in RECIPES {
            let metal = r.inputs.iter().any(|&(b, _)| {
                matches!(
                    b,
                    BLOCK_COPPER_ORE
                        | BLOCK_TIN_ORE
                        | BLOCK_IRON_ORE
                        | BLOCK_COPPER_INGOT
                        | BLOCK_TIN_INGOT
                        | BLOCK_BRONZE_INGOT
                        | BLOCK_IRON_INGOT
                )
            });
            if metal {
                assert_ne!(r.station, Station::Hands, "{} is smelted in a meadow", r.name);
            }
        }
    }

    #[test]
    fn the_fire_is_the_only_thing_a_cold_recipe_can_be_short_of() {
        // A recipe that needs no fire must behave exactly as it did
        // before stations existed, whether or not there is one nearby.
        // Otherwise every hand recipe quietly became a fireside one.
        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 1);
        assert_eq!(feasibility(&pack, &RECIPES[0], Heat::NONE), Feasibility::Ready);
        assert!(made(&mut pack, &RECIPES[0], Heat::NONE));
        assert_eq!(pack.count(BLOCK_PLANKS), 4);
    }

    #[test]
    fn the_whole_metal_age_is_reachable_from_the_stone_one() {
        // The counterpart of `the_whole_chain_runs_on_what_the_world_
        // leaves_lying_about`, one age up: **a player with ore, coal, a
        // haft, a sinew and a fire ends up holding an iron pick**, and
        // every ingredient after the first line comes out of a recipe.
        //
        // Worth a test for the same reason: the chain is five steps
        // deep, it crosses two ages and a hunt, and the failure mode is
        // silent -- a bronze recipe asking for an ingot count the smelt
        // cannot supply is a metal age nobody can enter.
        let mut pack = Inventory::new();
        pack.add(BLOCK_COPPER_ORE, 27);
        pack.add(BLOCK_TIN_ORE, 6);
        pack.add(BLOCK_COAL, 24);
        pack.add(BLOCK_WORKED_STICK, 2);
        pack.add(BLOCK_SINEW, 3);
        pack.add(BLOCK_CLAY, 8);
        pack.add(BLOCK_SAND, 1);

        // **The pottery comes first, and that is the whole change.** No
        // amount of ore and fuel is copper without a pot to melt it in
        // and a mould to pour it into -- so the walk starts at a
        // riverbank and goes through the kiln.
        assert!(made(&mut pack, named("clay vessel"), Heat::NONE), "throwing the pot");
        assert!(made(&mut pack, named("ingot mould"), Heat::NONE), "pressing the mould");
        assert!(smelt(&mut pack, named("fire vessel"), crate::hearth::Kind::Kiln), "firing the pot");
        assert!(smelt(&mut pack, named("fire mould"), crate::hearth::Kind::Kiln), "firing the mould");

        for _ in 0..9 {
            assert!(smelt(&mut pack, named("copper ingot"), crate::hearth::Kind::Campfire), "copper");
        }
        for _ in 0..3 {
            assert!(smelt(&mut pack, named("tin ingot"), crate::hearth::Kind::Campfire), "tin");
        }
        assert_eq!(pack.count(BLOCK_COPPER_INGOT), 9);
        // ...and one pot did all twelve pours.
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "the crucible did not survive the age");

        // A copper pick first -- which is the point of copper, because
        // iron ore is the one block in the game a tier *gates*.
        //
        // **Two steps now**, and the walk says so: the kiln pours a head
        // and the player ties it to a stick by hand. See the copper
        // bench in the table above for why the second step is not a
        // formality.
        assert!(smelt(&mut pack, named("pick casting"), crate::hearth::Kind::Kiln), "pick head");
        assert_eq!(pack.count(BLOCK_COPPER_PICK_HEAD), 1);
        assert!(made(&mut pack, named("copper pick"), Heat::NONE), "hafting the pick");
        assert_eq!(pack.count(BLOCK_COPPER_PICKAXE), 1);
        // ...and the crucible and the mould came back out of the fire,
        // as they do from every other pour. A chain that spent them
        // would be a chain that needs a new pot per tool.
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "the crucible did not survive the pour");
        assert_eq!(pack.count(BLOCK_MOULD), 1, "the mould did not survive the pour");
        assert!(
            crate::types::is_breakable_with(BLOCK_IRON_ORE, Some(BLOCK_COPPER_PICKAXE)),
            "the copper pick does not open iron ore, which is what it is for"
        );
        assert!(
            !crate::types::is_breakable_with(BLOCK_IRON_ORE, Some(BLOCK_STONE_PICKAXE)),
            "flint still opens iron ore, so copper is a formality"
        );

        // ...then bronze, out of what is left.
        assert!(smelt(&mut pack, named("bronze ingot"), crate::hearth::Kind::Kiln), "bronze");
        assert_eq!(pack.count(BLOCK_BRONZE_INGOT), 5);
        assert!(smelt(&mut pack, named("bronze pick"), crate::hearth::Kind::Kiln), "bronze pick");
        assert_eq!(pack.count(BLOCK_BRONZE_PICKAXE), 1);
    }

    #[test]
    fn all_four_copper_tools_can_actually_be_made() {
        // **The trap this guards against is silent**, which is why it is
        // a test and not a comment. A hearth is not told what to make:
        // it infers a recipe from what is in the tray, so two castings
        // asking for the same load are one casting and the other is
        // unreachable for ever -- no error, no warning, just a recipe in
        // the table nobody in the world can run. Four castings of one
        // metal in one mould is exactly the shape that goes wrong.
        //
        // So: pour each of the four and haft it, from one pack, with one
        // crucible and one mould.
        let mut pack = Inventory::new();
        pack.add(BLOCK_COPPER_INGOT, 7);
        pack.add(BLOCK_VESSEL, 1);
        pack.add(BLOCK_MOULD, 1);
        pack.add(BLOCK_WORKED_STICK, 4);
        pack.add(BLOCK_SINEW, 4);

        for (casting, head, hafting, tool) in [
            ("hoe casting", BLOCK_COPPER_HOE_HEAD, "copper hoe", BLOCK_COPPER_HOE),
            (
                "shovel casting",
                BLOCK_COPPER_SHOVEL_HEAD,
                "copper shovel",
                BLOCK_COPPER_SHOVEL,
            ),
            ("axe casting", BLOCK_COPPER_AXE_HEAD, "copper axe", BLOCK_COPPER_AXE),
            (
                "pick casting",
                BLOCK_COPPER_PICK_HEAD,
                "copper pick",
                BLOCK_COPPER_PICKAXE,
            ),
        ] {
            assert!(
                smelt(&mut pack, named(casting), crate::hearth::Kind::Kiln),
                "{casting} could not be poured"
            );
            assert_eq!(pack.count(head), 1, "{casting} poured nothing");
            // **By hand, and that is the half the forge was hiding.**
            // Tying a head to a stick wants a dry seat, not a thousand
            // degrees -- and it is the station the flint chain hafts at.
            assert!(
                made(&mut pack, named(hafting), Heat::NONE),
                "{hafting} could not be hafted away from a fire"
            );
            assert_eq!(pack.count(tool), 1, "{hafting} made nothing");
            assert_eq!(pack.count(head), 0, "{hafting} left the head behind");
        }
        // Seven ingots in, four tools out, and the apparatus still in the
        // pack: one pot and one mould did every pour, as they do for
        // every other thing this game melts.
        assert_eq!(pack.count(BLOCK_COPPER_INGOT), 0, "the metal did not all go in");
        assert_eq!(pack.count(BLOCK_VESSEL), 1, "the crucible did not survive");
        assert_eq!(pack.count(BLOCK_MOULD), 1, "the mould did not survive");
    }

    #[test]
    fn every_wound_can_be_dressed_by_hand_with_what_a_first_evening_gathers() {
        // **A player is hurt before they have a fire.** The wolf comes on
        // the first night, so every dressing is `Hands` and every input is
        // something a meadow, a wood or a cotton field gives without a
        // station -- one row moved to a kiln and a bite on day one is a
        // death rather than a decision.
        use crate::injury::{Kind, Treatment};
        for (dressing, heals) in [
            (BLOCK_BANDAGE, Kind::Cut),
            (BLOCK_SPLINT, Kind::Fracture),
            (BLOCK_POULTICE, Kind::Burn),
        ] {
            let rows: Vec<&Recipe> = RECIPES.iter().filter(|r| r.output.0 == dressing).collect();
            assert!(!rows.is_empty(), "nothing makes a {dressing}");
            assert!(
                rows.iter().any(|r| r.inputs.iter().all(|&(b, _)| {
                    matches!(b, BLOCK_FIBER | BLOCK_STICK | BLOCK_BRACKET_FUNGUS)
                })),
                "no {dressing} is made of what the first evening gathers"
            );
            for r in &rows {
                assert_eq!(r.station, Station::Hands, "{} needs a station", r.name);
                assert_eq!(r.failure, 0.0, "{} can fail", r.name);
            }
            let treatment = Treatment::of(dressing).expect("a dressing is a treatment");
            assert!(treatment.suits().contains(&heals), "a {dressing} does not dress a {heals:?}");
        }
    }

    #[test]
    fn a_metal_tool_cannot_be_bound_with_grass() {
        // What ties the metal age to the animals: fibre held a flint
        // head on and will not hold a metal one, so every metal tool
        // asks for sinew -- which only a butchered carcass gives up.
        // It used to be a strip of hide; see the copper bench.
        for r in RECIPES {
            let tier = crate::blocks::definition(r.output.0).tool;
            // A hone is not a binding: the sinew came in on the tool.
            if !tier.is_some_and(|t| t > crate::blocks::Tier::Flint) || is_rework(r) {
                continue;
            }
            assert!(
                r.inputs.iter().any(|&(b, _)| b == BLOCK_SINEW),
                "{} is bound with something that is not sinew",
                r.name
            );
            assert!(
                !r.inputs.iter().any(|&(b, _)| b == BLOCK_FIBER),
                "{} is lashed with grass",
                r.name
            );
        }
    }

    #[test]
    fn an_unknown_recipe_index_is_not_a_panic() {
        assert!(recipe(RECIPES.len()).is_none());
        assert!(recipe(usize::MAX).is_none());
    }

    #[test]
    fn a_hone_takes_the_bluntest_tool_and_hands_that_one_back_sharp() {
        use crate::inventory::Stack;
        use crate::tools::{blunt_step, edge_swings, with_edge, BLUNTEST};
        let hone = RECIPES
            .iter()
            .find(|r| r.name == "hone" && r.output.0 == BLOCK_COPPER_AXE)
            .expect("no row hones a copper axe");
        let edge = edge_swings(BLOCK_COPPER_AXE).unwrap();

        // A sharp axe alone is not honed: that would grind metal off for
        // nothing.
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::worn(BLOCK_COPPER_AXE, 1, 10));
        pack.add(crate::types::BLOCK_WHETSTONE, 1);
        assert_eq!(feasibility(&pack, hone, Heat::NONE), Feasibility::MissingIngredients, "a sharp axe was honed");
        assert_eq!(possible_crafts(&pack, hone), 0);

        // With a blunt one beside it, the blunt one is the axe honed, and it
        // is the same axe: its wear carried on to the next edge, not reset.
        pack.put_in_slot(5, Stack::worn(with_edge(BLOCK_COPPER_AXE, BLUNTEST), 1, 3 * edge + 7));
        assert!(made(&mut pack, hone, Heat::NONE), "the blunt axe could not be honed");
        let axes: Vec<Stack> = pack
            .slots()
            .iter()
            .flatten()
            .filter(|s| crate::types::block_kind(s.block) == BLOCK_COPPER_AXE)
            .copied()
            .collect();
        assert_eq!(axes.len(), 2, "a hone made or lost an axe");
        let honed = axes.iter().find(|s| s.damage == 4 * edge).expect("the blunt axe was not the one honed");
        assert_eq!(blunt_step(honed.block), 0, "the honed axe is still blunt");
        assert!(
            axes.iter().any(|s| s.damage == 10 && blunt_step(s.block) == 0),
            "the sharp axe was worked instead"
        );
        assert_eq!(pack.count(crate::types::BLOCK_WHETSTONE), 1, "the whetstone was used up");
    }

    #[test]
    fn smearing_paste_on_a_worn_spear_does_not_mend_it() {
        // The bug the rework path closed: a poisoned spear used to come out
        // of the menu new, so two toadstools were a repair.
        use crate::inventory::Stack;
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::worn(BLOCK_FLINT_SPEAR, 1, 50));
        pack.add(BLOCK_TOADSTOOL, 2);
        assert!(made(&mut pack, named("poison spear"), Heat::NONE));
        let spear = pack
            .slots()
            .iter()
            .flatten()
            .find(|s| crate::types::is_weapon(s.block))
            .copied()
            .expect("the spear went missing");
        assert!(crate::types::is_poisoned(spear.block));
        assert_eq!(spear.damage, 50, "two toadstools mended a spear");
    }

    #[test]
    fn dough_is_kneaded_with_river_or_pond_water_and_never_the_sea() {
        use crate::body::Water;
        use crate::types::{jug_of, BLOCK_FLOUR};
        let dough = named("dough");
        let mut pack = Inventory::new();
        pack.add(BLOCK_FLOUR, 2);
        pack.add(jug_of(Water::Salt), 1);
        assert!(!has_ingredients(&pack, dough), "sea water counted for dough");
        assert_eq!(missing_ingredient(&pack, dough), Some((BLOCK_JUG_WATER, 1)));
        assert!(!made(&mut pack, dough, Heat::NONE), "bread was kneaded with the sea");
        assert_eq!(pack.count(BLOCK_JUG_WATER), 1, "a refused dough spent the jug");

        // A pond jug beside it: the pond goes in, the sea stays where it was.
        pack.add(jug_of(Water::Standing), 1);
        assert!(made(&mut pack, dough, Heat::NONE), "pond water refused, though the oven makes it safe");
        let left: Vec<BlockId> = pack
            .slots()
            .iter()
            .flatten()
            .filter(|s| crate::types::block_kind(s.block) == BLOCK_JUG_WATER)
            .map(|s| s.block)
            .collect();
        assert_eq!(left, vec![jug_of(Water::Salt)], "the wrong jug went into the dough");
    }

    /// What `craft` answers for a row by hand: `None` where it refuses one.
    fn crafting_at_hand(recipe: &'static Recipe) -> Option<Crafted> {
        let mut pack = Inventory::new();
        for &(block, amount) in recipe.inputs {
            pack.add(block, amount);
        }
        match craft(&mut pack, recipe, Heat { fire: true, ..Heat::NONE }, Attempt::Succeeds) {
            Crafted::Refused => None,
            made => Some(made),
        }
    }

    #[test]
    fn salt_is_boiled_out_of_the_sea_at_a_fire_and_never_out_of_the_river() {
        use crate::body::Water;
        use crate::types::{jug_of, BLOCK_SALT};
        // A hearth row, so it is run at the hearth: `craft` refuses those by
        // hand, which is what stops a fire being a fuel-free furnace.
        use crate::hearth::{next_recipe, Kind, INPUT_SLOTS};
        let boil = named("boil salt");
        assert!(boil.station.is_hearth(), "salt is boiled, not made in the hand");
        assert_eq!(crafting_at_hand(boil), None, "salt was made in the hand");

        let mut tray = Inventory::new();
        tray.put_in_slot(INPUT_SLOTS.start, crate::inventory::Stack::new(jug_of(Water::Fresh), 1));
        assert!(
            next_recipe(Kind::Campfire, &tray).is_none_or(|(_, row)| row.name != "boil salt"),
            "the river was loaded for salt"
        );
        let mut tray = Inventory::new();
        tray.put_in_slot(INPUT_SLOTS.start, crate::inventory::Stack::new(jug_of(Water::Salt), 1));
        assert_eq!(
            next_recipe(Kind::Campfire, &tray).map(|(_, row)| row.name),
            Some("boil salt"),
            "a jug of the sea on the fire is not loaded for salt"
        );
        assert_eq!(boil.output.0, BLOCK_SALT);
        assert_eq!(boil.returns, &[(BLOCK_JUG, 1)], "the jug is spent with the water");
    }

    #[test]
    fn meat_and_fish_are_salted_by_hand_two_to_a_handful() {
        use crate::types::{BLOCK_RAW_FISH, BLOCK_SALT, BLOCK_SALTED_FISH, BLOCK_SALTED_MEAT};
        for (raw, salted, name) in [
            (BLOCK_RAW_MEAT, BLOCK_SALTED_MEAT, "salt meat"),
            (BLOCK_RAW_FISH, BLOCK_SALTED_FISH, "salt fish"),
        ] {
            let recipe = named(name);
            assert_eq!(recipe.station, Station::Hands, "{name} wants a station");
            let mut pack = Inventory::new();
            pack.add(raw, 2);
            assert!(!made(&mut pack, recipe, Heat::NONE), "{name} with no salt");
            pack.add(BLOCK_SALT, 1);
            assert!(made(&mut pack, recipe, Heat::NONE), "{name} refused");
            assert_eq!(pack.count(salted), 2);
            assert_eq!(pack.count(BLOCK_SALT), 0, "the salt was not spent");
            assert_eq!(pack.count(raw), 0);
        }
    }

    #[test]
    fn steel_is_a_bar_carburised_in_a_kiln_and_the_edge_is_quenched_in_water() {
        use crate::types::{jug_of, BLOCK_STEEL_INGOT};
        let steel = named("steel");
        assert_eq!(steel.station, Station::Forge);
        let mut pack = Inventory::new();
        pack.add(BLOCK_IRON_INGOT, 2);
        pack.add(BLOCK_COAL, 4);
        assert!(
            !smelt(&mut pack, steel, crate::hearth::Kind::Bloomery),
            "the shaft's draught carburised a bar it should have burnt"
        );
        for _ in 0..2 {
            assert!(smelt(&mut pack, steel, crate::hearth::Kind::Kiln), "carburising in a kiln");
        }
        assert_eq!(pack.count(BLOCK_STEEL_INGOT), 2);

        pack.add(BLOCK_WORKED_STICK, 1);
        pack.add(BLOCK_SINEW, 2);
        pack.add(jug_of(crate::body::Water::Salt), 1);
        assert!(smelt(&mut pack, named("steel axe"), crate::hearth::Kind::Kiln), "quenching");
        let axe = pack
            .slots()
            .iter()
            .flatten()
            .find(|s| crate::types::block_kind(s.block) == BLOCK_IRON_AXE)
            .copied()
            .expect("no axe came out of the quench");
        assert!(crate::tools::is_hardened(axe.block), "the quench left the axe soft");
        assert_eq!(pack.count(BLOCK_JUG), 1, "the jug did not come back empty");
    }

    #[test]
    fn birch_burns_to_charcoal_at_a_fire_as_oak_does() {
        let mut pack = Inventory::new();
        pack.add(BLOCK_BIRCH_LOG, 3);
        assert!(smelt(&mut pack, named("birch charcoal"), crate::hearth::Kind::Campfire));
        assert_eq!(pack.count(BLOCK_COAL), 1);
        assert_eq!(pack.count(BLOCK_BIRCH_LOG), 0);
    }

    #[test]
    fn a_sawn_log_is_six_boards_and_the_saw_comes_back_a_point_more_worn() {
        use crate::types::{BLOCK_COPPER_SAW, BLOCK_IRON_SAW, BLOCK_PINE_LOG, BLOCK_PINE_PLANKS};
        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 1);
        pack.add(BLOCK_COPPER_SAW, 1);
        assert!(made(&mut pack, named("sawn planks"), Heat::NONE), "a log and a saw made nothing");
        assert_eq!(pack.count(BLOCK_PLANKS), 6, "a sawn log is not six boards");
        let saw = pack.slots().iter().flatten().find(|s| s.block == BLOCK_COPPER_SAW).expect("the saw was spent");
        assert_eq!(saw.wear(), 1, "sawing a log did not wear the saw");
        // Split by hand it is four: the saw is the difference, and the
        // decision.
        let mut pack = Inventory::new();
        pack.add(BLOCK_LOG, 1);
        assert!(made(&mut pack, named("planks"), Heat::NONE));
        assert_eq!(pack.count(BLOCK_PLANKS), 4);
        // An iron saw does the copper saw's row, and a pine log is pine boards.
        let mut pack = Inventory::new();
        pack.add(BLOCK_PINE_LOG, 1);
        pack.add(BLOCK_IRON_SAW, 1);
        assert!(made(&mut pack, named("sawn pine"), Heat::NONE), "an iron saw would not saw");
        assert_eq!(pack.count(BLOCK_PINE_PLANKS), 6);
        assert_eq!(pack.count(BLOCK_PLANKS), 0, "a pine log was sawn into oak");
    }

    #[test]
    fn sawing_and_beaming_cannot_make_wood_out_of_nothing() {
        // The mill `Station::Bench` refused, asked of every wood: a log
        // sawn into boards and the boards glued back into beams must never
        // come out with more logs than it went in with.
        for wood in crate::wood::WOODS.iter() {
            let saw_row = RECIPES
                .iter()
                .find(|r| r.output.0 == wood.planks && used_tool(r).is_some())
                .unwrap_or_else(|| panic!("no sawn row for {}", crate::types::block_name(wood.log)));
            let beam_row = RECIPES
                .iter()
                .find(|r| r.output.0 == wood.log && r.inputs.iter().any(|&(b, _)| b == wood.planks))
                .expect("no beam row");
            let mut pack = Inventory::new();
            pack.add(wood.log, 12);
            pack.add(crate::types::BLOCK_IRON_SAW, 1);
            while made(&mut pack, saw_row, Heat::NONE) {}
            while made(&mut pack, beam_row, Heat::NONE) {}
            assert!(
                pack.count(wood.log) <= 12,
                "sawing and beaming {} made {} logs out of 12",
                crate::types::block_name(wood.log),
                pack.count(wood.log)
            );
        }
    }

    #[test]
    fn a_piece_begun_at_the_sawhorse_is_of_the_wood_its_boards_were() {
        use crate::types::{furniture_wood, BLOCK_PINE_PLANKS};
        let pine = crate::wood::WOODS.iter().position(|wood| wood.planks == BLOCK_PINE_PLANKS).unwrap();
        let mut pack = Inventory::new();
        pack.add(BLOCK_PINE_PLANKS, 2);
        pack.add(crate::types::BLOCK_FRAME, 1);
        pack.add(BLOCK_STICK, 2);
        let bench = Heat::NONE.with_workshop(Station::Bench);
        let (piece, count, boards) = begin_piece(&mut pack, named("chair"), bench).expect("pine boards made no chair");
        assert_eq!((crate::types::block_kind(piece), count), (BLOCK_CHAIR, 1));
        assert_eq!(furniture_wood(piece), pine, "a chair of pine boards is not pine");
        assert_eq!(boards, BLOCK_PINE_PLANKS, "the offcut of a pine chair is not pine");
        assert_eq!(pack.count(piece), 0, "the piece stayed in the pack before the cut was judged");
        assert_eq!(pack.count(BLOCK_PINE_PLANKS), 0, "the boards were not spent");
        // ...and a pack that cannot pay is left as it was.
        let mut short = Inventory::new();
        short.add(BLOCK_PLANKS, 1);
        let was = short.clone();
        assert!(begin_piece(&mut short, named("chair"), bench).is_none());
        assert_eq!(short.count(BLOCK_PLANKS), was.count(BLOCK_PLANKS));
    }

    #[test]
    fn a_blunt_chisel_makes_worse_work_than_a_sharp_one() {
        use crate::tools::{with_edge, BLUNTEST};
        use crate::types::BLOCK_BRONZE_CHISEL;
        let row = RECIPES.iter().find(|r| used_tool(r).is_some_and(|t| crate::types::block_kind(t) == crate::types::BLOCK_FLINT_CHISEL)).expect("no chisel row");
        let with = |chisel| {
            let mut pack = Inventory::new();
            pack.add(chisel, 1);
            tool_goodness(&pack, row)
        };
        assert!(
            with(with_edge(BLOCK_BRONZE_CHISEL, BLUNTEST)) < with(BLOCK_BRONZE_CHISEL),
            "the edge of a chisel does not reach the bench"
        );
    }
}
