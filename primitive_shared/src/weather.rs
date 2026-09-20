//! Weather: what is falling out of the sky, and how hard.
//!
//! ## Why the server owns it
//!
//! For the same reason it owns the clock. Two players standing in the
//! same field have to be standing in the same rain -- one of them
//! sheltering under a tree while the other sees sunshine is not a
//! graphical difference, it is two different worlds, and the moment rain
//! puts a fire out it stops being cosmetic at all.
//!
//! So the server holds one `Weather` for the whole world and broadcasts
//! it; the client draws it and derives everything visible from it. The
//! wire cost is a message every few minutes.
//!
//! ## Why *one* weather for the whole world
//!
//! A per-region system is the obvious ambition and it is the wrong first
//! step. It needs a field over the map, interpolation at the seams, and
//! a rule for what a player standing on a boundary sees -- and every one
//! of those is machinery rather than weather. What a player actually
//! wants from rain is that it arrives, that it changes what the world
//! looks like and what they can do in it, and that it stops. One value
//! delivers all three.
//!
//! What is *not* global is what falls: the same storm is rain over a
//! meadow and snow over a mountain, decided per cell from the climate
//! the world generator already computes. That is the cheap half of a
//! regional system and it is the half that shows.
//!
//! ## What weather does, besides look like something
//!
//! * **It puts fires out.** A campfire under open sky goes out in the
//!   rain, which is the whole reason to build a roof over one.
//! * **It darkens the day.** Storm light is what makes a cave mouth
//!   look inviting at noon.
//! * **It fills the world with sound and motion** -- which, in a game
//!   whose sky is otherwise a gradient and a cloud layer, is most of
//!   what makes a world feel weathered at all.

use serde::{Deserialize, Serialize};

/// What the sky is doing.
///
/// Three states rather than a continuum, because a player has to be able
/// to *say* what the weather is -- "it is raining" is a fact you act on,
/// and a precipitation coefficient of 0.31 is not. Intensity rides
/// alongside for the parts that genuinely are continuous (how heavy the
/// fall is drawn, how dark it gets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Weather {
    /// Nothing falling. The default, and where a fresh world starts:
    /// beginning a player's first session in a downpour would teach them
    /// the rain before it taught them the game.
    #[default]
    Clear,
    /// Steady precipitation. Rain in the warm, snow in the cold -- see
    /// `falls_as_snow`.
    Rain,
    /// The same, heavier and darker.
    Storm,
}

impl Weather {
    /// Is anything coming down at all?
    #[inline]
    pub fn is_wet(self) -> bool {
        !matches!(self, Weather::Clear)
    }

    /// How heavily, 0..1. Drives how much of it the client draws and how
    /// far down the daylight goes.
    #[inline]
    pub fn intensity(self) -> f32 {
        match self {
            Weather::Clear => 0.0,
            Weather::Rain => 0.55,
            Weather::Storm => 1.0,
        }
    }

    /// What fraction of the daylight this leaves.
    ///
    /// Never to nothing: a storm at noon is gloomy, not night. Two
    /// thirds under rain and a bit under half under a storm is enough
    /// that a torch-lit doorway reads as warm from outside, which is the
    /// point of darkening it at all.
    #[inline]
    pub fn daylight_factor(self) -> f32 {
        daylight_under(self.intensity())
    }

    /// Short name, for `/weather` and the debug panel. Not translated:
    /// it is a command argument as well as a label, and a command that
    /// only works in one language is worse than an English word.
    pub fn name(self) -> &'static str {
        match self {
            Weather::Clear => "clear",
            Weather::Rain => "rain",
            Weather::Storm => "storm",
        }
    }

    /// The reverse, for the command. Unknown names are `None` rather
    /// than clear, so a typo says so instead of silently stopping the
    /// rain.
    pub fn parse(name: &str) -> Option<Weather> {
        match name.trim().to_ascii_lowercase().as_str() {
            "clear" | "sun" | "sunny" => Some(Weather::Clear),
            "rain" => Some(Weather::Rain),
            "storm" | "thunder" => Some(Weather::Storm),
            _ => None,
        }
    }
}

/// What fraction of the daylight survives a sky this overcast.
///
/// The same curve `Weather::daylight_factor` reports, taking the number
/// rather than the enum -- because the client does not darken the world
/// in one step when the message lands. It eases toward the new weather
/// over a few seconds (`engine::sky::Sky::overcast`) and needs the
/// figure for every value in between, and a second copy of `0.55`
/// sitting in the renderer is how the sky and the light end up
/// disagreeing about how dark a storm is.
///
/// Never to nothing: a storm at noon is gloomy, not night. See
/// `daylight_factor`.
#[inline]
pub fn daylight_under(overcast: f32) -> f32 {
    1.0 - 0.55 * overcast.clamp(0.0, 1.0)
}

/// Does precipitation arrive here as snow rather than rain?
///
/// The one part of the weather that is local, and it costs one number:
/// the temperature the world generator already computed for the column,
/// which is what decides whether the ground there is turf or snowfield
/// in the first place. So a storm that whitens the mountain and soaks
/// the valley is the same storm, and nothing had to be interpolated.
///
/// The threshold matches the generator's own idea of where snow lies, so
/// falling snow lands on ground that is already white rather than in a
/// band of green above the snowline.
#[inline]
pub fn falls_as_snow(temperature: f32) -> bool {
    temperature < SNOW_TEMPERATURE
}

/// Where the cold begins, on the generator's 0..1 temperature scale.
///
/// **Taken from the generator rather than chosen here.** It used to be a
/// hand-picked `0.25` against the generator's own freezing line of
/// `0.29`, and the gap was not a rounding difference: a column between
/// the two is tundra, so the ground is snow, and the sky rained on it.
/// Swept over a real world, better than a quarter of every white column
/// in it was in that band -- rain falling on a snowfield, which is
/// exactly the "band of green above the snowline" this threshold was
/// written to avoid. See `snow_falls_on_everything_the_generator_paints_white`.
pub const SNOW_TEMPERATURE: f32 = crate::worldgen::CLIMATE_FREEZING;

/// What is actually coming down *here*.
///
/// ## Why the sky's three states are not enough
///
/// `Weather` is what the world's sky is doing; this is what a player
/// standing in one column sees fall out of it. They are different
/// questions and the gap between them is where "it rained in the desert"
/// lived: one storm over a world whose generator had spent four noise
/// fields deciding that this corner of it is sand.
///
/// The rule is the *generator's own*, asked in the generator's order
/// (`land_biome`): cold first, so a cold desert -- which is tundra, and
/// which the generator paints white -- gets snow like every other cold
/// place; then hot and dry, which is the one country where a storm
/// brings no water; then rain everywhere else. Nothing new is sampled
/// for it. The temperature and the humidity are the two numbers the
/// world already carries for every column, and the client has had them
/// since it drew its first tinted leaf.
///
/// ## Why dust and not "nothing"
///
/// A desert under a storm that simply had no weather in it would be a
/// desert where the sky does nothing and the word "storm" is a lie the
/// chat line tells. A dust storm is the same front arriving with what
/// that country has to give: it darkens, it blows, it is unpleasant to
/// be out in -- and it does not put a fire out, which is the whole of
/// why a fire in a desert is a different decision from a fire in a wood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Precipitation {
    /// A dry sky, or a sky whose weather has not reached this column.
    #[default]
    None,
    Rain,
    Snow,
    /// Sand and grit, in hot dry country. Wets nothing and quenches
    /// nothing.
    Dust,
}

impl Precipitation {
    /// What falls on a column of this climate under this sky.
    ///
    /// `snowing` is the season's answer rather than a temperature
    /// compared here, because the snow line moves with the year
    /// (`season::falls_as_snow_with_swing`) and with the latitude, and a
    /// second copy of that arithmetic is how the sky and the ground come
    /// to disagree about where the snow lies -- which is the mistake
    /// `SNOW_TEMPERATURE` is a monument to.
    pub fn of(weather: Weather, snowing: bool, temperature: f32, humidity: f32) -> Precipitation {
        if !weather.is_wet() {
            return Precipitation::None;
        }
        if snowing {
            return Precipitation::Snow;
        }
        if temperature >= crate::worldgen::CLIMATE_HOT && humidity < crate::worldgen::CLIMATE_DRY {
            return Precipitation::Dust;
        }
        Precipitation::Rain
    }

    /// Does this wet what it lands on -- the ground, a fire, a board left
    /// out?
    ///
    /// The one question the rest of the server asks. Dust does not, which
    /// is why a lightning fire in a desert burns until it runs out of
    /// fuel while the same fire in a marsh is out in a minute.
    #[inline]
    pub fn wets(self) -> bool {
        matches!(self, Precipitation::Rain | Precipitation::Snow)
    }

    /// Short name, for the debug panel and the chat line. Not translated,
    /// for `Weather::name`'s reason.
    pub fn name(self) -> &'static str {
        match self {
            Precipitation::None => "none",
            Precipitation::Rain => "rain",
            Precipitation::Snow => "snow",
            Precipitation::Dust => "dust",
        }
    }
}

/// How hard it comes down in country of this humidity, 0..1.
///
/// **The monsoon, in one multiply.** The wettest country there is --
/// marsh, bog, closed forest -- takes a quarter more than the enum says,
/// and the driest takes little better than half; between the generator's
/// two lines it is a straight run. So walking from a steppe into a
/// rainforest in one spell of rain is a change a player can see and
/// hear, and neither end of it needed a weather system of its own.
///
/// Rejected: a humidity term folded into `Weather::intensity`. That
/// figure is the *sky's*, and the fires, the sound bed and the darkening
/// all read it; making it local would mean a storm that is darker in a
/// marsh than over the sand dunes half a kilometre away, which is one
/// sky too many.
pub fn local_intensity(weather: Weather, humidity: f32) -> f32 {
    let dry = crate::worldgen::CLIMATE_DRY;
    let wet = crate::worldgen::CLIMATE_WET;
    let t = ((humidity - dry) / (wet - dry)).clamp(0.0, 1.0);
    weather.intensity() * (DRIZZLE_SHARE + (MONSOON_SHARE - DRIZZLE_SHARE) * t)
}

/// What the driest country takes of a shower.
pub const DRIZZLE_SHARE: f32 = 0.55;
/// ...and what the wettest takes.
pub const MONSOON_SHARE: f32 = 1.25;

/// How thick the dawn mist over sodden ground is, 0..1.
///
/// **A bog at first light, and nowhere else.** Mist is the one weather
/// that belongs to a *place* rather than to the sky: it wants still air,
/// ground with water in it and the hour the ground is colder than the
/// air, which is the hour before and after sunrise. So it is a function
/// of the humidity and the clock and takes nothing from `Weather` at all
/// -- a mist in a downpour would be a second sky over the first.
///
/// It draws rather than decides: the client tightens its fog with it.
/// Nothing mechanical hangs off mist, deliberately -- a fog that halved
/// what a player could see *and* did something to them would be a
/// punishment for logging in at the wrong hour.
pub fn dawn_mist(humidity: f32, time_of_day: f32) -> f32 {
    if humidity < crate::worldgen::CLIMATE_WET {
        return 0.0;
    }
    // How far into the wet end of the scale: a marsh is thicker than a
    // damp wood, and the far end of the scale is a bog.
    let wetness = ((humidity - crate::worldgen::CLIMATE_WET) / (1.0 - crate::worldgen::CLIMATE_WET)).clamp(0.0, 1.0);
    // Sunrise is 0.25 of the day (`commands::parse_time`). Round the
    // clock rather than along it, so a world whose hour wrapped past
    // midnight does not get an hour of mist at dusk.
    let from_dawn = (time_of_day - DAWN).abs().min(1.0 - (time_of_day - DAWN).abs());
    if from_dawn >= MIST_HOURS {
        return 0.0;
    }
    let t = 1.0 - from_dawn / MIST_HOURS;
    wetness * t * t * (3.0 - 2.0 * t)
}

/// When the sun comes up, as a fraction of the day. The same figure
/// `/time dawn` sets.
pub const DAWN: f32 = 0.25;

/// How much of a day either side of dawn the mist lies over, as a
/// fraction of it: a little over an hour and a half of a twenty-four
/// hour day, which is a thing a player who sleeps through the night
/// meets when they step outside and a thing they miss if they lie in.
pub const MIST_HOURS: f32 = 0.07;

/// How long a spell of weather lasts, in seconds, before the server
/// rolls again.
///
/// The two ends of the range and nothing between them, because the only
/// thing that matters about the length of a shower is that it is not
/// predictable and not interminable. Five minutes is long enough to
/// change what you do -- to go inside, or to give up on the fire -- and
/// twenty is short enough that nobody waits it out at a keyboard.
pub const SPELL_SECONDS: std::ops::Range<f32> = 300.0..1200.0;

/// The chance that a spell of clear weather is followed by weather.
///
/// Two in five, so most of a session is dry and rain is an event. A
/// storm is a third of what falls, so it is a thing that happens rather
/// than a thing that is always happening.
pub const CHANCE_OF_WEATHER: f32 = 0.4;
/// ...and of the wet spells, how many are storms rather than rain.
pub const CHANCE_OF_STORM: f32 = 0.33;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_is_the_default_and_the_only_dry_one() {
        assert_eq!(Weather::default(), Weather::Clear);
        assert!(!Weather::Clear.is_wet());
        assert!(Weather::Rain.is_wet());
        assert!(Weather::Storm.is_wet());
    }

    #[test]
    fn a_storm_is_heavier_and_darker_than_rain() {
        assert!(Weather::Storm.intensity() > Weather::Rain.intensity());
        assert!(Weather::Storm.daylight_factor() < Weather::Rain.daylight_factor());
        assert_eq!(Weather::Clear.daylight_factor(), 1.0);
    }

    #[test]
    fn even_a_storm_is_not_night() {
        // A storm that took the daylight to nothing would be a night
        // that arrives at noon, and a player caught in one would have no
        // way to tell it from the sun going down.
        assert!(
            Weather::Storm.daylight_factor() > 0.35,
            "a storm leaves {} of the daylight",
            Weather::Storm.daylight_factor()
        );
    }

    #[test]
    fn the_eased_curve_and_the_stepped_one_are_the_same_curve() {
        // The client darkens the world over a few seconds rather than in
        // the tick the message lands, so it needs this figure for every
        // value between the two states -- and a second copy of the
        // coefficient over there is how the sky and the light come to
        // disagree about how dark a storm is.
        for weather in [Weather::Clear, Weather::Rain, Weather::Storm] {
            assert_eq!(weather.daylight_factor(), daylight_under(weather.intensity()));
        }
        // Monotonic between them, and bounded at both ends.
        assert!(daylight_under(0.25) > daylight_under(0.75));
        assert_eq!(daylight_under(-1.0), daylight_under(0.0));
        assert_eq!(daylight_under(2.0), daylight_under(1.0));
    }

    #[test]
    fn names_survive_a_round_trip() {
        for weather in [Weather::Clear, Weather::Rain, Weather::Storm] {
            assert_eq!(Weather::parse(weather.name()), Some(weather));
        }
        // A typo is a refusal, not silently a clear sky.
        assert_eq!(Weather::parse("rian"), None);
        assert_eq!(Weather::parse(""), None);
        // ...and the words a player is likely to try instead.
        assert_eq!(Weather::parse("SUNNY"), Some(Weather::Clear));
        assert_eq!(Weather::parse(" thunder "), Some(Weather::Storm));
    }

    #[test]
    fn snow_falls_where_the_ground_is_already_white() {
        assert!(falls_as_snow(0.0));
        assert!(falls_as_snow(SNOW_TEMPERATURE - 0.01));
        assert!(!falls_as_snow(SNOW_TEMPERATURE));
        assert!(!falls_as_snow(1.0));
    }

    /// A desert column, as the generator would report it: hot and dry.
    const DESERT: (f32, f32) = (0.9, 0.2);
    /// A marsh: warm and as wet as country gets.
    const MARSH: (f32, f32) = (0.6, 0.9);
    /// A meadow, in the middle of everything.
    const MEADOW: (f32, f32) = (0.5, 0.5);

    #[test]
    fn it_does_not_rain_in_the_desert() {
        // The whole of the complaint. One storm over a world whose
        // generator spent four noise fields deciding this corner of it
        // is sand.
        assert_eq!(
            Precipitation::of(Weather::Storm, false, DESERT.0, DESERT.1),
            Precipitation::Dust
        );
        assert_eq!(
            Precipitation::of(Weather::Rain, false, DESERT.0, DESERT.1),
            Precipitation::Dust
        );
        assert!(!Precipitation::Dust.wets(), "a dust storm put a fire out");
    }

    #[test]
    fn a_cold_desert_is_tundra_and_gets_snow_like_everywhere_else_cold() {
        // The order the questions are asked in, stated as the thing it
        // is for: dry and cold is not desert, it is tundra, and the
        // generator paints it white. Asking "is it dry" first would put
        // a dust storm on a snowfield.
        assert_eq!(
            Precipitation::of(Weather::Storm, true, 0.05, 0.2),
            Precipitation::Snow
        );
    }

    #[test]
    fn rain_falls_where_the_country_is_neither_hot_nor_dry() {
        for (t, h) in [MEADOW, MARSH, (0.9, 0.9)] {
            assert_eq!(Precipitation::of(Weather::Rain, false, t, h), Precipitation::Rain);
        }
        // ...and a clear sky drops nothing anywhere.
        assert_eq!(Precipitation::of(Weather::Clear, false, MARSH.0, MARSH.1), Precipitation::None);
        assert_eq!(Precipitation::of(Weather::Clear, true, 0.0, 0.0), Precipitation::None);
    }

    #[test]
    fn dust_falls_on_the_sand_the_generator_drew_and_rain_on_the_woods() {
        // **Swept over a real world**, the way `SNOW_TEMPERATURE` was --
        // and for the lesson it records. The thresholds here are derived
        // from the classifier's own, so the two cannot drift; this is
        // the test that says they have not, because a column's *biome*
        // is decided by the surface temperature and what falls on it by
        // `climate_at`, which is the same field with the lapse rate
        // applied, and "near enough" is not a thing a test can assume.
        use crate::worldgen::{Biome, WorldGen, Zone};
        let (mut desert, mut dusty) = (0, 0);
        let (mut wood, mut wet) = (0, 0);
        // **Two worlds, because one has no desert in it.** A world is
        // laid at a latitude (`Zone`), and the temperate one the game
        // opens in has no sand anywhere: sweeping it for deserts finds
        // what sweeping England finds. So the dust is checked where the
        // dust is -- a world in the dry belt -- and the rain where the
        // rain is.
        for (zone, seed) in [(Zone::DryBelt, 4242), (Zone::Temperate, 4242)] {
            let gen = WorldGen::with_zone(seed, crate::worldgen::Preset::Normal, zone);
            for x in (-2400..2400).step_by(29) {
                for z in (-2400..2400).step_by(29) {
                    let biome = gen.biome_at(x, z);
                    let y = gen.height_at(x, z);
                    let (temperature, humidity) = gen.climate_at(x, y, z);
                    let falling = Precipitation::of(Weather::Storm, false, temperature, humidity);
                    match biome {
                        Biome::Desert => {
                            desert += 1;
                            dusty += usize::from(falling == Precipitation::Dust);
                        }
                        Biome::Forest | Biome::BirchForest | Biome::Swamp => {
                            wood += 1;
                            wet += usize::from(falling == Precipitation::Rain);
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(desert > 20 && wood > 20, "the sweep found {desert} desert and {wood} wooded columns");
        // **Nearly every column rather than every one**, and the gap is
        // the honest part of this test. The classifier reads the climate
        // at the *surface* and this reads `climate_at` at the ground
        // under it, with the lapse rate applied; along the edge where a
        // wood gives way to open country the two disagree by a
        // hundredth, and a hundredth is a column either way. What
        // matters is that the middle of a desert is never rained on and
        // the middle of a wood is never dusted, which is what these two
        // shares say.
        assert!(
            dusty * 10 >= desert * 9,
            "{dusty} of {desert} desert columns got dust; the rest were rained on"
        );
        assert!(
            wet * 100 >= wood * 97,
            "only {wet} of {wood} wooded columns were rained on"
        );
    }

    #[test]
    fn the_wettest_country_takes_the_heaviest_rain_out_of_one_storm() {
        let marsh = local_intensity(Weather::Rain, MARSH.1);
        let meadow = local_intensity(Weather::Rain, MEADOW.1);
        let dry = local_intensity(Weather::Rain, DESERT.1);
        assert!(marsh > meadow && meadow > dry, "{marsh} {meadow} {dry}");
        // Never to nothing and never past a storm's own weight: the
        // figure feeds a particle count and a sound bed.
        assert!(dry > 0.0);
        assert!(local_intensity(Weather::Storm, 1.0) <= Weather::Storm.intensity() * MONSOON_SHARE);
        assert_eq!(local_intensity(Weather::Clear, 1.0), 0.0);
    }

    #[test]
    fn the_mist_lies_on_a_bog_at_dawn_and_nowhere_else() {
        let bog = dawn_mist(0.95, DAWN);
        assert!(bog > 0.0, "a bog at first light had no mist");
        // Thicker on wetter ground...
        assert!(bog > dawn_mist(0.6, DAWN));
        // ...gone on dry ground, and gone by the middle of the day.
        assert_eq!(dawn_mist(0.3, DAWN), 0.0);
        assert_eq!(dawn_mist(0.95, 0.5), 0.0);
        assert_eq!(dawn_mist(0.95, 0.9), 0.0);
        // It eases in and out rather than switching on: a wall of fog
        // appearing between two frames is a bug a player reports.
        assert!(dawn_mist(0.95, DAWN - MIST_HOURS * 0.5) < bog);
        assert!(dawn_mist(0.95, DAWN - MIST_HOURS * 0.5) > 0.0);
    }

    #[test]
    fn a_spell_is_minutes_rather_than_seconds_or_hours() {
        const { assert!(SPELL_SECONDS.start >= 60.0, "showers should not flicker") };
        const { assert!(SPELL_SECONDS.end <= 3600.0, "nobody waits out an hour of rain") };
        const { assert!(SPELL_SECONDS.start < SPELL_SECONDS.end) };
    }
}

