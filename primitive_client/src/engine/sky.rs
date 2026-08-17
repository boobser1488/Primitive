//! Sky, sun and fog colour, derived from the server's world clock.
//!
//! The time of day is *server state*, not a local animation: the server
//! sends `TimeSync` and the client interpolates between those messages at
//! the day length it was told in `Welcome`. Two players standing next to
//! each other therefore see the same sunset at the same moment, which is
//! the whole point of syncing it rather than each client running its own
//! clock.
//!
//! Only the sun *direction* and *strength* change here. The per-block
//! light levels baked into the chunk meshes never move, so a full
//! day/night cycle costs zero re-meshing -- the shader does
//! `max(skylight × daylight, blocklight)` per fragment.

use glam::Vec3;

/// Ambient floor, so a moonlit night is navigable rather than pitch black.
const NIGHT_INTENSITY: f32 = 0.09;

const DAY_SKY: Vec3 = Vec3::new(0.53, 0.80, 0.92);
const SUNSET_SKY: Vec3 = Vec3::new(0.85, 0.45, 0.28);
const NIGHT_SKY: Vec3 = Vec3::new(0.02, 0.03, 0.08);

pub struct Sky {
    /// 0.0 = midnight, 0.5 = noon.
    pub time_of_day: f32,
    day_length_seconds: f32,
    /// Where the server last told us the clock was; we ease toward it
    /// rather than snapping, so a late `TimeSync` doesn't visibly jolt
    /// the sun.
    target_time: f32,
    /// Seconds since this sky was made.
    ///
    /// The cloud layer is the only thing that reads it, and it has to:
    /// `time_of_day` wraps at midnight, and a drift driven by a number
    /// that wraps snaps the whole sky back to where it was a day ago.
    /// Clouds are also the one part of the sky nobody expects two
    /// players to agree about, so a purely local clock costs nothing.
    elapsed: f32,
}

impl Sky {
    pub fn new(time_of_day: f32, day_length_seconds: f32) -> Self {
        Self {
            time_of_day: time_of_day.rem_euclid(1.0),
            day_length_seconds: day_length_seconds.max(1.0),
            target_time: time_of_day.rem_euclid(1.0),
            elapsed: 0.0,
        }
    }

    /// Seconds since this sky was made -- the cloud drift's clock.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn on_time_sync(&mut self, time_of_day: f32) {
        self.target_time = time_of_day.rem_euclid(1.0);
    }

    pub fn tick(&mut self, dt: f32) {
        self.elapsed += dt.max(0.0);
        self.time_of_day = (self.time_of_day + dt / self.day_length_seconds).rem_euclid(1.0);

        // Shortest-path correction around the 0.0/1.0 wrap.
        let mut delta = self.target_time - self.time_of_day;
        if delta > 0.5 {
            delta -= 1.0;
        } else if delta < -0.5 {
            delta += 1.0;
        }
        self.time_of_day = (self.time_of_day + delta * (dt * 2.0).min(1.0)).rem_euclid(1.0);
        self.target_time = (self.target_time + dt / self.day_length_seconds).rem_euclid(1.0);
    }

    /// Height of the sun above the horizon, -1..1. 0 = exactly at the
    /// horizon, 1 = directly overhead.
    pub fn sun_elevation(&self) -> f32 {
        ((self.time_of_day - 0.25) * std::f32::consts::TAU).sin()
    }

    /// Direction the sunlight *travels* (from the sun toward the ground),
    /// which is what the shader wants for a dot product against a face
    /// normal.
    pub fn sun_direction(&self) -> Vec3 {
        let angle = (self.time_of_day - 0.25) * std::f32::consts::TAU;
        // A little Z tilt so faces on the north/south axis aren't lit
        // perfectly evenly all day, which reads as flat.
        let to_sun = Vec3::new(angle.cos(), angle.sin(), 0.25).normalize();
        -to_sun
    }

    /// How strongly skylight is scaled right now.
    pub fn sun_intensity(&self) -> f32 {
        let e = self.sun_elevation();
        if e <= 0.0 {
            // Below the horizon: fade to the night floor over the last
            // bit of dusk rather than cutting to black.
            NIGHT_INTENSITY + (1.0 + e).max(0.0).powi(8) * (0.35 - NIGHT_INTENSITY)
        } else {
            (NIGHT_INTENSITY + e.powf(0.6) * (1.0 - NIGHT_INTENSITY)).min(1.0)
        }
    }

    pub fn sky_color(&self) -> Vec3 {
        let e = self.sun_elevation();
        if e >= 0.25 {
            DAY_SKY
        } else if e >= 0.0 {
            SUNSET_SKY.lerp(DAY_SKY, smoothstep(0.0, 0.25, e))
        } else if e >= -0.2 {
            NIGHT_SKY.lerp(SUNSET_SKY, smoothstep(-0.2, 0.0, e))
        } else {
            NIGHT_SKY
        }
    }

    pub fn clock_string(&self) -> String {
        let minutes = (self.time_of_day * 24.0 * 60.0) as i32;
        format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_and_midnight_is_not() {
        let noon = Sky::new(0.5, 600.0);
        let midnight = Sky::new(0.0, 600.0);
        assert!(noon.sun_intensity() > 0.9);
        assert!(midnight.sun_intensity() < 0.15);
        assert!(noon.sun_elevation() > 0.99);
        assert!(midnight.sun_elevation() < -0.99);
    }

    #[test]
    fn the_sun_is_overhead_at_noon() {
        let noon = Sky::new(0.5, 600.0);
        // Light travels downward at noon.
        assert!(noon.sun_direction().y < -0.9);
    }

    #[test]
    fn time_wraps_instead_of_running_away() {
        let mut sky = Sky::new(0.99, 10.0);
        sky.tick(1.0);
        assert!((0.0..1.0).contains(&sky.time_of_day), "{}", sky.time_of_day);
    }

    #[test]
    fn a_sync_across_the_midnight_wrap_takes_the_short_way() {
        let mut sky = Sky::new(0.99, 100_000.0);
        sky.on_time_sync(0.01);
        for _ in 0..60 {
            sky.tick(1.0 / 60.0);
        }
        // Should have crossed 0.0 forward, not run backwards through noon.
        assert!(
            sky.time_of_day < 0.1 || sky.time_of_day > 0.98,
            "took the long way round: {}",
            sky.time_of_day
        );
    }
}
