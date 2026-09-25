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
//! **The stride** is periodic and predictable: a figure-of-eight traced
//! by the head, at a rate tied to how far the player has walked. It has
//! to be steady, because it is on screen continuously and anything
//! random reads as a fault.
//!
//! **Damage** is the opposite: a sharp kick that decays. It is measured
//! in *trauma* rather than amplitude, and the offset goes as the square
//! of it, which is the standard trick -- the shake fades away smoothly
//! instead of stopping dead while still visibly moving.
//!
//! **A recoil is a third shape, and it is neither of those.** A pick
//! meeting a rock face and a fall arriving at the floor are one push in
//! one direction, over in a fraction of a second; the trauma shake with
//! a smaller number in front of it would rattle the view, which reads as
//! the player being hit by their own tool. So [`Kick`] has a direction
//! and no oscillation at all: the view dips, and comes back. See
//! [`Shake::on_landing`].
//!
//! ## Walking bobs too now, and that is a setting
//!
//! It used to bob only while sprinting, on the argument that a bob on
//! screen essentially always is what makes head bob unpopular. The
//! argument is real and the conclusion was wrong: a walk with a
//! perfectly still head is the thing that makes a first-person game
//! read as a camera on rails rather than as a person. So a walk bobs at
//! `WALK_SHARE` of the sprint, and the players who cannot stand it get
//! the number itself -- `ClientSettings::view_bob`, zero for off.
//!
//! **Driven by distance, not by a clock**, and that is the bug this
//! shape exists to avoid: a bob advanced by `dt` goes on bobbing while
//! the player leans into a wall and goes nowhere, which is the classic
//! version of this effect done wrong. `speed * dt` is zero against a
//! wall, so the stride stops where the feet do.

use glam::Vec3;

/// Peak sideways travel of the head while sprinting, in blocks.
///
/// The four `RUN_` amplitudes below are all the sprint's, at
/// `view_bob = 1`. A walk takes `WALK_SHARE` of them and the setting
/// takes its own fraction of that, so what is on screen by default is
/// well under any of these numbers -- which is worth knowing before
/// reading one of them as "how much the view moves".
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
/// How much of the sprint's bob a walk gets.
///
/// **Not the whole of it, and not a tenth of it.** The sprint bob is
/// tuned to be felt -- it is the tell that says the stamina bar is
/// draining -- and a walk that bobbed as hard would leave the sprint
/// with nothing left to say. Half and a little more is enough for the
/// step to be *there* while still being obviously the quieter of the
/// two, which is the whole reason a walking bob is worth having: the
/// difference between the two gaits is information.
const WALK_SHARE: f32 = 0.55;

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

// **A blow does not move the camera, and that is a decision, not an
// omission.** The recoil was here: a dip of 0.026 rad, a drop of 0.028
// blocks and a third of that in roll, played when the head arrived. It
// felt right for one swing and wrong for the twentieth -- mining is a
// rhythm of blows held down for minutes at a time, and a view that
// flinched on every one of them made a quarry tiring to look at. The
// jolt of landing and the shake of being hurt stay: those are events,
// not a rhythm. If it ever comes back it belongs behind a setting,
// off by default.
//
// What it was for, kept because the argument is still sound and the
// next person will make it again: a pick met a rock face and the only
// thing that moved was the arm, so the player read as a tripod the arm
// was bolted to. A degree and a half of dip, deliberately under the
// four degrees of `TRAUMA_ANGLE`, was the difference between swinging
// a tool and pressing a button. What killed it is the *rate*: three or
// four blows a second, held down for minutes.

/// The jolt of a landing at [`crate::audio::soundscape`]'s loudest fall,
/// in radians (about three degrees), how far it drops the eye, and how
/// long it takes.
///
/// **Twice as long as a blow, and that is the whole of what makes the
/// two read differently.** A tool stopping is an impact; a body arriving
/// is knees giving and straightening, which takes a third of a second
/// whatever the fall was. Shorter and a ten-block drop feels like
/// stubbing a toe.
const LANDING_ANGLE: f32 = 0.052;
const LANDING_DROP: f32 = 0.080;
const LANDING_SECONDS: f32 = 0.34;

/// One push of the view: a direction, and how much of it is left.
///
/// **No oscillation, unlike the trauma shake**, and no random phase: a
/// blow landing is one event with one direction, and the eye reads a
/// sine wave at this size as a wobble rather than as an impact. What it
/// shares with the trauma is the squared decay -- the push eases out
/// instead of being switched off while still visibly displaced.
#[derive(Clone, Copy, Default)]
struct Kick {
    /// 1 the moment it lands, 0 when it is over.
    left: f32,
    /// How many seconds the whole push lasts. Zero means there is none.
    span: f32,
    /// Radians at full: negative dips the view.
    pitch: f32,
    roll: f32,
    /// Blocks at full, along the camera's own up: negative drops the eye.
    rise: f32,
}

impl Kick {
    /// How much of the push is applied this instant, 0..1.
    fn amount(self) -> f32 {
        self.left * self.left
    }

    /// The tilt on screen right now, as a positive size. What two kicks
    /// are compared by; see [`Shake::push`].
    fn tilt(self) -> f32 {
        self.pitch.abs() * self.amount()
    }
}

pub struct Shake {
    /// Advances with distance travelled, not with time, so the bob stays
    /// in step with the stride when the player speeds up or stops.
    run_phase: f32,
    /// 0..1, how much of the stride bob is currently applied: nought
    /// standing, `WALK_SHARE` walking, one sprinting, and easing
    /// between them.
    run_blend: f32,
    /// 0..1, how much damage shake is left.
    trauma: f32,
    /// Advances with time while trauma lasts.
    trauma_phase: f32,
    /// The one-shot push a struck blow or a landing left -- see [`Kick`].
    ///
    /// **One slot and not a list.** Two of these overlapping would need
    /// their directions summed, and what a player would see of the sum
    /// is whichever was bigger; `push` keeps the bigger one and drops
    /// the other, which is the same picture for none of the bookkeeping.
    kick: Kick,
    /// The player's `view_bob` setting, 0..1.
    ///
    /// **It scales the stride and not the trauma.** Turning the bob off
    /// is a statement about walking, not a request to stop noticing
    /// that something is eating you: a player who set this to zero and
    /// then stopped being told they were being hit would file that as a
    /// second bug.
    strength: f32,
}

impl Shake {
    /// `view_bob` is `ClientSettings::view_bob`: 0..1, zero for off.
    ///
    /// Taken at construction rather than read per frame because there is
    /// no screen that changes it while a world is open -- and a
    /// constructor that demands it is a constructor nobody can forget,
    /// which `Default` would have quietly allowed (at zero, i.e. the
    /// effect silently missing).
    pub fn new(view_bob: f32) -> Self {
        Self {
            run_phase: 0.0,
            run_blend: 0.0,
            trauma: 0.0,
            trauma_phase: 0.0,
            kick: Kick::default(),
            strength: if view_bob.is_finite() {
                view_bob.clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }

    /// Adds the jolt for `damage` points of health lost.
    pub fn on_damage(&mut self, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        self.trauma = (self.trauma + damage * TRAUMA_PER_DAMAGE).clamp(0.0, 1.0);
    }

    /// The jolt of arriving at the floor, `hardness` being 0 for a step
    /// off a kerb and 1 for a fall that is about to cost health.
    ///
    /// **The same number the landing sound is played at**
    /// (`soundscape::LANDING_THRESHOLD` to `LANDING_FULL`), so the thump
    /// and the jolt are one event rather than two effects that each
    /// decided for themselves what counted as a hard landing.
    pub fn on_landing(&mut self, hardness: f32) {
        let hardness = if hardness.is_finite() { hardness.clamp(0.0, 1.0) } else { 0.0 };
        if hardness <= 0.0 {
            return;
        }
        self.push(Kick {
            left: 1.0,
            span: LANDING_SECONDS,
            pitch: -LANDING_ANGLE * hardness,
            roll: 0.0,
            rise: -LANDING_DROP * hardness,
        });
    }

    /// Takes a new push if it is at least as big as what is still on
    /// screen.
    ///
    /// **Both halves of that matter.** A tap on a rock face must not cut
    /// short the jolt of a ten-block fall, or landing hard and carrying
    /// on digging would swallow the landing; and a second blow must not
    /// be swallowed by the tail of the first, or a rhythm of blows would
    /// be one dip and then nothing.
    fn push(&mut self, kick: Kick) {
        if kick.tilt() >= self.kick.tilt() {
            self.kick = kick;
        }
    }

    /// How much of the stride bob is on screen this frame, 0..1: the
    /// gait's own blend with the player's setting already in it.
    ///
    /// Its own function because `offset` and `angles` must not be able
    /// to disagree about it. They are two halves of one effect, and a
    /// setting applied to the travel but not to the tilt would turn the
    /// bob *off* into a bob that only rolls.
    fn stride(&self) -> f32 {
        self.run_blend * self.strength
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
    /// `speed` is the player's horizontal speed in blocks per second,
    /// `footed` whether they are on the ground under their own power,
    /// and `running` whether that is a sprint -- the bob is a footfall,
    /// so it stops in mid-air and while swimming, and the two flags are
    /// separate because a walk bobs less than a sprint rather than not
    /// at all.
    ///
    /// `speed` is checked here as well as by the caller, so "a player
    /// pushing into a wall does not bob" is a property of this type on
    /// its own and not of the one line that happens to call it.
    pub fn update(&mut self, dt: f32, speed: f32, footed: bool, running: bool) {
        let dt = dt.clamp(0.0, 0.1);

        let target = if !footed || speed <= 0.1 {
            0.0
        } else if running {
            1.0
        } else {
            WALK_SHARE
        };
        let blend_step = RUN_BLEND_PER_SEC * dt;
        self.run_blend += (target - self.run_blend).clamp(-blend_step, blend_step);
        self.run_blend = self.run_blend.clamp(0.0, 1.0);
        // Phase follows distance, so the stride stays in step with the
        // ground rather than racing ahead while the player slows down --
        // and only while there is a stride to be in step with. A jump
        // and a fall cover ground at speed and take no steps doing it,
        // and a phase that ran on through them would land the player
        // mid-stride from a waveform nothing on screen was moving to.
        if target > 0.0 {
            self.run_phase = (self.run_phase + speed * dt * RUN_PHASE_PER_BLOCK)
                .rem_euclid(std::f32::consts::TAU);
        }

        if self.trauma > 0.0 {
            self.trauma = (self.trauma - TRAUMA_DECAY_PER_SEC * dt).max(0.0);
            self.trauma_phase =
                (self.trauma_phase + TRAUMA_RATE * dt).rem_euclid(std::f32::consts::TAU);
        } else {
            self.trauma_phase = 0.0;
        }

        // **Straight to zero, and the slot cleared with it.** The push
        // has to end *exactly* at nothing rather than at a millionth of
        // a radian: `push` compares against what is left, and a kick
        // that never quite finished would go on refusing quiet ones for
        // the rest of the session.
        if self.kick.span > 0.0 {
            self.kick.left -= dt / self.kick.span;
            if self.kick.left <= 0.0 {
                self.kick = Kick::default();
            }
        }
    }

    /// The offset to add to the eye this frame.
    ///
    /// `right` and `up` are the camera's own axes, so the bob sways
    /// across the view rather than along a fixed world axis -- otherwise
    /// running north and running east would look different.
    pub fn offset(&self, right: Vec3, up: Vec3) -> Vec3 {
        let mut offset = Vec3::ZERO;

        let stride = self.stride();
        if stride > 0.0 {
            // Vertical at twice the rate of horizontal: one dip per
            // footfall, one sway per pair of them.
            //
            // **Both are sines of the phase and neither carries a phase
            // offset**, so the whole stride passes through the
            // un-bobbed view at every footfall -- see
            // `the_stride_passes_through_the_still_view_at_every_footfall`.
            // The vertical was a cosine, which is what made this an arc
            // the head swept back and forth rather than the figure of
            // eight the module doc has always claimed: `cos 2p` is
            // `1 - 2 sin^2 p`, a parabola in the sway, and a parabola is
            // not a figure of anything.
            let sway = self.run_phase.sin() * RUN_SWAY;
            let bob = (self.run_phase * 2.0).sin() * RUN_BOB;
            offset += (right * sway + up * bob) * stride;
        }

        if self.trauma > 0.0 {
            // Squared, so the shake eases out instead of being switched
            // off while still visibly moving.
            let amount = self.trauma * self.trauma * TRAUMA_AMPLITUDE;
            let x = self.trauma_phase.sin();
            let y = (self.trauma_phase * 1.7 + 1.3).sin();
            offset += (right * x + up * y) * amount;
        }

        // The recoil rides the camera's own up for the same reason the
        // stride rides its right: a dip has to be a dip whichever way
        // the player is facing.
        if self.kick.span > 0.0 {
            offset += up * (self.kick.rise * self.kick.amount());
        }

        offset
    }

    /// The angular offset this frame, as (pitch, yaw, roll) in radians.
    ///
    /// This is the half of the effect the player actually notices; see
    /// `Camera::shake_angles`.
    pub fn angles(&self) -> Vec3 {
        let mut angles = Vec3::ZERO;

        let stride = self.stride();
        if stride > 0.0 {
            // Roll rides *with* the sway rather than a quarter turn
            // ahead of it: the body tilts toward the leg it is standing
            // on, and it is upright at the moment both feet pass.
            //
            // The quarter-turn lead that used to be here made the tilt
            // peak as the head crossed the middle, which is a
            // defensible reading of a stride and has one consequence
            // that is not: the phase where the head is level is then a
            // phase where the view is rolled, so the stride never
            // passes through the picture the crosshair implies. A
            // player who stops walking at that instant sees the world
            // straighten under a still crosshair.
            let roll = self.run_phase.sin() * RUN_ROLL;
            // Pitch nods once per footfall, so at twice the sway rate.
            let pitch = (self.run_phase * 2.0).sin() * RUN_PITCH;
            angles += Vec3::new(pitch, 0.0, roll) * stride;
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

        // **Not scaled by the `view_bob` setting**, for the reason the
        // trauma is not: turning the bob off is a statement about
        // *walking*, and a player who turned it off and then stopped
        // being told that their pick had landed or that the floor had
        // arrived would file that as a second bug. No yaw, unlike the
        // trauma: a push that turned the view sideways would read as the
        // mouse having moved.
        if self.kick.span > 0.0 {
            let amount = self.kick.amount();
            angles += Vec3::new(self.kick.pitch * amount, 0.0, self.kick.roll * amount);
        }

        angles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: Vec3 = Vec3::X;
    const UP: Vec3 = Vec3::Y;

    /// Everything below is measured with the setting at its maximum, so
    /// a failure is about the shape of the bob rather than about how
    /// much of it the default asks for.
    fn shake() -> Shake {
        Shake::new(1.0)
    }

    fn run_for(shake: &mut Shake, seconds: f32, speed: f32, running: bool) {
        walk_for(shake, seconds, speed, running, running);
    }

    fn walk_for(shake: &mut Shake, seconds: f32, speed: f32, footed: bool, running: bool) {
        let step = 1.0 / 60.0;
        for _ in 0..((seconds / step) as usize) {
            shake.update(step, speed, footed, running);
        }
    }

    #[test]
    fn standing_still_does_not_move_the_camera() {
        let mut shake = shake();
        run_for(&mut shake, 2.0, 0.0, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO);
        assert_eq!(shake.angles(), Vec3::ZERO);
    }

    #[test]
    fn walking_bobs_and_sprinting_bobs_harder() {
        // The player asked for a bob on foot; the sprint has to stay
        // the louder of the two or the gait stops carrying information.
        let mut walking = shake();
        walk_for(&mut walking, 2.0, 4.3, true, false);
        let mut sprinting = shake();
        walk_for(&mut sprinting, 2.0, 4.3, true, true);

        // Compared at the same phase -- both were fed the same speed
        // for the same time -- so the only difference left is the
        // blend.
        let (walk, sprint) = (walking.stride(), sprinting.stride());
        assert!(walk > 0.0, "a walk did not bob at all");
        assert!(
            sprint > walk * 1.2,
            "a sprint bobs {sprint} against a walk's {walk}, which is not a difference"
        );
        assert!(walking.offset(RIGHT, UP).length() > 0.0, "a walk moved nothing");
        assert!(walking.angles().length() > 0.0, "a walk tilted nothing");
    }

    #[test]
    fn walking_into_a_wall_does_not_bob() {
        // **The classic way to get this wrong.** A phase advanced by
        // `dt` goes on bobbing while the player leans into stone and
        // goes nowhere; this one is advanced by `speed * dt`, and the
        // blend is let go of as well, so the head settles.
        let mut shake = shake();
        walk_for(&mut shake, 2.0, 4.3, true, false);
        assert!(shake.offset(RIGHT, UP).length() > 0.0);
        // Still holding W, still on the ground, going nowhere.
        walk_for(&mut shake, 1.0, 0.0, true, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO, "the wall kept bobbing");
        assert_eq!(shake.angles(), Vec3::ZERO, "the wall kept tilting");
    }

    #[test]
    fn the_bob_switched_off_is_a_bob_that_is_not_there() {
        // `view_bob = 0` is the whole of the setting's contract, and it
        // has to reach the tilt as well as the travel: half an effect
        // switched off is a view that rolls without moving.
        let mut off = Shake::new(0.0);
        walk_for(&mut off, 2.0, 8.8, true, true);
        assert_eq!(off.offset(RIGHT, UP), Vec3::ZERO);
        assert_eq!(off.angles(), Vec3::ZERO);
    }

    #[test]
    fn a_quieter_setting_is_a_quieter_bob_and_not_a_shorter_stride() {
        let mut loud = Shake::new(1.0);
        let mut quiet = Shake::new(0.4);
        walk_for(&mut loud, 2.0, 8.8, true, true);
        walk_for(&mut quiet, 2.0, 8.8, true, true);
        // Same phase, so the ratio is the setting and nothing else.
        assert!(quiet.offset(RIGHT, UP).length() < loud.offset(RIGHT, UP).length());
        assert!(
            (quiet.run_phase - loud.run_phase).abs() < 1e-5,
            "the setting changed how long a stride is"
        );
    }

    #[test]
    fn the_setting_never_shakes_the_screen_off_however_it_is_hand_edited() {
        // A settings file is user input, and this one is read straight
        // into an amplitude.
        for asked in [-4.0, f32::NAN, f32::INFINITY, 900.0] {
            let mut shake = Shake::new(asked);
            walk_for(&mut shake, 2.0, 8.8, true, true);
            let bob = shake.offset(RIGHT, UP).length();
            assert!(bob.is_finite(), "view_bob = {asked} gave {bob}");
            assert!(bob <= 0.12, "view_bob = {asked} threw the view {bob} blocks");
        }
    }

    #[test]
    fn the_stride_passes_through_the_still_view_at_every_footfall() {
        // **What the crosshair is drawn against.** The bob is cosmetic
        // and the aim is not (see `Camera::shake`), so the two are only
        // ever the same picture at the instants the stride is at rest.
        // A waveform with a phase offset in it never has one: the head
        // is level exactly when the view is rolled, and the world
        // straightens under a still crosshair the moment the player
        // stops.
        //
        // Twice per cycle, because a stride is two footfalls.
        let mut shake = shake();
        walk_for(&mut shake, 1.0, 4.3, true, false);
        for footfall in [0.0, std::f32::consts::PI, std::f32::consts::TAU] {
            shake.run_phase = footfall;
            assert!(
                shake.offset(RIGHT, UP).length() < 1e-6,
                "the head is {} off the eye at phase {footfall}",
                shake.offset(RIGHT, UP).length()
            );
            assert!(
                shake.angles().length() < 1e-6,
                "the view is {} radians off level at phase {footfall}",
                shake.angles().length()
            );
        }
    }

    #[test]
    fn the_shake_is_mostly_rotation_because_that_is_what_is_seen() {
        // Regression: the first version was translation only. Six
        // centimetres of head movement against terrain metres away is
        // nearly pure parallax, so the effect was invisible and read as
        // simply not working.
        let mut running = shake();
        run_for(&mut running, 2.0, 8.8, true);
        assert!(
            running.angles().length() > 0.0,
            "sprinting produced no rotation at all"
        );

        let mut hit = shake();
        hit.on_damage(10.0);
        hit.update(1.0 / 60.0, 0.0, false, false);
        assert!(hit.angles().length() > 0.0, "a hit produced no rotation");
    }

    #[test]
    fn the_rotation_stays_small_enough_to_read_as_a_camera_and_not_a_fault() {
        // Big enough to see, small enough not to be motion sickness.
        let mut shake = shake();
        shake.on_damage(1000.0);
        let step = 1.0 / 60.0;
        let mut worst = 0.0f32;
        for _ in 0..600 {
            shake.update(step, 8.8, true, true);
            worst = worst.max(shake.angles().length());
        }
        let degrees = worst.to_degrees();
        assert!(degrees > 0.5, "the shake tops out at {degrees} degrees, invisible");
        assert!(degrees < 12.0, "the shake reaches {degrees} degrees, which is a lurch");
    }

    #[test]
    fn nothing_off_the_ground_bobs() {
        // A footfall is the whole mechanism, so what has no feet on the
        // ground has no bob: a jump, a fall, a swim. This used to be
        // covered by "only a sprint bobs" and is not any more, which is
        // exactly why it is now stated on its own.
        let mut shake = shake();
        walk_for(&mut shake, 2.0, 5.5, false, false);
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
        let mut fast = shake();
        let mut slow = shake();
        for _ in 0..20 {
            fast.update(1.0 / 60.0, 8.8, true, true);
            slow.update(1.0 / 60.0, 2.0, true, true);
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
        let mut shake = shake();
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
        let mut shake = shake();
        let step = 1.0 / 60.0;
        let mut worst = 0.0f32;
        for _ in 0..2000 {
            shake.update(step, 8.8, true, true);
            worst = worst.max(shake.offset(RIGHT, UP).length());
        }
        let bound = (RUN_SWAY * RUN_SWAY + RUN_BOB * RUN_BOB).sqrt() + 1e-3;
        assert!(worst <= bound, "bob reached {worst}, bound is {bound}");
    }

    #[test]
    fn the_bob_actually_oscillates_rather_than_drifting() {
        // A phase that only ever grows one way would slide the camera
        // off to the side instead of swaying.
        let mut shake = shake();
        let step = 1.0 / 60.0;
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for _ in 0..600 {
            shake.update(step, 8.8, true, true);
            let x = shake.offset(RIGHT, UP).x;
            lowest = lowest.min(x);
            highest = highest.max(x);
        }
        assert!(lowest < 0.0 && highest > 0.0, "swayed only one way");
    }

    #[test]
    fn stopping_eases_the_bob_out_instead_of_snapping() {
        let mut shake = shake();
        run_for(&mut shake, 2.0, 8.8, true);
        let moving = shake.offset(RIGHT, UP).length();
        assert!(moving > 0.0);

        // One frame after letting go it must have shrunk, not vanished.
        shake.update(1.0 / 60.0, 8.8, false, false);
        let just_after = shake.offset(RIGHT, UP).length();
        assert!(just_after < moving, "the bob did not start fading");
        assert!(just_after > 0.0, "the bob snapped off in a single frame");

        run_for(&mut shake, 1.0, 0.0, false);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO, "the bob never settled");
        assert_eq!(shake.angles(), Vec3::ZERO, "the tilt never settled");
    }

    #[test]
    fn taking_a_hit_shakes_the_view() {
        let mut shake = shake();
        shake.on_damage(6.0);
        shake.update(1.0 / 60.0, 0.0, false, false);
        assert!(shake.offset(RIGHT, UP).length() > 0.0, "a hit did nothing");
    }

    #[test]
    fn a_bigger_hit_shakes_harder() {
        let mut small = shake();
        small.on_damage(2.0);
        small.update(1.0 / 60.0, 0.0, false, false);

        let mut big = shake();
        big.on_damage(18.0);
        big.update(1.0 / 60.0, 0.0, false, false);

        assert!(
            big.trauma() > small.trauma(),
            "damage size did not affect the shake"
        );
    }

    #[test]
    fn the_shake_dies_down_on_its_own() {
        let mut shake = shake();
        shake.on_damage(20.0);
        run_for(&mut shake, 3.0, 0.0, false);
        assert_eq!(shake.trauma(), 0.0);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO, "the shake never stopped");
    }

    #[test]
    fn repeated_hits_do_not_shake_the_screen_off() {
        let mut shake = shake();
        for _ in 0..50 {
            shake.on_damage(20.0);
        }
        shake.update(1.0 / 60.0, 0.0, false, false);
        assert!(shake.trauma() <= 1.0, "trauma ran away: {}", shake.trauma());
        assert!(
            shake.offset(RIGHT, UP).length() <= TRAUMA_AMPLITUDE * 1.5,
            "the view was thrown {} blocks",
            shake.offset(RIGHT, UP).length()
        );
    }

    #[test]
    fn healing_is_not_a_hit() {
        let mut shake = shake();
        shake.on_damage(-5.0);
        shake.on_damage(0.0);
        assert_eq!(shake.trauma(), 0.0);
    }

    #[test]
    fn a_long_frame_does_not_teleport_the_view() {
        // dt is clamped, so a stall cannot advance the phase by a whole
        // cycle and make the camera jump.
        let mut shake = shake();
        shake.on_damage(20.0);
        shake.update(5.0, 8.8, true, true);
        assert!(shake.offset(RIGHT, UP).length().is_finite());
        assert!(shake.trauma() >= 0.0);
    }

    #[test]
    fn the_bob_follows_the_camera_rather_than_the_world() {
        // Running north and running east must look the same, so the
        // offset is built from the camera's own axes.
        let mut shake = shake();
        run_for(&mut shake, 1.0, 8.8, true);
        let east = shake.offset(Vec3::X, Vec3::Y);
        let north = shake.offset(Vec3::Z, Vec3::Y);
        assert!((east.length() - north.length()).abs() < 1e-6);
        assert_ne!(east, north, "the offset ignored the axes it was given");
    }

    // -------------------------------------------------------- the jolt

    /// How many frames at sixty a second a jolt is allowed to last.
    /// [`LANDING_SECONDS`] is 0.34, so two fifths of a second plus the
    /// frame the decay finishes in; `HALF_WAY` is where a jolt must
    /// still be visible.
    const HALF_WAY: usize = 11;
    const LANDING_FRAMES: usize = 21;

    fn frames(shake: &mut Shake, count: usize) {
        for _ in 0..count {
            shake.update(1.0 / 60.0, 0.0, false, false);
        }
    }

    #[test]
    fn a_hard_landing_jolts_and_the_jolt_comes_home() {
        // A body arriving is knees giving and straightening: a fifth of
        // a second in and it is still bent, two fifths and it is level
        // again. A jolt that never finished would go on refusing
        // quieter ones (`push` compares against what is left) for the
        // rest of the session.
        let mut landed = shake();
        landed.on_landing(1.0);
        frames(&mut landed, HALF_WAY);
        assert!(
            landed.angles().x < 0.0,
            "the landing was over before it should have been"
        );
        frames(&mut landed, LANDING_FRAMES - HALF_WAY);
        assert_eq!(landed.angles(), Vec3::ZERO, "the landing never came home");
    }

    #[test]
    fn stepping_off_a_kerb_does_not_shake_the_screen() {
        // `soundscape` hands over a hardness of zero for anything under
        // its landing threshold, and zero has to mean nothing at all --
        // otherwise every stair in the game is a jolt.
        let mut shake = shake();
        shake.on_landing(0.0);
        assert_eq!(shake.angles(), Vec3::ZERO);
        assert_eq!(shake.offset(RIGHT, UP), Vec3::ZERO);
    }

    #[test]
    fn a_short_drop_does_not_cut_short_the_jolt_of_a_long_fall() {
        // Down a shaft in steps is the ordinary way into a mine, and
        // the small push must not replace the big one -- nor the other
        // way about, or the first landing of a descent would be the
        // only one anybody felt.
        let mut shake = shake();
        shake.on_landing(1.0);
        let jolt = shake.angles().x;
        shake.on_landing(0.2);
        assert_eq!(shake.angles().x, jolt, "a step down swallowed the landing");

        // ...and the other way: once the jolt has faded, the next one
        // is felt.
        frames(&mut shake, LANDING_FRAMES);
        shake.on_landing(0.2);
        assert!(shake.angles().x < 0.0, "the tail of a jolt ate the next landing");
    }

    #[test]
    fn the_recoil_is_not_the_bob_and_the_bob_setting_does_not_silence_it() {
        // Turning `view_bob` off is a statement about walking. A player
        // who turned it off and then stopped being told that their pick
        // had landed, or that the floor had arrived, would file that as
        // a second bug -- the same argument the trauma shake makes.
        let mut off = Shake::new(0.0);
        off.on_landing(1.0);
        assert!(off.angles().x < 0.0, "the bob setting silenced the jolt");
        assert!(off.offset(RIGHT, UP).y < 0.0, "the bob setting silenced the jolt");
    }

    #[test]
    fn a_jolt_never_turns_the_view_sideways() {
        // Yaw is the one axis a mouse owns. A push that moved it would
        // be indistinguishable from the mouse having moved, which is
        // the one thing a first-person camera must never fake.
        //
        // **The trauma shake is the deliberate exception** and is not
        // asserted here: being hurt swings all three axes, because a
        // blow taken is supposed to read as the world hitting the
        // player rather than as the player looking about.
        let mut shake = shake();
        shake.on_landing(1.0);
        assert_eq!(shake.angles().y, 0.0);
        frames(&mut shake, LANDING_FRAMES);
        shake.on_landing(0.4);
        assert_eq!(shake.angles().y, 0.0);
    }

    #[test]
    fn a_recoil_is_bounded_however_it_is_asked_for() {
        // `hardness` is arithmetic on a fall speed and is checked
        // nowhere else.
        for asked in [-3.0, f32::NAN, f32::INFINITY, 40.0] {
            let mut shake = shake();
            shake.on_landing(asked);
            let tilt = shake.angles().length();
            assert!(tilt.is_finite(), "{asked} gave {tilt}");
            assert!(
                tilt <= LANDING_ANGLE + 1e-6,
                "{asked} threw the view {tilt} radians"
            );
        }
    }
}
