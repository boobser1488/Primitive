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

use crate::types::{block_kind, BlockId, BLOCK_AIR, BLOCK_SANDSTONE, BLOCK_LIMESTONE, BLOCK_GRANITE, BLOCK_PEAT, BLOCK_DRIED_PEAT, BLOCK_SINEW, BLOCK_BONE, BLOCK_CARCASS_HARE, BLOCK_CARCASS_DEER, BLOCK_CARCASS_BOAR, BLOCK_CARCASS_WOLF, BLOCK_CARCASS_SHEEP,
    BLOCK_CORD, BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, BLOCK_FLINT_SPEAR, BLOCK_RUSTY_STONE, BLOCK_IRON_DUST, BLOCK_ROTTEN,
};
#[allow(unused_imports)]
use crate::types::*;

/// Half a cell, in the eighths `BlockDef::thickness` is measured in.
///
/// Named rather than written as `4`, because what it means is "half",
/// and the day the layer count stops being eight is the day a bare 4
/// silently becomes a third of a block.
pub const HALF_BLOCK: u8 = crate::types::LAYERS_PER_BLOCK / 2;

/// A quarter of a cell -- four pixels of the sixteen a block face is
/// drawn at.
///
/// The height of a fire. A campfire was half a block, which is knee-high
/// on a person: a ring of stones you would have to climb rather than
/// step over, and tall enough that a pot standing on it would be at
/// chest height. Four pixels is a fire laid *on the ground*, which is
/// what a campfire is, and it is low enough to walk across without
/// jumping.
pub const QUARTER_BLOCK: u8 = crate::types::LAYERS_PER_BLOCK / 4;

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
/// **The three upper rungs have something standing on them now.** They
/// were empty for two versions: the metals were in the game -- ore,
/// smelting, alloying -- and every tool a player could hold was flint,
/// because a stone age with a metal escape hatch in it is a prologue
/// rather than a setting. What was missing was a reason the stone age
/// had to *end*, and hunger is it: a fire cooks, a fire smelts, and the
/// same evening that makes meat worth eating makes copper worth
/// pouring.
///
/// So each rung is now three tools rather than one pick, and the ladder
/// is climbed sideways as well as upwards -- see `Work`. What is still
/// enforced is that no block asks for a tier no tool has: see
/// `blocks::tests::nothing_needs_a_tool_that_cannot_be_made`.
///
/// `Hand` is a tier so that "no tool at all" is a value in the same
/// ordering rather than a special case in every comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Hand,
    /// A ground stone head lashed to a haft and nothing more. Between
    /// the hand and the wedged tool: it turns earth and cuts brush
    /// faster than fingers do, and it cannot fell a tree or break
    /// stone -- the joint would not take the shock. See `BLOCK_WEDGED_AXE`.
    Stone,
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
    /// what turned soft metal into a tool that holds.
    ///
    /// **Iron is a shade past bronze, and it was half as much again.** Six
    /// was the number that said the iron age was won at the bloomery, and
    /// it was not: wrought iron worked cold is about as hard as worked tin
    /// bronze (both near 200 on the Vickers scale), and what made iron the
    /// better metal was that it could be *steeled* -- carburised in a closed
    /// fire and quenched. That multiplier lives on the tool, not on the
    /// tier (`tools::STEEL_SPEED`), so the gate here still ranks iron above
    /// bronze and the speed says what the metal is. The spear's thrust and
    /// the knife's cut used to be read off this number too, and no longer
    /// are: see `tools`.
    #[inline]
    pub fn speed(self) -> f32 {
        match self {
            Tier::Hand => 1.0,
            Tier::Stone => 1.4,
            Tier::Flint => 2.0,
            Tier::Copper => 2.4,
            Tier::Bronze => 4.0,
            Tier::Iron => 4.4,
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
    /// Loose ground: soil, sand, gravel, snow, ash. A shovel.
    ///
    /// **The odd one out, and deliberately.** Every other variant here
    /// is a gate: a block that wants `Stone` is opened by a pick and by
    /// nothing else. Nothing in the game wants `Ground` -- loose blocks
    /// are `Any`, because a fist has always been able to dig a hole and
    /// taking that away would make a shovel a thing you are obliged to
    /// carry rather than a thing you choose to. This variant is the
    /// *tool's* side of the pair only: it says what a shovel is for, and
    /// `break_seconds_with` reads it to halve the time on loose ground.
    /// See `BLOCK_COPPER_SHOVEL` for the argument in full.
    Ground,
}

/// What the material *is*, for the handful of rules that turn on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matter {
    /// Rock, wood, worked stone: it stays where it is put.
    Solid,
    /// Sand, soil, gravel, ash, snow: what a shovel moves.
    Loose,
    /// Water. The reason `has_depth` exists -- and, since the sea floor
    /// grew things, **what stands in water** as well: a row that is
    /// liquid and not a cube is a plant or a shell with the sea round it
    /// (`types::stands_in_water`, and `types::BLOCK_KELP` for why).
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
    /// How much of its cell a cube fills, in eighths, measured up from
    /// the floor. Eight is a whole block, which is what nearly
    /// everything is.
    ///
    /// **The one thing a `Cube` has that is not cubic.** A campfire is a
    /// ring of stones with wood laid over it and a bag is a bag: drawn
    /// a metre tall they read as a crate with a picture of a fire on it,
    /// and worse, they are a metre-tall *wall* -- you cannot see over
    /// your own hearth or step over the pack you died next to. Half a
    /// block is a thing on the ground you walk over, which is what both
    /// of them are.
    ///
    /// A field rather than a fifth `Shape`, because the shape is not
    /// different: it is six faces filling the width of the cell, meshed,
    /// lit and collided by exactly the code a cube already goes through.
    /// What differs is one number, and the mesher has read that number
    /// since loose material came in layers -- see `types::block_layers`,
    /// which is where the two meet.
    ///
    /// Only eighths, and only up from the floor: a block that floated,
    /// or one two thirds deep, would need the mesher to interpolate
    /// something it currently reads off a table.
    pub thickness: u8,
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
    /// What stands in the cell afterwards, if breaking this does not
    /// empty it.
    ///
    /// `None` for everything: a broken block leaves air, and that is
    /// what breaking means. The one exception is the berry bush, and it
    /// is the reason this is a field rather than a rule somewhere in the
    /// server -- **a bush is picked, not pulled up**. What comes off it
    /// is the berries; what stays is the same bush with nothing on it,
    /// which fills again by itself.
    ///
    /// A row rather than a special case in the break path, for the
    /// reason at the top of this file: three separate parts of the game
    /// decide what a cell holds after an edit (the break itself, the
    /// collapse of whatever stood on it, and the client's prediction),
    /// and a rule only one of them knows is a bush that flickers.
    pub leaves_behind: Option<BlockId>,
    /// What one of them costs to carry.
    pub weight: f32,
    /// How many swings it survives before it breaks, if it is a tool.
    ///
    /// `None` for everything that is not one. The numbers are a ladder
    /// rather than a balance sheet: flint is a stone bound to a stick
    /// with grass and gives out fast, copper is soft, bronze is what
    /// bronze was for, and iron is the reason the age is named after it.
    ///
    /// **A swing, not a block.** Wear is counted per use rather than per
    /// cubic metre, so a pick that opens iron ore in six swings costs
    /// six of its life on that block and one on a lump of gravel. That is
    /// what makes a tool something you spend rather than something you
    /// have.
    ///
    /// **One row counts something else, and says so: the lit torch.** Its
    /// life is tenths of a second and what spends it is the clock -- see
    /// `types::TORCH_LIFE`. The counter is shared rather than duplicated
    /// because everything around it already works: wear survives a save,
    /// splits with a stack and is drawn on the item, and a second notion
    /// of "how much is left" would be two mechanisms obliged to agree.
    pub durability: Option<u32>,
    /// How many of it fit in one slot.
    ///
    /// `inventory::MAX_STACK` for materials, and **one for anything you
    /// hold rather than pile up**. A stack of a hundred and twenty-eight
    /// axes in one slot is not a pile of axes, it is a bug that reads as
    /// a design: it makes the slot the tool lives in weightless to
    /// manage, it makes "which axe am I holding" a question with no
    /// answer, and it is the reason a player can carry the whole
    /// toolshed and never think about it. A tool is a thing; things go
    /// one to a slot.
    pub stack: u32,
    /// How fast you walk over it, as a multiplier. Under one for the
    /// surfaces you push *through* rather than over.
    pub drag: f32,
    /// How well a foot holds on it, as a multiplier on friction and on
    /// the acceleration a player can put down. One for every surface a
    /// boot bites into; a fraction for the ones it does not.
    ///
    /// **Not the same number as `drag`, and the difference is the
    /// point.** Drag is a *top speed*: snow is deep, you push through
    /// it, and the moment you stop pushing you stop. Grip is how quickly
    /// speed can be gained or lost at all -- take it away and the top
    /// speed is unchanged while everything either side of it stretches
    /// out, so a player on ice accelerates slowly, keeps what they have
    /// and has to steer out of a corner they would have walked out of.
    ///
    /// It scales both halves deliberately. Scaling only friction would
    /// give a player on ice full control *and* no braking, which is not
    /// slippery, it is a cheat; scaling both is what makes momentum
    /// something you plan around rather than something you cancel.
    pub grip: f32,
    /// May a player put one down?
    pub placeable: bool,
    /// Tinted by the climate it grew in.
    pub foliage: bool,
    /// Drawn differently depending on which way it lies.
    pub orientable: bool,
    /// Has a *front*: drawn differently depending on which way it looks.
    ///
    /// Not the same question as `orientable`, and no block answers yes
    /// to both -- a log lies along an axis and a kiln looks in a
    /// direction. They share the variant bits for that reason; see
    /// `types::Facing`.
    ///
    /// **Yes exactly when a quarter turn of the block would show**: a side
    /// picture that differs from the others in `blocks.toml` (a kiln's
    /// mouth), or a model that is not the same after the turn (a chair's
    /// back, a jug's handle, a stool's odd leg). Not a list somebody keeps:
    /// the client's
    /// `a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show`
    /// reads the pictures and the mesher's own output and fails, naming
    /// the block, when a row says otherwise.
    ///
    /// **A field and not worked out from the pictures at start-up**, which
    /// was the other way to make it follow the data. The pictures are the
    /// client's; the server has none, and it is the server that decides
    /// whether an id with facing bits is a real block (`is_known_block`).
    /// A texture pack that could change which ids the anti-cheat accepts
    /// would be a texture pack that kicks its own players.
    pub faces: bool,
    /// Falls when nothing is under it.
    ///
    /// **Every soil, not only sand and gravel** -- earth, turf, dry turf,
    /// sandy soil, mud and snow: "сделай землю и любые сыпучие блоки такими
    /// же как песок". A hillside of earth that stood over a dug-out hole like
    /// a stone arch made digging under it free, and a tunnel through soil a
    /// thing that needed no thought. Clay and peat stay put: they are the
    /// sticky ones, and a clay bank you can undercut is what clay is.
    pub falls: bool,
    /// Has an inventory of its own, and a right click opens it.
    ///
    /// Three kinds of block now: the chest, which is forty slots and
    /// nothing else; a dead player's pack, which is the same thing with
    /// a different picture; and the three hearths, whose slots have
    /// *roles* -- see `crate::hearth`. What they share is everything
    /// this flag actually controls: the click opens rather than places,
    /// the server will serve container gestures against the cell, and
    /// breaking the block spills what is inside it into the world.
    pub container: bool,
    /// Its texture is turned a random quarter per cell, so a wall of it
    /// does not read as a grid.
    pub turns: bool,
    /// A cube that still needs something under it. Only the cactus:
    /// everything else that needs propping up is a `Cross` or a `Flat`,
    /// and needs it by being one.
    pub propped: bool,
}

/// What a tool stacks to. One, and the name is here so the rows say
/// *why* rather than repeating a bare number.
const ONE: u32 = 1;

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
    thickness: LAYERS_PER_BLOCK,
    matter: Matter::Solid,
    opacity: 15,
    emission: 0,
    hardness: None,
    felled: None,
    needs: Tier::Hand,
    work: Work::Any,
    tool: None,
    drop: None,
    leaves_behind: None,
    weight: 0.0,
    stack: crate::inventory::MAX_STACK,
    durability: None,
    drag: 1.0,
    grip: 1.0,
    placeable: false,
    foliage: false,
    orientable: false,
    faces: false,
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
        thickness: LAYERS_PER_BLOCK,
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
        leaves_behind: None,
        weight: 1.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    // Dry turf: the grass block's row in straw, untinted -- see
    // `types::BLOCK_DRY_TURF`.
    BlockDef {
        id: BLOCK_DRY_TURF,
        name: "dry_turf",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DIRT),
        leaves_behind: None,
        weight: 1.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DIRT,
        name: "dirt",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DIRT),
        leaves_behind: None,
        weight: 1.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE,
        name: "stone",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // The first row with a floor under it. Stone was simply
        // unbreakable before there was anything to break it with, which
        // meant the whole underground was scenery; now it is the block
        // the first tool is *for*, and three seconds of flint work is what
        // a metre of rock costs (hardness over `Tier::Flint.speed`; this
        // said six, which is what it cost before tiers were a speed, and
        // the guide copied it).
        hardness: Some(6.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_COBBLESTONE),
        leaves_behind: None,
        weight: 2.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SAND,
        name: "sand",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SAND),
        leaves_behind: None,
        weight: 1.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.95,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SNOW,
        name: "snow",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(0.9),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SNOW),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.55,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WATER,
        name: "water",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        // **One, and it was two.** Sunlight pays a level for every step it
        // takes and this on top (`lighting::sky_below`), so at two a block
        // of water cost three and the bed five blocks down was lit by
        // nothing but the ambient floor -- which is exactly where a warm
        // reef grows. Corals came out as black cut-outs and a kelp forest
        // as black walls, however clear the water over them was made. At
        // one the sea still goes dark, at eight blocks rather than five,
        // and the terraces a shelving bed casts are two levels a step
        // rather than three (see `WATER_DEPTH_FADE` in shader.wgsl for why
        // a terrace matters). What reads sunlight under the sea is the
        // picture: the terrain, the animals (`logic::entities`), and a
        // mod's `sky_light` -- no rule of play.
        opacity: 1,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LOG,
        name: "log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
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
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEAVES,
        name: "leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // **One, and two was tried for "сделай леса темнее" and taken out.**
        // Lit both ways in one run, the floor under an oak wood's crowns read
        // 13.21 levels of sky with a leaf of one and 13.21 with a leaf of two:
        // the floor's light does not come down through the crown, it comes in
        // sideways through the open trunks from the gaps a column or two away,
        // and no leaf stands on that path. A wood is darker for more of it
        // being closed, not for its leaves being thicker -- see
        // `the_floor_under_a_wood_is_darker_than_the_open_ground_beside_it`.
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow, not solid.** Leaves stopped being a wall
        // (`types::is_collidable`) and this is the other half of that:
        // pushing through a thicket costs well over half your speed, so
        // going round a wood is still a decision. Snow is 0.6 and deep
        // snow 0.45; a canopy is thicker than either.
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_GLOWSTONE,
        name: "glowstone",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 14,
        hardness: Some(2.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_GLOWSTONE),
        leaves_behind: None,
        weight: 1.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PLANKS,
        name: "planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PLANKS),
        leaves_behind: None,
        weight: 0.7,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Pegged timber, in both woods. Everything is the plain plank's
    // numbers except two things, and both are the mechanic: it is
    // **harder to break**, because a fastened joint is, and it drops
    // the boards without the pegs, because a peg is split out to get
    // it apart and does not survive. What it is *for* is in
    // `falling::Looseness::Built`: a pegged board holds itself and ends
    // the span its neighbours are measured across.
    BlockDef {
        id: BLOCK_PEGGED_PLANKS,
        name: "pegged_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PLANKS),
        leaves_behind: None,
        weight: 0.75,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // Not placeable: pegged boards are something a player *makes*
        // out of a wall they already built, with a peg and a gesture.
        // A pegged plank in the hotbar would be a way to skip the peg.
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEGGED_BIRCH_PLANKS,
        name: "pegged_birch_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BIRCH_PLANKS),
        leaves_behind: None,
        weight: 0.75,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COBBLESTONE,
        name: "cobblestone",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COBBLESTONE),
        leaves_behind: None,
        weight: 2.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TALL_GRASS,
        name: "tall_grass",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Dry grass: the tall grass's row in straw, and untinted -- see
    // `types::BLOCK_DRY_GRASS` for why it is not foliage.
    BlockDef {
        id: BLOCK_DRY_GRASS,
        name: "dry_grass",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CACTUS,
        name: "cactus",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_CACTUS),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_STICK,
        name: "stick",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STICK),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIBER,
        name: "fiber",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.02,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A fistful torn out of a crown. See `types::BLOCK_LEAF_HANDFUL`.
    BlockDef {
        id: BLOCK_LEAF_HANDFUL,
        name: "leaf_handful",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.03,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEBBLE,
        name: "pebble",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PEBBLE),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT,
        name: "flint",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CHEST,
        name: "chest",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // **Not opaque any more, because it is not a cube any more.** The
        // chest is drawn as a model a sixteenth in from its cell's sides
        // (`models::Prop::Chest`), and an opaque row told the mesher it
        // covered its neighbours: the grass under it and the wall behind it
        // were left undrawn, and the sky showed round its feet. Furniture
        // lets light by, as the table's row says.
        opacity: 0,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CHEST),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ASH,
        name: "ash",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ASH),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLAY,
        name: "clay",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(2.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLAY),
        leaves_behind: None,
        weight: 1.7,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_GRAVEL,
        name: "gravel",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.7),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_GRAVEL),
        leaves_behind: None,
        weight: 1.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_LOG,
        name: "birch_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
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
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- experimental trees ----
    //
    // The pieces a branching tree is built of, thin and thick. See
    // `types::BLOCK_TWIG` for why they are two rows, and
    // `mesh::branch_block` for how they are drawn.
    BlockDef {
        id: BLOCK_TWIG,
        name: "twig",
        // A cube to every rule that asks, drawn as a model: see the mesher.
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // **Lets the light through.** A post a few sixteenths wide in a
        // cell that is otherwise air; read as opaque it would put a
        // shadow one metre square under every twig, and -- worse -- the
        // mesher would cull the faces of the leaves beside it and leave
        // holes in the crown.
        opacity: 0,
        emission: 0,
        // **Hands take a twig.** A sapling and the tips of a crown are the
        // wood a player without an axe can reach, and that is the reason
        // small trees exist: sticks in the first minutes, from a thing
        // that grows in the wood rather than from pulling a canopy apart.
        //
        // **Half a second**, under the leaves' 0.6: a stem a few sixteenths
        // thick snaps. It was a whole second, longer than stripping the
        // leaves round it, for the one piece of a tree a bare hand is
        // meant to be good at ("ломать тонкие ветки руками").
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STICK),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **No drag.** A fifth of a player's speed was the price of pushing
        // through a stem that stopped nobody. A twig is walked into at its
        // wood now (`branch`), and a slowdown in the air beside a stem that
        // already stops you where it stands is a thicket nobody can see.
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BOUGH,
        name: "bough",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Not opaque, even at sixteen sixteenths, for the twig's second
        // reason: an opaque bough would hide the faces of the leaves and
        // the ground it stands against, and its model does not fill the
        // cell those faces were hidden behind.
        opacity: 0,
        emission: 0,
        // A standing trunk's numbers, because it is one: see `BLOCK_LOG`.
        // An axe, nine seconds, and a log for it.
        hardness: Some(9.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_LOG),
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- drowned wood ----
    //
    // A swamp snag's pieces under its pool's surface: the twig's and the
    // bough's rows, liquid, with the water's opacity so the foot of a snag is
    // lit as the pool beside it is, and weightless as every liquid row is --
    // neither is ever carried, only the stick and the log they give. See
    // `types::BLOCK_DROWNED_BOUGH` for why they are water at all.
    BlockDef {
        id: BLOCK_DROWNED_TWIG,
        name: "drowned_twig",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STICK),
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.8,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DROWNED_BOUGH,
        name: "drowned_bough",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(9.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_LOG),
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the palm ----
    //
    // A piece of trunk, the crown, the crown in fruit, and the fruit. The
    // trunk is the bough's row with the palm's name on it: timber in every
    // rule, an axe to cut and a log for it (see `types::BLOCK_PALM_TRUNK`
    // for why it is a piece of branch at all). The crown is the apple
    // canopy's row, except what it gives.
    BlockDef {
        id: BLOCK_PALM_TRUNK,
        name: "palm_trunk",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Not opaque, for the bough's reason: the post does not fill the
        // cell the faces round it would be hidden behind.
        opacity: 0,
        emission: 0,
        hardness: Some(9.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_LOG),
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PALM_FRONDS,
        name: "palm_fronds",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // **Fibre, not the frond.** A leaf a player carries home and puts
        // down is a leaf; a palm frond torn apart is what a coast with no
        // meadow behind it makes its cord from, and a stack of placeable
        // fronds would be a block nobody has a use for.
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // Slow, not solid, like every canopy: see the apple leaf below.
        drag: 0.35,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PALM_COCONUTS,
        name: "palm_coconuts",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        // Picked, not torn out: the apple's number.
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_COCONUT),
        // The picked frond, which is the one cell that fruits again. See
        // `types::BLOCK_PALM_FRONDS_PICKED`.
        leaves_behind: Some(BLOCK_PALM_FRONDS_PICKED),
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COCONUT,
        name: "coconut",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COCONUT),
        leaves_behind: None,
        // **Heavy, and that is the other half of the decision.** A nut
        // with its husk on is most of a kilogram: ten of them is ten meals
        // of water on a dry coast, and a pack that feels like it. A jug
        // carries twice the water and is carried back; a coconut is drunk
        // once and weighs what a jug does.
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the swamp ----
    BlockDef {
        id: BLOCK_MUD,
        name: "mud",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        // Softer than dirt: it is dirt with the water still in it.
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // **Clay, not mud.** What a swamp gives a potter is the clay its
        // still water settled out, and a block of mud in the pack would be
        // a second dirt nobody builds with. This is the reason to wade in.
        drop: Some(BLOCK_CLAY),
        leaves_behind: None,
        weight: 1.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow underfoot, and the swamp is made of it.** Between deep
        // snow (0.45) and a dusting (0.6): a boot sinks and pulls free, so a
        // swamp is crossed at half a walk and a straight line across one is
        // a choice against going round.
        drag: 0.5,
        grip: 0.85,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LILY_PAD,
        name: "lily_pad",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_HANGING_MOSS,
        name: "hanging_moss",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The apple tree's canopy, in two states. Everything here is the
    // oak leaf's numbers -- it is the same kind of thing on the same
    // kind of tree -- except what the fruiting one drops and what it
    // leaves behind, which is the whole difference between a tree you
    // strip and a tree you come back to.
    BlockDef {
        id: BLOCK_APPLE_LEAVES,
        name: "apple_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow, not solid.** Leaves stopped being a wall
        // (`types::is_collidable`) and this is the other half of that:
        // pushing through a thicket costs well over half your speed, so
        // going round a wood is still a decision. Snow is 0.6 and deep
        // snow 0.45; a canopy is thicker than either.
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_APPLE_LEAVES_FRUIT,
        name: "apple_leaves_fruit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        // Softer than the leaves it hangs in: what the player is doing
        // is picking fruit, not tearing out a branch, and the hand that
        // does it should be done before the hand that clears a canopy.
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_APPLE),
        // Picked, not stripped -- the berry bush's rule, and the reason
        // an orchard is a place worth walking back to. The picked leaf
        // and not the bare one, because only the picked one fruits again:
        // see `types::BLOCK_APPLE_LEAVES_PICKED`.
        leaves_behind: Some(BLOCK_APPLE_LEAVES_PICKED),
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow, not solid.** Leaves stopped being a wall
        // (`types::is_collidable`) and this is the other half of that:
        // pushing through a thicket costs well over half your speed, so
        // going round a wood is still a decision. Snow is 0.6 and deep
        // snow 0.45; a canopy is thicker than either.
        drag: 0.35,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- savanna ----
    //
    // The acacia's canopy: the birch leaf's row below in everything but
    // its tint. See `types::BLOCK_ACACIA_LEAVES` for why it is not
    // foliage.
    BlockDef {
        id: BLOCK_ACACIA_LEAVES,
        name: "acacia_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // Slow, not solid, as every canopy is -- see the birch leaf.
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        // **Untinted.** The climate tint's hot, dry corner is straw, and
        // an acacia the colour of the grass under it is no landmark.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The maple's canopy: the acacia's row above in everything but its
    // colour. Untinted for the reason `types::BLOCK_MAPLE_LEAVES` gives.
    BlockDef {
        id: BLOCK_MAPLE_LEAVES,
        name: "maple_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Sandy soil: dirt's row with sand in it -- a little quicker to dig
    // and a little lighter, because it is looser. See
    // `types::BLOCK_SANDY_SOIL` for what does and does not grow on it.
    BlockDef {
        id: BLOCK_SANDY_SOIL,
        name: "sandy_soil",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 15,
        emission: 0,
        hardness: Some(1.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SANDY_SOIL),
        leaves_behind: None,
        weight: 1.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: true,
        container: false,
        turns: false,
        propped: false,
    },
    // A termite mound: baked earth that breaks into the clay it was made
    // of. Harder than clay dug wet from a bank, as a baked thing is, and
    // not placeable -- see `types::BLOCK_TERMITE_MOUND`.
    BlockDef {
        id: BLOCK_TERMITE_MOUND,
        name: "termite_mound",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLAY),
        leaves_behind: None,
        weight: 1.7,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_LEAVES,
        name: "birch_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow, not solid.** Leaves stopped being a wall
        // (`types::is_collidable`) and this is the other half of that:
        // pushing through a thicket costs well over half your speed, so
        // going round a wood is still a decision. Snow is 0.6 and deep
        // snow 0.45; a canopy is thicker than either.
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BIRCH_PLANKS,
        name: "birch_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BIRCH_PLANKS),
        leaves_behind: None,
        weight: 0.7,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
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
        thickness: HALF_BLOCK,
        matter: Matter::Solid,
        // Half a cell, so it cannot black one out -- see the campfire
        // for the whole argument and the symptom. This was 15 and put a
        // dark square on the ground under every pack left standing.
        opacity: 1,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
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
        thickness: LAYERS_PER_BLOCK,
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
        leaves_behind: None,
        weight: 2.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // ...and therefore not placeable, for the same reason turf is
        // not: what you get from breaking it is not this.
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
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
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(8.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_COPPER_ORE),
        leaves_behind: None,
        weight: 2.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TIN_ORE,
        name: "tin_ore",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(8.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_TIN_ORE),
        leaves_behind: None,
        weight: 2.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_ORE,
        name: "iron_ore",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Nearly twice the copper ore, and the whole of iron's
        // difficulty is that one number rather than a tier.
        //
        // **Copper opens it, and flint no longer does.** This said
        // `Tier::Flint` for as long as there was no metal pick to say
        // anything else with: a tier lock would have been a block that
        // exists in the world, is drawn in it, and can never be taken
        // out of it, which is what "scenery" means. There are metal
        // picks again, so the lock is a lock rather than a wall, and it
        // is the one place in the game where a tier is a gate instead of
        // a speed.
        //
        // The thirteen stays, and it is what the gate is worth: even
        // with copper in hand -- barely faster than flint, because a
        // copper edge rolls over the first time it meets rock -- a metre
        // of iron ore is five and a half seconds. Bronze is what makes
        // iron a material rather than an expedition, which is the whole
        // argument for alloying it.
        hardness: Some(13.0),
        felled: None,
        needs: Tier::Copper,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_IRON_ORE),
        leaves_behind: None,
        weight: 3.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COAL,
        name: "coal",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COAL),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_INGOT,
        name: "copper_ingot",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COPPER_INGOT),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_TIN_INGOT,
        name: "tin_ingot",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_TIN_INGOT),
        leaves_behind: None,
        weight: 0.45,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_INGOT,
        name: "bronze_ingot",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_INGOT),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_INGOT,
        name: "iron_ingot",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_INGOT),
        leaves_behind: None,
        weight: 0.55,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
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
    //
    // **All but the flake, which lies on the ground the way the nodule it
    // came off does.** It was an item, and the generator lays flakes on
    // the floor of every rock shelter and round every knapping floor
    // (`worldgen::features`) -- and an item has no shape in the world.
    // The mesher drew it as the cube it falls through to, wearing the
    // icon with its transparent corners opaque: a pure black block in the
    // hearth of a shelter, which a player took for a ruin. Nor could it be
    // picked up, because a ray does not stop at an item (`is_targetable`).
    // Flat, like `BLOCK_FLINT`, so the same picture is the handful in the
    // pack and the shard on the floor; placeable, because a thing a player
    // can pick up off the ground and never put back is the drop rule's
    // dead end. See
    // `nothing_the_generator_lays_in_the_world_is_a_thing_that_exists_only_in_a_pack`.
    BlockDef {
        id: BLOCK_FLINT_FLAKE,
        name: "flint_flake",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // A flint's, which is a pebble's: picked up, not dug.
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Lighter than the nodule it came off, and there are several of
        // them: a flake is a shard, not a stone.
        drop: Some(BLOCK_FLINT_FLAKE),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        // Turned by a hash, like the nodule: a floor of flakes all
        // pointing one way is a floor of tiles.
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WORKED_STICK,
        name: "worked_stick",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WORKED_STICK),
        leaves_behind: None,
        // Heavier than the branch it was: a haft is trimmed to a shape,
        // and what is left is the dense part of the wood.
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_KNIFE_HEAD,
        name: "flint_knife_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FLINT_KNIFE_HEAD),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE_AXE_HEAD,
        name: "stone_axe_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STONE_AXE_HEAD),
        leaves_behind: None,
        weight: 0.45,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE_PICK_HEAD,
        name: "stone_pick_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STONE_PICK_HEAD),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
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
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: Some(Tier::Flint),
        drop: None,
        leaves_behind: None,
        // The lightest of the three, and by a lot: a knife is an edge
        // with something to hold it by.
        weight: 0.3,
        stack: ONE,
        durability: Some(72),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE_AXE,
        name: "stone_axe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        // Lashed, not wedged: brush and earth, never a tree or stone.
        // The wedged tool is the old tier -- see `types::BLOCK_WEDGED_AXE`.
        tool: Some(Tier::Stone),
        drop: None,
        leaves_behind: None,
        weight: 0.9,
        stack: ONE,
        durability: Some(90),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STONE_PICKAXE,
        name: "stone_pickaxe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        // Lashed, not wedged: brush and earth, never a tree or stone.
        // The wedged tool is the old tier -- see `types::BLOCK_WEDGED_AXE`.
        tool: Some(Tier::Stone),
        drop: None,
        leaves_behind: None,
        weight: 0.8,
        stack: ONE,
        durability: Some(90),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what grows, and what it feeds you ----
    //
    // Four plants and four things you carry. They are here because
    // hunger is: a world where the only food walks away from you is a
    // world whose first evening is a hunt or a death, and neither is
    // what the first evening should be about.
    //
    // All four plants are `Work::Plant` -- a knife is what cuts growing
    // things -- and all four are `Tier::Hand`, because a stone age that
    // gates *food* behind a tool is a stone age that starves the player
    // before it teaches them anything.
    BlockDef {
        id: BLOCK_BERRY_BUSH,
        name: "berry_bush",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BERRIES),
        // Picked, not pulled up. See `BlockDef::leaves_behind`.
        leaves_behind: Some(BLOCK_BARE_BUSH),
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // Not placeable: what a player carries is the bush they dug up,
        // and a bush comes up bare.
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BARE_BUSH,
        name: "bare_bush",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.35),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BARE_BUSH),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_MUSHROOM,
        name: "mushroom",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_MUSHROOM),
        leaves_behind: None,
        weight: 0.06,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        // Not tinted by the climate: a fungus is not a leaf, and the
        // one that grows in a swamp is the same colour as the one in a
        // cave -- which is where most of them are, and a cave has no
        // climate to be tinted by.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what there is to forage ----
    BlockDef {
        id: BLOCK_TOADSTOOL,
        name: "toadstool",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Exactly the mushroom's, because the difference between the two
        // must be what is drawn on the cap and nothing else. A toadstool
        // that took longer to pick would be a toadstool a player could
        // tell apart with their eyes shut.
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_TOADSTOOL),
        leaves_behind: None,
        weight: 0.06,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ROOTS,
        name: "roots",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // A tuft of leaves comes up in a moment. What takes the time is
        // that you have to notice it.
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // **The plant drops the root, not itself.** Pulling it up is
        // what the block is for: the leaves are a sign that something is
        // under them, and a player who ends up holding the leaves has
        // been given the sign instead of the meal.
        drop: Some(BLOCK_ROOT),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // Not placeable, and it is the only plant that is not: what a
        // player carries away is the root, and there is nothing left to
        // put back. Replanting one would be farming, and the field is
        // where farming lives.
        placeable: false,
        // Tinted with the meadow it grows in, like the grass around it:
        // leaves are leaves, and a rosette that stayed spring green in a
        // dry steppe is the one plant in the picture that does not
        // belong to it.
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ROOT,
        name: "root",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ROOT),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ROASTED_ROOT,
        name: "roasted_root",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ROASTED_ROOT),
        leaves_behind: None,
        // Lighter than it went in: a root over coals loses its water,
        // which is the same reason the picture is narrower.
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_REEDS,
        name: "reeds",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_REEDS),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLOWER,
        name: "flower",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FLOWER),
        leaves_behind: None,
        weight: 0.03,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        // A flower is its own colour. Tinting it by the climate would
        // turn a red one straw-coloured in a savanna, which is the one
        // thing the tint is not for.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_APPLE,
        name: "apple",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_APPLE),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A peg is not placed and not eaten: it is *driven*, with the
    // gesture a knife uses on a log (see the server's `use_block`), into
    // something already standing. That is why it is an item with no
    // hardness of its own -- there is never a peg in the world to break.
    BlockDef {
        id: BLOCK_PEG,
        name: "peg",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PEG),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Nails go in the way a peg does -- into a thing being made -- so they
    // are an item on the peg's terms. A handful of iron: heavier than a
    // peg, lighter than the bar they were cut from.
    BlockDef {
        id: BLOCK_NAILS,
        name: "nails",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_NAILS),
        leaves_behind: None,
        weight: 0.08,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A frame is carried to wherever the furniture is finished and never
    // stood up by itself: a rectangle of rails in a cell would be a fence
    // nobody asked for. Four rails' weight.
    BlockDef {
        id: BLOCK_FRAME,
        name: "frame",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FRAME),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BERRIES,
        name: "berries",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BERRIES),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_RAW_MEAT,
        name: "raw_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RAW_MEAT),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COOKED_MEAT,
        name: "cooked_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COOKED_MEAT),
        leaves_behind: None,
        // Lighter than it went on the fire: what cooking takes out of
        // meat is water.
        weight: 0.35,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRIED_MEAT,
        name: "dried_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_MEAT),
        leaves_behind: None,
        // Lighter still than the cooked cut: a rack takes out even the
        // water a fire leaves in.
        weight: 0.25,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_HIDE,
        name: "hide",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_HIDE),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- fire ----
    //
    // Two rows for one object, because whether it is burning is the
    // whole of what matters about it. See `types::BLOCK_CAMPFIRE`.
    //
    // Soft: a fire is sticks and stones laid in a ring, and taking one
    // apart is a job for hands and not for a pick. Breaking either
    // gives back the unlit fire -- there is no way to carry a burning
    // one, and picking a fire up puts it out.
    BlockDef {
        id: BLOCK_CAMPFIRE,
        name: "campfire",
        shape: Shape::Cube,
        thickness: QUARTER_BLOCK,
        matter: Matter::Solid,
        // **A quarter of a cell cannot swallow a whole cell's light.**
        // This was 15, the value a solid block of stone uses, and the
        // result was a black square on the ground under every campfire
        // and black faces on the blocks beside it: light propagates as
        // `level - (1 + opacity)`, so fifteen extinguishes it outright
        // however little of the cell is actually filled. The lit
        // version made the contradiction plain -- it *emits* thirteen
        // and blocked all of it, a lamp under a blackout curtain.
        //
        // One, because a fire pit is mostly air. The drying rack, which
        // fills its cell completely, costs two; nothing that fills a
        // quarter of one should cost more than that. See
        // `a_block_that_does_not_fill_its_cell_does_not_black_it_out`.
        opacity: 1,
        emission: 0,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CAMPFIRE),
        leaves_behind: None,
        weight: 1.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CAMPFIRE_LIT,
        name: "campfire_lit",
        shape: Shape::Cube,
        thickness: QUARTER_BLOCK,
        matter: Matter::Solid,
        // Not glowstone-bright, and deliberately: a fire lights the
        // hollow you are sitting in rather than the valley. Thirteen
        // reaches about as far as a cave passage is wide, which is the
        // distance a fire is actually worth carrying wood for.
        // **A quarter of a cell cannot swallow a whole cell's light.**
        // This was 15, the value a solid block of stone uses, and the
        // result was a black square on the ground under every campfire
        // and black faces on the blocks beside it: light propagates as
        // `level - (1 + opacity)`, so fifteen extinguishes it outright
        // however little of the cell is actually filled. The lit
        // version made the contradiction plain -- it *emits* thirteen
        // and blocked all of it, a lamp under a blackout curtain.
        //
        // One, because a fire pit is mostly air. The drying rack, which
        // fills its cell completely, costs two; nothing that fills a
        // quarter of one should cost more than that. See
        // `a_block_that_does_not_fill_its_cell_does_not_black_it_out`.
        opacity: 1,
        emission: 13,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CAMPFIRE),
        leaves_behind: None,
        weight: 1.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ---- the metal tools ----
    //
    // Nine rows that are three rows three times over, and the repetition
    // is the point: a copper axe and a bronze axe differ in exactly one
    // field. What a tier buys is `Tier::speed`, and nothing else --
    // there is no metal that fells a tree a stone axe cannot, only metal
    // that fells it faster. The one place a tier is a *gate* rather than
    // a speed is iron ore, which asks for copper (see `BLOCK_IRON_ORE`).
    //
    // Same shape as the flint three: `tool` set, no hardness, no drop,
    // not placeable. See the note above them for why a tool is never a
    // cell in the world.
    BlockDef {
        id: BLOCK_COPPER_KNIFE,
        name: "copper_knife",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: Some(Tier::Copper),
        drop: None,
        leaves_behind: None,
        weight: 0.45,
        stack: ONE,
        durability: Some(144),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_AXE,
        name: "copper_axe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: Some(Tier::Copper),
        drop: None,
        leaves_behind: None,
        weight: 1.2,
        stack: ONE,
        durability: Some(180),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_PICKAXE,
        name: "copper_pickaxe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: Some(Tier::Copper),
        drop: None,
        leaves_behind: None,
        weight: 1.1,
        stack: ONE,
        durability: Some(180),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_KNIFE,
        name: "bronze_knife",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: Some(Tier::Bronze),
        drop: None,
        leaves_behind: None,
        weight: 0.45,
        stack: ONE,
        durability: Some(256),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_AXE,
        name: "bronze_axe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: Some(Tier::Bronze),
        drop: None,
        leaves_behind: None,
        weight: 1.2,
        stack: ONE,
        durability: Some(320),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_PICKAXE,
        name: "bronze_pickaxe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: Some(Tier::Bronze),
        drop: None,
        leaves_behind: None,
        weight: 1.1,
        stack: ONE,
        durability: Some(320),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_KNIFE,
        name: "iron_knife",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: Some(Tier::Iron),
        drop: None,
        leaves_behind: None,
        weight: 0.5,
        stack: ONE,
        durability: Some(440),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_AXE,
        name: "iron_axe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: Some(Tier::Iron),
        drop: None,
        leaves_behind: None,
        weight: 1.3,
        stack: ONE,
        durability: Some(550),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_PICKAXE,
        name: "iron_pickaxe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: Some(Tier::Iron),
        drop: None,
        leaves_behind: None,
        weight: 1.2,
        stack: ONE,
        durability: Some(550),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the kiln, and what comes out of it ----
    //
    // Two rows for one object again, for the reason the campfire has
    // two. What is different about this fire is what it is *for*: a
    // campfire cooks, and a kiln is the only thing in the world hot
    // enough to take metal out of rock. See `types::BLOCK_KILN`.
    //
    // A whole cell tall where a campfire is half of one -- a kiln is a
    // chimney of daub you feed from the front, not a ring of stones you
    // sit around -- and heavier than anything else a player carries,
    // because it is a hundredweight of wet clay.
    //
    // Harder than a campfire and still hands' work: baked daub comes
    // apart with a bar and some swearing, and needing a pick to dismantle
    // the thing that lets you *make* picks would be a rung with the
    // ladder pulled up after it.
    BlockDef {
        id: BLOCK_KILN,
        name: "kiln",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(2.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_KILN),
        leaves_behind: None,
        weight: 12.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_KILN_LIT,
        name: "kiln_lit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        // Two below a campfire's. The fire is *inside* a kiln -- what
        // gets out is what the stoke hole lets past -- and a furnace
        // that lit a clearing better than an open fire would be a
        // furnace with no walls.
        emission: 11,
        hardness: Some(2.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        // Taking one apart puts it out, the same way it does a fire.
        drop: Some(BLOCK_KILN),
        leaves_behind: None,
        weight: 12.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRICK,
        name: "brick",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 2.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRICKS,
        name: "bricks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Between cobble and dressed stone: fired clay is harder than
        // the rubble it replaces and softer than the rock either came
        // out of. It gives itself back, which is what makes it worth
        // building with -- see the note on dressed stone in
        // `PLACEABLE_BLOCKS` for what happens to a block that does not.
        hardness: Some(3.2),
        felled: None,
        // Hands, like the cobble it is laid instead of. A block a player
        // can place and never take back is the one mistake the palette
        // does not allow -- see the note at the top of
        // `types::PLACEABLE_BLOCKS` -- and brickwork you had to own a
        // pick to undo would be exactly that for anyone who built with
        // it before mining.
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_BRICKS),
        leaves_behind: None,
        weight: 9.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Human flesh, raw and roasted. See `types::BLOCK_HUMAN_FLESH`.
    BlockDef {
        id: BLOCK_HUMAN_FLESH,
        name: "human_flesh",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_HUMAN_FLESH),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ROAST_HUMAN_FLESH,
        name: "roast_human_flesh",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ROAST_HUMAN_FLESH),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Sandstone bricks: see `types::BLOCK_SANDSTONE_BRICKS`. Brickwork's
    // terms throughout -- hands to undo, stone work, it gives itself back --
    // a shade harder, because it is rock rather than fired clay.
    BlockDef {
        id: BLOCK_SANDSTONE_BRICKS,
        name: "sandstone_bricks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_SANDSTONE_BRICKS),
        leaves_behind: None,
        weight: 4.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the field ----
    //
    // One implement, three stages and three things you carry. See
    // `types::BLOCK_SEEDS` for why the seed and the first stage are the
    // same block.
    BlockDef {
        id: BLOCK_HOE,
        name: "hoe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.9,
        stack: ONE,
        durability: Some(120),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SEEDS,
        name: "seeds",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // A crop is a plant: light goes through it and nothing stands on
        // it.
        opacity: 0,
        emission: 0,
        // Pulled up by hand in an instant, like every other plant. A
        // field that needed a tool to harvest would be a field you could
        // not harvest the day you planted it.
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_SEEDS),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_FARMLAND,
        name: "farmland",
        shape: Shape::Cube,
        // **A whole block, and it was very nearly seven eighths.**
        // Turned earth sitting below the turf around it would read
        // beautifully from across a valley -- and a plant needs a *full*
        // floor to stand on (see `can_grow_on`), so a field an eighth
        // low is a field where the crop floats an eighth of a block
        // above the soil, or, with the rule enforced, where nothing can
        // be planted at all. That is what happened. The furrows in the
        // texture are what says "tilled" instead, and they say it from
        // just as far away.
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Dug up, it is soil again. Tilling is work, not a material.
        drop: Some(BLOCK_DIRT),
        leaves_behind: None,
        weight: 8.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WHEAT,
        name: "wheat",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // A crop is a plant: light goes through it and nothing stands on
        // it.
        opacity: 0,
        emission: 0,
        // Pulled up by hand in an instant, like every other plant. A
        // field that needed a tool to harvest would be a field you could
        // not harvest the day you planted it.
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_SEEDS),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_WHEAT_RIPE,
        name: "wheat_ripe",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // A crop is a plant: light goes through it and nothing stands on
        // it.
        opacity: 0,
        emission: 0,
        // Pulled up by hand in an instant, like every other plant. A
        // field that needed a tool to harvest would be a field you could
        // not harvest the day you planted it.
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_GRAIN),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_GRAIN,
        name: "grain",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DOUGH,
        name: "dough",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BREAD,
        name: "bread",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the copper age ----
    //
    // See `types::BLOCK_NATIVE_COPPER` for why metal starts on the
    // ground and goes through a pot rather than starting in a recipe.
    BlockDef {
        id: BLOCK_NATIVE_COPPER,
        name: "native_copper",
        // Flat, like a flint nodule and a loose stone: a thing lying in
        // a cell rather than a cell made of something.
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Picked up, not mined. That is the whole argument for surface
        // copper: the first metal a player ever holds needs no tool,
        // because the first metal anybody ever held needed none.
        hardness: Some(0.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_NATIVE_COPPER),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_VESSEL_RAW,
        name: "vessel_raw",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 2.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_VESSEL,
        name: "vessel",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_MOULD_RAW,
        name: "mould_raw",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_MOULD,
        name: "mould",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_BLOOM,
        name: "iron_bloom",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 3.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BLOOMERY,
        name: "bloomery",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Harder than the kiln: a bloomery is a stone shaft with a clay
        // lining, not a pot of daub. Still hands' work to take apart --
        // needing a pick to dismantle the thing that makes picks is a
        // ladder with the bottom rung sawn off.
        hardness: Some(3.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_BLOOMERY),
        leaves_behind: None,
        weight: 16.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BLOOMERY_LIT,
        name: "bloomery_lit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        // Brighter than the kiln and dimmer than an open fire: a
        // bloomery is a chimney, and what you see of it is the glow out
        // of the top and the tuyere.
        emission: 12,
        hardness: Some(3.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_BLOOMERY),
        leaves_behind: None,
        weight: 16.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ---- ice ----
    //
    // The only row in this table with a `grip` that is not one, and the
    // reason that column exists. See `types::BLOCK_ICE`.
    BlockDef {
        id: BLOCK_ICE,
        name: "ice",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // **Light passes through it; sight does not.** Two levels taken
        // out, the same as water, so the sea under a frozen bay is dim
        // rather than pitch black -- which matters because the cell
        // under the lid is water, the cell under that is sea floor, and
        // a lid that stopped light would turn every shallow northern bay
        // into an unlit cave with a roof on it.
        //
        // Being under fifteen also means it is drawn in the cutout pass
        // rather than the solid one (see `types::is_cutout`), which is
        // the pass a leaf uses. That costs the faces against it not
        // being culled and buys the lighting above; what it does *not*
        // buy is seeing the bottom of the lake, because the picture is
        // opaque in every texel and the cutout pass does not blend. Real
        // transparency is the water pass, and putting ice in it would
        // mean calling ice a liquid, which is the one thing it is not.
        opacity: 2,
        emission: 0,
        // Softer than the stone it sits among and harder than the snow
        // beside it. A pick is what gets through it in any useful time,
        // but hands will do -- being shut out of a rink you are standing
        // on would be a strange thing for the world to insist on.
        hardness: Some(2.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        // It gives itself back rather than melting into water, which is
        // what makes a rink something a player can build. Melting would
        // be the truthful answer and would also mean every block of it
        // ever mined is gone, and with it any reason to mine one.
        drop: Some(BLOCK_ICE),
        leaves_behind: None,
        weight: 2.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // Not slower to cross -- there is nothing to push through. What
        // it costs is the grip below.
        drag: 1.0,
        grip: 0.12,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- hides, and what a hide becomes ----
    // Lighter than the hide it came from, because most of what a
    // skin weighs is water and the rack is where that goes.
    BlockDef {
        id: BLOCK_LEATHER,
        name: "leather",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEATHER),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- wool ----
    //
    // A fleece, and the one soft thing in the palette.
    BlockDef {
        id: BLOCK_WOOL,
        name: "wool",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Not opaque, and not transparent either. A bale of wool is a
        // dense thing that light does not go through, so fifteen -- the
        // half-measures in this column are for leaves and water, and a
        // block a player builds a wall out of that let daylight leak
        // into a room would be a bug reported as one.
        opacity: 15,
        emission: 0,
        // Instant, and by hand. Pulling wool apart is not work; the
        // number is small rather than zero so that the break animation
        // has a frame to play, because a block that vanishes with no
        // sound and no crack reads as a glitch rather than as an
        // action.
        hardness: Some(0.15),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOOL),
        leaves_behind: None,
        // Light enough that a full stack is not a load. What a fleece
        // weighs is mostly air, and the carrying rules should say so --
        // see `load::speed_scale`.
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // A little slower than a floor and no less sure-footed. Both
        // of these are multipliers with one as their ceiling -- a
        // surface can hold a boot as well as bare rock does and no
        // better -- so "soft" can only be said in the drag: you sink
        // into a bale slightly, and you never slip on one.
        drag: 0.95,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what wool is worn as ----
    //
    // Four items, one per slot, and none of them placeable: a tunic is
    // something you put on, not something you put down. What they are
    // worth is in `equipment::garment`; this is the row that says they
    // exist and what they weigh.
    BlockDef {
        id: BLOCK_WOOL_CAP,
        name: "wool_cap",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOOL_CAP),
        leaves_behind: None,
        weight: 0.10,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WOOL_TUNIC,
        name: "wool_tunic",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOOL_TUNIC),
        leaves_behind: None,
        weight: 0.30,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WOOL_LEGGINGS,
        name: "wool_leggings",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOOL_LEGGINGS),
        leaves_behind: None,
        weight: 0.25,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WOOL_BOOTS,
        name: "wool_boots",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOOL_BOOTS),
        leaves_behind: None,
        weight: 0.15,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRYING_RACK,
        name: "drying_rack",
        shape: Shape::Cube,
        // **A whole cell tall, and it is a frame rather than a slab.**
        // It used to be half a block on the argument that a rack is
        // something you step over -- which is what a rack *is not*: the
        // thing a tanner builds is a square of poles standing on end
        // with the skin stretched inside it, and you walk round one.
        // The model is drawn by `mesh::rack_block`; the number here does
        // two other jobs, and both of them want the full cell:
        //
        // * **Collision, upward.** A frame is something to walk round
        //   rather than step over, so it is a metre of obstacle from the
        //   floor of its cell. **Only upward, and that took a second
        //   report to get right**: this number was also read as the
        //   footprint, so the frame was a metre of stone *across* as
        //   well, and a tanner could not walk past a rack they could see
        //   straight through. How deep it stands is a separate answer
        //   in `types::collision_depth`, which is read off the model.
        // * **Cover.** `is_partial` is what tells the mesher how much of
        //   a neighbouring face this hides, and a half-height figure
        //   would have gone on hiding the bottom half of whatever the
        //   rack stands against -- correct for the slab it was, a hole
        //   in the wall behind the frame it is now.
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Sticks with gaps between them. Light comes through a rack the
        // way it comes through leaves, which is what stops a row of them
        // casting a wall of shadow across a camp.
        opacity: 2,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_DRYING_RACK),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It has a front**, which a square frame with a skin in it
        // obviously does: it is placed facing whoever put it down, so a
        // row of racks along a camp faces the camp rather than standing
        // edge-on to it.
        faces: true,
        falls: false,
        // **A container, and only two slots of one.** It was not, and
        // the argument for that was that forty slots and a drag-and-drop
        // screen would make a rack a chest that happens to cure things.
        // That much is still true and it is why `rack::USED_SLOTS` is
        // two -- a skin in, leather out. What was wrong was the
        // conclusion: without an inside, the one question a rack exists
        // to answer ("is it dry yet?") could only be asked by taking the
        // skin off, and nothing on screen could say that the rain had
        // stopped it. See `primitive_shared::rack`.
        container: true,
        turns: false,
        propped: false,
    },
    // **The hide frame**: the one-cell rack, back, and every number but one
    // is the old rack's, because it began as the old rack
    // (`types::BLOCK_HIDE_FRAME`): light through it, a front, a container of
    // two slots -- a skin on the pegs, leather off them.
    BlockDef {
        id: BLOCK_HIDE_FRAME,
        name: "hide_frame",
        shape: Shape::Cube,
        // **A whole cell: a frame standing on end** (`misc/hide_frame.bbmodel`),
        // the skin laced inside it -- walked round, not over. How deep it
        // stands is `types::collision_depth`. It was a quarter of a cell for
        // a while, drawn as a skin pegged out on the ground, and a player
        // found that "strange, like little pegs": the frame is what hides
        // were dried on.
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 2,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_HIDE_FRAME),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ---- the jug ----
    //
    // The third thing the clay chain makes, after the crucible and the
    // mould, and the first one that is not about metal. What it is for
    // is `primitive_shared::body`: water is not something a player
    // stands in and drinks from any more, it is something they have to
    // *carry* if they are going anywhere dry.
    BlockDef {
        id: BLOCK_JUG_RAW,
        name: "jug_raw",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_JUG_RAW),
        leaves_behind: None,
        weight: 1.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **One to a slot, and the reason is what a jug now holds.** What
    // has been poured into it rides in `inventory::Stack::damage` --
    // one number for the whole slot, exactly like the wear on a tool.
    // Four jugs sharing that number cannot say that this one holds
    // grain and that one sand: the first merge would pick a winner and
    // the loser's contents would stop existing. A jug is a vessel with
    // a shape anyway, and a pile of a hundred and twenty-eight of them
    // in one square was always the bug a stack of axes is.
    BlockDef {
        id: BLOCK_JUG,
        name: "jug",
        // **A vessel you can set down**, which is what a jug is: it
        // was carry-only, so a player with one had nowhere to put it
        // but a pack slot. Drawn by `mesh::jug_block` rather than as a
        // cube -- a jug rendered as a full block is a clay crate.
        shape: Shape::Cube,
        // Five eighths, which is the model's own height (`thickness` is
        // in eighths -- `LAYERS_PER_BLOCK`). The collider and the step
        // height read this, and a model taller than its box is a thing
        // you walk through.
        thickness: 5,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_JUG),
        leaves_behind: None,
        weight: 1.0,
        stack: ONE,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It turns, handle and all.** The handle stands off one side of
        // the belly, and it used to be the +X side of every jug in the
        // world whichever way the player was looking when they set it
        // down. A jug faces its placer now, like anything else that is
        // not the same all round -- and a jug from an older save, with no
        // facing in it, reads as north and keeps the handle it had.
        faces: true,
        falls: false,
        // **A set-down jug opens**, and it is a container for the same
        // reason a chest is: the right click has to open it rather than
        // put a block on top of it, the server has to accept container
        // gestures against its cell, and breaking it has to account for
        // what is inside. What is inside is one slot of the container
        // store (`inventory::VESSEL_SLOT`), folded back into the jug when
        // it drops -- see `primitive_server::pick_up_vessel` -- so a jug
        // carried, set down and picked up again is the same jug of grain.
        container: true,
        turns: false,
        // It stands on something. A jug hanging in the air where the
        // table was is the fault `needs_support` exists for.
        propped: true,
    },
    // Two kilos heavier than the empty one, which is what two litres
    // of water weighs and what makes carrying it a decision.
    BlockDef {
        id: BLOCK_JUG_WATER,
        name: "jug_water",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_JUG_WATER),
        leaves_behind: None,
        weight: 3.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a person wears ----
    //
    // One row apiece, and everything that makes a garment a garment --
    // which slot it fills, what it stops, what it keeps out, what it
    // costs to move in -- is in `crate::equipment`. This table says what
    // the *material* is: how heavy, how many swings it survives, and
    // that it is a thing you hold one of rather than pile up.
    BlockDef {
        id: BLOCK_LEATHER_CAP,
        name: "leather_cap",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEATHER_CAP),
        leaves_behind: None,
        weight: 0.4,
        stack: ONE,
        durability: Some(120),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEATHER_TUNIC,
        name: "leather_tunic",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEATHER_TUNIC),
        leaves_behind: None,
        weight: 1.2,
        stack: ONE,
        durability: Some(200),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEATHER_LEGGINGS,
        name: "leather_leggings",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEATHER_LEGGINGS),
        leaves_behind: None,
        weight: 0.9,
        stack: ONE,
        durability: Some(160),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEATHER_BOOTS,
        name: "leather_boots",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LEATHER_BOOTS),
        leaves_behind: None,
        weight: 0.7,
        stack: ONE,
        durability: Some(140),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_HELM,
        name: "bronze_helm",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_HELM),
        leaves_behind: None,
        weight: 2.2,
        stack: ONE,
        durability: Some(360),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_CUIRASS,
        name: "bronze_cuirass",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_CUIRASS),
        leaves_behind: None,
        weight: 7.5,
        stack: ONE,
        durability: Some(520),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_GREAVES,
        name: "bronze_greaves",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_GREAVES),
        leaves_behind: None,
        weight: 4.0,
        stack: ONE,
        durability: Some(420),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_BOOTS,
        name: "bronze_boots",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BRONZE_BOOTS),
        leaves_behind: None,
        weight: 2.6,
        stack: ONE,
        durability: Some(380),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_HELM,
        name: "iron_helm",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_HELM),
        leaves_behind: None,
        weight: 2.6,
        stack: ONE,
        durability: Some(700),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_CUIRASS,
        name: "iron_cuirass",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_CUIRASS),
        leaves_behind: None,
        weight: 9.0,
        stack: ONE,
        durability: Some(1000),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_GREAVES,
        name: "iron_greaves",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_GREAVES),
        leaves_behind: None,
        weight: 4.8,
        stack: ONE,
        durability: Some(820),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_BOOTS,
        name: "iron_boots",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_BOOTS),
        leaves_behind: None,
        weight: 3.1,
        stack: ONE,
        durability: Some(740),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a felled tree leaves ----
    BlockDef {
        id: BLOCK_STRIPPED_LOG,
        name: "stripped_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // **Softer than the trunk it came from, and that is the point of
        // felling.** A standing trunk is nine seconds to an axe; this is
        // deadfall the moment it lands, so it comes apart in the time a
        // fallen log always has. What felling buys is not cheaper wood,
        // it is *reachable* wood -- a tree you can bring down instead of
        // one you have to find already down.
        hardness: Some(4.5),
        felled: Some(3.0),
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STRIPPED_LOG),
        leaves_behind: None,
        // Lighter than a log: the bark and the sap went with the fall.
        weight: 0.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        // It lies along an axis, like the log it was. A felled trunk
        // drawn standing up would be a felled trunk nobody believes.
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- 1.9: the copper bench ----
    //
    // Four heads and two tools. A head is a lump of cast metal and
    // nothing else: it has no tier, so holding one against a rock face
    // achieves exactly what holding an ingot does. It stacks, because
    // the thing you cast six of in an evening is a supply of heads --
    // where a *hafted* tool is `ONE`, since it wears out and a stack of
    // things with different amounts of life left in them is not a stack.
    BlockDef {
        id: BLOCK_COPPER_AXE_HEAD,
        name: "copper_axe_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_PICK_HEAD,
        name: "copper_pick_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_SHOVEL_HEAD,
        name: "copper_shovel_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.7,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_HOE_HEAD,
        name: "copper_hoe_head",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The shovel. `Work::Ground` is the whole of what makes it a shovel
    // -- see that variant, and `break_seconds_with` for the factor of
    // two it buys. `tool: Some(Tier::Copper)` so that it counts as a
    // copper tool everywhere a tier is asked for, which for loose ground
    // is the same answer any other copper tool gives; the speed is the
    // difference, not the reach.
    BlockDef {
        id: BLOCK_COPPER_SHOVEL,
        name: "copper_shovel",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Ground,
        tool: Some(Tier::Copper),
        drop: None,
        leaves_behind: None,
        weight: 1.1,
        stack: ONE,
        // Longer-lived than the flint hoe and shorter than the axe: a
        // shovel meets nothing hard, and what wears one out is the sheer
        // number of swings rather than what it hits.
        durability: Some(200),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The copper hoe. `Work::Plant` and no tier, exactly like the flint
    // one: tilling is not mining, and what a hoe is worth is measured in
    // how long it lasts. See `is_hoe`, which is what the server asks
    // before turning a block of turf into a field.
    BlockDef {
        id: BLOCK_COPPER_HOE,
        name: "copper_hoe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.0,
        stack: ONE,
        // Three times the flint hoe's, which is the whole reason to make
        // one: a flint blade lashed to a stick is what breaks first in a
        // field, and a season of tilling is what the metal buys.
        durability: Some(360),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- 1.9: the torch ----
    //
    // Three rows for one object at three points in its life. See
    // `types::BLOCK_TORCH` for why the spent one is its own id.
    //
    // None of them is placeable, and that is a decision rather than an
    // omission: a torch on a wall is a *light source in the world*, and
    // light in the world is baked into the chunk light map, which is
    // where every other lamp in this game lives. What was asked for is
    // the other thing -- a light you carry -- and that one cannot be a
    // block at all, because the map has no cell to put it in. See the
    // held light in `shader.wgsl`.
    BlockDef {
        id: BLOCK_TORCH,
        name: "torch",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The burning one. **Its durability is seconds, not swings**, and it
    // is the only row in this table where that is true -- see
    // `types::TORCH_SECONDS` and the burn-down in the server's tick.
    // Stack of one for the reason every worn thing is: two torches with
    // different amounts of fibre left are not interchangeable, and a
    // stack is a promise that they are.
    BlockDef {
        id: BLOCK_TORCH_LIT,
        name: "torch_lit",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        // Light, and it goes nowhere. `emission` is what a *placed*
        // block pours into the light map, and a torch is never placed;
        // the number is here as a fact about the object, and as the one
        // place the held light's strength is written down.
        emission: 12,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: ONE,
        durability: Some(crate::types::TORCH_LIFE),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ...and the stick that is left, charred at one end.
    BlockDef {
        id: BLOCK_TORCH_SPENT,
        name: "torch_spent",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.25,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the rock under the soil, in three kinds ----
    //
    // Laid by the generator where each forms; see `types::BLOCK_SANDSTONE`
    // for why there are three and what each is for. Hardness climbs
    // with the rock: sandstone comes apart faster than plain stone,
    // limestone about the same, and granite needs copper -- a mountain is
    // the first place a flint pick is not enough.
    // Soft, and the desert quarry.
    BlockDef {
        id: BLOCK_SANDSTONE,
        name: "sandstone",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(4.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        // Itself, not cobble: a quarried rock is a building stone of its
        // own colour, and that colour is the whole reason to quarry it.
        drop: Some(BLOCK_SANDSTONE),
        leaves_behind: None,
        weight: 2.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The lowland rock, where flint forms.
    BlockDef {
        id: BLOCK_LIMESTONE,
        name: "limestone",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.5),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        // Itself, not cobble: a quarried rock is a building stone of its
        // own colour, and that colour is the whole reason to quarry it.
        drop: Some(BLOCK_LIMESTONE),
        leaves_behind: None,
        weight: 2.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // What dripping water leaves in a cave, standing and hanging. See
    // `dripstone` for what they are for.
    //
    // **A cube to every rule that asks, drawn as a model** -- the twig's
    // arrangement: the mesher draws the spike (`mesh::dripstone_block`),
    // `geometry::block_box` gives the box a body meets, and
    // `types::is_collidable` keeps the cell from being a floor.
    //
    // **Hands snap one off**, in a second of the table's time: a spike of
    // calcite is brittle, and a cave a player without a pick cannot walk
    // through is a cave behind a wall. A pick is quicker (`Work::Stone`).
    // What comes off is a pebble -- rubble, not a quarried stone, or the
    // spike would be limestone for a player who never made the pick
    // limestone asks for.
    //
    // **Propped**, so it goes when the rock it grows from does
    // (`types::support_at`), and **not placeable**: one is grown by water
    // over centuries, and a player who could stand one up anywhere would
    // be laying spike traps, which is a different game.
    BlockDef {
        id: crate::types::BLOCK_STALAGMITE,
        name: "stalagmite",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Lets the light through: a spike a few sixteenths wide in a cell
        // of air, for the twig's reason.
        opacity: 0,
        emission: 0,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(crate::types::BLOCK_PEBBLE),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: crate::types::BLOCK_STALACTITE,
        name: "stalactite",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: None,
        drop: Some(crate::types::BLOCK_PEBBLE),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // The mountain, and what a flint pick cannot bite.
    BlockDef {
        id: BLOCK_GRANITE,
        name: "granite",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(9.0),
        felled: None,
        needs: Tier::Copper,
        work: Work::Stone,
        tool: None,
        // Itself, not cobble: a quarried rock is a building stone of its
        // own colour, and that colour is the whole reason to quarry it.
        drop: Some(BLOCK_GRANITE),
        leaves_behind: None,
        weight: 2.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **The floor of the world, and the one rock that is a gate.**
    // Harder than granite and a rung above it in what it needs: granite
    // wants copper, this wants bronze, so the bottom of a deep shaft is
    // shut until the player has alloyed something. See
    // `types::BLOCK_BASALT` for why that is the point of it rather than
    // an obstacle in front of it.
    BlockDef {
        id: BLOCK_BASALT,
        name: "basalt",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Half again on granite. With a bronze pick that is about seven
        // seconds a block, which is slow enough to be felt and quick
        // enough that a corridor through it is an evening's work rather
        // than a project.
        hardness: Some(13.0),
        felled: None,
        needs: Tier::Bronze,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_BASALT),
        leaves_behind: None,
        // The heaviest stone in the game, and it is heavy for the same
        // reason it is hard: there is more rock in the same block.
        weight: 3.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- fur ----
    //
    // The two garments made of the skin itself. Heavy on purpose: a
    // pelt with the hair on has all the water it came with and none of
    // the tannery's work done to it, so the cloak weighs three times
    // the leather tunic and the hood two. That number is not flavour --
    // it goes through the load rules into a fall, into how fast a
    // player walks and, since 1.5, into whether they float. See
    // `equipment::garment` for the rest of the row and
    // `types::BLOCK_FUR_CLOAK` for why fur is in the game at all.
    //
    // Durability under leather's. Untanned skin rots and stiffens, and
    // the coat that costs one animal should not be the coat that lasts
    // longest.
    BlockDef {
        id: BLOCK_FUR_HOOD,
        name: "fur_hood",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FUR_HOOD),
        leaves_behind: None,
        weight: 1.8,
        stack: ONE,
        durability: Some(140),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FUR_CLOAK,
        name: "fur_cloak",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FUR_CLOAK),
        leaves_behind: None,
        weight: 3.6,
        stack: ONE,
        durability: Some(160),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- wild cereal, and what bread is made of ----
    //
    // A stand of wheat that nobody planted. Cross-shaped like every
    // other wild plant, picked by hand, and what comes off it is seed:
    // see `types::BLOCK_WILD_WHEAT` for why seed comes from here and
    // not from a tuft of grass any more.
    BlockDef {
        id: BLOCK_WILD_WHEAT,
        name: "wild_wheat",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // The tuft's, because it is one: a handful of stalks pulled up.
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_SEEDS),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        // Living plant matter, tinted by the climate like the grass
        // around it -- a stand in a savanna is straw and the same stand
        // in a meadow is green, which is what cereal does.
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- cotton ----
    //
    // The wild stand, the three stages of the planted crop, the boll
    // and the bolt, and the four garments. Every plant row is the
    // wheat's row with cotton's drops in it: see `types::BLOCK_WILD_COTTON`
    // for the chain and `types::also_drops` for where the seed comes from.
    BlockDef {
        id: BLOCK_WILD_COTTON,
        name: "wild_cotton",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Wild wheat's, because it is the same gesture: a handful of
        // stems pulled up.
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_COTTON),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COTTON_SEEDS,
        name: "cotton_seeds",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Pulled up by hand in an instant, like the wheat's seed.
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_COTTON_SEEDS),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_COTTON_PLANT,
        name: "cotton_plant",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_COTTON_SEEDS),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_COTTON_RIPE,
        name: "cotton_ripe",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_COTTON),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // ---- millet ----
    //
    // Cotton's rows with millet's drops: the grain is the seed, so every
    // stage gives grain back (`types::BLOCK_WILD_MILLET`), and the ripe head
    // three of it (`types::block_drop_count`). Porridge is bread's row.
    BlockDef {
        id: BLOCK_WILD_MILLET,
        name: "wild_millet",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_MILLET),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_MILLET,
        name: "millet",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_MILLET),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_MILLET_PLANT,
        name: "millet_plant",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_MILLET),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_MILLET_RIPE,
        name: "millet_ripe",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_MILLET),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_MILLET_PORRIDGE,
        name: "millet_porridge",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the bowl ----
    //
    // Thrown on the wheel and fired in the kiln like the pot, and carried
    // only: a bowl is a thing a meal is served in, not a thing that stands
    // in the world holding anything. What it is for is `types::BLOCK_STEW`.
    BlockDef {
        id: BLOCK_BOWL_RAW,
        name: "bowl_raw",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BOWL_RAW),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BOWL,
        name: "bowl",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BOWL),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A bowl with the stew in it: a bowl's weight and a meal's. Stacks, as
    // porridge does -- every bowl of it is the same stew.
    BlockDef {
        id: BLOCK_STEW,
        name: "stew",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A bowl of milk: the stew's bowl and weight, and the stew's stacking.
    BlockDef {
        id: BLOCK_BOWL_MILK,
        name: "bowl_milk",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A cairn (`types::BLOCK_CAIRN`): six stones piled three-quarters of a
    // cell high. Its box is its heap's height, the nest's rule -- the
    // collider and the step read `thickness`, and a heap drawn taller than
    // its box is a heap walked through. Light goes past it (`opacity: 0`)
    // and it covers nothing (`mesh`), because it is three stones and air.
    // Broken by hand, which is the point of a cairn: a heap of stones
    // anybody can take apart, back into the six (`block_drop_count`).
    BlockDef {
        id: BLOCK_CAIRN,
        name: "cairn",
        shape: Shape::Cube,
        thickness: 6,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PEBBLE),
        leaves_behind: None,
        // The six stones it is: carried to the place, which is the cost.
        weight: 0.9,
        stack: 8,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A lodestone: a lump of iron ore, and weighs as one. See `types::BLOCK_LODESTONE`.
    BlockDef {
        id: BLOCK_LODESTONE,
        name: "lodestone",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_LODESTONE),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A water compass: a bowl of water with a needle on a leaf, one to a
    // hand -- a stack of them would be a stack of bowls of water.
    BlockDef {
        id: BLOCK_WATER_COMPASS,
        name: "water_compass",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WATER_COMPASS),
        leaves_behind: None,
        weight: 0.9,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COTTON,
        name: "cotton",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COTTON),
        leaves_behind: None,
        // A boll is nearly nothing. A stack of them should not be a
        // load, which is the one thing that makes carrying the harvest
        // home from a field in the south a trip rather than a haul.
        weight: 0.03,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLOTH,
        name: "cloth",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLOTH),
        leaves_behind: None,
        weight: 0.12,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLOTH_CAP,
        name: "cloth_cap",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLOTH_CAP),
        leaves_behind: None,
        // Lighter than wool in every slot. The garments' real numbers
        // are in `equipment::garment`; this is only what they weigh.
        weight: 0.07,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLOTH_TUNIC,
        name: "cloth_tunic",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLOTH_TUNIC),
        leaves_behind: None,
        weight: 0.20,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLOTH_TROUSERS,
        name: "cloth_trousers",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLOTH_TROUSERS),
        leaves_behind: None,
        weight: 0.17,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CLOTH_WRAPS,
        name: "cloth_wraps",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CLOTH_WRAPS),
        leaves_behind: None,
        weight: 0.10,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WITHERED_CROP,
        name: "withered_crop",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Dead stalks: not living plant matter, so not tinted by the
        // climate either (`foliage` below) -- a frozen field is the same
        // brown in a swamp as on a steppe, which is what makes it read as
        // dead rather than as a drier kind of green.
        hardness: Some(0.05),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.85,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_FLOUR,
        name: "flour",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        // Lighter than the grain it came from, because grinding is the
        // one step in the chain that loses something: the bran.
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the nest ----
    //
    // A low bowl of twigs in a canopy, and the same bowl with eggs in
    // it. A squat cube rather than a cross, because a nest is a *thing
    // on a branch* and a crossed plane reads as a plant; two eighths
    // tall, the way a carcass is a heap. It needs something under it
    // (`propped`), and `can_grow_on` says that something is a tree.
    //
    // Breaking the full one gives the eggs and leaves the bowl -- see
    // `leaves_behind`, the field the berry bush is the reason for --
    // and the bowl fills again on the growth clock. See
    // `types::BLOCK_NEST`.
    BlockDef {
        id: BLOCK_NEST_EGGS,
        name: "nest_eggs",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        // It does not stop light: a bowl of sticks, not a wall.
        opacity: 0,
        emission: 0,
        // Taken, not smashed. Quick, because the whole cost of a nest
        // is the climb.
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_EGG),
        leaves_behind: Some(BLOCK_NEST),
        weight: 0.4,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // **Not placeable, and the empty one is not either.** A nest a
        // player can put where they like is a hen house, and this is
        // meant to be a place in a wood that you remember.
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_NEST,
        name: "nest",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // Sticks. Not nothing: a robbed nest is a handful of kindling
        // for anybody willing to take the whole thing apart -- and
        // taking it apart is how you lose the place it was.
        drop: Some(BLOCK_STICK),
        leaves_behind: None,
        weight: 0.3,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_EGG,
        name: "egg",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.06,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- furniture ----
    //
    // Four low blocks that are all the same idea: something a person
    // put here. Each is `propped`, because a chair floating in the air
    // is the one thing furniture must never do -- see
    // `types::BLOCK_STRAW_BED` for what each is worth.
    //
    // `Work::Wood` on all four, so an axe is the quick way to take one
    // apart and a fist is the slow one. Broken, each gives itself back:
    // furniture you cannot move is furniture you build once and regret.
    BlockDef {
        id: BLOCK_STRAW_BED,
        name: "straw_bed",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STRAW_BED),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It has a front**, because it has a head and a foot: a pallet
        // two cells long with a bolster of straw at one end, lying the way
        // the player looked when they put it down -- the plank bed's rule
        // and the plank bed's bits (`types::BED_HEAD`, `types::is_bed`).
        faces: true,
        falls: false,
        container: false,
        turns: false,
        // A pile of grass still needs a floor under it.
        propped: true,
    },
    BlockDef {
        id: BLOCK_BED,
        name: "bed",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BED),
        leaves_behind: None,
        weight: 6.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It has a front**, because it has a head and a foot: two cells
        // lying the way the player looked when they put it down, the foot
        // toward them. The facing is in the low bits of the variant field
        // and which half a cell is in the bit above -- see
        // `types::BED_HEAD`.
        faces: true,
        falls: false,
        container: false,
        turns: false,
        // A bed on nothing is a bed falling.
        propped: true,
    },
    BlockDef {
        id: BLOCK_STOOL,
        name: "stool",
        shape: Shape::Cube,
        thickness: 4,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(0.9),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STOOL),
        leaves_behind: None,
        weight: 2.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It turns.** Three legs are two at the back and one at the
        // front, so a quarter turn of a stool is a different stool -- and
        // a block that looks different from one side than another faces
        // whoever put it down. What it does *not* have is a side a sitter
        // faces: see `types::seat_yaw`, which is still the chair's alone.
        faces: true,
        falls: false,
        container: false,
        turns: false,
        // Three legs need a floor.
        propped: true,
    },
    BlockDef {
        id: BLOCK_CHAIR,
        name: "chair",
        shape: Shape::Cube,
        // The stool's half cell: the seat is where the collider stops, so
        // a sitter's feet rest on it and a walker steps up onto it. The
        // back above is the model's, not the row's -- see `types::BLOCK_CHAIR`.
        thickness: 4,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(1.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_CHAIR),
        leaves_behind: None,
        weight: 4.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        // **It has a front**: put down facing whoever placed it, back away
        // from them, which is the way round a chair is pulled out to sit
        // in. The sitter looks the way it faces (`types::seat_yaw`).
        faces: true,
        falls: false,
        container: false,
        turns: false,
        // Four legs need a floor as much as three do.
        propped: true,
    },
    BlockDef {
        id: BLOCK_TABLE,
        name: "table",
        shape: Shape::Cube,
        thickness: 6,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_TABLE),
        leaves_behind: None,
        weight: 8.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        // So does a table.
        propped: true,
    },
    // **The four workshops** (`types::BLOCK_WORKBENCH`), on the table's terms:
    // furniture that lets light by, stands on a floor and comes back whole.
    // They face whoever put them down, because a bench has a side you work
    // from and a wheel a side you sit at. Heights are the models' own, so
    // the collider is the top a player would set a thing on: a bench and a
    // currier's table waist high, the mason's slab a little lower because a
    // stone is worked from above, the wheel's head at a seated potter's lap -- and
    // above a step, because a wheel somebody walks up onto is a wheel
    // knocked off its spindle.
    //
    // **The mason's block breaks by hand, slowly**, rather than wanting a
    // pick: it is a slab lifted onto a stump, not rock, and a workshop that
    // could only be moved with a tool the stone age has not reached yet
    // would be a workshop built in the wrong place for good.
    BlockDef {
        id: BLOCK_WORKBENCH,
        name: "workbench",
        shape: Shape::Cube,
        thickness: 6,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_WORKBENCH),
        leaves_behind: None,
        weight: 12.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_MASON_BLOCK,
        name: "mason_block",
        shape: Shape::Cube,
        thickness: 5,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_MASON_BLOCK),
        leaves_behind: None,
        weight: 40.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_POTTERS_WHEEL,
        name: "potters_wheel",
        shape: Shape::Cube,
        thickness: 5,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_POTTERS_WHEEL),
        leaves_behind: None,
        weight: 14.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_ANVIL,
        name: "anvil",
        shape: Shape::Cube,
        thickness: 6,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // **The one workshop that wants a pick**, where the other four come up
        // by hand. A bronze block on an oak stump is not a slab lifted onto a
        // stump: it is eighty kilos of cast metal, and a player who could pull
        // one off the floor with their fingers would be a player for whom the
        // five ingots it cost bought nothing but a picture. Slower than the
        // mason's block for the same reason, and it comes back whole -- an
        // anvil that broke into scrap would be a workshop nobody dares move.
        hardness: Some(7.0),
        felled: None,
        needs: Tier::Stone,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_ANVIL),
        leaves_behind: None,
        weight: 60.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // ---- the hammer and the chisel ----
    //
    // **Held tools that open nothing.** `tool` is `None` on all five rows, so
    // neither of them is a gate on any block and neither hurries a swing: what
    // they are for is the two rows that ask for one (`crafting`) and the two
    // mini-games that do (`minigame`). The alternative -- giving the hammer
    // `Some(Tier::Stone)` so it felt like a tool -- would have made it a
    // second, heavier pick that opens the same rock, which is the chore this
    // codebase refuses.
    //
    // Durability is the axe's ladder, halved: a hammer is used a few times a
    // session at a bench, not a hundred times a minute in a mine, and a head
    // that wore like a pick would never be replaced at all. See
    // `types::BLOCK_STONE_HAMMER`.
    BlockDef {
        id: BLOCK_STONE_HAMMER,
        name: "stone_hammer",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.4,
        stack: ONE,
        durability: Some(96),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_HAMMER,
        name: "bronze_hammer",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.6,
        stack: ONE,
        durability: Some(220),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_HAMMER,
        name: "iron_hammer",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.7,
        stack: ONE,
        durability: Some(340),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FLINT_CHISEL,
        name: "flint_chisel",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.3,
        stack: ONE,
        durability: Some(72),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_CHISEL,
        name: "bronze_chisel",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.4,
        stack: ONE,
        durability: Some(200),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LEATHER_BENCH,
        name: "leather_bench",
        shape: Shape::Cube,
        thickness: 6,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_LEATHER_BENCH),
        leaves_behind: None,
        weight: 9.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // **A barter stall** (`types::BLOCK_STALL`): the chest's row in most
    // things -- a container, turned to face whoever put it down, a model a
    // little in from its cell -- and not `propped`, for the chest's reason:
    // a block that falls when its floor is dug goes through the drop path,
    // and a stall's goods belong to the spill.
    BlockDef {
        id: BLOCK_STALL,
        name: "stall",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(2.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STALL),
        leaves_behind: None,
        weight: 8.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // **The door, in its two halves** (`types::BLOCK_DOOR`). What the rows
    // say is the shut door: a wall's opacity, and the whole cell tall so a
    // player's head meets the top half. It is three sixteenths *across*,
    // which the row cannot say (`types::collision_depth`), and open it lets
    // light, smoke and animals through, which the row cannot say either
    // (`types::light_opacity`, `types::blocks_the_sky`). Propped, because a
    // door is hung on a floor; faces, because it is hung across the way its
    // placer looked. Boards to break, a little slower than a chair: a door
    // is the one piece of furniture somebody might break in to get past.
    BlockDef {
        id: BLOCK_DOOR,
        name: "door",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_DOOR),
        leaves_behind: None,
        weight: 6.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // The top gives the door, as the torch's flame gives the torch: a swing
    // at either half is a swing at the door. Never placed by hand.
    BlockDef {
        id: BLOCK_DOOR_TOP,
        name: "door_top",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_DOOR),
        leaves_behind: None,
        weight: 6.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // Undergrowth. The canopy's leaf in every respect but its picture
    // and the fact that it is what `worldgen::place_bush` builds with --
    // see `types::BLOCK_BUSH_LEAVES` for why that is a second id rather
    // than the same one.
    BlockDef {
        id: BLOCK_BUSH_LEAVES,
        name: "bush_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **Slow, not solid.** Leaves stopped being a wall
        // (`types::is_collidable`) and this is the other half of that:
        // pushing through a thicket costs well over half your speed, so
        // going round a wood is still a decision. Snow is 0.6 and deep
        // snow 0.45; a canopy is thicker than either.
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // What a carcass nobody came back for leaves behind: the animal's
    // own bones, lying where it fell. A low heap through the ordinary
    // cube path, like the carcass it used to be -- and drawn as the
    // animal's model in bone, which is why it costs no picture. See
    // `types::BLOCK_BONES`.
    BlockDef {
        id: BLOCK_BONES,
        name: "bones",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // Quicker than the body it was: there is nothing left to cut
        // through, only to gather up.
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // **The bones themselves**, and this is the one thing a
        // skeleton is for besides being a landmark: a hunter who was
        // too slow still gets the part of the animal that does not rot.
        drop: Some(BLOCK_BONE),
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // Not placeable, on the carcass's terms: a player carrying a
        // skeleton around to put down is a player carrying a dead deer.
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The same skeleton for the species past the eighth, row for row: the
    // variant field names eight animals and this id names the next eight.
    // See `types::BLOCK_BONES_2`.
    BlockDef {
        id: BLOCK_BONES_2,
        name: "bones_2",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BONE),
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ...and again for the species past the sixteenth. See
    // `types::BLOCK_BONES_3`.
    BlockDef {
        id: BLOCK_BONES_3,
        name: "bones_3",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BONE),
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ...and past the twenty-fourth. See `types::BLOCK_BONES_4`.
    BlockDef {
        id: BLOCK_BONES_4,
        name: "bones_4",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BONE),
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the water barrel ----
    //
    // Staves round water, drawn by `mesh::barrel_block`. Three rows for
    // one object because the kind of water is the id -- see
    // `types::BLOCK_BARREL` -- and all three break back into the empty
    // one: the water goes with the staves.
    BlockDef {
        id: BLOCK_BARREL,
        name: "barrel",
        shape: Shape::Cube,
        // Seven eighths: fourteen sixteenths of staves, which is the
        // model's own height. The collider and the step height read this.
        thickness: 7,
        matter: Matter::Solid,
        // Furniture does not wall a room off from its own light.
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        // It stands on something, like the table beside it.
        propped: true,
    },
    BlockDef {
        id: BLOCK_BARREL_STANDING,
        name: "barrel_standing",
        shape: Shape::Cube,
        thickness: 7,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        // Written by the world, never put down: see `PLACEABLE_BLOCKS`.
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_BARREL_SALT,
        name: "barrel_salt",
        shape: Shape::Cube,
        thickness: 7,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // ...and in its three grains (`types::BLOCK_BARREL_GRAIN`). They break
    // into the empty barrel as the waters do; the grain is spilled beside
    // it by the server, because unlike water it is still there.
    BlockDef {
        id: BLOCK_BARREL_GRAIN,
        name: "barrel_grain",
        shape: Shape::Cube,
        thickness: 7,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_BARREL_SEEDS,
        name: "barrel_seeds",
        shape: Shape::Cube,
        thickness: 7,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_BARREL_MILLET,
        name: "barrel_millet",
        shape: Shape::Cube,
        thickness: 7,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_BARREL),
        leaves_behind: None,
        weight: 6.0,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // ---- the tinder bracket ----
    //
    // A shelf of hard fungus on the side of dead wood, which is where
    // one grows: a bracket is a fruiting body pushing out through bark,
    // and it is horizontal because it has to drop spores from the pores
    // on its underside. It was a cross standing on top of a log for one
    // version, which is the shape of a plant and not of a fungus, and
    // it read as a mushroom somebody had balanced there. Picked with
    // the fingers: the thing amadou
    // is *for* is being available to somebody who has nothing, and a
    // tinder that wanted a knife would be tinder for a player who
    // already had a fire. See `types::BLOCK_BRACKET_FUNGUS`.
    BlockDef {
        id: BLOCK_BRACKET_FUNGUS,
        name: "bracket_fungus",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // Twice the mushroom's, because this one is *woody*. Still under
        // half a second: the difference is meant to be felt as the thing
        // coming away from the bark rather than as a wait.
        hardness: Some(0.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BRACKET_FUNGUS),
        leaves_behind: None,
        // Dry through, which is why it burns. Lighter than the mushroom
        // is heavy with water and a good deal larger, so the two come
        // out at about the same weight for opposite reasons.
        weight: 0.08,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        // The mushroom's argument, and stronger: this grows on dead wood
        // in the shade, and the climate of the meadow forty blocks away
        // has nothing to do with its colour.
        foliage: false,
        orientable: false,
        // **It hangs off a trunk, so it has a side to hang off.** The
        // facing is which way it looks *out* from the wood -- see
        // `types::support_at`, which turns that into the cell holding
        // it up, and `mesh::bracket_block`, which turns it into which
        // wall of the cell the shelf grows from.
        faces: true,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the bog's fuel ----
    //
    // Peat is a block of the ground, dug by hand like dirt, and it is
    // the raw thing: what it becomes is decided on the rack
    // (`rack::cures_into`) and what that is worth at the fire
    // (`hearth::fuel_seconds`).
    BlockDef {
        id: BLOCK_PEAT,
        name: "peat",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Ground,
        tool: None,
        drop: Some(BLOCK_PEAT),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.95,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A brick of peat off the rack: fuel.
    BlockDef {
        id: BLOCK_DRIED_PEAT,
        name: "dried_peat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_PEAT),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A sod half dried where it was set down (`types::BLOCK_DRYING_PEAT`):
    // an item, lighter than the wet sod and heavier than the brick, and not
    // fuel.
    BlockDef {
        id: BLOCK_DRYING_PEAT,
        name: "drying_peat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRYING_PEAT),
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a carcass is made of ----
    // The cord a metal head is lashed on with.
    BlockDef {
        id: BLOCK_SINEW,
        name: "sinew",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SINEW),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Bone, out of every carcass after the sinew.
    BlockDef {
        id: BLOCK_BONE,
        name: "bone",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BONE),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the carcasses ----
    //
    // Where an animal fell. A low heap -- two eighths for a hare, three
    // for the rest -- through the ordinary cube path, so it is lit,
    // collided and walked over like a layer of sand. The species is the
    // block and the *stage* of butchering is the variant, so a carcass
    // half taken apart survives a save without any state beside the
    // block: see `types::VARIANT_SHIFT` and `Species::butchering`.
    BlockDef {
        id: BLOCK_CARCASS_HARE,
        name: "carcass_hare",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // **Breaking it gives nothing, and it is slow on purpose.**
        // A carcass used to come apart under the fist for one cut of
        // meat, which meant the fastest way to eat was to punch the
        // animal you had just killed -- the knife, the order of the
        // cuts and the whole of `Species::butchering` were an option
        // for players who felt like it. Now the left hand only
        // *destroys*: three seconds of work and the deer is gone, hide,
        // sinew, bone and all. Everything a body is worth comes off it
        // with the right button and a blade in the hand.
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_DEER,
        name: "carcass_deer",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // **Breaking it gives nothing, and it is slow on purpose.**
        // A carcass used to come apart under the fist for one cut of
        // meat, which meant the fastest way to eat was to punch the
        // animal you had just killed -- the knife, the order of the
        // cuts and the whole of `Species::butchering` were an option
        // for players who felt like it. Now the left hand only
        // *destroys*: three seconds of work and the deer is gone, hide,
        // sinew, bone and all. Everything a body is worth comes off it
        // with the right button and a blade in the hand.
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 60.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_BOAR,
        name: "carcass_boar",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // **Breaking it gives nothing, and it is slow on purpose.**
        // A carcass used to come apart under the fist for one cut of
        // meat, which meant the fastest way to eat was to punch the
        // animal you had just killed -- the knife, the order of the
        // cuts and the whole of `Species::butchering` were an option
        // for players who felt like it. Now the left hand only
        // *destroys*: three seconds of work and the deer is gone, hide,
        // sinew, bone and all. Everything a body is worth comes off it
        // with the right button and a blade in the hand.
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 80.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_WOLF,
        name: "carcass_wolf",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // **Breaking it gives nothing, and it is slow on purpose.**
        // A carcass used to come apart under the fist for one cut of
        // meat, which meant the fastest way to eat was to punch the
        // animal you had just killed -- the knife, the order of the
        // cuts and the whole of `Species::butchering` were an option
        // for players who felt like it. Now the left hand only
        // *destroys*: three seconds of work and the deer is gone, hide,
        // sinew, bone and all. Everything a body is worth comes off it
        // with the right button and a blade in the hand.
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 35.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The two carcasses added with the bear and the bird. Same
    // row as the sheep's, and the three numbers that differ are
    // the animal: how much of the cell it fills, how long it
    // takes to tear at bare-handed, and what it weighs to carry.
    BlockDef {
        id: BLOCK_CARCASS_BEAR,
        name: "carcass_bear",
        shape: Shape::Cube,
        thickness: 4,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // The biggest body in the world takes the longest to destroy,
        // and destroying it is all the left hand does -- see the note
        // on the hare's hardness. Four and a half seconds of punching
        // a bear you have already killed is a thing nobody does by
        // accident -- and it is as long as this may be: five seconds
        // by hand is the ceiling the whole block table is held to (see
        // `types::mining_tests::digging_is_something_you_spend_time_on`),
        // because past that a player thinks the game has stopped.
        hardness: Some(4.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 140.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_FOWL,
        name: "carcass_fowl",
        shape: Shape::Cube,
        thickness: 1,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // A bird is the one body small enough that clearing it out of
        // the way is fair -- but it still gives nothing for it. The
        // meat and the feathers are a knife's work like everything
        // else's.
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 4.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_SHEEP,
        name: "carcass_sheep",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        // It does not stop light: a heap on the ground, not a wall.
        opacity: 0,
        emission: 0,
        // **Breaking it gives nothing, and it is slow on purpose.**
        // A carcass used to come apart under the fist for one cut of
        // meat, which meant the fastest way to eat was to punch the
        // animal you had just killed -- the knife, the order of the
        // cuts and the whole of `Species::butchering` were an option
        // for players who felt like it. Now the left hand only
        // *destroys*: three seconds of work and the deer is gone, hide,
        // sinew, bone and all. Everything a body is worth comes off it
        // with the right button and a blade in the hand.
        hardness: Some(3.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See the note on its hardness above.
        drop: None,
        leaves_behind: None,
        weight: 50.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // The savanna's three carcasses, on the sheep's row. What differs is
    // the animal: how much of the cell it fills, how long it takes to
    // destroy by hand -- which gives nothing, see the sheep's note -- and
    // what it weighs.
    BlockDef {
        id: BLOCK_CARCASS_ZEBRA,
        name: "carcass_zebra",
        shape: Shape::Cube,
        thickness: 4,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // A deer and a half of animal, and a little slower than the sheep
        // to tear apart by hand.
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 110.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_ANTELOPE,
        name: "carcass_antelope",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(2.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 30.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CARCASS_LION,
        name: "carcass_lion",
        shape: Shape::Cube,
        thickness: 3,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 90.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the honest tool chain ----
    //
    // See `types::BLOCK_CORD` for the argument. The wedged axe and pick
    // are what the stone ones used to be -- the tier that fells a tree
    // and breaks stone -- and the lashed-only ones drop to `Tier::Stone`
    // in the crafting rework that follows this table.
    // Fibre twisted into a length.
    BlockDef {
        id: BLOCK_CORD,
        name: "cord",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CORD),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the raft ----
    //
    // See `types::BLOCK_RAFT`. None of the three is placeable: the sail
    // and the oars are parts, and the raft goes onto water as a body
    // (`raft::launch`) rather than into a cell.
    // Four hides of leather and a yard: three kilos, one to a slot.
    BlockDef {
        id: BLOCK_SAIL,
        name: "sail",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SAIL),
        leaves_behind: None,
        weight: 3.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_OAR,
        name: "oar",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_OAR),
        leaves_behind: None,
        weight: 1.5,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **A hundred and fifty kilos, one to a slot.** Sixteen planks, the
    // sail, the oars and the lashing, rolled up. Forty was the first
    // number and it bought nothing against the old six-hundred-kilo
    // capacity. Against the ninety a player carries now
    // (`load::CARRY_CAPACITY_KG`) a rolled raft is an overload: dragged to
    // the water at a shuffle, never carried across country -- so where it
    // is built is the decision (see `types::BLOCK_RAFT`).
    BlockDef {
        id: BLOCK_RAFT,
        name: "raft",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RAFT),
        leaves_behind: None,
        weight: 150.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A stone axe wedged into its haft: fells a tree.
    // ---- what an animal gives, told apart ----
    //
    // Ten items, and every one of them is an existing row with the
    // numbers that make it a different thing: see `types` for why the
    // meats and skins share their pictures and differ in name, weight
    // and worth. The weights are the honest part -- a bear's hide is
    // more than a haunch of venison to carry home, and that is a
    // decision about what to leave behind.
    BlockDef {
        id: BLOCK_HARE_MEAT,
        name: "hare_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_HARE_MEAT),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FOWL_MEAT,
        name: "fowl_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FOWL_MEAT),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BEAR_MEAT,
        name: "bear_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BEAR_MEAT),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WOLF_MEAT,
        name: "wolf_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WOLF_MEAT),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PELT,
        name: "pelt",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PELT),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BEAR_HIDE,
        name: "bear_hide",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BEAR_HIDE),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FEATHER,
        name: "feather",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FEATHER),
        leaves_behind: None,
        weight: 0.01,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FAT,
        name: "fat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FAT),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_RIBS,
        name: "ribs",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RIBS),
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_ROASTED_RIBS,
        name: "roasted_ribs",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ROASTED_RIBS),
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WEDGED_AXE,
        name: "wedged_axe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: Some(Tier::Flint),
        drop: None,
        leaves_behind: None,
        weight: 0.95,
        stack: ONE,
        durability: Some(110),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A stone pick wedged into its haft: breaks stone.
    BlockDef {
        id: BLOCK_WEDGED_PICKAXE,
        name: "wedged_pickaxe",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Stone,
        tool: Some(Tier::Flint),
        drop: None,
        leaves_behind: None,
        weight: 1.05,
        stack: ONE,
        durability: Some(110),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A knapped point on a long haft: the hunting weapon.
    BlockDef {
        id: BLOCK_FLINT_SPEAR,
        name: "flint_spear",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        // A weapon, not a kind of work: it opens nothing a knife does not,
        // and the ladder of one tool per work per age stays a ladder.
        work: Work::Any,
        // No tier: a spear opens no block. What it does to an animal is
        // the server's `hunting_damage`, which knows it by name.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.1,
        stack: ONE,
        durability: Some(70),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the other three spears ----
    //
    // The same weapon with a different head; see
    // `types::BLOCK_BONE_SPEAR` for why there are four of them and why
    // none of them costs a picture. What differs down the table is the
    // two things a better point actually gives: it lasts longer, and
    // (in `hunting_damage`) it goes in deeper.
    BlockDef {
        id: BLOCK_BONE_SPEAR,
        name: "bone_spear",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        // No tier, like the flint one: a spear opens no block. What it
        // does to an animal is `hunting_damage`, which reads the id.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.9,
        // Forty thrusts against the flint's seventy: bone splinters, and that is what makes it the spear you carry until you have knapped a better one.
        stack: ONE,
        durability: Some(40),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_SPEAR,
        name: "copper_spear",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        // No tier, like the flint one: a spear opens no block. What it
        // does to an animal is `hunting_damage`, which reads the id.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.3,
        // Copper bends rather than shatters, so it outlasts flint by half.
        stack: ONE,
        durability: Some(110),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRONZE_SPEAR,
        name: "bronze_spear",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        // No tier, like the flint one: a spear opens no block. What it
        // does to an animal is `hunting_damage`, which reads the id.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.35,
        // Bronze is the first point that holds an edge through a hunt.
        stack: ONE,
        durability: Some(170),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_IRON_SPEAR,
        name: "iron_spear",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        // No tier, like the flint one: a spear opens no block. What it
        // does to an animal is `hunting_damage`, which reads the id.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.4,
        // Iron: the last spear anybody needs to make.
        stack: ONE,
        durability: Some(260),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A stone with rust on its face, lying where a river or a bog left
    // it: picked up like a pebble, and the whole of the iron a people
    // without mines has. See `types::BLOCK_RUSTY_STONE`.
    BlockDef {
        id: BLOCK_RUSTY_STONE,
        name: "rusty_stone",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RUSTY_STONE),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    // A bar of sandstone an edge is honed on. An ingredient that comes
    // back, like the pebble a grain is ground with. See `tools::hone`.
    BlockDef {
        id: BLOCK_WHETSTONE,
        name: "whetstone",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WHETSTONE),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // What a smelt runs off the metal. **A stone, and it is laid as one**:
    // heavier than rock (fayalite is near half as dense again as limestone)
    // and glassy-hard, so it wants the pick that wants rock. A player who
    // has slag has a bloomery or a crucible, and has had that pick for a
    // long time.
    BlockDef {
        id: BLOCK_SLAG,
        name: "slag",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_SLAG),
        leaves_behind: None,
        weight: 3.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    // Iron with carbon in it, not yet quenched. Weighs what the iron did:
    // a bar takes up a few grams of carbon and loses a little scale.
    BlockDef {
        id: BLOCK_STEEL_INGOT,
        name: "steel_ingot",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STEEL_INGOT),
        leaves_behind: None,
        weight: 0.55,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Cassiterite on a riverbank. **Lies like a pebble and comes up as
    // ore**, the way mud comes up as clay: the pebble is where it was, the
    // ore is what it is, and one tin ore item is what every smelting row
    // already knows how to use. Not placeable, because nothing a player
    // carries is a pebble of it.
    BlockDef {
        id: BLOCK_STREAM_TIN,
        name: "stream_tin",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_TIN_ORE),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    // ---- wild bees ----
    //
    // See `types::BLOCK_WILD_HIVE` and `bees`.
    BlockDef {
        id: BLOCK_WILD_HIVE,
        name: "wild_hive",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // **Not a wall's fifteen.** A comb is half a cell of wax against a
        // trunk (`types::hive_side`), and a full opacity made the mesher
        // treat the whole cell as covered: the turf under a hive lost its
        // top face and the sky showed through the floor beside the tree.
        // Three, so the shade under a hive is a shade rather than a hole.
        opacity: 3,
        emission: 0,
        // Torn out by hand, and quick: comb is soft. The whole cost of a
        // hive is the bees (`bees::stings`), and a slow break would be the
        // stings again, paid in waiting.
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // **The empty comb's drop**, because the plain id is the empty hive.
        // A hive with honey in it gives the honey and leaves the comb, which
        // the table cannot say: see `types::block_drop` and `block_residue`.
        drop: Some(BLOCK_BEESWAX),
        leaves_behind: None,
        weight: 1.0,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        // **Hung on a trunk, not stood on anything.** Propped, the first
        // hive the generator put on the side of a tree would have dropped off
        // it: there is air under a hive, and that is where the bees go in.
        propped: false,
    },
    BlockDef {
        id: BLOCK_HONEY,
        name: "honey",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_HONEY),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BEESWAX,
        name: "beeswax",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BEESWAX),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- fishing ----
    //
    // See `types::BLOCK_FISH_TRAP` and `fishing`.
    BlockDef {
        id: BLOCK_FISH_TRAP,
        name: "fish_trap",
        // A cube, and a solid one: see `types::BLOCK_FISH_TRAP` for why the
        // trap is not water with a basket in it, as kelp is water with a
        // plant in it.
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Wicker: light and water through the weave, so it is drawn with
        // its holes (`types::is_cutout`) and does not throw a black cube of
        // shade onto the river bed.
        opacity: 1,
        emission: 0,
        // Lifted out by hand, quickly: it is a basket. What is in it is
        // spilled at the lifter's feet (the server's break path), not lost.
        hardness: Some(0.4),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FISH_TRAP),
        leaves_behind: None,
        weight: 1.2,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        // It sits on the bed of the river, or is wedged in the bank; a trap
        // hanging in open water over nothing is not a thing anybody set.
        propped: false,
    },
    BlockDef {
        id: BLOCK_FISHING_ROD,
        name: "fishing_rod",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        // Opens no block: what it does is `fishing`, on the server.
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.5,
        stack: ONE,
        // Sixty fish, and then the line and the hook are gone and the haft
        // is kindling. Enough for a season's fishing from one hook, which is
        // a quarter of an ingot -- dear enough that a rod is a thing you
        // own, cheap enough that it is not a thing you save for.
        durability: Some(60),
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COPPER_HOOK,
        name: "copper_hook",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COPPER_HOOK),
        leaves_behind: None,
        weight: 0.02,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **Bait.** Three items that exist to go on a hook -- see `fishing::Bait`
    // for what each one catches and why there are six baits and only three
    // new things. Weightless enough to carry a pocketful; a worm that made a
    // player heavy would be a worm nobody dug.
    BlockDef {
        id: BLOCK_WORM,
        name: "worm",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WORM),
        leaves_behind: None,
        weight: 0.005,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // What was in the tuft of grass.
    BlockDef {
        id: BLOCK_GRUB,
        name: "grub",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_GRUB),
        leaves_behind: None,
        weight: 0.005,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Feather, cord and a hook's worth of patience. Not eaten off the line
    // (`fishing::Bait::keeps`), which is the whole of what it buys.
    BlockDef {
        id: BLOCK_FISHING_FLY,
        name: "fishing_fly",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FISHING_FLY),
        leaves_behind: None,
        weight: 0.01,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Knocked off many rusty stones; what the bloomery eats without ore.
    BlockDef {
        id: BLOCK_IRON_DUST,
        name: "iron_dust",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_IRON_DUST),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // What every perishable becomes. See `food`.
    BlockDef {
        id: BLOCK_ROTTEN,
        name: "rotten",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ROTTEN),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A pat of dung. See `types::BLOCK_DUNG` and `comfort`.
    BlockDef {
        id: BLOCK_DUNG,
        name: "dung",
        shape: Shape::Cube,
        // An eighth: a pat on the ground, walked over rather than climbed.
        thickness: 1,
        // Loose, so a shovel clears it faster and the one clearing it gets
        // dirty the way digging does.
        matter: Matter::Loose,
        // A heap on the ground, not a wall: no light stopped, no roof made.
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing. See `types::BLOCK_DUNG` for why it is not an item.
        drop: None,
        leaves_behind: None,
        weight: 0.5,
        stack: 1,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the sea floor ----
    //
    // Five things that stand in water, two stones a reef is built of, and
    // four things a player brings up. The first five are liquid by their
    // rows and a sprite by their shape -- see `types::BLOCK_KELP`, which is
    // the argument for that, and `types::stands_in_water`, which is the
    // one question the rest of the game asks about it. Their opacity is the
    // water's (one -- see its row), so the light at the bottom of a kelp
    // forest is the light at the bottom of the sea beside it rather than a
    // shaft of shade.
    BlockDef {
        id: BLOCK_KELP,
        name: "kelp",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        // Quicker than a reed: a stem of kelp is a strap of leather-soft
        // weed, and cutting a way through a forest of it while holding
        // your breath is the whole of what the number has to allow.
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_KELP_FROND),
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        // Untinted: kelp is brown in every sea, and the climate tint is
        // the colour of *land* plants -- a forest of kelp tinted like the
        // meadow on the shore is a forest of green ribbons.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_KELP_TOP,
        name: "kelp_top",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_KELP_FROND),
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SEAGRASS,
        name: "seagrass",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // **Fibre, which is what it is**: long flat blades that twist into
        // cord as readily as a tuft of dry grass does. A coast whose
        // hinterland is sand still has the stone age's first material in
        // it, a breath down.
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SEA_FAN,
        name: "sea_fan",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // **Nothing.** A fan is a lattice of lime as thin as paper and it
        // comes apart in the hand; what a reef gives is its stone (see
        // `BLOCK_BRAIN_CORAL`). A fan that dropped an item would be the
        // third decorative item nobody has a use for, or a use invented
        // for it -- and a use invented for a pretty thing at the equator
        // is exactly the shortcut that row argues against.
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STAGHORN_CORAL,
        name: "staghorn_coral",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing, for the fan's reason.
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRAIN_CORAL,
        name: "brain_coral",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        // Softer than the limestone it becomes -- a living reef is lime
        // with water in its pores -- and behind the same pick, so a colour
        // is not a way round the stone age.
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_BRAIN_CORAL),
        leaves_behind: None,
        weight: 1.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIRE_CORAL,
        name: "fire_coral",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Flint,
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_FIRE_CORAL),
        leaves_behind: None,
        weight: 1.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: true,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SHELL,
        name: "shell",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Liquid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // **Nothing, and the shell is there anyway.** It is what tells a
        // swimmer the sand under them is a sea bed rather than a beach
        // that got wet. A shell a player could pocket would want a use,
        // and every use a shell has had -- a scraper, a bead, lime -- is
        // a job something already in the pack does.
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_KELP_FROND,
        name: "kelp_frond",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_KELP_FROND),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRIED_KELP,
        name: "dried_kelp",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_KELP),
        leaves_behind: None,
        // Half the frond: what the rack takes out of it is water.
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_RAW_FISH,
        name: "raw_fish",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RAW_FISH),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_COOKED_FISH,
        name: "cooked_fish",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_COOKED_FISH),
        leaves_behind: None,
        weight: 0.25,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SALT,
        name: "salt",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SALT),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SALTED_MEAT,
        name: "salted_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SALTED_MEAT),
        leaves_behind: None,
        weight: 0.45,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SALTED_FISH,
        name: "salted_fish",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SALTED_FISH),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRIED_FISH,
        name: "dried_fish",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_FISH),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRIED_SALTED_MEAT,
        name: "dried_salted_meat",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_SALTED_MEAT),
        leaves_behind: None,
        weight: 0.25,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_DRIED_SALTED_FISH,
        name: "dried_salted_fish",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_DRIED_SALTED_FISH),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a wound is dressed with ----
    //
    // Items and nothing else: none of the three is a thing that stands in
    // the world. See `injury::Treatment` for what each is for. Light,
    // because a player is meant to carry a few of them into a wood and a
    // dressing that weighed like a tool would be left at home.
    BlockDef {
        id: BLOCK_BANDAGE,
        name: "bandage",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_BANDAGE),
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SPLINT,
        name: "splint",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SPLINT),
        leaves_behind: None,
        weight: 0.3,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_POULTICE,
        name: "poultice",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_POULTICE),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what is slung over the shoulders (see `BLOCK_RUCKSACK`) ----
    //
    // `ONE` and no durability. Stacking is obvious -- a rucksack is a
    // thing you have on -- and the durability is the interesting
    // omission: a pack that wore out would empty ten squares onto the
    // floor in the middle of a cave, and "your pack broke" is a
    // punishment the player cannot plan around. What already takes a
    // rucksack away is dying with it, which is a thing they chose.
    //
    // 2.4 kg is deliberately noticeable. It is carried in the pack
    // before it is worn, so making one is two squares and a fifth of a
    // stack of stone's weight up front, and the ten squares it buys are
    // paid for.
    BlockDef {
        id: BLOCK_RUCKSACK,
        name: "rucksack",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RUCKSACK),
        leaves_behind: None,
        weight: 2.4,
        stack: ONE,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- fires in the ground (see `pit`) ----
    BlockDef {
        id: BLOCK_BRICK_RAW,
        name: "brick_raw",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        // Wet clay is heavier than the brick it fires into.
        weight: 2.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **The four stages of a pit kiln.** Built by use, never placed or
    // carried, and they drop nothing of their own: what comes out of a
    // broken kiln is what went in -- the pottery, the fibre and the logs --
    // and the server spills exactly that (`logic::pits`). Drawn by
    // `mesh::pit_kiln_block`, so the cube these rows describe is only ever
    // collided with: a quarter of a cell of pots, half of fibre, and the
    // logs up to the rim.
    BlockDef {
        id: BLOCK_PIT_KILN,
        name: "pit_kiln",
        shape: Shape::Cube,
        thickness: QUARTER_BLOCK,
        matter: Matter::Solid,
        // Mostly air, for the campfire's reason: a quarter of a cell must
        // not black out a whole one.
        opacity: 1,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 2.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PIT_KILN_FIBRE,
        name: "pit_kiln_fibre",
        shape: Shape::Cube,
        thickness: HALF_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 2.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PIT_KILN_LOGS,
        name: "pit_kiln_logs",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Logs with gaps between them: light comes through a stack the
        // way it comes through a rack.
        opacity: 2,
        emission: 0,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 8.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PIT_KILN_LIT,
        name: "pit_kiln_lit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 2,
        // **Brighter than a campfire**: sixteen armfuls of fuel burning in
        // a hole throw more light than a ring of sticks, and a kiln is a
        // thing seen across a valley at night -- which is how anybody on a
        // server knows somebody is firing pots.
        emission: 14,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 8.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **A pile of logs and what it burns into.** Drawn as the logs in it
    // (`mesh::log_pile_block`) and collided as them (`pit::pile_log_boxes`):
    // it was a cube of log ends, which read as a strange block rather than
    // a woodpile ("у дровницы непонятная текстура").
    BlockDef {
        id: BLOCK_LOG_PILE,
        name: "log_pile",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        // Round logs with gaps between them, as the kiln's logs are: not
        // fifteen, which would make the pile opaque (`types::is_opaque`) --
        // and an opaque cell hides the faces of everything beside it, so the
        // grass round one log would be holes into the ground.
        opacity: 2,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing by the table: the server gives back the logs that went
        // in, oak or birch, rather than a count of one kind.
        drop: None,
        leaves_behind: None,
        weight: 7.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_LOG_PILE_LIT,
        name: "log_pile_lit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        // A smoulder, not a blaze: a pile burns without air, and what shows
        // is the glow on its one open face in the moments before it is
        // covered.
        emission: 8,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Broken open while it burns, it has nothing left to give: the
        // wood is half fire.
        drop: None,
        leaves_behind: None,
        weight: 7.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CHARCOAL_PILE,
        name: "charcoal_pile",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // As many lumps as the heap holds: see `types::block_drop_count`.
        drop: Some(BLOCK_COAL),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **The firepit**, on the campfire's rows except the drop: see
    // `types::BLOCK_FIREPIT`.
    BlockDef {
        id: BLOCK_FIREPIT,
        name: "firepit",
        shape: Shape::Cube,
        thickness: QUARTER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIREPIT_LIT,
        name: "firepit_lit",
        shape: Shape::Cube,
        thickness: QUARTER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 13,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 1.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ---- the wild plants ----
    //
    // Four tall, four low, and the sundew. See `types::BLOCK_FIREWEED` for
    // what each is for. Every one is `Work::Plant` and `Tier::Hand`, for the
    // berry bush's reason: a stone age that gates a plant behind a tool is a
    // stone age that starves before it teaches. Crosses with no opacity, as
    // every plant is. Both halves of a tall plant are one row and give the
    // plant's drop -- the half that is not broken goes with it and gives
    // nothing (`types::plant_partner`), the bed's bargain.
    BlockDef {
        id: BLOCK_FIREWEED,
        name: "fireweed",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        // Not placeable, and none of the tall plants is: what a player
        // carries off one is a stalk or a root, never the plant.
        placeable: false,
        // Its own colour: a spike tinted straw in a dry summer would lose the
        // pink that is the whole of how it is seen across a clearing.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CATTAIL,
        name: "cattail",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.35),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // The rhizome: a root, roasted as any root is, with a strand off the
        // leaves beside it (`types::also_drops`).
        drop: Some(BLOCK_ROOT),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Giant reed: the cattail's row, a little tougher to break -- a cane is
    // wood-hard where a cattail is leaf -- and giving cane. See
    // `types::BLOCK_ARUNDO`.
    BlockDef {
        id: BLOCK_ARUNDO,
        name: "arundo",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_CANE),
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // A length of cane, carried like a stick and never set down: a stick
    // lying in the grass is a thing the world has, and cane is a thing
    // a player cut.
    BlockDef {
        id: BLOCK_CANE,
        name: "cane",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_CANE),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_NETTLE,
        name: "nettle",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.35),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // Two of them (`types::block_drop_count`).
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        // **A nettle bed is slow going**, which is the second half of what
        // makes one a thing to go round: it stings the hand that pulls it and
        // it holds the legs that wade it.
        drag: 0.75,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BRACKEN,
        name: "bracken",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIBER),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BILBERRY,
        name: "bilberry",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.3),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BERRIES),
        // Picked, not pulled up, as the bush is.
        leaves_behind: Some(BLOCK_BILBERRY_BARE),
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        // The blue of the berries is what a player looks for on a dark floor.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BILBERRY_BARE,
        name: "bilberry_bare",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // A picked sprig pulled up gives nothing: the berries were the plant's
        // worth, and a shrub that could be carried off and set down in a
        // garden would be a larder in a pack.
        drop: None,
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STRAWBERRY,
        name: "strawberry",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.25),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_BERRIES),
        leaves_behind: Some(BLOCK_STRAWBERRY_BARE),
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_STRAWBERRY_BARE,
        name: "strawberry_bare",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.15,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PLANTAIN,
        name: "plantain",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // Itself: the leaves are the medicine (`crafting`, "plantain poultice").
        drop: Some(BLOCK_PLANTAIN),
        leaves_behind: None,
        weight: 0.03,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FERN,
        name: "fern",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        // Nothing. See `types::BLOCK_FERN`.
        drop: None,
        leaves_behind: None,
        weight: 0.05,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.9,
        grip: 1.0,
        placeable: false,
        foliage: true,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SUNDEW,
        name: "sundew",
        shape: Shape::Cross,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.15),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_SUNDEW),
        leaves_behind: None,
        weight: 0.02,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        // Red is what a sundew is, and what makes it findable at all.
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the fir and the saxaul ----
    //
    // Each wood's four rows are the oak's four rows with its own names on
    // them: a log, a crown, boards and pegged boards (see `wood`). What
    // differs is what a player sees and one thing a player does -- a saxaul
    // sinks (`types::density`) -- and nothing a player would have to learn
    // twice. Both crowns are untinted, for the reasons at their ids.
    BlockDef {
        id: BLOCK_FIR_LOG,
        name: "fir_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_FIR_LOG),
        leaves_behind: None,
        weight: 0.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIR_NEEDLES,
        name: "fir_needles",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_FIR_NEEDLES),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_FIR_PLANKS,
        name: "fir_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FIR_PLANKS),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEGGED_FIR_PLANKS,
        name: "pegged_fir_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_FIR_PLANKS),
        leaves_behind: None,
        weight: 0.65,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SAXAUL_LOG,
        name: "saxaul_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_SAXAUL_LOG),
        leaves_behind: None,
        weight: 1.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SAXAUL_LEAVES,
        name: "saxaul_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_SAXAUL_PLANKS,
        name: "saxaul_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SAXAUL_PLANKS),
        leaves_behind: None,
        weight: 0.9,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEGGED_SAXAUL_PLANKS,
        name: "pegged_saxaul_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_SAXAUL_PLANKS),
        leaves_behind: None,
        weight: 0.95,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- what a death leaves ----
    //
    // The body, and the bones it rots into. Both are the backpack's row
    // with a different picture on it, because the backpack's row was
    // right about everything except what the thing *was*: a container
    // nobody may place, that drops no block of its own, and that is soft
    // enough to open in a hurry. See `types::BLOCK_CORPSE`.
    BlockDef {
        id: BLOCK_CORPSE,
        name: "corpse",
        shape: Shape::Cube,
        thickness: HALF_BLOCK,
        matter: Matter::Solid,
        // Half a cell, so it cannot black one out -- the campfire's
        // argument, and the symptom was a dark square on the ground
        // under every bag left standing.
        opacity: 1,
        emission: 0,
        // The bag's fifth of a second, and for the bag's reason: a grave
        // you have to quarry, in the place that just killed you, is a
        // second death -- and anything slower than a grab is a job.
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // ...and two days later. Lower than the body was -- a skeleton is
    // the two eighths an animal's is (`BLOCK_BONES`) -- and otherwise
    // identical: everything still in it is still got at the same way.
    BlockDef {
        id: BLOCK_REMAINS,
        name: "remains",
        shape: Shape::Cube,
        thickness: 2,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.2),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // **No bone.** An animal's skeleton gives its bones up
        // (`BLOCK_BONES`), and a player's would be a reason to hope
        // somebody died -- and, on a server of four friends, a reason
        // to arrange it. What comes out of this is what was in it.
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: true,
        turns: false,
        propped: false,
    },
    // **A thing set down by hand** (`types::BLOCK_SET_DOWN` for the argument
    // about where it is kept). An eighth of a cell, which is what the ray
    // stops at and the cracks are drawn on; walked through all the same
    // (`types::is_collidable`). Drawn as nothing by the mesher and as the
    // thing in it by the frame, lying down.
    BlockDef {
        id: BLOCK_SET_DOWN,
        name: "set_down",
        shape: Shape::Cube,
        thickness: 1,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        // A grab. Taking a knife off a stone is not a job.
        hardness: Some(0.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        // Nothing of its own: what drops is what was in it, spilled like a
        // chest's. A row that dropped itself would be a free container.
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        // Which way the player looked, for the thing drawn in it. The
        // mesher draws the cell as nothing, so the turn shows only in the
        // frame (`logic::entities::build_set_down_into`).
        faces: true,
        falls: false,
        container: true,
        turns: false,
        // A floor dug out from under a knife drops the knife.
        propped: true,
    },
    // ---- fire that got loose (`wildfire`) ----
    //
    // **Alight, a trunk gives light and nothing else.** Twelve, a step under
    // a campfire: a burning wall is a lot of fire and not a lamp. Broken
    // while it burns it gives nothing -- what was burning is gone -- and it
    // is quick to break because what is left of it is char, which is how a
    // fire is fought here: tear the burning wall down before the roof goes.
    BlockDef {
        id: BLOCK_BURNING_LOG,
        name: "burning_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 12,
        hardness: Some(2.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Wood,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_BURNING_PLANKS,
        name: "burning_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 12,
        hardness: Some(1.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // **Char breaks by hand, into ash.** Not charcoal: a trunk that burnt in
    // the open is burnt *through*, and charcoal is what wood becomes with
    // the air kept off it -- the pit's whole argument (`pit::charcoal_from`).
    // A burnt house that paid a lump a log would be the best charcoal pit in
    // the game and the cheapest. Ash is worth having (`crafting`, and the
    // field it dresses), which is enough to make clearing the ruin worth it.
    BlockDef {
        id: BLOCK_CHARRED_LOG,
        name: "charred_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(1.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ASH),
        leaves_behind: None,
        weight: 0.5,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_CHARRED_PLANKS,
        name: "charred_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(0.8),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_ASH),
        leaves_behind: None,
        weight: 0.4,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the standing torch ----
    //
    // **A post, drawn as a post** (`mesh::standing_torch_block`), in two
    // cells. Opacity nought: a pole a sixteenth of a cell across casts no
    // shadow worth the name, and a torch whose own post blacked out the
    // cell under its flame would be the campfire's bug again. Propped,
    // because it is driven into the ground and a pole with nothing under it
    // falls over; *which* ground is `types::can_grow_on`: a whole top, so
    // not a drift or a slab. Breaking either half gives back the torch, and
    // the other half goes with it (`standing_torch_partner`).
    BlockDef {
        id: BLOCK_STANDING_TORCH,
        name: "standing_torch",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STANDING_TORCH),
        leaves_behind: None,
        weight: 0.8,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // The top, burning. **Fourteen**: brighter than a campfire, because a
    // torch is a flame held up where nothing stands between it and the
    // ground it lights, and dimmer than glowstone, which is the one light
    // in the game that is not a fire.
    BlockDef {
        id: BLOCK_STANDING_TORCH_LIT,
        name: "standing_torch_lit",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 14,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STANDING_TORCH),
        leaves_behind: None,
        weight: 0.8,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    BlockDef {
        id: BLOCK_STANDING_TORCH_OUT,
        name: "standing_torch_out",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 0,
        emission: 0,
        hardness: Some(0.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_STANDING_TORCH),
        leaves_behind: None,
        weight: 0.8,
        stack: 4,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: true,
    },
    // Snow lying on the ground. See `types::BLOCK_SNOW_COVER`.
    BlockDef {
        id: BLOCK_SNOW_COVER,
        name: "snow_cover",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 0,
        emission: 0,
        hardness: Some(0.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: None,
        leaves_behind: None,
        weight: 0.0,
        stack: 1,
        durability: None,
        drag: 0.9,
        grip: 0.8,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Fallen leaves: snow cover's row, swept up into a handful. See
    // `types::BLOCK_LEAF_LITTER`.
    BlockDef {
        id: BLOCK_LEAF_LITTER,
        name: "leaf_litter",
        shape: Shape::Flat,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Loose,
        opacity: 0,
        emission: 0,
        hardness: Some(0.1),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_LEAF_HANDFUL),
        leaves_behind: None,
        weight: 0.0,
        stack: 1,
        durability: None,
        drag: 0.9,
        grip: 0.95,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // Resin: a lump of pitch. See `types::BLOCK_RESIN`.
    BlockDef {
        id: BLOCK_RESIN,
        name: "resin",
        shape: Shape::Item,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: None,
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_RESIN),
        leaves_behind: None,
        weight: 0.1,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PINE_LOG,
        name: "pine_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_PINE_LOG),
        leaves_behind: None,
        weight: 0.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PINE_NEEDLES,
        name: "pine_needles",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_PINE_NEEDLES),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PINE_PLANKS,
        name: "pine_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PINE_PLANKS),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEGGED_PINE_PLANKS,
        name: "pegged_pine_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_PINE_PLANKS),
        leaves_behind: None,
        weight: 0.65,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WILLOW_LOG,
        name: "willow_log",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(9.0),
        felled: Some(4.5),
        needs: Tier::Flint,
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_WILLOW_LOG),
        leaves_behind: None,
        weight: 0.8,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: true,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WILLOW_LEAVES,
        name: "willow_leaves",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 1,
        emission: 0,
        hardness: Some(0.6),
        felled: None,
        needs: Tier::Hand,
        work: Work::Plant,
        tool: None,
        drop: Some(BLOCK_WILLOW_LEAVES),
        leaves_behind: None,
        weight: 0.2,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 0.35,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_WILLOW_PLANKS,
        name: "willow_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(3.5),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WILLOW_PLANKS),
        leaves_behind: None,
        weight: 0.6,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: true,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    BlockDef {
        id: BLOCK_PEGGED_WILLOW_PLANKS,
        name: "pegged_willow_planks",
        shape: Shape::Cube,
        thickness: LAYERS_PER_BLOCK,
        matter: Matter::Solid,
        opacity: 15,
        emission: 0,
        hardness: Some(5.0),
        felled: None,
        needs: Tier::Hand,
        work: Work::Any,
        tool: None,
        drop: Some(BLOCK_WILLOW_PLANKS),
        leaves_behind: None,
        weight: 0.65,
        stack: crate::inventory::MAX_STACK,
        durability: None,
        drag: 1.0,
        grip: 1.0,
        placeable: false,
        foliage: false,
        orientable: false,
        faces: false,
        falls: false,
        container: false,
        turns: false,
        propped: false,
    },
    // ---- the ground: see `ground`, and the templates under this table ----
    BlockDef { id: BLOCK_SANDSTONE_COBBLE, name: "sandstone_cobble", drop: Some(BLOCK_SANDSTONE_COBBLE), weight: 2.2, ..COBBLE_ROW },
    BlockDef { id: BLOCK_SANDSTONE_GRAVEL, name: "sandstone_gravel", drop: Some(BLOCK_SANDSTONE_GRAVEL), weight: 1.74, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_SANDSTONE_PEBBLE, name: "sandstone_pebble", drop: Some(BLOCK_SANDSTONE_PEBBLE), weight: 0.14, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_LIMESTONE_COBBLE, name: "limestone_cobble", drop: Some(BLOCK_LIMESTONE_COBBLE), weight: 2.3, ..COBBLE_ROW },
    BlockDef { id: BLOCK_LIMESTONE_GRAVEL, name: "limestone_gravel", drop: Some(BLOCK_LIMESTONE_GRAVEL), weight: 1.82, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_LIMESTONE_SAND, name: "limestone_sand", drop: Some(BLOCK_LIMESTONE_SAND), weight: 1.53, ..SAND_ROW },
    BlockDef { id: BLOCK_LIMESTONE_PEBBLE, name: "limestone_pebble", drop: Some(BLOCK_LIMESTONE_PEBBLE), weight: 0.14, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_GRANITE_COBBLE, name: "granite_cobble", drop: Some(BLOCK_GRANITE_COBBLE), weight: 2.6, ..COBBLE_ROW },
    BlockDef { id: BLOCK_GRANITE_GRAVEL, name: "granite_gravel", drop: Some(BLOCK_GRANITE_GRAVEL), weight: 2.06, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_GRANITE_SAND, name: "granite_sand", drop: Some(BLOCK_GRANITE_SAND), weight: 1.73, ..SAND_ROW },
    BlockDef { id: BLOCK_GRANITE_PEBBLE, name: "granite_pebble", drop: Some(BLOCK_GRANITE_PEBBLE), weight: 0.16, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_BASALT_COBBLE, name: "basalt_cobble", drop: Some(BLOCK_BASALT_COBBLE), weight: 3.0, ..COBBLE_ROW },
    BlockDef { id: BLOCK_BASALT_GRAVEL, name: "basalt_gravel", drop: Some(BLOCK_BASALT_GRAVEL), weight: 2.38, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_BASALT_SAND, name: "basalt_sand", drop: Some(BLOCK_BASALT_SAND), weight: 2.0, ..SAND_ROW },
    BlockDef { id: BLOCK_BASALT_PEBBLE, name: "basalt_pebble", drop: Some(BLOCK_BASALT_PEBBLE), weight: 0.19, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_SHALE, name: "shale", hardness: Some(3.5), needs: Tier::Flint, drop: Some(BLOCK_SHALE), weight: 2.1, ..ROCK_ROW },
    BlockDef { id: BLOCK_SHALE_COBBLE, name: "shale_cobble", drop: Some(BLOCK_SHALE_COBBLE), weight: 2.1, ..COBBLE_ROW },
    BlockDef { id: BLOCK_SHALE_GRAVEL, name: "shale_gravel", drop: Some(BLOCK_SHALE_GRAVEL), weight: 1.66, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_SHALE_SAND, name: "shale_sand", drop: Some(BLOCK_SHALE_SAND), weight: 1.4, ..SAND_ROW },
    BlockDef { id: BLOCK_SHALE_PEBBLE, name: "shale_pebble", drop: Some(BLOCK_SHALE_PEBBLE), weight: 0.13, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_CHALK, name: "chalk", hardness: Some(2.5), needs: Tier::Flint, drop: Some(BLOCK_CHALK), weight: 1.9, ..ROCK_ROW },
    BlockDef { id: BLOCK_CHALK_COBBLE, name: "chalk_cobble", drop: Some(BLOCK_CHALK_COBBLE), weight: 1.9, ..COBBLE_ROW },
    BlockDef { id: BLOCK_CHALK_GRAVEL, name: "chalk_gravel", drop: Some(BLOCK_CHALK_GRAVEL), weight: 1.5, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_CHALK_SAND, name: "chalk_sand", drop: Some(BLOCK_CHALK_SAND), weight: 1.27, ..SAND_ROW },
    BlockDef { id: BLOCK_CHALK_PEBBLE, name: "chalk_pebble", drop: Some(BLOCK_CHALK_PEBBLE), weight: 0.12, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_DOLOMITE, name: "dolomite", hardness: Some(6.0), needs: Tier::Flint, drop: Some(BLOCK_DOLOMITE), weight: 2.4, ..ROCK_ROW },
    BlockDef { id: BLOCK_DOLOMITE_COBBLE, name: "dolomite_cobble", drop: Some(BLOCK_DOLOMITE_COBBLE), weight: 2.4, ..COBBLE_ROW },
    BlockDef { id: BLOCK_DOLOMITE_GRAVEL, name: "dolomite_gravel", drop: Some(BLOCK_DOLOMITE_GRAVEL), weight: 1.9, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_DOLOMITE_SAND, name: "dolomite_sand", drop: Some(BLOCK_DOLOMITE_SAND), weight: 1.6, ..SAND_ROW },
    BlockDef { id: BLOCK_DOLOMITE_PEBBLE, name: "dolomite_pebble", drop: Some(BLOCK_DOLOMITE_PEBBLE), weight: 0.15, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_MARBLE, name: "marble", hardness: Some(7.0), needs: Tier::Flint, drop: Some(BLOCK_MARBLE), weight: 2.6, ..ROCK_ROW },
    BlockDef { id: BLOCK_MARBLE_COBBLE, name: "marble_cobble", drop: Some(BLOCK_MARBLE_COBBLE), weight: 2.6, ..COBBLE_ROW },
    BlockDef { id: BLOCK_MARBLE_GRAVEL, name: "marble_gravel", drop: Some(BLOCK_MARBLE_GRAVEL), weight: 2.06, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_MARBLE_SAND, name: "marble_sand", drop: Some(BLOCK_MARBLE_SAND), weight: 1.73, ..SAND_ROW },
    BlockDef { id: BLOCK_MARBLE_PEBBLE, name: "marble_pebble", drop: Some(BLOCK_MARBLE_PEBBLE), weight: 0.16, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_QUARTZITE, name: "quartzite", hardness: Some(10.0), needs: Tier::Copper, drop: Some(BLOCK_QUARTZITE), weight: 2.6, ..ROCK_ROW },
    BlockDef { id: BLOCK_QUARTZITE_COBBLE, name: "quartzite_cobble", drop: Some(BLOCK_QUARTZITE_COBBLE), weight: 2.6, ..COBBLE_ROW },
    BlockDef { id: BLOCK_QUARTZITE_GRAVEL, name: "quartzite_gravel", drop: Some(BLOCK_QUARTZITE_GRAVEL), weight: 2.06, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_QUARTZITE_SAND, name: "quartzite_sand", drop: Some(BLOCK_QUARTZITE_SAND), weight: 1.73, ..SAND_ROW },
    BlockDef { id: BLOCK_QUARTZITE_PEBBLE, name: "quartzite_pebble", drop: Some(BLOCK_QUARTZITE_PEBBLE), weight: 0.16, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_GNEISS, name: "gneiss", hardness: Some(9.0), needs: Tier::Copper, drop: Some(BLOCK_GNEISS), weight: 2.7, ..ROCK_ROW },
    BlockDef { id: BLOCK_GNEISS_COBBLE, name: "gneiss_cobble", drop: Some(BLOCK_GNEISS_COBBLE), weight: 2.7, ..COBBLE_ROW },
    BlockDef { id: BLOCK_GNEISS_GRAVEL, name: "gneiss_gravel", drop: Some(BLOCK_GNEISS_GRAVEL), weight: 2.14, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_GNEISS_SAND, name: "gneiss_sand", drop: Some(BLOCK_GNEISS_SAND), weight: 1.8, ..SAND_ROW },
    BlockDef { id: BLOCK_GNEISS_PEBBLE, name: "gneiss_pebble", drop: Some(BLOCK_GNEISS_PEBBLE), weight: 0.17, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_DIORITE, name: "diorite", hardness: Some(10.0), needs: Tier::Copper, drop: Some(BLOCK_DIORITE), weight: 2.8, ..ROCK_ROW },
    BlockDef { id: BLOCK_DIORITE_COBBLE, name: "diorite_cobble", drop: Some(BLOCK_DIORITE_COBBLE), weight: 2.8, ..COBBLE_ROW },
    BlockDef { id: BLOCK_DIORITE_GRAVEL, name: "diorite_gravel", drop: Some(BLOCK_DIORITE_GRAVEL), weight: 2.22, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_DIORITE_SAND, name: "diorite_sand", drop: Some(BLOCK_DIORITE_SAND), weight: 1.87, ..SAND_ROW },
    BlockDef { id: BLOCK_DIORITE_PEBBLE, name: "diorite_pebble", drop: Some(BLOCK_DIORITE_PEBBLE), weight: 0.17, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_GABBRO, name: "gabbro", hardness: Some(13.0), needs: Tier::Bronze, drop: Some(BLOCK_GABBRO), weight: 3.0, ..ROCK_ROW },
    BlockDef { id: BLOCK_GABBRO_COBBLE, name: "gabbro_cobble", drop: Some(BLOCK_GABBRO_COBBLE), weight: 3.0, ..COBBLE_ROW },
    BlockDef { id: BLOCK_GABBRO_GRAVEL, name: "gabbro_gravel", drop: Some(BLOCK_GABBRO_GRAVEL), weight: 2.38, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_GABBRO_SAND, name: "gabbro_sand", drop: Some(BLOCK_GABBRO_SAND), weight: 2.0, ..SAND_ROW },
    BlockDef { id: BLOCK_GABBRO_PEBBLE, name: "gabbro_pebble", drop: Some(BLOCK_GABBRO_PEBBLE), weight: 0.19, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_ANDESITE, name: "andesite", hardness: Some(8.0), needs: Tier::Copper, drop: Some(BLOCK_ANDESITE), weight: 2.6, ..ROCK_ROW },
    BlockDef { id: BLOCK_ANDESITE_COBBLE, name: "andesite_cobble", drop: Some(BLOCK_ANDESITE_COBBLE), weight: 2.6, ..COBBLE_ROW },
    BlockDef { id: BLOCK_ANDESITE_GRAVEL, name: "andesite_gravel", drop: Some(BLOCK_ANDESITE_GRAVEL), weight: 2.06, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_ANDESITE_SAND, name: "andesite_sand", drop: Some(BLOCK_ANDESITE_SAND), weight: 1.73, ..SAND_ROW },
    BlockDef { id: BLOCK_ANDESITE_PEBBLE, name: "andesite_pebble", drop: Some(BLOCK_ANDESITE_PEBBLE), weight: 0.16, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_TUFF, name: "tuff", hardness: Some(2.5), needs: Tier::Flint, drop: Some(BLOCK_TUFF), weight: 1.4, ..ROCK_ROW },
    BlockDef { id: BLOCK_TUFF_COBBLE, name: "tuff_cobble", drop: Some(BLOCK_TUFF_COBBLE), weight: 1.4, ..COBBLE_ROW },
    BlockDef { id: BLOCK_TUFF_GRAVEL, name: "tuff_gravel", drop: Some(BLOCK_TUFF_GRAVEL), weight: 1.11, ..GRAVEL_ROW },
    BlockDef { id: BLOCK_TUFF_SAND, name: "tuff_sand", drop: Some(BLOCK_TUFF_SAND), weight: 0.93, ..SAND_ROW },
    BlockDef { id: BLOCK_TUFF_PEBBLE, name: "tuff_pebble", drop: Some(BLOCK_TUFF_PEBBLE), weight: 0.09, ..PEBBLE_ROW },
    BlockDef { id: BLOCK_LOAM, name: "loam", hardness: Some(1.5), falls: true, drag: 1.0, drop: Some(BLOCK_LOAM), weight: 1.3, ..SOIL_ROW },
    BlockDef { id: BLOCK_CHERNOZEM, name: "chernozem", hardness: Some(1.5), falls: true, drag: 1.0, drop: Some(BLOCK_CHERNOZEM), weight: 1.2, ..SOIL_ROW },
    BlockDef { id: BLOCK_PODZOL, name: "podzol", hardness: Some(1.4), falls: true, drag: 1.0, drop: Some(BLOCK_PODZOL), weight: 1.3, ..SOIL_ROW },
    BlockDef { id: BLOCK_LATERITE, name: "laterite", hardness: Some(3.0), falls: false, drag: 1.0, drop: Some(BLOCK_LATERITE), weight: 1.7, ..SOIL_ROW },
    BlockDef { id: BLOCK_SOLONCHAK, name: "solonchak", hardness: Some(1.6), falls: true, drag: 1.0, drop: Some(BLOCK_SOLONCHAK), weight: 1.5, ..SOIL_ROW },
    BlockDef { id: BLOCK_LOESS, name: "loess", hardness: Some(1.2), falls: true, drag: 1.0, drop: Some(BLOCK_LOESS), weight: 1.1, ..SOIL_ROW },
    BlockDef { id: BLOCK_GLEY, name: "gley", hardness: Some(1.8), falls: true, drag: 0.8, drop: Some(BLOCK_GLEY), weight: 1.6, ..SOIL_ROW },
    // **Laid at a random quarter turn** (`types::texture_turns`): rendzina is
    // chalk rubble in dark earth, and its picture has no up, so a hillside of
    // it repeating on a grid is the lattice that rule was written against.
    BlockDef { id: BLOCK_RENDZINA, name: "rendzina", hardness: Some(1.7), falls: true, drag: 1.0, drop: Some(BLOCK_RENDZINA), weight: 1.4, turns: true, ..SOIL_ROW },
    BlockDef { id: BLOCK_ANDOSOL, name: "andosol", hardness: Some(1.3), falls: true, drag: 1.0, drop: Some(BLOCK_ANDOSOL), weight: 0.9, ..SOIL_ROW },
    BlockDef { id: BLOCK_PERMAFROST, name: "permafrost", hardness: Some(5.0), falls: false, drag: 1.0, drop: Some(BLOCK_PERMAFROST), weight: 1.6, ..SOIL_ROW },
    BlockDef { id: BLOCK_FEATHER_GRASS, name: "feather_grass", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_SEDGE, name: "sedge", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_COTTON_GRASS, name: "cotton_grass", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_FESCUE, name: "fescue", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_MARRAM, name: "marram", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_ELEPHANT_GRASS, name: "elephant_grass", drop: Some(crate::types::BLOCK_CANE), ..GRASS_ROW },
    BlockDef { id: BLOCK_BLUEGRASS, name: "bluegrass", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_TIMOTHY, name: "timothy", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_TUSSOCK_GRASS, name: "tussock_grass", drop: Some(BLOCK_FIBER), ..GRASS_ROW },
    BlockDef { id: BLOCK_SPINIFEX, name: "spinifex", drop: Some(crate::types::BLOCK_RESIN), ..GRASS_ROW },
    BlockDef { id: BLOCK_FIR_TWIG, name: "fir_twig", ..TWIG_ROW },
    BlockDef { id: BLOCK_FIR_BOUGH, name: "fir_bough", drop: Some(BLOCK_FIR_LOG), ..BOUGH_ROW },
    BlockDef { id: BLOCK_SAXAUL_TWIG, name: "saxaul_twig", ..TWIG_ROW },
    BlockDef { id: BLOCK_SAXAUL_BOUGH, name: "saxaul_bough", drop: Some(BLOCK_SAXAUL_LOG), ..BOUGH_ROW },
    BlockDef { id: BLOCK_PINE_TWIG, name: "pine_twig", ..TWIG_ROW },
    BlockDef { id: BLOCK_PINE_BOUGH, name: "pine_bough", drop: Some(BLOCK_PINE_LOG), ..BOUGH_ROW },
    BlockDef { id: BLOCK_WILLOW_TWIG, name: "willow_twig", ..TWIG_ROW },
    BlockDef { id: BLOCK_WILLOW_BOUGH, name: "willow_bough", drop: Some(BLOCK_WILLOW_LOG), ..BOUGH_ROW },
    BlockDef { id: BLOCK_MOSS, name: "moss", drop: Some(BLOCK_MOSS), weight: 0.03, ..MOSS_ROW },
    // ---- steps and roofs (`types::BLOCK_TILE_ROOF`) ----
    //
    // **A wall's opacity, all of them**, though none fills its cell: a roof
    // that let the sky through its own cell would light the room under it
    // like the meadow, and `types::blocks_the_sky` -- the rain, the night
    // chill, the smoke's ceiling -- reads the same number. `is_opaque` still
    // says no, because the shape is not a whole cube, so nothing beside one
    // is culled against it.
    //
    // Each gives itself back, on the rule `PLACEABLE_BLOCKS` keeps: a roof a
    // player cannot take down again is a mistake the palette does not allow.
    // **A pot of earth**: the fired crucible with soil in it, and a floor for
    // the small plants a pot holds (`types::grows_in_a_pot`). A whole cell
    // tall, so what grows in it stands on its rim rather than a finger above
    // it -- a pot drawn shorter than its cell is a plant hanging in the air,
    // which is the one thing a player would call broken.
    BlockDef {
        id: BLOCK_PLANTER,
        name: "planter",
        shape: Shape::Cube,
        opacity: 15,
        hardness: Some(0.6),
        work: Work::Stone,
        tool: None,
        drop: Some(BLOCK_PLANTER),
        weight: 3.0,
        placeable: true,
        ..STAKE_ROW
    },
    // **A pit prop**: a post of timber half a cell across. A cube by its row,
    // so it holds a roof and a body walks into it, and drawn and collided as
    // the post it is (`geometry::block_box`, `mesh::prop_block`).
    BlockDef {
        id: BLOCK_PROP,
        name: "prop",
        shape: Shape::Cube,
        opacity: 0,
        hardness: Some(0.8),
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_PROP),
        weight: 2.0,
        faces: true,
        placeable: true,
        ..STAKE_ROW
    },
    // **A sharpened pole**: stood on the ground or driven into a wall, and
    // held by whichever it is (`types::support_at`). Cross-shaped, so it is
    // walked through rather than into -- a fence of them is a thing to look
    // at and to hang a skin on, not a wall, and a stake that stopped a
    // player like stone would be a palisade nobody voted for. Wood, so it
    // burns with the wall it is driven into (`wildfire::fuel` reads the row).
    BlockDef {
        id: BLOCK_STAKE,
        name: "stake",
        shape: Shape::Cross,
        hardness: Some(0.35),
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_STAKE),
        weight: 0.4,
        faces: true,
        propped: true,
        placeable: true,
        ..STAKE_ROW
    },
    // **Slats crossed in a window frame**: light and air through it, and
    // nothing bigger than that. Opaque enough to shade a room a little and
    // nowhere near a wall's fifteen, so a hut with one is lit.
    BlockDef {
        id: BLOCK_WINDOW_LATTICE,
        name: "window_lattice",
        shape: Shape::Cube,
        opacity: 3,
        hardness: Some(0.5),
        work: Work::Wood,
        tool: None,
        drop: Some(BLOCK_WINDOW_LATTICE),
        weight: 0.8,
        // A panel, so it turns to face whoever sets it (`types::lattice_box`).
        faces: true,
        placeable: true,
        ..STAKE_ROW
    },
    BlockDef { id: BLOCK_PLANK_STAIRS, name: "plank_stairs", drop: Some(BLOCK_PLANK_STAIRS), work: Work::Wood, weight: 0.5, faces: true, ..STEP_ROW },
    BlockDef { id: BLOCK_COBBLESTONE_STAIRS, name: "cobblestone_stairs", drop: Some(BLOCK_COBBLESTONE_STAIRS), weight: 1.8, faces: true, ..STEP_ROW },
    BlockDef { id: BLOCK_TILE_ROOF, name: "tile_roof", drop: Some(BLOCK_TILE_ROOF), hardness: Some(2.5), work: Work::Stone, weight: 1.2, faces: true, ..STEP_ROW },
    BlockDef { id: BLOCK_TILE_SLAB, name: "tile_slab", drop: Some(BLOCK_TILE_SLAB), hardness: Some(2.0), work: Work::Stone, weight: 0.8, thickness: 4, ..STEP_ROW },
    // Thatch and branches come apart in the hands in a second: they are
    // tied, not joined.
    BlockDef { id: BLOCK_THATCH_ROOF, name: "thatch_roof", drop: Some(BLOCK_THATCH_ROOF), hardness: Some(0.8), work: Work::Plant, weight: 0.3, faces: true, ..STEP_ROW },
    BlockDef { id: BLOCK_THATCH_SLAB, name: "thatch_slab", drop: Some(BLOCK_THATCH_SLAB), hardness: Some(0.6), work: Work::Plant, weight: 0.2, thickness: 4, ..STEP_ROW },
    BlockDef { id: BLOCK_BRANCH_ROOF, name: "branch_roof", drop: Some(BLOCK_BRANCH_ROOF), hardness: Some(0.7), work: Work::Wood, weight: 0.3, faces: true, ..STEP_ROW },
    BlockDef { id: BLOCK_BRANCH_SLAB, name: "branch_slab", drop: Some(BLOCK_BRANCH_SLAB), hardness: Some(0.5), work: Work::Wood, weight: 0.2, thickness: 4, ..STEP_ROW },
];

/// The row every step and roofing slab starts from: cobble's numbers, which
/// are the plank's too (three and a half seconds by hand, `Work::Any`).
const STEP_ROW: BlockDef = BlockDef { weight: 1.2, ..COBBLE_ROW };

/// The row a stake and a window lattice are written against: sticks, worked
/// as wood, broken by hand, and nothing in them that a fire or a tool reads.
const STAKE_ROW: BlockDef =
    BlockDef { shape: Shape::Cross, opacity: 0, needs: Tier::Hand, work: Work::Wood, ..COBBLE_ROW };

// ---- the ground's rows, by template ----
//
// **Every rock is one row of numbers and every piece of rubble is its common
// kind's row with its own id**, written as a struct update on the templates
// below rather than as seventy copies of twenty-seven fields. The rows they
// copy (limestone, cobblestone, gravel, sand, a pebble, sandy soil, dry
// grass, a twig, a bough, fibre) are the numbers these blocks *stand in for*
// (`ground`), and a field changed on one of those without the template is
// what `the_ground_keeps_the_numbers_of_what_it_stands_in_for` catches.
//
// Rejected: copying the rows out in full. Seventy rows of which one had a
// stale `falls` would be the granite sand that hangs in the air.
const ROCK_ROW: BlockDef = BlockDef {
    id: 0,
    name: "",
    shape: Shape::Cube,
    thickness: LAYERS_PER_BLOCK,
    matter: Matter::Solid,
    opacity: 15,
    emission: 0,
    hardness: Some(5.5),
    felled: None,
    needs: Tier::Flint,
    work: Work::Stone,
    tool: None,
    drop: None,
    leaves_behind: None,
    weight: 2.3,
    stack: crate::inventory::MAX_STACK,
    durability: None,
    drag: 1.0,
    grip: 1.0,
    placeable: true,
    foliage: false,
    orientable: false,
    faces: false,
    falls: false,
    container: false,
    turns: false,
    propped: false,
};
const COBBLE_ROW: BlockDef = BlockDef { hardness: Some(3.5), needs: Tier::Hand, work: Work::Any, weight: 2.4, ..ROCK_ROW };
const GRAVEL_ROW: BlockDef =
    BlockDef { matter: Matter::Loose, hardness: Some(1.7), drag: 0.9, falls: true, weight: 1.9, ..COBBLE_ROW };
const SAND_ROW: BlockDef = BlockDef { hardness: Some(1.5), drag: 0.95, weight: 1.6, ..GRAVEL_ROW };
const PEBBLE_ROW: BlockDef =
    BlockDef { shape: Shape::Flat, opacity: 0, hardness: Some(0.25), weight: 0.15, ..COBBLE_ROW };
const SOIL_ROW: BlockDef = BlockDef { matter: Matter::Loose, hardness: Some(1.3), weight: 1.4, falls: true, ..COBBLE_ROW };
const GRASS_ROW: BlockDef =
    BlockDef { shape: Shape::Cross, opacity: 0, hardness: Some(0.25), work: Work::Plant, weight: 0.05, ..COBBLE_ROW };
const TWIG_ROW: BlockDef = BlockDef {
    opacity: 0,
    hardness: Some(0.5),
    work: Work::Wood,
    drop: Some(BLOCK_STICK),
    weight: 0.3,
    placeable: false,
    ..COBBLE_ROW
};
const BOUGH_ROW: BlockDef = BlockDef { hardness: Some(9.0), needs: Tier::Flint, weight: 0.9, ..TWIG_ROW };
const MOSS_ROW: BlockDef =
    BlockDef { shape: Shape::Item, hardness: None, placeable: false, ..COBBLE_ROW };

/// The row for a block, by kind. Falls back to a placeholder rather than
/// panicking: ids arrive over a socket.
///
/// A linear scan of a two-dozen-entry table, in a function small enough
/// to inline. The whole table is a couple of cache lines and the loop is
/// perfectly predicted; a lookup array built at startup would be a
/// second source of truth for nothing.
#[inline]
pub fn definition(id: BlockId) -> &'static BlockDef {
    let kind = block_kind(id) as usize;
    if kind >= INDEX.len() {
        return &UNKNOWN;
    }
    let row = INDEX[kind] as usize;
    if row >= BLOCKS.len() {
        return &UNKNOWN;
    }
    &BLOCKS[row]
}

/// How many kinds the index covers. Comfortably past the last id in the
/// table, and small enough that the table is two kilobytes of `.rodata`
/// rather than something to think about.
///
/// The assertion under it is what keeps this honest: adding a block with
/// an id past the end would otherwise make that block silently unknown
/// everywhere, which is the exact class of bug the one-row-per-block
/// table exists to abolish.
// 1024 since the ground's ids (`types::BLOCK_SHALE` and on) went past 512.
const INDEX_KINDS: usize = 1024;

/// Block kind to row in `BLOCKS`, worked out at compile time.
///
/// **This replaced a linear scan, and the scan was on every hot path in
/// the game.** `definition` is what the mesher asks per face, the
/// lighting flood-fill per cell, the collider per cell of the player's
/// box and the physics per step -- millions of calls a second -- and
/// each one walked the whole table comparing ids until it found the
/// right row. That is fine at a dozen blocks and quietly became a
/// hundred-comparison average as the table grew, with the *newest*
/// blocks the slowest, which is backwards.
///
/// A `u16` per kind, filled in at compile time by the loop below, turns
/// it into two array reads. `u16::MAX` is "no such block", which
/// `definition` resolves to `UNKNOWN` exactly as the scan's fall-through
/// did.
const INDEX: [u16; INDEX_KINDS] = build_index();

const fn build_index() -> [u16; INDEX_KINDS] {
    let mut table = [u16::MAX; INDEX_KINDS];
    let mut i = 0;
    while i < BLOCKS.len() {
        let id = BLOCKS[i].id as usize;
        // A row whose id is past the end of the index would be
        // unreachable, which the assertion below turns into a build
        // failure rather than a block that half exists.
        if id < INDEX_KINDS {
            table[id] = i as u16;
        }
        i += 1;
    }
    table
}

/// Every row is reachable through the index. A compile-time check, so a
/// block added with an id past `INDEX_KINDS` fails the build rather than
/// becoming invisible.
const _: () = {
    let mut i = 0;
    while i < BLOCKS.len() {
        assert!(
            (BLOCKS[i].id as usize) < INDEX_KINDS,
            "a block id past INDEX_KINDS would be unreachable in `definition`"
        );
        i += 1;
    }
};

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
        // Ids arrive over a socket, so this is reachable from outside. A kind
        // no row has: the ten bits of a kind all set. (60 000 was this once,
        // and is a fir twig with bits the wood field owns since kinds are ten
        // bits -- an id `types::is_known_block` refuses, which is where a
        // socket's ids are judged.)
        let unknown = crate::types::KIND_MASK;
        let nonsense = definition(unknown);
        assert_eq!(nonsense.name, "?");
        assert_eq!(nonsense.hardness, None);
        assert!(!nonsense.placeable);
        assert!(!is_defined(unknown));
        assert!(!crate::types::is_known_block(60_000));
    }

    #[test]
    fn a_row_is_self_consistent() {
        for block in BLOCKS {
            assert!(block.opacity <= 15, "{}: opacity is 0..15", block.name);
            assert!(block.emission <= 15, "{}: emission is 0..15", block.name);
            assert!(block.weight >= 0.0, "{}: negative weight", block.name);
            assert!(block.drag > 0.0, "{}: drag stops you dead", block.name);
            // Grip scales friction *and* acceleration, so zero is a
            // surface a player can neither speed up nor slow down on --
            // which is not ice, it is a conveyor belt with no way off.
            // Above one is the other end: a floor that stops you faster
            // than stopping.
            assert!(
                block.grip > 0.0 && block.grip <= 1.0,
                "{}: grip is 0..1, exclusive of 0",
                block.name
            );
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
                // Water itself is not mined. What stands in it is: a stem
                // of kelp is liquid by its row (`types::BLOCK_KELP`) and
                // is still a plant a player cuts, and a kelp forest that
                // could not be cut would be scenery with a drop table.
                if !crate::types::stands_in_water(block.id) {
                    assert_eq!(block.hardness, None, "{}: a liquid is not mined", block.name);
                } else if crate::types::is_branch(block.id) {
                    // ...and drowned wood is wood (`types::BLOCK_DROWNED_BOUGH`):
                    // a snag's foot asks the axe a standing bough asks,
                    // whether or not a pool stands round it.
                    assert!(block.hardness.is_some(), "{}: a snag in a pool that cannot be cut", block.name);
                } else {
                    assert!(block.hardness.is_some(), "{}: a plant in the sea that cannot be cut", block.name);
                    assert_eq!(block.needs, Tier::Hand, "{}: a tool gate on something under water", block.name);
                }
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
    fn every_age_is_a_knife_an_axe_and_a_pick() {
        // The shape of the whole tool tree, in one assertion: **each
        // tier is a knife, an axe and a pick.**
        //
        // A tier missing one of the three is a tier a player cannot
        // actually live in -- a bronze age with no bronze knife means
        // going back to a flint one to cut grass, which reads as the
        // metal being unfinished rather than as a choice.
        //
        // **This used to say "and nothing else", and copper broke it.**
        // The argument for the closed set was that a fourth tool in one
        // tier is something the other ages cannot answer, which is how a
        // set quietly becomes a ladder again -- and that argument is
        // about *gates*: a fourth kind of work that only copper could
        // open would shut the other ages out of it. A shovel opens
        // nothing. Loose ground is `Work::Any` and always was; the
        // shovel is twice as quick at it and no other tool got slower
        // (see `types::break_seconds_with`). So the copper age has four
        // tools and every other age can still do everything, which is
        // the property that mattered.
        //
        // What is genuinely unfinished is that there is no bronze or
        // iron shovel, and an iron pick out-digs a copper shovel on
        // soil -- 6.0 against 2.4 doubled. The shovel is a copper-age
        // convenience until somebody draws the other two.
        let tools: Vec<&BlockDef> = BLOCKS.iter().filter(|b| b.tool.is_some()).collect();
        for tier in [Tier::Flint, Tier::Copper, Tier::Bronze, Tier::Iron] {
            let of_tier: Vec<&&BlockDef> =
                tools.iter().filter(|t| t.tool == Some(tier)).collect();
            for kind in [Work::Stone, Work::Wood, Work::Plant] {
                assert_eq!(
                    of_tier.iter().filter(|t| t.work == kind).count(),
                    1,
                    "{tier:?}: {kind:?} is not the work of exactly one tool"
                );
            }
        }
        // Twelve rungs of the ladder, the two lashed stone tools below
        // the first rung -- a `Tier::Stone` axe and pick that a knife
        // does not join, because a knife is flint and flint is knapped,
        // not lashed -- and the one tool that is not a rung of anything.
        assert_eq!(
            tools.iter().filter(|t| t.work != Work::Ground).count(),
            14,
            "four ages of three tools, and the lashed stone age of two"
        );
        assert_eq!(
            tools.iter().filter(|t| t.tool == Some(Tier::Stone)).count(),
            2,
            "the lashed stone age is an axe and a pick"
        );
        assert_eq!(
            tools.iter().filter(|t| t.work == Work::Ground).count(),
            1,
            "more than one shovel, and this test has a claim to make about it"
        );
        for tool in &tools {
            assert_ne!(tool.work, Work::Any, "{} is a tool for nothing", tool.name);
            assert_ne!(tool.tool, Some(Tier::Hand), "{} is a tier that is no tier", tool.name);
        }
    }

    #[test]
    fn a_higher_tier_is_faster_and_never_slower() {
        // What a rung is *worth*, checked rather than assumed. The
        // numbers live in `Tier::speed` and are easy to edit into an
        // ordering that says a bronze pick is worse than a copper one,
        // which no test elsewhere would notice.
        let ladder = [Tier::Hand, Tier::Stone, Tier::Flint, Tier::Copper, Tier::Bronze, Tier::Iron];
        for pair in ladder.windows(2) {
            assert!(
                pair[1].speed() > pair[0].speed(),
                "{:?} is not faster than {:?}",
                pair[1],
                pair[0]
            );
        }
    }

    #[test]
    fn a_lashed_axe_cuts_brush_and_a_wedged_one_fells_a_tree() {
        // The whole of what the wedge buys, stated against the table: a
        // stone head lashed to a haft is faster than fingers on deadfall
        // and turf and cannot touch a standing trunk or living rock;
        // drive the same head down a split haft, pin it with two pegs,
        // and the trunk comes down and the rock opens. If a lashed tool
        // ever fells a tree, the row between the two is decoration.
        use crate::types::{
            break_seconds_with, is_breakable_with, oriented, Axis, BLOCK_DIRT, BLOCK_WEDGED_AXE,
            BLOCK_WEDGED_PICKAXE, BLOCK_LOG, BLOCK_STONE, BLOCK_STONE_AXE, BLOCK_STONE_PICKAXE,
        };
        let deadfall = oriented(BLOCK_LOG, Axis::X);
        assert!(!is_breakable_with(BLOCK_LOG, Some(BLOCK_STONE_AXE)), "a lashed axe felled a tree");
        assert!(is_breakable_with(BLOCK_LOG, Some(BLOCK_WEDGED_AXE)), "a wedged axe cannot fell a tree");
        assert!(
            break_seconds_with(deadfall, Some(BLOCK_STONE_AXE)).unwrap()
                < break_seconds_with(deadfall, None).unwrap(),
            "a lashed axe is no better than hands on deadfall"
        );
        assert!(!is_breakable_with(BLOCK_STONE, Some(BLOCK_STONE_PICKAXE)), "a lashed pick broke rock");
        assert!(is_breakable_with(BLOCK_STONE, Some(BLOCK_WEDGED_PICKAXE)), "a wedged pick cannot break rock");
        assert!(
            break_seconds_with(BLOCK_DIRT, Some(BLOCK_STONE_PICKAXE)).unwrap()
                < break_seconds_with(BLOCK_DIRT, None).unwrap(),
            "a lashed pick is no better than hands on earth"
        );
        // ...and the wedged tool is the old flint tier exactly, so nothing
        // above it in the ladder moved when the rung was split in two.
        assert_eq!(definition(BLOCK_WEDGED_AXE).tool, Some(Tier::Flint));
        assert_eq!(definition(BLOCK_WEDGED_PICKAXE).tool, Some(Tier::Flint));
        assert_eq!(definition(BLOCK_STONE_AXE).tool, Some(Tier::Stone));
        assert_eq!(definition(BLOCK_STONE_PICKAXE).tool, Some(Tier::Stone));
    }

    #[test]
    fn only_things_that_are_picked_leave_anything_behind() {
        // `leaves_behind` is the one field that makes breaking a block
        // not empty its cell, and every entry on this list is a rule the
        // client's prediction has to know about -- which the player
        // otherwise sees as a block that flickers back. So the list is
        // written down here, and adding to it is a decision somebody
        // has to make in this test first.
        //
        // Two now, and they are the same idea twice: a thing you *pick*
        // -- berries off a bush, an apple out of a canopy -- leaves the
        // plant standing, and the plant grows the crop back
        // (`types::ripens_into`). That is what makes a hedgerow and an
        // orchard places to come back to rather than places to strip.
        let residues: Vec<&str> = BLOCKS
            .iter()
            .filter(|b| b.leaves_behind.is_some())
            .map(|b| b.name)
            .collect();
        // ...and three now: a robbed nest is the same idea in a tree.
        // What is picked is the eggs, what stays is the bowl of twigs,
        // and the bird lays in it again.
        // ...and four: a palm's coconuts are the apple's idea on a coast.
        // ...and six: the bilberry and the wild strawberry are the bush's idea
        // on a forest floor and at its edge.
        assert_eq!(
            residues,
            ["palm_coconuts", "apple_leaves_fruit", "berry_bush", "nest_eggs", "bilberry", "strawberry"]
        );
        // ...and what each leaves has to be a real block of the same
        // shape that can stand where it did, or picking would put a hole
        // in the world.
        for picked in [
            BLOCK_BERRY_BUSH,
            BLOCK_APPLE_LEAVES_FRUIT,
            BLOCK_NEST_EGGS,
            BLOCK_PALM_COCONUTS,
            BLOCK_BILBERRY,
            BLOCK_STRAWBERRY,
        ] {
            let bare = definition(picked).leaves_behind.expect("checked above");
            assert_eq!(definition(bare).shape, definition(picked).shape);
            assert!(
                definition(bare).hardness.is_some(),
                "{} cannot be removed once picked",
                definition(bare).name
            );
            // ...and it has to grow back, or picking it is stripping it
            // with extra steps.
            assert_eq!(crate::types::ripens_into(bare), Some(picked));
        }
    }
}
