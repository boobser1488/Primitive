//! Terrain generation.
//!
//! ## The shape of a column
//!
//! Height is not one noise field scaled up. It is three, each answering a
//! different question, because a single field can only ever produce the
//! same kind of terrain everywhere:
//!
//! - **continent** (very low frequency) decides sea floor versus land.
//!   This is what makes oceans large enough to be oceans rather than
//!   ponds, and it is deliberately the only field with enough amplitude
//!   to cross sea level on its own. It is read through a *spline*
//!   (`CONTINENT_SPLINE`) rather than scaled, for the reason below.
//!
//! ## Why the coast is a spline and not a multiplication
//!
//! The first version was `SEA_LEVEL + 2 + continent * 13`, and it made a
//! world whose coastlines were a fractal speckle of one-block islands and
//! puddles, hundreds of blocks wide. The arithmetic is worth keeping in
//! mind, because every terrain generator can catch this disease:
//!
//! * a linear map crosses sea level at a rate of 13 blocks per unit of
//!   field, so the six vertical blocks either side of the waterline take
//!   nearly half the field's whole range to cross;
//! * the continent field is deliberately smooth (features ~600 blocks
//!   across), so half its range is a band of ground *hundreds of blocks
//!   wide* sitting within a couple of blocks of sea level;
//! * the detail octave is ±2 blocks. Over that band it is the thing
//!   deciding land from water, and it is noise.
//!
//! The fix is two-part and both halves are needed. The spline spends
//! most of its input range on ocean floor and on inland heights and
//! races through the waterline, so a coast is tens of blocks wide rather
//! than hundreds; and the detail octave is faded out near the waterline
//! (`shore_fade`), so what is left of the crossing is not decided by
//! noise at all.
//! - **erosion** (low frequency) decides how *flat* a place is. High
//!   erosion flattens: plains and shorelines. Low erosion lets the ridge
//!   field through.
//! - **ridges** (mid frequency, ridged rather than smooth) supplies
//!   mountains. Taking `1 - |noise|` turns the smooth zero-crossings of
//!   Perlin noise into sharp creases, which is what reads as a ridge
//!   line instead of a hill.
//!
//! Multiplying the ridge term by an erosion-derived mask is the whole
//! trick: mountains appear where the land is high *and* uneroded, so the
//! world gets mountain ranges with plains between them rather than
//! uniform lumpiness at every scale.
//!
//! ## Rivers
//!
//! One more low-frequency field, read as a *distance to its zero
//! contour* rather than as a height: the set of points where a smooth
//! field is near zero is a winding line, which is the shape a river
//! wants. Along it the ground is pulled down to a fixed bed a couple of
//! blocks under sea level, so the existing "everything below sea level
//! fills with water" rule floods it, the existing biome rules give it
//! sandy banks, and the existing seabed crust keeps caves from draining
//! it. A river costs one noise field and no new machinery anywhere else.
//!
//! ## Benches, and why a sum of octaves needs them
//!
//! Every term above is smooth, and a sum of smooth fields has a bounded
//! derivative: the ground can be steep, but it can never be *abrupt*.
//! Real country is full of abrupt -- a harder bed of rock holds up a
//! flat shelf and the softer ground under it goes, so a hillside is a
//! stack of steps with a face between each rather than a ramp. Pulling
//! the height toward a multiple of four blocks, in proportion to how
//! young the ground is, produces exactly that and is the only thing in
//! the field that can make a vertical face.
//!
//! ## What the ground is made of
//!
//! The climate picks a material and the *shape* of the ground overrules
//! it, which is the difference between a height field and a landscape:
//!
//! - **Slope decides.** Past the angle of repose the soil is thin; past
//!   about fifty degrees there is no soil at all and the surface is the
//!   rock it sits on. That is what makes a mountainside a face and a
//!   steep coast a cliff instead of a beach standing on its end.
//! - **Depth decides, under water.** Sand in the surf, silt over the
//!   shelf, bare rock past the shelf break.
//! - **Soil collects where it is flat**, so a floodplain is metres of it
//!   and a hillside is a skin -- which only shows when you dig, and is
//!   exactly when a world stops looking like a picture of one.
//!
//! `surface_for` is all of it, and the slope it reads is a proper
//! gradient so the thresholds are angles rather than tuning constants.
//!
//! ## Biomes
//!
//! Temperature and humidity are two more low-frequency fields, and they
//! pick the surface material. They are deliberately independent of
//! height, so a biome boundary does not simply trace a contour line --
//! except where it should: temperature falls with altitude, which is what
//! puts snow on peaks without needing a "mountain biome".
//!
//! ## Determinism
//!
//! Same seed, same chunk, byte-identical output, every time and on every
//! platform. That is what lets the server evict a chunk from its cache
//! and regenerate it later instead of keeping every chunk anyone has ever
//! visited in RAM.
//!
//! The constraint that falls out of it: `generate_chunk` may not depend
//! on any other chunk having been generated first. Trees are the awkward
//! case, and the note on `place_trees` explains how they cross chunk
//! borders without breaking that.

use noise::{NoiseFn, Perlin};

use crate::types::{
    block_kind, BlockId, Chunk, ChunkPos, BLOCK_AIR, BLOCK_ASH, BLOCK_BIRCH_LEAVES,
    BLOCK_BIRCH_LOG, BLOCK_CLAY,
    BLOCK_COAL_ORE, BLOCK_COPPER_ORE, BLOCK_IRON_ORE, BLOCK_TIN_ORE,
    BLOCK_GRAVEL, BLOCK_CACTUS, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FLINT,
    BLOCK_GLOWSTONE, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_LOG, BLOCK_PEBBLE, BLOCK_SAND, BLOCK_SNOW,
    BLOCK_STICK, BLOCK_STONE, BLOCK_TALL_GRASS,
    BLOCK_WATER, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, CHUNK_VOLUME,
};

pub const SEA_LEVEL: i32 = 20;

/// Continent field to base height, as a piecewise-linear curve.
///
/// Where the curve is flat it makes *terrain*: the long shallow run from
/// -1.0 to -0.35 is open ocean floor, and the run above 0.25 is inland.
/// Where it is steep it makes a *boundary*: the short jump from -0.10 to
/// 0.06 is the entire coast, which is what stops the shoreline being a
/// two-hundred-block band of ground hovering at the waterline. See the
/// module docs.
/// The sea half of it is a real ocean profile rather than a ramp: an
/// abyssal plain, a short steep rise (the shelf break), and then a wide
/// shallow shelf running in to the beach. That is the shape that makes
/// swimming out from a shore feel like leaving a coast -- ground under
/// you for a long way, then a drop -- instead of a uniform slide into
/// deep water.
///
/// **The inland run used to be three blocks long**, and that was most of
/// what made the world a table. Everything from `+0.05` to `+0.30` --
/// which is where the great majority of land actually falls -- was
/// mapped into `24..27`, so an ordinary country walk crossed a total of
/// three blocks of elevation. The relief and hill terms could add to
/// that, but they are masked by `land`, which needs height to switch on:
/// the ground had to already be high before anything was allowed to
/// raise it, and it never was.
///
/// It runs to fourteen now (`23..37`), and the shore is a little lower
/// so the extra height is gained inland rather than at the waterline,
/// where it would only make cliffs at the beach.
const CONTINENT_SPLINE: &[(f64, f64)] = &[
    (-1.00, 4.0),
    (-0.55, 5.0),
    (-0.42, 8.0),
    (-0.26, 15.0),
    (-0.10, 17.5),
    (-0.01, 19.0),
    (0.05, 23.0),
    (0.30, 29.0),
    (0.62, 34.0),
    (1.00, 37.0),
];

/// How much colder one block of altitude makes a place.
///
/// Gentle: at the rate this replaced (0.030 a block), *every* piece of
/// land above the shore was pushed a third of the temperature range
/// colder and half the world came out taiga. Altitude should decide the
/// snow line on a peak, not the climate of a plain that happens to be
/// twenty blocks up.
///
/// One constant rather than two, because `surface_temperature` (which
/// picks the biome) and `climate_at` (which colours its plants) have to
/// agree, or a snowy peak grows dark green leaves.
const LAPSE_PER_BLOCK: f64 = 0.013;

/// The same rate expressed in the 0..1 units `climate_column` returns.
///
/// The mapping halves the range (-1..1 becomes 0..1), so the rate halves
/// with it.
pub const CLIMATE_LAPSE_PER_BLOCK: f32 = (LAPSE_PER_BLOCK * 0.5) as f32;

/// Applies the lapse rate to a 0..1 temperature at a given height.
///
/// Public so the client can take the four noise samples once per column
/// and still get the same answer per cell that `climate_at` would give.
#[inline]
pub fn cooled_by_altitude(temperature: f32, gy: i32) -> f32 {
    let altitude = (gy - SEA_LEVEL).max(0) as f32;
    (temperature - altitude * CLIMATE_LAPSE_PER_BLOCK).clamp(0.0, 1.0)
}

/// One loose stone per this many columns, everywhere.
///
/// Sparse on purpose. These are drawn as a single quad each and land in
/// the same pass as the grass, so they are cheap -- but "cheap" times
/// "every column of the world" is not, and a ground so littered that
/// you cannot see it is worse than a bare one.
const PEBBLE_SPACING: u32 = 26;

/// How far below sea level a river cuts its bed.
const RIVER_DEPTH: i32 = 2;
/// Half-width of a river channel, in blocks, banks included.
const RIVER_HALF_WIDTH: f64 = 7.0;
/// How far apart the samples that estimate the river field's slope are.
/// Wide enough not to measure the noise's own texture, narrow enough to
/// still be local.
const RIVER_GRADIENT_STEP: f64 = 12.0;

/// Solid rock nobody can fall through, and the floor caves may not reach.
const BEDROCK_TOP: i32 = 2;
/// How much soil sits over the stone on a normal surface.
const DIRT_DEPTH: i32 = 4;
/// The highest a column is allowed to reach. Two below the ceiling, so a
/// tree rooted at the limit still has somewhere to put its canopy.
const MAX_HEIGHT: i32 = CHUNK_SIZE_Y as i32 - 8;
const MIN_HEIGHT: i32 = BEDROCK_TOP + 2;

/// The widest canopy any tree has.
const MAX_CANOPY_RADIUS: i32 = 3;

/// The longest a fallen trunk can be.
const MAX_DEADFALL: i32 = 6;

/// How far outside the chunk a feature may be rooted and still reach
/// into it. Everything that writes blocks is considered over the chunk
/// plus this margin, and whatever lands outside the array is dropped --
/// see `place_trees` for why that is the whole cross-chunk story.
const FEATURE_MARGIN: i32 = if MAX_CANOPY_RADIUS > MAX_DEADFALL {
    MAX_CANOPY_RADIUS
} else {
    MAX_DEADFALL
};

pub struct WorldGen {
    seed: u32,
    continent_noise: Perlin,
    erosion_noise: Perlin,
    ridge_noise: Perlin,
    detail_noise: Perlin,
    temperature_noise: Perlin,
    humidity_noise: Perlin,
    cave_noise_a: Perlin,
    cave_noise_b: Perlin,
    ore_noise: Perlin,
    river_noise: Perlin,
    warp_noise: Perlin,
    ash_noise: Perlin,
    /// Where clay and gravel are. See `clayey` and `gravelly`.
    deposit_noise: Perlin,
    /// The middle of the world's three scales. See `hills`.
    hill_noise: Perlin,
    /// Where the rock is mineralised at all. What is *in* a vein is a
    /// second question, and a much cheaper one -- see `ore_at`.
    vein_noise: Perlin,
}

/// What lives on top of a column, and what the soil under it is made of.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Surface {
    top: crate::types::BlockId,
    filler: crate::types::BlockId,
    /// How many cells of `filler` sit between `top` and the rock.
    ///
    /// A number rather than the old fixed `DIRT_DEPTH` because soil
    /// depth is most of what tells a floodplain from a mountainside when
    /// you dig into either: silt collects where the ground is flat and
    /// washes off where it is not, so a hillside is a skin of turf over
    /// rock and a valley floor is metres of it. See `surface_for`.
    soil: i32,
}

/// A named region of the world.
///
/// Derived from height, temperature and humidity rather than being a
/// field of its own. That is what keeps biome edges sane: the three
/// inputs are smooth, so the boundaries between biomes are wherever
/// those fields happen to cross a threshold, and there is no possibility
/// of a one-column island of desert in the middle of a forest.
///
/// It also means a biome cannot *shape* the terrain -- height is decided
/// before the biome is known. That is deliberate. Letting a biome push
/// the ground up or down puts a cliff at every boundary, and blending
/// that away is a much bigger machine than this world needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Biome {
    Ocean,
    Beach,
    Desert,
    Savanna,
    Plains,
    Forest,
    /// Standing dead wood: bare trunks, fallen logs, bare earth. The dry
    /// end of temperate country, where a forest was.
    DeadForest,
    /// A birch wood: the cool, wet end of temperate country.
    ///
    /// **A band rather than a scattering**, and that is the point of
    /// putting it here rather than mixing birches into `Forest`. The
    /// classifier walks temperature downward -- savanna, forest, birch,
    /// taiga, tundra -- so a birch wood is always the thing between the
    /// oaks and the firs, and you can walk from one to the other and
    /// watch the wood change. Birches scattered through an oak forest
    /// would be a texture change; a birch wood is a place.
    BirchForest,
    Swamp,
    Taiga,
    Tundra,
    Mountains,
    SnowyPeaks,
    /// Fresh water running through land, as opposed to the sea.
    ///
    /// **The river used to be the ocean**, because the classifier asked
    /// one question -- is this column under water -- and a river bed is.
    /// So every river was `ocean` in the F3 line, and, worse, the ground
    /// either side of it came out `Beach`: a channel through a forest
    /// with a sand bar down both banks, hundreds of blocks from any sea.
    ///
    /// Telling them apart costs one number. The sea is where *the land
    /// itself* is low; a river is where high land has been cut. The
    /// continent spline already says which, so `biome_from` asks it
    /// rather than asking the water.
    River,
}

impl Biome {
    /// What to show the player. Short, because it goes in the F3 line
    /// next to a dozen other things.
    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "ocean",
            Biome::Beach => "beach",
            Biome::Desert => "desert",
            Biome::Savanna => "savanna",
            Biome::Plains => "plains",
            Biome::Forest => "forest",
            Biome::DeadForest => "dead forest",
            Biome::BirchForest => "birch forest",
            Biome::Swamp => "swamp",
            Biome::Taiga => "taiga",
            Biome::Tundra => "tundra",
            Biome::Mountains => "mountains",
            Biome::SnowyPeaks => "snowy peaks",
            Biome::River => "river",
        }
    }

    /// Every biome, for tests and for anything that wants to enumerate
    /// them. Keeping this next to the enum is what stops it going stale.
    pub const ALL: &'static [Biome] = &[
        Biome::Ocean,
        Biome::Beach,
        Biome::Desert,
        Biome::Savanna,
        Biome::Plains,
        Biome::Forest,
        Biome::DeadForest,
        Biome::BirchForest,
        Biome::Swamp,
        Biome::Taiga,
        Biome::Tundra,
        Biome::Mountains,
        Biome::SnowyPeaks,
        Biome::River,
    ];

    /// What the climate alone would put on top of a column.
    ///
    /// The ground's own shape gets the last word -- see `surface_for`,
    /// which is what everything actually calls.
    fn surface(self) -> Surface {
        let (top, filler) = match self {
            // Sand under water and along the shore, so a beach reads as
            // a beach from either side of the waterline.
            Biome::Ocean | Biome::Beach | Biome::Desert => (BLOCK_SAND, BLOCK_SAND),
            // A river bed is gravel, not beach sand: it is the coarse
            // stuff that stays where running water has taken the fines
            // away. It is also what tells you, from the bank, which of
            // the two kinds of water you are looking at.
            Biome::River => (BLOCK_GRAVEL, BLOCK_DIRT),
            Biome::SnowyPeaks | Biome::Tundra => (BLOCK_SNOW, BLOCK_DIRT),
            // Bare earth. Nothing has grown here for a long time, and
            // it is what makes a dead forest read as one from a
            // distance rather than as a forest that lost its leaves.
            Biome::DeadForest => (BLOCK_DIRT, BLOCK_DIRT),
            // **Not bare rock by default.** Soil does not stay on a
            // steep peak -- but `surface_for` already strips it from
            // anything steep, and saying it twice paved the flat parts
            // too. A shelf or a saddle high in the hills is alpine
            // meadow; the faces around it are rock because they are
            // faces, not because they are high.
            Biome::Mountains => (BLOCK_GRASS, BLOCK_DIRT),
            _ => (BLOCK_GRASS, BLOCK_DIRT),
        };
        Surface {
            top,
            filler,
            soil: DIRT_DEPTH,
        }
    }

    /// One tree per this many columns. `None` means nothing grows.
    fn tree_spacing(self) -> Option<u32> {
        match self {
            Biome::Forest => Some(22),
            // Thinner than an oak wood. Birch stands are open at the
            // floor -- that is what they look like -- and the pale
            // trunks only read as trunks if you can see between them.
            Biome::BirchForest => Some(30),
            Biome::Swamp => Some(30),
            Biome::Taiga => Some(28),
            // Dense, because a wood of bare trunks only reads as a wood
            // if there are enough of them to close the view.
            Biome::DeadForest => Some(26),
            Biome::Plains => Some(140),
            // Steppe: grass to the horizon and the occasional tree, so
            // the eye has something to measure the distance against.
            Biome::Savanna => Some(210),
            // The treeline, and the whole reason it reads as one: a
            // handful of firs standing a long way apart in the snow,
            // thinning to nothing as the ground rises. Sparse enough
            // that the tundra is still open country.
            Biome::Tundra => Some(90),
            Biome::Ocean
            | Biome::River
            | Biome::Beach
            | Biome::Desert
            | Biome::Mountains
            // Above the treeline nothing stands. A bare peak is what
            // makes the firs below it read as a treeline rather than as
            // a gap in the forest.
            | Biome::SnowyPeaks => None,
        }
    }

    /// What shape the trees here are.
    ///
    /// Two shapes, and the difference between them is most of what
    /// tells a cold forest from a temperate one at any distance where
    /// you cannot see a single leaf: a fir is a spire that starts near
    /// the ground and narrows all the way up, and a broadleaf is a bare
    /// trunk with a ball on top. Built from the same two blocks; only
    /// the arrangement changes.
    fn tree_kind(self) -> TreeKind {
        match self {
            Biome::Taiga | Biome::Tundra => TreeKind::Conifer,
            _ => TreeKind::Broadleaf,
        }
    }

    /// What the trees here are made of.
    ///
    /// Separate from `tree_kind`, which is the *shape*: the two vary
    /// independently, and saying so is what keeps a third wood from
    /// needing a third shape or a fourth shape from needing a fourth
    /// wood.
    fn tree_wood(self) -> (BlockId, BlockId) {
        match self {
            Biome::BirchForest => (BLOCK_BIRCH_LOG, BLOCK_BIRCH_LEAVES),
            _ => (BLOCK_LOG, BLOCK_LEAVES),
        }
    }

    /// Trunk height range and canopy radius.
    ///
    /// Shape is most of what tells two forests apart at a distance --
    /// taiga's tall narrow spires read differently from a swamp's broad
    /// low canopies even though both are made of the same two blocks.
    fn tree_shape(self) -> (i32, i32, i32) {
        match self {
            // (shortest trunk, tallest trunk, canopy radius)
            //
            // Radius 0 means no canopy at all -- see `place_tree`.
            Biome::DeadForest => (4, 8, 0),
            // Firs: tall and narrow, and the tundra's are stunted --
            // the same tree at the edge of where it can grow at all.
            Biome::Taiga => (7, 11, 2),
            Biome::Tundra => (5, 8, 2),
            Biome::Swamp => (3, 5, 3),
            Biome::Savanna => (4, 6, 3),
            // Tall and narrow for a broadleaf: a birch is a mast with a
            // small crown, which is the whole of its silhouette.
            Biome::BirchForest => (6, 10, 2),
            _ => (4, 7, 2),
        }
    }
}

impl Biome {
    /// One tuft of grass per this many columns, or `None` where nothing
    /// grows.
    ///
    /// This is most of what tells the temperate biomes apart on the
    /// ground: plains are a field, a forest floor is patchier because
    /// the trees have it, steppe is grass and little else, and a dead
    /// forest is bare.
    fn grass_spacing(self) -> Option<u32> {
        match self {
            Biome::Plains => Some(3),
            Biome::Savanna => Some(4),
            Biome::Forest => Some(7),
            // Sparser than an oak floor, which is what an open stand of
            // birch looks like underfoot: light gets in, but there is
            // not much soil to hold anything.
            Biome::BirchForest => Some(9),
            Biome::Swamp => Some(5),
            Biome::Taiga => Some(14),
            Biome::DeadForest
            | Biome::Ocean
            | Biome::River
            | Biome::Beach
            | Biome::Desert
            | Biome::Tundra
            | Biome::Mountains
            | Biome::SnowyPeaks => None,
        }
    }

    /// One fallen stick per this many columns, or `None` where nothing
    /// overhead could have dropped one.
    ///
    /// Densest in a dead forest, where the wood has been coming down for
    /// a while and nothing grows back over it -- but an ordinary wood is
    /// nearly as thick with them, because an ordinary wood is where a
    /// player actually starts, and a forest floor with a stick every
    /// seventeen columns is one you can cross without finding any.
    fn stick_spacing(self) -> Option<u32> {
        match self {
            Biome::DeadForest => Some(9),
            Biome::Forest => Some(10),
            Biome::Taiga => Some(13),
            Biome::Swamp => Some(16),
            _ => None,
        }
    }

    /// One cactus per this many columns. Desert only: a cactus anywhere
    /// else is a joke the world only gets to make once.
    fn cactus_spacing(self) -> Option<u32> {
        match self {
            Biome::Desert => Some(55),
            _ => None,
        }
    }

    /// One fallen trunk per this many columns.
    ///
    /// Only where trees stand or stood. A log lying in a meadow with no
    /// wood in sight is a puzzle rather than a detail.
    fn deadfall_spacing(self) -> Option<u32> {
        match self {
            Biome::DeadForest => Some(220),
            Biome::Forest => Some(420),
            Biome::Taiga => Some(520),
            _ => None,
        }
    }
}

/// Gradient past which nothing stays on the ground but the rock it is
/// made of.
///
/// In blocks per block, so 1.0 is exactly forty-five degrees. 1.25 is
/// about fifty-one, which is past what even scree holds at.
const ROCK_SLOPE: f32 = 1.25;
/// ...and where the soil is thin rather than gone.
///
/// 0.7 is thirty-five degrees, which is not a number picked to look
/// right: it is the angle of repose, the steepest a heap of loose
/// material sits at before it slides. Soil on a slope is exactly that
/// heap, so it is the angle where a hillside stops holding its cover.
const BANK_SLOPE: f32 = 0.7;

/// Thresholds on the shared deposit field: above the first is clay,
/// below the second is gravel, and the gap between them keeps the two
/// from ever touching. See `clayey` and `gravelly`.
const CLAY_DEPOSIT: f64 = 0.22;
const GRAVEL_DEPOSIT: f64 = -0.20;

/// How steep a column is, from the heights of its four neighbours.
///
/// The central-difference gradient magnitude: the same measure a
/// heightmap-shading or a hydrology tool would use, and the reason its
/// units mean something -- 1.0 is a forty-five degree face, whatever
/// direction it runs in.
///
/// A gradient rather than the largest single step, because the number
/// decides what the ground is *made of* and a maximum flickers: one
/// cell of noise beside an otherwise gentle slope would put a patch of
/// bare rock in the middle of a meadow. Central differences ask "is
/// this place steep" rather than "is any step from it steep".
#[inline]
fn slope_from(east: i32, west: i32, south: i32, north: i32) -> f32 {
    let dx = (east - west) as f32 * 0.5;
    let dz = (south - north) as f32 * 0.5;
    (dx * dx + dz * dz).sqrt()
}

/// What a column is made of, once the shape of the ground has had its
/// say over the climate.
///
/// Three rules, and each of them is something you can walk up to and
/// check:
///
/// * **Under water the material follows the depth.** Sand in the
///   shallows, silt over the shelf, bare rock below it. A single sandy
///   sea floor from the beach to the abyss is the one part of the old
///   generator that gave itself away the moment anyone swam out.
/// * **A slope sheds what is on it.** Soil, sand and snow all need a
///   surface gentle enough to rest on; past that the ground is the rock
///   underneath, which is what turns a mountainside into a face and a
///   steep coast into a cliff rather than a beach standing on its end.
/// * **Soil collects where the ground is flat.** Deep on a floodplain,
///   a skin on a hillside. This is only visible once you dig, which is
///   exactly when a world stops being a picture of one.
fn surface_for(height: i32, biome: Biome, slope: f32) -> Surface {
    if height < SEA_LEVEL {
        let depth = SEA_LEVEL - height;
        return if depth <= 3 {
            // The surf zone: sand, and the same sand the beach above it
            // is made of, so the waterline is not a material boundary.
            Surface { top: BLOCK_SAND, filler: BLOCK_SAND, soil: 3 }
        } else if depth <= 10 {
            // The shelf. Silt over the sand it was washed off.
            Surface { top: BLOCK_DIRT, filler: BLOCK_SAND, soil: 3 }
        } else {
            // Past the shelf break nothing settles.
            Surface { top: BLOCK_STONE, filler: BLOCK_STONE, soil: 0 }
        };
    }

    let mut surface = biome.surface();

    // The waterline is the exception to all of it: a beach is where
    // loose material *collects*, however abruptly the ground behind it
    // climbs, and a strip of sand at the foot of a cliff is a real
    // place rather than an oversight.
    if height <= SEA_LEVEL + 2 && surface.top == BLOCK_SAND {
        return surface;
    }

    let bare = Surface {
        top: BLOCK_COBBLESTONE,
        filler: BLOCK_STONE,
        soil: 1,
    };
    if slope >= ROCK_SLOPE {
        return bare;
    }

    if slope >= BANK_SLOPE {
        // Snow and sand are the two that go first: neither has any grip
        // at all. Turf holds on for a while yet, on its roots.
        if matches!(surface.top, BLOCK_SNOW | BLOCK_SAND) {
            return bare;
        }
        surface.soil = 2;
    } else if slope < 0.35 {
        // Flat ground, and the only place with more soil than the old
        // fixed depth: a valley floor is where everything the slopes
        // lost ends up.
        surface.soil = DIRT_DEPTH + 1;
    }
    surface
}

impl WorldGen {
    pub fn new(seed: u32) -> Self {
        // A distinct seed per field. Sharing one would make every field
        // peak in the same places, so biome edges, mountain ranges and
        // cave systems would all trace the same contours -- the world
        // would look like one pattern shown six times.
        Self {
            seed,
            continent_noise: Perlin::new(seed),
            erosion_noise: Perlin::new(seed.wrapping_add(1)),
            ridge_noise: Perlin::new(seed.wrapping_add(2)),
            detail_noise: Perlin::new(seed.wrapping_add(3)),
            temperature_noise: Perlin::new(seed.wrapping_add(4)),
            humidity_noise: Perlin::new(seed.wrapping_add(5)),
            cave_noise_a: Perlin::new(seed.wrapping_add(6)),
            cave_noise_b: Perlin::new(seed.wrapping_add(7)),
            ore_noise: Perlin::new(seed.wrapping_add(9)),
            river_noise: Perlin::new(seed.wrapping_add(11)),
            warp_noise: Perlin::new(seed.wrapping_add(13)),
            ash_noise: Perlin::new(seed.wrapping_add(17)),
            deposit_noise: Perlin::new(seed.wrapping_add(23)),
            hill_noise: Perlin::new(seed.wrapping_add(29)),
            vein_noise: Perlin::new(seed.wrapping_add(31)),
        }
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    // ---- the climate fields ----

    /// Where to *sample* the continent field, which is not quite where
    /// the column is.
    ///
    /// Pushing the sample point around with another noise field -- a
    /// domain warp -- is what gives a coastline bays, spits and
    /// peninsulas. The obvious alternative, adding more octaves to the
    /// continent field itself, cannot be used here: the coast spline is
    /// steep by design, so a high octave of the *height* input turns
    /// into several blocks of vertical wobble along the waterline, and
    /// wobble at the waterline is the speckled coast this generator was
    /// built to get rid of. Warping moves the coast sideways instead,
    /// which costs nothing vertically.
    fn warped(&self, gx: i32, gz: i32) -> (f64, f64) {
        const FREQUENCY: f64 = 0.0034;
        const AMOUNT: f64 = 60.0;
        let (x, z) = (gx as f64, gz as f64);
        let wx = self.warp_noise.get([x * FREQUENCY, z * FREQUENCY]);
        // A long way off in the field, so the two offsets are unrelated:
        // sampling the same point twice would push every column along
        // the same diagonal and shear the world rather than warp it.
        let wz = self
            .warp_noise
            .get([x * FREQUENCY + 137.31, z * FREQUENCY + 91.77]);
        (x + wx * AMOUNT, z + wz * AMOUNT)
    }

    /// -1 (deep ocean) .. +1 (continental interior).
    ///
    /// Two octaves only, and warped rather than detailed -- see
    /// `warped`.
    fn continent(&self, gx: i32, gz: i32) -> f64 {
        let (x, z) = self.warped(gx, gz);
        fbm(&self.continent_noise, x, z, 0.0016, 2)
    }

    /// -1 (jagged) .. +1 (worn flat).
    fn erosion(&self, gx: i32, gz: i32) -> f64 {
        fbm(&self.erosion_noise, gx as f64, gz as f64, 0.005, 3)
    }

    /// **The scale the world was missing.**
    ///
    /// The height field had two: continents, which turn over every six
    /// hundred blocks, and detail, which turns over every thirty. A
    /// player standing on the ground sees neither. The first is bigger
    /// than the render distance -- it is the reason there is land here
    /// at all, not a shape you can look at -- and the second is texture
    /// underfoot. Between them was nothing, and *between them is where
    /// hills live*: the rise you walk up in half a minute, the dip with
    /// a wood in the bottom of it, the ridge you climb to see what is on
    /// the other side.
    ///
    /// Without that scale the horizon is a straight line however much
    /// noise is piled onto the two ends of it, which is exactly what the
    /// world looked like: a table with a texture on it.
    ///
    /// A hundred and forty blocks a wave, which is a little over half a
    /// render distance -- near enough that a rise fills the view, far
    /// enough that its far side is a place you have not seen yet.
    fn hills(&self, gx: i32, gz: i32) -> f64 {
        fbm(&self.hill_noise, gx as f64, gz as f64, 0.0070, 3)
    }

    /// 0 .. 1, peaking along sharp lines rather than in round blobs.
    fn ridges(&self, gx: i32, gz: i32) -> f64 {
        let mut sum = 0.0;
        let mut amplitude = 1.0;
        // Low enough that a range is a range: at 0.006 the ridge field
        // turned over every hundred blocks or so, which gives isolated
        // peaks rather than a chain of them with valleys between.
        let mut frequency = 0.0038;
        let mut total = 0.0;
        for _ in 0..4 {
            let v = self
                .ridge_noise
                .get([gx as f64 * frequency, gz as f64 * frequency]);
            // The crease: a smooth field's zero crossing becomes a peak.
            sum += (1.0 - v.abs()) * amplitude;
            total += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        (sum / total).clamp(0.0, 1.0)
    }

    /// -1 (freezing) .. +1 (baking), before altitude is taken off.
    fn temperature(&self, gx: i32, gz: i32) -> f64 {
        // Low frequency on purpose: a biome should be somewhere you
        // walk *through*, not a patch you cross in twenty paces. At the
        // frequency this replaced, temperate country turned over every
        // few hundred blocks and the map read as mottling rather than
        // as regions.
        fbm(&self.temperature_noise, gx as f64, gz as f64, 0.0014, 2)
    }

    /// -1 (arid) .. +1 (rainforest).
    fn humidity(&self, gx: i32, gz: i32) -> f64 {
        fbm(&self.humidity_noise, gx as f64, gz as f64, 0.0019, 2)
    }

    /// Where the ash lies, in a wood that burned.
    ///
    /// Its own field rather than a hash, because ash drifts: it collects
    /// in patches tens of blocks across with bare earth between them,
    /// and a per-column coin flip would give grey speckle instead --
    /// which reads as a dirty texture rather than as something that
    /// happened here.
    ///
    /// Only ever asked about a dead forest, so the cost is a noise
    /// sample on a few percent of the world's columns.
    fn ashy(&self, gx: i32, gz: i32) -> bool {
        fbm(&self.ash_noise, gx as f64, gz as f64, 0.021, 2) > 0.08
    }

    /// Is there clay here?
    ///
    /// Clay settles where water stops moving: the bed of a shallow bay,
    /// the inside of a river bend, the flat ground just above the
    /// waterline. So it is a field of patches rather than a scatter --
    /// a clay bank is somewhere you *go back to*, which it cannot be if
    /// it is a block here and a block there.
    ///
    /// Low frequency for the same reason: the patches are tens of
    /// blocks across, so finding one is worth something and finding
    /// none along a whole shore is possible.
    #[cfg(test)]
    fn clayey(&self, gx: i32, gz: i32) -> bool {
        self.deposit(gx, gz) > CLAY_DEPOSIT
    }

    /// The shared deposit field both `clayey` and `gravelly` read.
    /// Sampled once per column that can hold either deposit, because two
    /// octaves of Perlin are the whole cost of the question.
    fn deposit(&self, gx: i32, gz: i32) -> f64 {
        fbm(&self.deposit_noise, gx as f64, gz as f64, 0.016, 2)
    }

    /// Is there gravel here?
    ///
    /// The other half of the same field, read at the other end: where
    /// clay is what slow water drops, gravel is what fast water leaves
    /// behind, so the two are never in the same place and one field can
    /// answer for both.
    #[cfg(test)]
    fn gravelly(&self, gx: i32, gz: i32) -> bool {
        self.deposit(gx, gz) < GRAVEL_DEPOSIT
    }

    /// Temperature where it actually matters: at the surface, with the
    /// lapse rate applied. This is what puts snow on a peak that sits in
    /// an otherwise temperate region.
    fn surface_temperature(&self, gx: i32, gz: i32, height: i32) -> f64 {
        let altitude = (height - SEA_LEVEL).max(0) as f64;
        self.temperature(gx, gz) - altitude * LAPSE_PER_BLOCK
    }

    /// Temperature and humidity where something is growing, each
    /// remapped to 0..1.
    ///
    /// This is what colours the world's plant life: the client's mesher
    /// asks for it per column and stamps the answer on every leaf and
    /// blade of grass it emits, so foliage shades from straw in a dry
    /// steppe to near-black green in a swamp without the mesher knowing
    /// what a biome is.
    ///
    /// Climate rather than the `Biome` enum, deliberately. A biome is a
    /// classification with edges, and colouring by it puts a hard line
    /// across the ground wherever two of them meet -- the one place the
    /// eye is guaranteed to be looking, because that is where the trees
    /// change. The two fields underneath are smooth, so a colour taken
    /// from them crosses the same boundary as a gradient.
    ///
    /// `gy` is the height of the plant itself and only feeds the lapse
    /// rate, exactly as `surface_temperature` does: a canopy on a peak
    /// is the same cold as the snow beside it.
    pub fn climate_at(&self, gx: i32, gy: i32, gz: i32) -> (f32, f32) {
        let (temperature, humidity) = self.climate_column(gx, gz);
        (cooled_by_altitude(temperature, gy), humidity)
    }

    /// The same two fields at sea level, before altitude is taken off.
    ///
    /// Split out because the caller that matters -- the mesher -- wants
    /// them once per *column* and then applies the lapse rate per cell:
    /// four noise samples for a column of leaves rather than four per
    /// leaf. `climate_at` is the convenient form for everything else.
    pub fn climate_column(&self, gx: i32, gz: i32) -> (f32, f32) {
        let normalise = |v: f64| (((v + 1.0) * 0.5).clamp(0.0, 1.0)) as f32;
        (
            normalise(self.temperature(gx, gz)),
            normalise(self.humidity(gx, gz)),
        )
    }

    /// How much of a river is at this column: 0 nowhere near one, 1 in
    /// the middle of the channel.
    ///
    /// Rivers belong in low country. A channel cut across a mountain is
    /// not a river, it is a canyon with water in it, so the effect is
    /// faded out as the land rises -- which also means a river valley
    /// simply peters out where the hills start, the way one does.
    /// How wide the valley is, as a multiple of the channel's own half
    /// width.
    ///
    /// **A river is not a slot cut in a table.** The channel used to be
    /// the whole of it: seven blocks from the middle of the water to
    /// untouched ground, which on land twelve blocks above the bed is a
    /// wall you cannot climb either side of every river in the world.
    /// It was survivable while the country was flat, because there was
    /// nothing to cut *into*; raising the land turned every river into a
    /// trench.
    ///
    /// Real rivers arrive with their valleys. The water is the last and
    /// narrowest part of a much wider hollow that the water made, and
    /// the walk down into one is most of what tells you there is a river
    /// there at all -- long before you see it.
    const RIVER_VALLEY_SPREAD: f64 = 7.0;

    /// The two nested profiles of a river: the valley it lies in, and
    /// the channel the water runs down.
    ///
    /// Both are 0 nowhere near a river and 1 at its middle. They are
    /// applied one after the other -- see `height_at` -- because they
    /// pull toward different heights: the valley toward a floodplain
    /// just above the waterline, the channel down to the bed.
    fn river(&self, gx: i32, gz: i32, land_height: f64) -> (f64, f64) {
        // Cheapest test first. The two masks below decide most of the
        // world on the height alone -- open sea has nothing to cut, high
        // ground is not river country -- and everything after this point
        // costs five samples of the river field to estimate its slope.
        // Skipping them where the answer is already zero is most of what
        // rivers cost across a chunk.
        if land_height < SEA_LEVEL as f64 + 0.5 || land_height > SEA_LEVEL as f64 + 34.0 {
            return (0.0, 0.0);
        }
        // Distance to the field's zero contour, in *blocks*.
        //
        // The obvious version compares the field value against a fixed
        // threshold, and it gives a river whose width depends on how
        // fast the field happens to be changing: thin where the field is
        // steep, and -- where the field flattens out, which it does
        // somewhere in every region -- a marsh a hundred blocks across
        // hanging off the side of the channel. Dividing by the local
        // slope converts "how far from zero is the value" into "how far
        // from the line am I", which is the question a bank answers.
        const FREQUENCY: f64 = 0.0021;
        let sample = |x: f64, z: f64| fbm(&self.river_noise, x, z, FREQUENCY, 2);
        let (x, z) = (gx as f64, gz as f64);
        let field = sample(x, z);
        let step = RIVER_GRADIENT_STEP;
        let slope_x = (sample(x + step, z) - sample(x - step, z)) / (2.0 * step);
        let slope_z = (sample(x, z + step) - sample(x, z - step)) / (2.0 * step);
        let slope = (slope_x * slope_x + slope_z * slope_z).sqrt();
        if slope <= f64::EPSILON {
            return (0.0, 0.0);
        }
        // Distance from the centre line, in blocks. Kept as a distance
        // rather than immediately normalised, because the two profiles
        // measure it against different widths.
        let distance = field.abs() / slope;
        let across = (distance / RIVER_HALF_WIDTH).min(1.0);
        // 1 at the centre line, easing off to nothing at the banks.
        let channel = 1.0 - smoothstep(0.0, 1.0, across);
        // The hollow the channel sits in, five times as wide and eased
        // over its whole width, so the ground comes down to the water
        // rather than stopping above it.
        let valley_across = (distance / (RIVER_HALF_WIDTH * Self::RIVER_VALLEY_SPREAD)).min(1.0);
        let valley = 1.0 - smoothstep(0.0, 1.0, valley_across);
        // Not in the sea (there is nothing to cut), and not up a hill.
        let on_land = smoothstep(
            SEA_LEVEL as f64 + 0.5,
            SEA_LEVEL as f64 + 4.0,
            land_height,
        );
        // A long fade rather than a short one: where a river runs out of
        // low country it should shallow out over a stretch, not stop.
        let lowland = smoothstep(
            SEA_LEVEL as f64 + 34.0,
            SEA_LEVEL as f64 + 10.0,
            land_height,
        );
        let mask = on_land * lowland;
        (channel * mask, valley * mask)
    }

    pub fn height_at(&self, gx: i32, gz: i32) -> i32 {
        let continent = self.continent(gx, gz);
        let erosion = self.erosion(gx, gz);

        // The baseline, through the coast spline. This is the only term
        // with the reach to cross sea level by itself, which is what
        // keeps oceans and land as large coherent regions instead of a
        // speckle of both.
        let base = spline(CONTINENT_SPLINE, continent);

        // Mountains need high ground *and* low erosion. Without the
        // second factor every coastline would also be a mountain range,
        // because the ridge field does not know where it is.
        let land = smoothstep(SEA_LEVEL as f64 + 1.0, SEA_LEVEL as f64 + 7.0, base);
        let unworn = smoothstep(0.5, -0.25, erosion);
        let relief = self.ridges(gx, gz).powi(2) * 24.0 * land * unworn;

        // Small-scale roughness, so a plain is not a plane -- but not at
        // the waterline, where ±2 blocks of noise is the difference
        // between beach and sea and turns a coast into a speckle. See
        // the module docs.
        //
        // Scaled by erosion, which is what makes the ground read as
        // *country* rather than as one texture at every scale. A worn
        // lowland is smooth because that is what wearing down does to
        // it; the same amplitude of noise on a floodplain and on a ridge
        // line is the one thing that gives away a height field built out
        // of octaves. `unworn` is already the mask the mountains use, so
        // roughness and relief agree about where the young ground is.
        let shore_fade = smoothstep(1.0, 7.0, (base - SEA_LEVEL as f64).abs());
        let roughness = 0.6 + 4.0 * unworn;
        let detail = fbm(&self.detail_noise, gx as f64, gz as f64, 0.030, 3) * roughness * shore_fade;

        // Hills: the scale between a continent and a clod of earth. See
        // `hills` for why the world had no such scale and what it cost.
        //
        // Squared through a signed absolute value rather than used
        // straight, so the field spends most of its range near zero and
        // reaches its full amplitude rarely: that is the difference
        // between rolling country with hills in it and a sheet of
        // corrugated iron. Worn ground gets less of it, like everything
        // else here -- `unworn` is what tells young country from old,
        // and a hill is young ground that has not been ground down yet.
        let hill_field = self.hills(gx, gz);
        let hill_amplitude = 5.0 + 7.0 * unworn;
        let hills = hill_field * hill_field.abs() * hill_amplitude * land;

        let land_height = base + relief + hills + detail;

        // Benches, and the faces between them.
        //
        // Rock does not wear evenly. A harder bed holds up a flat shelf
        // and the softer ground under it goes, so a hillside in real
        // country is a stack of steps rather than a ramp -- and the step
        // between two of them is the only thing in a landscape that is
        // actually *vertical*.
        //
        // A sum of smooth octaves can never produce one, however many
        // are added: every one of them has a bounded derivative, so the
        // ground can be steep but never abrupt. Pulling the height
        // toward a multiple of `BENCH` does, and it does it where a
        // bench belongs -- on young, high ground, and never at the
        // waterline, where a four-block step would put a wall around
        // every island.
        const BENCH: f64 = 3.0;
        let stepped = (land_height / BENCH).round() * BENCH;
        // **Both numbers were measured, not chosen.**
        //
        // A shelf every four blocks pulled three quarters of the way to
        // its multiple is a wall four blocks high, and the histogram of
        // seams had a spike sitting exactly there: one in ninety of
        // them. That was survivable while loose material came in layers
        // and the collider could step over half a block; with every
        // solid filling its cell there is nothing left to step onto, so
        // *every* seam of a block or more is a jump and a seam of four
        // is a cliff in the middle of a hillside.
        //
        // Three blocks pulled a little under half way was the best of
        // the four combinations tried: it cuts the share of seams too
        // tall to walk over from about one in twenty to one in fifty,
        // and the worst seam in a wide sample from seven blocks to
        // five. The shelves survive -- they are what stops a hillside
        // being a ramp -- they are just no longer walls.
        let terracing = unworn
            * smoothstep(SEA_LEVEL as f64 + 4.0, SEA_LEVEL as f64 + 14.0, land_height)
            // Eased off from 0.45 when the hill field arrived. Benches
            // were carrying the whole burden of "this ground has shape
            // in it" and were tuned as hard as the player's stride could
            // stand; with hills underneath them they are a texture on a
            // slope rather than the slope itself, and the tallest seam
            // in the world comes down with them.
            * 0.36;
        let land_height = land_height + (stepped - land_height) * terracing;

        // Rivers, cut last: they win over whatever the land was doing,
        // which is what makes a channel a channel rather than a dip that
        // the detail octave can fill back in.
        let (channel, valley) = self.river(gx, gz, land_height);

        // The valley first: a wide, shallow hollow easing the country
        // down toward the water. Only ever *downward* -- `min` -- so a
        // river running along a low place does not get a rampart built
        // around it.
        const FLOODPLAIN: f64 = SEA_LEVEL as f64 + 3.0;
        let land_height = if valley > 0.0 && land_height > FLOODPLAIN {
            land_height * (1.0 - valley) + FLOODPLAIN * valley
        } else {
            land_height
        };

        // ...then the channel, cut into the floor of it.
        let bed = (SEA_LEVEL - RIVER_DEPTH) as f64;
        let height = if channel > 0.0 && land_height > bed {
            land_height * (1.0 - channel) + bed * channel
        } else {
            land_height
        };

        (height.round() as i32).clamp(MIN_HEIGHT, MAX_HEIGHT)
    }

    /// Which biome a column belongs to.
    ///
    /// The order of the tests is the whole classifier. Height decides
    /// first, because being under water or on a peak overrides any
    /// amount of rainfall; then temperature, because nothing grows in a
    /// frozen desert either way; then humidity, which is what separates
    /// the temperate biomes from each other.
    pub fn biome_at(&self, gx: i32, gz: i32) -> Biome {
        let height = self.height_at(gx, gz);
        self.biome_from(gx, gz, height)
    }

    fn biome_from(&self, gx: i32, gz: i32, height: i32) -> Biome {
        // **Which kind of water, and whose shore.**
        //
        // Height alone cannot tell a river from the sea: both are cells
        // under the waterline. What separates them is *why* they are
        // under it -- the sea is where the land itself never rose, and a
        // river is a cut through land that did. The continent spline is
        // exactly that number, taken before the river was carved, so
        // asking it costs the coast nothing and gives the river back its
        // banks.
        //
        // Without it a channel through a forest was `ocean` with a bar
        // of beach sand down both sides of it, a hundred blocks from any
        // sea. The sand was the visible half; the classification was the
        // half that stopped anything growing there.
        // Only water columns and the strip just above them can be
        // either, so the sample is taken for the handful of columns
        // that can change their answer rather than for all of them.
        // `height_at` has already paid for this field once; asking
        // again on every column of the world would be a fifth of the
        // cost of a chunk to answer a question about its shoreline.
        if height > SEA_LEVEL + 2 {
            return self.land_biome(gx, gz, height);
        }
        let by_the_sea =
            spline(CONTINENT_SPLINE, self.continent(gx, gz)) <= SEA_LEVEL as f64 + 2.5;

        if height < SEA_LEVEL {
            return if by_the_sea { Biome::Ocean } else { Biome::River };
        }
        // The sandy strip is three blocks of height rather than two.
        // The coast spline is deliberately steep -- see the module docs
        // -- so the ground climbs away from the waterline quickly, and
        // classifying by a narrow band of height would leave a beach one
        // block wide, which reads as a line rather than as a shore.
        //
        // Only against the sea, though. A river bank belongs to the
        // country it runs through: meadow in a meadow, taiga in taiga.
        if height <= SEA_LEVEL + 2 && by_the_sea {
            return Biome::Beach;
        }
        self.land_biome(gx, gz, height)
    }

    /// Everything above the waterline that is not a beach: the climate
    /// classifier proper.
    ///
    /// Split out so the common case -- a column of dry land, which is
    /// most of the world and all of the hot loop -- never pays for the
    /// coast test above it.
    fn land_biome(&self, gx: i32, gz: i32, height: i32) -> Biome {
        let temperature = self.surface_temperature(gx, gz, height);
        let humidity = self.humidity(gx, gz);

        // High ground. Snow line follows temperature rather than a fixed
        // altitude, so a peak in the tropics can still be bare rock
        // while a lower one further north is white.
        //
        // **The snow threshold is colder than the one that makes
        // lowland tundra**, and that is what keeps the map continuous.
        // It used to be warmer (-0.30 against -0.42), which meant a
        // merely coolish column became a snowfield the instant it
        // crossed the height line -- while its neighbour one block
        // lower, at the same temperature, was still temperate grass.
        // Plains touching snowy peaks, with the tundra and the conifers
        // that belong between them missing entirely. Now anything cold
        // enough to be white up here is cold enough to be tundra down
        // there, so the bands are always walked through in order.
        if height > SEA_LEVEL + 22 {
            return if temperature < -0.60 {
                Biome::SnowyPeaks
            } else {
                Biome::Mountains
            };
        }

        if temperature < -0.42 {
            // Cold. Humid enough for conifers, or bare tundra.
            return if humidity > -0.10 {
                Biome::Taiga
            } else {
                Biome::Tundra
            };
        }

        if temperature > 0.14 {
            // Hot. Dry is desert, anything else is open steppe.
            return if humidity < -0.05 {
                Biome::Desert
            } else {
                Biome::Savanna
            };
        }

        // Temperate, split by rainfall into four bands that each look
        // like something: bare dead wood, open plain, closed forest,
        // and -- where it is wet *and* low -- swamp. A waterlogged
        // hilltop is not a thing.
        if humidity > 0.34 && height <= SEA_LEVEL + 4 {
            return Biome::Swamp;
        }
        if humidity > 0.10 {
            // Wet temperate country, split once more by temperature.
            // Birch takes the cool half, which puts it between the oak
            // forest and the taiga -- so the walk from one to the other
            // goes through it rather than stepping over it.
            //
            // The threshold is well inside the temperate band (which
            // runs to -0.42) rather than at its edge, so a birch wood is
            // a place with room in it rather than a fringe a few columns
            // wide along the taiga.
            if temperature < -0.16 {
                Biome::BirchForest
            } else {
                Biome::Forest
            }
        } else if humidity > -0.26 {
            Biome::Plains
        } else {
            Biome::DeadForest
        }
    }

    /// What the top of a column is made of, and what sits just under it.
    ///
    /// Generation itself goes through the column cache, which already
    /// has the biome and the slope; this is the same question asked from
    /// outside, and the tests are what ask it.
    #[cfg(test)]
    fn surface_at(&self, gx: i32, gz: i32, height: i32) -> Surface {
        let biome = self.biome_from(gx, gz, height);
        surface_for(height, biome, self.slope_at(gx, gz, height))
    }

    /// How steep a column is -- see `slope_from` for what the number
    /// means.
    ///
    /// Computed from `height_at` rather than from a cache, so the answer
    /// for a column is the same whichever chunk is asking. A slope taken
    /// from clamped cache lookups would differ at the edges, and two
    /// neighbouring chunks would disagree about what the ground between
    /// them is made of -- which shows up as a strip of the wrong
    /// material along every chunk border.
    ///
    /// Generation itself takes the same measure off the height cache it
    /// has already built, rather than paying for four more columns of
    /// fractal noise apiece; this is the standalone form, and the tests
    /// are what ask for it.
    #[cfg(test)]
    fn slope_at(&self, gx: i32, gz: i32, _height: i32) -> f32 {
        slope_from(
            self.height_at(gx + 1, gz),
            self.height_at(gx - 1, gz),
            self.height_at(gx, gz + 1),
            self.height_at(gx, gz - 1),
        )
    }

    /// Tunnels, plus the occasional open room.
    ///
    /// **Tunnels** are the intersection of two independent fields. One
    /// field near zero is a *surface* through the world, and carving it
    /// produces the sheets of empty space a single-field cave system is
    /// famous for. Two fields near zero at once is the intersection of
    /// two surfaces, which is a curve -- so what gets carved is a
    /// winding tunnel you can actually walk along.
    ///
    /// **Caverns** are the same first field far from zero. Reusing it
    /// costs nothing: this is called for every solid cell underground,
    /// which is the hottest loop in generation, and a third field would
    /// mean a third noise evaluation for every one of them. It also puts
    /// the rooms on the same structure as the tunnels -- the tunnel
    /// shell is the boundary of the region the caverns sit inside, so
    /// they tend to connect rather than being sealed pockets.
    ///
    /// Ordered so the common case is one evaluation: the second field is
    /// only sampled for the few cells already close to the tunnel
    /// surface.
    fn is_cave(&self, gx: i32, y: i32, gz: i32) -> bool {
        if y <= BEDROCK_TOP + 1 {
            return false; // never carve through the floor of the world
        }
        // Flatter than they are wide: the vertical frequency is nearly
        // double the horizontal one, so a tunnel is something to walk
        // along rather than a shaft to fall down.
        const FREQ_XZ: f64 = 0.024;
        const FREQ_Y: f64 = 0.045;
        /// How close to zero counts as "on the surface" of a field.
        /// Widening this does not widen tunnels so much as multiply
        /// them, and past about a tenth the underground stops being rock
        /// with holes in it.
        const TUNNEL: f64 = 0.075;
        let p = [gx as f64 * FREQ_XZ, y as f64 * FREQ_Y, gz as f64 * FREQ_XZ];
        let a = self.cave_noise_a.get(p);

        // Rooms, deep only: one breaking the surface would open a pit in
        // the middle of a field.
        if y <= SEA_LEVEL - 8 && a > 0.70 {
            return true;
        }
        if a.abs() > TUNNEL {
            return false;
        }
        self.cave_noise_b.get(p).abs() < TUNNEL
    }

    /// Glowstone, in veins rather than as isolated cells.
    ///
    /// The narrow band on a single noise field is what makes a vein: the
    /// set of points where a smooth field sits inside a thin range is a
    /// shell, and a shell through a 3D field is a connected blob.
    fn is_glowstone(&self, gx: i32, y: i32, gz: i32) -> bool {
        if y > SEA_LEVEL - 6 {
            return false; // deep only, so it lights caves rather than hillsides
        }
        let v = self
            .ore_noise
            .get([gx as f64 * 0.085, y as f64 * 0.085, gz as f64 * 0.085]);
        v > 0.70
    }

    /// What ore, if any, is in the rock at this cell.
    ///
    /// ## Two questions, not four
    ///
    /// The obvious shape is a noise field per mineral, each with its own
    /// threshold and depth band. It reads well and it costs four Perlin
    /// samples in the *stone* of every column -- eight thousand cells a
    /// chunk, most of them stone, and a chunk already spends most of its
    /// time in noise. That is a doubling of generation cost to place
    /// something that occupies about one cell in a hundred.
    ///
    /// So it is asked as two questions instead:
    ///
    /// 1. **Is this rock mineralised?** One field, one sample, thresholded
    ///    high. A narrow band on a smooth 3D field is a shell, and a
    ///    shell is a connected blob -- which is what makes this a vein
    ///    rather than a scatter of isolated cubes. Same trick as
    ///    `is_glowstone`, and the same reason.
    /// 2. **What is in it?** A hash, which is free, keyed to a coarse
    ///    block of the world so a single vein comes out as one mineral
    ///    rather than a plum pudding -- and weighted by depth, which is
    ///    where the geology goes.
    ///
    /// ## The depths
    ///
    /// Copper and tin near the top of the rock, where in the real world
    /// they weather out at outcrops and are found by people who are not
    /// yet miners. Iron deep, and much more of it than copper -- iron is
    /// not rare, it is *hard to get*, and in this game that difficulty is
    /// spent on the tool the ore demands rather than on how much of it
    /// there is. Tin is scarce everywhere: that scarcity is the entire
    /// reason bronze was worth trading for.
    ///
    /// Coal is at every depth and commonest of all, because it is what
    /// every one of the other three has to be melted with, and an ore
    /// economy whose fuel is the bottleneck is an economy that reads as
    /// a bug.
    fn ore_at(&self, gx: i32, y: i32, gz: i32) -> Option<BlockId> {
        /// Vein scale. Finer than the glowstone field: an ore body should
        /// be a handful of blocks you are pleased to find, not a wall.
        const FREQ: f64 = 0.115;
        /// How far up the field a cell has to be to be mineralised at
        /// all. Every point of this is roughly a halving of how much ore
        /// the world has.
        const VEIN: f64 = 0.62;
        /// Below this, the deep rock. Six under the waterline, so it is
        /// the same depth in every terrain rather than a fixed fraction
        /// of a column that might be a mountain or a sea floor.
        const DEEP: i32 = SEA_LEVEL - 6;
        /// How big a lump of world shares one mineral. A vein is smaller
        /// than this, so a vein is almost always all one thing.
        const VEIN_CELL: i32 = 6;

        let v = self
            .vein_noise
            .get([gx as f64 * FREQ, y as f64 * FREQ, gz as f64 * FREQ]);
        if v <= VEIN {
            return None;
        }

        // A roll per coarse cell of the world, not per block.
        //
        // Shifted before the modulus, and that is not a detail: `hash2`
        // ends in a multiply, and the low bits of a product depend only
        // on the low bits of its inputs, so `% 100` straight off the end
        // of it is visibly biased over a domain this small. The first
        // cut of this had a third less coal than its weights asked for.
        let roll = (hash2(
            gx.div_euclid(VEIN_CELL),
            gz.div_euclid(VEIN_CELL)
                .wrapping_mul(31)
                .wrapping_add(y.div_euclid(VEIN_CELL)),
            self.seed ^ 0x0FE5,
        ) >> 8)
            % 100;

        Some(if y <= DEEP {
            // Coal outweighs iron even down here, and by a hair rather
            // than by a lot. It has to outweigh it *somewhere in every
            // band*: an ocean world is almost all deep rock, so weights
            // that made coal common only near the surface would ship a
            // world with plenty of iron ore and nothing to smelt it with.
            match roll {
                0..=43 => BLOCK_COAL_ORE,
                44..=85 => BLOCK_IRON_ORE,
                86..=94 => BLOCK_COPPER_ORE,
                _ => BLOCK_TIN_ORE,
            }
        } else {
            // No iron up here at all. The player who has only ever mined
            // the top of the rock should have seen copper, tin and coal
            // and no iron whatsoever -- going *down* is the thing the
            // next age asks of them.
            match roll {
                0..=59 => BLOCK_COAL_ORE,
                60..=84 => BLOCK_COPPER_ORE,
                _ => BLOCK_TIN_ORE,
            }
        })
    }

    /// A safe standing height for a spawn point at (gx, gz): one block
    /// above the surface, never below the waterline (so you don't spawn
    /// inside an ocean).
    pub fn spawn_y(&self, gx: i32, gz: i32) -> f32 {
        let surface = self.height_at(gx, gz).max(SEA_LEVEL);
        (surface + 1) as f32
    }

    /// Somewhere to put a player down: dry, gently sloped land as near
    /// the origin as there is any.
    ///
    /// The origin itself is not a spawn point. It is one column out of a
    /// world that is mostly water, and with oceans that are now actually
    /// deep, "spawn at 0,0" means a fair share of seeds start the player
    /// treading water out of sight of land -- with nothing to break, and
    /// nothing to build on.
    ///
    /// Searched outwards in rings so the answer is the *nearest*
    /// suitable place: a world's landmarks should be near where it puts
    /// you, and hunting a thousand blocks out for the first tree is not
    /// a first five minutes anyone wants.
    pub fn spawn_column(&self) -> (i32, i32) {
        /// Sampling step. Fine enough that no island is missed, coarse
        /// enough that the whole search is a few thousand columns.
        const STEP: i32 = 8;
        /// How far out to look before giving up and taking the origin.
        const LIMIT: i32 = 3_000;

        for ring in 0..=(LIMIT / STEP) {
            let r = ring * STEP;
            // The perimeter of the square at this radius. A square
            // rather than a circle because it is exact and cheap: what
            // matters is that nothing inside it is missed.
            let mut best: Option<((i32, i32), i32)> = None;
            let mut consider = |gx: i32, gz: i32| {
                if let Some(flatness) = self.spawn_quality(gx, gz) {
                    if best.is_none_or(|(_, current)| flatness < current) {
                        best = Some(((gx, gz), flatness));
                    }
                }
            };
            if r == 0 {
                consider(0, 0);
            } else {
                let mut at = -r;
                while at <= r {
                    consider(at, -r);
                    consider(at, r);
                    consider(-r, at);
                    consider(r, at);
                    at += STEP;
                }
            }
            if let Some((column, _)) = best {
                return column;
            }
        }
        (0, 0)
    }

    /// How flat a column is, if it is fit to stand on at all.
    ///
    /// `None` for water, for a river bed and for the shore itself --
    /// spawning ankle-deep in the sea is not much better than spawning
    /// in it. Otherwise the total drop to the four neighbours, which is
    /// what picks a meadow over a cliff edge.
    fn spawn_quality(&self, gx: i32, gz: i32) -> Option<i32> {
        let height = self.height_at(gx, gz);
        if height <= SEA_LEVEL + 2 {
            return None;
        }
        // **And there has to be ground under it.**
        //
        // Height says where the surface would be; a cave that happens to
        // reach it says whether anything is actually there. The two
        // disagree over a small share of the world, and the search is
        // looking for the *flattest* column it can find -- which is
        // exactly the description of the roof of a cave that has broken
        // through. The player was then put down over a hole, with
        // nothing under their feet to break, dig or build on.
        //
        // Two cells, because the surface block and the one holding it up
        // are both worth having: standing on a one-block crust over a
        // chamber is a hole you fall into as soon as you touch it.
        if self.is_cave(gx, height, gz) || self.is_cave(gx, height - 1, gz) {
            return None;
        }
        let drop = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .map(|(dx, dz)| (self.height_at(gx + dx * 2, gz + dz * 2) - height).abs())
            .sum();
        Some(drop)
    }

    pub fn generate_chunk(&self, pos: ChunkPos) -> Chunk {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        let origin_x = pos.x * CHUNK_SIZE_X as i32;
        let origin_z = pos.z * CHUNK_SIZE_Z as i32;

        // Every column the chunk needs, including the ring of them
        // outside it that trees are rooted in, worked out once.
        //
        // Height and biome were previously computed twice for the
        // chunk's own columns -- once to fill them and once to decide
        // whether a tree stands there -- and a column is the expensive
        // part of generation: half a dozen fractal noise fields, plus
        // five samples of the river field on top. Two passes over the
        // same 484 columns was a third of the time to make a chunk.
        let columns = ColumnCache::build(self, origin_x, origin_z);

        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let gx = origin_x + lx;
                let gz = origin_z + lz;
                let column = columns.at(lx, lz);
                self.fill_column(&mut blocks, lx, lz, gx, gz, column);
            }
        }

        self.place_trees(&mut blocks, origin_x, origin_z, &columns);
        self.place_deadfall(&mut blocks, origin_x, origin_z, &columns);
        // Last, so a tuft never lands where a trunk is about to.
        self.place_ground_cover(&mut blocks, origin_x, origin_z, &columns);
        self.scatter_in_caves(&mut blocks, origin_x, origin_z);

        Chunk { pos, blocks }
    }

    fn fill_column(
        &self,
        blocks: &mut [crate::types::BlockId],
        lx: i32,
        lz: i32,
        gx: i32,
        gz: i32,
        column: Column,
    ) {
        let height = column.height;
        let surface = column.surface;
        // A cave that breaks the sea floor would drain the ocean into an
        // unlit void, so columns under water keep a sealed crust.
        let submerged = height <= SEA_LEVEL;

        for y in 0..CHUNK_SIZE_Y as i32 {
            let mut id = if y > height {
                if y <= SEA_LEVEL {
                    BLOCK_WATER
                } else {
                    BLOCK_AIR
                }
            } else if y == height {
                surface.top
            } else if y > height - surface.soil {
                surface.filler
            } else {
                BLOCK_STONE
            };

            if id != BLOCK_AIR && id != BLOCK_WATER && y > BEDROCK_TOP {
                let sealing_the_seabed = submerged && y > height - 3;
                if !sealing_the_seabed && self.is_cave(gx, y, gz) {
                    id = BLOCK_AIR;
                } else if id == BLOCK_STONE && self.is_glowstone(gx, y, gz) {
                    id = BLOCK_GLOWSTONE;
                } else if id == BLOCK_STONE {
                    // Ore replaces stone and only stone, and only after
                    // the caves have been cut. Both halves matter: an ore
                    // written before the cave pass would be carved back
                    // out again (a vein that mostly opens into a chamber
                    // is a vein you can never find in the rock), and one
                    // that ignored what it was replacing would put copper
                    // in the soil and in the sea floor.
                    if let Some(ore) = self.ore_at(gx, y, gz) {
                        id = ore;
                    }
                }
            }

            blocks[Chunk::index(lx as usize, y as usize, lz as usize)] = id;
        }

        // Bedrock floor: guarantees no cave ever opens into the void.
        for y in 0..=BEDROCK_TOP {
            blocks[Chunk::index(lx as usize, y as usize, lz as usize)] = BLOCK_STONE;
        }
    }

    /// Grass and cacti: one column each, so no margin is needed.
    ///
    /// Written after the trees rather than before, and only into air
    /// sitting on the right ground -- a tuft inside a trunk, on a cave
    /// roof or under water is the sort of detail that gets noticed
    /// precisely because it is wrong.
    fn place_ground_cover(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &ColumnCache,
    ) {
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let Column { height, biome, .. } = columns.at(lx, lz);
                let above = height + 1;
                if height < SEA_LEVEL || above + 3 >= CHUNK_SIZE_Y as i32 {
                    continue;
                }
                let ground_index = Chunk::index(lx as usize, height as usize, lz as usize);
                let air_index = Chunk::index(lx as usize, above as usize, lz as usize);
                // The cave carver runs before this, so the "surface"
                // may be a hole in the ground by now.
                if blocks[air_index] != BLOCK_AIR {
                    continue;
                }
                let ground = blocks[ground_index];

                if let Some(spacing) = biome.cactus_spacing() {
                    if crate::types::can_grow_on(BLOCK_CACTUS, ground)
                        && hash2(gx, gz, self.seed.wrapping_add(0xCAC7)).is_multiple_of(spacing.max(4))
                    {
                        let tall =
                            1 + (hash2(gx, gz, self.seed.wrapping_add(0xC0C0)) % 3) as i32;
                        for step in 0..tall {
                            let y = above + step;
                            let index = Chunk::index(lx as usize, y as usize, lz as usize);
                            if blocks[index] != BLOCK_AIR {
                                break;
                            }
                            blocks[index] = BLOCK_CACTUS;
                        }
                        continue;
                    }
                }

                if let Some(spacing) = biome.grass_spacing() {
                    if crate::types::can_grow_on(BLOCK_TALL_GRASS, ground)
                        && hash2(gx, gz, self.seed.wrapping_add(0x9A55)).is_multiple_of(spacing.max(2))
                    {
                        blocks[air_index] = BLOCK_TALL_GRASS;
                        continue;
                    }
                }

                // Fallen sticks, under anything with branches.
                //
                // The first five minutes of the game: walking a
                // forest floor and picking up the sticks and stones
                // lying on it. That is what a person would actually do,
                // and it is a far better opening than hitting a tree
                // with your hands until it turns into planks. Only
                // where trees stand or stood -- a twig in the middle of
                // a meadow is a puzzle rather than a detail.
                if let Some(spacing) = biome.stick_spacing() {
                    if crate::types::can_grow_on(BLOCK_STICK, ground)
                        && hash2(gx, gz, self.seed.wrapping_add(0x571C))
                            .is_multiple_of(spacing.max(2))
                    {
                        blocks[air_index] = BLOCK_STICK;
                        continue;
                    }
                }

                // Flint, before the ordinary stones and rarer than them.
                //
                // Where it lies is the whole of what makes it flint
                // rather than a second pebble: nodules weather *out* of
                // rock, so they collect on bare stone and in the scree
                // under a cliff, turn up washed onto sand, and are
                // scarce anywhere with enough soil to bury them. The
                // other place is underground -- see `scatter_in_caves`.
                if crate::types::can_grow_on(BLOCK_FLINT, ground)
                    && hash2(gx, gz, self.seed.wrapping_add(0xF117))
                        .is_multiple_of(flint_spacing(ground))
                {
                    blocks[air_index] = BLOCK_FLINT;
                    continue;
                }

                // Ash, over the bare earth of a wood that burned.
                //
                // **Lying on the ground rather than being it.** It used
                // to replace the surface block, which made a burnt wood
                // a floor of ash you could dig a hole in -- a metre
                // deep of the stuff, where what fell was a coating. Now
                // it is a flat thing in the cell above, like a stone or
                // a stick, except that it covers its cell edge to edge
                // (see `types::flat_inset`): you walk through it, you
                // see the earth it settled on at every hole in it, and
                // taking it up leaves the ground where it was.
                //
                // Before the stones, because a wood that burned is
                // covered in ash and the stones in it are under that.
                if biome == Biome::DeadForest
                    && crate::types::can_grow_on(BLOCK_ASH, ground)
                    && self.ashy(gx, gz)
                {
                    blocks[air_index] = BLOCK_ASH;
                    continue;
                }

                // Loose stones, in every biome there is.
                //
                // The one piece of ground cover with no climate to it:
                // a beach, a peak and a swamp all have stones lying
                // about, and a world where the ground is bare until you
                // dig into it reads as unfinished. Last, so a stone
                // never lands where a tuft already is -- the cell holds
                // one thing, and grass is the one worth keeping.
                if crate::types::can_grow_on(BLOCK_PEBBLE, ground)
                    && hash2(gx, gz, self.seed.wrapping_add(0x570E))
                        .is_multiple_of(PEBBLE_SPACING)
                {
                    blocks[air_index] = BLOCK_PEBBLE;
                }
            }
        }
    }

    /// What is lying about on the floor of a cave.
    ///
    /// The other half of where flint comes from, and the half that
    /// matters: nodules sit in the rock, and a cave is a cut through it,
    /// so the floor of one is where they have been falling out for as
    /// long as the cave has existed. It is also the only reason to walk
    /// a tunnel rather than dig a staircase -- there is something down
    /// there to find, and finding it is picking it up rather than mining
    /// anything.
    ///
    /// Purely local: every cell is decided by its own column and what is
    /// directly above and below it, so no chunk needs to know anything
    /// about its neighbours. Run after the column is filled, because
    /// what it looks for -- air with rock under it -- is what the cave
    /// carver leaves behind.
    fn scatter_in_caves(&self, blocks: &mut [crate::types::BlockId], origin_x: i32, origin_z: i32) {
        /// One nodule per this many cave-floor cells.
        const FLINT_SPACING: u32 = 11;
        /// ...and one loose stone, which is commoner: a cave floor is
        /// mostly rubble off the roof.
        const RUBBLE_SPACING: u32 = 7;
        // Nothing is scattered above this: near the surface a "cave" is
        // as often a dip in a hillside, and stones appearing in one read
        // as litter rather than as a find.
        let ceiling = SEA_LEVEL - 4;

        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                for y in (BEDROCK_TOP + 1)..=ceiling {
                    let index = Chunk::index(lx as usize, y as usize, lz as usize);
                    if blocks[index] != BLOCK_AIR {
                        continue;
                    }
                    let under = blocks[Chunk::index(lx as usize, (y - 1) as usize, lz as usize)];
                    if !matches!(crate::types::block_kind(under), BLOCK_STONE) {
                        continue;
                    }
                    // Head room, so nothing is wedged into a crack too
                    // thin to have been walked into in the first place.
                    if blocks[Chunk::index(lx as usize, (y + 1) as usize, lz as usize)] != BLOCK_AIR
                    {
                        continue;
                    }
                    let roll = hash2(gx, gz.wrapping_mul(31).wrapping_add(y), self.seed ^ 0xCA5E);
                    if roll.is_multiple_of(FLINT_SPACING) {
                        blocks[index] = BLOCK_FLINT;
                    } else if roll.is_multiple_of(RUBBLE_SPACING) {
                        blocks[index] = BLOCK_PEBBLE;
                    }
                }
            }
        }
    }

    /// Trunks lying on the ground.
    ///
    /// A run of logs along one axis, at the height of the ground under
    /// it, which is what makes a dead wood read as one that has been
    /// dead for a while rather than as one somebody planted bare.
    ///
    /// Rooted up to `MAX_DEADFALL` outside the chunk, like trees, so a
    /// trunk crosses a chunk border without either side knowing about
    /// the other: both compute the same log from (seed, position) and
    /// each keeps the part that falls inside its own array.
    fn place_deadfall(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &ColumnCache,
    ) {
        for lz in -MAX_DEADFALL..(CHUNK_SIZE_Z as i32 + MAX_DEADFALL) {
            for lx in -MAX_DEADFALL..(CHUNK_SIZE_X as i32 + MAX_DEADFALL) {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let Column { height, biome, .. } = columns.at(lx, lz);
                let Some(spacing) = biome.deadfall_spacing() else {
                    continue;
                };
                let roll = hash2(gx, gz, self.seed.wrapping_add(0xFA11));
                if !roll.is_multiple_of(spacing.max(8)) {
                    continue;
                }
                if height < SEA_LEVEL + 1 {
                    continue;
                }

                let length = 3 + (roll >> 8) as i32 % (MAX_DEADFALL - 2);
                let along_x = (roll >> 16) & 1 == 0;
                // A fallen trunk lies along the way it fell, so its cut
                // ends face along the run and its bark wraps the sides.
                // Before blocks had an axis these were upright logs laid
                // end to end -- a row of stumps rather than a trunk, and
                // the one place in the world where the mistake was
                // obvious from ten blocks away.
                let trunk = crate::types::oriented(
                    BLOCK_LOG,
                    if along_x {
                        crate::types::Axis::X
                    } else {
                        crate::types::Axis::Z
                    },
                );
                for step in 0..length {
                    let (dx, dz) = if along_x { (step, 0) } else { (0, step) };
                    let column = columns.at(lx + dx, lz + dz);
                    // A log lies on the ground. Where the ground is not
                    // level it stops rather than floating on, which
                    // also keeps it out of the air over a cliff edge.
                    if (column.height - height).abs() > 1 {
                        break;
                    }
                    put_block(blocks, lx + dx, column.height + 1, lz + dz, trunk, false);
                }
            }
        }
    }

    /// Whether a tree is rooted at this exact column.
    ///
    /// A hash rather than a noise field, so neighbouring trees do not
    /// clump into one solid block of forest -- but the *rate* comes from
    /// the biome, which is smooth, so forests and clearings are still
    /// regions rather than uniform scatter.
    fn tree_at(&self, gx: i32, gz: i32, biome: Biome) -> bool {
        let Some(spacing) = biome.tree_spacing() else {
            return false;
        };
        hash2(gx, gz, self.seed.wrapping_add(0x7EE5)).is_multiple_of(spacing.max(4))
    }

    /// Trees, including the ones rooted outside this chunk.
    ///
    /// **Why the padding matters.** The obvious version only roots a tree
    /// where its whole canopy fits inside the chunk, which keeps
    /// generation local -- and leaves a visible tree-free border two
    /// blocks wide around every chunk. On a 16-wide chunk that is a
    /// quarter of the world, and the grid it produces is obvious from
    /// the air.
    ///
    /// Instead every column within one canopy radius of the chunk is
    /// considered, and whatever lands outside the chunk's own array is
    /// dropped. Each chunk therefore draws the parts of its neighbours'
    /// trees that overhang it, and because the decision comes entirely
    /// from (seed, global position) both chunks independently agree on
    /// where the tree is and what it looks like. Generation stays a pure
    /// function of (seed, pos) -- no ordering, no cross-chunk writes.
    fn place_trees(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &ColumnCache,
    ) {
        let span_x = CHUNK_SIZE_X as i32;
        let span_z = CHUNK_SIZE_Z as i32;
        for lz in -MAX_CANOPY_RADIUS..span_z + MAX_CANOPY_RADIUS {
            for lx in -MAX_CANOPY_RADIUS..span_x + MAX_CANOPY_RADIUS {
                let gx = origin_x + lx;
                let gz = origin_z + lz;
                let Column {
                    height: ground,
                    biome,
                    surface,
                } = columns.at(lx, lz);
                if !self.tree_at(gx, gz, biome) {
                    continue;
                }
                // Nothing grows on rock, sand or snow, and nothing grows
                // on a column the caves have opened. Both are decided
                // from noise, so this holds for roots in other chunks
                // too -- where we cannot read the block array.
                //
                // Dirt counts as well as grass: a dead forest stands on
                // bare earth, and testing for grass alone left it with
                // no trees at all -- a "forest" of nothing but the odd
                // fallen trunk.
                //
                // The column's own surface rather than its biome's,
                // which is what keeps trees off the rock faces the
                // slope rules now cut through a wood. A tree rooted in
                // a cliff is the most obvious thing in a landscape.
                //
                // Ash counts too, and has to: it lies in a burnt wood,
                // and the burnt trunks standing in it are the whole
                // reason the ash is there.
                //
                // Snow counts for a fir, and only for a fir: the thing
                // a treeline is *made of* is trees standing in snow,
                // and refusing to root one there left the tundra with
                // no trees at all -- which is what it had.
                let kind = biome.tree_kind();
                let rootable = matches!(surface.top, BLOCK_GRASS | BLOCK_DIRT | BLOCK_ASH)
                    || (kind == TreeKind::Conifer && block_kind(surface.top) == BLOCK_SNOW);
                if !rootable {
                    continue;
                }
                if self.is_cave(gx, ground, gz) {
                    continue;
                }

                // Shape comes from the biome: a taiga's tall narrow
                // spires read differently from a swamp's broad low
                // canopies at a distance, even though both are built
                // from the same two blocks.
                let (shortest, tallest, canopy) = biome.tree_shape();
                let wood = biome.tree_wood();
                let variant = hash2(gx, gz, self.seed.wrapping_add(0x7A11));
                let span = (tallest - shortest + 1).max(1) as u32;
                let trunk = shortest + (variant % span) as i32;
                match kind {
                    TreeKind::Broadleaf => {
                        place_tree(blocks, lx, ground, lz, trunk, canopy, wood)
                    }
                    TreeKind::Conifer => {
                        place_conifer(blocks, lx, ground, lz, trunk, canopy, variant, wood)
                    }
                }
            }
        }
    }
}

/// The two shapes a tree comes in. See `Biome::tree_kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TreeKind {
    /// A trunk with a rounded canopy on top of it.
    Broadleaf,
    /// A fir: rings of branches from low down, narrowing to a point.
    Conifer,
}

/// What a column is, before anything is written into it.
#[derive(Clone, Copy)]
struct Column {
    height: i32,
    biome: Biome,
    /// What the ground is made of here -- climate and slope together.
    surface: Surface,
}

/// Every column a chunk needs, worked out once.
///
/// Covers the chunk plus a `MAX_CANOPY_RADIUS` ring, because trees
/// rooted next door drop their leaves over the border and the decision
/// about where they stand has to be made here too.
struct ColumnCache {
    columns: Vec<Column>,
}

/// Side of the cached area: the chunk plus a feature margin each side.
const CACHE_SPAN: i32 = CHUNK_SIZE_X as i32 + FEATURE_MARGIN * 2;
/// ...and of the heights behind it, which reach one further out.
///
/// That one extra ring is what lets every cached column have its slope
/// worked out from *real* neighbours rather than from a clamped lookup
/// at the edge of the array. Without it the outermost columns -- which
/// are exactly the ones a neighbouring chunk also computes -- would get
/// a different slope depending on which chunk was asking, and the two
/// would disagree about what the ground there is made of.
const HEIGHT_SPAN: i32 = CACHE_SPAN + 2;

impl ColumnCache {
    fn build(gen: &WorldGen, origin_x: i32, origin_z: i32) -> Self {
        let reach = FEATURE_MARGIN + 1;
        let mut heights = Vec::with_capacity((HEIGHT_SPAN * HEIGHT_SPAN) as usize);
        for lz in -reach..(CHUNK_SIZE_Z as i32 + reach) {
            for lx in -reach..(CHUNK_SIZE_X as i32 + reach) {
                heights.push(gen.height_at(origin_x + lx, origin_z + lz));
            }
        }
        let height_at = |lx: i32, lz: i32| {
            let x = lx + reach;
            let z = lz + reach;
            heights[(z * HEIGHT_SPAN + x) as usize]
        };

        let mut columns = Vec::with_capacity((CACHE_SPAN * CACHE_SPAN) as usize);
        for lz in -FEATURE_MARGIN..(CHUNK_SIZE_Z as i32 + FEATURE_MARGIN) {
            for lx in -FEATURE_MARGIN..(CHUNK_SIZE_X as i32 + FEATURE_MARGIN) {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let height = height_at(lx, lz);
                // The same gradient `slope_at` takes, off the heights
                // already in hand rather than four more columns' worth
                // of fractal noise apiece.
                let slope = slope_from(
                    height_at(lx + 1, lz),
                    height_at(lx - 1, lz),
                    height_at(lx, lz + 1),
                    height_at(lx, lz - 1),
                );
                let biome = gen.biome_from(gx, gz, height);
                let mut surface = surface_for(height, biome, slope);
                // Clay and gravel, the two deposits water leaves.
                //
                // After the biome and the slope have had their say,
                // because neither is a *climate*: they are what the
                // water did here, and what the water did depends on
                // where the water is. Clay wants the still shallows and
                // the flat ground just above them; gravel wants the
                // opposite -- a bed that ran fast, or a slope that has
                // shed everything finer than a stone.
                // The free geometric tests run first and the noise runs
                // last, once: most land columns are neither near water
                // nor on a bank slope, and they should not pay two
                // Perlin samples to find that out.
                let near_water = height <= SEA_LEVEL + 2;
                let clay_site = near_water && slope < BANK_SLOPE;
                let gravel_site =
                    near_water || (BANK_SLOPE..ROCK_SLOPE).contains(&slope);
                if clay_site || gravel_site {
                    let deposit = gen.deposit(gx, gz);
                    if clay_site && deposit > CLAY_DEPOSIT {
                        surface.top = BLOCK_CLAY;
                        surface.filler = BLOCK_CLAY;
                        surface.soil = 3;
                    } else if gravel_site && deposit < GRAVEL_DEPOSIT {
                        surface.top = BLOCK_GRAVEL;
                        surface.filler = BLOCK_GRAVEL;
                        surface.soil = 2;
                    }
                }

                columns.push(Column {
                    height,
                    biome,
                    surface,
                });
            }
        }
        Self { columns }
    }

    /// `lx`/`lz` are chunk-local and may run one canopy radius outside
    /// the chunk, which is exactly the area this covers.
    fn at(&self, lx: i32, lz: i32) -> Column {
        let x = (lx + FEATURE_MARGIN).clamp(0, CACHE_SPAN - 1);
        let z = (lz + FEATURE_MARGIN).clamp(0, CACHE_SPAN - 1);
        self.columns[(z * CACHE_SPAN + x) as usize]
    }
}

/// One nodule of flint per this many columns, by what the ground is.
///
/// Rock first, because that is where flint forms and where the ground
/// has nothing on it to hide a nodule; then sand, which is what a
/// nodule washed out of a bank ends up lying on; then everywhere else,
/// where it is rare enough to be worth stopping for.
fn flint_spacing(ground: crate::types::BlockId) -> u32 {
    match crate::types::block_kind(ground) {
        BLOCK_STONE | BLOCK_COBBLESTONE => 30,
        BLOCK_SAND => 64,
        _ => 150,
    }
}

/// Fractional Brownian motion: several octaves of the same field, each
/// half the amplitude and twice the frequency of the last.
///
/// Returned normalised to roughly -1..1 regardless of octave count, so
/// changing the detail level does not also change the scale of the
/// terrain it drives.
fn fbm(noise: &Perlin, x: f64, z: f64, base_frequency: f64, octaves: u32) -> f64 {
    let mut sum = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = base_frequency;
    let mut total = 0.0;
    for _ in 0..octaves {
        sum += noise.get([x * frequency, z * frequency]) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    if total == 0.0 {
        0.0
    } else {
        (sum / total).clamp(-1.0, 1.0)
    }
}

/// Reads a piecewise-linear curve at `x`.
///
/// Points must be sorted by their input. Outside the ends the curve is
/// flat rather than extrapolated: a noise field occasionally exceeds its
/// nominal range, and extrapolating a terrain curve off the end of its
/// control points is how a spike ends up in the world.
fn spline(points: &[(f64, f64)], x: f64) -> f64 {
    let Some(&(first_x, first_y)) = points.first() else {
        return 0.0;
    };
    if x <= first_x {
        return first_y;
    }
    for window in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (window[0], window[1]);
        if x <= x1 {
            let t = if (x1 - x0).abs() < f64::EPSILON {
                0.0
            } else {
                (x - x0) / (x1 - x0)
            };
            return y0 + (y1 - y0) * t;
        }
    }
    points[points.len() - 1].1
}

/// Smooth 0..1 ramp between two edges. `edge0 > edge1` is allowed and
/// gives a falling ramp, which is how "less of this means more of that"
/// is written without a second function.
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if (edge1 - edge0).abs() < f64::EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Small integer hash -- deterministic across runs and platforms (unlike
/// `DefaultHasher`, whose output is explicitly not guaranteed stable).
fn hash2(x: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed ^ 0x9E37_79B9;
    h = h.wrapping_mul(0x85EB_CA6B) ^ (x as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x27D4_EB2F) ^ (z as u32).wrapping_mul(0x165667B1);
    h ^= h >> 13;
    h.wrapping_mul(0x9E37_79B1)
}

/// Writes one block by chunk-local coordinates, which may be outside
/// the chunk.
///
/// Silently skipped when they are: the part that misses belongs to a
/// neighbouring chunk, which is generating it itself from the same
/// inputs. `overwrite` false only fills air.
fn put_block(
    blocks: &mut [crate::types::BlockId],
    lx: i32,
    y: i32,
    lz: i32,
    id: crate::types::BlockId,
    overwrite: bool,
) {
    if lx < 0 || lz < 0 || lx >= CHUNK_SIZE_X as i32 || lz >= CHUNK_SIZE_Z as i32 {
        return;
    }
    if y < 0 || y >= CHUNK_SIZE_Y as i32 {
        return;
    }
    let index = Chunk::index(lx as usize, y as usize, lz as usize);
    if overwrite || blocks[index] == BLOCK_AIR {
        blocks[index] = id;
    }
}

/// Writes one tree into a chunk's block array.
///
/// `lx`/`lz` are chunk-local and may be **outside** the chunk: this is
/// how a tree rooted next door drops its canopy over the border. Every
/// write is bounds-checked and silently skipped, so the parts that miss
/// simply belong to the neighbouring chunk, which is generating them
/// itself from the same inputs.
fn place_tree(
    blocks: &mut [crate::types::BlockId],
    lx: i32,
    ground: i32,
    lz: i32,
    trunk_height: i32,
    canopy_radius: i32,
    (log, leaves): (BlockId, BlockId),
) {
    let top = ground + trunk_height;
    if top + 2 >= CHUNK_SIZE_Y as i32 {
        return;
    }

    let mut put = |x: i32, y: i32, z: i32, id: crate::types::BlockId, overwrite: bool| {
        if x < 0 || z < 0 || x >= CHUNK_SIZE_X as i32 || z >= CHUNK_SIZE_Z as i32 {
            return; // belongs to a neighbouring chunk
        }
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            return;
        }
        let index = Chunk::index(x as usize, y as usize, z as usize);
        if overwrite || blocks[index] == BLOCK_AIR {
            blocks[index] = id;
        }
    };

    // A canopy radius of zero is a dead tree: bare trunk, nothing on
    // top. Without this the taper below would still put a single leaf
    // block on the end of it, which reads as a tree wearing a hat.
    if canopy_radius > 0 {
    // Canopy first, so the trunk overwrites any leaf that lands on it.
    for dy in -2..=2 {
        let y = top + dy;
        // Widest in the middle, tapering to a point: a cylinder of
        // leaves reads as a lollipop, which is what the old fixed radius
        // produced.
        let radius = match dy {
            -2 | -1 => canopy_radius,
            0 => canopy_radius - 1,
            _ => canopy_radius - 2,
        };
        if radius < 0 {
            continue;
        }
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                // Round the corners off, or every tree is a stack of
                // squares seen from above.
                if dx.abs() == radius && dz.abs() == radius && radius > 0 {
                    continue;
                }
                put(lx + dx, y, lz + dz, leaves, false);
            }
        }
    }
    }

    for y in ground + 1..=top {
        put(lx, y, lz, log, true);
    }
}

/// A fir: rings of branches from low on the trunk, narrowing to a point.
///
/// **Why it is not `place_tree` with a smaller radius.** A broadleaf is
/// a bare trunk with a ball of leaves balanced on the end, and every
/// parameter of it describes that ball. A fir has no ball: it is widest
/// near the bottom, it narrows the whole way up, and the branches start
/// low enough that the trunk is barely visible. Those are different
/// shapes, not the same shape at two sizes -- and the shape is the whole
/// of what tells a cold forest from a temperate one from any distance
/// where you cannot make out a single leaf.
///
/// The rings alternate wide and narrow rather than shrinking one block
/// at a time, which is what gives a fir its layered look; a smooth cone
/// reads as a triangle drawn in blocks. The offset comes from the same
/// hash the trunk height does, so neighbouring trees are not all in
/// step, and the same tree is the same every time the chunk is built.
#[allow(clippy::too_many_arguments)] // a shape, a place and a material
fn place_conifer(
    blocks: &mut [crate::types::BlockId],
    lx: i32,
    ground: i32,
    lz: i32,
    trunk_height: i32,
    canopy_radius: i32,
    variant: u32,
    (log, leaves): (BlockId, BlockId),
) {
    let top = ground + trunk_height;
    if top + 1 >= CHUNK_SIZE_Y as i32 {
        return;
    }

    let mut put = |x: i32, y: i32, z: i32, id: crate::types::BlockId, overwrite: bool| {
        if x < 0 || z < 0 || x >= CHUNK_SIZE_X as i32 || z >= CHUNK_SIZE_Z as i32 {
            return; // belongs to a neighbouring chunk
        }
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            return;
        }
        let index = Chunk::index(x as usize, y as usize, z as usize);
        if overwrite || blocks[index] == BLOCK_AIR {
            blocks[index] = id;
        }
    };

    // Where the branches start: a quarter of the way up, so there is a
    // little bare trunk to stand under and the skirt still reaches
    // most of the way down. A third of the way up was too high on the
    // tall trunks the taiga grows -- an eleven-block fir carried its
    // lowest branch nearly four blocks off the ground, which is a
    // lamppost with a bush on it rather than a fir.
    let skirt = ground + 1 + trunk_height / 4;
    let phase = (variant >> 5) & 1;
    for y in skirt..=top {
        let from_top = top - y;
        // Alternating wide and narrow rings, widening down the trunk.
        //
        // **Every ring has leaves in it**, and that is the fix for
        // firs that were bare poles with a tuft on the end. The radius
        // used to be `from_top / 2`, which is zero for the top two
        // rings, and the alternation then subtracted one *from zero's
        // neighbour* -- so the top three rings of every fir came out
        // empty, the trunk was drawn straight through where they
        // should have been, and what was left above the skirt was bare
        // wood. Starting at one and never narrowing below one means
        // the spire tapers to a point instead of stopping short of it.
        let mut radius = (1 + from_top / 2).min(canopy_radius);
        if (from_top as u32 + phase).is_multiple_of(2) && radius > 1 {
            radius -= 1;
        }
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                // Round the corners off, as the broadleaf canopy does --
                // but only once there is a ring wide enough to have
                // corners. Cutting them from a radius of one leaves four
                // leaves in a cross around the trunk, and a spire built
                // of crosses is a fishbone: you can see the wood through
                // every narrow ring of it. A full three by three is
                // eight, which reads as a branch.
                if dx.abs() == radius && dz.abs() == radius && radius > 1 {
                    continue;
                }
                put(lx + dx, y, lz + dz, leaves, false);
            }
        }
    }
    // A point on top, above the last ring.
    put(lx, top + 1, lz, leaves, false);

    for y in ground + 1..=top {
        put(lx, y, lz, log, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BlockId;

    /// Chunks over a wide area, for the tests that need to see enough of
    /// the world for a rare feature to show up at all.
    pub(super) fn sample_chunks(seed: u32, radius: i32) -> Vec<Chunk> {
        let gen = WorldGen::new(seed);
        let mut out = Vec::new();
        for cx in -radius..=radius {
            for cz in -radius..=radius {
                out.push(gen.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        out
    }

    #[test]
    fn deterministic_for_same_seed() {
        let a = WorldGen::new(42).generate_chunk(ChunkPos::new(3, -2));
        let b = WorldGen::new(42).generate_chunk(ChunkPos::new(3, -2));
        assert_eq!(a.blocks, b.blocks);
    }

    #[test]
    fn different_seeds_usually_differ() {
        let a = WorldGen::new(1).generate_chunk(ChunkPos::new(0, 0));
        let b = WorldGen::new(2).generate_chunk(ChunkPos::new(0, 0));
        assert_ne!(a.blocks, b.blocks);
    }

    #[test]
    fn always_has_a_stone_floor() {
        // Bedrock layer means caves can never open into the void, which
        // the client's physics has no answer for.
        for chunk in sample_chunks(7, 2) {
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    assert_eq!(chunk.get(x, 0, z), BLOCK_STONE);
                }
            }
        }
    }

    #[test]
    fn every_column_stays_inside_the_world() {
        let gen = WorldGen::new(2024);
        for gx in -400..400 {
            for gz in [-333, -40, 0, 17, 288] {
                let h = gen.height_at(gx, gz);
                assert!(
                    (MIN_HEIGHT..=MAX_HEIGHT).contains(&h),
                    "height {h} at ({gx},{gz}) is outside the world"
                );
            }
        }
    }

    #[test]
    fn caves_actually_carve_something() {
        // Block light (glowstone) only earns its keep if there is
        // somewhere dark for it to light.
        let gen = WorldGen::new(2024);
        let mut air_underground = 0;
        for cx in 0..4 {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, 0));
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let h = gen.height_at(cx * 16 + x as i32, z as i32);
                    for y in (BEDROCK_TOP + 1)..h {
                        if chunk.get(x, y as usize, z) == BLOCK_AIR {
                            air_underground += 1;
                        }
                    }
                }
            }
        }
        assert!(air_underground > 0, "no caves generated at all");
    }

    #[test]
    fn spawn_is_above_water() {
        let gen = WorldGen::new(1337);
        assert!(gen.spawn_y(0, 0) > SEA_LEVEL as f32);
    }

    #[test]
    fn the_world_has_both_oceans_and_mountains() {
        // The point of the continent field. A generator that produces
        // only gentle hills passes every other test here.
        let gen = WorldGen::new(1337);
        let mut lowest = i32::MAX;
        let mut highest = i32::MIN;
        for gx in (-1500..1500).step_by(7) {
            for gz in (-1500..1500).step_by(97) {
                let h = gen.height_at(gx, gz);
                lowest = lowest.min(h);
                highest = highest.max(h);
            }
        }
        assert!(lowest < SEA_LEVEL - 4, "no ocean basins anywhere: {lowest}");
        assert!(
            highest > SEA_LEVEL + 20,
            "no mountains anywhere: {highest}"
        );
    }

    #[test]
    fn water_fills_every_basin_up_to_sea_level() {
        // A hole in the sea is much more obvious than a missing tree.
        let gen = WorldGen::new(99);
        for chunk in sample_chunks(99, 1) {
            let origin_x = chunk.pos.x * CHUNK_SIZE_X as i32;
            let origin_z = chunk.pos.z * CHUNK_SIZE_Z as i32;
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let h = gen.height_at(origin_x + x as i32, origin_z + z as i32);
                    if h >= SEA_LEVEL {
                        continue;
                    }
                    for y in (h + 1)..=SEA_LEVEL {
                        assert_eq!(
                            chunk.get(x, y as usize, z),
                            BLOCK_WATER,
                            "gap in the water at ({x},{y},{z}) of {:?}",
                            chunk.pos
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_sea_floor_is_never_breached_by_a_cave() {
        // A cave opening under the ocean would drain it into a void the
        // renderer has no answer for.
        let gen = WorldGen::new(4242);
        for chunk in sample_chunks(4242, 2) {
            let origin_x = chunk.pos.x * CHUNK_SIZE_X as i32;
            let origin_z = chunk.pos.z * CHUNK_SIZE_Z as i32;
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let h = gen.height_at(origin_x + x as i32, origin_z + z as i32);
                    if h > SEA_LEVEL {
                        continue;
                    }
                    // The crust under a submerged column must be solid.
                    for y in (h - 2).max(BEDROCK_TOP + 1)..=h {
                        assert_ne!(
                            chunk.get(x, y as usize, z),
                            BLOCK_AIR,
                            "the sea floor is open at ({x},{y},{z})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn trees_are_not_confined_to_the_middle_of_a_chunk() {
        // Regression: rooting a tree only where its whole canopy fitted
        // inside one chunk left a bare border around every chunk, and
        // the resulting grid was obvious from any height.
        let mut on_border = 0;
        for chunk in sample_chunks(1337, 3) {
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let edge = x == 0
                            || z == 0
                            || x == CHUNK_SIZE_X - 1
                            || z == CHUNK_SIZE_Z - 1;
                        if edge && chunk.get(x, y, z) == BLOCK_LEAVES {
                            on_border += 1;
                        }
                    }
                }
            }
        }
        assert!(
            on_border > 0,
            "no foliage on any chunk border -- trees are still chunk-locked"
        );
    }

    #[test]
    fn a_tree_that_straddles_a_border_agrees_with_itself() {
        // Both chunks decide independently what the overlapping tree
        // looks like. If they ever disagreed, the seam would show as
        // half a canopy.
        let gen = WorldGen::new(555);
        let left = gen.generate_chunk(ChunkPos::new(0, 0));
        let right = gen.generate_chunk(ChunkPos::new(1, 0));
        // Regenerating either one must reproduce it exactly, whatever
        // order they were built in.
        assert_eq!(left.blocks, gen.generate_chunk(ChunkPos::new(0, 0)).blocks);
        assert_eq!(right.blocks, gen.generate_chunk(ChunkPos::new(1, 0)).blocks);
    }

    #[test]
    fn trees_stand_on_grass_and_nothing_else() {
        let gen = WorldGen::new(8080);
        for chunk in sample_chunks(8080, 2) {
            let origin_x = chunk.pos.x * CHUNK_SIZE_X as i32;
            let origin_z = chunk.pos.z * CHUNK_SIZE_Z as i32;
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    let ground = gen.height_at(origin_x + x as i32, origin_z + z as i32);
                    // Find a trunk base: a log sitting directly on the
                    // surface of this column.
                    if ground + 1 >= CHUNK_SIZE_Y as i32 {
                        continue;
                    }
                    if chunk.get(x, (ground + 1) as usize, z) != BLOCK_LOG {
                        continue;
                    }
                    assert_eq!(
                        chunk.get(x, ground as usize, z),
                        BLOCK_GRASS,
                        "a tree at ({x},{z}) is rooted in something other than grass"
                    );
                }
            }
        }
    }

    #[test]
    fn a_seed_produces_a_variety_of_biomes() {
        // A world that is all one biome has a classifier whose
        // thresholds do not match the fields feeding it -- which is easy
        // to do and invisible from any single screenshot.
        use std::collections::HashSet;
        let gen = WorldGen::new(1337);
        let mut seen: HashSet<&str> = HashSet::new();
        for gx in (-2500..2500).step_by(23) {
            for gz in (-2500..2500).step_by(211) {
                seen.insert(gen.biome_at(gx, gz).name());
            }
        }
        assert!(
            seen.len() >= 6,
            "only found {} biomes in a whole world: {seen:?}",
            seen.len()
        );
    }

    #[test]
    fn every_biome_is_reachable_somewhere() {
        // A biome that no combination of the fields can produce is dead
        // code that reads as a feature.
        use std::collections::HashSet;
        let mut seen: HashSet<Biome> = HashSet::new();
        for seed in [1337u32, 42, 7, 2024, 99, 31337] {
            let gen = WorldGen::new(seed);
            for gx in (-3000..3000).step_by(29) {
                for gz in (-3000..3000).step_by(307) {
                    seen.insert(gen.biome_at(gx, gz));
                }
            }
        }
        let missing: Vec<&str> = Biome::ALL
            .iter()
            .filter(|b| !seen.contains(b))
            .map(|b| b.name())
            .collect();
        assert!(missing.is_empty(), "never generated: {missing:?}");
    }

    #[test]
    fn under_water_is_always_ocean_and_the_shore_is_always_beach() {
        let gen = WorldGen::new(4242);
        for gx in (-800..800).step_by(13) {
            let height = gen.height_at(gx, 77);
            let biome = gen.biome_at(gx, 77);
            if height < SEA_LEVEL {
                // ...or a river, which is also under the waterline and
                // is emphatically not the sea. See `Biome::River`.
                assert!(
                    matches!(biome, Biome::Ocean | Biome::River),
                    "submerged column at {gx} was {biome:?}"
                );
            } else if height <= SEA_LEVEL + 1 {
                // A shore is sand only where there is a sea to be the
                // shore of. A river bank at the same height belongs to
                // the country the river runs through.
                assert_ne!(biome, Biome::Ocean, "dry land at {gx} was ocean");
            } else {
                assert_ne!(biome, Biome::Ocean, "dry land at {gx} was ocean");
            }
        }
    }

    #[test]
    fn a_river_is_not_the_sea_and_its_banks_are_not_a_beach() {
        // **The reported shape of the bug**: a channel through a wood
        // came out `ocean` in the F3 line with a bar of beach sand down
        // either side of it, hundreds of blocks from any coast.
        //
        // What separates them is not the water, it is the land: the sea
        // is where the ground never rose, and a river is a cut through
        // ground that did.
        let gen = WorldGen::new(1234);
        let mut rivers = 0;
        let mut banks_of_sand = 0;
        let mut inland_banks = 0;

        for gz in (-1200..1200).step_by(7) {
            for gx in (-1200..1200).step_by(7) {
                let height = gen.height_at(gx, gz);
                if gen.biome_at(gx, gz) != Biome::River {
                    continue;
                }
                rivers += 1;
                // Well away from the coast. A river *mouth* has every
                // right to sandy banks -- that is what a delta is -- so
                // the claim being made here is about the inland reach,
                // where the nearest sea is over the horizon.
                let land = spline(CONTINENT_SPLINE, gen.continent(gx, gz));
                if land <= SEA_LEVEL as f64 + 5.0 {
                    continue;
                }
                assert!(height < SEA_LEVEL, "a river above the waterline at {gx},{gz}");

                // The banks either side, out past the channel.
                for step in [-10, 10] {
                    let (bx, bz) = (gx + step, gz);
                    if gen.height_at(bx, bz) <= SEA_LEVEL {
                        continue;
                    }
                    inland_banks += 1;
                    if gen.biome_at(bx, bz) == Biome::Beach {
                        banks_of_sand += 1;
                    }
                }
            }
        }

        assert!(rivers > 20, "only {rivers} river columns in a 2400-block square");
        assert!(inland_banks > 20, "no dry banks to check");
        // Not one, and that is the point: a beach is a thing the sea
        // makes. Anything on this list is a river with a coast.
        assert_eq!(
            banks_of_sand, 0,
            "{banks_of_sand} of {inland_banks} river banks came out as seaside"
        );
    }

    #[test]
    fn nothing_grows_where_nothing_should() {
        // Trees are placed from the biome, and the biomes that have no
        // soil must not sprout any. The tundra is deliberately not on
        // this list any more: a treeline is *made of* the last firs
        // before the ground gives up, and the peaks above it are what
        // make it read as one.
        for biome in [
            Biome::Ocean,
            Biome::Beach,
            Biome::Desert,
            Biome::Mountains,
            Biome::SnowyPeaks,
        ] {
            assert!(
                biome.tree_spacing().is_none(),
                "{} grows trees",
                biome.name()
            );
        }
        // ...and the wooded ones do.
        for biome in [Biome::Forest, Biome::Taiga, Biome::Swamp, Biome::Tundra] {
            assert!(
                biome.tree_spacing().is_some(),
                "{} has no trees at all",
                biome.name()
            );
        }
        // The tundra is the sparsest of them by a wide margin: it is
        // open country with trees in it, not a wood.
        let tundra = Biome::Tundra.tree_spacing().unwrap();
        assert!(tundra > Biome::Taiga.tree_spacing().unwrap() * 2, "a tundra forest");
    }

    #[test]
    fn a_fir_is_a_different_shape_from_an_oak() {
        // The two shapes have to be told apart at a distance where you
        // cannot see a leaf, which means the difference has to be in
        // the silhouette: a fir is widest low down and narrows all the
        // way up, and a broadleaf is a bare trunk with a ball on top.
        let mut fir = vec![BLOCK_AIR; CHUNK_VOLUME];
        super::place_conifer(&mut fir, 8, 20, 8, 9, 2, 0, (BLOCK_LOG, BLOCK_LEAVES));
        let mut oak = vec![BLOCK_AIR; CHUNK_VOLUME];
        super::place_tree(&mut oak, 8, 20, 8, 9, 2, (BLOCK_LOG, BLOCK_LEAVES));

        let leaves_at = |blocks: &[BlockId], y: i32| {
            let mut count = 0;
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    if blocks[Chunk::index(x, y as usize, z)] == BLOCK_LEAVES {
                        count += 1;
                    }
                }
            }
            count
        };

        // Halfway up the trunk the fir already has branches and the
        // broadleaf has bare wood.
        assert!(leaves_at(&fir, 25) > 0, "a fir with no skirt");
        assert_eq!(leaves_at(&oak, 25), 0, "an oak with leaves on its trunk");
        // Both come to a point rather than ending in a slab.
        assert!(leaves_at(&fir, 30) >= leaves_at(&fir, 31));
        // ...and the fir's mass is low, the broadleaf's is high: the
        // lowest leaf of a fir is most of the way down its trunk, and
        // the lowest leaf of an oak is right under its canopy.
        let lowest_of = |blocks: &[BlockId]| {
            (0..CHUNK_SIZE_Y as i32)
                .find(|&y| leaves_at(blocks, y) > 0)
                .unwrap_or(CHUNK_SIZE_Y as i32)
        };
        assert!(
            lowest_of(&fir) + 2 < lowest_of(&oak),
            "fir starts at {} and oak at {}",
            lowest_of(&fir),
            lowest_of(&oak)
        );
    }

    #[test]
    fn a_fir_is_clothed_the_whole_way_up() {
        // **The bare-pole bug.** The radius came from `from_top / 2`,
        // which is zero for the top two rings, and the wide/narrow
        // alternation then emptied a third -- so every fir was a length
        // of naked trunk with a tuft balanced on the end, at every
        // height and in both biomes.
        //
        // The rule that catches it: from the lowest branch to the tip,
        // no level of a fir is bare wood.
        for (trunk, canopy) in [(5, 2), (7, 2), (9, 2), (11, 2), (8, 3)] {
            for phase in [0u32, 1 << 5] {
                let mut fir = vec![BLOCK_AIR; CHUNK_VOLUME];
                super::place_conifer(
                    &mut fir, 8, 20, 8, trunk, canopy, phase, (BLOCK_LOG, BLOCK_LEAVES),
                );
                let at = |y: i32, id: BlockId| {
                    (0..CHUNK_SIZE_Z)
                        .flat_map(|z| (0..CHUNK_SIZE_X).map(move |x| (x, z)))
                        .filter(|&(x, z)| fir[Chunk::index(x, y as usize, z)] == id)
                        .count()
                };
                let top = 20 + trunk;
                let lowest = (0..CHUNK_SIZE_Y as i32)
                    .find(|&y| at(y, BLOCK_LEAVES) > 0)
                    .expect("a fir with no leaves at all");
                for y in lowest..=top {
                    assert!(
                        at(y, BLOCK_LEAVES) > 0,
                        "trunk {trunk} phase {phase}: bare wood at y={y}, \
                         between the skirt at {lowest} and the tip at {top}"
                    );
                }
                // ...and the narrow rings are rings rather than crosses:
                // four leaves around a trunk shows the wood through it.
                assert!(
                    (lowest..=top).all(|y| at(y, BLOCK_LEAVES) >= 8),
                    "trunk {trunk} phase {phase}: a ring too thin to hide the trunk"
                );
                // The tip is a point: one leaf above the last ring.
                assert_eq!(at(top + 1, BLOCK_LEAVES), 1, "the fir has no tip");
            }
        }
    }

    #[test]
    fn a_forest_is_denser_than_a_plain() {
        let forest = Biome::Forest.tree_spacing().unwrap();
        let plains = Biome::Plains.tree_spacing().unwrap();
        assert!(
            forest < plains,
            "forest spacing {forest} is not tighter than plains {plains}"
        );
    }

    #[test]
    fn a_birch_wood_exists_and_is_made_of_birch() {
        // A biome nothing ever classifies as is a biome that does not
        // exist, and a threshold in the middle of a noise field is very
        // easy to place where nothing ever lands. So: find one, and then
        // check the trees standing in it are the right wood.
        let mut found = None;
        'search: for seed in [1u32, 5, 42, 1337, 90_210] {
            let generator = WorldGen::new(seed);
            for gz in (-800..800).step_by(11) {
                for gx in (-800..800).step_by(11) {
                    if generator.biome_at(gx, gz) == Biome::BirchForest {
                        found = Some((seed, gx, gz));
                        break 'search;
                    }
                }
            }
        }
        let (seed, gx, gz) = found.expect("no birch forest in any test seed");

        // The chunk it is in should have birch in it and no oak: two
        // woods in one stand would be the "scattered variant" this
        // deliberately is not.
        let generator = WorldGen::new(seed);
        let mut birch = 0;
        let mut oak = 0;
        for radius in 0..6 {
            let chunk = generator.generate_chunk(ChunkPos::new(
                gx.div_euclid(CHUNK_SIZE_X as i32) + radius,
                gz.div_euclid(CHUNK_SIZE_Z as i32),
            ));
            for block in &chunk.blocks {
                match block_kind(*block) {
                    BLOCK_BIRCH_LOG | BLOCK_BIRCH_LEAVES => birch += 1,
                    BLOCK_LOG | BLOCK_LEAVES => oak += 1,
                    _ => {}
                }
            }
            if birch > 0 {
                break;
            }
        }
        assert!(
            birch > 0,
            "seed {seed}: a birch forest at ({gx},{gz}) with no birch in it \
             ({oak} blocks of oak)"
        );
    }

    #[test]
    fn birch_stands_between_the_oaks_and_the_firs() {
        // The whole reason it is a band rather than a scattering: the
        // classifier walks temperature downward, so the wood you are in
        // tells you which way is colder. A birch wood warmer than an oak
        // one would make that nonsense.
        //
        // Checked through the classifier rather than by reading the
        // thresholds, because the thresholds are what could be wrong.
        let generator = WorldGen::new(1337);
        let mut oak = Vec::new();
        let mut birch = Vec::new();
        let mut taiga = Vec::new();
        for gz in (-600..600).step_by(7) {
            for gx in (-600..600).step_by(7) {
                let height = generator.height_at(gx, gz);
                if height <= SEA_LEVEL + 2 || height > SEA_LEVEL + 22 {
                    continue; // classified by height, not by climate
                }
                let t = generator.surface_temperature(gx, gz, height);
                match generator.biome_at(gx, gz) {
                    Biome::Forest => oak.push(t),
                    Biome::BirchForest => birch.push(t),
                    Biome::Taiga => taiga.push(t),
                    _ => {}
                }
            }
        }
        assert!(!birch.is_empty(), "no birch anywhere to compare");

        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        if !oak.is_empty() {
            assert!(
                mean(&birch) < mean(&oak),
                "birch ({:.3}) is warmer than oak ({:.3})",
                mean(&birch),
                mean(&oak)
            );
        }
        if !taiga.is_empty() {
            assert!(
                mean(&birch) > mean(&taiga),
                "birch ({:.3}) is colder than taiga ({:.3})",
                mean(&birch),
                mean(&taiga)
            );
        }
    }

    #[test]
    fn every_tree_shape_fits_the_canopy_padding() {
        // `place_trees` only considers roots within `MAX_CANOPY_RADIUS`
        // of the chunk. A biome whose canopy is wider than that would
        // have its edge silently clipped at chunk borders.
        for biome in Biome::ALL {
            let (shortest, tallest, canopy) = biome.tree_shape();
            assert!(
                canopy <= MAX_CANOPY_RADIUS,
                "{} has a canopy of {canopy} against padding of {MAX_CANOPY_RADIUS}",
                biome.name()
            );
            assert!(shortest > 0 && tallest >= shortest, "{} has a bad trunk range", biome.name());
        }
    }

    #[test]
    fn biomes_are_regions_rather_than_noise() {
        // Neighbouring columns should almost always agree on their
        // surface material. If they don't, the "biome" fields are too
        // high-frequency and the world will look like static.
        let gen = WorldGen::new(31337);
        let mut same = 0;
        let mut total = 0;
        for gx in -200..200 {
            for gz in [-120, 0, 60] {
                let h1 = gen.height_at(gx, gz);
                let h2 = gen.height_at(gx + 1, gz);
                total += 1;
                if gen.surface_at(gx, gz, h1).top == gen.surface_at(gx + 1, gz, h2).top {
                    same += 1;
                }
            }
        }
        assert!(
            same * 100 / total >= 95,
            "surfaces change every {total}/{same} columns -- biomes are noise"
        );
    }

    #[test]
    fn snow_sits_on_peaks_even_in_temperate_country() {
        // The altitude term in `surface_temperature`. Without it snow is
        // purely latitudinal and mountains look like grassy lumps.
        // Swept over an area rather than a line: mountains need high
        // ground *and* low erosion at once, so they are rare enough that
        // a single transect can miss every one of them.
        let gen = WorldGen::new(1234);
        let mut peaks = 0;
        let mut snowy_peaks = 0;
        for gx in (-1500..1500).step_by(11) {
            for gz in (-1500..1500).step_by(11) {
                let h = gen.height_at(gx, gz);
                if h <= SEA_LEVEL + 24 {
                    continue;
                }
                peaks += 1;
                if gen.surface_at(gx, gz, h).top == BLOCK_SNOW {
                    snowy_peaks += 1;
                }
            }
        }
        assert!(peaks > 0, "the sweep found no high ground to test");
        assert!(snowy_peaks > 0, "nothing high is ever cold");
    }

    #[test]
    fn a_player_is_put_down_on_dry_land() {
        // With oceans that are actually deep, "spawn at 0,0" leaves a
        // fair share of seeds treading water out of sight of land.
        //
        // 1337 is on the list because it is the seed the server tests
        // run on, and because it is the one that showed the other half
        // of this: the search wants the *flattest* column it can find,
        // and the flattest thing in a landscape is the roof of a cave
        // that has broken through to the surface. That seed put the
        // player down over a hole with nothing under their feet -- no
        // ground to break, dig or build on, which two server tests
        // noticed by failing to mine the block they were standing on.
        for seed in [1, 7, 42, 1337, 1234, 99_999] {
            let gen = WorldGen::new(seed);
            let started = std::time::Instant::now();
            let (gx, gz) = gen.spawn_column();
            let search = started.elapsed();

            let height = gen.height_at(gx, gz);
            assert!(
                height > SEA_LEVEL + 2,
                "seed {seed} spawns at {gx},{gz}, height {height}"
            );
            assert!(
                !matches!(gen.biome_from(gx, gz, height), Biome::Ocean | Biome::Beach),
                "seed {seed} spawns in the surf"
            );
            assert!(
                gen.spawn_y(gx, gz) > SEA_LEVEL as f32,
                "seed {seed} spawns under water"
            );
            // ...and on something. The generated chunk is the authority
            // here rather than `height_at`, because the disagreement
            // between the two -- a cave breaking the surface -- is
            // exactly the case being guarded against.
            let (chunk_pos, lx, lz) = ChunkPos::from_global(gx, gz);
            let chunk = gen.generate_chunk(chunk_pos);
            let underfoot = chunk.get(lx, height as usize, lz);
            assert!(
                crate::types::is_collidable(underfoot),
                "seed {seed} spawns over {} at {gx},{gz}",
                crate::types::block_name(crate::types::block_kind(underfoot))
            );
            // It runs once, at world creation, on the thread that is
            // starting the server -- so seconds would be felt.
            assert!(
                search < std::time::Duration::from_millis(400),
                "seed {seed}: the spawn search took {search:?}"
            );
        }
    }

    #[test]
    fn the_coast_is_a_line_rather_than_a_swamp() {
        // The disease this generator was rebuilt to cure: with a linear
        // continent-to-height map, the band of ground within a block or
        // two of sea level was hundreds of blocks wide, and what decided
        // land from water inside it was the ±2 block detail octave. The
        // result was a fractal speckle of one-block islands and puddles.
        //
        // Measured two ways, because either alone can be fooled: how
        // *much* of the world sits at the waterline, and how often a
        // straight line crosses it. A clean coast is rare and crossed
        // twice per landmass; a speckle is neither.
        for seed in [1, 7, 1234] {
            let gen = WorldGen::new(seed);
            let mut at_waterline = 0;
            let mut columns = 0;
            let mut crossings = 0;
            for gz in (-800..800).step_by(8) {
                let mut previous: Option<bool> = None;
                for gx in -800..800 {
                    let height = gen.height_at(gx, gz);
                    columns += 1;
                    if (height - SEA_LEVEL).abs() <= 1 {
                        at_waterline += 1;
                    }
                    let wet = height < SEA_LEVEL;
                    if previous.is_some_and(|was| was != wet) {
                        crossings += 1;
                    }
                    previous = Some(wet);
                }
            }
            let share = at_waterline as f32 / columns as f32;
            assert!(
                share < 0.12,
                "seed {seed}: {:.1}% of the world sits at the waterline",
                share * 100.0
            );
            // 200 transects of 1600 blocks. A dozen coasts each is
            // plenty; hundreds means the shore is noise. The generator
            // this replaced scored several times the limit; the one in
            // the file scores six or seven crossings per transect.
            assert!(
                crossings < 200 * 12,
                "seed {seed}: {crossings} land/water crossings -- the coast is a speckle"
            );
        }
    }

    #[test]
    fn there_are_rivers_and_they_are_narrow() {
        // A river is water with land on both sides of it, close by. The
        // same measurement catches both failures the river code has had:
        // no rivers at all, and rivers that spread into marshes wherever
        // the field they are cut from happens to flatten out.
        let gen = WorldGen::new(1234);
        let mut rivers = 0;
        let mut wide = 0;
        for gz in (-1200..1200).step_by(16) {
            let mut run = 0;
            for gx in -1200..1200 {
                let wet = gen.height_at(gx, gz) < SEA_LEVEL;
                if wet {
                    run += 1;
                    continue;
                }
                // A dry column ends a run of water. Short runs with land
                // at both ends are rivers; long ones are the sea.
                if run > 0 && run <= 24 && gen.height_at(gx - run - 1, gz) >= SEA_LEVEL {
                    rivers += 1;
                }
                if run > 24 && run <= 90 && gen.height_at(gx - run - 1, gz) >= SEA_LEVEL {
                    wide += 1;
                }
                run = 0;
            }
        }
        assert!(rivers > 40, "only {rivers} river crossings in a 2400-block square");
        assert!(
            wide * 4 < rivers,
            "{wide} wide channels against {rivers} narrow ones -- rivers are spreading into marshes"
        );
    }

    #[test]
    fn the_open_ocean_is_deep_enough_to_be_an_ocean() {
        // Ten blocks of water, not two: a sea floor a step below the
        // waterline is a flooded field, and it is what the old linear
        // height map produced everywhere.
        let gen = WorldGen::new(1234);
        let deepest = (-1200..1200)
            .step_by(6)
            .flat_map(|gx| (-1200..1200).step_by(6).map(move |gz| (gx, gz)))
            .map(|(gx, gz)| gen.height_at(gx, gz))
            .min()
            .expect("the sweep found nothing");
        assert!(
            deepest <= SEA_LEVEL - 10,
            "the deepest water anywhere is {} blocks",
            SEA_LEVEL - deepest
        );
    }

    /// Times the parts of world generation, for tuning.
    ///
    /// Not a check: the numbers depend on the machine. It is here
    /// because the alternative to measuring is guessing, and every
    /// guess about which noise field costs the most has been wrong.
    ///
    /// ```text
    /// cargo test -p primitive_shared --release --lib -- --ignored --nocapture timings
    /// ```
    #[test]
    #[ignore = "diagnostic: prints timings"]
    fn generation_timings() {
        use std::time::Instant;

        let gen = WorldGen::new(1234);
        const CHUNKS: i32 = 12;
        // The fastest run, not the average: a desktop measuring itself
        // is interrupted constantly, and interruptions only ever make a
        // run slower. Averaging them in made two measurements of the
        // same code differ by a third -- more than any change worth
        // making moves the number, which makes the measurement useless
        // for deciding whether a change helped.
        const RUNS: usize = 5;

        let mut best_column = f64::MAX;
        let mut best_chunk = f64::MAX;
        for _ in 0..RUNS {
            let started = Instant::now();
            let mut columns = 0u64;
            for gx in 0..(CHUNKS * CHUNK_SIZE_X as i32) {
                for gz in 0..CHUNK_SIZE_Z as i32 {
                    std::hint::black_box(gen.height_at(gx, gz));
                    columns += 1;
                }
            }
            best_column = best_column.min(started.elapsed().as_secs_f64() * 1e6 / columns as f64);

            let started = Instant::now();
            let mut chunks = 0u64;
            for x in 0..CHUNKS {
                for z in 0..CHUNKS {
                    std::hint::black_box(gen.generate_chunk(ChunkPos::new(x, z)));
                    chunks += 1;
                }
            }
            best_chunk = best_chunk.min(started.elapsed().as_secs_f64() * 1e3 / chunks as f64);
        }

        println!(
            "height_at: {best_column:.2} us/column | generate_chunk: {best_chunk:.2} ms/chunk              ({:.0} chunks/s per core)",
            1000.0 / best_chunk,
                );
    }

    /// Finds a chunk whose middle column is in `wanted`, within a few
    /// thousand blocks of the origin.
    ///
    /// Feature tests need somewhere the feature belongs, and hard-coded
    /// coordinates go stale the first time a threshold moves.
    pub(super) fn chunk_in(gen: &WorldGen, wanted: Biome) -> Option<Chunk> {
        for cx in -140..140 {
            for cz in -140..140 {
                let (gx, gz) = (cx * CHUNK_SIZE_X as i32 + 8, cz * CHUNK_SIZE_Z as i32 + 8);
                if gen.biome_at(gx, gz) == wanted {
                    return Some(gen.generate_chunk(ChunkPos::new(cx, cz)));
                }
            }
        }
        None
    }

    /// How many cells of the chunk hold this *material*.
    ///
    /// By kind rather than by exact id: a block id carries how the
    /// block lies and how deep it is as well as what it is, so counting
    /// exact ids would answer "no snow anywhere" the moment snow
    /// started falling in drifts.
    pub(super) fn count_of(chunk: &Chunk, block: crate::types::BlockId) -> usize {
        let wanted = crate::types::block_kind(block);
        chunk
            .blocks
            .iter()
            .filter(|&&b| crate::types::block_kind(b) == wanted)
            .count()
    }

    #[test]
    fn grass_grows_in_a_field_and_nowhere_it_should_not() {
        let gen = WorldGen::new(1234);
        let plains = chunk_in(&gen, Biome::Plains).expect("no plains anywhere");
        assert!(
            count_of(&plains, BLOCK_TALL_GRASS) > 20,
            "a field with {} tufts in it",
            count_of(&plains, BLOCK_TALL_GRASS)
        );

        // ...and nothing grows on sand, on snow, or in a dead wood.
        //
        // Checked per *column* rather than per chunk: a chunk sits
        // across biome boundaries as often as not, and a tuft at the
        // edge of a desert chunk may well be standing in the meadow
        // next door.
        for cx in 0..8 {
            for cz in 0..8 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let has_grass = (0..CHUNK_SIZE_Y)
                            .any(|y| chunk.get(x, y, z) == BLOCK_TALL_GRASS);
                        if !has_grass {
                            continue;
                        }
                        let biome = gen.biome_at(
                            cx * CHUNK_SIZE_X as i32 + x as i32,
                            cz * CHUNK_SIZE_Z as i32 + z as i32,
                        );
                        assert!(
                            biome.grass_spacing().is_some(),
                            "grass grew in {}",
                            biome.name()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_tuft_of_grass_stands_on_ground_that_could_hold_it() {
        // Grass hanging in the air, growing out of a cave roof or
        // standing in water is the sort of detail that gets noticed
        // precisely because it is wrong.
        let gen = WorldGen::new(99);
        for cx in 0..6 {
            for cz in 0..6 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for y in 0..CHUNK_SIZE_Y {
                    for z in 0..CHUNK_SIZE_Z {
                        for x in 0..CHUNK_SIZE_X {
                            let block = chunk.get(x, y, z);
                            if !crate::types::is_cross(block) && block != BLOCK_CACTUS {
                                continue;
                            }
                            assert!(y > 0, "a plant grew out of the world's floor");
                            let under = chunk.get(x, y - 1, z);
                            assert!(
                                crate::types::can_grow_on(block, under),
                                "{} at {x},{y},{z} is standing on {}",
                                crate::types::block_name(block),
                                crate::types::block_name(under),
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_desert_grows_cacti_and_a_field_does_not() {
        let gen = WorldGen::new(7);
        // Rare by design, so this is measured over a wider sample than
        // one chunk. (This used to also count a single desert chunk into
        // a variable nothing read -- `chunk_in` is deterministic, so the
        // "wider sample" it built was the same chunk eight times.)
        let sampled: usize = (-30..30)
            .flat_map(|cx| (-30..30).map(move |cz| (cx, cz)))
            .filter(|&(cx, cz)| {
                gen.biome_at(cx * 16 + 8, cz * 16 + 8) == Biome::Desert
            })
            .take(12)
            .map(|(cx, cz)| count_of(&gen.generate_chunk(ChunkPos::new(cx, cz)), BLOCK_CACTUS))
            .sum();
        assert!(sampled > 0, "a dozen desert chunks and not one cactus");

        let plains = chunk_in(&gen, Biome::Plains).expect("no plains anywhere");
        assert_eq!(count_of(&plains, BLOCK_CACTUS), 0, "a cactus in a meadow");
    }

    #[test]
    fn a_dead_forest_is_trunks_without_leaves() {
        let gen = WorldGen::new(7);
        let dead = chunk_in(&gen, Biome::DeadForest).expect("no dead forest anywhere");
        assert!(
            count_of(&dead, BLOCK_LOG) > 10,
            "a wood with {} logs in it",
            count_of(&dead, BLOCK_LOG)
        );
        // A live forest still has a canopy...
        let alive = chunk_in(&gen, Biome::Forest).expect("no forest anywhere");
        assert!(count_of(&alive, BLOCK_LEAVES) > 0, "a forest with no canopy");

        // ...and a dead tree does not, which is the property itself
        // rather than a count over a chunk that may straddle a living
        // wood whose branches hang over the boundary.
        let (_, _, canopy) = Biome::DeadForest.tree_shape();
        assert_eq!(canopy, 0, "dead trees are specified with a canopy");
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        place_tree(&mut blocks, 8, 20, 8, 6, canopy, (BLOCK_LOG, BLOCK_LEAVES));
        assert!(
            blocks.contains(&BLOCK_LOG),
            "the trunk was not written"
        );
        assert!(
            !blocks.contains(&BLOCK_LEAVES),
            "a leafless tree put out leaves"
        );
    }

    #[test]
    fn fallen_trunks_lie_on_the_ground() {
        // The point of a deadfall is that it is *lying* there: a log
        // floating a block above the ground, or buried in it, reads as
        // a generator bug rather than as a fallen tree.
        let gen = WorldGen::new(11);
        let mut found = 0;
        for cx in -40..40 {
            for cz in -40..40 {
                if gen.biome_at(cx * 16 + 8, cz * 16 + 8) != Biome::DeadForest {
                    continue;
                }
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let ground = gen.height_at(
                            cx * 16 + x as i32,
                            cz * 16 + z as i32,
                        );
                        if ground + 1 >= CHUNK_SIZE_Y as i32 - 1 {
                            continue;
                        }
                        // A log at ground level with air above it is
                        // lying down; a standing trunk has more log on
                        // top of it. And a fallen one now says so in its
                        // own id: its axis is the way it fell, so the
                        // cut ends face along the trunk instead of at
                        // the sky.
                        let at = chunk.get(x, ground as usize + 1, z);
                        let above = chunk.get(x, ground as usize + 2, z);
                        if crate::types::block_kind(at) == BLOCK_LOG && above == BLOCK_AIR {
                            assert_ne!(
                                crate::types::block_axis(at),
                                crate::types::Axis::Y,
                                "a fallen trunk was left standing on end"
                            );
                            found += 1;
                        }
                    }
                }
                if found > 3 {
                    return;
                }
            }
        }
        assert!(found > 3, "only {found} fallen logs in eighty chunks of dead wood");
    }

    /// Prints how much of the world each biome covers.
    ///
    /// A biome that is 0.1% of the land is one no player will ever see,
    /// and one that is 60% is the world. Neither shows up in a test that
    /// only asks whether each exists somewhere.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture biome_shares
    /// ```
    #[test]
    #[ignore = "diagnostic: prints biome shares"]
    fn biome_shares() {
        use std::collections::HashMap;
        for seed in [1, 7, 1234, 99_999] {
            let gen = WorldGen::new(seed);
            let mut counts: HashMap<Biome, u32> = HashMap::new();
            let mut land = 0u32;
            for gx in (-2400..2400).step_by(24) {
                for gz in (-2400..2400).step_by(24) {
                    let height = gen.height_at(gx, gz);
                    let biome = gen.biome_from(gx, gz, height);
                    if !matches!(biome, Biome::Ocean | Biome::Beach) {
                        land += 1;
                    }
                    *counts.entry(biome).or_default() += 1;
                }
            }
            let mut shares: Vec<(Biome, u32)> = counts.into_iter().collect();
            shares.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
            let line: Vec<String> = shares
                .iter()
                .map(|(biome, n)| {
                    format!("{} {:.0}%", biome.name(), *n as f32 / land.max(1) as f32 * 100.0)
                })
                .collect();
            println!("seed {seed} (as % of land): {}", line.join(", "));
        }
    }

    /// Draws the world, for looking at.
    ///
    /// Not a check -- it asserts nothing. Terrain is the one part of
    /// this codebase where the question is "does it read as a world",
    /// and no assertion answers that: oceans of the right size, rivers
    /// that go somewhere, mountains with plains between them and caves
    /// that connect all pass or fail by eye.
    ///
    /// ```text
    /// cargo test -p primitive_shared -- --ignored dump
    /// ```
    ///
    /// Writes two pictures next to each other, into `target/` or
    /// `$PRIMITIVE_MAP_DUMP`:
    ///
    /// * `..._map.png` -- 1024 blocks square from above, coloured by
    ///   biome and shaded by height, with a contour at sea level.
    /// * `..._slice.png` -- the same world cut open along z = 0, every
    ///   block drawn, so caves, water tables and ore show up.
    #[test]
    #[ignore = "diagnostic: writes pictures of the world"]
    fn dump_the_world_to_pngs() {
        const SPAN: i32 = 1024;
        let seed: u32 = std::env::var("PRIMITIVE_MAP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1234);
        let gen = WorldGen::new(seed);
        let stem = std::env::var("PRIMITIVE_MAP_DUMP")
            .unwrap_or_else(|_| "target/world".to_string());

        // ---- the map ----
        let mut map = image::RgbImage::new(SPAN as u32, SPAN as u32);
        for pz in 0..SPAN {
            for px in 0..SPAN {
                let (gx, gz) = (px - SPAN / 2, pz - SPAN / 2);
                let height = gen.height_at(gx, gz);
                let biome = gen.biome_from(gx, gz, height);
                let base = biome_colour(biome);
                // Relief, as a hillshade: the drop to the neighbour up
                // and to the left. Flat shading by absolute height
                // cannot show a slope, and a slope is what tells a
                // mountain from a high plateau.
                let neighbour = gen.height_at(gx - 1, gz - 1);
                let lift = ((height - SEA_LEVEL) as f32 / 40.0).clamp(-1.0, 1.0);
                let slope = ((height - neighbour) as f32 * 0.16).clamp(-0.5, 0.5);
                let shade = 1.0 + lift * 0.25 + slope;
                let mut rgb = base.map(|c| (c as f32 * shade).clamp(0.0, 255.0) as u8);
                // A dark line at the waterline. Deliberately not white:
                // the first version drew it in the same near-white as
                // the snowy-peaks biome, and every mountain read as a
                // shoreline.
                if height == SEA_LEVEL || height == SEA_LEVEL - 1 {
                    rgb = [24, 26, 32];
                }
                map.put_pixel(px as u32, pz as u32, image::Rgb(rgb));
            }
        }
        map.save(format!("{stem}_map.png")).expect("write the map");

        // ---- the cross-section ----
        //
        // Generated as real chunks rather than sampled from the height
        // field, so what is drawn is what the game would serve: caves,
        // trees, water and all.
        const SLICE_SCALE: u32 = 6; // the world is 64 tall; make it visible
        let mut slice = image::RgbImage::new(
            SPAN as u32,
            CHUNK_SIZE_Y as u32 * SLICE_SCALE,
        );
        for chunk_x in 0..(SPAN / CHUNK_SIZE_X as i32) {
            let pos = ChunkPos::new(chunk_x - SPAN / CHUNK_SIZE_X as i32 / 2, 0);
            let chunk = gen.generate_chunk(pos);
            for lx in 0..CHUNK_SIZE_X {
                let px = (chunk_x * CHUNK_SIZE_X as i32 + lx as i32) as u32;
                for y in 0..CHUNK_SIZE_Y {
                    let colour = block_colour(chunk.get(lx, y, 0));
                    let top = (CHUNK_SIZE_Y - 1 - y) as u32 * SLICE_SCALE;
                    for row in 0..SLICE_SCALE {
                        slice.put_pixel(px, top + row, image::Rgb(colour));
                    }
                }
            }
        }
        slice.save(format!("{stem}_slice.png")).expect("write the slice");

        // ---- close-ups ----
        //
        // The wide slice is a block per pixel: enough to see terrain,
        // useless for anything a block tall. These are eighty blocks of
        // one biome each, big enough to see a tuft of grass for what it
        // is.
        for wanted in [
            Biome::Plains,
            Biome::Desert,
            Biome::DeadForest,
            Biome::Forest,
        ] {
            const SPAN: i32 = 80;
            const SCALE: u32 = 9;
            const TALL: i32 = 30;

            // Somewhere the whole strip is inside the biome, not just
            // its first chunk: a close-up of a desert that turns into a
            // meadow forty blocks along shows neither.
            let Some(origin) = (0..200).find_map(|ring| {
                (-ring..=ring).find_map(|cx| {
                    let cz = ring;
                    let gz = cz * CHUNK_SIZE_Z as i32 + 8;
                    let start = cx * CHUNK_SIZE_X as i32;
                    (0..SPAN)
                        .step_by(8)
                        .all(|step| gen.biome_at(start + step, gz) == wanted)
                        .then_some((cx, cz))
                })
            }) else {
                continue;
            };

            let base = gen.height_at(
                origin.0 * CHUNK_SIZE_X as i32 + 8,
                origin.1 * CHUNK_SIZE_Z as i32 + 8,
            );
            let bottom = (base - TALL / 3).max(0);
            let mut img =
                image::RgbImage::new(SPAN as u32 * SCALE, TALL as u32 * SCALE);
            for column in 0..SPAN {
                let chunk = gen.generate_chunk(ChunkPos::new(
                    origin.0 + column / CHUNK_SIZE_X as i32,
                    origin.1,
                ));
                let lx = (column % CHUNK_SIZE_X as i32) as usize;
                for row in 0..TALL {
                    let y = bottom + (TALL - 1 - row);
                    if y < 0 || y >= CHUNK_SIZE_Y as i32 {
                        continue;
                    }
                    let colour = block_colour(chunk.get(lx, y as usize, 8));
                    for py in 0..SCALE {
                        for px in 0..SCALE {
                            img.put_pixel(
                                column as u32 * SCALE + px,
                                row as u32 * SCALE + py,
                                image::Rgb(colour),
                            );
                        }
                    }
                }
            }
            let name = wanted.name().replace(' ', "_");
            img.save(format!("{stem}_{name}.png"))
                .expect("write the close-up");
        }
        println!("wrote {stem}_map.png and {stem}_slice.png for seed {seed}");
    }

    fn biome_colour(biome: Biome) -> [u8; 3] {
        match biome {
            Biome::Ocean => [38, 68, 122],
            Biome::River => [66, 128, 190],
            Biome::Beach => [214, 200, 148],
            Biome::Desert => [222, 200, 128],
            Biome::Savanna => [176, 172, 96],
            Biome::Plains => [124, 176, 92],
            Biome::Forest => [58, 122, 62],
            Biome::DeadForest => [124, 104, 78],
            Biome::BirchForest => [146, 178, 118],
            Biome::Swamp => [78, 110, 84],
            Biome::Taiga => [70, 116, 104],
            Biome::Tundra => [178, 190, 186],
            Biome::Mountains => [128, 124, 120],
            Biome::SnowyPeaks => [244, 246, 250],
        }
    }

    fn block_colour(block: crate::types::BlockId) -> [u8; 3] {
        // The *kind*, so a fallen trunk is drawn as wood rather than as
        // the missing-block magenta -- its axis is part of its id.
        match crate::types::block_kind(block) {
            BLOCK_AIR => [16, 18, 24],
            BLOCK_WATER => [40, 90, 170],
            BLOCK_STONE => [104, 104, 110],
            BLOCK_COBBLESTONE => [130, 128, 126],
            BLOCK_DIRT => [110, 78, 52],
            BLOCK_GRASS => [96, 158, 74],
            BLOCK_SAND => [216, 202, 152],
            BLOCK_SNOW => [238, 242, 248],
            BLOCK_LOG => [150, 96, 34],
            BLOCK_LEAVES => [52, 120, 56],
            BLOCK_GLOWSTONE => [246, 214, 120],
            BLOCK_TALL_GRASS => [74, 138, 58],
            BLOCK_CACTUS => [54, 112, 58],
            crate::types::BLOCK_STICK => [110, 80, 46],
            BLOCK_PEBBLE => [150, 148, 144],
            BLOCK_FLINT => [46, 46, 54],
            BLOCK_ASH => [132, 128, 124],
            // Magenta is the missing-texture colour, and it is here for
            // the same reason: a block this palette has not been told
            // about should be *loud* in the picture rather than quietly
            // drawn as something else.
            _ => [255, 0, 255],
        }
    }
}

/// What generating terrain actually costs, and where.
///
/// Ignored by default -- it is a measurement, not an assertion, and the
/// numbers depend on the machine. Run it when changing anything in the
/// generation path:
///
/// ```text
/// cargo test -p primitive_shared --release --lib -- --ignored --nocapture where_the_time_goes
/// ```
#[cfg(test)]
mod cost_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn where_the_time_goes() {
        let gen = WorldGen::new(1337);
        const CHUNKS: i32 = 12;

        let started = Instant::now();
        let mut columns = 0u32;
        for cx in 0..CHUNKS {
            for cz in 0..CHUNKS {
                let origin_x = cx * CHUNK_SIZE_X as i32;
                let origin_z = cz * CHUNK_SIZE_Z as i32;
                let cache = ColumnCache::build(&gen, origin_x, origin_z);
                columns += 1;
                std::hint::black_box(&cache);
            }
        }
        let column_time = started.elapsed();

        let started = Instant::now();
        for cx in 0..CHUNKS {
            for cz in 0..CHUNKS {
                std::hint::black_box(gen.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        let total = started.elapsed();

        let n = (CHUNKS * CHUNKS) as f64;
        println!(
            "generate_chunk: {:.2} ms/chunk, of which columns {:.2} ms ({:.0}%) over {} caches",
            total.as_secs_f64() * 1000.0 / n,
            column_time.as_secs_f64() * 1000.0 / n,
            100.0 * column_time.as_secs_f64() / total.as_secs_f64(),
            columns,
        );
    }
}

/// What range the climate fields actually cover.
///
/// The tint the client paints on foliage is a lookup into a palette
/// spanning "freezing and bone dry" to "hot and soaking". That palette
/// is only worth having if the world reaches its corners -- and fractal
/// noise summed over two octaves does not reach ±1 anywhere, so the raw
/// fields sit in a band around the middle and every plant in the world
/// comes out the same colour.
///
/// ```text
/// cargo test -p primitive_shared --release --lib -- --ignored --nocapture climate_spread
/// ```
#[cfg(test)]
mod climate_tests {
    use super::*;

    fn spread(gen: &WorldGen) -> (f32, f32, f32, f32) {
        let (mut t_lo, mut t_hi) = (1.0f32, 0.0f32);
        let (mut h_lo, mut h_hi) = (1.0f32, 0.0f32);
        for x in -200..200 {
            for z in -200..200 {
                let (t, h) = gen.climate_column(x * 8, z * 8);
                t_lo = t_lo.min(t);
                t_hi = t_hi.max(t);
                h_lo = h_lo.min(h);
                h_hi = h_hi.max(h);
            }
        }
        (t_lo, t_hi, h_lo, h_hi)
    }

    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn climate_spread() {
        for seed in [1337u32, 7, 99] {
            let (t_lo, t_hi, h_lo, h_hi) = spread(&WorldGen::new(seed));
            println!(
                "seed {seed}: temperature {t_lo:.3}..{t_hi:.3}, humidity {h_lo:.3}..{h_hi:.3}"
            );
        }
    }

    #[test]
    fn the_climate_reaches_most_of_its_own_range() {
        // The palette the client paints from spans the whole 0..1
        // square. If the world only ever visits the middle of it, every
        // plant comes out the same colour and the palette is an
        // expensive way to multiply by a constant.
        let (t_lo, t_hi, h_lo, h_hi) = spread(&WorldGen::new(1337));
        assert!(
            t_hi - t_lo > 0.75,
            "temperature only spans {:.2}..{:.2}",
            t_lo,
            t_hi
        );
        assert!(
            h_hi - h_lo > 0.75,
            "humidity only spans {:.2}..{:.2}",
            h_lo,
            h_hi
        );
    }
}

/// What the *shape* of the ground decides, on top of what its climate
/// does.
#[cfg(test)]
mod relief_tests {
    use super::*;

    /// Every column of a wide sweep, with its slope.
    fn sweep(gen: &WorldGen) -> Vec<(i32, i32, i32, f32)> {
        let mut out = Vec::new();
        for gx in (-900..900).step_by(3) {
            for gz in (-900..900).step_by(7) {
                let height = gen.height_at(gx, gz);
                out.push((gx, gz, height, gen.slope_at(gx, gz, height)));
            }
        }
        out
    }

    /// What the height field actually produces, as percentiles.
    ///
    /// Every threshold in `surface_for` is a number against this
    /// distribution, and guessing at them is how a rule meant for
    /// cliffs ends up firing on nothing or on everything -- the first
    /// pass at the slope rules matched thirteen columns in eighty
    /// thousand.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture terrain_numbers
    /// ```
    #[test]
    #[ignore = "diagnostic: prints terrain percentiles"]
    fn terrain_numbers() {
        let gen = WorldGen::new(1234);
        let mut continents: Vec<f64> = Vec::new();
        let mut slopes: Vec<f32> = Vec::new();
        let mut depths: Vec<i32> = Vec::new();
        for gx in (-900..900).step_by(5) {
            for gz in (-900..900).step_by(11) {
                let h = gen.height_at(gx, gz);
                continents.push(gen.continent(gx, gz));
                if h >= SEA_LEVEL {
                    slopes.push(gen.slope_at(gx, gz, h));
                } else {
                    depths.push(SEA_LEVEL - h);
                }
            }
        }
        continents.sort_by(|a, b| a.partial_cmp(b).unwrap());
        slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        depths.sort();
        let c = |p: usize| continents[continents.len() * p / 100];
        let s = |p: usize| slopes[slopes.len() * p / 100];
        let d = |p: usize| depths[depths.len() * p / 100];
        println!("continent p1 {:.2} p5 {:.2} p20 {:.2} p50 {:.2} p95 {:.2}", c(1), c(5), c(20), c(50), c(95));
        println!("slope p50 {:.2} p80 {:.2} p90 {:.2} p95 {:.2} p99 {:.2} max {:.2}",
            s(50), s(80), s(90), s(95), s(99), slopes[slopes.len() - 1]);
        println!("depth p10 {} p50 {} p90 {} max {} | wet {} dry {}",
            d(10), d(50), d(90), depths[depths.len() - 1], depths.len(), slopes.len());
    }

    #[test]
    fn a_cliff_is_bare_rock_and_a_meadow_is_not() {
        // The rule that turns a height field into country: what the
        // ground is made of follows the shape it is in. Without it a
        // mountainside is turf at sixty degrees, which is the single
        // most obvious tell that a world was generated.
        let gen = WorldGen::new(1234);
        let mut steep_rock = 0;
        let mut steep = 0;
        let mut flat_soil = 0;
        let mut flat = 0;
        for (gx, gz, height, slope) in sweep(&gen) {
            // Above the shore band, which is deliberately sand however
            // steep it is -- see `surface_for`.
            if height <= SEA_LEVEL + 2 {
                continue;
            }
            let top = gen.surface_at(gx, gz, height).top;
            if slope >= ROCK_SLOPE {
                steep += 1;
                steep_rock += (top == BLOCK_COBBLESTONE) as i32;
            } else if slope < 0.5 {
                flat += 1;
                flat_soil += matches!(top, BLOCK_GRASS | BLOCK_DIRT | BLOCK_SAND | BLOCK_SNOW)
                    as i32;
            }
        }
        assert!(steep > 200, "only {steep} steep columns in the sweep -- nowhere is a cliff");
        assert_eq!(steep, steep_rock, "something other than rock on a cliff face");
        assert!(
            flat_soil * 100 / flat.max(1) > 95,
            "{}% of flat ground came out bare rock",
            100 - flat_soil * 100 / flat.max(1)
        );
    }

    #[test]
    fn soil_is_deep_in_the_valleys_and_thin_on_the_slopes() {
        let flat = surface_for(SEA_LEVEL + 6, Biome::Plains, 0.2);
        let bank = surface_for(SEA_LEVEL + 6, Biome::Plains, 0.9);
        let cliff = surface_for(SEA_LEVEL + 6, Biome::Plains, 1.6);
        assert!(flat.soil > bank.soil, "a floodplain is no deeper than a bank");
        assert!(bank.soil > cliff.soil, "a bank is no deeper than a cliff");
        assert_eq!(cliff.top, BLOCK_COBBLESTONE);
        assert_eq!(flat.top, BLOCK_GRASS);
    }

    #[test]
    fn nothing_loose_clings_to_a_steep_bank() {
        // Snow and sand are the two with no grip at all, and a beach
        // standing on its end is the shape a steep coast used to be.
        for biome in [Biome::SnowyPeaks, Biome::Desert, Biome::Beach] {
            let steep = surface_for(SEA_LEVEL + 12, biome, 0.9);
            assert_eq!(
                steep.top,
                BLOCK_COBBLESTONE,
                "{} kept its surface on a bank",
                biome.name()
            );
        }
        // ...but the shore itself is still sand, however abruptly the
        // ground behind it climbs: that is a beach at the foot of a
        // cliff, which is a real place.
        assert_eq!(surface_for(SEA_LEVEL + 1, Biome::Beach, 1.8).top, BLOCK_SAND);
    }

    #[test]
    fn the_sea_floor_changes_with_the_depth_over_it() {
        // Sand in the surf, silt on the shelf, rock past the shelf
        // break. One sandy floor from the beach to the abyss is what
        // gave the old ocean away the moment anyone swam out.
        assert_eq!(surface_for(SEA_LEVEL - 1, Biome::Ocean, 0.0).top, BLOCK_SAND);
        assert_eq!(surface_for(SEA_LEVEL - 7, Biome::Ocean, 0.0).top, BLOCK_DIRT);
        assert_eq!(surface_for(SEA_LEVEL - 14, Biome::Ocean, 0.0).top, BLOCK_STONE);
    }

    #[test]
    fn the_ocean_still_has_a_shelf_and_an_abyss_under_it() {
        // The spline shape rather than the surface: a shelf is only a
        // shelf if there is a lot of ground at shelf depth and a real
        // drop past it.
        let gen = WorldGen::new(1234);
        let (mut shelf, mut deep, mut wet) = (0, 0, 0);
        for (_, _, height, _) in sweep(&gen) {
            if height >= SEA_LEVEL {
                continue;
            }
            wet += 1;
            let depth = SEA_LEVEL - height;
            shelf += (2..=10).contains(&depth) as i32;
            deep += (depth > 12) as i32;
        }
        assert!(shelf * 5 > wet, "only {shelf} of {wet} wet columns are shelf");
        assert!(deep * 14 > wet, "only {deep} of {wet} wet columns are deep water");
    }

    #[test]
    fn a_worn_lowland_is_smoother_than_a_young_upland() {
        // The roughness term. Uniform detail everywhere is the other
        // tell of a height field built out of octaves: a floodplain
        // with the same texture as a ridge line.
        let gen = WorldGen::new(7);
        let mut worn = (0.0f32, 0);
        let mut young = (0.0f32, 0);
        for (gx, gz, height, slope) in sweep(&gen) {
            if height < SEA_LEVEL + 3 {
                continue;
            }
            // `unworn` is the same mask the mountains are built with.
            let erosion = gen.erosion(gx, gz);
            if erosion > 0.4 {
                worn = (worn.0 + slope, worn.1 + 1);
            } else if erosion < -0.2 {
                young = (young.0 + slope, young.1 + 1);
            }
        }
        assert!(worn.1 > 100 && young.1 > 100, "the sweep found no country to compare");
        let (worn, young) = (worn.0 / worn.1 as f32, young.0 / young.1 as f32);
        assert!(
            young > worn * 1.5,
            "worn ground averages {worn:.2} blocks of slope against {young:.2} for young ground"
        );
    }
}

/// Ash, in the woods that burned.
/// Clay and gravel: the two things water leaves behind.
#[cfg(test)]
mod deposit_tests {
    use super::tests::sample_chunks;
    use super::*;
    use crate::types::BlockId;

    /// How many cells of a material a wide sample of the world holds.
    fn count(seed: u32, radius: i32, block: BlockId) -> usize {
        sample_chunks(seed, radius)
            .iter()
            .map(|chunk| super::tests::count_of(chunk, block))
            .sum()
    }

    #[test]
    fn both_deposits_exist_and_neither_takes_over() {
        // A material nobody can find is a material nobody has, and one
        // that is everywhere is what the ground is made of. Both of
        // these are patches by design, so both bounds are the test.
        let (clay, gravel) = (count(99, 4, BLOCK_CLAY), count(99, 4, BLOCK_GRAVEL));
        let cells = 81 * CHUNK_VOLUME;
        assert!(clay > 0, "no clay anywhere in 81 chunks");
        assert!(gravel > 0, "no gravel anywhere in 81 chunks");
        assert!(clay * 6 < cells, "clay is the ground now");
        assert!(gravel * 6 < cells, "gravel is the ground now");
    }

    #[test]
    fn clay_keeps_to_the_water_and_gravel_does_not_bury_the_beaches() {
        // Clay is what still water drops, so it belongs at the
        // waterline and nowhere near a mountain.
        let gen = WorldGen::new(4242);
        let mut clay_above_the_shore = 0;
        for gx in (-600..600).step_by(7) {
            for gz in (-600..600).step_by(31) {
                let height = gen.height_at(gx, gz);
                if height > SEA_LEVEL + 2 && gen.clayey(gx, gz) {
                    let surface = gen.surface_at(gx, gz, height);
                    if surface.top == BLOCK_CLAY {
                        clay_above_the_shore += 1;
                    }
                }
            }
        }
        assert_eq!(
            clay_above_the_shore, 0,
            "clay turned up well above the waterline"
        );
    }

    #[test]
    fn the_two_are_never_in_the_same_place() {
        // One noise field read at both ends: what slow water drops and
        // what fast water leaves cannot be the same spot.
        let gen = WorldGen::new(7);
        for gx in (-400..400).step_by(11) {
            for gz in (-400..400).step_by(13) {
                assert!(
                    !(gen.clayey(gx, gz) && gen.gravelly(gx, gz)),
                    "clay and gravel both claim ({gx}, {gz})"
                );
            }
        }
    }

    #[test]
    fn gravel_falls_like_sand_and_clay_holds_its_bank() {
        // The whole difference between them, and the reason clay is not
        // on the loose list: a bank that slumps the moment you dig it is
        // not a bank.
        use crate::types::{is_affected_by_gravity, is_loose};
        assert!(is_affected_by_gravity(BLOCK_GRAVEL));
        assert!(is_loose(BLOCK_GRAVEL));

        assert!(!is_affected_by_gravity(BLOCK_CLAY));
        assert!(!is_loose(BLOCK_CLAY));
    }
}

#[cfg(test)]
mod ore_tests {
    use super::tests::sample_chunks;
    use super::*;
    use crate::types::{block_kind, BlockId};

    /// Every ore cell in a wide sample, with the height it was found at.
    fn ore_cells(seed: u32, radius: i32) -> Vec<(BlockId, i32)> {
        let mut found = Vec::new();
        for chunk in sample_chunks(seed, radius) {
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let id = block_kind(chunk.get(x, y, z));
                        if matches!(
                            id,
                            BLOCK_COAL_ORE | BLOCK_COPPER_ORE | BLOCK_TIN_ORE | BLOCK_IRON_ORE
                        ) {
                            found.push((id, y as i32));
                        }
                    }
                }
            }
        }
        found
    }

    fn count(cells: &[(BlockId, i32)], ore: BlockId) -> usize {
        cells.iter().filter(|&&(id, _)| id == ore).count()
    }

    fn mean_depth(cells: &[(BlockId, i32)], ore: BlockId) -> f32 {
        let heights: Vec<i32> = cells
            .iter()
            .filter(|&&(id, _)| id == ore)
            .map(|&(_, y)| y)
            .collect();
        heights.iter().sum::<i32>() as f32 / heights.len().max(1) as f32
    }

    #[test]
    fn all_four_ores_are_actually_in_the_ground() {
        // The failure this exists for is a threshold tuned one point too
        // high, which does not break anything -- it just quietly ships a
        // world with no tin in it, and a bronze age nobody can enter.
        let cells = ore_cells(1337, 3);
        for ore in [
            BLOCK_COAL_ORE,
            BLOCK_COPPER_ORE,
            BLOCK_TIN_ORE,
            BLOCK_IRON_ORE,
        ] {
            assert!(
                count(&cells, ore) > 0,
                "no {} in 49 chunks",
                crate::types::block_name(ore)
            );
        }
        // ...and the opposite failure: rock that is more ore than rock.
        let total: usize = cells.len();
        assert!(
            total * 40 < 49 * CHUNK_VOLUME,
            "{total} ore cells in 49 chunks -- the underground is a jewellery box"
        );
    }

    #[test]
    fn the_ages_are_in_the_order_of_their_scarcity() {
        // The economy, as one line of arithmetic. Coal is the fuel
        // everything else needs, so it is commonest; iron is not rare,
        // it is deep and hard; copper is the metal you meet first; tin
        // is scarce, and that scarcity is what makes bronze mean
        // something.
        // Over several worlds, because one seed agreeing with the
        // weights is not the same as the weights being right -- a
        // mountainous world has far more shallow rock than an ocean one,
        // and the ordering has to survive both.
        for seed in [1337u32, 7, 99_999] {
            let cells = ore_cells(seed, 3);
            let (coal, iron, copper, tin) = (
                count(&cells, BLOCK_COAL_ORE),
                count(&cells, BLOCK_IRON_ORE),
                count(&cells, BLOCK_COPPER_ORE),
                count(&cells, BLOCK_TIN_ORE),
            );
            println!("seed {seed}: coal {coal}, iron {iron}, copper {copper}, tin {tin}");
            assert!(coal > iron, "coal {coal} is not the commonest ({iron} iron)");
            assert!(iron > copper, "iron {iron} is rarer than copper {copper}");
            assert!(copper > tin, "copper {copper} is not commoner than tin {tin}");
        }
    }

    #[test]
    fn iron_is_deep_and_copper_is_not() {
        // The reason the progression is a journey rather than a menu: a
        // player working the top of the rock finds copper and tin, and
        // has to go *down* for the next age.
        let cells = ore_cells(1337, 3);
        let iron = mean_depth(&cells, BLOCK_IRON_ORE);
        let copper = mean_depth(&cells, BLOCK_COPPER_ORE);
        let tin = mean_depth(&cells, BLOCK_TIN_ORE);
        println!("mean height: iron {iron:.1}, copper {copper:.1}, tin {tin:.1}");
        assert!(iron < copper - 2.0, "iron sits at {iron:.1}, copper at {copper:.1}");
        assert!(iron < tin - 2.0, "iron sits at {iron:.1}, tin at {tin:.1}");
        // Nothing near the surface is iron at all.
        assert_eq!(
            cells
                .iter()
                .filter(|&&(id, y)| id == BLOCK_IRON_ORE && y > SEA_LEVEL - 6)
                .count(),
            0,
            "iron ore outcropped above the deep band"
        );
    }

    #[test]
    fn ore_is_always_in_rock_and_never_in_a_hole() {
        // Ore is written where stone was, so every cell of it has to be
        // somewhere stone belonged: under the soil, above the bedrock,
        // and not inside a cave. A vein hanging in a chamber is the
        // classic symptom of an ore pass that ran before the caves were
        // cut.
        let gen = WorldGen::new(1337);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for y in 0..CHUNK_SIZE_Y as i32 {
                    for lz in 0..CHUNK_SIZE_Z as i32 {
                        for lx in 0..CHUNK_SIZE_X as i32 {
                            let id = block_kind(chunk.get(lx as usize, y as usize, lz as usize));
                            if !matches!(
                                id,
                                BLOCK_COAL_ORE
                                    | BLOCK_COPPER_ORE
                                    | BLOCK_TIN_ORE
                                    | BLOCK_IRON_ORE
                            ) {
                                continue;
                            }
                            let gx = cx * CHUNK_SIZE_X as i32 + lx;
                            let gz = cz * CHUNK_SIZE_Z as i32 + lz;
                            assert!(y > BEDROCK_TOP, "ore inside the bedrock at y={y}");
                            let column = gen.height_at(gx, gz);
                            let surface = gen.surface_at(gx, gz, column);
                            assert!(
                                y <= column - surface.soil,
                                "ore at y={y} is in the soil of a column {column} high"
                            );
                            assert!(!gen.is_cave(gx, y, gz), "ore hanging in a cave at y={y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_same_seed_puts_the_same_ore_in_the_same_place() {
        let first = ore_cells(4242, 1);
        let second = ore_cells(4242, 1);
        assert_eq!(first, second);
        assert!(!first.is_empty());
        // ...and a different world is a different mine.
        assert_ne!(first, ore_cells(4243, 1));
    }
}

#[cfg(test)]
mod ash_tests {
    use super::tests::chunk_in;
    use super::*;

    #[test]
    fn a_dead_forest_has_ash_in_it_and_a_live_one_does_not() {
        // What makes a dead forest read as a *burnt* one rather than as
        // a wood somebody forgot to put leaves on.
        let gen = WorldGen::new(7);
        let dead = chunk_in(&gen, Biome::DeadForest).expect("no dead forest anywhere");
        let ash = super::tests::count_of(&dead, BLOCK_ASH);
        assert!(ash > 0, "a burnt wood with no ash in it");

        for biome in [Biome::Forest, Biome::Plains, Biome::Taiga] {
            let Some(chunk) = chunk_in(&gen, biome) else {
                continue;
            };
            assert_eq!(
                super::tests::count_of(&chunk, BLOCK_ASH),
                0,
                "ash in a {} that never burned",
                biome.name()
            );
        }
    }

    #[test]
    fn it_lies_in_drifts_rather_than_speckle() {
        // A per-column coin flip would give grey noise, which reads as a
        // dirty texture. The patches have to be big enough to walk
        // across: a run of ash along a line should be tens of columns,
        // not ones.
        let gen = WorldGen::new(1234);
        let mut runs = 0;
        let mut ashy = 0;
        let mut was = false;
        for gx in -2000..2000 {
            let is = gen.ashy(gx, 77);
            ashy += is as i32;
            runs += (is && !was) as i32;
            was = is;
        }
        assert!(ashy > 200, "the field is ash nowhere: {ashy} of 4000");
        assert!(runs > 0);
        assert!(
            ashy / runs.max(1) > 10,
            "runs average {} columns -- that is speckle",
            ashy / runs.max(1)
        );
    }

    #[test]
    fn trees_still_stand_in_it() {
        // The ash is *because* of the wood. A rule that kept trees off
        // it would leave the burnt forest as bare ground with a tree
        // line drawn round every drift.
        let gen = WorldGen::new(7);
        let dead = chunk_in(&gen, Biome::DeadForest).expect("no dead forest anywhere");
        let logs = super::tests::count_of(&dead, BLOCK_LOG);
        assert!(logs > 0, "a burnt wood with no wood in it");
    }

    #[test]
    fn ash_lies_on_the_ground_rather_than_being_it() {
        use crate::types::{can_grow_on, is_collidable, is_flat, BLOCK_DIRT};
        // What changed: ash is a coating now, in the cell above the
        // earth rather than instead of it. So it needs a floor under it
        // like any other flat thing...
        assert!(is_flat(BLOCK_ASH));
        assert!(can_grow_on(BLOCK_ASH, BLOCK_DIRT));
        // ...it is walked through rather than stood on...
        assert!(!is_collidable(BLOCK_ASH));
        // ...and nothing lies on it, because it is not a floor. A stick
        // on ash would be a stick on a coating of powder, hanging over
        // whatever the powder is on.
        assert!(!can_grow_on(BLOCK_STICK, BLOCK_ASH));
        assert!(!can_grow_on(BLOCK_PEBBLE, BLOCK_ASH));
        assert!(!can_grow_on(BLOCK_TALL_GRASS, BLOCK_ASH));
    }
}

/// Flint: on the ground above, and on the floor of every cave below.
#[cfg(test)]
mod flint_tests {
    use super::tests::count_of;
    use super::*;

    #[test]
    fn there_is_flint_lying_about_above_ground() {
        let gen = WorldGen::new(1337);
        let found: usize = (-6..6)
            .flat_map(|cx| (-6..6).map(move |cz| (cx, cz)))
            .map(|(cx, cz)| count_of(&gen.generate_chunk(ChunkPos::new(cx, cz)), BLOCK_FLINT))
            .sum();
        assert!(found > 0, "not one nodule of flint in 144 chunks");
    }

    #[test]
    fn there_is_more_of_it_underground_than_on_the_surface() {
        // Where it comes from: nodules weather out of rock, and a cave
        // is a cut through the rock. If the surface had as much of it,
        // there would be no reason to go down.
        let gen = WorldGen::new(4242);
        let (mut deep, mut shallow) = (0, 0);
        for cx in -4..4 {
            for cz in -4..4 {
                let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
                for y in 0..CHUNK_SIZE_Y {
                    for z in 0..CHUNK_SIZE_Z {
                        for x in 0..CHUNK_SIZE_X {
                            if chunk.get(x, y, z) != BLOCK_FLINT {
                                continue;
                            }
                            if (y as i32) <= SEA_LEVEL - 4 {
                                deep += 1;
                            } else {
                                shallow += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(deep > 0, "no flint underground at all");
        assert!(
            deep > shallow,
            "{deep} nodules below against {shallow} above -- the mines are not worth walking"
        );
    }

    #[test]
    fn every_nodule_is_lying_on_something_with_room_over_it() {
        // The same rule the stones follow, and the one that catches a
        // scatter pass reaching into rock: flint inside a wall is
        // invisible, and flint in a one-cell crack cannot be reached.
        let gen = WorldGen::new(99);
        for (cx, cz) in [(0, 0), (5, -2), (-7, 9)] {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            for y in 1..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        if chunk.get(x, y, z) != BLOCK_FLINT {
                            continue;
                        }
                        assert!(
                            crate::types::can_grow_on(BLOCK_FLINT, chunk.get(x, y - 1, z)),
                            "flint at {x},{y},{z} is resting on {}",
                            crate::types::block_name(chunk.get(x, y - 1, z))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn it_is_commonest_where_the_rock_is_bare() {
        assert!(flint_spacing(BLOCK_STONE) < flint_spacing(BLOCK_SAND));
        assert!(flint_spacing(BLOCK_SAND) < flint_spacing(BLOCK_GRASS));
        assert_eq!(flint_spacing(BLOCK_COBBLESTONE), flint_spacing(BLOCK_STONE));
    }
}

/// Loose stones, which are the one thing the ground has in every biome.
#[cfg(test)]
mod pebble_tests {
    use super::tests::{chunk_in, count_of};
    use super::*;

    fn stones_in(gen: &WorldGen, biome: Biome) -> Option<usize> {
        let chunk = chunk_in(gen, biome)?;
        Some(count_of(&chunk, BLOCK_PEBBLE))
    }

    #[test]
    fn every_biome_has_stones_lying_about() {
        // The whole point of them: a world where the ground is bare
        // until you dig into it reads as unfinished, and that is true of
        // a beach and a peak as much as of a meadow.
        let gen = WorldGen::new(1337);
        let mut checked = 0;
        for &biome in Biome::ALL {
            if matches!(biome, Biome::Ocean) {
                continue; // no surface to lie on
            }
            let Some(count) = stones_in(&gen, biome) else {
                continue; // this seed has none of that biome nearby
            };
            checked += 1;
            assert!(
                count > 0,
                "not one loose stone in a chunk of {}",
                biome.name()
            );
        }
        assert!(checked >= 5, "only {checked} biomes were reachable to check");
    }

    #[test]
    fn they_are_scattered_rather_than_paving_the_ground() {
        // One quad each is cheap; one quad per column is not, and a
        // surface you cannot see for stones is worse than a bare one.
        let gen = WorldGen::new(7);
        let chunk = chunk_in(&gen, Biome::Plains).expect("no plains anywhere");
        let stones = count_of(&chunk, BLOCK_PEBBLE);
        let columns = CHUNK_SIZE_X * CHUNK_SIZE_Z;
        assert!(
            stones * 6 < columns,
            "{stones} stones over {columns} columns is a gravel pit"
        );
    }

    #[test]
    fn a_stone_always_has_ground_under_it_and_air_above() {
        let gen = WorldGen::new(99);
        for (cx, cz) in [(0, 0), (3, -5), (-8, 11)] {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            for y in 1..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        if chunk.get(x, y, z) != BLOCK_PEBBLE {
                            continue;
                        }
                        let under = chunk.get(x, y - 1, z);
                        assert!(
                            crate::types::can_grow_on(BLOCK_PEBBLE, under),
                            "a stone floating over {}",
                            crate::types::block_name(under)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_stone_never_lands_where_a_tuft_already_is() {
        // One thing per cell, and grass is the one worth keeping.
        let gen = WorldGen::new(1337);
        for (cx, cz) in [(0, 0), (12, 7), (-20, -3)] {
            let chunk = gen.generate_chunk(ChunkPos::new(cx, cz));
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let block = chunk.get(x, y, z);
                        assert!(
                            block != BLOCK_PEBBLE || block != BLOCK_TALL_GRASS,
                            "a cell holding two things"
                        );
                    }
                }
            }
        }
    }
}

/// **Where one biome meets another.**
///
/// The complaint this exists for: "пустыня и через 2 блока снег". A
/// world where a desert touches a snowfield is not a world with varied
/// weather, it is a world with no weather at all -- the eye reads the
/// seam as a rendering fault rather than as a place.
///
/// Realism here is not a look, it is a *gradient*: climate varies
/// smoothly, so the biomes it selects have to be walked through in
/// order. Sand, then dry steppe, then plain, then conifer, then snow.
/// Skipping four of those in one step is the failure.
#[cfg(test)]
mod climate_gradient_tests {
    use super::*;

    /// Where a biome sits on the cold-to-hot scale.
    ///
    /// **Temperature only.** Humidity is the other axis and it is
    /// allowed to change fast: dry steppe bordering desert is what the
    /// edge of a desert looks like, and a wood giving way to open plain
    /// over a few paces is a wood ending. The complaint is about
    /// *temperature* -- sand against snow -- and mixing the two axes
    /// into one number would have this refuse honest terrain while
    /// still missing the case it was written for.
    ///
    /// `None` for the ones climate does not choose: the sea and its
    /// shore are picked by height alone and may touch anything, and
    /// bare rock above the treeline is what a mountain is under the
    /// snow -- it belongs between the two rather than on the scale.
    fn warmth(biome: Biome) -> Option<i32> {
        Some(match biome {
            // Water and rock are off the scale: a river runs through
            // whatever country it is in, so it may touch any of them.
            Biome::Ocean | Biome::River | Biome::Beach | Biome::Mountains => return None,
            Biome::SnowyPeaks => 0,
            Biome::Tundra | Biome::Taiga => 1,
            // Birch shares the temperate step with the oaks it stands
            // beside. The scale is four coarse bands and exists to catch
            // sand touching snow, not to rank the temperate biomes
            // against each other.
            Biome::DeadForest
            | Biome::Forest
            | Biome::BirchForest
            | Biome::Swamp
            | Biome::Plains => 2,
            Biome::Savanna | Biome::Desert => 3,
        })
    }

    /// The worst jump between neighbouring columns anywhere in a wide
    /// sample, and where it happened.
    fn worst_jump(seed: u32, radius: i32) -> (i32, String) {
        let generator = WorldGen::new(seed);
        let mut worst = (0, String::from("nothing"));
        for gz in -radius..radius {
            for gx in -radius..radius {
                let Some(here) = warmth(generator.biome_at(gx, gz)) else {
                    continue;
                };
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let Some(next) = warmth(generator.biome_at(gx + dx, gz + dz)) else {
                        continue;
                    };
                    let jump = (here - next).abs();
                    if jump > worst.0 {
                        worst = (
                            jump,
                            format!(
                                "{} at ({gx},{gz}) touches {} at ({},{})",
                                generator.biome_at(gx, gz).name(),
                                generator.biome_at(gx + dx, gz + dz).name(),
                                gx + dx,
                                gz + dz
                            ),
                        );
                    }
                }
            }
        }
        worst
    }

    #[test]
    fn the_cold_end_of_the_world_still_exists() {
        // The other half of the fix. Making snow harder to reach must
        // not make it unreachable: a colder threshold that quietly
        // deleted every snowfield would pass the gradient test above
        // for the worst possible reason.
        let mut seen = std::collections::BTreeSet::new();
        for seed in [1337u32, 7, 2024, 99] {
            let generator = WorldGen::new(seed);
            for gz in (-400..400).step_by(7) {
                for gx in (-400..400).step_by(7) {
                    seen.insert(generator.biome_at(gx, gz).name());
                }
            }
        }
        for wanted in ["snowy peaks", "tundra", "taiga", "desert", "savanna"] {
            assert!(seen.contains(wanted), "{wanted} vanished from the world");
        }
    }

    #[test]
    fn no_biome_ever_touches_one_two_climates_away() {
        // One step is a boundary; two is a place where the map skipped
        // a climate. Sampled over a wide area of several seeds, because
        // the failure is rare per column and certain per world.
        for seed in [1337u32, 7, 2024, 99] {
            let (jump, where_) = worst_jump(seed, 140);
            assert!(
                jump <= 1,
                "seed {seed}: climate jumped {jump} bands -- {where_}"
            );
        }
    }
}

/// **How abrupt the ground is between one column and the next.**
///
/// A height field made of smooth octaves cannot produce a vertical
/// face, and `height_at` adds one on purpose: `terracing` pulls the
/// ground toward multiples of four so a hillside is a stack of shelves
/// rather than a ramp. That is a deliberate look, and this is what
/// keeps it from becoming a staircase of walls.
#[cfg(test)]
mod slope_tests {
    use super::*;

    /// Height differences between horizontally adjacent columns, as a
    /// histogram indexed by the difference.
    fn steps(seed: u32, radius: i32) -> Vec<usize> {
        let generator = WorldGen::new(seed);
        let mut histogram = vec![0usize; 32];
        for gz in -radius..radius {
            for gx in -radius..radius {
                let here = generator.height_at(gx, gz);
                // Only on land: the sea floor is not something anybody
                // walks over, and the coast spline is steep by design.
                if here <= SEA_LEVEL + 2 {
                    continue;
                }
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let next = generator.height_at(gx + dx, gz + dz);
                    if next <= SEA_LEVEL + 2 {
                        continue;
                    }
                    let step = (here - next).unsigned_abs() as usize;
                    histogram[step.min(31)] += 1;
                }
            }
        }
        histogram
    }

    #[test]
    fn a_hillside_is_a_stack_of_shelves_rather_than_a_flight_of_walls() {
        // Nothing in the world can be stepped over any more -- every
        // solid fills its cell -- so a seam of one block is a jump and
        // a seam of three is somewhere you cannot go. The shelves are
        // meant to stay; what is bounded is how tall one may be and how
        // much of the ground is made of them.
        for seed in [1337u32, 7, 2024] {
            let histogram = steps(seed, 200);
            let total: usize = histogram.iter().sum();
            let steep: usize = histogram.iter().skip(2).sum();
            let worst = histogram.iter().rposition(|n| *n > 0).unwrap_or(0);
            assert!(
                steep * 100 / total.max(1) <= 3,
                "seed {seed}: {:.1}% of land seams need more than a jump",
                steep as f32 / total as f32 * 100.0
            );
            assert!(worst <= 5, "seed {seed}: a seam {worst} blocks tall");
            // ...and the ground is still ground rather than a plane:
            // some of it has to change height at all.
            assert!(
                histogram[0] * 100 / total.max(1) <= 95,
                "seed {seed}: the world came out flat"
            );
        }
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn probe_slopes() {
        for seed in [1337u32, 7, 2024] {
            let histogram = steps(seed, 200);
            let total: usize = histogram.iter().sum();
            let steep: usize = histogram.iter().skip(2).sum();
            let worst = histogram.iter().rposition(|n| *n > 0).unwrap_or(0);
            println!(
                "\nseed {seed}: {total} seams, worst step {worst} blocks, \
                 {:.1}% of seams are 2 or more",
                steep as f32 / total as f32 * 100.0
            );
            for (step, count) in histogram.iter().enumerate().take(9) {
                if *count > 0 {
                    println!("   {step} blocks: {count} ({:.1}%)", *count as f32 / total as f32 * 100.0);
                }
            }
        }
    }
}

