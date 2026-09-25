//! Landforms: hill country, plain country, rivers that drain, and relief
//! that follows the rock. What `Scale::Landforms` adds to the Earth's scale.
//!
//! ## What was asked
//!
//! "Сделай новый биом холмов и равнин", "сделай генерацию реалистичнее". The
//! Earth-scale world already had its climate provinces, its mountain belts
//! and its rivers by order; what it did not have was *landforms* -- country
//! whose shape is a kind of country. Every lowland was the same ridge field
//! turned down, with the same benches on its slopes, so a meadow and a forest
//! stood on one relief and a player could not tell from the ground what sort
//! of country they were in, only from what grew on it.
//!
//! ## Three things, each chosen for what a player sees and decides
//!
//! * **Hill country and plain country** (`landform`). A field a few
//!   kilometres across says which a lowland is. In hill country the ridges
//!   are smoothed away and rounded domes rise in their place -- downs, twenty
//!   or thirty blocks from fold to crown, slopes a person walks up without a
//!   jump -- and the brooks run in the folds, because the folds are down at
//!   the floodplain where the water already stands. In plain country every
//!   term of the relief is turned down to a swell, and what is left is a
//!   horizon. Measured (`a_down_country_rolls_and_a_steppe_lies_flat`): the
//!   ground a quarter of a kilometre round a steppe rises and falls ten
//!   blocks, round a down thirty-five, round an ordinary meadow forty -- and
//!   a meadow has six times the downs' share of steps a player must jump. Each is a biome where the climate allows (`Biome::Hills`,
//!   `Biome::Steppe`), so the country a horse herd or a shepherd wants is a
//!   place on the map, not a statistic.
//! * **Rivers that run out to the sea** (`drained_field`). Every river of the
//!   Earth's scale is the zero line of a noise field, and a noise field's
//!   zero lines close on themselves round every hill and hollow of the field:
//!   a river can run in a ring, and a brook between two rises is a ditch
//!   closed at both ends -- half the Earth's brooks, followed from where a
//!   player meets them (`probe_river_ends`). Here a river is the zero of a
//!   wave laid across the country with noise on it, and a wave's zero lines
//!   are open -- each one crosses the continent from coast to coast, or to
//!   the hills where it rises -- and a brook runs only near the river it
//!   feeds (`tributary_reach`). A player who follows water downstream
//!   reaches the sea, or a wider river that does.
//! * **Relief that follows the rock** (`hardness`). The same field that says
//!   which rock a country is made of (`WorldGen::stratum`'s region) now also
//!   says how hard it wears: the ridges and downs on the hard end of it
//!   stand higher, and its soft end is shale (`WorldGen::stratum`) -- the
//!   vales. So a ridge is a place to look for the hard rock, and a vale is
//!   where the shale is: the rock under a hill is a reason for the hill.
//!
//! ## Rejected
//!
//! * *A biome that shapes its own terrain.* The classifier comes after the
//!   height (see `Biome`), and a biome that pushed the ground puts a cliff on
//!   every border. Here the landform shapes both: the field decides the
//!   relief and the biome reads the same field, so the border between hill
//!   and plain is a slope, not a step.
//! * *Real flow routing* -- a drainage graph found by walking downhill. The
//!   generator is a function of the column (a chunk is made without asking
//!   its neighbours), and routing is a question about a whole basin. Every
//!   channel here holds water at the sea's level anyway (`EARTH_RIVERS`), so
//!   what the player can see of "does this drain" is whether the channel is
//!   open to the sea, and an open line is.
//! * *More octaves on the old river field.* Loops are a property of every
//!   smooth field's zero set; no amount of detail opens them.
//!
//! ## Old worlds
//!
//! An Earth world keeps every number it had: every hook here answers
//! nothing unless the scale is `Landforms`, and
//! `an_old_worlds_new_chunks_are_the_old_generators_to_the_block` holds four
//! chunks of each older generator to the block.

use std::f64::consts::TAU;

use noise::NoiseFn;

use super::scale::{turned, RiverOrder, Scale, EARTH_RIVERS};
use super::{fbm, smoothstep, WorldGen};

/// The lattice spacing of the landform field, in blocks: a hill country or a
/// plain is a few kilometres across, so a day's walk crosses two or three,
/// and the first of each is within reach of a new player -- the field is
/// zero at every origin (`scale::far`), which is neither, and it leaves that
/// middle within a couple of kilometres in some direction.
pub(super) const LANDFORM_SPACING: f64 = 9_000.0;

/// Where the landform field turns a lowland into plain country and into hill
/// country: all of neither between the inner numbers, all of one past the
/// outer. Symmetric, so a seed has as much of one as of the other.
const LANDFORM_EDGE: (f64, f64) = (0.10, 0.34);

/// How high a down stands over its folds at its fullest, in blocks, and how
/// fast the domes turn over.
///
/// **Thirty over about two hundred metres**, a little more on hard rock, and
/// eased through a smoothstep so the flank is steepest half way up and gentle
/// at crown and foot: a climb, and nowhere near the angle a slope sheds its
/// turf at (`BANK_SLOPE`), so a hill is grass to the crown.
///
/// **Forty was tried**, and a hill country's crowns stood over the mountain
/// line: 5.4 per cent of all lowland, and 3.4 at thirty, where
/// `a_mountain_belt_stands_high_and_a_lowland_does_not` allows three. So the
/// downs are also laid only on low ground (`land_before_rivers`), which is
/// where a rolling country is anyway.
const DOWNS_HEIGHT: f64 = 30.0;
const DOWNS_FREQUENCY: f64 = 0.0026;

/// How tall the grain of open country stands, in blocks, and how far apart
/// its crests are.
///
/// ## What the grain is for
///
/// "Слои ландшафта странные и слишком гладкие". The ground is a height field
/// rounded to whole blocks, so every slope is a set of contour bands, and how
/// wide those bands are is entirely a question of how fast the field changes.
/// Measured on the landforms before this existed
/// (`probe_banding`, seed 1337): on a steppe **80 per cent** of columns had
/// all four neighbours at their own height and a band ran **22 blocks** along
/// a line; in a forest 89 per cent and 18 blocks. That is not a hillside, it
/// is a contour map -- wide flat shelves with a rim of quarter steps round
/// each (`lips`), which from the ground reads as terraces nobody cut. The
/// player saw exactly that and called them layers.
///
/// The cause is that every term of the old relief is a *country-sized* field.
/// The one term with a few blocks' wavelength in it is `detail`, and `detail`
/// is scaled by `unworn` -- so on worn lowland, which is all of the country a
/// landform is, it is six tenths of a block: less than the rounding, and
/// therefore nothing. A worn lowland was drawn as the algebra says a worn
/// lowland is, a plane, and forgot that a real one is still made of soil that
/// slumps, rock that stands out of it and water that has run over it.
///
/// So: one more band of scale, about twenty blocks to a crest and a block or
/// so either way of the ground -- five at the field's fullest, which is rare
/// and is what a knuckle of rock on a hillside is -- laid over the landforms'
/// country and *not* turned off by `unworn`. It does not change where a hill
/// is -- the quarter-kilometre relief of a steppe moves from nine to eleven
/// blocks and a down's stays at thirty-five -- but it breaks every contour
/// line into a coast. Measured (`probe_banding`, seed 1337, before → after):
///
/// ```text
///          shelf share     band, blocks    grain
///  steppe  80.3% → 56.3%   21.9 → 7.2    0.23 → 0.53
///  forest  88.8% → 59.1%   17.6 → 7.1    0.28 → 0.47
///   downs  52.0% → 45.1%    7.7 → 5.7    0.40 → 0.48
///  meadow  42.7% → 24.8%    4.9 → 3.1    0.40 → 0.65
/// ```
///
/// **The wavelength is chosen so the lips can still do their work.** A lip
/// ramps a terrace up to four columns wide (`lips::REACH`) and leaves a
/// narrower one the stair it is. The grain's own bands come out three to
/// seven columns across, which is that width: the country gained its shape
/// back and every rise in it is still a ramp rather than a hop. At five
/// blocks to a crest the same height is a seam of a block *everywhere*,
/// every one of them a terrace one column wide that no lip can ramp --
/// gravel to walk over, not ground.
///
/// **Five, and seven and a half was too many.** The bound is
/// `a_hillside_is_a_stack_of_shelves_rather_than_a_flight_of_walls`: no seam
/// over five blocks anywhere, and under three per cent of the *lowland's*
/// seams needing more than a jump, because the lowland is where people walk.
/// Measured over its three seeds (`probe_slopes`, before → after): the worst
/// seam in a wide sample 2 / 4 / 3 blocks → 4 / 4 / 4, and 0.3 / 1.4 / 1.1
/// per cent of seams two or more → 0.9 / 1.6 / 2.1. At seven and a half a
/// seam came out six blocks tall and the test went red. Six passed and
/// bought a tenth of a block of band for half the remaining margin, which is
/// not a trade.
///
/// And the ground is still mostly flat underfoot: 70 per cent of seams in a
/// world are no step at all, against 78 before, and what the grain turned
/// into steps it turned into steps of one -- which is a quarter-block ramp
/// wherever a lip can reach it.
const GRAIN_HEIGHT: f64 = 5.0;
const GRAIN_FREQUENCY: f64 = 0.048;

/// How much of the grain the softest rock keeps and how much the hardest
/// adds: a vale of shale is worn smooth and a hard-rock hillside stands in
/// knuckles and small outcrops. The same region field as `hardness`, so the
/// rough country and the high country are one country and a player reading a
/// hillside is reading its rock.
const GRAIN_BY_ROCK: (f64, f64) = (0.55, 0.9);

/// How much higher the ridges stand on the hardest rock than on the rest.
///
/// **Up on hard rock, never down on soft.** A factor from 0.8 on shale to
/// 1.25 on gneiss was tried first, and the region field turns over every two
/// hundred blocks -- so it took a fifth off the ridges of half of every
/// hillside, and the faces those ridges held up went with it: the scree
/// under them went from a find to none in the hundred and sixty-nine chunks
/// round a spawn. A vale is low because the ridge beside it is high; the soft
/// ground does not have to sink for that.
const HARD_ROCK_GAIN: f64 = 0.4;

/// The Earth's three orders, drawn as courses. Every width, depth, valley and
/// current is the Earth's; only the shape of the field is new.
///
/// **The three run at three angles**, a little over fifty degrees apart, so
/// a brook meets its river and a river its great river at the slant a
/// tributary joins at, rather than running beside it forever.
///
/// **And the river lies a little off every origin**, where the great river
/// lies as far off as it can: a quarter of a wave puts an origin half way
/// between two courses, which for the great river is the Earth's own rule (a
/// floodplain under every first day is a first day with no hill in it) --
/// and for the river is the rule that emptied the spawn of water, because
/// the brooks run only near their river (`TRIBUTARY_FIELD`) and half way
/// between two rivers is as far from every brook as the country gets. At
/// eight hundredths of a wave the river is about six hundred metres off,
/// outside its own valley, and its brooks come to within a walk of the fire.
pub(super) const LANDFORM_RIVERS: [RiverOrder; 3] = [
    RiverOrder { course: Some((0.0, 0.25)), ..EARTH_RIVERS[0] },
    RiverOrder { course: Some((0.95, 0.08)), ..EARTH_RIVERS[1] },
    RiverOrder { course: Some((1.85, 0.25)), feeds: Some(1), ..EARTH_RIVERS[2] },
];

/// How far from the river it feeds a brook runs, as the river's field: all of
/// a brook under the first, none over the second.
///
/// **Brooks are tributaries or they are trenches.** Every channel holds its
/// water at the sea's level, so a brook between two rises is a ditch full of
/// standing water closed at both ends -- and `probe_river_ends` found that
/// about half the brooks of the Earth's scale were exactly that. A brook only
/// near its river, running across the river's line at the slant its course is
/// turned to (`LANDFORM_RIVERS`), is a brook that reaches that river: a line
/// crossing a strip crosses the line in the middle of it. Measured over eight
/// seeds: 21 brooks in 43 closed at both ends before, 8 in 27 here. What it
/// costs is the brooks of the country between the rivers, which is interfluve
/// -- the dry ground a real brook drains *from* -- and a walk there is a walk
/// on which water has to be carried or found.
///
/// **Neither wider nor narrower.** At 0.15 .. 0.22 twelve brooks in thirty
/// were still trenches; at 0.10 .. 0.16 the country round a spawn held too
/// little water for the tests that count its banks and its crossings to find
/// twenty of either -- and a brook is the water a player camps by.
///
/// The field rather than a distance, because the field is in hand for the
/// price of one sample and the distance is five: at these numbers the strip is
/// about six hundred metres either side of a river whose neighbours are three
/// and a half kilometres apart.
const TRIBUTARY_FIELD: (f64, f64) = (0.12, 0.18);

/// How much of a course is the wave that keeps it open, and how much is the
/// noise that makes it wander.
///
/// **The wave has to win.** A zero line of `wave + noise` is a single open
/// line across each band of the wave as long as the wave changes faster
/// across the band than the noise can -- and at these weights it does by a
/// factor of two and more (see the module note of `drained_field`). More
/// noise than this and small rings come back where the noise is steep.
const COURSE_WAVE: f64 = 0.6;
const COURSE_NOISE: f64 = 0.4;

/// The whole field turned down to this, so its slope stays under
/// `scale::RIVER_SLOPE_BOUND`. A field and any multiple of it have the same
/// zero lines and the same distance to them (the distance is the field over
/// its slope), so this changes nothing on the ground; it only keeps the cull
/// honest. Measured by `the_river_cull_is_steeper_than_the_river_field_ever_gets`.
const COURSE_GAIN: f64 = 0.5;

/// How far a course bends over the country, as multiples of the order's own
/// spacing: the wave's lines are bent by a slow field this many spacings
/// across and this many deep, so a great river swings through a province
/// and a brook across a hillside, each at its own size.
///
/// **A fifth of the wavelength deep, and no more**: the bend must never fold
/// a line back on itself, which it cannot while the bend field's slope times
/// this ratio is under one -- and a Perlin field's steepest is about two and
/// a half.
const BEND_WAVELENGTHS: f64 = 8.0;
const BEND_DEPTH: f64 = 1.6;

impl WorldGen {
    /// How much of a plain and how much of a hill country a column is in, each
    /// 0 .. 1 and never both. Nothing in a mountain country, which is a
    /// country of its own, and nothing in a world without landforms.
    pub(super) fn landform(&self, gx: i32, gz: i32, highland: f64) -> (f64, f64) {
        if self.scale != Scale::Landforms {
            return (0.0, 0.0);
        }
        let field = self.landform_field(gx, gz);
        // A lowland or a middling upland; the belts keep their ranges.
        let low = smoothstep(0.66, 0.50, highland);
        let (near, full) = LANDFORM_EDGE;
        (smoothstep(-near, -full, field) * low, smoothstep(near, full, field) * low)
    }

    /// The region field that picks a country's rock (`stratum`), and now how
    /// hard that rock wears. One field for both, so the rock under a ridge is
    /// the hard one.
    pub(super) fn rock_region(&self, gx: i32, gz: i32) -> f64 {
        fbm(&self.strata_noise, gx as f64 - 3301.0, gz as f64 + 4409.0, 0.0045, 1)
    }

    /// How much the rock here holds the relief up: 1 on the shale of a vale
    /// and anything middling, up to `1 + HARD_ROCK_GAIN` on the hard rock of
    /// a ridge.
    pub(super) fn hardness(region: f64) -> f64 {
        1.0 + HARD_ROCK_GAIN * smoothstep(0.0, 0.4, region)
    }

    /// How tall the grain stands, which is `GRAIN_HEIGHT` unless
    /// **`PRIMITIVE_GRAIN=<blocks>`** says otherwise.
    ///
    /// The hook exists because the whole of this change is a before and an
    /// after of the same country, and a before that has to be reached by
    /// checking out an older tree is a before nobody re-runs. With
    /// `PRIMITIVE_GRAIN=0` the generator draws the ground it drew before the
    /// grain existed, to the block, and
    /// `scenario::tests::a_picture_of_the_open_country` renders the same
    /// four bearings from the same pinned spawn in either -- which is how
    /// the pictures in `CHANGELOG.md` were made, and how the numbers in
    /// `GRAIN_HEIGHT` were chosen. It reads the variable once for the life
    /// of the process, so no chunk of one world is drawn at two settings.
    ///
    /// This is the same kind of hook as `PRIMITIVE_TEST_SCALE`, and it is
    /// there for the same reason: a thing that can only be checked by hand
    /// is a thing that stops being checked.
    fn grain_height() -> f64 {
        static TUNE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
        *TUNE.get_or_init(|| std::env::var("PRIMITIVE_GRAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(GRAIN_HEIGHT))
    }

    /// The grain of the country: the fine relief a worn lowland keeps, in
    /// blocks either way. See `GRAIN_HEIGHT` for what it is for and what the
    /// ground looked like without it.
    ///
    /// **A field of its own, not another octave of `detail`.** `detail` is
    /// scaled by `unworn` from end to end -- that is the whole of what makes
    /// a floodplain read as a floodplain and a ridge line as a ridge line --
    /// and the grain is the opposite claim: it is the relief that survives
    /// being worn down, because soil slumps and rock stands out of it long
    /// after the hill they are on has gone. Carried as one more octave it
    /// would have had to be exempted from the one factor that term exists
    /// for; and two octave bands a factor apart in one `fbm` line up into a
    /// ridge where their crests agree.
    pub(super) fn grain(&self, gx: i32, gz: i32, region: f64) -> f64 {
        let field = fbm(&self.grain_noise, gx as f64 - 2_221.0, gz as f64 + 6_133.0, GRAIN_FREQUENCY, 3);
        let (soft, hard) = GRAIN_BY_ROCK;
        field * Self::grain_height() * (soft + hard * smoothstep(-0.35, 0.35, region))
    }

    /// The downs of hill country: rounded domes 0 .. `DOWNS_HEIGHT`, higher
    /// on hard rock. Before the hill country's weight is laid on.
    ///
    /// **A smoothstep of the field, not the field**: the field alone is as
    /// much hollow as dome, and a hollow under the floodplain floods. Eased
    /// through a smoothstep its low half is a flat fold between the hills and
    /// its high half a rounded crown, which is what a rolling country is --
    /// convex tops, concave feet.
    ///
    /// **Three octaves, and it was two.** Two octaves of Perlin eased through
    /// a smoothstep is a field of domes that are all the same dome: the same
    /// round crown, the same even flank, the same two hundred metres from
    /// fold to fold, and a player crossing a down country walked over one
    /// hill eleven times. The third octave is a quarter of the height at a
    /// quarter of the wavelength -- four blocks of swell over fifty -- which
    /// is too little to steepen a flank into a climb and enough to give each
    /// dome a shoulder, a false crest and a side that falls away faster than
    /// the other.
    pub(super) fn downs(&self, gx: i32, gz: i32, region: f64) -> f64 {
        let field = fbm(&self.hill_noise, gx as f64 + 7_717.0, gz as f64 - 5_503.0, DOWNS_FREQUENCY, 3);
        smoothstep(-0.45, 0.45, field) * DOWNS_HEIGHT * (0.75 + 0.5 * smoothstep(-0.35, 0.35, region))
    }

    /// A river order's field drawn as courses: zero along lines that cross
    /// the country and never close.
    ///
    /// ## Why a wave keeps the lines open
    ///
    /// `sin(2π f v)` is zero along parallel lines `1 / 2f` apart, where `v`
    /// is the distance across the courses. Noise added to it moves each line
    /// sideways, and cannot close one into a ring or join two while the wave
    /// changes across the line faster than the noise does: where the sum is
    /// zero, `|sin|` is at most `COURSE_NOISE / COURSE_WAVE`, so the wave's
    /// own slope there is at least three quarters of its steepest, which is
    /// more than twice the noise's.
    ///
    /// `v` is bent by a slow field (`BEND_WAVELENGTHS`), so the courses
    /// swing over the country instead of lying ruled across it, and the whole
    /// is turned by the order's own angle and the world's, so no two worlds
    /// drain the same way.
    ///
    /// The meander is the Earth's, laid on after: a warp moves the lines but
    /// cannot tear one, so they stay open.
    pub(super) fn drained_field(&self, order: &RiverOrder, x: f64, z: f64, shift: f64, (course, phase): (f64, f64)) -> f64 {
        let frequency = order.frequency;
        let (x, z) = match order.meander {
            Some((wavelength, reach)) => (
                x + self.warp_noise.get([x / wavelength + shift, z / wavelength - shift]) * reach,
                z + self.warp_noise.get([x / wavelength - shift - 17.0, z / wavelength + shift + 17.0]) * reach,
            ),
            None => (x, z),
        };
        let (sin, cos) = match self.drain.iter().find(|(known, _)| *known == course) {
            Some(&(_, turned)) => turned,
            None => (course + self.drain_turn()).sin_cos(),
        };
        let spacing = 1.0 / frequency;
        let (bx, bz) = turned(x / (spacing * BEND_WAVELENGTHS), z / (spacing * BEND_WAVELENGTHS), 2);
        let bend = self.warp_noise.get([bx + shift * 2.0, bz - shift * 2.0]) * spacing * BEND_DEPTH;
        let across = x * sin + z * cos + bend;
        // Off by the order's phase, so no origin -- where the bend is zero --
        // lies on a course: the Earth's rivers learned that a river through
        // every spawn is a floodplain under every first day (`river_field`).
        let wave = (TAU * (frequency * across + phase)).sin();
        // The Earth's two octaves, as its own river field reads them: the
        // second is what bends a channel within sight. With the first alone
        // the courses swung only as wide as the meander, and the outside of a
        // bend came out barely steeper than the inside -- there were too few
        // tight bends to cut a bank into (`the_outside_of_a_bend_is_steeper_than_the_inside`).
        let (ax, az) = turned(x * frequency, z * frequency, 0);
        let (bx, bz) = turned(x * frequency * 2.0, z * frequency * 2.0, 1);
        let first = self.river_noise.get([ax + shift + 0.5, az - shift + 0.37]);
        let second = self.river_noise.get([bx + shift + 31.29, bz - shift - 30.55]);
        let noise = (first + second * 0.5) / 1.5;
        ((COURSE_WAVE * wave + COURSE_NOISE * noise) * COURSE_GAIN).clamp(-1.0, 1.0)
    }

    /// How much of a tributary runs here: 1 near the river it feeds, easing to
    /// nothing a little under a kilometre out. See `TRIBUTARY_FIELD`.
    pub(super) fn tributary_reach(&self, order: &RiverOrder, x: f64, z: f64) -> f64 {
        let Some(parent) = order.feeds else {
            return 1.0;
        };
        let field = self.river_field(&self.river_orders()[parent], x, z).abs();
        smoothstep(TRIBUTARY_FIELD.1, TRIBUTARY_FIELD.0, field)
    }

    /// Every landform order's course with the world's turn on it, as a sine
    /// and a cosine: worked out once when the generator is made.
    ///
    /// **It was worked out at every sample of every river field** -- a hash
    /// of the seed and a sine and cosine, five to seven times a column for
    /// each of three orders, for three numbers fixed when the world was
    /// made. The same numbers from the same sum, so not a block moves.
    pub(super) fn drain_courses(&self) -> [(f64, (f64, f64)); 3] {
        LANDFORM_RIVERS.map(|order| {
            let course = order.course.map_or(0.0, |(course, _)| course);
            (course, (course + self.drain_turn()).sin_cos())
        })
    }

    /// The world's own turn of every course, in radians, off its seed: one
    /// world drains north-east and the next south.
    fn drain_turn(&self) -> f64 {
        f64::from(super::hash2(0, 0, self.seed.wrapping_add(0xD2A1)) % 6_283) / 1_000.0
    }
}

// ---------------------------------------------------------------------
// What the ground tells a newcomer
// ---------------------------------------------------------------------
//
// **The first hour used to be led by a tab and a line of text.** A new
// player opened the journal, read that flint is found by water, and walked
// until the prompt went away. Everything below is the same hour led by the
// country instead: the shingle at a river's edge, the paths something has
// worn to the pond, the talus round a cave mouth, the stain of copper on a
// hillside. None of it says anything. It is all things to notice, and every
// one of them is a reason to walk somewhere.
//
// **Landforms only**, every one of them, because an old world's chunks are
// held to the block by
// `an_old_worlds_new_chunks_are_the_old_generators_to_the_block`.
//
// Rejected: **markers.** A cairn at a cave mouth, a post on a trail, a pile
// of ore at an outcrop -- all of them read as somebody else's world rather
// than as the world, and all of them are the text tab again with a mesh
// instead of a sentence. What a player learns from a bank of gravel is that
// gravel means water; what they learn from a signpost is to look for
// signposts.

/// How far from the waterline the shingle reaches, in columns, and how far
/// above it.
///
/// **Three columns wide and two high.** Two was a hem nobody saw from a
/// hill; four crept up the bank into the grass and stopped reading as a
/// waterline at all. Three is about what a river leaves when it drops a
/// foot in the summer, and from a rise it draws the course of the water as
/// a pale line through the green -- which is the point: a player on high
/// ground should see where the water is before they can see the water.
const SHINGLE_REACH: i32 = 3;
const SHINGLE_RISE: i32 = 2;

/// One loose thing in every this many columns of the strip, and one in this
/// many of those is flint.
///
/// Against `PEBBLE_SPACING`'s twenty-six on ordinary ground: nine times as
/// thick, which is the difference that makes the band a band. Two thirds of
/// what lies there is the rock's own gravel and pebble and the other third
/// is flint, so a hundred blocks of shore is about a dozen nodules -- a
/// morning's knapping, and no tab had to say so.
const SHINGLE_SPACING: u32 = 3;
const SHINGLE_FLINT_SHARE: u32 = 3;

/// How far a game trail runs back from the water, in blocks, and how wide
/// it is either side of its line.
///
/// **Twenty-six and one.** The trail has to be long enough that a player
/// standing on it is not already at the pond -- otherwise it is a halo, not
/// a path -- and short enough that it reads as *converging* rather than as
/// a road going somewhere. A metre either side of the line is two columns
/// of bare ground: a path a person walks down without thinking about it,
/// and one something walked because it is easier than the grass.
const TRAIL_REACH: f64 = 26.0;
const TRAIL_HALF_WIDTH: f64 = 1.0;

/// How far a trail wanders off its bearing, in radians, and how quickly.
///
/// **Straight lines were tried first and they are unmistakably drawn.** A
/// path that runs dead straight for twenty-six blocks is a path somebody
/// laid; a quarter of a radian of weave over the same distance is a path
/// something walked. The weave is a function of the distance from the water
/// and of nothing else, so every column of one path agrees about where the
/// path is -- a wander that read the column's own position would be a
/// dotted line.
const TRAIL_WANDER: f64 = 0.24;
const TRAIL_WEAVE: f64 = 0.055;

/// How many paths come in to one watering place: three, and up to three
/// more.
///
/// Fewer than three and it is not a convergence; more than six and the
/// ground round a pond is more path than meadow, which is a lawn.
const TRAIL_PATHS: (u32, u32) = (3, 4);

/// How far the talus spreads from a hole in the ground, in columns.
const SCREE_REACH: i32 = 2;

/// One lump of the rock's own cobble in this many columns of the talus, and
/// one pebble in this many of what is left.
const SCREE_LUMP_SPACING: u32 = 3;
const SCREE_PEBBLE_SPACING: u32 = 2;

// Rejected: **scraping the turf off the ring round the hole.**
//
// It was written, it looked exactly right, and it broke four invariants that
// have nothing to do with caves: the flowers the ground cover had already
// grown were left standing on cobble, a longhouse's floor stopped being the
// floor its site had planned, the savanna's dry grass was found on stone,
// and `lay_lips` -- which reads the surface to decide what to cut -- dropped
// the ground out from under a feature. What a decoration pass may do is put
// things *on* the ground; what the ground *is* belongs to
// `build_column_tile`, and a pass that runs after the plants have been grown
// cannot change what they are standing on.
//
// So the colour comes from the talus itself: gravel and pebble of the
// country's own rock, thick at the lip and thinning out, which against turf
// is the pale patch the ask was asking for and costs nothing else.

/// One outcrop candidate in this many columns, before the ground is asked.
///
/// **Rare on purpose, and the rarity is the mechanic.** An outcrop is not
/// where a player gets their copper -- a handful of blocks is not a
/// smelting -- it is where they learn that *this* hill has copper in it,
/// and the decision it creates is whether to come back with a pick or keep
/// walking. Common outcrops would make the hills a shop; one to a few
/// hundred blocks of high ground makes them a map.
const OUTCROP_SPACING: u32 = 700;

/// What an outcrop needs: a face rather than a meadow, well above the
/// water, and high enough that the rock under it is carrying copper
/// (`WorldGen::copper_country`, which is a ramp on altitude -- so "the
/// copper is over the hill" is true of the ground and not only of the ore
/// table).
const OUTCROP_SLOPE: f32 = 0.85;
const OUTCROP_ABOVE_WATER: i32 = 12;
const OUTCROP_COUNTRY: f64 = 0.3;

/// How deep an outcrop goes: a lens, not a pebble. Three cells down and a
/// ragged four-neighbour spread, so it is a patch a player sees from across
/// a valley rather than a block they walk past.
const OUTCROP_DEPTH: i32 = 3;

/// A watering place, and the paths that come in to it.
///
/// **A lake and not a river**, and that is a decision rather than an
/// oversight: a trail is a line something walks to get to water, and a
/// river is already a line -- animals meet it wherever they happen to be
/// standing, so there is nothing for paths to converge *on*. A pond is a
/// point, and a point is what makes a convergence read as one.
#[derive(Clone, Copy, Debug)]
pub(super) struct Watering {
    centre_x: i32,
    centre_z: i32,
    /// Where the water ends, roughly: the lake's own radius. The trails
    /// begin outside it, because inside it is the lake.
    shore: i32,
    /// This place's own number, off its centre: how many paths and where
    /// they come in from.
    salt: u32,
}

impl Watering {
    /// Where the water is and how far it reaches. For the tests, which have
    /// to be able to say "and the paths reach *this*".
    #[cfg(test)]
    pub(super) fn centre_x(&self) -> i32 {
        self.centre_x
    }
    #[cfg(test)]
    pub(super) fn centre_z(&self) -> i32 {
        self.centre_z
    }
    #[cfg(test)]
    pub(super) fn shore(&self) -> i32 {
        self.shore
    }

    /// Is this column worn bare by whatever walks to the water?
    pub(super) fn on_a_trail(&self, gx: i32, gz: i32) -> bool {
        let (dx, dz) = (f64::from(gx - self.centre_x), f64::from(gz - self.centre_z));
        let far = dx.hypot(dz);
        let shore = f64::from(self.shore);
        if far <= shore || far > shore + TRAIL_REACH {
            return false;
        }
        let bearing = dz.atan2(dx);
        let paths = TRAIL_PATHS.0 + self.salt % TRAIL_PATHS.1;
        (0..paths).any(|path| {
            let spoke = f64::from(
                super::hash2(self.centre_x, self.centre_z, self.salt ^ (path.wrapping_mul(0x9E37) | 1)) % 6_283,
            ) / 1_000.0;
            let aim = spoke + TRAIL_WANDER * (far * TRAIL_WEAVE + f64::from(path)).sin();
            let turn = bearing - aim;
            // On the near side of the water only. Without this the same
            // line is a path coming in from both directions at once, which
            // is a road through the pond.
            turn.cos() > 0.0 && (turn.sin() * far).abs() <= TRAIL_HALF_WIDTH
        })
    }
}

impl WorldGen {
    /// The watering places whose trails could cross a chunk, worked out
    /// once for the chunk.
    ///
    /// **Once, and not once a column**, because a lake's site is judged off
    /// forty columns of terrain and asking for it two hundred and fifty-six
    /// times a chunk would put the cost of the trails on every column in
    /// the world. The cells are walked the way `wide_lake_within` walks
    /// them.
    pub(super) fn waterings_near(&self, origin_x: i32, origin_z: i32) -> Vec<Watering> {
        if self.scale != Scale::Landforms {
            return Vec::new();
        }
        let reach = TRAIL_REACH.ceil() as i32;
        let (from_x, to_x) = (origin_x - reach, origin_x + super::CHUNK_SIZE_X as i32 + reach);
        let (from_z, to_z) = (origin_z - reach, origin_z + super::CHUNK_SIZE_Z as i32 + reach);
        let cell = self.pond_cell();
        let mut found = Vec::new();
        for cell_z in from_z.div_euclid(cell)..=to_z.div_euclid(cell) {
            for cell_x in from_x.div_euclid(cell)..=to_x.div_euclid(cell) {
                if let Some(lake) = self.lake_site(cell_x, cell_z) {
                    found.push(Watering {
                        centre_x: lake.centre_x,
                        centre_z: lake.centre_z,
                        shore: lake.radius,
                        salt: super::hash2(lake.centre_x, lake.centre_z, self.seed.wrapping_add(0x7BA1)),
                    });
                }
            }
        }
        found
    }

    /// What lies loose on the shingle at this column, if anything.
    ///
    /// **Fresh water only.** The sea has a beach of its own and the ask was
    /// the rivers and the lakes: a shingle bank means "there is water here
    /// you can drink and there is flint in it", and a salt shore means
    /// neither. A flooded column whose water stands off the sea's level is
    /// a lake or a pool; a `Biome::River` column is the channel itself --
    /// the biome is never the bank, which is the trap `on_a_riverbank` is
    /// written round.
    ///
    /// The die is rolled before the strip is looked for, on the rule the
    /// rusty stones are written to: the look is forty-nine cache reads and
    /// the die is one hash, so an ordinary column pays one hash.
    pub(super) fn shingle_at(
        &self,
        columns: &super::ColumnCache,
        local: (i32, i32),
        world: (i32, i32),
        height: i32,
        rock: crate::types::BlockId,
    ) -> Option<crate::types::BlockId> {
        let ((lx, lz), (gx, gz)) = (local, world);
        if self.scale != Scale::Landforms
            || !super::hash2(gx, gz, self.seed.wrapping_add(0x5417)).is_multiple_of(SHINGLE_SPACING)
        {
            return None;
        }
        let fresh = (-SHINGLE_REACH..=SHINGLE_REACH).any(|dz| {
            (-SHINGLE_REACH..=SHINGLE_REACH).any(|dx| {
                let near = columns.at(lx + dx, lz + dz);
                near.height < near.water
                    && (near.water != super::SEA_LEVEL || near.biome == super::Biome::River)
                    && height <= near.water + SHINGLE_RISE
            })
        });
        if !fresh {
            return None;
        }
        // A third of it flint, and the rest the country's own stone, so the
        // band is the colour of the rock it was worn off and the flint is a
        // find in it rather than a carpet.
        if super::hash2(gz, gx, self.seed.wrapping_add(0xF117)).is_multiple_of(SHINGLE_FLINT_SHARE) {
            Some(crate::types::BLOCK_FLINT)
        } else if super::hash2(gx, gz, self.seed.wrapping_add(0x6BA7)).is_multiple_of(2) {
            Some(crate::ground::rubble_of(rock, crate::ground::Form::Gravel))
        } else {
            Some(crate::ground::rubble_of(rock, crate::ground::Form::Pebble))
        }
    }

    /// **The talus round a hole in the ground.**
    ///
    /// A cave that opens on a hillside is not a doorway in a lawn: the turf
    /// goes for a couple of columns round the lip, the rock under it shows,
    /// and what has fallen out of the roof lies in a fan below. Before
    /// this, a cave mouth in open country was a black rectangle in
    /// unbroken grass, and a player walked past a dozen of them for every
    /// one they noticed.
    ///
    /// **Run after `place_ground_cover`**, because the talus beats the
    /// tuft: ground cover writes one thing into the cell over the ground
    /// and stops, so a mouth surrounded by grass would stay surrounded by
    /// grass if this ran first.
    ///
    /// The holes are found once, for the chunk and one column of margin,
    /// and then read as a grid. Asked per column instead it is four
    /// `is_cave` samples a column -- eight Perlin lookups -- for the whole
    /// world, to find the handful of columns that are near a hole.
    pub(super) fn place_cave_mouths(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &super::ColumnCache,
    ) {
        if self.scale != Scale::Landforms {
            return;
        }
        let span = super::CHUNK_SIZE_X as i32 + 2 * SCREE_REACH;
        let mut holed = vec![false; (span * span) as usize];
        for lz in -SCREE_REACH..(super::CHUNK_SIZE_Z as i32 + SCREE_REACH) {
            for lx in -SCREE_REACH..(super::CHUNK_SIZE_X as i32 + SCREE_REACH) {
                let column = columns.at(lx, lz);
                // The carver ate this column's top: the surface cell is a
                // hole. The same question `place_boulders` asks so that a
                // boulder is never left hanging over one.
                holed[((lz + SCREE_REACH) * span + lx + SCREE_REACH) as usize] =
                    column.height >= column.water && self.is_cave(origin_x + lx, column.height, origin_z + lz);
            }
        }
        let holed_at = |lx: i32, lz: i32| holed[((lz + SCREE_REACH) * span + lx + SCREE_REACH) as usize];
        for lz in 0..super::CHUNK_SIZE_Z as i32 {
            for lx in 0..super::CHUNK_SIZE_X as i32 {
                if holed_at(lx, lz) {
                    continue;
                }
                let super::Column { height, water, rock, .. } = columns.at(lx, lz);
                if height < water || height + 1 >= super::CHUNK_SIZE_Y as i32 {
                    continue;
                }
                // How near the nearest hole is, in columns, which is what
                // decides whether the turf goes or only the stones fall.
                let near = (-SCREE_REACH..=SCREE_REACH)
                    .flat_map(|dz| (-SCREE_REACH..=SCREE_REACH).map(move |dx| (dx, dz)))
                    .filter(|&(dx, dz)| holed_at(lx + dx, lz + dz))
                    .map(|(dx, dz)| dx.abs().max(dz.abs()))
                    .min();
                let Some(near) = near else {
                    continue;
                };
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let air = super::Chunk::index(lx as usize, (height + 1) as usize, lz as usize);
                // **Into air, and into nothing else.** It overwrote
                // anything that was not solid, which reads as "the talus
                // beats a tuft of grass" and is in fact "the talus beats
                // the lower half of a tall plant, the flint flakes of a
                // knapping floor, and the fallen leaves of a wood" -- one
                // orphaned plant top and two broken finds, all found by
                // tests that have nothing to do with caves. A decoration
                // pass that runs last takes the cells nothing else wanted;
                // the ground cover leaves most of them, and a talus of two
                // columns in three of what is bare is still a talus.
                if blocks[air] != crate::types::BLOCK_AIR {
                    continue;
                }
                // **Gravel and not cobble.** A cobble block standing on the
                // ground is a *boulder* everywhere else in this generator,
                // and `a_boulder_lies_on_a_bank_or_under_a_cliff_and_never_in_a_meadow`
                // reads them by block id -- talus of cobble is a hillside
                // full of boulders nothing can explain. Gravel of the same
                // rock is the same colour and is what falls out of a roof
                // anyway.
                let stones = super::hash2(gz, gx, self.seed.wrapping_add(0x5C2F));
                let thick = if near <= 1 { SCREE_LUMP_SPACING } else { SCREE_LUMP_SPACING * 2 };
                if stones.is_multiple_of(thick) {
                    blocks[air] = crate::ground::rubble_of(rock, crate::ground::Form::Gravel);
                } else if near <= 1 || stones.is_multiple_of(SCREE_PEBBLE_SPACING) {
                    blocks[air] = crate::ground::rubble_of(rock, crate::ground::Form::Pebble);
                }
            }
        }
    }

    /// **Copper showing through a hillside.**
    ///
    /// "Медь за холмом" was true of the ore table and invisible from the
    /// ground: copper lies above `copper_country`'s line, which is an
    /// altitude, so the hills were where to dig -- and nothing on a hill
    /// said so. A player found their first copper by digging somewhere and
    /// being right, which is not a decision, it is a lottery.
    ///
    /// An outcrop is a lens of the ore itself set into a steep face, three
    /// cells deep and a ragged patch across, in country the ore table
    /// already agrees has copper in it. It is not a supply -- a handful of
    /// blocks is not a smelting -- it is a *sighting*, and the decision it
    /// makes is whether this hill is worth coming back to.
    ///
    /// Rejected: **a nugget on the surface.** There is one already
    /// (`BLOCK_NATIVE_COPPER`, in the same country), and it is a thing you
    /// find by walking over it. An outcrop has to be a thing you see from
    /// the other side of a valley, and one loose block is not.
    ///
    /// Runs after `place_ground_cover` for the reason `place_cave_mouths`
    /// does, and the die is rolled before anything is read.
    pub(super) fn place_outcrops(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
        columns: &super::ColumnCache,
    ) {
        if self.scale != Scale::Landforms {
            return;
        }
        for lz in 0..super::CHUNK_SIZE_Z as i32 {
            for lx in 0..super::CHUNK_SIZE_X as i32 {
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                if !super::hash2(gx, gz, self.seed.wrapping_add(0x0C09)).is_multiple_of(OUTCROP_SPACING) {
                    continue;
                }
                let column = columns.at(lx, lz);
                if column.height < column.water + OUTCROP_ABOVE_WATER
                    || column.slope < OUTCROP_SLOPE
                    || Self::copper_country(column.height) < OUTCROP_COUNTRY
                {
                    continue;
                }
                // The candidate column and whichever of its four
                // neighbours the second die takes: a patch of one to five
                // columns, which is a stain on a slope rather than a square.
                for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (lx + dx, lz + dz);
                    if !(0..super::CHUNK_SIZE_X as i32).contains(&nx)
                        || !(0..super::CHUNK_SIZE_Z as i32).contains(&nz)
                    {
                        continue;
                    }
                    if (dx, dz) != (0, 0)
                        && !super::hash2(origin_x + nx, origin_z + nz, self.seed.wrapping_add(0x0C0A)).is_multiple_of(2)
                    {
                        continue;
                    }
                    let near = columns.at(nx, nz);
                    if near.height < near.water || self.is_cave(origin_x + nx, near.height, origin_z + nz) {
                        continue;
                    }
                    for step in 0..OUTCROP_DEPTH {
                        let y = near.height - step;
                        if y <= 1 {
                            break;
                        }
                        let index = super::Chunk::index(nx as usize, y as usize, nz as usize);
                        // Only into ground that is there: a lens written
                        // into a cave the carver opened under the face
                        // would be ore floating in a black room.
                        if blocks[index] == crate::types::BLOCK_AIR
                            || crate::types::is_liquid(blocks[index])
                        {
                            break;
                        }
                        blocks[index] = crate::types::BLOCK_COPPER_ORE;
                    }
                }
            }
        }
    }
}
