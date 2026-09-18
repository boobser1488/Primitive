//! The world at the scale of the Earth.
//!
//! ## What was asked
//!
//! The player asked for "the scale of biomes and everything else one to one
//! with the real world" -- a block is a metre, the equator was already at
//! its real distance -- and, asked how, chose both answers: **countries as
//! big as the Earth's** (a forest, a steppe or a desert runs for hundreds of
//! kilometres, and the country changes over hours and days of walking) **and
//! relief and things at their real size** (trees as tall as their kind,
//! rivers as wide as their order, mountains and lakes of real extent).
//!
//! The world this replaced was one scale repeated everywhere: an archipelago
//! of islands a kilometre across, to the horizon and past it, with a climate
//! whose weather turned over every seven kilometres. `scale_maps` draws it:
//! four hundred kilometres of it is an even speckle of sea and land.
//!
//! ## Five scales, laid one over another
//!
//! ```text
//! scale       size of a thing      what it decides                    where
//! planet      10 000 km            latitude                           WARMTH_BY_LATITUDE
//! continent   thousands of km      oceans, continents, the coast      basin
//! province    hundreds of km       forest or steppe country, a        rain_mix, weather_mix,
//!                                  mountain belt, a lowland           highland
//! landscape   1 to 50 km           rivers by order, lakes, uplands    EARTH_RIVERS, wide lakes, rolling
//! mosaic      100 m to 3 km        glades, groves, floodplain marsh,  mosaic_wetness, the local
//!                                  ponds, brooks, hills               climate fields, hills
//! ```
//!
//! **The slow scales choose the mix and the fast one chooses the place.** A
//! forest province is humid enough that the mosaic seldom drops a column
//! under the forest line -- a glade, a burnt patch, a meadow in a river bend
//! -- and a steppe province is dry enough that it seldom lifts one over it:
//! a grove in a hollow, a wood along a river. Between them the two meet at
//! the forest line in proportion, which is what a forest-steppe is, and its
//! width is set by how fast the slow field crosses the line.
//!
//! ## The planet's origin stays neutral
//!
//! Every slow field here is Perlin noise read at a whole-number offset in
//! noise space (`far`), so every one of them is zero at the origin of every
//! seed, as the weather already was: the planet's own origin is on a coast,
//! in middling hills -- neither a plain with no stone showing nor a glacier
//! -- and in forest-steppe, where there are trees and open ground both
//! within a minute's walk.
//!
//! **That point is forty-five degrees north and it is one point, not one
//! per world** (`worldgen::PLANET_ORIGIN_DEGREES`). It used to be every
//! world's origin, because a world laid in the tropics was the same fields
//! read at the same coordinates with a different latitude term over them --
//! and that is exactly what made two zones of a seed one landscape instead
//! of two places. A world is cut from where it actually sits on the globe
//! now, so only a temperate world's origin is the neutral one, and the
//! others buy the same three promises back by hand: `meridian_with_land`
//! slides a world along its parallel until there is land under it, and
//! `WorldGen::spawn_quality` asks for the zone's climate band. What is far
//! from the planet's origin is decided by the seed; what is at it is what
//! the game was built on.
//!
//! ## Oceans, and a coast a player can reach
//!
//! **Chosen: a basin field thousands of kilometres across, multiplied up and
//! added to the kilometre-scale coast field** (`basin`, `BASIN_GAIN`). Where
//! the basin is far from zero the coast field cannot cross the waterline at
//! all -- open ocean or unbroken land -- and within a few tens of kilometres
//! of its zero the coast field draws the shore it always drew: bays,
//! headlands, islands and inlets, crisp at the waterline, because the steep
//! coast spline still does the crossing. So an ocean is thousands of
//! kilometres of water, a continent thousands of kilometres of land, and the
//! coast between them a belt of islands and bays a raft can explore.
//!
//! Rejected:
//! * *The old field alone.* An archipelago of kilometre islands everywhere
//!   is not a planet, and no walk ever leaves the coast.
//! * *An Earth-sized field with no coast field in it.* A field that turns
//!   over every thousand kilometres crosses the steep part of the coast
//!   spline over tens of kilometres, and a band of ground that wide within a
//!   block of the waterline is exactly the speckled marsh the spline was
//!   built to cure (see the module note of `worldgen`).
//! * *Dividing the field by its gradient to get a true distance to the
//!   coast.* Correct, and five evaluations of a six-octave field for every
//!   column of the world.
//!
//! ## Mountains: Earth's width, folded into the height the world has
//!
//! A mountain country is as wide as the Earth's: a belt about a hundred
//! kilometres across and a thousand long, with uplands and basins hundreds
//! of kilometres wide beside it (`highland`). Its heights are not: the world
//! is 256 blocks tall with the sea at 64, and a peak of four kilometres does
//! not fit in a hundred and ninety. **The ridges and valleys inside a range
//! keep the spacing their height wants** -- the ridge field is unchanged,
//! only its amplitude grows with the country -- so a slope in the high
//! country is as steep as a real one, and a range is a range to climb.
//!
//! Rejected:
//! * *Real ridge spacing under folded height.* A hundred and fifty blocks
//!   of relief spread over ten kilometres between ridges is a slope of one
//!   degree: a plain that happens to be high.
//! * *A taller world.* `CHUNK_SIZE_Y` from 64 to 256 doubled generation
//!   (2.25 to 4.51 ms a chunk) and more than doubled meshing (48 to 113 ms
//!   for 64 chunks), and four times more would still not fit a range of real
//!   height. Memory came back only through section packing, which a world of
//!   tall mountains would spend again.
//!
//! ## Old worlds keep their country
//!
//! A save is the player's edits, not chunks (`edits.bin`): every chunk is
//! regenerated from the seed whenever it is needed. A new scale under an old
//! save would put every house a player built into the side of a hill that
//! was not there. So the scale is written into the world like the seed, the
//! preset and the zone (`Scale`), a world that never wrote one is
//! `Scale::Regional`, and the regional world is drawn by exactly the
//! arithmetic it was drawn by before this existed: every hook below answers
//! the old number, not an approximation of it.

use std::cell::RefCell;

use noise::{NoiseFn, Perlin};

use crate::types::{BLOCK_GRAVEL, BLOCK_SAND, BLOCK_STONE};

use super::{
    fbm, hash2, smoothstep, spline, Biome, Lake, Surface, TileStore, WorldGen, CONTINENT_SPLINE, FREEZING, LAKE_CELL, LAKE_MAX_RADIUS,
    LAKE_MIN_WATER, LAKE_WOBBLE, OLD_TRUNK_SHORTEST, OLD_TRUNK_TALLEST, RIVER_DEPTH, RIVER_HALF_WIDTH,
    SEA_LEVEL,
};

/// Which scale a world's country is drawn at.
///
/// **Written beside the seed, the preset and the zone** -- in `world.toml`,
/// in the server's settings and in the handshake -- for their reason: the
/// same edits over a world drawn at another scale are the same buildings on
/// ground that never existed. See the module note.
///
/// Serialised by name, as the others are: a person reads `world.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// The world as 1.5.0 drew it: one scale of country everywhere, an
    /// archipelago of islands a kilometre across under a climate whose
    /// weather turns over every seven kilometres. Every world made before
    /// the Earth scale is this one, so its saved buildings stay on the
    /// ground they were built on.
    Regional,
    /// Oceans and continents thousands of kilometres across, forest and
    /// steppe provinces and mountain belts hundreds, rivers by order, lakes,
    /// and trees as tall as their kind. Every new world.
    #[default]
    Earth,
}

impl Scale {
    /// Every scale, oldest first.
    pub const ALL: &'static [Scale] = &[Scale::Regional, Scale::Earth];

    /// What `world.toml` and `settings.toml` call it.
    pub fn name(self) -> &'static str {
        match self {
            Scale::Regional => "regional",
            Scale::Earth => "earth",
        }
    }

    /// The scale a configuration file names, or `None`. Case-insensitive.
    pub fn parse(name: &str) -> Option<Scale> {
        Scale::ALL.iter().copied().find(|scale| name.eq_ignore_ascii_case(scale.name()))
    }

    /// The scale of a world that never wrote one down: every such world
    /// was made before the Earth scale existed. For `#[serde(default)]` on a
    /// saved field, where `Scale::default` -- the scale of a *new* world --
    /// would be the wrong answer about an old one.
    pub fn unrecorded() -> Scale {
        Scale::Regional
    }
}

/// Fractal noise for the planet's slow fields, read at a whole-number offset
/// in noise space.
///
/// `spacing` is the lattice spacing of the first octave in blocks; a lobe of
/// Perlin noise is about half of it. Each octave is shifted by its own whole
/// number of lattice cells, so the octaves do not all pass through one point
/// the way a plain `fbm` does, **and every one of them is still zero at the
/// origin** -- which is what keeps a new player's country the zone's own. See
/// the module note.
///
/// **Each octave turned about the origin by its own angle**, and the first
/// pictures are why. The noise's gradients include the four axis directions,
/// so along a lattice line every octave is nearly zero -- and an octave
/// shifted by whole cells has a lattice line through the origin along both
/// world axes, in every octave at once. A great river drawn off such a field
/// ran dead straight along z = 0 for fifty kilometres through every spawn.
/// Turned, the octaves' lattice lines cross the axes and each other at
/// angles, and the origin -- the one point a turn leaves where it was -- is
/// still a lattice point of every one of them.
///
/// Rejected: a new `Perlin` for every slow field. The generator reads the
/// fields it already has at offsets for the same reason `burnt` and
/// `fertility_at` do, and the permutation table repeats every 256 cells, so
/// a slow field read at an offset shares nothing a player could see with the
/// fast one on the same table.
fn far(noise: &Perlin, gx: i32, gz: i32, spacing: f64, octaves: u32, persistence: f64, lattice: f64) -> f64 {
    let (x, z) = (f64::from(gx) / spacing, f64::from(gz) / spacing);
    let (mut sum, mut amplitude, mut total, mut stretch) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..octaves {
        let shift = lattice + f64::from(octave) * 31.0;
        let (x, z) = turned(x, z, octave);
        sum += noise.get([x * stretch + shift, z * stretch - shift]) * amplitude;
        total += amplitude;
        amplitude *= persistence;
        stretch *= 2.0;
    }
    (sum / total).clamp(-1.0, 1.0)
}

/// The sine and cosine of each octave's turn, `0.61 + 1.37 * octave` radians,
/// written out. The turn is asked of every octave of every slow field at every
/// lattice point and of every river sample, and a `sin_cos` of a constant is a
/// constant; `the_turns_are_the_angles_they_say_they_are` holds the table to
/// the formula.
const TURNS: [(f64, f64); 6] = [
    (0.5728674601004813, 0.8196480178454796),
    (0.9174379552818098, -0.397878873789916),
    (-0.20690197167339974, -0.9783616785819341),
    (-0.9999710363300245, 0.0076109461341487905),
    (-0.19198591672995416, 0.981397680747901),
    (0.9233879612755193, 0.3838680410915191),
];
const _: () = assert!(BASIN_OCTAVES as usize <= TURNS.len(), "a slow field with more octaves than turns");

/// A point turned about the origin by an octave's own angle. See `far`: no
/// two octaves share an angle, and none is a multiple of a right angle.
fn turned(x: f64, z: f64, octave: u32) -> (f64, f64) {
    let (sin, cos) = TURNS[octave as usize];
    (x * cos - z * sin, x * sin + z * cos)
}

// ---- the slow fields, remembered ----

/// Which slow field a lattice point holds, by index.
const SLOW_BASIN: usize = 0;
const SLOW_HIGHLAND: usize = 1;
const SLOW_WEATHER: usize = 2;
const SLOW_RAIN: usize = 3;
const SLOW_ROLLING: usize = 4;

/// How far apart the slow fields are sampled, how many of those steps a
/// remembered tile is on a side, and how many tiles a thread keeps.
///
/// **Sampled on a lattice and read in between, because they are slow.** The
/// first Earth-scale generator read every slow field at every column, the
/// basin twice over, and `what_each_country_costs_to_generate` measured it in
/// release at 7.8 to 10.6 ms a chunk against 2.7 to 3.4 before: a quarter of a
/// hundred octaves of noise per column to learn which ocean, province and
/// mountain country it is in, when the answer does not change across a
/// kilometre. The finest of them is the rolling land, whose smallest octave
/// is a kilometre and a half; read linearly between points sixty-four blocks
/// apart it is out by a few hundredths of a block, and every other field by
/// less. A lattice point is the field itself, so the origin is still exactly
/// where every field is zero, and two chunks read one column the same way
/// because the points it lies between are a function of the world alone.
///
/// **Tiles of a quarter of a kilometre**, not a kilometre: a tile is built
/// the first time anything in it is asked, and the tests and tools that
/// sample the planet a few kilometres apart ask each tile once. At a
/// kilometre a side that was 289 points for every sample; at a quarter, 25.
///
/// Rejected: the slow fields once per chunk, at its middle. Two chunks would
/// then disagree about the columns along their seam -- a coast or a
/// province boundary stepping sideways at every chunk border.
const SLOW_STEP: i32 = 64;
const SLOW_CELLS: i32 = 4;
const SLOW_POINTS: i32 = SLOW_CELLS + 1;
const SLOW_TILES_KEPT: usize = 64;

/// One tile's lattice of slow fields: `SLOW_POINTS` a side, row by row, the
/// far edge included so a tile never has to ask its neighbour.
struct SlowTile {
    points: Box<[[f64; 5]]>,
}

thread_local! {
    static SLOW_TILES: RefCell<TileStore<SlowTile>> = RefCell::new(TileStore::<SlowTile>::new(SLOW_TILES_KEPT));
}

impl WorldGen {
    /// Every slow field at a column, read from the noise: what a lattice
    /// point holds. See `SLOW_STEP`.
    fn slow_exact(&self, gx: i32, gz: i32) -> [f64; 5] {
        let upland = far(&self.erosion_noise, gx, gz, UPLAND_SPACING, 3, 0.5, 1_777.0);
        let line = far(&self.ridge_noise, gx, gz, BELT_SPACING, 3, 0.5, 2_113.0);
        let belt = 1.0 - smoothstep(0.0, BELT_HALF_WIDTH, (line + BELT_LINE).abs());
        let mut fields = [0.0; 5];
        fields[SLOW_BASIN] = far(&self.continent_noise, gx, gz, BASIN_SPACING, BASIN_OCTAVES, BASIN_PERSISTENCE, 1_331.0);
        fields[SLOW_HIGHLAND] = (HIGHLAND_AT_HOME + UPLAND_WEIGHT * upland + BELT_WEIGHT * belt).clamp(0.0, 1.0);
        fields[SLOW_WEATHER] = far(&self.temperature_noise, gx, gz, WEATHER_SPACING, 3, 0.5, 3_301.0);
        fields[SLOW_RAIN] = far(&self.humidity_noise, gx, gz, RAIN_SPACING, 3, 0.5, 4_409.0);
        fields[SLOW_ROLLING] = far(&self.hill_noise, gx, gz, ROLLING_SPACING, 3, 0.5, 977.0);
        fields
    }

    /// Every slow field at a column, read between the lattice points round
    /// it. Only ever asked by an Earth-scale world: a regional one answers
    /// its constants before it gets here.
    fn slow(&self, gx: i32, gz: i32) -> [f64; 5] {
        let side = SLOW_STEP * SLOW_CELLS;
        let (tx, tz) = (gx.div_euclid(side), gz.div_euclid(side));
        let (lx, lz) = (gx.rem_euclid(side), gz.rem_euclid(side));
        let (cx, cz) = (lx / SLOW_STEP, lz / SLOW_STEP);
        let fx = f64::from(lx % SLOW_STEP) / f64::from(SLOW_STEP);
        let fz = f64::from(lz % SLOW_STEP) / f64::from(SLOW_STEP);
        SLOW_TILES.with(|store| {
            let mut store = store.borrow_mut();
            // Built under the borrow: a lattice point is noise and nothing
            // else, and never asks this store.
            let tile = store.get_or_insert((self.key(), tx, tz), || {
                let mut points = Vec::with_capacity((SLOW_POINTS * SLOW_POINTS) as usize);
                for pz in 0..SLOW_POINTS {
                    for px in 0..SLOW_POINTS {
                        points.push(self.slow_exact(tx * side + px * SLOW_STEP, tz * side + pz * SLOW_STEP));
                    }
                }
                SlowTile { points: points.into_boxed_slice() }
            });
            let at = |px: i32, pz: i32| tile.points[(pz * SLOW_POINTS + px) as usize];
            let (a, b, c, d) = (at(cx, cz), at(cx + 1, cz), at(cx, cz + 1), at(cx + 1, cz + 1));
            std::array::from_fn(|i| {
                let near = a[i] + (b[i] - a[i]) * fx;
                let far_row = c[i] + (d[i] - c[i]) * fx;
                near + (far_row - near) * fz
            })
        })
    }
}

// ---- oceans and continents ----

/// The lattice spacing of the basin field: lobes of ocean and continent
/// about fifteen hundred kilometres across, down to a finest octave of about
/// fifty. Persistence a little over a half, because the Earth's coasts have
/// gulfs and peninsulas at every scale and a half makes the middle scales too
/// faint to bend a coast.
const BASIN_SPACING: f64 = 3_000_000.0;
const BASIN_OCTAVES: u32 = 6;
const BASIN_PERSISTENCE: f64 = 0.55;

/// How much of the continent field the basin is, per unit of it.
///
/// **This number is the width of the coast.** The coast field runs about
/// ±0.7, so the islands and bays it draws reach as far from the basin's zero
/// as the basin takes to climb 0.7 / gain. Forty-five was tried first, and
/// `scale_maps` drew a belt of skerries forty kilometres deep on both sides
/// of every coast: the old archipelago with a continent behind it. Ninety and
/// a hundred and sixty still drew skerries ten to twenty kilometres deep
/// along every coast of the maps -- and the origin is always on a coast, so
/// the whole forty-kilometre country round a new player was islands. At four
/// hundred the belt is a few kilometres: bays, headlands and islands off a
/// mainland shore, and everything further out is open sea or unbroken land.
const BASIN_GAIN: f64 = 400.0;

/// How far the coast leans toward the land, in units of the continent field:
/// the origin, where the basin is zero, lies on the mainland side of its
/// coast rather than in the middle of the islands -- a new player's first
/// walk is along a shore with bays in it, not from one islet to the next.
const COAST_LEAN: f64 = 0.2;

/// The furthest into the coast spline an Earth-scale continent reads, before
/// its interior rises by the basin instead.
///
/// **The land of the old world stood on the lower half of the spline**,
/// because the kilometre field was seldom far over zero: sixty-seven to
/// seventy-four over the sea for most of it, and every rule written against
/// height -- the mountain line, the snow line, where the rivers fade -- was
/// measured on that. A continent carries the field far past one, and read
/// straight every column of its interior stood at eighty-one: a fifth of a
/// temperate continent went over the mountain line and into snow.
///
/// **0.35 was tried next and left a sixth of it there**, against a
/// sixteenth before the Earth's scale. The old land was lower than its spline
/// said as well as low on it: every island's relief was damped along its
/// coast (`land` in `terrain_height`), and an archipelago is nothing but
/// coast. A continent's interior has no coast to damp it, so it stands a
/// little lower on the spline to stand as high as the old land did.
const INLAND_CAP: f64 = 0.2;

/// How the dryness of the interior is split between the coast and the
/// continent, and how far into a continent its middle is, in units of the
/// basin.
///
/// **The kilometre coast field alone saturates twenty kilometres inland**:
/// read the way a regional world reads it, every column of a thousand-
/// kilometre continent past its coastal belt was as far from the sea as
/// anything could be, and the first maps were a tropics that was sixty per
/// cent desert and a temperate zone with three per cent of its land in woods.
/// So the coast field keeps half of the share -- a coast is wetter than the
/// ground a few kilometres behind it -- and the basin carries the rest over
/// hundreds of kilometres, which is how far rain actually carries inland.
const COAST_SHARE: f64 = 0.5;
const INTERIOR_SHARE: f64 = 0.35;
const INTERIOR_SPAN: f64 = 0.25;

/// How much higher the deep interior of a continent stands than its coastal
/// plain, and how much deeper the open ocean is than its shelf, in blocks.
/// The continent spline flattens past ±1, which the kilometre-scale field
/// never left; the basin carries the field far past it, and without these a
/// continent a thousand kilometres wide would be one plateau at the height
/// of its coastal hills, and an ocean one plain at the depth of the shelf's
/// foot.
///
/// **Two blocks, not nine.** The lapse rate here is many times the air's
/// -- it is what puts snow on a hill -- and nine blocks of interior under the
/// rolling uplands put a fifth of a temperate continent into tundra; three
/// still left a tenth of it there.
const INTERIOR_RISE: f64 = 2.0;

/// How far the open ocean falls below the foot of the shelf, in blocks,
/// and how quickly it gets there.
///
/// **"Сделай океан глубже".** Measured before this
/// (`how_deep_the_sea_is`): swimming straight out from a shore and going
/// on for two hundred kilometres, the deepest water in the way was
/// **twenty-two blocks** on one seed and **six** on another. The drop was
/// fourteen blocks and it was spread over `continent` from -1 to -14 --
/// a range the field only reaches in the middle of a true basin, which is
/// a thousand kilometres from anywhere a player starts. So the number
/// existed and nobody could ever swim to it.
///
/// It is thirty now, and it is spent between -0.30 and -3.0: just past the
/// shelf break, which is where a real sea floor falls away and where this
/// one was flat. Near the coast nothing changes -- the surf, the beach and
/// the shelf are the spline's own shallow end and untouched -- so what
/// this buys is that swimming *out* stops being a walk on a wet meadow.
///
/// Deep water is dark water: sunlight is worth two levels a block down
/// here, so the floor of the open ocean is black, and a diver who means to
/// reach it is a diver who brought a light and watched their breath. That
/// is the point of it.
///
/// **Only at the Earth's scale.** A regional world keeps the sea it was
/// made with, which is what `Scale` is for.
const ABYSS_DROP: f64 = 30.0;
const ABYSS_FROM: f64 = -0.30;
const ABYSS_TO: f64 = -3.0;

impl WorldGen {
    /// Which scale this world is drawn at. See `Scale`.
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// The ocean basins and the continents between them: 0 on the macro
    /// coast, positive inland. Always 0 in a regional world.
    pub(super) fn basin(&self, gx: i32, gz: i32) -> f64 {
        if self.scale == Scale::Regional {
            return 0.0;
        }
        self.slow(gx, gz)[SLOW_BASIN]
    }

    /// What the basin adds to the continent field: the basin multiplied up
    /// (`BASIN_GAIN`) and a lean toward the land (`COAST_LEAN`) at the
    /// Earth's scale, and nothing at all in a regional world.
    pub(super) fn basin_lift(&self, basin: f64) -> f64 {
        match self.scale {
            Scale::Regional => 0.0,
            Scale::Earth => basin * BASIN_GAIN + COAST_LEAN,
        }
    }

    /// How far inland a column is, for the rain: -1 out at sea to +1 in the
    /// deep interior. A regional world reads the continent field clamped, as
    /// it always did; the Earth's splits it between the coast and the basin.
    /// See `COAST_SHARE`.
    pub(super) fn inland(&self, gx: i32, gz: i32, basin: f64) -> f64 {
        let coast = self.continent_on(gx, gz, basin).clamp(-1.0, 1.0);
        match self.scale {
            Scale::Regional => coast,
            Scale::Earth => COAST_SHARE * coast + INTERIOR_SHARE * (basin / INTERIOR_SPAN).clamp(-1.0, 1.0),
        }
    }

    /// The height of the ground before any relief: the coast spline, read
    /// exactly as it always was in a regional world. At the Earth's scale the
    /// land reads it no further than `INLAND_CAP` and rises into a continent's
    /// interior by the basin instead, and the open ocean falls past the
    /// spline's deep end.
    pub(super) fn coast_profile(&self, continent: f64, basin: f64) -> f64 {
        match self.scale {
            Scale::Regional => spline(CONTINENT_SPLINE, continent),
            Scale::Earth if continent >= 0.0 => {
                spline(CONTINENT_SPLINE, continent.min(INLAND_CAP)) + INTERIOR_RISE * smoothstep(0.02, 0.3, basin)
            }
            Scale::Earth => {
                spline(CONTINENT_SPLINE, continent)
                    - ABYSS_DROP * smoothstep(ABYSS_FROM, ABYSS_TO, continent)
            }
        }
    }
}

// ---- islands in the open ocean ----

/// Side of the square one island group is drawn in, in blocks.
///
/// **The open ocean was empty.** Measured (`how_real_the_water_is`): over
/// three hundred thousand square kilometres of water deeper than twenty-five
/// blocks round two seeds, not one sample of dry land further than three
/// kilometres from a coast. The coast field draws islands, but only within a
/// few kilometres of a continent's shore (`BASIN_GAIN`), so a raft that left
/// the shelf left everything: thousands of kilometres of the same water, and
/// no reason to have gone.
///
/// Sixteen kilometres, so a group is a day's sailing from the next group
/// when the ocean has any, and most of the ocean has none.
const ISLAND_CELL: i32 = 16_384;

/// How deep the open ocean has to be where a group is centred, as the
/// continent field without the islands: well out over the abyss, past the
/// point where `ABYSS_DROP` has most of the way to its floor.
///
/// **This is the whole of what keeps islands away from where a player
/// wakes.** A new player's origin is lowland within a walk of the sea
/// (`meridian_with_land`), which is a continent's coast, and the field is
/// this far under the waterline only kilometres out past its shelf -- so an
/// island is a voyage, never the view from the first beach. The spawn search
/// refuses island ground as well (`spawn_quality`), so if a seed ever put
/// both within reach the player still wakes on the mainland.
const ISLAND_DEEP: f64 = -2.5;

/// How many cells in `ISLAND_ROLL` hold an archipelago, and how many more a
/// lone island: one in ten and one in six.
const ISLAND_ROLL: u32 = 60;
const ARCHIPELAGO_ROLLS: u32 = 6;
const LONE_ROLLS: u32 = 10;

/// How far from its group's centre an archipelago's islands are strewn.
const ARCHIPELAGO_SPREAD: f64 = 2_500.0;
/// The most islands in one group.
const ARCHIPELAGO_MOST: usize = 9;

/// The smallest and largest an island's land reaches from its centre, in
/// blocks: a lone island eighty metres to half a kilometre across, one of an
/// archipelago's forty to three hundred. Rolled as a square, so most are
/// small.
const LONE_RADII: (f64, f64) = (40.0, 260.0);
const ARCHIPELAGO_RADII: (f64, f64) = (20.0, 150.0);

/// How far an island's shore wanders in and out of its radius, as a share
/// of it. Enough that no island is a disc and a small one can pinch into
/// two; the field turns over a few times round each island, so a big one
/// has bays and headlands and a small one a lopsided shape.
const ISLAND_WOBBLE: f64 = 0.35;

/// How far out from an island's shore its slope reaches the abyss, in
/// blocks: forty and three fifths of its radius. **A seamount, not a
/// shelf**: a continent's shelf is kilometres of shallow water, and an
/// ocean island rises straight off the deep floor -- a player swimming off
/// its beach is out of their depth in a stone's throw, and a diver sees the
/// slope fall away into the dark.
fn island_slope(radius: f64) -> f64 {
    40.0 + radius * 0.6
}

/// The deepest the island slope reads, as the continent field: past the far
/// end of `ABYSS_TO`, so the slope meets the floor rather than a step.
const ISLAND_FOOT: f64 = -3.2;

/// One island: where, how far its land reaches, and how high the continent
/// field stands at its middle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Island {
    pub x: f64,
    pub z: f64,
    pub radius: f64,
    pub top: f64,
}

/// An accepted group: its islands, the first `count` of the array.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct IslandGroup {
    pub islands: [Island; ARCHIPELAGO_MOST],
    pub count: usize,
}

/// How many island groups one thread remembers: each is sixteen kilometres
/// on a side, so this is a band of ocean wider than anybody sails in a
/// sitting.
const ISLAND_GROUPS_KEPT: usize = 64;

thread_local! {
    static ISLAND_GROUPS: RefCell<TileStore<Option<IslandGroup>>> =
        RefCell::new(TileStore::<Option<IslandGroup>>::new(ISLAND_GROUPS_KEPT));
}

/// How far from its group's centre anything of a group can reach: the
/// spread, and the largest island's land and slope past it.
fn island_group_reach(archipelago: bool) -> f64 {
    let (spread, largest) = if archipelago { (ARCHIPELAGO_SPREAD, ARCHIPELAGO_RADII.1) } else { (0.0, LONE_RADII.1) };
    let land = largest * (1.0 + ISLAND_WOBBLE);
    spread + land + island_slope(largest) + 2.0
}

impl WorldGen {
    /// The continent field with no island in it: the coast field and the
    /// basin. What an island is judged against, and what `continent_on` was.
    pub(super) fn open_sea(&self, gx: i32, gz: i32, basin: f64) -> f64 {
        let (x, z) = self.warped(gx, gz);
        fbm(&self.continent_noise, x, z, 0.0009, 2) + self.basin_lift(basin)
    }

    /// Where a cell's group would be and what kind it is, from hashes alone:
    /// `None` for the cells that have none, which is most of them.
    fn island_candidate(&self, cell_x: i32, cell_z: i32) -> Option<(i32, i32, bool)> {
        let salt = self.seed.wrapping_add(0x151A_4D00);
        let roll = hash2(cell_x, cell_z, salt) % ISLAND_ROLL;
        let archipelago = roll < ARCHIPELAGO_ROLLS;
        if !archipelago && roll >= ARCHIPELAGO_ROLLS + LONE_ROLLS {
            return None;
        }
        let margin = island_group_reach(true).ceil() as i32 + 2;
        let play = (ISLAND_CELL - 2 * margin) as u32;
        let x = cell_x * ISLAND_CELL + margin + (hash2(cell_x, cell_z, salt ^ 0x0C0A) % play) as i32;
        let z = cell_z * ISLAND_CELL + margin + (hash2(cell_z, cell_x, salt ^ 0x5EA5) % play) as i32;
        Some((x, z, archipelago))
    }

    /// The group of a cell, if its centre is over the abyss; remembered per
    /// thread, like a lake.
    fn island_group(&self, cell_x: i32, cell_z: i32, (x, z, archipelago): (i32, i32, bool)) -> Option<IslandGroup> {
        let key = (self.key(), cell_x, cell_z);
        let known = ISLAND_GROUPS.with(|store| store.borrow().tiles.get(&key).copied());
        if let Some(group) = known {
            return group;
        }
        let group = (self.open_sea(x, z, self.basin(x, z)) < ISLAND_DEEP).then(|| {
            let salt = self.seed.wrapping_add(0x1517_0000) ^ hash2(cell_x, cell_z, 0x7A11);
            let count = if archipelago { 4 + (hash2(cell_x, cell_z, salt) % 6) as usize } else { 1 };
            let (smallest, largest) = if archipelago { ARCHIPELAGO_RADII } else { LONE_RADII };
            let mut islands = [Island::default(); ARCHIPELAGO_MOST];
            for (i, island) in islands.iter_mut().enumerate().take(count) {
                let n = i as i32;
                let unit = |bits: u32| f64::from(hash2(n, bits as i32, salt) % 10_000) / 10_000.0;
                let (angle, out) = (unit(1) * std::f64::consts::TAU, unit(2).sqrt());
                let spread = if archipelago { ARCHIPELAGO_SPREAD } else { 0.0 };
                let share = unit(3) * unit(3);
                *island = Island {
                    x: f64::from(x) + angle.cos() * out * spread,
                    z: f64::from(z) + angle.sin() * out * spread,
                    radius: smallest + (largest - smallest) * share,
                    // Bigger islands stand higher: a cay is a sandbar with
                    // grass on it and an island of half a kilometre has a hill.
                    top: 0.05 + 0.2 * share + 0.06 * unit(4),
                };
            }
            IslandGroup { islands, count }
        });
        ISLAND_GROUPS.with(|store| *store.borrow_mut().get_or_insert(key, || group))
    }

    /// The continent field an island raises at a column, or `None` where no
    /// island's slope reaches. Always `None` in a regional world.
    ///
    /// **A cone in the continent field, not a heap of blocks.** Raising the
    /// field and letting everything downstream of it read the island is what
    /// gives an island a beach, a surf, a shelf of sand and a slope into the
    /// dark, grass or palms or tundra by its latitude, hills if it is big
    /// enough to have any -- every rule that makes a coast a coast, for
    /// nothing, and all of it agreeing with itself, because `biome_from`
    /// asks the same field whether a shore is the sea's.
    ///
    /// Inside the shore the field falls from the island's top to zero -- the
    /// waterline -- and outside it on to `ISLAND_FOOT` over `island_slope`:
    /// continuous at the shore, and taken as the greater of it and the open
    /// sea, so an island never lowers anything.
    pub(super) fn island_field(&self, gx: i32, gz: i32) -> Option<f64> {
        if self.scale == Scale::Regional {
            return None;
        }
        let (cell_x, cell_z) = (gx.div_euclid(ISLAND_CELL), gz.div_euclid(ISLAND_CELL));
        let candidate = self.island_candidate(cell_x, cell_z)?;
        let reach = island_group_reach(candidate.2);
        let (dx, dz) = (f64::from(gx - candidate.0), f64::from(gz - candidate.1));
        if dx * dx + dz * dz > reach * reach {
            return None;
        }
        let group = self.island_group(cell_x, cell_z, candidate)?;
        let mut best: Option<f64> = None;
        for (i, island) in group.islands[..group.count].iter().enumerate() {
            let slope = island_slope(island.radius);
            let outer = island.radius * (1.0 + ISLAND_WOBBLE) + slope;
            let (dx, dz) = (f64::from(gx) - island.x, f64::from(gz) - island.z);
            let d2 = dx * dx + dz * dz;
            if d2 > outer * outer {
                continue;
            }
            let wobble = fbm(
                &self.lake_noise,
                f64::from(gx) + 40_000.0 + 977.0 * i as f64,
                f64::from(gz) - 40_000.0,
                2.0 / island.radius,
                2,
            );
            let shore = island.radius * (1.0 + ISLAND_WOBBLE * wobble);
            let d = d2.sqrt();
            let field = if d < shore { island.top * (1.0 - d / shore) } else { (d - shore) / slope * ISLAND_FOOT };
            best = Some(best.map_or(field, |b: f64| b.max(field)));
        }
        best
    }

    /// Whether a column's ground is an island's rather than the open sea's
    /// or a continent's: the island field stands over the field without it.
    /// For the spawn search, which wakes nobody on one, and the cold shore.
    pub(crate) fn on_island(&self, gx: i32, gz: i32) -> bool {
        self.island_field(gx, gz).is_some_and(|island| island > self.open_sea(gx, gz, self.basin(gx, gz)))
    }
}

/// How cold an island's shore has to be before it is stone and shingle
/// rather than sand, on the generator's temperature scale.
///
/// **A northern island is a rock.** Sand is what warm seas grind shells and
/// coral into and what a long shelf sorts out of a river's load; an island
/// rising off the deep floor of a cold sea has neither, and its shore is the
/// island's own stone broken by ice. So past this line an island beach is
/// gravel over stone -- which is also the place a player who sailed north
/// finds flint and stone for the axe they lost -- and a mainland beach keeps
/// the sand it always had.
const ROCKY_SHORE: f64 = FREEZING + 0.22;

impl WorldGen {
    /// An island's shore in a cold sea, or `surface` unchanged. See
    /// `ROCKY_SHORE`. Asked of beach columns only, so the island lookup and
    /// the temperature are spent on a strip of the world.
    pub(super) fn island_shore(&self, gx: i32, gz: i32, biome: Biome, surface: Surface) -> Surface {
        if self.scale == Scale::Regional || biome != Biome::Beach || surface.top != BLOCK_SAND {
            return surface;
        }
        if self.temperature(gx, gz) >= ROCKY_SHORE || !self.on_island(gx, gz) {
            return surface;
        }
        Surface { top: BLOCK_GRAVEL, filler: BLOCK_STONE, soil: 1 }
    }
}

// ---- mountain countries ----

/// How high the country is at the origin of every world: middling, the hills
/// the world always had round its spawn -- and what the regional world is
/// everywhere. See `highland`.
const HIGHLAND_AT_HOME: f64 = 0.45;

/// Uplands and lowland basins: lobes about four hundred kilometres across,
/// and how far they move the country from middling. Measured down from 0.6,
/// at which a sixth of every continent stood over the mountain line against
/// a sixteenth before the Earth's scale -- and the last thing the player
/// asked of the relief was fewer mountains.
const UPLAND_SPACING: f64 = 800_000.0;
const UPLAND_WEIGHT: f64 = 0.4;

/// Mountain belts: the line where a second slow field crosses `BELT_LINE`,
/// lobes about a thousand kilometres long, and how wide the belt is in units
/// of that field. **A line rather than a blob**, because the Earth's
/// mountains are ranges: the crossing of a smooth field is a winding line,
/// and a band either side of it is a belt a thousand kilometres long and a
/// hundred wide. Not at zero, so the origin -- where the field is zero -- is
/// never on one.
///
/// The half width was 0.07 first, and the belts were a fifth of the land.
const BELT_SPACING: f64 = 2_000_000.0;
const BELT_LINE: f64 = 0.2;
const BELT_HALF_WIDTH: f64 = 0.04;
const BELT_WEIGHT: f64 = 0.75;

/// How tall the ridges of the lowest and of the highest country are, in
/// blocks. At the origin (`HIGHLAND_AT_HOME`) the two give 76, a little under
/// the eighty the world was measured at since it went to 256 blocks -- for
/// `INLAND_CAP`'s reason: a continent has no coast to damp its relief the way
/// every island of the old archipelago had.
const RELIEF_LOWLAND: f64 = 12.0;
const RELIEF_SPAN: f64 = 142.0;

/// The rolling of the land between a hill and a province: broad rises and
/// uplands a few kilometres across, and how tall they are in the lowest and
/// the highest country.
///
/// Cut from four and twenty-four to one and a half and eight, for
/// `INTERIOR_RISE`'s reason: every block an upland stands higher is a block
/// colder, and the uplands were the white speckle across the first map of a
/// temperate continent. What is left is a rise you see across a valley, not
/// a plateau.
const ROLLING_SPACING: f64 = 6_000.0;
const ROLLING_LOW: f64 = 1.5;
const ROLLING_SPAN: f64 = 8.0;

/// Where the highest ground starts to be folded down, in blocks: over it
/// every block of height the fields ask for is half a block. The sum of every
/// term at its peak reaches past the world's ceiling; a peak clamped at
/// `MAX_HEIGHT` is a table, and a folded one is still a peak.
const CEILING_FROM: f64 = (SEA_LEVEL + 130) as f64;

impl WorldGen {
    /// How much of a mountain country this is: 0 a lowland, 1 the crest of a
    /// mountain belt, `HIGHLAND_AT_HOME` at every origin and everywhere in a
    /// regional world.
    pub(super) fn highland(&self, gx: i32, gz: i32) -> f64 {
        if self.scale == Scale::Regional {
            return HIGHLAND_AT_HOME;
        }
        self.slow(gx, gz)[SLOW_HIGHLAND]
    }

    /// How tall the ridges are in this country. Eighty in a regional world.
    pub(super) fn relief_amplitude(&self, highland: f64) -> f64 {
        match self.scale {
            Scale::Regional => 80.0,
            Scale::Earth => RELIEF_LOWLAND + RELIEF_SPAN * highland,
        }
    }

    /// How far the erosion window leans toward young ground: a mountain
    /// country has more of its land in ridges, and a lowland less. Zero in a
    /// regional world and at every origin.
    ///
    /// **Gently up and steeply down.** A lean the same both ways gains more
    /// mountain in the high country than it loses in the low, because the
    /// relief there is taller too; an upland keeps most of its meadows, and a
    /// lowland is smoothed nearly flat.
    pub(super) fn unworn_lean(&self, highland: f64) -> f64 {
        match self.scale {
            Scale::Regional => 0.0,
            Scale::Earth if highland > HIGHLAND_AT_HOME => 0.25 * (highland - HIGHLAND_AT_HOME),
            Scale::Earth => 0.6 * (highland - HIGHLAND_AT_HOME),
        }
    }

    /// The broad rises of the land: only ever up, so a lowland is a plain
    /// with uplands in it rather than a plain with basins under the sea's
    /// level, which would flood as lakes nobody dug. Zero in a regional world.
    pub(super) fn rolling(&self, gx: i32, gz: i32, highland: f64) -> f64 {
        if self.scale == Scale::Regional {
            return 0.0;
        }
        let rise = self.slow(gx, gz)[SLOW_ROLLING];
        smoothstep(-0.2, 0.6, rise) * (ROLLING_LOW + ROLLING_SPAN * highland)
    }

    /// The highest ground folded under the ceiling. See `CEILING_FROM`.
    pub(super) fn soft_ceiling(&self, height: f64) -> f64 {
        if self.scale == Scale::Regional || height <= CEILING_FROM {
            height
        } else {
            CEILING_FROM + (height - CEILING_FROM) * 0.5
        }
    }
}

// ---- climate provinces ----

/// How the weather half of the temperature is split between the provinces
/// and the local lobes, and how big a province is: lobes about eight hundred
/// kilometres across. The local lobes keep a share because a province has
/// cool hollows and warm slopes in it, and a birch wood is where they fall.
const WEATHER_LOCAL: f64 = 0.62;
const WEATHER_REGIONAL: f64 = 0.7;
const WEATHER_SPACING: f64 = 1_600_000.0;

/// The same for rainfall, with provinces about three hundred and fifty
/// kilometres across -- rain is the more local of the two on a real map, and
/// it is the whole of what tells a forest country from a steppe. The local
/// lobes keep a little over half their old weight: enough that the origin's
/// forest-steppe has woods and meadows in it, not so much that a province is
/// the old mottle with an offset.
const RAIN_LOCAL: f64 = 0.6;
const RAIN_REGIONAL: f64 = 1.0;
const RAIN_SPACING: f64 = 700_000.0;

/// The mosaic inside a province: glades in a forest, groves in a steppe.
///
/// **The cube of a field, not the field.** A field used straight moves every
/// column a little and the forest line a lot; its cube is near zero over
/// most of the ground and reaches its full weight rarely, which is what a
/// glade is -- a clearing now and then in a wood that is otherwise closed,
/// with an edge. Lobes about three hundred metres across.
const GLADE_FREQUENCY: f64 = 0.0016;
const GLADE_WEIGHT: f64 = 2.2;

/// How much wetter the ground is in a floodplain, a coastal flat or the
/// bottom of a valley than on the slope above it: where a marsh is in a
/// forest country, and where the wood is in a steppe one.
///
/// **Down from a fifth, and over a narrower band**: at a fifth every coastal
/// flat of the north was a bog, and the spawn search -- which looks for the
/// flattest dry ground near the sea -- woke all six northern test players in
/// one.
const LOWLAND_WETNESS: f64 = 0.14;

impl WorldGen {
    /// The weather half of the temperature: the local lobes it always was
    /// in a regional world, and a province under them at the Earth's scale.
    pub(super) fn weather_mix(&self, gx: i32, gz: i32, local: f64) -> f64 {
        match self.scale {
            Scale::Regional => local,
            Scale::Earth => WEATHER_LOCAL * local + WEATHER_REGIONAL * self.slow(gx, gz)[SLOW_WEATHER],
        }
    }

    /// The rainfall: the local lobes, and a province under them. See
    /// `RAIN_SPACING`.
    pub(super) fn rain_mix(&self, gx: i32, gz: i32, local: f64) -> f64 {
        match self.scale {
            Scale::Regional => local,
            Scale::Earth => RAIN_LOCAL * local + RAIN_REGIONAL * self.slow(gx, gz)[SLOW_RAIN],
        }
    }

    /// What the ground itself adds to the rain when a biome is chosen:
    /// glades and groves, and the wet bottoms of valleys. Zero in a regional
    /// world. See `GLADE_WEIGHT` and `LOWLAND_WETNESS`.
    ///
    /// **Added where the biome is chosen and nowhere else.** The tint of the
    /// leaves, the fertility of a field and the air a player feels read the
    /// rain without it: a glade is a hole in the trees, not a change of
    /// weather.
    pub(super) fn mosaic_wetness(&self, gx: i32, gz: i32, height: i32) -> f64 {
        if self.scale == Scale::Regional {
            return 0.0;
        }
        let patch = fbm(&self.ash_noise, f64::from(gx) - 90_017.0, f64::from(gz) + 60_013.0, GLADE_FREQUENCY, 2);
        let low = smoothstep(f64::from(SEA_LEVEL + 7), f64::from(SEA_LEVEL + 2), f64::from(height));
        GLADE_WEIGHT * patch * patch * patch + LOWLAND_WETNESS * low
    }
}

// ---- rivers by order ----

/// One order of river: a brook, a river or a great river.
///
/// Each is its own slow field read as a distance to its zero contour, as the
/// one river always was (`WorldGen::river`), with its own width, its own
/// valley and its own bed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RiverOrder {
    /// How fast the field turns over; the rivers of an order are about half
    /// of one over this apart.
    pub frequency: f64,
    /// Whole lattice cells the field is shifted by, so two orders are two
    /// networks. `None` for the regional river, which is read exactly as the
    /// one field it always was.
    pub lattice: Option<f64>,
    /// Half the width of the channel, banks included, in blocks.
    pub half_width: f64,
    /// Half the width of the valley the channel lies in, in blocks.
    pub valley: f64,
    /// How far under the sea the bed is cut.
    pub depth: i32,
    /// Above the sea, in blocks: no river at or over the first, all of one
    /// at or under the second.
    pub fades: (f64, f64),
    /// The meander, as a wavelength and a reach in blocks: how the field is
    /// pushed about before it is read.
    ///
    /// **A river's field turns over far slower than a river bends.** A great
    /// river drawn off a field fifty kilometres across was a straight band a
    /// hundred metres wide for as far as anyone could see, and a real one
    /// swings through loops a few kilometres long. Pushing the point the
    /// field is read at, by a field of that size, bends the channel without
    /// changing how far apart the rivers are. `None` where the field is
    /// already as small as a bend: the brook, and the regional river.
    pub meander: Option<(f64, f64)>,
    /// How fast its water runs, calm and in a rapid. See `currents`.
    ///
    /// **Per order, because size is speed.** A great river is deep and
    /// heavy and runs at a walking pace through the flattest plain, and it
    /// seldom breaks into a rapid; a river of thirty metres is lazy on the
    /// flat and white where it leaves the hills; a brook is quick and shallow
    /// and a wading player stands in it whatever it does.
    pub flow: super::currents::Flow,
}

/// The regional world's one river: every number what it was.
pub(super) const REGIONAL_RIVERS: [RiverOrder; 1] = [RiverOrder {
    frequency: 0.0021,
    lattice: None,
    half_width: RIVER_HALF_WIDTH,
    valley: RIVER_HALF_WIDTH * WorldGen::RIVER_VALLEY_SPREAD,
    depth: RIVER_DEPTH,
    fades: (80.0, 25.0),
    meander: None,
    flow: super::currents::Flow { calm: 0.6, rapid: 2.2, reach: 48.0 },
}];

/// The Earth's three orders, widest first -- the order they are cut in, so a
/// brook's channel lies in the floor of a river's valley and not the other
/// way round.
///
/// * **A great river**: about a hundred and sixty metres of water, measured
///   across it (`a_brook_a_river_and_a_great_river_are_each_as_wide_as_their_order`),
///   eight deep in its channel, in a floodplain two kilometres wide, one
///   every twenty-odd kilometres of lowland. Nobody wades it.
/// * **A river**: twenty-five to thirty-five metres and four deep, a valley
///   six hundred wide, a few kilometres apart -- a swim, and in a fast reach
///   one that carries the swimmer (`WorldGen::river_current`).
/// * **A brook**: three or four metres and a block deep, in a dip seventy
///   wide, a few hundred metres apart -- and only in low country, where
///   water gathers rather than runs off.
///
/// **Measured against the Earth, a block a metre** (`how_real_the_water_is`,
/// two seeds, 640 km of straight walks each). Before: the great river 108
/// wide and 6 deep, the river 11 wide and 3 deep, the brook **one block**
/// wide and one deep, crossed 10 to 22 times a hundred kilometres. A river
/// eleven metres across is a stream on any map, and a brook one block wide
/// is a gutter a player steps over without seeing: the hierarchy had the
/// right shape and every rung a size too small. What a real country has is a
/// stream two to five metres wide and under a metre deep, a river fifteen to
/// sixty and two to five deep, and a large river a hundred to three hundred;
/// the widths below are those, and the brook is also closer together, since
/// a walk in wet lowland crosses a stream every kilometre or two and not
/// every five.
///
/// **The great river's floodplain was five kilometres**, and it cost the
/// generator more than any other field: the slope of a river's field is only
/// skipped where the field is further from zero than the valley is wide, and
/// a valley that wide is almost everywhere. Two kilometres is still a flat
/// that takes half an hour to cross, and the cull works over most of it.
///
/// Rejected: branching networks that join. A tributary meeting its river
/// needs the land to slope toward it, and every channel here holds water at
/// the sea's level; three networks that cross is what the water model can
/// keep, and a player following one downstream still reaches wider water.
///
/// Rejected too, for the brook: deeper. A stream two blocks deep is a swim
/// in a meadow, and the ford -- the place a stream can be walked across
/// without the current taking hold of anyone -- is the decision a river is
/// for (`river_current`). A block of water is wading.
pub(super) const EARTH_RIVERS: [RiverOrder; 3] = [
    RiverOrder {
        frequency: 0.000_019,
        lattice: Some(307.0),
        half_width: 140.0,
        valley: 900.0,
        depth: 8,
        fades: (110.0, 45.0),
        meander: Some((3_500.0, 450.0)),
        flow: super::currents::Flow { calm: 0.9, rapid: 1.4, reach: 240.0 },
    },
    RiverOrder {
        frequency: 0.000_14,
        lattice: Some(211.0),
        half_width: 32.0,
        valley: 300.0,
        depth: 4,
        fades: (80.0, 25.0),
        meander: Some((900.0, 110.0)),
        flow: super::currents::Flow { calm: 0.7, rapid: 2.6, reach: 64.0 },
    },
    // **Fourteen and six, not thirty and ten.** The water of every river here
    // stands at the sea's level, so a brook up a hillside is a ditch down to
    // the sea, and a channel five columns wide cut a trench into ground
    // fifteen blocks over its bed with walls six blocks tall -- found by
    // `a_hillside_is_a_stack_of_shelves_rather_than_a_flight_of_walls`, a wall
    // across a meadow slope where people walk. A brook runs where the land is
    // within a few blocks of the water, which is where one gathers anyway.
    //
    // **Seven across its half, for three or four of water.** A brook is a
    // block deep, so its water is only the middle of the cut where the bank
    // has come all the way down: two fifths of the half width either side.
    RiverOrder {
        frequency: 0.001_8,
        lattice: Some(101.0),
        half_width: 7.0,
        valley: 36.0,
        depth: 1,
        fades: (14.0, 6.0),
        meander: None,
        flow: super::currents::Flow { calm: 0.4, rapid: 1.2, reach: 24.0 },
    },
];

/// The steepest a two-octave river field ever gets, in units of its
/// frequency. What lets a column far from every river skip the four samples
/// that measure the field's slope: the distance to the line is at least
/// `|field| / (bound * frequency)`, so a field further from zero than the
/// valley is wide times that is a column no river reaches.
///
/// Measured rather than derived -- `the_river_cull_is_steeper_than_the_river_field_ever_gets`
/// samples the field's true slope and holds this number over it. **Four was
/// the guess, and it was wrong**: the regional field reaches 4.8 somewhere
/// in two hundred thousand samples, and a cull under the true slope skips
/// columns a valley reaches -- a river ending in a straight line, in a world
/// whose ground was promised not to move. Eight since the meanders, which
/// steepen a field where they squeeze it.
pub(super) const RIVER_SLOPE_BOUND: f64 = 8.0;

impl WorldGen {
    /// The orders of river this world's land is cut by.
    pub(super) fn river_orders(&self) -> &'static [RiverOrder] {
        match self.scale {
            Scale::Regional => &REGIONAL_RIVERS,
            Scale::Earth => &EARTH_RIVERS,
        }
    }

    /// An order's river field at a point, -1..1, zero along its rivers: two
    /// octaves, each shifted by its own whole number of lattice cells so the
    /// orders are three networks and not one drawn three times. The regional
    /// river is the plain `fbm` it always was.
    pub(super) fn river_field(&self, order: &RiverOrder, x: f64, z: f64) -> f64 {
        let frequency = order.frequency;
        match order.lattice {
            None => fbm(&self.river_noise, x, z, frequency, 2),
            // Turned, for `far`'s reason: a river on a lattice line is a
            // straight line fifty kilometres long.
            Some(shift) => {
                // The meander first, off the warp field at its own whole
                // offsets -- so the origin is still where it was. See
                // `RiverOrder::meander`.
                let (x, z) = match order.meander {
                    Some((wavelength, reach)) => (
                        x + self.warp_noise.get([x / wavelength + shift, z / wavelength - shift]) * reach,
                        z + self.warp_noise.get([x / wavelength - shift - 17.0, z / wavelength + shift + 17.0]) * reach,
                    ),
                    None => (x, z),
                };
                // **Half a cell off the lattice**, where every slow field is
                // whole cells off. A whole-cell field is zero at the origin,
                // which is what a province must be and what a river must not:
                // every order's zero ran through every spawn, and the great
                // river's floodplain flattened the country round it in every
                // world -- a first day with no hill, no cliff and no copper.
                let (ax, az) = turned(x * frequency, z * frequency, 0);
                let (bx, bz) = turned(x * frequency * 2.0, z * frequency * 2.0, 1);
                let first = self.river_noise.get([ax + shift + 0.5, az - shift + 0.37]);
                let second = self.river_noise.get([bx + shift + 31.29, bz - shift - 30.55]);
                ((first + second * 0.5) / 1.5).clamp(-1.0, 1.0)
            }
        }
    }
}

// ---- ponds at the Earth's scale ----

/// Side of the square one Earth pond's site is drawn in, and the narrowest
/// and widest its bed reaches from its centre.
///
/// **A pond is tens of metres across.** The regional pond is three to six
/// from its centre, seven to thirteen blocks of water, and measured
/// (`how_real_the_water_is`) that is what every pond at the Earth's scale was
/// too: a pool a player jumps half across. A real pond -- a field pond, a
/// kettle hole, an old flooded pit -- is fifteen to fifty metres of water,
/// and so is this one, six to sixteen from its centre with its shore wobbling
/// a fifth of that in and out.
///
/// **In cells of a hundred and twenty-eight, not forty-eight**, because the
/// pond and its rim have to fit in its own cell (`lake_candidate`), and
/// because a bigger pond is not to be a more frequent one: the player said
/// the lakes were too many. See `POND_SHARE`.
pub(super) const EARTH_POND_CELL: i32 = 128;
pub(super) const EARTH_POND_SMALLEST: i32 = 6;
pub(super) const EARTH_POND_LARGEST: i32 = 16;
const EARTH_POND_SHALLOWEST: i32 = 2;
const EARTH_POND_DEEPEST: i32 = 5;
/// How far an Earth pond's shore wanders, as a share of its radius: the
/// regional two blocks on a radius of sixteen is a circle again.
const EARTH_POND_WOBBLE: f64 = 0.18;
const _: () = assert!(2 * (EARTH_POND_LARGEST + 5) < EARTH_POND_CELL, "a pond and its rim do not fit in their cell");

/// One pond cell in this many is offered a pond at the Earth's scale.
///
/// **A pond is a find again.** The regional world had sixty-seven ponds to a
/// square kilometre, and the Earth's flatter lowlands let a hundred and
/// seventeen through the same rim test (`scale_water`): a pond every ninety
/// metres, which is a marsh country everywhere rather than a meadow with
/// water in it. A third of the cells was about forty to the square
/// kilometre, and the wide lakes now carry the standing water a country has.
///
/// **A sixth was about twenty**, because forty was still "озёра слишком
/// частые": a pond every hundred and sixty metres meant a player never
/// once had to carry water or think about where to camp.
///
/// **A third again, of cells seven times the area**, since the ponds grew to
/// their real size (`EARTH_POND_CELL`): a cell a hundred and twenty-eight on a
/// side holds a pond of forty metres where one of forty-eight held one of
/// ten, and a wider rim is refused more often, so the count per square
/// kilometre fell to a few however the share was set. The number that has
/// to stay down is how often a walk meets standing water, and it did -- see
/// the CHANGELOG for the before and after.
const POND_SHARE: u32 = 3;

impl WorldGen {
    /// Whether a pond's cell is offered one at all, before its rim is read:
    /// always in a regional world, one in `POND_SHARE` at the Earth's scale.
    /// Asked first because it is a hash and the rim is forty columns of
    /// terrain.
    pub(super) fn pond_offered(&self, cell_x: i32, cell_z: i32) -> bool {
        self.scale == Scale::Regional || hash2(cell_x, cell_z, self.seed.wrapping_add(0x90D5)).is_multiple_of(POND_SHARE)
    }

    /// The side of a pond's cell at this world's scale.
    pub(super) fn pond_cell(&self) -> i32 {
        match self.scale {
            Scale::Regional => LAKE_CELL,
            Scale::Earth => EARTH_POND_CELL,
        }
    }

    /// How far a pond's shore wanders at this radius: the regional two
    /// blocks, or a share of the radius at the Earth's scale.
    pub(super) fn pond_wobble(&self, radius: i32) -> f64 {
        match self.scale {
            Scale::Regional => LAKE_WOBBLE,
            Scale::Earth => (f64::from(radius) * EARTH_POND_WOBBLE).max(LAKE_WOBBLE),
        }
    }

    /// The radii a pond is drawn between at this world's scale.
    #[cfg(test)]
    pub(super) fn pond_radii(&self) -> std::ops::RangeInclusive<i32> {
        match self.scale {
            Scale::Regional => 3..=LAKE_MAX_RADIUS,
            Scale::Earth => EARTH_POND_SMALLEST..=EARTH_POND_LARGEST,
        }
    }

    /// Where an Earth pond's cell would put its pond, before the ground is
    /// asked: the regional arithmetic with the Earth's sizes, and more small
    /// ponds than large -- the radius is the square of an even roll.
    pub(super) fn earth_pond_candidate(&self, cell_x: i32, cell_z: i32) -> (i32, i32, i32, i32) {
        let salt = self.seed.wrapping_add(0x1A4E);
        let margin = EARTH_POND_LARGEST + self.pond_wobble(EARTH_POND_LARGEST).ceil() as i32 + 2;
        let play = (EARTH_POND_CELL - 2 * margin) as u32;
        let centre_x = cell_x * EARTH_POND_CELL + margin + (hash2(cell_x, cell_z, salt) % play) as i32;
        let centre_z = cell_z * EARTH_POND_CELL + margin + (hash2(cell_z, cell_x, salt ^ 0x7B1D) % play) as i32;
        let even = f64::from(hash2(cell_x, cell_z, salt ^ 0x3A11) % 1_000) / 1_000.0;
        let radius = EARTH_POND_SMALLEST + (f64::from(EARTH_POND_LARGEST - EARTH_POND_SMALLEST) * even * even).round() as i32;
        let depth = EARTH_POND_SHALLOWEST
            + (hash2(cell_x, cell_z, salt ^ 0xD3E9) % (EARTH_POND_DEEPEST - EARTH_POND_SHALLOWEST + 1) as u32) as i32;
        (centre_x, centre_z, radius, depth)
    }
}

// ---- lakes wider than a pond ----

/// Side of the square one wide lake's site is drawn in, in blocks.
pub(super) const WIDE_LAKE_CELL: i32 = 2_048;
/// The narrowest and widest a wide lake's bed reaches from its centre.
///
/// **A lake is hundreds of metres to a kilometre across.** It was fourteen
/// to forty-eight from the centre -- thirty to a hundred metres of water,
/// measured (`how_real_the_water_is`): a pond with a name, and nothing a
/// player would call a lake on a map. Forty to four hundred and twenty is
/// eighty metres to most of a kilometre, rolled as a square so most lakes are
/// a couple of hundred metres and one in a few is a water to plan a crossing
/// round. The cell grew with it, to two kilometres: measured, that is about
/// one lake to a hundred square kilometres, a lake a day's walk apart --
/// four kilometres was one to seven hundred, and a player could live out a
/// world without seeing one.
///
/// **Wider than any pond**, which the assertion under it holds: a pond's rim
/// is refused inside a lake's footprint (`wide_lake_within`), and a lake that
/// could be smaller than a pond would be a pond judged twice.
pub(super) const WIDE_LAKE_SMALLEST: i32 = 40;
pub(super) const WIDE_LAKE_LARGEST: i32 = 420;
const _: () = assert!(WIDE_LAKE_SMALLEST > EARTH_POND_LARGEST && WIDE_LAKE_SMALLEST > LAKE_MAX_RADIUS);
/// How far under the water the middle of a wide lake's bed lies: five to
/// twenty-five, deeper with the lake -- a real lake of a few hundred metres
/// is five to thirty deep, and the old four to nine was a pond's.
const WIDE_LAKE_SHALLOWEST: i32 = 5;
const WIDE_LAKE_DEEPEST: i32 = 25;
/// How far a wide lake's shore wanders in and out of its radius, as a share
/// of it: a pond's wobble of two blocks on a radius of forty is a circle.
const WIDE_LAKE_WOBBLE: f64 = 0.22;
/// The highest the ground under a wide lake's centre may stand: lakes are a
/// lowland's and a plateau's, not a mountainside's.
const WIDE_LAKE_HIGHEST: i32 = SEA_LEVEL + 60;
/// How many wide lake sites one thread remembers. A site is two kilometres
/// on a side, so this is a band of the world far wider than anybody streams.
const WIDE_LAKE_SITES_KEPT: usize = 64;

/// How far out from a wide lake's shore its banks are eased down to the
/// water, in blocks, for a radius: twelve and a fifth of the radius.
///
/// **A lake of four hundred metres is not a pond with its rim test
/// scaled.** The rim of a pond is a ring of a hundred columns that may rise
/// three blocks, and ground that level for a hundred columns is common; a
/// ring three kilometres round that level is not, and at a pond's spread
/// almost no lake of a real size would ever be accepted. So a wide lake may
/// sit in ground that rises well over its water round it (`wide_rim_spread`)
/// -- and then its water would meet the higher side as a wall. The skirt is what makes that a shore:
/// every column within it that stands over the water is pulled toward a
/// block over it, eased over the skirt's width, the way `river` eases a
/// valley down to its floodplain. Only ever downward and never under a block
/// over the water, so no column that held the lake in stops holding it.
///
/// Rejected: filling a noise field's local hollows. Real-looking and cheap,
/// and it has no rim to prove closed -- the one thing a lake here must have,
/// because water is conserved and a lake that leaks runs into the wood on its
/// first tick.
pub(super) fn wide_skirt(radius: i32) -> i32 {
    12 + radius / 5
}

/// How far the rim of a wide lake may rise over its water and still hold
/// the lake: three quarters of the skirt, which eases a rise that size into a
/// slope of about one in one at its steepest -- a lake in hills, with a
/// steep shore on its high side, which is what a lake in hills has.
///
/// **Measured, not guessed** (`why_wide_lakes_are_refused`): round the
/// sites a temperate world offers, the ring of a lake of eighty to eight
/// hundred metres rises twenty to eighty blocks from its lowest column to its
/// highest. At a pond's three blocks -- which is what the wide lake was held
/// to -- one site in two hundred and fifty-six was accepted, and none of a
/// real size. The ground here is hills; a lake that waits for a plain a
/// kilometre across finds none.
fn wide_rim_spread(radius: i32) -> i32 {
    wide_skirt(radius) * 3 / 4
}

/// How far under a wide lake's water a few columns of its rim may lie, as a
/// sill the lake raises to its own level.
///
/// **The water is not set by the lowest column of the rim any more**, and
/// this is why. A ring kilometres round crosses a dip somewhere -- a gully, a
/// fold of a hill -- and water at the lowest column is a lake set by the one
/// low place on its shore, tens of blocks under the country round it: a pit.
/// So the water stands at the lowest tenth of the ring (`judge_wide_lake`),
/// and the columns of the ring under it, which may be no further under it
/// than this, are raised to it: a sill a block or three high, which is what
/// holds a real lake in at its outlet. Deeper than that is a valley, and a
/// lake across a valley is a dam; refused.
const WIDE_LAKE_SILL: i32 = 3;

thread_local! {
    static WIDE_LAKE_SITES: RefCell<TileStore<Option<Lake>>> =
        RefCell::new(TileStore::<Option<Lake>>::new(WIDE_LAKE_SITES_KEPT));
}

/// How far out a wide lake's rim ring is judged from, for a radius.
fn wide_edge(radius: i32) -> i32 {
    radius + (f64::from(radius) * WIDE_LAKE_WOBBLE).ceil() as i32
}

impl WorldGen {
    /// Where the wide lake of a cell would be, before the ground is asked.
    /// Kept wholly inside its cell, rim ring and skirt and all, as a pond is
    /// in its.
    ///
    /// **More small lakes than large**: the radius is the square of an even
    /// roll, so a lake of four hundred is a find and a lake of eighty is a
    /// lake. Deeper with its size, and a little either way.
    pub(super) fn wide_lake_candidate(&self, cell_x: i32, cell_z: i32) -> (i32, i32, i32, i32) {
        let salt = self.seed.wrapping_add(0x71DE_1A4E);
        let margin = wide_edge(WIDE_LAKE_LARGEST) + wide_skirt(WIDE_LAKE_LARGEST) + 2;
        let play = (WIDE_LAKE_CELL - 2 * margin) as u32;
        let centre_x = cell_x * WIDE_LAKE_CELL + margin + (hash2(cell_x, cell_z, salt) % play) as i32;
        let centre_z = cell_z * WIDE_LAKE_CELL + margin + (hash2(cell_z, cell_x, salt ^ 0x9E37) % play) as i32;
        let even = f64::from(hash2(cell_x, cell_z, salt ^ 0x51ED) % 1_000) / 1_000.0;
        let share = even * even;
        let radius = WIDE_LAKE_SMALLEST + (f64::from(WIDE_LAKE_LARGEST - WIDE_LAKE_SMALLEST) * share).round() as i32;
        let spread = WIDE_LAKE_DEEPEST - WIDE_LAKE_SHALLOWEST;
        let depth = WIDE_LAKE_SHALLOWEST
            + (f64::from(spread) * 0.7 * share).round() as i32
            + (hash2(cell_x, cell_z, salt ^ 0xDEE9) % (spread * 3 / 10 + 1) as u32) as i32;
        (centre_x, centre_z, radius, depth)
    }

    /// The wide lake of a cell, if the ground holds one; remembered per
    /// thread like a pond's. Judged exactly as a pond is -- the rim ring read
    /// before the bed exists, the water at its lowest column, and refused if
    /// the ring spreads further than `wide_rim_spread` -- so the closure is
    /// proved the same way. See `WorldGen::lake_site`.
    pub(super) fn wide_lake_site(&self, cell_x: i32, cell_z: i32) -> Option<Lake> {
        if self.scale == Scale::Regional {
            return None;
        }
        let key = (self.key(), cell_x, cell_z);
        let known = WIDE_LAKE_SITES.with(|store| store.borrow().tiles.get(&key).copied());
        if let Some(site) = known {
            return site;
        }
        let site = self.judge_wide_lake(cell_x, cell_z);
        WIDE_LAKE_SITES.with(|store| *store.borrow_mut().get_or_insert(key, || site))
    }

    fn judge_wide_lake(&self, cell_x: i32, cell_z: i32) -> Option<Lake> {
        let (cx, cz, radius, depth) = self.wide_lake_candidate(cell_x, cell_z);
        let centre = self.terrain_height(cx, cz);
        if !(LAKE_MIN_WATER..=WIDE_LAKE_HIGHEST).contains(&centre) {
            return None;
        }
        // Whatever the water turns out to be, every column of the ring has
        // to lie between a sill under it and the spread over it, so a ring
        // wider than both together is refused as soon as it is seen to be.
        let widest = wide_rim_spread(radius) + WIDE_LAKE_SILL;
        let edge = wide_edge(radius);
        let (inner, outer) = (edge * edge, (edge + 1) * (edge + 1));
        let (mut lowest, mut highest) = (i32::MAX, i32::MIN);
        let mut ring: Vec<i32> = Vec::with_capacity((7 * edge + 8) as usize);
        // Row by row, and only as far along each row as the ring reaches:
        // the square round a lake of four hundred is a million columns and
        // its ring three thousand.
        for dz in -(edge + 1)..=(edge + 1) {
            let far = f64::from(outer - dz * dz).sqrt().floor() as i32;
            for dx in -far..=far {
                let d2 = dx * dx + dz * dz;
                if d2 <= inner || d2 > outer {
                    continue;
                }
                let h = self.terrain_height(cx + dx, cz + dz);
                lowest = lowest.min(h);
                highest = highest.max(h);
                if highest - lowest > widest || lowest < LAKE_MIN_WATER - WIDE_LAKE_SILL {
                    return None;
                }
                ring.push(h);
            }
        }
        ring.sort_unstable();
        let water = ring[ring.len() / 10];
        if water < LAKE_MIN_WATER || lowest < water - WIDE_LAKE_SILL || highest - water > wide_rim_spread(radius) {
            return None;
        }
        Some(Lake { centre_x: cx, centre_z: cz, radius, depth, water, wide: true })
    }

    /// How far a wide lake's water reaches at this column, squared. See
    /// `WIDE_LAKE_WOBBLE`.
    ///
    /// **The wobble's field turns over with the lake's size**, so a lake of
    /// four hundred has bays and points a hundred metres long rather than
    /// the same forty-metre ripple a lake of forty has, which on a shore that
    /// long is a saw blade.
    pub(super) fn wide_lake_reach2(&self, radius: i32, gx: i32, gz: i32) -> i32 {
        let frequency = 0.018 * f64::from(WIDE_LAKE_SMALLEST) / f64::from(radius.max(WIDE_LAKE_SMALLEST));
        let wobble = fbm(&self.lake_noise, f64::from(gx) + 7_001.0, f64::from(gz) - 3_003.0, frequency, 2)
            * WIDE_LAKE_WOBBLE
            * f64::from(radius);
        let reach = (f64::from(radius) + wobble).max(1.0);
        (reach * reach).round() as i32
    }

    /// The wide lake whose footprint -- bed or rim ring -- this column is in,
    /// and the squared distance to its centre.
    pub(super) fn wide_lake_near(&self, gx: i32, gz: i32) -> Option<(Lake, i32)> {
        if self.scale == Scale::Regional {
            return None;
        }
        let (cell_x, cell_z) = (gx.div_euclid(WIDE_LAKE_CELL), gz.div_euclid(WIDE_LAKE_CELL));
        let (cx, cz, radius, _) = self.wide_lake_candidate(cell_x, cell_z);
        let (dx, dz) = (gx - cx, gz - cz);
        let d2 = dx * dx + dz * dz;
        let outer = wide_edge(radius) + 1;
        if d2 > outer * outer {
            return None;
        }
        self.wide_lake_site(cell_x, cell_z).map(|lake| (lake, d2))
    }

    /// The ground at a column with a wide lake's banks eased down to it, and
    /// its sill raised to its water. See `wide_skirt` and `WIDE_LAKE_SILL`.
    /// `terrain` itself everywhere no lake's shore reaches, and in a regional
    /// world.
    pub(super) fn wide_lake_shore(&self, gx: i32, gz: i32, terrain: i32) -> i32 {
        if self.scale == Scale::Regional {
            return terrain;
        }
        let (cell_x, cell_z) = (gx.div_euclid(WIDE_LAKE_CELL), gz.div_euclid(WIDE_LAKE_CELL));
        let (cx, cz, radius, _) = self.wide_lake_candidate(cell_x, cell_z);
        let (dx, dz) = (gx - cx, gz - cz);
        let d2 = dx * dx + dz * dz;
        let skirt = wide_skirt(radius);
        let edge = wide_edge(radius);
        let outer = edge + 1 + skirt;
        if d2 > outer * outer {
            return terrain;
        }
        let Some(lake) = self.wide_lake_site(cell_x, cell_z) else {
            return terrain;
        };
        // **The ring is the lake's wall**: every column of it at least the
        // water, which is the whole of why the water cannot leave. The judge
        // let no column of it lie more than a sill under.
        if d2 > edge * edge && d2 <= (edge + 1) * (edge + 1) && terrain < lake.water {
            return lake.water;
        }
        let bank = lake.water + 1;
        if terrain <= bank {
            return terrain;
        }
        let shore = f64::from(self.wide_lake_reach2(radius, gx, gz)).sqrt();
        let out = ((f64::from(d2).sqrt() - shore) / f64::from(skirt)).clamp(0.0, 1.0);
        let eased = bank + (f64::from(terrain - bank) * smoothstep(0.0, 1.0, out)).round() as i32;
        terrain.min(eased)
    }

    /// Whether any wide lake's footprint or skirt comes within `reach` of a
    /// column, on either axis. A pond that would is refused
    /// (`judge_lake_site`): a pond in a lake's rim would be judged on ground
    /// the lake then floods, and one in its skirt on ground the skirt then
    /// lowers from under the pond's own water.
    pub(super) fn wide_lake_within(&self, gx: i32, gz: i32, reach: i32) -> bool {
        if self.scale == Scale::Regional {
            return false;
        }
        for cell_z in (gz - reach).div_euclid(WIDE_LAKE_CELL)..=(gz + reach).div_euclid(WIDE_LAKE_CELL) {
            for cell_x in (gx - reach).div_euclid(WIDE_LAKE_CELL)..=(gx + reach).div_euclid(WIDE_LAKE_CELL) {
                let (cx, cz, radius, _) = self.wide_lake_candidate(cell_x, cell_z);
                let span = wide_edge(radius) + 1 + wide_skirt(radius) + reach;
                if (gx - cx).abs() > span || (gz - cz).abs() > span {
                    continue;
                }
                if self.wide_lake_site(cell_x, cell_z).is_some() {
                    return true;
                }
            }
        }
        false
    }
}

// ---- trees as tall as their kind ----

/// How tall an old tree's bole is at the Earth's scale, shortest and tallest.
///
/// **Taller than the tallest ordinary crown of its wood by a head**, for the
/// landmark's reason (`Biome::old_tree_share`): a forest oak's trunk reaches
/// thirteen with its branch height (`trunks` plus `branches::BRANCH_TALLER`)
/// and its crown two or three over that, and an old tree of sixteen to twenty
/// with its crown over it clears that wood from outside it.
///
/// **It was twenty-three to thirty**, with every oak round it at twenty, and
/// the player's words for that wood were "не все деревья высотой с 10ти
/// этажный дом". A landmark is a tree a head over its neighbours, not a tower
/// over a wood of towers; see `trunks` for the wood.
pub(super) const EARTH_OLD_TRUNK: (i32, i32) = (16, 20);

impl WorldGen {
    /// The trunk range a biome's trees grow at this world's scale:
    /// `regional`, the biome's own `tree_shape` range, in a regional world.
    ///
    /// **Heights a player reads as trees, most of them eight to sixteen to the
    /// top of the wood.** A block is a metre, and the regional trunks were four
    /// to eleven: an oak the height of a house. The Earth's scale first set
    /// them as tall as the kind grows at its tallest -- a forest oak seventeen
    /// to twenty-four metres to its crown, a fir sixteen to twenty-four -- and
    /// the wood that made was the player's "деревья высотой с 10ти этажный
    /// дом": measured (`how_tall_the_trees_stand`), the middle oak's wood
    /// topped out at nineteen, the middle fir and birch at nineteen, the middle
    /// maple at twenty-one, and a third of the oaks stood over twenty. A player
    /// is two blocks tall and sees a wood from inside it, where a crown twenty
    /// up is a ceiling nobody looks at, and the tallest trees of a kind are
    /// what the *old* trees are for (`EARTH_OLD_TRUNK`).
    ///
    /// So the ordinary trunks are what a wood's middle tree is rather than
    /// its tallest: an oak's six to ten (nine to thirteen with its branch
    /// height, and the crown over that), a birch's seven to eleven, a fir's
    /// ten to fourteen, a swamp tree's five to eight -- the shortest a crown
    /// still clears a walking player under (`a_swamp_tree_holds_its_crown_
    /// over_a_standing_player`) -- and a dead wood's snags six to eleven. The
    /// acacia keeps five to nine, which was already its height. The tundra's
    /// firs stay stunted, as the regional table has them.
    ///
    /// **Not wider.** A tree's reach is the border the tree pass walks and
    /// the width of every chunk's column cache (`OLD_TREE_REACH`,
    /// `FEATURE_MARGIN`), and a crown five columns from its trunk is what a
    /// forest-grown tree has -- the broad spreading crown is an oak alone in a
    /// field, and those are the old trees.
    ///
    /// **Only the heights.** Which trees stand where, what they are made of
    /// and how they grow from a sapling are the tree tables' and the growth
    /// rules' (`tree_spacing`, `branches::tree_stage_cells`), and a tree a
    /// player grows from a sapling is a young one: it stops at the height a
    /// sapling reaches in a lifetime, shorter than the old forest round it.
    pub(super) fn trunks(&self, biome: Biome, regional: (i32, i32)) -> (i32, i32) {
        if self.scale == Scale::Regional {
            return regional;
        }
        match biome {
            Biome::Forest | Biome::Plains => (6, 10),
            Biome::BirchForest => (7, 11),
            Biome::Swamp => (5, 8),
            Biome::Taiga => (10, 14),
            Biome::Savanna => (5, 9),
            Biome::DeadForest => (6, 11),
            _ => regional,
        }
    }

    /// An old tree's trunk range at this world's scale.
    pub(super) fn old_trunks(&self) -> (i32, i32) {
        match self.scale {
            Scale::Regional => (OLD_TRUNK_SHORTEST, OLD_TRUNK_TALLEST),
            Scale::Earth => EARTH_OLD_TRUNK,
        }
    }
}

/// Measurements of scale, and pictures of it. Nothing here asserts.
///
/// ```text
/// PRIMITIVE_SCALE_DUMP=C:/absolute/dir PRIMITIVE_SCALE_TAG=after \
///     cargo test -p primitive_shared --release --lib -- --ignored --nocapture scale::tools
/// ```
///
/// **Give `PRIMITIVE_SCALE_DUMP` an absolute directory**: a test runs in the
/// crate's own directory, and a relative path writes the pictures somewhere
/// nobody looks. `PRIMITIVE_SCALE=regional` measures a world from before the
/// Earth scale instead, which is how a regional world is checked against the
/// numbers it had.
#[cfg(test)]
mod tools {
    use super::super::*;
    use super::{wide_edge, wide_rim_spread, WIDE_LAKE_CELL, WIDE_LAKE_HIGHEST};
    use crate::types::{block_kind, is_branch, BLOCK_BIRCH_LOG, BLOCK_PALM_TRUNK};
    use std::collections::HashMap;

    fn out_dir() -> Option<String> {
        let dir = std::env::var("PRIMITIVE_SCALE_DUMP").ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    fn tag() -> String {
        std::env::var("PRIMITIVE_SCALE_TAG").unwrap_or_else(|_| "now".to_string())
    }

    /// The scale the tools measure: the Earth's, unless told otherwise.
    fn measured() -> Scale {
        std::env::var("PRIMITIVE_SCALE").ok().and_then(|name| Scale::parse(&name)).unwrap_or_default()
    }

    fn world(seed: u32, zone: Zone) -> WorldGen {
        WorldGen::with_scale(seed, Preset::Normal, zone, measured())
    }

    /// A square of the world, `pixels` samples a side and `step` blocks
    /// apart, centred on `centre`: the height and the biome of each sample,
    /// row by row. Split over a few threads, because a four-hundred-kilometre
    /// map is six hundred thousand columns and each is a dozen noise fields.
    fn sample(gen: &WorldGen, centre: (i32, i32), pixels: i32, step: i32) -> Vec<(i32, Biome)> {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 6);
        let side = pixels as usize;
        let rows_per = side.div_ceil(threads);
        let mut out = vec![(0, Biome::Ocean); side * side];
        std::thread::scope(|scope| {
            for (band, slice) in out.chunks_mut(rows_per * side).enumerate() {
                scope.spawn(move || {
                    for (i, cell) in slice.iter_mut().enumerate() {
                        let row = (band * rows_per + i / side) as i32;
                        let col = (i % side) as i32;
                        let gx = centre.0 + (col - pixels / 2) * step;
                        let gz = centre.1 + (row - pixels / 2) * step;
                        let height = gen.height_at(gx, gz);
                        *cell = (height, gen.biome_from(gx, gz, height));
                    }
                });
            }
        });
        out
    }

    fn palette(biome: Biome) -> [u8; 3] {
        match biome {
            Biome::Ocean => [34, 64, 118],
            Biome::River => [70, 140, 210],
            Biome::Beach => [226, 210, 150],
            Biome::Desert => [230, 204, 126],
            Biome::Savanna => [184, 176, 92],
            Biome::Plains => [134, 184, 96],
            Biome::Forest => [46, 112, 52],
            Biome::DeadForest => [128, 106, 78],
            Biome::BirchForest => [120, 170, 104],
            Biome::Swamp => [66, 104, 86],
            Biome::Bog => [98, 96, 72],
            Biome::Taiga => [52, 98, 90],
            Biome::Tundra => [182, 192, 186],
            Biome::Mountains => [132, 126, 120],
            Biome::SnowyPeaks => [246, 248, 252],
        }
    }

    /// Draws a sampled square: biome colours, lighter with height, a
    /// hillshade off the sample up and to the left, water darker with depth,
    /// and a red cross where the world puts a new player.
    fn draw(gen: &WorldGen, path: &str, cells: &[(i32, Biome)], centre: (i32, i32), pixels: i32, step: i32) {
        let mut img = image::RgbImage::new(pixels as u32, pixels as u32);
        let at = |row: i32, col: i32| cells[(row * pixels + col) as usize];
        let scale = (step as f32).powf(0.6);
        for row in 0..pixels {
            for col in 0..pixels {
                let (height, biome) = at(row, col);
                let base = palette(biome);
                let rgb = if matches!(biome, Biome::Ocean | Biome::River) {
                    let depth = ((SEA_LEVEL - height).max(0) as f32 / 40.0).min(1.0);
                    base.map(|c| (c as f32 * (1.0 - depth * 0.6)) as u8)
                } else {
                    let up = if row > 0 && col > 0 { at(row - 1, col - 1).0 } else { height };
                    let shade = 1.0 + ((height - up) as f32 / scale).clamp(-3.0, 3.0) * 0.08;
                    let lift = ((height - SEA_LEVEL) as f32 / 160.0).clamp(0.0, 1.0);
                    base.map(|c| (c as f32 * (0.85 + 0.35 * lift) * shade).clamp(0.0, 255.0) as u8)
                };
                img.put_pixel(col as u32, row as u32, image::Rgb(rgb));
            }
        }
        let (sx, sz) = gen.spawn_column();
        let (px, pz) = ((sx - centre.0) / step + pixels / 2, (sz - centre.1) / step + pixels / 2);
        for d in -6..=6 {
            for (x, z) in [(px + d, pz), (px, pz + d)] {
                if (0..pixels).contains(&x) && (0..pixels).contains(&z) {
                    img.put_pixel(x as u32, z as u32, image::Rgb([230, 30, 30]));
                }
            }
        }
        img.save(path).expect("write the map");
    }

    /// The shares a sampled square is made of, as one line.
    fn describe(cells: &[(i32, Biome)]) -> String {
        let mut counts: HashMap<Biome, usize> = HashMap::new();
        let (mut land, mut over_line, mut alpine, mut highest) = (0usize, 0usize, 0usize, i32::MIN);
        for &(height, biome) in cells {
            *counts.entry(biome).or_default() += 1;
            if biome != Biome::Ocean {
                land += 1;
                over_line += usize::from(height > SEA_LEVEL + 42);
                alpine += usize::from(height > SEA_LEVEL + 100);
            }
            highest = highest.max(height);
        }
        let mut shares: Vec<(Biome, usize)> = counts.into_iter().collect();
        shares.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        let of_land = |n: usize| n as f64 * 100.0 / land.max(1) as f64;
        let list: Vec<String> = shares
            .iter()
            .filter(|(b, _)| *b != Biome::Ocean)
            .map(|(b, n)| format!("{} {:.1}", b.name(), of_land(*n)))
            .collect();
        format!(
            "land {:.1}% of the square; of the land: over sea+42 {:.1}%, over sea+100 {:.2}%, highest {}; biomes % of land: {}",
            land as f64 * 100.0 / cells.len().max(1) as f64,
            of_land(over_line),
            of_land(alpine),
            highest,
            list.join(", ")
        )
    }

    /// Four hundred kilometres of each zone, forty of a temperate world and
    /// four round its spawn, drawn to `PRIMITIVE_SCALE_DUMP`.
    #[test]
    #[ignore = "a tool: writes maps of the world at three scales"]
    fn scale_maps() {
        let Some(dir) = out_dir() else {
            println!("set PRIMITIVE_SCALE_DUMP to an absolute directory to keep the maps");
            return;
        };
        let tag = tag();
        for &zone in Zone::ALL {
            let gen = world(1337, zone);
            // Half a kilometre a pixel for the temperate world, which is the
            // one looked at closest; a kilometre for the rest.
            let (pixels, step) = if zone == Zone::Temperate { (800, 500) } else { (400, 1000) };
            let started = std::time::Instant::now();
            let cells = sample(&gen, (0, 0), pixels, step);
            let path = format!("{dir}/{tag}_world_400km_{}.png", zone.name());
            draw(&gen, &path, &cells, (0, 0), pixels, step);
            println!("[scale] {} 400 km ({:.1} s): {}", zone.name(), started.elapsed().as_secs_f32(), describe(&cells));
        }
        let gen = world(1337, Zone::Temperate);
        let (sx, sz) = gen.spawn_column();
        for (name, pixels, step) in [("country_40km", 1000, 40), ("detail_4km", 1000, 4)] {
            let cells = sample(&gen, (sx, sz), pixels, step);
            draw(&gen, &format!("{dir}/{tag}_{name}_temperate.png"), &cells, (sx, sz), pixels, step);
            println!("[scale] temperate {name} round spawn: {}", describe(&cells));
        }
        println!("[scale] maps in {dir}");
    }

    /// What a country is from the point of view of a long walk: water,
    /// wood, open ground, desert, cold ground or bare rock.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    enum Cover {
        Water,
        Wood,
        Open,
        Desert,
        Cold,
        Rock,
    }

    fn cover(biome: Biome) -> Cover {
        match biome {
            Biome::Ocean | Biome::River => Cover::Water,
            Biome::Forest | Biome::BirchForest | Biome::Swamp | Biome::Taiga => Cover::Wood,
            Biome::Plains | Biome::Savanna | Biome::DeadForest | Biome::Beach => Cover::Open,
            Biome::Desert => Cover::Desert,
            Biome::Tundra | Biome::Bog => Cover::Cold,
            Biome::Mountains | Biome::SnowyPeaks => Cover::Rock,
        }
    }

    fn changes<T: PartialEq>(walk: &[T]) -> usize {
        1 + walk.windows(2).filter(|w| w[0] != w[1]).count()
    }

    /// How far a walk goes before the country changes, at three scales: the
    /// biome underfoot, the cover (wood, open, ...), and the mix -- the
    /// commonest cover of each four-kilometre stretch, which is the thing
    /// that is a forest province or a steppe one.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn scale_walks() {
        const LENGTH: i32 = 200_000;
        const STEP: i32 = 25;
        const WINDOW: usize = (4_000 / STEP) as usize;
        for seed in [1337u32, 7, 99] {
            let gen = world(seed, Zone::Temperate);
            let (mut walked, mut biome_runs, mut cover_runs, mut mix_runs, mut windows) = (0i64, 0usize, 0usize, 0usize, 0usize);
            let (mut wood, mut land, mut wet) = (0usize, 0usize, 0usize);
            for line in 0..8 {
                let along_x = line % 2 == 0;
                let offset = (line / 2 - 2) * 37_000 + 1_234;
                let walk: Vec<Biome> = (0..LENGTH / STEP)
                    .map(|i| {
                        let along = -LENGTH / 2 + i * STEP;
                        let (gx, gz) = if along_x { (along, offset) } else { (offset, along) };
                        gen.biome_at(gx, gz)
                    })
                    .collect();
                let covers: Vec<Cover> = walk.iter().map(|&b| cover(b)).collect();
                wood += covers.iter().filter(|&&c| c == Cover::Wood).count();
                land += covers.iter().filter(|&&c| c != Cover::Water).count();
                wet += walk.iter().filter(|&&b| b == Biome::Ocean).count();
                biome_runs += changes(&walk);
                cover_runs += changes(&covers);
                let mixes: Vec<Cover> = covers
                    .chunks(WINDOW)
                    .map(|window| {
                        let mut counts: HashMap<Cover, usize> = HashMap::new();
                        for &c in window {
                            *counts.entry(c).or_default() += 1;
                        }
                        counts.into_iter().max_by_key(|&(c, n)| (n, c as u8)).map_or(Cover::Water, |(c, _)| c)
                    })
                    .collect();
                mix_runs += changes(&mixes);
                windows += mixes.len();
                walked += i64::from(LENGTH);
            }
            println!(
                "[scale] seed {seed}, 8 walks of {} km: the biome changes every {:.0} m, the cover every {:.0} m, \
                 the four-kilometre mix every {:.1} km ({windows} windows); wood is {:.0}% of the land, sea {:.0}% of the walk",
                LENGTH / 1000,
                walked as f64 / biome_runs as f64,
                walked as f64 / cover_runs as f64,
                walked as f64 / 1000.0 / mix_runs as f64,
                wood as f64 * 100.0 / land.max(1) as f64,
                wet as f64 * 100.0 / (walked / i64::from(STEP)) as f64
            );
        }
    }

    /// How far a new player walks to the first tree, water, loose or bare
    /// stone, and flint lying on the ground, in each zone -- found in the
    /// generated chunks, ring by ring out from the spawn chunk.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn scale_from_spawn() {
        const MAX_RING: i32 = 40;
        const NAMES: [&str; 4] = ["tree", "water", "stone", "flint"];
        for &zone in Zone::ALL {
            // Per kind: the sum and count of the distances found, the worst,
            // and how many worlds had none in reach.
            let mut found_in: [(f64, usize, f64, usize); 4] = [(0.0, 0, 0.0, 0); 4];
            let (mut chunks_seen, mut tall_chunks) = (0usize, 0usize);
            for seed in 0..6u32 {
                let gen = world(seed, zone);
                let started = std::time::Instant::now();
                let (sx, sz) = gen.spawn_column();
                let search = started.elapsed();
                let (home, _, _) = ChunkPos::from_global(sx, sz);
                let mut best = [f64::MAX; 4];
                for ring in 0..=MAX_RING {
                    for cz in -ring..=ring {
                        for cx in -ring..=ring {
                            if cx.abs().max(cz.abs()) != ring {
                                continue;
                            }
                            let pos = ChunkPos::new(home.x + cx, home.z + cz);
                            let chunk = gen.generate_chunk(pos);
                            let (ox, oz) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
                            let mut skyline = 0;
                            for lz in 0..CHUNK_SIZE_Z {
                                for lx in 0..CHUNK_SIZE_X {
                                    let (gx, gz) = (ox + lx as i32, oz + lz as i32);
                                    let ground = gen.height_at(gx, gz);
                                    let distance = f64::from((gx - sx) * (gx - sx) + (gz - sz) * (gz - sz)).sqrt();
                                    for y in (1..CHUNK_SIZE_Y - 1).rev() {
                                        let id = chunk.get(lx, y, lz);
                                        if id == BLOCK_AIR {
                                            continue;
                                        }
                                        skyline = skyline.max(y as i32);
                                        let open = chunk.get(lx, y + 1, lz) == BLOCK_AIR;
                                        let kind = block_kind(id);
                                        let near_top = y as i32 >= ground - 1;
                                        let hits = [
                                            kind == BLOCK_LOG || kind == BLOCK_BIRCH_LOG || kind == BLOCK_PALM_TRUNK || is_branch(id),
                                            (kind == BLOCK_WATER || kind == BLOCK_ICE) && open,
                                            open && near_top
                                                && matches!(
                                                    kind,
                                                    BLOCK_PEBBLE | BLOCK_COBBLESTONE | BLOCK_STONE | BLOCK_GRANITE | BLOCK_LIMESTONE | BLOCK_SANDSTONE
                                                ),
                                            kind == BLOCK_FLINT && near_top,
                                        ];
                                        for (slot, hit) in best.iter_mut().zip(hits) {
                                            if hit {
                                                *slot = slot.min(distance);
                                            }
                                        }
                                        if y as i32 <= ground - 2 {
                                            break;
                                        }
                                    }
                                }
                            }
                            chunks_seen += 1;
                            tall_chunks += usize::from(skyline >= SEA_LEVEL + 32);
                        }
                    }
                    let reach = f64::from(ring * CHUNK_SIZE_X as i32);
                    if best.iter().all(|&d| d <= reach) {
                        break;
                    }
                }
                let line: Vec<String> = NAMES
                    .iter()
                    .zip(best)
                    .map(|(name, d)| if d == f64::MAX { format!("{name} >{} m", MAX_RING * 16) } else { format!("{name} {d:.0} m") })
                    .collect();
                println!(
                    "[scale] {} seed {seed}: spawn {sx},{sz} in {} (search {:.0} ms): {}",
                    zone.name(),
                    gen.biome_at(sx, sz).name(),
                    search.as_secs_f64() * 1000.0,
                    line.join(", ")
                );
                for (tally, d) in found_in.iter_mut().zip(best) {
                    if d == f64::MAX {
                        tally.3 += 1;
                    } else {
                        *tally = (tally.0 + d, tally.1 + 1, tally.2.max(d), tally.3);
                    }
                }
            }
            let line: Vec<String> = NAMES
                .iter()
                .zip(found_in)
                .map(|(name, (sum, n, worst, missing))| {
                    let missing = if missing > 0 { format!(" ({missing} not found)") } else { String::new() };
                    format!("{name} mean {:.0} m worst {worst:.0} m{missing}", sum / n.max(1) as f64)
                })
                .collect();
            println!(
                "[scale] {} over six seeds: {}; {} of {} chunks scanned reach sea+32",
                zone.name(),
                line.join(", "),
                tall_chunks,
                chunks_seen
            );
        }
    }

    #[test]
    fn the_open_ocean_is_deeper_than_a_dive_and_the_shelf_is_not() {
        // **"Сделай океан глубже".** Measured before the change
        // (`how_deep_the_sea_is`): two hundred kilometres of swimming out
        // from a shore met twenty-two blocks of water at the deepest. The
        // drop existed but was spent over a range of the continent field
        // that only the middle of a basin reaches.
        //
        // Both halves are asserted here because the fix is only right if
        // the second one holds: the coast, the surf and the shelf are the
        // spline's own shallow end and must not have moved a block --
        // beaches, boats and every rule about wading are built on them.
        let gen = world(1337, Zone::Temperate);
        let depth = |continent: f64| SEA_LEVEL as f64 - gen.coast_profile(continent, 0.0);

        assert!(depth(-3.0) > 40.0, "the open ocean is {:.0} blocks deep", depth(-3.0));
        // The foot of the shelf is where the drop has only started -- a
        // fifth of it is spent by here -- and it is already twice the
        // shelf's own ten blocks, which is what makes the break read as a
        // break from the surface.
        assert!(depth(-1.0) > 18.0, "the foot of the shelf is {:.0} blocks down", depth(-1.0));

        // ...and the shallow end, unchanged: the surf is the spline's own.
        for (continent, wanted) in [(-0.26, 59.0), (-0.10, 61.5), (-0.01, 63.0)] {
            let height = gen.coast_profile(continent, 0.0);
            assert!(
                (height - wanted).abs() < 0.01,
                "the coast moved: continent {continent} used to stand at {wanted} and now stands at {height:.2}"
            );
        }
    }

    /// **How deep the sea is, walked out from a shore.**
    ///
    /// Asked because "сделай океан глубже" needs a number to be answered
    /// with: the deep end of `CONTINENT_SPLINE` is a shelf foot, and how
    /// much further the open ocean falls is `ABYSS_DROP` alone. This walks
    /// straight out to sea from the first beach it finds and reports the
    /// depth by distance, which is what a player swimming out actually
    /// meets.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn how_deep_the_sea_is() {
        for seed in [1337u32, 7] {
            let gen = world(seed, Zone::Temperate);
            // Out along +x from the origin, which is somewhere on a
            // continent: the first wet column is the shore.
            let mut shore = None;
            for x in 0..400_000 {
                if gen.height_at(x, 0) < SEA_LEVEL {
                    shore = Some(x);
                    break;
                }
            }
            let Some(shore) = shore else {
                println!("[sea] seed {seed}: no sea within 400 km of the origin");
                continue;
            };
            let mut deepest = 0;
            let mut line = String::new();
            for step in [0, 100, 500, 1_000, 5_000, 20_000, 50_000, 100_000, 200_000] {
                let depth = SEA_LEVEL - gen.height_at(shore + step, 0);
                line.push_str(&format!(" {}km:{}", step / 1000, depth.max(0)));
            }
            for step in 0..200_000 {
                deepest = deepest.max(SEA_LEVEL - gen.height_at(shore + step, 0));
            }
            println!("[sea] seed {seed}: shore at x={shore},{line} | deepest in 200 km: {deepest}");
        }
    }

    /// **The water measured against the Earth's**, a block a metre: every
    /// river crossing on long straight walks sorted to the order whose line
    /// it is, its width taken square to the channel and its depth at the
    /// deepest column; every pond and lake site in a square by diameter and
    /// depth; and the open ocean sampled for dry land far from any coast.
    ///
    /// Written for "реки и озёра как в реальном мире" -- the numbers a
    /// stream, a river, a pond and a lake are held to came from here, before
    /// and after.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn how_real_the_water_is() {
        fn percentiles(values: &mut [i32]) -> String {
            if values.is_empty() {
                return "none".to_string();
            }
            values.sort_unstable();
            let at = |share: f64| values[((values.len() - 1) as f64 * share).round() as usize];
            format!("p10 {} p50 {} p90 {} max {}", at(0.1), at(0.5), at(0.9), values[values.len() - 1])
        }
        // `PRIMITIVE_WATER_KM` shortens the walks and skips the ocean and the
        // spawns, for a look from a debug build.
        let quick: Option<i32> = std::env::var("PRIMITIVE_WATER_KM").ok().and_then(|km| km.parse().ok());
        for seed in [1337u32, 7] {
            let gen = world(seed, Zone::Temperate);
            let orders = gen.river_orders();
            // Sixteen walks of forty kilometres, eight each way.
            let length: i32 = quick.map_or(40_000, |km| km * 1_000);
            const LINES: i32 = 16;
            let found: Vec<Vec<(usize, i32, i32)>> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..LINES)
                    .map(|line| {
                        let gen = &gen;
                        scope.spawn(move || {
                            let along_x = line % 2 == 0;
                            let offset = (line / 2 - 4) * 5_000 + 517;
                            let at = |i: i32| if along_x { (i, offset) } else { (offset, i) };
                            let mut out = Vec::new();
                            let (mut run, mut deepest) = (0, i32::MAX);
                            for i in -length / 2..length / 2 {
                                let (gx, gz) = at(i);
                                let h = gen.height_at(gx, gz);
                                if h < SEA_LEVEL {
                                    run += 1;
                                    deepest = deepest.min(h);
                                    continue;
                                }
                                if run > 0 {
                                    let (mx, mz) = at(i - run / 2 - 1);
                                    let mh = gen.height_at(mx, mz);
                                    if gen.biome_from(mx, mz, mh) == Biome::River {
                                        // The order whose line is nearest, in
                                        // widths of its own channel.
                                        let (x, z) = (f64::from(mx), f64::from(mz));
                                        let mut best: Option<(usize, f64, f64)> = None;
                                        for (index, order) in orders.iter().enumerate() {
                                            let field = gen.river_field(order, x, z);
                                            let sx = (gen.river_field(order, x + 1.0, z) - gen.river_field(order, x - 1.0, z)) / 2.0;
                                            let sz = (gen.river_field(order, x, z + 1.0) - gen.river_field(order, x, z - 1.0)) / 2.0;
                                            let slope = sx.hypot(sz).max(f64::EPSILON);
                                            let across = field.abs() / slope / order.half_width;
                                            let cos = if along_x { sx / slope } else { sz / slope };
                                            if best.is_none_or(|(_, a, _)| across < a) {
                                                best = Some((index, across, cos.abs()));
                                            }
                                        }
                                        if let Some((index, across, cos)) = best.filter(|b| b.1 < 2.0) {
                                            let _ = across;
                                            let width = ((f64::from(run) * cos).round() as i32).max(1);
                                            out.push((index, width, SEA_LEVEL - deepest));
                                        }
                                    }
                                }
                                run = 0;
                                deepest = i32::MAX;
                            }
                            out
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().expect("a walk panicked")).collect()
            });
            let km = f64::from(LINES * length) / 1_000.0;
            for (index, order) in orders.iter().enumerate() {
                let crossings: Vec<(usize, i32, i32)> = found.iter().flatten().copied().filter(|c| c.0 == index).collect();
                let mut widths: Vec<i32> = crossings.iter().map(|c| c.1).collect();
                let mut depths: Vec<i32> = crossings.iter().map(|c| c.2).collect();
                println!(
                    "[water] seed {seed} order {index} (half width {}): {} crossings in {km:.0} km ({:.1} per 100 km) | width {} | depth {}",
                    order.half_width,
                    crossings.len(),
                    crossings.len() as f64 * 100.0 / km,
                    percentiles(&mut widths),
                    percentiles(&mut depths)
                );
            }
            // Ponds in a square twelve kilometres a side.
            let cells = 6_000 / gen.pond_cell();
            let (mut diameters, mut depths) = (Vec::new(), Vec::new());
            for cz in -cells..cells {
                for cx in -cells..cells {
                    if let Some(lake) = gen.lake_site(cx, cz) {
                        diameters.push(lake.radius * 2 + 1);
                        depths.push(lake.depth);
                    }
                }
            }
            println!(
                "[water] seed {seed}: {} ponds in 144 km2 ({:.1} per 100 km2) | across {} | depth {}",
                diameters.len(),
                diameters.len() as f64 * 100.0 / 144.0,
                percentiles(&mut diameters),
                percentiles(&mut depths)
            );
            // Wide lakes in a square sixty-four kilometres a side.
            let cells = 32_000 / WIDE_LAKE_CELL;
            let (mut diameters, mut depths) = (Vec::new(), Vec::new());
            for cz in -cells..cells {
                for cx in -cells..cells {
                    if let Some(lake) = gen.wide_lake_site(cx, cz) {
                        diameters.push(lake.radius * 2 + 1);
                        depths.push(lake.depth);
                    }
                }
            }
            let area = f64::from(2 * cells * WIDE_LAKE_CELL).powi(2) / 1e6;
            println!(
                "[water] seed {seed}: {} wide lakes in {area:.0} km2 ({:.2} per 100 km2) | across {} | depth {}",
                diameters.len(),
                diameters.len() as f64 * 100.0 / area,
                percentiles(&mut diameters),
                percentiles(&mut depths)
            );
            if quick.is_some() {
                continue;
            }
            // The open ocean: a 512-block lattice over a square eight hundred
            // kilometres a side, counting the deep samples and the dry ones
            // with deep water three kilometres out on every side -- land that
            // is not a coast's.
            let (deep, islands): (usize, usize) = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..8)
                    .map(|band| {
                        let gen = &gen;
                        scope.spawn(move || {
                            let is_deep = |gx: i32, gz: i32| gen.height_at(gx, gz) < SEA_LEVEL - 25;
                            let (mut deep, mut islands) = (0usize, 0usize);
                            for row in (-800 + band * 200)..(-600 + band * 200) {
                                for col in -800..800 {
                                    let (gx, gz) = (col * 512, row * 512);
                                    let h = gen.height_at(gx, gz);
                                    if h < SEA_LEVEL - 25 {
                                        deep += 1;
                                    } else if h >= SEA_LEVEL
                                        && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dz)| is_deep(gx + dx * 3_000, gz + dz * 3_000))
                                    {
                                        islands += 1;
                                    }
                                }
                            }
                            (deep, islands)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().expect("a band panicked")).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
            });
            println!(
                "[water] seed {seed}: {deep} deep-ocean samples on a 512-block lattice ({:.0} km2), {islands} dry samples out there ({:.2} per 10 000 km2 of deep ocean)",
                deep as f64 * 0.262_144,
                islands as f64 * 10_000.0 / (deep as f64 * 0.262_144).max(1.0)
            );
        }
        // Where new players wake, so a change to the water can be seen not to
        // have moved one.
        if quick.is_some() {
            return;
        }
        for &zone in Zone::ALL {
            let spawns: Vec<(i32, i32)> = (0..6u32).map(|seed| world(seed, zone).spawn_column()).collect();
            println!("[water] spawns in {}: {spawns:?}", zone.name());
        }
    }

    #[test]
    #[ignore = "scratch"]
    fn why_wide_lakes_are_refused() {
        let gen = world(1337, Zone::Temperate);
        let (mut high, mut low, mut n) = (0, 0, 0);
        let mut spreads = Vec::new();
        for cz in -8..8 {
            for cx in -8..8 {
                n += 1;
                let (x, z, radius, _) = gen.wide_lake_candidate(cx, cz);
                let centre = gen.terrain_height(x, z);
                if centre < LAKE_MIN_WATER { low += 1; continue; }
                if centre > WIDE_LAKE_HIGHEST { high += 1; continue; }
                let edge = wide_edge(radius);
                let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                for i in 0..720 {
                    let a = f64::from(i) / 720.0 * std::f64::consts::TAU;
                    let h = gen.terrain_height(x + (a.cos() * f64::from(edge)) as i32, z + (a.sin() * f64::from(edge)) as i32);
                    lo = lo.min(h); hi = hi.max(h);
                }
                spreads.push((radius, centre - SEA_LEVEL, lo - SEA_LEVEL, hi - lo, wide_rim_spread(radius)));
            }
        }
        println!("{n} cells: centre under {low}, over {high}; rest (radius, centre, lowest, spread, allowed): {spreads:?}");
    }

    /// Rivers by width and lakes by count: the water a walk crosses.
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn scale_water() {
        for seed in [1337u32, 7] {
            let gen = world(seed, Zone::Temperate);
            // Widths of the water a straight line crosses with land on both
            // sides and a river under the middle of it.
            const LENGTH: i32 = 40_000;
            let bounds = [3, 10, 30, 100, 400, i32::MAX];
            let mut widths = [0usize; 6];
            for line in 0..8 {
                let along_x = line % 2 == 0;
                let offset = (line / 2 - 2) * 9_000 + 517;
                let mut run = 0;
                for i in -LENGTH / 2..LENGTH / 2 {
                    let (gx, gz) = if along_x { (i, offset) } else { (offset, i) };
                    if gen.height_at(gx, gz) < SEA_LEVEL {
                        run += 1;
                        continue;
                    }
                    if run > 0 {
                        let middle = i - run / 2 - 1;
                        let (mx, mz) = if along_x { (middle, offset) } else { (offset, middle) };
                        let h = gen.height_at(mx, mz);
                        if gen.biome_from(mx, mz, h) == Biome::River {
                            let slot = bounds.iter().position(|&b| run <= b).unwrap_or(5);
                            widths[slot] += 1;
                        }
                    }
                    run = 0;
                }
            }
            println!(
                "[scale] seed {seed}, 320 km of walks: river crossings by width 1-3 {}, 4-10 {}, 11-30 {}, 31-100 {}, 101-400 {}, wider {}",
                widths[0], widths[1], widths[2], widths[3], widths[4], widths[5]
            );
            // Ponds in a square six kilometres a side round the origin.
            let cells = 3_000 / LAKE_CELL;
            let mut radii: Vec<i32> = Vec::new();
            for cz in -cells..cells {
                for cx in -cells..cells {
                    if let Some(lake) = gen.lake_site(cx, cz) {
                        radii.push(lake.radius);
                    }
                }
            }
            let mean = radii.iter().map(|&r| f64::from(r)).sum::<f64>() / radii.len().max(1) as f64;
            println!(
                "[scale] seed {seed}: {} ponds in 36 km2 ({:.1} per 100 km2), mean radius {mean:.1}, widest {}",
                radii.len(),
                radii.len() as f64 * 100.0 / 36.0,
                radii.iter().max().copied().unwrap_or(0)
            );
            // Wide lakes in a square sixty-four kilometres a side.
            let mut wide: Vec<i32> = Vec::new();
            for cz in -32..32 {
                for cx in -32..32 {
                    if let Some(lake) = gen.wide_lake_site(cx, cz) {
                        wide.push(lake.radius);
                    }
                }
            }
            let mean = wide.iter().map(|&r| f64::from(r)).sum::<f64>() / wide.len().max(1) as f64;
            println!(
                "[scale] seed {seed}: {} wide lakes in {} km2 ({:.2} per 100 km2), mean radius {mean:.1}, widest {}",
                wide.len(),
                64 * 64 * WIDE_LAKE_CELL / 1_000 * WIDE_LAKE_CELL / 1_000,
                wide.len() as f64 * 100.0 / (64.0 * 64.0 * f64::from(WIDE_LAKE_CELL).powi(2) / 1e6),
                wide.iter().max().copied().unwrap_or(0)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use crate::types::is_branch;

    /// **The written-out turns are the turns.** A digit dropped from the table
    /// is an octave turned a little the wrong way, which nothing else would
    /// ever notice.
    #[test]
    fn the_turns_are_the_angles_they_say_they_are() {
        for (octave, &(sin, cos)) in TURNS.iter().enumerate() {
            let (s, c) = (0.61 + octave as f64 * 1.37).sin_cos();
            assert!((s - sin).abs() < 1e-12 && (c - cos).abs() < 1e-12, "octave {octave} is turned ({sin}, {cos}), not ({s}, {c})");
        }
    }

    /// **A slow field read between its lattice points is the field.** The
    /// lattice exists for speed (`SLOW_STEP`); a field read off it that
    /// strayed from the noise would move a coast or a province boundary for
    /// no reason a player could find.
    #[test]
    fn the_slow_fields_read_between_the_lattice_are_the_fields_themselves() {
        let gen = WorldGen::new(1337);
        for i in 0..2_000 {
            let gx = (hash2(i, 7, 0x51) % 400_000) as i32 - 200_000;
            let gz = (hash2(i, 9, 0x52) % 400_000) as i32 - 200_000;
            let (read, exact) = (gen.slow(gx, gz), gen.slow_exact(gx, gz));
            // Gain on the basin: a shift of the basin moves the coast by
            // itself times `BASIN_GAIN` in units of the coast field.
            assert!((read[SLOW_BASIN] - exact[SLOW_BASIN]).abs() * BASIN_GAIN < 0.01, "the basin at {gx},{gz}");
            for field in [SLOW_HIGHLAND, SLOW_WEATHER, SLOW_RAIN] {
                assert!((read[field] - exact[field]).abs() < 1e-3, "field {field} at {gx},{gz}");
            }
            assert!((read[SLOW_ROLLING] - exact[SLOW_ROLLING]).abs() < 0.01, "the rolling land at {gx},{gz}");
        }
    }

    /// **The promise every slow field makes to a new player.** The zone's
    /// climate at spawn, a coast at the origin, middling hills round it: all
    /// three rest on each slow field being exactly zero at the origin, and a
    /// field read at a fractional offset -- one refactor away -- would move
    /// every spawn in the world without failing anything else.
    #[test]
    fn every_slow_field_is_nothing_at_the_origin_whatever_the_seed() {
        for seed in [0u32, 1, 7, 1337, 99_999, u32::MAX] {
            let gen = WorldGen::new(seed);
            assert!(gen.basin(0, 0).abs() < 1e-12, "seed {seed}: the basin is {} at the origin", gen.basin(0, 0));
            assert!((gen.highland(0, 0) - HIGHLAND_AT_HOME).abs() < 1e-12, "seed {seed}: the origin is not middling country");
            assert!(gen.weather_mix(0, 0, 0.0).abs() < 1e-12, "seed {seed}: a weather province sits on the origin");
            assert!(gen.rain_mix(0, 0, 0.0).abs() < 1e-12, "seed {seed}: a rain province sits on the origin");
        }
    }

    /// **...and a river is the one thing that must not be.** A river field
    /// zero at the origin puts every order's channel through every spawn, and
    /// the great river's floodplain five kilometres wide under it: the first
    /// version flattened the country round the origin of every seed.
    #[test]
    fn no_order_of_river_is_drawn_through_the_origin_of_every_world() {
        for order in &EARTH_RIVERS {
            let through = [0u32, 1, 7, 1337, 99_999]
                .iter()
                .filter(|&&seed| WorldGen::new(seed).river_field(order, 0.0, 0.0).abs() < 1e-6)
                .count();
            assert_eq!(through, 0, "the order at {} runs through the origin of {through} worlds", order.frequency);
        }
    }

    /// **An old world is drawn by the arithmetic it was drawn by.** Every
    /// hook the Earth's scale added answers the regional number exactly in a
    /// regional world -- not nearly, exactly, because a height one block off
    /// under a saved house is a house with a step in its floor.
    #[test]
    fn a_world_from_before_the_earth_scale_is_given_none_of_it() {
        let old = WorldGen::with_scale(1337, Preset::Normal, Zone::Temperate, Scale::Regional);
        for (gx, gz) in [(0, 0), (12_345, -6_789), (-300_000, 250_000), (1_500_000, 40)] {
            let highland = old.highland(gx, gz);
            assert_eq!(old.basin(gx, gz).to_bits(), 0.0f64.to_bits());
            assert_eq!(highland.to_bits(), HIGHLAND_AT_HOME.to_bits());
            assert_eq!(old.relief_amplitude(highland).to_bits(), 80.0f64.to_bits());
            assert_eq!(old.unworn_lean(highland).to_bits(), 0.0f64.to_bits());
            assert_eq!(old.rolling(gx, gz, highland).to_bits(), 0.0f64.to_bits());
            assert_eq!(old.mosaic_wetness(gx, gz, SEA_LEVEL + 3).to_bits(), 0.0f64.to_bits());
            assert_eq!(old.weather_mix(gx, gz, 0.3).to_bits(), 0.3f64.to_bits());
            assert_eq!(old.rain_mix(gx, gz, -0.4).to_bits(), (-0.4f64).to_bits());
            assert_eq!(old.soft_ceiling(250.0).to_bits(), 250.0f64.to_bits());
            assert!(old.wide_lake_near(gx, gz).is_none() && !old.wide_lake_within(gx, gz, 100));
            assert!(old.pond_offered(gx.div_euclid(LAKE_CELL), gz.div_euclid(LAKE_CELL)), "a regional pond was thinned");
        }
        for (gx, gz) in [(0, 0), (4_321, -987), (-80_000, 60_000)] {
            let continent = old.continent_on(gx, gz, 0.0).clamp(-1.0, 1.0);
            assert_eq!(old.inland(gx, gz, old.basin(gx, gz)).to_bits(), continent.to_bits());
        }
        for continent in [-1.0, -0.3, 0.0, 0.7, 1.0] {
            assert_eq!(old.coast_profile(continent, 0.0).to_bits(), spline(CONTINENT_SPLINE, continent).to_bits());
            assert_eq!(old.basin_lift(0.0).to_bits(), 0.0f64.to_bits());
        }
        assert_eq!(old.river_orders(), &REGIONAL_RIVERS);
        assert_eq!(REGIONAL_RIVERS[0].valley, 49.0, "the regional valley is seven times seven");
        for &biome in Biome::ALL {
            let (shortest, tallest, _) = biome.tree_shape();
            assert_eq!(old.trunks(biome, (shortest, tallest)), (shortest, tallest), "{}", biome.name());
        }
        assert_eq!(old.old_trunks(), (OLD_TRUNK_SHORTEST, OLD_TRUNK_TALLEST));
        assert_ne!(old.key(), WorldGen::new(1337).key(), "an old world and a new one would share tiles");
    }

    /// **The cull may never skip a column a river reaches.** It skips the
    /// slope where the field is further from zero than the valley is wide
    /// times the steepest the field can be; a field steeper than the bound
    /// somewhere is a river that ends in a straight line across its own
    /// valley. Measured on the field's true slope, over every order, with a
    /// fifth of the bound to spare.
    #[test]
    fn the_river_cull_is_steeper_than_the_river_field_ever_gets() {
        let gen = WorldGen::new(4242);
        for order in REGIONAL_RIVERS.iter().chain(EARTH_RIVERS.iter()) {
            let step = 0.001 / order.frequency;
            let mut steepest = 0.0f64;
            for i in 0..200_000 {
                // Scattered over a thousand lattice cells either way.
                let x = (f64::from(hash2(i, 3, 0x51) % 2_000_000) / 1_000.0 - 1_000.0) / order.frequency;
                let z = (f64::from(hash2(i, 5, 0x52) % 2_000_000) / 1_000.0 - 1_000.0) / order.frequency;
                let sx = (gen.river_field(order, x + step, z) - gen.river_field(order, x - step, z)) / (2.0 * step);
                let sz = (gen.river_field(order, x, z + step) - gen.river_field(order, x, z - step)) / (2.0 * step);
                steepest = steepest.max((sx * sx + sz * sz).sqrt() / order.frequency);
            }
            println!("an order at {}: steepest {steepest:.3} per unit of frequency", order.frequency);
            assert!(
                steepest < RIVER_SLOPE_BOUND * 0.8,
                "the field of the order at {} is {steepest:.3} steep, over the cull's bound of {RIVER_SLOPE_BOUND}",
                order.frequency
            );
        }
    }

    /// **Oceans and continents are hundreds of kilometres, and the origin is
    /// on the coast between them.** Along lines sixteen hundred kilometres
    /// long the sign of the basin runs for hundreds of kilometres at a time;
    /// and a new player can see the sea from where they wake.
    #[test]
    fn the_sea_and_the_land_run_for_hundreds_of_kilometres_and_a_new_player_wakes_on_a_coast() {
        for seed in [1337u32, 7, 99] {
            let gen = WorldGen::new(seed);
            let (mut runs, mut samples) = (0usize, 0usize);
            for line in 0..6 {
                let across = (line - 3) * 230_000 + 7_000;
                let signs: Vec<bool> = (0..800)
                    .map(|i| {
                        let along = -800_000 + i * 2_000;
                        let (gx, gz) = if line % 2 == 0 { (along, across) } else { (across, along) };
                        gen.basin(gx, gz) > 0.0
                    })
                    .collect();
                runs += 1 + signs.windows(2).filter(|w| w[0] != w[1]).count();
                samples += signs.len();
            }
            let mean_km = (samples * 2) as f64 / runs as f64;
            assert!(mean_km > 150.0, "seed {seed}: sea and land change every {mean_km:.0} km");

            let (sx, sz) = gen.spawn_column();
            let sea = (1..=30).any(|out| {
                (0..16).any(|heading| {
                    let angle = f64::from(heading) / 16.0 * std::f64::consts::TAU;
                    let (gx, gz) = (sx + (angle.cos() * f64::from(out * 100)) as i32, sz + (angle.sin() * f64::from(out * 100)) as i32);
                    let height = gen.height_at(gx, gz);
                    height < SEA_LEVEL && gen.biome_from(gx, gz, height) == Biome::Ocean
                })
            });
            assert!(sea, "seed {seed}: no sea within three kilometres of the spawn at {sx},{sz}");
        }
    }

    /// **A forest country and an open one are provinces, and neither is one
    /// colour.** Along walks of six hundred kilometres the commonest cover of
    /// each ten-kilometre stretch -- wood or open ground -- holds for tens of
    /// kilometres at a time; and the biome underfoot still changes every few
    /// hundred metres, because a forest has glades and a steppe has groves.
    #[test]
    fn a_forest_country_and_an_open_one_are_provinces_with_glades_and_groves_in_them() {
        const STEP: i32 = 100;
        const WINDOW: usize = 100;
        let probe = SEA_LEVEL + 8;
        for seed in [1337u32, 7] {
            let gen = WorldGen::new(seed);
            let (mut stretches, mut stretch_runs, mut steps, mut biome_runs) = (0usize, 0usize, 0usize, 0usize);
            for line in 0..4 {
                let across = (line - 2) * 150_000 + 3_000;
                let walk: Vec<Biome> = (0..6_000)
                    .map(|i| {
                        let along = -300_000 + i * STEP;
                        let (gx, gz) = if line % 2 == 0 { (along, across) } else { (across, along) };
                        gen.land_biome(gx, gz, probe)
                    })
                    .collect();
                let wooded: Vec<bool> = walk
                    .chunks(WINDOW)
                    .map(|window| {
                        let wood = window.iter().filter(|b| matches!(b, Biome::Forest | Biome::BirchForest | Biome::Taiga | Biome::Swamp)).count();
                        wood * 2 > window.len()
                    })
                    .collect();
                stretches += wooded.len();
                stretch_runs += 1 + wooded.windows(2).filter(|w| w[0] != w[1]).count();
                steps += walk.len();
                biome_runs += 1 + walk.windows(2).filter(|w| w[0] != w[1]).count();
            }
            let province_km = (stretches * WINDOW) as f64 * f64::from(STEP) / 1_000.0 / stretch_runs as f64;
            let biome_m = steps as f64 * f64::from(STEP) / biome_runs as f64;
            println!("seed {seed}: the wooded or open stretch holds for {province_km:.0} km, the biome for {biome_m:.0} m");
            assert!(province_km >= 30.0, "seed {seed}: a province is only {province_km:.0} km");
            assert!(biome_m <= 2_000.0, "seed {seed}: a province is one colour for {biome_m:.0} m at a time");
        }
    }

    /// Every island group of a seed in a square of cells round the origin,
    /// with the cell it is in.
    fn island_groups(gen: &WorldGen, cells: i32) -> Vec<IslandGroup> {
        let mut out = Vec::new();
        for cz in -cells..cells {
            for cx in -cells..cells {
                if let Some(group) = gen.island_candidate(cx, cz).and_then(|candidate| gen.island_group(cx, cz, candidate)) {
                    out.push(group);
                }
            }
        }
        out
    }

    /// **"Сделай острова в океане."** Over the open ocean of two seeds there
    /// are lone islands and archipelagos; an island of any size is dry land in
    /// the middle and a beach -- sand, or stone in a cold sea -- where a walk
    /// out from its middle meets the water; and a few kilometres from any
    /// island the abyss is as deep as `ABYSS_DROP` made it.
    #[test]
    fn islands_rise_from_the_abyss_with_beaches_and_the_abyss_is_deep_away_from_them() {
        let (mut groups_seen, mut islands_seen, mut beached, mut walked, mut abyss) = (0usize, 0usize, 0usize, 0usize, 0usize);
        for seed in [1337u32, 7] {
            let gen = WorldGen::new(seed);
            let groups = island_groups(&gen, 40);
            groups_seen += groups.len();
            for group in groups.iter().take(12) {
                for island in &group.islands[..group.count] {
                    islands_seen += 1;
                    let (ix, iz) = (island.x.round() as i32, island.z.round() as i32);
                    if island.radius < 60.0 || !gen.on_island(ix, iz) {
                        continue;
                    }
                    assert!(gen.height_at(ix, iz) >= SEA_LEVEL, "seed {seed}: the middle of an island at {ix},{iz} is under water");
                    for heading in 0..8 {
                        let angle = f64::from(heading) / 8.0 * std::f64::consts::TAU;
                        let at = |out: i32| (ix + (angle.cos() * f64::from(out)).round() as i32, iz + (angle.sin() * f64::from(out)).round() as i32);
                        let Some(last) = (1..2_000).take_while(|&out| {
                            let (gx, gz) = at(out);
                            gen.height_at(gx, gz) >= SEA_LEVEL
                        }).last() else {
                            continue;
                        };
                        let (gx, gz) = at(last);
                        let height = gen.height_at(gx, gz);
                        // A river mouth or a cliff can meet the water too;
                        // most of a shore is a beach.
                        walked += 1;
                        beached += usize::from(gen.biome_from(gx, gz, height) == Biome::Beach);
                    }
                }
                // Out past the group, where nothing of it reaches, over the
                // deep: as deep as the open ocean ever was.
                let centre = group.islands[..group.count].iter().fold((0.0, 0.0), |a, i| (a.0 + i.x / group.count as f64, a.1 + i.z / group.count as f64));
                for (dx, dz) in [(6_000, 0), (-6_000, 0), (0, 6_000), (0, -6_000)] {
                    let (gx, gz) = (centre.0 as i32 + dx, centre.1 as i32 + dz);
                    if gen.island_field(gx, gz).is_some() || gen.open_sea(gx, gz, gen.basin(gx, gz)) > ABYSS_TO {
                        continue;
                    }
                    abyss += 1;
                    let depth = SEA_LEVEL - gen.height_at(gx, gz);
                    assert!(depth > 40, "seed {seed}: the ocean six kilometres from an island at {gx},{gz} is {depth} deep");
                }
            }
        }
        assert!(groups_seen >= 20, "only {groups_seen} island groups under two seeds' thousand-kilometre squares");
        assert!(islands_seen >= 40, "only {islands_seen} islands in the groups looked at");
        assert!(walked >= 40, "only {walked} walks off an island's middle reached its shore");
        assert!(beached * 3 >= walked * 2, "{beached} of {walked} walks off an island reached a beach");
        assert!(abyss >= 10, "only {abyss} samples of the abyss round the islands");
    }

    /// **No island is the view from a new player's beach.** For every zone,
    /// the nearest island to where six seeds wake is a voyage away, and the
    /// spawn is not on one.
    #[test]
    fn no_island_lies_within_sight_of_where_a_player_wakes() {
        for &zone in Zone::ALL {
            for seed in 0..6u32 {
                let gen = WorldGen::with_zone(seed, Preset::Normal, zone);
                let (sx, sz) = gen.spawn_column();
                let (px, pz) = (sx.wrapping_add(gen.planet_origin().0), sz.wrapping_add(gen.planet_origin().1));
                assert!(!gen.on_island(px, pz), "{} seed {seed} wakes on an island", zone.name());
                for cz in (pz - 20_000).div_euclid(ISLAND_CELL)..=(pz + 20_000).div_euclid(ISLAND_CELL) {
                    for cx in (px - 20_000).div_euclid(ISLAND_CELL)..=(px + 20_000).div_euclid(ISLAND_CELL) {
                        let Some(group) = gen.island_candidate(cx, cz).and_then(|candidate| gen.island_group(cx, cz, candidate)) else {
                            continue;
                        };
                        for island in &group.islands[..group.count] {
                            let far = (island.x - f64::from(px)).hypot(island.z - f64::from(pz));
                            assert!(far > 8_000.0, "{} seed {seed}: an island {far:.0} blocks from where the player wakes", zone.name());
                        }
                    }
                }
            }
        }
    }

    /// **A mountain belt stands high and a lowland does not.** Where the
    /// country is the crest of a belt a good share of the land is over the
    /// mountain line and some of it far over; where it is a lowland almost
    /// none is.
    #[test]
    fn a_mountain_belt_stands_high_and_a_lowland_does_not() {
        let gen = WorldGen::new(1337);
        let (mut high, mut high_over, mut high_peak) = (0usize, 0usize, i32::MIN);
        let (mut low, mut low_over) = (0usize, 0usize);
        for cz in -100..100 {
            for cx in -100..100 {
                let (gx, gz) = (cx * 10_000 + 1_234, cz * 10_000 - 777);
                let highland = gen.highland(gx, gz);
                let crest = highland > 0.9;
                if !crest && highland > 0.3 || (crest && high > 4_000) || (!crest && low > 4_000) {
                    continue;
                }
                for dz in (-1_000..=1_000).step_by(500) {
                    for dx in (-1_000..=1_000).step_by(500) {
                        let height = gen.height_at(gx + dx, gz + dz);
                        if height < SEA_LEVEL {
                            continue;
                        }
                        if crest {
                            high += 1;
                            high_over += usize::from(height > SEA_LEVEL + 42);
                            high_peak = high_peak.max(height);
                        } else {
                            low += 1;
                            low_over += usize::from(height > SEA_LEVEL + 42);
                        }
                    }
                }
            }
        }
        assert!(high > 200 && low > 200, "the sweep found {high} crest and {low} lowland columns of land");
        let (high_share, low_share) = (high_over as f64 / high as f64, low_over as f64 / low as f64);
        println!("crest: {:.0}% over the mountain line, highest {high_peak}; lowland: {:.1}%", high_share * 100.0, low_share * 100.0);
        assert!(high_share > 0.25, "a mountain belt is only {:.0}% over the mountain line", high_share * 100.0);
        assert!(high_peak > SEA_LEVEL + 110, "the highest crest in the sweep is {high_peak}");
        assert!(low_share < 0.03, "a lowland is {:.1}% over the mountain line", low_share * 100.0);
    }

    /// **A brook is a stride, a river a swim, a great river a crossing.**
    /// Where each order's field crosses zero in water, the water is walked
    /// across along the field's slope -- square to the channel -- and the
    /// middle width of each order has to be the width its order promises.
    #[test]
    fn a_brook_a_river_and_a_great_river_are_each_as_wide_as_their_order() {
        let gen = WorldGen::new(1337);
        let wet = |gx: i32, gz: i32| gen.height_at(gx, gz) < SEA_LEVEL;
        for (index, (order, (narrowest, widest))) in EARTH_RIVERS.iter().zip([(100, 300), (15, 60), (2, 6)]).enumerate() {
            let stride = (0.02 / order.frequency).max(4.0);
            let mut widths: Vec<i32> = Vec::new();
            // A brook's zero inside a great river's water is the great
            // river's width; only crossings outside every wider order's
            // valley say anything about this order.
            let wider = &EARTH_RIVERS[..index];
            'lines: for line in 0..40 {
                let z = f64::from(line * 7_919 - 150_000);
                let mut last = gen.river_field(order, -400_000.0, z);
                let mut x = -400_000.0;
                while x < 400_000.0 {
                    x += stride;
                    let field = gen.river_field(order, x, z);
                    if (field > 0.0) == (last > 0.0) {
                        last = field;
                        continue;
                    }
                    last = field;
                    let (gx, gz) = (x as i32, z as i32);
                    if !wet(gx, gz) || gen.biome_at(gx, gz) != Biome::River {
                        continue;
                    }
                    if wider.iter().any(|w| gen.river_field(w, x, z).abs() <= RIVER_SLOPE_BOUND * w.frequency * w.valley) {
                        continue;
                    }
                    let e = 1.0;
                    let sx = gen.river_field(order, x + e, z) - gen.river_field(order, x - e, z);
                    let sz = gen.river_field(order, x, z + e) - gen.river_field(order, x, z - e);
                    let length = (sx * sx + sz * sz).sqrt().max(f64::EPSILON);
                    let (ux, uz) = (sx / length, sz / length);
                    let reach = |sign: f64| {
                        (1..600).take_while(|&d| wet((x + ux * sign * f64::from(d)) as i32, (z + uz * sign * f64::from(d)) as i32)).count() as i32
                    };
                    widths.push(1 + reach(1.0) + reach(-1.0));
                    if widths.len() >= 15 {
                        break 'lines;
                    }
                }
            }
            widths.sort_unstable();
            println!("order at {}: {} crossings, widths {widths:?}", order.frequency, widths.len());
            assert!(widths.len() >= 3, "only {} crossings of the order at {}", widths.len(), order.frequency);
            let middle = widths[widths.len() / 2];
            assert!(
                (narrowest..=widest).contains(&middle),
                "the order at {} is {middle} wide in the middle, not {narrowest}..={widest}",
                order.frequency
            );
        }
    }

    /// **A wide lake is a closed basin, as a pond is**, proved the same way:
    /// every cell of its water has ground or water beside it and under it in
    /// the generated blocks, or it drains on the first tick.
    ///
    /// The smallest lake near the origin of three seeds in the blocks, because
    /// a lake of seven hundred metres is two thousand chunks; and then every
    /// lake found, large ones included, column by column from the height and
    /// the waterline the chunks are built from -- which is where a sill that
    /// failed to rise or a skirt that dug under the water would show.
    #[test]
    fn a_wide_lake_holds_its_water_like_a_pond() {
        let mut checked = 0usize;
        let mut columns = 0usize;
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            let lakes: Vec<Lake> = (-6..6).flat_map(|cz| (-6..6).map(move |cx| (cx, cz))).filter_map(|(cx, cz)| gen.wide_lake_site(cx, cz)).collect();
            for lake in &lakes {
                // The water a chunk puts in a column: its lake's level over the
                // lake's footprint, the sea's elsewhere (`build_column_tile`).
                let wet = |gx: i32, gz: i32| {
                    let level = gen.lake_near(gx, gz).map_or(SEA_LEVEL, |(near, _)| near.water);
                    level == lake.water && gen.height_at(gx, gz) < level
                };
                let edge = wide_edge(lake.radius) + 1;
                for gz in lake.centre_z - edge..=lake.centre_z + edge {
                    for gx in lake.centre_x - edge..=lake.centre_x + edge {
                        if !wet(gx, gz) {
                            continue;
                        }
                        columns += 1;
                        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            assert!(
                                wet(gx + dx, gz + dz) || gen.height_at(gx + dx, gz + dz) >= lake.water,
                                "seed {seed}: the wide lake at {},{} (radius {}) spills from {gx},{gz} to the column beside it, at {} under water at {}",
                                lake.centre_x,
                                lake.centre_z,
                                lake.radius,
                                gen.height_at(gx + dx, gz + dz),
                                lake.water
                            );
                        }
                    }
                }
            }
            let Some(lake) = lakes.iter().min_by_key(|lake| lake.radius) else {
                continue;
            };
            let reach = wide_edge(lake.radius) + 2;
            let mut chunks = std::collections::HashMap::new();
            for tz in (lake.centre_z - reach).div_euclid(TILE)..=(lake.centre_z + reach).div_euclid(TILE) {
                for tx in (lake.centre_x - reach).div_euclid(TILE)..=(lake.centre_x + reach).div_euclid(TILE) {
                    chunks.insert((tx, tz), gen.generate_chunk(ChunkPos::new(tx, tz)));
                }
            }
            let get = |gx: i32, y: i32, gz: i32| {
                chunks
                    .get(&(gx.div_euclid(TILE), gz.div_euclid(TILE)))
                    .map(|chunk| chunk.get(gx.rem_euclid(TILE) as usize, y as usize, gz.rem_euclid(TILE) as usize))
            };
            let edge = wide_edge(lake.radius) + 1;
            for gz in lake.centre_z - edge..=lake.centre_z + edge {
                for gx in lake.centre_x - edge..=lake.centre_x + edge {
                    for y in SEA_LEVEL + 1..=lake.water {
                        if get(gx, y, gz) != Some(BLOCK_WATER) {
                            continue;
                        }
                        checked += 1;
                        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
                            assert_ne!(
                                get(gx + dx, y + dy, gz + dz),
                                Some(BLOCK_AIR),
                                "seed {seed}: the wide lake at {},{} has water at {gx},{y},{gz} beside air",
                                lake.centre_x,
                                lake.centre_z
                            );
                        }
                    }
                }
            }
        }
        assert!(checked > 2_000, "only {checked} cells of wide lake water in three seeds");
        assert!(columns > 50_000, "only {columns} columns of wide lake water in three seeds");
    }

    /// **A lake is as big as a lake and a pond as big as a pond**, a block a
    /// metre: every wide lake found over two hundred square kilometres of
    /// three seeds is eighty metres of water or more and five deep or more, a
    /// few are more than half a kilometre, and every pond is ten to forty.
    #[test]
    fn a_lake_is_hundreds_of_metres_across_and_a_pond_tens() {
        let (mut wide, mut large) = (0usize, 0usize);
        for seed in [1337u32, 42, 7] {
            let gen = WorldGen::new(seed);
            for cz in -8..8 {
                for cx in -8..8 {
                    let Some(lake) = gen.wide_lake_site(cx, cz) else {
                        continue;
                    };
                    wide += 1;
                    large += usize::from(lake.radius >= 250);
                    assert!(lake.radius * 2 >= 80 && lake.depth >= 5, "seed {seed}: a lake {} across and {} deep", lake.radius * 2, lake.depth);
                }
            }
            for cz in -20..20 {
                for cx in -20..20 {
                    if let Some(pond) = gen.lake_site(cx, cz) {
                        assert!((10..=40).contains(&(pond.radius * 2 + 1)), "seed {seed}: a pond {} across", pond.radius * 2 + 1);
                    }
                }
            }
        }
        assert!(wide >= 10, "only {wide} lakes in three seeds' six hundred square kilometres");
        assert!(large >= 1, "not one lake of half a kilometre in {wide}");
    }

    /// **A tree is as tall as its kind.** In the woods the generator actually
    /// grows, the trunks standing on each kind's ground reach the heights
    /// `WorldGen::trunks` promises -- measured in the blocks, as the column of
    /// wood straight up from the root, which is what a player cuts. An oak's
    /// is its trunk and branch height and the leader out of the top, nine to
    /// fourteen; a fir's is its trunk, ten to fourteen.
    ///
    /// Not the acacia, whose stem is two or three pieces straight and then
    /// crooked sideways: a straight column is not its height, and the plate
    /// is the acacia tests' business.
    #[test]
    fn a_tree_of_the_earth_is_as_tall_as_its_kind() {
        for (biome, shortest, tallest) in [(Biome::Forest, 9, 14), (Biome::Taiga, 10, 14)] {
            let gen = crate::worldgen::tests::world_for(1337, biome);
            let mut trunks: Vec<i32> = Vec::new();
            // Two dozen chunks: a savanna's acacias stand in groves with open
            // grass between, and six chunks of it held eight trees.
            for chunk in crate::worldgen::tests::chunks_in(&gen, biome, 24) {
                let (ox, oz) = (chunk.pos.x * CHUNK_SIZE_X as i32, chunk.pos.z * CHUNK_SIZE_Z as i32);
                for lz in 0..CHUNK_SIZE_Z {
                    for lx in 0..CHUNK_SIZE_X {
                        let (gx, gz) = (ox + lx as i32, oz + lz as i32);
                        let ground = gen.height_at(gx, gz);
                        if gen.biome_at(gx, gz) != biome || ground + 1 >= CHUNK_SIZE_Y as i32 {
                            continue;
                        }
                        let wood = |y: i32| {
                            let id = chunk.get(lx, y as usize, lz);
                            crate::wood::is_log(id) || is_branch(id)
                        };
                        if !wood(ground + 1) {
                            continue;
                        }
                        let height = (ground + 1..CHUNK_SIZE_Y as i32).take_while(|&y| wood(y)).count() as i32;
                        trunks.push(height);
                    }
                }
            }
            trunks.sort_unstable();
            assert!(trunks.len() >= 5, "only {} trunks found standing in {}", trunks.len(), biome.name());
            let middle = trunks[trunks.len() / 2];
            println!("{}: {} trunks, middle {middle}, from {} to {}", biome.name(), trunks.len(), trunks[0], trunks[trunks.len() - 1]);
            assert!(
                (shortest..=tallest).contains(&middle),
                "the middle trunk of {} is {middle}, not {shortest}..={tallest}",
                biome.name()
            );
        }
    }
}

#[cfg(test)]
mod island_scratch {
    use super::super::*;
    #[test]
    #[ignore = "scratch"]
    fn why_is_the_island_wet() {
        let gen = WorldGen::new(7);
        let (x, z) = (-126059, -644595);
        let basin = gen.basin(x, z);
        println!("open {} island {:?} continent {} base {} terrain {} height {} biome {:?}", gen.open_sea(x, z, basin), gen.island_field(x, z), gen.continent(x, z), gen.coast_profile(gen.continent(x, z), basin), gen.terrain_height(x, z), gen.height_at(x, z), gen.biome_at(x, z));
        for order in gen.river_orders() {
            println!("order {} field {}", order.half_width, gen.river_field(order, f64::from(x), f64::from(z)));
        }
    }
}
