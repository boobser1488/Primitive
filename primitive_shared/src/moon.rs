//! The moon: where in its month it is, and how much of it is lit.
//!
//! "Сделай фазы луны", from a list of light things that make a decision: a
//! night under a full moon is one a player can walk through, and a night with
//! no moon is one they wait out by a fire or cross with a torch. The moon used
//! to stand opposite the sun every night with a shadow swinging across it on
//! the hour, so it changed shape during a night and was never full on any
//! night in particular -- a picture of a moon, with nothing in the world that
//! depended on it.
//!
//! ## Why the month is eight days
//!
//! The year is twelve days (`season`), so a real month of twenty-nine and a
//! half would be longer than the year, and a player who plays an evening
//! would only ever know one phase of it. A month of one day would change the
//! moon's shape inside a single night, which is the thing being fixed. Eight
//! turns the moon a phase every night -- full, then waning, a moonless night,
//! then waxing -- so an evening's play sees it change and a player can plan
//! a trip for the bright nights. It also rises later each night by an eighth
//! of a turn, three hours, which is how a real moon is told apart from a lamp
//! nailed to the sky.
//!
//! Every function takes `world_time`, the world's age in days with the hour
//! in the fraction -- `season`'s clock -- so the client and the server read
//! the same moon off the same number.

/// Days from one new moon to the next. See the module note for why eight.
pub const CYCLE_DAYS: f32 = 8.0;

/// Where in its month a new world's moon starts: past first quarter, so the
/// first night of a world has a bright moon well up, and the first moonless
/// night comes on the fifth -- by when a player has had four nights to learn
/// that fire is worth making.
const START: f32 = 0.375;

/// Where the moon is in its month, 0..1: nought the new moon, a half the
/// full moon, a quarter and three quarters the half moons waxing and waning.
///
/// Also the angle the moon stands from the sun, as a share of a turn: new
/// beside the sun, full opposite it. The client places the moon in the sky by
/// this (`Sky::moon_direction`), which is what makes the shape it draws and
/// the time the moon is up agree.
pub fn phase(world_time: f32) -> f32 {
    (world_time / CYCLE_DAYS + START).rem_euclid(1.0)
}

/// How much of the moon's face is lit, 0..1: the cosine of the angle from
/// the sun, halved -- nought at the new moon, one at the full.
pub fn illumination(world_time: f32) -> f32 {
    (1.0 - (phase(world_time) * std::f32::consts::TAU).cos()) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_moon_comes_round_in_its_month_and_is_full_half_way() {
        for day in 0..40 {
            let t = day as f32 * 0.37;
            assert!((phase(t) - phase(t + CYCLE_DAYS)).abs() < 1e-4, "the month at {t} is not {CYCLE_DAYS} days long");
        }
        // Full on the second night of a world, new on the sixth.
        assert!((phase(1.0) - 0.5).abs() < 1e-5 && illumination(1.0) > 0.999, "the full moon is not on day 1");
        assert!(illumination(5.0) < 1e-3, "the new moon is not on day 5");
    }

    #[test]
    fn a_new_world_opens_under_a_bright_moon_and_every_month_has_a_moonless_night() {
        assert!(illumination(0.0) > 0.8, "the first night of a world is dark: {}", illumination(0.0));
        let darkest = (0..8).map(|day| illumination(day as f32)).fold(f32::MAX, f32::min);
        assert!(darkest < 0.05, "no night of the month is moonless: the darkest is {darkest}");
        // A phase a night: no two midnights in a row share a moon.
        for day in 0..8 {
            let (tonight, tomorrow) = (illumination(day as f32), illumination(day as f32 + 1.0));
            assert!((tonight - tomorrow).abs() > 0.1, "day {day} and day {} have the same moon", day + 1);
        }
    }
}
