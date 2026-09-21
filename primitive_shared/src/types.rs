use serde::{Deserialize, Serialize};

// Этап 2: real 3D chunks, per the plan's "16x16x16" example. Height is
// now 64 (Этап 5 terrain needs real vertical room -- 16 was fine for a
// flat world but way too short for hills).
pub const CHUNK_SIZE_X: usize = 16;
pub const CHUNK_SIZE_Y: usize = 256;
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

/// A backpack: what *used* to be left of a player where they died.
///
/// **Nothing makes one of these any more** -- a death leaves the body
/// itself now (`BLOCK_CORPSE`), which is the same container machinery
/// wearing the right picture and, unlike a bag, something the world can
/// do something to. The row is still here, and it has to be: a world
/// saved before the change has bags standing in it with somebody's iron
/// in them, and an id that stopped being defined is a cell that loads as
/// the missing-block placeholder with its contents filed against a
/// nothing. So it stays a container, stays breakable, stays drawn, and
/// stays off every list that would put a new one in the world.
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
/// who puts one in the world: the server did, at the moment of death,
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
pub const BLOCK_STONE_PICKAXE: BlockId = 35;

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
pub const BLOCK_STONE_AXE_HEAD: BlockId = 42;
pub const BLOCK_STONE_PICK_HEAD: BlockId = 43;
/// A flint knife: what cuts growing things, and the fastest way to
/// gather fibre and leaves.
pub const BLOCK_FLINT_KNIFE: BlockId = 44;
/// A flint axe: what brings a standing tree down. Before it existed,
/// wood came only from deadfall.
pub const BLOCK_STONE_AXE: BlockId = 45;

// ---- what grows ----
//
// **Four plants that are not scenery.** The world had grass, leaves and
// a cactus, and every one of them was something to walk past: pulling a
// tuft yields fibre and that is the whole of what a growing thing was
// for. These four exist because a player now has to eat, and because a
// world where the only food is an animal is a world where the first
// evening is a hunt or a death.
//
// They are deliberately *unequal*. A bush is worth going to and comes
// back; a mushroom is a handful you find in the dark and is barely
// worth a detour; reeds are not food at all. That spread is what makes
// looking around the map a decision -- one plant that fed you would be
// a bar to keep topped up rather than a place to go.
/// A berry bush: the one plant that feeds you and is still there
/// tomorrow.
///
/// Picked rather than pulled up -- breaking it yields berries and puts
/// the bush back bare, and it fills again on its own (see the server's
/// `logic::growth`). A plant that had to be replanted from a seed would
/// be a farm, and a farm is a screen and a crop timer and a hoe; a bush
/// that comes back is the same promise with none of that.
pub const BLOCK_BERRY_BUSH: BlockId = 46;
/// A bush that has been picked: the same plant with nothing on it.
///
/// Its own id rather than a bit in the variant field, for the reason
/// birch is its own wood: the field is shared with the layer count and
/// the axis, and what a picked bush *is* has to survive a save, a wire
/// and an anti-cheat that reads ids.
pub const BLOCK_BARE_BUSH: BlockId = 47;
/// A mushroom, growing where the sun does not reach.
pub const BLOCK_MUSHROOM: BlockId = 48;
/// Reeds at the water's edge: the fibre you can gather without a knife
/// and stripping a whole meadow.
pub const BLOCK_REEDS: BlockId = 49;
/// A flower. Food for nothing, use for nothing, and the only block in
/// the game that is here because a meadow with nothing in it but grass
/// reads as a texture rather than as a place.
pub const BLOCK_FLOWER: BlockId = 50;

// ---- what you eat ----
/// A handful of berries. The first food, and a poor one: it keeps you
/// walking and it will not get you through a night of digging.
pub const BLOCK_BERRIES: BlockId = 51;
/// Meat, as it comes off the animal. Edible, and worth about what
/// berries are -- which is the argument for the fire.
pub const BLOCK_RAW_MEAT: BlockId = 52;
/// Meat that has been over a fire. Worth four times the raw, and the
/// reason a campfire is the first thing worth building.
pub const BLOCK_COOKED_MEAT: BlockId = 53;
/// A hide. Not food: what a lashing is made of once fibre stops being
/// strong enough to hold a metal head on (see `crafting::RECIPES`).
pub const BLOCK_HIDE: BlockId = 54;

// ---- fire ----
//
// **Two ids, not one with a flag.** Whether a fire is burning is the
// most consequential thing about it -- it is light, it is heat, it is
// what a recipe asks for and what the rain puts out -- and every one of
// those questions is asked of a block id, by the mesher, by the light
// engine, by the anti-cheat and by the save file. A bit in the variant
// field would have been free and would have made "is this alight" a
// question only code that remembered to strip the field could answer.
/// A campfire, laid and not lit: sticks over a ring of stones.
pub const BLOCK_CAMPFIRE: BlockId = 55;
/// The same fire, burning. Gives light, cooks what is worked beside it,
/// burns whoever stands in it, and goes out in the rain.
pub const BLOCK_CAMPFIRE_LIT: BlockId = 56;

// ---- the metals, finally made into something ----
//
// `blocks::Tier` has had Copper, Bronze and Iron in it since ore was
// added, with nothing standing on any of them: the ladder was a landing
// at the top of a staircase nobody could climb. These nine are what the
// smelting was always for.
//
// **Three metals times three tools, and no heads** -- which was the
// rule here and is no longer the whole of it. The argument ran: the
// flint chain is four steps because knapping *is* four steps, casting
// is one, so a metal tool is an ingot, a haft and a strap of hide in a
// single row, and the difficulty is in getting the metal rather than in
// the assembly. Copper has heads now; see `BLOCK_COPPER_AXE_HEAD` for
// what that argument got wrong and what survives of it. Bronze and iron
// still work the way this paragraph describes.
pub const BLOCK_COPPER_KNIFE: BlockId = 57;
pub const BLOCK_COPPER_AXE: BlockId = 58;
pub const BLOCK_COPPER_PICKAXE: BlockId = 59;
pub const BLOCK_BRONZE_KNIFE: BlockId = 60;
pub const BLOCK_BRONZE_AXE: BlockId = 61;
pub const BLOCK_BRONZE_PICKAXE: BlockId = 62;
pub const BLOCK_IRON_KNIFE: BlockId = 63;
pub const BLOCK_IRON_AXE: BlockId = 64;
pub const BLOCK_IRON_PICKAXE: BlockId = 65;

// ---- 1.9: the copper bench ----
//
// **The metals grow heads, and the argument above is now half wrong.**
// It said casting is one step -- metal goes into a mould and comes out
// the shape of the thing -- and used that to justify a metal tool being
// an ingot, a haft and a strap of hide in a single row. What comes out
// of the mould is the *head*. It still has to go on a stick and be
// strapped there, and that second step is not a formality: it is why a
// worked stick and a hide are in the recipe at all, and until now the
// recipe listed them without ever showing you the piece they were
// binding.
//
// So copper gets the same two-step shape flint has -- head, then
// hafting -- and the mould finally does something it can be seen doing.
// The difference between the two ages is still real and still where it
// was: knapping a head is four steps of *skill* and casting one is a
// supply line, but both of them end with a person tying a thing to a
// stick.
//
// **Copper only, for now.** Bronze and iron keep the single row until
// their heads are drawn; a head with no picture is a magenta
// checkerboard in the pack, which is a worse lie than a recipe that is
// one row short.
pub const BLOCK_COPPER_AXE_HEAD: BlockId = 115;
pub const BLOCK_COPPER_PICK_HEAD: BlockId = 116;
pub const BLOCK_COPPER_SHOVEL_HEAD: BlockId = 117;
pub const BLOCK_COPPER_HOE_HEAD: BlockId = 118;

/// A shovel: the tool for ground that is not rock.
///
/// **The first tool in the game that gates nothing**, and that is the
/// point of it. Soil, sand, gravel, snow and ash are `Work::Any` -- a
/// fist opens them and always has -- so a shovel cannot be the thing
/// that lets you dig, only the thing that makes digging quick. See
/// `break_seconds_with`, where `Work::Ground` buys a factor of two and
/// nothing else.
///
/// The alternative was to make loose ground a shovel's own work and let
/// every other tool fall to `Hand` against it, which is the rule the
/// rest of the tool set follows. It was rejected: a pick that cannot
/// dig a hole is not a decision, it is a second tool you are obliged to
/// carry, and the design note this repository argues from says a
/// mechanic should create a decision rather than a chore. Halving the
/// time makes carrying one a *choice* about pack space.
pub const BLOCK_COPPER_SHOVEL: BlockId = 119;
/// A copper hoe. The flint one (`BLOCK_HOE`) still tills; this is the
/// same job done by something that does not shatter -- see
/// `tool_durability`.
pub const BLOCK_COPPER_HOE: BlockId = 120;

// ---- 1.9: the torch ----
//
// **The first light a player can carry**, and it arrives now because
// the dark arrived first: a mine lit only by `ambient_light` at 0.02 is
// stone at a level and a half, which is black with a hint of shape in
// it (see `ClientSettings::ambient_light`, and `NIGHT_INTENSITY` beside
// it). Before that, a torch would have been a decoration; after it, a
// shaft without one is a shaft you cannot work in.
//
// **Three ids, and the third is the whole mechanic.** A torch that
// simply burned for ever is a lamp, and a lamp is not a decision. What
// makes carrying one a decision is that it *runs out*: the head is a
// wad of fibre, the fibre burns away in three quarters of a minute, and
// what is left in the hand is a stick with a charred end. So:
//
// * `BLOCK_TORCH` -- wadded and ready, not yet alight;
// * `BLOCK_TORCH_LIT` -- burning, and the only thing in the game that
//   lights the cell the *player* is standing in rather than a cell the
//   world holds (see the held light in `shader.wgsl`);
// * `BLOCK_TORCH_SPENT` -- the stick back, black at the end. Another
//   wad of fibre makes it a torch again.
//
// The alternative was two ids, with a spent torch and a fresh one being
// the same item and the fibre "topping it up". That reads as nothing at
// all: the player looks at their pack and cannot tell whether the thing
// in it will light. A burnt-out torch has to *look* burnt out, and the
// only way a picture says that is by being a different picture.
pub const BLOCK_TORCH: BlockId = 121;
pub const BLOCK_TORCH_LIT: BlockId = 122;
pub const BLOCK_TORCH_SPENT: BlockId = 123;

/// How long a wad of fibre burns, in seconds.
///
/// Three quarters of a minute is long enough to cross a cave and short
/// enough that the crossing is a *plan*: a player who walks in with one
/// torch and no fibre is a player counting seconds on the way back.
/// Shorter than this and the torch is a nuisance rather than a
/// decision; much longer and it is a lamp again.
///
/// Here rather than in the server because the client shows the burn
/// down (the flame shortens, see the hand), and a client that thought
/// the torch had longer to live than the server did would put it out in
/// the player's hand a second after saying it was fine.
pub const TORCH_SECONDS: f32 = 45.0;

/// The same life, counted in the units `BlockDef::durability` is counted
/// in.
///
/// **Tenths of a second, and it is the one row in that table that is not
/// swings.** Wear already exists, already survives a save, already
/// splits correctly when a stack is divided and is already drawn on the
/// item -- building a second, parallel notion of "how much of this is
/// left" would be two mechanisms that have to agree about one thing.
/// So the torch spends the same counter, and what advances it is the
/// clock instead of the pick.
///
/// **Hundredths rather than tenths or seconds**, and the reason is
/// arithmetic rather than smoothness. The server spends elapsed seconds,
/// and a tick at twenty hertz is a twentieth of one: in tenths that is
/// half a step, which truncates to nothing, and a torch counted that way
/// burns for ever. In hundredths it is exactly five, and at any other
/// tick rate the part that is thrown away is under a hundredth of a
/// second -- small enough that no accumulator has to be carried on the
/// player to catch it.
///
/// It also happens to be what the flame wants: it is drawn shorter as
/// the wad burns down, and a step a second would drop in visible jerks.
///
/// **Not derived from the tick rate**, which is a server setting: a
/// torch that burned for forty-five seconds on one server and thirty on
/// another would be the same item with two lifetimes. The server
/// converts elapsed *seconds* into these, so the number is what it says
/// it is wherever it is run.
pub const TORCH_LIFE: u32 = (TORCH_SECONDS as u32) * 100;

// ---- clay, fired ----
//
// **The step the world was missing.** A campfire is about six hundred
// degrees on a good day; copper melts at a thousand and eighty-five, and
// iron does not melt at anything a person could build until very much
// later -- it is won out of the ore as a spongy bloom in a shaft of
// burning charcoal with air forced through it. Smelting bronze over a
// ring of stones in a meadow was the one thing in this world's
// progression that could not have happened, and it was also the one
// place clay had nothing to do: the note on `BLOCK_CLAY` has said
// "nothing here fires anything yet" since the day it was added.
//
// So there is a second fire now, and it is the one every metal goes
// through. It is built out of the riverbank, it burns like a campfire
// and it goes out in the rain like one, and it is the reason to go
// looking for a river before going looking for ore.
//
// Two ids for the same reason the campfire has two; see the note above
// `BLOCK_CAMPFIRE`.
/// A kiln of daubed clay over a stone footing, built and not lit.
pub const BLOCK_KILN: BlockId = 66;
/// The same kiln, burning: what `crafting::Station::Forge` asks for.
pub const BLOCK_KILN_LIT: BlockId = 67;
/// A fired brick. Clay that has been through the kiln, and the first
/// thing anybody ever made in one.
pub const BLOCK_BRICK: BlockId = 68;
/// A hoe: a blade of flint lashed across the end of a haft.
///
/// **Not a `Tier`, and that is the point.** The three tools in this
/// world are a ladder -- knife, axe, pick, one set per age -- and each
/// rung opens harder rock. A hoe opens nothing: it turns turf into
/// tilled earth and does nothing else whatever it is made of, so putting
/// it on the ladder would mean four tools an age, three of which are
/// about mining and one of which is about soil. It is an *implement*,
/// which is a different word for a reason.
pub const BLOCK_HOE: BlockId = 70;

// ---- what grows because you planted it ----
//
// **The step that turns a camp into a place.** Everything a player eats
// up to here is something they found: a bush that was already there, an
// animal that was already walking about. Both run out where you are
// standing and both make you walk further to eat, which is the correct
// early game and the wrong whole game -- it means there is never a
// reason to be *here* rather than a hundred metres that way.
//
// A field is the first thing in this world that is worth coming back to.
// The chain is four blocks and one implement: hoe the turf, plant the
// seed, wait two days, cut it. What it yields is grain, which is flour
// and bread and the first food that keeps.
/// Seed, in the ground: the first stage of the crop.
///
/// The seeds a player carries and the crop's first stage are the *same
/// block*, which is what makes planting work without a second mechanic:
/// putting a seed down is placing a block, and the rules about what a
/// block may be placed on already say it has to be tilled earth. See
/// `can_grow_on`.
pub const BLOCK_SEEDS: BlockId = 71;
/// Tilled earth: what a hoe makes of turf.
pub const BLOCK_FARMLAND: BlockId = 72;
/// The crop, growing. Green, and worth nothing if you cut it now.
pub const BLOCK_WHEAT: BlockId = 73;
/// The crop, ripe. Gold, and the only stage that yields grain.
pub const BLOCK_WHEAT_RIPE: BlockId = 74;
/// Threshed grain.
pub const BLOCK_GRAIN: BlockId = 75;
/// Ground and wetted: dough, waiting for an oven.
pub const BLOCK_DOUGH: BlockId = 76;
/// Bread.
///
/// Not as good as a roast, and it does not have to be: what a loaf is
/// worth is that it came from a field you can walk back to rather than
/// from an animal you had to find.
pub const BLOCK_BREAD: BlockId = 77;

// ---- the copper age, as it actually happened ----
//
// **What this replaced.** Smelting was one row in the recipe table:
// ore plus fuel goes in, an ingot comes out, and the only thing between
// a player and metal was a fire hot enough. That is not how anybody ever
// got copper out of rock. What they did was make a *pot*: clay worked
// into a crucible and a mould, fired hard, and then ore melted inside
// the pot and poured into the mould. The pottery is not a step before
// the metal -- it is the *equipment*, and the reason the ceramic age and
// the copper age are the same age.
//
// So the chain now runs: clay in the hands, fired in the kiln, ore
// melted in what came out of it. Four blocks, and between them the
// difference between metal being a recipe and metal being a craft.
/// Native copper: a green-stained nodule lying on stony ground.
///
/// **The piece you find before you own a mine.** Copper is the one metal
/// that occurs in the ground already metallic, and picking it off a
/// scree slope is how the first copper anybody ever worked was got. It
/// is drawn and gathered exactly like a flint nodule or a loose stone --
/// a flat thing lying in a cell, no tool needed -- which is also what
/// makes it the right first sight of metal: it is lying there, and you
/// already know how to pick it up.
pub const BLOCK_NATIVE_COPPER: BlockId = 78;
/// A crucible, thrown from wet clay and not yet fired.
pub const BLOCK_VESSEL_RAW: BlockId = 79;
/// ...and fired hard: the pot that ore is melted in.
pub const BLOCK_VESSEL: BlockId = 80;
/// An ingot mould in wet clay.
pub const BLOCK_MOULD_RAW: BlockId = 81;
/// ...and fired: what molten metal is poured into.
pub const BLOCK_MOULD: BlockId = 82;
/// A bloomery: a stone shaft that wins iron without melting it.
///
/// **Iron is not copper and cannot be treated as it.** Copper melts at
/// 1085 degrees, which a pot in a good fire reaches; iron melts at 1538,
/// which nothing in the ancient world reached at all. Iron was won as a
/// *bloom* -- a spongy lump of metal and slag, reduced out of the ore
/// below its melting point in a tall charcoal shaft with air forced
/// through it -- and then beaten until the slag came out.
pub const BLOCK_BLOOMERY: BlockId = 83;
/// The same, burning.
pub const BLOCK_BLOOMERY_LIT: BlockId = 84;
/// A bloom: iron and slag together, straight out of the shaft.
pub const BLOCK_IRON_BLOOM: BlockId = 85;

/// Ice: water that stopped being liquid.
///
/// **The one surface in the world with no grip.** Everything else the
/// ground can be either slows you down (snow, sand, water to the knees)
/// or leaves you alone; ice takes the friction away instead, and it is
/// the one place where the momentum a player has been carrying all along
/// becomes something they have to steer rather than something the ground
/// quietly cancels for them. See `blocks::BlockDef::grip`.
///
/// A whole solid cube rather than a skin on top of water, because a
/// frozen lake is walked on, mined through, and built with -- and each
/// of those wants an ordinary block, not a fourth kind of fluid state.
/// Breaking it gives it back, so a rink is a thing a player can build.
pub const BLOCK_ICE: BlockId = 86;

// ---- what a hide becomes, and what a person wears ----
//
// The chain from here to a coat is the same shape as the one from flint
// to a pick: a thing the world gives you, a *process* that takes time
// and conditions rather than a recipe, and then something made out of
// what came off it. What is different is that the process is drying,
// which is the first mechanic in this world that a player starts and
// then walks away from -- see `primitive_server::drying`.

/// A raw hide, dried on a rack until it is leather.
///
/// **Not a recipe, deliberately.** Every other transformation in the
/// game is instantaneous once you have the ingredients: the craft menu
/// is a promise that if you hold these things you get that thing. Curing
/// a skin is not like that and never was -- it is days of doing nothing
/// in particular in weather that is neither wet nor freezing -- and
/// making it a recipe would flatten the one process in the game whose
/// whole content is *waiting somewhere suitable*. So a hide goes on a
/// rack, the rack watches the sky, and leather is what is there when you
/// come back.
pub const BLOCK_LEATHER: BlockId = 87;

/// **The drying rack: the larder's rack of two by two**, meat and fish and
/// kelp hung from a ridge between two A-frames (`rack_cells`,
/// `rack::Trade::Larder`).
///
/// It was a frame of sticks a hide is stretched on, one cell, and that frame
/// is `BLOCK_HIDE_FRAME` now: two racks for two jobs. A lone cell of this
/// kind is that old frame, and becomes one when its world is read.
pub const BLOCK_DRYING_RACK: BlockId = 88;

/// A jug, thrown from wet clay and not yet fired.
pub const BLOCK_JUG_RAW: BlockId = 89;
/// ...and fired hard, empty: what water is carried in.
pub const BLOCK_JUG: BlockId = 90;
/// The same jug with water in it.
///
/// A separate id rather than a variant field, because a full jug and an
/// empty one are two pictures, two weights and two things to do with
/// them -- and because the variant bits on this kind are spoken for by
/// nothing at all, which is precisely the argument for not starting.
/// See `jug_of` and `emptied_jug`.
pub const BLOCK_JUG_WATER: BlockId = 91;

// ---- clothing ----
//
// Four slots, and the four garments that fill them in leather. What a
// garment *does* is in `primitive_shared::equipment`; what it is made of
// is here, because that is the one row per block this file is.
pub const BLOCK_LEATHER_CAP: BlockId = 92;
pub const BLOCK_LEATHER_TUNIC: BlockId = 93;
pub const BLOCK_LEATHER_LEGGINGS: BlockId = 94;
pub const BLOCK_LEATHER_BOOTS: BlockId = 95;

// ---- armour ----
//
// The same four slots in metal, at the two tiers worth beating into
// plate. No copper set: copper is soft enough that a copper cuirass is
// a costume, and the age it belongs to is the age of the first knife
// rather than the first armour.
pub const BLOCK_BRONZE_HELM: BlockId = 96;
pub const BLOCK_BRONZE_CUIRASS: BlockId = 97;
pub const BLOCK_BRONZE_GREAVES: BlockId = 98;
pub const BLOCK_BRONZE_BOOTS: BlockId = 99;
pub const BLOCK_IRON_HELM: BlockId = 100;
pub const BLOCK_IRON_CUIRASS: BlockId = 101;
pub const BLOCK_IRON_GREAVES: BlockId = 102;
pub const BLOCK_IRON_BOOTS: BlockId = 103;

/// A trunk with the bark off it, lying down.
///
/// **What a felled tree leaves behind.** Cut the base of a standing
/// trunk and the whole tree comes down (see
/// `primitive_server::felling`); what lands is not the trunk it was.
/// Some of it is lost -- the crown, the taper, the part that shatters --
/// and the bark comes off in the fall, which is what happens to a tree
/// that hits the ground and is also what a woodsman would do to it next
/// anyway.
///
/// One block for every wood, and that is deliberate rather than thrift:
/// **debarked timber is debarked timber.** An oak and a birch are told
/// apart by their bark, and once it is off there is nothing left to tell
/// apart -- which is why it wears the pale cut-end picture both of them
/// already have (`terrain/log_top.png`, shared, costing no texture
/// layer at all).
pub const BLOCK_STRIPPED_LOG: BlockId = 104;

// ---- what there is to forage, and the one thing to get wrong ----
//
// A meadow had two things in it you could eat: a bush and, if you went
// underground, a mushroom. Both are picked standing up, both are worth
// almost nothing, and neither is a reason to look at the ground. These
// four are.

/// Root leaves: a low rosette with something underneath it.
///
/// **The first plant in this world that is worth digging up rather than
/// picking.** What is above ground is inedible and what is below it is a
/// meal -- so the plant is a *sign* rather than a crop, and a player who
/// learns to read it eats on the way to somewhere else. It grows where
/// grass does and takes no tending; it is not farming, which the field
/// already is.
pub const BLOCK_ROOTS: BlockId = 105;
/// The root itself: pale, filling, and much better for a fire.
///
/// Raw it is worth about what a berry is. Roasted it is worth most of a
/// meal -- the same ratio meat has, and for the same reason: the fire is
/// the thing this game wants a player to build, so everything edible
/// should have an opinion about it.
pub const BLOCK_ROOT: BlockId = 106;
/// ...and after the fire has had it.
pub const BLOCK_ROASTED_ROOT: BlockId = 107;
/// A toadstool: the mushroom's shape, spotted, and a mistake.
///
/// **The first thing in this world that punishes not looking.** It grows
/// where mushrooms grow and is drawn from the same silhouette, so the
/// difference is the cap: plain is food, flecked is not. Eating one
/// costs health and empties the stomach it was supposed to fill, which
/// is what a bad mushroom does and is deliberately *not* a status effect
/// -- see `food::harm`. There is nothing to cure and nothing to wait
/// out; there is a thing you should not have eaten.
pub const BLOCK_TOADSTOOL: BlockId = 108;

/// Dried meat: a cut that hung on a rack instead of over a fire.
///
/// **The rack's second trade, and the reason it is worth building before
/// the weather turns.** Cooking is the better meal and always will be --
/// the fire keeps its argument -- but drying happens while you are
/// somewhere else, costs no fuel, and turns a good hunt into a shelf of
/// food instead of a pile of raw cuts eaten at a loss. The frame that
/// cures a skin cures a haunch by the same rules: sun and wind do it,
/// rain stops it, smoke stands in for the sun -- see `rack::cures_into`.
pub const BLOCK_DRIED_MEAT: BlockId = 109;

// ---- wool ----
//
// What a sheep is for, and the third answer to the cold.
//
// The two that came before it are opposites and neither is good enough
// on its own: leather is warm *and* sheds rain, but not very warm; metal
// is armour and a refrigerator. Wool is the extreme -- the warmest thing
// in this world by a wide margin, worth nothing at all in a fight, and
// **useless the moment it is wet**, which is exactly what wool is.
//
// That is the point of it as a piece of play rather than a piece of
// content. It makes the far north reachable without metal, and it makes
// weather a decision: a player crossing the tundra in wool is warm until
// it rains, and what they do about that -- shelter, a fire, a leather
// cap over the top -- is a plan they had to have. See
// `equipment::garment` for the numbers and `body` for what they buy.

/// A fleece, off the animal. Also a block, because it is soft.
///
/// Placeable on purpose: a bale of wool is the one building material
/// here that is not stone, wood or earth, and a floor of it is
/// something a player will lay for the look of it. It breaks instantly
/// by hand, weighs almost nothing, and stops no light -- it is a
/// furnishing, not a wall.
pub const BLOCK_WOOL: BlockId = 110;

/// The four garments, in slot order, head down -- the same order the
/// leather and the metal sets are in above.
///
/// They share the leather set's four pictures and are told apart by
/// colour alone; see `garment_tint`. Four more images for four more
/// garments would be four more layers of a texture array that has a
/// fixed ceiling, to draw the same cap in a different wool.
pub const BLOCK_WOOL_CAP: BlockId = 111;
pub const BLOCK_WOOL_TUNIC: BlockId = 112;
pub const BLOCK_WOOL_LEGGINGS: BlockId = 113;
pub const BLOCK_WOOL_BOOTS: BlockId = 114;

/// Brickwork: bricks laid into a wall.
///
/// The first building material in this world that is *made* rather than
/// dug up, which is the whole of why it is here -- a player who has got
/// this far has a kiln, a clay pit and a reason to use both, and what
/// they get for it is the one block in the palette that says somebody
/// built this rather than found it.
pub const BLOCK_BRICKS: BlockId = 69;

// ---- the rock under the soil, in three kinds ----
//
// The world used to be one stone from the beach to the summit, and it
// read as a texture rather than as a country: nothing about a cliff
// told you where you were. Three rocks now, laid by the generator
// where their kind of rock forms -- see `worldgen::stratum` -- and each
// one is a *reason to go somewhere*, which is the rule every addition
// to this game answers to. Sandstone is soft and lies under sand; a
// desert is the easy quarry. Limestone lies under the lowlands and it
// is where flint nodules sit, so the knapper's stone has a home rather
// than a chance. Granite is the mountain, and it will not be broken by
// flint: the first thing a copper pick opens is the high country.

/// Sandstone: sand that has been sand long enough. Under deserts and
/// beaches, soft, and the block a desert builder has plenty of.
pub const BLOCK_SANDSTONE: BlockId = 124;

/// Limestone: the pale rock of the lowlands, and the rock flint forms
/// in. A nodule of flint in the world is in limestone or it fell out of
/// it.
pub const BLOCK_LIMESTONE: BlockId = 125;

/// Granite: the mountains' own rock. Harder than flint can bite, so a
/// mountain is a place a copper pick opens and a flint one only walks
/// over.
pub const BLOCK_GRANITE: BlockId = 126;

// ---- what dripping water leaves in a cave ----
//
// Two ids at 223, in the gap handed out for them, away from the hands
// filling 214-222 and 227 onwards. See `dripstone` for what they are for
// and the three uses that were turned down.

/// **A stalagmite: a spike of rock standing on a cave floor**, in three
/// sizes and a shaft (the variant, `dripstone::size`) -- a column of two to
/// four cells is shafts under one tip (`dripstone::column`). Grown where the
/// rock is one the water dissolves and comes down through
/// (`dripstone::forms_in`, `worldgen`), and the thing a player drops onto at
/// the cost of a cut (`dripstone::SPIKE_FROM_BLOCKS`).
pub const BLOCK_STALAGMITE: BlockId = 223;
/// ...and a stalactite, the same spike hung from the roof over it. Held from
/// above (`support_at`), so taking the rock it hangs from takes it down.
pub const BLOCK_STALACTITE: BlockId = 224;

// ---- the bog's fuel ----

/// Peat, as it lies in a bog: sodden, dark, dug by hand. Worthless as
/// it comes out; set down on open ground (`can_be_set_down`) the sun and the
/// wind turn it into `BLOCK_DRYING_PEAT` and then into `BLOCK_DRIED_PEAT`,
/// which is the reason to go into a bog with a shovel. It used to dry on the
/// rack; the rack is the larder now (`rack::Trade`).
pub const BLOCK_PEAT: BlockId = 127;

/// Dried peat: a brick of it, dried on the ground. Burns long and low -- longer
/// than a log, shorter than coal -- and it is the fuel of a country with
/// no trees worth felling. See `hearth::fuel_seconds`.
pub const BLOCK_DRIED_PEAT: BlockId = 128;

// ---- what a carcass is made of ----
//
// An animal used to die into a heap of meat and hide on the ground. It
// dies into a *carcass* now -- a block where it fell -- and a carcass is
// taken apart with a knife, one cut at a time, in the order a butcher
// works: the skin comes off whole first, then the sinew along the back,
// then the bones, then the meat. See `animals::Species::butchering` for
// the yields and `primitive_server` for the knife.

/// Sinew: the tendon along an animal's back, the strongest cord this
/// world has before rope. It is what a metal head is lashed to its haft
/// with -- see the recipes -- and a flint one can be lashed with it
/// instead of fibre.
pub const BLOCK_SINEW: BlockId = 129;

/// Bone. Comes out of every carcass after the sinew; a material, and
/// the first thing a butchered animal leaves that is not food.
pub const BLOCK_BONE: BlockId = 130;

/// The carcass of a hare: a small heap, two eighths high, that a knife
/// takes apart in four cuts.
pub const BLOCK_CARCASS_HARE: BlockId = 131;
/// The carcass of a deer.
pub const BLOCK_CARCASS_DEER: BlockId = 132;
/// The carcass of a boar.
pub const BLOCK_CARCASS_BOAR: BlockId = 133;
/// The carcass of a wolf.
pub const BLOCK_CARCASS_WOLF: BlockId = 134;
/// The carcass of a sheep: the fleece comes off first, before the skin.
pub const BLOCK_CARCASS_SHEEP: BlockId = 135;

// ---- the tool chain, made honest ----
//
// The player put it plainly: a *flint* axe is not a thing. Flint is
// what you knap an edge from -- a knife, a spear -- because it splits
// sharp; an axe is a hard stone ground against another stone, and it
// is the haft and the binding that are the work, not the head. So the
// stone tools are stone (`BLOCK_STONE_AXE`, `BLOCK_STONE_PICKAXE`, the
// old flint ones renamed), a head is lashed on with cord or sinew, and
// a lashing alone does not hold against a tree: for that the joint is
// set in glue, which is pine resin cooked with charcoal. See
// `crafting.rs` for the chain and `blocks.rs` for what each tier can
// bite.

/// Cord: fibre twisted into a length of rope. Fibre is slow to gather
/// and a cord takes a handful of it, which is the first real price of
/// a hafted tool; sinew does the same job and comes off a carcass.
pub const BLOCK_CORD: BlockId = 136;

// ---- the raft ----
//
// Three things in the pack and one on the water. See `raft` for the body
// and `crafting` for the chain: a sail and a pair of oars are made first,
// each of them a thing in its own right, and the raft is lashed together
// out of them and a great deal of timber, cord and rawhide.

/// A square of leather or cloth laced to a yard: what drives a raft
/// downwind. Only ever a part -- it has nothing to be hoisted on until it
/// is lashed to a mast.
pub const BLOCK_SAIL: BlockId = 265;
/// An oar: a board shaped to a blade on a pole.
pub const BLOCK_OAR: BlockId = 266;
/// A raft, rolled up in the pack, waiting for water to be launched onto.
///
/// **Heavy on purpose** (see its row in `blocks`): a raft carried over a
/// ridge is a player walking slowly for a long way, so where it is built
/// is a decision -- beside the water where the timber has to be hauled,
/// or in the woods where the raft does.
pub const BLOCK_RAFT: BlockId = 267;

// **137 is resin again**, and 138 (glue) is still free. The glue chain is
// gone: tapping a trunk for resin, cooking it with charcoal, and
// setting a lashed head in the result was three steps and one of them
// was a wait, and what it bought was a joint that held. A joint that
// holds is a *wedge* -- the head driven into a split haft and pinned --
// which is one step, uses the peg the player already makes
// (`BLOCK_PEG`), and is what actually held a stone axe together. The
// ids are left unused rather than reissued: a save written last week
// has resin in a chest, and handing that chest a different item would
// be worse than handing it nothing.
//
// **Which is exactly why resin came back on its own number.** The standing
// torch wanted a wad that burns for twenty minutes and shrugs off rain, and
// that is what resin is. A chest from before the glue chain went holds
// resin, and it holds resin again -- the one reissue that hands a save the
// same thing it had.

/// **Resin**: the pitch that bleeds from a scored trunk. A knife on a
/// standing trunk takes the bark off that cell and a lump of resin with it
/// (`primitive_server`'s `tap_trunk`), and what it is for is the standing
/// torch (`BLOCK_STANDING_TORCH`) and re-wadding one that has burnt out.
pub const BLOCK_RESIN: BlockId = 137;

/// A stone axe whose head is driven into a split haft and pinned: the
/// first tool that fells a tree. The lashed-only axe is
/// `BLOCK_STONE_AXE` and stops at brush.
pub const BLOCK_WEDGED_AXE: BlockId = 139;

/// The pick on the same terms: lashed, it turns earth and gravel;
/// wedged, it breaks stone.
pub const BLOCK_WEDGED_PICKAXE: BlockId = 140;

/// A flint spear: a knapped point on a long haft. What flint is for --
/// it pierces -- and the hunter's weapon before metal.
pub const BLOCK_FLINT_SPEAR: BlockId = 141;

/// A stone with rust on its face, lying on a riverbank or in a bog:
/// bog iron, the iron a people without mines ever had. Many of them,
/// crushed to dust and smelted long, make a bloom. See `BLOCK_IRON_DUST`.
pub const BLOCK_RUSTY_STONE: BlockId = 142;

/// Iron-bearing dust, knocked off the rusty faces of many stones. What
/// the bloomery is fed when there is no ore.
pub const BLOCK_IRON_DUST: BlockId = 143;

// ---- what an honest smelt leaves, and what an edge is kept on ----
//
// Four ids in the gap at 240, away from the fires at 440 and the wild
// plants at 470: several hands add blocks at once in this file, and a gap
// nobody else is filling is the one place two of them cannot hand out the
// same number.

/// A whetstone: a bar of sandstone a tool is honed on. It comes back from
/// every hone (the "hone" rows in `crafting`) -- a stone wears too slowly to
/// be worth counting, and what honing really costs is metal. See
/// `tools::hone`.
pub const BLOCK_WHETSTONE: BlockId = 240;

/// Slag: the glassy waste a smelt runs off the metal. What a crucible and a
/// bloomery leave in the tray beside the ingot and the bloom
/// (`hearth::byproduct`), and heavy dark stone to build with -- which is what
/// old slag heaps were dug for.
pub const BLOCK_SLAG: BlockId = 241;

/// A bar of iron carburised in a closed kiln: steel, before it is quenched.
/// The quench is the tool recipe -- see the "steel axe" rows in `crafting`
/// and `tools::HARDENED`.
pub const BLOCK_STEEL_INGOT: BlockId = 242;

/// Stream tin: cassiterite pebbles on the banks of rivers in tin country.
/// Picked up like a pebble, and what comes up is tin ore -- the placer a
/// people panned long before anybody drove a shaft into granite. See the
/// ground cover in `worldgen`.
pub const BLOCK_STREAM_TIN: BlockId = 243;

// ---- wild bees ----
//
// Three ids at 248..=250, handed to this work for the reason the gap at 240
// was: several hands add blocks to this file at once. See `bees` for the
// raid, the stings and the smoke.

/// **A wild hive: comb on the side of a trunk, in a warm wood.** Found, not
/// built -- not placeable, for the nest's reason (`BLOCK_NEST`): a hive a
/// player can put where they like is a farm, and this is a place to
/// remember.
///
/// **The variant is how much honey is in it** (`bees::honey_in`), nought to
/// `bees::HIVE_FULL`. The generator grows it full; breaking a hive with honey
/// in it gives the honey and leaves the empty comb (`block_residue`), which
/// fills again a comb at a time on the growth clock (`ripens_into`); breaking
/// the empty comb gives beeswax and takes the hive away for good.
///
/// **One id with a count rather than the nest's two ids**, because there are
/// four states and not two: a hive raided half-full gives half the honey, and
/// the emptier it is the harder it stings (`bees::stings`). Two ids would say
/// "full" and "not", and the not-full hive would have to forget how much was
/// left in it.
pub const BLOCK_WILD_HIVE: BlockId = 248;

/// Honey, out of a wild hive. The best mouthful in a wood that needs no fire,
/// and **it keeps** (`food::rot_per_step`): the one food a forager can carry
/// through a winter without a rack or an oven.
pub const BLOCK_HONEY: BlockId = 249;

/// Beeswax: the comb of a hive taken apart. It wads a torch as tallow does
/// (the "wax torch" rows in `crafting`), and what it cost was the hive.
pub const BLOCK_BEESWAX: BlockId = 250;

// ---- fishing ----
//
// Three ids at 235..=237, handed to this work for the reason the gap at 240
// was: several hands add blocks to this file at once. See `fishing` for the
// two ways of taking a fish that are not a spear, and for the ways that were
// weighed and left out.

/// **A fish trap: a basket of reeds with a funnel mouth, set in the water
/// and left.** The early way, and the one that needs no metal and no knife.
///
/// A block rather than something thrown into a cell of water, and a solid
/// one rather than a thing standing in the sea (`BLOCK_KELP`), because the
/// sea's bargain would cost the trap what it exists to hold: a liquid's
/// variant is its depth, the flow rewrites a liquid cell as plain water the
/// moment a level moves round it, and winter turns the top cell of a pond
/// to ice. A kelp stem lost that way is a plant a player never owned; a
/// trap lost that way is an evening's cord and the fish in it. So the trap
/// displaces the water it is set in, the way a stone dropped into a lake
/// does, and the water round it stays the water.
///
/// **The variant is how many fish are in it** (`trap_catch`), up to
/// `fishing::TRAP_HOLDS` -- written into the world, so a full trap is full
/// on the wire, in the save and after a restart without a file of its own
/// for the count. What does need a file is *which cells are traps*, so the
/// clock can find them: see the server's `logic::fishing`.
pub const BLOCK_FISH_TRAP: BlockId = 235;

/// **A fishing rod: a haft, a line of cord and a copper hook.** The later
/// way, and the better one where the water is deep. Worn a little by every
/// fish it lands (its `durability`), like a knife by every cut. See
/// `fishing` for why the hook is copper and not bone.
pub const BLOCK_FISHING_ROD: BlockId = 236;

/// A copper hook, four to an ingot at the forge: what a rod is made from and
/// the whole of what makes it wait for copper.
pub const BLOCK_COPPER_HOOK: BlockId = 237;

/// **A worm**, out of dug soil or a dung heap. The everyday bait: see
/// `fishing::Bait`, which is where what each one catches is decided.
///
/// **Ids up in the seven hundreds rather than in the low hundreds**, because
/// the low numbers are the blocks a world is built of and these three are
/// items nothing is built of. Well clear of the last one in use: the gap
/// after it is where the next few things to be added will go, and two things
/// landing on one number is a block that silently becomes another block in
/// every old save.
pub const BLOCK_WORM: BlockId = 720;

/// **A grub**: the insect that was in the tuft of grass. What a trout is
/// already eating, and the bait that comes free with the fibre.
pub const BLOCK_GRUB: BlockId = 721;

/// **A fishing fly**: a feather bound to a hook's length of cord so it looks
/// alive in the water. The one bait that is made rather than found, and the
/// one a fish does not eat off the hook (`fishing::Bait::keeps`).
pub const BLOCK_FISHING_FLY: BlockId = 722;

/// Food that has gone off. Everything perishable ends here if it is
/// carried long enough -- see `food` for what rots and how fast -- and
/// eating it is a mistake on the toadstool's terms.
pub const BLOCK_ROTTEN: BlockId = 144;

/// **A pat of dung**, left by a player some while after eating: an eighth
/// of a block on the ground, and filth to anybody's comfort within a few
/// steps of it (`comfort::filth_worth`). A body that is due waits until it
/// is out of doors (`comfort::goes_now`), so it only lands on a floor when
/// somebody never leaves the house.
///
/// Broken by hand into nothing and loose like soil, so clearing it up
/// dirties the one who does (`comfort::GRIME_PER_FILTHY_BLOCK`). It gives no
/// item: carrying it about would be a way to spoil somebody else's house.
/// **Cleared off a furrow it is dug into it** and dresses it as ash does,
/// resting a tired one (`manure_the_furrow_under` on the server), so the
/// only dung a field gets is the dung that was dropped on it.
pub const BLOCK_DUNG: BlockId = 700;

/// The canopy of an apple tree with nothing on it.
///
/// Its own leaf rather than the oak's, and the tree's own *wood* is
/// still `BLOCK_LOG`: an apple trunk is a broadleaf trunk and a second
/// wood for it would be a second everything -- planks, felling, the
/// texture budget -- for a tree that differs in one thing only, which is
/// what hangs in it. See `worldgen::place_trees`, where the fruit is a
/// roll on top of the wood rather than a wood of its own.
pub const BLOCK_APPLE_LEAVES: BlockId = 145;

/// The same canopy in fruit: five to seven cells of an apple tree, the
/// rest of it plain (`worldgen::FRUIT_FEWEST`). Picked -- by a right click,
/// or by breaking it -- it goes back to `BLOCK_APPLE_LEAVES_PICKED` and
/// ripens again on the growth clock, exactly as a berry bush does.
pub const BLOCK_APPLE_LEAVES_FRUIT: BlockId = 146;

/// An apple leaf the fruit was taken from: the one cell of the canopy that
/// grows apples again.
///
/// **A variant of the bare leaf rather than the bare leaf itself**, and
/// the difference is the whole of the count. The growth mechanic samples
/// cells near the players and puts anything that ripens on the clock
/// (`growth::Growth::sample`); when every bare apple leaf ripened, the
/// sample found the canopy one leaf at a time and a tree that was grown
/// with six apples was, a few days later, a tree of forty. Only the cell
/// that *was* fruit remembers it, so a tree keeps the fruit it was grown
/// with and no more -- and a leaf a player carries home and puts down is
/// a leaf, not an orchard.
///
/// A new id was the other way and was not taken: it is a second name and
/// a second row in every leaf list for a cell that looks, breaks, burns
/// and falls exactly like the leaf beside it. The variant field is where
/// this codebase keeps state of that kind (a carcass's cuts, a barrel's
/// jugs), and everything that asks by *kind* -- the mesher's picture, the
/// felling, the birds -- already treats it as the leaf it is.
pub const BLOCK_APPLE_LEAVES_PICKED: BlockId = BLOCK_APPLE_LEAVES | (1 << VARIANT_SHIFT);

/// An apple. The one food a player can gather standing still, and the
/// reason to remember where a tree was.
pub const BLOCK_APPLE: BlockId = 147;

/// A wooden peg: a split, shaped length of hardwood driven through a
/// joint. Not a block you place -- it goes *into* something already
/// built, and what it buys is that the thing stops needing to be held
/// up. See `pegged_form`.
pub const BLOCK_PEG: BlockId = 148;

/// Planks with pegs driven through them: a joint that is fastened
/// rather than merely stacked.
///
/// **Its own block rather than a bit in the variant field**, and the
/// reason is that the player has to be able to *see* it. A flag in the
/// id would have been free -- planks spend no variant bits -- and would
/// have left the one thing the mechanic needs invisible: which joints
/// have been fastened and which are waiting to come down. The picture
/// is the ordinary planks with four peg heads in them, made from the
/// same file, so the wood still matches the wall it is in.
pub const BLOCK_PEGGED_PLANKS: BlockId = 149;

/// The same in birch, for the same reason a birch plank exists at all.
pub const BLOCK_PEGGED_BIRCH_PLANKS: BlockId = 150;

// ---- joinery: the frame, and the two ways of holding one together ----
//
// Two ids at 215, in a gap handed out for exactly this, away from the
// hands filling 214 and 223 onwards. See the "furniture" rows in
// `crafting` for why furniture goes through a frame and what the
// fastening decides.

/// Iron nails: a bar of wrought iron drawn out and cut at the kiln, a
/// dozen to the bar. What a box of butted boards is held together with,
/// and the one use of iron that is not an edge or a plate.
///
/// **An item, not a block with a place in the world**, on the peg's terms:
/// a nail is driven into something that is being made, and there is never
/// a nail standing in a cell to break.
pub const BLOCK_NAILS: BlockId = 215;

/// A joined frame: four rails and their joints, the carcass a chair, a
/// table, a bed and a pegged chest are built round.
///
/// **One item whichever way it was fastened.** It is pegged out of worked
/// sticks before there is iron and nailed out of plain ones after, and the
/// thing that comes off the bench is the same frame -- so the choice
/// between flint and iron is made once, here, and not again in a copy of
/// every row that uses one. See the "furniture" rows in `crafting`.
pub const BLOCK_FRAME: BlockId = 216;

// ---- the door ----
//
// Two ids at 217, in the gap after the frame a door is built round. See
// `BLOCK_DOOR` for why a door is two ids when a bed is one.

/// A board door: a frame boarded over, two cells tall, hung on one edge of
/// the lower cell and the one over it. **The lower half, and the item** --
/// what a player carries, puts down and gets back.
///
/// **What a door is for is the decision it makes about a wall**: shut, a
/// room is a room -- its smoke stays in (`wildfire::smoke_room`), the sky's
/// light does not come through, and nothing walks in; open, it is a hole
/// in the wall that all three go through. A wolf follows a player through a
/// door left open, and not through a shut one (`door_is_open`).
///
/// **Two ids, one for each half, and a bit for open** -- and the bed says
/// a variant bit for which half, so the difference has to be argued. The
/// field is three bits (`VARIANT_MASK`). A bed spends two on which way it
/// lies and the third on which half a cell is. A door needs all of that
/// *and* whether it is open, which is four bits, and there are three.
/// Weighed:
///
/// * **The sixteenth bit**, above the variant field. Free today, and it is
///   free because nothing reads it: every `id & VARIANT_MASK == 0` in the
///   game, the anti-cheat's `is_known_block`, the chunk packer and every
///   save would see a door's top half as a door with no variant and silently
///   strip the bit that says it is a top. A field of one used by one block
///   is a trap for the next.
/// * **The top reads its facing off the half under it.** Then the top half
///   is not a block that knows where its own slab is: the collider, the
///   placement check and the aim box all ask `geometry::block_box` about
///   one id with no world round it, and a top asked alone would be a slab
///   at the wrong face -- a door that stops you at its bottom half and lets
///   your head through the top.
/// * **An id for the top (chosen)**, which is the standing torch's
///   arrangement (`wildfire::standing_torch_partner`) for the same want: a
///   thing two cells tall whose halves are drawn differently. What it costs
///   is one more name in the table, and `is_door` keeps "is this a door" a
///   question about one thing.
///
/// **Open is a variant bit and not a third and fourth id**, because
/// opening a door is something a player does fifty times an evening, and
/// every rule that asks "is this a door" -- the break, the partner, the
/// mesher -- would otherwise be asked about four ids. `DOOR_OPEN`.
pub const BLOCK_DOOR: BlockId = 217;

/// The upper half of a board door. Written by the placement over the
/// lower half and never carried: breaking it gives the door, and takes the
/// lower half with it (`door_partner`).
pub const BLOCK_DOOR_TOP: BlockId = 218;

// ---- what comes off an animal, told apart ----
//
// **One picture, several names, different numbers.** A hare is not a
// bear and its meat should not be worth the same, but a texture layer
// per species would spend the atlas on a distinction the eye makes from
// the *animal*, not from the icon (see CLAUDE.md on the 256 layers). So
// these share `food/raw_meat.png` and `hide/leather.png`'s neighbours,
// and differ where it matters: what they are called and what they are
// worth.
//
// Four meats rather than seven. Deer, boar and sheep all give the same
// good red meat and always did (`BLOCK_RAW_MEAT`); what is genuinely
// different is small game, fowl, the fat-rich meat of a bear and the
// stringy meat of a wolf, and those are the four here.

/// Lean meat off a hare: half a meal, and the commonest thing a new
/// player kills.
pub const BLOCK_HARE_MEAT: BlockId = 151;

/// A bird. Light, quick to cook, and worth about what a hare is.
pub const BLOCK_FOWL_MEAT: BlockId = 152;

/// Bear meat: heavy and rich, the best mouthful in the world raw --
/// and the animal it comes off is the worst thing to meet.
pub const BLOCK_BEAR_MEAT: BlockId = 153;

/// Wolf meat: stringy and poor. Eaten by people who have run out of
/// choices, which is exactly what it is here.
pub const BLOCK_WOLF_MEAT: BlockId = 154;

/// A small fur: hare and wolf. Cures into leather like a hide and gives
/// less of it, being a smaller animal off a smaller frame.
pub const BLOCK_PELT: BlockId = 155;

/// A bear's hide: the biggest skin in the world, and the warmest.
pub const BLOCK_BEAR_HIDE: BlockId = 156;

/// A feather. Off a bird, and light enough that a stack of them weighs
/// nothing worth carrying about.
pub const BLOCK_FEATHER: BlockId = 157;

/// Rendered fat. What the thick-skinned animals carry and the lean ones
/// do not, and the thing that makes a torch burn long -- see the
/// "fat torch" recipe.
pub const BLOCK_FAT: BlockId = 158;

/// A rack of ribs: bone with meat still on it, which is what a carcass
/// gives up last. Roasts whole at a fire.
pub const BLOCK_RIBS: BlockId = 159;

/// Ribs off the fire: the biggest single meal in the game, and it takes
/// a whole animal and a fire to get one.
pub const BLOCK_ROASTED_RIBS: BlockId = 160;

/// A bear where it fell. See `animals::Species::carcass`.
pub const BLOCK_CARCASS_BEAR: BlockId = 161;

/// A bird where it fell. Small enough that the cell it lies in is
/// mostly air, which is what its thickness says.
pub const BLOCK_CARCASS_FOWL: BlockId = 162;

/// **Basalt: the floor of the world.** The fourth rock, and the only
/// one that is a *depth* rather than a place -- sandstone, limestone
/// and granite are what a region is made of, and this is what is under
/// all of them once you have dug far enough.
///
/// What it is for is the one thing the other three are not: a wall.
/// A flint pick will not scratch it and neither will copper, so the
/// bottom of the world is shut until there is bronze -- which turns
/// "how deep can I dig" from a question about patience into a question
/// about what you have made. See `worldgen::BASALT_FROM`.
pub const BLOCK_BASALT: BlockId = 163;

/// **The tinder fungus, growing on a fallen trunk.** The hard bracket
/// that lives on dead wood, and the reason it is in the game is a hole
/// in the map rather than a hole in the recipe book: a torch wants a
/// wad of fibre, fibre comes off grass, and the three places a player
/// most needs a torch -- the dead forest, the bog and the deep taiga --
/// are exactly the three where no grass grows. Before this, going into
/// the dark meant carrying the light in with you from a meadow.
///
/// **On a fallen log and nowhere else.** A bracket on the side of a
/// standing trunk is the picture everybody has of one, and it is not
/// what this is, because every support rule in this game names what is
/// *under* a block and nothing names a neighbour -- see `needs_support`
/// and `can_grow_on`. A support rule that could say "the cell beside
/// me" would be a second kind of support, checked in the generator, in
/// mining, in placing and in the collapse pass, and all of it for one
/// decoration. A deadfall gives the same fungus on the same dead wood
/// for none of that, and a deadfall is already where a player looks.
///
/// It is not food. Amadou is wood that stopped being a tree; it burns
/// slowly and takes a spark, and both of those it does here -- see
/// `hearth::fuel_seconds` and the "fungus torch" row in `crafting`.
pub const BLOCK_BRACKET_FUNGUS: BlockId = 164;

/// **The hide, worn as it comes off the animal.** A pelt over the
/// shoulders and a hood cut from another one -- no tannery, no bark
/// liquor, no rack: the first warm thing in this world that costs one
/// dead animal and an evening rather than a chain of three buildings.
///
/// It is the fourth material in the clothing table and it exists because
/// the other three left a hole in the *first week*. Leather is warm and
/// cheap to wear and expensive to make; wool is warmer and needs a
/// sheep, shears and a spindle; metal is neither. A player caught out by
/// their first cold night has a pelt in their pack and no answer, and
/// this is the answer -- see `equipment::garment` for what it costs
/// them, which is real: raw fur is heavy, it is stiff, and it is the
/// one garment in the game that will help drown you (see
/// `load::buoyancy`).
///
/// **Two pieces and not four.** A fur legging and a fur boot are the
/// same idea twice with nothing new to decide, and the two that cover
/// three quarters of a person (see `Slot::hit_share`) are the two worth
/// having. What that means in play is that a fur-clad player is warm
/// down the middle and cold at the ends, which is exactly the shape of
/// dressing in skins.
pub const BLOCK_FUR_HOOD: BlockId = 165;
pub const BLOCK_FUR_CLOAK: BlockId = 166;

/// **Wild cereal, standing in open country, and the only place seed
/// comes from now.**
///
/// Seed used to fall out of tall grass -- one tuft in four -- and that
/// was wrong in the way that is easy to miss: it meant a field cost
/// nothing to start and nothing to find. A player pulling up fibre in
/// their first minute was already holding the whole of agriculture, so
/// the moment somebody *became a farmer* never happened; it was a thing
/// that had quietly always been true.
///
/// Wild wheat is that moment put back. It is a stand of a real plant in
/// a real place -- open, dry, warm country and nowhere else -- and
/// finding it is the work. What it gives is seed, and what seed gives
/// is a field.
///
/// **It shares the ripe crop's picture**, deliberately and not to save
/// a texture layer (although it does): the thing a player has to learn
/// is what wheat looks like, and teaching them one silhouette that
/// means "grain" everywhere is better than teaching them two.
pub const BLOCK_WILD_WHEAT: BlockId = 167;

/// Ground grain, and the middle of the bread chain.
///
/// **Bread used to be two rows: three grain became dough in the hand,
/// and dough became bread at a fire.** The comment on that first row
/// admitted what it was -- threshing, grinding and wetting collapsed
/// into one step because a quern would have been another block, another
/// station and another screen. That is still true of a quern, and this
/// is not one: flour is ground *with a stone you already carry*, and
/// the stone comes back.
///
/// What the extra step buys is the water. Dough is flour and water, so
/// bread now needs the jug -- and the jug was a thing you filled for
/// thirst and nothing else. One item doing two jobs is worth more than
/// two items doing one each. See the "grind grain" and "dough" rows in
/// `crafting`.
pub const BLOCK_FLOUR: BlockId = 168;

/// A bird's nest in the branches, and the same nest with eggs in it.
///
/// The pair works exactly as the berry bush does: the full one is what
/// the world grows, breaking it gives what is *in* it and leaves the
/// empty one standing (`BlockDef::leaves_behind`), and the empty one
/// fills again on the growth clock (`ripens_into`). Nothing new had to
/// be built for any of that, which is the argument for this being two
/// ids rather than a variant or a second mechanic.
///
/// Why it is in the game: a bird was four feathers and half a meal, and
/// the only way to have either was to catch one. A nest is food that
/// **stays where it is** -- a place you can come back to, on a tree you
/// remember, which is the same thing an orchard is and the thing this
/// world is mostly made of.
pub const BLOCK_NEST: BlockId = 169;
pub const BLOCK_NEST_EGGS: BlockId = 170;

/// What is in it. Food raw, and poor -- an egg is a mouthful, not a
/// meal, and a nest that fed you would make the hunt optional again.
pub const BLOCK_EGG: BlockId = 171;

// ---- the four spears ----
//
// **One weapon in four materials, and the ladder is the point.** A
// spear was the one thing in the game that never improved: a knapped
// flint point, seventy thrusts, and the same number from the first
// evening to the iron age. Everything else a player makes has a better
// version of itself waiting further up the tech tree, and the thing
// they actually fight with did not.
//
// Bone comes *before* flint and is worse than it -- that is what makes
// it worth having, because it costs no knapping: a bone off the first
// animal you butcher, a stick and a sinew, and you are armed on your
// first evening instead of after the flint has shattered twice. The
// metals come after and are better in the two ways a metal point is
// better: it goes in deeper and it does not snap.
//
// **None of them costs a texture layer.** All four share the flint
// spear's picture and are told apart by colour, exactly as the twelve
// garments share four (`garment_tint`, and `spear_tint` beside it).
// That is also the honest drawing: the difference between these is the
// head, and at hotbar size the head is four texels.
pub const BLOCK_BONE_SPEAR: BlockId = 172;
pub const BLOCK_COPPER_SPEAR: BlockId = 173;
pub const BLOCK_BRONZE_SPEAR: BlockId = 174;
pub const BLOCK_IRON_SPEAR: BlockId = 175;

// ---- furniture, and the thing before furniture ----
//
// **Why a game about flint has chairs in it.** Every block up to here
// is either a material, a tool or a machine: something you spend or
// something that does work. Furniture is the first thing in this world
// that exists because a *person* lives here -- and the mechanism that
// makes it more than decoration is sleep, which is the one thing a body
// cannot do standing up.
//
// Three of the four are made of planks, and none of them costs a new
// picture: their faces are the same boards and the same hide the rest
// of the game is built from (see `assets/textures/blocks.toml`). That
// is not only thrift -- a stool made of planks should *look* like the
// planks it was made of.

/// A pallet of dry grass to lie on, and the first bed anybody has.
///
/// Two eighths of a cell high and two cells long, like the plank bed (see
/// [`is_bed`] for why it stopped being one cell), made of fibre, made in
/// the first hour. It is
/// deliberately poor: sleeping on it passes the night and leaves you
/// stiff -- less of the tiredness goes than in a real bed (see
/// `body::Rest`) -- which is what makes the bed worth building later
/// without making the straw a mistake now.
pub const BLOCK_STRAW_BED: BlockId = 176;

/// A bed: a plank frame, a hide over it, and the best sleep there is.
///
/// The point at which a camp becomes a house. It is not warm by itself
/// -- warmth is the roof and the hearth (see `body`) -- it is *rest*,
/// and it is the only place a whole night's tiredness goes.
pub const BLOCK_BED: BlockId = 177;

/// A stool. Half a cell of boards, and a place to sit.
///
/// Sitting is not sleeping and does not pass time: it rests you while
/// you stay on it, which is what somebody at a fire is doing between
/// jobs. It also does the thing furniture does that no stat measures --
/// it makes a hut read as somewhere somebody lives.
pub const BLOCK_STOOL: BlockId = 178;

/// A table. Waist high, boards on top, and the one piece of furniture
/// that is honestly only furniture.
///
/// It rests nobody and cooks nothing. What it does is give a room a
/// middle, and a player who has built one has decided this place is
/// theirs -- which is the whole of why it is in the list.
pub const BLOCK_TABLE: BlockId = 179;

/// A chair: the stool's seat with four legs and a back, and it faces a way.
///
/// **What the back buys is a direction, and a direction is a decision.**
/// A stool is sat on however the player happened to be looking; a chair
/// is put down facing something -- the fire, the door, the valley -- and
/// whoever sits in it looks that way (`seat_yaw`). It also rests a body
/// half again as fast as a stool does (`body::sitting_recovery`), for a
/// plank and a stick more and a piece of furniture that no longer goes
/// anywhere a stool goes. So the stool stays the camp's seat and the
/// chair is the house's, which is the same split the straw and the bed
/// make for sleep.
///
/// The seat is at the stool's height and the collider stops there: the
/// back is drawn and aimed at (`geometry::block_box_for_aim`) but not
/// collided with, because one box is all a cell has and a box as tall as
/// the back would put a sitter's feet inside the chair.
pub const BLOCK_CHAIR: BlockId = 230;

/// **What a bush is made of**, and it is not the canopy's leaf.
///
/// The undergrowth was built out of `BLOCK_LEAVES` when it was added,
/// which was the cheapest thing that worked and read as exactly that: a
/// meadow with tree-tops lying in it. A bush is not a piece of a tree.
/// It sits in shade, it is dense enough that you cannot see through it,
/// and it has woody stems in it -- which is what the picture says now
/// (`assets/textures/plants/bush_leaves.png`, recoloured from the
/// canopy's, as new pictures here are).
///
/// Everything else about it is the leaf's: a cutout cube, tinted by the
/// climate it grew in, broken by hand, and given back when it is.
pub const BLOCK_BUSH_LEAVES: BlockId = 180;

// ---- savanna ----

/// **An acacia's canopy: a thin, flat, grey-green plate.**
///
/// A leaf of its own rather than the oak's, and the reason is the tint.
/// The oak leaf is foliage, and the hot, dry corner of the climate tint
/// is straw (`TINT_HOT_DRY` in the shader) -- right for grass, and wrong
/// for the only trees on a plain: a savanna oak came out the colour of
/// the ground under it, and a tree the colour of the ground is not a
/// thing the eye can measure a distance against.
///
/// **Not foliage, on purpose.** It grows in one climate and nowhere else,
/// so a tint would have had nothing to vary except towards the straw it
/// exists to stand out from. It wears the dusty grey-green its picture
/// has (`plants/acacia_leaves.png`, the oak's picture recoloured, as new
/// pictures here are). The other way -- tinted, with a bluer picture
/// painted to survive the tint -- was rejected: the tree would change
/// colour towards the savanna's edge for no reason a player could see.
///
/// Everything else is the oak leaf's: a cutout cube, pushed through
/// rather than walked into (`is_canopy`), broken by hand and given back.
pub const BLOCK_ACACIA_LEAVES: BlockId = 195;

/// **A termite mound: a spire of sun-baked red earth on the savanna.**
///
/// A block of its own because no block in the game reads as one.
/// Measured by average colour, clay is blue-grey (142, 146, 158) and
/// stands out of straw grass like a stone post; dirt is the dark brown
/// (67, 51, 35) of every hole a player has dug, and a column of it is
/// what a player's own scaffolding looks like; sandstone and limestone
/// are pale rock. The one thing a landmark cannot be is mistaken for
/// something a person left behind. The picture is dirt's, recoloured to
/// a red ochre.
///
/// **It breaks into clay**, because that is what a mound is: earth
/// cemented by the insects that built it, and dug for pots and ovens
/// wherever people and termites meet. Clay otherwise lies only at the
/// edge of still water (`worldgen`'s clay sites), so a player who wants
/// a kiln in dry country chooses between the walk to a river and the
/// walk to the last mound they passed. Harder than clay from a bank, as
/// a baked thing is, and not placeable: what a player carries away is
/// clay.
pub const BLOCK_TERMITE_MOUND: BlockId = 196;

/// **A maple's canopy: red-orange, the broadleaf a player can tell from an
/// oak across a valley.** One forest tree in five (`worldgen::place_trees`).
///
/// Its own leaf, untinted, for the acacia's reason turned round: the
/// climate tint pulls every foliage texel toward the green of where it
/// grew, and a maple that came out the same green as the oaks round it
/// would be an oak with a different name.
pub const BLOCK_MAPLE_LEAVES: BlockId = 197;

/// **Sandy soil: the savanna's bare ground between the grass.**
///
/// Turf everywhere reads as a meadow that got hot, whatever the tint
/// does -- and the grass tint in particular barely yellows the turf
/// picture, which has almost no red in it for the hot corner to multiply.
/// What makes grassland read as dry is the ground showing through it:
/// patches of warm, sandy earth where nothing much grows (see
/// `worldgen::build_column_tile`).
///
/// **A ground, not a rock.** Trees root in it -- an acacia or a baobab on
/// bare earth is what one looks like -- and so do termite mounds; a hoe
/// turns it into a field, so it is ground a player can use. What does not
/// grow on it is what grows on turf: tufts, flowers, bushes and the wild
/// wheat and cotton. What does grow on it is `BLOCK_DRY_GRASS`, thinly,
/// which is what a dry patch of grassland actually is.
pub const BLOCK_SANDY_SOIL: BlockId = 198;

/// **Dry grass: the savanna's tuft, on dry ground.** A tall-grass tuft gone
/// to straw, growing on sandy soil (`can_grow_on`) and scattered over the
/// savanna's bare patches (`worldgen::place_ground_cover`).
///
/// **Its own block rather than the tall grass in a hot tint.** The tint
/// multiplies green texels, and the tuft picture has almost no red for the
/// straw corner of it to work on -- tall grass on a savanna stays green,
/// which is what the savanna looked like before this. A straw picture with
/// no green left in it, untinted, is straw everywhere.
///
/// Cut, it gives fibre one time in two, on the tall grass's own rule: it
/// is the same grass, dried.
pub const BLOCK_DRY_GRASS: BlockId = 199;

/// **Dry turf: the savanna's floor.** Turf in the dry season -- straw and
/// faded olive instead of green -- and the one change that most stops a
/// savanna reading as a forest meadow with the trees recoloured.
///
/// **Its own block rather than a hotter tint on the grass.** The climate
/// tint multiplies green texels, and the turf picture has almost no red for
/// a straw tint to work with: the hot, dry corner took a meadow from green
/// to a slightly different green, which is what the savanna looked like.
/// A straw picture, untinted, is straw.
///
/// **Turf in every other respect**: it breaks into dirt, a hoe tills it,
/// acacias and baobabs root in it, the savanna's wild wheat and cotton,
/// its bushes and flowers grow on it, and its animals graze it. What does
/// not grow on it is the green tuft -- a dry savanna grows `BLOCK_DRY_GRASS`
/// instead (`can_grow_on`).
pub const BLOCK_DRY_TURF: BlockId = 210;

// ---- branching trees: pieces of branch ----
//
// A fir or a birch is a column of whole log cubes with a ball of leaves
// on it. A broadleaf tree, an acacia or a baobab (`Preset::grows_branches`)
// grows out of *pieces*: a trunk that is thick at the foot and
// thinner up the stem, limbs that thin again as they reach out, and
// twigs at the ends. Each piece is a post of some thickness, and the
// mesher joins it to the wood beside it -- see `mesh::branch_block`.

/// **A thin piece of a tree**: a twig, the tip of a limb, the whole stem
/// of a sapling. Two, four or six sixteenths across, in the variant field
/// (see [`branch_width`]).
///
/// **Two ids for a thin and a thick piece, and thickness inside each.**
/// Three ways were weighed:
///
/// * *One id, the thickness in the variant, and every rule asking it.* A
///   twig is pulled off by hand and gives a stick; a trunk wants an axe
///   and gives a log. Those are the break time, the tool gate and the
///   drop -- predicates that read the block's row, and each would need to
///   learn to read the variant instead. The first one forgotten is a
///   sapling that needs an axe. (The collider was a fourth, and is not any
///   more: both pieces are walked into at their own width -- `branch`.)
/// * *One id per thickness.* Eight ids of timber, and the mesher's "is
///   this wood I join to" is a match on eight.
/// * **Two ids, thickness in the variant (chosen).** The line between
///   them is a line a player can see -- what their hands can do -- so it
///   is a line in the table, and every rule keeps reading the row it
///   always read. The exact width is only the mesher's business.
///
/// **Not placeable.** A piece is a part of a grown tree; what a player
/// carries away is sticks and logs.
pub const BLOCK_TWIG: BlockId = 211;
/// **A thick piece of a tree**: the trunk and the lower limbs. Eight to
/// sixteen sixteenths across. Timber in every rule -- an axe to cut, a log
/// when cut -- and solid to walk into. See [`BLOCK_TWIG`] for why it is a
/// second id.
pub const BLOCK_BOUGH: BlockId = 212;

// ---- the sea floor ----
//
// What grows under the sea, and what comes out of it. The ids start at
// 280 rather than straight after the last one because several pieces of
// work add blocks at once, and two of them counting up from the same
// number is two blocks with one id -- a collision `every_row_is_reachable_
// and_unique` catches only after both have landed.

/// **A stem of kelp: a plant that is also the water it stands in.**
///
/// The one shape of block this world had no room for. Everything that
/// stands in a cell -- a tuft, a reed, a flower -- stands in *air*, and
/// the sea is made of cells that are water. Three ways to put a plant in
/// one were weighed:
///
/// * *A plant, and water round it.* Solid-matter cross, as a reed is. The
///   cell is then not water: a player swimming up the kelp is standing in
///   a dry chimney, the underwater fog switches off inside it, the mesher
///   draws the surface of the sea as a wall round every stem, and the flow
///   simulation -- which floods anything that stands in a cell -- washes
///   the forest away the first time a player digs beside it. Every one of
///   those is a separate rule to teach, in a separate crate.
/// * *A second layer of state: a "waterlogged" bit beside every id.* Right
///   for Minecraft, which has one; here it is a field on the wire, in the
///   save and in every chunk, for a dozen blocks.
/// * **Water that happens to have a plant in it (chosen).** The row says
///   `Matter::Liquid`, so everything that asks "is this water" -- the
///   swimming, the drowning, the fog, the collider, the flow, the lighting
///   under the sea, the faces the mesher culls between two cells of sea --
///   already gives the right answer without being told. What had to learn
///   is the short list of things that must *not* treat it as water:
///   what a ray may stop at (`is_targetable`), what breaking it leaves
///   (`block_residue`), and how the mesher draws it (the plant, and then
///   the water). See [`stands_in_water`].
///
/// The cost, said plainly: **the kelp is the water, so it goes where the
/// water goes.** A cell of sea that gives some of itself away to a hole a
/// player dug beside it is written back as ordinary water, and the plant
/// is gone. That is a forest you can drain, which is a truthful thing for
/// a sea plant to be.
pub const BLOCK_KELP: BlockId = 280;
/// The top of a kelp stem: the fronds that float up toward the light.
/// A second id rather than a variant, because the variant of anything
/// liquid is its depth (`has_depth`).
pub const BLOCK_KELP_TOP: BlockId = 281;
/// Seagrass: a low meadow on the sand of warm and temperate shallows.
/// Liquid for `BLOCK_KELP`'s reason. What it gives is fibre, so a coast
/// without a meadow behind it still has cordage in it.
pub const BLOCK_SEAGRASS: BlockId = 282;
/// A sea fan: a purple lattice standing on a reef. Liquid for
/// `BLOCK_KELP`'s reason; see `BLOCK_BRAIN_CORAL` for what a reef gives.
pub const BLOCK_SEA_FAN: BlockId = 283;
/// Staghorn coral: an orange thicket of branches on a reef.
pub const BLOCK_STAGHORN_CORAL: BlockId = 284;
/// **Brain coral: the stone a warm reef is built of**, in ochre.
///
/// A reef is a place a player finds, and what it is worth finding for is
/// colour: the only building stone in the game that is not grey, brown or
/// sand. Mined like limestone -- it *is* lime, laid down by animals -- so
/// it needs the pick limestone needs and gives nothing a quarry does not
/// already give except the colour. **What it deliberately does not give**
/// is a shortcut: coral burnt to lime, a flux for the bloomery, a
/// whetstone. Each was considered and each would make the equator the
/// place every metal age starts, which is a decision the latitude is not
/// supposed to make for a player.
pub const BLOCK_BRAIN_CORAL: BlockId = 285;
/// Fire coral: the reef's second stone, in red. See `BLOCK_BRAIN_CORAL`.
pub const BLOCK_FIRE_CORAL: BlockId = 286;
/// A shell lying on the sand of the shallows. Liquid for `BLOCK_KELP`'s
/// reason, and flat for the pebble's. Decoration and nothing else: see
/// its row for why it drops nothing.
pub const BLOCK_SHELL: BlockId = 287;
/// What a stem of kelp gives: a frond, as an item. Its own id because
/// the stem is water (`BLOCK_KELP`), and a pack slot holding a liquid is
/// a jug nobody made.
pub const BLOCK_KELP_FROND: BlockId = 288;
/// A frond dried on the rack: a light meal that keeps. See `rack::cures_into`.
pub const BLOCK_DRIED_KELP: BlockId = 289;
/// A fish, whole and raw. See `animals::Species::Fish`.
pub const BLOCK_RAW_FISH: BlockId = 290;
/// ...and cooked over a fire.
pub const BLOCK_COOKED_FISH: BlockId = 291;

// ---- the palm and the swamp ----
//
// Ids from 356, well clear of the sea floor's 280s and the dressings' 340s,
// for the sea floor's reason: several pieces of work add blocks at once, and
// two of them counting up from the same number is one id for two blocks.

/// **A piece of a palm's trunk**: a ringed, slender post, eight to sixteen
/// sixteenths across in the variant exactly as a bough is.
///
/// **A piece of branch rather than a log**, and three ways were weighed:
///
/// * *`BLOCK_LOG` cubes, as the oak and the acacia are.* A palm is recognised
///   by being thin and curved, and a stack of whole cubes a metre wide is a
///   chimney with a crown on it -- the silhouette would be the one thing a
///   player could not see.
/// * *A wood of its own* -- a palm log, palm planks, felling taught a third
///   timber. A second everything for a tree whose wood nobody builds with
///   differently, and palm timber is ordinary timber here: what a palm is
///   for is its fruit.
/// * **A branch piece with its own bark (chosen).** The mesher already draws
///   a piece as a post of its width and joins it to the piece above and
///   across a corner (`mesh::branch_block`), which is a curved stem; felling
///   already brings a tree of pieces down by what holds it up
///   (`felling::fell_branches`); and a piece already gives a log. What this
///   id adds is the bark, and the one rule that must not confuse a palm with
///   a sapling (`growth::young_root`).
pub const BLOCK_PALM_TRUNK: BlockId = 356;
/// A palm's crown: long fronds reaching out from the top of the trunk and
/// drooping at their tips. A leaf in every rule -- pushed through, felled
/// with the tree, perched in -- and cut out, so a frond reads as a frond.
pub const BLOCK_PALM_FRONDS: BlockId = 357;
/// **The fronds with coconuts in them**, under the crown where the trunk
/// meets it. Picked by hand, as an apple is (`picks_by_hand`), and what is
/// left is `BLOCK_PALM_FRONDS_PICKED`, which sets fruit again in warm air.
pub const BLOCK_PALM_COCONUTS: BlockId = 358;
/// The frond a cluster of coconuts was picked from: the one cell of a crown
/// that fruits again. A variant of the frond for `BLOCK_APPLE_LEAVES_PICKED`'s
/// reason -- a crown that fruited everywhere the growth sampler touched it
/// would be a larder, not a palm.
pub const BLOCK_PALM_FRONDS_PICKED: BlockId = BLOCK_PALM_FRONDS | (1 << VARIANT_SHIFT);
/// **A coconut: food, and clean water.** See `food::water_in` for why the
/// water is the point of it -- on a coast whose only water is the sea, the
/// palm is the well.
pub const BLOCK_COCONUT: BlockId = 359;
/// **Mud**: the floor of a swamp between its pools, wet earth that holds a
/// boot. Slow to walk on (its row's `drag`), and what it gives when dug is
/// clay -- the reason a potter wades into a swamp at all.
pub const BLOCK_MUD: BlockId = 360;
/// A lily pad lying on still water: flat, and held up by the water under
/// it (`can_grow_on`). Walked through, not on -- a player who trusts one is
/// a player in the swamp.
pub const BLOCK_LILY_PAD: BlockId = 361;
/// Moss hanging under a swamp tree's crown. **Held from above** -- the one
/// plant in the world that is (`support_at`) -- so it falls with the leaf it
/// hangs from, rather than floating where the crown was.
pub const BLOCK_HANGING_MOSS: BlockId = 362;

/// **A twig standing in water, and the water round it.** See
/// [`BLOCK_DROWNED_BOUGH`]; a twig is walked into at its wood, as a dry one
/// is (`branch`), and the rest of its cell is the water a player swims in.
pub const BLOCK_DROWNED_TWIG: BlockId = 390;
/// **A bough standing in water, and the water round it**: the foot of a
/// drowned snag in a swamp pool.
///
/// "болотные ветки не заполнены водой и вытесняют ее": a snag was pieces of
/// branch written over the pool's cells, and a piece of branch is a post in
/// a cell of air. Every pool with a snag in it had a square hole round the
/// wood, walled by the water beside it, down to the mud.
///
/// Three ways to put the water back were weighed:
///
/// * *A waterlogged bit in every block id.* Bit fifteen is free, and every
///   rule in the game strips the variant and asks the kind -- so every rule
///   that must see water would have to learn to ask the bit as well: the
///   mesher, the fluid simulation, swimming, the light, the drink.
/// * *Water drawn round a dry piece by the mesher*, when water stands beside
///   it. A picture: the fluid simulation would still find a wall there, a
///   swimmer a hole, and the mesher would have to look two cells out to hide
///   the wall of surface the water beside it draws against the wood.
/// * **Wood that is water by its row (chosen)**, as kelp is
///   ([`BLOCK_KELP`]): liquid to every rule that asks, so the pool round it
///   is one body of water, and a piece of branch to the rules about wood --
///   the mesher's post, felling, the axe and the log. What it adds to kelp's
///   bargain is short and each line of it is at its site: a bough is still
///   walked into ([`is_collidable`]), its width is not a depth
///   ([`has_depth`]), and water does not wash it away
///   ([`can_be_displaced_by_falling`]). Breaking or felling one leaves the
///   water ([`block_residue`]).
///
/// Two ids rather than one with the twig's widths below the bough's in its
/// variant, so the axe, the log and the stick stay in the table rows they
/// were already in.
pub const BLOCK_DROWNED_BOUGH: BlockId = 391;

// ---- fires in the ground: the pit kiln, the charcoal pit, the firepit ----
//
// See `pit` for all three and for TerraFirmaCraft's versions of them. The
// ids start at 440 rather than at the next free number because other work
// was adding ids above 391 at the same time, and two sets of constants that
// happen to agree on a number are two blocks that are one block.

// 440 was hay, fibre dried on a rack to pack a pit kiln with. The player
// asked for the kiln to take fibre itself ("нужно для печи использовать
// волокно а не сено"), and an item whose only use was that one was dead
// weight in the pack, so it is gone and the number is not reused.
/// **A brick, shaped and not yet fired.** The pit kiln fires what is put in
/// it, and every other piece of pottery already had a raw state; the brick
/// was clay in the kiln's recipe and nothing in between, so there was
/// nothing to lay in a pit. Clay itself would not do: a lump of clay in the
/// hand is a *block* and a right click with it builds.
pub const BLOCK_BRICK_RAW: BlockId = 441;
/// A pit kiln with pottery on its floor and nothing over it. See
/// `pit::Stage` for what the variant field holds at each of these four.
pub const BLOCK_PIT_KILN: BlockId = 442;
/// ...with fibre over the pottery.
pub const BLOCK_PIT_KILN_FIBRE: BlockId = 443;
/// ...with eight fibre and logs over that.
pub const BLOCK_PIT_KILN_LOGS: BlockId = 444;
/// ...alight. One kind with nothing in its variant: a burning kiln is
/// always the full one, because only the full one can be lit.
pub const BLOCK_PIT_KILN_LIT: BlockId = 445;
/// A pile of logs laid for charcoal: the variant is how many, one to eight.
pub const BLOCK_LOG_PILE: BlockId = 446;
/// The same pile burning under its cover. The count stays in the id.
pub const BLOCK_LOG_PILE_LIT: BlockId = 447;
/// What a covered pile leaves: a heap of charcoal, the variant how much.
pub const BLOCK_CHARCOAL_PILE: BlockId = 448;
/// **A firepit: TerraFirmaCraft's fire laid on bare ground** -- three
/// sticks and a log struck where they lie. A hearth on every rule a
/// campfire is one (`hearth::Kind::Campfire`), with one difference that is
/// the reason it is its own id: there are no stones in it, so breaking one
/// gives nothing back. Written as a campfire it would drop a ring of three
/// cobblestones for three sticks and a log.
pub const BLOCK_FIREPIT: BlockId = 449;
/// ...alight.
pub const BLOCK_FIREPIT_LIT: BlockId = 450;

// ---- fire that gets loose, and the fire you stand up ----
//
// Ids from the top of the index down (`blocks::INDEX_KINDS` is 512), for
// the sea floor's reason and the wild plants': other work adds ids upward
// from 480 at the same time, and two sets of constants that agree on a
// number are two blocks that are one block. See `wildfire` for the
// whole of the mechanic these are the states of.

/// **A trunk alight**: a log, of any wood, that a fire has caught
/// (`wildfire::Fuel::Log`). It lies along the axis the log did, gives light,
/// and in `wildfire::LOG_BURN_SECONDS` it is `BLOCK_CHARRED_LOG`.
///
/// **Its own id and not a bit on the log**, because a log's variant field is
/// its axis and the two bits left over are one short of saying "burning" for
/// three woods; and because what is alight has to *give light*, which the
/// light engine reads off the row (`emission`) and never off a variant.
pub const BLOCK_BURNING_LOG: BlockId = 505;
/// ...and boards alight, of any wood, pegged or not.
pub const BLOCK_BURNING_PLANKS: BlockId = 506;
/// **What a fire leaves of a trunk**: black, split, and still standing. One
/// id for every wood -- charcoal is the colour of charcoal whatever tree it
/// was -- lying along its old axis. Breaks by hand into ash.
pub const BLOCK_CHARRED_LOG: BlockId = 507;
/// ...and of boards. **Not pegged any more**, whatever it was: the peg burnt
/// with the board, and a charred roof is a roof the next gust brings down
/// (`falling::Looseness`), which is what a burnt roof does.
pub const BLOCK_CHARRED_PLANKS: BlockId = 508;
/// **A standing torch**: a pole as tall as a person with a wad of resin
/// bound to the top, driven into flat ground. This is the lower cell and
/// the item; the upper cell is `BLOCK_STANDING_TORCH_LIT` or `_OUT`
/// (`standing_torch_partner`).
///
/// Two cells rather than one, because a light a player walks past has to
/// be above their head to light the path rather than their shins -- which
/// is also why it wants flat ground: a pole two cells tall on the edge of a
/// drift leans.
pub const BLOCK_STANDING_TORCH: BlockId = 509;
/// ...its top, burning. The light is here, a cell up.
pub const BLOCK_STANDING_TORCH_LIT: BlockId = 510;
/// ...its top, burnt out: the wad gone to a black knot. Another lump of
/// resin lights it again (`wildfire::TORCH_SECONDS`).
pub const BLOCK_STANDING_TORCH_OUT: BlockId = 511;

/// A cover of snow on the ground: what a snowfall lays (`server
/// logic::snowfall`) and a thaw takes back.
///
/// **A coating, like ash, and not a layer of snow.** Snow used to come in
/// eighths of a block, and the layers went -- every loose material is a
/// whole block now (`has_depth` is the liquids' alone). A snowfall laying
/// whole blocks would wall a camp in overnight, so what lies on a meadow is a
/// white sheet on the floor of the cell over it (`is_covering_flat`): walked
/// through, hiding the turf, gone with a sweep of the hand. It gives nothing
/// back: a handful of snow is not a thing to carry.
pub const BLOCK_SNOW_COVER: BlockId = 207;

/// Sandstone cut into bricks and laid in courses.
///
/// **The desert's brickwork, without a kiln.** Fired brick is clay and a
/// kiln, and the dry belt has neither clay banks nor wood to spare -- so a
/// player building in sandstone country laid cobble or nothing. Sandstone
/// splits along its bedding into blocks a person can square with a stone,
/// which is how every sandstone town that ever stood was built: two blocks
/// of rough stone make one of dressed bricks, and the cost is the stone,
/// not a fire.
pub const BLOCK_SANDSTONE_BRICKS: BlockId = 208;

/// Human flesh, cut from a dead player's body with a knife.
///
/// **"Добавь разделку трупов людей и человеческое мясо."** A body used to be a
/// bag the dead player's things lie in and nothing else, and a player
/// starving beside one had no choice to make about it. Now there is one, and
/// it has a price the game states rather than hides: human flesh feeds like a
/// haunch and makes a player ill for far longer than any other meat, raw or
/// cooked (`food::sickness_seconds`) -- the disease of eating your own kind is
/// not one a fire kills. It is a thing a desperate player does once and
/// remembers, not a larder.
pub const BLOCK_HUMAN_FLESH: BlockId = 209;

/// ...and the same flesh cooked at a fire. Fed like cooked meat; still the
/// long illness. See `BLOCK_HUMAN_FLESH`.
///
/// 213, not the 210 that follows 209: 210 is `BLOCK_DRY_TURF`, declared far
/// up this file, and the first build that took it laid roast flesh across
/// every savanna and let the dry grass and wild cotton on it fall through.
/// Ids are not in file order here; look for the next free one, not the next
/// number.
pub const BLOCK_ROAST_HUMAN_FLESH: BlockId = 213;

/// **A handful of leaves: what a crown gives, rather than the crown.**
/// Breaking a block of leaves used to hand back the block, so a player who
/// cleared a canopy walked off with a stack of hedge and nothing to do with
/// it but build a hedge. What a hand tears out of a tree is a fistful of
/// leaves, and that is a material: kindling that catches and is gone
/// (`hearth::fuel_seconds`), the mulch and the turf the leaves used to
/// make, and -- four of them pressed together -- a block of leaves again,
/// for the player who does want the hedge (`crafting`, "bundle leaves").
///
/// **One handful for every broadleaf crown, not one per tree.** A birch
/// leaf and an oak leaf do the same thing in a fire and on a field, and
/// seven items of identical use would be seven stacks that refuse to
/// merge in a pack. The needles of a fir are not leaves and keep their
/// own drop.
///
/// Rejected: **a chance of a handful, the block otherwise** -- the same
/// swing giving two different kinds of thing is a coin toss, not a
/// decision; **several handfuls per block** -- a crown is dozens of blocks,
/// and a single one already gives more kindling than a stick does.
pub const BLOCK_LEAF_HANDFUL: BlockId = 214;

/// **Fallen leaves** on the floor of a broadleaf wood: a brown coating over
/// the earth under the crowns, as ash and snow are coatings
/// (`is_covering_flat`). "добавь опавшие листья".
///
/// What it is for is two things a player can use. Swept up by hand it is
/// the handful the crown gives (`BLOCK_LEAF_HANDFUL`) -- kindling and mulch
/// without climbing into a tree. And it is **tinder on the ground**
/// (`wildfire::fuel`): a fire lit on a wood's floor walks along it, so a
/// camp under the oaks is a camp cleared first, and a camp in the open is
/// the one that did not have to be.
///
/// Rejected: **leaves that fall through the year and pile up** -- a block
/// written into every wood near every player on a timer, for a floor a
/// player sees as the same brown either way; **a tint on the grass under
/// the crowns** -- right to look at and nothing to pick up or burn.
pub const BLOCK_LEAF_LITTER: BlockId = 225;

// ---- the wild plants: tall, low, and the sundew ----
//
// Ids from 470, for the sea floor's reason: other work was adding ids above
// 450 at the same time, and two sets of constants that happen to agree on a
// number are two blocks that are one block.
//
// "добавь больше растений и сделай растения высотой в 2 блока": four plants
// two cells tall, four one cell tall, and the sundew on its own. Each is here
// for something it gives or something it tells, and where they grow is
// `worldgen::Biome`'s tables; what they need under them is `can_grow_on`.

/// **Fireweed** (иван-чай): a tall spike of pink flowers, the first thing to
/// come back where a wood was opened -- a clearing, an edge, a felled stand.
/// Two cells tall (`is_tall_plant`). Its stem is bast: a stalk gives a fibre.
/// Never under a closed canopy: it is the plant that says the light got in.
pub const BLOCK_FIREWEED: BlockId = 470;
/// **Cattail** (рогоз): a tall flag of leaves with a brown head, at the edge
/// of still fresh water. Two cells tall. What it gives is its rhizome -- a
/// root, roasted like any other -- and a fibre off the leaves: the swamp
/// feeds a player who is willing to be wet (`worldgen::Biome::berry_spacing`).
pub const BLOCK_CATTAIL: BlockId = 471;
/// **Stinging nettle** (крапива): tall, dark and hairy, on riverbanks and the
/// rich ground at the edge of a wood. Two cells tall. The best fibre there is
/// -- two a plant -- and it stings a bare hand that pulls it
/// (`nettle_stings`): a knife, or the price.
pub const BLOCK_NETTLE: BlockId = 472;
/// **Bracken** (папоротник-орляк): a fern frond held up on a tall stalk, in
/// the light woods and their clearings. Two cells tall; its upper half wears
/// the fern's picture, because that is what bracken is. A stalk gives a fibre.
pub const BLOCK_BRACKEN: BlockId = 473;
/// **Giant reed** (арундо, *Arundo donax*): a cane taller than a man with a
/// pale plume, at the edge of fresh water in hot country -- the savanna's
/// rivers and the desert's pools. Two cells tall (`is_tall_plant`), the
/// cattail's rules for where it stands and how it spreads, one climate
/// further south.
///
/// What it gives is **cane** (`BLOCK_CANE`): a straight, light, jointed pole
/// nobody had to shape. Two of them a plant, because the stem is two cells
/// of it.
pub const BLOCK_ARUNDO: BlockId = 233;
/// A length of giant reed. See `BLOCK_ARUNDO` for where it grows and
/// `crafting`'s "cane frame" for what it is for: a furniture frame lashed
/// with cord, with no flint worked and no iron spent -- a third way to the
/// same frame, and the one that costs a walk to a hot river instead.
/// It burns as poorly as a stick does, being hollow.
pub const BLOCK_CANE: BlockId = 234;
/// **Bilberry** (черника): a low shrub of the shaded floor of a birch wood, a
/// taiga and an oak wood, blue with berries. The berries come off with a
/// break and leave `BLOCK_BILBERRY_BARE`, for the berry bush's reason -- two
/// ids, because a harvest in the variant would be a harvest every rule that
/// reads a row has to be taught to refuse twice.
pub const BLOCK_BILBERRY: BlockId = 474;
/// ...picked. Sets fruit again in warm air (`growth::fruit_sets_above_c`).
pub const BLOCK_BILBERRY_BARE: BlockId = 475;
/// **Wild strawberry** (земляника): the bilberry's sun-side twin -- a low
/// rosette with red fruit on the edges of woods and in meadows. Where the one
/// is found is where the other is not, and that is the decision: shade or
/// light.
pub const BLOCK_STRAWBERRY: BlockId = 476;
/// ...picked.
pub const BLOCK_STRAWBERRY_BARE: BlockId = 477;
/// **Plantain** (подорожник): a flat rosette of ribbed leaves on trodden
/// ground and meadow. Picked whole; two leaves and a strand make a poultice
/// (`crafting`), which is the meadow's answer to a wound where the dark woods
/// answer with a bracket fungus.
pub const BLOCK_PLANTAIN: BlockId = 478;
/// **Fern** (папоротник): the ground cover of a shaded forest floor, where
/// grass does not grow. Gives nothing: a frond crumbles rather than twists,
/// and a forest floor that paid fibre like a meadow would be a meadow with a
/// roof on.
pub const BLOCK_FERN: BlockId = 479;
/// **Sundew** (росянка): a tiny red rosette beaded with sticky drops, on the
/// sodden turf of a bog and nowhere else. Where it grows the ground under the
/// turf is peat (`worldgen::Biome::Bog`), which is the one thing it tells a
/// player -- the fuel is under the red. Picked whole and set down again on
/// wet ground; it is not food and not medicine, and pretending otherwise
/// would be a rule nobody could discover.
pub const BLOCK_SUNDEW: BlockId = 480;

// ---- the woods of the north and of the desert ----
//
// Ids from 490, for the sea floor's reason: fire was adding ids from 500 at
// the same time, and every id has to stay under `blocks::INDEX_KINDS`.
//
// "в пустыне растёт то же дерево, что и в лесу": the taiga's firs and the
// treeline's were the oak's log under the oak's leaf, and the desert grew no
// tree at all. Two woods, each the four blocks a wood is (`wood::Wood`), and
// what ties each to its country is `worldgen::Biome::tree_wood`.

/// **A fir's trunk**: dark, rough bark with a reddish cast, and a pale cut
/// end. The taiga's and the treeline's timber. Every number is the oak's
/// log's -- an axe, nine seconds standing and four and a half lying -- for
/// the birch's reason: two rules for one material is a rule nobody finds.
pub const BLOCK_FIR_LOG: BlockId = 490;
/// **A fir's needles.** Its own crown rather than the oak's leaf in a cold
/// tint, and untinted, for the maple's reason turned round: a fir is the
/// dark blue-green of a fir wherever it stands, and the climate tint pulled
/// the taiga's crowns toward the meadow's green at its warm edge.
pub const BLOCK_FIR_NEEDLES: BlockId = 491;
/// **Fir boards**: pale and yellow, the lightest boards there are.
pub const BLOCK_FIR_PLANKS: BlockId = 492;
/// Fir boards pegged. See `BLOCK_PEGGED_PLANKS`.
pub const BLOCK_PEGGED_FIR_PLANKS: BlockId = 493;
/// **A saxaul's trunk**: the desert's tree, a crooked grey stem no taller
/// than a person, as hard as wood gets. The oak's log's numbers in every
/// rule, for the fir's reason -- the saxaul is one of the hardest woods there is,
/// and a desert tree that took twice the axe would be the one tree in the
/// game a player learns to walk past.
pub const BLOCK_SAXAUL_LOG: BlockId = 494;
/// **A saxaul's crown**: green-grey jointed twigs rather than leaves, sparse,
/// untinted -- the desert's tint is straw, and a tree the colour of the sand
/// under it is not a thing an eye can find.
pub const BLOCK_SAXAUL_LEAVES: BlockId = 495;
/// **Saxaul boards**: dark, dense and grey-brown.
pub const BLOCK_SAXAUL_PLANKS: BlockId = 496;
/// Saxaul boards pegged. See `BLOCK_PEGGED_PLANKS`.
pub const BLOCK_PEGGED_SAXAUL_PLANKS: BlockId = 497;

// ---- the ground: rocks, their rubble, soils, grasses, two woods, moss ----
//
// **Ids from 512**, past the old end of the index (`blocks::INDEX_KINDS`
// was 512 and is 1024 now): the space under it had a few scattered holes
// left and three pieces of work were writing into them at once. One range,
// laid out by kind so a number says what it is:
//
// | ids       | what                                                      |
// |-----------|-----------------------------------------------------------|
// | 512..=521 | ten rocks (`ground::ROCKS`)                               |
// | 524..=578 | each rock's cobble, gravel, sand and pebble               |
// | 580..=589 | ten soils (`ground::SOILS`)                               |
// | 590..=599 | ten grasses (`ground::GRASSES`)                           |
// | 600..=607 | the pine and the willow, four blocks each (`wood::WOODS`) |
// | 608..=615 | a twig and a bough in the bark of each non-broadleaf wood |
// | 616       | moss, the thing scraped off a mossy stone or trunk        |
//
// What each is *for* is written at `ground` and `wood`, where the tables
// are; the constants below only name them.

/// **Shale**: mud and clay pressed into thin grey leaves. The floor of the
/// wet lowlands -- swamp, bog, river country -- where the mud still is. Soft.
pub const BLOCK_SHALE: BlockId = 512;
/// **Chalk**: the whitest, softest rock, and the one flint grows in.
pub const BLOCK_CHALK: BlockId = 513;
/// **Dolomite**: a limestone that took magnesium in; buff and harder.
pub const BLOCK_DOLOMITE: BlockId = 514;
/// **Marble**: limestone baked in the roots of a mountain.
pub const BLOCK_MARBLE: BlockId = 515;
/// **Quartzite**: sandstone baked until the grains fused; the hardest rock
/// under a desert.
pub const BLOCK_QUARTZITE: BlockId = 516;
/// **Gneiss**: granite squeezed into bands; the old north's rock.
pub const BLOCK_GNEISS: BlockId = 517;
/// **Diorite**: salt-and-pepper speckled, a mountain's other granite.
pub const BLOCK_DIORITE: BlockId = 518;
/// **Gabbro**: basalt's slow-cooled twin, deep in the dykes.
pub const BLOCK_GABBRO: BlockId = 519;
/// **Andesite**: the grey lava of high volcanic peaks.
pub const BLOCK_ANDESITE: BlockId = 520;
/// **Tuff**: volcanic ash turned soft rock, under the dead forest.
pub const BLOCK_TUFF: BlockId = 521;

pub const BLOCK_SANDSTONE_COBBLE: BlockId = 524;
pub const BLOCK_SANDSTONE_GRAVEL: BlockId = 525;
pub const BLOCK_SANDSTONE_PEBBLE: BlockId = 526;
pub const BLOCK_LIMESTONE_COBBLE: BlockId = 527;
pub const BLOCK_LIMESTONE_GRAVEL: BlockId = 528;
pub const BLOCK_LIMESTONE_SAND: BlockId = 529;
pub const BLOCK_LIMESTONE_PEBBLE: BlockId = 530;
pub const BLOCK_GRANITE_COBBLE: BlockId = 531;
pub const BLOCK_GRANITE_GRAVEL: BlockId = 532;
pub const BLOCK_GRANITE_SAND: BlockId = 533;
pub const BLOCK_GRANITE_PEBBLE: BlockId = 534;
pub const BLOCK_BASALT_COBBLE: BlockId = 535;
pub const BLOCK_BASALT_GRAVEL: BlockId = 536;
pub const BLOCK_BASALT_SAND: BlockId = 537;
pub const BLOCK_BASALT_PEBBLE: BlockId = 538;
pub const BLOCK_SHALE_COBBLE: BlockId = 539;
pub const BLOCK_SHALE_GRAVEL: BlockId = 540;
pub const BLOCK_SHALE_SAND: BlockId = 541;
pub const BLOCK_SHALE_PEBBLE: BlockId = 542;
pub const BLOCK_CHALK_COBBLE: BlockId = 543;
pub const BLOCK_CHALK_GRAVEL: BlockId = 544;
pub const BLOCK_CHALK_SAND: BlockId = 545;
pub const BLOCK_CHALK_PEBBLE: BlockId = 546;
pub const BLOCK_DOLOMITE_COBBLE: BlockId = 547;
pub const BLOCK_DOLOMITE_GRAVEL: BlockId = 548;
pub const BLOCK_DOLOMITE_SAND: BlockId = 549;
pub const BLOCK_DOLOMITE_PEBBLE: BlockId = 550;
pub const BLOCK_MARBLE_COBBLE: BlockId = 551;
pub const BLOCK_MARBLE_GRAVEL: BlockId = 552;
pub const BLOCK_MARBLE_SAND: BlockId = 553;
pub const BLOCK_MARBLE_PEBBLE: BlockId = 554;
pub const BLOCK_QUARTZITE_COBBLE: BlockId = 555;
pub const BLOCK_QUARTZITE_GRAVEL: BlockId = 556;
pub const BLOCK_QUARTZITE_SAND: BlockId = 557;
pub const BLOCK_QUARTZITE_PEBBLE: BlockId = 558;
pub const BLOCK_GNEISS_COBBLE: BlockId = 559;
pub const BLOCK_GNEISS_GRAVEL: BlockId = 560;
pub const BLOCK_GNEISS_SAND: BlockId = 561;
pub const BLOCK_GNEISS_PEBBLE: BlockId = 562;
pub const BLOCK_DIORITE_COBBLE: BlockId = 563;
pub const BLOCK_DIORITE_GRAVEL: BlockId = 564;
pub const BLOCK_DIORITE_SAND: BlockId = 565;
pub const BLOCK_DIORITE_PEBBLE: BlockId = 566;
pub const BLOCK_GABBRO_COBBLE: BlockId = 567;
pub const BLOCK_GABBRO_GRAVEL: BlockId = 568;
pub const BLOCK_GABBRO_SAND: BlockId = 569;
pub const BLOCK_GABBRO_PEBBLE: BlockId = 570;
pub const BLOCK_ANDESITE_COBBLE: BlockId = 571;
pub const BLOCK_ANDESITE_GRAVEL: BlockId = 572;
pub const BLOCK_ANDESITE_SAND: BlockId = 573;
pub const BLOCK_ANDESITE_PEBBLE: BlockId = 574;
pub const BLOCK_TUFF_COBBLE: BlockId = 575;
pub const BLOCK_TUFF_GRAVEL: BlockId = 576;
pub const BLOCK_TUFF_SAND: BlockId = 577;
pub const BLOCK_TUFF_PEBBLE: BlockId = 578;

/// **Loam**: the brown crumbly earth of a broadleaf wood.
pub const BLOCK_LOAM: BlockId = 580;
/// **Chernozem**: black steppe earth, the richest there is.
pub const BLOCK_CHERNOZEM: BlockId = 581;
/// **Podzol**: the ashen, leached earth under conifers.
pub const BLOCK_PODZOL: BlockId = 582;
/// **Laterite**: red, iron-hard tropical earth, washed of everything else.
pub const BLOCK_LATERITE: BlockId = 583;
/// **Solonchak**: salt-crusted earth in a dry basin.
pub const BLOCK_SOLONCHAK: BlockId = 584;
/// **Loess**: pale wind-blown dust, deep and soft.
pub const BLOCK_LOESS: BlockId = 585;
/// **Gley**: waterlogged blue-grey clayey earth.
pub const BLOCK_GLEY: BlockId = 586;
/// **Rendzina**: thin dark earth full of chips of the chalk under it.
pub const BLOCK_RENDZINA: BlockId = 587;
/// **Andosol**: dark, light earth of volcanic ash.
pub const BLOCK_ANDOSOL: BlockId = 588;
/// **Permafrost**: earth frozen through, under the tundra's snow.
pub const BLOCK_PERMAFROST: BlockId = 589;

/// **Feather grass** (ковыль): silver plumes of the dry plains.
pub const BLOCK_FEATHER_GRASS: BlockId = 590;
/// **Sedge**: tough, sharp-edged leaves at the edge of still water.
pub const BLOCK_SEDGE: BlockId = 591;
/// **Cotton grass**: white tufts over a bog or the tundra.
pub const BLOCK_COTTON_GRASS: BlockId = 592;
/// **Fescue**: a fine blue-green mountain grass.
pub const BLOCK_FESCUE: BlockId = 593;
/// **Marram**: the grass that holds a dune together.
pub const BLOCK_MARRAM: BlockId = 594;
/// **Elephant grass**: head-high savanna cane.
pub const BLOCK_ELEPHANT_GRASS: BlockId = 595;
/// **Bluegrass**: the soft short grass under a broadleaf wood.
pub const BLOCK_BLUEGRASS: BlockId = 596;
/// **Timothy**: the meadow grass with a cat's-tail spike.
pub const BLOCK_TIMOTHY: BlockId = 597;
/// **Tussock grass**: dense hummocks of the cold wet north.
pub const BLOCK_TUSSOCK_GRASS: BlockId = 598;
/// **Spinifex**: spiny desert hummocks that weep resin.
pub const BLOCK_SPINIFEX: BlockId = 599;

/// **A pine's trunk**: tall, bare, orange-scaled under a crown at the top.
/// The oak's numbers in every rule, for the fir's reason.
pub const BLOCK_PINE_LOG: BlockId = 600;
/// **A pine's needles**: long, grey-green, untinted for the fir's reason.
pub const BLOCK_PINE_NEEDLES: BlockId = 601;
/// **Pine boards**: warm yellow with dark knots.
pub const BLOCK_PINE_PLANKS: BlockId = 602;
/// Pine boards pegged. See `BLOCK_PEGGED_PLANKS`.
pub const BLOCK_PEGGED_PINE_PLANKS: BlockId = 603;
/// **A willow's trunk**: furrowed grey-brown, by a swamp's water.
pub const BLOCK_WILLOW_LOG: BlockId = 604;
/// **A willow's leaves**: narrow, silvery, hanging.
pub const BLOCK_WILLOW_LEAVES: BlockId = 605;
/// **Willow boards**: pale pinkish and light.
pub const BLOCK_WILLOW_PLANKS: BlockId = 606;
/// Willow boards pegged. See `BLOCK_PEGGED_PLANKS`.
pub const BLOCK_PEGGED_WILLOW_PLANKS: BlockId = 607;

/// **A twig and a bough in each other bark.** The oak and the birch share
/// `BLOCK_TWIG`/`BLOCK_BOUGH` through the width steps of the variant
/// (`BIRCH_TWIG_FROM`), and those steps are all spent; a fir's, a saxaul's, a
/// pine's and a willow's pieces are ids of their own, each with the oak's
/// widths. **Every rule that asks "is this a twig" asks `is_twig`/`is_bough`,
/// never the id**, which is the price of the ids and is paid once, there.
pub const BLOCK_FIR_TWIG: BlockId = 608;
pub const BLOCK_FIR_BOUGH: BlockId = 609;
pub const BLOCK_SAXAUL_TWIG: BlockId = 610;
pub const BLOCK_SAXAUL_BOUGH: BlockId = 611;
pub const BLOCK_PINE_TWIG: BlockId = 612;
pub const BLOCK_PINE_BOUGH: BlockId = 613;
pub const BLOCK_WILLOW_TWIG: BlockId = 614;
pub const BLOCK_WILLOW_BOUGH: BlockId = 615;

/// **Moss**, scraped by hand off a mossy stone or trunk (`ground::scraped`).
/// A wound dressing -- dried sphagnum was field dressing within living
/// memory -- and nothing else: see `crafting`'s "moss dressing".
pub const BLOCK_MOSS: BlockId = 616;

/// **The four workshops**: a joiner's bench, a mason's block, a potter's
/// wheel and a currier's bench. Each is what `crafting::Station` names, and
/// standing within `FIRE_WORKING_RANGE` of one is what lets its rows run.
///
/// **A player asked for them in as many words**, and the changelog carries
/// the older refusal: "a carpenter's bench as a station -- a new block and a
/// new flag for work that needs a knife and a flat place". That was right
/// about the flag and wrong about the work. What a knife and a flat place
/// make is a stool; what a bench makes is a joint square enough to hang a
/// door on, and what it buys a player is not the permission to make things
/// but the *better* way to make them -- six boards to a log where a hatchet
/// splits four, a quern where a pebble grinds, a thrown pot out of less
/// clay than a coiled one. The field keeps the hatchet, the pebble and the
/// coil. So every station answers a question the player can get wrong:
/// carry the logs home to the bench, or split them where they fell.
///
/// Stations and not hearths: nothing burns, nothing is loaded and left, and
/// the craft runs from the pack at the moment the player asks, exactly as a
/// hand craft does -- see `crafting::Station::Bench`.
pub const BLOCK_WORKBENCH: BlockId = 617;
/// A slab of stone on a stump. See `BLOCK_WORKBENCH`.
pub const BLOCK_MASON_BLOCK: BlockId = 618;
/// A wheel on a spindle, turned by foot. See `BLOCK_WORKBENCH`.
pub const BLOCK_POTTERS_WHEEL: BlockId = 619;
/// A low bench with a hide pinned across it. See `BLOCK_WORKBENCH`.
pub const BLOCK_LEATHER_BENCH: BlockId = 620;
/// **A barter stall**: a counter under a cloth awning, where a player leaves
/// goods and a price for whoever comes by while they are away. What is on it
/// lives in the container store like a chest's (`stall::STOCK`,
/// `stall::TAKINGS`); who owns it and what it asks is the server's
/// `logic::stalls`. See `stall` for the rules of a trade.
///
/// Id 665, in the gap after the drying goods: nothing reads a range there.
pub const BLOCK_STALL: BlockId = 665;

/// **A step**: the lower half of its cell whole, and the back quarter of the
/// upper half -- two boxes, turned the way it was put down, its low side
/// toward the placer (`Facing`, in the low bits of the variant). "сделай
/// ступеньки" was the request, and a roof was the reason: "не используй \
/// такое" ruled out a sloped slab, so a pitched roof is built as a stair of
/// these and the slabs beside them.
///
/// **Three shapes weighed:**
///
/// * *A diagonal wedge.* The shape a roof "really" is, and the one the
///   player forbade in as many words: a slope on a grid of cubes is a
///   surface nothing else in the world meets, a collider the step-up cannot
///   ride and a light no face of the mesher computes.
/// * *A half slab only, stacked offset.* Buildable today with no new shape --
///   and a roof of it is a ladder of shelves a player has to jump up, since
///   two half steps one on the next are a whole block rise from the edge.
/// * *The step* (chosen): each rise is half a cell, which is exactly
///   `geometry::PLAYER_STEP_HEIGHT`, so a staircase is walked and not
///   climbed, and two boxes are what the collider, the mesher and the aim
///   already know how to be (`geometry::step_boxes`).
///
/// Upside-down steps are not here: the variant's third bit is free for them,
/// and nothing a roof needs asks for one.
pub const BLOCK_PLANK_STAIRS: BlockId = 621;
/// A step of cobbles. See `BLOCK_PLANK_STAIRS`.
pub const BLOCK_COBBLESTONE_STAIRS: BlockId = 622;
/// A step of fired clay tiles, the roof that does not burn. See
/// `BLOCK_TILE_SLAB` for how the tiles are made.
pub const BLOCK_TILE_ROOF: BlockId = 623;
/// **Half a cell of fired roof tiles**: the flat course of a tiled roof, and
/// the ridge. Fired in a kiln straight from clay, the way the kiln's own
/// bricks are, because a tile is a thinner brick and the kiln is where
/// pottery that has to shed rain is fired.
pub const BLOCK_TILE_SLAB: BlockId = 624;
/// A step of thatch. See `BLOCK_THATCH_SLAB`.
pub const BLOCK_THATCH_ROOF: BlockId = 625;
/// **Half a cell of thatch**: bundles of dry grass tied to a lath. The
/// cheapest roof there is, and the one a spark takes (`wildfire::fuel`).
pub const BLOCK_THATCH_SLAB: BlockId = 626;
/// A step of branches over leaves. See `BLOCK_BRANCH_SLAB`.
pub const BLOCK_BRANCH_ROOF: BlockId = 627;
/// **Half a cell of branches laid over leaves**: the first night's roof, out
/// of what the wood drops. Burns like the canopy it came from.
pub const BLOCK_BRANCH_SLAB: BlockId = 628;

// ---- the salt, and what it keeps ----
//
// Ids from 640, clear of the roofs' 620s: the rack of two by two, the salt
// and what salting and the rack make of a catch came in one piece of work.

/// **Salt**, boiled out of a jug of the sea at a fire (`crafting`, "boil
/// salt"). The one ingredient a larder needs that is not food.
///
/// Boiled rather than scraped off a solonchak, though the crust is real:
/// what a salt flat's crust is is salt and mud and gypsum, and a player
/// scraping it into their meat would be eating the mud -- while a jug of the
/// sea over a fire leaves salt and nothing else, from a coast every map has,
/// with a jug the pottery already makes. One route, and the one that is
/// clean.
pub const BLOCK_SALT: BlockId = 640;
/// A haunch rubbed in salt: keeps four times as long as raw, and dries on
/// the rack into `BLOCK_DRIED_SALTED_MEAT`. See `food::rot_per_step`.
pub const BLOCK_SALTED_MEAT: BlockId = 641;
/// A fish rubbed in salt, on the haunch's terms.
pub const BLOCK_SALTED_FISH: BlockId = 642;
/// A fish dried on the rack. The row `rack::cures_into` was missing: a
/// catch hung on the frame used to have nothing to come off as.
pub const BLOCK_DRIED_FISH: BlockId = 643;
/// A salted haunch dried on the rack: the food that keeps longest.
pub const BLOCK_DRIED_SALTED_MEAT: BlockId = 644;
/// A salted fish dried on the rack: stockfish's salted cousin.
pub const BLOCK_DRIED_SALTED_FISH: BlockId = 645;

// ---- two racks, two jobs ----
//
// Ids at 670, clear of the 650s being handed out this month and of the
// bowls at 685 (see `BLOCK_BONES_4` for why a change adding blocks does not
// take the next free number).

/// **A hide frame**: the one-cell square of poles with a skin laced inside
/// it -- the rack every rack was before the rack of two by two
/// (`BLOCK_DRYING_RACK`), back as a thing of its own.
///
/// "сделай эту сушилку только для мяса и рыбы, и верни старую сушилку (раму
/// для шкуры)" was the request, and it is the right split for a reason the
/// player did not have to say: **a skin is stretched and a haunch is hung.**
/// The frame holds one hide flat to the wind and the ridge holds strips in a
/// row; a big rack of red strips with a skin in the middle of it was two
/// jobs on one frame, and the frame could not look like either. Which of
/// the two takes what is `rack::Trade`.
///
/// **Its own id, not a lone cell of the big rack.** A lone
/// `BLOCK_DRYING_RACK` cell *was* this frame in old saves, and the first
/// answer was to leave it that way. Rejected, because a lone cell is only
/// lone by asking the world (`rack_whole`), so every rule that differs --
/// what it takes, what it drops, what it is called -- would have had to ask
/// the neighbours first, and an item cannot ask anything: a player holding
/// "a rack" could not be told which one it was. A lone cell in an old save
/// becomes this when the world is read (`primitive_server`'s `World::load`).
///
/// Its variant field is the old rack's: two bits of facing and
/// `RACK_LOADED`, the skin that shows in it (`rack_with_hide`) -- and one of
/// the wood bits it never had a use for, `HIDE_CURED`, which says the skin
/// pegged out on it has dried (`hide_frame_showing`).
///
/// **Drawn as a skin laced into a standing frame of four poles**
/// (`misc/hide_frame.bbmodel`), the way hides were dried: two uprights planted
/// in the ground, a cross pole lashed top and bottom, and a zig-zag of short
/// cords from holes round the skin's edge to the poles. A cell tall and thin
/// across the way it faces (`collision_depth`), so it is walked round.
pub const BLOCK_HIDE_FRAME: BlockId = 670;

/// **A sod of peat half dried**, lying where it was set down: lighter at the
/// top, cracking, not yet fuel.
///
/// Peat dries on the ground now, laid out in the open the way a bog is cut
/// (`primitive_server`'s `peat`), and a field of it has to be readable at a
/// glance -- which sods are still wet, which are nearly there, which are
/// bricks. The wet sod is `BLOCK_PEAT` and the brick `BLOCK_DRIED_PEAT`; this
/// is the stage between, **as a thing and not as a bit on the cell**, so a
/// sod picked up half dried and set down again carries its half with it, and
/// the drawing is the ordinary set-down one (`ServerMessage::SetDownItem`)
/// with nothing added to the wire. Rejected: a stage in the set-down cell's
/// variant, which is lost the moment a hand takes the sod.
///
/// Not fuel: a half-dried sod smoulders and goes out, which is why nobody
/// burns one. See `hearth::fuel_seconds`.
pub const BLOCK_DRYING_PEAT: BlockId = 671;

/// A sharpened pole, stood on the ground or driven into a wall.
///
/// **One block for both, and the third variant bit says which**
/// (`STAKE_UPRIGHT`): a stake is a stake, and two ids for one stick would be
/// two rows, two pictures and a pack that holds the same thing twice. The low
/// two bits are the facing of a stake driven into a wall, which is what
/// `support_at` reads to know which cell holds it up -- the bracket fungus's
/// arrangement, and the same machinery takes it down when that cell goes.
pub const BLOCK_STAKE: BlockId = 646;

/// Slats crossed in a window: light and air through, and nothing bigger.
pub const BLOCK_WINDOW_LATTICE: BlockId = 647;

/// Is this a window lattice?
#[inline]
pub fn is_lattice(id: BlockId) -> bool {
    block_kind(id) == BLOCK_WINDOW_LATTICE
}

/// The panel of a window lattice, as a box in cell units: two sixteenths
/// thick, across the middle of its cell, turned to face whoever set it.
///
/// **It was the whole cell**, a cube wearing a picture of slats -- a lattice
/// a metre thick, which a player could stand on, and which filled a window
/// from the inside face of the wall to the outside one ("решётка ставилась
/// по середине блока и не была полным блоком"). One answer for the collider,
/// the aim and the mesher, as `prop_box` is for a pit prop.
pub fn lattice_box(id: BlockId) -> ([f32; 3], [f32; 3]) {
    const LO: f32 = 7.0 / 16.0;
    const HI: f32 = 9.0 / 16.0;
    match block_facing(id) {
        Facing::East | Facing::West => ([LO, 0.0, 0.0], [HI, 1.0, 1.0]),
        _ => ([0.0, 0.0, LO], [1.0, 1.0, HI]),
    }
}

/// A pit prop: a post of timber that holds a roof or a gallery up.
///
/// **Half a cell across and a whole one tall**, stood in the middle of its
/// cell or set against a wall (`PROP_CENTRED`, the third variant bit, with
/// the facing in the low two as everything against a wall has it). What it
/// is *for* is the rule in `logic::falling`: a block that would fall does
/// not, if a prop stands under it. A gallery driven under sand is a gallery
/// that caves in, and a prop every few cells is what a miner does about it.
///
/// **It holds the roof of the gallery it stands in, not the cell over it**
/// (`falling::PROP_REACH`, three cells). A post set on the floor of a
/// working two cells high used to hold nothing at all: the sand was over
/// the player's head and the prop was under their feet, with a cell of air
/// between, and the whole mechanic only ever worked in a crawl nobody can
/// walk down. Underground that made props useless, which is what a player
/// said about them.
///
/// Rejected: a prop that holds a whole span of roof, a few cells either
/// side. It is the same rule with a search in it, and a player cannot see
/// how far it reaches -- one column, the one it stands in, is a rule
/// anybody can read off the wall of their own mine.
///
/// **Props stack, and a stack is a pillar.** A chamber taller than the
/// reach is held by setting one on another, and a prop on a prop is
/// nothing special: it is a whole cube by its row, so it is a floor, a
/// support and a stop to a span like any other. That is the decision a
/// tall room asks for and the reason the reach is finite.
pub const BLOCK_PROP: BlockId = 761;

/// The bit that says a prop stands in the middle of its cell rather than
/// against a wall. See `BLOCK_PROP`.
pub const PROP_CENTRED: BlockId = 0b100 << VARIANT_SHIFT;

/// Is this a pit prop?
#[inline]
pub fn is_prop(id: BlockId) -> bool {
    block_kind(id) == BLOCK_PROP
}

/// Where the post of a pit prop stands in its cell, as a box in cell units.
///
/// **One answer for the collider, the aim and the mesher.** The post was
/// placed by three separate pieces of arithmetic -- the collider's table of
/// quarters, the mesher's box turned by `turned_from_north`, and a placement
/// that faced it toward the viewer -- and they turned opposite ways: the
/// post was drawn against one wall and collided against the other, so a
/// player walked into air, walked through timber, and could not click a
/// second prop onto the side of the first because the ray met a box that
/// was not where the post was. Now the wall it leans on is `wall_behind`,
/// the same cell a driven stake hangs from, and every caller asks here.
pub fn prop_box(id: BlockId) -> ([f32; 3], [f32; 3]) {
    const LO: f32 = 4.0 / 16.0;
    const HI: f32 = 12.0 / 16.0;
    const HALF: f32 = 8.0 / 16.0;
    if id & PROP_CENTRED != 0 {
        return ([LO, 0.0, LO], [HI, 1.0, HI]);
    }
    match wall_behind(id) {
        (-1, _, _) => ([0.0, 0.0, LO], [HALF, 1.0, HI]),
        (1, _, _) => ([1.0 - HALF, 0.0, LO], [1.0, 1.0, HI]),
        (_, _, -1) => ([LO, 0.0, 0.0], [HI, 1.0, HALF]),
        _ => ([LO, 0.0, 1.0 - HALF], [HI, 1.0, 1.0]),
    }
}

/// A fired pot filled with earth: a berry bush or a herb grown where there is
/// no ground to grow it in.
///
/// **The crucible is a pot, and a pot holds earth.** The fired vessel was one
/// thing only -- what ore is melted in -- and a player asked for it to be
/// worth something else ("сделай полезным vessel"). A pot of earth is what a
/// clay vessel is anywhere outside a furnace, and it is the answer to a
/// question the world already asks: a hut of stone on a shelf of rock has
/// nowhere to put a bush.
///
/// Rejected: growing anything at all in it. A pot of earth that grew a pine
/// would be a forest on a windowsill; what it takes is the small plants a
/// pot really holds (`grows_in_a_pot`).
pub const BLOCK_PLANTER: BlockId = 760;

/// Does this plant grow in a pot of earth (`BLOCK_PLANTER`)?
///
/// The small ones: the two wild berries and the berry bush, the herbs and
/// ferns, a flower, a tuft, and the sown crops. Not a tree, not a tall plant
/// whose second half stands on its own first, and not a cactus, which wants
/// the desert's sand and its heat rather than a hut's windowsill.
pub fn grows_in_a_pot(plant: BlockId) -> bool {
    if crate::ground::is_grass(plant) {
        return true;
    }
    matches!(
        block_kind(plant),
        BLOCK_FLOWER
            | BLOCK_BILBERRY
            | BLOCK_BILBERRY_BARE
            | BLOCK_STRAWBERRY
            | BLOCK_STRAWBERRY_BARE
            | BLOCK_BERRY_BUSH
            | BLOCK_FERN
            | BLOCK_SEEDS
            | BLOCK_WHEAT
            | BLOCK_MILLET
    )
}

// ---- the hammer, the chisel and the anvil ----
//
// **Three tools and a block that are about *how* a thing is made rather than
// what it is made of.** Everything else in this table is a material or a
// thing; these are the two gestures a workshop is -- striking and paring --
// and the block you strike on.
//
// The hammer comes in three metals and the chisel in two, which is the same
// ladder the axe and the knife climb, and it is the ladder for the same
// reason: what a better hammer buys is not a thing you could not make, it is
// a wider sweet spot at the anvil (`minigame::Hammer::tolerance`) and a tool
// that lasts. There is no iron chisel, and that is deliberate: a chisel pares
// wood, and bronze pares wood as well as iron does -- an iron row would be a
// row that costs the player an ingot to change nothing.

/// **A stone head lashed to a haft**: the first hammer, pecked to shape on
/// the mason's block out of a cobble.
///
/// Not a mining tool. `tool` is `None` on every hammer row in `blocks`, so a
/// hammer opens no rock a hand does not -- it is held *while* something else
/// is worked, at the mason's block and at the anvil, and a player who took it
/// down a mine would have carried a heavy stick.
pub const BLOCK_STONE_HAMMER: BlockId = 648;
/// A cast bronze head: the hammer an anvil is built for. See `BLOCK_STONE_HAMMER`.
pub const BLOCK_BRONZE_HAMMER: BlockId = 649;
/// A forged iron head, the last of the three. See `BLOCK_STONE_HAMMER`.
pub const BLOCK_IRON_HAMMER: BlockId = 650;

/// **A flint blade set square in a handle**: the stone age's chisel, and the
/// reason the joiner's rows are not locked behind metal.
///
/// A chisel is *never* the only way to make a piece of furniture. Every row it
/// cheapens has a chisel-less twin beside it in `crafting`, which is the whole
/// of how a new tool is added to this game without taking anything away.
pub const BLOCK_FLINT_CHISEL: BlockId = 651;
/// A bronze chisel: the same gesture, an edge that holds. See `BLOCK_FLINT_CHISEL`.
pub const BLOCK_BRONZE_CHISEL: BlockId = 652;

/// **The anvil**: a bronze block on a stump, and the one workshop that is
/// mostly metal.
///
/// Built in the bronze age and paid off in the iron one, which is the point
/// of it -- see `minigame` for what is done on it and `crafting` for what it
/// costs. Faced like the other workshops, because a smith stands on one side
/// of an anvil and the horn points the other way.
pub const BLOCK_ANVIL: BlockId = 653;

/// **A saw**: a toothed blade in a wooden back. What it is for is *wood
/// saved*: a log split with wedges and an axe is four boards and a heap of
/// splinters, a log sawn along its length is six (`crafting`, "sawn planks").
/// So the saw is a costly tool bought to spend less of something cheap, and
/// whether it is worth two ingots depends on how far away the trees are.
///
/// Held, not spent: the sawn rows name it in `inputs` and `returns` at once,
/// and it takes a point of wear and dulls a step at a time like any edge
/// (`tools::edge_swings`). The sawhorse asks for one in the hand, the way the
/// anvil asks for a hammer (`minigame::Game::Saw`).
///
/// Ids 380–384 in the long empty run under the handfuls: well clear of the
/// ids other changes in flight take from the bottom of the free list.
pub const BLOCK_COPPER_SAW: BlockId = 380;
/// A bronze saw. See `BLOCK_COPPER_SAW`.
pub const BLOCK_BRONZE_SAW: BlockId = 381;
/// An iron saw. See `BLOCK_COPPER_SAW`.
pub const BLOCK_IRON_SAW: BlockId = 382;
/// **A sawhorse**: two splayed trestles and a beam, where a joiner cuts to a
/// line. A station with a timing game behind it (`minigame::Game::Saw`):
/// furniture cut true wastes less wood and comes out better made. It stands
/// in for the joiner's bench for the pieces it makes, so it is a second way
/// to a chair and not a toll on the first.
pub const BLOCK_SAWHORSE: BlockId = 383;
/// **A honing stone**: a slab of sandstone set on a stump with a trough of
/// water beside it -- the whetstone (`BLOCK_WHETSTONE`) grown too big to
/// carry. A station with a timing game (`minigame::Game::Whet`): a careful
/// hand takes an edge back with almost no metal, where the whetstone in the
/// pack grinds away the rest of the old edge every time (`tools::hone`).
pub const BLOCK_HONING_STONE: BlockId = 384;

/// **A lean-to**: a debris hut -- a ridge pole propped in a fork at the mouth
/// and running down to the ground behind, ribs of sticks leaned on it from
/// both sides, courses of leaves laid over the ribs with sticks thrown over
/// them, and a bed of leaves inside: the shelter a traveller builds at dusk
/// and leaves at dawn. Fifteen cells, three long, three wide and two high
/// over its front two rows (`lean_to`, where the size and the shapes
/// rejected are argued); the two a body lies across are the straw pallet's
/// two halves (`is_bed`), and it is slept in like one (`body::Rest::Straw`).
///
/// It was those two cells and nothing else once, with a tent three quarters
/// of a cell high drawn over them, and the player's word for it was
/// "размером с 2 блока и не имеет нормальной модели".
///
/// **What it is for is the night away from home.** Inside it the rain does
/// not reach (`shelter::is_lean_to`), the night's chill is half kept out and
/// a body rests better than on the open ground (`comfort`), which is most of
/// what a hut gives -- for a dozen sticks and a heap of leaves. What it
/// costs is that it is **one night's**: the morning after it is slept in, it
/// falls in, and gives back some of its sticks and leaves (`LEAN_TO_REMAINS`).
/// So a trip out is a choice between carrying the makings of a camp every
/// night and building a hut that stays; a home is the second.
///
/// Rejected: *a lean-to that lasts*. It would be a hut for a dozen sticks,
/// and the hut, the thatch and the walls would be a longer way to the same
/// roof. Rejected too: *a lean-to you put a bed into*. The debris hut *is*
/// the bed -- a hollow in the leaves under a roof of them -- and a separate
/// pallet inside a structure would be two things to build, place and line up
/// for one night.
///
/// Id 637, in the empty run after the roofs (621–628), clear of the ids other
/// changes in flight take from the bottom of the free list.
pub const BLOCK_LEAN_TO: BlockId = 637;

/// What a lean-to gives back when it falls in: half its sticks and half its
/// leaves (its recipe is twelve and sixteen). The rest is broken, trodden and
/// blown -- which is why the next night's camp costs something again. Held
/// to the recipe by `a_fallen_lean_to_gives_back_half_of_what_it_took`.
pub const LEAN_TO_REMAINS: [(BlockId, u32); 2] = [(BLOCK_STICK, 6), (BLOCK_LEAF_HANDFUL, 8)];

/// The skeletons past the sixteenth species -- which, so far, is one rat.
///
/// **Not an argument for a species, an argument the note on
/// `BLOCK_BONES_2` already settled**: three variant bits name eight
/// animals, and the seventeenth needs somewhere to be. What is new is
/// only that the ladder is now walked in `bones_of` rather than chosen
/// with an `if`, so the twenty-fifth species is one more arm and not a
/// rewrite.
///
/// It shares the picture the other two wear. A skeleton is drawn from the
/// living animal's own proportions (`animal_model::skeleton_parts`), so a
/// bones block has never carried a texture of its own and this one does
/// not start.
pub const BLOCK_BONES_3: BlockId = 654;

/// The skeletons past the twenty-fourth species -- of which there are none
/// yet, and that is the point of it.
///
/// **Room made before it is needed**, because the note on `Species::ALL`
/// asks for exactly that order: a new bones block *before* a new animal,
/// never after. The rat used the last room in `BLOCK_BONES_3` but seven, and
/// the next few animals asked for (the young were one candidate, until they
/// turned out to be the same species at a smaller size -- see
/// `primitive_shared::youth`) would each have had to add a block in the same
/// change as the animal. It shares the picture the others wear, and it is one
/// entry in `SKELETON_BLOCKS`. An id well clear of the ones being handed out
/// this month, so two changes adding blocks at once do not both take 655.
pub const BLOCK_BONES_4: BlockId = 690;

/// Is this a hammer or a chisel: a tool that is **held while something else
/// is made**, rather than one that opens a block or feeds a recipe?
///
/// **The twelfth kind of made thing**, and it needed a name. `crafting`'s
/// "every item a recipe makes has something to do" test walks eleven cases --
/// a tool with a tier, food, an implement, a garment, a vessel, a torch, a
/// weapon, a raft, a dressing, tackle, and an ingredient of another row -- and
/// a hammer is none of them. It has no tier (it opens nothing), nobody eats
/// it, and the bronze and iron ones are named in no recipe at all: they stand
/// in for the cheapest rung (`crafting::TOOL_LADDERS`) and are held at the
/// anvil (`minigame`). Without this the test would fail for a tool that is
/// doing exactly what it was made to do.
/// **An instrument**: something held in the hand to be *read*, and spent on
/// nothing -- the water compass, whose needle the HUD draws while it is
/// held (`BLOCK_WATER_COMPASS`). Its own answer for the tests that ask every
/// made thing what it is for, as `is_workshop_tool` is: it is not a tool, a
/// garment or food, and a list of names in each test would be two places
/// to forget the next one.
pub fn is_instrument(id: BlockId) -> bool {
    block_kind(id) == BLOCK_WATER_COMPASS
}

/// **A horse's tack**: a saddle or saddlebags, spent by being put on a
/// horse (the server's `saddle_up`). Its own answer for the tests that ask
/// every made thing what it is for, on `is_instrument`'s terms: it is not a
/// tool, a garment or food, and it is used on an animal rather than on the
/// world, which is why it is not an implement.
pub fn is_tack(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_SADDLE | BLOCK_SADDLEBAGS)
}

pub fn is_workshop_tool(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_STONE_HAMMER
            | BLOCK_BRONZE_HAMMER
            | BLOCK_IRON_HAMMER
            | BLOCK_FLINT_CHISEL
            | BLOCK_BRONZE_CHISEL
            | BLOCK_COPPER_SAW
            | BLOCK_BRONZE_SAW
            | BLOCK_IRON_SAW
    )
}

/// The bit that says a stake stands on the ground rather than juts from a
/// wall. See `BLOCK_STAKE`.
pub const STAKE_UPRIGHT: BlockId = 0b100 << VARIANT_SHIFT;

/// Is this a stake, standing or driven in?
#[inline]
pub fn is_stake(id: BlockId) -> bool {
    block_kind(id) == BLOCK_STAKE
}

/// Does this stake stand on the ground?
#[inline]
pub fn stake_is_upright(id: BlockId) -> bool {
    is_stake(id) && id & STAKE_UPRIGHT != 0
}

/// Is this a step (`BLOCK_PLANK_STAIRS`) of any material -- a stair or a roof?
#[inline]
pub fn is_step(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_PLANK_STAIRS | BLOCK_COBBLESTONE_STAIRS | BLOCK_TILE_ROOF | BLOCK_THATCH_ROOF | BLOCK_BRANCH_ROOF
    )
}

/// The bit that says a cell of a tall plant is its **upper half**.
///
/// **A tall plant is two cells, one plant**: the bed's arrangement
/// (`BED_HEAD`), for the bed's reason -- two cells of which one is the other's
/// support, broken together, grown together. A plant has no front, so it
/// spends the low bit of the variant rather than the bed's third.
///
/// Rejected: *a second id per plant for its top.* Four more rows, four more
/// names, and every rule that asks "is this nettle" asking about two ids --
/// the alternative `BED_HEAD` already argues against. What the variant costs
/// instead is one line in the mesher, which draws the upper half from the
/// row's `top` picture (`mesh::cross_face`).
pub const PLANT_TOP: BlockId = 0b001 << VARIANT_SHIFT;

/// The bit that says a tall plant is still a **shoot**: one cell, the lower
/// half's picture at half height, that grows into both halves
/// (`ripens_into`, and `growth` for the second cell).
///
/// Breaking a shoot gives nothing (`block_drop`): what a stalk is worth is
/// the stalk, and a plant that paid out the day it came up would be a crop
/// with no waiting in it.
pub const PLANT_YOUNG: BlockId = 0b010 << VARIANT_SHIFT;

/// Is this one of the plants two cells tall -- either half, or a shoot?
#[inline]
pub fn is_tall_plant(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_FIREWEED | BLOCK_CATTAIL | BLOCK_NETTLE | BLOCK_BRACKEN | BLOCK_ARUNDO)
}

/// Is this the upper half of a tall plant?
#[inline]
pub fn is_plant_top(id: BlockId) -> bool {
    is_tall_plant(id) && id & PLANT_TOP != 0
}

/// Is this a tall plant that has not grown its second cell yet?
#[inline]
pub fn is_plant_shoot(id: BlockId) -> bool {
    is_tall_plant(id) && id & PLANT_YOUNG != 0
}

/// A tall plant's shoot of this kind.
#[inline]
pub fn plant_shoot(kind: BlockId) -> BlockId {
    block_kind(kind) | PLANT_YOUNG
}

/// Where the other half of this tall plant is, and exactly what must stand
/// there for the two to be one plant. `None` for a shoot, and for anything
/// that is not a tall plant. `bed_partner`'s contract, for a plant: the one
/// function the generator, the growth mechanic and every break path ask.
#[inline]
pub fn plant_partner(at: (i32, i32, i32), id: BlockId) -> Option<((i32, i32, i32), BlockId)> {
    if !is_tall_plant(id) || is_plant_shoot(id) {
        return None;
    }
    let kind = block_kind(id);
    if is_plant_top(id) {
        Some(((at.0, at.1 - 1, at.2), kind))
    } else {
        Some(((at.0, at.1 + 1, at.2), kind | PLANT_TOP))
    }
}

/// Does pulling this up with what is in the hand sting?
///
/// **A nettle, by a hand that holds no knife.** Anything cut with a blade is
/// cut without being held; a tall stand of nettles pulled up by the fistful is
/// a stand that stings. What it costs is small and never a death
/// (`primitive_server`'s break path refuses the sting at the last of a
/// player's health) -- the point is the choice between the knife and the
/// price, not a trap.
#[inline]
pub fn nettle_stings(block: BlockId, held: Option<BlockId>) -> bool {
    block_kind(block) == BLOCK_NETTLE
        && !held.is_some_and(|tool| {
            let def = crate::blocks::definition(tool);
            def.tool.is_some() && def.work == crate::blocks::Work::Plant
        })
}

/// Does cutting this with what is in the hand give a strip of nettle bast
/// rather than a handful of fibre?
///
/// **A grown nettle, cut with a blade.** The knife that spares the hand the
/// sting is the knife that splits the stalk and strips the bast out whole;
/// pulled up by the fist it is a tangle of fibre, as any stand of weeds is.
/// So the nettle bed gives one or the other, and the player chooses at the
/// bed: fibre for cord and every hand row, or bast for the cloth the north
/// has no cotton for (`crafting`, "nettle cloth"). A young nettle has no
/// bast worth the stripping and gives what it always gave.
#[inline]
pub fn strips_bast(block: BlockId, held: Option<BlockId>) -> bool {
    block_kind(block) == BLOCK_NETTLE && block & PLANT_YOUNG == 0 && !nettle_stings(block, held)
}

#[cfg(test)]
mod wild_plant_tests {
    use super::*;

    #[test]
    fn a_knife_strips_bast_off_a_grown_nettle_and_a_fist_does_not() {
        use crate::types::BLOCK_FLINT_KNIFE;
        assert!(strips_bast(BLOCK_NETTLE, Some(BLOCK_FLINT_KNIFE)));
        assert!(strips_bast(BLOCK_NETTLE | PLANT_TOP, Some(BLOCK_FLINT_KNIFE)));
        assert!(!strips_bast(BLOCK_NETTLE, None), "a fist stripped bast");
        assert!(!strips_bast(BLOCK_NETTLE | PLANT_YOUNG, Some(BLOCK_FLINT_KNIFE)), "a seedling gave bast");
        assert!(!strips_bast(BLOCK_FIREWEED, Some(BLOCK_FLINT_KNIFE)));
    }

    #[test]
    fn a_birch_piece_is_the_oak_piece_of_its_width_in_birch_bark_and_fells_as_birch() {
        for width in (2..=16u8).step_by(2) {
            let oak = branch(width);
            assert!(!is_birch_wood(oak), "an oak piece {width} wide reads as birch");
            assert_eq!(branch_width(oak), Some(width));
            if width > BIRCH_WIDEST {
                continue;
            }
            let birch = birch_branch(width);
            assert!(is_birch_wood(birch), "a birch piece {width} wide reads as oak");
            assert_eq!(block_kind(birch), block_kind(oak), "a birch piece {width} wide is not a {} by its row", block_name(oak));
            assert_eq!(branch_width(birch), Some(width), "a birch piece lost its width");
            assert!(is_known_block(birch), "a birch piece {width} wide is an invented id");
            assert!(is_known_block(drowned(birch)) && is_birch_wood(drowned(birch)), "a drowned birch lost its bark");
            assert_eq!(in_bark_of(birch, BLOCK_LOG), oak);
            assert_eq!(in_bark_of(oak, BLOCK_BIRCH_LOG), birch);
        }
        assert_eq!(block_drop(birch_branch(12)), Some(BLOCK_BIRCH_LOG), "a birch bough gave oak");
        assert_eq!(block_drop(branch(12)), Some(BLOCK_LOG));
        assert_eq!(block_drop(birch_branch(4)), Some(BLOCK_STICK), "a birch twig is not a stick");
        // A palm's trunk has the oak's steps and no bark in them.
        assert!(!is_known_block(BLOCK_PALM_TRUNK | (5 << VARIANT_SHIFT)));
        assert!(!is_birch_wood(BLOCK_PALM_TRUNK | (4 << VARIANT_SHIFT)));
    }

    #[test]
    fn a_tall_plant_is_two_halves_that_hold_each_other_and_a_shoot_is_one_cell_that_gives_nothing() {
        for kind in [BLOCK_FIREWEED, BLOCK_CATTAIL, BLOCK_NETTLE, BLOCK_BRACKEN, BLOCK_ARUNDO] {
            let name = block_name(kind);
            let at = (3, 20, -4);
            let (top_at, top) = plant_partner(at, kind).expect("a stalk has a top");
            assert_eq!(top_at, (3, 21, -4), "a {name}'s top is not over its stalk");
            assert!(is_plant_top(top));
            assert_eq!(plant_partner(top_at, top), Some((at, kind)), "a {name}'s top does not know its stalk");
            assert!(can_grow_on(top, kind), "a {name}'s top will not stand on its stalk");
            for wrong in [plant_shoot(kind), top, BLOCK_GRASS, BLOCK_AIR] {
                assert!(!can_grow_on(top, wrong), "a {name}'s top stands on {}", block_name(wrong));
            }
            assert_eq!(plant_partner(at, plant_shoot(kind)), None, "a {name} shoot has a second half");
            assert_eq!(ripens_into(plant_shoot(kind)), Some(kind), "a {name} shoot never grows");
            assert_eq!(ripens_into(kind), None, "a grown {name} grows again");
            assert_eq!(block_drop(plant_shoot(kind)), None, "a {name} shoot gave something");
            assert_eq!(block_drop(top), block_drop(kind), "a {name}'s two halves give different things");
            for id in [kind, top, plant_shoot(kind)] {
                assert!(is_known_block(id), "{} is an invented id", block_name(id));
                assert!(needs_support(id));
            }
            assert!(!is_known_block(kind | PLANT_TOP | PLANT_YOUNG), "a {name} that is a top and a shoot at once");
        }
    }

    #[test]
    fn a_nettle_stings_the_hand_that_pulls_it_and_not_the_knife_that_cuts_it() {
        assert!(nettle_stings(BLOCK_NETTLE, None));
        assert!(nettle_stings(BLOCK_NETTLE | PLANT_TOP, Some(BLOCK_STICK)), "a stick is not a blade");
        assert!(!nettle_stings(BLOCK_NETTLE, Some(BLOCK_FLINT_KNIFE)), "a knife was stung");
        assert!(!nettle_stings(BLOCK_TALL_GRASS, None), "grass stung");
    }
}

/// **Is this a thing standing in water rather than water alone?**
///
/// The question `BLOCK_KELP` exists to make askable: liquid by its row,
/// and not the cube a cell of water is. The kelp, the seagrass, the two
/// corals that grow as sprites and the shell. See `BLOCK_KELP` for why
/// these are liquid at all, and for the short list of rules that read
/// this instead of `is_liquid`.
#[inline]
pub fn stands_in_water(id: BlockId) -> bool {
    let def = crate::blocks::definition(id);
    // ...and a drowned piece of branch, which is a cube to every rule that
    // asks a shape and drawn as a post (`BLOCK_DROWNED_BOUGH`).
    def.matter == crate::blocks::Matter::Liquid
        && (def.shape != crate::blocks::Shape::Cube || matches!(block_kind(id), BLOCK_DROWNED_TWIG | BLOCK_DROWNED_BOUGH))
}

/// The same piece of branch standing in water: see `BLOCK_DROWNED_BOUGH`.
/// Anything that is not a dry twig or bough comes back as it was.
#[inline]
pub fn drowned(id: BlockId) -> BlockId {
    let variant = id & VARIANT_MASK;
    match block_kind(id) {
        BLOCK_TWIG => BLOCK_DROWNED_TWIG | variant,
        BLOCK_BOUGH => BLOCK_DROWNED_BOUGH | variant,
        _ => id,
    }
}

/// The thinnest twig and the thinnest bough, in sixteenths.
///
/// Eight is the line between what a hand takes and what wants an axe. It
/// used to be argued from the collider as well -- a bough was walked into as
/// its whole cell, and half a cell of post in a cell of wall was the most a
/// picture could disagree with it -- and that argument is gone: every piece
/// is walked into at its own width now (`branch`). The hand's line stands.
const TWIG_THINNEST: u8 = 2;
const BOUGH_THINNEST: u8 = 8;

/// Is this a piece of branch -- a twig, a bough, or a piece of a palm's
/// trunk (`BLOCK_PALM_TRUNK`, which is a bough in every rule but its bark)?
#[inline]
pub fn is_branch(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_TWIG | BLOCK_BOUGH | BLOCK_PALM_TRUNK | BLOCK_DROWNED_TWIG | BLOCK_DROWNED_BOUGH
    ) || OWN_BARK.iter().any(|&(_, twig, bough)| kind_is(id, twig) || kind_is(id, bough))
}

#[inline]
fn kind_is(id: BlockId, kind: BlockId) -> bool {
    block_kind(id) == kind
}

/// **The woods whose pieces are ids of their own**: a log, its twig and its
/// bough. The oak and the birch share `BLOCK_TWIG` and `BLOCK_BOUGH` through
/// the variant (`BIRCH_TWIG_FROM`) and are not here. See `BLOCK_FIR_TWIG`.
///
/// Rejected: *more barks in the variant*. The oak's and the birch's steps
/// fill all eight values of a bough's field; a third bark there is a narrower
/// bough for every wood, and a fourth does not fit at all.
pub const OWN_BARK: [(BlockId, BlockId, BlockId); 4] = [
    (BLOCK_FIR_LOG, BLOCK_FIR_TWIG, BLOCK_FIR_BOUGH),
    (BLOCK_SAXAUL_LOG, BLOCK_SAXAUL_TWIG, BLOCK_SAXAUL_BOUGH),
    (BLOCK_PINE_LOG, BLOCK_PINE_TWIG, BLOCK_PINE_BOUGH),
    (BLOCK_WILLOW_LOG, BLOCK_WILLOW_TWIG, BLOCK_WILLOW_BOUGH),
];

/// Is this a twig -- under eight sixteenths, a stick by hand -- of any bark,
/// dry or drowned? **Every rule that means "a twig" asks this**, never
/// `BLOCK_TWIG`, or a fir's twig is the one a hand cannot walk through.
#[inline]
pub fn is_twig(id: BlockId) -> bool {
    let kind = block_kind(id);
    matches!(kind, BLOCK_TWIG | BLOCK_DROWNED_TWIG) || OWN_BARK.iter().any(|&(_, twig, _)| twig == kind)
}

/// Is this a bough -- timber, an axe's -- of any bark, dry or drowned? Not a
/// palm's trunk, which the rules that want it name.
#[inline]
pub fn is_bough(id: BlockId) -> bool {
    let kind = block_kind(id);
    matches!(kind, BLOCK_BOUGH | BLOCK_DROWNED_BOUGH) || OWN_BARK.iter().any(|&(_, _, bough)| bough == kind)
}

/// The log of the wood a piece of branch is in the bark of: the birch's for a
/// birch piece, a fir's for a fir's, the oak's for the oak's and a drowned
/// snag's. `None` for anything that is not a twig or a bough.
#[inline]
pub fn piece_log(id: BlockId) -> Option<BlockId> {
    let kind = block_kind(id);
    if let Some(&(log, _, _)) = OWN_BARK.iter().find(|&&(_, twig, bough)| kind == twig || kind == bough) {
        return Some(log);
    }
    match kind {
        BLOCK_TWIG | BLOCK_BOUGH | BLOCK_DROWNED_TWIG | BLOCK_DROWNED_BOUGH => {
            Some(if is_birch_wood(id) { BLOCK_BIRCH_LOG } else { BLOCK_LOG })
        }
        _ => None,
    }
}

/// How many sixteenths across this piece of branch is, or `None` for
/// anything that is not one.
///
/// The variant is a step of two sixteenths up from the thinnest of its
/// kind: a twig is 2, 4 or 6 and a bough is 8, 10, 12, 14 or 16. Even
/// numbers only, so a post stands centred in its cell on whole sixteenths
/// and the bark crop (`mesh::pack_crop`) lands on texels.
#[inline]
pub fn branch_width(id: BlockId) -> Option<u8> {
    let step = ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8;
    match block_kind(id) {
        // A birch's steps start past the oak's (`birch_branch`), and mean the
        // same widths from there.
        BLOCK_TWIG | BLOCK_DROWNED_TWIG => {
            let step = if step >= BIRCH_TWIG_FROM { step - BIRCH_TWIG_FROM } else { step };
            Some(TWIG_THINNEST + 2 * step.min(2))
        }
        BLOCK_BOUGH | BLOCK_DROWNED_BOUGH => {
            let step = if step >= BIRCH_BOUGH_FROM { step - BIRCH_BOUGH_FROM } else { step.min(4) };
            Some(BOUGH_THINNEST + 2 * step)
        }
        BLOCK_PALM_TRUNK => Some(BOUGH_THINNEST + 2 * step.min(4)),
        // A bark of its own has the oak's steps and no others.
        _ if is_twig(id) => Some(TWIG_THINNEST + 2 * step.min(2)),
        _ if is_bough(id) => Some(BOUGH_THINNEST + 2 * step.min(4)),
        _ => None,
    }
}

/// Where a birch's steps start in a twig's and a bough's variant.
///
/// **A birch is the oak's pieces in white bark, and the bark is in the
/// variant.** "у березы нету веток": the birch wood was the one broadleaf
/// still built of log cubes, and a tree of pieces wore the oak's bark whatever
/// it grew in (`mesh::branch_block`). Three ways to give it its own were
/// weighed:
///
/// * *A birch twig and a birch bough, two more ids.* Every rule that names a
///   twig or a bough -- what a hand walks through, what an axe is needed for,
///   what felling counts as timber, what the map calls a wood, and the
///   collider another piece of work is changing as this one is written --
///   would need a second arm, and the first one forgotten is a birch you
///   cannot walk through or cannot fell.
/// * *The bark decided by the mesher from the leaves round a piece.* A
///   picture that changes when a player picks the leaves off, and a felled
///   birch that could not say what timber it was.
/// * **The steps above the oak's (chosen).** A twig has three widths and
///   spends three of the field's eight values; a bough five. A birch's twig
///   takes the next three and its bough the last three, which is 8, 10 and
///   12 sixteenths -- the widest a birch grows (`BIRCH_WIDEST`), and a birch
///   is a slender tree. Every rule that asks the kind is untouched; the
///   width (`branch_width`), the bark (the mesher), the timber
///   (`block_drop`) and the valid ids (`is_known_block`) learned the steps.
const BIRCH_TWIG_FROM: u8 = 3;
const BIRCH_BOUGH_FROM: u8 = 5;

/// The widest piece a birch has: its bough has three steps, 8 to 12.
pub const BIRCH_WIDEST: u8 = 12;

/// Is this a piece of a birch -- a twig or a bough in birch bark, dry or
/// drowned? See `BIRCH_TWIG_FROM`.
#[inline]
pub fn is_birch_wood(id: BlockId) -> bool {
    let step = ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8;
    match block_kind(id) {
        BLOCK_TWIG | BLOCK_DROWNED_TWIG => (BIRCH_TWIG_FROM..BIRCH_TWIG_FROM + 3).contains(&step),
        BLOCK_BOUGH | BLOCK_DROWNED_BOUGH => step >= BIRCH_BOUGH_FROM,
        _ => false,
    }
}

/// The birch's piece this many sixteenths across: `branch` in white bark,
/// no wider than `BIRCH_WIDEST`.
#[inline]
pub fn birch_branch(width16: u8) -> BlockId {
    let width = width16.clamp(TWIG_THINNEST, BIRCH_WIDEST) & !1;
    if width < BOUGH_THINNEST {
        BLOCK_TWIG | ((((width - TWIG_THINNEST) / 2) + BIRCH_TWIG_FROM) as BlockId) << VARIANT_SHIFT
    } else {
        BLOCK_BOUGH | ((((width - BOUGH_THINNEST) / 2) + BIRCH_BOUGH_FROM) as BlockId) << VARIANT_SHIFT
    }
}

/// The same piece of a tree in the bark of this timber: a birch's pieces for
/// `BLOCK_BIRCH_LOG`, an oak's for anything else. Anything that is not a dry
/// twig or bough comes back as it was -- a palm keeps its own bark.
#[inline]
pub fn in_bark_of(id: BlockId, log: BlockId) -> BlockId {
    let drowned = matches!(block_kind(id), BLOCK_DROWNED_TWIG | BLOCK_DROWNED_BOUGH);
    if !(is_twig(id) || is_bough(id)) || drowned {
        return id;
    }
    let width = branch_width(id).unwrap_or(TWIG_THINNEST);
    piece_in(log, width)
}

/// The piece this many sixteenths across in the bark of `log`: `branch` for
/// an oak (and anything that is not a log with a bark), `birch_branch` for a
/// birch, and a bark's own ids for the rest (`OWN_BARK`).
#[inline]
pub fn piece_in(log: BlockId, width16: u8) -> BlockId {
    let log = block_kind(log);
    if log == BLOCK_BIRCH_LOG {
        return birch_branch(width16);
    }
    let oak = branch(width16);
    match OWN_BARK.iter().find(|&&(own, _, _)| own == log) {
        Some(&(_, twig, bough)) => (if block_kind(oak) == BLOCK_TWIG { twig } else { bough }) | (oak & VARIANT_MASK),
        None => oak,
    }
}

/// The piece of branch this many sixteenths across: a twig under eight,
/// a bough from eight. Odd widths round down and the range is clamped,
/// so a generator can taper by arithmetic without checking its answer.
#[inline]
pub fn branch(width16: u8) -> BlockId {
    let width = width16.clamp(TWIG_THINNEST, 16) & !1;
    if width < BOUGH_THINNEST {
        BLOCK_TWIG | (((width - TWIG_THINNEST) / 2) as BlockId) << VARIANT_SHIFT
    } else {
        BLOCK_BOUGH | (((width - BOUGH_THINNEST) / 2) as BlockId) << VARIANT_SHIFT
    }
}

/// **What is left of an animal nobody came back for.**
///
/// A carcass that rots used to *vanish*, dropping its hide and its
/// bones on the grass -- and what a player saw was a deer they had left
/// overnight replaced by two items lying in a field, which reads as the
/// world tidying up after them rather than as something having
/// happened. A skeleton stays where the animal fell, and stays for
/// good: it is the one landmark in this world that means "I was here,
/// and I was too slow".
///
/// **The species rides in the variant field**, so one id covers the first
/// eight species (and `BLOCK_BONES_2` the next eight -- the field has three
/// bits) and the picture is that animal's skeleton, built from the
/// proportions of its living model (`animal_model::skeleton_parts` in
/// the client) -- no new block per animal, and no new texture at all.
/// The bones wear the ivory of the boar's tusks; `hide/bone.png`, which
/// the game has carried since the first butchering, stays the icon it
/// is, because it is a bone drawn on a transparent square and a thin
/// bone cut out of its corner is a crop of nothing.
pub const BLOCK_BONES: BlockId = 181;

/// How many species one skeleton block can name: the variant field's
/// eight values. See `BLOCK_BONES_2`.
const SKELETONS_PER_BLOCK: usize = ((VARIANT_MASK >> VARIANT_SHIFT) + 1) as usize;

/// Every skeleton block, in the order the species fill them: eight to a
/// block, the first eight in `BLOCK_BONES`, the next eight in
/// `BLOCK_BONES_2`, and so on.
///
/// **A table rather than a ladder of `match` arms**, and that is the whole
/// change `BLOCK_BONES_4` made. Each new block used to be an arm in
/// `bones_of`, an arm in `species_in_bones` and a name in `is_bones`, three
/// edits that had to agree -- and the one nobody would have remembered was
/// `is_bones`, which is what the mesher asks, so a fourth block missing from
/// it is a skeleton drawn as a cube of the bone icon. Now a block is one entry
/// here, and all three walk this.
///
/// Rejected: **one id per species**, for the reason `BLOCK_BONES_2` gives,
/// and **a wider variant field**, which is every save changing shape.
pub const SKELETON_BLOCKS: [BlockId; 4] = [BLOCK_BONES, BLOCK_BONES_2, BLOCK_BONES_3, BLOCK_BONES_4];

/// How many species the skeleton blocks have room for: thirty-two.
pub const SKELETON_ROOM: usize = SKELETON_BLOCKS.len() * SKELETONS_PER_BLOCK;

/// The skeleton of this species: its place in `Species::ALL`, eight to a
/// block, walked down `SKELETON_BLOCKS`. The first eight are exactly the ids
/// they always were, so a skeleton already lying in an old world is still its
/// animal.
#[inline]
pub fn bones_of(species: crate::animals::Species) -> BlockId {
    let index = crate::animals::Species::ALL
        .iter()
        .position(|s| *s == species)
        .unwrap_or(0);
    // Past the last block is a species appended without room for its bones;
    // `every_species_comes_back_out_of_its_own_skeleton` is a const assert
    // against exactly that, so this clamp is never reached in a build that
    // passed its tests -- it is here so a build that did not still draws a
    // skeleton (somebody else's) rather than panicking in the tick loop.
    let block = SKELETON_BLOCKS[(index / SKELETONS_PER_BLOCK).min(SKELETON_BLOCKS.len() - 1)];
    block | (((index % SKELETONS_PER_BLOCK) as BlockId) << VARIANT_SHIFT)
}

/// ...and the species a skeleton belongs to, if this is one.
#[inline]
pub fn species_in_bones(id: BlockId) -> Option<crate::animals::Species> {
    let kind = block_kind(id);
    let first = SKELETON_BLOCKS.iter().position(|&block| block == kind)? * SKELETONS_PER_BLOCK;
    let code = ((id & VARIANT_MASK) >> VARIANT_SHIFT) as usize;
    crate::animals::Species::ALL.get(first + code).copied()
}

/// Is this a skeleton? Read by the mesher, which draws it as a model
/// rather than as a cube, and by the cover table beside it. Every id in
/// `SKELETON_BLOCKS`: asking for `BLOCK_BONES` alone is how the ninth
/// species' bones would have been drawn as a heap.
#[inline]
pub fn is_bones(id: BlockId) -> bool {
    SKELETON_BLOCKS.contains(&block_kind(id))
}

/// A water barrel: staves, two hoops and an open top, standing where a
/// camp needs water and there is no river beside it.
///
/// **Three ids, one per kind of water, with the level in the variant
/// field.** A jug says what water it holds in its variant (`jug_of`); a
/// barrel has two things to say -- what is in it and how much -- and the
/// field holds one of eight values. Folding both into three bits would
/// leave a barrel either two jugs deep or unable to tell a pond from the
/// sea, and a barrel that forgot the sea was the sea is a desalination
/// plant made of planks: pour seawater in, dip drinking water out. So
/// the kind is the id, the way a jug's two states already are
/// (`BLOCK_JUG`, `BLOCK_JUG_WATER`), and the variant is the level.
///
/// An empty barrel is always this id. Water that is not there has no
/// kind, and a barrel dipped dry of pond water is clean to fill again.
pub const BLOCK_BARREL: BlockId = 182;
/// A barrel holding pond water. See `BLOCK_BARREL`.
pub const BLOCK_BARREL_STANDING: BlockId = 183;
/// ...and one holding the sea.
pub const BLOCK_BARREL_SALT: BlockId = 184;

/// A barrel of grain: the same staves, with the threshed harvest in them
/// instead of water, a jug's measure (`inventory::JUG_UNITS`) for every
/// step of the level. See `barrel_of_goods` for what goes in and why.
///
/// **One id per crop, on the water barrel's argument**: the level is the
/// variant, so what is in the barrel has to be the id -- and a barrel that
/// forgot whether it held seed or bread-grain would hand a player back
/// sixteen of the other one, which is either a field made out of a meal or
/// a meal made out of a field.
///
/// Numbered out of order on purpose (238, 239, 244): those are the ids that
/// were free when the table was being filled from several directions at
/// once, and nothing reads the barrels as a range.
pub const BLOCK_BARREL_GRAIN: BlockId = 238;
/// ...holding wheat seed. See `BLOCK_BARREL_GRAIN`.
pub const BLOCK_BARREL_SEEDS: BlockId = 239;
/// ...holding millet, which is its own seed. See `BLOCK_BARREL_GRAIN`.
pub const BLOCK_BARREL_MILLET: BlockId = 244;

// ---- cotton, and what is made of it ----
//
// **A fibre that does not come off an animal, and the first crop that is
// not food.** Wool is the warm coat and leather the tough one; what
// neither of them is, is *light* -- and the one place a player needs
// light clothing is the one place wool and fur are a hazard: hot country,
// where every degree a coat holds in is a degree toward
// `body::OVERHEATED`. Cloth is that answer (see `equipment::garment` and
// `body::felt_ambient_shaded`), and cotton grows only where the answer is
// needed: it wants warm air to grow in at all (`growth::min_growing_c` on
// the server), so the meadow a player wakes up in is somewhere they can
// carry cotton seed to and not somewhere it thrives.
//
// **Stages are ids, as the wheat's are, not the variant field.** The
// variant field is how an item carries a state now (a carcass half
// butchered, food going off), and it was the first thing considered.
// It loses here on the picture: each stage of a crop is a different
// cross-shaped sprite, and a picture per variant is a lookup the mesher
// does not have -- while `types::ripens_into` is already a table of ids
// that both sides of the socket read. A crop is three blocks.

/// A stand of wild cotton: a knee-high shrub with its bolls open.
///
/// **Wild wheat's rule, for wild wheat's reason.** It gives what it is
/// carrying -- a boll, and the seed inside it (`also_drops`) -- and it
/// does not come back: a wild stand that refilled would be a field
/// nobody had to plant, and finding the next one would stop mattering.
/// Where it stands is the world generator's business (the savanna);
/// what it needs under it is turf, never tilled earth (`can_grow_on`).
pub const BLOCK_WILD_COTTON: BlockId = 185;
/// Cotton seed: what is carried and what has just been sown, one block
/// for both, as `BLOCK_SEEDS` is and for the same reason.
pub const BLOCK_COTTON_SEEDS: BlockId = 186;
/// The growing plant, green and without bolls. Picked, it gives its seed
/// back and nothing else -- a crop pulled up early is a crop wasted, not
/// a crop lost.
pub const BLOCK_COTTON_PLANT: BlockId = 187;
/// The ripe plant, bolls open: the harvest.
pub const BLOCK_COTTON_RIPE: BlockId = 188;
/// One boll, which is the fibre a player spins. Not food, not fuel, and
/// nothing on its own -- four of them make a bolt of `BLOCK_CLOTH`.
pub const BLOCK_COTTON: BlockId = 189;

// ---- millet, the hot country's grain ----
//
// **A second cereal, and the reason to have one is where it grows.** Wheat
// is a cool-season grass (`growth::min_growing_c`: four degrees); millet
// wants fourteen, and in exchange it ripens in little more than half the
// time (`growth::ripening_seconds`) and needs no mill -- it is boiled whole.
// So the meadow's farmer bakes bread and the savanna's cooks porridge, and a
// player who has moved south finds that their seed has stopped growing and
// a new one is standing in the grass.
//
// **Bread is the better meal; porridge is the quicker one.** Eight against
// six (`food::nutrition`), and bread is four steps -- grind, wet, bake --
// where porridge is one: grain and a jug of water at a fire. A player picks
// between the harvest that feeds more and the harvest that comes sooner.
//
// **The grain is the seed**, as it is in the field: a millet seed *is* a
// millet grain. One id for what is carried, sown and eaten, and a ripe
// head gives three, which is the seed back and two to spare -- the wheat's
// "grain and seed" in one kind, since there is nothing to tell them apart.
//
// Rejected: **millet as another wheat, ground into the same flour** --
// then it would be wheat with a different picture, and the only thing it
// would decide is which texture a field wore; **millet growing wherever
// wheat does** -- two crops with one answer, where the one that ripens
// faster wins everywhere.

/// A wild stand of millet: a few stalks with drooping heads, in the savanna
/// and the desert's edge. Wild wheat's rules (turf, never tilled earth;
/// does not come back).
pub const BLOCK_WILD_MILLET: BlockId = 227;
/// Millet grain: what is carried, what is sown, and what is boiled.
pub const BLOCK_MILLET: BlockId = 228;
/// The growing millet, green.
pub const BLOCK_MILLET_PLANT: BlockId = 229;
/// The ripe millet, heads bowed.
pub const BLOCK_MILLET_RIPE: BlockId = 231;
/// Millet boiled in a jug's water at a fire.
pub const BLOCK_MILLET_PORRIDGE: BlockId = 232;
/// A bowl thrown on the potter's wheel and not yet fired.
pub const BLOCK_BOWL_RAW: BlockId = 685;
/// A fired bowl: what a stew is served in. See [`BLOCK_STEW`].
pub const BLOCK_BOWL: BlockId = 686;
/// **A bowl of stew**: meat and a root boiled in a jug's water at a fire,
/// ladled into two bowls, and the bowl given back when it is eaten.
///
/// **What the bowl is for, and the decision it makes.** A haunch and a root
/// roasted separately are seventeen points of food a player can put in a
/// pack and walk off with; the same haunch and root stewed are two bowls at
/// twelve -- a third more out of one kill -- that go off in a day, want a
/// fire and a jug of water, and tie up two bowls until they are eaten. So a
/// hunter back at the hearth stews and a hunter leaving roasts, and a
/// household with a shelf of bowls eats better than one with none. It is
/// the carried meal against the meal at home, which is the choice the
/// tannery and the ore already make of the map.
///
/// Weighed and rejected: **water carried in a bowl.** A jug already carries
/// water, carries more, and has a neck; a bowl of water would be a smaller
/// jug with one correct answer (use the jug). And **porridge in a bowl**:
/// porridge is already the quick grain meal that needs nothing but a jug,
/// and making it want bowls would be a toll on it rather than a choice.
pub const BLOCK_STEW: BlockId = 687;
/// **A bowl of ewe's milk**, drawn from a kept ewe with a lamb at foot, once
/// a day (`husbandry::MILK_EVERY_DAYS`). The bowl comes back when it is
/// drunk, as the stew's does.
///
/// **In a bowl and not a jug**, because a jug is a measure of water with
/// levels, a barrel to pour into and a dough that asks for it by name, and
/// milk in it would be milk in every one of those. And because the decision
/// the milk carries is small and daily -- this bowl, or the lamb grows a day
/// faster -- and a bowl a day is its size. It sours within the day: what a
/// flock gives is eaten at home.
pub const BLOCK_BOWL_MILK: BlockId = 694;
/// **A cairn**: a heap of six stones a player piles up to say "here".
///
/// The map draws what the server streamed and nothing else (see
/// `logic::map` on the client), so until this there was no way to leave
/// a word on it: a spring, the good clay, the ford. A cairn is that word,
/// put down *in the world* and read off the map by name -- the client
/// asks for the name when the placement is confirmed, and keeps it with
/// the map, not on the server (the map's reason, written there: what you
/// know of the land is what you walked).
///
/// **A block and not a mark drawn on the map from anywhere.** A mark
/// drawn from the fireside would be knowledge with no trip in it, and
/// "marks you make" would become a note-taking chore. A cairn costs six
/// stones carried to the place, stands where anybody walking past can see
/// it, and is gone when somebody takes it apart -- and so is the mark.
/// Taking it apart gives the six stones back (`block_drop_count`).
pub const BLOCK_CAIRN: BlockId = 697;
/// **A lodestone**: iron ore that is a magnet. One vein cell in eight
/// gives one beside its ore when it is broken, by the cell and not by a
/// die (the server's `lodestone_in`), so it is found by mining iron and
/// in no other way -- which is what puts a compass behind the iron age.
pub const BLOCK_LODESTONE: BlockId = 698;
/// **A water compass**: an iron nail stroked on a lodestone, laid on
/// a leaf floating in a bowl of water. Held in the hand, it gives the HUD
/// a needle that points north (`ui::hud::compass_dial`).
///
/// **North, and never a bag.** There was an arrow on the HUD once that
/// pointed at the nearest bag, and it was taken away on purpose (the
/// note in `ui::journal`): an arrow to the goal is a walk with one right
/// answer. A needle to north is an instrument the map is read *with* --
/// it says which way the map's top is, and the way is still chosen off
/// the land. Before iron that is the sky's job, and the sky goes out
/// under cloud (`logic::bearing`); this is what a player reaches iron
/// for, if they travel.
pub const BLOCK_WATER_COMPASS: BlockId = 699;

// ---- the shore: mussel beds, starfish and what a crab is worth ----
//
// **Numbered from 300**, in the long empty run between the meadow's flowers
// and the savanna, and not at 700: the 690s are full and another change was
// counting through them at the same time. See the note above
// `BLOCK_HANDFUL_EARTH`, which took its own run for the same reason, and
// `build::the_ids_still_free_under_seven_hundred_are_listed`, which prints
// what is left.

/// **A mussel bed**: the floor of the tidal shallows, crusted with mussels.
///
/// **A whole cell of rock and not a sprite standing on one**, which is what
/// a crust of mussels is: they *are* the surface of the stone. The alternative
/// -- a flat picture in the water over the floor, the way the shell and the
/// starfish are drawn -- was written first and taken out, because a block
/// that sits in water has to be `Matter::Liquid` for the mesher to draw the
/// sea round it, and `Matter::Liquid` means the water simulation owns the
/// cell: the first mussel taken off a bed woke the sim there and it wrote
/// water over the bed. That is the right answer for kelp, whose cut stem
/// leaves water behind, and the wrong one for a rock.
///
/// The variant is **how many mussels are left**, one to
/// [`crate::shore::BED_FULL`] -- the wild hive's field said again, and for
/// the hive's reason: it fills one mussel at a time on the growth clock
/// (`ripens_into`), so a shore worked out this morning is thin this evening
/// and whole in a week. That is the fishing spot's rule
/// (`fishing::SPOT_HOLDS`) written into the world instead of into a table of
/// pressure nobody can see.
///
/// **A stripped bed is a different id** ([`BLOCK_MUSSEL_ROCK`]) and not a
/// count of nought, which is the berry bush's shape exactly
/// (`BLOCK_BILBERRY_BARE`) and is here for the berry bush's reason: a
/// picture belongs to an id, so a bed that kept its own id when the last
/// mussel came off would look full for ever. The whole of what makes
/// over-gathering a thing a player can *see* -- a headland of bare rock
/// where somebody has been along -- is that those are two pictures.
///
/// **A block and not an animal**, which is the argument `BLOCK_STARFISH`
/// makes at length: a mussel does not go anywhere.
pub const BLOCK_MUSSEL_BED: BlockId = 300;
/// ...and the rock with the last of them off it. See `BLOCK_MUSSEL_BED`.
pub const BLOCK_MUSSEL_ROCK: BlockId = 306;
/// **Mussels**, a handful of them, as they come off the rock. Food, and food
/// **Grubs roasted on a stone by the fire**: a handful of the fat white
/// larvae out of a tuft of grass, turned on the heat until they crisp.
///
/// **Food that is there before anything else is.** A player with no fire,
/// no spear and no hook still turns up grubs while pulling grass for cord,
/// and until now they could only go on a hook (`fishing::Bait::Grub`) --
/// which is a fine second use for a thing a hungry beginner is holding and
/// a strange only one. Raw they are a mouthful with a price
/// (`food::sickness_seconds`); over a fire they are the first cooked meal
/// the meadow gives, and still a poor one beside a haunch, because a
/// handful of insects is what it is.
pub const BLOCK_ROASTED_GRUBS: BlockId = 36;

/// that has to be cooked: see `food::sickness_seconds`.
pub const BLOCK_MUSSELS: BlockId = 301;
/// ...and the same handful opened over a fire.
pub const BLOCK_COOKED_MUSSELS: BlockId = 302;
/// **A starfish** on the sea floor, drawn flat like the shell it lies beside
/// (`BLOCK_SHELL`).
///
/// **A block, and the argument is worth writing down**, because three
/// answers were weighed and two of them are animals:
///
/// * *A `Species`.* It would inherit a mind, a gait, a hit box, a stamina, a
///   sheet of twelve pictures and a slot in the skeleton table
///   (`SKELETON_BLOCKS`) -- the whole apparatus of a body that goes
///   somewhere -- for a thing that does not go anywhere, cannot be hunted,
///   is not food and has nothing to flee. Every predicate in
///   `animals::Species` would get a row saying "not this one".
/// * *An entity that is not an animal*, the way a raft is. A raft is a thing
///   a player steers; a starfish is a thing a player finds. An entity is
///   streamed, ticked and interpolated, and none of those verbs is true of
///   it.
/// * **A block (chosen).** The sea floor already has one of these -- the
///   shell, which exists to tell a swimmer that the sand under them is a sea
///   bed -- and a starfish is the same sentence with a consequence attached.
///   And the consequence is *about a place*: a shore with starfish on it
///   grows fewer mussels (`shore::starfish_stall`), which is a fact about
///   that stretch of rock. A block is a place; that is the whole of what a
///   block is.
///
/// What it costs to be wrong here is small and it is worth saying: a starfish
/// that should crawl does not. It moves a few inches an hour in life, and
/// nothing in this game has a clock that slow.
pub const BLOCK_STARFISH: BlockId = 303;
/// **Crab meat**, raw: what is in the claws and the body of one crab.
pub const BLOCK_CRAB_MEAT: BlockId = 304;
/// ...and the crab put whole on the embers.
pub const BLOCK_COOKED_CRAB: BlockId = 305;

// ---- building in stages: handfuls, mortar, walls laid in place ----
//
// See `build` for all of it. **Numbered from 415**, in the empty run between
// the drowned bough (391) and the pit kiln (440), and not at the next free
// number past 699: the 690s are full, three other changes were adding blocks
// at the same time, and ids taken from the end of a run someone else is
// counting through are a collision found by a save file. `build`'s
// `the_ids_still_free_under_seven_hundred_are_listed` prints what is left.

/// **A handful of earth**: what one swing of a spade takes out of a soil.
/// Which soil rides in the id (`build::handful_of`).
pub const BLOCK_HANDFUL_EARTH: BlockId = 415;
/// A handful of sand, of whichever rock the sand is.
pub const BLOCK_HANDFUL_SAND: BlockId = 416;
/// A handful of gravel, of whichever rock.
pub const BLOCK_HANDFUL_GRAVEL: BlockId = 417;
/// A handful of clay: out of a bank, or out of a swamp's mud.
pub const BLOCK_HANDFUL_CLAY: BlockId = 418;
/// **Stone chips**: a quarter of a heap of cobble, knocked off it.
pub const BLOCK_STONE_CHIPS: BlockId = 419;
/// **Quicklime**: limestone or chalk burnt in a kiln. Slaked with water and
/// beaten into sand it is lime mortar.
pub const BLOCK_QUICKLIME: BlockId = 420;
/// **Mortar**, a trowel's worth: laid under a course of bricks.
pub const BLOCK_MORTAR: BlockId = 421;
/// **Daub**: clay, earth and straw (and dung, if there is any) worked
/// together, pressed into a woven panel.
pub const BLOCK_DAUB: BlockId = 422;
/// **Cob**: a lump of earth and clay kneaded with straw, built up in lifts.
pub const BLOCK_COB: BlockId = 423;
/// A brick wall being laid in mortar, one to three courses up. The fourth
/// course makes it `BLOCK_BRICKS`.
pub const BLOCK_BRICK_COURSES: BlockId = 424;
/// Bricks laid without mortar, one to four courses up.
pub const BLOCK_DRY_BRICKS: BlockId = 425;
/// **A dry stone wall** of field stones, one to four courses up.
pub const BLOCK_DRY_STONE_WALL: BlockId = 426;
/// **A wattle panel**: a frame of stakes, woven with rods, daubed wet, dry.
pub const BLOCK_WATTLE: BlockId = 427;
/// **A cob wall**, one to four lifts, the top one wet or dry.
pub const BLOCK_COB_WALL: BlockId = 428;

// ---- the larder, the trapline and the pack: ten old answers ----
//
// **Numbered 397 to 414**, the top of the empty run under the handfuls (415),
// for the handfuls' own reason: the saws took 380 and up from the bottom of
// the same run, and two changes counting through one gap from the same end
// meet in a save file. What each one is for is written at its module --
// `ferment`, `snare`, `pitfall`, `saltpan` -- or at its row.

/// **Young cheese**: two bowls of milk curdled with salt and pressed. Not a
/// food that keeps, and not yet the one that does: it ripens into
/// [`BLOCK_CHEESE`] in the cool and goes off in the warm (`ferment`).
pub const BLOCK_CURD: BlockId = 400;
/// **A ripe cheese**: milk that keeps. See `ferment` for the cellar it needs.
pub const BLOCK_CHEESE: BlockId = 401;
/// **A jug of must**: honey stirred into fresh water, working. It turns to
/// mead in the warm and barely moves in the cold (`ferment`).
pub const BLOCK_JUG_MUST: BlockId = 402;
/// **A jug of mead**: drunk, it is water, a little food and a glow
/// (`food::warmth_in`); the jug comes back.
pub const BLOCK_JUG_MEAD: BlockId = 403;
/// **Pemmican**: dried meat pounded into fat with berries. The traveller's
/// food -- see its row in `food::nutrition`.
pub const BLOCK_PEMMICAN: BlockId = 404;
/// **Birch bark**, peeled off a standing birch with a knife (the server's
/// `tap_trunk`). What tar is distilled from.
pub const BLOCK_BIRCH_BARK: BlockId = 405;
/// **Birch tar**: bark cooked in a sealed pot until the pitch runs out of it.
pub const BLOCK_TAR: BlockId = 406;
/// **A tarred coat**: a leather tunic worked with birch tar. Sheds the rain
/// nearly as metal does, and reeks (`equipment::reek`).
pub const BLOCK_TARRED_TUNIC: BlockId = 407;
/// **Willow bark**, peeled off a standing willow. Bound on a bruise it
/// takes the swelling down (`injury::Treatment::WillowBark`).
pub const BLOCK_WILLOW_BARK: BlockId = 408;
/// **A snare**: a noose of cord on two pegs, set on the ground for a hare.
/// What is in it rides in its variant (`snare`).
pub const BLOCK_SNARE: BlockId = 409;
/// **A pit's cover**: boughs and leaves laid over a hole. It holds a hare
/// and not a deer (`pitfall`).
pub const BLOCK_PIT_COVER: BlockId = 410;
/// **A salt pan**: a shallow bed of puddled clay on the shore, filled with
/// the sea and left to the sun. What is in it rides in its variant
/// (`saltpan`).
pub const BLOCK_SALT_PAN: BlockId = 411;
/// **Snowshoes**: a bent frame laced with cord, worn on the feet. See their
/// row in `equipment::garment` and `types::surface_drag_shod`.
pub const BLOCK_SNOWSHOES: BlockId = 412;
/// **Nettle bast**: the fibre stripped out of nettle stalks, for cloth
/// where no cotton grows. See the "nettle cloth" row in `crafting`.
pub const BLOCK_NETTLE_BAST: BlockId = 413;
/// **A salt pan full of the sea**, drying: how far it has gone is its
/// variant (`saltpan`).
///
/// **Three ids for the pan's three states, and not one id with its state in
/// the variant**, for the berry bush's reason (`BLOCK_BILBERRY_BARE`): a
/// block's picture is chosen by its kind, and an empty pan, a pan of water
/// and a pan of white crust are three pictures a player reads from across a
/// beach -- the one that says "come back now" most of all.
pub const BLOCK_SALT_PAN_BRINE: BlockId = 414;
/// **A salt pan with the salt in it**: the sea dried to a crust, waiting to
/// be scraped up. See [`BLOCK_SALT_PAN_BRINE`] for why it is its own id.
pub const BLOCK_SALT_PAN_SALT: BlockId = 399;
/// **A snare with a hare in it.** How long it has hung there is the variant
/// (`snare`), because a catch left too long is somebody else's supper. Its
/// own id for the salt pan's reason: a noose and a noose with a hare in it
/// are read across a clearing.
pub const BLOCK_SNARE_CAUGHT: BlockId = 398;
/// **A snare that was robbed**: pulled out of true and emptied by whatever
/// found the hare first. Reset by hand (`snare`).
pub const BLOCK_SNARE_SPRUNG: BlockId = 397;

// ---- the horse ----
//
// **Three ids out of the gap after the sundew (480)**, not the lowest free
// ones. Block kinds are ten bits and the room under 700 is shared with
// whatever else is being written beside this; the lowest gaps (36-38) are
// the ids anybody else reaches for first, and two branches that both took
// 36 would merge into a world where a saddle is a stall. The test
// `the_horses_ids_are_its_own` holds them apart from every other id.

/// A horse where it fell -- see `animals::Species::carcass` -- on the
/// savanna carcasses' terms: the butchering stage in the variant field, and
/// drawn as the animal's own model lying on its side.
pub const BLOCK_CARCASS_HORSE: BlockId = 484;
/// **A saddle**: a wooden tree, a leather seat and girth, a cord to lace
/// it. Put on a tame horse with a right click, and what makes a tame horse
/// something to *ride* rather than something to lead (`horse`): bareback a
/// broken horse will carry you at a walk and throw you at a gallop.
pub const BLOCK_SADDLE: BlockId = 485;
/// **Saddlebags**: two leather panniers over a horse's back. They give it a
/// pack of its own (`horse::BAGS_SLOTS`), opened from beside it, and the
/// load in them slows it (`horse::load_factor`) -- a horse carries a trip's
/// ore home, and pays for it in pace.
pub const BLOCK_SADDLEBAGS: BlockId = 486;

// ---- winter feed: hay, and the stack a flock eats it from ----
//
// **Two ids from the gap after 671**, for the reason the horse's took the
// gap after the sundew: the low gaps are what a parallel branch reaches
// for first. Not 440, which was hay once (a pit kiln's packing) and is
// retired: a save from then may still name it, and it must stay nothing.

/// **Hay**: cut grass dried on a rack (`rack::cures_into`), the fodder a
/// sheep or a horse is kept on through a winter. Fibre fed fresh does the
/// same for a day, and in summer the pen's own turf does half of it
/// (`husbandry::GRAZING_HUNGER`); in winter the turf feeds nothing
/// (`husbandry::grazes`), and what the flock eats is what was put up for it.
pub const BLOCK_HAY: BlockId = 672;
/// **A haystack**: eight hay built into a stack in the pen. A kept sheep or
/// horse within `husbandry::MANGER_REACH` eats from it by itself when it
/// is getting hungry, a bite at a time, and the stack goes down by what it
/// eats -- **the variant counts the bites taken** ([`hay_in_stack`]).
///
/// What it is for is the trip. A flock fed by hand is a visit every day
/// or two all winter, and a player on a week's walk to the tin country
/// comes back to sheep that have gone wild; a stack by the pen keeps them
/// while nobody is there, and eight bites is a week for one ewe and two
/// days for four. Broken, it gives back the hay left in it.
pub const BLOCK_HAYSTACK: BlockId = 673;
/// Hay a new stack is built of, and bites it holds.
pub const HAYSTACK_HOLDS: u8 = 8;

/// A haystack with `left` hay in it; air for none. `left` over
/// [`HAYSTACK_HOLDS`] is a full stack.
#[inline]
pub fn haystack_holding(left: u8) -> BlockId {
    if left == 0 {
        return BLOCK_AIR;
    }
    let eaten = HAYSTACK_HOLDS - left.min(HAYSTACK_HOLDS);
    BLOCK_HAYSTACK | ((eaten as BlockId) << VARIANT_SHIFT)
}

/// How much hay is left in this stack, or `None` if it is not one.
#[inline]
pub fn hay_in_stack(id: BlockId) -> Option<u8> {
    (block_kind(id) == BLOCK_HAYSTACK).then(|| HAYSTACK_HOLDS - ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8)
}
/// A bolt of plain woven cotton, and the material of the cloth set.
pub const BLOCK_CLOTH: BlockId = 190;
/// What cloth is worn as, one per slot. What they do is in
/// `equipment::garment`; they share the leather set's pictures and are
/// told apart by `garment_tint`.
pub const BLOCK_CLOTH_CAP: BlockId = 191;
pub const BLOCK_CLOTH_TUNIC: BlockId = 192;
pub const BLOCK_CLOTH_TROUSERS: BlockId = 193;
pub const BLOCK_CLOTH_WRAPS: BlockId = 194;

/// **What frost leaves of a field.**
///
/// A crop that is still growing when the air goes below
/// `growth::FROST_C` does not pause, it dies -- and it dies *visibly*,
/// as a stand of brown stalks where the green ones were. Clearing the
/// cell to air instead was considered and is worse in the way that
/// matters: a field that is simply empty one morning reads as a bug or a
/// thief, and a field of dead stalks reads as "I sowed too late", which
/// is the lesson.
///
/// One block for every crop rather than one per crop: what is left of
/// frozen wheat and frozen cotton is the same straw, and a player needs
/// to see *that* the field froze, not what it had been. It gives fibre,
/// because dead stalks are what fibre is; it ripens into nothing; it
/// stands on farmland only. Not placeable -- it is a thing that happens
/// to a field, not a thing anybody carries.
///
/// A *ripe* crop does not wither: the grain is already made, and the
/// growth clock has let go of it (`ripens_into` is `None`), so there is
/// nothing left for frost to interrupt.
pub const BLOCK_WITHERED_CROP: BlockId = 200;

/// The savanna's three where they fell -- see `animals::Species::carcass`.
/// One id per species with the butchering stage in the variant field, and
/// drawn as the animal's own model lying on its side, so no picture of
/// their own.
pub const BLOCK_CARCASS_ZEBRA: BlockId = 201;
pub const BLOCK_CARCASS_ANTELOPE: BlockId = 202;
pub const BLOCK_CARCASS_LION: BlockId = 203;

/// **The second skeleton block**, for the species past the eighth.
///
/// A skeleton keeps its species in the three bits of the variant field
/// (`bones_of`), and three bits name eight animals. There were seven; the
/// savanna made ten, and the ninth -- the antelope, at index 8 -- would
/// have written a one into the bit above the field. Nothing refuses that:
/// the kind is still a skeleton's, the three bits read back as zero, and
/// every antelope left to rot would have lain in the grass as a hare.
///
/// Three ways out were weighed. **A second id for the next eight** is this
/// one: two table rows, a picture already loaded, and `bones_of` choosing
/// by index. **A wider variant field** is every block in the world changing
/// shape -- the jug, the barrel, the carcass stages and every save read
/// those bits. **One skeleton id per species** is ten rows where two do,
/// and a species added later is a row in three files again rather than
/// nothing at all. So: a block per eight, and a test that every species
/// comes back out of its own skeleton
/// (`animals::tests::every_species_comes_back_out_of_its_own_skeleton`).
pub const BLOCK_BONES_2: BlockId = 204;

/// **A dead player, lying where they fell, with everything they owned
/// still on them.**
///
/// This is what a death leaves now, and it replaced a backpack
/// (`BLOCK_BACKPACK`, still defined for the worlds that have bags in
/// them). The machinery underneath is unchanged and deliberately so: it
/// is a container, its contents are keyed by its cell, it is opened by
/// the gesture that opens a chest and it spills what is in it when it is
/// broken. What changed is what it *is*, and that is not decoration --
/// a bag is an object the world has no opinion about, and a body is
/// something the world does something to. It rots (`logic::carrion` on
/// the server, on the same clock as every other piece of meat in the
/// game), and that turns "go and get your things" from a walk into a
/// decision with a clock on it. See `BLOCK_REMAINS` for what is left
/// when the clock runs out, and for the argument about what the rot may
/// and may not take.
///
/// **Its own id rather than a carcass of a new species.** A carcass
/// (`is_carcass`) is butchered by a knife into meat and hide, and a
/// body that could be butchered is a game about eating your friends. It
/// spends its variant field on the stage of that butchering, which this
/// has no use for; and `Species::of_carcass` would have to name a
/// species that is not an animal. What the two do share is the *ageing*,
/// and that is shared where it belongs: `rots_where_it_lies` is the one
/// question the carrion pass asks, and it says yes to both.
///
/// **Not placeable, and it drops nothing**, for the reason the backpack
/// was not: only the server puts one in the world, at the moment of
/// death. A player who could place one could stamp fake bodies across a
/// world, and one that dropped an item would be a free container.
pub const BLOCK_CORPSE: BlockId = 205;

/// **What is left of a body the world got to first**: bones, rags, and
/// the things in them that rot could not touch.
///
/// Still a container, still in the same cell, still on the player's map.
/// Two days of not coming back (`carrion::CORPSE_STEPS_TO_ROT`) turns
/// the corpse into this, and the contents are filtered on the way
/// through: what the ground takes, it takes (`rots_with_a_body`), and
/// the rest is lying in the bones for as long as the world stands.
///
/// ## Why this and not one of the other three answers
///
/// The question a rotting body has to answer is what happens at the end
/// of it, and there are only four answers.
///
/// * **Nothing -- the body keeps everything for ever.** Then the rot is
///   a picture: it changes what the cell looks like and nothing a player
///   does. A mechanic that creates no decision is not a mechanic.
/// * **Everything is destroyed.** This is the answer that sounds
///   strictest and is actually the weakest, because it is not a choice:
///   a player who died four hundred blocks out, at night, in the
///   mountains, with no boat and no food, has exactly one correct move,
///   and being told to make it under a timer is a chore with a stopwatch.
///   Worse, it punishes hardest precisely when the trip back is
///   genuinely impossible, which is the one case the player did not
///   choose.
/// * **Everything spills on the ground.** Drops have a lifetime
///   (`logic::items`), so this is the previous answer with a delay and a
///   lie on top of it: the player arrives to an empty field and cannot
///   tell a despawn from a thief.
/// * **The soft half goes and the hard half stays.** Which is this.
///
/// It is the only one of the four that is a *decision* rather than an
/// instruction. Hurrying back through the dark tonight buys the whole
/// kit -- the leather, the furs, the food, the cord, the hides you were
/// carrying home. Waiting for daylight, going round, coming back fed and
/// armed and alive costs you the soft half and keeps the expensive one:
/// the iron, the bronze, the flint, the tools, the ingots you spent a
/// week of ore on. Both are playable, neither is free, and which is
/// right depends on what you were carrying and where you died -- which
/// is the shape every mechanic in this game is supposed to have.
///
/// The bones stay for good, like an animal's (`BLOCK_BONES`): a body
/// that vanished and left a heap of items would read as the world
/// tidying up after the player rather than as something having happened
/// to them.
pub const BLOCK_REMAINS: BlockId = 206;

/// **One thing set down on the ground by hand**: a knife laid on a stone, a
/// loaf on the table, an ingot on the floor of the forge -- whatever is not
/// built with, put down with the modifier held and the use gesture
/// (`primitive_server::set_down_item`). "добавь возможность ставить любые
/// небольшие предметы на землю, зажав Shift" was the request.
///
/// **A block holding a one-slot store, and not a list of lying stacks
/// beside the world.** The three ways it could have been kept:
///
/// * *A dropped stack that never expires* (`logic::items` with a flag). The
///   drawing was already there, and nothing else was: items are not saved,
///   so everything laid out on a table would be gone after a restart; a ray
///   does not stop at an item, so taking it back would need a second aim
///   that agrees with the first; and whether a cell is taken, whether there
///   is ground under it and what happens when that ground is dug out would
///   all be asked again of a thing that is not in the world's grid.
/// * *A second store beside the chests*, saved on its own. Everything the
///   first lacked, bought by writing a second copy of what the container
///   store already does -- saving, the rot clock, spilling when broken --
///   and a second copy is a second place for a dupe to live.
/// * *This*: a cell in the grid with the thing in its store, the way a jug
///   set down keeps its grain (`set_down_vessel`). The world save keeps the
///   cell, the chest save keeps the stack, the rot pass ages food in it as
///   it ages food in a chest, breaking it spills it like a chest, and a
///   floor dug out from under it drops it (`propped`). What it costs is one
///   message, because the id cannot carry which of six hundred things is
///   lying there: `ServerMessage::SetDownItem`, `PitPottery`'s arrangement.
///
/// **Walked through, not over** (`is_collidable`): a knife on the path is
/// not a step. Nothing is put on top of it for the same reason -- it has no
/// top (`has_full_top`) -- and nothing replaces it but a hand taking it.
///
/// Its facing is which way the player was looking, so a row of tools laid
/// out along a bench lies along the bench. Not obtainable: it drops
/// nothing of its own, only what is in it.
pub const BLOCK_SET_DOWN: BlockId = 1000;

/// Is this a thing set down by hand (`BLOCK_SET_DOWN`)?
#[inline]
pub fn is_set_down(id: BlockId) -> bool {
    block_kind(id) == BLOCK_SET_DOWN
}

/// May `held` be set down on the ground as one thing (`BLOCK_SET_DOWN`)?
///
/// **Whatever is not built with**, which is what an item is (`is_item`): a
/// block goes down as itself, and a second way to put a plank somewhere
/// would be a plank that lies in a cell it does not fill. Two items are
/// refused:
///
/// * *the raft*, which is not a small thing and has its own way into the
///   world (`rafts::launch`);
/// * *the lit torch*, whose life is the clock on a stack in the pack
///   (`TORCH_LIFE`). Lying in a store it would stop burning, and a torch you
///   can pause by putting it down is a torch that never runs out.
///
/// **And one block is let through: a sod of peat.** It is a block, built with
/// like earth, and set down it is the other thing it is -- a sod cut out of a
/// bog and laid on the ground to dry, which is how peat has always been made
/// into fuel ("сделай возможность сушить торф на земле просто под солнцем").
/// The modifier is the whole difference: put down plainly it is a block of
/// peat and stays one; set down, it lies on the grass and the sun works on it
/// (`primitive_server`'s `peat`). The half-dried sod is an item already.
#[inline]
pub fn can_be_set_down(held: BlockId) -> bool {
    crate::blocks::is_defined(held)
        && (is_item(held) || block_kind(held) == BLOCK_PEAT)
        && !matches!(block_kind(held), BLOCK_RAFT | BLOCK_TORCH_LIT)
}

/// Is this cell a dead player?
///
/// Both states, because everything that asks -- the map, the landmark
/// list, the screen that opens on it -- means "a body of mine, whatever
/// stage it is at". The two are told apart by name where the difference
/// matters, which is exactly one place: the rot pass.
#[inline]
pub fn is_corpse(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_CORPSE | BLOCK_REMAINS)
}

/// Does this cell hold something that the world is slowly taking back?
///
/// The one question the carrion pass asks of a cell (see
/// `primitive_server::logic::carrion`), and the reason a player's body
/// did not need a second ageing mechanism bolted on beside the one the
/// animals already had. An animal's carcass and a player's corpse rot on
/// the same clock, keep in the same frost and are forgotten by the same
/// reconciliation; what they do *at the end* differs, and that is decided
/// there rather than here.
///
/// `BLOCK_REMAINS` is deliberately not in this: bones are what is left
/// when the rotting is over, and a thing that rotted twice would be a
/// loop that ends with an empty cell.
#[inline]
pub fn rots_where_it_lies(id: BlockId) -> bool {
    is_carcass(id) || block_kind(id) == BLOCK_CORPSE
}

/// Is there meat in this cell that something would come and eat?
///
/// Asked by a hungry scavenger with nothing alive to chase
/// (`primitive_server::logic::animals::carrion_near`), and by nothing
/// else.
///
/// **The same membership as `rots_where_it_lies`, and that is worth a
/// name rather than a shortcut.** What the ground takes back is what a
/// wolf wants, because both are "flesh lying in the open" -- but they are
/// two questions, put by two mechanisms on two clocks, and the day one of
/// them changes (a carcass stripped to the last stage that still rots but
/// has nothing left to eat, say) they should part company *here*, in one
/// line with a reason beside it, rather than in an `if` somebody has to
/// go and find.
///
/// A dead player is in it, and that is the point of it existing: a body
/// is meat lying in a wood, and the wolf that comes to it is the reason
/// to hurry back. See `BLOCK_CORPSE`.
///
/// Bones are in neither: `BLOCK_REMAINS` and `BLOCK_BONES` are what is
/// left when there was nothing more to take, and a scavenger crossing a
/// meadow for a skeleton is an animal doing arithmetic rather than
/// smelling something.
#[inline]
pub fn draws_scavengers(id: BlockId) -> bool {
    rots_where_it_lies(id)
}

/// Does the ground take this when the body carrying it falls apart?
///
/// Asked of every stack in a corpse at the moment it becomes
/// `BLOCK_REMAINS`, and of nothing else. True is "gone"; the default is
/// false, so anything this has never heard of survives -- a mod's block,
/// an id off a socket, something added next month and not thought about
/// here. **Wrong in the player's favour on purpose**: the failure it
/// prevents is a patch quietly deleting a stack of something valuable
/// because nobody remembered to classify it.
///
/// The line is "was it alive and is it soft": meat, bread, roots and
/// berries, skin, fur, fleece, cotton and the fibre and cord spun out of
/// them, and the clothes made of any of those. Wood is not on it, and
/// that is not an oversight -- a log lies in a forest for years, a stick
/// is still a stick, and a stone axe with a rotted handle would be a rule
/// nobody could predict from looking at it. Metal, stone, flint, bone,
/// clay and everything fired or smelted keeps.
///
/// Food is asked of `food::is_food` rather than listed, so a fruit added
/// next year is classified the day it exists. The rest is a list, and
/// `types::tests::the_list_of_things_the_ground_takes_from_a_body_is_the_soft_half`
/// is what stops that list going stale: it fails when a garment is added
/// without a decision being made about it.
pub fn rots_with_a_body(block: BlockId) -> bool {
    let kind = block_kind(block);
    // Anything edible, at any stage of going off -- including the rot
    // itself, which is what most of a two-day-old pack of food has
    // become by the time this is asked.
    if crate::food::is_food(kind) || kind == BLOCK_ROTTEN {
        return true;
    }
    matches!(
        kind,
        // ---- off an animal ----
        BLOCK_HIDE
            | BLOCK_LEATHER
            | BLOCK_PELT
            | BLOCK_BEAR_HIDE
            | BLOCK_WOOL
            | BLOCK_FEATHER
            | BLOCK_SINEW
            | BLOCK_FAT
            // ---- off a plant ----
            | BLOCK_FIBER
            | BLOCK_CORD
            | BLOCK_COTTON
            | BLOCK_CLOTH
            // ---- and what is worn, by what it is made of ----
            //
            // The metal sets are absent and that is the whole mechanic:
            // a full iron harness is the most expensive thing a player
            // owns and it is the thing that is still there next week.
            // Hide over a stick frame, so it goes the way every other
            // hide does. Losing a rucksack to a late return is the
            // sharpest version of the trade this whole mechanic is: the
            // ten squares are exactly what the player wanted the walk
            // back for.
            | BLOCK_RUCKSACK
            | BLOCK_LEATHER_CAP
            | BLOCK_LEATHER_TUNIC
            | BLOCK_LEATHER_LEGGINGS
            | BLOCK_LEATHER_BOOTS
            | BLOCK_WOOL_CAP
            | BLOCK_WOOL_TUNIC
            | BLOCK_WOOL_LEGGINGS
            | BLOCK_WOOL_BOOTS
            | BLOCK_CLOTH_CAP
            | BLOCK_CLOTH_TUNIC
            | BLOCK_CLOTH_TROUSERS
            | BLOCK_CLOTH_WRAPS
            | BLOCK_FUR_HOOD
            | BLOCK_FUR_CLOAK
            // Snowshoes go by their lacing: the frame would last, and a frame
            // with no lacing is two bent sticks.
            | BLOCK_SNOWSHOES
            // **A tarred coat does not.** Tar is what keeps a boat's hide
            // and a roof's shingles out of the weather for years, and a
            // tarred hide in the grass is the one leather that is still a
            // coat when its owner walks back for it -- the other half of
            // what the tar costs in the nose of every deer (`equipment::reek`).
    )
}

/// **What a wound is dressed with**, one item per kind of answer -- see
/// `injury::Treatment` for which suits which, and `injury` for why a body
/// has wounds at all.
///
/// Numbered at 340 rather than at the next free id, and on purpose: the
/// table was being extended from several directions at once (the sea floor
/// took 280, the chair 230), and three ids taken from the middle of a run
/// someone else is counting through would be a collision found by a save
/// file rather than by the compiler. Nothing reads these as a range.
///
/// A strip of fibre or cloth rolled up: stops a cut bleeding, and covers a
/// burn when there is nothing better.
pub const BLOCK_BANDAGE: BlockId = 340;
/// Two sticks and a binding: sets a broken arm or leg so it can knit.
pub const BLOCK_SPLINT: BlockId = 341;
/// Bracket fungus pounded into a pad and tied on: heals a burn three times
/// as fast as a bandage. The birch polypore was carried as a dressing by
/// the man found in the Ötztal ice, which is the whole reason it is this
/// fungus and not a leaf.
pub const BLOCK_POULTICE: BlockId = 342;

/// A rucksack: hide, cord and a frame of worked sticks, worn on the back
/// (`equipment::Slot::Back`) and worth ten more squares of pack while it
/// is on -- see `inventory::BACKPACK_SLOTS`.
///
/// **Not [`BLOCK_BACKPACK`], which is the old death bag.** That id still
/// exists for worlds that have bags standing in them, it is a container
/// in the world, it cannot be placed and nothing makes a new one. Giving
/// it a second life as a worn item would mean one id that is a block on
/// the ground in an old save and a garment in a new one, and the save
/// format has no way to tell those apart. The two do share their
/// pictures, which costs no atlas layer: `blocks.toml` entries naming
/// the same file share a layer.
///
/// Numbered at 350 for the reason the dressings are numbered at 340: the
/// table is being extended from several directions at once, and an id
/// taken from the middle of somebody else's run is a collision found by
/// a save file rather than by the compiler.
pub const BLOCK_RUCKSACK: BlockId = 350;

/// How many jugs a barrel holds: as many as the variant field can count.
pub const BARREL_JUGS: u8 = 7;

/// Whether this is a barrel, empty or holding any water or any grain.
#[inline]
pub fn is_barrel(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_BARREL
            | BLOCK_BARREL_STANDING
            | BLOCK_BARREL_SALT
            | BLOCK_BARREL_GRAIN
            | BLOCK_BARREL_SEEDS
            | BLOCK_BARREL_MILLET
    )
}

/// The barrel holding `jugs` jugs' measure of `goods`, or `None` for goods
/// a barrel does not keep. No jugs is the empty barrel, as for water.
///
/// ## Why grain, seed and millet, and nothing else a jug carries
///
/// A jug takes eleven kinds of loose stuff (`pours`); a barrel takes the
/// three that come off a field. Each kind a barrel holds is an id of its
/// own (see `BLOCK_BARREL_GRAIN`), and ids are the one thing this table
/// cannot make more of, so the question was which goods a *barrel* is the
/// answer for -- and it is the ones that arrive by the hundred at once.
/// A threshed field is a heap nobody can carry home in jugs; sand, clay
/// and gravel are dug a block at a time where they lie, ash comes a lump
/// a load, and nobody stores a hundred flint flakes.
///
/// ## What a barrel of grain is for, against a chest
///
/// Nothing a chest cannot hold -- a chest slot takes a whole stack, and a
/// barrel full to the rim is 112, less than one. What it is instead is the
/// **cheap granary**: six boards and two cords by hand (the "barrel"
/// recipe), where the chest wants a pegged frame and pegs, which is a
/// knife's worked sticks or iron nails. So the first harvest has somewhere
/// to go before the frame does. It is also worked without a screen -- a
/// jug in the hand at the staves, pour or dip -- and shows how much is left
/// from across the yard (`mesh::barrel_block` draws the level), where a
/// chest has to be opened to be counted. And it holds one crop, so the
/// seed for next year and the grain for the table are two barrels, and a
/// hurried hand cannot take one for the other.
///
/// Rejected: *every pouring good in a barrel*. Eight more ids for a
/// cask of gravel nobody asked for; and the ids left to this table are
/// few enough that each is spent on a reason.
///
/// The cost is the barrel's other life. **A barrel of grain is not a water
/// barrel**, and a camp that has one barrel has to choose which it is; and
/// broken, it spills its grain on the ground (`primitive_server`'s
/// `spawn_block_drop`) rather than coming away full -- it weighs what a
/// barrel weighs, empty, and a barrel carried full of grain would be a
/// chest's worth of pack slots in one.
pub fn barrel_of_goods(goods: BlockId, jugs: u8) -> Option<BlockId> {
    let id = match block_kind(goods) {
        BLOCK_GRAIN => BLOCK_BARREL_GRAIN,
        BLOCK_SEEDS => BLOCK_BARREL_SEEDS,
        BLOCK_MILLET => BLOCK_BARREL_MILLET,
        _ => return None,
    };
    if jugs == 0 {
        return Some(BLOCK_BARREL);
    }
    Some(id | ((jugs.min(BARREL_JUGS) as BlockId) << VARIANT_SHIFT))
}

/// What grain a barrel holds and how many jugs' measure of it; `None` for
/// a barrel of water, an empty one, and anything that is not a barrel.
#[inline]
pub fn barrel_goods(id: BlockId) -> Option<(BlockId, u8)> {
    let goods = match block_kind(id) {
        BLOCK_BARREL_GRAIN => BLOCK_GRAIN,
        BLOCK_BARREL_SEEDS => BLOCK_SEEDS,
        BLOCK_BARREL_MILLET => BLOCK_MILLET,
        _ => return None,
    };
    let jugs = ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8;
    (jugs > 0).then_some((goods, jugs))
}

/// Why a jug was not emptied into a barrel or filled from one. Each is a
/// sentence the player is told, so a refusal is never a click that did
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrelRefusal {
    /// Seven jugs in it already.
    Full,
    /// Nothing in it to dip.
    Empty,
    /// Grain offered to a barrel of water, or water to a barrel of grain.
    HoldsWater,
    HoldsGrain,
    /// One grain offered to a barrel of another.
    HoldsOtherGoods,
    /// A jug that is not full: the barrel counts in whole jugs.
    JugNotFull,
    /// Goods a barrel does not keep (`barrel_of_goods`).
    NotForABarrel,
}

impl BarrelRefusal {
    /// What the player is told, in the words every other refusal at a
    /// barrel already uses (`ServerMessage::Error`).
    pub fn words(self) -> &'static str {
        match self {
            BarrelRefusal::Full => "the barrel is full",
            BarrelRefusal::Empty => "the barrel is empty",
            BarrelRefusal::HoldsWater => "there is water in the barrel",
            BarrelRefusal::HoldsGrain => "there is grain in the barrel",
            BarrelRefusal::HoldsOtherGoods => "the barrel holds another grain",
            BarrelRefusal::JugNotFull => "a barrel is filled a full jug at a time",
            BarrelRefusal::NotForABarrel => "only grain is kept in a barrel",
        }
    }
}

/// The barrel once a jug holding `units` of `goods` has been emptied into
/// it.
///
/// **A full jug or nothing.** The level counts whole jugs -- it is the
/// variant, and there is no field for part of one -- so a jug of nine
/// poured in would have to become a jug's worth (grain out of nothing) or
/// vanish into a level that cannot show it (grain into nothing).
///
/// **Never mixed.** Water's rule is "the worse of the two", because two
/// waters mixed really are the worse one; two grains mixed are a heap
/// nobody can sow, and there is no worse-of for wheat and millet that is
/// not simply deleting one of them. So the barrel says what is in it and
/// refuses the other.
pub fn barrel_after_pouring_goods(barrel: BlockId, goods: BlockId, units: u32) -> Result<BlockId, BarrelRefusal> {
    if !is_barrel(barrel) {
        return Err(BarrelRefusal::NotForABarrel);
    }
    if barrel_of_goods(goods, 1).is_none() {
        return Err(BarrelRefusal::NotForABarrel);
    }
    if barrel_contents(barrel).is_some_and(|(_, jugs)| jugs > 0) {
        return Err(BarrelRefusal::HoldsWater);
    }
    let jugs = match barrel_goods(barrel) {
        None => 0,
        Some((held, _)) if block_kind(held) != block_kind(goods) => {
            return Err(BarrelRefusal::HoldsOtherGoods);
        }
        Some((_, jugs)) => jugs,
    };
    if jugs >= BARREL_JUGS {
        return Err(BarrelRefusal::Full);
    }
    if units != crate::inventory::JUG_UNITS {
        return Err(BarrelRefusal::JugNotFull);
    }
    barrel_of_goods(goods, jugs + 1).ok_or(BarrelRefusal::NotForABarrel)
}

/// The barrel once an empty jug has been dipped into its grain, and what
/// the jug comes out holding: a full jug's measure of it.
pub fn barrel_after_scooping(barrel: BlockId) -> Result<(BlockId, BlockId), BarrelRefusal> {
    let Some((goods, jugs)) = barrel_goods(barrel) else {
        return Err(BarrelRefusal::Empty);
    };
    let next = barrel_of_goods(goods, jugs - 1).ok_or(BarrelRefusal::NotForABarrel)?;
    Ok((next, goods))
}

/// A barrel holding `jugs` of this water, and an empty one for none.
#[inline]
pub fn barrel_of(kind: crate::body::Water, jugs: u8) -> BlockId {
    use crate::body::Water;
    if jugs == 0 {
        return BLOCK_BARREL;
    }
    let id = match kind {
        Water::Fresh => BLOCK_BARREL,
        Water::Standing => BLOCK_BARREL_STANDING,
        Water::Salt => BLOCK_BARREL_SALT,
    };
    id | ((jugs.min(BARREL_JUGS) as BlockId) << VARIANT_SHIFT)
}

/// What is in a barrel and how many jugs of it; `None` for anything that
/// is not a barrel.
#[inline]
pub fn barrel_contents(id: BlockId) -> Option<(crate::body::Water, u8)> {
    use crate::body::Water;
    let kind = match block_kind(id) {
        BLOCK_BARREL => Water::Fresh,
        BLOCK_BARREL_STANDING => Water::Standing,
        BLOCK_BARREL_SALT => Water::Salt,
        _ => return None,
    };
    Some((kind, ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8))
}

/// The barrel once a full jug has been poured into it, or `None` if it is
/// already full or `jug` is not a full jug.
///
/// **Mixed water is the worse of the two**: a jug from the pond poured
/// into river water makes a barrel of pond water, and a jug of the sea
/// makes a barrel of the sea. The other rule -- the newest jug wins -- is
/// the desalination plant again, one jug of river water at a time.
pub fn barrel_after_pouring(barrel: BlockId, jug: BlockId) -> Option<BlockId> {
    let (held, jugs) = barrel_contents(barrel)?;
    if block_kind(jug) != BLOCK_JUG_WATER || jugs >= BARREL_JUGS {
        return None;
    }
    let poured = vessel_water(jug);
    let kind = if jugs == 0 { poured } else { worse_water(held, poured) };
    Some(barrel_of(kind, jugs + 1))
}

/// The barrel once an empty jug has been dipped in it, and the full jug
/// that comes out; `None` for an empty barrel.
pub fn barrel_after_dipping(barrel: BlockId) -> Option<(BlockId, BlockId)> {
    let (kind, jugs) = barrel_contents(barrel)?;
    if jugs == 0 {
        return None;
    }
    Some((barrel_of(kind, jugs - 1), jug_of(kind)))
}

/// How much of a barrel one drink with a bare hand takes, in jugs.
///
/// **One, and the drink is worth exactly what a jug is worth**
/// (`body::JUG_HYDRATION`). The barrel counts its water in whole jugs --
/// the level is the variant, and there is no field for a part of one -- so
/// the question was only ever what a handful from it is paid in, and both
/// other answers break something that already works:
///
/// * **A river's mouthful (`body::DRINK_HYDRATION`) that costs nothing.**
///   A barrel you can drink from for ever is a well: carrying water home
///   to fill it -- the whole reason a barrel exists -- would be done once
///   per world, and the jug beside it would never be dipped again.
/// * **A mouthful that costs a whole jug.** Water destroyed by the empty
///   hand: dipping a jug and drinking it would give more than twice as
///   much for the same level, so the right way to drink from a barrel
///   would be never to drink from it. A gesture whose correct use is not
///   using it is a chore rather than a choice.
///
/// Paid in jugs and credited as a jug, seven barrels' worth of drinking is
/// seven jugs whichever hand does it, and the only thing the empty hand
/// saves is the jug -- which is what it is for.
pub const BARREL_DRINK_JUGS: u8 = 1;

/// The barrel once somebody has drunk from it with a bare hand, and what
/// kind of water they swallowed; `None` for an empty barrel and for
/// anything that is not a barrel.
///
/// The level falls by `BARREL_DRINK_JUGS`, and the kind is the barrel's
/// own -- whatever the worst jug poured into it was (see
/// `barrel_after_pouring`). What the swallow *does* is not decided here:
/// it is `Vitals::drink_water` on the server, the same one call a river
/// and a jug go through, so a barrel of the sea cannot end up with a
/// gentler rule than the sea.
pub fn barrel_after_drinking(barrel: BlockId) -> Option<(BlockId, crate::body::Water)> {
    let (kind, jugs) = barrel_contents(barrel)?;
    if jugs < BARREL_DRINK_JUGS {
        return None;
    }
    Some((barrel_of(kind, jugs - BARREL_DRINK_JUGS), kind))
}

/// Whichever of two waters does a body more harm.
fn worse_water(a: crate::body::Water, b: crate::body::Water) -> crate::body::Water {
    use crate::body::Water;
    let harm = |water: Water| match water {
        Water::Fresh => 0,
        Water::Standing => 1,
        Water::Salt => 2,
    };
    if harm(b) > harm(a) {
        b
    } else {
        a
    }
}

#[cfg(test)]
mod barrel_tests {
    use super::*;
    use crate::body::Water;

    #[test]
    fn a_jug_keeps_its_water_through_being_set_down_and_broken_and_says_which() {
        for kind in [Water::Fresh, Water::Standing, Water::Salt] {
            assert_eq!(
                block_drop(jug_of(kind)),
                Some(jug_of(kind)),
                "a jug of {kind:?} broken on the ground came back as other water"
            );
            assert!(water_label(jug_of(kind)).is_some(), "a jug of {kind:?} does not say what is in it");
        }
        assert_eq!(water_label(jug_of(Water::Standing)), Some("pond water"));
        assert_eq!(water_label(barrel_of(Water::Salt, 3)), Some("sea water"));
        assert_eq!(water_label(BLOCK_BARREL), None, "an empty barrel says it holds water");
        assert_eq!(water_label(BLOCK_JUG), None, "an empty jug says it holds water");
    }

    #[test]
    fn a_barrel_gives_back_every_jug_that_was_poured_into_it() {
        let mut barrel = BLOCK_BARREL;
        for _ in 0..BARREL_JUGS {
            barrel = barrel_after_pouring(barrel, jug_of(Water::Fresh)).expect("room in the barrel");
        }
        assert_eq!(barrel_contents(barrel), Some((Water::Fresh, BARREL_JUGS)));
        assert_eq!(
            barrel_after_pouring(barrel, jug_of(Water::Fresh)),
            None,
            "one jug more than it holds went into a full barrel"
        );
        let mut dipped = 0;
        while let Some((next, jug)) = barrel_after_dipping(barrel) {
            assert_eq!(jug, jug_of(Water::Fresh));
            barrel = next;
            dipped += 1;
        }
        assert_eq!(dipped, BARREL_JUGS, "water was made or lost in the barrel");
        assert_eq!(barrel, BLOCK_BARREL);
    }

    #[test]
    fn a_drink_from_a_barrel_takes_a_jug_and_tastes_of_what_is_in_it() {
        // One drink is one jug off the level -- the same step a dipped jug
        // takes -- so a barrel drunk from by hand runs dry after exactly
        // as many drinks as it holds jugs, and not one more.
        for kind in [Water::Fresh, Water::Standing, Water::Salt] {
            let mut barrel = barrel_of(kind, BARREL_JUGS);
            let mut drinks = 0;
            while let Some((next, swallowed)) = barrel_after_drinking(barrel) {
                assert_eq!(swallowed, kind, "a barrel of {kind:?} was drunk as {swallowed:?}");
                assert_eq!(
                    barrel_contents(next).map(|(_, jugs)| jugs),
                    barrel_contents(barrel).map(|(_, jugs)| jugs - BARREL_DRINK_JUGS),
                    "a drink took something other than the defined amount"
                );
                barrel = next;
                drinks += 1;
            }
            assert_eq!(drinks, BARREL_JUGS / BARREL_DRINK_JUGS, "{kind:?}: the barrel is a well");
            assert_eq!(barrel, BLOCK_BARREL, "a drunk-dry barrel kept its kind of water");
        }
    }

    #[test]
    fn a_barrel_gives_back_every_jug_of_grain_that_was_poured_into_it() {
        use crate::inventory::JUG_UNITS;
        for goods in [BLOCK_GRAIN, BLOCK_SEEDS, BLOCK_MILLET] {
            let mut barrel = BLOCK_BARREL;
            for _ in 0..BARREL_JUGS {
                barrel = barrel_after_pouring_goods(barrel, goods, JUG_UNITS).expect("room in the barrel");
            }
            assert_eq!(barrel_goods(barrel), Some((goods, BARREL_JUGS)));
            assert_eq!(
                barrel_after_pouring_goods(barrel, goods, JUG_UNITS),
                Err(BarrelRefusal::Full),
                "an eighth jug of {goods} went into a full barrel"
            );
            let mut scooped = 0;
            while let Ok((next, out)) = barrel_after_scooping(barrel) {
                assert_eq!(out, goods, "a barrel of {goods} gave back something else");
                barrel = next;
                scooped += 1;
            }
            assert_eq!(scooped, BARREL_JUGS, "grain was made or lost in the barrel");
            assert_eq!(barrel, BLOCK_BARREL, "a barrel scooped dry did not become the empty barrel");
        }
    }

    #[test]
    fn a_barrel_holds_one_thing_and_says_why_it_refuses_another() {
        use crate::inventory::JUG_UNITS;
        let wheat = barrel_after_pouring_goods(BLOCK_BARREL, BLOCK_GRAIN, JUG_UNITS).unwrap();
        assert_eq!(
            barrel_after_pouring_goods(wheat, BLOCK_MILLET, JUG_UNITS),
            Err(BarrelRefusal::HoldsOtherGoods),
            "millet went into a barrel of wheat"
        );
        assert_eq!(
            barrel_after_pouring_goods(barrel_of(Water::Fresh, 2), BLOCK_SEEDS, JUG_UNITS),
            Err(BarrelRefusal::HoldsWater),
            "seed was poured into water"
        );
        assert_eq!(barrel_after_pouring(wheat, jug_of(Water::Fresh)), None, "water was poured onto grain");
        assert_eq!(barrel_after_dipping(wheat), None, "a jug of water came out of a barrel of grain");
        assert_eq!(barrel_after_drinking(wheat), None, "somebody drank a barrel of grain");
        assert_eq!(
            barrel_after_pouring_goods(BLOCK_BARREL, BLOCK_GRAIN, JUG_UNITS - 1),
            Err(BarrelRefusal::JugNotFull),
            "a part jug raised the level by a whole one"
        );
        assert_eq!(
            barrel_after_pouring_goods(BLOCK_BARREL, BLOCK_SAND, JUG_UNITS),
            Err(BarrelRefusal::NotForABarrel)
        );
        assert_eq!(barrel_after_scooping(BLOCK_BARREL), Err(BarrelRefusal::Empty));
        assert_eq!(barrel_after_scooping(barrel_of(Water::Fresh, 3)), Err(BarrelRefusal::Empty));
        // Dipped dry, it holds nothing, and a barrel of nothing takes water.
        let (dry, _) = barrel_after_scooping(wheat).unwrap();
        assert!(barrel_after_pouring(dry, jug_of(Water::Fresh)).is_some(), "an emptied grain barrel refused water");
    }

    #[test]
    fn every_barrel_of_grain_is_a_block_the_server_accepts_and_no_empty_one_is() {
        for goods in [BLOCK_GRAIN, BLOCK_SEEDS, BLOCK_MILLET] {
            for jugs in 1..=BARREL_JUGS {
                let barrel = barrel_of_goods(goods, jugs).unwrap();
                assert!(is_known_block(barrel), "{goods} x{jugs} is an id the anti-cheat refuses");
                assert!(is_barrel(barrel));
                assert_eq!(barrel_contents(barrel), None, "a barrel of {goods} says it holds water");
            }
            assert_eq!(barrel_of_goods(goods, 0), Some(BLOCK_BARREL));
            let hollow = barrel_of_goods(goods, 1).unwrap() & !VARIANT_MASK;
            assert!(!is_known_block(hollow), "a grain barrel of nothing is a second empty barrel");
        }
    }

    #[test]
    fn a_barrel_cannot_turn_the_sea_into_something_you_can_drink() {
        let river = barrel_after_pouring(BLOCK_BARREL, jug_of(Water::Fresh)).unwrap();
        let spoiled = barrel_after_pouring(river, jug_of(Water::Salt)).unwrap();
        let (_, jug) = barrel_after_dipping(spoiled).unwrap();
        assert_eq!(vessel_water(jug), Water::Salt, "the sea came out of the barrel fresh");
        // Pond water after the sea does not make it better either.
        let still = barrel_after_pouring(spoiled, jug_of(Water::Standing)).unwrap();
        assert_eq!(barrel_contents(still).map(|(kind, _)| kind), Some(Water::Salt));
        // ...but a barrel dipped dry holds nothing, and nothing has no kind.
        let (dry, _) = barrel_after_dipping(barrel_of(Water::Salt, 1)).unwrap();
        assert_eq!(dry, BLOCK_BARREL);
        let fresh_again = barrel_after_pouring(dry, jug_of(Water::Fresh)).unwrap();
        assert_eq!(barrel_contents(fresh_again), Some((Water::Fresh, 1)));
    }

    #[test]
    fn only_a_full_jug_pours_and_every_barrel_is_a_block_the_server_accepts() {
        assert_eq!(barrel_after_pouring(BLOCK_BARREL, BLOCK_JUG), None);
        assert_eq!(barrel_after_pouring(BLOCK_BARREL, BLOCK_STONE), None);
        assert_eq!(barrel_after_pouring(BLOCK_STONE, jug_of(Water::Fresh)), None);
        assert_eq!(barrel_after_dipping(BLOCK_BARREL), None);
        assert_eq!(barrel_after_drinking(BLOCK_BARREL), None);
        assert_eq!(barrel_after_drinking(BLOCK_STONE), None);
        for kind in [Water::Fresh, Water::Standing, Water::Salt] {
            for jugs in 0..=BARREL_JUGS {
                let barrel = barrel_of(kind, jugs);
                assert!(is_known_block(barrel), "{kind:?} x{jugs} is an id the anti-cheat refuses");
                assert!(is_barrel(barrel));
            }
        }
    }
}

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
    (BLOCK_STONE_AXE_HEAD, "stone_axe_head"),
    (BLOCK_STONE_PICK_HEAD, "stone_pick_head"),
    (BLOCK_FLINT_KNIFE, "flint_knife"),
    (BLOCK_STONE_AXE, "stone_axe"),
    (BLOCK_STONE_PICKAXE, "stone_pickaxe"),
    // What grows, what it feeds you, and the fire you cook it over --
    // appended in that order because it is the order a player meets
    // them: you find a bush before you eat a berry, and you eat a berry
    // long before you get a fire lit.
    (BLOCK_BERRY_BUSH, "berry_bush"),
    (BLOCK_BARE_BUSH, "bare_bush"),
    (BLOCK_MUSHROOM, "mushroom"),
    (BLOCK_REEDS, "reeds"),
    (BLOCK_FLOWER, "flower"),
    (BLOCK_BERRIES, "berries"),
    (BLOCK_RAW_MEAT, "raw_meat"),
    (BLOCK_COOKED_MEAT, "cooked_meat"),
    (BLOCK_HIDE, "hide"),
    (BLOCK_CAMPFIRE, "campfire"),
    (BLOCK_CAMPFIRE_LIT, "campfire_lit"),
    // The metal tools, by age and then by trade -- the order the ladder
    // is climbed rather than the order the three are used in.
    (BLOCK_COPPER_KNIFE, "copper_knife"),
    (BLOCK_COPPER_AXE, "copper_axe"),
    (BLOCK_COPPER_PICKAXE, "copper_pickaxe"),
    (BLOCK_BRONZE_KNIFE, "bronze_knife"),
    (BLOCK_BRONZE_AXE, "bronze_axe"),
    (BLOCK_BRONZE_PICKAXE, "bronze_pickaxe"),
    (BLOCK_IRON_KNIFE, "iron_knife"),
    (BLOCK_IRON_AXE, "iron_axe"),
    (BLOCK_IRON_PICKAXE, "iron_pickaxe"),
    (BLOCK_KILN, "kiln"),
    (BLOCK_KILN_LIT, "kiln_lit"),
    (BLOCK_BRICK, "brick"),
    (BLOCK_BRICKS, "bricks"),
    (BLOCK_HOE, "hoe"),
    (BLOCK_SEEDS, "seeds"),
    (BLOCK_FARMLAND, "farmland"),
    (BLOCK_WHEAT, "wheat"),
    (BLOCK_WHEAT_RIPE, "wheat_ripe"),
    (BLOCK_GRAIN, "grain"),
    (BLOCK_DOUGH, "dough"),
    (BLOCK_BREAD, "bread"),
    (BLOCK_NATIVE_COPPER, "native_copper"),
    (BLOCK_VESSEL_RAW, "vessel_raw"),
    (BLOCK_VESSEL, "vessel"),
    (BLOCK_MOULD_RAW, "mould_raw"),
    (BLOCK_MOULD, "mould"),
    (BLOCK_BLOOMERY, "bloomery"),
    (BLOCK_BLOOMERY_LIT, "bloomery_lit"),
    (BLOCK_IRON_BLOOM, "iron_bloom"),
    (BLOCK_ICE, "ice"),
    // Hides, and what they become. Appended in the order of the chain,
    // like the flint tools above.
    (BLOCK_LEATHER, "leather"),
    (BLOCK_DRYING_RACK, "drying_rack"),
    // The jug, which is the pottery chain's answer to thirst.
    (BLOCK_JUG_RAW, "jug_raw"),
    (BLOCK_JUG, "jug"),
    (BLOCK_JUG_WATER, "jug_water"),
    // What a person wears, head down, leather then bronze then iron --
    // which is the order they become possible in.
    (BLOCK_LEATHER_CAP, "leather_cap"),
    (BLOCK_LEATHER_TUNIC, "leather_tunic"),
    (BLOCK_LEATHER_LEGGINGS, "leather_leggings"),
    (BLOCK_LEATHER_BOOTS, "leather_boots"),
    (BLOCK_BRONZE_HELM, "bronze_helm"),
    (BLOCK_BRONZE_CUIRASS, "bronze_cuirass"),
    (BLOCK_BRONZE_GREAVES, "bronze_greaves"),
    (BLOCK_BRONZE_BOOTS, "bronze_boots"),
    (BLOCK_IRON_HELM, "iron_helm"),
    (BLOCK_IRON_CUIRASS, "iron_cuirass"),
    (BLOCK_IRON_GREAVES, "iron_greaves"),
    (BLOCK_IRON_BOOTS, "iron_boots"),
    // What a felled tree leaves lying on the ground.
    (BLOCK_STRIPPED_LOG, "stripped_log"),
    // What there is to forage.
    (BLOCK_ROOTS, "roots"),
    (BLOCK_ROOT, "root"),
    (BLOCK_ROASTED_ROOT, "roasted_root"),
    (BLOCK_TOADSTOOL, "toadstool"),
    // What the rack makes of a hunt -- see `BLOCK_DRIED_MEAT`.
    (BLOCK_DRIED_MEAT, "dried_meat"),
    // The fleece, and what is made of it. The garments have no pictures
    // of their own -- see `garment_tint` -- but they still need names
    // here, because this table is also what the hotbar and the pack
    // screen look an item up in.
    (BLOCK_WOOL, "wool"),
    (BLOCK_WOOL_CAP, "wool_cap"),
    (BLOCK_WOOL_TUNIC, "wool_tunic"),
    (BLOCK_WOOL_LEGGINGS, "wool_leggings"),
    (BLOCK_WOOL_BOOTS, "wool_boots"),
    // 1.9: the copper bench -- four heads, and the two tools that had
    // no metal version at all.
    (BLOCK_COPPER_AXE_HEAD, "copper_axe_head"),
    (BLOCK_COPPER_PICK_HEAD, "copper_pick_head"),
    (BLOCK_COPPER_SHOVEL_HEAD, "copper_shovel_head"),
    (BLOCK_COPPER_HOE_HEAD, "copper_hoe_head"),
    (BLOCK_COPPER_SHOVEL, "copper_shovel"),
    (BLOCK_COPPER_HOE, "copper_hoe"),
    // 1.9: the torch, in its three states. See `BLOCK_TORCH`.
    (BLOCK_TORCH, "torch"),
    (BLOCK_TORCH_LIT, "torch_lit"),
    (BLOCK_TORCH_SPENT, "torch_spent"),
    // Three rocks, the bog's fuel, and what a carcass is made of. See
    // `BLOCK_SANDSTONE` and `BLOCK_SINEW`.
    (BLOCK_SANDSTONE, "sandstone"),
    (BLOCK_LIMESTONE, "limestone"),
    (BLOCK_GRANITE, "granite"),
    // What water leaves in a cave. See `dripstone`.
    (BLOCK_STALAGMITE, "stalagmite"),
    (BLOCK_STALACTITE, "stalactite"),
    (BLOCK_PEAT, "peat"),
    (BLOCK_DRIED_PEAT, "dried_peat"),
    (BLOCK_SINEW, "sinew"),
    (BLOCK_BONE, "bone"),
    (BLOCK_CARCASS_HARE, "carcass_hare"),
    (BLOCK_CARCASS_DEER, "carcass_deer"),
    (BLOCK_CARCASS_BOAR, "carcass_boar"),
    (BLOCK_CARCASS_WOLF, "carcass_wolf"),
    (BLOCK_CARCASS_SHEEP, "carcass_sheep"),
    // The honest tool chain, bog iron, and what food becomes. See
    // `BLOCK_CORD`.
    (BLOCK_CORD, "cord"),
    // The raft and its two made parts. See `BLOCK_RAFT`.
    (BLOCK_SAIL, "sail"),
    (BLOCK_OAR, "oar"),
    (BLOCK_RAFT, "raft"),
    (BLOCK_WEDGED_AXE, "wedged_axe"),
    (BLOCK_WEDGED_PICKAXE, "wedged_pickaxe"),
    (BLOCK_FLINT_SPEAR, "flint_spear"),
    (BLOCK_RUSTY_STONE, "rusty_stone"),
    (BLOCK_IRON_DUST, "iron_dust"),
    (BLOCK_WHETSTONE, "whetstone"),
    (BLOCK_SLAG, "slag"),
    (BLOCK_STEEL_INGOT, "steel_ingot"),
    (BLOCK_STREAM_TIN, "stream_tin"),
    // Wild bees. See `bees`.
    (BLOCK_WILD_HIVE, "wild_hive"),
    (BLOCK_HONEY, "honey"),
    (BLOCK_BEESWAX, "beeswax"),
    // Fishing. See `fishing`.
    (BLOCK_FISH_TRAP, "fish_trap"),
    (BLOCK_FISHING_ROD, "fishing_rod"),
    (BLOCK_COPPER_HOOK, "copper_hook"),
    // ...and what goes on the hook. See `fishing::Bait`.
    (BLOCK_WORM, "worm"),
    (BLOCK_GRUB, "grub"),
    // **The roasted handful was missing from this list**, which is the
    // list the client builds its name-to-picture lookup from: the block
    // existed, had a name and had a picture on disk, and drew the magenta
    // placeholder because nothing here pointed at it.
    (BLOCK_ROASTED_GRUBS, "roasted_grubs"),
    (BLOCK_FISHING_FLY, "fishing_fly"),
    (BLOCK_ROTTEN, "rotten"),
    (BLOCK_DUNG, "dung"),
    (BLOCK_APPLE_LEAVES, "apple_leaves"),
    (BLOCK_APPLE_LEAVES_FRUIT, "apple_leaves_fruit"),
    (BLOCK_APPLE, "apple"),
    (BLOCK_PEG, "peg"),
    // The nail and the frame it and the peg hold. See `BLOCK_FRAME`.
    (BLOCK_NAILS, "nails"),
    (BLOCK_FRAME, "frame"),
    // The door, in its two halves. See `BLOCK_DOOR`.
    (BLOCK_DOOR, "door"),
    (BLOCK_DOOR_TOP, "door_top"),
    (BLOCK_PEGGED_PLANKS, "pegged_planks"),
    (BLOCK_PEGGED_BIRCH_PLANKS, "pegged_birch_planks"),
    (BLOCK_HARE_MEAT, "hare_meat"),
    (BLOCK_FOWL_MEAT, "fowl_meat"),
    (BLOCK_BEAR_MEAT, "bear_meat"),
    (BLOCK_WOLF_MEAT, "wolf_meat"),
    (BLOCK_PELT, "pelt"),
    (BLOCK_BEAR_HIDE, "bear_hide"),
    (BLOCK_FEATHER, "feather"),
    (BLOCK_FAT, "fat"),
    (BLOCK_RIBS, "ribs"),
    (BLOCK_ROASTED_RIBS, "roasted_ribs"),
    (BLOCK_CARCASS_BEAR, "carcass_bear"),
    (BLOCK_CARCASS_FOWL, "carcass_fowl"),
    // The savanna's three. See `animals::Species::Zebra`.
    (BLOCK_CARCASS_ZEBRA, "carcass_zebra"),
    (BLOCK_CARCASS_ANTELOPE, "carcass_antelope"),
    (BLOCK_CARCASS_LION, "carcass_lion"),
    // ...and the plains' horse, beside them. See `animals::Species::Horse`.
    (BLOCK_CARCASS_HORSE, "carcass_horse"),
    (BLOCK_BASALT, "basalt"),
    (BLOCK_BRACKET_FUNGUS, "bracket_fungus"),
    // Fur, which shares the leather set's pictures the way the wool and
    // the metals do -- see `garment_tint`.
    (BLOCK_FUR_HOOD, "fur_hood"),
    (BLOCK_FUR_CLOAK, "fur_cloak"),
    // Where seed comes from, and what bread is made of on the way.
    (BLOCK_WILD_WHEAT, "wild_wheat"),
    (BLOCK_FLOUR, "flour"),
    // The nest, full and empty, and what is in it.
    (BLOCK_NEST_EGGS, "nest_eggs"),
    (BLOCK_NEST, "nest"),
    (BLOCK_EGG, "egg"),
    // The other three spears, which share the flint one's picture --
    // see `spear_tint`.
    (BLOCK_BONE_SPEAR, "bone_spear"),
    (BLOCK_COPPER_SPEAR, "copper_spear"),
    (BLOCK_BRONZE_SPEAR, "bronze_spear"),
    (BLOCK_IRON_SPEAR, "iron_spear"),
    // Furniture, and the straw that comes before it.
    (BLOCK_STRAW_BED, "straw_bed"),
    (BLOCK_BED, "bed"),
    (BLOCK_STOOL, "stool"),
    (BLOCK_TABLE, "table"),
    // The stool with a back, which faces a way. See `BLOCK_CHAIR`.
    (BLOCK_CHAIR, "chair"),
    (BLOCK_BUSH_LEAVES, "bush_leaves"),
    (BLOCK_BONES, "bones"),
    // The skeletons past the eighth species. See `BLOCK_BONES_2`.
    (BLOCK_BONES_2, "bones_2"),
    // ...and past the sixteenth. See `BLOCK_BONES_3`.
    (BLOCK_BONES_3, "bones_3"),
    // ...and past the twenty-fourth, before anybody needs it. See `BLOCK_BONES_4`.
    (BLOCK_BONES_4, "bones_4"),
    // The water barrel, in its three waters. See `BLOCK_BARREL`.
    (BLOCK_BARREL, "barrel"),
    (BLOCK_BARREL_STANDING, "barrel_standing"),
    (BLOCK_BARREL_SALT, "barrel_salt"),
    // ...and in its three grains. See `BLOCK_BARREL_GRAIN`.
    (BLOCK_BARREL_GRAIN, "barrel_grain"),
    (BLOCK_BARREL_SEEDS, "barrel_seeds"),
    (BLOCK_BARREL_MILLET, "barrel_millet"),
    // ---- savanna ----
    // The acacia's canopy and the termite mound. See
    // `BLOCK_ACACIA_LEAVES` and `BLOCK_TERMITE_MOUND`.
    (BLOCK_ACACIA_LEAVES, "acacia_leaves"),
    (BLOCK_TERMITE_MOUND, "termite_mound"),
    // The forest's maple. See `BLOCK_MAPLE_LEAVES`.
    (BLOCK_MAPLE_LEAVES, "maple_leaves"),
    // The savanna's bare ground. See `BLOCK_SANDY_SOIL`.
    (BLOCK_SANDY_SOIL, "sandy_soil"),
    // ...and the dry grass that grows on it. See `BLOCK_DRY_GRASS`.
    (BLOCK_DRY_GRASS, "dry_grass"),
    // ...and the dry turf it and the savanna stand on. See `BLOCK_DRY_TURF`.
    (BLOCK_DRY_TURF, "dry_turf"),
    // Cotton, from the wild stand to what is worn -- the order of the
    // chain, which is the order a player meets it -- and what frost
    // leaves of a field. See `BLOCK_WILD_COTTON`.
    (BLOCK_WILD_COTTON, "wild_cotton"),
    (BLOCK_COTTON_SEEDS, "cotton_seeds"),
    (BLOCK_COTTON_PLANT, "cotton_plant"),
    (BLOCK_COTTON_RIPE, "cotton_ripe"),
    (BLOCK_COTTON, "cotton"),
    // Millet. See `BLOCK_WILD_MILLET`.
    (BLOCK_WILD_MILLET, "wild_millet"),
    (BLOCK_MILLET, "millet"),
    (BLOCK_MILLET_PLANT, "millet_plant"),
    (BLOCK_MILLET_RIPE, "millet_ripe"),
    (BLOCK_MILLET_PORRIDGE, "millet_porridge"),
    (BLOCK_BOWL_RAW, "bowl_raw"),
    (BLOCK_BOWL, "bowl"),
    (BLOCK_STEW, "stew"),
    (BLOCK_BOWL_MILK, "bowl_milk"),
    (BLOCK_CAIRN, "cairn"),
    (BLOCK_LODESTONE, "lodestone"),
    (BLOCK_WATER_COMPASS, "water_compass"),
    // The shore. See `shore`.
    (BLOCK_MUSSEL_BED, "mussel_bed"),
    (BLOCK_MUSSEL_ROCK, "mussel_rock"),
    (BLOCK_MUSSELS, "mussels"),
    (BLOCK_COOKED_MUSSELS, "cooked_mussels"),
    (BLOCK_STARFISH, "starfish"),
    (BLOCK_CRAB_MEAT, "crab_meat"),
    (BLOCK_COOKED_CRAB, "cooked_crab"),
    // Building in stages. See `build`.
    (BLOCK_HANDFUL_EARTH, "handful_earth"),
    (BLOCK_HANDFUL_SAND, "handful_sand"),
    (BLOCK_HANDFUL_GRAVEL, "handful_gravel"),
    (BLOCK_HANDFUL_CLAY, "handful_clay"),
    (BLOCK_STONE_CHIPS, "stone_chips"),
    (BLOCK_QUICKLIME, "quicklime"),
    (BLOCK_MORTAR, "mortar"),
    (BLOCK_DAUB, "daub"),
    (BLOCK_COB, "cob"),
    (BLOCK_BRICK_COURSES, "brick_courses"),
    (BLOCK_DRY_BRICKS, "dry_bricks"),
    (BLOCK_DRY_STONE_WALL, "dry_stone_wall"),
    (BLOCK_WATTLE, "wattle"),
    (BLOCK_COB_WALL, "cob_wall"),
    // The horse's tack. See `BLOCK_SADDLE`.
    (BLOCK_SADDLE, "saddle"),
    (BLOCK_SADDLEBAGS, "saddlebags"),
    (BLOCK_CLOTH, "cloth"),
    (BLOCK_CLOTH_CAP, "cloth_cap"),
    (BLOCK_CLOTH_TUNIC, "cloth_tunic"),
    (BLOCK_CLOTH_TROUSERS, "cloth_trousers"),
    (BLOCK_CLOTH_WRAPS, "cloth_wraps"),
    (BLOCK_WITHERED_CROP, "withered_crop"),
    // The experimental trees' pieces. See `BLOCK_TWIG`.
    (BLOCK_TWIG, "twig"),
    (BLOCK_BOUGH, "bough"),
    // The sea floor: what grows there, what a reef is built of, and what
    // a player brings up. See `BLOCK_KELP`.
    (BLOCK_KELP, "kelp"),
    (BLOCK_KELP_TOP, "kelp_top"),
    (BLOCK_SEAGRASS, "seagrass"),
    (BLOCK_SEA_FAN, "sea_fan"),
    (BLOCK_STAGHORN_CORAL, "staghorn_coral"),
    (BLOCK_BRAIN_CORAL, "brain_coral"),
    (BLOCK_FIRE_CORAL, "fire_coral"),
    (BLOCK_SHELL, "shell"),
    (BLOCK_KELP_FROND, "kelp_frond"),
    (BLOCK_DRIED_KELP, "dried_kelp"),
    (BLOCK_RAW_FISH, "raw_fish"),
    (BLOCK_COOKED_FISH, "cooked_fish"),
    // The palm and the swamp. See `BLOCK_PALM_TRUNK` and `BLOCK_MUD`.
    (BLOCK_PALM_TRUNK, "palm_trunk"),
    (BLOCK_PALM_FRONDS, "palm_fronds"),
    (BLOCK_PALM_COCONUTS, "palm_coconuts"),
    (BLOCK_COCONUT, "coconut"),
    (BLOCK_MUD, "mud"),
    (BLOCK_LILY_PAD, "lily_pad"),
    (BLOCK_HANGING_MOSS, "hanging_moss"),
    // A swamp's snag standing in its pool. See `BLOCK_DROWNED_BOUGH`.
    (BLOCK_DROWNED_TWIG, "drowned_twig"),
    (BLOCK_DROWNED_BOUGH, "drowned_bough"),
    // What a wound is dressed with. See `BLOCK_BANDAGE`.
    (BLOCK_BANDAGE, "bandage"),
    (BLOCK_SPLINT, "splint"),
    (BLOCK_POULTICE, "poultice"),
    // What is slung over the shoulders. See `BLOCK_RUCKSACK`.
    (BLOCK_RUCKSACK, "rucksack"),
    // Fires in the ground. See `pit`.
    (BLOCK_BRICK_RAW, "brick_raw"),
    (BLOCK_PIT_KILN, "pit_kiln"),
    (BLOCK_PIT_KILN_FIBRE, "pit_kiln_fibre"),
    (BLOCK_PIT_KILN_LOGS, "pit_kiln_logs"),
    (BLOCK_PIT_KILN_LIT, "pit_kiln_lit"),
    (BLOCK_LOG_PILE, "log_pile"),
    (BLOCK_LOG_PILE_LIT, "log_pile_lit"),
    (BLOCK_CHARCOAL_PILE, "charcoal_pile"),
    (BLOCK_FIREPIT, "firepit"),
    (BLOCK_FIREPIT_LIT, "firepit_lit"),
    // Fire that got loose, and the torch that stands. See `wildfire`.
    (BLOCK_BURNING_LOG, "burning_log"),
    (BLOCK_BURNING_PLANKS, "burning_planks"),
    (BLOCK_CHARRED_LOG, "charred_log"),
    (BLOCK_CHARRED_PLANKS, "charred_planks"),
    (BLOCK_STANDING_TORCH, "standing_torch"),
    (BLOCK_STANDING_TORCH_LIT, "standing_torch_lit"),
    (BLOCK_STANDING_TORCH_OUT, "standing_torch_out"),
    (BLOCK_SNOW_COVER, "snow_cover"),
    (BLOCK_SANDSTONE_BRICKS, "sandstone_bricks"),
    (BLOCK_HUMAN_FLESH, "human_flesh"),
    (BLOCK_ROAST_HUMAN_FLESH, "roast_human_flesh"),
    (BLOCK_LEAF_HANDFUL, "leaf_handful"),
    (BLOCK_LEAF_LITTER, "leaf_litter"),
    (BLOCK_RESIN, "resin"),
    // The wild plants. See `BLOCK_FIREWEED`.
    (BLOCK_FIREWEED, "fireweed"),
    (BLOCK_CATTAIL, "cattail"),
    (BLOCK_NETTLE, "nettle"),
    (BLOCK_BRACKEN, "bracken"),
    (BLOCK_ARUNDO, "arundo"),
    (BLOCK_CANE, "cane"),
    (BLOCK_BILBERRY, "bilberry"),
    (BLOCK_BILBERRY_BARE, "bilberry_bare"),
    (BLOCK_STRAWBERRY, "strawberry"),
    (BLOCK_STRAWBERRY_BARE, "strawberry_bare"),
    (BLOCK_PLANTAIN, "plantain"),
    (BLOCK_FERN, "fern"),
    (BLOCK_SUNDEW, "sundew"),
    // The fir and the saxaul. See `BLOCK_FIR_LOG` and `wood`.
    (BLOCK_FIR_LOG, "fir_log"),
    (BLOCK_FIR_NEEDLES, "fir_needles"),
    (BLOCK_FIR_PLANKS, "fir_planks"),
    (BLOCK_PEGGED_FIR_PLANKS, "pegged_fir_planks"),
    (BLOCK_SAXAUL_LOG, "saxaul_log"),
    (BLOCK_SAXAUL_LEAVES, "saxaul_leaves"),
    (BLOCK_SAXAUL_PLANKS, "saxaul_planks"),
    (BLOCK_PEGGED_SAXAUL_PLANKS, "pegged_saxaul_planks"),
    // ---- the ground. See `ground`. ----
    (BLOCK_SANDSTONE_COBBLE, "sandstone_cobble"),
    (BLOCK_SANDSTONE_GRAVEL, "sandstone_gravel"),
    (BLOCK_SANDSTONE_PEBBLE, "sandstone_pebble"),
    (BLOCK_LIMESTONE_COBBLE, "limestone_cobble"),
    (BLOCK_LIMESTONE_GRAVEL, "limestone_gravel"),
    (BLOCK_LIMESTONE_SAND, "limestone_sand"),
    (BLOCK_LIMESTONE_PEBBLE, "limestone_pebble"),
    (BLOCK_GRANITE_COBBLE, "granite_cobble"),
    (BLOCK_GRANITE_GRAVEL, "granite_gravel"),
    (BLOCK_GRANITE_SAND, "granite_sand"),
    (BLOCK_GRANITE_PEBBLE, "granite_pebble"),
    (BLOCK_BASALT_COBBLE, "basalt_cobble"),
    (BLOCK_BASALT_GRAVEL, "basalt_gravel"),
    (BLOCK_BASALT_SAND, "basalt_sand"),
    (BLOCK_BASALT_PEBBLE, "basalt_pebble"),
    (BLOCK_SHALE, "shale"),
    (BLOCK_SHALE_COBBLE, "shale_cobble"),
    (BLOCK_SHALE_GRAVEL, "shale_gravel"),
    (BLOCK_SHALE_SAND, "shale_sand"),
    (BLOCK_SHALE_PEBBLE, "shale_pebble"),
    (BLOCK_CHALK, "chalk"),
    (BLOCK_CHALK_COBBLE, "chalk_cobble"),
    (BLOCK_CHALK_GRAVEL, "chalk_gravel"),
    (BLOCK_CHALK_SAND, "chalk_sand"),
    (BLOCK_CHALK_PEBBLE, "chalk_pebble"),
    (BLOCK_DOLOMITE, "dolomite"),
    (BLOCK_DOLOMITE_COBBLE, "dolomite_cobble"),
    (BLOCK_DOLOMITE_GRAVEL, "dolomite_gravel"),
    (BLOCK_DOLOMITE_SAND, "dolomite_sand"),
    (BLOCK_DOLOMITE_PEBBLE, "dolomite_pebble"),
    (BLOCK_MARBLE, "marble"),
    (BLOCK_MARBLE_COBBLE, "marble_cobble"),
    (BLOCK_MARBLE_GRAVEL, "marble_gravel"),
    (BLOCK_MARBLE_SAND, "marble_sand"),
    (BLOCK_MARBLE_PEBBLE, "marble_pebble"),
    (BLOCK_QUARTZITE, "quartzite"),
    (BLOCK_QUARTZITE_COBBLE, "quartzite_cobble"),
    (BLOCK_QUARTZITE_GRAVEL, "quartzite_gravel"),
    (BLOCK_QUARTZITE_SAND, "quartzite_sand"),
    (BLOCK_QUARTZITE_PEBBLE, "quartzite_pebble"),
    (BLOCK_GNEISS, "gneiss"),
    (BLOCK_GNEISS_COBBLE, "gneiss_cobble"),
    (BLOCK_GNEISS_GRAVEL, "gneiss_gravel"),
    (BLOCK_GNEISS_SAND, "gneiss_sand"),
    (BLOCK_GNEISS_PEBBLE, "gneiss_pebble"),
    (BLOCK_DIORITE, "diorite"),
    (BLOCK_DIORITE_COBBLE, "diorite_cobble"),
    (BLOCK_DIORITE_GRAVEL, "diorite_gravel"),
    (BLOCK_DIORITE_SAND, "diorite_sand"),
    (BLOCK_DIORITE_PEBBLE, "diorite_pebble"),
    (BLOCK_GABBRO, "gabbro"),
    (BLOCK_GABBRO_COBBLE, "gabbro_cobble"),
    (BLOCK_GABBRO_GRAVEL, "gabbro_gravel"),
    (BLOCK_GABBRO_SAND, "gabbro_sand"),
    (BLOCK_GABBRO_PEBBLE, "gabbro_pebble"),
    (BLOCK_ANDESITE, "andesite"),
    (BLOCK_ANDESITE_COBBLE, "andesite_cobble"),
    (BLOCK_ANDESITE_GRAVEL, "andesite_gravel"),
    (BLOCK_ANDESITE_SAND, "andesite_sand"),
    (BLOCK_ANDESITE_PEBBLE, "andesite_pebble"),
    (BLOCK_TUFF, "tuff"),
    (BLOCK_TUFF_COBBLE, "tuff_cobble"),
    (BLOCK_TUFF_GRAVEL, "tuff_gravel"),
    (BLOCK_TUFF_SAND, "tuff_sand"),
    (BLOCK_TUFF_PEBBLE, "tuff_pebble"),
    (BLOCK_LOAM, "loam"),
    (BLOCK_CHERNOZEM, "chernozem"),
    (BLOCK_PODZOL, "podzol"),
    (BLOCK_LATERITE, "laterite"),
    (BLOCK_SOLONCHAK, "solonchak"),
    (BLOCK_LOESS, "loess"),
    (BLOCK_GLEY, "gley"),
    (BLOCK_RENDZINA, "rendzina"),
    (BLOCK_ANDOSOL, "andosol"),
    (BLOCK_PERMAFROST, "permafrost"),
    (BLOCK_FEATHER_GRASS, "feather_grass"),
    (BLOCK_SEDGE, "sedge"),
    (BLOCK_COTTON_GRASS, "cotton_grass"),
    (BLOCK_FESCUE, "fescue"),
    (BLOCK_MARRAM, "marram"),
    (BLOCK_ELEPHANT_GRASS, "elephant_grass"),
    (BLOCK_BLUEGRASS, "bluegrass"),
    (BLOCK_TIMOTHY, "timothy"),
    (BLOCK_TUSSOCK_GRASS, "tussock_grass"),
    (BLOCK_SPINIFEX, "spinifex"),
    (BLOCK_PINE_LOG, "pine_log"),
    (BLOCK_PINE_NEEDLES, "pine_needles"),
    (BLOCK_PINE_PLANKS, "pine_planks"),
    (BLOCK_PEGGED_PINE_PLANKS, "pegged_pine_planks"),
    (BLOCK_WILLOW_LOG, "willow_log"),
    (BLOCK_WILLOW_LEAVES, "willow_leaves"),
    (BLOCK_WILLOW_PLANKS, "willow_planks"),
    (BLOCK_PEGGED_WILLOW_PLANKS, "pegged_willow_planks"),
    (BLOCK_FIR_TWIG, "fir_twig"),
    (BLOCK_FIR_BOUGH, "fir_bough"),
    (BLOCK_SAXAUL_TWIG, "saxaul_twig"),
    (BLOCK_SAXAUL_BOUGH, "saxaul_bough"),
    (BLOCK_PINE_TWIG, "pine_twig"),
    (BLOCK_PINE_BOUGH, "pine_bough"),
    (BLOCK_WILLOW_TWIG, "willow_twig"),
    (BLOCK_WILLOW_BOUGH, "willow_bough"),
    (BLOCK_MOSS, "moss"),
    // The four workshops. See `BLOCK_WORKBENCH`.
    (BLOCK_WORKBENCH, "workbench"),
    (BLOCK_MASON_BLOCK, "mason_block"),
    (BLOCK_POTTERS_WHEEL, "potters_wheel"),
    (BLOCK_LEATHER_BENCH, "leather_bench"),
    // A barter stall. See `BLOCK_STALL`.
    (BLOCK_STALL, "stall"),
    // Steps, and the three roofs built out of steps and slabs. See
    // `BLOCK_TILE_ROOF`.
    (BLOCK_PLANK_STAIRS, "plank_stairs"),
    (BLOCK_COBBLESTONE_STAIRS, "cobblestone_stairs"),
    (BLOCK_TILE_ROOF, "tile_roof"),
    (BLOCK_TILE_SLAB, "tile_slab"),
    (BLOCK_THATCH_ROOF, "thatch_roof"),
    (BLOCK_THATCH_SLAB, "thatch_slab"),
    (BLOCK_BRANCH_ROOF, "branch_roof"),
    (BLOCK_BRANCH_SLAB, "branch_slab"),
    (BLOCK_SALT, "salt"),
    (BLOCK_SALTED_MEAT, "salted_meat"),
    (BLOCK_SALTED_FISH, "salted_fish"),
    (BLOCK_DRIED_FISH, "dried_fish"),
    (BLOCK_DRIED_SALTED_MEAT, "dried_salted_meat"),
    (BLOCK_DRIED_SALTED_FISH, "dried_salted_fish"),
    // The hide frame back beside the rack of two by two, and the sod between
    // wet peat and a brick. See `BLOCK_HIDE_FRAME` and `BLOCK_DRYING_PEAT`.
    (BLOCK_HIDE_FRAME, "hide_frame"),
    (BLOCK_DRYING_PEAT, "drying_peat"),
    (BLOCK_STAKE, "stake"),
    (BLOCK_PROP, "prop"),
    (BLOCK_PLANTER, "planter"),
    (BLOCK_WINDOW_LATTICE, "window_lattice"),
    // Striking and paring, and the block struck on. See `BLOCK_STONE_HAMMER`.
    (BLOCK_STONE_HAMMER, "stone_hammer"),
    (BLOCK_BRONZE_HAMMER, "bronze_hammer"),
    (BLOCK_IRON_HAMMER, "iron_hammer"),
    (BLOCK_FLINT_CHISEL, "flint_chisel"),
    (BLOCK_BRONZE_CHISEL, "bronze_chisel"),
    (BLOCK_ANVIL, "anvil"),
    // The saws, and the two stations that ask for an edge. See
    // `BLOCK_COPPER_SAW`, `BLOCK_SAWHORSE` and `BLOCK_HONING_STONE`.
    (BLOCK_COPPER_SAW, "copper_saw"),
    (BLOCK_BRONZE_SAW, "bronze_saw"),
    (BLOCK_IRON_SAW, "iron_saw"),
    (BLOCK_SAWHORSE, "sawhorse"),
    (BLOCK_HONING_STONE, "honing_stone"),
    // The one night's shelter. See `BLOCK_LEAN_TO`.
    (BLOCK_LEAN_TO, "lean_to"),
    // What a death leaves, in its two states. The backpack above is
    // still in this list and still loads; nothing makes a new one. See
    // `BLOCK_CORPSE`.
    (BLOCK_CORPSE, "corpse"),
    (BLOCK_REMAINS, "remains"),
    // The ten old answers: the larder, the trapline and the pack. See
    // `BLOCK_CURD` and the modules each row names.
    (BLOCK_CURD, "curd"),
    (BLOCK_CHEESE, "cheese"),
    (BLOCK_JUG_MUST, "jug_must"),
    (BLOCK_JUG_MEAD, "jug_mead"),
    (BLOCK_PEMMICAN, "pemmican"),
    (BLOCK_BIRCH_BARK, "birch_bark"),
    (BLOCK_TAR, "tar"),
    (BLOCK_TARRED_TUNIC, "tarred_tunic"),
    (BLOCK_WILLOW_BARK, "willow_bark"),
    (BLOCK_SNARE, "snare"),
    (BLOCK_PIT_COVER, "pit_cover"),
    (BLOCK_SALT_PAN, "salt_pan"),
    (BLOCK_SNOWSHOES, "snowshoes"),
    (BLOCK_NETTLE_BAST, "nettle_bast"),
    (BLOCK_SALT_PAN_BRINE, "salt_pan_brine"),
    (BLOCK_SALT_PAN_SALT, "salt_pan_salt"),
    (BLOCK_SNARE_CAUGHT, "snare_caught"),
    (BLOCK_SNARE_SPRUNG, "snare_sprung"),
    // Winter feed. See `BLOCK_HAY`.
    (BLOCK_HAY, "hay"),
    (BLOCK_HAYSTACK, "haystack"),
];

/// What a player is allowed to put into the world: the client hotbar
/// offers exactly these, and the server validates against the same list
/// (see `is_placeable`) rather than trusting the client's choice.
pub const PLACEABLE_BLOCKS: &[BlockId] = &[
    // The ground's rocks, rubble, soils and grasses, and the two new
    // woods, on the terms of the blocks they stand in for. See `ground`.
    BLOCK_SANDSTONE_COBBLE,
    BLOCK_SANDSTONE_GRAVEL,
    BLOCK_SANDSTONE_PEBBLE,
    BLOCK_LIMESTONE_COBBLE,
    BLOCK_LIMESTONE_GRAVEL,
    BLOCK_LIMESTONE_SAND,
    BLOCK_LIMESTONE_PEBBLE,
    BLOCK_GRANITE_COBBLE,
    BLOCK_GRANITE_GRAVEL,
    BLOCK_GRANITE_SAND,
    BLOCK_GRANITE_PEBBLE,
    BLOCK_BASALT_COBBLE,
    BLOCK_BASALT_GRAVEL,
    BLOCK_BASALT_SAND,
    BLOCK_BASALT_PEBBLE,
    BLOCK_SHALE,
    BLOCK_SHALE_COBBLE,
    BLOCK_SHALE_GRAVEL,
    BLOCK_SHALE_SAND,
    BLOCK_SHALE_PEBBLE,
    BLOCK_CHALK,
    BLOCK_CHALK_COBBLE,
    BLOCK_CHALK_GRAVEL,
    BLOCK_CHALK_SAND,
    BLOCK_CHALK_PEBBLE,
    BLOCK_DOLOMITE,
    BLOCK_DOLOMITE_COBBLE,
    BLOCK_DOLOMITE_GRAVEL,
    BLOCK_DOLOMITE_SAND,
    BLOCK_DOLOMITE_PEBBLE,
    BLOCK_MARBLE,
    BLOCK_MARBLE_COBBLE,
    BLOCK_MARBLE_GRAVEL,
    BLOCK_MARBLE_SAND,
    BLOCK_MARBLE_PEBBLE,
    BLOCK_QUARTZITE,
    BLOCK_QUARTZITE_COBBLE,
    BLOCK_QUARTZITE_GRAVEL,
    BLOCK_QUARTZITE_SAND,
    BLOCK_QUARTZITE_PEBBLE,
    BLOCK_GNEISS,
    BLOCK_GNEISS_COBBLE,
    BLOCK_GNEISS_GRAVEL,
    BLOCK_GNEISS_SAND,
    BLOCK_GNEISS_PEBBLE,
    BLOCK_DIORITE,
    BLOCK_DIORITE_COBBLE,
    BLOCK_DIORITE_GRAVEL,
    BLOCK_DIORITE_SAND,
    BLOCK_DIORITE_PEBBLE,
    BLOCK_GABBRO,
    BLOCK_GABBRO_COBBLE,
    BLOCK_GABBRO_GRAVEL,
    BLOCK_GABBRO_SAND,
    BLOCK_GABBRO_PEBBLE,
    BLOCK_ANDESITE,
    BLOCK_ANDESITE_COBBLE,
    BLOCK_ANDESITE_GRAVEL,
    BLOCK_ANDESITE_SAND,
    BLOCK_ANDESITE_PEBBLE,
    BLOCK_TUFF,
    BLOCK_TUFF_COBBLE,
    BLOCK_TUFF_GRAVEL,
    BLOCK_TUFF_SAND,
    BLOCK_TUFF_PEBBLE,
    BLOCK_LOAM,
    BLOCK_CHERNOZEM,
    BLOCK_PODZOL,
    BLOCK_LATERITE,
    BLOCK_SOLONCHAK,
    BLOCK_LOESS,
    BLOCK_GLEY,
    BLOCK_RENDZINA,
    BLOCK_ANDOSOL,
    BLOCK_PERMAFROST,
    BLOCK_FEATHER_GRASS,
    BLOCK_SEDGE,
    BLOCK_COTTON_GRASS,
    BLOCK_FESCUE,
    BLOCK_MARRAM,
    BLOCK_ELEPHANT_GRASS,
    BLOCK_BLUEGRASS,
    BLOCK_TIMOTHY,
    BLOCK_TUSSOCK_GRASS,
    BLOCK_SPINIFEX,
    BLOCK_PINE_LOG,
    BLOCK_PINE_NEEDLES,
    BLOCK_PINE_PLANKS,
    BLOCK_WILLOW_LOG,
    BLOCK_WILLOW_LEAVES,
    BLOCK_WILLOW_PLANKS,
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
    // The fir's and the saxaul's, on the same terms. See `wood`.
    BLOCK_FIR_LOG,
    BLOCK_FIR_NEEDLES,
    BLOCK_SAXAUL_LOG,
    BLOCK_SAXAUL_LEAVES,
    // ---- savanna ----
    // The acacia's canopy, on the terms of the other two. The termite
    // mound is absent on the grass block's terms: breaking one gives
    // clay, so there is never a mound in a pack to put down.
    BLOCK_ACACIA_LEAVES,
    // ...and the maple's, on the same terms.
    BLOCK_MAPLE_LEAVES,
    // Sandy soil, on dirt's terms: ground you dug up and can put back.
    BLOCK_SANDY_SOIL,
    // Dry grass, on the tuft's terms: pulled up and put back, on dry ground.
    BLOCK_DRY_GRASS,
    // Dry turf, on the grass block's terms: lifted and put back.
    BLOCK_DRY_TURF,
    // The bare apple canopy only: the fruiting one would be a larder a
    // player could carry. A leaf put down is a plain leaf and never fruits
    // -- only a cell apples were picked from does (see
    // `BLOCK_APPLE_LEAVES_PICKED`) -- so this is building material and
    // not an orchard in a pack.
    BLOCK_APPLE_LEAVES,
    BLOCK_GLOWSTONE,
    BLOCK_PLANKS,
    BLOCK_BIRCH_PLANKS,
    BLOCK_FIR_PLANKS,
    BLOCK_SAXAUL_PLANKS,
    BLOCK_COBBLESTONE,
    BLOCK_TALL_GRASS,
    BLOCK_CACTUS,
    BLOCK_STICK,
    BLOCK_PEBBLE,
    BLOCK_FLINT,
    // A flake lies on the ground like the nodule, so it goes back down
    // like one. See its row in `blocks.rs` for the black block it was.
    BLOCK_FLINT_FLAKE,
    // A pole driven into flat ground: see `BLOCK_STANDING_TORCH`. Only the
    // pole; its top is written by the placement, as a bed's head is.
    BLOCK_STANDING_TORCH,
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
    // The floor of the world. Placeable on the same terms the other
    // three rocks are: a player who has a bronze pick and a stack of
    // basalt has earned the right to build a black wall with it.
    BLOCK_BASALT,
    // The tinder bracket, on the mushroom's terms: a player who picked
    // one may want it back on a log, and `can_grow_on` is what stops
    // them growing a fungus farm on the roof of a house -- it wants
    // timber under it, which means a player who wants a crop of these
    // has to lay the timber first. That is a chore with an answer, not
    // a decision, and it is why the fungus does not spread on its own.
    BLOCK_BRACKET_FUNGUS,
    // A stand of wild cereal, on the tuft's terms: pulled up and put
    // back. Seed is what it *gives*; the stand itself is a plant like
    // any other, and a player who dug one up should be able to plant it
    // again -- on turf, though, never on tilled earth (`can_grow_on`),
    // so this is not a second way to farm.
    BLOCK_WILD_WHEAT,
    // Furniture. Placed like any other block and broken back into
    // itself -- a chair you could not pick up again would be a chair
    // nobody moves.
    BLOCK_STRAW_BED,
    BLOCK_BED,
    BLOCK_STOOL,
    BLOCK_TABLE,
    BLOCK_CHAIR,
    // The workshops stand where they are put, like the furniture they sit
    // among, and come back whole when broken. See `BLOCK_WORKBENCH`.
    BLOCK_WORKBENCH,
    BLOCK_MASON_BLOCK,
    BLOCK_POTTERS_WHEEL,
    BLOCK_LEATHER_BENCH,
    BLOCK_ANVIL,
    // ...and the two stations of the edge, which come back whole as the
    // anvil does. See `BLOCK_SAWHORSE` and `BLOCK_HONING_STONE`.
    BLOCK_SAWHORSE,
    BLOCK_HONING_STONE,
    // ...and a lean-to, put down like the pallet it is and taken back whole
    // until it has been slept in (`BLOCK_LEAN_TO`).
    BLOCK_LEAN_TO,
    // ...and a stall, which comes back whole with its goods in the owner's
    // pack or spilled for anybody else (`stall`).
    BLOCK_STALL,
    // Steps and roofing, put down and taken back like the planks and the
    // cobbles they are cut from. See `BLOCK_TILE_ROOF`.
    BLOCK_STAKE,
    BLOCK_PROP,
    BLOCK_WINDOW_LATTICE,
    BLOCK_PLANTER,
    BLOCK_PLANK_STAIRS,
    BLOCK_COBBLESTONE_STAIRS,
    BLOCK_TILE_ROOF,
    BLOCK_TILE_SLAB,
    BLOCK_THATCH_ROOF,
    BLOCK_THATCH_SLAB,
    BLOCK_BRANCH_ROOF,
    BLOCK_BRANCH_SLAB,
    // A door, and only its lower half: the top is written by the placement,
    // as a bed's head is. See `BLOCK_DOOR`.
    BLOCK_DOOR,
    // A water barrel, and only the empty one: the other two are states
    // the world writes into a barrel that is already standing, and
    // breaking any of them gives this one back.
    BLOCK_BARREL,
    // Undergrowth, placed the way the canopy's leaf is.
    BLOCK_BUSH_LEAVES,
    // What grows. All four are put down the way a tuft of grass is:
    // they need something under them (see `can_grow_on`), and breaking
    // one gives it back.
    //
    // **The bare bush is placeable and the full one is not**, which
    // looks backwards and is not. What a player carries is a bush they
    // dug up, and a bush comes up bare -- the berries were what they
    // picked off it. Letting a full one be planted would mean carrying
    // a bush around as a larder.
    BLOCK_BARE_BUSH,
    BLOCK_MUSHROOM,
    // ...and the one beside it that is not food. Placeable for the same
    // reason the mushroom is -- a player who picked one may want it back
    // in the ground, and a cap that could be picked and never put down
    // would be a thing you carry for ever. See `food::harm` for what
    // eating it does.
    BLOCK_TOADSTOOL,
    BLOCK_REEDS,
    BLOCK_FLOWER,
    // The two wild plants that are picked whole, on the flower's terms: put
    // back where they will grow. See `BLOCK_PLANTAIN` and `BLOCK_SUNDEW`.
    BLOCK_PLANTAIN,
    BLOCK_SUNDEW,
    // A fire, laid but not lit. The burning one is deliberately absent:
    // fire is something you *start*, with flint and a spark, and a
    // player who could place a lit one would be carrying a burning
    // campfire in their pack.
    BLOCK_CAMPFIRE,
    // The kiln, on the same terms and for the same reasons.
    BLOCK_KILN,
    // A nodule of native copper, put back the way flint is.
    BLOCK_NATIVE_COPPER,
    // The bloomery, laid and unlit, on the same terms as the other two
    // hearths.
    BLOCK_BLOOMERY,
    // Seed. The two later stages of the crop are deliberately absent:
    // what a player carries is seed, and a handful of half-grown wheat
    // that could be planted would be a way to skip the waiting.
    BLOCK_SEEDS,
    // Not brickwork: that is laid a course at a time in the place it
    // stands, and never put down whole (`build`).
    // ...and the dry belt's, cut rather than fired. See the constant.
    BLOCK_SANDSTONE_BRICKS,
    // Ice, which is placeable for one reason: the surface it makes is a
    // thing to build *with*. A block a player can walk on and cannot
    // lay is a block the world owns and they do not.
    BLOCK_ICE,
    // Set down and picked up again like any other block. What is *in*
    // it is a separate question -- see `is_container`.
    BLOCK_JUG,
    // A drying rack, laid down beside a camp. The one new block here
    // that is a *thing in the world* rather than something carried.
    BLOCK_DRYING_RACK,
    // ...and the hide frame, the one-cell rack a skin is laced into, back
    // beside it. See `BLOCK_HIDE_FRAME`.
    BLOCK_HIDE_FRAME,
    // ...and a cairn, piled where a player wants a mark on their map. See
    // `BLOCK_CAIRN`.
    BLOCK_CAIRN,
    // Debarked timber, which is a building material like any other --
    // and one a player has plenty of the moment they fell a tree.
    BLOCK_STRIPPED_LOG,
    // A bale of wool. Its own row has said `placeable: true` since it
    // was added and this list did not, which is the whole reason
    // `the_list_of_what_can_be_placed_and_the_flag_that_says_so_are_one_answer`
    // now exists: the flag and the list are two spellings of one rule,
    // and nothing derived either from the other.
    BLOCK_WOOL,
    // The three rocks are building stone, and peat is a block of the
    // ground that can be put back where it came from. A carcass is not
    // placeable on purpose: it is where an animal fell, and a player who
    // could set one down would be carrying a dead deer in their pack.
    BLOCK_SANDSTONE,
    BLOCK_LIMESTONE,
    BLOCK_GRANITE,
    // A reef's two stones go back the way limestone does -- they are
    // limestone, in colour. The corals that grow as sprites and the kelp
    // are not here: they are water with something in it (`BLOCK_KELP`),
    // and a liquid is not placed.
    BLOCK_BRAIN_CORAL,
    BLOCK_FIRE_CORAL,
    BLOCK_PEAT,
    // A rusty stone is put back the way a pebble is.
    BLOCK_RUSTY_STONE,
    // Slag is laid like the stone it looks like. Stream tin is not here:
    // what comes up off a bank is tin ore, so there is never a pebble of
    // it in a pack to put back.
    BLOCK_SLAG,
    // A fish trap, set into water or wherever a player likes; it only fishes
    // with water on two sides (`fishing::trap_water`).
    BLOCK_FISH_TRAP,
    // Cotton: the wild stand on wild wheat's terms (turf, never a
    // field), and the seed on the wheat seed's. The growing and ripe
    // stages are absent for the reason the wheat's are, and so is the
    // withered crop -- see `BLOCK_WITHERED_CROP`.
    BLOCK_WILD_COTTON,
    BLOCK_COTTON_SEEDS,
    // Millet on cotton's terms: the wild stand, and the grain that is its
    // seed.
    BLOCK_WILD_MILLET,
    BLOCK_MILLET,
    // The three things set on the ground and left: a snare for a hare, a
    // cover over a pit, and a pan of the sea for the sun. See `snare`,
    // `pitfall` and `saltpan`.
    BLOCK_SNARE,
    BLOCK_PIT_COVER,
    BLOCK_SALT_PAN,
    // The winter's feed, built by the pen. See `BLOCK_HAYSTACK`.
    BLOCK_HAYSTACK,
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
/// The low ten bits: what the block actually is.
///
/// **Ten, not the twelve below the variant field**, since furniture learned
/// its wood (`WOOD_MASK`). No kind has ever reached 1024 -- the definitions
/// are indexed by a table that long (`blocks::INDEX_KINDS`) -- so bits ten
/// and eleven were zero in every id there has been, and a save or a packet
/// from before reads back as the same kinds with no wood, which is oak.
pub const KIND_MASK: BlockId = (1 << 10) - 1;

/// Which wood a piece of furniture is made of: bits ten and eleven, and the
/// sixteenth, above the variant field. See `furniture_wood`.
///
/// **One table, a wood in its id**, where the choice was between that and a
/// table of every wood (`fir_table`, `birch_table`...) -- six woods times
/// seven pieces, each a row, a name, a recipe and a line in every match that
/// names a table. The player said which: "не делай oak_table, делай просто
/// table с вариантом текстуры". It cannot be the variant field, which a
/// chair already spends on its facing and a bed on which half it is, and
/// three bits are what six woods need; the two bits the kinds never used and
/// the one above the variant are three, if not side by side.
pub const WOOD_LOW_SHIFT: u32 = 10;
/// The third bit of a furniture's wood. See `WOOD_LOW_SHIFT`.
pub const WOOD_HIGH_BIT: BlockId = 1 << 15;
/// Every bit of a furniture's wood.
pub const WOOD_MASK: BlockId = (0b11 << WOOD_LOW_SHIFT) | WOOD_HIGH_BIT;

/// Is this a piece of furniture that carries its wood in its id -- a stool,
/// a chair, a table, a plank bed, a chest, a door?
#[inline]
pub fn is_wooden_furniture(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_STOOL | BLOCK_CHAIR | BLOCK_TABLE | BLOCK_BED | BLOCK_CHEST | BLOCK_DOOR | BLOCK_DOOR_TOP)
}

/// Which side of its cell a wild hive is stuck to, as quarter turns from
/// north -- in the two bits the wood field owns, which a hive has no use for.
///
/// **A hive hung in the middle of its cell looked like a box floating beside
/// the tree** ("пусть улей будет нормально крепится к стволу"). Its own
/// variant field is full: it holds how much honey is in the comb
/// (`bees::hive_holding`), which is what a raid reads. So the side goes in
/// the spare bits, and the drawing and the collider both take the comb to
/// that wall (`geometry::block_box`, `mesh::hive_block`).
pub const HIVE_SIDE_SHIFT: u32 = WOOD_LOW_SHIFT;

/// The comb stuck to the wall in this direction.
#[inline]
pub fn hive_against(id: BlockId, facing: Facing) -> BlockId {
    if !crate::bees::is_hive(id) {
        return id;
    }
    (id & !(0b11 << HIVE_SIDE_SHIFT)) | ((facing as BlockId) << HIVE_SIDE_SHIFT)
}

/// Which way a hive's comb faces: the trunk it grew on is behind it.
#[inline]
pub fn hive_side(id: BlockId) -> Facing {
    match (id >> HIVE_SIDE_SHIFT) & 0b11 {
        1 => Facing::East,
        2 => Facing::South,
        3 => Facing::West,
        _ => Facing::North,
    }
}

/// Does this block carry a wood in its id at all (`WOOD_MASK`)?
///
/// The furniture, and **what falls off a tree**: a fistful of leaves and the
/// litter they make on the ground. A birch wood's floor is pale and a fir's
/// is rust-coloured needles, and one picture for all six was a forest where
/// every floor was an oak's. The same three bits, for the same reason they
/// exist: six woods do not fit in a variant field that already holds a
/// facing, and six ids apiece would be twelve rows and twelve names for two
/// things.
#[inline]
pub fn carries_wood(id: BlockId) -> bool {
    is_wooden_furniture(id)
        || matches!(
            block_kind(id),
            BLOCK_LEAF_HANDFUL | BLOCK_LEAF_LITTER | BLOCK_PROP | BLOCK_LOG_PILE
        )
}

/// The index into `wood::WOODS` a piece of furniture is made of: 0, the oak,
/// for anything that is not furniture and for every piece made before woods.
#[inline]
pub fn furniture_wood(id: BlockId) -> usize {
    if !carries_wood(id) {
        return 0;
    }
    let low = (id >> WOOD_LOW_SHIFT) & 0b11;
    let high = BlockId::from(id & WOOD_HIGH_BIT != 0);
    usize::from(low | (high << 2))
}

/// The same piece in another wood. Anything that is not furniture, or a wood
/// past the table, comes back as it was.
#[inline]
pub fn in_wood(id: BlockId, wood: usize) -> BlockId {
    if !carries_wood(id) || wood >= crate::wood::WOODS.len() {
        return id;
    }
    let wood = wood as BlockId;
    let high = if wood & 0b100 != 0 { WOOD_HIGH_BIT } else { 0 };
    (id & !WOOD_MASK) | ((wood & 0b11) << WOOD_LOW_SHIFT) | high
}

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

/// Which way a block that has a front is pointing.
///
/// **Four values in the same three bits an axis uses.** A log needs to
/// know which way it *lies* and a kiln needs to know which way it
/// *looks*, and no block needs both: one is timber and the other is a
/// thing with a mouth in it. So they share the variant field, and which
/// question a given id is answering is a fact about its kind (see
/// `blocks::BlockDef::faces` and `is_orientable`).
///
/// `North` is zero, so every id ever written before this existed reads
/// as facing north -- which is what a block with one texture on all four
/// sides looked like anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Facing {
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

impl Facing {
    /// Which way somebody looking along `yaw` is facing.
    ///
    /// The *opposite* of where they are looking, because a block is
    /// placed with its front toward the player who put it down: you
    /// build a kiln to work at it, and a kiln whose mouth faced away
    /// would be a kiln you had to walk round.
    pub fn toward_viewer(yaw: f32) -> Facing {
        // Yaw zero looks along +x (see the animals' convention), and the
        // quarters run anticlockwise from there.
        let turns = (yaw / std::f32::consts::FRAC_PI_2).round() as i32;
        match turns.rem_euclid(4) {
            0 => Facing::West,
            1 => Facing::North,
            2 => Facing::East,
            _ => Facing::South,
        }
    }

    /// How many quarter turns anticlockwise from north.
    pub fn quarters(self) -> u32 {
        self as u32
    }

    /// The step, in whole cells on (x, z), that a block facing this way
    /// looks along.
    ///
    /// Read off `toward_viewer` rather than chosen: a player looking along
    /// +z (a yaw of a quarter turn, see `Camera::forward`) puts down a block
    /// facing North, whose front is toward them -- toward -z. So North is
    /// -z, and the rest go round from there. A test holds the two together.
    pub fn step(self) -> (i32, i32) {
        match self {
            Facing::North => (0, -1),
            Facing::East => (1, 0),
            Facing::South => (0, 1),
            Facing::West => (-1, 0),
        }
    }
}

/// Which way this block is facing. `North` for anything that has no
/// front, which is nearly everything.
#[inline]
pub fn block_facing(id: BlockId) -> Facing {
    if !has_front(id) {
        return Facing::North;
    }
    match (id & ORIENTATION_MASK) >> ORIENTATION_SHIFT {
        1 => Facing::East,
        2 => Facing::South,
        3 => Facing::West,
        _ => Facing::North,
    }
}

/// The same block, turned to face somewhere.
///
/// Only the two facing bits are written, and a block with no front comes
/// back exactly as it went in. Both used to be the bare kind, which wiped
/// the third bit -- a rack's skin, a bed's head -- and, for a block that
/// does not turn, everything its variant field held: see `oriented`.
#[inline]
pub fn faced(id: BlockId, facing: Facing) -> BlockId {
    if !has_front(id) {
        return id;
    }
    (id & !ORIENTATION_MASK) | ((facing as BlockId) << ORIENTATION_SHIFT)
}

/// The bit that says a drying rack has a skin stretched on it -- **on a
/// rack of one cell**, which is what every rack was before it was two by
/// two (see [`RACK_TOP`], the same bit, and [`rack_whole`]). A lone cell a
/// save left behind still reads it this way and still dries on it.
///
/// **The spare bit of the variant field.** A rack has a front, so it
/// spends the low two bits of that field on which way it looks (see
/// [`Facing`]); the third bit is not used by anything that faces, and a
/// rack is the one block whose *contents* change what it looks like.
///
/// The alternative was a second block id -- `drying_rack_full` -- with
/// its own row in the table, its own name, its own drop rule and its own
/// place in every match that mentions racks. This is one bit, and
/// [`block_kind`] strips it like every other variant, so a loaded rack
/// is a rack everywhere except in the mesher.
pub const RACK_LOADED: BlockId = 0b100 << VARIANT_SHIFT;

/// Is there a skin on this rack?
#[inline]
pub fn rack_is_loaded(id: BlockId) -> bool {
    carries_a_skin(id) && id & RACK_LOADED != 0
}

/// The bit that says the skin pegged out on a hide frame has dried into
/// leather and is waiting to be taken up. Only ever set with `RACK_LOADED`.
///
/// **What a frame is looked at to find out.** A skin used to show on the
/// frame while it was raw and vanish the moment it cured -- the leather went
/// into the tray and the frame was drawn bare -- so a tanner walking back to
/// the camp saw an empty frame exactly when there was something to collect,
/// and a frame with a skin on it exactly when there was nothing to do. Now
/// the skin stays pegged out until it is taken, in its cured colour.
///
/// **A wood bit, because a hide frame has no wood**: bit ten, the one a rack
/// of two by two spends on its goods (`RACK_GOODS_SHIFT`). Rejected: the
/// goods bits themselves -- a lone frame is never whole, so the two could
/// not collide -- because `rack_goods` answers for the rack of two by two
/// only, and a shared field read two ways by one function is how a hide
/// frame starts hanging meat.
pub const HIDE_CURED: BlockId = 1 << WOOD_LOW_SHIFT;

/// Has the skin on this hide frame dried?
#[inline]
pub fn hide_is_cured(id: BlockId) -> bool {
    block_kind(id) == BLOCK_HIDE_FRAME && rack_is_loaded(id) && id & HIDE_CURED != 0
}

/// The same hide frame showing a raw skin, a cured one, or none; any other
/// block is only given or relieved of its skin (`rack_with_hide`), since a
/// skin that has cured shows on nothing but a hide frame.
#[inline]
pub fn hide_frame_showing(id: BlockId, raw: bool, cured: bool) -> BlockId {
    if block_kind(id) != BLOCK_HIDE_FRAME {
        return rack_with_hide(id, raw);
    }
    let bare = rack_with_hide(id, raw || cured) & !HIDE_CURED;
    if cured && !raw {
        bare | HIDE_CURED
    } else {
        bare
    }
}

/// Is this a frame whose third variant bit is a skin laced in it: the hide
/// frame, or a lone rack cell a save has not been read past yet?
///
/// Both, because the bit means the same on both and the frame *is* the old
/// lone rack (`BLOCK_HIDE_FRAME`); a cell of a rack of two by two reads the
/// same bit as `RACK_TOP` and draws what hangs from its own goods instead.
#[inline]
fn carries_a_skin(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_DRYING_RACK | BLOCK_HIDE_FRAME)
}

/// The bit that says a cell of bed is its head half.
///
/// **A bed is two cells**, because a body is: a bed one cell long left a
/// sleeper's head and feet hanging over both ends, and it was drawn as a
/// cube of boards with a picture of hide on top. Which of the two cells a
/// cell is has to be *in* the cell -- the mesher draws the pillow on one
/// and the footboard on the other, and whoever breaks either has to take
/// both -- so it is the spare bit of the variant field, the one a rack
/// spends on its skin (`RACK_LOADED`). A bed has a front, so its low two
/// bits already say which way it lies.
///
/// Rejected: two ids, `bed_head` and `bed_foot`. It is the alternative
/// `RACK_LOADED` already argues against -- two rows, two names, two drops,
/// and an arm in every match that asks "is this a bed" waiting to forget
/// one of them.
pub const BED_HEAD: BlockId = 0b100 << VARIANT_SHIFT;

/// Is this something two cells long that a body lies across: a bed or a
/// straw pallet.
///
/// **The straw is two cells as well now**, for the bed's reason: a body
/// is. It used to be one cell, on the argument that a heap of straw has no
/// head end and a first shelter is often three cells wide -- and what the
/// player saw was a sleeper's head and feet hanging over both ends of a
/// square of grass. Both kinds spend the spare variant bit on which half a
/// cell is (`BED_HEAD`), so everything that asks "where is the other half"
/// asks this and never names a kind.
#[inline]
pub fn is_bed(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_BED | BLOCK_STRAW_BED)
        // ...and the two cells of a lean-to a body lies across, its mouth
        // and its middle: the other thirteen are its walls and its roof
        // (`lean_to::PARTS`), and a wall that answered "bed" would be paired
        // with a cell beside it that is nobody's head.
        || crate::lean_to::is_bed_part(id)
}

/// One half of a plank bed lying `facing`. See [`bed_half_of`].
#[inline]
pub fn bed_half(facing: Facing, head: bool) -> BlockId {
    bed_half_of(BLOCK_BED, facing, head)
}

/// One half of a bed of this kind -- a plank bed or a straw pallet --
/// lying `facing`. The foot is toward whoever put it down and the head is
/// the cell behind it, `facing.step()` further away.
#[inline]
pub fn bed_half_of(kind: BlockId, facing: Facing, head: bool) -> BlockId {
    faced(block_kind(kind), facing) | if head { BED_HEAD } else { 0 }
}

/// Is this the head half of a bed or a pallet?
#[inline]
pub fn is_bed_head(id: BlockId) -> bool {
    is_bed(id) && id & BED_HEAD != 0
}

/// Where the other half of this bed is, and exactly what must be standing
/// there for the two to be one bed. `None` for anything that is not a bed.
///
/// One function for every party that has to agree about it: the server
/// placing and breaking, the client refusing a placement it can already
/// see will fail, and the player's body, which lies across the seam.
#[inline]
pub fn bed_partner(at: (i32, i32, i32), id: BlockId) -> Option<((i32, i32, i32), BlockId)> {
    if !is_bed(id) {
        return None;
    }
    // The partner is the same *kind*: a straw foot against a plank head is
    // two halves of two different beds, and neither is whole.
    let kind = block_kind(id);
    let facing = block_facing(id);
    let (dx, dz) = facing.step();
    // ...and the same wood: a fir head against an oak foot is two beds.
    let wood = furniture_wood(id);
    Some(if is_bed_head(id) {
        ((at.0 + dx, at.1, at.2 + dz), in_wood(bed_half_of(kind, facing, false), wood))
    } else {
        ((at.0 - dx, at.1, at.2 - dz), in_wood(bed_half_of(kind, facing, true), wood))
    })
}

/// The bit that says a door is open. The spare bit of the variant field,
/// the one a bed spends on its head and a rack on its skin; a door's low
/// two bits say which way it was hung.
pub const DOOR_OPEN: BlockId = 0b100 << VARIANT_SHIFT;

/// Is this either half of a door?
#[inline]
pub fn is_door(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_DOOR | BLOCK_DOOR_TOP)
}

/// Is this a door standing open, either half?
#[inline]
pub fn door_is_open(id: BlockId) -> bool {
    is_door(id) && id & DOOR_OPEN != 0
}

/// The same half of the same door, swung the other way: open if it was
/// shut and shut if it was open. Anything that is not a door comes back as
/// it was.
#[inline]
pub fn door_swung(id: BlockId) -> BlockId {
    if !is_door(id) {
        return id;
    }
    id ^ DOOR_OPEN
}

/// Where the other half of this door is, and exactly what must stand there
/// for the two to be one door: the same facing and the same open or shut.
/// `None` for anything that is not a door.
///
/// `bed_partner`'s contract, for a door -- one function for the placement,
/// every break path and the swing, so a half that is not its partner's
/// match is never swung or taken with it.
#[inline]
pub fn door_partner(at: (i32, i32, i32), id: BlockId) -> Option<((i32, i32, i32), BlockId)> {
    // The wood with the state: a fir door's top half is fir.
    let state = id & (VARIANT_MASK | WOOD_MASK);
    match block_kind(id) {
        BLOCK_DOOR => Some(((at.0, at.1 + 1, at.2), BLOCK_DOOR_TOP | state)),
        BLOCK_DOOR_TOP => Some(((at.0, at.1 - 1, at.2), BLOCK_DOOR | state)),
        _ => None,
    }
}

/// How many quarter turns from north a door's slab stands at: its facing,
/// and one more when it is open.
///
/// **Shut, the slab is across the back of the cell** -- the face away from
/// whoever hung it -- and **open, it is one quarter turn further round**,
/// along a side. Turning the same slab is what makes the hinge a hinge: the
/// back face and the side face one quarter on share exactly one corner
/// column of the cell, so the door swings about that corner and the two
/// positions never stand apart. The box turns by these quarters in
/// `geometry::block_box` and the model by the same in `mesh::door_block`,
/// and a test there holds the two together.
///
/// Rejected: *a hinge side chosen by the placer*, a door hung left or
/// right. It is a fourth bit the field has not got, and in a doorway a
/// cell wide it decides nothing: the door is open or it is not.
#[inline]
pub fn door_quarters(id: BlockId) -> u32 {
    let facing = block_facing(id).quarters();
    if door_is_open(id) {
        (facing + 1) % 4
    } else {
        facing
    }
}

/// Is this something a player sits on: a stool or a chair.
///
/// **One question for the three parties that have to agree on it** -- the
/// server deciding a click sits somebody down, the server's tick deciding
/// the seat under a sitter is still there, and the client deciding a right
/// click is a rest rather than a placement. The stool was spelled out at
/// all three, and a chair added at two of them would be a chair that sits
/// you down and then, a tick later, stands you up again because the seat
/// "was broken".
#[inline]
pub fn is_seat(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_STOOL | BLOCK_CHAIR)
}

/// Which way somebody sitting on this faces, as a yaw in the
/// `Camera::forward` convention, or `None` for a seat with no front.
///
/// A chair is sat in facing the way it faces -- out over its seat, away
/// from its back -- which is `Facing::step` turned into an angle. A stool
/// turns too, but only so its odd leg is where its placer left it: it has
/// no side a sitter faces, so a sitter keeps looking wherever they were.
#[inline]
pub fn seat_yaw(id: BlockId) -> Option<f32> {
    if block_kind(id) != BLOCK_CHAIR {
        return None;
    }
    let (dx, dz) = block_facing(id).step();
    Some((dz as f32).atan2(dx as f32))
}

/// The same rack, with a skin on it or without one.
///
/// Keeps whichever way it was facing: the two facts live in one field
/// and setting one must not lose the other.
#[inline]
pub fn rack_with_hide(id: BlockId, loaded: bool) -> BlockId {
    if !carries_a_skin(id) {
        return id;
    }
    if loaded {
        id | RACK_LOADED
    } else {
        id & !RACK_LOADED
    }
}

// ---- the rack of two by two ----
//
// **Two cells along the ridge and two cells high, not two by two on the
// floor.** The player's picture is an A of crossed poles at each end and a
// ridge pole between the crotches: a frame that is long and tall and a pole
// thick. Two cells deep on the floor as well would be eight cells of which
// four are air under a ridge nobody can reach, and a rack a tanner cannot
// walk up to from either side. Along the ridge is where the goods hang, so
// along the ridge is where the length is.
//
// **Every cell says which of the four it is, in its own id.** The bits a
// rack has are the six above the kind: the facing takes two, and
// `RACK_TOP` and `RACK_FAR` say which cell -- so any cell broken, opened or
// meshed knows where the other three are without asking anything, the way
// a bed's half knows its partner (`bed_partner`). A rack is not wooden
// furniture, so bits ten, eleven and fifteen are free on it
// (`WOOD_MASK`), and fifteen is `RACK_FAR`.
//
// **What hangs is two bits a cell, four a column** (`RACK_GOODS_MASK`):
// each column shows one kind of goods from `rack::HANGING`, its low two
// bits on the bottom cell and its high two on the top. Rejected: a
// server-to-client message with the rack's contents, the way a thing set
// down is sent (`ServerMessage::SetDownItem`). It is a second channel that
// has to be sent on join, on chunk load, on every change and forgotten on
// unload, for a picture that fits in the chunk the client already has --
// and a chunk is what a remesh is triggered by anyway. The cost of the
// bits is that a column shows one *kind*, not a count; see `rack::HANGING`.

/// The bit that says a cell of a two-by-two rack is in its upper row. The
/// same bit as [`RACK_LOADED`]: a lone cell with it set is an old loaded
/// rack, and [`rack_whole`] is what tells the two apart.
pub const RACK_TOP: BlockId = 0b100 << VARIANT_SHIFT;
/// The bit that says a cell of a rack is in the column away from the one
/// it was put down in, along the ridge ([`rack_far_step`]). Bit fifteen,
/// the high bit of a furniture's wood, which a rack never has.
pub const RACK_FAR: BlockId = WOOD_HIGH_BIT;
/// Where a rack cell's two bits of what hangs start.
pub const RACK_GOODS_SHIFT: u32 = WOOD_LOW_SHIFT;
/// A rack cell's two bits of what hangs on its column.
pub const RACK_GOODS_MASK: BlockId = 0b11 << RACK_GOODS_SHIFT;

/// Is this a cell of a rack in its upper row?
#[inline]
pub fn rack_is_top(id: BlockId) -> bool {
    block_kind(id) == BLOCK_DRYING_RACK && id & RACK_TOP != 0
}

/// Is this a cell of a rack in its far column?
#[inline]
pub fn rack_is_far(id: BlockId) -> bool {
    block_kind(id) == BLOCK_DRYING_RACK && id & RACK_FAR != 0
}

/// The step along the ridge, from the column a rack was put down in to the
/// other one.
///
/// **The model's +x turned the way the model is**: the frame is written at
/// its north with the ridge along x (`mesh::rack_column_block`), and
/// `push_box` turns it anticlockwise by `turned_from_north`. Read off that
/// turn rather than off `Facing::step`, and held to the drawing by
/// `a_two_by_two_rack_is_drawn_in_the_cells_it_occupies` in the client.
#[inline]
pub fn rack_far_step(facing: Facing) -> (i32, i32) {
    match facing {
        Facing::North => (1, 0),
        Facing::East => (0, 1),
        Facing::South => (-1, 0),
        Facing::West => (0, -1),
    }
}

/// One cell of a rack lying `facing`, with nothing hanging on it.
#[inline]
pub fn rack_cell(facing: Facing, far: bool, top: bool) -> BlockId {
    faced(BLOCK_DRYING_RACK, facing) | if far { RACK_FAR } else { 0 } | if top { RACK_TOP } else { 0 }
}

/// What a rack cell is with its goods left out: what a partner has to be
/// for the four to be one rack, whatever hangs on them.
#[inline]
pub fn rack_shape(id: BlockId) -> BlockId {
    id & !RACK_GOODS_MASK
}

/// The cell a rack was put down in -- its near bottom -- worked out from
/// any of its cells by its own bits. That cell keys its contents and its
/// drying (`rack::home`).
#[inline]
pub fn rack_anchor(at: (i32, i32, i32), id: BlockId) -> (i32, i32, i32) {
    let (dx, dz) = if rack_is_far(id) { rack_far_step(block_facing(id)) } else { (0, 0) };
    let dy = i32::from(rack_is_top(id));
    (at.0 - dx, at.1 - dy, at.2 - dz)
}

/// All four cells of a rack whose near bottom is at `anchor`, with nothing
/// hanging: near bottom, near top, far bottom, far top.
pub fn rack_cells(anchor: (i32, i32, i32), facing: Facing) -> [((i32, i32, i32), BlockId); 4] {
    let (dx, dz) = rack_far_step(facing);
    let (x, y, z) = anchor;
    [
        ((x, y, z), rack_cell(facing, false, false)),
        ((x, y + 1, z), rack_cell(facing, false, true)),
        ((x + dx, y, z + dz), rack_cell(facing, true, false)),
        ((x + dx, y + 1, z + dz), rack_cell(facing, true, true)),
    ]
}

/// The other three cells of the rack this cell belongs to, and what shape
/// each must be (goods left out, see [`rack_shape`]). Empty for anything
/// that is not a rack.
pub fn rack_partners(at: (i32, i32, i32), id: BlockId) -> Vec<((i32, i32, i32), BlockId)> {
    if block_kind(id) != BLOCK_DRYING_RACK {
        return Vec::new();
    }
    rack_cells(rack_anchor(at, id), block_facing(id))
        .into_iter()
        .filter(|&(cell, _)| cell != at)
        .collect()
}

/// Is the rack cell at `at` one of four that stand where its bits say?
///
/// **This is what tells a two-by-two rack from an old one-cell rack**, and
/// it has to ask the world: an empty rack of one cell has the bits of a
/// near bottom, and a loaded one the bits of a near top (`RACK_LOADED` is
/// `RACK_TOP`). Neither has three partners, so both read as lone and keep
/// behaving as the rack they were -- one cell, drawn as it was, with its
/// contents in its own cell. That is the migration: nothing is rewritten,
/// and a save from before opens with its racks exactly as they stood.
///
/// Rejected: growing every old rack into four cells when its chunk loads.
/// A rack in a hut stands against walls and under a roof, and there is no
/// good answer for one with no room -- delete it, spill it, leave it half
/// grown -- while a lone cell that keeps working needs no answer at all.
///
/// `block_at` answers `None` for a cell nobody has; a rack whose partner is
/// in such a cell is not whole, which is the cautious answer.
pub fn rack_whole(at: (i32, i32, i32), id: BlockId, block_at: impl Fn((i32, i32, i32)) -> Option<BlockId>) -> bool {
    if block_kind(id) != BLOCK_DRYING_RACK {
        return false;
    }
    rack_partners(at, id)
        .into_iter()
        .all(|(cell, shape)| block_at(cell).is_some_and(|there| rack_shape(there) == shape))
}

/// A rack cell's two bits of what hangs on its column.
#[inline]
pub fn rack_goods(id: BlockId) -> u8 {
    if block_kind(id) != BLOCK_DRYING_RACK {
        return 0;
    }
    ((id & RACK_GOODS_MASK) >> RACK_GOODS_SHIFT) as u8
}

/// The same rack cell with these two bits of goods.
#[inline]
pub fn with_rack_goods(id: BlockId, bits: u8) -> BlockId {
    if block_kind(id) != BLOCK_DRYING_RACK {
        return id;
    }
    rack_shape(id) | ((BlockId::from(bits) & 0b11) << RACK_GOODS_SHIFT)
}

/// What hangs on a column, from its bottom cell and its top cell: an index
/// into `rack::HANGING`.
#[inline]
pub fn rack_column_goods(bottom: BlockId, top: BlockId) -> u8 {
    rack_goods(bottom) | (rack_goods(top) << 2)
}

/// Does this block look different from one side than from another?
///
/// The kiln and the bloomery have a mouth, the chest a lid catch, a chair
/// a back, a jug a handle, a three-legged stool its odd leg. What decides
/// it is the look and nothing else: a block turns when a quarter turn of
/// it would show, and not otherwise. That is read off the table row, and
/// the row is held to the pictures and the models by the client's
/// `a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show`.
#[inline]
pub fn has_front(id: BlockId) -> bool {
    crate::blocks::definition(id).faces
}

/// Puts an axis on a block, or leaves it alone if it has no use for one.
///
/// **Alone means alone.** This used to return the bare kind for anything
/// without an axis, which quietly stripped whatever else the variant
/// field held -- a barrel's water, a jug's river -- from every id that went
/// through the placement path. Turning a block must never change what
/// is in it, so only the orientation bits are written.
#[inline]
pub fn oriented(id: BlockId, axis: Axis) -> BlockId {
    if !is_orientable(id) {
        return id;
    }
    (id & !ORIENTATION_MASK) | ((axis as BlockId) << ORIENTATION_SHIFT)
}

/// **Is `held` what a placement of `placing` is made of** -- the question
/// the server asks of the slot before it takes one out of it.
///
/// The same kind, whichever way it was laid; the same wood, for a thing that
/// is made of one (`carries_wood`); and **wet exactly when the hand's is**
/// (`wet`, "Put down wet"), because the wet bit is the one part of a stack a
/// placement keeps and so the one a client could invent or shed. What
/// [`placed`] takes off -- the lie, the facing, a log's seasoning -- is not
/// asked. It used to be `held == block_kind(placing)`, which is the same
/// answer for a dry seasoned log and refused a green one: the log a player
/// has just felled would not go into a wall.
pub fn spends(held: BlockId, placing: BlockId) -> bool {
    held != 0
        && block_kind(held) == block_kind(placing)
        && crate::wet::is_wet(held) == crate::wet::is_wet(placing)
        && (!carries_wood(placing) || furniture_wood(held) == furniture_wood(placing))
}

/// What a player's block becomes when it is put down: the one rule for
/// which way it lies and which way it looks.
///
/// `yaw` is where the placer is looking (`Camera::forward`'s convention)
/// and `clicked` the normal of the face they built against, from the
/// block they aimed at toward the cell the new one goes in.
///
/// Three kinds of block, three rules, and each is what the thing is for:
///
/// * **A length** -- a log, a stripped trunk -- lies along the clicked
///   face's axis. Click the top and it stands; click a side and it lies
///   pointing at you. It is how everyone already builds with timber.
/// * **A front** -- a kiln's mouth, a chest's catch, a chair's seat, a
///   jug's handle -- faces the placer. You build a kiln to work at it, and
///   one whose mouth faced away would be one you had to walk round.
/// * **Something held up from the side** -- a bracket fungus -- faces
///   out of the face that was clicked, whichever way the placer stands.
///   Its front is decided by the trunk it grows from, and "toward the
///   placer" put an east- or west-clicked shelf with its root in the air
///   on the placer's side, which `support_at` then refused: a bracket
///   could only be put on two of a trunk's four sides.
///
/// **Rejected: the server working the facing out for itself.** It has
/// the player's yaw, but only as of their last movement packet, and a
/// player who turns and clicks in the same breath would get a chest
/// facing where they were looking a tick ago. Every facing is a legal
/// block (`is_known_block`), so there is nothing for the server to
/// protect by refusing one: it checks the id, the support and the space,
/// exactly as it does for a block that does not turn.
pub fn placed(held: BlockId, yaw: f32, clicked: (i32, i32, i32)) -> BlockId {
    // A log in the world carries no seasoning (`wood`, "seasoning") -- but
    // **a wet thing put down is still wet** (`wet`, "Put down wet"): the
    // sixteenth bit stays, and dries in the weather where it stands. It used
    // to come off here, and a wet log laid in a wall and cut out again was a
    // dry one: a swim, a wall and an axe were a way to dry kindling in two
    // seconds. The bit is free on every kind that gets wet, in the world as
    // in the pack; on every other kind `wet` leaves it alone.
    let held = crate::wood::seasoned(held);
    let laid = match Axis::of_normal(clicked.0, clicked.1, clicked.2) {
        Some(axis) => oriented(held, axis),
        None => held,
    };
    if !has_front(laid) {
        return laid;
    }
    // Held from the side: the facing whose support is the block that
    // was clicked. A click on a top or a bottom has no side to hang
    // from, and falls through to the placer's facing -- which the
    // support rule then judges like any other placement.
    let (sx, sy, sz) = support_at(faced(laid, Facing::North));
    if sy == 0 && sx | sz != 0 && clicked.1 == 0 {
        let wall = (-clicked.0, 0, -clicked.2);
        let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
        if let Some(facing) = facings.into_iter().find(|&f| support_at(faced(laid, f)) == wall) {
            return faced(laid, facing);
        }
    }
    // **A stake stood on a floor stands up**; driven into a wall it is the
    // side placement above. A click on a top or a bottom has no wall to be
    // driven into, and a stake lying in the air held by nothing is the one
    // thing this block must not be.
    if is_stake(laid) && clicked.1 != 0 {
        return laid | STAKE_UPRIGHT;
    }
    // A prop set on a floor or a ceiling stands in the middle of its cell;
    // one set against a wall leans on that wall (`BLOCK_PROP`).
    if is_prop(laid) && clicked.1 != 0 {
        return laid | PROP_CENTRED;
    }
    // ...and one set against a wall leans on *that* wall -- the block that
    // was clicked -- rather than turning toward the viewer, which put it
    // against whichever wall was behind the player's shoulder.
    if is_prop(laid) {
        let wall = (-clicked.0, 0, -clicked.2);
        let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
        if let Some(facing) = facings.into_iter().find(|&f| wall_behind(faced(laid, f)) == wall) {
            return faced(laid, facing);
        }
        return laid | PROP_CENTRED;
    }
    faced(laid, Facing::toward_viewer(yaw))
}

/// Whether this kind of block is drawn differently depending on which
/// way it lies.
///
/// A length of something: two ends that are the same picture and sides
/// that are another, which is a log and a birch log -- or a block whose
/// *breaking* depends on the axis, which is the stripped trunk (see
/// `BlockDef::felled`), drawn in rings all over and still deadfall only
/// when it lies down. A cube of stone rotated is a cube of stone, and
/// giving it an orientation would mean two ids for one thing -- which
/// costs an inventory slot the moment two of them meet.
///
/// A turf or a campfire has two *different* ends and is not on this
/// list: laid on its side it is a block upside down, not a block lying
/// down. The client's
/// `a_block_turns_when_it_is_placed_exactly_when_turning_it_would_show`
/// reads the pictures and holds this to them both ways.
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
    // ...and not drowned wood, which is liquid by its row and spends the
    // field on its width (`BLOCK_DROWNED_BOUGH`). Read as a depth, a bough
    // ten sixteenths wide was a cell of water one eighth full.
    is_liquid(id) && !is_branch(id)
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
///
/// A carcass is on the list because its variant is the *stage of
/// butchering* (see `animals::butcher`), and the server writes that
/// stage into the world after every cut. Left off this list, the first
/// cut would produce a block `is_known_block` calls invented: a mod
/// reading the cell would be told it does not exist, and a save full of
/// half-butchered deer would be a save full of junk.
///
/// Food that goes off is on it for the same reason from the other side
/// of the wire: its variant is *how old it is* (see `food::rot_stage`),
/// and the server writes that into every stack in every pack and chest
/// four times a day. Left off, the first step of the clock would turn a
/// player's dinner into an id the anti-cheat refuses -- at three points
/// of a twelve-point kick per stack the client so much as mentions --
/// and a profile full of yesterday's meat would not load.
#[inline]
fn may_carry_variant(kind: BlockId) -> bool {
    // ...and a full jug, whose variant is **what is in it**: a river, a
    // pond or the sea (see `vessel_water`). Without this the first jug
    // filled from a pond would be an id the anti-cheat calls invented,
    // and a pack with one in it would not load.
    kind == BLOCK_JUG_WATER
        // ...and a weapon, whose variant is whether there is fly agaric
        // on it (see `POISONED`). Without this the first poisoned spear
        // would be an id the anti-cheat calls invented, and the pack
        // holding it would not load.
        || is_weapon(kind)
        || is_loose(kind)
        || is_liquid(kind)
        || is_carcass(kind)
        // ...and a dead player's body, whose variant is **how many cuts have
        // been taken off it** (`animals::cut_body`).
        || kind == BLOCK_CORPSE
        // ...and a skeleton, whose variant is *which animal* it was
        // (see `bones_of`). Left off, the first deer to rot would leave
        // an id the anti-cheat calls invented, in the middle of a
        // meadow, for ever. Both skeleton ids: see `BLOCK_BONES_2`.
        || is_bones(kind)
        // ...and a barrel, whose variant is *how many jugs are in it*
        // (see `barrel_of`). Left off, the first jug poured into one
        // would write an id the anti-cheat calls invented into the world.
        || is_barrel(kind)
        // ...and a haystack, whose variant is **how many bites a flock has
        // taken out of it** (`hay_in_stack`). Left off, the first bite
        // would write an id the anti-cheat calls invented into the pen.
        || kind == BLOCK_HAYSTACK
        || crate::food::is_perishable(kind)
        // ...and every stage of a pit kiln and a pile of logs, whose
        // variant is a count (see `pit::Stage`). Left off, the first fibre
        // laid in a pit would be an id the anti-cheat calls invented.
        || crate::pit::carries_variant(kind)
        // ...and boards and stone over a hearth, whose variant is **how
        // sooted** they are (`wildfire::soot`). Left off, the first ceiling
        // to blacken would be an id the anti-cheat calls invented, in the
        // roof of somebody's house.
        || crate::wildfire::may_carry_soot(kind)
        // ...and tilled earth, whose variant is **whether ash has been dug
        // into it** (`wildfire::dressed`).
        || crate::wildfire::may_carry_dressing(kind)
        // ...and a fish trap, whose variant is **how many fish are in it**
        // (`trap_catch`). Left off, the first fish to swim in would write an
        // id the anti-cheat calls invented into somebody's river.
        || kind == BLOCK_FISH_TRAP
        // ...and a wild hive, whose variant is **how much honey is in it**
        // (`bees::honey_in`). `is_known_block` holds it to what a hive holds
        // before this is asked; this is the list of what carries one at all.
        || kind == BLOCK_WILD_HIVE
        // ...and the three counts of the larder and the trapline: how far a
        // young cheese or a must has worked (`ferment`), how long a hare has
        // hung in a snare (`snare`), how far a pan of the sea has dried
        // (`saltpan`). `is_known_block` holds each to what it counts.
        || crate::ferment::is_working(kind)
        || kind == BLOCK_SNARE_CAUGHT
        || kind == BLOCK_SALT_PAN_BRINE
}

/// How many fish are in this trap: nought for an empty one, and for
/// anything that is not a trap at all.
#[inline]
pub fn trap_catch(id: BlockId) -> u8 {
    if block_kind(id) != BLOCK_FISH_TRAP {
        return 0;
    }
    ((id & VARIANT_MASK) >> VARIANT_SHIFT) as u8
}

/// A trap holding `fish`, clamped to what one holds (`fishing::TRAP_HOLDS`):
/// a count past it would be a fourth fish in a basket that has no room for
/// one, and a variant `is_known_block` has no reason to believe.
#[inline]
pub fn trap_holding(fish: u8) -> BlockId {
    BLOCK_FISH_TRAP | ((fish.min(crate::fishing::TRAP_HOLDS) as BlockId) << VARIANT_SHIFT)
}

/// A full jug of water of this kind.
///
/// **Two bits of the variant field**, which a jug has spare: the block
/// says what it holds, so a jug of pond water is a different item from
/// a jug of river water in the pack, on the wire and in a save, with no
/// new id and no new field anywhere. See `may_carry_variant`.
#[inline]
pub fn jug_of(kind: crate::body::Water) -> BlockId {
    let code: BlockId = match kind {
        crate::body::Water::Fresh => 0,
        crate::body::Water::Standing => 1,
        crate::body::Water::Salt => 2,
    };
    BLOCK_JUG_WATER | (code << VARIANT_SHIFT)
}

/// What is in this jug. `Fresh` for anything that is not a full jug at
/// all, because a caller asking about a stick has asked a question with
/// no answer and the safe one is the one that does no harm.
#[inline]
pub fn vessel_water(block: BlockId) -> crate::body::Water {
    if block_kind(block) != BLOCK_JUG_WATER {
        return crate::body::Water::Fresh;
    }
    match (block & VARIANT_MASK) >> VARIANT_SHIFT {
        1 => crate::body::Water::Standing,
        2 => crate::body::Water::Salt,
        _ => crate::body::Water::Fresh,
    }
}

/// What a player is told is in a full jug or a barrel: the water's purity,
/// or `None` for anything holding no water.
///
/// **The one fact about a vessel that decides whether to drink from it**, and
/// until this it was kept and never shown: a jug of river water and a jug of
/// the sea had the same name in the pack, so the player had to remember which
/// was which -- and a jug set down and broken used to forget as well (see
/// `block_drop`). The name is the block's own; this is what goes after it.
pub fn water_label(id: BlockId) -> Option<&'static str> {
    use crate::body::Water;
    let kind = if block_kind(id) == BLOCK_JUG_WATER {
        vessel_water(id)
    } else {
        match barrel_contents(id)? {
            (_, 0) => return None,
            (kind, _) => kind,
        }
    };
    Some(match kind {
        Water::Fresh => "clean water",
        Water::Standing => "pond water",
        Water::Salt => "sea water",
    })
}

/// Is this cell what is left of an animal?
///
/// The species is the block and the stage of butchering is the variant,
/// so this strips the variant first: a deer with the skin already off is
/// still a deer. See `animals::Species::of_carcass` for *which* animal.
#[inline]
pub fn is_carcass(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_CARCASS_HARE
            | BLOCK_CARCASS_DEER
            | BLOCK_CARCASS_BOAR
            | BLOCK_CARCASS_WOLF
            | BLOCK_CARCASS_SHEEP
            | BLOCK_CARCASS_BEAR
            | BLOCK_CARCASS_FOWL
            // **Not optional, and the compiler will not say so.** A carcass
            // missing from here still draws and still butchers (both ask
            // `Species::of_carcass`), and quietly stops being allowed a
            // stage, ageing into bones and drawing a lion to it.
            | BLOCK_CARCASS_ZEBRA
            | BLOCK_CARCASS_ANTELOPE
            | BLOCK_CARCASS_LION
            | BLOCK_CARCASS_HORSE
    )
}

/// A thing made to be swung at an animal and at nothing else.
///
/// The spear opens no block -- it has no tier, so it is not on the
/// ladder `blocks::tests::every_age_is_a_knife_an_axe_and_a_pick`
/// counts -- and what it does is decided by the server's hunting
/// damage, which knows it by name. Kept as a list so that the recipe
/// table's test can see that a spear is *for* something.
#[inline]
pub fn is_weapon(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_FLINT_SPEAR
            | BLOCK_BONE_SPEAR
            | BLOCK_COPPER_SPEAR
            | BLOCK_BRONZE_SPEAR
            | BLOCK_IRON_SPEAR
    )
}

/// What colour to draw a spear's icon.
///
/// The four share one picture (see `BLOCK_BONE_SPEAR`) and are told
/// apart by the colour of the head, on exactly the terms the garments
/// are: the drawing is in values, and a value multiplied by a metal is
/// that metal. `None` for the flint one, which is what the picture was
/// drawn as -- tinting a thing to the colour it already is would be a
/// multiplication that only rounds it.
pub fn spear_tint(id: BlockId) -> Option<[f32; 3]> {
    match block_kind(id) {
        // Bone: pale, faintly warm, and lighter than everything else in
        // the hotbar so the weakest spear is not the one that looks
        // like iron.
        BLOCK_BONE_SPEAR => Some([0.90, 0.86, 0.74]),
        BLOCK_COPPER_SPEAR => Some([0.85, 0.52, 0.32]),
        BLOCK_BRONZE_SPEAR => Some([0.82, 0.62, 0.30]),
        BLOCK_IRON_SPEAR => Some([0.66, 0.70, 0.76]),
        _ => None,
    }
}

/// **A weapon with fly agaric smeared on it**, and what it takes off.
///
/// One bit of the variant field, which a spear has spare. The poison is
/// therefore a property of the *item*: it survives a save, it goes into
/// a chest and comes back out, it is visible in the pack as a colour,
/// and it needed no new id, no new field on a stack and no table
/// anywhere. See `may_carry_variant`, which had to learn about it, and
/// the "poison a spear" row in `crafting`.
///
/// **One thrust and it is gone**, which is the whole of the mechanic:
/// the paste is worth the walk to a toadstool exactly once, so it is
/// something a hunter puts on before going after a bear rather than a
/// permanent upgrade to a spear.
pub const POISONED: BlockId = 0b001 << VARIANT_SHIFT;

/// The same weapon with the paste on it.
#[inline]
pub fn poisoned(id: BlockId) -> BlockId {
    if is_weapon(id) {
        block_kind(id) | POISONED
    } else {
        id
    }
}

/// ...and the same weapon with it used up.
#[inline]
pub fn unpoisoned(id: BlockId) -> BlockId {
    if is_weapon(id) {
        block_kind(id)
    } else {
        id
    }
}

/// Is there paste on this?
#[inline]
pub fn is_poisoned(id: BlockId) -> bool {
    is_weapon(id) && (id & VARIANT_MASK) == POISONED
}

/// Is this the tool a carcass is taken apart with?
///
/// **A list of ids and not a test of `Work::Plant`**, although every
/// knife is `Work::Plant`: so is the hoe, which is a digging stick and
/// not an edge, and "a hoe skins a deer" is exactly the kind of answer
/// that comes out of asking the wrong field. The knives are also the
/// only tools whose tier is spent on the *edge* rather than on the
/// work -- see `lib::hunting_damage` on the server -- so the same list
/// is what makes the hunting tool and the butchering tool one tool.
#[inline]
pub fn is_knife(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_FLINT_KNIFE | BLOCK_COPPER_KNIFE | BLOCK_BRONZE_KNIFE | BLOCK_IRON_KNIFE
    )
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
        // Not in the id, then, but in the table: a campfire and a
        // backpack fill half their cell and everything else fills all of
        // it. See `blocks::BlockDef::thickness`.
        return crate::blocks::definition(id).thickness.clamp(1, LAYERS_PER_BLOCK);
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

/// Anything that does not fill its cell from floor to ceiling.
///
/// Two things reach this, and they arrive by different routes: water,
/// which carries how deep it is in its id, and the handful of blocks
/// the table calls half-height. Everything the *mesher* does about it is
/// the same either way -- draw the cube short, cover only as much of the
/// shared wall as it reaches, do not let anything cull the top face --
/// so it is one question with one answer.
///
/// It is deliberately **not** the question `needs_support` and
/// `can_grow_on` ask. Those are about the layer economy -- a shovelful
/// of earth with nothing under it is a shovelful that fell -- and a
/// campfire is not a shovelful of anything. See `is_loose_layer`.
///
/// **Water is excluded**, even though a cell of it is usually part
/// deep. Its depth is drawn and culled by rules of its own -- two cells
/// of water share no face at all, whatever depths they are, because a
/// wall inside a lake is visible from underneath (see
/// `fluid::surface_height`) -- and letting it answer yes here puts that
/// wall straight back.
#[inline]
pub fn is_partial(id: BlockId) -> bool {
    !is_liquid(id) && block_layers(id) < LAYERS_PER_BLOCK
}

/// A *drift* that does not fill its cell: loose material, part-deep.
///
/// The narrow half of `is_partial`, and the one the placement rules
/// want. Splitting them is what lets a campfire be half a block tall
/// without becoming a thing that falls when you mine under it -- which
/// is right for a spadeful of sand and wrong for a hearth somebody built
/// on a ledge.
#[inline]
pub fn is_loose_layer(id: BlockId) -> bool {
    is_loose(id) && is_partial(id)
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

/// How much of its cell this block actually stands in, across the axis
/// its front faces, when it is not the whole of it.
///
/// `None` for nearly everything: a solid block is its whole cell, and
/// that is what keeps the collider cheap. The exception is a block that
/// is a *frame* rather than a slab.
///
/// **The drying rack is the reason this exists.** Its height is right --
/// the uprights run floor to ceiling of the cell, and the argument for
/// that in `blocks.rs` still stands -- but the frame is only two and a
/// half sixteenths deep, standing in the middle of its cell like a
/// fence panel. Collision was the whole cell on both horizontal axes,
/// so a tanner could not walk past their own rack: a square of poles
/// you can see straight through stopped a player like a wall of stone.
/// Reported as "the rack has the collision of a full block".
///
/// Given as (near, far) in cell units along the model's own depth axis
/// and turned by [`block_facing`] where it is used -- see
/// `geometry::block_box`. Named in the model's axis rather than in
/// world axes because that is what keeps one number true for all four
/// rotations of the same rack.
///
/// Horizontal extent only, and deliberately: height already has an
/// answer in [`collision_height`], and a second answer here is how two
/// numbers that have to agree begin to disagree.
#[inline]
pub fn collision_depth(id: BlockId) -> Option<(f32, f32)> {
    // **The ridge pole and the crossing of the poles under it**
    // (`assets/models/misc/drying_rack.bbmodel`): the ridge is 7.3..8.8 of
    // sixteen and the crossing 6.8..9.2, and the slab is 7..9.5 -- the old
    // frame's, kept on purpose. A player walking along the rack stands
    // against it in the client's physics tests, whose numbers are this pair
    // (`RACK_FRAME`), and half a sixteenth against the pole a player sees is
    // not worth moving a collider every test of that file measures.
    //
    // Not the splayed feet, which stand 2.5..13.5 across: a box that deep is
    // the whole cell to a player, the wall `there_is_room_to_walk_past_a_drying_rack_in_its_own_cell`
    // was written against, stopping a tanner a pace from a frame they can see
    // straight through. What a player walks into is the part at their chest;
    // a foot of a pole brushed past at the corner of the cell is the price, and
    // the outline (`geometry::block_box_for_aim`) holds all of it.
    if block_kind(id) == BLOCK_DRYING_RACK {
        return Some((7.0 / 16.0, 9.5 / 16.0));
    }
    // **The hide frame is its poles and their lashings**, 6.75..9.25 of
    // sixteen (`misc/hide_frame.bbmodel`): the top cross pole lashed in front
    // of the uprights and the bottom one behind, and the lashings round both.
    // All of it, and not only the skin at 7.75..8.25, because a pole a body
    // walks through reads as a hole in the world; the client holds the box to
    // the drawing (`a_laced_hide_is_drawn_exactly_where_it_is_walked_into_and_aimed_at`).
    // Symmetric about the middle, so which way a quarter turn counts cannot
    // put it on the wrong side.
    if block_kind(id) == BLOCK_HIDE_FRAME {
        return Some((6.75 / 16.0, 9.25 / 16.0));
    }
    // **A door is three sixteenths of boards across the back of its cell**
    // (`door_quarters` turns it; the client's `mesh::door_block` draws it). Thick
    // enough that a sweep a frame long never steps over it, and thin enough
    // that an open door along the side of a doorway leaves the doorway.
    if is_door(id) {
        return Some((1.0 - DOOR_THICKNESS, 1.0));
    }
    None
}

/// How thick a door's boards are, in cells: three sixteenths, a plank on a
/// batten.
pub const DOOR_THICKNESS: f32 = 3.0 / 16.0;

/// The top of this block's collision box, measured up from the cell
/// floor. Zero means there is nothing here to walk into.
#[inline]
pub fn collision_height(id: BlockId) -> f32 {
    if !is_collidable(id) {
        return 0.0;
    }
    // **A floor dug downward is as tall as what is left of it.** Answered
    // here and not only in `geometry::block_box` because this is the number
    // the server's ground probe and the animals' footing read
    // (`logic::animals::standing_height`), and a server that thought a
    // quarter-dug floor was still a metre tall would hold every body a
    // three-quarter block above the hole the player is standing in.
    //
    // Only a bite out of the *top*: a bite out of a side takes nothing off
    // the height, and a bite out of the underside leaves the top of the
    // cell exactly where it was -- what moves there is the floor, which is
    // a thing a height measured up from the cell floor cannot say and
    // `block_box` answers instead.
    if let Some((crate::dig::Side::PosY, _)) = crate::dig::bite(id) {
        return crate::dig::left(id);
    }
    // ...and a wall built part of the way up is as tall as its courses
    // (`build::stage_box`), for the same reason. A wattle panel is a whole
    // cell tall and thin, which the height cannot say and `block_box` does.
    if let Some((min, max)) = crate::build::stage_box(id) {
        if min[0] == 0.0 && max[0] == 1.0 && min[2] == 0.0 && max[2] == 1.0 {
            return max[1];
        }
    }
    // **A pile of logs is as tall as its highest log** (`pit::pile_extent`),
    // for the dug floor's reason: one log is a third of a cell, and a probe
    // that read the table's whole cube would hold a player a metre over it.
    if let Some((_, top)) = crate::pit::pile_extent(id) {
        return top[1];
    }
    // ...and a cell of a lean-to is as tall as the thatch it holds
    // (`lean_to::boxes`): a heap of leaves pitched to a ridge, not the
    // pallet's two eighths it was.
    if crate::lean_to::is_lean_to(id) {
        return crate::lean_to::extent(id).map_or(0.0, |(_, top)| top[1]);
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
    // **Nothing is set down on a block being quarried.** A bite out of any
    // face leaves a cell that is no longer a flat metre of ground: out of
    // the top it is a hole, and out of a side it is a ledge with a quarter
    // of the cell missing from under the far edge. A torch stood on one
    // would hang over the gap.
    //
    // Deliberately *not* the same answer as `can_be_displaced_by_falling`,
    // which is what decides whether sand comes down: a bitten block still
    // holds a drift up, because what brings sand down is a cell becoming
    // air and a cell with rock left in it is not air. See `dig`.
    // ...and nothing on a pile of logs until it is full: short of eight its
    // top course has gaps in it, or there is no top course at all
    // (`pit::PILE_LOGS_AT`) -- six reach the top of the cell with one log.
    // ...and nothing on a wall part of the way up: its top is a course,
    // not a floor, and a heap or a wall is only started on a whole one.
    // ...and nothing on a block whose row says "a whole cube" while its
    // shape is less than that at the top: a step (the riser is the back
    // half of the top; the front half is the tread, half a cell down), a
    // door, a rack or a hide frame (a few sixteenths of it across the
    // cell), a lattice, a pit prop, a hive, a standing torch. Everything
    // that rests on the block under it -- a tuft, a torch, a knife set
    // down, a drift of snow, the snowfall itself -- is drawn from the
    // floor of its own cell, and on one of these that floor is mostly
    // air: snow fell onto a roof of steps as a sheet at the height of the
    // ridge, hanging over every tread. The boxes are the authority, and
    // `geometry`'s `nothing_is_a_floor_that_has_no_whole_floor_at_the_top_of_its_cell`
    // holds this list to them.
    //
    // Not the boughs and the palm's trunk, whose wood is also less than
    // their cell: which pieces of a tree join is read off this answer
    // (`branch::joins`), and the generator plants what it plants on it;
    // changing it moves trees, which is a different change from this one.
    !crate::dig::is_part(id)
        && is_collidable(id)
        && block_layers(id) == LAYERS_PER_BLOCK
        && (crate::pit::pile_extent(id).is_none() || crate::pit::pile_logs(id) == Some(crate::pit::PILE_LOGS_MAX))
        && !is_step(id)
        && collision_depth(id).is_none()
        && !is_lattice(id)
        && !is_prop(id)
        && !crate::bees::is_hive(id)
        && !crate::wildfire::is_standing_torch(id)
}

/// **The height a coating lying on this block rests at**, in the block's own
/// cell: 1.0 on a whole top, the bite's height on a block lowered from the
/// top (`dig::lowered` -- a lip on a generated slope, a floor dug down, a
/// handful heaped), `None` where a coating cannot lie.
///
/// What snow and ash ask instead of `has_full_top`. A lowered top is a level
/// square the width of the cell, a quarter, a half or three quarters up it,
/// and a coating is a sheet with no thickness: laid on that square it lies
/// on ground, where a torch or a stool would stand on it and hang its foot
/// over nothing. Asked `has_full_top`, every lip on every hillside
/// (`worldgen::lips`) stayed a green stripe through the whole of a winter,
/// and a burnt slope was grey with a green ring round each rise.
///
/// The coating stays in the cell over the block, where it always was, and is
/// drawn and aimed at lowered onto this height ([`rest_drop`]). Weighed and
/// rejected:
///
/// * **A snowy lip** -- the bite and the snow in one id. The lip's sixteen
///   bits are full (kind, `dig::DUG`, the face, the depth), so it would be a
///   kind of its own, and then every rule that reads turf (the spread, the
///   plants, the grazing, the spade lifting the sod) has to learn a second
///   turf, and ash would want a third. It also has no answer for a floor a
///   player dug down, which is the same id as a lip of earth.
/// * **The coating moved down into the lip's own cell.** A cell holds one
///   id; the lip is already in it.
/// * **A step's tread** is not in this list: a step is two levels, the tread
///   and the riser's top, and one sheet cannot lie on both.
#[inline]
pub fn coating_rests_at(ground: BlockId) -> Option<f32> {
    if has_full_top(ground) {
        return Some(1.0);
    }
    match crate::dig::bite(ground) {
        Some((crate::dig::Side::PosY, _)) => Some(crate::dig::left(ground)),
        _ => None,
    }
}

/// **How far below the floor of its own cell a flat thing lying on `ground`
/// is drawn, aimed at and cracked**: the part of a lowered top that is not
/// there (`coating_rests_at`), and nought on anything else.
///
/// Every flat thing and not only the coatings: a pebble or a stick lies on a
/// turf lip as it does on turf (`can_grow_on`), and drawn from the floor of
/// its cell it floated a quarter of a block over the grass. The mesher, the
/// aim (`geometry::block_box_for_aim_near`), the ray (`physics`) and the
/// cracks all take this one number, so none of them can put the snow
/// somewhere the others do not.
#[inline]
pub fn rest_drop(lying: BlockId, ground: BlockId) -> f32 {
    if !is_flat(lying) || has_full_top(ground) {
        return 0.0;
    }
    coating_rests_at(ground).map_or(0.0, |top| 1.0 - top)
}

/// **How far below the floor of its own cell a thing set down by hand on
/// `ground` lies** (`BLOCK_SET_DOWN`), or `None` where there is no flat top
/// for it to lie on at all.
///
/// A knife, a bowl or a loaf is not a torch: it has no foot to hang over a
/// gap, only an underside that wants a level surface somewhere in the cell
/// below. So it asks for *a* flat top, not a whole one -- a turf lip or a
/// floor dug down at the bite's height (`coating_rests_at`), a slab at its
/// half, and a step on its
/// tread, half a cell down ([`crate::geometry::set_down_rest`] moves it onto
/// the tread's half of the cell).
///
/// **It asked `has_full_top` once, and that was a floor almost nowhere**:
/// every generated slope is lipped (`worldgen::lips`), every stair and slab a
/// player builds is short of a whole top, and a player with Shift held and a
/// knife in hand found the only places it would go down were bare cubes.
/// Still refused: whatever has no single level top the width of a thing --
/// a block bitten from a side (a ledge with its far edge gone), a lattice, a
/// pit prop, a hive, a standing torch, a door or a frame of poles, a pile of
/// logs short of full, water and air -- and the furniture and the fires,
/// below.
pub fn set_down_drop(ground: BlockId) -> Option<f32> {
    if has_full_top(ground) {
        return Some(0.0);
    }
    if is_step(ground) {
        return Some(1.0 - 0.5);
    }
    if let Some(top) = coating_rests_at(ground) {
        return Some(1.0 - top);
    }
    // A slab is a level half, drawn at its height. Named rather than read
    // off "a collider the width of the cell short of the top": that rule,
    // tried first, also took a campfire, a table, a barrel, a carcass and a
    // bed, whose rows are a box round a drawing that is anything but level at
    // the box's top -- a knife on the logs of a fire, a loaf floating over a
    // boar -- and each of those is a decision of its own, not this one.
    matches!(block_kind(ground), BLOCK_TILE_SLAB | BLOCK_THATCH_SLAB | BLOCK_BRANCH_SLAB).then(|| 1.0 - block_height(ground))
}

/// **How far below the floor of its own cell anything that stands on the
/// block under it is drawn, aimed at and cracked**: a flat thing by
/// [`rest_drop`], and a plant -- a tuft, a flower, a sapling, a mushroom,
/// any cross -- by the same rule, onto the real top of what it grows on.
///
/// `ground` is the cell under it and `under` the one under that, for the
/// upper half of a tall plant: it stands on its own lower half, which stands
/// on the ground, and the two halves go down together or the stalk parts at
/// the seam.
///
/// **A plant used to be drawn from its own cell's floor whatever it grew
/// on**, and `can_grow_on` lets the meadow's tufts and flowers grow on the
/// turf lips the generator lays up every slope: every flower on a hillside
/// hung a quarter of a block over the grass it grew from, with daylight
/// under its stem. Kelp is not lowered: it is a ribbon that stacks, and a
/// lowered bottom length would open a gap under the next one up.
///
/// A slab, or anything else whose collider is the whole cell across and
/// short of the top, is a top too, at [`collision_height`].
#[inline]
pub fn stand_drop(standing: BlockId, ground: BlockId, under: BlockId) -> f32 {
    if is_flat(standing) {
        return rest_drop(standing, ground);
    }
    if !is_cross(standing) || matches!(block_kind(standing), BLOCK_KELP | BLOCK_KELP_TOP) {
        return 0.0;
    }
    if is_plant_top(standing) && is_tall_plant(ground) && !is_plant_top(ground) {
        return stand_drop(ground, under, BLOCK_AIR);
    }
    if has_full_top(ground) {
        return 0.0;
    }
    if let Some(top) = coating_rests_at(ground) {
        return 1.0 - top;
    }
    let height = collision_height(ground);
    let across = crate::geometry::block_box(ground, 0, 0, 0)
        .is_some_and(|(min, max)| min[0] <= 0.0 && min[2] <= 0.0 && max[0] >= 1.0 && max[2] >= 1.0 && min[1] <= 0.0);
    if across && height > 0.0 && height < 1.0 {
        1.0 - height
    } else {
        0.0
    }
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

/// What a cubic metre of this weighs, in kilograms.
///
/// **Density, not weight, is what decides whether a thing floats**, and
/// the two are different questions the table already half-answered: a
/// `BlockDef` carries the weight of *one item*, which says how much of
/// your carrying capacity a stack costs and nothing at all about what it
/// does in a lake. A bundle of feathers and a bundle of iron nails can
/// weigh the same and one of them floats.
///
/// So this is a property of the *material*, written as the real figure
/// rather than a made-up scale, because the real figures are what make
/// the answers agree with what a player expects: oak is about 700, dry
/// pine 500, fat 900, ice 920, water 1000, bone 1800, clay 1900, stone
/// 2600, iron 7800. Anything under a thousand floats; everything else
/// goes down, and how fast depends on how much heavier than water it is
/// (see `items::step_one`).
///
/// Grouped by family rather than listed per block: two hundred rows of
/// numbers is a table nobody would keep true, and the families are
/// exactly the distinctions that matter -- wood floats, rock does not,
/// and the interesting cases are the handful in between.
pub fn density(id: BlockId) -> f32 {
    // A piece of branch floats as its wood does -- a saxaul's sinks -- and a
    // grass or a wad of moss as the tuft does.
    if let Some(log) = piece_log(id).filter(|&log| log != BLOCK_LOG) {
        return density(log);
    }
    if crate::ground::is_grass(id) || block_kind(id) == BLOCK_MOSS {
        return 250.0;
    }
    match block_kind(id) {
        // **Wood and what is made of it.** Dry timber is the classic
        // float, and a raft is the reason anybody cares.
        BLOCK_LOG | BLOCK_BIRCH_LOG | BLOCK_STRIPPED_LOG | BLOCK_PLANKS | BLOCK_BIRCH_PLANKS
        | BLOCK_FIR_LOG | BLOCK_FIR_PLANKS | BLOCK_PINE_LOG | BLOCK_PINE_PLANKS | BLOCK_PEGGED_PINE_PLANKS
        | BLOCK_STICK | BLOCK_WORKED_STICK | BLOCK_TORCH | BLOCK_TORCH_LIT
        | BLOCK_TORCH_SPENT | BLOCK_BRACKET_FUNGUS | BLOCK_BARREL | BLOCK_BARREL_STANDING
        | BLOCK_BARREL_SALT | BLOCK_BARREL_GRAIN | BLOCK_BARREL_SEEDS | BLOCK_BARREL_MILLET
        | BLOCK_TWIG | BLOCK_BOUGH | BLOCK_OAR | BLOCK_RAFT
        | BLOCK_PALM_TRUNK => 600.0,
        // **Saxaul sinks.** It is a wood heavier than water: a log of it
        // dropped in a river goes to the bottom, where a fir's floats off
        // downstream. Eleven hundred.
        BLOCK_SAXAUL_LOG | BLOCK_SAXAUL_PLANKS | BLOCK_PEGGED_SAXAUL_PLANKS => 1100.0,
        // Willow is the lightest timber here, as it is anywhere.
        BLOCK_WILLOW_LOG | BLOCK_WILLOW_PLANKS | BLOCK_PEGGED_WILLOW_PLANKS => 450.0,
        // Grass, straw, wool, feathers: air held in a shape.
        BLOCK_FIBER | BLOCK_LEAF_HANDFUL | BLOCK_CORD | BLOCK_TALL_GRASS | BLOCK_DRY_GRASS | BLOCK_WOOL | BLOCK_FEATHER
        | BLOCK_STRAW_BED | BLOCK_LEAN_TO | BLOCK_LEAVES | BLOCK_BIRCH_LEAVES | BLOCK_APPLE_LEAVES
        | BLOCK_FIR_NEEDLES | BLOCK_SAXAUL_LEAVES | BLOCK_PINE_NEEDLES | BLOCK_WILLOW_LEAVES
        | BLOCK_BUSH_LEAVES | BLOCK_ACACIA_LEAVES | BLOCK_MAPLE_LEAVES | BLOCK_NEST | BLOCK_NEST_EGGS
        // Cotton is the most air-in-a-shape thing there is, and a bolt of
        // dry cloth floats until it soaks -- which `items` does not model.
        | BLOCK_WILD_COTTON | BLOCK_COTTON_PLANT | BLOCK_COTTON_RIPE | BLOCK_COTTON
        | BLOCK_WILD_MILLET | BLOCK_MILLET_PLANT | BLOCK_MILLET_RIPE
        | BLOCK_CLOTH | BLOCK_WITHERED_CROP
        // ...and the wild plants, leaf and stalk.
        | BLOCK_FIREWEED | BLOCK_CATTAIL | BLOCK_NETTLE | BLOCK_BRACKEN | BLOCK_ARUNDO | BLOCK_CANE | BLOCK_BILBERRY
        | BLOCK_BILBERRY_BARE | BLOCK_STRAWBERRY | BLOCK_STRAWBERRY_BARE | BLOCK_PLANTAIN | BLOCK_FERN
        | BLOCK_SUNDEW => 250.0,
        // Fat floats, and only just -- which is the whole of why tallow
        // is skimmed off the top of a pot.
        BLOCK_FAT => 900.0,
        // Ice, and it floats by a whisker, as everybody has seen.
        BLOCK_ICE => 920.0,
        // Hide, leather and what is worn: waterlogged rather than
        // buoyant, and it sinks slowly.
        BLOCK_HIDE | BLOCK_PELT | BLOCK_LEATHER | BLOCK_BEAR_HIDE | BLOCK_SINEW | BLOCK_SAIL | BLOCK_SADDLEBAGS => 1050.0,
        // Bone and clay.
        BLOCK_BONE | BLOCK_BONES | BLOCK_BONES_2 | BLOCK_BONES_3 | BLOCK_BONES_4 => 1800.0,
        BLOCK_CLAY | BLOCK_JUG | BLOCK_JUG_RAW | BLOCK_VESSEL | BLOCK_VESSEL_RAW | BLOCK_BOWL | BLOCK_BOWL_RAW
        | BLOCK_MOULD | BLOCK_MOULD_RAW | BLOCK_BRICKS | BLOCK_TERMITE_MOUND => 1900.0,
        BLOCK_SANDSTONE_BRICKS => 2300.0,
        // Metal, which is the one thing nobody expects to see bob.
        BLOCK_COPPER_INGOT | BLOCK_BRONZE_INGOT | BLOCK_TIN_INGOT | BLOCK_IRON_INGOT
        | BLOCK_IRON_BLOOM | BLOCK_NATIVE_COPPER => 7800.0,
        // Everything else is rock, earth or something that behaves like
        // it. Stone is the honest default here: the failure it prevents
        // is a new block quietly floating because nobody added a row.
        _ => 2600.0,
    }
}

/// What water weighs, for the comparison above.
pub const WATER_DENSITY: f32 = 1000.0;

/// Does this bob rather than sink?
#[inline]
pub fn floats(id: BlockId) -> bool {
    density(id) < WATER_DENSITY
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

/// The same drag for a foot in a snowshoe: **snow is walked over, not
/// waded**, and nothing else changes.
///
/// Snow and only snow -- a drift, a snow cover, a block of it -- because a
/// snowshoe spreads a foot over what gives under it, and mud, sand and a
/// ford do not give that way (a snowshoe in a river is a raft on a foot).
/// Not all the way to a bare meadow's pace: the frame sinks a hand's depth
/// and a stride in one is shorter (`SNOWSHOE_DRAG`).
///
/// Shared for `surface_drag`'s reason: the client moves by it and the server
/// has to expect what it moves.
#[inline]
pub fn surface_drag_shod(id: BlockId, snowshoes: bool) -> f32 {
    let drag = surface_drag(id);
    if snowshoes && is_snow(id) {
        drag.max(SNOWSHOE_DRAG)
    } else {
        drag
    }
}

/// What a snowshoe leaves of a stride on snow. See [`surface_drag_shod`].
pub const SNOWSHOE_DRAG: f32 = 0.9;

/// Is this snow a foot sinks into: the block, a drift, the cover on a field?
#[inline]
pub fn is_snow(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_SNOW | BLOCK_SNOW_COVER)
}

/// How well a foot holds on this, as a multiplier on both friction and
/// acceleration.
///
/// One for every surface a boot bites into, and a fraction for ice. See
/// `blocks::BlockDef::grip` for why it is a second number rather than a
/// smaller `drag`, and the client's `physics` for what it does with it.
///
/// Shared for the same reason `surface_drag` is: what the client may do
/// on a surface has to be what the server expects, or a player crossing
/// a frozen lake is a player the anti-cheat is arguing with.
#[inline]
pub fn surface_grip(id: BlockId) -> f32 {
    crate::blocks::definition(id).grip
}

/// How many of this fit in one slot.
///
/// `inventory::MAX_STACK` for everything you gather and one for
/// everything you *hold* -- see `blocks::BlockDef::stack`. Every path
/// that puts something into a slot asks this rather than the constant,
/// which is what makes "a tool does not stack" a fact about the tool
/// instead of a rule thirteen call sites have to remember.
#[inline]
pub fn stack_limit(id: BlockId) -> u32 {
    // **An empty jug stacks to one, whatever the block table says, and
    // the reason is where its contents live.** What has been poured
    // into a jug rides in `inventory::Stack::damage` -- one number for
    // the whole slot, exactly like the wear on a tool. A slot holding
    // four jugs has one such number between them, so it cannot say that
    // this jug has grain in it and that one sand; the first merge would
    // pick a winner and the loser's contents would simply stop existing.
    // That is the same argument `blocks::BlockDef::stack` already makes
    // for a tool -- see the note on `Stack::damage` -- and a jug is now
    // in the same class of object for the same reason.
    //
    // The table says so as well (`blocks.rs`, the `BLOCK_JUG` row), and
    // the two agree because this function *is* the table: everything
    // that fills a slot asks here, which is the whole point of it, so a
    // caller reading the row directly cannot get round the rule.
    // `logic::api_impl` was the one that did, and now asks this.
    crate::blocks::definition(id).stack.max(1)
}

/// How many swings this tool has in it, or `None` if it is not a tool.
///
/// The one question everything about wear asks: the server, which spends
/// a swing every time one is used; the inventory, which will not carry a
/// tool worn past its own limit; and the client, which draws the bar.
#[inline]
pub fn tool_durability(id: BlockId) -> Option<u32> {
    crate::blocks::definition(id).durability
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
    // **But not by another of its own kind.** A plant set into a plant, or
    // a stone set down on a stone lying there, used to be written straight
    // over it, and the one that was there went nowhere -- no drop, no
    // refusal. Building a wall through a tuft of grass is still building
    // through it; putting a strawberry into a bilberry is a misclick, and
    // a misclick must not cost a plant. "код проверяет можно ли поставить
    // блок в блок и запрещает, но если поставить растение в растение то
    // одно исчезнет в никуда".
    let small = |id: BlockId| is_cross(id) || is_flat(id);
    if small(existing) && small(wanted) {
        return None;
    }
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
    is_flat(id) && matches!(block_kind(id), BLOCK_ASH | BLOCK_SNOW_COVER | BLOCK_LEAF_LITTER)
}

/// A coating that is also opaque enough to stand in for the floor, so
/// the face under it need not be drawn and it can lie on the floor itself.
///
/// Leaf litter covers its cell edge to edge like ash, but it is fallen
/// leaves with the ground showing between them. Hiding the face under it
/// the way ash does left those gaps looking into the block below -- the
/// grass "vanished under the leaves". So litter keeps ash's zero inset and
/// lies flush on a floor that is still drawn.
#[inline]
pub fn hides_the_floor(id: BlockId) -> bool {
    // **Leaf litter hides it again, and its picture is why.** It was here
    // as an exception, because the leaves were drawn with holes and the
    // grass had to show through them -- and the price of that was the
    // fiftieth of a block it had to be lifted by, which the player saw as
    // leaves hanging over the ground. The holes were painted earth for a
    // while, so the litter was a coating like ash: on the floor of its cell,
    // with the face under it not drawn at all.
    //
    // **...and not any more: "опавшую листву сделай прозрачной".** A floor
    // of painted earth is a brown carpet, and the ground a wood grows on --
    // the turf, the moss, the mud -- was gone under every crown. The holes
    // are holes again and the face under them is drawn; what keeps the
    // litter off the fiftieth that read as hovering is that it is not
    // lifted at all (`flat_lift`), and what keeps it from fighting the face
    // it lies flush on is a nudge of its depth toward the eye in the vertex
    // shader (`mesh::DECAL_TINT`) -- the comparison moved, not the leaves.
    is_covering_flat(id) && block_kind(id) != BLOCK_LEAF_LITTER
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
    // Every coating, and not only the ones that hide the floor: leaf litter
    // lies flush on a floor that is still drawn, and wins the depth test by
    // a bias rather than by a height (`hides_the_floor`).
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

/// A campfire, lit or laid.
///
/// Asked wherever the two have to be treated as one thing: the fuelling
/// gesture, which works on either; the drop, which is the same sticks
/// and stones whichever it was; and worldgen, which will not plant
/// anything on top of one.
#[inline]
pub fn is_campfire(id: BlockId) -> bool {
    // A firepit is drawn and carried as the low fire a campfire is; what
    // differs is only what it gives back (`BLOCK_FIREPIT`).
    matches!(block_kind(id), BLOCK_CAMPFIRE | BLOCK_CAMPFIRE_LIT | BLOCK_FIREPIT | BLOCK_FIREPIT_LIT)
}

/// Is this a fire of any kind -- a campfire or a kiln, lit or laid?
///
/// The question `is_campfire` was really being asked, everywhere it was
/// asked. There are two hearths now and everything that treats a fire as
/// a fire wants both of them: the gesture that strikes a spark into one
/// or feeds one, the drop that gives its materials back, and worldgen,
/// which will not plant anything on top of either.
#[inline]
pub fn is_hearth(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_CAMPFIRE
            | BLOCK_CAMPFIRE_LIT
            | BLOCK_KILN
            | BLOCK_KILN_LIT
            | BLOCK_BLOOMERY
            | BLOCK_BLOOMERY_LIT
            | BLOCK_FIREPIT
            | BLOCK_FIREPIT_LIT
    )
}

/// Is this fire actually burning?
///
/// The one question four separate parts of the game ask about a cell,
/// and the reason lit and unlit are two ids rather than one with a flag
/// (see `BLOCK_CAMPFIRE`): the light engine reads it off the block
/// table, the crafting menu asks whether one is within reach, the
/// weather puts it out, and standing on it hurts.
#[inline]
pub fn is_burning(id: BlockId) -> bool {
    // Not the pit kiln or the log pile, and deliberately: this is what the
    // fire map adopts as a fire with fuel in it and burns down in four
    // minutes, and a kiln that went out at four minutes of its hour would
    // be a kiln that never fired a pot. Those burn on their own clock
    // (`logic::pits` on the server).
    matches!(block_kind(id), BLOCK_CAMPFIRE_LIT | BLOCK_KILN_LIT | BLOCK_BLOOMERY_LIT | BLOCK_FIREPIT_LIT)
}

/// Is this a flame an unlit torch can be lit from?
///
/// Every fire there is, not only the ones `is_burning` names: that list is
/// what the fire map burns down on its own clock, and a pit kiln, a log pile,
/// a burning wall and a standing torch alight are all fires a player can hold
/// a torch to, whatever keeps them burning.
#[inline]
pub fn lights_a_torch(id: BlockId) -> bool {
    is_burning(id)
        || matches!(
            block_kind(id),
            BLOCK_PIT_KILN_LIT | BLOCK_LOG_PILE_LIT | BLOCK_BURNING_LOG | BLOCK_BURNING_PLANKS | BLOCK_STANDING_TORCH_LIT
        )
}

/// Does a cell of this block count as a ceiling over whoever is under
/// it?
///
/// Anything that takes most of the light out of what passes through it,
/// which is deliberately the lighting engine's own number rather than a
/// second list of block names: a wall, a floor and a slab of ice are all
/// shelter, and a leaf, a pane of water and a tuft of grass are all not.
/// That is the right line -- a canopy keeps the sun off and does nothing
/// at all about a night in the open, which is what a player standing
/// under one would expect.
///
/// **Air is checked first and by name.** Air is not in the block table
/// -- it is the absence the table lists the alternatives to -- so
/// `definition` resolves it to the deliberately inert `UNKNOWN` row,
/// which is opaque by design (see the note on it: an unaccounted cell
/// must not leak light at the edge of the loaded world). Asking that row
/// about the sky would answer "there is a ceiling here" for every cell
/// of empty air above every player in the world.
///
/// **One rule, two askers.** The server asks it about shelter --
/// `climate::has_roof`, which decides whether the night chill and the
/// rain reach a player -- and the client asks it about sight, in
/// `engine::fog`, which decides whether distance is the colour of the
/// sky or the colour of rock. Those two have to agree or the game says
/// "you are indoors" about a place it draws as outdoors; they agreed by
/// coincidence when the rule was written twice, and they agree by
/// construction now that it is written here.
#[inline]
pub fn blocks_the_sky(id: BlockId) -> bool {
    if is_air(id) {
        return false;
    }
    if is_liquid(id) {
        return false;
    }
    // A burning cell overhead is not a ceiling, it is a fire on a shelf.
    if is_burning(id) {
        return false;
    }
    // A door is a wall while it is shut and a hole while it is open, and
    // the row can only say one of those. See `light_opacity`.
    if is_door(id) {
        return !door_is_open(id);
    }
    // **A wall is a wall when it is finished**, and not before: three
    // courses of brick are a cube by their row, and a room walled with them
    // would be shut to the sky over a gap a quarter of a metre high. A
    // daubed wattle panel closes a room though light gets past its edges
    // (its row's opacity is nought). See `build::closes`.
    if crate::build::is_staged(id) {
        return crate::build::closes(id);
    }
    crate::blocks::definition(id).opacity >= 8
}

/// What a fire of this kind looks like once it has gone out.
///
/// The counterpart of `is_burning`, and it exists because there are two
/// fires now: a kiln whose fuel ran out has to come back as a kiln and
/// not as a campfire, which is what a single hard-coded id would have
/// done to it. `None` for anything that was never alight.
pub fn burnt_out(id: BlockId) -> Option<BlockId> {
    let cold = match block_kind(id) {
        BLOCK_CAMPFIRE_LIT => BLOCK_CAMPFIRE,
        BLOCK_KILN_LIT => BLOCK_KILN,
        BLOCK_BLOOMERY_LIT => BLOCK_BLOOMERY,
        BLOCK_FIREPIT_LIT => BLOCK_FIREPIT,
        _ => return None,
    };
    // **Through the facing, not past it.** A kiln and a bloomery both
    // have a front (`blocks::BlockDef::faces`), and the way they are
    // turned lives in the same variant field the kind does -- so a match
    // on `block_kind` and a bare constant back is a match that throws it
    // away. It did: a furnace a player had set with its mouth toward
    // them swung round to face north the moment its fuel ran out, and
    // again the moment they lit it (see `lights_into`). The rack has
    // carried its own second fact through the same field since it was
    // written; these two did not. A campfire has no front, and `faced`
    // hands back the plain kind for anything that has not, so this costs
    // it nothing.
    Some(faced(cold, block_facing(id)))
}

/// A torch, in any of its three states.
///
/// A list rather than a name checked at each site, for the reason
/// `is_implement` is one: a torch is a *seventh* thing an item can be --
/// not a tool, not food, not a garment, not a vessel, not an implement,
/// not an ingredient -- and the places that have to know are spread
/// across three crates. It is the thing you light and carry.
#[inline]
pub fn is_torch(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_TORCH | BLOCK_TORCH_LIT | BLOCK_TORCH_SPENT
    )
}

/// ...and whether this one is actually alight.
#[inline]
pub fn is_lit_torch(id: BlockId) -> bool {
    block_kind(id) == BLOCK_TORCH_LIT
}

/// Is this material **one sheet** rather than a stack of separate
/// pieces?
///
/// The client gives every block face a faint shade of its own, so that a
/// wall of one texture is not a hundred identical copies of it -- see
/// `BLOCK_VARIATION` in the terrain shader. That is right for everything
/// a wall is built out of, and wrong for the three materials that are
/// not built out of anything: **a frozen lake is not a hundred sheets of
/// ice, it is one sheet**, and so is a snowfield, and so is water.
///
/// **What it looked like was reported as a rendering fault**, and the
/// report was accurate: a chequerboard of light and dark squares laid
/// over the ice, exactly on the block boundaries. The variation is a
/// *proportion*, so what it is worth in levels grows with the material's
/// brightness -- at 5.5% it is under four levels on grass at 55 and over
/// ten on ice at 195 -- and a bright, smooth material gives the eye
/// nothing else to look at, so the grid is the only thing in the
/// picture. Grass hides the same variation inside its own texture.
///
/// Here rather than in the client because the mesher writes the flag and
/// the shader reads it, and the two must not come to disagree about
/// which materials are sheets.
#[inline]
pub fn is_one_sheet(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_ICE | BLOCK_SNOW | BLOCK_WATER)
}

/// Is this something you *use on the world* rather than mine with,
/// build from or eat?
///
/// A list, and it is here so that it is a list rather than a special
/// case buried in a test. The mining tools are a ladder of tiers and
/// everything else an item can be is either an ingredient or a meal --
/// an implement is the fourth thing, and `crafting`'s "nothing in this
/// table is a dead end" check has to know about it or a hoe reads as an
/// oversight.
///
/// **Both hoes, and that is the point of the function being a list.**
/// The server asks this one question before turning turf into a field
/// (`lib::use_block`); a copper hoe that tilled nothing would be a
/// recipe with no purpose, and the bug would sit in the server rather
/// than anywhere a reader of `blocks.rs` would look.
#[inline]
pub fn is_implement(id: BlockId) -> bool {
    // ...and the peg, which is the third thing on this list and the
    // first that is *spent* rather than kept: a hoe tills a hundred
    // fields, a peg goes into one joint and stays there. Same question
    // though -- it is used on the world -- and the server asks this one
    // before driving it (`lib::use_block`).
    is_hoe(id) || block_kind(id) == BLOCK_PEG
}

/// Is this a hoe, of either metal?
///
/// Split out of `is_implement` the day the peg joined that list. The
/// server asks "is this an implement" to decide whether a right click
/// means anything at all, and then has to ask *which* -- and for one
/// day it did not, so a peg held over a meadow tilled it.
#[inline]
pub fn is_hoe(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_HOE | BLOCK_COPPER_HOE)
}

/// What this timber becomes when a peg is driven through it, if it is
/// timber a peg can be driven through.
///
/// **Planks and nothing else.** A peg fastens a *joint*, and the joints
/// in this world are made of boards: a log is already a post holding
/// something up, stone is not pegged at all (a mason uses piers), and a
/// chest with a peg in it is a chest with a peg in it. Keeping the list
/// this short is what keeps the mechanic explicable in one sentence --
/// see `falling::Looseness::Built` for the other half of it.
#[inline]
pub fn pegged_form(id: BlockId) -> Option<BlockId> {
    match block_kind(id) {
        BLOCK_PLANKS => Some(BLOCK_PEGGED_PLANKS),
        BLOCK_BIRCH_PLANKS => Some(BLOCK_PEGGED_BIRCH_PLANKS),
        BLOCK_FIR_PLANKS => Some(BLOCK_PEGGED_FIR_PLANKS),
        BLOCK_SAXAUL_PLANKS => Some(BLOCK_PEGGED_SAXAUL_PLANKS),
        BLOCK_PINE_PLANKS => Some(BLOCK_PEGGED_PINE_PLANKS),
        BLOCK_WILLOW_PLANKS => Some(BLOCK_PEGGED_WILLOW_PLANKS),
        _ => None,
    }
}

/// Is this timber already fastened?
///
/// The question `falling` asks, and the reason it is a function rather
/// than a `matches!` at the call site: a fourth wood would otherwise be
/// a fourth thing to remember in a file about gravity.
#[inline]
pub fn is_pegged(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_PEGGED_PLANKS | BLOCK_PEGGED_BIRCH_PLANKS | BLOCK_PEGGED_FIR_PLANKS | BLOCK_PEGGED_SAXAUL_PLANKS
            | BLOCK_PEGGED_PINE_PLANKS | BLOCK_PEGGED_WILLOW_PLANKS
    )
}

/// What colour to draw a garment's icon.
///
/// **Twelve garments share four pictures**, and this is what tells them
/// apart. The pictures (`assets/textures/worn/*.png`) are drawn in
/// values rather than in colours -- a light field, a darker outline, a
/// highlight -- so multiplying one by a leather brown gives leather and
/// multiplying it by a pale gold gives bronze.
///
/// That is not thrift for its own sake. The texture array the client
/// builds is capped at **256 layers by the hardware**, and this build is
/// within a handful of that ceiling; twelve more images would not fit
/// and twelve tints cost nothing at all. It also happens to be the right
/// design: a cuirass and a tunic *are* the same garment in different
/// materials, and saying so once is better than drawing it twice.
///
/// `None` for everything that is not a garment, which is nearly
/// everything -- so a caller multiplies by this only when there is
/// something to multiply by, and every other icon is drawn exactly as it
/// was.
pub fn garment_tint(id: BlockId) -> Option<[f32; 3]> {
    // Leather. The same brown the leather item itself is drawn in, so a
    // player who has both in their pack sees the material rather than
    // two unrelated pictures.
    const LEATHER: [f32; 3] = [0.60, 0.40, 0.24];
    // Bronze: warm, pale gold. Distinctly *not* copper -- the ingots
    // are already two different browns and a third would be one too
    // many.
    const BRONZE: [f32; 3] = [0.82, 0.62, 0.30];
    // Iron: a cool blue-grey, so it reads as the darker and harder of
    // the two metals at a glance and at hotbar size.
    const IRON: [f32; 3] = [0.66, 0.70, 0.76];
    // Wool: the undyed off-white a fleece actually is, warmed very
    // slightly so it does not read as snow against a winter sky. Pale
    // enough that it is never mistaken for the leather set at hotbar
    // size, which is the only job a tint has.
    const WOOL: [f32; 3] = [0.92, 0.89, 0.82];
    // Fur: dark, cool and desaturated, which is what an untanned skin
    // with the hair still on looks like beside a tanned one. It has to
    // read as *not leather* at hotbar size and that is the whole
    // constraint -- leather is a warm mid brown, so this is a cold dark
    // one, and the two are told apart by value before colour.
    const FUR: [f32; 3] = [0.34, 0.28, 0.24];
    // Cloth: undyed cotton, which is a greyer, flatter off-white than a
    // fleece. It sits beside wool in a pack and has to be told from it
    // at hotbar size, so it is darker by a clear step in value and a
    // touch cooler -- the colour of unbleached calico rather than of
    // cream.
    const CLOTH: [f32; 3] = [0.78, 0.76, 0.68];

    match block_kind(id) {
        BLOCK_LEATHER_CAP | BLOCK_LEATHER_TUNIC | BLOCK_LEATHER_LEGGINGS
        | BLOCK_LEATHER_BOOTS => Some(LEATHER),
        BLOCK_BRONZE_HELM | BLOCK_BRONZE_CUIRASS | BLOCK_BRONZE_GREAVES | BLOCK_BRONZE_BOOTS => {
            Some(BRONZE)
        }
        BLOCK_IRON_HELM | BLOCK_IRON_CUIRASS | BLOCK_IRON_GREAVES | BLOCK_IRON_BOOTS => Some(IRON),
        BLOCK_WOOL_CAP | BLOCK_WOOL_TUNIC | BLOCK_WOOL_LEGGINGS | BLOCK_WOOL_BOOTS => Some(WOOL),
        BLOCK_FUR_HOOD | BLOCK_FUR_CLOAK => Some(FUR),
        BLOCK_CLOTH_CAP | BLOCK_CLOTH_TUNIC | BLOCK_CLOTH_TROUSERS | BLOCK_CLOTH_WRAPS => {
            Some(CLOTH)
        }
        // Tar: leather gone nearly black and a little warm, the colour of a
        // boat's seams. Darker than fur by a clear step, because the two sit
        // in the same slot and fur is the one coat it must not be taken for.
        BLOCK_TARRED_TUNIC => Some([0.22, 0.17, 0.13]),
        // Snowshoes: pale bent ash and rawhide, the colour of the frame and
        // not of a boot -- the one pair of "boots" drawn in wood.
        BLOCK_SNOWSHOES => Some([0.80, 0.68, 0.48]),
        _ => None,
    }
}

// ---- vessels ----
//
// A jug is the first thing in this world that is the *same item* in two
// states, and the two ids are how that is said. Everything that has to
// know which is which asks one of the three functions below rather than
// comparing ids, so the day there is a second liquid there is one place
// to widen.

/// Whether this is a jug at all, full or empty.
#[inline]
pub fn is_vessel(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_JUG | BLOCK_JUG_WATER)
}

/// Whether this vessel can be *opened*: looked into, filled and emptied a
/// handful at a time, in the hand or set down on a table.
///
/// ## Why this is the jug and nothing else
///
/// **Only the empty jug holds goods, and the jug of water holds water.**
/// A full jug is one measure of one river, and the only fact inside it --
/// which river -- is already its name (`jug_of`). A screen with one slot
/// that can never change is a screen that says what the tooltip said, so
/// a liquid vessel is used rather than opened: drunk, poured into a
/// barrel, dipped full.
///
/// **The crucible (`BLOCK_VESSEL`) is not a carrier either**, though it is
/// the older pot. It is apparatus: the smelting recipes take it and give
/// it back (`crafting.rs`, every `returns` naming it), and what they give
/// back is a new stack. A crucible of seeds put through a pour would come
/// out a clean crucible with the seeds deleted, and guarding every recipe
/// against that is a rule for every future recipe to forget.
///
/// **A few slots, the way a small vessel has elsewhere, was rejected** for
/// the reason the jug stacks to one. What is inside rides in
/// `inventory::Stack::damage`, and that is one number: one kind, a count
/// (`inventory::jug_contents`). Three kinds would need either a fourth
/// field on `Stack` -- every save format and the wire versioned for it,
/// the cost `inventory.rs` already refused -- or a synthetic container id
/// in `damage` with the contents kept beside the world, which is a second
/// lifetime to garbage-collect and a copied stack away from a dupe. And a
/// jug holding three kinds is a sixth of a pack slot that ends the
/// decision about which one thing is worth a lump of fired clay.
#[inline]
pub fn opens_as_vessel(id: BlockId) -> bool {
    block_kind(id) == BLOCK_JUG
}

/// What an empty vessel becomes when it is filled.
///
/// `None` for a vessel that is already full, and for everything that is
/// not one -- so the server's fill gesture is a single call and a
/// `let else`.
#[inline]
pub fn filled_vessel(id: BlockId) -> Option<BlockId> {
    match block_kind(id) {
        BLOCK_JUG => Some(BLOCK_JUG_WATER),
        _ => None,
    }
}

/// ...and what a full one becomes when it is drunk.
#[inline]
pub fn emptied_vessel(id: BlockId) -> Option<BlockId> {
    match block_kind(id) {
        BLOCK_JUG_WATER => Some(BLOCK_JUG),
        _ => None,
    }
}

/// Whether this is loose, dry stuff that can be poured into a jug and
/// carried in it. See `inventory::jug_contents` for where it then lives.
///
/// ## Why the list is this short, and why no food is on it
///
/// Two rules pick it, and both are about what a jug *is*. A jug is a
/// vessel with a neck: what goes in has to pour, so it is grain and
/// seed and dust and grit and the small hard things a handful of which
/// is a handful -- not a plank, not a hide, not an axe. And a jug has
/// no lid the world can see inside, so what goes in has to be something
/// the world does not need to keep watching.
///
/// **That second rule is why there is no berry, no meat and no root
/// here, and it is not squeamishness about realism.** Perishables age
/// because `primitive_server::logic::rot` walks the *slots* of an
/// inventory and steps the variant field of whatever is in them (see
/// `food::rot_stage`). A stack hidden inside another stack's `damage`
/// is not a slot, so that walk would never reach it: a jug of berries
/// would be a larder that stops time, and the one mechanic in this game
/// that makes a player come home before the meat turns would have a
/// one-item answer costing a lump of clay. A jug of grain is fine
/// precisely because grain has no clock -- it is the flour end of the
/// field, and it keeps whether it is in a jug or not.
///
/// Fibre is on the list and cord is not, for the pouring rule: a wad of
/// fibre is a handful of dry stuff, a cord is a made thing with two
/// ends.
pub fn pours(block: BlockId) -> bool {
    matches!(
        block_kind(block),
        // Off a field or a threshing floor: the two halves of the crop
        // that are not the plant.
        BLOCK_GRAIN | BLOCK_SEEDS | BLOCK_MILLET
            // Out of a fire, out of a bloom, out of a riverbank: the
            // three powders and grits this world produces.
            | BLOCK_ASH
            | BLOCK_IRON_DUST
            | BLOCK_CLAY
            // What the ground is made of where it is not rock.
            | BLOCK_SAND
            | BLOCK_GRAVEL
            // Small hard things picked up off the floor of the world.
            | BLOCK_PEBBLE
            | BLOCK_FLINT_FLAKE
            | BLOCK_FIBER
    )
}

/// What this block becomes if it is left alone long enough.
///
/// The whole of the growth table, in one function, because the server's
/// `logic::growth` used to have the bush's answer written into it in
/// three places -- and the day a second thing grew, all three would have
/// had to learn about it. A crop is exactly that second thing.
///
/// `None` for anything that is already finished, or that never grows.
pub fn ripens_into(id: BlockId) -> Option<BlockId> {
    match block_kind(id) {
        BLOCK_BARE_BUSH => Some(BLOCK_BERRY_BUSH),
        BLOCK_SEEDS => Some(BLOCK_WHEAT),
        BLOCK_WHEAT => Some(BLOCK_WHEAT_RIPE),
        BLOCK_COTTON_SEEDS => Some(BLOCK_COTTON_PLANT),
        BLOCK_COTTON_PLANT => Some(BLOCK_COTTON_RIPE),
        BLOCK_MILLET => Some(BLOCK_MILLET_PLANT),
        BLOCK_MILLET_PLANT => Some(BLOCK_MILLET_RIPE),
        // A picked apple tree fruits again -- where it was picked, and
        // nowhere else. The bare leaf with no variant is the rest of the
        // canopy and stays leaves: see `BLOCK_APPLE_LEAVES_PICKED` for the
        // tree of forty apples that answering it grew. The clock is the
        // bush's rather than the crop's, and it only runs in warm air --
        // see `growth::fruit_sets_above_c`.
        BLOCK_APPLE_LEAVES if id == BLOCK_APPLE_LEAVES_PICKED => Some(BLOCK_APPLE_LEAVES_FRUIT),
        // ...and a picked palm, where its coconuts were and nowhere else,
        // for the orchard's reason.
        BLOCK_PALM_FRONDS if id == BLOCK_PALM_FRONDS_PICKED => Some(BLOCK_PALM_COCONUTS),
        // ...and a robbed nest is laid in again. On the bush's clock,
        // not the crop's, for the reason the orchard is: a nest is a
        // place you come back to, and one that refilled by the time you
        // had climbed down would be a vending machine in a tree.
        BLOCK_NEST => Some(BLOCK_NEST_EGGS),
        // ...and a raided hive fills again, one comb at a time, on the same
        // clock counted in warm air (`growth::fruit_sets_above_c`): the bees
        // forage when they fly (`bees::BEES_FLY_C`). A comb at a time rather
        // than full in one step, so a hive raided early is worth less than
        // one left alone -- and a full one ripens into nothing.
        BLOCK_WILD_HIVE if crate::bees::honey_in(id) < crate::bees::HIVE_FULL => {
            Some(crate::bees::hive_holding(crate::bees::honey_in(id) + 1))
        }
        // ...and a mussel bed fills a mussel at a time, as a raided hive
        // fills a comb at a time and for exactly the hive's reason: a bed
        // picked over early is worth less than one left alone, and a rock
        // that came back full in one step would make the walk along the
        // headland pointless. A full bed ripens into nothing. What makes it
        // *slow* is not here -- it is the number of steps the server counts
        // before it runs this (`shore::REGROW_STEPS`), because this table
        // says what a thing becomes and never how long it takes.
        BLOCK_MUSSEL_BED | BLOCK_MUSSEL_ROCK if crate::shore::mussels_in(id) < crate::shore::BED_FULL => {
            Some(crate::shore::bed_holding(crate::shore::mussels_in(id) + 1))
        }
        // ...and the two berries of the forest floor, picked, as the bush.
        BLOCK_BILBERRY_BARE => Some(BLOCK_BILBERRY),
        BLOCK_STRAWBERRY_BARE => Some(BLOCK_STRAWBERRY),
        // ...and a tall plant's shoot grows into the plant. This says what
        // the shoot's own cell becomes; the upper half it grows into the
        // cell over it is `growth`'s to write, because a table from one id to
        // one id cannot say "and the cell above".
        BLOCK_FIREWEED | BLOCK_CATTAIL | BLOCK_NETTLE | BLOCK_BRACKEN | BLOCK_ARUNDO if id & PLANT_YOUNG != 0 => {
            Some(block_kind(id))
        }
        _ => None,
    }
}

/// Can what is in this cell be taken off it by hand, with a right click,
/// leaving the plant standing?
///
/// **Apples only, and on purpose.** The berry bush and the nest leave
/// something behind too (`blocks::BlockDef::leaves_behind`) and would fit
/// the same gesture; they were not asked for and they keep the break they
/// have. What the hand gets is what breaking the cell drops, and what
/// stays is what breaking it leaves (`block_drop`, `block_residue`), so a
/// pick and a break can never disagree about an apple.
///
/// Shared, because the client decides from it that a right click is a
/// pick rather than a placement (`UseGesture::Pick`) and the server decides
/// from it that the pick is allowed.
#[inline]
pub fn picks_by_hand(id: BlockId) -> bool {
    // ...and coconuts, which are the palm's apples: the thing a player
    // walked to, taken without felling the thing that grows it.
    // ...and a mussel bed with anything on it, which is the same gesture
    // again on a rock: what comes off is the mussels and what stays is the
    // bed, thinner. **Only while there is something on it** -- a bare bed is
    // not a pick that gives nothing, it is a rock, and a rock is broken.
    matches!(block_kind(id), BLOCK_APPLE_LEAVES_FRUIT | BLOCK_PALM_COCONUTS)
        || crate::shore::mussels_in(id) > 0
}

/// The lit form of a hearth that can be struck alight, if it is one.
///
/// The other direction, for the flint that lights it. Same reason: the
/// spark has to know what it is striking into.
pub fn lights_into(id: BlockId) -> Option<BlockId> {
    let alight = match block_kind(id) {
        BLOCK_CAMPFIRE => BLOCK_CAMPFIRE_LIT,
        BLOCK_KILN => BLOCK_KILN_LIT,
        BLOCK_BLOOMERY => BLOCK_BLOOMERY_LIT,
        BLOCK_FIREPIT => BLOCK_FIREPIT_LIT,
        _ => return None,
    };
    // The facing survives the spark. See `burnt_out` for what happened
    // when it did not.
    Some(faced(alight, block_facing(id)))
}

/// How far a fire's heat reaches, in blocks.
///
/// What "beside a fire" means for a recipe that needs one. Three metres
/// is close enough that the player is unmistakably *at* the fire --
/// they can see it, it lights them -- and far enough that they are not
/// standing in it while they work. Shared because the client greys out
/// the recipes it cannot run and the server refuses them, and a
/// disagreement between the two is a menu row that lies.
pub const FIRE_WORKING_RANGE: f32 = 3.0;

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

/// Is this block a tree's canopy -- leaves of any wood, the acacia's
/// included?
///
/// One list, in one place, because three parts of the game ask it and
/// they must not drift: what a nest may sit on, what a dropped item
/// falls *through* (see `server::logic::items`), and what counts as
/// wood for the sake of a bird looking for somewhere to perch.
///
/// A whole cube that a player may stand on -- the answer to "is it
/// solid" is unchanged and this does not touch it. What it says is only
/// "this is foliage rather than timber or ground".
/// Leaves of any kind, canopy or undergrowth.
///
/// **Wider than `is_canopy` on purpose.** What a nest may sit in is a
/// *tree* (a bush is a metre tall and a nest in one is a nest a fox
/// eats); what a dropped apple falls through is anything leafy, bush
/// included -- an apple that came to rest on top of a bush would be the
/// same complaint that started this, one block lower.
#[inline]
pub fn is_leafy(id: BlockId) -> bool {
    is_canopy(id) || block_kind(id) == BLOCK_BUSH_LEAVES
}

#[inline]
pub fn is_canopy(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_LEAVES
            | BLOCK_BIRCH_LEAVES
            | BLOCK_FIR_NEEDLES
            | BLOCK_SAXAUL_LEAVES
            | BLOCK_PINE_NEEDLES
            | BLOCK_WILLOW_LEAVES
            | BLOCK_APPLE_LEAVES
            | BLOCK_APPLE_LEAVES_FRUIT
            | BLOCK_ACACIA_LEAVES
            | BLOCK_MAPLE_LEAVES
            | BLOCK_PALM_FRONDS
            | BLOCK_PALM_COCONUTS
    )
}

/// A cell of a palm's crown: fronds, picked or not, or a bunch of coconuts.
///
/// **Canopy, and not a canopy anything else grows on.** A frond is a leaf a
/// player picks and felling clears, which is what `is_canopy` answers; it is
/// not a bough moss hangs under. `WorldGen::place_swamp_growth` asked only
/// `is_canopy`, and a palm whose fronds reached over a swamp column came out
/// of the generator with grey strands hanging off its leaves -- the "лишай
/// на пальмах" a player photographed on a tropical beach.
#[inline]
pub fn is_palm_crown(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_PALM_FRONDS | BLOCK_PALM_COCONUTS)
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
/// Which neighbouring cell holds this block up, as an offset.
///
/// **Everything in this world is held up from below, and one thing is
/// not.** Every support rule here used to say "the cell under it", in
/// four places -- the generator, placing, mining and the collapse pass
/// -- and a bracket fungus grows out of the *side* of a trunk. It stood
/// on top of a fallen log for a version rather than have a second kind
/// of support, and that was the wrong trade: what it bought was four
/// unchanged call sites, and what it cost was the one block in the game
/// whose whole point is that you find it on a tree.
///
/// So there is no second kind of support, there is a *direction*. Every
/// caller asks this where it used to subtract one from `y`, and the
/// rules it feeds -- `needs_support` and `can_grow_on` -- did not
/// change at all.
///
/// The direction is the block's facing turned the way `mesh::push_box`
/// turns a model, and the two have to agree or the shelf grows out of
/// one wall while being held by another. `mesh` has the test that says
/// they do.
pub fn support_at(id: BlockId) -> (i32, i32, i32) {
    // **Hanging moss is held from above**, the second direction there is.
    // What it hangs from is the leaf over it, so that is the cell whose
    // loss takes it down -- asked here like every other support, so the
    // generator, placing, breaking and the collapse pass all agree.
    // ...and so is a stalactite, by the rock of the roof it grew out of.
    if block_kind(id) == BLOCK_HANGING_MOSS || block_kind(id) == BLOCK_STALACTITE {
        return (0, 1, 0);
    }
    // **A stake driven into a wall is held by that wall**, and one stood on
    // the ground by the ground: the same block, and the bit that tells them
    // apart is what this reads (`STAKE_UPRIGHT`). Without the first case a
    // stake in a wall was held by the air under it and fell the moment it
    // was put there.
    if is_stake(id) {
        if stake_is_upright(id) {
            return (0, -1, 0);
        }
        return wall_behind(id);
    }
    if block_kind(id) != BLOCK_BRACKET_FUNGUS {
        return (0, -1, 0);
    }
    wall_behind(id)
}

/// The cell behind a block built against a wall: -z at no turn, turned with
/// its facing. What both the bracket fungus and a driven stake hang from.
fn wall_behind(id: BlockId) -> (i32, i32, i32) {
    let (mut dx, mut dz) = (0i32, -1i32);
    for _ in 0..block_facing(id).quarters() {
        let turned = (dz, -dx);
        dx = turned.0;
        dz = turned.1;
    }
    (dx, 0, dz)
}

pub fn needs_support(id: BlockId) -> bool {
    is_cross(id)
        || is_flat(id)
        // A *drift*, not merely something short. A campfire is half a
        // block tall and stays where it is put, the way the chest it
        // sits beside does; a spadeful of earth with nothing under it is
        // not a floating spadeful of earth.
        || is_loose_layer(id)
        || crate::blocks::definition(id).propped
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
    if is_loose_layer(plant) {
        return has_full_top(ground);
    }
    // **A pot of earth is ground for what a pot holds** (`BLOCK_PLANTER`):
    // asked before the soils below, because the pot is not any of them and
    // every one of those rules would say no.
    if block_kind(ground) == BLOCK_PLANTER {
        return grows_in_a_pot(plant);
    }
    // **A turf lip is a floor for what grows in turf** (`dig::is_turf_lip`):
    // the meadow's own tufts and flowers do not stop a block short of every
    // rise. Not for a layer (above: a drift over a lip would hang over the
    // quarter it does not fill) and not for a torch or a chest, which ask
    // `has_full_top` themselves -- a lip is ground for roots, not for
    // things set down. A plant is drawn, aimed at and cracked on the lip's
    // real top (`stand_drop`) -- it was drawn from the floor of its own cell
    // for a while after this said so, a quarter of a block over the grass.
    // ...and **a coating lies on any level top** (`coating_rests_at`): snow,
    // ash and fallen leaves on a lip of any soil and on a floor dug down, at
    // its real height. A sheet with no thickness has no foot to hang over a
    // gap, which is the only reason a torch is refused one.
    // ...and so does **every flat thing**, a pebble, a flint or a stick as
    // much as a sheet of snow: it lies on the top and has no foot either, and
    // it is drawn there (`rest_drop`). Asked of the coatings only, the
    // stones of a scree and the sticks of a wood kept the whole block under
    // them on every sand, gravel and earth lip the generator laid, a step
    // left standing round each (`worldgen::lips`).
    let full_floor = has_full_top(ground)
        || crate::dig::is_turf_lip(ground)
        || (is_flat(plant) && coating_rests_at(ground).is_some());
    // The whole id, for the one rule that asks what exactly is underneath:
    // the upper half of a tall plant stands on its own grown lower half.
    let whole = ground;
    // **The ground as what it stands in for** (`ground::as_common`): a cactus
    // roots in quartzite sand as in sand, a fern in loam as in dirt. The ten
    // grasses ask the ground itself, because which soil it is *is* their
    // rule (`ground::grows_on`).
    if crate::ground::is_grass(plant) {
        return full_floor && crate::ground::grows_on(plant, whole);
    }
    let ground = crate::ground::as_common(ground);
    match block_kind(plant) {
        // **The upper half of a tall plant is held by its lower half and by
        // nothing else** -- not a shoot, not another plant's top, not the
        // same kind of plant grown beside it one cell down. Asked first, so
        // the soil rules below are only ever the lower half's.
        BLOCK_FIREWEED | BLOCK_CATTAIL | BLOCK_NETTLE | BLOCK_BRACKEN | BLOCK_ARUNDO if plant & PLANT_TOP != 0 => {
            whole == block_kind(plant)
        }
        // Fireweed, nettle and bracken root in turf and bare earth, as the
        // meadow's own plants do.
        BLOCK_FIREWEED | BLOCK_NETTLE | BLOCK_BRACKEN => matches!(ground, BLOCK_GRASS | BLOCK_DIRT) && full_floor,
        // A cattail stands in the reed's ground: the mud, clay and sand at the
        // edge of fresh water, and the bank behind it.
        BLOCK_CATTAIL => {
            matches!(ground, BLOCK_MUD | BLOCK_CLAY | BLOCK_SAND | BLOCK_DIRT | BLOCK_GRASS) && full_floor
        }
        // ...and giant reed on the same banks, and on the hot country's own
        // ground behind them: dry turf and sandy soil.
        BLOCK_ARUNDO => {
            matches!(
                ground,
                BLOCK_MUD | BLOCK_CLAY | BLOCK_SAND | BLOCK_DIRT | BLOCK_GRASS | BLOCK_DRY_TURF | BLOCK_SANDY_SOIL
            ) && full_floor
        }
        // The two berries and the fern are forest-floor plants: turf and
        // bare earth. The fern takes a swamp's mud as well.
        BLOCK_BILBERRY | BLOCK_BILBERRY_BARE | BLOCK_STRAWBERRY | BLOCK_STRAWBERRY_BARE => {
            matches!(ground, BLOCK_GRASS | BLOCK_DIRT) && full_floor
        }
        BLOCK_FERN => matches!(ground, BLOCK_GRASS | BLOCK_DIRT | BLOCK_MUD) && full_floor,
        // Plantain is the plant of trodden ground, and dry turf is trodden
        // ground too.
        BLOCK_PLANTAIN => matches!(ground, BLOCK_GRASS | BLOCK_DIRT | BLOCK_DRY_TURF) && full_floor,
        // **A sundew wants a bog and only a bog**: bare peat, the turf over
        // it, and mud. Not dirt -- a sundew on a garden bed is a sundew
        // somebody carried, and the whole of what it tells a player is that
        // the ground under it is sodden.
        BLOCK_SUNDEW => matches!(ground, BLOCK_PEAT | BLOCK_GRASS | BLOCK_MUD) && full_floor,
        BLOCK_TALL_GRASS => matches!(ground, BLOCK_GRASS | BLOCK_DIRT) && full_floor,
        // **Dry grass wants dry ground**: the savanna's sandy soil, and bare
        // dirt, which is dry ground too. Not turf -- on turf the grass is
        // the green kind, and a dry tuft on a lawn is a tuft in the wrong
        // place.
        BLOCK_DRY_GRASS => matches!(ground, BLOCK_SANDY_SOIL | BLOCK_DIRT | BLOCK_DRY_TURF) && full_floor,
        BLOCK_CACTUS => matches!(ground, BLOCK_SAND | BLOCK_CACTUS) && full_floor,
        // Turf and bare soil, like the tuft: a bush and a flower are
        // rooted in the same ground a meadow is.
        BLOCK_BERRY_BUSH | BLOCK_BARE_BUSH | BLOCK_FLOWER => {
            matches!(ground, BLOCK_GRASS | BLOCK_DIRT | BLOCK_DRY_TURF) && full_floor
        }
        // **A crop wants tilled earth and nothing else.** This one rule
        // is the whole reason the hoe exists: without it a seed would go
        // in anywhere a tuft of grass does, and a field would be
        // something you scatter rather than something you make.
        //
        // Every stage of every crop, and the dead stalks frost leaves
        // of one: a withered crop is still standing in the furrow it
        // grew in, and one that could be left on turf would be a crop
        // somebody had carried.
        BLOCK_SEEDS
        | BLOCK_WHEAT
        | BLOCK_WHEAT_RIPE
        | BLOCK_COTTON_SEEDS
        | BLOCK_COTTON_PLANT
        | BLOCK_COTTON_RIPE
        | BLOCK_MILLET
        | BLOCK_MILLET_PLANT
        | BLOCK_MILLET_RIPE
        | BLOCK_WITHERED_CROP => ground == BLOCK_FARMLAND && full_floor,
        // **A standing torch is driven into flat ground**: a whole top,
        // so not a drift, a slab or a campfire. The player's words --
        // "stand only on flat ground" -- and the reason under them: a
        // pole two cells tall on half a block of snow is a pole that has
        // nothing to stand in. Its top stands on its own pole and nothing
        // else, the tall plant's rule (`wildfire::standing_torch_partner`).
        BLOCK_STANDING_TORCH => full_floor && !crate::wildfire::is_standing_torch(whole),
        BLOCK_STANDING_TORCH_LIT | BLOCK_STANDING_TORCH_OUT => ground == BLOCK_STANDING_TORCH,
        // **A door is hung on a floor**, the pole's rule: a whole top under
        // it, and not the top of another door -- a door standing on a door
        // is a doorway four cells tall that nothing but a ladder uses. Its
        // top half stands on its own lower half, open or shut the same way,
        // and on nothing else (`door_partner`).
        BLOCK_DOOR => full_floor && !is_door(whole),
        BLOCK_DOOR_TOP => door_partner((0, 1, 0), plant) == Some(((0, 0, 0), whole)),
        // A mushroom wants what has rotted rather than what is growing:
        // bare earth, cave floor, gravel, the ash of a burnt wood. Not
        // turf -- a living skin of grass is exactly what a fungus is
        // not found on, and the rule is also what keeps mushrooms in
        // the dark places they are worth walking into.
        // ...and the toadstool wants exactly what the mushroom wants,
        // which is the point of it: the two grow in the same places, so
        // telling them apart is a thing a player does with their eyes
        // rather than with a map.
        // ...and any of the three rocks under the soil: a cave floor is
        // limestone in the lowlands and sandstone under the sea shelf now
        // (`worldgen::stratum`), and a fungus that grew on plain stone
        // has no reason to refuse pale stone.
        BLOCK_MUSHROOM | BLOCK_TOADSTOOL => {
            matches!(
                ground,
                BLOCK_DIRT
                    | BLOCK_STONE
                    | BLOCK_COBBLESTONE
                    | BLOCK_GRAVEL
                    | BLOCK_ASH
                    | BLOCK_SANDSTONE
                    | BLOCK_LIMESTONE
                    | BLOCK_GRANITE
            ) && full_floor
        }
        // **The bracket wants dead wood and only dead wood.** Not soil,
        // not stone, not the cave floor its cousins like -- a fungus
        // that ate rock would be a fungus nobody has to find a fallen
        // tree for, and finding the fallen tree is the whole of what
        // this block is worth. Both timbers, because a birch rots the
        // same way an oak does; planks as well, because a plank is a
        // log somebody sawed and a shed roof in a wet wood grows the
        // same shelf a deadfall does. See `BLOCK_BRACKET_FUNGUS` for
        // why it stands *on* the log rather than against its side.
        // **`ground` here is the cell `support_at` names, and for this
        // one that is beside it rather than beneath it.** No
        // `full_floor`: nothing stands on a bracket, it hangs off bark,
        // and asking a wall for a full *top* would refuse every trunk
        // whose neighbour above happens to be a branch.
        BLOCK_BRACKET_FUNGUS => crate::wood::is_log(ground) || crate::wood::is_planks(ground),
        // **A stake wants something solid, floor or wall.** Upright it is the
        // floor's full top, as a post needs; driven in it is any solid cell,
        // because a wall is what it is driven into and a wall has no "top".
        BLOCK_STAKE => {
            if stake_is_upright(plant) {
                full_floor
            } else {
                matches!(crate::blocks::definition(ground).shape, crate::blocks::Shape::Cube) && !is_liquid(ground)
            }
        }
        // **A lily pad lies on still water and on nothing else.** `ground`
        // is the cell under it, and that cell is water of any depth: a pad
        // on mud is a leaf somebody dropped, and one left on the bed of a
        // pool that drained is a pool that is not there any more.
        BLOCK_LILY_PAD => ground == BLOCK_WATER,
        // **Moss hangs from a crown or a bough.** `ground` is the cell
        // *above* it here (`support_at`), and a tuft of moss under a roof or
        // a cloud is the grass-in-the-sky mistake upside down.
        BLOCK_HANGING_MOSS => is_canopy(ground) || matches!(ground, BLOCK_LOG | BLOCK_HANGING_MOSS) || is_branch(ground),
        // **Dripstone grows out of rock and out of nothing else**, the floor
        // under a stalagmite and the roof over a stalactite (`support_at`).
        // No `full_floor`: every rock is a whole cube, and the one question
        // worth asking is what it is made of -- see `dripstone::grows_from`.
        // ...**or more of the same spike**, which is what makes a column of
        // two to four cells stand up (`dripstone::stands_on`): without it
        // the second cell of every column was a block held by nothing and
        // the support pass took it off again the moment the chunk loaded.
        BLOCK_STALAGMITE | BLOCK_STALACTITE => crate::dripstone::stands_on(plant, ground),
        // **Wild wheat wants the meadow, not the field.** Turf and bare
        // earth, like every other wild plant -- emphatically *not*
        // tilled earth, because a stand of wild cereal that could be
        // planted would be seed you never had to find. What a player
        // plants is `BLOCK_SEEDS`, and the seed is what this gives.
        // ...and wild cotton on the same terms, for the same reason: what
        // a player sows is `BLOCK_COTTON_SEEDS`.
        // ...and on the savanna's dry turf, which is where the steppe's
        // cereal and all of the wild cotton grow.
        BLOCK_WILD_WHEAT | BLOCK_WILD_COTTON | BLOCK_WILD_MILLET => {
            matches!(ground, BLOCK_GRASS | BLOCK_DIRT | BLOCK_DRY_TURF) && full_floor
        }
        // A nest sits in a canopy: on a branch, which in this world is
        // leaves, or in the fork of a trunk. Not on the ground -- a
        // nest a player could put down anywhere would be a nest they
        // farm rather than find, and the whole of what it is worth is
        // that it is up a tree somebody has to remember.
        BLOCK_NEST | BLOCK_NEST_EGGS => {
            is_canopy(ground) || crate::wood::is_log(ground)
        }
        // Roots want living ground: the turf of a meadow or the bare
        // earth of a forest floor. Not stone and not sand -- a root
        // needs something to have grown in.
        BLOCK_ROOTS => matches!(ground, BLOCK_GRASS | BLOCK_DIRT) && full_floor,
        // Reeds stand in the mud a river leaves. Clay and sand are the
        // riverbank materials, and dirt is the bank behind them.
        // ...and mud, which is the ground a swamp's reed beds stand in.
        BLOCK_REEDS => matches!(ground, BLOCK_CLAY | BLOCK_SAND | BLOCK_DIRT | BLOCK_GRASS | BLOCK_MUD) && full_floor,
        // **Kelp holds on to anything that does not move**: sand, silt,
        // gravel, clay and bare rock of every kind -- a holdfast grips a
        // stone as happily as a sand bar. And on kelp, which is the whole
        // plant: a stem is a column, and the stem below is what holds the
        // one above up. No `full_floor` for that case, because kelp is not
        // collidable and so has no top a foot could stand on.
        BLOCK_KELP | BLOCK_KELP_TOP => {
            ground == BLOCK_KELP
                || (matches!(
                    ground,
                    BLOCK_SAND
                        | BLOCK_DIRT
                        | BLOCK_GRAVEL
                        | BLOCK_CLAY
                        | BLOCK_STONE
                        | BLOCK_COBBLESTONE
                        | BLOCK_SANDSTONE
                        | BLOCK_LIMESTONE
                        | BLOCK_GRANITE
                ) && full_floor)
        }
        // Seagrass roots in what a meadow roots in, under water: sand and
        // silt, and the clay of a quiet bay. Not rock and not gravel -- a
        // meadow on a stone floor is a lawn laid on a pavement.
        BLOCK_SEAGRASS => matches!(ground, BLOCK_SAND | BLOCK_DIRT | BLOCK_CLAY) && full_floor,
        // A coral grows on a reef, and a reef is coral and the rock it
        // started on. Not sand: a fan on a sand bar is a fan a wave put
        // there.
        BLOCK_SEA_FAN | BLOCK_STAGHORN_CORAL => {
            matches!(
                ground,
                BLOCK_BRAIN_CORAL | BLOCK_FIRE_CORAL | BLOCK_STONE | BLOCK_COBBLESTONE | BLOCK_LIMESTONE
            ) && full_floor
        }
        // A stick lies wherever it is dropped, and so does a stone --
        // as long as there is a whole surface under it. A pebble on a
        // dusting of snow would sit an eighth of a block in the air,
        // and the eye finds that instantly on flat ground.
        // ...and a nodule of native copper, which weathers out of rock
        // the way flint does and lies where flint lies. *Where it
        // generates* is the world generator's business (see
        // `scatter_ground_cover`); what this says is only that a thing
        // lying on the ground needs a whole floor under it.
        // ...and a shell on the sea bed, for the pebble's reason.
        // ...and a flake of flint, which lies where the nodule it was
        // struck off lies.
        // ...and a pebble of stream tin, which is a pebble.
        // ...and a starfish, which lies on the bed as the shell does.
        BLOCK_STICK | BLOCK_PEBBLE | BLOCK_FLINT | BLOCK_FLINT_FLAKE | BLOCK_ASH | BLOCK_NATIVE_COPPER
        | BLOCK_SHELL | BLOCK_STREAM_TIN | BLOCK_STARFISH => full_floor,
        // ...and anything a hand sets down, on any flat top at its real
        // height (`set_down_drop`): a lip, a slab, a step's tread. Never the
        // top of a fence post, a lattice or a riser, which is no top at all.
        // The whole id, because a lip of loam is not loam's whole block.
        BLOCK_SET_DOWN => set_down_drop(whole).is_some(),
        // Fallen leaves on the earth of a wood's floor: turf, bare earth and
        // a swamp's mud, never stone or sand, where no crown stands.
        BLOCK_LEAF_LITTER => matches!(ground, BLOCK_GRASS | BLOCK_DIRT | BLOCK_MUD) && full_floor,
        // **Anything else lying flat lies on a whole floor**, the pebble's
        // rule for the pebble's reason: a pebble of every other rock, a
        // snare, a skin of snow. They fell through to "anywhere" below, and
        // a granite pebble on a slab lay half a block over it.
        _ if is_flat(plant) => full_floor,
        // **A lean-to holds itself up**: its upper cells stand on its lower
        // ones, which are thatch and not a floor, and its ground row is asked
        // for a whole floor over all nine cells where it is put down
        // (`net::connection`), which is a question about the hut and not
        // about one cell of it.
        _ if crate::lean_to::is_lean_to(plant) => true,
        // **And a thing that stands -- a chair, a barrel, a bed, a bench --
        // does not stand on a top that is not whole.** Drawn from the floor
        // of its own cell, on a slab, a step, a campfire or a bitten block
        // it hung over the air where that top is not. What stays is what
        // was always allowed: a thing put against a wall with nothing
        // under it, which is plainly where somebody put it -- it is the
        // half-supported one that reads as the world being wrong.
        _ => full_floor || !is_collidable(ground),
    }
}

/// Maximum light level, matching the 4 bits per light channel packed into
/// each vertex on the client (see `primitive_client::mesh`).
pub const MAX_LIGHT: u8 = 15;

#[inline]
pub fn is_air(id: BlockId) -> bool {
    block_kind(id) == BLOCK_AIR
}

/// Collision.
///
/// **The note that stood here said water is deliberately still solid,
/// "there's no swimming yet" -- and there is.** Swimming, wading, the
/// depth-aware collider in `geometry`, drowning and the underwater fog
/// all arrived and none of them updated this line, so the one comment a
/// reader would check before making water passable told them it had
/// been tried and rejected. It had not; it had been done.
#[inline]
pub fn is_collidable(id: BlockId) -> bool {
    // **Foliage is pushed through, not walked into.** A canopy was a
    // wall: a player at the edge of a wood could stand on a leaf a
    // storey up and walk across the treetops, and getting *into* a
    // thicket meant breaking it. Neither is what a bush is. It is not
    // free either -- see `surface_drag`, where leaves cost more than
    // half your speed, so a wood is still something you go round when
    // you are in a hurry.
    //
    // What it costs, said plainly because a player will meet it: a
    // canopy no longer holds you up, so falling into a tree is falling
    // through it.
    // **A twig is not collidable here, and it is still walked into.** A
    // sapling a few sixteenths thick that stopped a player like a wall would
    // make every young wood a maze of invisible pillars -- which is why this
    // line was all a twig's collision, and why it was none. What a *body*
    // meets is neither piece's row now: a twig and a bough are both walked
    // into at their wood, the post and arms `branch::wood_boxes` gives
    // (`geometry::for_each_block_box`). This line is what the *cell* is to
    // the rules that ask it -- a floor to lay a stone on, a support for a
    // stack -- and there a twig stays air and a bough timber, as they were.
    // **A drowned bough is water that is walked into**: liquid by its row so
    // the pool round it is one body of water (`BLOCK_DROWNED_BOUGH`), and a
    // standing trunk by every rule about bumping into one.
    !is_air(id)
        && !is_leafy(id)
        && !is_twig(id)
        // **A spike of dripstone is a twig in this respect**: a body meets
        // its box (`dripstone::body`, through `geometry::block_box`), and
        // the cell is not a floor to lay anything on or a full cube of
        // height for the rules that read one -- a stalactite's box is at the
        // *top* of its cell, which a height from the floor cannot say.
        && !crate::dripstone::is_dripstone(id)
        // **A thing set down by hand is walked through** (`BLOCK_SET_DOWN`):
        // an eighth of a cell of collider under a knife lifted a player
        // stepping on it, and a floor laid out with tools was a floor of
        // steps. Nor is it a top to put anything on.
        && !is_set_down(id)
        && (block_kind(id) == BLOCK_DROWNED_BOUGH || {
            let def = crate::blocks::definition(id);
            def.matter != crate::blocks::Matter::Liquid && matches!(def.shape, crate::blocks::Shape::Cube)
        })
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
    // ...except where the water has something standing in it. Kelp is
    // liquid by its row (see `BLOCK_KELP`), and a ray that went through
    // it the way it goes through the sea is a forest nobody can cut.
    !is_air(id) && (!is_liquid(id) || stands_in_water(id)) && !is_item(id)
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
    // **Not drowned wood**, which is liquid by its row: this is the list the
    // fluid simulation reads as "water may be here and may move", and a snag
    // on that list was a cell the first ripple of a pool wrote plain water
    // over, wood and all. It holds its water and gives none of it away.
    is_air(id) || (is_liquid(id) && !is_branch(id)) || is_cross(id) || is_flat(id)
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
    // ...and it has to *fill* the cell. A half-height block that
    // reported itself opaque would have its own top face culled by the
    // air above it -- a hole straight down into the middle of a campfire
    // -- and would hide the faces of everything it stands against.
    // ...across as well as up: a shut door stops light and is three
    // sixteenths of its cell, and called opaque it would hide the wall
    // faces beside it and the floor under it (`collision_depth`).
    def.shape == crate::blocks::Shape::Cube
        && def.opacity >= MAX_LIGHT
        && !is_partial(id)
        && collision_depth(id).is_none()
        // ...and a block with a bite out of it does not fill its cell
        // either, for the layer's reason exactly: called opaque, the face
        // the digger has just opened would be culled against the air in
        // front of it and the player would be looking into a hole with a
        // whole block still drawn across it. Light comes in through a bite
        // for the same reason it comes in through a drift of snow.
        // ...and a wall part way up is a bite the other way up (`build`).
        && !crate::dig::is_part(id)
        // ...and a step is a tread and a riser, with the back half of its
        // cell over the tread empty. Its row is a cube with a wall's
        // opacity (so a roof of steps keeps the room under it dark), and
        // that alone answered yes here: the mesher then culled every face
        // of every block a stair was set against, and the half the riser
        // does not reach was a hole straight into the block -- "если
        // поставить с блоком, то грань блока будет пустая". The light is
        // unchanged by this: a step's opacity still stops all of it.
        && !is_step(id)
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
    // **A shut door stops light and an open one lets it all through.**
    // Decided against the furniture's rule -- a bed and a chair take
    // nothing out, because "furniture does not wall a room off from its
    // own light" -- because walling a room off is what a door is *for*: a
    // house whose shut door let the noon sun into it a cell at a time would
    // be a house with a doorway in it, and the torch inside it would light
    // the path outside for anyone looking.
    //
    // What it costs is that light is kept per cell and a door is three
    // sixteenths of one: the cell a shut door stands in is dark. The door
    // is drawn with each broad face lit from the cell it looks into
    // (`mesh::door_block`); it used to be drawn whole from the brighter
    // side, so the inside of a dark hut's door at noon was lit as if the sun
    // were on it. Rejected: *a door that lets light through shut*, which is
    // the leak above, and *light kept per face* in the light engine, which
    // is a second light engine for one block.
    if is_door(id) {
        return if door_is_open(id) { 0 } else { MAX_LIGHT };
    }
    // **A wall part way up lets the light over it**, as a bite lets it in:
    // the row is the finished wall's, and a cell dark to the light engine
    // would draw its three courses unlit (they take their light from their
    // own cell and the cells round it).
    if crate::build::stage_box(id).is_some() {
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
    // **A bite is asked about before anything else that reads these
    // bits**, because a bite is written over all of them: the flag is the
    // same sixteenth bit a stool spends on its wood and the field under it
    // the same two, and which of the two an id means is decided by its
    // kind (`dig::digs_in_slices`). Asked here rather than waved through,
    // because the server writes a new id into the world on every swing and
    // a client is free to invent one: two of the eight faces name nothing,
    // and the flag on a block nobody quarries is a claim.
    // ...and the turf's lip, which the generator lays (`dig::is_turf_lip`).
    if id & crate::dig::DUG != 0 && crate::dig::may_be_bitten(kind) {
        return crate::dig::bite(id).is_some() && is_known_block(kind);
    }
    // **A handful carries its material and a wall its courses** in the same
    // bits a wood and a bite use, and which a kind means is the kind's
    // (`build`). Asked here for the bite's reason: every stage is written by
    // the server and every one of them crosses the wire.
    if crate::build::is_handful(kind) {
        return crate::build::block_of_handful(id).is_some();
    }
    if crate::build::is_staged(kind) {
        return crate::build::is_valid(id);
    }
    // **A wet thing spends the sixteenth bit on the water in it** (`wet`),
    // on the kinds that get wet and nowhere else -- none of which spends
    // that bit on anything. Asked before the wood below, whose bits it
    // shares, and answered for the thing dry. Wet flour ages where dry
    // flour does not (`food::rot_per_step`), so any stage of it is flour.
    if id & crate::wet::WET != 0 && crate::wet::gets_wet(kind) {
        let dry = id & !crate::wet::WET;
        return if kind == BLOCK_FLOUR { dry & !VARIANT_MASK == kind } else { is_known_block(dry) };
    }
    // **Every one of a rack's six bits means something**: a facing, which
    // of its four cells, and two bits of what hangs (`RACK_TOP`). Asked
    // before the wood, whose bits a rack spends on its goods and its column.
    if kind == BLOCK_DRYING_RACK {
        return true;
    }
    // ...and a lean-to's four spare bits say which of its fifteen cells this
    // is (`lean_to::PART_MASK`), the two facing bits which way it looks. The
    // sixteenth number is no part.
    if kind == BLOCK_LEAN_TO {
        return crate::lean_to::written_offset(id).is_some();
    }
    // **A wood only on furniture, and only a wood there is.** Before kinds
    // were ten bits the wood bits were part of the kind and an id with them
    // set was a kind nobody defined; now they are stripped by `block_kind`,
    // and without this a stone with them set would be four ids for a stone.
    if id & WOOD_MASK != 0 {
        // ...and a hide frame one of them on its skin having dried
        // (`HIDE_CURED`), which is only ever said of a skin that is there.
        if kind == BLOCK_HIDE_FRAME {
            return id & WOOD_MASK == HIDE_CURED && id & RACK_LOADED != 0 && is_known_block(id & !HIDE_CURED);
        }
        // **A rack spends the same bits on which of its four cells this is
        // and what hangs on the column** (`RACK_FAR`, `RACK_GOODS_MASK`):
        // the two fields are the same three spare bits, and which one an id
        // means is decided by its kind. Without this the anti-cheat refused
        // every cell of a two-by-two rack, which is every rack put down
        // since it grew.
        if kind == BLOCK_DRYING_RACK {
            return is_known_block(rack_shape(id) & !RACK_FAR);
        }
        // ...and a hive spends two of the same bits on which trunk it is
        // stuck to (`hive_side`), all four of which are a side.
        if crate::bees::is_hive(kind) {
            return id & WOOD_HIGH_BIT == 0 && is_known_block(id & !WOOD_MASK);
        }
        // ...and a log spends the two low ones on how green it still is
        // (`wood::greenness`), all four of which are a stage.
        if crate::wood::is_log(kind) {
            return id & WOOD_HIGH_BIT == 0 && is_known_block(id & !WOOD_MASK);
        }
        if !carries_wood(kind) || furniture_wood(id) >= crate::wood::WOODS.len() {
            return false;
        }
        return is_known_block(id & !WOOD_MASK);
    }
    // **Moss is the third bit on a trunk or a stone** (`ground::MOSSY`), and
    // asked first: under it a log is still its axis and a rock still has no
    // variant at all. Without this the first mossy trunk the generator wrote
    // would be an id the anti-cheat and a save both call invented.
    let id = if crate::ground::is_mossy(id) { id & !crate::ground::MOSSY } else { id };
    if is_orientable(kind) {
        // An axis of 3 is the fourth value of a two-bit field and means
        // nothing, and the bit above the axis is not part of an
        // orientation at all.
        return (id & VARIANT_MASK) >> VARIANT_SHIFT <= 2;
    }
    // **A block with a front spends the same field on a facing**, and
    // this used to say no to three quarters of them. `Facing` has four
    // values and `faced()` writes them into the low two bits, so a chest
    // or a kiln put down by anyone not happening to look north came out
    // of the client as an id this called invented -- and the anti-cheat
    // refuses an invented id, at three points of a twelve-point kick.
    // Turning round four times while building a camp was a disconnect.
    if has_front(kind) {
        let spare = id & VARIANT_MASK & !ORIENTATION_MASK;
        // The third bit is the rack's "there is a skin on it" flag and a
        // bed's "this is the head half", and it means nothing on anything
        // else -- see `RACK_LOADED` and `BED_HEAD`.
        // ...and a door's "it is open" (`DOOR_OPEN`).
        return spare == 0
            || (spare == STAKE_UPRIGHT && kind == BLOCK_STAKE)
            || (spare == PROP_CENTRED && kind == BLOCK_PROP)
            || (spare == RACK_LOADED && matches!(kind, BLOCK_DRYING_RACK | BLOCK_HIDE_FRAME))
            || (spare == BED_HEAD && is_bed(kind))
            || (spare == DOOR_OPEN && is_door(kind));
    }
    // **A piece of branch spends the field on its width**, and only as
    // many steps as its kind has: three for a twig, five for a bough.
    // The rest of the field would read back as the widest piece (see
    // `branch_width`), which is a second id for one block.
    // ...and a birch's pieces spend the steps above the oak's on the same
    // widths in its own bark (`birch_branch`): a twig's six steps, a bough's
    // eight. A palm's trunk has only the oak's five.
    if is_branch(kind) {
        let step = (id & VARIANT_MASK) >> VARIANT_SHIFT;
        return step
            <= match kind {
                BLOCK_TWIG | BLOCK_DROWNED_TWIG => 5,
                BLOCK_BOUGH | BLOCK_DROWNED_BOUGH => 7,
                // A bark of its own: the oak's three twig steps.
                _ if is_twig(kind) => 2,
                _ => 4,
            };
    }
    // ...and dripstone spends it on its size, four of them -- three tips
    // and the shaft a column is made of (`dripstone::SIZES`); the rest
    // would read back as the largest.
    if crate::dripstone::is_dripstone(kind) {
        return (id & VARIANT_MASK) >> VARIANT_SHIFT < BlockId::from(crate::dripstone::SIZES);
    }
    // ...and a fish trap spends it on how many fish are in it, which stops
    // at what a trap holds (`trap_catch`); a fifth fish is a claim.
    // ...and a barrel of grain spends it on its level, which is never
    // nought: a barrel with no grain in it is the empty barrel's own id
    // (`barrel_of_goods`), and a second id for the same empty staves is a
    // second kind of empty barrel that does not stack with the first.
    if barrel_goods(kind | (1 << VARIANT_SHIFT)).is_some() {
        return barrel_goods(id).is_some();
    }
    if kind == BLOCK_FISH_TRAP {
        return trap_catch(id) <= crate::fishing::TRAP_HOLDS;
    }
    // ...and raw pottery spends it on how dry it is (`clay::Dryness`),
    // three stages; a fourth is a claim.
    if crate::clay::is_raw_pottery(kind) {
        return crate::clay::is_valid_variant(id);
    }
    // ...and a wild hive spends it on its honey, which stops at what a hive
    // holds (`bees::HIVE_FULL`). Asked here rather than waved through by
    // `may_carry_variant`, which would let all seven values past: the server
    // writes the emptied comb into the world on every raid, and a fifth comb
    // is a claim.
    if kind == BLOCK_WILD_HIVE {
        return (id & VARIANT_MASK) >> VARIANT_SHIFT <= BlockId::from(crate::bees::HIVE_FULL);
    }
    // ...and a mussel bed on how many mussels are left on it, for the hive's
    // reason: the server writes a new count into the world every time a hand
    // comes off the rock, and a fifth mussel is a claim.
    if kind == BLOCK_MUSSEL_BED {
        return crate::shore::is_valid_bed(id);
    }
    // ...and the ten old answers' three counts, each held to what it counts:
    // a young cheese's or a must's stage (`ferment::STAGES`), how long a hare
    // has hung (`snare::ROBBED_AFTER`), how far a pan has dried
    // (`saltpan::STAGES`, which is all eight). The server writes every one of
    // them on the rot clock, into packs and into the world.
    if crate::ferment::is_working(kind) {
        return (id & VARIANT_MASK) >> VARIANT_SHIFT < BlockId::from(crate::ferment::STAGES);
    }
    if kind == BLOCK_SNARE_CAUGHT {
        return (id & VARIANT_MASK) >> VARIANT_SHIFT < BlockId::from(crate::snare::ROBBED_AFTER);
    }
    if kind == BLOCK_SALT_PAN_BRINE {
        return (id & VARIANT_MASK) >> VARIANT_SHIFT < BlockId::from(crate::saltpan::STAGES);
    }
    // ...and a tall plant's cell is its lower half, its upper half or a
    // shoot, and nothing else (`PLANT_TOP`, `PLANT_YOUNG`).
    if is_tall_plant(kind) {
        let spare = id & VARIANT_MASK;
        return spare == 0 || spare == PLANT_TOP || spare == PLANT_YOUNG;
    }
    // **An apple leaf has one variant, the cell its fruit was picked
    // from**, and the other six values mean nothing. Not on
    // `may_carry_variant`, which would wave all seven through: the server
    // writes the picked leaf into the world on every pick, and without
    // this it is an id the anti-cheat and a save both call invented.
    if kind == BLOCK_APPLE_LEAVES {
        return id == BLOCK_APPLE_LEAVES || id == BLOCK_APPLE_LEAVES_PICKED;
    }
    // ...and a palm frond, on the same terms for the same reason.
    if kind == BLOCK_PALM_FRONDS {
        return id == BLOCK_PALM_FRONDS || id == BLOCK_PALM_FRONDS_PICKED;
    }
    // Everything else: the variant field may only carry bits on the ids
    // that have a use for one. Bits anywhere else came off a socket
    // wrong, or off a save from a build that used them differently.
    // **A tool that takes an edge spends the field on how blunt it is**, and
    // an iron one on whether it is steeled as well (see `tools`). Asked here
    // rather than added to `may_carry_variant`, which would wave all three
    // bits through on every tool: a steeled copper axe is two ids for one
    // axe, and the anti-cheat has to be able to call it invented.
    if crate::tools::takes_an_edge(kind) {
        return crate::tools::is_valid_variant(id);
    }
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
    // ...and a rotten board is soft (`weathering::ROTTEN_BREAK_FACTOR`),
    // here rather than in the table because the table is by kind and rot
    // is in the variant.
    let rot = if crate::weathering::is_rotten(id) { crate::weathering::ROTTEN_BREAK_FACTOR } else { 1.0 };
    break_seconds_unscaled(id, tool).map(|seconds| seconds * work_slowdown(id) * rot)
}

/// How much longer breaking takes than the hardness table says.
///
/// **Twice, and three times for a branch** -- the player's own numbers
/// ("удаление всех блоков сделай дольше в 2 раза веток в 3"). The world was
/// coming apart in the hand: a wood was cleared in the time it took to walk
/// through it, and a shelter was not a thing you *built* so much as a thing
/// you dug in a minute, which made every material the same material. A
/// branch gets the most because it was the cheapest wood in the game by far:
/// a twig came off in a blink, so a crown was a stack of free sticks and the
/// axe was optional.
///
/// **A factor over the whole table, not a retuned table.** Every hardness in
/// `blocks` is argued against its neighbours -- clay against sand, a copper
/// pick against a bronze one -- and those arguments are about *ratios*,
/// which a single factor leaves exactly as they were. Applied in the one
/// place mining time is decided, so the client's bar, the server's rate
/// limit and the stamina bill all slow down together and never disagree.
/// The palm's trunk is a branch in every rule but this one: it is a tree's
/// trunk, and it takes a trunk's time.
#[inline]
pub fn work_slowdown(id: BlockId) -> f32 {
    // Something lying on the ground is picked up, not broken -- a pebble,
    // a stick, a flint. Doubling the time it takes to bend down for one is
    // not "harder work", it is a slower hand, and the pack filling from the
    // ground is the one thing that is supposed to feel quick.
    if matches!(crate::blocks::definition(id).shape, crate::blocks::Shape::Item | crate::blocks::Shape::Flat) {
        return 1.0;
    }
    if is_branch(id) && block_kind(id) != BLOCK_PALM_TRUNK {
        3.0
    } else {
        2.0
    }
}

/// `break_seconds_with` before `work_slowdown`: the hardness table as the
/// table has it.
fn break_seconds_unscaled(id: BlockId, tool: Option<BlockId>) -> Option<f32> {
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
            return Some(felled / crate::tools::speed(tier, tool));
        }
    }
    let hardness = def.hardness?;
    if tier < def.needs {
        return None;
    }
    // A shovel on loose ground. **The one speed rule that is not a
    // tier**, and it is the whole of what a shovel is: soil, sand,
    // gravel, snow and ash are `Work::Any`, so every tool already digs
    // them at its own tier and a fist digs them slowly -- there was
    // nothing left for a shovel to unlock. What it can do is be *twice
    // as good at the job it is for*, which is a reason to carry one
    // rather than an obligation. See `Work::Ground` and
    // `BLOCK_COPPER_SHOVEL` for why it was not made a gate.
    //
    // Read off the tool rather than off the tier, because a shovel of
    // any metal is a shovel: the day there is a bronze one this line
    // does not change.
    let shovelling = def.matter == crate::blocks::Matter::Loose
        && tool.map(crate::blocks::definition).map(|t| t.work) == Some(crate::blocks::Work::Ground);
    // ...and the edge and the steel, which are the tool's rather than the
    // tier's: a blunt pick is the same pick at the same gate, slower. See
    // `tools::speed`.
    let working = crate::tools::speed(tier, tool);
    let speed = if shovelling { working * 2.0 } else { working };
    Some(hardness / speed)
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
    // **A handful is a quarter of what it heaps into**, rock and all: a pack
    // of basalt chips is carried where a pack of tuff chips is, as the
    // cobbles are (`ground`). The row's number is only a placeholder.
    if let Some(block) = crate::build::block_of_handful(id) {
        return crate::blocks::definition(block).weight / f32::from(crate::dig::SLICES);
    }
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
///
/// **A full jug drops as itself, water and all.** The table's row says
/// `BLOCK_JUG_WATER`, and the table cannot say which water: the kind is the
/// variant (`jug_of`), and a drop spelled as the bare id is river water. So
/// a jug of pond water set down and picked up again came back clean, and a
/// jug of the sea came back drinkable -- the purity a player had kept track
/// of, lost in one break.
#[inline]
pub fn block_drop(id: BlockId) -> Option<BlockId> {
    if block_kind(id) == BLOCK_JUG_WATER {
        return Some(id);
    }
    // **A rotten board crumbles**, where the row says a board. The stages
    // before it give their board back, so pulling a grey roof down in time
    // is a roof's worth of boards and leaving it is dust. See `weathering`.
    if crate::weathering::is_rotten(id) {
        return None;
    }
    // **A shoot gives nothing**, where the row says what the grown plant
    // gives. See `PLANT_YOUNG`.
    if is_plant_shoot(id) {
        return None;
    }
    // **A bough of birch is birch timber.** The row is the oak's bough, and
    // what tells the two apart is the bark in the variant (`birch_branch`); a
    // birch felled for its white planks that came down as oak would be the
    // one tree in the wood not worth cutting.
    if is_birch_wood(id) && matches!(block_kind(id), BLOCK_BOUGH | BLOCK_DROWNED_BOUGH) {
        return Some(BLOCK_BIRCH_LOG);
    }
    // **A hive with honey in it gives the honey**, and the row -- which is
    // the empty comb's, the plain id -- says beeswax. What is in the comb is
    // the variant (`bees::honey_in`), so the table cannot say it.
    if crate::bees::honey_in(id) > 0 {
        return Some(BLOCK_HONEY);
    }
    // **A crown gives a fistful of its own leaves.** The rows all say the
    // oak's handful, because a row cannot say "of whatever wood this is";
    // the wood comes from the crown (`wood::wood_of`) and rides in the
    // handful's id (`carries_wood`).
    if crate::wood::is_wood_leaves(id) {
        let wood = crate::wood::WOODS.iter().position(|w| w.leaves == block_kind(id)).unwrap_or(0);
        return crate::blocks::definition(id).drop.map(|drop| in_wood(drop, wood));
    }
    // **Furniture gives itself back in its own wood**, where the row says
    // the oak piece (`furniture_wood`).
    if carries_wood(id) {
        return crate::blocks::definition(id).drop.map(|drop| in_wood(drop, furniture_wood(id)));
    }
    // **A log cut out of the world is green** (`wood::green`): a trunk is a
    // living tree, and a log pulled out of a wall is a log that stood in the
    // weather. The boughs above returned first, and are dead wood -- dry.
    let drop = crate::blocks::definition(id).drop;
    let drop = if crate::wood::is_log(id) { drop.map(crate::wood::green) } else { drop };
    // **A thing broken while it is still wet comes away wet** (`wet`, "Put
    // down wet"): the water is in the wood, not in the wall. Only onto what
    // gets wet, which leaves a stone's drop -- and a leaf handful's wood --
    // alone.
    if crate::wet::is_wet(id) {
        return drop.map(crate::wet::wetted);
    }
    drop
}

/// What stands in the cell after this block is broken.
///
/// Air for everything, which is what breaking means -- and a picked
/// berry bush for the one block that is *harvested* rather than
/// removed. See `blocks::BlockDef::leaves_behind`.
///
/// Shared because three parties decide what a cell holds after an edit
/// and have to agree: the server, which writes it; the client, which
/// predicts it so the swing does not wait for a round trip; and the
/// collapse pass, which asks what is now standing on what.
#[inline]
pub fn block_residue(id: BlockId) -> BlockId {
    // **Something standing in the sea leaves the sea.** Not a
    // `leaves_behind` row: that field is the harvest list -- a plant that
    // is picked and grows back (`only_things_that_are_picked_leave_anything_behind`)
    // -- and cutting kelp is not picking it. Air here instead was a hole
    // in the ocean the flow simulation then had to fill, a cell at a time,
    // with a wall of surface drawn round it until it did.
    if stands_in_water(id) {
        return BLOCK_WATER;
    }
    // **A raided hive leaves its empty comb**, and the empty comb taken apart
    // leaves nothing. Not a `leaves_behind` row, because one kind is both:
    // the row would have to say "the empty hive" for the empty hive too, and
    // a hive nobody could ever take down is a hive that is not a choice.
    if crate::bees::honey_in(id) > 0 {
        return crate::bees::hive_holding(0);
    }
    crate::blocks::definition(id)
        .leaves_behind
        .unwrap_or(BLOCK_AIR)
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
///
/// **One block says two, and the argument above does not reach it.** That
/// argument is about *material*: a cell that cost one item to build. A
/// ripe cotton plant was never built, it grew from one seed, and a plant
/// that gave back a single boll would make a shirt a field of forty -- a
/// harvest smaller than the chore of bringing it in. Two bolls and the
/// seed back (`also_drops`) is a field that pays for itself.
///
/// **Until cotton, nothing called this.** The one place a broken block's
/// drop is spawned (`spawn_block_drop` on the server) wrote the literal
/// `1`, so the decision written here was not the decision being made.
#[inline]
pub fn block_drop_count(id: BlockId) -> u8 {
    match block_kind(id) {
        BLOCK_COTTON_RIPE => 2,
        // **A ripe head of millet is two grains to eat, and the seed back
        // beside them** (`also_drops`): three in all, where wheat gives one
        // grain and a seed. The grain is the seed (`BLOCK_MILLET`), so the
        // three stack as one -- a smaller meal in more of it, and a field
        // that still pays for its sowing.
        BLOCK_MILLET_RIPE => 2,
        // Two cells of stem, two canes.
        BLOCK_ARUNDO => 2,
        // **A heap of charcoal is the charcoal in it**, which is the
        // number in its variant: a charcoal pit that gave one lump for a
        // heap of four would be a pit nobody dug twice. See `pit`.
        BLOCK_CHARCOAL_PILE => crate::pit::charcoal_in(id).unwrap_or(1),
        // What a flock has not eaten yet: a stack taken apart is its hay.
        BLOCK_HAYSTACK => hay_in_stack(id).unwrap_or(1),
        // **A nettle is two strands**: the best fibre in the world, and it
        // stings the hand that takes it without a knife (`nettle_stings`).
        // One would make it grass that hurts; two is a reason to go to the
        // riverbank with a blade.
        BLOCK_NETTLE => 2,
        // **A cairn is the six stones it was piled from**, so taking one
        // apart to move it costs nothing but the walk.
        BLOCK_CAIRN => 6,
        // **Three bricks out of a mortared wall's four**: the mortar keeps
        // one. See the row, and `build` for the dry wall that keeps none.
        BLOCK_BRICKS => 3,
        // **A hive gives every comb that is in it**, and the empty comb two
        // lumps of wax: taking a hive apart is the end of it, and one lump --
        // two torches -- for the end of a place is a trade nobody would make
        // twice.
        BLOCK_WILD_HIVE => match crate::bees::honey_in(id) {
            0 => 2,
            honey => honey,
        },
        _ => 1,
    }
}

/// What else comes off a block when it is broken, beside `block_drop`.
///
/// **The seed, and only the seed.** A ripe crop used to give its harvest
/// and nothing else -- ripe wheat was grain and no seed -- so a field
/// ended the day it was reaped, and sowing it again meant walking back to
/// the wild stand for more. That is not a decision, it is a commute: a
/// player who has found wild wheat once learns nothing by going back to
/// it every harvest. With the seed back a field *keeps* itself, and the
/// wild stand becomes what it ought to be -- the place you go to make the
/// field bigger.
///
/// One seed, not two. Two would double a field every harvest, and the
/// wild stand would matter for exactly one trip.
///
/// Three shapes were weighed for saying this:
///
/// * **A second `drop` column on `BlockDef`.** Two hundred rows carrying
///   a `None` for the sake of three plants, and "what breaking this
///   gives" split across two columns that have to be read together.
/// * **Threshing as a recipe** -- a ripe ear in, grain and seed out.
///   Honest, and a chore with exactly one answer: nobody would ever *not*
///   thresh, so the row would be a click tax on every harvest.
/// * **This**: one function beside `block_drop` and `block_drop_count`,
///   asked in the one place the server spawns what a broken block leaves,
///   so a mod that breaks a crop gets the seed as well. What was chosen.
pub fn also_drops(id: BlockId) -> Option<(BlockId, u8)> {
    match block_kind(id) {
        BLOCK_WHEAT_RIPE => Some((BLOCK_SEEDS, 1)),
        // The wild stand as well as the field: a boll *is* seed wrapped
        // in fibre, and a stand that gave the fibre and kept the seed
        // would be a plant nobody could ever sow.
        BLOCK_COTTON_RIPE | BLOCK_WILD_COTTON => Some((BLOCK_COTTON_SEEDS, 1)),
        // Millet's seed is its grain, and it comes back all the same: the rule
        // is "a field sows itself again", not "a seed is a different thing".
        BLOCK_MILLET_RIPE => Some((BLOCK_MILLET, 1)),
        // **Not a seed, and still the rule's shape**: a cattail pulled up is
        // its root and the leaves on it, and the leaves are a strand. Here
        // rather than as a count, because the two are two kinds.
        BLOCK_CATTAIL => Some((BLOCK_FIBER, 1)),
        _ => None,
    }
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
    ///
    /// Takes either float: a position is an `f64` wherever it is kept, and
    /// an `f32` wherever it has already been measured from somewhere near.
    pub fn from_world(x: impl Into<f64>, z: impl Into<f64>) -> ChunkPos {
        let (pos, _, _) = ChunkPos::from_global(x.into().floor() as i32, z.into().floor() as i32);
        pos
    }

    /// Chebyshev distance in chunks -- the natural metric for a square
    /// render/interest area.
    ///
    /// **Worked out in `i64` because one side of it comes off a socket.**
    /// A `ChunkPos` is two `i32`s that a client picks, and the very first
    /// thing done with a `RequestChunk` is to measure it against where
    /// the player is (`anticheat::check_chunk_request`). Subtracting
    /// `i32::MIN` from a chunk near the origin overflows: in a debug
    /// build that is a panic on the reader task -- a disconnect for one
    /// malformed packet -- and in release it wraps to a *negative*
    /// distance, which passes the range check and lets a client have the
    /// server generate terrain anywhere it likes.
    ///
    /// Saturating at `i32::MAX` rather than returning the true `i64`:
    /// every caller compares against a view distance of a few dozen, so
    /// "further than anything" is the whole of the answer they need.
    pub fn chebyshev_distance(&self, other: ChunkPos) -> i32 {
        let dx = (self.x as i64 - other.x as i64).abs();
        let dz = (self.z as i64 - other.z as i64).abs();
        dx.max(dz).min(i32::MAX as i64) as i32
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
    //
    // **Run-length encoded on the wire, flat in memory.** A column two
    // hundred and fifty-six blocks tall is stone at the bottom, air at
    // the top and a thin crust of everything interesting between, and
    // sent raw that is 131 KB a chunk -- four times what it cost at
    // sixty-four, for a quarter more information. Measured over real
    // generated chunks it is about eleven hundred runs, which is four
    // and a half kilobytes: the same chunk at a twenty-ninth of the
    // bandwidth. See `rle_blocks`.
    #[serde(with = "rle_blocks")]
    pub blocks: Vec<BlockId>,
}

/// How a chunk's block array travels: as `(run length, block)` pairs.
///
/// Chosen over a general compressor for three reasons that are all about
/// this data rather than about compression. A chunk's array is laid out
/// y-first (`Chunk::index`), so a layer of air or stone is one long run
/// by construction; run-length needs no dictionary and no dependency; and
/// decoding is a loop of copies with no state a malicious length could
/// blow up -- the total is checked against `CHUNK_VOLUME` before a single
/// cell is written.
mod rle_blocks {
    use super::{BlockId, CHUNK_VOLUME};
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(blocks: &[BlockId], s: S) -> Result<S::Ok, S::Error> {
        let mut runs: Vec<(u32, BlockId)> = Vec::new();
        for &block in blocks {
            match runs.last_mut() {
                Some((count, last)) if *last == block => *count += 1,
                _ => runs.push((1, block)),
            }
        }
        runs.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<BlockId>, D::Error> {
        let runs: Vec<(u32, BlockId)> = Vec::deserialize(d)?;
        // Summed before anything is allocated: a packet claiming a run of
        // four billion stone must be refused, not attempted.
        let total: u64 = runs.iter().map(|(count, _)| *count as u64).sum();
        if total > CHUNK_VOLUME as u64 {
            return Err(D::Error::custom(format!(
                "a chunk of {total} cells, and a chunk holds {CHUNK_VOLUME}"
            )));
        }
        let mut blocks = Vec::with_capacity(total as usize);
        for (count, block) in runs {
            blocks.extend(std::iter::repeat_n(block, count as usize));
        }
        // A *short* chunk is still let through, deliberately: the length
        // check lives in `Chunk::is_well_formed`, on receipt, where the
        // client already turns a malformed chunk into an error rather
        // than a panic. Two places deciding that would disagree one day.
        Ok(blocks)
    }
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

    /// **A pot of earth grows what a pot grows**: the berries, the herbs and
    /// the sown crops, and nothing that would be a tree on a windowsill.
    #[test]
    fn a_pot_of_earth_is_ground_for_a_bush_and_not_for_a_tree() {
        for plant in [BLOCK_BILBERRY, BLOCK_STRAWBERRY, BLOCK_BERRY_BUSH, BLOCK_FERN, BLOCK_FLOWER, BLOCK_SEEDS] {
            assert!(can_grow_on(plant, BLOCK_PLANTER), "{} will not grow in a pot", block_name(plant));
        }
        for wrong in [BLOCK_CACTUS, BLOCK_LEAVES] {
            assert!(!can_grow_on(wrong, BLOCK_PLANTER), "{} grew in a pot", block_name(wrong));
        }
        // ...and a pot is a block like any other: it is set down, not planted,
        // so nothing holds it up and nothing has to.
        assert!(!needs_support(BLOCK_PLANTER), "a pot of earth is held up by something");
        assert!(is_known_block(BLOCK_PLANTER));
    }

    /// **A stake stands on a floor or is driven into a wall**, and each is
    /// held by the cell it is in or against: put one anywhere else and the
    /// rules that take away what nothing holds take it away.
    #[test]
    fn a_stake_is_held_by_the_floor_it_stands_on_or_the_wall_it_is_driven_into() {
        let upright = placed(BLOCK_STAKE, 0.0, (0, 1, 0));
        assert!(stake_is_upright(upright), "a stake put on a floor did not stand up");
        assert_eq!(support_at(upright), (0, -1, 0));
        assert!(can_grow_on(upright, BLOCK_STONE), "a stake will not stand on stone");
        assert!(!can_grow_on(upright, BLOCK_AIR), "a stake stood in the air");

        // Driven in: the facing is the wall that was clicked, and the cell
        // it names is that wall.
        for (clicked, wall) in [((0, 0, 1), (0, 0, -1)), ((1, 0, 0), (-1, 0, 0)), ((0, 0, -1), (0, 0, 1)), ((-1, 0, 0), (1, 0, 0))] {
            let driven = placed(BLOCK_STAKE, 0.0, clicked);
            assert!(!stake_is_upright(driven), "{clicked:?}: a stake driven into a wall stood up instead");
            assert_eq!(support_at(driven), wall, "{clicked:?}: the stake hangs from the wrong cell");
            assert!(can_grow_on(driven, BLOCK_STONE), "{clicked:?}: a stake will not go into stone");
            assert!(!can_grow_on(driven, BLOCK_WATER), "{clicked:?}: a stake was driven into water");
            assert!(needs_support(driven), "a stake hangs on nothing");
            assert!(is_known_block(driven), "a driven stake is an id the anti-cheat refuses");
        }
        assert!(is_known_block(upright), "a standing stake is an id the anti-cheat refuses");
    }

    /// **A rack is four cells and one frame**: two along its ridge and two
    /// high, any of which names the same anchor and the same three partners.
    #[test]
    fn every_cell_of_a_rack_names_the_same_four_cells_and_the_same_anchor() {
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let anchor = (10, 4, -7);
            let cells = rack_cells(anchor, facing);
            assert_eq!(cells.len(), 4);
            let mut seen: Vec<(i32, i32, i32)> = cells.iter().map(|&(at, _)| at).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 4, "{facing:?}: two cells of one rack are the same cell");
            for &(at, id) in &cells {
                assert_eq!(rack_anchor(at, id), anchor, "{facing:?}: a cell of the rack lost its anchor");
                let partners = rack_partners(at, id);
                assert_eq!(partners.len(), 3);
                let whole = rack_whole(at, id, |cell| cells.iter().find(|&&(c, _)| c == cell).map(|&(_, there)| there));
                assert!(whole, "{facing:?}: a rack standing in its own four cells is not whole");
                // ...and a lone cell, which is what an older save holds, is not.
                assert!(!rack_whole(at, id, |_| None), "{facing:?}: a rack with no partners is whole");
            }
        }
    }

    /// What hangs on a rack rides in the bits its cells have spare, two a
    /// cell and four a column, and the anti-cheat takes every one of them.
    #[test]
    fn a_rack_cell_carries_what_hangs_on_it_and_is_still_a_known_block() {
        for facing in [Facing::North, Facing::South] {
            for far in [false, true] {
                for top in [false, true] {
                    let cell = rack_cell(facing, far, top);
                    assert!(is_known_block(cell), "a bare rack cell is refused");
                    for bits in 0..4u8 {
                        let hung = with_rack_goods(cell, bits);
                        assert_eq!(rack_goods(hung), bits);
                        assert_eq!(rack_shape(hung), cell, "the goods changed which cell this is");
                        assert_eq!(block_facing(hung), facing);
                        assert_eq!(rack_is_far(hung), far);
                        assert_eq!(rack_is_top(hung), top);
                        assert!(is_known_block(hung), "a rack with {bits} hanging on it is refused");
                    }
                }
            }
        }
        // The column reads bottom bits then top bits, which is how a rack
        // says which of the fifteen things in `rack::HANGING` is on it.
        let bottom = with_rack_goods(rack_cell(Facing::North, false, false), 0b10);
        let top = with_rack_goods(rack_cell(Facing::North, false, true), 0b11);
        assert_eq!(rack_column_goods(bottom, top), 0b1110);
    }

    #[test]
    fn the_tinder_bracket_grows_on_dead_wood_and_on_nothing_else() {
        // The rule that keeps the fungus a thing you find rather than a
        // thing you farm. It wants timber under it, so a player who
        // wants a crop of them has to lay the timber first -- and it
        // refuses every floor its cousins accept, which is what stops
        // "a fungus" meaning "the mushroom with a different picture".
        for wood in [BLOCK_LOG, BLOCK_BIRCH_LOG, BLOCK_PLANKS, BLOCK_BIRCH_PLANKS] {
            assert!(
                can_grow_on(BLOCK_BRACKET_FUNGUS, wood),
                "the bracket refused {}",
                block_name(wood)
            );
        }
        for not_wood in [
            BLOCK_GRASS,
            BLOCK_DIRT,
            BLOCK_STONE,
            BLOCK_GRAVEL,
            BLOCK_SAND,
            BLOCK_ASH,
            BLOCK_LIMESTONE,
        ] {
            assert!(
                !can_grow_on(BLOCK_BRACKET_FUNGUS, not_wood),
                "the bracket grew on {}",
                block_name(not_wood)
            );
        }
        // ...and the mushroom does not move in the other direction: the
        // two fungi are told apart by where they are, so a mushroom on
        // a log would undo the whole distinction.
        assert!(!can_grow_on(BLOCK_MUSHROOM, BLOCK_LOG));
    }

    #[test]
    fn fur_is_a_garment_and_is_told_apart_from_leather_by_its_colour() {
        // Twelve garments became fourteen and none of them gained a
        // picture -- see `garment_tint`. What that costs is that two
        // sets share one drawing, so the tints have to be far enough
        // apart to read at hotbar size. The leather set is the one it
        // could be confused with, and this is the check that says it
        // cannot: a whole step of value between them.
        let fur = garment_tint(BLOCK_FUR_CLOAK).expect("fur is a garment");
        let leather = garment_tint(BLOCK_LEATHER_TUNIC).expect("leather is a garment");
        assert_eq!(fur, garment_tint(BLOCK_FUR_HOOD).expect("so is the hood"));
        let value = |c: [f32; 3]| (c[0] + c[1] + c[2]) / 3.0;
        assert!(
            value(leather) - value(fur) > 0.1,
            "fur {fur:?} and leather {leather:?} are the same weight of colour"
        );
    }

    #[test]
    fn an_empty_jug_refuses_meat_that_rots() {
        // The one rule `pours` exists to keep. A jug is not a slot, so
        // `logic::rot` never walks what is inside one -- see the note on
        // `pours` -- and anything with a clock that went in would stop
        // ageing. This is the test that goes red when somebody adds
        // berries to the list because a jug of berries sounds right.
        for perishable in [
            BLOCK_RAW_MEAT,
            BLOCK_COOKED_MEAT,
            BLOCK_DRIED_MEAT,
            BLOCK_BERRIES,
            BLOCK_ROOT,
            BLOCK_ROASTED_ROOT,
            BLOCK_APPLE,
            BLOCK_BREAD,
            BLOCK_MUSHROOM,
        ] {
            assert!(
                !pours(perishable),
                "{} may be poured into a jug, which would stop its rot clock",
                block_name(perishable)
            );
        }
        // And nothing on the list has one, checked the other way round
        // so a food that grows a clock later cannot quietly join it.
        for id in 0..=u8::MAX as BlockId {
            if !is_known_block(id) || !pours(id) {
                continue;
            }
            assert!(
                !crate::food::is_food(id) && !crate::food::is_perishable(id),
                "{} is food and is also pourable",
                block_name(id)
            );
        }
    }

    #[test]
    fn everything_that_pours_is_something_a_player_can_actually_hold() {
        // A pourable that does not stack is a contradiction: the whole
        // gesture is "a handful of this goes in", and a handful of one
        // axe is not a handful.
        for id in 0..=u8::MAX as BlockId {
            if !is_known_block(id) || !pours(id) {
                continue;
            }
            assert!(
                stack_limit(id) > 1,
                "{} pours but stacks to one",
                block_name(id)
            );
            assert!(
                tool_durability(id).is_none(),
                "{} pours but wears out, and wear lives where the contents do",
                block_name(id)
            );
        }
    }

    #[test]
    fn a_jug_stacks_to_one_because_its_contents_do_not_split() {
        // A slot holds one number for whatever is in the jug (see
        // `inventory::jug_contents`), so two jugs in one square cannot
        // both be described. If this ever goes back to four, a pack of
        // jugs is a pack of jugs that all hold the same thing.
        assert_eq!(stack_limit(BLOCK_JUG), 1);
    }

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
    fn the_list_of_what_can_be_placed_and_the_flag_that_says_so_are_one_answer() {
        // **Two encodings of one rule.** `is_placeable` reads a `bool`
        // off the block's own row; `PLACEABLE_BLOCKS` is that same set
        // written out by hand, and its own doc comment says so. Nothing
        // derived one from the other, so they were free to drift, and
        // they did -- wool arrived with `placeable: true` on its row and
        // never reached the list.
        //
        // What that cost is only visible in the test world, because the
        // gallery `showcase` lays out is read off the list: a bale of
        // wool -- a block whose own doc explains that players will lay a
        // floor of it for the look of the thing -- was the one placeable
        // block missing from the field of one of everything. The next
        // divergence need not be so cheap, which is why this is a test
        // rather than a fix.
        let by_flag: Vec<&str> = ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| is_placeable(id))
            .map(|&(_, name)| name)
            .collect();
        let mut listed: Vec<&str> = PLACEABLE_BLOCKS.iter().map(|&id| block_name(id)).collect();
        listed.sort_unstable();
        let mut by_flag_sorted = by_flag.clone();
        by_flag_sorted.sort_unstable();
        assert_eq!(
            listed, by_flag_sorted,
            "the hand-written list and the `placeable` column disagree"
        );
        // ...and the list says each block once, or the gallery draws one
        // of them twice and stops being one of everything.
        let mut unique = listed.clone();
        unique.dedup();
        assert_eq!(unique.len(), listed.len(), "a block is listed twice");
    }

    #[test]
    fn chunk_distance_is_chebyshev() {
        assert_eq!(
            ChunkPos::new(0, 0).chebyshev_distance(ChunkPos::new(3, -5)),
            5
        );
    }

    #[test]
    fn a_chunk_at_the_end_of_the_number_line_is_far_away_rather_than_a_panic() {
        // A `ChunkPos` arrives off a socket, and the first thing done
        // with one is this subtraction. `i32::MIN` overflowed it: a
        // panic on the reader task in debug, and in release a *negative*
        // distance that sailed through the anti-cheat's range check and
        // set the generator to work on the far side of the world.
        let here = ChunkPos::new(0, 0);
        for absurd in [
            ChunkPos::new(i32::MIN, 0),
            ChunkPos::new(0, i32::MIN),
            ChunkPos::new(i32::MAX, i32::MIN),
            ChunkPos::new(i32::MIN, i32::MIN),
        ] {
            let d = here.chebyshev_distance(absurd);
            assert!(d > 0, "{absurd:?} came out {d} chunks away");
            assert_eq!(d, here.chebyshev_distance(absurd), "not symmetric in itself");
        }
        // ...and the far corner measured from the other far corner is
        // still a number, not a wrap.
        assert!(ChunkPos::new(i32::MIN, i32::MIN).chebyshev_distance(ChunkPos::new(i32::MAX, i32::MAX)) > 0);
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
    fn solids_are_still_solid_and_foliage_is_not() {
        for id in [BLOCK_STONE, BLOCK_DIRT, BLOCK_GRASS, BLOCK_LOG] {
            assert!(is_collidable(id), "{} should be solid", block_name(id));
            assert!(!is_liquid(id));
        }
        assert!(!is_collidable(BLOCK_AIR));
        // **Leaves are pushed through now**, and they were on this list
        // because for two years they were a wall. What they are instead
        // is slow: see `surface_drag`, which is what keeps a wood
        // something you decide to go round.
        for id in [BLOCK_LEAVES, BLOCK_BIRCH_LEAVES, BLOCK_BUSH_LEAVES] {
            assert!(!is_collidable(id), "{} should be pushed through", block_name(id));
            assert!(
                surface_drag(id) < 0.5,
                "{} lets you through at full speed",
                block_name(id)
            );
        }
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
                "iron_ore",
                // ...and the three rocks under the soil, each a tool's job:
                // flint for sandstone and limestone, copper for granite.
                "sandstone",
                "limestone",
                "granite",
                // ...and slag laid as a wall, which is glassy stone and a
                // pick's work like the rock it was smelted out of. See
                // `BLOCK_SLAG`.
                "slag",
                // ...and the floor of the world, which needs bronze.
                "basalt",
                // ...and the thick piece of an experimental tree, which is
                // a standing trunk by another name. See `BLOCK_BOUGH`.
                "bough",
                // ...and the two stones of a reef, which are limestone in
                // colour and want limestone's pick. See `BLOCK_BRAIN_CORAL`.
                "brain_coral",
                "fire_coral",
                // ...and a piece of a palm's trunk, which is a bough in
                // every rule but its bark. See `BLOCK_PALM_TRUNK`.
                "palm_trunk",
                // ...and a drowned snag in a swamp pool, a bough that stands
                // in water: timber, and an axe's. See `BLOCK_DROWNED_BOUGH`.
                "drowned_bough",
                // ...and the fir's and the saxaul's standing trunks, which are
                // the oak's in every rule. See `BLOCK_FIR_LOG`.
                "fir_log",
                "saxaul_log",
                // ...and the ten rocks `ground` added, each a tool's job as
                // the older three are -- flint, copper or bronze by its row.
                "shale",
                "chalk",
                "dolomite",
                "marble",
                "quartzite",
                "gneiss",
                "diorite",
                "gabbro",
                "andesite",
                "tuff",
                // ...and the pine's and the willow's trunks, the oak's again.
                "pine_log",
                "willow_log",
                // ...and every other bark's bough, a standing trunk as the
                // oak's bough is. See `OWN_BARK`.
                "fir_bough",
                "saxaul_bough",
                "pine_bough",
                "willow_bough",
                // ...and the anvil, the one workshop that wants a pick. A
                // slab of cast bronze on a stump is not a bench you lift.
                // See `BLOCK_ANVIL`.
                "anvil",
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
        //
        // **Every tool in the game, not only the flint three.** Iron ore
        // asks for copper now, so a list of flint tools would report the
        // one deliberately gated block in the world as scenery -- which
        // is the opposite of what this test is for. Reading the block
        // table also means a tool added later is covered without anybody
        // remembering to add it here.
        let tools: Vec<BlockId> = crate::blocks::BLOCKS
            .iter()
            .filter(|b| b.tool.is_some())
            .map(|b| b.id)
            .collect();
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
    fn a_shovel_digs_loose_ground_twice_as_fast_and_rock_not_at_all() {
        // The whole of what a shovel is, in one test, because it is the
        // one tool in the game whose value is a *speed* rather than a
        // gate -- see `BLOCK_COPPER_SHOVEL`.
        //
        // Both halves matter. Twice as fast is the reason to carry one;
        // no use on rock is the reason it is not simply a better pick.
        for loose in [BLOCK_DIRT, BLOCK_SAND, BLOCK_GRAVEL, BLOCK_SNOW, BLOCK_ASH] {
            let with_shovel = break_seconds_with(loose, Some(BLOCK_COPPER_SHOVEL))
                .unwrap_or_else(|| panic!("{} cannot be dug at all", block_name(loose)));
            let with_pick = break_seconds_with(loose, Some(BLOCK_COPPER_PICKAXE))
                .unwrap_or_else(|| panic!("{} cannot be picked at all", block_name(loose)));
            assert!(
                (with_pick / with_shovel - 2.0).abs() < 1e-4,
                "{}: a shovel is {:.2} times a pick of the same metal, not twice",
                block_name(loose),
                with_pick / with_shovel
            );
        }
        // ...and it takes nothing away from anybody. A pick still digs
        // soil at its own tier, which is the promise that made a shovel
        // safe to add: see `Work::Ground`.
        assert!(
            break_seconds_with(BLOCK_DIRT, Some(BLOCK_COPPER_PICKAXE)).unwrap()
                < break_seconds_with(BLOCK_DIRT, None).unwrap(),
            "a pick digs soil no better than fingernails"
        );
        // A shovel is no use on rock, and no use on a standing trunk.
        assert!(
            !is_breakable_with(BLOCK_STONE, Some(BLOCK_COPPER_SHOVEL)),
            "a shovel opens rock"
        );
        assert_eq!(
            break_seconds_with(BLOCK_LOG, Some(BLOCK_COPPER_SHOVEL)),
            break_seconds_with(BLOCK_LOG, None),
            "a shovel fells a tree faster than a fist"
        );
        // ...and the bonus is the tool's, not the tier's: a copper axe
        // is the same metal and digs soil at the plain copper rate.
        assert_eq!(
            break_seconds_with(BLOCK_DIRT, Some(BLOCK_COPPER_AXE)),
            break_seconds_with(BLOCK_DIRT, Some(BLOCK_COPPER_KNIFE)),
            "some copper tool other than the shovel is quick at digging"
        );
    }

    #[test]
    fn both_hoes_till_and_nothing_else_does() {
        // `is_implement` is what the server asks before turning turf
        // into a field, and it is a list precisely so that a second hoe
        // cannot be forgotten -- which is exactly the bug a copper hoe
        // that tilled nothing would have been.
        assert!(is_implement(BLOCK_HOE), "the flint hoe stopped tilling");
        assert!(is_implement(BLOCK_COPPER_HOE), "the copper hoe does not till");
        for not_a_hoe in [
            BLOCK_COPPER_SHOVEL,
            BLOCK_COPPER_AXE,
            BLOCK_COPPER_KNIFE,
            BLOCK_COPPER_HOE_HEAD,
            BLOCK_DIRT,
        ] {
            assert!(
                !is_implement(not_a_hoe),
                "{} tills a field",
                block_name(not_a_hoe)
            );
        }
        // The metal is the whole reason to make one: a flint blade
        // lashed to a stick is what breaks first in a field.
        assert!(
            tool_durability(BLOCK_COPPER_HOE) > tool_durability(BLOCK_HOE),
            "a copper hoe wears out no slower than a flint one"
        );
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
        // A flint pick opens rock and everything in it **except iron**,
        // which is the one place in the game where a tier is a gate
        // rather than a speed. That exception is the whole shape of the
        // metal age: copper is not a better pick, it is the only pick
        // that reaches the next material.
        for block in [
            BLOCK_STONE,
            BLOCK_COAL_ORE,
            BLOCK_COPPER_ORE,
            BLOCK_TIN_ORE,
        ] {
            assert!(
                is_breakable_with(block, Some(BLOCK_WEDGED_PICKAXE)),
                "{} with a glued pick",
                block_name(block)
            );
        }
        assert!(
            !is_breakable_with(BLOCK_IRON_ORE, Some(BLOCK_WEDGED_PICKAXE)),
            "flint opens iron ore, so copper is a formality"
        );
        assert!(
            is_breakable_with(BLOCK_IRON_ORE, Some(BLOCK_COPPER_PICKAXE)),
            "copper does not open iron ore, which is the only thing it is for"
        );
        // ...and it is still miserable with copper. The gate is a gate;
        // what makes iron a material rather than an expedition is bronze.
        let iron = break_seconds_with(BLOCK_IRON_ORE, Some(BLOCK_COPPER_PICKAXE)).unwrap();
        let stone = break_seconds_with(BLOCK_STONE, Some(BLOCK_COPPER_PICKAXE)).unwrap();
        assert!(iron > stone * 2.0, "iron ore should punish a soft edge");
        let with_bronze = break_seconds_with(BLOCK_IRON_ORE, Some(BLOCK_BRONZE_PICKAXE)).unwrap();
        assert!(with_bronze < iron * 0.75, "bronze does not repay the alloying");

        // The axe opens standing timber, and only the axe does.
        for wood in [BLOCK_LOG, BLOCK_BIRCH_LOG] {
            assert!(
                is_breakable_with(wood, Some(BLOCK_WEDGED_AXE)),
                "{} with an axe",
                block_name(wood)
            );
            for wrong in [BLOCK_WEDGED_PICKAXE, BLOCK_FLINT_KNIFE] {
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
        assert!(!is_breakable_with(BLOCK_STONE, Some(BLOCK_WEDGED_AXE)));
        assert!(!is_breakable_with(BLOCK_STONE, Some(BLOCK_FLINT_KNIFE)));

        // The knife is the odd one: what it opens, hands already could.
        // What it buys is speed on growing things, and nothing else does.
        for plant in [BLOCK_TALL_GRASS, BLOCK_LEAVES, BLOCK_BIRCH_LEAVES] {
            let by_hand = break_seconds(plant).unwrap();
            let cut = break_seconds_with(plant, Some(BLOCK_FLINT_KNIFE)).unwrap();
            assert!(cut < by_hand, "{} is no faster with a knife", block_name(plant));
            assert_eq!(
                break_seconds_with(plant, Some(BLOCK_WEDGED_PICKAXE)),
                Some(by_hand),
                "{} gave way to a pickaxe",
                block_name(plant)
            );
        }

        // Work nobody needs a tool for takes the tier from any of them:
        // a haft is a haft.
        for tool in [BLOCK_WEDGED_PICKAXE, BLOCK_WEDGED_AXE, BLOCK_FLINT_KNIFE] {
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
        for pick in [BLOCK_WEDGED_PICKAXE, BLOCK_WEDGED_AXE, BLOCK_FLINT_KNIFE] {
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
            // **Followed back rather than looked up.** Iron does not
            // come out of its ore in one step any more -- the shaft
            // yields a bloom and the bloom is worked down to metal (see
            // `crafting`'s bloomery rows) -- so what this checks is that
            // the ore is somewhere *behind* the ingot, however many
            // steps back that is.
            let mut behind: Vec<BlockId> =
                recipe.inputs.iter().map(|&(b, _)| b).collect();
            let mut reached = false;
            for _ in 0..4 {
                if behind.contains(&ore) {
                    reached = true;
                    break;
                }
                behind = behind
                    .iter()
                    .filter_map(|&b| makes(b))
                    .flat_map(|r| r.inputs.iter().map(|&(b, _)| b))
                    .collect();
            }
            assert!(reached, "{} does not lead back to its ore", block_name(ingot));
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
        for tool in [BLOCK_WEDGED_PICKAXE, BLOCK_WEDGED_AXE, BLOCK_FLINT_KNIFE] {
            assert!(makes(tool).is_some(), "{} cannot be made", block_name(tool));
        }
    }

    #[test]
    fn digging_is_something_you_spend_time_on() {
        // Nothing is instant, and nothing takes so long that a player
        // would think the game had stopped responding. Five seconds was
        // the limit of what read as work rather than as a hang; the player
        // asked for every block to take twice as long (`work_slowdown`),
        // and ten is that limit doubled -- the bar is moving the whole
        // time, which is what separates slow work from a frozen game.
        for &(id, name) in ALL_BLOCK_IDS {
            let Some(seconds) = break_seconds(id) else { continue };
            assert!(seconds > 0.0, "{name} breaks instantly");
            assert!(seconds <= 10.0, "{name} takes {seconds}s by hand");
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
        // A trunk gives the same log, green off the stump (`wood::green`).
        assert_eq!(block_drop(BLOCK_LOG), Some(crate::wood::green(BLOCK_LOG)));
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
                // **The exception list is gone.** Bronze and iron used
                // to be smelted and then wait, because the tools they
                // fed had been taken out; the nine metal tools are what
                // closed that. See the matching note in
                // `crafting::tests::every_recipe_is_made_of_real_blocks`.
                //
                // Food counts as spent, because eating is spending: a
                // handful of berries no recipe wants is not dead weight,
                // it is dinner.
                if crate::food::is_food(drop) {
                    continue;
                }
                // ...and the two categories 1.7 added, on the same
                // terms. A garment is spent by being worn -- it stops
                // blows until it is gone -- and a jug is spent by being
                // drunk from. Neither is an ingredient and neither is
                // dead weight, which is the distinction this test is
                // actually about.
                if crate::equipment::is_wearable(drop) || is_vessel(drop) {
                    continue;
                }
                // ...and what the rack turns into something else is
                // spent by being hung up: a hide is cured, not crafted,
                // now that no haft asks for a strip of it. Fuel is spent
                // by being burned: a brick of dried peat goes into a
                // hearth, not a recipe.
                if crate::rack::cures_into(drop).is_some() || crate::hearth::is_fuel(drop) {
                    continue;
                }
                // ...and an implement, which is spent on the world: a
                // hoe on turf, a peg into a joint. Same category as the
                // garment above -- used rather than crafted with -- and
                // the reason it is `is_implement` rather than a name
                // here is that the list lives in one place.
                if is_implement(drop) {
                    continue;
                }
                // ...and a dressing, which is spent on a wound: a bandage
                // picked back up is still a bandage waiting for a cut. See
                // `injury::Treatment`.
                if crate::injury::Treatment::of(drop).is_some() {
                    continue;
                }
                // ...and a raft, which is spent on the water: it is put
                // down as a boat, not as a block, and breaking one gives it
                // back to be launched again.
                if block_kind(drop) == crate::types::BLOCK_RAFT {
                    continue;
                }
                // ...and bait, which is spent on the hook: a worm is eaten
                // by the fish that takes it (`fishing::Bait`), which is the
                // garment's bargain again -- used rather than crafted with.
                if crate::fishing::Bait::of_block(drop).is_some() {
                    continue;
                }
                // ...and an instrument, which is read in the hand
                // (`is_instrument`): a compass is spent on nothing.
                if is_instrument(drop) {
                    continue;
                }
                // ...and tack, which is put on a horse (`is_tack`).
                if is_tack(drop) {
                    continue;
                }
                // ...and daub and cob, which are laid into a wall rather than
                // crafted with (`build::is_laid`).
                if crate::build::is_laid(drop) {
                    continue;
                }
                // ...and a half-dried sod of peat, which nothing is made
                // from because it is not finished: it is set down again to
                // finish drying (`BLOCK_DRYING_PEAT`).
                if block_kind(drop) == crate::types::BLOCK_DRYING_PEAT {
                    continue;
                }
                // ...and a must or a young cheese, for the same reason: it is
                // not finished, and what finishes it is time (`ferment`).
                if crate::ferment::is_working(drop) {
                    continue;
                }
                // ...and what is set on the ground and left to work there: a
                // snare, a pit's cover, a salt pan (`snare`, `pitfall`,
                // `saltpan`) -- spent by the world, as a hoe is.
                if crate::snare::is_snare(drop) || crate::saltpan::is_pan(drop) || block_kind(drop) == BLOCK_PIT_COVER {
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
    fn sand_and_every_soil_fall_and_rock_and_wood_do_not() {
        for id in [BLOCK_SAND, BLOCK_GRAVEL, BLOCK_DIRT, BLOCK_GRASS, BLOCK_SNOW, BLOCK_MUD] {
            assert!(is_affected_by_gravity(id), "{} should fall", block_name(id));
        }
        for id in [BLOCK_STONE, BLOCK_CLAY, BLOCK_LOG, BLOCK_GLOWSTONE] {
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

#[cfg(test)]
mod rack_tests {
    use super::*;

    #[test]
    fn a_skin_on_the_frame_is_a_bit_and_not_a_block() {
        // The whole argument for the spare variant bit: a loaded rack
        // has to be a rack everywhere except in the mesher. If any of
        // these stopped holding, it would be a second block id wearing a
        // disguise -- and the disguise would slip in the drop table, the
        // container store or the crafting menu.
        let north = faced(BLOCK_DRYING_RACK, Facing::North);
        let east = faced(BLOCK_DRYING_RACK, Facing::East);
        for rack in [north, east] {
            let loaded = rack_with_hide(rack, true);
            assert!(rack_is_loaded(loaded));
            assert!(!rack_is_loaded(rack));
            assert_eq!(block_kind(loaded), BLOCK_DRYING_RACK, "it stopped being a rack");
            assert_eq!(
                block_facing(loaded),
                block_facing(rack),
                "putting a skin on it turned it round"
            );
            assert_eq!(rack_with_hide(loaded, false), rack, "taking it off left something behind");
            assert_eq!(crate::blocks::definition(loaded).drop, Some(BLOCK_DRYING_RACK));
        }
    }

    #[test]
    fn a_hide_frame_says_its_skin_has_cured_and_nothing_else_can() {
        // `HIDE_CURED` is a wood bit, and a wood bit on anything that is not
        // furniture is an id the anti-cheat calls invented -- so it is legal
        // on a hide frame with a skin on it and on nothing else, and a cured
        // skin is still a skin: the frame is loaded, faces where it faced and
        // drops a frame.
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let frame = faced(BLOCK_HIDE_FRAME, facing);
            let raw = hide_frame_showing(frame, true, false);
            let cured = hide_frame_showing(frame, false, true);
            assert!(rack_is_loaded(raw) && !hide_is_cured(raw), "{facing:?}: a raw skin read as cured");
            assert!(rack_is_loaded(cured) && hide_is_cured(cured), "{facing:?}: a cured skin read as {cured:#x}");
            // A raw skin hung while the last one's leather waits in the tray
            // is still drying, and that is what the frame says.
            assert_eq!(hide_frame_showing(frame, true, true), raw);
            assert_eq!(hide_frame_showing(cured, false, false), frame, "{facing:?}: taking the leather left something");
            for id in [frame, raw, cured] {
                assert!(is_known_block(id), "{facing:?}: {id:#x} is a frame the rules call invented");
                assert_eq!(block_kind(id), BLOCK_HIDE_FRAME);
                assert_eq!(block_facing(id), facing);
                assert_eq!(crate::blocks::definition(id).drop, Some(BLOCK_HIDE_FRAME));
            }
            assert!(!is_known_block(frame | HIDE_CURED), "{facing:?}: a cured skin on a bare frame");
        }
        for &(id, name) in ALL_BLOCK_IDS {
            if id == BLOCK_HIDE_FRAME {
                continue;
            }
            assert!(!hide_is_cured(id | HIDE_CURED | RACK_LOADED), "{name} claimed a cured skin");
            assert_eq!(hide_frame_showing(id, false, true), rack_with_hide(id, false), "{name} was given a cured skin");
        }
    }

    #[test]
    fn nothing_else_can_be_loaded() {
        // The helpers are asked about arbitrary ids -- a chest, a lit
        // campfire, a log lying down -- and must answer about racks
        // only. The bit means something else on every one of them.
        for &(id, name) in ALL_BLOCK_IDS {
            // ...and the hide frame, which is the old one-cell rack and
            // carries its skin in the same bit (`BLOCK_HIDE_FRAME`).
            if id == BLOCK_DRYING_RACK || id == BLOCK_HIDE_FRAME {
                continue;
            }
            assert!(!rack_is_loaded(id), "{name} claimed to have a skin on it");
            assert_eq!(rack_with_hide(id, true), id, "{name} was given a skin");
        }
    }
}

/// Loose material in layers: the second thing the variant field means.
#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn how_deep_a_block_is_comes_from_the_table_and_not_from_its_id() {
        // The removal, stated once, and then the exception.
        //
        // Layers are gone from every *id* that is not a liquid: a save
        // written while loose material carried a depth reads back as
        // whole blocks, because the depth field is simply not consulted
        // any more. What replaced it for the handful of blocks that are
        // genuinely not a metre tall is a column in the block table, and
        // the two must not be confused -- one is a property of the cell,
        // the other of the material.
        let half: Vec<&str> = ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| is_partial(id))
            .map(|&(_, name)| name)
            .collect();
        // The drying rack was on this list while it was a slab. It is a
        // frame standing on end now (`mesh::rack_block`) and takes the
        // whole cell -- which is also what stops it hiding the bottom
        // half of whatever it stands against.
        // The carcasses are two and three eighths high: a heap on the
        // ground, through the same table column. The nest is two as
        // well -- a bowl of twigs on a branch.
        assert_eq!(
            half,
            [
                "backpack",
                "campfire",
                "campfire_lit",
                // ...the jug, five eighths of fired clay: a vessel you
                // set down, not a cube of it.
                "jug",
                "carcass_hare",
                "carcass_deer",
                "carcass_boar",
                "carcass_wolf",
                "carcass_sheep",
                // ...a pat of dung, one eighth. See `BLOCK_DUNG`.
                "dung",
                "carcass_bear",
                "carcass_fowl",
                // ...and the savanna's three, the same heaps.
                "carcass_zebra",
                "carcass_antelope",
                "carcass_lion",
                "carcass_horse",
                "nest_eggs",
                "nest",
                // ...and the furniture, none of which fills its
                // cell: straw two eighths, a bed three, a stool four
                // and a table six.
                "straw_bed",
                "bed",
                "stool",
                "table",
                // ...and the chair, whose row is the stool's four eighths:
                // the back above the seat is the model's.
                "chair",
                // ...and the skeleton, two eighths of bone -- in both of
                // its ids (`BLOCK_BONES_2`).
                "bones",
                "bones_2",
                "bones_3",
                "bones_4",
                // ...and the water barrel, seven eighths of staves in
                // all three of its waters and all three of its grains.
                "barrel",
                "barrel_standing",
                "barrel_salt",
                "barrel_grain",
                "barrel_seeds",
                "barrel_millet",
                "cairn",
                // ...and a pit kiln while it holds only pots (two eighths)
                // or fibre (four), and the firepit, a quarter like the
                // campfire it is on every other rule. See `pit`.
                "pit_kiln",
                "pit_kiln_fibre",
                "firepit",
                "firepit_lit",
                // ...and the four workshops, each at the height its work is
                // done at (`BLOCK_WORKBENCH`).
                "workbench",
                "mason_block",
                "potters_wheel",
                "leather_bench",
                // ...and the three roofing slabs, half a cell each
                // (`BLOCK_TILE_SLAB`). A step is a whole cell tall: its
                // shape is two boxes (`geometry::step_boxes`), not a depth.
                "tile_slab",
                "thatch_slab",
                "branch_slab",
                // ...and the anvil, which stands at the height a smith's
                // hand falls to rather than a cell's.
                "anvil",
                "sawhorse",
                "honing_stone",
                // ...and the lean-to, whose row is still the pallet's two
                // eighths: a hut is a model and collides as its thatch
                // (`lean_to::boxes`, `collision_height`), and the row only
                // keeps it from hiding what it stands beside.
                "lean_to",
                // ...and a dead player: the body half a cell, like the
                // bag it replaced, and what is left of it two eighths,
                // like any other skeleton.
                "corpse",
                "remains",
                // ...and a pit's cover, an eighth of leaves over a hole, and
                // the salt pan's three states, a tray two eighths deep.
                "pit_cover",
                "salt_pan",
                "salt_pan_brine",
                "salt_pan_salt",
            ],
            "the list of blocks that are not a whole block has changed"
        );

        for &(id, name) in ALL_BLOCK_IDS {
            let expected = crate::blocks::definition(id).thickness;
            assert_eq!(block_layers(id), expected, "{name} is the wrong depth");
            assert_eq!(
                block_height(id),
                expected as f32 / LAYERS_PER_BLOCK as f32,
                "{name} is drawn at the wrong height"
            );
            // ...and the *id* still says nothing about it -- for
            // everything but water, whose depth is the one thing the
            // variant field genuinely still means. That is what makes a
            // save written while loose material had layers load: the
            // bits are there and are simply not read.
            if !is_liquid(id) {
                assert_eq!(
                    block_layers(with_layers(id, 3)),
                    block_layers(id),
                    "{name} took a depth out of its id"
                );
            }
        }
    }

    /// **Nothing that stands on the floor stands on a floor that is not
    /// whole.** Everything held from below is drawn from the floor of its
    /// own cell, which is the top of a *whole* block under it; on a slab, a
    /// step, a campfire, a drift, a bitten block or a heap of handfuls that
    /// floor is air, and the thing hangs over it. Asked of every id there
    /// is against one of each.
    #[test]
    fn nothing_held_from_below_stands_on_a_top_that_is_not_whole() {
        let dirt_bitten_on_top = crate::dig::next_bite(BLOCK_DIRT, crate::dig::Side::PosY).expect("dirt bites");
        let dirt_bitten_on_a_side = crate::dig::next_bite(BLOCK_DIRT, crate::dig::Side::PosX).expect("dirt bites");
        let grounds = [
            BLOCK_TILE_SLAB,
            faced(BLOCK_PLANK_STAIRS, Facing::North),
            faced(BLOCK_TILE_ROOF, Facing::East),
            BLOCK_CAMPFIRE,
            dirt_bitten_on_top,
            dirt_bitten_on_a_side,
            crate::dig::heaped(BLOCK_DIRT),
            faced(BLOCK_DOOR, Facing::North),
            BLOCK_PROP,
            BLOCK_WINDOW_LATTICE,
        ];
        let mut wrong = Vec::new();
        for id in 0..=u16::MAX {
            let id = id as BlockId;
            // A lean-to's footing is asked of its whole ground row where it
            // is put down, not cell by cell (see its arm in `can_grow_on`).
            if !is_known_block(id) || !needs_support(id) || support_at(id) != (0, -1, 0) || crate::lean_to::is_lean_to(id) {
                continue;
            }
            for ground in grounds {
                // The top half of a door stands on its own lower half,
                // which is the one thing it stands on (`door_partner`).
                if is_door(id) && is_door(ground) {
                    continue;
                }
                // A flat thing on a top dug down is not drawn from its own
                // floor but on that top (`rest_drop`), so it stands over
                // nothing: snow and ash on a lip, which this list refused
                // until every winter hillside was green -- and a pebble, a
                // flint or a stick on one, which it refused until every
                // stone on a slope kept a step of its own
                // (`worldgen::lips`).
                if is_flat(id) && rest_drop(id, ground) > 0.0 {
                    continue;
                }
                // ...and a thing set down by hand, which is drawn on the slab,
                // the lip or the step's tread at its real height
                // (`geometry::set_down_rest`), not from its own cell's floor.
                if is_set_down(id) && set_down_drop(ground).is_some_and(|drop| drop > 0.0) {
                    continue;
                }
                if can_grow_on(id, ground) {
                    wrong.push(format!("{} ({id}) on {} ({ground})", block_name(id), block_name(ground)));
                }
            }
        }
        wrong.dedup_by(|a, b| a.split(' ').next() == b.split(' ').next());
        assert!(wrong.is_empty(), "these stand over air:\n  {}", wrong.join("\n  "));
    }

    #[test]
    fn a_half_block_is_short_without_being_a_drift() {
        // The two questions `is_partial` used to answer at once. A
        // campfire is half a block tall -- the mesher has to know -- and
        // it is *not* loose material, so nothing about the layer economy
        // applies to it: it does not fall when the ground goes, it is not
        // placed a layer at a time, and it does not need a whole floor
        // under it.
        for id in [BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_BACKPACK] {
            assert!(is_partial(id), "{} fills its cell", block_name(id));
            assert!(!is_loose_layer(id), "{} is a drift", block_name(id));
            assert!(!needs_support(id), "{} falls over", block_name(id));
            assert!(!is_opaque(id), "{} hides its own top face", block_name(id));
            // Still something you walk into rather than through, and
            // still something you can stand on.
            assert!(is_collidable(id), "{} is walked through", block_name(id));
            // How short each one is is its own business -- a bag is half
            // a cell and a fire is a quarter of one (see
            // `blocks::QUARTER_BLOCK`) -- so what this checks is that
            // they are short, not that they agree.
            let height = collision_height(id);
            assert!(
                height > 0.0 && height < 1.0,
                "{} is {height} of a cell",
                block_name(id)
            );
            // Nothing is planted on top of one: its top is inside its
            // own cell, so a tuft standing on it would float.
            assert!(!has_full_top(id), "{} is a floor", block_name(id));
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
        // Sandstone and not boards: boards carry soot in the field now
        // (`wildfire::soot`), so three in a plank's field is a black
        // ceiling and not a depth.
        assert!(!is_known_block(BLOCK_SANDSTONE | (3 << VARIANT_SHIFT)));
    }

    #[test]
    fn a_carcass_half_taken_apart_is_still_a_known_carcass() {
        // The stage of butchering lives in the variant field, and the
        // server writes it into the world after every cut. If the id
        // with a stage in it were not a known block, a mod reading the
        // cell would be told nothing is there and a save would come
        // back full of junk -- see `may_carry_variant`.
        for carcass in [
            BLOCK_CARCASS_HARE,
            BLOCK_CARCASS_DEER,
            BLOCK_CARCASS_BOAR,
            BLOCK_CARCASS_WOLF,
            BLOCK_CARCASS_SHEEP,
            BLOCK_CARCASS_ZEBRA,
            BLOCK_CARCASS_ANTELOPE,
            BLOCK_CARCASS_LION,
            BLOCK_CARCASS_HORSE,
        ] {
            for stage in 0..8u16 {
                let staged = carcass | (stage << VARIANT_SHIFT);
                assert!(is_known_block(staged), "{} at stage {stage}", block_name(carcass));
                assert!(is_carcass(staged));
                assert_eq!(block_kind(staged), carcass);
                // It is still a heap on the ground and not a wall: the
                // stage does not change how tall it is.
                assert_eq!(block_layers(staged), block_layers(carcass));
            }
        }
        // ...and it is a carcass the *player* cannot carry: the block
        // is not placeable, so the anti-cheat refuses a client asking
        // to put one down with a stage of its choosing.
        assert!(!is_placeable(BLOCK_CARCASS_DEER));
    }

    #[test]
    fn every_knife_is_a_knife_and_the_hoe_is_not() {
        for knife in [BLOCK_FLINT_KNIFE, BLOCK_COPPER_KNIFE, BLOCK_BRONZE_KNIFE, BLOCK_IRON_KNIFE] {
            assert!(is_knife(knife), "{}", block_name(knife));
        }
        // The hoe shares the knife's `Work::Plant`, which is the trap
        // `is_knife` is a list to avoid.
        assert!(!is_knife(BLOCK_HOE));
        assert!(!is_knife(BLOCK_WEDGED_AXE));
        assert!(!is_knife(BLOCK_FLINT));
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
        // One kind of log out of every axis: green, as every cut log is.
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::X)), Some(crate::wood::green(BLOCK_LOG)));
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::Z)), Some(crate::wood::green(BLOCK_LOG)));
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
    fn a_chest_placed_from_each_side_faces_its_placer() {
        // A player standing anywhere round a cell puts a chest down on
        // the floor, and its front must step back toward them. Off the
        // quarter lines as well, because nobody places anything looking
        // exactly along an axis -- the worst of these is 39 degrees from
        // one, which is still a front pointing back at the eye.
        for step in 0..16 {
            let yaw = step as f32 * std::f32::consts::TAU / 16.0 + 0.1;
            let chest = placed(BLOCK_CHEST, yaw, (0, 1, 0));
            assert_eq!(block_kind(chest), BLOCK_CHEST);
            assert!(is_known_block(chest), "a chest put down looking along {yaw} is an invented id");
            let (dx, dz) = block_facing(chest).step();
            let toward = dx as f32 * yaw.cos() + dz as f32 * yaw.sin();
            assert!(
                toward < -0.7,
                "a chest put down looking along {yaw} faces {:?}, away from whoever put it there",
                block_facing(chest)
            );
        }
    }

    #[test]
    fn a_stone_block_never_carries_orientation_bits() {
        // Whatever face it is built against and whichever way the player
        // looks: a turned stone would be a second id for one block, and a
        // second slot in the pack the moment the two meet.
        let clicks = [(0, 1, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];
        for click in clicks {
            for step in 0..8 {
                let yaw = step as f32 * std::f32::consts::FRAC_PI_4;
                assert_eq!(placed(BLOCK_STONE, yaw, click), BLOCK_STONE, "{click:?} at {yaw} turned a stone");
            }
        }
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            assert_eq!(faced(BLOCK_STONE, facing), BLOCK_STONE);
        }
        for bits in 1..=3 {
            assert!(
                !is_known_block(BLOCK_STONE | (bits << ORIENTATION_SHIFT)),
                "a turned stone is an id the server would accept"
            );
        }
    }

    #[test]
    fn a_barrel_keeps_its_water_level_whatever_it_is_placed_against() {
        // A barrel is the same on every side, so it does not turn -- and
        // its variant field is how many jugs are in it. The placement rule
        // used to give back the bare kind for anything that does not turn,
        // and the bare kind of a barrel is an empty one.
        use crate::body::Water;
        for water in [Water::Fresh, Water::Standing, Water::Salt] {
            for jugs in 1..=BARREL_JUGS {
                let barrel = barrel_of(water, jugs);
                assert!(!has_front(barrel) && !is_orientable(barrel), "a barrel turns");
                for click in [(0, 1, 0), (1, 0, 0), (0, 0, -1)] {
                    assert_eq!(placed(barrel, 1.0, click), barrel, "{click:?} emptied a barrel");
                }
                assert_eq!(faced(barrel, Facing::East), barrel);
                assert_eq!(oriented(barrel, Axis::X), barrel);
                assert_eq!(barrel_contents(placed(barrel, 2.0, (1, 0, 0))), Some((water, jugs)));
            }
        }
    }

    #[test]
    fn turning_a_block_keeps_the_rest_of_its_variant_field() {
        // The third bit is a rack's skin and a bed's head. Turning either
        // must not take it away.
        let loaded = rack_with_hide(faced(BLOCK_DRYING_RACK, Facing::North), true);
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let turned = faced(loaded, facing);
            assert!(rack_is_loaded(turned), "turning a rack to {facing:?} took the skin off it");
            assert_eq!(block_facing(turned), facing);
            assert!(is_bed_head(faced(bed_half(Facing::North, true), facing)), "a bed's head became its foot");
        }
    }

    #[test]
    fn nothing_spends_its_variant_field_on_both_a_direction_and_what_it_holds() {
        // **The structural half of "orientation must not clobber state".**
        // A direction takes the low two bits of the field; a barrel's water,
        // a jug's river, a carcass's cuts, a skeleton's animal, a branch's
        // width and a leaf's picked fruit take all three. A block that did
        // both would have its contents rewritten by the way it was put
        // down -- so a block that holds something in the field may not
        // turn, and the day one needs to is the day it needs a field of its
        // own rather than a share of this one.
        for &(id, name) in ALL_BLOCK_IDS {
            if !has_front(id) && !is_orientable(id) {
                continue;
            }
            assert!(!may_carry_variant(id), "{name} turns, and keeps its contents in the same bits");
            assert!(!is_branch(id), "{name} turns, and its width is in the same bits");
            assert_ne!(id, BLOCK_APPLE_LEAVES, "{name} turns, and its fruit is in the same bits");
        }
    }

    #[test]
    fn a_bracket_fungus_put_on_any_side_of_a_trunk_hangs_from_that_trunk() {
        // It faces out of the face that was clicked, not toward the
        // placer: "toward the placer" hung an east- or west-clicked shelf
        // from the air on the placer's side, and the support rule refused
        // it -- a bracket went on two of a trunk's four sides.
        for clicked in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)] {
            for step in 0..4 {
                let yaw = step as f32 * std::f32::consts::FRAC_PI_2;
                let shelf = placed(BLOCK_BRACKET_FUNGUS, yaw, clicked);
                assert_eq!(
                    support_at(shelf),
                    (-clicked.0, 0, -clicked.2),
                    "a shelf put against the {clicked:?} face while looking along {yaw} hangs from somewhere else"
                );
            }
        }
    }

    #[test]
    fn a_fire_is_still_facing_the_way_it_was_built_after_it_burns() {
        // **Which way it faces and whether it is alight are one field.**
        // A kiln's front rides in the variant bits (`faced`), and so does
        // nothing else about it -- lit and unlit are two block *kinds*.
        // So the two functions that swap one kind for the other have to
        // carry the other fact across, and for two versions they did not:
        // they matched on `block_kind`, which strips the field, and
        // handed back a bare constant, which is north.
        //
        // What a player saw was a furnace they had set with its mouth
        // toward the door swinging round to face north the instant they
        // struck it alight -- and round again when the charcoal ran out.
        for kind in [BLOCK_KILN, BLOCK_BLOOMERY] {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let cold = faced(kind, facing);
                let alight = lights_into(cold).expect("a kiln can be lit");
                assert!(is_burning(alight), "{} did not catch", block_name(kind));
                assert_eq!(
                    block_facing(alight),
                    facing,
                    "{} turned {:?} when it was lit",
                    block_name(kind),
                    block_facing(alight)
                );
                // ...and all the way back round to where it started.
                assert_eq!(burnt_out(alight), Some(cold));
            }
        }
        // A campfire has no front, so both directions give the plain
        // kind and nothing has to be preserved.
        let lit = lights_into(BLOCK_CAMPFIRE).expect("a campfire can be lit");
        assert_eq!(lit, BLOCK_CAMPFIRE_LIT);
        assert_eq!(burnt_out(lit), Some(BLOCK_CAMPFIRE));
        // Nothing that was never a fire acquires one.
        assert_eq!(lights_into(BLOCK_STONE), None);
        assert_eq!(burnt_out(BLOCK_STONE), None);
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
    fn a_block_turned_to_face_the_player_is_still_a_block_the_game_wrote() {
        // The client picks the facing from the camera yaw and sends the
        // finished id (`lib.rs`, `place_block`), and the server's
        // anti-cheat refuses anything `is_known_block` calls invented.
        // Three of the four facings failing that test meant three out of
        // four chests refused, at three points each of a twelve-point
        // kick -- so building a camp without walking in a circle first
        // was a disconnect.
        for kind in [BLOCK_CHEST, BLOCK_KILN, BLOCK_BLOOMERY, BLOCK_DRYING_RACK] {
            assert!(has_front(kind), "{} has no front to turn", block_name(kind));
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let id = faced(kind, facing);
                assert!(
                    is_known_block(id),
                    "{} facing {facing:?} reads as an invented id",
                    block_name(kind)
                );
                assert_eq!(block_facing(id), facing, "the facing did not survive");
                assert_eq!(block_kind(id), kind, "the kind did not survive");
            }
        }
    }

    #[test]
    fn only_a_rack_may_say_it_has_a_skin_on_it() {
        // `RACK_LOADED` is the spare third bit of the field the facing
        // uses. It is a real id on a rack -- the server writes one -- and
        // meaningless anywhere else, including on the other three blocks
        // that have a front.
        let loaded = rack_with_hide(faced(BLOCK_DRYING_RACK, Facing::East), true);
        assert!(is_known_block(loaded));
        assert!(rack_is_loaded(loaded));
        for kind in [BLOCK_CHEST, BLOCK_KILN, BLOCK_BLOOMERY] {
            let bogus = faced(kind, Facing::East) | RACK_LOADED;
            assert!(
                !is_known_block(bogus),
                "{} accepted the rack's flag",
                block_name(kind)
            );
        }
    }

    #[test]
    fn a_bed_is_two_halves_that_find_each_other_from_either_end() {
        // Placing, breaking and lying down all start from one cell and have
        // to arrive at the same other one. A head that pointed at a cell
        // whose foot pointed somewhere else would be a bed one player can
        // break into a pillow on the floor.
        // ...and the straw pallet is the same two halves: a pallet's head
        // that named a plank foot would be a bed nobody could lie across.
        for kind in [BLOCK_BED, BLOCK_STRAW_BED] {
            for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
                let foot = bed_half_of(kind, facing, false);
                let head = bed_half_of(kind, facing, true);
                assert!(is_known_block(foot) && is_known_block(head), "{kind} {facing:?}");
                assert!(is_bed_head(head) && !is_bed_head(foot));
                assert_eq!(block_kind(head), kind);
                assert_eq!(block_facing(head), facing, "the head forgot which way it lies");
                let at = (10, 30, -4);
                let (head_at, want_head) = bed_partner(at, foot).unwrap();
                assert_eq!(want_head, head);
                assert_eq!(bed_partner(head_at, head), Some((at, foot)), "{kind} {facing:?}");
                // The head is the cell *away* from whoever put the bed down,
                // who stands where the front of the foot looks.
                let (dx, dz) = facing.step();
                assert_eq!((head_at.0 - at.0, head_at.2 - at.2), (-dx, -dz));
            }
        }
        assert_eq!(bed_half(Facing::East, true), bed_half_of(BLOCK_BED, Facing::East, true));
        assert_eq!(bed_partner((0, 0, 0), BLOCK_STOOL), None);
    }

    #[test]
    fn a_door_is_two_halves_that_find_each_other_open_or_shut() {
        // The placement, every break path and the swing all start from the
        // cell clicked and have to arrive at the same other one -- and at the
        // same *state*: a shut top over an open bottom is two halves of two
        // doors, and swinging or breaking one must not take the other.
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            for open in [false, true] {
                let mut lower = faced(BLOCK_DOOR, facing);
                if open {
                    lower = door_swung(lower);
                }
                assert!(is_known_block(lower), "{facing:?} open {open}");
                assert_eq!(door_is_open(lower), open);
                let at = (-3, 40, 7);
                let (top_at, top) = door_partner(at, lower).unwrap();
                assert_eq!(top_at, (at.0, at.1 + 1, at.2), "the top half is not over the bottom");
                assert!(is_known_block(top) && block_kind(top) == BLOCK_DOOR_TOP);
                assert_eq!((block_facing(top), door_is_open(top)), (facing, open), "the top forgot how it hangs");
                assert_eq!(door_partner(top_at, top), Some((at, lower)));
                // Swung, both halves are still each other's.
                assert_eq!(door_partner(at, door_swung(lower)), Some((top_at, door_swung(top))));
                assert_ne!(door_partner(at, lower).map(|(_, b)| b), Some(door_swung(top)));
                // Hung on a floor, and the top on its own bottom only.
                assert!(can_grow_on(lower, BLOCK_STONE) && !can_grow_on(lower, BLOCK_AIR));
                assert!(!can_grow_on(lower, top), "a door hung on a door");
                assert!(can_grow_on(top, lower) && !can_grow_on(top, door_swung(lower)) && !can_grow_on(top, BLOCK_STONE));
            }
        }
        assert_eq!(door_partner((0, 0, 0), BLOCK_BED), None);
        assert_eq!(door_swung(BLOCK_STONE), BLOCK_STONE);
        // The top is never in a pack: it is written by the placement.
        assert!(is_placeable(BLOCK_DOOR) && !is_placeable(BLOCK_DOOR_TOP));
        assert_eq!(block_drop(BLOCK_DOOR_TOP), Some(BLOCK_DOOR));
    }

    #[test]
    fn a_shut_door_walls_a_room_off_and_an_open_one_does_not() {
        // Light, the sky's shelter and a fire's smoke all ask about the door
        // as a wall: shut it is one, open it is a hole. And neither is a
        // cube that hides the faces beside it.
        let shut = faced(BLOCK_DOOR, Facing::East);
        for door in [shut, door_partner((0, 0, 0), shut).unwrap().1] {
            assert_eq!(light_opacity(door), MAX_LIGHT);
            assert!(blocks_the_sky(door));
            assert_eq!(light_opacity(door_swung(door)), 0);
            assert!(!blocks_the_sky(door_swung(door)));
            assert!(!is_opaque(door) && !is_opaque(door_swung(door)));
            assert!(is_collidable(door) && is_collidable(door_swung(door)));
        }
    }

    #[test]
    fn a_facing_steps_toward_the_player_who_put_it_down() {
        // `Facing::step` is stated rather than derived; this is what it has
        // to agree with. A player at the origin looking along a yaw puts a
        // block down one cell ahead, and its front must step back toward
        // them -- against the direction they look.
        for quarter in 0..4 {
            let yaw = quarter as f32 * std::f32::consts::FRAC_PI_2;
            let looking = (yaw.cos().round() as i32, yaw.sin().round() as i32);
            let (dx, dz) = Facing::toward_viewer(yaw).step();
            assert_eq!((dx, dz), (-looking.0, -looking.1), "yaw of {quarter} quarters");
        }
    }

    #[test]
    fn only_a_bed_may_say_it_is_a_head() {
        // The head bit is the rack's skin bit, and each means nothing on
        // the other -- or on anything else with a front.
        for kind in [BLOCK_CHEST, BLOCK_KILN, BLOCK_DRYING_RACK] {
            let bogus = faced(kind, Facing::West) | BED_HEAD;
            if kind == BLOCK_DRYING_RACK {
                assert!(!is_bed_head(bogus), "a rack read as the head of a bed");
            } else {
                assert!(!is_known_block(bogus), "{} accepted the head bit", block_name(kind));
            }
        }
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

/// Things set down by hand (`BLOCK_SET_DOWN`).
#[cfg(test)]
mod set_down_tests {
    use super::*;

    #[test]
    fn a_knife_a_loaf_and_a_bone_are_set_down_and_a_plank_a_raft_and_a_lit_torch_are_not() {
        for thing in [BLOCK_COPPER_KNIFE, BLOCK_BREAD, BLOCK_BONE, BLOCK_COPPER_INGOT] {
            assert!(can_be_set_down(thing), "{} cannot be set down", crate::blocks::definition(thing).name);
        }
        for thing in [BLOCK_PLANKS, BLOCK_RAFT, BLOCK_TORCH_LIT, BLOCK_AIR, BLOCK_SET_DOWN] {
            assert!(!can_be_set_down(thing), "{} can be set down", crate::blocks::definition(thing).name);
        }
    }

    #[test]
    fn a_thing_set_down_is_walked_through_holds_nothing_up_and_needs_a_floor_under_it() {
        let lying = faced(BLOCK_SET_DOWN, Facing::East);
        assert!(is_known_block(lying), "a thing set down facing east is an invented id");
        assert_eq!(block_facing(lying), Facing::East, "the facing did not survive the id");
        assert!(!is_collidable(lying), "a knife on the path is a step");
        assert!(!has_full_top(lying), "something can be put on top of a knife");
        assert!(is_container(lying), "what lies there is not kept in a store");
        assert!(needs_support(lying) && can_grow_on(lying, BLOCK_STONE), "a knife cannot lie on stone");
        assert!(!can_grow_on(lying, BLOCK_AIR), "a knife lies on air");
        assert!(layer_placement(lying, BLOCK_STONE).is_none(), "a block built into the cell deletes the knife");
        assert!(!can_be_displaced_by_falling(lying), "sand falling on a knife deletes it");
        let (low, high) = crate::geometry::block_target_box(lying, 0, 0, 0).expect("a knife nobody can aim at");
        assert!(high[0] - low[0] < 1.0 && high[1] - low[1] <= 0.125, "a knife is aimed at across its whole cell");
    }

    /// **"через шифт можно ставить только на полные блоки".** Setting down
    /// asked `has_full_top`, and once that said no to every partial top a
    /// player could only lay a knife on a bare cube: not on the lip every
    /// slope is edged with, not on a slab, not on a stair. A thing with no
    /// foot lies on any level top at that top's height -- and still not on a
    /// riser, a lattice or a stake, which have no level top at all.
    #[test]
    fn a_set_down_item_on_a_lip_lies_at_the_lips_top() {
        let air = |_: i32, _: i32, _: i32| BLOCK_AIR;
        let lying = faced(BLOCK_SET_DOWN, Facing::North);
        for quarters in 1..=3u8 {
            let lip = crate::dig::lowered(BLOCK_GRASS, quarters);
            let top = crate::dig::left(lip);
            assert!(can_grow_on(lying, lip), "a knife is refused a lip {quarters} quarters down");
            let rest = crate::geometry::set_down_rest(lip, air).expect("a lip is no floor");
            assert_eq!(rest, [0.0, top - 1.0, 0.0], "a knife on {quarters} quarters of lip is not on its top");
            // Aimed at where it lies: the box starts on the lip's top.
            let near = |_: i32, dy: i32, _: i32| if dy == -1 { lip } else { BLOCK_AIR };
            let (min, _) = crate::geometry::block_box_for_aim_near(lying, 0, 1, 0, false, near).expect("unaimable");
            assert!((min[1] - top).abs() < 1e-6, "a knife on a lip is aimed at {} over a top at {top}", min[1]);
        }
        for slab in [BLOCK_TILE_SLAB, BLOCK_THATCH_SLAB, BLOCK_BRANCH_SLAB] {
            assert!(can_grow_on(lying, slab), "a knife is refused a {}", block_name(slab));
            assert_eq!(crate::geometry::set_down_rest(slab, air), Some([0.0, -0.5, 0.0]), "{}", block_name(slab));
        }
        // A step: half a cell down and onto the front half, whichever way it
        // faces -- the tread, never the riser behind it.
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let step = faced(BLOCK_PLANK_STAIRS, facing);
            assert!(can_grow_on(lying, step), "a knife is refused a stair facing {facing:?}");
            let [dx, dy, dz] = crate::geometry::set_down_rest(step, air).expect("a stair is no floor");
            assert_eq!(dy, -0.5, "a knife on a stair is not on its tread");
            let (x, z) = (0.5 + dx, 0.5 + dz);
            let on_riser = crate::geometry::step_boxes(step, air)
                .iter()
                .any(|(min, max)| min[1] >= 0.5 && (min[0]..max[0]).contains(&x) && (min[2]..max[2]).contains(&z));
            assert!(!on_riser, "a knife on a stair facing {facing:?} lies in its riser at ({x}, {z})");
        }
        for nothing in [BLOCK_STAKE, BLOCK_WINDOW_LATTICE, BLOCK_AIR, BLOCK_WATER, BLOCK_CAMPFIRE, BLOCK_TABLE] {
            assert!(!can_grow_on(lying, nothing), "a knife lies on a {}", block_name(nothing));
            assert!(set_down_drop(nothing).is_none(), "a knife lies on a {}", block_name(nothing));
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
    fn a_plant_or_a_stone_set_into_another_is_refused_rather_than_swallowing_it() {
        // The bug: the second one was written over the first, which went
        // nowhere. Both orders, and a solid block still builds through.
        assert_eq!(layer_placement(BLOCK_TALL_GRASS, BLOCK_BERRY_BUSH), None);
        assert_eq!(layer_placement(BLOCK_BERRY_BUSH, BLOCK_TALL_GRASS), None);
        assert_eq!(layer_placement(BLOCK_PEBBLE, BLOCK_PEBBLE), None);
        assert_eq!(layer_placement(BLOCK_TALL_GRASS, BLOCK_PEBBLE), None);
        assert_eq!(layer_placement(BLOCK_TALL_GRASS, BLOCK_COBBLESTONE), Some(Placement::Fresh));
        assert_eq!(layer_placement(BLOCK_AIR, BLOCK_BERRY_BUSH), Some(Placement::Fresh));
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

    /// A block that does not fill its cell does not black it out.
    ///
    /// **The symptom was a dark square on the ground.** A campfire is a
    /// quarter of a cell tall and a backpack half of one, and both
    /// carried `opacity: 15` -- the figure a solid block of stone uses.
    /// Light propagates as `level - (1 + opacity)`, so fifteen puts the
    /// cell at zero whatever is actually standing in it, and the faces
    /// around it are then lit by a black neighbour.
    ///
    /// The lit campfire is what makes it impossible to argue with: it
    /// *emits* thirteen and extinguished all of it, which is a lamp
    /// under a blackout curtain.
    ///
    /// The rule is about filling rather than about these three blocks,
    /// so it is written as one: `is_opaque` already refuses to call a
    /// partial block opaque -- for the same reason, and it says so --
    /// and this is the other half of that answer. Anything that does
    /// not fill its cell has to let light past.
    #[test]
    fn a_block_that_does_not_fill_its_cell_does_not_black_it_out() {
        let mut checked = 0;
        for id in 0..=u8::MAX {
            let id = id as BlockId;
            if is_air(id) {
                continue;
            }
            let def = crate::blocks::definition(id);
            // Only the cube-shaped ones: a cross or a flat sprite is not
            // a thing standing in a cell, and `is_opaque` refuses those
            // by shape before opacity is ever read.
            if def.shape != crate::blocks::Shape::Cube || !is_partial(id) {
                continue;
            }
            checked += 1;
            assert!(
                def.opacity < MAX_LIGHT,
                "{} fills {} of {LAYERS_PER_BLOCK} layers and still stops all light",
                def.name,
                block_layers(id),
            );
            // ...and one that gives light may not be darker than what
            // it gives, which is the campfire's own contradiction.
            if light_emission(id) > 0 {
                assert!(
                    u16::from(def.opacity) < u16::from(light_emission(id)),
                    "{} emits {} and blocks {}",
                    def.name,
                    light_emission(id),
                    def.opacity,
                );
            }
        }
        assert!(checked > 0, "no partial cubes were checked, so nothing was tested");
    }

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
        assert_eq!(block_drop(oriented(BLOCK_LOG, Axis::X)).map(block_kind), Some(BLOCK_LOG));
    }

    #[test]
    fn an_axe_makes_a_forest_into_timber() {
        // What the axe changed: a standing trunk is work rather than
        // scenery. The number is chosen so that felling one costs what
        // pulling a fallen one apart by hand does -- an axe does not make
        // wood cheap, it makes it available.
        let felled_by_hand = break_seconds(oriented(BLOCK_LOG, Axis::X)).unwrap();
        let felled_with_an_axe = break_seconds_with(BLOCK_LOG, Some(BLOCK_WEDGED_AXE)).unwrap();
        assert!((felled_with_an_axe - felled_by_hand).abs() < 0.01);
        // ...and deadfall is still quicker with one in your hand, the
        // way anything with a haft is.
        assert!(
            break_seconds_with(oriented(BLOCK_LOG, Axis::X), Some(BLOCK_WEDGED_AXE)).unwrap()
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
            // "The tool its own tier demands", which is the tool the
            // player necessarily had: the only placeable blocks hands
            // will not shift are ore, and ore comes out of a wall with a
            // pick or not at all. Asked of every pick rather than of the
            // flint one, because iron ore is now gated on copper -- and
            // a player holding iron ore is a player holding a copper
            // pick, which is the whole of what this rule is checking.
            let by_the_tool_that_won_it = crate::blocks::BLOCKS
                .iter()
                .filter(|b| b.tool.is_some())
                .any(|tool| is_breakable_with(id, Some(tool.id)));
            assert!(
                by_hand || by_the_tool_that_won_it,
                "{} can be placed but never removed",
                block_name(id)
            );
        }
    }
}



/// What a death leaves, and what the ground takes out of it.
///
/// The rules are on `BLOCK_CORPSE` and `BLOCK_REMAINS`; these are the
/// three of them a patch could break without noticing.
#[cfg(test)]
mod corpse_tests {
    use super::*;

    /// **A body is not a carcass, and bones are not a body.**
    ///
    /// Both halves have teeth. A corpse that answered `is_carcass` would
    /// be butchered by a knife into meat and hide, which is a different
    /// game; and `BLOCK_REMAINS` inside `rots_where_it_lies` would rot a
    /// second time into air, taking everything filed against that cell
    /// with it.
    #[test]
    fn a_body_rots_where_it_lies_and_what_is_left_of_it_does_not() {
        assert!(!is_carcass(BLOCK_CORPSE), "a body can be butchered");
        assert!(crate::animals::Species::of_carcass(BLOCK_CORPSE).is_none());
        assert!(rots_where_it_lies(BLOCK_CORPSE), "a body waits for ever");
        assert!(!rots_where_it_lies(BLOCK_REMAINS), "the bones rot a second time");
        // Every animal still rots on the same pass -- this is the one
        // question it asks, and a carcass falling out of it would be a
        // deer that keeps for ever again.
        assert!(rots_where_it_lies(crate::animals::carcass_at_stage(
            crate::animals::Species::Deer,
            0
        )));
        // Both states are somewhere a player's things can be, which is
        // what the map and the landmark list mean by "a body".
        assert!(is_corpse(BLOCK_CORPSE) && is_corpse(BLOCK_REMAINS));
        assert!(!is_corpse(BLOCK_BACKPACK), "the old bag reads as a body");
    }

    /// **A body is something to eat and its bones are not.**
    ///
    /// What a hungry wolf with nothing to chase goes to. The first half
    /// is the mechanic: a body lying in a wood is meat, and the animal
    /// standing over it when the player gets back is the reason to hurry.
    /// The second half is what keeps it from becoming furniture -- the
    /// bones stand for as long as the world does, and a scavenger drawn
    /// to them would post a guard on that cell for ever.
    ///
    /// A bag is not food either: the worlds that still have one standing
    /// hold cloth and iron, and a wolf sniffing at luggage is a wolf that
    /// cannot tell a person from a rucksack.
    #[test]
    fn a_body_draws_scavengers_and_bones_and_bags_do_not() {
        assert!(draws_scavengers(BLOCK_CORPSE), "a body in a wood is not worth crossing to");
        assert!(!draws_scavengers(BLOCK_REMAINS), "a wolf crosses a meadow for a skeleton");
        assert!(!draws_scavengers(BLOCK_BACKPACK), "a wolf eats a rucksack");
        assert!(draws_scavengers(crate::animals::carcass_at_stage(
            crate::animals::Species::Deer,
            0
        )));
    }

    /// **The bag is still a block, and it has to be.**
    ///
    /// Nothing makes a new one (`server::leave_corpse` lays a corpse),
    /// but worlds saved before this have bags standing in them with
    /// somebody's iron inside. An id that stopped being defined is a cell
    /// that loads as the missing-block placeholder with its contents
    /// filed against a nothing.
    #[test]
    fn a_backpack_from_an_older_world_still_loads_and_still_opens() {
        assert!(crate::blocks::is_defined(BLOCK_BACKPACK));
        assert!(is_container(BLOCK_BACKPACK), "an old bag cannot be opened any more");
        assert!(is_breakable(BLOCK_BACKPACK), "an old bag cannot be got into");
        assert!(!is_placeable(BLOCK_BACKPACK), "a bag can be stamped into the world");
        // ...and neither of the new two is placeable either, for the
        // same reason: a player who could put a body down could stamp
        // fake graves across a world.
        assert!(!is_placeable(BLOCK_CORPSE) && !is_placeable(BLOCK_REMAINS));
        assert!(is_container(BLOCK_CORPSE) && is_container(BLOCK_REMAINS));
    }

    /// **What the ground takes is the soft half, and the list is the
    /// decision.**
    ///
    /// The first assertion is the guard that matters: it names every
    /// garment in the game, so *adding* one fails this test and makes
    /// somebody decide whether a body left in the rain still has it two
    /// days later. Without it a new leather coat would quietly survive
    /// the grave that a leather tunic does not, and nothing would say so.
    #[test]
    fn the_list_of_things_the_ground_takes_from_a_body_is_the_soft_half() {
        let worn: Vec<&str> = ALL_BLOCK_IDS
            .iter()
            .filter(|&&(id, _)| crate::equipment::is_wearable(id))
            .map(|&(_, name)| name)
            .collect();
        assert_eq!(
            worn,
            [
                "leather_cap",
                "leather_tunic",
                "leather_leggings",
                "leather_boots",
                "bronze_helm",
                "bronze_cuirass",
                "bronze_greaves",
                "bronze_boots",
                "iron_helm",
                "iron_cuirass",
                "iron_greaves",
                "iron_boots",
                "wool_cap",
                "wool_tunic",
                "wool_leggings",
                "wool_boots",
                "fur_hood",
                "fur_cloak",
                "cloth_cap",
                "cloth_tunic",
                "cloth_trousers",
                "cloth_wraps",
                // Worn and not a garment -- see `equipment::slot_of`.
                // It is in this list because the list is built from
                // `is_wearable`, which is the right question: anything
                // a player has *on* has to be decided about, whether or
                // not it keeps them warm.
                "rucksack",
                // The tarred coat outlasts the grave and the snowshoes do
                // not: see `rots_with_a_body`.
                "tarred_tunic",
                "snowshoes",
            ],
            "a garment was added or taken away: decide whether the ground takes it,\
             in `rots_with_a_body`, and then say so here"
        );
        for &(id, name) in ALL_BLOCK_IDS {
            if !crate::equipment::is_wearable(id) {
                continue;
            }
            // **The rucksack is decided above and skipped here.** The
            // rule below infers "soft" from `garment_tint`, and a
            // rucksack has no row there -- deliberately, because that
            // table is read by the systems that decide how warm and how
            // protected a player is, and a bag has nothing to say to
            // either (`equipment::slot_of`). Giving it a row to satisfy
            // an inference would also tint its item picture, which is
            // already drawn in leather.
            if block_kind(id) == BLOCK_RUCKSACK {
                assert!(rots_with_a_body(id), "a hide bag outlasts a hide tunic");
                continue;
            }
            // Skin, fleece and cloth go; metal stays. That is the whole
            // of the trade the mechanic offers -- hurry back for the
            // clothes, take the safe road and keep the harness.
            let metal = garment_tint(id) != Some([0.60, 0.40, 0.24]) // leather
                && garment_tint(id) != Some([0.92, 0.89, 0.82]) // wool
                && garment_tint(id) != Some([0.34, 0.28, 0.24]) // fur
                && garment_tint(id) != Some([0.78, 0.76, 0.68]) // cloth
                && block_kind(id) != BLOCK_SNOWSHOES; // lacing, see `rots_with_a_body`
            assert_eq!(
                rots_with_a_body(id),
                !metal,
                "{name} is on the wrong side of the grave"
            );
        }

        // Food, at every stage of going off, including what it becomes.
        for &(id, name) in ALL_BLOCK_IDS {
            if crate::food::is_food(id) {
                assert!(rots_with_a_body(id), "{name} kept in a body for two days");
            }
        }
        assert!(rots_with_a_body(BLOCK_ROTTEN));
        assert!(rots_with_a_body(crate::food::with_rot_stage(BLOCK_RAW_MEAT, 3)));

        // ...and the things a week of work went into, which is the half
        // that is still there when the player arrives late.
        for keeps in [
            BLOCK_IRON_INGOT,
            BLOCK_IRON_PICKAXE,
            BLOCK_FLINT_KNIFE,
            BLOCK_BRONZE_AXE,
            BLOCK_STONE,
            BLOCK_BONE,
            BLOCK_JUG,
            BLOCK_PLANKS,
            // Wood is *not* soft, and that is deliberate: a log lies in
            // a forest for years, and a stone axe with a rotted handle
            // would be a rule nobody could predict by looking at it.
            BLOCK_LOG,
            BLOCK_STICK,
        ] {
            assert!(
                !rots_with_a_body(keeps),
                "{} rotted in a grave",
                block_name(keeps)
            );
        }

        // An id this has never heard of survives. Wrong in the player's
        // favour on purpose: the failure worth preventing is a patch
        // quietly deleting something valuable because nobody classified
        // it.
        assert!(!rots_with_a_body(60_000));
    }
}

#[cfg(test)]
mod horse_id_tests {
    use super::*;

    /// **The horse's three ids are its own**: under 700, inside the ten bits
    /// of a kind, and named by nothing else in the table. The ids were taken
    /// from the gap after the sundew rather than the lowest free ones (see
    /// `BLOCK_CARCASS_HORSE`), and this is what says they were still free
    /// when that was written -- and what goes red first if a merge lands
    /// another block on one of them.
    #[test]
    fn the_horses_ids_are_its_own() {
        for id in [BLOCK_CARCASS_HORSE, BLOCK_SADDLE, BLOCK_SADDLEBAGS] {
            assert!(id < 700 && id & !KIND_MASK == 0, "{id} is out of a kind's room");
            let named: Vec<&str> = ALL_BLOCK_IDS.iter().filter(|&&(other, _)| other == id).map(|&(_, n)| n).collect();
            assert_eq!(named.len(), 1, "id {id} is named {named:?}");
        }
        // And every id in the table is one block's, which is the rule the
        // three above lean on.
        let mut ids: Vec<BlockId> = ALL_BLOCK_IDS.iter().map(|&(id, _)| id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "two blocks share an id");
    }
}
