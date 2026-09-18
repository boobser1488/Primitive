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

    #[test]
    fn a_spell_is_minutes_rather_than_seconds_or_hours() {
        const { assert!(SPELL_SECONDS.start >= 60.0, "showers should not flicker") };
        const { assert!(SPELL_SECONDS.end <= 3600.0, "nobody waits out an hour of rain") };
        const { assert!(SPELL_SECONDS.start < SPELL_SECONDS.end) };
    }
}

