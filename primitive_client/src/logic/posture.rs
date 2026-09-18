//! How the local player's body is arranged, and what that does to the eye.
//!
//! **The server decides and this is the client's copy of the answer.**
//! Sitting and lying used to be server state and nothing else: a player who
//! sat on a stool was told "you sit down" in the chat and went on standing
//! a stride away from it with their eye at standing height, and a sleeper's
//! camera stayed wherever they had clicked the bed from. The server now
//! puts the body on the seat or across the bed and says so
//! (`protocol::ServerMessage::Posture`); this is where the eye goes while
//! it is there.

use glam::Vec3;
use primitive_shared::geometry::EYE_HEIGHT;
use primitive_shared::protocol::Posture;

/// How high the eye is above the seat, sitting.
///
/// A seated person's eye is a little under half their height above what
/// they sit on. The collider's feet rest on the seat -- that is where the
/// server puts them -- so this takes the place of `EYE_HEIGHT` rather than
/// being taken off it: at a table the eye is just above the top, which is
/// what sitting at a table looks like.
pub const SEATED_EYE: f32 = 0.82;

/// How high the eye is above the mattress, lying: a head on a pillow.
pub const LYING_EYE: f32 = 0.28;

/// How far toward the head of the bed the eye is from the middle of the
/// body, which is where the server lays a sleeper's feet: most of a
/// body's half-length, so the view is from the pillow and not from the
/// sleeper's navel.
pub const LYING_HEAD_REACH: f32 = 0.72;

/// How far a sleeper looks up when they lie down, in radians.
///
/// Along the bed and a little above it -- at the foot of the bed and the
/// room beyond -- rather than straight at the ceiling, which in a hut is a
/// screen of boards and says nothing about where you are.
pub const LYING_PITCH: f32 = 0.3;

/// The local player's posture, and the one number lying needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resting {
    Standing,
    /// `facing` is the way a chair faces, in the `Camera::forward`
    /// convention, and `None` on a stool, which has no front.
    Sitting { facing: Option<f32> },
    /// `head_yaw` points from the middle of the body toward the head, in
    /// the `Camera::forward` convention.
    Lying { head_yaw: f32 },
}

impl Resting {
    /// What the server said, and what the seat under the body is.
    ///
    /// **The seat comes from the client's own copy of the world**, because
    /// the message cannot say it: its yaw is the chair's facing for a chair
    /// and the player's own last-sent yaw for a stool, and turning the
    /// camera to the second is a jolt back to where the mouse was fifty
    /// milliseconds ago. `seat` is the block the body was put on; only a
    /// seat with a front (`types::seat_yaw`) turns the view.
    pub fn from_wire(posture: Posture, yaw: f32, seat: Option<primitive_shared::types::BlockId>) -> Self {
        match posture {
            Posture::Standing => Resting::Standing,
            Posture::Sitting => Resting::Sitting {
                facing: seat.and_then(primitive_shared::types::seat_yaw).map(|_| yaw),
            },
            Posture::Lying => Resting::Lying { head_yaw: yaw },
            // Never sent for the player's own body (`Posture::Fallen`); the
            // death screen is how a client learns it died, and reading it
            // as standing is the one answer that cannot trap the camera.
            Posture::Fallen => Resting::Standing,
            // Never sent for the player's own body either (`Posture::Swimming`):
            // the client's own physics knows when it swims.
            Posture::Swimming => Resting::Standing,
            // A rider sits, and with no front of their own to be turned to:
            // the horse turns under them and the view goes with the reins
            // (`horseback`), not with a chair.
            Posture::Mounted => Resting::Sitting { facing: None },
        }
    }

    /// Whether the body is on a seat of any kind.
    pub fn is_sitting(self) -> bool {
        matches!(self, Resting::Sitting { .. })
    }

    /// The camera's yaw on sitting down in a chair: the way it faces, so a
    /// chair put down facing the fire is sat in looking at the fire. `None`
    /// on a stool and for anything but sitting; the pitch is left alone,
    /// because nobody sits down staring at the ceiling.
    pub fn face_on_sitting_down(self) -> Option<f32> {
        match self {
            Resting::Sitting { facing } => facing,
            _ => None,
        }
    }

    /// Where the eye is, from where the feet are.
    pub fn eye(self, feet: glam::DVec3) -> glam::DVec3 {
        let offset = match self {
            Resting::Standing => Vec3::Y * EYE_HEIGHT,
            Resting::Sitting { .. } => Vec3::Y * SEATED_EYE,
            Resting::Lying { head_yaw } => {
                let (sin, cos) = head_yaw.sin_cos();
                Vec3::new(cos * LYING_HEAD_REACH, LYING_EYE, sin * LYING_HEAD_REACH)
            }
        };
        feet + offset.as_dvec3()
    }

    /// The camera's yaw and pitch on lying down: from the pillow, along the
    /// bed toward its foot. `None` for anything but lying.
    pub fn look_on_lying_down(self) -> Option<(f32, f32)> {
        match self {
            Resting::Lying { head_yaw } => Some((head_yaw + std::f32::consts::PI, LYING_PITCH)),
            _ => None,
        }
    }
}

/// How long the dark is held after a sleeper is told they are awake,
/// before it starts to lift, in seconds.
///
/// **The morning arrives as two messages** -- the new hour, then the
/// waking (see `sleep_through_to_dawn` on the server) -- and the frame the
/// second lands on is the frame the sky has only just been handed the
/// first. Lifting then would open the player's eyes on whatever the last
/// frame drew. Half a second more of black costs nothing after a whole
/// night; a glimpse of the sun jumping is the bug this dark exists to hide.
pub const DAWN_HOLD_SECONDS: f32 = 0.5;

/// How long the dark takes to lift, in seconds: a little quicker than it
/// fell (`body::FALLING_ASLEEP_SECONDS`), because waking is, and because
/// the player is waiting to see the morning.
pub const WAKING_SECONDS: f32 = 1.2;

/// How long a sleeper lies in the dark before the screen says why the
/// night is not passing, in seconds.
///
/// Past the moment the night would have passed for a sleeper alone, and a
/// round trip over that, so in singleplayer the morning always comes first
/// and the line is never seen. On a server where somebody is still up it is
/// the difference between a black screen that has frozen and a black
/// screen that is waiting for somebody.
pub const LONELY_AFTER_SECONDS: f32 = primitive_shared::body::NIGHT_PASSES_AFTER_SECONDS + 1.5;

/// How dark a sleeper's screen is, and whether that dark is lifting.
///
/// **The night passes behind this and nowhere else.** The server winds the
/// clock to dawn only once everybody has been asleep for longer than this
/// takes to go black (`body::NIGHT_PASSES_AFTER_SECONDS`), so the jump
/// lands on a screen that is already dark; the dark is held a moment after
/// the waking and then lifts on a morning, with the body still lying in the
/// bed, because morning no longer stands anybody up.
///
/// What it replaced was nothing at all: a notice saying "asleep" and a
/// notice saying "awake", fifty milliseconds apart, over a sky that jumped.
///
/// Rejected: tying the lift to the sky -- lifting when the hour is seen to
/// have jumped. A player who lies down at a quarter to six sleeps a quarter
/// of an hour and the sky barely moves, so there is no jump to see and the
/// screen would stay black; and a `/time` typed by somebody else would wake
/// every sleeper on the server. The server says when a sleeper is awake,
/// and that is what this listens to.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Sleep {
    /// What the server last said (`ServerMessage::Asleep`).
    asleep: bool,
    /// Nought clear to one black, moving at a constant rate; drawn eased.
    dark: f32,
    /// Seconds since the server said asleep.
    asleep_for: f32,
    /// Seconds the dark has been held since the server said awake.
    held: f32,
    /// Whether the dark now lifting was whole when the waking came: a
    /// morning, or a blow in the night, as opposed to getting up before the
    /// eyes had closed. Only it is held, and only it is announced.
    from_black: bool,
}

impl Sleep {
    /// What the server said. Only a change does anything: the message is
    /// sent on change, and a repeat must not restart the dark.
    pub fn set_asleep(&mut self, asleep: bool) {
        if asleep == self.asleep {
            return;
        }
        self.asleep = asleep;
        self.asleep_for = 0.0;
        self.held = 0.0;
        self.from_black = !asleep && self.dark >= 1.0;
    }

    /// Whether the server has this player asleep: the keys walk nobody.
    pub fn is_asleep(&self) -> bool {
        self.asleep
    }

    /// One frame. True on the frame the screen finishes clearing after a
    /// waking that began in the whole dark -- the moment to say it is
    /// morning, which said any earlier would be printed on black.
    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if self.asleep {
            self.asleep_for += dt;
            self.dark = (self.dark + dt / primitive_shared::body::FALLING_ASLEEP_SECONDS).min(1.0);
            return false;
        }
        if self.dark <= 0.0 {
            return false;
        }
        if self.from_black && self.held < DAWN_HOLD_SECONDS {
            self.held += dt;
            return false;
        }
        self.dark = (self.dark - dt / WAKING_SECONDS).max(0.0);
        self.dark <= 0.0 && std::mem::take(&mut self.from_black)
    }

    /// How opaque the black is, eased at both ends so the fade neither
    /// starts nor stops with a step.
    pub fn darkness(&self) -> f32 {
        let d = self.dark.clamp(0.0, 1.0);
        d * d * (3.0 - 2.0 * d)
    }

    /// Whether the sleeper has lain in the dark long enough that the night
    /// would have passed if everybody were asleep. See `LONELY_AFTER_SECONDS`.
    pub fn waiting(&self) -> bool {
        self.asleep && self.asleep_for >= LONELY_AFTER_SECONDS
    }

    /// What of this the interface draws, for `UiKey`: nothing while the
    /// screen is clear, and otherwise how dark, whether the lines on the
    /// black are up, and which lines.
    pub fn ui_key(&self) -> Option<(u8, bool, bool)> {
        (self.dark > 0.0).then(|| {
            (
                (self.darkness() * 255.0).round() as u8,
                self.asleep && self.dark >= 1.0,
                self.waiting(),
            )
        })
    }
}

/// Whether a movement key or the jump should ask the server to get the
/// player up.
///
/// **Only a key pressed while resting.** A player walks to a bed holding
/// the key that walks, and clicks it; the posture arrives with that key
/// still down, and the step they were already taking asked to get up on the
/// first frame they lay there. Half of "the player stands straight up": the
/// server stood them up at dawn, and the client did not even need a dawn. A
/// key has to be let go of, once, after the body is resting, before it
/// counts.
///
/// Once asked, not again until the posture changes -- a held key would
/// otherwise send "get up" every frame while the server's answer is on its
/// way.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rising {
    /// The keys have been let go of since the body last came to rest.
    armed: bool,
    /// "Get up" has been sent for this rest.
    sent: bool,
}

impl Rising {
    /// One frame: whether to send `ClientMessage::StandUp` now. `asked` is
    /// whether a movement key or the jump is down and the controls are the
    /// player's (no screen open, not typing).
    pub fn ask(&mut self, resting: Resting, asked: bool) -> bool {
        if resting == Resting::Standing {
            *self = Rising::default();
            return false;
        }
        if !asked {
            self.armed = true;
            return false;
        }
        if !self.armed || self.sent {
            return false;
        }
        self.sent = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seated_eye_is_lower_than_a_standing_one_and_above_the_seat() {
        // The bug: a player "sitting" on a stool saw the world from
        // standing height, because nothing but the server knew they sat.
        let feet = Vec3::new(3.5, 10.5, 7.5);
        let standing = Resting::Standing.eye(feet.as_dvec3());
        let seated = Resting::Sitting { facing: None }.eye(feet.as_dvec3());
        assert!(seated.y < standing.y - 0.5, "sitting did not lower the eye");
        assert!(seated.y > f64::from(feet.y + 0.5), "the seated eye is down at the seat");
        assert_eq!((seated.x, seated.z), (f64::from(feet.x), f64::from(feet.z)));
    }

    #[test]
    fn a_lying_eye_is_at_the_pillow_and_looks_toward_the_foot() {
        // A sleeper's body is laid across the middle of a two-cell bed; the
        // eye has to be at the head end of it, low, and the view along the
        // bed toward the foot -- the direction opposite the head.
        let feet = Vec3::new(10.0, 64.375, 5.5);
        for quarter in 0..4 {
            let head_yaw = quarter as f32 * std::f32::consts::FRAC_PI_2;
            let lying = Resting::Lying { head_yaw };
            let eye = lying.eye(feet.as_dvec3());
            let toward_head = Vec3::new(head_yaw.cos(), 0.0, head_yaw.sin());
            assert!((eye.as_vec3() - feet).dot(toward_head) > 0.5, "the eye is not at the head end");
            assert!(eye.y - f64::from(feet.y) < 0.5, "a lying eye at standing height");
            let (yaw, _) = lying.look_on_lying_down().unwrap();
            let looking = Vec3::new(yaw.cos(), 0.0, yaw.sin());
            assert!(looking.dot(toward_head) < -0.99, "the sleeper looks at the headboard");
        }
        assert_eq!(Resting::Sitting { facing: None }.look_on_lying_down(), None);
    }

    #[test]
    fn sitting_in_a_chair_faces_the_chair_and_sitting_on_a_stool_turns_nothing() {
        // A chair is put down facing something, and sitting in it looks
        // there. A stool has no front: the yaw the server sends with it is
        // the player's own, already stale by the time it arrives, and
        // turning the camera to it would be a jolt for nothing.
        use primitive_shared::types::{faced, seat_yaw, Facing, BLOCK_CHAIR, BLOCK_STOOL};
        let chair = faced(BLOCK_CHAIR, Facing::East);
        let yaw = seat_yaw(chair).expect("a chair faces a way");
        let seated = Resting::from_wire(Posture::Sitting, yaw, Some(chair));
        assert_eq!(seated.face_on_sitting_down(), Some(yaw));
        assert!(seated.is_sitting());
        let on_stool = Resting::from_wire(Posture::Sitting, 1.25, Some(BLOCK_STOOL));
        assert_eq!(on_stool.face_on_sitting_down(), None, "a stool turned the view");
        assert!(on_stool.is_sitting());
        assert_eq!(Resting::from_wire(Posture::Standing, yaw, Some(chair)).face_on_sitting_down(), None);
        // East is +x, which is a yaw of nought in the camera's convention.
        assert!(yaw.abs() < 1e-6, "an east-facing chair is sat in facing {yaw}");
    }

    const FRAME: f32 = 1.0 / 60.0;

    #[test]
    fn the_night_passes_in_the_dark_and_the_dark_lifts_on_a_player_still_lying_in_bed() {
        // The whole sequence a sleeper sees, a frame at a time: lying down,
        // the screen going dark gradually, the night passing (on the
        // server's clock) behind a screen that is already black, the waking,
        // a moment's more dark, the dark lifting -- and the body still in
        // the bed until a key is pressed.
        use primitive_shared::body::{FALLING_ASLEEP_SECONDS, NIGHT_PASSES_AFTER_SECONDS};
        let lying = Resting::Lying { head_yaw: 0.0 };
        let (mut sleep, mut rising) = (Sleep::default(), Rising::default());
        assert_eq!(sleep.darkness(), 0.0);

        sleep.set_asleep(true);
        let (mut t, mut last) = (0.0, 0.0);
        while t < FALLING_ASLEEP_SECONDS * 0.5 {
            assert!(!sleep.tick(FRAME), "a sleeper going to sleep was told it is morning");
            assert!(!rising.ask(lying, false));
            t += FRAME;
            assert!(sleep.darkness() >= last, "the dark flickered on the way down");
            last = sleep.darkness();
        }
        assert!(
            sleep.darkness() > 0.05 && sleep.darkness() < 0.95,
            "half way through the fall the screen is at {}, not part way dark",
            sleep.darkness()
        );
        while t < NIGHT_PASSES_AFTER_SECONDS {
            sleep.tick(FRAME);
            t += FRAME;
        }
        assert_eq!(
            sleep.darkness(),
            1.0,
            "the server passes the night at {NIGHT_PASSES_AFTER_SECONDS} s and the screen is not black yet"
        );

        // The new hour, then the waking.
        sleep.set_asleep(false);
        sleep.tick(DAWN_HOLD_SECONDS * 0.5);
        assert_eq!(sleep.darkness(), 1.0, "the dark lifted on the frame the waking came, before the sky caught up");
        let mut frames = 0;
        while !sleep.tick(FRAME) {
            frames += 1;
            assert!(frames < 600, "the dark never lifted, or lifted without the morning being said");
        }
        assert_eq!(sleep.darkness(), 0.0);
        assert!(!sleep.tick(FRAME), "the morning was said twice");

        // Still lying: nothing here gets anybody up until they ask.
        assert!(!rising.ask(lying, false));
        assert!(rising.ask(lying, true), "a step in the morning did not ask to get up");
        assert!(!rising.ask(lying, true), "a held key asked to get up every frame");
        assert!(!rising.ask(Resting::Standing, true));
    }

    #[test]
    fn a_key_still_held_from_walking_to_the_bed_does_not_get_the_player_straight_back_up() {
        // The client's half of "при сне игрок сразу встает": the player walks
        // up holding forward and clicks the bed, and the key they were
        // already holding asked to get up on the first frame they lay there.
        for resting in [Resting::Lying { head_yaw: 1.0 }, Resting::Sitting { facing: None }] {
            let mut rising = Rising::default();
            for _ in 0..30 {
                assert!(!rising.ask(resting, true), "a key held since before {resting:?} got the player up");
            }
            assert!(!rising.ask(resting, false));
            assert!(rising.ask(resting, true), "a fresh press after letting go did not get them up");
        }
    }

    #[test]
    fn getting_up_before_the_screen_is_black_lifts_the_dark_at_once_and_is_not_a_morning() {
        let mut sleep = Sleep::default();
        sleep.set_asleep(true);
        sleep.tick(primitive_shared::body::FALLING_ASLEEP_SECONDS * 0.4);
        let partway = sleep.darkness();
        sleep.set_asleep(false);
        sleep.tick(FRAME);
        assert!(sleep.darkness() < partway, "a waking that interrupted the fall was held in the dark");
        let mut morning = false;
        for _ in 0..300 {
            morning |= sleep.tick(FRAME);
        }
        assert_eq!(sleep.darkness(), 0.0);
        assert!(!morning, "getting straight back up was greeted as the morning");
    }

    #[test]
    fn a_sleeper_is_told_the_night_is_waiting_on_the_others_only_once_it_should_have_passed() {
        use primitive_shared::body::NIGHT_PASSES_AFTER_SECONDS;
        let mut sleep = Sleep::default();
        sleep.set_asleep(true);
        sleep.tick(NIGHT_PASSES_AFTER_SECONDS);
        assert!(!sleep.waiting(), "a lone sleeper was told somebody is awake before the night could pass");
        sleep.tick(LONELY_AFTER_SECONDS);
        assert!(sleep.waiting());
        sleep.set_asleep(false);
        assert!(!sleep.waiting(), "a waking sleeper is still waiting for the night");
    }
}
