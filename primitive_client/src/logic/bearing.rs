//! Which way is north: read off the sky, or off a needle.
//!
//! ## What the sky says, and why the hint is only there when you look up
//!
//! The sun rides one circle a day (`Sky::sun_direction`): up in the east
//! at dawn, a little south of overhead at noon -- the circle is tilted a
//! quarter toward +z, which is south, because north is -z on the map
//! (`map_screen::heading`) -- and down in the west at dusk. The moon rides
//! the same circle behind it by its phase (`Sky::moon_direction`), so it
//! is read the same way. The stars turn about the z axis (`sky.wgsl`,
//! "stars"), so the one still point of the night sky is due north on the
//! horizon, and a player who watches them wheel knows where it is.
//!
//! So a player who *looks* can orient, and the hint says what they would
//! work out: "by the sun: north is to your left". It is there **only while
//! they are looking at the thing it is read from** -- the sun or the moon
//! within [`GAZE`] of the middle of the view, or the stars with the view
//! turned up past [`STARGAZE`] -- under open sky ([`SkyView::open_sky`]),
//! and not under cloud ([`OVERCAST_BLIND`]).
//!
//! Three ways were weighed:
//!
//! * **A needle on the HUD all the time.** The free compass this game took
//!   away (the note in `ui::journal`). Orienting has to cost a look.
//! * **Nothing: the sky is drawn, let players read it.** True, and the sky
//!   is honest about it. But the circle is tilted a quarter, which a player
//!   cannot see, and nothing in the game says the stars turn about north.
//!   A reading nobody can learn is scenery.
//! * **A hint while looking at the sky** (chosen). It costs stopping and
//!   looking up, it is wrong-footed by an overcast day -- when the moss on
//!   a trunk's north face (`ground`) is what is left -- and it names only
//!   the four quarters. The made compass is the instrument that answers
//!   exactly and in any weather; that is what iron buys.
//!
//! ## The needle
//!
//! [`needle`] is where north is on a dial whose top is where the player
//! faces. A dial that turned *with* the map would be the map's arrow again;
//! this one answers "which way is the top of my map from here".

use glam::Vec3;

/// How near the middle of the view the sun or moon has to be for the hint
/// to count it as looked at: thirty degrees, as a cosine. About the middle
/// half of the view at the default field of view -- a player glancing at
/// the horizon at dusk sees it; one walking along with the sun at their
/// shoulder does not.
pub const GAZE: f32 = 0.866;

/// How far up the view has to be turned for the stars to count as looked
/// at: thirty degrees, as the sine the look's height is.
pub const STARGAZE: f32 = 0.5;

/// How overcast a sky may be and still give a bearing. Past this the sun
/// is a bright patch in the cloud with no edge to it, and the night has no
/// stars (`sky.wgsl` puts them out behind cloud).
pub const OVERCAST_BLIND: f32 = 0.6;

/// How lit the moon has to be to be read: a crescent thinner than this is
/// not something a player would find in the sky to read off.
pub const MOON_BRIGHT_ENOUGH: f32 = 0.15;

/// What the reading was taken off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guide {
    Sun,
    Moon,
    Stars,
}

/// Where north is from where the player is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ahead,
    Right,
    Behind,
    Left,
}

/// A bearing read off the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub guide: Guide,
    pub north: Side,
}

/// What the sky shows and where the player looks, as [`read_sky`] needs it.
/// Built from `Sky` by the frame; built by hand in the tests, which is why
/// it is plain numbers rather than a `&Sky`.
#[derive(Debug, Clone, Copy)]
pub struct SkyView {
    /// The camera's forward, unit length.
    pub look: Vec3,
    /// Toward the sun (the opposite of `Sky::sun_direction`, which is the
    /// way the light travels).
    pub to_sun: Vec3,
    /// Toward the moon.
    pub to_moon: Vec3,
    /// How much of the moon is lit, 0..1 (`moon::illumination`).
    pub moon_lit: f32,
    /// `Sky::overcast`.
    pub overcast: f32,
    /// Whether the sky is over the player's head at all: full sky light
    /// where the eyes are. In a cave, a house or a dense wood there is no
    /// sky to read, and a hint there would be the game knowing north for
    /// them.
    pub open_sky: bool,
}

/// The bearing of a direction on the ground, clockwise from north, in
/// radians: north 0, east a quarter turn, south a half. North is -z and
/// east +x (`map_screen::heading`).
pub fn bearing_of(x: f32, z: f32) -> f32 {
    x.atan2(-z).rem_euclid(std::f32::consts::TAU)
}

/// The bearing the camera looks along, from its yaw (forward on the ground
/// is `(cos yaw, sin yaw)`).
pub fn facing(yaw: f32) -> f32 {
    bearing_of(yaw.cos(), yaw.sin())
}

/// Which quarter north falls in, seen from a player facing `facing`.
pub fn north_from(facing: f32) -> Side {
    // How far round to the right north is, 0..TAU, in quarters with the
    // quarter boundaries half way between the four sides.
    let right = (-facing).rem_euclid(std::f32::consts::TAU);
    let quarter = ((right / std::f32::consts::FRAC_PI_2) + 0.5).floor() as i32 % 4;
    match quarter {
        0 => Side::Ahead,
        1 => Side::Right,
        2 => Side::Behind,
        _ => Side::Left,
    }
}

/// Where the needle of a dial whose top is the way the player faces points:
/// the angle clockwise from the top of the dial to north, in radians --
/// `hud::spoke`'s convention, positive to the right.
pub fn needle(yaw: f32) -> f32 {
    -facing(yaw)
}

/// What the sky tells a player looking where they are looking, if anything.
///
/// The sun first, then the moon, then the stars: a player looking at the
/// sun is reading the sun, and the stars are only the answer on a night
/// with nothing brighter in the part of the sky being looked at.
pub fn read_sky(view: &SkyView) -> Option<Reading> {
    if !view.open_sky || view.overcast >= OVERCAST_BLIND {
        return None;
    }
    let look = view.look.normalize_or_zero();
    let north = north_from(bearing_of(look.x, look.z));
    let looked_at = |toward: Vec3| toward.y > -0.02 && look.dot(toward.normalize_or_zero()) >= GAZE;
    if looked_at(view.to_sun) {
        return Some(Reading { guide: Guide::Sun, north });
    }
    if view.moon_lit >= MOON_BRIGHT_ENOUGH && looked_at(view.to_moon) {
        return Some(Reading { guide: Guide::Moon, north });
    }
    // The stars are out when the sun is well down: `sky.wgsl` fades them
    // in on daylight, which is nought a little past the sun's setting.
    let night = view.to_sun.normalize_or_zero().y < -0.2;
    if night && look.y >= STARGAZE {
        return Some(Reading { guide: Guide::Stars, north });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::sky::Sky;

    /// A player under open, clear sky at `time_of_day`, looking at `look`.
    fn view(sky: &Sky, look: Vec3) -> SkyView {
        SkyView {
            look: look.normalize(),
            to_sun: -sky.sun_direction(),
            to_moon: -sky.moon_direction(),
            moon_lit: primitive_shared::moon::illumination(sky.world_days()),
            overcast: 0.0,
            open_sky: true,
        }
    }

    #[test]
    fn the_sun_puts_north_on_the_left_at_dawn_behind_at_noon_and_on_the_right_at_dusk() {
        // Dawn: the sun is in the east, so a player facing it has north on
        // their left hand.
        let dawn_sky = Sky::new(0.27, 1200.0);
        let dawn = read_sky(&view(&dawn_sky, -dawn_sky.sun_direction()));
        assert_eq!(dawn, Some(Reading { guide: Guide::Sun, north: Side::Left }), "dawn");
        // Noon: the sun stands a little south of overhead, and north is
        // behind the player looking up at it.
        let noon_sky = Sky::new(0.5, 1200.0);
        let noon = read_sky(&view(&noon_sky, -noon_sky.sun_direction()));
        assert_eq!(noon, Some(Reading { guide: Guide::Sun, north: Side::Behind }), "noon");
        // Dusk: the sun goes down in the west, and north is on the right.
        let dusk_sky = Sky::new(0.73, 1200.0);
        let dusk = read_sky(&view(&dusk_sky, -dusk_sky.sun_direction()));
        assert_eq!(dusk, Some(Reading { guide: Guide::Sun, north: Side::Right }), "dusk");
    }

    #[test]
    fn the_sun_says_nothing_to_a_player_who_is_not_looking_at_it() {
        let sky = Sky::new(0.27, 1200.0);
        // Facing west at dawn, with the sun at their back.
        assert_eq!(read_sky(&view(&sky, Vec3::new(-1.0, 0.1, 0.0))), None);
    }

    #[test]
    fn cloud_and_a_roof_take_the_reading_away() {
        let sky = Sky::new(0.5, 1200.0);
        let mut clouded = view(&sky, -sky.sun_direction());
        clouded.overcast = 0.9;
        assert_eq!(read_sky(&clouded), None, "an overcast noon gave a bearing");
        let mut indoors = view(&sky, -sky.sun_direction());
        indoors.open_sky = false;
        assert_eq!(read_sky(&indoors), None, "a roof gave a bearing");
    }

    #[test]
    fn looking_up_at_a_clear_night_reads_the_stars() {
        let sky = Sky::new(0.0, 1200.0);
        // Up and a little to the north, with the moon wherever it is.
        let mut night = view(&sky, Vec3::new(0.0, 0.8, -0.6));
        night.moon_lit = 0.0;
        assert_eq!(read_sky(&night), Some(Reading { guide: Guide::Stars, north: Side::Ahead }));
    }

    #[test]
    fn the_needle_points_north_whichever_way_the_player_faces() {
        use std::f32::consts::{FRAC_PI_2, PI};
        // Yaw 0 looks along +x, east; north is -z, so yaw -PI/2 looks north.
        let close = |a: f32, b: f32| (a - b).rem_euclid(std::f32::consts::TAU).min((b - a).rem_euclid(std::f32::consts::TAU)) < 1e-4;
        assert!(close(needle(-FRAC_PI_2), 0.0), "facing north, the needle is not straight up");
        assert!(close(needle(0.0), -FRAC_PI_2), "facing east, the needle is not to the left");
        assert!(close(needle(FRAC_PI_2), PI), "facing south, the needle is not straight down");
        assert!(close(needle(PI), FRAC_PI_2), "facing west, the needle is not to the right");
    }
}
