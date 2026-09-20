//! The test world: one flat field with every mechanic built on it.
//!
//! ## Why a second generator rather than a seed
//!
//! Everything the game does is checked somewhere -- in a unit test, in
//! an integration test, or by reading the code. What none of those
//! answer is "does the *game* still work", and the only way to ask that
//! is to stand in a world and try things. Doing that in an ordinary
//! world means walking until you find a kiln's worth of clay, a stand of
//! reeds, a cave with tin in it and a boar, which is an evening of play
//! before the first thing under test is in reach.
//!
//! So: a world where all of it is within a minute's walk of spawn,
//! already built, always in the same place. A player picks `TEST` on the
//! new-world screen and lands in the middle of it.
//!
//! ## What it is not
//!
//! It is not a creative mode and not a cheat. Everything here is built
//! out of ordinary blocks, obeys ordinary rules and can be broken and
//! rebuilt like anything else -- the fires burn their fuel down, the
//! crops ripen on the same clock, the sand falls when you dig out from
//! under it. What the preset removes is the *searching*, not the game.
//!
//! ## How it is generated
//!
//! Deterministically, with no noise and no state: `generate_chunk` is a
//! pure function of the chunk's position, exactly like the ordinary
//! generator, so an evicted chunk comes back byte-identical and a
//! player's edits live in the same overlay they always did.
//!
//! The structures are described in *world* coordinates and clipped to
//! whichever chunk is being asked for -- see `Canvas`. That is the whole
//! trick that keeps this readable: a plot is written the way it would be
//! built ("a wall from here to here") rather than as a table of which
//! cells of which chunk hold what, and a building that straddles a seam
//! is one description rather than two halves that can disagree.
//!
//! ## The layout
//!
//! One chunk per plot, in a grid around spawn, so a plot boundary is a
//! chunk boundary and F3 says which plot you are standing in. Spawn is
//! the middle of chunk (0, 0).
//!
//! ```text
//!                                  z-
//!             quarry   tannery   hearths   pottery    bog
//!              ice      snow     layers    falling  wardrobe  woodland
//!   x-  outcrop grove  gallery    SPAWN      house     tower   parlour  x+
//!      thicket shelters water      farm       mine       pen    savanna
//!             collapse workshop  weather    forage   butchery
//!                                  z+
//! ```
//!
//! The outer ring -- outcrop, thicket, woodland, parlour, savanna -- came
//! with the chair: the furniture, hot country and the generator's finds
//! (`worldgen::features`), which had no slot left in the five-by-five.
//!
//! The three plots added in 1.7 are the three things 1.7 added: a
//! **tannery** for hides and racks, a **pottery** for clay and jugs, a
//! **wardrobe** stocked with every garment in the game, and a
//! **weather** yard whose whole purpose is to be four temperatures
//! within twenty paces of each other.
//!
//! Everything outside that block is plain field, and the lanes between
//! the plots run down the chunk boundaries -- see `lanes`.

// `CHEST_SLOTS` and not the player's `SLOTS`: these are chests in the
// world, and a chest stayed forty squares when the pack was halved (see
// `inventory::CHEST_SLOTS`). Stocking them a pack at a time would have
// wanted twice as many chests as the gallery has drawn.
use crate::inventory::{Inventory, Stack, CHEST_SLOTS, MAX_STACK};
use crate::types::Facing;
use crate::types::{
    can_grow_on, oriented, with_layers, Axis, BlockId, Chunk, ChunkPos, ALL_BLOCK_IDS, BLOCK_AIR,
    BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT, BLOCK_APPLE_LEAVES_PICKED, BLOCK_ASH, BLOCK_BARE_BUSH, BLOCK_BIRCH_LEAVES,
    BLOCK_BIRCH_LOG, BLOCK_BLOOMERY, BLOCK_PEG, BLOCK_PEGGED_PLANKS,
    BLOCK_BLOOMERY_LIT, BLOCK_BRICKS, BLOCK_CACTUS, BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT,
    BLOCK_CHEST, BLOCK_CLAY, BLOCK_COAL_ORE, BLOCK_COBBLESTONE, BLOCK_COPPER_ORE, BLOCK_DIRT,
    BLOCK_FARMLAND, BLOCK_FLINT, BLOCK_FLOWER, BLOCK_GLOWSTONE, BLOCK_GRASS, BLOCK_GRAVEL,
    BLOCK_ICE, BLOCK_IRON_ORE, BLOCK_KILN, BLOCK_KILN_LIT, BLOCK_LEAVES, BLOCK_LOG,
    BLOCK_MUSHROOM, BLOCK_NATIVE_COPPER, BLOCK_PEBBLE, BLOCK_PLANKS, BLOCK_REEDS, BLOCK_SAND,
    BLOCK_SEEDS, BLOCK_SNOW, BLOCK_STICK, BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_TIN_ORE,
    BLOCK_WATER, BLOCK_WHEAT, BLOCK_WHEAT_RIPE, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
    CHUNK_VOLUME, PLACEABLE_BLOCKS,
    // 1.5: the tinder bracket, wild cereal, the nest, and what a room
    // is furnished with.
    BLOCK_BRACKET_FUNGUS, BLOCK_NEST_EGGS, BLOCK_STOOL, BLOCK_STRAW_BED,
    BLOCK_TABLE, BLOCK_WILD_WHEAT,
    // Cotton, and what frost leaves of a field.
    BLOCK_COTTON_PLANT, BLOCK_COTTON_RIPE, BLOCK_COTTON_SEEDS, BLOCK_WILD_COTTON,
    BLOCK_WITHERED_CROP,
    // Water kept by the field.
    barrel_of, BLOCK_BARREL,
    // 1.7: the tannery, the jug, and what a person wears.
    BLOCK_BIRCH_PLANKS, BLOCK_BRONZE_BOOTS, BLOCK_BRONZE_CUIRASS, BLOCK_BRONZE_GREAVES,
    BLOCK_BRONZE_HELM, BLOCK_DRYING_RACK, BLOCK_HIDE, BLOCK_IRON_BOOTS, BLOCK_IRON_CUIRASS,
    BLOCK_IRON_GREAVES, BLOCK_IRON_HELM, BLOCK_JUG, BLOCK_JUG_WATER, BLOCK_LEATHER,
    BLOCK_LEATHER_BOOTS, BLOCK_LEATHER_CAP, BLOCK_LEATHER_LEGGINGS, BLOCK_LEATHER_TUNIC,
    // 1.8: what there is to forage, and the one thing to get wrong.
    BLOCK_ROOTS, BLOCK_TOADSTOOL,
    BLOCK_SANDSTONE, BLOCK_LIMESTONE, BLOCK_GRANITE, BLOCK_PEAT, BLOCK_DRIED_PEAT, BLOCK_RUSTY_STONE, BLOCK_IRON_DUST, BLOCK_CORD, BLOCK_SINEW, BLOCK_BONE, BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, BLOCK_FLINT_SPEAR, BLOCK_STONE_AXE, BLOCK_STONE_PICKAXE, BLOCK_STONE_AXE_HEAD, BLOCK_STONE_PICK_HEAD, BLOCK_FLINT_KNIFE_HEAD, BLOCK_FLINT_KNIFE, BLOCK_FLINT_FLAKE, BLOCK_WORKED_STICK, BLOCK_FIBER, BLOCK_COAL, BLOCK_RAW_MEAT,
};

/// The top of the field: the last solid cell, so a player stands at
/// `GROUND_Y + 1`.
///
/// Four above sea level, which is the one number this shares with the
/// ordinary world: a pool dug down to sea level here is the same pool as
/// one dug there, and the fluid simulation needs no telling which world
/// it is running in.
pub const GROUND_Y: i32 = crate::worldgen::SEA_LEVEL + 4;

/// How much soil sits over the rock.
const SOIL_DEPTH: i32 = 4;

/// The bedrock floor, matching the ordinary generator's: nothing opens
/// into the void.
const BEDROCK_TOP: i32 = 2;

/// How far out the built plots reach, in chunks.
///
/// Everything past this is plain field. A bound rather than an
/// open-ended script, because the build pass is run for *every* chunk
/// the world is asked for and a player walking east for ten minutes
/// should not keep paying for plots they left behind -- see
/// `generate_chunk`, which skips the pass entirely outside this square.
///
/// **Three, and it was two until every slot of the five-by-five held a
/// plot.** The furniture, the savanna and the generator's finds arrived
/// with nowhere to stand, and a thing that exists only in a chest is a
/// thing nobody sees from across a field. The ring this adds is
/// twenty-four slots, most of them still open field.
const PLOT_RADIUS: i32 = 3;

/// Half the width of a plot's built area, which leaves a lane of open
/// field between neighbours.
const PLOT_HALF: i32 = 6;

/// The floor of the mine, in world coordinates. Deep enough to be
/// underground and shallow enough that the stair is a walk.
const MINE_FLOOR: i32 = 8;

/// One turn of a spiral stair, as offsets from the middle of a
/// five-by-five shaft.
///
/// **Every cell is a neighbour of the one before it.** That is the whole
/// requirement and it is easy to miss: a ring of corner posts two cells
/// apart looks like a spiral on paper and is a series of standing jumps
/// in the world, half of them diagonal. One cell along and one cell down
/// is a stair; two cells along is a gap.
const SPIRAL: [(i32, i32); 16] = [
    (-2, -2),
    (-1, -2),
    (0, -2),
    (1, -2),
    (2, -2),
    (2, -1),
    (2, 0),
    (2, 1),
    (2, 2),
    (1, 2),
    (0, 2),
    (-1, 2),
    (-2, 2),
    (-2, 1),
    (-2, 0),
    (-2, -1),
];

/// Every chunk the plots are drawn in.
///
/// What the server needs to know to go looking for things the world was
/// *born* holding -- the lit hearths, which are blocks rather than
/// mechanics until something registers them. Outside this square the
/// build pass never runs, so there is nothing there to find.
pub fn built_chunks() -> Vec<ChunkPos> {
    let mut chunks = Vec::new();
    for cx in -PLOT_RADIUS..=PLOT_RADIUS {
        for cz in -PLOT_RADIUS..=PLOT_RADIUS {
            chunks.push(ChunkPos::new(cx, cz));
        }
    }
    chunks
}

/// Where a player is put down: the middle of the spawn plaza.
pub fn spawn_column() -> (i32, i32) {
    // **`PRIMITIVE_TEST_SPAWN=x,z` puts a new player somewhere else.**
    // A dev hook, on the same terms as `PRIMITIVE_AUTOSTART`: a fault
    // reported at the far corner of the test world can only be
    // photographed unattended if the camera can be put there, and the
    // client has no way to walk. Read once, when a player is first
    // placed; an ordinary world never asks.
    if let Some((x, z)) = std::env::var("PRIMITIVE_TEST_SPAWN").ok().and_then(|raw| {
        let (x, z) = raw.split_once(',')?;
        Some((x.trim().parse::<i32>().ok()?, z.trim().parse::<i32>().ok()?))
    }) {
        return (x, z);
    }
    (8, 8)
}

/// What the weather has to work with here.
///
/// A fixed temperate, moderately damp climate, so the whole field is one
/// green and the eye can tell a texture apart from a tint. In an
/// ordinary world these come out of noise and shift from step to step,
/// which is right there and wrong in a room built for comparing things
/// side by side.
pub fn climate() -> (f32, f32) {
    (0.55, 0.55)
}

// ---- the canvas ----

/// One chunk, written through world coordinates.
///
/// Every plot below is written as if it had the whole world to draw on;
/// this clips each cell to the chunk being generated and drops the rest.
struct Canvas<'a> {
    blocks: &'a mut [BlockId],
    origin_x: i32,
    origin_z: i32,
}

impl Canvas<'_> {
    /// One cell, if it is in this chunk and inside the world.
    fn set(&mut self, gx: i32, gy: i32, gz: i32, id: BlockId) {
        let lx = gx - self.origin_x;
        let lz = gz - self.origin_z;
        if !(0..CHUNK_SIZE_X as i32).contains(&lx)
            || !(0..CHUNK_SIZE_Z as i32).contains(&lz)
            || !(0..CHUNK_SIZE_Y as i32).contains(&gy)
        {
            return;
        }
        self.blocks[Chunk::index(lx as usize, gy as usize, lz as usize)] = id;
    }

    /// An inclusive box, written the way a wall is described rather than
    /// as three nested loops at every call site.
    fn fill(&mut self, from: (i32, i32, i32), to: (i32, i32, i32), id: BlockId) {
        for y in from.1.min(to.1)..=from.1.max(to.1) {
            for z in from.2.min(to.2)..=from.2.max(to.2) {
                for x in from.0.min(to.0)..=from.0.max(to.0) {
                    self.set(x, y, z, id);
                }
            }
        }
    }

    /// The four walls of a box, without its floor or its ceiling.
    fn walls(&mut self, from: (i32, i32, i32), to: (i32, i32, i32), id: BlockId) {
        let (x0, x1) = (from.0.min(to.0), from.0.max(to.0));
        let (z0, z1) = (from.2.min(to.2), from.2.max(to.2));
        self.fill((x0, from.1, z0), (x1, to.1, z0), id);
        self.fill((x0, from.1, z1), (x1, to.1, z1), id);
        self.fill((x0, from.1, z0), (x0, to.1, z1), id);
        self.fill((x1, from.1, z0), (x1, to.1, z1), id);
    }

    /// A column standing on the field.
    fn column(&mut self, x: i32, z: i32, height: i32, id: BlockId) {
        self.fill((x, GROUND_Y + 1, z), (x, GROUND_Y + height, z), id);
    }
}

// ---- generation ----

/// The whole world, one chunk at a time.
pub fn generate_chunk(pos: ChunkPos) -> Chunk {
    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;

    // The field: bedrock, rock, soil, turf. No noise anywhere -- a test
    // world whose ground was not level is a test world where you cannot
    // tell a slope from a bug.
    for lz in 0..CHUNK_SIZE_Z {
        for lx in 0..CHUNK_SIZE_X {
            for y in 0..=GROUND_Y {
                let id = if y <= BEDROCK_TOP {
                    BLOCK_STONE
                } else if y == GROUND_Y {
                    BLOCK_GRASS
                } else if y > GROUND_Y - SOIL_DEPTH {
                    BLOCK_DIRT
                } else {
                    BLOCK_STONE
                };
                blocks[Chunk::index(lx, y as usize, lz)] = id;
            }
        }
    }

    // Outside the built square there is nothing to draw, and saying so
    // here is what keeps a distant chunk as cheap as the field above.
    if pos.x.abs() <= PLOT_RADIUS && pos.z.abs() <= PLOT_RADIUS {
        let mut canvas = Canvas {
            blocks: &mut blocks,
            origin_x,
            origin_z,
        };
        // Every plot, not only this chunk's own: a structure may reach
        // over a seam and the canvas drops whatever misses.
        build_all(&mut canvas);
    }

    Chunk { pos, blocks }
}

/// Where a plot's middle is, in world coordinates.
fn plot(cx: i32, cz: i32) -> (i32, i32) {
    (
        cx * CHUNK_SIZE_X as i32 + CHUNK_SIZE_X as i32 / 2,
        cz * CHUNK_SIZE_Z as i32 + CHUNK_SIZE_Z as i32 / 2,
    )
}

fn build_all(c: &mut Canvas) {
    // The lanes first, so that a plot laid over one wins: a path is
    // what runs *between* the plots, and a strip of gravel across the
    // plaza would be a path through a room.
    lanes(c);
    plaza(c, plot(0, 0));
    house(c, plot(1, 0));
    gallery(c, plot(-1, 0));
    ground_gallery(c, plot(-3, 2));
    layers_and_steps(c, plot(0, -1));
    mine(c, plot(1, 1));
    falling(c, plot(1, -1));
    snowfield(c, plot(-1, -1));
    ice_rink(c, plot(-2, -1));
    water(c, plot(-1, 1));
    farm(c, plot(0, 1));
    hearths(c, plot(0, -2));
    tower(c, plot(2, 0));
    grove(c, plot(-2, 0));
    pen(c, plot(2, 1));
    tannery(c, plot(-1, -2));
    pottery(c, plot(1, -2));
    wardrobe(c, plot(2, -1));
    weather_yard(c, plot(0, 2));
    forage(c, plot(1, 2));
    // The six plots that were empty until the world learned rock,
    // peat, carcasses, collapses, cold nights and the honest tool chain.
    quarry(c, plot(-2, -2));
    bog(c, plot(2, -2));
    butchery(c, plot(2, 2));
    collapse_yard(c, plot(-2, 2));
    shelter_yard(c, plot(-2, 1));
    workshop(c, plot(-1, 2));
    // The ring round the square: the chair and the rest of the furniture,
    // hot country, and the finds the generator lays out. See the section
    // note above `parlour`.
    parlour(c, plot(3, 0));
    savanna(c, plot(3, 1));
    woodland(c, plot(3, -1));
    outcrop(c, plot(-3, 0));
    thicket(c, plot(-3, 1));
    // The sea floor in a basin: kelp, a reef, seagrass and shells.
    aquarium(c, plot(0, 3));
    // A palm on a strip of beach, and a patch of swamp beside it.
    coast_and_marsh(c, plot(-3, -1));
    // North of the hearths: the fires in the ground. See `fire_pits`.
    fire_pits(c, plot(0, -3));
}

/// **Fires in the ground**: the pit kiln at every stage the player's rules
/// name, a charcoal pit, and a firepit -- see `pit`.
///
/// A row of five pits dug one deep into the turf, west to east: an empty
/// pit to start from, two raw pots on the floor of the next, eight fibre over
/// them in the third, eight logs over that in the fourth -- ready for the
/// flint -- and the fifth alight under a roof two cells up, so the rain
/// cannot put it out before anybody arrives (a roof one up would be a lid,
/// and smother it). What is in each is `pit_kiln_stock`.
///
/// Behind the row, a charcoal pit of two piles of eight, open on top and
/// ready to strike, and a heap of charcoal already burnt through in a pit
/// of its own. In front, a firepit alight on the grass. The chest at the
/// east end has what building every stage again takes.
fn fire_pits(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::pit::{charcoal_pile, log_pile, Stage};
    let y = GROUND_Y;
    let stages = [
        None,
        Some(Stage::Pottery { pieces: 2, fired: false }),
        Some(Stage::Fibre(8)),
        Some(Stage::Logs(8)),
        Some(Stage::Burning),
    ];
    for (i, stage) in stages.into_iter().enumerate() {
        c.set(x - 4 + 2 * i as i32, y, z - 2, stage.map_or(BLOCK_AIR, Stage::block));
    }
    // The roof over the burning kiln: four posts on the corners of its
    // walls and a slab of cobble two cells over the pit.
    let (bx, bz) = (x + 4, z - 2);
    for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
        c.set(bx + dx, y + 1, bz + dz, BLOCK_COBBLESTONE);
    }
    c.fill((bx - 1, y + 2, bz - 1), (bx + 1, y + 2, bz + 1), BLOCK_COBBLESTONE);

    // The charcoal pit, two piles side by side in one trench.
    c.set(x - 3, y, z + 2, log_pile(8));
    c.set(x - 2, y, z + 2, log_pile(8));
    // ...and what one leaves.
    c.set(x + 1, y, z + 2, charcoal_pile(4));
    // The firepit, on the ground and alight.
    c.set(x + 3, y + 1, z + 2, crate::types::BLOCK_FIREPIT_LIT);
    let (cx, cy, cz) = fire_pits_chest();
    c.set(cx, cy, cz, BLOCK_CHEST);
}

/// Where the fire pits' chest stands.
fn fire_pits_chest() -> (i32, i32, i32) {
    let (x, z) = plot(0, -3);
    (x + 5, GROUND_Y + 1, z)
}

/// What is in each of the test world's pit kilns: two raw pots apiece, in
/// every kiln but the empty pit. The server puts them there on the first
/// run of a test world, the way it stocks the chests.
pub fn pit_kiln_stock() -> Vec<((i32, i32, i32), Vec<BlockId>)> {
    let (x, z) = plot(0, -3);
    (1..5)
        .map(|i| {
            (
                (x - 4 + 2 * i, GROUND_Y, z - 2),
                vec![crate::types::BLOCK_VESSEL_RAW, crate::types::BLOCK_JUG_RAW],
            )
        })
        .collect()
}

/// **The warm coast and the marsh.** A palm on a strip of beach, drawn by
/// the generator's own `worldgen::palm_cells` so it is the shape the world
/// grows, with its coconuts under the crown -- picked by a right click, or
/// shaken down by an empty hand on the trunk. Beside it a patch of swamp as
/// the generator lays one: pools a cell deep between hummocks of mud, a hole
/// two deep, lily pads on the still water, reeds at the edges, a drowned snag
/// standing in a pool, and a low tree with moss hanging under its crown. The
/// field's own climate is temperate, so the coconuts here set fruit only as
/// fast as a warm day allows (`growth::COCONUT_SET_C`).
fn coast_and_marsh(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::{branch, BLOCK_HANGING_MOSS, BLOCK_LILY_PAD, BLOCK_MUD};
    // The beach, and a palm on it leaning south, its crown inside the plot.
    c.fill((x - 6, GROUND_Y - 2, z - 6), (x - 1, GROUND_Y, z + 6), BLOCK_SAND);
    for ((dx, dy, dz), id) in crate::worldgen::palm_cells(0x0802, (0, 1)) {
        c.set(x - 3 + dx, GROUND_Y + dy, z - 2 + dz, id);
    }
    // The swamp: mud to the surface, pools one deep with mud under them.
    c.fill((x, GROUND_Y - 1, z - 6), (x + 6, GROUND_Y, z + 6), BLOCK_MUD);
    for (px, pz) in [(2, -4), (3, -4), (2, -3), (3, -3), (4, -3), (5, 0), (5, 1), (4, 1)] {
        c.set(x + px, GROUND_Y, z + pz, BLOCK_WATER);
    }
    // The hole a player goes into up to the neck.
    c.set(x + 5, GROUND_Y - 1, z + 4, BLOCK_WATER);
    c.set(x + 5, GROUND_Y, z + 4, BLOCK_WATER);
    for (px, pz) in [(2, -4), (4, -3), (5, 1)] {
        c.set(x + px, GROUND_Y + 1, z + pz, BLOCK_LILY_PAD);
    }
    for (px, pz) in [(1, -4), (1, -3), (4, -4), (6, 0), (3, 1)] {
        c.set(x + px, GROUND_Y + 1, z + pz, BLOCK_REEDS);
    }
    // A drowned snag in the pool: a bough thinning to a twig, its foot in
    // the water the way the generator stands one (`types::BLOCK_DROWNED_BOUGH`).
    for (dy, width) in [(0, 12), (1, 10), (2, 8), (3, 4)] {
        let piece = if dy == 0 { crate::types::drowned(branch(width)) } else { branch(width) };
        c.set(x + 3, GROUND_Y + dy, z - 3, piece);
    }
    // A low swamp tree with moss hanging from its crown.
    c.column(x + 2, z + 4, 3, BLOCK_LOG);
    c.fill((x + 1, GROUND_Y + 4, z + 3), (x + 3, GROUND_Y + 4, z + 5), BLOCK_LEAVES);
    c.set(x + 2, GROUND_Y + 5, z + 4, BLOCK_LEAVES);
    for (px, pz) in [(1, 3), (3, 5), (1, 5)] {
        c.set(x + px, GROUND_Y + 3, z + pz, BLOCK_HANGING_MOSS);
    }
    c.set(x + 1, GROUND_Y + 2, z + 3, BLOCK_HANGING_MOSS);
}

/// **The aquarium.** A basin eight deep cut into the field and floored the
/// way the generator floors a sea (`WorldGen::place_seabed`): a kelp forest
/// in the south-west, a reef of both corals with fans and staghorn on it in
/// the north-east, a seagrass meadow on a shallow shelf along the south,
/// shells on the open sand between, and a boulder. The
/// north rim has a step down into the water.
///
/// Built rather than found because a real kelp forest and a real reef are
/// a climate apart -- one off a birch wood, the other off a savanna -- and
/// the whole of the sea floor has to be in one swim here. **The fish are not
/// placed**: the spawner puts a school in any water deep enough near a
/// player (`logic::animals::populate_water`), and a basin this size is found
/// in a minute or two of standing beside it.
fn aquarium(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::{
        BLOCK_BRAIN_CORAL, BLOCK_FIRE_CORAL, BLOCK_KELP, BLOCK_KELP_TOP, BLOCK_SEAGRASS,
        BLOCK_SEA_FAN, BLOCK_SHELL, BLOCK_STAGHORN_CORAL,
    };
    let floor = GROUND_Y - 8;
    let surface = GROUND_Y - 1;
    // The basin: stone walls and floor so nothing drains, water to one
    // below the rim, sand over the floor.
    c.fill((x - 6, floor - 1, z - 6), (x + 6, GROUND_Y, z + 6), BLOCK_STONE);
    c.fill((x - 5, floor + 1, z - 5), (x + 5, GROUND_Y, z + 5), BLOCK_AIR);
    c.fill((x - 5, floor, z - 5), (x + 5, floor, z + 5), BLOCK_SAND);
    c.fill((x - 5, floor + 1, z - 5), (x + 5, surface, z + 5), BLOCK_WATER);
    // The shelf along the south: sand three below the surface, for the
    // meadow.
    c.fill((x - 5, floor + 1, z + 4), (x + 5, surface - 3, z + 5), BLOCK_SAND);
    for dx in -5i32..=5 {
        for dz in 4i32..=5 {
            if (dx + dz).rem_euclid(3) != 0 {
                c.set(x + dx, surface - 2, z + dz, BLOCK_SEAGRASS);
            }
        }
    }
    // Kelp: stems of four to six with a top, in the south-west quarter.
    for (dx, dz, stem) in [(-4, 1, 6), (-3, 3, 5), (-2, 1, 4), (-4, -1, 5), (-1, 2, 6), (-3, -1, 4)] {
        for y in floor + 1..floor + stem {
            c.set(x + dx, y, z + dz, BLOCK_KELP);
        }
        c.set(x + dx, floor + stem, z + dz, BLOCK_KELP_TOP);
    }
    // The reef: a lumpy mound of both corals, and what grows on top of it.
    for (dx, dz, tall, stone, crown) in [
        (2, -4, 2, BLOCK_BRAIN_CORAL, Some(BLOCK_SEA_FAN)),
        (3, -4, 3, BLOCK_BRAIN_CORAL, None),
        (4, -3, 1, BLOCK_FIRE_CORAL, Some(BLOCK_STAGHORN_CORAL)),
        (3, -2, 2, BLOCK_FIRE_CORAL, Some(BLOCK_SEA_FAN)),
        (4, -4, 2, BLOCK_BRAIN_CORAL, Some(BLOCK_STAGHORN_CORAL)),
        (2, -2, 1, BLOCK_BRAIN_CORAL, Some(BLOCK_STAGHORN_CORAL)),
        (5, -2, 1, BLOCK_FIRE_CORAL, Some(BLOCK_SEA_FAN)),
    ] {
        c.fill((x + dx, floor + 1, z + dz), (x + dx, floor + tall, z + dz), stone);
        if let Some(crown) = crown {
            c.set(x + dx, floor + tall + 1, z + dz, crown);
        }
    }
    // A boulder and shells on the open sand. No sand waves: the generator
    // stopped laying them (`WorldGen::place_seabed`).
    c.fill((x + 3, floor + 1, z + 1), (x + 4, floor + 1, z + 2), BLOCK_COBBLESTONE);
    c.set(x + 3, floor + 2, z + 1, BLOCK_COBBLESTONE);
    for (dx, dz) in [(0, -1), (-1, -4), (2, 3), (0, 3)] {
        c.set(x + dx, floor + 1, z + dz, BLOCK_SHELL);
    }
    // A way in: a step cut down from the north rim.
    c.set(x, GROUND_Y, z - 6, BLOCK_AIR);
    c.set(x, GROUND_Y - 1, z - 6, BLOCK_STONE);
}

/// **The quarry.** The three rocks under the soil, cut as a stepped
/// face so all three are in one look: sandstone at the front and
/// lowest, limestone behind and higher with flint nodules on its ledge,
/// granite at the back and highest. A player with a lashed pick learns
/// here which of them it will not bite (`Tier::Stone` -- none of them),
/// and with a glued one which needs copper (granite). Along the south
/// edge, a riverbank in miniature: a closed trench of water, a gravel
/// bank, and rusty stones on it at the spacing the generator uses --
/// the bog iron a player would otherwise walk a river to find.
fn quarry(c: &mut Canvas, (x, z): (i32, i32)) {
    // Three terraces, each a full-width band, rising away from spawn.
    c.fill((x - 6, GROUND_Y + 1, z - 6), (x + 6, GROUND_Y + 3, z - 4), BLOCK_SANDSTONE);
    c.fill((x - 6, GROUND_Y + 1, z - 3), (x + 6, GROUND_Y + 5, z - 1), BLOCK_LIMESTONE);
    c.fill((x - 6, GROUND_Y + 1, z), (x + 6, GROUND_Y + 7, z + 2), BLOCK_GRANITE);
    // Flint where it forms: on the limestone ledge, one every other cell.
    for dx in (-5..=5).step_by(2) {
        c.set(x + dx, GROUND_Y + 6, z - 2, BLOCK_FLINT);
    }
    // ...and the same ledge on the sandstone, so the contrast is a step
    // away: pebbles, which are what a stone head is ground from.
    for dx in (-4..=4).step_by(2) {
        c.set(x + dx, GROUND_Y + 4, z - 5, BLOCK_PEBBLE);
    }
    // The bank: a trench of water closed by the turf around it, a strip
    // of gravel beside it, rusty stones on the gravel.
    c.fill((x - 5, GROUND_Y, z + 5), (x + 5, GROUND_Y, z + 5), BLOCK_WATER);
    c.fill((x - 6, GROUND_Y, z + 4), (x + 6, GROUND_Y, z + 4), BLOCK_GRAVEL);
    for dx in [-5, -2, 1, 4] {
        c.set(x + dx, GROUND_Y + 1, z + 4, BLOCK_RUSTY_STONE);
    }
}

/// **The bog.** Peat, two to four layers under the turf as the
/// generator lays it, with the eastern half of the turf stripped so the
/// peat is at the surface where it is dug; a puddle ringed with clay and
/// reeds -- the fibre country -- and rusty stones on the wet peat, thick
/// as they lie in a bog. A rack stands at the edge with a sod of peat
/// already on it (`rack_stock`), so the drying can be watched rather
/// than waited for.
fn bog(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill((x - 6, GROUND_Y - 3, z - 6), (x + 6, GROUND_Y - 1, z + 6), BLOCK_PEAT);
    c.fill((x, GROUND_Y, z - 6), (x + 6, GROUND_Y, z + 6), BLOCK_PEAT);
    // The puddle: closed by the peat around it, a reed bank of clay on
    // its west side where the turf still is.
    c.fill((x + 2, GROUND_Y, z - 1), (x + 3, GROUND_Y, z + 1), BLOCK_WATER);
    c.fill((x + 1, GROUND_Y, z - 2), (x + 1, GROUND_Y, z + 2), BLOCK_CLAY);
    for dz in -2..=2 {
        c.set(x + 1, GROUND_Y + 1, z + dz, BLOCK_REEDS);
    }
    for (dx, dz) in [(4, -4), (5, 3), (2, 4), (5, -1)] {
        c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_RUSTY_STONE);
    }
    rack(c, (x - 4, GROUND_Y + 1, z + 4), Facing::North);
    c.set(x - 4, GROUND_Y + 1, z - 4, BLOCK_CHEST);
}

/// Where the bog's chest stands: a shovel's worth of what the bog is for.
fn bog_chest() -> (i32, i32, i32) {
    let (x, z) = plot(2, -2);
    (x - 4, GROUND_Y + 1, z - 4)
}

/// **The butchery.** The five carcasses in a row, whole, and the same
/// five in a second row with the first cut made -- so a player sees the
/// animal, the skinned animal and, for the sheep, the fleeced one
/// without having to kill anything first. The hunter's chest beside
/// them holds the knife, the spear and an axe, which is the whole
/// decision the carcass exists to pose (`Species::butchering`).
fn butchery(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::animals::{carcass_at_stage, Species};
    c.fill((x - 6, GROUND_Y, z - 6), (x + 6, GROUND_Y, z + 6), BLOCK_COBBLESTONE);
    // **Five to a row, two rows per stage.** One row of seven, three cells
    // apart, already ran seven cells past the east edge of the pad; ten in a
    // row would have laid the savanna's three in the lane. Five three apart
    // is the pad's thirteen cells exactly, and a carcass lies about two
    // long, so three apart is as close as two can lie without touching.
    // Only the animals that leave a body: a fish gives itself where it dies
    // (`Species::carcass`), and the air its "carcass" would be would clear a
    // cell of the row it landed in.
    for (i, species) in Species::ALL.iter().filter(|s| s.carcass().is_some()).enumerate() {
        let bx = x - 6 + (i % 5) as i32 * 3;
        let row = (i / 5) as i32 * 3;
        c.set(bx, GROUND_Y + 1, z - 5 + row, carcass_at_stage(*species, 0));
        c.set(bx, GROUND_Y + 1, z + 1 + row, carcass_at_stage(*species, 1));
    }
    c.set(x - 6, GROUND_Y + 1, z + 6, BLOCK_CHEST);
    c.set(x + 6, GROUND_Y + 1, z + 6, BLOCK_CAMPFIRE);
}

fn hunters_chest() -> (i32, i32, i32) {
    let (x, z) = plot(2, 2);
    (x - 6, GROUND_Y + 1, z + 6)
}

/// **The collapse yard.** A hill of rock with three ways in, each one
/// lesson from `falling::is_unsupported`:
///
/// * a tunnel two wide in stone, which holds whatever you do in it;
/// * a gallery five wide in stone with one pillar left standing in the
///   middle -- dig the pillar and the roof over it comes down, and the
///   roof beside that, out to the cap;
/// * a gallery four wide in *earth*, which would fall on its own at
///   three, held up by logs standing floor to ceiling every third cell.
///   Pull a log.
///
/// ...and, on the hill's flat top, the lesson that is about what a
/// *player* builds rather than what they dig: two identical plank roofs
/// nine across on one post each, one of them pegged over the post.
/// Knock the post out from under either and the difference is the whole
/// of `types::BLOCK_PEG`.
///
/// Nothing here falls until somebody digs: the whole hill is generated
/// terrain, and the mechanic looks only at cells over a *removed* block.
fn collapse_yard(c: &mut Canvas, (x, z): (i32, i32)) {
    let roof = GROUND_Y + 3;
    let top = GROUND_Y + 5;
    c.fill((x - 6, GROUND_Y + 1, z - 6), (x + 1, top, z + 6), BLOCK_STONE);
    c.fill((x + 2, GROUND_Y + 1, z - 6), (x + 6, top, z + 6), BLOCK_DIRT);
    // The narrow tunnel, open at the south face.
    c.fill((x - 6, GROUND_Y + 1, z - 6), (x - 5, roof - 1, z + 5), BLOCK_AIR);
    // The wide stone gallery with its pillar.
    c.fill((x - 3, GROUND_Y + 1, z - 6), (x + 1, roof - 1, z + 5), BLOCK_AIR);
    c.fill((x - 1, GROUND_Y + 1, z), (x - 1, roof - 1, z), BLOCK_STONE);
    // The earth gallery, propped.
    c.fill((x + 3, GROUND_Y + 1, z - 6), (x + 6, roof - 1, z + 5), BLOCK_AIR);
    for dz in [-4, -1, 2, 5] {
        c.fill((x + 4, GROUND_Y + 1, z + dz), (x + 4, roof - 1, z + dz), BLOCK_LOG);
    }
    // ...and the fourth lesson, which is not the hill's: **a building
    // falls too.** Two bays of plank roof nine across, each on one
    // post. Pull the post out of the left bay and the roof comes in --
    // nine is past what boards span (`falling::Looseness::Built`). Pull
    // the post out of the right one and nothing happens, because the
    // middle of that roof is pegged.
    //
    // Side by side and identical but for the pegs, because the whole
    // claim is about the pegs, and a demonstration of a mechanic needs
    // the control standing next to it.
    // Built on the hill's own flat top, which is the only nine-by-three
    // of open ground this plot has: the yard below it is three
    // galleries and a face.
    let bay_roof = top + 4;
    for (z0, pegged) in [(z - 5, false), (z + 3, true)] {
        c.fill((x - 4, bay_roof, z0), (x + 4, bay_roof, z0 + 2), BLOCK_PLANKS);
        for dy in 1..=3 {
            c.set(x, top + dy, z0 + 1, BLOCK_LOG);
        }
        if pegged {
            c.set(x, bay_roof, z0 + 1, BLOCK_PEGGED_PLANKS);
        }
    }
}

/// **The shelter yard.** Three fires laid for the same night: one in
/// the open, one under a roof on four posts, one inside a room with a
/// door. Light all three at dusk and stand by each in turn -- the
/// temperature readout says what `climate::shelter_at` says: the open
/// fire warms a circle and the night bites at its edge, the roof takes
/// half the night, and the room with a fire in it is warm through.
fn shelter_yard(c: &mut Canvas, (x, z): (i32, i32)) {
    // In the open.
    c.set(x - 5, GROUND_Y + 1, z, BLOCK_CAMPFIRE);
    // Under a roof: four posts and planks, no walls.
    for (px, pz) in [(x - 2, z - 2), (x + 2, z - 2), (x - 2, z + 2), (x + 2, z + 2)] {
        c.column(px, pz, 3, BLOCK_LOG);
    }
    c.fill((x - 2, GROUND_Y + 4, z - 2), (x + 2, GROUND_Y + 4, z + 2), BLOCK_PLANKS);
    c.set(x, GROUND_Y + 1, z, BLOCK_CAMPFIRE);
    // The room: walls two high, a roof, a doorway facing the lane.
    let (x0, x1) = (x + 4, x + 6);
    c.walls((x0, GROUND_Y + 1, z - 1), (x1, GROUND_Y + 3, z + 1), BLOCK_PLANKS);
    c.fill((x0, GROUND_Y + 4, z - 1), (x1, GROUND_Y + 4, z + 1), BLOCK_PLANKS);
    c.fill((x + 5, GROUND_Y + 1, z + 1), (x + 5, GROUND_Y + 2, z + 1), BLOCK_AIR);
    c.set(x + 5, GROUND_Y + 1, z - 1 + 1, BLOCK_CAMPFIRE);
}

/// **The workshop.** Everything the honest tool chain asks for, a step
/// apart: a bed of tall grass and a reed bank for fibre, two standing
/// trunks to tap for resin with a knife, pebbles and cobble to grind a
/// head from, flint to knap (and shatter), a fire to cook glue on, and
/// a chest holding one of every intermediate so each step can be tried
/// without the ones before it.
fn workshop(c: &mut Canvas, (x, z): (i32, i32)) {
    // Fibre: a tuft bed on turf, and reeds on a clay bank by a puddle.
    for dx in (-5..=-1).step_by(2) {
        for dz in (-5..=-3).step_by(2) {
            c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_TALL_GRASS);
        }
    }
    c.fill((x + 2, GROUND_Y, z - 5), (x + 5, GROUND_Y, z - 5), BLOCK_WATER);
    c.fill((x + 2, GROUND_Y, z - 4), (x + 5, GROUND_Y, z - 4), BLOCK_CLAY);
    for dx in 2..=5 {
        c.set(x + dx, GROUND_Y + 1, z - 4, BLOCK_REEDS);
    }
    // Two standing trunks, bark on: a knife takes resin off each once.
    c.column(x - 4, z, 3, BLOCK_LOG);
    c.column(x - 1, z, 3, BLOCK_BIRCH_LOG);
    // Stone to grind and flint to knap, on paving so they read as laid out.
    c.fill((x + 1, GROUND_Y, z - 1), (x + 5, GROUND_Y, z + 1), BLOCK_COBBLESTONE);
    for dx in [1, 3, 5] {
        c.set(x + dx, GROUND_Y + 1, z - 1, BLOCK_PEBBLE);
        c.set(x + dx, GROUND_Y + 1, z + 1, BLOCK_FLINT);
    }
    c.set(x + 3, GROUND_Y + 1, z, BLOCK_COBBLESTONE);
    // A fire for the glue, and the chest of intermediates.
    c.set(x, GROUND_Y + 1, z + 4, BLOCK_CAMPFIRE_LIT);
    c.set(x - 5, GROUND_Y + 1, z + 5, BLOCK_CHEST);
}

fn workshop_chest() -> (i32, i32, i32) {
    let (x, z) = plot(-1, 2);
    (x - 5, GROUND_Y + 1, z + 5)
}

/// **The forage patch.** What a meadow and a cave floor have on them,
/// side by side and a step apart.
///
/// The test world is flat and its biome is its own, so nothing scattered
/// by the generator appears in it -- which for the two things added in
/// 1.8 would mean a player could only meet them by walking off the edge
/// of the showcase and hunting. What is laid out here is the *choice*:
/// a row of root plants to dig up, and a bed of caps of which some are
/// food and some are not.
///
/// The caps alternate deliberately. A player who has read nothing should
/// be able to stand here, look at two mushrooms, and see that one of
/// them is flecked -- which is the entire mechanic, and it is worth one
/// plot of a world built for looking at things.
fn forage(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill((x - 4, GROUND_Y, z - 3), (x + 4, GROUND_Y, z + 3), BLOCK_GRASS);
    // The row you dig up, in the grass where it grows.
    for n in 0..5 {
        c.set(x - 4 + n * 2, GROUND_Y + 1, z - 2, BLOCK_ROOTS);
    }
    // ...and the bed of caps, on bare earth the way a fungus wants it.
    c.fill((x - 4, GROUND_Y, z + 1), (x + 4, GROUND_Y, z + 2), BLOCK_DIRT);
    for n in 0..5 {
        let at = x - 4 + n * 2;
        c.set(at, GROUND_Y + 1, z + 1, BLOCK_MUSHROOM);
        c.set(at, GROUND_Y + 1, z + 2, BLOCK_TOADSTOOL);
    }
}

/// **The tannery.** A row of racks in the sun with a fire at the end of
/// it, which is the shape a real one has and the shape the mechanic
/// rewards: a rack cures faster warm, stops in the rain, and takes a
/// floor from a fire beside it (see `primitive_server::drying`).
///
/// A whole rack -- its four cells -- with its near bottom at `anchor`.
///
/// **The showcase set one cell**, which is the lone rack an old save keeps
/// (`types::rack_whole`): so every rack in the test world was the small
/// one-cell frame and never the rack a player builds. The ridge runs along
/// `types::rack_far_step`, so a row of them set two apart stands end to end.
fn rack(c: &mut Canvas, anchor: (i32, i32, i32), facing: Facing) {
    for ((x, y, z), block) in crate::types::rack_cells(anchor, facing) {
        c.set(x, y, z, block);
    }
}

/// Six hide frames in two rows, because the interesting thing to test is
/// *several at once* -- a mechanic that steps a list is a mechanic whose
/// bugs are in the list. Three of them are under a roof and three are
/// not, so one `/weather rain` is an experiment rather than a wait: the
/// covered ones keep going and the open ones stop.
fn tannery(c: &mut Canvas, (x, z): (i32, i32)) {
    // The open row.
    //
    // **Frames, not racks**: a tannery stretches hides, and the big rack is
    // for meat and fish now (`rack::Trade`). A tannery of meat racks would
    // be a row of things that refuse the chest of hides beside them.
    for n in 0..3 {
        c.set(x - 3 + n * 2, GROUND_Y + 1, z - 2, crate::types::BLOCK_HIDE_FRAME);
    }
    // ...and the covered one, under a plank roof on four posts. Low
    // enough to walk under and high enough to see the racks from
    // outside.
    for n in 0..3 {
        c.set(x - 3 + n * 2, GROUND_Y + 1, z + 2, crate::types::BLOCK_HIDE_FRAME);
    }
    for &(px, pz) in &[(x - 4, z + 1), (x + 2, z + 1), (x - 4, z + 3), (x + 2, z + 3)] {
        c.column(px, pz, 3, BLOCK_LOG);
    }
    c.fill(
        (x - 4, GROUND_Y + 4, z + 1),
        (x + 2, GROUND_Y + 4, z + 3),
        BLOCK_PLANKS,
    );

    // The fire the smoked rack sits by. Lit, like the plaza's -- the
    // server registers every burning cell in a built chunk on a world's
    // first run, so this is a real fire with real fuel rather than a
    // block that looks like one.
    c.set(x + 4, GROUND_Y + 1, z, BLOCK_CAMPFIRE_LIT);
    // A rack right beside it, so the difference a fire makes is one step
    // rather than one plot.
    c.set(x + 4, GROUND_Y + 1, z - 2, crate::types::BLOCK_HIDE_FRAME);

    // A chest of hides at the near end, so the first thing to try is in
    // reach of the thing to try it on.
    c.set(x, GROUND_Y + 1, z - 4, BLOCK_CHEST);
}

/// **The pottery.** A clay bank, a kiln, and a trough of water to fill a
/// jug from.
///
/// The clay is a *pit* rather than a scatter: four cells deep, so
/// digging it is digging rather than picking it up, and so the hole left
/// behind is somewhere the water plot's rules can be tried next door.
fn pottery(c: &mut Canvas, (x, z): (i32, i32)) {
    // The bank.
    c.fill(
        (x - 4, GROUND_Y - 3, z - 4),
        (x - 1, GROUND_Y, z - 1),
        BLOCK_CLAY,
    );
    // The kiln, laid and unlit: lighting it is part of what there is to
    // try, and a kiln that arrived burning would skip the flint.
    c.set(x + 2, GROUND_Y + 1, z - 2, BLOCK_KILN);
    // ...and a lit one beside it, for the half of the test that is about
    // what a kiln *does* rather than about lighting one.
    c.set(x + 4, GROUND_Y + 1, z - 2, BLOCK_KILN_LIT);

    // A trough: a stone rim with water in it, at ground level, so a jug
    // is filled by looking down rather than by swimming.
    c.walls(
        (x + 1, GROUND_Y, z + 1),
        (x + 5, GROUND_Y + 1, z + 5),
        BLOCK_COBBLESTONE,
    );
    c.fill(
        (x + 2, GROUND_Y, z + 2),
        (x + 4, GROUND_Y, z + 4),
        BLOCK_WATER,
    );
}

/// **The wardrobe.** Four chests and a mirror of nothing: what is here
/// is the clothes.
///
/// Every garment in the game is already in the plaza chests -- the stock
/// is read off `ALL_BLOCK_IDS`, so it always is -- but they are spread
/// over a dozen chests in the order blocks were added, which is the
/// wrong order for the one question this plot exists to answer: *what
/// does a full set feel like*. So there are four chests here, and
/// `chest_stock` fills them with one material apiece.
fn wardrobe(c: &mut Canvas, (x, z): (i32, i32)) {
    // A room with a door-width gap in the near wall, so the chests are
    // somewhere rather than four blocks in a field.
    c.walls(
        (x - 4, GROUND_Y + 1, z - 4),
        (x + 4, GROUND_Y + 4, z + 4),
        BLOCK_BIRCH_PLANKS,
    );
    c.fill(
        (x - 1, GROUND_Y + 1, z + 4),
        (x + 1, GROUND_Y + 3, z + 4),
        BLOCK_AIR,
    );
    c.fill(
        (x - 4, GROUND_Y + 5, z - 4),
        (x + 4, GROUND_Y + 5, z + 4),
        BLOCK_BIRCH_PLANKS,
    );
    // Light, or a room with a roof on it is a room you cannot see the
    // chests in.
    c.set(x, GROUND_Y + 5, z, BLOCK_GLOWSTONE);
    for n in 0..WARDROBE_CHESTS as i32 {
        c.set(x - 3 + n * 2, GROUND_Y + 1, z - 3, BLOCK_CHEST);
    }
}

/// How many chests the wardrobe has, and therefore how many `chest_stock`
/// fills. One per material plus one for everything else a body wears.
const WARDROBE_CHESTS: usize = 4;

/// Where the wardrobe's chests are, in world coordinates.
fn wardrobe_chest(n: usize) -> (i32, i32, i32) {
    let (x, z) = plot(2, -1);
    (x - 3 + n as i32 * 2, GROUND_Y + 1, z - 3)
}

/// **The weather yard.** Four temperatures within twenty paces.
///
/// Everything that decides how warm a player is, except the biome --
/// which in this world is one value everywhere on purpose (see
/// `climate`). What is left is exactly what
/// `primitive_server::climate::Ambient` reads: a fire, a roof, open sky,
/// and water. Standing in each of the four in turn and watching the
/// gauge is the whole test, and it takes under a minute.
///
/// Laid out as four quarters around a crossing of lanes, so the walk
/// between any two of them is a few steps and the gauge has time to move
/// but not to settle -- which is what makes the *drift* visible rather
/// than only the ends of it.
fn weather_yard(c: &mut Canvas, (x, z): (i32, i32)) {
    // North-west: a bonfire. Three lit hearths in a row, so the warmth
    // is unmistakable and the falloff with distance is something you can
    // walk along.
    for n in 0..3 {
        c.set(x - 4 + n, GROUND_Y + 1, z - 3, BLOCK_CAMPFIRE_LIT);
    }

    // North-east: a shelter. A roof and three walls -- the fourth side
    // open, so it is somewhere to stand rather than somewhere to be
    // sealed in. What it does is halve the day-night swing and keep the
    // rain off.
    c.walls(
        (x + 1, GROUND_Y + 1, z - 5),
        (x + 5, GROUND_Y + 3, z - 1),
        BLOCK_COBBLESTONE,
    );
    c.fill(
        (x + 2, GROUND_Y + 1, z - 1),
        (x + 4, GROUND_Y + 3, z - 1),
        BLOCK_AIR,
    );
    c.fill(
        (x + 1, GROUND_Y + 4, z - 5),
        (x + 5, GROUND_Y + 4, z - 1),
        BLOCK_COBBLESTONE,
    );
    c.set(x + 3, GROUND_Y + 4, z - 3, BLOCK_GLOWSTONE);

    // South-west: open sky and nothing at all. The control, and it earns
    // its space -- "what does the gauge do when nothing is happening" is
    // the reading the other three are compared against.

    // South-east: a plunge pool, three deep, with a step down into it.
    // Water is a *ceiling* on temperature rather than an offset (see
    // `climate::WATER_C`), so this is the fastest way to get cold in the
    // game and the quickest way to see the wetness tail afterwards.
    c.fill(
        (x + 1, GROUND_Y - 2, z + 1),
        (x + 5, GROUND_Y, z + 5),
        BLOCK_AIR,
    );
    c.fill(
        (x + 1, GROUND_Y - 3, z + 1),
        (x + 5, GROUND_Y - 3, z + 5),
        BLOCK_COBBLESTONE,
    );
    c.fill(
        (x + 1, GROUND_Y - 2, z + 1),
        (x + 5, GROUND_Y, z + 5),
        BLOCK_WATER,
    );
    // A step, so getting out is walking rather than a jump the collider
    // may or may not allow.
    c.set(x + 3, GROUND_Y - 1, z + 5, BLOCK_COBBLESTONE);
    c.set(x + 3, GROUND_Y, z + 5, BLOCK_COBBLESTONE);
}

/// The gravel lanes between the plots.
///
/// They run down the chunk boundaries -- which is exactly where no plot
/// is, since a plot is `PLOT_HALF` either side of its chunk's middle --
/// so the grid of them is the grid of plots, and standing on one you can
/// see four plots without walking through any of them.
///
/// Two cells wide, one either side of the boundary. That is not width
/// for width's sake: it means **every lane straddles a seam**, so the
/// clipping in `Canvas` is exercised by the world itself rather than
/// only by a test, and a lane with one half missing would be visible
/// from spawn.
fn lanes(c: &mut Canvas) {
    let reach = PLOT_RADIUS * CHUNK_SIZE_X as i32 + CHUNK_SIZE_X as i32 - 1;
    // Interior boundaries only. The outermost ones have a chunk on the
    // far side that the build pass never runs for, and half a lane is
    // worse than none.
    for k in -PLOT_RADIUS..PLOT_RADIUS {
        let edge = k * CHUNK_SIZE_X as i32 + CHUNK_SIZE_X as i32;
        c.fill(
            (-reach, GROUND_Y, edge - 1),
            (reach, GROUND_Y, edge),
            BLOCK_GRAVEL,
        );
        c.fill(
            (edge - 1, GROUND_Y, -reach),
            (edge, GROUND_Y, reach),
            BLOCK_GRAVEL,
        );
    }
}

/// Spawn: a paved square with the chests on it.
///
/// Paved because this is the one plot a player has to be able to find
/// again from anywhere, and a cobbled square in a green field is visible
/// from every plot around it.
fn plaza(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill(
        (x - PLOT_HALF, GROUND_Y, z - PLOT_HALF),
        (x + PLOT_HALF, GROUND_Y, z + PLOT_HALF),
        BLOCK_COBBLESTONE,
    );
    // A brick kerb, so the edge of the plaza is a line rather than a
    // fade into the grass.
    c.walls(
        (x - PLOT_HALF, GROUND_Y, z - PLOT_HALF),
        (x + PLOT_HALF, GROUND_Y, z + PLOT_HALF),
        BLOCK_BRICKS,
    );

    // Four lamps, tall enough to walk under and to see from the next
    // plot along. This is the night-lighting test as well: the light
    // falls off over exactly the distance the lighting says it does.
    for (dx, dz) in [(-5, -5), (5, -5), (-5, 5), (5, 5)] {
        c.column(x + dx, z + dz, 3, BLOCK_COBBLESTONE);
        c.set(x + dx, GROUND_Y + 4, z + dz, BLOCK_GLOWSTONE);
    }

    // The chests, in a row in front of the player as they arrive. What
    // is in them is the server's business -- see `chest_stock`.
    for chest in 0..CHEST_COUNT {
        let (cx, cy, cz) = chest_position(x, z, chest);
        c.set(cx, cy, cz, BLOCK_CHEST);
    }

    // A fire to cook at, already lit, with a roof over it: the plaza is
    // the place a player comes back to, and a hearth that dies in the
    // first shower is not that.
    c.set(x, GROUND_Y + 1, z + 3, BLOCK_CAMPFIRE_LIT);
    c.fill(
        (x - 1, GROUND_Y + 4, z + 2),
        (x + 1, GROUND_Y + 4, z + 4),
        BLOCK_PLANKS,
    );
    for (dx, dz) in [(-1, 2), (1, 2), (-1, 4), (1, 4)] {
        c.column(x + dx, z + dz, 3, BLOCK_LOG);
    }
}

/// A house: four walls, a doorway, a roof and a chest.
///
/// The one plot that is a *building* rather than a demonstration, and
/// what it tests is what a building tests -- that a doorway is walkable,
/// that a roof keeps the rain off the fire under it, and that a room
/// with a lamp in it is lit and the corner without one is not.
fn house(c: &mut Canvas, (x, z): (i32, i32)) {
    let (x0, x1) = (x - 4, x + 4);
    let (z0, z1) = (z - 4, z + 4);
    c.fill((x0, GROUND_Y, z0), (x1, GROUND_Y, z1), BLOCK_BRICKS);
    c.walls((x0, GROUND_Y + 1, z0), (x1, GROUND_Y + 3, z1), BLOCK_PLANKS);
    c.fill((x0, GROUND_Y + 4, z0), (x1, GROUND_Y + 4, z1), BLOCK_LOG);
    // A doorway two cells high in the wall facing spawn.
    c.fill((x0, GROUND_Y + 1, z), (x0, GROUND_Y + 2, z), BLOCK_AIR);
    // A window in each of the other two walls: a hole in a wall is also
    // the cheapest test that light comes through one.
    c.set(x, GROUND_Y + 2, z0, BLOCK_AIR);
    c.set(x, GROUND_Y + 2, z1, BLOCK_AIR);

    c.set(x + 3, GROUND_Y + 3, z - 3, BLOCK_GLOWSTONE);
    c.set(x + 3, GROUND_Y + 1, z - 3, BLOCK_CHEST);
    // A laid fire under a real roof: strike it with flint and it stays
    // alight through a storm.
    c.set(x + 2, GROUND_Y + 1, z + 2, BLOCK_CAMPFIRE);

    // **Furnished, because the house is where sleeping is tested.**
    // Both beds side by side, so the difference between them is one
    // step rather than one world: lie on the straw, wake with a fifth
    // of the night still on you, lie on the bed and wake clear (see
    // `body::Rest`). The stool is beside the fire where somebody would
    // actually sit, and the table is in the middle because that is what
    // a table is for.
    // The pallet two cells along from the bed, head against the north wall:
    // two cells as well, now -- see `types::is_bed`.
    c.set(x - 1, GROUND_Y + 1, z - 2, crate::types::bed_half_of(BLOCK_STRAW_BED, crate::types::Facing::South, false));
    c.set(x - 1, GROUND_Y + 1, z - 3, crate::types::bed_half_of(BLOCK_STRAW_BED, crate::types::Facing::South, true));
    // Two cells, head against the wall behind it -- see `types::BED_HEAD`.
    // A bed written as one cell here would be half a bed in the test world,
    // which is exactly the thing a player can no longer build.
    c.set(x - 3, GROUND_Y + 1, z - 1, crate::types::bed_half(crate::types::Facing::South, false));
    c.set(x - 3, GROUND_Y + 1, z - 2, crate::types::bed_half(crate::types::Facing::South, true));
    c.set(x + 1, GROUND_Y + 1, z + 2, BLOCK_STOOL);
    c.set(x, GROUND_Y + 1, z, BLOCK_TABLE);
    // A barrel by the door, most of the way full, so a jug can be both
    // dipped and poured on the first visit.
    c.set(
        x + 2,
        GROUND_Y + 1,
        z - 2,
        crate::types::barrel_of(crate::body::Water::Fresh, 5),
    );
}

/// One of everything a player may put down, in rows.
///
/// Read off `PLACEABLE_BLOCKS` rather than listed here, so a block added
/// to the game stands in the test world the day it is added and cannot
/// be forgotten.
fn gallery(c: &mut Canvas, (x, z): (i32, i32)) {
    const PER_ROW: i32 = 11;
    for (index, &id) in PLACEABLE_BLOCKS.iter().filter(|&&id| !in_ground_gallery(id)).enumerate() {
        let index = index as i32;
        let bx = x - 5 + index % PER_ROW;
        let bz = z - 4 + (index / PER_ROW) * 2;
        // What each stands on is *asked* rather than assumed -- see
        // `ground_for`. A seed on cobble and a mushroom on turf are
        // both blocks with nothing holding them up, and the first thing
        // to touch the cell under one would knock it out of the world.
        c.set(bx, GROUND_Y, bz, ground_for(id));
        c.set(bx, GROUND_Y + 1, bz, id);
        // A bed is shown whole: its head in the gap between two rows, which
        // is the one cell behind it nothing else stands in.
        if let Some(((hx, hy, hz), head)) = crate::types::bed_partner((bx, GROUND_Y + 1, bz), id) {
            c.set(hx, GROUND_Y, hz, ground_for(id));
            c.set(hx, hy, hz, head);
        }
    }
}

/// **The ground's blocks stand in a gallery of their own** (`ground`): ninety
/// rocks, rubble, soils, grasses and two woods' blocks would have run the one
/// gallery's rows out of its plot and under the next one's water.
fn in_ground_gallery(id: BlockId) -> bool {
    crate::types::block_kind(id) >= 512
}

/// The ground's gallery: the same rows, wider, in a plot of their own.
const GROUND_PER_ROW: i32 = 13;

fn ground_gallery(c: &mut Canvas, (x, z): (i32, i32)) {
    for (index, &id) in PLACEABLE_BLOCKS.iter().filter(|&&id| in_ground_gallery(id)).enumerate() {
        let index = index as i32;
        let bx = x - 6 + index % GROUND_PER_ROW;
        let bz = z - 6 + (index / GROUND_PER_ROW) * 2;
        c.set(bx, GROUND_Y, bz, ground_for(id));
        c.set(bx, GROUND_Y + 1, bz, id);
    }
}

/// What to put under a block so that it stays where it is put.
///
/// Anything cross-shaped, flat or propped needs the right floor beneath
/// it (see `types::can_grow_on`), and *which* floor differs by plant:
/// turf for a tuft, tilled earth for a seed, sand for a cactus, cave
/// floor for a mushroom. Asking the rule rather than listing the answers
/// means the gallery cannot fall out of step with the rule -- and a
/// plant added later gets the right ground with no line here.
///
/// Cobble for everything that needs nothing, because a paving flag under
/// each exhibit is what makes the rows read as a display rather than as
/// litter dropped on a lawn.
fn ground_for(id: BlockId) -> BlockId {
    if !crate::types::needs_support(id) {
        return BLOCK_COBBLESTONE;
    }
    // Timber is on the list because the tinder bracket wants dead wood
    // and nothing else -- it is the first exhibit in the gallery that
    // stands on something a builder would call a material rather than
    // on ground. Last but for the paving, so nothing that would rather
    // have soil ends up on a plank.
    [
        BLOCK_GRASS,
        BLOCK_FARMLAND,
        BLOCK_SAND,
        BLOCK_DIRT,
        BLOCK_PLANKS,
        BLOCK_COBBLESTONE,
    ]
        .into_iter()
        .find(|&ground| can_grow_on(id, ground))
        .unwrap_or(BLOCK_COBBLESTONE)
}

/// Loose material in every depth it comes in, and a flight of kerbs.
///
/// The first half is the layer economy: four materials, eight cells
/// each, one to eight eighths deep. Walk along a row and the surface
/// rises under you an eighth at a time, which is the only way to see
/// whether the collider and the mesher agree about where the top of a
/// drift is.
///
/// The second is the step-up: four kerbs, two to eight eighths high. The
/// low ones are walked over and the high one is not (see
/// `PLAYER_STEP_HEIGHT`), and that is a thing to feel rather than to
/// read.
fn layers_and_steps(c: &mut Canvas, (x, z): (i32, i32)) {
    for (row, material) in [BLOCK_DIRT, BLOCK_SAND, BLOCK_SNOW, BLOCK_ASH]
        .into_iter()
        .enumerate()
    {
        let bz = z - 4 + row as i32 * 2;
        for depth in 1..=8u8 {
            c.set(
                x - 5 + depth as i32,
                GROUND_Y + 1,
                bz,
                with_layers(material, depth),
            );
        }
    }

    for step in 1..=4u8 {
        c.set(
            x + 4,
            GROUND_Y + 1,
            z - 4 + step as i32,
            with_layers(BLOCK_DIRT, step * 2),
        );
    }
}

/// A shaft into the rock with a stair round it, and a gallery with every
/// ore in its walls.
///
/// The point of it is that mining is a *place*: the ore is in stone
/// where stone belongs, and the way down is a stair rather than a hole
/// you dig, because a player testing a pickaxe should not spend the
/// first minute of it digging the hole to test it in.
///
/// A spiral in a five-by-five shaft rather than a straight flight,
/// because a straight one at one cell of drop per cell of run is
/// sixteen cells long -- it would leave its own plot, pass *over* the
/// gallery it is meant to reach, and arrive at the right depth two plots
/// away. The spiral gets the same descent out of a footprint that fits
/// where it belongs.
fn mine(c: &mut Canvas, (x, z): (i32, i32)) {
    // The shaft, cut from the surface to the floor.
    c.fill(
        (x - 2, MINE_FLOOR, z - 2),
        (x + 2, GROUND_Y, z + 2),
        BLOCK_AIR,
    );
    // The stair: one block a step, wound round the shaft wall, with the
    // step below always one cell along -- so it is walked down rather
    // than fallen down.
    for (step, (dx, dz)) in (MINE_FLOOR..GROUND_Y).rev().zip(SPIRAL.iter().cycle()) {
        c.set(x + dx, step, z + dz, BLOCK_COBBLESTONE);
    }
    // A lamp every fourth course: enough to walk by, dark enough between
    // them that the descent is still a descent.
    for y in (MINE_FLOOR..GROUND_Y).step_by(4) {
        c.set(x, y, z, BLOCK_GLOWSTONE);
    }

    // The gallery at the bottom, opening off the shaft, and the ores in
    // its walls. Each vein is three cells in a line, which is what a
    // vein looks like where a gallery has cut one.
    c.fill(
        (x - 4, MINE_FLOOR, z + 1),
        (x + 4, MINE_FLOOR + 2, z + 6),
        BLOCK_AIR,
    );
    c.set(x, MINE_FLOOR + 2, z + 4, BLOCK_GLOWSTONE);
    for (i, ore) in [
        BLOCK_COAL_ORE,
        BLOCK_COPPER_ORE,
        BLOCK_TIN_ORE,
        BLOCK_IRON_ORE,
    ]
    .into_iter()
    .enumerate()
    {
        let vz = z + 2 + i as i32;
        c.fill((x - 5, MINE_FLOOR, vz), (x - 5, MINE_FLOOR + 2, vz), ore);
        c.fill((x + 5, MINE_FLOOR, vz), (x + 5, MINE_FLOOR + 2, vz), ore);
    }
    // ...and the two things that are picked up rather than mined, lying
    // on the floor where a cave leaves them.
    //
    // **On `MINE_FLOOR`, not above it.** The gallery is hollowed from
    // `MINE_FLOOR` upward, so the rock the floor is made of is the
    // course *below* it and `MINE_FLOOR` is the lowest cell there is to
    // stand in. These four sat a course higher with air underneath --
    // which for a flat stone and a mushroom is a picture hanging in mid
    // air, and worse than that it is unsupported: the first block update
    // that reaches the cell below knocks all four out of the world, so
    // the exhibit disappears days into a world rather than visibly at
    // generation. See
    // `nothing_in_the_whole_world_is_standing_on_something_that_cannot_hold_it`.
    c.set(x - 2, MINE_FLOOR, z + 3, BLOCK_FLINT);
    c.set(x + 2, MINE_FLOOR, z + 3, BLOCK_NATIVE_COPPER);
    c.set(x - 3, MINE_FLOOR, z + 4, BLOCK_MUSHROOM);
    c.set(x + 3, MINE_FLOOR, z + 4, BLOCK_MUSHROOM);
    // ...and dripstone at the far end, every tip standing and hanging, a
    // spike under each drip the way the generator pairs them. The
    // stalactites hang in the top course, from the rock of the roof.
    //
    // The tips only: the fourth size is the *shaft* a column of two to
    // four cells is made of (`dripstone::SHAFT`), which is never laid on
    // its own and would read here as a post of stone rather than as a
    // spike. A column of three stands beside them instead.
    for (i, dx) in [-2, 0, 2].into_iter().enumerate() {
        c.set(x + dx, MINE_FLOOR, z + 6, crate::dripstone::sized(crate::types::BLOCK_STALAGMITE, i as u8));
        c.set(x + dx, MINE_FLOOR + 2, z + 6, crate::dripstone::sized(crate::types::BLOCK_STALACTITE, i as u8));
    }
    for (step, piece) in crate::dripstone::column(3, crate::dripstone::SIZES - 2).enumerate() {
        c.set(x + 4, MINE_FLOOR + step as i32, z + 6, crate::dripstone::sized(crate::types::BLOCK_STALAGMITE, piece));
    }
}

/// Sand and gravel held up by one block each.
///
/// Break the support and the column comes down, which is the whole of
/// the falling-block simulation and the one mechanic whose bug stays
/// invisible until something is standing underneath it.
fn falling(c: &mut Canvas, (x, z): (i32, i32)) {
    for (i, material) in [BLOCK_SAND, BLOCK_GRAVEL, BLOCK_SNOW, BLOCK_DIRT]
        .into_iter()
        .enumerate()
    {
        let bx = x - 4 + i as i32 * 3;
        // The support: one cobble pillar, within reach from the ground.
        c.column(bx, z, 4, BLOCK_COBBLESTONE);
        c.fill((bx, GROUND_Y + 5, z), (bx, GROUND_Y + 8, z), material);
    }
}

/// Snow, deepening as you walk into it.
///
/// Eight bands, an eighth deeper each, so the slowing is a gradient
/// under your feet rather than a number in a table.
fn snowfield(c: &mut Canvas, (x, z): (i32, i32)) {
    for depth in 1..=8u8 {
        let bz = z - 4 + depth as i32;
        c.fill(
            (x - 5, GROUND_Y + 1, bz),
            (x + 5, GROUND_Y + 1, bz),
            with_layers(BLOCK_SNOW, depth),
        );
    }
}

/// A rink of ice, with a kerb round it and a way in to build speed on.
///
/// **The plot the inertia was written for.** Walking onto it and letting
/// go of the keys is the difference between a player who is a position
/// and a player who is a mass: on grass you stop, on ice you keep going
/// and have to steer out of it. The kerb is what makes that safe to try
/// -- a rink you slide off the edge of is a joke played on whoever is
/// testing it.
fn ice_rink(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill(
        (x - PLOT_HALF, GROUND_Y, z - PLOT_HALF),
        (x + PLOT_HALF, GROUND_Y, z + PLOT_HALF),
        BLOCK_ICE,
    );
    c.walls(
        (x - PLOT_HALF - 1, GROUND_Y + 1, z - PLOT_HALF - 1),
        (x + PLOT_HALF + 1, GROUND_Y + 1, z + PLOT_HALF + 1),
        BLOCK_COBBLESTONE,
    );
    // A gap in the kerb on the spawn side, so there is a way in that is
    // not a jump -- and a run-up outside it, because what the plot is
    // for is what happens when you arrive carrying speed.
    c.fill(
        (x + PLOT_HALF + 1, GROUND_Y + 1, z - 1),
        (x + PLOT_HALF + 1, GROUND_Y + 1, z + 1),
        BLOCK_AIR,
    );
}

/// Water: a pool to swim in, and a tank with a plug in it.
///
/// Ankle deep at the near edge, over your head at the far one. Wading,
/// swimming, drowning and climbing out again are four different
/// mechanics, and this is the one place all four are within a stride of
/// each other.
///
/// The tank is the flow simulation, and it is built the way it is for a
/// reason worth reading before changing it. **A tank holds what was put
/// in it and no more** (see `fluid`), so pulling the plug empties a
/// tank rather than opening a spring: what runs down the channel is the
/// water that was standing above the plug, the tank goes down as it
/// goes, and when the tank is empty the run stops. That is the whole
/// demonstration, and it is the one thing the old model could not do --
/// there a full cell was a source, a source was endless, and the plug
/// opened a flood that had to be bounded by a reach limit
/// (`MAX_FLOW_DEPTH`, gone with the model that needed it) rather than by
/// running out.
///
/// **So the tank's volume is the length of the demonstration**, and it
/// is the number to change if the run is too short to watch or long
/// enough to be boring. The channel is short and the pool is at the end
/// of it, so what leaves the tank arrives somewhere a player standing at
/// the plug can see it arrive. `GUIDE.md` promises exactly this: "вода
/// пойдёт по жёлобу в пруд и будет идти, пока бак не опустеет".
fn water(c: &mut Canvas, (x, z): (i32, i32)) {
    // The basin, cut into the field and floored with clay so nothing
    // under it can drain.
    c.fill(
        (x - 5, GROUND_Y - 4, z - 1),
        (x + 5, GROUND_Y, z + 5),
        BLOCK_AIR,
    );
    c.fill(
        (x - 5, GROUND_Y - 4, z - 1),
        (x + 5, GROUND_Y - 4, z + 5),
        BLOCK_CLAY,
    );
    // The shelf along the near side: one step, then two, then the deep
    // end.
    c.fill(
        (x - 5, GROUND_Y - 1, z + 4),
        (x + 5, GROUND_Y - 1, z + 5),
        BLOCK_SAND,
    );
    c.fill(
        (x - 5, GROUND_Y - 2, z + 2),
        (x + 5, GROUND_Y - 2, z + 3),
        BLOCK_SAND,
    );
    // **Filled to one below the rim**, which is the freeboard the tank
    // pours into. A pool filled to the brim would leave the released
    // water nowhere to go but back up its own channel, and the plug
    // would be a thing that visibly did nothing.
    c.fill(
        (x - 5, GROUND_Y - 3, z - 1),
        (x + 5, GROUND_Y - 1, z + 5),
        BLOCK_WATER,
    );
    // Reeds stand where reeds stand: on the bank at the waterline.
    for dx in [-4, -2, 2, 4] {
        c.set(x + dx, GROUND_Y + 1, z + 6, BLOCK_REEDS);
    }

    // The tank: a walled box of water standing on the field, one course
    // above the pool's rim.
    c.walls(
        (x - 2, GROUND_Y + 1, z - 5),
        (x + 2, GROUND_Y + 2, z - 3),
        BLOCK_COBBLESTONE,
    );
    c.fill(
        (x - 1, GROUND_Y + 1, z - 4),
        (x + 1, GROUND_Y + 1, z - 4),
        BLOCK_WATER,
    );
    // The plug, and the walled channel it opens onto. Breaking the one
    // cell is the whole of the demonstration.
    c.set(x, GROUND_Y + 1, z - 3, BLOCK_COBBLESTONE);
    c.set(x, GROUND_Y + 1, z - 2, BLOCK_AIR);
    for dx in [-1, 1] {
        c.set(x + dx, GROUND_Y + 1, z - 2, BLOCK_COBBLESTONE);
        c.set(x + dx, GROUND_Y + 1, z - 3, BLOCK_COBBLESTONE);
    }
}

/// A field: tilled soil, water down the middle of it, and every stage of
/// the crop standing next to the one before it.
///
/// Growing takes half an hour, so a test world that made you wait for it
/// would be a test world where nobody ever saw ripe wheat. All three
/// stages are here at once; the row of seed at the front is the one that
/// grows while you watch. Across the channel is the cotton, seed to bolls,
/// with a patch of what frost leaves at the end of its row, and the two
/// rows behind that are where you plant your own.
fn farm(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill(
        (x - 4, GROUND_Y, z - 3),
        (x + 4, GROUND_Y, z + 3),
        BLOCK_FARMLAND,
    );
    // The channel: a cell of water at field level, which is what keeps
    // the soil round it wet and a puddle to wade through besides.
    c.fill(
        (x - 4, GROUND_Y - 1, z),
        (x + 4, GROUND_Y - 1, z),
        BLOCK_CLAY,
    );
    c.fill((x - 4, GROUND_Y, z), (x + 4, GROUND_Y, z), BLOCK_WATER);

    for (row, stage) in [BLOCK_SEEDS, BLOCK_WHEAT, BLOCK_WHEAT_RIPE]
        .into_iter()
        .enumerate()
    {
        let bz = z - 3 + row as i32;
        c.fill((x - 4, GROUND_Y + 1, bz), (x + 4, GROUND_Y + 1, bz), stage);
    }
    // The withered crop is here so a player has seen dead stalks before
    // the first autumn shows them their own.
    for (from, to, stage) in [
        (-4, -3, BLOCK_COTTON_SEEDS),
        (-2, -1, BLOCK_COTTON_PLANT),
        (0, 2, BLOCK_COTTON_RIPE),
        (3, 4, BLOCK_WITHERED_CROP),
    ] {
        c.fill((x + from, GROUND_Y + 1, z + 1), (x + to, GROUND_Y + 1, z + 1), stage);
    }
    // A full barrel and an empty one at the ends of the channel. A model
    // that only exists in a chest is a model nobody looks at from across
    // a field, which is where a barrel is actually seen; and the two side
    // by side show the water line that is the only thing telling them
    // apart.
    c.set(x - 5, GROUND_Y + 1, z, barrel_of(crate::body::Water::Fresh, 7));
    c.set(x + 5, GROUND_Y + 1, z, BLOCK_BARREL);
}

/// The three hearths, lit and unlit, half of them under a roof.
///
/// The roof is the test: a fire under the open sky goes out in the rain
/// and one with a block over it does not, and the two rows are side by
/// side so that the difference is a step to the left rather than an
/// experiment spread over two evenings.
fn hearths(c: &mut Canvas, (x, z): (i32, i32)) {
    c.fill(
        (x - 6, GROUND_Y, z - 3),
        (x + 5, GROUND_Y, z + 3),
        BLOCK_COBBLESTONE,
    );
    for (i, hearth) in [
        BLOCK_CAMPFIRE,
        BLOCK_CAMPFIRE_LIT,
        BLOCK_KILN,
        BLOCK_KILN_LIT,
        BLOCK_BLOOMERY,
        BLOCK_BLOOMERY_LIT,
    ]
    .into_iter()
    .enumerate()
    {
        let bx = x - 5 + i as i32 * 2;
        // Two of each: one in the open, one under the roof below.
        c.set(bx, GROUND_Y + 1, z - 2, hearth);
        c.set(bx, GROUND_Y + 1, z + 2, hearth);
    }
    // The roof over the second row only, held up at its corners.
    c.fill(
        (x - 6, GROUND_Y + 4, z + 1),
        (x + 5, GROUND_Y + 4, z + 3),
        BLOCK_PLANKS,
    );
    for bx in [x - 6, x + 5] {
        for bz in [z + 1, z + 3] {
            c.column(bx, bz, 3, BLOCK_LOG);
        }
    }
}

/// A tower with landings at the heights that matter.
///
/// Three blocks is nothing, six is a scratch, twelve is half of you and
/// eighteen is all of it -- so the landings are at three, six, twelve
/// and eighteen, and there is a pool at the foot of one side, because
/// "water cancels a fall entirely" is the other half of the rule and
/// needs somewhere to be tried.
fn tower(c: &mut Canvas, (x, z): (i32, i32)) {
    const TOP: i32 = 18;
    c.fill(
        (x - 1, GROUND_Y + 1, z - 1),
        (x + 1, GROUND_Y + TOP, z + 1),
        BLOCK_COBBLESTONE,
    );
    // A stair winding up the outside: one block a step and one cell
    // along, so it is walked up rather than jumped up, with the two
    // cells above each step cleared so the climb has headroom.
    for step in 0..TOP as usize {
        let (dx, dz) = SPIRAL[step % SPIRAL.len()];
        let y = GROUND_Y + 1 + step as i32;
        c.set(x + dx, y, z + dz, BLOCK_COBBLESTONE);
        c.fill((x + dx, y + 1, z + dz), (x + dx, y + 2, z + dz), BLOCK_AIR);
    }
    for height in [3, 6, 12, TOP] {
        c.fill(
            (x - 2, GROUND_Y + height, z - 2),
            (x + 2, GROUND_Y + height, z + 2),
            BLOCK_PLANKS,
        );
    }
    // The pool to jump into: three cells deep, and off to one side of
    // the tower so that missing it is possible.
    c.fill(
        (x + 4, GROUND_Y - 2, z - 2),
        (x + 7, GROUND_Y, z + 2),
        BLOCK_AIR,
    );
    c.fill(
        (x + 4, GROUND_Y - 2, z - 2),
        (x + 7, GROUND_Y, z + 2),
        BLOCK_WATER,
    );
}

/// Everything that grows, standing where it grows.
fn grove(c: &mut Canvas, (x, z): (i32, i32)) {
    oak(c, x - 4, z - 3);
    birch(c, x + 3, z - 3);
    // The orchard tree, in fruit and picked, because the pair is the
    // mechanic: one to take apples off with a right click, one that has
    // already been taken from and is filling again on the growth clock
    // (`types::ripens_into`). Standing where the tree is a broadleaf
    // like the oak beside it -- the wood is the same, and only the
    // canopy differs.
    apple_tree(c, x - 4, z + 4, BLOCK_APPLE_LEAVES_FRUIT);
    apple_tree(c, x, z + 4, BLOCK_APPLE_LEAVES_PICKED);
    // A trunk that came down, lying along X. Deadfall is the only wood
    // a player has before there is an axe, and "lying" is an axis in the
    // id rather than a block of its own.
    c.fill(
        (x - 3, GROUND_Y + 1, z + 4),
        (x + 1, GROUND_Y + 1, z + 4),
        oriented(BLOCK_LOG, Axis::X),
    );

    // The desert corner: sand, and a cactus standing on it.
    c.fill((x + 3, GROUND_Y, z + 2), (x + 5, GROUND_Y, z + 4), BLOCK_SAND);
    c.column(x + 4, z + 3, 3, BLOCK_CACTUS);

    // **A nest in the oak, on the branch the generator would use.** The
    // canopy's outer ring at the height where the ring above it is
    // narrower -- see `worldgen::place_nests` -- so what stands here is
    // what a player finds in a wood rather than a nest posed on a
    // shelf. Break it for the eggs; the bowl stays and fills again.
    c.set(x - 6, GROUND_Y + 7, z - 3, BLOCK_NEST_EGGS);

    // ...and tinder on the fallen trunk, which is the other half of the
    // same lesson: the wood that has been dead longest is the wood with
    // something growing on it. Two of them, on a five-block trunk, at
    // about the spacing the generator gives a damp wood.
    c.set(x - 2, GROUND_Y + 2, z + 4, BLOCK_BRACKET_FUNGUS);
    c.set(x + 1, GROUND_Y + 2, z + 4, BLOCK_BRACKET_FUNGUS);

    // ...and a stand of wild cereal at the edge of the wood, where a
    // dry opening in a forest actually grows one. This is where seed
    // comes from now (`types::BLOCK_WILD_WHEAT`), so the test world has
    // to have some standing rather than only a chest full of it.
    for dx in 0..3 {
        c.set(x + 4 + dx, GROUND_Y + 1, z - 5, BLOCK_WILD_WHEAT);
    }
    // ...and wild cotton a row behind it. The generator puts cotton in
    // the savanna and never at the edge of a wood, but this world is one
    // climate everywhere (`climate`), and a stand a player can walk to in
    // a minute is worth more here than the truth about where it grows.
    for dx in 0..2 {
        c.set(x + 4 + dx, GROUND_Y + 1, z - 6, BLOCK_WILD_COTTON);
    }

    // ...and the small things, in a row along the front.
    for (i, plant) in [
        BLOCK_BARE_BUSH,
        BLOCK_FLOWER,
        BLOCK_TALL_GRASS,
        BLOCK_STICK,
        BLOCK_PEBBLE,
        BLOCK_FLINT,
    ]
    .into_iter()
    .enumerate()
    {
        c.set(x - 5 + i as i32 * 2, GROUND_Y + 1, z + 1, plant);
    }
}

/// A pen: grass under the open sky, walled, for whatever walks into it.
///
/// Animals are spawned by the server on grass near a player and vanish
/// when everybody leaves, so this cannot be stocked in advance. What it
/// can be is the one place where the ground is right for them and the
/// walls keep whatever arrives.
fn pen(c: &mut Canvas, (x, z): (i32, i32)) {
    c.walls(
        (x - PLOT_HALF, GROUND_Y + 1, z - PLOT_HALF),
        (x + PLOT_HALF, GROUND_Y + 2, z + PLOT_HALF),
        BLOCK_COBBLESTONE,
    );
    // A gate: two cells of the wall left out, on the spawn side.
    c.fill(
        (x - 1, GROUND_Y + 1, z - PLOT_HALF),
        (x, GROUND_Y + 2, z - PLOT_HALF),
        BLOCK_AIR,
    );
    // Something for a herbivore to graze at, which is what actually
    // holds one in a place -- see the server's grazing.
    for (dx, dz) in [(-3, -3), (3, -3), (-3, 3), (3, 3), (0, 0)] {
        c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_BARE_BUSH);
    }
    for (dx, dz) in [(-2, 0), (2, 0), (0, -2), (0, 2)] {
        c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_TALL_GRASS);
    }
}

/// An apple tree: the oak's shape in apple leaves, with `fruit` in six
/// cells round the outside of its two lower rows.
///
/// Six, because the generator hangs five to seven (`worldgen::FRUIT_FEWEST`)
/// and a demonstration of a tree the world does not grow is worse than
/// none. `fruit` is the fruiting leaf for a tree to pick, or the picked one
/// for a tree that is filling again -- which looks like leaves, and is
/// where the apples come back.
///
/// Shorter than the oak by a row, which is what an orchard tree is --
/// and it means a player can reach the fruit from the ground, which a
/// tree whose apples were six blocks up would fail at as a
/// demonstration.
fn apple_tree(c: &mut Canvas, x: i32, z: i32, fruit: BlockId) {
    c.fill((x - 2, GROUND_Y + 4, z - 2), (x + 2, GROUND_Y + 5, z + 2), BLOCK_APPLE_LEAVES);
    c.fill((x - 1, GROUND_Y + 6, z - 1), (x + 1, GROUND_Y + 6, z + 1), BLOCK_APPLE_LEAVES);
    for (dx, dy, dz) in [(-2, 4, 0), (2, 4, 1), (-1, 4, -2), (1, 4, 2), (-2, 5, -1), (2, 5, -1)] {
        c.set(x + dx, GROUND_Y + dy, z + dz, fruit);
    }
    c.column(x, z, 5, BLOCK_LOG);
}

fn oak(c: &mut Canvas, x: i32, z: i32) {
    c.fill(
        (x - 2, GROUND_Y + 5, z - 2),
        (x + 2, GROUND_Y + 6, z + 2),
        BLOCK_LEAVES,
    );
    c.fill(
        (x - 1, GROUND_Y + 7, z - 1),
        (x + 1, GROUND_Y + 7, z + 1),
        BLOCK_LEAVES,
    );
    // The trunk last, so the canopy above does not overwrite it.
    c.column(x, z, 6, BLOCK_LOG);
}

/// A birch, in the shape `worldgen::place_birch` actually grows: a bare
/// mast, a small crown widest one row below its tip, and one whorl of
/// branches partway down.
///
/// It was a three-by-three box of leaves on a pole, which is what an oak
/// looks like at half size -- and the showcase exists to be photographed.
/// A picture of a tree the world does not grow is worse than no picture.
fn birch(c: &mut Canvas, x: i32, z: i32) {
    const TRUNK: i32 = 8;
    let top = GROUND_Y + TRUNK;
    // One leaf at the tip, then three across, then five, then three.
    for (dy, radius) in [(1i32, 0i32), (0, 1), (-1, 2), (-2, 1)] {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                // Corners off, as the generator cuts them.
                if dx.abs() == radius && dz.abs() == radius && radius > 0 {
                    continue;
                }
                c.set(x + dx, top + dy, z + dz, BLOCK_BIRCH_LEAVES);
            }
        }
    }
    // The whorl: four leaves round the mast, and the bare wood above and
    // below it is the whole silhouette.
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        c.set(x + dx, GROUND_Y + 4, z + dz, BLOCK_BIRCH_LEAVES);
    }
    // The trunk last, so the canopy above does not overwrite it.
    c.column(x, z, TRUNK, BLOCK_BIRCH_LOG);
}

// ---- the ring round the square: furniture, the savanna and the finds ----
//
// **Everything since the square filled up.** The five-by-five was every
// plot there was room for, and the furniture, the hot country and the
// finds the generator lays out (`worldgen::features`) had nowhere to
// stand but a chest -- which is where nobody looks at a thing from across
// a field. So the square grew a ring (`PLOT_RADIUS`), and each of these is
// its own plot function, so the next thing added is one more line in
// `build_all` rather than an edit inside somebody else's plot.
//
// The finds are drawn by hand here, to the shapes `features` plans on
// uneven ground, for the birch's reason: the showcase exists to be looked
// at, and a picture of a find the world does not make is worse than none.

/// **The parlour.** A room without a roof, so it can be seen into from
/// outside, furnished the way a house is once it is a house: a table with
/// a chair at each side facing it, a fire in the corner under a canopy
/// with a chair facing *that* and a stool beside it, both beds against the
/// back wall, water by the door.
///
/// The four chairs round the table are the four facings, which is the
/// test a chair needs: sit in each and the view turns to the table every
/// time (`types::seat_yaw`). The chair at the fire is the reason a chair
/// has a front at all -- sat in, it faces the warmth.
fn parlour(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::{bed_half, faced, Facing, BLOCK_CHAIR};
    c.fill((x - 5, GROUND_Y, z - 5), (x + 5, GROUND_Y, z + 5), BLOCK_PLANKS);
    c.walls((x - 5, GROUND_Y + 1, z - 5), (x + 5, GROUND_Y + 2, z + 5), BLOCK_LOG);
    // The doorway, toward the tower and the plaza beyond it.
    c.fill((x - 5, GROUND_Y + 1, z - 1), (x - 5, GROUND_Y + 2, z + 1), BLOCK_AIR);

    c.set(x, GROUND_Y + 1, z, BLOCK_TABLE);
    for (dx, dz, facing) in [
        (0, -1, Facing::South),
        (0, 1, Facing::North),
        (-1, 0, Facing::East),
        (1, 0, Facing::West),
    ] {
        c.set(x + dx, GROUND_Y + 1, z + dz, faced(BLOCK_CHAIR, facing));
    }

    // The fire on a hearthstone, under a plank canopy on four posts so the
    // rain does not put it out, a chair two steps south of it facing it and
    // a stool at its side.
    let (fx, fz) = (x + 3, z + 3);
    c.set(fx, GROUND_Y, fz, BLOCK_COBBLESTONE);
    c.set(fx, GROUND_Y + 1, fz, BLOCK_CAMPFIRE_LIT);
    for (px, pz) in [(fx - 1, fz - 1), (fx + 1, fz - 1), (fx - 1, fz + 1), (fx + 1, fz + 1)] {
        c.column(px, pz, 3, BLOCK_LOG);
    }
    c.fill((fx - 1, GROUND_Y + 4, fz - 1), (fx + 1, GROUND_Y + 4, fz + 1), BLOCK_PLANKS);
    c.set(fx, GROUND_Y + 1, fz - 2, faced(BLOCK_CHAIR, Facing::South));
    c.set(fx - 2, GROUND_Y + 1, fz, BLOCK_STOOL);

    // Both beds against the north wall, head to the wall -- see
    // `types::BED_HEAD` -- and the straw beside them.
    c.set(x - 3, GROUND_Y + 1, z - 3, bed_half(Facing::South, false));
    c.set(x - 3, GROUND_Y + 1, z - 4, bed_half(Facing::South, true));
    c.set(x - 1, GROUND_Y + 1, z - 3, crate::types::bed_half_of(BLOCK_STRAW_BED, Facing::South, false));
    c.set(x - 1, GROUND_Y + 1, z - 4, crate::types::bed_half_of(BLOCK_STRAW_BED, Facing::South, true));

    // A full barrel and an empty one, and a jug set down beside them.
    c.set(x + 4, GROUND_Y + 1, z - 4, barrel_of(crate::body::Water::Fresh, 6));
    c.set(x + 3, GROUND_Y + 1, z - 4, BLOCK_BARREL);
    c.set(x + 2, GROUND_Y + 1, z - 4, BLOCK_JUG);
}

/// **The savanna.** Hot country in one plot: dry turf, bare patches of
/// sandy soil with dry grass on them, a stand of wild cotton, a termite
/// mound and an acacia -- the flat plate on a crooked stem that is how a
/// savanna reads from a distance (`worldgen::place_acacia`).
///
/// The climate here is the test world's one temperate value
/// (`climate`), so what this shows is the blocks and the shapes, not the
/// heat; the heat is the weather yard's.
fn savanna(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::{
        BLOCK_ACACIA_LEAVES, BLOCK_DRY_GRASS, BLOCK_DRY_TURF, BLOCK_SANDY_SOIL, BLOCK_TERMITE_MOUND,
        BLOCK_WILD_COTTON,
    };
    c.fill((x - 6, GROUND_Y, z - 6), (x + 6, GROUND_Y, z + 6), BLOCK_DRY_TURF);
    for (dx, dz) in [(-4, 2), (-3, 3), (-4, 3), (2, -4), (3, -4), (3, -3), (-1, 5), (0, 5)] {
        c.set(x + dx, GROUND_Y, z + dz, BLOCK_SANDY_SOIL);
    }
    for (dx, dz) in [(-4, 2), (-3, 3), (2, -4), (3, -3), (0, 5), (5, 0), (1, -6), (-6, -1)] {
        c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_DRY_GRASS);
    }
    for (dx, dz) in [(-5, -5), (-4, -5), (-5, -4)] {
        c.set(x + dx, GROUND_Y + 1, z + dz, BLOCK_WILD_COTTON);
    }

    // The mound: a two-by-two base two high, one corner rising to a spire.
    c.fill((x + 3, GROUND_Y + 1, z + 2), (x + 4, GROUND_Y + 2, z + 3), BLOCK_TERMITE_MOUND);
    c.fill((x + 4, GROUND_Y + 3, z + 3), (x + 4, GROUND_Y + 4, z + 3), BLOCK_TERMITE_MOUND);

    // The acacia: two logs up, a step across a corner for two more, and a
    // plate of radius three over the top with a dome in its middle. The rim
    // is ragged, as the generator's is.
    let (ax, az) = (x - 2, z - 2);
    c.column(ax, az, 2, BLOCK_LOG);
    c.fill((ax + 1, GROUND_Y + 3, az + 1), (ax + 1, GROUND_Y + 4, az + 1), BLOCK_LOG);
    let (px, pz) = (ax + 1, az + 1);
    for dz in -3..=3 {
        for dx in -3i32..=3 {
            let d = dx * dx + dz * dz;
            if d > 10 || (d > 5 && (dx * 3 + dz).rem_euclid(3) == 0) {
                continue;
            }
            c.set(px + dx, GROUND_Y + 5, pz + dz, BLOCK_ACACIA_LEAVES);
            if d <= 2 {
                c.set(px + dx, GROUND_Y + 6, pz + dz, BLOCK_ACACIA_LEAVES);
            }
        }
    }
}

/// **The woodland.** Three things a wood holds that the grove does not: a
/// maple, a fallen giant and a ring of mushrooms (`worldgen::features`).
///
/// The giant is nine long with its root plate at the west end and a limb
/// under its south flank -- climb onto the limb, then onto the trunk, and
/// walk it end to end; break it by hand for timber before there is an
/// axe. The ring bares the turf it stands on and one cap in four is the
/// toadstool: tell them apart before eating one.
fn woodland(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::BLOCK_MAPLE_LEAVES;
    // The maple: the oak's shape in red.
    let (mx, mz) = (x - 4, z + 3);
    c.fill((mx - 2, GROUND_Y + 5, mz - 2), (mx + 2, GROUND_Y + 6, mz + 2), BLOCK_MAPLE_LEAVES);
    c.fill((mx - 1, GROUND_Y + 7, mz - 1), (mx + 1, GROUND_Y + 7, mz + 1), BLOCK_MAPLE_LEAVES);
    c.column(mx, mz, 6, BLOCK_LOG);

    // The giant, along x, two across (z - 4 and z - 3) and two high.
    let trunk = oriented(BLOCK_LOG, Axis::X);
    c.fill((x - 4, GROUND_Y + 1, z - 4), (x + 4, GROUND_Y + 2, z - 3), trunk);
    // The root plate at the butt: the trunk's torn end framed in earth.
    c.fill((x - 5, GROUND_Y + 1, z - 4), (x - 5, GROUND_Y + 2, z - 3), trunk);
    c.fill((x - 5, GROUND_Y + 3, z - 4), (x - 5, GROUND_Y + 4, z - 3), BLOCK_DIRT);
    for side in [z - 5, z - 2] {
        c.fill((x - 5, GROUND_Y + 1, side), (x - 5, GROUND_Y + 3, side), BLOCK_DIRT);
    }
    // The limb under its flank, lying across: the step up.
    c.fill((x, GROUND_Y + 1, z - 2), (x, GROUND_Y + 1, z - 1), oriented(BLOCK_LOG, Axis::Z));
    // Where the crown broke up.
    c.set(x + 5, GROUND_Y + 1, z - 4, BLOCK_STICK);
    c.set(x + 6, GROUND_Y + 1, z - 3, BLOCK_STICK);

    // The ring, radius three: bared earth all the way round, caps on most
    // of it, every fourth cap the toadstool.
    let (rx, rz) = (x + 2, z + 3);
    let mut caps = 0;
    for dz in -4..=4 {
        for dx in -4i32..=4 {
            if (dx * dx + dz * dz - 9).abs() > 3 {
                continue;
            }
            c.set(rx + dx, GROUND_Y, rz + dz, BLOCK_DIRT);
            if (dx * 7 + dz * 13).rem_euclid(10) < 7 {
                caps += 1;
                let cap = if caps % 4 == 0 { BLOCK_TOADSTOOL } else { BLOCK_MUSHROOM };
                c.set(rx + dx, GROUND_Y + 1, rz + dz, cap);
            }
        }
    }
}

/// **The outcrop.** A rock shelter in limestone with the ash of old fires
/// under it, and a knapping floor beside it (`worldgen::features`).
///
/// The shelter opens east. Lay a fire at the ash and call `/weather rain`:
/// it burns on, and the temperature reading under the roof holds where the
/// open field beside it drops. The knapping floor is a limestone pavement
/// with nodules, struck flakes and the pebble they were struck with.
fn outcrop(c: &mut Canvas, (x, z): (i32, i32)) {
    let rock = BLOCK_LIMESTONE;
    let (sx, sz) = (x - 2, z);
    for u in -3..=1 {
        for v in -3i32..=3 {
            let (bx, bz) = (sx + u, sz + v);
            if u <= -2 {
                c.fill((bx, GROUND_Y + 1, bz), (bx, GROUND_Y + 3, bz), rock);
            } else if v.abs() == 3 && u <= 0 {
                c.fill((bx, GROUND_Y + 1, bz), (bx, GROUND_Y + 1 + (u + v).rem_euclid(2), bz), rock);
            }
            let ragged = u == 1 && (v.abs() == 3 || v == 1);
            if !ragged {
                c.set(bx, GROUND_Y + 4, bz, rock);
            }
            if u <= -1 && v.abs() <= 2 {
                c.set(bx, GROUND_Y + 5, bz, rock);
            }
            if u <= -2 && v.abs() <= 1 {
                c.set(bx, GROUND_Y + 6, bz, rock);
            }
        }
    }
    c.set(sx, GROUND_Y + 1, sz, BLOCK_ASH);
    c.set(sx - 1, GROUND_Y + 1, sz + 1, BLOCK_FLINT_FLAKE);

    let (kx, kz) = (x + 4, z);
    c.fill((kx - 2, GROUND_Y, kz - 2), (kx + 2, GROUND_Y, kz + 2), BLOCK_LIMESTONE);
    for (dx, dz, item) in [
        (0, 0, BLOCK_FLINT),
        (1, -1, BLOCK_FLINT),
        (-1, 1, BLOCK_FLINT),
        (-1, -2, BLOCK_FLINT),
        (2, 0, BLOCK_FLINT_FLAKE),
        (-2, -1, BLOCK_FLINT_FLAKE),
        (0, 2, BLOCK_FLINT_FLAKE),
        (1, 1, BLOCK_FLINT_FLAKE),
        (0, -2, BLOCK_PEBBLE),
    ] {
        c.set(kx + dx, GROUND_Y + 1, kz + dz, item);
    }
}

/// **The thicket.** A clump of berry bushes with scrub among them
/// (`worldgen::features`), and in front of it a row of what animals leave
/// when nobody comes back for the carcass: one skeleton of every species
/// that leaves one (`types::bones_of`).
fn thicket(c: &mut Canvas, (x, z): (i32, i32)) {
    use crate::types::{bones_of, BLOCK_BERRY_BUSH, BLOCK_BUSH_LEAVES};
    let (tx, tz) = (x - 2, z - 1);
    for dz in -3..=3 {
        for dx in -3i32..=3 {
            let d = dx * dx + dz * dz;
            if d > 10 {
                continue;
            }
            let n = (dx * 5 + dz * 3).rem_euclid(10);
            if (dx, dz) == (0, 0) || n < 6 {
                c.set(tx + dx, GROUND_Y + 1, tz + dz, BLOCK_BERRY_BUSH);
            } else if n == 6 && d >= 4 {
                c.fill((tx + dx, GROUND_Y + 1, tz + dz), (tx + dx, GROUND_Y + 2, tz + dz), BLOCK_BUSH_LEAVES);
            }
        }
    }
    let species = crate::animals::Species::ALL.iter().filter(|s| s.carcass().is_some());
    for (i, &kind) in species.enumerate() {
        c.set(x - 5 + i as i32, GROUND_Y + 1, z + 5, bones_of(kind));
    }
}

// ---- what is in the chests ----

/// How many chests the plaza has.
///
/// Enough for one stack of every block the game has, rounded up. Derived
/// rather than written down, so adding blocks past the end of the third
/// chest adds a fourth chest instead of silently dropping them.
const CHEST_COUNT: usize = ALL_BLOCK_IDS.len().div_ceil(CHEST_SLOTS);

/// The nth chest of the plaza row, in world coordinates.
///
/// One function, called by the plot that draws them and by the stock
/// that fills them, because those two have to name the same cells -- and
/// a test below checks that they do.
///
/// **In rows of seven**, every other cell across the paving. It was one row
/// centred on the plaza, and a row grows by a chest every forty blocks the
/// game gains: at twelve chests it ran off both edges of the paving, and the
/// first chest was drawn in the grass where the next plot's tree stood on
/// it. Seven is the most that fit on the paving two cells apart; the second
/// row is behind the first and the third in front of it, so the player
/// arriving in the middle still has them all in view. Past three rows is
/// past what the paving holds, and `the_chests_the_server_stocks_are_the_chests_the_world_drew`
/// is what says so.
fn chest_position(plaza_x: i32, plaza_z: i32, chest: usize) -> (i32, i32, i32) {
    const PER_ROW: usize = 7;
    const ROWS: [i32; 3] = [-4, -6, -2];
    let (row, place) = (chest / PER_ROW, chest % PER_ROW);
    let in_row = CHEST_COUNT.saturating_sub(row * PER_ROW).min(PER_ROW);
    (
        plaza_x + (place as i32 - in_row as i32 / 2) * 2,
        GROUND_Y + 1,
        plaza_z + ROWS.get(row).copied().unwrap_or(-4 - 2 * row as i32),
    )
}

/// Where the plaza's chests are, and what goes in them.
///
/// **Here rather than in the server** because it is a fact about the
/// test world, and the test world is a thing the shared crate knows how
/// to build. The server's part is only to notice that a world was
/// generated with this preset and has never been stocked, and to write
/// these inventories into its container store.
///
/// The stock is read off `ALL_BLOCK_IDS`, so every block the game has is
/// in a chest the day it is added -- including the ones a player can
/// never otherwise hold, which are exactly the ones worth being able to
/// look at.
pub fn chest_stock() -> Vec<((i32, i32, i32), Inventory)> {
    let (sx, sz) = spawn_column();
    let mut stock = Vec::new();
    let mut inventory = Inventory::chest();
    let mut chest = 0;
    let mut slot = 0;

    for &(id, _) in ALL_BLOCK_IDS {
        if slot == CHEST_SLOTS {
            stock.push((
                chest_position(sx, sz, chest),
                std::mem::replace(&mut inventory, Inventory::chest()),
            ));
            chest += 1;
            slot = 0;
        }
        // A stack rather than one of each: this is a world for trying
        // things, and trying a recipe twice should not mean walking back
        // to the chest. A quarter of a full stack, so that a player who
        // takes the lot is slowed by the weight rather than pinned by
        // it.
        // A quarter of a stack of anything that stacks, and one of
        // anything that does not -- a chest slot holding thirty-two axes
        // would be a chest slot holding one axe and thirty-one dropped
        // on the floor of `put_in_slot`. See `types::stack_limit`.
        let count = crate::types::stack_limit(id).min(MAX_STACK / 4);
        inventory.put_in_slot(slot, Stack::new(id, count));
        slot += 1;
    }
    if !inventory.is_empty() {
        stock.push((chest_position(sx, sz, chest), inventory));
    }

    // ---- the wardrobe ----
    //
    // The garments are already in the plaza chests above -- the stock is
    // read off `ALL_BLOCK_IDS`, so everything always is -- but they are
    // spread through a dozen chests in the order blocks were added,
    // which is the wrong order for the question the wardrobe exists to
    // answer: *what does a full set feel like*. One chest per material
    // is that question laid out.
    // The three plot chests: each holds what its plot is for, in the
    // counts one try needs, so a step of a chain can be tried without
    // walking the steps before it.
    /// Where a plot's chest stands and what goes in it.
    type PlotChest = ((i32, i32, i32), &'static [(BlockId, u32)]);
    let plot_chests: [PlotChest; 4] = [
        (
            // The fires in the ground: every stage of a pit kiln, again.
            fire_pits_chest(),
            &[
                (crate::types::BLOCK_FIBER, 16),
                (BLOCK_LOG, 16),
                (crate::types::BLOCK_BIRCH_LOG, 8),
                (BLOCK_STICK, 8),
                (BLOCK_FLINT, 2),
                (crate::types::BLOCK_VESSEL_RAW, 2),
                (crate::types::BLOCK_MOULD_RAW, 2),
                (crate::types::BLOCK_JUG_RAW, 2),
                (crate::types::BLOCK_BRICK_RAW, 8),
                (BLOCK_FIBER, 16),
                (BLOCK_DRYING_RACK, 1),
                (crate::types::BLOCK_CLAY, 8),
                (BLOCK_DIRT, 16),
            ],
        ),
        (
            workshop_chest(),
            &[
                (BLOCK_FLINT, 12),
                (BLOCK_FLINT_FLAKE, 6),
                (BLOCK_PEBBLE, 16),
                (BLOCK_COBBLESTONE, 8),
                (BLOCK_STICK, 8),
                (BLOCK_WORKED_STICK, 4),
                (BLOCK_FIBER, 12),
                (BLOCK_CORD, 3),
                (BLOCK_SINEW, 3),
                (BLOCK_COAL, 4),
                (BLOCK_STONE_AXE_HEAD, 1),
                (BLOCK_STONE_PICK_HEAD, 1),
                (BLOCK_FLINT_KNIFE_HEAD, 1),
                (BLOCK_STONE_AXE, 1),
                (BLOCK_STONE_PICKAXE, 1),
                (BLOCK_WEDGED_AXE, 1),
                (BLOCK_WEDGED_PICKAXE, 1),
                (BLOCK_FLINT_KNIFE, 1),
                (BLOCK_RUSTY_STONE, 12),
                (BLOCK_IRON_DUST, 3),
                // Pegs and the boards to drive them into: the collapse
                // yard's roofs are up the hill, and a player who wants
                // to try fastening one of their own should not have to
                // fell a tree first.
                (BLOCK_PEG, 16),
                (BLOCK_PLANKS, 32),
            ],
        ),
        (
            hunters_chest(),
            &[
                (BLOCK_FLINT_KNIFE, 1),
                (BLOCK_FLINT_SPEAR, 1),
                (BLOCK_STONE_AXE, 1),
                (BLOCK_SINEW, 4),
                (BLOCK_BONE, 4),
                (BLOCK_HIDE, 2),
                (BLOCK_RAW_MEAT, 4),
            ],
        ),
        (
            bog_chest(),
            &[
                (BLOCK_PEAT, 16),
                (BLOCK_DRIED_PEAT, 8),
                (BLOCK_DRYING_RACK, 1),
                (BLOCK_RUSTY_STONE, 8),
                (BLOCK_REEDS, 4),
            ],
        ),
    ];
    for (at, set) in plot_chests {
        let mut chest = Inventory::chest();
        for (slot, &(id, count)) in set.iter().enumerate() {
            let count = count.min(crate::types::stack_limit(id));
            chest.put_in_slot(slot, Stack::new(id, count));
        }
        stock.push((at, chest));
    }

    // The wardrobe last, and the server's tests count on that: they
    // take the final four chests as the wardrobe.
    let sets: [&[BlockId]; WARDROBE_CHESTS] = [
        &[
            BLOCK_LEATHER_CAP,
            BLOCK_LEATHER_TUNIC,
            BLOCK_LEATHER_LEGGINGS,
            BLOCK_LEATHER_BOOTS,
        ],
        &[
            BLOCK_BRONZE_HELM,
            BLOCK_BRONZE_CUIRASS,
            BLOCK_BRONZE_GREAVES,
            BLOCK_BRONZE_BOOTS,
        ],
        &[
            BLOCK_IRON_HELM,
            BLOCK_IRON_CUIRASS,
            BLOCK_IRON_GREAVES,
            BLOCK_IRON_BOOTS,
        ],
        // The fourth is the tannery's supply and the jug bench: what a
        // player needs to *make* a set rather than to wear one.
        &[
            BLOCK_HIDE,
            BLOCK_LEATHER,
            BLOCK_DRYING_RACK,
            BLOCK_CLAY,
            BLOCK_JUG,
            BLOCK_JUG_WATER,
            BLOCK_FLINT,
        ],
    ];
    for (n, set) in sets.iter().enumerate() {
        let mut chest = Inventory::chest();
        for (slot, &id) in set.iter().enumerate() {
            let count = crate::types::stack_limit(id).min(MAX_STACK / 4);
            chest.put_in_slot(slot, Stack::new(id, count));
        }
        stock.push((wardrobe_chest(n), chest));
    }
    stock
}

/// Hides already on racks, for a world's first run.
///
/// **The one thing in this world that cannot be built out of blocks.**
/// A drying rack is a block and what is *on* it is a container entry
/// plus a float (see `crate::rack` and `primitive_server::drying`), so a
/// rack the generator drew is an empty rack. That is fine for the six in
/// the tannery -- putting a hide on one is part of what there is to try
/// -- but it means the *end* of the process is twelve minutes away from
/// a fresh world, which is twelve minutes of not being able to check
/// that it works.
///
/// So two of them arrive already loaded, at two different points: one
/// nearly done and one barely started. Between them a player can see a
/// rack finish within a minute of arriving and still watch one cure from
/// the beginning.
///
/// Read by the server on the same "first run" test the chests and the
/// hearths use: only when nothing is drying anywhere, so a player who
/// took the hides off has taken them off.
pub fn rack_stock() -> Vec<((i32, i32, i32), BlockId, f32)> {
    let (x, z) = plot(-1, -2);
    let (bx, bz) = plot(2, -2);
    vec![
        // The one by the fire, nearly done.
        ((x + 4, GROUND_Y + 1, z - 2), BLOCK_HIDE, 0.92),
        // ...and one in the open row, just laid.
        ((x - 3, GROUND_Y + 1, z - 2), BLOCK_HIDE, 0.0),
        // The bog's rack: a fish half dried. It held a sod of peat once,
        // and peat is laid on the ground to dry now (`logic::peat`), so a
        // rack showing one was a rack a player could never load the same
        // way.
        ((bx - 4, GROUND_Y + 1, bz + 4), crate::types::BLOCK_RAW_FISH, 0.5),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(cx: i32, cz: i32) -> Chunk {
        generate_chunk(ChunkPos::new(cx, cz))
    }

    fn block_at(gx: i32, gy: i32, gz: i32) -> BlockId {
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        chunk(pos.x, pos.z).get(lx, gy as usize, lz)
    }

    #[test]
    fn the_field_is_flat_and_solid_everywhere() {
        // Including a long way out, where no plot reaches: the build
        // pass is skipped there and the ground still has to be ground.
        for (cx, cz) in [(0, 0), (2, -2), (37, -91)] {
            let c = chunk(cx, cz);
            for lx in 0..CHUNK_SIZE_X {
                for lz in 0..CHUNK_SIZE_Z {
                    assert_ne!(
                        c.get(lx, GROUND_Y as usize - 1, lz),
                        BLOCK_AIR,
                        "a hole under the field in chunk ({cx}, {cz})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_player_spawns_on_the_plaza_rather_than_inside_it() {
        let (x, z) = spawn_column();
        assert_eq!(block_at(x, GROUND_Y, z), BLOCK_COBBLESTONE);
        assert_eq!(block_at(x, GROUND_Y + 1, z), BLOCK_AIR);
        assert_eq!(block_at(x, GROUND_Y + 2, z), BLOCK_AIR);
    }

    #[test]
    fn generation_is_deterministic() {
        // The property the whole preset rests on: an evicted chunk comes
        // back identical, so the world does not change under a player
        // who walked away and came back.
        assert_eq!(chunk(1, -1).blocks, chunk(1, -1).blocks);
        assert_eq!(chunk(0, 0).blocks, chunk(0, 0).blocks);
    }

    #[test]
    fn a_lane_is_whole_across_the_seam_it_runs_down() {
        // The one thing that can only be checked across two chunks: a
        // lane is written once, in world coordinates, and each of the
        // two chunks it touches keeps the half that is theirs. Read the
        // pair back through the chunks that hold them.
        for edge in [0, 16, -16] {
            assert_eq!(
                block_at(edge - 1, GROUND_Y, 40),
                BLOCK_GRAVEL,
                "the near half of the lane at x={edge} is missing"
            );
            assert_eq!(
                block_at(edge, GROUND_Y, 40),
                BLOCK_GRAVEL,
                "the far half of the lane at x={edge} is missing"
            );
        }
    }

    #[test]
    fn the_tower_is_a_tower_with_landings_on_it() {
        let (x, z) = plot(2, 0);
        assert_eq!(block_at(x, GROUND_Y + 17, z), BLOCK_COBBLESTONE);
        for height in [3, 6, 12, 18] {
            assert_eq!(
                block_at(x + 2, GROUND_Y + height, z + 2),
                BLOCK_PLANKS,
                "no landing at {height}"
            );
        }
        assert_eq!(block_at(x + 7, GROUND_Y, z), BLOCK_WATER);
    }

    #[test]
    fn the_chests_the_server_stocks_are_the_chests_the_world_drew() {
        let stock = chest_stock();
        assert!(!stock.is_empty());
        for (at, _) in stock {
            assert_eq!(
                block_at(at.0, at.1, at.2),
                BLOCK_CHEST,
                "stocked a chest at {at:?} where the world drew none"
            );
        }
    }

    #[test]
    fn every_block_in_the_game_is_in_a_chest() {
        let stocked: std::collections::HashSet<BlockId> = chest_stock()
            .iter()
            .flat_map(|(_, inventory)| {
                inventory
                    .slots()
                    .iter()
                    .filter_map(|slot| slot.map(|stack| stack.block))
            })
            .collect();
        for &(id, name) in ALL_BLOCK_IDS {
            assert!(stocked.contains(&id), "'{name}' is in no chest");
        }
    }

    #[test]
    fn nothing_in_the_whole_world_is_standing_on_something_that_cannot_hold_it() {
        // **The gallery was only ever a twelfth of the world.** The
        // check this grew out of walked the one plot where the exhibits
        // are laid out and nothing else, and the mine -- which is the
        // other place in the test world where things are put down on a
        // floor -- was outside it. Four of them were a course too high:
        // the gallery is hollowed from `MINE_FLOOR` *upward*, so a stone
        // laid at `MINE_FLOOR + 1` has air under it.
        //
        // That is not only a picture hanging in mid air. Anything with
        // `needs_support` goes when the cell beneath it is disturbed
        // (`collapse_unsupported`), so the exhibits would fall out of the
        // world days into a save rather than visibly at generation --
        // which is exactly the quiet, total failure the original test was
        // written against. So it is asked of every cell of every built
        // chunk now, and the mine is inside it.
        let mut floating: Vec<(&str, i32, i32, i32)> = Vec::new();
        for pos in built_chunks() {
            let chunk = generate_chunk(pos);
            for y in 1..crate::types::CHUNK_SIZE_Y {
                for lz in 0..CHUNK_SIZE_Z {
                    for lx in 0..CHUNK_SIZE_X {
                        let standing = chunk.get(lx, y, lz);
                        if !crate::types::needs_support(standing) {
                            continue;
                        }
                        // The cell that holds it: the one below, except for
                        // moss, which hangs from the leaf above it
                        // (`types::support_at`). The brackets on this field
                        // are laid on top of their logs, which the cell below
                        // is the answer for.
                        // ...and the stalactite, which hangs from its roof.
                        let (sx, sy, sz) = if crate::types::support_at(standing).1 == 1 {
                            crate::types::support_at(standing)
                        } else {
                            (0, -1, 0)
                        };
                        let (hx, hy, hz) = (lx as i32 + sx, y as i32 + sy, lz as i32 + sz);
                        let inside = (0..CHUNK_SIZE_X as i32).contains(&hx)
                            && (0..CHUNK_SIZE_Z as i32).contains(&hz)
                            && (0..crate::types::CHUNK_SIZE_Y as i32).contains(&hy);
                        if inside && !can_grow_on(standing, chunk.get(hx as usize, hy as usize, hz as usize)) {
                            floating.push((
                                crate::types::block_name(standing),
                                pos.x * CHUNK_SIZE_X as i32 + lx as i32,
                                y as i32,
                                pos.z * CHUNK_SIZE_Z as i32 + lz as i32,
                            ));
                        }
                    }
                }
            }
        }
        assert!(floating.is_empty(), "nothing holds these up: {floating:?}");
    }

    #[test]
    fn everything_in_the_gallery_is_standing_on_ground_that_holds_it() {
        // The failure this catches is quiet and total: a plant put down
        // on the wrong floor is a plant with nothing under it, and the
        // first thing to touch the cell below knocks it out of the
        // world -- so the gallery would lose exhibits days into a world
        // rather than visibly at generation.
        let (x, z) = plot(-1, 0);
        let old = PLACEABLE_BLOCKS.iter().filter(|&&id| !in_ground_gallery(id)).count() as i32;
        let (gx, gz) = plot(-3, 2);
        let ground = PLACEABLE_BLOCKS.iter().filter(|&&id| in_ground_gallery(id)).count() as i32;
        let cells = (0..old)
            .map(|index| (x - 5 + index % 11, z - 4 + (index / 11) * 2))
            .chain((0..ground).map(|index| (gx - 6 + index % GROUND_PER_ROW, gz - 6 + (index / GROUND_PER_ROW) * 2)));
        for (bx, bz) in cells {
            let standing = block_at(bx, GROUND_Y + 1, bz);
            let under = block_at(bx, GROUND_Y, bz);
            if !crate::types::needs_support(standing) {
                continue;
            }
            assert!(
                can_grow_on(standing, under),
                "the gallery stood {standing} on {under}, which cannot hold it"
            );
        }
    }

    #[test]
    fn the_hearths_that_are_drawn_alight_are_inside_the_built_square() {
        // The server goes looking for them there and nowhere else, so a
        // lit hearth outside it would be a fire that draws as burning,
        // never registers, and quietly refuses to cook anything.
        let inside: std::collections::HashSet<(i32, i32)> = built_chunks()
            .iter()
            .map(|pos| (pos.x, pos.z))
            .collect();
        let mut found = 0;
        for &(cx, cz) in &inside {
            let c = chunk(cx, cz);
            for (index, &id) in c.blocks.iter().enumerate() {
                if crate::types::is_burning(id) {
                    found += 1;
                    let _ = index;
                }
            }
        }
        assert!(found > 0, "the test world has no lit hearth in it at all");
    }

    #[test]
    fn a_spiral_stair_is_walked_rather_than_jumped() {
        // One cell along and one course up is a stair. Two cells along
        // is a gap, and a gap in a stair that only shows up in the world
        // -- never in a test -- is the reason this one exists.
        for pair in SPIRAL.windows(2).chain(std::iter::once(
            &[SPIRAL[SPIRAL.len() - 1], SPIRAL[0]][..],
        )) {
            let step = (pair[1].0 - pair[0].0).abs() + (pair[1].1 - pair[0].1).abs();
            assert_eq!(step, 1, "{:?} to {:?} is not one cell", pair[0], pair[1]);
        }
    }

    #[test]
    fn the_ice_rink_is_ice_and_has_a_way_in() {
        let (x, z) = plot(-2, -1);
        assert_eq!(block_at(x, GROUND_Y, z), BLOCK_ICE);
        assert_eq!(block_at(x + PLOT_HALF + 1, GROUND_Y + 1, z), BLOCK_AIR);
    }

    #[test]
    fn the_mine_reaches_the_ores() {
        let (x, z) = plot(1, 1);
        let wall: Vec<BlockId> = (2..=5)
            .map(|i| block_at(x - 5, MINE_FLOOR + 1, z + i))
            .collect();
        for ore in [
            BLOCK_COAL_ORE,
            BLOCK_COPPER_ORE,
            BLOCK_TIN_ORE,
            BLOCK_IRON_ORE,
        ] {
            assert!(wall.contains(&ore), "no ore {ore} in the mine wall: {wall:?}");
        }
        // ...and the shaft actually arrives at the gallery rather than
        // stopping in the rock above it: an unbroken column of air from
        // the field down to the floor, and floor that runs on into the
        // gallery.
        for y in MINE_FLOOR + 1..GROUND_Y {
            assert_eq!(
                block_at(x, y, z - 1),
                BLOCK_AIR,
                "the shaft is blocked at y={y}"
            );
        }
        assert_eq!(block_at(x, MINE_FLOOR + 1, z + 3), BLOCK_AIR);
    }

    /// Prints the test world from above: what a player would be
    /// standing on, plot by plot.
    ///
    /// Ignored, like the generator's timing test, because it asserts
    /// nothing -- it is here for the same reason that one is. Building a
    /// world out of coordinates typed into a file is a thing that goes
    /// wrong in ways no assertion catches (a wall one cell into its
    /// neighbour, a pool with a corner missing), and the cheapest way to
    /// see that is to look at it.
    ///
    /// `cargo test -p primitive_shared showcase::tests::map -- --ignored --nocapture`
    #[test]
    #[ignore = "diagnostic: prints the map"]
    fn map() {
        let reach = (PLOT_RADIUS + 1) * CHUNK_SIZE_X as i32;
        for gz in -reach..reach {
            let mut line = String::new();
            for gx in -reach..reach {
                // The highest cell that is not air, which is what a
                // player standing here would be standing on.
                let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
                let chunk = chunk(pos.x, pos.z);
                let top = (0..CHUNK_SIZE_Y)
                    .rev()
                    .find(|&y| chunk.get(lx, y, lz) != BLOCK_AIR)
                    .unwrap_or(0);
                line.push(symbol(chunk.get(lx, top, lz)));
            }
            println!("{line}");
        }
    }

    /// One letter per material, for `map`.
    fn symbol(id: BlockId) -> char {
        match crate::types::block_kind(id) {
            BLOCK_GRASS => '.',
            BLOCK_GRAVEL => ',',
            BLOCK_COBBLESTONE => '#',
            BLOCK_BRICKS => 'B',
            BLOCK_PLANKS => '=',
            BLOCK_LOG | BLOCK_BIRCH_LOG => 'T',
            BLOCK_LEAVES | BLOCK_BIRCH_LEAVES => '^',
            BLOCK_WATER => '~',
            BLOCK_ICE => 'i',
            BLOCK_SNOW => 's',
            BLOCK_SAND => 'd',
            BLOCK_FARMLAND => 'f',
            BLOCK_SEEDS | BLOCK_WHEAT | BLOCK_WHEAT_RIPE | BLOCK_COTTON_SEEDS
            | BLOCK_COTTON_PLANT | BLOCK_COTTON_RIPE | BLOCK_WITHERED_CROP => 'w',
            BLOCK_CHEST => 'C',
            BLOCK_GLOWSTONE => '*',
            BLOCK_CAMPFIRE | BLOCK_CAMPFIRE_LIT => 'c',
            BLOCK_KILN | BLOCK_KILN_LIT => 'k',
            BLOCK_BLOOMERY | BLOCK_BLOOMERY_LIT => 'b',
            BLOCK_AIR => ' ',
            _ => 'o',
        }
    }

    #[test]
    fn the_plots_stand_on_ground_that_is_theirs() {
        // Nothing may be built outside the square the generator promises
        // to build in, because outside it the build pass never runs and
        // half a structure would be the result.
        let edge = (PLOT_RADIUS + 1) * CHUNK_SIZE_X as i32;
        for gx in [-edge, edge] {
            for gz in -edge..=edge {
                assert_eq!(
                    block_at(gx, GROUND_Y + 1, gz),
                    BLOCK_AIR,
                    "something is built at the edge of the world at ({gx}, {gz})"
                );
            }
        }
    }
}
