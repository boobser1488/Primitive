//! What the *game* sounds like, as opposed to what the speaker does.
//!
//! [`Audio`] can play a sound. This decides which sounds a world makes
//! while nobody is doing anything in particular: the footfalls, the fire
//! two rooms away, the rain on the roof, and which mood the composer
//! should be in.
//!
//! ## Why it is a module and not six fields in the frame loop
//!
//! Almost everything in here is a *timer with an opinion*. A footstep is
//! not an event the game already has -- there is no "the player took a
//! step" anywhere, because walking is a continuous change of position --
//! so it has to be reconstructed from distance travelled. The same is
//! true of swimming strokes, of the rhythm a pick makes, of a fire
//! crackling and of rain. Six of those live here rather than as six
//! more `let mut` in a four-thousand-line `run`.
//!
//! The events that *are* events -- a block broken, a hit taken, a menu
//! clicked -- are played straight from where they happen. There is
//! nothing for this module to add to them, and routing them through it
//! would only put distance between the cause and the sound.
//!
//! ## The rule about the server
//!
//! Nothing here decides anything. A footstep is played because the
//! player has moved, and the player has moved because physics said so.
//! If the client is wrong about where it is, the sound is wrong in
//! exactly the same way and for exactly one round trip, which is the
//! correct amount of wrong.

use glam::Vec3;

use primitive_shared::animals::Species;
use primitive_shared::horse::Gait;
use primitive_shared::worldgen::Biome;
use primitive_shared::raft;
use primitive_shared::protocol::EntityId;
use primitive_shared::types::{self, BlockId, CHUNK_SIZE_Y};
use primitive_shared::weather::Weather;

use super::bank::{self, crumble_of, voice_of, Cry, Footing, Impact, Material, Sfx};
use super::music::Mood;
use super::Audio;
use crate::engine::camera::Camera;
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::logic::physics::Player;
use crate::audio::clip::Rng;

/// How far a player walks between footfalls, in blocks.
///
/// Measured off the animation rather than chosen: the view bob in
/// `logic::shake` is a cycle of about this length, so a step lands with
/// the bottom of the bob. Getting this wrong is very noticeable and very
/// hard to name -- it reads as the sound being slightly late.
///
/// **One number with the legs of everybody else**, `player_model::STEP_BLOCKS`:
/// the figure's stride used to be its own 2.2 read upside down, and walked
/// ten steps to every footfall.
const STRIDE: f32 = crate::logic::player_model::STEP_BLOCKS;

/// Below this there is no walking going on, only drift and
/// collision-resolution jitter.
const WALK_THRESHOLD: f32 = 0.6;

/// The quietest a footfall gets, as a share of a sprint's.
///
/// A quarter. There is no crouch key in this game, so the only way to
/// move quietly is to move slowly, and this is what that buys: a creep
/// against a sprint is two full stops of difference, which is about what
/// it is in a room. Silence was rejected -- a player who cannot hear
/// their own feet cannot tell walking from being stuck against a wall.
const CREEP_EFFORT: f32 = 0.25;

/// How often a foot finds a dry stick where the canopy is closed, and
/// how loud that is against a whole branch coming down.
///
/// A fifth of the steps under a full canopy, scaled by how much of the
/// survey came back leaves -- so a meadow never cracks and a thicket
/// does most of the time. Loud enough to be startling and quiet enough
/// not to sound like the player broke something: this is a *fifth* of
/// the gain the same clip gets when a tree is felled.
const TWIG_CHANCE: f32 = 0.2;
const TWIG_GAIN: f32 = 0.22;

/// Seconds between one swimming stroke and the next, least and most.
///
/// **A stroke has to finish before the next one starts.** It was 0.55 to
/// 0.8 s, with a splash effect under half a second long: three slaps
/// every two seconds, which reads as a machine gun rather than as arms.
/// The strokes are recordings of a person swimming now, up to a second
/// each, and a breaststroke or a crawl arm is about a second and a
/// quarter -- so the gap is that, and never shorter than the longest
/// stroke (`recorded` holds the clips to it).
pub(super) const SWIM_GAP: (f32, f32) = (1.05, 1.45);

// The rhythm of a swing that is still going is the arm's own -- see
// `Soundscape::digging`. It is deliberately not tied to the mining
// progress: a block that takes six seconds and one that takes a quarter
// of one are the same arm.
//
// It was a constant of its own here, 0.26 against an arm at 0.28, so the
// knock walked a fiftieth of a second further off the blow with every
// swing and was landing on the backswing within seven -- a rhythm that
// sounds and looks out of step without anybody being able to say which
// is wrong. Then it was `hand::SWING_SECONDS`, which was right until a
// blow's length started depending on what is in the hand. It is
// `hand::dig_seconds(held)` now, which is the same number the arm is
// actually drawn at.

/// Landing softer than this makes no sound at all -- it is walking off a
/// kerb.
const LANDING_THRESHOLD: f32 = 3.4;

/// The speed a landing is at its loudest. Above it the sound stops
/// getting worse and the damage takes over.
const LANDING_FULL: f32 = 14.0;

/// How far to look for something burning.
const FIRE_RANGE: i32 = 8;

/// Height above the player's head that counts as "there is a roof".
const ROOF_PROBE: i32 = 48;

/// How close a fire has to be to be the fire you are sitting at.
///
/// Five blocks rather than [`FIRE_RANGE`]'s eight: a fire you can hear
/// through a wall is atmosphere, and a fire you are sitting at is a
/// room. The music only agrees to be a hearth for the second one.
const HEARTH_RANGE: f32 = 5.0;

/// How much of a ring of probes has to come back water before the
/// player counts as being out on it, and leaves before the player
/// counts as being under them. See `look_around`.
///
/// **Both are well over half on purpose.** A pond in a meadow and a
/// copse beside a field are not a sea and a forest, and a mood that
/// triggers on either would leave a player who cannot work out why the
/// music keeps changing.
const AT_SEA: f32 = 0.65;
const IN_FOREST: f32 = 0.6;

/// Seconds between one look at what is around the player and the next.
const SURVEY_INTERVAL: f32 = 1.5;

/// One animal, as the soundscape hears it: who, what, where, and how fast.
///
/// Built by the frame loop out of the entity list (`Entities::heard`) and
/// handed in, for the reason `Frame` borrows rather than reaches: this
/// module decides what is heard and nothing about what exists.
#[derive(Debug, Clone, Copy)]
pub struct Heard {
    pub id: EntityId,
    pub species: Species,
    pub at: glam::DVec3,
    pub speed: f32,
    /// The flash of a blow, as the server sends it: above zero for a
    /// moment after every hit. What a wound is heard from -- see [`Voices`].
    pub hurt: f32,
    /// Low and circling: a hunter walking the ring round somebody, or round
    /// the edge of a fire's light (`protocol::Attitude::Stalking`). What
    /// makes a night hunter howl close -- see [`STALKING_HOWL`].
    pub stalking: bool,
}

/// Is a bird in the air, from its speed and whether it was a moment ago?
///
/// **Two thresholds either side of the model's own**
/// (`animal_model::AIRBORNE_SPEED`), because one flickers: a bird coming in
/// to land slows through it, and a single line would play its take-off
/// every time a snapshot put its speed a hair either side.
pub fn in_the_air(speed: f32, was: bool) -> bool {
    let line = crate::logic::animal_model::AIRBORNE_SPEED;
    if was {
        speed > line * 0.6
    } else {
        speed > line * 1.4
    }
}

/// Everything a frame needs to tell the soundscape.
///
/// A struct rather than fourteen arguments, and borrowed rather than
/// copied, because most of it is the world.
pub struct Frame<'a> {
    pub dt: f32,
    pub player: &'a Player,
    pub camera: &'a Camera,
    pub chunks: &'a ChunkManager,
    pub sky: &'a Sky,
    pub weather: Weather,
    /// 0..1. Drives [`Mood::Peril`] and nothing else.
    pub health_fraction: f32,
    /// False on the menus, where there is no world to make noises.
    pub in_world: bool,
    /// The cell currently coming apart under the crosshair, if the
    /// player is actually swinging at it.
    pub digging: Option<(i32, i32, i32)>,
    /// The button is down and there is nothing under it. A miss is a
    /// different sound from a blow, and a player who cannot hear the
    /// difference thinks the game is ignoring them.
    pub swinging: bool,
    /// What is in the hand.
    ///
    /// **Two things read it, and both of them are the blow.** A heavy
    /// head swings slower (`hand::dig_seconds`), so the knock has to
    /// keep the arm's time rather than a constant's; and the blow itself
    /// is two materials meeting, not one -- a copper axe on stone is a
    /// different noise from a flint one, and before this the tool was
    /// silent and only the rock was heard.
    pub held: Option<BlockId>,
}

/// The timers, and what they were last frame.
pub struct Soundscape {
    /// Health lost since the last hurt sound, and when that sound was. See
    /// `on_hurt`.
    hurt_owed: f32,
    hurt_at: Option<std::time::Instant>,
    /// Distance walked since the last footfall.
    walked: f32,
    was_grounded: bool,
    /// How hard the player arrived at the floor this frame, 0..1, if
    /// they did -- see [`Soundscape::take_landing`].
    landing: Option<f32>,
    /// The fastest the player was falling while airborne. Reset on
    /// landing, which is what makes a landing's loudness the height of
    /// the fall rather than the speed at the moment of contact -- those
    /// differ by a frame of the collision response, and the difference
    /// is most of the impact.
    fall_speed: f32,
    was_submerged: bool,
    was_in_water: bool,

    dig_left: f32,
    swim_left: f32,
    bubble_left: f32,
    fire_left: f32,
    /// Seconds until the next piece of the rain bed is laid, and until the
    /// next drip under shelter.
    rain_left: f32,
    drip_left: f32,
    /// Which bed the last piece of rain was, so walking under a roof hands
    /// over at once rather than a whole piece later.
    rain_shelter: Shelter,
    wind_left: f32,
    thunder_left: f32,
    /// Bolts that have flashed and not yet been heard: where each one
    /// fell, and how long the noise still has to travel
    /// (`lightning::thunder_delay`).
    ///
    /// **The crack is the distance**, and it is the only part of the
    /// mechanic a player can measure. The flash is instant and the sound
    /// is three seconds to the kilometre, so counting between the two is
    /// how far off the storm is -- a fact nobody has to be told and
    /// everybody already knows. A bolt played the moment the message
    /// landed would throw that away and, worse, would make every strike
    /// sound as though it were overhead.
    cracks: Vec<(glam::DVec3, f32)>,
    /// Seconds until the next frog in the chorus calls.
    frog_left: f32,
    /// What every animal near the player is saying -- see [`Voices`].
    voices: Voices,
    /// The player pushing through leaves, and each animal doing it.
    brush: Brush,
    brushes: Vec<(EntityId, Brush)>,
    /// Where the ears are and whether it is dark, as of this frame's
    /// `update` -- for `wildlife`, which is called after it.
    ear: glam::DVec3,
    night: bool,
    /// ...and until the hum of the bees round a hive is heard again.
    bee_left: f32,
    /// ...and until the next flutter of a bat's wings.
    bat_left: f32,
    /// Which birds near the player were in the air last frame -- what a
    /// take-off is read from. See `wildlife`.
    aloft: Vec<(EntityId, bool)>,
    /// Seconds until the next piece of the crickets' bed, how far round the
    /// player they have fallen silent, and where the ears were when that
    /// was last asked. See `cricket_chorus` and `cricket_hush`.
    cricket_left: f32,
    hush: f32,
    cricket_ear: glam::DVec3,
    /// The sun's height and whether it is raining, as of this frame's
    /// `update` -- for the crickets in `wildlife`.
    sun: f32,
    wet: bool,
    /// ...and whether it is before noon, and how old the world is, which
    /// is where the season comes from. See [`Air`].
    morning: bool,
    world_days: f32,

    /// The three beds the weather is not: the cicadas in the heat, the
    /// birds at either end of the day, and the water off the eaves in a
    /// thaw. Each is a level that is only ever eased ([`Level`]) and a
    /// timer that lays pieces or calls against it.
    cicada: Level,
    cicada_left: f32,
    birds: Level,
    bird_left: f32,
    thaw: Level,
    thaw_left: f32,
    /// Every horse's feet near the player. See [`Hoofbeats`].
    hooves: Hoofbeats,

    /// Whether the sky is visible from where the player stands, and when
    /// that was last established. Answering it costs a column of block
    /// lookups, and it cannot change in less than the time it takes to
    /// walk out of a cave.
    outdoors: bool,
    /// ...and, established with it, how the rain reaches the player.
    shelter: Shelter,
    roof_left: f32,

    /// What the last look around found: how much of the ring came back
    /// water, how much of it came back leaves, and whether there is a
    /// fire close enough to be sitting at. See `look_around`.
    water_share: f32,
    leaf_share: f32,
    fire_near: bool,
    survey_left: f32,

    /// What the music is being asked for, and how long the *candidate*
    /// mood has held. A mood is not sent until it has been true for a
    /// moment: walking under an overhang is not a cave, and one bad hit
    /// is not peril.
    mood: Mood,
    candidate: Mood,
    candidate_held: f32,

    rng: Rng,
}

impl Default for Soundscape {
    fn default() -> Self {
        Self::new()
    }
}

/// The least time between two hurt sounds, in seconds: about as fast as a
/// wolf bites, and slower than any drain ticks.
const HURT_GAP_SECONDS: f32 = 0.7;

/// A single loss this big is a blow, and is heard without waiting for a
/// whole point to be owed.
const HURT_BLOW: f32 = 0.5;

/// How loud a quarter of a block coming away is, against the 0.45 the same
/// crumble plays at under a whole block breaking (`on_block_broken`): a
/// little under it, because a quarter is less debris -- and not much under
/// it, because here there is no break on top to be heard with, and a
/// crumble at a quarter of a break's weight was lost under the swing's own
/// knock. See `Soundscape::on_slice_taken`.
const SLICE_GAIN: f32 = 0.38;

impl Soundscape {
    pub fn new() -> Soundscape {
        Soundscape {
            hurt_owed: 0.0,
            hurt_at: None,
            walked: 0.0,
            was_grounded: true,
            landing: None,
            fall_speed: 0.0,
            was_submerged: false,
            was_in_water: false,
            dig_left: 0.0,
            swim_left: 0.0,
            bubble_left: 0.0,
            fire_left: 0.0,
            rain_left: 0.0,
            drip_left: 0.0,
            rain_shelter: Shelter::Open,
            wind_left: 0.0,
            thunder_left: 0.0,
            cracks: Vec::new(),
            frog_left: 0.0,
            voices: Voices::new(),
            brush: Brush::new(),
            brushes: Vec::new(),
            ear: glam::DVec3::ZERO,
            night: false,
            bee_left: 0.0,
            bat_left: 0.0,
            aloft: Vec::new(),
            cricket_left: 0.0,
            hush: CRICKET_NEAREST,
            cricket_ear: glam::DVec3::ZERO,
            sun: 1.0,
            wet: false,
            morning: true,
            world_days: 0.0,
            cicada: Level::default(),
            cicada_left: 0.0,
            birds: Level::default(),
            bird_left: 0.0,
            thaw: Level::default(),
            thaw_left: 0.0,
            hooves: Hoofbeats::new(),
            outdoors: true,
            shelter: Shelter::Open,
            roof_left: 0.0,
            water_share: 0.0,
            leaf_share: 0.0,
            fire_near: false,
            survey_left: 0.0,
            mood: Mood::Menu,
            candidate: Mood::Menu,
            candidate_held: 0.0,
            rng: Rng::new(0xF007_5735),
        }
    }

    /// Everything a frame of standing about makes noise about.
    pub fn update(&mut self, audio: &Audio, frame: &Frame<'_>) {
        // The listener first, so anything played below is placed against
        // where the camera is *now* rather than where it was last frame.
        audio.set_listener(frame.camera.eye(), frame.camera.right_horizontal());
        audio.set_submerged(frame.in_world && frame.player.submerged);
        self.ear = frame.camera.eye();
        self.night = frame.sky.sun_elevation() <= 0.0;
        self.sun = frame.sky.sun_elevation();
        self.wet = frame.weather.is_wet();
        // 0.5 is noon, so anything under it is on the way up. Read from
        // the clock rather than from the sun's height changing between
        // frames, which would answer "morning" for a whole frame after
        // every `TimeSync` eased the clock backwards.
        self.morning = frame.sky.time_of_day < 0.5;
        self.world_days = frame.sky.world_days();

        if !frame.in_world {
            self.push_mood(audio, Mood::Menu, frame.dt);
            // Everything below is about a body in a world. The menus get
            // the music and nothing else.
            self.was_grounded = true;
            self.fall_speed = 0.0;
            return;
        }

        self.roof_left -= frame.dt;
        if self.roof_left <= 0.0 {
            self.shelter = shelter_at(frame.chunks, frame.player.position.as_vec3());
            self.outdoors = matches!(self.shelter, Shelter::Open | Shelter::Canopy);
            self.roof_left = 0.45;
        }

        self.survey_left -= frame.dt;
        if self.survey_left <= 0.0 {
            self.look_around(frame);
            self.survey_left = SURVEY_INTERVAL;
        }

        self.body(audio, frame);
        self.digging(audio, frame);
        self.ambience(audio, frame);
        self.push_mood(audio, mood_for(self.surroundings(frame)), frame.dt);
    }

    // ------------------------------------------------------------- body

    /// Footsteps, jumping, landing, and being in water.
    fn body(&mut self, audio: &Audio, frame: &Frame<'_>) {
        let player = frame.player;
        let feet = player.position;
        let horizontal = player.velocity.with_y(0.0).length();

        // ---- entering and leaving water ----
        if player.in_water && !self.was_in_water {
            // Loud in proportion to how hard the surface was hit. Wading
            // in off a beach should not sound like a dive.
            let speed = self.fall_speed.max(horizontal);
            let gain = (0.25 + speed / 14.0).min(1.0);
            audio.play_flat(Sfx::Splash, gain, self.jitter(0.08));
            self.fall_speed = 0.0;
        }
        self.was_in_water = player.in_water;
        self.was_submerged = player.submerged;

        if player.submerged {
            // Air, leaving. Irregular on purpose: a bubble every 1.4
            // seconds exactly is a metronome.
            self.bubble_left -= frame.dt;
            if self.bubble_left <= 0.0 {
                audio.play_flat(Sfx::Bubble, self.rng.range(0.4, 0.8), self.jitter(0.2));
                self.bubble_left = self.rng.range(1.1, 3.2);
            }
        } else {
            self.bubble_left = self.bubble_left.min(1.5);
        }

        if player.swimming {
            self.swim_left -= frame.dt;
            if self.swim_left <= 0.0 && horizontal > 0.3 {
                // Harder strokes for a swimmer going somewhere; a paddle
                // to stay put is barely a sound.
                let effort = (horizontal / 3.0).clamp(0.5, 1.0);
                audio.play_flat(Sfx::Swim, 0.5 * effort, self.jitter(0.08));
                self.swim_left = self.rng.range(SWIM_GAP.0, SWIM_GAP.1);
            }
            // A swimmer has no feet on anything.
            self.walked = 0.0;
            self.was_grounded = false;
            return;
        }

        // ---- pushing through leaves ----
        //
        // By distance moved through them, like a footstep, and at any
        // height: dropping through a canopy brushes it as much as walking
        // into a bush does.
        let speed = player.velocity.length();
        let thickness = foliage_around(frame.chunks, feet.as_vec3(), 1.8);
        if let Some(gain) = self.brush.advance(speed * frame.dt, speed, thickness) {
            audio.play_flat(Sfx::Rustle, 0.8 * gain, self.jitter(0.08));
        }

        // ---- jumping and falling ----
        if player.jumped {
            audio.play_flat(Sfx::Jump, 0.5, self.jitter(0.1));
        }
        if !player.grounded {
            self.fall_speed = self.fall_speed.max(-player.velocity.y);
        }

        if player.grounded && !self.was_grounded {
            let material = self.ground_material(frame.chunks, feet.as_vec3());
            if self.fall_speed >= LANDING_THRESHOLD {
                let hardness = ((self.fall_speed - LANDING_THRESHOLD)
                    / (LANDING_FULL - LANDING_THRESHOLD))
                    .clamp(0.0, 1.0);
                // **The same number the view is jolted by.** The thump
                // and the jolt are one event, and letting the camera
                // decide for itself what counted as a hard landing would
                // be two thresholds to keep in step -- and a fall that
                // shook the screen without a sound under it.
                self.landing = Some(hardness);
                audio.play_flat(Sfx::Land, 0.35 + 0.65 * hardness, 1.0 - hardness * 0.2);
                // The ground gets a word in as well, so landing on
                // gravel and landing on snow are not the same event with
                // one sound.
                audio.play_flat(
                    Sfx::Material(Impact::Step, material),
                    0.4 + 0.4 * hardness,
                    self.jitter(0.1),
                );
            } else if self.fall_speed > 1.0 {
                // A hop, or a step off something low.
                audio.play_flat(
                    Sfx::Material(Impact::Step, material),
                    0.45,
                    self.jitter(0.1),
                );
            }
            self.fall_speed = 0.0;
            // Landing is a footfall, so the next one is a full stride
            // away rather than immediately.
            self.walked = 0.0;
        }
        self.was_grounded = player.grounded;

        // ---- walking ----
        if player.grounded && horizontal > WALK_THRESHOLD {
            self.walked += horizontal * frame.dt;
            if self.walked >= STRIDE {
                self.walked -= STRIDE;
                let material = self.ground_material(frame.chunks, feet.as_vec3());
                let sfx = stride_sound(player.in_water, material);
                // Quieter when creeping, louder at a run: the same clip,
                // and the difference is entirely in these two numbers.
                //
                // **There is no crouch key in this game** (see
                // `physics`), so creeping *is* walking slowly, and the
                // only way a player can be quiet is to go slowly. That
                // makes this the whole of the mechanic rather than a
                // detail of it: the floor was 0.4 of a sprint's loudness
                // and it is a quarter now, measured against the sprint
                // (`animals::NOMINAL_SPRINT_SPEED`) rather than against
                // a 6.0 nobody could say the origin of.
                let effort = (horizontal / primitive_shared::animals::NOMINAL_SPRINT_SPEED)
                    .clamp(CREEP_EFFORT, 1.15);
                audio.play_flat(sfx, 0.55 * effort, self.jitter(0.12));
                self.underfoot(audio, material, effort);
            }
        } else {
            // Stopping resets the stride, so setting off again starts
            // with a step rather than finishing the one from before.
            self.walked = self.walked.min(STRIDE * 0.5);
        }
    }

    /// The small thing that goes under the boot with the step.
    ///
    /// **Why a step needs a second sound at all.** A footfall is one
    /// clip per material, and twenty minutes of walking is that clip
    /// several thousand times; what makes a wood sound like a wood
    /// rather than like a loop is that now and then something *else*
    /// happens -- a dry stick gives, a stem is pushed aside. Neither is
    /// a new recording: a twig is a very small branch breaking
    /// (`Impact::Break` on wood, at a fifth of the gain a felled tree
    /// gets), and the litter it lies in is the reason it only happens
    /// where there are trees.
    ///
    /// **Read off `leaf_share`**, which the survey already measures for
    /// the wind and the music -- a player in a meadow never steps on a
    /// branch, and one under a canopy does about every fifth step.
    /// Rejected: a block lookup for deadfall under the foot. There is no
    /// such block, and adding one to carry a sound would be a change to
    /// the world for a change to the ear.
    fn underfoot(&mut self, audio: &Audio, ground: Material, effort: f32) {
        // Only what a wood's floor is made of. A branch does not lie on
        // rock, in sand or in a stream, and one snapping on a plank
        // floor is a board giving way, which is a different report.
        if !matches!(ground, Material::Grass | Material::Dirt | Material::Snow) {
            return;
        }
        if self.rng.range(0.0, 1.0) > TWIG_CHANCE * self.leaf_share {
            return;
        }
        // A twig under a creeping foot still cracks -- that is the
        // point of it, and the reason it is not scaled all the way down
        // by the effort: the one sound a player cannot be quiet about.
        audio.play_flat(
            Sfx::Material(Impact::Break, Material::Wood),
            TWIG_GAIN * (0.7 + 0.3 * effort),
            self.jitter(0.25),
        );
    }

    /// How hard the player arrived at the floor this frame, 0..1, taken
    /// once.
    ///
    /// **Here rather than in the frame loop** because the thresholds a
    /// landing is measured against are this module's
    /// ([`LANDING_THRESHOLD`] and [`LANDING_FULL`]), and the thump and
    /// the jolt have to be one event. The frame loop hands it to
    /// `Shake::on_landing` and does not otherwise know what a hard
    /// landing is.
    pub fn take_landing(&mut self) -> Option<f32> {
        self.landing.take()
    }

    // ---------------------------------------------------------- digging

    /// The rhythm of a swing that has not finished yet, whether or not
    /// it is hitting anything.
    fn digging(&mut self, audio: &Audio, frame: &Frame<'_>) {
        // **The arm's own clock, and not a constant beside it.** A heavy
        // head swings slower (`hand::dig_seconds`) and lands later in
        // its own blow (`hand::impact_at`); a knock at a fixed rate
        // would walk off the arm within a few swings of picking up an
        // iron pick, which is the same bug the note above `SWIM_GAP`
        // describes from the first time it happened.
        let blow = crate::logic::hand::dig_seconds(frame.held);
        if frame.digging.is_none() && !frame.swinging {
            // Ready to strike when the next swing *lands*, rather than up
            // to a quarter-second later -- and not the instant it starts,
            // which was a knock a twelfth of a second before the blow it
            // belongs to.
            self.dig_left = blow * crate::logic::hand::impact_at(frame.held);
            return;
        }
        self.dig_left -= frame.dt;
        if self.dig_left > 0.0 {
            return;
        }
        self.dig_left = blow;

        let Some(cell) = frame.digging else {
            // Air, or something a bare hand will never get through. The
            // arm still moves.
            audio.play_flat(Sfx::Swing, 0.4, self.jitter(0.12));
            return;
        };
        let Some(block) = frame.chunks.block_at(cell.0, cell.1, cell.2) else {
            return;
        };
        audio.play_at_block(
            Sfx::Material(Impact::Dig, Material::of(block)),
            cell,
            0.8,
            self.jitter(0.14),
        );
        // **...and what struck it.** A blow is two things meeting, and
        // only one of them was ever heard: a flint axe, a copper one and
        // a fist all made the same noise against the same log, which is
        // the whole of what a tool is, gone. The tool's own material is
        // played under the block's, quiet and a little lower -- see
        // `tool_voice` for why it is a second sound rather than a
        // different one.
        if let Some((tool, gain, pitch)) = tool_voice(frame.held) {
            audio.play_at_block(Sfx::Material(Impact::Dig, tool), cell, gain, pitch);
        }
    }

    // -------------------------------------------------------- ambience

    /// Fire, rain, wind and thunder.
    fn ambience(&mut self, audio: &Audio, frame: &Frame<'_>) {
        // ---- anything burning nearby ----
        //
        // **A bed, laid end over end**: each piece starts where the last
        // one's crossfade does (`bank::FIRE_SECONDS - FIRE_FADE`), so the
        // fire is one continuous sound rather than a crackle every second
        // or so -- which, with a clip of snaps, was a string of snaps with
        // gaps in it. No pitch jitter on top of the mixer's own three per
        // cent: a piece played slower is a piece that ends later than its
        // neighbour starts fading in.
        self.fire_left -= frame.dt;
        if self.fire_left <= 0.0 {
            if let Some(cell) = nearest_fire(frame.chunks, frame.player.position.as_vec3()) {
                audio.play_at_block(Sfx::FireCrackle, cell, 0.9, 1.0);
                self.fire_left = (self.fire_left + bank::FIRE_SECONDS - bank::FIRE_FADE).max(0.2);
            } else {
                self.fire_left = 1.0;
            }
        }

        // **Worked out here rather than handed in.** The wind is a pure
        // function of the world's age and the weather -- that is the
        // whole design of `raft::wind`, and the reason nothing about it
        // goes over the network -- and this module is already given
        // both of its arguments. A field on `Frame` carrying it would
        // be the frame loop passing in something the soundscape can
        // work out itself, and a second place for the two to disagree
        // about which wind it is. The renderer computes the same
        // number the same way, a few lines from where it draws the
        // rain, and the point of this change is that the ear and the
        // eye agree.
        let wind = raft::wind(frame.sky.world_days(), frame.weather).strength;

        // ---- the wind itself ----
        //
        // A gust now and then up on the surface, and only there: wind
        // inside a mine is a different game.
        //
        // **In the rain as well now.** This used to sit under an early
        // return taken whenever the weather was wet, so the one thing a
        // squall is actually made of was the one thing that could not
        // be heard during one.
        //
        // **Which of three winds is decided when it comes**, by
        // `wind_kind`: a breeze in the leaves, a gust over open ground, a
        // howl up high or in a storm. The one exception to "only outdoors"
        // is the howl, which a hut in a storm hears round its walls.
        self.wind_left -= frame.dt;
        if self.wind_left <= 0.0 {
            let storm = matches!(frame.weather, Weather::Storm);
            let height = frame.player.position.y;
            let kind = wind_kind(wind, height as f32, self.leaf_share, storm);
            let (gap, gain) = gusting(wind);
            let (gap, gain) = match kind {
                // Leaves stir oftener than a gust arrives, and softer.
                Sfx::WindBreeze => (gap * 0.6, gain * 0.8),
                Sfx::WindHowl => (gap, gain * 1.1),
                _ => (gap, gain),
            };
            self.wind_left = self.rng.range(gap * 0.7, gap * 1.4);
            if self.outdoors && height > 60.0 {
                audio.play_flat(kind, gain, self.jitter(0.1));
            } else if kind == Sfx::WindHowl && height > 50.0 && self.shelter == Shelter::Roofed {
                audio.play_flat(kind, gain * 0.35, self.jitter(0.1));
            }
        }

        // ---- the crack of a bolt that has already flashed ----
        //
        // **Above the dry-sky return**, and that is not tidiness: a
        // bolt at the tail of a squall flashes while it is still
        // raining and is heard three seconds later, which may be after
        // the sky has cleared -- and an operator's `/lightning` happens
        // under whatever sky there is. A crack that was dropped because
        // the weather had moved on would be a flash with no thunder,
        // which is the one thing a player would report as broken.
        let ear = frame.camera.eye();
        let jitter = self.jitter(0.05);
        self.cracks.retain_mut(|(at, left)| {
            *left -= frame.dt;
            if *left > 0.0 {
                return true;
            }
            let distance = (*at - ear).length() as f32;
            // Where it fell, not flat: a bolt is the one weather sound
            // that comes from somewhere, and a player who turns towards
            // it is turning towards the fire it started.
            audio.play_at(
                Sfx::Thunder,
                *at,
                primitive_shared::lightning::thunder_gain(distance),
                jitter,
            );
            false
        });

        if !frame.weather.is_wet() {
            self.rain_left = 0.0;
            return;
        }

        let intensity = frame.weather.intensity();
        let rainfall = raining(intensity, wind);

        // ---- rain ----
        //
        // **A bed, not drops.** This was a few dozen thirty-millisecond
        // ticks a second placed round the player, defended here as "what
        // rain *is*" -- and what the player heard was a click at every
        // drop. The rain is now pieces of a continuous band of noise laid
        // end over end (see `bank::rain`), one bed under the sky and a
        // duller one under a roof, and what thins it under shelter is
        // which bed plays and how loud, plus the odd drip off the eaves.
        // Deep in a cave, nothing: rain you cannot see the way out to is
        // not rain you hear.
        let heard = rain_heard(self.shelter);
        if self.shelter != self.rain_shelter {
            // Walking in under a roof: hand over now, not a piece later.
            // The new piece fades in over the old one's tail.
            self.rain_left = self.rain_left.min(0.15);
            self.rain_shelter = self.shelter;
        }
        self.rain_left -= frame.dt;
        if self.rain_left <= 0.0 {
            self.rain_left = (self.rain_left + bank::BED_SECONDS - bank::BED_FADE).max(0.2);
            if let Some((bed, loudness, _)) = heard {
                audio.play_flat(bed, rainfall.gain * loudness, 1.0);
            }
        }
        if let Some((_, _, drips)) = heard.filter(|(_, _, drips)| *drips > 0.0) {
            self.drip_left -= frame.dt;
            if self.drip_left <= 0.0 {
                self.drip_left = self.rng.range(0.4, 1.6) / (rainfall.drips * drips).max(0.05);
                let angle = self.rng.range(0.0, std::f32::consts::TAU);
                let distance = self.rng.range(1.5, 6.0);
                let at = frame.camera.eye()
                    + Vec3::new(angle.cos() * distance, self.rng.range(-1.5, 1.0), angle.sin() * distance).as_dvec3();
                audio.play_at(Sfx::RainTick, at, rainfall.gain * 0.8, self.jitter(0.2));
            }
        }

        // ---- thunder ----
        //
        // Storms only. Rain that thunders every twenty seconds is a
        // storm the weather system did not agree to.
        self.thunder_left -= frame.dt;
        if self.thunder_left <= 0.0 {
            self.thunder_left = self.rng.range(18.0, 55.0);
            if matches!(frame.weather, Weather::Storm) {
                // Muffled indoors rather than absent: a roof is not a
                // reason not to hear thunder.
                let gain = if self.outdoors { 0.85 } else { 0.45 };
                audio.play_flat(Sfx::Thunder, gain, self.jitter(0.1));
            }
        }
    }

    // -------------------------------------------------------- wildlife

    /// What the animals and the small life near the player sound like.
    ///
    /// **Its own call rather than more of `Frame`**, because only the frame
    /// loop in a world has animals to hand it, and the menu's soundscape --
    /// which builds a `Frame` too -- has none.
    ///
    /// Three things, each read off what the client already has:
    ///
    /// * **a bird going up** is a bird whose speed crossed into flight
    ///   since last frame (`in_the_air`). It is the sound of a covey
    ///   flushed by a wolf the player has not seen -- the server now puts
    ///   every bird up off anything hostile -- so it is played wherever the
    ///   bird is, not only where the player is looking;
    /// * **every animal's voice** -- a sheep bleating, a boar grunting as it
    ///   roots, a deer's blow as it bolts, a wolf's snarl as it comes, the
    ///   squeal of a wound and the last cry of a kill -- is decided by
    ///   [`Voices`], from nothing but what was drawn. The gulls' calls are
    ///   in there now too, as a gull's calm voice;
    /// * **something pushing through a bush** is heard from wherever it is,
    ///   by the same measure as the player's own (`Brush`);
    /// * **the frogs** call from wherever `Critters` says one is calling --
    ///   which is nowhere within earshot of a wolf. The silence is decided
    ///   there, not here;
    /// * **bees** hum from wherever one is flying round a hive. A clip of
    ///   swarm laid over itself a little before the last one ends, from a
    ///   bee picked at random, so the hum is where the cloud is and a robbed
    ///   hive -- twice the bees -- is heard as the louder one;
    /// * **bats** flutter from wherever one is on the wing;
    /// * **crickets** sing in the grass on a warm night, from wherever the
    ///   chorus is -- which is not within a few steps of a player who is
    ///   moving (`cricket_chorus`, `cricket_hush`). `air` is the country
    ///   under the player and how warm it is now, from `Critters`, which
    ///   already asks the generator for the frogs;
    /// * **hooves** drum under every horse that is going somewhere, ridden
    ///   or not ([`Hoofbeats`]).
    ///
    /// One slice per kind of small life rather than a struct of them: each is
    /// a list `Critters` hands over as it is, and a struct built only to pass
    /// them in would be a type with one caller.
    #[allow(clippy::too_many_arguments)]
    pub fn wildlife(
        &mut self,
        audio: &Audio,
        dt: f32,
        chunks: &ChunkManager,
        animals: &[Heard],
        croaking: &[Vec3],
        buzzing: &[Vec3],
        flapping: &[Vec3],
        air: Option<(Biome, f32)>,
    ) {
        let mut aloft = Vec::with_capacity(self.aloft.len());
        for animal in animals.iter().filter(|a| a.species.flies()) {
            let was = self.aloft.iter().find(|(id, _)| *id == animal.id).map(|&(_, up)| up);
            let up = in_the_air(animal.speed, was.unwrap_or(false));
            if was == Some(false) && up {
                // The wings only: the cry that goes up with them is the
                // bird's alarm, and `Voices` has it.
                audio.play_at(Sfx::WingBeats, animal.at, 0.7, self.jitter(0.1));
            }
            aloft.push((animal.id, up));
        }
        self.aloft = aloft;

        for cry in self.voices.hear(dt, animals, self.ear, self.night, &mut self.rng) {
            let pitch = self.jitter(0.05);
            audio.play_at(cry.sfx, cry.at, cry.gain, pitch);
        }

        // Anything within earshot pushing through leaves. Birds in the air
        // and fish are not in a bush.
        let mut brushes = Vec::with_capacity(self.brushes.len());
        for animal in animals {
            if animal.species.swims() || (animal.species.flies() && animal.speed > crate::logic::animal_model::AIRBORNE_SPEED) {
                continue;
            }
            if (animal.at - self.ear).length_squared() > f64::from(BRUSH_HEARING * BRUSH_HEARING) {
                continue;
            }
            let mut brush = self
                .brushes
                .iter()
                .find(|(id, _)| *id == animal.id)
                .map(|&(_, brush)| brush)
                .unwrap_or_else(Brush::new);
            let height = animal.species.height();
            let feet = animal.at.as_vec3() - Vec3::Y * (height * 0.5);
            let thickness = foliage_around(chunks, feet, height);
            if let Some(gain) = brush.advance(animal.speed * dt, animal.speed, thickness) {
                // A bear through a thicket is louder than a hare through
                // grass: the same clip, at the animal's size.
                let size = (height / 1.5).clamp(0.4, 1.2);
                let pitch = self.jitter(0.08) / size.sqrt();
                audio.play_at(Sfx::Rustle, animal.at, 0.8 * gain * size, pitch);
            }
            brushes.push((animal.id, brush));
        }
        self.brushes = brushes;

        self.frog_left -= dt;
        if self.frog_left <= 0.0 {
            self.frog_left = self.rng.range(0.3, 1.1);
            if !croaking.is_empty() {
                let at = croaking[self.rng.below(croaking.len())];
                audio.play_at(Sfx::FrogCroak, at.as_dvec3(), 0.45, self.jitter(0.12));
            }
        }

        self.bee_left -= dt;
        if self.bee_left <= 0.0 {
            self.bee_left = self.rng.range(0.9, 1.3);
            if !buzzing.is_empty() {
                let at = buzzing[self.rng.below(buzzing.len())];
                let loud = (0.12 * buzzing.len() as f32).min(0.6);
                audio.play_at(Sfx::Swarm, at.as_dvec3(), loud, self.jitter(0.05));
            }
        }

        // **A bat is the bird's wingbeats, smaller**: the same shove of air
        // off something flat, raised half as much again for a wing a tenth
        // the size and played quiet. Not a clip of its own, because what an
        // ear names first in a colony going up is the flutter, and the bank
        // already makes a flutter that is not a note (`bank::WingBeats`). The
        // squeak was rejected outright: a bat's call is above hearing, and
        // one a player could hear would be a whistle -- a note.
        self.bat_left -= dt;
        if self.bat_left <= 0.0 {
            self.bat_left = self.rng.range(0.35, 0.9);
            if !flapping.is_empty() {
                let at = flapping[self.rng.below(flapping.len())];
                let loud = (0.08 * flapping.len() as f32).min(0.4);
                audio.play_at(Sfx::WingBeats, at.as_dvec3(), loud, 1.5 * self.jitter(0.1));
            }
        }

        // ---- crickets ----
        //
        // **A bed, from somewhere in the grass.** Pieces laid end over end
        // like the rain, but each from its own spot a little way off
        // rather than in the ears, because a chorus is a field of singers
        // and the nearest of them is where the ear places it. The spot is
        // outside the hush, so a player walking through the meadow hears
        // the singing stop round them and start again behind.
        let moved = (self.ear - self.cricket_ear).with_y(0.0).length() as f32;
        self.cricket_ear = self.ear;
        let moving = dt > 0.0 && moved / dt > CRICKET_STILL && moved < 5.0;
        self.hush = cricket_hush(self.hush, moving, dt);
        self.cricket_left -= dt;
        if self.cricket_left <= 0.0 {
            self.cricket_left = (self.cricket_left + bank::BED_SECONDS - bank::BED_FADE).max(0.2);
            let loud = air.map_or(0.0, |(biome, warmth)| {
                cricket_chorus(&Night {
                    sun: self.sun,
                    warmth,
                    biome,
                    wet: self.wet,
                    shelter: self.shelter,
                    at_sea: self.water_share > AT_SEA,
                })
            });
            if loud > 0.02 {
                let angle = self.rng.range(0.0, std::f32::consts::TAU);
                let distance = self.hush + self.rng.range(1.0, 5.0);
                let at = self.ear
                    + glam::DVec3::new(f64::from(angle.cos() * distance), -1.3, f64::from(angle.sin() * distance));
                audio.play_at(Sfx::Crickets, at, loud, self.jitter(0.04));
            }
        }

        // ---- the country's own bed ----
        //
        // Three of them, and all three read the same `Air`: the heat of
        // the day, the hour, and which way the year is going. See the
        // section that defines them for why the season arrives as a
        // temperature and never as an enum.
        if let Some((biome, warmth)) = air {
            let here = Air {
                sun: self.sun,
                morning: self.morning,
                warmth,
                thaw: thawing(self.world_days),
                biome,
                wet: self.wet,
                shelter: self.shelter,
                at_sea: self.water_share > AT_SEA,
            };
            self.cicadas(audio, dt, &here);
            self.birdsong(audio, dt, &here);
            self.thaw_drip(audio, dt, &here);
        } else {
            // No chunk under the player yet, so there is no country to
            // sing. Eased down rather than cut, or walking out of a
            // world's edge would click.
            self.cicada.ease(0.0, dt);
            self.birds.ease(0.0, dt);
            self.thaw.ease(0.0, dt);
        }

        // ---- hooves ----
        let mut horses: Vec<Hoof> = animals
            .iter()
            .filter(|a| a.species == Species::Horse && (a.at - self.ear).length() < f64::from(HOOF_HEARING))
            .map(|a| {
                let feet = a.at.as_vec3() - Vec3::Y * (a.species.height() * 0.5);
                let wading = chunks
                    .block_at(feet.x.floor() as i32, (feet.y + 0.1).floor() as i32, feet.z.floor() as i32)
                    .is_some_and(types::is_liquid);
                Hoof {
                    id: a.id,
                    at: a.at,
                    speed: if wading { 0.0 } else { a.speed },
                    footing: Footing::of(self.ground_material(chunks, feet)),
                }
            })
            .collect();
        let ear = self.ear;
        horses.sort_by(|a, b| (a.at - ear).length_squared().total_cmp(&(b.at - ear).length_squared()));
        horses.truncate(HOOVES_HEARD);
        for beat in self.hooves.hear(dt, &horses) {
            audio.play_at(beat.sfx, beat.at, beat.gain, beat.pitch);
        }
    }

    // ------------------------------------------- the country's own bed

    /// The cicadas: the crickets' recording, played high and fast, laid
    /// end over end through the heat of the day.
    ///
    /// The interval is divided by [`CICADA_PITCH`] because a piece
    /// played faster *ends* earlier: laid at the crickets' spacing it
    /// would be a wall of sound with a hole in every second of it.
    fn cicadas(&mut self, audio: &Audio, dt: f32, air: &Air) {
        let loud = self.cicada.ease(cicada_chorus(air), dt);
        self.cicada_left -= dt;
        if self.cicada_left > 0.0 {
            return;
        }
        self.cicada_left =
            ((bank::BED_SECONDS - bank::BED_FADE) / CICADA_PITCH).max(0.2);
        if loud <= BED_FLOOR {
            return;
        }
        // Out in the grass rather than in the player's ears, and level
        // with the head: a cicada is in a tree or a stem, not underfoot
        // the way a cricket is.
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(3.0, 9.0);
        let at = self.ear
            + glam::DVec3::new(
                f64::from(angle.cos() * distance),
                f64::from(self.rng.range(-0.5, 2.5)),
                f64::from(angle.sin() * distance),
            );
        // The pitch jitter rides *on* the shift rather than replacing
        // it, so two pieces are never quite the same insect and the
        // spacing above still holds to within a few per cent.
        audio.play_at(Sfx::Crickets, at, loud * 0.85, CICADA_PITCH * self.jitter(0.03));
    }

    /// The birds at either end of the day: single calls from all round
    /// the player, as often as the chorus is loud.
    ///
    /// **The fowl's own voice, and nothing was downloaded for this.**
    /// Same argument as the monkey's (`bank::voice_of`): what that clip
    /// *is* is a small warm-blooded thing calling in short repeated
    /// bursts from a hedge, which is what a bird in a hedge is. Played
    /// high and quiet and from a long way off, which is the difference
    /// between a hen in the yard and a thrush four gardens away.
    fn birdsong(&mut self, audio: &Audio, dt: f32, air: &Air) {
        let loud = self.birds.ease(dawn_chorus(air), dt);
        self.bird_left -= dt;
        if self.bird_left > 0.0 {
            return;
        }
        if loud <= BED_FLOOR {
            // Asked again shortly rather than never: the window opens
            // over a couple of minutes and a timer set for the gap at
            // silence would miss the first half of it.
            self.bird_left = 1.0;
            return;
        }
        let gap = BIRD_GAP.1 + (BIRD_GAP.0 - BIRD_GAP.1) * loud;
        self.bird_left = self.rng.range(gap * 0.5, gap * 1.6);
        let Some(sfx) = voice_of(Species::Fowl, Cry::Idle) else {
            return;
        };
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(6.0, 22.0);
        let at = self.ear
            + glam::DVec3::new(
                f64::from(angle.cos() * distance),
                f64::from(self.rng.range(1.0, 6.0)),
                f64::from(angle.sin() * distance),
            );
        audio.play_at(sfx, at, 0.16 + 0.24 * loud, self.rng.range(1.25, 1.65));
    }

    /// The thaw: water off an edge, every second or so, while the year
    /// is warming and there is still something up there to melt.
    fn thaw_drip(&mut self, audio: &Audio, dt: f32, air: &Air) {
        let loud = self.thaw.ease(eaves_drip(air), dt);
        self.thaw_left -= dt;
        if self.thaw_left > 0.0 {
            return;
        }
        if loud <= BED_FLOOR {
            self.thaw_left = 1.0;
            return;
        }
        let gap = DRIP_GAP.1 + (DRIP_GAP.0 - DRIP_GAP.1) * loud;
        self.thaw_left = self.rng.range(gap * 0.4, gap * 1.8);
        // **The rain's own tick, from above the head.** A drop off an
        // eave and a drop out of a cloud are the same event, and the
        // difference an ear can name is where it comes from: the rain's
        // drips are placed at shoulder height round the player, and
        // these come off whatever is over them.
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(1.0, 4.5);
        let at = self.ear
            + glam::DVec3::new(
                f64::from(angle.cos() * distance),
                f64::from(self.rng.range(0.8, 2.2)),
                f64::from(angle.sin() * distance),
            );
        audio.play_at(Sfx::RainTick, at, 0.25 + 0.4 * loud, self.rng.range(0.85, 1.25));
    }

    // ------------------------------------------------------------ mood

    /// Everything the choice of mood is made of, and nothing else.
    ///
    /// **A struct of facts rather than the whole `Frame`.** What the
    /// music should be doing when a player is bleeding out in a
    /// thunderstorm under a roof is a question about six booleans, and
    /// answering it in a test should not require a camera, a chunk
    /// manager and a sky. Pulling the facts out here is what lets
    /// [`mood_for`] be a function anybody can call with a literal.
    fn surroundings(&self, frame: &Frame<'_>) -> Surroundings {
        Surroundings {
            health_fraction: frame.health_fraction,
            outdoors: self.outdoors,
            height: frame.player.position.y as f32,
            weather: frame.weather,
            night: frame.sky.sun_elevation() <= 0.0,
            fire_near: self.fire_near,
            water_share: self.water_share,
            leaf_share: self.leaf_share,
            afloat: frame.player.in_water || frame.player.swimming,
        }
    }

    /// Re-reads what is around the player, now and then.
    ///
    /// **A sample rather than a survey.** Eight directions at four
    /// distances, one probe each: thirty-two lookups against the
    /// thousands a radius would cost, answered every second and a half
    /// rather than every frame. What it is deciding is which of two
    /// pieces of background music plays, and being a second and a half
    /// late to a coastline is not something anybody can notice -- the
    /// composer takes three seconds to believe a mood anyway.
    fn look_around(&mut self, frame: &Frame<'_>) {
        let at = frame.player.position;
        let mut water = 0;
        let mut leaves = 0;
        let mut probes = 0;
        for step in 0..8 {
            let angle = step as f32 * std::f32::consts::TAU / 8.0;
            for distance in [3.0f32, 7.0, 12.0, 18.0] {
                let x = (at.x + f64::from(angle.cos() * distance)).floor() as i32;
                let z = (at.z + f64::from(angle.sin() * distance)).floor() as i32;
                probes += 1;
                // Water at the player's own level, because that is what
                // "out on the water" means -- a lake seen from a cliff
                // is scenery, not a crossing.
                if let Some(block) = frame.chunks.block_at(x, at.y.floor() as i32, z) {
                    if types::is_liquid(block) {
                        water += 1;
                    }
                }
                // Leaves anywhere from head height up: a canopy is
                // overhead by definition, and the trunks are not what
                // makes a forest sound like one.
                let head = (at.y + 2.0).floor() as i32;
                for y in head..(head + 6).min(CHUNK_SIZE_Y as i32 - 1) {
                    if frame.chunks.block_at(x, y, z).is_some_and(types::is_leafy) {
                        leaves += 1;
                        break;
                    }
                }
            }
        }
        self.water_share = water as f32 / probes as f32;
        self.leaf_share = leaves as f32 / probes as f32;
        self.fire_near = nearest_fire(frame.chunks, at.as_vec3()).is_some_and(|(x, y, z)| {
            let dx = x as f32 + 0.5 - at.x as f32;
            let dy = y as f32 + 0.5 - at.y as f32;
            let dz = z as f32 + 0.5 - at.z as f32;
            dx * dx + dy * dy + dz * dz <= HEARTH_RANGE * HEARTH_RANGE
        });
    }

    /// Sends a mood only once it has been true for a moment.
    ///
    /// Three seconds of hysteresis, because every one of the tests above
    /// can flicker: walking under a tree, a health bar hovering on the
    /// threshold, the sun sitting exactly on the horizon. Without this
    /// the composer would be told to fade out and start again every few
    /// frames, and the music would never actually play.
    fn push_mood(&mut self, audio: &Audio, wanted: Mood, dt: f32) {
        if wanted == self.mood {
            self.candidate = wanted;
            self.candidate_held = 0.0;
            return;
        }
        if wanted == self.candidate {
            self.candidate_held += dt;
            if self.candidate_held >= 3.0 {
                self.mood = wanted;
                self.candidate_held = 0.0;
                audio.set_mood(wanted);
            }
        } else {
            // The frame a candidate first appears counts towards its
            // own hold. Starting the clock at zero instead would make
            // the threshold depend on the frame rate, which is exactly
            // the sort of thing that works on the machine it was
            // written on.
            self.candidate = wanted;
            self.candidate_held = dt;
        }
    }

    // ----------------------------------------------------------- pieces

    /// A pitch multiplier within `spread` either side of unity.
    ///
    /// Applied to nearly everything. Three recorded variants stop the
    /// ear finding a loop; a per-play pitch shift stops it finding the
    /// three.
    fn jitter(&mut self, spread: f32) -> f32 {
        1.0 + self.rng.range(-spread, spread)
    }

    /// What the player is standing on.
    ///
    /// Looks down rather than at one cell, because what is under the
    /// feet is often not what is being stood on: a tuft of grass, a
    /// pebble or a layer of snow occupies the cell the feet are in, and
    /// the ground is the next one down. Three cells is enough for any of
    /// those and cheap enough to do on every footfall.
    fn ground_material(&self, chunks: &ChunkManager, feet: Vec3) -> Material {
        let x = feet.x.floor() as i32;
        let z = feet.z.floor() as i32;
        // Start slightly below the feet: exactly at `feet.y` is the
        // boundary, and floating-point puts it on either side of it.
        let from = (feet.y - 0.05).floor() as i32;
        for y in (from - 2..=from).rev() {
            let Some(block) = chunks.block_at(x, y, z) else {
                continue;
            };
            if types::is_air(block) || types::is_liquid(block) {
                continue;
            }
            return Material::of(block);
        }
        // Nothing under the feet at all -- a chunk that has not arrived,
        // or the moment of stepping off the edge of the world. Soil is
        // the least surprising thing to be wrong about.
        Material::Dirt
    }

    // ------------------------------------------------- one-off events
    //
    // These are here rather than at their call sites only because they
    // want the pitch jitter, which needs the generator this owns.

    /// A block finished coming apart. `was` is what used to be there.
    ///
    /// **Two sounds, not one, where the material has debris to shed.** The
    /// break is the blow that finished it; the crumble laid under it is
    /// what falls afterwards -- scree off the face of a cut, soil off a
    /// bank, a board cracking through. Without it a pick through a cliff
    /// was a series of clean taps, which is what a hammer on a worktop
    /// sounds like and not what a mine does. It is quiet (the recordings
    /// are trimmed low and this is quieter again), it is only the five
    /// materials that have one (`bank::CRUMBLES` says why glass and metal
    /// do not), and it is pitched apart from the break so the two do not
    /// read as one longer knock.
    pub fn on_block_broken(&mut self, audio: &Audio, cell: (i32, i32, i32), was: BlockId) {
        let material = Material::of(was);
        audio.play_at_block(Sfx::Material(Impact::Break, material), cell, 1.0, self.jitter(0.1));
        if let Some(crumble) = crumble_of(material) {
            audio.play_at_block(crumble, cell, 0.45, self.jitter(0.12) + 0.1);
        }
    }

    /// Something let go: a roof nobody was holding up, a column of sand
    /// peeling away, a board that had burned through. `block` is what is
    /// falling.
    ///
    /// **The crumble on its own and at full weight.** A break has a blow
    /// in front of it and this has nothing -- the world simply gave way --
    /// so what the player hears is the debris, and it has to be loud
    /// enough to be a warning rather than a texture: the whole reason to
    /// carry logs into a mine is that a gallery might come down, and a
    /// collapse nobody heard is a mechanic that only exists in the death
    /// screen. A material with no crumble (an ingot dislodged, a pot off a
    /// shelf) falls quietly, which is what it would do.
    pub fn on_gave_way(&mut self, audio: &Audio, at: glam::DVec3, block: BlockId) {
        if let Some(crumble) = crumble_of(Material::of(block)) {
            audio.play_at(crumble, at, 1.0, self.jitter(0.1));
        }
    }

    /// A quarter came off a block somebody is digging
    /// (`dig::took_a_slice`).
    ///
    /// **The debris and not a second blow.** Every swing already knocks
    /// (`Impact::Dig`, played by the hand that swung); what a quarter coming
    /// away adds is the stuff falling off the face -- scree off rock, a
    /// shovelful of soil, gravel running, sand pouring -- which is exactly
    /// what [`crumble_of`] already holds for those four materials. So the
    /// crumble, quieter than under a break (it is a quarter of the block)
    /// and pitched a little up for the same reason.
    ///
    /// Rejected: **four new recordings of "a chunk coming off"**. They would
    /// be the same four materials giving way, which the crumbles already
    /// are, and the recordings are held to a budget
    /// (`the_recordings_stay_a_modest_download`); reusing them costs nothing.
    /// Turf borrows soil's (`bank::slice_of`); a material with nothing to shed
    /// -- none of the ones that dig in slices, and
    /// `every_block_that_digs_in_slices_has_a_sound_for_a_quarter_coming_off`
    /// holds that -- would fall silent rather than play something wrong.
    pub fn on_slice_taken(&mut self, audio: &Audio, cell: (i32, i32, i32), was: BlockId) {
        if let Some(crumble) = crate::audio::bank::slice_of(Material::of(was)) {
            audio.play_at_block(crumble, cell, SLICE_GAIN, self.jitter(0.12) + 0.15);
        }
    }

    /// A block appeared.
    pub fn on_block_placed(&mut self, audio: &Audio, cell: (i32, i32, i32), block: BlockId) {
        audio.play_at_block(
            Sfx::Material(Impact::Place, Material::of(block)),
            cell,
            0.85,
            self.jitter(0.1),
        );
    }

    /// The player lost `amount` of health.
    ///
    /// **A blow is heard; a drain is not a drum roll.** Every drop in health
    /// played the heavy punch, and health also goes to illness, cold, thirst
    /// and a bleeding cut a crumb a tick -- so a sick player heard a punch a
    /// dozen times a second ("звук урона слишком быстрый"). Now the crumbs are
    /// owed up to a whole point before they are heard, and nothing is heard
    /// more often than `HURT_GAP_SECONDS`. A real blow (`HURT_BLOW` or more)
    /// is heard at once, gap permitting.
    pub fn on_hurt(&mut self, audio: &Audio, amount: f32) {
        if self.hurt_due(amount, std::time::Instant::now()) {
            audio.play_flat(Sfx::Hurt, 0.8, self.jitter(0.08));
        }
    }

    /// Whether a loss of `amount` now is heard, and the owing that goes with
    /// it. Split out so the rule can be tested without an audio device.
    fn hurt_due(&mut self, amount: f32, now: std::time::Instant) -> bool {
        self.hurt_owed += amount.max(0.0);
        let rested = self
            .hurt_at
            .is_none_or(|at| now.saturating_duration_since(at).as_secs_f32() >= HURT_GAP_SECONDS);
        if rested && (amount >= HURT_BLOW || self.hurt_owed >= 1.0) {
            self.hurt_owed = 0.0;
            self.hurt_at = Some(now);
            true
        } else {
            false
        }
    }

    /// **A bolt of lightning came down there.** The flash is the sky's
    /// (`engine::sky::Sky::strike`); what is queued here is the noise,
    /// to be played when it arrives -- see `cracks`.
    ///
    /// The delay is worked out from where the player is *now* rather
    /// than where they will be when it lands, which is wrong by the
    /// width of a few paces and right by the whole of what matters: a
    /// player running from a storm is not outrunning its sound.
    pub fn lightning(&mut self, at: glam::DVec3, ear: glam::DVec3) {
        let distance = (at - ear).length() as f32;
        if distance > primitive_shared::lightning::HEARD_WITHIN {
            return;
        }
        self.cracks
            .push((at, primitive_shared::lightning::thunder_delay(distance)));
    }

    /// A swing that landed on something alive.
    pub fn on_strike(&mut self, audio: &Audio, connected: bool) {
        if connected {
            audio.play_flat(Sfx::Hit, 0.7, self.jitter(0.1));
        } else {
            audio.play_flat(Sfx::Swing, 0.5, self.jitter(0.12));
        }
    }
}

/// How hard it is raining, as the ear hears it.
#[derive(Debug, Clone, Copy)]
struct Rainfall {
    /// How loud the bed plays.
    gain: f32,
    /// Drips a second off the eaves, where there are eaves.
    drips: f32,
}

/// How hard it is raining, from the weather and the wind.
///
/// **The same curve the rain on the screen uses.** The particles are
/// dropped at `weather.intensity() * (0.65 + 0.5 * wind.strength)` (see
/// `lib.rs`, where the rain is emitted), and rain that thickens in the
/// window while the sound of it stays exactly level is worse than
/// either alone -- the eye and the ear disagreeing is the kind of wrong
/// nobody can name and everybody notices.
///
/// **A squall multiplies it; a calm does not switch it off.** Rain with
/// no wind in it is still rain, and the quiet end of this is two thirds
/// of a squall's loudness -- steadier, not absent. Weather that is not wet
/// at all is the caller's business: this is never asked.
///
/// This used to answer drops a second and a gain per drop, when the rain
/// was drops. The bed has no drops to count, so what the squall now moves
/// is the loudness of the band and how often the eaves drip.
fn raining(intensity: f32, wind: f32) -> Rainfall {
    let wind = wind.clamp(0.0, 1.0);
    let squall = 0.65 + 0.5 * wind;
    Rainfall {
        gain: (0.45 + 0.4 * intensity.clamp(0.0, 1.0)) * squall,
        drips: (0.6 + 1.4 * intensity.clamp(0.0, 1.0)) * squall,
    }
}

/// How the rain reaches the player where they stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelter {
    /// Nothing overhead.
    Open,
    /// Leaves overhead and sky through them: the rain, and drops falling
    /// off the canopy.
    Canopy,
    /// A roof or an overhang, with open sky a few steps away.
    Roofed,
    /// No sky anywhere near: deep in a house, a mine, a cave.
    Enclosed,
}

/// Where the rain can be heard from, a few steps from the player, and at
/// what reach.
const SHELTER_PROBES: [f32; 2] = [3.0, 6.0];

/// How the rain reaches a player at `position`.
///
/// **Three questions, cheapest first**: is there sky straight up; if so,
/// is some of what is up there leaves; if not, is there sky a few steps
/// away in any direction. The last is sixteen columns, and it is asked
/// only under a roof, every half a second.
fn shelter_at(chunks: &ChunkManager, position: Vec3) -> Shelter {
    let overhead = sky_visible(chunks, position);
    let leaves = overhead && canopy_over(chunks, position);
    let open_nearby = if overhead {
        0
    } else {
        let mut open = 0;
        for step in 0..8 {
            let angle = step as f32 * std::f32::consts::TAU / 8.0;
            for reach in SHELTER_PROBES {
                let probe = position + Vec3::new(angle.cos() * reach, 0.0, angle.sin() * reach);
                if sky_visible(chunks, probe) {
                    open += 1;
                }
            }
        }
        open
    };
    shelter_from(overhead, leaves, open_nearby)
}

/// The decision [`shelter_at`] makes, out of the facts it gathers -- apart,
/// so it can be asked with literals.
fn shelter_from(overhead: bool, leaves: bool, open_nearby: usize) -> Shelter {
    match (overhead, leaves) {
        (true, true) => Shelter::Canopy,
        (true, false) => Shelter::Open,
        (false, _) if open_nearby > 0 => Shelter::Roofed,
        (false, _) => Shelter::Enclosed,
    }
}

/// Any leaves in the column over the player's head?
fn canopy_over(chunks: &ChunkManager, position: Vec3) -> bool {
    let x = position.x.floor() as i32;
    let z = position.z.floor() as i32;
    let head = (position.y + 1.6).floor() as i32;
    let top = (head + ROOF_PROBE).min(CHUNK_SIZE_Y as i32 - 1);
    ((head + 1)..=top).any(|y| chunks.block_at(x, y, z).is_some_and(types::is_leafy))
}

/// Which rain bed a shelter hears, how loud against the open sky, and how
/// much of the rain's dripping reaches it.
///
/// **Under a roof the rain is the other bed, not the same one quieter.**
/// Turning the open-sky rain down leaves its hiss in, and a hiss is what
/// says "outside". What a roof leaves is the patter on it and a dull
/// rush, and the odd heavy drop off the edge -- which is only noticed
/// because the bed has gone quiet enough to notice one.
fn rain_heard(shelter: Shelter) -> Option<(Sfx, f32, f32)> {
    match shelter {
        Shelter::Open => Some((Sfx::Rain, 1.0, 0.0)),
        Shelter::Canopy => Some((Sfx::Rain, 0.8, 1.0)),
        Shelter::Roofed => Some((Sfx::RainSheltered, 0.75, 0.5)),
        Shelter::Enclosed => None,
    }
}

/// How strong the wind has to be before it howls anywhere.
const HOWLING: f32 = 0.8;

/// How far above the sea the ground counts as high, for the wind.
///
/// Forty blocks: the height at which a hill stops being a rise in a meadow
/// and becomes somewhere the wind has nothing in its way.
const HIGH_GROUND: f32 = primitive_shared::worldgen::SEA_LEVEL as f32 + 40.0;

/// Which of the three winds is heard.
///
/// **The most exposed answer wins.** A storm howls wherever it is; a
/// strong wind or a moderate one on high ground howls too. Below that,
/// trees round the player or a wind too light to be a gust is heard as
/// the leaves and grass it moves. What is left is a gust over open ground.
///
/// Rejected: choosing by `Mood` or by biome. The music's forest is a
/// canopy overhead; the wind's is leaves anywhere near, and a player at
/// the edge of a wood hears the wood. The ring of probes (`leaf_share`)
/// already says that.
pub fn wind_kind(strength: f32, height: f32, leaf_share: f32, storm: bool) -> Sfx {
    let strength = strength.clamp(0.0, 1.0);
    if storm || strength >= HOWLING || (height >= HIGH_GROUND && strength >= 0.3) {
        Sfx::WindHowl
    } else if leaf_share >= 0.25 || strength < 0.25 {
        Sfx::WindBreeze
    } else {
        Sfx::Wind
    }
}

/// How thick the foliage is around a body standing at `feet` and
/// `height` tall: one for leaves, about half for tall stems at the feet,
/// nothing for open air.
///
/// **What can be pushed through, not what is green.** A leaf block is
/// `Slow, not solid` (see `types::is_collidable`); so are grasses, reeds
/// and crops. Turf is solid and is walked *on*, and a block of snow is
/// not a bush.
fn foliage_around(chunks: &ChunkManager, feet: Vec3, height: f32) -> f32 {
    let x = feet.x.floor() as i32;
    let z = feet.z.floor() as i32;
    let bottom = (feet.y + 0.05).floor() as i32;
    let top = (feet.y + height.max(0.5) - 0.05).floor() as i32;
    let mut thickest = 0.0f32;
    for y in bottom..=top {
        if let Some(block) = chunks.block_at(x, y, z) {
            thickest = thickest.max(brushes(block));
        }
    }
    thickest
}

/// How much one block brushes against a body pushing through it.
fn brushes(block: BlockId) -> f32 {
    if types::is_leafy(block) {
        return 1.0;
    }
    if types::is_air(block) || types::is_liquid(block) || types::is_collidable(block) {
        return 0.0;
    }
    if Material::of(block) == Material::Grass {
        0.55
    } else {
        0.0
    }
}

/// Blocks moved through foliage between one rustle and the next.
///
/// A little under a stride: a body in a thicket is in contact with it the
/// whole time, and a rustle per footstep left gaps a bush does not have.
pub const BRUSH_STRIDE: f32 = 0.8;

/// Slower than this, pushing through leaves makes no sound: standing in a
/// hedge, swaying.
pub const BRUSH_SPEED: f32 = 0.4;

/// The speed a rustle is at its loudest. Through leaves at a third of a
/// walk that is barely reached, which is right: forcing through a bush is
/// loud, easing through it is not.
pub const BRUSH_LOUD: f32 = 3.0;

/// One body pushing through foliage.
///
/// **By distance, like a footstep**, so a run through a thicket rustles
/// oftener than a creep through it -- and loudness by speed, so it also
/// rustles harder. Entering is heard at once: the first leaves touched are
/// the ones that say there is a bush there.
#[derive(Debug, Clone, Copy)]
pub struct Brush {
    travelled: f32,
}

impl Brush {
    pub fn new() -> Brush {
        Brush { travelled: BRUSH_STRIDE }
    }

    /// `moved` blocks at `speed` through foliage `thickness` thick (see
    /// [`foliage_around`]). A gain when this movement rustles.
    pub fn advance(&mut self, moved: f32, speed: f32, thickness: f32) -> Option<f32> {
        if thickness <= 0.0 {
            // Out of it: the next leaf touched is heard straight away.
            self.travelled = BRUSH_STRIDE;
            return None;
        }
        if speed < BRUSH_SPEED {
            return None;
        }
        self.travelled += moved;
        if self.travelled < BRUSH_STRIDE {
            return None;
        }
        self.travelled = 0.0;
        Some(thickness * (speed / BRUSH_LOUD).clamp(0.25, 1.0))
    }
}

impl Default for Brush {
    fn default() -> Self {
        Brush::new()
    }
}

/// How far away an animal pushing through a bush is heard.
const BRUSH_HEARING: f32 = 20.0;

// ------------------------------------------------------------- voices

/// Cries a second, on average, across every animal in earshot -- and how
/// many may come at once.
///
/// **A budget, because a herd is many throats.** Twenty sheep calling at
/// their own rates is a wall of bleating, and a herd bolting is twenty
/// alarms in one frame. One and a half a second, three at once, keeps a
/// meadow a meadow; a wound and a death are not charged to it, because
/// those are the answer to something the player did and have to be heard.
const CRIES_PER_SECOND: f32 = 1.5;
const CRY_BURST: f32 = 3.0;

/// After one of a species raises the alarm, how long the rest of its herd
/// is taken to be saying the same thing.
const HERD_ALARM: f32 = 1.2;

/// How long after a blow an animal that vanishes is taken to have died of
/// it, rather than to have walked out of range.
///
/// Snapshots arrive twenty times a second and a carcass replaces a dead
/// animal in the next one; a second and a half is room for a slow link and
/// no room for a hare that was struck, fled and was then forgotten at the
/// edge of the interest radius.
const DEATH_WINDOW: f32 = 1.5;

/// How far a calm call is listened for. A hunter's cry and a wound carry to
/// the edge of hearing; grazing does not need to.
const IDLE_RANGE: f32 = 32.0;

/// How close a charge has to start to be a threat to *you*.
const THREAT_RANGE: f32 = 20.0;

/// How often a night hunter that is *stalking* -- circling a person, or the
/// edge of a fire -- is picked to howl, against its calm night rate
/// (`idle_chance`, 0.15 for a wolf).
///
/// **The warning is the sound.** A pack circling a camp in the dark is two
/// shapes nobody can see, and the only way a player at the fire knows the
/// ring is there -- and that it is *closing*, as the howls come from nearer
/// -- is to hear it. At the calm rate a pair walking the ring said something
/// once a minute, which is no warning; at this it is every few seconds from
/// somewhere round the dark, and the one that falls silent is the one
/// coming in (a charge is `Cry::Threat`, not a howl).
///
/// Rejected: *eyes that glint at the edge of the light.* It is the picture
/// everybody has, and the renderer has no cheap way to draw a lit point on
/// an animal the night has darkened: the animal models are lit as the world
/// is, and a pair of unlit texels would need a second pass or a new vertex
/// flag through every model. A voice needed a byte the wire already had.
const STALKING_HOWL: f32 = 0.7;

/// ...and how far off a stalking hunter is listened for: the whole of
/// hearing, where a calm call stops at `IDLE_RANGE`. The ring round a fire
/// is inside either; a pack working round a sleeper from further off is the
/// one worth hearing at the edge of it.
const STALKING_RANGE: f32 = crate::audio::HEARING;

/// Blocks from the ears at which an animal that minds company is walked up to.
const APPROACHED: f64 = 4.0;

/// Does this animal say something when somebody walks up to it? See `hear`.
fn snorts_at_company(species: Species) -> bool {
    species == Species::Horse
}

/// How often one of a species, picked while calm, actually says something.
///
/// **By what it is, and by the hour.** A sheep and a hen talk all day; a
/// deer barely does; a wolf howls after dark and not before; a lion roars
/// at night and only now and then by day. A hare and a fish say nothing
/// calm at all, and have no calm voice to say it with.
fn idle_chance(species: Species, night: bool) -> f32 {
    match species {
        Species::Sheep => 0.5,
        Species::Fowl => 0.55,
        Species::Boar => 0.35,
        Species::Deer => 0.12,
        Species::Zebra => 0.25,
        // A herd blowing and snorting as it grazes. The whinny is its alarm
        // now that it has a voice of its own; a calm horse is the snort, and
        // that is how a player on a hill learns there are horses below
        // without the herd sounding frightened.
        Species::Horse => 0.2,
        Species::Antelope => 0.08,
        Species::Bear => 0.12,
        Species::Gull => 0.7,
        Species::Wolf => {
            if night {
                0.15
            } else {
                0.0
            }
        }
        Species::Lion => {
            if night {
                0.2
            } else {
                0.03
            }
        }
        // **The one animal that is louder at night than by day**, and the
        // only reason it has a calm voice at all: a rat is the thing you
        // hear and cannot see. By day it is out of the walls and running,
        // and running animals here say nothing calm.
        Species::Rat => {
            if night {
                0.45
            } else {
                0.0
            }
        }
        // **The loudest calm animal in the world, and the point of it.** A
        // troop in the crowns chatters at each other all day about nothing,
        // and a player who has learned the sound knows there is a grove over
        // the dune before they can see one. What it *also* means -- because
        // the same voice is the alarm (`cry_of`) -- is that a troop going
        // off all at once is something under the trees that is not you, and
        // there is no other warning of that on a coast with no wolves.
        Species::Monkey => 0.6,
        // A crab says nothing, ever. It has no voice at all, and what a
        // beach at night sounds like is the surf.
        Species::Crab
        | Species::Hare
        | Species::Fish
        | Species::Cod
        | Species::Trout
        | Species::Pike
        | Species::Herring => 0.0,
    }
}

/// One cry, to be played.
#[derive(Debug, Clone, Copy)]
pub struct Utterance {
    pub sfx: Sfx,
    pub at: glam::DVec3,
    pub gain: f32,
}

/// What the soundscape remembers about one animal between frames.
#[derive(Debug, Clone, Copy)]
struct Known {
    id: EntityId,
    species: Species,
    at: glam::DVec3,
    hurt: f32,
    running: bool,
    /// When it was last struck, on [`Voices`]' clock.
    wounded_at: f32,
    /// It says nothing calm before this.
    quiet_until: f32,
    /// ...and raises no alarm before this. Apart from the calm hush on
    /// purpose: a boar that grunted a moment ago still snarls when it
    /// charges.
    cried_until: f32,
    /// Within a few steps of the ears. See `snorts_at_company`.
    near: bool,
    /// As it was last drawn: see `Heard::stalking`.
    stalking: bool,
}

/// Every animal's voice, read off what was drawn.
///
/// **Nothing about an animal's mind is sent, and nothing is sent for
/// this.** The server knows whether a boar is fleeing or charging; the
/// client knows where it was drawn last frame and this frame, how fast, and
/// whether the flash of a blow went up. That is enough:
///
/// * **a wound** is the flash rising -- the same edge `Entities::apply`
///   draws blood on;
/// * **a death** is an animal that was wounded a moment ago and is no longer
///   in the snapshot (a carcass is a block, not an entity);
/// * **an alarm** is the moment it breaks from a walk into a run, and **a
///   threat** is the same moment for a hunter whose run is pointed at the
///   player;
/// * **a calm call** is one animal, picked now and then among the calm ones
///   nearby, speaking at its species' own rate.
///
/// Rejected: sending the server's state for sound's sake. A sound is a
/// drawing of something, like blood is; a field on the wire for it is a
/// protocol change for every client to decide nothing, and a boar that
/// visibly breaks into a run at you *is* a charge, whatever the server
/// called it.
pub struct Voices {
    clock: f32,
    known: Vec<Known>,
    idle_left: f32,
    budget: f32,
    /// Which species last raised an alarm, and when.
    herd_alarm: Vec<(Species, f32)>,
}

impl Default for Voices {
    fn default() -> Self {
        Voices::new()
    }
}

impl Voices {
    pub fn new() -> Voices {
        Voices { clock: 0.0, known: Vec::new(), idle_left: 1.0, budget: CRY_BURST, herd_alarm: Vec::new() }
    }

    fn spend(&mut self) -> bool {
        if self.budget >= 1.0 {
            self.budget -= 1.0;
            true
        } else {
            false
        }
    }

    /// One frame of listening: what the animals in `animals` say, given
    /// what they were doing last frame.
    pub fn hear(&mut self, dt: f32, animals: &[Heard], ear: glam::DVec3, night: bool, rng: &mut Rng) -> Vec<Utterance> {
        self.clock += dt;
        self.budget = (self.budget + dt * CRIES_PER_SECOND).min(CRY_BURST);
        let clock = self.clock;
        let mut said = Vec::new();
        let mut known = Vec::with_capacity(animals.len());

        for animal in animals {
            let before = self.known.iter().find(|k| k.id == animal.id).copied();
            let mut now = before.unwrap_or(Known {
                id: animal.id,
                species: animal.species,
                at: animal.at,
                hurt: animal.hurt,
                running: false,
                wounded_at: f32::NEG_INFINITY,
                // A herd walking into earshot does not all speak at once.
                quiet_until: clock + rng.range(0.0, 4.0),
                cried_until: clock,
                near: false,
                stalking: animal.stalking,
            });

            // ---- a wound ----
            if let Some(before) = before {
                if animal.hurt > before.hurt + 1e-3 {
                    // Not twice for one blow arriving over two snapshots.
                    if clock - before.wounded_at > 0.3 {
                        if let Some(sfx) = voice_of(animal.species, Cry::Hurt) {
                            said.push(Utterance { sfx, at: animal.at, gain: 0.85 });
                        }
                    }
                    now.wounded_at = clock;
                    // The squeal is the alarm: no snort on top of it as it
                    // turns to run.
                    now.quiet_until = now.quiet_until.max(clock + 1.0);
                    now.cried_until = now.cried_until.max(clock + 1.0);
                }
            }

            // ---- breaking into a run ----
            let walk = animal.species.walk_speed();
            let running = if now.running { animal.speed > walk * 1.2 } else { animal.speed > walk * 1.6 };
            if running && !now.running && before.is_some() {
                let heading = (animal.at - now.at).as_vec3().with_y(0.0).normalize_or_zero();
                let to_ear = (ear - animal.at).as_vec3().with_y(0.0);
                let charging = voice_of(animal.species, Cry::Threat).is_some()
                    && to_ear.length() < THREAT_RANGE
                    && heading.dot(to_ear.normalize_or_zero()) > 0.6;
                let cry = if charging { Cry::Threat } else { Cry::Alarm };
                let herd_spoke = cry == Cry::Alarm
                    && self
                        .herd_alarm
                        .iter()
                        .any(|&(species, when)| species == animal.species && clock - when < HERD_ALARM);
                if clock >= now.cried_until && !herd_spoke && (animal.at - ear).length() < f64::from(crate::audio::HEARING) && self.spend() {
                    if let Some(sfx) = voice_of(animal.species, cry) {
                        let gain = if cry == Cry::Threat { 0.9 } else { 0.7 };
                        said.push(Utterance { sfx, at: animal.at, gain });
                        now.cried_until = clock + 2.5;
                        now.quiet_until = now.quiet_until.max(clock + 2.5);
                        if cry == Cry::Alarm {
                            self.herd_alarm.retain(|(species, _)| *species != animal.species);
                            self.herd_alarm.push((animal.species, clock));
                        }
                    }
                }
            }

            // ---- walked up to ----
            //
            // **A horse snorts at somebody coming up to it**: the blow down
            // the nose that is a horse taking the measure of what is at its
            // head. The one calm sound here that answers the player rather
            // than the clock, and it answers "will it let me near" before
            // the right click does. Not every calm animal: a sheep walked
            // up to walks off, and the alarm already says so.
            let distance = (animal.at - ear).length();
            let near = if now.near { distance < APPROACHED * 1.5 } else { distance < APPROACHED };
            if near
                && !now.near
                && before.is_some()
                && !running
                && snorts_at_company(animal.species)
                && clock >= now.cried_until
                && self.spend()
            {
                if let Some(sfx) = voice_of(animal.species, Cry::Idle) {
                    said.push(Utterance { sfx, at: animal.at, gain: 0.6 });
                    now.quiet_until = now.quiet_until.max(clock + 6.0);
                }
            }
            now.near = near;

            now.running = running;
            now.at = animal.at;
            now.hurt = animal.hurt;
            now.stalking = animal.stalking;
            known.push(now);
        }

        // ---- a death ----
        for gone in &self.known {
            if animals.iter().any(|a| a.id == gone.id) {
                continue;
            }
            if clock - gone.wounded_at < DEATH_WINDOW && (gone.at - ear).length() < f64::from(crate::audio::HEARING) {
                if let Some(sfx) = voice_of(gone.species, Cry::Death) {
                    said.push(Utterance { sfx, at: gone.at, gain: 0.9 });
                }
            }
        }
        self.known = known;

        // ---- calm ----
        self.idle_left -= dt;
        if self.idle_left <= 0.0 {
            self.idle_left = rng.range(0.8, 2.0);
            // A night hunter on the ring howls as a stalker, anything else
            // as itself: see `STALKING_HOWL`.
            let stalker = |k: &Known| night && k.stalking && idle_chance(k.species, true) > 0.0;
            let calm: Vec<usize> = (0..self.known.len())
                .filter(|&i| {
                    let k = &self.known[i];
                    let range = if stalker(k) { STALKING_RANGE } else { IDLE_RANGE };
                    !k.running
                        && clock >= k.quiet_until
                        && (k.at - ear).length() < f64::from(range)
                        && idle_chance(k.species, night) > 0.0
                        && voice_of(k.species, Cry::Idle).is_some()
                })
                .collect();
            // **The ring first.** A stalker in earshot is picked before a
            // grazing sheep, or the pack round the camp would be waiting its
            // turn behind the flock in the pen.
            let stalking: Vec<usize> = calm.iter().copied().filter(|&i| stalker(&self.known[i])).collect();
            let pool = if stalking.is_empty() { &calm } else { &stalking };
            if !pool.is_empty() {
                let i = pool[rng.below(pool.len())];
                let species = self.known[i].species;
                let ringing = stalker(&self.known[i]);
                let chance = if ringing { STALKING_HOWL } else { idle_chance(species, night) };
                if rng.chance(chance) && self.spend() {
                    if let Some(sfx) = voice_of(species, Cry::Idle) {
                        let gain = if ringing { 0.75 } else { 0.55 };
                        said.push(Utterance { sfx, at: self.known[i].at, gain });
                        self.known[i].quiet_until = clock + if ringing { rng.range(3.0, 6.0) } else { rng.range(5.0, 12.0) };
                    }
                }
            }
        }
        said
    }
}

// ------------------------------------------------------------ crickets

/// What the night is like where the player stands, as far as a cricket
/// cares. A struct of facts for the reason `Surroundings` is one: whether
/// the grass sings is a question a test should answer with a literal.
#[derive(Debug, Clone, Copy)]
pub struct Night {
    /// `Sky::sun_elevation`: 1 at noon, 0 on the horizon, -1 at midnight.
    pub sun: f32,
    /// Degrees over freezing here and now, the night's cooling taken off
    /// (`Critters::air_here`) -- the number the frogs wake by.
    pub warmth: f32,
    pub biome: Biome,
    pub wet: bool,
    pub shelter: Shelter,
    /// Out on the water: the look around came back mostly water.
    pub at_sea: bool,
}

/// Degrees over freezing the grass has to be for a cricket to sing at all,
/// and for the whole field to. **Measured against the night, not the day**:
/// a cricket is cold-blooded and its song slows and stops with the air it
/// is in, so a mild autumn afternoon followed by a frost is a chorus that
/// thins through the evening and is gone by midnight. The frogs learned
/// this the hard way (`critters::FROG_WARMTH_C`, "лягушки игнорируют
/// температуру"), and the floor is theirs: below eight the grass is silent.
const CRICKET_COLD: f32 = 8.0;
const CRICKET_WARM: f32 = 16.0;

/// How loud the crickets are, 0 to 1.
///
/// **Grass, warmth and dark, and any one of them can silence it**, so the
/// answer is a product:
///
/// * **the dark** -- a fade in from just before sunset to just after, and
///   out again at dawn, so the evening has a moment where the day is over
///   and the field has not yet started;
/// * **the warmth** -- nothing below eight degrees over freezing and the
///   whole field above sixteen, which is what makes winter, a cold spring
///   and the north silent without a word about seasons here;
/// * **the country** -- a meadow, a steppe and a savanna are the grass
///   crickets live in; a wood sings less (its floor is litter, not grass), a
///   marsh somewhat, a desert and a beach a little; the sea, the tundra and
///   the peaks not at all;
/// * **the weather and the roof** -- rain silences a field (a wet wing does
///   not ring, and the rain is louder anyway); under a roof with the sky a
///   step away the field is heard through the wall, and deep in a cave or a
///   mine it is not heard at all.
///
/// Rejected: a clock ("crickets from nine till four"). A day is twenty
/// minutes and the sun is what a player reads the evening by; a chorus that
/// started by a clock would start on a bright horizon at midsummer.
pub fn cricket_chorus(night: &Night) -> f32 {
    let dark = ((0.08 - night.sun) / 0.2).clamp(0.0, 1.0);
    let warm = ((night.warmth - CRICKET_COLD) / (CRICKET_WARM - CRICKET_COLD)).clamp(0.0, 1.0);
    let grass = match night.biome {
        Biome::Plains | Biome::Steppe | Biome::Savanna | Biome::Hills => 1.0,
        Biome::Swamp | Biome::Bog | Biome::River => 0.55,
        Biome::Forest | Biome::BirchForest | Biome::DeadForest => 0.4,
        Biome::Taiga | Biome::Desert | Biome::Beach => 0.25,
        Biome::Mountains => 0.15,
        Biome::Ocean | Biome::Tundra | Biome::SnowyPeaks => 0.0,
    };
    if night.wet || night.at_sea {
        return 0.0;
    }
    dark * warm * grass * heard_through(night.shelter)
}

/// How far round a moving player the crickets fall silent, in blocks. A
/// cricket stops when the grass near it shakes, and it is the one thing in
/// the night that tells a player they are being loud: walk and a ring of
/// quiet walks with you, stand still and the field closes in again.
const CRICKET_HUSH: f32 = 7.0;
/// ...and where the nearest start again when nothing moves.
const CRICKET_NEAREST: f32 = 1.5;
/// Blocks a second the silence closes back in: several seconds of standing
/// still before the near ones trust it.
const CRICKET_TRUST: f32 = 0.8;
/// Faster than this across the ground, in blocks a second, is moving --
/// under a creep, because crickets do not know about sneaking.
const CRICKET_STILL: f32 = 0.5;

/// The hush after one frame: out at once when something moves, back slowly
/// when it stops.
pub fn cricket_hush(hush: f32, moving: bool, dt: f32) -> f32 {
    if moving {
        CRICKET_HUSH
    } else {
        (hush - CRICKET_TRUST * dt).max(CRICKET_NEAREST)
    }
}

// ------------------------------------------------ the country's own bed
//
// **Three beds the weather is not.** Rain, wind and thunder are what is
// *falling*; these are what is *living* where the player is standing, at
// this hour of this year. Before them a meadow at noon in July and the
// same meadow at noon in February were the same silence with a different
// colour of grass in it.
//
// **The season reaches all three as a temperature and as a direction,
// never as an enum.** `Season::at` is a step function -- one frame it is
// autumn and the next it is winter -- and a bed whose loudness was read
// off it would change by a whole step between two frames, which is the
// one thing a continuous sound must never do. The crickets already
// learned this ("what makes winter, a cold spring and the north silent
// without a word about seasons here"); what the two new beds needed on
// top of warmth was a way to tell a thaw from a freeze at the same six
// degrees, and that is [`thawing`], which is a sine and not a step.

/// What the air round the player is doing, as far as anything that sings
/// in it cares.
///
/// A struct of facts for the reason [`Night`] is one: whether the steppe
/// rings at noon is a question a test should answer with a literal
/// rather than with a chunk manager and a sky.
#[derive(Debug, Clone, Copy)]
pub struct Air {
    /// `Sky::sun_elevation`: 1 at noon, 0 on the horizon, -1 at midnight.
    pub sun: f32,
    /// Before noon. What makes the morning's birds louder than the
    /// evening's -- see [`dawn_chorus`].
    pub morning: bool,
    /// Degrees over freezing here and now, the same number the crickets
    /// and the frogs wake by (`Critters::air_here`). **This is where the
    /// season is**: midsummer is [`primitive_shared::season::SUMMER_PEAK_C`]
    /// warmer than the generator's climate and midwinter ten colder.
    pub warmth: f32,
    /// How hard the year is warming, 0..1 -- see [`thawing`].
    pub thaw: f32,
    pub biome: Biome,
    pub wet: bool,
    pub shelter: Shelter,
    /// Out on the water: the look around came back mostly water.
    pub at_sea: bool,
}

/// How hard the year is warming at `world_time`, 0..1: nothing at either
/// solstice, one at the steepest part of the climb out of winter.
///
/// **A sine of the day of the year, clamped at zero.** The warming half
/// of the year is midwinter to midsummer, and its steepest point is the
/// spring equinox -- which is what a thaw is, and what tells spring from
/// an autumn at the same temperature. The other half comes out negative
/// and is clamped away: nothing drips in October.
///
/// Rejected: `Season::at(t) == Season::Spring`. It is a step, and a bed
/// whose level stepped would click on the stroke of a season -- the one
/// artefact that makes a generated ambience sound like a recording being
/// switched on. See `no_bed_ever_jumps_over_a_whole_year`.
pub fn thawing(world_time: f32) -> f32 {
    use primitive_shared::season::{day_of_year, MIDWINTER_DAY_OF_YEAR, YEAR_DAYS};
    if !world_time.is_finite() {
        return 0.0;
    }
    let phase = (day_of_year(world_time) - MIDWINTER_DAY_OF_YEAR) / YEAR_DAYS;
    (std::f32::consts::TAU * phase).sin().max(0.0)
}

/// Degrees over freezing before the cicadas start, and where the whole
/// field is going.
///
/// **Twenty and twenty-eight, against the crickets' eight and sixteen**,
/// and the gap is the point: a cricket sings on a mild evening anywhere
/// with grass in it, and a cicada is a *hot* sound. Twenty over freezing
/// is a summer noon in the temperate middle of the map and is never
/// reached at all in the taiga; twenty-eight belongs to the steppe and
/// the savanna in the middle of summer. So the same code gives a meadow
/// a few cicadas on the hottest days of July and gives the steppe a
/// solid wall of them, without a word about latitude here.
const CICADA_COLD: f32 = 20.0;
const CICADA_WARM: f32 = 28.0;

/// How loud the cicadas are, 0 to 1.
///
/// A product, like [`cricket_chorus`], and for the same reason -- any
/// one of these can silence a field:
///
/// * **the sun** -- the mirror of the crickets. A cicada sings in the
///   heat of the day and stops at dusk, so this fades in as the sun
///   climbs clear of the horizon rather than as it drops below it. The
///   two beds therefore hand over at twilight, which is what an evening
///   in a hot place actually sounds like;
/// * **the heat** -- see [`CICADA_COLD`]. This is the season, arriving
///   as a temperature;
/// * **the country** -- dry open grass and scrub. A steppe or a savanna
///   is what this sound *is*; a wood has some, a marsh a few, the taiga
///   nearly none, and the sea, the tundra and the peaks none at all;
/// * **the weather and the roof** -- rain silences it outright, and a
///   wall muffles it, exactly as for the crickets.
pub fn cicada_chorus(air: &Air) -> f32 {
    // Clear of the horizon, not merely up: the first hour of daylight is
    // the birds' and not the cicadas'.
    let day = ((air.sun - 0.12) / 0.25).clamp(0.0, 1.0);
    let hot = ((air.warmth - CICADA_COLD) / (CICADA_WARM - CICADA_COLD)).clamp(0.0, 1.0);
    let country = match air.biome {
        Biome::Steppe | Biome::Savanna => 1.0,
        Biome::Desert => 0.8,
        Biome::Plains | Biome::Hills => 0.65,
        Biome::Forest | Biome::BirchForest => 0.5,
        Biome::DeadForest | Biome::Beach => 0.3,
        Biome::Swamp | Biome::Bog | Biome::River => 0.35,
        Biome::Taiga | Biome::Mountains => 0.1,
        Biome::Ocean | Biome::Tundra | Biome::SnowyPeaks => 0.0,
    };
    if air.wet || air.at_sea {
        return 0.0;
    }
    day * hot * country * heard_through(air.shelter)
}

/// Degrees over freezing the birds need. Much lower than the cicadas'
/// and a little over the crickets': a bird is warm-blooded and sings on
/// a cold April morning, and what actually stops it is a hard frost.
const BIRD_COLD: f32 = 3.0;
const BIRD_WARM: f32 = 11.0;
/// How near the horizon the sun has to be for the birds to be at it, in
/// the units `Sky::sun_elevation` reports.
///
/// A quarter either way, which at this day length is roughly the hour
/// before and after sunrise and the same at sunset. **Wider than the
/// crickets' fade** (0.2 from full to nothing), because a dawn chorus is
/// not an on-off: it starts in the dark, peaks as the light comes, and
/// thins out as the day gets on.
const BIRD_TWILIGHT: f32 = 0.25;
/// How much of the morning's chorus the evening gets.
///
/// **Not the same, and this is the one asymmetry in the module.** Birds
/// sing hardest at dawn -- it is territory, and it is settled at first
/// light -- and the evening is a handful of calls going to roost. Equal
/// mornings and evenings would throw away the one thing this bed can
/// tell a player who has lost track of the clock underground: which end
/// of the day they have come up into.
const BIRD_EVENING: f32 = 0.45;

/// How loud the birds are at this hour, 0 to 1.
///
/// **The hour either side of sunrise and sunset**, which is where a
/// chorus is; and warm enough that there is something to sing about,
/// which is what keeps a midwinter morning silent without naming a
/// season. Rain thins it rather than stopping it -- birds do sing in
/// drizzle, and the rain is louder anyway.
pub fn dawn_chorus(air: &Air) -> f32 {
    let twilight = (1.0 - air.sun.abs() / BIRD_TWILIGHT).clamp(0.0, 1.0);
    let mild = ((air.warmth - BIRD_COLD) / (BIRD_WARM - BIRD_COLD)).clamp(0.0, 1.0);
    // Hedges, wood edges and meadows. A desert, a peak and the open sea
    // have birds and they are not a chorus.
    let country = match air.biome {
        Biome::Forest | Biome::BirchForest => 1.0,
        Biome::Plains | Biome::Hills | Biome::River => 0.8,
        Biome::Swamp | Biome::Bog => 0.6,
        Biome::Taiga | Biome::Steppe | Biome::Savanna => 0.5,
        Biome::DeadForest => 0.25,
        Biome::Beach => 0.3,
        Biome::Desert | Biome::Mountains | Biome::Tundra => 0.15,
        Biome::Ocean | Biome::SnowyPeaks => 0.0,
    };
    if air.at_sea {
        return 0.0;
    }
    let hour = if air.morning { 1.0 } else { BIRD_EVENING };
    let dry = if air.wet { 0.4 } else { 1.0 };
    twilight * mild * country * hour * dry * heard_through(air.shelter)
}

/// The window, in degrees over freezing, where there is both ice left to
/// melt and enough warmth to melt it.
///
/// **Both ends matter.** At or under freezing nothing runs, and eight
/// degrees into a thaw the snow that was on the roof has been gone for a
/// day. The drip is the *edge* of winter, which is the point of it: it
/// is the sound of the season turning, and it is over in a few days.
const THAW_COLD: f32 = 0.5;
const THAW_WARM: f32 = 8.0;

/// How much is running off the eaves, 0 to 1.
///
/// **The one bed that needs to know which way the year is going**, and
/// the reason [`thawing`] exists: six degrees over freezing in a warming
/// year is a thaw and in a cooling one is a wet November, and no
/// temperature can tell them apart.
///
/// Loudest under a roof or a canopy, because that is what a drip *is* --
/// water leaving an edge above your head. Out in the open there is some
/// of it off whatever is standing about; underground there is none, and
/// rain drowns the lot.
pub fn eaves_drip(air: &Air) -> f32 {
    if air.wet || air.at_sea {
        return 0.0;
    }
    let melting = if air.warmth <= THAW_COLD {
        0.0
    } else {
        // Up fast and down slowly: the first degree over freezing is
        // most of the effect, and what takes the rest of it away is the
        // snow running out.
        let t = ((air.warmth - THAW_COLD) / (THAW_WARM - THAW_COLD)).clamp(0.0, 1.0);
        (1.0 - t) * (1.0 - t) * 4.0 * t
    };
    // Where there was snow to begin with. A desert does not drip.
    let country = match air.biome {
        Biome::Taiga | Biome::SnowyPeaks | Biome::Tundra | Biome::Mountains => 1.0,
        Biome::Forest | Biome::BirchForest | Biome::DeadForest => 0.8,
        Biome::Plains | Biome::Hills | Biome::River | Biome::Swamp | Biome::Bog => 0.7,
        Biome::Steppe => 0.5,
        Biome::Beach | Biome::Savanna => 0.25,
        Biome::Desert | Biome::Ocean => 0.0,
    };
    let under = match air.shelter {
        Shelter::Roofed | Shelter::Canopy => 1.0,
        Shelter::Open => 0.55,
        // Deep underground. There is no sky for the snow to have been on.
        Shelter::Enclosed => 0.0,
    };
    air.thaw * melting * country * under
}

/// How much of what is going on outside is heard through whatever is
/// over the player's head.
///
/// The crickets' numbers, pulled out because three beds now want them
/// and three copies is three chances to disagree about what a roof is.
fn heard_through(shelter: Shelter) -> f32 {
    match shelter {
        Shelter::Open | Shelter::Canopy => 1.0,
        Shelter::Roofed => 0.45,
        Shelter::Enclosed => 0.0,
    }
}

/// How fast an ambient level is allowed to change, per second.
///
/// **The whole of "no bed ever clicks".** Every one of the functions
/// above is a product of things that can move quickly -- the player
/// walks under a roof, crosses a biome edge, steps out of the rain --
/// and a bed whose gain followed them frame for frame would jump. A
/// third of the way a second means a bed takes about three seconds to
/// come up or go down: slower than one piece of it is long
/// (`bank::BED_SECONDS` less its crossfade), so two consecutive pieces
/// are never more than a fraction apart and the seam between them cannot
/// be heard.
///
/// Rejected: an exponential follow (`level += (wanted - level) * k`). It
/// never actually arrives, so a bed that should be silent goes on
/// playing pieces at a millionth of full gain for ever, and every one of
/// them is a voice the mixer has spent.
const BED_SLEW: f32 = 0.34;

/// A level that is only ever eased toward what the world asks for.
///
/// One field and one method, and it exists so that "the background does
/// not click" is a property of a *type* rather than a thing three call
/// sites each remember to do.
#[derive(Debug, Clone, Copy, Default)]
pub struct Level(f32);

impl Level {
    /// Moves one frame toward `wanted` and returns where it got to.
    pub fn ease(&mut self, wanted: f32, dt: f32) -> f32 {
        let wanted = if wanted.is_finite() { wanted.clamp(0.0, 1.0) } else { 0.0 };
        let step = BED_SLEW * dt.clamp(0.0, 0.25);
        self.0 += (wanted - self.0).clamp(-step, step);
        self.0 = self.0.clamp(0.0, 1.0);
        self.0
    }
}

/// Under this a bed is not laid at all.
///
/// A voice at a fiftieth of full gain is inaudible and costs the mixer
/// exactly as much as a loud one. The slew above guarantees a level
/// actually reaches zero, so this is a floor and not a fudge.
const BED_FLOOR: f32 = 0.02;

/// How much faster the cicadas' bed runs than the crickets'.
///
/// **The crickets' own recording, played high.** A cicada and a cricket
/// are the same event -- a file on one wing drawn across a scraper on
/// the other -- at different sizes, and what separates them to an ear is
/// rate and pitch. The alternative was a download, and it would have
/// been a download of very nearly this.
///
/// **A pitched bed is a shorter bed**, which is the trap here: the
/// pieces are laid end over end against `bank::BED_SECONDS`, and a piece
/// played half again as fast ends half again as early. The interval is
/// divided by this, or the wall of sound comes out as a wall with gaps
/// in it -- the exact fault the fire's comment warns about, arrived at
/// from the other direction.
const CICADA_PITCH: f32 = 1.5;

/// Seconds between one bird and the next at a full chorus, and at the
/// thinnest one that is heard at all.
///
/// **Calls, not a bed.** A chorus laid end over end would be one
/// continuous sound, and what a dawn chorus is, is many small sounds
/// from many directions -- so this is a rate, and the level sets the
/// rate as well as the gain. At full it is about two a second from all
/// round the player; at the very edge of the window it is one every five
/// seconds from somewhere off in the trees.
const BIRD_GAP: (f32, f32) = (0.45, 5.0);

/// ...and the same for the thaw: a drip is a drip and never a bed.
const DRIP_GAP: (f32, f32) = (0.35, 3.5);

// -------------------------------------------------------------- hooves

/// How far a horse's feet are heard, and how many horses at once. A herd
/// of twelve galloping by is heard as the nearest four: past that the drum
/// is one sound, and twelve beds of it are only twelve of the mixer's
/// voices spent.
const HOOF_HEARING: f32 = 40.0;
const HOOVES_HEARD: usize = 4;

/// Under this, in blocks a second, a horse is standing, and its feet say
/// nothing: shifting its weight is not a stride.
const HOOF_STANDING: f32 = 0.8;

/// Which pace a horse is going at, from how fast it is going.
///
/// **From the speed, the one thing the client sees of every horse**, ridden
/// or wild: a rider's gait is their reins, but a stranger's horse on a
/// server is a speed in a snapshot, and one rule for both is one rule that
/// cannot disagree with itself. The lines are halfway between the paces
/// `horse::Gait` holds a horse to, so a horse coming down from a gallop is
/// heard to trot when it has come most of the way down, not at the first
/// stride less.
pub fn pace_of(speed: f32) -> Option<Gait> {
    use primitive_shared::horse::{GALLOP, TROT, WALK};
    if speed < HOOF_STANDING {
        None
    } else if speed < (WALK + TROT) * 0.5 {
        Some(Gait::Walk)
    } else if speed < (TROT + GALLOP) * 0.5 {
        Some(Gait::Trot)
    } else {
        Some(Gait::Gallop)
    }
}

/// One horse, as its hooves need it.
#[derive(Debug, Clone, Copy)]
pub struct Hoof {
    pub id: EntityId,
    pub at: glam::DVec3,
    /// Along the ground; nought for a horse in water, which splashes rather
    /// than strikes.
    pub speed: f32,
    pub footing: Footing,
}

/// One piece of a pace, to be played.
#[derive(Debug, Clone, Copy)]
pub struct HoofBeat {
    pub sfx: Sfx,
    pub at: glam::DVec3,
    pub gain: f32,
    pub pitch: f32,
}

/// The hooves of every horse near the player.
///
/// **Pieces of a pace laid end over end, not a clop per stride.** The
/// recordings are a stride or two of a real horse at a walk, a trot or a
/// gallop (`bank::hoof_piece_seconds`), and each horse's next piece starts
/// as its last one ends. A clop per footfall timed from the speed was the
/// alternative, and it is a metronome: a walk is four beats in a lopsided
/// rhythm and a gallop is three in a rush and a gap, and that rhythm is
/// what a listener names the pace by. A change of pace plays at once
/// rather than when the old piece ends, because the moment a horse breaks
/// into a gallop is the moment worth hearing.
///
/// The rider's own horse is one of these like any other, heard from under
/// the saddle: the player's own footsteps are silent on horseback (the
/// body's velocity is nought there -- `lib.rs` puts it on the saddle), so
/// these are the only feet.
pub struct Hoofbeats {
    /// Each horse's pace, and seconds until its next piece.
    going: Vec<(EntityId, Gait, f32)>,
}

impl Default for Hoofbeats {
    fn default() -> Self {
        Hoofbeats::new()
    }
}

impl Hoofbeats {
    pub fn new() -> Hoofbeats {
        Hoofbeats { going: Vec::new() }
    }

    /// One frame of listening to `horses`.
    pub fn hear(&mut self, dt: f32, horses: &[Hoof]) -> Vec<HoofBeat> {
        let mut going = Vec::with_capacity(horses.len());
        let mut beats = Vec::new();
        for horse in horses {
            let Some(gait) = pace_of(horse.speed) else {
                continue;
            };
            let left = match self.going.iter().find(|(id, _, _)| *id == horse.id) {
                Some(&(_, was, left)) if was == gait => left - dt,
                _ => 0.0,
            };
            let left = if left <= 0.0 {
                // A little faster than the pace plays a little faster, within
                // the few per cent a recording stretches before it sounds
                // played rather than ridden.
                let pitch = (horse.speed / gait.speed()).clamp(0.9, 1.12);
                let gain = match gait {
                    Gait::Walk => 0.5,
                    Gait::Trot => 0.7,
                    Gait::Gallop => 0.9,
                };
                beats.push(HoofBeat { sfx: Sfx::Hoofs(gait, horse.footing), at: horse.at, gain, pitch });
                // The next piece starts as this one's last strike fades: the
                // pieces carry a 30 ms fade at each end.
                left + (bank::hoof_piece_seconds(gait) / pitch - 0.03).max(0.2)
            } else {
                left
            };
            going.push((horse.id, gait, left));
        }
        self.going = going;
        beats
    }
}

/// Seconds between gusts, and how loud one is.
///
/// **The gap matters more than the gain.** A gust is an event, and what
/// makes a squall a squall is that the events stop leaving room between
/// themselves: every forty seconds in a light air, every ten or so when
/// it is blowing hard. Scaling only the loudness would give a wind that
/// is always breathing at the same rate and merely leaning on it, which
/// is what a volume knob sounds like.
fn gusting(wind: f32) -> (f32, f32) {
    let wind = wind.clamp(0.0, 1.0);
    // Never zero in the denominator, and never faster than about one
    // every seven seconds at the top: a gust that arrives before the
    // last one has died is a loop.
    (24.0 / (0.35 + wind), 0.12 + 0.34 * wind)
}

/// Where the player is, as far as the music is concerned.
#[derive(Debug, Clone, Copy)]
pub struct Surroundings {
    pub health_fraction: f32,
    pub outdoors: bool,
    pub height: f32,
    pub weather: Weather,
    pub night: bool,
    /// A lit fire within [`HEARTH_RANGE`].
    pub fire_near: bool,
    /// What the ring of probes found -- see `look_around`.
    pub water_share: f32,
    pub leaf_share: f32,
    /// In the water, or swimming in it.
    pub afloat: bool,
}

/// Which mood the music should be in.
///
/// **The order is a priority and it is the whole of the design: the
/// most specific true thing wins.** Nearly dying beats everything; a
/// fire with a roof over it beats being underground, because a hearth
/// is a place you made and a cave is one you are in; underground beats
/// the weather, which beats where you are, which beats the hour.
///
/// The two arguable lines, written down because they *were* arguments:
///
/// * **the sea beats the night.** A crossing is the fact about a player
///   who is doing one, and it does not stop being the fact after dark;
/// * **the night beats the forest.** The trees are still there in the
///   morning, and what changes what a player does at dusk is the dark.
///   A forest is a daytime mood on purpose.
///
/// Four of these are new. Before them a thunderstorm and a drizzle were
/// the same piece of music, and a raft in the middle of the sea, a
/// clearing in a wood and a hut with a fire in it were all `Day`.
pub fn mood_for(place: Surroundings) -> Mood {
    if place.health_fraction < 0.35 {
        return Mood::Peril;
    }
    if place.fire_near && !place.outdoors {
        return Mood::Hearth;
    }
    if !place.outdoors && place.height < 58.0 {
        return Mood::Cave;
    }
    if matches!(place.weather, Weather::Storm) {
        return Mood::Storm;
    }
    if place.weather.is_wet() {
        return Mood::Rain;
    }
    // Being in the water counts for as much as being surrounded by it:
    // a swimmer in a channel is at sea, and a player on a raft in the
    // middle of a lake is surrounded by it without touching it.
    if place.outdoors && (place.water_share >= AT_SEA || (place.afloat && place.water_share > 0.35))
    {
        return Mood::Sea;
    }
    if place.night {
        return Mood::Night;
    }
    if place.leaf_share >= IN_FOREST {
        return Mood::Forest;
    }
    Mood::Day
}

/// Is there open sky above this position?
///
/// A column of lookups, stopping at the first thing light would not get
/// through. Capped at [`ROOF_PROBE`] rather than run to the top of the
/// world: past fifty blocks of clear air the answer stops changing, and
/// the cap is what keeps this affordable at all.
fn sky_visible(chunks: &ChunkManager, position: Vec3) -> bool {
    let x = position.x.floor() as i32;
    let z = position.z.floor() as i32;
    let head = (position.y + 1.6).floor() as i32;
    let top = (head + ROOF_PROBE).min(CHUNK_SIZE_Y as i32 - 1);
    for y in (head + 1)..=top {
        match chunks.block_at(x, y, z) {
            // A chunk that is not loaded is not a roof. Assuming the
            // opposite would put a player briefly in a cave every time
            // they outran the streamer.
            None => continue,
            Some(block) => {
                if types::is_opaque(block) {
                    return false;
                }
            }
        }
    }
    true
}

/// The nearest lit fire, hearth or kiln within [`FIRE_RANGE`].
///
/// A scan rather than an index, for the reason `main.rs` gives about
/// working range: keeping a list of "where are the fires" in step with
/// every block update is a structure to maintain, and this is answered
/// about once a second rather than once a frame.
///
/// Nearest rather than all of them: several fires in a room would
/// otherwise crackle in chorus, which sounds like a bug, and one that is
/// clearly in the right direction is what a player is actually listening
/// for.
fn nearest_fire(chunks: &ChunkManager, position: Vec3) -> Option<(i32, i32, i32)> {
    let cx = position.x.floor() as i32;
    let cy = (position.y + 1.0).floor() as i32;
    let cz = position.z.floor() as i32;
    let mut best: Option<((i32, i32, i32), i32)> = None;
    for y in (cy - FIRE_RANGE)..=(cy + FIRE_RANGE) {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            continue;
        }
        for x in (cx - FIRE_RANGE)..=(cx + FIRE_RANGE) {
            for z in (cz - FIRE_RANGE)..=(cz + FIRE_RANGE) {
                let Some(block) = chunks.block_at(x, y, z) else {
                    continue;
                };
                if !types::is_burning(block) {
                    continue;
                }
                let dx = x - cx;
                let dy = y - cy;
                let dz = z - cz;
                let distance = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                    best = Some(((x, y, z), distance));
                }
            }
        }
    }
    best.map(|(cell, _)| cell)
}

/// What one stride sounds like.
///
/// Wading is a different noise from walking, whatever is under the water
/// -- and a different noise from swimming too. It played `Swim`, so every
/// stride along a shore was a swimmer's arm slapping the surface.
fn stride_sound(in_water: bool, ground: Material) -> Sfx {
    if in_water {
        Sfx::Wade
    } else {
        Sfx::Material(Impact::Step, ground)
    }
}

/// How loud the tool that struck a blow is heard under the block it
/// struck, and at what pitch -- `None` for a fist and for anything in
/// the hand that is not a tool.
///
/// **A second sound rather than a different one.** The alternative
/// considered was one recording per (tool material, block material)
/// pair, which is a hundred and forty-four downloads for a distinction
/// the bank already carries in twelve. What a blow actually is, is two
/// things meeting: the rock rings, and under it there is a hard edge of
/// copper or a lashed flake of flint. Both clips are already here, so
/// the pair costs a second voice and nothing else.
///
/// **Quiet, and quieter still for a light head.** The block is what the
/// player is working on and has to stay the loud half: a tool that
/// matched it would sound like two blocks being hit at once. A third of
/// the block's gain at the heavy end, an eighth at the light, so a flint
/// knife is nearly all rock and an iron pick is a ringing edge.
///
/// **Lower for a heavy head**, by up to a fifth -- which is the one
/// thing an ear reads mass by without being told. The same reasoning as
/// `hand::heft`, which is where the weight comes from.
fn tool_voice(held: Option<BlockId>) -> Option<(Material, f32, f32)> {
    let id = held?;
    // A held block is not a striking head: setting a slab of stone
    // against a rock face is not a blow with it. `heft` gives a block a
    // weight because it is carried; this asks a narrower question.
    primitive_shared::blocks::definition(types::block_kind(id)).tool?;
    let heft = crate::logic::hand::heft(held);
    Some((Material::of(id), 0.12 + 0.18 * heft, 1.08 - 0.2 * heft))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stride_through_water_is_wading_and_never_a_swimming_stroke() {
        for ground in [Material::Dirt, Material::Sand, Material::Stone, Material::Gravel] {
            assert_eq!(stride_sound(true, ground), Sfx::Wade);
            assert_eq!(stride_sound(false, ground), Sfx::Material(Impact::Step, ground));
        }
    }

    #[test]
    fn a_drain_of_crumbs_is_not_a_punch_a_tick_and_a_blow_is_heard_at_once() {
        let mut scape = Soundscape::new();
        let start = std::time::Instant::now();
        // A sick player: a twentieth of a point twenty times a second, for
        // ten seconds. Heard no more than once in each 0.7 s, and far fewer
        // times than the 200 ticks.
        let mut heard = 0;
        for tick in 0..200u64 {
            if scape.hurt_due(0.05, start + std::time::Duration::from_millis(tick * 50)) {
                heard += 1;
            }
        }
        assert!(heard <= 10, "a drain played the hurt sound {heard} times in ten seconds");
        assert!(heard >= 1, "ten points of health went and nothing was heard");
        // ...and a bite after a quiet second is heard straight away.
        let later = start + std::time::Duration::from_secs(12);
        assert!(scape.hurt_due(2.0, later), "a bite was not heard");
        assert!(!scape.hurt_due(2.0, later + std::time::Duration::from_millis(100)), "two bites 0.1 s apart were both heard");
    }
    use primitive_shared::types::{ChunkPos, BLOCK_AIR, BLOCK_CAMPFIRE_LIT, BLOCK_STONE};

    /// A world one chunk wide, filled to `height` with stone.
    fn flat_world(height: usize) -> ChunkManager {
        let mut chunks = ChunkManager::new(4);
        let mut chunk = primitive_shared::types::Chunk::generate_flat(ChunkPos::new(0, 0));
        for x in 0..primitive_shared::types::CHUNK_SIZE_X {
            for z in 0..primitive_shared::types::CHUNK_SIZE_Z {
                for y in 0..CHUNK_SIZE_Y {
                    chunk.set(x, y, z, if y < height { BLOCK_STONE } else { BLOCK_AIR });
                }
            }
        }
        chunks.insert(chunk);
        chunks
    }

    #[test]
    fn open_sky_is_open_and_a_roof_is_not() {
        let chunks = flat_world(8);
        assert!(sky_visible(&chunks, Vec3::new(4.0, 8.0, 4.0)));

        let mut roofed = flat_world(8);
        let mut chunk = roofed.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(4, 14, 4, BLOCK_STONE);
        roofed.insert(chunk);
        assert!(!sky_visible(&roofed, Vec3::new(4.5, 8.0, 4.5)));
    }

    #[test]
    fn a_fire_is_found_and_an_unlit_one_is_not() {
        let mut chunks = flat_world(8);
        assert_eq!(nearest_fire(&chunks, Vec3::new(4.5, 8.0, 4.5)), None);

        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(6, 8, 6, BLOCK_CAMPFIRE_LIT);
        chunks.insert(chunk);
        assert_eq!(
            nearest_fire(&chunks, Vec3::new(4.5, 8.0, 4.5)),
            Some((6, 8, 6))
        );
    }

    /// The ground under the feet is the first solid thing *below* them,
    /// not whatever shares their cell.
    #[test]
    fn the_ground_is_what_is_under_the_feet() {
        let chunks = flat_world(8);
        let scape = Soundscape::new();
        assert_eq!(
            scape.ground_material(&chunks, Vec3::new(4.5, 8.0, 4.5)),
            Material::Stone
        );
        // Standing in mid-air over nothing: something plausible rather
        // than a panic.
        assert_eq!(
            scape.ground_material(&chunks, Vec3::new(4.5, 40.0, 4.5)),
            Material::Dirt
        );
    }

    /// **Rain used to be one steady hiss whatever the wind did.** The
    /// drops on the screen already thickened in a squall -- the same
    /// `0.65 + 0.5 * strength` this uses -- and the sound of them did
    /// not, so the window and the speaker disagreed about the weather.
    #[test]
    fn rain_thickens_with_the_wind_and_does_not_stop_without_it() {
        let storm = Weather::Storm.intensity();
        let calm = raining(storm, 0.0);
        let squall = raining(storm, 1.0);
        assert!(
            squall.gain > calm.gain * 1.5,
            "a squall is only {:.0}% louder than a dead calm",
            100.0 * (squall.gain / calm.gain - 1.0)
        );
        assert!(squall.drips > calm.drips);
        // ...and the calm end is rain, not silence. Rain with no wind in
        // it is still rain.
        assert!(calm.gain > squall.gain * 0.5, "a calm downpour plays at {:.2}", calm.gain);
        // Heavier weather is heavier at either end of the wind.
        for wind in [0.0f32, 0.5, 1.0] {
            assert!(raining(Weather::Storm.intensity(), wind).gain > raining(Weather::Rain.intensity(), wind).gain);
        }
    }

    /// **Under a roof the rain is duller, not merely quieter, and in a
    /// cave it is gone.** The open bed has the hiss that says "outside";
    /// the roof keeps the patter and adds the drip off the eaves; a canopy
    /// is the open rain with the leaves dripping on top of it.
    #[test]
    fn rain_is_heard_by_how_the_player_is_sheltered_from_it() {
        assert_eq!(shelter_from(true, false, 0), Shelter::Open);
        assert_eq!(shelter_from(true, true, 0), Shelter::Canopy);
        assert_eq!(shelter_from(false, false, 3), Shelter::Roofed);
        assert_eq!(shelter_from(false, true, 1), Shelter::Roofed);
        assert_eq!(shelter_from(false, false, 0), Shelter::Enclosed);

        let (open, open_gain, open_drips) = rain_heard(Shelter::Open).unwrap();
        let (roof, roof_gain, roof_drips) = rain_heard(Shelter::Roofed).unwrap();
        let (canopy, _, canopy_drips) = rain_heard(Shelter::Canopy).unwrap();
        assert_eq!(open, Sfx::Rain);
        assert_eq!(canopy, Sfx::Rain);
        assert_eq!(roof, Sfx::RainSheltered, "a roof plays the open-sky rain turned down");
        assert!(roof_gain < open_gain);
        assert_eq!(open_drips, 0.0, "the open sky drips off nothing");
        assert!(roof_drips > 0.0 && canopy_drips > 0.0);
        assert!(rain_heard(Shelter::Enclosed).is_none(), "rain is heard at the bottom of a mine");
    }

    #[test]
    fn a_roof_over_the_head_with_sky_beside_it_is_shelter_and_a_cave_is_not() {
        // Open ground.
        let chunks = flat_world(8);
        assert_eq!(shelter_at(&chunks, Vec3::new(8.5, 8.0, 8.5)), Shelter::Open);
        // One slab over the head, sky all round it.
        let mut roofed = flat_world(8);
        let mut chunk = roofed.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(8, 12, 8, BLOCK_STONE);
        roofed.insert(chunk);
        assert_eq!(shelter_at(&roofed, Vec3::new(8.5, 8.0, 8.5)), Shelter::Roofed);
        // Buried: stone to well over the head everywhere.
        let buried = flat_world(40);
        assert_eq!(shelter_at(&buried, Vec3::new(8.5, 8.0, 8.5)), Shelter::Enclosed);
    }

    /// **Three winds, each where it belongs**: the leaves in a light air
    /// or among trees, a gust over open ground, a howl up high or in a
    /// storm -- and the storm howls even in a wood.
    #[test]
    fn each_wind_is_heard_where_it_belongs() {
        let meadow = 70.0;
        assert_eq!(wind_kind(0.1, meadow, 0.0, false), Sfx::WindBreeze, "a light air in a meadow");
        assert_eq!(wind_kind(0.5, meadow, 0.0, false), Sfx::Wind, "a fair wind over open ground");
        assert_eq!(wind_kind(0.5, meadow, 0.6, false), Sfx::WindBreeze, "the same wind among trees");
        assert_eq!(wind_kind(0.5, HIGH_GROUND + 10.0, 0.0, false), Sfx::WindHowl, "a fair wind on a ridge");
        assert_eq!(wind_kind(0.1, HIGH_GROUND + 10.0, 0.0, false), Sfx::WindBreeze, "a still day on a ridge");
        assert_eq!(wind_kind(0.9, meadow, 0.0, false), Sfx::WindHowl, "a gale anywhere");
        assert_eq!(wind_kind(0.3, meadow, 0.8, true), Sfx::WindHowl, "a storm, even in a wood");
    }

    /// **A rustle only for moving through leaves**: nothing standing in a
    /// bush, nothing walking in the open, and a run through a thicket both
    /// oftener and louder than a creep.
    #[test]
    fn a_rustle_is_heard_only_when_something_moves_through_foliage() {
        let dt = 1.0 / 60.0;
        let count = |speed: f32, thickness: f32, seconds: f32| {
            let mut brush = Brush::new();
            let mut heard = Vec::new();
            for _ in 0..(seconds / dt) as usize {
                if let Some(gain) = brush.advance(speed * dt, speed, thickness) {
                    heard.push(gain);
                }
            }
            heard
        };
        assert!(count(0.0, 1.0, 5.0).is_empty(), "standing in a bush rustled");
        assert!(count(4.0, 0.0, 5.0).is_empty(), "walking in the open rustled");
        let creep = count(1.0, 1.0, 5.0);
        let run = count(4.0, 1.0, 5.0);
        assert!(!creep.is_empty(), "creeping through a bush was silent");
        assert!(run.len() > creep.len() * 2, "a run ({}) is not oftener than a creep ({})", run.len(), creep.len());
        let loudest = |v: &[f32]| v.iter().fold(0.0f32, |m, g| m.max(*g));
        assert!(loudest(&run) > loudest(&creep));
        // Tall stems at the feet are softer than a canopy round the body.
        assert!(loudest(&count(4.0, 0.55, 5.0)) < loudest(&run));
        // Stepping into a bush is heard at once, not a stride later.
        let mut brush = Brush::new();
        assert!(brush.advance(0.05, 2.0, 1.0).is_some(), "the first leaves touched were silent");
    }

    #[test]
    fn leaves_and_tall_stems_brush_and_turf_and_stone_do_not() {
        use primitive_shared::types::{BLOCK_GRASS, BLOCK_LEAVES};
        assert_eq!(brushes(BLOCK_LEAVES), 1.0);
        assert_eq!(brushes(BLOCK_GRASS), 0.0, "turf is walked on, not through");
        assert_eq!(brushes(BLOCK_STONE), 0.0);
        assert_eq!(brushes(BLOCK_AIR), 0.0);
        let mut chunks = flat_world(8);
        let mut chunk = chunks.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(4, 9, 4, BLOCK_LEAVES);
        chunks.insert(chunk);
        assert_eq!(foliage_around(&chunks, Vec3::new(4.5, 8.0, 4.5), 1.8), 1.0, "a leaf at chest height");
        assert_eq!(foliage_around(&chunks, Vec3::new(6.5, 8.0, 6.5), 1.8), 0.0);
    }

    fn animal(id: u32, species: Species, at: Vec3, speed: f32, hurt: f32) -> Heard {
        Heard { id: id as EntityId, species, at: at.as_dvec3(), speed, hurt, stalking: false }
    }

    /// Runs `frames` of listening with the same animals, and gathers what
    /// was said.
    fn listen(voices: &mut Voices, frames: usize, animals: &[Heard], night: bool, rng: &mut Rng) -> Vec<Utterance> {
        (0..frames).flat_map(|_| voices.hear(1.0 / 60.0, animals, (Vec3::ZERO).as_dvec3(), night, rng)).collect()
    }

    #[test]
    fn a_wounded_animal_cries_once_for_each_blow() {
        let mut voices = Voices::new();
        let mut rng = Rng::new(1);
        let at = Vec3::new(5.0, 0.0, 0.0);
        listen(&mut voices, 10, &[animal(1, Species::Boar, at, 0.0, 0.0)], false, &mut rng);
        // The flash goes up and decays over several snapshots.
        let mut cries = Vec::new();
        for hurt in [0.8f32, 0.6, 0.4, 0.2, 0.0] {
            cries.extend(voices.hear(1.0 / 60.0, &[animal(1, Species::Boar, at, 0.0, hurt)], (Vec3::ZERO).as_dvec3(), false, &mut rng));
        }
        let wounds = cries.iter().filter(|c| c.sfx == Sfx::Animal(Species::Boar, Cry::Hurt)).count();
        assert_eq!(wounds, 1, "one blow squealed {wounds} times");
    }

    #[test]
    fn an_animal_gone_just_after_a_blow_dies_aloud_and_one_that_wandered_off_does_not() {
        let at = Vec3::new(6.0, 0.0, 0.0);
        let mut rng = Rng::new(2);

        let mut voices = Voices::new();
        listen(&mut voices, 10, &[animal(7, Species::Deer, at, 0.0, 0.0)], false, &mut rng);
        listen(&mut voices, 2, &[animal(7, Species::Deer, at, 0.0, 1.0)], false, &mut rng);
        let after = listen(&mut voices, 3, &[], false, &mut rng);
        assert!(
            after.iter().any(|c| c.sfx == Sfx::Animal(Species::Deer, Cry::Death)),
            "a deer killed in front of the player died in silence"
        );

        let mut voices = Voices::new();
        listen(&mut voices, 120, &[animal(8, Species::Deer, at, 0.0, 0.0)], false, &mut rng);
        let after = listen(&mut voices, 3, &[], false, &mut rng);
        assert!(after.iter().all(|c| c.sfx != Sfx::Animal(Species::Deer, Cry::Death)), "a deer that left died");
    }

    #[test]
    fn a_herd_that_bolts_raises_one_alarm_not_twenty() {
        let mut voices = Voices::new();
        let mut rng = Rng::new(3);
        let herd: Vec<Heard> = (0..20)
            .map(|i| animal(i, Species::Zebra, Vec3::new(10.0 + i as f32, 0.0, 0.0), 0.5, 0.0))
            .collect();
        // Long enough that every one of them is past its arrival hush.
        listen(&mut voices, 300, &herd, false, &mut rng);
        let bolting: Vec<Heard> = herd
            .iter()
            .map(|h| Heard { speed: Species::Zebra.run_speed(), at: h.at + glam::DVec3::X * 0.2, ..*h })
            .collect();
        let cries = listen(&mut voices, 1, &bolting, false, &mut rng);
        let alarms = cries.iter().filter(|c| c.sfx == Sfx::Animal(Species::Zebra, Cry::Alarm)).count();
        assert_eq!(alarms, 1, "twenty zebras bolting cried {alarms} alarms");
    }

    #[test]
    fn a_boar_running_at_the_player_threatens_and_one_running_off_is_alarmed() {
        let mut rng = Rng::new(4);
        for (towards, expected) in [(true, Cry::Threat), (false, Cry::Alarm)] {
            let mut voices = Voices::new();
            let at = Vec3::new(12.0, 0.0, 0.0);
            listen(&mut voices, 300, &[animal(1, Species::Boar, at, 0.5, 0.0)], false, &mut rng);
            let step = if towards { -0.3 } else { 0.3 };
            let running = animal(1, Species::Boar, at + Vec3::X * step, Species::Boar.run_speed(), 0.0);
            let cries = listen(&mut voices, 1, &[running], false, &mut rng);
            assert!(
                cries.iter().any(|c| c.sfx == Sfx::Animal(Species::Boar, expected)),
                "a boar running {} the player did not {}",
                if towards { "at" } else { "away from" },
                expected.key()
            );
        }
    }

    /// **A calm meadow is heard now and then, not in chorus**: a flock of a
    /// dozen sheep for a minute says something, keeps within the budget, and
    /// no one sheep bleats again within a few seconds of itself.
    #[test]
    fn a_calm_flock_is_heard_now_and_then_not_in_chorus() {
        let mut voices = Voices::new();
        let mut rng = Rng::new(5);
        let flock: Vec<Heard> = (0..12)
            .map(|i| animal(i, Species::Sheep, Vec3::new(4.0 + i as f32, 0.0, 3.0), 0.3, 0.0))
            .collect();
        let dt = 1.0 / 60.0;
        let mut when: Vec<(Vec3, f32)> = Vec::new();
        for frame in 0..3600 {
            for cry in voices.hear(dt, &flock, (Vec3::ZERO).as_dvec3(), false, &mut rng) {
                assert_eq!(cry.sfx, Sfx::Animal(Species::Sheep, Cry::Idle));
                when.push((cry.at.as_vec3(), frame as f32 * dt));
            }
        }
        assert!(when.len() >= 8, "a flock of twelve said {} things in a minute", when.len());
        assert!(
            when.len() as f32 <= 60.0 * CRIES_PER_SECOND + CRY_BURST,
            "a flock of twelve said {} things in a minute",
            when.len()
        );
        for (i, (at, t)) in when.iter().enumerate() {
            for (other_at, other_t) in &when[i + 1..] {
                if at == other_at {
                    assert!(other_t - t >= 5.0, "one sheep bleated twice in {:.1} s", other_t - t);
                }
            }
        }
    }

    #[test]
    fn a_pack_circling_in_the_dark_howls_far_oftener_than_one_at_rest() {
        // Two wolves thirty-five blocks off -- past a calm call's reach -- and two
        // at ten: resting, the far pair is never heard and the near pair now
        // and then; walking the ring, both pairs are heard, and the near pair
        // several times as often. The ring is the warning, and a warning
        // that came once a minute would not be one.
        let pack = |stalking: bool| -> Vec<Heard> {
            [(0, 10.0), (1, 11.0), (2, 35.0), (3, 36.0)]
                .iter()
                .map(|&(i, x)| Heard { stalking, ..animal(i, Species::Wolf, Vec3::new(x, 0.0, i as f32), 0.3, 0.0) })
                .collect()
        };
        let howls = |stalking: bool| -> (usize, usize) {
            let mut voices = Voices::new();
            let mut rng = Rng::new(9);
            let said = listen(&mut voices, 60 * 60, &pack(stalking), true, &mut rng);
            let is_howl = |c: &&Utterance| c.sfx == Sfx::Animal(Species::Wolf, Cry::Idle);
            let near = said.iter().filter(is_howl).filter(|c| c.at.x < 20.0).count();
            let far = said.iter().filter(is_howl).filter(|c| c.at.x >= 20.0).count();
            (near, far)
        };
        let (resting_near, resting_far) = howls(false);
        let (ringing_near, ringing_far) = howls(true);
        assert_eq!(resting_far, 0, "a resting pack thirty-five blocks off was heard");
        assert!(ringing_far > 0, "a pack circling thirty-five blocks off was never heard");
        assert!(
            ringing_near >= resting_near * 3 && ringing_near >= 6,
            "a circling pack at ten blocks howled {ringing_near} times in a minute, against {resting_near} at rest"
        );
    }

    #[test]
    fn a_wolf_howls_only_after_dark() {
        let pack: Vec<Heard> = (0..4).map(|i| animal(i, Species::Wolf, Vec3::new(10.0, 0.0, i as f32), 0.3, 0.0)).collect();
        let howls = |night: bool| {
            let mut voices = Voices::new();
            let mut rng = Rng::new(6);
            listen(&mut voices, 60 * 120, &pack, night, &mut rng)
                .iter()
                .filter(|c| c.sfx == Sfx::Animal(Species::Wolf, Cry::Idle))
                .count()
        };
        assert_eq!(howls(false), 0, "a wolf howled in daylight");
        assert!(howls(true) > 0, "a pack was silent for two minutes of night");
    }

    /// A squall is gusts that leave no room between them. Scaling only
    /// how loud one is would be a volume knob on a wind that breathes
    /// at one rate whatever it is doing.
    #[test]
    fn the_wind_is_heard_oftener_and_louder_the_harder_it_blows() {
        let (calm_gap, calm_gain) = gusting(0.0);
        let (blowing_gap, blowing_gain) = gusting(1.0);
        assert!(blowing_gap < calm_gap * 0.5, "{blowing_gap:.0}s against {calm_gap:.0}s");
        assert!(blowing_gain > calm_gain * 2.0);
        // Never so often that one gust arrives on top of the last, and
        // never silent: a light air is still air moving.
        assert!(blowing_gap > 7.0, "gusts every {blowing_gap:.0} seconds run together");
        assert!(calm_gain > 0.05, "a calm is silent at {calm_gain:.2}");
        // Monotonic in between, which is the property that makes it a
        // wind rather than three cases.
        let mut last = gusting(0.0);
        for step in 1..=10 {
            let next = gusting(step as f32 / 10.0);
            assert!(next.0 < last.0 && next.1 > last.1, "the wind is not monotonic at {step}");
            last = next;
        }
    }

    /// Somewhere ordinary, in daylight, in one piece. Every test below
    /// changes one thing about it and says what that should sound like.
    fn meadow() -> Surroundings {
        Surroundings {
            health_fraction: 1.0,
            outdoors: true,
            height: 70.0,
            weather: Weather::Clear,
            night: false,
            fire_near: false,
            water_share: 0.0,
            leaf_share: 0.0,
            afloat: false,
        }
    }

    /// **Two states of the game that sound the same are one state as
    /// far as the player is concerned.** Rain and a thunderstorm were
    /// exactly that until the storm got a recipe of its own, and so
    /// were a raft at sea, a wood and a hut with a fire in it, all
    /// three of which were `Day`.
    #[test]
    fn each_place_the_music_knows_about_sounds_like_itself() {
        assert_eq!(mood_for(meadow()), Mood::Day);

        let storm = Surroundings { weather: Weather::Storm, ..meadow() };
        assert_eq!(mood_for(storm), Mood::Storm);
        let rain = Surroundings { weather: Weather::Rain, ..meadow() };
        assert_eq!(mood_for(rain), Mood::Rain);
        assert_ne!(mood_for(storm), mood_for(rain));

        let sea = Surroundings { water_share: 0.9, ..meadow() };
        assert_eq!(mood_for(sea), Mood::Sea);
        // Swimming a channel is being at sea as well, without the ring
        // of probes ever filling up.
        let swimming = Surroundings { afloat: true, water_share: 0.5, ..meadow() };
        assert_eq!(mood_for(swimming), Mood::Sea);
        // ...and a pond in a field is not.
        let pond = Surroundings { water_share: 0.2, ..meadow() };
        assert_eq!(mood_for(pond), Mood::Day);

        let wood = Surroundings { leaf_share: 0.8, ..meadow() };
        assert_eq!(mood_for(wood), Mood::Forest);

        let hut = Surroundings { fire_near: true, outdoors: false, height: 70.0, ..meadow() };
        assert_eq!(mood_for(hut), Mood::Hearth);
        // A fire out in the open is atmosphere rather than a home: the
        // crackle is already playing and the music stays where it was.
        let campfire = Surroundings { fire_near: true, ..meadow() };
        assert_eq!(mood_for(campfire), Mood::Day);
    }

    /// The order the tests above take for granted, and the two lines in
    /// it that were arguments: the sea outranks the night, and the
    /// night outranks the forest.
    #[test]
    fn the_more_specific_place_is_the_one_that_plays() {
        let dying = Surroundings {
            health_fraction: 0.1,
            outdoors: false,
            height: 20.0,
            weather: Weather::Storm,
            fire_near: true,
            ..meadow()
        };
        assert_eq!(mood_for(dying), Mood::Peril, "nearly dying beats everything");

        // A fire with a roof over it beats being underground: a hearth
        // is a place somebody made.
        let camp_in_a_cave = Surroundings {
            outdoors: false,
            height: 20.0,
            fire_near: true,
            ..meadow()
        };
        assert_eq!(mood_for(camp_in_a_cave), Mood::Hearth);

        // ...and a cave without one beats the weather outside it.
        let cave = Surroundings {
            outdoors: false,
            height: 20.0,
            weather: Weather::Storm,
            ..meadow()
        };
        assert_eq!(mood_for(cave), Mood::Cave);

        let night_crossing = Surroundings { night: true, water_share: 0.9, ..meadow() };
        assert_eq!(mood_for(night_crossing), Mood::Sea, "a crossing is still a crossing");

        let wood_at_night = Surroundings { night: true, leaf_share: 0.9, ..meadow() };
        assert_eq!(mood_for(wood_at_night), Mood::Night, "the dark is what changed");
    }

    /// A mood must survive a few frames of being true before it is sent,
    /// or the composer spends the whole session fading.
    #[test]
    fn a_mood_has_to_hold_before_it_counts() {
        let audio = Audio::silent();
        let mut scape = Soundscape::new();
        scape.mood = Mood::Day;
        scape.push_mood(&audio, Mood::Cave, 1.0);
        assert_eq!(scape.mood, Mood::Day, "one second was enough");
        scape.push_mood(&audio, Mood::Cave, 1.0);
        scape.push_mood(&audio, Mood::Cave, 1.5);
        assert_eq!(scape.mood, Mood::Cave);

        // ...and a flicker resets the clock rather than accumulating.
        scape.push_mood(&audio, Mood::Day, 2.0);
        scape.push_mood(&audio, Mood::Night, 0.1);
        scape.push_mood(&audio, Mood::Day, 2.0);
        assert_eq!(scape.mood, Mood::Cave);
    }

    fn summer_night() -> Night {
        Night { sun: -0.6, warmth: 20.0, biome: Biome::Plains, wet: false, shelter: Shelter::Open, at_sea: false }
    }

    #[test]
    fn crickets_sing_on_a_warm_night_in_the_grass() {
        assert!(cricket_chorus(&summer_night()) > 0.9);
        for biome in [Biome::Steppe, Biome::Savanna] {
            assert!(cricket_chorus(&Night { biome, ..summer_night() }) > 0.9, "{biome:?} is silent");
        }
    }

    #[test]
    fn crickets_are_silent_by_day_in_the_cold_in_rain_underground_and_at_sea() {
        let night = summer_night();
        let silent = [
            ("noon", Night { sun: 0.9, ..night }),
            ("a frosty night", Night { warmth: 2.0, ..night }),
            ("a winter night", Night { warmth: -10.0, ..night }),
            ("rain", Night { wet: true, ..night }),
            ("a mine", Night { shelter: Shelter::Enclosed, ..night }),
            ("the sea", Night { at_sea: true, ..night }),
            ("the ocean", Night { biome: Biome::Ocean, ..night }),
            ("the tundra", Night { biome: Biome::Tundra, ..night }),
        ];
        for (what, n) in silent {
            assert_eq!(cricket_chorus(&n), 0.0, "crickets sang in {what}");
        }
    }

    #[test]
    fn a_wood_a_desert_and_a_hut_hear_fewer_crickets_than_a_meadow() {
        let meadow = cricket_chorus(&summer_night());
        for (what, n) in [
            ("a forest", Night { biome: Biome::Forest, ..summer_night() }),
            ("a desert", Night { biome: Biome::Desert, ..summer_night() }),
            ("a hut", Night { shelter: Shelter::Roofed, ..summer_night() }),
        ] {
            let there = cricket_chorus(&n);
            assert!(there > 0.0 && there < meadow * 0.6, "{what} heard {there:.2} against a meadow's {meadow:.2}");
        }
    }

    #[test]
    fn the_crickets_fade_in_through_dusk_rather_than_switching_on() {
        let at = |sun: f32| cricket_chorus(&Night { sun, ..summer_night() });
        assert_eq!(at(0.2), 0.0, "singing in the evening sun");
        let dusk = at(0.0);
        assert!(dusk > 0.1 && dusk < 0.9, "at sunset the chorus is {dusk:.2}, not a fade");
        assert!(at(-0.2) > 0.99);
        // ...and a mild night is a thinner chorus than a hot one.
        let mild = cricket_chorus(&Night { warmth: 11.0, ..summer_night() });
        assert!(mild > 0.0 && mild < 0.6);
    }

    #[test]
    fn the_crickets_hush_round_a_moving_player_and_come_back_when_they_stand_still() {
        let dt = 1.0 / 60.0;
        let mut hush = CRICKET_NEAREST;
        hush = cricket_hush(hush, true, dt);
        assert!(hush >= 6.0, "walking through the grass hushed only {hush:.1} blocks");
        for _ in 0..60 {
            hush = cricket_hush(hush, false, dt);
        }
        assert!(hush > 5.0, "the crickets trusted a player who stopped a second ago");
        for _ in 0..(60 * 10) {
            hush = cricket_hush(hush, false, dt);
        }
        assert_eq!(hush, CRICKET_NEAREST, "ten seconds of standing still and the near ones are still quiet");
    }

    #[test]
    fn each_pace_of_a_horse_plays_its_own_hoofbeat() {
        use primitive_shared::horse::{GALLOP, TROT, WALK};
        assert_eq!(pace_of(0.0), None);
        assert_eq!(pace_of(0.3), None, "a horse shifting its weight is striding");
        assert_eq!(pace_of(WALK), Some(Gait::Walk));
        assert_eq!(pace_of(TROT), Some(Gait::Trot));
        assert_eq!(pace_of(GALLOP), Some(Gait::Gallop));
        assert_eq!(pace_of(GALLOP * 0.9), Some(Gait::Gallop), "a gallop slowing a little is still a gallop");

        for (speed, gait) in [(WALK, Gait::Walk), (TROT, Gait::Trot), (GALLOP, Gait::Gallop)] {
            for footing in [Footing::Soft, Footing::Hard] {
                let mut hooves = Hoofbeats::new();
                let horse = Hoof { id: 7, at: glam::DVec3::ZERO, speed, footing };
                let beats = hooves.hear(1.0 / 60.0, &[horse]);
                assert_eq!(beats.len(), 1, "a horse setting off at {speed} played {} pieces", beats.len());
                assert_eq!(beats[0].sfx, Sfx::Hoofs(gait, footing));
            }
        }
    }

    #[test]
    fn hoofbeats_follow_one_another_without_a_gap_or_a_pile_up() {
        let dt = 1.0 / 60.0;
        let mut hooves = Hoofbeats::new();
        let horse = Hoof { id: 1, at: glam::DVec3::ZERO, speed: primitive_shared::horse::TROT, footing: Footing::Soft };
        let mut starts = Vec::new();
        for frame in 0..(60 * 6) {
            if !hooves.hear(dt, &[horse]).is_empty() {
                starts.push(frame as f32 * dt);
            }
        }
        let piece = bank::hoof_piece_seconds(Gait::Trot);
        for pair in starts.windows(2) {
            let gap = pair[1] - pair[0];
            assert!((gap - (piece - 0.03)).abs() < 0.04, "trot pieces {gap:.2} s apart and {piece} s long");
        }
        // Standing: nothing. Breaking into a gallop: at once.
        assert!(hooves.hear(dt, &[Hoof { speed: 0.0, ..horse }]).is_empty());
        let gallop = Hoof { speed: primitive_shared::horse::GALLOP, ..horse };
        hooves.hear(dt, &[Hoof { speed: primitive_shared::horse::TROT, ..horse }]);
        let broke = hooves.hear(dt, &[gallop]);
        assert_eq!(broke.len(), 1, "a horse breaking into a gallop waited for the trot to finish");
        assert_eq!(broke[0].sfx, Sfx::Hoofs(Gait::Gallop, Footing::Soft));
    }

    #[test]
    fn stone_and_boards_ring_under_a_hoof_and_earth_does_not() {
        assert_eq!(Footing::of(Material::Stone), Footing::Hard);
        assert_eq!(Footing::of(Material::Wood), Footing::Hard);
        for soft in [Material::Dirt, Material::Grass, Material::Sand, Material::Snow, Material::Gravel] {
            assert_eq!(Footing::of(soft), Footing::Soft, "{soft:?}");
        }
    }

    #[test]
    fn a_horse_snorts_at_a_player_walking_up_to_it_and_not_again_at_once() {
        let mut voices = Voices::new();
        let mut rng = Rng::new(3);
        let dt = 1.0 / 60.0;
        let far = [animal(1, Species::Horse, Vec3::new(10.0, 0.0, 0.0), 0.0, 0.0)];
        let close = [animal(1, Species::Horse, Vec3::new(2.0, 0.0, 0.0), 0.0, 0.0)];
        // Seen from a distance first. Half a second in all, well inside the
        // second before the first calm call is even considered, so any snort
        // here is the approach.
        voices.hear(dt, &far, glam::DVec3::ZERO, false, &mut rng);
        let said = voices.hear(dt, &close, glam::DVec3::ZERO, false, &mut rng);
        assert!(
            said.iter().any(|u| u.sfx == Sfx::Animal(Species::Horse, Cry::Idle)),
            "a horse walked up to said nothing"
        );
        // Standing beside it is not walking up to it again.
        let after: Vec<Utterance> = (0..30).flat_map(|_| voices.hear(dt, &close, glam::DVec3::ZERO, false, &mut rng)).collect();
        assert!(after.is_empty(), "a horse snorted {} more times at somebody standing still", after.len());
        // A sheep walked up to says nothing for it.
        let mut voices = Voices::new();
        voices.hear(dt, &[animal(2, Species::Sheep, Vec3::new(10.0, 0.0, 0.0), 0.0, 0.0)], glam::DVec3::ZERO, false, &mut rng);
        let sheep = voices.hear(dt, &[animal(2, Species::Sheep, Vec3::new(2.0, 0.0, 0.0), 0.0, 0.0)], glam::DVec3::ZERO, false, &mut rng);
        assert!(sheep.is_empty());
    }

    // ------------------------------------------- the country's own bed

    /// Noon in the middle of summer on the steppe: what the cicadas are
    /// for.
    fn summer_noon() -> Air {
        Air {
            sun: 0.9,
            morning: false,
            warmth: 28.0,
            thaw: 0.0,
            biome: Biome::Steppe,
            wet: false,
            shelter: Shelter::Open,
            at_sea: false,
        }
    }

    #[test]
    fn the_steppe_rings_with_cicadas_at_noon_in_summer_and_is_silent_in_winter() {
        // The season arrives as a temperature and nothing else -- so
        // this is the same place, the same hour, and the only thing
        // moved is what the year did to the air.
        let summer = cicada_chorus(&summer_noon());
        let winter = cicada_chorus(&Air { warmth: 4.0, ..summer_noon() });
        assert!(summer > 0.9, "a summer steppe at noon heard {summer:.2}");
        assert_eq!(winter, 0.0, "the steppe sang in the snow");
    }

    #[test]
    fn cicadas_and_crickets_hand_over_at_dusk_rather_than_singing_together() {
        // The two beds are mirrors of each other in the sun's height: a
        // cicada is the heat of the day and a cricket is the evening. If
        // both were loud at once a July night would be twice as noisy as
        // a July afternoon, which is the wrong way round.
        let warm = 22.0;
        let air = |sun| Air { sun, warmth: warm, biome: Biome::Plains, ..summer_noon() };
        let night = |sun| Night {
            sun,
            warmth: warm,
            biome: Biome::Plains,
            wet: false,
            shelter: Shelter::Open,
            at_sea: false,
        };
        assert!(cicada_chorus(&air(0.9)) > cricket_chorus(&night(0.9)), "noon belonged to the crickets");
        assert!(cricket_chorus(&night(-0.5)) > cicada_chorus(&air(-0.5)), "midnight belonged to the cicadas");
        // ...and at the horizon neither is at full, so the handover is a
        // fade and not a swap.
        assert!(cicada_chorus(&air(0.0)) < 0.2 && cricket_chorus(&night(0.0)) < 0.8);
    }

    #[test]
    fn cicadas_are_silent_in_the_rain_underground_at_sea_and_on_the_ice() {
        let noon = summer_noon();
        for (what, air) in [
            ("rain", Air { wet: true, ..noon }),
            ("a mine", Air { shelter: Shelter::Enclosed, ..noon }),
            ("the open sea", Air { at_sea: true, ..noon }),
            ("the peaks", Air { biome: Biome::SnowyPeaks, ..noon }),
            ("the tundra", Air { biome: Biome::Tundra, ..noon }),
            ("midnight", Air { sun: -0.8, ..noon }),
        ] {
            assert_eq!(cicada_chorus(&air), 0.0, "cicadas sang in {what}");
        }
    }

    #[test]
    fn the_birds_sing_at_either_end_of_the_day_and_hardest_at_dawn() {
        // The one thing this bed tells a player who has lost the clock
        // underground: which end of the day they have come up into.
        let spring = Air {
            sun: 0.0,
            morning: true,
            warmth: 11.0,
            thaw: 1.0,
            biome: Biome::Forest,
            wet: false,
            shelter: Shelter::Open,
            at_sea: false,
        };
        let dawn = dawn_chorus(&spring);
        let dusk = dawn_chorus(&Air { morning: false, ..spring });
        let noon = dawn_chorus(&Air { sun: 0.9, ..spring });
        let midnight = dawn_chorus(&Air { sun: -0.9, ..spring });
        assert!(dawn > 0.9, "a spring dawn in a wood heard {dawn:.2}");
        assert!(dusk > 0.0 && dusk < dawn * 0.7, "dusk sang {dusk:.2} against dawn's {dawn:.2}");
        assert_eq!(noon, 0.0, "the chorus was still going at noon");
        assert_eq!(midnight, 0.0, "the chorus was going at midnight");
    }

    #[test]
    fn a_frozen_morning_has_no_dawn_chorus() {
        // Winter's silence, stated without the word winter: what stops
        // the birds is the frost, which is what the season *is*.
        let frozen = Air {
            sun: 0.0,
            morning: true,
            warmth: 1.0,
            thaw: 0.0,
            biome: Biome::Taiga,
            wet: false,
            shelter: Shelter::Open,
            at_sea: false,
        };
        assert_eq!(dawn_chorus(&frozen), 0.0);
        // ...and the same morning eight degrees warmer is not silent.
        assert!(dawn_chorus(&Air { warmth: 11.0, ..frozen }) > 0.0);
    }

    #[test]
    fn a_thaw_drips_and_a_wet_autumn_at_the_same_temperature_does_not() {
        // The whole reason `thawing` exists: no temperature can tell a
        // March morning from a November one, and a drip belongs to only
        // one of them.
        use primitive_shared::season::{MIDSUMMER_DAY_OF_YEAR, WORLD_OPENS_ON_DAY, YEAR_DAYS};
        let quarter = YEAR_DAYS / 4.0;
        // Midwinter is half a year from midsummer; a quarter of a year
        // after it is the spring equinox, and a quarter before it is the
        // autumn one. Converted out of the year's own days into a
        // world's, which is what the game counts.
        let midwinter = MIDSUMMER_DAY_OF_YEAR + YEAR_DAYS / 2.0 - WORLD_OPENS_ON_DAY;
        let spring = thawing(midwinter + quarter);
        let autumn = thawing(midwinter - quarter);
        assert!(spring > 0.95, "the spring equinox thawed at {spring:.2}");
        assert_eq!(autumn, 0.0, "the year was thawing in the autumn");

        let air = |thaw| Air {
            sun: 0.4,
            morning: true,
            warmth: 3.0,
            thaw,
            biome: Biome::Taiga,
            wet: false,
            shelter: Shelter::Roofed,
            at_sea: false,
        };
        assert!(eaves_drip(&air(spring)) > 0.5, "nothing ran off the eaves in a thaw");
        assert_eq!(eaves_drip(&air(autumn)), 0.0, "the eaves dripped in October");
    }

    #[test]
    fn nothing_drips_while_it_is_still_frozen_or_once_the_snow_has_gone() {
        let thawing_taiga = Air {
            sun: 0.4,
            morning: true,
            warmth: 3.0,
            thaw: 1.0,
            biome: Biome::Taiga,
            wet: false,
            shelter: Shelter::Roofed,
            at_sea: false,
        };
        assert!(eaves_drip(&thawing_taiga) > 0.0);
        for (what, air) in [
            ("a hard frost", Air { warmth: -4.0, ..thawing_taiga }),
            ("a warm week later", Air { warmth: 14.0, ..thawing_taiga }),
            ("a desert", Air { biome: Biome::Desert, ..thawing_taiga }),
            ("rain", Air { wet: true, ..thawing_taiga }),
            ("a mine", Air { shelter: Shelter::Enclosed, ..thawing_taiga }),
        ] {
            assert_eq!(eaves_drip(&air), 0.0, "the eaves dripped in {what}");
        }
    }

    #[test]
    fn no_bed_ever_jumps_however_fast_the_world_changes() {
        // **The test the whole `Level` type exists for.** Every one of
        // these functions is a product of things a player can change in
        // one step -- walk under a roof, cross a biome edge, step out of
        // the rain -- and a gain that followed them frame for frame
        // would click. The level is what stands between the two.
        let dt = 1.0 / 60.0;
        let mut level = Level::default();
        let mut last = 0.0;
        // Full on, full off, full on again, as abruptly as anything can
        // ask.
        for frame in 0..600 {
            let wanted = if (frame / 100) % 2 == 0 { 1.0 } else { 0.0 };
            let now = level.ease(wanted, dt);
            assert!(
                (now - last).abs() <= BED_SLEW * dt + 1e-6,
                "the bed jumped from {last} to {now}"
            );
            last = now;
        }
        // ...and it actually arrives, rather than approaching for ever:
        // a bed at a millionth of full gain is a voice the mixer has
        // spent on silence.
        for _ in 0..600 {
            level.ease(0.0, dt);
        }
        assert_eq!(level.ease(0.0, dt), 0.0, "the bed never reached silence");
    }

    #[test]
    fn no_bed_ever_jumps_over_a_whole_year() {
        // The season turning must not be audible as an event. `Season`
        // is a step function and none of these beds reads it: they read
        // a temperature and `thawing`, both of which are sines. Walked
        // day by day through four years, with the level easing at its
        // own rate, the envelope has to stay continuous over every
        // boundary -- including the one where `Season::at` changes its
        // answer.
        let dt = 1.0 / 60.0;
        let mut level = Level::default();
        let mut last = 0.0;
        let mut worst = 0.0f32;
        let mut seen_a_thaw = false;
        let mut seen_a_dry_spell = false;
        // A tenth of a day at a time, a frame of easing each: fast
        // enough that a step in the underlying function would show as a
        // jump of the whole step.
        for tenth in 0..1600 {
            let world_time = tenth as f32 * 0.1;
            let air = Air {
                sun: 0.4,
                morning: true,
                // The year's own swing, so the temperature moves the way
                // the world moves it.
                warmth: 8.0 + primitive_shared::season::ambient_offset_c(world_time),
                thaw: thawing(world_time),
                biome: Biome::Taiga,
                wet: false,
                shelter: Shelter::Roofed,
                at_sea: false,
            };
            let wanted = eaves_drip(&air);
            seen_a_thaw |= wanted > 0.3;
            seen_a_dry_spell |= wanted == 0.0;
            let now = level.ease(wanted, dt);
            worst = worst.max((now - last).abs());
            last = now;
        }
        assert!(seen_a_thaw, "four years went by without a thaw");
        assert!(seen_a_dry_spell, "the eaves dripped all year round");
        assert!(
            worst <= BED_SLEW * dt + 1e-6,
            "the year turned with a jump of {worst} in the envelope"
        );
    }

    #[test]
    fn a_roof_muffles_every_bed_by_the_same_amount_and_a_mine_silences_them_all() {
        // One answer to "what is over my head", not three -- three
        // copies is three chances to disagree about what a roof is.
        let noon = summer_noon();
        let open = cicada_chorus(&noon);
        let roofed = cicada_chorus(&Air { shelter: Shelter::Roofed, ..noon });
        assert!(roofed > 0.0 && roofed < open * 0.6);
        assert_eq!(heard_through(Shelter::Enclosed), 0.0);
        assert_eq!(heard_through(Shelter::Canopy), heard_through(Shelter::Open));
    }

    // ------------------------------------------------ the blow and the step

    #[test]
    fn a_heavier_tool_knocks_slower_and_lower_than_a_light_one() {
        use crate::logic::hand::dig_seconds;
        use primitive_shared::types::{BLOCK_FLINT_KNIFE, BLOCK_IRON_PICKAXE};
        let flint = Some(BLOCK_FLINT_KNIFE);
        let iron = Some(BLOCK_IRON_PICKAXE);
        assert!(
            dig_seconds(iron) > dig_seconds(flint),
            "an iron pick swung at a flint knife's rate"
        );
        assert!(
            dig_seconds(flint) > dig_seconds(None),
            "a tool swung at a fist's rate"
        );
        let (_, flint_gain, flint_pitch) = tool_voice(flint).expect("a flint knife has a voice");
        let (_, iron_gain, iron_pitch) = tool_voice(iron).expect("an iron pick has a voice");
        assert!(iron_gain > flint_gain, "the heavier head was the quieter one");
        assert!(iron_pitch < flint_pitch, "the heavier head was the higher one");
    }

    #[test]
    fn the_blow_carries_the_tool_as_well_as_the_block() {
        use primitive_shared::types::{BLOCK_COPPER_AXE, BLOCK_FLINT_KNIFE, BLOCK_STONE};
        // The failure this exists to stop: a flint axe, a copper one and
        // a fist all made the same noise against the same log, which is
        // the whole of what a tool is, gone.
        let flint = tool_voice(Some(BLOCK_FLINT_KNIFE)).expect("a flint knife has a voice");
        let copper = tool_voice(Some(BLOCK_COPPER_AXE)).expect("a copper axe has a voice");
        assert_eq!(flint.0, Material::Stone, "a knapped edge is not stone");
        assert_eq!(copper.0, Material::Metal, "a cast head is not metal");
        // A fist and a block in the hand are not striking heads: setting
        // a slab of stone against a rock face is not a blow with it.
        assert!(tool_voice(None).is_none(), "an empty hand rang");
        assert!(tool_voice(Some(BLOCK_STONE)).is_none(), "a carried block rang");
        // ...and the tool never drowns the block. The rock is what the
        // player is working on; the tool is under it.
        assert!(copper.1 < 0.4, "the tool was as loud as the block at {}", copper.1);
    }

    #[test]
    fn a_creeping_step_is_far_quieter_than_a_sprinting_one() {
        // There is no crouch key in this game, so going slowly *is*
        // creeping, and this ratio is the whole of the mechanic. Stated
        // on the numbers the footfall is played at rather than through a
        // player and a chunk manager.
        let effort = |speed: f32| {
            (speed / primitive_shared::animals::NOMINAL_SPRINT_SPEED).clamp(CREEP_EFFORT, 1.15)
        };
        let creep = effort(0.8);
        let sprint = effort(primitive_shared::animals::NOMINAL_SPRINT_SPEED);
        assert_eq!(creep, CREEP_EFFORT, "a creep was not at the floor");
        assert!(
            sprint > creep * 3.5,
            "a sprint is {sprint:.2} against a creep's {creep:.2}, which nobody would hear as an effort"
        );
    }
}
