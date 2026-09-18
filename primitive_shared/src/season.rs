//! The year: which season it is, and what that does to the air.
//!
//! ## What a season is here
//!
//! One number added to the temperature of every place in the world at
//! once, moving slowly, and one line on the map -- where rain becomes
//! snow -- moving with it. Nothing else. The generator does not know
//! the year exists: a taiga is a taiga in July and a meadow is a meadow
//! in January, and what changes is what falls on them and how cold a
//! night in them is. **Seasons are weather, not terrain**, and that is
//! what keeps a saved world the same world after a season has turned.
//!
//! The alternative -- seasons in the generator, with grass that browns
//! and lakes that freeze in the chunk data -- would mean either
//! regenerating loaded terrain on a schedule (and losing everything a
//! player built in it) or a second copy of every chunk. Neither is a
//! season; both are a save-file problem.
//!
//! ## Why the year is forty days
//!
//! **A season is ten days, because the player asked for seasons that are
//! lived through rather than glimpsed.** It used to be twelve days to the
//! year at a fifteen-minute day: a season was forty-five minutes, which
//! made winter a thing that happened during one sitting and passed before
//! a player had built anything *for* it. At the thirty-minute day a
//! season is now five hours of play -- a winter has to be stocked for in
//! earnest (a stack of fuel, a roof, wool, meat put by), and a player who
//! was not ready cannot simply wait it out in a hut, which is the
//! decision the season exists to force. Forty also divides into four
//! whole seasons, so "the first day of winter" is a day and not a
//! fraction of one.
//!
//! ## How time is told
//!
//! Every function here takes `world_time`: **the world's age in days,
//! with the hour of the day in the fraction.** `13.5` is noon on the
//! fourteenth day. That is the same number the server's clock hands
//! `climate::Ambient::of` for the hour, extended past one rather than
//! wrapped -- a calendar is a clock that does not wrap, and one number
//! that carries both means the season and the hour can never disagree
//! about what day it is. A `/time` jump moves both; a save carries
//! both.
//!
//! ## Where a world starts
//!
//! **A new world opens on the last day of spring**, when the seasonal
//! offset passes through zero. That is not sentiment: the generator's
//! climate was tuned so that the starting meadow is comfortable at
//! noon with nothing on (see `body::METABOLIC_LIFT_C`), and a world
//! that opened in winter would hand a player ten degrees of cold before
//! they had a way to make clothing. Opening at zero means the world a
//! player wakes up in is exactly the one the generator describes, with
//! a whole summer ahead in which to build before the first winter
//! arrives -- on the twenty-fourth day, about eleven and a half hours
//! of play in at the default thirty-minute day.

/// Days in a year. See the module note for why forty.
pub const YEAR_DAYS: f32 = 40.0;

/// Days in a season: a quarter of the year.
pub const SEASON_DAYS: f32 = YEAR_DAYS / 4.0;

/// How much warmer than the generator's climate the middle of summer
/// is, in the degrees `body` measures in.
pub const SUMMER_PEAK_C: f32 = 6.0;

/// ...and how much colder the middle of winter is.
///
/// Asymmetric on purpose. Ten degrees is what turns the starting
/// meadow's night from cold into dangerous and its noon from
/// comfortable into cold -- winter has to *reach* a temperate biome, or
/// it is a thing that happens to other people on the map. Six degrees
/// of summer is enough to thaw the edge of the taiga and make a desert
/// noon worse without making the rest of the world uncomfortable in
/// the season a player spends most of their first year in.
pub const WINTER_TROUGH_C: f32 = -10.0;

/// The day of the year the offset peaks: the middle of summer.
///
/// The year's own zero is the first day of spring, so the seasons fall
/// on whole days: spring 0..10, summer 10..20, autumn 20..30, winter
/// 30..40.
pub const MIDSUMMER_DAY_OF_YEAR: f32 = SEASON_DAYS * 1.5;

/// ...and the middle of winter, half a year on.
pub const MIDWINTER_DAY_OF_YEAR: f32 = MIDSUMMER_DAY_OF_YEAR + YEAR_DAYS / 2.0;

/// The day of the year a new world opens on. See the module note.
///
/// Seven, not zero: late spring. The offset is `-2 + 8 cos`, which
/// crosses zero where the cosine is a quarter -- 0.21 of a year, eight
/// and a half days, before midsummer on day fifteen -- and day seven is
/// the whole day nearest that. **Derived, not chosen**: when the year
/// was twelve days this was two, and leaving it at two after the year
/// grew opened every new world eleven degrees colder than the
/// generator's climate, in the middle of a spring that had not warmed
/// up yet. `a_new_world_opens_at_the_climate_the_generator_describes`
/// is the test that holds the two together. A world's own
/// day zero is therefore *not* the year's day zero, and everything
/// here converts through this constant rather than assuming they
/// agree.
pub const WORLD_OPENS_ON_DAY: f32 = 7.0;

/// How many degrees one unit of the generator's 0..1 climate scale
/// spans.
///
/// **Must equal `climate::HOTTEST_C - climate::COLDEST_C` on the
/// server**, which owns the mapping from the generator's field to
/// degrees; there is a test over there that says so. It is written out
/// here because the snow line has to move in the generator's units and
/// the offset is decided in degrees, and this crate cannot see the
/// server's constants.
pub const CLIMATE_SPAN_C: f32 = 56.0;

/// The four seasons, in the order the year runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    /// Which season a world time falls in.
    ///
    /// A nonsense time is the season a world opens in, for the reason a
    /// nonsense temperature is neutral: the number crosses a save file
    /// and a network message, and "it is spring" is a better failure
    /// than a panic in the tick loop.
    pub fn at(world_time: f32) -> Season {
        let day = day_of_year(world_time);
        match (day / SEASON_DAYS) as u32 {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        }
    }

    /// The season after this one.
    pub fn next(self) -> Season {
        match self {
            Season::Spring => Season::Summer,
            Season::Summer => Season::Autumn,
            Season::Autumn => Season::Winter,
            Season::Winter => Season::Spring,
        }
    }

    /// Short name, for the debug panel and `/time`. Not translated, on
    /// the same rule as `Weather::name`: it is a word that will end up
    /// in a bug report as well as on a screen.
    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        }
    }
}

/// Where in the year a world time falls, in `0..YEAR_DAYS`.
///
/// The year's zero is the first day of spring; the world's zero is
/// [`WORLD_OPENS_ON_DAY`] days later.
pub fn day_of_year(world_time: f32) -> f32 {
    if !world_time.is_finite() {
        return WORLD_OPENS_ON_DAY;
    }
    (world_time + WORLD_OPENS_ON_DAY).rem_euclid(YEAR_DAYS)
}

/// Which day of the world this is, counting from one, for people.
pub fn day_number(world_time: f32) -> u32 {
    if !world_time.is_finite() || world_time < 0.0 {
        return 1;
    }
    world_time.floor() as u32 + 1
}

/// How many degrees the season adds to every temperature in the world.
///
/// A cosine over the year rather than four steps, so the first day of
/// winter is a little colder than the last day of autumn rather than a
/// wall -- a player has to be able to feel it *coming*. Peaks at
/// [`SUMMER_PEAK_C`] in the middle of summer and bottoms at
/// [`WINTER_TROUGH_C`] in the middle of winter, and the two ends of the
/// range are the whole of the tuning: the midpoint and the amplitude
/// are derived from them rather than chosen.
pub fn ambient_offset_c(world_time: f32) -> f32 {
    if !world_time.is_finite() {
        return 0.0;
    }
    let mid = (SUMMER_PEAK_C + WINTER_TROUGH_C) * 0.5;
    let amplitude = (SUMMER_PEAK_C - WINTER_TROUGH_C) * 0.5;
    let phase = (day_of_year(world_time) - MIDSUMMER_DAY_OF_YEAR) / YEAR_DAYS;
    mid + amplitude * (std::f32::consts::TAU * phase).cos()
}

/// How much of the year's swing reaches a latitude, as a share of the
/// temperate swing the offsets above were tuned on.
///
/// **Because a season is the tilt of the planet, and the tilt does little
/// on the equator.** The summer peak and the winter trough were set for the
/// one latitude the world used to have -- a winter that turns a meadow's
/// night dangerous. Since the globe went to real scale a world can be laid
/// in the tropics (`worldgen::Zone`), where ten degrees of January would put
/// frost on a coconut palm and snow in the surf. On the Earth the year's
/// range is a degree or two on the equator, the whole of a temperate winter
/// by the middle latitudes, and more again in the north. So the offset is
/// scaled: a fifth of it within ten degrees of the equator, rising to all of
/// it at forty-five, and a quarter as much again by sixty-five.
///
/// **Exactly one at forty-five and on the test world** (`None`), so every
/// temperature, snow line and frozen lake the seasons were measured against
/// in the temperate world is what it was, to the digit.
///
/// Rejected: moving the sun as well. The noon height of the sun and the
/// length of the day do depend on latitude and season on the Earth, and the
/// sky here reads neither -- it is one path for every world. That path is a
/// picture the shadows are cast from; a temperature is a mechanic, and the
/// swing is where a wrong season costs a player something.
pub fn seasonal_swing(latitude_degrees: Option<f32>) -> f32 {
    let Some(degrees) = latitude_degrees.filter(|d| d.is_finite()) else {
        return 1.0;
    };
    let away = degrees.abs();
    if away <= 10.0 {
        0.2
    } else if away <= 45.0 {
        0.2 + 0.8 * (away - 10.0) / 35.0
    } else {
        (1.0 + 0.25 * (away - 45.0) / 20.0).min(1.25)
    }
}

/// [`falls_as_snow`], with the season's shift scaled to a latitude's share
/// of the year ([`seasonal_swing`]).
pub fn falls_as_snow_with_swing(climate_temperature: f32, world_time: f32, swing: f32) -> bool {
    climate_temperature < crate::weather::SNOW_TEMPERATURE + snow_line_shift(world_time) * swing
}

/// How far the snow line moves, in the generator's 0..1 climate units.
///
/// Positive means *more* of the world is under snow. It is the ambient
/// offset turned into the other scale and negated: a colder season is
/// a higher line, and a column that was rain at the equinox is snow in
/// midwinter if it sits within the shift of the line.
pub fn snow_line_shift(world_time: f32) -> f32 {
    -ambient_offset_c(world_time) / CLIMATE_SPAN_C
}

/// Does precipitation fall as snow here, at this time of year?
///
/// `weather::falls_as_snow` with the season on it. The threshold is the
/// generator's own freezing line, which is where snow *lies*; in
/// winter the line where snow *falls* is above it, and that is the
/// point -- snow on green ground is what winter looks like, and it is
/// gone with the season because the ground under it was never changed.
/// In summer the same shift runs the other way and rain falls on the
/// edge of the taiga.
pub fn falls_as_snow(climate_temperature: f32, world_time: f32) -> bool {
    climate_temperature < crate::weather::SNOW_TEMPERATURE + snow_line_shift(world_time)
}

/// Does still water freeze over here, at this time of year?
///
/// The same line as the snow, deliberately: the generator freezes water
/// exactly where the bank beside it is white (`worldgen::FREEZING`, one
/// threshold for both), and a seasonal rule that split them would put
/// open water in a snowfield or ice in a green one. Whoever forms and
/// thaws ice at run time should ask this and nothing else.
pub fn water_freezes(climate_temperature: f32, world_time: f32) -> bool {
    falls_as_snow(climate_temperature, world_time)
}

/// [`water_freezes`], with the season's shift scaled to a latitude's share
/// of the year ([`seasonal_swing`]).
///
/// **This is the one a server should ask**, and the plain form above is it
/// at forty-five degrees. A tropical winter is a cool spell rather than ten
/// degrees of frost, and a rule that ignored the swing would put a lid on a
/// lagoon in January.
pub fn water_freezes_with_swing(climate_temperature: f32, world_time: f32, swing: f32) -> bool {
    falls_as_snow_with_swing(climate_temperature, world_time, swing)
}

/// The world time at the height of summer -- the warmest moment of any
/// year, and the one [`water_never_thaws`] asks about.
///
/// The year's zero and the world's are [`WORLD_OPENS_ON_DAY`] apart; this
/// is the conversion done once rather than at every caller, because a
/// caller that got it wrong would be asking about the wrong week and
/// nothing would look obviously broken.
pub const MIDSUMMER_WORLD_TIME: f32 = MIDSUMMER_DAY_OF_YEAR - WORLD_OPENS_ON_DAY;

/// Is this place cold enough that its still water is frozen even at the
/// height of summer?
///
/// **Permafrost, and it is what the generator is allowed to freeze.** The
/// generator works from the climate field alone -- a place's average, with
/// no year in it -- so the ice it lays is ice for ever: the chunk is
/// regenerated from the seed every time it is loaded, and nothing about a
/// season reaches it (see this module's note on why seasons are weather and
/// never terrain). Freezing everything under the year's *mean* freezing
/// line therefore froze the whole north permanently, in a country whose
/// thermometer reads twenty degrees at noon in July -- which is the bug
/// this exists to answer, in the player's words: "на севере вся вода
/// заледеневшая хотя температура 20 градусов".
///
/// So the world lays ice only where it would be honest all year, and the
/// *season's* ice -- the bay that freezes in November and opens in April --
/// is made and unmade at run time by the server, which has a clock. See
/// `primitive_server::logic::water::Frost`.
///
/// Rejected: leaving the generator's line where it was and letting the
/// server thaw. Every chunk would arrive with a lid on it and lose it a few
/// seconds later, so a player flying over the north in summer would watch
/// the sea turn from white to blue chunk by chunk, and every one of those
/// thaws is a block change broadcast to everybody.
pub fn water_never_thaws(climate_temperature: f32, swing: f32) -> bool {
    water_freezes_with_swing(climate_temperature, MIDSUMMER_WORLD_TIME, swing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_day_of(day_of_year: f32) -> f32 {
        day_of_year - WORLD_OPENS_ON_DAY
    }

    #[test]
    fn the_year_turns_through_four_seasons_and_comes_back() {
        // Walk a world from its first day through a whole year, a
        // quarter of a day at a time, and the seasons have to arrive in
        // order, each exactly once, and the year has to end where it
        // began.
        let first = Season::at(0.0);
        let mut seen = vec![first];
        let mut t = 0.0;
        while t < YEAR_DAYS {
            let now = Season::at(t);
            if now != *seen.last().unwrap() {
                assert_eq!(now, seen.last().unwrap().next(), "the seasons ran out of order at day {t}");
                seen.push(now);
            }
            t += 0.25;
        }
        assert_eq!(seen.len(), 5, "a year saw {} seasons: {seen:?}", seen.len() - 1);
        assert_eq!(*seen.last().unwrap(), first, "the year did not come back round");
        assert_eq!(Season::at(YEAR_DAYS), first);
        assert_eq!(Season::at(YEAR_DAYS * 7.0 + 0.3), Season::at(0.3));
    }

    #[test]
    fn a_new_world_opens_at_the_climate_the_generator_describes() {
        // The claim in the module note, as a number: the generator's
        // climate is what a fresh player meets, and summer is three days
        // away.
        assert_eq!(Season::at(0.0), Season::Spring);
        assert_eq!(Season::at(2.9), Season::Spring);
        assert_eq!(Season::at(3.0), Season::Summer);
        // Within a degree or so at the moment the world opens and about
        // two by its first noon -- less than the difference between a
        // meadow and the wood beside it, and on the warm side of zero.
        let opening = ambient_offset_c(0.0);
        assert!(opening.abs() < 1.0, "a new world opens {opening} degrees off the generator");
        let first_noon = ambient_offset_c(0.5);
        assert!(
            (0.0..=2.5).contains(&first_noon),
            "the first noon is {first_noon} degrees off the generator"
        );
        // ...and it is getting warmer, not colder.
        assert!(ambient_offset_c(1.5) > first_noon);
    }

    #[test]
    fn winter_is_colder_than_summer_by_the_offset_at_their_peaks() {
        // At the peaks themselves, not at "noon of the peak day": with a
        // forty-day year the peak falls on a midnight, and a noon half a
        // day away from it is a tenth of a degree short of the number
        // this test is about.
        let summer_noon = world_day_of(MIDSUMMER_DAY_OF_YEAR);
        let winter_noon = world_day_of(MIDWINTER_DAY_OF_YEAR);
        assert_eq!(Season::at(summer_noon), Season::Summer);
        assert_eq!(Season::at(winter_noon), Season::Winter);
        let summer = ambient_offset_c(summer_noon);
        let winter = ambient_offset_c(winter_noon);
        assert!((summer - SUMMER_PEAK_C).abs() < 0.01, "midsummer noon is {summer}");
        assert!((winter - WINTER_TROUGH_C).abs() < 0.01, "midwinter noon is {winter}");
        assert!(
            (summer - winter - (SUMMER_PEAK_C - WINTER_TROUGH_C)).abs() < 0.02,
            "summer and winter are {} apart",
            summer - winter
        );
    }

    #[test]
    fn the_seasons_arrive_as_a_slope_rather_than_a_wall() {
        // The first day of winter must not be a step: a player has to
        // feel it coming. The steepest the cosine gets, at the
        // equinoxes, is eight degrees times two pi over forty days --
        // about a third of a degree every six hours of game time. A wall
        // would be ten degrees in one tick.
        let mut t = 0.0;
        let mut worst = 0.0f32;
        while t < YEAR_DAYS {
            let step = (ambient_offset_c(t + 0.25) - ambient_offset_c(t)).abs();
            worst = worst.max(step);
            t += 0.25;
        }
        assert!(worst < 1.2, "the offset jumped {worst} degrees in a quarter of a day");
        assert!(worst > 0.25, "the offset barely moves: {worst} a quarter-day at its steepest");
    }

    #[test]
    fn a_temperate_rain_falls_as_snow_in_winter_and_as_rain_in_summer() {
        // A cool temperate column: ten degrees at noon on the server's
        // scale, comfortably above the freezing line at the equinox.
        let cool_meadow = crate::weather::SNOW_TEMPERATURE + 0.11;
        let summer = world_day_of(MIDSUMMER_DAY_OF_YEAR);
        let winter = world_day_of(MIDWINTER_DAY_OF_YEAR);
        assert!(!falls_as_snow(cool_meadow, summer), "it snowed on a meadow in midsummer");
        assert!(!falls_as_snow(cool_meadow, 0.5), "it snowed on a meadow the day the world opened");
        assert!(falls_as_snow(cool_meadow, winter), "midwinter rained on a cool meadow");

        // ...and the same shift the other way: the edge of the taiga,
        // just inside the line, thaws in summer.
        let taiga_edge = crate::weather::SNOW_TEMPERATURE - 0.05;
        assert!(falls_as_snow(taiga_edge, 0.5));
        assert!(!falls_as_snow(taiga_edge, summer), "summer did not thaw the taiga's edge");
        // The deep cold does not: a snowfield in July is still one.
        assert!(falls_as_snow(0.0, summer));

        // Ice follows the snow, not a line of its own.
        assert_eq!(water_freezes(cool_meadow, winter), falls_as_snow(cool_meadow, winter));
        assert_eq!(water_freezes(taiga_edge, summer), falls_as_snow(taiga_edge, summer));
    }

    #[test]
    fn the_tropics_barely_have_a_winter_and_the_temperate_zone_keeps_the_one_it_was_tuned_for() {
        // The temperate world and the test world are what every seasonal
        // number was measured in, and they must not move.
        assert_eq!(seasonal_swing(Some(45.0)), 1.0);
        assert_eq!(seasonal_swing(Some(-45.0)), 1.0);
        assert_eq!(seasonal_swing(None), 1.0);
        assert_eq!(seasonal_swing(Some(f32::NAN)), 1.0);
        // A tropical midwinter is under two degrees colder than the
        // generator's climate, where a temperate one is ten.
        let winter = world_day_of(MIDWINTER_DAY_OF_YEAR) + 0.5;
        let tropical_winter = ambient_offset_c(winter) * seasonal_swing(Some(8.0));
        assert!(tropical_winter > -2.1, "a tropical midwinter is {tropical_winter} degrees");
        // ...so a warm coast's rain stays rain all year, while the same
        // column at forty-five would see snow in midwinter.
        let cool_meadow = crate::weather::SNOW_TEMPERATURE + 0.11;
        assert!(!falls_as_snow_with_swing(cool_meadow, winter, seasonal_swing(Some(8.0))));
        assert_eq!(
            falls_as_snow_with_swing(cool_meadow, winter, seasonal_swing(Some(45.0))),
            falls_as_snow(cool_meadow, winter)
        );
        // The swing grows away from the equator and never runs away.
        let mut before = seasonal_swing(Some(0.0));
        for degrees in 1..=90 {
            let here = seasonal_swing(Some(degrees as f32));
            assert!(here >= before && here <= 1.25, "{degrees} degrees swings {here} after {before}");
            before = here;
        }
    }

    /// **Permafrost is what the generator may freeze, and nothing else.**
    /// The two lines used to be one number -- the yearly mean -- and that is
    /// why a northern lake was frozen solid in a country whose thermometer
    /// read twenty degrees at noon in July. [`water_never_thaws`] is the
    /// colder of the two, by exactly the summer's share of the year.
    #[test]
    fn permafrost_is_colder_than_freezing_by_a_whole_summer() {
        let summer = world_day_of(MIDSUMMER_DAY_OF_YEAR);
        // The two lines, in the generator's 0..1 units, at forty-five
        // degrees where the swing is one.
        let freezing = crate::weather::SNOW_TEMPERATURE;
        let permafrost = freezing + snow_line_shift(summer);
        assert!(
            permafrost < freezing - 0.10,
            "permafrost is only {:.3} colder than freezing",
            freezing - permafrost
        );
        // A column between the two lines is the whole mechanic: frozen in
        // winter, open in summer, and never the generator's business.
        let between = (freezing + permafrost) * 0.5;
        assert!(!water_never_thaws(between, 1.0), "a seasonal bay was called permafrost");
        assert!(water_freezes(between, world_day_of(MIDWINTER_DAY_OF_YEAR)), "it did not close in winter");
        assert!(!water_freezes(between, summer), "it did not open in summer");
        // ...and below the permafrost line nothing opens it, all year.
        let polar = permafrost - 0.05;
        assert!(water_never_thaws(polar, 1.0));
        for tenth in 0..120 {
            let t = tenth as f32 * 0.1;
            assert!(water_freezes(polar, t), "permafrost opened on day {t}");
        }
    }

    /// The swing reaches the ice as well as the air: a tropical lagoon has a
    /// winter of about two degrees, and no amount of it puts a lid on the
    /// water that a temperate pond of the same climate would grow.
    #[test]
    fn a_tropical_winter_freezes_nothing_a_temperate_one_would() {
        let winter = world_day_of(MIDWINTER_DAY_OF_YEAR);
        let cool = crate::weather::SNOW_TEMPERATURE + 0.11;
        assert!(water_freezes_with_swing(cool, winter, seasonal_swing(Some(45.0))));
        assert!(!water_freezes_with_swing(cool, winter, seasonal_swing(Some(8.0))));
        // ...and the plain form is the temperate one, to the digit, so
        // everything measured at forty-five degrees still holds.
        assert_eq!(
            water_freezes_with_swing(cool, winter, seasonal_swing(Some(45.0))),
            water_freezes(cool, winter)
        );
    }

    #[test]
    fn a_nonsense_time_is_spring_with_no_offset() {
        // The number crosses a save and a socket.
        assert_eq!(Season::at(f32::NAN), Season::at(0.0));
        assert_eq!(ambient_offset_c(f32::NAN), 0.0);
        assert_eq!(ambient_offset_c(f32::INFINITY), 0.0);
        assert_eq!(snow_line_shift(f32::NAN), 0.0);
        assert_eq!(day_number(f32::NAN), 1);
        assert_eq!(day_number(-3.0), 1);
        assert_eq!(day_number(0.99), 1);
        assert_eq!(day_number(13.5), 14);
    }

    #[test]
    fn a_time_before_the_world_began_is_still_a_season() {
        // `/time` can wind the clock back; `rem_euclid` rather than `%`
        // is what keeps a negative day from being a fifth season.
        for t in [-0.5f32, -3.0, -YEAR_DAYS + 0.01, -YEAR_DAYS, -100.25] {
            let day = day_of_year(t);
            assert!((0.0..YEAR_DAYS).contains(&day), "day {t} fell at {day} in the year");
            assert!(ambient_offset_c(t).is_finite());
        }
        assert_eq!(Season::at(-YEAR_DAYS), Season::at(0.0));
    }
}
