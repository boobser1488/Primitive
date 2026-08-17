//! **Every block, in one table.**
//!
//! ## What this is for
//!
//! Adding a block used to mean finding it in twenty separate `match`
//! arms scattered down a fifteen-hundred-line file -- one for how hard
//! it is, one for what it drops, one for whether light goes through it,
//! one for whether it may be placed, and so on down. Miss one and the
//! block is subtly wrong in a way nothing catches: a new stone that
//! weighs nothing, a new plant that stops light, a new ore that cannot
//! be picked up. The birch was added that way and touched eleven of
//! them.
//!
//! Here a block is **one row**. Fill it in, add a line to
//! `assets/textures/blocks.toml` and a line to the client's
//! `embedded.rs`, and the block exists everywhere: the mesher draws it,
//! the collider stops at it, the light engine attenuates through it, the
//! anti-cheat accepts it, the hotbar offers it and the crafting menu can
//! name it.
//!
//! ## What is here and what is not
//!
//! Here: everything that is a *property of the material*, one value per
//! kind. That is what a row is.
//!
//! Not here: anything that depends on more than the block. Which way a
//! log lies is in its id rather than in this table; how deep a cell of
//! water is belongs to the flow simulation; what may grow on what is a
//! question about a *pair*. Those live in `types` beside the bit
//! twiddling they are made of, and they are a handful of functions
//! rather than twenty.
//!
//! `types` re-exports every predicate this table feeds, so nothing
//! outside had to learn a new name.

use crate::types::{block_kind, BlockId, BLOCK_AIR};
#[allow(unused_imports)]
use crate::types::*;

/// The shape a block is drawn and collided as.
///
/// Four, and they are genuinely different things rather than four sizes
/// of one thing -- see the mesher, which has a separate path for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Six faces filling the cell.
    Cube,
    /// Two crossed planes standing in it: a tuft of grass.
    Cross,
    /// One quad lying on the floor of it: a pebble, a nodule of flint.
    Flat,
    /// Not a block in the world at all -- a thing you carry, drawn only
    /// where it has been dropped or is being held.
    Item,
}

/// What a tool is made of -- which is to say *when* it is from.
///
/// The order is the order the ages came in, and the order is the whole
/// mechanism: a tool opens every block whose `needs` is at or below its
/// own tier, and works faster the further above it stands. So the ladder
/// is not a list of numbers someone balanced, it is a history, and a
/// player climbs it the way the world did.
///
/// **Flint rather than wood.** The obvious first tier elsewhere is a
/// wooden pick, and it is pure convention -- wood does not cut rock, and
/// a game whose whole argument is that its materials behave like
/// materials cannot open with the one tool that never existed. Flint
/// did: a nodule knapped to an edge, bound to a haft with fibre, and
/// every one of those three things is already lying on the ground in
/// this world before any of this was added.
///
/// **Copper, bronze and iron are here and nothing is made of them
/// yet.** The metals themselves are in the game -- ore, smelting,
/// alloying -- but every tool a player can hold is flint, because the
/// stone age is what this game is currently about and a stone age with
/// a metal escape hatch in it is a prologue rather than a setting. The
/// rungs stay in the enum: they are what the metals are *for*, and an
/// ordering with holes punched in it and refilled later is a worse
/// thing to read than an ordering with a landing at the top of it. What
/// is enforced instead is that no block asks for a tier no tool has --
/// see `blocks::tests::nothing_needs_a_tool_that_cannot_be_made`.
///
/// `Hand` is a tier so that "no tool at all" is a value in the same
/// ordering rather than a special case in every comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Hand,
    Flint,
    Copper,
    Bronze,
    Iron,
}

impl Tier {
    /// How fast this tier works, as a multiplier on a block's hardness.
    ///
    /// One for bare hands, so every number in the table below keeps
    /// meaning exactly what it meant before tools existed: seconds.
    ///
    /// The gaps are deliberately uneven. Flint doubles what hands do --
    /// that is the difference between a stone edge and no edge at all --
    /// and copper barely improves on flint, because a copper edge rolls
    /// over the first time it meets rock and the metal's real advantage
    /// is that it can be *remade*. Bronze is the jump: alloying it is
    /// what turned soft metal into a tool that holds. Iron is another
    /// half again on top.
    #[inline]
    pub fn speed(self) -> f32 {
        match self {
            Tier::Hand => 1.0,
            Tier::Flint => 2.0,
            Tier::Copper => 2.4,
            Tier::Bronze => 4.0,
            Tier::Iron => 6.0,
        }
    }
}

/// What kind of work a block asks for -- and, read off a tool's row,
/// what kind of work that tool does.
///
/// **One field with two readings, because it is one question.** A block
/// says "this is rock" and a pickaxe says "I work rock"; storing those
/// separately would be two lists that have to agree, and the agreement
/// is the whole mechanism.
///
/// This exists because `Tier` alone cannot express the stone age. A tier
/// is a *when*, and an ordering: flint is worse than bronze at
/// everything. But a flint axe and a flint pick are the same when and
/// are not interchangeable at all -- an axe does not open rock and a
/// pick does not fell a tree, and the difference between them is not
/// that one is better. Gating a standing trunk by tier alone would mean
/// the first pickaxe felled forests, which is exactly the thing this
/// game refuses to say about materials.
///
/// The alternative considered and dropped was a separate `needs_tool:
/// Option<ToolKind>` beside `needs: Tier`, which reads the same and
/// costs a second nullable field that is meaningless whenever the first
/// one is `Hand`. `Any` says the same thing with no second field: work
/// nobody needs a special tool for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    /// Digging, carrying, pulling apart -- everything hands do. Any tool
    /// helps here, because anything with a haft beats a fist.
    Any,
    /// Rock and ore. A pick.
    Stone,
    /// Standing timber. An axe.
    Wood,
    /// Growing things: turf, leaves, a tuft of grass. A knife.
    Plant,
}

/// What the material *is*, for the handful of rules that turn on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matter {
    /// Rock, wood, worked stone: it stays where it is put.
    Solid,
    /// Sand, soil, gravel, ash, snow: what a shovel moves.
    Loose,
    /// Water. The only one, and the reason `has_depth` exists.
    Liquid,
}

/// One block, entire.
///
/// Every field is a property of the material and nothing else, so a row
/// can be read straight down and understood without looking anywhere
/// else.
#[derive(Debug, Clone, Copy)]
pub struct BlockDef {
    pub id: BlockId,
    /// What `blocks.toml` calls it, and what the game shows the player.
    pub name: &'static str,
    pub shape: Shape,
    pub matter: Matter,
    /// Light levels taken out of anything passing through, 0..15.
    /// Fifteen is opaque; water is 2; a leaf is 1.
    pub opacity: u8,
    /// Light levels given out. Only glowstone, so far.
    pub emission: u8,
    /// Seconds of mining with a *just adequate* tool -- the one named by
    /// `needs`. `None` means nothing takes this apart at any tier:
    /// water, and a standing trunk, which is not mined but felled.
    ///
    /// For everything `Hand` opens this is still exactly what it always
    /// was, seconds of bare-handed work, because bare hands are the tier
    /// whose speed is one. Nothing in the table changed value when tools
    /// arrived; what changed is that some rows now have a floor under
    /// them.
    pub hardness: Option<f32>,
    /// The lowest tier of tool that gets into this block at all.
    ///
    /// `Hand` for everything a person can shift with their fingers --
    /// soil, sand, gravel, clay, leaves, anything lying on the ground.
    /// Rock and ore are where the ladder starts, and each rung is a
    /// block you cannot reach until you have made the tool before it.
    pub needs: Tier,
    /// What *kind* of work this is, on both readings -- see `Work`.
    ///
    /// For a material: which tool opens it, and which tool is merely
    /// something to hold while you do it by hand. For a tool: what it is
    /// for. A row that is neither (an ingot, a lump of coal) says `Any`,
    /// which is the truth about it: nothing special is needed.
    pub work: Work,
    /// What tier this *is*, if it is a tool rather than a material.
    ///
    /// A field rather than a list of pickaxe ids somewhere else, for the
    /// reason at the top of this file: a block is one row, and "is this
    /// a pick, and how good a one" is a property of the thing.
    pub tool: Option<Tier>,
    /// Seconds of mining once it is *lying down*, for the blocks that
    /// can.
    ///
    /// Only a log has one, and it is the whole path to wood in the early
    /// game: a standing trunk needs a tool nobody has yet, and one that
    /// has come down is deadfall you can pull apart. Worldgen lays
    /// fallen trunks along the way they fell, so the axis already says
    /// which is which and this needs no new state.
    pub felled: Option<f32>,
    /// What breaking it yields. `None` means nothing is left behind.
    pub drop: Option<BlockId>,
    /// What one of them costs to carry.
    pub weight: f32,
    /// How fast you walk over it, as a multiplier. Under one for the
    /// surfaces you push *through* rather than over.
    pub drag: f32,
    /// May a player put one down?
    pub placeable: bool,
    /// Tinted by the climate it grew in.
    pub foliage: bool,
    /// Drawn differently depending on which way it lies.
    pub orientable: bool,
    /// Falls when nothing is under it.
    pub falls: bool,
    /// Has an inventory of its own.
    pub container: bool,
    /// Its texture is turned a random quarter per cell, so a wall of it
    /// does not read as a grid.
    pub turns: bool,
    /// A cube that still needs something under it. Only the cactus:
    /// everything else that needs propping up is a `Cross` or a `Flat`,
    /// and needs it by being one.
    pub propped: bool,
}

/// The placeholder every unknown id resolves to.
///
/// Not air: air is a real answer with real properties. This is "no such
/// block", and every predicate reads a deliberately inert value off it
/// rather than panicking on a byte that came off a socket.
///
/// **Inert is not the same as absent, and the difference is the
/// opacity.** A cell nobody can account for is a cell that must not leak
/// light or ambient occlusion out of itself -- the mesher uses exactly
/// this for the far side of a chunk that has not arrived, and treating
/// it as transparent puts a bright seam along the edge of the loaded
/// world. So the unknown block is a solid, opaque cube that cannot be
/// mined, placed, carried or dropped: it stops everything and offers
/// nothing.
const UNKNOWN: BlockDef = BlockDef {
    id: BLOCK_AIR,
    name: "?",
    shape: Shape::Cube,
    matter: Matter::Solid,
    opacity: 15,
    emission: 0,
    hardness: None,
    felled: None,
    needs: Tier::Hand,
    work: Work::Any,
    tool: None,
    drop: None,
    weight: 0.0,
    drag: 1.0,
    placeable: false,
    foliage: false,
    orientable: false,
    falls: false,
    container: false,
    turns: false,
    propped: false,
};

/// The table. **This is the list of blocks the game has.**
///
/// Order is the order things are offered in -- the hotbar and the
/// texture loader both walk it -- so a new block goes where it belongs
/// in that list rather than at the end.
pub const BLOCKS: &[BlockDef] = &[
    BlockDef {
        id: BLOCK_GRASS,
        name: "grass",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        // Turf is soil with a living skin on it, and what you do to it
        // is dig -- a knife is for cutting things that grow, not for
        // lifting a square metre of earth.
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DIRT),
        weight: 1.3,
        drag: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DIRT,
        name: "dirt",
        shape: Shape::Cube,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DIRT),
        weight: 1.3,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE,
        name: "stone",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // The first row with a floor under it. Stone was simply
        // unbreakable before there was anything to break it with, which
        // meant the whole underground was scenery; now it is the block
        // the first tool is *for*, and six seconds of flint work is what
        // a metre of rock costs.
        hardness: Some(6.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_COBBLESTONE),
        weight: 2.4,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SAND,
        name: "sand",
        shape: Shape::Cube,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SAND),
        weight: 1.6,
        drag: 0.95,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SNOW,
        name: "snow",
        shape: Shape::Cube,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(0.9),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SNOW),
        weight: 0.5,
        drag: 0.55,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WATER,
        name: "water",
        shape: Shape::Cube,
        matter: Matter::Liquid,
        opacity: 2,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        weight: 0.0,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LOG,
        name: "log",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // **A standing trunk comes down now, and only to an axe.** It
        // was `hardness: None` -- unbreakable by anything, ever -- for
        // as long as the only tool in the game was a pick, and that was
        // honest while it lasted: a pick is not an axe, and deadfall was
        // the whole path to wood. Now that there is an axe the trunk has
        // a number, and the number is chosen so that felling a tree with
        // one costs exactly what pulling a fallen one apart by hand does
        // (nine seconds at the flint tier's doubling, against `felled`'s
        // four and a half). The axe does not make wood cheaper; it makes
        // wood something you can go and *get* rather than something you
        // have to find lying down.
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_LOG),
        weight: 0.9,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEAVES,
        name: "leaves",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAVES),
        weight: 0.2,
        drag: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_GLOWSTONE,
        name: "glowstone",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 14,
        hardness: Some(2.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_GLOWSTONE),
        weight: 1.5,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PLANKS,
        name: "planks",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PLANKS),
        weight: 0.7,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COBBLESTONE,
        name: "cobblestone",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COBBLESTONE),
        weight: 2.4,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TALL_GRASS,
        name: "tall_grass",
        shape: Shape::Cross,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        weight: 0.05,
        drag: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CACTUS,
        name: "cactus",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_CACTUS),
        weight: 0.6,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_STICK,
        name: "stick",
        shape: Shape::Flat,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STICK),
        weight: 1.0,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIBER,
        name: "fiber",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FIBER),
        weight: 0.02,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEBBLE,
        name: "pebble",
        shape: Shape::Flat,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PEBBLE),
        weight: 0.15,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT,
        name: "flint",
        shape: Shape::Flat,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT),
        weight: 0.2,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CHEST,
        name: "chest",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CHEST),
        weight: 1.2,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ASH,
        name: "ash",
        shape: Shape::Flat,
        matter: Matter::Loose,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ASH),
        weight: 0.4,
        drag: 0.9,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLAY,
        name: "clay",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(2.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLAY),
        weight: 1.7,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_GRAVEL,
        name: "gravel",
        shape: Shape::Cube,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.7),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_GRAVEL),
        weight: 1.9,
        drag: 0.9,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_LOG,
        name: "birch_log",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // The same tree in a second wood, and the same numbers: see
        // `BLOCK_LOG`. A birch that fell to a knife while an oak needed
        // an axe would be two rules for one material.
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BIRCH_LOG),
        weight: 0.9,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_LEAVES,
        name: "birch_leaves",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BIRCH_LEAVES),
        weight: 0.2,
        drag: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_PLANKS,
        name: "birch_planks",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BIRCH_PLANKS),
        weight: 0.7,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A dead player's pack, standing where they fell. The server puts it
    // there; nothing else can (`placeable: false`), and breaking it
    // yields no block of its own (`drop: None`) -- what comes out is what
    // was inside, spilled by the same path a chest uses.
    //
    // Soft on purpose. A grave you have to spend three and a half seconds
    // quarrying, in the place that just killed you, is a second death; a
    // fifth of a second is long enough that a stray click does not empty
    // it and short enough to be a grab rather than a job.
    BlockDef {
        id: BLOCK_BACKPACK,
        name: "backpack",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        weight: 0.0,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ---- the ores, and what comes out of them ----
    //
    // Four rocks and nine things you carry, appended rather than filed
    // beside the stone they are found in: the hotbar walks this list, and
    // a player who has just learned where the stone is should not have
    // the palette reshuffled under them by a patch. They are in the order
    // the ages came -- coal, copper, tin, iron -- which is also the order
    // a player meets them.
    //
    // Every one of them is `needs: Tier::Flint` or worse, so none of this
    // exists at all until the first tool is made. That is the point: the
    // underground was scenery, and it is now the reason to go down.
    BlockDef {
        id: BLOCK_COAL_ORE,
        name: "coal_ore",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(7.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        // Coal, not the rock it was in. A seam of coal is the one ore
        // that is *already* the material -- there is nothing to smelt out
        // of it -- so carrying the block would be carrying a block of
        // stone with the useful part still stuck inside.
        drop: Some(BLOCK_COAL),
        weight: 2.2,
        drag: 1.0,
        // ...and therefore not placeable, for the same reason turf is
        // not: what you get from breaking it is not this.
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The metal ores drop *themselves*, and so are placeable, which is a
    // deliberate departure from stone. Stone is not placeable because it
    // cannot be got back once put down; an ore can, by the very pick that
    // was needed to collect it in the first place. Nobody is ever holding
    // one of these without also holding the tool that undoes it.
    BlockDef {
        id: BLOCK_COPPER_ORE,
        name: "copper_ore",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(8.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_COPPER_ORE),
        weight: 2.6,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TIN_ORE,
        name: "tin_ore",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(8.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_TIN_ORE),
        weight: 2.6,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_ORE,
        name: "iron_ore",
        shape: Shape::Cube,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Nearly twice the copper ore, and the whole of iron's
        // difficulty is that one number rather than a tier.
        //
        // It used to say `needs: Tier::Copper`: iron was locked until
        // there was a metal pick to open it with. There is no metal pick
        // any more, so a tier lock here would be a block that exists in
        // the world, is drawn in the world, and can never be taken out
        // of it by anything -- which is what "scenery" means and what
        // ore was added to stop being. So flint opens it, and pays
        // through the nose: thirteen seconds of hardness at the flint
        // tier's doubling is six and a half seconds a block, the longest
        // swing in the game by half again. Prising iron out of rock with
        // a stone edge *should* be miserable. It is not forbidden; it is
        // priced, and the price is the argument for the metal tools that
        // will eventually replace it.
        hardness: Some(13.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_IRON_ORE),
        weight: 3.0,
        drag: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COAL,
        name: "coal",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COAL),
        weight: 0.3,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_INGOT,
        name: "copper_ingot",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COPPER_INGOT),
        weight: 0.5,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TIN_INGOT,
        name: "tin_ingot",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_TIN_INGOT),
        weight: 0.45,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_INGOT,
        name: "bronze_ingot",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_INGOT),
        weight: 0.5,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_INGOT,
        name: "iron_ingot",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_INGOT),
        weight: 0.55,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a tool is made of ----
    //
    // Five parts, none of which does anything on its own. They exist
    // because a tool that is one recipe is a tool you buy; a tool that is
    // four is a tool you *make*, and the difference is what this game is
    // about. See `crafting::RECIPES` for the chain and the numbers.
    //
    // All items, all with `tool: None` -- a head is not a tool, it is a
    // stone with an edge on it, and holding one digs exactly as well as
    // holding a fistful of dirt.
    BlockDef {
        id: BLOCK_FLINT_FLAKE,
        name: "flint_flake",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Lighter than the nodule it came off, and there are several of
        // them: a flake is a shard, not a stone.
        drop: Some(BLOCK_FLINT_FLAKE),
        weight: 0.05,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WORKED_STICK,
        name: "worked_stick",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WORKED_STICK),
        // Heavier than the branch it was: a haft is trimmed to a shape,
        // and what is left is the dense part of the wood.
        weight: 0.6,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_KNIFE_HEAD,
        name: "flint_knife_head",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT_KNIFE_HEAD),
        weight: 0.15,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_AXE_HEAD,
        name: "flint_axe_head",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT_AXE_HEAD),
        weight: 0.45,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_PICK_HEAD,
        name: "flint_pick_head",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT_PICK_HEAD),
        weight: 0.5,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the three tools ----
    //
    // Items with a `tool` tier and **no drop**: a tool is never a cell in
    // the world, so nothing can ever break one out of the ground, and a
    // drop of itself would only make the "every drop is something you can
    // use" check ask for a recipe that eats pickaxes.
    //
    // All three are `Tier::Flint` and differ only in `work`, which is the
    // whole point of that field: they are not a ladder, they are a set.
    // A player who has made one has not made progress towards the others.
    BlockDef {
        id: BLOCK_FLINT_KNIFE,
        name: "flint_knife",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: Some(Tier::Flint),
        drop: None,
        // The lightest of the three, and by a lot: a knife is an edge
        // with something to hold it by.
        weight: 0.3,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_AXE,
        name: "flint_axe",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: Some(Tier::Flint),
        drop: None,
        weight: 0.9,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_PICKAXE,
        name: "flint_pickaxe",
        shape: Shape::Item,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: Some(Tier::Flint),
        drop: None,
        weight: 0.8,
        drag: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
];

/// The row for a block, by kind. Falls back to a placeholder rather than
/// panicking: ids arrive over a socket.
///
/// A linear scan of a two-dozen-entry table, in a function small enough
/// to inline. The whole table is a couple of cache lines and the loop is
/// perfectly predicted; a lookup array built at startup would be a
/// second source of truth for nothing.
#[inline]
pub fn definition(id: BlockId) -> &'static BlockDef {
    let kind = block_kind(id);
    let mut i = 0;
    while i < BLOCKS.len() {
        if BLOCKS[i].id == kind {
            return &BLOCKS[i];
        }
        i += 1;
    }
    &UNKNOWN
}

/// Whether this id names a block at all. Air does, and is not in the
/// table -- it is the absence the table lists the alternatives to.
#[inline]
pub fn is_defined(id: BlockId) -> bool {
    let kind = block_kind(id);
    kind == BLOCK_AIR || BLOCKS.iter().any(|b| b.id == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_reachable_and_unique() {
        // A duplicate id makes the second row dead, silently.
        for (i, block) in BLOCKS.iter().enumerate() {
            assert_eq!(
                definition(block.id).name,
                block.name,
                "row {i} ({}) is shadowed by an earlier one",
                block.name
            );
            assert_eq!(
                BLOCKS.iter().filter(|b| b.id == block.id).count(),
                1,
                "{} shares its id with another row",
                block.name
            );
            assert_ne!(block.name, "?", "'?' is the placeholder's name");
        }
    }

    #[test]
    fn an_unknown_id_answers_inertly_rather_than_panicking() {
        // Ids arrive over a socket, so this is reachable from outside.
        let nonsense = definition(60_000);
        assert_eq!(nonsense.name, "?");
        assert_eq!(nonsense.hardness, None);
        assert!(!nonsense.placeable);
        assert!(!is_defined(60_000));
    }

    #[test]
    fn a_row_is_self_consistent() {
        for block in BLOCKS {
            assert!(block.opacity <= 15, "{}: opacity is 0..15", block.name);
            assert!(block.emission <= 15, "{}: emission is 0..15", block.name);
            assert!(block.weight >= 0.0, "{}: negative weight", block.name);
            assert!(block.drag > 0.0, "{}: drag stops you dead", block.name);
            // A tier on a block nothing can break is a rule that never
            // fires -- it reads as "this needs a bronze pick" and means
            // "this never gives way to anything".
            if block.needs != Tier::Hand {
                assert!(
                    block.hardness.is_some(),
                    "{}: needs a {:?} tool and then cannot be broken by one",
                    block.name,
                    block.needs
                );
            }
            // A tool is a thing you hold, not a thing you dig.
            if block.tool.is_some() {
                assert_eq!(block.shape, Shape::Item, "{}: a tool is an item", block.name);
                assert_eq!(block.hardness, None, "{}: a tool is not mined", block.name);
                assert!(!block.placeable, "{}: a tool is not placed", block.name);
            }
            if block.matter == Matter::Liquid {
                assert!(!block.placeable, "{}: a liquid is not placed", block.name);
                assert_eq!(block.hardness, None, "{}: a liquid is not mined", block.name);
            }
        }
    }

    #[test]
    fn nothing_needs_a_tool_that_cannot_be_made() {
        // The rule that keeps `Tier`'s unused upper rungs honest. Copper,
        // bronze and iron are still in the enum because the metals are
        // still in the game, but nothing is made of them yet -- so a
        // block asking for one of those tiers is a block nobody can ever
        // open, which is the difference between a locked door and a
        // painted one.
        //
        // This is not a check that some tool exists somewhere: it is a
        // check that a tool exists *for that kind of work*. A row saying
        // "rock, bronze tier" would pass a tier-only test and still be
        // unbreakable, because the only bronze thing in the game would be
        // an ingot.
        for block in BLOCKS {
            if block.needs == Tier::Hand {
                continue;
            }
            let opened_by = BLOCKS.iter().find(|tool| {
                tool.tool.is_some_and(|tier| tier >= block.needs)
                    && (block.work == Work::Any || tool.work == block.work)
            });
            assert!(
                opened_by.is_some(),
                "{} needs a {:?} tool for {:?} and no such tool exists",
                block.name,
                block.needs,
                block.work
            );
        }
    }

    #[test]
    fn the_three_tools_are_a_set_rather_than_a_ladder() {
        // Same tier, different work. If one of them ever came out a rung
        // above the others it would be strictly better at everything --
        // see `break_seconds_with`, where a higher tier is faster even on
        // work it is not for -- and the set would quietly become a
        // ladder again.
        let tools: Vec<&BlockDef> = BLOCKS.iter().filter(|b| b.tool.is_some()).collect();
        assert_eq!(tools.len(), 3, "the stone age has three tools");
        for tool in &tools {
            assert_eq!(tool.tool, Some(Tier::Flint), "{} is not flint", tool.name);
            assert_ne!(tool.work, Work::Any, "{} is a tool for nothing", tool.name);
        }
        for kind in [Work::Stone, Work::Wood, Work::Plant] {
            assert_eq!(
                tools.iter().filter(|t| t.work == kind).count(),
                1,
                "{kind:?} is not the work of exactly one tool"
            );
        }
    }
}
