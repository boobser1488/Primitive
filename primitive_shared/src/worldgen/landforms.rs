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
