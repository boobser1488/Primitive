//! The fog: what colour distance is, and where it starts.
//!
//! ## Why this is a module rather than four fields
//!
//! Fog is the one piece of this renderer that four other things all have
//! an opinion about, and until this file existed each of them held a
//! piece of it. The sky decided the colour, the settings decided the
//! range, `main` decided what happens under water, and the shaders
//! decided the curve -- and none of them could be read without the other
//! three, because the numbers only make sense together.
//!
//! They are together now. What a caller wants is one question -- *what
//! is the fog this frame* -- and it gets one answer.
//!
//! ## The invariant everything else leans on
//!
//! **The fog colour is what the world fades into, so it has to be what
//! is behind the world.** The frame is cleared to it, the sky's gradient
//! reaches it at the horizon, and the terrain shader mixes all the way
//! to it at `end`. Those three agreeing is the whole reason a limited
//! render distance does not read as a wall: terrain thins out into
//! exactly the colour that was already there.
//!
//! Break the agreement anywhere and the symptom is the same -- a visible
//! line drawn round the edge of the world -- which is why the colour has
//! one definition and three readers rather than three definitions.
//!
//! ## Under water it is not fog at all
//!
//! Air haze is a distance cue, and a player may switch it off. Water is
//! a *medium*: it absorbs red first and keeps absorbing with depth,
//! whether or not anybody wants a distance cue. So the two are separate
//! -- the shader applies absorption before the fog and independently of
//! the toggle -- and what this module supplies for the submerged case is
//! a much nearer range and the colour of the water.

use glam::Vec3;

use crate::engine::sky::Sky;
use crate::settings::ClientSettings;

/// The colour deep water closes in to.
///
/// Not the surface colour: looking *through* water is a longer path than
/// looking *at* it, so it lands somewhere darker and greener than the
/// blue the surface shows.
const UNDERWATER: Vec3 = Vec3::new(0.10, 0.28, 0.38);

/// Where the underwater fog starts, as a fraction of where it ends.
///
/// Much nearer than the fifth or so air uses. Under water there is no
/// clear near field: the medium is between the eye and everything,
/// including what is an arm's length away.
const UNDERWATER_START: f32 = 0.15;

/// The fog for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    /// What distance is the colour of, and what the frame is cleared to.
    pub color: Vec3,
    /// Where it begins, in blocks.
    pub start: f32,
    /// Where it is complete, in blocks. Terrain past this is exactly
    /// `color`, which is why it can be culled -- see the renderer.
    pub end: f32,
    /// Whether the player has it switched on at all.
    ///
    /// Off means the *air* haze is off. It does not turn off the water,
    /// which is not a preference.
    pub enabled: bool,
    /// Whether the eye is under water.
    pub underwater: bool,
}

impl Fog {
    /// The fog for this frame, from the sky, the settings and where the
    /// player's head is.
    pub fn for_frame(
        settings: &ClientSettings,
        sky: &Sky,
        render_distance_chunks: i32,
        enabled: bool,
        underwater: bool,
    ) -> Self {
        let (start, end) = if underwater {
            let end = settings.underwater_fog_distance;
            (end * UNDERWATER_START, end)
        } else {
            settings.fog_range(render_distance_chunks)
        };

        Self {
            color: Self::color(sky, underwater),
            start,
            end,
            enabled,
            underwater,
        }
    }

    /// What distance is the colour of.
    ///
    /// Tinted toward the sky rather than being a grey, so distant
    /// terrain dissolves into the horizon instead of into a band. That
    /// is the whole trick to making a limited render distance not look
    /// like one.
    ///
    /// Desaturated a little on the way, because fog is not sky: air
    /// scatters everything, so a long look through it trends toward its
    /// own brightness rather than keeping the sky's colour at full
    /// strength.
    fn color(sky: &Sky, underwater: bool) -> Vec3 {
        if underwater {
            return UNDERWATER;
        }
        let sky = sky.sky_color();
        let grey = Vec3::splat(sky.length() / 3f32.sqrt());
        sky.lerp(grey, 0.15)
    }

    /// How far a chunk may be before none of it can change a pixel.
    ///
    /// Past `end` the fog is complete, and the frame was *cleared* to
    /// the same colour -- so a chunk out there produces exactly what is
    /// already in the buffer. The renderer culls against this, and it is
    /// not a trade: the picture is identical.
    ///
    /// `None` where nothing is invisible at distance. With the fog off
    /// there is no such distance; under water the terrain shader dims
    /// toward the water colour *before* the fog mix, so at maximum range
    /// a submerged chunk lands on the fog colour while the sky behind it
    /// lands on a fraction of it -- and the difference would show as a
    /// dark band where the far terrain used to be. Nothing is lost by
    /// drawing it: the underwater range is short enough that the frustum
    /// has already thrown away almost everything.
    pub fn cull_distance(&self) -> Option<f32> {
        (self.enabled && !self.underwater).then_some(self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sky_at(time: f32) -> Sky {
        Sky::new(time, 600.0)
    }

    #[test]
    fn under_water_the_range_collapses_and_the_colour_changes() {
        let settings = ClientSettings::default();
        let air = Fog::for_frame(&settings, &sky_at(0.5), 12, true, false);
        let water = Fog::for_frame(&settings, &sky_at(0.5), 12, true, true);

        assert!(water.end < air.end, "water should close the world in");
        assert!(water.start < water.end);
        assert_ne!(water.color, air.color);
        assert_eq!(water.color, UNDERWATER);
    }

    #[test]
    fn the_colour_follows_the_sky_rather_than_being_a_grey() {
        // The whole trick: terrain has to fade into the horizon, so the
        // fog is the sky's own colour and changes with the hour.
        let settings = ClientSettings::default();
        let noon = Fog::for_frame(&settings, &sky_at(0.5), 12, true, false);
        let midnight = Fog::for_frame(&settings, &sky_at(0.0), 12, true, false);
        assert_ne!(noon.color, midnight.color);
        assert!(noon.color.length() > midnight.color.length(), "night is darker");
    }

    #[test]
    fn distance_culling_is_offered_only_where_it_is_free() {
        let settings = ClientSettings::default();
        let plain = Fog::for_frame(&settings, &sky_at(0.5), 12, true, false);
        assert_eq!(plain.cull_distance(), Some(plain.end));

        // Off: nothing is invisible at distance any more.
        assert_eq!(
            Fog::for_frame(&settings, &sky_at(0.5), 12, false, false).cull_distance(),
            None
        );
        // Under water the terrain and the sky do not agree at maximum
        // range, so culling there would draw a band.
        assert_eq!(
            Fog::for_frame(&settings, &sky_at(0.5), 12, true, true).cull_distance(),
            None
        );
    }

    #[test]
    fn the_range_widens_with_the_render_distance() {
        // Fog that ends at a fixed distance is fog that hides the world
        // on a machine that could draw it.
        let settings = ClientSettings::default();
        let near = Fog::for_frame(&settings, &sky_at(0.5), 4, true, false);
        let far = Fog::for_frame(&settings, &sky_at(0.5), 16, true, false);
        assert!(far.end > near.end);
        assert!(far.start > near.start);
    }
}
