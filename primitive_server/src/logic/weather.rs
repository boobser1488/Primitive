//! The sky, as a state machine.
//!
//! ## What is here and what is in `primitive_shared::weather`
//!
//! There: what weather *is* -- the three states, how heavy each one is,
//! how much daylight it leaves, how long a spell lasts. Both sides read
//! that, because the client draws the rain and the server decides what
//! it does to a fire, and a disagreement between the two is a player
//! sheltering from weather nobody else can see.
//!
//! Here: the clock. One value for the world, a countdown, and a roll
//! when it runs out.
//!
//! ## Why it is this small
//!
//! Because the alternative -- a pressure field over the map, fronts that
//! move, rain that arrives from the west -- is a simulation, and what a
//! player wants from weather is that it arrives, changes what they can
//! do, and stops. Every one of those is delivered by a countdown and a
//! coin, and none of them is delivered any better by a model of the
//! atmosphere.
//!
//! The one thing that *is* local is what falls: the same storm is rain
//! over a valley and snow over the peak above it, decided per column
//! from the temperature the world generator already computed. That is
//! the cheap half of a regional system and it is the half that shows.
//! See `weather::falls_as_snow` -- and `season::falls_as_snow`, which
//! is the same line moved by the time of year: the storm that is rain
//! over the valley in July is snow over it in January, and the valley
//! itself was never changed. The season is the other thing that is not
//! decided here: it is a function of the world's clock
//! (`season::ambient_offset_c`), not a state the sky rolls for, because
//! a season that arrived on a coin would be one nobody could prepare
//! for.

use primitive_shared::weather::{Weather, CHANCE_OF_STORM, CHANCE_OF_WEATHER, SPELL_SECONDS};

use crate::logic::rng::Rng;

/// The world's sky.
pub struct Sky {
    weather: Weather,
    /// Seconds until the next roll.
    remaining: f32,
    /// Set by `/weather`, and what makes it different from the same
    /// weather arriving on its own: an operator's choice is not
    /// overwritten thirty seconds later by the countdown that was
    /// already running.
    held: bool,
    rng: Rng,
}

impl Default for Sky {
    fn default() -> Self {
        Self::new()
    }
}

impl Sky {
    /// A fresh sky. Clear, and clear for a while.
    ///
    /// **A world does not open in the rain**, deliberately. The first
    /// minutes of a new world are the ones where a player is learning
    /// what everything is, and a downpour teaches them the rain before
    /// it teaches them the game -- and puts out the first fire they
    /// manage to light.
    pub fn new() -> Self {
        let mut rng = Rng::from_clock();
        let first = rng.range(SPELL_SECONDS.start, SPELL_SECONDS.end);
        Self {
            weather: Weather::Clear,
            remaining: first,
            held: false,
            rng,
        }
    }

    /// The same, repeatable. For tests, and for anybody who wants a
    /// server whose weather is the same every run.
    pub fn seeded(seed: u64) -> Self {
        let mut sky = Self::new();
        sky.rng = Rng::seeded(seed);
        sky.remaining = sky.rng.range(SPELL_SECONDS.start, SPELL_SECONDS.end);
        sky
    }

    pub fn weather(&self) -> Weather {
        self.weather
    }

    /// Seconds until this spell ends. Shown by `/weather` with no
    /// argument, which is the only way for an operator to find out
    /// whether the rain they are standing in is nearly over.
    pub fn remaining(&self) -> f32 {
        self.remaining
    }

    /// Is the weather being held where an operator put it?
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// One tick. Returns the new weather if it changed, so the caller
    /// can broadcast -- and nothing at all the rest of the time, which
    /// is almost every tick.
    pub fn step(&mut self, dt: f32) -> Option<Weather> {
        if self.held || dt <= 0.0 {
            return None;
        }
        self.remaining -= dt;
        if self.remaining > 0.0 {
            return None;
        }
        let next = self.roll();
        self.remaining = self.rng.range(SPELL_SECONDS.start, SPELL_SECONDS.end);
        if next == self.weather {
            // The same weather again is not a change and must not be a
            // broadcast: two spells of rain in a row are one spell of
            // rain as far as anybody watching the sky is concerned.
            return None;
        }
        self.weather = next;
        Some(next)
    }

    /// What the sky does next.
    ///
    /// Weather always gives way to clear, and clear rolls for weather.
    /// That asymmetry is what keeps most of a session dry: without it,
    /// two in five spells are wet from *any* state and it rains nearly
    /// half the time.
    fn roll(&mut self) -> Weather {
        if self.weather.is_wet() {
            return Weather::Clear;
        }
        if !self.rng.chance(CHANCE_OF_WEATHER) {
            return Weather::Clear;
        }
        if self.rng.chance(CHANCE_OF_STORM) {
            Weather::Storm
        } else {
            Weather::Rain
        }
    }

    /// What `/weather` does.
    ///
    /// Sets the weather and **holds it**, until an operator hands it
    /// back with `release`. Anything else makes the command a suggestion:
    /// an operator who clears the sky for a screenshot and has it start
    /// raining again forty seconds later has not been given a command,
    /// they have been given a coin flip.
    pub fn set(&mut self, weather: Weather) -> bool {
        self.held = true;
        if self.weather == weather {
            return false;
        }
        self.weather = weather;
        true
    }

    /// Hands the sky back to the countdown.
    pub fn release(&mut self) {
        self.held = false;
        if self.remaining <= 0.0 {
            self.remaining = self.rng.range(SPELL_SECONDS.start, SPELL_SECONDS.end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the sky forward, collecting every change it announces.
    fn run(sky: &mut Sky, seconds: f32) -> Vec<Weather> {
        let mut changes = Vec::new();
        let mut elapsed = 0.0;
        while elapsed < seconds {
            if let Some(next) = sky.step(1.0) {
                changes.push(next);
            }
            elapsed += 1.0;
        }
        changes
    }

    #[test]
    fn a_new_world_opens_dry() {
        // The first minutes are the ones where a player is learning what
        // everything is. A downpour teaches them the rain first.
        let sky = Sky::seeded(1);
        assert_eq!(sky.weather(), Weather::Clear);
        assert!(sky.remaining() >= SPELL_SECONDS.start);
    }

    #[test]
    fn the_weather_eventually_changes_and_eventually_stops() {
        let mut sky = Sky::seeded(20260817);
        let changes = run(&mut sky, 12.0 * 3600.0);
        assert!(!changes.is_empty(), "half a day and the sky never moved");
        assert!(
            changes.iter().any(|w| w.is_wet()),
            "half a day and it never rained"
        );
        assert!(
            changes.iter().any(|w| !w.is_wet()),
            "it started raining and never stopped"
        );
    }

    #[test]
    fn most_of_a_session_is_dry() {
        // The asymmetry in `roll` stated as the thing it is for. A world
        // that rains half the time makes the fire unbuildable and the
        // weather background noise rather than an event.
        let mut sky = Sky::seeded(7);
        let mut wet_seconds = 0.0;
        for _ in 0..(6 * 3600) {
            sky.step(1.0);
            if sky.weather().is_wet() {
                wet_seconds += 1.0;
            }
        }
        let fraction = wet_seconds / (6.0 * 3600.0);
        assert!(
            fraction < 0.45,
            "it rained {}% of six hours",
            (fraction * 100.0) as i32
        );
        assert!(fraction > 0.0, "six hours without a drop");
    }

    #[test]
    fn the_same_weather_twice_is_not_announced_twice() {
        // Two spells of clear in a row are one clear sky as far as
        // anybody watching is concerned, and a broadcast for it is a
        // message that says nothing.
        let mut sky = Sky::seeded(3);
        let changes = run(&mut sky, 6.0 * 3600.0);
        for pair in changes.windows(2) {
            assert_ne!(pair[0], pair[1], "the same weather was announced twice running");
        }
    }

    #[test]
    fn rain_gives_way_to_clear_rather_than_to_more_rain() {
        let mut sky = Sky::seeded(11);
        sky.set(Weather::Storm);
        sky.release();
        // Wind it to the end of the spell.
        while sky.step(1.0).is_none() {}
        assert_eq!(sky.weather(), Weather::Clear);
    }

    #[test]
    fn an_operator_gets_the_sky_they_asked_for_and_keeps_it() {
        // The whole point of holding it. A command that is overwritten
        // forty seconds later by a countdown that was already running is
        // not a command, it is a suggestion.
        let mut sky = Sky::seeded(99);
        assert!(sky.set(Weather::Storm));
        assert!(sky.is_held());
        run(&mut sky, 4.0 * 3600.0);
        assert_eq!(sky.weather(), Weather::Storm, "the countdown overrode the operator");

        // ...and setting the same weather again is not a change worth
        // telling anybody about.
        assert!(!sky.set(Weather::Storm));

        sky.release();
        assert!(!sky.is_held());
        let changes = run(&mut sky, 6.0 * 3600.0);
        assert!(!changes.is_empty(), "releasing it left the sky stuck");
    }

    #[test]
    fn a_zero_or_negative_tick_changes_nothing() {
        // A server that hitches, or a test that steps by nothing.
        let mut sky = Sky::seeded(5);
        let before = sky.remaining();
        assert!(sky.step(0.0).is_none());
        assert!(sky.step(-4.0).is_none());
        assert_eq!(sky.remaining(), before);
    }

    #[test]
    fn a_seeded_sky_is_the_same_sky_twice() {
        let mut a = Sky::seeded(1234);
        let mut b = Sky::seeded(1234);
        assert_eq!(run(&mut a, 3.0 * 3600.0), run(&mut b, 3.0 * 3600.0));
    }
}
