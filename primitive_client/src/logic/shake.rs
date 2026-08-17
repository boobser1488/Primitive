//! Camera motion that is not the player moving: the running bob and the
//! jolt of taking a hit.
//!
//! Both come out as a small offset added to the eye when the view matrix
//! is built, and *only* there. The break/place ray and the transform
//! sent to the server both read `Camera::position`, which this never
//! touches -- otherwise a cosmetic wobble would become aim wander, and a
//! player standing still would stream movement updates forever.
//!
//! ## Two different shapes
//!
//! **Running** is periodic and predictable: a figure-of-eight traced by
//! the head, at a rate tied to stride. It has to be steady, because it
//! is on screen continuously and anything random reads as a fault.
//!
//! **Damage** is the opposite: a sharp kick that decays. It is measured
//! in *trauma* rather than amplitude, and the offset goes as the square
//! of it, which is the standard trick -- the shake fades away smoothly
//! instead of stopping dead while still visibly moving.

use glam::Vec3;

/// Peak sideways travel of the head while sprinting, in blocks.
const RUN_SWAY: f32 = 0.070;
/// Peak vertical travel. Half the sway, so the head traces a flattened
/// figure of eight rather than a circle.
const RUN_BOB: f32 = 0.045;
/// Peak roll while sprinting, in radians (about 1.4 degrees).
///
/// The tilt, not the travel, is what a player actually sees. Six
/// centimetres of head movement against terrain metres away is nearly
/// pure parallax; a degree of roll moves every pixel on screen.
const RUN_ROLL: f32 = 0.024;
/// Peak pitch while sprinting -- the nod at each footfall.
const RUN_PITCH: f32 = 0.010;
/// Radians of stride phase per block travelled.
///
/// At the 8.8 blocks per second of a sprint this is about one and a
/// quarter strides a second, which is a run rather than a jog.
const RUN_PHASE_PER_BLOCK: f32 = 0.90;
/// How fast the bob winds up and down when you start or stop running.
/// Instant would snap the view; this takes about a fifth of a second.
const RUN_BLEND_PER_SEC: f32 = 6.0;

/// Trauma added by one point of damage.
///
/// Twenty points is a full health bar, so a hit that nearly kills you
/// tops out the shake while a scratch is a twitch.
const TRAUMA_PER_DAMAGE: f32 = 0.09;
/// Trauma shed per second. A hit is over in about half a second.
const TRAUMA_DECAY_PER_SEC: f32 = 2.2;
/// Offset at full trauma, in blocks.
const TRAUMA_AMPLITUDE: f32 = 0.22;
/// Peak angular kick at full trauma, in radians (about four degrees).
const TRAUMA_ANGLE: f32 = 0.070;
/// How fast the damage shake oscillates. High enough to read as a jolt
/// rather than a sway.
const TRAUMA_RATE: f32 = 27.0;

#[derive(Default)]
pub struct Shake {
    /// Advances with distance travelled, not with time, so the bob stays
    /// in step with the stride when the player speeds up or stops.
    run_phase: f32,
    /// 0..1, how much of the run bob is currently applied.
    run_blend: f32,
    /// 0..1, how much damage shake is left.
    trauma: f32,
    /// Advances with time while trauma lasts.
    trauma_phase: f32,
}

impl Shake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the jolt for `damage` points of health lost.
    pub fn on_damage(&mut self, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        self.trauma = (self.trauma + damage * TRAUMA_PER_DAMAGE).clamp(0.0, 1.0);
    }

    /// How much damage shake is left, 0..1.
    ///
    /// The frame loop only needs `offset`; this is what the tests assert
    /// on, because "the shake decays and is bounded" is a statement
    /// about the trauma rather than about any one frame's offset.
    #[allow(dead_code)]
    pub fn trauma(&self) -> f32 {
        self.trauma
    }

    /// Advances both effects by one frame.
    ///
    /// `speed` is the player's horizontal speed in blocks per second and
    /// `running` whether they are actually sprinting on the ground --
    /// the bob is a footfall, so it stops in mid-air and while swimming.
    pub fn update(&mut self, dt: f32, speed: f32, running: bool) {
        let dt = dt.clamp(0.0, 0.1);

        let target = if running && speed > 0.1 { 1.0 } else { 0.0 };
        let blend_step = RUN_BLEND_PER_SEC * dt;
        self.run_blend += (target - self.run_blend).clamp(-blend_step, blend_step);
        self.run_blend = self.run_blend.clamp(0.0, 1.0);
        // Phase follows distance, so the stride stays in step with the
        // ground rather than racing ahead while the player slows down.
        self.run_phase = (self.run_phase + speed * dt * RUN_PHASE_PER_BLOCK)
            .rem_euclid(std::f32::consts::TAU);

        if self.trauma > 0.0 {
            self.trauma = (self.trauma - TRAUMA_DECAY_PER_SEC * dt).max(0.0);
            self.trauma_phase =
                (self.trauma_phase + TRAUMA_RATE * dt).rem_euclid(std::f32::consts::TAU);
        } else {
            self.trauma_phase = 0.0;
        }
    }

    /// The offset to add to the eye this frame.
    ///
    /// `right` and `up` are the camera's own axes, so the bob sways
    /// across the view rather than along a fixed world axis -- otherwise
    /// running north and running east would look different.
    pub fn offset(&self, right: Vec3, up: Vec3) -> Vec3 {
        let mut offset = Vec3::ZERO;

        if self.run_blend > 0.0 {
            // Vertical at twice the rate of horizontal: one dip per
            // footfall, one sway per pair of them.
            let sway = self.run_phase.sin() * RUN_SWAY;
            let bob = (self.run_phase * 2.0).cos() * RUN_BOB;
            offset += (right * sway + up * bob) * self.run_blend;
        }

        if self.trauma > 0.0 {
            // Squared, so the shake eases out instead of being switched
            // off while still visibly moving.
            let amount = self.trauma * self.trauma * TRAUMA_AMPLITUDE;
            let x = self.trauma_phase.sin();
            let y = (self.trauma_phase * 1.7 + 1.3).sin();
            offset += (right * x + up * y) * amount;
        }

        offset
    }

    /// The angular offset this frame, as (pitch, yaw, roll) in radians.
    ///
    /// This is the half of the effect the player actually notices; see
    /// `Camera::shake_angles`.
    pub fn angles(&self) -> Vec3 {
        let mut angles = Vec3::ZERO;

        if self.run_blend > 0.0 {
            // Roll leads the sway by a quarter turn: the body tilts into
            // the step before the head reaches the far side of it.
            let roll = (self.run_phase + std::f32::consts::FRAC_PI_2).sin() * RUN_ROLL;
            // Pitch nods once per footfall, so at twice the sway rate.
            let pitch = (self.run_phase * 2.0).sin() * RUN_PITCH;
            angles += Vec3::new(pitch, 0.0, roll) * self.run_blend;
        }

        if self.trauma > 0.0 {
            let amount = self.trauma * self.trauma * TRAUMA_ANGLE;
            // Three different rates, so the kick does not resolve into a
            // clean circle the eye can follow.
            angles += Vec3::new(
                (self.trauma_phase * 1.3).sin(),
                (self.trauma_phase * 0.9 + 2.1).sin(),
                (self.trauma_phase * 1.6 + 0.7).sin(),
            ) * amount;
        }

        angles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: Vec3 = Vec3::X;
    const UP: Vec3 = Vec3::Y;

    fn run_for(shake: &mut Shake, seconds: f32, speed: f32, running: bool) {
        let step = 1.0 / 60.0;
        for _ in 0..((seconds / step) as usize) {
            shake.update(step, speed, running);
        }
    }

    #[test]
    fn standing_still_does_not_move_the_camera() {
        let mut shake = Shake::new();
        run_for(&mut shake, 2.0, 0.0, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO);
        assert_eq!(shake.angles(), Vec3::ZERO);
    }

    #[test]
    fn the_shake_is_mostly_rotation_because_that_is_what_is_seen() {
        // Regression: the first version was translation only. Six
        // centimetres of head movement against terrain metres away is
        // nearly pure parallax, so the effect was invisible and read as
        // simply not working.
        let mut running = Shake::new();
        run_for(&mut running, 2.0, 8.8, true);
        assert!(
            running.angles().length() > 0.0,
            "sprinting produced no rotation at all"
        );

        let mut hit = Shake::new();
        hit.on_damage(10.0);
        hit.update(1.0 / 60.0, 0.0, false);
        assert!(hit.angles().length() > 0.0, "a hit produced no rotation");
    }

    #[test]
    fn the_rotation_stays_small_enough_to_read_as_a_camera_and_not_a_fault() {
        // Big enough to see, small enough not to be motion sickness.
        let mut shake = Shake::new();
        shake.on_damage(1000.0);
        let step = 1.0 / 60.0;
        let mut worst = 0.0f32;
        for _ in 0..600 {
            shake.update(step, 8.8, true);
            worst = worst.max(shake.angles().length());
        }
        let degrees = worst.to_degrees();
        assert!(degrees > 0.5, "the shake tops out at {degrees} degrees, invisible");
        assert!(degrees < 12.0, "the shake reaches {degrees} degrees, which is a lurch");
    }

    #[test]
    fn walking_without_sprinting_does_not_bob() {
        // The bob is the sprint tell. Applying it to a walk means it is
        // on screen essentially always, which is what makes head bob
        // unpopular.
        let mut shake = Shake::new();
        run_for(&mut shake, 2.0, 5.5, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO);
        assert_eq!(shake.angles(), Vec3::ZERO);
    }

    #[test]
    fn the_stride_keeps_time_with_the_ground() {
        // Phase advances with distance, not with time, so slowing down
        // lengthens the stride instead of the legs spinning faster.
        // Kept short enough that neither phase wraps past a full turn,
        // or the comparison is between two angles modulo tau and means
        // nothing.
        let mut fast = Shake::new();
        let mut slow = Shake::new();
        for _ in 0..20 {
            fast.update(1.0 / 60.0, 8.8, true);
            slow.update(1.0 / 60.0, 2.0, true);
        }
        assert!(fast.run_phase < std::f32::consts::TAU);
        assert!(
            fast.run_phase > slow.run_phase,
            "the stride ignored how fast the player was going: {} vs {}",
            fast.run_phase,
            slow.run_phase
        );
    }

    #[test]
    fn sprinting_moves_the_camera_but_not_far() {
        let mut shake = Shake::new();
        run_for(&mut shake, 2.0, 8.8, true);
        let offset = shake.offset(RIGHT, UP);
        assert!(offset.length() > 0.0, "no bob while sprinting");
        assert!(
            offset.length() < 0.12,
            "the bob is {} blocks, which is a lurch not a bob",
            offset.length()
        );
    }

    #[test]
    fn the_bob_stays_within_its_bounds_over_a_long_run() {
        let mut shake = Shake::new();
        let step = 1.0 / 60.0;
        let mut worst = 0.0f32;
        for _ in 0..2000 {
            shake.update(step, 8.8, true);
            worst = worst.max(shake.offset(RIGHT, UP).length());
        }
        let bound = (RUN_SWAY * RUN_SWAY + RUN_BOB * RUN_BOB).sqrt() + 1e-3;
        assert!(worst <= bound, "bob reached {worst}, bound is {bound}");
    }

    #[test]
    fn the_bob_actually_oscillates_rather_than_drifting() {
        // A phase that only ever grows one way would slide the camera
        // off to the side instead of swaying.
        let mut shake = Shake::new();
        let step = 1.0 / 60.0;
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for _ in 0..600 {
            shake.update(step, 8.8, true);
            let x = shake.offset(RIGHT, UP).x;
            lowest = lowest.min(x);
            highest = highest.max(x);
        }
        assert!(lowest < 0.0 && highest > 0.0, "swayed only one way");
    }

    #[test]
    fn stopping_eases_the_bob_out_instead_of_snapping() {
        let mut shake = Shake::new();
        run_for(&mut shake, 2.0, 8.8, true);
        let moving = shake.offset(RIGHT, UP).length();
        assert!(moving > 0.0);

        // One frame after letting go it must have shrunk, not vanished.
        shake.update(1.0 / 60.0, 8.8, false);
        let just_after = shake.offset(RIGHT, UP).length();
        assert!(just_after < moving, "the bob did not start fading");
        assert!(just_after > 0.0, "the bob snapped off in a single frame");

        run_for(&mut shake, 1.0, 0.0, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO, "the bob never settled");
        assert_eq!(shake.angles(), Vec3::ZERO, "the tilt never settled");
    }

    #[test]
    fn taking_a_hit_shakes_the_view() {
        let mut shake = Shake::new();
        shake.on_damage(6.0);
        shake.update(1.0 / 60.0, 0.0, false);
        assert!(shake.offset(RIGHT, UP).length() > 0.0, "a hit did nothing");
    }

    #[test]
    fn a_bigger_hit_shakes_harder() {
        let mut small = Shake::new();
        small.on_damage(2.0);
        small.update(1.0 / 60.0, 0.0, false);

        let mut big = Shake::new();
        big.on_damage(18.0);
        big.update(1.0 / 60.0, 0.0, false);

        assert!(
            big.trauma() > small.trauma(),
            "damage size did not affect the shake"
        );
    }

    #[test]
    fn the_shake_dies_down_on_its_own() {
        let mut shake = Shake::new();
        shake.on_damage(20.0);
        run_for(&mut shake, 3.0, 0.0, false);
        assert_eq!(shake.trauma(), 0.0);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO, "the shake never stopped");
    }

    #[test]
    fn repeated_hits_do_not_shake_the_screen_off() {
        let mut shake = Shake::new();
        for _ in 0..50 {
            shake.on_damage(20.0);
        }
        shake.update(1.0 / 60.0, 0.0, false);
        assert!(shake.trauma() <= 1.0, "trauma ran away: {}", shake.trauma());
        assert!(
            shake.offset(RIGHT, UP).length() <= TRAUMA_AMPLITUDE * 1.5,
            "the view was thrown {} blocks",
            shake.offset(RIGHT, UP).length()
        );
    }

    #[test]
    fn healing_is_not_a_hit() {
        let mut shake = Shake::new();
        shake.on_damage(-5.0);
        shake.on_damage(0.0);
        assert_eq!(shake.trauma(), 0.0);
    }

    #[test]
    fn a_long_frame_does_not_teleport_the_view() {
        // dt is clamped, so a stall cannot advance the phase by a whole
        // cycle and make the camera jump.
        let mut shake = Shake::new();
        shake.on_damage(20.0);
        shake.update(5.0, 8.8, true);
        assert!(shake.offset(RIGHT, UP).length().is_finite());
        assert!(shake.trauma() >= 0.0);
    }

    #[test]
    fn the_bob_follows_the_camera_rather_than_the_world() {
        // Running north and running east must look the same, so the
        // offset is built from the camera's own axes.
        let mut shake = Shake::new();
        run_for(&mut shake, 1.0, 8.8, true);
        let east = shake.offset(Vec3::X, Vec3::Y);
        let north = shake.offset(Vec3::Z, Vec3::Y);
        assert!((east.length() - north.length()).abs() < 1e-6);
        assert_ne!(east, north, "the offset ignored the axes it was given");
    }
}
