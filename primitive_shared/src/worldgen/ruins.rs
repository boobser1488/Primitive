//! Ruins of the people who were here before: huts, halls, towers and
//! yards gone to grass, in every country with dry level ground, with a
//! few simple things left in them.
//!
//! ## What they are for
//!
//! **A reason to leave the path, a few times an hour.** The brick ruins
//! (`WorldGen::place_ruin`) are a landmark you tell somebody about --
//! one in two thousand chunks, fired material nobody can make yet. These
//! are the other end of the same idea: common enough that a player
//! crossing country passes one on most long walks, small enough
//! that each is a minute's detour, and built from what the country
//! around them is built from -- cut stone in the meadow, sandstone on
//! the savanna, granite in the hills, logs in the wood -- so a ruin says
//! where it is as much as what it was.
//!
//! What is left inside is **the kit of the age the world starts in**:
//! sticks, fibre, cord, flint, clay, seeds, a worn stone tool, a jug.
//! Never metal, never an ingot. Every step up the ladder is supposed to
//! be a reason to go somewhere (see CLAUDE.md), and a ruin that handed
//! out bronze would be a shortcut past the hills the bronze is in. What a
//! ruin buys is *time*: the first evening's knapping and twisting, found
//! rather than made.
//!
//! ## Where they stand
//!
//! A few candidates per `CELL`-square region (`ATTEMPTS`), tried in
//! turn, each at a hashed spot inside it and **kept `PAD` clear of every
//! cell edge**. That margin is the whole cross-chunk story. A ruin plus
//! its rubble plus the reach of any tree that could lean over it stays
//! inside its own cell, so any column in the world belongs to at most one
//! cell's ruin, and "is there a ruin here" is three hashes and a
//! subtraction for whoever asks -- the chunk
//! being generated, the tree pass deciding whether to root, the server
//! asking whether a chest is a ruin's.
//!
//! The candidate is then **judged from the column tiles**, not from the
//! chunk's own column cache: a ruin eleven wide straddles chunks, the
//! cache only reaches `FEATURE_MARGIN` past its chunk, and the verdict
//! has to be the same from every chunk the ruin touches. The tiles are
//! the store the caches are copied out of, so the two cannot disagree.
//! The verdict is remembered per thread (`RUIN_SITES`), the way a lake's
//! is.
//!
//! Each chunk then writes **only its own columns**, and what goes in a
//! column is worked out from the site and that column alone -- never
//! from what a neighbouring column holds. That is what makes a ruin
//! whole across a seam without either chunk knowing the other exists;
//! `a_ruin_is_whole_across_a_chunk_seam` holds it to that.
//!
//! ## Rejected
//!
//! * **One ruin per chunk, inside the chunk** -- what the brick ruins
//!   do. Fine at one in two thousand; at this density it would put every
//!   ruin three cells from a chunk edge on a sixteen-block grid, which
//!   from a hill is a lattice, and would cap them at nine wide.
//! * **Poisson scatter with a minimum distance.** Honest spacing, but
//!   whether a spot has a ruin would depend on its neighbours' verdicts,
//!   which depend on theirs: a chain of site judgements with no end, for
//!   a statistic a jittered grid already gets close to.
//! * **Clearing the trees after the fact**, as the brick ruins do. A
//!   crown rooted outside the clearing is cut flat on one side, and a
//!   forest of half-trees round a hut was the first thing that ruin's
//!   pictures showed. Here the tree pass asks `ruin_claims` before it
//!   roots, so no tree that could reach a wall is ever grown.

use super::{
    column_tile_with, hash2, put_block, ruin_offered, Biome, Column, ColumnCache, Preset, TileStore,
    WorldGen, ACACIA_REACH, MAX_CANOPY_RADIUS, OLD_TREE_REACH, SEA_LEVEL, TILE,
};
use crate::inventory::{Inventory, Stack};
use crate::types::{
    block_kind, faced, oriented, Axis, BlockId, ChunkPos, Facing, BLOCK_AIR, BLOCK_ASH,
    BLOCK_BIRCH_LOG, BLOCK_BIRCH_PLANKS, BLOCK_BONE, BLOCK_BUSH_LEAVES, BLOCK_CHEST, BLOCK_CLAY,
    BLOCK_COAL, BLOCK_COBBLESTONE, BLOCK_CORD, BLOCK_COTTON_SEEDS, BLOCK_DIRT, BLOCK_FEATHER,
    BLOCK_FIBER, BLOCK_FLINT, BLOCK_FLINT_FLAKE, BLOCK_FLINT_KNIFE, BLOCK_GRANITE, BLOCK_GRAVEL,
    BLOCK_HIDE, BLOCK_JUG, BLOCK_LEATHER, BLOCK_LIMESTONE, BLOCK_LOG, BLOCK_PEBBLE, BLOCK_PLANKS,
    BLOCK_SAND, BLOCK_SANDSTONE, BLOCK_SEEDS, BLOCK_SINEW, BLOCK_SNOW, BLOCK_STICK,
    BLOCK_STONE_AXE, BLOCK_STRIPPED_LOG, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

/// Side of a region cell, in blocks: one ruin candidate per cell.
///
/// Measured rather than chosen -- see `ruins_turn_up_a_few_hundred_blocks_apart_on_every_seed`,
/// which prints the spacing, and `why_ruin_sites_are_refused`, which
/// prints what turns candidates down.
///
/// It was 160 first, and 160 was a ruin every 2304 blocks of walking on
/// seed 1337: of 432 candidates over three seeds, 211 stood in or beside
/// water, 84 over a cave, 69 on a slope, and 59 were built. With caves
/// plugged, a fall of four allowed and one attempt per cell, 128 was one
/// ruin per 715 blocks walked *on land*: 103 of 432 built, 223 refused
/// for water and 82 for slope. Hence `ATTEMPTS`. With three attempts, 112
/// overshot: one ruin per 272, 325 and 285 blocks of land on seeds 1337,
/// 42 and 7, nearest neighbours a median 93 to 99 blocks apart -- under
/// the few hundred blocks a ruin has to be from the last to stay a find.
/// So 128 again, with the attempts -- which, once the tree guard went
/// round and a candidate's margin grew with it, is one ruin per 481, 385
/// and 392 blocks of land on the same seeds, neighbours a median 97 to
/// 112 blocks apart, and 141 of 432 candidates built.
///
/// **192 now, and the player said 128 was too many.** At 128 a ruin came
/// every four to five hundred blocks of land, which in the game is a wall
/// on the skyline of nearly every walk: a find stops being one when it is
/// in view from the last. Half again the side is a bit over twice the area
/// per candidate, and measured it is a bit under half as many -- one ruin
/// per 729, 781 and 770 blocks of land on seeds 1337, 42 and 7, nearest
/// neighbours a median 141 to 172 blocks apart. Fewer than the area says,
/// because the attempts rescue more of a big cell's refused candidates.
const CELL: i32 = 192;

/// Half the widest footprint: the yard is eleven across.
const HALF: i32 = 5;

/// The ring round a footprint that rubble falls into and that has to be
/// dry and near the floor's height for the site to stand.
const RING: i32 = 2;

/// Everything a ruin writes lies within this of its centre.
const REACH: i32 = HALF + RING;

/// The furthest a crown reaches from its root. The largest of the three
/// rather than a number of its own, for `FEATURE_MARGIN`'s reason: a
/// guard that stopped tracking the widest tree would let that tree lean
/// its crown over a wall.
const TREE_REACH: i32 = {
    let widest = if OLD_TREE_REACH > ACACIA_REACH { OLD_TREE_REACH } else { ACACIA_REACH };
    if widest > MAX_CANOPY_RADIUS {
        widest
    } else {
        MAX_CANOPY_RADIUS
    }
};

/// The squared distance past a footprint inside which no tree is rooted.
/// A crown reaches `TREE_REACH` along either axis, so a root nearer than
/// that on both axes can put a leaf on a wall -- and every such root lies
/// within `TREE_REACH` times the square root of two measured round, which
/// is this. See `ruin_claims` for why round.
const CLEAR_SQUARED: i32 = 2 * TREE_REACH * TREE_REACH;

/// Up to this much further, by a hash per column: the ragged edge.
const RAGGED_SQUARED: i32 = 24;

/// No tree is refused further than this from a ruin's centre on either
/// axis: the widest footprint, and the clearing at its most ragged.
const GUARD: i32 = HALF + 9;
const _: () = assert!(
    (GUARD - HALF) * (GUARD - HALF) >= CLEAR_SQUARED + RAGGED_SQUARED,
    "the guard is narrower than the clearing it guards"
);

/// How far a candidate's centre stays from its cell's edges. At least the
/// guard, so a root is only ever claimed by the cell it stands in.
const PAD: i32 = GUARD + 1;
const _: () = assert!(PAD * 2 < CELL, "a ruin cell too small for its own guard");

/// How many spots in a cell are tried before the cell is given up.
///
/// **Only a refusal about the lie of the ground is retried**: a slope, a
/// fallen stone over a cave, the lip of a bank, a floor too near the top
/// of the world. On land those were most of what was refused -- 82 of the
/// 432 candidates at `CELL` 128 were slopes -- and a hillside cell
/// usually has a level shoulder somewhere in it. Water, the spawn square
/// and the brick ruins are final. A cell whose first spot is in the sea
/// would otherwise walk its ruin up the beach until it found the shore,
/// and every coast in the world would be lined with them.
///
/// Rejected: **a smaller cell alone.** It buys the same count, but the
/// cells that refuse are the hilly ones, so the extra ruins would all go
/// where there were ruins already -- and a ruin whose nearest neighbour
/// is in sight of it is a village, not a find.
const ATTEMPTS: u32 = 3;

/// The most the ground may fall across a footprint. A hut is laid on a
/// levelled floor at the middle of the fall, so four is at most a
/// two-block step at the low edge and a two-block cut at the high one --
/// the mound an old building sits on. At three, a fall of exactly four
/// was the commonest slope refusal (22 of 432 candidates), on ground a
/// player would call flat.
const MAX_FALL: i32 = 4;

/// How far round a floor the room is cleared. Taller than the tallest
/// wall (the tower's seven) with a course to spare.
const ROOM: i32 = 9;

/// No ruin centre within this of the origin, on either axis.
///
/// **Not `spawn_column`**, which would be the exact answer: it reads
/// `PRIMITIVE_TEST_SPAWN`, and terrain that changed with an environment
/// variable is a world whose saved edits land on different ground
/// depending on how the server was started. The search starts at the
/// origin and takes the first ring with a standing column in it, so on
/// most seeds this square is where a new player wakes -- and a player
/// who wakes beside a chest of flint and cord has had the first evening
/// done for them.
const SPAWN_CLEAR: i32 = 96;

const SALT: u32 = 0x5E77_1ED0;

/// How many cell verdicts one thread remembers. A cell is ten chunks on a
/// side; this is a wide band of the world for a few bytes each.
const RUIN_SITES_KEPT: usize = 256;

thread_local! {
    static RUIN_SITES: std::cell::RefCell<TileStore<Option<Site>>> =
        std::cell::RefCell::new(TileStore::<Option<Site>>::new(RUIN_SITES_KEPT));
}

/// The five shapes.
///
/// Each is recognisable from outside at a glance -- a landmark that has
/// to be walked into before it can be told apart is not one -- and each
/// has a place where somebody obviously kept things, which is where the
/// chest goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuinKind {
    /// One room, a doorway, a hearth and the roof fallen in.
    Hut,
    /// A long hall: low side walls, a gable end still standing, a row of
    /// posts with one of them down.
    Longhouse,
    /// A round stone base, tall on one side and fallen to rubble on the
    /// other.
    Tower,
    /// A low wall round a yard, a hut in one corner and a storage pit
    /// sunk in the other.
    Yard,
    /// Only the footing left: a line of stone in the grass, one corner
    /// still standing, a bush where the floor was.
    Footing,
}

impl RuinKind {
    const ALL: [RuinKind; 5] = [
        RuinKind::Hut,
        RuinKind::Longhouse,
        RuinKind::Tower,
        RuinKind::Yard,
        RuinKind::Footing,
    ];

    /// Half the footprint along the ruin's own axes, before it is turned.
    fn half(self, roll: u32) -> (i32, i32) {
        match self {
            // Two sizes of hut, because one-room houses were never one
            // size and five by five is small enough to step over.
            RuinKind::Hut => {
                if (roll >> 20) & 1 == 0 {
                    (2, 2)
                } else {
                    (3, 3)
                }
            }
            RuinKind::Longhouse => (3, 5),
            RuinKind::Tower => (4, 4),
            RuinKind::Yard => (5, 5),
            RuinKind::Footing => (3, 4),
        }
    }

    /// A word for a tool's log line.
    pub fn name(self) -> &'static str {
        match self {
            RuinKind::Hut => "hut",
            RuinKind::Longhouse => "longhouse",
            RuinKind::Tower => "tower",
            RuinKind::Yard => "yard",
            RuinKind::Footing => "footing",
        }
    }
}

/// A judged, accepted site: everything a column needs to know to build
/// its share of the ruin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Site {
    x: i32,
    z: i32,
    /// The levelled floor: the cell the ground surface is laid in.
    floor: i32,
    kind: RuinKind,
    /// Quarter turns, and whether the plan is mirrored first -- eight
    /// ways to lay one plan, so two yards are never the same way round.
    turn: u8,
    mirror: bool,
    /// `kind.half`, along the ruin's own axes.
    half: (i32, i32),
    roll: u32,
    biome: Biome,
    /// The dressed stone of the walls, and whether they are logs over a
    /// stone sill instead.
    stone: BlockId,
    log_walls: bool,
    /// The log and the boards of the roof that fell in, where the country
    /// has wood to have built one from.
    wood: Option<(BlockId, BlockId)>,
    snowy: bool,
    chest: bool,
}

/// Why a candidate was turned down. Named rather than a bare `None` so
/// the rate can be tuned from what actually refuses sites --
/// `why_ruin_sites_are_refused` counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Refused {
    TestWorld,
    Spawn,
    BrickRuin,
    /// Sea, beach, river or lake anywhere under the footprint or its ring.
    Water,
    Cave,
    /// The fall across the footprint.
    Slope(i32),
    Bank,
    Ceiling,
}

/// A ruin, for somebody outside the generator: a tool looking for one to
/// photograph, a test, the server's chest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ruin {
    pub centre: (i32, i32),
    pub floor: i32,
    pub kind: RuinKind,
    pub biome: Biome,
    /// Half the footprint along the world's axes.
    pub half: (i32, i32),
    pub chest: Option<(i32, i32, i32)>,
}

/// Where cell (`cx`, `cz`)'s `attempt`-th candidate stands, and the roll
/// that decides what it is. One hash per axis: nothing else is read,
/// which is what lets `ruin_claims` and the pass reject almost every
/// column for the price of a subtraction.
fn candidate(seed: u32, cx: i32, cz: i32, attempt: u32) -> (i32, i32, u32) {
    let salt = seed.wrapping_add(SALT).wrapping_add(attempt.wrapping_mul(0x9E37_79B9));
    let roll = hash2(cx, cz, salt);
    let span = (CELL - 2 * PAD) as u32;
    let x = cx * CELL + PAD + (roll % span) as i32;
    let z = cz * CELL + PAD + (hash2(cz, cx, salt ^ 0x9E37) % span) as i32;
    (x, z, roll)
}

/// Whether any of a cell's candidates stands within `reach` of a column
/// on both axes -- the subtraction every asker does before paying for a
/// site judgement.
fn near_a_candidate(seed: u32, cx: i32, cz: i32, gx: i32, gz: i32, reach: i32) -> bool {
    (0..ATTEMPTS).any(|attempt| {
        let (x, z, _) = candidate(seed, cx, cz, attempt);
        (gx - x).abs() <= reach && (gz - z).abs() <= reach
    })
}

/// One column, from the tile store -- the same copy every chunk's cache
/// is assembled from.
fn column(gen: &WorldGen, gx: i32, gz: i32) -> Column {
    let (tx, tz) = (gx.div_euclid(TILE), gz.div_euclid(TILE));
    let (lx, lz) = (gx.rem_euclid(TILE), gz.rem_euclid(TILE));
    column_tile_with(gen, tx, tz, |columns| columns[(lz * TILE + lx) as usize])
}

impl Site {
    /// Half the footprint along the world's axes.
    fn world_half(&self) -> (i32, i32) {
        if self.turn % 2 == 1 {
            (self.half.1, self.half.0)
        } else {
            self.half
        }
    }

    /// The ruin's own axes to an offset in the world.
    fn to_world(self, u: i32, v: i32) -> (i32, i32) {
        let u = if self.mirror { -u } else { u };
        match self.turn {
            0 => (u, v),
            1 => (-v, u),
            2 => (-u, -v),
            _ => (v, -u),
        }
    }

    /// And back: the exact inverse of `to_world`, which
    /// `turning_a_ruin_and_turning_it_back_is_where_it_started` holds it to.
    fn to_local(self, dx: i32, dz: i32) -> (i32, i32) {
        let (u, v) = match self.turn {
            0 => (dx, dz),
            1 => (dz, -dx),
            2 => (-dx, -dz),
            _ => (-dz, dx),
        };
        (if self.mirror { -u } else { u }, v)
    }

    /// Which world axis a line along the ruin's own `u` runs on.
    fn axis_of_u(&self) -> Axis {
        if self.turn.is_multiple_of(2) {
            Axis::X
        } else {
            Axis::Z
        }
    }

    fn axis_of_v(&self) -> Axis {
        if self.turn.is_multiple_of(2) {
            Axis::Z
        } else {
            Axis::X
        }
    }

    /// A number for one column of this ruin, off its own salt. From the
    /// world position, so every chunk that holds the column rolls the
    /// same.
    fn hash(&self, u: i32, v: i32, salt: u32) -> u32 {
        let (dx, dz) = self.to_world(u, v);
        hash2(self.x + dx, self.z + dz, self.roll ^ salt)
    }

    fn summary(&self) -> Ruin {
        Ruin {
            centre: (self.x, self.z),
            floor: self.floor,
            kind: self.kind,
            biome: self.biome,
            half: self.world_half(),
            chest: self.chest_at(),
        }
    }
}

impl WorldGen {
    /// The verdict on cell (`cx`, `cz`), remembered.
    pub(super) fn ruin_site(&self, cx: i32, cz: i32) -> Option<Site> {
        let key = (self.key(), cx, cz);
        let known = RUIN_SITES.with(|store| store.borrow().tiles.get(&key).copied());
        if let Some(site) = known {
            return site;
        }
        // Judged outside the store's borrow, for `lake_site`'s reason:
        // the judge reads column tiles, and a judge run under the borrow
        // is one refactor from re-entering it.
        let site = self.judge_ruin_site(cx, cz).ok();
        RUIN_SITES.with(|store| *store.borrow_mut().get_or_insert(key, || site))
    }

    /// The uncached half of `ruin_site`: the cell's candidates in turn,
    /// the first that stands, or why the last one did not. See
    /// `ATTEMPTS` for which refusals are worth another try.
    fn judge_ruin_site(&self, cx: i32, cz: i32) -> Result<Site, Refused> {
        let mut refused = Refused::TestWorld;
        for attempt in 0..ATTEMPTS {
            match self.judge_candidate(cx, cz, attempt) {
                Ok(site) => return Ok(site),
                Err(why @ (Refused::Slope(_) | Refused::Cave | Refused::Bank | Refused::Ceiling)) => {
                    refused = why;
                }
                Err(final_word) => return Err(final_word),
            }
        }
        Err(refused)
    }

    /// One candidate, judged.
    fn judge_candidate(&self, cx: i32, cz: i32, attempt: u32) -> Result<Site, Refused> {
        use crate::types::BLOCK_STONE;
        if self.preset == Preset::Test {
            return Err(Refused::TestWorld);
        }
        let (x, z, roll) = candidate(self.seed, cx, cz, attempt);
        if x.abs() < SPAWN_CLEAR && z.abs() < SPAWN_CLEAR {
            return Err(Refused::Spawn);
        }
        let kind = RuinKind::ALL[((roll >> 8) % 5) as usize];
        let turn = ((roll >> 12) & 3) as u8;
        let mirror = (roll >> 14) & 1 == 1;
        let half = kind.half(roll);
        let (hx, hz) = if turn % 2 == 1 { (half.1, half.0) } else { half };

        // **Not on an old brick ruin's chunk.** That pass clears to the
        // sky and lays its own floor, and two buildings interleaved in
        // one clearing read as neither. Asked of the roll alone, which is
        // conservative -- a refused brick site still keeps this one away
        // -- and costs four hashes against a thousandth of the ruins.
        let (w, e) = ((x - REACH).div_euclid(CHUNK_SIZE_X as i32), (x + REACH).div_euclid(CHUNK_SIZE_X as i32));
        let (n, s) = ((z - REACH).div_euclid(CHUNK_SIZE_Z as i32), (z + REACH).div_euclid(CHUNK_SIZE_Z as i32));
        for chunk_z in n..=s {
            for chunk_x in w..=e {
                if ruin_offered(self, ChunkPos::new(chunk_x, chunk_z)) {
                    return Err(Refused::BrickRuin);
                }
            }
        }

        // **Dry, above the tide, and level enough.**
        // Every column of the footprint and its ring, before anything is
        // decided about what to build: a hut laid across a river bank is
        // a wall with its foot in the water, and the generator has no
        // way to walk it back afterwards.
        let mut floor_heights = Vec::with_capacity(((2 * hx + 1) * (2 * hz + 1)) as usize);
        let mut ring = Vec::with_capacity(64);
        for dz in -(hz + RING)..=hz + RING {
            for dx in -(hx + RING)..=hx + RING {
                let (gx, gz) = (x + dx, z + dz);
                let here = column(self, gx, gz);
                if matches!(here.biome, Biome::Ocean | Biome::Beach | Biome::River)
                    || here.height <= SEA_LEVEL + 1
                    || here.height <= here.water
                {
                    return Err(Refused::Water);
                }
                // **No cave check here any more.** Every column of the
                // footprint and ring was asked, and it refused 84 of 432
                // candidates -- a fifth of the world's sites, for holes a
                // floor can simply be laid over. The floor now plugs what
                // a cave took from under it (`build_column`), and only the
                // ring cells that actually get a fallen stone are asked,
                // below, once the site exists to say which they are.
                if dx.abs() <= hx && dz.abs() <= hz {
                    floor_heights.push(here.height);
                } else {
                    ring.push((dx, dz, here));
                }
            }
        }
        floor_heights.sort_unstable();
        let (lowest, highest) = (floor_heights[0], floor_heights[floor_heights.len() - 1]);
        if highest - lowest > MAX_FALL {
            return Err(Refused::Slope(highest - lowest));
        }
        // The median, so the cut on the high side and the fill on the low
        // side are as small as the ground allows.
        let floor = floor_heights[floor_heights.len() / 2];
        if ring.iter().any(|(_, _, here)| (here.height - floor).abs() > MAX_FALL + 1) {
            return Err(Refused::Bank); // the lip of a bank: the rubble would hang
        }
        if floor + ROOM + 2 >= CHUNK_SIZE_Y as i32 {
            return Err(Refused::Ceiling);
        }

        // **Built from the ground it stands on.** Sandstone in hot
        // country, granite where the column's rock is, limestone over
        // limestone, and dressed stone over plain stone.
        //
        // **Dressed, not fieldstone.** Plain-stone country was built in
        // cobble first -- a dry-stone wall picked off a meadow -- and the
        // first picture of a hut in the tundra had no hut in it: a bared
        // slope wears cobble (see `place_boulders`), the slopes there are
        // cobble under snow, and the walls were the scree. Cut stone is
        // the one grey in this world that no slope wears.
        let middle = column(self, x, z);
        let stone = match middle.biome {
            Biome::Desert | Biome::Savanna => BLOCK_SANDSTONE,
            _ if middle.height >= middle.granite_from => BLOCK_GRANITE,
            _ => match middle.rock {
                BLOCK_LIMESTONE => BLOCK_LIMESTONE,
                BLOCK_SANDSTONE => BLOCK_SANDSTONE,
                BLOCK_GRANITE => BLOCK_GRANITE,
                _ => BLOCK_STONE,
            },
        };
        // Log walls in the woods, and only there: a log cabin on the open
        // steppe is a house somebody carried a forest to. The roof that
        // fell in is wood wherever wood grows at all, and nothing in the
        // desert or above the tree line, where it would have been hide
        // and has long since gone.
        let log_walls = matches!(
            middle.biome,
            Biome::Forest | Biome::BirchForest | Biome::Taiga | Biome::DeadForest | Biome::Swamp
        );
        let wood = match middle.biome {
            Biome::BirchForest => Some((BLOCK_BIRCH_LOG, BLOCK_BIRCH_PLANKS)),
            // A cabin in a fir wood is built of firs.
            Biome::Taiga => Some((crate::types::BLOCK_FIR_LOG, crate::types::BLOCK_FIR_PLANKS)),
            Biome::Forest
            | Biome::DeadForest
            | Biome::Swamp
            | Biome::Bog
            | Biome::Plains
            | Biome::Savanna => Some((BLOCK_LOG, BLOCK_PLANKS)),
            _ => None,
        };
        let snowy = block_kind(middle.surface.top) == BLOCK_SNOW;
        // A bare footing has somewhere to keep a chest half the time:
        // there is no room left standing, only a corner.
        let chest = kind != RuinKind::Footing || (roll >> 22) & 1 == 0;
        let site = Site {
            x,
            z,
            floor,
            kind,
            turn,
            mirror,
            half,
            roll,
            biome: middle.biome,
            stone,
            log_walls: log_walls && kind != RuinKind::Tower && kind != RuinKind::Footing,
            wood,
            snowy,
            chest,
        };
        // A fallen stone lands on the ground, and where a cave has taken
        // the ground away -- the surface cell or the one holding it up,
        // `spawn_quality`'s rule -- it would hang over the hole. Asked of
        // the stones only, which are a cell in seven of the ring.
        for &(dx, dz, here) in &ring {
            let (u, v) = site.to_local(dx, dz);
            if site.rubble(u, v, here).is_some()
                && (self.is_cave(x + dx, here.height, z + dz) || self.is_cave(x + dx, here.height - 1, z + dz))
            {
                return Err(Refused::Cave);
            }
        }
        Ok(site)
    }

    /// Whether a tree rooted at this column could reach a ruin.
    ///
    /// **A round clearing with a ragged edge, not a square one.** This
    /// first refused every root inside a rectangle round the ruin, and the
    /// first picture of a longhouse in a swamp was a lawn with ruler-
    /// straight edges cut out of the canopy -- the one shape no clearing
    /// has. The distance that has to be kept is measured round now
    /// (`CLEAR_SQUARED`), which covers the square without being one, and a
    /// hash per column takes a few roots past it (`RAGGED_SQUARED`) so the
    /// edge frays. What it gives up is the rubble ring: a bough may hang
    /// over a fallen stone, which is a ruin at the edge of a wood. It never
    /// hangs over a wall.
    ///
    /// Asked by `place_trees` of every column that has already rolled a
    /// tree. Almost always answered by the subtraction: the candidates
    /// for a column are the ones in its own cell (`PAD` sees to that), and
    /// a column further than `GUARD` from all of them cannot be claimed
    /// whether a site was accepted or not.
    pub(super) fn ruin_claims(&self, gx: i32, gz: i32) -> bool {
        if self.preset == Preset::Test {
            return false;
        }
        let (cx, cz) = (gx.div_euclid(CELL), gz.div_euclid(CELL));
        if !near_a_candidate(self.seed, cx, cz, gx, gz, GUARD) {
            return false;
        }
        self.ruin_site(cx, cz).is_some_and(|site| {
            let (hx, hz) = site.world_half();
            // How far outside the footprint, on each axis; zero inside it.
            let out_x = ((gx - site.x).abs() - hx).max(0);
            let out_z = ((gz - site.z).abs() - hz).max(0);
            let ragged = (hash2(gx, gz, site.roll ^ 0xC1EA) % RAGGED_SQUARED as u32) as i32;
            out_x * out_x + out_z * out_z <= CLEAR_SQUARED + ragged
        })
    }
}

impl WorldGen {
    /// Every ruin that reaches into this chunk, its own columns of it.
    ///
    /// Runs after the boulders and the termite mounds and before the
    /// ground cover, and both halves of that are load-bearing. After: the
    /// room is cleared from the floor up, so a boulder or a mound that
    /// rolled into the footprint is swept out rather than left standing
    /// in the hearth. Before: the paving and the packed earth are what
    /// the ground cover reads, so grass grows up to the walls, a pebble
    /// can lie on a hut floor, and nothing sprouts through a sill.
    pub(super) fn place_ruins(
        &self,
        blocks: &mut [BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &ColumnCache,
    ) {
        if self.preset == Preset::Test {
            return;
        }
        let (size_x, size_z) = (CHUNK_SIZE_X as i32, CHUNK_SIZE_Z as i32);
        let cells_x = (origin_x - REACH).div_euclid(CELL)..=(origin_x + size_x - 1 + REACH).div_euclid(CELL);
        let cells_z = (origin_z - REACH).div_euclid(CELL)..=(origin_z + size_z - 1 + REACH).div_euclid(CELL);
        for cz in cells_z {
            for cx in cells_x.clone() {
                // The subtraction before the judge: nine chunks in ten
                // are nowhere near their cell's candidate.
                let reaches = (0..ATTEMPTS).any(|attempt| {
                    let (x, z, _) = candidate(self.seed, cx, cz, attempt);
                    x + REACH >= origin_x
                        && x - REACH < origin_x + size_x
                        && z + REACH >= origin_z
                        && z - REACH < origin_z + size_z
                });
                if !reaches {
                    continue;
                }
                let Some(site) = self.ruin_site(cx, cz) else {
                    continue;
                };
                let (hx, hz) = site.world_half();
                for dz in -(hz + RING)..=hz + RING {
                    let lz = site.z + dz - origin_z;
                    if !(0..size_z).contains(&lz) {
                        continue;
                    }
                    for dx in -(hx + RING)..=hx + RING {
                        let lx = site.x + dx - origin_x;
                        if !(0..size_x).contains(&lx) {
                            continue;
                        }
                        // The chunk's own cache, which for a column
                        // inside the chunk is the tile's copy exactly.
                        let here = columns.at(lx, lz);
                        site.build_column(dx, dz, here, &mut |y, id, overwrite| {
                            put_block(blocks, lx, y, lz, id, overwrite)
                        });
                    }
                }
            }
        }
    }
}

/// What one column of a ruin holds from the floor up, before it is
/// written.
///
/// A column's plan is a function of the site and of where in the ruin the
/// column is -- never of the blocks around it -- which is the property
/// every seam depends on. See the module note.
struct Plan {
    /// The cell the floor is laid in: open ground, packed earth, a sill.
    ground: BlockId,
    /// The cell under that, for the one plan that digs: the storage pit.
    below: Option<BlockId>,
    /// Where `stack[0]` goes, counted from the floor. One for everything
    /// standing on the floor; zero for a jug standing in the pit.
    base: i32,
    stack: [BlockId; 10],
    len: usize,
}

impl Plan {
    fn push(&mut self, block: BlockId) {
        if self.len < self.stack.len() {
            self.stack[self.len] = block;
            self.len += 1;
        }
    }
}

impl Site {
    /// The chest's cell along the ruin's own axes: the back corner of
    /// the room, where whoever lived here kept things away from the door.
    fn chest_cell(&self) -> (i32, i32) {
        let (a, b) = self.half;
        match self.kind {
            RuinKind::Hut => (a - 1, b - 1),
            RuinKind::Longhouse => (-(a - 1), b - 1),
            RuinKind::Tower => (-2, 1),
            RuinKind::Yard => (a - 1, b - 1),
            RuinKind::Footing => (-(a - 1), -(b - 1)),
        }
    }

    /// Where the chest stands in the world, if this ruin has one.
    fn chest_at(&self) -> Option<(i32, i32, i32)> {
        if !self.chest {
            return None;
        }
        let (u, v) = self.chest_cell();
        let (dx, dz) = self.to_world(u, v);
        Some((self.x + dx, self.floor + 1, self.z + dz))
    }

    /// A chest turned to face into the room, so it opens toward whoever
    /// walked in rather than against the wall.
    fn chest_block(&self, u: i32, v: i32) -> BlockId {
        let (tu, tv) = if u.abs() >= v.abs() { (-u.signum(), 0) } else { (0, -v.signum()) };
        let facing = match self.to_world(tu, tv) {
            (1, 0) => Facing::East,
            (-1, 0) => Facing::West,
            (0, 1) => Facing::South,
            _ => Facing::North,
        };
        faced(BLOCK_CHEST, facing)
    }

    /// Whether the wall's stone is grey itself, and so takes a fieldstone
    /// patch without the patch reading as a hole. See `raise_wall`.
    fn grey(&self) -> bool {
        matches!(self.stone, BLOCK_GRANITE | crate::types::BLOCK_STONE)
    }

    /// Whether anything green grows here -- a bush in the yard or the
    /// footing. Not in sand, snow or bare rock.
    fn green(&self) -> bool {
        !self.snowy
            && !matches!(
                self.biome,
                Biome::Desert | Biome::Savanna | Biome::Mountains | Biome::SnowyPeaks | Biome::Tundra
            )
    }

    /// The floor of a room people lived in: trodden earth, or whatever
    /// the desert and the snow have laid over it since.
    fn packed(&self, here: Column) -> BlockId {
        if self.snowy {
            BLOCK_SNOW
        } else if matches!(self.biome, Biome::Desert | Biome::Savanna) {
            here.surface.top
        } else {
            BLOCK_DIRT
        }
    }

    /// Courses left standing in one wall cell of a wall `tall` high when
    /// whole.
    ///
    /// **Cut in runs, not cell by cell.** A wall whose every cell rolls
    /// its own height is a row of crenellations -- it reads as built that
    /// way. A wall falls in lengths, so the height is rolled for a pair of
    /// cells along the wall (`along`, on this `side`) and only trimmed per
    /// cell after that.
    fn standing(&self, side: i32, along: i32, u: i32, v: i32, tall: i32) -> i32 {
        let run = hash2(side, (along + side).div_euclid(2), self.roll ^ 0x3A11);
        let courses = match run % 8 {
            0 => 0,
            1 | 2 => (tall - 2).max(1),
            3..=5 => tall - 1,
            _ => tall,
        };
        let trim = i32::from(self.hash(u, v, 0x7A1E).is_multiple_of(4));
        (courses - trim).max(0)
    }

    /// The side of a rectangular wall a cell is on, how far along it the
    /// cell is, and which world axis the wall runs on.
    fn wall_side(&self, u: i32, v: i32) -> (i32, i32, Axis) {
        let (_, b) = self.half;
        if v.abs() == b {
            (if v < 0 { 0 } else { 1 }, u, self.axis_of_u())
        } else {
            (if u < 0 { 2 } else { 3 }, v, self.axis_of_v())
        }
    }

    /// Stacks `courses` of wall into a plan. Logs lie along the wall on a
    /// stone sill where the walls are timber, with a stripped post at a
    /// corner; everywhere else it is the ruin's stone -- patched with
    /// fieldstone a cell in four where that stone is grey itself (`grey`), and nowhere
    /// else. Cobble in a sandstone or limestone wall was the loudest fault
    /// in the first pictures: a dark square every few cells, which from
    /// twenty blocks reads as holes rather than as repairs.
    ///
    /// **Logs on their side, never upright.** An upright log is a trunk
    /// to `felling`, and a player chopping a hut wall would bring down a
    /// tree that is not there. A post is stripped wood for the same
    /// reason: `felling::is_standing_trunk` does not count it.
    fn raise_wall(&self, plan: &mut Plan, u: i32, v: i32, courses: i32, axis: Axis, corner: bool) {
        for course in 1..=courses {
            let block = match self.wood {
                Some((log, _)) if self.log_walls && course >= 2 => {
                    if corner {
                        BLOCK_STRIPPED_LOG
                    } else {
                        oriented(log, axis)
                    }
                }
                _ if self.grey() && self.hash(u, v, 0xC0B + course as u32).is_multiple_of(4) => {
                    BLOCK_COBBLESTONE
                }
                _ => self.stone,
            };
            plan.push(block);
        }
        // Snow lies on a wall top in cold country, on about half of them
        // -- every one capped is a wall built out of snow.
        if courses > 0 && self.snowy && self.hash(u, v, 0x50E).is_multiple_of(2) {
            plan.push(BLOCK_SNOW);
        }
    }

    /// The roof that came down, over one interior cell.
    ///
    /// Boards lying where they fell, a cell in four, where the country
    /// has wood; drifted sand against the walls in hot country, which had
    /// no timber roof to lose. Never in `aisle`, the way in from the
    /// door, which has to stay walkable or the ruin is a thing to climb.
    fn fallen_roof(&self, plan: &mut Plan, u: i32, v: i32, aisle: bool) {
        if aisle {
            return;
        }
        let (a, b) = self.half;
        let roll = self.hash(u, v, 0x2006);
        match self.wood {
            Some((_, boards)) => {
                // Four, not three: at three a five-by-five hut was a heap
                // of boards with a wall round it, which from a hillside
                // read as a pile rather than a room.
                if roll.is_multiple_of(4) {
                    plan.push(boards);
                }
            }
            None if matches!(self.biome, Biome::Desert | Biome::Savanna) => {
                let against_a_wall = u.abs() == a - 1 || v.abs() == b - 1;
                if against_a_wall && roll.is_multiple_of(3) {
                    plan.push(BLOCK_SAND);
                }
            }
            None => {}
        }
    }

    /// A stone that fell outside the walls, or `None`. Only on ground
    /// near the floor's height: the ring is not levelled, and a stone on
    /// top of a bank above the ruin fell uphill.
    fn rubble(&self, u: i32, v: i32, here: Column) -> Option<BlockId> {
        if here.height > self.floor + 1 || here.height < self.floor - 2 {
            return None;
        }
        let roll = self.hash(u, v, 0x4B1E);
        if !roll.is_multiple_of(7) {
            return None;
        }
        // The wall's own stone, for `raise_wall`'s reason: dark cobble lying
        // round a sandstone hut is a second building's rubble.
        Some(if self.grey() && (roll >> 8) & 1 == 1 { BLOCK_COBBLESTONE } else { self.stone })
    }

    /// Something small left on a floor that holds nothing else: a pebble,
    /// a flint, a stick. The chest is what a player came for; these are
    /// what tells them, before they find it, that somebody was here.
    fn litter(&self, plan: &mut Plan, u: i32, v: i32) {
        let roll = self.hash(u, v, 0x1177);
        if !roll.is_multiple_of(16) || !crate::types::can_grow_on(BLOCK_PEBBLE, plan.ground) {
            return;
        }
        plan.push([BLOCK_PEBBLE, BLOCK_FLINT, BLOCK_STICK][((roll >> 8) % 3) as usize]);
    }

    /// Writes one column of the ruin through `emit(y, block, overwrite)`.
    ///
    /// `dx`, `dz` are the column's offset from the centre and `here` is
    /// what the column was before anything was built on it.
    fn build_column(&self, dx: i32, dz: i32, here: Column, emit: &mut dyn FnMut(i32, BlockId, bool)) {
        let (u, v) = self.to_local(dx, dz);
        let (a, b) = self.half;
        let floor = self.floor;
        if u.abs() > a || v.abs() > b {
            // Outside the walls: nothing levelled and nothing cleared --
            // this is somebody's meadow now -- only a fallen stone, and
            // only into air, so it yields to a bush the way a boulder does.
            if let Some(stone) = self.rubble(u, v, here) {
                emit(here.height + 1, stone, false);
            }
            return;
        }
        // **Level the floor.** The low side is built up with the column's
        // own soil and the high side cut back, so the ruin sits on the
        // mound its floor made rather than on a slope.
        for y in here.height + 1..floor {
            emit(y, here.surface.filler, true);
        }
        // **And plug what a cave took from under it.** Two cells under
        // the floor, written only into air, so solid ground is left as it
        // is and a hole is filled -- which is what lets a site stand over
        // a cave at all, instead of a fifth of the world's sites being
        // refused for one. Stone where the soil is sand, because sand
        // falls, and a floor on a sand plug over a void is a floor that
        // drops the first time something next to it is touched.
        let plug = if crate::blocks::definition(here.surface.filler).falls {
            self.stone
        } else {
            here.surface.filler
        };
        for y in floor - 2..floor {
            emit(y, plug, false);
        }
        // The room, cleared: a boulder, a mound, a bush or a length of
        // deadfall that landed in it is gone. Trees never got this far
        // (`ruin_claims`).
        for y in floor + 1..=floor + ROOM {
            emit(y, BLOCK_AIR, true);
        }
        let mut plan = Plan {
            ground: here.surface.top,
            below: None,
            base: 1,
            stack: [BLOCK_AIR; 10],
            len: 0,
        };
        let walled = match self.kind {
            RuinKind::Hut => self.plan_hut(&mut plan, u, v, here),
            RuinKind::Longhouse => self.plan_longhouse(&mut plan, u, v, here),
            RuinKind::Tower => self.plan_tower(&mut plan, u, v, here),
            RuinKind::Yard => self.plan_yard(&mut plan, u, v, here),
            RuinKind::Footing => self.plan_footing(&mut plan, u, v, here),
        };
        if !walled && plan.len == 0 && plan.below.is_none() {
            self.litter(&mut plan, u, v);
        }
        emit(floor, plan.ground, true);
        if let Some(below) = plan.below {
            emit(floor - 1, below, true);
        }
        for (step, &block) in plan.stack[..plan.len].iter().enumerate() {
            emit(floor + plan.base + step as i32, block, true);
        }
    }

    /// One room, a doorway, a hearth, and the roof on the floor.
    ///
    /// ```text
    ///   # # # # #      # wall        c chest
    ///   # j . c #      o hearth      j jug
    ///   # = o = #      = boards where the roof came down
    ///   # . . . #
    ///   # #   # #      the gap is the door
    /// ```
    fn plan_hut(&self, plan: &mut Plan, u: i32, v: i32, here: Column) -> bool {
        let (a, b) = self.half;
        if u.abs() == a || v.abs() == b {
            if u == 0 && v == -b {
                return true; // the doorway: open ground, nothing on it
            }
            let corner = u.abs() == a && v.abs() == b;
            // Timber stands a course taller than stone: a log is a
            // thicker course, and a stone hut three high already hides a
            // player.
            let tall = if self.log_walls { 4 } else { 3 };
            let (side, along, axis) = self.wall_side(u, v);
            // A corner stands whole: it is the part of a wall that two
            // walls hold up.
            let courses = if corner { tall } else { self.standing(side, along, u, v, tall) };
            self.raise_wall(plan, u, v, courses, axis, corner);
            return true;
        }
        plan.ground = self.packed(here);
        let inner = a - 1;
        if (u, v) == (0, 0) {
            // The hearth: a stone laid in the earth floor with the ash of
            // the last fire still on it.
            plan.ground = BLOCK_COBBLESTONE;
            plan.push(BLOCK_ASH);
        } else if self.chest && (u, v) == self.chest_cell() {
            plan.push(self.chest_block(u, v));
        } else if (u, v) == (-inner, inner) {
            plan.push(BLOCK_JUG);
        } else if let (Some((log, _)), true) = (self.wood, a >= 3 && u == -inner && v <= 0) {
            // The ridge beam of a big hut, down in one piece along the
            // wall it fell against.
            plan.push(oriented(log, self.axis_of_v()));
        } else {
            self.fallen_roof(plan, u, v, u == 0 && v < 0);
        }
        false
    }

    /// A long hall: low side walls, a gable standing at the back, a post
    /// still up and the one opposite it fallen along the floor.
    fn plan_longhouse(&self, plan: &mut Plan, u: i32, v: i32, here: Column) -> bool {
        let (a, b) = self.half;
        if u.abs() == a || v.abs() == b {
            // The door in the front gable, and one in the long side.
            if (u == 0 && v == -b) || (u == a && v == 1) {
                return true;
            }
            let corner = u.abs() == a && v.abs() == b;
            let (side, along, axis) = self.wall_side(u, v);
            let courses = if v == b && u.abs() <= 1 {
                // **The back gable keeps its peak**: five courses in the
                // middle and four beside it. A hall with every wall cut
                // to the same stump is a pen; the triangle is what says
                // there was a roof.
                5 - u.abs()
            } else if corner {
                3
            } else if v.abs() == b {
                self.standing(side, along, u, v, 3)
            } else {
                self.standing(side, along, u, v, 2)
            };
            self.raise_wall(plan, u, v, courses, axis, corner);
            return true;
        }
        plan.ground = self.packed(here);
        let post = if self.wood.is_some() { BLOCK_STRIPPED_LOG } else { self.stone };
        if (u, v) == (-1, -2) {
            for _ in 0..4 {
                plan.push(post);
            }
        } else if (u, v) == (-1, 2) {
            plan.push(post); // the stump of the one that fell
        } else if let (Some((log, _)), true) = (self.wood, u == 1 && (-2..=2).contains(&v)) {
            // ...lying beside the aisle, where it came down.
            plan.push(oriented(log, self.axis_of_v()));
        } else if (u, v) == (0, 0) {
            plan.ground = BLOCK_COBBLESTONE;
            plan.push(BLOCK_ASH);
        } else if self.chest && (u, v) == self.chest_cell() {
            plan.push(self.chest_block(u, v));
        } else if (u, v) == (a - 1, b - 1) {
            plan.push(BLOCK_JUG);
        } else {
            self.fallen_roof(plan, u, v, u == 0);
        }
        false
    }

    /// A round base of stone, tall at the back and down to two courses
    /// at the door, with what fell of it lying inside and out.
    fn plan_tower(&self, plan: &mut Plan, u: i32, v: i32, here: Column) -> bool {
        let d2 = u * u + v * v;
        // Twelve and a quarter to twenty and a quarter: the cells whose
        // centres lie within half a block of a circle four across. Worked
        // in squares so no float decides what is wall.
        if d2 > 20 {
            // Inside the square, outside the circle: the ground the tower
            // fell onto. Rubble on the door side, which is the side that
            // came down.
            if v < 0 && self.hash(u, v, 0x70E7).is_multiple_of(3) {
                plan.push(self.stone);
            }
            return false;
        }
        if d2 >= 13 {
            if u == 0 && v == -4 {
                return true;
            }
            // Two courses at the door, seven at the back: the lean is
            // along the ruin's own axis, and the turn and the mirror
            // point it anywhere.
            let courses = 2 + (v + 4) * 5 / 8 - (self.hash(u, v, 0x7011) % 2) as i32;
            self.raise_wall(plan, u, v, courses, Axis::Y, false);
            return true;
        }
        plan.ground = if self.hash(u, v, 0x6A7E).is_multiple_of(3) { BLOCK_GRAVEL } else { self.packed(here) };
        if self.chest && (u, v) == self.chest_cell() {
            plan.push(self.chest_block(u, v));
        } else if v >= 1 && self.hash(u, v, 0x70E9).is_multiple_of(3) {
            // Stones from the tall side, fallen in.
            plan.push(self.stone);
        }
        false
    }

    /// A low wall round a yard, a hut in the back corner, a storage pit
    /// dug in the front one and a gravel path from the gate to the door.
    ///
    /// ```text
    ///   # # # # # # # # # # #
    ///   #         # . . c . #
    ///   #  *      # . . . . #
    ///   #         # . . . . #
    ///   #         # # #   # #
    ///   # - - - - - - - . . #    - path
    ///   #         -         #
    ///   #  p p p  -         #    p pit, with the jug in it
    ///   #  p p p  -         #    * a bush
    ///   #  p p p  -         #
    ///   # # # #       # # # #    the gate
    /// ```
    fn plan_yard(&self, plan: &mut Plan, u: i32, v: i32, here: Column) -> bool {
        let (a, b) = self.half;
        let in_hut = u >= 1 && v >= 1;
        let fence = u.abs() == a || v.abs() == b;
        let hut_wall = in_hut && (u == 1 || v == 1);
        if fence || hut_wall {
            if v == -b && u.abs() <= 1 {
                plan.ground = BLOCK_GRAVEL; // the gate
                return true;
            }
            if (u, v) == (3, 1) {
                return true; // the hut's door
            }
            let corner = (u.abs() == a && v.abs() == b)
                || (u == 1 && (v == 1 || v == b))
                || (v == 1 && u == a);
            let tall = if in_hut { 3 } else { 2 };
            let (side, along, axis) = if hut_wall && !fence {
                if v == 1 { (4, u, self.axis_of_u()) } else { (5, v, self.axis_of_v()) }
            } else {
                self.wall_side(u, v)
            };
            let courses = if corner { tall } else { self.standing(side, along, u, v, tall) };
            self.raise_wall(plan, u, v, courses, axis, corner);
            return true;
        }
        if in_hut {
            plan.ground = self.packed(here);
            if self.chest && (u, v) == self.chest_cell() {
                plan.push(self.chest_block(u, v));
            } else {
                self.fallen_roof(plan, u, v, u == 3 && v == 2);
            }
            return false;
        }
        if (-4..=-2).contains(&u) && (-4..=-2).contains(&v) {
            // **The pit is dug, not drawn.** Its floor is a block below
            // the yard, so the jug in it stands where you have to step
            // down to reach it -- and it is the one cell in any ruin
            // whose plan writes under the floor.
            plan.ground = BLOCK_AIR;
            plan.below = Some(BLOCK_DIRT);
            plan.base = 0;
            if (u, v) == (-3, -3) {
                plan.push(BLOCK_JUG);
            }
            return false;
        }
        if (u == 0 && v <= 0) || (v == 0 && (0..=3).contains(&u)) {
            plan.ground = BLOCK_GRAVEL;
            return false;
        }
        if (u, v) == (-4, 3) && self.green() {
            for _ in 0..1 + self.hash(u, v, 0xB05B) % 2 {
                plan.push(BLOCK_BUSH_LEAVES);
            }
        }
        false
    }

    /// Only the footing left: a line of stone flush with the grass round
    /// two rooms, one corner still standing, and a bush where the floor
    /// was. The one kind that is found by walking across it.
    fn plan_footing(&self, plan: &mut Plan, u: i32, v: i32, _here: Column) -> bool {
        let (a, b) = self.half;
        if u.abs() == a || v.abs() == b || v == 0 {
            // The sill is *in* the ground rather than on it: from a
            // distance it is a line in the grass, which is exactly what a
            // foundation looks like after the walls have been carted off.
            plan.ground = self.stone;
            let courses = if (u, v) == (a, b) {
                3
            } else if (u, v) == (a - 1, b) || (u, v) == (a, b - 1) {
                2
            } else {
                i32::from(self.hash(u, v, 0xF007).is_multiple_of(3))
            };
            let axis = if v.abs() == b || v == 0 { self.axis_of_u() } else { self.axis_of_v() };
            self.raise_wall(plan, u, v, courses, axis, false);
            return true;
        }
        if self.chest && (u, v) == self.chest_cell() {
            plan.push(self.chest_block(u, v));
        } else if (u, v) == (a - 1, b - 1) {
            plan.push(BLOCK_JUG);
        } else if (u, v) == (-1, 2) && self.green() {
            for _ in 0..1 + self.hash(u, v, 0xB05B) % 2 {
                plan.push(BLOCK_BUSH_LEAVES);
            }
        }
        false
    }
}

// ---- what was left behind ----
//
// ## Why the chest is filled on the server, the first time it is touched
//
// A chest's contents are not in the terrain -- a cell is one `u16`, and
// what is inside lives in the server's container store, keyed by
// position (see `primitive_server::logic::containers`). So the generator
// can put a chest *here*, and something else has to put the things in it.
//
// **What the chest holds is a pure function of the seed and where it
// stands** (`ruin_chest_loot`), and the server asks it at the first
// moment anybody can tell: when a chest that nobody has ever edited is
// opened, broken, or looked into by a mod. It writes the loot into the
// store and writes the chest's own block back over itself as a world
// edit, and that edit is the seal being broken -- from then on it is an
// ordinary chest, and emptying it empties it for good.
//
// Rejected, both for what they would have cost the save:
//
// * **Stocking when the chunk is generated.** Chunks are regenerated
//   every time one is evicted and every time the server starts, so "the
//   first time" would need its own record of every chunk ever made --
//   and a store entry for every ruin a player ever walked *past*.
// * **A set of opened ruin chests in `chests.bin`.** A new field is a new
//   format version, and a version bump is the one change that file's own
//   note warns loses a player everything they own if it goes wrong. The
//   world overlay already records, per cell, that a player has touched
//   it; a chest written over itself is exactly that fact.

/// Stone-age things a player picks up off the floor, and how many:
/// always two of these. `(block, fewest, most)`.
const KINDLING: [(BlockId, u32, u32); 4] = [
    (BLOCK_STICK, 3, 8),
    (BLOCK_FIBER, 4, 10),
    (BLOCK_PEBBLE, 3, 8),
    (BLOCK_FLINT, 1, 4),
];

/// Things somebody *made* or kept: one or two of these. Each is an hour
/// of the first day saved, and none of them is a step past it.
const MAKINGS: [(BlockId, u32, u32); 6] = [
    (BLOCK_CORD, 1, 3),
    (BLOCK_FLINT_FLAKE, 2, 4),
    (BLOCK_CLAY, 3, 6),
    (BLOCK_SEEDS, 2, 6),
    (BLOCK_BONE, 1, 3),
    (BLOCK_SINEW, 1, 2),
];

/// The odd better thing, when there is no tool or jug in the chest.
const KEEPSAKES: [(BlockId, u32, u32); 4] = [
    (BLOCK_COAL, 2, 5),
    (BLOCK_LEATHER, 1, 1),
    (BLOCK_HIDE, 1, 1),
    (BLOCK_FEATHER, 2, 5),
];

impl Site {
    /// What this ruin's chest holds.
    ///
    /// **Nothing metal, and nothing that goes off.** Metal for the reason
    /// in the module note. Perishables because a chest nobody has opened
    /// has no clock -- the rot pass only walks the store -- so a haunch
    /// found in a ruin a year after the world was made would be fresh,
    /// which is a thing no player would believe. Both are tests, named
    /// for it.
    ///
    /// Four or five stacks -- two things off the floor, one or two
    /// makings, one find -- in scattered slots: a chest somebody left in
    /// a hurry is not sorted.
    fn loot(&self) -> Inventory {
        let mut inventory = Inventory::new();
        let (cx, cy, cz) = self.chest_at().unwrap_or((self.x, self.floor + 1, self.z));
        let mut state = hash2(cx.wrapping_add(cy.wrapping_mul(7919)), cz, self.roll ^ 0x1007) | 1;
        // A xorshift over the position's hash: the same numbers from the
        // same chest on any machine, and a fresh one per draw without a
        // new salt per line.
        let mut next = |below: u32| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state % below.max(1)
        };
        let slots = inventory.slots().len() as u32;
        // Into a free slot picked at random; a chest of forty with six
        // things in it always has one within forty tries.
        let put = |inventory: &mut Inventory, stack: Stack, next: &mut dyn FnMut(u32) -> u32| {
            for _ in 0..slots {
                let slot = next(slots) as usize;
                if inventory.slots()[slot].is_none() {
                    inventory.put_in_slot(slot, stack);
                    return;
                }
            }
        };
        let hot = matches!(self.biome, Biome::Desert | Biome::Savanna);
        let draw = |(block, fewest, most): (BlockId, u32, u32), next: &mut dyn FnMut(u32) -> u32| {
            // Cotton seed where the grain would not grow: the savanna's
            // crop is cotton, and a ruin keeps what its country sowed.
            let block = if block == BLOCK_SEEDS && hot { BLOCK_COTTON_SEEDS } else { block };
            Stack::new(block, fewest + next(most - fewest + 1))
        };

        // Two different things off the floor, then one or two different
        // makings: the second pick steps past the first rather than
        // rerolling, so it always differs and costs one draw.
        for table in [&KINDLING[..], &MAKINGS[..]] {
            let first = next(table.len() as u32) as usize;
            let picks = if table.len() == KINDLING.len() { 2 } else { 1 + next(2) as usize };
            for pick in 0..picks {
                let index = (first + pick * (1 + next(table.len() as u32 - 1) as usize)) % table.len();
                let stack = draw(table[index], &mut next);
                if inventory.count(stack.block) == 0 {
                    put(&mut inventory, stack, &mut next);
                }
            }
        }
        match next(4) {
            // A worn tool: somebody's, used for years, with a season or
            // two left in it. Worn, so it is a find and not a gift -- the
            // player still has to learn to make the next one.
            0 => {
                let tool = if next(2) == 0 { BLOCK_STONE_AXE } else { BLOCK_FLINT_KNIFE };
                let total = crate::types::tool_durability(tool).unwrap_or(1);
                let damage = total * (40 + next(40)) / 100;
                put(&mut inventory, Stack::worn(tool, 1, damage), &mut next);
            }
            // A jug, with something still in it half the time.
            1 => {
                let jug = match next(3) {
                    0 => crate::inventory::filled_jug(if hot { BLOCK_COTTON_SEEDS } else { BLOCK_SEEDS }, 4 + next(8)),
                    1 => crate::inventory::filled_jug(BLOCK_CLAY, 6 + next(10)),
                    _ => Stack::new(BLOCK_JUG, 1),
                };
                put(&mut inventory, jug, &mut next);
            }
            _ => {
                let keepsake = KEEPSAKES[next(KEEPSAKES.len() as u32) as usize];
                let stack = draw(keepsake, &mut next);
                put(&mut inventory, stack, &mut next);
            }
        }
        inventory
    }
}

/// What the ruin chest at `at` holds, or `None` if the generator put no
/// ruin chest there.
///
/// Asked by the server of a chest nobody has touched (see the note above
/// `KINDLING`). Self-checking rather than trusting the caller: the answer
/// is `Some` only for the exact cell this seed's ruin put its chest in,
/// so a chest a player built, a test world's chest, or a chest in a
/// world from before ruins existed all get `None` and stay empty.
pub fn ruin_chest_loot(gen: &WorldGen, at: (i32, i32, i32)) -> Option<Inventory> {
    // The cell is the caller's world; the ruin is on the planet. See
    // `WorldGen::on_planet`.
    let (px, pz) = gen.on_planet(at.0, at.2);
    let at = (px, at.1, pz);
    let (cx, cz) = (at.0.div_euclid(CELL), at.2.div_euclid(CELL));
    if !near_a_candidate(gen.seed, cx, cz, at.0, at.2, HALF) {
        return None;
    }
    let site = gen.ruin_site(cx, cz)?;
    if site.chest_at()? != at {
        return None;
    }
    Some(site.loot())
}

/// Every ruin whose centre lies in the block rectangle `from..to` (`to`
/// exclusive). For tools and tests: each accepted cell costs a site
/// judgement, which is a few hundred column reads.
pub fn ruins_in(gen: &WorldGen, from: (i32, i32), to: (i32, i32)) -> Vec<Ruin> {
    // In and out of the planet frame, for `features_in`'s reason.
    let (from, to) = (gen.on_planet(from.0, from.1), gen.on_planet(to.0, to.1));
    let mut found = Vec::new();
    for cz in from.1.div_euclid(CELL)..=(to.1 - 1).div_euclid(CELL) {
        for cx in from.0.div_euclid(CELL)..=(to.0 - 1).div_euclid(CELL) {
            // Judged first and filtered by where the ruin that stood
            // actually is: which of a cell's spots won is not known until
            // the cell has been judged.
            let Some(site) = gen.ruin_site(cx, cz) else {
                continue;
            };
            if (from.0..to.0).contains(&site.x) && (from.1..to.1).contains(&site.z) {
                let mut ruin = site.summary();
                ruin.centre = gen.off_planet(ruin.centre.0, ruin.centre.1);
                if let Some(chest) = ruin.chest.as_mut() {
                    (chest.0, chest.2) = gen.off_planet(chest.0, chest.2);
                }
                found.push(ruin);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{block_axis, is_leafy, is_liquid, Chunk};
    use std::collections::HashMap;

    /// A few ruins off several seeds, with the site each was built from.
    /// Several seeds because one seed's numbers are one seed's luck.
    fn sample(wanted: usize) -> Vec<(WorldGen, Site)> {
        let mut out = Vec::new();
        for seed in [1337u32, 42, 7, 2024] {
            let gen = WorldGen::new(seed);
            for cz in -3..3 {
                for cx in -3..3 {
                    if let Some(site) = gen.ruin_site(cx, cz) {
                        out.push((WorldGen::new(seed), site));
                        if out.len() >= wanted {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    /// Every chunk a site's columns fall in, generated the ordinary way.
    fn chunks_under(gen: &WorldGen, site: &Site) -> HashMap<ChunkPos, Chunk> {
        let (hx, hz) = site.world_half();
        let mut chunks = HashMap::new();
        for gz in [site.z - hz - RING, site.z + hz + RING] {
            for gx in [site.x - hx - RING, site.x + hx + RING] {
                let (pos, _, _) = ChunkPos::from_global(gx, gz);
                chunks.entry(pos).or_insert_with(|| gen.generate_chunk(pos));
            }
        }
        chunks
    }

    fn block_at(chunks: &HashMap<ChunkPos, Chunk>, gx: i32, y: i32, gz: i32) -> BlockId {
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        chunks[&pos].get(lx, y as usize, lz)
    }

    /// What the site says every cell should hold, built in one piece
    /// from the column tiles -- the ruin as it would be if there were no
    /// such thing as a chunk.
    fn planned(gen: &WorldGen, site: &Site) -> HashMap<(i32, i32, i32), (BlockId, bool)> {
        let (hx, hz) = site.world_half();
        let mut cells = HashMap::new();
        for dz in -(hz + RING)..=hz + RING {
            for dx in -(hx + RING)..=hx + RING {
                let (gx, gz) = (site.x + dx, site.z + dz);
                site.build_column(dx, dz, column(gen, gx, gz), &mut |y, id, overwrite| {
                    // Later writes win, as they do in `put_block`, except
                    // that an into-air write never replaces a planned one.
                    let entry = cells.entry((gx, y, gz)).or_insert((id, overwrite));
                    if overwrite {
                        *entry = (id, overwrite);
                    }
                });
            }
        }
        cells
    }

    #[test]
    fn turning_a_ruin_and_turning_it_back_is_where_it_started() {
        let (_, site) = sample(1).pop().expect("no ruin on any sampled seed");
        for turn in 0..4 {
            for mirror in [false, true] {
                let site = Site { turn, mirror, ..site };
                for v in -5..=5 {
                    for u in -5..=5 {
                        let (dx, dz) = site.to_world(u, v);
                        assert_eq!(site.to_local(dx, dz), (u, v), "turn {turn} mirror {mirror}");
                    }
                }
            }
        }
    }

    /// **The number the whole feature is tuned by**, measured two ways
    /// over a 1920-block square on each of three seeds.
    ///
    /// * **Walking.** Straight lines eighty blocks apart across the
    ///   square, and a ruin counts as met on a line if its centre is
    ///   within thirty-two blocks of it -- close enough to see a wall
    ///   over the grass. Blocks walked *on land* per ruin met is what
    ///   "every few hundred blocks" means to a player crossing country:
    ///   the first version counted the sea as walked, and half of the
    ///   square is sea.
    /// * **Neighbours.** The median distance from a ruin to the nearest
    ///   other one, which is what says they are not in clumps.
    ///
    /// At `CELL` 160, with every rule as strict as it first was, this
    /// printed for seed 1337: 21 ruins, one met per 2304 blocks walked
    /// (sea included), nearest neighbour median 174 -- five times too
    /// rare, which is what `why_ruin_sites_are_refused` was written to
    /// explain.
    ///
    /// At `CELL` 128 (three attempts, the round tree guard; debug build):
    /// seed 1337 -- 71 ruins, land 49% of the walk, one met per 481 blocks
    /// of land, nearest neighbour median 97; seed 42 -- 90, 61%, 385, 106;
    /// seed 7 -- 67, 46%, 392, 112. The player found that too many: a wall
    /// on the skyline of nearly every walk.
    ///
    /// As set now (`CELL` 192): seed 1337 -- 41 ruins, 729 blocks of land
    /// per ruin met, neighbour median 154; seed 42 -- 41, 781, 172; seed 7
    /// -- 34, 770, 141. The bounds keep both failures out: under 500 is the
    /// crowding the player complained of, over 1300 is the rarity
    /// `why_ruin_sites_are_refused` was written to explain.
    #[test]
    fn ruins_turn_up_several_hundred_blocks_apart_on_every_seed() {
        const SIDE: i32 = 1920;
        const LINES: i32 = 24;
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            let ruins = ruins_in(&gen, (-SIDE / 2, -SIDE / 2), (SIDE / 2, SIDE / 2));
            let (mut met, mut walked) = (0usize, 0usize);
            for line in 0..LINES {
                let z = -SIDE / 2 + 40 + line * (SIDE / LINES);
                met += ruins.iter().filter(|r| (r.centre.1 - z).abs() <= 32).count();
                // A step every sixteen blocks, counted as walked only if
                // it is dry land: above the tide, above any lake, and not
                // a river bed.
                let dry = (0..SIDE / 16)
                    .filter(|step| {
                        let gx = -SIDE / 2 + step * 16 + 8;
                        let height = gen.height_at(gx, z);
                        height > SEA_LEVEL + 1
                            && height > gen.water_level_at(gx, z)
                            && !matches!(gen.biome_at(gx, z), Biome::Ocean | Biome::Beach | Biome::River)
                    })
                    .count();
                walked += dry * 16;
            }
            let per_ruin = walked / met.max(1);
            let mut nearest: Vec<i32> = ruins
                .iter()
                .map(|a| {
                    ruins
                        .iter()
                        .filter(|b| b.centre != a.centre)
                        .map(|b| {
                            let (dx, dz) = (a.centre.0 - b.centre.0, a.centre.1 - b.centre.1);
                            ((dx * dx + dz * dz) as f64).sqrt() as i32
                        })
                        .min()
                        .unwrap_or(i32::MAX)
                })
                .collect();
            nearest.sort_unstable();
            let median = nearest.get(nearest.len() / 2).copied().unwrap_or(0);
            println!(
                "seed {seed}: {} ruins, land {}% of the walk, one met per {per_ruin} blocks walked on land, nearest neighbour median {median}",
                ruins.len(),
                walked * 100 / (LINES * SIDE) as usize
            );
            assert!(
                (500..=1300).contains(&per_ruin),
                "seed {seed}: a ruin every {per_ruin} blocks of walking, not a long walk and still a find"
            );
            // Four chunks: a ruin whose nearest neighbour is usually
            // closer than that is standing in sight of it, which makes the
            // two a village rather than two finds.
            assert!(median >= 64, "seed {seed}: ruins in clumps, median neighbour {median} blocks");
        }
    }

    #[test]
    fn a_ruin_is_whole_across_a_chunk_seam() {
        let mut across = 0;
        for (gen, site) in sample(6) {
            // Across a seam means the *walls* cross it. The ring can
            // cross a seam the footprint does not, and the ring holds
            // only fallen stones, which this test does not compare -- so
            // a ruin counted by its ring was a ruin with nothing to find
            // on one side, and the first run failed on exactly that.
            let (hx, hz) = site.world_half();
            let spans = ChunkPos::from_global(site.x - hx, site.z - hz).0
                != ChunkPos::from_global(site.x + hx, site.z + hz).0;
            if !spans {
                continue;
            }
            let chunks = chunks_under(&gen, &site);
            across += 1;
            let mut seen = std::collections::HashSet::new();
            for (&(gx, y, gz), &(id, overwrite)) in &planned(&gen, &site) {
                // Air is left out: the ground cover puts a tuft or a
                // pebble in a planned-empty cell after this pass, which is
                // meant to happen. So is a fallen stone, which is written
                // only into air and rightly yields to a bush or a boulder
                // already standing in its cell.
                if id == BLOCK_AIR || !overwrite {
                    continue;
                }
                assert_eq!(
                    block_at(&chunks, gx, y, gz),
                    id,
                    "{:?} at ({gx},{y},{gz}) is not what its site planned",
                    site.kind
                );
                seen.insert(ChunkPos::from_global(gx, gz).0);
            }
            assert!(seen.len() >= 2, "a ruin over a seam was built on one side of it only");
        }
        assert!(across > 0, "no sampled ruin crosses a chunk seam");
    }

    #[test]
    fn no_ruin_stands_in_water_or_hangs_in_the_air() {
        for (gen, site) in sample(6) {
            let chunks = chunks_under(&gen, &site);
            let (hx, hz) = site.world_half();
            for dz in -hz..=hz {
                for dx in -hx..=hx {
                    for y in site.floor - 1..=site.floor + ROOM {
                        let block = block_at(&chunks, site.x + dx, y, site.z + dz);
                        assert!(!is_liquid(block), "water in a {:?} at floor {}", site.kind, site.floor);
                    }
                }
            }
            // Everything the ruin put up stands on something.
            let (hx, hz) = site.world_half();
            for (&(gx, y, gz), &(id, _)) in &planned(&gen, &site) {
                if id == BLOCK_AIR || block_at(&chunks, gx, y, gz) != id {
                    continue;
                }
                // The plugs under a floor are ground: held by the ground
                // beside them, over whatever cave they close, like any
                // other cell of earth over a cave.
                let under_the_floor = y < site.floor
                    && (gx - site.x).abs() <= hx
                    && (gz - site.z).abs() <= hz;
                if under_the_floor {
                    continue;
                }
                assert_ne!(
                    block_at(&chunks, gx, y - 1, gz),
                    BLOCK_AIR,
                    "{:?} block {id} at ({gx},{y},{gz}) hangs over air",
                    site.kind
                );
            }
        }
    }

    #[test]
    fn a_ruin_chest_holds_simple_things_and_never_metal() {
        const METALS: [&str; 7] = ["copper", "bronze", "iron", "tin", "gold", "ingot", "ore"];
        let mut chests = 0;
        for (gen, site) in sample(24) {
            let Some(at) = site.chest_at() else { continue };
            let loot = ruin_chest_loot(&gen, at).expect("a ruin's own chest has no loot");
            chests += 1;
            let stacks: Vec<Stack> = loot.slots().iter().flatten().copied().collect();
            assert!((3..=7).contains(&stacks.len()), "{} stacks in a ruin chest", stacks.len());
            for stack in stacks {
                let inside = crate::inventory::jug_contents(&stack).map(|(block, _)| block);
                for block in [Some(stack.block), inside].into_iter().flatten() {
                    let name = crate::blocks::definition(block).name;
                    assert!(
                        !METALS.iter().any(|metal| name.contains(metal)),
                        "a ruin chest holds {name}: metal is what the hills are for"
                    );
                    assert!(
                        !crate::food::is_perishable(block),
                        "a ruin chest holds {name}, which would be fresh a year later"
                    );
                }
                assert!(stack.count <= crate::types::stack_limit(stack.block));
                assert!(stack.condition() < 1.0 || crate::types::tool_durability(stack.block).is_none(),
                    "a tool in a ruin that nobody ever used");
            }
        }
        assert!(chests >= 10, "only {chests} chests to look in");
    }

    #[test]
    fn the_chest_stands_exactly_where_its_loot_is_asked_for() {
        let mut checked = 0;
        for (gen, site) in sample(8) {
            let Some(at) = site.chest_at() else { continue };
            let chunks = chunks_under(&gen, &site);
            assert_eq!(block_kind(block_at(&chunks, at.0, at.1, at.2)), BLOCK_CHEST, "no chest where the loot is");
            // Every chest the generator wrote round it is a ruin's -- this
            // one's, or a neighbouring ruin's that shares a chunk with it.
            for (pos, chunk) in &chunks {
                for y in 0..CHUNK_SIZE_Y {
                    for lz in 0..CHUNK_SIZE_Z {
                        for lx in 0..CHUNK_SIZE_X {
                            if block_kind(chunk.get(lx, y, lz)) == BLOCK_CHEST {
                                let gx = pos.x * CHUNK_SIZE_X as i32 + lx as i32;
                                let gz = pos.z * CHUNK_SIZE_Z as i32 + lz as i32;
                                assert!(
                                    ruin_chest_loot(&gen, (gx, y as i32, gz)).is_some(),
                                    "a chest at ({gx},{y},{gz}) nobody will ever fill"
                                );
                            }
                        }
                    }
                }
            }
            for (dx, dy, dz) in [(1, 0, 0), (0, 1, 0), (0, 0, -1)] {
                assert!(ruin_chest_loot(&gen, (at.0 + dx, at.1 + dy, at.2 + dz)).is_none());
            }
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn a_ruin_and_its_chest_are_the_same_every_time_they_are_generated() {
        let (gen, site) = sample(1).pop().expect("a ruin");
        let (pos, _, _) = ChunkPos::from_global(site.x, site.z);
        let here = gen.generate_chunk(pos);
        let loot = site.chest_at().and_then(|at| ruin_chest_loot(&gen, at));
        // On a fresh thread, so none of this one's remembered tiles or
        // verdicts can make the second answer agree with the first.
        let seed = gen.seed;
        let (there, loot_there) = std::thread::spawn(move || {
            let gen = WorldGen::new(seed);
            let chest = gen.ruin_site(site.x.div_euclid(CELL), site.z.div_euclid(CELL)).and_then(|s| s.chest_at());
            (gen.generate_chunk(pos), chest.and_then(|at| ruin_chest_loot(&gen, at)))
        })
        .join()
        .expect("thread");
        assert!(here.blocks == there.blocks, "the same ruin came out two ways");
        assert_eq!(
            loot.map(|l| l.slots().to_vec()),
            loot_there.map(|l| l.slots().to_vec()),
            "the same chest was filled two ways"
        );
    }

    #[test]
    fn no_tree_grows_within_reach_of_a_ruins_walls() {
        let mut wooded = 0;
        for seed in [1337u32, 42, 7, 2024, 99] {
            let gen = WorldGen::new(seed);
            for cz in -6..6 {
                for cx in -6..6 {
                    let Some(site) = gen.ruin_site(cx, cz) else { continue };
                    if !matches!(site.biome, Biome::Forest | Biome::BirchForest | Biome::Taiga) {
                        continue;
                    }
                    wooded += 1;
                    let chunks = chunks_under(&gen, &site);
                    let (hx, hz) = site.world_half();
                    for dz in -(hz + RING)..=hz + RING {
                        for dx in -(hx + RING)..=hx + RING {
                            for y in site.floor + 1..CHUNK_SIZE_Y as i32 {
                                let block = block_at(&chunks, site.x + dx, y, site.z + dz);
                                // Bush leaves are the ruin's own, and a bush
                                // rooted in the ring is undergrowth. A crown
                                // is kept off the walls and not off the ring:
                                // the clearing is round (`ruin_claims`), and a
                                // bough over a fallen stone is allowed.
                                let over_the_walls = dx.abs() <= hx && dz.abs() <= hz;
                                let crown = over_the_walls
                                    && is_leafy(block)
                                    && block_kind(block) != BLOCK_BUSH_LEAVES;
                                let trunk = crate::wood::is_log(block) && block_axis(block) == Axis::Y;
                                assert!(!crown && !trunk, "a tree through a {:?} at seed {seed}", site.kind);
                            }
                        }
                    }
                    if wooded >= 3 {
                        return;
                    }
                }
            }
        }
        assert!(wooded > 0, "no ruin in a wood to check");
    }

    /// What turns candidates down, counted, so the rate is tuned by the
    /// rule that costs the most rather than by guessing.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture why_ruin_sites_are_refused
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn why_ruin_sites_are_refused() {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut total = 0;
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            for cz in -6..6 {
                for cx in -6..6 {
                    total += 1;
                    let key = match gen.judge_ruin_site(cx, cz) {
                        Ok(_) => "accepted".to_string(),
                        Err(Refused::Slope(fall)) => format!("slope {fall}"),
                        Err(other) => format!("{other:?}"),
                    };
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
        let mut lines: Vec<_> = counts.into_iter().collect();
        lines.sort_by_key(|line| std::cmp::Reverse(line.1));
        for (why, count) in lines {
            println!("{why:>12}: {count} of {total}");
        }
    }

    #[test]
    fn the_test_world_has_no_ruins() {
        let gen = WorldGen::with_preset(1337, Preset::Test);
        assert!(ruins_in(&gen, (-640, -640), (640, 640)).is_empty());
        assert!(!gen.ruin_claims(300, 300));
    }

    #[test]
    fn a_ruin_lays_nothing_on_its_floor_that_exists_only_in_a_pack() {
        // For the reason in `features`'s
        // `nothing_the_generator_lays_in_the_world_is_a_thing_that_exists_only_in_a_pack`:
        // an item in a cell is a black cube nobody can pick up. A ruin's
        // floor is where `litter` goes, and the loot table beside it is
        // full of things it would be tempting to scatter there -- a flake,
        // a cord, a bone -- so the floor is held to what the world can draw.
        for (gen, site) in sample(6) {
            for ((x, y, z), (id, _)) in planned(&gen, &site) {
                assert!(
                    !crate::types::is_item(id),
                    "the ruin at ({}, {}) lays {} at ({x}, {y}, {z}), which exists only in a pack",
                    site.x,
                    site.z,
                    crate::types::block_name(id)
                );
            }
        }
    }

    /// What ruins cost chunk generation, from the same chunks.
    ///
    /// ```text
    /// cargo test -p primitive_shared --release --lib -- --ignored --nocapture what_the_ruins_cost_a_chunk
    /// ```
    ///
    /// The pass and the tree guard timed on their own, inside a sweep of
    /// ordinary generation, so the fraction is from one run's conditions.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_the_ruins_cost_a_chunk() {
        use std::time::Instant;
        let gen = WorldGen::new(1337);
        // **Two sets of chunks, because one of them lies.** An ordinary
        // sweep is mostly chunks no ruin reaches, and there the pass is a
        // subtraction -- the first run of this measured 0.0002 ms and
        // said nothing about what building one costs. So the chunks
        // under ruins are timed on their own as well.
        let sweep: Vec<ChunkPos> = (8..32)
            .flat_map(|cz| (8..32).map(move |cx| ChunkPos::new(cx, cz)))
            .collect();
        let mut under: Vec<ChunkPos> = Vec::new();
        for ruin in ruins_in(&gen, (-1280, -1280), (1280, 1280)) {
            let (hx, hz) = ruin.half;
            for gz in [ruin.centre.1 - hz - RING, ruin.centre.1 + hz + RING] {
                for gx in [ruin.centre.0 - hx - RING, ruin.centre.0 + hx + RING] {
                    let pos = ChunkPos::from_global(gx, gz).0;
                    if !under.contains(&pos) {
                        under.push(pos);
                    }
                }
            }
        }
        for (set, chunks) in [("sweep", &sweep), ("under ruins", &under)] {
        for round in ["cold", "warm"] {
            let (mut whole, mut pass, mut guard) = (0f64, 0f64, 0f64);
            for &pos in chunks.iter() {
                {
                    let clock = Instant::now();
                    let chunk = gen.generate_chunk(pos);
                    whole += clock.elapsed().as_secs_f64();
                    let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
                    let columns = ColumnCache::build(&gen, ox, oz);
                    let mut blocks = chunk.blocks.to_vec();
                    let clock = Instant::now();
                    gen.place_ruins(&mut blocks, ox, oz, &columns);
                    pass += clock.elapsed().as_secs_f64();
                    // Every column the tree pass could ask about, which is
                    // many more than roll a tree: an upper bound.
                    let clock = Instant::now();
                    let mut claimed = 0;
                    for lz in -OLD_TREE_REACH..CHUNK_SIZE_Z as i32 + OLD_TREE_REACH {
                        for lx in -OLD_TREE_REACH..CHUNK_SIZE_X as i32 + OLD_TREE_REACH {
                            claimed += usize::from(gen.ruin_claims(ox + lx, oz + lz));
                        }
                    }
                    guard += clock.elapsed().as_secs_f64();
                    std::hint::black_box((blocks, claimed));
                }
            }
            let n = chunks.len().max(1) as f64;
            println!(
                "{set}, {round}: {} chunks | generate_chunk {:.3} ms | ruin pass {:.4} ms | guard on every column {:.4} ms",
                chunks.len(),
                whole * 1000.0 / n,
                pass * 1000.0 / n,
                guard * 1000.0 / n
            );
        }
        }
    }
}
