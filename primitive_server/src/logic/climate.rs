//! How cold it is *where a particular player is standing*.
//!
//! ## The one question this module answers
//!
//! [`primitive_shared::body`] knows what being cold does to a person and
//! how fast it happens. It deliberately does not know how cold it is,
//! because that is a question about the world -- the biome, the hour of
//! the day, the time of year, whether it is raining, whether there is a
//! fire two blocks away, whether the player is in a lake, and whether
//! there is a roof over them and walls round them. Every one of those
//! is the server's to answer, and this is where they are added up.
//!
//! ## Why the answer is a sample and not a subscription
//!
//! Temperature is the sort of mechanic that invites a per-frame check
//! per player: what biome, what light, what is nearby. That is exactly
//! the shape the rest of this server avoids, and the reason is the
//! arithmetic -- a hundred players at twenty ticks a second is two
//! thousand of those a second, each of which reads a dozen cells and
//! walks a fire map.
//!
//! So the world's contribution to a player's temperature is worked out
//! on a **slow interval** ([`SAMPLE_INTERVAL_SECS`]) and held between
//! samples, while the *body* keeps drifting toward it every tick. That
//! is not a cheat: the body's time constant is minutes (see
//! `body::BASE_ADJUST_PER_SECOND`), so a world sampled twice a second is
//! sampled roughly two hundred times per meaningful change in the thing
//! it drives. A player who runs from a fire into a snowdrift feels it
//! within half a second, which is well under the time it takes to
//! matter.
//!
//! The sample itself is also cheap by construction: one climate lookup
//! (which the generator has cached per column anyway), one block read at
//! the feet, one at the head, a short walk up for the sky, four short
//! walks sideways for the walls, and one pass over the fires already
//! within range.
//!
//! ## What the numbers mean
//!
//! Everything here is in the degrees `body` uses. The pieces are
//! deliberately *additive* rather than multiplicative, because a player
//! has to be able to reason about them: a fire is worth so many degrees,
//! night takes so many off, rain takes so many more. A product of six
//! factors is a number nobody can predict the sign of.
//!
//! ## The two answers to the cold, and which question each answers
//!
//! **Shelter answers the night. Fire answers the season.** A roof and
//! walls take most of the day/night swing away, because that swing is
//! the sky pulling heat out of the ground and out of you, and a ceiling
//! is between you and the sky. They take nothing off the *season*,
//! because a season is the air itself being colder, and a hut full of
//! winter air is a cold hut -- anyone who has stood in an unheated
//! house in January knows this. What warms the air in a hut is a fire,
//! and a fire in a room warms the room (`HEARTH_ROOM_SHARE`) where the
//! same fire in the open warms the yard of ground round it and nothing
//! further. So: a summer night is survivable under a roof, an autumn
//! night wants walls, a winter night wants a fire behind them, and the
//! player who has all three has *built* their way out of the cold.
//! That is the progression the mechanic exists to create.

use std::sync::Arc;

use primitive_shared::body;
use primitive_shared::season;
use primitive_shared::types::{blocks_the_sky, CHUNK_SIZE_Y};
use primitive_shared::weather::Weather;

use crate::logic::fire::Fires;
use crate::logic::world::World;

/// How often the world is sampled for one player.
///
/// Twice a second. See the note above for why that is not a compromise.
pub const SAMPLE_INTERVAL_SECS: f32 = 0.5;

/// The temperature of a biome at its most comfortable -- noon, clear
/// sky, sea level, the day the world opened.
///
/// Read off the generator's own climate field rather than off the biome
/// *name*, for the reason the mesher tints foliage from the field: a
/// biome is a classification with edges, and a temperature that stepped
/// at a biome boundary would put a wall of cold across the ground
/// exactly where the player is looking. The field underneath is smooth.
pub const HOTTEST_C: f32 = 44.0;
pub const COLDEST_C: f32 = -12.0;

/// How much colder the middle of the night is than noon.
///
/// A real diurnal range, which is what makes a desert a *desert*: baking
/// by day and freezing by night is the thing about one that kills
/// people, and a game that models only the heat has modelled the
/// postcard.
pub const NIGHT_DROP_C: f32 = 17.0;

/// ...and how much of that a humid place is spared.
///
/// Water holds heat. A swamp barely cools off overnight and a desert
/// loses everything it gained, which is one field of the generator's
/// doing something a second time and costs nothing.
pub const HUMIDITY_EVENS_OUT: f32 = 0.6;

/// How much colder the hours before dawn are than the cosine says.
///
/// **The cosine alone made the night a non-event**, and that is what
/// "make the night cold enough that a house or a fire is mandatory"
/// was about. With a seventeen-degree swing damped by humidity, the
/// starting meadow bottomed out at four degrees at midnight -- sixteen
/// on the skin, cold enough to shiver and never cold enough to matter
/// -- and a player could sleep in the open every night of the year at
/// the cost of a little food. The body's own clock is the reason: it
/// closes the gap to the air over five minutes (`body::BASE_ADJUST`),
/// and a night is seven and a half, so the air has to be *well* below
/// the skin's danger line for long enough that the body gets there.
///
/// A real night is not a cosine either. The ground radiates all night
/// long and the coldest hour is the one before sunrise, not midnight;
/// this term is that radiation, shaped so it is nothing at dusk and
/// all of itself at dawn (see `pre_dawn_chill`). Ten degrees, on top
/// of the swing, puts the starting meadow at about zero before dawn --
/// twelve on bare skin, the edge of the line where the cold starts to
/// cost health -- so with no season on it a night in the open is a
/// *warning* (skin at seventeen by sunrise: Cold, hunger running at
/// double, no blood) and in midwinter the same night, ten degrees
/// lower again, kills a healthy player where they lie. The season is
/// what turns the warning into the verdict, which is the order those
/// two should come in. Measured, not estimated: the numbers are the
/// simulated nights in the tests at the end of this file, run through
/// the real body model at the default clock.
///
/// **Not damped by humidity, deliberately**, although in life it would
/// be. The swing already is, and a second damping on the same field
/// would let a swamp off the night entirely; the pre-dawn hour is cold
/// everywhere, which is what everyone who has slept outside knows.
pub const PRE_DAWN_CHILL_C: f32 = 10.0;

/// How much of the day the pre-dawn chill takes to lift after sunrise,
/// as a fraction of a full day.
///
/// A fifth of a day is a little under five hours of game time -- gone
/// by mid-morning. Without a thaw the term would vanish at the instant
/// of sunrise, and a step of ten degrees at 06:00 is a wall a player
/// can feel and cannot explain.
pub const MORNING_THAW: f32 = 0.2;

/// How much the rain takes off.
pub const RAIN_CHILL_C: f32 = 5.0;
/// ...and a storm, which is the rain plus the wind.
pub const STORM_CHILL_C: f32 = 11.0;

/// How much colder it is under water.
///
/// Swimming is the fastest way to get cold in this game, which is what
/// it is in life. Applied as a floor rather than an offset -- see
/// [`Ambient::of`] -- because a lake in a desert is not a hot lake.
pub const WATER_C: f32 = 12.0;

/// ...and how much of a soaking a swimmer gets, per second in the water.
pub const WETTING_PER_SECOND: f32 = 0.6;
/// How fast a player dries off out of the rain, per second.
///
/// Much slower than getting wet, which is both true and the point: a
/// dunk is a decision with a tail on it rather than a state you shrug
/// off by climbing out.
pub const DRYING_PER_SECOND: f32 = 0.045;

/// How much faster than that a wet player dries in hot, dry air, at most.
///
/// Once as fast again: a soaking that lasts twenty seconds in a meadow
/// lasts about twelve in the open desert at noon. Scaled down by the
/// humidity, because what dries a shirt is air with room in it for the
/// water, and a swamp at thirty degrees has none.
///
/// **This works against the river, and that is right.** The lasting
/// good of a dip in hot country is the dip itself -- water takes heat off
/// a body three times as fast as air (`body::WATER_ADJUST_FACTOR`) -- and
/// the wet shirt afterwards (`body::WET_COOLING_C`) is the minute after
/// it, not a way to cross a desert. A soaking that lasted in the heat
/// would make a bucket of water a coat of shade, and nobody would ever
/// need to plan around the noon.
pub const HEAT_DRYING: f32 = 1.0;

/// ...and how many degrees above [`SUN_FROM_C`], sun included, it takes
/// to dry at the whole of [`HEAT_DRYING`].
pub const HEAT_DRYING_SPAN_C: f32 = 20.0;

/// How many degrees the noon sun adds on bare skin under a clear sky, in
/// hot country.
///
/// **Heat was modelled and never bit.** The air here peaks at 44 in the
/// hottest desert, a bare body heads for the air, and so a desert at noon
/// was Warm at worst: nothing about hot country asked anything of a
/// player -- no shade to find, no hour to wait out, no shirt worth
/// weaving. What was missing is the sun. The air is what a thermometer in
/// the shade reads, and nobody crossing a desert is standing in the shade.
///
/// Fourteen, chosen by running the body model through a clear day from
/// sunrise, standing still, on a day whose noon the season puts two
/// degrees warm (the tests' `no_offset_day`). Measured, not estimated --
/// the desert and savanna tests below are these rows:
///
/// ```text
///                          noon air  heading for  what the day does
/// hot desert (0.90, 0.10)    40.4       54.4      Warm 09:47, heatstroke
///                                                 11:26, dead by afternoon
///   ...under a roof          40.4       40.4      peaks 39.1, Warm, unhurt
///   ...in full cloth         40.4       40.1      peaks 38.0, unhurt
///   ...in full wool          40.4       56.2      heatstroke 11:05, dead
///   ...midsummer, in shade   44.4       44.4      peaks 43.1, unhurt
/// savanna (0.72, 0.55)       30.3       44.3      peaks 40.8, Warm, unhurt
///   ...in cloth              30.3       34.0      peaks 32.6
///   ...midsummer             34.3       48.3      peaks 44.7
/// the test meadow            18.8       18.8      no sun to speak of
/// ```
///
/// The desert's verdict is the whole day stood in the open; a player who
/// walks into shade when the strip says "heatstroke" is out of the damage
/// in twenty seconds (`body`'s
/// `the_sun_arrives_faster_than_the_cold_and_crossing_it_is_still_free`). That is the decision the number exists for: travel in the
/// morning and the evening, rest under a tree at noon, carry water, wear
/// cloth -- each of those is worth one band on the gauge.
///
/// **Straight down, deliberately.** A sun that came in at the hour's
/// angle would cast a tree's shadow west in the morning and east in the
/// afternoon, which is true and is a shade a player cannot find by
/// looking up; "stand under something" is the rule a person can act on,
/// and at noon, when the sun matters, it is also the truth.
pub const SUN_C: f32 = 14.0;

/// The air temperature below which the sun adds nothing.
///
/// **The same sun shines on the meadow, and the meadow is pleasant.** What
/// makes a desert sun dangerous is that the body has nowhere left to put
/// the heat -- the air is already nearly as warm as the skin -- so the sun
/// is scaled by how hot the air already is rather than laid on
/// everywhere. From nothing at twenty-one degrees to all of it at
/// twenty-nine ([`SUN_RAMP_C`]), which is hot country and nowhere else, and
/// which leaves the test meadow's midsummer noon (24.8) with under seven
/// degrees of sun: bare skin heading for 31.5, inside the comfortable
/// band -- the promise `a_temperate_day_is_still_comfortable_in_nothing_but_skin`
/// keeps.
///
/// Rejected: a sun by biome name. A biome is a classification with edges,
/// and a sun that switched on at the savanna's border would be a wall of
/// heat across the grass -- the step the smooth climate field exists to
/// avoid (see [`HOTTEST_C`]).
pub const SUN_FROM_C: f32 = 21.0;

/// ...and how many degrees above that the sun takes to count in full.
pub const SUN_RAMP_C: f32 = 8.0;

/// How warm a fire makes the cell it is in.
///
/// A campfire at your feet is the answer to a cold night, and it has to
/// be a *large* number for that to be true: it must beat a snowfield at
/// midnight, or the first thing a player does when they get cold turns
/// out not to work.
pub const FIRE_C: f32 = 46.0;

/// How far a fire's warmth reaches, in blocks.
///
/// Short. A hearth warms the place you sit at it, not the camp; a fire
/// with a large radius is a fire nobody has to gather round.
pub const FIRE_RANGE: f32 = 5.0;

/// The least a fire is worth anywhere inside an enclosed room it is in
/// reach of, as a fraction of [`FIRE_C`].
///
/// In the open a fire's warmth falls off with distance to nothing at
/// the edge of its range, and that edge is where a cold night still
/// bites -- the far side of the camp is cold, which is what makes
/// people gather. Inside four walls and a roof the heat has nowhere to
/// go, so the same fire warms the whole room: this floor is that. A
/// little over half, which is twenty-five degrees of air and thirty on
/// the skin -- comfortable, anywhere in a hut the fire can reach, in
/// any season. That is the sentence "a house with a fire in it is
/// warm", and it is the whole reward for building one.
pub const HEARTH_ROOM_SHARE: f32 = 0.55;

/// How much of the day/night swing a roof alone keeps out.
///
/// Half. A roof is a ceiling between you and the sky, and the swing is
/// mostly the sky's doing -- but a roof with the wind under it is a
/// lean-to, not a house. It used to be nearly two thirds, back when the
/// roof was the only kind of shelter there was; a lean-to was worth as
/// much as a hut because there was no hut to compare it with.
pub const ROOF_EVENS_OUT: f32 = 0.5;

/// ...and how much a roof *with walls* keeps out.
///
/// Four fifths. Not all of it: an unheated hut is still colder at night
/// than by day, and the fifth that gets in is what a fire is for. The
/// number was chosen against the starting meadow at the equinox: open,
/// it is on the edge of costing health before dawn; under a roof it is
/// cold; behind walls it is cool. Those are three different nights and
/// a player can tell which one they built.
pub const WALLS_EVENS_OUT: f32 = 0.8;

/// How many blocks of cover it takes to shut the day and the night out
/// entirely, counted past the first ([`earth_shares`]).
///
/// **Two, because the daily wave does not get far into the ground.** The
/// depth at which a temperature swing has fallen to a third of itself goes
/// as the square root of its period; for soil and rock it is about twelve
/// centimetres for a day and about two metres for a year. So a hand's depth
/// of earth already hides noon from midnight, and a cellar two blocks down
/// has no hour in it at all.
pub const EARTH_DAILY_BLOCKS: f32 = 2.0;

/// ...and the depth, in blocks past the first, over which the *year's* swing
/// falls by e: the damping depth of the annual wave in soil, which is a
/// little over two metres.
///
/// With it, three blocks of cover keep out about half of summer and winter,
/// six keep out nine tenths, and a deep cave has none of either -- it sits at
/// the place's own average, the mean of every hour of every day of the year.
/// That is why a cave is cool in August and mild in January, and why a
/// root cellar dug into a hillside keeps food: see `rot::COOL_BELOW_C`.
pub const EARTH_ANNUAL_BLOCKS: f32 = 2.2;

/// The average of [`pre_dawn_chill`] over a whole day.
///
/// Worked out from its shape rather than sampled: the square over the
/// half-day of darkness averages a third, which is a sixth of a day, and the
/// straight thaw is half of [`MORNING_THAW`]. A test integrates the function
/// and holds this to it, so reshaping the chill cannot leave a cave at the
/// wrong average.
pub const MEAN_PRE_DAWN_CHILL: f32 = 1.0 / 6.0 + MORNING_THAW * 0.5;

/// How much of the day's swing and of the year's swing a cell's cover keeps
/// out, `(daily, annual)`, each 0..1, from the number of sky-stopping blocks
/// over it.
///
/// **The first block counts for nothing.** It is the roof, and the roof
/// already has its share ([`Shelter::evens_out`]); a hut of one plank over
/// a player's head is not a cellar, and a winter night in one has to stay
/// the cold night `a_roof_and_three_walls_take_the_bite_out_of_the_night`
/// measures. From the second on, every block is thermal mass between the
/// cell and the sky.
///
/// Rejected: counting only earth and rock. It is truer -- planks are poor
/// mass -- but it would need a table of what is "ground" beside the one
/// `blocks_the_sky` already is, and the case it gets wrong is a player who
/// has built three storeys of timber over their larder, who has earned
/// something for it anyway.
pub fn earth_shares(cover: u32) -> (f32, f32) {
    let past_roof = cover.saturating_sub(1) as f32;
    let daily = (past_roof / EARTH_DAILY_BLOCKS).clamp(0.0, 1.0);
    let annual = 1.0 - (-past_roof / EARTH_ANNUAL_BLOCKS).exp();
    (daily, annual)
}

/// How far up the search for a roof goes.
///
/// Sixteen cells. Far enough that a house, a cave and a tree canopy all
/// count; short enough that it is sixteen array reads out of a chunk
/// that is already in cache, on a path taken twice a second per player.
pub const ROOF_SEARCH: i32 = 16;

/// How far sideways the search for a wall goes, in each direction.
///
/// Three. A room a player would call a room has a wall within three
/// cells of where they stand in most directions; a hall with walls six
/// away is a hall, and the draught in it is real. Also the difference
/// between a cave nook and a cave passage: a dead end has three walls
/// close, a corridor has two.
pub const WALL_SEARCH: i32 = 3;

/// How many of the four directions must have a wall for a roofed cell
/// to count as enclosed.
///
/// Three, so a doorway is allowed. Four would make every hut a sealed
/// box, and a player who left the door open to see out would lose the
/// house they built.
pub const WALLS_NEEDED: u32 = 3;

/// How much of the sky a player is under.
///
/// Three states rather than a number, because a player has to be able
/// to *say* which one they are in -- "I am under a roof" and "I am
/// indoors" are things you know, and a shelter coefficient of 0.62 is
/// not. `Roofed` is what a cave mouth, a lean-to and a doorway are;
/// `Enclosed` is a room.
///
/// **There is no walls-without-a-roof tier**, and that was considered.
/// A walled yard keeps the wind off and does nothing about the sky,
/// which is most of the night; a pit in the ground is the same. Giving
/// either a share would make the first "shelter" a player builds a
/// hole, and the roof -- the thing that actually takes the night off
/// -- an upgrade they might never find out about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelter {
    Open,
    Roofed,
    Enclosed,
}

impl Shelter {
    /// How much of the sky's swing this keeps out, 0..1.
    pub fn evens_out(self) -> f32 {
        match self {
            Shelter::Open => 0.0,
            Shelter::Roofed => ROOF_EVENS_OUT,
            Shelter::Enclosed => WALLS_EVENS_OUT,
        }
    }

    /// Is there anything at all between this player and the sky?
    pub fn has_roof(self) -> bool {
        !matches!(self, Shelter::Open)
    }
}

/// What the world is doing to one player right now.
///
/// Carried on the player's own state and refreshed on the interval; the
/// tick loop reads it every tick and the sampler writes it twice a
/// second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ambient {
    /// The temperature around them, in degrees.
    pub temperature_c: f32,
    /// Whether anything is falling on them -- rain they are standing out
    /// in, or water they are standing in.
    pub getting_wet: bool,
    /// Whether they are in reach of a fire. Reported so the client's
    /// debug panel can say *why* somebody is warm, and so a plugin can
    /// ask.
    pub near_fire: bool,
    /// Whether there is anything over their head -- `Shelter::has_roof`.
    /// Whether there are walls as well is not carried here: the racks
    /// (`drying`) build this struct by hand and ask only about rain,
    /// and the answer for a player is a call to `shelter_at` away.
    pub sheltered: bool,
    /// Whether their feet are in water.
    pub in_water: bool,
    /// Degrees the sun adds on bare skin where they stand: zero at night,
    /// in rain, in water, under a roof and under a canopy. Apart from
    /// `temperature_c`, never added into it -- see `body::Exposure` for
    /// the two bugs that folding it in would have been.
    pub sun_c: f32,
    /// How fast a wet player dries here, per second: [`DRYING_PER_SECOND`]
    /// in mild air, faster in hot dry air -- see [`HEAT_DRYING`].
    pub drying_per_second: f32,
}

impl Default for Ambient {
    fn default() -> Self {
        Self {
            temperature_c: body::NEUTRAL_C,
            getting_wet: false,
            near_fire: false,
            sheltered: false,
            in_water: false,
            sun_c: 0.0,
            drying_per_second: DRYING_PER_SECOND,
        }
    }
}

impl Ambient {
    /// What this sample is to a body: the air, the sun, the water.
    pub fn exposure(&self) -> body::Exposure {
        body::Exposure {
            air_c: self.temperature_c,
            sun_c: self.sun_c,
            in_water: self.in_water,
        }
    }

    /// Samples the world at a player's position.
    ///
    /// `world_time` is the world's age **in days, with the hour in the
    /// fraction** -- see `season`. The hour is what the night is worked
    /// out from and the whole number is what the season is; one value
    /// carries both so the two can never disagree about what day it
    /// is. A caller that only has an hour (`0.0..1.0`) gets the day the
    /// world opened, which the calendar puts on the last day of spring
    /// with no offset to speak of -- so the clock as it stands today
    /// produces the world the generator describes, and wiring the day
    /// count through changes nothing about a fresh world's first day.
    ///
    /// **Every lookup here is a cached one.** `cached_block` answers
    /// `None` for a chunk nobody has loaded rather than generating it,
    /// which matters for exactly the reason the anti-cheat's ground
    /// check reads the cache: a per-player periodic sample that could
    /// trigger terrain generation would be a way for a client to make
    /// the server generate chunks by standing somewhere.
    ///
    /// A player in an unloaded column -- which happens for a tick or two
    /// after a teleport -- gets the neutral answer rather than a guess,
    /// so nobody freezes because their chunk was late.
    pub fn of(
        world: &Arc<World>,
        fires: &Fires,
        position: (f32, f32, f32),
        world_time: f32,
        weather: Weather,
    ) -> Ambient {
        Self::of_in(world, fires, position, world_time, weather, |x, y, z| {
            world.climate_at(x, y, z)
        })
    }

    /// [`Ambient::of`], with the climate field handed in rather than read
    /// off the generator.
    ///
    /// For the tests. The test preset's climate is one value everywhere --
    /// a temperate meadow -- so hot country cannot be stood in there, and
    /// the alternative, a seed hunted for a desert near the origin, would
    /// be a test of that seed rather than of the sun.
    fn of_in(
        world: &Arc<World>,
        fires: &Fires,
        position: (f32, f32, f32),
        world_time: f32,
        weather: Weather,
        climate: impl Fn(i32, i32, i32) -> (f32, f32),
    ) -> Ambient {
        if !position.0.is_finite() || !position.1.is_finite() || !position.2.is_finite() {
            return Ambient::default();
        }
        let (fx, fy, fz) = (
            position.0.floor() as i32,
            position.1.floor() as i32,
            position.2.floor() as i32,
        );
        let Some(_here) = world.cached_block(fx, fy.clamp(0, CHUNK_SIZE_Y as i32 - 1), fz) else {
            return Ambient::default();
        };
        // A nonsense clock is noon on the day the world opened, and it
        // is decided once so the hour and the season below agree.
        let world_time = if world_time.is_finite() { world_time } else { 0.5 };
        let time_of_day = world_time.rem_euclid(1.0);

        // ---- the biome, as a smooth field ----
        //
        // `climate_at` is normalised 0..1 and already carries the lapse
        // rate, so altitude is in the number before anything here
        // touches it: a peak is cold because it is high, in the same
        // arithmetic that decides whether snow lies on it.
        let (warmth, humidity) = climate(fx, fy, fz);
        let mut degrees = COLDEST_C + (HOTTEST_C - COLDEST_C) * warmth.clamp(0.0, 1.0);

        // ---- the season ----
        //
        // Before shelter, and not damped by it: the season is the air
        // being colder, and a hut full of January is a cold hut. What
        // answers the season is the fire, below. See the module note.
        // Scaled to the latitude: a tropical world's winter is a cool
        // spell, not ten degrees of frost. Exactly the old offset at
        // forty-five and on the test world -- see `season::seasonal_swing`.
        //
        // **Under enough ground the season is the year's average instead**
        // (`earth_shares`). Before this a cave had the open sky's January
        // in it, which made a hole in a hillside no warmer in winter and no
        // cooler in summer than the meadow over it -- the opposite of every
        // cave anyone has stood in, and a cellar worth nothing as a larder.
        let shelter = shelter_at(world, fx, fy, fz);
        let (earth_daily, earth_annual) = if shelter.has_roof() {
            earth_shares(cover_over(world, fx, fy, fz))
        } else {
            (0.0, 0.0)
        };
        let swing = season::seasonal_swing(world.latitude_degrees(fz));
        let season_now = season::ambient_offset_c(world_time) * swing;
        let season_mean = (season::SUMMER_PEAK_C + season::WINTER_TROUGH_C) * 0.5 * swing;
        degrees += season_now + (season_mean - season_now) * earth_annual;

        // ---- the hour ----
        //
        // Two terms. A cosine, so dusk is a slope -- peak at noon
        // (`time_of_day` 0.5), trough at midnight -- damped by humidity
        // because water holds heat. And the ground radiating, which is
        // nothing at dusk and everything just before sunrise, because
        // that is when the coldest hour actually is. Both are the sky's
        // doing, so both are what shelter keeps out.
        let humid = 1.0 - HUMIDITY_EVENS_OUT * humidity.clamp(0.0, 1.0);
        // 1 at noon, 0 at midnight.
        let sun = 0.5 - 0.5 * (std::f32::consts::TAU * time_of_day).cos();
        let night = NIGHT_DROP_C * humid * (1.0 - sun)
            + PRE_DAWN_CHILL_C * pre_dawn_chill(time_of_day);
        // Under ground the hour is replaced by its own average, not taken
        // away: the earth over a cellar has soaked up every night as well
        // as every noon, and a cave at the day's *warmest* would be a
        // cave warmer than the meadow averages, which no cave is.
        let night_mean = NIGHT_DROP_C * humid * 0.5 + PRE_DAWN_CHILL_C * MEAN_PRE_DAWN_CHILL;
        let night_here = night * (1.0 - shelter.evens_out());
        degrees -= night_here + (night_mean - night_here) * earth_daily;

        // ---- the sun ----
        //
        // Read off the air as it stands *now* -- biome, season, hour --
        // before the rain, the water and the fire, because what the sun
        // needs to know is whether this is hot country at this hour, and
        // a campfire at your feet does not make a meadow a desert.
        // Whether anything is overhead is asked here, with the roof the
        // hour already found and a canopy, which is shade and not
        // shelter (see `has_canopy`); whether it reaches the player
        // through rain or water is decided at the end, once both are
        // known. The walk up for a canopy is only taken when there is a
        // sun to be shaded from, which on most of the map at most hours
        // there is not.
        let overhead_sun = solar_heat_c(degrees, time_of_day);
        let shaded =
            overhead_sun > 0.0 && (shelter.has_roof() || has_canopy(world, fx, fy, fz));

        // ---- the sky ----
        //
        // Only if it can reach you. Rain in a cave is somebody else's
        // rain, and this is the same roof test the hour used.
        let raining_on_me = weather.is_wet() && !shelter.has_roof();
        if raining_on_me {
            degrees -= match weather {
                Weather::Storm => STORM_CHILL_C,
                _ => RAIN_CHILL_C,
            };
        }

        // ---- the water ----
        //
        // A ceiling rather than an offset: water is a heat sink at a
        // temperature of its own, so a lake in a desert is cold and a
        // lake in a tundra is not *colder* than the tundra. `min` says
        // exactly that in one line.
        let in_water = world
            .cached_block(fx, fy, fz)
            .is_some_and(primitive_shared::types::is_liquid);
        if in_water {
            degrees = degrees.min(WATER_C);
        }

        // ---- the fire ----
        //
        // Last, and it wins, which is the design: a fire is the answer
        // to being cold, and an answer that could be outvoted by a
        // storm or a season would not be one. Falls off with distance
        // so that sitting at a hearth and standing across the camp from
        // it are different things -- except in a room, where the heat
        // has nowhere to go and the whole room is warm.
        //
        // The cell the player is standing in is checked first and
        // separately, because a campfire is half a block tall and a
        // player standing *in* one is not near a fire, they are in it --
        // which the burn damage in the tick loop already has an opinion
        // about.
        let mut near_fire = false;
        let feet = (position.0, position.1, position.2);
        let mut fire_share = 0.0f32;
        for (x, y, z) in fires.within(feet, FIRE_RANGE) {
            near_fire = true;
            let (dx, dy, dz) = (
                x as f32 + 0.5 - position.0,
                y as f32 + 0.5 - position.1,
                z as f32 + 0.5 - position.2,
            );
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            // Linear falloff to nothing at the edge of the range. Not
            // inverse-square: an inverse-square fire is scalding at one
            // block and useless at three, which is physically right and
            // reads as a fire that does not work.
            let mut share = (1.0 - distance / FIRE_RANGE).clamp(0.0, 1.0);
            if shelter == Shelter::Enclosed {
                share = share.max(HEARTH_ROOM_SHARE);
            }
            fire_share = fire_share.max(share);
        }
        if near_fire && degrees < FIRE_C {
            // The fire pulls toward its own temperature rather than
            // adding to whatever was there: a bonfire in a blizzard is
            // warm, not "blizzard plus forty". And it pulls *from where
            // the air is*, by the share, rather than replacing the air
            // with `FIRE_C * share` when that is higher -- which is what
            // this did, and which made the last block inside a fire's
            // reach on a winter night a strip of exactly zero degrees
            // with the block after it twelve below. The edge of a
            // fire's warmth is the falloff and nothing else, so the
            // curve has to arrive at the air, not at zero.
            degrees += (FIRE_C - degrees) * fire_share;
        }

        // The sun, if it reaches them: not through a roof or a canopy, not
        // through rain cloud, and not into a lake -- the water is the heat
        // sink the section above says it is, and a swimmer under a noon
        // sky is a cool swimmer.
        let sun_c = if shaded || in_water || weather.is_wet() {
            0.0
        } else {
            overhead_sun
        };

        Ambient {
            temperature_c: degrees,
            getting_wet: raining_on_me || in_water,
            near_fire,
            sheltered: shelter.has_roof(),
            in_water,
            sun_c,
            // With the fire in it, which is why sitting at a hearth dries
            // a soaked player -- the thing `body::WET_METABOLIC_LOSS`
            // says a fire is worth doing for.
            drying_per_second: drying_rate(degrees + sun_c, humidity),
        }
    }

    /// How wet the player should be after `dt` seconds of this.
    ///
    /// Wetness is 0..1 and lives on the player rather than here, because
    /// it has memory: climbing out of a lake does not dry you.
    pub fn step_wetness(&self, wetness: f32, dt: f32) -> f32 {
        let dt = dt.clamp(0.0, 1.0);
        let next = if self.in_water {
            wetness + WETTING_PER_SECOND * dt
        } else if self.getting_wet {
            // Rain soaks more slowly than a lake does, which is the
            // difference between being caught out and going for a swim.
            wetness + WETTING_PER_SECOND * 0.35 * dt
        } else {
            // A rate off a struct that the racks build by hand and a
            // save could in principle carry: a nonsense one is the mild
            // day's, never a player who stays wet for ever.
            let drying = if self.drying_per_second.is_finite() {
                self.drying_per_second.max(0.0)
            } else {
                DRYING_PER_SECOND
            };
            wetness - drying * dt
        };
        next.clamp(0.0, 1.0)
    }
}

/// Degrees the sun adds on bare skin in the open, under a clear sky, at
/// this hour, in air this warm. See [`SUN_C`] and [`SUN_FROM_C`].
///
/// The height of the sun is `-cos` of the hour: nothing from dusk (0.75)
/// to sunrise (0.25) -- the two instants `pre_dawn_chill` counts from --
/// and all of it at noon. **Not the hour term's own cosine**, which is
/// one at noon and nothing only at midnight: borrowed, it put half the
/// noon sun on the ground at sunrise and a quarter of it at nine at
/// night.
pub fn solar_heat_c(air_c: f32, time_of_day: f32) -> f32 {
    if !air_c.is_finite() || !time_of_day.is_finite() {
        return 0.0;
    }
    let height = (-(std::f32::consts::TAU * time_of_day).cos()).max(0.0);
    let hot = ((air_c - SUN_FROM_C) / SUN_RAMP_C).clamp(0.0, 1.0);
    SUN_C * height * hot
}

/// How fast a wet player dries, per second, in air this warm (the sun
/// that reaches them included) and this humid. See [`HEAT_DRYING`].
pub fn drying_rate(felt_air_c: f32, humidity: f32) -> f32 {
    if !felt_air_c.is_finite() || !humidity.is_finite() {
        return DRYING_PER_SECOND;
    }
    let heat = ((felt_air_c - SUN_FROM_C) / HEAT_DRYING_SPAN_C).clamp(0.0, 1.0);
    DRYING_PER_SECOND * (1.0 + HEAT_DRYING * heat * (1.0 - humidity.clamp(0.0, 1.0)))
}

/// Is there a tree over this cell?
///
/// **Shade is not shelter, and this is not `has_roof`.** A canopy keeps
/// the sun off -- anybody who has stood under an oak at noon knows it --
/// and keeps neither the rain nor the night off, because leaves are a
/// sieve for both and the lighting engine says so (`blocks_the_sky`;
/// `a_roof_keeps_the_rain_and_the_night_off`). So a wood is where a player
/// rests at midday and not where they sleep, and the two questions are
/// asked separately rather than one being bent to answer both.
///
/// Any leaf in the column up to [`ROOF_SEARCH`] counts, one as much as a
/// whole crown: a canopy with gaps in it is still where the shade is, and
/// a player has no way to count leaves by looking up. Which blocks are
/// leaves is `types::is_canopy`'s, so a new tree is shade the day it is
/// added. An unloaded column is open sky, as it is for the roof.
fn has_canopy(world: &Arc<World>, gx: i32, gy: i32, gz: i32) -> bool {
    let top = (gy + ROOF_SEARCH).min(CHUNK_SIZE_Y as i32 - 1);
    ((gy + 1)..=top).any(|y| {
        world
            .cached_block(gx, y, gz)
            .is_some_and(primitive_shared::types::is_canopy)
    })
}

/// How much of the pre-dawn chill applies at this hour, 0..1.
///
/// Zero from mid-morning to dusk. From dusk (`0.75`) it rises as the
/// square of the night's progress -- so the first hours of darkness
/// are barely touched and the last are the whole of it -- reaches one
/// at sunrise (`0.25`), and then falls off in a straight line over
/// [`MORNING_THAW`] of a day. The square is the shape of ground that
/// has been radiating for hours rather than minutes; the straight thaw
/// is a choice, and the reason there is a thaw at all is on the
/// constant.
pub fn pre_dawn_chill(time_of_day: f32) -> f32 {
    if !time_of_day.is_finite() {
        return 0.0;
    }
    // 0 at dusk, 1 at sunrise, 2 at the next dusk: half-days, which is
    // why the thaw -- a fraction of a whole day -- is doubled below.
    let night = (time_of_day - 0.75).rem_euclid(1.0) / 0.5;
    if night < 1.0 {
        night * night
    } else {
        (1.0 - (night - 1.0) / (MORNING_THAW * 2.0)).clamp(0.0, 1.0)
    }
}

/// What kind of shelter a cell is in.
///
/// Reads the cache and only the cache; an unloaded column counts as
/// open sky, which is the safe answer -- being told you are sheltered
/// when you are not is how a player freezes in the rain.
pub fn shelter_at(world: &Arc<World>, gx: i32, gy: i32, gz: i32) -> Shelter {
    if !has_roof(world, gx, gy, gz) {
        return Shelter::Open;
    }
    if has_walls(world, gx, gy, gz) {
        Shelter::Enclosed
    } else {
        Shelter::Roofed
    }
}

/// Is there anything solid over this cell?
///
/// Walks up to [`ROOF_SEARCH`] cells, stopping at the first thing that
/// stops light.
fn has_roof(world: &Arc<World>, gx: i32, gy: i32, gz: i32) -> bool {
    let top = (gy + ROOF_SEARCH).min(CHUNK_SIZE_Y as i32 - 1);
    for y in (gy + 1)..=top {
        let Some(block) = world.cached_block(gx, y, gz) else {
            return false;
        };
        if blocks_the_sky(block) {
            return true;
        }
    }
    false
}

/// How many sky-stopping blocks there are over this cell, within
/// [`ROOF_SEARCH`]: the thickness of what lies between it and the weather,
/// for [`earth_shares`].
///
/// Every such block counts, not only an unbroken run from the ceiling up: a
/// cave with a pocket of air in its roof is still under that much rock. An
/// unloaded cell ends the count, as it ends the roof search, and for the same
/// reason -- a guess of shelter is how a player freezes.
fn cover_over(world: &Arc<World>, gx: i32, gy: i32, gz: i32) -> u32 {
    let top = (gy + ROOF_SEARCH).min(CHUNK_SIZE_Y as i32 - 1);
    let mut cover = 0;
    for y in (gy + 1)..=top {
        let Some(block) = world.cached_block(gx, y, gz) else {
            break;
        };
        if blocks_the_sky(block) {
            cover += 1;
        }
    }
    cover
}

/// Are there walls round this cell?
///
/// In each of the four horizontal directions, walks out to
/// [`WALL_SEARCH`] cells looking for something that stops the sky, at
/// **both** the feet and the head: a wall a player can see over is a
/// fence, and a slab floating at head height is a shelf. Anything that
/// does not stop the sky -- a tuft of grass, a torch, a fire, a doorway
/// -- is walked through. [`WALLS_NEEDED`] of the four is a room.
///
/// The same `blocks_the_sky` the roof uses, deliberately: one
/// definition of what keeps the weather out, and it is the lighting
/// engine's own. A hedge of leaves is not a wall for the same reason a
/// canopy is not a roof.
fn has_walls(world: &Arc<World>, gx: i32, gy: i32, gz: i32) -> bool {
    let head = (gy + 1).min(CHUNK_SIZE_Y as i32 - 1);
    let walled_at = |dx: i32, dz: i32, y: i32| {
        (1..=WALL_SEARCH).any(|step| {
            world
                .cached_block(gx + dx * step, y, gz + dz * step)
                .is_some_and(blocks_the_sky)
        })
    };
    let walls = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter(|&(dx, dz)| walled_at(dx, dz, gy) && walled_at(dx, dz, head))
        .count() as u32;
    walls >= WALLS_NEEDED
}

// What counts as a roof used to be written out here. It is
// `types::blocks_the_sky` now, because the client came to need the same
// answer -- the fog goes black where the sky cannot be seen -- and two
// copies of "what is a ceiling" would be a game that shelters you from
// the rain in a place it draws as open air. See the note on it.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::survival::Vitals;
    use primitive_shared::types::{
        BLOCK_AIR, BLOCK_CAMPFIRE_LIT, BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER,
    };
    use primitive_shared::worldgen::Preset;

    /// A world at the test preset -- whose climate is one value
    /// everywhere, which is exactly what a test of *everything else*
    /// wants -- with the sky over the patch these tests stand on cleared
    /// out.
    ///
    /// The clearing is not tidiness. The test world is a built one: it
    /// has a plaza with things on it, and a sample taken under one of
    /// them would be a sample of shelter rather than of sky. Every test
    /// below is about what the *weather* does, so the weather has to be
    /// able to reach the ground.
    fn flat_world() -> Arc<World> {
        let world = Arc::new(World::with_preset(7, Preset::Test, 1024));
        for cx in -1..=1 {
            for cz in -1..=1 {
                let pos = primitive_shared::types::ChunkPos::new(cx, cz);
                let chunk = world.generate(pos);
                world.insert(chunk);
            }
        }
        let ground = primitive_shared::showcase::GROUND_Y;
        for x in -4..=10 {
            for z in -4..=10 {
                for y in (ground + 1)..primitive_shared::types::CHUNK_SIZE_Y as i32 {
                    world.set_block(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    fn ground(_world: &Arc<World>) -> f32 {
        (primitive_shared::showcase::GROUND_Y + 1) as f32
    }

    fn sample(world: &Arc<World>, fires: &Fires, at: (f32, f32, f32), t: f32, w: Weather) -> Ambient {
        Ambient::of(world, fires, at, t, w)
    }

    /// The world day that puts an hour in the middle of winter, and the
    /// middle of summer. Whole days, so `day + hour` reads as a clock.
    fn midwinter() -> f32 {
        (season::MIDWINTER_DAY_OF_YEAR - season::WORLD_OPENS_ON_DAY).floor()
    }
    fn midsummer() -> f32 {
        (season::MIDSUMMER_DAY_OF_YEAR - season::WORLD_OPENS_ON_DAY).floor()
    }

    /// The world day whose *night* carries no seasonal offset to speak
    /// of: the autumn day the cosine crosses zero on its way down.
    ///
    /// Not day zero. A world opens in late spring on the way up,
    /// and its first nights are already on the warm side
    /// -- kind to a player with nothing, and the wrong reference for a
    /// test of what the night itself does. Derived from the constants
    /// rather than written as `4.0`, so retuning the season moves it.
    fn no_offset_day() -> f32 {
        let mid = (season::SUMMER_PEAK_C + season::WINTER_TROUGH_C) * 0.5;
        let amplitude = (season::SUMMER_PEAK_C - season::WINTER_TROUGH_C) * 0.5;
        let crossing = season::MIDSUMMER_DAY_OF_YEAR
            + (-mid / amplitude).acos() / std::f32::consts::TAU * season::YEAR_DAYS
            - season::WORLD_OPENS_ON_DAY;
        // The night straddling the crossing starts the evening before.
        let day = (crossing - 1.0).round();
        let at_midnight = season::ambient_offset_c(day + 1.0);
        assert!(at_midnight.abs() < 0.5, "the reference night is {at_midnight} degrees off");
        day
    }

    /// A player's cell at `(x, z)` on the ground, with a stone roof
    /// three up and stone walls `reach` cells out on four sides, two
    /// high. A hut with no door; the tests knock a wall out when they
    /// want one.
    fn build_room(world: &Arc<World>, x: i32, z: i32, reach: i32) {
        let y = ground(world) as i32;
        world.set_block(x, y + 3, z, BLOCK_STONE);
        for (wx, wz) in [(x + reach, z), (x - reach, z), (x, z + reach), (x, z - reach)] {
            world.set_block(wx, y, wz, BLOCK_STONE);
            world.set_block(wx, y + 1, wz, BLOCK_STONE);
        }
    }

    fn build_hut(world: &Arc<World>, x: i32, z: i32) {
        build_room(world, x, z, 2);
    }

    /// What a night does to a bare body.
    ///
    /// Runs a player from dusk to mid-morning at the default clock
    /// (fifteen minutes a day), sampling the world twice a second the
    /// way the tick loop does and stepping the body every tick, and
    /// reports the coldest the skin and the air got, what the skin was
    /// at sunrise, and how much health the night took. Mid-morning
    /// rather than sunrise because the body lags the air by minutes
    /// and the coldest skin of the night is a little after the sun is
    /// up. The whole chain -- generator, sky, shelter, fire, body,
    /// damage -- because the claim under test is about what a *player*
    /// experiences and not about any one term.
    struct Night {
        coldest_c: f32,
        coldest_air_c: f32,
        at_sunrise_c: f32,
        health_lost: f32,
        survived: bool,
    }

    fn a_night(world: &Arc<World>, fires: &Fires, at: (f32, f32, f32), day: f32) -> Night {
        const DAY_SECONDS: f32 = 900.0;
        const TICK: f32 = 0.05;
        let ticks_per_sample = (SAMPLE_INTERVAL_SECS / TICK) as usize;
        let sunrise = (0.5 * DAY_SECONDS / TICK) as usize;
        let ticks = (0.6 * DAY_SECONDS / TICK) as usize;
        let mut vitals = Vitals::new();
        let mut coldest = body::NEUTRAL_C;
        let mut coldest_air = f32::INFINITY;
        let mut at_sunrise = body::NEUTRAL_C;
        let mut ambient = Ambient::default();
        for tick in 0..ticks {
            let hour = 0.75 + tick as f32 * TICK / DAY_SECONDS;
            if tick % ticks_per_sample == 0 {
                ambient = sample(world, fires, at, day + hour, Weather::Clear);
                coldest_air = coldest_air.min(ambient.temperature_c);
            }
            vitals.warm(ambient.exposure(), 0.0, 0.0, 0.0, TICK);
            coldest = coldest.min(vitals.temperature());
            if tick == sunrise {
                at_sunrise = vitals.temperature();
            }
        }
        Night {
            coldest_c: coldest,
            coldest_air_c: coldest_air,
            at_sunrise_c: at_sunrise,
            health_lost: crate::logic::survival::MAX_HEALTH - vitals.health(),
            survived: !vitals.is_dead(),
        }
    }

    #[test]
    fn night_is_colder_than_noon() {
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let noon = sample(&world, &fires, (0.5, y, 0.5), 0.5, Weather::Clear);
        let midnight = sample(&world, &fires, (0.5, y, 0.5), 0.0, Weather::Clear);
        assert!(
            midnight.temperature_c < noon.temperature_c - 5.0,
            "midnight {} was not much colder than noon {}",
            midnight.temperature_c,
            noon.temperature_c
        );
    }

    #[test]
    fn the_coldest_hour_is_the_one_before_dawn() {
        // The shape of the night, stated: it gets colder all night and
        // the worst of it is just before sunrise, not at midnight -- and
        // by mid-morning it has lifted.
        let world = flat_world();
        let fires = Fires::new();
        let at = (0.5, ground(&world), 0.5);
        // On a day with no season on it, so the season's own drift
        // across the small hours -- most of a degree on the day the
        // world opens -- does not stand in for the night's shape.
        let day = no_offset_day();
        let read = |t: f32| sample(&world, &fires, at, day + t, Weather::Clear).temperature_c;
        let dusk = read(0.75);
        let midnight = read(1.0);
        // Half past four. The cosine has begun to recover by then and
        // the ground has not, and the two together bottom out here --
        // a degree or two under midnight rather than ten, because the
        // cosine is most of the night and this term is the rest of it.
        let before_dawn = read(1.18);
        let mid_morning = read(1.4);
        assert!(midnight < dusk, "midnight {midnight} was no colder than dusk {dusk}");
        assert!(
            before_dawn < midnight - 1.0,
            "the hour before dawn ({before_dawn}) was not the coldest; midnight was {midnight}"
        );
        assert!(
            mid_morning > before_dawn + 8.0,
            "mid-morning {mid_morning} had not thawed from {before_dawn}"
        );
        // ...and the curve has no step in it anywhere: no minute of the
        // day moves the air by more than a degree.
        let mut t = 0.75;
        while t < 1.75 {
            let step = (read(t + 1.0 / 1440.0) - read(t)).abs();
            assert!(step < 1.0, "the air jumped {step} degrees in a minute at {t}");
            t += 1.0 / 96.0;
        }
        assert_eq!(pre_dawn_chill(0.5), 0.0);
        assert!((pre_dawn_chill(0.25) - 1.0).abs() < 1e-5);
        assert_eq!(pre_dawn_chill(f32::NAN), 0.0);
    }

    #[test]
    fn a_player_outdoors_on_a_temperate_night_without_a_fire_is_chilled_by_dawn() {
        // The claim the whole change rests on, with no season on it: a
        // night in the open with nothing on ends Cold -- hunger running
        // at double, the gauge on screen -- with the air heading for
        // the line where it starts to cost health. Not over it: without
        // a season the night is the warning. The season is the verdict
        // (below), and the two have to arrive in that order.
        //
        // The test meadow is three degrees warmer than the real spawn
        // column (18.8 against 16 at noon), so every number here is a
        // shade kinder than a new world's; the same run on the spawn
        // column bottoms the air at zero exactly.
        let world = flat_world();
        let fires = Fires::new();
        let at = (0.5, ground(&world), 0.5);
        let night = a_night(&world, &fires, at, no_offset_day());
        assert!(
            night.at_sunrise_c < body::CHILLED,
            "a night in the open left the skin at {} at sunrise, which is not even cold",
            night.at_sunrise_c
        );
        let heading_for = body::felt_ambient(night.coldest_air_c, 0.0, 0.0);
        assert!(
            heading_for < body::FREEZING + 4.0,
            "the air bottomed out at {} ({heading_for} on the skin), nowhere near the line",
            night.coldest_air_c
        );
        assert!(night.survived, "a night with no season on it killed a healthy player");
        assert!(
            night.health_lost < 1.0,
            "a night with no season on it took {} health -- that is winter's job",
            night.health_lost
        );

        // ...and the season turns the warning into the verdict, in
        // steps a player can feel coming. Measured on this meadow: the
        // open night costs nothing until the offset passes about minus
        // seven, the last night of autumn takes a few points, and by
        // midwinter it kills a healthy bare player where they lie --
        // which, after a season of Cold nights and a night of blood,
        // is the sentence "a house or a fire is mandatory" meant.
        // As shares of the year rather than days, so the year's length can
        // change without the nights moving to other seasons: these were
        // three and two days when the year was twelve, and a forty-day
        // year that kept "three" put mid-autumn a week into winter.
        let autumn_day = midwinter() - (season::YEAR_DAYS * 0.225).round();
        let mid_autumn = a_night(&world, &fires, at, autumn_day);
        assert_eq!(mid_autumn.health_lost, 0.0, "a mid-autumn night already drew blood");
        assert!(mid_autumn.coldest_c < body::CHILLED, "a mid-autumn night was not cold: {}", mid_autumn.coldest_c);

        let last_of_autumn = a_night(&world, &fires, at, midwinter() - (season::YEAR_DAYS * 0.15).round());
        assert!(
            last_of_autumn.health_lost > 0.5,
            "the last night of autumn cost nothing -- winter arrives as a wall"
        );
        assert!(last_of_autumn.survived, "the last night of autumn killed a healthy player");

        let winter = a_night(&world, &fires, at, midwinter());
        assert!(
            winter.coldest_c < body::FREEZING,
            "a midwinter night in the open never crossed the line: {}",
            winter.coldest_c
        );
        assert!(
            winter.health_lost > last_of_autumn.health_lost + 5.0,
            "midwinter ({}) was no worse than the last night of autumn ({})",
            winter.health_lost,
            last_of_autumn.health_lost
        );
        assert!(!winter.survived, "a midwinter night in the open, bare, with no fire, was survivable");
    }

    #[test]
    fn a_roof_and_three_walls_take_the_bite_out_of_the_night() {
        // Three nights, one meadow: open, under a roof, behind walls.
        // They have to be three *different* nights or the player cannot
        // tell what they built.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let day = no_offset_day();
        let open = a_night(&world, &fires, (0.5, y, 0.5), day);

        world.set_block(4, y as i32 + 3, 4, BLOCK_STONE);
        assert_eq!(shelter_at(&world, 4, y as i32, 4), Shelter::Roofed);
        let roofed = a_night(&world, &fires, (4.5, y, 4.5), day);

        build_hut(&world, 8, 8);
        assert_eq!(shelter_at(&world, 8, y as i32, 8), Shelter::Enclosed);
        let walled = a_night(&world, &fires, (8.5, y, 8.5), day);

        // Measured: nineteen, twenty-five and twenty-eight on the skin
        // at the coldest. Cold, cool, comfortable.
        assert!(
            open.coldest_c < body::CHILLED,
            "a night in the open was not even cold: {}",
            open.coldest_c
        );
        assert!(
            roofed.coldest_c > open.coldest_c + 2.0,
            "a roof was worth nothing: {} under it against {} in the open",
            roofed.coldest_c,
            open.coldest_c
        );
        assert!(
            walled.coldest_c > roofed.coldest_c + 2.0,
            "walls were worth nothing over a roof: {} against {}",
            walled.coldest_c,
            roofed.coldest_c
        );
        assert!(
            walled.coldest_c > body::COMFORT_LOW,
            "a night in a hut with no season on it was not comfortable: {}",
            walled.coldest_c
        );
        assert_eq!(walled.health_lost, 0.0);

        // A house without a fire is merely cold: in midwinter the hut
        // keeps the night out and the season in, and the player is Cold
        // and unhurt.
        let winter_hut = a_night(&world, &fires, (8.5, y, 8.5), midwinter());
        assert!(
            winter_hut.coldest_c < body::COMFORT_LOW,
            "an unheated hut in midwinter was comfortable: {}",
            winter_hut.coldest_c
        );
        assert!(
            winter_hut.coldest_c > body::FREEZING,
            "an unheated hut in midwinter cost health: skin at {}",
            winter_hut.coldest_c
        );
        assert_eq!(winter_hut.health_lost, 0.0);
    }

    #[test]
    fn three_walls_are_a_room_and_two_are_a_passage() {
        // The doorway is allowed and the corridor is not. A player who
        // leaves the door open has still built a house; a player in a
        // straight tunnel has not.
        let world = flat_world();
        let y = ground(&world) as i32;
        build_hut(&world, 5, 5);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Enclosed);

        // Knock a doorway through: still a room.
        world.set_block(7, y, 5, BLOCK_AIR);
        world.set_block(7, y + 1, 5, BLOCK_AIR);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Enclosed, "a doorway unmade the house");

        // Knock the opposite wall out too: a passage, and the wind is
        // through it.
        world.set_block(3, y, 5, BLOCK_AIR);
        world.set_block(3, y + 1, 5, BLOCK_AIR);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Roofed, "a corridor counted as a room");

        // A fence -- walls one block high -- is not a wall.
        world.set_block(3, y, 5, BLOCK_STONE);
        world.set_block(7, y, 5, BLOCK_STONE);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Roofed, "a fence counted as a wall");

        // ...nor is a hedge, for the reason a canopy is not a roof.
        world.set_block(3, y + 1, 5, BLOCK_LEAVES);
        world.set_block(7, y + 1, 5, BLOCK_LEAVES);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Roofed, "a hedge counted as a wall");

        // And walls with no roof are nothing at all -- see `Shelter`.
        world.set_block(5, y + 3, 5, BLOCK_AIR);
        world.set_block(3, y + 1, 5, BLOCK_STONE);
        world.set_block(7, y + 1, 5, BLOCK_STONE);
        assert_eq!(shelter_at(&world, 5, y, 5), Shelter::Open, "a walled yard counted as shelter");
    }

    #[test]
    fn a_lit_campfire_in_a_hut_makes_a_winter_night_survivable() {
        // The end of the progression: roof, walls, fire, midwinter,
        // comfortable. And the fire has to be *in the room* for the
        // room to matter -- the same fire four blocks off in the open
        // is a fire the cold still bites at the edge of.
        let world = flat_world();
        let y = ground(&world);
        let mut fires = Fires::new();

        // A big room -- walls at the limit of the search -- with the
        // fire in its far corner, four and a half blocks from where the
        // player stands: well past the distance at which a fire in the
        // open still keeps somebody comfortable.
        let player = (8.5, y, 8.5);
        build_room(&world, 8, 8, WALL_SEARCH);
        assert_eq!(shelter_at(&world, 8, y as i32, 8), Shelter::Enclosed);
        let hearth = (8 + 4, y as i32, 8 + 2);
        world.set_block(hearth.0, hearth.1, hearth.2, BLOCK_CAMPFIRE_LIT);
        fires.light(hearth);
        let warmed = a_night(&world, &fires, player, midwinter());
        assert!(
            warmed.coldest_c >= body::COMFORT_LOW,
            "a room with a fire in it was cold in midwinter: skin at {}",
            warmed.coldest_c
        );
        assert_eq!(warmed.health_lost, 0.0);

        // The same fire at the same distance, no walls: the edge bites.
        let mut open_fires = Fires::new();
        let camp = (4, y as i32, 2);
        world.set_block(camp.0, camp.1, camp.2, BLOCK_CAMPFIRE_LIT);
        open_fires.light(camp);
        let at_the_edge = sample(&world, &open_fires, (0.5, y, 0.5), midwinter() + 0.2, Weather::Clear);
        assert!(at_the_edge.near_fire);
        let in_the_room = sample(&world, &fires, player, midwinter() + 0.2, Weather::Clear);
        assert!(
            in_the_room.temperature_c > at_the_edge.temperature_c + 5.0,
            "a fire in a room ({}) was worth no more than one in the open ({}) at the same distance",
            in_the_room.temperature_c,
            at_the_edge.temperature_c
        );
        assert!(
            body::felt_ambient(at_the_edge.temperature_c, 0.0, 0.0) < body::COMFORT_LOW,
            "the edge of a fire in the open in midwinter was comfortable: {}",
            at_the_edge.temperature_c
        );
    }

    #[test]
    fn a_temperate_day_is_still_comfortable_in_nothing_but_skin() {
        // The other half of making the night cold: the day must not
        // be. From mid-morning to late afternoon -- on the day the
        // world opens, on a day with no season on it, and in midsummer
        // -- a bare player in the test meadow is inside the comfortable
        // band; in midwinter the same noon is cold and never harmful.
        // Late afternoon rather than dusk: by dusk the cosine has
        // taken five degrees off and the skin is at the band's edge,
        // which is the evening telling you to go in.
        let world = flat_world();
        let fires = Fires::new();
        let at = (0.5, ground(&world), 0.5);
        for day in [0.0, no_offset_day(), midsummer()] {
            let mut hour = 0.4;
            while hour <= 0.7 {
                // Under the sun as well as in its air. The sun is real
                // here and small (`SUN_FROM_C`), and this is what keeps
                // it small: a meadow noon that pushed bare skin out of
                // the band would be hot country everywhere.
                let ambient = sample(&world, &fires, at, day + hour, Weather::Clear);
                let air = ambient.temperature_c;
                let skin = body::felt_under_sky(ambient.exposure(), 0.0, 0.0, 0.0);
                assert!(
                    (body::COMFORT_LOW..=body::COMFORT_HIGH).contains(&skin),
                    "day {day} at {hour}: air {air}, sun {}, skin heading for {skin}",
                    ambient.sun_c
                );
                hour += 1.0 / 48.0;
            }
        }
        let winter_noon = sample(&world, &fires, at, midwinter() + 0.5, Weather::Clear).temperature_c;
        let skin = body::felt_ambient(winter_noon, 0.0, 0.0);
        assert!(skin < body::COMFORT_LOW, "a midwinter noon was comfortable bare: {skin}");
        assert!(skin > body::FREEZING, "a midwinter noon cost health bare: {skin}");
    }

    #[test]
    fn the_season_moves_every_temperature_by_the_same_amount() {
        // The season is one number added everywhere, and it is added
        // *before* shelter so a hut does not keep winter out. Open air
        // and a closed room at the same hour in summer and winter have
        // to differ by exactly the offset.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        build_hut(&world, 6, 6);
        let offset = season::ambient_offset_c(midsummer() + 0.5) - season::ambient_offset_c(midwinter() + 0.5);
        for at in [(0.5, y, 0.5), (6.5, y, 6.5)] {
            let summer = sample(&world, &fires, at, midsummer() + 0.5, Weather::Clear).temperature_c;
            let winter = sample(&world, &fires, at, midwinter() + 0.5, Weather::Clear).temperature_c;
            assert!(
                (summer - winter - offset).abs() < 0.05,
                "at {at:?} summer {summer} and winter {winter} differ by other than the offset {offset}"
            );
        }
        // ...and the span the season is written against is the one the
        // server actually uses. See `season::CLIMATE_SPAN_C`.
        assert_eq!(season::CLIMATE_SPAN_C, HOTTEST_C - COLDEST_C);
    }

    #[test]
    fn the_mean_pre_dawn_chill_is_the_average_of_the_chill() {
        let samples = 20_000;
        let sum: f32 = (0..samples)
            .map(|i| pre_dawn_chill(i as f32 / samples as f32))
            .sum();
        let mean = sum / samples as f32;
        assert!(
            (mean - MEAN_PRE_DAWN_CHILL).abs() < 0.002,
            "the chill averages {mean}, the constant says {MEAN_PRE_DAWN_CHILL}"
        );
    }

    #[test]
    fn a_deep_cave_is_cool_in_summer_and_mild_in_winter() {
        // A chamber eight blocks under the meadow, with seven of earth over
        // it: the place a root cellar is dug.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let floor = primitive_shared::showcase::GROUND_Y - 7;
        for x in -1..=1 {
            for z in -1..=1 {
                world.set_block(x, floor, z, BLOCK_AIR);
                world.set_block(x, floor + 1, z, BLOCK_AIR);
            }
        }
        let cave = (0.5, floor as f32, 0.5);
        let open = (6.5, y, 6.5);
        let at = |where_: (f32, f32, f32), t: f32| sample(&world, &fires, where_, t, Weather::Clear).temperature_c;

        let summer_noon = midsummer() + 0.5;
        let winter_dawn = midwinter() + 0.24;
        assert!(
            at(cave, summer_noon) < at(open, summer_noon) - 5.0,
            "a cave at midsummer noon ({}) was not cooler than the meadow ({})",
            at(cave, summer_noon),
            at(open, summer_noon)
        );
        assert!(
            at(cave, winter_dawn) > at(open, winter_dawn) + 15.0,
            "a cave before a midwinter dawn ({}) was not milder than the meadow ({})",
            at(cave, winter_dawn),
            at(open, winter_dawn)
        );
        // The year hardly reaches it: less than a fifth of the season's
        // swing between midsummer noon and midwinter dawn.
        let cave_swing = at(cave, summer_noon) - at(cave, winter_dawn);
        let open_swing = at(open, summer_noon) - at(open, winter_dawn);
        assert!(
            cave_swing.abs() < open_swing * 0.2,
            "the cave swung {cave_swing} against the meadow's {open_swing}"
        );
        // ...and in a temperate meadow that average is cellar-cold: under
        // the line food keeps longer at, over the frost.
        let mean = at(cave, midsummer() + 0.5);
        assert!(
            mean > crate::logic::rot::KEEPS_BELOW_C && mean < crate::logic::rot::COOL_BELOW_C,
            "a temperate cellar sits at {mean}, not between freezing and cool"
        );
    }

    #[test]
    fn one_roof_is_not_a_cellar() {
        assert_eq!(earth_shares(0), (0.0, 0.0));
        assert_eq!(earth_shares(1), (0.0, 0.0));
        let (daily, annual) = earth_shares(3);
        assert_eq!(daily, 1.0, "two blocks past the roof still let the night in");
        assert!(annual > 0.4 && annual < 0.7, "three blocks kept out {annual} of the year");
    }

    #[test]
    fn rain_is_colder_than_a_clear_sky_and_a_storm_is_colder_still() {
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let clear = sample(&world, &fires, (0.5, y, 0.5), 0.5, Weather::Clear);
        let rain = sample(&world, &fires, (0.5, y, 0.5), 0.5, Weather::Rain);
        let storm = sample(&world, &fires, (0.5, y, 0.5), 0.5, Weather::Storm);
        assert!(rain.temperature_c < clear.temperature_c);
        assert!(storm.temperature_c < rain.temperature_c);
        assert!(rain.getting_wet && storm.getting_wet && !clear.getting_wet);
    }

    #[test]
    fn a_roof_keeps_the_rain_and_the_night_off() {
        // Both halves through the one test, because they are the one
        // mechanism: the roof is asked for once and used twice.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let at = (0.5, y, 0.5);
        let open = sample(&world, &fires, at, 0.5, Weather::Storm);
        assert!(!open.sheltered);

        world.set_block(0, y as i32 + 3, 0, BLOCK_STONE);
        let under = sample(&world, &fires, at, 0.5, Weather::Storm);
        assert!(under.sheltered, "a stone slab overhead was not a roof");
        assert!(!under.getting_wet, "it rained through a stone roof");
        assert!(under.temperature_c > open.temperature_c);

        // ...and a canopy does *not*, which is the other half of using
        // the lighting engine's own number rather than a list of block
        // names: leaves are nearly transparent to it, and a wood is not
        // a house.
        world.set_block(0, y as i32 + 3, 0, BLOCK_AIR);
        world.set_block(0, y as i32 + 4, 0, BLOCK_LEAVES);
        let wooded = sample(&world, &fires, at, 0.5, Weather::Clear);
        assert!(!wooded.sheltered, "a single leaf counted as a house");
    }

    #[test]
    fn a_fire_is_the_answer_to_a_cold_night() {
        // The claim the whole mechanic rests on: when a player gets cold
        // the first thing they will try is a fire, and it has to work.
        let world = flat_world();
        let y = ground(&world);
        let mut fires = Fires::new();
        let cell = (2, y as i32, 0);
        world.set_block(cell.0, cell.1, cell.2, BLOCK_CAMPFIRE_LIT);
        fires.light(cell);

        let cold = sample(&world, &Fires::new(), (2.5, y, 0.5), 0.0, Weather::Storm);
        let warm = sample(&world, &fires, (2.5, y, 0.5), 0.0, Weather::Storm);
        assert!(warm.near_fire);
        assert!(
            warm.temperature_c > body::COMFORT_LOW,
            "a fire in a storm at midnight left a player at {}",
            warm.temperature_c
        );
        assert!(warm.temperature_c > cold.temperature_c + 10.0);
    }

    #[test]
    fn a_fire_across_the_camp_is_not_a_fire_you_are_sitting_at() {
        let world = flat_world();
        let y = ground(&world);
        let mut fires = Fires::new();
        let cell = (0, y as i32, 0);
        world.set_block(cell.0, cell.1, cell.2, BLOCK_CAMPFIRE_LIT);
        fires.light(cell);

        let close = sample(&world, &fires, (0.5, y, 1.0), 0.0, Weather::Clear);
        let across = sample(&world, &fires, (0.5, y, 4.5), 0.0, Weather::Clear);
        assert!(close.temperature_c > across.temperature_c);
        // ...and past the range it is not there at all.
        let away = sample(&world, &fires, (0.5, y, 12.0), 0.0, Weather::Clear);
        assert!(!away.near_fire);
        // The last block inside the range is the falloff and nothing
        // else: a fire's edge on a winter night is a winter night. It
        // used to be a strip of exactly zero degrees.
        let no_fire = sample(&world, &Fires::new(), (0.5, y, 5.4), midwinter() + 0.2, Weather::Clear);
        let edge = sample(&world, &fires, (0.5, y, 5.4), midwinter() + 0.2, Weather::Clear);
        assert!(edge.near_fire, "the edge was not inside the range");
        assert!(
            edge.temperature_c - no_fire.temperature_c < 2.0,
            "the very edge of a fire was worth {} degrees",
            edge.temperature_c - no_fire.temperature_c
        );
    }

    #[test]
    fn standing_in_water_is_cold_and_makes_you_wet() {
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        world.set_block(0, y as i32, 0, BLOCK_WATER);
        let wet = sample(&world, &fires, (0.5, y, 0.5), 0.5, Weather::Clear);
        assert!(wet.in_water && wet.getting_wet);
        assert!(wet.temperature_c <= WATER_C);
    }

    #[test]
    fn getting_wet_is_fast_and_drying_off_is_not() {
        let soaking = Ambient {
            in_water: true,
            getting_wet: true,
            ..Ambient::default()
        };
        let dry_air = Ambient::default();
        let after_a_dunk = soaking.step_wetness(0.0, 2.0);
        assert!(after_a_dunk > 0.5, "two seconds in a lake left {after_a_dunk}");
        let ten_seconds_later = dry_air.step_wetness(after_a_dunk, 10.0);
        assert!(
            ten_seconds_later > after_a_dunk * 0.5,
            "a soaking dried off in ten seconds"
        );
        // ...and it does dry, eventually. In steps, because
        // `step_wetness` clamps its own `dt` -- a frame that took five
        // seconds must not be five seconds of rain applied at once, and
        // the clamp is what says so.
        let mut soaked = 1.0;
        for _ in 0..60 {
            soaked = dry_air.step_wetness(soaked, 1.0);
        }
        assert_eq!(soaked, 0.0);
    }

    #[test]
    fn a_player_in_an_unloaded_column_is_left_alone() {
        // A teleport puts somebody in a chunk nobody has for a tick or
        // two. Guessing "very cold" there would mean a `/tp` across the
        // map came with a dose of hypothermia.
        let world = Arc::new(World::with_preset(7, Preset::Test, 1024));
        let fires = Fires::new();
        let out_there = sample(&world, &fires, (900_000.0, 30.0, 900_000.0), 0.0, Weather::Storm);
        assert_eq!(out_there, Ambient::default());
    }

    #[test]
    fn a_nonsense_position_or_time_is_refused_rather_than_floored() {
        let world = flat_world();
        let fires = Fires::new();
        for bad in [
            (f32::NAN, 30.0, 0.0),
            (0.0, f32::INFINITY, 0.0),
            (0.0, 30.0, f32::NEG_INFINITY),
        ] {
            assert_eq!(sample(&world, &fires, bad, 0.5, Weather::Clear), Ambient::default());
        }
        // A nonsense clock is noon on the day the world opened, not a
        // NaN that the body then drifts toward.
        let at = (0.5, ground(&world), 0.5);
        let odd = sample(&world, &fires, at, f32::NAN, Weather::Clear);
        assert!(odd.temperature_c.is_finite());
        assert_eq!(odd, sample(&world, &fires, at, 0.5, Weather::Clear));
    }

    // ---- the heat ----

    /// The hot desert: the climate field at warmth 0.9, very dry -- the
    /// generator's hot tenth. Its air at a noon with no season: 38.4.
    const DESERT: (f32, f32) = (0.9, 0.1);

    fn in_the_desert(world: &Arc<World>, fires: &Fires, at: (f32, f32, f32), t: f32, w: Weather) -> Ambient {
        Ambient::of_in(world, fires, at, t, w, |_, _, _| DESERT)
    }

    /// What a clear desert day does to a body that stands in one place
    /// from sunrise to dusk -- `a_night`'s other half, through the same
    /// chain: the sample twice a second, the body every tick.
    struct Day {
        hottest_c: f32,
        warm_after_s: Option<f32>,
        scorching_after_s: Option<f32>,
        health_lost: f32,
    }

    fn a_hot_day(
        world: &Arc<World>,
        at: (f32, f32, f32),
        day: f32,
        climate: (f32, f32),
        insulation: f32,
        shade: f32,
    ) -> Day {
        const DAY_SECONDS: f32 = 900.0;
        const TICK: f32 = 0.05;
        let fires = Fires::new();
        let ticks_per_sample = (SAMPLE_INTERVAL_SECS / TICK) as usize;
        let ticks = (0.5 * DAY_SECONDS / TICK) as usize;
        let mut vitals = Vitals::new();
        let mut result = Day {
            hottest_c: f32::NEG_INFINITY,
            warm_after_s: None,
            scorching_after_s: None,
            health_lost: 0.0,
        };
        let mut ambient = Ambient::default();
        for tick in 0..ticks {
            let hour = 0.25 + tick as f32 * TICK / DAY_SECONDS;
            if tick % ticks_per_sample == 0 {
                ambient = Ambient::of_in(world, &fires, at, day + hour, Weather::Clear, |_, _, _| climate);
            }
            vitals.warm(ambient.exposure(), insulation, shade, 0.0, TICK);
            let skin = vitals.temperature();
            let seconds = tick as f32 * TICK;
            result.hottest_c = result.hottest_c.max(skin);
            if skin > body::OVERHEATED && result.warm_after_s.is_none() {
                result.warm_after_s = Some(seconds);
            }
            if skin > body::SCALDING && result.scorching_after_s.is_none() {
                result.scorching_after_s = Some(seconds);
            }
        }
        result.health_lost = crate::logic::survival::MAX_HEALTH - vitals.health();
        result
    }

    #[test]
    fn the_open_desert_at_noon_overheats_a_bare_player_and_the_shade_of_a_roof_does_not() {
        let world = flat_world();
        let y = ground(&world);
        let day = no_offset_day();
        let open = a_hot_day(&world, (0.5, y, 0.5), day, DESERT, 0.0, 0.0);
        world.set_block(4, y as i32 + 3, 4, BLOCK_STONE);
        let roofed = a_hot_day(&world, (4.5, y, 4.5), day, DESERT, 0.0, 0.0);
        // Measured, on a clear day whose noon the season has put two
        // degrees on the warm side: Warm 142 seconds after sunrise
        // (09:47), heatstroke at 204 (11:26), and a player who never
        // moves is dead by the afternoon. Under the roof the same day
        // peaks at 39.1 -- Warm, and unhurt.
        let scorched = open
            .scorching_after_s
            .unwrap_or_else(|| panic!("a whole bare day in the open desert peaked at {}", open.hottest_c));
        let warned = open.warm_after_s.expect("heatstroke with no Warm before it");
        assert!(scorched < 300.0, "heatstroke took {scorched}s of standing in the desert sun");
        assert!(
            scorched - warned > 45.0,
            "Warm at {warned}s and heatstroke at {scorched}s: the warning is not one"
        );
        assert!(open.health_lost > 5.0, "a day stood in the open desert cost {}", open.health_lost);
        assert!(
            roofed.hottest_c < body::SCALDING,
            "a day under a roof in the same desert reached {}",
            roofed.hottest_c
        );
        assert_eq!(roofed.health_lost, 0.0);
    }

    /// A player's worn set, from the clothing blocks in slot order.
    ///
    /// **`GARMENT_SLOTS` and not `SLOTS`**: this is about what the weather
    /// finds, and the fifth slot is a rucksack (`equipment::Slot::Back`),
    /// which has no insulation and no shade. Spelled the narrow way round so
    /// that the day a sixth *garment* slot appears these arrays fail to
    /// compile, which is the reminder wanted, and the day another carried
    /// thing appears they do not.
    fn dressed_in(
        blocks: [primitive_shared::types::BlockId; primitive_shared::equipment::GARMENT_SLOTS],
    ) -> primitive_shared::equipment::Worn {
        let mut pieces = [None; primitive_shared::equipment::SLOTS];
        for (piece, block) in pieces.iter_mut().zip(blocks) {
            *piece = primitive_shared::equipment::garment(block);
        }
        primitive_shared::equipment::Worn::total(pieces)
    }

    fn full_cloth() -> primitive_shared::equipment::Worn {
        use primitive_shared::types::{BLOCK_CLOTH_CAP, BLOCK_CLOTH_TROUSERS, BLOCK_CLOTH_TUNIC, BLOCK_CLOTH_WRAPS};
        dressed_in([BLOCK_CLOTH_CAP, BLOCK_CLOTH_TUNIC, BLOCK_CLOTH_TROUSERS, BLOCK_CLOTH_WRAPS])
    }

    #[test]
    fn the_savanna_noon_is_hot_in_the_open_and_comfortable_in_cloth_or_shade() {
        // What a hot climate in the savanna is in play: not a killer --
        // that is the desert -- but a noon that asks for a shirt or a
        // tree. Measured, on the same clear day as the desert above: bare
        // in the open the skin peaks at 40.8 (Warm, thirsty, unhurt), in
        // cloth at 32.6, under a roof at 30.1. In midsummer the open
        // noon peaks at 44.7, a degree off heatstroke.
        const SAVANNA: (f32, f32) = (0.72, 0.55);
        let world = flat_world();
        let y = ground(&world);
        let day = no_offset_day();
        world.set_block(4, y as i32 + 3, 4, BLOCK_STONE);
        let cloth = full_cloth();
        let open = a_hot_day(&world, (0.5, y, 0.5), day, SAVANNA, 0.0, 0.0);
        let dressed = a_hot_day(&world, (0.5, y, 0.5), day, SAVANNA, cloth.insulation, cloth.shade);
        let shaded = a_hot_day(&world, (4.5, y, 4.5), day, SAVANNA, 0.0, 0.0);
        assert!(
            open.warm_after_s.is_some(),
            "a bare savanna noon in the open never got hot: the skin peaked at {}",
            open.hottest_c
        );
        assert_eq!(open.health_lost, 0.0, "a savanna noon hurt a bare player; that is the desert's job");
        for (what, day) in [("cloth", dressed), ("the shade", shaded)] {
            assert!(
                day.hottest_c <= body::OVERHEATED,
                "a savanna noon in {what} still reached {}",
                day.hottest_c
            );
        }
    }

    #[test]
    fn cloth_carries_a_player_through_a_desert_noon_that_kills_one_in_wool() {
        // The decision the cloth rows exist for, through the sun as well
        // as the air. Measured: full cloth in the open desert peaks at
        // 38.0 and is never hurt; full wool is in heatstroke by about
        // eleven in the morning and dead before the afternoon is out.
        use primitive_shared::types::{BLOCK_WOOL_BOOTS, BLOCK_WOOL_CAP, BLOCK_WOOL_LEGGINGS, BLOCK_WOOL_TUNIC};
        let world = flat_world();
        let y = ground(&world);
        let at = (0.5, y, 0.5);
        let day = no_offset_day();
        let cloth = full_cloth();
        let wool = dressed_in([BLOCK_WOOL_CAP, BLOCK_WOOL_TUNIC, BLOCK_WOOL_LEGGINGS, BLOCK_WOOL_BOOTS]);
        let in_cloth = a_hot_day(&world, at, day, DESERT, cloth.insulation, cloth.shade);
        let in_wool = a_hot_day(&world, at, day, DESERT, wool.insulation, wool.shade);
        assert!(
            in_cloth.hottest_c < body::SCALDING && in_cloth.health_lost == 0.0,
            "a desert day in full cloth peaked at {} and cost {}",
            in_cloth.hottest_c,
            in_cloth.health_lost
        );
        assert!(
            in_wool.scorching_after_s.is_some_and(|s| s < 240.0) && in_wool.health_lost > 5.0,
            "a desert day in full wool: heatstroke at {:?}, cost {}",
            in_wool.scorching_after_s,
            in_wool.health_lost
        );
    }

    #[test]
    fn a_tree_canopy_keeps_the_sun_off_and_is_still_no_roof() {
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let at = (0.5, y, 0.5);
        let noon = no_offset_day() + 0.5;
        let open = in_the_desert(&world, &fires, at, noon, Weather::Clear);
        assert!(open.sun_c > SUN_C * 0.99, "the open desert noon had a sun of {}", open.sun_c);

        world.set_block(0, y as i32 + 4, 0, BLOCK_LEAVES);
        let under = in_the_desert(&world, &fires, at, noon, Weather::Clear);
        assert_eq!(under.sun_c, 0.0, "a tree cast no shade");
        assert!(!under.sheltered, "a tree became a roof");
        assert_eq!(
            under.temperature_c, open.temperature_c,
            "the shade changed the air rather than the sun"
        );
        // ...and what that is worth: a whole day under one tree is hot
        // and harmless.
        let rested = a_hot_day(&world, at, no_offset_day(), DESERT, 0.0, 0.0);
        assert!(rested.hottest_c < body::SCALDING, "a day under a tree reached {}", rested.hottest_c);
        assert_eq!(rested.health_lost, 0.0);
    }

    #[test]
    fn the_sun_counts_only_in_hot_country_by_day_and_under_an_open_sky() {
        assert_eq!(solar_heat_c(18.8, 0.5), 0.0, "the temperate noon had a sun");
        assert!((solar_heat_c(SUN_FROM_C + SUN_RAMP_C, 0.5) - SUN_C).abs() < 1e-3);
        for hour in [0.0, 0.1, 0.2, 0.25, 0.75, 0.8, 0.95] {
            assert!(solar_heat_c(40.0, hour) < 1e-3, "a sun of {} at {hour}", solar_heat_c(40.0, hour));
        }
        let mid_morning = solar_heat_c(40.0, 0.375);
        assert!(mid_morning > 0.5 * SUN_C && mid_morning < SUN_C, "nine in the morning had {mid_morning}");
        assert_eq!(solar_heat_c(f32::NAN, 0.5), 0.0);

        // Rain puts it out, and so does a lake.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let noon = no_offset_day() + 0.5;
        assert_eq!(in_the_desert(&world, &fires, (0.5, y, 0.5), noon, Weather::Rain).sun_c, 0.0);
        world.set_block(2, y as i32, 2, BLOCK_WATER);
        assert_eq!(in_the_desert(&world, &fires, (2.5, y, 2.5), noon, Weather::Clear).sun_c, 0.0);
        // The starting meadow in midsummer has a little of it; what that
        // does to a body is `a_temperate_day_is_still_comfortable_in_nothing_but_skin`.
        let meadow = sample(&world, &fires, (0.5, y, 0.5), midsummer() + 0.5, Weather::Clear);
        assert!(meadow.sun_c < 8.0, "a midsummer meadow noon had a sun of {}", meadow.sun_c);
    }

    #[test]
    fn night_in_the_desert_still_cools_a_body_off() {
        // The other half of the diurnal range: the sun is gone, the air
        // falls a long way, and a body that spent the noon burning is
        // heading back into comfort by dawn. Not *cold* in the hottest
        // tenth of the map, and that is measured rather than wished --
        // a dry desert at warmth 0.9 bottoms out near twenty before dawn,
        // which bare skin lifts to neutral. The cooler deserts are cold
        // at night; this one is only not hot.
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let at = (0.5, y, 0.5);
        let day = no_offset_day();
        let noon = in_the_desert(&world, &fires, at, day + 0.5, Weather::Clear);
        let before_dawn = in_the_desert(&world, &fires, at, day + 1.2, Weather::Clear);
        assert_eq!(before_dawn.sun_c, 0.0);
        assert!(
            before_dawn.temperature_c < noon.temperature_c - 15.0,
            "the desert went from {} at noon to only {} before dawn",
            noon.temperature_c,
            before_dawn.temperature_c
        );
        let heading = body::felt_under_sky(before_dawn.exposure(), 0.0, 0.0, 0.0);
        assert!(heading <= body::COMFORT_HIGH, "a desert dawn heads bare skin for {heading}");
    }

    #[test]
    fn a_wet_player_dries_faster_in_the_desert_sun_than_in_the_meadow() {
        let world = flat_world();
        let fires = Fires::new();
        let y = ground(&world);
        let at = (0.5, y, 0.5);
        let noon = no_offset_day() + 0.5;
        let meadow = sample(&world, &fires, at, noon, Weather::Clear);
        let desert = in_the_desert(&world, &fires, at, noon, Weather::Clear);
        assert_eq!(
            meadow.drying_per_second, DRYING_PER_SECOND,
            "a temperate noon changed how fast a player dries"
        );
        assert!(
            desert.drying_per_second > meadow.drying_per_second * 1.5,
            "the desert sun dries at {} against the meadow's {}",
            desert.drying_per_second,
            meadow.drying_per_second
        );
        // ...and never faster than the cap: a soaking is the minute after
        // a dip, not nothing.
        assert!(desert.drying_per_second <= DRYING_PER_SECOND * (1.0 + HEAT_DRYING) + 1e-6);
        let seconds_to_dry = |a: Ambient| {
            let mut wet = 1.0;
            let mut seconds = 0;
            while wet > 0.0 {
                wet = a.step_wetness(wet, 1.0);
                seconds += 1;
            }
            seconds
        };
        assert!(seconds_to_dry(desert) < seconds_to_dry(meadow));
    }
}
