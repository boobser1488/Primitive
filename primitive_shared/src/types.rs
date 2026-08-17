use serde::{Deserialize, Serialize};

// Этап 2: real 3D chunks, per the plan's "16x16x16" example. Height is
// now 64 (Этап 5 terrain needs real vertical room -- 16 was fine for a
// flat world but way too short for hills).
pub const CHUNK_SIZE_X: usize = 16;
pub const CHUNK_SIZE_Y: usize = 64;
pub const CHUNK_SIZE_Z: usize = 16;
pub const CHUNK_VOLUME: usize = CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z;

pub type BlockId = u16;

pub const BLOCK_AIR: BlockId = 0;
pub const BLOCK_GRASS: BlockId = 1;
pub const BLOCK_DIRT: BlockId = 2;
pub const BLOCK_STONE: BlockId = 3;
pub const BLOCK_SAND: BlockId = 4;
pub const BLOCK_SNOW: BlockId = 5;
pub const BLOCK_WATER: BlockId = 6;
pub const BLOCK_LOG: BlockId = 7;
pub const BLOCK_LEAVES: BlockId = 8;
pub const BLOCK_GLOWSTONE: BlockId = 9;
pub const BLOCK_PLANKS: BlockId = 10;
pub const BLOCK_COBBLESTONE: BlockId = 11;
/// A tuft of grass: drawn as two crossed planes, walked through rather
/// than over. See `is_cross`.
pub const BLOCK_TALL_GRASS: BlockId = 12;
/// Desert cactus. A solid block that happens to grow.
pub const BLOCK_CACTUS: BlockId = 13;
/// Sticks. A fallen branch, so it *lies* on the ground rather than
/// standing in its cell like a plant -- see `is_flat`. It stood upright
/// while the cross was the only shape that was not a cube, and looked
/// like a sapling someone had planted.
pub const BLOCK_STICK: BlockId = 14;
/// Plant fibre, pulled out of a tuft of grass.
///
/// The first id in this game that is **only** an item: there is no cell
/// of the world that can hold it. See `is_item` for what that costs and
/// what it buys.
pub const BLOCK_FIBER: BlockId = 15;
/// A loose stone lying on the ground.
///
/// The first block with no height at all: it is drawn as a single quad
/// laid on the surface, not as a cube and not as a cross. See `is_flat`.
pub const BLOCK_PEBBLE: BlockId = 16;
/// A nodule of flint.
///
/// Lies flat like a pebble and is gathered the same way, but it comes
/// from somewhere else: flint is a nodule that weathers out of rock, so
/// it is found on stony and gravelly ground above and -- much more of it
/// -- on the floor of a cave, which is the one place underground worth
/// walking rather than digging.
pub const BLOCK_FLINT: BlockId = 17;
/// Wood ash: what is left of a wood that burned.
///
/// A full block rather than something lying on the ground, because it is
/// the ground -- in a burnt forest it *is* the surface, in drifts around
/// the standing trunks. Soft: it is powder, and digging a hole in it
/// should feel like digging a hole in powder.
pub const BLOCK_ASH: BlockId = 19;
/// Clay: the riverbank material.
///
/// Loose in the sense that a shovel moves it and *not* in the sense the
/// rest of the loose materials are: wet clay holds a vertical face, which
/// is why a riverbank is a bank rather than a slope. So it does not come
/// in layers and it does not fall -- it is the one soft material you can
/// dig a tunnel through and have the roof stay up.
///
/// It is also the material with the most obvious future: fired clay is
/// pottery, and pottery is the first container that is not a hole in the
/// ground. Nothing here fires anything yet.
pub const BLOCK_CLAY: BlockId = 20;
/// Gravel: what a river leaves where it slows down.
///
/// Loose in every sense -- it comes in layers, it falls, and it is the
/// one material in the game that is *mostly* something else: flint
/// nodules weather out of it, which is where the black stones on a
/// stony shore come from and why four shovelfuls sift down to one.
pub const BLOCK_GRAVEL: BlockId = 21;

/// Birch: the same three blocks as the oak, in a second wood.
///
/// **A second wood rather than a variant of the first**, and that is the
/// question every game of this shape has to answer once. A variant would
/// be a bit in the id, which costs nothing and means birch and oak stack
/// together, craft interchangeably and are one entry in the pack -- and
/// it also means a birch wall and an oak wall are the same wall. Two
/// woods are two materials: they do not stack, and a house built of one
/// is visibly built of one.
///
/// The cost is exactly three ids and three textures, which is what a
/// material costs here. See `crafting`, where birch logs make birch
/// planks and nothing else does.
pub const BLOCK_BIRCH_LOG: BlockId = 22;
pub const BLOCK_BIRCH_LEAVES: BlockId = 23;
pub const BLOCK_BIRCH_PLANKS: BlockId = 24;

/// A chest: a block with an inventory inside it.
///
/// The first block in this game that is a *place* rather than a
/// material. Everything else is decided by its id alone -- two blocks of
/// dirt are the same block -- but two chests differ by what is in them,
/// so what is in one is keyed by where it stands and kept on the server
/// beside the world (see `primitive_server::logic::containers`).
pub const BLOCK_CHEST: BlockId = 18;

/// A backpack: what is left of a player where they died.
///
/// A container like the chest, and deliberately the *same* machinery --
/// contents keyed by position, spilled when the block is broken, opened
/// by the same gesture. What makes it a second block rather than a chest
/// dropped by the server is that the two mean different things and the
/// player has to be able to tell them apart across a hillside: a chest is
/// somewhere you chose to put things, a backpack is somewhere you lost
/// them.
///
/// **Not in `PLACEABLE_BLOCKS`, and it drops nothing.** Both follow from
/// who puts one in the world: the server does, at the moment of death,
/// and nobody else ever should. A player able to place one could stamp
/// fake graves across a world; one that dropped an item when broken would
/// be a free container, which is a thing you are supposed to have to make
/// planks for. Breaking it gives back exactly what was inside and the bag
/// itself is gone -- which is what happens to a bag you tip out.
pub const BLOCK_BACKPACK: BlockId = 25;

// ---- ore, metal, and the stone age that has to come first ----
//
// **Everything a player can hold is flint.** Not wood-stone-iron, which
// is the convention everywhere else and is not how any of it happened --
// you cannot cut rock with a stick -- and not the four-pick ladder this
// briefly had either. What is here instead is one age, done properly:
// a knife, an axe and a pick, all knapped from the same nodules, each
// for work the other two cannot do.
//
// The reason is that a ladder answers every question with "later". A
// player who cannot fell a tree is told to make a better pick; a player
// who wants stone faster is told to make a better pick; and the stone
// age becomes the thing you leave rather than the thing you are in. A
// set answers with "with what?", which is the question this game is
// about. Three tools that are equals is a wider stone age than four
// picks that are a queue.
//
// The metals stay: coal, copper, tin, iron, their ores and their
// smelting recipes. Nothing is made of them yet, and the ores are all
// reachable with flint -- slowly, which is what iron's thirteen seconds
// of hardness is for. Metal tools are the obvious next thing, and
// `blocks::Tier` still has its rungs waiting for them.
//
// See `blocks::Work` for how a tool knows what it is for, `blocks::Tier`
// for how a tier turns into a mining speed, and `break_seconds_with` for
// the one function all of it goes through.

/// A seam of coal. Drops the coal itself rather than the rock.
pub const BLOCK_COAL_ORE: BlockId = 26;
/// Copper ore: green-stained rock, shallow, and the commonest metal.
pub const BLOCK_COPPER_ORE: BlockId = 27;
/// Tin ore: the same depths as copper and a fraction as much of it.
///
/// The rarity is the mechanism, not flavour. Bronze needs both, so the
/// scarce one sets the price -- which is why a bronze pick is an
/// achievement and a copper one is a Tuesday.
pub const BLOCK_TIN_ORE: BlockId = 28;
/// Iron ore: deep, plentiful, and out of reach until there is metal to
/// dig it with.
pub const BLOCK_IRON_ORE: BlockId = 29;

/// Coal: fuel, and the thing every smelting recipe wants.
pub const BLOCK_COAL: BlockId = 30;
pub const BLOCK_COPPER_INGOT: BlockId = 31;
pub const BLOCK_TIN_INGOT: BlockId = 32;
/// Bronze: three parts copper to one of tin, which is roughly the real
/// alloy and exactly the reason tin matters.
pub const BLOCK_BRONZE_INGOT: BlockId = 33;
pub const BLOCK_IRON_INGOT: BlockId = 34;

/// A flint pick: a knapped head lashed to a worked haft. Rock and ore.
///
/// **Three tools, all flint, and no metal ones.** There were four picks
/// here -- flint, copper, bronze, iron -- and they were a ladder: each
/// one strictly better than the last, and the game's answer to every
/// obstacle was "come back with the next pick". That is a tech tree, and
/// it made the stone age a tutorial you leave. What replaced it is a
/// *set*: a knife, an axe and a pick, all of the same stone, none of
/// them an upgrade on any other, and each opening work the other two
/// cannot touch (see `blocks::Work`). Breadth instead of height.
///
/// The metals are still in the world -- ore, coal, smelting, bronze --
/// and nothing is made of them yet. That is a landing rather than a
/// dead end: see `blocks::Tier`.
pub const BLOCK_FLINT_PICKAXE: BlockId = 35;

// 36, 37 and 38 were the copper, bronze and iron picks, and are left
// unused rather than recycled. An id is what a save file says: a world
// put down while those existed has ingots and picks written into its
// inventories by number, and handing 36 to a flake of flint would turn
// a stranger's copper pick into a handful of stone chips on load. New
// things get new numbers; that is what makes the number cheap.

/// A struck flake of flint: the sharp waste from knapping a nodule, and
/// the first thing in the chain that makes a tool.
///
/// **This is the answer to the chicken and the egg.** A haft has to be
/// whittled to shape before anything can be bound to it, and whittling
/// wants a blade -- so a knife would need a knife. It does not, because
/// what actually cuts here is not a finished tool at all: a flake struck
/// off a nodule is *already* an edge, sharper than the knife it will
/// eventually help build and useless for anything but a few cuts, which
/// is exactly what a flake is. Every stone-age assemblage on earth is
/// mostly flakes for that reason. So the chain opens with a rock hit
/// against a rock, and nothing in it needs a tool that does not exist.
pub const BLOCK_FLINT_FLAKE: BlockId = 39;
/// A haft: a branch pared down to something a head can be bound to.
pub const BLOCK_WORKED_STICK: BlockId = 40;
/// The three heads. A head is a stone with an edge and no handle -- the
/// half of a tool that is made of the hard thing.
pub const BLOCK_FLINT_KNIFE_HEAD: BlockId = 41;
pub const BLOCK_FLINT_AXE_HEAD: BlockId = 42;
pub const BLOCK_FLINT_PICK_HEAD: BlockId = 43;
/// A flint knife: what cuts growing things, and the fastest way to
/// gather fibre and leaves.
pub const BLOCK_FLINT_KNIFE: BlockId = 44;
/// A flint axe: what brings a standing tree down. Before it existed,
/// wood came only from deadfall.
pub const BLOCK_FLINT_AXE: BlockId = 45;

/// Every block type that has a texture, in a stable order -- used by the
/// client's texture system to build a name<->id lookup, by the hotbar,
/// and by the server's anti-cheat to reject `SetBlock` carrying a block
/// id that doesn't exist. Keep in sync when adding a new BLOCK_*.
pub const ALL_BLOCK_IDS: &[(BlockId, &str)] = &[
    (BLOCK_GRASS, "grass"),
    (BLOCK_DIRT, "dirt"),
    (BLOCK_STONE, "stone"),
    (BLOCK_SAND, "sand"),
    (BLOCK_SNOW, "snow"),
    (BLOCK_WATER, "water"),
    (BLOCK_LOG, "log"),
    (BLOCK_LEAVES, "leaves"),
    (BLOCK_GLOWSTONE, "glowstone"),
    (BLOCK_PLANKS, "planks"),
    (BLOCK_COBBLESTONE, "cobblestone"),
    (BLOCK_TALL_GRASS, "tall_grass"),
    (BLOCK_CACTUS, "cactus"),
    (BLOCK_STICK, "stick"),
    (BLOCK_FIBER, "fiber"),
    (BLOCK_PEBBLE, "pebble"),
    (BLOCK_FLINT, "flint"),
    (BLOCK_CHEST, "chest"),
    (BLOCK_ASH, "ash"),
    (BLOCK_CLAY, "clay"),
    (BLOCK_GRAVEL, "gravel"),
    (BLOCK_BIRCH_LOG, "birch_log"),
    (BLOCK_BIRCH_LEAVES, "birch_leaves"),
    (BLOCK_BIRCH_PLANKS, "birch_planks"),
    (BLOCK_BACKPACK, "backpack"),
    // Ore and metal, appended in the order of the ages rather than filed
    // beside the stone they sit in: this list is the order the hotbar
    // offers things in, and a patch should add to the end of a player's
    // palette rather than reshuffle it.
    (BLOCK_COAL_ORE, "coal_ore"),
    (BLOCK_COPPER_ORE, "copper_ore"),
    (BLOCK_TIN_ORE, "tin_ore"),
    (BLOCK_IRON_ORE, "iron_ore"),
    (BLOCK_COAL, "coal"),
    (BLOCK_COPPER_INGOT, "copper_ingot"),
    (BLOCK_TIN_INGOT, "tin_ingot"),
    (BLOCK_BRONZE_INGOT, "bronze_ingot"),
    (BLOCK_IRON_INGOT, "iron_ingot"),
    // The stone age: five parts and the three tools they make. In the
    // order of the chain rather than the order of the tools, because
    // that is the order a player meets them -- a flake before a haft,
    // a haft before a head goes onto one.
    (BLOCK_FLINT_FLAKE, "flint_flake"),
    (BLOCK_WORKED_STICK, "worked_stick"),
    (BLOCK_FLINT_KNIFE_HEAD, "flint_knife_head"),
    (BLOCK_FLINT_AXE_HEAD, "flint_axe_head"),
    (BLOCK_FLINT_PICK_HEAD, "flint_pick_head"),
    (BLOCK_FLINT_KNIFE, "flint_knife"),
    (BLOCK_FLINT_AXE, "flint_axe"),
    (BLOCK_FLINT_PICKAXE, "flint_pickaxe"),
];

/// What a player is allowed to put into the world: the client hotbar
/// offers exactly these, and the server validates against the same list
/// (see `is_placeable`) rather than trusting the client's choice.
pub const PLACEABLE_BLOCKS: &[BlockId] = &[
    // No dressed stone: it cannot be broken by hand (see
    // `break_seconds`), and a block you can place but never remove is a
    // mistake the player cannot undo. Cobblestone is the stone you
    // build with.
    BLOCK_DIRT,
    BLOCK_GRASS,
    BLOCK_SAND,
    BLOCK_SNOW,
    BLOCK_LOG,
    BLOCK_LEAVES,
    BLOCK_BIRCH_LOG,
    BLOCK_BIRCH_LEAVES,
    BLOCK_GLOWSTONE,
    BLOCK_PLANKS,
    BLOCK_BIRCH_PLANKS,
    BLOCK_COBBLESTONE,
    BLOCK_TALL_GRASS,
    BLOCK_CACTUS,
    BLOCK_STICK,
    BLOCK_PEBBLE,
    BLOCK_FLINT,
    BLOCK_CHEST,
    BLOCK_ASH,
    BLOCK_CLAY,
    BLOCK_GRAVEL,
    // The metal ores, which *are* placeable even though the stone around
    // them is not. The rule the stone comment states is "nothing you can
    // put down and never pick up again", and an ore breaks the tie the
    // other way: you cannot be holding one without holding the pick that
    // took it out of the wall, so putting it back down is always undoable.
    // Coal ore is missing for the opposite reason to stone's -- breaking
    // it yields coal, so there is no ore block to place.
    BLOCK_COPPER_ORE,
    BLOCK_TIN_ORE,
    BLOCK_IRON_ORE,
];

// ---- what a block id carries besides its kind ----
//
// A block id carries two things: *what* it is in the low bits, and one
// small field on top of that saying which of its shapes this particular
// cell holds. A cell is still one `u16`, so nothing about chunk storage,
// saves or the wire format changes -- and an id saved before the field
// existed reads back as 0, which every kind defines as "the ordinary
// one".
//
// What that field *means* depends on the kind, and there are exactly two
// answers:
//
// * a log reads it as an **axis**: standing, or lying along X, or lying
//   along Z. The sixth face of a log is the same as its first, so a full
//   six-way facing would be two ids for one block.
// * a loose material (sand, soil, snow, ash) reads it as a **layer
//   count**: how many eighths of the cell it fills. Zero means all
//   eight, which is what every grain of sand in every save already on
//   disk is.
//
// Sharing one field rather than spending a bit each is not thrift for
// its own sake: the two are mutually exclusive by construction. Nothing
// that lies in layers has an axis -- a drift of snow turned on its side
// is a drift of snow -- and a log does not come in eighths. `is_known_block`
// is where that exclusivity is enforced, so a client cannot invent a
// sideways layer of sand.
//
// The cost is that every question about a block has to ask about its
// kind rather than its id, which is why every predicate below starts by
// stripping the variant. Getting that wrong does not corrupt anything;
// it makes a sideways log stop being wood, or a footprint in the snow
// stop being snow.

/// Where the variant field sits in a block id.
pub const VARIANT_SHIFT: u32 = 12;
/// The variant field itself: an axis, or a layer count.
pub const VARIANT_MASK: BlockId = 0b111 << VARIANT_SHIFT;
/// Where the orientation sits in a block id.
///
/// The same place as the variant field; the name is kept because
/// orientation is what the field meant when it was the only thing in it.
pub const ORIENTATION_SHIFT: u32 = VARIANT_SHIFT;
/// The orientation bits themselves -- the low two of the variant field.
pub const ORIENTATION_MASK: BlockId = 0b11 << ORIENTATION_SHIFT;
/// Everything below the variant field: what the block actually is.
pub const KIND_MASK: BlockId = (1 << VARIANT_SHIFT) - 1;

/// How many layers make up a whole block.
///
/// Eight because it is the coarsest count that still reads as a
/// gradient underfoot (an eighth is 12.5 cm at this scale, about the
/// depth of snow you would notice walking through) and because the
/// variant field is three bits wide, so eight values are exactly what
/// there is room for.
pub const LAYERS_PER_BLOCK: u8 = 8;

/// Which way an orientable block lies.
///
/// `Y` is 0 so that an id with no orientation bits set -- every id ever
/// written before this existed -- means upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Axis {
    Y = 0,
    X = 1,
    Z = 2,
}

impl Axis {
    /// The axis a face normal runs along. `None` for a zero vector.
    pub fn of_normal(dx: i32, dy: i32, dz: i32) -> Option<Axis> {
        match (dx != 0, dy != 0, dz != 0) {
            (true, false, false) => Some(Axis::X),
            (false, true, false) => Some(Axis::Y),
            (false, false, true) => Some(Axis::Z),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::Y => "y",
            Axis::X => "x",
            Axis::Z => "z",
        }
    }
}

/// What a block *is*, with any orientation stripped off.
#[inline]
pub fn block_kind(id: BlockId) -> BlockId {
    id & KIND_MASK
}

/// Which way it lies. Anything not orientable is upright by definition.
///
/// The guard is not decoration. The variant field is shared with the
/// layer count, so a three-eighths drift of snow has bits in it that
/// *read* as an axis -- and a block that answers "lying along X" to a
/// question it has no business being asked is exactly how a layer of
/// snow would turn into deadfall you could gather (see `break_seconds`).
#[inline]
pub fn block_axis(id: BlockId) -> Axis {
    if !is_orientable(id) {
        return Axis::Y;
    }
    match (id & ORIENTATION_MASK) >> ORIENTATION_SHIFT {
        1 => Axis::X,
        2 => Axis::Z,
        _ => Axis::Y,
    }
}

/// Puts an axis on a block, or leaves it alone if it has no use for one.
#[inline]
pub fn oriented(kind: BlockId, axis: Axis) -> BlockId {
    if !is_orientable(kind) {
        return block_kind(kind);
    }
    block_kind(kind) | ((axis as BlockId) << ORIENTATION_SHIFT)
}

/// Whether this kind of block is drawn differently depending on which
/// way it lies.
///
/// Only wood, for now. A cube of stone rotated is a cube of stone, and
/// giving it an orientation would mean two ids for one thing -- which
/// costs an inventory slot the moment two of them meet.
#[inline]
pub fn is_orientable(id: BlockId) -> bool {
    crate::blocks::definition(id).orientable
}

// ---- loose material, in layers ----
//
// Sand, soil, snow and ash are not laid down a metre at a time. A
// dusting of snow, a drift banked against a wall, a spadeful of earth
// tipped onto a path -- every one of them is *some* of a block, and a
// world whose only answers are "block" and "nothing" has to round each
// of them to a full metre. Rounding up walls a path in; rounding down
// loses it.
//
// So a loose material fills its cell in eighths, and everything else
// follows from that one number: how tall it is to stand on, how long it
// takes to shift, how much of it you get back, and whether the block
// beside it still has to draw the wall they share.

/// Does this material lie in layers?
///
/// The list is what a shovel moves. Turf is deliberately not on it: a
/// grass block is soil with a living skin, and half a skin is not a
/// thing -- breaking it already yields plain soil, which is what does
/// come in layers.
#[inline]
pub fn is_loose(id: BlockId) -> bool {
    crate::blocks::definition(id).matter == crate::blocks::Matter::Loose
}

/// Does this material carry a depth in the variant field?
///
/// **Water, and nothing else.** A cell of water has a *level* -- how
/// much of the cell the fluid simulation has put in it -- and that is
/// the number this field holds.
///
/// Loose material used to answer yes as well: a drift of snow filled
/// its cell in eighths, and so did ash, and sand, and soil. That is
/// gone. What it bought was a surface at eighths of a block; what it
/// cost was every consumer of a block having to ask *how much of a
/// block*, and a collider that had to step over material, sink into
/// it, and be lifted back out of it -- which is where the worst of the
/// movement bugs lived. A block is a block again.
#[inline]
pub fn has_depth(id: BlockId) -> bool {
    is_liquid(id)
}

/// May this id legitimately carry bits in the variant field?
///
/// Deliberately wider than `has_depth`. Worlds saved while loose
/// material came in layers have drifts with a depth written into them,
/// and a save that has suddenly become full of *invalid* blocks is a
/// save that will not load. The bits are simply ignored now --
/// `block_layers` reads a full cell for anything that is not a liquid
/// -- so a legacy drift comes back as the whole block it is now drawn
/// as, and nobody has to migrate anything.
#[inline]
fn may_carry_variant(kind: BlockId) -> bool {
    is_loose(kind) || is_liquid(kind)
}

/// How many eighths of its cell this block fills.
///
/// `LAYERS_PER_BLOCK` for everything with no depth of its own, and for
/// loose material stacked all the way up -- including every id written
/// before layers existed, because the field they left at zero means
/// "all of it". A save from before this feature reads back exactly as
/// it was.
#[inline]
pub fn block_layers(id: BlockId) -> u8 {
    if !has_depth(id) {
        return LAYERS_PER_BLOCK;
    }
    match ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8 {
        0 => LAYERS_PER_BLOCK,
        n => n,
    }
}

/// The same id carrying a layer count, or the plain block if this
/// material does not come in layers.
///
/// A count of zero would be a cell holding nothing, which is what air
/// is for; asking for one gives a single layer rather than an id that
/// draws nothing and can never be removed.
#[inline]
pub fn with_layers(kind: BlockId, layers: u8) -> BlockId {
    let kind = block_kind(kind);
    if !has_depth(kind) || layers >= LAYERS_PER_BLOCK {
        return kind;
    }
    kind | ((layers.max(1) as BlockId) << VARIANT_SHIFT)
}

/// Loose material that does *not* fill its cell.
#[inline]
pub fn is_partial(id: BlockId) -> bool {
    is_loose(id) && block_layers(id) < LAYERS_PER_BLOCK
}

/// How much of the cell this block occupies, measured up from its
/// floor: 1.0 for an ordinary block, less for a layer.
///
/// This is the *drawn* height. What you can stand on is
/// `collision_height`, which is the same number for everything solid
/// and zero for everything you walk through.
#[inline]
pub fn block_height(id: BlockId) -> f32 {
    block_layers(id) as f32 / LAYERS_PER_BLOCK as f32
}

/// The top of this block's collision box, measured up from the cell
/// floor. Zero means there is nothing here to walk into.
#[inline]
pub fn collision_height(id: BlockId) -> f32 {
    if !is_collidable(id) {
        return 0.0;
    }
    block_height(id)
}

/// Is the top of this cell a floor you can put something on?
///
/// Distinct from "is it solid": a three-eighths drift of snow is solid
/// enough to walk over, but a tuft of grass planted on it would stand
/// five eighths of a block in the air, and another layer laid on it
/// would hang in space. Anything that has to *rest* on the block below
/// asks this rather than `is_collidable`.
#[inline]
pub fn has_full_top(id: BlockId) -> bool {
    is_collidable(id) && block_layers(id) == LAYERS_PER_BLOCK
}

/// Adding `added` layers to what is already in a cell.
///
/// Returns the new block and whatever did not fit, so a caller with
/// more material than room -- a falling drift landing on a shallow one
/// -- can carry the remainder to the cell above instead of losing it.
/// `None` means the two are not the same material and nothing merges.
#[inline]
pub fn merge_layers(existing: BlockId, added: BlockId) -> Option<(BlockId, u8)> {
    if !is_loose(existing) || block_kind(existing) != block_kind(added) {
        return None;
    }
    let total = block_layers(existing) as u16 + block_layers(added) as u16;
    let kept = total.min(LAYERS_PER_BLOCK as u16) as u8;
    let spilled = (total - kept as u16) as u8;
    Some((with_layers(existing, kept), spilled))
}

/// How fast you walk over this, as a multiplier on walking speed.
///
/// One for every surface that is a surface. Less for the ones you have
/// to push *through*: snow takes the effort out of a stride, and deep
/// snow takes more of it than a dusting does, so the number follows the
/// layer count. Ash is powder that gives underfoot rather than resisting
/// it, so it costs a little and not much.
///
/// Shared because the server's anti-cheat has a speed limit and the
/// client has to stay under it; a client that thought snow was faster
/// than the server did would be rubber-banded in a snowfield.
#[inline]
pub fn surface_drag(id: BlockId) -> f32 {
    crate::blocks::definition(id).drag
}

/// What a placement costs, once the world has been asked whether it is
/// allowed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// New material: one item out of the pack.
    Fresh,
    /// More of what is already in the cell, and free.
    ///
    /// The item that started this cell bought a whole block's worth of
    /// material; laying it a layer at a time only decides how it is
    /// spread. Charging per layer would mean a cell filled in eighths
    /// cost eight times what the same cell filled in one click does,
    /// and paying eight items for a block that breaks back into one is
    /// how a player quietly loses everything they dug.
    Thicken,
}

/// May `wanted` be put into a cell that currently holds `existing`, and
/// what does it cost?
///
/// `None` means no: the cell is occupied by something this cannot go
/// into. The whole of the layer economy lives here, and it is one rule
/// -- **a cell costs one item, however thinly the material in it is
/// spread**. That is what keeps the arithmetic closed: a cell can hold
/// at most one item's worth of material, breaking it gives that item
/// back (`block_drop_count` counts layers only for what layers cost),
/// and there is no sequence of placements and breaks that ends with
/// more than you started with.
///
/// Shared because the client decides what to *ask* for and the server
/// decides what to *allow*, and a disagreement between the two shows up
/// as a block that appears and is then taken away again.
pub fn layer_placement(existing: BlockId, wanted: BlockId) -> Option<Placement> {
    // Something you build through: a tuft of grass, a stone lying on
    // the ground, water, air. Anything may replace it, as it always
    // could.
    let replaceable = is_air(existing) || is_liquid(existing) || is_cross(existing)
        || is_flat(existing);
    if !is_loose(wanted) {
        return if replaceable { Some(Placement::Fresh) } else { None };
    }
    if replaceable {
        return Some(Placement::Fresh);
    }
    // Adding to what is there: only more of the same material, and only
    // if there is room for it.
    if block_kind(existing) != block_kind(wanted) {
        return None;
    }
    if block_layers(wanted) > block_layers(existing) {
        Some(Placement::Thicken)
    } else {
        None
    }
}

/// Drawn as two quads crossing at the diagonals of the cell instead of
/// as a cube: grass, sticks -- anything that is a *thing standing in* a
/// block rather than a block.
///
/// Everything else about them follows from that shape. They cannot be
/// walked on (there is no surface), they do not stop light (there is
/// nothing solid to stop it), they are alpha-cutout (most of the texture
/// is empty), and a falling block passes straight through them.
#[inline]
pub fn is_cross(id: BlockId) -> bool {
    crate::blocks::definition(id).shape == crate::blocks::Shape::Cross
}

/// Lies flat on whatever is under it: one quad on the ground, no
/// height, no sides.
///
/// A third shape after the cube and the cross, and it costs a quarter
/// of what either does -- which is the point, because this is the one
/// piece of decoration that appears in *every* biome and so has to be
/// nearly free. Everything else about it follows the cross: you walk
/// through it, it stops no light, it is an alpha cutout, and a falling
/// block passes straight through.
#[inline]
pub fn is_flat(id: BlockId) -> bool {
    crate::blocks::definition(id).shape == crate::blocks::Shape::Flat
}

/// How far in from its cell's edges a flat thing lies, as a fraction.
///
/// A stone, a stick and a flake of flint are *objects* sitting on the
/// ground: they have a size, and it is smaller than the cell, or a
/// pebble would be a metre across. Ash is not an object -- it is what
/// is left of a wood that burned, and it covers what it settles on
/// edge to edge. A coating inset from the cell walls would draw a grid
/// of bare earth between every square of it.
#[inline]
pub fn flat_inset(id: BlockId) -> f32 {
    if is_covering_flat(id) {
        0.0
    } else {
        0.13
    }
}

/// A flat thing that hides the floor it lies on, edge to edge.
///
/// The distinction is between a flat **object** and a flat **coating**,
/// and every difference in how the two are drawn follows from it. A
/// pebble is an object: it sits in the middle of its cell with earth
/// showing round it, so it is inset, and it is lifted clear of the
/// ground so the depth buffer can tell the two apart. Ash is a coating:
/// it covers its cell corner to corner and there is no earth to show,
/// so it is neither.
///
/// **Lifting a coating is what put a gap under it.** A fiftieth of a
/// block is nothing seen from above and a visible ledge seen from the
/// side -- ash along the lip of a bank hung over the edge with daylight
/// under it, and the quad is drawn from both sides, so from below it
/// was a grey sheet floating in the air. The floor of the cell is where
/// a coating belongs, and it can go there because the face it would
/// have fought with is not drawn at all: see `face_visible`.
#[inline]
pub fn is_covering_flat(id: BlockId) -> bool {
    is_flat(id) && block_kind(id) == BLOCK_ASH
}

/// How far a flat thing floats above the floor of its cell.
///
/// Zero for a coating, which *replaces* the surface under it; a
/// fiftieth of a block for an object, which lies on a surface that is
/// still drawn. See `is_covering_flat` for why those are different
/// answers rather than one tolerance that suits both.
///
/// A fiftieth rather than the thousandth this started at: it is the
/// depth buffer that has to tell an object from the ground beneath it,
/// and its precision falls away with distance, so a pebble twenty
/// blocks off and the earth under it landed on the same value and
/// flickered between them as the camera moved.
#[inline]
pub fn flat_lift(id: BlockId) -> f32 {
    if is_covering_flat(id) {
        0.0
    } else {
        0.02
    }
}

/// Whether a face of this block may be laid down at a random quarter
/// turn.
///
/// A 16x16 texture repeated across a hillside is a grid, and the eye
/// finds a grid from further away than it finds any single texture.
/// Turning each face by a hash of where it is breaks the repeat for
/// nothing: the mesher already writes the four corners out one at a
/// time, so *which* corner gets which texture coordinate is a choice
/// rather than a cost (see the client's `mesher`).
///
/// It is only free for a texture with no up: a face of stone turned
/// sideways is a face of stone, and a plank turned sideways is a
/// mistake. Hence the per-face answer -- the top and bottom of a grass
/// block are turf and soil, both of which turn, while its sides are one
/// image of turf *over* soil and must stay the way round they were
/// drawn.
///
/// `face` is the mesher's face index: 0 = +Y, 1 = -Y, 2..5 the sides.
///
/// **Building blocks are no longer turned.** They were, for the reason
/// above, and the reason was sound and the result was not: a wall of
/// stone or a floor of dirt is a surface a player *builds*, and turning
/// each face of it by a hash means two blocks of the same material laid
/// side by side do not match. What breaks up the grid on a hillside
/// breaks up a wall the player squared off by hand, and only one of
/// those two is the game's business.
///
/// What still turns is what is scattered rather than built: a stone or a
/// nodule of flint lying on the ground is a separate object, and a
/// hundred of them all facing the same way is the lattice this was
/// written for -- with none of the cost, because you cannot build a wall
/// out of pebbles.
#[inline]
pub fn texture_turns(id: BlockId, _face: usize) -> bool {
    crate::blocks::definition(id).turns
}

/// Something you can carry but not put down as a block.
///
/// The inventory is indexed by `BlockId` throughout -- slots, stacks,
/// recipes, drops and the wire format all speak it -- so a pure item is
/// an id with no cell in the world rather than a second type running in
/// parallel. What makes it an item is exactly that it is missing from
/// `PLACEABLE_BLOCKS`, so the hotbar will not offer it and the server
/// refuses a `SetBlock` carrying it; `is_item` names that state so the
/// checks that care (drops, tests) can ask directly instead of
/// rediscovering it by negation.
#[inline]
pub fn is_item(id: BlockId) -> bool {
    crate::blocks::definition(id).shape == crate::blocks::Shape::Item
}

/// Whether a right click on this block *opens* something rather than
/// putting a block against it.
///
/// One question, asked by three places that would otherwise each have
/// their own list: the client, which must not place a block when the
/// player meant to open a chest; the server, which will only serve a
/// container gesture against a cell that actually holds one; and the
/// break path, which has to empty a chest into the world before the
/// block goes.
#[inline]
pub fn is_container(id: BlockId) -> bool {
    crate::blocks::definition(id).container
}

/// Living plant matter, tinted by the climate it grew in.
///
/// The client's mesher stamps a colour on these faces from the world
/// generator's temperature and humidity fields, so a tuft of grass in a
/// savanna is straw-coloured and the same block in a swamp is dark
/// green. Which blocks are alive is a property of the block, so it is
/// decided here rather than in the renderer.
#[inline]
pub fn is_foliage(id: BlockId) -> bool {
    crate::blocks::definition(id).foliage
}

/// Does this block fall down without something under it?
///
/// The two places that ask are the generator, which will not plant one
/// where it cannot stand, and everything afterwards -- breaking the
/// ground under it, and building it in the first place. Those three used
/// to answer separately, and they disagreed: worldgen refused, mining
/// took the plant with it, and *placing* one let a player hang a tuft of
/// grass in the sky by hand.
///
/// A partial layer is on the list for the same reason a stick is: it
/// lies *on* something. A shovelful of earth with nothing under it is
/// not a floating shovelful of earth, it is a shovelful of earth that
/// fell -- and a player who could hang eighths of soil in the sky would
/// have found the cheapest scaffolding in the game.
#[inline]
pub fn needs_support(id: BlockId) -> bool {
    is_cross(id) || is_flat(id) || is_partial(id) || crate::blocks::definition(id).propped
}

/// What a cross-shaped block needs under it to stay.
///
/// Grass on rock is not a thing, and neither is grass hanging in the
/// air where the dirt used to be -- worldgen checks this when it plants
/// them, and so does anything that removes the block underneath.
#[inline]
pub fn can_grow_on(plant: BlockId, ground: BlockId) -> bool {
    // A layer needs a floor to lie on, whatever it is made of -- and it
    // needs a *whole* one. Two shallow drifts stacked in adjacent cells
    // would leave the upper one hanging over the gap the lower one did
    // not fill, which is the one thing layers exist to avoid.
    if is_partial(plant) {
        return has_full_top(ground);
    }
    let full_floor = has_full_top(ground);
    let ground = block_kind(ground);
    match block_kind(plant) {
        BLOCK_TALL_GRASS => matches!(ground, BLOCK_GRASS | BLOCK_DIRT) && full_floor,
        BLOCK_CACTUS => matches!(ground, BLOCK_SAND | BLOCK_CACTUS) && full_floor,
        // A stick lies wherever it is dropped, and so does a stone --
        // as long as there is a whole surface under it. A pebble on a
        // dusting of snow would sit an eighth of a block in the air,
        // and the eye finds that instantly on flat ground.
        BLOCK_STICK | BLOCK_PEBBLE | BLOCK_FLINT | BLOCK_ASH => full_floor,
        _ => true,
    }
}

/// Maximum light level, matching the 4 bits per light channel packed into
/// each vertex on the client (see `primitive_client::mesh`).
pub const MAX_LIGHT: u8 = 15;

#[inline]
pub fn is_air(id: BlockId) -> bool {
    block_kind(id) == BLOCK_AIR
}

/// Collision. Water is deliberately still solid: there's no swimming yet,
/// so making it passable would just drop the player onto the seabed with
/// no way back up.
#[inline]
pub fn is_collidable(id: BlockId) -> bool {
    !is_air(id) && {
        let def = crate::blocks::definition(id);
        def.matter != crate::blocks::Matter::Liquid
            && matches!(def.shape, crate::blocks::Shape::Cube)
    }
}

/// A block you can move through but that resists you: swimmable, and
/// enough to slow a fall.
#[inline]
pub fn is_liquid(id: BlockId) -> bool {
    crate::blocks::definition(id).matter == crate::blocks::Matter::Liquid
}

/// What a build/break ray is allowed to stop at.
///
/// **This used to ask `is_collidable`, and that was the bug that made
/// grass unbreakable.** A tuft is deliberately not collidable -- one you
/// have to jump over is a tuft everyone hates -- so the ray went
/// straight through it and the crosshair reported the ground behind
/// instead. The block had a break time, a drop and a recipe waiting for
/// it, and no way to aim at it.
///
/// Water stays see-through to targeting, which is a separate and
/// deliberate choice: it lets you mine a lake bed and place blocks into
/// water rather than having the ray stop at the surface.
#[inline]
pub fn is_targetable(id: BlockId) -> bool {
    let id = block_kind(id);
    !is_air(id) && !is_liquid(id) && !is_item(id)
}

/// Blocks that fall when nothing holds them up.
#[inline]
pub fn is_affected_by_gravity(id: BlockId) -> bool {
    crate::blocks::definition(id).falls
}

/// Can a falling block displace what's here? Air yes, water yes (it
/// gets flooded out of the way), anything solid no.
#[inline]
pub fn can_be_displaced_by_falling(id: BlockId) -> bool {
    // Air, water, and the things that stand or lie in a cell without
    // filling it. **Not an item**: a dropped stack is an entity rather
    // than a block, and sand landing where one lies has nothing to
    // displace.
    is_air(id) || is_liquid(id) || is_cross(id) || is_flat(id)
}

/// Fully blocks light and line of sight. Non-opaque blocks still get
/// their own faces drawn, and light propagates through them (attenuated
/// by `light_opacity`).
///
/// A partial layer is not opaque, and that single answer is what keeps
/// the rest of the world honest about it: light reaches the soil under
/// a dusting of snow, the block beside it still draws the wall they
/// share (a layer only hides part of it), and the mesher does not cull
/// against something that is mostly air.
#[inline]
pub fn is_opaque(id: BlockId) -> bool {
    if is_air(id) {
        return false;
    }
    // Opaque means *this shape fills its cell and stops light*. A fibre
    // has an opacity of fifteen and is still not opaque, because it is
    // not a cube: it is a thing lying in a cell with air all round it.
    // Both halves are needed, and reading only the opacity was the one
    // place moving these into a table changed an answer.
    let def = crate::blocks::definition(id);
    def.shape == crate::blocks::Shape::Cube && def.opacity >= MAX_LIGHT
}

/// Drawn with alpha blending rather than in the opaque pass -- you can
/// see through it, and what's behind it has to be drawn first.
///
/// This is deliberately *not* the same question as `is_opaque`. Leaves
/// are see-through in the lighting and face-culling sense but are still
/// drawn in the opaque pass, as an alpha *cutout*: every one of their
/// texels is either fully solid or fully absent, so they need no
/// blending, no sorting, and they can keep writing depth. Water is the
/// only block that actually needs the transparent pass.
#[inline]
pub fn is_translucent(id: BlockId) -> bool {
    crate::blocks::definition(id).matter == crate::blocks::Matter::Liquid
}

/// Drawn with an alpha cutout: every texel is either fully solid or
/// fully absent, so the empty ones are discarded.
///
/// Kept apart from `is_translucent` because the two need opposite things
/// from the renderer. A cutout writes depth and needs no sorting; what it
/// does need is a fragment shader containing `discard`, and a shader
/// containing `discard` costs the GPU its early depth rejection for
/// *every* draw that uses it. So these get their own pass, and the
/// terrain -- which is almost all of the triangles -- keeps early-Z.
#[inline]
pub fn is_cutout(id: BlockId) -> bool {
    // Alpha with holes in it: a leaf, a tuft, a stone on the
    // ground. Read off the table as "lets light through but is not
    // a liquid", which is exactly the set.
    let def = crate::blocks::definition(id);
    !is_air(id)
        && def.opacity < MAX_LIGHT
        && def.matter != crate::blocks::Matter::Liquid
}

/// Extra light level lost when crossing one cell of this block, on top of
/// the usual 1-per-step. `MAX_LIGHT` = light stops dead.
#[inline]
pub fn light_opacity(id: BlockId) -> u8 {
    if is_air(id) {
        return 0;
    }
    crate::blocks::definition(id).opacity
}

/// Light this block emits on its own, independent of the sun -- what
/// makes caves and night-time builds visible.
#[inline]
pub fn light_emission(id: BlockId) -> u8 {
    crate::blocks::definition(id).emission
}

/// Is this an id the game could actually have written?
///
/// Stricter than "is the kind known", because the orientation bits are
/// part of the id and a client is free to put anything in them. Three
/// ways to fail: an unknown kind, an axis of 3 (the fourth value has no
/// meaning), and an orientation on something that has no use for one --
/// an upright cobblestone and a sideways one would be two ids for the
/// same block, which costs an inventory slot the moment two of them
/// meet.
#[inline]
pub fn is_known_block(id: BlockId) -> bool {
    let kind = block_kind(id);
    if !crate::blocks::is_defined(id) {
        return false;
    }
    if is_orientable(kind) {
        // An axis of 3 is the fourth value of a two-bit field and means
        // nothing, and the bit above the axis is not part of an
        // orientation at all.
        return (id & VARIANT_MASK) >> VARIANT_SHIFT <= 2;
    }
    // Everything else: the variant field may only carry bits on the ids
    // that have a use for one. Bits anywhere else came off a socket
    // wrong, or off a save from a build that used them differently.
    (id & VARIANT_MASK) == 0 || may_carry_variant(kind)
}

/// Anti-cheat helper: may a client ask to *place* this block?
#[inline]
pub fn is_placeable(id: BlockId) -> bool {
    crate::blocks::definition(id).placeable
}

/// How long breaking this block takes, in seconds, bare-handed.
///
/// Shared rather than client-side because the server rate-limits block
/// edits, and the two numbers have to agree: a hardness the server
/// thought was lower than the client did would let a legitimate player
/// trip the anti-cheat by mining normally.
///
/// `None` means the block cannot be broken at all -- water is not a
/// thing you mine, it is a thing you swim through.
///
/// A partial layer costs its share of the whole and no more: clearing a
/// dusting of snow off a path should not take as long as digging a
/// metre of it out. The floor stops the shallowest layer from being
/// instant, because a block that vanishes on the down-click reads as a
/// misclick rather than as work.
#[inline]
pub fn break_seconds(id: BlockId) -> Option<f32> {
    break_seconds_with(id, None)
}

/// What tier the thing in the player's hand is, if it is a tool at all.
///
/// Anything that is not a tool -- a block, an ingot, an empty hand --
/// is `Hand`, so the caller never has to distinguish "holding nothing"
/// from "holding a lump of dirt". They dig equally well.
///
/// This is the tier of the *tool*, not the tier it brings to a
/// particular block: an axe held against a rock face is still a flint
/// axe, and it is still no use. That question is
/// `tool_tier_against`, which is what mining actually asks.
#[inline]
pub fn tool_tier(held: Option<BlockId>) -> crate::blocks::Tier {
    held.and_then(|id| crate::blocks::definition(id).tool)
        .unwrap_or(crate::blocks::Tier::Hand)
}

/// What tier the held thing counts as **against this block**.
///
/// The one place the tool set becomes a rule. A tool brings its own tier
/// to the work it is for and to work nobody needs a tool for; against
/// anything else it is a lump of stone on a stick, which is to say
/// `Hand`.
///
/// The consequence worth stating plainly: a pick does not fell a tree
/// and an axe does not open rock, and neither of them *slowly* does the
/// other's job. A halved speed would have been the gentler rule and it
/// would have taught the player nothing -- "keep swinging and it will
/// eventually work" is how you get a game where the tools are a tax. The
/// swing has to achieve nothing, or the distinction is decoration.
///
/// Work nobody needs a tool for -- soil, sand, planks, gathering
/// deadfall -- takes the tier from any tool at all, because anything
/// with a haft beats a fist and a player who has just spent an evening
/// on an axe should feel it everywhere they swing it.
#[inline]
pub fn tool_tier_against(block: BlockId, held: Option<BlockId>) -> crate::blocks::Tier {
    use crate::blocks::{definition, Tier, Work};
    let Some(held) = held else { return Tier::Hand };
    let tool = definition(held);
    let Some(tier) = tool.tool else { return Tier::Hand };
    let wanted = definition(block).work;
    if wanted == Work::Any || wanted == tool.work {
        tier
    } else {
        Tier::Hand
    }
}

/// How long breaking this block takes with a particular thing in hand.
///
/// **The one place mining time is decided**, and it has to be, because
/// three parties compute it a frame apart and any disagreement is a bug
/// the player experiences as the world lying to them: the client, which
/// fills the progress bar; the server, which refuses an edit the tier
/// cannot make; and the stamina tank, which bills for the swing.
///
/// `None` means this cannot be broken with that -- either the block is
/// nothing you mine (water, air) or the tool is below the tier the block
/// needs. Those are deliberately the same answer: from the player's side
/// they are the same experience, a swing that achieves nothing, and the
/// client's rule is simply not to start swinging.
///
/// A tool is faster on everything it is *for*, and on everything nobody
/// needs a tool for -- not only on what it unlocks. The alternative -- a
/// tool that helps only on the blocks it gates -- means a pick digs soil
/// no faster than fingernails, and the player who has just spent an
/// evening making one can feel that. What it is not faster on is another
/// tool's work: see `tool_tier_against`.
#[inline]
pub fn break_seconds_with(id: BlockId, tool: Option<BlockId>) -> Option<f32> {
    let def = crate::blocks::definition(id);
    let tier = tool_tier_against(id, tool);
    // A standing trunk needs an axe; the same log lying down is
    // deadfall, and pulling deadfall apart is something hands can do.
    // The early game runs on that difference and still does -- an axe
    // makes a forest into timber, and a player without one is not
    // stranded, only slower and dependent on what has already fallen.
    // See `BlockDef::felled`.
    if block_axis(id) != Axis::Y {
        if let Some(felled) = def.felled {
            return Some(felled / tier.speed());
        }
    }
    let hardness = def.hardness?;
    if tier < def.needs {
        return None;
    }
    Some(hardness / tier.speed())
}

/// Can this be taken apart with *that* at all?
#[inline]
pub fn is_breakable_with(id: BlockId, tool: Option<BlockId>) -> bool {
    break_seconds_with(id, tool).is_some()
}


/// Can this be taken apart with bare hands at all?
///
/// The counterpart of `break_seconds` returning `None` for something
/// that is nonetheless solid and in the way. Asked by the server, which
/// refuses the edit, and by the client, which does not start swinging.
#[inline]
pub fn is_breakable(id: BlockId) -> bool {
    break_seconds(id).is_some()
}

/// What one of these weighs, in kilograms.
///
/// Shared rather than client-side because carried weight feeds fall
/// damage, which the server decides. If the two sides disagreed about
/// what a stack of stone weighs, a player would be hurt by a load they
/// were never told they had.
///
/// The numbers are roughly a cubic metre of the real material scaled
/// down by twenty, which keeps the ordering honest -- stone really is
/// about twice sand and sand about twice packed leaves -- while landing
/// a full load somewhere a person could plausibly stagger under.
#[inline]
pub fn block_weight(id: BlockId) -> f32 {
    crate::blocks::definition(id).weight
}

/// What breaking this block puts in your hotbar.
///
/// Not always the block itself: grass turns to dirt and stone turns to
/// cobblestone, which is what stops a player from quietly reshaping the
/// world's surface into whatever they mined last.
///
/// A tuft of grass yields fibre rather than the tuft. Pulling up a plant
/// and getting a plant back makes grass a block you *harvest*, which is
/// not what tearing a handful of grass out of the ground is; the fibre
/// is the material, and a tuft can be replanted from it (see
/// `crafting::RECIPES`).
#[inline]
pub fn block_drop(id: BlockId) -> Option<BlockId> {
    crate::blocks::definition(id).drop
}

/// How many of `block_drop` breaking this block yields.
///
/// Always one, and deliberately so -- including for a cell holding a
/// single layer of soil. A cell costs one item to start whatever depth
/// it ends up at (see `layer_placement`), so it has to give one item
/// back, and only one. Paying by the layer instead would mean a block
/// built in eighths cost eight and returned one; paying *out* by the
/// layer would mean digging a hillside yielded eight times what the
/// same hillside used to.
///
/// It is a function rather than the literal `1` because the question is
/// real -- something that comes apart into several things is an obvious
/// thing to want -- and the answer above is a decision that should be
/// written down where it is made.
#[inline]
pub fn block_drop_count(_id: BlockId) -> u8 {
    1
}

pub fn block_name(id: BlockId) -> &'static str {
    crate::blocks::definition(id).name
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Converts a global block coordinate into (chunk position, local x, local z).
    ///
    /// FIX (per plan warning "Работа с отрицательными координатами"): must use
    /// div_euclid/rem_euclid rather than plain `/` and `%`, otherwise negative
    /// global coordinates map to the wrong chunk/local index.
    pub fn from_global(gx: i32, gz: i32) -> (ChunkPos, usize, usize) {
        let cx = gx.div_euclid(CHUNK_SIZE_X as i32);
        let cz = gz.div_euclid(CHUNK_SIZE_Z as i32);
        let lx = gx.rem_euclid(CHUNK_SIZE_X as i32) as usize;
        let lz = gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize;
        (ChunkPos::new(cx, cz), lx, lz)
    }

    /// Chunk containing a world-space position.
    pub fn from_world(x: f32, z: f32) -> ChunkPos {
        let (pos, _, _) = ChunkPos::from_global(x.floor() as i32, z.floor() as i32);
        pos
    }

    /// Chebyshev distance in chunks -- the natural metric for a square
    /// render/interest area.
    pub fn chebyshev_distance(&self, other: ChunkPos) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub pos: ChunkPos,
    // FIX: a fixed-size array ([T; 256]) is the "flat array, not Vec<Vec<T>>"
    // layout the plan calls for, but serde's built-in derive only covers
    // arrays up to length 32 without pulling in an extra crate (e.g.
    // serde_arrays). A single Vec<BlockId> of exactly CHUNK_VOLUME elements
    // keeps the same one-flat-buffer property (contiguous, no nesting,
    // O(1) indexed access) while staying trivially (de)serializable.
    pub blocks: Vec<BlockId>,
}

impl Chunk {
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        (y * CHUNK_SIZE_Z + z) * CHUNK_SIZE_X + x
    }

    /// Whether this chunk has the block array its accessors assume.
    ///
    /// **The one thing a `Vec` costs that a fixed-size array would not.**
    /// The field is a `Vec` because serde's derive stops at arrays of
    /// thirty-two (see the note on the field), and the price of that is
    /// that a chunk arriving over a socket carries its own length --
    /// whatever the sender felt like putting there. Every accessor here
    /// indexes straight into it, so a chunk one element short is a panic
    /// in `get`, which for a client means the game closing on a packet.
    ///
    /// Deserialisation cannot check this: bincode is being asked for a
    /// `Vec<BlockId>` and a short one is a perfectly good `Vec`. So it
    /// is checked on receipt, where inventories are already checked with
    /// `sanitize` and for the same reason -- everything off a wire is a
    /// claim until something has looked at it.
    ///
    /// A predicate rather than a repair: an inventory can be sensibly
    /// clamped back into shape, but a chunk of the wrong size is not a
    /// chunk with a mistake in it, it is terrain nobody can reconstruct.
    /// Padding it with air would draw a hole in the world and let the
    /// player walk into it.
    #[inline]
    pub fn is_well_formed(&self) -> bool {
        self.blocks.len() == CHUNK_VOLUME
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.blocks[Self::index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        self.blocks[Self::index(x, y, z)] = id;
    }

    /// Highest non-air block in a column, or -1 for an entirely empty
    /// column. Used by the server to pick a safe spawn height.
    pub fn height_at(&self, x: usize, z: usize) -> i32 {
        for y in (0..CHUNK_SIZE_Y).rev() {
            if self.get(x, y, z) != BLOCK_AIR {
                return y as i32;
            }
        }
        -1
    }

    /// Этап 1 world generation, extended for Этап 2's real chunk height:
    /// a single grass layer at y=0, air above. Kept for tests and as a
    /// trivial fallback; `worldgen::WorldGen` is the real generator.
    pub fn generate_flat(pos: ChunkPos) -> Self {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for x in 0..CHUNK_SIZE_X {
            for z in 0..CHUNK_SIZE_Z {
                blocks[Self::index(x, 0, z)] = BLOCK_GRASS;
            }
        }
        Self { pos, blocks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coords_map_correctly() {
        // -1 should land in chunk -1, local index 15 (last cell), not panic
        // or wrap into chunk 0 the way naive `%` would.
        let (pos, lx, lz) = ChunkPos::from_global(-1, -1);
        assert_eq!(pos, ChunkPos::new(-1, -1));
        assert_eq!(lx, 15);
        assert_eq!(lz, 15);
    }

    #[test]
    fn chunk_index_roundtrip() {
        let mut chunk = Chunk::generate_flat(ChunkPos::new(0, 0));
        chunk.set(3, 0, 7, BLOCK_STONE);
        assert_eq!(chunk.get(3, 0, 7), BLOCK_STONE);
        assert_eq!(chunk.get(0, 0, 0), BLOCK_GRASS);
    }

    #[test]
    fn block_properties_are_consistent() {
        assert!(!is_opaque(BLOCK_AIR));
        assert!(is_opaque(BLOCK_STONE));
        assert_eq!(light_opacity(BLOCK_AIR), 0);
        assert!(light_emission(BLOCK_GLOWSTONE) > 0);
        // A client must not be able to place air (that's what breaking is
        // for) or water (there's no bucket).
        assert!(!is_placeable(BLOCK_AIR));
        assert!(!is_placeable(BLOCK_WATER));
        // Dressed stone is deliberately not placeable: it cannot be
        // broken by hand, and a block you can put down and never pick
        // up is a mistake with no undo. Cobblestone is what you build
        // with.
        assert!(!is_placeable(BLOCK_STONE));
        assert!(is_placeable(BLOCK_COBBLESTONE));
        for &id in PLACEABLE_BLOCKS {
            assert!(is_known_block(id), "{id} is placeable but unknown");
        }
    }

    #[test]
    fn chunk_distance_is_chebyshev() {
        assert_eq!(
            ChunkPos::new(0, 0).chebyshev_distance(ChunkPos::new(3, -5)),
            5
        );
    }
}

#[cfg(test)]
mod fluid_tests {
    use super::*;

    #[test]
    fn water_is_not_something_you_can_stand_on() {
        // Regression: water used to be collidable, so a lake behaved
        // like a sheet of glass.
        assert!(!is_collidable(BLOCK_WATER));
        assert!(is_liquid(BLOCK_WATER));
    }

    #[test]
    fn solids_are_still_solid() {
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG, BLOCK_LEAVES] {
            assert!(is_collidable(id), "{} should be solid", block_name(id));
            assert!(!is_liquid(id));
        }
        assert!(!is_collidable(BLOCK_AIR));
    }

    #[test]
    fn cutout_and_translucent_are_different_questions() {
        // Leaves are see-through but write depth and need no sorting;
        // water is blended and does. Conflating them puts one of them
        // in a pass that renders it wrong.
        assert!(is_cutout(BLOCK_LEAVES));
        assert!(!is_translucent(BLOCK_LEAVES));
        assert!(is_translucent(BLOCK_WATER));
        assert!(!is_cutout(BLOCK_WATER));
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG] {
            assert!(!is_cutout(id) && !is_translucent(id));
        }
    }

    #[test]
    fn water_still_dims_light_without_blocking_it() {
        assert!(!is_opaque(BLOCK_WATER));
        assert!(light_opacity(BLOCK_WATER) > 0);
        assert!(light_opacity(BLOCK_WATER) < MAX_LIGHT);
    }
}

#[cfg(test)]
mod mining_tests {
    use super::*;

    #[test]
    fn what_cannot_be_broken_is_a_short_and_deliberate_list() {
        // A solid block with no break time is one the player can aim at
        // and swing at forever, so each one has to be a decision rather
        // than an oversight. Rock, ore and standing timber are the
        // decision. Cobblestone is not on the list because it is loose
        // rock you stacked yourself, and a fallen log is not because
        // gathering deadfall is picking something up.
        //
        // The list grew when ore did, and that is the *point* of ore:
        // every entry on it is now something a tool opens rather than
        // something nothing opens -- see the next test, which checks
        // exactly that.
        let unbreakable: Vec<&str> = ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| is_collidable(id) && break_seconds(id).is_none())
            .map(|&(_, name)| name)
            .collect();
        assert_eq!(
            unbreakable,
            [
                "stone",
                "log",
                "birch_log",
                "coal_ore",
                "copper_ore",
                "tin_ore",
                "iron_ore"
            ]
        );
    }

    #[test]
    fn everything_hands_cannot_open_is_opened_by_some_tool() {
        // The rule that keeps the list above from being a list of dead
        // ends: a block hands cannot break has to be reachable with
        // *some* tool in the game, or it is scenery.
        //
        // There is no longer an exception. Standing timber used to be
        // one -- unbreakable by anything, because the only tool was a
        // pick -- and the axe is what closed it. Every solid block in
        // the world now comes apart for somebody.
        let tools = [BLOCK_FLINT_PICKAXE, BLOCK_FLINT_AXE, BLOCK_FLINT_KNIFE];
        for &(id, name) in ALL_BLOCK_IDS {
            if !is_collidable(id) || break_seconds(id).is_some() {
                continue;
            }
            assert!(
                tools.iter().any(|&t| is_breakable_with(id, Some(t))),
                "{name} cannot be broken by anything at all"
            );
        }
    }

    #[test]
    fn each_tool_opens_its_own_work_and_nobody_elses() {
        // The set, stated as what is and is not possible. Every line is
        // a thing a player will try in their first hour.
        //
        // Bare hands: no rock, no ore, no standing tree.
        for block in [
            BLOCK_STONE,
            BLOCK_COAL_ORE,
            BLOCK_COPPER_ORE,
            BLOCK_TIN_ORE,
            BLOCK_IRON_ORE,
            BLOCK_LOG,
            BLOCK_BIRCH_LOG,
        ] {
            assert!(!is_breakable_with(block, None), "{} by hand", block_name(block));
        }
        // The pick opens rock and everything in it, iron included --
        // slowly, which is iron's whole difficulty now that no metal
        // tool exists to do it faster.
        for block in [
            BLOCK_STONE,
            BLOCK_COAL_ORE,
            BLOCK_COPPER_ORE,
            BLOCK_TIN_ORE,
            BLOCK_IRON_ORE,
        ] {
            assert!(
                is_breakable_with(block, Some(BLOCK_FLINT_PICKAXE)),
                "{} with a flint pick",
                block_name(block)
            );
        }
        let iron = break_seconds_with(BLOCK_IRON_ORE, Some(BLOCK_FLINT_PICKAXE)).unwrap();
        let stone = break_seconds_with(BLOCK_STONE, Some(BLOCK_FLINT_PICKAXE)).unwrap();
        assert!(iron > stone * 2.0, "iron ore should punish a stone edge");

        // The axe opens standing timber, and only the axe does.
        for wood in [BLOCK_LOG, BLOCK_BIRCH_LOG] {
            assert!(
                is_breakable_with(wood, Some(BLOCK_FLINT_AXE)),
                "{} with an axe",
                block_name(wood)
            );
            for wrong in [BLOCK_FLINT_PICKAXE, BLOCK_FLINT_KNIFE] {
                assert!(
                    !is_breakable_with(wood, Some(wrong)),
                    "{} felled by a {}",
                    block_name(wood),
                    block_name(wrong)
                );
            }
        }
        // ...and the pick is no use on a tree, nor the axe on a rock. Not
        // *slower* -- no use at all. A swing that achieves something
        // eventually would make the tools a tax rather than a choice.
        assert!(!is_breakable_with(BLOCK_STONE, Some(BLOCK_FLINT_AXE)));
        assert!(!is_breakable_with(BLOCK_STONE, Some(BLOCK_FLINT_KNIFE)));

        // The knife is the odd one: what it opens, hands already could.
        // What it buys is speed on growing things, and nothing else does.
        for plant in [BLOCK_TALL_GRASS, BLOCK_LEAVES, BLOCK_BIRCH_LEAVES] {
            let by_hand = break_seconds(plant).unwrap();
            let cut = break_seconds_with(plant, Some(BLOCK_FLINT_KNIFE)).unwrap();
            assert!(cut < by_hand, "{} is no faster with a knife", block_name(plant));
            assert_eq!(
                break_seconds_with(plant, Some(BLOCK_FLINT_PICKAXE)),
                Some(by_hand),
                "{} gave way to a pickaxe",
                block_name(plant)
            );
        }

        // Work nobody needs a tool for takes the tier from any of them:
        // a haft is a haft.
        for tool in [BLOCK_FLINT_PICKAXE, BLOCK_FLINT_AXE, BLOCK_FLINT_KNIFE] {
            assert!(
                break_seconds_with(BLOCK_DIRT, Some(tool)).unwrap()
                    < break_seconds(BLOCK_DIRT).unwrap(),
                "{} does not help with a hole",
                block_name(tool)
            );
        }
    }

    #[test]
    fn a_tool_is_not_a_block() {
        for pick in [BLOCK_FLINT_PICKAXE, BLOCK_FLINT_AXE, BLOCK_FLINT_KNIFE] {
            assert!(is_item(pick), "{} is not an item", block_name(pick));
            assert!(!is_placeable(pick));
            assert!(!is_collidable(pick));
            // Holding one does not make you better at holding it.
            assert_eq!(break_seconds_with(pick, Some(pick)), None);
        }
        // Anything that is not a tool digs like a bare hand, including a
        // pocketful of dirt.
        assert_eq!(
            break_seconds_with(BLOCK_DIRT, Some(BLOCK_SAND)),
            break_seconds(BLOCK_DIRT)
        );
    }

    #[test]
    fn smelting_and_alloying_have_recipes_that_lead_somewhere() {
        // Every ore has to end up as something, or digging it is a
        // hobby. Checked as a chain rather than recipe by recipe: ore ->
        // ingot -> pick, with coal in the middle, is the actual claim.
        use crate::crafting::RECIPES;
        let makes = |output: BlockId| RECIPES.iter().find(|r| r.output.0 == output);
        for (ore, ingot) in [
            (BLOCK_COPPER_ORE, BLOCK_COPPER_INGOT),
            (BLOCK_TIN_ORE, BLOCK_TIN_INGOT),
            (BLOCK_IRON_ORE, BLOCK_IRON_INGOT),
        ] {
            let recipe = makes(ingot).expect("no smelting recipe");
            assert!(
                recipe.inputs.iter().any(|&(b, _)| b == ore),
                "{} is not smelted from its ore",
                block_name(ingot)
            );
            assert!(
                recipe.inputs.iter().any(|&(b, _)| b == BLOCK_COAL),
                "{} is smelted without fuel",
                block_name(ingot)
            );
        }
        let bronze = makes(BLOCK_BRONZE_INGOT).expect("no bronze");
        assert!(bronze.inputs.iter().any(|&(b, _)| b == BLOCK_COPPER_INGOT));
        assert!(bronze.inputs.iter().any(|&(b, _)| b == BLOCK_TIN_INGOT));
        // The chain now stops at the ingot: nothing is forged. What the
        // test still insists on is that every *tool* can be made, which
        // is the half a player cannot do without.
        for tool in [BLOCK_FLINT_PICKAXE, BLOCK_FLINT_AXE, BLOCK_FLINT_KNIFE] {
            assert!(makes(tool).is_some(), "{} cannot be made", block_name(tool));
        }
    }

    #[test]
    fn digging_is_something_you_spend_time_on() {
        // Nothing is instant, and nothing takes so long that a player
        // would think the game had stopped responding. Five seconds is
        // about the limit of what reads as work rather than as a hang.
        for &(id, name) in ALL_BLOCK_IDS {
            let Some(seconds) = break_seconds(id) else { continue };
            assert!(seconds > 0.0, "{name} breaks instantly");
            assert!(seconds <= 5.0, "{name} takes {seconds}s by hand");
        }
        // Picking something up off the ground is not digging.
        assert!(break_seconds(BLOCK_PEBBLE).unwrap() < 0.5);
        assert!(break_seconds(BLOCK_STICK).unwrap() < 0.5);
    }

    #[test]
    fn liquids_and_air_are_not_mineable() {
        assert!(break_seconds(BLOCK_AIR).is_none());
        assert!(break_seconds(BLOCK_WATER).is_none());
        assert!(block_drop(BLOCK_AIR).is_none());
        assert!(block_drop(BLOCK_WATER).is_none());
    }

    #[test]
    fn worked_wood_takes_longer_than_dirt_which_takes_longer_than_leaves() {
        // The ordering is the whole point of having hardness at all.
        // Stone used to be the top of that scale and is now off it
        // entirely.
        let dirt = break_seconds(BLOCK_DIRT).unwrap();
        let planks = break_seconds(BLOCK_PLANKS).unwrap();
        let leaves = break_seconds(BLOCK_LEAVES).unwrap();
        assert!(planks > dirt, "worked wood should be slower than dirt");
        assert!(dirt > leaves, "dirt should be slower than leaves");
        assert!(break_seconds(BLOCK_STONE).is_none(), "stone needs a tool");
    }

    #[test]
    fn grass_and_stone_drop_something_else() {
        assert_eq!(block_drop(BLOCK_GRASS), Some(BLOCK_DIRT));
        assert_eq!(block_drop(BLOCK_STONE), Some(BLOCK_COBBLESTONE));
        assert_eq!(block_drop(BLOCK_LOG), Some(BLOCK_LOG));
    }

    #[test]
    fn every_drop_is_something_you_can_put_back_or_use() {
        // A drop that can neither be placed nor spent is an item that
        // can only accumulate, which reads as a bug the first time a
        // player tries to use it. An item is allowed to be unplaceable
        // -- that is what makes it an item -- but then a recipe has to
        // want it, or it is the same dead weight by another name.
        for &(id, name) in ALL_BLOCK_IDS {
            let Some(drop) = block_drop(id) else { continue };
            if is_item(drop) {
                // Bronze and iron are smelted and then wait: the metal
                // tools they fed are gone and the forge that will want
                // them again is not built. Named here rather than
                // excused by a vaguer rule -- see the matching note in
                // `crafting::tests::every_recipe_is_made_of_real_blocks`.
                if matches!(drop, BLOCK_BRONZE_INGOT | BLOCK_IRON_INGOT) {
                    continue;
                }
                assert!(
                    crate::crafting::RECIPES
                        .iter()
                        .any(|r| r.inputs.iter().any(|&(block, _)| block == drop)),
                    "{name} drops {}, which nothing can be made from",
                    block_name(drop)
                );
                continue;
            }
            assert!(
                is_placeable(drop),
                "{name} drops {} , which cannot be placed",
                block_name(drop)
            );
        }
    }

    #[test]
    fn an_item_has_no_place_in_the_world() {
        // The whole definition of `is_item`: carried, never a cell.
        assert!(is_item(BLOCK_FIBER));
        assert!(!is_placeable(BLOCK_FIBER));
        assert!(!is_collidable(BLOCK_FIBER));
        assert!(!is_opaque(BLOCK_FIBER));
        assert_eq!(break_seconds(BLOCK_FIBER), None);
        // ...but it is a real id, so the anti-cheat lets it into an
        // inventory and the client can draw it.
        assert!(is_known_block(BLOCK_FIBER));
        assert_ne!(block_name(BLOCK_FIBER), "unknown");
    }

    #[test]
    fn pulling_up_grass_yields_fibre_rather_than_more_grass() {
        assert_eq!(block_drop(BLOCK_TALL_GRASS), Some(BLOCK_FIBER));
    }
}

#[cfg(test)]
mod gravity_tests {
    use super::*;

    #[test]
    fn only_sand_falls() {
        assert!(is_affected_by_gravity(BLOCK_SAND));
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG, BLOCK_GLOWSTONE] {
            assert!(!is_affected_by_gravity(id), "{} should not fall", block_name(id));
        }
    }

    #[test]
    fn sand_falls_through_air_and_water_but_not_through_solids() {
        assert!(can_be_displaced_by_falling(BLOCK_AIR));
        assert!(can_be_displaced_by_falling(BLOCK_WATER));
        assert!(!can_be_displaced_by_falling(BLOCK_STONE));
        assert!(!can_be_displaced_by_falling(BLOCK_SAND));
    }
}

/// Loose material in layers: the second thing the variant field means.
#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn every_solid_block_fills_its_cell() {
        // The removal, stated once. Layers are gone from everything
        // that is not a liquid, and a save written while they existed
        // reads back as whole blocks -- the depth field is simply not
        // consulted any more.
        for &(id, name) in ALL_BLOCK_IDS {
            assert_eq!(block_layers(id), LAYERS_PER_BLOCK, "{name} came back partial");
            assert_eq!(block_height(id), 1.0, "{name} shrank");
            assert!(!is_partial(id), "{name} is still partial");
        }
    }

    #[test]
    fn asking_for_a_layer_of_anything_gives_the_whole_block() {
        for id in [BLOCK_STONE, BLOCK_PLANKS, BLOCK_SNOW, BLOCK_SAND, BLOCK_ASH, BLOCK_GRAVEL] {
            for depth in 1..LAYERS_PER_BLOCK {
                assert_eq!(with_layers(id, depth), block_kind(id), "{}", block_name(id));
            }
        }
    }

    #[test]
    fn a_legacy_drift_still_loads() {
        // Worlds saved while loose material came in layers have a depth
        // written into the variant field. Rejecting those ids would
        // make every one of those saves unopenable; they are accepted
        // and read as the whole block they are now drawn as.
        // Ash is not here any more: it stopped being a solid at all
        // when it became a powder lying on the ground rather than a
        // block of the stuff. Its legacy bits are still accepted, and
        // that is what the loop below checks; what it no longer has is
        // a collision box.
        for kind in [BLOCK_SNOW, BLOCK_SAND, BLOCK_GRAVEL, BLOCK_DIRT] {
            let legacy = kind | (3 << VARIANT_SHIFT);
            assert!(is_known_block(legacy), "{} would not load", block_name(kind));
            assert_eq!(block_layers(legacy), LAYERS_PER_BLOCK);
            assert_eq!(collision_height(legacy), 1.0);
        }
        // Ash keeps the tolerance without keeping the collision box.
        assert!(is_known_block(BLOCK_ASH | (3 << VARIANT_SHIFT)));
        assert_eq!(collision_height(BLOCK_ASH), 0.0, "ash is walked through now");

        // ...but a depth on something that never had one is still junk.
        assert!(!is_known_block(BLOCK_PLANKS | (3 << VARIANT_SHIFT)));
    }

    #[test]
    fn water_keeps_its_levels() {
        // The half that stays: a cell of water has a level, and the
        // fluid simulation needs somewhere to keep it.
        assert!(has_depth(BLOCK_WATER));
        for level in 1..LAYERS_PER_BLOCK {
            assert_eq!(block_layers(with_layers(BLOCK_WATER, level)), level);
        }
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::*;

    #[test]
    fn a_sideways_log_is_still_a_log_in_almost_every_way() {
        // The whole risk of packing the axis into the id: one predicate
        // that forgets to strip it turns a rotated block into an
        // unknown one, and unknown blocks are invisible, weightless and
        // unbreakable.
        let upright = BLOCK_LOG;
        for axis in [Axis::X, Axis::Z] {
            let lying = oriented(BLOCK_LOG, axis);
            assert_ne!(lying, upright, "{} did not change the id", axis.name());
            assert_eq!(block_kind(lying), BLOCK_LOG);
            assert_eq!(block_axis(lying), axis);
            assert_eq!(block_name(lying), block_name(upright));
            assert_eq!(is_opaque(lying), is_opaque(upright));
            assert_eq!(is_collidable(lying), is_collidable(upright));
            // Break time is the one deliberate exception: a fallen
            // trunk can be gathered by hand and a standing one cannot.
            // See `tool_tests`.
            assert_eq!(block_weight(lying), block_weight(upright));
            assert_eq!(light_opacity(lying), light_opacity(upright));
            assert!(is_placeable(lying));
            assert!(is_known_block(lying));
            assert!(is_targetable(lying));
        }
    }

    #[test]
    fn breaking_a_sideways_log_gives_an_ordinary_one() {
        // Or the inventory would hold three kinds of log and a stack of
        // each -- three slots for one material.
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::X)), Some(BLOCK_LOG));
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::Z)), Some(BLOCK_LOG));
    }

    #[test]
    fn an_id_from_before_orientation_existed_reads_as_upright() {
        // Every block in every save on disk. Axis 0 has to be the
        // default, and it has to be the one that means "standing".
        assert_eq!(block_axis(BLOCK_LOG), Axis::Y);
        assert_eq!(oriented(BLOCK_LOG, Axis::Y), BLOCK_LOG);
        for &(id, name) in ALL_BLOCK_IDS {
            assert_eq!(block_kind(id), id, "{name} collides with the axis bits");
            assert_eq!(block_axis(id), Axis::Y, "{name} reads as rotated");
        }
    }

    #[test]
    fn nothing_but_wood_can_be_turned() {
        // Asking for a rotated stone gives plain stone rather than a
        // second id for the same block.
        assert!(!is_orientable(BLOCK_STONE));
        assert_eq!(oriented(BLOCK_STONE, Axis::X), BLOCK_STONE);
        assert!(is_orientable(BLOCK_LOG));
    }

    #[test]
    fn nonsense_orientation_bits_are_refused() {
        // These arrive from the network: a client picks the axis when it
        // places a block, so the field is under its control.
        let bad_axis = BLOCK_LOG | (3 << ORIENTATION_SHIFT);
        assert!(!is_known_block(bad_axis), "axis 3 has no meaning");
        let turned_stone = BLOCK_STONE | (1 << ORIENTATION_SHIFT);
        assert!(!is_known_block(turned_stone), "stone has no axis to set");
        // ...and the sentinel the mesher uses for unloaded chunks must
        // still be nobody's block.
        assert!(!is_known_block(BlockId::MAX));
    }

    #[test]
    fn a_face_normal_names_an_axis() {
        assert_eq!(Axis::of_normal(1, 0, 0), Some(Axis::X));
        assert_eq!(Axis::of_normal(0, -1, 0), Some(Axis::Y));
        assert_eq!(Axis::of_normal(0, 0, -1), Some(Axis::Z));
        assert_eq!(Axis::of_normal(0, 0, 0), None, "no face, no axis");
        assert_eq!(Axis::of_normal(1, 1, 0), None, "a diagonal is not a face");
    }
}

/// What a ray is allowed to stop at.
#[cfg(test)]
mod targeting_tests {
    use super::*;

    #[test]
    fn a_tuft_of_grass_can_be_aimed_at() {
        // The bug: targeting asked `is_collidable`, and a tuft is
        // deliberately not collidable -- so the ray went through it and
        // reported the ground behind. Grass had a break time, a drop and
        // a recipe waiting for it, and no way to aim at it.
        assert!(is_targetable(BLOCK_TALL_GRASS));
        assert!(is_targetable(BLOCK_STICK));
        assert!(!is_collidable(BLOCK_TALL_GRASS), "and it still is not solid");
        assert!(break_seconds(BLOCK_TALL_GRASS).is_some());
    }

    #[test]
    fn air_and_water_are_still_seen_through() {
        // Water on purpose: it lets you mine a lake bed and place blocks
        // into water instead of the ray stopping at the surface.
        assert!(!is_targetable(BLOCK_AIR));
        assert!(!is_targetable(BLOCK_WATER));
    }

    #[test]
    fn everything_solid_can_still_be_aimed_at() {
        for &(id, name) in ALL_BLOCK_IDS {
            if is_collidable(id) {
                assert!(is_targetable(id), "{name} is solid but cannot be aimed at");
            }
        }
    }
}

/// Stones lying on the ground: the third shape, after the cube and the
/// cross.
#[cfg(test)]
mod pebble_tests {
    use super::*;

    #[test]
    fn a_pebble_is_something_you_walk_over_rather_than_into() {
        // Everything about the shape follows from it having no height.
        assert!(is_flat(BLOCK_PEBBLE));
        assert!(!is_collidable(BLOCK_PEBBLE), "a stone you trip on");
        assert!(!is_opaque(BLOCK_PEBBLE), "a stone that casts a shadow");
        assert_eq!(light_opacity(BLOCK_PEBBLE), 0);
        assert!(is_cutout(BLOCK_PEBBLE), "most of its texture is not there");
        assert!(can_be_displaced_by_falling(BLOCK_PEBBLE), "sand should bury it");
        assert!(is_targetable(BLOCK_PEBBLE), "and it must be pickable");
    }

    #[test]
    fn it_is_neither_a_cube_nor_a_cross() {
        // Three shapes, and a block is exactly one of them. A block that
        // claimed two would be drawn twice.
        for &(id, name) in ALL_BLOCK_IDS {
            let shapes = is_flat(id) as u8 + is_cross(id) as u8;
            assert!(shapes <= 1, "{name} claims two shapes at once");
        }
        assert!(!is_cross(BLOCK_PEBBLE));
    }

    #[test]
    fn a_stone_needs_something_under_it() {
        // It lies on the ground, so taking the ground away takes it --
        // the same rule as a stick, through the same check.
        assert!(can_grow_on(BLOCK_PEBBLE, BLOCK_GRASS));
        assert!(can_grow_on(BLOCK_PEBBLE, BLOCK_SAND));
        assert!(can_grow_on(BLOCK_PEBBLE, BLOCK_SNOW));
        assert!(can_grow_on(BLOCK_PEBBLE, BLOCK_STONE));
        assert!(!can_grow_on(BLOCK_PEBBLE, BLOCK_AIR));
        assert!(!can_grow_on(BLOCK_PEBBLE, BLOCK_WATER));
    }

    #[test]
    fn picking_one_up_gives_a_stone_back() {
        assert_eq!(block_drop(BLOCK_PEBBLE), Some(BLOCK_PEBBLE));
        assert!(is_placeable(BLOCK_PEBBLE), "and it can be put down again");
        // Bending down, not digging: quick next to everything that has
        // to be dug, even after break times went up fivefold.
        let stone = break_seconds(BLOCK_PEBBLE).unwrap();
        assert!(stone < 0.5, "picking a stone up took {stone}s");
        assert!(stone * 4.0 < break_seconds(BLOCK_DIRT).unwrap());
    }
}

/// What holds a block up, asked once by everything that cares.
#[cfg(test)]
mod support_tests {
    use super::*;

    #[test]
    fn everything_that_lies_on_the_ground_says_so() {
        // The rule the generator, the collapse and the placement check
        // all read. A block missing from it can be hung in the sky; a
        // block wrongly in it cannot be placed at all.
        for id in [BLOCK_TALL_GRASS, BLOCK_STICK, BLOCK_PEBBLE, BLOCK_CACTUS] {
            assert!(needs_support(id), "{} floats", block_name(id));
        }
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_LOG, BLOCK_LEAVES, BLOCK_GLOWSTONE] {
            assert!(
                !needs_support(id),
                "{} was made to need ground -- floating stone is a build, not a bug",
                block_name(id)
            );
        }
    }

    #[test]
    fn nothing_that_needs_ground_can_stand_on_air_or_water() {
        // The bug this pairs with: placing had no support check at all,
        // so a tuft of grass could be built into the sky by hand while
        // the generator refused to plant one there.
        for id in [BLOCK_TALL_GRASS, BLOCK_STICK, BLOCK_PEBBLE, BLOCK_CACTUS] {
            assert!(!can_grow_on(id, BLOCK_AIR), "{} stands on nothing", block_name(id));
            assert!(!can_grow_on(id, BLOCK_WATER), "{} stands on water", block_name(id));
        }
    }

    #[test]
    fn a_rotated_log_is_not_suddenly_a_plant() {
        // `needs_support` strips the orientation like everything else;
        // a sideways log that thought it needed ground could not be
        // placed anywhere a normal one could.
        assert!(!needs_support(oriented(BLOCK_LOG, Axis::X)));
        assert!(!needs_support(oriented(BLOCK_LOG, Axis::Z)));
    }
}

/// What bare hands can and cannot get through.
#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn a_standing_tree_resists_and_a_fallen_one_does_not() {
        // The path to wood before there is an axe, and it needs no new
        // state: worldgen already lays fallen trunks along the way they
        // fell, and a tree already grows upright.
        assert!(!is_breakable(BLOCK_LOG), "a standing trunk gave way to hands");
        for axis in [Axis::X, Axis::Z] {
            assert!(
                is_breakable(oriented(BLOCK_LOG, axis)),
                "deadfall lying on the ground could not be gathered"
            );
        }
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::X)), Some(BLOCK_LOG));
    }

    #[test]
    fn an_axe_makes_a_forest_into_timber() {
        // What the axe changed: a standing trunk is work rather than
        // scenery. The number is chosen so that felling one costs what
        // pulling a fallen one apart by hand does -- an axe does not make
        // wood cheap, it makes it available.
        let felled_by_hand = break_seconds(oriented(BLOCK_LOG, Axis::X)).unwrap();
        let felled_with_an_axe = break_seconds_with(BLOCK_LOG, Some(BLOCK_FLINT_AXE)).unwrap();
        assert!((felled_with_an_axe - felled_by_hand).abs() < 0.01);
        // ...and deadfall is still quicker with one in your hand, the
        // way anything with a haft is.
        assert!(
            break_seconds_with(oriented(BLOCK_LOG, Axis::X), Some(BLOCK_FLINT_AXE)).unwrap()
                < felled_by_hand
        );
    }

    #[test]
    fn the_early_game_is_still_finishable() {
        // Every step from an empty inventory to worked wood has to be
        // reachable with bare hands, or the difficulty is a dead end
        // rather than a difficulty.
        //
        // Stones and sticks lie on the ground; deadfall is gathered;
        // planks come from that log; and four stones knap into cobble.
        for id in [BLOCK_PEBBLE, BLOCK_STICK, BLOCK_TALL_GRASS, BLOCK_LEAVES] {
            assert!(is_breakable(id), "{} cannot be collected", block_name(id));
        }
        assert!(is_breakable(oriented(BLOCK_LOG, Axis::X)));
        let makes_planks = crate::crafting::RECIPES
            .iter()
            .any(|r| r.output.0 == BLOCK_PLANKS && r.inputs.iter().all(|&(b, _)| b == BLOCK_LOG));
        assert!(makes_planks, "no way from a log to planks");
        let makes_cobble = crate::crafting::RECIPES
            .iter()
            .any(|r| r.output.0 == BLOCK_COBBLESTONE);
        assert!(makes_cobble, "no way from loose stones to a building block");
    }

    #[test]
    fn what_you_build_can_be_taken_down_again() {
        // A placeable block you cannot break is a mistake the player
        // cannot undo. Natural rock and standing timber are not
        // placeable, so this holds for everything they can actually put
        // down.
        //
        // **With the tool that produced it**, which is the ore's
        // amendment to this rule. An ore block is placeable and hands
        // will not shift it -- but the only way to be holding one is to
        // have cut it out of a wall with a pick, and the pick does not
        // evaporate. What the rule is really about is a player stranding
        // themselves, and nobody can strand themselves with a block they
        // could only have got by owning the undo.
        for &id in PLACEABLE_BLOCKS {
            if matches!(block_kind(id), BLOCK_LOG | BLOCK_BIRCH_LOG) {
                continue; // placed against a face, so it lands lying or upright
            }
            let by_hand = is_breakable(id) || !is_collidable(id);
            // The pick stands in for "the tool its own tier demands",
            // which is the tool the player necessarily had: the only
            // placeable blocks hands will not shift are ore, and ore
            // comes out of a wall with a pick or not at all.
            let by_the_tool_that_won_it = is_breakable_with(id, Some(BLOCK_FLINT_PICKAXE));
            assert!(
                by_hand || by_the_tool_that_won_it,
                "{} can be placed but never removed",
                block_name(id)
            );
        }
    }
}
